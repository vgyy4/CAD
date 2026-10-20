//! Numerical reference test for OCCT 8.0.1's package `Convert`: the conic converters (`Convert_ConicToBSplineCurve` and its
//! circle / ellipse / hyperbola / parabola subclasses, `BuildCosAndSin`, `BuildPolynomialCosAndSin`), the elementary-surface
//! converters (cylinder, cone, sphere, torus), `Convert_CompBezierCurvesToBSplineCurve` (3D and 2D),
//! `Convert_CompPolynomialToPoles` and `Convert_GridPolynomialToPoles`.
//!
//! `convert.txt` is printed by `oracle/convert.cpp` (real OCCT). Each line is `<family> <inputs...> | <outputs...>`: f64
//! values are 16-hex-digit bit patterns, ints are decimal and bools are 0/1. Every input of a call is on the left of ` | `;
//! this test reads them back with the `Rd` reader methods of the same name as the oracle's schema words (`f i iv fv ipairs fr2
//! fr3 conic surf bzc2 bzc3`, grammars in docs/api-convert.md) and replays the call on the Rust API of docs/api-convert.md.
//! The right side is the expected values, `throws` (OCCT raised an exception) or `unchecked` (OCCT 8.0.1 as built has that check
//! compiled out, or the result is unspecified; the Rust API must return an error). No family prints a text, so there is no text
//! token to escape; the only text value is the `unchecked` marker that follows a not-done grid converter.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::convert::{
    CompBezierCurves2dToBSplineCurve2d, CompBezierCurvesToBSplineCurve, CompPolynomialToPoles, ConicToBSplineCurve,
    ConvertError, CosAndSin, ElementarySurfaceToBSplineSurface, GridPolynomialToPoles, ParameterisationType,
    build_polynomial_cos_and_sin,
};
use occt_port::gp::{
    self, Ax2d, Ax3, Circ2d, Cone, Cylinder, Dir, Dir2d, Elips2d, Hypr2d, Parab2d, Pnt, Pnt2d, Sphere, Torus,
};
use occt_port::mathlin::MathError;
use occt_port::plib::Array2;
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for the families that use sin / cos / tan / sinh / cosh / atan2 through the C library, in units
/// of f64::EPSILON times the largest absolute value on the line (at least 1; zero tolerance if that value is infinite). The
/// oracle only prints cases whose outputs stay within 2 ulps when every such result is moved by an ulp.
const MAX_ULPS: f64 = 8.0;

/// Every family with inputs must have at least this many distinct lines (except the `SMALL_DOMAIN` ones).
const MIN_DISTINCT: usize = 16;

// BEGIN GENERATED
/// Families that really take no input: they are printed exactly once.
const INPUT_LESS: &[&str] = &[];

/// Families whose whole input domain is printed (so they are exempt from MIN_DISTINCT): name and number of lines.
const SMALL_DOMAIN: &[(&str, usize)] = &[("cos_sin_periodic", 8)];

/// Families compared bit for bit: only + - * / sqrt, comparisons and integers (no C library function). The others use MAX_ULPS.
const EXACT: &[&str] = &["cpoly_array", "cpoly_single", "cpoly_uniform", "gpoly_multi", "gpoly_single", "parabola"];

/// Every family the oracle must print (a family that disappears from convert.txt fails).
const FAMILIES: &[&str] = &[
    "bezier2d",
    "bezier3d",
    "circle_full",
    "circle_trim",
    "cone_full",
    "cone_trim",
    "conic_index",
    "cos_sin_periodic",
    "cos_sin_trim",
    "cpoly_array",
    "cpoly_single",
    "cpoly_uniform",
    "cylinder_full",
    "cylinder_trim",
    "ellipse_full",
    "ellipse_trim",
    "gpoly_multi",
    "gpoly_single",
    "hyperbola",
    "parabola",
    "poly_cos_sin",
    "sphere_full",
    "sphere_trim2",
    "sphere_trim4",
    "surface_index",
    "torus_full",
    "torus_trim2",
    "torus_trim4",
];

/// The number of lines of tests/convert.txt.
const GOLDEN_LINES: usize = 1106;
// END GENERATED

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

impl From<MathError> for Thrown {
    fn from(_: MathError) -> Self {
        Thrown
    }
}

impl From<ConvertError> for Thrown {
    fn from(_: ConvertError) -> Self {
        Thrown
    }
}

#[derive(Clone, Debug)]
enum Tok {
    F(f64),
    B(bool),
    I(i64),
    /// A text value (only the `unchecked` marker of a not-done grid converter).
    S(String),
}

/// Collects the values produced by the Rust side, in the order the oracle printed them.
#[derive(Default)]
struct Got(Vec<Tok>);

impl Got {
    fn f(&mut self, v: f64) {
        self.0.push(Tok::F(v));
    }
    fn b(&mut self, v: bool) {
        self.0.push(Tok::B(v));
    }
    fn i(&mut self, v: i32) {
        self.0.push(Tok::I(i64::from(v)));
    }
    fn l(&mut self, v: usize) {
        self.0.push(Tok::I(v as i64));
    }
    /// `o_a1` of the oracle: lower bound (always 1), upper bound, the values.
    fn a1(&mut self, v: &[f64]) {
        self.i(1);
        self.l(v.len());
        for x in v {
            self.f(*x);
        }
    }
    fn a1i(&mut self, v: &[i32]) {
        self.i(1);
        self.l(v.len());
        for x in v {
            self.i(*x);
        }
    }
    fn a1p2(&mut self, v: &[Pnt2d]) {
        self.i(1);
        self.l(v.len());
        for p in v {
            self.f(p.x());
            self.f(p.y());
        }
    }
    /// `o_a2` of the oracle: lower and upper row, lower and upper column, the values row by row.
    fn a2(&mut self, a: &Array2<f64>) {
        self.i(1);
        self.l(a.rows());
        self.i(1);
        self.l(a.cols());
        for x in a.as_slice() {
            self.f(*x);
        }
    }
    fn a2p(&mut self, a: &Array2<Pnt>) {
        self.i(1);
        self.l(a.rows());
        self.i(1);
        self.l(a.cols());
        for p in a.as_slice() {
            self.f(p.x());
            self.f(p.y());
            self.f(p.z());
        }
    }
    /// `put_cs` of the oracle.
    fn cs(&mut self, c: &CosAndSin) {
        self.a1(&c.cos_numerator);
        self.a1(&c.sin_numerator);
        self.a1(&c.denominator);
        self.i(c.degree);
        self.a1(&c.knots);
        self.a1i(&c.mults);
    }
    /// `put_curve` of the oracle.
    fn curve(&mut self, c: &ConicToBSplineCurve) {
        self.i(c.degree());
        self.i(c.nb_poles());
        self.i(c.nb_knots());
        self.b(c.is_periodic());
        self.a1p2(c.poles());
        self.a1(c.weights());
        self.a1(c.knots());
        self.a1i(c.multiplicities());
    }
    /// `put_surface` of the oracle.
    fn surface(&mut self, s: &ElementarySurfaceToBSplineSurface) {
        self.i(s.u_degree());
        self.i(s.v_degree());
        self.i(s.nb_u_poles());
        self.i(s.nb_v_poles());
        self.i(s.nb_u_knots());
        self.i(s.nb_v_knots());
        self.b(s.is_u_periodic());
        self.b(s.is_v_periodic());
        self.a2p(s.poles());
        self.a2(s.weights());
        self.a1(s.u_knots());
        self.a1(s.v_knots());
        self.a1i(s.u_multiplicities());
        self.a1i(s.v_multiplicities());
    }
    /// `put_cp` of the oracle.
    fn cpoly(&mut self, p: &CompPolynomialToPoles) {
        self.b(p.is_done());
        self.i(p.degree());
        self.i(p.nb_poles());
        self.a2(p.poles());
        self.i(p.nb_knots());
        self.a1(p.knots());
        self.a1i(p.multiplicities());
    }
}

// ---------------------------------------------------------------------------------
// Recipes read from the inputs of a line

fn bits(w: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(w, 16).unwrap_or_else(|e| panic!("bad hex word {w}: {e}")))
}

struct Rd<'a> {
    words: &'a [&'a str],
    at: usize,
}

/// `fr2`: origin, raw direction, sense.
#[derive(Clone, Copy)]
struct Fr2 {
    o: [f64; 2],
    d: [f64; 2],
    sense: bool,
}

/// `fr3`: origin, raw normal, raw x direction, indirect flag.
#[derive(Clone, Copy)]
struct Fr3 {
    o: [f64; 3],
    n: [f64; 3],
    v: [f64; 3],
    indirect: bool,
}

/// `conic`.
#[derive(Clone, Copy)]
struct ConicSpec {
    kind: i32,
    f: Fr2,
    a: f64,
    b: f64,
    trim: bool,
    pt: i32,
    u1: f64,
    u2: f64,
}

/// `surf`.
#[derive(Clone, Copy)]
struct SurfSpec {
    kind: i32,
    f: Fr3,
    a: f64,
    b: f64,
    u1: f64,
    u2: f64,
    v1: f64,
    v2: f64,
    ut: bool,
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
    fn count(&mut self) -> usize {
        self.word().parse().expect("count input")
    }
    fn iv(&mut self) -> Vec<i32> {
        let n = self.count();
        (0..n).map(|_| self.i()).collect()
    }
    fn fv(&mut self) -> Vec<f64> {
        let n = self.count();
        (0..n).map(|_| self.f()).collect()
    }
    /// A count of rows, then two integers per row.
    fn ipairs(&mut self) -> Vec<[i32; 2]> {
        let n = self.count();
        (0..n)
            .map(|_| {
                let a = self.i();
                let b = self.i();
                [a, b]
            })
            .collect()
    }
    fn fr2(&mut self) -> Fr2 {
        let o = [self.f(), self.f()];
        let d = [self.f(), self.f()];
        Fr2 { o, d, sense: self.i() != 0 }
    }
    fn fr3(&mut self) -> Fr3 {
        let o = [self.f(), self.f(), self.f()];
        let n = [self.f(), self.f(), self.f()];
        let v = [self.f(), self.f(), self.f()];
        Fr3 { o, n, v, indirect: self.i() != 0 }
    }
    fn conic(&mut self) -> ConicSpec {
        let kind = self.i();
        let f = self.fr2();
        let mut c = ConicSpec { kind, f, a: 0.0, b: 0.0, trim: false, pt: 0, u1: 0.0, u2: 0.0 };
        match kind {
            0 => {
                c.a = self.f();
                c.trim = self.i() != 0;
                c.pt = self.i();
                if c.trim {
                    c.u1 = self.f();
                    c.u2 = self.f();
                }
            }
            1 => {
                c.a = self.f();
                c.b = self.f();
                c.trim = self.i() != 0;
                c.pt = self.i();
                if c.trim {
                    c.u1 = self.f();
                    c.u2 = self.f();
                }
            }
            2 => {
                c.a = self.f();
                c.b = self.f();
                c.u1 = self.f();
                c.u2 = self.f();
            }
            3 => {
                c.a = self.f();
                c.u1 = self.f();
                c.u2 = self.f();
            }
            _ => panic!("unknown conic kind {kind}"),
        }
        c
    }
    fn surf(&mut self) -> SurfSpec {
        let kind = self.i();
        let f = self.fr3();
        let mut s = SurfSpec { kind, f, a: 0.0, b: 0.0, u1: 0.0, u2: 0.0, v1: 0.0, v2: 0.0, ut: false };
        match kind {
            0 | 4 => {
                s.a = self.f();
                s.u1 = self.f();
                s.u2 = self.f();
                s.v1 = self.f();
                s.v2 = self.f();
            }
            1 => {
                s.a = self.f();
                s.v1 = self.f();
                s.v2 = self.f();
            }
            2 | 7 => {
                s.a = self.f();
                s.b = self.f();
                s.u1 = self.f();
                s.u2 = self.f();
                s.v1 = self.f();
                s.v2 = self.f();
            }
            3 => {
                s.a = self.f();
                s.b = self.f();
                s.v1 = self.f();
                s.v2 = self.f();
            }
            5 => {
                s.a = self.f();
                s.u1 = self.f();
                s.u2 = self.f();
                s.ut = self.i() != 0;
            }
            6 => s.a = self.f(),
            8 => {
                s.a = self.f();
                s.b = self.f();
                s.u1 = self.f();
                s.u2 = self.f();
                s.ut = self.i() != 0;
            }
            9 => {
                s.a = self.f();
                s.b = self.f();
            }
            _ => panic!("unknown surf kind {kind}"),
        }
        s
    }
    /// A list of Bezier curves: a count, then per curve its number of poles and `dim` coordinates per pole.
    fn bzc(&mut self, dim: usize) -> Vec<Vec<f64>> {
        let n = self.count();
        (0..n)
            .map(|_| {
                let np = self.count();
                (0..np * dim).map(|_| self.f()).collect()
            })
            .collect()
    }
    /// All words must have been consumed.
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread input words");
    }
}

fn ptype(i: i32) -> ParameterisationType {
    match i {
        0 => ParameterisationType::TgtThetaOver2,
        1 => ParameterisationType::TgtThetaOver2_1,
        2 => ParameterisationType::TgtThetaOver2_2,
        3 => ParameterisationType::TgtThetaOver2_3,
        4 => ParameterisationType::TgtThetaOver2_4,
        5 => ParameterisationType::QuasiAngular,
        6 => ParameterisationType::RationalC1,
        7 => ParameterisationType::Polynomial,
        _ => panic!("unknown parametrisation type {i}"),
    }
}

fn ax2d(f: &Fr2) -> Result<Ax2d, Thrown> {
    Ok(Ax2d::new(Pnt2d::new(f.o[0], f.o[1]), Dir2d::new(f.d[0], f.d[1])?))
}

fn ax3(f: &Fr3) -> Result<Ax3, Thrown> {
    let mut a = Ax3::new(
        Pnt::new(f.o[0], f.o[1], f.o[2]),
        Dir::new(f.n[0], f.n[1], f.n[2])?,
        Dir::new(f.v[0], f.v[1], f.v[2])?,
    )?;
    if f.indirect {
        a.y_reverse();
    }
    Ok(a)
}

/// The conic converter of a recipe (the `gp` object first: its constructor can fail).
fn mk_conic(c: &ConicSpec) -> Result<ConicToBSplineCurve, Thrown> {
    let ax = ax2d(&c.f)?;
    let pt = ptype(c.pt);
    Ok(match c.kind {
        0 => {
            let g = Circ2d::new(ax, c.a, c.f.sense)?;
            if c.trim {
                ConicToBSplineCurve::circle_trimmed(&g, c.u1, c.u2, pt)?
            } else {
                ConicToBSplineCurve::circle(&g, pt)?
            }
        }
        1 => {
            let g = Elips2d::new(ax, c.a, c.b, c.f.sense)?;
            if c.trim {
                ConicToBSplineCurve::ellipse_trimmed(&g, c.u1, c.u2, pt)?
            } else {
                ConicToBSplineCurve::ellipse(&g, pt)?
            }
        }
        2 => {
            let g = Hypr2d::new(ax, c.a, c.b, c.f.sense)?;
            ConicToBSplineCurve::hyperbola(&g, c.u1, c.u2)?
        }
        _ => {
            let g = Parab2d::new(ax, c.a, c.f.sense)?;
            ConicToBSplineCurve::parabola(&g, c.u1, c.u2)?
        }
    })
}

/// The surface converter of a recipe.
fn mk_surf(s: &SurfSpec) -> Result<ElementarySurfaceToBSplineSurface, Thrown> {
    let ax = ax3(&s.f)?;
    Ok(match s.kind {
        0 => ElementarySurfaceToBSplineSurface::cylinder_trimmed(&Cylinder::new(ax, s.a)?, s.u1, s.u2, s.v1, s.v2)?,
        1 => ElementarySurfaceToBSplineSurface::cylinder(&Cylinder::new(ax, s.a)?, s.v1, s.v2)?,
        2 => ElementarySurfaceToBSplineSurface::cone_trimmed(&Cone::new(ax, s.a, s.b)?, s.u1, s.u2, s.v1, s.v2)?,
        3 => ElementarySurfaceToBSplineSurface::cone(&Cone::new(ax, s.a, s.b)?, s.v1, s.v2)?,
        4 => ElementarySurfaceToBSplineSurface::sphere_trimmed(&Sphere::new(ax, s.a)?, s.u1, s.u2, s.v1, s.v2)?,
        5 => ElementarySurfaceToBSplineSurface::sphere_param(&Sphere::new(ax, s.a)?, s.u1, s.u2, s.ut)?,
        6 => ElementarySurfaceToBSplineSurface::sphere(&Sphere::new(ax, s.a)?),
        7 => ElementarySurfaceToBSplineSurface::torus_trimmed(&Torus::new(ax, s.a, s.b)?, s.u1, s.u2, s.v1, s.v2)?,
        8 => ElementarySurfaceToBSplineSurface::torus_param(&Torus::new(ax, s.a, s.b)?, s.u1, s.u2, s.ut)?,
        _ => ElementarySurfaceToBSplineSurface::torus(&Torus::new(ax, s.a, s.b)?),
    })
}

fn pairs(v: &[f64]) -> Vec<[f64; 2]> {
    v.chunks(2).map(|c| [c[0], c[1]]).collect()
}

/// Replays the Bezier program of the oracle (`run_bezier`): the first `split` curves, `perform`, the rest, `perform`.
fn run_bezier3(g: &mut Got, tol: f64, cs: &[Vec<f64>], split: usize) -> Result<(), Thrown> {
    let mut c = CompBezierCurvesToBSplineCurve::new(tol);
    let pts = |q: &Vec<f64>| -> Vec<Pnt> { q.chunks(3).map(|p| Pnt::new(p[0], p[1], p[2])).collect() };
    let block = |c: &mut CompBezierCurvesToBSplineCurve, g: &mut Got| -> Result<(), Thrown> {
        c.perform()?;
        g.i(c.degree());
        g.i(c.nb_poles());
        g.i(c.nb_knots());
        for p in c.poles() {
            g.f(p.x());
            g.f(p.y());
            g.f(p.z());
        }
        let (k, m) = c.knots_and_mults();
        for x in k {
            g.f(x);
        }
        for x in m {
            g.i(x);
        }
        Ok(())
    };
    for q in &cs[..split] {
        c.add_curve(&pts(q))?;
    }
    block(&mut c, g)?;
    for q in &cs[split..] {
        c.add_curve(&pts(q))?;
    }
    block(&mut c, g)
}

fn run_bezier2(g: &mut Got, tol: f64, cs: &[Vec<f64>], split: usize) -> Result<(), Thrown> {
    let mut c = CompBezierCurves2dToBSplineCurve2d::new(tol);
    let pts = |q: &Vec<f64>| -> Vec<Pnt2d> { q.chunks(2).map(|p| Pnt2d::new(p[0], p[1])).collect() };
    let block = |c: &mut CompBezierCurves2dToBSplineCurve2d, g: &mut Got| -> Result<(), Thrown> {
        c.perform()?;
        g.i(c.degree());
        g.i(c.nb_poles());
        g.i(c.nb_knots());
        for p in c.poles() {
            g.f(p.x());
            g.f(p.y());
        }
        let (k, m) = c.knots_and_mults();
        for x in k {
            g.f(x);
        }
        for x in m {
            g.i(x);
        }
        Ok(())
    };
    for q in &cs[..split] {
        c.add_curve(&pts(q))?;
    }
    block(&mut c, g)?;
    for q in &cs[split..] {
        c.add_curve(&pts(q))?;
    }
    block(&mut c, g)
}

/// Outputs of a grid converter; a not-done converter (never printed by OCCT 8.0.1: its interpolation raises) must return errors.
fn put_grid(g: &mut Got, p: &GridPolynomialToPoles) -> Result<(), Thrown> {
    g.b(p.is_done());
    if !p.is_done() {
        assert!(
            p.nb_u_poles().is_err() && p.poles().is_err() && p.u_knots().is_err(),
            "accessors of a not-done converter"
        );
        g.0.push(Tok::S("unchecked".to_string()));
        return Ok(());
    }
    g.i(p.u_degree()?);
    g.i(p.v_degree()?);
    g.i(p.nb_u_poles()?);
    g.i(p.nb_v_poles()?);
    g.i(p.nb_u_knots()?);
    g.i(p.nb_v_knots()?);
    g.a2p(p.poles()?);
    g.a1(p.u_knots()?);
    g.a1(p.v_knots()?);
    g.a1i(p.u_multiplicities()?);
    g.a1i(p.v_multiplicities()?);
    Ok(())
}

// ---------------------------------------------------------------------------------
// The families

fn run(name: &str, r: &mut Rd, g: &mut Got) -> Result<(), Thrown> {
    match name {
        "circle_full" | "circle_trim" | "ellipse_full" | "ellipse_trim" | "hyperbola" | "parabola" => {
            let c = r.conic();
            g.curve(&mk_conic(&c)?);
        }
        "conic_index" => {
            let c = r.conic();
            let acc = r.i();
            let idx = r.i();
            let k = mk_conic(&c)?;
            match acc {
                0 => {
                    let p = k.pole(idx)?;
                    g.f(p.x());
                    g.f(p.y());
                }
                1 => g.f(k.weight(idx)?),
                2 => g.f(k.knot(idx)?),
                _ => g.i(k.multiplicity(idx)?),
            }
        }
        "cos_sin_periodic" => {
            let pt = r.i();
            g.cs(&ConicToBSplineCurve::build_cos_and_sin_periodic(ptype(pt))?);
        }
        "cos_sin_trim" => {
            let pt = r.i();
            let u1 = r.f();
            let u2 = r.f();
            g.cs(&ConicToBSplineCurve::build_cos_and_sin(ptype(pt), u1, u2)?);
        }
        "poly_cos_sin" => {
            let u1 = r.f();
            let u2 = r.f();
            let np = r.i();
            let (c, s, d) = build_polynomial_cos_and_sin(u1, u2, np)?;
            g.a1(&c);
            g.a1(&s);
            g.a1(&d);
        }
        "cylinder_trim" | "cylinder_full" | "cone_trim" | "cone_full" | "sphere_trim4" | "sphere_trim2"
        | "sphere_full" | "torus_trim4" | "torus_trim2" | "torus_full" => {
            let s = r.surf();
            g.surface(&mk_surf(&s)?);
        }
        "surface_index" => {
            let s = r.surf();
            let acc = r.i();
            let i1 = r.i();
            let i2 = r.i();
            let k = mk_surf(&s)?;
            match acc {
                0 => {
                    let p = k.pole(i1, i2)?;
                    g.f(p.x());
                    g.f(p.y());
                    g.f(p.z());
                }
                1 => g.f(k.weight(i1, i2)?),
                2 => g.f(k.u_knot(i1)?),
                3 => g.f(k.v_knot(i1)?),
                4 => g.i(k.u_multiplicity(i1)?),
                _ => g.i(k.v_multiplicity(i1)?),
            }
        }
        "bezier3d" => {
            let tol = r.f();
            let cs = r.bzc(3);
            let split = r.count();
            run_bezier3(g, tol, &cs, split)?;
        }
        "bezier2d" => {
            let tol = r.f();
            let cs = r.bzc(2);
            let split = r.count();
            run_bezier2(g, tol, &cs, split)?;
        }
        "cpoly_uniform" => {
            let n = r.i();
            let cont = r.i();
            let dim = r.i();
            let maxdeg = r.i();
            let nc = r.iv();
            let coef = r.fv();
            let pint = r.fv();
            let tint = r.fv();
            let p = CompPolynomialToPoles::new(n, cont, dim, maxdeg, &nc, &coef, &pairs(&pint), &tint)?;
            g.cpoly(&p);
        }
        "cpoly_array" => {
            let n = r.i();
            let dim = r.i();
            let maxdeg = r.i();
            let cont = r.iv();
            let nc = r.iv();
            let coef = r.fv();
            let pint = r.fv();
            let tint = r.fv();
            let p = CompPolynomialToPoles::with_continuities(n, dim, maxdeg, &cont, &nc, &coef, &pairs(&pint), &tint)?;
            g.cpoly(&p);
        }
        "cpoly_single" => {
            let dim = r.i();
            let maxdeg = r.i();
            let degree = r.i();
            let coef = r.fv();
            let pint = r.fv();
            let tint = r.fv();
            let p = CompPolynomialToPoles::single(dim, maxdeg, degree, &coef, &pint, &tint)?;
            g.cpoly(&p);
        }
        "gpoly_single" => {
            let mu = r.i();
            let mv = r.i();
            let nc = r.iv();
            let coef = r.fv();
            let pu = r.fv();
            let pv = r.fv();
            let p = GridPolynomialToPoles::single(mu, mv, &nc, &coef, &pu, &pv)?;
            put_grid(g, &p)?;
        }
        "gpoly_multi" => {
            let nu = r.i();
            let nv = r.i();
            let uc = r.i();
            let vc = r.i();
            let mu = r.i();
            let mv = r.i();
            let nc = r.ipairs();
            let coef = r.fv();
            let pu = r.fv();
            let pv = r.fv();
            let tu = r.fv();
            let tv = r.fv();
            let p = GridPolynomialToPoles::multi(nu, nv, uc, vc, mu, mv, &nc, &coef, &pu, &pv, &tu, &tv)?;
            put_grid(g, &p)?;
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

/// Compares the expected words of a line with the replayed values. Floats are compared bit for bit for `EXACT` families and
/// within `MAX_ULPS` of the line's largest magnitude otherwise; bools, ints and texts always exactly.
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
        match t {
            Tok::F(g) => {
                let w = bits(w);
                let d = ulps(w, *g);
                let ok =
                    if exact { w.to_bits() == g.to_bits() } else { (w - g).abs() <= tol || w.to_bits() == g.to_bits() };
                assert!(ok, "value {index}: want {w:e}, got {g:e} ({d} ulps)\n{line}");
                tally.values += 1;
                tally.exact += usize::from(d == 0);
                tally.worst = tally.worst.max(d);
            }
            Tok::B(g) => assert_eq!(*w, if *g { "1" } else { "0" }, "value {index} (bool)\n{line}"),
            Tok::I(g) => assert_eq!(w.parse::<i64>().expect("integer output"), *g, "value {index} (int)\n{line}"),
            Tok::S(g) => assert_eq!(*w, g.as_str(), "value {index} (text)\n{line}"),
        }
    }
}

// ---------------------------------------------------------------------------------

#[test]
fn convert_matches_occt() {
    let text = include_str!("convert.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let (mut n_throws, mut n_unchecked) = (0usize, 0usize);

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in convert.txt:\n{line}");
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
        reader.finish();
        let marker = matches!(want.first(), Some(&"throws") | Some(&"unchecked"));
        match outcome {
            Err(Thrown) => assert!(marker, "Rust API returned an error but OCCT returned values\n{line}"),
            Ok(()) => {
                assert!(!marker, "Rust API returned values but OCCT {}\n{line}", want[0]);
                compare(&mut tally, line, name, &want, &got.0);
            }
        }
        if marker {
            n_throws += usize::from(want[0] == "throws");
            n_unchecked += usize::from(want[0] == "unchecked");
        }
    }

    // Input coverage: no duplicates (checked above), every family with inputs has at least MIN_DISTINCT distinct lines (the
    // families of SMALL_DOMAIN have all the inputs their domain has), and only the listed families are input-less.
    for (name, (count, with_inputs)) in &stats {
        if INPUT_LESS.contains(name) {
            assert_eq!((*count, *with_inputs), (1, 0), "input-less family {name} must be printed exactly once");
        } else if let Some((_, size)) = SMALL_DOMAIN.iter().find(|(n, _)| n == name) {
            assert_eq!(*with_inputs, *count, "family {name} lost its inputs");
            assert_eq!(*count, *size, "family {name} must print its whole input domain ({size} lines)");
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
    println!(
        "{lines} lines ({n_throws} throws, {n_unchecked} unchecked), {} values, {} bit-identical, worst {} ulps",
        tally.values, tally.exact, tally.worst
    );
}
