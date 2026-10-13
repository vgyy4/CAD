//! Rebuild time of features that cut or unite many tools (review H3, security review V1):
//! a plate with an n x n grid of through holes from one sketch, a linear body pattern of
//! disjoint copies, and `combine` with many tool bodies. With the real OCCT kernel.
//!
//! `perf_booleans_report` prints the timings (run it in release mode, see
//! docs/PERFORMANCE.md); `holes_400_budget` is the enforced budget.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn kernel() -> Arc<dyn gusset_kernel::Kernel> {
    Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")))
}

/// A plate of (n * 10) x (n * 10) x 5 mm with an n x n grid of 3 mm through holes from one
/// sketch.
fn holes_doc(n: usize) -> String {
    let size = n * 10;
    let mut t = format!(
        "gusset 1\nunits mm\n\nsketch S1 (plane: XY, next_id: 9) {{\n  p1 = point(0, 0)\n  p2 = point({size}, 0)\n  \
         p3 = point({size}, {size})\n  p4 = point(0, {size})\n  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  \
         l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}}\n\nextrude E1 (profile: S1, distance: 5 mm, op: new)\n\n\
         sketch Pts (plane: E1/cap(end), next_id: {}) {{\n",
        n * n + 1
    );
    for i in 0..n * n {
        let (x, y) = ((i % n) * 10 + 5, (i / n) * 10 + 5);
        let _ = writeln!(t, "  p{} = point({x}, {y})", i + 1);
    }
    t.push_str("}\n\nhole H1 (face: E1/cap(end), points: Pts, diameter: 3 mm, through: true)\n");
    t
}

/// A 10 mm cube patterned `count` times 20 mm apart (the copies do not touch).
fn pattern_doc(count: usize) -> String {
    format!(
        "gusset 1\nunits mm\n\nsketch S1 (plane: XY, next_id: 9) {{\n  p1 = point(0, 0)\n  p2 = point(10, 0)\n  \
         p3 = point(10, 10)\n  p4 = point(0, 10)\n  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  \
         l8 = line(p4, p1)\n}}\n\nextrude E1 (profile: S1, distance: 10 mm, op: new)\n\n\
         pattern_linear P1 (direction: X, count: {count}, spacing: 20 mm)\n"
    )
}

/// A 10 mm x 10 mm x (n * 10) mm bar and `n` 4 mm pins through it, cut with one combine.
fn combine_doc(n: usize) -> String {
    let len = n * 10;
    let mut t = format!(
        "gusset 1\nunits mm\n\nsketch S1 (plane: XY, next_id: 9) {{\n  p1 = point(0, 0)\n  p2 = point({len}, 0)\n  \
         p3 = point({len}, 10)\n  p4 = point(0, 10)\n  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  \
         l8 = line(p4, p1)\n}}\n\nextrude E1 (profile: S1, distance: 10 mm, op: new)\n"
    );
    let mut tools = Vec::new();
    for i in 0..n {
        let x = i * 10 + 5;
        let _ = write!(
            t,
            "\nsketch C{i} (plane: XY, next_id: 3) {{\n  p1 = point({x}, 5)\n  c2 = circle(p1, 2)\n}}\n\
             extrude T{i} (profile: C{i}, distance: 30 mm, symmetric: true, op: new)\n"
        );
        tools.push(format!("T{i}"));
    }
    let _ = writeln!(t, "\ncombine K1 (body: E1, tools: [{}], op: cut)", tools.join(", "));
    t
}

/// `lines` chained count-2 patterns of a 10 mm cube (security review V1): each line doubles
/// the body.
fn chained_doc(lines: u32) -> String {
    let mut t = pattern_doc(2);
    for i in 1..lines {
        let _ = writeln!(t, "pattern_linear P{} (direction: X, count: 2, spacing: {} mm)", i + 1, 20u64 << i);
    }
    t
}

fn evaluate(text: &str) -> (Duration, gusset_eval::Evaluation) {
    let doc = gusset_doc::Document::from_text(text).unwrap_or_else(|e| panic!("{e}"));
    let mut ev = gusset_eval::Evaluator::new(kernel());
    let t = Instant::now();
    let r = ev.evaluate(&doc, None, &());
    (t.elapsed(), r)
}

fn time(text: &str) -> (Duration, usize) {
    let (took, r) = evaluate(text);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let faces = r.bodies.iter().map(|b| b.topology.faces.len()).sum();
    (took, faces)
}

#[test]
#[ignore = "timing report; run in release: cargo test --release -p gusset-cli --test perf_booleans -- --ignored --nocapture"]
fn perf_booleans_report() {
    for n in [5, 10, 20, 31] {
        let (t, f) = time(&holes_doc(n));
        eprintln!("holes {:>4}: {t:>10.3?} ({f} faces)", n * n);
    }
    for c in [100, 500] {
        let (t, f) = time(&pattern_doc(c));
        eprintln!("pattern_linear count {c:>4}: {t:>10.3?} ({f} faces)");
    }
    for n in [10, 50] {
        let (t, f) = time(&combine_doc(n));
        eprintln!("combine cut {n:>4} tools: {t:>10.3?} ({f} faces)");
    }
    let (t, r) = evaluate(&chained_doc(30));
    let failed = r.errors().first().map(|e| e.feature.clone());
    eprintln!("30 chained count-2 patterns: {t:>10.3?} (first failure: {failed:?})");
}

/// Review H3: 400 holes from one sketch rebuild in a few seconds (105 s before the holes were
/// cut in one boolean; about 2 s in release), also in a debug build of the Rust code.
#[test]
fn holes_400_budget() {
    let (t, f) = time(&holes_doc(20));
    eprintln!("holes_400: {t:?}");
    assert_eq!(f, 406);
    assert!(t < Duration::from_secs(15), "400 holes took {t:?}");
}

/// Security review V1: a chain of doubling patterns stops at the face limit instead of
/// running for hours.
#[test]
fn chained_patterns_stop_at_the_budget() {
    let (t, r) = evaluate(&chained_doc(30));
    eprintln!("30 chained patterns: {t:?}");
    let Some(gusset_eval::FeatureStatus::Failed(e)) = r.status("P11") else { panic!("{:?}", r.status("P11")) };
    assert_eq!(e.code, gusset_eval::ErrorCode::TooComplex);
    assert!(matches!(r.status("P10"), Some(gusset_eval::FeatureStatus::Ok)));
    assert!(t < Duration::from_secs(60), "took {t:?}");
}
