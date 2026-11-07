#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
//! Run 10: raster chords across steep walls must not gouge (F10-1).

use super::*;

/// Worst amount (mm) any cutting segment of `t`, sampled every 0.05 mm, lies below the
/// drop-cutter height.
fn worst_chord_gouge(t: &Toolpath, p: &[[f32; 3]], i: &[u32], cutter: Cutter, d: f64) -> f64 {
    let r = d / 2.0;
    let c = match cutter {
        Cutter::Flat => 0.0,
        Cutter::Ball => r,
        Cutter::Bull { corner_radius } => corner_radius,
    };
    let (lo, hi) = mesh_bounds(p).unwrap();
    let mut dr = Dropper::new(&Surface3d { positions: p, indices: i }, Profile { r, c }, lo, hi).unwrap();
    let mut worst = 0.0_f64;
    let mut prev: Option<[f64; 3]> = None;
    for m in &t.moves {
        match m {
            Move::Line { to, plunge: false } => {
                if let Some(a) = prev {
                    let len = libm::sqrt((to[0] - a[0]).powi(2) + (to[1] - a[1]).powi(2) + (to[2] - a[2]).powi(2));
                    let n = (len / 0.05).ceil().max(1.0) as usize;
                    for k in 0..=n {
                        let f = k as f64 / n as f64;
                        let q = [a[0] + (to[0] - a[0]) * f, a[1] + (to[1] - a[1]) * f, a[2] + (to[2] - a[2]) * f];
                        // The shortfall of the tool tip against the cutter-location height, forgiven
                        // by a lateral shift of 0.004 mm (a wall is a step in the height; what
                        // counts is how far the tool is off horizontally, not the step's height).
                        let (dx, dy) = ((to[0] - a[0]) / len.max(1e-12), (to[1] - a[1]) / len.max(1e-12));
                        let hl = libm::hypot(dx, dy).max(1e-12);
                        let short = [-0.004, 0.0, 0.004]
                            .iter()
                            .filter_map(|s| dr.drop(q[0] + s * dx / hl, q[1] + s * dy / hl).map(|z| z - q[2]))
                            .fold(f64::INFINITY, f64::min);
                        if short.is_finite() {
                            worst = worst.max(short);
                        }
                    }
                }
                prev = Some(*to);
            }
            Move::Line { to, plunge: true } => prev = Some(*to),
            _ => prev = None,
        }
    }
    worst
}

fn cutters() -> [(Cutter, f64); 3] {
    [(Cutter::Ball, 6.0), (Cutter::Flat, 6.0), (Cutter::Bull { corner_radius: 1.0 }, 6.0)]
}

#[test]
fn raster_across_a_vertical_wall_never_gouges() {
    let (mut p, mut i) = (Vec::new(), Vec::new());
    plate(30.0, &mut p, &mut i);
    boss(8.0, 10.0, 64, &mut p, &mut i);
    for (cutter, d) in cutters() {
        for angle in [0.0, 30.0] {
            let sp = RasterSpec {
                angle_deg: angle,
                stepover: 1.0,
                step: 0.6,
                bounds: Some(([-20.0, -20.0], [20.0, 20.0])),
                stock_to_leave: 0.0,
                tolerance: 1e-3,
            };
            let t = raster_finish(&Surface3d { positions: &p, indices: &i }, cutter, &sp, &setup(d)).unwrap();
            let w = worst_chord_gouge(&t, &p, &i, cutter, d);
            assert!(w < 0.005, "{cutter:?} angle {angle}: gouge {w}");
        }
    }
}

#[test]
fn raster_across_a_steep_fillet_never_gouges() {
    // A smooth steep step from 10 down to 0 between radius 6 and 8.5.
    let (p, i) = field(-20.0, 20.0, -20.0, 20.0, 80, |x, y| {
        let t = ((libm::hypot(x, y) - 6.0) / 2.5).clamp(0.0, 1.0);
        10.0 * (1.0 - t * t * (3.0 - 2.0 * t))
    });
    for (cutter, d) in cutters() {
        let sp = RasterSpec {
            angle_deg: 0.0,
            stepover: 1.0,
            step: 0.7,
            bounds: Some(([-12.0, -12.0], [12.0, 12.0])),
            stock_to_leave: 0.0,
            tolerance: 1e-3,
        };
        let t = raster_finish(&Surface3d { positions: &p, indices: &i }, cutter, &sp, &setup(d)).unwrap();
        let w = worst_chord_gouge(&t, &p, &i, cutter, d);
        assert!(w < 0.005, "{cutter:?}: gouge {w}");
    }
}

#[test]
fn clipped_waterline_runs_end_on_the_box_edge_and_an_empty_box_says_why() {
    let (p, i) = boxed::block(0.0, 60.0, 0.0, 40.0, 20.0);
    let s = Setup { top: 20.0, depth: 20.0, safe: 40.0, stepdown: 100.0, ..setup(6.0) };
    let surf = Surface3d { positions: &p, indices: &i };
    let mut w = wspec(100.0, 0.02);
    // Long sides inside the box, the ends clipped: the runs reach x = 0 and x = 60 (F10-7).
    w.bounds = Some(([0.0, -10.0], [60.0, 50.0]));
    let t = waterline_finish(&surf, Cutter::Ball, &w, &s).unwrap();
    for lp in polylines(&t) {
        let (lo, hi) = lp.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, q| (a.0.min(q[0]), a.1.max(q[0])));
        assert!(lo.abs() < 1e-3 && (hi - 60.0).abs() < 1e-3, "{lo} {hi}");
    }
    // The box equal to the footprint: say that the contour lies outside it (F10-2), in
    // waterline and in steep/shallow.
    w.bounds = Some(([0.0, 0.0], [60.0, 40.0]));
    let CamError::NothingToCut(m) = waterline_finish(&surf, Cutter::Ball, &w, &s).unwrap_err() else { panic!() };
    assert!(m.contains("grow the region by the tool radius"), "{m}");
    let sp = SteepShallowSpec::new(w, spec(0.5, 1.0));
    let CamError::NothingToCut(m) = steep_shallow_finish(&surf, Cutter::Ball, &sp, &s).unwrap_err() else { panic!() };
    assert!(m.contains("grow the region by the tool radius"), "{m}");
}
