//! `durable_write`: the one audited write path (ADR 0007).
//!
//! Every persistent write (project saves, auto-saves, recovery snapshots, last-known-good
//! copies, settings, the project registry) goes through [`durable_write`]:
//!
//! 1. create `~$<stem>.<16 hex>.gtmp` in the target's directory with create-new
//!    semantics (unpredictable name, never follows a planted symlink) and write all bytes;
//! 2. `sync_all` (`fsync` / `FlushFileBuffers` / `F_FULLFSYNC`);
//! 3. read the temp file back by path, check it is byte-identical, and run the caller's
//!    validator; on failure delete the temp file and return an error;
//! 4. atomically replace the target (`rename(2)`, or `MoveFileExW` with
//!    `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH` on Windows);
//! 5. `fsync` the parent directory (Unix).
//!
//! Only when it returns `Ok` is the write *acknowledged*: after a crash or power loss the
//! target then holds exactly the new bytes. A crash at any earlier point leaves the
//! complete old file (and possibly an orphaned temp file that housekeeping removes).

use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::container;
use crate::fs::{Fs, FsFile};

/// Prefix of every temp file created by `durable_write`.
pub const TEMP_PREFIX: &str = "~$";
/// Extension of every temp file created by `durable_write`.
pub const TEMP_SUFFIX: &str = ".gtmp";

/// The step of [`durable_write`] that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Creating the temp file.
    CreateTemp,
    /// Writing the temp file.
    Write,
    /// Flushing the temp file to stable storage.
    Sync,
    /// Reading the temp file back.
    ReadBack,
    /// Renaming the temp file over the target.
    Replace,
    /// Flushing the directory entry (the target already holds the new data, but its
    /// durability is not guaranteed).
    SyncDir,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Step::CreateTemp => "creating the temporary file",
            Step::Write => "writing the temporary file",
            Step::Sync => "flushing the temporary file to disk",
            Step::ReadBack => "reading the temporary file back",
            Step::Replace => "replacing the target file",
            Step::SyncDir => "flushing the directory",
        })
    }
}

/// Why [`durable_write`] failed. In every case except `SyncDir` the target is untouched.
#[derive(Debug)]
pub enum DurableError<E> {
    /// An I/O error at `step`.
    Io {
        /// The failing step.
        step: Step,
        /// The target path.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The bytes read back differ from the bytes written (bad media, driver or
    /// file-system bug, or concurrent tampering).
    ReadBackMismatch {
        /// The target path.
        path: PathBuf,
        /// Bytes written.
        written: usize,
        /// Bytes read back.
        read: usize,
    },
    /// The caller's validator rejected the bytes read back.
    Invalid(E),
}

impl<E: fmt::Display> fmt::Display for DurableError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DurableError::Io { step, path, source } => {
                write!(f, "saving {} failed while {step}: {source}", path.display())
            }
            DurableError::ReadBackMismatch { path, written, read } => write!(
                f,
                "saving {} failed: the data read back from disk ({read} bytes) does not match what was \
                 written ({written} bytes); the previous file was left untouched",
                path.display()
            ),
            DurableError::Invalid(e) => {
                write!(f, "the data read back from disk failed verification ({e}); the previous file was kept")
            }
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for DurableError<E> {}

/// Whether a file name looks like a temp file created by [`durable_write`]:
/// `~$<stem>.<16 lowercase hex>.gtmp`.
#[must_use]
pub fn is_temp_name(name: &OsStr) -> bool {
    temp_stem(name).is_some()
}

/// The `<stem>` part of a temp-file name created by [`durable_write`], if it is one.
#[must_use]
pub fn temp_stem(name: &OsStr) -> Option<&str> {
    let s = name.to_str()?;
    let inner = s.strip_prefix(TEMP_PREFIX)?.strip_suffix(TEMP_SUFFIX)?;
    let (stem, token) = inner.rsplit_once('.')?;
    let ok = token.len() == 16 && token.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    ok.then_some(stem)
}

fn parent_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

/// Writes `bytes` to `path` durably and atomically. See the module docs.
///
/// `validate` is run on the bytes read back from the flushed temp file; it should do a
/// full structural check (for `.gus` files: [`container::verify`]).
///
/// # Errors
/// See [`DurableError`]. On error the target is unchanged (except for
/// [`Step::SyncDir`], where it already holds the new data) and no temp file remains,
/// unless removing it also failed.
pub fn durable_write<F, E>(
    fs: &F,
    path: &Path,
    bytes: &[u8],
    validate: &dyn Fn(&[u8]) -> Result<(), E>,
) -> Result<(), DurableError<E>>
where
    F: Fs + ?Sized,
{
    let io_err = |step: Step, source: io::Error| DurableError::Io { step, path: path.to_path_buf(), source };
    let dir = parent_dir(path);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();

    // 1. Create the temp file (retry on the astronomically unlikely name collision).
    let mut attempt = 0;
    let (temp, mut file) = loop {
        let token = fs.random_u64().map_err(|e| io_err(Step::CreateTemp, e))?;
        let temp = dir.join(format!("{TEMP_PREFIX}{stem}.{token:016x}{TEMP_SUFFIX}"));
        match fs.create_new(&temp) {
            Ok(f) => break (temp, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && attempt < 8 => attempt += 1,
            Err(e) => return Err(io_err(Step::CreateTemp, e)),
        }
    };
    let cleanup = |e: DurableError<E>| {
        let _ = fs.remove_file(&temp);
        e
    };

    if let Err(e) = file.write_all(bytes) {
        drop(file);
        return Err(cleanup(io_err(Step::Write, e)));
    }
    // 2. Flush.
    if let Err(e) = file.sync_all() {
        drop(file);
        return Err(cleanup(io_err(Step::Sync, e)));
    }
    drop(file);

    // 3. Verify what is actually on disk before touching the target.
    let back = match fs.read(&temp) {
        Ok(b) => b,
        Err(e) => return Err(cleanup(io_err(Step::ReadBack, e))),
    };
    if back != bytes {
        return Err(cleanup(DurableError::ReadBackMismatch {
            path: path.to_path_buf(),
            written: bytes.len(),
            read: back.len(),
        }));
    }
    if let Err(e) = validate(&back) {
        return Err(cleanup(DurableError::Invalid(e)));
    }

    // 4. Atomic replace.
    if let Err(e) = fs.replace(&temp, path) {
        return Err(cleanup(io_err(Step::Replace, e)));
    }
    // 5. Make the rename durable.
    fs.sync_dir(dir).map_err(|e| io_err(Step::SyncDir, e))
}

/// [`durable_write`] without content validation (the byte-identical read-back check
/// still runs).
///
/// # Errors
/// See [`durable_write`].
pub fn durable_write_unchecked<F: Fs + ?Sized>(
    fs: &F,
    path: &Path,
    bytes: &[u8],
) -> Result<(), DurableError<std::convert::Infallible>> {
    durable_write(fs, path, bytes, &|_| Ok(()))
}

/// Saves a project file (`.gus` container bytes) per ADR 0007:
///
/// 1. `bytes` must verify as a container (never write an invalid project);
/// 2. if `lkg` is given and the existing project file verifies, it is first copied there
///    with `durable_write` (the single last-known-good generation);
/// 3. `bytes` are written with `durable_write`, validated by [`container::verify`].
///
/// `project_fs` is used for the project path and `data_fs` for the LKG copy (so the LKG
/// can live in the private data directory while the project uses default permissions).
///
/// # Errors
/// A [`DurableError`] whose validation error is a [`container::OpenError`]. A failure
/// while writing the LKG copy aborts the save before the project file is touched.
pub fn save_project<P, D>(
    project_fs: &P,
    path: &Path,
    data_fs: &D,
    lkg: Option<&Path>,
    bytes: &[u8],
) -> Result<(), DurableError<container::OpenError>>
where
    P: Fs + ?Sized,
    D: Fs + ?Sized,
{
    container::verify(bytes).map_err(DurableError::Invalid)?;
    if let Some(lkg) = lkg {
        match project_fs.read(path) {
            Ok(old) if container::verify(&old).is_ok() => {
                durable_write(data_fs, lkg, &old, &container::verify)?;
            }
            // Missing or damaged: keep whatever LKG we already have.
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(DurableError::Io { step: Step::ReadBack, path: path.to_path_buf(), source: e }),
        }
    }
    durable_write(project_fs, path, bytes, &container::verify)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_names() {
        assert_eq!(temp_stem(OsStr::new("~$part.0123456789abcdef.gtmp")), Some("part"));
        assert_eq!(temp_stem(OsStr::new("~$my.part.0123456789abcdef.gtmp")), Some("my.part"));
        assert!(!is_temp_name(OsStr::new("~$part.0123456789ABCDEF.gtmp")));
        assert!(!is_temp_name(OsStr::new("~$part.123.gtmp")));
        assert!(!is_temp_name(OsStr::new("part.0123456789abcdef.gtmp")));
        assert!(!is_temp_name(OsStr::new("~$part.0123456789abcdef.tmp")));
    }
}
