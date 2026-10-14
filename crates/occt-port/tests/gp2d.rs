//! Numerical reference test for the in-scope gp 2D value algebra (OCCT 8.0.1).
//!
//! `gp2d.txt` is printed by `oracle/gp2d.cpp` (real OCCT). Each line is
//! `<family> <inputs...> | <outputs...>`: f64 values are 16-hex-digit bit patterns, ints
//! are decimal and bools are 0/1. Every input of a call is on the left of ` | `; this test
//! reads them back with the `Rd` reader methods of the same name as the oracle's schema
//! words (`f i xy pnt vec dir ax2d ax22d mat lin circ elips hypr parab trsf3d trsf2d gtrsf2d`)
//! and replays the call on the Rust API of docs/api-gp2d.md. The right side is either the
//! expected values, `throws` (OCCT raised) or `unchecked` (OCCT 8.0.1 as built has that check
//! compiled out and returns unspecified numbers; the Rust API must return an error there).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::gp::{
    self, Ax1, Ax2, Ax2d, Ax22d, Circ2d, Dir, Dir2d, Direction2d, Elips2d, GTrsf2d, Hypr2d, Lin2d, Mat2d, Parab2d, Pnt,
    Pnt2d, Quaternion, Trsf, Vec as GpVec3, Vec2d, XY, XYZ,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for sin/cos/atan2/acos/asin dependent results, in units of
/// f64::EPSILON times the largest absolute value on the line (at least 1).
const MAX_ULPS: f64 = 8.0;

/// Every family with inputs must have at least this many distinct lines.
const MIN_DISTINCT: usize = 16;

/// Families that really take no input: they are printed exactly once.
const INPUT_LESS: &[&str] = &["constants2d"];

/// Families whose results use only + - * / and sqrt: compared bit for bit. All other families
/// depend on sin/cos/atan2/acos/asin somewhere (or are bool/int results of such a computation) and
/// are compared with MAX_ULPS.
const EXACT: &[&str] = &[
    "ax22d_access",
    "ax22d_construct",
    "ax22d_mirror",
    "ax22d_scale_transform_translate",
    "ax2d_access",
    "ax2d_mirror_axis",
    "ax2d_mirror_point",
    "ax2d_reverse",
    "ax2d_scale",
    "ax2d_transform",
    "ax2d_translate",
    "axis2d_enum_constructors",
    "circ2d_access",
    "circ2d_constructors",
    "circ2d_motion",
    "circ2d_queries",
    "circ2d_radius_checks",
    "constants2d",
    "dir2d_bad_coord",
    "dir2d_bad_set_index",
    "dir2d_construct",
    "dir2d_mirrors",
    "dir2d_ops",
    "dir2d_setters",
    "dir2d_transform",
    "elips2d_access",
    "elips2d_coefficients",
    "elips2d_constructors",
    "elips2d_directrix",
    "elips2d_motion",
    "elips2d_radius_checks",
    "gtrsf2d_bad_set_value",
    "gtrsf2d_bad_value",
    "gtrsf2d_composition",
    "gtrsf2d_composition_all",
    "gtrsf2d_constructors",
    "gtrsf2d_invert",
    "gtrsf2d_power",
    "gtrsf2d_queries",
    "gtrsf2d_setters",
    "gtrsf2d_to_trsf2d",
    "gtrsf2d_value_transform",
    "hypr2d_access",
    "hypr2d_asymptotes",
    "hypr2d_coefficients",
    "hypr2d_constructors",
    "hypr2d_directrix",
    "hypr2d_eccentricity",
    "hypr2d_motion",
    "hypr2d_radius_checks",
    "lin2d_access",
    "lin2d_constructors",
    "lin2d_from_coefficients",
    "lin2d_motion",
    "lin2d_point_queries",
    "mat2d_access",
    "mat2d_arithmetic",
    "mat2d_bad_change_value",
    "mat2d_bad_column",
    "mat2d_bad_index",
    "mat2d_bad_row",
    "mat2d_bad_set_column",
    "mat2d_bad_set_row",
    "mat2d_bad_set_value",
    "mat2d_constructors",
    "mat2d_divide_zero",
    "mat2d_inverse",
    "mat2d_power",
    "mat2d_setters",
    "mat2d_singular",
    "mat2d_transpose",
    "parab2d_access",
    "parab2d_coefficients",
    "parab2d_constructors",
    "parab2d_focal_checks",
    "parab2d_motion",
    "pnt2d_access",
    "pnt2d_bad_coord",
    "pnt2d_bad_set_index",
    "pnt2d_constructors",
    "pnt2d_distance",
    "pnt2d_equal",
    "pnt2d_mirror_axis",
    "pnt2d_mirror_point",
    "pnt2d_scale",
    "pnt2d_setters",
    "pnt2d_transform",
    "pnt2d_translate_points",
    "pnt2d_translate_vec",
    "trsf2d_bad_value",
    "trsf2d_composition",
    "trsf2d_inverse",
    "trsf2d_mirror_axis",
    "trsf2d_mirror_point",
    "trsf2d_power",
    "trsf2d_queries",
    "trsf2d_scale",
    "trsf2d_scale_factor",
    "trsf2d_set_values",
    "trsf2d_transformation",
    "trsf2d_transforms",
    "trsf2d_translation",
    "trsf2d_translation_part",
    "trsf2d_values",
    "vec2d_access",
    "vec2d_arithmetic",
    "vec2d_bad_coord",
    "vec2d_bad_set_index",
    "vec2d_constructors",
    "vec2d_cross",
    "vec2d_mirrors",
    "vec2d_mutators",
    "vec2d_normalize",
    "vec2d_scale_transform",
    "xy_access",
    "xy_add",
    "xy_bad_change_index",
    "xy_bad_index",
    "xy_bad_set_index",
    "xy_constructors",
    "xy_cross",
    "xy_divide",
    "xy_dot",
    "xy_equal",
    "xy_linear_forms",
    "xy_modulus",
    "xy_multiply",
    "xy_multiply_matrix",
    "xy_normalize",
    "xy_reverse",
    "xy_setters",
    "xy_subtract",
];

/// Every family the oracle must print (a family that disappears from gp2d.txt fails).
const FAMILIES: &[&str] = &[
    "ax22d_access",
    "ax22d_construct",
    "ax22d_mirror",
    "ax22d_rotate",
    "ax22d_scale_transform_translate",
    "ax2d_access",
    "ax2d_mirror_axis",
    "ax2d_mirror_point",
    "ax2d_relations",
    "ax2d_reverse",
    "ax2d_rotate",
    "ax2d_scale",
    "ax2d_transform",
    "ax2d_translate",
    "axis2d_enum_constructors",
    "circ2d_access",
    "circ2d_constructors",
    "circ2d_motion",
    "circ2d_queries",
    "circ2d_radius_checks",
    "circ2d_rotate",
    "constants2d",
    "dir2d_angle",
    "dir2d_bad_coord",
    "dir2d_bad_set_index",
    "dir2d_construct",
    "dir2d_mirrors",
    "dir2d_ops",
    "dir2d_queries",
    "dir2d_rotate",
    "dir2d_setters",
    "dir2d_transform",
    "elips2d_access",
    "elips2d_coefficients",
    "elips2d_constructors",
    "elips2d_directrix",
    "elips2d_motion",
    "elips2d_radius_checks",
    "elips2d_rotate",
    "gtrsf2d_bad_set_value",
    "gtrsf2d_bad_value",
    "gtrsf2d_composition",
    "gtrsf2d_composition_all",
    "gtrsf2d_constructors",
    "gtrsf2d_invert",
    "gtrsf2d_power",
    "gtrsf2d_queries",
    "gtrsf2d_rotation",
    "gtrsf2d_setters",
    "gtrsf2d_to_trsf2d",
    "gtrsf2d_value_transform",
    "hypr2d_access",
    "hypr2d_asymptotes",
    "hypr2d_coefficients",
    "hypr2d_constructors",
    "hypr2d_directrix",
    "hypr2d_eccentricity",
    "hypr2d_motion",
    "hypr2d_radius_checks",
    "hypr2d_rotate",
    "lin2d_access",
    "lin2d_constructors",
    "lin2d_from_coefficients",
    "lin2d_line_queries",
    "lin2d_motion",
    "lin2d_point_queries",
    "lin2d_rotate",
    "mat2d_access",
    "mat2d_arithmetic",
    "mat2d_bad_change_value",
    "mat2d_bad_column",
    "mat2d_bad_index",
    "mat2d_bad_row",
    "mat2d_bad_set_column",
    "mat2d_bad_set_row",
    "mat2d_bad_set_value",
    "mat2d_constructors",
    "mat2d_divide_zero",
    "mat2d_inverse",
    "mat2d_power",
    "mat2d_rotation",
    "mat2d_setters",
    "mat2d_singular",
    "mat2d_transpose",
    "parab2d_access",
    "parab2d_coefficients",
    "parab2d_constructors",
    "parab2d_focal_checks",
    "parab2d_motion",
    "parab2d_rotate",
    "parab2d_zero_focal",
    "pnt2d_access",
    "pnt2d_bad_coord",
    "pnt2d_bad_set_index",
    "pnt2d_constructors",
    "pnt2d_distance",
    "pnt2d_equal",
    "pnt2d_mirror_axis",
    "pnt2d_mirror_point",
    "pnt2d_rotate",
    "pnt2d_scale",
    "pnt2d_setters",
    "pnt2d_transform",
    "pnt2d_transform_rotation",
    "pnt2d_translate_points",
    "pnt2d_translate_vec",
    "trsf2d_bad_value",
    "trsf2d_composition",
    "trsf2d_composition_rotation",
    "trsf2d_from_trsf",
    "trsf2d_inverse",
    "trsf2d_mirror_axis",
    "trsf2d_mirror_point",
    "trsf2d_power",
    "trsf2d_queries",
    "trsf2d_rotation",
    "trsf2d_rotation_ops",
    "trsf2d_rotation_part",
    "trsf2d_scale",
    "trsf2d_scale_factor",
    "trsf2d_set_values",
    "trsf2d_transformation",
    "trsf2d_transforms",
    "trsf2d_transforms_rotation",
    "trsf2d_translation",
    "trsf2d_translation_part",
    "trsf2d_values",
    "vec2d_access",
    "vec2d_arithmetic",
    "vec2d_bad_coord",
    "vec2d_bad_set_index",
    "vec2d_constructors",
    "vec2d_cross",
    "vec2d_mirrors",
    "vec2d_mutators",
    "vec2d_normalize",
    "vec2d_queries",
    "vec2d_relations",
    "vec2d_rotate",
    "vec2d_scale_transform",
    "vec2d_zero_angle",
    "vec2d_zero_is_equal",
    "vec2d_zero_is_normal",
    "vec2d_zero_is_opposite",
    "vec2d_zero_is_parallel",
    "xy_access",
    "xy_add",
    "xy_bad_change_index",
    "xy_bad_index",
    "xy_bad_set_index",
    "xy_constructors",
    "xy_cross",
    "xy_divide",
    "xy_dot",
    "xy_equal",
    "xy_linear_forms",
    "xy_modulus",
    "xy_multiply",
    "xy_multiply_matrix",
    "xy_normalize",
    "xy_reverse",
    "xy_setters",
    "xy_subtract",
];

/// The number of lines of the golden file.
const GOLDEN_LINES: usize = 3102;

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
    I(i32),
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
    fn i(&mut self, v: i32) {
        self.0.push(Tok::I(v));
    }
    fn xy(&mut self, v: &XY) {
        self.fs(&[v.x(), v.y()]);
    }
    fn pnt(&mut self, v: &Pnt2d) {
        self.xy(v.xy());
    }
    fn vec(&mut self, v: &Vec2d) {
        self.xy(v.xy());
    }
    fn dir(&mut self, v: &Dir2d) {
        self.xy(v.xy());
    }
    fn ax2d(&mut self, v: &Ax2d) {
        self.pnt(v.location());
        self.dir(v.direction());
    }
    fn ax22d(&mut self, v: &Ax22d) {
        self.pnt(v.location());
        self.dir(v.x_direction());
        self.dir(v.y_direction());
    }
    fn mat(&mut self, v: &Mat2d) {
        for row in 1..=2 {
            for col in 1..=2 {
                self.f(v.value(row, col).expect("valid index"));
            }
        }
    }
    /// A Trsf2d is compared through its internal state: form, scale factor, raw matrix, translation.
    fn trsf(&mut self, t: &gp::Trsf2d) {
        self.i(t.form() as i32);
        self.f(t.scale_factor());
        self.mat(t.h_vectorial_part());
        self.xy(t.translation_part());
    }
    /// A GTrsf2d: form, raw matrix, translation, then the six values (they expose the hidden scale).
    fn gtrsf(&mut self, t: &GTrsf2d) {
        self.i(t.form() as i32);
        self.mat(t.vectorial_part());
        self.xy(t.translation_part());
        for row in 1..=2 {
            for col in 1..=3 {
                self.f(t.value(row, col).expect("valid index"));
            }
        }
    }
    fn lin(&mut self, v: &Lin2d) {
        self.ax2d(v.position());
    }
    fn circ(&mut self, v: &Circ2d) {
        self.ax22d(v.position());
        self.f(v.radius());
    }
    fn elips(&mut self, v: &Elips2d) {
        self.ax22d(v.axis());
        self.f(v.major_radius());
        self.f(v.minor_radius());
    }
    fn hypr(&mut self, v: &Hypr2d) {
        self.ax22d(v.axis());
        self.f(v.major_radius());
        self.f(v.minor_radius());
    }
    fn parab(&mut self, v: &Parab2d) {
        self.ax22d(v.axis());
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
    fn xy(&mut self) -> XY {
        let x = self.f();
        let y = self.f();
        XY::new(x, y)
    }
    fn pnt(&mut self) -> Pnt2d {
        let x = self.f();
        let y = self.f();
        Pnt2d::new(x, y)
    }
    fn vec(&mut self) -> Vec2d {
        let x = self.f();
        let y = self.f();
        Vec2d::new(x, y)
    }
    /// A raw (un-normalized) direction; `Dir2d::new` normalizes it exactly like the oracle did.
    fn dir(&mut self) -> Result<Dir2d, Thrown> {
        let x = self.f();
        let y = self.f();
        Ok(Dir2d::new(x, y)?)
    }
    /// Location and a raw direction.
    fn ax2d(&mut self) -> Result<Ax2d, Thrown> {
        let p = self.pnt();
        let d = self.dir()?;
        Ok(Ax2d::new(p, d))
    }
    /// Location, X direction (raw) and a second raw direction that only selects the handedness.
    fn ax22d_parts(&mut self) -> Result<(Pnt2d, Dir2d, Dir2d), Thrown> {
        let p = self.pnt();
        let vx = self.dir()?;
        let vy = self.dir()?;
        Ok((p, vx, vy))
    }
    fn ax22d(&mut self) -> Result<Ax22d, Thrown> {
        let (p, vx, vy) = self.ax22d_parts()?;
        Ok(Ax22d::from_dirs(p, vx, vy))
    }
    /// Four values, row-major.
    fn mat(&mut self) -> Mat2d {
        let mut v = [0.0; 4];
        for e in &mut v {
            *e = self.f();
        }
        Mat2d::from_columns(XY::new(v[0], v[2]), XY::new(v[1], v[3]))
    }
    fn lin(&mut self) -> Result<Lin2d, Thrown> {
        Ok(Lin2d::from_ax2d(self.ax2d()?))
    }
    fn circ(&mut self) -> Result<Circ2d, Thrown> {
        let a = self.ax22d()?;
        let r = self.f();
        Ok(Circ2d::from_ax22d(a, r)?)
    }
    fn elips(&mut self) -> Result<Elips2d, Thrown> {
        let a = self.ax22d()?;
        let (major, minor) = (self.f(), self.f());
        Ok(Elips2d::from_ax22d(a, major, minor)?)
    }
    fn hypr(&mut self) -> Result<Hypr2d, Thrown> {
        let a = self.ax22d()?;
        let (major, minor) = (self.f(), self.f());
        Ok(Hypr2d::from_ax22d(a, major, minor)?)
    }
    fn parab(&mut self) -> Result<Parab2d, Thrown> {
        let a = self.ax22d()?;
        let focal = self.f();
        Ok(Parab2d::from_ax22d(a, focal)?)
    }

    // ---- 3D recipe of gp_Trsf2d(gp_Trsf) -----------------------------------------
    fn pnt3(&mut self) -> Pnt {
        let x = self.f();
        let y = self.f();
        let z = self.f();
        Pnt::new(x, y, z)
    }
    fn xyz3(&mut self) -> XYZ {
        let x = self.f();
        let y = self.f();
        let z = self.f();
        XYZ::new(x, y, z)
    }
    fn dir3(&mut self) -> Result<Dir, Thrown> {
        let x = self.f();
        let y = self.f();
        let z = self.f();
        Ok(Dir::new(x, y, z)?)
    }
    fn ax1_3(&mut self) -> Result<Ax1, Thrown> {
        let p = self.pnt3();
        let d = self.dir3()?;
        Ok(Ax1::new(p, d))
    }
    fn quat3(&mut self) -> Quaternion {
        let x = self.f();
        let y = self.f();
        let z = self.f();
        let w = self.f();
        Quaternion::from_components(x, y, z, w)
    }
    /// A 3D transformation recipe: kind, then the fields that kind uses (see oracle `mkT3`).
    fn trsf3d(&mut self) -> Result<Trsf, Thrown> {
        let kind = self.i();
        let mut t = Trsf::new();
        match kind {
            0 | 1 => {
                let axis = self.ax1_3()?;
                let angle = self.f();
                t.set_rotation(&axis, angle)?;
                if kind == 1 {
                    let v = self.xyz3();
                    t.set_translation_part(&GpVec3::from_xyz(v));
                }
            }
            2 => {
                let p = self.pnt3();
                let factor = self.f();
                t.set_scale(&p, factor)?;
            }
            3 => {
                let v = self.xyz3();
                t.set_translation(&GpVec3::from_xyz(v));
            }
            4 => {
                let axis = self.ax1_3()?;
                t.set_mirror(&axis);
            }
            5 => {
                let p = self.pnt3();
                t.set_mirror(&p);
            }
            6 => {
                let p = self.pnt3();
                let d = self.dir3()?;
                t.set_mirror(&Ax2::from_direction(p, d));
            }
            7 | 8 => {
                let q = self.quat3();
                t.set_rotation_quaternion(&q)?;
                if kind == 8 {
                    let v = self.xyz3();
                    let factor = self.f();
                    t.set_translation_part(&GpVec3::from_xyz(v));
                    t.set_scale_factor(factor)?;
                }
            }
            _ => panic!("unknown 3D transformation recipe {kind}"),
        }
        Ok(t)
    }

    /// A 2D transformation recipe: kind, then the fields that kind uses (see oracle `mkT2`).
    fn trsf2d(&mut self) -> Result<gp::Trsf2d, Thrown> {
        let kind = self.i();
        let mut t = gp::Trsf2d::new();
        match kind {
            0 | 1 => {
                let p = self.pnt();
                let angle = self.f();
                t.set_rotation(&p, angle);
                if kind == 1 {
                    let v = self.xy();
                    t.set_translation_part(&Vec2d::from_xy(v));
                }
            }
            2 => {
                let p = self.pnt();
                let factor = self.f();
                t.set_scale(&p, factor);
            }
            3 => {
                let v = self.xy();
                t.set_translation(&Vec2d::from_xy(v));
            }
            4 => {
                let (p, q) = (self.pnt(), self.pnt());
                t.set_translation_points(&p, &q);
            }
            5 => {
                let p = self.pnt();
                t.set_mirror(&p);
            }
            6 => {
                let a = self.ax2d()?;
                t.set_mirror(&a);
            }
            7 => {
                let a = self.ax2d()?;
                t.set_transformation(&a);
            }
            8 => {
                let from = self.ax2d()?;
                let to = self.ax2d()?;
                t.set_transformation_between(&from, &to);
            }
            9 => {
                let a = self.ax2d()?;
                let factor = self.f();
                let v = self.xy();
                t.set_mirror(&a);
                t.set_scale_factor(factor);
                t.set_translation_part(&Vec2d::from_xy(v));
            }
            10 => {
                let mut v = [0.0; 6];
                for e in &mut v {
                    *e = self.f();
                }
                t.set_values(v)?;
            }
            11 => {
                let t3 = self.trsf3d()?;
                t = gp::Trsf2d::from_trsf(&t3);
            }
            12 => {}
            _ => panic!("unknown 2D transformation recipe {kind}"),
        }
        Ok(t)
    }
    /// 0: GTrsf2d from a Trsf2d recipe, 1: from a matrix and a translation, 2: default + affinity.
    fn gtrsf2d(&mut self) -> Result<GTrsf2d, Thrown> {
        match self.i() {
            0 => Ok(GTrsf2d::from_trsf2d(&self.trsf2d()?)),
            1 => {
                let m = self.mat();
                let v = self.xy();
                Ok(GTrsf2d::from_parts(m, v))
            }
            2 => {
                let a = self.ax2d()?;
                let ratio = self.f();
                let mut g = GTrsf2d::new();
                g.set_affinity(&a, ratio);
                Ok(g)
            }
            k => panic!("unknown general transformation recipe {k}"),
        }
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread inputs");
    }
}

// The shared bodies of the motion families of the curves and frames. The object has been read
// by the caller; the rest of the inputs are read here in the oracle's order.
macro_rules! motion_exact {
    ($g:expr, $r:expr, $o:expr, $put:ident) => {{
        let cen = $r.pnt();
        let mx = $r.ax2d()?;
        let sp = $r.pnt();
        let ss = $r.f();
        let tr = $r.trsf2d()?;
        let dv = $r.xy();
        let (p1, p2) = ($r.pnt(), $r.pnt());
        let o = $o;
        $g.$put(&o.mirrored(&cen));
        let mut t = o;
        t.mirror(&cen);
        $g.$put(&t);
        $g.$put(&o.mirrored(&mx));
        t = o;
        t.mirror(&mx);
        $g.$put(&t);
        $g.$put(&o.scaled(&sp, ss));
        t = o;
        t.scale(&sp, ss);
        $g.$put(&t);
        $g.$put(&o.transformed(&tr));
        t = o;
        t.transform(&tr);
        $g.$put(&t);
        $g.$put(&o.translated(&Vec2d::from_xy(dv)));
        t = o;
        t.translate(&Vec2d::from_xy(dv));
        $g.$put(&t);
        $g.$put(&o.translated_points(&p1, &p2));
        t = o;
        t.translate_points(&p1, &p2);
        $g.$put(&t);
    }};
}
macro_rules! motion_rotate {
    ($g:expr, $r:expr, $o:expr, $put:ident) => {{
        let rc = $r.pnt();
        let ang = $r.f();
        let tr = $r.trsf2d()?;
        let o = $o;
        $g.$put(&o.rotated(&rc, ang));
        let mut t = o;
        t.rotate(&rc, ang);
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
        // ---- gp_XY ----
        "xy_constructors" => {
            let (x, y) = (r.f(), r.f());
            g.xy(&XY::default());
            g.xy(&XY::new(x, y));
        }
        "xy_add" => {
            let (a, c) = (r.xy(), r.xy());
            g.xy(&a.added(&c));
            let mut t = a;
            t.add(&c);
            g.xy(&t);
        }
        "xy_subtract" => {
            let (a, c) = (r.xy(), r.xy());
            g.xy(&a.subtracted(&c));
            let mut t = a;
            t.subtract(&c);
            g.xy(&t);
        }
        "xy_cross" => {
            let (a, c) = (r.xy(), r.xy());
            g.fs(&[a.crossed(&c), a.cross_magnitude(&c), a.cross_square_magnitude(&c)]);
        }
        "xy_dot" => {
            let (a, c) = (r.xy(), r.xy());
            g.f(a.dot(&c));
        }
        "xy_modulus" => {
            let a = r.xy();
            g.fs(&[a.modulus(), a.square_modulus()]);
        }
        "xy_multiply" => {
            let (a, c, s) = (r.xy(), r.xy(), r.f());
            g.xy(&a.multiplied(s));
            g.xy(&a.multiplied_xy(&c));
            let mut t = a;
            t.multiply(s);
            g.xy(&t);
            t = a;
            t.multiply_xy(&c);
            g.xy(&t);
        }
        "xy_multiply_matrix" => {
            let (a, m) = (r.xy(), r.mat());
            g.xy(&a.multiplied_mat(&m));
            let mut t = a;
            t.multiply_mat(&m);
            g.xy(&t);
        }
        "xy_divide" => {
            let (a, s) = (r.xy(), r.f());
            // OCCT does not check the divisor; the zero divisor is an error by contract.
            if s.abs() <= gp::resolution() {
                return Err(Thrown);
            }
            g.xy(&a.divided(s));
            let mut t = a;
            t.divide(s);
            g.xy(&t);
        }
        "xy_reverse" => {
            let a = r.xy();
            g.xy(&a.reversed());
            let mut t = a;
            t.reverse();
            g.xy(&t);
        }
        "xy_equal" => {
            let (a, c, tol) = (r.xy(), r.xy(), r.f());
            g.b(a.is_equal(&c, tol));
        }
        "xy_access" => {
            let (a, idx) = (r.xy(), r.i());
            g.fs(&[a.coord(1)?, a.coord(2)?, a.coord(idx)?, a.x(), a.y()]);
            let (cx, cy) = a.coords();
            g.fs(&[cx, cy]);
        }
        "xy_bad_index" => {
            let (a, idx) = (r.xy(), r.i());
            g.f(a.coord(idx)?);
        }
        "xy_bad_set_index" => {
            let (a, idx, val) = (r.xy(), r.i(), r.f());
            let mut t = a;
            t.set_coord_at(idx, val)?;
            g.xy(&t);
        }
        "xy_bad_change_index" => {
            let (a, idx) = (r.xy(), r.i());
            let mut t = a;
            g.f(*t.change_coord(idx)?);
        }
        "xy_setters" => {
            let (a, c, s, i1, i2) = (r.xy(), r.xy(), r.f(), r.i(), r.i());
            let mut t = a;
            t.set_coord_at(i1, c.x())?;
            g.xy(&t);
            t.set_x(c.y());
            g.xy(&t);
            t.set_y(s);
            g.xy(&t);
            t.set_coord(a.x(), c.x());
            g.xy(&t);
            *t.change_coord(i2)? = a.y();
            g.xy(&t);
        }
        "xy_linear_forms" => {
            let (a, c, s, r2, e1, e2) = (r.xy(), r.xy(), r.f(), r.f(), r.xy(), r.xy());
            let mut t = XY::zero();
            t.set_linear_form_2(s, &a, r2, &c);
            g.xy(&t);
            t.set_linear_form_2_plus(s, &a, r2, &c, &e1);
            g.xy(&t);
            t.set_linear_form_1_plus(s, &a, &e2);
            g.xy(&t);
            t.set_linear_form_sum(&a, &c);
            g.xy(&t);
        }
        "xy_normalize" => {
            let a = r.xy();
            g.xy(&a.normalized()?);
            let mut t = a;
            t.normalize()?;
            g.xy(&t);
        }

        // ---- gp_Pnt2d ----
        "pnt2d_constructors" => {
            let (a, u, v) = (r.xy(), r.f(), r.f());
            g.pnt(&Pnt2d::default());
            g.pnt(&Pnt2d::from_xy(a));
            g.pnt(&Pnt2d::new(u, v));
        }
        "pnt2d_access" => {
            let p = r.pnt();
            g.fs(&[p.x(), p.y(), p.coord(1)?, p.coord(2)?]);
            g.xy(p.xy());
            let (px, py) = p.coords();
            g.fs(&[px, py]);
        }
        "pnt2d_bad_coord" => {
            let (p, idx) = (r.pnt(), r.i());
            g.f(p.coord(idx)?);
        }
        "pnt2d_bad_set_index" => {
            let (p, idx, val) = (r.pnt(), r.i(), r.f());
            let mut t = p;
            t.set_coord_at(idx, val)?;
            g.pnt(&t);
        }
        "pnt2d_setters" => {
            let (p, c, s, i1) = (r.pnt(), r.xy(), r.f(), r.i());
            let mut t = p;
            t.set_coord_at(i1, c.x())?;
            g.pnt(&t);
            t.set_coord(c.y(), s);
            g.pnt(&t);
            t.set_x(p.x());
            g.pnt(&t);
            t.set_y(p.y());
            g.pnt(&t);
            t.set_xy(c);
            g.pnt(&t);
            t.change_coord().set_coord(s, p.x());
            g.pnt(&t);
        }
        "pnt2d_equal" => {
            let (p, q, tol) = (r.pnt(), r.pnt(), r.f());
            g.b(p.is_equal(&q, tol));
        }
        "pnt2d_distance" => {
            let (p, q) = (r.pnt(), r.pnt());
            g.fs(&[p.distance(&q), p.square_distance(&q)]);
        }
        "pnt2d_mirror_point" => {
            let (p, q) = (r.pnt(), r.pnt());
            g.pnt(&p.mirrored(&q));
            let mut t = p;
            t.mirror(&q);
            g.pnt(&t);
        }
        "pnt2d_mirror_axis" => {
            let (p, ax) = (r.pnt(), r.ax2d()?);
            g.pnt(&p.mirrored(&ax));
            let mut t = p;
            t.mirror(&ax);
            g.pnt(&t);
        }
        "pnt2d_rotate" => {
            let (p, q, s) = (r.pnt(), r.pnt(), r.f());
            g.pnt(&p.rotated(&q, s));
            let mut t = p;
            t.rotate(&q, s);
            g.pnt(&t);
        }
        "pnt2d_scale" => {
            let (p, q, s) = (r.pnt(), r.pnt(), r.f());
            g.pnt(&p.scaled(&q, s));
            let mut t = p;
            t.scale(&q, s);
            g.pnt(&t);
        }
        "pnt2d_translate_vec" => {
            let (p, v) = (r.pnt(), r.vec());
            g.pnt(&p.translated(&v));
            let mut t = p;
            t.translate(&v);
            g.pnt(&t);
        }
        "pnt2d_translate_points" => {
            let (p, p1, p2) = (r.pnt(), r.pnt(), r.pnt());
            g.pnt(&p.translated_points(&p1, &p2));
            let mut t = p;
            t.translate_points(&p1, &p2);
            g.pnt(&t);
        }
        "pnt2d_transform" | "pnt2d_transform_rotation" => {
            let (p, tr) = (r.pnt(), r.trsf2d()?);
            g.pnt(&p.transformed(&tr));
            let mut t = p;
            t.transform(&tr);
            g.pnt(&t);
        }

        // ---- gp_Vec2d ----
        "vec2d_constructors" => {
            let (a, c, d1) = (r.xy(), r.xy(), r.dir()?);
            g.vec(&Vec2d::default());
            g.vec(&Vec2d::from_xy(a));
            g.vec(&Vec2d::new(a.x(), a.y()));
            g.vec(&Vec2d::from_dir(&d1));
            g.vec(&Vec2d::between(&Pnt2d::from_xy(a), &Pnt2d::from_xy(c)));
        }
        "vec2d_access" => {
            let (vv, ww, s, i1) = (r.vec(), r.vec(), r.f(), r.i());
            g.fs(&[vv.x(), vv.y(), vv.coord(1)?, vv.coord(2)?]);
            g.xy(vv.xy());
            let (cx, cy) = vv.coords();
            g.fs(&[cx, cy]);
            let mut t = vv;
            t.set_coord_at(i1, ww.x())?;
            g.vec(&t);
            t.set_coord(ww.y(), s);
            g.vec(&t);
            t.set_x(vv.x());
            g.vec(&t);
            t.set_y(vv.y());
            g.vec(&t);
            t.set_xy(*ww.xy());
            g.vec(&t);
        }
        "vec2d_bad_coord" => {
            let (v, idx) = (r.vec(), r.i());
            g.f(v.coord(idx)?);
        }
        "vec2d_bad_set_index" => {
            let (v, idx, val) = (r.vec(), r.i(), r.f());
            let mut t = v;
            t.set_coord_at(idx, val)?;
            g.vec(&t);
        }
        "vec2d_zero_angle" => {
            let (v, which) = (r.vec(), r.i());
            let z = Vec2d::zero();
            g.f(if which == 0 { z.angle(&v)? } else { v.angle(&z)? });
        }
        "vec2d_zero_is_normal" => {
            let (v, tol, which) = (r.vec(), r.f(), r.i());
            let z = Vec2d::zero();
            g.b(if which == 0 { z.is_normal(&v, tol)? } else { v.is_normal(&z, tol)? });
        }
        "vec2d_zero_is_opposite" => {
            let (v, tol, which) = (r.vec(), r.f(), r.i());
            let z = Vec2d::zero();
            g.b(if which == 0 { z.is_opposite(&v, tol)? } else { v.is_opposite(&z, tol)? });
        }
        "vec2d_zero_is_parallel" => {
            let (v, tol, which) = (r.vec(), r.f(), r.i());
            let z = Vec2d::zero();
            g.b(if which == 0 { z.is_parallel(&v, tol)? } else { v.is_parallel(&z, tol)? });
        }
        "vec2d_zero_is_equal" => {
            let (v, lt, at, which) = (r.vec(), r.f(), r.f(), r.i());
            let z = Vec2d::zero();
            g.b(if which == 0 { z.is_equal(&v, lt, at)? } else { v.is_equal(&z, lt, at)? });
        }
        "vec2d_cross" => {
            let (v, w) = (r.vec(), r.vec());
            g.fs(&[v.crossed(&w), v.dot(&w), v.cross_magnitude(&w), v.cross_square_magnitude(&w)]);
        }
        "vec2d_arithmetic" => {
            let (v, w, s) = (r.vec(), r.vec(), r.f());
            g.vec(&v.added(&w));
            g.vec(&v.subtracted(&w));
            g.vec(&v.multiplied(s));
            g.vec(&v.reversed());
            g.vec(&v.get_normal());
        }
        "vec2d_normalize" => {
            let v = r.vec();
            g.vec(&v.normalized()?);
            let mut t = v;
            t.normalize()?;
            g.vec(&t);
        }
        "vec2d_relations" => {
            let (va, vb, lt, at) = (r.vec(), r.vec(), r.f(), r.f());
            g.b(va.is_equal(&vb, lt, at)?);
            g.b(va.is_normal(&vb, at)?);
            g.b(va.is_opposite(&vb, at)?);
            g.b(va.is_parallel(&vb, at)?);
        }
        "vec2d_queries" => {
            let (va, vb) = (r.vec(), r.vec());
            g.f(va.angle(&vb)?);
            g.fs(&[
                va.magnitude(),
                va.square_magnitude(),
                va.cross_magnitude(&vb),
                va.cross_square_magnitude(&vb),
                va.dot(&vb),
                va.crossed(&vb),
            ]);
        }
        "vec2d_mutators" => {
            let (va, vb, vr, s, r1) = (r.vec(), r.vec(), r.vec(), r.f(), r.f());
            let sd = if s == 0.0 { 1.0 } else { s };
            g.vec(&va.added(&vb));
            g.vec(&va.subtracted(&vb));
            g.vec(&va.multiplied(s));
            g.vec(&va.divided(sd));
            g.vec(&va.reversed());
            let mut vm = va;
            vm.add(&vb);
            g.vec(&vm);
            vm = va;
            vm.subtract(&vb);
            g.vec(&vm);
            vm = va;
            vm.multiply(s);
            g.vec(&vm);
            vm = va;
            vm.divide(sd);
            g.vec(&vm);
            vm = va;
            vm.reverse();
            g.vec(&vm);
            vm.set_linear_form_2(s, &va, r1, &vb);
            g.vec(&vm);
            vm.set_linear_form_2_plus(s, &va, r1, &vb, &vr);
            g.vec(&vm);
            vm.set_linear_form_1_plus(s, &va, &vr);
            g.vec(&vm);
            vm.set_linear_form_sum(&va, &vb);
            g.vec(&vm);
        }
        "vec2d_mirrors" => {
            let (va, vmz, ax) = (r.vec(), r.vec(), r.ax2d()?);
            g.vec(&va.mirrored(&vmz));
            let mut vm = va;
            vm.mirror(&vmz);
            g.vec(&vm);
            g.vec(&va.mirrored(&ax));
            vm = va;
            vm.mirror(&ax);
            g.vec(&vm);
        }
        "vec2d_scale_transform" => {
            let (va, s, tr) = (r.vec(), r.f(), r.trsf2d()?);
            g.vec(&va.scaled(s));
            let mut vm = va;
            vm.scale(s);
            g.vec(&vm);
            g.vec(&va.transformed(&tr));
            vm = va;
            vm.transform(&tr);
            g.vec(&vm);
        }
        "vec2d_rotate" => {
            let (va, s, tr) = (r.vec(), r.f(), r.trsf2d()?);
            g.vec(&va.rotated(s));
            let mut vm = va;
            vm.rotate(s);
            g.vec(&vm);
            g.vec(&va.transformed(&tr));
            vm = va;
            vm.transform(&tr);
            g.vec(&vm);
        }

        // ---- gp_Dir2d ----
        "dir2d_construct" => {
            let a = r.xy();
            g.dir(&Dir2d::new(a.x(), a.y())?);
            g.dir(&Dir2d::from_vec(&Vec2d::from_xy(a))?);
            g.dir(&Dir2d::from_xy(a)?);
        }
        "dir2d_angle" => {
            let (a, b) = (r.xy(), r.xy());
            let d = Dir2d::new(a.x(), a.y())?;
            let e = Dir2d::new(b.x(), b.y())?;
            g.fs(&[d.angle(&e), d.dot(&e), d.crossed(&e)]);
        }
        "dir2d_setters" => {
            let (dd, i1, r1, r2, r3, e1, e2) = (r.dir()?, r.i(), r.f(), r.f(), r.f(), r.xy(), r.xy());
            g.fs(&[dd.x(), dd.y(), dd.coord(1)?, dd.coord(2)?]);
            g.xy(dd.xy());
            let (cx, cy) = dd.coords();
            g.fs(&[cx, cy]);
            let mut ds = dd;
            ds.set_coord_at(i1, r1)?;
            g.dir(&ds);
            ds.set_x(r2)?;
            g.dir(&ds);
            ds.set_y(r3)?;
            g.dir(&ds);
            ds.set_coord(r1, r3)?;
            g.dir(&ds);
            ds.set_xy(e1)?;
            g.dir(&ds);
            g.dir(&Dir2d::default());
            g.dir(&Dir2d::from_vec(&Vec2d::from_xy(e2))?);
            g.dir(&Dir2d::from_xy(e2)?);
        }
        "dir2d_bad_coord" => {
            let (d, idx) = (r.dir()?, r.i());
            g.f(d.coord(idx)?);
        }
        "dir2d_bad_set_index" => {
            let (d, idx, val) = (r.dir()?, r.i(), r.f());
            let mut t = d;
            t.set_coord_at(idx, val)?;
            g.dir(&t);
        }
        "axis2d_enum_constructors" => {
            let p = r.pnt();
            for d in [Direction2d::X, Direction2d::Y, Direction2d::NegativeX, Direction2d::NegativeY] {
                g.dir(&Dir2d::from_direction(d));
                g.ax2d(&Ax2d::from_standard_direction(d));
                g.ax2d(&Ax2d::from_point_standard(p, d));
                g.ax22d(&Ax22d::from_ax2d(Ax2d::from_point_standard(p, d), false));
            }
            g.ax2d(&Ax2d::default());
            g.ax22d(&Ax22d::default());
        }
        "dir2d_queries" => {
            let (dd, de, tol) = (r.dir()?, r.dir()?, r.f());
            g.b(dd.is_equal(&de, tol));
            g.b(dd.is_normal(&de, tol));
            g.b(dd.is_opposite(&de, tol));
            g.b(dd.is_parallel(&de, tol));
        }
        "dir2d_ops" => {
            let (dd, de) = (r.dir()?, r.dir()?);
            g.fs(&[dd.crossed(&de), dd.dot(&de)]);
            g.dir(&dd.reversed());
            let mut dm = dd;
            dm.reverse();
            g.dir(&dm);
        }
        "dir2d_mirrors" => {
            let (dd, de, ax) = (r.dir()?, r.dir()?, r.ax2d()?);
            g.dir(&dd.mirrored(&de));
            let mut dm = dd;
            dm.mirror(&de);
            g.dir(&dm);
            g.dir(&dd.mirrored(&ax));
            dm = dd;
            dm.mirror(&ax);
            g.dir(&dm);
        }
        "dir2d_transform" => {
            let (dd, tr) = (r.dir()?, r.trsf2d()?);
            g.dir(&dd.transformed(&tr));
            let mut dm = dd;
            dm.transform(&tr);
            g.dir(&dm);
        }
        "dir2d_rotate" => {
            let (dd, s, tr) = (r.dir()?, r.f(), r.trsf2d()?);
            g.dir(&dd.rotated(s));
            let mut dm = dd;
            dm.rotate(s);
            g.dir(&dm);
            g.dir(&dd.transformed(&tr));
            dm = dd;
            dm.transform(&tr);
            g.dir(&dm);
        }

        // ---- gp_Ax2d ----
        "ax2d_access" => {
            let (aa, nl, nd) = (r.ax2d()?, r.pnt(), r.dir()?);
            g.ax2d(&aa);
            g.ax2d(&Ax2d::default());
            g.ax2d(&Ax2d::new(nl, nd));
            let mut t = aa;
            t.set_location(nl);
            g.ax2d(&t);
            t.set_direction(nd);
            g.ax2d(&t);
        }
        "ax2d_relations" => {
            let (aa, ab, atol, lt) = (r.ax2d()?, r.ax2d()?, r.f(), r.f());
            g.b(aa.is_coaxial(&ab, atol, lt));
            g.b(aa.is_normal(&ab, atol));
            g.b(aa.is_opposite(&ab, atol));
            g.b(aa.is_parallel(&ab, atol));
            g.f(aa.angle(&ab));
            g.b(aa.is_coaxial(&aa, 1e-9, 1e-9));
            g.b(aa.is_opposite(&Ax2d::new(*aa.location(), aa.direction().reversed()), 0.1));
            g.b(aa.is_parallel(&aa, 0.1));
        }
        "ax2d_reverse" => {
            let aa = r.ax2d()?;
            g.ax2d(&aa.reversed());
            let mut t = aa;
            t.reverse();
            g.ax2d(&t);
        }
        "ax2d_mirror_point" => {
            let (aa, q) = (r.ax2d()?, r.pnt());
            g.ax2d(&aa.mirrored(&q));
            let mut t = aa;
            t.mirror(&q);
            g.ax2d(&t);
        }
        "ax2d_mirror_axis" => {
            let (aa, o) = (r.ax2d()?, r.ax2d()?);
            g.ax2d(&aa.mirrored(&o));
            let mut t = aa;
            t.mirror(&o);
            g.ax2d(&t);
        }
        "ax2d_scale" => {
            let (aa, q, s) = (r.ax2d()?, r.pnt(), r.f());
            g.ax2d(&aa.scaled(&q, s));
            let mut t = aa;
            t.scale(&q, s);
            g.ax2d(&t);
        }
        "ax2d_transform" => {
            let (aa, tr) = (r.ax2d()?, r.trsf2d()?);
            g.ax2d(&aa.transformed(&tr));
            let mut t = aa;
            t.transform(&tr);
            g.ax2d(&t);
        }
        "ax2d_rotate" => {
            let (aa, q, s, tr) = (r.ax2d()?, r.pnt(), r.f(), r.trsf2d()?);
            g.ax2d(&aa.rotated(&q, s));
            let mut t = aa;
            t.rotate(&q, s);
            g.ax2d(&t);
            g.ax2d(&aa.transformed(&tr));
            t = aa;
            t.transform(&tr);
            g.ax2d(&t);
        }
        "ax2d_translate" => {
            let (aa, c, p1, p2) = (r.ax2d()?, r.xy(), r.pnt(), r.pnt());
            let v = Vec2d::from_xy(c);
            g.ax2d(&aa.translated(&v));
            let mut t = aa;
            t.translate(&v);
            g.ax2d(&t);
            g.ax2d(&aa.translated_points(&p1, &p2));
            t = aa;
            t.translate_points(&p1, &p2);
            g.ax2d(&t);
        }

        // ---- gp_Ax22d ----
        "ax22d_construct" => {
            let ((p, dx, dy), a1, sense) = (r.ax22d_parts()?, r.ax2d()?, r.i());
            g.ax22d(&Ax22d::default());
            g.ax22d(&Ax22d::from_dirs(p, dx, dy));
            g.ax22d(&Ax22d::from_dirs(p, dx, dy));
            g.ax22d(&Ax22d::from_dir(p, dx, sense == 1));
            g.ax22d(&Ax22d::from_dir(p, dy, sense == 0));
            g.ax22d(&Ax22d::from_ax2d(a1, sense == 1));
            g.ax22d(&Ax22d::from_ax2d(a1, true));
        }
        "ax22d_access" => {
            let (fr, g2, a1, a2, nl, d1, d2) =
                (r.ax22d()?, r.ax22d()?, r.ax2d()?, r.ax2d()?, r.pnt(), r.dir()?, r.dir()?);
            g.ax22d(&fr);
            g.ax2d(&fr.x_axis());
            g.ax2d(&fr.y_axis());
            g.pnt(fr.location());
            g.dir(fr.x_direction());
            g.dir(fr.y_direction());
            let mut t = fr;
            t.set_axis(g2);
            g.ax22d(&t);
            t = fr;
            t.set_x_axis(a1);
            g.ax22d(&t);
            t = fr;
            t.set_y_axis(a2);
            g.ax22d(&t);
            t = fr;
            t.set_location(nl);
            g.ax22d(&t);
            t = fr;
            t.set_x_direction(d1);
            g.ax22d(&t);
            t = fr;
            t.set_y_direction(d2);
            g.ax22d(&t);
        }
        "ax22d_mirror" => {
            let (fr, cen, mx) = (r.ax22d()?, r.pnt(), r.ax2d()?);
            g.ax22d(&fr.mirrored(&cen));
            let mut t = fr;
            t.mirror(&cen);
            g.ax22d(&t);
            g.ax22d(&fr.mirrored(&mx));
            t = fr;
            t.mirror(&mx);
            g.ax22d(&t);
        }
        "ax22d_scale_transform_translate" => {
            let fr = r.ax22d()?;
            let (sp, ss, tr, dv, p1, p2) = (r.pnt(), r.f(), r.trsf2d()?, r.xy(), r.pnt(), r.pnt());
            g.ax22d(&fr.scaled(&sp, ss));
            let mut t = fr;
            t.scale(&sp, ss);
            g.ax22d(&t);
            g.ax22d(&fr.transformed(&tr));
            t = fr;
            t.transform(&tr);
            g.ax22d(&t);
            g.ax22d(&fr.translated(&Vec2d::from_xy(dv)));
            t = fr;
            t.translate(&Vec2d::from_xy(dv));
            g.ax22d(&t);
            g.ax22d(&fr.translated_points(&p1, &p2));
            t = fr;
            t.translate_points(&p1, &p2);
            g.ax22d(&t);
        }
        "ax22d_rotate" => {
            let (fr, rc, ang, tr) = (r.ax22d()?, r.pnt(), r.f(), r.trsf2d()?);
            g.ax22d(&fr.rotated(&rc, ang));
            let mut t = fr;
            t.rotate(&rc, ang);
            g.ax22d(&t);
            g.ax22d(&fr.transformed(&tr));
            t = fr;
            t.transform(&tr);
            g.ax22d(&t);
        }

        // ---- gp_Mat2d ----
        "mat2d_constructors" => {
            let (m, e1, e2) = (r.mat(), r.xy(), r.xy());
            g.mat(&Mat2d::new());
            g.mat(&m);
            g.mat(&Mat2d::from_columns(e1, e2));
        }
        "mat2d_access" => {
            let (m, ci, ri, vr, vc, cr, cc, nv) = (r.mat(), r.i(), r.i(), r.i(), r.i(), r.i(), r.i(), r.f());
            g.xy(&m.column(ci)?);
            g.xy(&m.row(ri)?);
            g.xy(&m.diagonal());
            g.f(m.value(vr, vc)?);
            g.f(m.determinant());
            let mut t = m;
            *t.change_value(cr, cc)? = nv;
            g.mat(&t);
        }
        "mat2d_setters" => {
            let (m, e1, e2) = (r.mat(), r.xy(), r.xy());
            let (ci, ri, vr, vc) = (r.i(), r.i(), r.i(), r.i());
            let (d1, d2, sc, nv) = (r.f(), r.f(), r.f(), r.f());
            let mut t = m;
            t.set_col(ci, &e1)?;
            g.mat(&t);
            t = m;
            t.set_cols(&e1, &e2);
            g.mat(&t);
            t = m;
            t.set_row(ri, &e1)?;
            g.mat(&t);
            t = m;
            t.set_rows(&e1, &e2);
            g.mat(&t);
            t = m;
            t.set_diagonal(d1, d2);
            g.mat(&t);
            t = m;
            t.set_identity();
            g.mat(&t);
            t = m;
            t.set_scale(sc);
            g.mat(&t);
            t = m;
            t.set_value(vr, vc, nv)?;
            g.mat(&t);
        }
        "mat2d_rotation" => {
            let (m, s) = (r.mat(), r.f());
            let mut t = m;
            t.set_rotation(s);
            g.mat(&t);
        }
        "mat2d_singular" => {
            let m = r.mat();
            g.b(m.is_singular());
            g.f(m.determinant());
        }
        "mat2d_bad_index" => {
            let (m, row, col) = (r.mat(), r.i(), r.i());
            g.f(m.value(row, col)?);
        }
        "mat2d_bad_row" => {
            let (m, idx) = (r.mat(), r.i());
            g.xy(&m.row(idx)?);
        }
        "mat2d_bad_column" => {
            let (m, idx) = (r.mat(), r.i());
            g.xy(&m.column(idx)?);
        }
        "mat2d_bad_set_row" => {
            let (m, idx, e) = (r.mat(), r.i(), r.xy());
            let mut t = m;
            t.set_row(idx, &e)?;
            g.mat(&t);
        }
        "mat2d_bad_set_column" => {
            let (m, idx, e) = (r.mat(), r.i(), r.xy());
            let mut t = m;
            t.set_col(idx, &e)?;
            g.mat(&t);
        }
        "mat2d_bad_change_value" => {
            let (m, row, col) = (r.mat(), r.i(), r.i());
            let mut t = m;
            g.f(*t.change_value(row, col)?);
        }
        "mat2d_bad_set_value" => {
            let (m, row, col, val) = (r.mat(), r.i(), r.i(), r.f());
            let mut t = m;
            t.set_value(row, col, val)?;
            g.mat(&t);
        }
        "mat2d_arithmetic" => {
            let (m1, m2, sc) = (r.mat(), r.mat(), r.f());
            g.mat(&m1.added(&m2));
            g.mat(&m1.subtracted(&m2));
            g.mat(&m1.multiplied_mat(&m2));
            g.mat(&m1.multiplied_scalar(sc));
            g.mat(&m1.divided(sc));
            let mut t = m1;
            t.add(&m2);
            g.mat(&t);
            t = m1;
            t.subtract(&m2);
            g.mat(&t);
            t = m1;
            t.multiply_mat(&m2);
            g.mat(&t);
            t = m1;
            t.pre_multiply(&m2);
            g.mat(&t);
            t = m1;
            t.multiply_scalar(sc);
            g.mat(&t);
            t = m1;
            t.divide(sc);
            g.mat(&t);
        }
        "mat2d_divide_zero" => {
            let (m, dv) = (r.mat(), r.f());
            // OCCT does not check the divisor; the zero divisor is an error by contract.
            if dv.abs() <= gp::resolution() {
                return Err(Thrown);
            }
            g.mat(&m.divided(dv));
            let mut t = m;
            t.divide(dv);
            g.mat(&t);
        }
        "mat2d_inverse" => {
            let m = r.mat();
            g.mat(&m.inverted()?);
            let mut t = m;
            t.invert()?;
            g.mat(&t);
        }
        "mat2d_power" => {
            let (m, pe) = (r.mat(), r.i());
            g.mat(&m.powered(pe)?);
            let mut t = m;
            t.power(pe)?;
            g.mat(&t);
            g.mat(&m.powered(0)?);
        }
        "mat2d_transpose" => {
            let m = r.mat();
            g.mat(&m.transposed());
            let mut t = m;
            t.transpose();
            g.mat(&t);
        }

        // ---- gp_Trsf2d ----
        "trsf2d_rotation" => {
            let (p, s) = (r.pnt(), r.f());
            g.trsf(&gp::Trsf2d::new());
            let mut t = gp::Trsf2d::new();
            t.set_rotation(&p, s);
            g.trsf(&t);
        }
        "trsf2d_mirror_point" => {
            let p = r.pnt();
            let mut t = gp::Trsf2d::new();
            t.set_mirror(&p);
            g.trsf(&t);
        }
        "trsf2d_mirror_axis" => {
            let ax = r.ax2d()?;
            let mut t = gp::Trsf2d::new();
            t.set_mirror(&ax);
            g.trsf(&t);
        }
        "trsf2d_scale" => {
            let (p, sv) = (r.pnt(), r.f());
            let mut t = gp::Trsf2d::new();
            t.set_scale(&p, sv);
            g.trsf(&t);
        }
        "trsf2d_translation" => {
            let (c, p1, p2) = (r.xy(), r.pnt(), r.pnt());
            let mut t = gp::Trsf2d::new();
            t.set_translation(&Vec2d::from_xy(c));
            g.trsf(&t);
            let mut t = gp::Trsf2d::new();
            t.set_translation_points(&p1, &p2);
            g.trsf(&t);
        }
        "trsf2d_transformation" => {
            let (from, to) = (r.ax2d()?, r.ax2d()?);
            let mut t = gp::Trsf2d::new();
            t.set_transformation(&to);
            g.trsf(&t);
            let mut t = gp::Trsf2d::new();
            t.set_transformation_between(&from, &to);
            g.trsf(&t);
        }
        "trsf2d_translation_part" => {
            let (tr, tp) = (r.trsf2d()?, r.xy());
            let mut t = tr;
            t.set_translation_part(&Vec2d::from_xy(tp));
            g.trsf(&t);
        }
        "trsf2d_scale_factor" => {
            let (tr, sf) = (r.trsf2d()?, r.f());
            let mut t = tr;
            t.set_scale_factor(sf);
            g.trsf(&t);
        }
        "trsf2d_set_values" => {
            let v = [r.f(), r.f(), r.f(), r.f(), r.f(), r.f()];
            let mut t = gp::Trsf2d::new();
            t.set_values(v)?;
            g.trsf(&t);
        }
        "trsf2d_queries" => {
            let tr = r.trsf2d()?;
            g.b(tr.is_negative());
            g.i(tr.form() as i32);
            g.f(tr.scale_factor());
            g.xy(tr.translation_part());
            g.mat(&tr.vectorial_part());
            g.mat(tr.h_vectorial_part());
        }
        "trsf2d_rotation_part" => {
            let tr = r.trsf2d()?;
            g.f(tr.rotation_part());
        }
        "trsf2d_values" => {
            let tr = r.trsf2d()?;
            for row in 1..=2 {
                for col in 1..=3 {
                    g.f(tr.value(row, col)?);
                }
            }
        }
        "trsf2d_bad_value" => {
            let (tr, row, col) = (r.trsf2d()?, r.i(), r.i());
            g.f(tr.value(row, col)?);
        }
        "trsf2d_inverse" => {
            let tr = r.trsf2d()?;
            g.trsf(&tr.inverted()?);
            let mut t = tr;
            t.invert()?;
            g.trsf(&t);
        }
        "trsf2d_transforms" | "trsf2d_transforms_rotation" => {
            let (tr, pt) = (r.trsf2d()?, r.xy());
            let mut t1 = pt;
            tr.transforms_xy(&mut t1);
            g.xy(&t1);
            let (tx, ty) = tr.transforms_coords(pt.x(), pt.y());
            g.fs(&[tx, ty]);
        }
        "trsf2d_from_trsf" => {
            let t3 = r.trsf3d()?;
            g.trsf(&gp::Trsf2d::from_trsf(&t3));
        }
        "trsf2d_composition" | "trsf2d_composition_rotation" => {
            let (ta, tb) = (r.trsf2d()?, r.trsf2d()?);
            g.trsf(&ta.multiplied(&tb));
            let mut t = ta;
            t.multiply(&tb);
            g.trsf(&t);
            t = ta;
            t.pre_multiply(&tb);
            g.trsf(&t);
        }
        "trsf2d_power" => {
            let (tr, pe) = (r.trsf2d()?, r.i());
            g.trsf(&tr.powered(pe)?);
            let mut t = tr;
            t.power(pe)?;
            g.trsf(&t);
        }
        "trsf2d_rotation_ops" => {
            let (tr, pe) = (r.trsf2d()?, r.i());
            g.trsf(&tr.inverted()?);
            let mut t = tr;
            t.invert()?;
            g.trsf(&t);
            g.trsf(&tr.powered(pe)?);
            t = tr;
            t.power(pe)?;
            g.trsf(&t);
        }

        // ---- gp_GTrsf2d ----
        "gtrsf2d_constructors" => {
            let (tr, m, e) = (r.trsf2d()?, r.mat(), r.xy());
            g.gtrsf(&GTrsf2d::new());
            g.gtrsf(&GTrsf2d::from_trsf2d(&tr));
            g.gtrsf(&GTrsf2d::from_parts(m, e));
        }
        "gtrsf2d_setters" => {
            let (g0, ax, ratio, ri, ci, val) = (r.gtrsf2d()?, r.ax2d()?, r.f(), r.i(), r.i(), r.f());
            let (m, c1, tr) = (r.mat(), r.xy(), r.trsf2d()?);
            let mut t = g0;
            t.set_affinity(&ax, ratio);
            g.gtrsf(&t);
            t = g0;
            t.set_value(ri, ci, val)?;
            g.gtrsf(&t);
            t = g0;
            t.set_vectorial_part(&m);
            g.gtrsf(&t);
            t = g0;
            t.set_translation_part(&c1);
            g.gtrsf(&t);
            t = g0;
            t.set_trsf2d(&tr);
            g.gtrsf(&t);
            t = g0;
            t.set_translation_part(&XY::zero());
            g.gtrsf(&t);
        }
        "gtrsf2d_queries" => {
            let gt = r.gtrsf2d()?;
            g.gtrsf(&gt);
            g.b(gt.is_negative());
            g.b(gt.is_singular());
            g.xy(gt.translation_part());
            g.mat(gt.vectorial_part());
            g.i(gt.form() as i32);
        }
        "gtrsf2d_value_transform" => {
            let (gt, r1, c1, r2, c2, pt) = (r.gtrsf2d()?, r.i(), r.i(), r.i(), r.i(), r.xy());
            g.fs(&[gt.value(r1, c1)?, gt.value(r2, c2)?]);
            let mut t1 = pt;
            gt.transforms_xy(&mut t1);
            g.xy(&t1);
            g.xy(&gt.transformed(&pt));
            let (tx, ty) = gt.transforms_coords(pt.x(), pt.y());
            g.fs(&[tx, ty]);
        }
        "gtrsf2d_bad_value" => {
            let (gt, row, col) = (r.gtrsf2d()?, r.i(), r.i());
            g.f(gt.value(row, col)?);
        }
        "gtrsf2d_bad_set_value" => {
            let (gt, row, col, val) = (r.gtrsf2d()?, r.i(), r.i(), r.f());
            let mut t = gt;
            t.set_value(row, col, val)?;
            g.gtrsf(&t);
        }
        "gtrsf2d_invert" => {
            let gt = r.gtrsf2d()?;
            g.gtrsf(&gt.inverted()?);
            let mut t = gt;
            t.invert()?;
            g.gtrsf(&t);
        }
        "gtrsf2d_to_trsf2d" => {
            let gt = r.gtrsf2d()?;
            g.trsf(&gt.trsf2d()?);
        }
        "gtrsf2d_composition" | "gtrsf2d_composition_all" => {
            let (ga, gb) = (r.gtrsf2d()?, r.gtrsf2d()?);
            g.gtrsf(&ga.multiplied(&gb)?);
            let mut m = ga;
            m.multiply(&gb)?;
            g.gtrsf(&m);
            m = ga;
            m.pre_multiply(&gb)?;
            g.gtrsf(&m);
        }
        "gtrsf2d_power" => {
            let (gt, pe) = (r.gtrsf2d()?, r.i());
            g.gtrsf(&gt.powered(pe)?);
            let mut t = gt;
            t.power(pe)?;
            g.gtrsf(&t);
        }
        "gtrsf2d_rotation" => {
            let (ga, h2) = (r.gtrsf2d()?, r.gtrsf2d()?);
            g.gtrsf(&ga.multiplied(&h2)?);
            let mut m = ga;
            m.multiply(&h2)?;
            g.gtrsf(&m);
            m = ga;
            m.pre_multiply(&h2)?;
            g.gtrsf(&m);
            let mut i = ga;
            i.invert()?;
            g.gtrsf(&i);
        }

        // ---- gp_Lin2d ----
        "lin2d_constructors" => {
            let (a1, nl, nd) = (r.ax2d()?, r.pnt(), r.dir()?);
            g.lin(&Lin2d::default());
            g.lin(&Lin2d::from_ax2d(a1));
            g.lin(&Lin2d::new(nl, nd));
        }
        "lin2d_from_coefficients" => {
            let (ca, cb, cc) = (r.f(), r.f(), r.f());
            g.lin(&Lin2d::from_coefficients(ca, cb, cc)?);
        }
        "lin2d_access" => {
            let (l, nl, a2, nd) = (r.lin()?, r.pnt(), r.ax2d()?, r.dir()?);
            g.lin(&l);
            g.pnt(l.location());
            g.dir(l.direction());
            g.ax2d(l.position());
            let (ca, cb, cc) = l.coefficients();
            g.fs(&[ca, cb, cc]);
            g.lin(&l.reversed());
            let mut t = l;
            t.reverse();
            g.lin(&t);
            t = l;
            t.set_location(nl);
            g.lin(&t);
            t = l;
            t.set_direction(nd);
            g.lin(&t);
            t = l;
            t.set_position(a2);
            g.lin(&t);
        }
        "lin2d_point_queries" => {
            let (l, pt, onl, lt) = (r.lin()?, r.pnt(), r.pnt(), r.f());
            g.b(l.contains(&pt, lt));
            g.b(l.contains(&onl, lt));
            g.f(l.distance(&pt));
            g.f(l.square_distance(&pt));
            g.f(l.distance(&onl));
            g.lin(&l.normal(&pt));
        }
        "lin2d_line_queries" => {
            let (l1, l2) = (r.lin()?, r.lin()?);
            g.f(l1.angle(&l2));
            g.f(l1.distance(&l2));
            g.f(l1.square_distance(&l2));
        }
        "lin2d_motion" => {
            let l = r.lin()?;
            motion_exact!(g, r, l, lin);
        }
        "lin2d_rotate" => {
            let l = r.lin()?;
            motion_rotate!(g, r, l, lin);
        }

        // ---- gp_Circ2d ----
        "circ2d_constructors" => {
            let (k, a1, sense, f2) = (r.circ()?, r.ax2d()?, r.i(), r.ax22d()?);
            let rad = k.radius();
            g.circ(&Circ2d::default());
            g.circ(&k);
            g.circ(&Circ2d::new(a1, rad, sense == 1)?);
            g.circ(&Circ2d::new(a1, rad, true)?);
            g.circ(&Circ2d::from_ax22d(f2, rad)?);
        }
        "circ2d_radius_checks" => {
            let (a1, sense, f2, rneg, w) = (r.ax2d()?, r.i(), r.ax22d()?, r.f(), r.i());
            if w == 0 {
                g.circ(&Circ2d::new(a1, rneg, sense == 1)?);
            } else if w == 1 {
                g.circ(&Circ2d::from_ax22d(f2, rneg)?);
            } else {
                let mut t = Circ2d::new(a1, 1.0, true)?;
                t.set_radius(rneg)?;
                g.circ(&t);
            }
        }
        "circ2d_access" => {
            let (c0, nl, a1, a2, f2, nr) = (r.circ()?, r.pnt(), r.ax2d()?, r.ax2d()?, r.ax22d()?, r.f());
            g.pnt(c0.location());
            g.f(c0.radius());
            g.ax22d(c0.axis());
            g.ax22d(c0.position());
            g.ax2d(&c0.x_axis());
            g.ax2d(&c0.y_axis());
            g.fs(&[c0.area(), c0.length()]);
            g.b(c0.is_direct());
            let (ca, cb, cc, cd, ce, cf) = c0.coefficients();
            g.fs(&[ca, cb, cc, cd, ce, cf]);
            g.circ(&c0.reversed());
            let mut t = c0;
            t.reverse();
            g.circ(&t);
            t = c0;
            t.set_location(nl);
            g.circ(&t);
            t = c0;
            t.set_x_axis(a1);
            g.circ(&t);
            t = c0;
            t.set_y_axis(a2);
            g.circ(&t);
            t = c0;
            t.set_axis(f2);
            g.circ(&t);
            t = c0;
            t.set_radius(nr)?;
            g.circ(&t);
        }
        "circ2d_queries" => {
            let (c, pt, lt) = (r.circ()?, r.pnt(), r.f());
            g.b(c.contains(&pt, lt));
            g.f(c.distance(&pt));
            g.f(c.square_distance(&pt));
            let xd = c.position().x_direction();
            let on = Pnt2d::new(c.location().x() + c.radius() * xd.x(), c.location().y() + c.radius() * xd.y());
            g.b(c.contains(&on, 1e-6));
        }
        "circ2d_motion" => {
            let k = r.circ()?;
            motion_exact!(g, r, k, circ);
        }
        "circ2d_rotate" => {
            let k = r.circ()?;
            motion_rotate!(g, r, k, circ);
        }

        // ---- gp_Elips2d ----
        "elips2d_constructors" => {
            let (k, a1, sense, f2) = (r.elips()?, r.ax2d()?, r.i(), r.ax22d()?);
            let (maj, min) = (k.major_radius(), k.minor_radius());
            g.elips(&Elips2d::default());
            g.elips(&k);
            g.elips(&Elips2d::new(a1, maj, min, sense == 1)?);
            g.elips(&Elips2d::new(a1, maj, min, true)?);
            g.elips(&Elips2d::from_ax22d(f2, maj, min)?);
        }
        "elips2d_radius_checks" => {
            let (a1, sense, f2, mj, mn, w) = (r.ax2d()?, r.i(), r.ax22d()?, r.f(), r.f(), r.i());
            if w == 0 {
                g.elips(&Elips2d::new(a1, mj, mn, sense == 1)?);
            } else if w == 1 {
                g.elips(&Elips2d::from_ax22d(f2, mj, mn)?);
            } else if w == 2 {
                let mut t = Elips2d::new(a1, 3.0, 2.0, true)?;
                t.set_major_radius(mj)?;
                g.elips(&t);
            } else {
                let mut t = Elips2d::new(a1, 3.0, 2.0, true)?;
                t.set_minor_radius(mn)?;
                g.elips(&t);
            }
        }
        "elips2d_access" => {
            let (e0, nl, a1, a2, f2, nmaj, nmin) =
                (r.elips()?, r.pnt(), r.ax2d()?, r.ax2d()?, r.ax22d()?, r.f(), r.f());
            g.pnt(e0.location());
            g.fs(&[e0.major_radius(), e0.minor_radius()]);
            g.ax22d(e0.axis());
            g.ax2d(&e0.x_axis());
            g.ax2d(&e0.y_axis());
            g.fs(&[e0.area(), e0.focal(), e0.parameter(), e0.eccentricity()]);
            g.b(e0.is_direct());
            g.pnt(&e0.focus1());
            g.pnt(&e0.focus2());
            g.elips(&e0.reversed());
            let mut t = e0;
            t.reverse();
            g.elips(&t);
            t = e0;
            t.set_location(nl);
            g.elips(&t);
            t = e0;
            t.set_x_axis(a1);
            g.elips(&t);
            t = e0;
            t.set_y_axis(a2);
            g.elips(&t);
            t = e0;
            t.set_axis(f2);
            g.elips(&t);
            t = e0;
            t.set_major_radius(nmaj)?;
            g.elips(&t);
            t = e0;
            t.set_minor_radius(nmin)?;
            g.elips(&t);
        }
        "elips2d_coefficients" => {
            let k = r.elips()?;
            let (ca, cb, cc, cd, ce, cf) = k.coefficients();
            g.fs(&[ca, cb, cc, cd, ce, cf]);
        }
        "elips2d_directrix" => {
            let k = r.elips()?;
            g.ax2d(&k.directrix1()?);
            g.ax2d(&k.directrix2()?);
        }
        "elips2d_motion" => {
            let k = r.elips()?;
            motion_exact!(g, r, k, elips);
        }
        "elips2d_rotate" => {
            let k = r.elips()?;
            motion_rotate!(g, r, k, elips);
        }

        // ---- gp_Hypr2d ----
        "hypr2d_constructors" => {
            let (k, a1, sense, f2) = (r.hypr()?, r.ax2d()?, r.i(), r.ax22d()?);
            let (maj, min) = (k.major_radius(), k.minor_radius());
            g.hypr(&Hypr2d::default());
            g.hypr(&k);
            g.hypr(&Hypr2d::new(a1, maj, min, sense == 1)?);
            g.hypr(&Hypr2d::new(a1, maj, min, true)?);
            g.hypr(&Hypr2d::from_ax22d(f2, maj, min)?);
        }
        "hypr2d_radius_checks" => {
            let (a1, sense, f2, mj, mn, w) = (r.ax2d()?, r.i(), r.ax22d()?, r.f(), r.f(), r.i());
            if w == 0 {
                g.hypr(&Hypr2d::new(a1, mj, mn, sense == 1)?);
            } else if w == 1 {
                g.hypr(&Hypr2d::from_ax22d(f2, mj, mn)?);
            } else if w == 2 {
                let mut t = Hypr2d::new(a1, 3.0, 2.0, true)?;
                t.set_major_radius(mj)?;
                g.hypr(&t);
            } else if w == 3 {
                let mut t = Hypr2d::new(a1, 3.0, 2.0, true)?;
                t.set_minor_radius(mn)?;
                g.hypr(&t);
            } else {
                let (major, minor) = if mj < 0.0 { (mj, 1.0) } else { (1.0, mn) };
                g.hypr(&Hypr2d::new(a1, major, minor, true)?);
            }
        }
        "hypr2d_access" => {
            let (e0, nl, a1, a2, f2, nmaj, nmin) = (r.hypr()?, r.pnt(), r.ax2d()?, r.ax2d()?, r.ax22d()?, r.f(), r.f());
            g.pnt(e0.location());
            g.fs(&[e0.major_radius(), e0.minor_radius()]);
            g.ax22d(e0.axis());
            g.ax2d(&e0.x_axis());
            g.ax2d(&e0.y_axis());
            g.f(e0.focal());
            g.b(e0.is_direct());
            g.pnt(&e0.focus1());
            g.pnt(&e0.focus2());
            g.hypr(&e0.reversed());
            let mut t = e0;
            t.reverse();
            g.hypr(&t);
            g.hypr(&e0.conjugate_branch1());
            g.hypr(&e0.conjugate_branch2());
            g.hypr(&e0.other_branch());
            t = e0;
            t.set_location(nl);
            g.hypr(&t);
            t = e0;
            t.set_x_axis(a1);
            g.hypr(&t);
            t = e0;
            t.set_y_axis(a2);
            g.hypr(&t);
            t = e0;
            t.set_axis(f2);
            g.hypr(&t);
            t = e0;
            t.set_major_radius(nmaj)?;
            g.hypr(&t);
            t = e0;
            t.set_minor_radius(nmin)?;
            g.hypr(&t);
        }
        "hypr2d_eccentricity" => {
            let k = r.hypr()?;
            g.f(k.eccentricity()?);
            g.f(k.parameter()?);
        }
        "hypr2d_asymptotes" => {
            let k = r.hypr()?;
            g.ax2d(&k.asymptote1()?);
            g.ax2d(&k.asymptote2()?);
        }
        "hypr2d_directrix" => {
            let k = r.hypr()?;
            g.ax2d(&k.directrix1()?);
            g.ax2d(&k.directrix2()?);
        }
        "hypr2d_coefficients" => {
            let k = r.hypr()?;
            let (ca, cb, cc, cd, ce, cf) = k.coefficients();
            g.fs(&[ca, cb, cc, cd, ce, cf]);
        }
        "hypr2d_motion" => {
            let k = r.hypr()?;
            motion_exact!(g, r, k, hypr);
        }
        "hypr2d_rotate" => {
            let k = r.hypr()?;
            motion_rotate!(g, r, k, hypr);
        }

        // ---- gp_Parab2d ----
        "parab2d_constructors" => {
            let (k, a1, sense, f2, dr, fo) = (r.parab()?, r.ax2d()?, r.i(), r.ax22d()?, r.ax2d()?, r.pnt());
            let fp = k.focal();
            g.parab(&Parab2d::default());
            g.parab(&k);
            g.parab(&Parab2d::new(a1, fp, sense == 1)?);
            g.parab(&Parab2d::new(a1, fp, true)?);
            g.parab(&Parab2d::from_ax22d(f2, fp)?);
            g.parab(&Parab2d::from_directrix_focus(&dr, &fo, sense == 1));
            g.parab(&Parab2d::from_directrix_focus(&dr, &fo, true));
        }
        "parab2d_zero_focal" => {
            let (dr, sense) = (r.ax2d()?, r.i());
            g.parab(&Parab2d::from_directrix_focus(&dr, dr.location(), sense == 1));
        }
        "parab2d_focal_checks" => {
            let (a1, sense, f2, neg, w) = (r.ax2d()?, r.i(), r.ax22d()?, r.f(), r.i());
            if w == 0 {
                g.parab(&Parab2d::new(a1, neg, sense == 1)?);
            } else if w == 1 {
                g.parab(&Parab2d::from_ax22d(f2, neg)?);
            } else {
                let mut t = Parab2d::new(a1, 1.0, true)?;
                t.set_focal(neg)?;
                g.parab(&t);
            }
        }
        "parab2d_access" => {
            let (e0, nl, a1, f2, nf) = (r.parab()?, r.pnt(), r.ax2d()?, r.ax22d()?, r.f());
            g.pnt(e0.location());
            g.fs(&[e0.focal(), e0.parameter()]);
            g.ax22d(e0.axis());
            g.ax2d(&e0.mirror_axis());
            g.pnt(&e0.focus());
            g.ax2d(&e0.directrix());
            g.b(e0.is_direct());
            g.parab(&e0.reversed());
            let mut t = e0;
            t.reverse();
            g.parab(&t);
            t = e0;
            t.set_location(nl);
            g.parab(&t);
            t = e0;
            t.set_mirror_axis(a1);
            g.parab(&t);
            t = e0;
            t.set_axis(f2);
            g.parab(&t);
            t = e0;
            t.set_focal(nf)?;
            g.parab(&t);
        }
        "parab2d_coefficients" => {
            let k = r.parab()?;
            let (ca, cb, cc, cd, ce, cf) = k.coefficients();
            g.fs(&[ca, cb, cc, cd, ce, cf]);
        }
        "parab2d_motion" => {
            let k = r.parab()?;
            motion_exact!(g, r, k, parab);
        }
        "parab2d_rotate" => {
            let k = r.parab()?;
            motion_rotate!(g, r, k, parab);
        }

        // ---- package constants (no inputs) ----
        "constants2d" => {
            g.pnt(&gp::origin2d());
            g.dir(&gp::dx2d());
            g.dir(&gp::dy2d());
            g.ax2d(&gp::ox2d());
            g.ax2d(&gp::oy2d());
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
/// otherwise (zero tolerance when that magnitude is infinite); bools and ints always exactly.
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
            Tok::I(g) => assert_eq!(w.parse::<i32>().expect("integer output"), g, "value {index} (int)\n{line}"),
        }
    }
}

// ---------------------------------------------------------------------------------

#[test]
fn gp2d_matches_occt() {
    let text = include_str!("gp2d.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in gp2d.txt:\n{line}");
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
