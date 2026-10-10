//! Shared test helpers.
// Shared by several test crates, each using a subset. Test-only code (see the
// matching allowance at the top of each test file).
#![allow(dead_code, unreachable_pub, clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use gusset_store::container::{ContainerWriter, Kind, SaveInfo};

/// A temporary directory removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).unwrap();
        let p = std::env::temp_dir().join(format!("gusset-store-{tag}-{}", u64::from_le_bytes(b)));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Deterministic test RNG (xorshift64*).
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0.max(1);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

/// A small but representative project image with save sequence `seq`.
pub fn project_bytes(seq: u64) -> Vec<u8> {
    let mut w = ContainerWriter::new();
    let meta = w.add(Kind::PROJECT_META, b"gusset meta 1\nid 0123456789abcdef0123456789abcdef\n").unwrap();
    let feat = w.add(Kind::FEATURE, format!("gusset feature 1\nextrude {seq} mm\n").as_bytes()).unwrap();
    let blob: Vec<u8> = (0..3000u32).map(|i| (i % 7) as u8).collect();
    w.add(Kind::BLOB, &blob).unwrap();
    w.set_root("meta", meta.as_bytes()).unwrap();
    w.set_root("head", feat.as_bytes()).unwrap();
    w.finish(&SaveInfo { save_seq: seq, created_unix: 1_700_000_000, app_version: "0.0.1" }).unwrap()
}

/// Recursively lists every file under `dir`.
pub fn all_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            out.extend(all_files(&p));
        } else {
            out.push(p);
        }
    }
    out.sort();
    out
}
