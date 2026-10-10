//! Project lock files (ADR 0007): `<data_dir>/locks/<project-id>.lock`, held with an
//! exclusive OS lock (`File::try_lock`: `flock` on Unix, `LockFileEx` on Windows).
//!
//! The file contains the holder's PID, a random instance ID, the host name and the
//! start time. Because the OS releases the lock when a process dies, a lock file whose
//! OS lock can be acquired is **stale** and is taken over. A clean release deletes the
//! file while still holding the lock. After acquiring, the holder checks that the path
//! still names the file it locked (a concurrent release may have unlinked it), and
//! retries otherwise, so two instances can never both believe they hold a lock.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum lock-file size we read.
const MAX_LOCK_FILE: u64 = 4096;
const HEADER: &str = "gusset-lock 1";

/// Who holds (or held) a lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockInfo {
    /// Process ID.
    pub pid: u32,
    /// Random per-process instance ID (32 hex digits).
    pub instance: String,
    /// Host name (best effort; may be empty).
    pub host: String,
    /// When the holder acquired the lock (Unix seconds).
    pub start_unix: u64,
}

impl LockInfo {
    fn to_text(&self) -> String {
        format!(
            "{HEADER}\npid={}\ninstance={}\nhost={}\nstart={}\n",
            self.pid, self.instance, self.host, self.start_unix
        )
    }

    /// Parses lock-file contents; `None` if malformed.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next()? != HEADER {
            return None;
        }
        let (mut pid, mut instance, mut host, mut start) = (None, None, None, None);
        for line in lines {
            let (k, v) = line.split_once('=')?;
            match k {
                "pid" => pid = v.parse().ok(),
                "instance" => instance = Some(v.to_owned()),
                "host" => host = Some(v.to_owned()),
                "start" => start = v.parse().ok(),
                _ => {}
            }
        }
        Some(Self { pid: pid?, instance: instance?, host: host?, start_unix: start? })
    }
}

impl fmt::Display for LockInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let host = if self.host.is_empty() { "unknown host" } else { &self.host };
        write!(f, "process {} on {host}", self.pid)
    }
}

/// Why a lock could not be acquired.
#[derive(Debug)]
pub enum LockError {
    /// Another running instance holds it. The holder's details are included when the
    /// platform lets us read a locked file (not on Windows).
    Held(Option<LockInfo>),
    /// The lock path is a symlink or not a regular file.
    NotARegularFile(PathBuf),
    /// An I/O error.
    Io(io::Error),
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockError::Held(Some(info)) => write!(f, "the project is open in another Gusset window ({info})"),
            LockError::Held(None) => f.write_str("the project is open in another Gusset window"),
            LockError::NotARegularFile(p) => write!(f, "refusing to use {} as a lock file", p.display()),
            LockError::Io(e) => write!(f, "cannot lock the project: {e}"),
        }
    }
}

impl std::error::Error for LockError {}

impl From<io::Error> for LockError {
    fn from(e: io::Error) -> Self {
        LockError::Io(e)
    }
}

/// How a lock was obtained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acquired {
    /// There was no previous holder.
    Fresh,
    /// A crashed instance's stale lock was taken over (its details, if readable).
    TookOverStale(Option<LockInfo>),
}

/// A held project lock. Dropping it releases the OS lock but leaves the file (as a
/// crash would); call [`ProjectLock::release`] for a clean release that deletes it.
#[derive(Debug)]
pub struct ProjectLock {
    file: File,
    path: PathBuf,
    info: LockInfo,
}

fn host_name() -> String {
    let raw = std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").ok())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_default();
    raw.trim().chars().filter(|c| !c.is_control() && *c != '=').take(255).collect()
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn open_lock_file(path: &Path, create: bool) -> Result<File, LockError> {
    match std::fs::symlink_metadata(path) {
        Ok(md) if !md.file_type().is_file() => return Err(LockError::NotARegularFile(path.to_path_buf())),
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound && create => {}
        Err(e) => return Err(LockError::Io(e)),
    }
    let mut o = OpenOptions::new();
    o.read(true).write(true).create(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    Ok(o.open(path)?)
}

fn read_info(file: &mut File) -> Option<LockInfo> {
    let mut s = String::new();
    file.seek(SeekFrom::Start(0)).ok()?;
    file.take(MAX_LOCK_FILE).read_to_string(&mut s).ok()?;
    LockInfo::parse(&s)
}

/// Windows locks the whole lock file against other handles (`LockFileEx`), so the
/// holder also publishes its details in an unlocked sidecar `<lock>.info`.
fn sidecar(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".info");
    PathBuf::from(s)
}

/// The holder's details as another process can see them: the lock file itself, or the
/// sidecar where the OS lock blocks reading it.
#[must_use]
pub fn holder_info(path: &Path) -> Option<LockInfo> {
    let read = |p: &Path| -> Option<LockInfo> {
        let mut s = String::new();
        File::open(p).ok()?.take(MAX_LOCK_FILE).read_to_string(&mut s).ok()?;
        LockInfo::parse(&s)
    };
    read(path).or_else(|| read(&sidecar(path)))
}

/// Whether `path` currently names the same file as `file`.
fn same_file(file: &File, path: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let a = file.metadata()?;
        match std::fs::symlink_metadata(path) {
            Ok(b) => Ok(a.dev() == b.dev() && a.ino() == b.ino()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Opening with no access rights works even while the file is locked.
        let other = match OpenOptions::new().access_mode(0).open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        Ok(crate::win::file_id(file)? == crate::win::file_id(&other)?)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (file, path);
        Ok(true)
    }
}

enum TryOutcome {
    Locked(File, Option<LockInfo>, bool),
    Held(Option<LockInfo>),
    Missing,
}

/// One attempt: open, try-lock, verify the path still names the locked file.
fn try_once(path: &Path, create: bool) -> Result<TryOutcome, LockError> {
    let mut file = match open_lock_file(path, create) {
        Ok(f) => f,
        Err(LockError::Io(e)) if e.kind() == io::ErrorKind::NotFound && !create => return Ok(TryOutcome::Missing),
        Err(e) => return Err(e),
    };
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Ok(TryOutcome::Held(read_info(&mut file).or_else(|| holder_info(path))));
        }
        Err(std::fs::TryLockError::Error(e)) => return Err(LockError::Io(e)),
    }
    if !same_file(&file, path)? {
        // Unlinked by a concurrent release between our open and lock: retry.
        return Ok(TryOutcome::Missing);
    }
    let len = file.metadata()?.len();
    let previous = read_info(&mut file);
    Ok(TryOutcome::Locked(file, previous, len > 0))
}

impl ProjectLock {
    /// Acquires the lock at `path`, creating the file if needed and taking over a stale
    /// lock left by a crashed instance.
    ///
    /// # Errors
    /// [`LockError::Held`] if a running instance holds it.
    pub fn acquire(path: &Path) -> Result<(ProjectLock, Acquired), LockError> {
        for _ in 0..16 {
            match try_once(path, true)? {
                TryOutcome::Locked(mut file, previous, had_content) => {
                    let info = LockInfo {
                        pid: std::process::id(),
                        instance: crate::wire::to_hex(&crate::rng::os_random_128()?),
                        host: host_name(),
                        start_unix: now_unix(),
                    };
                    file.set_len(0)?;
                    file.seek(SeekFrom::Start(0))?;
                    file.write_all(info.to_text().as_bytes())?;
                    file.sync_all()?;
                    if cfg!(windows) {
                        // Best effort: only the "who holds it" display depends on it.
                        let _ = std::fs::write(sidecar(path), info.to_text());
                    }
                    let how = if had_content { Acquired::TookOverStale(previous) } else { Acquired::Fresh };
                    return Ok((ProjectLock { file, path: path.to_path_buf(), info }, how));
                }
                TryOutcome::Held(info) => return Err(LockError::Held(info)),
                TryOutcome::Missing => {}
            }
        }
        Err(LockError::Io(io::Error::other("lock file keeps disappearing (heavy contention)")))
    }

    /// This holder's details.
    #[must_use]
    pub fn info(&self) -> &LockInfo {
        &self.info
    }

    /// The lock file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Clean release: deletes the lock file while still holding the lock, then unlocks.
    ///
    /// # Errors
    /// If deleting the file fails (the OS lock is released anyway).
    pub fn release(self) -> io::Result<()> {
        let _ = std::fs::remove_file(sidecar(&self.path));
        let r = match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
        let _ = self.file.unlock();
        r
    }
}

/// The state of an existing lock file, as seen by housekeeping.
#[derive(Debug)]
pub enum Probe {
    /// No lock file.
    Absent,
    /// A running instance holds it.
    Held(Option<LockInfo>),
    /// Stale: we now hold it (call [`ProjectLock::release`] to delete it).
    Stale(ProjectLock, Option<LockInfo>),
}

/// Checks an existing lock file without creating one. A stale lock is acquired and
/// returned so the caller can clean up under it.
///
/// # Errors
/// I/O errors, or [`LockError::NotARegularFile`].
pub fn probe(path: &Path) -> Result<Probe, LockError> {
    // A file unlinked by a concurrent release reports `Missing`, which is `Absent`.
    match try_once(path, false)? {
        TryOutcome::Missing => Ok(Probe::Absent),
        TryOutcome::Held(info) => Ok(Probe::Held(info)),
        TryOutcome::Locked(file, previous, _) => {
            let info = LockInfo {
                pid: std::process::id(),
                instance: crate::wire::to_hex(&crate::rng::os_random_128()?),
                host: host_name(),
                start_unix: now_unix(),
            };
            Ok(Probe::Stale(ProjectLock { file, path: path.to_path_buf(), info }, previous))
        }
    }
}

/// Whether a running instance holds the lock at `path` (without taking it).
///
/// # Errors
/// I/O errors.
pub fn is_held(path: &Path) -> Result<bool, LockError> {
    Ok(match probe(path)? {
        Probe::Held(_) => true,
        Probe::Absent => false,
        Probe::Stale(lock, _) => {
            // Leave the stale file for housekeeping to decide about; just unlock.
            drop(lock);
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("gusset-lock-{}", crate::rng::os_random_u64().unwrap()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn exclusive_then_release() {
        let d = tmp();
        let p = d.join("a.lock");
        let (lock, how) = ProjectLock::acquire(&p).unwrap();
        assert_eq!(how, Acquired::Fresh);
        assert_eq!(lock.info().pid, std::process::id());
        match ProjectLock::acquire(&p) {
            Err(LockError::Held(info)) => {
                #[cfg(unix)]
                assert_eq!(info.unwrap().instance, lock.info().instance);
                #[cfg(not(unix))]
                let _ = info;
            }
            other => unreachable!("{other:?}"),
        }
        assert!(is_held(&p).unwrap());
        lock.release().unwrap();
        assert!(!p.exists());
        assert!(!is_held(&p).unwrap());
        let (lock2, how) = ProjectLock::acquire(&p).unwrap();
        assert_eq!(how, Acquired::Fresh);
        lock2.release().unwrap();
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn stale_lock_is_taken_over() {
        let d = tmp();
        let p = d.join("b.lock");
        let dead = LockInfo { pid: 999_999, instance: "ab".repeat(16), host: "old-host".into(), start_unix: 5 };
        std::fs::write(&p, dead.to_text()).unwrap();
        let (lock, how) = ProjectLock::acquire(&p).unwrap();
        assert_eq!(how, Acquired::TookOverStale(Some(dead)));
        // Another process sees the holder's details on every platform.
        assert_eq!(holder_info(&p).as_ref(), Some(lock.info()));
        // Dropping without release simulates a crash: the file stays, the lock is free.
        drop(lock);
        assert!(p.exists());
        match probe(&p).unwrap() {
            Probe::Stale(l, Some(_)) => l.release().unwrap(),
            other => unreachable!("{other:?}"),
        }
        assert!(!p.exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn garbage_lock_file_is_stale_without_info() {
        let d = tmp();
        let p = d.join("c.lock");
        std::fs::write(&p, b"\x00\xffnot a lock").unwrap();
        let (lock, how) = ProjectLock::acquire(&p).unwrap();
        assert_eq!(how, Acquired::TookOverStale(None));
        lock.release().unwrap();
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_lock_path_is_refused() {
        let d = tmp();
        let target = d.join("target");
        std::fs::write(&target, b"x").unwrap();
        let p = d.join("d.lock");
        std::os::unix::fs::symlink(&target, &p).unwrap();
        assert!(matches!(ProjectLock::acquire(&p), Err(LockError::NotARegularFile(_))));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
