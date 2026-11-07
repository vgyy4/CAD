#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
//! Timings of the Run 9 jobs:
//! `cargo test --release -p gusset-cam perf_report -- --ignored --nocapture`.

use super::*;

fn rapid_length(t: &Toolpath) -> f64 {
    let mut prev = [0.0; 3];
    let mut sum = 0.0;
    for m in &t.moves {
        match m {
            Move::Rapid(q) => {
                sum += libm::sqrt((0..3).map(|k| (q[k] - prev[k]) * (q[k] - prev[k])).sum::<f64>());
                prev = *q;
            }
            Move::Line { to, .. } => prev = *to,
            _ => {}
        }
    }
    sum
}

#[test]
#[ignore = "timing report"]
fn perf_report() {
    let (mut p, mut i) = (Vec::new(), Vec::new());
    plate(40.0, &mut p, &mut i);
    hemisphere(30.0, 60, 120, &mut p, &mut i);
    let surf = Surface3d { positions: &p, indices: &i };
    let s = Setup { top: 30.0, depth: 30.0, safe: 50.0, stepdown: 0.4, ..setup(6.0) };
    let w = WaterlineSpec {
        stepdown: 0.4,
        tolerance: 0.01,
        bounds: Some(([-40.0, -40.0], [40.0, 40.0])),
        stock_to_leave: 0.0,
    };
    let r = RasterSpec {
        angle_deg: 0.0,
        stepover: 0.5,
        step: 0.5,
        bounds: Some(([-40.0, -40.0], [40.0, 40.0])),
        stock_to_leave: 0.0,
        tolerance: 0.01,
    };
    let sp = SteepShallowSpec::new(w, r);
    let t0 = std::time::Instant::now();
    let t = steep_shallow_finish(&surf, Cutter::Ball, &sp, &s).unwrap();
    eprintln!("steep dome: {:?}, {} moves, rapid {:.0} mm", t0.elapsed(), t.moves.len(), rapid_length(&t));
    let t0 = std::time::Instant::now();
    let t = waterline_finish(&surf, Cutter::Ball, &w, &s).unwrap();
    eprintln!("waterline dome: {:?}, {} moves", t0.elapsed(), t.moves.len());
}
