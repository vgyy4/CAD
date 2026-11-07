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

/// Independent gouge check: the cutter shape (own formula) against random points on the mesh.
#[test]
fn no_gouge_and_no_floating_against_brute_force() {
    let (p, i) = bumpy();
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
    // Dense surface samples, vertices included.
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
    for (cutter, r, c) in
        [(Cutter::Flat, 2.5, 0.0), (Cutter::Ball, 2.5, 2.5), (Cutter::Bull { corner_radius: 1.0 }, 2.5, 1.0)]
    {
        let h = |rho: f64| {
            let s = rho - (r - c);
            if s <= 0.0 { 0.0 } else { c - libm::sqrt(c * c - s * s) }
        };
        let pts = all_z(cutter, 2.0 * r, &p, &i);
        let mut checked = 0;
        for q in pts.iter().step_by(7) {
            let mut best = f64::NEG_INFINITY;
            for s in &samples {
                let rho = libm::hypot(s[0] - q[0], s[1] - q[1]);
                if rho <= r {
                    best = best.max(s[2] - h(rho));
                }
            }
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
