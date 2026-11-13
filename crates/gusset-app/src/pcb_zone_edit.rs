//! The zone outline editor of the board view (ADR 0053): pick a zone, drag its corners,
//! insert a corner at an edge's middle, delete a corner. The functions here take and give
//! board millimetres; each edit is one `pcb.zone_outline` command, so one undo step.

use gusset_pcb::Board;
use gusset_pcb::board::Pt;
use serde_json::{Value, json};

/// A corner of a zone: (outline index, corner index in it).
pub type Corner = (usize, usize);

/// The picked zone and what is picked on it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ZoneEdit {
    /// Index in the board's zones.
    pub zone: usize,
    /// The corner picked (for Delete and the panel's button).
    pub corner: Option<Corner>,
    /// A corner being dragged and where it is now (board mm).
    pub drag: Option<(Corner, Pt)>,
}

/// The outlines of zone `zone` (every polygon, the first being the main one).
pub fn outlines_of(board: &Board, zone: usize) -> Vec<Vec<Pt>> {
    board
        .zones
        .get(zone)
        .map(|z| std::iter::once(z.outline.clone()).chain(z.more_outlines.iter().cloned()).collect())
        .unwrap_or_default()
}

fn dist(a: Pt, b: Pt) -> f64 {
    libm::hypot(a[0] - b[0], a[1] - b[1])
}

fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 <= 0.0 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
    dist(p, [a[0] + t * dx, a[1] + t * dy])
}

fn inside(poly: &[Pt], p: Pt) -> bool {
    gusset_pcb::geom::inside(poly, p)
}

fn area(poly: &[Pt]) -> f64 {
    let n = poly.len();
    (0..n)
        .filter_map(|i| Some((poly.get(i)?, poly.get((i + 1) % n)?)))
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum::<f64>()
        .abs()
        / 2.0
}

/// The zone under board point `p`: one whose outline `p` is inside or within `tol` mm of
/// (the smallest first, so a zone inside another can be picked), on a layer `shown`.
pub fn zone_at(board: &Board, p: Pt, tol: f64, shown: &dyn Fn(&str) -> bool) -> Option<usize> {
    board
        .zones
        .iter()
        .enumerate()
        .filter(|(_, z)| z.keepout || board.copper_layers().iter().any(|c| board.on_layer(&z.layers, c) && shown(c)))
        .filter_map(|(i, z)| {
            let hits = z.outlines().filter(|o| {
                let n = o.len();
                inside(o, p)
                    || (0..n).any(
                        |k| matches!((o.get(k), o.get((k + 1) % n)), (Some(a), Some(b)) if seg_dist(p, *a, *b) <= tol),
                    )
            });
            hits.map(|o| area(o)).min_by(f64::total_cmp).map(|a| (i, a))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The corner within `tol` mm of `p` (the nearest).
pub fn corner_at(outlines: &[Vec<Pt>], p: Pt, tol: f64) -> Option<Corner> {
    outlines
        .iter()
        .enumerate()
        .flat_map(|(o, pts)| pts.iter().enumerate().map(move |(k, q)| ((o, k), dist(p, *q))))
        .filter(|(_, d)| *d <= tol)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(c, _)| c)
}

/// The middle of an edge, as (outline, index of the edge's first corner).
pub fn edge_mid(outlines: &[Vec<Pt>], c: Corner) -> Option<Pt> {
    let pts = outlines.get(c.0)?;
    let (a, b) = (pts.get(c.1)?, pts.get((c.1 + 1) % pts.len())?);
    Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0])
}

/// The edge whose middle handle is within `tol` mm of `p` (the nearest).
pub fn edge_at(outlines: &[Vec<Pt>], p: Pt, tol: f64) -> Option<Corner> {
    (0..outlines.len())
        .flat_map(|o| (0..outlines.get(o).map_or(0, Vec::len)).map(move |k| (o, k)))
        .filter_map(|c| edge_mid(outlines, c).map(|m| (c, dist(p, m))))
        .filter(|(_, d)| *d <= tol)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(c, _)| c)
}

/// Rounds to a micrometre, as the file keeps it.
fn round(p: Pt) -> Pt {
    [(p[0] * 1e3).round() / 1e3, (p[1] * 1e3).round() / 1e3]
}

/// The outlines with corner `c` moved to `to`; `None` for a corner that is not there.
pub fn move_corner(outlines: &[Vec<Pt>], c: Corner, to: Pt) -> Option<Vec<Vec<Pt>>> {
    let mut out = outlines.to_vec();
    *out.get_mut(c.0)?.get_mut(c.1)? = round(to);
    Some(out)
}

/// The outlines with a corner inserted at the middle of the edge after corner `c`, and
/// the new corner.
pub fn insert_corner(outlines: &[Vec<Pt>], c: Corner) -> Option<(Vec<Vec<Pt>>, Corner)> {
    let mid = round(edge_mid(outlines, c)?);
    let mut out = outlines.to_vec();
    out.get_mut(c.0)?.insert(c.1 + 1, mid);
    Some((out, (c.0, c.1 + 1)))
}

/// The outlines without corner `c`. An outline keeps at least 3 corners; one of exactly 3
/// goes whole when the zone has another, and the last cannot go.
pub fn delete_corner(outlines: &[Vec<Pt>], c: Corner) -> Option<Vec<Vec<Pt>>> {
    let n = outlines.get(c.0)?.len();
    if c.1 >= n {
        return None;
    }
    let mut out = outlines.to_vec();
    if n > 3 {
        out.get_mut(c.0)?.remove(c.1);
    } else if outlines.len() > 1 {
        out.remove(c.0);
    } else {
        return None;
    }
    Some(out)
}

/// `pcb.zone_outline` for zone `zone` with these outlines.
pub fn zone_outline_command(zone: usize, outlines: &[Vec<Pt>]) -> (&'static str, Value) {
    ("pcb.zone_outline", json!({ "zone": zone, "outlines": outlines }))
}
