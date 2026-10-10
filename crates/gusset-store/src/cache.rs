//! Content-addressed, size-capped, LRU-evicting cache directory (ADR 0007).
//!
//! ```text
//! <cache>/shared/<key hex>.bin   entries usable by any project
//! <cache>/<project-id>/<key hex>.bin   entries owned by one project (thumbnails, ...)
//! entry: magic "GCE1" | payload_len u64 | blake3(payload) [32] | payload
//! ```
//!
//! - Writes go to a unique `~$<key>.<random>.gtmp` temp file that is renamed into place,
//!   so readers never see a partial entry. Entries are not fsynced: everything here is
//!   regenerable, and a torn entry after power loss fails its checksum and becomes a
//!   miss (and is deleted).
//! - LRU is tracked with file modification times: a hit touches the entry; eviction
//!   deletes the oldest entries until usage is within the cap.
//! - Several instances may share the directory. The worst case of any race (an entry
//!   evicted or replaced while another instance reads it) is a miss, never corruption.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::Hash;
use crate::paths::{ProjectId, create_private_dir_all};

const MAGIC: [u8; 4] = *b"GCE1";
const ENTRY_HEADER: u64 = 44;
const SHARED: &str = "shared";

/// Which namespace an entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Shared between projects (e.g. tessellations keyed by geometry hash).
    Shared,
    /// Owned by one project; removed when the project is deleted.
    Project(ProjectId),
}

/// The default size cap: 2 % of free disk space, clamped to 256 MiB–4 GiB.
#[must_use]
pub fn default_cap(free_bytes: u64) -> u64 {
    (free_bytes / 50).clamp(256 << 20, 4 << 30)
}

/// A cache directory.
#[derive(Debug)]
pub struct Cache {
    dir: PathBuf,
    cap: u64,
    /// Running estimate of usage (bytes), refreshed by every full scan.
    usage: Mutex<Option<u64>>,
}

/// One entry found by a scan.
#[derive(Debug, Clone)]
struct Entry {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
}

fn encode(data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(data.len() + 44);
    v.extend_from_slice(&MAGIC);
    v.extend_from_slice(&(data.len() as u64).to_le_bytes());
    v.extend_from_slice(Hash::of(data).as_bytes());
    v.extend_from_slice(data);
    v
}

fn decode(bytes: &[u8]) -> Option<&[u8]> {
    let mut r = crate::wire::Reader::new(bytes);
    if r.array::<4>()? != MAGIC {
        return None;
    }
    let len = crate::wire::usize_of(r.u64()?)?;
    let hash = Hash(r.array::<32>()?);
    let payload = r.take(len)?;
    (r.remaining() == 0 && Hash::of(payload) == hash).then_some(payload)
}

fn is_entry_name(name: &str) -> bool {
    name.strip_suffix(".bin").is_some_and(|h| Hash::from_hex(h).is_some())
}

impl Cache {
    /// Opens (creating, owner-only) a cache directory with a size cap in bytes.
    ///
    /// # Errors
    /// If the directory cannot be created.
    pub fn open(dir: impl Into<PathBuf>, cap: u64) -> io::Result<Self> {
        let dir = dir.into();
        create_private_dir_all(&dir)?;
        Ok(Self { dir, cap, usage: Mutex::new(None) })
    }

    /// The cache directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The size cap.
    #[must_use]
    pub fn cap(&self) -> u64 {
        self.cap
    }

    fn scope_dir(&self, scope: Scope) -> PathBuf {
        match scope {
            Scope::Shared => self.dir.join(SHARED),
            Scope::Project(id) => self.dir.join(id.to_hex()),
        }
    }

    fn entry_path(&self, scope: Scope, key: &Hash) -> PathBuf {
        self.scope_dir(scope).join(format!("{}.bin", key.to_hex()))
    }

    /// Looks up an entry. Damaged entries are deleted and reported as a miss.
    ///
    /// # Errors
    /// I/O errors other than "not found".
    pub fn get(&self, scope: Scope, key: &Hash) -> io::Result<Option<Vec<u8>>> {
        let path = self.entry_path(scope, key);
        match std::fs::symlink_metadata(&path) {
            Ok(md) if md.file_type().is_file() && md.len() <= self.cap.saturating_add(ENTRY_HEADER) => {}
            Ok(_) => return Ok(None),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let Some(payload) = decode(&bytes) else {
            let _ = std::fs::remove_file(&path);
            return Ok(None);
        };
        // Touch for LRU; failure only affects eviction order.
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&path) {
            let _ = f.set_modified(SystemTime::now());
        }
        Ok(Some(payload.to_vec()))
    }

    /// Stores an entry (replacing any existing one), then evicts if over the cap.
    /// Entries larger than the cap are silently not stored.
    ///
    /// # Errors
    /// I/O errors.
    pub fn put(&self, scope: Scope, key: &Hash, data: &[u8]) -> io::Result<()> {
        let size = data.len() as u64 + ENTRY_HEADER;
        if size > self.cap {
            return Ok(());
        }
        let dir = self.scope_dir(scope);
        create_private_dir_all(&dir)?;
        let token = crate::rng::os_random_u64()?;
        let temp = dir.join(format!(
            "{}{}.{token:016x}{}",
            crate::durable::TEMP_PREFIX,
            key.to_hex(),
            crate::durable::TEMP_SUFFIX
        ));
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        // std::fs::rename replaces an existing entry on every platform.
        let result = o
            .open(&temp)
            .and_then(|mut f| f.write_all(&encode(data)))
            .and_then(|()| std::fs::rename(&temp, self.entry_path(scope, key)));
        if let Err(e) = result {
            let _ = std::fs::remove_file(&temp);
            return Err(e);
        }
        let over = match self.usage.lock() {
            Ok(mut u) => match *u {
                Some(ref mut v) => {
                    *v = v.saturating_add(size);
                    *v > self.cap
                }
                None => true,
            },
            Err(_) => true,
        };
        if over {
            self.evict()?;
        }
        Ok(())
    }

    /// Removes one entry (missing is fine).
    ///
    /// # Errors
    /// I/O errors other than "not found".
    pub fn remove(&self, scope: Scope, key: &Hash) -> io::Result<()> {
        match std::fs::remove_file(self.entry_path(scope, key)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Removes every entry of a project.
    ///
    /// # Errors
    /// I/O errors other than "not found".
    pub fn remove_project(&self, id: &ProjectId) -> io::Result<()> {
        remove_dir_no_follow(&self.scope_dir(Scope::Project(*id)))
    }

    /// Project IDs that have a cache namespace.
    ///
    /// # Errors
    /// I/O errors listing the directory.
    pub fn project_scopes(&self) -> io::Result<Vec<ProjectId>> {
        Self::scopes_in(&self.dir)
    }

    pub(crate) fn scopes_in(dir: &Path) -> io::Result<Vec<ProjectId>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            if e.file_type().is_ok_and(|t| t.is_dir())
                && let Some(id) = e.file_name().to_str().and_then(ProjectId::parse)
            {
                out.push(id);
            }
        }
        out.sort();
        Ok(out)
    }

    fn scan(&self) -> io::Result<Vec<Entry>> {
        let mut dirs = vec![self.dir.join(SHARED)];
        for id in self.project_scopes()? {
            dirs.push(self.scope_dir(Scope::Project(id)));
        }
        let mut out = Vec::new();
        for d in dirs {
            let rd = match std::fs::read_dir(&d) {
                Ok(rd) => rd,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            for e in rd {
                let Ok(e) = e else { continue };
                let name = e.file_name();
                if !name.to_str().is_some_and(is_entry_name) {
                    continue;
                }
                if let Ok(md) = e.metadata()
                    && md.file_type().is_file()
                {
                    let modified = md.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    out.push(Entry { path: e.path(), len: md.len(), modified });
                }
            }
        }
        Ok(out)
    }

    /// Total size of all entries in bytes.
    ///
    /// # Errors
    /// I/O errors listing the directory.
    pub fn usage(&self) -> io::Result<u64> {
        Ok(self.scan()?.iter().map(|e| e.len).sum())
    }

    /// Evicts least-recently-used entries until usage is within the cap. Returns the
    /// number of entries removed.
    ///
    /// # Errors
    /// I/O errors listing the directory.
    pub fn evict(&self) -> io::Result<usize> {
        self.evict_to(self.cap)
    }

    /// Evicts least-recently-used entries until usage is at most `target` bytes.
    ///
    /// # Errors
    /// I/O errors listing the directory.
    pub fn evict_to(&self, target: u64) -> io::Result<usize> {
        let mut entries = self.scan()?;
        entries.sort_by(|a, b| a.modified.cmp(&b.modified).then_with(|| a.path.cmp(&b.path)));
        let mut total: u64 = entries.iter().map(|e| e.len).sum();
        let mut removed = 0;
        for e in &entries {
            if total <= target {
                break;
            }
            match std::fs::remove_file(&e.path) {
                Ok(()) => removed += 1,
                Err(err) if err.kind() == io::ErrorKind::NotFound => {} // another instance won
                Err(err) => return Err(err),
            }
            total = total.saturating_sub(e.len);
        }
        if let Ok(mut u) = self.usage.lock() {
            *u = Some(total);
        }
        Ok(removed)
    }

    /// Deletes every entry ("Clear cache").
    ///
    /// # Errors
    /// I/O errors.
    pub fn clear(&self) -> io::Result<()> {
        remove_dir_no_follow(&self.dir.join(SHARED))?;
        for id in self.project_scopes()? {
            self.remove_project(&id)?;
        }
        if let Ok(mut u) = self.usage.lock() {
            *u = Some(0);
        }
        Ok(())
    }
}

/// `remove_dir_all` that refuses to start at a symlink (std never follows symlinks
/// inside the tree). Missing is fine.
pub(crate) fn remove_dir_no_follow(dir: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(md) if md.file_type().is_symlink() => std::fs::remove_file(dir),
        Ok(_) => match std::fs::remove_dir_all(dir) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_codec() {
        let e = encode(b"hello");
        assert_eq!(decode(&e), Some(&b"hello"[..]));
        for i in 0..e.len() {
            let mut d = e.clone();
            d[i] ^= 0x10;
            assert_eq!(decode(&d), None, "flip at {i}");
        }
        assert_eq!(decode(&e[..e.len() - 1]), None);
    }

    #[test]
    fn caps() {
        assert_eq!(default_cap(0), 256 << 20);
        assert_eq!(default_cap(100 << 30), 2 << 30);
        assert_eq!(default_cap(10 << 40), 4 << 30);
    }
}
