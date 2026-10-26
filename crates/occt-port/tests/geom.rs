//! Numerical reference test for OCCT 8.0.1's 3D geometry: the package `Geom` (`cpp/Geom/`, toolkit TKG3d) and the parts of
//! `GeomEval` it uses (`GeomEval_RepCurveDesc`, `GeomEval_RepSurfaceDesc`, `GeomEval_RepUtils`): `Geom_Geometry` and its
//! transformations, points, vectors, directions, axis placements, `Geom_Transformation`, the curves (`Geom_Curve`, `Geom_Line`, the
//! conics, Bezier, B-spline, trimmed and offset curves), the surfaces (`Geom_Surface`, the elementary surfaces, the swept surfaces,
//! Bezier, B-spline, rectangular trimmed and offset surfaces) and the evaluation representations that Bezier, B-spline, offset,
//! extrusion, revolution and offset-surface objects can carry.
//!
//! `geom.txt` is printed by `oracle/geom.cpp` (real OCCT). Each line is `<family> <inputs...> | <outputs...>`: f64 values are
//! 16-hex-digit bit patterns, ints are decimal and bools are 0/1. Every input of a call is on the left of ` | `; this test reads
//! them back with the `Rd` reader methods of the same name as the oracle's schema words (`f i tr crv srf ops`; the grammar of the
//! composite words and the table of the operation codes are in docs/api-geom.md) and replays the calls on the Rust API of
//! docs/api-geom.md. The right side is the expected values, `throws` (OCCT raised an exception) or `unchecked` (a check of OCCT
//! 8.0.1 as built is compiled out, so the result is unspecified; the Rust API must return an error). A line is a sequence of
//! operations on one object; when one of them throws (or is unchecked) the whole line prints that marker, and the Rust replay
//! must return an error at that operation or before. No family prints a text, so there is no text token to escape.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::float_cmp,
    clippy::too_many_lines
)]

use occt_port::geom::{
    Axis1Placement, Axis2Placement, AxisPlacement, BSplineCurve, BSplineSurface, BezierCurve, BezierSurface,
    BoundedCurve, CartesianPoint, Circle, Conic, ConicalSurface, Curve, CylindricalSurface, Direction,
    ElementarySurface, Ellipse, EvalRepDesc, EvalRepKind, GeomError, Geometry, Hyperbola, Line, OffsetCurve,
    OffsetSurface, Parabola, Plane, Point, RectangularTrimmedSurface, SphericalSurface, Surface, SurfaceEvalRepDesc,
    SurfaceOfLinearExtrusion, SurfaceOfRevolution, SweptSurface, ToroidalSurface, Transformation, TrimmedCurve, Vector,
    VectorWithMagnitude,
};
use occt_port::gp::{
    self, Ax1, Ax2, Ax3, Circ, Cone, Cylinder, Dir, Elips, Hypr, Lin, Parab, Pln, Pnt, Sphere, Torus, Trsf, Vec as GVec,
};
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
    "bezier_increase_segment",
    "bezier_poles",
    "bezier_set_pole",
    "bezsurf_build",
    "bezsurf_eval",
    "bezsurf_increase_segment",
    "bezsurf_insert_remove",
    "bezsurf_set_pole",
    "bspline_build",
    "bspline_eval",
    "bspline_increase",
    "bspline_insert_knots",
    "bspline_is_equal",
    "bspline_locate_local",
    "bspline_move_point",
    "bspline_periodic",
    "bspline_remove_knot",
    "bspline_segment",
    "bspline_set_knot",
    "bspline_set_pole",
    "bsplsurf_build",
    "bsplsurf_eval",
    "bsplsurf_increase",
    "bsplsurf_iso_local",
    "bsplsurf_knots",
    "bsplsurf_move_point",
    "bsplsurf_periodic",
    "bsplsurf_segment",
    "bsplsurf_set_knot",
    "bsplsurf_set_pole",
    "cpoint_ops",
    "vecmag_arith",
];

/// Every family the oracle must print (a family that disappears from geom.txt fails).
const FAMILIES: &[&str] = &[
    "axis1_ops",
    "axis2_ops",
    "bezier_build",
    "bezier_eval",
    "bezier_increase_segment",
    "bezier_poles",
    "bezier_set_pole",
    "bezsurf_build",
    "bezsurf_eval",
    "bezsurf_increase_segment",
    "bezsurf_insert_remove",
    "bezsurf_set_pole",
    "bspline_build",
    "bspline_eval",
    "bspline_increase",
    "bspline_insert_knots",
    "bspline_is_equal",
    "bspline_is_g1",
    "bspline_locate_local",
    "bspline_move_point",
    "bspline_move_tangent",
    "bspline_periodic",
    "bspline_remove_knot",
    "bspline_segment",
    "bspline_set_knot",
    "bspline_set_pole",
    "bsplsurf_build",
    "bsplsurf_eval",
    "bsplsurf_increase",
    "bsplsurf_iso_local",
    "bsplsurf_knots",
    "bsplsurf_move_point",
    "bsplsurf_periodic",
    "bsplsurf_segment",
    "bsplsurf_set_knot",
    "bsplsurf_set_pole",
    "circle_ops",
    "cone_ops",
    "conic_build",
    "cpoint_ops",
    "cpoint_xform",
    "curve_d01",
    "curve_d23",
    "curve_dn",
    "curve_geom_ops",
    "curve_params",
    "curve_reverse",
    "curve_xform",
    "cylinder_ops",
    "direction_cross",
    "direction_ops",
    "direction_xform",
    "elementary_build",
    "ellipse_ops",
    "evalrep_curve_basic",
    "evalrep_curve_copy",
    "evalrep_curve_invalid",
    "evalrep_curve_mapped",
    "evalrep_surf_basic",
    "evalrep_surf_copy",
    "evalrep_surf_invalid",
    "evalrep_surf_mapped",
    "extrusion_ops",
    "hyperbola_ops",
    "line_ops",
    "offset_build",
    "offset_eval",
    "offset_set",
    "offset_singular",
    "offsurf_build",
    "offsurf_osculating",
    "offsurf_set",
    "offsurf_singular",
    "parabola_ops",
    "plane_ops",
    "revolution_ops",
    "revolution_singular",
    "rts_build",
    "rts_set_trim",
    "sphere_ops",
    "surface_d01",
    "surface_d2",
    "surface_d3dn",
    "surface_geom_ops",
    "surface_iso",
    "surface_params",
    "surface_reverse",
    "surface_xform",
    "swept_build",
    "torus_ops",
    "trimmed_build",
    "trimmed_periodic",
    "trimmed_set_trim",
    "trsf_invert",
    "trsf_multiply",
    "trsf_power",
    "trsf_set",
    "vecmag_arith",
    "vecmag_cross",
    "vecmag_ops",
    "vecmag_xform",
];

/// The number of lines of tests/geom.txt.
const GOLDEN_LINES: usize = 1615; // 1616 printed, one removed in review (docs/api-geom.md)
// END GENERATED

/// Families whose lines carry a second curve recipe (`crv crv ops`).
const TWO_CURVES: &[&str] = &[
    "bspline_is_equal",
    "evalrep_curve_basic",
    "evalrep_curve_copy",
    "evalrep_curve_invalid",
    "evalrep_curve_mapped",
    "offset_set",
];
/// Families whose lines carry a curve as second recipe after a surface (`srf crv ops`).
const SURF_CURVE: &[&str] = &["extrusion_ops", "revolution_ops"];
/// Families whose lines carry a second surface recipe (`srf srf ops`).
const TWO_SURFACES: &[&str] =
    &["evalrep_surf_basic", "evalrep_surf_copy", "evalrep_surf_invalid", "evalrep_surf_mapped", "offsurf_set"];

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

impl From<GeomError> for Thrown {
    fn from(_: GeomError) -> Self {
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
    /// `o_p` / `o_v` / `o_d` of the oracle: three floats.
    fn xyz(&mut self, x: f64, y: f64, z: f64) {
        self.f(x);
        self.f(y);
        self.f(z);
    }
    fn p(&mut self, p: &Pnt) {
        self.xyz(p.x(), p.y(), p.z());
    }
    fn v(&mut self, v: &GVec) {
        self.xyz(v.x(), v.y(), v.z());
    }
    fn d(&mut self, d: &Dir) {
        self.xyz(d.x(), d.y(), d.z());
    }
    /// `o_ax1`: location, direction.
    fn ax1(&mut self, a: &Ax1) {
        self.p(a.location());
        self.d(a.direction());
    }
    /// `o_ax2`: location, direction, x direction, y direction.
    fn ax2(&mut self, a: &Ax2) {
        self.p(a.location());
        self.d(a.direction());
        self.d(a.x_direction());
        self.d(a.y_direction());
    }
    /// `o_ax3`: as `o_ax2`, then `direct`.
    fn ax3(&mut self, a: &Ax3) {
        self.p(a.location());
        self.d(a.direction());
        self.d(a.x_direction());
        self.d(a.y_direction());
        self.b(a.direct());
    }
    /// `o_arr_p` / `o_arr_f` / `o_arr_i`: the length, then the values.
    fn arr_p(&mut self, v: &[Pnt]) {
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
    /// `o_arr2_p` / `o_arr2_f`: the number of rows, the number of columns, then the values row by row.
    fn arr2_p(&mut self, v: &[Vec<Pnt>]) {
        self.l(v.len());
        self.l(v.first().map_or(0, Vec::len));
        for r in v {
            for x in r {
                self.p(x);
            }
        }
    }
    fn arr2_f(&mut self, v: &[Vec<f64>]) {
        self.l(v.len());
        self.l(v.first().map_or(0, Vec::len));
        for r in v {
            for x in r {
                self.f(*x);
            }
        }
    }
}

// Operation codes (generated from the enums of oracle/geom.cpp; the table is in docs/api-geom.md)
#[allow(dead_code)]
mod codes {
    pub(crate) const P_SET_COORD: i32 = 40;
    pub(crate) const P_SET_PNT: i32 = 41;
    pub(crate) const P_SET_X: i32 = 42;
    pub(crate) const P_SET_Y: i32 = 43;
    pub(crate) const P_SET_Z: i32 = 44;
    pub(crate) const P_COORD: i32 = 45;
    pub(crate) const P_PNT: i32 = 46;
    pub(crate) const P_X: i32 = 47;
    pub(crate) const P_Y: i32 = 48;
    pub(crate) const P_Z: i32 = 49;
    pub(crate) const P_DIST: i32 = 50;
    pub(crate) const P_SQDIST: i32 = 51;
    pub(crate) const V_SET_COORD: i32 = 50;
    pub(crate) const V_SET_X: i32 = 51;
    pub(crate) const V_SET_Y: i32 = 52;
    pub(crate) const V_SET_Z: i32 = 53;
    pub(crate) const V_SET_VEC: i32 = 54;
    pub(crate) const V_REVERSE: i32 = 55;
    pub(crate) const V_REVERSED: i32 = 56;
    pub(crate) const V_ANGLE: i32 = 57;
    pub(crate) const V_ANGLE_REF: i32 = 58;
    pub(crate) const V_COORD: i32 = 59;
    pub(crate) const V_MAG: i32 = 60;
    pub(crate) const V_SQMAG: i32 = 61;
    pub(crate) const V_X: i32 = 62;
    pub(crate) const V_Y: i32 = 63;
    pub(crate) const V_Z: i32 = 64;
    pub(crate) const V_CROSS: i32 = 65;
    pub(crate) const V_CROSSED: i32 = 66;
    pub(crate) const V_CROSSCROSS: i32 = 67;
    pub(crate) const V_CROSSCROSSED: i32 = 68;
    pub(crate) const V_DOT: i32 = 69;
    pub(crate) const V_DOTCROSS: i32 = 70;
    pub(crate) const V_VEC: i32 = 71;
    pub(crate) const V_DIR: i32 = 72;
    pub(crate) const V_ADD: i32 = 80;
    pub(crate) const V_ADDED: i32 = 81;
    pub(crate) const V_SUBTRACT: i32 = 82;
    pub(crate) const V_SUBTRACTED: i32 = 83;
    pub(crate) const V_MULTIPLY: i32 = 84;
    pub(crate) const V_MULTIPLIED: i32 = 85;
    pub(crate) const V_DIVIDE: i32 = 86;
    pub(crate) const V_DIVIDED: i32 = 87;
    pub(crate) const V_NORMALIZE: i32 = 88;
    pub(crate) const V_NORMALIZED: i32 = 89;
    pub(crate) const X_REVERSE: i32 = 110;
    pub(crate) const X_REVERSED: i32 = 111;
    pub(crate) const X_SET_AXIS: i32 = 112;
    pub(crate) const X_SET_DIR: i32 = 113;
    pub(crate) const X_SET_LOC: i32 = 114;
    pub(crate) const X_ANGLE: i32 = 115;
    pub(crate) const X_AXIS: i32 = 116;
    pub(crate) const X_DIRECTION: i32 = 117;
    pub(crate) const X_LOCATION: i32 = 118;
    pub(crate) const X_AX1: i32 = 119;
    pub(crate) const X2_SET_AX2: i32 = 125;
    pub(crate) const X2_SET_XDIR: i32 = 126;
    pub(crate) const X2_SET_YDIR: i32 = 127;
    pub(crate) const X2_AX2: i32 = 128;
    pub(crate) const X2_XDIR: i32 = 129;
    pub(crate) const X2_YDIR: i32 = 130;
    pub(crate) const T_STATE: i32 = 1;
    pub(crate) const T_SET: i32 = 2;
    pub(crate) const T_INVERT: i32 = 3;
    pub(crate) const T_INVERTED: i32 = 4;
    pub(crate) const T_MULTIPLY: i32 = 5;
    pub(crate) const T_MULTIPLIED: i32 = 6;
    pub(crate) const T_PREMULTIPLY: i32 = 7;
    pub(crate) const T_POWER: i32 = 8;
    pub(crate) const T_POWERED: i32 = 9;
    pub(crate) const T_TRANSFORMS: i32 = 10;
    pub(crate) const T_VALUE: i32 = 11;
    pub(crate) const T_COPY_SWAP: i32 = 12;
    pub(crate) const T_FROM_TRSF: i32 = 13;
    pub(crate) const T_SET_TRSF: i32 = 14;
    pub(crate) const C_STATE: i32 = 1;
    pub(crate) const C_REVERSE: i32 = 2;
    pub(crate) const C_REVERSED_PARAM: i32 = 3;
    pub(crate) const C_TRANSFORM_UNUSED: i32 = 4;
    pub(crate) const C_TRANSFORMED_PARAM: i32 = 5;
    pub(crate) const C_PARAM_TRANSFORM: i32 = 6;
    pub(crate) const C_D0: i32 = 7;
    pub(crate) const C_D1: i32 = 8;
    pub(crate) const C_D2: i32 = 9;
    pub(crate) const C_D3: i32 = 10;
    pub(crate) const C_DN: i32 = 11;
    pub(crate) const C_VALUE: i32 = 12;
    pub(crate) const C_FIRST: i32 = 13;
    pub(crate) const C_LAST: i32 = 14;
    pub(crate) const C_IS_CLOSED: i32 = 15;
    pub(crate) const C_IS_PERIODIC: i32 = 16;
    pub(crate) const C_PERIOD: i32 = 17;
    pub(crate) const C_CONTINUITY: i32 = 18;
    pub(crate) const C_IS_CN: i32 = 19;
    pub(crate) const C_REVERSED: i32 = 20;
    pub(crate) const L_SET_LIN: i32 = 100;
    pub(crate) const L_SET_DIR: i32 = 101;
    pub(crate) const L_SET_LOC: i32 = 102;
    pub(crate) const L_SET_POS: i32 = 103;
    pub(crate) const L_LIN: i32 = 104;
    pub(crate) const L_POSITION: i32 = 105;
    pub(crate) const K_SET_AXIS: i32 = 110;
    pub(crate) const K_SET_LOC: i32 = 111;
    pub(crate) const K_SET_POS: i32 = 112;
    pub(crate) const K_AXIS: i32 = 113;
    pub(crate) const K_LOCATION: i32 = 114;
    pub(crate) const K_POSITION: i32 = 115;
    pub(crate) const K_XAXIS: i32 = 116;
    pub(crate) const K_YAXIS: i32 = 117;
    pub(crate) const K_ECC: i32 = 118;
    pub(crate) const CI_SET_RADIUS: i32 = 120;
    pub(crate) const CI_SET_CIRC: i32 = 121;
    pub(crate) const CI_CIRC: i32 = 122;
    pub(crate) const E_SET_MAJOR: i32 = 123;
    pub(crate) const E_SET_MINOR: i32 = 124;
    pub(crate) const E_SET_ELIPS: i32 = 125;
    pub(crate) const E_ELIPS: i32 = 126;
    pub(crate) const EH_DIRECTRIX1: i32 = 127;
    pub(crate) const EH_DIRECTRIX2: i32 = 128;
    pub(crate) const EH_FOCAL: i32 = 129;
    pub(crate) const EH_FOCUS1: i32 = 130;
    pub(crate) const EH_FOCUS2: i32 = 131;
    pub(crate) const EH_MAJOR: i32 = 132;
    pub(crate) const EH_MINOR: i32 = 133;
    pub(crate) const EH_PARAMETER: i32 = 134;
    pub(crate) const H_ASYMPTOTE1: i32 = 135;
    pub(crate) const H_ASYMPTOTE2: i32 = 136;
    pub(crate) const H_CONJ1: i32 = 137;
    pub(crate) const H_CONJ2: i32 = 138;
    pub(crate) const H_OTHER: i32 = 139;
    pub(crate) const H_SET_HYPR: i32 = 140;
    pub(crate) const H_HYPR: i32 = 141;
    pub(crate) const PA_SET_FOCAL: i32 = 142;
    pub(crate) const PA_SET_PARAB: i32 = 143;
    pub(crate) const PA_PARAB: i32 = 144;
    pub(crate) const PA_DIRECTRIX: i32 = 145;
    pub(crate) const PA_FOCUS: i32 = 146;
    pub(crate) const B_START: i32 = 150;
    pub(crate) const B_END: i32 = 151;
    pub(crate) const BB_INCREASE: i32 = 160;
    pub(crate) const BB_INS_AFTER: i32 = 161;
    pub(crate) const BB_INS_BEFORE: i32 = 162;
    pub(crate) const BB_REMOVE_POLE: i32 = 163;
    pub(crate) const BB_SEGMENT: i32 = 164;
    pub(crate) const BB_SET_POLE: i32 = 165;
    pub(crate) const BB_SET_POLE_W: i32 = 166;
    pub(crate) const BB_SET_WEIGHT: i32 = 167;
    pub(crate) const BB_RESOLUTION: i32 = 168;
    pub(crate) const BB_POLE: i32 = 169;
    pub(crate) const BB_WEIGHT: i32 = 170;
    pub(crate) const BB_NBPOLES: i32 = 171;
    pub(crate) const BB_DEGREE: i32 = 172;
    pub(crate) const BB_IS_RATIONAL: i32 = 173;
    pub(crate) const BB_POLES: i32 = 174;
    pub(crate) const BB_WEIGHTS: i32 = 175;
    pub(crate) const BB_KNOTS: i32 = 176;
    pub(crate) const BB_MULTS: i32 = 177;
    pub(crate) const BB_KNOTSEQ: i32 = 178;
    pub(crate) const BB_MAXDEGREE: i32 = 179;
    pub(crate) const S_INC_MULT: i32 = 180;
    pub(crate) const S_INC_MULT_RANGE: i32 = 181;
    pub(crate) const S_INCR_MULT: i32 = 182;
    pub(crate) const S_INS_KNOT: i32 = 183;
    pub(crate) const S_INS_KNOTS: i32 = 184;
    pub(crate) const S_REMOVE_KNOT: i32 = 185;
    pub(crate) const S_SET_KNOT: i32 = 186;
    pub(crate) const S_SET_KNOT_M: i32 = 187;
    pub(crate) const S_SET_KNOTS: i32 = 188;
    pub(crate) const S_PERIODIC_NORM: i32 = 189;
    pub(crate) const S_SET_PERIODIC: i32 = 190;
    pub(crate) const S_SET_ORIGIN: i32 = 191;
    pub(crate) const S_SET_NOT_PERIODIC: i32 = 192;
    pub(crate) const S_MOVE_POINT: i32 = 193;
    pub(crate) const S_MOVE_TANGENT: i32 = 194;
    pub(crate) const S_IS_G1: i32 = 195;
    pub(crate) const S_LOCATE: i32 = 196;
    pub(crate) const S_LOCAL_VALUE: i32 = 197;
    pub(crate) const S_LOCAL_D0: i32 = 198;
    pub(crate) const S_LOCAL_D1: i32 = 199;
    pub(crate) const S_LOCAL_D2: i32 = 200;
    pub(crate) const S_LOCAL_D3: i32 = 201;
    pub(crate) const S_LOCAL_DN: i32 = 202;
    pub(crate) const S_KNOT: i32 = 203;
    pub(crate) const S_MULT: i32 = 204;
    pub(crate) const S_DISTRIBUTION: i32 = 205;
    pub(crate) const S_FIRST_UKNOT: i32 = 206;
    pub(crate) const S_LAST_UKNOT: i32 = 207;
    pub(crate) const S_NBKNOTS: i32 = 208;
    pub(crate) const S_SET_ORIGIN_U: i32 = 209;
    pub(crate) const S_IS_EQUAL: i32 = 210;
    pub(crate) const BB_INS_AFTER_NW: i32 = 211;
    pub(crate) const BB_INS_BEFORE_NW: i32 = 212;
    pub(crate) const TR_SET_TRIM: i32 = 220;
    pub(crate) const TR_BASIS: i32 = 221;
    pub(crate) const O_SET_BASIS: i32 = 230;
    pub(crate) const O_SET_OFFSET: i32 = 231;
    pub(crate) const O_OFFSET: i32 = 232;
    pub(crate) const O_SET_DIR: i32 = 233;
    pub(crate) const O_BASIS_CONT: i32 = 234;
    pub(crate) const O_DIRECTION: i32 = 235;
    pub(crate) const R_SET_EREP: i32 = 240;
    pub(crate) const R_CLEAR_EREP: i32 = 241;
    pub(crate) const R_HAS_EREP: i32 = 242;
    pub(crate) const R_EREP_KIND: i32 = 243;
    pub(crate) const SU_UREVERSE: i32 = 300;
    pub(crate) const SU_UREVERSED: i32 = 301;
    pub(crate) const SU_UREV_PARAM: i32 = 302;
    pub(crate) const SU_VREVERSE: i32 = 303;
    pub(crate) const SU_VREVERSED: i32 = 304;
    pub(crate) const SU_VREV_PARAM: i32 = 305;
    pub(crate) const SU_TRANSFORM_PARAMS: i32 = 306;
    pub(crate) const SU_PARAM_TRANSFORM: i32 = 307;
    pub(crate) const SU_BOUNDS: i32 = 308;
    pub(crate) const SU_IS_UCLOSED: i32 = 309;
    pub(crate) const SU_IS_VCLOSED: i32 = 310;
    pub(crate) const SU_IS_UPER: i32 = 311;
    pub(crate) const SU_IS_VPER: i32 = 312;
    pub(crate) const SU_UPERIOD: i32 = 313;
    pub(crate) const SU_VPERIOD: i32 = 314;
    pub(crate) const SU_UISO: i32 = 315;
    pub(crate) const SU_VISO: i32 = 316;
    pub(crate) const SU_CONT: i32 = 317;
    pub(crate) const SU_ISCNU: i32 = 318;
    pub(crate) const SU_ISCNV: i32 = 319;
    pub(crate) const SU_D0: i32 = 320;
    pub(crate) const SU_D1: i32 = 321;
    pub(crate) const SU_D2: i32 = 322;
    pub(crate) const SU_D3: i32 = 323;
    pub(crate) const SU_DN: i32 = 324;
    pub(crate) const SU_VALUE: i32 = 325;
    pub(crate) const EL_SET_AXIS: i32 = 330;
    pub(crate) const EL_SET_LOC: i32 = 331;
    pub(crate) const EL_SET_POS: i32 = 332;
    pub(crate) const EL_AXIS: i32 = 333;
    pub(crate) const EL_LOCATION: i32 = 334;
    pub(crate) const EL_POSITION: i32 = 335;
    pub(crate) const PL_SET_PLN: i32 = 340;
    pub(crate) const PL_PLN: i32 = 341;
    pub(crate) const PL_COEFFS: i32 = 342;
    pub(crate) const CY_SET_CYL: i32 = 345;
    pub(crate) const CY_SET_RADIUS: i32 = 346;
    pub(crate) const CY_CYL: i32 = 347;
    pub(crate) const CY_COEFFS: i32 = 348;
    pub(crate) const CY_RADIUS: i32 = 349;
    pub(crate) const CO_SET_CONE: i32 = 350;
    pub(crate) const CO_SET_RADIUS: i32 = 351;
    pub(crate) const CO_SET_SEMI: i32 = 352;
    pub(crate) const CO_CONE: i32 = 353;
    pub(crate) const CO_APEX: i32 = 354;
    pub(crate) const CO_COEFFS: i32 = 355;
    pub(crate) const CO_REFRADIUS: i32 = 356;
    pub(crate) const CO_SEMIANGLE: i32 = 357;
    pub(crate) const SP_SET_SPHERE: i32 = 360;
    pub(crate) const SP_SET_RADIUS: i32 = 361;
    pub(crate) const SP_SPHERE: i32 = 362;
    pub(crate) const SP_AREA: i32 = 363;
    pub(crate) const SP_VOLUME: i32 = 364;
    pub(crate) const SP_COEFFS: i32 = 365;
    pub(crate) const SP_RADIUS: i32 = 366;
    pub(crate) const TO_SET_TORUS: i32 = 370;
    pub(crate) const TO_SET_MAJOR: i32 = 371;
    pub(crate) const TO_SET_MINOR: i32 = 372;
    pub(crate) const TO_TORUS: i32 = 373;
    pub(crate) const TO_AREA: i32 = 374;
    pub(crate) const TO_VOLUME: i32 = 375;
    pub(crate) const TO_COEFFS: i32 = 376;
    pub(crate) const TO_MAJOR: i32 = 377;
    pub(crate) const TO_MINOR: i32 = 378;
    pub(crate) const SW_DIRECTION: i32 = 380;
    pub(crate) const SW_BASIS: i32 = 381;
    pub(crate) const SW_SET_DIR: i32 = 382;
    pub(crate) const SW_SET_BASIS: i32 = 383;
    pub(crate) const RV_SET_AXIS: i32 = 384;
    pub(crate) const RV_SET_LOC: i32 = 385;
    pub(crate) const RV_AXIS: i32 = 386;
    pub(crate) const RV_LOCATION: i32 = 387;
    pub(crate) const RV_REFPLANE: i32 = 388;
    pub(crate) const RT_SET_TRIM: i32 = 390;
    pub(crate) const RT_SET_TRIM_P: i32 = 391;
    pub(crate) const RT_BASIS: i32 = 392;
    pub(crate) const OS_SET_BASIS: i32 = 395;
    pub(crate) const OS_SET_OFFSET: i32 = 396;
    pub(crate) const OS_OFFSET: i32 = 397;
    pub(crate) const OS_BASIS: i32 = 398;
    pub(crate) const OS_BASIS_CONT: i32 = 399;
    pub(crate) const OS_SURFACE: i32 = 400;
    pub(crate) const OS_UOSC: i32 = 401;
    pub(crate) const OS_VOSC: i32 = 402;
    pub(crate) const RS_SET_EREP: i32 = 410;
    pub(crate) const RS_CLEAR_EREP: i32 = 411;
    pub(crate) const RS_HAS_EREP: i32 = 412;
    pub(crate) const RS_EREP_KIND: i32 = 413;
    pub(crate) const BZ_EXCHANGE: i32 = 420;
    pub(crate) const BZ_INCREASE: i32 = 421;
    pub(crate) const BZ_INS_COL_AFTER: i32 = 422;
    pub(crate) const BZ_INS_COL_BEFORE: i32 = 423;
    pub(crate) const BZ_INS_ROW_AFTER: i32 = 424;
    pub(crate) const BZ_INS_ROW_BEFORE: i32 = 425;
    pub(crate) const BZ_REMOVE_COL: i32 = 426;
    pub(crate) const BZ_REMOVE_ROW: i32 = 427;
    pub(crate) const BZ_SEGMENT: i32 = 428;
    pub(crate) const BZ_SET_POLE: i32 = 429;
    pub(crate) const BZ_SET_POLE_W: i32 = 430;
    pub(crate) const BZ_SET_POLE_COL: i32 = 431;
    pub(crate) const BZ_SET_POLE_ROW: i32 = 432;
    pub(crate) const BZ_SET_WEIGHT: i32 = 433;
    pub(crate) const BZ_SET_WEIGHT_COL: i32 = 434;
    pub(crate) const BZ_SET_WEIGHT_ROW: i32 = 435;
    pub(crate) const BZ_RESOLUTION: i32 = 436;
    pub(crate) const BZ_MAXDEGREE: i32 = 437;
    pub(crate) const SB_POLE: i32 = 440;
    pub(crate) const SB_WEIGHT: i32 = 441;
    pub(crate) const SB_POLES: i32 = 442;
    pub(crate) const SB_WEIGHTS: i32 = 443;
    pub(crate) const BB_UKNOTS: i32 = 444;
    pub(crate) const BB_VKNOTS: i32 = 445;
    pub(crate) const BB_UMULTS: i32 = 446;
    pub(crate) const BB_VMULTS: i32 = 447;
    pub(crate) const BB_UKNOTSEQ: i32 = 448;
    pub(crate) const BB_VKNOTSEQ: i32 = 449;
    pub(crate) const BB_NBU: i32 = 450;
    pub(crate) const BB_NBV: i32 = 451;
    pub(crate) const BS_EXCHANGE: i32 = 460;
    pub(crate) const BS_SET_UPER: i32 = 461;
    pub(crate) const BS_SET_VPER: i32 = 462;
    pub(crate) const BS_PER_NORM: i32 = 463;
    pub(crate) const BS_SET_UORIGIN: i32 = 464;
    pub(crate) const BS_SET_VORIGIN: i32 = 465;
    pub(crate) const BS_SET_UNOTPER: i32 = 466;
    pub(crate) const BS_SET_VNOTPER: i32 = 467;
    pub(crate) const BS_INCREASE: i32 = 468;
    pub(crate) const BS_INS_UKNOTS: i32 = 469;
    pub(crate) const BS_INS_VKNOTS: i32 = 470;
    pub(crate) const BS_REMOVE_UKNOT: i32 = 471;
    pub(crate) const BS_REMOVE_VKNOT: i32 = 472;
    pub(crate) const BS_INC_UMULT: i32 = 473;
    pub(crate) const BS_INC_UMULT_R: i32 = 474;
    pub(crate) const BS_INCR_UMULT: i32 = 475;
    pub(crate) const BS_INC_VMULT: i32 = 476;
    pub(crate) const BS_INC_VMULT_R: i32 = 477;
    pub(crate) const BS_INCR_VMULT: i32 = 478;
    pub(crate) const BS_INS_UKNOT: i32 = 479;
    pub(crate) const BS_INS_VKNOT: i32 = 480;
    pub(crate) const BS_SEGMENT: i32 = 481;
    pub(crate) const BS_CHECK_SEGMENT: i32 = 482;
    pub(crate) const BS_SET_UKNOT: i32 = 483;
    pub(crate) const BS_SET_UKNOTS: i32 = 484;
    pub(crate) const BS_SET_UKNOT_M: i32 = 485;
    pub(crate) const BS_SET_VKNOT: i32 = 486;
    pub(crate) const BS_SET_VKNOTS: i32 = 487;
    pub(crate) const BS_SET_VKNOT_M: i32 = 488;
    pub(crate) const BS_LOCATE_U: i32 = 489;
    pub(crate) const BS_LOCATE_V: i32 = 490;
    pub(crate) const BS_SET_POLE: i32 = 491;
    pub(crate) const BS_SET_POLE_W: i32 = 492;
    pub(crate) const BS_SET_POLE_COL: i32 = 493;
    pub(crate) const BS_SET_POLE_COL_W: i32 = 494;
    pub(crate) const BS_SET_POLE_ROW: i32 = 495;
    pub(crate) const BS_SET_POLE_ROW_W: i32 = 496;
    pub(crate) const BS_SET_WEIGHT: i32 = 497;
    pub(crate) const BS_SET_WEIGHT_COL: i32 = 498;
    pub(crate) const BS_SET_WEIGHT_ROW: i32 = 499;
    pub(crate) const BS_MOVE_POINT: i32 = 500;
    pub(crate) const BS_FIRST_UKNOT: i32 = 501;
    pub(crate) const BS_FIRST_VKNOT: i32 = 502;
    pub(crate) const BS_LAST_UKNOT: i32 = 503;
    pub(crate) const BS_LAST_VKNOT: i32 = 504;
    pub(crate) const BS_UKNOT: i32 = 505;
    pub(crate) const BS_VKNOT: i32 = 506;
    pub(crate) const BS_UMULT: i32 = 507;
    pub(crate) const BS_VMULT: i32 = 508;
    pub(crate) const BS_LOCAL_VALUE: i32 = 509;
    pub(crate) const BS_LOCAL_D0: i32 = 510;
    pub(crate) const BS_LOCAL_D1: i32 = 511;
    pub(crate) const BS_LOCAL_D2: i32 = 512;
    pub(crate) const BS_LOCAL_D3: i32 = 513;
    pub(crate) const BS_LOCAL_DN: i32 = 514;
    pub(crate) const BS_UISO_C: i32 = 515;
    pub(crate) const BS_VISO_C: i32 = 516;
    pub(crate) const BS_RESOLUTION: i32 = 517;
    pub(crate) const BS_MAXDEGREE: i32 = 518;
}
use codes::*;

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
    fn pnt(&self, i: usize) -> Pnt {
        Pnt::new(self.a(i), self.a(i + 1), self.a(i + 2))
    }
    fn vec(&self, i: usize) -> GVec {
        GVec::new(self.a(i), self.a(i + 1), self.a(i + 2))
    }
    fn dir(&self, i: usize) -> R<Dir> {
        Ok(Dir::new(self.a(i), self.a(i + 1), self.a(i + 2))?)
    }
    /// A gp_Ax1 from (x, y, z, dx, dy, dz).
    fn ax1(&self, i: usize) -> R<Ax1> {
        Ok(Ax1::new(self.pnt(i), self.dir(i + 3)?))
    }
    /// A gp_Ax2 from (x, y, z, nx, ny, nz, xx, xy, xz).
    fn ax2(&self, i: usize) -> R<Ax2> {
        Ok(Ax2::new(self.pnt(i), self.dir(i + 3)?, &self.dir(i + 6)?)?)
    }
    /// A gp_Ax2 from a point and a normal (x, y, z, nx, ny, nz).
    fn ax2_n(&self, i: usize) -> R<Ax2> {
        Ok(Ax2::from_direction(self.pnt(i), self.dir(i + 3)?))
    }
    /// A gp_Ax3 from a frame (x, y, z, nx, ny, nz, xx, xy, xz, flip).
    fn ax3(&self, i: usize) -> R<Ax3> {
        ax3_of(&self.a[i.min(self.a.len())..])
    }
}

fn ax3_of(f: &[f64]) -> R<Ax3> {
    let g = |k: usize| f.get(k).copied().unwrap_or(0.0);
    let mut r = Ax3::new(Pnt::new(g(0), g(1), g(2)), Dir::new(g(3), g(4), g(5))?, Dir::new(g(6), g(7), g(8))?)?;
    if g(9).round() as i32 != 0 {
        r.y_reverse();
    }
    Ok(r)
}

/// Number of parameters of each kind of a `tr` recipe.
const TRN: [usize; 12] = [0, 3, 6, 6, 7, 4, 10, 20, 3, 6, 12, 20];

/// `tr`: a transformation recipe (kind and the parameters of the kind; the unused ones are 0).
#[derive(Clone, Copy)]
struct TrSpec {
    kind: i32,
    p: [f64; 20],
}

/// `crv`: a curve recipe.
enum CurveSpec {
    /// Kinds 0..=4: line, circle, ellipse, hyperbola, parabola.
    Basic {
        kind: i32,
        ctor: i32,
        v: [f64; 9],
        a: f64,
        b: f64,
    },
    Bezier {
        poles: Vec<Pnt>,
        weights: Option<Vec<f64>>,
    },
    BSpline {
        degree: i32,
        periodic: bool,
        knots: Vec<f64>,
        mults: Vec<i32>,
        poles: Vec<Pnt>,
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
        dir: [f64; 3],
        basis: Box<CurveSpec>,
    },
}

/// `srf`: a surface recipe.
enum SurfSpec {
    Plane {
        ctor: i32,
        fr: [f64; 10],
        abcd: [f64; 4],
    },
    Cylinder {
        ctor: i32,
        fr: [f64; 10],
        r: f64,
    },
    Cone {
        ctor: i32,
        fr: [f64; 10],
        ang: f64,
        r: f64,
    },
    Sphere {
        ctor: i32,
        fr: [f64; 10],
        r: f64,
    },
    Torus {
        ctor: i32,
        fr: [f64; 10],
        major: f64,
        minor: f64,
    },
    Bezier {
        nu: usize,
        nv: usize,
        poles: Vec<Pnt>,
        weights: Option<Vec<f64>>,
    },
    BSpline {
        udeg: i32,
        vdeg: i32,
        uper: bool,
        vper: bool,
        uk: Vec<f64>,
        um: Vec<i32>,
        vk: Vec<f64>,
        vm: Vec<i32>,
        nu: usize,
        nv: usize,
        poles: Vec<Pnt>,
        weights: Option<Vec<f64>>,
    },
    Extrusion {
        dir: [f64; 3],
        curve: Box<CurveSpec>,
    },
    Revolution {
        axis: [f64; 6],
        curve: Box<CurveSpec>,
    },
    Trimmed {
        ctor: i32,
        t: [f64; 4],
        s1: bool,
        s2: bool,
        utrim: bool,
        basis: Box<SurfSpec>,
    },
    Offset {
        offset: f64,
        not_check_c0: bool,
        basis: Box<SurfSpec>,
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
    fn fl<const N: usize>(&mut self) -> [f64; N] {
        let mut a = [0.0; N];
        for v in &mut a {
            *v = self.f();
        }
        a
    }
    fn pnts_n(&mut self, n: usize) -> Vec<Pnt> {
        (0..n)
            .map(|_| {
                let x = self.f();
                let y = self.f();
                let z = self.f();
                Pnt::new(x, y, z)
            })
            .collect()
    }
    fn pnts(&mut self) -> Vec<Pnt> {
        let n = self.count();
        self.pnts_n(n)
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
        let mut p = [0.0; 20];
        for v in p.iter_mut().take(TRN[kind as usize]) {
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
                let mut v = [0.0; 9];
                let (mut a, mut b) = (0.0, 0.0);
                if kind == 0 {
                    for x in v.iter_mut().take(6) {
                        *x = self.f();
                    }
                } else {
                    for x in &mut v {
                        *x = self.f();
                    }
                    if kind == 1 || (kind == 4 && ctor != 2) {
                        a = self.f();
                    }
                    if kind == 2 || kind == 3 {
                        a = self.f();
                        b = self.f();
                    }
                }
                CurveSpec::Basic { kind, ctor, v, a, b }
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
                let dir = self.fl::<3>();
                CurveSpec::Offset { offset, not_check_c0, dir, basis: Box::new(self.crv()) }
            }
            k => panic!("bad curve kind {k}"),
        }
    }
    /// `srf`.
    fn srf(&mut self) -> SurfSpec {
        let kind = self.i();
        match kind {
            0 => {
                let ctor = self.i();
                let (mut fr, mut abcd) = ([0.0; 10], [0.0; 4]);
                match ctor {
                    0 | 1 => fr = self.fl::<10>(),
                    2 => {
                        let v = self.fl::<6>();
                        fr[..6].copy_from_slice(&v);
                    }
                    _ => abcd = self.fl::<4>(),
                }
                SurfSpec::Plane { ctor, fr, abcd }
            }
            1 => {
                let ctor = self.i();
                let fr = self.fl::<10>();
                SurfSpec::Cylinder { ctor, fr, r: self.f() }
            }
            2 => {
                let ctor = self.i();
                let fr = self.fl::<10>();
                let ang = self.f();
                SurfSpec::Cone { ctor, fr, ang, r: self.f() }
            }
            3 => {
                let ctor = self.i();
                let fr = self.fl::<10>();
                SurfSpec::Sphere { ctor, fr, r: self.f() }
            }
            4 => {
                let ctor = self.i();
                let fr = self.fl::<10>();
                let major = self.f();
                SurfSpec::Torus { ctor, fr, major, minor: self.f() }
            }
            5 => {
                let nu = self.count();
                let nv = self.count();
                let poles = self.pnts_n(nu * nv);
                let weights = self.weights();
                SurfSpec::Bezier { nu, nv, poles, weights }
            }
            6 => {
                let udeg = self.i();
                let vdeg = self.i();
                let uper = self.i() != 0;
                let vper = self.i() != 0;
                let nku = self.count();
                let uk = (0..nku).map(|_| self.f()).collect();
                let nmu = self.count();
                let um = (0..nmu).map(|_| self.i()).collect();
                let nkv = self.count();
                let vk = (0..nkv).map(|_| self.f()).collect();
                let nmv = self.count();
                let vm = (0..nmv).map(|_| self.i()).collect();
                let nu = self.count();
                let nv = self.count();
                let poles = self.pnts_n(nu * nv);
                let weights = self.weights();
                SurfSpec::BSpline { udeg, vdeg, uper, vper, uk, um, vk, vm, nu, nv, poles, weights }
            }
            7 => {
                let dir = self.fl::<3>();
                SurfSpec::Extrusion { dir, curve: Box::new(self.crv()) }
            }
            8 => {
                let axis = self.fl::<6>();
                SurfSpec::Revolution { axis, curve: Box::new(self.crv()) }
            }
            9 => {
                let ctor = self.i();
                let (t, s1, s2, utrim);
                if ctor == 0 {
                    t = self.fl::<4>();
                    s1 = self.i() != 0;
                    s2 = self.i() != 0;
                    utrim = true;
                } else {
                    let (a, b) = (self.f(), self.f());
                    t = [a, b, 0.0, 0.0];
                    utrim = self.i() != 0;
                    s1 = self.i() != 0;
                    s2 = true;
                }
                SurfSpec::Trimmed { ctor, t, s1, s2, utrim, basis: Box::new(self.srf()) }
            }
            10 => {
                let offset = self.f();
                let not_check_c0 = self.i() != 0;
                SurfSpec::Offset { offset, not_check_c0, basis: Box::new(self.srf()) }
            }
            k => panic!("bad surface kind {k}"),
        }
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread input words");
    }
}

// ---------------------------------------------------------------------------------
// Transformation recipes

/// `tr_gp` of the oracle: the gp_Trsf of a recipe (an error where gp raises or the check of a gp method is compiled out).
fn tr_gp(t: &TrSpec) -> R<Trsf> {
    let p = &t.p;
    let op = Op { code: 0, a: p.to_vec() };
    let mut tt = Trsf::new();
    match t.kind {
        1 => tt.set_mirror(&op.pnt(0)),
        2 => tt.set_mirror(&op.ax1(0)?),
        3 => tt.set_mirror(&op.ax2_n(0)?),
        4 => tt.set_rotation(&op.ax1(0)?, p[6])?,
        5 => tt.set_scale(&op.pnt(0), p[3])?,
        6 => tt.set_transformation(&ax3_of(&p[..10])?),
        7 => tt.set_transformation_between(&ax3_of(&p[..10])?, &ax3_of(&p[10..])?),
        8 => tt.set_translation(&op.vec(0)),
        9 => tt.set_translation_points(&op.pnt(0), &op.pnt(3)),
        10 => tt.set_values([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[8], p[9], p[10], p[11]])?,
        11 => tt.set_displacement(&ax3_of(&p[..10])?, &ax3_of(&p[10..])?),
        _ => {}
    }
    Ok(tt)
}

/// The recipe in the arguments of an operation, from index `at`.
fn tr_of(op: &Op, at: usize) -> TrSpec {
    let kind = if op.a.len() > at { (op.a(at).round() as i32).clamp(0, 11) } else { 0 };
    let mut p = [0.0; 20];
    for (k, v) in p.iter_mut().enumerate().take(TRN[kind as usize]) {
        *v = op.a(at + 1 + k);
    }
    TrSpec { kind, p }
}

/// `tr_set`: the recipe applied with the `set_*` methods of `Transformation`.
fn tr_set(g: &mut Transformation, t: &TrSpec) -> R {
    let p = &t.p;
    let op = Op { code: 0, a: p.to_vec() };
    match t.kind {
        1 => g.set_mirror_point(&op.pnt(0)),
        2 => g.set_mirror_axis1(&op.ax1(0)?),
        3 => g.set_mirror_axis2(&op.ax2_n(0)?),
        4 => g.set_rotation(&op.ax1(0)?, p[6])?,
        5 => g.set_scale(&op.pnt(0), p[3])?,
        6 => g.set_transformation(&ax3_of(&p[..10])?),
        7 => g.set_transformation_between(&ax3_of(&p[..10])?, &ax3_of(&p[10..])?),
        8 => g.set_translation(&op.vec(0)),
        9 => g.set_translation_points(&op.pnt(0), &op.pnt(3)),
        10 | 11 => g.set_trsf(&tr_gp(t)?),
        _ => g.set_trsf(&Trsf::new()),
    }
    Ok(())
}

fn mk_tr_obj(t: &TrSpec) -> R<Transformation> {
    let mut g = Transformation::new();
    tr_set(&mut g, t)?;
    Ok(g)
}

// ---------------------------------------------------------------------------------
// Curve and surface recipes

fn ax2_v(v: &[f64; 9]) -> R<Ax2> {
    Ok(Ax2::new(Pnt::new(v[0], v[1], v[2]), Dir::new(v[3], v[4], v[5])?, &Dir::new(v[6], v[7], v[8])?)?)
}

/// `build` of the oracle.
fn build(c: &CurveSpec) -> R<Box<dyn Curve>> {
    Ok(match c {
        CurveSpec::Basic { kind, ctor, v, a, b } => {
            let o = Pnt::new(v[0], v[1], v[2]);
            match *kind {
                0 => {
                    let d = Dir::new(v[3], v[4], v[5])?;
                    match ctor {
                        0 => Box::new(Line::from_ax1(Ax1::new(o, d))),
                        1 => Box::new(Line::from_lin(Lin::from_axis(Ax1::new(o, d)))),
                        _ => Box::new(Line::new(o, d)),
                    }
                }
                1 => {
                    let ax = ax2_v(v)?;
                    if *ctor == 0 {
                        Box::new(Circle::from_circ(Circ::new(ax, *a)?))
                    } else {
                        Box::new(Circle::new(ax, *a)?)
                    }
                }
                2 => {
                    let ax = ax2_v(v)?;
                    if *ctor == 0 {
                        Box::new(Ellipse::from_elips(Elips::new(ax, *a, *b)?))
                    } else {
                        Box::new(Ellipse::new(ax, *a, *b)?)
                    }
                }
                3 => {
                    let ax = ax2_v(v)?;
                    if *ctor == 0 {
                        Box::new(Hyperbola::from_hypr(Hypr::new(ax, *a, *b)?))
                    } else {
                        Box::new(Hyperbola::new(ax, *a, *b)?)
                    }
                }
                _ => match ctor {
                    0 => Box::new(Parabola::from_parab(Parab::new(ax2_v(v)?, *a)?)),
                    1 => Box::new(Parabola::new(ax2_v(v)?, *a)?),
                    _ => Box::new(Parabola::from_directrix_focus(
                        Ax1::new(o, Dir::new(v[3], v[4], v[5])?),
                        Pnt::new(v[6], v[7], v[8]),
                    )?),
                },
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
            let b: Arc<dyn Curve> = Arc::from(build(basis)?);
            Box::new(TrimmedCurve::new(b, *u1, *u2, *sense, *adjust)?)
        }
        CurveSpec::Offset { offset, not_check_c0, dir, basis } => {
            let b: Arc<dyn Curve> = Arc::from(build(basis)?);
            Box::new(OffsetCurve::new(b, *offset, Dir::new(dir[0], dir[1], dir[2])?, *not_check_c0)?)
        }
    })
}

/// Poles (and weights) of a surface recipe as rows (the oracle fills the weights that are missing with 0).
fn rows_p(nu: usize, nv: usize, poles: &[Pnt]) -> Vec<Vec<Pnt>> {
    (0..nu).map(|i| (0..nv).map(|j| poles[i * nv + j]).collect()).collect()
}

fn rows_w(nu: usize, nv: usize, w: &[f64]) -> Vec<Vec<f64>> {
    (0..nu).map(|i| (0..nv).map(|j| w.get(i * nv + j).copied().unwrap_or(0.0)).collect()).collect()
}

fn build_surface(s: &SurfSpec) -> R<Box<dyn Surface>> {
    Ok(match s {
        SurfSpec::Plane { ctor, fr, abcd } => match ctor {
            0 => Box::new(Plane::from_ax3(ax3_of(fr)?)),
            1 => Box::new(Plane::from_pln(Pln::new(ax3_of(fr)?))),
            2 => Box::new(Plane::from_point_normal(Pnt::new(fr[0], fr[1], fr[2]), Dir::new(fr[3], fr[4], fr[5])?)),
            _ => Box::new(Plane::from_coefficients(abcd[0], abcd[1], abcd[2], abcd[3])?),
        },
        SurfSpec::Cylinder { ctor, fr, r } => {
            if *ctor == 0 {
                Box::new(CylindricalSurface::new(ax3_of(fr)?, *r)?)
            } else {
                Box::new(CylindricalSurface::from_cylinder(Cylinder::new(ax3_of(fr)?, *r)?))
            }
        }
        SurfSpec::Cone { ctor, fr, ang, r } => {
            if *ctor == 0 {
                Box::new(ConicalSurface::new(ax3_of(fr)?, *ang, *r)?)
            } else {
                Box::new(ConicalSurface::from_cone(Cone::new(ax3_of(fr)?, *ang, *r)?))
            }
        }
        SurfSpec::Sphere { ctor, fr, r } => {
            if *ctor == 0 {
                Box::new(SphericalSurface::new(ax3_of(fr)?, *r)?)
            } else {
                Box::new(SphericalSurface::from_sphere(Sphere::new(ax3_of(fr)?, *r)?))
            }
        }
        SurfSpec::Torus { ctor, fr, major, minor } => {
            if *ctor == 0 {
                Box::new(ToroidalSurface::new(ax3_of(fr)?, *major, *minor)?)
            } else {
                Box::new(ToroidalSurface::from_torus(Torus::new(ax3_of(fr)?, *major, *minor)?))
            }
        }
        SurfSpec::Bezier { nu, nv, poles, weights } => {
            let p = rows_p(*nu, *nv, poles);
            match weights {
                None => Box::new(BezierSurface::new(&p)?),
                Some(w) => Box::new(BezierSurface::with_weights(&p, &rows_w(*nu, *nv, w))?),
            }
        }
        SurfSpec::BSpline { udeg, vdeg, uper, vper, uk, um, vk, vm, nu, nv, poles, weights } => {
            let p = rows_p(*nu, *nv, poles);
            match weights {
                None => Box::new(BSplineSurface::new(&p, uk, vk, um, vm, *udeg, *vdeg, *uper, *vper)?),
                Some(w) => Box::new(BSplineSurface::with_weights(
                    &p,
                    &rows_w(*nu, *nv, w),
                    uk,
                    vk,
                    um,
                    vm,
                    *udeg,
                    *vdeg,
                    *uper,
                    *vper,
                )?),
            }
        }
        SurfSpec::Extrusion { dir, curve } => {
            let c: Arc<dyn Curve> = Arc::from(build(curve)?);
            Box::new(SurfaceOfLinearExtrusion::new(c, Dir::new(dir[0], dir[1], dir[2])?)?)
        }
        SurfSpec::Revolution { axis, curve } => {
            let c: Arc<dyn Curve> = Arc::from(build(curve)?);
            let a = Ax1::new(Pnt::new(axis[0], axis[1], axis[2]), Dir::new(axis[3], axis[4], axis[5])?);
            Box::new(SurfaceOfRevolution::new(c, a)?)
        }
        SurfSpec::Trimmed { ctor, t, s1, s2, utrim, basis } => {
            let b: Arc<dyn Surface> = Arc::from(build_surface(basis)?);
            if *ctor == 0 {
                Box::new(RectangularTrimmedSurface::new(b, t[0], t[1], t[2], t[3], *s1, *s2)?)
            } else {
                Box::new(RectangularTrimmedSurface::new_param(b, t[0], t[1], *utrim, *s1)?)
            }
        }
        SurfSpec::Offset { offset, not_check_c0, basis } => {
            let b: Arc<dyn Surface> = Arc::from(build_surface(basis)?);
            Box::new(OffsetSurface::new(b, *offset, *not_check_c0)?)
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
    if b.is_rational() {
        for i in 1..=b.nb_poles() {
            g.f(b.weight(i)?);
        }
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
    if b.is_rational() {
        for i in 1..=b.nb_poles() {
            g.f(b.weight(i)?);
        }
    }
    g.b(b.is_closed());
    g.i(b.knot_distribution() as i32);
    g.i(b.continuity() as i32);
    g.b(b.has_eval_representation());
    Ok(())
}

fn put_bezs(g: &mut Got, b: &BezierSurface) -> R {
    g.i(27);
    g.i(b.nb_u_poles());
    g.i(b.nb_v_poles());
    for i in 1..=b.nb_u_poles() {
        for j in 1..=b.nb_v_poles() {
            g.p(&b.pole(i, j)?);
        }
    }
    g.b(b.is_u_rational());
    g.b(b.is_v_rational());
    if b.is_u_rational() || b.is_v_rational() {
        for i in 1..=b.nb_u_poles() {
            for j in 1..=b.nb_v_poles() {
                g.f(b.weight(i, j)?);
            }
        }
    }
    g.b(b.is_u_closed());
    g.b(b.is_v_closed());
    g.b(b.has_eval_representation());
    Ok(())
}

fn put_bsps(g: &mut Got, b: &BSplineSurface) -> R {
    g.i(28);
    g.i(b.u_degree());
    g.i(b.v_degree());
    g.b(b.is_u_periodic());
    g.b(b.is_v_periodic());
    g.b(b.is_u_rational());
    g.b(b.is_v_rational());
    g.i(b.nb_u_knots());
    g.i(b.nb_v_knots());
    g.i(b.nb_u_poles());
    g.i(b.nb_v_poles());
    for k in b.u_knots() {
        g.f(*k);
    }
    for k in b.v_knots() {
        g.f(*k);
    }
    for m in b.u_multiplicities() {
        g.i(*m);
    }
    for m in b.v_multiplicities() {
        g.i(*m);
    }
    for i in 1..=b.nb_u_poles() {
        for j in 1..=b.nb_v_poles() {
            g.p(&b.pole(i, j)?);
        }
    }
    if b.is_u_rational() || b.is_v_rational() {
        for i in 1..=b.nb_u_poles() {
            for j in 1..=b.nb_v_poles() {
                g.f(b.weight(i, j)?);
            }
        }
    }
    g.b(b.try_is_u_closed()?);
    g.b(b.try_is_v_closed()?);
    g.i(b.u_knot_distribution() as i32);
    g.i(b.v_knot_distribution() as i32);
    g.i(b.continuity() as i32);
    g.b(b.has_eval_representation());
    Ok(())
}

fn put_state(g: &mut Got, obj: &dyn Geometry) -> R {
    let a = obj.as_any();
    if let Some(x) = a.downcast_ref::<Line>() {
        g.i(0);
        g.ax1(x.position());
    } else if let Some(x) = a.downcast_ref::<Circle>() {
        g.i(1);
        g.ax2(Conic::position(x));
        g.f(x.radius());
    } else if let Some(x) = a.downcast_ref::<Ellipse>() {
        g.i(2);
        g.ax2(Conic::position(x));
        g.f(x.major_radius());
        g.f(x.minor_radius());
    } else if let Some(x) = a.downcast_ref::<Hyperbola>() {
        g.i(3);
        g.ax2(Conic::position(x));
        g.f(x.major_radius());
        g.f(x.minor_radius());
    } else if let Some(x) = a.downcast_ref::<Parabola>() {
        g.i(4);
        g.ax2(Conic::position(x));
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
        g.d(&x.direction());
        g.i(Curve::continuity(x) as i32);
        g.i(x.get_basis_curve_continuity() as i32);
        g.b(x.has_eval_representation());
        let basis = x.basis_curve();
        put_state(g, &*basis)?;
    } else if let Some(x) = a.downcast_ref::<CartesianPoint>() {
        g.i(10);
        g.f(Point::x(x));
        g.f(Point::y(x));
        g.f(Point::z(x));
    } else if let Some(x) = a.downcast_ref::<Direction>() {
        g.i(11);
        g.f(Vector::x(x));
        g.f(Vector::y(x));
        g.f(Vector::z(x));
    } else if let Some(x) = a.downcast_ref::<VectorWithMagnitude>() {
        g.i(12);
        g.f(Vector::x(x));
        g.f(Vector::y(x));
        g.f(Vector::z(x));
    } else if let Some(x) = a.downcast_ref::<Axis1Placement>() {
        g.i(13);
        g.p(&AxisPlacement::location(x));
        g.d(&AxisPlacement::direction(x));
    } else if let Some(x) = a.downcast_ref::<Axis2Placement>() {
        g.i(14);
        g.ax2(&x.ax2()?);
    } else if let Some(x) = a.downcast_ref::<Plane>() {
        g.i(20);
        g.ax3(ElementarySurface::position(x));
    } else if let Some(x) = a.downcast_ref::<CylindricalSurface>() {
        g.i(21);
        g.ax3(ElementarySurface::position(x));
        g.f(x.radius());
    } else if let Some(x) = a.downcast_ref::<ConicalSurface>() {
        g.i(22);
        g.ax3(ElementarySurface::position(x));
        g.f(x.semi_angle());
        g.f(x.ref_radius());
    } else if let Some(x) = a.downcast_ref::<SphericalSurface>() {
        g.i(23);
        g.ax3(ElementarySurface::position(x));
        g.f(x.radius());
    } else if let Some(x) = a.downcast_ref::<ToroidalSurface>() {
        g.i(24);
        g.ax3(ElementarySurface::position(x));
        g.f(x.major_radius());
        g.f(x.minor_radius());
    } else if let Some(x) = a.downcast_ref::<SurfaceOfLinearExtrusion>() {
        g.i(25);
        g.d(&SweptSurface::direction(x));
        g.b(x.has_eval_representation());
        let basis = SweptSurface::basis_curve(x);
        put_state(g, &*basis)?;
    } else if let Some(x) = a.downcast_ref::<SurfaceOfRevolution>() {
        g.i(26);
        g.p(&x.location());
        g.d(&SweptSurface::direction(x));
        g.b(x.has_eval_representation());
        let basis = SweptSurface::basis_curve(x);
        put_state(g, &*basis)?;
    } else if let Some(x) = a.downcast_ref::<BezierSurface>() {
        put_bezs(g, x)?;
    } else if let Some(x) = a.downcast_ref::<BSplineSurface>() {
        put_bsps(g, x)?;
    } else if let Some(x) = a.downcast_ref::<RectangularTrimmedSurface>() {
        let (u1, u2, v1, v2) = Surface::bounds(x);
        g.i(29);
        g.f(u1);
        g.f(u2);
        g.f(v1);
        g.f(v2);
        let basis = x.basis_surface();
        put_state(g, &*basis)?;
    } else if let Some(x) = a.downcast_ref::<OffsetSurface>() {
        g.i(30);
        g.f(x.offset());
        g.i(x.get_basis_surf_continuity() as i32);
        g.b(x.has_eval_representation());
        let basis = x.basis_surface();
        put_state(g, &*basis)?;
    } else {
        panic!("put_state: unknown geometry");
    }
    Ok(())
}

fn put_trsf(g: &mut Got, t: &Transformation) -> R {
    g.i(t.form() as i32);
    g.b(t.is_negative());
    g.f(t.scale_factor());
    for r in 1..=3 {
        for c in 1..=4 {
            g.f(t.value(r, c)?);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Operations on geometric objects

/// `geom_common` of the oracle: the operations every geometric object has (codes 1, 4, 21 .. 36). Returns false for another code
/// (code 37, replacing the object by its copy, is done by the callers).
fn geom_common(g: &mut Got, obj: &mut dyn Geometry, op: &Op) -> R<bool> {
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
        23 => obj.mirror_axis1(&op.ax1(0)?),
        24 => obj.mirror_axis2(&op.ax2_n(0)?),
        25 => obj.rotate(&op.ax1(0)?, op.a(6)),
        26 => obj.scale(&op.pnt(0), op.a(3)),
        27 => obj.translate(&op.vec(0)),
        28 => obj.translate_points(&op.pnt(0), &op.pnt(3)),
        29 => put_state(g, &*obj.mirrored_point(&op.pnt(0)))?,
        30 => put_state(g, &*obj.mirrored_axis1(&op.ax1(0)?))?,
        31 => put_state(g, &*obj.mirrored_axis2(&op.ax2_n(0)?))?,
        32 => put_state(g, &*obj.rotated(&op.ax1(0)?, op.a(6)))?,
        33 => put_state(g, &*obj.scaled(&op.pnt(0), op.a(3)))?,
        34 => put_state(g, &*obj.translated(&op.vec(0)))?,
        35 => put_state(g, &*obj.translated_points(&op.pnt(0), &op.pnt(3)))?,
        36 => put_state(g, &*obj.transformed(&tr_gp(&tr_of(op, 0))?))?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn mk_cpoint(ctor: i32, x: f64, y: f64, z: f64) -> CartesianPoint {
    if ctor == 0 { CartesianPoint::from_pnt(Pnt::new(x, y, z)) } else { CartesianPoint::new(x, y, z) }
}

fn run_point(g: &mut Got, ctor: i32, c: [f64; 3], ops: &[Op]) -> R {
    let mut obj: Box<dyn Geometry> = Box::new(mk_cpoint(ctor, c[0], c[1], c[2]));
    for op in ops {
        if op.code == 37 {
            obj = obj.copy_geometry();
            continue;
        }
        if geom_common(g, &mut *obj, op)? {
            continue;
        }
        let p = obj.as_any_mut().downcast_mut::<CartesianPoint>().expect("a Cartesian point");
        match op.code {
            40 => p.set_coord(op.a(0), op.a(1), op.a(2)),
            41 => p.set_pnt(op.pnt(0)),
            42 => p.set_x(op.a(0)),
            43 => p.set_y(op.a(0)),
            44 => p.set_z(op.a(0)),
            45 => {
                let (x, y, z) = Point::coord(p);
                g.xyz(x, y, z);
            }
            46 => g.p(&p.pnt()),
            47 => g.f(Point::x(p)),
            48 => g.f(Point::y(p)),
            49 => g.f(Point::z(p)),
            50 => g.f(p.distance(&mk_cpoint(op.ai(3), op.a(0), op.a(1), op.a(2)))),
            51 => g.f(p.square_distance(&mk_cpoint(op.ai(3), op.a(0), op.a(1), op.a(2)))),
            c => panic!("point op {c}"),
        }
    }
    Ok(())
}

fn mk_vec(kind: i32, ctor: i32, v: &[f64; 6]) -> R<Box<dyn Geometry>> {
    Ok(if kind == 0 {
        if ctor == 0 {
            Box::new(Direction::new(v[0], v[1], v[2])?)
        } else {
            Box::new(Direction::from_dir(Dir::new(v[0], v[1], v[2])?))
        }
    } else {
        match ctor {
            0 => Box::new(VectorWithMagnitude::from_vec(GVec::new(v[0], v[1], v[2]))),
            1 => Box::new(VectorWithMagnitude::new(v[0], v[1], v[2])),
            _ => Box::new(VectorWithMagnitude::from_points(Pnt::new(v[0], v[1], v[2]), Pnt::new(v[3], v[4], v[5]))),
        }
    })
}

/// `mk_other`: the operand (kind, x, y, z) of a vector operation.
fn mk_other(op: &Op, i: usize) -> R<Box<dyn Vector>> {
    Ok(if op.ai(i) == 0 {
        Box::new(Direction::new(op.a(i + 1), op.a(i + 2), op.a(i + 3))?)
    } else {
        Box::new(VectorWithMagnitude::new(op.a(i + 1), op.a(i + 2), op.a(i + 3)))
    })
}

fn as_vector(obj: &dyn Geometry) -> &dyn Vector {
    let a = obj.as_any();
    if let Some(d) = a.downcast_ref::<Direction>() {
        d
    } else {
        a.downcast_ref::<VectorWithMagnitude>().expect("a vector")
    }
}

fn as_vector_mut(obj: &mut dyn Geometry) -> &mut dyn Vector {
    if obj.as_any().is::<Direction>() {
        obj.as_any_mut().downcast_mut::<Direction>().expect("a direction")
    } else {
        obj.as_any_mut().downcast_mut::<VectorWithMagnitude>().expect("a vector with magnitude")
    }
}

fn run_vec(g: &mut Got, kind: i32, ctor: i32, v: [f64; 6], ops: &[Op]) -> R {
    let mut obj = mk_vec(kind, ctor, &v)?;
    for op in ops {
        if op.code == 37 {
            obj = obj.copy_geometry();
            continue;
        }
        if geom_common(g, &mut *obj, op)? {
            continue;
        }
        match op.code {
            50..=54 => {
                if let Some(d) = obj.as_any_mut().downcast_mut::<Direction>() {
                    match op.code {
                        50 => d.set_coord(op.a(0), op.a(1), op.a(2))?,
                        51 => d.set_x(op.a(0))?,
                        52 => d.set_y(op.a(0))?,
                        53 => d.set_z(op.a(0))?,
                        _ => d.set_dir(Dir::new(op.a(0), op.a(1), op.a(2))?),
                    }
                } else {
                    let m = obj.as_any_mut().downcast_mut::<VectorWithMagnitude>().expect("a vector with magnitude");
                    match op.code {
                        50 => m.set_coord(op.a(0), op.a(1), op.a(2)),
                        51 => m.set_x(op.a(0)),
                        52 => m.set_y(op.a(0)),
                        53 => m.set_z(op.a(0)),
                        _ => m.set_vec(GVec::new(op.a(0), op.a(1), op.a(2))),
                    }
                }
            }
            55 => as_vector_mut(&mut *obj).reverse(),
            56 => {
                let r = as_vector(&*obj).reversed();
                put_state(g, &*r)?;
            }
            57 => g.f(as_vector(&*obj).angle(&*mk_other(op, 0)?)?),
            58 => g.f(as_vector(&*obj).angle_with_ref(&*mk_other(op, 0)?, &*mk_other(op, 4)?)?),
            59 => {
                let (x, y, z) = as_vector(&*obj).coord();
                g.xyz(x, y, z);
            }
            60 => g.f(as_vector(&*obj).magnitude()),
            61 => g.f(as_vector(&*obj).square_magnitude()),
            62 => g.f(as_vector(&*obj).x()),
            63 => g.f(as_vector(&*obj).y()),
            64 => g.f(as_vector(&*obj).z()),
            65 => as_vector_mut(&mut *obj).cross(&*mk_other(op, 0)?)?,
            66 => {
                let r = as_vector(&*obj).crossed(&*mk_other(op, 0)?)?;
                put_state(g, &*r)?;
            }
            67 => as_vector_mut(&mut *obj).cross_cross(&*mk_other(op, 0)?, &*mk_other(op, 4)?)?,
            68 => {
                let r = as_vector(&*obj).cross_crossed(&*mk_other(op, 0)?, &*mk_other(op, 4)?)?;
                put_state(g, &*r)?;
            }
            69 => g.f(as_vector(&*obj).dot(&*mk_other(op, 0)?)),
            70 => g.f(as_vector(&*obj).dot_cross(&*mk_other(op, 0)?, &*mk_other(op, 4)?)),
            71 => g.v(&as_vector(&*obj).vec()),
            72 => g.d(&obj.as_any().downcast_ref::<Direction>().expect("a direction").dir()),
            80..=89 => {
                let m = obj.as_any_mut().downcast_mut::<VectorWithMagnitude>().expect("a vector with magnitude");
                match op.code {
                    80 => m.add(&*mk_other(op, 0)?),
                    81 => put_state(g, &m.added(&*mk_other(op, 0)?))?,
                    82 => m.subtract(&*mk_other(op, 0)?),
                    83 => put_state(g, &m.subtracted(&*mk_other(op, 0)?))?,
                    84 => m.multiply(op.a(0)),
                    85 => put_state(g, &m.multiplied(op.a(0)))?,
                    86 => m.divide(op.a(0))?,
                    87 => put_state(g, &m.divided(op.a(0))?)?,
                    88 => m.normalize()?,
                    _ => put_state(g, &m.normalized()?)?,
                }
            }
            c => panic!("vector op {c}"),
        }
    }
    Ok(())
}

fn run_axis(g: &mut Got, two: bool, ctor: i32, v: [f64; 9], ops: &[Op]) -> R {
    let p = Pnt::new(v[0], v[1], v[2]);
    let n = Dir::new(v[3], v[4], v[5])?;
    let mut obj: Box<dyn Geometry> = if !two {
        if ctor == 0 {
            Box::new(Axis1Placement::new(Ax1::new(p, n)))
        } else {
            Box::new(Axis1Placement::from_point_direction(p, n))
        }
    } else {
        let x = Dir::new(v[6], v[7], v[8])?;
        if ctor == 0 {
            Box::new(Axis2Placement::new(Ax2::new(p, n, &x)?))
        } else {
            Box::new(Axis2Placement::from_point_directions(p, n, x)?)
        }
    };
    for op in ops {
        if op.code == 37 {
            obj = obj.copy_geometry();
            continue;
        }
        if geom_common(g, &mut *obj, op)? {
            continue;
        }
        match op.code {
            110 => obj.as_any_mut().downcast_mut::<Axis1Placement>().expect("an axis 1 placement").reverse(),
            111 => {
                let r = obj.as_any().downcast_ref::<Axis1Placement>().expect("an axis 1 placement").reversed();
                put_state(g, &r)?;
            }
            112..=119 => {
                let a = obj.as_any_mut();
                let x: &mut dyn AxisPlacement = if a.is::<Axis1Placement>() {
                    a.downcast_mut::<Axis1Placement>().expect("an axis 1 placement")
                } else {
                    a.downcast_mut::<Axis2Placement>().expect("an axis 2 placement")
                };
                match op.code {
                    112 => x.set_axis(op.ax1(0)?)?,
                    113 => x.set_direction(op.dir(0)?)?,
                    114 => x.set_location(op.pnt(0)),
                    115 => g.f(x.angle(&Axis1Placement::new(op.ax1(0)?))),
                    116 => g.ax1(&x.axis()),
                    117 => g.d(&x.direction()),
                    118 => g.p(&x.location()),
                    _ => {}
                }
                if op.code == 119 {
                    g.ax1(&obj.as_any().downcast_ref::<Axis1Placement>().expect("an axis 1 placement").ax1());
                }
            }
            125..=130 => {
                let x = obj.as_any_mut().downcast_mut::<Axis2Placement>().expect("an axis 2 placement");
                match op.code {
                    125 => x.set_ax2(op.ax2(0)?),
                    126 => x.set_x_direction(op.dir(0)?)?,
                    127 => x.set_y_direction(op.dir(0)?)?,
                    128 => g.ax2(&x.ax2()?),
                    129 => g.d(&x.x_direction()),
                    _ => g.d(&x.y_direction()),
                }
            }
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
                let (x, y, z) = t.transforms(op.a(0), op.a(1), op.a(2));
                g.xyz(x, y, z);
            }
            11 => g.f(t.value(op.ai(0), op.ai(1))?),
            12 => t = t.copy(),
            13 => t = Transformation::from_trsf(&tr_gp(&tr_of(op, 0))?),
            14 => t.set_trsf(&tr_gp(&tr_of(op, 0))?),
            c => panic!("transformation op {c}"),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Operations on curves (the codes of the table in docs/api-geom.md)

fn conic_mut(c: &mut dyn Curve) -> Option<&mut dyn Conic> {
    let a = c.as_any();
    if a.is::<Circle>() {
        return c.as_any_mut().downcast_mut::<Circle>().map(|x| x as &mut dyn Conic);
    }
    if a.is::<Ellipse>() {
        return c.as_any_mut().downcast_mut::<Ellipse>().map(|x| x as &mut dyn Conic);
    }
    if a.is::<Hyperbola>() {
        return c.as_any_mut().downcast_mut::<Hyperbola>().map(|x| x as &mut dyn Conic);
    }
    if a.is::<Parabola>() {
        return c.as_any_mut().downcast_mut::<Parabola>().map(|x| x as &mut dyn Conic);
    }
    None
}

fn conic_ref(c: &dyn Curve) -> Option<&dyn Conic> {
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
    a.downcast_ref::<Parabola>().map(|x| x as &dyn Conic)
}

fn bounded_ref(c: &dyn Curve) -> Option<&dyn BoundedCurve> {
    let a = c.as_any();
    if let Some(x) = a.downcast_ref::<BezierCurve>() {
        return Some(x);
    }
    if let Some(x) = a.downcast_ref::<BSplineCurve>() {
        return Some(x);
    }
    a.downcast_ref::<TrimmedCurve>().map(|x| x as &dyn BoundedCurve)
}

fn put_hypr(g: &mut Got, h: &Hypr) {
    g.f(h.major_radius());
    g.f(h.minor_radius());
    g.ax2(h.position());
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

fn curve_op(g: &mut Got, c: &mut Box<dyn Curve>, op: &Op, cv2: Option<&CurveSpec>) -> R {
    if op.code == 37 {
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
        100..=105 => {
            let l = c.as_any_mut().downcast_mut::<Line>().expect("a line");
            match op.code {
                100 => l.set_lin(Lin::from_axis(op.ax1(0)?)),
                101 => l.set_direction(op.dir(0)?),
                102 => l.set_location(op.pnt(0)),
                103 => l.set_position(op.ax1(0)?),
                104 => g.ax1(l.lin().position()),
                _ => g.ax1(l.position()),
            }
        }
        110..=118 => {
            if op.code <= 112 {
                let k = conic_mut(&mut **c).expect("a conic");
                match op.code {
                    110 => k.set_axis(op.ax1(0)?)?,
                    111 => k.set_location(op.pnt(0)),
                    _ => k.set_position(op.ax2(0)?),
                }
            } else {
                let k = conic_ref(&**c).expect("a conic");
                match op.code {
                    113 => g.ax1(&k.axis()),
                    114 => g.p(&k.location()),
                    115 => g.ax2(k.position()),
                    116 => g.ax1(&k.x_axis()),
                    117 => g.ax1(&k.y_axis()),
                    _ => g.f(k.eccentricity()?),
                }
            }
        }
        120..=122 => {
            let k = c.as_any_mut().downcast_mut::<Circle>().expect("a circle");
            match op.code {
                120 => k.set_radius(op.a(0))?,
                121 => k.set_circ(Circ::new(op.ax2(0)?, op.a(9))?),
                _ => {
                    let x = k.circ();
                    g.f(x.radius());
                    g.ax2(x.position());
                }
            }
        }
        123..=126 => {
            if let Some(k) = c.as_any_mut().downcast_mut::<Ellipse>() {
                match op.code {
                    123 => k.set_major_radius(op.a(0))?,
                    124 => k.set_minor_radius(op.a(0))?,
                    125 => k.set_elips(Elips::new(op.ax2(0)?, op.a(9), op.a(10))?),
                    _ => {
                        let x = k.elips();
                        g.f(x.major_radius());
                        g.f(x.minor_radius());
                        g.ax2(x.position());
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
                    127 => g.ax1(&k.directrix1()?),
                    128 => g.ax1(&k.directrix2()?),
                    129 => g.f(k.focal()),
                    130 => g.p(&k.focus1()),
                    131 => g.p(&k.focus2()),
                    132 => g.f(k.major_radius()),
                    133 => g.f(k.minor_radius()),
                    _ => g.f(k.parameter()),
                }
            } else if let Some(k) = a.downcast_ref::<Hyperbola>() {
                match op.code {
                    127 => g.ax1(&k.directrix1()?),
                    128 => g.ax1(&k.directrix2()?),
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
                k.set_hypr(Hypr::new(op.ax2(0)?, op.a(9), op.a(10))?);
            } else {
                let k = c.as_any().downcast_ref::<Hyperbola>().expect("a hyperbola");
                match op.code {
                    135 => g.ax1(&k.asymptote1()?),
                    136 => g.ax1(&k.asymptote2()?),
                    137 => put_hypr(g, &k.conjugate_branch1()?),
                    138 => put_hypr(g, &k.conjugate_branch2()?),
                    139 => put_hypr(g, &k.other_branch()?),
                    _ => put_hypr(g, &k.hypr()),
                }
            }
        }
        142..=146 => {
            if op.code == 142 || op.code == 143 {
                let k = c.as_any_mut().downcast_mut::<Parabola>().expect("a parabola");
                if op.code == 142 {
                    k.set_focal(op.a(0))?;
                } else {
                    k.set_parab(Parab::new(op.ax2(0)?, op.a(9))?);
                }
            } else {
                let k = c.as_any().downcast_ref::<Parabola>().expect("a parabola");
                match op.code {
                    144 => {
                        let x = k.parab();
                        g.f(x.focal());
                        g.ax2(x.position());
                    }
                    145 => g.ax1(&k.directrix()),
                    _ => g.p(&k.focus()),
                }
            }
        }
        150 => g.p(&bounded_ref(&**c).expect("a bounded curve").start_point()),
        151 => g.p(&bounded_ref(&**c).expect("a bounded curve").end_point()),
        160..=179 | 211 | 212 => bb_op(g, c, op)?,
        180..=210 => bs_op(g, c, op, cv2)?,
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
        230..=235 => {
            if op.code == 230 {
                let basis: Arc<dyn Curve> = Arc::from(build(cv2.expect("a second curve"))?);
                let o = c.as_any_mut().downcast_mut::<OffsetCurve>().expect("an offset curve");
                o.set_basis_curve(basis, op.ai(0) != 0)?;
            } else if op.code == 233 {
                c.as_any_mut().downcast_mut::<OffsetCurve>().expect("an offset curve").set_direction(op.dir(0)?);
            } else {
                let o = c.as_any_mut().downcast_mut::<OffsetCurve>().expect("an offset curve");
                match op.code {
                    231 => o.set_offset_value(op.a(0)),
                    232 => g.f(o.offset()),
                    234 => g.i(o.get_basis_curve_continuity() as i32),
                    _ => g.d(&o.direction()),
                }
            }
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

/// Operations 160 .. 179 (and 211, 212): Bezier and B-spline curves.
fn bb_op(g: &mut Got, c: &mut Box<dyn Curve>, op: &Op) -> R {
    if let Some(b) = c.as_any_mut().downcast_mut::<BezierCurve>() {
        match op.code {
            160 => b.increase(op.ai(0))?,
            161 => b.insert_pole_after_weight(op.ai(0), op.pnt(1), op.a(4))?,
            162 => b.insert_pole_before_weight(op.ai(0), op.pnt(1), op.a(4))?,
            211 => b.insert_pole_after(op.ai(0), op.pnt(1))?,
            212 => b.insert_pole_before(op.ai(0), op.pnt(1))?,
            163 => b.remove_pole(op.ai(0))?,
            164 => b.segment(op.a(0), op.a(1)),
            165 => b.set_pole(op.ai(0), op.pnt(1))?,
            166 => b.set_pole_weight(op.ai(0), op.pnt(1), op.a(4))?,
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
        164 => b.segment(op.a(0), op.a(1), op.a(2))?,
        165 => b.set_pole(op.ai(0), op.pnt(1))?,
        166 => b.set_pole_weight(op.ai(0), op.pnt(1), op.a(4))?,
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
        179 => g.i(BSplineCurve::max_degree()),
        k => panic!("B-spline op {k}"),
    }
    Ok(())
}

/// Operations 180 .. 210: B-spline curves.
fn bs_op(g: &mut Got, c: &mut Box<dyn Curve>, op: &Op, cv2: Option<&CurveSpec>) -> R {
    if op.code == 210 {
        // IsEqual with the second curve
        let other = build(cv2.expect("a second curve"))?;
        let o = other.as_any().downcast_ref::<BSplineCurve>().expect("a B-spline curve");
        let b = c.as_any().downcast_ref::<BSplineCurve>().expect("a B-spline curve");
        g.b(b.is_equal(o, op.a(0)));
        return Ok(());
    }
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
            let (f, l) = b.move_point(op.a(0), op.pnt(1), op.ai(4), op.ai(5))?;
            g.i(f);
            g.i(l);
        }
        194 => {
            let e = b.move_point_and_tangent(op.a(0), op.pnt(1), op.vec(4), op.a(7), op.ai(8), op.ai(9))?;
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
        208 => g.i(b.nb_knots()),
        209 => b.set_origin_param(op.a(0), op.a(1))?,
        k => panic!("B-spline op {k}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Replaying one curve line

fn run_curve(g: &mut Got, cv: &CurveSpec, cv2: Option<&CurveSpec>, ops: &[Op]) -> R {
    let mut c = build(cv)?;
    for op in ops {
        curve_op(g, &mut c, op, cv2)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Operations on surfaces (the codes of the table in docs/api-geom.md; the names are those of the oracle's enum)

fn put_gtrsf2d(g: &mut Got, t: &gp::GTrsf2d) -> R {
    g.i(t.form() as i32);
    g.b(t.is_negative());
    for r in 1..=2 {
        for c in 1..=3 {
            g.f(t.value(r, c)?);
        }
    }
    Ok(())
}

/// The arrays inside the arguments: a count, then that many points (x y z) or values; `i` advances.
fn a_pts(op: &Op, i: &mut usize) -> Vec<Pnt> {
    let n = op.ai(*i).max(0) as usize;
    *i += 1;
    let v = (0..n).map(|k| op.pnt(*i + 3 * k)).collect();
    *i += 3 * n;
    v
}

fn a_vals(op: &Op, i: &mut usize) -> Vec<f64> {
    let n = op.ai(*i).max(0) as usize;
    *i += 1;
    let v = (0..n).map(|k| op.a(*i + k)).collect();
    *i += n;
    v
}

fn a_mults(op: &Op, i: &mut usize) -> Vec<i32> {
    a_vals(op, i).iter().map(|x| x.round() as i32).collect()
}

fn put_eval(g: &mut Got, s: &dyn Surface, code: i32, u: f64, v: f64, nu: i32, nv: i32) -> R {
    match code {
        SU_D0 => g.p(&s.eval_d0(u, v)?),
        SU_D1 => {
            let r = s.eval_d1(u, v)?;
            g.p(&r.point);
            g.v(&r.d1u);
            g.v(&r.d1v);
        }
        SU_D2 => {
            let r = s.eval_d2(u, v)?;
            g.p(&r.point);
            for x in [&r.d1u, &r.d1v, &r.d2u, &r.d2v, &r.d2uv] {
                g.v(x);
            }
        }
        SU_D3 => {
            let r = s.eval_d3(u, v)?;
            g.p(&r.point);
            for x in [&r.d1u, &r.d1v, &r.d2u, &r.d2v, &r.d2uv, &r.d3u, &r.d3v, &r.d3uuv, &r.d3uvv] {
                g.v(x);
            }
        }
        SU_DN => g.v(&s.eval_dn(u, v, nu, nv)?),
        _ => g.p(&s.value(u, v)?),
    }
    Ok(())
}

fn elementary_mut(s: &mut dyn Surface) -> &mut dyn ElementarySurface {
    let a = s.as_any();
    if a.is::<Plane>() {
        return s.as_any_mut().downcast_mut::<Plane>().expect("a plane");
    }
    if a.is::<CylindricalSurface>() {
        return s.as_any_mut().downcast_mut::<CylindricalSurface>().expect("a cylinder");
    }
    if a.is::<ConicalSurface>() {
        return s.as_any_mut().downcast_mut::<ConicalSurface>().expect("a cone");
    }
    if a.is::<SphericalSurface>() {
        return s.as_any_mut().downcast_mut::<SphericalSurface>().expect("a sphere");
    }
    s.as_any_mut().downcast_mut::<ToroidalSurface>().expect("a torus")
}

fn elementary_ref(s: &dyn Surface) -> &dyn ElementarySurface {
    let a = s.as_any();
    if let Some(x) = a.downcast_ref::<Plane>() {
        return x;
    }
    if let Some(x) = a.downcast_ref::<CylindricalSurface>() {
        return x;
    }
    if let Some(x) = a.downcast_ref::<ConicalSurface>() {
        return x;
    }
    if let Some(x) = a.downcast_ref::<SphericalSurface>() {
        return x;
    }
    a.downcast_ref::<ToroidalSurface>().expect("a torus")
}

fn swept_ref(s: &dyn Surface) -> &dyn SweptSurface {
    let a = s.as_any();
    if let Some(x) = a.downcast_ref::<SurfaceOfLinearExtrusion>() {
        return x;
    }
    a.downcast_ref::<SurfaceOfRevolution>().expect("a swept surface")
}

/// The evaluation representation of a RS_SET_EREP operation (`a` = kind, max derivative order, has domain, u1, u2, v1, v2, scale u,
/// offset u, scale v, offset v, swap, null representation), over the representation surface `rep`.
fn serep_desc(op: &Op, rep: Option<&SurfSpec>) -> R<SurfaceEvalRepDesc> {
    let kind = op.ai(0);
    let mut d = SurfaceEvalRepDesc {
        kind: EvalRepKind::Full,
        representation: None,
        max_deriv_order: 3,
        domain: None,
        scale_u: 1.0,
        offset_u: 0.0,
        scale_v: 1.0,
        offset_v: 0.0,
        swap_uv: false,
    };
    if kind == 1 {
        d.kind = EvalRepKind::DerivBounded;
        d.max_deriv_order = op.ai(1).max(0) as usize;
    } else if kind != 0 {
        d.kind = EvalRepKind::Mapped;
        d.max_deriv_order = op.ai(1).max(0) as usize;
        if op.ai(2) != 0 {
            d.domain = Some((op.a(3), op.a(4), op.a(5), op.a(6)));
        }
        d.scale_u = op.a(7);
        d.offset_u = op.a(8);
        d.scale_v = op.a(9);
        d.offset_v = op.a(10);
        d.swap_uv = op.ai(11) != 0;
    }
    if op.ai(12) == 0 {
        d.representation = Some(Arc::from(build_surface(rep.expect("a representation surface"))?));
    }
    Ok(d)
}

fn surf_op(g: &mut Got, s: &mut Box<dyn Surface>, op: &Op, cv2: Option<&CurveSpec>, sv2: Option<&SurfSpec>) -> R {
    if op.code == 37 {
        *s = s.copy_surface();
        return Ok(());
    }
    if geom_common(g, &mut **s, op)? {
        return Ok(());
    }
    match op.code {
        SU_UREVERSE => s.u_reverse(),
        SU_UREVERSED => put_state(g, &*s.u_reversed())?,
        SU_UREV_PARAM => g.f(s.u_reversed_parameter(op.a(0))),
        SU_VREVERSE => s.v_reverse(),
        SU_VREVERSED => put_state(g, &*s.v_reversed())?,
        SU_VREV_PARAM => g.f(s.v_reversed_parameter(op.a(0))),
        SU_TRANSFORM_PARAMS => {
            let (u, v) = s.transform_parameters(op.a(0), op.a(1), &tr_gp(&tr_of(op, 2))?);
            g.f(u);
            g.f(v);
        }
        SU_PARAM_TRANSFORM => put_gtrsf2d(g, &s.parametric_transformation(&tr_gp(&tr_of(op, 0))?))?,
        SU_BOUNDS => {
            let (u1, u2, v1, v2) = s.bounds();
            for x in [u1, u2, v1, v2] {
                g.f(x);
            }
        }
        SU_IS_UCLOSED => g.b(s.is_u_closed()),
        SU_IS_VCLOSED => g.b(s.is_v_closed()),
        SU_IS_UPER => g.b(s.is_u_periodic()),
        SU_IS_VPER => g.b(s.is_v_periodic()),
        SU_UPERIOD => g.f(s.u_period()?),
        SU_VPERIOD => g.f(s.v_period()?),
        SU_UISO => put_state(g, &*s.u_iso(op.a(0))?)?,
        SU_VISO => put_state(g, &*s.v_iso(op.a(0))?)?,
        SU_CONT => g.i(s.continuity() as i32),
        SU_ISCNU => g.b(s.is_cn_u(op.ai(0))?),
        SU_ISCNV => g.b(s.is_cn_v(op.ai(0))?),
        SU_D0 | SU_D1 | SU_D2 | SU_D3 | SU_VALUE => put_eval(g, &**s, op.code, op.a(0), op.a(1), 0, 0)?,
        SU_DN => put_eval(g, &**s, op.code, op.a(0), op.a(1), op.ai(2), op.ai(3))?,
        // elementary surfaces
        EL_SET_AXIS => elementary_mut(&mut **s).set_axis(op.ax1(0)?)?,
        EL_SET_LOC => elementary_mut(&mut **s).set_location(op.pnt(0)),
        EL_SET_POS => elementary_mut(&mut **s).set_position(op.ax3(0)?),
        EL_AXIS => g.ax1(&elementary_ref(&**s).axis()),
        EL_LOCATION => g.p(&elementary_ref(&**s).location()),
        EL_POSITION => g.ax3(elementary_ref(&**s).position()),
        PL_SET_PLN | PL_PLN | PL_COEFFS => {
            let k = s.as_any_mut().downcast_mut::<Plane>().expect("a plane");
            match op.code {
                PL_SET_PLN => k.set_pln(Pln::new(op.ax3(0)?)),
                PL_PLN => g.ax3(k.pln().position()),
                _ => {
                    let (a, b, c, d) = k.coefficients();
                    for x in [a, b, c, d] {
                        g.f(x);
                    }
                }
            }
        }
        CY_SET_CYL..=CY_RADIUS => {
            let k = s.as_any_mut().downcast_mut::<CylindricalSurface>().expect("a cylinder");
            match op.code {
                CY_SET_CYL => k.set_cylinder(Cylinder::new(op.ax3(0)?, op.a(10))?),
                CY_SET_RADIUS => k.set_radius(op.a(0))?,
                CY_CYL => {
                    let x = k.cylinder();
                    g.f(x.radius());
                    g.ax3(x.position());
                }
                CY_COEFFS => {
                    for x in k.coefficients() {
                        g.f(x);
                    }
                }
                _ => g.f(k.radius()),
            }
        }
        CO_SET_CONE..=CO_SEMIANGLE => {
            let k = s.as_any_mut().downcast_mut::<ConicalSurface>().expect("a cone");
            match op.code {
                CO_SET_CONE => k.set_cone(Cone::new(op.ax3(0)?, op.a(10), op.a(11))?),
                CO_SET_RADIUS => k.set_radius(op.a(0))?,
                CO_SET_SEMI => k.set_semi_angle(op.a(0))?,
                CO_CONE => {
                    let x = k.cone();
                    g.f(x.semi_angle());
                    g.f(x.ref_radius());
                    g.ax3(x.position());
                }
                CO_APEX => g.p(&k.apex()),
                CO_COEFFS => {
                    for x in k.coefficients() {
                        g.f(x);
                    }
                }
                CO_REFRADIUS => g.f(k.ref_radius()),
                _ => g.f(k.semi_angle()),
            }
        }
        SP_SET_SPHERE..=SP_RADIUS => {
            let k = s.as_any_mut().downcast_mut::<SphericalSurface>().expect("a sphere");
            match op.code {
                SP_SET_SPHERE => k.set_sphere(Sphere::new(op.ax3(0)?, op.a(10))?),
                SP_SET_RADIUS => k.set_radius(op.a(0))?,
                SP_SPHERE => {
                    let x = k.sphere();
                    g.f(x.radius());
                    g.ax3(x.position());
                }
                SP_AREA => g.f(k.area()),
                SP_VOLUME => g.f(k.volume()),
                SP_COEFFS => {
                    for x in k.coefficients() {
                        g.f(x);
                    }
                }
                _ => g.f(k.radius()),
            }
        }
        TO_SET_TORUS..=TO_MINOR => {
            let k = s.as_any_mut().downcast_mut::<ToroidalSurface>().expect("a torus");
            match op.code {
                TO_SET_TORUS => k.set_torus(Torus::new(op.ax3(0)?, op.a(10), op.a(11))?),
                TO_SET_MAJOR => k.set_major_radius(op.a(0))?,
                TO_SET_MINOR => k.set_minor_radius(op.a(0))?,
                TO_TORUS => {
                    let x = k.torus();
                    g.f(x.major_radius());
                    g.f(x.minor_radius());
                    g.ax3(x.position());
                }
                TO_AREA => g.f(k.area()),
                TO_VOLUME => g.f(k.volume()),
                TO_COEFFS => g.arr_f(&k.coefficients()),
                TO_MAJOR => g.f(k.major_radius()),
                _ => g.f(k.minor_radius()),
            }
        }
        // swept surfaces
        SW_DIRECTION => g.d(&swept_ref(&**s).direction()),
        SW_BASIS => put_state(g, &*swept_ref(&**s).basis_curve())?,
        SW_SET_DIR => {
            let v = op.dir(0)?;
            if let Some(x) = s.as_any_mut().downcast_mut::<SurfaceOfLinearExtrusion>() {
                x.set_direction(v);
            } else {
                s.as_any_mut().downcast_mut::<SurfaceOfRevolution>().expect("a revolution").set_direction(v);
            }
        }
        SW_SET_BASIS => {
            let c: Arc<dyn Curve> = Arc::from(build(cv2.expect("a second curve"))?);
            if let Some(x) = s.as_any_mut().downcast_mut::<SurfaceOfLinearExtrusion>() {
                x.set_basis_curve(c);
            } else {
                s.as_any_mut().downcast_mut::<SurfaceOfRevolution>().expect("a revolution").set_basis_curve(c);
            }
        }
        RV_SET_AXIS | RV_SET_LOC | RV_AXIS | RV_LOCATION | RV_REFPLANE => {
            let k = s.as_any_mut().downcast_mut::<SurfaceOfRevolution>().expect("a revolution");
            match op.code {
                RV_SET_AXIS => k.set_axis(op.ax1(0)?),
                RV_SET_LOC => k.set_location(op.pnt(0)),
                RV_AXIS => g.ax1(&k.axis()),
                RV_LOCATION => g.p(&k.location()),
                _ => g.ax2(&k.reference_plane()?),
            }
        }
        // rectangular trimmed surfaces
        RT_SET_TRIM => s
            .as_any_mut()
            .downcast_mut::<RectangularTrimmedSurface>()
            .expect("a trimmed surface")
            .set_trim(op.a(0), op.a(1), op.a(2), op.a(3), op.ai(4) != 0, op.ai(5) != 0)?,
        RT_SET_TRIM_P => s
            .as_any_mut()
            .downcast_mut::<RectangularTrimmedSurface>()
            .expect("a trimmed surface")
            .set_trim_param(op.a(0), op.a(1), op.ai(2) != 0, op.ai(3) != 0)?,
        RT_BASIS => put_state(
            g,
            &*s.as_any().downcast_ref::<RectangularTrimmedSurface>().expect("a trimmed surface").basis_surface(),
        )?,
        // offset surfaces
        OS_SET_BASIS => {
            let b: Arc<dyn Surface> = Arc::from(build_surface(sv2.expect("a second surface"))?);
            s.as_any_mut()
                .downcast_mut::<OffsetSurface>()
                .expect("an offset surface")
                .set_basis_surface(b, op.ai(0) != 0)?;
        }
        OS_SET_OFFSET | OS_OFFSET | OS_BASIS | OS_BASIS_CONT | OS_SURFACE | OS_UOSC | OS_VOSC => {
            if op.code == OS_SET_OFFSET {
                s.as_any_mut().downcast_mut::<OffsetSurface>().expect("an offset surface").set_offset_value(op.a(0));
                return Ok(());
            }
            let k = s.as_any().downcast_ref::<OffsetSurface>().expect("an offset surface");
            match op.code {
                OS_OFFSET => g.f(k.offset()),
                OS_BASIS => put_state(g, &*k.basis_surface())?,
                OS_BASIS_CONT => g.i(k.get_basis_surf_continuity() as i32),
                OS_SURFACE => {
                    let r = k.surface();
                    g.b(r.is_some());
                    if let Some(r) = r {
                        put_state(g, &*r)?;
                    }
                }
                _ => {
                    let r = if op.code == OS_UOSC {
                        k.u_osculating_surface(op.a(0), op.a(1))
                    } else {
                        k.v_osculating_surface(op.a(0), op.a(1))
                    };
                    g.b(r.is_some());
                    if let Some((opposite, surf)) = r {
                        g.b(opposite);
                        put_state(g, &surf)?;
                    }
                }
            }
        }
        // evaluation representations
        RS_SET_EREP => {
            let d = serep_desc(op, sv2)?;
            let a = s.as_any_mut();
            if let Some(b) = a.downcast_mut::<BezierSurface>() {
                b.set_eval_representation(Some(d))?;
            } else if let Some(b) = a.downcast_mut::<BSplineSurface>() {
                b.set_eval_representation(Some(d))?;
            } else if let Some(b) = a.downcast_mut::<SurfaceOfLinearExtrusion>() {
                b.set_eval_representation(Some(d))?;
            } else if let Some(b) = a.downcast_mut::<SurfaceOfRevolution>() {
                b.set_eval_representation(Some(d))?;
            } else {
                a.downcast_mut::<OffsetSurface>()
                    .expect("a surface with an evaluation representation")
                    .set_eval_representation(Some(d))?;
            }
        }
        RS_CLEAR_EREP => {
            let a = s.as_any_mut();
            if let Some(b) = a.downcast_mut::<BezierSurface>() {
                b.clear_eval_representation();
            } else if let Some(b) = a.downcast_mut::<BSplineSurface>() {
                b.clear_eval_representation();
            } else if let Some(b) = a.downcast_mut::<SurfaceOfLinearExtrusion>() {
                b.clear_eval_representation();
            } else if let Some(b) = a.downcast_mut::<SurfaceOfRevolution>() {
                b.clear_eval_representation();
            } else {
                a.downcast_mut::<OffsetSurface>()
                    .expect("a surface with an evaluation representation")
                    .clear_eval_representation();
            }
        }
        RS_HAS_EREP | RS_EREP_KIND => {
            let a = s.as_any();
            let d: Option<&SurfaceEvalRepDesc> = if let Some(b) = a.downcast_ref::<BezierSurface>() {
                b.eval_representation()
            } else if let Some(b) = a.downcast_ref::<BSplineSurface>() {
                b.eval_representation()
            } else if let Some(b) = a.downcast_ref::<SurfaceOfLinearExtrusion>() {
                b.eval_representation()
            } else if let Some(b) = a.downcast_ref::<SurfaceOfRevolution>() {
                b.eval_representation()
            } else {
                a.downcast_ref::<OffsetSurface>()
                    .expect("a surface with an evaluation representation")
                    .eval_representation()
            };
            if op.code == RS_HAS_EREP {
                g.b(d.is_some());
            } else {
                g.i(d.map_or(-1, |d| d.kind as i32));
            }
        }
        BZ_EXCHANGE..=BZ_MAXDEGREE | SB_POLE..=BB_NBV | BS_EXCHANGE..=BS_MAXDEGREE => bb_surf_op(g, s, op)?,
        k => panic!("surface op {k}"),
    }
    Ok(())
}

/// Bezier and B-spline surfaces (codes 420 ..).
fn bb_surf_op(g: &mut Got, s: &mut Box<dyn Surface>, op: &Op) -> R {
    // the operations of both kinds
    match op.code {
        SB_POLE | SB_WEIGHT | SB_POLES | SB_WEIGHTS | BB_UKNOTS | BB_VKNOTS | BB_UMULTS | BB_VMULTS | BB_UKNOTSEQ
        | BB_VKNOTSEQ | BB_NBU | BB_NBV | BZ_RESOLUTION | BZ_MAXDEGREE => {
            if let Some(b) = s.as_any_mut().downcast_mut::<BezierSurface>() {
                match op.code {
                    SB_POLE => g.p(&b.pole(op.ai(0), op.ai(1))?),
                    SB_WEIGHT => g.f(b.weight(op.ai(0), op.ai(1))?),
                    SB_POLES => g.arr2_p(b.poles()),
                    SB_WEIGHTS => {
                        g.b(b.weights().is_some());
                        if let Some(w) = b.weights() {
                            g.arr2_f(w);
                        }
                    }
                    BB_UKNOTS => g.arr_f(&b.u_knots()),
                    BB_VKNOTS => g.arr_f(&b.v_knots()),
                    BB_UMULTS => g.arr_i(&b.u_multiplicities()),
                    BB_VMULTS => g.arr_i(&b.v_multiplicities()),
                    BB_UKNOTSEQ => g.arr_f(&b.u_knot_sequence()),
                    BB_VKNOTSEQ => g.arr_f(&b.v_knot_sequence()),
                    BB_NBU => {
                        g.i(b.nb_u_poles());
                        g.i(b.u_degree());
                    }
                    BB_NBV => {
                        g.i(b.nb_v_poles());
                        g.i(b.v_degree());
                    }
                    BZ_RESOLUTION => {
                        let (u, v) = b.resolution(op.a(0));
                        g.f(u);
                        g.f(v);
                    }
                    _ => g.i(BezierSurface::max_degree()),
                }
                return Ok(());
            }
            let b = s.as_any_mut().downcast_mut::<BSplineSurface>().expect("a Bezier or B-spline surface");
            match op.code {
                SB_POLE => g.p(&b.pole(op.ai(0), op.ai(1))?),
                SB_WEIGHT => g.f(b.weight(op.ai(0), op.ai(1))?),
                SB_POLES => g.arr2_p(b.poles()),
                SB_WEIGHTS => {
                    g.b(b.weights().is_some());
                    if let Some(w) = b.weights() {
                        g.arr2_f(w);
                    }
                }
                BB_UKNOTS => g.arr_f(b.u_knots()),
                BB_VKNOTS => g.arr_f(b.v_knots()),
                BB_UMULTS => g.arr_i(b.u_multiplicities()),
                BB_VMULTS => g.arr_i(b.v_multiplicities()),
                BB_UKNOTSEQ => g.arr_f(b.u_knot_sequence()),
                BB_VKNOTSEQ => g.arr_f(b.v_knot_sequence()),
                BB_NBU => {
                    g.i(b.nb_u_poles());
                    g.i(b.u_degree());
                }
                BB_NBV => {
                    g.i(b.nb_v_poles());
                    g.i(b.v_degree());
                }
                BZ_RESOLUTION => {
                    let (u, v) = b.resolution(op.a(0));
                    g.f(u);
                    g.f(v);
                }
                _ => g.i(BSplineSurface::max_degree()),
            }
            return Ok(());
        }
        _ => {}
    }
    if (BZ_EXCHANGE..=BZ_SET_WEIGHT_ROW).contains(&op.code) {
        let b = s.as_any_mut().downcast_mut::<BezierSurface>().expect("a Bezier surface");
        match op.code {
            BZ_EXCHANGE => b.exchange_uv(),
            BZ_INCREASE => b.increase(op.ai(0), op.ai(1))?,
            BZ_INS_COL_AFTER | BZ_INS_COL_BEFORE | BZ_INS_ROW_AFTER | BZ_INS_ROW_BEFORE => {
                let mut i = 1;
                let p = a_pts(op, &mut i);
                let hw = op.ai(i) != 0;
                i += 1;
                let w = if hw { a_vals(op, &mut i) } else { Vec::new() };
                let idx = op.ai(0);
                match (op.code, hw) {
                    (BZ_INS_COL_AFTER, false) => b.insert_pole_col_after(idx, &p)?,
                    (BZ_INS_COL_AFTER, true) => b.insert_pole_col_after_weights(idx, &p, &w)?,
                    (BZ_INS_COL_BEFORE, false) => b.insert_pole_col_before(idx, &p)?,
                    (BZ_INS_COL_BEFORE, true) => b.insert_pole_col_before_weights(idx, &p, &w)?,
                    (BZ_INS_ROW_AFTER, false) => b.insert_pole_row_after(idx, &p)?,
                    (BZ_INS_ROW_AFTER, true) => b.insert_pole_row_after_weights(idx, &p, &w)?,
                    (_, false) => b.insert_pole_row_before(idx, &p)?,
                    (_, true) => b.insert_pole_row_before_weights(idx, &p, &w)?,
                }
            }
            BZ_REMOVE_COL => b.remove_pole_col(op.ai(0))?,
            BZ_REMOVE_ROW => b.remove_pole_row(op.ai(0))?,
            BZ_SEGMENT => b.segment(op.a(0), op.a(1), op.a(2), op.a(3)),
            BZ_SET_POLE => b.set_pole(op.ai(0), op.ai(1), op.pnt(2))?,
            BZ_SET_POLE_W => b.set_pole_weight(op.ai(0), op.ai(1), op.pnt(2), op.a(5))?,
            BZ_SET_POLE_COL | BZ_SET_POLE_ROW => {
                let mut i = 1;
                let p = a_pts(op, &mut i);
                let hw = op.ai(i) != 0;
                i += 1;
                let w = if hw { a_vals(op, &mut i) } else { Vec::new() };
                match (op.code, hw) {
                    (BZ_SET_POLE_COL, false) => b.set_pole_col(op.ai(0), &p)?,
                    (BZ_SET_POLE_COL, true) => b.set_pole_col_weights(op.ai(0), &p, &w)?,
                    (_, false) => b.set_pole_row(op.ai(0), &p)?,
                    (_, true) => b.set_pole_row_weights(op.ai(0), &p, &w)?,
                }
            }
            BZ_SET_WEIGHT => b.set_weight(op.ai(0), op.ai(1), op.a(2))?,
            _ => {
                let mut i = 1;
                let w = a_vals(op, &mut i);
                if op.code == BZ_SET_WEIGHT_COL {
                    b.set_weight_col(op.ai(0), &w)?
                } else {
                    b.set_weight_row(op.ai(0), &w)?
                }
            }
        }
        return Ok(());
    }
    let b = s.as_any_mut().downcast_mut::<BSplineSurface>().expect("a B-spline surface");
    match op.code {
        BS_EXCHANGE => b.exchange_uv(),
        BS_SET_UPER => b.set_u_periodic(),
        BS_SET_VPER => b.set_v_periodic(),
        BS_PER_NORM => {
            let (u, v) = b.periodic_normalization(op.a(0), op.a(1));
            g.f(u);
            g.f(v);
        }
        BS_SET_UORIGIN => b.set_u_origin(op.ai(0))?,
        BS_SET_VORIGIN => b.set_v_origin(op.ai(0))?,
        BS_SET_UNOTPER => b.set_u_not_periodic(),
        BS_SET_VNOTPER => b.set_v_not_periodic(),
        BS_INCREASE => b.increase_degree(op.ai(0), op.ai(1))?,
        BS_INS_UKNOTS | BS_INS_VKNOTS => {
            let mut i = 2;
            let k = a_vals(op, &mut i);
            let m = a_mults(op, &mut i);
            // with no knots the oracle passes empty arrays (the second count is then not read)
            let (k, m) = if op.ai(2) == 0 { (Vec::new(), Vec::new()) } else { (k, m) };
            if op.code == BS_INS_UKNOTS {
                b.insert_u_knots(&k, &m, op.a(0), op.ai(1) != 0)?;
            } else {
                b.insert_v_knots(&k, &m, op.a(0), op.ai(1) != 0)?;
            }
        }
        BS_REMOVE_UKNOT => g.b(b.remove_u_knot(op.ai(0), op.ai(1), op.a(2))?),
        BS_REMOVE_VKNOT => g.b(b.remove_v_knot(op.ai(0), op.ai(1), op.a(2))?),
        BS_INC_UMULT => b.increase_u_multiplicity(op.ai(0), op.ai(1))?,
        BS_INC_UMULT_R => b.increase_u_multiplicity_range(op.ai(0), op.ai(1), op.ai(2))?,
        BS_INCR_UMULT => b.increment_u_multiplicity(op.ai(0), op.ai(1), op.ai(2))?,
        BS_INC_VMULT => b.increase_v_multiplicity(op.ai(0), op.ai(1))?,
        BS_INC_VMULT_R => b.increase_v_multiplicity_range(op.ai(0), op.ai(1), op.ai(2))?,
        BS_INCR_VMULT => b.increment_v_multiplicity(op.ai(0), op.ai(1), op.ai(2))?,
        BS_INS_UKNOT => b.insert_u_knot(op.a(0), op.ai(1), op.a(2), op.ai(3) != 0)?,
        BS_INS_VKNOT => b.insert_v_knot(op.a(0), op.ai(1), op.a(2), op.ai(3) != 0)?,
        BS_SEGMENT => b.segment(op.a(0), op.a(1), op.a(2), op.a(3), op.a(4), op.a(5))?,
        BS_CHECK_SEGMENT => b.check_and_segment(op.a(0), op.a(1), op.a(2), op.a(3), op.a(4), op.a(5))?,
        BS_SET_UKNOT => b.set_u_knot(op.ai(0), op.a(1))?,
        BS_SET_UKNOT_M => b.set_u_knot_mult(op.ai(0), op.a(1), op.ai(2))?,
        BS_SET_VKNOT => b.set_v_knot(op.ai(0), op.a(1))?,
        BS_SET_VKNOT_M => b.set_v_knot_mult(op.ai(0), op.a(1), op.ai(2))?,
        BS_SET_UKNOTS | BS_SET_VKNOTS => {
            let mut i = 0;
            let k = a_vals(op, &mut i);
            if op.code == BS_SET_UKNOTS { b.set_u_knots(&k)? } else { b.set_v_knots(&k)? }
        }
        BS_LOCATE_U => {
            let (i1, i2) = b.locate_u(op.a(0), op.a(1), op.ai(2) != 0);
            g.i(i1);
            g.i(i2);
        }
        BS_LOCATE_V => {
            let (i1, i2) = b.locate_v(op.a(0), op.a(1), op.ai(2) != 0);
            g.i(i1);
            g.i(i2);
        }
        BS_SET_POLE => b.set_pole(op.ai(0), op.ai(1), op.pnt(2))?,
        BS_SET_POLE_W => b.set_pole_weight(op.ai(0), op.ai(1), op.pnt(2), op.a(5))?,
        BS_SET_POLE_COL | BS_SET_POLE_COL_W | BS_SET_POLE_ROW | BS_SET_POLE_ROW_W => {
            let mut i = 1;
            let p = a_pts(op, &mut i);
            let w = if op.code == BS_SET_POLE_COL_W || op.code == BS_SET_POLE_ROW_W {
                a_vals(op, &mut i)
            } else {
                Vec::new()
            };
            match op.code {
                BS_SET_POLE_COL => b.set_pole_col(op.ai(0), &p)?,
                BS_SET_POLE_COL_W => b.set_pole_col_weights(op.ai(0), &p, &w)?,
                BS_SET_POLE_ROW => b.set_pole_row(op.ai(0), &p)?,
                _ => b.set_pole_row_weights(op.ai(0), &p, &w)?,
            }
        }
        BS_SET_WEIGHT => b.set_weight(op.ai(0), op.ai(1), op.a(2))?,
        BS_SET_WEIGHT_COL | BS_SET_WEIGHT_ROW => {
            let mut i = 1;
            let w = a_vals(op, &mut i);
            if op.code == BS_SET_WEIGHT_COL { b.set_weight_col(op.ai(0), &w)? } else { b.set_weight_row(op.ai(0), &w)? }
        }
        BS_MOVE_POINT => {
            let (a1, a2, a3, a4) = b.move_point(op.a(0), op.a(1), op.pnt(2), op.ai(5), op.ai(6), op.ai(7), op.ai(8))?;
            for x in [a1, a2, a3, a4] {
                g.i(x);
            }
        }
        BS_FIRST_UKNOT => g.i(b.first_u_knot_index()),
        BS_FIRST_VKNOT => g.i(b.first_v_knot_index()),
        BS_LAST_UKNOT => g.i(b.last_u_knot_index()),
        BS_LAST_VKNOT => g.i(b.last_v_knot_index()),
        BS_UKNOT => g.f(b.u_knot(op.ai(0))?),
        BS_VKNOT => g.f(b.v_knot(op.ai(0))?),
        BS_UMULT => g.i(b.u_multiplicity(op.ai(0))?),
        BS_VMULT => g.i(b.v_multiplicity(op.ai(0))?),
        BS_LOCAL_VALUE => g.p(&b.local_value(op.a(0), op.a(1), op.ai(2), op.ai(3), op.ai(4), op.ai(5))?),
        BS_LOCAL_D0 => g.p(&b.local_d0(op.a(0), op.a(1), op.ai(2), op.ai(3), op.ai(4), op.ai(5))?),
        BS_LOCAL_D1 => {
            let (p, u, v) = b.local_d1(op.a(0), op.a(1), op.ai(2), op.ai(3), op.ai(4), op.ai(5))?;
            g.p(&p);
            g.v(&u);
            g.v(&v);
        }
        BS_LOCAL_D2 => {
            let (p, a1, a2, a3, a4, a5) = b.local_d2(op.a(0), op.a(1), op.ai(2), op.ai(3), op.ai(4), op.ai(5))?;
            g.p(&p);
            for x in [&a1, &a2, &a3, &a4, &a5] {
                g.v(x);
            }
        }
        BS_LOCAL_D3 => {
            let r = b.local_d3(op.a(0), op.a(1), op.ai(2), op.ai(3), op.ai(4), op.ai(5))?;
            g.p(&r.point);
            for x in [&r.d1u, &r.d1v, &r.d2u, &r.d2v, &r.d2uv, &r.d3u, &r.d3v, &r.d3uuv, &r.d3uvv] {
                g.v(x);
            }
        }
        BS_LOCAL_DN => {
            g.v(&b.local_dn(op.a(0), op.a(1), op.ai(2), op.ai(3), op.ai(4), op.ai(5), op.ai(6), op.ai(7))?)
        }
        BS_UISO_C => put_state(g, &b.u_iso_check(op.a(0), op.ai(1) != 0)?)?,
        BS_VISO_C => put_state(g, &b.v_iso_check(op.a(0), op.ai(1) != 0)?)?,
        BS_MAXDEGREE => g.i(BSplineSurface::max_degree()),
        k => panic!("B-spline surface op {k}"),
    }
    Ok(())
}

fn run_surface(g: &mut Got, sv: &SurfSpec, cv2: Option<&CurveSpec>, sv2: Option<&SurfSpec>, ops: &[Op]) -> R {
    let mut s = build_surface(sv)?;
    for op in ops {
        surf_op(g, &mut s, op, cv2, sv2)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Replaying one line

const SURFACE_PREFIXES: &[&str] = &[
    "surface_",
    "plane_",
    "cylinder_",
    "cone_",
    "sphere_",
    "torus_",
    "elementary_",
    "swept_",
    "extrusion_",
    "revolution_",
    "bezsurf_",
    "bsplsurf_",
    "rts_",
    "offsurf_",
    "evalrep_surf_",
];

fn run(name: &str, r: &mut Rd, g: &mut Got) -> R {
    match name {
        "cpoint_ops" | "cpoint_xform" => {
            let ctor = r.i();
            let c = r.fl::<3>();
            let ops = r.ops();
            run_point(g, ctor, c, &ops)
        }
        n if n.starts_with("direction_") || n.starts_with("vecmag_") => {
            let kind = r.i();
            let ctor = r.i();
            let v = r.fl::<6>();
            let ops = r.ops();
            run_vec(g, kind, ctor, v, &ops)
        }
        n if n.starts_with("axis1_") => {
            let ctor = r.i();
            let mut v = [0.0; 9];
            v[..6].copy_from_slice(&r.fl::<6>());
            let ops = r.ops();
            run_axis(g, false, ctor, v, &ops)
        }
        n if n.starts_with("axis2_") => {
            let ctor = r.i();
            let v = r.fl::<9>();
            let ops = r.ops();
            run_axis(g, true, ctor, v, &ops)
        }
        n if n.starts_with("trsf_") => {
            let t = r.tr();
            let ops = r.ops();
            run_trsf(g, &t, &ops)
        }
        n if SURFACE_PREFIXES.iter().any(|p| n.starts_with(p)) => {
            let sv = r.srf();
            let cv2 = if SURF_CURVE.contains(&n) { Some(r.crv()) } else { None };
            let sv2 = if TWO_SURFACES.contains(&n) { Some(r.srf()) } else { None };
            let ops = r.ops();
            run_surface(g, &sv, cv2.as_ref(), sv2.as_ref(), &ops)
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
fn geom_matches_occt() {
    let text = include_str!("geom.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let (mut n_throws, mut n_unchecked) = (0usize, 0usize);

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in geom.txt:\n{line}");
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
            // Review 2026-10-26: one curve_reverse line printed uninitialised OCCT memory and
            // was removed (docs/api-geom.md, "Removed judge line").
            let min = if *name == "curve_reverse" { MIN_DISTINCT - 1 } else { MIN_DISTINCT };
            assert!(*count >= min, "family {name} has only {count} distinct lines");
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
