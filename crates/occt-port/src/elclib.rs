//! OCCT's `ElCLib`: points, derivatives and parameters of lines and conics (3D and 2D), and `To3d`.
//!
//! The functions with a `gp` object (`value_circ`, `d1_circ2d`, ...) call the position-based
//! functions (`circle_value`, `circle2d_d1`, ...) exactly like `ElCLib.lxx` does.
// 0.70710678118655 is OCCT's literal, not an approximation of FRAC_1_SQRT_2 to replace.
#![allow(clippy::approx_constant)]
use crate::gp::{
    self, Ax1, Ax2, Ax2d, Ax22d, Circ, Circ2d, ConstructionError, Dir, Dir2d, Elips, Elips2d, Hypr, Hypr2d, Lin, Lin2d,
    Parab, Parab2d, Pnt, Pnt2d, Vec, Vec2d, XY, XYZ,
};
use std::f64::consts::PI;

// ---------------------------------------------------------------------------------------------
// gp_XYZ / gp_XY helpers (SetLinearForm into a fresh value)

// A1 * X1 + A2 * X2 + X3
fn lf2p(a1: f64, x1: &XYZ, a2: f64, x2: &XYZ, x3: &XYZ) -> XYZ {
    let mut r = XYZ::zero();
    r.set_linear_form_2_plus(a1, x1, a2, x2, x3);
    r
}
// A1 * X1 + A2 * X2
fn lf2(a1: f64, x1: &XYZ, a2: f64, x2: &XYZ) -> XYZ {
    let mut r = XYZ::zero();
    r.set_linear_form_2(a1, x1, a2, x2);
    r
}
// A1 * X1 + X2
fn lf1p(a1: f64, x1: &XYZ, x2: &XYZ) -> XYZ {
    let mut r = XYZ::zero();
    r.set_linear_form_1_plus(a1, x1, x2);
    r
}
// 2D: A1 * X1 + A2 * X2 + X3
fn lf2p_xy(a1: f64, x1: &XY, a2: f64, x2: &XY, x3: &XY) -> XY {
    let mut r = XY::zero();
    r.set_linear_form_2_plus(a1, x1, a2, x2, x3);
    r
}
// 2D: A1 * X1 + A2 * X2
fn lf2_xy(a1: f64, x1: &XY, a2: f64, x2: &XY) -> XY {
    let mut r = XY::zero();
    r.set_linear_form_2(a1, x1, a2, x2);
    r
}
// 2D: A1 * X1 + X2
fn lf1p_xy(a1: f64, x1: &XY, x2: &XY) -> XY {
    let mut r = XY::zero();
    r.set_linear_form_1_plus(a1, x1, x2);
    r
}

// The X direction, the Y direction and the location of a 3D frame.
fn frame3(pos: &Ax2) -> (XYZ, XYZ, XYZ) {
    (*pos.x_direction().xyz(), *pos.y_direction().xyz(), *pos.location().xyz())
}
// The X direction, the Y direction and the location of a 2D frame.
fn frame2(pos: &Ax22d) -> (XY, XY, XY) {
    (*pos.x_direction().xy(), *pos.y_direction().xy(), *pos.location().xy())
}
fn zero_vec() -> Vec {
    Vec::new(0., 0., 0.)
}
fn zero_vec2d() -> Vec2d {
    Vec2d::new(0., 0.)
}

// ---------------------------------------------------------------------------------------------
// 3D: Value

// ElCLib::LineValue
pub fn line_value(u: f64, pos: &Ax1) -> Pnt {
    let z = pos.direction().xyz();
    let p = pos.location().xyz();
    Pnt::new(u * z.x() + p.x(), u * z.y() + p.y(), u * z.z() + p.z())
}
// ElCLib::CircleValue
pub fn circle_value(u: f64, pos: &Ax2, radius: f64) -> Pnt {
    let (x, y, p) = frame3(pos);
    let a1 = radius * libm::cos(u);
    let a2 = radius * libm::sin(u);
    Pnt::from_xyz(lf2p(a1, &x, a2, &y, &p))
}
// ElCLib::EllipseValue
pub fn ellipse_value(u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64) -> Pnt {
    let (x, y, p) = frame3(pos);
    let a1 = major_radius * libm::cos(u);
    let a2 = minor_radius * libm::sin(u);
    Pnt::from_xyz(lf2p(a1, &x, a2, &y, &p))
}
// ElCLib::HyperbolaValue
pub fn hyperbola_value(u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64) -> Pnt {
    let (x, y, p) = frame3(pos);
    let a1 = major_radius * libm::cosh(u);
    let a2 = minor_radius * libm::sinh(u);
    Pnt::from_xyz(lf2p(a1, &x, a2, &y, &p))
}
// ElCLib::ParabolaValue
pub fn parabola_value(u: f64, pos: &Ax2, focal: f64) -> Pnt {
    let (x, y, p) = frame3(pos);
    if focal.abs() <= gp::resolution() {
        return Pnt::new(u * x.x() + p.x(), u * x.y() + p.y(), u * x.z() + p.z());
    }
    let a1 = u * u / (4.0 * focal);
    Pnt::from_xyz(lf2p(a1, &x, u, &y, &p))
}
// ElCLib::Value(U, gp_Lin)
pub fn value_lin(u: f64, c: &Lin) -> Pnt {
    line_value(u, c.position())
}
// ElCLib::Value(U, gp_Circ)
pub fn value_circ(u: f64, c: &Circ) -> Pnt {
    circle_value(u, c.position(), c.radius())
}
// ElCLib::Value(U, gp_Elips)
pub fn value_elips(u: f64, c: &Elips) -> Pnt {
    ellipse_value(u, c.position(), c.major_radius(), c.minor_radius())
}
// ElCLib::Value(U, gp_Hypr)
pub fn value_hypr(u: f64, c: &Hypr) -> Pnt {
    hyperbola_value(u, c.position(), c.major_radius(), c.minor_radius())
}
// ElCLib::Value(U, gp_Parab)
pub fn value_parab(u: f64, c: &Parab) -> Pnt {
    parabola_value(u, c.position(), c.focal())
}

// ---------------------------------------------------------------------------------------------
// 3D: D1

// ElCLib::LineD1
pub fn line_d1(u: f64, pos: &Ax1) -> (Pnt, Vec) {
    let coord = *pos.direction().xyz();
    (Pnt::from_xyz(lf1p(u, &coord, pos.location().xyz())), Vec::from_xyz(coord))
}
// ElCLib::CircleD1
pub fn circle_d1(u: f64, pos: &Ax2, radius: f64) -> (Pnt, Vec) {
    let xc = radius * libm::cos(u);
    let yc = radius * libm::sin(u);
    let (x, y, p) = frame3(pos);
    (Pnt::from_xyz(lf2p(xc, &x, yc, &y, &p)), Vec::from_xyz(lf2(-yc, &x, xc, &y)))
}
// ElCLib::EllipseD1
pub fn ellipse_d1(u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64) -> (Pnt, Vec) {
    let xc = libm::cos(u);
    let yc = libm::sin(u);
    let (x, y, p) = frame3(pos);
    (
        Pnt::from_xyz(lf2p(xc * major_radius, &x, yc * minor_radius, &y, &p)),
        Vec::from_xyz(lf2(-yc * major_radius, &x, xc * minor_radius, &y)),
    )
}
// ElCLib::HyperbolaD1
pub fn hyperbola_d1(u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64) -> (Pnt, Vec) {
    let xc = libm::cosh(u);
    let yc = libm::sinh(u);
    let (x, y, p) = frame3(pos);
    (
        Pnt::from_xyz(lf2p(xc * major_radius, &x, yc * minor_radius, &y, &p)),
        Vec::from_xyz(lf2(yc * major_radius, &x, xc * minor_radius, &y)),
    )
}
// ElCLib::ParabolaD1
pub fn parabola_d1(u: f64, pos: &Ax2, focal: f64) -> (Pnt, Vec) {
    let (x, y, p) = frame3(pos);
    if focal.abs() <= gp::resolution() {
        // the parabola degenerates into a line
        let mut coord1 = x;
        coord1.multiply(u);
        coord1.add(&p);
        (Pnt::from_xyz(coord1), Vec::from_xyz(x))
    } else {
        let v1 = lf1p(u / (2.0 * focal), &x, &y);
        (Pnt::from_xyz(lf2p((u * u) / (4.0 * focal), &x, u, &y, &p)), Vec::from_xyz(v1))
    }
}
// ElCLib::D1(U, gp_Lin)
pub fn d1_lin(u: f64, l: &Lin) -> (Pnt, Vec) {
    line_d1(u, l.position())
}
// ElCLib::D1(U, gp_Circ)
pub fn d1_circ(u: f64, c: &Circ) -> (Pnt, Vec) {
    circle_d1(u, c.position(), c.radius())
}
// ElCLib::D1(U, gp_Elips)
pub fn d1_elips(u: f64, c: &Elips) -> (Pnt, Vec) {
    ellipse_d1(u, c.position(), c.major_radius(), c.minor_radius())
}
// ElCLib::D1(U, gp_Hypr)
pub fn d1_hypr(u: f64, c: &Hypr) -> (Pnt, Vec) {
    hyperbola_d1(u, c.position(), c.major_radius(), c.minor_radius())
}
// ElCLib::D1(U, gp_Parab)
pub fn d1_parab(u: f64, c: &Parab) -> (Pnt, Vec) {
    parabola_d1(u, c.position(), c.focal())
}

// ---------------------------------------------------------------------------------------------
// 3D: D2

// ElCLib::CircleD2
pub fn circle_d2(u: f64, pos: &Ax2, radius: f64) -> (Pnt, Vec, Vec) {
    let xc = radius * libm::cos(u);
    let yc = radius * libm::sin(u);
    let (x, y, p) = frame3(pos);
    (Pnt::from_xyz(lf2p(xc, &x, yc, &y, &p)), Vec::from_xyz(lf2(-yc, &x, xc, &y)), Vec::from_xyz(lf2(-xc, &x, -yc, &y)))
}
// ElCLib::EllipseD2
pub fn ellipse_d2(u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64) -> (Pnt, Vec, Vec) {
    let xc = libm::cos(u);
    let yc = libm::sin(u);
    let (x, y, p) = frame3(pos);
    (
        Pnt::from_xyz(lf2p(xc * major_radius, &x, yc * minor_radius, &y, &p)),
        Vec::from_xyz(lf2(-yc * major_radius, &x, xc * minor_radius, &y)),
        Vec::from_xyz(lf2(-xc * major_radius, &x, -yc * minor_radius, &y)),
    )
}
// ElCLib::HyperbolaD2
pub fn hyperbola_d2(u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64) -> (Pnt, Vec, Vec) {
    let xc = libm::cosh(u);
    let yc = libm::sinh(u);
    let (x, y, p) = frame3(pos);
    let v2 = lf2(xc * major_radius, &x, yc * minor_radius, &y);
    let mut coord0 = v2;
    coord0.add(&p);
    (Pnt::from_xyz(coord0), Vec::from_xyz(lf2(yc * major_radius, &x, xc * minor_radius, &y)), Vec::from_xyz(v2))
}
// ElCLib::ParabolaD2
pub fn parabola_d2(u: f64, pos: &Ax2, focal: f64) -> (Pnt, Vec, Vec) {
    let (x, y, p) = frame3(pos);
    if focal.abs() <= gp::resolution() {
        let mut coord1 = x;
        coord1.multiply(u);
        coord1.add(&p);
        (Pnt::from_xyz(coord1), Vec::from_xyz(x), zero_vec())
    } else {
        let pt = lf2p((u * u) / (4.0 * focal), &x, u, &y, &p);
        let v1 = lf1p(u / (2.0 * focal), &x, &y);
        let mut coord1 = x;
        coord1.multiply(1.0 / (2.0 * focal));
        (Pnt::from_xyz(pt), Vec::from_xyz(v1), Vec::from_xyz(coord1))
    }
}
// ElCLib::D2(U, gp_Circ)
pub fn d2_circ(u: f64, c: &Circ) -> (Pnt, Vec, Vec) {
    circle_d2(u, c.position(), c.radius())
}
// ElCLib::D2(U, gp_Elips)
pub fn d2_elips(u: f64, c: &Elips) -> (Pnt, Vec, Vec) {
    ellipse_d2(u, c.position(), c.major_radius(), c.minor_radius())
}
// ElCLib::D2(U, gp_Hypr)
pub fn d2_hypr(u: f64, c: &Hypr) -> (Pnt, Vec, Vec) {
    hyperbola_d2(u, c.position(), c.major_radius(), c.minor_radius())
}
// ElCLib::D2(U, gp_Parab)
pub fn d2_parab(u: f64, c: &Parab) -> (Pnt, Vec, Vec) {
    parabola_d2(u, c.position(), c.focal())
}

// ---------------------------------------------------------------------------------------------
// 3D: D3

// ElCLib::CircleD3
pub fn circle_d3(u: f64, pos: &Ax2, radius: f64) -> (Pnt, Vec, Vec, Vec) {
    let xc = radius * libm::cos(u);
    let yc = radius * libm::sin(u);
    let (x, y, p) = frame3(pos);
    (
        Pnt::from_xyz(lf2p(xc, &x, yc, &y, &p)),
        Vec::from_xyz(lf2(-yc, &x, xc, &y)),
        Vec::from_xyz(lf2(-xc, &x, -yc, &y)),
        Vec::from_xyz(lf2(yc, &x, -xc, &y)),
    )
}
// ElCLib::EllipseD3
pub fn ellipse_d3(u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64) -> (Pnt, Vec, Vec, Vec) {
    let xc = libm::cos(u);
    let yc = libm::sin(u);
    let (x, y, p) = frame3(pos);
    (
        Pnt::from_xyz(lf2p(xc * major_radius, &x, yc * minor_radius, &y, &p)),
        Vec::from_xyz(lf2(-yc * major_radius, &x, xc * minor_radius, &y)),
        Vec::from_xyz(lf2(-xc * major_radius, &x, -yc * minor_radius, &y)),
        Vec::from_xyz(lf2(yc * major_radius, &x, -xc * minor_radius, &y)),
    )
}
// ElCLib::HyperbolaD3
pub fn hyperbola_d3(u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64) -> (Pnt, Vec, Vec, Vec) {
    let xc = libm::cosh(u);
    let yc = libm::sinh(u);
    let (x, y, p) = frame3(pos);
    let v2 = lf2(xc * major_radius, &x, yc * minor_radius, &y);
    let mut coord0 = v2;
    coord0.add(&p);
    let v1 = lf2(yc * major_radius, &x, xc * minor_radius, &y);
    (Pnt::from_xyz(coord0), Vec::from_xyz(v1), Vec::from_xyz(v2), Vec::from_xyz(v1))
}
// ElCLib::D3(U, gp_Circ)
pub fn d3_circ(u: f64, c: &Circ) -> (Pnt, Vec, Vec, Vec) {
    circle_d3(u, c.position(), c.radius())
}
// ElCLib::D3(U, gp_Elips)
pub fn d3_elips(u: f64, c: &Elips) -> (Pnt, Vec, Vec, Vec) {
    ellipse_d3(u, c.position(), c.major_radius(), c.minor_radius())
}
// ElCLib::D3(U, gp_Hypr)
pub fn d3_hypr(u: f64, c: &Hypr) -> (Pnt, Vec, Vec, Vec) {
    hyperbola_d3(u, c.position(), c.major_radius(), c.minor_radius())
}

// ---------------------------------------------------------------------------------------------
// 3D: DN

// ElCLib::LineDN
pub fn line_dn(_u: f64, pos: &Ax1, n: i32) -> Vec {
    if n == 1 { Vec::from_dir(pos.direction()) } else { zero_vec() }
}
// The (Xc, Yc) factors of the N-th derivative of a circle or an ellipse (`rx`, `ry` the radii).
fn trig_dn(u: f64, rx: f64, ry: f64, n: i32) -> (f64, f64) {
    if n == 1 {
        (rx * -libm::sin(u), ry * libm::cos(u))
    } else if (n + 2) % 4 == 0 {
        (rx * -libm::cos(u), ry * -libm::sin(u))
    } else if (n + 1) % 4 == 0 {
        (rx * libm::sin(u), ry * -libm::cos(u))
    } else if n % 4 == 0 {
        (rx * libm::cos(u), ry * libm::sin(u))
    } else if (n - 1) % 4 == 0 {
        (rx * -libm::sin(u), ry * libm::cos(u))
    } else {
        (0., 0.)
    }
}
// The (Xc, Yc) factors of the N-th derivative of a hyperbola. OCCT's IsOdd is `N % 2 == 1` and
// IsEven `N % 2 == 0` (C truncation), so a negative odd N is neither: both factors stay 0.
fn hyp_dn(u: f64, major: f64, minor: f64, n: i32) -> (f64, f64) {
    if n % 2 == 1 {
        (major * libm::sinh(u), minor * libm::cosh(u))
    } else if n % 2 == 0 {
        (major * libm::cosh(u), minor * libm::sinh(u))
    } else {
        (0., 0.)
    }
}
// ElCLib::CircleDN
pub fn circle_dn(u: f64, pos: &Ax2, radius: f64, n: i32) -> Vec {
    let (xc, yc) = trig_dn(u, radius, radius, n);
    Vec::from_xyz(lf2(xc, pos.x_direction().xyz(), yc, pos.y_direction().xyz()))
}
// ElCLib::EllipseDN
pub fn ellipse_dn(u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64, n: i32) -> Vec {
    let (xc, yc) = trig_dn(u, major_radius, minor_radius, n);
    Vec::from_xyz(lf2(xc, pos.x_direction().xyz(), yc, pos.y_direction().xyz()))
}
// ElCLib::HyperbolaDN
pub fn hyperbola_dn(u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64, n: i32) -> Vec {
    let (xc, yc) = hyp_dn(u, major_radius, minor_radius, n);
    Vec::from_xyz(lf2(xc, pos.x_direction().xyz(), yc, pos.y_direction().xyz()))
}
// ElCLib::ParabolaDN
pub fn parabola_dn(u: f64, pos: &Ax2, focal: f64, n: i32) -> Vec {
    if n > 2 || n <= 0 {
        return zero_vec();
    }
    let (x, y, _) = frame3(pos);
    if n == 1 {
        if focal.abs() <= gp::resolution() {
            return Vec::from_xyz(x);
        }
        return Vec::from_xyz(lf1p(u / (2.0 * focal), &x, &y));
    }
    if focal.abs() <= gp::resolution() {
        return zero_vec();
    }
    let mut coord1 = x;
    coord1.multiply(1.0 / (2.0 * focal));
    Vec::from_xyz(coord1)
}
// ElCLib::DN(U, gp_Lin, N)
pub fn dn_lin(u: f64, l: &Lin, n: i32) -> Vec {
    line_dn(u, l.position(), n)
}
// ElCLib::DN(U, gp_Circ, N)
pub fn dn_circ(u: f64, c: &Circ, n: i32) -> Vec {
    circle_dn(u, c.position(), c.radius(), n)
}
// ElCLib::DN(U, gp_Elips, N)
pub fn dn_elips(u: f64, c: &Elips, n: i32) -> Vec {
    ellipse_dn(u, c.position(), c.major_radius(), c.minor_radius(), n)
}
// ElCLib::DN(U, gp_Hypr, N)
pub fn dn_hypr(u: f64, c: &Hypr, n: i32) -> Vec {
    hyperbola_dn(u, c.position(), c.major_radius(), c.minor_radius(), n)
}
// ElCLib::DN(U, gp_Parab, N)
pub fn dn_parab(u: f64, c: &Parab, n: i32) -> Vec {
    parabola_dn(u, c.position(), c.focal(), n)
}

// ---------------------------------------------------------------------------------------------
// 3D: Parameter

// ElCLib::LineParameter
pub fn line_parameter(pos: &Ax1, p: &Pnt) -> f64 {
    p.xyz().subtracted(pos.location().xyz()).dot(pos.direction().xyz())
}
// ElCLib::CircleParameter
pub fn circle_parameter(pos: &Ax2, p: &Pnt) -> f64 {
    let o = pos.location().xyz();
    let q = p.xyz();
    let v = gp::xyz(q.x() - o.x(), q.y() - o.y(), q.z() - o.z());
    if v.dot(&v) < f64::MIN_POSITIVE {
        return 0.;
    }
    let n = *pos.direction().xyz();
    let proj = n.crossed(&v.crossed(&n));
    if proj.dot(&proj) < f64::MIN_POSITIVE {
        return 0.;
    }
    let mut t = angle_with_ref(*pos.x_direction().xyz(), dir_coords(proj), n);
    normalize(&mut t);
    t
}
// ElCLib::EllipseParameter
pub fn ellipse_parameter(pos: &Ax2, major_radius: f64, minor_radius: f64, p: &Pnt) -> f64 {
    let q = p.xyz();
    let o = pos.location().xyz();
    let op = gp::xyz(q.x() - o.x(), q.y() - o.y(), q.z() - o.z());
    let x = *pos.x_direction().xyz();
    let y = *pos.y_direction().xyz();
    let ny = op.dot(&y);
    let nx = op.dot(&x);
    if nx.abs() <= f64::MIN_POSITIVE && ny.abs() <= f64::MIN_POSITIVE {
        return 0.;
    }
    let om = x.multiplied(nx).added(&y.multiplied(ny * (major_radius / minor_radius)));
    let mut t = angle_with_ref(dir_coords(x), dir_coords(om), dir_coords(*pos.direction().xyz()));
    normalize(&mut t);
    t
}
// ElCLib::HyperbolaParameter
pub fn hyperbola_parameter(pos: &Ax2, _major_radius: f64, minor_radius: f64, p: &Pnt) -> f64 {
    let q = p.xyz();
    let o = pos.location().xyz();
    libm::asinh((gp::xyz(q.x() - o.x(), q.y() - o.y(), q.z() - o.z()).dot(pos.y_direction().xyz())) / minor_radius)
}
// ElCLib::ParabolaParameter
pub fn parabola_parameter(pos: &Ax2, p: &Pnt) -> f64 {
    let q = p.xyz();
    let o = pos.location().xyz();
    gp::xyz(q.x() - o.x(), q.y() - o.y(), q.z() - o.z()).dot(pos.y_direction().xyz())
}
// ElCLib::Parameter(gp_Lin, P)
pub fn parameter_lin(l: &Lin, p: &Pnt) -> f64 {
    line_parameter(l.position(), p)
}
// ElCLib::Parameter(gp_Circ, P)
pub fn parameter_circ(c: &Circ, p: &Pnt) -> f64 {
    circle_parameter(c.position(), p)
}
// ElCLib::Parameter(gp_Elips, P)
pub fn parameter_elips(c: &Elips, p: &Pnt) -> f64 {
    ellipse_parameter(c.position(), c.major_radius(), c.minor_radius(), p)
}
// ElCLib::Parameter(gp_Hypr, P)
pub fn parameter_hypr(c: &Hypr, p: &Pnt) -> f64 {
    hyperbola_parameter(c.position(), c.major_radius(), c.minor_radius(), p)
}
// ElCLib::Parameter(gp_Parab, P)
pub fn parameter_parab(c: &Parab, p: &Pnt) -> f64 {
    parabola_parameter(c.position(), p)
}

// ---------------------------------------------------------------------------------------------
// 2D: Value

// ElCLib::LineValue (2D)
pub fn line2d_value(u: f64, pos: &Ax2d) -> Pnt2d {
    let z = pos.direction().xy();
    let p = pos.location().xy();
    Pnt2d::new(u * z.x() + p.x(), u * z.y() + p.y())
}
// ElCLib::CircleValue (2D)
pub fn circle2d_value(u: f64, pos: &Ax22d, radius: f64) -> Pnt2d {
    let (x, y, p) = frame2(pos);
    let a1 = radius * libm::cos(u);
    let a2 = radius * libm::sin(u);
    Pnt2d::from_xy(lf2p_xy(a1, &x, a2, &y, &p))
}
// ElCLib::EllipseValue (2D)
pub fn ellipse2d_value(u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64) -> Pnt2d {
    let (x, y, p) = frame2(pos);
    let a1 = major_radius * libm::cos(u);
    let a2 = minor_radius * libm::sin(u);
    Pnt2d::from_xy(lf2p_xy(a1, &x, a2, &y, &p))
}
// ElCLib::HyperbolaValue (2D)
pub fn hyperbola2d_value(u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64) -> Pnt2d {
    let (x, y, p) = frame2(pos);
    let a1 = major_radius * libm::cosh(u);
    let a2 = minor_radius * libm::sinh(u);
    Pnt2d::from_xy(lf2p_xy(a1, &x, a2, &y, &p))
}
// ElCLib::ParabolaValue (2D)
pub fn parabola2d_value(u: f64, pos: &Ax22d, focal: f64) -> Pnt2d {
    let (x, y, p) = frame2(pos);
    if focal.abs() <= gp::resolution() {
        return Pnt2d::new(u * x.x() + p.x(), u * x.y() + p.y());
    }
    let a1 = u * u / (4.0 * focal);
    Pnt2d::from_xy(lf2p_xy(a1, &x, u, &y, &p))
}
// ElCLib::Value(U, gp_Lin2d)
pub fn value_lin2d(u: f64, c: &Lin2d) -> Pnt2d {
    line2d_value(u, c.position())
}
// ElCLib::Value(U, gp_Circ2d)
pub fn value_circ2d(u: f64, c: &Circ2d) -> Pnt2d {
    circle2d_value(u, c.axis(), c.radius())
}
// ElCLib::Value(U, gp_Elips2d)
pub fn value_elips2d(u: f64, c: &Elips2d) -> Pnt2d {
    ellipse2d_value(u, c.axis(), c.major_radius(), c.minor_radius())
}
// ElCLib::Value(U, gp_Hypr2d)
pub fn value_hypr2d(u: f64, c: &Hypr2d) -> Pnt2d {
    hyperbola2d_value(u, c.axis(), c.major_radius(), c.minor_radius())
}
// ElCLib::Value(U, gp_Parab2d)
pub fn value_parab2d(u: f64, c: &Parab2d) -> Pnt2d {
    parabola2d_value(u, c.axis(), c.focal())
}

// ---------------------------------------------------------------------------------------------
// 2D: D1

// ElCLib::LineD1 (2D)
pub fn line2d_d1(u: f64, pos: &Ax2d) -> (Pnt2d, Vec2d) {
    let coord = *pos.direction().xy();
    (Pnt2d::from_xy(lf1p_xy(u, &coord, pos.location().xy())), Vec2d::from_xy(coord))
}
// ElCLib::CircleD1 (2D)
pub fn circle2d_d1(u: f64, pos: &Ax22d, radius: f64) -> (Pnt2d, Vec2d) {
    let xc = radius * libm::cos(u);
    let yc = radius * libm::sin(u);
    let (x, y, p) = frame2(pos);
    (Pnt2d::from_xy(lf2p_xy(xc, &x, yc, &y, &p)), Vec2d::from_xy(lf2_xy(-yc, &x, xc, &y)))
}
// ElCLib::EllipseD1 (2D)
pub fn ellipse2d_d1(u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64) -> (Pnt2d, Vec2d) {
    let xc = libm::cos(u);
    let yc = libm::sin(u);
    let (x, y, p) = frame2(pos);
    (
        Pnt2d::from_xy(lf2p_xy(xc * major_radius, &x, yc * minor_radius, &y, &p)),
        Vec2d::from_xy(lf2_xy(-yc * major_radius, &x, xc * minor_radius, &y)),
    )
}
// ElCLib::HyperbolaD1 (2D)
pub fn hyperbola2d_d1(u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64) -> (Pnt2d, Vec2d) {
    let xc = libm::cosh(u);
    let yc = libm::sinh(u);
    let (x, y, p) = frame2(pos);
    (
        Pnt2d::from_xy(lf2p_xy(xc * major_radius, &x, yc * minor_radius, &y, &p)),
        Vec2d::from_xy(lf2_xy(yc * major_radius, &x, xc * minor_radius, &y)),
    )
}
// ElCLib::ParabolaD1 (2D)
pub fn parabola2d_d1(u: f64, pos: &Ax22d, focal: f64) -> (Pnt2d, Vec2d) {
    let (x, y, p) = frame2(pos);
    if focal.abs() <= gp::resolution() {
        // the parabola degenerates into a line
        (Pnt2d::from_xy(lf1p_xy(u, &x, &p)), Vec2d::from_xy(x))
    } else {
        let v1 = lf1p_xy(u / (2.0 * focal), &x, &y);
        (Pnt2d::from_xy(lf2p_xy((u * u) / (4.0 * focal), &x, u, &y, &p)), Vec2d::from_xy(v1))
    }
}
// ElCLib::D1(U, gp_Lin2d)
pub fn d1_lin2d(u: f64, c: &Lin2d) -> (Pnt2d, Vec2d) {
    line2d_d1(u, c.position())
}
// ElCLib::D1(U, gp_Circ2d)
pub fn d1_circ2d(u: f64, c: &Circ2d) -> (Pnt2d, Vec2d) {
    circle2d_d1(u, c.axis(), c.radius())
}
// ElCLib::D1(U, gp_Elips2d)
pub fn d1_elips2d(u: f64, c: &Elips2d) -> (Pnt2d, Vec2d) {
    ellipse2d_d1(u, c.axis(), c.major_radius(), c.minor_radius())
}
// ElCLib::D1(U, gp_Hypr2d)
pub fn d1_hypr2d(u: f64, c: &Hypr2d) -> (Pnt2d, Vec2d) {
    hyperbola2d_d1(u, c.axis(), c.major_radius(), c.minor_radius())
}
// ElCLib::D1(U, gp_Parab2d)
pub fn d1_parab2d(u: f64, c: &Parab2d) -> (Pnt2d, Vec2d) {
    parabola2d_d1(u, c.axis(), c.focal())
}

// ---------------------------------------------------------------------------------------------
// 2D: D2

// ElCLib::CircleD2 (2D)
pub fn circle2d_d2(u: f64, pos: &Ax22d, radius: f64) -> (Pnt2d, Vec2d, Vec2d) {
    let (x, y, loc) = frame2(pos);
    let xc = radius * libm::cos(u);
    let yc = radius * libm::sin(u);
    let mut vxy = lf2_xy(xc, &x, yc, &y);
    let v2 = vxy.reversed();
    vxy.add(&loc);
    (Pnt2d::from_xy(vxy), Vec2d::from_xy(lf2_xy(-yc, &x, xc, &y)), Vec2d::from_xy(v2))
}
// ElCLib::EllipseD2 (2D)
pub fn ellipse2d_d2(u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64) -> (Pnt2d, Vec2d, Vec2d) {
    let (x, y, loc) = frame2(pos);
    let xc = libm::cos(u);
    let yc = libm::sin(u);
    let mut vxy = lf2_xy(xc * major_radius, &x, yc * minor_radius, &y);
    let v2 = vxy.reversed();
    vxy.add(&loc);
    (Pnt2d::from_xy(vxy), Vec2d::from_xy(lf2_xy(-yc * major_radius, &x, xc * minor_radius, &y)), Vec2d::from_xy(v2))
}
// ElCLib::HyperbolaD2 (2D)
pub fn hyperbola2d_d2(u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64) -> (Pnt2d, Vec2d, Vec2d) {
    let (x, y, loc) = frame2(pos);
    let xc = libm::cosh(u);
    let yc = libm::sinh(u);
    let v2 = lf2_xy(xc * major_radius, &x, yc * minor_radius, &y);
    let mut vxy = v2;
    vxy.add(&loc);
    (Pnt2d::from_xy(vxy), Vec2d::from_xy(lf2_xy(yc * major_radius, &x, xc * minor_radius, &y)), Vec2d::from_xy(v2))
}
// ElCLib::ParabolaD2 (2D)
pub fn parabola2d_d2(u: f64, pos: &Ax22d, focal: f64) -> (Pnt2d, Vec2d, Vec2d) {
    let (x, y, loc) = frame2(pos);
    if focal.abs() <= gp::resolution() {
        (Pnt2d::from_xy(lf1p_xy(u, &x, &loc)), Vec2d::from_xy(x), zero_vec2d())
    } else {
        let v2 = x.multiplied(1.0 / (2.0 * focal));
        let v1 = lf1p_xy(u, &v2, &y);
        let mut vxy = lf2_xy(u * u / (4.0 * focal), &x, u, &y);
        vxy.add(&loc);
        (Pnt2d::from_xy(vxy), Vec2d::from_xy(v1), Vec2d::from_xy(v2))
    }
}
// ElCLib::D2(U, gp_Circ2d)
pub fn d2_circ2d(u: f64, c: &Circ2d) -> (Pnt2d, Vec2d, Vec2d) {
    circle2d_d2(u, c.axis(), c.radius())
}
// ElCLib::D2(U, gp_Elips2d)
pub fn d2_elips2d(u: f64, c: &Elips2d) -> (Pnt2d, Vec2d, Vec2d) {
    ellipse2d_d2(u, c.axis(), c.major_radius(), c.minor_radius())
}
// ElCLib::D2(U, gp_Hypr2d)
pub fn d2_hypr2d(u: f64, c: &Hypr2d) -> (Pnt2d, Vec2d, Vec2d) {
    hyperbola2d_d2(u, c.axis(), c.major_radius(), c.minor_radius())
}
// ElCLib::D2(U, gp_Parab2d)
pub fn d2_parab2d(u: f64, c: &Parab2d) -> (Pnt2d, Vec2d, Vec2d) {
    parabola2d_d2(u, c.axis(), c.focal())
}

// ---------------------------------------------------------------------------------------------
// 2D: D3

// ElCLib::CircleD3 (2D)
pub fn circle2d_d3(u: f64, pos: &Ax22d, radius: f64) -> (Pnt2d, Vec2d, Vec2d, Vec2d) {
    let (x, y, loc) = frame2(pos);
    let xc = radius * libm::cos(u);
    let yc = radius * libm::sin(u);
    let mut vxy = lf2_xy(xc, &x, yc, &y);
    let v2 = vxy.reversed();
    vxy.add(&loc);
    let v1 = lf2_xy(-yc, &x, xc, &y);
    (Pnt2d::from_xy(vxy), Vec2d::from_xy(v1), Vec2d::from_xy(v2), Vec2d::from_xy(v1.reversed()))
}
// ElCLib::EllipseD3 (2D)
pub fn ellipse2d_d3(u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64) -> (Pnt2d, Vec2d, Vec2d, Vec2d) {
    let (x, y, loc) = frame2(pos);
    let xc = libm::cos(u);
    let yc = libm::sin(u);
    let mut vxy = lf2_xy(xc * major_radius, &x, yc * minor_radius, &y);
    let v2 = vxy.reversed();
    vxy.add(&loc);
    let v1 = lf2_xy(-yc * major_radius, &x, xc * minor_radius, &y);
    (Pnt2d::from_xy(vxy), Vec2d::from_xy(v1), Vec2d::from_xy(v2), Vec2d::from_xy(v1.reversed()))
}
// ElCLib::HyperbolaD3 (2D)
pub fn hyperbola2d_d3(u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64) -> (Pnt2d, Vec2d, Vec2d, Vec2d) {
    let (x, y, loc) = frame2(pos);
    let xc = libm::cosh(u);
    let yc = libm::sinh(u);
    let v2 = lf2_xy(xc * major_radius, &x, yc * minor_radius, &y);
    let mut vxy = v2;
    vxy.add(&loc);
    let v1 = lf2_xy(yc * major_radius, &x, xc * minor_radius, &y);
    (Pnt2d::from_xy(vxy), Vec2d::from_xy(v1), Vec2d::from_xy(v2), Vec2d::from_xy(v1))
}
// ElCLib::D3(U, gp_Circ2d)
pub fn d3_circ2d(u: f64, c: &Circ2d) -> (Pnt2d, Vec2d, Vec2d, Vec2d) {
    circle2d_d3(u, c.axis(), c.radius())
}
// ElCLib::D3(U, gp_Elips2d)
pub fn d3_elips2d(u: f64, c: &Elips2d) -> (Pnt2d, Vec2d, Vec2d, Vec2d) {
    ellipse2d_d3(u, c.axis(), c.major_radius(), c.minor_radius())
}
// ElCLib::D3(U, gp_Hypr2d)
pub fn d3_hypr2d(u: f64, c: &Hypr2d) -> (Pnt2d, Vec2d, Vec2d, Vec2d) {
    hyperbola2d_d3(u, c.axis(), c.major_radius(), c.minor_radius())
}

// ---------------------------------------------------------------------------------------------
// 2D: DN

// ElCLib::LineDN (2D)
pub fn line2d_dn(_u: f64, pos: &Ax2d, n: i32) -> Vec2d {
    if n == 1 { Vec2d::from_dir(pos.direction()) } else { zero_vec2d() }
}
// ElCLib::CircleDN (2D)
pub fn circle2d_dn(u: f64, pos: &Ax22d, radius: f64, n: i32) -> Vec2d {
    let (xc, yc) = trig_dn(u, radius, radius, n);
    Vec2d::from_xy(lf2_xy(xc, pos.x_direction().xy(), yc, pos.y_direction().xy()))
}
// ElCLib::EllipseDN (2D)
pub fn ellipse2d_dn(u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64, n: i32) -> Vec2d {
    let (xc, yc) = trig_dn(u, major_radius, minor_radius, n);
    Vec2d::from_xy(lf2_xy(xc, pos.x_direction().xy(), yc, pos.y_direction().xy()))
}
// ElCLib::HyperbolaDN (2D)
pub fn hyperbola2d_dn(u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64, n: i32) -> Vec2d {
    let (xc, yc) = hyp_dn(u, major_radius, minor_radius, n);
    Vec2d::from_xy(lf2_xy(xc, pos.x_direction().xy(), yc, pos.y_direction().xy()))
}
// ElCLib::ParabolaDN (2D)
pub fn parabola2d_dn(u: f64, pos: &Ax22d, focal: f64, n: i32) -> Vec2d {
    if n > 2 || n <= 0 {
        return zero_vec2d();
    }
    let (x, y, _) = frame2(pos);
    if n == 1 {
        if focal.abs() <= gp::resolution() {
            return Vec2d::from_xy(x);
        }
        return Vec2d::from_xy(lf1p_xy(u / (2.0 * focal), &x, &y));
    }
    if focal.abs() <= gp::resolution() {
        return zero_vec2d();
    }
    Vec2d::from_xy(x.multiplied(1.0 / (2.0 * focal)))
}
// ElCLib::DN(U, gp_Lin2d, N)
pub fn dn_lin2d(u: f64, c: &Lin2d, n: i32) -> Vec2d {
    line2d_dn(u, c.position(), n)
}
// ElCLib::DN(U, gp_Circ2d, N)
pub fn dn_circ2d(u: f64, c: &Circ2d, n: i32) -> Vec2d {
    circle2d_dn(u, c.axis(), c.radius(), n)
}
// ElCLib::DN(U, gp_Elips2d, N)
pub fn dn_elips2d(u: f64, c: &Elips2d, n: i32) -> Vec2d {
    ellipse2d_dn(u, c.axis(), c.major_radius(), c.minor_radius(), n)
}
// ElCLib::DN(U, gp_Hypr2d, N)
pub fn dn_hypr2d(u: f64, c: &Hypr2d, n: i32) -> Vec2d {
    hyperbola2d_dn(u, c.axis(), c.major_radius(), c.minor_radius(), n)
}
// ElCLib::DN(U, gp_Parab2d, N)
pub fn dn_parab2d(u: f64, c: &Parab2d, n: i32) -> Vec2d {
    parabola2d_dn(u, c.axis(), c.focal(), n)
}

// ---------------------------------------------------------------------------------------------
// 2D: Parameter

// ElCLib::LineParameter (2D)
pub fn line2d_parameter(pos: &Ax2d, p: &Pnt2d) -> f64 {
    let mut coord = *p.xy();
    coord.subtract(pos.location().xy());
    coord.dot(pos.direction().xy())
}
// ElCLib::CircleParameter (2D). OCCT 8.0.1 as built has the zero-magnitude check of the
// `gp_Dir2d(gp_Vec2d)` conversion compiled out and returns NaN there; this is an `Err`.
pub fn circle2d_parameter(pos: &Ax22d, p: &Pnt2d) -> Result<f64, ConstructionError> {
    let d = Dir2d::from_vec(&Vec2d::between(pos.location(), p))?;
    let mut t = pos.x_direction().angle(&d);
    t = if pos.x_direction().crossed(pos.y_direction()) >= 0.0 { t } else { -t };
    normalize(&mut t);
    Ok(t)
}
// ElCLib::EllipseParameter (2D). `gp_Vec2d::Angle` raises when `Om` has no magnitude: `Err`.
pub fn ellipse2d_parameter(
    pos: &Ax22d,
    major_radius: f64,
    minor_radius: f64,
    p: &Pnt2d,
) -> Result<f64, ConstructionError> {
    let (xaxis, mut yaxis, loc) = frame2(pos);
    let mut op = *p.xy();
    op.subtract(&loc);
    let mut om = xaxis.multiplied(op.dot(&xaxis));
    yaxis.multiply(op.dot(&yaxis) * (major_radius / minor_radius));
    om.add(&yaxis);
    let mut t = Vec2d::from_xy(xaxis).angle(&Vec2d::from_xy(om))?;
    t = if pos.x_direction().crossed(pos.y_direction()) >= 0.0 { t } else { -t };
    normalize(&mut t);
    Ok(t)
}
// ElCLib::HyperbolaParameter (2D)
pub fn hyperbola2d_parameter(pos: &Ax22d, _major_radius: f64, minor_radius: f64, p: &Pnt2d) -> f64 {
    let v = Vec2d::from_xy(*pos.y_direction().xy());
    let sht = Vec2d::between(pos.location(), p).dot(&v) / minor_radius;
    libm::asinh(sht)
}
// ElCLib::ParabolaParameter (2D)
pub fn parabola2d_parameter(pos: &Ax22d, p: &Pnt2d) -> f64 {
    let directrix = Vec2d::from_xy(*pos.y_direction().xy());
    Vec2d::between(pos.location(), p).dot(&directrix)
}
// ElCLib::Parameter(gp_Lin2d, P)
pub fn parameter_lin2d(c: &Lin2d, p: &Pnt2d) -> f64 {
    line2d_parameter(c.position(), p)
}
// ElCLib::Parameter(gp_Circ2d, P)
pub fn parameter_circ2d(c: &Circ2d, p: &Pnt2d) -> Result<f64, ConstructionError> {
    circle2d_parameter(c.axis(), p)
}
// ElCLib::Parameter(gp_Elips2d, P)
pub fn parameter_elips2d(c: &Elips2d, p: &Pnt2d) -> Result<f64, ConstructionError> {
    ellipse2d_parameter(c.axis(), c.major_radius(), c.minor_radius(), p)
}
// ElCLib::Parameter(gp_Hypr2d, P)
pub fn parameter_hypr2d(c: &Hypr2d, p: &Pnt2d) -> f64 {
    hyperbola2d_parameter(c.axis(), c.major_radius(), c.minor_radius(), p)
}
// ElCLib::Parameter(gp_Parab2d, P)
pub fn parameter_parab2d(c: &Parab2d, p: &Pnt2d) -> f64 {
    parabola2d_parameter(c.axis(), p)
}

// ---------------------------------------------------------------------------------------------
// To3d

// ElCLib::To3d(gp_Ax2, gp_Pnt2d)
pub fn to3d_pnt(pos: &Ax2, x: &Pnt2d) -> Pnt {
    let (xd, yd, loc) = frame3(pos);
    Pnt::from_xyz(lf2p(x.x(), &xd, x.y(), &yd, &loc))
}
// ElCLib::To3d(gp_Ax2, gp_Vec2d)
pub fn to3d_vec(pos: &Ax2, x: &Vec2d) -> Vec {
    let mut vx = Vec::from_dir(pos.x_direction());
    let mut vy = Vec::from_dir(pos.y_direction());
    vx.multiply(x.x());
    vy.multiply(x.y());
    vx.add(&vy);
    vx
}
// ElCLib::To3d(gp_Ax2, gp_Dir2d)
pub fn to3d_dir(pos: &Ax2, x: &Dir2d) -> Result<Dir, ConstructionError> {
    let mut vx = Vec::from_dir(pos.x_direction());
    let mut vy = Vec::from_dir(pos.y_direction());
    vx.multiply(x.x());
    vy.multiply(x.y());
    vx.add(&vy);
    Dir::from_vec(&vx)
}
// ElCLib::To3d(gp_Ax2, gp_Ax2d)
pub fn to3d_ax2d(pos: &Ax2, x: &Ax2d) -> Result<Ax1, ConstructionError> {
    let p = to3d_pnt(pos, x.location());
    let v = Vec::from_dir(&to3d_dir(pos, x.direction())?);
    Ok(Ax1::new(p, Dir::from_vec(&v)?))
}
// ElCLib::To3d(gp_Ax2, gp_Ax22d)
pub fn to3d_ax22d(pos: &Ax2, x: &Ax22d) -> Result<Ax2, ConstructionError> {
    let p = to3d_pnt(pos, x.location());
    let vx = Vec::from_dir(&to3d_dir(pos, x.x_direction())?);
    let vy = Vec::from_dir(&to3d_dir(pos, x.y_direction())?);
    Ax2::new(p, Dir::from_vec(&vx.crossed(&vy))?, &Dir::from_vec(&vx)?)
}
// ElCLib::To3d(gp_Ax2, gp_Lin2d)
pub fn to3d_lin(pos: &Ax2, x: &Lin2d) -> Result<Lin, ConstructionError> {
    Ok(Lin::from_axis(to3d_ax2d(pos, x.position())?))
}
// ElCLib::To3d(gp_Ax2, gp_Circ2d)
pub fn to3d_circ(pos: &Ax2, x: &Circ2d) -> Result<Circ, ConstructionError> {
    Circ::new(to3d_ax22d(pos, x.axis())?, x.radius())
}
// ElCLib::To3d(gp_Ax2, gp_Elips2d)
pub fn to3d_elips(pos: &Ax2, x: &Elips2d) -> Result<Elips, ConstructionError> {
    Elips::new(to3d_ax22d(pos, x.axis())?, x.major_radius(), x.minor_radius())
}
// ElCLib::To3d(gp_Ax2, gp_Hypr2d)
pub fn to3d_hypr(pos: &Ax2, x: &Hypr2d) -> Result<Hypr, ConstructionError> {
    Hypr::new(to3d_ax22d(pos, x.axis())?, x.major_radius(), x.minor_radius())
}
// ElCLib::To3d(gp_Ax2, gp_Parab2d)
pub fn to3d_parab(pos: &Ax2, x: &Parab2d) -> Result<Parab, ConstructionError> {
    Parab::new(to3d_ax22d(pos, x.axis())?, x.focal())
}

// ---------------------------------------------------------------------------------------------
// helpers shared with `elslib`

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
    let c = a.crossed(&other);
    let cosinus = a.dot(&other);
    let sinus = (c.x() * c.x() + c.y() * c.y() + c.z() * c.z()).sqrt();
    let ang = if cosinus > -0.70710678118655 && cosinus < 0.70710678118655 {
        libm::acos(cosinus)
    } else if cosinus < 0.0 {
        PI - libm::asin(sinus)
    } else {
        libm::asin(sinus)
    };
    if c.dot(&vref) >= 0.0 { ang } else { -ang }
}

// normalizeAngle (ElCLib.cxx, the same helper is in ElSLib.cxx)
pub(crate) fn normalize(t: &mut f64) {
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
