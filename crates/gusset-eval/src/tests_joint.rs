//! Joints (ADR 0026) on the mock kernel: planar-face frames, flip, offset, motion, limits,
//! carried bodies.

use super::*;
use gusset_kernel::Transform;
use gusset_kernel::mock::MockKernel;

fn rect(name: &str, w: f64, h: f64) -> String {
    format!(
        "sketch {name} (plane: XY, next_id: 9) {{
  p1 = point(0, 0)
  p2 = point({w}, 0)
  p3 = point({w}, {h})
  p4 = point(0, {h})
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}}
"
    )
}

fn eval(joint: &str) -> (Evaluator, Evaluation) {
    let text = format!(
        "gusset 1\nunits mm\nparam open = 90 deg\n{}extrude Base (profile: S1, distance: 5 mm, op: new)\n{}\
         extrude Lid (profile: S2, distance: 2 mm, op: new)\nextrude Tag (profile: S2, distance: 1 mm, op: new)\n{joint}\n",
        rect("S1", 10.0, 10.0),
        rect("S2", 10.0, 4.0),
    );
    let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn bbox(ev: &Evaluator, r: &Evaluation, body: &str) -> ([f64; 3], [f64; 3]) {
    let b = ev.kernel().bbox(&r.body(body).unwrap_or_else(|| panic!("no body {body}")).shape).unwrap();
    (b.min, b.max)
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

#[test]
fn a_rigid_joint_puts_the_lid_on_the_base() {
    let (ev, r) = eval("joint J1 (type: rigid, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // The lid's bottom face center (5, 2, 0) lands on the base's top center (5, 5, 5).
    let (lo, hi) = bbox(&ev, &r, "Lid");
    assert!(close(lo, [0.0, 3.0, 5.0]) && close(hi, [10.0, 7.0, 7.0]), "{lo:?} {hi:?}");
    // Names survive the placement.
    assert!(r.body("Lid").unwrap().names.faces.iter().any(|f| f.to_string() == "Lid/cap(end)"));
}

#[test]
fn a_revolute_joint_turns_and_carries_bodies() {
    let (ev, r) = eval(
        "joint J1 (type: revolute, body: Lid, with: [Tag], at: Lid/cap(start), to: Base/cap(end), flip: true, \
         angle: open, offset: 1 mm)",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // A quarter turn about the base's vertical center line, one millimetre above it.
    let (lo, hi) = bbox(&ev, &r, "Lid");
    assert!(close(lo, [3.0, 0.0, 6.0]) && close(hi, [7.0, 10.0, 8.0]), "{lo:?} {hi:?}");
    // The tag moved with the lid (same transform).
    let (lo, hi) = bbox(&ev, &r, "Tag");
    assert!(close(lo, [3.0, 0.0, 6.0]) && close(hi, [7.0, 10.0, 7.0]), "{lo:?} {hi:?}");
}

#[test]
fn a_slider_moves_along_the_frame_normal_and_respects_limits() {
    let (ev, r) = eval(
        "joint J1 (type: slider, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true, distance: -3 mm, \
         distance_limits: [-4 mm, 0 mm])",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // The flipped fixed frame's Z points down: -3 mm moves the lid up.
    let (lo, _) = bbox(&ev, &r, "Lid");
    assert!(close(lo, [0.0, 3.0, 8.0]), "{lo:?}");
    let (_, r) = eval(
        "joint J1 (type: slider, body: Lid, at: Lid/cap(start), to: Base/cap(end), distance: 9 mm, \
         distance_limits: [-4 mm, 0 mm])",
    );
    let e = r.errors();
    assert!(e.iter().any(|e| e.feature == "J1" && e.message.contains("outside the joint's limits")), "{e:?}");
}

#[test]
fn a_relation_is_an_expression_of_another_joint_value() {
    // A rack: 2 mm of travel per quarter turn of `open` (90 deg).
    let (ev, r) = eval(
        "joint J1 (type: slider, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true, \
         distance: open / 360 deg * 8 mm)",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let (lo, _) = bbox(&ev, &r, "Lid");
    assert!(close(lo, [0.0, 3.0, 3.0]), "{lo:?}");
}

#[test]
fn a_joint_to_the_origin_and_a_bad_frame() {
    let (ev, r) = eval("joint J1 (type: rigid, body: Lid, at: Lid/cap(end), to: origin)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // The lid's top face center goes to the world origin.
    let (lo, hi) = bbox(&ev, &r, "Lid");
    assert!(close(lo, [-5.0, -2.0, -2.0]) && close(hi, [5.0, 2.0, 0.0]), "{lo:?} {hi:?}");
    // An unknown body is caught when the document is read.
    let bad = format!(
        "gusset 1\n{}extrude Base (profile: S1, distance: 5 mm, op: new)\njoint J1 (type: rigid, body: Nope, at: origin, to: origin)\n",
        rect("S1", 1.0, 1.0)
    );
    assert!(Document::from_text(&bad).unwrap_err().to_string().contains("Nope"));
}

/// A four-bar linkage in the XY plane: links are boxes along +X, pivots their vertical
/// corner edges at (0, 0) and (length, 0). Ground 40, crank 10, coupler 35, rocker 25.
fn four_bar(crank: &str, closing: &str) -> (Evaluator, Evaluation) {
    let pivot = |b: &str, s: &str, end: bool| {
        if end {
            format!("edge({b}/side({s}/l5), {b}/side({s}/l6))")
        } else {
            format!("edge({b}/side({s}/l8), {b}/side({s}/l5))")
        }
    };
    let text = format!(
        "gusset 1\nunits mm\nparam crank = {crank}\n{}{}{}{}\
         extrude Ground (profile: G, distance: 1 mm, op: new)\nextrude Crank (profile: C, distance: 1 mm, op: new)\n\
         extrude Coupler (profile: K, distance: 1 mm, op: new)\nextrude Rocker (profile: R, distance: 1 mm, op: new)\n\
         joint J1 (type: revolute, body: Crank, at: {}, to: {}, angle: crank)\n\
         joint J2 (type: revolute, body: Coupler, at: {}, to: {}, free: [angle])\n\
         joint J3 (type: revolute, body: Rocker, at: {}, to: {}, angle: 90 deg, free: [angle])\n\
         joint J4 (type: revolute, body: Coupler, at: {}, to: {}{closing})\n",
        rect("G", 40.0, 2.0),
        rect("C", 10.0, 2.0),
        rect("K", 35.0, 2.0),
        rect("R", 25.0, 2.0),
        pivot("Crank", "C", false),
        pivot("Ground", "G", false),
        pivot("Coupler", "K", false),
        pivot("Crank", "C", true),
        pivot("Rocker", "R", false),
        pivot("Ground", "G", true),
        pivot("Coupler", "K", true),
        pivot("Rocker", "R", true),
    );
    let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

/// The rocker's tip for rocker angle `a` (either rotation sense), and the crank's tip.
fn closes(crank_deg: f64, r: &Evaluation) -> bool {
    let a = r.joint_values.get("J3").map_or(f64::NAN, |v| v.angle);
    let c = crank_deg.to_radians();
    let tip = (10.0 * libm::cos(c), 10.0 * libm::sin(c));
    [a, -a].iter().any(|a| {
        let d = (40.0 + 25.0 * libm::cos(*a), 25.0 * libm::sin(*a));
        (libm::hypot(d.0 - tip.0, d.1 - tip.1) - 35.0).abs() < 1e-6
    })
}

#[test]
fn a_closing_joint_solves_the_free_values_of_a_four_bar() {
    let (_, r) = four_bar("60 deg", ", free: [angle]");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(closes(60.0, &r), "{:?}", r.joint_values);
    // A driven value stays as written.
    assert!((r.joint_values["J1"].angle - 60f64.to_radians()).abs() < 1e-12);
    // Moving the crank re-solves the loop.
    let (_, r) = four_bar("30 deg", ", free: [angle]");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(closes(30.0, &r), "{:?}", r.joint_values);
}

#[test]
fn a_loop_that_cannot_close_says_why() {
    let first_error =
        |r: &Evaluation| r.errors().first().map(|e| format!("{} | {:?}", e.message, e.hint)).unwrap_or_default();
    // With the crank driven, all three other pivots must turn: fixing the closing joint's
    // angle over-constrains the loop.
    let (_, r) = four_bar("60 deg", "");
    let e = first_error(&r);
    assert!(e.contains("cannot close") && r.errors().first().is_some_and(|e| e.feature == "J4"), "{e}");
    // Nothing free: a second joint on the lid cannot also put it on the base's bottom.
    let rigid = "joint J1 (type: rigid, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true)
";
    let (_, r) = eval(&format!("{rigid}joint J2 (type: rigid, body: Lid, at: Lid/cap(start), to: Base/cap(start))"));
    let e = first_error(&r);
    assert!(e.contains("cannot close") && e.contains("already placed by J1") && e.contains("free"), "{e}");
    // Free, but a turn cannot make up for a 5 mm gap.
    let (_, r) = eval(&format!(
        "{rigid}joint J2 (type: revolute, body: Lid, at: Lid/cap(start), to: Base/cap(start), free: [angle])"
    ));
    let e = first_error(&r);
    assert!(e.contains("cannot close") && e.contains("mm") && e.contains("cannot be assembled"), "{e}");
}

// ---- review fixes (v0.3 assemblies review: M1, M2, M3, L2, N2) ----

fn failed(r: &Evaluation) -> Vec<String> {
    r.errors().iter().map(|e| e.feature.clone()).collect()
}

fn message(r: &Evaluation, feature: &str) -> String {
    r.errors()
        .iter()
        .find(|e| e.feature == feature)
        .map(|e| format!("{} | {:?}", e.message, e.hint))
        .unwrap_or_default()
}

const J1_OK: &str = "joint J1 (type: rigid, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true)\n";

#[test]
fn a_failed_joint_leaves_no_record_and_the_next_joint_places_the_body() {
    // M2 (a): J1 breaks its limits, so it places nothing; J2 is then the first joint of the
    // lid, not a loop-closing one blamed on J1.
    let (ev, r) = eval(
        "joint J1 (type: slider, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true, distance: 9 mm, \
         distance_limits: [0 mm, 4 mm])\n\
         joint J2 (type: rigid, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true)",
    );
    assert_eq!(failed(&r), ["J1"], "{:?}", r.errors());
    assert!(!r.joint_values.contains_key("J1") && r.joint_values.contains_key("J2"));
    let (lo, _) = bbox(&ev, &r, "Lid");
    assert!(close(lo, [0.0, 3.0, 5.0]), "{lo:?}");
}

#[test]
fn a_closing_joint_that_fails_does_not_poison_an_independent_loop() {
    // M2 (b): J2 cannot close the lid's loop; the tag's own loop (J3, J4) is independent.
    let tag = "joint J3 (type: rigid, body: Tag, at: Tag/cap(start), to: Base/cap(end), flip: true)\n\
               joint J4 (type: revolute, body: Tag, at: Tag/cap(start), to: Base/cap(end), flip: true, free: [angle])";
    let (_, r) =
        eval(&format!("{J1_OK}joint J2 (type: rigid, body: Lid, at: Lid/cap(start), to: Base/cap(start))\n{tag}"));
    assert_eq!(failed(&r), ["J2"], "{:?}", r.errors());
}

#[test]
fn a_frame_on_another_body_is_refused() {
    // M3: the 'at' frame must lie on the moving body or a carried one, 'to' on none of them.
    let at = "joint J1 (type: rigid, body: Lid, at: Base/cap(start), to: origin)";
    let (ev, r) = eval(at);
    assert!(message(&r, "J1").contains("'at' frame") && message(&r, "J1").contains("Base"), "{:?}", r.errors());
    let (lo, _) = bbox(&ev, &r, "Lid");
    assert!(close(lo, [0.0, 0.0, 0.0]), "nothing moved: {lo:?}");
    let to = "joint J1 (type: rigid, body: Lid, at: Lid/cap(start), to: Lid/cap(end))";
    let (_, r) = eval(to);
    assert!(message(&r, "J1").contains("'to' frame") && message(&r, "J1").contains("Lid"), "{:?}", r.errors());
    let carried = "joint J1 (type: rigid, body: Lid, with: [Tag], at: Lid/cap(start), to: Tag/cap(end))";
    let (_, r) = eval(carried);
    assert!(message(&r, "J1").contains("'to' frame") && message(&r, "J1").contains("Tag"), "{:?}", r.errors());
    // An 'at' frame on a carried body is fine.
    let (_, r) = eval("joint J1 (type: rigid, body: Lid, with: [Tag], at: Tag/cap(start), to: Base/cap(end))");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
}

#[test]
fn carried_bodies_must_be_new_and_distinct() {
    // M3: Tag was placed by J1; J2 may not carry it again.
    let (_, r) = eval(
        "joint J1 (type: rigid, body: Tag, at: Tag/cap(start), to: Base/cap(end), flip: true)\n\
         joint J2 (type: rigid, body: Lid, with: [Tag], at: Lid/cap(start), to: Base/cap(end), flip: true)",
    );
    assert_eq!(failed(&r), ["J2"], "{:?}", r.errors());
    assert!(message(&r, "J2").contains("'Tag'") && message(&r, "J2").contains("already placed"), "{:?}", r.errors());
    // The same body twice is caught when the document is read.
    let dup = format!(
        "gusset 1\n{}extrude A (profile: S1, distance: 1 mm, op: new)\nextrude B (profile: S1, distance: 1 mm, op: new)\n\
         joint J1 (type: rigid, body: A, with: [B, B], at: origin, to: origin)\n",
        rect("S1", 1.0, 1.0)
    );
    assert!(Document::from_text(&dup).unwrap_err().to_string().contains("twice"));
}

#[test]
fn a_closing_joint_refuses_with() {
    // M3: the carried body would stay behind silently.
    let (_, r) = eval(&format!(
        "{J1_OK}joint J2 (type: revolute, body: Lid, with: [Tag], at: Lid/cap(start), to: Base/cap(end), flip: true, \
         free: [angle])"
    ));
    assert_eq!(failed(&r), ["J2"], "{:?}", r.errors());
    assert!(message(&r, "J2").contains("cannot carry"), "{:?}", r.errors());
}

#[test]
fn limit_errors_use_the_documents_unit_and_name_the_joint() {
    // N2.
    let (_, r) = eval(
        "joint J1 (type: slider, body: Lid, at: Lid/cap(start), to: Base/cap(end), distance: 9 mm, \
         distance_limits: [-4 mm, 0 mm])",
    );
    let m = message(&r, "J1");
    assert!(m.contains("joint J1") && m.contains("9 mm") && m.contains("-4 mm") && !m.contains("base units"), "{m}");
    let (_, r) = eval(
        "joint J1 (type: revolute, body: Lid, at: Lid/cap(start), to: Base/cap(end), angle: open, \
         angle_limits: [0 deg, 45 deg])",
    );
    let m = message(&r, "J1");
    assert!(m.contains("90 deg") && m.contains("45 deg") && !m.contains("radians"), "{m}");
}

/// Ball joints B1..Bn chained (each to the previous body), then `loop` joints.
fn chain(n: usize, tail: &str) -> (Evaluator, Evaluation) {
    let mut text = format!("gusset 1\nunits mm\n{}", rect("S", 10.0, 4.0));
    for i in 0..=n {
        text.push_str(&format!("extrude B{i} (profile: S, distance: 1 mm, op: new)\n"));
    }
    for i in 1..=n {
        text.push_str(&format!(
            "joint J{i} (type: ball, body: B{i}, at: B{i}/cap(start), to: B{}/cap(end), free: [angles])\n",
            i - 1
        ));
    }
    text.push_str(tail);
    let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

#[test]
fn too_many_unknowns_in_one_loop_is_an_error() {
    // M1: 21 chained ball joints are 63 unknowns for the loop that closes B21 on B1.
    let (_, r) = chain(21, "joint Jc (type: rigid, body: B21, at: B21/cap(end), to: B1/cap(end))\n");
    let m = message(&r, "Jc");
    assert!(m.contains("63 free values") && m.contains("limit is 60") && m.contains("fewer"), "{m}");
    assert!(r.errors().iter().any(|e| e.code == ErrorCode::TooComplex));
}

#[test]
fn free_values_outside_the_loop_are_not_unknowns() {
    // M1: 30 independent ball joints (90 free values) do not count for a two-joint loop.
    let mut text = String::from("gusset 1\nunits mm\n");
    text.push_str(&rect("S", 10.0, 4.0));
    for i in 0..=31 {
        text.push_str(&format!("extrude B{i} (profile: S, distance: 1 mm, op: new)\n"));
    }
    for i in 1..=30 {
        text.push_str(&format!(
            "joint J{i} (type: ball, body: B{i}, at: B{i}/cap(start), to: B0/cap(end), free: [angles])\n"
        ));
    }
    text.push_str(
        "joint Ja (type: slider, body: B31, at: B31/cap(start), to: B0/cap(end), flip: true, free: [distance])\n\
         joint Jb (type: slider, body: B31, at: B31/cap(start), to: B0/cap(end), flip: true, distance: 3 mm, \
         free: [distance])\n",
    );
    let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let (a, b) = (r.joint_values["Ja"].distance, r.joint_values["Jb"].distance);
    assert!((a - b).abs() < 1e-6, "{a} {b}");
}

/// The mock kernel counting transforms (the cost of placing bodies).
#[derive(Debug, Default)]
struct CountingKernel {
    transforms: std::sync::atomic::AtomicUsize,
}

impl gusset_kernel::Kernel for CountingKernel {
    fn name(&self) -> &'static str {
        "mock"
    }
    fn extrude(
        &self,
        p: &gusset_kernel::Profile,
        s: &gusset_kernel::ExtrudeSpec,
    ) -> gusset_kernel::KResult<gusset_kernel::OpResult> {
        MockKernel.extrude(p, s)
    }
    fn boolean(
        &self,
        op: gusset_kernel::BoolOp,
        a: &gusset_kernel::Shape,
        b: &gusset_kernel::Shape,
    ) -> gusset_kernel::KResult<gusset_kernel::OpResult> {
        MockKernel.boolean(op, a, b)
    }
    fn transform(&self, s: &gusset_kernel::Shape, t: &Transform) -> gusset_kernel::KResult<gusset_kernel::OpResult> {
        self.transforms.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        MockKernel.transform(s, t)
    }
    fn compound(&self, s: &[gusset_kernel::Shape]) -> gusset_kernel::KResult<gusset_kernel::OpResult> {
        MockKernel.compound(s)
    }
    fn topology(&self, s: &gusset_kernel::Shape) -> gusset_kernel::KResult<gusset_kernel::Topology> {
        MockKernel.topology(s)
    }
    fn mass_properties(&self, s: &gusset_kernel::Shape) -> gusset_kernel::KResult<gusset_kernel::MassProps> {
        MockKernel.mass_properties(s)
    }
    fn bbox(&self, s: &gusset_kernel::Shape) -> gusset_kernel::KResult<gusset_kernel::BBox> {
        MockKernel.bbox(s)
    }
    fn tessellate(
        &self,
        s: &gusset_kernel::Shape,
        p: &gusset_kernel::TessellationParams,
    ) -> gusset_kernel::KResult<gusset_kernel::Mesh> {
        MockKernel.tessellate(s, p)
    }
}

#[test]
fn a_closing_joint_only_moves_the_bodies_whose_place_changed() {
    // L2: the tag (turned a quarter) and the lid are placed by J1 and J2; the closing joint
    // J3 agrees with J1, so it must not transform anything again (rounding noise).
    let transforms = |closing: &str| {
        let text = format!(
            "gusset 1\nunits mm\nparam open = 90 deg\n{}{}extrude Base (profile: S1, distance: 5 mm, op: new)\n\
             extrude Lid (profile: S2, distance: 2 mm, op: new)\nextrude Tag (profile: S2, distance: 1 mm, op: new)\n\
             joint J1 (type: revolute, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true, angle: open, \
             offset: 1 mm)\n\
             joint J2 (type: revolute, body: Tag, at: Tag/cap(start), to: Base/cap(end), flip: true, angle: open / 3, \
             offset: 1 mm)\n{closing}",
            rect("S1", 10.0, 10.0),
            rect("S2", 10.0, 4.0)
        );
        let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        let kernel = Arc::new(CountingKernel::default());
        let mut ev = Evaluator::new(kernel.clone());
        let r = ev.evaluate(&doc, None, &());
        assert!(r.errors().is_empty(), "{:?}", r.errors());
        kernel.transforms.load(std::sync::atomic::Ordering::SeqCst)
    };
    let without = transforms("");
    let with = transforms(
        "joint J3 (type: revolute, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true, angle: open, \
         offset: 1 mm, free: [angle])\n",
    );
    assert_eq!(with, without, "the closing joint transformed bodies that did not move");
}
