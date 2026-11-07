#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
//! Run 9: vertical walls (F9-2) and the ends of a crease (F9-3).

use super::*;

const PI: f64 = std::f64::consts::PI;

/// Which outer corner of a square plate the boss-base point `(x, y)` is nearest to.
fn corner_of(x: f64, y: f64) -> u32 {
    match (x >= 0.0, y >= 0.0) {
        (false, false) => 0,
        (true, false) => 1,
        (true, true) => 2,
        (false, true) => 3,
    }
}

/// A plate `[-half, half]^2` at z = 0 with a prism boss (`n`-gon of circumradius `rad`, height
/// `h`) in the middle. One closed mesh surface, no gaps.
fn plate_with_boss(half: f64, rad: f64, h: f64, n: usize, phase: f64) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    // Outer corners 0..4, boss base ring 4..4+n, boss top ring 4+n..4+2n, top centre.
    for (x, y) in [(-half, -half), (half, -half), (half, half), (-half, half)] {
        pos.push([x as f32, y as f32, 0.0]);
    }
    let ring = |k: usize| {
        let t = phase + 2.0 * PI * (k % n) as f64 / n as f64;
        (rad * libm::cos(t), rad * libm::sin(t))
    };
    for z in [0.0, h] {
        for k in 0..n {
            let (x, y) = ring(k);
            pos.push([x as f32, y as f32, z as f32]);
        }
    }
    let c = pos.len() as u32;
    pos.push([0.0, 0.0, h as f32]);
    let base = |k: usize| 4 + (k % n) as u32;
    let top = |k: usize| 4 + n as u32 + (k % n) as u32;
    for k in 0..n {
        idx.extend([base(k), base(k + 1), top(k + 1), base(k), top(k + 1), top(k)]);
        idx.extend([top(k), top(k + 1), c]);
        let (c0, c1) = (corner_of(ring(k).0, ring(k).1), corner_of(ring(k + 1).0, ring(k + 1).1));
        idx.extend([base(k + 1), base(k), c0]);
        if c0 != c1 {
            idx.extend([base(k + 1), c0, c1]);
        }
    }
    (pos, idx)
}

#[test]
fn cylinder_boss_on_a_plate_gives_a_circle_at_the_floor() {
    let (p, i) = plate_with_boss(20.0, 10.0, 10.0, 96, 0.5 * PI / 96.0);
    let t = pencil_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &spec(), &setup(6.0)).unwrap();
    assert_eq!(t.rings, 1, "{}", t.rings);
    let apothem = 10.0 * libm::cos(PI / 96.0);
    let pts = cuts(&t);
    assert!(pts.len() > 20, "{}", pts.len());
    for q in &pts {
        assert!((libm::hypot(q[0], q[1]) - (apothem + 3.0)).abs() < 0.03, "{q:?}");
        assert!(q[2].abs() < 1e-3, "{q:?}");
    }
    // Closed, and all the way round.
    let (f, l) = (pts[0], pts[pts.len() - 1]);
    assert!(libm::hypot(f[0] - l[0], f[1] - l[1]) < 0.01, "{f:?} {l:?} {}", pts.len());
    let len: f64 = pts.windows(2).map(|w| libm::hypot(w[1][0] - w[0][0], w[1][1] - w[0][1])).sum();
    assert!((len - 2.0 * PI * 13.0).abs() < 1.5, "{len}");
}

#[test]
fn square_boss_on_a_plate_gives_a_closed_loop_at_the_floor() {
    // A 4-gon turned by 45 degrees is an axis-parallel square; circumradius 10 gives a half-side of 7.07.
    let (p, i) = plate_with_boss(20.0, 10.0, 10.0, 4, 0.25 * PI);
    let t = pencil_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &spec(), &setup(6.0)).unwrap();
    assert_eq!(t.rings, 1);
    let pts = cuts(&t);
    let hs = 10.0 / std::f64::consts::SQRT_2;
    for q in &pts {
        assert!(q[2].abs() < 1e-3, "{q:?}");
        let (du, dv) = ((q[0].abs() - hs).max(0.0), (q[1].abs() - hs).max(0.0));
        assert!((libm::hypot(du, dv) - 3.0).abs() < 0.02, "{q:?}");
    }
    let (f, l) = (pts[0], pts[pts.len() - 1]);
    assert!(libm::hypot(f[0] - l[0], f[1] - l[1]) < 0.01);
}

#[test]
fn a_wall_leaning_over_the_floor_is_found() {
    // Floor, then a wall at x = 0 rising 10 mm and leaning towards x < 0 by 0.1 degrees.
    let w = 10.0 * libm::tan(0.1_f64.to_radians());
    let (p, i) = field(-10.0, 10.0, 160, |x, _| {
        if x < -w {
            0.0
        } else if x > 0.0 {
            10.0
        } else {
            10.0 * (x + w) / w
        }
    });
    let t = pencil_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &spec(), &setup(6.0)).unwrap();
    let pts = cuts(&t);
    for q in &pts {
        assert!((q[0] + 3.0).abs() < 0.1 && q[2].abs() < 0.01, "{q:?}");
    }
}

#[test]
fn the_line_stops_at_the_end_of_the_crease() {
    // A V-groove of slopes 45 degrees, 30 mm long (y in 0 ..= 30).
    let (mut p, i) = field(-10.0, 10.0, 40, |x, _| x.abs());
    for v in &mut p {
        v[1] = ((f64::from(v[1]) + 10.0) * 1.5) as f32;
    }
    let t = pencil_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &spec(), &setup(6.0)).unwrap();
    let zc = 3.0 * (std::f64::consts::SQRT_2 - 1.0);
    for q in cuts(&t) {
        assert!(q[1] > -1e-6 && q[1] < 30.0 + 1e-6, "{q:?}");
        assert!((q[2] - zc).abs() < 1e-4, "{q:?}");
    }
}
