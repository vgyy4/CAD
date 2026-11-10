//! Fusion design timelines as features (ADR 0039 amendment), against real files: runs when
//! `GUSSET_F3D_SAMPLES` names a folder of `.f3d`/`.f3z` files (searched recursively; not in the
//! repository). Each file either comes in as features that rebuild to its stored bodies, or as
//! the exact bodies with a reason; the counts and reasons are printed and no import may fail
//! outright. `GUSSET_F3D_MIN_HISTORY` asserts a floor for the history count, `GUSSET_F3D_ONLY`
//! keeps the files whose path contains the text, `GUSSET_F3D_VERBOSE` prints each file's outcome.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// The kind of a reason, without the file's own names, counts and numbers.
fn kind_of(why: &str) -> String {
    let mut out = String::new();
    for c in why.chars().take(160) {
        let digit = c.is_ascii_digit() || (c == '.' || c == '-') && out.ends_with('#');
        if digit && !out.ends_with('#') {
            out.push('#');
        } else if !digit {
            out.push(c);
        }
    }
    out
}

#[test]
fn fusion_designs_come_in_as_history_or_bodies() {
    let Ok(dir) = std::env::var("GUSSET_F3D_SAMPLES") else { return };
    let mut files = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("f3d") || x.eq_ignore_ascii_case("f3z")) {
                files.push(p);
            }
        }
    }
    files.sort();
    if let Ok(only) = std::env::var("GUSSET_F3D_ONLY") {
        files.retain(|f| f.to_string_lossy().contains(&only));
    }
    let worker = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/gusset-worker");
    let (mut history, mut shapes, mut failed) = (0, 0, 0);
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let verbose = std::env::var_os("GUSSET_F3D_VERBOSE").is_some();
    for f in &files {
        let k: Arc<dyn gusset_kernel::Kernel> =
            Arc::new(gusset_kernel_occt::OcctKernel::with_worker(Some(worker.clone())).unwrap());
        let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
        let mut s = Session::new(&doc, k, &CmdCtx::user(0));
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("f3d").to_ascii_lowercase();
        let name = format!("design.{ext}");
        let p = std::env::temp_dir().join(format!("gusset-f3d-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::copy(f, p.join(&name)).unwrap();
        s.project_dir = Some(p.clone());
        s.importer = Some(Arc::new(
            gusset_worker::WorkerImporter::new(worker.clone()).with_timeout(std::time::Duration::from_secs(300)),
        ));
        match execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": name})) {
            Ok(r) if r["tier"].as_str().unwrap_or_default().contains("Fusion design timeline") => {
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
                    .map(|w| kind_of(w.split_once("exact bodies: ").map_or(w, |x| x.1)))
                    .unwrap_or_else(|| "no feature history".into());
                if verbose {
                    if std::env::var_os("GUSSET_F3D_RAW").is_some() {
                        eprintln!("{}: raw {:?}", f.display(), r["warnings_user_text"]);
                    }
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
    if let Some(min) = std::env::var("GUSSET_F3D_MIN_HISTORY").ok().and_then(|v| v.parse::<usize>().ok()) {
        assert!(history >= min, "{history} files as history, at least {min} expected");
    }
}

/// Keys of a feature that are not part of what the importer reads (ids, notes, counts).
const IGNORED: [&str; 11] = [
    "source_id",
    "sketch_source_id",
    "note",
    "component",
    "in_timeline",
    "creates_bodies",
    "unsupported_entities",
    "extent_from",
    "plane_note",
    "extent",
    "profile_note",
];

fn same(a: &serde_json::Value, b: &serde_json::Value, path: &str, diffs: &mut Vec<String>) {
    use serde_json::Value as V;
    match (a, b) {
        (V::Number(x), V::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if (x - y).abs() > 1e-6 * (1.0 + x.abs()) {
                diffs.push(format!("{path}: {x} vs {y}"));
            }
        }
        (V::Object(x), V::Object(y)) => {
            for (k, v) in x.iter().filter(|(k, _)| !IGNORED.contains(&k.as_str()) && !k.starts_with('_')) {
                match y.get(k) {
                    Some(w) => same(v, w, &format!("{path}.{k}"), diffs),
                    None if v.is_null() => {}
                    None => diffs.push(format!("{path}.{k}: only in the prototype")),
                }
            }
            for (k, v) in y.iter().filter(|(k, _)| !IGNORED.contains(&k.as_str()) && !k.starts_with('_')) {
                if !x.contains_key(k) && !v.is_null() {
                    diffs.push(format!("{path}.{k}: only in the Rust tree"));
                }
            }
        }
        (V::Array(x), V::Array(y)) => {
            if x.len() != y.len() {
                diffs.push(format!("{path}: {} vs {} entries", x.len(), y.len()));
            }
            for (i, (v, w)) in x.iter().zip(y).enumerate() {
                same(v, w, &format!("{path}[{i}]"), diffs);
            }
        }
        _ if a == b => {}
        _ => diffs.push(format!("{path}: {a} vs {b}")),
    }
}

/// The Rust feature trees against the Python prototype's (`scripts/re/fusion/f3dfeat.py`): runs
/// when `GUSSET_F3D_SAMPLES` and `GUSSET_F3D_PROTO` (the prototype's output folder, one
/// `<file name>.json` per sample) are set. A design the prototype rebuilds in full must give the
/// same features here; the sketch placements of components that stand at their own origin only.
#[test]
fn fusion_trees_match_the_prototype() {
    let (Ok(dir), Ok(proto)) = (std::env::var("GUSSET_F3D_SAMPLES"), std::env::var("GUSSET_F3D_PROTO")) else { return };
    let mut compared = 0;
    let mut stack = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let Ok(want) = std::fs::read(std::path::Path::new(&proto).join(format!("{name}.json"))) else { continue };
            let want: serde_json::Value = serde_json::from_slice(&want).unwrap();
            let bytes = std::fs::read(&p).unwrap();
            let limits = gusset_meshio::Limits::default();
            let mut budget = limits.max_decompressed_bytes;
            let keep = |n: &str| n.ends_with("Stream.dat") || n.ends_with(".smb") || n.ends_with(".smbh");
            let entries =
                gusset_meshio::archive::read_entries_within(&bytes, &limits, &keep, 256 << 20, &mut budget).unwrap();
            let got =
                gusset_sat::fusion_tree::feature_tree_local(&entries, &name).unwrap_or_else(|e| panic!("{name}: {e}"));
            let mut diffs = Vec::new();
            same(&want["features"], &got["features"], "features", &mut diffs);
            same(&want["skipped"], &got["skipped"], "skipped", &mut diffs);
            assert!(
                diffs.is_empty(),
                "{name}: {} differences, the first: {:?}",
                diffs.len(),
                &diffs[..diffs.len().min(8)]
            );
            compared += 1;
            eprintln!("{name}: {} features equal to the prototype's", got["features"].as_array().unwrap().len());
        }
    }
    if let Ok(min) = std::env::var("GUSSET_F3D_MIN_PROTO").ok().map_or(Ok(0usize), |v| v.parse::<usize>()) {
        assert!(compared >= min, "{compared} designs compared, at least {min} expected");
    }
}

/// Writes each sample's unchecked feature tree to `<GUSSET_F3D_DUMP>/<file name>.json` (a reading
/// aid, runs when `GUSSET_F3D_SAMPLES` and `GUSSET_F3D_DUMP` are set; `GUSSET_F3D_ONLY` filters).
#[test]
fn fusion_trees_are_dumped_on_request() {
    let (Ok(dir), Ok(out)) = (std::env::var("GUSSET_F3D_SAMPLES"), std::env::var("GUSSET_F3D_DUMP")) else { return };
    let only = std::env::var("GUSSET_F3D_ONLY").ok();
    let mut stack = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if only.as_ref().is_some_and(|o| !p.to_string_lossy().contains(o.as_str())) {
                continue;
            }
            let bytes = std::fs::read(&p).unwrap();
            let limits = gusset_meshio::Limits::default();
            let mut budget = limits.max_decompressed_bytes;
            let keep = |n: &str| n.ends_with("Stream.dat") || n.ends_with(".smb") || n.ends_with(".smbh");
            type Entries = Vec<(String, Vec<u8>)>;
            let mut designs: Vec<(String, Entries)> = Vec::new();
            if let Ok(e) = gusset_meshio::archive::read_entries_within(&bytes, &limits, &keep, 256 << 20, &mut budget)
                && e.iter().any(|(n, _)| n.ends_with("Stream.dat"))
            {
                designs.push((name.clone(), e));
            } else if let Ok(inner) = gusset_meshio::archive::read_entries_within(
                &bytes,
                &limits,
                &|n| n.to_ascii_lowercase().ends_with(".f3d"),
                256 << 20,
                &mut budget,
            ) {
                // An `.f3z`: every design it holds, as `<file>.<design>.json`.
                for (n, data) in inner {
                    if let Ok(e) =
                        gusset_meshio::archive::read_entries_within(&data, &limits, &keep, 256 << 20, &mut budget)
                    {
                        designs.push((format!("{name}.{}", n.chars().take(8).collect::<String>()), e));
                    }
                }
            }
            for (name, entries) in designs {
                // What the reader alone says of the design (no rebuild): a quick tally aid.
                match gusset_sat::fusion_tree::feature_tree_local(&entries, &name) {
                    Err(e) => eprintln!("reader: {name}: {e}"),
                    Ok(_) => eprintln!("reader: {name}: reads in full"),
                }
                match gusset_sat::fusion_tree::feature_tree_unchecked(&entries, &name) {
                    Err(e) => eprintln!("{name}: {e}"),
                    Ok(tree) => {
                        std::fs::write(
                            std::path::Path::new(&out).join(format!("{name}.json")),
                            serde_json::to_vec_pretty(&tree).unwrap(),
                        )
                        .unwrap();
                    }
                }
            }
        }
    }
}
