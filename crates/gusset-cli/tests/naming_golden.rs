//! Persistent-naming golden scenarios (ADR 0003, review H7) with the real OCCT kernel;
//! the same scenarios run against the mock kernel in gusset-commands.

#[cfg(gusset_occt)]
#[test]
fn naming_golden_scenarios_with_occt() {
    use std::sync::Arc;
    let kernel: Arc<dyn gusset_kernel::Kernel> =
        Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")));
    if let Err(e) = gusset_commands::golden::all(&kernel) {
        panic!("{e}");
    }
}

/// PERFORMANCE.md "Rebuild, 100-feature part" (`rebuild_100`): < 5 s full, < 1 s for a
/// one-feature parameter change (incremental), with the real kernel.
#[cfg(gusset_occt)]
#[test]
fn rebuild_100_budget() {
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    let mut text = String::from(
        "gusset 1\nunits mm\nparam h = 6 mm\n\
         sketch S0 (plane: XY, next_id: 9) {\n  p1 = point(0, 0)\n  p2 = point(200, 0)\n  p3 = point(200, 200)\n  \
         p4 = point(0, 200)\n  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}\n\
         extrude E0 (profile: S0, distance: 5 mm, op: new)\n",
    );
    for i in 0..49 {
        let (x, y) = ((i % 7) as f64 * 28.0 + 5.0, (i / 7) as f64 * 28.0 + 5.0);
        let dist = if i == 48 { "h".to_string() } else { format!("{} mm", 3 + i % 4) };
        text.push_str(&format!(
            "sketch B{i} (plane: E0/cap(end), next_id: 9) {{\n  p1 = point({x}, {y})\n  p2 = point({}, {y})\n  \
             p3 = point({}, {})\n  p4 = point({x}, {})\n  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  \
             l8 = line(p4, p1)\n}}\nextrude X{i} (profile: B{i}, distance: {dist}, op: add, body: E0)\n",
            x + 10.0,
            x + 10.0,
            y + 10.0,
            y + 10.0
        ));
    }
    let doc = gusset_doc::Document::from_text(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(doc.features.len(), 100);
    let kernel: Arc<dyn gusset_kernel::Kernel> =
        Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")));
    let mut ev = gusset_eval::Evaluator::new(kernel);
    let t = Instant::now();
    let r = ev.evaluate(&doc, None, &());
    let full = t.elapsed();
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let mut d2 = doc.clone();
    d2.params[0].expr = gusset_units::expr::parse("9 mm").unwrap();
    let t = Instant::now();
    let r2 = ev.evaluate(&d2, None, &());
    let incremental = t.elapsed();
    assert!(r2.errors().is_empty(), "{:?}", r2.errors());
    assert_eq!(r2.recomputed, 1, "only the last boss depends on h");
    eprintln!("rebuild_100: full {full:?}, incremental {incremental:?}");
    assert!(full < Duration::from_secs(5), "full rebuild took {full:?}");
    assert!(incremental < Duration::from_secs(1), "incremental rebuild took {incremental:?}");
}
