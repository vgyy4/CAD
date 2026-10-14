//! Numerical reference test for the in-scope gp 3D value algebra (OCCT 8.0.1).
//!
//! `gp3d.txt` is printed by `oracle/gp3d.cpp` (real OCCT). Each line is
//! `<family> <inputs...> | <outputs...>`: f64 values are 16-hex-digit bit patterns, ints
//! are decimal and bools are 0/1. Every input of a call is on the left of ` | `; this test
//! reads them back with the `Rd` reader methods of the same name as the oracle's schema
//! words (`f i xyz pnt vec dir ax1 ax2 ax3 mat quat trsf gtrsf`) and replays the call on
//! the Rust API. The right side is either the expected values, `throws` (OCCT raised) or
//! `unchecked` (OCCT 8.0.1 as built has that check compiled out and returns unspecified
//! numbers; the Rust API must return an error there, as documented in docs/api-gp3d.md).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::gp::{
    self, Ax1, Ax2, Ax3, Dir, Direction, EulerSequence, GTrsf, Mat, Pnt, Quaternion, QuaternionNLerp, QuaternionSLerp,
    Trsf, TrsfForm, Vec as GpVec, XYZ,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for sin/cos/atan2/acos/asin/pow dependent results, in units of
/// f64::EPSILON times the largest absolute value on the line (at least 1).
const MAX_ULPS: f64 = 8.0;

/// Every family must have at least this many distinct lines (distinct inputs).
const MIN_DISTINCT: usize = 10;

/// Families that really take no input: they are printed exactly once.
const INPUT_LESS: &[&str] = &[
    "xyz_constructors",
    "pnt_constructors",
    "quat_zero_normalize",
    "constants",
    "constants_oy",
    "constants_oz",
    "constants_zox",
    "constants_yoz",
    "enum_values",
];

/// Families whose results use only + - * / and sqrt: compared bit for bit. All other
/// families depend on sin/cos/atan2/acos/asin/pow somewhere (or are bool/int results) and
/// are compared with MAX_ULPS.
const EXACT: &[&str] = &[
    "xyz_constructors",
    "xyz_add",
    "xyz_subtract",
    "xyz_cross",
    "xyz_dot",
    "xyz_dot_cross",
    "xyz_modulus",
    "xyz_cross_magnitude",
    "xyz_multiply",
    "xyz_multiply_matrix",
    "xyz_divide",
    "xyz_reverse",
    "xyz_add_in_place",
    "xyz_cross_in_place",
    "xyz_scale_in_place",
    "axis_enum_constructors",
    "xyz_normalize",
    "pnt_distance",
    "pnt_translate",
    "pnt_transform",
    "vec_cross",
    "vec_arithmetic",
    "vec_normalize",
    "quaternion_product",
    "quaternion_nlerp",
    "xyz_access",
    "xyz_setters",
    "xyz_sub_in_place",
    "xyz_cross_cross",
    "xyz_linear_forms",
    "pnt_access",
    "pnt_constructors",
    "pnt_setters",
    "pnt_barycenter",
    "pnt_scale",
    "pnt_translate_vec",
    "pnt_translate_points",
    "pnt_mirror_point",
    "pnt_mirror_axis",
    "pnt_mirror_plane",
    "vec_mutators",
    "vec_mirrors",
    "vec_constructors",
    "vec_access",
    "dir_construct",
    "dir_cross",
    "dir_setters",
    "dir_ops",
    "dir_mirrors",
    "ax1_access",
    "ax1_reverse",
    "ax1_mirror_point",
    "ax1_mirror_axis",
    "ax1_mirror_plane",
    "ax1_translate_vec",
    "ax1_translate_points",
    "ax2_access",
    "ax2_mirror_point",
    "ax2_mirror_axis",
    "ax2_mirror_plane",
    "ax2_translate_vec",
    "ax2_translate_points",
    "ax3_access",
    "ax3_reverse",
    "ax3_mirror_point",
    "ax3_mirror_axis",
    "ax3_mirror_plane",
    "ax3_translate_vec",
    "ax3_translate_points",
    "mat_constructors",
    "mat_access",
    "mat_arithmetic",
    "mat_inverse_power_transpose",
    "gtrsf_constructors",
    "gtrsf_queries",
    "gtrsf_value_transform",
    "gtrsf_composition",
    "gtrsf_to_trsf",
    "gtrsf_mat4",
    "quat_arithmetic",
    "quat_components",
    "quat_nlerp_all",
    "quat_zero_normalize",
];

/// Every family the oracle must print (a family that disappears from gp3d.txt fails).
const FAMILIES: &[&str] = &[
    "ax1_access",
    "ax1_mirror_axis",
    "ax1_mirror_plane",
    "ax1_mirror_point",
    "ax1_motion",
    "ax1_relations",
    "ax1_reverse",
    "ax1_translate_points",
    "ax1_translate_vec",
    "ax2_access",
    "ax2_coplanar",
    "ax2_mirror_axis",
    "ax2_mirror_plane",
    "ax2_mirror_point",
    "ax2_motion",
    "ax2_relations",
    "ax2_translate_points",
    "ax2_translate_vec",
    "ax3_access",
    "ax3_coplanar",
    "ax3_direct",
    "ax3_mirror_axis",
    "ax3_mirror_plane",
    "ax3_mirror_point",
    "ax3_motion",
    "ax3_relations",
    "ax3_reverse",
    "ax3_translate_points",
    "ax3_translate_vec",
    "axis_enum_constructors",
    "constants",
    "constants_oy",
    "constants_oz",
    "constants_yoz",
    "constants_zox",
    "dir_angle",
    "dir_angles",
    "dir_bad_coord",
    "dir_bad_set_index",
    "dir_construct",
    "dir_cross",
    "dir_mirrors",
    "dir_motion",
    "dir_ops",
    "dir_queries",
    "dir_setters",
    "enum_values",
    "gtrsf_bad_value",
    "gtrsf_composition",
    "gtrsf_constructors",
    "gtrsf_flags",
    "gtrsf_form",
    "gtrsf_mat4",
    "gtrsf_point",
    "gtrsf_queries",
    "gtrsf_setters",
    "gtrsf_to_trsf",
    "gtrsf_value_transform",
    "mat_access",
    "mat_arithmetic",
    "mat_bad_change_value",
    "mat_bad_column",
    "mat_bad_index",
    "mat_bad_row",
    "mat_bad_set_column",
    "mat_bad_set_row",
    "mat_bad_set_value",
    "mat_constructors",
    "mat_inverse_power_transpose",
    "mat_setters",
    "mat_singular",
    "mat_zero_rotation",
    "pnt_access",
    "pnt_bad_coord",
    "pnt_bad_set_index",
    "pnt_barycenter",
    "pnt_constructors",
    "pnt_distance",
    "pnt_equal",
    "pnt_mirror_axis",
    "pnt_mirror_plane",
    "pnt_mirror_point",
    "pnt_rotate",
    "pnt_scale",
    "pnt_setters",
    "pnt_transform",
    "pnt_translate",
    "pnt_translate_points",
    "pnt_translate_vec",
    "quat_arithmetic",
    "quat_components",
    "quat_constructors",
    "quat_equal",
    "quat_euler_sequences",
    "quat_nlerp_all",
    "quat_norm_ops",
    "quat_rotation_setters",
    "quat_slerp_all",
    "quat_zero_axis_angle",
    "quat_zero_normalize",
    "quat_zero_set_axis",
    "quaternion_axis_angle",
    "quaternion_nlerp",
    "quaternion_product",
    "quaternion_slerp",
    "trsf_bad_value",
    "trsf_composition",
    "trsf_form",
    "trsf_get_rotation",
    "trsf_inverse",
    "trsf_mat4",
    "trsf_negative",
    "trsf_quaternion",
    "trsf_queries",
    "trsf_rotate",
    "trsf_setters",
    "trsf_transform_coords",
    "trsf_zero_scale",
    "vec_access",
    "vec_arithmetic",
    "vec_bad_coord",
    "vec_bad_set_index",
    "vec_constructors",
    "vec_cross",
    "vec_mirrors",
    "vec_motion",
    "vec_mutators",
    "vec_normalize",
    "vec_queries",
    "vec_relations",
    "vec_zero_angle",
    "vec_zero_angle_ref",
    "vec_zero_is_normal",
    "vec_zero_is_opposite",
    "vec_zero_is_parallel",
    "xyz_access",
    "xyz_add",
    "xyz_add_in_place",
    "xyz_bad_change_index",
    "xyz_bad_index",
    "xyz_bad_set_index",
    "xyz_constructors",
    "xyz_cross",
    "xyz_cross_cross",
    "xyz_cross_in_place",
    "xyz_cross_magnitude",
    "xyz_divide",
    "xyz_dot",
    "xyz_dot_cross",
    "xyz_equal",
    "xyz_linear_forms",
    "xyz_modulus",
    "xyz_multiply",
    "xyz_multiply_matrix",
    "xyz_normalize",
    "xyz_reverse",
    "xyz_scale_in_place",
    "xyz_setters",
    "xyz_sub_in_place",
    "xyz_subtract",
];

const EULER_SEQUENCES: [EulerSequence; 26] = [
    EulerSequence::EulerAngles,
    EulerSequence::YawPitchRoll,
    EulerSequence::ExtrinsicXYZ,
    EulerSequence::ExtrinsicXZY,
    EulerSequence::ExtrinsicYZX,
    EulerSequence::ExtrinsicYXZ,
    EulerSequence::ExtrinsicZXY,
    EulerSequence::ExtrinsicZYX,
    EulerSequence::IntrinsicXYZ,
    EulerSequence::IntrinsicXZY,
    EulerSequence::IntrinsicYZX,
    EulerSequence::IntrinsicYXZ,
    EulerSequence::IntrinsicZXY,
    EulerSequence::IntrinsicZYX,
    EulerSequence::ExtrinsicXYX,
    EulerSequence::ExtrinsicXZX,
    EulerSequence::ExtrinsicYZY,
    EulerSequence::ExtrinsicYXY,
    EulerSequence::ExtrinsicZYZ,
    EulerSequence::ExtrinsicZXZ,
    EulerSequence::IntrinsicXYX,
    EulerSequence::IntrinsicXZX,
    EulerSequence::IntrinsicYZY,
    EulerSequence::IntrinsicYXY,
    EulerSequence::IntrinsicZXZ,
    EulerSequence::IntrinsicZYZ,
];

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
    fn xyz(&mut self, v: &XYZ) {
        self.fs(&[v.x(), v.y(), v.z()]);
    }
    fn pnt(&mut self, v: &Pnt) {
        self.xyz(v.xyz());
    }
    fn vec(&mut self, v: &GpVec) {
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
    fn mat(&mut self, v: &Mat) {
        for r in 1..=3 {
            for c in 1..=3 {
                self.f(v.value(r, c).expect("valid index"));
            }
        }
    }
    fn trsf(&mut self, v: &Trsf) {
        for r in 1..=3 {
            for c in 1..=4 {
                self.f(v.value(r, c).expect("valid index"));
            }
        }
    }
    fn gtrsf(&mut self, v: &GTrsf) {
        for r in 1..=3 {
            for c in 1..=4 {
                self.f(v.value(r, c).expect("valid index"));
            }
        }
    }
    fn quat(&mut self, v: &Quaternion) {
        self.fs(&[v.x(), v.y(), v.z(), v.w()]);
    }
    fn mat4(&mut self, m: [[f64; 4]; 4]) {
        for row in m {
            self.fs(&row);
        }
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
    fn xyz(&mut self) -> XYZ {
        let x = self.f();
        let y = self.f();
        let z = self.f();
        XYZ::new(x, y, z)
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
    fn ax1(&mut self) -> Result<Ax1, Thrown> {
        let p = self.pnt();
        let d = self.dir()?;
        Ok(Ax1::new(p, d))
    }
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
    /// Nine values, row-major.
    fn mat(&mut self) -> Mat {
        let mut v = [0.0; 9];
        for e in &mut v {
            *e = self.f();
        }
        Mat::from_values(v)
    }
    /// Components x y z w, exactly as printed (not normalized).
    fn quat(&mut self) -> Quaternion {
        let x = self.f();
        let y = self.f();
        let z = self.f();
        let w = self.f();
        Quaternion::from_components(x, y, z, w)
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
                    let v = self.xyz();
                    t.set_translation_part(&GpVec::from_xyz(v));
                }
            }
            2 => {
                let p = self.pnt();
                let factor = self.f();
                t.set_scale(&p, factor)?;
            }
            3 => {
                let v = self.xyz();
                t.set_translation(&GpVec::from_xyz(v));
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
                let q = self.quat();
                t.set_rotation_quaternion(&q)?;
                if kind == 8 {
                    let v = self.xyz();
                    let factor = self.f();
                    t.set_translation_part(&GpVec::from_xyz(v));
                    t.set_scale_factor(factor)?;
                }
            }
            _ => panic!("unknown transformation recipe {kind}"),
        }
        Ok(t)
    }
    /// 0: GTrsf from a Trsf recipe, 1: GTrsf from a matrix and a translation.
    fn gtrsf(&mut self) -> Result<GTrsf, Thrown> {
        match self.i() {
            0 => Ok(GTrsf::from_trsf(&self.trsf()?)),
            1 => {
                let m = self.mat();
                let v = self.xyz();
                Ok(GTrsf::from_parts(m, v))
            }
            k => panic!("unknown general transformation recipe {k}"),
        }
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread inputs");
    }
}

// ---------------------------------------------------------------------------------
// Replaying one family

fn run(name: &str, r: &mut Rd, g: &mut Got) -> Result<(), Thrown> {
    match name {
        // ---- gp_XYZ ----
        "xyz_add" => {
            let (a, c) = (r.xyz(), r.xyz());
            g.xyz(&a.added(&c));
        }
        "xyz_subtract" => {
            let (a, c) = (r.xyz(), r.xyz());
            g.xyz(&a.subtracted(&c));
        }
        "xyz_cross" => {
            let (a, c) = (r.xyz(), r.xyz());
            g.xyz(&a.crossed(&c));
        }
        "xyz_dot" => {
            let (a, c) = (r.xyz(), r.xyz());
            g.f(a.dot(&c));
        }
        "xyz_dot_cross" => {
            let (a, c, e) = (r.xyz(), r.xyz(), r.xyz());
            g.f(a.dot_cross(&c, &e));
        }
        "xyz_modulus" => {
            let a = r.xyz();
            g.fs(&[a.modulus(), a.square_modulus()]);
        }
        "xyz_cross_magnitude" => {
            let (a, c) = (r.xyz(), r.xyz());
            g.fs(&[a.cross_magnitude(&c), a.cross_square_magnitude(&c)]);
        }
        "xyz_multiply" => {
            let (a, c, s) = (r.xyz(), r.xyz(), r.f());
            g.xyz(&a.multiplied(s));
            g.xyz(&a.multiplied_xyz(&c));
        }
        "xyz_multiply_matrix" => {
            let (a, m) = (r.xyz(), r.mat());
            g.xyz(&a.multiplied_mat(&m));
            let mut t = a;
            t.multiply_mat(&m);
            g.xyz(&t);
        }
        "xyz_divide" => {
            let (a, s) = (r.xyz(), r.f());
            // OCCT does not check the divisor; the zero divisor is an error by contract.
            if s.abs() <= gp::resolution() {
                return Err(Thrown);
            }
            g.xyz(&a.divided(s));
        }
        "xyz_reverse" => {
            let a = r.xyz();
            g.xyz(&a.reversed());
        }
        "xyz_equal" => {
            let (a, c, tol) = (r.xyz(), r.xyz(), r.f());
            g.b(a.is_equal(&c, tol));
        }
        "xyz_access" => {
            let (a, idx) = (r.xyz(), r.i());
            g.fs(&[a.coord(1)?, a.coord(2)?, a.coord(3)?]);
            g.fs(&a.get_data());
            g.f(a.coord(idx)?);
        }
        "xyz_bad_index" => {
            let (a, idx) = (r.xyz(), r.i());
            g.f(a.coord(idx)?);
        }
        "xyz_bad_set_index" => {
            let (a, idx, val) = (r.xyz(), r.i(), r.f());
            let mut t = a;
            t.set_coord_at(idx, val)?;
            g.xyz(&t);
        }
        "xyz_bad_change_index" => {
            let (a, idx) = (r.xyz(), r.i());
            let mut t = a;
            g.f(*t.change_coord(idx)?);
        }
        "xyz_setters" => {
            let (a, c, s) = (r.xyz(), r.xyz(), r.f());
            let (i1, i2, i3) = (r.i(), r.i(), r.i());
            let mut t = a;
            t.set_coord_at(i1, c.x())?;
            t.set_x(c.y());
            t.set_y(c.z());
            t.set_z(s);
            *t.change_coord(i2)? = a.x();
            t.change_data()[usize::try_from(i3).expect("data index")] = a.y();
            g.xyz(&t);
        }
        "xyz_sub_in_place" => {
            let (a, c) = (r.xyz(), r.xyz());
            let mut t = a;
            t.subtract(&c);
            g.xyz(&t);
        }
        "xyz_cross_cross" => {
            let (a, c, e) = (r.xyz(), r.xyz(), r.xyz());
            g.xyz(&a.cross_crossed(&c, &e));
        }
        "xyz_linear_forms" => {
            let (a, c, s, r2, r3) = (r.xyz(), r.xyz(), r.f(), r.f(), r.f());
            let (e1, e2, e3) = (r.xyz(), r.xyz(), r.xyz());
            let mut t = XYZ::zero();
            t.set_linear_form_2(s, &a, r2, &c);
            g.xyz(&t);
            t.set_linear_form_2_plus(s, &a, r2, &c, &e1);
            g.xyz(&t);
            t.set_linear_form_3(s, &a, r2, &c, r3, &e2);
            g.xyz(&t);
            t.set_linear_form_3_plus(s, &a, r2, &c, r3, &e2, &e3);
            g.xyz(&t);
            t.set_linear_form_1_plus(s, &a, &e3);
            g.xyz(&t);
            t.set_linear_form_sum(&a, &c);
            g.xyz(&t);
        }
        "xyz_add_in_place" => {
            let (a, c) = (r.xyz(), r.xyz());
            let mut t = a;
            t.add(&c);
            g.xyz(&t);
        }
        "xyz_cross_in_place" => {
            let (a, c) = (r.xyz(), r.xyz());
            let mut t = a;
            t.cross(&c);
            g.xyz(&t);
        }
        "xyz_scale_in_place" => {
            let (a, s) = (r.xyz(), r.f());
            let mut t = a;
            t.multiply(s);
            g.xyz(&t);
        }
        "xyz_normalize" => {
            let a = r.xyz();
            let mut t = a;
            t.normalize()?;
            g.xyz(&t);
        }
        "xyz_constructors" => {
            g.xyz(&XYZ::zero());
            g.xyz(&XYZ::default());
            g.xyz(&XYZ::new(1., 2., 3.));
        }

        // ---- gp_Pnt ----
        "pnt_access" => {
            let p = r.pnt();
            g.fs(&[p.coord(1)?, p.coord(2)?, p.coord(3)?]);
            g.xyz(p.xyz());
        }
        "pnt_bad_coord" => {
            let (p, idx) = (r.pnt(), r.i());
            g.f(p.coord(idx)?);
        }
        "pnt_bad_set_index" => {
            let (p, idx, val) = (r.pnt(), r.i(), r.f());
            let mut t = p;
            t.set_coord_at(idx, val)?;
            g.pnt(&t);
        }
        "pnt_setters" => {
            let (p, c, s, i1) = (r.pnt(), r.xyz(), r.f(), r.i());
            let (x, y, z) = (p.xyz().x(), p.xyz().y(), p.xyz().z());
            let mut t = p;
            t.set_coord_at(i1, c.x())?;
            t.set_coord(c.y(), c.z(), s);
            t.set_x(x);
            t.set_y(y);
            t.set_z(z);
            t.set_xyz(c);
            t.change_coord().set_coord(x, c.x(), c.y());
            g.pnt(&t);
        }
        "pnt_barycenter" => {
            let (p, q, alpha, beta) = (r.pnt(), r.pnt(), r.f(), r.f());
            let mut t = p;
            t.bary_center(alpha, &q, beta);
            g.pnt(&t);
        }
        "pnt_equal" => {
            let (p, q, tol) = (r.pnt(), r.pnt(), r.f());
            g.b(p.is_equal(&q, tol));
        }
        "pnt_mirror_point" => {
            let (p, q) = (r.pnt(), r.pnt());
            g.pnt(&p.mirrored(&q));
            let mut t = p;
            t.mirror(&q);
            g.pnt(&t);
        }
        "pnt_mirror_axis" => {
            let (p, axis) = (r.pnt(), r.ax1()?);
            g.pnt(&p.mirrored(&axis));
            let mut t = p;
            t.mirror(&axis);
            g.pnt(&t);
        }
        "pnt_mirror_plane" => {
            let (p, plane) = (r.pnt(), r.ax2()?);
            g.pnt(&p.mirrored(&plane));
            let mut t = p;
            t.mirror(&plane);
            g.pnt(&t);
        }
        "pnt_rotate" => {
            let (p, axis, s) = (r.pnt(), r.ax1()?, r.f());
            g.pnt(&p.rotated(&axis, s));
            let mut t = p;
            t.rotate(&axis, s);
            g.pnt(&t);
        }
        "pnt_scale" => {
            let (p, q, s) = (r.pnt(), r.pnt(), r.f());
            g.pnt(&p.scaled(&q, s));
            let mut t = p;
            t.scale(&q, s);
            g.pnt(&t);
        }
        "pnt_translate_vec" => {
            let (p, v) = (r.pnt(), r.vec());
            g.pnt(&p.translated(&v));
            let mut t = p;
            t.translate(&v);
            g.pnt(&t);
        }
        "pnt_translate_points" => {
            let (p, p1, p2) = (r.pnt(), r.pnt(), r.pnt());
            g.pnt(&p.translated_points(&p1, &p2));
            let mut t = p;
            t.translate_points(&p1, &p2);
            g.pnt(&t);
        }
        "pnt_distance" => {
            let (p, q) = (r.pnt(), r.pnt());
            g.fs(&[p.distance(&q), p.square_distance(&q)]);
        }
        "pnt_translate" => {
            let (p, q) = (r.pnt(), r.pnt());
            g.pnt(&p.translated_points(&p, &q));
        }
        "pnt_transform" => {
            let (p, t) = (r.pnt(), r.trsf()?);
            g.pnt(&p.transformed(&t));
            let mut m = p;
            m.transform(&t);
            g.pnt(&m);
        }
        "pnt_constructors" => {
            g.pnt(&Pnt::default());
            g.pnt(&Pnt::from_xyz(XYZ::new(1., 2., 3.)));
            g.pnt(&Pnt::new(1., 2., 3.));
            g.pnt(&Pnt::origin());
        }

        // ---- gp_Vec ----
        "vec_constructors" => {
            let (a, c, d1, d2) = (r.xyz(), r.xyz(), r.dir()?, r.dir()?);
            g.vec(&GpVec::zero());
            g.vec(&GpVec::from_xyz(a));
            g.vec(&GpVec::from_dir(&Dir::default_y()));
            g.vec(&GpVec::from_dir(&Dir::default_z()));
            g.vec(&GpVec::from_dir(&d1));
            g.vec(&GpVec::between(&Pnt::new(a.x(), a.y(), a.z()), &Pnt::new(c.x(), c.y(), c.z())));
            g.vec(&GpVec::from_dir(&d2));
        }
        "vec_access" => {
            let (vv, ww, s, i1) = (r.vec(), r.vec(), r.f(), r.i());
            let (x, y, z) = (vv.xyz().x(), vv.xyz().y(), vv.xyz().z());
            let (u, v, w) = (ww.xyz().x(), ww.xyz().y(), ww.xyz().z());
            g.fs(&[vv.coord(1)?, vv.coord(2)?, vv.coord(3)?]);
            let mut t = vv;
            t.set_coord_at(i1, u)?;
            t.set_coord(v, w, s);
            t.set_x(x);
            t.set_y(y);
            t.set_z(z);
            t.set_xyz(XYZ::new(u, v, w));
            g.vec(&t);
        }
        "vec_bad_coord" => {
            let (v, idx) = (r.vec(), r.i());
            g.f(v.coord(idx)?);
        }
        "vec_bad_set_index" => {
            let (v, idx, val) = (r.vec(), r.i(), r.f());
            let mut t = v;
            t.set_coord_at(idx, val)?;
            g.vec(&t);
        }
        "vec_zero_angle" => {
            let (v, which) = (r.vec(), r.i());
            let zero = GpVec::zero();
            g.f(if which == 0 { zero.angle(&v)? } else { v.angle(&zero)? });
        }
        "vec_zero_angle_ref" => {
            let (v, which) = (r.vec(), r.i());
            let zero = GpVec::zero();
            g.f(match which {
                0 => v.angle_with_ref(&v, &zero)?,
                1 => zero.angle_with_ref(&v, &v)?,
                _ => v.angle_with_ref(&zero, &v)?,
            });
        }
        "vec_zero_is_normal" => {
            let (v, tol, which) = (r.vec(), r.f(), r.i());
            let zero = GpVec::zero();
            g.b(if which == 0 { zero.is_normal(&v, tol)? } else { v.is_normal(&zero, tol)? });
        }
        "vec_zero_is_opposite" => {
            let (v, tol, which) = (r.vec(), r.f(), r.i());
            let zero = GpVec::zero();
            g.b(if which == 0 { zero.is_opposite(&v, tol)? } else { v.is_opposite(&zero, tol)? });
        }
        "vec_zero_is_parallel" => {
            let (v, tol, which) = (r.vec(), r.f(), r.i());
            let zero = GpVec::zero();
            g.b(if which == 0 { zero.is_parallel(&v, tol)? } else { v.is_parallel(&zero, tol)? });
        }
        "vec_cross" => {
            let (a, b) = (r.vec(), r.vec());
            g.vec(&a.crossed(&b));
            g.f(a.dot(&b));
        }
        "vec_arithmetic" => {
            let (a, b, s) = (r.vec(), r.vec(), r.f());
            g.vec(&a.added(&b));
            g.vec(&a.subtracted(&b));
            g.vec(&a.multiplied(s));
        }
        "vec_normalize" => {
            let a = r.vec();
            g.vec(&a.normalized()?);
        }
        "vec_relations" => {
            let (a, b, lin_tol, ang_tol) = (r.vec(), r.vec(), r.f(), r.f());
            g.b(a.is_equal(&b, lin_tol, ang_tol));
            g.b(a.is_normal(&b, ang_tol)?);
            g.b(a.is_opposite(&b, ang_tol)?);
            g.b(a.is_parallel(&b, ang_tol)?);
        }
        "vec_queries" => {
            let (a, b, vr) = (r.vec(), r.vec(), r.vec());
            g.fs(&[
                a.angle(&b)?,
                a.angle_with_ref(&b, &vr)?,
                a.magnitude(),
                a.square_magnitude(),
                a.cross_magnitude(&b),
                a.cross_square_magnitude(&b),
                a.dot(&b),
                a.dot_cross(&b, &vr),
            ]);
        }
        "vec_mutators" => {
            let (a, b, vr, s, r1, r2) = (r.vec(), r.vec(), r.vec(), r.f(), r.f(), r.f());
            let sd = if s == 0. { 1. } else { s };
            g.vec(&a.added(&b));
            g.vec(&a.subtracted(&b));
            g.vec(&a.multiplied(s));
            g.vec(&a.divided(sd));
            g.vec(&a.cross_crossed(&b, &vr));
            g.vec(&a.reversed());
            let mut m = a;
            m.add(&b);
            g.vec(&m);
            m = a;
            m.subtract(&b);
            g.vec(&m);
            m = a;
            m.multiply(s);
            g.vec(&m);
            m = a;
            m.divide(sd);
            g.vec(&m);
            m = a;
            m.cross(&b);
            g.vec(&m);
            m = a;
            m.cross_cross(&b, &vr);
            g.vec(&m);
            m = a;
            m.reverse();
            g.vec(&m);
            m.set_linear_form_2(s, &a, r1, &b);
            g.vec(&m);
            m.set_linear_form_2_plus(s, &a, r1, &b, &vr);
            g.vec(&m);
            m.set_linear_form_3(s, &a, r1, &b, r2, &vr);
            g.vec(&m);
            m.set_linear_form_3_plus(s, &a, r1, &b, r2, &vr, &a);
            g.vec(&m);
            m.set_linear_form_1_plus(s, &a, &vr);
            g.vec(&m);
            m.set_linear_form_sum(&a, &b);
            g.vec(&m);
        }
        "vec_mirrors" => {
            let (a, b, axis, plane) = (r.vec(), r.vec(), r.ax1()?, r.ax2()?);
            g.vec(&a.mirrored(&b));
            let mut m = a;
            m.mirror(&b);
            g.vec(&m);
            g.vec(&a.mirrored(&axis));
            m = a;
            m.mirror(&axis);
            g.vec(&m);
            g.vec(&a.mirrored(&plane));
            m = a;
            m.mirror(&plane);
            g.vec(&m);
        }
        "vec_motion" => {
            let (a, axis, s) = (r.vec(), r.ax1()?, r.f());
            g.vec(&a.rotated(&axis, s));
            let mut m = a;
            m.rotate(&axis, s);
            g.vec(&m);
            g.vec(&a.scaled(s));
            m = a;
            m.scale(s);
            g.vec(&m);
            let mut t = Trsf::new();
            t.set_rotation(&axis, s)?;
            g.vec(&a.transformed(&t));
            m = a;
            m.transform(&t)?;
            g.vec(&m);
        }

        // ---- gp_Dir ----
        "dir_construct" => {
            let a = r.xyz();
            g.dir(&Dir::new(a.x(), a.y(), a.z())?);
            g.dir(&Dir::from_vec(&GpVec::from_xyz(a))?);
            g.dir(&Dir::from_xyz(a)?);
        }
        "dir_angle" => {
            let (a, b) = (r.xyz(), r.xyz());
            let (d, e) = (Dir::new(a.x(), a.y(), a.z())?, Dir::new(b.x(), b.y(), b.z())?);
            g.f(d.angle(&e));
            g.f(d.dot(&e));
        }
        "dir_cross" => {
            let (a, b) = (r.xyz(), r.xyz());
            let (d, e) = (Dir::new(a.x(), a.y(), a.z())?, Dir::new(b.x(), b.y(), b.z())?);
            g.dir(&d.crossed(&e)?);
        }
        "dir_setters" => {
            let (d, i1, r1, r2, r3, r4) = (r.dir()?, r.i(), r.f(), r.f(), r.f(), r.f());
            let (e1, e2, e3) = (r.xyz(), r.xyz(), r.xyz());
            g.fs(&[d.coord(1)?, d.coord(2)?, d.coord(3)?]);
            let mut t = d;
            t.set_coord_at(i1, r1)?;
            t.set_x(r2)?;
            t.set_y(r3)?;
            t.set_z(r4)?;
            t.set_xyz(e1)?;
            g.dir(&t);
            g.dir(&Dir::default());
            g.dir(&Dir::from_direction(Direction::NegativeX));
            g.dir(&Dir::from_vec(&GpVec::from_xyz(e2))?);
            g.dir(&Dir::from_xyz(e3)?);
            g.dir(&Dir::default());
        }
        "dir_bad_coord" => {
            let (d, idx) = (r.dir()?, r.i());
            g.f(d.coord(idx)?);
        }
        "dir_bad_set_index" => {
            let (d, idx, val) = (r.dir()?, r.i(), r.f());
            let mut t = d;
            t.set_coord_at(idx, val)?;
            g.dir(&t);
        }
        "axis_enum_constructors" => {
            let p = r.pnt();
            g.ax1(&Ax1::default());
            for d in [
                Direction::X,
                Direction::Y,
                Direction::Z,
                Direction::NegativeX,
                Direction::NegativeY,
                Direction::NegativeZ,
            ] {
                let v = Dir::from_direction(d);
                g.dir(&v);
                g.ax1(&Ax1::from_direction(d));
                g.ax1(&Ax1::at(p, d));
                g.ax2(&Ax2::from_standard_direction(d));
                g.ax2(&Ax2::from_point_standard(p, d));
                g.ax3(&Ax3::from_standard_direction(d));
                g.ax3(&Ax3::from_point_standard(p, d));
            }
            g.ax2(&Ax2::from_standard_directions(p, Direction::Z, Direction::X));
            g.ax3(&Ax3::from_standard_directions(p, Direction::Z, Direction::X));
        }
        "dir_queries" => {
            let (a, b, tol) = (r.dir()?, r.dir()?, r.f());
            g.b(a.is_equal(&b, tol));
            g.b(a.is_normal(&b, tol));
            g.b(a.is_opposite(&b, tol));
            g.b(a.is_parallel(&b, tol));
        }
        "dir_angles" => {
            let (a, b, vref) = (r.dir()?, r.dir()?, r.dir()?);
            g.fs(&[a.angle(&b), a.angle_with_ref(&b, &vref), a.dot(&b), a.dot_cross(&b, &vref)]);
        }
        "dir_ops" => {
            let (a, b, c) = (r.dir()?, r.dir()?, r.dir()?);
            g.dir(&a.crossed(&b)?);
            g.dir(&a.cross_crossed(&b, &c)?);
            g.dir(&a.reversed());
            let mut m = a;
            m.cross(&b)?;
            g.dir(&m);
            m = a;
            m.cross_cross(&b, &c)?;
            g.dir(&m);
            m = a;
            m.reverse();
            g.dir(&m);
        }
        "dir_mirrors" => {
            let (a, b, axis, plane) = (r.dir()?, r.dir()?, r.ax1()?, r.ax2()?);
            g.dir(&a.mirrored(&b));
            let mut m = a;
            m.mirror(&b);
            g.dir(&m);
            g.dir(&a.mirrored(&axis));
            m = a;
            m.mirror(&axis);
            g.dir(&m);
            g.dir(&a.mirrored(&plane));
            m = a;
            m.mirror(&plane);
            g.dir(&m);
        }
        "dir_motion" => {
            let (a, axis, s) = (r.dir()?, r.ax1()?, r.f());
            g.dir(&a.rotated(&axis, s));
            let mut m = a;
            m.rotate(&axis, s);
            g.dir(&m);
            let mut t = Trsf::new();
            t.set_rotation(&axis, s)?;
            g.dir(&a.transformed(&t)?);
            m = a;
            m.transform(&t)?;
            g.dir(&m);
        }

        // ---- gp_Ax1 ----
        "ax1_access" => {
            let (a, loc, d) = (r.ax1()?, r.pnt(), r.dir()?);
            g.ax1(&a);
            let mut t = a;
            t.set_location(loc);
            t.set_direction(d);
            g.ax1(&t);
        }
        "ax1_relations" => {
            let (a, b, ang_tol, lin_tol) = (r.ax1()?, r.ax1()?, r.f(), r.f());
            g.b(a.is_coaxial(&b, ang_tol, lin_tol));
            g.b(a.is_normal(&b, ang_tol));
            g.b(a.is_opposite(&b, ang_tol));
            g.b(a.is_parallel(&b, ang_tol));
            g.f(a.angle(&b));
            g.b(a.is_coaxial(&a, 1e-9, 1e-9));
            g.b(a.is_opposite(&Ax1::new(*a.location(), a.direction().reversed()), 0.1));
            g.b(a.is_parallel(&a, 0.1));
        }
        "ax1_reverse" => {
            let a = r.ax1()?;
            g.ax1(&a.reversed());
            let mut t = a;
            t.reverse();
            g.ax1(&t);
        }
        "ax1_mirror_point" => {
            let (a, q) = (r.ax1()?, r.pnt());
            g.ax1(&a.mirrored(&q));
            let mut t = a;
            t.mirror(&q);
            g.ax1(&t);
        }
        "ax1_mirror_axis" => {
            let (a, o) = (r.ax1()?, r.ax1()?);
            g.ax1(&a.mirrored(&o));
            let mut t = a;
            t.mirror(&o);
            g.ax1(&t);
        }
        "ax1_mirror_plane" => {
            let (a, plane) = (r.ax1()?, r.ax2()?);
            g.ax1(&a.mirrored(&plane));
            let mut t = a;
            t.mirror(&plane);
            g.ax1(&t);
        }
        "ax1_motion" => {
            let (a, axis, q, s) = (r.ax1()?, r.ax1()?, r.pnt(), r.f());
            g.ax1(&a.rotated(&axis, s));
            let mut t = a;
            t.rotate(&axis, s);
            g.ax1(&t);
            g.ax1(&a.scaled(&q, s));
            t = a;
            t.scale(&q, s);
            g.ax1(&t);
            let mut tr = Trsf::new();
            tr.set_rotation(&axis, s)?;
            g.ax1(&a.transformed(&tr));
            t = a;
            t.transform(&tr);
            g.ax1(&t);
        }
        "ax1_translate_vec" => {
            let (a, v) = (r.ax1()?, r.vec());
            g.ax1(&a.translated(&v));
            let mut t = a;
            t.translate(&v);
            g.ax1(&t);
        }
        "ax1_translate_points" => {
            let (a, p1, p2) = (r.ax1()?, r.pnt(), r.pnt());
            g.ax1(&a.translated_points(&p1, &p2));
            let mut t = a;
            t.translate_points(&p1, &p2);
            g.ax1(&t);
        }

        // ---- gp_Ax2 ----
        "ax2_access" => {
            let (a, loc, axis, d4, d5, d6) = (r.ax2()?, r.pnt(), r.ax1()?, r.dir()?, r.dir()?, r.dir()?);
            g.ax2(&a);
            let mut t = a;
            t.set_location(loc);
            t.set_axis(axis)?;
            t.set_direction(d4)?;
            t.set_x_direction(d5)?;
            t.set_y_direction(d6)?;
            g.ax2(&t);
            g.ax2(&Ax2::default());
            g.ax2(&Ax2::from_point_standard(*a.location(), Direction::Z));
            g.ax2(&Ax2::from_standard_direction(Direction::Y));
            g.ax2(&Ax2::from_direction(loc, d4));
        }
        "ax2_relations" => {
            let (a, b) = (r.ax2()?, r.ax2()?);
            g.f(a.angle(&b));
        }
        "ax2_coplanar" => {
            let (a, o2, o1, lin_tol, ang_tol) = (r.ax2()?, r.ax2()?, r.ax1()?, r.f(), r.f());
            g.b(a.is_coplanar(&o2, lin_tol, ang_tol));
            g.b(a.is_coplanar(&o1, lin_tol, ang_tol));
        }
        "ax2_mirror_point" => {
            let (a, c) = (r.ax2()?, r.pnt());
            g.ax2(&a.mirrored(&c));
            let mut t = a;
            t.mirror(&c);
            g.ax2(&t);
        }
        "ax2_mirror_axis" => {
            let (a, o) = (r.ax2()?, r.ax1()?);
            g.ax2(&a.mirrored(&o));
            let mut t = a;
            t.mirror(&o);
            g.ax2(&t);
        }
        "ax2_mirror_plane" => {
            let (a, plane) = (r.ax2()?, r.ax2()?);
            g.ax2(&a.mirrored(&plane));
            let mut t = a;
            t.mirror(&plane);
            g.ax2(&t);
        }
        "ax2_motion" => {
            let (a, axis, q, s) = (r.ax2()?, r.ax1()?, r.pnt(), r.f());
            g.ax2(&a.rotated(&axis, s));
            let mut t = a;
            t.rotate(&axis, s);
            g.ax2(&t);
            g.ax2(&a.scaled(&q, s));
            t = a;
            t.scale(&q, s);
            g.ax2(&t);
            let mut tr = Trsf::new();
            tr.set_rotation(&axis, s)?;
            g.ax2(&a.transformed(&tr));
            t = a;
            t.transform(&tr);
            g.ax2(&t);
        }
        "ax2_translate_vec" => {
            let (a, v) = (r.ax2()?, r.vec());
            g.ax2(&a.translated(&v));
            let mut t = a;
            t.translate(&v);
            g.ax2(&t);
        }
        "ax2_translate_points" => {
            let (a, p1, p2) = (r.ax2()?, r.pnt(), r.pnt());
            g.ax2(&a.translated_points(&p1, &p2));
            let mut t = a;
            t.translate_points(&p1, &p2);
            g.ax2(&t);
        }

        // ---- gp_Ax3 ----
        "ax3_access" => {
            let (a, loc, axis, d4, d5, d6) = (r.ax3()?, r.pnt(), r.ax1()?, r.dir()?, r.dir()?, r.dir()?);
            g.ax3(&a);
            let mut t = a;
            t.set_location(loc);
            t.set_axis(axis)?;
            t.set_direction(d4)?;
            t.set_x_direction(d5)?;
            t.set_y_direction(d6)?;
            g.ax3(&t);
            g.ax2(&a.ax2());
            g.ax3(&Ax3::default());
            g.ax3(&Ax3::from_point_standard(*a.location(), Direction::Z));
            g.ax3(&Ax3::new(loc, d4, d5)?);
            g.ax3(&Ax3::from_standard_direction(Direction::Y));
        }
        "ax3_relations" => {
            let (a, b) = (r.ax3()?, r.ax3()?);
            g.f(a.angle(&b));
            g.ax2(&a.ax2());
        }
        "ax3_direct" => {
            let a = r.ax3()?;
            g.b(a.direct());
            let mut left = a;
            left.x_reverse();
            g.b(!left.direct());
        }
        "ax3_reverse" => {
            let a = r.ax3()?;
            let mut t = a;
            t.x_reverse();
            g.ax3(&t);
            t = a;
            t.y_reverse();
            g.ax3(&t);
            t = a;
            t.z_reverse();
            g.ax3(&t);
        }
        "ax3_coplanar" => {
            let (a, o3, o1, lin_tol, ang_tol) = (r.ax3()?, r.ax3()?, r.ax1()?, r.f(), r.f());
            g.b(a.is_coplanar(&o3, lin_tol, ang_tol));
            g.b(a.is_coplanar(&o1, lin_tol, ang_tol));
        }
        "ax3_mirror_point" => {
            let (a, c) = (r.ax3()?, r.pnt());
            g.ax3(&a.mirrored(&c));
            let mut t = a;
            t.mirror(&c);
            g.ax3(&t);
        }
        "ax3_mirror_axis" => {
            let (a, o) = (r.ax3()?, r.ax1()?);
            g.ax3(&a.mirrored(&o));
            let mut t = a;
            t.mirror(&o);
            g.ax3(&t);
        }
        "ax3_mirror_plane" => {
            let (a, plane) = (r.ax3()?, r.ax2()?);
            g.ax3(&a.mirrored(&plane));
            let mut t = a;
            t.mirror(&plane);
            g.ax3(&t);
        }
        "ax3_motion" => {
            let (a, axis, q, s) = (r.ax3()?, r.ax1()?, r.pnt(), r.f());
            g.ax3(&a.rotated(&axis, s));
            let mut t = a;
            t.rotate(&axis, s);
            g.ax3(&t);
            g.ax3(&a.scaled(&q, s));
            t = a;
            t.scale(&q, s);
            g.ax3(&t);
            let mut tr = Trsf::new();
            tr.set_rotation(&axis, s)?;
            g.ax3(&a.transformed(&tr));
            t = a;
            t.transform(&tr);
            g.ax3(&t);
        }
        "ax3_translate_vec" => {
            let (a, v) = (r.ax3()?, r.vec());
            g.ax3(&a.translated(&v));
            let mut t = a;
            t.translate(&v);
            g.ax3(&t);
        }
        "ax3_translate_points" => {
            let (a, p1, p2) = (r.ax3()?, r.pnt(), r.pnt());
            g.ax3(&a.translated_points(&p1, &p2));
            let mut t = a;
            t.translate_points(&p1, &p2);
            g.ax3(&t);
        }

        // ---- gp_Mat ----
        "mat_constructors" => {
            let (m, e1, e2, e3) = (r.mat(), r.xyz(), r.xyz(), r.xyz());
            g.mat(&Mat::new());
            g.mat(&m);
            g.mat(&Mat::from_columns(e1, e2, e3));
        }
        "mat_access" => {
            let m = r.mat();
            let (ci, ri, vr, vc, cr, cc, nv) = (r.i(), r.i(), r.i(), r.i(), r.i(), r.i(), r.f());
            g.xyz(&m.column(ci)?);
            g.xyz(&m.row(ri)?);
            g.xyz(&m.diagonal());
            g.f(m.value(vr, vc)?);
            g.f(m.determinant());
            let mut t = m;
            *t.change_value(cr, cc)? = nv;
            g.mat(&t);
        }
        "mat_singular" => {
            let m = r.mat();
            g.b(m.is_singular());
        }
        "mat_bad_index" => {
            let (m, row, col) = (r.mat(), r.i(), r.i());
            g.f(m.value(row, col)?);
        }
        "mat_bad_row" => {
            let (m, idx) = (r.mat(), r.i());
            g.xyz(&m.row(idx)?);
        }
        "mat_bad_column" => {
            let (m, idx) = (r.mat(), r.i());
            g.xyz(&m.column(idx)?);
        }
        "mat_bad_set_row" => {
            let (m, idx, e) = (r.mat(), r.i(), r.xyz());
            let mut t = m;
            t.set_row(idx, &e)?;
            g.mat(&t);
        }
        "mat_bad_set_column" => {
            let (m, idx, e) = (r.mat(), r.i(), r.xyz());
            let mut t = m;
            t.set_col(idx, &e)?;
            g.mat(&t);
        }
        "mat_bad_change_value" => {
            let (m, row, col) = (r.mat(), r.i(), r.i());
            let mut t = m;
            g.f(*t.change_value(row, col)?);
        }
        "mat_bad_set_value" => {
            let (m, row, col, val) = (r.mat(), r.i(), r.i(), r.f());
            let mut t = m;
            t.set_value(row, col, val)?;
            g.mat(&t);
        }
        "mat_zero_rotation" => {
            let angle = r.f();
            let mut t = Mat::new();
            t.set_rotation(&XYZ::zero(), angle)?;
            g.mat(&t);
        }
        "mat_setters" => {
            let m = r.mat();
            let ci = r.i();
            let (e1, e2, e3, e4, e5) = (r.xyz(), r.xyz(), r.xyz(), r.xyz(), r.xyz());
            let ri = r.i();
            let (f1, f2, f3, f4) = (r.f(), r.f(), r.f(), r.f());
            let (axis, angle, vr, vc, f5) = (r.xyz(), r.f(), r.i(), r.i(), r.f());
            let mut t = m;
            t.set_col(ci, &e1)?;
            g.mat(&t);
            t.set_cols(&e1, &e2, &e3);
            g.mat(&t);
            t.set_row(ri, &e5)?;
            g.mat(&t);
            t.set_rows(&e3, &e4, &e5);
            g.mat(&t);
            t.set_cross(&e4);
            g.mat(&t);
            t.set_dot(&e5);
            g.mat(&t);
            t.set_diagonal(f1, f2, f3);
            g.mat(&t);
            t.set_scale(f4);
            g.mat(&t);
            t.set_identity();
            g.mat(&t);
            t.set_rotation(&axis, angle)?;
            g.mat(&t);
            t.set_value(vr, vc, f5)?;
            g.mat(&t);
        }
        "mat_arithmetic" => {
            let (a, b, d, sm) = (r.mat(), r.mat(), r.f(), r.f());
            g.mat(&a.added(&b));
            g.mat(&a.subtracted(&b));
            g.mat(&a.divided(d));
            g.mat(&a.multiplied_mat(&b));
            g.mat(&a.multiplied_scalar(sm));
            let mut m = a;
            m.add(&b);
            g.mat(&m);
            m = a;
            m.subtract(&b);
            g.mat(&m);
            m = a;
            m.divide(d);
            g.mat(&m);
            m = a;
            m.multiply_mat(&b);
            g.mat(&m);
            m = a;
            m.pre_multiply(&b);
            g.mat(&m);
            m = a;
            m.multiply_scalar(sm);
            g.mat(&m);
        }
        "mat_inverse_power_transpose" => {
            let (a, pe) = (r.mat(), r.i());
            g.mat(&a.inverted()?);
            let mut m = a;
            m.invert()?;
            g.mat(&m);
            g.mat(&a.powered(pe)?);
            m = a;
            m.power(pe)?;
            g.mat(&m);
            g.mat(&a.powered(0)?);
            g.mat(&a.transposed());
            m = a;
            m.transpose();
            g.mat(&m);
        }

        // ---- gp_Trsf ----
        "trsf_setters" => {
            let (p, q) = (r.pnt(), r.pnt());
            let (mirror_axis, plane, rot_axis, s) = (r.ax1()?, r.ax2()?, r.ax1()?, r.f());
            let (tq_axis, tq_angle, scale) = (r.vec(), r.f(), r.f());
            let (from, to, c, a) = (r.ax3()?, r.ax3()?, r.vec(), r.xyz());
            let tq = Quaternion::from_axis_angle(&tq_axis, tq_angle)?;
            let mut t = Trsf::new();
            t.set_mirror(&q);
            g.trsf(&t);
            t.set_mirror(&mirror_axis);
            g.trsf(&t);
            t.set_mirror(&plane);
            g.trsf(&t);
            t.set_rotation(&rot_axis, s)?;
            g.trsf(&t);
            t.set_rotation_quaternion(&tq)?;
            g.trsf(&t);
            t.set_rotation_part(&tq);
            g.trsf(&t);
            t.set_scale(&q, scale)?;
            g.trsf(&t);
            t.set_displacement(&from, &to);
            g.trsf(&t);
            t.set_transformation_between(&from, &to);
            g.trsf(&t);
            t.set_transformation(&from);
            g.trsf(&t);
            t.set_transformation_quaternion(&tq, &c);
            g.trsf(&t);
            t.set_translation(&c);
            g.trsf(&t);
            t.set_translation_points(&p, &q);
            g.trsf(&t);
            t.set_translation_part(&c);
            g.trsf(&t);
            t.set_scale_factor(scale)?;
            g.trsf(&t);
            t.set_form(TrsfForm::Identity);
            g.trsf(&t);
            t.set_values([1., 0., 0., a.x(), 0., 1., 0., a.y(), 0., 0., 1., a.z()])?;
            g.trsf(&t);
        }
        "trsf_queries" => {
            let t = r.trsf()?;
            g.f(t.scale_factor());
            g.xyz(t.translation_part());
            g.mat(&t.vectorial_part());
            g.mat(t.h_vectorial_part());
            g.trsf(&t);
        }
        "trsf_mat4" => {
            let t = r.trsf()?;
            g.mat4(t.get_mat4());
        }
        "trsf_negative" => {
            let t = r.trsf()?;
            g.b(t.is_negative());
        }
        "trsf_form" => {
            let t = r.trsf()?;
            g.i(t.form() as i32);
        }
        "trsf_get_rotation" => {
            let t = r.trsf()?;
            let (ok, axis, angle) = t.get_rotation();
            g.b(ok);
            g.xyz(&axis);
            g.f(angle);
        }
        "trsf_quaternion" => {
            let t = r.trsf()?;
            g.quat(&t.get_rotation_quaternion());
        }
        "trsf_transform_coords" => {
            let (t, a) = (r.trsf()?, r.xyz());
            let mut c = a;
            t.transforms_xyz(&mut c);
            g.xyz(&c);
            let (tx, ty, tz) = t.transforms_coords(a.x(), a.y(), a.z());
            g.fs(&[tx, ty, tz]);
        }
        "trsf_composition" => {
            let (t1, t2, pe) = (r.trsf()?, r.trsf()?, r.i());
            g.trsf(&t1.multiplied(&t2));
            let mut m = t1;
            m.multiply(&t2);
            g.trsf(&m);
            m = t1;
            m.pre_multiply(&t2);
            g.trsf(&m);
            g.trsf(&t1.powered(pe)?);
            m = t1;
            m.power(pe)?;
            g.trsf(&m);
        }
        "trsf_inverse" => {
            let t = r.trsf()?;
            let mut ti = t;
            ti.invert()?;
            g.trsf(&ti);
        }
        "trsf_rotate" => {
            let (p, d, s, a) = (r.pnt(), r.dir()?, r.f(), r.xyz());
            let mut t = Trsf::new();
            t.set_rotation(&Ax1::new(p, d), s)?;
            g.pnt(&p.transformed(&t));
            g.vec(&GpVec::from_xyz(a).transformed(&t));
            for c in 1..=4 {
                g.f(t.value(1, c)?);
            }
            g.i(t.form() as i32);
        }
        "trsf_bad_value" => {
            let (t, row, col) = (r.trsf()?, r.i(), r.i());
            g.f(t.value(row, col)?);
        }
        "trsf_zero_scale" => {
            let p = r.pnt();
            let mut t = Trsf::new();
            t.set_scale(&p, 0.)?;
            g.trsf(&t);
        }

        // ---- gp_GTrsf ----
        "gtrsf_constructors" => {
            let (t, m, e) = (r.trsf()?, r.mat(), r.xyz());
            g.gtrsf(&GTrsf::new());
            g.gtrsf(&GTrsf::from_trsf(&t));
            g.gtrsf(&GTrsf::from_parts(m, e));
        }
        "gtrsf_setters" => {
            let (axis, plane, s, ri, ci, val) = (r.ax1()?, r.ax2()?, r.f(), r.i(), r.i(), r.f());
            let (m, c, tr, r1, r2, r3, a) = (r.mat(), r.xyz(), r.trsf()?, r.f(), r.f(), r.f(), r.xyz());
            let mut t = GTrsf::new();
            t.set_affinity(&axis, s);
            g.gtrsf(&t);
            t.set_affinity(&plane, s);
            g.gtrsf(&t);
            t.set_value(ri, ci, val)?;
            g.gtrsf(&t);
            t.set_vectorial_part(&m);
            g.gtrsf(&t);
            t.set_translation_part(&c);
            g.gtrsf(&t);
            t.set_trsf(&tr);
            g.gtrsf(&t);
            t.set_form();
            g.gtrsf(&t);
            t.set_mat4([[r1, 0., 0., a.x()], [0., r2, 0., a.y()], [0., 0., r3, a.z()], [0., 0., 0., 1.]]);
            g.gtrsf(&t);
        }
        "gtrsf_queries" => {
            let t = r.gtrsf()?;
            g.gtrsf(&t);
            g.xyz(t.translation_part());
            g.mat(t.vectorial_part());
        }
        "gtrsf_flags" => {
            let t = r.gtrsf()?;
            g.b(t.is_negative());
            g.b(t.is_singular());
        }
        "gtrsf_form" => {
            let t = r.gtrsf()?;
            g.i(t.form() as i32);
        }
        "gtrsf_value_transform" => {
            let (t, r1, c1, r2, c2, a) = (r.gtrsf()?, r.i(), r.i(), r.i(), r.i(), r.xyz());
            g.fs(&[t.value(r1, c1)?, t.value(r2, c2)?]);
            let mut c = a;
            t.transforms_xyz(&mut c);
            g.xyz(&c);
            let (gx, gy, gz) = t.transforms_coords(a.x(), a.y(), a.z());
            g.fs(&[gx, gy, gz]);
        }
        "gtrsf_composition" => {
            let (a, b, pe) = (r.gtrsf()?, r.gtrsf()?, r.i());
            g.gtrsf(&a.multiplied(&b));
            let mut m = a;
            m.multiply(&b);
            g.gtrsf(&m);
            m = a;
            m.pre_multiply(&b);
            g.gtrsf(&m);
            g.gtrsf(&a.powered(pe)?);
            m = a;
            m.power(pe)?;
            g.gtrsf(&m);
        }
        "gtrsf_to_trsf" => {
            let t = r.gtrsf()?;
            g.trsf(&t.trsf()?);
        }
        "gtrsf_mat4" => {
            let t = r.gtrsf()?;
            g.mat4(t.get_mat4());
        }
        "gtrsf_bad_value" => {
            let (t, row, col) = (r.gtrsf()?, r.i(), r.i());
            g.f(t.value(row, col)?);
        }
        "gtrsf_point" => {
            let (p, d, s, a) = (r.pnt(), r.dir()?, r.f(), r.xyz());
            let mut t = Trsf::new();
            t.set_rotation(&Ax1::new(p, d), s)?;
            let gt = GTrsf::from_trsf(&t);
            let mut v = a;
            gt.transforms_xyz(&mut v);
            g.xyz(&v);
        }

        // ---- gp_Quaternion ----
        "quat_constructors" => {
            let (qa, from, to, help, axis, angle, mat_axis, mat_angle) =
                (r.quat(), r.vec(), r.vec(), r.vec(), r.vec(), r.f(), r.xyz(), r.f());
            g.quat(&Quaternion::new());
            g.quat(&qa);
            g.quat(&Quaternion::from_vectors(&from, &to)?);
            g.quat(&Quaternion::from_vectors_with_help(&from, &to, &help)?);
            g.quat(&Quaternion::from_axis_angle(&axis, angle)?);
            let mut m = Mat::new();
            m.set_rotation(&mat_axis, mat_angle)?;
            g.quat(&Quaternion::from_matrix(&m));
        }
        "quat_equal" => {
            let (a, b) = (r.quat(), r.quat());
            g.b(a.is_equal(&b));
        }
        "quat_rotation_setters" => {
            let (from1, to1, from2, help, axis, angle) = (r.vec(), r.vec(), r.vec(), r.vec(), r.vec(), r.f());
            let (mat_axis, mat_angle, e1, e2, e3) = (r.xyz(), r.f(), r.f(), r.f(), r.f());
            let mut q = Quaternion::new();
            q.set_rotation(&from1, &to1)?;
            g.quat(&q);
            q.set_rotation_with_help(&from2, &from2.reversed(), &help)?;
            g.quat(&q);
            q.set_vector_and_angle(&axis, angle)?;
            g.quat(&q);
            let (got_axis, got_angle) = q.vector_and_angle();
            g.vec(&got_axis);
            g.f(got_angle);
            let mut m = Mat::new();
            m.set_rotation(&mat_axis, mat_angle)?;
            q.set_matrix(&m);
            g.quat(&q);
            g.mat(&q.get_matrix());
            q.set_euler_angles(EulerSequence::ExtrinsicXYZ, e1, e2, e3);
            g.quat(&q);
            let (a, b, c) = q.get_euler_angles(EulerSequence::ExtrinsicXYZ);
            g.fs(&[a, b, c]);
        }
        "quat_euler_sequences" => {
            let (e1, e2, e3, qn) = (r.f(), r.f(), r.f(), r.quat());
            for e in EULER_SEQUENCES {
                let mut q = Quaternion::new();
                q.set_euler_angles(e, e1, e2, e3);
                g.quat(&q);
                let (a, b, c) = q.get_euler_angles(e);
                g.fs(&[a, b, c]);
                let (a, b, c) = qn.get_euler_angles(e);
                g.fs(&[a, b, c]);
            }
        }
        "quat_components" => {
            let (a, b, c0, c1, c2, c3) = (r.quat(), r.quat(), r.f(), r.f(), r.f(), r.f());
            g.quat(&a);
            g.fs(&[a.x(), a.y(), a.z(), a.w()]);
            let mut q = a;
            q.set_components(c0, c1, c2, c3);
            g.quat(&q);
            q.set_quaternion(&b);
            g.quat(&q);
            q.set_ident();
            g.quat(&q);
        }
        "quat_norm_ops" => {
            let (a, sc) = (r.quat(), r.f());
            g.fs(&[a.square_norm(), a.norm()]);
            g.quat(&a.reversed());
            let mut q = a;
            q.reverse();
            g.quat(&q);
            g.quat(&a.inverted());
            q = a;
            q.invert();
            g.quat(&q);
            g.quat(&a.scaled(sc));
            q = a;
            q.scale(sc);
            g.quat(&q);
            q = a;
            q.stabilize_length();
            g.quat(&q);
            g.quat(&a.normalized());
            q = a;
            q.normalize();
            g.quat(&q);
            g.quat(&a.negated());
            g.f(a.rotation_angle());
        }
        "quat_arithmetic" => {
            let (a, b, sc, v) = (r.quat(), r.quat(), r.f(), r.vec());
            g.quat(&a.added(&b));
            g.quat(&a.subtracted(&b));
            g.quat(&a.multiplied_quaternion(&b));
            g.f(a.dot(&b));
            g.quat(&a.scaled(sc));
            g.quat(&a.negated());
            let mut q = a;
            q.add(&b);
            g.quat(&q);
            q = a;
            q.subtract(&b);
            g.quat(&q);
            q = a;
            q.multiply_quaternion(&b);
            g.quat(&q);
            g.vec(&a.rotate(&v));
        }
        "quat_zero_normalize" => {
            let mut q = Quaternion::from_components(0., 0., 0., 0.);
            q.normalize();
            g.quat(&q);
        }
        "quat_zero_axis_angle" => {
            let angle = r.f();
            g.quat(&Quaternion::from_axis_angle(&GpVec::zero(), angle)?);
        }
        "quat_zero_set_axis" => {
            let angle = r.f();
            let mut q = Quaternion::new();
            q.set_vector_and_angle(&GpVec::zero(), angle)?;
            g.quat(&q);
        }
        "quat_nlerp_all" => {
            let (a, b, t) = (r.quat(), r.quat(), r.f());
            g.quat(&QuaternionNLerp::interpolate_between(&a, &b, t));
            g.quat(&QuaternionNLerp::between(&a, &b).interpolate(t));
            let mut tool = QuaternionNLerp::new();
            tool.init(&a, &b);
            g.quat(&tool.interpolate(t));
            let mut tool = QuaternionNLerp::new();
            tool.init_from_unit(&a.normalized(), &b.normalized());
            g.quat(&tool.interpolate(t));
        }
        "quat_slerp_all" => {
            let (a, b, t) = (r.quat(), r.quat(), r.f());
            g.quat(&QuaternionSLerp::interpolate_between(&a, &b, t));
            g.quat(&QuaternionSLerp::between(&a, &b).interpolate(t));
            let mut tool = QuaternionSLerp::new();
            tool.init(&a, &b);
            g.quat(&tool.interpolate(t));
            let mut tool = QuaternionSLerp::new();
            tool.init_from_unit(&a.normalized(), &b.normalized());
            g.quat(&tool.interpolate(t));
        }
        "quaternion_axis_angle" => {
            let (a, axis, s) = (r.xyz(), r.vec(), r.f());
            let q = Quaternion::from_axis_angle(&axis, s)?;
            g.quat(&q);
            g.vec(&q.rotate(&GpVec::new(a.x(), a.y(), a.z())));
        }
        "quaternion_product" => {
            let (a, b) = (r.quat(), r.quat());
            g.quat(&a.multiplied_quaternion(&b));
            g.f(a.dot(&b));
        }
        "quaternion_nlerp" => {
            let (a, b, t) = (r.quat(), r.quat(), r.f());
            g.quat(&QuaternionNLerp::interpolate_between(&a, &b, t));
        }
        "quaternion_slerp" => {
            let (a, b, t) = (r.quat(), r.quat(), r.f());
            g.quat(&QuaternionSLerp::interpolate_between(&a, &b, t));
        }

        // ---- package constants and enums (no inputs) ----
        "constants" => {
            g.f(gp::resolution());
            g.pnt(&gp::origin());
            g.dir(&gp::dx());
            g.dir(&gp::dy());
            g.dir(&gp::dz());
            let ox = gp::ox();
            g.pnt(ox.location());
            g.dir(ox.direction());
            let plane = gp::xoy();
            g.pnt(plane.location());
            g.dir(plane.direction());
            g.dir(plane.x_direction());
            g.dir(plane.y_direction());
        }
        "constants_oy" => {
            let ax = gp::oy();
            g.pnt(ax.location());
            g.dir(ax.direction());
        }
        "constants_oz" => {
            let ax = gp::oz();
            g.pnt(ax.location());
            g.dir(ax.direction());
        }
        "constants_zox" => {
            let ax = gp::zox();
            g.pnt(ax.location());
            g.dir(ax.direction());
            g.dir(ax.x_direction());
            g.dir(ax.y_direction());
        }
        "constants_yoz" => {
            let ax = gp::yoz();
            g.pnt(ax.location());
            g.dir(ax.direction());
            g.dir(ax.x_direction());
            g.dir(ax.y_direction());
        }
        "enum_values" => {
            for e in EULER_SEQUENCES {
                g.i(e as i32);
            }
            for f in [
                TrsfForm::Identity,
                TrsfForm::Rotation,
                TrsfForm::Translation,
                TrsfForm::PntMirror,
                TrsfForm::Ax1Mirror,
                TrsfForm::Ax2Mirror,
                TrsfForm::Scale,
                TrsfForm::CompoundTrsf,
                TrsfForm::Other,
            ] {
                g.i(f as i32);
            }
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
/// otherwise; bools and ints always exactly.
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
fn gp3d_matches_occt() {
    let text = include_str!("gp3d.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in gp3d.txt:\n{line}");
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
    assert_eq!(lines, 2425, "golden line count changed");
    println!("{lines} lines, {} values, {} bit-identical, worst {} ulps", tally.values, tally.exact, tally.worst);
}
