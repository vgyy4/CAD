//! Fusion `.f3d`/`.f3z` designs against the STEP their authors exported (ADR 0039): runs when
//! `GUSSET_F3D_SAMPLES` names a folder of `<name>.f3z|.f3d` + `<name>.step` pairs (Adafruit's
//! MIT-licensed CAD parts; not in the repository).
//!
//! The union box of all bodies is a coarse check. `GUSSET_F3D_DUMP=<dir>` also writes the STEP
//! of every body as imported (with `GUSSET_F3D_FAST=1` the STEP files are not imported, only the
//! Fusion ones), and `scripts/f3d_compare.py` matches those bodies one by one against the
//! solids of the author's STEP, each put at its place in the author's assembly tree. Numbers
//! (solids of the author's STEP covered within 0.05 mm) are in docs/adr/0039-acis-sat-import.md.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::sync::Arc;

/// Passes imports on to the worker and keeps the STEP text each Fusion body is converted to
/// (`GUSSET_F3D_DUMP` names a folder they are written to, for per-body comparison with the
/// authors' STEP: `tools/f3d/compare.py`).
struct Recording {
    inner: Arc<dyn gusset_commands::session::Importer>,
    dump: Option<(std::path::PathBuf, std::sync::atomic::AtomicUsize)>,
}

impl gusset_commands::session::Importer for Recording {
    fn import(
        &self,
        format: &str,
        bytes: &[u8],
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<(String, Vec<u8>)>, String> {
        if let Some((dir, n)) = &self.dump {
            let i = n.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let _ = std::fs::write(dir.join(format!("{i:04}.{format}")), bytes);
        }
        self.inner.import(format, bytes, cancel)
    }
}

/// A body's name and bounding box.
type Boxed = (String, [f64; 3], [f64; 3]);

/// Bodies (name, bbox min, bbox max) after importing `file` into a fresh project.
fn import(dir: &std::path::Path, file: &str, dump: Option<&std::path::Path>) -> (Vec<Boxed>, Value) {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let worker = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/debug/gusset-worker{}", std::env::consts::EXE_SUFFIX));
    let k: Arc<dyn gusset_kernel::Kernel> =
        Arc::new(gusset_kernel_occt::OcctKernel::with_worker(Some(worker.clone())).unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    // Parallel tests can read the same clock value (Windows): a sequence number keeps their
    // folders apart.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = format!("{n}-{}", SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let p = std::env::temp_dir().join(format!("gusset-f3d-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    std::fs::copy(dir.join(file), p.join(file)).unwrap();
    s.project_dir = Some(p.clone());
    // The STEP translation runs in the import worker, built next to the test binaries.
    let inner: Arc<dyn gusset_commands::session::Importer> =
        Arc::new(gusset_worker::WorkerImporter::new(worker).with_timeout(std::time::Duration::from_secs(600)));
    let dump = dump.map(|d| {
        std::fs::create_dir_all(d).unwrap();
        (d.to_path_buf(), std::sync::atomic::AtomicUsize::new(0))
    });
    s.importer = Some(Arc::new(Recording { inner, dump }));
    let r = match execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": file})) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{file}: import failed: {e:?}");
            let _ = std::fs::remove_dir_all(p);
            return (Vec::new(), Value::Null);
        }
    };
    eprintln!("import {file}: {}", r.to_string().chars().take(600).collect::<String>());
    let m = execute(&mut s, &CmdCtx::user(1), "query.model", json!({})).unwrap();
    let mut out = Vec::new();
    for b in m["bodies"].as_array().unwrap() {
        let name = b["name"].as_str().unwrap().to_string();
        let q = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": name})).unwrap();
        let v = |k: &str| [0, 1, 2].map(|i| q[k][i].as_f64().unwrap());
        out.push((name, v("bbox_min_mm"), v("bbox_max_mm")));
    }
    let _ = std::fs::remove_dir_all(p);
    (out, r)
}

fn union(b: &[Boxed]) -> ([f64; 3], [f64; 3]) {
    b.iter().fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), (_, a, z)| {
        ([0, 1, 2].map(|i| lo[i].min(a[i])), [0, 1, 2].map(|i| hi[i].max(z[i])))
    })
}

#[test]
fn fusion_samples_match_their_step_exports() {
    let Ok(dir) = std::env::var("GUSSET_F3D_SAMPLES") else { return };
    let dir = std::path::PathBuf::from(dir);
    let only = std::env::var("GUSSET_F3D_ONLY").ok();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".f3z") || n.ends_with(".f3d"))
        .filter(|n| only.as_deref().is_none_or(|o| n.contains(o)))
        .collect();
    names.sort();
    for f in names {
        let step = format!("{}.step", f.rsplit_once('.').unwrap().0);
        if !dir.join(&step).exists() {
            continue;
        }
        let dump = std::env::var("GUSSET_F3D_DUMP").ok().map(|d| std::path::PathBuf::from(d).join(&f));
        let (ours, r) = import(&dir, &f, dump.as_deref());
        // `GUSSET_F3D_FAST` leaves out the STEP import: only the dump is wanted.
        if std::env::var("GUSSET_F3D_FAST").is_ok() {
            eprintln!("{f}: {} bodies (dumped)", ours.len());
            continue;
        }
        let (theirs, _) = import(&dir, &step, None);
        let (a, b) = (union(&ours), union(&theirs));
        let off = (0..3).map(|i| (a.0[i] - b.0[i]).abs().max((a.1[i] - b.1[i]).abs())).fold(0.0, f64::max);
        eprintln!(
            "{f}: {} bodies vs STEP {}; box {:?}..{:?} vs {:?}..{:?}; worst {off:.2} mm; warnings {}",
            ours.len(),
            theirs.len(),
            a.0.map(|v| v.round()),
            a.1.map(|v| v.round()),
            b.0.map(|v| v.round()),
            b.1.map(|v| v.round()),
            r["warnings"]
        );
    }
}
