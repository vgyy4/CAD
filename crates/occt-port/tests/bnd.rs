//! Numerical reference test for the in-scope parts of OCCT 8.0.1's `Bnd` and `CSLib` packages.
//!
//! `bnd.txt` is printed by `oracle/bnd.cpp` (real OCCT). Each line is
//! `<family> <inputs...> | <outputs...>`: f64 values are 16-hex-digit bit patterns, ints are decimal and bools are
//! 0/1. Every input of a call is on the left of ` | `; this test reads them back with the `Rd` reader methods of the
//! same name as the oracle's schema words (`f i xyz pnt vec dir xy pnt2d dir2d bx bx2 trsf trsf2d rg sph pts tols obb
//! bsb a2 c2d la`) and replays the call on the Rust API of docs/api-bnd.md. The right side is the expected values,
//! `throws` (OCCT raised an exception) or `unchecked` (OCCT 8.0.1 as built has that check compiled out; the result
//! is unspecified and the Rust API must return an error).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::bnd::{BndBoundSortBox, BndBox, BndBox2d, BndObb, BndRange, BndSphere};
use occt_port::cslib::{self, Class2d, NormalPolyDef, VecArray2};
use occt_port::gp::{
    self, Ax1, Ax2, Ax2d, Dir, Dir2d, Lin, Lin2d, Pln, Pnt, Pnt2d, Quaternion, Trsf, Trsf2d, Vec as GpVec, Vec2d, XY,
    XYZ,
};
use occt_port::math1d::{Function, FunctionWithDerivative};
use occt_port::mathlin::MathError;
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for the results that depend on sin/cos/pow (`cslib_normal_order`,
/// `cslib_normal_poly_def`), in units of f64::EPSILON times the largest absolute value on the line (at least 1; zero
/// tolerance if that value is infinite).
const MAX_ULPS: f64 = 8.0;

/// Every family with inputs must have at least this many distinct lines.
const MIN_DISTINCT: usize = 16;

// BEGIN GENERATED
/// Families that really take no input: they are printed exactly once.
const INPUT_LESS: &[&str] = &["cslib_enum_values", "sphere_default"];

/// Families whose results use only + - * / sqrt (and comparisons): compared bit for bit. The others (`cslib_normal_order`,
/// `cslib_normal_poly_def`) call sin/cos/pow and are compared with MAX_ULPS.
const EXACT: &[&str] = &[
    "box2d_add_box",
    "box2d_add_dir",
    "box2d_add_pnt",
    "box2d_add_pnt_dir",
    "box2d_distance",
    "box2d_dump",
    "box2d_gap",
    "box2d_get",
    "box2d_is_out_box",
    "box2d_is_out_box_trsf",
    "box2d_is_out_lin",
    "box2d_is_out_pnt",
    "box2d_is_out_segment",
    "box2d_is_out_trsf_box_trsf",
    "box2d_modify",
    "box2d_set_pnt",
    "box2d_set_pnt_dir",
    "box2d_state",
    "box2d_transformed",
    "box2d_update2",
    "box2d_update4",
    "box_add_box",
    "box_add_dir",
    "box_add_pnt",
    "box_add_pnt_dir",
    "box_corners",
    "box_distance",
    "box_dump",
    "box_finite_part",
    "box_gap",
    "box_get",
    "box_is_out_box",
    "box_is_out_box_trsf",
    "box_is_out_lin",
    "box_is_out_pln",
    "box_is_out_pnt",
    "box_is_out_segment",
    "box_is_out_trsf_box_trsf",
    "box_modify",
    "box_set_pnt",
    "box_set_pnt_dir",
    "box_state",
    "box_thin",
    "box_transformed",
    "box_update3",
    "box_update6",
    "bsb_add",
    "bsb_compare_box",
    "bsb_compare_box_gen",
    "bsb_compare_plane",
    "bsb_compare_plane_gen",
    "class2d_default",
    "class2d_internal",
    "class2d_si_dans",
    "class2d_si_dans_on_mode",
    "cslib_dnnormal",
    "cslib_dnnuv",
    "cslib_dnnuv_two",
    "cslib_enum_values",
    "cslib_normal_mag_tol",
    "cslib_normal_second_order",
    "cslib_normal_sin_tol",
    "obb_add_obb",
    "obb_add_pnt",
    "obb_completely_inside",
    "obb_ctor",
    "obb_enlarge",
    "obb_from_box",
    "obb_is_out_obb",
    "obb_is_out_pnt",
    "obb_position",
    "obb_rebuild",
    "obb_rebuild_optimal",
    "obb_rebuild_optimal_special",
    "obb_rebuild_special",
    "obb_setters",
    "obb_state",
    "obb_vertices",
    "obb_void_aabox",
    "range_add_param",
    "range_add_range",
    "range_common",
    "range_enlarge",
    "range_intermediate",
    "range_is_intersected",
    "range_is_out",
    "range_is_out_range",
    "range_set_void",
    "range_shift",
    "range_split",
    "range_state",
    "range_trim",
    "range_union",
    "sphere_add",
    "sphere_default",
    "sphere_distances",
    "sphere_is_out_pnt",
    "sphere_is_out_sphere",
    "sphere_set_valid",
    "sphere_state",
];

/// Every family the oracle must print (a family that disappears from bnd.txt fails).
const FAMILIES: &[&str] = &[
    "box2d_add_box",
    "box2d_add_dir",
    "box2d_add_pnt",
    "box2d_add_pnt_dir",
    "box2d_distance",
    "box2d_dump",
    "box2d_gap",
    "box2d_get",
    "box2d_is_out_box",
    "box2d_is_out_box_trsf",
    "box2d_is_out_lin",
    "box2d_is_out_pnt",
    "box2d_is_out_segment",
    "box2d_is_out_trsf_box_trsf",
    "box2d_modify",
    "box2d_set_pnt",
    "box2d_set_pnt_dir",
    "box2d_state",
    "box2d_transformed",
    "box2d_update2",
    "box2d_update4",
    "box_add_box",
    "box_add_dir",
    "box_add_pnt",
    "box_add_pnt_dir",
    "box_corners",
    "box_distance",
    "box_dump",
    "box_finite_part",
    "box_gap",
    "box_get",
    "box_is_out_box",
    "box_is_out_box_trsf",
    "box_is_out_lin",
    "box_is_out_pln",
    "box_is_out_pnt",
    "box_is_out_segment",
    "box_is_out_trsf_box_trsf",
    "box_modify",
    "box_set_pnt",
    "box_set_pnt_dir",
    "box_state",
    "box_thin",
    "box_transformed",
    "box_update3",
    "box_update6",
    "bsb_add",
    "bsb_compare_box",
    "bsb_compare_box_gen",
    "bsb_compare_plane",
    "bsb_compare_plane_gen",
    "class2d_default",
    "class2d_internal",
    "class2d_si_dans",
    "class2d_si_dans_on_mode",
    "cslib_dnnormal",
    "cslib_dnnuv",
    "cslib_dnnuv_two",
    "cslib_enum_values",
    "cslib_normal_mag_tol",
    "cslib_normal_order",
    "cslib_normal_poly_def",
    "cslib_normal_second_order",
    "cslib_normal_sin_tol",
    "obb_add_obb",
    "obb_add_pnt",
    "obb_completely_inside",
    "obb_ctor",
    "obb_enlarge",
    "obb_from_box",
    "obb_is_out_obb",
    "obb_is_out_pnt",
    "obb_position",
    "obb_rebuild",
    "obb_rebuild_optimal",
    "obb_rebuild_optimal_special",
    "obb_rebuild_special",
    "obb_setters",
    "obb_state",
    "obb_vertices",
    "obb_void_aabox",
    "range_add_param",
    "range_add_range",
    "range_common",
    "range_enlarge",
    "range_intermediate",
    "range_is_intersected",
    "range_is_out",
    "range_is_out_range",
    "range_set_void",
    "range_shift",
    "range_split",
    "range_state",
    "range_trim",
    "range_union",
    "sphere_add",
    "sphere_default",
    "sphere_distances",
    "sphere_is_out_pnt",
    "sphere_is_out_sphere",
    "sphere_set_valid",
    "sphere_state",
];

/// The number of lines of tests/bnd.txt.
const GOLDEN_LINES: usize = 3532;
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

#[derive(Clone, Debug)]
enum Tok {
    F(f64),
    B(bool),
    I(i32),
    /// A `Dump()` text, spaces as `_` and newlines as `\n`.
    S(String),
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
    fn dump(&mut self, text: &str) {
        self.0.push(Tok::S(text.replace('\n', "\\n").replace(' ', "_")));
    }
    fn xyz(&mut self, v: &XYZ) {
        self.fs(&[v.x(), v.y(), v.z()]);
    }
    fn pnt(&mut self, v: &Pnt) {
        self.xyz(v.xyz());
    }
    fn pnt2d(&mut self, v: &Pnt2d) {
        self.fs(&[v.x(), v.y()]);
    }
    fn dir(&mut self, v: &Dir) {
        self.xyz(v.xyz());
    }
    fn vec(&mut self, v: &GpVec) {
        self.xyz(v.xyz());
    }
    fn opt_f(&mut self, v: Option<f64>) {
        self.b(v.is_some());
        if let Some(v) = v {
            self.f(v);
        }
    }
    /// `put_box` of the oracle.
    fn bx(&mut self, b: &BndBox) {
        self.b(b.is_void());
        self.b(b.is_whole());
        self.b(b.is_open());
        self.b(b.is_open_xmin());
        self.b(b.is_open_xmax());
        self.b(b.is_open_ymin());
        self.b(b.is_open_ymax());
        self.b(b.is_open_zmin());
        self.b(b.is_open_zmax());
        self.f(b.gap());
        let l = b.limits();
        assert_eq!(
            [l.xmin, l.xmax, l.ymin, l.ymax, l.zmin, l.zmax].map(f64::to_bits),
            [b.get_x_min(), b.get_x_max(), b.get_y_min(), b.get_y_max(), b.get_z_min(), b.get_z_max()]
                .map(f64::to_bits),
            "limits() and the GetXMin.. accessors differ"
        );
        self.fs(&[l.xmin, l.xmax, l.ymin, l.ymax, l.zmin, l.zmax]);
        self.f(b.square_extent());
        self.b(b.has_finite_part());
    }
    /// `put_box2` of the oracle.
    fn bx2(&mut self, b: &BndBox2d) {
        self.b(b.is_void());
        self.b(b.is_whole());
        self.b(b.is_open_xmin());
        self.b(b.is_open_xmax());
        self.b(b.is_open_ymin());
        self.b(b.is_open_ymax());
        self.f(b.gap());
        let l = b.limits();
        assert_eq!(
            [l.xmin, l.xmax, l.ymin, l.ymax].map(f64::to_bits),
            [b.get_x_min(), b.get_x_max(), b.get_y_min(), b.get_y_max()].map(f64::to_bits),
            "limits() and the GetXMin.. accessors differ"
        );
        self.fs(&[l.xmin, l.xmax, l.ymin, l.ymax]);
        self.f(b.square_extent());
    }
    /// `put_range` of the oracle.
    fn range(&mut self, r: &BndRange) {
        self.b(r.is_void());
        let g = r.get();
        self.b(g.is_some());
        if let Some((a, c)) = g {
            self.f(a);
            self.f(c);
        }
    }
    /// `put_sphere` of the oracle.
    fn sphere(&mut self, s: &BndSphere) {
        self.i(s.u());
        self.i(s.v());
        self.b(s.is_valid());
        self.xyz(s.center());
        self.f(s.radius());
        self.f(s.square_extent());
    }
    /// `put_obb` of the oracle.
    fn obb(&mut self, o: &BndObb) {
        self.b(o.is_void());
        self.b(o.is_aabox());
        self.xyz(o.center());
        self.xyz(o.x_direction());
        self.xyz(o.y_direction());
        self.xyz(o.z_direction());
        self.fs(&[o.x_hsize(), o.y_hsize(), o.z_hsize()]);
        let (x, y, z) = o.half_sizes();
        self.fs(&[x, y, z]);
        self.f(o.square_extent());
    }
    fn list(&mut self, v: &[i32]) {
        self.i(v.len() as i32);
        for x in v {
            self.i(*x);
        }
    }
    /// The `written` flag of the oracle and the direction when it is written.
    fn written(&mut self, d: &Option<Dir>) {
        self.b(d.is_some());
        if let Some(d) = d {
            self.dir(d);
        }
    }
}

// ---------------------------------------------------------------------------------
// Recipes read from the inputs of a line

#[derive(Clone, Copy)]
struct BxSpec {
    kind: i32,
    c: [f64; 6],
    gap: f64,
    flags: i32,
}

/// The `Bnd_Box` of a recipe (see the header of oracle/bnd.cpp).
fn mk_box(s: &BxSpec) -> BndBox {
    let c = s.c;
    let mut b = match s.kind {
        1 => BndBox::from_corners(&Pnt::new(c[0], c[1], c[2]), &Pnt::new(c[3], c[4], c[5])),
        2 => {
            let mut b = BndBox::new();
            b.update(c[0], c[1], c[2], c[3], c[4], c[5]);
            b
        }
        3 => {
            let mut b = BndBox::new();
            b.set_whole();
            b
        }
        4 => {
            let mut b = BndBox::new();
            b.update_point(c[0], c[1], c[2]);
            b
        }
        _ => BndBox::new(),
    };
    b.set_gap(s.gap);
    apply_open_bits(&mut b, s.flags);
    b
}

fn apply_open_bits(b: &mut BndBox, flags: i32) {
    if flags & 1 != 0 {
        b.open_xmin();
    }
    if flags & 2 != 0 {
        b.open_xmax();
    }
    if flags & 4 != 0 {
        b.open_ymin();
    }
    if flags & 8 != 0 {
        b.open_ymax();
    }
    if flags & 16 != 0 {
        b.open_zmin();
    }
    if flags & 32 != 0 {
        b.open_zmax();
    }
}

#[derive(Clone, Copy)]
struct Bx2Spec {
    kind: i32,
    c: [f64; 4],
    gap: f64,
    flags: i32,
}

fn mk_box2(s: &Bx2Spec) -> BndBox2d {
    let c = s.c;
    let mut b = BndBox2d::new();
    match s.kind {
        1 => b.update(c[0], c[1], c[2], c[3]),
        2 => b.set_whole(),
        3 => b.update_point(c[0], c[1]),
        4 => b.set_point(&Pnt2d::new(c[0], c[1])),
        _ => {}
    }
    b.set_gap(s.gap);
    apply_open_bits2(&mut b, s.flags);
    b
}

fn apply_open_bits2(b: &mut BndBox2d, flags: i32) {
    if flags & 1 != 0 {
        b.open_xmin();
    }
    if flags & 2 != 0 {
        b.open_xmax();
    }
    if flags & 4 != 0 {
        b.open_ymin();
    }
    if flags & 8 != 0 {
        b.open_ymax();
    }
}

#[derive(Clone, Copy)]
struct RgSpec {
    kind: i32,
    a: f64,
    b: f64,
}

fn mk_range(s: &RgSpec) -> Result<BndRange, Thrown> {
    Ok(match s.kind {
        1 => BndRange::from_bounds(s.a, s.b)?,
        2 => {
            let mut r = BndRange::new();
            r.add_parameter(s.a);
            r.add_parameter(s.b);
            r
        }
        3 => {
            let mut r = BndRange::new();
            r.add_parameter(s.a);
            r
        }
        _ => BndRange::new(),
    })
}

struct SpSpec {
    c: XYZ,
    r: f64,
    u: i32,
    v: i32,
    valid: i32,
}

fn mk_sphere(s: &SpSpec) -> BndSphere {
    let mut b = BndSphere::with_center(&s.c, s.r, s.u, s.v);
    b.set_valid(s.valid != 0);
    b
}

struct PtsSpec {
    pts: Vec<Pnt>,
    tols: Option<Vec<f64>>,
}

struct ObbSpec {
    kind: i32,
    center: Pnt,
    axes: [Dir; 3],
    hs: [f64; 3],
    bx: BxSpec,
    pts: PtsSpec,
    optimal: bool,
    aabox: i32,
}

fn mk_obb(s: &ObbSpec) -> Result<BndObb, Thrown> {
    let mut o = match s.kind {
        1 => BndObb::from_axes(&s.center, &s.axes[0], &s.axes[1], &s.axes[2], s.hs[0], s.hs[1], s.hs[2]),
        2 => BndObb::from_box(&mk_box(&s.bx)),
        3 => {
            let mut o = BndObb::new();
            o.rebuild(&s.pts.pts, s.pts.tols.as_deref(), s.optimal)?;
            o
        }
        _ => BndObb::new(),
    };
    if s.aabox >= 0 {
        o.set_aabox(s.aabox != 0);
    }
    Ok(o)
}

struct GenSpec {
    seed: i64,
    n: i32,
    lo: f64,
    hi: f64,
    size: f64,
    gmode: i32,
}

/// The box generator of docs/api-bnd.md ("Box generator").
fn gen_boxes(g: &GenSpec) -> Vec<BndBox> {
    let mut st: u64 = 0x9E37_79B9_7F4A_7C15 ^ (g.seed as u64).wrapping_mul(0xD1B5_4A32_D192_ED03);
    if st == 0 {
        st = 1;
    }
    let mut r = |lo: f64, hi: f64| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        lo + (hi - lo) * ((st >> 11) as f64) / 9007199254740992.0
    };
    let mut out = Vec::new();
    for i in 0..g.n {
        let (x, y, z) = (r(g.lo, g.hi), r(g.lo, g.hi), r(g.lo, g.hi));
        let (sx, sy, sz) = (r(0.0, g.size), r(0.0, g.size), r(0.0, g.size));
        let mut b = BndBox::new();
        if !(g.gmode == 1 && i % 5 == 4) {
            b.update(x, y, z, x + sx, y + sy, z + sz);
            if g.gmode == 1 && i % 7 == 3 {
                b.set_gap(0.1 * sx);
            }
        }
        out.push(b);
    }
    out
}

struct BsbSpec {
    mode: i32,
    lo: i32,
    n: i32,
    enc: BxSpec,
    boxes: Vec<BxSpec>,
    adds: Vec<(i32, BxSpec)>,
    generated: Option<GenSpec>,
}

fn mk_bsb(s: &BsbSpec) -> Result<BndBoundSortBox, Thrown> {
    let mut b = BndBoundSortBox::new();
    let explicit = |v: &[BxSpec]| v.iter().map(mk_box).collect::<Vec<_>>();
    match s.mode {
        0 => b.initialize_boxes(&explicit(&s.boxes), s.lo),
        1 => b.initialize_enclosing_boxes(&mk_box(&s.enc), &explicit(&s.boxes), s.lo),
        2 => {
            b.initialize_enclosing_count(&mk_box(&s.enc), s.n)?;
            for (idx, bx) in &s.adds {
                b.add(&mk_box(bx), *idx)?;
            }
        }
        3 => b.initialize_boxes(&gen_boxes(s.generated.as_ref().unwrap()), s.lo),
        4 => b.initialize_enclosing_boxes(&mk_box(&s.enc), &gen_boxes(s.generated.as_ref().unwrap()), s.lo),
        _ => {
            let v = gen_boxes(s.generated.as_ref().unwrap());
            b.initialize_enclosing_count(&mk_box(&s.enc), v.len() as i32)?;
            for (i, bx) in v.iter().enumerate() {
                b.add(bx, i as i32 + 1)?;
            }
        }
    }
    Ok(b)
}

struct A2Spec {
    rlo: i32,
    clo: i32,
    nr: usize,
    nc: usize,
    v: Vec<GpVec>,
}

fn mk_a2(a: &A2Spec) -> Result<VecArray2, Thrown> {
    Ok(VecArray2::new(a.rlo, a.clo, a.nr, a.nc, a.v.clone())?)
}

struct C2Spec {
    kind: i32,
    pts: Vec<Pnt2d>,
    tol_u: f64,
    tol_v: f64,
    umin: f64,
    vmin: f64,
    umax: f64,
    vmax: f64,
}

fn mk_class2d(c: &C2Spec) -> Class2d {
    if c.kind == 0 {
        Class2d::new()
    } else {
        Class2d::from_points(&c.pts, c.tol_u, c.tol_v, c.umin, c.vmin, c.umax, c.vmax)
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
    /// The number of points of the last `pts` word (the `tols` word that follows has as many values).
    last_n: usize,
    /// A reader failure (a constructor that raises); see `dir`.
    err: bool,
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
    /// A raw (un-normalized) direction; `Dir::new` normalizes it exactly like the oracle did. A failure is
    /// remembered in `err` (the line then must be a `throws` / `unchecked` one) and the rest of the line is still read.
    fn dir(&mut self) -> Dir {
        let x = self.f();
        let y = self.f();
        let z = self.f();
        Dir::new(x, y, z).unwrap_or_else(|_| {
            self.err = true;
            Dir::new(1.0, 0.0, 0.0).unwrap()
        })
    }
    fn xy(&mut self) -> XY {
        let x = self.f();
        let y = self.f();
        XY::new(x, y)
    }
    fn pnt2d(&mut self) -> Pnt2d {
        let x = self.f();
        let y = self.f();
        Pnt2d::new(x, y)
    }
    fn dir2d(&mut self) -> Dir2d {
        let x = self.f();
        let y = self.f();
        Dir2d::new(x, y).unwrap_or_else(|_| {
            self.err = true;
            Dir2d::new(1.0, 0.0).unwrap()
        })
    }
    fn ax2d(&mut self) -> Ax2d {
        let p = self.pnt2d();
        let d = self.dir2d();
        Ax2d::new(p, d)
    }
    /// kind xmin ymin zmin xmax ymax zmax gap flags.
    fn bx(&mut self) -> BxSpec {
        let kind = self.i();
        let mut c = [0.0; 6];
        for e in &mut c {
            *e = self.f();
        }
        let gap = self.f();
        let flags = self.i();
        BxSpec { kind, c, gap, flags }
    }
    /// kind xmin ymin xmax ymax gap flags.
    fn bx2(&mut self) -> Bx2Spec {
        let kind = self.i();
        let mut c = [0.0; 4];
        for e in &mut c {
            *e = self.f();
        }
        let gap = self.f();
        let flags = self.i();
        Bx2Spec { kind, c, gap, flags }
    }
    /// A 3D transformation recipe: kind, then the fields that kind uses (see the header of oracle/bnd.cpp).
    fn trsf(&mut self) -> Result<Trsf, Thrown> {
        let kind = self.i();
        let mut t = Trsf::new();
        match kind {
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
                let p = self.pnt();
                let d = self.dir();
                t.set_mirror(&Ax1::new(p, d));
            }
            5 => {
                let p = self.pnt();
                t.set_mirror(&p);
            }
            6 => {
                let p = self.pnt();
                let d = self.dir();
                t.set_mirror(&Ax2::from_direction(p, d));
            }
            7 | 8 => {
                let x = self.f();
                let y = self.f();
                let z = self.f();
                let w = self.f();
                t.set_rotation_quaternion(&Quaternion::from_components(x, y, z, w))?;
                if kind == 8 {
                    let v = self.xyz();
                    let factor = self.f();
                    t.set_translation_part(&GpVec::from_xyz(v));
                    t.set_scale_factor(factor)?;
                }
            }
            9 => {}
            _ => panic!("unknown 3D transformation recipe {kind}"),
        }
        Ok(t)
    }
    /// A 2D transformation recipe: kind, then the fields that kind uses (see the header of oracle/bnd.cpp).
    fn trsf2d(&mut self) -> Result<Trsf2d, Thrown> {
        let kind = self.i();
        let mut t = Trsf2d::new();
        match kind {
            2 => {
                let p = self.pnt2d();
                let factor = self.f();
                t.set_scale(&p, factor);
            }
            3 => {
                let v = self.xy();
                t.set_translation(&Vec2d::from_xy(v));
            }
            4 => {
                let (p, q) = (self.pnt2d(), self.pnt2d());
                t.set_translation_points(&p, &q);
            }
            5 => {
                let p = self.pnt2d();
                t.set_mirror(&p);
            }
            6 => {
                let a = self.ax2d();
                t.set_mirror(&a);
            }
            7 => {
                let a = self.ax2d();
                t.set_transformation(&a);
            }
            8 => {
                let from = self.ax2d();
                let to = self.ax2d();
                t.set_transformation_between(&from, &to);
            }
            9 => {
                let a = self.ax2d();
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
            12 => {}
            _ => panic!("unknown 2D transformation recipe {kind}"),
        }
        Ok(t)
    }
    fn rg(&mut self) -> RgSpec {
        let kind = self.i();
        let a = self.f();
        let b = self.f();
        RgSpec { kind, a, b }
    }
    fn sph(&mut self) -> SpSpec {
        let c = self.xyz();
        let r = self.f();
        let u = self.i();
        let v = self.i();
        let valid = self.i();
        SpSpec { c, r, u, v, valid }
    }
    /// lo n x1 y1 z1 .. xn yn zn (the lower index is irrelevant to the Rust API).
    fn pts(&mut self) -> Vec<Pnt> {
        let _lo = self.i();
        let n = self.i() as usize;
        self.last_n = n;
        (0..n).map(|_| self.pnt()).collect()
    }
    /// flag [t1 .. tn] with the n of the last `pts`.
    fn tols(&mut self) -> Option<Vec<f64>> {
        if self.i() == 0 {
            return None;
        }
        let n = self.last_n;
        Some((0..n).map(|_| self.f()).collect())
    }
    /// kind payload aabox (see the header of oracle/bnd.cpp).
    fn obb(&mut self) -> ObbSpec {
        let kind = self.i();
        let unit = Dir::new(1.0, 0.0, 0.0).unwrap();
        let mut s = ObbSpec {
            kind,
            center: Pnt::new(0.0, 0.0, 0.0),
            axes: [unit, unit, unit],
            hs: [0.0; 3],
            bx: BxSpec { kind: 0, c: [0.0; 6], gap: 0.0, flags: 0 },
            pts: PtsSpec { pts: Vec::new(), tols: None },
            optimal: false,
            aabox: -1,
        };
        match kind {
            1 => {
                s.center = self.pnt();
                for k in 0..3 {
                    s.axes[k] = self.dir();
                }
                for k in 0..3 {
                    s.hs[k] = self.f();
                }
            }
            2 => s.bx = self.bx(),
            3 => {
                s.pts.pts = self.pts();
                s.pts.tols = self.tols();
                s.optimal = self.i() != 0;
            }
            _ => {}
        }
        s.aabox = self.i();
        s
    }
    /// mode lo n, then (see the header of oracle/bnd.cpp and docs/api-bnd.md).
    fn bsb(&mut self) -> BsbSpec {
        let mode = self.i();
        let lo = self.i();
        let n = self.i();
        let zero = BxSpec { kind: 0, c: [0.0; 6], gap: 0.0, flags: 0 };
        let mut s = BsbSpec { mode, lo, n, enc: zero, boxes: Vec::new(), adds: Vec::new(), generated: None };
        match mode {
            0 => s.boxes = (0..n).map(|_| self.bx()).collect(),
            1 => {
                s.enc = self.bx();
                s.boxes = (0..n).map(|_| self.bx()).collect();
            }
            2 => {
                s.enc = self.bx();
                let m = self.i();
                s.adds = (0..m)
                    .map(|_| {
                        let idx = self.i();
                        (idx, self.bx())
                    })
                    .collect();
            }
            3 => s.generated = Some(self.bgen()),
            _ => {
                s.enc = self.bx();
                s.generated = Some(self.bgen());
            }
        }
        s
    }
    fn bgen(&mut self) -> GenSpec {
        let seed: i64 = self.word().parse().expect("seed");
        let n = self.i();
        let lo = self.f();
        let hi = self.f();
        let size = self.f();
        let gmode = self.i();
        GenSpec { seed, n, lo, hi, size, gmode }
    }
    /// rlo clo nr nc then nr*nc vectors, row-major.
    fn a2(&mut self) -> A2Spec {
        let rlo = self.i();
        let clo = self.i();
        let nr = self.i() as usize;
        let nc = self.i() as usize;
        let v = (0..nr * nc).map(|_| self.vec()).collect();
        A2Spec { rlo, clo, nr, nc, v }
    }
    /// kind, then for kind > 0: [lo if kind 1] n points tolu tolv umin vmin umax vmax.
    fn c2d(&mut self) -> C2Spec {
        let kind = self.i();
        let mut c =
            C2Spec { kind, pts: Vec::new(), tol_u: 0.0, tol_v: 0.0, umin: 0.0, vmin: 0.0, umax: 0.0, vmax: 0.0 };
        if kind > 0 {
            if kind == 1 {
                let _lo = self.i();
            }
            let n = self.i() as usize;
            c.pts = (0..n).map(|_| self.pnt2d()).collect();
            c.tol_u = self.f();
            c.tol_v = self.f();
            c.umin = self.f();
            c.vmin = self.f();
            c.umax = self.f();
            c.vmax = self.f();
        }
        c
    }
    /// lo n v1 .. vn (an array of doubles with lower index lo): returns (lo, values).
    fn la(&mut self) -> (i32, Vec<f64>) {
        let lo = self.i();
        let n = self.i() as usize;
        (lo, (0..n).map(|_| self.f()).collect())
    }
    /// The `Bnd_Range` of the next recipe (a constructor failure is remembered in `err`).
    fn range(&mut self) -> BndRange {
        let spec = self.rg();
        mk_range(&spec).unwrap_or_else(|_| {
            self.err = true;
            BndRange::new()
        })
    }
    fn obb_built(&mut self) -> BndObb {
        let spec = self.obb();
        mk_obb(&spec).unwrap_or_else(|_| {
            self.err = true;
            BndObb::new()
        })
    }
    fn a2_built(&mut self) -> VecArray2 {
        let spec = self.a2();
        mk_a2(&spec).unwrap_or_else(|_| {
            self.err = true;
            VecArray2::new(0, 0, 1, 1, vec![GpVec::new(0.0, 0.0, 0.0)]).unwrap()
        })
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread inputs");
    }
}

// ---------------------------------------------------------------------------------
// Replaying one family

fn run(name: &str, r: &mut Rd, g: &mut Got) -> Result<(), Thrown> {
    match name {
        // ---- Bnd_Box ----
        "box_state" => {
            let b = mk_box(&r.bx());
            g.bx(&b);
            let c = b.center();
            g.b(c.is_some());
            if let Some(c) = c {
                g.pnt(&c);
            }
        }
        "box_get" => {
            let b = mk_box(&r.bx());
            g.fs(&b.get()?);
        }
        "box_corners" => {
            let b = mk_box(&r.bx());
            g.pnt(&b.corner_min()?);
            g.pnt(&b.corner_max()?);
        }
        "box_thin" => {
            let (b, tol) = (mk_box(&r.bx()), r.f());
            g.b(b.is_x_thin(tol));
            g.b(b.is_y_thin(tol));
            g.b(b.is_z_thin(tol));
            g.b(b.is_thin(tol));
        }
        "box_gap" => {
            let (mut b, a, c) = (mk_box(&r.bx()), r.f(), r.f());
            b.set_gap(a);
            g.bx(&b);
            b.enlarge(c);
            g.bx(&b);
        }
        "box_modify" => {
            let (mut b, m) = (mk_box(&r.bx()), r.i());
            apply_open_bits(&mut b, m & 63);
            if m & 64 != 0 {
                b.set_whole();
            }
            if m & 128 != 0 {
                b.set_void();
            }
            g.bx(&b);
        }
        "box_update6" => {
            let mut b = mk_box(&r.bx());
            let v: Vec<f64> = (0..6).map(|_| r.f()).collect();
            b.update(v[0], v[1], v[2], v[3], v[4], v[5]);
            g.bx(&b);
        }
        "box_update3" => {
            let mut b = mk_box(&r.bx());
            let v: Vec<f64> = (0..3).map(|_| r.f()).collect();
            b.update_point(v[0], v[1], v[2]);
            g.bx(&b);
        }
        "box_set_pnt" => {
            let (mut b, p) = (mk_box(&r.bx()), r.pnt());
            b.set_point(&p);
            g.bx(&b);
        }
        "box_set_pnt_dir" => {
            let (mut b, p, d) = (mk_box(&r.bx()), r.pnt(), r.dir());
            b.set_point_direction(&p, &d);
            g.bx(&b);
        }
        "box_add_pnt" => {
            let (mut b, p) = (mk_box(&r.bx()), r.pnt());
            b.add_point(&p);
            g.bx(&b);
        }
        "box_add_dir" => {
            let (mut b, d) = (mk_box(&r.bx()), r.dir());
            b.add_direction(&d);
            g.bx(&b);
        }
        "box_add_pnt_dir" => {
            let (mut b, p, d) = (mk_box(&r.bx()), r.pnt(), r.dir());
            b.add_point_direction(&p, &d);
            g.bx(&b);
        }
        "box_add_box" => {
            let (mut b, o) = (mk_box(&r.bx()), mk_box(&r.bx()));
            b.add_box(&o);
            g.bx(&b);
        }
        "box_is_out_pnt" => {
            let (b, p) = (mk_box(&r.bx()), r.pnt());
            g.b(b.is_out_point(&p));
            g.b(b.contains(&p));
        }
        "box_is_out_lin" => {
            let (b, p, d) = (mk_box(&r.bx()), r.pnt(), r.dir());
            g.b(b.is_out_line(&Lin::new(p, d)));
        }
        "box_is_out_pln" => {
            let (b, p, d) = (mk_box(&r.bx()), r.pnt(), r.dir());
            g.b(b.is_out_plane(&Pln::from_point_normal(p, d)));
        }
        "box_is_out_box" => {
            let (b, o) = (mk_box(&r.bx()), mk_box(&r.bx()));
            g.b(b.is_out_box(&o));
            g.b(b.intersects(&o));
        }
        "box_is_out_box_trsf" => {
            let (b, o, t) = (mk_box(&r.bx()), mk_box(&r.bx()), r.trsf()?);
            g.b(b.is_out_box_transformed(&o, &t));
        }
        "box_is_out_trsf_box_trsf" => {
            let b = mk_box(&r.bx());
            let t1 = r.trsf()?;
            let o = mk_box(&r.bx());
            let t2 = r.trsf()?;
            g.b(b.is_out_transformed(&t1, &o, &t2));
        }
        "box_is_out_segment" => {
            let (b, p1, p2, d) = (mk_box(&r.bx()), r.pnt(), r.pnt(), r.dir());
            g.b(b.is_out_segment(&p1, &p2, &d));
        }
        "box_distance" => {
            let (b, o) = (mk_box(&r.bx()), mk_box(&r.bx()));
            g.f(b.distance(&o));
        }
        "box_transformed" => {
            let (b, t) = (mk_box(&r.bx()), r.trsf()?);
            g.bx(&b.transformed(&t));
        }
        "box_finite_part" => {
            let b = mk_box(&r.bx());
            g.bx(&b.finite_part());
        }
        "box_dump" => {
            let b = mk_box(&r.bx());
            g.dump(&b.dump());
        }

        // ---- Bnd_Box2d ----
        "box2d_state" => {
            let b = mk_box2(&r.bx2());
            g.bx2(&b);
            let c = b.center();
            g.b(c.is_some());
            if let Some(c) = c {
                g.pnt2d(&c);
            }
        }
        "box2d_get" => {
            let b = mk_box2(&r.bx2());
            g.fs(&b.get()?);
        }
        "box2d_gap" => {
            let (mut b, a, c) = (mk_box2(&r.bx2()), r.f(), r.f());
            b.set_gap(a);
            g.bx2(&b);
            b.enlarge(c);
            g.bx2(&b);
        }
        "box2d_modify" => {
            let (mut b, m) = (mk_box2(&r.bx2()), r.i());
            apply_open_bits2(&mut b, m & 15);
            if m & 16 != 0 {
                b.set_whole();
            }
            if m & 32 != 0 {
                b.set_void();
            }
            g.bx2(&b);
        }
        "box2d_update4" => {
            let mut b = mk_box2(&r.bx2());
            let v: Vec<f64> = (0..4).map(|_| r.f()).collect();
            b.update(v[0], v[1], v[2], v[3]);
            g.bx2(&b);
        }
        "box2d_update2" => {
            let mut b = mk_box2(&r.bx2());
            let v: Vec<f64> = (0..2).map(|_| r.f()).collect();
            b.update_point(v[0], v[1]);
            g.bx2(&b);
        }
        "box2d_set_pnt" => {
            let (mut b, p) = (mk_box2(&r.bx2()), r.pnt2d());
            b.set_point(&p);
            g.bx2(&b);
        }
        "box2d_set_pnt_dir" => {
            let (mut b, p, d) = (mk_box2(&r.bx2()), r.pnt2d(), r.dir2d());
            b.set_point_direction(&p, &d);
            g.bx2(&b);
        }
        "box2d_add_pnt" => {
            let (mut b, p) = (mk_box2(&r.bx2()), r.pnt2d());
            b.add_point(&p);
            g.bx2(&b);
        }
        "box2d_add_dir" => {
            let (mut b, d) = (mk_box2(&r.bx2()), r.dir2d());
            b.add_direction(&d);
            g.bx2(&b);
        }
        "box2d_add_pnt_dir" => {
            let (mut b, p, d) = (mk_box2(&r.bx2()), r.pnt2d(), r.dir2d());
            b.add_point_direction(&p, &d);
            g.bx2(&b);
        }
        "box2d_add_box" => {
            let (mut b, o) = (mk_box2(&r.bx2()), mk_box2(&r.bx2()));
            b.add_box(&o);
            g.bx2(&b);
        }
        "box2d_is_out_pnt" => {
            let (b, p) = (mk_box2(&r.bx2()), r.pnt2d());
            g.b(b.is_out_point(&p));
            g.b(b.contains(&p));
        }
        "box2d_is_out_lin" => {
            let (b, p, d) = (mk_box2(&r.bx2()), r.pnt2d(), r.dir2d());
            g.b(b.is_out_line(&Lin2d::new(p, d)));
        }
        "box2d_is_out_segment" => {
            let (b, p0, p1) = (mk_box2(&r.bx2()), r.pnt2d(), r.pnt2d());
            g.b(b.is_out_segment(&p0, &p1));
        }
        "box2d_is_out_box" => {
            let (b, o) = (mk_box2(&r.bx2()), mk_box2(&r.bx2()));
            g.b(b.is_out_box(&o));
            g.b(b.intersects(&o));
        }
        "box2d_is_out_box_trsf" => {
            let (b, o, t) = (mk_box2(&r.bx2()), mk_box2(&r.bx2()), r.trsf2d()?);
            g.b(b.is_out_box_transformed(&o, &t));
        }
        "box2d_is_out_trsf_box_trsf" => {
            let b = mk_box2(&r.bx2());
            let t1 = r.trsf2d()?;
            let o = mk_box2(&r.bx2());
            let t2 = r.trsf2d()?;
            g.b(b.is_out_transformed(&t1, &o, &t2));
        }
        "box2d_distance" => {
            let (b, o) = (mk_box2(&r.bx2()), mk_box2(&r.bx2()));
            g.f(b.distance(&o));
        }
        "box2d_transformed" => {
            let (b, t) = (mk_box2(&r.bx2()), r.trsf2d()?);
            g.bx2(&b.transformed(&t));
        }
        "box2d_dump" => {
            let b = mk_box2(&r.bx2());
            g.dump(&b.dump());
        }

        // ---- Bnd_Range ----
        "range_state" => {
            let rg = r.range();
            g.range(&rg);
            g.opt_f(rg.get_min());
            g.opt_f(rg.get_max());
            let bounds = rg.get_bounds();
            g.b(bounds.is_some());
            if let Some((a, c)) = bounds {
                g.fs(&[a, c]);
            }
            g.opt_f(rg.center());
            g.f(rg.delta());
            g.opt_f(rg.min());
            g.opt_f(rg.max());
        }
        "range_intermediate" => {
            let (rg, l) = (r.range(), r.f());
            g.opt_f(rg.get_intermediate_point(l));
        }
        "range_add_param" => {
            let (mut rg, v) = (r.range(), r.f());
            rg.add_parameter(v);
            g.range(&rg);
        }
        "range_add_range" => {
            let (mut rg, o) = (r.range(), r.range());
            rg.add_range(&o);
            g.range(&rg);
        }
        "range_common" => {
            let (mut rg, o) = (r.range(), r.range());
            rg.common(&o);
            g.range(&rg);
        }
        "range_union" => {
            let (mut rg, o) = (r.range(), r.range());
            let u = rg.union(&o);
            g.b(u);
            g.range(&rg);
        }
        "range_enlarge" => {
            let (mut rg, d) = (r.range(), r.f());
            rg.enlarge(d);
            g.range(&rg);
        }
        "range_shift" => {
            let (mut rg, d) = (r.range(), r.f());
            g.range(&rg.shifted(d));
            rg.shift(d);
            g.range(&rg);
        }
        "range_trim" => {
            let (mut rg, lo, hi) = (r.range(), r.f(), r.f());
            let mut q = rg.clone();
            rg.trim_from(lo);
            g.range(&rg);
            q.trim_to(hi);
            g.range(&q);
        }
        "range_set_void" => {
            let mut rg = r.range();
            rg.set_void();
            g.range(&rg);
        }
        "range_is_out" => {
            let (rg, v) = (r.range(), r.f());
            g.b(rg.is_out_value(v));
            g.b(rg.contains(v));
        }
        "range_is_out_range" => {
            let (rg, q) = (r.range(), r.range());
            g.b(rg.is_out_range(&q));
            g.b(rg.intersects(&q));
            g.b(rg == q);
        }
        "range_is_intersected" => {
            let (rg, v, per) = (r.range(), r.f(), r.f());
            g.i(rg.is_intersected(v, per) as i32);
        }
        "range_split" => {
            let (rg, v, per) = (r.range(), r.f(), r.f());
            let parts = rg.split(v, per);
            g.i(parts.len() as i32);
            for p in &parts {
                g.range(p);
            }
        }

        // ---- Bnd_Sphere ----
        "sphere_default" => g.sphere(&BndSphere::new()),
        "sphere_state" => g.sphere(&mk_sphere(&r.sph())),
        "sphere_set_valid" => {
            let (mut s, v) = (mk_sphere(&r.sph()), r.i());
            s.set_valid(v != 0);
            g.sphere(&s);
        }
        "sphere_distances" => {
            let (s, p) = (mk_sphere(&r.sph()), r.xyz());
            let (mn, mx) = s.distances(&p);
            g.fs(&[mn, mx]);
            let (mn, mx) = s.square_distances(&p);
            g.fs(&[mn, mx]);
            g.f(s.distance(&p));
            g.f(s.square_distance(&p));
            let (ok, proj, dist, inside) = s.project(&p);
            g.b(ok);
            g.xyz(&proj);
            g.f(dist);
            g.b(inside);
        }
        "sphere_add" => {
            let (mut s, o) = (mk_sphere(&r.sph()), mk_sphere(&r.sph()));
            s.add(&o);
            g.sphere(&s);
        }
        "sphere_is_out_sphere" => {
            let (s, o) = (mk_sphere(&r.sph()), mk_sphere(&r.sph()));
            g.b(s.is_out_sphere(&o));
        }
        "sphere_is_out_pnt" => {
            let (s, p, md) = (mk_sphere(&r.sph()), r.xyz(), r.f());
            let (o, m) = s.is_out_point(&p, md);
            g.b(o);
            g.f(m);
        }

        // ---- Bnd_OBB ----
        "obb_state" => g.obb(&r.obb_built()),
        "obb_from_box" => g.obb(&BndObb::from_box(&mk_box(&r.bx()))),
        "obb_ctor" => {
            let c = r.pnt();
            let (x, y, z) = (r.dir(), r.dir(), r.dir());
            let (hx, hy, hz) = (r.f(), r.f(), r.f());
            g.obb(&BndObb::from_axes(&c, &x, &y, &z, hx, hy, hz));
        }
        "obb_setters" => {
            let mut o = r.obb_built();
            let c = r.pnt();
            let (dx, hx) = (r.dir(), r.f());
            let (dy, hy) = (r.dir(), r.f());
            let (dz, hz) = (r.dir(), r.f());
            o.set_center(&c);
            g.obb(&o);
            o.set_x_component(&dx, hx);
            g.obb(&o);
            o.set_y_component(&dy, hy);
            o.set_z_component(&dz, hz);
            g.obb(&o);
        }
        "obb_enlarge" => {
            let (mut o, gap) = (r.obb_built(), r.f());
            o.enlarge(gap);
            g.obb(&o);
        }
        "obb_void_aabox" => {
            let (mut o, m) = (r.obb_built(), r.i());
            if m == 0 {
                o.set_void();
            } else {
                o.set_aabox(m == 2);
            }
            g.obb(&o);
        }
        "obb_vertices" => {
            let o = r.obb_built();
            let v = o.vertices();
            g.b(v.is_some());
            if let Some(v) = v {
                for p in &v {
                    g.pnt(p);
                }
            }
        }
        "obb_position" => {
            let o = r.obb_built();
            let a = o.position()?;
            g.pnt(a.location());
            g.dir(a.direction());
            g.dir(a.x_direction());
            g.dir(a.y_direction());
        }
        "obb_is_out_pnt" => {
            let (o, p) = (r.obb_built(), r.pnt());
            g.b(o.is_out_point(&p));
            g.b(o.contains(&p));
        }
        "obb_is_out_obb" => {
            let (o, q) = (r.obb_built(), r.obb_built());
            g.b(o.is_out_obb(&q));
            g.b(o.intersects(&q));
        }
        "obb_completely_inside" => {
            let (o, q) = (r.obb_built(), r.obb_built());
            g.b(o.is_completely_inside(&q));
        }
        "obb_add_pnt" => {
            let (mut o, p) = (r.obb_built(), r.pnt());
            o.add_point(&p);
            g.obb(&o);
        }
        "obb_add_obb" => {
            let (mut o, q) = (r.obb_built(), r.obb_built());
            o.add_obb(&q);
            g.obb(&o);
        }
        "obb_rebuild" | "obb_rebuild_special" | "obb_rebuild_optimal" | "obb_rebuild_optimal_special" => {
            let pts = r.pts();
            let tols = r.tols();
            let optimal = r.i() != 0;
            let mut o = BndObb::new();
            o.rebuild(&pts, tols.as_deref(), optimal)?;
            g.obb(&o);
        }

        // ---- Bnd_BoundSortBox ----
        "bsb_compare_box" | "bsb_compare_box_gen" | "bsb_add" => {
            let spec = r.bsb();
            let queries: Vec<BndBox> = (0..3).map(|_| mk_box(&r.bx())).collect();
            let mut b = mk_bsb(&spec)?;
            for q in &queries {
                g.list(&b.compare_box(q));
            }
        }
        "bsb_compare_plane" | "bsb_compare_plane_gen" => {
            let spec = r.bsb();
            let mut planes = Vec::new();
            for _ in 0..3 {
                let p = r.pnt();
                let d = r.dir();
                planes.push(Pln::from_point_normal(p, d));
            }
            let mut b = mk_bsb(&spec)?;
            for p in &planes {
                g.list(&b.compare_plane(p));
            }
        }

        // ---- CSLib::Normal and the derivative helpers ----
        "cslib_enum_values" => {
            use cslib::{DerivativeStatus as D, NormalStatus as N};
            for v in [
                N::Singular as i32,
                N::Defined as i32,
                N::InfinityOfSolutions as i32,
                N::D1NuIsNull as i32,
                N::D1NvIsNull as i32,
                N::D1NIsNull as i32,
                N::D1NuNvRatioIsNull as i32,
                N::D1NvNuRatioIsNull as i32,
                N::D1NuIsParallelD1Nv as i32,
                D::Done as i32,
                D::D1uIsNull as i32,
                D::D1vIsNull as i32,
                D::D1IsNull as i32,
                D::D1uD1vRatioIsNull as i32,
                D::D1vD1uRatioIsNull as i32,
                D::D1uIsParallelD1v as i32,
                cslib::Class2dResult::Inside as i32,
                cslib::Class2dResult::Outside as i32,
                cslib::Class2dResult::Uncertain as i32,
            ] {
                g.i(v);
            }
        }
        "cslib_normal_sin_tol" => {
            let (u, v, t) = (r.vec(), r.vec(), r.f());
            let (st, d) = cslib::normal_sin_tol(&u, &v, t)?;
            g.i(st as i32);
            g.written(&d);
        }
        "cslib_normal_mag_tol" => {
            let (u, v, t) = (r.vec(), r.vec(), r.f());
            let (st, d) = cslib::normal_mag_tol(&u, &v, t)?;
            g.i(st as i32);
            g.written(&d);
        }
        "cslib_normal_second_order" => {
            let (d1u, d1v, d2u, d2v, d2uv, t) = (r.vec(), r.vec(), r.vec(), r.vec(), r.vec(), r.f());
            let (done, st, d) = cslib::normal_second_order(&d1u, &d1v, &d2u, &d2v, &d2uv, t)?;
            g.b(done);
            g.i(st as i32);
            g.written(&d);
        }
        "cslib_normal_order" => {
            let order = r.i();
            let a = r.a2_built();
            let (mag, u, v, umin, umax, vmin, vmax) = (r.f(), r.f(), r.f(), r.f(), r.f(), r.f(), r.f());
            let res = cslib::normal_of_order(order, &a, mag, u, v, umin, umax, vmin, vmax)?;
            g.i(res.status as i32);
            g.written(&res.normal);
            g.i(res.order_u);
            g.i(res.order_v);
        }
        "cslib_dnnuv" => {
            let (nu, nv) = (r.i(), r.i());
            let a = r.a2_built();
            g.vec(&cslib::dn_nuv(nu, nv, &a)?);
        }
        "cslib_dnnuv_two" => {
            let (nu, nv) = (r.i(), r.i());
            let a = r.a2_built();
            let b = r.a2_built();
            g.vec(&cslib::dn_nuv_two(nu, nv, &a, &b)?);
        }
        "cslib_dnnormal" => {
            let (nu, nv) = (r.i(), r.i());
            let a = r.a2_built();
            let (iu, iv) = (r.i(), r.i());
            g.vec(&cslib::dn_normal(nu, nv, &a, iu, iv)?);
        }

        // ---- CSLib_Class2d and CSLib_NormalPolyDef ----
        "class2d_default" => {
            let (p, t) = (r.pnt2d(), r.f());
            let c = Class2d::new();
            g.i(c.si_dans(&p) as i32);
            g.i(c.si_dans_on_mode(&p, t) as i32);
        }
        "class2d_si_dans" => {
            let c = mk_class2d(&r.c2d());
            let q: Vec<Pnt2d> = (0..4).map(|_| r.pnt2d()).collect();
            for p in &q {
                g.i(c.si_dans(p) as i32);
            }
        }
        "class2d_si_dans_on_mode" => {
            let c = mk_class2d(&r.c2d());
            let t = r.f();
            let q: Vec<Pnt2d> = (0..4).map(|_| r.pnt2d()).collect();
            for p in &q {
                g.i(c.si_dans_on_mode(p, t) as i32);
            }
        }
        "class2d_internal" => {
            let c = mk_class2d(&r.c2d());
            let q: Vec<Pnt2d> = (0..4).map(|_| r.pnt2d()).collect();
            for p in &q {
                g.b(c.internal_si_dans(p.x(), p.y())?);
                g.i(c.internal_si_dans_ou_on(p.x(), p.y())? as i32);
            }
        }
        "cslib_normal_poly_def" => {
            let k0 = r.i();
            let (lo, li) = r.la();
            let x = r.f();
            let mut f = NormalPolyDef::new(k0, lo, &li)?;
            let v = f.value(x);
            let d = f.derivative(x);
            let vd = f.values(x);
            g.b(v.is_some());
            g.f(v.unwrap_or(11.5));
            g.b(d.is_some());
            g.f(d.unwrap_or(12.5));
            g.b(vd.is_some());
            let (v2, d2) = vd.unwrap_or((13.5, 14.5));
            g.fs(&[v2, d2]);
        }
        _ => panic!("unknown oracle family {name}"),
    }
    if r.err { Err(Thrown) } else { Ok(()) }
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

/// Compares the expected words of a line with the replayed values. Floats are compared bit for bit for `EXACT`
/// families and within `MAX_ULPS` of the line's largest magnitude otherwise; bools, ints and texts always exactly.
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
            Tok::I(g) => assert_eq!(w.parse::<i32>().expect("integer output"), *g, "value {index} (int)\n{line}"),
            Tok::S(g) => assert_eq!(*w, g.as_str(), "value {index} (text)\n{line}"),
        }
    }
}

// ---------------------------------------------------------------------------------

#[test]
fn bnd_matches_occt() {
    let text = include_str!("bnd.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let (mut n_throws, mut n_unchecked) = (0usize, 0usize);

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in bnd.txt:\n{line}");
        let (head, tail) = line.split_once(" | ").unwrap_or_else(|| panic!("line without ' | ': {line}"));
        let head: Vec<&str> = head.split_whitespace().collect();
        let want: Vec<&str> = tail.split_whitespace().collect();
        let (name, inputs) = head.split_first().expect("family name");
        let name: &str = name;
        let entry = stats.entry(name).or_default();
        entry.0 += 1;
        entry.1 += usize::from(!inputs.is_empty());

        let mut reader = Rd { words: inputs, at: 0, last_n: 0, err: false };
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
        if marker {
            // the whole input must still be readable (the reader reads before it calls)
            n_throws += usize::from(want[0] == "throws");
            n_unchecked += usize::from(want[0] == "unchecked");
        }
    }

    // Input coverage: no duplicates (checked above), every family with inputs has at least MIN_DISTINCT distinct
    // lines, and only the listed families are input-less (printed once).
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
    println!(
        "{lines} lines ({n_throws} throws, {n_unchecked} unchecked), {} values, {} bit-identical, worst {} ulps",
        tally.values, tally.exact, tally.worst
    );
}
