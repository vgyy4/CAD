//! SolidWorks `.sldprt` parts (ADR 0043): runs when `GUSSET_SLDPRT_SAMPLES` names a folder
//! with `blussyya/SW<year>/<model>/model.SLDPRT` + `model.step` pairs (the STEP SolidWorks
//! exported is the ground truth: volume within 1e-3, bounding box within 0.01 mm) and
//! any other `.sldprt` files below it, which must import with at least one solid. The
//! samples are not in the repository.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Bodies' total volume and union bounding box after importing `path` into a fresh project.
struct Imported {
    bodies: usize,
    volume: f64,
    lo: [f64; 3],
    hi: [f64; 3],
    warnings: Value,
}

fn import(path: &Path) -> Result<Imported, String> {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let worker = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/gusset-worker");
    let k: Arc<dyn gusset_kernel::Kernel> =
        Arc::new(gusset_kernel_occt::OcctKernel::with_worker(Some(worker.clone())).unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let p = std::env::temp_dir().join(format!("gusset-sldprt-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    let file = path.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::copy(path, p.join(&file)).unwrap();
    s.project_dir = Some(p.clone());
    s.importer =
        Some(Arc::new(gusset_worker::WorkerImporter::new(worker).with_timeout(std::time::Duration::from_secs(600))));
    let mut run = || -> Result<Imported, String> {
        let r =
            execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": file})).map_err(|e| format!("{e:?}"))?;
        let m = execute(&mut s, &CmdCtx::user(1), "query.model", json!({})).map_err(|e| format!("{e:?}"))?;
        let mut out =
            Imported { bodies: 0, volume: 0.0, lo: [f64::MAX; 3], hi: [f64::MIN; 3], warnings: r["warnings"].clone() };
        for b in m["bodies"].as_array().unwrap() {
            let name = b["name"].as_str().unwrap().to_string();
            let q =
                execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": name})).map_err(|e| format!("{e:?}"))?;
            let v = |k: &str| [0, 1, 2].map(|i| q[k][i].as_f64().unwrap());
            out.bodies += 1;
            out.volume += q["volume_mm3"].as_f64().unwrap_or(0.0);
            let (a, z) = (v("bbox_min_mm"), v("bbox_max_mm"));
            for i in 0..3 {
                out.lo[i] = out.lo[i].min(a[i]);
                out.hi[i] = out.hi[i].max(z[i]);
            }
        }
        Ok(out)
    };
    let r = run();
    let _ = std::fs::remove_dir_all(p);
    r
}

/// The STEP SolidWorks exported for `part`: `model.step` next to a blussyya `model.SLDPRT`, or
/// NIST's `step/<test case>_ap242*.stp` for the NIST parts.
fn ground_truth(part: &Path) -> Option<(PathBuf, bool)> {
    let own = part.with_extension("step");
    if own.exists() {
        return Some((own, true));
    }
    let stem = part.file_stem()?.to_str()?;
    let case = stem.strip_prefix("nist_")?.split("_asme1").next()?;
    let dir = part.parent()?.parent()?.join("step");
    let step = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&format!("nist_{case}_asme1_ap242")))
    });
    Some((step?, false))
}

fn find_sldprt(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            find_sldprt(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sldprt")) {
            out.push(p);
        }
    }
}

#[test]
fn sldprt_samples_match_their_step_exports() {
    let Ok(root) = std::env::var("GUSSET_SLDPRT_SAMPLES") else { return };
    let root = PathBuf::from(root);
    let mut files = Vec::new();
    find_sldprt(&root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no .sldprt files below {}", root.display());
    let only = std::env::var("GUSSET_SLDPRT_ONLY").ok();
    let mut failures = Vec::new();
    for f in files.iter().filter(|f| only.as_deref().is_none_or(|o| f.to_string_lossy().contains(o))) {
        let shown = f.strip_prefix(&root).unwrap_or(f).display().to_string();
        let ours = match import(f) {
            Ok(o) if o.bodies >= 1 => o,
            Ok(_) => {
                failures.push(format!("{shown}: no solid"));
                continue;
            }
            Err(e) => {
                failures.push(format!("{shown}: import failed: {}", e.chars().take(300).collect::<String>()));
                continue;
            }
        };
        let Some((step, strict)) = ground_truth(f) else {
            eprintln!(
                "{shown}: {} bodies, volume {:.3} mm3, box {:?}..{:?}",
                ours.bodies, ours.volume, ours.lo, ours.hi
            );
            continue;
        };
        let theirs = import(&step).unwrap();
        let dv = (ours.volume - theirs.volume).abs() / theirs.volume.abs().max(1e-9);
        let db = (0..3)
            .map(|i| (ours.lo[i] - theirs.lo[i]).abs().max((ours.hi[i] - theirs.hi[i]).abs()))
            .fold(0.0, f64::max);
        eprintln!(
            "{shown}: {} bodies vs STEP {}; volume {:.4} vs {:.4} (rel {dv:.2e}); box off {db:.4} mm; warnings {}",
            ours.bodies, theirs.bodies, ours.volume, theirs.volume, ours.warnings
        );
        // NIST's AP242 file is NIST's own export of its model, not SolidWorks' export of this
        // part: the two differ by a revision (informational only).
        if strict && (ours.bodies != theirs.bodies || dv > 1e-3 || db > 0.01) {
            failures.push(format!(
                "{shown}: bodies {}/{}, volume rel {dv:.2e}, box {db:.4} mm",
                ours.bodies, theirs.bodies
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
