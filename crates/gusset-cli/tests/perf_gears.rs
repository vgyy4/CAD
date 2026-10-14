//! Edit latency on a model with generated gears (dogfooding F3-5: `feature_delete` took
//! 11-18 s on a gear pair). With the real OCCT kernel; run in release mode and see
//! docs/PERFORMANCE.md:
//!
//! `cargo test --release -p gusset-cli --test perf_gears -- --ignored --nocapture`
//!
//! Build `gusset-kernel-worker` in release first to also time the kernel in its worker
//! process (ADR 0022): `cargo build --release -p gusset-kernel-worker`.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use gusset_doc::Document;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;

/// The kernel worker built next to the test's target directory, if any.
fn worker() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let w =
        exe.parent()?.parent()?.join(if cfg!(windows) { "gusset-kernel-worker.exe" } else { "gusset-kernel-worker" });
    w.is_file().then_some(w)
}

fn pool(workers: usize) -> Option<Arc<dyn gusset_kernel::Kernel>> {
    let w = worker()?;
    let gate = Arc::new(gusset_kernel_worker::OcctGate::next_to_exe());
    let members = (0..workers)
        .map(|_| {
            gusset_kernel_remote::RemoteKernel::new(
                "occt",
                gusset_kernel_worker::connector(w.clone(), 4 << 30),
                Box::new(gate.clone()),
            )
        })
        .collect();
    Some(Arc::new(gusset_kernel_remote::PoolKernel::new("occt", members)))
}

fn session(in_worker: bool) -> Session {
    let kernel: Arc<dyn gusset_kernel::Kernel> = match worker().filter(|_| in_worker) {
        Some(w) => Arc::new(gusset_kernel_remote::RemoteKernel::new(
            "occt",
            gusset_kernel_worker::connector(w, 4 << 30),
            Box::new(gusset_kernel_worker::OcctGate::next_to_exe()),
        )),
        None => Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}"))),
    };
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
    let modes: &[bool] = if worker().is_some() { &[false, true] } else { &[false] };
    for (&in_worker, helix) in modes.iter().flat_map(|m| [(m, "0 deg"), (m, "20 deg")]) {
        eprintln!("-- gear pair, helix {helix}, kernel {}", if in_worker { "in its worker" } else { "in process" });
        let mut s = session(in_worker);
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

/// A cold rebuild of a helical gear pair (two independent sweeps) on one worker and on a
/// pool of workers (ADR 0024 §3).
#[test]
#[ignore = "timing report; run in release"]
fn perf_gear_pair_cold_rebuild_on_a_pool() {
    let mut s = session(false);
    for teeth in ["20", "30"] {
        let r = execute(&mut s, &CmdCtx::user(1), "generate.gear", json!({"teeth": teeth, "helix": "20 deg"})).unwrap();
        assert_eq!(r["rebuild_ok"], true, "{r}");
    }
    let doc = s.document(&gusset_history::Actor::User).unwrap();
    for keep in [3usize, 6] {
        let mut part = doc.clone();
        part.features.truncate(keep);
        let Some(kernel) = pool(1) else { return };
        let t = Instant::now();
        let r = gusset_eval::Evaluator::new(kernel).evaluate(&part, None, &());
        eprintln!(
            "first {keep} features, 1 worker   {:>8.0} ms ({} errors)",
            t.elapsed().as_secs_f64() * 1e3,
            r.errors().len()
        );
    }
    for workers in [1, 2, 3] {
        let Some(kernel) = pool(workers) else { return };
        let t = Instant::now();
        let mut ev = gusset_eval::Evaluator::new(kernel);
        let r = ev.evaluate(&doc, None, &());
        eprintln!(
            "cold rebuild, {workers} worker(s)   {:>8.0} ms (built ahead: {})",
            t.elapsed().as_secs_f64() * 1e3,
            ev.speculated()
        );
        assert!(r.errors().is_empty() && r.body("Gear2").is_some(), "{:?}", r.errors());
    }
}
