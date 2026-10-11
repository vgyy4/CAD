//! The project registry, startup housekeeping and project deletion (ADR 0007).
//!
//! # Registry
//! `<data>/projects.txt` maps project IDs to their last known path, so housekeeping can
//! tell whether a project still exists:
//! ```text
//! gusset-projects 1
//! <32 hex id>\t<percent-encoded path>
//! ```
//! Paths are percent-encoded byte strings (`%` and bytes outside printable ASCII are
//! `%XX`). Lines are sorted by ID. Updates are read-modify-write under an exclusive OS
//! lock on `<data>/projects.lock` and saved with `durable_write`, so concurrent
//! instances never lose each other's entries.
//!
//! # When is a project "gone"?
//! When its registered path does not exist **but its parent directory does**. A missing
//! parent usually means an unmounted drive or share, so its data is kept.
//!
//! # Housekeeping never touches a project whose lock is held
//! Before deleting anything that belongs to a project, housekeeping acquires that
//! project's lock itself; if another instance holds it, the project is skipped.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::durable::{self, durable_write_unchecked};
use crate::fs::RealFs;
use crate::lock::{self, Acquired, LockError, LockInfo, ProjectLock};
use crate::paths::{DataDirs, ProjectId};

const REGISTRY_HEADER: &str = "gusset-projects 1";

/// The project registry: project ID → last known path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    entries: BTreeMap<ProjectId, PathBuf>,
}

fn encode_path(p: &Path) -> io::Result<String> {
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        p.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let bytes = p
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "path is not valid Unicode"))?
        .as_bytes()
        .to_vec();
    let mut s = String::with_capacity(bytes.len());
    for b in bytes {
        if (0x20..0x7f).contains(&b) && b != b'%' {
            s.push(char::from(b));
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    Ok(s)
}

fn decode_path(s: &str) -> Option<PathBuf> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            let mut v = [0u8; 1];
            if !crate::wire::from_hex_into(hex, &mut v) {
                return None;
            }
            out.push(v[0]);
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    if out.is_empty() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(out).ok().map(PathBuf::from)
    }
}

/// Best-effort absolute, canonical form used for registry comparisons.
fn normalise(path: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(path) {
        return c;
    }
    match (path.parent().and_then(|p| std::fs::canonicalize(p).ok()), path.file_name()) {
        (Some(parent), Some(name)) => parent.join(name),
        _ => std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()),
    }
}

impl Registry {
    /// Parses registry text; malformed lines are skipped.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut entries = BTreeMap::new();
        let mut lines = text.lines();
        if lines.next() != Some(REGISTRY_HEADER) {
            return Self::default();
        }
        for line in lines {
            if let Some((id, path)) = line.split_once('\t')
                && let (Some(id), Some(path)) = (ProjectId::parse(id), decode_path(path))
            {
                entries.insert(id, path);
            }
        }
        Self { entries }
    }

    /// Serialises the registry.
    ///
    /// # Errors
    /// `InvalidData` for a path that cannot be represented (non-Unicode on Windows).
    pub fn to_text(&self) -> io::Result<String> {
        let mut s = String::from(REGISTRY_HEADER);
        s.push('\n');
        for (id, p) in &self.entries {
            s.push_str(&format!("{id}\t{}\n", encode_path(p)?));
        }
        Ok(s)
    }

    /// Loads the registry file (missing → empty).
    ///
    /// # Errors
    /// I/O errors other than "not found".
    pub fn load(path: &Path) -> io::Result<Self> {
        match std::fs::read(path) {
            Ok(b) => Ok(Self::parse(&String::from_utf8_lossy(&b))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    /// The registered path of `id`.
    #[must_use]
    pub fn get(&self, id: &ProjectId) -> Option<&Path> {
        self.entries.get(id).map(PathBuf::as_path)
    }

    /// The ID registered for `path` (compared after canonicalisation).
    #[must_use]
    pub fn find(&self, path: &Path) -> Option<ProjectId> {
        let n = normalise(path);
        self.entries.iter().find(|(_, p)| **p == n || p.as_path() == path).map(|(id, _)| *id)
    }

    /// Registers (or moves) a project. The path is canonicalised when possible.
    pub fn insert(&mut self, id: ProjectId, path: &Path) {
        self.entries.insert(id, normalise(path));
    }

    /// Removes a project.
    pub fn remove(&mut self, id: &ProjectId) -> Option<PathBuf> {
        self.entries.remove(id)
    }

    /// All entries, sorted by ID.
    pub fn iter(&self) -> impl Iterator<Item = (&ProjectId, &Path)> {
        self.entries.iter().map(|(id, p)| (id, p.as_path()))
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Read-modify-write of the registry under its OS lock. Returns the updated registry.
///
/// # Errors
/// I/O errors.
pub fn update_registry(dirs: &DataDirs, f: impl FnOnce(&mut Registry)) -> io::Result<Registry> {
    crate::paths::create_private_dir_all(dirs.data_dir())?;
    let lock_path = dirs.data_dir().join("projects.lock");
    let mut o = OpenOptions::new();
    o.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let guard = o.open(&lock_path)?;
    guard.lock()?;
    let path = dirs.registry_file();
    let result = (|| {
        let mut reg = Registry::load(&path)?;
        let before = reg.clone();
        f(&mut reg);
        if reg != before {
            durable_write_unchecked(&RealFs::private(), &path, reg.to_text()?.as_bytes())
                .map_err(|e| io::Error::other(e.to_string()))?;
        }
        Ok(reg)
    })();
    let _ = guard.unlock();
    result
}

/// Registers a project (call when a project is opened or saved to a new path).
///
/// # Errors
/// I/O errors.
pub fn register(dirs: &DataDirs, id: ProjectId, path: &Path) -> io::Result<()> {
    update_registry(dirs, |r| r.insert(id, path)).map(|_| ())
}

/// Unregisters a project.
///
/// # Errors
/// I/O errors.
pub fn unregister(dirs: &DataDirs, id: &ProjectId) -> io::Result<()> {
    update_registry(dirs, |r| {
        r.remove(id);
    })
    .map(|_| ())
}

/// Housekeeping options.
#[derive(Debug, Clone)]
pub struct Options {
    /// Temp files younger than this are left alone (another instance may be writing
    /// them). Default: one hour.
    pub temp_min_age: Duration,
    /// More directories to clean of our temp files (e.g. recently used folders).
    pub extra_dirs: Vec<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self { temp_min_age: Duration::from_secs(3600), extra_dirs: Vec::new() }
    }
}

/// What housekeeping removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Orphaned `~$*.gtmp` files.
    pub temp_files: usize,
    /// Recovery files of projects that no longer exist.
    pub recovery_files: usize,
    /// LKG copies of projects that no longer exist.
    pub lkg_files: usize,
    /// Stale lock files.
    pub stale_locks: usize,
    /// Cache namespaces of projects that no longer exist.
    pub cache_scopes: usize,
    /// Registry entries of projects that no longer exist.
    pub unregistered: usize,
    /// Projects skipped because a running instance holds their lock.
    pub skipped_locked: usize,
}

fn exists_no_follow(p: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(p) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Whether the project at `path` is gone (see the module docs).
fn project_gone(path: &Path) -> bool {
    let parent_exists = path.parent().is_some_and(|p| p.as_os_str().is_empty() || p.is_dir());
    parent_exists && matches!(exists_no_follow(path), Ok(false))
}

/// Removes a regular file or symlink (never follows it). Returns whether it existed.
fn remove_if_exists(p: &Path) -> io::Result<bool> {
    match std::fs::remove_file(p) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// IDs found as `<id>.<ext>` in `dir`.
fn ids_in(dir: &Path, ext: &str) -> io::Result<BTreeSet<ProjectId>> {
    let mut out = BTreeSet::new();
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    for e in rd {
        let e = e?;
        let name = e.file_name();
        if let Some(id) = name.to_str().and_then(|n| n.strip_suffix(ext)).and_then(ProjectId::parse) {
            out.insert(id);
        }
    }
    Ok(out)
}

/// Removes everything stored for project `id` outside the project file itself:
/// recovery file, LKG copy and cache namespace. Updates `report`.
fn remove_project_data(dirs: &DataDirs, id: &ProjectId, report: &mut Report) -> io::Result<()> {
    if remove_if_exists(&dirs.recovery_file(id))? {
        report.recovery_files += 1;
    }
    if remove_if_exists(&dirs.lkg_file(id))? {
        report.lkg_files += 1;
    }
    let scope = dirs.cache_dir().join(id.to_hex());
    if exists_no_follow(&scope)? {
        crate::cache::remove_dir_no_follow(&scope)?;
        report.cache_scopes += 1;
    }
    Ok(())
}

/// Runs startup housekeeping. Safe to run while other instances are running.
///
/// # Errors
/// I/O errors on our own directories (errors on individual files are skipped).
pub fn run(dirs: &DataDirs, opts: &Options) -> io::Result<Report> {
    let mut report = Report::default();
    crate::paths::create_private_dir_all(&dirs.locks_dir())?;
    let registry = Registry::load(&dirs.registry_file())?;

    let mut ids: BTreeSet<ProjectId> = registry.iter().map(|(id, _)| *id).collect();
    ids.extend(ids_in(&dirs.recovery_dir(), &format!(".{}", crate::recovery::EXTENSION))?);
    ids.extend(ids_in(&dirs.lkg_dir(), ".gus")?);
    ids.extend(ids_in(&dirs.locks_dir(), ".lock")?);
    if let Ok(scopes) = crate::cache::Cache::scopes_in(dirs.cache_dir()) {
        ids.extend(scopes);
    }

    let mut held = BTreeSet::new();
    let mut gone = Vec::new();
    for id in &ids {
        let lock_path = dirs.lock_file(id);
        let is_gone = registry.get(id).is_none_or(project_gone);
        let has_lock_file = exists_no_follow(&lock_path)?;
        if !is_gone && !has_lock_file {
            continue; // healthy project, nothing to do
        }
        let lock = match ProjectLock::acquire(&lock_path) {
            Ok((lock, how)) => {
                if matches!(how, Acquired::TookOverStale(_)) {
                    report.stale_locks += 1;
                }
                lock
            }
            Err(LockError::Held(_)) => {
                held.insert(*id);
                report.skipped_locked += 1;
                continue;
            }
            Err(LockError::NotARegularFile(_)) => continue,
            Err(LockError::Io(e)) => return Err(e),
        };
        if is_gone {
            remove_project_data(dirs, id, &mut report)?;
            if registry.get(id).is_some() {
                gone.push(*id);
            }
        }
        lock.release()?;
    }
    if !gone.is_empty() {
        update_registry(dirs, |r| {
            for id in &gone {
                r.remove(id);
            }
        })?;
        report.unregistered = gone.len();
    }

    report.stale_locks += lock::clean_stale_sidecars(&dirs.locks_dir())?;

    // Orphaned temp files.
    let mut temp_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    temp_dirs.insert(dirs.data_dir().to_path_buf());
    temp_dirs.insert(dirs.recovery_dir());
    temp_dirs.insert(dirs.lkg_dir());
    temp_dirs.insert(dirs.locks_dir());
    temp_dirs.insert(dirs.cache_dir().join("shared"));
    if let Ok(scopes) = crate::cache::Cache::scopes_in(dirs.cache_dir()) {
        for id in scopes {
            temp_dirs.insert(dirs.cache_dir().join(id.to_hex()));
        }
    }
    // Directories of every project registered at the start (including ones just
    // found gone, whose temp files are orphans now).
    for (_, p) in registry.iter() {
        if let Some(parent) = p.parent() {
            temp_dirs.insert(parent.to_path_buf());
        }
    }
    let registry = Registry::load(&dirs.registry_file())?;
    temp_dirs.extend(opts.extra_dirs.iter().cloned());
    let now = SystemTime::now();
    for dir in &temp_dirs {
        report.temp_files += clean_temps(dir, &registry, dirs, &held, now, opts.temp_min_age);
    }
    Ok(report)
}

/// The project a temp file belongs to: `<id>` stems in our data dirs, or a registered
/// project `<dir>/<stem>.gus`.
fn temp_owner(dir: &Path, stem: &str, registry: &Registry) -> Option<ProjectId> {
    ProjectId::parse(stem).or_else(|| registry.find(&dir.join(format!("{stem}.gus"))))
}

fn clean_temps(
    dir: &Path,
    registry: &Registry,
    dirs: &DataDirs,
    held: &BTreeSet<ProjectId>,
    now: SystemTime,
    min_age: Duration,
) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut removed = 0;
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(stem) = durable::temp_stem(&name) else { continue };
        let Ok(md) = e.metadata() else { continue }; // does not follow symlinks
        if !md.file_type().is_file() {
            continue;
        }
        let age = md.modified().ok().and_then(|m| now.duration_since(m).ok()).unwrap_or(Duration::ZERO);
        if age < min_age {
            continue;
        }
        if let Some(owner) = temp_owner(dir, stem, registry)
            && (held.contains(&owner) || lock::is_held(&dirs.lock_file(&owner)).unwrap_or(true))
        {
            continue;
        }
        if std::fs::remove_file(e.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Why [`delete_project`] failed.
#[derive(Debug)]
pub enum DeleteError {
    /// Another instance has the project open.
    InUse(Option<LockInfo>),
    /// Moving to the trash failed (nothing was deleted).
    Trash(String),
    /// Another I/O error.
    Io(io::Error),
}

impl fmt::Display for DeleteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeleteError::InUse(Some(i)) => write!(f, "the project is open in another Gusset window ({i})"),
            DeleteError::InUse(None) => f.write_str("the project is open in another Gusset window"),
            DeleteError::Trash(e) => write!(f, "moving the project to the trash failed: {e}"),
            DeleteError::Io(e) => write!(f, "deleting the project failed: {e}"),
        }
    }
}

impl std::error::Error for DeleteError {}

impl From<io::Error> for DeleteError {
    fn from(e: io::Error) -> Self {
        DeleteError::Io(e)
    }
}

/// Deletes a project from inside the app: moves the file to the OS trash (or deletes
/// it if `permanently`), then removes its recovery file, LKG copy, cache namespace
/// (thumbnails included), leftover temp files, registry entry and lock file.
///
/// `held` is the caller's own lock on this project, if it has the project open. `id` is
/// the project's ID if known (otherwise it is looked up in the registry; an
/// unregistered project has no app data to remove).
///
/// # Errors
/// [`DeleteError::InUse`] if another instance holds the lock; nothing is deleted when
/// moving to the trash fails.
pub fn delete_project(
    dirs: &DataDirs,
    path: &Path,
    id: Option<ProjectId>,
    held: Option<ProjectLock>,
    permanently: bool,
) -> Result<(), DeleteError> {
    let id = id.or_else(|| Registry::load(&dirs.registry_file()).ok().and_then(|r| r.find(path)));
    let lock = match (held, id) {
        (Some(l), _) => Some(l),
        (None, Some(id)) => {
            crate::paths::create_private_dir_all(&dirs.locks_dir())?;
            match ProjectLock::acquire(&dirs.lock_file(&id)) {
                Ok((l, _)) => Some(l),
                Err(LockError::Held(info)) => return Err(DeleteError::InUse(info)),
                Err(LockError::NotARegularFile(p)) => {
                    return Err(DeleteError::Io(io::Error::other(format!("bad lock file {}", p.display()))));
                }
                Err(LockError::Io(e)) => return Err(DeleteError::Io(e)),
            }
        }
        (None, None) => None,
    };

    if exists_no_follow(path)? {
        let r = if permanently {
            std::fs::remove_file(path).map_err(DeleteError::Io)
        } else {
            trash::delete(path).map_err(|e| DeleteError::Trash(e.to_string()))
        };
        if let Err(e) = r {
            if let Some(l) = lock {
                let _ = l.release();
            }
            return Err(e);
        }
    }
    // Leftover temp files of this project next to it.
    if let (Some(dir), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str()))
        && let Ok(rd) = std::fs::read_dir(if dir.as_os_str().is_empty() { Path::new(".") } else { dir })
    {
        for e in rd.flatten() {
            if durable::temp_stem(&e.file_name()) == Some(stem) && e.metadata().is_ok_and(|m| m.is_file()) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    if let Some(id) = id {
        remove_project_data(dirs, &id, &mut Report::default())?;
        unregister(dirs, &id)?;
    }
    if let Some(l) = lock {
        l.release()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_text_round_trip() {
        let mut r = Registry::default();
        let a = ProjectId([1; 16]);
        let b = ProjectId([2; 16]);
        r.entries.insert(a, PathBuf::from("/home/u/My Projects/100%/a\tb.gus"));
        r.entries.insert(b, PathBuf::from("/home/u/ünï/b.gus"));
        let text = r.to_text().unwrap();
        assert!(text.lines().all(|l| l.split('\t').count() <= 2));
        assert_eq!(Registry::parse(&text), r);
        assert_eq!(Registry::parse("garbage\nx\ty"), Registry::default());
        let mut t = text.clone();
        t.push_str("not-an-id\t/x\nabcd\n");
        assert_eq!(Registry::parse(&t), r);
    }
}
