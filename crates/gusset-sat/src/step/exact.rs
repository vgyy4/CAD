//! Intersection curves recomputed on their exact surfaces.
//!
//! An ACIS `int_int_cur` is stored as a cubic fit of the intersection of two surfaces, within a
//! fit tolerance (3350's hexagon nut: 0.68 um, a mean of 0.09 um off the revolved spline its
//! edges lie on). OCCT puts each face's boundary on the projection of that curve onto the face's
//! own surface, so the two faces of an edge leave a gap the width of the fit's error along it:
//! the nut's corners, a plane cut by a revolved spline on each side, came to 0.031 mm3 (1e-4 of
//! the body) less than the same solid made by a boolean. Where both surfaces are known exactly
//! (a plane, or a revolved exact B-spline), the curve is rebuilt from points on the true
//! intersection: Hermite pieces, sixteen to each span of the stored fit, through the points Newton
//! steps from the fit's own points reach, with the tangent along the two surfaces' normals'
//! cross product. A piece on a plane stays on it (its control points are on it).

use super::{Bs3, V3, axpy, bs3_point, cross, dot, len, sub, unit};

/// A surface whose nearest points can be found exactly.
pub(super) enum Exact {
    Plane {
        origin: V3,
        normal: V3,
    },
    /// A curve revolved about an axis: the axis's point and unit direction, the curve, and
    /// samples of it (parameter, distance from the axis, height along it) to start searches.
    Revolution {
        origin: V3,
        axis: V3,
        profile: Bs3,
        samples: Vec<[f64; 3]>,
    },
}

/// Pieces of the rebuilt curve to one span of the stored fit.
const PIECES_PER_SPAN: usize = 16;
/// Most points a rebuilt curve is made of.
const MAX_POINTS: usize = 4096;
/// Samples of a revolved curve to start the nearest-point search from.
const PROFILE_SAMPLES: usize = 400;
/// How far (relative to the curve's size) a point on the true intersection may lie from the
/// stored fit: more is a misread surface, and the fit is kept.
const MAX_SHIFT: f64 = 1e-3;

impl Exact {
    pub(super) fn plane(origin: V3, normal: V3) -> Option<Exact> {
        Some(Exact::Plane { origin, normal: unit(normal)? })
    }

    pub(super) fn revolution(origin: V3, axis: V3, profile: Bs3) -> Option<Exact> {
        let axis = unit(axis)?;
        let (t0, t1) = (*profile.knots.first()?, *profile.knots.last()?);
        if t1.partial_cmp(&t0) != Some(std::cmp::Ordering::Greater) {
            return None;
        }
        let mut samples = Vec::with_capacity(PROFILE_SAMPLES + 1);
        for i in 0..=PROFILE_SAMPLES {
            let t = t0 + (t1 - t0) * i as f64 / PROFILE_SAMPLES as f64;
            let (rho, h) = rho_h(origin, axis, bs3_point(&profile, t)?);
            samples.push([t, rho, h]);
        }
        Some(Exact::Revolution { origin, axis, profile, samples })
    }

    /// The point of the surface nearest `p` (near it: the search starts from the nearest
    /// sample) and the unit normal there.
    fn foot(&self, p: V3) -> Option<(V3, V3)> {
        match self {
            Exact::Plane { origin, normal } => Some((axpy(p, -dot(sub(p, *origin), *normal), *normal), *normal)),
            Exact::Revolution { origin, axis, profile, samples } => {
                let h = dot(sub(p, *origin), *axis);
                let radial = axpy(sub(p, *origin), -h, *axis);
                let rho = len(radial);
                let e = unit(radial)?;
                let (t0, t1) = (*profile.knots.first()?, *profile.knots.last()?);
                let at = |t: f64| -> Option<(f64, f64)> { Some(rho_h(*origin, *axis, bs3_point(profile, t)?)) };
                let step = 1e-6 * (t1 - t0);
                // The profile's tangent in (rho, h) at t, by differences inside its range.
                let tangent = |t: f64| -> Option<(f64, f64)> {
                    let (a, b) = ((t - step).max(t0), (t + step).min(t1));
                    let ((r0, h0), (r1, h1)) = (at(a)?, at(b)?);
                    Some(((r1 - r0) / (b - a), (h1 - h0) / (b - a)))
                };
                // Where the distance is least: (c(t) - q) . c'(t) = 0, by secant steps.
                let g = |t: f64| -> Option<f64> {
                    let ((rc, hc), (dr, dh)) = (at(t)?, tangent(t)?);
                    Some((rc - rho) * dr + (hc - h) * dh)
                };
                let start = samples
                    .iter()
                    .min_by(|a, b| {
                        let d = |s: &[f64; 3]| (s[1] - rho).powi(2) + (s[2] - h).powi(2);
                        d(a).total_cmp(&d(b))
                    })?
                    .first()
                    .copied()?;
                let mut t = start;
                for _ in 0..40 {
                    let gt = g(t)?;
                    let dg =
                        (g((t + step).min(t1))? - g((t - step).max(t0))?) / ((t + step).min(t1) - (t - step).max(t0));
                    if dg.abs() < 1e-300 || !dg.is_finite() {
                        break;
                    }
                    let next = (t - gt / dg).clamp(t0, t1);
                    let done = (next - t).abs() <= 1e-15 * (t1 - t0);
                    t = next;
                    if done {
                        break;
                    }
                }
                let ((rc, hc), (dr, dh)) = (at(t)?, tangent(t)?);
                let foot = axpy(axpy(*origin, hc, *axis), rc, e);
                // The normal across the profile's tangent, in the plane of the axis and p.
                let normal = unit(axpy([e[0] * dh, e[1] * dh, e[2] * dh], -dr, *axis))?;
                Some((foot, normal))
            }
        }
    }
}

/// A point's distance from an axis and height along it.
fn rho_h(origin: V3, axis: V3, p: V3) -> (f64, f64) {
    let h = dot(sub(p, origin), axis);
    (len(axpy(sub(p, origin), -h, axis)), h)
}

/// The point of the intersection of two surfaces nearest `p`: Newton steps onto the line where
/// their tangent planes at the nearest points meet. `None` where the steps do not settle within
/// `1e-13` of the size `scale` (tangent surfaces, a point far from either).
fn on_both(a: &Exact, b: &Exact, p: V3, scale: f64) -> Option<(V3, V3)> {
    let mut x = p;
    for _ in 0..40 {
        let ((sa, na), (sb, nb)) = (a.foot(x)?, b.foot(x)?);
        let (aa, ab, bb) = (dot(na, na), dot(na, nb), dot(nb, nb));
        let det = aa * bb - ab * ab;
        if det < 1e-12 {
            return None;
        }
        let (ra, rb) = (dot(na, sub(sa, x)), dot(nb, sub(sb, x)));
        let la = (ra * bb - rb * ab) / det;
        let lb = (rb * aa - ra * ab) / det;
        let next = axpy(axpy(x, la, na), lb, nb);
        let moved = len(sub(next, x));
        x = next;
        if moved <= 1e-14 * scale {
            let ((sa, na), (sb, nb)) = (a.foot(x)?, b.foot(x)?);
            if len(sub(sa, x)) > 1e-13 * scale || len(sub(sb, x)) > 1e-13 * scale {
                return None;
            }
            return Some((x, unit(cross(na, nb))?));
        }
    }
    None
}

/// The stored fit `fit` of the intersection of `a` and `b`, rebuilt on it (see the module). Its
/// parameters are the fit's: an edge's range on it still holds. `None` where any point fails.
pub(super) fn refit(fit: &Bs3, a: &Exact, b: &Exact) -> Option<Bs3> {
    let span = fit.points.iter().flatten().fold(0.0f64, |m, x| m.max(x.abs())).max(1e-9);
    let size = fit.points.iter().fold(0.0f64, |m, p| fit.points.first().map_or(m, |q| m.max(len(sub(*p, *q)))));
    let mut breaks = Vec::new();
    for w in fit.knots.windows(2) {
        let [k0, k1] = *w else { return None };
        for j in 0..PIECES_PER_SPAN {
            breaks.push(k0 + (k1 - k0) * j as f64 / PIECES_PER_SPAN as f64);
        }
    }
    breaks.push(*fit.knots.last()?);
    if breaks.len() > MAX_POINTS || breaks.len() < 2 {
        return None;
    }
    let (t0, t1) = (*fit.knots.first()?, *fit.knots.last()?);
    let step = 1e-7 * (t1 - t0);
    let mut nodes = Vec::with_capacity(breaks.len());
    for t in &breaks {
        let p = bs3_point(fit, *t)?;
        let (lo, hi) = ((t - step).max(t0), (t + step).min(t1));
        let d = sub(bs3_point(fit, hi)?, bs3_point(fit, lo)?);
        let d = [d[0] / (hi - lo), d[1] / (hi - lo), d[2] / (hi - lo)];
        let (x, along) = on_both(a, b, p, span)?;
        if len(sub(x, p)) > MAX_SHIFT * size.max(1e-9) {
            return None;
        }
        // The tangent along the intersection, as fast as the fit runs there.
        let speed = dot(d, along);
        if speed.abs() < 1e-9 * len(d) || len(d) == 0.0 {
            return None;
        }
        nodes.push((*t, x, [along[0] * speed, along[1] * speed, along[2] * speed]));
    }
    let (_, first, _) = *nodes.first()?;
    let mut points = vec![first];
    let mut knots = vec![t0];
    let mut mults = vec![4];
    let last = nodes.len() - 1;
    for (j, w) in nodes.windows(2).enumerate() {
        let [(ta, pa, da), (tb, pb, db)] = *w else { return None };
        let h = (tb - ta) / 3.0;
        points.push(axpy(pa, h, da));
        points.push(axpy(pb, -h, db));
        points.push(pb);
        knots.push(tb);
        mults.push(if j + 1 == last { 4 } else { 3 });
    }
    Some(Bs3 { degree: 3, knots, mults, points, weights: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quarter of a circle of radius `r` in the plane z = `z`, as a coarse cubic fit (one
    /// Bezier span, about 0.03% off the circle).
    fn coarse_quarter(r: f64, z: f64) -> Bs3 {
        let k = 0.5523;
        Bs3 {
            degree: 3,
            knots: vec![0.0, 1.0],
            mults: vec![4, 4],
            points: vec![[r, 0.0, z], [r, r * k, z], [r * k, r, z], [0.0, r, z]],
            weights: None,
        }
    }

    #[test]
    fn a_fit_of_a_plane_through_a_cone_is_rebuilt_on_both() {
        // A cone about z (a line from (2, 0, 0) to (4, 0, 2) revolved) cut by the plane z = 1:
        // the circle of radius 3.
        let profile = Bs3 {
            degree: 1,
            knots: vec![0.0, 1.0],
            mults: vec![2, 2],
            points: vec![[2.0, 0.0, 0.0], [4.0, 0.0, 2.0]],
            weights: None,
        };
        let cone = Exact::revolution([0.0; 3], [0.0, 0.0, 1.0], profile).unwrap();
        let plane = Exact::plane([0.0, 0.0, 1.0], [0.0, 0.0, 1.0]).unwrap();
        let fit = coarse_quarter(3.0, 1.0);
        let off = |b: &Bs3| {
            (0..=100)
                .map(|i| {
                    let p = bs3_point(b, f64::from(i) / 100.0).unwrap();
                    ((p[0] * p[0] + p[1] * p[1]).sqrt() - 3.0).abs().max((p[2] - 1.0).abs())
                })
                .fold(0.0f64, f64::max)
        };
        assert!(off(&fit) > 1e-4, "{}", off(&fit));
        let exact = refit(&fit, &cone, &plane).unwrap();
        // Sixteen Hermite pieces to a quarter turn: 1.3e-6 off the circle at most.
        assert!(off(&exact) < 2e-6, "{}", off(&exact));
        // The ends stay where the fit's are, on the parameters the fit gives them.
        assert!(len(sub(bs3_point(&exact, 0.0).unwrap(), [3.0, 0.0, 1.0])) < 1e-12);
        assert!(len(sub(bs3_point(&exact, 1.0).unwrap(), [0.0, 3.0, 1.0])) < 1e-12);
        // A misread surface (a plane far from the fit) leaves the fit as it is.
        let far = Exact::plane([0.0, 0.0, 1.5], [0.0, 0.0, 1.0]).unwrap();
        assert!(refit(&fit, &cone, &far).is_none());
    }
}
