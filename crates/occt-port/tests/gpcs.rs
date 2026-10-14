//! Numerical reference test for the 3D gp curves and elementary surfaces (OCCT 8.0.1).
//!
//! `gpcs.txt` is printed by `oracle/gpcs.cpp` (real OCCT). Each line is
//! `<family> <inputs...> | <outputs...>`: f64 values are 16-hex-digit bit patterns, ints
//! are decimal and bools are 0/1. Every input of a call is on the left of ` | `; this test
//! reads them back with the `Rd` reader methods of the same name as the oracle's schema
//! words (`f i xyz pnt vec dir ax1 ax2 ax3 trsf lin circ elips hypr parab pln cylinder cone
//! sphere torus`) and replays the call on the Rust API of docs/api-gpcs.md. The right side
//! is either the expected values, `throws` (OCCT raised) or `unchecked` (OCCT 8.0.1 as built
//! has that check compiled out and returns unspecified numbers; the Rust API must return an
//! error there).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::gp::{
    self, Ax1, Ax2, Ax3, Circ, Cone, Cylinder, Dir, Elips, Hypr, Lin, Parab, Pln, Pnt, Quaternion, Sphere, Torus, Trsf,
    Vec as GpVec, XYZ,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for sin/cos/tan/acos/asin dependent results, in units of
/// f64::EPSILON times the largest absolute value on the line (at least 1).
const MAX_ULPS: f64 = 8.0;

/// Every family with inputs must have at least this many distinct lines.
const MIN_DISTINCT: usize = 16;

/// Families that really take no input: they are printed exactly once (there are none).
const INPUT_LESS: &[&str] = &[];

/// Families whose results use only + - * / and sqrt: compared bit for bit. All other families
/// depend on sin/cos/tan/acos/asin somewhere (the rotations, `Dir::angle` and the relation tests
/// built on it, the cone's tangent) and are compared with MAX_ULPS.
const EXACT: &[&str] = &[
    "circ_access",
    "circ_constructors",
    "circ_motion",
    "circ_queries",
    "circ_radius_checks",
    "cone_access",
    "cone_checks",
    "cone_constructors",
    "cone_motion",
    "cylinder_access",
    "cylinder_coefficients",
    "cylinder_constructors",
    "cylinder_motion",
    "cylinder_radius_checks",
    "elips_access",
    "elips_constructors",
    "elips_directrix",
    "elips_motion",
    "elips_radius_checks",
    "hypr_access",
    "hypr_asymptotes",
    "hypr_constructors",
    "hypr_default_branches",
    "hypr_directrix",
    "hypr_eccentricity",
    "hypr_motion",
    "hypr_radius_checks",
    "lin_access",
    "lin_constructors",
    "lin_motion",
    "lin_normal",
    "lin_point_queries",
    "parab_access",
    "parab_constructors",
    "parab_directrix_focus",
    "parab_focal_checks",
    "parab_motion",
    "pln_access",
    "pln_constructors",
    "pln_from_coefficients",
    "pln_motion",
    "pln_point_queries",
    "sphere_access",
    "sphere_coefficients",
    "sphere_constructors",
    "sphere_motion",
    "sphere_radius_checks",
    "torus_access",
    "torus_coefficients",
    "torus_constructors",
    "torus_motion",
    "torus_radius_checks",
];

/// Every family the oracle must print (a family that disappears from gpcs.txt fails).
const FAMILIES: &[&str] = &[
    "circ_access",
    "circ_constructors",
    "circ_motion",
    "circ_queries",
    "circ_radius_checks",
    "circ_rotate",
    "cone_access",
    "cone_apex",
    "cone_checks",
    "cone_coefficients",
    "cone_constructors",
    "cone_motion",
    "cone_rotate",
    "cylinder_access",
    "cylinder_coefficients",
    "cylinder_constructors",
    "cylinder_motion",
    "cylinder_radius_checks",
    "cylinder_rotate",
    "elips_access",
    "elips_constructors",
    "elips_directrix",
    "elips_motion",
    "elips_radius_checks",
    "elips_rotate",
    "hypr_access",
    "hypr_asymptotes",
    "hypr_constructors",
    "hypr_default_branches",
    "hypr_directrix",
    "hypr_eccentricity",
    "hypr_motion",
    "hypr_radius_checks",
    "hypr_rotate",
    "lin_access",
    "lin_angle",
    "lin_constructors",
    "lin_line_distance",
    "lin_motion",
    "lin_normal",
    "lin_point_queries",
    "lin_rotate",
    "parab_access",
    "parab_constructors",
    "parab_directrix_focus",
    "parab_focal_checks",
    "parab_motion",
    "parab_rotate",
    "pln_access",
    "pln_constructors",
    "pln_from_coefficients",
    "pln_line_queries",
    "pln_motion",
    "pln_plane_queries",
    "pln_point_queries",
    "pln_rotate",
    "sphere_access",
    "sphere_coefficients",
    "sphere_constructors",
    "sphere_motion",
    "sphere_radius_checks",
    "sphere_rotate",
    "torus_access",
    "torus_coefficients",
    "torus_constructors",
    "torus_motion",
    "torus_radius_checks",
    "torus_rotate",
];

/// The number of lines of the golden file.
const GOLDEN_LINES: usize = 1088;

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

#[derive(Clone, Copy, Debug)]
enum Tok {
    F(f64),
    B(bool),
}

/// Collects the values produced by the Rust side, in the order the oracle printed them.
#[derive(Default)]
struct Got(Vec<Tok>);

impl Got {
    fn f(&mut self, v: f64) {
        self.0.push(Tok::F(v));
    }
    fn fs(&mut self, vs: &[f64]) {
        for v in vs {
            self.f(*v);
        }
    }
    fn b(&mut self, v: bool) {
        self.0.push(Tok::B(v));
    }
    fn xyz(&mut self, v: &XYZ) {
        self.fs(&[v.x(), v.y(), v.z()]);
    }
    fn pnt(&mut self, v: &Pnt) {
        self.xyz(v.xyz());
    }
    fn dir(&mut self, v: &Dir) {
        self.xyz(v.xyz());
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
    fn ax3(&mut self, v: &Ax3) {
        self.pnt(v.location());
        self.dir(v.direction());
        self.dir(v.x_direction());
        self.dir(v.y_direction());
    }
    /// The location of a frame is printed as part of the frame; the axis triple prints
    /// the main axis, the X axis and the Y axis.
    fn axes(&mut self, axis: &Ax1, x: &Ax1, y: &Ax1) {
        self.ax1(axis);
        self.ax1(x);
        self.ax1(y);
    }
    // The state of every curve and surface.
    fn lin(&mut self, v: &Lin) {
        self.ax1(v.position());
    }
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
    fn pln(&mut self, v: &Pln) {
        self.ax3(v.position());
    }
    fn cylinder(&mut self, v: &Cylinder) {
        self.ax3(v.position());
        self.f(v.radius());
    }
    fn cone(&mut self, v: &Cone) {
        self.ax3(v.position());
        self.fs(&[v.semi_angle(), v.ref_radius()]);
    }
    fn sphere(&mut self, v: &Sphere) {
        self.ax3(v.position());
        self.f(v.radius());
    }
    fn torus(&mut self, v: &Torus) {
        self.ax3(v.position());
        self.fs(&[v.major_radius(), v.minor_radius()]);
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
    fn vec(&mut self) -> GpVec {
        let x = self.f();
        let y = self.f();
        let z = self.f();
        GpVec::new(x, y, z)
    }
    /// A raw (un-normalized) direction; `Dir::new` normalizes it exactly like the oracle did.
    fn dir(&mut self) -> Result<Dir, Thrown> {
        let x = self.f();
        let y = self.f();
        let z = self.f();
        Ok(Dir::new(x, y, z)?)
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
    fn pln(&mut self) -> Result<Pln, Thrown> {
        Ok(Pln::new(self.ax3()?))
    }
    fn cylinder(&mut self) -> Result<Cylinder, Thrown> {
        let a = self.ax3()?;
        let r = self.f();
        Ok(Cylinder::new(a, r)?)
    }
    /// Frame, semi-angle, reference radius.
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
    /// A transformation recipe: kind, then the fields that kind uses (see oracle `mkTR`).
    fn trsf(&mut self) -> Result<Trsf, Thrown> {
        let kind = self.i();
        let mut t = Trsf::new();
        match kind {
            0 | 1 => {
                let axis = self.ax1()?;
                let angle = self.f();
                t.set_rotation(&axis, angle)?;
                if kind == 1 {
                    let v = self.vec();
                    t.set_translation_part(&v);
                }
            }
            2 => {
                let p = self.pnt();
                let factor = self.f();
                t.set_scale(&p, factor)?;
            }
            3 => {
                let v = self.vec();
                t.set_translation(&v);
            }
            4 => {
                let axis = self.ax1()?;
                t.set_mirror(&axis);
            }
            5 => {
                let p = self.pnt();
                t.set_mirror(&p);
            }
            6 => {
                let p = self.pnt();
                let d = self.dir()?;
                t.set_mirror(&Ax2::from_direction(p, d));
            }
            7 | 8 => {
                let (x, y, z, w) = (self.f(), self.f(), self.f(), self.f());
                let q = Quaternion::from_components(x, y, z, w);
                t.set_rotation_quaternion(&q)?;
                if kind == 8 {
                    let v = self.vec();
                    let factor = self.f();
                    t.set_translation_part(&v);
                    t.set_scale_factor(factor)?;
                }
            }
            _ => panic!("unknown transformation recipe {kind}"),
        }
        Ok(t)
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread inputs");
    }
}

// The shared bodies of the motion families of the curves and surfaces. The object has been read
// by the caller; the rest of the inputs are read here in the oracle's order. All copy forms are
// replayed; of the in-place forms `translate_points` always, the others as selected by `sel`
// (0 the three mirrors, 1 scale and transform, otherwise translate by a vector).
macro_rules! motion_exact {
    ($g:expr, $r:expr, $o:expr, $put:ident) => {{
        let cen = $r.pnt();
        let mx1 = $r.ax1()?;
        let mx2 = $r.ax2()?;
        let sp = $r.pnt();
        let ss = $r.f();
        let tr = $r.trsf()?;
        let dv = $r.vec();
        let (p1, p2) = ($r.pnt(), $r.pnt());
        let sel = $r.i();
        let o = $o;
        $g.$put(&o.mirrored(&cen));
        $g.$put(&o.mirrored(&mx1));
        $g.$put(&o.mirrored(&mx2));
        $g.$put(&o.scaled(&sp, ss));
        $g.$put(&o.transformed(&tr));
        $g.$put(&o.translated(&dv));
        $g.$put(&o.translated_points(&p1, &p2));
        let mut t = o;
        t.translate_points(&p1, &p2);
        $g.$put(&t);
        match sel {
            0 => {
                t = o;
                t.mirror(&cen);
                $g.$put(&t);
                t = o;
                t.mirror(&mx1);
                $g.$put(&t);
                t = o;
                t.mirror(&mx2);
                $g.$put(&t);
            }
            1 => {
                t = o;
                t.scale(&sp, ss);
                $g.$put(&t);
                t = o;
                t.transform(&tr);
                $g.$put(&t);
            }
            _ => {
                t = o;
                t.translate(&dv);
                $g.$put(&t);
            }
        }
    }};
}
macro_rules! motion_rotate {
    ($g:expr, $r:expr, $o:expr, $put:ident) => {{
        let rax = $r.ax1()?;
        let ang = $r.f();
        let tr = $r.trsf()?;
        let o = $o;
        $g.$put(&o.rotated(&rax, ang));
        let mut t = o;
        t.rotate(&rax, ang);
        $g.$put(&t);
        $g.$put(&o.transformed(&tr));
        t = o;
        t.transform(&tr);
        $g.$put(&t);
    }};
}

// ---------------------------------------------------------------------------------
// Replaying one family

fn run(name: &str, r: &mut Rd, g: &mut Got) -> Result<(), Thrown> {
    match name {
        // ---- gp_Lin ----
        "lin_constructors" => {
            let (k, a1, np, nd) = (r.lin()?, r.ax1()?, r.pnt(), r.dir()?);
            g.lin(&Lin::default());
            g.lin(&k);
            g.lin(&Lin::from_axis(a1));
            g.lin(&Lin::new(np, nd));
        }
        "lin_access" => {
            let (l, nl, a1, nd) = (r.lin()?, r.pnt(), r.ax1()?, r.dir()?);
            g.pnt(l.location());
            g.dir(l.direction());
            g.ax1(l.position());
            g.lin(&l.reversed());
            let mut t = l;
            t.reverse();
            g.lin(&t);
            t = l;
            t.set_location(nl);
            g.lin(&t);
            t = l;
            t.set_position(a1);
            g.lin(&t);
            t = l;
            t.set_direction(nd);
            g.lin(&t);
        }
        "lin_point_queries" => {
            let (l, pt, onl, lt) = (r.lin()?, r.pnt(), r.pnt(), r.f());
            g.f(l.distance(&pt));
            g.f(l.square_distance(&pt));
            g.b(l.contains(&pt, lt));
            g.f(l.distance(&onl));
            g.f(l.square_distance(&onl));
            g.b(l.contains(&onl, 1e-6));
        }
        "lin_normal" => {
            let (l, pt) = (r.lin()?, r.pnt());
            g.lin(&l.normal(&pt)?);
        }
        "lin_line_distance" => {
            let (l1, l2) = (r.lin()?, r.lin()?);
            g.f(l1.distance(&l2));
            g.f(l1.square_distance(&l2));
            g.f(l2.distance(&l1));
        }
        "lin_angle" => {
            let (l1, l2) = (r.lin()?, r.lin()?);
            g.f(l1.angle(&l2));
            g.f(l2.angle(&l1));
        }
        "lin_motion" => {
            let l = r.lin()?;
            motion_exact!(g, r, l, lin);
        }
        "lin_rotate" => {
            let l = r.lin()?;
            motion_rotate!(g, r, l, lin);
        }

        // ---- gp_Circ ----
        "circ_constructors" => {
            let (k, a1) = (r.circ()?, r.ax2()?);
            g.circ(&Circ::default());
            g.circ(&k);
            g.circ(&Circ::new(a1, k.radius())?);
        }
        "circ_radius_checks" => {
            let (a1, rneg, w) = (r.ax2()?, r.f(), r.i());
            if w == 0 {
                g.circ(&Circ::new(a1, rneg)?);
            } else {
                let mut t = Circ::new(a1, 1.0)?;
                t.set_radius(rneg)?;
                g.circ(&t);
            }
        }
        "circ_access" => {
            let (c0, nl, a1, nx, nr) = (r.circ()?, r.pnt(), r.ax2()?, r.ax1()?, r.f());
            g.pnt(c0.location());
            g.f(c0.radius());
            g.ax2(c0.position());
            g.axes(&c0.axis(), &c0.x_axis(), &c0.y_axis());
            g.fs(&[c0.area(), c0.length()]);
            let mut t = c0;
            t.set_location(nl);
            g.circ(&t);
            t = c0;
            t.set_position(a1);
            g.circ(&t);
            t = c0;
            t.set_axis(nx)?;
            g.circ(&t);
            t = c0;
            t.set_radius(nr)?;
            g.circ(&t);
        }
        "circ_queries" => {
            let (c, pt, onc, lt) = (r.circ()?, r.pnt(), r.pnt(), r.f());
            g.b(c.contains(&pt, lt));
            g.f(c.distance(&pt));
            g.f(c.square_distance(&pt));
            g.b(c.contains(&onc, 1e-6));
            g.f(c.distance(&onc));
            g.f(c.square_distance(c.location()));
        }
        "circ_motion" => {
            let k = r.circ()?;
            motion_exact!(g, r, k, circ);
        }
        "circ_rotate" => {
            let k = r.circ()?;
            motion_rotate!(g, r, k, circ);
        }

        // ---- gp_Elips ----
        "elips_constructors" => {
            let (k, a1) = (r.elips()?, r.ax2()?);
            g.elips(&Elips::default());
            g.elips(&k);
            g.elips(&Elips::new(a1, k.major_radius(), k.minor_radius())?);
        }
        "elips_radius_checks" => {
            let (a1, mj, mn, w) = (r.ax2()?, r.f(), r.f(), r.i());
            if w == 0 {
                g.elips(&Elips::new(a1, mj, mn)?);
            } else if w == 1 {
                let mut t = Elips::new(a1, 3.0, 2.0)?;
                t.set_major_radius(mj)?;
                g.elips(&t);
            } else {
                let mut t = Elips::new(a1, 3.0, 2.0)?;
                t.set_minor_radius(mn)?;
                g.elips(&t);
            }
        }
        "elips_access" => {
            let (e0, nl, a1, nx, nmaj, nmin) = (r.elips()?, r.pnt(), r.ax2()?, r.ax1()?, r.f(), r.f());
            g.pnt(e0.location());
            g.fs(&[e0.major_radius(), e0.minor_radius()]);
            g.ax2(e0.position());
            g.axes(&e0.axis(), &e0.x_axis(), &e0.y_axis());
            g.fs(&[e0.area(), e0.focal(), e0.parameter(), e0.eccentricity()]);
            g.pnt(&e0.focus1());
            g.pnt(&e0.focus2());
            let mut t = e0;
            t.set_location(nl);
            g.elips(&t);
            t = e0;
            t.set_position(a1);
            g.elips(&t);
            t = e0;
            t.set_axis(nx)?;
            g.elips(&t);
            t = e0;
            t.set_major_radius(nmaj)?;
            g.elips(&t);
            t = e0;
            t.set_minor_radius(nmin)?;
            g.elips(&t);
        }
        "elips_directrix" => {
            let k = r.elips()?;
            g.ax1(&k.directrix1()?);
            g.ax1(&k.directrix2()?);
        }
        "elips_motion" => {
            let k = r.elips()?;
            motion_exact!(g, r, k, elips);
        }
        "elips_rotate" => {
            let k = r.elips()?;
            motion_rotate!(g, r, k, elips);
        }

        // ---- gp_Hypr ----
        "hypr_constructors" => {
            let (k, a1) = (r.hypr()?, r.ax2()?);
            g.hypr(&Hypr::default());
            g.hypr(&k);
            g.hypr(&Hypr::new(a1, k.major_radius(), k.minor_radius())?);
        }
        "hypr_radius_checks" => {
            let (a1, mj, mn, w) = (r.ax2()?, r.f(), r.f(), r.i());
            if w == 0 {
                g.hypr(&Hypr::new(a1, mj, mn)?);
            } else if w == 1 {
                let mut t = Hypr::new(a1, 3.0, 2.0)?;
                t.set_major_radius(mj)?;
                g.hypr(&t);
            } else {
                let mut t = Hypr::new(a1, 3.0, 2.0)?;
                t.set_minor_radius(mn)?;
                g.hypr(&t);
            }
        }
        "hypr_access" => {
            let (e0, nl, a1, nx, nmaj, nmin) = (r.hypr()?, r.pnt(), r.ax2()?, r.ax1()?, r.f(), r.f());
            g.pnt(e0.location());
            g.fs(&[e0.major_radius(), e0.minor_radius()]);
            g.ax2(e0.position());
            g.axes(&e0.axis(), &e0.x_axis(), &e0.y_axis());
            g.f(e0.focal());
            g.pnt(&e0.focus1());
            g.pnt(&e0.focus2());
            g.hypr(&e0.conjugate_branch1()?);
            g.hypr(&e0.conjugate_branch2()?);
            g.hypr(&e0.other_branch()?);
            let mut t = e0;
            t.set_location(nl);
            g.hypr(&t);
            t = e0;
            t.set_position(a1);
            g.hypr(&t);
            t = e0;
            t.set_axis(nx)?;
            g.hypr(&t);
            t = e0;
            t.set_major_radius(nmaj)?;
            g.hypr(&t);
            t = e0;
            t.set_minor_radius(nmin)?;
            g.hypr(&t);
        }
        "hypr_default_branches" => {
            let (a1, w) = (r.ax2()?, r.i());
            let mut t = Hypr::default();
            t.set_position(a1);
            match w {
                0 => g.hypr(&t.conjugate_branch1()?),
                1 => g.hypr(&t.conjugate_branch2()?),
                _ => g.hypr(&t.other_branch()?),
            }
        }
        "hypr_eccentricity" => {
            let k = r.hypr()?;
            g.f(k.eccentricity()?);
            g.f(k.parameter()?);
        }
        "hypr_asymptotes" => {
            let k = r.hypr()?;
            g.ax1(&k.asymptote1()?);
            g.ax1(&k.asymptote2()?);
        }
        "hypr_directrix" => {
            let k = r.hypr()?;
            g.ax1(&k.directrix1()?);
            g.ax1(&k.directrix2()?);
        }
        "hypr_motion" => {
            let k = r.hypr()?;
            motion_exact!(g, r, k, hypr);
        }
        "hypr_rotate" => {
            let k = r.hypr()?;
            motion_rotate!(g, r, k, hypr);
        }

        // ---- gp_Parab ----
        "parab_constructors" => {
            let (k, a1) = (r.parab()?, r.ax2()?);
            g.parab(&Parab::default());
            g.parab(&k);
            g.parab(&Parab::new(a1, k.focal())?);
        }
        "parab_directrix_focus" => {
            let (dr, fo) = (r.ax1()?, r.pnt());
            g.parab(&Parab::from_directrix_focus(&dr, &fo)?);
        }
        "parab_focal_checks" => {
            let (a1, neg, w) = (r.ax2()?, r.f(), r.i());
            if w == 0 {
                g.parab(&Parab::new(a1, neg)?);
            } else {
                let mut t = Parab::new(a1, 1.0)?;
                t.set_focal(neg)?;
                g.parab(&t);
            }
        }
        "parab_access" => {
            let (e0, nl, a1, nx, nf) = (r.parab()?, r.pnt(), r.ax2()?, r.ax1()?, r.f());
            g.pnt(e0.location());
            g.fs(&[e0.focal(), e0.parameter()]);
            g.ax2(e0.position());
            g.axes(&e0.axis(), &e0.x_axis(), &e0.y_axis());
            g.ax1(&e0.directrix());
            g.pnt(&e0.focus());
            let mut t = e0;
            t.set_location(nl);
            g.parab(&t);
            t = e0;
            t.set_position(a1);
            g.parab(&t);
            t = e0;
            t.set_axis(nx)?;
            g.parab(&t);
            t = e0;
            t.set_focal(nf)?;
            g.parab(&t);
        }
        "parab_motion" => {
            let k = r.parab()?;
            motion_exact!(g, r, k, parab);
        }
        "parab_rotate" => {
            let k = r.parab()?;
            motion_rotate!(g, r, k, parab);
        }

        // ---- gp_Pln ----
        "pln_constructors" => {
            let (k, a3, np, nv) = (r.pln()?, r.ax3()?, r.pnt(), r.dir()?);
            g.pln(&Pln::default());
            g.pln(&k);
            g.pln(&Pln::new(a3));
            g.pln(&Pln::from_point_normal(np, nv));
        }
        "pln_from_coefficients" => {
            let (a, b, c, d) = (r.f(), r.f(), r.f(), r.f());
            g.pln(&Pln::from_coefficients(a, b, c, d)?);
        }
        "pln_access" => {
            let (p0, nl, a3, nx) = (r.pln()?, r.pnt(), r.ax3()?, r.ax1()?);
            g.pnt(p0.location());
            g.ax3(p0.position());
            g.axes(&p0.axis(), &p0.x_axis(), &p0.y_axis());
            g.b(p0.direct());
            let (ca, cb, cc, cd) = p0.coefficients();
            g.fs(&[ca, cb, cc, cd]);
            let mut t = p0;
            t.set_location(nl);
            g.pln(&t);
            t = p0;
            t.set_position(a3);
            g.pln(&t);
            t = p0;
            t.set_axis(nx)?;
            g.pln(&t);
            t = p0;
            t.u_reverse();
            g.pln(&t);
            g.b(t.direct());
            t = p0;
            t.v_reverse();
            g.pln(&t);
            g.b(t.direct());
        }
        "pln_point_queries" => {
            let (p, pt, onp, lt) = (r.pln()?, r.pnt(), r.pnt(), r.f());
            g.f(p.signed_distance(&pt));
            g.f(p.distance(&pt));
            g.f(p.square_distance(&pt));
            g.b(p.contains(&pt, lt));
            g.f(p.signed_distance(&onp));
            g.f(p.distance(&onp));
            g.f(p.square_distance(&onp));
            g.b(p.contains(&onp, 1e-6));
        }
        "pln_line_queries" => {
            let (p, l2, lt, at) = (r.pln()?, r.lin()?, r.f(), r.f());
            g.f(p.signed_distance(&l2));
            g.f(p.distance(&l2));
            g.f(p.square_distance(&l2));
            g.b(p.contains_line(&l2, lt, at));
        }
        "pln_plane_queries" => {
            let (p, p2) = (r.pln()?, r.pln()?);
            g.f(p.signed_distance(&p2));
            g.f(p.distance(&p2));
            g.f(p.square_distance(&p2));
            g.f(p2.signed_distance(&p));
        }
        "pln_motion" => {
            let k = r.pln()?;
            motion_exact!(g, r, k, pln);
        }
        "pln_rotate" => {
            let k = r.pln()?;
            motion_rotate!(g, r, k, pln);
        }

        // ---- gp_Cylinder ----
        "cylinder_constructors" => {
            let (k, a1) = (r.cylinder()?, r.ax3()?);
            g.cylinder(&Cylinder::default());
            g.cylinder(&k);
            g.cylinder(&Cylinder::new(a1, k.radius())?);
        }
        "cylinder_radius_checks" => {
            let (a1, rneg, w) = (r.ax3()?, r.f(), r.i());
            if w == 0 {
                g.cylinder(&Cylinder::new(a1, rneg)?);
            } else {
                let mut t = Cylinder::new(a1, 1.0)?;
                t.set_radius(rneg)?;
                g.cylinder(&t);
            }
        }
        "cylinder_access" => {
            let (c0, nl, a3, nx, nr) = (r.cylinder()?, r.pnt(), r.ax3()?, r.ax1()?, r.f());
            g.pnt(c0.location());
            g.f(c0.radius());
            g.ax3(c0.position());
            g.axes(&c0.axis(), &c0.x_axis(), &c0.y_axis());
            g.b(c0.direct());
            let mut t = c0;
            t.set_location(nl);
            g.cylinder(&t);
            t = c0;
            t.set_position(a3);
            g.cylinder(&t);
            t = c0;
            t.set_axis(nx)?;
            g.cylinder(&t);
            t = c0;
            t.set_radius(nr)?;
            g.cylinder(&t);
            t = c0;
            t.u_reverse();
            g.cylinder(&t);
            g.b(t.direct());
            t = c0;
            t.v_reverse();
            g.cylinder(&t);
            g.b(t.direct());
        }
        "cylinder_coefficients" => {
            let k = r.cylinder()?;
            let (a1, a2, a3, b1, b2, b3, c1, c2, c3, d) = k.coefficients();
            g.fs(&[a1, a2, a3, b1, b2, b3, c1, c2, c3, d]);
        }
        "cylinder_motion" => {
            let k = r.cylinder()?;
            motion_exact!(g, r, k, cylinder);
        }
        "cylinder_rotate" => {
            let k = r.cylinder()?;
            motion_rotate!(g, r, k, cylinder);
        }

        // ---- gp_Cone ----
        "cone_constructors" => {
            let (k, a1) = (r.cone()?, r.ax3()?);
            g.cone(&Cone::default());
            g.cone(&k);
            g.cone(&Cone::new(a1, k.semi_angle(), k.ref_radius())?);
        }
        "cone_checks" => {
            let (a1, ang, rad, w) = (r.ax3()?, r.f(), r.f(), r.i());
            if w == 0 {
                g.cone(&Cone::new(a1, ang, rad)?);
            } else if w == 1 {
                let mut t = Cone::new(a1, 0.5, 1.0)?;
                t.set_semi_angle(ang)?;
                g.cone(&t);
            } else {
                let mut t = Cone::new(a1, 0.5, 1.0)?;
                t.set_radius(rad)?;
                g.cone(&t);
            }
        }
        "cone_access" => {
            let (c0, nl, a3, nx, nr, na) = (r.cone()?, r.pnt(), r.ax3()?, r.ax1()?, r.f(), r.f());
            g.pnt(c0.location());
            g.fs(&[c0.ref_radius(), c0.semi_angle()]);
            g.ax3(c0.position());
            g.axes(&c0.axis(), &c0.x_axis(), &c0.y_axis());
            g.b(c0.direct());
            let mut t = c0;
            t.set_location(nl);
            g.cone(&t);
            t = c0;
            t.set_position(a3);
            g.cone(&t);
            t = c0;
            t.set_axis(nx)?;
            g.cone(&t);
            t = c0;
            t.set_radius(nr)?;
            g.cone(&t);
            t = c0;
            t.set_semi_angle(na)?;
            g.cone(&t);
            t = c0;
            t.u_reverse();
            g.cone(&t);
            g.b(t.direct());
            t = c0;
            t.v_reverse();
            g.cone(&t);
            g.b(t.direct());
        }
        "cone_apex" => {
            let k = r.cone()?;
            g.pnt(&k.apex());
            let mut t = k;
            t.v_reverse();
            g.pnt(&t.apex());
        }
        "cone_coefficients" => {
            let k = r.cone()?;
            let (a1, a2, a3, b1, b2, b3, c1, c2, c3, d) = k.coefficients();
            g.fs(&[a1, a2, a3, b1, b2, b3, c1, c2, c3, d]);
        }
        "cone_motion" => {
            let k = r.cone()?;
            motion_exact!(g, r, k, cone);
        }
        "cone_rotate" => {
            let k = r.cone()?;
            motion_rotate!(g, r, k, cone);
        }

        // ---- gp_Sphere ----
        "sphere_constructors" => {
            let (k, a1) = (r.sphere()?, r.ax3()?);
            g.sphere(&Sphere::default());
            g.sphere(&k);
            g.sphere(&Sphere::new(a1, k.radius())?);
        }
        "sphere_radius_checks" => {
            let (a1, rneg, w) = (r.ax3()?, r.f(), r.i());
            if w == 0 {
                g.sphere(&Sphere::new(a1, rneg)?);
            } else {
                let mut t = Sphere::new(a1, 1.0)?;
                t.set_radius(rneg)?;
                g.sphere(&t);
            }
        }
        "sphere_access" => {
            let (c0, nl, a3, nr) = (r.sphere()?, r.pnt(), r.ax3()?, r.f());
            g.pnt(c0.location());
            g.f(c0.radius());
            g.ax3(c0.position());
            g.ax1(&c0.x_axis());
            g.ax1(&c0.y_axis());
            g.b(c0.direct());
            g.fs(&[c0.area(), c0.volume()]);
            let mut t = c0;
            t.set_location(nl);
            g.sphere(&t);
            t = c0;
            t.set_position(a3);
            g.sphere(&t);
            t = c0;
            t.set_radius(nr)?;
            g.sphere(&t);
            t = c0;
            t.u_reverse();
            g.sphere(&t);
            g.b(t.direct());
            t = c0;
            t.v_reverse();
            g.sphere(&t);
            g.b(t.direct());
        }
        "sphere_coefficients" => {
            let k = r.sphere()?;
            let (a1, a2, a3, b1, b2, b3, c1, c2, c3, d) = k.coefficients();
            g.fs(&[a1, a2, a3, b1, b2, b3, c1, c2, c3, d]);
        }
        "sphere_motion" => {
            let k = r.sphere()?;
            motion_exact!(g, r, k, sphere);
        }
        "sphere_rotate" => {
            let k = r.sphere()?;
            motion_rotate!(g, r, k, sphere);
        }

        // ---- gp_Torus ----
        "torus_constructors" => {
            let (k, a1) = (r.torus()?, r.ax3()?);
            g.torus(&Torus::default());
            g.torus(&k);
            g.torus(&Torus::new(a1, k.major_radius(), k.minor_radius())?);
        }
        "torus_radius_checks" => {
            let (a1, mj, mn, w) = (r.ax3()?, r.f(), r.f(), r.i());
            if w == 0 {
                g.torus(&Torus::new(a1, mj, mn)?);
            } else if w == 1 {
                let mut t = Torus::new(a1, 3.0, 1.0)?;
                t.set_major_radius(mj)?;
                g.torus(&t);
            } else {
                let mut t = Torus::new(a1, 3.0, 1.0)?;
                t.set_minor_radius(mn)?;
                g.torus(&t);
            }
        }
        "torus_access" => {
            let (c0, nl, a3, nx, nmaj, nmin) = (r.torus()?, r.pnt(), r.ax3()?, r.ax1()?, r.f(), r.f());
            g.pnt(c0.location());
            g.fs(&[c0.major_radius(), c0.minor_radius()]);
            g.ax3(c0.position());
            g.axes(&c0.axis(), &c0.x_axis(), &c0.y_axis());
            g.b(c0.direct());
            g.fs(&[c0.area(), c0.volume()]);
            let mut t = c0;
            t.set_location(nl);
            g.torus(&t);
            t = c0;
            t.set_position(a3);
            g.torus(&t);
            t = c0;
            t.set_axis(nx)?;
            g.torus(&t);
            t = c0;
            t.set_major_radius(nmaj)?;
            g.torus(&t);
            t = c0;
            t.set_minor_radius(nmin)?;
            g.torus(&t);
            t = c0;
            t.u_reverse();
            g.torus(&t);
            g.b(t.direct());
            t = c0;
            t.v_reverse();
            g.torus(&t);
            g.b(t.direct());
        }
        "torus_coefficients" => {
            let k = r.torus()?;
            g.fs(&k.coefficients());
        }
        "torus_motion" => {
            let k = r.torus()?;
            motion_exact!(g, r, k, torus);
        }
        "torus_rotate" => {
            let k = r.torus()?;
            motion_rotate!(g, r, k, torus);
        }
        _ => panic!("unknown oracle family {name}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Comparison

fn ulps(a: f64, b: f64) -> u64 {
    let key = |x: f64| {
        let n = x.to_bits() as i64;
        if n < 0 { i64::MIN - n } else { n }
    };
    key(a).abs_diff(key(b))
}

#[derive(Default)]
struct Tally {
    values: usize,
    exact: usize,
    worst: u64,
}

/// Compares the expected words of a line with the replayed values. Floats are compared bit
/// for bit for `EXACT` families and within `MAX_ULPS` of the line's largest magnitude
/// otherwise (zero tolerance when that magnitude is infinite); bools always exactly.
fn compare(tally: &mut Tally, line: &str, name: &str, want: &[&str], got: &[Tok]) {
    assert_eq!(want.len(), got.len(), "value count differs\n{line}");
    let exact = EXACT.contains(&name);
    let mut scale = 1.0_f64;
    for (w, t) in want.iter().zip(got) {
        if let Tok::F(_) = t {
            scale = scale.max(bits(w).abs());
        }
    }
    let tol = if scale.is_finite() { MAX_ULPS * f64::EPSILON * scale } else { 0.0 };
    for (index, (w, t)) in want.iter().zip(got).enumerate() {
        match *t {
            Tok::F(g) => {
                let w = bits(w);
                let d = ulps(w, g);
                let ok =
                    if exact { w.to_bits() == g.to_bits() } else { (w - g).abs() <= tol || w.to_bits() == g.to_bits() };
                assert!(ok, "value {index}: want {w:e}, got {g:e} ({d} ulps)\n{line}");
                tally.values += 1;
                tally.exact += usize::from(d == 0);
                tally.worst = tally.worst.max(d);
            }
            Tok::B(g) => assert_eq!(*w, if g { "1" } else { "0" }, "value {index} (bool)\n{line}"),
        }
    }
}

// ---------------------------------------------------------------------------------

#[test]
fn gpcs_matches_occt() {
    let text = include_str!("gpcs.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in gpcs.txt:\n{line}");
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
    // Every family is still covered, and every EXACT entry names a real family.
    let present: BTreeSet<&str> = stats.keys().copied().collect();
    let expected: BTreeSet<&str> = FAMILIES.iter().copied().collect();
    assert_eq!(present, expected, "family list changed");
    for name in EXACT {
        assert!(expected.contains(name), "EXACT names an unknown family {name}");
    }

    assert_eq!(lines, text.lines().count());
    assert_eq!(lines, GOLDEN_LINES, "golden line count changed");
    println!("{lines} lines, {} values, {} bit-identical, worst {} ulps", tally.values, tally.exact, tally.worst);
}
