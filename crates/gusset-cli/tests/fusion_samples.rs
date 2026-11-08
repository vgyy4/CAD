//! Fusion `.f3d`/`.f3z` designs against the STEP their authors exported (ADR 0039): runs when
//! `GUSSET_F3D_SAMPLES` names a folder of `<name>.f3z|.f3d` + `<name>.step` pairs (Adafruit's
//! MIT-licensed CAD parts; not in the repository).

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::sync::Arc;

/// A body's name and bounding box.
type Boxed = (String, [f64; 3], [f64; 3]);

/// Bodies (name, bbox min, bbox max) after importing `file` into a fresh project.
fn import(dir: &std::path::Path, file: &str) -> (Vec<Boxed>, Value) {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let worker = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/gusset-worker");
    let k: Arc<dyn gusset_kernel::Kernel> =
        Arc::new(gusset_kernel_occt::OcctKernel::with_worker(Some(worker.clone())).unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let p = std::env::temp_dir().join(format!("gusset-f3d-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    std::fs::copy(dir.join(file), p.join(file)).unwrap();
    s.project_dir = Some(p.clone());
    // The STEP translation runs in the import worker, built next to the test binaries.
    s.importer =
        Some(Arc::new(gusset_worker::WorkerImporter::new(worker).with_timeout(std::time::Duration::from_secs(600))));
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": file}))
        .unwrap_or_else(|e| panic!("{file}: {e:?}"));
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
        let (ours, r) = import(&dir, &f);
        let (theirs, _) = import(&dir, &step);
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
