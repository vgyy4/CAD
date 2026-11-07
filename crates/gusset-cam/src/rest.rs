//! Rest machining of pockets (ADR 0040, "Rest machining"): the material a larger tool left.
//!
//! The larger tool (radius `R`) cleared the pocket shrunk by `R` and grown again by `R` (a
//! morphological opening); what remains is the pocket minus that: corners sharper than `R`
//! and arms narrower than `2R`. Each remaining piece is grown by the small tool's radius `r`
//! and clipped to the pocket shrunk by `r` (where the tool's centre may go): that is where the
//! centre has to travel to touch the piece. It is cut as that area's outline plus rings inside
//! it, a stepover apart, from the middle out. The centre stays in the pocket shrunk by `r`, so
//! the tool never gouges a wall.

use super::{CamError, MAX_RINGS, Setup, plines_of};
use cavalier_contours::polyline::{BooleanOp, PlineSource, PlineSourceMut, Polyline};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};

/// Rest pieces thinner than this (mm) are not worth a pass (also swallows numerical slivers).
const MIN_WIDTH: f64 = 0.05;
/// The cleared area is grown this much (mm) before it is taken away, so that its walls do not
/// coincide with the pocket's (coincident edges make the boolean unreliable).
const EPS: f64 = 1e-3;

fn ccw(mut p: Polyline<f64>) -> Polyline<f64> {
    if p.area() < 0.0 {
        p.invert_direction_mut();
    }
    p
}

/// The rings to cut for one region (outer loop `outer`), innermost first.
pub(crate) fn plan(
    outer: &Polyline<f64>,
    stepover: f64,
    previous_diameter: f64,
    s: &Setup,
) -> Result<Vec<Polyline<f64>>, CamError> {
    let opts = ShapeOffsetOptions::default();
    let (r, rp) = (s.tool_diameter / 2.0, previous_diameter / 2.0);
    let outer = ccw(outer.clone());
    let opened = Shape::from_plines([outer.clone()]).parallel_offset(rp, &opts).parallel_offset(-(rp + EPS), &opts);
    // Pocket minus the opened area.
    let mut pieces = vec![outer.clone()];
    for a in opened.ccw_plines.iter().map(|p| &p.polyline) {
        let mut next = Vec::new();
        for piece in &pieces {
            next.extend(piece.boolean(a, BooleanOp::Not).pos_plines.into_iter().map(|b| b.pline));
        }
        pieces = next;
    }
    // Cleared area that closed round uncut stock (a hole in the opened area) is rest too.
    pieces.extend(opened.cw_plines.iter().map(|p| ccw(p.polyline.clone())));
    let core = Shape::from_plines([outer.clone()]).parallel_offset(r, &opts);
    let mut out: Vec<Polyline<f64>> = Vec::new();
    for piece in pieces {
        let piece = ccw(piece);
        if piece.vertex_count() < 2 || piece.area() < MIN_WIDTH * MIN_WIDTH {
            continue;
        }
        let shape = Shape::from_plines([piece]);
        if shape.parallel_offset(MIN_WIDTH / 2.0, &opts).ccw_plines.is_empty() {
            continue;
        }
        // Where the tool's centre has to go: within r of the piece (its rim then touches it)
        // and inside the pocket shrunk by r (its rim stays in the pocket). Cut as the outline
        // of that area and rings inside it, the stepover apart.
        for grown in shape.parallel_offset(-r, &opts).ccw_plines.iter().map(|p| &p.polyline) {
            for core in core.ccw_plines.iter().map(|p| &p.polyline) {
                for q in grown.boolean(core, BooleanOp::And).pos_plines {
                    let mut levels: Vec<Vec<Polyline<f64>>> = Vec::new();
                    let mut cur = Shape::from_plines([ccw(q.pline)]);
                    loop {
                        let ring = plines_of(&cur);
                        if ring.is_empty() {
                            break;
                        }
                        levels.push(ring);
                        if out.len() + levels.len() > MAX_RINGS {
                            return Err(CamError::TooLarge(
                                "more than 10000 pocket rings: use a larger stepover".into(),
                            ));
                        }
                        cur = cur.parallel_offset(stepover * s.tool_diameter, &opts);
                    }
                    // From the middle out: the last rings are the outline, cut last.
                    out.extend(levels.into_iter().rev().flatten());
                }
            }
        }
    }
    Ok(out)
}
