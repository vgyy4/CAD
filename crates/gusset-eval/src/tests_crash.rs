//! A kernel worker that dies during a feature (ADR 0022 §2), simulated with the mock
//! kernel: the feature fails with `kernel_crashed`, the rest of the model is rebuilt, and
//! the crash is not repeated until the feature changes.

use super::*;
use gusset_kernel::mock::MockKernel;
use gusset_kernel::{BBox, KernelErrorKind, MassProps, Mesh, Profile, TessellationParams};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const PLATE: &str = "gusset 1
units mm
sketch S1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(10, 0)
  p3 = point(10, 10)
  p4 = point(0, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}
extrude E1 (profile: S1, distance: 5 mm, op: new)
";

/// Crashes (bumps its epoch and fails with `Crashed`) on extrudes of exactly `crash_at` mm.
/// The first extrude of 6 mm is stopped like a worker killed by "Stop rebuild": it sets
/// the caller's cancel flag ([`gusset_kernel::cancel`]), bumps the epoch and fails with
/// `Cancelled`.
#[derive(Debug)]
struct CrashingKernel {
    epoch: AtomicU64,
    crashes: AtomicUsize,
    crash_at: f64,
    stops: AtomicUsize,
}

impl Kernel for CrashingKernel {
    fn name(&self) -> &'static str {
        "mock"
    }
    fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }
    fn extrude(&self, profile: &Profile, spec: &ExtrudeSpec) -> gusset_kernel::KResult<OpResult> {
        if (spec.distance - self.crash_at).abs() < 1e-9 {
            self.crashes.fetch_add(1, Ordering::SeqCst);
            self.epoch.fetch_add(1, Ordering::SeqCst);
            return Err(KernelError::new(
                KernelErrorKind::Crashed,
                "the geometry kernel stopped: crashed (segmentation fault)",
            ));
        }
        if (spec.distance - 6.0).abs() < 1e-9 && self.stops.fetch_add(1, Ordering::SeqCst) == 0 {
            let flag = gusset_kernel::cancel::current().unwrap_or_else(|| panic!("no cancel scope"));
            flag.store(true, Ordering::SeqCst);
            self.epoch.fetch_add(1, Ordering::SeqCst);
            return Err(KernelError::new(KernelErrorKind::Cancelled, "stopped on request"));
        }
        MockKernel.extrude(profile, spec)
    }
    fn boolean(&self, op: BoolOp, target: &Shape, tool: &Shape) -> gusset_kernel::KResult<OpResult> {
        MockKernel.boolean(op, target, tool)
    }
    fn transform(&self, shape: &Shape, t: &gusset_kernel::Transform) -> gusset_kernel::KResult<OpResult> {
        MockKernel.transform(shape, t)
    }
    fn compound(&self, shapes: &[Shape]) -> gusset_kernel::KResult<OpResult> {
        MockKernel.compound(shapes)
    }
    fn topology(&self, shape: &Shape) -> gusset_kernel::KResult<Topology> {
        MockKernel.topology(shape)
    }
    fn mass_properties(&self, shape: &Shape) -> gusset_kernel::KResult<MassProps> {
        MockKernel.mass_properties(shape)
    }
    fn bbox(&self, shape: &Shape) -> gusset_kernel::KResult<BBox> {
        MockKernel.bbox(shape)
    }
    fn tessellate(&self, shape: &Shape, params: &TessellationParams) -> gusset_kernel::KResult<Mesh> {
        MockKernel.tessellate(shape, params)
    }
}

fn doc(e2: &str) -> Document {
    let text = format!(
        "{PLATE}extrude E2 (profile: S1, distance: {e2}, op: new)\nextrude E3 (profile: S1, distance: 3 mm, op: new)\n"
    );
    Document::from_text(&text).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_crash_fails_the_feature_rebuilds_the_rest_and_is_not_repeated() {
    let kernel = Arc::new(CrashingKernel {
        epoch: AtomicU64::new(1),
        crashes: AtomicUsize::new(0),
        crash_at: 7.0,
        stops: AtomicUsize::new(0),
    });
    let mut ev = Evaluator::new(kernel.clone());
    let r = ev.evaluate(&doc("7 mm"), None, &());
    match r.status("E2") {
        Some(FeatureStatus::Failed(e)) => {
            assert_eq!(e.code, ErrorCode::KernelCrashed, "{e:?}");
            assert!(e.message.contains("crashed while building E2") && e.message.contains("segmentation"), "{e}");
            assert!(e.hint.is_some());
        }
        other => panic!("E2: {other:?}"),
    }
    // Everything else was rebuilt against the new worker (new epoch) and is fine.
    assert!(matches!(r.status("E1"), Some(FeatureStatus::Ok)));
    assert!(matches!(r.status("E3"), Some(FeatureStatus::Ok)));
    assert!(r.body("E1").is_some() && r.body("E3").is_some());
    assert_eq!(r.epoch, 2, "the bodies belong to the new worker");
    assert_eq!(kernel.crashes.load(Ordering::SeqCst), 1);
    // Rebuilding the same document does not crash the worker again.
    let r = ev.evaluate(&doc("7 mm"), None, &());
    assert_eq!(kernel.crashes.load(Ordering::SeqCst), 1);
    assert!(matches!(r.status("E2"), Some(FeatureStatus::Failed(e)) if e.code == ErrorCode::KernelCrashed));
    // A changed feature is tried again (and works).
    let r = ev.evaluate(&doc("8 mm"), None, &());
    assert!(matches!(r.status("E2"), Some(FeatureStatus::Ok)), "{:?}", r.status("E2"));
    assert!(r.errors().is_empty());
}

#[test]
fn a_restart_outside_the_evaluator_drops_the_cache() {
    let kernel = Arc::new(CrashingKernel {
        epoch: AtomicU64::new(1),
        crashes: AtomicUsize::new(0),
        crash_at: -1.0,
        stops: AtomicUsize::new(0),
    });
    let mut ev = Evaluator::new(kernel.clone());
    let d = doc("4 mm");
    let r = ev.evaluate(&d, None, &());
    assert!(r.errors().is_empty() && r.recomputed > 0);
    assert_eq!(ev.evaluate(&d, None, &()).recomputed, 0, "cached");
    // The worker died elsewhere (e.g. during a query): cached shapes are gone.
    kernel.epoch.fetch_add(1, Ordering::SeqCst);
    let r = ev.evaluate(&d, None, &());
    assert_eq!(r.recomputed, 4, "everything rebuilt (S1, E1, E2, E3)");
    assert_eq!(r.epoch, 2);
}

#[test]
fn a_stop_inside_a_kernel_call_cancels_the_rebuild_without_memoising_it() {
    let kernel = Arc::new(CrashingKernel {
        epoch: AtomicU64::new(1),
        crashes: AtomicUsize::new(0),
        crash_at: -1.0,
        stops: AtomicUsize::new(0),
    });
    let mut ev = Evaluator::new(kernel.clone());
    let d = doc("6 mm");
    let r = ev.evaluate(&d, None, &());
    assert!(r.cancelled);
    assert!(matches!(r.status("E2"), Some(FeatureStatus::Failed(e)) if e.code == ErrorCode::Cancelled));
    assert!(matches!(r.status("E3"), Some(FeatureStatus::Failed(e)) if e.code == ErrorCode::Cancelled));
    assert_eq!(r.epoch, 1, "the evaluation holds shapes of the stopped worker");
    // The next rebuild (flag cleared by the host) runs on the new worker and does E2 again.
    ev.cancel_flag().store(false, Ordering::SeqCst);
    let r = ev.evaluate(&d, None, &());
    assert!(!r.cancelled && r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.epoch, 2);
    assert_eq!(r.recomputed, 4, "the cache died with the worker");
}
