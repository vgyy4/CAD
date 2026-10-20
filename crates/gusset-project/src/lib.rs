//! Projects on disk (ADR 0005, ADR 0007).
//!
//! - [`encode`]/[`decode`] map a [`History`] to the `.gus` container and back.
//! - [`Project`] owns a project's on-disk life: lock, saves (durable, with a
//!   last-known-good copy), recovery snapshots (one two-slot file per project), damage
//!   handling on open, and clean close.

use gusset_history::{History, HistoryRecords, ObjectKind};
use gusset_store::container::{self, ContainerWriter, Kind, RecordCache, SaveInfo};
use gusset_store::fs::RealFs;
use gusset_store::lock::{Acquired, ProjectLock};
use gusset_store::paths::{DataDirs, ProjectId, RiskyStorage};
use gusset_store::{Hash as StoreHash, durable, housekeeping, recovery};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub mod settings;
mod startup;

pub use settings::{ProjectSettings, SaveAction, SaveMode, SaveScheduler, Settings};
pub use startup::{StartupReport, startup};

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
                // `holder` says who has it (dogfooding F5-12: the text was said twice).
                write!(f, "{}: {holder}", path.display())
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
    encode_with(history, meta, None, &ProjectSettings::default(), &mut RecordCache::new())
}

/// Root holding the per-project settings overrides.
const SETTINGS_ROOT: &str = "settings";

/// Root naming the project file a recovery image belongs to (UTF-8, lossy). Lets
/// `open` refuse to offer one copy's recovery for another copy (review H2).
const ORIGIN_ROOT: &str = "origin";

fn origin_text(path: &Path) -> String {
    housekeeping::normalise(path).to_string_lossy().into_owned()
}

fn encode_with(
    history: &History,
    meta: &ProjectMeta,
    origin: Option<&Path>,
    overrides: &ProjectSettings,
    cache: &mut RecordCache,
) -> Result<Vec<u8>, ProjectError> {
    let r = history.to_records();
    let w = |e: container::WriteError| ProjectError::Write(e.to_string());
    let mut cw = ContainerWriter::new();
    for (k, h, bytes) in &r.objects {
        cw.add_cached(kind_of(*k), bytes, Some(StoreHash(h.0)), cache).map_err(w)?;
    }
    let nodes = cw.add_cached(Kind::NODE, r.nodes.as_bytes(), None, cache).map_err(w)?;
    let branches = cw.add_cached(Kind::BRANCHES, r.branches.as_bytes(), None, cache).map_err(w)?;
    let events = cw.add_cached(Kind::EVENTS, r.events.as_bytes(), None, cache).map_err(w)?;
    cache.retain_used();
    cw.set_root("nodes", nodes.as_bytes()).map_err(w)?;
    cw.set_root("branches", branches.as_bytes()).map_err(w)?;
    cw.set_root("events", events.as_bytes()).map_err(w)?;
    cw.set_root("meta", meta.to_text().as_bytes()).map_err(w)?;
    if let Some(o) = origin {
        cw.set_root(ORIGIN_ROOT, origin_text(o).as_bytes()).map_err(w)?;
    }
    if !overrides.is_empty() {
        cw.set_root(SETTINGS_ROOT, overrides.to_text().as_bytes()).map_err(w)?;
    }
    cw.finish(&SaveInfo { save_seq: meta.save_seq, created_unix: meta.created, app_version: APP_VERSION }).map_err(w)
}

/// Parses and fully verifies a `.gus` image.
pub fn decode(bytes: &[u8]) -> Result<(History, ProjectMeta), ProjectError> {
    decode_full(bytes, None).map(|(h, m, _)| (h, m))
}

fn decode_full(
    bytes: &[u8],
    prime: Option<&mut RecordCache>,
) -> Result<(History, ProjectMeta, ProjectSettings), ProjectError> {
    let c = container::open(bytes).map_err(|e| ProjectError::Damaged(e.to_string()))?;
    if let Some(cache) = prime {
        cache.prime(&c, |k| [Kind::FEATURE, Kind::PARAMS, Kind::SNAPSHOT, Kind::BLOB].contains(&k));
    }
    let overrides =
        c.root(SETTINGS_ROOT).map(|t| ProjectSettings::parse(&String::from_utf8_lossy(t))).unwrap_or_default();
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
    Ok((h, meta, overrides))
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
    /// When opened: a recovery file exists that is not provably older than the project
    /// file (it was offered, or could not be read). Until the user resolved it in the
    /// app, nothing may delete it: headless front ends must refuse to work on the
    /// project (see [`Project::release`]).
    pub recovery_pending: bool,
    /// Set when the opened file turned out to be a copy of another project (same ID,
    /// different path, the other still registered or open): it got this new identity
    /// and the old ID stays with the original (review H2). Holds the old ID.
    pub reidentified_from: Option<ProjectId>,
    /// Per-project overrides of the save settings (saved in the project file).
    pub overrides: ProjectSettings,
    /// Encoded records of the last save or snapshot, reused by the next one (M3).
    records: RecordCache,
    /// Orders recovery-copy writes made on other threads (see [`SnapshotWrite`]).
    gate: WriteGate,
    /// Saves prepared so far, and the newest one written (see [`SaveWrite`]).
    saves_prepared: u64,
    saves_written: WriteGate,
}

/// The generation of the newest recovery-copy state: bumped by every prepared snapshot
/// and by everything that makes pending snapshots obsolete (save, discard, close).
/// Writers hold the lock while writing, so bumping also waits for a write in progress.
type WriteGate = Arc<Mutex<u64>>;

fn lock_gate(gate: &WriteGate) -> MutexGuard<'_, u64> {
    gate.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A recovery snapshot encoded on the model thread, to be written on another thread
/// so the disk write does not block edits (M3). A write that something newer has
/// superseded (a later snapshot, a save, a discard or a close) is skipped, so a late
/// writer can neither overwrite newer state nor bring back an obsolete recovery file.
#[derive(Debug)]
#[must_use = "a prepared snapshot does nothing until written"]
pub struct SnapshotWrite {
    bytes: Vec<u8>,
    path: PathBuf,
    generation: u64,
    gate: WriteGate,
}

impl SnapshotWrite {
    /// Writes the snapshot unless it was superseded (then `Ok(None)`). Returns the
    /// sequence number of the written slot.
    pub fn write(self) -> Result<Option<u64>, ProjectError> {
        let current = lock_gate(&self.gate);
        if *current != self.generation {
            return Ok(None);
        }
        // Records were verified when encoded; the image is checked for framing and CRCs
        // and the read-back is compared byte-for-byte (ADR 0007, amended for M3).
        recovery::write_snapshot(&RealFs::default(), &self.path, &self.bytes, &container::verify_framing)
            .map(Some)
            .map_err(|e| ProjectError::Write(e.to_string()))
    }
}

/// `saves_written` once the project is gone.
const CLOSED: u64 = u64::MAX;

/// A project save encoded on the model thread, to be written on another thread. Saves
/// are written in the order they were prepared: one superseded by a newer save already
/// written is skipped, so an older state never replaces a newer file.
#[derive(Debug)]
#[must_use = "a prepared save does nothing until written"]
pub struct SaveWrite {
    bytes: Vec<u8>,
    path: PathBuf,
    dirs: DataDirs,
    id: ProjectId,
    generation: u64,
    written: WriteGate,
    /// The recovery-snapshot generation when the save was prepared.
    snapshot_generation: u64,
    snapshots: WriteGate,
}

impl SaveWrite {
    /// Writes the file durably (keeping a last-known-good copy) unless a newer save was
    /// already written (then `Ok(false)`).
    ///
    /// # Errors
    /// The write failed: the file on disk is unchanged.
    pub fn write(self) -> Result<bool, ProjectError> {
        let mut written = lock_gate(&self.written);
        if *written == CLOSED {
            return Err(ProjectError::Write("the project was closed before the save was written".into()));
        }
        if *written > self.generation {
            return Ok(false);
        }
        let fs = RealFs::default();
        let lkg = self.dirs.lkg_file(&self.id);
        durable::save_project_with(&fs, &self.path, &fs, Some(&lkg), &self.bytes, &container::verify_framing)
            .map_err(|e| ProjectError::Write(e.to_string()))?;
        *written = self.generation;
        let _ = housekeeping::register(&self.dirs, self.id, &self.path);
        // The file now holds everything up to the save's preparation. A recovery copy
        // prepared after it holds later edits and stays; otherwise it is obsolete, and
        // so are snapshots still waiting to be written.
        let mut snapshots = lock_gate(&self.snapshots);
        if *snapshots == self.snapshot_generation {
            *snapshots += 1;
            let _ = recovery::remove(&fs, &self.dirs.recovery_file(&self.id));
        }
        Ok(true)
    }
}

/// The retention part of history GC (ADR 0006), run when a project is opened: purges
/// "Recently deleted" branches past their retention and what only they referenced, and
/// keeps the history below its save limits. Never prunes live iterations.
fn retention_gc(history: &mut History, settings: &Settings) {
    history.gc(now_unix(), &settings.gc_policy(false));
}

fn new_id() -> Result<ProjectId, ProjectError> {
    ProjectId::new_random().map_err(|e| ProjectError::Io { path: PathBuf::new(), message: e.to_string() })
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
        let mut p = Project {
            path: None,
            meta,
            dirs,
            lock: None,
            risky: None,
            took_over_stale_lock: false,
            recovery_pending: false,
            reidentified_from: None,
            overrides: ProjectSettings::default(),
            records: RecordCache::new(),
            gate: WriteGate::default(),
            saves_prepared: 0,
            saves_written: WriteGate::default(),
        };
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

    /// Whether another window or server holds the project at `path` (its lock is taken).
    pub fn is_locked_elsewhere(path: &Path, dirs: &DataDirs) -> bool {
        let Ok(bytes) = std::fs::read(path) else { return false };
        let Ok((_, meta, _)) = decode_full(&bytes, None) else { return false };
        gusset_store::lock::is_held(&dirs.lock_file(&meta.id)).unwrap_or(false)
    }

    /// The history of a project file as last saved, without locking it or touching its
    /// recovery data: for read-only sessions next to the one that holds it (F5-12).
    ///
    /// # Errors
    /// The file cannot be read or is damaged.
    pub fn read_saved(path: &Path) -> Result<History, ProjectError> {
        let bytes =
            std::fs::read(path).map_err(|e| ProjectError::Io { path: path.to_path_buf(), message: e.to_string() })?;
        decode_full(&bytes, None).map(|(h, _, _)| h).map_err(|e| ProjectError::Damaged(e.to_string()))
    }

    /// Opens a project file, handling damage and pending recovery (ADR 0007).
    pub fn open(path: &Path, dirs: DataDirs) -> Result<(Project, History, OpenReport), ProjectError> {
        dirs.ensure().map_err(|e| ProjectError::Io { path: dirs.data_dir().to_path_buf(), message: e.to_string() })?;
        let io = |e: std::io::Error| ProjectError::Io { path: path.to_path_buf(), message: e.to_string() };
        let bytes = std::fs::read(path).map_err(io)?;
        let risky = gusset_store::paths::classify(path);
        let mut records = RecordCache::new();
        match decode_full(&bytes, Some(&mut records)) {
            Ok((mut history, meta, overrides)) => {
                let mut p = Project {
                    path: Some(path.to_path_buf()),
                    meta,
                    dirs,
                    lock: None,
                    risky,
                    took_over_stale_lock: false,
                    recovery_pending: false,
                    reidentified_from: None,
                    overrides,
                    records,
                    gate: WriteGate::default(),
                    saves_prepared: 0,
                    saves_written: WriteGate::default(),
                };
                let copy_of = p.copy_of(path);
                if copy_of.is_some() {
                    p.reidentify(path, &history)?;
                } else {
                    p.lock()?;
                    let _ = housekeeping::register(&p.dirs, p.meta.id, path);
                }
                let mut report = p.check_recovery(path);
                let settings = p.settings();
                retention_gc(&mut history, &settings);
                if let OpenReport::RecoveryAvailable { recovered, .. } = &mut report {
                    retention_gc(recovered, &settings);
                }
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
                            recovery_pending: false,
                            reidentified_from: None,
                            overrides: ProjectSettings::default(),
                            records: RecordCache::new(),
                            gate: WriteGate::default(),
                            saves_prepared: 0,
                            saves_written: WriteGate::default(),
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
                            recovery_pending: false,
                            reidentified_from: None,
                            overrides: ProjectSettings::default(),
                            records: RecordCache::new(),
                            gate: WriteGate::default(),
                            saves_prepared: 0,
                            saves_written: WriteGate::default(),
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

    /// Saves to a new path (Save As). A project that already has a file gets a **new
    /// project ID** for the new file, so the two files never share a lock, recovery file
    /// or last-known-good slot (review H2); the old file keeps its ID. Saving over a
    /// file that is open in another Gusset window is refused.
    pub fn save_as(&mut self, path: &Path, history: &History) -> Result<(), ProjectError> {
        let same = self.path.as_deref().is_some_and(|p| housekeeping::same_path(p, path));
        if let Ok(reg) = housekeeping::Registry::load(&self.dirs.registry_file())
            && let Some(other) = reg.find(path)
            && other != self.meta.id
            && gusset_store::lock::is_held(&self.dirs.lock_file(&other)).unwrap_or(false)
        {
            return Err(ProjectError::Locked {
                path: path.to_path_buf(),
                holder: "the project is open in another Gusset window or server".into(),
            });
        }
        if self.path.is_none() || same {
            self.save_to(path, history)?;
        } else {
            let new_id = new_id()?;
            let (new_lock, _) = ProjectLock::acquire(&self.dirs.lock_file(&new_id))
                .map_err(|e| ProjectError::Locked { path: path.to_path_buf(), holder: e.to_string() })?;
            let old_id = std::mem::replace(&mut self.meta.id, new_id);
            let old_lock = self.lock.replace(new_lock);
            if let Err(e) = self.save_to(path, history) {
                self.meta.id = old_id;
                if let Some(l) = std::mem::replace(&mut self.lock, old_lock) {
                    let _ = l.release();
                }
                return Err(e);
            }
            // The unsaved work the old file's recovery copy held is in the new file now.
            let _ = recovery::remove(&RealFs::default(), &self.dirs.recovery_file(&old_id));
            if let Some(l) = old_lock {
                let _ = l.release();
            }
        }
        self.path = Some(path.to_path_buf());
        self.risky = gusset_store::paths::classify(path);
        Ok(())
    }

    /// If the file at `path` (whose ID is `self.meta.id`) is a copy of another project,
    /// the other project's path: the registry maps this ID to a different existing
    /// file, or this ID's recovery copy was written for a different existing file.
    fn copy_of(&self, path: &Path) -> Option<PathBuf> {
        let reg = housekeeping::Registry::load(&self.dirs.registry_file()).ok()?;
        if let Some(other) = reg.get(&self.meta.id)
            && !housekeeping::same_path(other, path)
            && other.exists()
        {
            return Some(other.to_path_buf());
        }
        let origin = self.recovery_origin()?;
        (!housekeeping::same_path(&origin, path) && origin.exists()).then_some(origin)
    }

    /// The project file the recovery copy of this ID was written for, if recorded.
    fn recovery_origin(&self) -> Option<PathBuf> {
        let snap = recovery::read_newest(&RealFs::default(), &self.recovery_path(), &container::verify).ok()??;
        let c = container::open(&snap.payload).ok()?;
        let o = c.root(ORIGIN_ROOT)?;
        Some(PathBuf::from(String::from_utf8_lossy(o).into_owned()))
    }

    /// Gives an opened copy a fresh identity, registers it and writes it into the file
    /// right away (durably, like a save), so the copy keeps it. If the file cannot be
    /// written (read-only), the new identity still holds for this session.
    fn reidentify(&mut self, path: &Path, history: &History) -> Result<(), ProjectError> {
        let old = self.meta.id;
        self.meta.id = new_id()?;
        self.lock()?;
        self.reidentified_from = Some(old);
        if self.save_to(path, history).is_err() {
            let _ = housekeeping::register(&self.dirs, self.meta.id, path);
        }
        Ok(())
    }

    /// Looks for a recovery copy newer than the opened file.
    fn check_recovery(&mut self, path: &Path) -> OpenReport {
        match recovery::read_newest(&RealFs::default(), &self.recovery_path(), &container::verify) {
            Ok(None) => OpenReport::Clean,
            Ok(Some(snap)) => match decode(&snap.payload) {
                Ok((rh, rm)) if rm.base_seq >= self.meta.save_seq => {
                    if self.recovery_origin().is_some_and(|o| !housekeeping::same_path(&o, path)) {
                        // Written for another file; never offer it here (and never
                        // delete it: `copy_of` already gave this file its own ID).
                        return OpenReport::Clean;
                    }
                    self.recovery_pending = true;
                    OpenReport::RecoveryAvailable { recovered: Box::new(rh), recovery_seq: snap.seq }
                }
                // Older than the file: obsolete.
                Ok(_) => OpenReport::Clean,
                Err(_) => {
                    self.recovery_pending = true;
                    OpenReport::Clean
                }
            },
            // Exists but unreadable or damaged: keep it for the user to decide.
            Err(_) => {
                self.recovery_pending = true;
                OpenReport::Clean
            }
        }
    }

    fn save_to(&mut self, path: &Path, history: &History) -> Result<(), ProjectError> {
        self.prepare_save_to(path, history)?.write().map(|_| ())
    }

    /// Encodes a save to the current path; write it with [`SaveWrite::write`], on any
    /// thread, so the disk write does not hold up edits. The save sequence number is
    /// taken now: recovery snapshots prepared after this one count as newer than it.
    ///
    /// # Errors
    /// The project has no file yet, or encoding failed.
    pub fn prepare_save(&mut self, history: &History) -> Result<SaveWrite, ProjectError> {
        let path =
            self.path.clone().ok_or_else(|| ProjectError::Write("the project has no file yet; use Save As".into()))?;
        self.prepare_save_to(&path, history)
    }

    fn prepare_save_to(&mut self, path: &Path, history: &History) -> Result<SaveWrite, ProjectError> {
        let mut meta = self.meta.clone();
        meta.save_seq += 1;
        meta.base_seq = meta.save_seq;
        let bytes = encode_with(history, &meta, None, &self.overrides, &mut self.records)?;
        self.meta = meta;
        self.saves_prepared += 1;
        Ok(SaveWrite {
            bytes,
            path: path.to_path_buf(),
            dirs: self.dirs.clone(),
            id: self.meta.id,
            generation: self.saves_prepared,
            written: Arc::clone(&self.saves_written),
            snapshot_generation: *lock_gate(&self.gate),
            snapshots: Arc::clone(&self.gate),
        })
    }

    /// Makes every prepared snapshot obsolete; waits for a write in progress. Hold the
    /// returned guard while removing the recovery file.
    fn supersede_snapshots(&self) -> MutexGuard<'_, u64> {
        let mut g = lock_gate(&self.gate);
        *g += 1;
        g
    }

    /// Writes a recovery snapshot (call only at consistent points).
    pub fn snapshot(&mut self, history: &History) -> Result<u64, ProjectError> {
        // Nothing can supersede it between these two calls (both need this project).
        Ok(self.prepare_snapshot(history)?.write()?.unwrap_or_default())
    }

    /// Encodes a recovery snapshot (call only at consistent points); write it with
    /// [`SnapshotWrite::write`], on any thread. Supersedes snapshots prepared earlier.
    pub fn prepare_snapshot(&mut self, history: &History) -> Result<SnapshotWrite, ProjectError> {
        let mut meta = self.meta.clone();
        meta.base_seq = self.meta.save_seq;
        let bytes = encode_with(history, &meta, self.path.as_deref(), &self.overrides, &mut self.records)?;
        let generation = *self.supersede_snapshots();
        Ok(SnapshotWrite { bytes, path: self.recovery_path(), generation, gate: Arc::clone(&self.gate) })
    }

    /// Discards a pending recovery snapshot (after the user chose what to keep).
    pub fn discard_recovery(&self) {
        let _gate = self.supersede_snapshots();
        let _ = recovery::remove(&RealFs::default(), &self.recovery_path());
    }

    /// The user's settings with this project's overrides applied (the save mode and
    /// recovery timing to use for it).
    pub fn settings(&self) -> Settings {
        Settings::load(&self.dirs).with_overrides(&self.overrides)
    }

    /// "Compact history" (Settings → Storage): runs the full GC policy, including pruning
    /// of old unnamed intermediate iterations if the user enabled it. The caller marks
    /// the project dirty if anything was removed, so the next save shrinks the file.
    pub fn compact(&self, history: &mut History) -> gusset_history::GcReport {
        history.gc(now_unix(), &self.settings().gc_policy(true))
    }

    /// Releases the lock **without** touching the recovery file (headless front ends
    /// that must not resolve a pending recovery, and error paths).
    pub fn release(mut self) {
        // Another window may take the project over: no late writes into its recovery.
        drop(self.supersede_snapshots());
        if let Some(l) = self.lock.take() {
            let _ = l.release();
        }
    }

    /// Clean close: deletes the recovery file and releases the lock.
    pub fn close(mut self) {
        self.discard_recovery();
        if let Some(l) = self.lock.take() {
            let _ = l.release();
        }
    }
}

impl Drop for Project {
    /// However the project goes (close, release or a plain drop, e.g. after a failed
    /// final snapshot), recovery writes prepared earlier must not land after the lock is
    /// gone (security review V7). Waits for a write in progress.
    fn drop(&mut self) {
        drop(self.supersede_snapshots());
        // Nor saves: waits for one in progress; later ones fail (never land unlocked).
        *lock_gate(&self.saves_written) = CLOSED;
    }
}

/// A crash-recovery copy found at startup that the user has not resolved yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRecovery {
    pub id: ProjectId,
    /// The project file it belongs to. `None`: a never-saved ("Untitled") project, or
    /// one whose file is no longer reachable; recover it with
    /// [`Project::recover_unsaved`].
    pub path: Option<PathBuf>,
    /// Sequence number of the newest valid slot.
    pub recovery_seq: u64,
    /// When the recovery copy was last written.
    pub modified: Option<std::time::SystemTime>,
}

/// Every recovery copy that should be offered to the user at startup ("Recover unsaved
/// work"), review H1: copies of never-saved projects, and copies newer than their
/// project file. Copies held by a running Gusset instance (open projects) and copies
/// older than their file are skipped. Damaged copies are listed too (the user decides),
/// with `recovery_seq` 0.
pub fn pending_recoveries(dirs: &DataDirs) -> Vec<PendingRecovery> {
    let reg = housekeeping::Registry::load(&dirs.registry_file()).unwrap_or_default();
    let Ok(rd) = std::fs::read_dir(dirs.recovery_dir()) else { return Vec::new() };
    let ext = format!(".{}", recovery::EXTENSION);
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(id) = name.to_str().and_then(|n| n.strip_suffix(&ext)).and_then(ProjectId::parse) else {
            continue;
        };
        if !e.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        if gusset_store::lock::is_held(&dirs.lock_file(&id)).unwrap_or(true) {
            continue; // open in a running instance (or its lock is unreadable): not ours
        }
        let path = reg.get(&id).filter(|p| p.exists()).map(Path::to_path_buf);
        let snap = recovery::read_newest(&RealFs::default(), &e.path(), &container::verify).ok().flatten();
        if let (Some(p), Some(snap)) = (&path, &snap) {
            let file_seq = std::fs::read(p).ok().and_then(|b| container::open(&b).ok().map(|c| c.header().save_seq));
            let rec_base = decode(&snap.payload).ok().map(|(_, m)| m.base_seq);
            if let (Some(f), Some(r)) = (file_seq, rec_base)
                && r < f
            {
                continue; // older than the saved file: obsolete
            }
        }
        out.push(PendingRecovery {
            id,
            path,
            recovery_seq: snap.map_or(0, |s| s.seq),
            modified: e.metadata().ok().and_then(|m| m.modified().ok()),
        });
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.id.cmp(&b.id)));
    out
}

/// Deletes a pending recovery copy after the user chose to discard it.
///
/// # Errors
/// [`ProjectError::Locked`] if a running instance has that project open.
pub fn discard_pending_recovery(dirs: &DataDirs, id: ProjectId) -> Result<(), ProjectError> {
    let lp = dirs.lock_file(&id);
    let (lock, _) =
        ProjectLock::acquire(&lp).map_err(|e| ProjectError::Locked { path: lp.clone(), holder: e.to_string() })?;
    let r = recovery::remove(&RealFs::default(), &dirs.recovery_file(&id))
        .map_err(|e| ProjectError::Io { path: dirs.recovery_file(&id), message: e.to_string() });
    let _ = lock.release();
    r
}

impl Project {
    /// Opens a pending recovery copy (see [`pending_recoveries`]) as an unsaved project
    /// with the same ID, so further snapshots keep replacing the same single recovery
    /// file. The recovery file stays until the user saves (Save As) or closes cleanly.
    ///
    /// # Errors
    /// `Locked` if another instance has it open, `Damaged` if no slot is usable.
    pub fn recover_unsaved(dirs: DataDirs, id: ProjectId) -> Result<(Project, History), ProjectError> {
        dirs.ensure().map_err(|e| ProjectError::Io { path: dirs.data_dir().to_path_buf(), message: e.to_string() })?;
        let file = dirs.recovery_file(&id);
        let snap = recovery::read_newest(&RealFs::default(), &file, &container::verify)
            .map_err(|e| ProjectError::Damaged(e.to_string()))?
            .ok_or_else(|| ProjectError::Io { path: file.clone(), message: "no recovery copy".into() })?;
        let (mut history, mut meta, overrides) = decode_full(&snap.payload, None)?;
        meta.id = id;
        let mut p = Project {
            path: None,
            meta,
            dirs,
            lock: None,
            risky: None,
            took_over_stale_lock: false,
            recovery_pending: true,
            reidentified_from: None,
            overrides,
            records: RecordCache::new(),
            gate: WriteGate::default(),
            saves_prepared: 0,
            saves_written: WriteGate::default(),
        };
        p.lock()?;
        retention_gc(&mut history, &p.settings());
        Ok((p, history))
    }
}

#[cfg(test)]
mod tests;
