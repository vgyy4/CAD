//! `FaultFs`: an in-memory file system that models a volatile page cache, for
//! power-loss testing (ADR 0007, "Power-loss testing").
//!
//! # Model
//! - **File data.** Every inode has a *durable* image (what is on the platter) and a
//!   *cached* image (what reads see). Each `write_all` is recorded as a pending write.
//!   `sync_all` makes the cached image durable and clears the pending list.
//! - **Directory entries.** Creating, renaming and removing names updates the cached
//!   namespace immediately and appends a pending link/unlink operation to the parent
//!   directory. `sync_dir(dir)` makes that directory's pending operations durable. A
//!   file's `sync_all` does *not* persist its directory entry (POSIX gives no such
//!   guarantee).
//! - **Directories** themselves are created durably (simplification).
//! - **Crash.** [`FaultFs::set_crash_after`] arms a crash point: once that many
//!   operations have run, every further operation fails as if the machine lost power.
//!   [`FaultFs::crash`] then returns the file system as found after reboot. For each
//!   file, its unflushed writes are applied in a random order (reordering), and each is
//!   either dropped, applied whole, or torn at 512-byte sector granularity (each sector
//!   independently present or not). For each directory, an arbitrary subset of its
//!   unsynced entry operations survives, in order.
//!
//! Everything is deterministic for a given seed and operation sequence.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use super::{EntryInfo, EntryKind, Fs, FsFile};
use crate::rng::SplitMix64;

/// Sector size used to tear writes.
pub const SECTOR: usize = 512;

#[derive(Debug, Clone)]
struct PendingWrite {
    offset: usize,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
struct Inode {
    durable: Vec<u8>,
    cached: Vec<u8>,
    pending: Vec<PendingWrite>,
    mtime: u64,
}

#[derive(Debug, Clone)]
enum DirOp {
    Link(PathBuf, usize),
    Unlink(PathBuf),
}

#[derive(Debug)]
struct State {
    rng: SplitMix64,
    inodes: Vec<Inode>,
    names: BTreeMap<PathBuf, usize>,
    durable_names: BTreeMap<PathBuf, usize>,
    pending_dirs: BTreeMap<PathBuf, Vec<DirOp>>,
    dirs: BTreeSet<PathBuf>,
    ops: u64,
    crash_after: Option<u64>,
    crashed: bool,
    clock: u64,
}

impl State {
    fn begin_op(&mut self) -> io::Result<()> {
        if !self.crashed && self.crash_after.is_some_and(|n| self.ops >= n) {
            self.crashed = true;
        }
        if self.crashed {
            return Err(io::Error::other("simulated power loss"));
        }
        self.ops += 1;
        self.clock += 1;
        Ok(())
    }

    fn parent_of(&self, path: &Path) -> io::Result<PathBuf> {
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        if self.dirs.contains(&parent) {
            Ok(parent)
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, format!("no such directory: {}", parent.display())))
        }
    }

    fn inode_mut(&mut self, ino: usize) -> io::Result<&mut Inode> {
        self.inodes.get_mut(ino).ok_or_else(|| io::Error::other("FaultFs: dangling inode"))
    }
}

/// A seeded, in-memory file system that simulates power loss. See the module docs.
#[derive(Debug, Clone)]
pub struct FaultFs {
    state: Arc<Mutex<State>>,
}

/// A file handle on a [`FaultFs`].
#[derive(Debug)]
pub struct FaultFile {
    state: Arc<Mutex<State>>,
    inode: usize,
    pos: usize,
}

fn lock(state: &Mutex<State>) -> io::Result<MutexGuard<'_, State>> {
    state.lock().map_err(|_| io::Error::other("FaultFs state poisoned"))
}

fn not_found(p: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("not found: {}", p.display()))
}

fn apply_write(data: &mut Vec<u8>, offset: usize, bytes: &[u8]) {
    let end = offset.saturating_add(bytes.len());
    if data.len() < end {
        data.resize(end, 0);
    }
    if let Some(dst) = data.get_mut(offset..end) {
        dst.copy_from_slice(bytes);
    }
}

impl FaultFs {
    /// An empty file system containing only the root directory `/`, seeded with `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut dirs = BTreeSet::new();
        dirs.insert(PathBuf::from("/"));
        dirs.insert(PathBuf::new());
        Self {
            state: Arc::new(Mutex::new(State {
                rng: SplitMix64::new(seed),
                inodes: Vec::new(),
                names: BTreeMap::new(),
                durable_names: BTreeMap::new(),
                pending_dirs: BTreeMap::new(),
                dirs,
                ops: 0,
                crash_after: None,
                crashed: false,
                clock: 0,
            })),
        }
    }

    /// Arms the crash point: after `ops` more successful operations (counted from the
    /// start of this file system's life, see [`FaultFs::op_count`]), every operation
    /// fails. `None` disarms it.
    pub fn set_crash_after(&self, ops: Option<u64>) {
        if let Ok(mut s) = lock(&self.state) {
            s.crash_after = ops;
        }
    }

    /// Number of operations performed so far (reads, writes, syncs, renames, ...).
    #[must_use]
    pub fn op_count(&self) -> u64 {
        lock(&self.state).map(|s| s.ops).unwrap_or(0)
    }

    /// Whether the crash point has been reached.
    #[must_use]
    pub fn has_crashed(&self) -> bool {
        lock(&self.state).map(|s| s.crashed).unwrap_or(true)
    }

    /// Simulates power loss now and returns the file system as found after reboot
    /// (see the module docs for what survives). `self` is left unchanged apart from
    /// its random state.
    #[must_use]
    pub fn crash(&self) -> FaultFs {
        let Ok(mut st) = lock(&self.state) else {
            return FaultFs::new(0);
        };
        let mut rng = SplitMix64::new(st.rng.next_u64());
        let mut inodes = Vec::with_capacity(st.inodes.len());
        for inode in &st.inodes {
            let mut data = inode.durable.clone();
            let mut writes = inode.pending.clone();
            rng.shuffle(&mut writes);
            for w in &writes {
                match rng.below(3) {
                    0 => {}
                    1 => apply_write(&mut data, w.offset, &w.data),
                    _ => {
                        // Torn: each sector touched by the write independently survives.
                        let end = w.offset + w.data.len();
                        let mut start = w.offset;
                        while start < end {
                            let sector_end = ((start / SECTOR) + 1).saturating_mul(SECTOR).min(end);
                            if rng.chance(1, 2)
                                && let Some(chunk) = w.data.get(start - w.offset..sector_end - w.offset)
                            {
                                apply_write(&mut data, start, chunk);
                            }
                            start = sector_end;
                        }
                    }
                }
            }
            inodes.push(Inode { durable: data.clone(), cached: data, pending: Vec::new(), mtime: inode.mtime });
        }
        let mut names = st.durable_names.clone();
        for ops in st.pending_dirs.values() {
            for op in ops {
                if rng.chance(1, 2) {
                    match op {
                        DirOp::Link(p, ino) => {
                            names.insert(p.clone(), *ino);
                        }
                        DirOp::Unlink(p) => {
                            names.remove(p);
                        }
                    }
                }
            }
        }
        FaultFs {
            state: Arc::new(Mutex::new(State {
                rng,
                inodes,
                durable_names: names.clone(),
                names,
                pending_dirs: BTreeMap::new(),
                dirs: st.dirs.clone(),
                ops: 0,
                crash_after: None,
                crashed: false,
                clock: st.clock,
            })),
        }
    }

    /// All file paths currently visible (cached namespace), sorted.
    #[must_use]
    pub fn files(&self) -> Vec<PathBuf> {
        lock(&self.state).map(|s| s.names.keys().cloned().collect()).unwrap_or_default()
    }
}

impl FsFile for FaultFile {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        let mut st = lock(&self.state)?;
        st.begin_op()?;
        let clock = st.clock;
        let pos = self.pos;
        let inode = st.inode_mut(self.inode)?;
        apply_write(&mut inode.cached, pos, data);
        inode.pending.push(PendingWrite { offset: pos, data: data.to_vec() });
        inode.mtime = clock;
        self.pos = pos.saturating_add(data.len());
        Ok(())
    }

    fn sync_all(&mut self) -> io::Result<()> {
        let mut st = lock(&self.state)?;
        st.begin_op()?;
        let inode = st.inode_mut(self.inode)?;
        inode.durable = inode.cached.clone();
        inode.pending.clear();
        Ok(())
    }
}

impl Fs for FaultFs {
    type File = FaultFile;

    fn create_new(&self, path: &Path) -> io::Result<FaultFile> {
        let mut st = lock(&self.state)?;
        st.begin_op()?;
        if st.names.contains_key(path) || st.dirs.contains(path) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("exists: {}", path.display())));
        }
        let parent = st.parent_of(path)?;
        let ino = st.inodes.len();
        let mtime = st.clock;
        st.inodes.push(Inode { mtime, ..Inode::default() });
        st.names.insert(path.to_path_buf(), ino);
        st.pending_dirs.entry(parent).or_default().push(DirOp::Link(path.to_path_buf(), ino));
        Ok(FaultFile { state: Arc::clone(&self.state), inode: ino, pos: 0 })
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let mut st = lock(&self.state)?;
        st.begin_op()?;
        let ino = *st.names.get(path).ok_or_else(|| not_found(path))?;
        Ok(st.inode_mut(ino)?.cached.clone())
    }

    fn replace(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut st = lock(&self.state)?;
        st.begin_op()?;
        let ino = *st.names.get(from).ok_or_else(|| not_found(from))?;
        let to_parent = st.parent_of(to)?;
        let from_parent = st.parent_of(from)?;
        if from == to {
            return Ok(());
        }
        st.names.remove(from);
        st.names.insert(to.to_path_buf(), ino);
        st.pending_dirs.entry(to_parent).or_default().push(DirOp::Link(to.to_path_buf(), ino));
        st.pending_dirs.entry(from_parent).or_default().push(DirOp::Unlink(from.to_path_buf()));
        Ok(())
    }

    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        let mut st = lock(&self.state)?;
        st.begin_op()?;
        if !st.dirs.contains(dir) {
            return Err(not_found(dir));
        }
        for op in st.pending_dirs.remove(dir).unwrap_or_default() {
            match op {
                DirOp::Link(p, ino) => {
                    st.durable_names.insert(p, ino);
                }
                DirOp::Unlink(p) => {
                    st.durable_names.remove(&p);
                }
            }
        }
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let mut st = lock(&self.state)?;
        st.begin_op()?;
        st.names.remove(path).ok_or_else(|| not_found(path))?;
        let parent = st.parent_of(path)?;
        st.pending_dirs.entry(parent).or_default().push(DirOp::Unlink(path.to_path_buf()));
        Ok(())
    }

    fn read_dir(&self, dir: &Path) -> io::Result<Vec<EntryInfo>> {
        let mut st = lock(&self.state)?;
        st.begin_op()?;
        if !st.dirs.contains(dir) {
            return Err(not_found(dir));
        }
        let mut out = Vec::new();
        for (p, &ino) in &st.names {
            if p.parent() == Some(dir) {
                let inode = st.inodes.get(ino);
                out.push(EntryInfo {
                    name: p.file_name().map(OsString::from).unwrap_or_default(),
                    kind: EntryKind::File,
                    len: inode.map_or(0, |i| i.cached.len() as u64),
                    modified: inode.map(|i| SystemTime::UNIX_EPOCH + Duration::from_secs(i.mtime)),
                });
            }
        }
        for d in &st.dirs {
            if d.parent() == Some(dir) && d.as_path() != dir {
                out.push(EntryInfo {
                    name: d.file_name().map(OsString::from).unwrap_or_default(),
                    kind: EntryKind::Dir,
                    len: 0,
                    modified: None,
                });
            }
        }
        Ok(out)
    }

    fn metadata(&self, path: &Path) -> io::Result<EntryInfo> {
        let mut st = lock(&self.state)?;
        st.begin_op()?;
        let name = path.file_name().map(OsString::from).unwrap_or_default();
        if let Some(&ino) = st.names.get(path) {
            let inode = st.inode_mut(ino)?;
            return Ok(EntryInfo {
                name,
                kind: EntryKind::File,
                len: inode.cached.len() as u64,
                modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(inode.mtime)),
            });
        }
        if st.dirs.contains(path) {
            return Ok(EntryInfo { name, kind: EntryKind::Dir, len: 0, modified: None });
        }
        Err(not_found(path))
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let mut st = lock(&self.state)?;
        st.begin_op()?;
        for anc in path.ancestors() {
            if st.names.contains_key(anc) {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "a file is in the way"));
            }
            st.dirs.insert(anc.to_path_buf());
        }
        Ok(())
    }

    fn random_u64(&self) -> io::Result<u64> {
        Ok(lock(&self.state)?.rng.next_u64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(fs: &FaultFs, p: &str, data: &[u8], sync: bool) {
        let mut f = fs.create_new(Path::new(p)).unwrap();
        f.write_all(data).unwrap();
        if sync {
            f.sync_all().unwrap();
        }
    }

    #[test]
    fn synced_file_and_dir_survive_every_crash() {
        for seed in 0..200 {
            let fs = FaultFs::new(seed);
            fs.create_dir_all(Path::new("/d")).unwrap();
            write_file(&fs, "/d/a", &[7u8; 3000], true);
            fs.sync_dir(Path::new("/d")).unwrap();
            let after = fs.crash();
            assert_eq!(after.read(Path::new("/d/a")).unwrap(), vec![7u8; 3000]);
        }
    }

    #[test]
    fn unsynced_data_can_be_lost_or_torn() {
        let mut lost = false;
        let mut torn = false;
        let mut whole = false;
        for seed in 0..200 {
            let fs = FaultFs::new(seed);
            write_file(&fs, "/a", &[1u8; 4096], false);
            fs.sync_dir(Path::new("/")).unwrap();
            let after = fs.crash();
            let data = after.read(Path::new("/a")).unwrap();
            match data.iter().filter(|&&b| b == 1).count() {
                0 => lost = true,
                4096 => whole = true,
                _ => torn = true,
            }
        }
        assert!(lost && torn && whole);
    }

    #[test]
    fn unsynced_rename_can_be_lost() {
        let mut kept_old = false;
        let mut got_new = false;
        for seed in 0..100 {
            let fs = FaultFs::new(seed);
            write_file(&fs, "/t", b"old", true);
            fs.replace(Path::new("/t"), Path::new("/x")).unwrap();
            fs.sync_dir(Path::new("/")).unwrap();
            write_file(&fs, "/t2", b"new", true);
            fs.replace(Path::new("/t2"), Path::new("/x")).unwrap();
            let after = fs.crash();
            match after.read(Path::new("/x")).unwrap().as_slice() {
                b"old" => kept_old = true,
                b"new" => got_new = true,
                other => unreachable!("{other:?}"),
            }
        }
        assert!(kept_old && got_new);
    }

    #[test]
    fn crash_point_stops_everything() {
        let fs = FaultFs::new(1);
        fs.set_crash_after(Some(2));
        write_file(&fs, "/a", b"x", false); // create + write = 2 ops
        assert!(!fs.has_crashed());
        assert!(fs.read(Path::new("/a")).is_err());
        assert!(fs.has_crashed());
        assert!(fs.metadata(Path::new("/a")).is_err());
    }
}
