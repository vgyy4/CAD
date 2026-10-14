//! The ElCLib port against the C++ original: `elclib.txt` was printed by
//! `oracle/elclib.cpp` (OCCT 8.0.1), every value as its exact f64 bit pattern. Results
//! must be identical, bit for bit, except for last-place differences of the math library
//! (see `MAX_ULPS`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::elclib;
use occt_port::gp::{Ax2, Circ, Dir, Elips, Hypr, Lin, Parab, Pnt, Vec, XYZ};

fn f(word: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(word, 16).unwrap_or_else(|e| panic!("{word}: {e}")))
}

fn xyz(v: &XYZ) -> [f64; 3] {
    [v.x(), v.y(), v.z()]
}

/// Pushes the coordinates of points and vectors in output order.
#[derive(Default)]
struct Out(Vec_);
type Vec_ = std::vec::Vec<f64>;

impl Out {
    fn p(&mut self, p: &Pnt) {
        self.0.extend(xyz(p.xyz()));
    }
    fn v(&mut self, v: &Vec) {
        self.0.extend(xyz(v.xyz()));
    }
    fn s(&mut self, x: f64) {
        self.0.push(x);
    }
}

/// Distance in units in the last place (0 = identical bits).
fn ulps(a: f64, b: f64) -> u64 {
    let key = |x: f64| {
        let b = x.to_bits() as i64;
        if b < 0 { i64::MIN - b } else { b }
    };
    key(a).abs_diff(key(b))
}

/// The port uses the `libm` crate (the same bits on every platform, PROMPT.md
/// Determinism) where OCCT calls the C library; libm's sin, cos, sinh, cosh, asinh and acos
/// are within 2 ulps of glibc's, and cancellation (a hyperbola point is the difference of
/// two large terms) carries that into the result. So a value may differ by a few ulps of
/// the largest value on its line, never more.
const MAX_ULPS: f64 = 8.0;

#[derive(Default)]
struct Tally {
    values: usize,
    exact: usize,
    worst: u64,
}

fn check(t: &mut Tally, line: &str, want: &[f64], got: &[f64]) {
    assert_eq!(want.len(), got.len(), "{line}");
    let scale = want.iter().fold(1.0_f64, |m, w| m.max(w.abs()));
    let tolerance = MAX_ULPS * f64::EPSILON * scale;
    for (i, (w, g)) in want.iter().zip(got).enumerate() {
        let d = ulps(*w, *g);
        assert!((w - g).abs() <= tolerance, "value {i}: want {w:e}, got {g:e} ({d} ulps)\n{line}");
        t.values += 1;
        t.exact += usize::from(d == 0);
        t.worst = t.worst.max(d);
    }
}

#[test]
fn elclib_matches_occt() {
    let text = include_str!("elclib.txt");
    let mut cases = 0;
    let mut t = Tally::default();
    for line in text.lines() {
        let parts: std::vec::Vec<&str> = line.split(" | ").collect();
        let head: std::vec::Vec<&str> = parts[0].split_whitespace().collect();
        match head[0] {
            "case" => {
                let k: u32 = head[1].parse().unwrap();
                let a: std::vec::Vec<f64> = head[2..].iter().map(|w| f(w)).collect();
                let want: std::vec::Vec<f64> = parts[1].split_whitespace().map(f).collect();
                let ax = Ax2::new(
                    Pnt::new(a[0], a[1], a[2]),
                    Dir::new(a[3], a[4], a[5]).unwrap(),
                    &Dir::new(a[6], a[7], a[8]).unwrap(),
                )
                .unwrap();
                let (r1, r2, u) = (a[9], a[10], a[11]);
                let q = Pnt::new(a[12], a[13], a[14]);
                let mut o = Out::default();
                match k {
                    0 => {
                        let l = Lin::from_axis(ax.axis());
                        let (p, v1) = elclib::d1_lin(u, &l);
                        o.p(&p);
                        o.v(&v1);
                        o.s(elclib::parameter_lin(&l, &q));
                        (1..=4).for_each(|n| o.v(&elclib::dn_lin(u, &l, n)));
                    }
                    1 => {
                        let c = Circ::new(ax, r1).unwrap();
                        let (p, v1, v2, v3) = elclib::d3_circ(u, &c);
                        o.p(&p);
                        [v1, v2, v3].iter().for_each(|v| o.v(v));
                        o.s(elclib::parameter_circ(&c, &q));
                        (1..=4).for_each(|n| o.v(&elclib::dn_circ(u, &c, n)));
                    }
                    2 => {
                        let e = Elips::new(ax, r1, r2).unwrap();
                        let (p, v1, v2, v3) = elclib::d3_elips(u, &e);
                        o.p(&p);
                        [v1, v2, v3].iter().for_each(|v| o.v(v));
                        o.s(elclib::parameter_elips(&e, &q));
                        (1..=4).for_each(|n| o.v(&elclib::dn_elips(u, &e, n)));
                    }
                    3 => {
                        let h = Hypr::new(ax, r1, r2).unwrap();
                        let (p, v1, v2, v3) = elclib::d3_hypr(u, &h);
                        o.p(&p);
                        [v1, v2, v3].iter().for_each(|v| o.v(v));
                        o.s(elclib::parameter_hypr(&h, &q));
                        (1..=4).for_each(|n| o.v(&elclib::dn_hypr(u, &h, n)));
                    }
                    4 => {
                        let pa = Parab::new(ax, r2).unwrap();
                        let (p, v1, v2) = elclib::d2_parab(u, &pa);
                        o.p(&p);
                        [v1, v2].iter().for_each(|v| o.v(v));
                        o.s(elclib::parameter_parab(&pa, &q));
                        (1..=4).for_each(|n| o.v(&elclib::dn_parab(u, &pa, n)));
                    }
                    _ => panic!("unknown case {k}"),
                }
                check(&mut t, line, &want, &o.0);
                cases += 1;
            }
            "period" => {
                let a: std::vec::Vec<f64> = head[1..].iter().map(|w| f(w)).collect();
                // `period x a b | in_period u1 u2 | adjusted_u1 adjusted_u2`
                let mid: std::vec::Vec<f64> = parts[1].split_whitespace().map(f).collect();
                check(&mut t, line, &mid[..1], &[elclib::in_period(a[0], a[1], a[2])]);
                let adj: std::vec::Vec<f64> = parts[2].split_whitespace().map(f).collect();
                let (o1, o2) = elclib::adjust_periodic(a[1], a[2], 1e-9, mid[1], mid[2]);
                check(&mut t, line, &adj, &[o1, o2]);
                cases += 1;
            }
            other => panic!("unknown line kind {other}"),
        }
    }
    assert!(cases > 500, "{cases}");
    println!("{} values, {} bit-identical, worst {} ulps", t.values, t.exact, t.worst);
    // Almost everything must be bit-identical; differences come from the math library only.
    assert!(t.exact * 100 >= t.values * 95, "{} of {} identical", t.exact, t.values);
}
