//! `Poly::PointOnTriangle`, `Poly::Intersect`, `Poly::IntersectTriLine` and `Poly::PolygonProperties`.
use super::{Triangle, Triangulation};
use crate::gp::{self, Ax1, Dir, Pnt2d, XY, XYZ};

/// `std::max(a, b)`.
fn max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// `std::min(a, b)`.
fn min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `Poly::PointOnTriangle`: the squared distance from `p` to the triangle and the parameters `(u, v)` of the closest
/// point (`P1 + u (P2 - P1) + v (P3 - P1)`).
pub fn point_on_triangle(p1: &XY, p2: &XY, p3: &XY, p: &XY) -> (f64, XY) {
    let res = gp::resolution();
    let dp = p.subtracted(p1);
    let du = p2.subtracted(p1);
    let dv = p3.subtracted(p1);
    let det = du.crossed(&dv);
    if det.abs() > res {
        let mut u = dp.crossed(&dv) / det;
        let mut v = -dp.crossed(&du) / det;
        if u > -res && v > -res && 1. - u - v > -res {
            return (0., XY::new(u, v));
        }
        u = 0.;
        v = min(1., max(0., dp.dot(&dv) / dv.square_modulus()));
        let mut ad = dv.multiplied(v).subtracted(&dp).square_modulus();
        let uu = min(1., max(0., dp.dot(&du) / du.square_modulus()));
        let mut d = du.multiplied(uu).subtracted(&dp).square_modulus();
        if d < ad {
            u = uu;
            v = 0.;
            ad = d;
        }
        let duv = dv.subtracted(&du);
        let vv = min(1., max(0., dp.subtracted(&du).dot(&duv) / duv.square_modulus()));
        d = p2.added(&duv.multiplied(vv)).subtracted(p).square_modulus();
        if d < ad {
            u = 1. - vv;
            v = vv;
            ad = d;
        }
        return (ad, XY::new(u, v));
    }
    let l2u = du.square_modulus();
    let l2v = dv.square_modulus();
    if l2u < res {
        if l2v < res {
            (p.subtracted(p1).square_modulus(), XY::new(0., 0.))
        } else {
            let uv = XY::new(0., dp.dot(&dv) / l2v);
            (p.subtracted(&p1.added(&dv.multiplied(uv.y()))).square_modulus(), uv)
        }
    } else if l2v < res {
        let uv = XY::new(dp.dot(&du) / l2u, 0.);
        (p.subtracted(&p1.added(&du.multiplied(uv.x()))).square_modulus(), uv)
    } else {
        let u = min(1., max(0., dp.dot(&du) / l2u));
        let v = min(1., max(0., dp.dot(&dv) / l2v));
        let d1 = dp.subtracted(&du.multiplied(u)).square_modulus();
        let d2 = dp.subtracted(&dv.multiplied(v)).square_modulus();
        if d1 < d2 { (d1, XY::new(dp.dot(&du) / l2u, 0.)) } else { (d2, XY::new(0., dp.dot(&dv) / l2v)) }
    }
}

fn determinant(a: &[[f64; 4]; 3], c1: usize, c2: usize, c3: usize) -> f64 {
    let g = |r: usize, c: usize| a.get(r).and_then(|row| row.get(c)).copied().unwrap_or(0.0);
    g(0, c1) * g(1, c2) * g(2, c3) + g(0, c2) * g(1, c3) * g(2, c1) + g(0, c3) * g(1, c1) * g(2, c2)
        - g(0, c3) * g(1, c2) * g(2, c1)
        - g(0, c2) * g(1, c1) * g(2, c3)
        - g(0, c1) * g(1, c3) * g(2, c2)
}

/// `Poly::IntersectTriLine`: 1 and the line parameter when the half line meets the triangle, else 0.
pub fn intersect_tri_line(start: &XYZ, dir: &Dir, v0: &XYZ, v1: &XYZ, v2: &XYZ) -> (i32, Option<f64>) {
    let mut res = 0;
    let conf = 1e-15;
    let m = [
        [-dir.x(), v1.x() - v0.x(), v2.x() - v0.x(), start.x() - v0.x()],
        [-dir.y(), v1.y() - v0.y(), v2.y() - v0.y(), start.y() - v0.y()],
        [-dir.z(), v1.z() - v0.z(), v2.z() - v0.z(), start.z() - v0.z()],
    ];
    let d = determinant(&m, 0, 1, 2);
    let dt = determinant(&m, 3, 1, 2);
    if d > conf {
        let da = determinant(&m, 0, 3, 2);
        if da > -conf {
            let db = determinant(&m, 0, 1, 3);
            res = i32::from((db > -conf) && (da + db <= d + conf));
        }
    } else if d < -conf {
        let da = determinant(&m, 0, 3, 2);
        if da < conf {
            let db = determinant(&m, 0, 1, 3);
            res = i32::from((db < conf) && (da + db >= d - conf));
        }
    }
    (res, if res != 0 { Some(dt / d) } else { None })
}

/// `Poly::Intersect`: the triangle hit by the axis (the closest one, or the farthest) and the parameter.
pub fn intersect(tri: &Triangulation, axis: &Ax1, closest: bool) -> Option<(Triangle, f64)> {
    let conf = 1e-15;
    let loc = axis.location().xyz();
    let dir = axis.direction();
    let mut result = if closest { f64::MAX } else { 0.0 };
    let mut found = Triangle::new();
    for iter in 1..=tri.nb_triangles() {
        let t = tri.triangle(iter).ok()?;
        let (a, b, c) = t.get();
        let (n0, n1, n2) = (tri.node(a).ok()?, tri.node(b).ok()?, tri.node(c).ok()?);
        let (r, param) = intersect_tri_line(loc, dir, n0.xyz(), n1.xyz(), n2.xyz());
        if r != 0 {
            let param = param?;
            if param > conf {
                if closest {
                    if param < result {
                        result = param;
                        found = t;
                    }
                } else if param > result {
                    result = param;
                    found = t;
                }
            }
        }
    }
    if conf < result && result < f64::MAX { Some((found, result)) } else { None }
}

/// `Poly::PolygonProperties`: the (signed) area and the perimeter of a polygon; fewer than 2 points give `(0, 0)`.
pub fn polygon_properties(pts: &[Pnt2d]) -> (f64, f64) {
    let (Some(first), Some(second)) = (pts.first(), pts.get(1)) else { return (0.0, 0.0) };
    let ref_pnt = *first.xy();
    let mut prev = second.xy().subtracted(&ref_pnt);
    let mut area = 0.0;
    let mut perimeter = prev.modulus();
    for p in pts.iter().skip(2) {
        let curr = p.xy().subtracted(&ref_pnt);
        let delta = prev.crossed(&curr);
        area += delta;
        perimeter += prev.subtracted(&curr).modulus();
        prev = curr;
    }
    perimeter += prev.modulus();
    area *= 0.5;
    (area, perimeter)
}
