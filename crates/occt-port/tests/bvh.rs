//! Numerical contract for OCCT 8.0.1 BVH core APIs.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::bvh::{Aabb3, Ray3, Tools3};
use std::collections::{BTreeMap, BTreeSet};
const MIN_DISTINCT: usize = 16;
const FAMILIES: &[&str] = &[
    "box_valid",
    "box_area",
    "box_size",
    "box_center",
    "box_center_axis",
    "box_add",
    "box_combine",
    "box_is_out_box",
    "box_is_out_point",
    "box_contains",
    "box_clear",
    "tools_box_distance",
    "tools_point_distance",
    "tools_projection",
    "tools_triangle_projection",
    "tools_triangle_distance",
    "ray_inverse",
    "box_degenerate",
    "box_coincident",
    "box_tangent",
    "box_parallel",
    "box_perpendicular",
    "box_overload_bounds",
    "tools_box_distance_corners",
    "tools_projection_corners",
];
const EXACT: &[&str] = FAMILIES;
const GOLDEN_LINES: usize = 400;
struct Rd<'a> {
    words: std::str::SplitWhitespace<'a>,
}
impl<'a> Rd<'a> {
    fn new(text: &'a str) -> Self {
        Self { words: text.split_whitespace() }
    }
    fn word(&mut self) -> &'a str {
        self.words.next().expect("record word")
    }
    fn f(&mut self) -> f64 {
        f64::from_bits(u64::from_str_radix(self.word(), 16).expect("float bits"))
    }
    fn i(&mut self) -> i32 {
        self.word().parse().expect("integer")
    }
    fn done(&mut self) {
        assert!(self.words.next().is_none(), "extra input tokens");
    }
}
fn bits(x: f64) -> u64 {
    x.to_bits()
}
fn run_line(family: &str, rd: &mut Rd<'_>, outs: &[&str]) {
    assert_eq!(outs.len(), 41);
    let v: Vec<f64> = (0..6).map(|_| rd.f()).collect();
    let axis = rd.i() as usize;
    let ta = [rd.f(), rd.f(), rd.f()];
    let tb = [rd.f(), rd.f(), rd.f()];
    let tc = [rd.f(), rd.f(), rd.f()];
    rd.done();
    let lo = [v[0], v[1], v[2]];
    let hi = [v[0] + v[3].abs(), v[1] + v[4].abs(), v[2] + v[5].abs()];
    let lo2 = [v[3], v[4], v[5]];
    let hi2 = [v[3] + 1., v[4] + 1., v[5] + 1.];
    let p = lo2;
    let b = Aabb3::new(lo, hi);
    let c = Aabb3::new(lo2, hi2);
    let (inside, overlap) = b.contains_box(&c);
    let proj = Tools3::point_box_projection(p, &b);
    let tr = Tools3::point_triangle_projection(p, ta, tb, tc);
    let ray = Ray3::new(lo, p);
    let mut added = b;
    added.add(p);
    let mut combined = b;
    combined.combine(&c);
    let mut cleared = b;
    cleared.clear();
    let mut expected = Vec::<String>::new();
    expected.push(if b.is_valid() { "1" } else { "0" }.to_string());
    expected.push(format!("{:016x}", bits(b.area())));
    let sz = b.size();
    for x in sz {
        expected.push(format!("{:016x}", bits(x)));
    }
    let ce = b.center();
    for x in ce {
        expected.push(format!("{:016x}", bits(x)));
    }
    expected.push(format!("{:016x}", bits(b.center_axis(axis))));
    expected.push(if b.is_out_box(&c) { "1" } else { "0" }.to_string());
    expected.push(if b.is_out_point(p) { "1" } else { "0" }.to_string());
    expected.push(if inside { "1" } else { "0" }.to_string());
    expected.push(if overlap { "1" } else { "0" }.to_string());
    expected.push(format!("{:016x}", bits(Tools3::box_box_square_distance(&b, &c))));
    expected.push(format!("{:016x}", bits(Tools3::point_box_square_distance(p, &b))));
    for x in proj {
        expected.push(format!("{:016x}", bits(x)));
    }
    for x in tr.point {
        expected.push(format!("{:016x}", bits(x)));
    }
    expected.push((tr.state as i32).to_string());
    expected.push(tr.first_node.to_string());
    expected.push(tr.last_node.to_string());
    expected.push(format!("{:016x}", bits(Tools3::point_triangle_square_distance(p, ta, tb, tc))));
    for x in ray.inverse_direction() {
        expected.push(format!("{:016x}", bits(x)));
    }
    for x in added.corner_min().into_iter().chain(added.corner_max()) {
        expected.push(format!("{:016x}", bits(x)));
    }
    for x in combined.corner_min().into_iter().chain(combined.corner_max()) {
        expected.push(format!("{:016x}", bits(x)));
    }
    expected.push(if cleared.is_valid() { "1" } else { "0" }.to_string());
    assert_eq!(expected.len(), outs.len(), "output schema {family}");
    for (a, b) in expected.iter().zip(outs) {
        assert_eq!(a, *b, "{family}: expected {a}, got {b}");
    }
}
#[test]
fn bvh_oracle_records() {
    let mut seen = BTreeSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    for line in include_str!("bvh.txt").lines() {
        assert!(seen.insert(line.to_owned()), "duplicate record line");
        let (l, r) = line.split_once(" | ").expect("record separator");
        let mut rd = Rd::new(l);
        let fam = rd.word();
        assert!(FAMILIES.contains(&fam));
        let outs: Vec<_> = r.split_whitespace().collect();
        run_line(fam, &mut rd, &outs);
        *counts.entry(fam.to_owned()).or_default() += 1;
    }
    assert_eq!(counts.len(), FAMILIES.len());
    assert_eq!(counts.values().sum::<usize>(), GOLDEN_LINES);
    for fam in FAMILIES {
        assert!(counts.get(*fam).copied().unwrap_or(0) >= MIN_DISTINCT, "missing or sparse {fam}");
    }
    assert_eq!(EXACT.len(), FAMILIES.len());
}
