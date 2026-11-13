//! Annotation orders, sheet ranges, kept numbers and multi-unit parts (ADR 0052).

use crate::annotate::{self, Options, Renumber};
use crate::sch::{Mirror, Schematic};
use crate::tests_sch::place;

/// Four unnumbered resistors: A (10, 30), B (20, 10), C (30, 20), D (10, 10).
fn unnumbered() -> Schematic {
    let mut s = Schematic::new_empty("a").unwrap();
    for (x, y) in [(10.0, 30.0), (20.0, 10.0), (30.0, 20.0), (10.0, 10.0)] {
        place(&mut s, "R", [x, y], 0.0, Mirror::None, 1, "R?");
    }
    s
}

/// The new references by symbol index.
fn by_symbol(plan: &[Renumber]) -> Vec<String> {
    let mut v: Vec<(usize, String)> = plan.iter().map(|r| (r.symbol, r.new.clone())).collect();
    v.sort();
    v.into_iter().map(|x| x.1).collect()
}

#[test]
fn annotation_orders_by_x_or_y_first() {
    let s = unnumbered();
    // X first: D, A, B, C.
    let x = annotate::plan(&[&s], Options::default());
    assert_eq!(by_symbol(&x), ["R2", "R3", "R4", "R1"]);
    // Y first: D, B, C, A.
    let y = annotate::plan(&[&s], Options { y_first: true, ..Options::default() });
    assert_eq!(by_symbol(&y), ["R4", "R2", "R3", "R1"]);
}

#[test]
fn annotation_ranges_and_keeping_numbers() {
    let a = unnumbered();
    let mut b = Schematic::new_empty("b").unwrap();
    place(&mut b, "R", [10.0, 10.0], 0.0, Mirror::None, 1, "R7");
    place(&mut b, "R", [20.0, 10.0], 0.0, Mirror::None, 1, "R?");
    // One run of numbers across sheets: 1 to 4 on the first sheet, 5 skips R7 (kept), then 6.
    let p = annotate::plan(&[&a, &b], Options::default());
    assert_eq!(p.iter().filter(|r| r.sheet == 1).map(|r| r.new.as_str()).collect::<Vec<_>>(), ["R5"]);
    assert!(p.iter().all(|r| r.old != "R7"));
    // Sheet n starts at n * 100: R100.. on the first sheet, R200 on the second; R7 stays.
    let step = Options { sheet_step: 100, ..Options::default() };
    let p = annotate::plan(&[&a, &b], step);
    assert_eq!(by_symbol(&p.iter().filter(|r| r.sheet == 0).cloned().collect::<Vec<_>>()), ["R101", "R102", "R103", "R100"]);
    assert_eq!(p.iter().filter(|r| r.sheet == 1).map(|r| r.new.as_str()).collect::<Vec<_>>(), ["R200"]);
    // Everything again: R7 is renumbered too (the second sheet's first place is R7's).
    let all = annotate::plan(&[&a, &b], Options { all: true, sheet_step: 1000, ..Options::default() });
    assert_eq!(all.iter().filter(|r| r.sheet == 1).map(|r| r.new.as_str()).collect::<Vec<_>>(), ["R2000", "R2001"]);
    // A numbered symbol that already has its number is left out.
    let mut c = Schematic::new_empty("c").unwrap();
    place(&mut c, "R", [10.0, 10.0], 0.0, Mirror::None, 1, "R1");
    assert!(annotate::plan(&[&c], Options { all: true, ..Options::default() }).is_empty());
    assert_eq!(annotate::split_reference("R12"), ("R".to_string(), Some(12)));
    assert_eq!(annotate::split_reference("U?"), ("U".to_string(), None));
}

#[test]
fn units_of_a_part_share_a_number() {
    let mut s = Schematic::new_empty("u").unwrap();
    place(&mut s, "AMP", [10.0, 10.0], 0.0, Mirror::None, 1, "U?");
    place(&mut s, "AMP", [30.0, 10.0], 0.0, Mirror::None, 2, "U?");
    place(&mut s, "AMP", [50.0, 10.0], 0.0, Mirror::None, 1, "U?");
    let p = annotate::plan(&[&s], Options::default());
    let got: Vec<(&str, Option<&str>)> = p.iter().map(|r| (r.new.as_str(), r.unit_letter.as_deref())).collect();
    assert_eq!(got, [("U1", Some("A")), ("U1", Some("B")), ("U2", Some("A"))]);
    // A new unit 2 joins the kept U1 (unit 1) instead of taking a number.
    let mut k = Schematic::new_empty("k").unwrap();
    place(&mut k, "AMP", [10.0, 10.0], 0.0, Mirror::None, 1, "U1");
    place(&mut k, "AMP", [30.0, 10.0], 0.0, Mirror::None, 2, "U?");
    let p = annotate::plan(&[&k], Options::default());
    assert_eq!(p.len(), 1);
    assert_eq!((p[0].new.as_str(), p[0].unit_letter.as_deref()), ("U1", Some("B")));
    // Re-annotating all keeps units that share a reference together.
    let mut r = Schematic::new_empty("r").unwrap();
    place(&mut r, "AMP", [10.0, 10.0], 0.0, Mirror::None, 1, "U5");
    place(&mut r, "AMP", [30.0, 10.0], 0.0, Mirror::None, 2, "U5");
    let p = annotate::plan(&[&r], Options { all: true, ..Options::default() });
    assert_eq!(p.iter().map(|x| x.new.as_str()).collect::<Vec<_>>(), ["U1", "U1"]);
    assert_eq!(annotate::unit_letter(1), "A");
    assert_eq!(annotate::unit_letter(26), "Z");
    assert_eq!(annotate::unit_letter(27), "AA");
}
