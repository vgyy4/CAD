//! SolidWorks feature history as features (ADR 0043 amendment), against real parts: runs when
//! `GUSSET_SLDPRT_SAMPLES` names a folder of `.sldprt` files (not in the repository). Each part
//! either comes in as features that rebuild to its own Parasolid body, or as exact bodies with
//! a reason; the counts and reasons are printed (a part whose exact body cannot be read
//! either counts as failed).
//! The variable may name several folders (separated like `PATH`): the Slip-Pad parts (the older
//! OLE2 container) and the ARIG revision folders (`arig-revisions`, the newer chunk container, where
//! `plaque_base_roulante` and `plaque_superieure` are the parts that come in as history).
//! `GUSSET_SLDPRT_MIN_HISTORY` asserts a floor for the history count.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// The kind of a refusal, without the part's own names: "Fillet2: 2 of 3 fillet edges
/// resolved" -> "fillet edges resolved", "Hole1 (moHoleWzd_c) is not supported" ->
/// "moHoleWzd_c is not supported".
fn kind_of(why: &str) -> String {
    if let Some((_, rest)) = why.split_once(" (")
        && let Some((cls, tail)) = rest.split_once(") ")
    {
        return format!("{cls} {tail}");
    }
    if why.contains("could not be rebuilt") {
        return "features could not be rebuilt".into();
    }
    let w = why.rsplit_once(": ").map_or(why, |x| x.1);
    let w = w.split(',').next().unwrap_or_default();
    w.split_whitespace().filter(|t| *t != "of" && !t.chars().all(|c| c.is_ascii_digit())).collect::<Vec<_>>().join(" ")
}

#[test]
fn sldprt_parts_come_in_as_history_or_shapes() {
    let Ok(dir) = std::env::var("GUSSET_SLDPRT_SAMPLES") else { return };
    let mut files = Vec::new();
    let mut stack: Vec<std::path::PathBuf> = std::env::split_paths(&dir).collect();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sldprt")) {
                files.push(p);
            }
        }
    }
    files.sort();
    let only = std::env::var("GUSSET_SLDPRT_ONLY").ok();
    let worker = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/gusset-worker");
    let (mut history, mut shapes, mut failed) = (0, 0, 0);
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for f in files.iter().filter(|f| only.as_deref().is_none_or(|o| f.to_string_lossy().contains(o))) {
        let k: Arc<dyn gusset_kernel::Kernel> =
            Arc::new(gusset_kernel_occt::OcctKernel::with_worker(Some(worker.clone())).unwrap());
        let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
        let mut s = Session::new(&doc, k, &CmdCtx::user(0));
        let p = std::env::temp_dir().join(format!("gusset-swhist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::copy(f, p.join("part.SLDPRT")).unwrap();
        s.project_dir = Some(p.clone());
        s.importer = Some(Arc::new(
            gusset_worker::WorkerImporter::new(worker.clone()).with_timeout(std::time::Duration::from_secs(600)),
        ));
        let shown = f.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        match execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "part.SLDPRT"})) {
            Ok(r) if r["tier"].as_str().unwrap_or_default().contains("SolidWorks feature history") => {
                history += 1;
                let m = execute(&mut s, &CmdCtx::user(1), "query.model", json!({})).unwrap();
                let features = m["features"].as_array().map_or(0, Vec::len);
                eprintln!("{shown}: history ({features} features)");
            }
            Ok(r) => {
                shapes += 1;
                let why = r["warnings_user_text"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|w| w.as_str())
                    .find_map(|w| w.split_once("design history came in as exact bodies: ").map(|x| x.1.to_string()))
                    .unwrap_or_else(|| "no feature history".into());
                eprintln!("{shown}: shapes: {why}");
                *reasons.entry(kind_of(&why)).or_default() += 1;
            }
            Err(e) => {
                failed += 1;
                eprintln!("{shown}: failed: {}", e.message);
            }
        }
        let _ = std::fs::remove_dir_all(&p);
    }
    eprintln!("{} parts: {history} as history, {shapes} as shapes, {failed} failed", files.len());
    for (why, n) in &reasons {
        eprintln!("  {n:4}  {why}");
    }
    if let Some(min) = std::env::var("GUSSET_SLDPRT_MIN_HISTORY").ok().and_then(|v| v.parse::<usize>().ok()) {
        assert!(history >= min, "{history} parts as history, at least {min} expected");
    }
}
