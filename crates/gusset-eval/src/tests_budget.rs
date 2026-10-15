//! Evaluation work limits and cancellation (security review V1) against the mock kernel.

use super::*;
use gusset_kernel::mock::MockKernel;
use gusset_kernel::{BBox, MassProps, Mesh, Profile, TessellationParams};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const BOX: &str = "gusset 1
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

extrude E1 (profile: S1, distance: 10 mm, op: new)
";

/// `lines` chained count-2 patterns: each doubles the body (copies 20 * 2^i mm apart, so
/// they never touch).
fn chained(lines: u32) -> Document {
    let mut t = String::from(BOX);
    for i in 0..lines {
        t.push_str(&format!("pattern_linear P{} (direction: X, count: 2, spacing: {} mm)\n", i + 1, 20u64 << i));
    }
    Document::from_text(&t).unwrap_or_else(|e| panic!("{e}"))
}

fn failure(r: &Evaluation, feature: &str) -> EvalError {
    match r.status(feature) {
        Some(FeatureStatus::Failed(e)) => e.clone(),
        other => panic!("{feature}: {other:?}"),
    }
}

#[test]
fn chained_doubling_patterns_hit_the_budget_quickly() {
    let doc = chained(40);
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let t = Instant::now();
    let r = ev.evaluate(&doc, None, &());
    let took = t.elapsed();
    // 6 faces doubled 10 times is 6,144 faces; the 11th line would make 12,288.
    assert!(matches!(r.status("P10"), Some(FeatureStatus::Ok)), "{:?}", r.status("P10"));
    let e = failure(&r, "P11");
    assert_eq!(e.code, ErrorCode::TooComplex);
    assert!(e.message.contains("12288 faces") && e.message.contains("10000"), "{e}");
    assert!(e.hint.is_some());
    // Later lines target the same body and fail the same way at once.
    assert_eq!(failure(&r, "P40").code, ErrorCode::TooComplex);
    assert_eq!(r.body("E1").map(|b| b.topology.faces.len()), Some(6 << 10));
    // Most of the time is naming the 6,144 faces, whose names nest ten patterns deep; an
    // unoptimised build is about three times slower.
    eprintln!("chained patterns: {took:?}");
    let budget = if cfg!(debug_assertions) { Duration::from_secs(3) } else { Duration::from_millis(500) };
    assert!(took < budget, "took {took:?}");
}

#[test]
fn limits_are_configurable_and_count_the_whole_evaluation() {
    // Two 50-instance patterns of a 6-face box: 300 + 1,800 faces in.
    let text = format!(
        "{BOX}\npattern_linear P1 (direction: X, count: 50, spacing: 20 mm)\n\
         pattern_linear P2 (direction: Y, count: 6, spacing: 20 mm)\n"
    );
    let doc = Document::from_text(&text).unwrap();
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // Total boolean work: 300 + 1,800 = 2,100 faces.
    ev.set_limits(EvalLimits { max_boolean_work: 2_000, ..EvalLimits::default() });
    let r = ev.evaluate(&doc, None, &());
    assert!(matches!(r.status("P1"), Some(FeatureStatus::Ok)));
    let e = failure(&r, "P2");
    assert_eq!(e.code, ErrorCode::TooComplex);
    assert!(e.message.contains("2100 faces"), "{e}");
    // Tools: 49 + 5 = 54.
    ev.set_limits(EvalLimits { max_tools: 50, ..EvalLimits::default() });
    let r = ev.evaluate(&doc, None, &());
    assert_eq!(failure(&r, "P2").code, ErrorCode::TooComplex);
    // The decision does not depend on the cache: with P1 cached, P2 still fails.
    ev.set_limits(EvalLimits { max_boolean_work: 2_000, ..EvalLimits::default() });
    let first =
        Document::from_text(&format!("{BOX}\npattern_linear P1 (direction: X, count: 50, spacing: 20 mm)\n")).unwrap();
    assert!(ev.evaluate(&first, None, &()).errors().is_empty());
    let r = ev.evaluate(&doc, None, &());
    assert_eq!(r.recomputed, 1);
    assert_eq!(failure(&r, "P2").code, ErrorCode::TooComplex);
}

/// The mock kernel, setting the evaluator's cancel flag on the `at`-th extrusion.
#[derive(Debug)]
struct CancelOnExtrude {
    flag: OnceLock<Arc<std::sync::atomic::AtomicBool>>,
    calls: std::sync::atomic::AtomicUsize,
    at: usize,
}

impl Kernel for CancelOnExtrude {
    fn name(&self) -> &'static str {
        "mock"
    }
    fn extrude(&self, profile: &Profile, spec: &ExtrudeSpec) -> gusset_kernel::KResult<OpResult> {
        let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        if n == self.at
            && let Some(f) = self.flag.get()
        {
            f.store(true, std::sync::atomic::Ordering::SeqCst);
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

#[test]
fn a_cancelled_rebuild_stops_and_caches_nothing_after_the_cancel() {
    let text = format!(
        "{BOX}\nextrude E2 (profile: S1, distance: 5 mm, op: new)\n\
         pattern_linear P1 (body: E2, direction: X, count: 3, spacing: 20 mm)\n\
         extrude E3 (profile: S1, distance: 7 mm, op: new)\n"
    );
    let doc = Document::from_text(&text).unwrap();
    let kernel = Arc::new(CancelOnExtrude { flag: OnceLock::new(), calls: Default::default(), at: 2 });
    let mut ev = Evaluator::new(kernel.clone());
    let _ = kernel.flag.set(ev.cancel_flag());
    let r = ev.evaluate(&doc, None, &());
    assert!(r.cancelled);
    assert!(matches!(r.status("E1"), Some(FeatureStatus::Ok)));
    for f in ["E2", "P1", "E3"] {
        assert_eq!(failure(&r, f).code, ErrorCode::Cancelled, "{f}");
    }
    assert!(r.body("E2").is_none(), "a cancelled feature's state is not kept");
    // While the flag is set, evaluations stop at once.
    let r = ev.evaluate(&doc, None, &());
    assert!(r.cancelled && r.recomputed == 0);
    assert_eq!(failure(&r, "E2").code, ErrorCode::Cancelled);
    // Once it is cleared, the next evaluation reuses what finished and recomputes the rest.
    ev.cancel_flag().store(false, std::sync::atomic::Ordering::SeqCst);
    let r = ev.evaluate(&doc, None, &());
    assert!(!r.cancelled);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.recomputed, 3, "the sketch and E1 came from the cache");
}

#[test]
fn component_builds_share_the_evaluation_budget() {
    // Security review v0.3 W3: each component build had a fresh budget, so different `set:`
    // values multiplied the work. The part: a 50-instance pattern (300 faces of boolean work).
    struct Part(String);
    impl BlobSource for Part {
        fn blob(&self, _: &str) -> Option<Arc<[u8]>> {
            Some(Arc::from(self.0.as_bytes()))
        }
    }
    let part = BOX.replacen("units mm\n", "units mm\n\nparam w = 20 mm\n", 1)
        + "\npattern_linear P1 (direction: X, count: 50, spacing: w)\n";
    let hash = "ab".repeat(32);
    let mut text = String::from("gusset 1\nunits mm\n");
    for k in 0..10 {
        text.push_str(&format!(
            "\ncomponent C{k} (source: \"p.gus\", version: \"n1\", document: \"{hash}\", set: [w = {} mm])\n",
            20 + k
        ));
    }
    let doc = Document::from_text(&text).unwrap();
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    ev.set_limits(EvalLimits { max_boolean_work: 2_000, ..EvalLimits::default() });
    let r = ev.evaluate(&doc, None, &Part(part));
    // Six builds fit in 2,000 faces of work; the seventh does not.
    assert!(matches!(r.status("C5"), Some(FeatureStatus::Ok)), "{:?}", r.errors());
    let e = failure(&r, "C6");
    assert!(e.message.contains("faces"), "{e}");
}
