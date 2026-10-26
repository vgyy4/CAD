//! Numerical reference test for OCCT 8.0.1's 2D geometry: the package `Geom2d` (`cpp/Geom2d/`, toolkit TKG2d) and the parts of
//! `Geom2dEval` it uses (`Geom2dEval_RepCurveDesc`, `Geom2dEval_RepUtils`): `Geom2d_Geometry` and its transformations, points,
//! vectors, directions, axis placements, `Geom2d_Transformation`, the curves (`Geom2d_Curve`, `Geom2d_Line`, the conics, Bezier,
//! B-spline, trimmed and offset curves) and the evaluation representations that Bezier, B-spline and offset curves can carry.
//!
//! `geom2d.txt` is printed by `oracle/geom2d.cpp` (real OCCT). Each line is `<family> <inputs...> | <outputs...>`: f64 values are
//! 16-hex-digit bit patterns, ints are decimal and bools are 0/1. Every input of a call is on the left of ` | `; this test reads
//! them back with the `Rd` reader methods of the same name as the oracle's schema words (`f i tr crv ops`; the grammar of the
//! composite words and the table of the operation codes are in docs/api-geom2d.md) and replays the calls on the Rust API of
//! docs/api-geom2d.md. The right side is the expected values, `throws` (OCCT raised an exception) or `unchecked` (a check of OCCT
//! 8.0.1 as built is compiled out, so the result is unspecified; the Rust API must return an error). A line is a sequence of
//! operations on one object; when one of them throws (or is unchecked) the whole line prints that marker, and the Rust replay
//! must return an error at that operation or before. No family prints a text, so there is no text token to escape.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::geom2d::{
    AxisPlacement, BSplineCurve, BezierCurve, BoundedCurve2d, CartesianPoint, Circle, Conic2d, Curve2d, Direction,
    Ellipse, EvalRepDesc, EvalRepKind, Geom2dError, Geometry2d, Hyperbola, Line, OffsetCurve, Parabola, Point2d,
    Transformation, TrimmedCurve, Vector2d, VectorWithMagnitude,
};
use occt_port::gp::{self, Ax2d, Ax22d, Circ2d, Dir2d, Elips2d, Hypr2d, Lin2d, Parab2d, Pnt2d, Trsf2d, Vec2d};
use occt_port::mathlin::MathError;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for the families that use sin / cos / atan2 / acos ... through the C library, in units of
/// f64::EPSILON times the largest absolute value on the line (at least 1; zero tolerance if that value is infinite). The oracle
/// only prints cases whose outputs stay within 2 ulps when every such result is moved by an ulp.
const MAX_ULPS: f64 = 8.0;

/// Every family with inputs must have at least this many distinct lines.
const MIN_DISTINCT: usize = 16;

// BEGIN GENERATED
/// Families that really take no input: they are printed exactly once.
const INPUT_LESS: &[&str] = &[];

/// Families whose whole input domain is printed (so they are exempt from MIN_DISTINCT): name and number of lines.
const SMALL_DOMAIN: &[(&str, usize)] = &[];

/// Families compared bit for bit: only + - * / sqrt, comparisons and integers (no C library function). The others use MAX_ULPS.
const EXACT: &[&str] = &[
    "bezier_build",
    "bezier_eval",
    "bezier_increase",
    "bezier_insert_pole",
    "bezier_remove_pole",
    "bezier_resolution",
    "bezier_segment",
    "bezier_set_pole",
    "bspline_build",
    "bspline_eval",
    "bspline_increase_degree",
    "bspline_increase_mult",
    "bspline_insert_knot",
    "bspline_insert_knots",
    "bspline_insert_pole",
    "bspline_local",
    "bspline_locate",
    "bspline_move_point",
    "bspline_periodic",
    "bspline_remove_knot",
    "bspline_remove_pole",
    "bspline_resolution",
    "bspline_segment",
    "bspline_set_knot",
    "bspline_set_pole",
    "point_ops",
    "trsf_invert",
    "trsf_multiply",
    "trsf_power",
    "vecmag_arith",
];

/// Every family the oracle must print (a family that disappears from geom2d.txt fails).
const FAMILIES: &[&str] = &[
    "axis_ops",
    "axis_xform",
    "bezier_build",
    "bezier_eval",
    "bezier_increase",
    "bezier_insert_pole",
    "bezier_remove_pole",
    "bezier_resolution",
    "bezier_segment",
    "bezier_set_pole",
    "bspline_build",
    "bspline_eval",
    "bspline_increase_degree",
    "bspline_increase_mult",
    "bspline_insert_knot",
    "bspline_insert_knots",
    "bspline_insert_pole",
    "bspline_is_g1",
    "bspline_local",
    "bspline_locate",
    "bspline_move_point",
    "bspline_move_tangent",
    "bspline_periodic",
    "bspline_remove_knot",
    "bspline_remove_pole",
    "bspline_resolution",
    "bspline_segment",
    "bspline_set_knot",
    "bspline_set_pole",
    "circle_ops",
    "conic_axes",
    "conic_build",
    "curve_copy",
    "curve_d0",
    "curve_d1",
    "curve_d2",
    "curve_d3",
    "curve_dn",
    "curve_geom_ops",
    "curve_params",
    "curve_reverse",
    "curve_xform",
    "direction_ops",
    "direction_xform",
    "ellipse_ops",
    "evalrep_bounded",
    "evalrep_copy",
    "evalrep_full",
    "evalrep_invalid",
    "evalrep_mapped",
    "hyperbola_ops",
    "line_ops",
    "offset_build",
    "offset_eval",
    "offset_set",
    "offset_singular",
    "parabola_ops",
    "point_ops",
    "point_xform",
    "trimmed_build",
    "trimmed_eval",
    "trimmed_periodic",
    "trimmed_set_trim",
    "trsf_invert",
    "trsf_misc",
    "trsf_multiply",
    "trsf_power",
    "trsf_set",
    "vecmag_arith",
    "vecmag_ops",
    "vecmag_xform",
];

/// The number of lines of tests/geom2d.txt.
const GOLDEN_LINES: usize = 1278;
// END GENERATED

/// Families whose lines carry a second curve recipe (`crv crv ops`).
const TWO_CURVES: &[&str] =
    &["evalrep_bounded", "evalrep_copy", "evalrep_full", "evalrep_invalid", "evalrep_mapped", "offset_set"];

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

impl From<Geom2dError> for Thrown {
    fn from(_: Geom2dError) -> Self {
        Thrown
    }
}

type R<T = ()> = Result<T, Thrown>;

#[derive(Clone, Debug)]
enum Tok {
    F(f64),
    B(bool),
    I(i64),
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
    /// `o_p` / `o_v` of the oracle: two floats.
    fn xy(&mut self, x: f64, y: f64) {
        self.f(x);
        self.f(y);
    }
    fn p(&mut self, p: &Pnt2d) {
        self.xy(p.x(), p.y());
    }
    fn v(&mut self, v: &Vec2d) {
        self.xy(v.x(), v.y());
    }
    fn d(&mut self, d: &Dir2d) {
        self.xy(d.x(), d.y());
    }
    /// `o_ax`: location, direction.
    fn ax(&mut self, a: &Ax2d) {
        self.p(a.location());
        self.d(a.direction());
    }
    /// `o_ax22`: location, x direction, y direction.
    fn ax22(&mut self, a: &Ax22d) {
        self.p(a.location());
        self.d(a.x_direction());
        self.d(a.y_direction());
    }
    /// `o_arr_p` / `o_arr_f` / `o_arr_i`: the length, then the values.
    fn arr_p(&mut self, v: &[Pnt2d]) {
        self.l(v.len());
        for x in v {
            self.p(x);
        }
    }
    fn arr_f(&mut self, v: &[f64]) {
        self.l(v.len());
        for x in v {
            self.f(*x);
        }
    }
    fn arr_i(&mut self, v: &[i32]) {
        self.l(v.len());
        for x in v {
            self.i(*x);
        }
    }
}

// ---------------------------------------------------------------------------------
// Recipes read from the inputs of a line

fn bits(w: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(w, 16).unwrap_or_else(|e| panic!("bad hex word {w}: {e}")))
}

/// One operation of an `ops` word: its code and its f64 arguments (integers are rounded, as the oracle's `lround`).
struct Op {
    code: i32,
    a: Vec<f64>,
}

impl Op {
    fn a(&self, i: usize) -> f64 {
        self.a.get(i).copied().unwrap_or(0.0)
    }
    fn ai(&self, i: usize) -> i32 {
        self.a(i).round() as i32
    }
    fn pnt(&self, i: usize) -> Pnt2d {
        Pnt2d::new(self.a(i), self.a(i + 1))
    }
    fn vec(&self, i: usize) -> Vec2d {
        Vec2d::new(self.a(i), self.a(i + 1))
    }
    fn dir(&self, i: usize) -> R<Dir2d> {
        Ok(Dir2d::new(self.a(i), self.a(i + 1))?)
    }
    /// A gp_Ax2d from (x, y, dx, dy); the direction is normalized (an error for a zero direction, as gp_Dir2d).
    fn ax(&self, i: usize) -> R<Ax2d> {
        Ok(Ax2d::new(self.pnt(i), self.dir(i + 2)?))
    }
}

/// `tr`: a transformation recipe (kind and eight parameters).
#[derive(Clone, Copy)]
struct TrSpec {
    kind: i32,
    p: [f64; 8],
}

/// `crv`: a curve recipe.
enum CurveSpec {
    /// Kinds 0..=4: line, circle, ellipse, hyperbola, parabola.
    Basic {
        kind: i32,
        ctor: i32,
        o: [f64; 2],
        d: [f64; 2],
        sense: bool,
        a: f64,
        b: f64,
        focus: [f64; 2],
    },
    Bezier {
        poles: Vec<Pnt2d>,
        weights: Option<Vec<f64>>,
    },
    BSpline {
        degree: i32,
        periodic: bool,
        knots: Vec<f64>,
        mults: Vec<i32>,
        poles: Vec<Pnt2d>,
        weights: Option<Vec<f64>>,
    },
    Trimmed {
        u1: f64,
        u2: f64,
        sense: bool,
        adjust: bool,
        basis: Box<CurveSpec>,
    },
    Offset {
        offset: f64,
        not_check_c0: bool,
        basis: Box<CurveSpec>,
    },
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
    /// A scalar (`f`).
    fn f(&mut self) -> f64 {
        bits(self.word())
    }
    /// An integer (`i`).
    fn i(&mut self) -> i32 {
        self.word().parse().expect("integer input")
    }
    fn count(&mut self) -> usize {
        self.word().parse().expect("count input")
    }
    fn pnts(&mut self) -> Vec<Pnt2d> {
        let n = self.count();
        (0..n)
            .map(|_| {
                let x = self.f();
                let y = self.f();
                Pnt2d::new(x, y)
            })
            .collect()
    }
    fn weights(&mut self) -> Option<Vec<f64>> {
        if self.i() == 0 {
            return None;
        }
        let n = self.count();
        Some((0..n).map(|_| self.f()).collect())
    }
    /// `tr`.
    fn tr(&mut self) -> TrSpec {
        let kind = self.i();
        let mut p = [0.0; 8];
        for v in &mut p {
            *v = self.f();
        }
        TrSpec { kind, p }
    }
    /// `ops`.
    fn ops(&mut self) -> Vec<Op> {
        let n = self.count();
        (0..n)
            .map(|_| {
                let code = self.i();
                let k = self.count();
                Op { code, a: (0..k).map(|_| self.f()).collect() }
            })
            .collect()
    }
    /// `crv`.
    fn crv(&mut self) -> CurveSpec {
        let kind = self.i();
        match kind {
            0..=4 => {
                let ctor = self.i();
                let o = [self.f(), self.f()];
                let d = [self.f(), self.f()];
                let sense = self.i() != 0;
                let (mut a, mut b, mut focus) = (0.0, 0.0, [0.0; 2]);
                match kind {
                    1 | 4 => a = self.f(),
                    2 | 3 => {
                        a = self.f();
                        b = self.f();
                    }
                    _ => {}
                }
                if kind == 4 && ctor == 3 {
                    focus = [self.f(), self.f()];
                }
                CurveSpec::Basic { kind, ctor, o, d, sense, a, b, focus }
            }
            5 => {
                let poles = self.pnts();
                let weights = self.weights();
                CurveSpec::Bezier { poles, weights }
            }
            6 => {
                let degree = self.i();
                let periodic = self.i() != 0;
                let nk = self.count();
                let knots = (0..nk).map(|_| self.f()).collect();
                let nm = self.count();
                let mults = (0..nm).map(|_| self.i()).collect();
                let poles = self.pnts();
                let weights = self.weights();
                CurveSpec::BSpline { degree, periodic, knots, mults, poles, weights }
            }
            7 => {
                let u1 = self.f();
                let u2 = self.f();
                let sense = self.i() != 0;
                let adjust = self.i() != 0;
                CurveSpec::Trimmed { u1, u2, sense, adjust, basis: Box::new(self.crv()) }
            }
            8 => {
                let offset = self.f();
                let not_check_c0 = self.i() != 0;
                CurveSpec::Offset { offset, not_check_c0, basis: Box::new(self.crv()) }
            }
            k => panic!("bad curve kind {k}"),
        }
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread input words");
    }
}

// ---------------------------------------------------------------------------------
// Transformation recipes

/// `tr_gp` of the oracle: the gp_Trsf2d of a recipe (an error where gp raises or the check of SetValues is compiled out).
fn tr_gp(t: &TrSpec) -> R<Trsf2d> {
    let p = &t.p;
    let mut tt = Trsf2d::new();
    let ax = |i: usize| -> R<Ax2d> { Ok(Ax2d::new(Pnt2d::new(p[i], p[i + 1]), Dir2d::new(p[i + 2], p[i + 3])?)) };
    match t.kind {
        1 => tt.set_mirror(&Pnt2d::new(p[0], p[1])),
        2 => tt.set_mirror(&ax(0)?),
        3 => tt.set_rotation(&Pnt2d::new(p[0], p[1]), p[2]),
        4 => tt.set_scale(&Pnt2d::new(p[0], p[1]), p[2]),
        5 => tt.set_transformation(&ax(0)?),
        6 => tt.set_transformation_between(&ax(0)?, &ax(4)?),
        7 => tt.set_translation(&Vec2d::new(p[0], p[1])),
        8 => tt.set_translation_points(&Pnt2d::new(p[0], p[1]), &Pnt2d::new(p[2], p[3])),
        9 => tt.set_values([p[0], p[1], p[2], p[3], p[4], p[5]])?,
        _ => {}
    }
    Ok(tt)
}

/// The recipe in the arguments of an operation, from index `at`.
fn tr_of(op: &Op, at: usize) -> TrSpec {
    let mut p = [0.0; 8];
    for (k, v) in p.iter_mut().enumerate() {
        *v = op.a(at + 1 + k);
    }
    TrSpec { kind: op.ai(at), p }
}

/// `tr_set`: the recipe applied with the `set_*` methods of `Transformation`.
fn tr_set(g: &mut Transformation, t: &TrSpec) -> R {
    let p = &t.p;
    let ax = |i: usize| -> R<Ax2d> { Ok(Ax2d::new(Pnt2d::new(p[i], p[i + 1]), Dir2d::new(p[i + 2], p[i + 3])?)) };
    match t.kind {
        1 => g.set_mirror_point(&Pnt2d::new(p[0], p[1])),
        2 => g.set_mirror_axis(&ax(0)?),
        3 => g.set_rotation(&Pnt2d::new(p[0], p[1]), p[2]),
        4 => g.set_scale(&Pnt2d::new(p[0], p[1]), p[2]),
        5 => g.set_transformation(&ax(0)?),
        6 => g.set_transformation_between(&ax(0)?, &ax(4)?),
        7 => g.set_translation(&Vec2d::new(p[0], p[1])),
        8 => g.set_translation_points(&Pnt2d::new(p[0], p[1]), &Pnt2d::new(p[2], p[3])),
        9 => g.set_trsf2d(&tr_gp(t)?),
        _ => g.set_trsf2d(&Trsf2d::new()),
    }
    Ok(())
}

fn mk_tr_obj(t: &TrSpec) -> R<Transformation> {
    let mut g = Transformation::new();
    tr_set(&mut g, t)?;
    Ok(g)
}

// ---------------------------------------------------------------------------------
// Curve recipes

fn ax_of(o: &[f64; 2], d: &[f64; 2]) -> R<Ax2d> {
    Ok(Ax2d::new(Pnt2d::new(o[0], o[1]), Dir2d::new(d[0], d[1])?))
}

/// `build` of the oracle.
fn build(c: &CurveSpec) -> R<Box<dyn Curve2d>> {
    Ok(match c {
        CurveSpec::Basic { kind, ctor, o, d, sense, a, b, focus } => {
            let ax = ax_of(o, d)?;
            let s = *sense;
            match (*kind, *ctor) {
                (0, 0) => Box::new(Line::from_ax2d(ax)),
                (0, 1) => Box::new(Line::from_lin2d(Lin2d::from_ax2d(ax))),
                (0, _) => Box::new(Line::new(*ax.location(), *ax.direction())),
                (1, 0) => Box::new(Circle::from_circ2d(Circ2d::new(ax, *a, s)?)),
                (1, 1) => Box::new(Circle::new(ax, *a, s)?),
                (1, _) => Box::new(Circle::from_ax22d(Ax22d::from_ax2d(ax, s), *a)?),
                (2, 0) => Box::new(Ellipse::from_elips2d(Elips2d::new(ax, *a, *b, s)?)),
                (2, 1) => Box::new(Ellipse::new(ax, *a, *b, s)?),
                (2, _) => Box::new(Ellipse::from_ax22d(Ax22d::from_ax2d(ax, s), *a, *b)?),
                (3, 0) => Box::new(Hyperbola::from_hypr2d(Hypr2d::new(ax, *a, *b, s)?)),
                (3, 1) => Box::new(Hyperbola::new(ax, *a, *b, s)?),
                (3, _) => Box::new(Hyperbola::from_ax22d(Ax22d::from_ax2d(ax, s), *a, *b)?),
                (_, 0) => Box::new(Parabola::from_parab2d(Parab2d::new(ax, *a, s)?)),
                (_, 1) => Box::new(Parabola::new(ax, *a, s)?),
                (_, 2) => Box::new(Parabola::from_ax22d(Ax22d::from_ax2d(ax, s), *a)?),
                (_, _) => Box::new(Parabola::from_directrix_focus(ax, Pnt2d::new(focus[0], focus[1]))),
            }
        }
        CurveSpec::Bezier { poles, weights } => match weights {
            None => Box::new(BezierCurve::new(poles)?),
            Some(w) => Box::new(BezierCurve::with_weights(poles, w)?),
        },
        CurveSpec::BSpline { degree, periodic, knots, mults, poles, weights } => match weights {
            None => Box::new(BSplineCurve::new(poles, knots, mults, *degree, *periodic)?),
            Some(w) => Box::new(BSplineCurve::with_weights(poles, w, knots, mults, *degree, *periodic)?),
        },
        CurveSpec::Trimmed { u1, u2, sense, adjust, basis } => {
            let b: Arc<dyn Curve2d> = Arc::from(build(basis)?);
            Box::new(TrimmedCurve::new(b, *u1, *u2, *sense, *adjust)?)
        }
        CurveSpec::Offset { offset, not_check_c0, basis } => {
            let b: Arc<dyn Curve2d> = Arc::from(build(basis)?);
            Box::new(OffsetCurve::new(b, *offset, *not_check_c0)?)
        }
    })
}

// ---------------------------------------------------------------------------------
// The state of a geometric object (`o_state` of the oracle)

fn put_bez(g: &mut Got, b: &BezierCurve) -> R {
    g.i(5);
    g.l(b.poles().len());
    for p in b.poles() {
        g.p(p);
    }
    g.b(b.is_rational());
    for i in 1..=b.nb_poles() {
        g.f(b.weight(i)?);
    }
    g.b(b.is_closed());
    g.b(b.has_eval_representation());
    Ok(())
}

fn put_bsp(g: &mut Got, b: &BSplineCurve) -> R {
    g.i(6);
    g.i(b.degree());
    g.b(b.is_periodic());
    g.b(b.is_rational());
    g.i(b.nb_knots());
    g.i(b.nb_poles());
    for k in b.knots() {
        g.f(*k);
    }
    for m in b.multiplicities() {
        g.i(*m);
    }
    for p in b.poles() {
        g.p(p);
    }
    for i in 1..=b.nb_poles() {
        g.f(b.weight(i)?);
    }
    g.b(b.is_closed());
    g.i(b.knot_distribution() as i32);
    g.i(b.continuity() as i32);
    g.b(b.has_eval_representation());
    Ok(())
}

fn put_state(g: &mut Got, obj: &dyn Geometry2d) -> R {
    let a = obj.as_any();
    if let Some(x) = a.downcast_ref::<Line>() {
        g.i(0);
        g.p(x.location());
        g.d(x.direction());
    } else if let Some(x) = a.downcast_ref::<Circle>() {
        g.i(1);
        g.ax22(x.position());
        g.f(x.radius());
    } else if let Some(x) = a.downcast_ref::<Ellipse>() {
        g.i(2);
        g.ax22(x.position());
        g.f(x.major_radius());
        g.f(x.minor_radius());
    } else if let Some(x) = a.downcast_ref::<Hyperbola>() {
        g.i(3);
        g.ax22(x.position());
        g.f(x.major_radius());
        g.f(x.minor_radius());
    } else if let Some(x) = a.downcast_ref::<Parabola>() {
        g.i(4);
        g.ax22(x.position());
        g.f(x.focal());
    } else if let Some(x) = a.downcast_ref::<BezierCurve>() {
        put_bez(g, x)?;
    } else if let Some(x) = a.downcast_ref::<BSplineCurve>() {
        put_bsp(g, x)?;
    } else if let Some(x) = a.downcast_ref::<TrimmedCurve>() {
        g.i(7);
        g.f(x.first_parameter());
        g.f(x.last_parameter());
        g.b(x.is_periodic());
        g.b(x.is_closed());
        let basis = x.basis_curve();
        put_state(g, &*basis)?;
    } else if let Some(x) = a.downcast_ref::<OffsetCurve>() {
        g.i(8);
        g.f(x.offset());
        g.i(x.continuity() as i32);
        g.i(x.get_basis_curve_continuity() as i32);
        g.b(x.has_eval_representation());
        let basis = x.basis_curve();
        put_state(g, &*basis)?;
    } else if let Some(x) = a.downcast_ref::<CartesianPoint>() {
        g.i(10);
        g.f(Point2d::x(x));
        g.f(Point2d::y(x));
    } else if let Some(x) = a.downcast_ref::<Direction>() {
        g.i(11);
        g.f(Vector2d::x(x));
        g.f(Vector2d::y(x));
    } else if let Some(x) = a.downcast_ref::<VectorWithMagnitude>() {
        g.i(12);
        g.f(Vector2d::x(x));
        g.f(Vector2d::y(x));
    } else if let Some(x) = a.downcast_ref::<AxisPlacement>() {
        g.i(13);
        g.p(&x.location());
        g.d(&x.direction());
    } else {
        panic!("put_state: unknown geometry");
    }
    Ok(())
}

fn put_trsf(g: &mut Got, t: &Transformation) -> R {
    g.i(t.form() as i32);
    g.b(t.is_negative());
    g.f(t.scale_factor());
    let tt = t.trsf2d();
    for r in 1..=2 {
        for c in 1..=3 {
            g.f(tt.value(r, c)?);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Operations on geometric objects

/// `geom_common` of the oracle: the operations every geometric object has (codes 1, 4, 21 .. 34). Returns false for another code.
fn geom_common(g: &mut Got, obj: &mut dyn Geometry2d, op: &Op) -> R<bool> {
    match op.code {
        1 => put_state(g, obj)?,
        4 => obj.transform(&tr_gp(&tr_of(op, 0))?),
        21 => {
            let t = tr_gp(&tr_of(op, 0))?;
            let cp = obj.copy_geometry();
            obj.transform(&t);
            put_state(g, &*cp)?;
        }
        22 => obj.mirror_point(&op.pnt(0)),
        23 => obj.mirror_axis(&op.ax(0)?),
        24 => obj.rotate(&op.pnt(0), op.a(2)),
        25 => obj.scale(&op.pnt(0), op.a(2)),
        26 => obj.translate(&op.vec(0)),
        27 => obj.translate_points(&op.pnt(0), &op.pnt(2)),
        28 => put_state(g, &*obj.mirrored_point(&op.pnt(0)))?,
        29 => put_state(g, &*obj.mirrored_axis(&op.ax(0)?))?,
        30 => put_state(g, &*obj.rotated(&op.pnt(0), op.a(2)))?,
        31 => put_state(g, &*obj.scaled(&op.pnt(0), op.a(2)))?,
        32 => put_state(g, &*obj.translated(&op.vec(0)))?,
        33 => put_state(g, &*obj.translated_points(&op.pnt(0), &op.pnt(2)))?,
        34 => put_state(g, &*obj.transformed(&tr_gp(&tr_of(op, 0))?))?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn mk_cpoint(ctor: i32, x: f64, y: f64) -> CartesianPoint {
    if ctor == 0 { CartesianPoint::from_pnt2d(Pnt2d::new(x, y)) } else { CartesianPoint::new(x, y) }
}

fn run_point(g: &mut Got, ctor: i32, x: f64, y: f64, ops: &[Op]) -> R {
    let mut obj: Box<dyn Geometry2d> = Box::new(mk_cpoint(ctor, x, y));
    for op in ops {
        if op.code == 35 {
            obj = obj.copy_geometry();
            continue;
        }
        if geom_common(g, &mut *obj, op)? {
            continue;
        }
        let p = obj.as_any_mut().downcast_mut::<CartesianPoint>().expect("a Cartesian point");
        match op.code {
            40 => p.set_coord(op.a(0), op.a(1)),
            41 => p.set_pnt2d(Pnt2d::new(op.a(0), op.a(1))),
            42 => p.set_x(op.a(0)),
            43 => p.set_y(op.a(0)),
            44 => {
                let (x, y) = p.coord();
                g.xy(x, y);
            }
            45 => g.p(&p.pnt2d()),
            46 => g.f(Point2d::x(p)),
            47 => g.f(Point2d::y(p)),
            48 => g.f(p.distance(&mk_cpoint(op.ai(2), op.a(0), op.a(1)))),
            49 => g.f(p.square_distance(&mk_cpoint(op.ai(2), op.a(0), op.a(1)))),
            c => panic!("point op {c}"),
        }
    }
    Ok(())
}

fn mk_vec(kind: i32, ctor: i32, x: f64, y: f64, x2: f64, y2: f64) -> R<Box<dyn Geometry2d>> {
    Ok(if kind == 0 {
        if ctor == 0 { Box::new(Direction::new(x, y)?) } else { Box::new(Direction::from_dir2d(Dir2d::new(x, y)?)) }
    } else {
        match ctor {
            0 => Box::new(VectorWithMagnitude::from_vec2d(Vec2d::new(x, y))),
            1 => Box::new(VectorWithMagnitude::new(x, y)),
            _ => Box::new(VectorWithMagnitude::from_points(Pnt2d::new(x, y), Pnt2d::new(x2, y2))),
        }
    })
}

/// `mk_other`: the second operand (kind, x, y) of a vector operation.
fn mk_other(op: &Op, i: usize) -> R<Box<dyn Vector2d>> {
    Ok(if op.ai(i) == 0 {
        Box::new(Direction::new(op.a(i + 1), op.a(i + 2))?)
    } else {
        Box::new(VectorWithMagnitude::new(op.a(i + 1), op.a(i + 2)))
    })
}

fn as_vector(obj: &dyn Geometry2d) -> &dyn Vector2d {
    let a = obj.as_any();
    if let Some(d) = a.downcast_ref::<Direction>() {
        d
    } else {
        a.downcast_ref::<VectorWithMagnitude>().expect("a vector")
    }
}

fn run_vec(g: &mut Got, kind: i32, ctor: i32, v: [f64; 4], ops: &[Op]) -> R {
    let mut obj = mk_vec(kind, ctor, v[0], v[1], v[2], v[3])?;
    for op in ops {
        if op.code == 35 {
            obj = obj.copy_geometry();
            continue;
        }
        if geom_common(g, &mut *obj, op)? {
            continue;
        }
        match op.code {
            50..=53 => {
                if let Some(d) = obj.as_any_mut().downcast_mut::<Direction>() {
                    match op.code {
                        50 => d.set_coord(op.a(0), op.a(1))?,
                        51 => d.set_x(op.a(0))?,
                        52 => d.set_y(op.a(0))?,
                        _ => d.set_dir2d(Dir2d::new(op.a(0), op.a(1))?),
                    }
                } else {
                    let m = obj.as_any_mut().downcast_mut::<VectorWithMagnitude>().expect("a vector with magnitude");
                    match op.code {
                        50 => m.set_coord(op.a(0), op.a(1)),
                        51 => m.set_x(op.a(0)),
                        52 => m.set_y(op.a(0)),
                        _ => m.set_vec2d(Vec2d::new(op.a(0), op.a(1))),
                    }
                }
            }
            54 => {
                if let Some(d) = obj.as_any_mut().downcast_mut::<Direction>() {
                    Vector2d::reverse(d);
                } else if let Some(m) = obj.as_any_mut().downcast_mut::<VectorWithMagnitude>() {
                    Vector2d::reverse(m);
                }
            }
            55 => {
                let r = as_vector(&*obj).reversed();
                put_state(g, &*r)?;
            }
            56 => g.f(as_vector(&*obj).angle(&*mk_other(op, 0)?)?),
            57 => {
                let (x, y) = as_vector(&*obj).coord();
                g.xy(x, y);
            }
            58 => g.f(as_vector(&*obj).magnitude()),
            59 => g.f(as_vector(&*obj).square_magnitude()),
            60 => g.f(as_vector(&*obj).x()),
            61 => g.f(as_vector(&*obj).y()),
            62 => g.f(as_vector(&*obj).crossed(&*mk_other(op, 0)?)),
            63 => g.f(as_vector(&*obj).dot(&*mk_other(op, 0)?)),
            64 => g.v(&as_vector(&*obj).vec2d()),
            65 => g.d(&obj.as_any().downcast_ref::<Direction>().expect("a direction").dir2d()),
            70..=79 => {
                let m = obj.as_any_mut().downcast_mut::<VectorWithMagnitude>().expect("a vector with magnitude");
                match op.code {
                    70 => m.add(&*mk_other(op, 0)?),
                    71 => put_state(g, &m.added(&*mk_other(op, 0)?))?,
                    72 => m.subtract(&*mk_other(op, 0)?),
                    73 => put_state(g, &m.subtracted(&*mk_other(op, 0)?))?,
                    74 => m.multiply(op.a(0)),
                    75 => put_state(g, &m.multiplied(op.a(0)))?,
                    76 => m.divide(op.a(0))?,
                    77 => put_state(g, &m.divided(op.a(0))?)?,
                    78 => m.normalize()?,
                    _ => put_state(g, &m.normalized()?)?,
                }
            }
            c => panic!("vector op {c}"),
        }
    }
    Ok(())
}

fn run_axis(g: &mut Got, ctor: i32, o: [f64; 2], d: [f64; 2], ops: &[Op]) -> R {
    let (p, dir) = (Pnt2d::new(o[0], o[1]), Dir2d::new(d[0], d[1])?);
    let mut obj: Box<dyn Geometry2d> = if ctor == 0 {
        Box::new(AxisPlacement::new(Ax2d::new(p, dir)))
    } else {
        Box::new(AxisPlacement::from_point_direction(p, dir))
    };
    for op in ops {
        if op.code == 35 {
            obj = obj.copy_geometry();
            continue;
        }
        if geom_common(g, &mut *obj, op)? {
            continue;
        }
        let x = obj.as_any_mut().downcast_mut::<AxisPlacement>().expect("an axis placement");
        match op.code {
            80 => x.reverse(),
            81 => put_state(g, &x.reversed())?,
            82 => x.set_axis(op.ax(0)?),
            83 => x.set_direction(op.dir(0)?),
            84 => x.set_location(op.pnt(0)),
            85 => g.f(x.angle(&AxisPlacement::new(op.ax(0)?))),
            86 => g.ax(&x.ax2d()),
            87 => g.d(&x.direction()),
            88 => g.p(&x.location()),
            c => panic!("axis op {c}"),
        }
    }
    Ok(())
}

fn run_trsf(g: &mut Got, t0: &TrSpec, ops: &[Op]) -> R {
    let mut t = mk_tr_obj(t0)?;
    for op in ops {
        match op.code {
            1 => put_trsf(g, &t)?,
            2 => tr_set(&mut t, &tr_of(op, 0))?,
            3 => t.invert()?,
            4 => put_trsf(g, &t.inverted()?)?,
            5 => t.multiply(&mk_tr_obj(&tr_of(op, 0))?),
            6 => put_trsf(g, &t.multiplied(&mk_tr_obj(&tr_of(op, 0))?))?,
            7 => t.pre_multiply(&mk_tr_obj(&tr_of(op, 0))?),
            8 => t.power(op.ai(0))?,
            9 => put_trsf(g, &t.powered(op.ai(0))?)?,
            10 => {
                let (x, y) = t.transforms(op.a(0), op.a(1));
                g.xy(x, y);
            }
            11 => g.f(t.value(op.ai(0), op.ai(1))?),
            12 => t = t.copy(),
            13 => t = Transformation::from_trsf2d(&tr_gp(&tr_of(op, 0))?),
            14 => t.set_trsf2d(&tr_gp(&tr_of(op, 0))?),
            15 => {
                let o = mk_tr_obj(&tr_of(op, 0))?;
                put_trsf(g, &(&t * &o))?;
            }
            16 => {
                let o = mk_tr_obj(&tr_of(op, 0))?;
                t *= &o;
            }
            c => panic!("transformation op {c}"),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Operations on curves (the codes of the table in docs/api-geom2d.md)

fn conic_mut(c: &mut dyn Curve2d) -> Option<&mut dyn Conic2d> {
    let a = c.as_any();
    if a.is::<Circle>() {
        return c.as_any_mut().downcast_mut::<Circle>().map(|x| x as &mut dyn Conic2d);
    }
    if a.is::<Ellipse>() {
        return c.as_any_mut().downcast_mut::<Ellipse>().map(|x| x as &mut dyn Conic2d);
    }
    if a.is::<Hyperbola>() {
        return c.as_any_mut().downcast_mut::<Hyperbola>().map(|x| x as &mut dyn Conic2d);
    }
    if a.is::<Parabola>() {
        return c.as_any_mut().downcast_mut::<Parabola>().map(|x| x as &mut dyn Conic2d);
    }
    None
}

fn conic_ref(c: &dyn Curve2d) -> Option<&dyn Conic2d> {
    let a = c.as_any();
    if let Some(x) = a.downcast_ref::<Circle>() {
        return Some(x);
    }
    if let Some(x) = a.downcast_ref::<Ellipse>() {
        return Some(x);
    }
    if let Some(x) = a.downcast_ref::<Hyperbola>() {
        return Some(x);
    }
    a.downcast_ref::<Parabola>().map(|x| x as &dyn Conic2d)
}

fn bounded_ref(c: &dyn Curve2d) -> Option<&dyn BoundedCurve2d> {
    let a = c.as_any();
    if let Some(x) = a.downcast_ref::<BezierCurve>() {
        return Some(x);
    }
    if let Some(x) = a.downcast_ref::<BSplineCurve>() {
        return Some(x);
    }
    a.downcast_ref::<TrimmedCurve>().map(|x| x as &dyn BoundedCurve2d)
}

fn put_hypr(g: &mut Got, h: &Hypr2d) {
    g.f(h.major_radius());
    g.f(h.minor_radius());
    g.ax22(h.axis());
}

/// The evaluation representation of a SET_EREP operation (`a` = kind, max derivative order, has domain, first, last, scale, offset,
/// null representation), over the representation curve `rep`.
fn erep_desc(op: &Op, rep: Option<&CurveSpec>) -> R<EvalRepDesc> {
    let kind = op.ai(0);
    let max_deriv_order = op.ai(1).max(0) as usize;
    let mut d = EvalRepDesc {
        kind: EvalRepKind::Full,
        representation: None,
        max_deriv_order: 3,
        domain: None,
        scale: 1.0,
        offset: 0.0,
    };
    if kind == 1 {
        d.kind = EvalRepKind::DerivBounded;
        d.max_deriv_order = max_deriv_order;
    } else if kind != 0 {
        d.kind = EvalRepKind::Mapped;
        d.max_deriv_order = max_deriv_order;
        if op.ai(2) != 0 {
            d.domain = Some((op.a(3), op.a(4)));
        }
        d.scale = op.a(5);
        d.offset = op.a(6);
    }
    if op.ai(7) == 0 {
        let rep = rep.expect("a representation curve");
        d.representation = Some(Arc::from(build(rep)?));
    }
    Ok(d)
}

fn curve_op(g: &mut Got, c: &mut Box<dyn Curve2d>, op: &Op, cv2: Option<&CurveSpec>) -> R {
    if op.code == 35 {
        *c = c.copy_curve();
        return Ok(());
    }
    if geom_common(g, &mut **c, op)? {
        return Ok(());
    }
    match op.code {
        2 => c.reverse(),
        3 => g.f(c.reversed_parameter(op.a(0))),
        5 => g.f(c.transformed_parameter(op.a(0), &tr_gp(&tr_of(op, 1))?)),
        6 => g.f(c.parametric_transformation(&tr_gp(&tr_of(op, 0))?)),
        7 => g.p(&c.eval_d0(op.a(0))?),
        8 => {
            let r = c.eval_d1(op.a(0))?;
            g.p(&r.point);
            g.v(&r.d1);
        }
        9 => {
            let r = c.eval_d2(op.a(0))?;
            g.p(&r.point);
            g.v(&r.d1);
            g.v(&r.d2);
        }
        10 => {
            let r = c.eval_d3(op.a(0))?;
            g.p(&r.point);
            g.v(&r.d1);
            g.v(&r.d2);
            g.v(&r.d3);
        }
        11 => g.v(&c.eval_dn(op.a(0), op.ai(1))?),
        12 => g.p(&c.value(op.a(0))?),
        13 => g.f(c.first_parameter()),
        14 => g.f(c.last_parameter()),
        15 => g.b(c.is_closed()),
        16 => g.b(c.is_periodic()),
        17 => g.f(c.period()?),
        18 => g.i(c.continuity() as i32),
        19 => g.b(c.is_cn(op.ai(0))?),
        20 => put_state(g, &*c.reversed())?,
        100..=108 => {
            let l = c.as_any_mut().downcast_mut::<Line>().expect("a line");
            match op.code {
                100 => l.set_lin2d(Lin2d::from_ax2d(op.ax(0)?)),
                101 => l.set_direction(op.dir(0)?),
                102 => l.set_location(op.pnt(0)),
                103 => l.set_position(op.ax(0)?),
                104 => {
                    let x = l.lin2d();
                    g.p(x.location());
                    g.d(x.direction());
                }
                105 => g.f(l.distance(&op.pnt(0))),
                106 => g.d(l.direction()),
                107 => g.p(l.location()),
                _ => g.ax(l.position()),
            }
        }
        110..=119 => {
            if (110..=114).contains(&op.code) {
                let k = conic_mut(&mut **c).expect("a conic");
                match op.code {
                    110 => k.set_axis(Ax22d::from_ax2d(op.ax(0)?, op.ai(4) != 0)),
                    111 => k.set_axis(Ax22d::from_dirs(op.pnt(0), op.dir(2)?, op.dir(4)?)),
                    112 => k.set_x_axis(op.ax(0)?),
                    113 => k.set_y_axis(op.ax(0)?),
                    _ => k.set_location(op.pnt(0)),
                }
            } else {
                let k = conic_ref(&**c).expect("a conic");
                match op.code {
                    115 => g.ax(&k.x_axis()),
                    116 => g.ax(&k.y_axis()),
                    117 => g.f(k.eccentricity()?),
                    118 => g.p(&k.location()),
                    _ => g.ax22(k.position()),
                }
            }
        }
        120..=122 => {
            let k = c.as_any_mut().downcast_mut::<Circle>().expect("a circle");
            match op.code {
                120 => k.set_radius(op.a(0))?,
                121 => k.set_circ2d(Circ2d::new(op.ax(0)?, op.a(5), op.ai(4) != 0)?),
                _ => {
                    let x = k.circ2d();
                    g.f(x.radius());
                    g.ax22(x.position());
                }
            }
        }
        123..=126 => {
            if let Some(k) = c.as_any_mut().downcast_mut::<Ellipse>() {
                match op.code {
                    123 => k.set_major_radius(op.a(0))?,
                    124 => k.set_minor_radius(op.a(0))?,
                    125 => k.set_elips2d(Elips2d::new(op.ax(0)?, op.a(5), op.a(6), op.ai(4) != 0)?),
                    _ => {
                        let x = k.elips2d();
                        g.f(x.major_radius());
                        g.f(x.minor_radius());
                        g.ax22(x.axis());
                    }
                }
            } else {
                let k = c.as_any_mut().downcast_mut::<Hyperbola>().expect("a hyperbola");
                match op.code {
                    123 => k.set_major_radius(op.a(0))?,
                    124 => k.set_minor_radius(op.a(0))?,
                    c => panic!("hyperbola op {c}"),
                }
            }
        }
        127..=134 => {
            let a = c.as_any();
            if let Some(k) = a.downcast_ref::<Ellipse>() {
                match op.code {
                    127 => g.ax(&k.directrix1()?),
                    128 => g.ax(&k.directrix2()?),
                    129 => g.f(k.focal()),
                    130 => g.p(&k.focus1()),
                    131 => g.p(&k.focus2()),
                    132 => g.f(k.major_radius()),
                    133 => g.f(k.minor_radius()),
                    _ => g.f(k.parameter()),
                }
            } else if let Some(k) = a.downcast_ref::<Hyperbola>() {
                match op.code {
                    127 => g.ax(&k.directrix1()?),
                    128 => g.ax(&k.directrix2()?),
                    129 => g.f(k.focal()),
                    130 => g.p(&k.focus1()),
                    131 => g.p(&k.focus2()),
                    132 => g.f(k.major_radius()),
                    133 => g.f(k.minor_radius()),
                    _ => g.f(k.parameter()?),
                }
            } else {
                let k = a.downcast_ref::<Parabola>().expect("a parabola");
                match op.code {
                    129 => g.f(k.focal()),
                    134 => g.f(k.parameter()),
                    c => panic!("parabola op {c}"),
                }
            }
        }
        135..=141 => {
            if op.code == 140 {
                let k = c.as_any_mut().downcast_mut::<Hyperbola>().expect("a hyperbola");
                k.set_hypr2d(Hypr2d::new(op.ax(0)?, op.a(5), op.a(6), op.ai(4) != 0)?);
            } else {
                let k = c.as_any().downcast_ref::<Hyperbola>().expect("a hyperbola");
                match op.code {
                    135 => g.ax(&k.asymptote1()?),
                    136 => g.ax(&k.asymptote2()?),
                    137 => put_hypr(g, &k.conjugate_branch1()),
                    138 => put_hypr(g, &k.conjugate_branch2()),
                    139 => put_hypr(g, &k.other_branch()),
                    _ => put_hypr(g, &k.hypr2d()),
                }
            }
        }
        142..=146 => {
            if op.code == 142 || op.code == 143 {
                let k = c.as_any_mut().downcast_mut::<Parabola>().expect("a parabola");
                if op.code == 142 {
                    k.set_focal(op.a(0))?;
                } else {
                    k.set_parab2d(Parab2d::new(op.ax(0)?, op.a(5), op.ai(4) != 0)?);
                }
            } else {
                let k = c.as_any().downcast_ref::<Parabola>().expect("a parabola");
                match op.code {
                    144 => {
                        let x = k.parab2d();
                        g.f(x.focal());
                        g.ax22(x.axis());
                    }
                    145 => g.ax(&k.directrix()),
                    _ => g.p(&k.focus()),
                }
            }
        }
        150 => g.p(&bounded_ref(&**c).expect("a bounded curve").start_point()),
        151 => g.p(&bounded_ref(&**c).expect("a bounded curve").end_point()),
        160..=179 => bb_op(g, c, op)?,
        180..=208 => bs_op(g, c, op)?,
        220 => {
            let t = c.as_any_mut().downcast_mut::<TrimmedCurve>().expect("a trimmed curve");
            t.set_trim(op.a(0), op.a(1), op.ai(2) != 0, op.ai(3) != 0)?;
        }
        221 => {
            let a = c.as_any();
            let basis = if let Some(t) = a.downcast_ref::<TrimmedCurve>() {
                t.basis_curve()
            } else {
                a.downcast_ref::<OffsetCurve>().expect("an offset curve").basis_curve()
            };
            put_state(g, &*basis)?;
        }
        230 => {
            let basis: Arc<dyn Curve2d> = Arc::from(build(cv2.expect("a second curve"))?);
            let o = c.as_any_mut().downcast_mut::<OffsetCurve>().expect("an offset curve");
            o.set_basis_curve(basis, op.ai(0) != 0)?;
        }
        231 => c.as_any_mut().downcast_mut::<OffsetCurve>().expect("an offset curve").set_offset_value(op.a(0)),
        232 => g.f(c.as_any().downcast_ref::<OffsetCurve>().expect("an offset curve").offset()),
        234 => {
            g.i(c.as_any().downcast_ref::<OffsetCurve>().expect("an offset curve").get_basis_curve_continuity() as i32)
        }
        240 => {
            let d = erep_desc(op, cv2)?;
            let a = c.as_any_mut();
            if let Some(b) = a.downcast_mut::<BezierCurve>() {
                b.set_eval_representation(Some(d))?;
            } else if let Some(b) = a.downcast_mut::<BSplineCurve>() {
                b.set_eval_representation(Some(d))?;
            } else {
                a.downcast_mut::<OffsetCurve>()
                    .expect("a curve with an evaluation representation")
                    .set_eval_representation(Some(d))?;
            }
        }
        241 => {
            let a = c.as_any_mut();
            if let Some(b) = a.downcast_mut::<BezierCurve>() {
                b.clear_eval_representation();
            } else if let Some(b) = a.downcast_mut::<BSplineCurve>() {
                b.clear_eval_representation();
            } else {
                a.downcast_mut::<OffsetCurve>()
                    .expect("a curve with an evaluation representation")
                    .clear_eval_representation();
            }
        }
        242 | 243 => {
            let a = c.as_any();
            let d: Option<&EvalRepDesc> = if let Some(b) = a.downcast_ref::<BezierCurve>() {
                b.eval_representation()
            } else if let Some(b) = a.downcast_ref::<BSplineCurve>() {
                b.eval_representation()
            } else {
                a.downcast_ref::<OffsetCurve>()
                    .expect("a curve with an evaluation representation")
                    .eval_representation()
            };
            if op.code == 242 {
                g.b(d.is_some());
            } else {
                g.i(d.map_or(-1, |d| d.kind as i32));
            }
        }
        k => panic!("curve op {k}"),
    }
    Ok(())
}

/// Operations 160 .. 179: Bezier and B-spline curves.
fn bb_op(g: &mut Got, c: &mut Box<dyn Curve2d>, op: &Op) -> R {
    if let Some(b) = c.as_any_mut().downcast_mut::<BezierCurve>() {
        match op.code {
            160 => b.increase(op.ai(0))?,
            161 => b.insert_pole_after(op.ai(0), op.pnt(1), op.a(3))?,
            162 => b.insert_pole_before(op.ai(0), op.pnt(1), op.a(3))?,
            163 => b.remove_pole(op.ai(0))?,
            164 => b.segment(op.a(0), op.a(1)),
            165 => b.set_pole(op.ai(0), op.pnt(1))?,
            166 => b.set_pole_weight(op.ai(0), op.pnt(1), op.a(3))?,
            167 => b.set_weight(op.ai(0), op.a(1))?,
            168 => g.f(b.resolution(op.a(0))),
            169 => g.p(&b.pole(op.ai(0))?),
            170 => g.f(b.weight(op.ai(0))?),
            171 => g.i(b.nb_poles()),
            172 => g.i(b.degree()),
            173 => g.b(b.is_rational()),
            174 => g.arr_p(b.poles()),
            175 => {
                g.b(b.weights().is_some());
                if let Some(w) = b.weights() {
                    g.arr_f(w);
                }
            }
            176 => g.arr_f(&b.knots()),
            177 => g.arr_i(&b.multiplicities()),
            178 => g.arr_f(&b.knot_sequence()),
            _ => g.i(BezierCurve::max_degree()),
        }
        return Ok(());
    }
    let b = c.as_any_mut().downcast_mut::<BSplineCurve>().expect("a Bezier or B-spline curve");
    match op.code {
        160 => b.increase_degree(op.ai(0))?,
        161 => b.insert_pole_after(op.ai(0), op.pnt(1), op.a(3))?,
        162 => b.insert_pole_before(op.ai(0), op.pnt(1), op.a(3))?,
        163 => b.remove_pole(op.ai(0))?,
        164 => b.segment(op.a(0), op.a(1), op.a(2))?,
        165 => b.set_pole(op.ai(0), op.pnt(1))?,
        166 => b.set_pole_weight(op.ai(0), op.pnt(1), op.a(3))?,
        167 => b.set_weight(op.ai(0), op.a(1))?,
        168 => g.f(b.resolution(op.a(0))),
        169 => g.p(&b.pole(op.ai(0))?),
        170 => g.f(b.weight(op.ai(0))?),
        171 => g.i(b.nb_poles()),
        172 => g.i(b.degree()),
        173 => g.b(b.is_rational()),
        174 => g.arr_p(b.poles()),
        175 => {
            g.b(b.weights().is_some());
            if let Some(w) = b.weights() {
                g.arr_f(w);
            }
        }
        176 => g.arr_f(b.knots()),
        177 => g.arr_i(b.multiplicities()),
        178 => g.arr_f(b.knot_sequence()),
        _ => g.i(BSplineCurve::max_degree()),
    }
    Ok(())
}

/// Operations 180 .. 208: B-spline curves.
fn bs_op(g: &mut Got, c: &mut Box<dyn Curve2d>, op: &Op) -> R {
    let b = c.as_any_mut().downcast_mut::<BSplineCurve>().expect("a B-spline curve");
    match op.code {
        180 => b.increase_multiplicity(op.ai(0), op.ai(1))?,
        181 => b.increase_multiplicity_range(op.ai(0), op.ai(1), op.ai(2))?,
        182 => b.increment_multiplicity(op.ai(0), op.ai(1), op.ai(2))?,
        183 => b.insert_knot(op.a(0), op.ai(1), op.a(2))?,
        184 => {
            let n = op.ai(2).max(0) as usize;
            let knots: Vec<f64> = (0..n).map(|i| op.a(3 + i)).collect();
            let mults: Vec<i32> = (0..n).map(|i| op.ai(3 + n + i)).collect();
            b.insert_knots(&knots, &mults, op.a(0), op.ai(1) != 0)?;
        }
        185 => g.b(b.remove_knot(op.ai(0), op.ai(1), op.a(2))?),
        186 => b.set_knot(op.ai(0), op.a(1))?,
        187 => b.set_knot_mult(op.ai(0), op.a(1), op.ai(2))?,
        188 => {
            let n = op.ai(0).max(0) as usize;
            let knots: Vec<f64> = (0..n).map(|i| op.a(1 + i)).collect();
            b.set_knots(&knots)?;
        }
        189 => g.f(b.periodic_normalization(op.a(0))),
        190 => b.set_periodic(),
        191 => b.set_origin(op.ai(0))?,
        192 => b.set_not_periodic(),
        193 => {
            let (f, l) = b.move_point(op.a(0), op.pnt(1), op.ai(3), op.ai(4))?;
            g.i(f);
            g.i(l);
        }
        194 => {
            let e = b.move_point_and_tangent(op.a(0), op.pnt(1), op.vec(3), op.a(5), op.ai(6), op.ai(7))?;
            g.i(e);
        }
        195 => g.b(b.is_g1(op.a(0), op.a(1), op.a(2))?),
        196 => {
            let (i1, i2) = b.locate_u(op.a(0), op.a(1), op.ai(2) != 0);
            g.i(i1);
            g.i(i2);
        }
        197 => g.p(&b.local_value(op.a(0), op.ai(1), op.ai(2))?),
        198 => g.p(&b.local_d0(op.a(0), op.ai(1), op.ai(2))?),
        199 => {
            let (p, v1) = b.local_d1(op.a(0), op.ai(1), op.ai(2))?;
            g.p(&p);
            g.v(&v1);
        }
        200 => {
            let (p, v1, v2) = b.local_d2(op.a(0), op.ai(1), op.ai(2))?;
            g.p(&p);
            g.v(&v1);
            g.v(&v2);
        }
        201 => {
            let (p, v1, v2, v3) = b.local_d3(op.a(0), op.ai(1), op.ai(2))?;
            g.p(&p);
            g.v(&v1);
            g.v(&v2);
            g.v(&v3);
        }
        202 => g.v(&b.local_dn(op.a(0), op.ai(1), op.ai(2), op.ai(3))?),
        203 => g.f(b.knot(op.ai(0))?),
        204 => g.i(b.multiplicity(op.ai(0))?),
        205 => g.i(b.knot_distribution() as i32),
        206 => g.i(b.first_u_knot_index()),
        207 => g.i(b.last_u_knot_index()),
        _ => g.i(b.nb_knots()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Replaying one line

fn run_curve(g: &mut Got, cv: &CurveSpec, cv2: Option<&CurveSpec>, ops: &[Op]) -> R {
    let mut c = build(cv)?;
    for op in ops {
        curve_op(g, &mut c, op, cv2)?;
    }
    Ok(())
}

fn run(name: &str, r: &mut Rd, g: &mut Got) -> R {
    match name {
        "point_ops" | "point_xform" => {
            let ctor = r.i();
            let x = r.f();
            let y = r.f();
            let ops = r.ops();
            run_point(g, ctor, x, y, &ops)
        }
        "direction_ops" | "direction_xform" | "vecmag_ops" | "vecmag_arith" | "vecmag_xform" => {
            let kind = r.i();
            let ctor = r.i();
            let v = [r.f(), r.f(), r.f(), r.f()];
            let ops = r.ops();
            run_vec(g, kind, ctor, v, &ops)
        }
        "axis_ops" | "axis_xform" => {
            let ctor = r.i();
            let o = [r.f(), r.f()];
            let d = [r.f(), r.f()];
            let ops = r.ops();
            run_axis(g, ctor, o, d, &ops)
        }
        n if n.starts_with("trsf_") => {
            let t = r.tr();
            let ops = r.ops();
            run_trsf(g, &t, &ops)
        }
        n => {
            let cv = r.crv();
            let cv2 = if TWO_CURVES.contains(&n) { Some(r.crv()) } else { None };
            let ops = r.ops();
            run_curve(g, &cv, cv2.as_ref(), &ops)
        }
    }
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
/// within `MAX_ULPS` of the line's largest magnitude otherwise; bools and ints always exactly.
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
        }
    }
}

// ---------------------------------------------------------------------------------

#[test]
fn geom2d_matches_occt() {
    let text = include_str!("geom2d.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let (mut n_throws, mut n_unchecked) = (0usize, 0usize);

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in geom2d.txt:\n{line}");
        // a line whose operations print nothing ends with " |"
        let (head, tail) = match line.split_once(" | ") {
            Some(parts) => parts,
            None => (line.strip_suffix(" |").unwrap_or_else(|| panic!("line without ' | ': {line}")), ""),
        };
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

    // Input coverage: no duplicates (checked above), every family with inputs has at least MIN_DISTINCT distinct lines, and only
    // the listed families are input-less.
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
    for name in EXACT {
        assert!(expected.contains(name), "EXACT names an unknown family {name}");
    }

    assert_eq!(lines, text.lines().count());
    assert_eq!(lines, GOLDEN_LINES, "golden line count changed");
    println!(
        "{lines} lines ({n_throws} throws, {n_unchecked} unchecked), {} values, {} bit-identical, worst {} ulps",
        tally.values, tally.exact, tally.worst
    );
}
