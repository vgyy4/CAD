//! OCCT's `ElCLib`: points, derivatives and parameters of lines and conics (3D part).
// 0.70710678118655 is OCCT's literal, not an approximation of FRAC_1_SQRT_2 to replace.
#![allow(clippy::approx_constant)]
use crate::gp::{self, Circ, Elips, Hypr, Lin, Parab, Pnt, Vec, XYZ};
use std::f64::consts::PI;
fn form(a: f64, x: XYZ, b: f64, y: XYZ, loc: XYZ) -> XYZ {
    XYZ { x: a * x.x() + b * y.x() + loc.x(), y: a * x.y() + b * y.y() + loc.y(), z: a * x.z() + b * y.z() + loc.z() }
}
fn form0(a: f64, x: XYZ, b: f64, y: XYZ) -> XYZ {
    XYZ { x: a * x.x() + b * y.x(), y: a * x.y() + b * y.y(), z: a * x.z() + b * y.z() }
}
// ElCLib::LineD1
pub fn d1_lin(u: f64, l: &Lin) -> (Pnt, Vec) {
    let a = l.0.dir.0;
    let p = l.0.loc.0;
    (gp::point(gp::xyz(u * a.x() + p.x(), u * a.y() + p.y(), u * a.z() + p.z())), gp::vector(a))
}
// ElCLib::CircleD3
pub fn d3_circ(u: f64, c: &Circ) -> (Pnt, Vec, Vec, Vec) {
    let a = c.r * libm::cos(u);
    let b = c.r * libm::sin(u);
    let (x, y, p) = (c.a.x.0, c.a.y.0, c.a.loc.0);
    (
        gp::point(form(a, x, b, y, p)),
        gp::vector(form0(-b, x, a, y)),
        gp::vector(form0(-a, x, -b, y)),
        gp::vector(form0(b, x, -a, y)),
    )
}
// ElCLib::EllipseD3
pub fn d3_elips(u: f64, c: &Elips) -> (Pnt, Vec, Vec, Vec) {
    let (a, b) = (libm::cos(u), libm::sin(u));
    let (x, y, p) = (c.a.x.0, c.a.y.0, c.a.loc.0);
    (
        gp::point(form(a * c.major, x, b * c.minor, y, p)),
        gp::vector(form0(-b * c.major, x, a * c.minor, y)),
        gp::vector(form0(-a * c.major, x, -b * c.minor, y)),
        gp::vector(form0(b * c.major, x, -a * c.minor, y)),
    )
}
// ElCLib::HyperbolaD3
pub fn d3_hypr(u: f64, c: &Hypr) -> (Pnt, Vec, Vec, Vec) {
    let (a, b) = (libm::cosh(u), libm::sinh(u));
    let (x, y) = (c.a.x.0, c.a.y.0);
    let v = form0(b * c.major, x, a * c.minor, y);
    let q = form0(a * c.major, x, b * c.minor, y);
    (gp::point(q.add(c.a.loc.0)), gp::vector(v), gp::vector(q), gp::vector(v))
}
// ElCLib::ParabolaD2
pub fn d2_parab(u: f64, c: &Parab) -> (Pnt, Vec, Vec) {
    let (x, y, p) = (c.a.x.0, c.a.y.0, c.a.loc.0);
    if c.f.abs() <= f64::MIN_POSITIVE {
        (
            gp::point(gp::xyz(u * x.x() + p.x(), u * x.y() + p.y(), u * x.z() + p.z())),
            gp::vector(x),
            gp::vector(gp::xyz(0., 0., 0.)),
        )
    } else {
        (
            gp::point(form(u * u / (4. * c.f), x, u, y, p)),
            gp::vector(form0(u / (2. * c.f), x, 1., y)),
            gp::vector(x.scale(1. / (2. * c.f))),
        )
    }
}
// ElCLib::LineDN
pub fn dn_lin(_: f64, l: &Lin, n: i32) -> Vec {
    gp::vector(if n == 1 { l.0.dir.0 } else { gp::xyz(0., 0., 0.) })
}
// ElCLib::CircleDN
pub fn dn_circ(u: f64, c: &Circ, n: i32) -> Vec {
    let (mut a, mut b) = (0., 0.);
    if n == 1 {
        a = c.r * -libm::sin(u);
        b = c.r * libm::cos(u)
    } else if (n + 2) % 4 == 0 {
        a = c.r * -libm::cos(u);
        b = c.r * -libm::sin(u)
    } else if (n + 1) % 4 == 0 {
        a = c.r * libm::sin(u);
        b = c.r * -libm::cos(u)
    } else if n % 4 == 0 {
        a = c.r * libm::cos(u);
        b = c.r * libm::sin(u)
    } else if (n - 1) % 4 == 0 {
        a = c.r * -libm::sin(u);
        b = c.r * libm::cos(u)
    }
    gp::vector(form0(a, c.a.x.0, b, c.a.y.0))
}
// ElCLib::EllipseDN
pub fn dn_elips(u: f64, c: &Elips, n: i32) -> Vec {
    let (mut a, mut b) = (0., 0.);
    if n == 1 {
        a = c.major * -libm::sin(u);
        b = c.minor * libm::cos(u)
    } else if (n + 2) % 4 == 0 {
        a = c.major * -libm::cos(u);
        b = c.minor * -libm::sin(u)
    } else if (n + 1) % 4 == 0 {
        a = c.major * libm::sin(u);
        b = c.minor * -libm::cos(u)
    } else if n % 4 == 0 {
        a = c.major * libm::cos(u);
        b = c.minor * libm::sin(u)
    } else if (n - 1) % 4 == 0 {
        a = c.major * -libm::sin(u);
        b = c.minor * libm::cos(u)
    }
    gp::vector(form0(a, c.a.x.0, b, c.a.y.0))
}
// ElCLib::HyperbolaDN
pub fn dn_hypr(u: f64, c: &Hypr, n: i32) -> Vec {
    let (a, b) = if n % 2 != 0 {
        (c.major * libm::sinh(u), c.minor * libm::cosh(u))
    } else {
        (c.major * libm::cosh(u), c.minor * libm::sinh(u))
    };
    gp::vector(form0(a, c.a.x.0, b, c.a.y.0))
}
// ElCLib::ParabolaDN
pub fn dn_parab(u: f64, c: &Parab, n: i32) -> Vec {
    if n > 2 || n <= 0 {
        return gp::vector(gp::xyz(0., 0., 0.));
    }
    if n == 1 {
        if c.f.abs() <= f64::MIN_POSITIVE {
            return gp::vector(c.a.x.0);
        }
        return gp::vector(form0(u / (2. * c.f), c.a.x.0, 1., c.a.y.0));
    }
    if c.f.abs() <= f64::MIN_POSITIVE {
        return gp::vector(gp::xyz(0., 0., 0.));
    }
    gp::vector(c.a.x.0.scale(1. / (2. * c.f)))
}
// ElCLib::LineParameter
pub fn parameter_lin(l: &Lin, p: &Pnt) -> f64 {
    let q = p.0;
    let o = l.0.loc.0;
    let d = l.0.dir.0;
    (q.x() - o.x()) * d.x() + (q.y() - o.y()) * d.y() + (q.z() - o.z()) * d.z()
}
// ElCLib::CircleParameter
pub fn parameter_circ(c: &Circ, p: &Pnt) -> f64 {
    let o = c.a.loc.0;
    let q = p.0;
    let v = gp::xyz(q.x() - o.x(), q.y() - o.y(), q.z() - o.z());
    if v.dot(v) < f64::MIN_POSITIVE {
        return 0.;
    }
    let n = c.a.dir.0;
    let proj = n.cross(v.cross(n));
    if proj.dot(proj) < f64::MIN_POSITIVE {
        return 0.;
    }
    let mut t = angle_with_ref(c.a.x.0, dir_coords(proj), n);
    normalize(&mut t);
    t
}
// ElCLib::EllipseParameter
pub fn parameter_elips(c: &Elips, p: &Pnt) -> f64 {
    let q = p.0;
    let o = c.a.loc.0;
    let op = gp::xyz(q.x() - o.x(), q.y() - o.y(), q.z() - o.z());
    let x = c.a.x.0;
    let y = c.a.y.0;
    let ny = op.dot(y);
    let nx = op.dot(x);
    if nx.abs() <= f64::MIN_POSITIVE && ny.abs() <= f64::MIN_POSITIVE {
        return 0.;
    }
    let om = x.scale(nx).add(y.scale(ny * (c.major / c.minor)));
    let mut t = angle_with_ref(dir_coords(x), dir_coords(om), dir_coords(c.a.dir.0));
    normalize(&mut t);
    t
}
// ElCLib::HyperbolaParameter
pub fn parameter_hypr(c: &Hypr, p: &Pnt) -> f64 {
    let q = p.0;
    let o = c.a.loc.0;
    libm::asinh((gp::xyz(q.x() - o.x(), q.y() - o.y(), q.z() - o.z()).dot(c.a.y.0)) / c.minor)
}
// ElCLib::ParabolaParameter
pub fn parameter_parab(c: &Parab, p: &Pnt) -> f64 {
    let q = p.0;
    let o = c.a.loc.0;
    gp::xyz(q.x() - o.x(), q.y() - o.y(), q.z() - o.z()).dot(c.a.y.0)
}
/// The coordinates `gp_Dir(x, y, z)` stores: unchanged when already of unit length (within
/// gp::Resolution), else divided by the length. Callers checked the length is not zero.
fn dir_coords(v: XYZ) -> XYZ {
    let sq = v.x() * v.x() + v.y() * v.y() + v.z() * v.z();
    // As written in OCCT; 1.0 ± Resolution rounds to 1.0, so this means "exactly 1".
    #[allow(clippy::double_comparisons)]
    if sq >= 1.0 - f64::MIN_POSITIVE && sq <= 1.0 + f64::MIN_POSITIVE {
        return v;
    }
    let d = sq.sqrt();
    gp::xyz(v.x() / d, v.y() / d, v.z() / d)
}

// gp_Dir::AngleWithRef (`a`, `other` and `vref` are unit directions)
fn angle_with_ref(a: XYZ, other: XYZ, vref: XYZ) -> f64 {
    let c = a.cross(other);
    let cosinus = a.dot(other);
    let sinus = (c.x() * c.x() + c.y() * c.y() + c.z() * c.z()).sqrt();
    let ang = if cosinus > -0.70710678118655 && cosinus < 0.70710678118655 {
        libm::acos(cosinus)
    } else if cosinus < 0.0 {
        PI - libm::asin(sinus)
    } else {
        libm::asin(sinus)
    };
    if c.dot(vref) >= 0.0 { ang } else { -ang }
}

// normalizeAngle (ElCLib.cxx)
fn normalize(t: &mut f64) {
    let pipi = PI + PI;
    while *t < -f64::EPSILON {
        *t += pipi;
    }
    while *t > pipi * (1.0 + f64::MIN_POSITIVE) {
        *t -= pipi;
    }
    if *t < 0.0 {
        *t = 0.0;
    }
}

// ElCLib::InPeriod
pub fn in_period(u: f64, first: f64, last: f64) -> f64 {
    let p = last - first;
    if p < f64::EPSILON * last.abs() { u } else { first.max(u + p * ((first - u) / p).ceil()) }
}
// ElCLib::AdjustPeriodic
pub fn adjust_periodic(first: f64, last: f64, precision: f64, mut u1: f64, mut u2: f64) -> (f64, f64) {
    let p = last - first;
    if p < f64::EPSILON * last.abs() {
        return (first, last);
    }
    u1 -= ((u1 - first) / p).floor() * p;
    if last - u1 < precision {
        u1 -= p
    }
    u2 -= ((u2 - u1) / p).floor() * p;
    if u2 - u1 < precision {
        u2 += p
    }
    (u1, u2)
}
