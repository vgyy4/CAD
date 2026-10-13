//! Edit latency on a model with generated gears (dogfooding F3-5: `feature_delete` took
//! 11-18 s on a gear pair). With the real OCCT kernel; run in release mode and see
//! docs/PERFORMANCE.md:
//!
//! `cargo test --release -p gusset-cli --test perf_gears -- --ignored --nocapture`

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use gusset_doc::Document;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;

fn session() -> Session {
    let kernel: Arc<dyn gusset_kernel::Kernel> =
        Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")));
    Session::new(&Document::from_text("gusset 1\nunits mm\n").unwrap(), kernel, &CmdCtx::user(0))
}

fn timed(s: &mut Session, what: &str, id: &str, args: Value) -> Value {
    let t = Instant::now();
    let r = execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"));
    eprintln!("{what:<34} {:>8.0} ms", t.elapsed().as_secs_f64() * 1e3);
    r
}

#[test]
#[ignore = "timing report; run in release mode"]
fn perf_gears_report() {
    for helix in ["0 deg", "20 deg"] {
        eprintln!("-- gear pair, helix {helix}");
        let mut s = session();
        let r = timed(&mut s, "generate.gear 20 teeth", "generate.gear", json!({"teeth": "20", "helix": helix}));
        assert_eq!(r["rebuild_ok"], true, "{r}");
        let r = timed(&mut s, "generate.gear 30 teeth", "generate.gear", json!({"teeth": "30", "helix": helix}));
        assert_eq!(r["rebuild_ok"], true, "{r}");
        timed(&mut s, "param.set (no geometry change)", "param.set", json!({"name": "x", "expr": "1 mm"}));
        timed(&mut s, "query.body", "query.body", json!({"body": "Gear2"}));
        timed(&mut s, "feature.delete Gear2", "feature.delete", json!({"name": "Gear2"}));
        timed(&mut s, "history.undo", "history.undo", json!({}));
        timed(&mut s, "query.model after undo", "query.model", json!({}));
    }
}
