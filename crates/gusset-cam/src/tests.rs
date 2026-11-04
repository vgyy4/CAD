#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use super::*;
use gusset_kernel::ProfileEdge;

fn rect(w: f64, h: f64) -> Region2 {
    let p = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
    let edges = (0..4).map(|i| ProfileEdge { seg: Seg2::Line { a: p[i], b: p[(i + 1) % 4] }, tag: 0 }).collect();
    Region2 { outer: Loop2 { edges }, holes: Vec::new() }
}

fn circle(c: [f64; 2], r: f64) -> Loop2 {
    Loop2 { edges: vec![ProfileEdge { seg: Seg2::Circle { center: c, radius: r }, tag: 0 }] }
}

fn setup(d: f64) -> Setup {
    Setup {
        tool_diameter: d,
        top: 0.0,
        depth: 5.0,
        stepdown: 2.0,
        safe: 5.0,
        feed: 800.0,
        plunge: 200.0,
        rpm: 12000.0,
        ramp_deg: 0.0,
    }
}

fn extent(t: &Toolpath, z: f64) -> [f64; 4] {
    let mut e = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for m in &t.moves {
        let to = match m {
            Move::Line { to, plunge: false } | Move::Arc { to, .. } => *to,
            _ => continue,
        };
        if (to[2] - z).abs() < 1e-9 {
            e = [e[0].min(to[0]), e[1].min(to[1]), e[2].max(to[0]), e[3].max(to[1])];
        }
    }
    e
}

#[test]
fn a_pocket_stays_a_tool_radius_inside_and_steps_down() {
    let t = toolpath(&[rect(40.0, 20.0)], Operation::Pocket { stepover: 0.5 }, &setup(6.0)).unwrap();
    // 5 mm in passes of at most 2: three levels.
    assert_eq!(t.levels.len(), 3);
    assert!((t.levels[2] + 5.0).abs() < 1e-12);
    let e = extent(&t, -5.0);
    for (got, want) in e.iter().zip([3.0, 3.0, 37.0, 17.0]) {
        assert!((got - want).abs() < 1e-6, "{e:?}");
    }
    // Rings 3 mm apart until the middle: 14 mm across leaves rings at 3, 6, 9 (and the slot).
    assert!(t.rings >= 3, "{}", t.rings);
    let g = t.gcode(&setup(6.0), "pocket");
    assert!(g.starts_with("(pocket)\n") && g.contains("G21 G90 G17") && g.ends_with("M30\n"));
    assert!(g.contains("S12000 M3") && g.contains("G1 Z-5 F200"));
    let st = t.stats(&setup(6.0));
    assert!(st.cut > 3.0 * 2.0 * (34.0 + 14.0) && st.minutes > 0.0, "{st:?}");
}

#[test]
fn an_outside_profile_rounds_the_corners_and_a_hole_profile_is_a_circle() {
    let t = toolpath(&[rect(40.0, 20.0)], Operation::Profile { outside: true }, &setup(6.0)).unwrap();
    let e = extent(&t, -5.0);
    for (got, want) in e.iter().zip([-3.0, -3.0, 43.0, 23.0]) {
        assert!((got - want).abs() < 1e-6, "{e:?}");
    }
    assert_eq!(t.moves.iter().filter(|m| matches!(m, Move::Arc { .. })).count(), 4 * 3, "4 corner arcs per level");
    let mut plate = rect(40.0, 20.0);
    plate.holes.push(circle([20.0, 10.0], 5.0));
    // The hole alone: a 10 mm circle cut inside with a 4 mm tool runs on radius 3.
    let hole = Region2 { outer: circle([20.0, 10.0], 5.0), holes: Vec::new() };
    let t = toolpath(&[hole], Operation::Profile { outside: false }, &setup(4.0)).unwrap();
    for m in &t.moves {
        if let Move::Arc { to, center, .. } = m {
            let r = ((to[0] - center[0]).powi(2) + (to[1] - center[1]).powi(2)).sqrt();
            assert!((r - 3.0).abs() < 1e-9 && (center[0] - 20.0).abs() < 1e-9, "{r} {center:?}");
        }
    }
    assert!(t.gcode(&setup(4.0), "hole").contains("G3 "));
    // A pocket keeps clear of an island.
    let t = toolpath(&[plate], Operation::Pocket { stepover: 0.4 }, &setup(4.0)).unwrap();
    for m in &t.moves {
        if let Move::Line { to, plunge: false } | Move::Arc { to, .. } = m {
            let d = ((to[0] - 20.0).powi(2) + (to[1] - 10.0).powi(2)).sqrt();
            assert!(d >= 7.0 - 1e-6, "the tool enters the island: {to:?}");
        }
    }
}

#[test]
fn a_tool_too_big_or_bad_numbers_are_refused() {
    let small = rect(4.0, 4.0);
    assert!(matches!(
        toolpath(&[small], Operation::Pocket { stepover: 0.5 }, &setup(6.0)),
        Err(CamError::NothingToCut(_))
    ));
    let mut s = setup(6.0);
    s.stepdown = 0.0;
    assert!(matches!(
        toolpath(&[rect(40.0, 20.0)], Operation::Pocket { stepover: 0.5 }, &s),
        Err(CamError::Invalid(_))
    ));
    let mut s = setup(6.0);
    s.safe = -1.0;
    assert!(toolpath(&[rect(40.0, 20.0)], Operation::Profile { outside: true }, &s).is_err());
    assert!(toolpath(&[rect(40.0, 20.0)], Operation::Pocket { stepover: 1.5 }, &setup(6.0)).is_err());
}

#[test]
fn a_ramp_spirals_down_the_ring_without_plunging_below_the_level_above() {
    let mut s = setup(6.0);
    s.ramp_deg = 3.0;
    let t = toolpath(&[rect(40.0, 20.0)], Operation::Pocket { stepover: 0.5 }, &s).unwrap();
    let mut z_prev = s.safe;
    let mut lowest_plunge = 0.0f64;
    for m in &t.moves {
        match m {
            Move::Line { to, plunge: true } => lowest_plunge = lowest_plunge.min(to[2]),
            Move::Line { to, plunge: false } | Move::Arc { to, .. } => {
                // Never more than the ramp's slope over a move's length (moves here are
                // at most the ring's longest side, 34 mm).
                assert!(z_prev - to[2] <= 34.0 * libm::tan(3f64.to_radians()) + 1e-9 || z_prev == s.safe);
                z_prev = to[2];
            }
            Move::Rapid(to) => z_prev = to[2],
        }
    }
    // Plunges only go down to the level above (the top for the first level).
    assert!(lowest_plunge >= -3.34, "{lowest_plunge}");
    assert!(t.gcode(&s, "ramp").contains(" Z-"));
}
