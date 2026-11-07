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
        tabs: None,
        climb: true,
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

#[test]
fn holes_are_drilled_nearest_first_with_canned_cycles() {
    let pts = [[30.0, 0.0], [10.0, 0.0], [20.0, 0.0]];
    let t = drilling(&pts, Some(2.0), &setup(5.0)).unwrap();
    let d = t.drill.as_ref().unwrap();
    assert_eq!(d.points, vec![[10.0, 0.0], [20.0, 0.0], [30.0, 0.0]]);
    let g = t.gcode(&setup(5.0), "holes");
    assert!(g.contains("G98 G83 X10 Y0 Z-5 R1 Q2 F200") && g.contains("\nX20 Y0\n") && g.contains("G80\n"), "{g}");
    // Pecks of 2, 2 and 1 per hole, each from the top: 2 + 4 + 5 mm of plunging per hole.
    let st = t.stats(&setup(5.0));
    assert!(st.plunge > 3.0 * 5.0 - 1e-9, "{st:?}");
    let g = drilling(&pts, None, &setup(5.0)).unwrap().gcode(&setup(5.0), "holes");
    assert!(g.contains("G98 G81 X10 Y0 Z-5 R1 F200"), "{g}");
    assert!(drilling(&[], None, &setup(5.0)).is_err());
    assert!(drilling(&pts, Some(0.0), &setup(5.0)).is_err());
}

#[test]
fn tabs_lift_the_last_profile_passes_over_bridges() {
    let mut s = setup(6.0);
    s.tabs = Some(Tabs { count: 4, width: 3.0, height: 1.5 });
    let t = toolpath(&[rect(40.0, 20.0)], Operation::Profile { outside: true }, &s).unwrap();
    // The last level (-5) runs at -5 except over the tabs, at -3.5.
    let zs: Vec<f64> = t
        .moves
        .iter()
        .filter_map(|m| match m {
            Move::Line { to, plunge: false } | Move::Arc { to, .. } => Some(to[2]),
            _ => None,
        })
        .collect();
    assert!(zs.iter().any(|z| (z + 3.5).abs() < 1e-9) && zs.iter().any(|z| (z + 5.0).abs() < 1e-9), "{zs:?}");
    let g = t.gcode(&s, "tabs");
    assert!(g.contains("Z-3.5"), "{g}");
    s.tabs = Some(Tabs { count: 40, width: 3.0, height: 1.5 });
    assert!(toolpath(&[rect(40.0, 20.0)], Operation::Profile { outside: true }, &s).is_err());
}

#[test]
fn review_fixes_hold() {
    // A ramp on a tabbed profile never cuts below the tabs inside a tab.
    let mut s = setup(6.0);
    s.ramp_deg = 3.0;
    s.tabs = Some(Tabs { count: 4, width: 3.0, height: 1.5 });
    let t = toolpath(&[rect(100.0, 60.0)], Operation::Profile { outside: true }, &s).unwrap();
    check_tabs(&t, &s, &rect(100.0, 60.0));
    // Tabs: at least one, lower than the depth, and only on profiles; the spindle must turn.
    for (count, height) in [(0, 1.0), (4, 5.0)] {
        s.tabs = Some(Tabs { count, width: 3.0, height });
        assert!(toolpath(&[rect(100.0, 60.0)], Operation::Profile { outside: true }, &s).is_err());
    }
    s.tabs = Some(Tabs { count: 4, width: 3.0, height: 1.0 });
    assert!(toolpath(&[rect(100.0, 60.0)], Operation::Pocket { stepover: 0.5 }, &s).is_err());
    assert!(drilling(&[[0.0, 0.0]], None, &s).is_err());
    let mut s = setup(6.0);
    s.rpm = 0.0;
    assert!(toolpath(&[rect(40.0, 20.0)], Operation::Pocket { stepover: 0.5 }, &s).is_err());
    s.rpm = 1000.0;
    s.feed = f64::MAX;
    assert!(toolpath(&[rect(40.0, 20.0)], Operation::Pocket { stepover: 0.5 }, &s).is_err());
    // Steps wider than a radius would leave ridges: refused; at 0.5 the middle is cut.
    assert!(toolpath(&[rect(40.0, 14.0)], Operation::Pocket { stepover: 0.6 }, &setup(6.0)).is_err());
    let t = toolpath(&[rect(40.0, 14.0)], Operation::Pocket { stepover: 0.5 }, &setup(6.0)).unwrap();
    let mid = t.moves.iter().any(|m| match m {
        Move::Line { to, plunge: false } | Move::Arc { to, .. } => (to[1] - 7.0).abs() <= 1.0 + 1e-9,
        _ => false,
    });
    assert!(mid, "the middle strip is left uncut");
    // An arc that rounds to nothing is written as a straight cut, never `I0 J0`.
    let t = Toolpath {
        moves: vec![
            Move::Rapid([0.0, 0.0, 5.0]),
            Move::Line { to: [0.0, 0.0, -1.0], plunge: true },
            Move::Arc { to: [0.00001, 0.00001, -1.0], center: [0.00002, 0.0], ccw: true },
        ],
        ..Toolpath::default()
    };
    let g = t.gcode(&setup(6.0), "tiny");
    assert!(!g.contains("I0 J0") && g.contains("G40 G49 G80 G54"), "{g}");
    // Curves are sampled to a tolerance: a large one gets more chords than a small one.
    let big = [[0.0, 0.0], [0.0, 55.0], [45.0, 100.0], [100.0, 100.0]];
    let small = [[0.0, 0.0], [0.0, 0.5], [0.5, 1.0], [1.0, 1.0]];
    assert!(bezier_steps(&big) > 100 && bezier_steps(&small) <= 16, "{} {}", bezier_steps(&big), bezier_steps(&small));
}

#[test]
fn climb_milling_runs_clockwise_round_a_part_and_conventional_the_other_way() {
    // The signed area of the cut's path at the last level: negative when clockwise.
    let turn = |t: &Toolpath| {
        let z = *t.levels.last().unwrap();
        let pts: Vec<[f64; 2]> = t
            .moves
            .iter()
            .filter_map(|m| match m {
                Move::Line { to, plunge: false } | Move::Arc { to, .. } if (to[2] - z).abs() < 1e-9 => {
                    Some([to[0], to[1]])
                }
                _ => None,
            })
            .collect();
        (0..pts.len())
            .map(|i| pts[i][0] * pts[(i + 1) % pts.len()][1] - pts[(i + 1) % pts.len()][0] * pts[i][1])
            .sum::<f64>()
    };
    let mut s = setup(6.0);
    let climb = toolpath(&[rect(40.0, 20.0)], Operation::Profile { outside: true }, &s).unwrap();
    assert!(turn(&climb) < 0.0, "round the part clockwise");
    assert!(climb.gcode(&s, "c").contains("G2 "));
    s.climb = false;
    let conv = toolpath(&[rect(40.0, 20.0)], Operation::Profile { outside: true }, &s).unwrap();
    assert!(turn(&conv) > 0.0);
    // A pocket's wall: material outside, so climb runs counter-clockwise.
    s.climb = true;
    let pocket = toolpath(&[rect(40.0, 20.0)], Operation::Profile { outside: false }, &s).unwrap();
    assert!(turn(&pocket) > 0.0);
}

type BoreArc = ([f64; 3], [f64; 3], [f64; 2], bool);

fn bore_arcs(t: &Toolpath) -> Vec<BoreArc> {
    let mut at = None;
    let mut out = Vec::new();
    for m in &t.moves {
        match *m {
            Move::Rapid(p) | Move::Line { to: p, .. } => at = Some(p),
            Move::Arc { to, center, ccw } => {
                out.push((at.unwrap(), to, center, ccw));
                at = Some(to);
            }
        }
    }
    out
}

#[test]
fn a_helical_bore_circles_at_the_radius_and_sinks_by_the_stepdown() {
    let s = setup(6.0);
    let t = helical_bore(&[([10.0, 20.0], 10.0)], &s).unwrap();
    let arcs = bore_arcs(&t);
    assert!(!arcs.is_empty());
    let mut lowest = f64::INFINITY;
    for (a, b, c, ccw) in &arcs {
        assert_eq!(*c, [10.0, 20.0]);
        assert!(*ccw);
        for p in [a, b] {
            assert!((libm::hypot(p[0] - c[0], p[1] - c[1]) - 2.0).abs() < 1e-9);
            lowest = lowest.min(p[2]);
        }
        // Half a turn: at most half the stepdown.
        assert!(a[2] - b[2] <= s.stepdown / 2.0 + 1e-9 && a[2] >= b[2]);
        assert!(libm::hypot(a[0] - b[0], a[1] - b[1]) > 3.9);
    }
    assert!((lowest + 5.0).abs() < 1e-9);
    assert_eq!(t.rings, 1);
    assert_eq!(t.levels, vec![-5.0]);
    let g = t.gcode(&s, "bore");
    assert!(g.contains("G3 ") && !g.contains("G2 "));
    assert!(!g.contains("I0 J0"));
    let s2 = Setup { climb: false, ..s };
    let g2 = helical_bore(&[([10.0, 20.0], 10.0)], &s2).unwrap().gcode(&s2, "bore");
    assert!(g2.contains("G2 ") && !g2.contains("G3 "));
}

#[test]
fn a_helical_bore_refuses_holes_the_tool_cannot_make() {
    let s = setup(6.0);
    assert!(matches!(helical_bore(&[([0.0, 0.0], 5.0)], &s), Err(CamError::NothingToCut(_))));
    assert!(matches!(helical_bore(&[([0.0, 0.0], 20.0)], &s), Err(CamError::Invalid(_))));
    let drilled = helical_bore(&[([0.0, 0.0], 6.0)], &s).unwrap();
    assert!(bore_arcs(&drilled).is_empty());
}

#[test]
fn a_helical_bore_cuts_the_helix_length_plus_the_floor_circle() {
    let s = setup(6.0);
    let t = helical_bore(&[([0.0, 0.0], 10.0)], &s).unwrap();
    let r = 2.0;
    let turns = 3.0; // 5 mm in steps of at most 2
    let drop: f64 = 5.0 / turns;
    let want = turns * libm::sqrt((std::f64::consts::TAU * r).powi(2) + drop * drop) + std::f64::consts::TAU * r;
    let got = t.stats(&s).cut;
    assert!(((got - want) / want).abs() < 0.01, "{got} vs {want}");
}

/// Whether `pt` lies on the move `m` that starts at `from` (seen from above).
fn on_move(from: [f64; 3], m: &Move, pt: [f64; 2]) -> bool {
    let tol = 1e-5;
    match *m {
        Move::Rapid(_) => false,
        Move::Line { to, .. } => {
            let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
            let l2 = dx * dx + dy * dy;
            let u =
                if l2 < 1e-18 { 0.0 } else { (((pt[0] - from[0]) * dx + (pt[1] - from[1]) * dy) / l2).clamp(0.0, 1.0) };
            libm::hypot(from[0] + u * dx - pt[0], from[1] + u * dy - pt[1]) < tol
        }
        Move::Arc { to, center, ccw } => {
            let r = libm::hypot(from[0] - center[0], from[1] - center[1]);
            if (libm::hypot(pt[0] - center[0], pt[1] - center[1]) - r).abs() > tol {
                return false;
            }
            let ang = |p: [f64; 2]| libm::atan2(p[1] - center[1], p[0] - center[0]);
            let dir = if ccw { 1.0 } else { -1.0 };
            let rel = |a: f64| (dir * (a - ang([from[0], from[1]]))).rem_euclid(std::f64::consts::TAU);
            rel(ang(pt)) <= rel(ang([to[0], to[1]])) + tol / r.max(1e-9)
        }
    }
}

fn end_z(m: &Move) -> f64 {
    match m {
        Move::Rapid(p) => p[2],
        Move::Line { to, .. } | Move::Arc { to, .. } => to[2],
    }
}

/// Moves with the point each starts from.
fn with_starts(t: &Toolpath) -> Vec<([f64; 3], Move)> {
    let mut at = [0.0; 3];
    t.moves
        .iter()
        .map(|m| {
            let from = at;
            at = match *m {
                Move::Rapid(p) | Move::Line { to: p, .. } | Move::Arc { to: p, .. } => p,
            };
            (from, *m)
        })
        .collect()
}

fn samples(from: f64, to: f64, step: f64) -> Vec<f64> {
    let n = ((to - from) / step) as usize;
    (0..=n).map(|i| from + step * i as f64).collect()
}

/// The tabs' guarantees on the only ring of an outside profile of `region`: nothing below the
/// tabs' top cuts inside a tab span, and every point of the ring outside the spans is cut at
/// each tabbed level's depth.
fn check_tabs(t: &Toolpath, s: &Setup, region: &Region2) {
    let tabs = s.tabs.unwrap();
    let ring = rings(std::slice::from_ref(region), Operation::Profile { outside: true }, s).unwrap();
    let w = Walk::new(&ring[0]);
    let spans = tab_spans(w.total, &tabs, s.tool_diameter);
    let tab_top = s.top - s.depth + tabs.height;
    let moves = with_starts(t);
    for (a, b) in &spans {
        for sm in samples(a + 0.01, b - 0.01, 0.2) {
            let pt = w.xy(sm);
            for (from, m) in &moves {
                if matches!(m, Move::Rapid(_)) || from[2].min(end_z(m)) >= tab_top - 1e-9 {
                    continue;
                }
                assert!(!on_move(*from, m, pt), "cuts below the tab at {pt:?}: {m:?} from {from:?}");
            }
        }
    }
    for z in t.levels.iter().filter(|z| **z < tab_top - 1e-9) {
        for sm in samples(0.0, w.total, 0.37) {
            if spans.iter().any(|(a, b)| sm > a - 1e-6 && sm < b + 1e-6) {
                continue;
            }
            let pt = w.xy(sm);
            let hit = moves.iter().any(|(from, m)| {
                matches!(m, Move::Line { plunge: false, .. } | Move::Arc { .. })
                    && (from[2] - z).abs() < 1e-9
                    && (end_z(m) - z).abs() < 1e-9
                    && on_move(*from, m, pt)
            });
            assert!(hit, "the ring at s = {sm} {pt:?} is never cut at {z}");
        }
    }
}

#[test]
fn tabbed_levels_ramp_down_along_the_ring_instead_of_plunging() {
    let region = rect(100.0, 60.0);
    let tab_top = -3.5;
    for (stepdown, ramp) in [(2.0, 3.0), (1.0, 3.0), (2.0, 10.0), (2.0, 0.0)] {
        let mut s = setup(6.0);
        s.stepdown = stepdown;
        s.ramp_deg = ramp;
        s.tabs = Some(Tabs { count: 4, width: 3.0, height: 1.5 });
        let t = toolpath(std::slice::from_ref(&region), Operation::Profile { outside: true }, &s).unwrap();
        check_tabs(&t, &s, &region);
        let moves = with_starts(&t);
        // A straight plunge that ends below the tabs' top, in a level whose ring is already open
        // at the level above (the vertical drop after a tab or at the start).
        let drops = moves
            .iter()
            .filter(
                |(f, m)| matches!(m, Move::Line { to, plunge: true } if to[2] < tab_top - 1e-9 && f[2] < s.safe - 1e-9),
            )
            .count();
        if ramp > 0.0 {
            assert_eq!(drops, 0, "stepdown {stepdown} ramp {ramp}: a vertical plunge below the tabs");
            // The descent is a ramp: cutting moves that go down below the tabs' top.
            assert!(
                moves.iter().any(
                    |(f, m)| matches!(m, Move::Line { to, plunge: false } if to[2] < f[2] - 1e-6 && to[2] < tab_top)
                )
            );
        } else {
            // No ramp: coming down after a tab is the vertical plunge.
            assert!(drops >= 4, "{drops}");
        }
    }
}

#[test]
fn drilling_retracts_to_a_plane_just_above_the_top() {
    let pts = [[10.0, 0.0]];
    let mut s = setup(5.0);
    let t = drilling(&pts, Some(2.0), &s).unwrap();
    assert!(t.gcode(&s, "h").contains("R1 Q2"));
    // Pecks return to R, not to the top.
    let rapids: Vec<f64> =
        t.moves.iter().filter_map(|m| if let Move::Rapid(p) = m { Some(p[2]) } else { None }).collect();
    assert!(
        rapids.iter().filter(|z| (**z - 1.0).abs() < 1e-12).count() == 3 && !rapids.iter().any(|z| z.abs() < 1e-12),
        "{rapids:?}"
    );
    // Still below the safe height when that is close to the top.
    s.safe = 1.0;
    let t = drilling(&pts, None, &s).unwrap();
    assert!(t.gcode(&s, "h").contains("R0.5 F"));
    s.top = 3.0;
    s.safe = 10.0;
    assert!(drilling(&pts, None, &s).unwrap().gcode(&s, "h").contains("Z-2 R4 F"));
}

#[test]
fn gcode_never_repeats_a_line_or_a_position_and_names_the_tool() {
    let mut s = setup(6.0);
    s.tabs = Some(Tabs { count: 4, width: 3.0, height: 1.5 });
    s.ramp_deg = 3.0;
    let t = toolpath(&[rect(100.0, 60.0)], Operation::Profile { outside: true }, &s).unwrap();
    let pocket = toolpath(&[rect(40.0, 20.0)], Operation::Pocket { stepover: 0.5 }, &setup(6.0)).unwrap();
    for (t, s) in [(t, s), (pocket, setup(6.0))] {
        let g = t.gcode(&s, "tidy");
        let lines: Vec<&str> = g.lines().collect();
        let mut xy = String::new();
        for w in lines.windows(2) {
            assert_ne!(w[0], w[1], "a repeated line");
        }
        for l in &lines {
            let pos: Vec<&str> = l.split(' ').filter(|w| w.starts_with('X') || w.starts_with('Y')).collect();
            if l.starts_with("G0 X") {
                assert_ne!(pos.join(" "), xy, "a rapid to where the tool is: {l}");
            }
            if pos.len() == 2 {
                xy = pos.join(" ");
            }
        }
        assert_eq!(lines.get(2), Some(&"(T1 D6 flat end mill)"), "{g}");
        assert!(!g.contains("M6"));
    }
    // Rapids to where the tool already is are dropped.
    let t = Toolpath {
        moves: vec![Move::Rapid([1.0, 2.0, 5.0]), Move::Rapid([1.0, 2.0, 5.0]), Move::Rapid([1.0, 2.0, 5.0])],
        ..Toolpath::default()
    };
    let g = t.gcode(&setup(6.0), "r");
    assert_eq!(g.matches("G0 X1 Y2").count(), 1, "{g}");
    assert_eq!(g.matches("G0 Z5").count(), 2, "the header's and the end's: {g}");
}
