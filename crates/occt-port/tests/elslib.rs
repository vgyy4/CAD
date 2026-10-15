//! Numerical reference test for `elslib` (all of OCCT 8.0.1's ElSLib) and the second part of `elclib`
//! (3D `Value`/`D1`/`D2`, the `Ax1`/`Ax2`-based overloads, every 2D function, `To3d`).
//!
//! `elslib.txt` is printed by `oracle/elslib.cpp` (real OCCT). Each line is
//! `<family> <inputs...> | <outputs...>`: f64 values are 16-hex-digit bit patterns and ints are decimal.
//! Every input of a call is on the left of ` | `; this test reads them back with the `Rd` reader methods
//! of the same name as the oracle's schema words (`f i pnt pnt2d vec2d dir2d ax1 ax2 ax3 ax2d ax22d lin
//! circ elips hypr parab lin2d circ2d elips2d hypr2d parab2d pln cylinder cone sphere torus`) and replays
//! the call on the Rust API (docs/api-elslib.md). The right side is either the expected values, `throws`
//! (OCCT raised) or `unchecked` (a check that OCCT 8.0.1 as built has compiled out would have failed and
//! the numbers are unspecified); on both the Rust API must return an error.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::gp::{
    self, Ax1, Ax2, Ax2d, Ax3, Ax22d, Circ, Circ2d, Cone, Cylinder, Dir, Dir2d, Elips, Elips2d, Hypr, Hypr2d, Lin,
    Lin2d, Parab, Parab2d, Pln, Pnt, Pnt2d, Sphere, Torus, Vec as GpVec, Vec2d,
};
use occt_port::{elclib, elslib};
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for sin/cos/atan2/acos/asin/sinh/cosh/asinh dependent results, in units of
/// f64::EPSILON times the largest absolute value on the line (at least 1).
const MAX_ULPS: f64 = 8.0;

/// Every family must have at least this many distinct lines (distinct inputs).
const MIN_DISTINCT: usize = 16;

/// Families that really take no input: they are printed exactly once. There are none here.
const INPUT_LESS: &[&str] = &[];

/// Families whose results use only + - * / and sqrt: compared bit for bit (signed zeros included). All
/// other families depend on sin/cos/tan/atan2/acos/asin/atan/sinh/cosh/asinh somewhere and are compared
/// with MAX_ULPS.
const EXACT: &[&str] = &[
    "elclib_lin2d_dn",
    "elclib_lin2d_eval",
    "elclib_lin2d_parameter",
    "elclib_lin_dn",
    "elclib_lin_eval",
    "elclib_lin_parameter",
    "elclib_parab2d_dn",
    "elclib_parab2d_eval",
    "elclib_parab2d_parameter",
    "elclib_parab_dn",
    "elclib_parab_eval",
    "elclib_parab_parameter",
    "elclib_to3d_ax22d",
    "elclib_to3d_ax2d",
    "elclib_to3d_circ",
    "elclib_to3d_dir",
    "elclib_to3d_elips",
    "elclib_to3d_hypr",
    "elclib_to3d_lin",
    "elclib_to3d_parab",
    "elclib_to3d_pnt",
    "elclib_to3d_vec",
    "elslib_cylinder_viso",
    "elslib_pln_d1",
    "elslib_pln_dn",
    "elslib_pln_object",
    "elslib_pln_parameters",
    "elslib_pln_uiso",
    "elslib_pln_value",
    "elslib_pln_viso",
];

/// Every family the oracle must print (a family that disappears from elslib.txt fails).
const FAMILIES: &[&str] = &[
    "elclib_circ2d_dn",
    "elclib_circ2d_eval",
    "elclib_circ2d_parameter",
    "elclib_circ_dn",
    "elclib_circ_eval",
    "elclib_circ_parameter",
    "elclib_elips2d_dn",
    "elclib_elips2d_eval",
    "elclib_elips2d_parameter",
    "elclib_elips_dn",
    "elclib_elips_eval",
    "elclib_elips_parameter",
    "elclib_hypr2d_dn",
    "elclib_hypr2d_eval",
    "elclib_hypr2d_parameter",
    "elclib_hypr_dn",
    "elclib_hypr_eval",
    "elclib_hypr_parameter",
    "elclib_lin2d_dn",
    "elclib_lin2d_eval",
    "elclib_lin2d_parameter",
    "elclib_lin_dn",
    "elclib_lin_eval",
    "elclib_lin_parameter",
    "elclib_parab2d_dn",
    "elclib_parab2d_eval",
    "elclib_parab2d_parameter",
    "elclib_parab_dn",
    "elclib_parab_eval",
    "elclib_parab_parameter",
    "elclib_to3d_ax22d",
    "elclib_to3d_ax2d",
    "elclib_to3d_circ",
    "elclib_to3d_dir",
    "elclib_to3d_elips",
    "elclib_to3d_hypr",
    "elclib_to3d_lin",
    "elclib_to3d_parab",
    "elclib_to3d_pnt",
    "elclib_to3d_vec",
    "elslib_cone_d1",
    "elslib_cone_d2",
    "elslib_cone_d3",
    "elslib_cone_dn",
    "elslib_cone_object",
    "elslib_cone_parameters",
    "elslib_cone_uiso",
    "elslib_cone_value",
    "elslib_cone_viso",
    "elslib_cylinder_d1",
    "elslib_cylinder_d2",
    "elslib_cylinder_d3",
    "elslib_cylinder_dn",
    "elslib_cylinder_object",
    "elslib_cylinder_parameters",
    "elslib_cylinder_uiso",
    "elslib_cylinder_value",
    "elslib_cylinder_viso",
    "elslib_pln_d1",
    "elslib_pln_dn",
    "elslib_pln_object",
    "elslib_pln_parameters",
    "elslib_pln_uiso",
    "elslib_pln_value",
    "elslib_pln_viso",
    "elslib_sphere_d1",
    "elslib_sphere_d2",
    "elslib_sphere_d3",
    "elslib_sphere_dn",
    "elslib_sphere_object",
    "elslib_sphere_parameters",
    "elslib_sphere_uiso",
    "elslib_sphere_value",
    "elslib_sphere_viso",
    "elslib_torus_d1",
    "elslib_torus_d2",
    "elslib_torus_d3",
    "elslib_torus_dn",
    "elslib_torus_object",
    "elslib_torus_parameters",
    "elslib_torus_uiso",
    "elslib_torus_value",
    "elslib_torus_viso",
];

/// The number of lines of the golden file.
const GOLDEN_LINES: usize = 2739;

// ---------------------------------------------------------------------------------
// Results of one replayed call

/// The Rust API returned an error (or the call is known to be invalid).
#[derive(Debug)]
struct Thrown;

impl From<gp::ConstructionError> for Thrown {
    fn from(_: gp::ConstructionError) -> Self {
        Thrown
    }
}

/// Collects the values produced by the Rust side, in the order the oracle printed them.
#[derive(Default)]
struct Got(Vec<f64>);

impl Got {
    fn f(&mut self, v: f64) {
        self.0.push(v);
    }
    fn fs(&mut self, vs: &[f64]) {
        for v in vs {
            self.f(*v);
        }
    }
    fn pnt(&mut self, v: &Pnt) {
        self.fs(&[v.x(), v.y(), v.z()]);
    }
    fn vec(&mut self, v: &GpVec) {
        self.fs(&[v.x(), v.y(), v.z()]);
    }
    fn vecs(&mut self, vs: &[GpVec]) {
        for v in vs {
            self.vec(v);
        }
    }
    fn dir(&mut self, v: &Dir) {
        self.fs(&[v.x(), v.y(), v.z()]);
    }
    fn pnt2(&mut self, v: &Pnt2d) {
        self.fs(&[v.x(), v.y()]);
    }
    fn vec2(&mut self, v: &Vec2d) {
        self.fs(&[v.x(), v.y()]);
    }
    fn vec2s(&mut self, vs: &[Vec2d]) {
        for v in vs {
            self.vec2(v);
        }
    }
    fn ax1(&mut self, v: &Ax1) {
        self.pnt(v.location());
        self.dir(v.direction());
    }
    fn ax2(&mut self, v: &Ax2) {
        self.pnt(v.location());
        self.dir(v.direction());
        self.dir(v.x_direction());
        self.dir(v.y_direction());
    }
    /// The full state of a line: location and direction.
    fn lin(&mut self, v: &Lin) {
        self.ax1(v.position());
    }
    /// The full state of a circle: frame and radius (a negative radius is a legal state, see the docs).
    fn circ(&mut self, v: &Circ) {
        self.ax2(v.position());
        self.f(v.radius());
    }
    fn elips(&mut self, v: &Elips) {
        self.ax2(v.position());
        self.fs(&[v.major_radius(), v.minor_radius()]);
    }
    fn hypr(&mut self, v: &Hypr) {
        self.ax2(v.position());
        self.fs(&[v.major_radius(), v.minor_radius()]);
    }
    fn parab(&mut self, v: &Parab) {
        self.ax2(v.position());
        self.f(v.focal());
    }
}

// ---------------------------------------------------------------------------------
// Reading the inputs of a line

fn bits(w: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(w, 16).unwrap_or_else(|e| panic!("bad hex word {w}: {e}")))
}

struct Rd<'a> {
    words: &'a [&'a str],
    at: usize,
}

impl<'a> Rd<'a> {
    fn word(&mut self) -> &'a str {
        let w = *self.words.get(self.at).unwrap_or_else(|| panic!("input word {} missing", self.at));
        self.at += 1;
        w
    }
    /// A scalar.
    fn f(&mut self) -> f64 {
        bits(self.word())
    }
    /// An integer.
    fn i(&mut self) -> i32 {
        self.word().parse().expect("integer input")
    }
    fn pnt(&mut self) -> Pnt {
        let x = self.f();
        let y = self.f();
        let z = self.f();
        Pnt::new(x, y, z)
    }
    fn pnt2d(&mut self) -> Pnt2d {
        let x = self.f();
        let y = self.f();
        Pnt2d::new(x, y)
    }
    /// A 2D vector, exactly as printed.
    fn vec2d(&mut self) -> Vec2d {
        let x = self.f();
        let y = self.f();
        Vec2d::new(x, y)
    }
    /// A raw (un-normalized) 3D direction; `Dir::new` normalizes it exactly like the oracle did.
    fn dir(&mut self) -> Result<Dir, Thrown> {
        let x = self.f();
        let y = self.f();
        let z = self.f();
        Ok(Dir::new(x, y, z)?)
    }
    /// A raw 2D direction.
    fn dir2d(&mut self) -> Result<Dir2d, Thrown> {
        let x = self.f();
        let y = self.f();
        Ok(Dir2d::new(x, y)?)
    }
    /// Location and a raw direction.
    fn ax1(&mut self) -> Result<Ax1, Thrown> {
        let p = self.pnt();
        let d = self.dir()?;
        Ok(Ax1::new(p, d))
    }
    /// Location, raw main direction and raw X direction.
    fn ax2(&mut self) -> Result<Ax2, Thrown> {
        let p = self.pnt();
        let n = self.dir()?;
        let x = self.dir()?;
        Ok(Ax2::new(p, n, &x)?)
    }
    /// Location, main direction, X direction and a handedness flag (0 direct, 1 X reversed,
    /// 2 Y reversed).
    fn ax3(&mut self) -> Result<Ax3, Thrown> {
        let a = self.ax2()?;
        let mut a3 = Ax3::from_ax2(&a);
        match self.i() {
            1 => a3.x_reverse(),
            2 => a3.y_reverse(),
            _ => {}
        }
        Ok(a3)
    }
    /// Location and a raw 2D direction.
    fn ax2d(&mut self) -> Result<Ax2d, Thrown> {
        let p = self.pnt2d();
        let d = self.dir2d()?;
        Ok(Ax2d::new(p, d))
    }
    /// Location, raw X direction and a raw second direction that only selects the handedness.
    fn ax22d(&mut self) -> Result<Ax22d, Thrown> {
        let p = self.pnt2d();
        let vx = self.dir2d()?;
        let vy = self.dir2d()?;
        Ok(Ax22d::from_dirs(p, vx, vy))
    }
    fn lin(&mut self) -> Result<Lin, Thrown> {
        Ok(Lin::from_axis(self.ax1()?))
    }
    fn circ(&mut self) -> Result<Circ, Thrown> {
        let a = self.ax2()?;
        let r = self.f();
        Ok(Circ::new(a, r)?)
    }
    fn elips(&mut self) -> Result<Elips, Thrown> {
        let a = self.ax2()?;
        let (major, minor) = (self.f(), self.f());
        Ok(Elips::new(a, major, minor)?)
    }
    fn hypr(&mut self) -> Result<Hypr, Thrown> {
        let a = self.ax2()?;
        let (major, minor) = (self.f(), self.f());
        Ok(Hypr::new(a, major, minor)?)
    }
    fn parab(&mut self) -> Result<Parab, Thrown> {
        let a = self.ax2()?;
        let focal = self.f();
        Ok(Parab::new(a, focal)?)
    }
    fn lin2d(&mut self) -> Result<Lin2d, Thrown> {
        Ok(Lin2d::from_ax2d(self.ax2d()?))
    }
    fn circ2d(&mut self) -> Result<Circ2d, Thrown> {
        let a = self.ax22d()?;
        let r = self.f();
        Ok(Circ2d::from_ax22d(a, r)?)
    }
    fn elips2d(&mut self) -> Result<Elips2d, Thrown> {
        let a = self.ax22d()?;
        let (major, minor) = (self.f(), self.f());
        Ok(Elips2d::from_ax22d(a, major, minor)?)
    }
    fn hypr2d(&mut self) -> Result<Hypr2d, Thrown> {
        let a = self.ax22d()?;
        let (major, minor) = (self.f(), self.f());
        Ok(Hypr2d::from_ax22d(a, major, minor)?)
    }
    fn parab2d(&mut self) -> Result<Parab2d, Thrown> {
        let a = self.ax22d()?;
        let focal = self.f();
        Ok(Parab2d::from_ax22d(a, focal)?)
    }
    fn pln(&mut self) -> Result<Pln, Thrown> {
        Ok(Pln::new(self.ax3()?))
    }
    fn cylinder(&mut self) -> Result<Cylinder, Thrown> {
        let a = self.ax3()?;
        let r = self.f();
        Ok(Cylinder::new(a, r)?)
    }
    /// Frame, semi-angle, reference radius (the order of `gp_Cone(A3, Ang, Radius)`).
    fn cone(&mut self) -> Result<Cone, Thrown> {
        let a = self.ax3()?;
        let (angle, r) = (self.f(), self.f());
        Ok(Cone::new(a, angle, r)?)
    }
    fn sphere(&mut self) -> Result<Sphere, Thrown> {
        let a = self.ax3()?;
        let r = self.f();
        Ok(Sphere::new(a, r)?)
    }
    fn torus(&mut self) -> Result<Torus, Thrown> {
        let a = self.ax3()?;
        let (major, minor) = (self.f(), self.f());
        Ok(Torus::new(a, major, minor)?)
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread inputs");
    }
}

// ---------------------------------------------------------------------------------
// Replaying one family

fn run(name: &str, r: &mut Rd, g: &mut Got) -> Result<(), Thrown> {
    match name {
        "elslib_pln_value" => {
            let a = r.ax3()?;
            let (u, v) = (r.f(), r.f());
            g.pnt(&elslib::plane_value(u, v, &a));
            g.pnt(&elslib::plane_d0(u, v, &a));
        }
        "elslib_pln_d1" => {
            let a = r.ax3()?;
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv) = elslib::plane_d1(u, v, &a);
            g.pnt(&p);
            g.vecs(&[vu, vv]);
        }
        "elslib_pln_dn" => {
            let a = r.ax3()?;
            let (u, v) = (r.f(), r.f());
            let (nu, nv) = (r.i(), r.i());
            g.vec(&elslib::plane_dn(u, v, &a, nu, nv));
        }
        "elslib_pln_parameters" => {
            let a = r.ax3()?;
            let p = r.pnt();
            let (u, v) = elslib::plane_parameters(&a, &p);
            g.fs(&[u, v]);
        }
        "elslib_pln_uiso" => {
            let a = r.ax3()?;
            let u = r.f();
            g.lin(&elslib::plane_uiso(&a, u));
        }
        "elslib_pln_viso" => {
            let a = r.ax3()?;
            let v = r.f();
            g.lin(&elslib::plane_viso(&a, v));
        }
        "elslib_pln_object" => {
            let s = r.pln()?;
            let (u, v) = (r.f(), r.f());
            let (nu, nv) = (r.i(), r.i());
            let p = r.pnt();
            g.pnt(&elslib::value_pln(u, v, &s));
            g.pnt(&elslib::d0_pln(u, v, &s));
            let (q, vu, vv) = elslib::d1_pln(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv]);
            g.vec(&elslib::dn_pln(u, v, &s, nu, nv));
            let (pu, pv) = elslib::parameters_pln(&s, &p);
            g.fs(&[pu, pv]);
        }
        "elslib_cylinder_value" => {
            let a = r.ax3()?;
            let rad = r.f();
            let (u, v) = (r.f(), r.f());
            g.pnt(&elslib::cylinder_value(u, v, &a, rad));
            g.pnt(&elslib::cylinder_d0(u, v, &a, rad));
        }
        "elslib_cylinder_d1" => {
            let a = r.ax3()?;
            let rad = r.f();
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv) = elslib::cylinder_d1(u, v, &a, rad);
            g.pnt(&p);
            g.vecs(&[vu, vv]);
        }
        "elslib_cylinder_d2" => {
            let a = r.ax3()?;
            let rad = r.f();
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv, vuu, vvv, vuv) = elslib::cylinder_d2(u, v, &a, rad);
            g.pnt(&p);
            g.vecs(&[vu, vv, vuu, vvv, vuv]);
        }
        "elslib_cylinder_d3" => {
            let a = r.ax3()?;
            let rad = r.f();
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv) = elslib::cylinder_d3(u, v, &a, rad);
            g.pnt(&p);
            g.vecs(&[vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv]);
        }
        "elslib_cylinder_dn" => {
            let a = r.ax3()?;
            let rad = r.f();
            let (u, v) = (r.f(), r.f());
            let (nu, nv) = (r.i(), r.i());
            g.vec(&elslib::cylinder_dn(u, v, &a, rad, nu, nv));
        }
        "elslib_cylinder_parameters" => {
            let a = r.ax3()?;
            let rad = r.f();
            let p = r.pnt();
            let (u, v) = elslib::cylinder_parameters(&a, rad, &p);
            g.fs(&[u, v]);
        }
        "elslib_cylinder_uiso" => {
            let a = r.ax3()?;
            let rad = r.f();
            let u = r.f();
            g.lin(&elslib::cylinder_uiso(&a, rad, u));
        }
        "elslib_cylinder_viso" => {
            let a = r.ax3()?;
            let rad = r.f();
            let v = r.f();
            g.circ(&elslib::cylinder_viso(&a, rad, v));
        }
        "elslib_cylinder_object" => {
            let s = r.cylinder()?;
            let (u, v) = (r.f(), r.f());
            let (nu, nv) = (r.i(), r.i());
            let p = r.pnt();
            g.pnt(&elslib::value_cylinder(u, v, &s));
            g.pnt(&elslib::d0_cylinder(u, v, &s));
            let (q, vu, vv) = elslib::d1_cylinder(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv]);
            let (q, vu, vv, vuu, vvv, vuv) = elslib::d2_cylinder(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv, vuu, vvv, vuv]);
            let (q, vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv) = elslib::d3_cylinder(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv]);
            g.vec(&elslib::dn_cylinder(u, v, &s, nu, nv));
            let (pu, pv) = elslib::parameters_cylinder(&s, &p);
            g.fs(&[pu, pv]);
        }
        "elslib_cone_value" => {
            let a = r.ax3()?;
            let rad = r.f();
            let sa = r.f();
            let (u, v) = (r.f(), r.f());
            g.pnt(&elslib::cone_value(u, v, &a, rad, sa));
            g.pnt(&elslib::cone_d0(u, v, &a, rad, sa));
        }
        "elslib_cone_d1" => {
            let a = r.ax3()?;
            let rad = r.f();
            let sa = r.f();
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv) = elslib::cone_d1(u, v, &a, rad, sa);
            g.pnt(&p);
            g.vecs(&[vu, vv]);
        }
        "elslib_cone_d2" => {
            let a = r.ax3()?;
            let rad = r.f();
            let sa = r.f();
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv, vuu, vvv, vuv) = elslib::cone_d2(u, v, &a, rad, sa);
            g.pnt(&p);
            g.vecs(&[vu, vv, vuu, vvv, vuv]);
        }
        "elslib_cone_d3" => {
            let a = r.ax3()?;
            let rad = r.f();
            let sa = r.f();
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv) = elslib::cone_d3(u, v, &a, rad, sa);
            g.pnt(&p);
            g.vecs(&[vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv]);
        }
        "elslib_cone_dn" => {
            let a = r.ax3()?;
            let rad = r.f();
            let sa = r.f();
            let (u, v) = (r.f(), r.f());
            let (nu, nv) = (r.i(), r.i());
            g.vec(&elslib::cone_dn(u, v, &a, rad, sa, nu, nv));
        }
        "elslib_cone_parameters" => {
            let a = r.ax3()?;
            let rad = r.f();
            let sa = r.f();
            let p = r.pnt();
            let (u, v) = elslib::cone_parameters(&a, rad, sa, &p);
            g.fs(&[u, v]);
        }
        "elslib_cone_uiso" => {
            let a = r.ax3()?;
            let rad = r.f();
            let sa = r.f();
            let u = r.f();
            g.lin(&elslib::cone_uiso(&a, rad, sa, u));
        }
        "elslib_cone_viso" => {
            let a = r.ax3()?;
            let rad = r.f();
            let sa = r.f();
            let v = r.f();
            g.circ(&elslib::cone_viso(&a, rad, sa, v));
        }
        "elslib_cone_object" => {
            let s = r.cone()?;
            let (u, v) = (r.f(), r.f());
            let (nu, nv) = (r.i(), r.i());
            let p = r.pnt();
            g.pnt(&elslib::value_cone(u, v, &s));
            g.pnt(&elslib::d0_cone(u, v, &s));
            let (q, vu, vv) = elslib::d1_cone(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv]);
            let (q, vu, vv, vuu, vvv, vuv) = elslib::d2_cone(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv, vuu, vvv, vuv]);
            let (q, vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv) = elslib::d3_cone(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv]);
            g.vec(&elslib::dn_cone(u, v, &s, nu, nv));
            let (pu, pv) = elslib::parameters_cone(&s, &p);
            g.fs(&[pu, pv]);
        }
        "elslib_sphere_value" => {
            let a = r.ax3()?;
            let rad = r.f();
            let (u, v) = (r.f(), r.f());
            g.pnt(&elslib::sphere_value(u, v, &a, rad));
            g.pnt(&elslib::sphere_d0(u, v, &a, rad));
        }
        "elslib_sphere_d1" => {
            let a = r.ax3()?;
            let rad = r.f();
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv) = elslib::sphere_d1(u, v, &a, rad);
            g.pnt(&p);
            g.vecs(&[vu, vv]);
        }
        "elslib_sphere_d2" => {
            let a = r.ax3()?;
            let rad = r.f();
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv, vuu, vvv, vuv) = elslib::sphere_d2(u, v, &a, rad);
            g.pnt(&p);
            g.vecs(&[vu, vv, vuu, vvv, vuv]);
        }
        "elslib_sphere_d3" => {
            let a = r.ax3()?;
            let rad = r.f();
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv) = elslib::sphere_d3(u, v, &a, rad);
            g.pnt(&p);
            g.vecs(&[vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv]);
        }
        "elslib_sphere_dn" => {
            let a = r.ax3()?;
            let rad = r.f();
            let (u, v) = (r.f(), r.f());
            let (nu, nv) = (r.i(), r.i());
            g.vec(&elslib::sphere_dn(u, v, &a, rad, nu, nv));
        }
        "elslib_sphere_parameters" => {
            let a = r.ax3()?;
            let rad = r.f();
            let p = r.pnt();
            let (u, v) = elslib::sphere_parameters(&a, rad, &p);
            g.fs(&[u, v]);
        }
        "elslib_sphere_uiso" => {
            let a = r.ax3()?;
            let rad = r.f();
            let u = r.f();
            g.circ(&elslib::sphere_uiso(&a, rad, u));
        }
        "elslib_sphere_viso" => {
            let a = r.ax3()?;
            let rad = r.f();
            let v = r.f();
            g.circ(&elslib::sphere_viso(&a, rad, v));
        }
        "elslib_sphere_object" => {
            let s = r.sphere()?;
            let (u, v) = (r.f(), r.f());
            let (nu, nv) = (r.i(), r.i());
            let p = r.pnt();
            g.pnt(&elslib::value_sphere(u, v, &s));
            g.pnt(&elslib::d0_sphere(u, v, &s));
            let (q, vu, vv) = elslib::d1_sphere(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv]);
            let (q, vu, vv, vuu, vvv, vuv) = elslib::d2_sphere(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv, vuu, vvv, vuv]);
            let (q, vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv) = elslib::d3_sphere(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv]);
            g.vec(&elslib::dn_sphere(u, v, &s, nu, nv));
            let (pu, pv) = elslib::parameters_sphere(&s, &p);
            g.fs(&[pu, pv]);
        }
        "elslib_torus_value" => {
            let a = r.ax3()?;
            let major = r.f();
            let minor = r.f();
            let (u, v) = (r.f(), r.f());
            g.pnt(&elslib::torus_value(u, v, &a, major, minor));
            g.pnt(&elslib::torus_d0(u, v, &a, major, minor));
        }
        "elslib_torus_d1" => {
            let a = r.ax3()?;
            let major = r.f();
            let minor = r.f();
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv) = elslib::torus_d1(u, v, &a, major, minor);
            g.pnt(&p);
            g.vecs(&[vu, vv]);
        }
        "elslib_torus_d2" => {
            let a = r.ax3()?;
            let major = r.f();
            let minor = r.f();
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv, vuu, vvv, vuv) = elslib::torus_d2(u, v, &a, major, minor);
            g.pnt(&p);
            g.vecs(&[vu, vv, vuu, vvv, vuv]);
        }
        "elslib_torus_d3" => {
            let a = r.ax3()?;
            let major = r.f();
            let minor = r.f();
            let (u, v) = (r.f(), r.f());
            let (p, vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv) = elslib::torus_d3(u, v, &a, major, minor);
            g.pnt(&p);
            g.vecs(&[vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv]);
        }
        "elslib_torus_dn" => {
            let a = r.ax3()?;
            let major = r.f();
            let minor = r.f();
            let (u, v) = (r.f(), r.f());
            let (nu, nv) = (r.i(), r.i());
            g.vec(&elslib::torus_dn(u, v, &a, major, minor, nu, nv));
        }
        "elslib_torus_parameters" => {
            let a = r.ax3()?;
            let major = r.f();
            let minor = r.f();
            let p = r.pnt();
            let (u, v) = elslib::torus_parameters(&a, major, minor, &p);
            g.fs(&[u, v]);
        }
        "elslib_torus_uiso" => {
            let a = r.ax3()?;
            let major = r.f();
            let minor = r.f();
            let u = r.f();
            g.circ(&elslib::torus_uiso(&a, major, minor, u));
        }
        "elslib_torus_viso" => {
            let a = r.ax3()?;
            let major = r.f();
            let minor = r.f();
            let v = r.f();
            g.circ(&elslib::torus_viso(&a, major, minor, v));
        }
        "elslib_torus_object" => {
            let s = r.torus()?;
            let (u, v) = (r.f(), r.f());
            let (nu, nv) = (r.i(), r.i());
            let p = r.pnt();
            g.pnt(&elslib::value_torus(u, v, &s));
            g.pnt(&elslib::d0_torus(u, v, &s));
            let (q, vu, vv) = elslib::d1_torus(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv]);
            let (q, vu, vv, vuu, vvv, vuv) = elslib::d2_torus(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv, vuu, vvv, vuv]);
            let (q, vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv) = elslib::d3_torus(u, v, &s);
            g.pnt(&q);
            g.vecs(&[vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv]);
            g.vec(&elslib::dn_torus(u, v, &s, nu, nv));
            let (pu, pv) = elslib::parameters_torus(&s, &p);
            g.fs(&[pu, pv]);
        }
        "elclib_lin_eval" => {
            let c = r.lin()?;
            let u = r.f();
            g.pnt(&elclib::value_lin(u, &c));
            g.pnt(&elclib::line_value(u, c.position()));
            let (p, v1) = elclib::d1_lin(u, &c);
            g.pnt(&p);
            g.vecs(&[v1]);
            let (p, v1) = elclib::line_d1(u, c.position());
            g.pnt(&p);
            g.vecs(&[v1]);
        }
        "elclib_lin_dn" => {
            let c = r.lin()?;
            let u = r.f();
            let n = r.i();
            g.vec(&elclib::dn_lin(u, &c, n));
            g.vec(&elclib::line_dn(u, c.position(), n));
        }
        "elclib_lin_parameter" => {
            let c = r.lin()?;
            let p = r.pnt();
            g.f(elclib::parameter_lin(&c, &p));
            g.f(elclib::line_parameter(c.position(), &p));
        }
        "elclib_circ_eval" => {
            let c = r.circ()?;
            let u = r.f();
            g.pnt(&elclib::value_circ(u, &c));
            g.pnt(&elclib::circle_value(u, c.position(), c.radius()));
            let (p, v1) = elclib::d1_circ(u, &c);
            g.pnt(&p);
            g.vecs(&[v1]);
            let (p, v1) = elclib::circle_d1(u, c.position(), c.radius());
            g.pnt(&p);
            g.vecs(&[v1]);
            let (p, v1, v2) = elclib::d2_circ(u, &c);
            g.pnt(&p);
            g.vecs(&[v1, v2]);
            let (p, v1, v2) = elclib::circle_d2(u, c.position(), c.radius());
            g.pnt(&p);
            g.vecs(&[v1, v2]);
            let (p, v1, v2, v3) = elclib::d3_circ(u, &c);
            g.pnt(&p);
            g.vecs(&[v1, v2, v3]);
            let (p, v1, v2, v3) = elclib::circle_d3(u, c.position(), c.radius());
            g.pnt(&p);
            g.vecs(&[v1, v2, v3]);
        }
        "elclib_circ_dn" => {
            let c = r.circ()?;
            let u = r.f();
            let n = r.i();
            g.vec(&elclib::dn_circ(u, &c, n));
            g.vec(&elclib::circle_dn(u, c.position(), c.radius(), n));
        }
        "elclib_circ_parameter" => {
            let c = r.circ()?;
            let p = r.pnt();
            g.f(elclib::parameter_circ(&c, &p));
            g.f(elclib::circle_parameter(c.position(), &p));
        }
        "elclib_elips_eval" => {
            let c = r.elips()?;
            let u = r.f();
            g.pnt(&elclib::value_elips(u, &c));
            g.pnt(&elclib::ellipse_value(u, c.position(), c.major_radius(), c.minor_radius()));
            let (p, v1) = elclib::d1_elips(u, &c);
            g.pnt(&p);
            g.vecs(&[v1]);
            let (p, v1) = elclib::ellipse_d1(u, c.position(), c.major_radius(), c.minor_radius());
            g.pnt(&p);
            g.vecs(&[v1]);
            let (p, v1, v2) = elclib::d2_elips(u, &c);
            g.pnt(&p);
            g.vecs(&[v1, v2]);
            let (p, v1, v2) = elclib::ellipse_d2(u, c.position(), c.major_radius(), c.minor_radius());
            g.pnt(&p);
            g.vecs(&[v1, v2]);
            let (p, v1, v2, v3) = elclib::d3_elips(u, &c);
            g.pnt(&p);
            g.vecs(&[v1, v2, v3]);
            let (p, v1, v2, v3) = elclib::ellipse_d3(u, c.position(), c.major_radius(), c.minor_radius());
            g.pnt(&p);
            g.vecs(&[v1, v2, v3]);
        }
        "elclib_elips_dn" => {
            let c = r.elips()?;
            let u = r.f();
            let n = r.i();
            g.vec(&elclib::dn_elips(u, &c, n));
            g.vec(&elclib::ellipse_dn(u, c.position(), c.major_radius(), c.minor_radius(), n));
        }
        "elclib_elips_parameter" => {
            let c = r.elips()?;
            let p = r.pnt();
            g.f(elclib::parameter_elips(&c, &p));
            g.f(elclib::ellipse_parameter(c.position(), c.major_radius(), c.minor_radius(), &p));
        }
        "elclib_hypr_eval" => {
            let c = r.hypr()?;
            let u = r.f();
            g.pnt(&elclib::value_hypr(u, &c));
            g.pnt(&elclib::hyperbola_value(u, c.position(), c.major_radius(), c.minor_radius()));
            let (p, v1) = elclib::d1_hypr(u, &c);
            g.pnt(&p);
            g.vecs(&[v1]);
            let (p, v1) = elclib::hyperbola_d1(u, c.position(), c.major_radius(), c.minor_radius());
            g.pnt(&p);
            g.vecs(&[v1]);
            let (p, v1, v2) = elclib::d2_hypr(u, &c);
            g.pnt(&p);
            g.vecs(&[v1, v2]);
            let (p, v1, v2) = elclib::hyperbola_d2(u, c.position(), c.major_radius(), c.minor_radius());
            g.pnt(&p);
            g.vecs(&[v1, v2]);
            let (p, v1, v2, v3) = elclib::d3_hypr(u, &c);
            g.pnt(&p);
            g.vecs(&[v1, v2, v3]);
            let (p, v1, v2, v3) = elclib::hyperbola_d3(u, c.position(), c.major_radius(), c.minor_radius());
            g.pnt(&p);
            g.vecs(&[v1, v2, v3]);
        }
        "elclib_hypr_dn" => {
            let c = r.hypr()?;
            let u = r.f();
            let n = r.i();
            g.vec(&elclib::dn_hypr(u, &c, n));
            g.vec(&elclib::hyperbola_dn(u, c.position(), c.major_radius(), c.minor_radius(), n));
        }
        "elclib_hypr_parameter" => {
            let c = r.hypr()?;
            let p = r.pnt();
            g.f(elclib::parameter_hypr(&c, &p));
            g.f(elclib::hyperbola_parameter(c.position(), c.major_radius(), c.minor_radius(), &p));
        }
        "elclib_parab_eval" => {
            let c = r.parab()?;
            let u = r.f();
            g.pnt(&elclib::value_parab(u, &c));
            g.pnt(&elclib::parabola_value(u, c.position(), c.focal()));
            let (p, v1) = elclib::d1_parab(u, &c);
            g.pnt(&p);
            g.vecs(&[v1]);
            let (p, v1) = elclib::parabola_d1(u, c.position(), c.focal());
            g.pnt(&p);
            g.vecs(&[v1]);
            let (p, v1, v2) = elclib::d2_parab(u, &c);
            g.pnt(&p);
            g.vecs(&[v1, v2]);
            let (p, v1, v2) = elclib::parabola_d2(u, c.position(), c.focal());
            g.pnt(&p);
            g.vecs(&[v1, v2]);
        }
        "elclib_parab_dn" => {
            let c = r.parab()?;
            let u = r.f();
            let n = r.i();
            g.vec(&elclib::dn_parab(u, &c, n));
            g.vec(&elclib::parabola_dn(u, c.position(), c.focal(), n));
        }
        "elclib_parab_parameter" => {
            let c = r.parab()?;
            let p = r.pnt();
            g.f(elclib::parameter_parab(&c, &p));
            g.f(elclib::parabola_parameter(c.position(), &p));
        }
        "elclib_lin2d_eval" => {
            let c = r.lin2d()?;
            let u = r.f();
            g.pnt2(&elclib::value_lin2d(u, &c));
            g.pnt2(&elclib::line2d_value(u, c.position()));
            let (p, v1) = elclib::d1_lin2d(u, &c);
            g.pnt2(&p);
            g.vec2s(&[v1]);
            let (p, v1) = elclib::line2d_d1(u, c.position());
            g.pnt2(&p);
            g.vec2s(&[v1]);
        }
        "elclib_lin2d_dn" => {
            let c = r.lin2d()?;
            let u = r.f();
            let n = r.i();
            g.vec2(&elclib::dn_lin2d(u, &c, n));
            g.vec2(&elclib::line2d_dn(u, c.position(), n));
        }
        "elclib_lin2d_parameter" => {
            let c = r.lin2d()?;
            let p = r.pnt2d();
            g.f(elclib::parameter_lin2d(&c, &p));
            g.f(elclib::line2d_parameter(c.position(), &p));
        }
        "elclib_circ2d_eval" => {
            let c = r.circ2d()?;
            let u = r.f();
            g.pnt2(&elclib::value_circ2d(u, &c));
            g.pnt2(&elclib::circle2d_value(u, c.axis(), c.radius()));
            let (p, v1) = elclib::d1_circ2d(u, &c);
            g.pnt2(&p);
            g.vec2s(&[v1]);
            let (p, v1) = elclib::circle2d_d1(u, c.axis(), c.radius());
            g.pnt2(&p);
            g.vec2s(&[v1]);
            let (p, v1, v2) = elclib::d2_circ2d(u, &c);
            g.pnt2(&p);
            g.vec2s(&[v1, v2]);
            let (p, v1, v2) = elclib::circle2d_d2(u, c.axis(), c.radius());
            g.pnt2(&p);
            g.vec2s(&[v1, v2]);
            let (p, v1, v2, v3) = elclib::d3_circ2d(u, &c);
            g.pnt2(&p);
            g.vec2s(&[v1, v2, v3]);
            let (p, v1, v2, v3) = elclib::circle2d_d3(u, c.axis(), c.radius());
            g.pnt2(&p);
            g.vec2s(&[v1, v2, v3]);
        }
        "elclib_circ2d_dn" => {
            let c = r.circ2d()?;
            let u = r.f();
            let n = r.i();
            g.vec2(&elclib::dn_circ2d(u, &c, n));
            g.vec2(&elclib::circle2d_dn(u, c.axis(), c.radius(), n));
        }
        "elclib_circ2d_parameter" => {
            let c = r.circ2d()?;
            let p = r.pnt2d();
            g.f(elclib::parameter_circ2d(&c, &p)?);
            g.f(elclib::circle2d_parameter(c.axis(), &p)?);
        }
        "elclib_elips2d_eval" => {
            let c = r.elips2d()?;
            let u = r.f();
            g.pnt2(&elclib::value_elips2d(u, &c));
            g.pnt2(&elclib::ellipse2d_value(u, c.axis(), c.major_radius(), c.minor_radius()));
            let (p, v1) = elclib::d1_elips2d(u, &c);
            g.pnt2(&p);
            g.vec2s(&[v1]);
            let (p, v1) = elclib::ellipse2d_d1(u, c.axis(), c.major_radius(), c.minor_radius());
            g.pnt2(&p);
            g.vec2s(&[v1]);
            let (p, v1, v2) = elclib::d2_elips2d(u, &c);
            g.pnt2(&p);
            g.vec2s(&[v1, v2]);
            let (p, v1, v2) = elclib::ellipse2d_d2(u, c.axis(), c.major_radius(), c.minor_radius());
            g.pnt2(&p);
            g.vec2s(&[v1, v2]);
            let (p, v1, v2, v3) = elclib::d3_elips2d(u, &c);
            g.pnt2(&p);
            g.vec2s(&[v1, v2, v3]);
            let (p, v1, v2, v3) = elclib::ellipse2d_d3(u, c.axis(), c.major_radius(), c.minor_radius());
            g.pnt2(&p);
            g.vec2s(&[v1, v2, v3]);
        }
        "elclib_elips2d_dn" => {
            let c = r.elips2d()?;
            let u = r.f();
            let n = r.i();
            g.vec2(&elclib::dn_elips2d(u, &c, n));
            g.vec2(&elclib::ellipse2d_dn(u, c.axis(), c.major_radius(), c.minor_radius(), n));
        }
        "elclib_elips2d_parameter" => {
            let c = r.elips2d()?;
            let p = r.pnt2d();
            g.f(elclib::parameter_elips2d(&c, &p)?);
            g.f(elclib::ellipse2d_parameter(c.axis(), c.major_radius(), c.minor_radius(), &p)?);
        }
        "elclib_hypr2d_eval" => {
            let c = r.hypr2d()?;
            let u = r.f();
            g.pnt2(&elclib::value_hypr2d(u, &c));
            g.pnt2(&elclib::hyperbola2d_value(u, c.axis(), c.major_radius(), c.minor_radius()));
            let (p, v1) = elclib::d1_hypr2d(u, &c);
            g.pnt2(&p);
            g.vec2s(&[v1]);
            let (p, v1) = elclib::hyperbola2d_d1(u, c.axis(), c.major_radius(), c.minor_radius());
            g.pnt2(&p);
            g.vec2s(&[v1]);
            let (p, v1, v2) = elclib::d2_hypr2d(u, &c);
            g.pnt2(&p);
            g.vec2s(&[v1, v2]);
            let (p, v1, v2) = elclib::hyperbola2d_d2(u, c.axis(), c.major_radius(), c.minor_radius());
            g.pnt2(&p);
            g.vec2s(&[v1, v2]);
            let (p, v1, v2, v3) = elclib::d3_hypr2d(u, &c);
            g.pnt2(&p);
            g.vec2s(&[v1, v2, v3]);
            let (p, v1, v2, v3) = elclib::hyperbola2d_d3(u, c.axis(), c.major_radius(), c.minor_radius());
            g.pnt2(&p);
            g.vec2s(&[v1, v2, v3]);
        }
        "elclib_hypr2d_dn" => {
            let c = r.hypr2d()?;
            let u = r.f();
            let n = r.i();
            g.vec2(&elclib::dn_hypr2d(u, &c, n));
            g.vec2(&elclib::hyperbola2d_dn(u, c.axis(), c.major_radius(), c.minor_radius(), n));
        }
        "elclib_hypr2d_parameter" => {
            let c = r.hypr2d()?;
            let p = r.pnt2d();
            g.f(elclib::parameter_hypr2d(&c, &p));
            g.f(elclib::hyperbola2d_parameter(c.axis(), c.major_radius(), c.minor_radius(), &p));
        }
        "elclib_parab2d_eval" => {
            let c = r.parab2d()?;
            let u = r.f();
            g.pnt2(&elclib::value_parab2d(u, &c));
            g.pnt2(&elclib::parabola2d_value(u, c.axis(), c.focal()));
            let (p, v1) = elclib::d1_parab2d(u, &c);
            g.pnt2(&p);
            g.vec2s(&[v1]);
            let (p, v1) = elclib::parabola2d_d1(u, c.axis(), c.focal());
            g.pnt2(&p);
            g.vec2s(&[v1]);
            let (p, v1, v2) = elclib::d2_parab2d(u, &c);
            g.pnt2(&p);
            g.vec2s(&[v1, v2]);
            let (p, v1, v2) = elclib::parabola2d_d2(u, c.axis(), c.focal());
            g.pnt2(&p);
            g.vec2s(&[v1, v2]);
        }
        "elclib_parab2d_dn" => {
            let c = r.parab2d()?;
            let u = r.f();
            let n = r.i();
            g.vec2(&elclib::dn_parab2d(u, &c, n));
            g.vec2(&elclib::parabola2d_dn(u, c.axis(), c.focal(), n));
        }
        "elclib_parab2d_parameter" => {
            let c = r.parab2d()?;
            let p = r.pnt2d();
            g.f(elclib::parameter_parab2d(&c, &p));
            g.f(elclib::parabola2d_parameter(c.axis(), &p));
        }
        "elclib_to3d_pnt" => {
            let f = r.ax2()?;
            let p = r.pnt2d();
            g.pnt(&elclib::to3d_pnt(&f, &p));
        }
        "elclib_to3d_vec" => {
            let f = r.ax2()?;
            let v = r.vec2d();
            g.vec(&elclib::to3d_vec(&f, &v));
        }
        "elclib_to3d_dir" => {
            let f = r.ax2()?;
            let d = r.dir2d()?;
            g.dir(&elclib::to3d_dir(&f, &d)?);
        }
        "elclib_to3d_ax2d" => {
            let f = r.ax2()?;
            let l = r.ax2d()?;
            g.ax1(&elclib::to3d_ax2d(&f, &l)?);
        }
        "elclib_to3d_lin" => {
            let f = r.ax2()?;
            let l = r.lin2d()?;
            g.lin(&elclib::to3d_lin(&f, &l)?);
        }
        "elclib_to3d_ax22d" => {
            let f = r.ax2()?;
            let a = r.ax22d()?;
            g.ax2(&elclib::to3d_ax22d(&f, &a)?);
        }
        "elclib_to3d_circ" => {
            let f = r.ax2()?;
            let c = r.circ2d()?;
            g.circ(&elclib::to3d_circ(&f, &c)?);
        }
        "elclib_to3d_elips" => {
            let f = r.ax2()?;
            let c = r.elips2d()?;
            g.elips(&elclib::to3d_elips(&f, &c)?);
        }
        "elclib_to3d_hypr" => {
            let f = r.ax2()?;
            let c = r.hypr2d()?;
            g.hypr(&elclib::to3d_hypr(&f, &c)?);
        }
        "elclib_to3d_parab" => {
            let f = r.ax2()?;
            let c = r.parab2d()?;
            g.parab(&elclib::to3d_parab(&f, &c)?);
        }
        _ => panic!("unknown family {name}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Comparison

/// Distance in units in the last place (0 = identical bits).
fn ulps(a: f64, b: f64) -> u64 {
    let key = |x: f64| {
        let b = x.to_bits() as i64;
        if b < 0 { i64::MIN - b } else { b }
    };
    key(a).abs_diff(key(b))
}

#[derive(Default)]
struct Tally {
    values: usize,
    exact: usize,
    worst: u64,
}

fn compare(tally: &mut Tally, line: &str, name: &str, want: &[&str], got: &[f64]) {
    assert_eq!(want.len(), got.len(), "value count differs\n{line}");
    let exact = EXACT.contains(&name);
    let mut scale = 1.0_f64;
    for w in want {
        scale = scale.max(bits(w).abs());
    }
    let tol = if scale.is_finite() { MAX_ULPS * f64::EPSILON * scale } else { 0.0 };
    for (index, (w, g)) in want.iter().zip(got).enumerate() {
        let w = bits(w);
        let d = ulps(w, *g);
        let ok = if exact { w.to_bits() == g.to_bits() } else { (w - g).abs() <= tol || w.to_bits() == g.to_bits() };
        assert!(ok, "value {index}: want {w:e}, got {g:e} ({d} ulps)\n{line}");
        tally.values += 1;
        tally.exact += usize::from(d == 0);
        tally.worst = tally.worst.max(d);
    }
}

// ---------------------------------------------------------------------------------

#[test]
fn elslib_matches_occt() {
    let text = include_str!("elslib.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in elslib.txt:\n{line}");
        let (head, tail) = line.split_once(" | ").unwrap_or_else(|| panic!("line without ' | ': {line}"));
        let head: Vec<&str> = head.split_whitespace().collect();
        let want: Vec<&str> = tail.split_whitespace().collect();
        let (name, inputs) = head.split_first().expect("family name");
        let name: &str = name;
        let entry = stats.entry(name).or_default();
        entry.0 += 1;
        entry.1 += usize::from(!inputs.is_empty());

        let mut reader = Rd { words: inputs, at: 0 };
        let mut got = Got::default();
        let outcome = run(name, &mut reader, &mut got);
        let marker = matches!(want.first(), Some(&"throws") | Some(&"unchecked"));
        match outcome {
            Err(Thrown) => assert!(marker, "Rust API returned an error but OCCT returned values\n{line}"),
            Ok(()) => {
                assert!(!marker, "Rust API returned values but OCCT {}\n{line}", want[0]);
                reader.finish();
                compare(&mut tally, line, name, &want, &got.0);
            }
        }
    }

    // Input coverage: no duplicates (checked above), every family with inputs has at least
    // MIN_DISTINCT distinct lines, and only the listed families are input-less (printed once).
    for (name, (count, with_inputs)) in &stats {
        if INPUT_LESS.contains(name) {
            assert_eq!((*count, *with_inputs), (1, 0), "input-less family {name} must be printed exactly once");
        } else {
            assert_eq!(*with_inputs, *count, "family {name} lost its inputs");
            assert!(*count >= MIN_DISTINCT, "family {name} has only {count} distinct lines");
        }
    }
    for name in INPUT_LESS {
        assert!(stats.contains_key(name), "input-less family {name} missing");
    }
    // Every family is still covered.
    let present: BTreeSet<&str> = stats.keys().copied().collect();
    let expected: BTreeSet<&str> = FAMILIES.iter().copied().collect();
    assert_eq!(present, expected, "family list changed");

    assert_eq!(lines, text.lines().count());
    assert_eq!(lines, GOLDEN_LINES, "golden line count changed");
    println!("{lines} lines, {} values, {} bit-identical, worst {} ulps", tally.values, tally.exact, tally.worst);
}
