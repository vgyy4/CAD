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

fn field(lo: f64, hi: f64, n: usize, f: impl Fn(f64, f64) -> f64) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut pos = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            let x = lo + (hi - lo) * i as f64 / n as f64;
            let y = lo + (hi - lo) * j as f64 / n as f64;
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

fn spec() -> PencilSpec {
    PencilSpec { bitangency_angle_deg: 20.0, tolerance: 1e-3, bounds: None }
}

fn cuts(t: &Toolpath) -> Vec<[f64; 3]> {
    t.moves
        .iter()
        .filter_map(|m| match m {
            Move::Line { to, .. } => Some(*to),
            _ => None,
        })
        .collect()
}

#[test]
fn right_angle_inside_corner_gives_a_straight_line_at_the_analytic_height() {
    // Two 45 degree planes meeting along x = 0: z = |x|.
    let (p, i) = field(-10.0, 10.0, 40, |x, _| x.abs());
    let r = 3.0;
    let t = pencil_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &spec(), &setup(2.0 * r)).unwrap();
    assert_eq!(t.rings, 1);
    let pts = cuts(&t);
    // A straight crease simplifies to its two ends.
    assert_eq!(pts.len(), 2, "{pts:?}");
    // The ball touches both planes: centre r*sqrt2 above the corner, tip r*(sqrt2 - 1).
    let zc = r * (std::f64::consts::SQRT_2 - 1.0);
    for q in &pts {
        assert!(q[0].abs() < 1e-6, "{q:?}");
        assert!((q[2] - zc).abs() < 1e-6, "{q:?}");
    }
    let (ya, yb) = (pts[0][1].min(pts[1][1]), pts[0][1].max(pts[1][1]));
    assert!(ya < -8.0 && yb > 8.0, "{ya} {yb}");
}

#[test]
fn flat_surface_has_nothing_to_do() {
    let (p, i) = field(-10.0, 10.0, 20, |_, _| 3.0);
    let e = pencil_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &spec(), &setup(6.0));
    assert!(matches!(e, Err(CamError::NothingToCut(_))));
}

#[test]
fn a_fillet_larger_than_the_cutter_and_a_ridge_give_no_line() {
    // Concave circular valley of radius 12 (4 x the cutter radius).
    let (p, i) = field(-10.0, 10.0, 200, |x, _| 12.0 - libm::sqrt(144.0 - x * x));
    let e = pencil_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &spec(), &setup(6.0));
    assert!(matches!(e, Err(CamError::NothingToCut(_))), "{e:?}");
    // A convex ridge z = -|x| is not a corner to pencil.
    let (p, i) = field(-10.0, 10.0, 40, |x, _| -x.abs());
    let e = pencil_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &spec(), &setup(6.0));
    assert!(matches!(e, Err(CamError::NothingToCut(_))), "{e:?}");
}

#[test]
fn errors_and_caps() {
    let (p, i) = field(-10.0, 10.0, 4, |x, _| x.abs());
    let s = Surface3d { positions: &p, indices: &i };
    assert!(matches!(pencil_finish(&s, Cutter::Flat, &spec(), &setup(6.0)), Err(CamError::Invalid(_))));
    let bad = PencilSpec { bitangency_angle_deg: 0.0, ..spec() };
    assert!(matches!(pencil_finish(&s, Cutter::Ball, &bad, &setup(6.0)), Err(CamError::Invalid(_))));
    // 20 mm / (0.05 / 8) nodes per side is far over the cap.
    assert!(matches!(pencil_finish(&s, Cutter::Ball, &spec(), &setup(0.1)), Err(CamError::TooLarge(_))));
}

mod rim;
mod walls;
