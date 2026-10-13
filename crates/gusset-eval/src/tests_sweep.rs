//! Sweep and loft features against the mock kernel: paths and profiles are checked
//! before the kernel is asked, and a kernel without sweeps fails cleanly. The real
//! geometry is tested with OCCT in `gusset-cli/tests/modeling_sweep.rs`.

use super::*;
use gusset_kernel::mock::MockKernel;

const BASE: &str = "gusset 1
units mm

sketch Profile (plane: XY, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 3)
}

sketch Path (plane: XZ, next_id: 7) {
  p1 = point(0, 0)
  p2 = point(0, 9)
  p3 = point(5, 9)
  l4 = line(p1, p2)
  l5 = line(p2, p3)
}

plane Up (base: XY, offset: 10 mm)

sketch Top (plane: Up, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 2)
}
";

fn run(extra: &str) -> Evaluation {
    let doc = Document::from_text(&format!("{BASE}\n{extra}\n")).unwrap_or_else(|e| panic!("{e}"));
    Evaluator::new(Arc::new(MockKernel)).evaluate(&doc, None, &())
}

fn failure(r: &Evaluation, feature: &str) -> EvalError {
    match r.status(feature) {
        Some(FeatureStatus::Failed(e)) => e.clone(),
        other => panic!("{feature}: {other:?}"),
    }
}

#[test]
fn a_kernel_without_sweeps_fails_cleanly() {
    let r = run("sweep Sweep1 (profile: Profile, path: Path, op: new)");
    let e = failure(&r, "Sweep1");
    assert_eq!(e.code, ErrorCode::KernelFailure);
    assert!(e.message.contains("sweep is not supported"), "{e}");
    let r = run("loft Loft1 (profiles: [Profile, Top], op: new)");
    let e = failure(&r, "Loft1");
    assert_eq!(e.code, ErrorCode::KernelFailure);
    assert!(e.message.contains("loft is not supported"), "{e}");
}

#[test]
fn bad_paths_are_reported_before_the_kernel_runs() {
    // Naming a curve that is not there is a document problem; naming curves that do not
    // join is an evaluation error with a hint.
    let r = run("sketch Gap (plane: XZ, next_id: 7) {
  p1 = point(0, 0)
  p2 = point(0, 9)
  p3 = point(5, 12)
  p4 = point(5, 20)
  l5 = line(p1, p2)
  l6 = line(p3, p4)
}
sweep Sweep1 (profile: Profile, path: Gap, op: new)");
    let e = failure(&r, "Sweep1");
    assert_eq!(e.code, ErrorCode::InvalidReference);
    assert!(e.message.contains("not one chain") && e.message.contains("l6"), "{e}");
    assert!(e.hint.is_some());
    let r = run("sketch Tee (plane: XZ, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(0, 9)
  p3 = point(5, 9)
  p4 = point(-5, 9)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p2, p4)
}
sweep Sweep1 (profile: Profile, path: Tee, op: new)");
    let e = failure(&r, "Sweep1");
    assert!(e.message.contains("meet at (0.000, 9.000)"), "{e}");
    // Only the first leg: the chain is fine, the mock then declines.
    let r = run("sweep Sweep1 (profile: Profile, path: Path.curves(l4), op: new)");
    assert!(failure(&r, "Sweep1").message.contains("not supported"));
    let r = run(
        "sketch Empty (plane: XZ, next_id: 2) {\n  p1 = point(0, 0)\n}\nsweep Sweep1 (profile: Profile, path: Empty, op: new)",
    );
    assert!(failure(&r, "Sweep1").message.contains("no curves"));
}

#[test]
fn loft_profiles_need_one_region_each() {
    // Two separate outlines in one sketch are two regions: pick one.
    let r = run("sketch Pair (plane: Up, next_id: 7) {
  p1 = point(0, 0)
  c2 = circle(p1, 2)
  p3 = point(10, 0)
  c4 = circle(p3, 2)
}
loft Loft1 (profiles: [Profile, Pair], op: new)");
    let e = failure(&r, "Loft1");
    assert_eq!(e.code, ErrorCode::InvalidReference);
    assert!(e.message.contains("exactly one") && e.hint.as_deref().is_some_and(|h| h.contains("Pair.regions(")), "{e}");
    // A profile with a hole cannot be lofted.
    let r = run("sketch Ring (plane: Up, next_id: 7) {
  p1 = point(0, 0)
  c2 = circle(p1, 4)
  c3 = circle(p1, 2)
}
loft Loft1 (profiles: [Profile, Ring.regions(c2)], op: new)");
    assert!(failure(&r, "Loft1").message.contains("holes"));
    // An open outline is an open-profile error naming the sketch.
    let r = run("sketch Open (plane: Up, next_id: 4) {
  p1 = point(0, 0)
  p2 = point(5, 0)
  l3 = line(p1, p2)
}
loft Loft1 (profiles: [Profile, Open], op: new)");
    assert_eq!(failure(&r, "Loft1").code, ErrorCode::OpenProfile);
}

#[test]
fn features_after_a_failed_sweep_are_blocked_and_the_rebuild_goes_on() {
    let r = run("sweep Sweep1 (profile: Profile, path: Path, op: new)
mirror Mirror1 (body: Sweep1, plane: YZ)");
    assert!(
        matches!(r.status("Mirror1"), Some(FeatureStatus::Blocked { by }) if by == "Sweep1"),
        "{:?}",
        r.status("Mirror1")
    );
    assert_eq!(r.status("Top"), Some(&FeatureStatus::Ok));
}

#[test]
fn absurd_twists_are_refused_before_the_kernel() {
    // Review H2: `twist: 1e15 deg` overflowed the kernel's sampling and crashed.
    for twist in ["1e15 deg", "36001 deg"] {
        let r =
            run(&format!("sweep Sweep1 (profile: Profile, path: Path, orientation: frenet, twist: {twist}, op: new)"));
        let e = failure(&r, "Sweep1");
        assert_eq!(e.code, ErrorCode::Expression, "{twist}: {e:?}");
        assert!(e.message.contains("too large"), "{twist}: {e:?}");
    }
}
