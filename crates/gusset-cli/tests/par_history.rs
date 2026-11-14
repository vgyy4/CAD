//! Solid Edge part feature history as features (ADR 0046 amendment), against real files: runs
//! when `GUSSET_PAR_SAMPLES` names a folder of `.par` files (searched recursively; not in the
//! repository). Each part either comes in as features that rebuild to its own result body, or
//! as the exact body with a reason; the counts and reasons are printed and no import may fail
//! outright (a synchronous part keeps no history: it counts as a body). `GUSSET_PAR_MIN_HISTORY` asserts a floor for the history count,
//! `GUSSET_PAR_ONLY` keeps the files whose path contains the text, `GUSSET_PAR_VERBOSE`
//! prints each file's outcome.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

#[test]
fn solid_edge_parts_come_in_as_history_or_bodies() {
    let Ok(dir) = std::env::var("GUSSET_PAR_SAMPLES") else { return };
    let mut files = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("par")) {
                files.push(p);
            }
        }
    }
    files.sort();
    if let Ok(only) = std::env::var("GUSSET_PAR_ONLY") {
        files.retain(|f| f.to_string_lossy().contains(&only));
    }
    let worker = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/debug/gusset-worker{}", std::env::consts::EXE_SUFFIX));
    let (mut history, mut shapes, mut failed) = (0, 0, 0);
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for f in &files {
        let k: Arc<dyn gusset_kernel::Kernel> =
            Arc::new(gusset_kernel_occt::OcctKernel::with_worker(Some(worker.clone())).unwrap());
        let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
        let mut s = Session::new(&doc, k, &CmdCtx::user(0));
        let p = std::env::temp_dir().join(format!("gusset-par-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::copy(f, p.join("part.par")).unwrap();
        s.project_dir = Some(p.clone());
        s.importer = Some(Arc::new(
            gusset_worker::WorkerImporter::new(worker.clone()).with_timeout(std::time::Duration::from_secs(900)),
        ));
        let verbose = std::env::var_os("GUSSET_PAR_VERBOSE").is_some();
        match execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "part.par"})) {
            Ok(r) if r["tier"].as_str().unwrap_or_default().contains("Solid Edge feature history") => {
                history += 1;
                if verbose {
                    eprintln!("{}: history {}", f.display(), r["warnings_user_text"]);
                }
            }
            Ok(r) => {
                shapes += 1;
                let why = r["warnings_user_text"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|w| w.as_str())
                    .find(|w| w.contains("design history"))
                    .map(|w| {
                        // The kind of reason, without the file's own names, counts and numbers.
                        let w = w.split_once("exact body: ").map_or(w, |x| x.1);
                        let w =
                            w.split_once("\"reason\":\"").map_or(w, |x| x.1.split("\"}").next().unwrap_or_default());
                        let w = w.split_once("cannot be read: ").map_or(w, |x| x.1);
                        let mut out = String::new();
                        for c in w.chars().take(200) {
                            let digit = c.is_ascii_digit() || (c == '.' || c == '-') && out.ends_with('#');
                            if digit && !out.ends_with('#') {
                                out.push('#');
                            } else if !digit {
                                out.push(c);
                            }
                        }
                        out
                    })
                    .unwrap_or_else(|| "no feature history".into());
                if verbose {
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
    eprintln!("{} files: {history} as history, {shapes} as bodies, {failed} failed", files.len());
    for (why, n) in &reasons {
        eprintln!("  {n:4}  {why}");
    }
    assert_eq!(failed, 0, "{failed} files did not import at all");
    if let Some(min) = std::env::var("GUSSET_PAR_MIN_HISTORY").ok().and_then(|v| v.parse::<usize>().ok()) {
        assert!(history >= min, "{history} files as history, at least {min} expected");
    }
}
