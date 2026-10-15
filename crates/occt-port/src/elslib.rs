//! OCCT's `ElSLib`: points, derivatives, parameters and iso curves of the elementary surfaces
//! (plane, cylinder, cone, sphere, torus).
//!
//! `ElSLib.cxx` is compiled with `No_Exception`: the checks inside inline `gp` constructors are
//! absent, so the iso functions store a negative radius as it is (`Circ` is built without
//! `Circ::new`). Functions with a `gp` surface (`value_cylinder`, ...) call the `Ax3`-level
//! functions with the surface's frame and radii like `ElSLib.lxx` does.
use crate::elclib::normalize;
use crate::gp::{self, Ax2, Ax3, Circ, Cone, Cylinder, Dir, Lin, Pln, Pnt, Sphere, Torus, Trsf, Vec, XYZ};
use std::f64::consts::{FRAC_PI_2, PI};

/// `(P, Vu, Vv, Vuu, Vvv, Vuv)`, the result of the `D2` functions.
pub type D2Result = (Pnt, Vec, Vec, Vec, Vec, Vec);
/// `(P, Vu, Vv, Vuu, Vvv, Vuv, Vuuu, Vvvv, Vuuv, Vuvv)`, the result of the `D3` functions.
pub type D3Result = (Pnt, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec);

// ---------------------------------------------------------------------------------------------
// helpers

/// The X, Y and main directions and the location of a frame.
struct Frame {
    x: XYZ,
    y: XYZ,
    z: XYZ,
    p: XYZ,
}

fn frame(pos: &Ax3) -> Frame {
    Frame {
        x: *pos.x_direction().xyz(),
        y: *pos.y_direction().xyz(),
        z: *pos.direction().xyz(),
        p: *pos.location().xyz(),
    }
}

// A1 * X1 + A2 * X2 + A3 * X3 + X4
fn lf3p(a1: f64, x1: &XYZ, a2: f64, x2: &XYZ, a3: f64, x3: &XYZ, x4: &XYZ) -> XYZ {
    let mut r = XYZ::zero();
    r.set_linear_form_3_plus(a1, x1, a2, x2, a3, x3, x4);
    r
}
// A1 * X1 + A2 * X2 + A3 * X3
fn lf3(a1: f64, x1: &XYZ, a2: f64, x2: &XYZ, a3: f64, x3: &XYZ) -> XYZ {
    let mut r = XYZ::zero();
    r.set_linear_form_3(a1, x1, a2, x2, a3, x3);
    r
}
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
fn neg(v: &XYZ) -> XYZ {
    v.reversed()
}
fn vec(v: XYZ) -> Vec {
    Vec::from_xyz(v)
}
fn zero() -> Vec {
    Vec::new(0., 0., 0.)
}
// The OCC620 clamp of the torus functions: values within `eps` of zero become 0.
fn clamp(a: f64, eps: f64) -> f64 {
    if a.abs() <= eps { 0. } else { a }
}
// 10 * (MinorRadius + MajorRadius) * RealEpsilon()
fn torus_eps(major: f64, minor: f64) -> f64 {
    10. * (minor + major) * f64::EPSILON
}
// IsOdd / IsEven of Standard_Integer.hxx (C truncation: a negative odd number is neither)
fn is_odd(n: i32) -> bool {
    n % 2 == 1
}
fn is_even(n: i32) -> bool {
    n % 2 == 0
}
// The point P in the local coordinate system of `pos` (gp_Trsf::SetTransformation + Transformed).
fn local(pos: &Ax3, p: &Pnt) -> (f64, f64, f64) {
    let mut t = Trsf::new();
    t.set_transformation(pos);
    let q = p.transformed(&t);
    (q.x(), q.y(), q.z())
}
// `gp_Dir(gp_Vec)` of a vector that is never null here (a unit direction or a combination of
// two orthogonal unit directions), so the zero-length error cannot happen; `fallback` is unused.
fn dir_of(v: &Vec, fallback: &Dir) -> Dir {
    Dir::from_vec(v).unwrap_or(*fallback)
}

// ---------------------------------------------------------------------------------------------
// Value / D0

// ElSLib::PlaneValue
pub fn plane_value(u: f64, v: f64, pos: &Ax3) -> Pnt {
    let f = frame(pos);
    Pnt::from_xyz(lf2p(u, &f.x, v, &f.y, &f.p))
}
// ElSLib::PlaneD0
pub fn plane_d0(u: f64, v: f64, pos: &Ax3) -> Pnt {
    plane_value(u, v, pos)
}
// ElSLib::ConeValue
pub fn cone_value(u: f64, v: f64, pos: &Ax3, radius: f64, semi_angle: f64) -> Pnt {
    let f = frame(pos);
    let r = radius + v * libm::sin(semi_angle);
    let a3 = v * libm::cos(semi_angle);
    let a1 = r * libm::cos(u);
    let a2 = r * libm::sin(u);
    Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, a3, &f.z, &f.p))
}
// ElSLib::ConeD0
pub fn cone_d0(u: f64, v: f64, pos: &Ax3, radius: f64, semi_angle: f64) -> Pnt {
    cone_value(u, v, pos, radius, semi_angle)
}
// ElSLib::CylinderValue
pub fn cylinder_value(u: f64, v: f64, pos: &Ax3, radius: f64) -> Pnt {
    let f = frame(pos);
    let a1 = radius * libm::cos(u);
    let a2 = radius * libm::sin(u);
    Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, v, &f.z, &f.p))
}
// ElSLib::CylinderD0
pub fn cylinder_d0(u: f64, v: f64, pos: &Ax3, radius: f64) -> Pnt {
    cylinder_value(u, v, pos, radius)
}
// ElSLib::SphereValue
pub fn sphere_value(u: f64, v: f64, pos: &Ax3, radius: f64) -> Pnt {
    let f = frame(pos);
    let r = radius * libm::cos(v);
    let a3 = radius * libm::sin(v);
    let a1 = r * libm::cos(u);
    let a2 = r * libm::sin(u);
    Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, a3, &f.z, &f.p))
}
// ElSLib::SphereD0
pub fn sphere_d0(u: f64, v: f64, pos: &Ax3, radius: f64) -> Pnt {
    sphere_value(u, v, pos, radius)
}
// ElSLib::TorusValue
pub fn torus_value(u: f64, v: f64, pos: &Ax3, major_radius: f64, minor_radius: f64) -> Pnt {
    let f = frame(pos);
    let r = major_radius + minor_radius * libm::cos(v);
    let a3 = minor_radius * libm::sin(v);
    let a1 = r * libm::cos(u);
    let a2 = r * libm::sin(u);
    let eps = torus_eps(major_radius, minor_radius);
    let (a1, a2, a3) = (clamp(a1, eps), clamp(a2, eps), clamp(a3, eps));
    Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, a3, &f.z, &f.p))
}
// ElSLib::TorusD0
pub fn torus_d0(u: f64, v: f64, pos: &Ax3, major_radius: f64, minor_radius: f64) -> Pnt {
    torus_value(u, v, pos, major_radius, minor_radius)
}

// ---------------------------------------------------------------------------------------------
// DN

// ElSLib::PlaneDN
pub fn plane_dn(_u: f64, _v: f64, pos: &Ax3, nu: i32, nv: i32) -> Vec {
    if nu == 0 && nv == 1 {
        Vec::from_dir(pos.y_direction())
    } else if nu == 1 && nv == 0 {
        Vec::from_dir(pos.x_direction())
    } else {
        zero()
    }
}
// ElSLib::ConeDN
pub fn cone_dn(u: f64, v: f64, pos: &Ax3, radius: f64, semi_angle: f64, nu: i32, nv: i32) -> Vec {
    let f = frame(pos);
    let mut xdir = f.x;
    let mut ydir = f.y;
    let um = u + f64::from(nu) * FRAC_PI_2;
    xdir.multiply(libm::cos(um));
    ydir.multiply(libm::sin(um));
    xdir.add(&ydir);
    if nv == 0 {
        xdir.multiply(radius + v * libm::sin(semi_angle));
        if nu == 0 {
            xdir.add(&f.p);
        }
        vec(xdir)
    } else if nv == 1 {
        xdir.multiply(libm::sin(semi_angle));
        if nu == 0 {
            xdir.add(&f.z.multiplied(libm::cos(semi_angle)));
        }
        vec(xdir)
    } else {
        zero()
    }
}
// ElSLib::CylinderDN
pub fn cylinder_dn(u: f64, _v: f64, pos: &Ax3, radius: f64, nu: i32, nv: i32) -> Vec {
    if nu + nv < 1 || nu < 0 || nv < 0 {
        return zero();
    }
    if nv == 0 {
        let rcosu = radius * libm::cos(u);
        let rsinu = radius * libm::sin(u);
        let f = frame(pos);
        let mut xdir = f.x;
        let mut ydir = f.y;
        if (nu + 6) % 4 == 0 {
            xdir.multiply(-rcosu);
            ydir.multiply(-rsinu);
        } else if (nu + 5) % 4 == 0 {
            xdir.multiply(rsinu);
            ydir.multiply(-rcosu);
        } else if (nu + 3) % 4 == 0 {
            xdir.multiply(-rsinu);
            ydir.multiply(rcosu);
        } else if nu % 4 == 0 {
            xdir.multiply(rcosu);
            ydir.multiply(rsinu);
        }
        xdir.add(&ydir);
        vec(xdir)
    } else if nv == 1 && nu == 0 {
        Vec::from_dir(pos.direction())
    } else {
        zero()
    }
}
// ElSLib::SphereDN
pub fn sphere_dn(u: f64, v: f64, pos: &Ax3, radius: f64, nu: i32, nv: i32) -> Vec {
    if nu + nv < 1 || nu < 0 || nv < 0 {
        return zero();
    }
    let cosu = libm::cos(u);
    let sinu = libm::sin(u);
    let rcosv = radius * libm::cos(v);
    let f = frame(pos);
    let mut r;
    let negate;
    if nu == 0 {
        let rsinv = radius * libm::sin(v);
        let (a1, a2, a3) =
            if is_odd(nv) { (-rsinv * cosu, -rsinv * sinu, rcosv) } else { (-rcosv * cosu, -rcosv * sinu, -rsinv) };
        r = lf3(a1, &f.x, a2, &f.y, a3, &f.z);
        negate = (nv + 2) % 4 != 0 && (nv + 3) % 4 != 0;
    } else if nv == 0 {
        let (a1, a2) = if is_odd(nu) { (-rcosv * sinu, rcosv * cosu) } else { (rcosv * cosu, rcosv * sinu) };
        r = lf2(a1, &f.x, a2, &f.y);
        negate = (nu + 2) % 4 == 0 || (nu + 1) % 4 == 0;
    } else {
        let rsinv = radius * libm::sin(v);
        let (a1, a2) = if is_odd(nu) { (-sinu, cosu) } else { (-cosu, -sinu) };
        let a3 = if is_odd(nv) { -rsinv } else { -rcosv };
        r = lf2(a1, &f.x, a2, &f.y).multiplied(a3);
        negate = ((nu + 2) % 4 != 0 && (nu + 3) % 4 != 0 && ((nv + 2) % 4 == 0 || (nv + 3) % 4 == 0))
            || (((nu + 2) % 4 == 0 || (nu + 3) % 4 == 0) && (nv + 2) % 4 != 0 && (nv + 3) % 4 != 0);
    }
    if negate {
        r = neg(&r);
    }
    vec(r)
}
// ElSLib::TorusDN
pub fn torus_dn(u: f64, v: f64, pos: &Ax3, major_radius: f64, minor_radius: f64, nu: i32, nv: i32) -> Vec {
    if nu + nv < 1 || nu < 0 || nv < 0 {
        return zero();
    }
    let cosu = libm::cos(u);
    let sinu = libm::sin(u);
    let f = frame(pos);
    let eps = torus_eps(major_radius, minor_radius);
    let mut r = XYZ::zero();
    if nv == 0 {
        let rr = major_radius + minor_radius * libm::cos(v);
        let (a1, a2) = if is_odd(nu) { (-rr * sinu, rr * cosu) } else { (-rr * cosu, -rr * sinu) };
        r = lf2(clamp(a1, eps), &f.x, clamp(a2, eps), &f.y);
        if (nu + 2) % 4 != 0 && (nu + 3) % 4 != 0 {
            r = neg(&r);
        }
    } else if nu == 0 {
        let rcosv = minor_radius * libm::cos(v);
        let rsinv = minor_radius * libm::sin(v);
        let (a1, a2, a3) =
            if is_odd(nv) { (-rsinv * cosu, -rsinv * sinu, rcosv) } else { (-rcosv * cosu, -rcosv * sinu, -rsinv) };
        r = lf3(clamp(a1, eps), &f.x, clamp(a2, eps), &f.y, clamp(a3, eps), &f.z);
        if (nv + 2) % 4 != 0 && (nv + 3) % 4 != 0 {
            r = neg(&r);
        }
    } else if is_odd(nu) && is_odd(nv) {
        let rsinv = minor_radius * libm::sin(v);
        let (a1, a2) = (rsinv * sinu, -rsinv * cosu);
        r = lf2(clamp(a1, eps), &f.x, clamp(a2, eps), &f.y);
    } else if is_even(nu) && is_even(nv) {
        let rcosv = minor_radius * libm::cos(v);
        let (a1, a2) = (rcosv * cosu, rcosv * sinu);
        r = lf2(clamp(a1, eps), &f.x, clamp(a2, eps), &f.y);
    } else if is_even(nv) && is_odd(nu) {
        let rcosv = minor_radius * libm::cos(v);
        let (a1, a2) = (rcosv * sinu, -rcosv * cosu);
        r = lf2(clamp(a1, eps), &f.x, clamp(a2, eps), &f.y);
        if (nv + nu + 3) % 4 == 0 {
            r = neg(&r);
        }
    } else if is_odd(nv) && is_even(nu) {
        let rsinv = minor_radius * libm::sin(v);
        let (a1, a2) = (rsinv * cosu, rsinv * sinu);
        r = lf2(clamp(a1, eps), &f.x, clamp(a2, eps), &f.y);
        if (nu + nv + 3) % 4 == 0 {
            r = neg(&r);
        }
    }
    vec(r)
}

// ---------------------------------------------------------------------------------------------
// D1

// ElSLib::PlaneD1
pub fn plane_d1(u: f64, v: f64, pos: &Ax3) -> (Pnt, Vec, Vec) {
    let f = frame(pos);
    (Pnt::from_xyz(lf2p(u, &f.x, v, &f.y, &f.p)), vec(f.x), vec(f.y))
}
// ElSLib::ConeD1
pub fn cone_d1(u: f64, v: f64, pos: &Ax3, radius: f64, semi_angle: f64) -> (Pnt, Vec, Vec) {
    let f = frame(pos);
    let cosu = libm::cos(u);
    let sinu = libm::sin(u);
    let cosa = libm::cos(semi_angle);
    let sina = libm::sin(semi_angle);
    let r = radius + v * sina;
    let a3 = v * cosa;
    let a1 = r * cosu;
    let a2 = r * sinu;
    let r1 = sina * cosu;
    let r2 = sina * sinu;
    (
        Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, a3, &f.z, &f.p)),
        vec(lf2(-a2, &f.x, a1, &f.y)),
        vec(lf3(r1, &f.x, r2, &f.y, cosa, &f.z)),
    )
}
// ElSLib::CylinderD1
pub fn cylinder_d1(u: f64, v: f64, pos: &Ax3, radius: f64) -> (Pnt, Vec, Vec) {
    let f = frame(pos);
    let a1 = radius * libm::cos(u);
    let a2 = radius * libm::sin(u);
    (Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, v, &f.z, &f.p)), vec(lf2(-a2, &f.x, a1, &f.y)), vec(f.z))
}
// ElSLib::SphereD1
pub fn sphere_d1(u: f64, v: f64, pos: &Ax3, radius: f64) -> (Pnt, Vec, Vec) {
    let f = frame(pos);
    let cosu = libm::cos(u);
    let sinu = libm::sin(u);
    let r1 = radius * libm::cos(v);
    let r2 = radius * libm::sin(v);
    let a1 = r1 * cosu;
    let a2 = r1 * sinu;
    let a3 = r2 * cosu;
    let a4 = r2 * sinu;
    (
        Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, r2, &f.z, &f.p)),
        vec(lf2(-a2, &f.x, a1, &f.y)),
        vec(lf3(-a3, &f.x, -a4, &f.y, r1, &f.z)),
    )
}
// ElSLib::TorusD1
pub fn torus_d1(u: f64, v: f64, pos: &Ax3, major_radius: f64, minor_radius: f64) -> (Pnt, Vec, Vec) {
    let f = frame(pos);
    let cosu = libm::cos(u);
    let sinu = libm::sin(u);
    let r1 = minor_radius * libm::cos(v);
    let r2 = minor_radius * libm::sin(v);
    let r = major_radius + r1;
    let eps = torus_eps(major_radius, minor_radius);
    let a1 = clamp(r * cosu, eps);
    let a2 = clamp(r * sinu, eps);
    let a3 = clamp(r2 * cosu, eps);
    let a4 = clamp(r2 * sinu, eps);
    (
        Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, r2, &f.z, &f.p)),
        vec(lf2(-a2, &f.x, a1, &f.y)),
        vec(lf3(-a3, &f.x, -a4, &f.y, r1, &f.z)),
    )
}

// ---------------------------------------------------------------------------------------------
// D2

// ElSLib::ConeD2
pub fn cone_d2(u: f64, v: f64, pos: &Ax3, radius: f64, semi_angle: f64) -> D2Result {
    let f = frame(pos);
    let cosu = libm::cos(u);
    let sinu = libm::sin(u);
    let cosa = libm::cos(semi_angle);
    let sina = libm::sin(semi_angle);
    let r = radius + v * sina;
    let a3 = v * cosa;
    let a1 = r * cosu;
    let a2 = r * sinu;
    let r1 = sina * cosu;
    let r2 = sina * sinu;
    let som1 = lf2(a1, &f.x, a2, &f.y);
    (
        Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, a3, &f.z, &f.p)),
        vec(lf2(-a2, &f.x, a1, &f.y)),
        vec(lf3(r1, &f.x, r2, &f.y, cosa, &f.z)),
        vec(neg(&som1)),
        zero(),
        vec(lf2(-r2, &f.x, r1, &f.y)),
    )
}
// ElSLib::CylinderD2
pub fn cylinder_d2(u: f64, v: f64, pos: &Ax3, radius: f64) -> D2Result {
    let f = frame(pos);
    let a1 = radius * libm::cos(u);
    let a2 = radius * libm::sin(u);
    let som1 = lf2(a1, &f.x, a2, &f.y);
    (
        Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, v, &f.z, &f.p)),
        vec(lf2(-a2, &f.x, a1, &f.y)),
        vec(f.z),
        vec(neg(&som1)),
        zero(),
        zero(),
    )
}
// ElSLib::SphereD2
pub fn sphere_d2(u: f64, v: f64, pos: &Ax3, radius: f64) -> D2Result {
    let f = frame(pos);
    let cosu = libm::cos(u);
    let sinu = libm::sin(u);
    let r1 = radius * libm::cos(v);
    let r2 = radius * libm::sin(v);
    let a1 = r1 * cosu;
    let a2 = r1 * sinu;
    let a3 = r2 * cosu;
    let a4 = r2 * sinu;
    let som1 = lf2(a1, &f.x, a2, &f.y);
    let r2z = f.z.multiplied(r2);
    (
        Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, r2, &f.z, &f.p)),
        vec(lf2(-a2, &f.x, a1, &f.y)),
        vec(lf3(-a3, &f.x, -a4, &f.y, r1, &f.z)),
        vec(neg(&som1)),
        vec(neg(&som1).subtracted(&r2z)),
        vec(lf2(a4, &f.x, -a3, &f.y)),
    )
}
// ElSLib::TorusD2
pub fn torus_d2(u: f64, v: f64, pos: &Ax3, major_radius: f64, minor_radius: f64) -> D2Result {
    let f = frame(pos);
    let cosu = libm::cos(u);
    let sinu = libm::sin(u);
    let r1 = minor_radius * libm::cos(v);
    let r2 = minor_radius * libm::sin(v);
    let r = major_radius + r1;
    let eps = torus_eps(major_radius, minor_radius);
    let a1 = clamp(r * cosu, eps);
    let a2 = clamp(r * sinu, eps);
    let a3 = clamp(r2 * cosu, eps);
    let a4 = clamp(r2 * sinu, eps);
    let a5 = clamp(r1 * cosu, eps);
    let a6 = clamp(r1 * sinu, eps);
    let som1 = lf2(a1, &f.x, a2, &f.y);
    let r2z = f.z.multiplied(r2);
    (
        Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, r2, &f.z, &f.p)),
        vec(lf2(-a2, &f.x, a1, &f.y)),
        vec(lf3(-a3, &f.x, -a4, &f.y, r1, &f.z)),
        vec(neg(&som1)),
        vec(lf2(-a5, &f.x, -a6, &f.y).subtracted(&r2z)),
        vec(lf2(a4, &f.x, -a3, &f.y)),
    )
}

// ---------------------------------------------------------------------------------------------
// D3

// ElSLib::ConeD3
pub fn cone_d3(u: f64, v: f64, pos: &Ax3, radius: f64, semi_angle: f64) -> D3Result {
    let f = frame(pos);
    let cosu = libm::cos(u);
    let sinu = libm::sin(u);
    let cosa = libm::cos(semi_angle);
    let sina = libm::sin(semi_angle);
    let r = radius + v * sina;
    let a3 = v * cosa;
    let a1 = r * cosu;
    let a2 = r * sinu;
    let r1 = sina * cosu;
    let r2 = sina * sinu;
    let som1 = lf2(a1, &f.x, a2, &f.y);
    let som2 = lf2(r1, &f.x, r2, &f.y);
    let dif1 = lf2(a2, &f.x, -a1, &f.y);
    (
        Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, a3, &f.z, &f.p)),
        vec(neg(&dif1)),
        vec(som2.added(&f.z.multiplied(cosa))),
        vec(neg(&som1)),
        zero(),
        vec(lf2(-r2, &f.x, r1, &f.y)),
        vec(dif1),
        zero(),
        vec(neg(&som2)),
        zero(),
    )
}
// ElSLib::CylinderD3
pub fn cylinder_d3(u: f64, v: f64, pos: &Ax3, radius: f64) -> D3Result {
    let f = frame(pos);
    let a1 = radius * libm::cos(u);
    let a2 = radius * libm::sin(u);
    let som1 = lf2(a1, &f.x, a2, &f.y);
    let dif1 = lf2(a2, &f.x, -a1, &f.y);
    (
        Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, v, &f.z, &f.p)),
        vec(neg(&dif1)),
        vec(f.z),
        vec(neg(&som1)),
        zero(),
        zero(),
        vec(dif1),
        zero(),
        zero(),
        zero(),
    )
}
// ElSLib::SphereD3
pub fn sphere_d3(u: f64, v: f64, pos: &Ax3, radius: f64) -> D3Result {
    let f = frame(pos);
    let cosu = libm::cos(u);
    let sinu = libm::sin(u);
    let r1 = radius * libm::cos(v);
    let r2 = radius * libm::sin(v);
    let a1 = r1 * cosu;
    let a2 = r1 * sinu;
    let a3 = r2 * cosu;
    let a4 = r2 * sinu;
    let som1 = lf2(a1, &f.x, a2, &f.y);
    let som3 = lf2(a3, &f.x, a4, &f.y);
    let dif1 = lf2(a2, &f.x, -a1, &f.y);
    let r1z = f.z.multiplied(r1);
    let r2z = f.z.multiplied(r2);
    (
        Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, r2, &f.z, &f.p)),
        vec(neg(&dif1)),
        vec(neg(&som3).added(&r1z)),
        vec(neg(&som1)),
        vec(neg(&som1).subtracted(&r2z)),
        vec(lf2(a4, &f.x, -a3, &f.y)),
        vec(dif1),
        vec(som3.subtracted(&r1z)),
        vec(som3),
        vec(dif1),
    )
}
// ElSLib::TorusD3
pub fn torus_d3(u: f64, v: f64, pos: &Ax3, major_radius: f64, minor_radius: f64) -> D3Result {
    let f = frame(pos);
    let cosu = libm::cos(u);
    let sinu = libm::sin(u);
    let r1 = minor_radius * libm::cos(v);
    let r2 = minor_radius * libm::sin(v);
    let r = major_radius + r1;
    let eps = torus_eps(major_radius, minor_radius);
    let a1 = clamp(r * cosu, eps);
    let a2 = clamp(r * sinu, eps);
    let a3 = clamp(r2 * cosu, eps);
    let a4 = clamp(r2 * sinu, eps);
    let a5 = clamp(r1 * cosu, eps);
    let a6 = clamp(r1 * sinu, eps);
    let som1 = lf2(a1, &f.x, a2, &f.y);
    let som3 = lf2(a3, &f.x, a4, &f.y);
    let dif1 = lf2(a2, &f.x, -a1, &f.y);
    let r1z = f.z.multiplied(r1);
    let r2z = f.z.multiplied(r2);
    (
        Pnt::from_xyz(lf3p(a1, &f.x, a2, &f.y, r2, &f.z, &f.p)),
        vec(neg(&dif1)),
        vec(neg(&som3).added(&r1z)),
        vec(neg(&som1)),
        vec(lf2(-a5, &f.x, -a6, &f.y).subtracted(&r2z)),
        vec(lf2(a4, &f.x, -a3, &f.y)),
        vec(dif1),
        vec(som3.subtracted(&r1z)),
        vec(som3),
        vec(lf2(a6, &f.x, -a5, &f.y)),
    )
}

// ---------------------------------------------------------------------------------------------
// Parameters

// ElSLib::PlaneParameters
pub fn plane_parameters(pos: &Ax3, p: &Pnt) -> (f64, f64) {
    let (x, y, _) = local(pos, p);
    (x, y)
}
// ElSLib::CylinderParameters
pub fn cylinder_parameters(pos: &Ax3, _radius: f64, p: &Pnt) -> (f64, f64) {
    let (x, y, z) = local(pos, p);
    let mut u = libm::atan2(y, x);
    normalize(&mut u);
    (u, z)
}
// ElSLib::ConeParameters
pub fn cone_parameters(pos: &Ax3, radius: f64, semi_angle: f64, p: &Pnt) -> (f64, f64) {
    let (x, y, z) = local(pos, p);
    let mut u = if x.abs() < gp::resolution() && y.abs() < gp::resolution() {
        0.0
    } else if -radius > z * libm::tan(semi_angle) {
        libm::atan2(-y, -x)
    } else {
        libm::atan2(y, x)
    };
    normalize(&mut u);
    let v = libm::sin(semi_angle) * (x * libm::cos(u) + y * libm::sin(u) - radius) + libm::cos(semi_angle) * z;
    (u, v)
}
// ElSLib::SphereParameters
pub fn sphere_parameters(pos: &Ax3, _radius: f64, p: &Pnt) -> (f64, f64) {
    let (x, y, z) = local(pos, p);
    let l = (x * x + y * y).sqrt();
    if l < gp::resolution() {
        // point on the axis Z of the sphere
        (0., if z > 0. { FRAC_PI_2 } else { -FRAC_PI_2 })
    } else {
        let v = libm::atan(z / l);
        let mut u = libm::atan2(y, x);
        normalize(&mut u);
        (u, v)
    }
}
// ElSLib::TorusParameters
pub fn torus_parameters(pos: &Ax3, major_radius: f64, minor_radius: f64, p: &Pnt) -> (f64, f64) {
    let (x, y, z) = local(pos, p);
    let mut u = libm::atan2(y, x);
    if major_radius < minor_radius {
        let cosu = libm::cos(u);
        let sinu = libm::sin(u);
        let z2 = z * z;
        let min_r2 = minor_radius * minor_radius;
        let rcosu = major_radius * cosu;
        let rsinu = major_radius * sinu;
        let xm = x - rcosu;
        let ym = y - rsinu;
        let xp = x + rcosu;
        let yp = y + rsinu;
        let d1 = xm * xm + ym * ym + z2 - min_r2;
        let d2 = xp * xp + yp * yp + z2 - min_r2;
        if d2.abs() < d1.abs() {
            u += PI;
        }
    }
    normalize(&mut u);
    let cosu = libm::cos(u);
    let sinu = libm::sin(u);
    let d_pv = gp::xyz(x - major_radius * cosu, y - major_radius * sinu, z);
    let mut v = if d_pv.modulus() <= gp::resolution() {
        0.
    } else {
        // gp_Dir dx(cosu, sinu, 0), gp_Dir dP(dPV): neither can be null here
        match (Dir::new(cosu, sinu, 0.), Dir::from_xyz(d_pv)) {
            (Ok(dx), Ok(d_p)) => match dx.crossed(&gp::dz()) {
                Ok(vref) => dx.angle_with_ref(&d_p, &vref),
                Err(_) => 0.,
            },
            _ => 0.,
        }
    };
    normalize(&mut v);
    (u, v)
}

// ---------------------------------------------------------------------------------------------
// Iso curves

// ElSLib::PlaneUIso
pub fn plane_uiso(pos: &Ax3, u: f64) -> Lin {
    let mut l = Lin::new(*pos.location(), *pos.y_direction());
    let mut ve = Vec::from_dir(pos.x_direction());
    ve.multiply(u);
    l.translate(&ve);
    l
}
// ElSLib::CylinderUIso
pub fn cylinder_uiso(pos: &Ax3, radius: f64, u: f64) -> Lin {
    let (p, _, dv) = cylinder_d1(u, 0., pos, radius);
    Lin::new(p, dir_of(&dv, pos.direction()))
}
// ElSLib::ConeUIso
pub fn cone_uiso(pos: &Ax3, radius: f64, semi_angle: f64, u: f64) -> Lin {
    let (p, _, dv) = cone_d1(u, 0., pos, radius, semi_angle);
    Lin::new(p, dir_of(&dv, pos.direction()))
}
// The frame `gp_Ax2(Pos.Location(), cx ^ dz, cx)` of the U iso circles of the sphere and the torus,
// with `cx = cos(U) * XDirection + sin(U) * YDirection` (normalized): `cx` is a unit direction
// normal to `dz`, so no step can fail; the fallbacks are unreachable.
fn u_iso_frame(pos: &Ax3, u: f64) -> (Ax2, Dir) {
    let dx = Vec::from_dir(pos.x_direction());
    let dy = Vec::from_dir(pos.y_direction());
    let cx = dir_of(&dx.multiplied(libm::cos(u)).added(&dy.multiplied(libm::sin(u))), pos.x_direction());
    let axes =
        cx.crossed(pos.direction()).and_then(|n| Ax2::new(*pos.location(), n, &cx)).unwrap_or_else(|_| pos.ax2());
    (axes, cx)
}
// ElSLib::SphereUIso
pub fn sphere_uiso(pos: &Ax3, radius: f64, u: f64) -> Circ {
    let (axes, _) = u_iso_frame(pos, u);
    Circ { a: axes, r: radius }
}
// ElSLib::TorusUIso
pub fn torus_uiso(pos: &Ax3, major_radius: f64, minor_radius: f64, u: f64) -> Circ {
    let (mut axes, cx) = u_iso_frame(pos, u);
    let mut ve = Vec::from_dir(&cx);
    ve.multiply(major_radius);
    axes.translate(&ve);
    Circ { a: axes, r: minor_radius }
}
// ElSLib::PlaneVIso
pub fn plane_viso(pos: &Ax3, v: f64) -> Lin {
    let mut l = Lin::new(*pos.location(), *pos.x_direction());
    let mut ve = Vec::from_dir(pos.y_direction());
    ve.multiply(v);
    l.translate(&ve);
    l
}
// ElSLib::CylinderVIso
pub fn cylinder_viso(pos: &Ax3, radius: f64, v: f64) -> Circ {
    let mut axes = pos.ax2();
    let mut ve = Vec::from_dir(pos.direction());
    ve.multiply(v);
    axes.translate(&ve);
    Circ { a: axes, r: radius }
}
// ElSLib::ConeVIso
pub fn cone_viso(pos: &Ax3, radius: f64, semi_angle: f64, v: f64) -> Circ {
    let mut axes = *pos;
    let mut ve = Vec::from_dir(pos.direction());
    ve.multiply(v * libm::cos(semi_angle));
    axes.translate(&ve);
    let mut r = radius + v * libm::sin(semi_angle);
    if r < 0. {
        axes.x_reverse();
        axes.y_reverse();
        r = -r;
    }
    Circ { a: axes.ax2(), r }
}
// ElSLib::SphereVIso
pub fn sphere_viso(pos: &Ax3, radius: f64, v: f64) -> Circ {
    let mut axes = pos.ax2();
    let mut ve = Vec::from_dir(pos.direction());
    ve.multiply(radius * libm::sin(v));
    axes.translate(&ve);
    let mut r = radius * libm::cos(v);
    if r < 0. {
        // Cannot fail: the reversed main direction is still normal to the X direction.
        let _ = axes.set_direction(axes.direction().reversed());
        r = -r;
    }
    Circ { a: axes, r }
}
// ElSLib::TorusVIso
pub fn torus_viso(pos: &Ax3, major_radius: f64, minor_radius: f64, v: f64) -> Circ {
    let mut axes = Ax3::from_ax2(&pos.ax2());
    let mut ve = Vec::from_dir(pos.direction());
    ve.multiply(minor_radius * libm::sin(v));
    axes.translate(&ve);
    let mut r = major_radius + minor_radius * libm::cos(v);
    if r < 0. {
        axes.x_reverse();
        axes.y_reverse();
        r = -r;
    }
    Circ { a: axes.ax2(), r }
}

// ---------------------------------------------------------------------------------------------
// The overloads taking a gp surface

// ElSLib::Value(.., gp_Pln)
pub fn value_pln(u: f64, v: f64, s: &Pln) -> Pnt {
    plane_value(u, v, s.position())
}
// ElSLib::D0(.., gp_Pln)
pub fn d0_pln(u: f64, v: f64, s: &Pln) -> Pnt {
    plane_d0(u, v, s.position())
}
// ElSLib::D1(.., gp_Pln)
pub fn d1_pln(u: f64, v: f64, s: &Pln) -> (Pnt, Vec, Vec) {
    plane_d1(u, v, s.position())
}
// ElSLib::DN(.., gp_Pln)
pub fn dn_pln(u: f64, v: f64, s: &Pln, nu: i32, nv: i32) -> Vec {
    plane_dn(u, v, s.position(), nu, nv)
}
// ElSLib::Parameters(.., gp_Pln)
pub fn parameters_pln(s: &Pln, p: &Pnt) -> (f64, f64) {
    plane_parameters(s.position(), p)
}
// ElSLib::Value(.., gp_Cylinder)
pub fn value_cylinder(u: f64, v: f64, s: &Cylinder) -> Pnt {
    cylinder_value(u, v, s.position(), s.radius())
}
// ElSLib::D0(.., gp_Cylinder)
pub fn d0_cylinder(u: f64, v: f64, s: &Cylinder) -> Pnt {
    cylinder_d0(u, v, s.position(), s.radius())
}
// ElSLib::D1(.., gp_Cylinder)
pub fn d1_cylinder(u: f64, v: f64, s: &Cylinder) -> (Pnt, Vec, Vec) {
    cylinder_d1(u, v, s.position(), s.radius())
}
// ElSLib::D2(.., gp_Cylinder)
pub fn d2_cylinder(u: f64, v: f64, s: &Cylinder) -> D2Result {
    cylinder_d2(u, v, s.position(), s.radius())
}
// ElSLib::D3(.., gp_Cylinder)
pub fn d3_cylinder(u: f64, v: f64, s: &Cylinder) -> D3Result {
    cylinder_d3(u, v, s.position(), s.radius())
}
// ElSLib::DN(.., gp_Cylinder)
pub fn dn_cylinder(u: f64, v: f64, s: &Cylinder, nu: i32, nv: i32) -> Vec {
    cylinder_dn(u, v, s.position(), s.radius(), nu, nv)
}
// ElSLib::Parameters(.., gp_Cylinder)
pub fn parameters_cylinder(s: &Cylinder, p: &Pnt) -> (f64, f64) {
    cylinder_parameters(s.position(), s.radius(), p)
}
// ElSLib::Value(.., gp_Cone)
pub fn value_cone(u: f64, v: f64, s: &Cone) -> Pnt {
    cone_value(u, v, s.position(), s.ref_radius(), s.semi_angle())
}
// ElSLib::D0(.., gp_Cone)
pub fn d0_cone(u: f64, v: f64, s: &Cone) -> Pnt {
    cone_d0(u, v, s.position(), s.ref_radius(), s.semi_angle())
}
// ElSLib::D1(.., gp_Cone)
pub fn d1_cone(u: f64, v: f64, s: &Cone) -> (Pnt, Vec, Vec) {
    cone_d1(u, v, s.position(), s.ref_radius(), s.semi_angle())
}
// ElSLib::D2(.., gp_Cone)
pub fn d2_cone(u: f64, v: f64, s: &Cone) -> D2Result {
    cone_d2(u, v, s.position(), s.ref_radius(), s.semi_angle())
}
// ElSLib::D3(.., gp_Cone)
pub fn d3_cone(u: f64, v: f64, s: &Cone) -> D3Result {
    cone_d3(u, v, s.position(), s.ref_radius(), s.semi_angle())
}
// ElSLib::DN(.., gp_Cone)
pub fn dn_cone(u: f64, v: f64, s: &Cone, nu: i32, nv: i32) -> Vec {
    cone_dn(u, v, s.position(), s.ref_radius(), s.semi_angle(), nu, nv)
}
// ElSLib::Parameters(.., gp_Cone)
pub fn parameters_cone(s: &Cone, p: &Pnt) -> (f64, f64) {
    cone_parameters(s.position(), s.ref_radius(), s.semi_angle(), p)
}
// ElSLib::Value(.., gp_Sphere)
pub fn value_sphere(u: f64, v: f64, s: &Sphere) -> Pnt {
    sphere_value(u, v, s.position(), s.radius())
}
// ElSLib::D0(.., gp_Sphere)
pub fn d0_sphere(u: f64, v: f64, s: &Sphere) -> Pnt {
    sphere_d0(u, v, s.position(), s.radius())
}
// ElSLib::D1(.., gp_Sphere)
pub fn d1_sphere(u: f64, v: f64, s: &Sphere) -> (Pnt, Vec, Vec) {
    sphere_d1(u, v, s.position(), s.radius())
}
// ElSLib::D2(.., gp_Sphere)
pub fn d2_sphere(u: f64, v: f64, s: &Sphere) -> D2Result {
    sphere_d2(u, v, s.position(), s.radius())
}
// ElSLib::D3(.., gp_Sphere)
pub fn d3_sphere(u: f64, v: f64, s: &Sphere) -> D3Result {
    sphere_d3(u, v, s.position(), s.radius())
}
// ElSLib::DN(.., gp_Sphere)
pub fn dn_sphere(u: f64, v: f64, s: &Sphere, nu: i32, nv: i32) -> Vec {
    sphere_dn(u, v, s.position(), s.radius(), nu, nv)
}
// ElSLib::Parameters(.., gp_Sphere)
pub fn parameters_sphere(s: &Sphere, p: &Pnt) -> (f64, f64) {
    sphere_parameters(s.position(), s.radius(), p)
}
// ElSLib::Value(.., gp_Torus)
pub fn value_torus(u: f64, v: f64, s: &Torus) -> Pnt {
    torus_value(u, v, s.position(), s.major_radius(), s.minor_radius())
}
// ElSLib::D0(.., gp_Torus)
pub fn d0_torus(u: f64, v: f64, s: &Torus) -> Pnt {
    torus_d0(u, v, s.position(), s.major_radius(), s.minor_radius())
}
// ElSLib::D1(.., gp_Torus)
pub fn d1_torus(u: f64, v: f64, s: &Torus) -> (Pnt, Vec, Vec) {
    torus_d1(u, v, s.position(), s.major_radius(), s.minor_radius())
}
// ElSLib::D2(.., gp_Torus)
pub fn d2_torus(u: f64, v: f64, s: &Torus) -> D2Result {
    torus_d2(u, v, s.position(), s.major_radius(), s.minor_radius())
}
// ElSLib::D3(.., gp_Torus)
pub fn d3_torus(u: f64, v: f64, s: &Torus) -> D3Result {
    torus_d3(u, v, s.position(), s.major_radius(), s.minor_radius())
}
// ElSLib::DN(.., gp_Torus)
pub fn dn_torus(u: f64, v: f64, s: &Torus, nu: i32, nv: i32) -> Vec {
    torus_dn(u, v, s.position(), s.major_radius(), s.minor_radius(), nu, nv)
}
// ElSLib::Parameters(.., gp_Torus)
pub fn parameters_torus(s: &Torus, p: &Pnt) -> (f64, f64) {
    torus_parameters(s.position(), s.major_radius(), s.minor_radius(), p)
}
