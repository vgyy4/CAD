#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use super::*;

fn setup(d: f64) -> Setup {
    Setup {
        tool_diameter: d,
        top: 20.0,
        depth: 60.0,
        stepdown: 2.0,
        safe: 60.0,
        feed: 800.0,
        plunge: 200.0,
        rpm: 12000.0,
        ramp_deg: 0.0,
        tabs: None,
        climb: true,
    }
}

fn quad(pos: &mut Vec<[f32; 3]>, idx: &mut Vec<u32>, q: [[f32; 3]; 4]) {
    let b = pos.len() as u32;
    pos.extend(q);
    idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
}

/// A slot 8 wide (x in -4..4), floor at z = 0, walls 30 high, y in 0..20.
fn slot() -> (Vec<[f32; 3]>, Vec<u32>) {
    let (mut p, mut i) = (Vec::new(), Vec::new());
    let (y0, y1) = (0.0, 20.0);
    quad(&mut p, &mut i, [[-4.0, y0, 0.0], [4.0, y0, 0.0], [4.0, y1, 0.0], [-4.0, y1, 0.0]]);
    for (xa, xb) in [(-4.0, -20.0), (4.0, 20.0)] {
        quad(&mut p, &mut i, [[xa, y0, 30.0], [xb, y0, 30.0], [xb, y1, 30.0], [xa, y1, 30.0]]);
    }
    // Inner wall faces.
    for x in [-4.0, 4.0] {
        quad(&mut p, &mut i, [[x, y0, 0.0], [x, y1, 0.0], [x, y1, 30.0], [x, y0, 30.0]]);
    }
    (p, i)
}

fn pass() -> Toolpath {
    Toolpath {
        moves: vec![
            Move::Rapid([0.0, 2.0, 50.0]),
            Move::Line { to: [0.0, 2.0, 0.0], plunge: true },
            Move::Line { to: [0.0, 18.0, 0.0], plunge: false },
        ],
        ..Toolpath::default()
    }
}

fn holder(shank: f64, stickout: f64) -> Holder {
    Holder { shank_diameter: shank, flute_length: 5.0, holder_diameter: 12.0, stickout }
}

#[test]
fn short_stickout_hits_the_holder_long_stickout_is_clear() {
    let (p, i) = slot();
    let s = Surface3d { positions: &p, indices: &i };
    let r = check_holder(&s, Cutter::Ball, &setup(2.0), &holder(6.0, 10.0), &pass()).unwrap();
    assert!(r.total_collisions > 0);
    assert!(r.collisions.iter().all(|c| c.part == HolderPart::Holder));
    // Holder face at 10, wall top at 30.
    assert!((r.collisions[0].depth - 20.0).abs() < 1e-9);
    assert!((r.min_clearance + 20.0).abs() < 1e-9);

    let r = check_holder(&s, Cutter::Ball, &setup(2.0), &holder(6.0, 40.0), &pass()).unwrap();
    assert_eq!(r.total_collisions, 0);
    assert!(r.collisions.is_empty());
    assert!((r.min_clearance - 5.0).abs() < 1e-9, "{}", r.min_clearance);
}

#[test]
fn a_fat_shank_hits_the_walls() {
    let (p, i) = slot();
    let s = Surface3d { positions: &p, indices: &i };
    // Shank radius 5 > 4: the walls (30) are above the flute top (5), the holder is clear.
    let r = check_holder(&s, Cutter::Ball, &setup(2.0), &holder(10.0, 40.0), &pass()).unwrap();
    assert!(r.collisions.iter().any(|c| c.part == HolderPart::Shank));
    assert!(r.collisions.iter().all(|c| c.part == HolderPart::Shank));
    assert!((r.collisions[0].depth - 25.0).abs() < 1e-9);
}

#[test]
fn bad_holder_is_invalid() {
    let (p, i) = slot();
    let s = Surface3d { positions: &p, indices: &i };
    let bad = Holder { stickout: 1.0, ..holder(6.0, 10.0) };
    assert!(matches!(check_holder(&s, Cutter::Ball, &setup(2.0), &bad, &pass()), Err(CamError::Invalid(_))));
}

#[test]
fn safe_raster_lifts_until_clear() {
    let (p, i) = slot();
    let s = Surface3d { positions: &p, indices: &i };
    let sp = RasterSpec {
        angle_deg: 90.0,
        stepover: 1.0,
        step: 1.0,
        bounds: Some(([-2.0, 2.0], [2.0, 18.0])),
        stock_to_leave: 0.0,
        tolerance: 1e-4,
    };
    let st = setup(2.0);
    let h = holder(6.0, 10.0);
    let out = raster_finish_safe(&s, Cutter::Ball, &sp, &st, &h).unwrap();
    assert!(out.report.total_collisions > 0);
    // At x = +-2 the shank (radius 3) overlaps the wall at 4: its flute-top face (tip + 5)
    // must reach the wall top 30, so 25 (the holder alone would need 20).
    assert!((out.max_lift - 25.0).abs() < 1e-9, "{}", out.max_lift);
    assert!(out.lifted_samples > 0);
    let again = check_holder(&s, Cutter::Ball, &st, &h, &out.toolpath).unwrap();
    assert_eq!(again.total_collisions, 0, "{:?}", again.collisions.first());
    // A clear holder leaves the raster path untouched.
    let clear = raster_finish_safe(&s, Cutter::Ball, &sp, &st, &holder(3.0, 40.0)).unwrap();
    assert_eq!(clear.max_lift, 0.0);
    assert_eq!(clear.toolpath, raster_finish(&s, Cutter::Ball, &sp, &st).unwrap());
}

#[test]
fn moves_between_lifted_and_unlifted_samples_never_dip_under_the_requirement() {
    // Dogfooding F11-1: across the slot at y = 10 with a 4 mm shank (flute 5): beyond
    // |x| = 2 the shank overlaps a wall and the tip must be at 25; inside it may cut at 0.
    // Samples 1 mm apart from -3.3 straddle the edge (-2.3 lifted, -1.3 not).
    let (p, i) = slot();
    let s = Surface3d { positions: &p, indices: &i };
    let st = setup(2.0);
    let h = holder(4.0, 40.0);
    let path = Toolpath {
        moves: vec![
            Move::Rapid([-3.3, 10.0, 50.0]),
            Move::Line { to: [-3.3, 10.0, 0.0], plunge: true },
            Move::Line { to: [3.3, 10.0, 0.0], plunge: false },
        ],
        ..Toolpath::default()
    };
    let (out, rise, _) = lift_for_holder(&s, &st, &h, &path).unwrap();
    assert!((rise - 25.0).abs() < 1e-9, "{rise}");
    let mut ck = Checker::new(&s, &st, &h).unwrap();
    let mut at = [0.0; 3];
    for mv in &out.moves {
        let to = end_of(mv);
        if !matches!(mv, Move::Rapid(_)) {
            for k in 0..=200 {
                let t = f64::from(k) / 200.0;
                let q = [at[0] + (to[0] - at[0]) * t, at[1] + (to[1] - at[1]) * t, at[2] + (to[2] - at[2]) * t];
                assert!(q[2] >= ck.required_tip(q[0], q[1]) - 1e-6, "{q:?} under {}", ck.required_tip(q[0], q[1]));
            }
        }
        at = to;
    }
    // And it still cuts the middle at the floor.
    assert!(out.moves.iter().any(|m| matches!(m, Move::Line { to, .. } if to[2] == 0.0 && to[0].abs() < 1.5)));
}

#[test]
fn a_shank_tangent_to_a_wall_is_not_a_collision() {
    // Dogfooding F11-2: a 2 mm ball 1 mm from the wall at x = 4 with a 2 mm shank touches it.
    let (p, i) = slot();
    let s = Surface3d { positions: &p, indices: &i };
    let path = Toolpath {
        moves: vec![
            Move::Rapid([3.0, 2.0, 50.0]),
            Move::Line { to: [3.0, 2.0, 0.0], plunge: true },
            Move::Line { to: [3.0, 18.0, 0.0], plunge: false },
        ],
        ..Toolpath::default()
    };
    let r = check_holder(&s, Cutter::Ball, &setup(2.0), &holder(2.0, 40.0), &path).unwrap();
    assert_eq!(r.total_collisions, 0, "{:?}", r.collisions.first());
    let r = check_holder(&s, Cutter::Ball, &setup(2.0), &holder(2.2, 40.0), &path).unwrap();
    assert!(r.total_collisions > 0);
}
