//! SolidWorks assembly import (ADR 0043 amendment): runs when `GUSSET_SLDASM_SAMPLES` names the
//! folder with NIST's single-die nanocalorimeter assembly and its three parts (public domain, not in
//! the repository). The three layers must land stacked: the placed bounding boxes touch.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

const ASM: &str = "NIST_Nanocalorimeter_single_die_3D_model.SLDASM";

/// Imports the assembly from a copy of the sample folder without the files `skip` names.
/// A body's label and bounding box corners (mm).
type Placed = (String, [f64; 3], [f64; 3]);

fn import(src: &Path, skip: &[&str]) -> (Value, Vec<Placed>) {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let worker = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/debug/gusset-worker{}", std::env::consts::EXE_SUFFIX));
    let k: Arc<dyn gusset_kernel::Kernel> =
        Arc::new(gusset_kernel_occt::OcctKernel::with_worker(Some(worker.clone())).unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    let p = std::env::temp_dir().join(format!("gusset-sldasm-{}-{}", std::process::id(), skip.len()));
    std::fs::create_dir_all(&p).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        let up = n.to_ascii_uppercase();
        if (up.ends_with(".SLDPRT") || up.ends_with(".SLDASM")) && !skip.iter().any(|s| n.contains(s)) {
            std::fs::copy(e.path(), p.join(&n)).unwrap();
        }
    }
    s.project_dir = Some(p.clone());
    s.importer =
        Some(Arc::new(gusset_worker::WorkerImporter::new(worker).with_timeout(std::time::Duration::from_secs(600))));
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": ASM})).unwrap();
    let m = execute(&mut s, &CmdCtx::user(1), "query.model", json!({})).unwrap();
    let text = execute(&mut s, &CmdCtx::user(1), "document.get_text", json!({})).unwrap()["text_user_text"]
        .as_str()
        .unwrap()
        .to_string();
    // The body's feature name and its label (the component's name).
    let label = |feature: &str| -> String {
        let line = text.lines().find(|l| l.starts_with(&format!("import {feature} "))).unwrap();
        line.split("label: \"").nth(1).unwrap().trim_end_matches(')').trim_end_matches('"').to_string()
    };
    let mut out = Vec::new();
    for b in m["bodies"].as_array().unwrap() {
        let feature = b["name"].as_str().unwrap().to_string();
        let name = label(&feature);
        let q = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": feature})).unwrap();
        let v = |k: &str| [0, 1, 2].map(|i| q[k][i].as_f64().unwrap());
        out.push((name, v("bbox_min_mm"), v("bbox_max_mm")));
    }
    let _ = std::fs::remove_dir_all(p);
    (r, out)
}

#[test]
fn the_nist_layers_stack() {
    let Ok(dir) = std::env::var("GUSSET_SLDASM_SAMPLES") else { return };
    let src = Path::new(&dir);
    if !src.join(ASM).exists() {
        return;
    }
    let (r, bodies) = import(src, &[]);
    assert_eq!(bodies.len(), 3, "{bodies:?} {r}");
    let by = |k: &str| bodies.iter().find(|b| b.0.contains(k)).unwrap_or_else(|| panic!("{k} in {bodies:?}")).clone();
    let (die, nitride, metal) = (by("die layer"), by("silicon nitride"), by("top metal"));
    // The two virtual components are read from the assembly file; their parts hold no solid.
    let w = r["warnings_user_text"].to_string();
    assert!(w.contains("Part1^") && w.contains("Part2^") && !w.contains("not stored"), "{w}");
    // The die is 0.5 mm thick; the two thin layers lie on its underside (z of its minimum),
    // inside its outline.
    assert!((die.2[2] - die.1[2] - 0.5).abs() < 1e-3, "{die:?}");
    for layer in [&nitride, &metal] {
        assert!((layer.2[2] - die.1[2]).abs() < 1e-3 && (layer.1[2] - die.1[2]).abs() < 1e-3, "{layer:?} {die:?}");
        for i in 0..2 {
            assert!(layer.1[i] >= die.1[i] - 1e-3 && layer.2[i] <= die.2[i] + 1e-3, "axis {i}: {layer:?} {die:?}");
        }
    }
    // A missing part is a warning, the others still import.
    let (r, bodies) = import(src, &["top_metal"]);
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    assert!(r["warnings_user_text"].to_string().contains("top metal layer-1"), "{}", r["warnings_user_text"]);
}
