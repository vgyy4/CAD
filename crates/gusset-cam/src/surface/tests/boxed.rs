#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
//! Run 9: contours clipped by the bounds box (F9-1), and the pencil and speed fixes.

use super::*;

/// An axis-aligned block `[x0, x1] x [y0, y1] x [0, h]` (12 triangles).
pub(super) fn block(x0: f64, x1: f64, y0: f64, y1: f64, h: f64) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut pos = Vec::new();
    for z in [0.0, h] {
        for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
            pos.push([x as f32, y as f32, z as f32]);
        }
    }
    let idx = vec![
        0, 2, 1, 0, 3, 2, // bottom
        4, 5, 6, 4, 6, 7, // top
        0, 1, 5, 0, 5, 4, // y0
        1, 2, 6, 1, 6, 5, // x1
        2, 3, 7, 2, 7, 6, // y1
        3, 0, 4, 3, 4, 7, // x0
    ];
    (pos, idx)
}

/// Worst depth (mm) a ball of radius `r` on the cutting moves of `t` (and along the segments
/// between consecutive points of one cut) goes into the sampled mesh.
fn worst_gouge(t: &Toolpath, samples: &[[f64; 3]], r: f64) -> f64 {
    let mut worst = 0.0_f64;
    for lp in polylines(t) {
        for w in lp.windows(2) {
            for k in 0..=8 {
                let f = f64::from(k) / 8.0;
                let q = [
                    w[0][0] + f * (w[1][0] - w[0][0]),
                    w[0][1] + f * (w[1][1] - w[0][1]),
                    w[0][2] + f * (w[1][2] - w[0][2]),
                ];
                // A ball a hair smaller: a wall sample exactly at the ball radius is a touch, not a gouge.
                let best = brute_tip(samples, &q, r - 0.01, r - 0.01) + 0.01;
                if best.is_finite() {
                    worst = worst.max(best - q[2]);
                }
            }
        }
    }
    worst
}

#[test]
fn waterline_with_the_footprint_as_box_never_runs_along_the_box() {
    let (p, i) = block(0.0, 60.0, 0.0, 40.0, 20.0);
    let samples = surface_samples(&p, &i);
    let s = Setup { top: 20.0, depth: 20.0, safe: 40.0, stepdown: 100.0, ..setup(6.0) };
    let surf = Surface3d { positions: &p, indices: &i };
    let mut w = wspec(100.0, 0.02);
    w.bounds = Some(([0.0, 0.0], [60.0, 40.0]));
    // The tool centre cannot reach the walls inside the footprint: nothing to cut (it used to
    // cut round the box edge, with the ball centre on the wall plane).
    let e = waterline_finish(&surf, Cutter::Ball, &w, &s);
    assert!(matches!(e, Err(CamError::NothingToCut(_))), "{e:?}");
    // Box larger along y only: open runs along the long sides, none along the cut-off ends.
    w.bounds = Some(([0.0, -10.0], [60.0, 50.0]));
    let t = waterline_finish(&surf, Cutter::Ball, &w, &s).unwrap();
    assert_eq!(t.rings, 2, "{:?}", polylines(&t).len());
    assert!(worst_gouge(&t, &samples, 3.0) < 0.03);
    for lp in polylines(&t) {
        for q in &lp {
            assert!((q[1] + 3.0).abs() < 0.05 || (q[1] - 43.0).abs() < 0.05, "{q:?}");
        }
    }
}

#[test]
fn waterline_and_steep_on_a_dome_in_a_tight_box_do_not_gouge() {
    let (mut p, mut i) = (Vec::new(), Vec::new());
    hemisphere(10.0, 24, 48, &mut p, &mut i);
    let samples = surface_samples(&p, &i);
    let s = Setup { top: 10.0, depth: 10.0, safe: 30.0, stepdown: 2.0, ..setup(4.0) };
    let surf = Surface3d { positions: &p, indices: &i };
    let mut w = wspec(2.0, 0.02);
    w.bounds = Some(([-10.0, -10.0], [10.0, 10.0]));
    let t = waterline_finish(&surf, Cutter::Ball, &w, &s).unwrap();
    let worst = worst_gouge(&t, &samples, 2.0);
    // The contour is simplified within 0.02 mm sideways, 0.07 mm in height on the steep rim.
    assert!(worst < 0.12, "gouge {worst}");
    // No long segment runs along the box edge.
    for lp in polylines(&t) {
        for seg in lp.windows(2) {
            let len = libm::hypot(seg[1][0] - seg[0][0], seg[1][1] - seg[0][1]);
            for k in 0..2 {
                let along = (seg[0][k].abs() - 10.0).abs() < 1e-3 && (seg[1][k].abs() - 10.0).abs() < 1e-3;
                assert!(!along || len < 0.3, "{seg:?}");
            }
        }
    }
    let sp = SteepShallowSpec::new(w, spec(0.5, 1.0));
    let t = steep_shallow_finish(&surf, Cutter::Ball, &sp, &s).unwrap();
    let worst = worst_gouge(&t, &samples, 2.0);
    assert!(worst < 0.12, "steep gouge {worst}");
}
