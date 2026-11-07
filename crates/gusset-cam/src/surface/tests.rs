#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use super::*;

fn setup(d: f64) -> Setup {
    Setup {
        tool_diameter: d,
        top: 20.0,
        depth: 60.0,
        stepdown: 2.0,
        safe: 40.0,
        feed: 800.0,
        plunge: 200.0,
        rpm: 12000.0,
        ramp_deg: 0.0,
        tabs: None,
        climb: true,
    }
}

/// A height field over `[x0, x1] x [y0, y1]` as a triangle grid of `n x n` cells.
fn field(x0: f64, x1: f64, y0: f64, y1: f64, n: usize, f: impl Fn(f64, f64) -> f64) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut pos = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            let x = x0 + (x1 - x0) * i as f64 / n as f64;
            let y = y0 + (y1 - y0) * j as f64 / n as f64;
            pos.push([x as f32, y as f32, f(x, y) as f32]);
        }
    }
    let mut idx = Vec::new();
    let w = n as u32 + 1;
    for j in 0..n as u32 {
        for i in 0..n as u32 {
            let a = j * w + i;
            idx.extend([a, a + 1, a + w + 1, a, a + w + 1, a + w]);
        }
    }
    (pos, idx)
}

fn spec(step: f64, stepover: f64) -> RasterSpec {
    RasterSpec { angle_deg: 0.0, stepover, step, bounds: None, stock_to_leave: 0.0, tolerance: 1e-4 }
}

fn cut_points(t: &Toolpath) -> Vec<[f64; 3]> {
    t.moves
        .iter()
        .filter_map(|m| match m {
            Move::Line { to, plunge: false } => Some(*to),
            _ => None,
        })
        .collect()
}

#[test]
fn flat_cutter_on_a_plane_cuts_at_its_height() {
    let (p, i) = field(0.0, 30.0, 0.0, 30.0, 4, |_, _| 5.0);
    let sp = RasterSpec { stock_to_leave: 0.25, ..spec(1.0, 2.0) };
    let t = raster_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Flat, &sp, &setup(6.0)).unwrap();
    let pts = cut_points(&t);
    assert!(!pts.is_empty());
    for q in &pts {
        assert!((q[2] - 5.25).abs() < 1e-9, "{q:?}");
    }
    // A flat plane merges to one cut per line.
    assert_eq!(pts.len(), t.rings);
}

#[test]
fn ball_cutter_follows_the_sphere_offset() {
    // Cap of radius 20 centred on the origin (z = sqrt(400 - rho^2), rho <= 12), a fine mesh
    // inscribed in the sphere: the mesh lies below it by the sag, 3e-4 here.
    let (p, i) = field(-12.0, 12.0, -12.0, 12.0, 240, |x, y| libm::sqrt(400.0 - x * x - y * y));
    let sp = RasterSpec { angle_deg: 30.0, ..spec(0.5, 3.0) };
    let t = raster_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &sp, &setup(6.0)).unwrap();
    let mut n = 0;
    for q in cut_points(&t) {
        let rho2 = q[0] * q[0] + q[1] * q[1];
        // Contact stays on the cap while the contact point is inside the mesh.
        if rho2 > 7.0 * 7.0 {
            continue;
        }
        let want = libm::sqrt(23.0 * 23.0 - rho2) - 3.0;
        assert!(q[2] <= want + 1e-6 && q[2] > want - 1e-3, "{q:?} want {want}");
        n += 1;
    }
    assert!(n > 50, "{n}");
}

#[test]
fn ball_cutter_on_a_45_degree_ramp_is_exact() {
    let (p, i) = field(0.0, 40.0, -20.0, 20.0, 1, |x, _| x);
    let sp = RasterSpec { bounds: Some(([4.0, -10.0], [36.0, 10.0])), ..spec(1.0, 2.0) };
    let t = raster_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &sp, &setup(6.0)).unwrap();
    let pts = cut_points(&t);
    assert!(!pts.is_empty());
    for q in &pts {
        let want = q[0] + 3.0 * (std::f64::consts::SQRT_2 - 1.0);
        assert!((q[2] - want).abs() < 1e-6, "{q:?} want {want}");
    }
}

fn bumpy() -> (Vec<[f32; 3]>, Vec<u32>) {
    field(0.0, 30.0, 0.0, 24.0, 60, |x, y| 6.0 + 2.0 * libm::sin(x * 0.7) * libm::cos(y * 0.9) + 0.1 * x)
}

fn all_z(cutter: Cutter, d: f64, p: &[[f32; 3]], i: &[u32]) -> Vec<[f64; 3]> {
    let t = raster_finish(
        &Surface3d { positions: p, indices: i },
        cutter,
        &RasterSpec { tolerance: 0.0, ..spec(0.7, 1.3) },
        &setup(d),
    )
    .unwrap();
    cut_points(&t)
}

#[test]
fn bull_nose_degenerates_to_flat_and_ball() {
    let (p, i) = bumpy();
    let same = |a: &[[f64; 3]], b: &[[f64; 3]]| {
        assert_eq!(a.len(), b.len());
        for (u, v) in a.iter().zip(b) {
            assert!(
                (u[0] - v[0]).abs() < 1e-9 && (u[1] - v[1]).abs() < 1e-9 && (u[2] - v[2]).abs() < 1e-9,
                "{u:?} {v:?}"
            );
        }
    };
    same(&all_z(Cutter::Flat, 5.0, &p, &i), &all_z(Cutter::Bull { corner_radius: 0.0 }, 5.0, &p, &i));
    same(&all_z(Cutter::Ball, 5.0, &p, &i), &all_z(Cutter::Bull { corner_radius: 2.5 }, 5.0, &p, &i));
    // A real bull nose lies between flat (highest) and ball (lowest).
    let flat = all_z(Cutter::Flat, 5.0, &p, &i);
    let ball = all_z(Cutter::Ball, 5.0, &p, &i);
    let bull = all_z(Cutter::Bull { corner_radius: 1.0 }, 5.0, &p, &i);
    let mut some_between = false;
    for (n, q) in bull.iter().enumerate() {
        // Same sample positions are not guaranteed after merging: compare by drop at the point.
        let _ = n;
        let at = |set: &[[f64; 3]]| {
            set.iter().find(|r| (r[0] - q[0]).abs() < 1e-9 && (r[1] - q[1]).abs() < 1e-9).map(|r| r[2])
        };
        if let (Some(f), Some(b)) = (at(&flat), at(&ball)) {
            assert!(q[2] <= f + 1e-9 && q[2] >= b - 1e-9, "{q:?} {f} {b}");
            if q[2] > b + 1e-3 && q[2] < f - 1e-3 {
                some_between = true;
            }
        }
    }
    assert!(some_between);
}

/// Dense surface samples of a mesh (vertices and random points on each triangle).
fn surface_samples(p: &[[f32; 3]], i: &[u32]) -> Vec<[f64; 3]> {
    let mut seed = 0x1234_5678_9abc_def0_u64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let tri = |k: usize| -> [[f64; 3]; 3] {
        let g = |n: usize| {
            let v = p[i[3 * k + n] as usize];
            [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
        };
        [g(0), g(1), g(2)]
    };
    let mut samples: Vec<[f64; 3]> = p.iter().map(|v| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]).collect();
    for k in 0..i.len() / 3 {
        let t = tri(k);
        for _ in 0..12 {
            let (mut u, mut v) = (rnd(), rnd());
            if u + v > 1.0 {
                u = 1.0 - u;
                v = 1.0 - v;
            }
            let w = 1.0 - u - v;
            samples.push([
                w * t[0][0] + u * t[1][0] + v * t[2][0],
                w * t[0][1] + u * t[1][1] + v * t[2][1],
                w * t[0][2] + u * t[1][2] + v * t[2][2],
            ]);
        }
    }
    samples
}

/// Brute-force tip Z at `q` over sampled surface points, with its own cutter formula.
fn brute_tip(samples: &[[f64; 3]], q: &[f64; 3], r: f64, c: f64) -> f64 {
    let h = |rho: f64| {
        let s = rho - (r - c);
        if s <= 0.0 { 0.0 } else { c - libm::sqrt(c * c - s * s) }
    };
    let mut best = f64::NEG_INFINITY;
    for s in samples {
        let rho = libm::hypot(s[0] - q[0], s[1] - q[1]);
        if rho <= r {
            best = best.max(s[2] - h(rho));
        }
    }
    best
}

/// Independent gouge check: the cutter shape (own formula) against random points on the mesh.
#[test]
fn no_gouge_and_no_floating_against_brute_force() {
    let (p, i) = bumpy();
    let samples = surface_samples(&p, &i);
    for (cutter, r, c) in
        [(Cutter::Flat, 2.5, 0.0), (Cutter::Ball, 2.5, 2.5), (Cutter::Bull { corner_radius: 1.0 }, 2.5, 1.0)]
    {
        let pts = all_z(cutter, 2.0 * r, &p, &i);
        let mut checked = 0;
        for q in pts.iter().step_by(7) {
            let best = brute_tip(&samples, q, r, c);
            if best.is_finite() {
                // Never below the brute-force bound (no gouge), and close to it (not floating).
                assert!(q[2] >= best - 1e-6, "gouge {cutter:?} {q:?} {best}");
                assert!(q[2] - best < 0.1, "floating {cutter:?} {q:?} {best}");
                checked += 1;
            }
        }
        assert!(checked > 20, "{checked}");
    }
}

#[test]
fn never_below_the_stock_bottom_and_errors() {
    let (p, i) = field(0.0, 20.0, 0.0, 20.0, 2, |_, _| -100.0);
    let s = setup(6.0);
    let t = raster_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &spec(1.0, 2.0), &s).unwrap();
    for q in cut_points(&t) {
        assert!((q[2] - (s.top - s.depth)).abs() < 1e-12);
    }
    let sf = Surface3d { positions: &p, indices: &i };
    let bad = RasterSpec { step: 0.0, ..spec(1.0, 2.0) };
    assert!(matches!(raster_finish(&sf, Cutter::Flat, &bad, &s), Err(CamError::Invalid(_))));
    let tiny = spec(1e-4, 1e-4);
    assert!(matches!(raster_finish(&sf, Cutter::Flat, &tiny, &s), Err(CamError::TooLarge(_))));
    assert!(matches!(
        raster_finish(&sf, Cutter::Bull { corner_radius: 4.0 }, &spec(1.0, 2.0), &s),
        Err(CamError::Invalid(_))
    ));
    let low = Setup { safe: -50.0, ..s };
    let (p2, i2) = field(0.0, 20.0, 0.0, 20.0, 2, |_, _| 5.0);
    assert!(matches!(
        raster_finish(&Surface3d { positions: &p2, indices: &i2 }, Cutter::Flat, &spec(1.0, 2.0), &low),
        Err(CamError::Invalid(_))
    ));
}

#[test]
fn gcode_is_well_formed() {
    let (p, i) = bumpy();
    let s = setup(6.0);
    let t = raster_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &spec(0.5, 2.0), &s).unwrap();
    let g = t.gcode(&s, "surface");
    assert!(g.starts_with("(surface)\n") && g.contains("G21 G90 G17") && g.contains("S12000 M3"));
    assert!(g.ends_with("M5\nM30\n"));
    assert!(!g.contains("NaN") && !g.contains("inf"));
    assert!(g.lines().filter(|l| l.starts_with("G1 ")).count() > 100);
    let st = t.stats(&s);
    assert!(st.cut > 100.0 && st.minutes.is_finite() && st.minutes > 0.0);
}

// ---- waterline and steep/shallow ----

fn wspec(stepdown: f64, tolerance: f64) -> WaterlineSpec {
    WaterlineSpec { stepdown, tolerance, bounds: None, stock_to_leave: 0.0 }
}

const PI: f64 = std::f64::consts::PI;

/// A plate (two triangles) `[-half, half]^2` at z = 0.
fn plate(half: f64, pos: &mut Vec<[f32; 3]>, idx: &mut Vec<u32>) {
    let b = pos.len() as u32;
    for (x, y) in [(-half, -half), (half, -half), (half, half), (-half, half)] {
        pos.push([x as f32, y as f32, 0.0]);
    }
    idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
}

/// A vertical cylinder boss of radius `r` from z = 0 to `h`, with a top fan.
fn boss(r: f64, h: f64, n: usize, pos: &mut Vec<[f32; 3]>, idx: &mut Vec<u32>) {
    let b = pos.len() as u32;
    for k in 0..n {
        let a = 2.0 * PI * k as f64 / n as f64;
        let (x, y) = (r * libm::cos(a), r * libm::sin(a));
        pos.push([x as f32, y as f32, 0.0]);
        pos.push([x as f32, y as f32, h as f32]);
    }
    let c = pos.len() as u32;
    pos.push([0.0, 0.0, h as f32]);
    for k in 0..n as u32 {
        let k1 = (k + 1) % n as u32;
        let (b0, t0, b1, t1) = (b + 2 * k, b + 2 * k + 1, b + 2 * k1, b + 2 * k1 + 1);
        idx.extend([b0, b1, t1, b0, t1, t0, t0, t1, c]);
    }
}

/// A hemisphere (z >= 0) of radius `r` centred on the origin, `nl` rings by `nm` meridians.
fn hemisphere(r: f64, nl: usize, nm: usize, pos: &mut Vec<[f32; 3]>, idx: &mut Vec<u32>) {
    let b = pos.len() as u32;
    for i in 0..=nl {
        let el = 0.5 * PI * i as f64 / nl as f64;
        for k in 0..nm {
            let a = 2.0 * PI * k as f64 / nm as f64;
            let (rho, z) = (r * libm::cos(el), r * libm::sin(el));
            pos.push([(rho * libm::cos(a)) as f32, (rho * libm::sin(a)) as f32, z as f32]);
        }
    }
    for i in 0..nl as u32 {
        for k in 0..nm as u32 {
            let k1 = (k + 1) % nm as u32;
            let (a, bb) = (b + i * nm as u32 + k, b + i * nm as u32 + k1);
            let (c, d) = (a + nm as u32, bb + nm as u32);
            idx.extend([a, bb, d, a, d, c]);
        }
    }
}

/// Cutting moves grouped by plunge: one polyline per loop or raster run.
fn polylines(t: &Toolpath) -> Vec<Vec<[f64; 3]>> {
    let mut out: Vec<Vec<[f64; 3]>> = Vec::new();
    for m in &t.moves {
        match m {
            Move::Line { to, plunge: true } => out.push(vec![*to]),
            Move::Line { to, plunge: false } => out.last_mut().unwrap().push(*to),
            _ => {}
        }
    }
    out
}

fn signed_area(lp: &[[f64; 3]]) -> f64 {
    lp.windows(2).map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1]).sum::<f64>() / 2.0
}

#[test]
fn waterline_around_a_boss_is_at_r_plus_tool_radius() {
    let (mut p, mut i) = (Vec::new(), Vec::new());
    plate(40.0, &mut p, &mut i);
    boss(10.0, 10.0, 360, &mut p, &mut i);
    let s = Setup { top: 10.0, depth: 8.0, safe: 30.0, stepdown: 2.0, ..setup(6.0) };
    let sf = Surface3d { positions: &p, indices: &i };
    let t = waterline_finish(&sf, Cutter::Flat, &wspec(2.0, 0.005), &s).unwrap();
    // Level 10 is the boss top itself: the tool tip cannot rest above it, no contour. Then one
    // loop per stepdown down to the bottom at 2.
    assert_eq!(t.levels, vec![8.0, 6.0, 4.0, 2.0]);
    assert_eq!(t.rings, 4);
    let loops = polylines(&t);
    assert_eq!(loops.len(), 4);
    for (lp, z) in loops.iter().zip([8.0, 6.0, 4.0, 2.0]) {
        assert!(lp.len() > 20);
        assert_eq!(lp.first().unwrap(), lp.last().unwrap(), "closed");
        for q in lp {
            assert!((q[2] - z).abs() < 1e-12);
            let rho = libm::hypot(q[0], q[1]);
            // Distance 13 from the axis, up to the 360-gon's sag (4e-4) and the bisection.
            assert!((rho - 13.0).abs() < 0.01, "{q:?} {rho}");
        }
        // Climb: the boss (material) is on the right, so the loop runs clockwise.
        assert!(signed_area(lp) < 0.0);
    }
    // Conventional milling runs the other way.
    let conv = Setup { climb: false, ..s };
    let t2 = waterline_finish(&sf, Cutter::Flat, &wspec(2.0, 0.005), &conv).unwrap();
    assert!(signed_area(&polylines(&t2)[0]) > 0.0);
}

#[test]
fn waterline_below_the_plate_goes_around_the_plate() {
    let (mut p, mut i) = (Vec::new(), Vec::new());
    plate(20.0, &mut p, &mut i);
    let s = Setup { top: 5.0, depth: 10.0, safe: 30.0, stepdown: 2.0, ..setup(6.0) };
    let t = waterline_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Flat, &wspec(2.0, 0.01), &s).unwrap();
    // Plate at 0: levels 5, 3, 1 are above it (nothing), -1, -3, -5 are below it.
    assert_eq!(t.levels, vec![-1.0, -3.0, -5.0]);
    for lp in polylines(&t) {
        for q in &lp {
            // The tool edge touches the plate's edge: the centre is 3 from the square.
            let (ex, ey) = ((q[0].abs() - 20.0).max(0.0), (q[1].abs() - 20.0).max(0.0));
            let d = if ex > 0.0 && ey > 0.0 { libm::hypot(ex, ey) } else { ex.max(ey) };
            assert!((d - 3.0).abs() < 0.02, "{q:?}");
        }
    }
}

#[test]
fn waterline_on_a_hemisphere_with_a_ball_follows_the_offset_sphere() {
    let (mut p, mut i) = (Vec::new(), Vec::new());
    hemisphere(10.0, 32, 64, &mut p, &mut i);
    let s = Setup { top: 10.0, depth: 7.0, safe: 40.0, stepdown: 2.0, ..setup(4.0) };
    let t = waterline_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &wspec(2.0, 0.02), &s).unwrap();
    // Level 10 (the apex tip) has no contour; 8, 6, 4 and the bottom 3 do.
    assert_eq!(t.levels, vec![8.0, 6.0, 4.0, 3.0]);
    assert_eq!(t.rings, 4);
    for lp in polylines(&t) {
        let z = lp[0][2];
        // Ball centre at z + 2 on a sphere of radius R + r about the origin.
        let want = libm::sqrt(12.0 * 12.0 - (z + 2.0) * (z + 2.0));
        for q in &lp {
            let rho = libm::hypot(q[0], q[1]);
            assert!((rho - want).abs() < 0.03, "{q:?} {rho} want {want}");
        }
    }
}

/// A smaller bumpy height field for the slower tests.
fn bumpy_small() -> (Vec<[f32; 3]>, Vec<u32>) {
    field(0.0, 18.0, 0.0, 14.0, 36, |x, y| 6.0 + 2.0 * libm::sin(x * 0.7) * libm::cos(y * 0.9) + 0.1 * x)
}

#[test]
fn waterline_never_gouges_and_never_floats() {
    let (p, i) = bumpy_small();
    let samples = surface_samples(&p, &i);
    for (cutter, r, c) in
        [(Cutter::Flat, 2.5, 0.0), (Cutter::Ball, 2.5, 2.5), (Cutter::Bull { corner_radius: 1.0 }, 2.5, 1.0)]
    {
        let s = Setup { top: 10.0, depth: 6.0, safe: 30.0, stepdown: 1.0, ..setup(2.0 * r) };
        let t = waterline_finish(&Surface3d { positions: &p, indices: &i }, cutter, &wspec(1.0, 0.02), &s).unwrap();
        assert!(t.levels.len() >= 4, "{:?}", t.levels);
        let mut checked = 0;
        for lp in polylines(&t) {
            for q in &lp {
                let best = brute_tip(&samples, q, r, c);
                if best.is_finite() {
                    assert!(q[2] >= best - 1e-3, "gouge {cutter:?} {q:?} {best}");
                    // The brute force is only as dense as its samples (poor along the mesh's
                    // edge): check floating away from the border.
                    if (r..18.0 - r).contains(&q[0]) && (r..14.0 - r).contains(&q[1]) {
                        assert!(q[2] - best < 0.1, "floating {cutter:?} {q:?} {best}");
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 30, "{checked}");
    }
}

#[test]
fn waterline_errors_and_gcode() {
    let (mut p, mut i) = (Vec::new(), Vec::new());
    plate(20.0, &mut p, &mut i);
    boss(5.0, 6.0, 90, &mut p, &mut i);
    let sf = Surface3d { positions: &p, indices: &i };
    let s = Setup { top: 6.0, depth: 6.0, safe: 30.0, ..setup(4.0) };
    assert!(matches!(waterline_finish(&sf, Cutter::Flat, &wspec(0.0, 0.01), &s), Err(CamError::Invalid(_))));
    assert!(matches!(waterline_finish(&sf, Cutter::Flat, &wspec(1e-5, 0.01), &s), Err(CamError::TooLarge(_))));
    // Fine grid over big bounds.
    let big = WaterlineSpec { bounds: Some(([-1e5, -1e5], [1e5, 1e5])), ..wspec(1.0, 1e-9) };
    assert!(matches!(waterline_finish(&sf, Cutter::Flat, &big, &s), Err(CamError::TooLarge(_))));
    let low = Setup { safe: 1.0, ..s };
    assert!(matches!(waterline_finish(&sf, Cutter::Flat, &wspec(1.0, 0.01), &low), Err(CamError::Invalid(_))));
    // Levels all above the surface: nothing to cut.
    let high = Setup { top: 100.0, depth: 10.0, safe: 200.0, ..s };
    assert!(matches!(waterline_finish(&sf, Cutter::Flat, &wspec(2.0, 0.01), &high), Err(CamError::NothingToCut(_))));
    let t = waterline_finish(&sf, Cutter::Flat, &wspec(2.0, 0.01), &s).unwrap();
    let g = t.gcode(&s, "waterline");
    assert!(g.starts_with("(waterline)\n") && g.contains("G21 G90 G17") && g.contains("S12000 M3"));
    assert!(g.ends_with("M5\nM30\n"));
    assert!(!g.contains("NaN") && !g.contains("inf"));
    assert!(g.lines().filter(|l| l.starts_with("G1 ")).count() > 20);
    let st = t.stats(&s);
    assert!(st.cut > 50.0 && st.minutes.is_finite() && st.minutes > 0.0);
}

/// Hemisphere of radius 10 on a 32 x 32 plate at z = 0.
fn dome() -> (Vec<[f32; 3]>, Vec<u32>) {
    let (mut p, mut i) = (Vec::new(), Vec::new());
    plate(16.0, &mut p, &mut i);
    hemisphere(10.0, 32, 64, &mut p, &mut i);
    (p, i)
}

fn is_level(z: f64) -> bool {
    [5.0, 2.5, 0.0].iter().any(|l| (z - l).abs() < 1e-12)
}

#[test]
fn steep_shallow_splits_a_dome_on_a_plate() {
    let (p, i) = dome();
    let sf = Surface3d { positions: &p, indices: &i };
    let s = Setup { top: 10.0, depth: 10.0, safe: 40.0, stepdown: 2.5, ..setup(4.0) };
    let sp = SteepShallowSpec::new(
        wspec(2.5, 0.02),
        RasterSpec { angle_deg: 0.0, stepover: 1.0, step: 0.5, bounds: None, stock_to_leave: 0.0, tolerance: 1e-3 },
    );
    assert!((sp.steep_deg - 45.0).abs() < 1e-12);
    let t = steep_shallow_finish(&sf, Cutter::Ball, &sp, &s).unwrap();
    // The ball's offset sphere (R + r = 12) is steeper than 45 degrees below
    // z = 12 / sqrt 2 - 2 = 6.5: waterlines at 5, 2.5 and 0 (where it meets the plate), none at
    // 7.5 (shallow) or 10 (the apex tip).
    assert_eq!(t.levels, vec![5.0, 2.5, 0.0]);
    let runs = polylines(&t);
    // A waterline is a constant-Z run at one of the levels around the foot; rasters are the rest.
    let (wl, rs): (Vec<_>, Vec<_>) = runs.iter().partition(|r| {
        let rho = libm::hypot(r[0][0], r[0][1]);
        r.len() > 20 && is_level(r[0][2]) && r.iter().all(|q| (q[2] - r[0][2]).abs() < 1e-12) && rho > 8.0 && rho < 13.0
    });
    assert_eq!(wl.len(), 3);
    for lp in &wl {
        // Radius sqrt(12^2 - (z + 2)^2).
        let want = libm::sqrt(12.0 * 12.0 - (lp[0][2] + 2.0) * (lp[0][2] + 2.0));
        for q in lp.iter() {
            assert!((libm::hypot(q[0], q[1]) - want).abs() < 0.04, "{q:?} want {want}");
        }
    }
    // No raster point in the steep foot of the dome (offset slope above 45 degrees from
    // rho = 8.5 to where the offset meets the plate at 11.8).
    let mut segs: Vec<([f64; 3], [f64; 3])> = Vec::new();
    for r in &rs {
        for w in r.windows(2) {
            let rho = libm::hypot(w[1][0], w[1][1]);
            assert!(!(9.2..=11.3).contains(&rho), "raster in the steep band {:?}", w[1]);
            segs.push((w[0], w[1]));
        }
    }
    // Coverage: every XY point that is clearly shallow (top of the dome, the plate) is within
    // half a stepover of a raster segment.
    let dist = |(a, b): &([f64; 3], [f64; 3]), x: f64, y: f64| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let u = if l2 < 1e-18 { 0.0 } else { (((x - a[0]) * dx + (y - a[1]) * dy) / l2).clamp(0.0, 1.0) };
        libm::hypot(x - (a[0] + u * dx), y - (a[1] + u * dy))
    };
    let mut tested = 0;
    for ix in -14..=14 {
        for iy in -14..=14 {
            let (x, y) = (f64::from(ix) + 0.13, f64::from(iy) + 0.31);
            let rho = libm::hypot(x, y);
            if !(7.6..12.9).contains(&rho) {
                let d = segs.iter().map(|sg| dist(sg, x, y)).fold(f64::INFINITY, f64::min);
                assert!(d <= 0.5 + 1e-6, "uncovered ({x}, {y}) rho {rho} d {d}");
                tested += 1;
            }
        }
    }
    assert!(tested > 300, "{tested}");
    let g = t.gcode(&s, "steep-shallow");
    assert!(g.starts_with("(steep-shallow)\n") && g.ends_with("M5\nM30\n"));
    assert!(!g.contains("NaN") && !g.contains("inf"));
}

#[test]
fn steep_shallow_threshold_extremes_and_errors() {
    let (p, i) = dome();
    let sf = Surface3d { positions: &p, indices: &i };
    let s = Setup { top: 10.0, depth: 10.0, safe: 40.0, stepdown: 2.5, ..setup(4.0) };
    let rs =
        RasterSpec { angle_deg: 0.0, stepover: 2.0, step: 1.0, bounds: None, stock_to_leave: 0.0, tolerance: 1e-3 };
    // Nearly everything steep: waterlines at every level with a contour.
    let steep = SteepShallowSpec { steep_deg: 0.5, ..SteepShallowSpec::new(wspec(2.5, 0.02), rs) };
    let t = steep_shallow_finish(&sf, Cutter::Ball, &steep, &s).unwrap();
    assert_eq!(t.levels, vec![7.5, 5.0, 2.5, 0.0]);
    // Nothing steep: raster only.
    let flat = SteepShallowSpec { steep_deg: 89.9, ..SteepShallowSpec::new(wspec(2.5, 0.02), rs) };
    let t = steep_shallow_finish(&sf, Cutter::Ball, &flat, &s).unwrap();
    assert!(t.rings > 10);
    let bad = SteepShallowSpec { steep_deg: 90.0, ..flat };
    assert!(matches!(steep_shallow_finish(&sf, Cutter::Ball, &bad, &s), Err(CamError::Invalid(_))));
}
