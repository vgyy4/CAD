//! The file-system operations `durable_write` needs, behind a trait so that power loss
//! can be simulated ([`FaultFs`]).
//!
//! Paths are used as given (no canonicalisation). Metadata never follows symlinks.

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;
use std::time::SystemTime;

mod fault;

pub use fault::{FaultFile, FaultFs, SECTOR};

/// What a directory entry is (symlinks are reported as such, never followed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A symbolic link (or Windows reparse point reported as a symlink).
    Symlink,
    /// Anything else (device, socket, ...).
    Other,
}

/// Metadata of one file or directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    /// The final path component.
    pub name: OsString,
    /// Entry type.
    pub kind: EntryKind,
    /// Length in bytes (files).
    pub len: u64,
    /// Last modification time, when the platform reports one.
    pub modified: Option<SystemTime>,
}

/// An open, writable file created by [`Fs::create_new`].
pub trait FsFile {
    /// Writes all of `data` at the current position.
    fn write_all(&mut self, data: &[u8]) -> io::Result<()>;
    /// Flushes data and metadata to stable storage (`fsync` / `FlushFileBuffers` /
    /// `F_FULLFSYNC`).
    fn sync_all(&mut self) -> io::Result<()>;
}

/// File-system operations used by the durable write path.
pub trait Fs {
    /// The file handle type returned by [`Fs::create_new`].
    type File: FsFile;

    /// Creates a new file for writing; fails with `AlreadyExists` if anything (including
    /// a symlink) exists at `path` (`O_CREAT | O_EXCL` semantics).
    fn create_new(&self, path: &Path) -> io::Result<Self::File>;
    /// Reads a whole file.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Atomically renames `from` over `to`, replacing `to` if it exists. On Windows the
    /// real implementation uses `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`.
    fn replace(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Makes directory-entry changes in `dir` durable (`fsync` on the directory on
    /// Unix; a no-op on Windows, where `replace` already writes through).
    fn sync_dir(&self, dir: &Path) -> io::Result<()>;
    /// Removes a file (never a directory; a symlink itself, not its target).
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    /// Lists a directory (not recursive, in unspecified order).
    fn read_dir(&self, dir: &Path) -> io::Result<Vec<EntryInfo>>;
    /// Metadata of `path` without following a final symlink.
    fn metadata(&self, path: &Path) -> io::Result<EntryInfo>;
    /// Creates a directory and its parents.
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    /// Randomness for unpredictable temp names (OS CSPRNG for [`RealFs`], seeded for
    /// [`FaultFs`]).
    fn random_u64(&self) -> io::Result<u64>;
}

/// The real file system.
///
/// `RealFs::private()` creates files with mode `0600` and directories with `0700` on
/// Unix (for the per-user data directories); `RealFs::default()` uses the normal
/// `0666`/`0777` masked by the umask (for user project files).
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFs {
    private: bool,
}

impl RealFs {
    /// Owner-only files and directories (Unix); identical to `default()` elsewhere.
    #[must_use]
    pub fn private() -> Self {
        Self { private: true }
    }
}

/// A file opened by [`RealFs`].
#[derive(Debug)]
pub struct RealFile(std::fs::File);

impl FsFile for RealFile {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.0.write_all(data)
    }

    fn sync_all(&mut self) -> io::Result<()> {
        // On Apple targets std implements this with fcntl(F_FULLFSYNC).
        self.0.sync_all()
    }
}

pub(crate) fn kind_of(ft: std::fs::FileType) -> EntryKind {
    if ft.is_symlink() {
        EntryKind::Symlink
    } else if ft.is_file() {
        EntryKind::File
    } else if ft.is_dir() {
        EntryKind::Dir
    } else {
        EntryKind::Other
    }
}

fn info_of(name: OsString, md: &std::fs::Metadata) -> EntryInfo {
    EntryInfo { name, kind: kind_of(md.file_type()), len: md.len(), modified: md.modified().ok() }
}

impl Fs for RealFs {
    type File = RealFile;

    fn create_new(&self, path: &Path) -> io::Result<RealFile> {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(if self.private { 0o600 } else { 0o666 });
        }
        o.open(path).map(RealFile)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn replace(&self, from: &Path, to: &Path) -> io::Result<()> {
        #[cfg(windows)]
        {
            crate::win::replace_file(from, to)
        }
        #[cfg(not(windows))]
        {
            std::fs::rename(from, to)
        }
    }

    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            std::fs::File::open(dir)?.sync_all()
        }
        #[cfg(not(unix))]
        {
            let _ = dir;
            Ok(())
        }
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn read_dir(&self, dir: &Path) -> io::Result<Vec<EntryInfo>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            // DirEntry::metadata does not follow symlinks.
            match e.metadata() {
                Ok(md) => out.push(info_of(e.file_name(), &md)),
                Err(err) if err.kind() == io::ErrorKind::NotFound => {} // raced with a delete
                Err(err) => return Err(err),
            }
        }
        Ok(out)
    }

    fn metadata(&self, path: &Path) -> io::Result<EntryInfo> {
        let md = std::fs::symlink_metadata(path)?;
        Ok(info_of(path.file_name().map(OsString::from).unwrap_or_default(), &md))
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        if self.private { create_private_dir_all(path) } else { std::fs::create_dir_all(path) }
    }

    fn random_u64(&self) -> io::Result<u64> {
        crate::rng::os_random_u64()
    }
}

/// Creates `path` and missing parents. Directories this call creates get mode `0700` on
/// Unix; the leaf is then set to `0700` even if it already existed (it is ours). On
/// other platforms the per-user profile directories are already private.
///
/// # Errors
/// I/O errors, or `InvalidInput` if the leaf exists and is a symlink.
pub fn create_private_dir_all(path: &Path) -> io::Result<()> {
    if let Ok(md) = std::fs::symlink_metadata(path)
        && md.file_type().is_symlink()
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "refusing to use a symlinked app directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path)
    }
}

/// Restricts an existing file to owner read/write (`0600`) on Unix; no-op elsewhere.
///
/// # Errors
/// I/O errors.
pub fn make_private_file(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
