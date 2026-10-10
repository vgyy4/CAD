//! Projects on disk (ADR 0005, ADR 0007).
//!
//! - [`encode`]/[`decode`] map a [`History`] to the `.gus` container and back.
//! - [`Project`] owns a project's on-disk life: lock, saves (durable, with a
//!   last-known-good copy), recovery snapshots (one two-slot file per project), damage
//!   handling on open, and clean close.

use gusset_history::{History, HistoryRecords, ObjectKind};
use gusset_store::container::{self, ContainerWriter, Kind, SaveInfo};
use gusset_store::fs::RealFs;
use gusset_store::lock::{Acquired, ProjectLock};
use gusset_store::paths::{DataDirs, ProjectId, RiskyStorage};
use gusset_store::{Hash as StoreHash, durable, housekeeping, recovery};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Project identity and save bookkeeping, stored in the `meta` root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMeta {
    pub id: ProjectId,
    /// Increases with every save of the project file.
    pub save_seq: u64,
    /// For recovery images: the `save_seq` of the project file they started from.
    pub base_seq: u64,
    pub created: u64,
}

impl ProjectMeta {
    fn to_text(&self) -> String {
        format!(
            "project 1\nid {}\nsave_seq {}\nbase_seq {}\ncreated {}\n",
            self.id.to_hex(),
            self.save_seq,
            self.base_seq,
            self.created
        )
    }

    fn parse(t: &str) -> Option<ProjectMeta> {
        let mut lines = t.lines();
        if lines.next()? != "project 1" {
            return None;
        }
        let mut get = |k: &str| lines.next()?.strip_prefix(k)?.strip_prefix(' ').map(str::to_string);
        Some(ProjectMeta {
            id: ProjectId::parse(&get("id")?)?,
            save_seq: get("save_seq")?.parse().ok()?,
            base_seq: get("base_seq")?.parse().ok()?,
            created: get("created")?.parse().ok()?,
        })
    }
}

#[derive(Debug)]
pub enum ProjectError {
    Io {
        path: PathBuf,
        message: String,
    },
    /// The file is damaged; the message says how.
    Damaged(String),
    /// Opened elsewhere by a running Gusset instance.
    Locked {
        path: PathBuf,
        holder: String,
    },
    Write(String),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::Io { path, message } => write!(f, "could not access {}: {message}", path.display()),
            ProjectError::Damaged(m) => write!(f, "the project file is damaged: {m}"),
            ProjectError::Locked { path, holder } => {
                write!(f, "{} is already open in another Gusset window ({holder})", path.display())
            }
            ProjectError::Write(m) => write!(f, "saving failed: {m}"),
        }
    }
}

impl std::error::Error for ProjectError {}

fn kind_of(k: ObjectKind) -> Kind {
    match k {
        ObjectKind::Feature => Kind::FEATURE,
        ObjectKind::Params => Kind::PARAMS,
        ObjectKind::Snapshot => Kind::SNAPSHOT,
        ObjectKind::Blob => Kind::BLOB,
    }
}

/// Serializes a history into a complete `.gus` image.
pub fn encode(history: &History, meta: &ProjectMeta) -> Result<Vec<u8>, ProjectError> {
    let r = history.to_records();
    let w = |e: container::WriteError| ProjectError::Write(e.to_string());
    let mut cw = ContainerWriter::new();
    for (k, _h, bytes) in &r.objects {
        cw.add(kind_of(*k), bytes).map_err(w)?;
    }
    let nodes = cw.add(Kind::NODE, r.nodes.as_bytes()).map_err(w)?;
    let branches = cw.add(Kind::BRANCHES, r.branches.as_bytes()).map_err(w)?;
    let events = cw.add(Kind::EVENTS, r.events.as_bytes()).map_err(w)?;
    cw.set_root("nodes", nodes.as_bytes()).map_err(w)?;
    cw.set_root("branches", branches.as_bytes()).map_err(w)?;
    cw.set_root("events", events.as_bytes()).map_err(w)?;
    cw.set_root("meta", meta.to_text().as_bytes()).map_err(w)?;
    cw.finish(&SaveInfo { save_seq: meta.save_seq, created_unix: meta.created, app_version: APP_VERSION }).map_err(w)
}

/// Parses and fully verifies a `.gus` image.
pub fn decode(bytes: &[u8]) -> Result<(History, ProjectMeta), ProjectError> {
    let c = container::open(bytes).map_err(|e| ProjectError::Damaged(e.to_string()))?;
    let meta = c
        .root("meta")
        .and_then(|m| std::str::from_utf8(m).ok())
        .and_then(ProjectMeta::parse)
        .ok_or_else(|| ProjectError::Damaged("project metadata is missing".into()))?;
    let text_root = |name: &str| -> Result<String, ProjectError> {
        let h = c
            .root(name)
            .and_then(StoreHash::from_slice)
            .ok_or_else(|| ProjectError::Damaged(format!("the {name} record is missing")))?;
        let b = c
            .get(&h)
            .map_err(|e| ProjectError::Damaged(e.to_string()))?
            .ok_or_else(|| ProjectError::Damaged(format!("the {name} record is missing")))?;
        String::from_utf8(b).map_err(|_| ProjectError::Damaged(format!("the {name} record is not text")))
    };
    let mut objects = Vec::new();
    for rec in c.records() {
        let kind = match rec.kind {
            k if k == Kind::FEATURE => ObjectKind::Feature,
            k if k == Kind::PARAMS => ObjectKind::Params,
            k if k == Kind::SNAPSHOT => ObjectKind::Snapshot,
            k if k == Kind::BLOB => ObjectKind::Blob,
            _ => continue,
        };
        let bytes = c
            .get(&rec.hash)
            .map_err(|e| ProjectError::Damaged(e.to_string()))?
            .ok_or_else(|| ProjectError::Damaged("a record vanished".into()))?;
        objects.push((kind, gusset_history::Hash(*rec.hash.as_bytes()), Arc::<[u8]>::from(bytes)));
    }
    let recs = HistoryRecords {
        objects,
        nodes: text_root("nodes")?,
        branches: text_root("branches")?,
        events: text_root("events")?,
    };
    let h = History::from_records(&recs).map_err(ProjectError::Damaged)?;
    Ok((h, meta))
}

/// Best-effort salvage of a damaged file: every intact record is recovered; if the
/// history graph survived, the full history comes back, otherwise the newest intact
/// document snapshot becomes a fresh project.
pub fn salvage(bytes: &[u8], now: u64) -> Option<History> {
    let s = container::salvage(bytes);
    let mut objects = Vec::new();
    let mut texts: Vec<(Kind, String)> = Vec::new();
    for r in &s.records {
        let kind = match r.kind {
            k if k == Kind::FEATURE => Some(ObjectKind::Feature),
            k if k == Kind::PARAMS => Some(ObjectKind::Params),
            k if k == Kind::SNAPSHOT => Some(ObjectKind::Snapshot),
            k if k == Kind::BLOB => Some(ObjectKind::Blob),
            _ => None,
        };
        match kind {
            Some(k) => objects.push((k, gusset_history::Hash(*r.hash.as_bytes()), Arc::<[u8]>::from(r.data.clone()))),
            None => {
                if let Ok(t) = String::from_utf8(r.data.clone()) {
                    texts.push((r.kind, t));
                }
            }
        }
    }
    let find = |k: Kind| texts.iter().find(|(kk, _)| *kk == k).map(|(_, t)| t.clone());
    if let (Some(nodes), Some(branches), Some(events)) = (find(Kind::NODE), find(Kind::BRANCHES), find(Kind::EVENTS)) {
        let recs = HistoryRecords { objects: objects.clone(), nodes, branches, events };
        if let Ok(h) = History::from_records(&recs) {
            return Some(h);
        }
    }
    // Fall back: any snapshot whose objects are all intact.
    let mut store = gusset_history::ObjectStore::default();
    for (k, h, b) in &objects {
        let _ = store.insert_verified(*k, *h, b.clone());
    }
    let docs: Vec<gusset_doc::Document> = objects
        .iter()
        .filter(|(k, _, _)| *k == ObjectKind::Snapshot)
        .filter_map(|(_, h, _)| store.document(*h).ok())
        .collect();
    let best = docs.into_iter().max_by_key(|d| d.features.len())?;
    Some(History::new(&best, &gusset_history::Ctx::user(now)))
}

/// What opening found.
#[derive(Debug)]
pub enum OpenReport {
    Clean,
    /// A recovery snapshot newer than the last save exists (the app did not close
    /// cleanly). The caller asks: recover / open saved / keep both.
    RecoveryAvailable {
        recovered: Box<History>,
        recovery_seq: u64,
    },
    /// The file was damaged; `source` says what was offered instead.
    Damaged {
        reason: String,
        source: DamageSource,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSource {
    RecoverySnapshot,
    LastKnownGood,
    Salvaged,
}

/// An open project on disk.
#[derive(Debug)]
pub struct Project {
    path: Option<PathBuf>,
    pub meta: ProjectMeta,
    dirs: DataDirs,
    lock: Option<ProjectLock>,
    /// Network share / cloud folder / FAT media warning (shown once).
    pub risky: Option<RiskyStorage>,
    pub took_over_stale_lock: bool,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Project {
    /// A new, never-saved project.
    pub fn create(dirs: DataDirs) -> Result<Project, ProjectError> {
        dirs.ensure().map_err(|e| ProjectError::Io { path: dirs.data_dir().to_path_buf(), message: e.to_string() })?;
        let id =
            ProjectId::new_random().map_err(|e| ProjectError::Io { path: PathBuf::new(), message: e.to_string() })?;
        let meta = ProjectMeta { id, save_seq: 0, base_seq: 0, created: now_unix() };
        let mut p = Project { path: None, meta, dirs, lock: None, risky: None, took_over_stale_lock: false };
        p.lock()?;
        Ok(p)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn recovery_path(&self) -> PathBuf {
        self.dirs.recovery_file(&self.meta.id)
    }

    fn lock(&mut self) -> Result<(), ProjectError> {
        let lp = self.dirs.lock_file(&self.meta.id);
        match ProjectLock::acquire(&lp) {
            Ok((l, acq)) => {
                self.took_over_stale_lock = matches!(acq, Acquired::TookOverStale(_));
                self.lock = Some(l);
                Ok(())
            }
            Err(e) => Err(ProjectError::Locked { path: self.path.clone().unwrap_or_default(), holder: e.to_string() }),
        }
    }

    /// Opens a project file, handling damage and pending recovery (ADR 0007).
    pub fn open(path: &Path, dirs: DataDirs) -> Result<(Project, History, OpenReport), ProjectError> {
        dirs.ensure().map_err(|e| ProjectError::Io { path: dirs.data_dir().to_path_buf(), message: e.to_string() })?;
        let io = |e: std::io::Error| ProjectError::Io { path: path.to_path_buf(), message: e.to_string() };
        let bytes = std::fs::read(path).map_err(io)?;
        let risky = gusset_store::paths::classify(path);
        match decode(&bytes) {
            Ok((history, meta)) => {
                let mut p = Project {
                    path: Some(path.to_path_buf()),
                    meta,
                    dirs,
                    lock: None,
                    risky,
                    took_over_stale_lock: false,
                };
                p.lock()?;
                let _ = housekeeping::register(&p.dirs, p.meta.id, path);
                let report = match recovery::read_newest(&RealFs::default(), &p.recovery_path(), &container::verify) {
                    Ok(Some(snap)) => match decode(&snap.payload) {
                        Ok((rh, rm)) if rm.base_seq >= p.meta.save_seq => {
                            OpenReport::RecoveryAvailable { recovered: Box::new(rh), recovery_seq: snap.seq }
                        }
                        _ => OpenReport::Clean,
                    },
                    _ => OpenReport::Clean,
                };
                Ok((p, history, report))
            }
            Err(e) => {
                let reason = e.to_string();
                // Offer, in order: recovery snapshot, last-known-good, salvage.
                let salvaged_meta = container::open(&bytes).ok().and_then(|c| {
                    c.root("meta").and_then(|m| std::str::from_utf8(m).ok()).and_then(ProjectMeta::parse)
                });
                let meta_guess = salvaged_meta.or_else(|| {
                    let reg = housekeeping::Registry::load(&dirs.registry_file()).ok()?;
                    let id = reg.find(path)?;
                    Some(ProjectMeta { id, save_seq: 0, base_seq: 0, created: now_unix() })
                });
                if let Some(meta) = &meta_guess {
                    if let Ok(Some(snap)) =
                        recovery::read_newest(&RealFs::default(), &dirs.recovery_file(&meta.id), &container::verify)
                        && let Ok((h, m)) = decode(&snap.payload)
                    {
                        let mut p = Project {
                            path: Some(path.to_path_buf()),
                            meta: m,
                            dirs,
                            lock: None,
                            risky,
                            took_over_stale_lock: false,
                        };
                        p.lock()?;
                        return Ok((p, h, OpenReport::Damaged { reason, source: DamageSource::RecoverySnapshot }));
                    }
                    if let Ok(lkg) = std::fs::read(dirs.lkg_file(&meta.id))
                        && let Ok((h, m)) = decode(&lkg)
                    {
                        let mut p = Project {
                            path: Some(path.to_path_buf()),
                            meta: m,
                            dirs,
                            lock: None,
                            risky,
                            took_over_stale_lock: false,
                        };
                        p.lock()?;
                        return Ok((p, h, OpenReport::Damaged { reason, source: DamageSource::LastKnownGood }));
                    }
                }
                let h = salvage(&bytes, now_unix())
                    .ok_or_else(|| ProjectError::Damaged(format!("{reason}; nothing could be salvaged")))?;
                // Salvage goes into a *new* project (never overwrite the damaged file blindly).
                let mut p = Project::create(dirs)?;
                p.risky = risky;
                Ok((p, h, OpenReport::Damaged { reason, source: DamageSource::Salvaged }))
            }
        }
    }

    /// Saves to the current path (durably, keeping a last-known-good copy).
    pub fn save(&mut self, history: &History) -> Result<(), ProjectError> {
        let path =
            self.path.clone().ok_or_else(|| ProjectError::Write("the project has no file yet; use Save As".into()))?;
        self.save_to(&path, history)
    }

    /// Saves to a new path (Save As).
    pub fn save_as(&mut self, path: &Path, history: &History) -> Result<(), ProjectError> {
        self.save_to(path, history)?;
        self.path = Some(path.to_path_buf());
        self.risky = gusset_store::paths::classify(path);
        Ok(())
    }

    fn save_to(&mut self, path: &Path, history: &History) -> Result<(), ProjectError> {
        let mut meta = self.meta.clone();
        meta.save_seq += 1;
        meta.base_seq = meta.save_seq;
        let bytes = encode(history, &meta)?;
        let fs = RealFs::default();
        let lkg = self.dirs.lkg_file(&self.meta.id);
        durable::save_project(&fs, path, &fs, Some(&lkg), &bytes).map_err(|e| ProjectError::Write(e.to_string()))?;
        self.meta = meta;
        let _ = housekeeping::register(&self.dirs, self.meta.id, path);
        // The project file now holds everything: the recovery file is obsolete.
        let _ = recovery::remove(&fs, &self.recovery_path());
        Ok(())
    }

    /// Writes a recovery snapshot (call only at consistent points).
    pub fn snapshot(&mut self, history: &History) -> Result<u64, ProjectError> {
        let mut meta = self.meta.clone();
        meta.base_seq = self.meta.save_seq;
        let bytes = encode(history, &meta)?;
        recovery::write_snapshot(&RealFs::default(), &self.recovery_path(), &bytes, &container::verify)
            .map_err(|e| ProjectError::Write(e.to_string()))
    }

    /// Discards a pending recovery snapshot (after the user chose what to keep).
    pub fn discard_recovery(&self) {
        let _ = recovery::remove(&RealFs::default(), &self.recovery_path());
    }

    /// Clean close: deletes the recovery file and releases the lock.
    pub fn close(mut self) {
        self.discard_recovery();
        if let Some(l) = self.lock.take() {
            let _ = l.release();
        }
    }
}

#[cfg(test)]
mod tests;
