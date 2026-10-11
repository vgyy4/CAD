//! Runs the real sandboxed worker binary (needs OCCT).
#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use gusset_kernel::*;
use gusset_worker::{Format, ImportError, import_out_of_process};
use std::path::Path;
use std::time::Duration;

fn worker() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_gusset-worker"))
}

fn step_box() -> Vec<u8> {
    let k = gusset_kernel_occt::OcctKernel::new().unwrap();
    let pts = [[0.0, 0.0], [10.0, 0.0], [10.0, 20.0], [0.0, 20.0]];
    let edges =
        (0..4).map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 4] }, tag: i as u64 }).collect();
    let p = Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] };
    let r = k.extrude(&p, &ExtrudeSpec { distance: 5.0, symmetric: false }).unwrap();
    k.export_step(&[(&r.shape, "Box")]).unwrap()
}

#[test]
fn imports_step_in_sandboxed_worker() {
    let lim = gusset_sandbox::Limits::default();
    let out = import_out_of_process(worker(), Format::Step, &step_box(), &lim, Duration::from_secs(60)).unwrap();
    assert_eq!(out.bodies.len(), 1);
    assert!(out.sandbox_report.contains("applied"), "{}", out.sandbox_report);
    let k = gusset_kernel_occt::OcctKernel::new().unwrap();
    let s = k.read_native(&out.bodies[0].brep).unwrap();
    assert!((k.mass_properties(&s).unwrap().volume - 1000.0).abs() < 1e-6);
}

#[test]
fn hostile_inputs_fail_cleanly() {
    let lim = gusset_sandbox::Limits::default();
    for junk in [&b""[..], b"ISO-10303-21;\nHEADER;\0\0\xff\xff garbage", &[0xffu8; 4096][..]] {
        let r = import_out_of_process(worker(), Format::Step, junk, &lim, Duration::from_secs(60));
        assert!(matches!(r, Err(ImportError::Failed(_) | ImportError::Crashed(_))), "{r:?}");
    }
    let r = import_out_of_process(worker(), Format::Iges, b"not iges", &lim, Duration::from_secs(60));
    assert!(r.is_err());
}

#[test]
fn timeout_kills_the_worker() {
    let lim = gusset_sandbox::Limits::default();
    let big = step_box().repeat(200);
    let r = import_out_of_process(worker(), Format::Step, &big, &lim, Duration::from_millis(1));
    assert!(matches!(r, Err(ImportError::TimedOut) | Err(ImportError::Failed(_)) | Ok(_)), "{r:?}");
}

#[test]
fn relative_worker_path_is_refused() {
    let r = import_out_of_process(
        Path::new("gusset-worker"),
        Format::Step,
        b"x",
        &gusset_sandbox::Limits::default(),
        Duration::from_secs(1),
    );
    assert!(matches!(r, Err(ImportError::Spawn(_))));
}

/// Review H5 / dogfood F-B1: STEP import end to end through a session, the host's
/// off-thread import path and the sandboxed worker, as the CLI and the app wire it.
#[test]
fn step_import_end_to_end_through_a_session() {
    use gusset_commands::{CmdCtx, Session};
    let dir = std::env::temp_dir().join(format!("gusset-e2e-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("box.step"), step_box()).unwrap();
    let k = std::sync::Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    s.importer = Some(std::sync::Arc::new(gusset_worker::WorkerImporter::new(worker().to_path_buf())));
    s.project_dir = Some(dir.clone());
    let host = gusset_host::Host::spawn(s, None);
    let a = CmdCtx::agent("e2e", 1);
    let r = host.call(a.clone(), "import.file", serde_json::json!({"path": "box.step"})).unwrap();
    assert_eq!(r["features"].as_array().unwrap().len(), 1, "{r}");
    let b = host.call(a, "query.body", serde_json::json!({})).unwrap();
    assert!((b["volume_mm3"].as_f64().unwrap() - 1000.0).abs() < 1e-6, "{b}");
    let _ = host.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
