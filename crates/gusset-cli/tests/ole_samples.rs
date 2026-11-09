//! Solid Edge (`.par`, `.psm`) and Inventor (`.ipt`) parts (ADR 0046): runs when
//! `GUSSET_OLE_SAMPLES` names a folder (searched recursively) with such files. Each must import
//! with at least one solid. Next to a file, a STEP of the same name (`.step`/`.stp`, any case)
//! is the ground truth (volume within 1e-6 relative, bounding box within 1e-6 mm; pairs that
//! are known to be two revisions of the model are only reported), and a `.sldprt` whose name
//! matches ignoring case, spaces, `-` and `_` is a cross-check (reported). The samples are not
//! in the repository.

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
    // Parallel tests can read the same clock value (Windows): a sequence number keeps their
    // folders apart.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = format!("{n}-{}", SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let p = std::env::temp_dir().join(format!("gusset-ole-{}-{n}", std::process::id()));
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

/// Case, spaces, `-` and `_` do not count when matching names.
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

/// The files next to `part` with the same normalised stem and one of `exts` (lower case).
fn siblings(part: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let Some(stem) = part.file_stem().and_then(|s| s.to_str()).map(norm) else { return Vec::new() };
    let Some(dir) = part.parent() else { return Vec::new() };
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p != part
                && p.extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| exts.contains(&x.to_ascii_lowercase().as_str()))
                && p.file_stem().and_then(|x| x.to_str()).is_some_and(|x| norm(x) == stem)
        })
        .collect();
    found.sort();
    found
}

fn find(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            find(&p, out);
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| ["par", "psm", "ipt"].contains(&x.to_ascii_lowercase().as_str()))
        {
            out.push(p);
        }
    }
}

fn compare(ours: &Imported, theirs: &Imported) -> (f64, f64) {
    let dv = (ours.volume - theirs.volume).abs() / theirs.volume.abs().max(1e-9);
    let db =
        (0..3).map(|i| (ours.lo[i] - theirs.lo[i]).abs().max((ours.hi[i] - theirs.hi[i]).abs())).fold(0.0, f64::max);
    (dv, db)
}

/// Pairs known to be different revisions of the model (the STEP was exported before the
/// part was last saved, or from another configuration): reported, not asserted.
/// (`top cover 2`: both STEP files agree with each other and differ from the part by 4e-3;
/// `Top motor plate`: 1.5 mm3 of 61109, the area differs by 0.8 mm2, the box is identical.)
const REVISION_PAIRS: &[&str] = &["top cover 2", "Top motor plate"];

/// Parts that use Parasolid node types of schema 31100 that gusset-xt does not know yet
/// (ADR 0046): they are reported as errors, not silently skipped.
const UNKNOWN_NODE_TYPES: &[&str] = &["M1.2x3 screw", "Plastic-handle-Loaded-Felfer-holder"];

#[test]
fn solid_edge_and_inventor_parts_import() {
    let Ok(root) = std::env::var("GUSSET_OLE_SAMPLES") else { return };
    let root = PathBuf::from(root);
    let mut files = Vec::new();
    find(&root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no .par/.psm/.ipt files below {}", root.display());
    let only = std::env::var("GUSSET_OLE_ONLY").ok();
    let mut failures = Vec::new();
    for f in files.iter().filter(|f| only.as_deref().is_none_or(|o| f.to_string_lossy().contains(o))) {
        let shown = f.strip_prefix(&root).unwrap_or(f).display().to_string();
        let ours = match import(f) {
            Ok(o) if o.bodies >= 1 && o.volume > 0.0 => o,
            Ok(o) => {
                failures.push(format!("{shown}: {} bodies, volume {}", o.bodies, o.volume));
                continue;
            }
            Err(e) => {
                let known = UNKNOWN_NODE_TYPES.iter().any(|u| shown.contains(u));
                eprintln!(
                    "{shown}: import failed{}: {}",
                    if known { " (expected)" } else { "" },
                    e.chars().take(200).collect::<String>()
                );
                if !known {
                    failures.push(format!("{shown}: import failed: {}", e.chars().take(300).collect::<String>()));
                }
                continue;
            }
        };
        eprintln!(
            "{shown}: {} bodies, volume {:.3} mm3, box {:.3}..{:.3}",
            ours.bodies, ours.volume, ours.lo[0], ours.hi[0]
        );
        if ours.warnings.as_array().is_some_and(|w| !w.is_empty()) {
            eprintln!("    warnings: {}", ours.warnings);
        }
        for step in siblings(f, &["step", "stp"]) {
            let theirs = import(&step).unwrap();
            let (dv, db) = compare(&ours, &theirs);
            eprintln!(
                "    vs {}: {} bodies; volume {:.4} vs {:.4} (rel {dv:.2e}); box off {db:.2e} mm",
                step.file_name().unwrap().to_string_lossy(),
                theirs.bodies,
                ours.volume,
                theirs.volume
            );
            let revision = REVISION_PAIRS.iter().any(|r| shown.contains(r));
            if !revision && (ours.bodies != theirs.bodies || dv > 1e-6 || db > 1e-6) {
                failures.push(format!(
                    "{shown}: bodies {}/{}, volume rel {dv:.2e}, box {db:.2e} mm",
                    ours.bodies, theirs.bodies
                ));
            }
        }
        for sw in siblings(f, &["sldprt", "f3d"]) {
            let theirs = import(&sw).unwrap();
            let (dv, db) = compare(&ours, &theirs);
            eprintln!(
                "    vs {} (cross-check): {} bodies; volume {:.4} vs {:.4} (rel {dv:.2e}); box off {db:.2e} mm",
                sw.file_name().unwrap().to_string_lossy(),
                theirs.bodies,
                ours.volume,
                theirs.volume
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
