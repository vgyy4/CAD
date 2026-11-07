//! Feature-tree import (ADR 0042) with the real OCCT kernel: each exporter plugin's sample
//! file imports as features whose volume matches the volume the sample states.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::sync::Arc;

fn session() -> Session {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("gusset-tree-occt-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    s.project_dir = Some(dir);
    s
}

#[test]
fn the_plugin_samples_import_with_their_stated_volumes() {
    for sample in ["fusion", "onshape", "solidworks"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../plugins/{sample}/sample/expected.gtree.json"));
        let bytes = std::fs::read(&path).unwrap();
        let mut s = session();
        std::fs::write(s.project_dir.clone().unwrap().join("s.gtree.json"), &bytes).unwrap();
        let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "s.gtree.json"}))
            .unwrap_or_else(|e| panic!("{sample}: {e:?}"));
        eprintln!("{sample}: {r}");
        let t: Value = serde_json::from_slice(&bytes).unwrap();
        let unsupported = t["features"].as_array().unwrap().iter().filter(|f| f["type"] == "unsupported").count();
        assert_eq!(r["skipped_user_text"].as_array().unwrap().len(), unsupported, "{sample}: {r}");
        let v = &r["volume_mm3"];
        let (src, ours) = (v["source"].as_f64().unwrap(), v["imported"].as_f64().unwrap());
        assert!((src - ours).abs() < 1e-3 * src, "{sample}: {r}");
    }
}
