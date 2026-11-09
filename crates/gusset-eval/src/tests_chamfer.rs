//! Unequal chamfers: what evaluation refuses before the kernel is asked.

use super::*;
use gusset_kernel::mock::MockKernel;

const BASE: &str = "gusset 1
units mm

param d2 = 3 mm
param a = 30 deg

sketch Sketch1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new)
";

const EDGE: &str = "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l6))";

fn errors(args: &str) -> Vec<EvalError> {
    let doc = Document::from_text(&format!("{BASE}\nchamfer C1 (edges: [{EDGE}], {args})\n"))
        .unwrap_or_else(|e| panic!("{e}"));
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    ev.evaluate(&doc, None, &()).errors().into_iter().cloned().collect()
}

fn first(args: &str) -> EvalError {
    errors(args).into_iter().next().unwrap_or_else(|| panic!("no error for {args}"))
}

#[test]
fn second_distance_and_angle_are_checked() {
    for bad in ["distance2: 0 mm", "distance2: -1 mm", "distance2: d2 - 3 mm"] {
        let e = first(&format!("distance: 2 mm, {bad}"));
        assert_eq!(e.code, ErrorCode::Expression, "{bad}: {e:?}");
        assert!(e.message.contains("second distance must be positive"), "{}", e.message);
    }
    for bad in ["angle: 0 deg", "angle: 90 deg", "angle: 120 deg", "angle: -10 deg", "angle: a - 30 deg"] {
        let e = first(&format!("distance: 2 mm, {bad}"));
        assert_eq!(e.code, ErrorCode::Expression, "{bad}: {e:?}");
        assert!(e.message.contains("chamfer angle"), "{}", e.message);
    }
    // The first distance keeps its own check.
    let e = first("distance: 0 mm, distance2: 1 mm");
    assert!(e.message.contains("distance must be positive"), "{}", e.message);
    // A length where an angle belongs (and the reverse) is a dimension error.
    assert_eq!(first("distance: 2 mm, angle: 5 mm").code, ErrorCode::Expression);
    assert_eq!(first("distance: 2 mm, distance2: 30 deg").code, ErrorCode::Expression);
}

#[test]
fn valid_forms_pass_validation_and_reach_the_kernel() {
    // The mock kernel cannot chamfer: a kernel failure, not a validation error, shows the
    // spec was accepted and handed over.
    for args in ["distance: 2 mm, distance2: d2", "distance: 2 mm, angle: a", "distance: 2 mm, angle: 45 deg"] {
        let e = first(args);
        assert_ne!(e.code, ErrorCode::Expression, "{args}: {e:?}");
    }
}

#[test]
fn reference_faces_must_exist_and_border_the_edge() {
    let e = first("distance: 2 mm, distance2: 1 mm, faces: [Extrude1/side(Sketch1/l99)]");
    assert_eq!(e.code, ErrorCode::MissingReference, "{e:?}");
    assert!(e.reference.is_some());
    // l8 is a real face, but not one of the two faces of the l6/cap(end) edge.
    let e = first("distance: 2 mm, distance2: 1 mm, faces: [Extrude1/side(Sketch1/l8)]");
    assert_eq!(e.code, ErrorCode::InvalidReference, "{e:?}");
    assert!(e.message.contains("does not border"), "{}", e.message);
    // A border face is accepted by validation.
    let e = first("distance: 2 mm, distance2: 1 mm, faces: [Extrude1/side(Sketch1/l6)]");
    assert_ne!(e.code, ErrorCode::InvalidReference, "{e:?}");
}

#[test]
fn full_round_reaches_the_kernel_and_checks_its_faces() {
    let run = |args: &str| {
        let doc = Document::from_text(&format!("{BASE}\nfillet F1 ({args})\n")).unwrap_or_else(|e| panic!("{e}"));
        let mut ev = Evaluator::new(Arc::new(MockKernel));
        ev.evaluate(&doc, None, &()).errors().into_iter().next().cloned().unwrap_or_else(|| panic!("no error"))
    };
    // A missing face is a reference error.
    let e = run("full_round: [Extrude1/side(Sketch1/l99)]");
    assert_eq!(e.code, ErrorCode::MissingReference, "{e:?}");
    assert!(e.reference.is_some());
    // An existing face reaches the kernel; the mock cannot round, so that is what fails.
    let e = run("full_round: [Extrude1/side(Sketch1/l6)]");
    assert_ne!(e.code, ErrorCode::MissingReference, "{e:?}");
    assert!(e.message.contains("full round"), "{}", e.message);
}
