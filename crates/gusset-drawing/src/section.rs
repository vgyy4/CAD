//! Full section views (ADR 0032 phase 4, ISO 128-40/44/50): the bodies are cut by a
//! plane, the part between the plane and the viewer is removed, the rest is projected
//! as usual, and the cut faces are hatched with thin lines at 45°. The parent view shows
//! the cutting plane: a long-dash-dot line, thick at its ends, with arrows in the viewing
//! direction and the section letter.

use crate::scene::{Anchor, Prim, Scene, Style};
use gusset_kernel::{
    BoolOp, ExtrudeSpec, Frame, Kernel, Loop2, Profile, ProfileEdge, Region2, Seg2, Shape, TessellationParams, V3,
};

/// Hatching stops after this many line pieces (callers report it).
const HATCH_LIMIT: usize = 400_000;

/// A line segment on the sheet.
pub type Segment = ([f64; 2], [f64; 2]);

/// Hatch line spacing on the sheet (mm).
pub const HATCH_SPACING: f64 = 2.5;
/// Letter height of the section label and the cutting-plane letters (mm).
pub const LABEL_H: f64 = 5.0;

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The extent of `shapes` along `n`, from their bounding boxes.
pub fn extent(k: &dyn Kernel, shapes: &[Shape], n: V3) -> Result<(f64, f64, V3, f64), String> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut bmin = [f64::INFINITY; 3];
    let mut bmax = [f64::NEG_INFINITY; 3];
    for s in shapes {
        let b = k.bbox(s).map_err(|e| e.message)?;
        for i in 0..8 {
            let c = [
                if i & 1 == 0 { b.min[0] } else { b.max[0] },
                if i & 2 == 0 { b.min[1] } else { b.max[1] },
                if i & 4 == 0 { b.min[2] } else { b.max[2] },
            ];
            lo = lo.min(dot(c, n));
            hi = hi.max(dot(c, n));
        }
        for (m, v) in bmin.iter_mut().zip(b.min) {
            *m = m.min(v);
        }
        for (m, v) in bmax.iter_mut().zip(b.max) {
            *m = m.max(v);
        }
    }
    if !lo.is_finite() {
        return Err("there is nothing to cut".into());
    }
    let centre = [(bmin[0] + bmax[0]) / 2.0, (bmin[1] + bmax[1]) / 2.0, (bmin[2] + bmax[2]) / 2.0];
    let d = [bmax[0] - bmin[0], bmax[1] - bmin[1], bmax[2] - bmin[2]];
    let diag = libm::sqrt(dot(d, d));
    Ok((lo, hi, centre, diag))
}

/// `shapes` with everything where `dot(p, n) > c` removed. Bodies entirely on the removed
/// side are left out.
pub fn cut(k: &dyn Kernel, shapes: &[Shape], n: V3, x: V3, c: f64) -> Result<Vec<Shape>, String> {
    let (lo, hi, centre, diag) = extent(k, shapes, n)?;
    if c <= lo {
        return Err("the cutting plane leaves nothing to show".into());
    }
    if c >= hi {
        return Err("the cutting plane misses the part".into());
    }
    let r = diag + 10.0;
    // A box from the plane outwards: its profile lies in the plane (normal x × y = n).
    let along = c - dot(centre, n);
    let origin = [centre[0] + n[0] * along, centre[1] + n[1] * along, centre[2] + n[2] * along];
    let frame = Frame { origin, x, y: cross(n, x) };
    let pts = [[-r, -r], [r, -r], [r, r], [-r, r]];
    let edges = pts
        .iter()
        .zip(pts.iter().cycle().skip(1))
        .zip(0u64..)
        .map(|((a, b), tag)| ProfileEdge { seg: Seg2::Line { a: *a, b: *b }, tag })
        .collect();
    let profile = Profile { frame, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] };
    let tool = k.extrude(&profile, &ExtrudeSpec { distance: 2.0 * r, symmetric: false }).map_err(|e| e.message)?.shape;
    let mut out = Vec::new();
    for s in shapes {
        let kept = k.boolean(BoolOp::Cut, s, &tool).map_err(|e| e.message)?.shape;
        if k.topology(&kept).is_ok_and(|t| !t.faces.is_empty()) {
            out.push(kept);
        }
    }
    if out.is_empty() {
        return Err("the cutting plane leaves nothing to show".into());
    }
    Ok(out)
}

/// Triangles of `shapes` lying in the plane `dot(p, n) = c`, in view coordinates (x, y).
pub fn cut_faces(k: &dyn Kernel, shapes: &[Shape], n: V3, x: V3, y: V3, c: f64) -> Result<Vec<[[f64; 2]; 3]>, String> {
    let mut out = Vec::new();
    // The mesh stores f32 coordinates: allow their rounding, nothing more (review D9).
    let tol = 1e-4 + 2.0 * f64::from(f32::EPSILON) * c.abs();
    for s in shapes {
        let m = k.tessellate(s, &TessellationParams::default()).map_err(|e| e.message)?;
        let p = |i: u32| m.positions.get(i as usize).map(|v| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]);
        for t in m.indices.chunks_exact(3) {
            let &[i, j, l] = t else { continue };
            let (Some(a), Some(b), Some(d)) = (p(i), p(j), p(l)) else { continue };
            if [a, b, d].iter().all(|v| (dot(*v, n) - c).abs() < tol) {
                let q = |v: V3| [dot(v, x), dot(v, y)];
                out.push([q(a), q(b), q(d)]);
            }
        }
    }
    Ok(out)
}

/// Hatch segments over the union of `tris`: lines at 45° `spacing` apart, and whether
/// the hatching is complete (false when a limit cut it short).
pub fn hatch(tris: &[[[f64; 2]; 3]], spacing: f64) -> (Vec<Segment>, bool) {
    let mut complete = true;
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let d = [h, h];
    let m = [-h, h];
    let dt = |p: [f64; 2], v: [f64; 2]| p[0] * v[0] + p[1] * v[1];
    // Per hatch line index: intervals along d.
    let mut lines: std::collections::BTreeMap<i64, Vec<(f64, f64)>> = std::collections::BTreeMap::new();
    let mut count = 0usize;
    for t in tris {
        if count > HATCH_LIMIT {
            complete = false;
            break;
        }
        let s = [dt(t[0], m), dt(t[1], m), dt(t[2], m)];
        let lo = s.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = s.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if spacing.is_nan() || spacing <= 0.0 || hi - lo < 1e-12 {
            continue;
        }
        let (k0, k1) = (libm::ceil(lo / spacing) as i64, libm::floor(hi / spacing) as i64);
        if k1 - k0 > 100_000 {
            complete = false;
            continue;
        }
        for k in k0..=k1 {
            let level = k as f64 * spacing;
            let mut ts = Vec::with_capacity(2);
            for (a, b, sa, sb) in [(t[0], t[1], s[0], s[1]), (t[1], t[2], s[1], s[2]), (t[2], t[0], s[2], s[0])] {
                if (sa - level) * (sb - level) <= 0.0 && (sa - sb).abs() > 1e-15 {
                    let u = (level - sa) / (sb - sa);
                    ts.push(dt([a[0] + u * (b[0] - a[0]), a[1] + u * (b[1] - a[1])], d));
                }
            }
            if ts.len() >= 2 {
                let (a, b) = ts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &v| (l.min(v), h.max(v)));
                if b - a > 1e-9 {
                    count += 1;
                    lines.entry(k).or_default().push((a, b));
                }
            }
        }
    }
    let mut out = Vec::new();
    for (k, mut iv) in lines {
        iv.sort_by(|p, q| p.0.total_cmp(&q.0));
        let level = k as f64 * spacing;
        let at = |t: f64| [t * d[0] + level * m[0], t * d[1] + level * m[1]];
        let mut cur: Option<(f64, f64)> = None;
        for (a, b) in iv {
            cur = match cur {
                Some((ca, cb)) if a <= cb + 1e-6 => Some((ca, cb.max(b))),
                Some((ca, cb)) => {
                    out.push((at(ca), at(cb)));
                    Some((a, b))
                }
                None => Some((a, b)),
            };
        }
        if let Some((ca, cb)) = cur {
            out.push((at(ca), at(cb)));
        }
    }
    (out, complete)
}

/// The cutting-plane line across a parent view's box `[min, max]` (sheet mm): `vertical`
/// when it runs along the sheet's Y axis at `pos` (an X coordinate), else along X at `pos`
/// (a Y coordinate). `look` is the viewing direction on the sheet (unit).
pub fn cutting_plane(
    scene: &mut Scene,
    min: [f64; 2],
    max: [f64; 2],
    vertical: bool,
    pos: f64,
    look: [f64; 2],
    label: &str,
) {
    const OVER: f64 = 6.0;
    const THICK: f64 = 5.0;
    let (a, b) = if vertical {
        ([pos, min[1] - OVER], [pos, max[1] + OVER])
    } else {
        ([min[0] - OVER, pos], [max[0] + OVER, pos])
    };
    scene.line(Style::Center, a, b);
    let u = if vertical { [0.0, 1.0] } else { [1.0, 0.0] };
    for (end, s) in [(a, 1.0), (b, -1.0)] {
        let inner = [end[0] + s * u[0] * THICK, end[1] + s * u[1] * THICK];
        scene.line(Style::Visible, end, inner);
        // An arrow from the line's end, pointing the way the section is seen.
        let mid = [end[0] + s * u[0] * THICK * 0.5, end[1] + s * u[1] * THICK * 0.5];
        let tail = [mid[0] - look[0] * 8.0, mid[1] - look[1] * 8.0];
        scene.line(Style::Dimension, tail, mid);
        let w = [-look[1] * 0.8, look[0] * 0.8];
        let base = [mid[0] - look[0] * 3.0, mid[1] - look[1] * 3.0];
        scene.push(
            Style::Dimension,
            Prim::Fill { points: vec![mid, [base[0] + w[0], base[1] + w[1]], [base[0] - w[0], base[1] - w[1]]] },
        );
        let tp = [tail[0] - look[0] * 2.0 - u[0] * s * 3.0, tail[1] - look[1] * 2.0 - u[1] * s * 3.0 - LABEL_H / 2.0];
        scene.push(
            Style::Text,
            Prim::Text { at: tp, height: LABEL_H, text: label.to_string(), anchor: Anchor::Center, angle: 0.0 },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hatching_covers_the_union_once() {
        // A 10 x 10 square as two triangles.
        let tris = [[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], [[0.0, 0.0], [10.0, 10.0], [0.0, 10.0]]];
        let (segs, complete) = hatch(&tris, 1.0);
        assert!(complete);
        // Lines at 45° 1 apart cross a 10 mm square: levels -7..=7 (10/√2 ≈ 7.07).
        assert_eq!(segs.len(), 15, "{segs:?}");
        // The diagonal line (level 0) runs corner to corner, merged across both triangles.
        let total: f64 = segs.iter().map(|(a, b)| libm::hypot(b[0] - a[0], b[1] - a[1])).sum();
        // Area / spacing = 100 / 1.
        assert!((total - 100.0).abs() < 1.5, "{total}");
        for (a, b) in &segs {
            for p in [a, b] {
                assert!(p[0] > -1e-9 && p[0] < 10.0 + 1e-9 && p[1] > -1e-9 && p[1] < 10.0 + 1e-9, "{p:?}");
            }
        }
        assert!(hatch(&tris, 0.0).0.is_empty());
    }
}
