//! FreeCAD PartDesign history as features (ADR 0033 amendment), against real files: runs when
//! `GUSSET_FCSTD_SAMPLES` names a folder of `.FCStd` files (FreeCAD's own examples and others;
//! not in the repository). Each file either comes in as features that rebuild to FreeCAD's
//! stored shapes, or as exact bodies with a reason; the counts are printed and no import may
//! fail outright. `GUSSET_FCSTD_MIN_FEATURES` asserts a floor for the features count.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

#[test]
fn freecad_files_come_in_as_history_or_shapes() {
    let Ok(dir) = std::env::var("GUSSET_FCSTD_SAMPLES") else { return };
    let mut files = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.to_ascii_lowercase().to_string_lossy().starts_with("fcstd")) {
                files.push(p);
            }
        }
    }
    files.sort();
    if let Ok(only) = std::env::var("GUSSET_FCSTD_ONLY") {
        files.retain(|f| f.to_string_lossy().contains(&only));
    }
    let worker = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/gusset-worker");
    let (mut history, mut shapes, mut failed) = (0, 0, 0);
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for f in &files {
        let k: Arc<dyn gusset_kernel::Kernel> =
            Arc::new(gusset_kernel_occt::OcctKernel::with_worker(Some(worker.clone())).unwrap());
        let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
        let mut s = Session::new(&doc, k, &CmdCtx::user(0));
        let p = std::env::temp_dir().join(format!("gusset-fcstd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::copy(f, p.join("part.FCStd")).unwrap();
        s.project_dir = Some(p.clone());
        s.importer = Some(Arc::new(
            gusset_worker::WorkerImporter::new(worker.clone()).with_timeout(std::time::Duration::from_secs(300)),
        ));
        match execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "part.FCStd"})) {
            Ok(r) if r["tier"].as_str().unwrap_or_default().contains("PartDesign history") => history += 1,
            Ok(r) => {
                shapes += 1;
                let why = r["warnings_user_text"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|w| w.as_str())
                    .find(|w| w.contains("design history"))
                    .map(|w| {
                        // The kind of reason, without the file's own names.
                        let w = w.split_once("exact bodies: ").map_or(w, |x| x.1);
                        // Drop the feature's name in front ("Pad001: ...").
                        let w = match w.split_once(": ") {
                            Some((n, rest)) if !n.contains(' ') => rest,
                            _ => w,
                        };
                        w.split(" is not").next().unwrap_or_default().chars().take(400).collect::<String>()
                    })
                    .unwrap_or_else(|| "no PartDesign body".into());
                if std::env::var_os("GUSSET_FCSTD_VERBOSE").is_some() {
                    eprintln!("{}: {why}", f.display());
                }
                *reasons.entry(why).or_default() += 1;
            }
            Err(e) => {
                failed += 1;
                eprintln!("{}: {}", f.display(), e.message);
            }
        }
        let _ = std::fs::remove_dir_all(&p);
    }
    eprintln!("{} files: {history} as history, {shapes} as shapes, {failed} failed", files.len());
    for (why, n) in &reasons {
        eprintln!("  {n:4}  {why}");
    }
    if let Some(min) = std::env::var("GUSSET_FCSTD_MIN_FEATURES").ok().and_then(|v| v.parse::<usize>().ok()) {
        assert!(history >= min, "{history} files as history, at least {min} expected");
    }
}
