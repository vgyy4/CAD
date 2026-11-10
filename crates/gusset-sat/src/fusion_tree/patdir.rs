//! The second direction of a rectangular pattern that stores one (ADR 0039 amendment, section 22
//! of the format notes). A pattern with `CountV` > 1 and a single `F2A7590D` direction record has
//! its V direction perpendicular to U; the file does not say on which side. It is taken from the
//! stored bodies: the straight profile edges of the patterned extrudes, moved to every copy's
//! place, must each be the edge of a stored body for one of the two senses and not for the other.

use super::geom::{self, V3, dot};
use super::{Ctx, EXTRUDE};

/// The lattice of one pattern: unit U direction, spacings (cm) and counts.
pub(super) struct Lattice {
    pub u: V3,
    pub spacing_u: f64,
    pub count_u: usize,
    pub spacing_v: f64,
    pub count_v: usize,
}

const TOL: f64 = 1e-6;
/// Most copies looked at (a pattern beyond this is not decided).
const MAX_COPIES: usize = 4096;

impl Ctx<'_> {
    /// The unit V direction of `lat`, for a pattern of the features `inputs`, when the stored
    /// bodies decide it.
    pub(super) fn second_direction(&mut self, inputs: &[u64], lat: &Lattice) -> Option<V3> {
        if lat.count_v < 2 || lat.count_u.checked_mul(lat.count_v)? > MAX_COPIES {
            return None;
        }
        // The straight profile edges of the patterned extrudes (their sketches share a normal).
        let mut normal: Option<V3> = None;
        let mut segs: Vec<(V3, V3)> = Vec::new();
        for &f in inputs {
            if !self.d.is(f, EXTRUDE) {
                continue;
            }
            let g = self.ext_geom(f)?;
            let z = g.pl.z;
            if normal.is_some_and(|n| (dot(n, z).abs() - 1.0).abs() > 1e-9) {
                return None;
            }
            normal = Some(z);
            for sg in &g.segs {
                if !g.sketch.lines.contains_key(&sg.curve) {
                    continue;
                }
                let (Some(a), Some(b)) = (sg.poly.first(), sg.poly.last()) else { continue };
                let (pa, pb) = (geom::world(&g.pl, *a, 0.0), geom::world(&g.pl, *b, 0.0));
                if geom::norm(geom::sub(pb, pa)) > 1e-4 {
                    segs.push((pa, pb));
                }
            }
        }
        let stored: Vec<(V3, V3)> = self.edge_sets().iter().flatten().copied().collect();
        side_of(&segs, normal?, lat, &stored)
    }
}

/// The unit V direction of `lat` in the plane of normal `n`: `n x U` or its opposite, whichever
/// puts every copy of the straight edges `segs` (one of them per copy at least) on an edge of
/// `stored`; `None` when both senses or neither do. Edges are compared flattened onto the plane,
/// so a stored edge anywhere along the normal counts.
pub(super) fn side_of(segs: &[(V3, V3)], n: V3, lat: &Lattice, stored: &[(V3, V3)]) -> Option<V3> {
    if lat.count_v < 2 || segs.is_empty() || dot(n, lat.u).abs() > 1e-9 {
        return None;
    }
    let v = geom::unit(geom::cross(n, lat.u))?;
    let flat = |p: V3| geom::sub(p, geom::mul(n, dot(p, n)));
    let stored: Vec<(V3, V3)> = stored.iter().map(|(a, b)| (flat(*a), flat(*b))).collect();
    let near = |a: V3, b: V3| geom::norm(geom::sub(a, b)) < TOL;
    let supported = |sense: f64| -> bool {
        (0..lat.count_u).all(|i| {
            (1..lat.count_v).all(|j| {
                let off = geom::add(
                    geom::mul(lat.u, lat.spacing_u * i as f64),
                    geom::mul(v, sense * lat.spacing_v * j as f64),
                );
                segs.iter().any(|(a, b)| {
                    let (a, b) = (flat(geom::add(*a, off)), flat(geom::add(*b, off)));
                    stored.iter().any(|(p, q)| (near(*p, a) && near(*q, b)) || (near(*p, b) && near(*q, a)))
                })
            })
        })
    };
    match (supported(1.0), supported(-1.0)) {
        (true, false) => Some(v),
        (false, true) => Some(geom::mul(v, -1.0)),
        _ => None,
    }
}
