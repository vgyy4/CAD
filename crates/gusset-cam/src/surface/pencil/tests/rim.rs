#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
//! Run 10: a pocket's rim corners (F10-4a) and a depth that stops above the creases (F10-4b).

use super::*;

/// A closed 100 x 60 x 10 block with a pocket x 14..50, y 12..48 down to `floor`.
fn block_with_pocket(floor: f64) -> (Vec<[f32; 3]>, Vec<u32>) {
    let (xs, ys) = ([0.0, 14.0, 50.0, 100.0], [0.0, 12.0, 48.0, 60.0]);
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut quad = |a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]| {
        let n = pos.len() as u32;
        for p in [a, b, c, d] {
            pos.push([p[0] as f32, p[1] as f32, p[2] as f32]);
        }
        idx.extend([n, n + 1, n + 2, n, n + 2, n + 3]);
    };
    for i in 0..3 {
        for j in 0..3 {
            let (x0, x1, y0, y1) = (xs[i], xs[i + 1], ys[j], ys[j + 1]);
            if i == 1 && j == 1 {
                quad([x0, y0, floor], [x1, y0, floor], [x1, y1, floor], [x0, y1, floor]);
            } else {
                quad([x0, y0, 10.0], [x1, y0, 10.0], [x1, y1, 10.0], [x0, y1, 10.0]);
            }
            quad([x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0]);
        }
    }
    for i in 0..3 {
        let (x0, x1) = (xs[i], xs[i + 1]);
        quad([x0, 0.0, 0.0], [x1, 0.0, 0.0], [x1, 0.0, 10.0], [x0, 0.0, 10.0]);
        quad([x0, 60.0, 0.0], [x1, 60.0, 0.0], [x1, 60.0, 10.0], [x0, 60.0, 10.0]);
        if i == 1 {
            quad([x0, 12.0, floor], [x1, 12.0, floor], [x1, 12.0, 10.0], [x0, 12.0, 10.0]);
            quad([x0, 48.0, floor], [x1, 48.0, floor], [x1, 48.0, 10.0], [x0, 48.0, 10.0]);
        }
    }
    for j in 0..3 {
        let (y0, y1) = (ys[j], ys[j + 1]);
        quad([0.0, y0, 0.0], [0.0, y1, 0.0], [0.0, y1, 10.0], [0.0, y0, 10.0]);
        quad([100.0, y0, 0.0], [100.0, y1, 0.0], [100.0, y1, 10.0], [100.0, y0, 10.0]);
        if j == 1 {
            quad([14.0, y0, floor], [14.0, y1, floor], [14.0, y1, 10.0], [14.0, y0, 10.0]);
            quad([50.0, y0, floor], [50.0, y1, floor], [50.0, y1, 10.0], [50.0, y0, 10.0]);
        }
    }
    (pos, idx)
}

fn pocket_setup() -> Setup {
    Setup { top: 10.0, depth: 10.0, safe: 20.0, ..setup(4.0) }
}

#[test]
fn rim_corners_of_a_pocket_get_no_stubs() {
    let (p, i) = block_with_pocket(4.0);
    let sp = PencilSpec { bounds: Some(([5.0, 5.0], [60.0, 55.0])), ..spec() };
    let t = pencil_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &sp, &pocket_setup()).unwrap();
    // Every cut point is inside the pocket (the floor loop and the wall corner lines).
    for q in cuts(&t) {
        assert!(q[0] > 14.0 - 1e-6 && q[0] < 50.0 + 1e-6 && q[1] > 12.0 - 1e-6 && q[1] < 48.0 + 1e-6, "{q:?}");
    }
}

#[test]
fn a_depth_that_stops_above_the_creases_says_so() {
    let (p, i) = block_with_pocket(4.0);
    // The creases lie at z 4 and up to 10; the stock bottom is at z 12.
    let s = Setup { top: 14.0, depth: 2.0, safe: 20.0, ..setup(4.0) };
    let e = pencil_finish(&Surface3d { positions: &p, indices: &i }, Cutter::Ball, &spec(), &s).unwrap_err();
    let CamError::NothingToCut(m) = e else { panic!("{e:?}") };
    assert!(m.contains("below the depth"), "{m}");
}
