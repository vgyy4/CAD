#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
//! Run 10: pocket rings are not re-ramped through cleared material (F10-3), the G-code header (F10-9).

use super::*;
use gusset_kernel::ProfileEdge;

fn square(w: f64) -> Region2 {
    let p = [[0.0, 0.0], [w, 0.0], [w, w], [0.0, w]];
    let edges = (0..4).map(|i| ProfileEdge { seg: Seg2::Line { a: p[i], b: p[(i + 1) % 4] }, tag: 0 }).collect();
    Region2 { outer: Loop2 { edges }, holes: Vec::new() }
}

fn setup() -> Setup {
    Setup {
        tool_diameter: 8.0,
        top: 10.0,
        depth: 6.0,
        stepdown: 3.0,
        safe: 15.0,
        feed: 800.0,
        plunge: 200.0,
        rpm: 12000.0,
        ramp_deg: 3.0,
        tabs: None,
        climb: true,
    }
}

/// Length (mm) of the cutting moves that change height.
fn ramp_length(t: &Toolpath) -> f64 {
    let mut at: Option<[f64; 3]> = None;
    let mut sum = 0.0;
    for m in &t.moves {
        match m {
            Move::Rapid(p) => at = Some(*p),
            Move::Line { to, plunge: false } => {
                if let Some(a) = at.filter(|a| (a[2] - to[2]).abs() > 1e-9) {
                    sum += libm::sqrt((a[0] - to[0]).powi(2) + (a[1] - to[1]).powi(2) + (a[2] - to[2]).powi(2));
                }
                at = Some(*to);
            }
            Move::Line { to, .. } | Move::Arc { to, .. } => at = Some(*to),
        }
    }
    sum
}

#[test]
fn pocket_rings_after_the_first_are_entered_at_the_depth() {
    let t = toolpath(&[square(48.0)], Operation::Pocket { stepover: 0.4 }, &setup()).unwrap();
    let st = t.stats(&setup());
    let ramp = ramp_length(&t);
    // Only the innermost ring of each of the two levels ramps (it used to be every ring: more than half).
    assert!(ramp < 0.25 * (st.cut + st.plunge), "ramp {ramp} of {}", st.cut + st.plunge);
    // The pocket is still cleared at both depths down to the wall rings.
    for z in [7.0, 4.0] {
        let e = extent(&t, z);
        for (got, want) in e.iter().zip([4.0, 4.0, 44.0, 44.0]) {
            assert!((got - want).abs() < 1e-6, "{z}: {e:?}");
        }
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
fn gcode_header_does_not_claim_2_5d() {
    let t = toolpath(&[square(48.0)], Operation::Pocket { stepover: 0.4 }, &setup()).unwrap();
    let g = t.gcode(&setup(), "job");
    assert!(g.contains("(Gusset toolpath:") && !g.contains("2.5D"), "{g}");
}
