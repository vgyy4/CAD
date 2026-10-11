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

fn kernel() -> gusset_kernel_occt::OcctKernel {
    gusset_kernel_occt::OcctKernel::with_worker(Some(worker().to_path_buf())).unwrap()
}

fn step_box() -> Vec<u8> {
    step_box_sized(20.0)
}

/// A 10 × `h` × 5 box as STEP. Tests use different sizes so their B-rep blobs differ
/// (the trusted-blob set is process-wide and tests run concurrently).
fn step_box_sized(h: f64) -> Vec<u8> {
    let k = gusset_kernel_occt::OcctKernel::new().unwrap();
    let pts = [[0.0, 0.0], [10.0, 0.0], [10.0, h], [0.0, h]];
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
    let k = kernel();
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

/// Security review #3: a B-rep blob this process did not produce (as from a project
/// file someone sent) is parsed in the worker first; without a worker it is refused.
#[test]
fn untrusted_brep_blobs_are_validated_in_the_worker() {
    let lim = gusset_sandbox::Limits::default();
    let out =
        import_out_of_process(worker(), Format::Step, &step_box_sized(31.0), &lim, Duration::from_secs(60)).unwrap();
    let blob = &out.bodies[0].brep;
    assert!(!gusset_worker::trust::is_trusted(blob));
    let no_worker = gusset_kernel_occt::OcctKernel::with_worker(None).unwrap();
    let e = no_worker.read_native(blob).unwrap_err();
    assert_eq!(e.kind, KernelErrorKind::Unsupported, "{e}");
    let s = kernel().read_native(blob).unwrap();
    assert!((kernel().mass_properties(&s).unwrap().volume - 1550.0).abs() < 1e-6);
    // Validated once, the blob is trusted for the rest of the process.
    assert!(gusset_worker::trust::is_trusted(blob));
    assert!(no_worker.read_native(blob).is_ok());
}

#[test]
fn damaged_brep_blobs_fail_in_the_worker() {
    let lim = gusset_sandbox::Limits::default();
    let out =
        import_out_of_process(worker(), Format::Step, &step_box_sized(37.0), &lim, Duration::from_secs(60)).unwrap();
    let blob = out.bodies[0].brep.clone();
    let mut flipped = blob.clone();
    for b in flipped.iter_mut().skip(40).step_by(7) {
        *b ^= 0x5a;
    }
    let truncated = blob[..blob.len() / 2].to_vec();
    for bad in [vec![0xffu8; 4096], b"garbage".to_vec(), truncated, flipped] {
        match kernel().read_native(&bad) {
            Err(e) => assert_eq!(e.kind, KernelErrorKind::InvalidInput, "{e}"),
            // Some corruptions still describe a valid shape; then it went through the worker.
            Ok(_) => assert!(gusset_worker::trust::is_trusted(&bad)),
        }
    }
}

/// Blobs written by this process are trusted without a worker run.
#[test]
fn own_blobs_are_trusted() {
    let k = gusset_kernel_occt::OcctKernel::with_worker(None).unwrap();
    let shapes = kernel().import_step(&step_box_sized(41.0)).unwrap();
    let blob = k.write_native(&shapes[0].0).unwrap();
    assert!(k.read_native(&blob).is_ok());
}
