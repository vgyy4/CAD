//! Numerical reference test for the in-scope parts of OCCT 8.0.1's `TopLoc` and `Poly` packages.
//!
//! `poly.txt` is printed by `oracle/poly.cpp` (real OCCT). Each line is
//! `<family> <inputs...> | <outputs...>`: f64 values are 16-hex-digit bit patterns, ints are decimal and bools are
//! 0/1. Every input of a call is on the left of ` | `; this test reads them back with the `Rd` reader methods of the
//! same name as the oracle's schema words (`f i xyz pnt dir xy bx trsf lp lprog items tri tops trilist pg3 pg2 pot txt
//! pts2d mcfg mops loops mapops cops`, grammars in docs/api-poly.md) and replays the call on the Rust API of
//! docs/api-poly.md. The right side is the expected values, `throws` (OCCT raised an exception) or `unchecked` (OCCT
//! 8.0.1 as built has that check compiled out, or the result is unspecified; the Rust API must return an error).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::bnd::BndBox;
use occt_port::gp::{self, Ax1, Ax2, Dir, Dir2d, Pnt, Pnt2d, Quaternion, Trsf, Vec as GpVec, XY, XYZ};
use occt_port::mathlin::MathError;
use occt_port::poly::{
    self, CoherentLink, CoherentNode, CoherentTriangle, CoherentTriangulation, Connect, HeapOfInteger, Link,
    MakeLoops2D, MakeLoops2DHelper, MakeLoops3D, MakeLoops3DHelper, MakeLoopsHelper, MergeNodesTool,
    PackedMapOfInteger, Polygon2D, Polygon3D, PolygonOnTriangulation, Triangle, Triangulation,
};
use occt_port::toploc::{Datum3D, ItemLocation, Location, SListOfItemLocation};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::rc::Rc;

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for the families that use sin/cos/atan2 through the C library (`merge_tool`,
/// `merge_static`, `merge_element_errors`, `make_loops`), in units of f64::EPSILON times the largest absolute value on
/// the line (at least 1; zero tolerance if that value is infinite). The oracle only prints cases whose outputs do not
/// change at all when every such result is moved by an ulp.
const MAX_ULPS: f64 = 8.0;

/// Every family with inputs must have at least this many distinct lines.
const MIN_DISTINCT: usize = 16;

// BEGIN GENERATED
/// Families that really take no input: they are printed exactly once.
const INPUT_LESS: &[&str] = &[];

/// Families compared bit for bit: only + - * / sqrt, comparisons and integers (no C library function). The others
/// (`merge_tool`, `merge_static`, `merge_element_errors`, `make_loops`) depend on cos / atan2 and use MAX_ULPS.
const EXACT: &[&str] = &[
    "coherent_basic",
    "coherent_clone",
    "coherent_empty",
    "coherent_program",
    "connect_adjacency",
    "connect_around",
    "connect_checks",
    "connect_reload",
    "datum3d",
    "heap_of_integer",
    "item_loc",
    "loc_div",
    "loc_dump",
    "loc_equal",
    "loc_first",
    "loc_from_trsf",
    "loc_inverted",
    "loc_laws",
    "loc_mul",
    "loc_powered",
    "loc_predivided",
    "loc_state",
    "merge_params",
    "mesh_bad_index",
    "packed_map",
    "pg2_state",
    "pg3_params_none",
    "pg3_state",
    "poly_catenate",
    "poly_intersect",
    "poly_point_on_triangle",
    "poly_polygon_properties",
    "poly_read_pg2",
    "poly_read_pg3",
    "poly_read_tri",
    "poly_tri_line",
    "poly_write_pg2",
    "poly_write_pg3",
    "poly_write_tri",
    "pot_ops",
    "pot_state",
    "slist_build",
    "tri_access",
    "tri_copy",
    "tri_ctor_invalid",
    "tri_minmax",
    "tri_modify",
    "tri_normals",
    "tri_state",
];

/// Every family the oracle must print (a family that disappears from poly.txt fails).
const FAMILIES: &[&str] = &[
    "coherent_basic",
    "coherent_clone",
    "coherent_empty",
    "coherent_program",
    "connect_adjacency",
    "connect_around",
    "connect_checks",
    "connect_reload",
    "datum3d",
    "heap_of_integer",
    "item_loc",
    "loc_div",
    "loc_dump",
    "loc_equal",
    "loc_first",
    "loc_from_trsf",
    "loc_inverted",
    "loc_laws",
    "loc_mul",
    "loc_powered",
    "loc_predivided",
    "loc_state",
    "make_loops",
    "merge_element_errors",
    "merge_params",
    "merge_static",
    "merge_tool",
    "mesh_bad_index",
    "packed_map",
    "pg2_state",
    "pg3_params_none",
    "pg3_state",
    "poly_catenate",
    "poly_intersect",
    "poly_point_on_triangle",
    "poly_polygon_properties",
    "poly_read_pg2",
    "poly_read_pg3",
    "poly_read_tri",
    "poly_tri_line",
    "poly_write_pg2",
    "poly_write_pg3",
    "poly_write_tri",
    "pot_ops",
    "pot_state",
    "slist_build",
    "tri_access",
    "tri_copy",
    "tri_ctor_invalid",
    "tri_minmax",
    "tri_modify",
    "tri_normals",
    "tri_state",
];

/// The number of lines of tests/poly.txt.
const GOLDEN_LINES: usize = 1512;
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
    I(i64),
    /// A text (a `Dump()` or `Write()` output), spaces as `_` and newlines as `\n`.
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
    /// A `float` of the C++ side, printed as the `double` it widens to.
    fn f32(&mut self, v: f32) {
        self.0.push(Tok::F(f64::from(v)));
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
    fn text(&mut self, text: &str) {
        self.0.push(Tok::S(escape(text)));
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
    fn f3(&mut self, v: [f32; 3]) {
        for x in v {
            self.f32(x);
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
        self.fs(&[l.xmin, l.xmax, l.ymin, l.ymax, l.zmin, l.zmax]);
        self.f(b.square_extent());
        self.b(b.has_finite_part());
    }
    /// `put_trsf` of the oracle: the 3 x 4 matrix row by row, the scale factor, the form and the sign.
    fn trsf(&mut self, t: &Trsf) -> Result<(), Thrown> {
        for r in 1..=3 {
            for c in 1..=4 {
                self.f(t.value(r, c)?);
            }
        }
        self.f(t.scale_factor());
        self.i(t.form() as i32);
        self.b(t.is_negative());
        Ok(())
    }
    /// `put_tri_state` of the oracle.
    fn tri_state(&mut self, t: &Triangulation) -> Result<(), Thrown> {
        let (nn, nt) = (t.nb_nodes(), t.nb_triangles());
        self.i(nn);
        self.i(nt);
        self.b(t.has_uv_nodes());
        self.b(t.has_normals());
        self.b(t.has_geometry());
        self.b(t.is_double_precision());
        self.f(t.deflection());
        self.i(t.mesh_purpose() as i32);
        self.b(t.has_cached_min_max());
        for i in 1..=nn {
            self.pnt(&t.node(i)?);
        }
        if t.has_uv_nodes() {
            for i in 1..=nn {
                self.pnt2d(&t.uv_node(i)?);
            }
        }
        if t.has_normals() {
            for i in 1..=nn {
                self.f3(t.normal(i)?);
            }
        }
        let tl = if nt > 0 { t.triangle_lower() } else { 1 };
        self.i(tl);
        for i in tl..tl + nt {
            let (a, b, c) = t.triangle(i)?.get();
            self.i(a);
            self.i(b);
            self.i(c);
        }
        Ok(())
    }
    /// `put_pg3` of the oracle.
    fn pg3(&mut self, p: &Polygon3D) -> Result<(), Thrown> {
        self.i(p.nb_nodes());
        self.b(p.has_parameters());
        self.f(p.deflection());
        // the node array of a polygon always has the lower bound 1
        self.i(1);
        self.i(p.nb_nodes());
        for q in p.nodes() {
            self.pnt(q);
        }
        if p.has_parameters() {
            let par = p.parameters()?;
            self.i(1);
            self.l(par.len());
            self.fs(par);
        }
        Ok(())
    }
    /// `put_pg2` of the oracle.
    fn pg2(&mut self, p: &Polygon2D) {
        self.i(p.nb_nodes());
        self.f(p.deflection());
        self.i(1);
        self.i(p.nb_nodes());
        for q in p.nodes() {
            self.pnt2d(q);
        }
    }
    /// `put_pot` of the oracle.
    fn pot(&mut self, p: &PolygonOnTriangulation) -> Result<(), Thrown> {
        self.i(p.nb_nodes());
        self.b(p.has_parameters());
        self.f(p.deflection());
        self.i(p.nodes_lower());
        self.i(p.nodes_upper());
        for i in p.nodes_lower()..=p.nodes_upper() {
            self.i(p.node(i)?);
        }
        if let Some((lo, up)) = p.parameters_bounds() {
            self.i(lo);
            self.i(up);
            for i in lo..=up {
                self.f(p.parameter(i)?);
            }
        }
        Ok(())
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
    let f = s.flags;
    if f & 1 != 0 {
        b.open_xmin();
    }
    if f & 2 != 0 {
        b.open_xmax();
    }
    if f & 4 != 0 {
        b.open_ymin();
    }
    if f & 8 != 0 {
        b.open_ymax();
    }
    if f & 16 != 0 {
        b.open_zmin();
    }
    if f & 32 != 0 {
        b.open_zmax();
    }
    b
}

/// A triangulation recipe, see docs/api-poly.md ("tri").
struct TriSpec {
    kind: i32,
    lo: i32,
    uv: bool,
    nrm: bool,
    defl: f64,
    purpose: u32,
    p: Vec<XYZ>,
    uvs: Vec<XY>,
    t: Vec<[i32; 3]>,
    n: Vec<[f32; 3]>,
}

fn mk_tri(s: &TriSpec) -> Result<Option<Triangulation>, Thrown> {
    if s.kind < 0 {
        return Ok(None);
    }
    let (nn, nt) = (s.p.len() as i32, s.t.len() as i32);
    let nodes: Vec<Pnt> = s.p.iter().map(|q| Pnt::from_xyz(*q)).collect();
    let tris: Vec<Triangle> = s.t.iter().map(|t| Triangle::from_nodes(t[0], t[1], t[2])).collect();
    let mut t = match s.kind {
        0 => Triangulation::with_sizes(nn, nt, s.uv, s.nrm)?,
        1 => {
            let mut t = Triangulation::new();
            t.set_double_precision(false)?;
            t.resize_nodes(nn, false)?;
            t.resize_triangles(nt, false)?;
            if s.uv {
                t.add_uv_nodes();
            }
            if s.nrm {
                t.add_normals();
            }
            t
        }
        _ => {
            let mut t = if s.uv {
                let uvs: Vec<Pnt2d> = s.uvs.iter().map(|q| Pnt2d::new(q.x(), q.y())).collect();
                Triangulation::from_arrays_uv(&nodes, &uvs, &tris, s.lo)?
            } else {
                Triangulation::from_arrays(&nodes, &tris, s.lo)?
            };
            if s.nrm {
                t.add_normals();
            }
            t
        }
    };
    for i in 1..=nn {
        let k = (i - 1) as usize;
        t.set_node(i, &nodes[k])?;
        if s.uv {
            t.set_uv_node(i, &Pnt2d::new(s.uvs[k].x(), s.uvs[k].y()))?;
        }
        if s.nrm {
            t.set_normal(i, s.n[k])?;
        }
    }
    let tl = if s.kind == 2 { s.lo } else { 1 };
    for (k, tr) in tris.iter().enumerate() {
        t.set_triangle(tl + k as i32, *tr)?;
    }
    t.set_deflection(s.defl);
    t.set_mesh_purpose(s.purpose);
    Ok(Some(t))
}

/// The pool of datums of a `lp` word and the registry of the datums created by `T` operations, in creation order.
struct Pool {
    datums: Vec<Rc<Datum3D>>,
    reg: Vec<Rc<Datum3D>>,
}

impl Pool {
    fn index(&self, d: &Rc<Datum3D>) -> i32 {
        self.reg.iter().position(|e| Rc::ptr_eq(e, d)).expect("datum in the registry") as i32
    }
    fn datum(&self, i: i32) -> Result<Rc<Datum3D>, Thrown> {
        self.datums.get(i as usize).cloned().ok_or(Thrown)
    }
}

type Prog = Vec<(i32, i32)>;

/// Evaluates a location program (codes in docs/api-poly.md, "lprog").
fn eval_prog(pool: &mut Pool, prog: &Prog) -> Result<Location, Thrown> {
    let mut st: Vec<Location> = Vec::new();
    for &(code, arg) in prog {
        match code {
            0 => st.push(Location::new()),
            1 => st.push(Location::from_datum(pool.datum(arg)?)?),
            2 => {
                let t = *pool.datum(arg)?.transformation();
                let l = Location::from_trsf(&t)?;
                pool.reg.push(l.first_datum()?.clone());
                st.push(l);
            }
            3..=5 => {
                let b = st.pop().ok_or(Thrown)?;
                let a = st.pop().ok_or(Thrown)?;
                st.push(match code {
                    3 => a.multiplied(&b)?,
                    4 => a.divided(&b)?,
                    _ => a.predivided(&b)?,
                });
            }
            6 => {
                let top = st.pop().ok_or(Thrown)?;
                st.push(top.powered(arg)?);
            }
            7 => {
                let top = st.pop().ok_or(Thrown)?;
                st.push(top.inverted()?);
            }
            _ => {
                let top = st.last().cloned().ok_or(Thrown)?;
                st.push(top);
            }
        }
    }
    st.pop().ok_or(Thrown)
}

/// `put_loc` of the oracle.
fn put_loc(g: &mut Got, pool: &Pool, l: &Location) -> Result<(), Thrown> {
    g.b(l.is_identity());
    let mut n = 0;
    let mut c = l.clone();
    while !c.is_identity() {
        n += 1;
        c = c.next_location();
    }
    g.i(n);
    let mut c = l.clone();
    while !c.is_identity() {
        g.i(pool.index(c.first_datum()?));
        g.i(c.first_power()?);
        c = c.next_location();
    }
    g.trsf(l.transformation())
}

struct PgSpec3 {
    kind: i32,
    nn: i32,
    hasp: bool,
    lo: i32,
    np: i32,
    plo: i32,
    defl: f64,
    p: Vec<Pnt>,
    par: Vec<f64>,
}

/// `pg3_unchecked` of the oracle.
fn pg3_unchecked(s: &PgSpec3) -> bool {
    s.kind == 2 && (s.np != s.nn || s.plo != s.lo)
}

fn mk_pg3(s: &PgSpec3) -> Result<Polygon3D, Thrown> {
    let mut p = match s.kind {
        0 => {
            let mut p = Polygon3D::with_size(s.nn, s.hasp)?;
            for (i, q) in s.p.iter().enumerate() {
                p.change_nodes()[i] = *q;
            }
            if s.hasp {
                for (i, v) in s.par.iter().enumerate() {
                    p.change_parameters()?[i] = *v;
                }
            }
            p
        }
        1 => Polygon3D::from_nodes(&s.p),
        _ => Polygon3D::from_nodes_parameters(&s.p, s.lo, &s.par, s.plo)?,
    };
    p.set_deflection(s.defl);
    Ok(p)
}

struct PgSpec2 {
    kind: i32,
    nn: i32,
    defl: f64,
    p: Vec<Pnt2d>,
}

fn mk_pg2(s: &PgSpec2) -> Result<Polygon2D, Thrown> {
    let mut p = match s.kind {
        0 => {
            let mut p = Polygon2D::with_size(s.nn)?;
            for (i, q) in s.p.iter().enumerate() {
                p.change_nodes()[i] = *q;
            }
            p
        }
        _ => Polygon2D::from_nodes(&s.p),
    };
    p.set_deflection(s.defl);
    Ok(p)
}

struct PotSpec {
    kind: i32,
    nn: i32,
    hasp: bool,
    nlo: i32,
    plo: i32,
    defl: f64,
    n: Vec<i32>,
    par: Vec<f64>,
}

fn mk_pot(s: &PotSpec) -> Result<PolygonOnTriangulation, Thrown> {
    let mut p = match s.kind {
        0 => {
            let mut p = PolygonOnTriangulation::with_size(s.nn, s.hasp)?;
            for (i, v) in s.n.iter().enumerate() {
                p.set_node(1 + i as i32, *v)?;
            }
            if s.hasp {
                for (i, v) in s.par.iter().enumerate() {
                    p.set_parameter(1 + i as i32, *v)?;
                }
            }
            p
        }
        1 => PolygonOnTriangulation::from_nodes(&s.n, s.nlo),
        _ => PolygonOnTriangulation::from_nodes_parameters(&s.n, s.nlo, &s.par, s.plo),
    };
    p.set_deflection(s.defl);
    Ok(p)
}

/// A recipe of the triangulation-modifying operations (`tops`).
struct TOpSpec {
    code: i32,
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    x: f64,
    y: f64,
    z: f64,
}

fn run_tops(t: &mut Triangulation, ops: &[TOpSpec]) -> Result<(), Thrown> {
    for o in ops {
        match o.code {
            0 => t.add_uv_nodes(),
            1 => t.remove_uv_nodes(),
            2 => t.add_normals(),
            3 => t.remove_normals(),
            4 => t.resize_nodes(o.a as i32, o.b != 0)?,
            5 => t.resize_triangles(o.a as i32, o.b != 0)?,
            6 => t.set_double_precision(o.a != 0)?,
            7 => t.clear(),
            8 => t.compute_normals()?,
            9 => t.set_deflection(o.x),
            10 => t.set_mesh_purpose(o.a as u32),
            11 => t.set_node(o.a as i32, &Pnt::new(o.x, o.y, o.z))?,
            12 => t.set_uv_node(o.a as i32, &Pnt2d::new(o.x, o.y))?,
            13 => t.set_triangle(o.a as i32, Triangle::from_nodes(o.b as i32, o.c as i32, o.d as i32))?,
            14 => t.set_normal(o.a as i32, [o.x as f32, o.y as f32, o.z as f32])?,
            _ => *t = t.clone(),
        }
    }
    Ok(())
}

/// Parameters of a `merge_tool` line (`mcfg`).
struct MCfg {
    angle: f64,
    tol: f64,
    unit: f64,
    nbf: i32,
    flags: i32,
}

/// One operation of a `mops` word.
enum MOp {
    Tri([XYZ; 3]),
    Quad([XYZ; 4]),
    Triangulation(Box<TriSpec>, Trsf, bool),
}

fn run_merge(g: &mut Got, c: &MCfg, ops: &[MOp], nbf: i32, per_elem: bool) -> Result<(), Thrown> {
    let mut t = MergeNodesTool::new(c.angle, c.tol, nbf);
    t.set_merge_opposite(c.flags & 1 != 0);
    t.set_drop_degenerative(c.flags & 2 != 0);
    t.set_merge_elems(c.flags & 4 != 0);
    t.set_unit_factor(c.unit);
    g.f(t.merge_tolerance());
    g.f(t.merge_angle());
    g.b(t.to_merge_opposite());
    g.b(t.to_drop_degenerative());
    g.b(t.to_merge_elems());
    if per_elem {
        for k in 0..4 {
            g.i(t.element_node_index(k));
        }
    }
    for o in ops {
        match o {
            MOp::Tri(q) => t.add_triangle(q),
            MOp::Quad(q) => t.add_quad(q),
            MOp::Triangulation(s, tr, rev) => {
                let tri = mk_tri(s)?;
                t.add_triangulation(tri.as_ref(), tr, *rev)?;
            }
        }
        if per_elem {
            for k in 0..4 {
                g.i(t.element_node_index(k));
            }
        }
    }
    g.i(t.nb_nodes());
    g.i(t.nb_elements());
    g.i(t.nb_degenerative_elems());
    g.i(t.nb_merged_elems());
    let r = t.result()?;
    g.b(r.is_some());
    if let Some(r) = r {
        g.tri_state(&r)?;
    }
    Ok(())
}

/// A `loops` recipe (helper tables and the operations on the loop maker).
struct LoopSpec {
    dim: i32,
    left: bool,
    nn: i32,
    pos: Vec<[f64; 3]>,
    hl: Vec<[i32; 3]>,
    nrm: Vec<(bool, Dir)>,
    ops: Vec<(i32, [i32; 6])>,
}

struct Table<'a> {
    s: &'a LoopSpec,
    adj: Vec<Vec<Link>>,
    none: Vec<Link>,
}

impl<'a> Table<'a> {
    fn new(s: &'a LoopSpec) -> Self {
        let mut adj = vec![Vec::new(); s.nn as usize + 1];
        for l in &s.hl {
            let k = Link::new(l[0], l[1]);
            if l[0] >= 1 && l[0] <= s.nn {
                adj[l[0] as usize].push(k);
            }
            if l[1] >= 1 && l[1] <= s.nn && l[1] != l[0] {
                adj[l[1] as usize].push(k);
            }
        }
        Table { s, adj, none: Vec::new() }
    }
    fn tangent(&self, l: &Link, first: bool) -> Option<[f64; 3]> {
        let mut mask = 0;
        for h in &self.s.hl {
            if (h[0] == l.node1 && h[1] == l.node2) || (h[0] == l.node2 && h[1] == l.node1) {
                mask = h[2];
                break;
            }
        }
        if mask & (if first { 1 } else { 2 }) != 0 {
            return None;
        }
        let (a, b) = if l.is_reversed() { (l.node2, l.node1) } else { (l.node1, l.node2) };
        if a < 1 || a > self.s.nn || b < 1 || b > self.s.nn {
            return None;
        }
        let (pa, pb) = (self.s.pos[a as usize - 1], self.s.pos[b as usize - 1]);
        Some([pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]])
    }
}

impl MakeLoopsHelper for Table<'_> {
    fn adjacent_links(&self, node: i32) -> &[Link] {
        if node >= 1 && node <= self.s.nn { &self.adj[node as usize] } else { &self.none }
    }
}

impl MakeLoops3DHelper for Table<'_> {
    fn first_tangent(&self, l: &Link) -> Option<Dir> {
        let v = self.tangent(l, true)?;
        if v == [0.0, 0.0, 0.0] { None } else { Dir::new(v[0], v[1], v[2]).ok() }
    }
    fn last_tangent(&self, l: &Link) -> Option<Dir> {
        let v = self.tangent(l, false)?;
        if v == [0.0, 0.0, 0.0] { None } else { Dir::new(v[0], v[1], v[2]).ok() }
    }
    fn normal(&self, node: i32) -> Option<Dir> {
        if node < 1 || node > self.s.nn {
            return None;
        }
        let (has, d) = &self.s.nrm[node as usize - 1];
        if *has { Some(*d) } else { None }
    }
}

impl MakeLoops2DHelper for Table<'_> {
    fn first_tangent(&self, l: &Link) -> Option<Dir2d> {
        let v = self.tangent(l, true)?;
        if v[0] == 0.0 && v[1] == 0.0 { None } else { Dir2d::new(v[0], v[1]).ok() }
    }
    fn last_tangent(&self, l: &Link) -> Option<Dir2d> {
        let v = self.tangent(l, false)?;
        if v[0] == 0.0 && v[1] == 0.0 { None } else { Dir2d::new(v[0], v[1]).ok() }
    }
}

fn put_link(g: &mut Got, l: &Link) {
    g.i(l.node1);
    g.i(l.node2);
    g.i(l.flags);
}

/// Either kind of loop maker, driven through the same operations.
enum Loops<'a> {
    D3(MakeLoops3D<'a>),
    D2(MakeLoops2D<'a>),
}

macro_rules! both {
    ($s:expr, $m:ident, $($a:expr),*) => {
        match $s {
            Loops::D3(m) => m.$m($($a),*),
            Loops::D2(m) => m.$m($($a),*),
        }
    };
}

fn run_loops(g: &mut Got, s: &LoopSpec) -> Result<(), Thrown> {
    let table = Table::new(s);
    let mut ml =
        if s.dim == 3 { Loops::D3(MakeLoops3D::new(&table)) } else { Loops::D2(MakeLoops2D::new(s.left, &table)) };
    for (code, a) in &s.ops {
        match code {
            0 => {
                let mut l = Link::new(a[0], a[1]);
                l.flags = a[2];
                both!(&mut ml, add_link, &l);
            }
            1 => {
                let l = Link::new(a[0], a[1]);
                let mut m = Link::new(a[3], a[4]);
                m.flags = a[5];
                both!(&mut ml, replace_link, &l, &m);
            }
            2 => {
                let o = both!(&mut ml, set_link_orientation, &Link::new(a[0], a[1]), a[2]);
                g.i(o);
            }
            3 => {
                let l = both!(&mut ml, find_link, &Link::new(a[0], a[1]));
                put_link(g, &l);
            }
            _ => {
                g.i(both!(&mut ml, perform,));
                let nl = both!(&ml, nb_loops,);
                g.i(nl);
                for i in 1..=nl {
                    let lp = both!(&ml, loop_links, i)?;
                    g.l(lp.len());
                    for l in lp {
                        put_link(g, l);
                    }
                }
                g.i(both!(&ml, nb_hanging,));
                let hang = both!(&ml, hanging_links,);
                g.l(hang.len());
                for l in &hang {
                    put_link(g, l);
                }
            }
        }
    }
    Ok(())
}

/// Same as `loops_unchecked` of the oracle: a replacement that gives two entries of the link map the same key.
fn loops_unchecked(s: &LoopSpec) -> bool {
    let mut keys: Vec<(i32, i32)> = Vec::new();
    let find = |keys: &Vec<(i32, i32)>, a: i32, b: i32| {
        keys.iter().position(|k| (k.0 == a && k.1 == b) || (k.0 == b && k.1 == a))
    };
    for (code, a) in &s.ops {
        if *code == 0 {
            if a[0] != a[1] && find(&keys, a[0], a[1]).is_none() {
                keys.push((a[0], a[1]));
            }
        } else if *code == 1 {
            if a[3] == a[4] {
                continue;
            }
            let i = match find(&keys, a[0], a[1]) {
                Some(i) => i,
                None => {
                    keys.push((a[0], a[1]));
                    keys.len() - 1
                }
            };
            if let Some(j) = find(&keys, a[3], a[4])
                && j != i
            {
                return true;
            }
            keys[i] = (a[3], a[4]);
        }
    }
    false
}

/// One operation of a `cops` word (see the oracle for the codes).
struct COpSpec {
    code: i32,
    x: f64,
    y: f64,
    z: f64,
    a: i64,
    b: i64,
    c: i64,
    d: i64,
}

fn tri_slot(t: Option<usize>) -> i32 {
    t.map_or(-1, |v| v as i32)
}

/// `put_coh` of the oracle.
fn put_coh(g: &mut Got, c: &CoherentTriangulation) -> Result<(), Thrown> {
    g.i(c.max_node());
    g.i(c.max_triangle());
    g.i(c.n_nodes());
    g.i(c.n_triangles());
    g.i(c.n_links());
    g.l(c.nb_link_slots());
    g.f(c.deflection());
    for i in 0..=c.max_node() {
        let n = c.node(i)?;
        g.f(n.x());
        g.f(n.y());
        g.f(n.z());
        g.b(n.is_free_node());
        g.i(n.get_index());
        g.f(n.get_u());
        g.f(n.get_v());
        g.b(n.has_normal());
        g.xyz(&n.get_normal());
        if !n.is_free_node() {
            let ring = n.triangles();
            g.l(ring.len());
            for r in ring {
                g.i(r as i32);
            }
        }
    }
    for i in 0..=c.max_triangle() {
        let t = c.triangle(i)?;
        g.b(t.is_empty());
        for k in 0..3 {
            g.i(t.node(k));
        }
        g.i(t.n_connections());
        for k in 0..3 {
            g.i(t.connected_node(k));
        }
        for k in 0..3 {
            g.i(tri_slot(t.connected_triangle(k)));
        }
        for k in 0..3 {
            g.i(tri_slot(t.link(k)));
        }
    }
    for i in 0..c.nb_link_slots() {
        let l = c.link(i)?;
        g.b(l.is_empty());
        g.i(l.node(0));
        g.i(l.node(1));
        g.i(l.opposite_node(0));
        g.i(l.opposite_node(1));
        g.b(true);
        if !l.is_empty() {
            let (r, t0, t1) = c.find_triangle(i)?;
            g.b(r);
            g.i(tri_slot(t0));
            g.i(tri_slot(t1));
        }
    }
    let (r, fr) = c.get_free_nodes();
    g.b(r);
    g.l(fr.len());
    for v in fr {
        g.i(v);
    }
    g.text(&c.dump());
    let t = c.get_triangulation()?;
    g.b(t.is_some());
    if let Some(t) = t {
        g.tri_state(&t)?;
    }
    Ok(())
}

fn run_cops(g: &mut Got, c: &mut CoherentTriangulation, ops: &[COpSpec]) -> Result<(), Thrown> {
    for o in ops {
        let n = i64::from(c.max_node() + 1);
        let nt = i64::from(c.max_triangle() + 1);
        match o.code {
            0 => {
                let p = XYZ::new(o.x, o.y, o.z);
                let r = if o.a == 1 {
                    let fr = (0..n as i32).find(|&i| c.node(i).map(|nd| nd.is_free_node()).unwrap_or(false));
                    match fr {
                        Some(i) => c.set_node(&p, i)?,
                        None => c.set_node(&p, -1)?,
                    }
                } else if o.a == 2 {
                    c.set_node(&p, n as i32)?
                } else {
                    c.set_node(&p, -1)?
                };
                g.i(r);
            }
            1 => {
                if n == 0 {
                    g.i(-1);
                    continue;
                }
                let r = c.add_triangle((o.a % n) as i32, (o.b % n) as i32, (o.c % n) as i32)?;
                g.i(tri_slot(r));
            }
            2 => {
                if nt == 0 {
                    g.i(-1);
                    continue;
                }
                let r = c.remove_triangle((o.a % nt) as usize)?;
                g.b(r);
            }
            3 => {
                if nt == 0 || n == 0 {
                    g.i(-1);
                    continue;
                }
                let v = [o.b, o.c, o.d].map(|x| if x < 0 { -1 } else { (x % n) as i32 });
                let r = c.replace_nodes((o.a % nt) as usize, v[0], v[1], v[2])?;
                g.b(r);
            }
            4 => {
                if c.nb_link_slots() != 0 {
                    g.i(-1);
                    continue;
                }
                g.i(c.compute_links()?);
            }
            5 => c.clear_links(),
            6 => {
                let (r, lst) = c.remove_degenerated(o.x, o.a != 0)?;
                g.b(r);
                g.l(lst.len());
                for (a, b) in lst {
                    g.i(a);
                    g.i(b);
                }
            }
            7 => {
                if nt == 0 {
                    g.i(-1);
                    continue;
                }
                let r = c.add_link((o.a % nt) as usize, (o.b % 3) as i32)?;
                g.i(tri_slot(r));
            }
            8 => {
                let ne: Vec<usize> =
                    (0..c.nb_link_slots()).filter(|&i| c.link(i).map(|l| !l.is_empty()).unwrap_or(false)).collect();
                if ne.is_empty() {
                    g.i(-1);
                    continue;
                }
                let s = ne[(o.a % ne.len() as i64) as usize];
                c.remove_link(s)?;
                g.i(s as i32);
            }
            9 => {
                if n > 0 {
                    c.change_node((o.a % n) as i32)?.set_uv(o.x, o.y);
                }
            }
            10 => {
                if n > 0 {
                    c.change_node((o.a % n) as i32)?.set_normal(&XYZ::new(o.x, o.y, o.z));
                }
            }
            11 => {
                if n > 0 {
                    c.change_node((o.a % n) as i32)?.set_index(o.b as i32);
                }
            }
            _ => c.set_deflection(o.x),
        }
    }
    Ok(())
}

fn bits(w: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(w, 16).unwrap_or_else(|e| panic!("bad hex word {w}: {e}")))
}

struct Rd<'a> {
    words: &'a [&'a str],
    at: usize,
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
        Pnt::from_xyz(self.xyz())
    }
    fn xy(&mut self) -> XY {
        let x = self.f();
        let y = self.f();
        XY::new(x, y)
    }
    /// A raw (un-normalized) direction; `Dir::new` normalizes it exactly like the oracle did. A failure is remembered in
    /// `err` (the line then must be a `throws` / `unchecked` one) and the rest of the line is still read.
    fn dir(&mut self) -> Dir {
        let x = self.f();
        let y = self.f();
        let z = self.f();
        Dir::new(x, y, z).unwrap_or_else(|_| {
            self.err = true;
            Dir::new(1.0, 0.0, 0.0).unwrap()
        })
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
    /// The fields of a 3D transformation recipe: kind, then the fields that kind uses (header of oracle/bnd.cpp).
    fn trsf_body(&mut self) -> Trsf {
        let kind = self.i();
        let mut t = Trsf::new();
        let mut ok = true;
        match kind {
            2 => {
                let p = self.pnt();
                let factor = self.f();
                ok = t.set_scale(&p, factor).is_ok();
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
                ok = t.set_rotation_quaternion(&Quaternion::from_components(x, y, z, w)).is_ok();
                if kind == 8 {
                    let v = self.xyz();
                    let factor = self.f();
                    t.set_translation_part(&GpVec::from_xyz(v));
                    ok &= t.set_scale_factor(factor).is_ok();
                }
            }
            9 => {}
            _ => panic!("unknown 3D transformation recipe {kind}"),
        }
        if !ok {
            self.err = true;
        }
        t
    }
    fn trsf(&mut self) -> Trsf {
        self.trsf_body()
    }
    /// nd, then nd transformation recipes: the datums of a location pool.
    fn lp(&mut self) -> Pool {
        let nd = self.i();
        let mut datums = Vec::new();
        for _ in 0..nd {
            let t = self.trsf_body();
            datums.push(Rc::new(Datum3D::from_trsf(&t)));
        }
        Pool { reg: datums.clone(), datums }
    }
    /// nops, then per operation its code and, for the codes 1, 2 and 6, its argument.
    fn lprog(&mut self) -> Prog {
        let n = self.i();
        let mut p = Vec::new();
        for _ in 0..n {
            let code = self.i();
            let arg = if matches!(code, 1 | 2 | 6) { self.i() } else { 0 };
            p.push((code, arg));
        }
        p
    }
    /// n, then n pairs (datum index, power).
    fn items(&mut self) -> Vec<(i32, i32)> {
        let n = self.i();
        (0..n)
            .map(|_| {
                let a = self.i();
                let b = self.i();
                (a, b)
            })
            .collect()
    }
    fn tri_body(&mut self) -> TriSpec {
        let kind = self.i();
        let mut s = TriSpec {
            kind,
            lo: 1,
            uv: false,
            nrm: false,
            defl: 0.0,
            purpose: 0,
            p: vec![],
            uvs: vec![],
            t: vec![],
            n: vec![],
        };
        if kind < 0 {
            return s;
        }
        let nn = self.i();
        let nt = self.i();
        s.uv = self.i() != 0;
        s.nrm = self.i() != 0;
        s.lo = self.i();
        s.defl = self.f();
        s.purpose = self.i() as u32;
        for _ in 0..nn {
            let q = self.xyz();
            s.p.push(q);
        }
        if s.uv {
            for _ in 0..nn {
                let q = self.xy();
                s.uvs.push(q);
            }
        }
        for _ in 0..nt {
            let a = self.i();
            let b = self.i();
            let c = self.i();
            s.t.push([a, b, c]);
        }
        if s.nrm {
            for _ in 0..nn {
                let x = self.f() as f32;
                let y = self.f() as f32;
                let z = self.f() as f32;
                s.n.push([x, y, z]);
            }
        }
        s
    }
    fn tri(&mut self) -> TriSpec {
        self.tri_body()
    }
    /// k, then k triangulation recipes (kind -1: a null handle).
    fn trilist(&mut self) -> Vec<TriSpec> {
        let k = self.i();
        (0..k).map(|_| self.tri_body()).collect()
    }
    /// nops, then per operation its code and the fields the code uses (docs/api-poly.md, "tops").
    fn tops(&mut self) -> Vec<TOpSpec> {
        let n = self.i();
        let mut v = Vec::new();
        for _ in 0..n {
            let code = self.i();
            let mut o = TOpSpec { code, a: 0, b: 0, c: 0, d: 0, x: 0.0, y: 0.0, z: 0.0 };
            match code {
                4 | 5 => {
                    o.a = self.i() as i64;
                    o.b = self.i() as i64;
                }
                6 | 10 => o.a = self.i() as i64,
                9 => o.x = self.f(),
                11 | 14 => {
                    o.a = self.i() as i64;
                    o.x = self.f();
                    o.y = self.f();
                    o.z = self.f();
                }
                12 => {
                    o.a = self.i() as i64;
                    o.x = self.f();
                    o.y = self.f();
                }
                13 => {
                    o.a = self.i() as i64;
                    o.b = self.i() as i64;
                    o.c = self.i() as i64;
                    o.d = self.i() as i64;
                }
                _ => {}
            }
            v.push(o);
        }
        v
    }
    fn pg3(&mut self) -> PgSpec3 {
        let kind = self.i();
        let nn = self.i();
        let hasp = self.i() != 0;
        let lo = self.i();
        let np = self.i();
        let plo = self.i();
        let defl = self.f();
        let p = (0..nn).map(|_| self.pnt()).collect();
        let par = (0..np).map(|_| self.f()).collect();
        PgSpec3 { kind, nn, hasp, lo, np, plo, defl, p, par }
    }
    fn pg2(&mut self) -> PgSpec2 {
        let kind = self.i();
        let nn = self.i();
        let _lo = self.i();
        let defl = self.f();
        let p = (0..nn)
            .map(|_| {
                let q = self.xy();
                Pnt2d::new(q.x(), q.y())
            })
            .collect();
        PgSpec2 { kind, nn, defl, p }
    }
    fn pot(&mut self) -> PotSpec {
        let kind = self.i();
        let nn = self.i();
        let hasp = self.i() != 0;
        let nlo = self.i();
        let np = self.i();
        let plo = self.i();
        let defl = self.f();
        let n = (0..nn).map(|_| self.i()).collect();
        let par = (0..np).map(|_| self.f()).collect();
        PotSpec { kind, nn, hasp, nlo, plo, defl, n, par }
    }
    /// A text: "T", then the escaped text (see [`escape`]).
    fn txt(&mut self) -> String {
        let w = self.word();
        unescape(w.strip_prefix('T').expect("text token"))
    }
    /// n, then n points (pnt2d).
    fn pts2d(&mut self) -> Vec<Pnt2d> {
        let n = self.i();
        (0..n)
            .map(|_| {
                let q = self.xy();
                Pnt2d::new(q.x(), q.y())
            })
            .collect()
    }
    /// angle tolerance unit-factor nb-facets flags.
    fn mcfg(&mut self) -> MCfg {
        let angle = self.f();
        let tol = self.f();
        let unit = self.f();
        let nbf = self.i();
        let flags = self.i();
        MCfg { angle, tol, unit, nbf, flags }
    }
    /// n, then per operation its code (0: a triangle, 3 points; 1: a quad, 4 points; 2: a triangulation, a
    /// transformation and the reverse flag).
    fn mops(&mut self) -> Vec<MOp> {
        let n = self.i();
        let mut v = Vec::new();
        for _ in 0..n {
            let code = self.i();
            v.push(match code {
                0 => MOp::Tri([self.xyz(), self.xyz(), self.xyz()]),
                1 => MOp::Quad([self.xyz(), self.xyz(), self.xyz(), self.xyz()]),
                _ => {
                    let t = self.tri_body();
                    let tr = self.trsf_body();
                    let rev = self.i() != 0;
                    MOp::Triangulation(Box::new(t), tr, rev)
                }
            });
        }
        v
    }
    /// The recipe of a loop-maker line, see docs/api-poly.md ("loops").
    fn loops(&mut self) -> LoopSpec {
        let dim = self.i();
        let left = self.i() != 0;
        let nn = self.i();
        let mut pos = Vec::new();
        for _ in 0..nn {
            let x = self.f();
            let y = self.f();
            let z = if dim == 3 { self.f() } else { 0.0 };
            pos.push([x, y, z]);
        }
        let nh = self.i();
        let mut hl = Vec::new();
        for _ in 0..nh {
            let a = self.i();
            let b = self.i();
            let c = self.i();
            hl.push([a, b, c]);
        }
        let mut nrm = Vec::new();
        if dim == 3 {
            for _ in 0..nn {
                let has = self.i() != 0;
                let d = self.dir();
                nrm.push((has, d));
            }
        }
        let nops = self.i();
        let mut ops = Vec::new();
        for _ in 0..nops {
            let code = self.i();
            let k = match code {
                0 => 3,
                1 => 6,
                2 => 3,
                3 => 2,
                _ => 0,
            };
            let mut a = [0; 6];
            for e in a.iter_mut().take(k) {
                *e = self.i();
            }
            ops.push((code, a));
        }
        LoopSpec { dim, left, nn, pos, hl, nrm, ops }
    }
    /// nb-buckets, n, then n pairs (code, key).
    fn mapops(&mut self) -> (i32, Vec<(i32, i32)>) {
        let nb = self.i();
        let n = self.i();
        let ops = (0..n)
            .map(|_| {
                let c = self.i();
                let k = self.i();
                (c, k)
            })
            .collect();
        (nb, ops)
    }
    /// n, then per operation its code and the fields the code uses (docs/api-poly.md, "cops").
    fn cops(&mut self) -> Vec<COpSpec> {
        let n = self.i();
        let mut v = Vec::new();
        for _ in 0..n {
            let code = self.i();
            let mut o = COpSpec { code, x: 0.0, y: 0.0, z: 0.0, a: 0, b: 0, c: 0, d: 0 };
            match code {
                0 => {
                    o.x = self.f();
                    o.y = self.f();
                    o.z = self.f();
                    o.a = self.i() as i64;
                }
                1 => {
                    o.a = self.i() as i64;
                    o.b = self.i() as i64;
                    o.c = self.i() as i64;
                }
                2 | 8 => o.a = self.i() as i64,
                3 => {
                    o.a = self.i() as i64;
                    o.b = self.i() as i64;
                    o.c = self.i() as i64;
                    o.d = self.i() as i64;
                }
                6 => {
                    o.x = self.f();
                    o.a = self.i() as i64;
                }
                7 | 11 => {
                    o.a = self.i() as i64;
                    o.b = self.i() as i64;
                }
                9 => {
                    o.a = self.i() as i64;
                    o.x = self.f();
                    o.y = self.f();
                }
                10 => {
                    o.a = self.i() as i64;
                    o.x = self.f();
                    o.y = self.f();
                    o.z = self.f();
                }
                12 => o.x = self.f(),
                _ => {}
            }
            v.push(o);
        }
        v
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread inputs");
    }
}

fn put_adjacency(g: &mut Got, c: &Connect, t: &Triangulation) -> Result<(), Thrown> {
    for k in 1..=t.nb_triangles() {
        let (a, b, d) = c.triangles(k)?;
        let (n1, n2, n3) = c.nodes(k)?;
        for v in [a, b, d, n1, n2, n3] {
            g.i(v);
        }
    }
    for n in 1..=t.nb_nodes() {
        g.i(c.triangle(n)?);
    }
    Ok(())
}

fn put_packed(g: &mut Got, m: &PackedMapOfInteger) {
    g.i(m.extent());
    g.b(m.is_empty());
    let keys = m.keys();
    g.l(keys.len());
    for k in keys {
        g.i(k);
    }
}

fn tri_of(s: &TriSpec) -> Result<Triangulation, Thrown> {
    mk_tri(s)?.ok_or(Thrown)
}

// ---------------------------------------------------------------------------------
// Replaying one family

fn run(name: &str, r: &mut Rd, g: &mut Got) -> Result<(), Thrown> {
    match name {
        // ---- TopLoc ----
        "loc_state" => {
            let mut pool = r.lp();
            let a = r.lprog();
            let l = eval_prog(&mut pool, &a)?;
            put_loc(g, &pool, &l)?;
        }
        "loc_mul" | "loc_div" | "loc_predivided" => {
            let mut pool = r.lp();
            let (a, b) = (r.lprog(), r.lprog());
            let x = eval_prog(&mut pool, &a)?;
            let y = eval_prog(&mut pool, &b)?;
            match name {
                "loc_mul" => {
                    put_loc(g, &pool, &x.multiplied(&y)?)?;
                    put_loc(g, &pool, &x.multiplied(&y)?)?;
                }
                "loc_div" => {
                    put_loc(g, &pool, &x.divided(&y)?)?;
                    put_loc(g, &pool, &x.divided(&y)?)?;
                }
                _ => put_loc(g, &pool, &x.predivided(&y)?)?,
            }
        }
        "loc_inverted" => {
            let mut pool = r.lp();
            let a = r.lprog();
            let x = eval_prog(&mut pool, &a)?;
            put_loc(g, &pool, &x.inverted()?)?;
        }
        "loc_powered" => {
            let mut pool = r.lp();
            let a = r.lprog();
            let n = r.i();
            let x = eval_prog(&mut pool, &a)?;
            put_loc(g, &pool, &x.powered(n)?)?;
        }
        "loc_equal" => {
            let mut pool = r.lp();
            let (a, b) = (r.lprog(), r.lprog());
            let x = eval_prog(&mut pool, &a)?;
            let y = eval_prog(&mut pool, &b)?;
            let e = x.is_equal(&y);
            g.b(e);
            g.b(x.is_different(&y));
            g.b(x == y);
            g.b(x != y);
            if e {
                g.b(x.hash_code() == y.hash_code());
            }
            g.b(x.is_identity());
            if x.is_identity() {
                g.b(x.hash_code() == 0);
            }
        }
        "loc_first" => {
            let mut pool = r.lp();
            let a = r.lprog();
            let x = eval_prog(&mut pool, &a)?;
            let d = x.first_datum()?.clone();
            g.i(pool.index(&d));
            g.i(x.first_power()?);
            put_loc(g, &pool, &x.next_location())?;
            g.trsf(d.transformation())?;
        }
        "loc_laws" => {
            let mut pool = r.lp();
            let (a, b) = (r.lprog(), r.lprog());
            let x = eval_prog(&mut pool, &a)?;
            let y = eval_prog(&mut pool, &b)?;
            g.b(x.multiplied(&x.inverted()?)?.is_identity());
            g.b(x.inverted()?.multiplied(&x)?.is_identity());
            g.b(x.divided(&x)?.is_identity());
            g.b(x.predivided(&x)?.is_identity());
            g.b(x.powered(0)?.is_identity());
            g.b(x.powered(2)?.multiplied(&x.powered(-2)?)?.is_identity());
            g.b(x.inverted()?.inverted()? == x);
            g.b(x.powered(1)? == x);
            g.b(x.powered(-1)? == x.inverted()?);
            g.b(x.multiplied(&Location::new())? == x);
            g.b(Location::new().multiplied(&x)? == x);
            let mut c = x.clone();
            c.identity();
            g.b(c.is_identity());
            let mut d = x.clone();
            d.clear();
            g.b(d.is_identity());
            g.b(x.multiplied(&y)?.inverted()? == y.inverted()?.multiplied(&x.inverted()?)?);
            g.b(x.multiplied(&y)?.divided(&y)? == x);
            g.b(x.predivided(&y)? == y.inverted()?.multiplied(&x)?);
            g.b(x.multiplied(&y)?.multiplied(&y.inverted()?)? == x);
            g.b(x.divided(&y)? == x.multiplied(&y.inverted()?)?);
            g.b(x.multiplied(&y)?.is_identity());
            g.b(x.powered(3)? == x.multiplied(&x)?.multiplied(&x)?);
        }
        "loc_from_trsf" => {
            let t = r.trsf();
            let l = Location::from_trsf(&t)?;
            let pool = Pool { datums: vec![], reg: vec![l.first_datum()?.clone()] };
            put_loc(g, &pool, &l)?;
            g.trsf(&l.to_trsf())?;
            g.b(l.is_identity());
        }
        "datum3d" => {
            let t = r.trsf();
            let d = Datum3D::from_trsf(&t);
            g.trsf(d.transformation())?;
            g.trsf(d.trsf())?;
            g.i(d.form() as i32);
            g.text(&d.shallow_dump());
            g.trsf(Datum3D::new().transformation())?;
        }
        "loc_dump" => {
            let mut pool = r.lp();
            let a = r.lprog();
            let x = eval_prog(&mut pool, &a)?;
            g.text(&x.shallow_dump());
        }
        "item_loc" => {
            let pool = r.lp();
            let idx = r.i();
            let pwr = r.i();
            let it = ItemLocation::new(pool.datum(idx)?, pwr)?;
            g.i(pool.index(it.datum()));
            g.i(it.power());
            g.trsf(it.transformation())?;
        }
        "slist_build" => {
            let pool = r.lp();
            let items = r.items();
            let ntail = r.i();
            let flag = r.i();
            let mut built = SListOfItemLocation::new();
            for (idx, p) in &items {
                built.construct(ItemLocation::new(pool.datum(*idx)?, *p)?)?;
            }
            let mut s = built.clone();
            for _ in 0..ntail {
                s.to_tail();
            }
            g.b(s.is_empty());
            g.b(s.more());
            if s.is_empty() && flag != 0 {
                return Err(Thrown);
            }
            let mut cnt = 0;
            let mut c = s.clone();
            while c.more() {
                cnt += 1;
                c.next();
            }
            g.i(cnt);
            let mut c = s.clone();
            while c.more() {
                let v = c.value()?;
                g.i(pool.index(v.datum()));
                g.i(v.power());
                g.trsf(v.transformation())?;
                c = c.tail();
            }
            g.b(s.tail().is_empty());
        }
        // ---- Poly_Triangulation ----
        "tri_state" => {
            let s = r.tri();
            g.tri_state(&tri_of(&s)?)?;
        }
        "tri_copy" => {
            let s = r.tri();
            let mode = r.i();
            let mut t = tri_of(&s)?;
            if mode == 2 {
                t.update_cached_min_max();
            }
            let mut c = t.clone();
            g.tri_state(&c)?;
            c.set_node(1, &Pnt::new(7.0, 8.0, 9.0))?;
            c.set_deflection(99.0);
            let (a, b) = (t.node(1)?, c.node(1)?);
            g.pnt(&a);
            g.pnt(&b);
            g.f(t.deflection());
            g.f(c.deflection());
        }
        "tri_ctor_invalid" => {
            let (nn, nt, uv, nrm) = (r.i(), r.i(), r.i(), r.i());
            if nn > 0 && nt < 0 {
                return Err(Thrown);
            }
            let t = Triangulation::with_sizes(nn, nt, uv != 0, nrm != 0)?;
            g.i(t.nb_nodes());
            g.i(t.nb_triangles());
            g.b(t.has_uv_nodes());
            g.b(t.has_normals());
            g.b(t.is_double_precision());
            g.b(t.has_geometry());
            g.f(t.deflection());
            g.i(t.mesh_purpose() as i32);
        }
        "tri_access" => {
            let s = r.tri();
            let opc = r.i();
            let idx = r.i();
            let (x, y, z) = {
                let v = r.xyz();
                (v.x(), v.y(), v.z())
            };
            let mut t = tri_of(&s)?;
            match opc {
                0 => g.pnt(&t.node(idx)?),
                1 => g.pnt2d(&t.uv_node(idx)?),
                2 => {
                    let (a, b, c) = t.triangle(idx)?.get();
                    g.i(a);
                    g.i(b);
                    g.i(c);
                }
                3 => g.dir(&t.normal_dir(idx)?),
                4 => g.f3(t.normal(idx)?),
                5 => {
                    t.set_node(idx, &Pnt::new(x, y, z))?;
                    g.pnt(&t.node(idx)?);
                }
                6 => {
                    t.set_uv_node(idx, &Pnt2d::new(x, y))?;
                    g.pnt2d(&t.uv_node(idx)?);
                }
                7 => {
                    t.set_triangle(idx, Triangle::from_nodes(1, 2, idx))?;
                    let (a, b, c) = t.triangle(idx)?.get();
                    g.i(a);
                    g.i(b);
                    g.i(c);
                }
                8 => {
                    t.set_normal(idx, [x as f32, y as f32, z as f32])?;
                    g.f3(t.normal(idx)?);
                }
                9 => {
                    t.set_normal_dir(idx, &Dir::new(x, y, z)?)?;
                    g.f3(t.normal(idx)?);
                }
                _ => {
                    t.set_normal(idx, [x as f32, y as f32, z as f32])?;
                    g.dir(&t.normal_dir(idx)?);
                }
            }
        }
        "tri_minmax" => {
            let s = r.tri();
            let cb = r.bx();
            let tb = r.bx();
            let tr = r.trsf();
            let acc = r.i();
            let mode = r.i();
            let mut t = tri_of(&s)?;
            let c = mk_box(&cb);
            let mut bx = mk_box(&tb);
            match mode {
                1 => t.update_cached_min_max(),
                2 => t.set_cached_min_max(&c),
                3 => {
                    t.set_cached_min_max(&c);
                    t.set_node(1, &Pnt::new(100.0, 100.0, 100.0))?;
                }
                4 => {
                    t.update_cached_min_max();
                    t.clear();
                }
                _ => {}
            }
            let ret = t.min_max(&mut bx, &tr, acc != 0);
            g.b(ret);
            g.bx(&bx);
            g.b(t.has_cached_min_max());
            if t.has_cached_min_max() {
                g.bx(&t.cached_min_max());
            }
        }
        "tri_normals" => {
            let s = r.tri();
            let mode = r.i();
            let mut t = tri_of(&s)?;
            match mode {
                0 => t.compute_normals()?,
                1 => poly::compute_normals(&mut t)?,
                2 => {
                    t.compute_normals()?;
                    t.remove_normals();
                }
                _ => {
                    t.add_normals();
                    g.i(t.nb_nodes());
                    g.b(t.has_normals());
                    return Ok(());
                }
            }
            g.b(t.has_normals());
            if t.has_normals() {
                for i in 1..=t.nb_nodes() {
                    g.f3(t.normal(i)?);
                    g.dir(&t.normal_dir(i)?);
                }
            }
        }
        "tri_modify" => {
            let s = r.tri();
            let ops = r.tops();
            let mut t = tri_of(&s)?;
            run_tops(&mut t, &ops)?;
            g.tri_state(&t)?;
        }
        "mesh_bad_index" => {
            let _ = r.tri();
            let _ = r.i();
            return Err(Thrown);
        }
        "poly_catenate" => {
            let specs = r.trilist();
            let mut tris: Vec<Option<Triangulation>> = Vec::new();
            for s in &specs {
                tris.push(mk_tri(s)?);
            }
            let refs: Vec<Option<&Triangulation>> = tris.iter().map(|t| t.as_ref()).collect();
            let res = poly::catenate(&refs);
            g.b(res.is_some());
            if let Some(t) = res {
                g.tri_state(&t)?;
            }
        }
        // ---- Poly:: helpers ----
        "poly_point_on_triangle" => {
            let (a, b, c, p) = (r.xy(), r.xy(), r.xy(), r.xy());
            let (d, uv) = poly::point_on_triangle(&a, &b, &c, &p);
            g.f(d);
            g.f(uv.x());
            g.f(uv.y());
        }
        "poly_tri_line" => {
            let start = r.xyz();
            let d = r.dir();
            let (v0, v1, v2) = (r.xyz(), r.xyz(), r.xyz());
            let (res, param) = poly::intersect_tri_line(&start, &d, &v0, &v1, &v2);
            g.i(res);
            g.f(param.unwrap_or(-123.5));
        }
        "poly_intersect" => {
            let s = r.tri();
            let loc = r.pnt();
            let d = r.dir();
            let closest = r.i();
            let t = tri_of(&s)?;
            if r.err {
                return Err(Thrown);
            }
            let res = poly::intersect(&t, &Ax1::new(loc, d), closest != 0);
            g.b(res.is_some());
            let (tr, dist) = res.unwrap_or((Triangle::from_nodes(7, 8, 9), -5.0));
            let (a, b, c) = tr.get();
            g.i(a);
            g.i(b);
            g.i(c);
            g.f(dist);
        }
        "poly_polygon_properties" => {
            let _lo = r.i();
            let pts = r.pts2d();
            if pts.is_empty() {
                return Err(Thrown);
            }
            let (area, per) = poly::polygon_properties(&pts);
            g.b(true);
            g.f(area);
            g.f(per);
        }
        // ---- polygons ----
        "pg3_state" => {
            let s = r.pg3();
            if pg3_unchecked(&s) {
                return Err(Thrown);
            }
            let p = mk_pg3(&s)?;
            g.pg3(&p)?;
            let mut c = p.clone();
            g.pg3(&c)?;
            if c.nb_nodes() >= 1 {
                c.change_nodes()[0] = Pnt::new(9.0, 9.0, 9.0);
                c.set_deflection(5.5);
                if c.has_parameters() {
                    c.change_parameters()?[0] = 77.0;
                }
            }
            g.pg3(&p)?;
            g.pg3(&c)?;
        }
        "pg3_params_none" => {
            let _ = r.pg3();
            return Err(Thrown);
        }
        "pg2_state" => {
            let s = r.pg2();
            let p = mk_pg2(&s)?;
            g.pg2(&p);
            let mut c = p.clone();
            g.pg2(&c);
            if c.nb_nodes() >= 1 {
                c.change_nodes()[0] = Pnt2d::new(9.0, 9.0);
                c.set_deflection(5.5);
            }
            g.pg2(&p);
            g.pg2(&c);
        }
        "pot_state" => {
            let s = r.pot();
            let p = mk_pot(&s)?;
            g.pot(&p)?;
            if let Some((lo, up)) = p.parameters_bounds()
                && up - lo + 1 != p.nb_nodes()
            {
                g.pot(&p)?;
                return Ok(());
            }
            let mut c = p.clone();
            g.pot(&c)?;
            if c.nb_nodes() >= 1 {
                c.set_node(1, 99)?;
                c.set_deflection(5.5);
                if let Some((lo, up)) = c.parameters_bounds()
                    && up - lo + 1 >= 1
                {
                    c.set_parameter(1, 77.0)?;
                }
            }
            g.pot(&p)?;
            g.pot(&c)?;
        }
        "pot_ops" => {
            let s = r.pot();
            let op = r.i();
            let idx = r.i();
            let iv = r.i();
            let fv = r.f();
            let mut p = mk_pot(&s)?;
            match op {
                0 => g.i(p.node(idx)?),
                1 => {
                    p.set_node(idx, iv)?;
                    g.pot(&p)?;
                }
                2 => g.f(p.parameter(idx)?),
                3 | 6 => {
                    p.set_parameter(idx, fv)?;
                    g.pot(&p)?;
                }
                4 => {
                    let h = if iv > 0 {
                        Some((idx, (0..iv).map(|k| fv + f64::from(k)).collect::<Vec<f64>>()))
                    } else {
                        None
                    };
                    p.set_parameters(h)?;
                    g.pot(&p)?;
                }
                _ => {
                    p.set_node(idx, iv)?;
                    g.pot(&p)?;
                }
            }
        }
        // ---- text formats ----
        "poly_write_tri" => {
            let s = r.tri();
            let compact = r.i();
            let prec = r.i();
            let t = tri_of(&s)?;
            g.text(&poly::write_triangulation(&t, compact != 0, prec as usize)?);
            if compact == 0 {
                g.text(&poly::dump_triangulation(&t, prec as usize)?);
            }
        }
        "poly_write_pg3" => {
            let s = r.pg3();
            let compact = r.i();
            let prec = r.i();
            let p = mk_pg3(&s)?;
            g.text(&poly::write_polygon3d(&p, compact != 0, prec as usize)?);
            if compact == 0 {
                g.text(&poly::dump_polygon3d(&p, prec as usize)?);
            }
        }
        "poly_write_pg2" => {
            let s = r.pg2();
            let compact = r.i();
            let prec = r.i();
            let p = mk_pg2(&s)?;
            g.text(&poly::write_polygon2d(&p, compact != 0, prec as usize));
            if compact == 0 {
                g.text(&poly::dump_polygon2d(&p, prec as usize));
            }
        }
        "poly_read_tri" => {
            let text = r.txt();
            let t = poly::read_triangulation(&text)?;
            g.b(t.is_some());
            if let Some(t) = t {
                g.tri_state(&t)?;
            }
        }
        "poly_read_pg3" => {
            let text = r.txt();
            let p = poly::read_polygon3d(&text)?;
            g.b(p.is_some());
            if let Some(p) = p {
                g.pg3(&p)?;
            }
        }
        "poly_read_pg2" => {
            let text = r.txt();
            let p = poly::read_polygon2d(&text)?;
            g.b(p.is_some());
            if let Some(p) = p {
                g.pg2(&p);
            }
        }
        // ---- Poly_Connect ----
        "connect_adjacency" => {
            let s = r.tri();
            let t = Rc::new(tri_of(&s)?);
            let c = Connect::from_triangulation(&t)?;
            g.b(c.triangulation().is_some_and(|x| Rc::ptr_eq(x, &t)));
            g.b(c.more());
            g.i(c.value());
            put_adjacency(g, &c, &t)?;
        }
        "connect_around" => {
            let s = r.tri();
            let t = Rc::new(tri_of(&s)?);
            let mut c = Connect::from_triangulation(&t)?;
            for n in 1..=t.nb_nodes() {
                c.initialize(n)?;
                g.b(c.more());
                let mut seq = Vec::new();
                let mut guard = 0;
                while c.more() && guard < 4 * t.nb_triangles() + 8 {
                    seq.push(c.value());
                    c.next()?;
                    guard += 1;
                }
                g.l(seq.len());
                for v in seq {
                    g.i(v);
                }
                g.b(c.more());
            }
        }
        "connect_reload" => {
            let s1 = r.tri();
            let s2 = r.tri();
            let first = r.i();
            let t1 = Rc::new(tri_of(&s1)?);
            let t2 = Rc::new(tri_of(&s2)?);
            let mut c = Connect::new();
            g.b(c.triangulation().is_none());
            g.b(c.more());
            g.i(c.value());
            if first == 0 {
                c.load(&t1)?;
                put_adjacency(g, &c, &t1)?;
            } else if first == 1 {
                let d = Connect::from_triangulation(&t1)?;
                put_adjacency(g, &d, &t1)?;
                c.load(&t1)?;
            }
            c.load(&t2)?;
            g.b(c.triangulation().is_some_and(|x| Rc::ptr_eq(x, &t2)));
            g.b(c.more());
            g.i(c.value());
            put_adjacency(g, &c, &t2)?;
            c.initialize(1)?;
            g.b(c.more());
            g.i(c.value());
        }
        "connect_checks" => {
            let s = r.tri();
            let op = r.i();
            let idx = r.i();
            let dflt = r.i();
            let t = Rc::new(tri_of(&s)?);
            let nn = t.nb_nodes();
            let mut c = if dflt == 0 { Connect::from_triangulation(&t)? } else { Connect::new() };
            if dflt != 0 && op == 3 {
                return Err(Thrown);
            }
            if op == 3 && (idx < 1 || idx > nn) {
                return Err(Thrown);
            }
            match op {
                0 => g.i(c.triangle(idx)?),
                1 => {
                    let (a, b, d) = c.triangles(idx)?;
                    g.i(a);
                    g.i(b);
                    g.i(d);
                }
                2 => {
                    let (a, b, d) = c.nodes(idx)?;
                    g.i(a);
                    g.i(b);
                    g.i(d);
                }
                _ => {
                    c.initialize(idx)?;
                    g.b(c.more());
                    g.i(c.value());
                }
            }
        }
        // ---- Poly_MergeNodesTool ----
        "merge_tool" => {
            let c = r.mcfg();
            let ops = r.mops();
            run_merge(g, &c, &ops, c.nbf, true)?;
        }
        "merge_static" => {
            let s = r.tri();
            let t = r.trsf();
            let rev = r.i();
            let angle = r.f();
            let tol = r.f();
            let force = r.i();
            let tri = mk_tri(&s)?;
            let res = MergeNodesTool::merge_nodes(tri.as_ref(), &t, rev != 0, angle, tol, force != 0)?;
            g.b(res.is_some());
            if let Some(res) = res {
                g.tri_state(&res)?;
            }
        }
        "merge_params" => {
            let (a1, t1, a2, t2) = (r.f(), r.f(), r.f(), r.f());
            let mut t = MergeNodesTool::new(a1, t1, -1);
            g.f(t.merge_angle());
            g.f(t.merge_tolerance());
            g.b(t.to_merge_opposite());
            g.b(t.to_drop_degenerative());
            g.b(t.to_merge_elems());
            t.set_merge_angle(a2);
            t.set_merge_tolerance(t2);
            t.set_merge_opposite(true);
            t.set_drop_degenerative(false);
            t.set_merge_elems(true);
            g.f(t.merge_angle());
            g.f(t.merge_tolerance());
            g.b(t.to_merge_opposite());
            g.b(t.to_drop_degenerative());
            g.b(t.to_merge_elems());
        }
        "merge_element_errors" => {
            let nbn = r.i();
            let via = r.i();
            let ang = r.f();
            let q = [r.xyz(), r.xyz(), r.xyz(), r.xyz()];
            let mut t = MergeNodesTool::new(ang, 0.0, 10);
            if via == 0 {
                t.add_element(&q, nbn)?;
            } else {
                for (k, v) in q.iter().enumerate() {
                    *t.change_element_node(k as i32) = *v;
                }
                t.push_last_element(nbn)?;
            }
            g.i(t.nb_nodes());
            g.i(t.nb_elements());
            g.i(t.nb_degenerative_elems());
            for k in 0..4 {
                g.i(t.element_node_index(k));
            }
        }
        // ---- Poly_MakeLoops, packed map ----
        "make_loops" => {
            let s = r.loops();
            if loops_unchecked(&s) {
                return Err(Thrown);
            }
            run_loops(g, &s)?;
        }
        "packed_map" => {
            let (nb, ops) = r.mapops();
            let mut m = PackedMapOfInteger::new(nb);
            for (c, k) in ops {
                match c {
                    0 => g.b(m.add(k)),
                    1 => g.b(m.remove(k)),
                    2 => g.b(m.contains(k)),
                    3 => put_packed(g, &m),
                    _ => {
                        m.clear();
                        g.b(m.is_empty());
                    }
                }
            }
            put_packed(g, &m);
        }
        "heap_of_integer" => {
            let (nb, ops) = r.mapops();
            let mut hp = HeapOfInteger::new(nb);
            for (c, k) in ops {
                match c {
                    0 => hp.add(k),
                    1 => hp.remove(k),
                    2 => g.b(hp.contains(k)),
                    3 => g.i(hp.top()?),
                    4 => g.b(hp.is_empty()),
                    _ => {
                        hp.clear();
                        g.b(hp.is_empty());
                    }
                }
            }
            g.b(hp.is_empty());
            if !hp.is_empty() {
                g.i(hp.top()?);
            }
        }
        // ---- Poly_CoherentTriangulation ----
        "coherent_program" => {
            let s = r.tri();
            let ops = r.cops();
            let t = mk_tri(&s)?;
            let mut c = if s.kind < 0 {
                CoherentTriangulation::new()
            } else {
                CoherentTriangulation::from_triangulation(t.as_ref())?
            };
            run_cops(g, &mut c, &ops)?;
            put_coh(g, &c)?;
        }
        "coherent_clone" => {
            let s = r.tri();
            let ops = r.cops();
            let t = mk_tri(&s)?;
            let mut c = CoherentTriangulation::from_triangulation(t.as_ref())?;
            run_cops(g, &mut c, &ops)?;
            let k = c.clone_coherent()?;
            g.b(k.is_some());
            if let Some(k) = k {
                put_coh(g, &k)?;
            }
        }
        "coherent_empty" => {
            let mode = r.i();
            let n = r.i();
            let x = r.f();
            let mut c =
                if mode == 1 { CoherentTriangulation::from_triangulation(None)? } else { CoherentTriangulation::new() };
            for i in 0..n {
                c.set_node(&XYZ::new(x + f64::from(i), 0.0, 0.0), -1)?;
            }
            if mode == 3 {
                c.set_deflection(x);
            }
            put_coh(g, &c)?;
            g.b(c.clone_coherent()?.is_none());
        }
        "coherent_basic" => {
            let (a, b, c, side, k) = (r.i(), r.i(), r.i(), r.i(), r.i());
            let (u, v, nx, ny, nz) = (r.f(), r.f(), r.f(), r.f(), r.f());
            let l0 = CoherentLink::new();
            g.b(l0.is_empty());
            for q in [l0.node(0), l0.node(1), l0.opposite_node(0), l0.opposite_node(1)] {
                g.i(q);
            }
            let mut l1 = CoherentLink::from_nodes(a, b);
            g.b(l1.is_empty());
            for i in 0..4 {
                g.i(l1.node(i));
            }
            g.i(l1.opposite_node(0));
            g.i(l1.opposite_node(1));
            g.b(l1.get_attribute().is_none());
            l1.set_attribute(Some(5));
            g.b(l1.get_attribute() == Some(5));
            l1.nullify();
            g.b(l1.is_empty());
            g.i(l1.node(0));
            let t0 = CoherentTriangle::new();
            g.b(t0.is_empty());
            g.i(t0.n_connections());
            for i in 0..3 {
                g.i(t0.node(i));
                g.i(t0.connected_node(i));
                g.b(t0.connected_triangle(i).is_none());
                g.b(t0.link(i).is_none());
            }
            let t1 = CoherentTriangle::from_nodes(a, b, c);
            g.b(t1.is_empty());
            g.i(t1.n_connections());
            for i in 0..3 {
                g.i(t1.node(i));
                g.i(t1.connected_node(i));
            }
            let l2 = CoherentLink::from_triangle(&t1, side)?;
            g.i(l2.node(0));
            g.i(l2.node(1));
            g.i(l2.opposite_node(0));
            g.i(l2.opposite_node(1));
            let n0 = CoherentNode::new();
            g.b(n0.is_free_node());
            g.i(n0.get_index());
            g.f(n0.get_u());
            g.f(n0.get_v());
            g.f(n0.x());
            g.f(n0.y());
            g.f(n0.z());
            let mut n1 = CoherentNode::from_xyz(&XYZ::new(u, v, nx));
            g.b(n1.is_free_node());
            g.i(n1.get_index());
            g.f(n1.get_u());
            g.f(n1.get_v());
            g.b(n1.has_normal());
            g.xyz(&n1.get_normal());
            n1.set_uv(u, v);
            n1.set_index(k);
            n1.set_normal(&XYZ::new(nx, ny, nz));
            g.f(n1.get_u());
            g.f(n1.get_v());
            g.i(n1.get_index());
            g.b(n1.has_normal());
            g.xyz(&n1.get_normal());
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
            Tok::I(g) => assert_eq!(w.parse::<i64>().expect("integer output"), *g, "value {index} (int)\n{line}"),
            Tok::S(g) => assert_eq!(*w, g.as_str(), "value {index} (text)\n{line}"),
        }
    }
}

// ---------------------------------------------------------------------------------

#[test]
fn poly_matches_occt() {
    let text = include_str!("poly.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let (mut n_throws, mut n_unchecked) = (0usize, 0usize);

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in poly.txt:\n{line}");
        let (head, tail) = line.split_once(" | ").unwrap_or_else(|| panic!("line without ' | ': {line}"));
        let head: Vec<&str> = head.split_whitespace().collect();
        let want: Vec<&str> = tail.split_whitespace().collect();
        let (name, inputs) = head.split_first().expect("family name");
        let name: &str = name;
        let entry = stats.entry(name).or_default();
        entry.0 += 1;
        entry.1 += usize::from(!inputs.is_empty());

        let mut reader = Rd { words: inputs, at: 0, err: false };
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

/// A text as one whitespace-free token, as the oracle's `os_text` writes it: `\\`, `\n`, `\t`,
/// `\r`, `\s` for a space, and `\e` for the empty text.
fn escape(t: &str) -> String {
    let mut o = String::new();
    for c in t.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            ' ' => o.push_str("\\s"),
            c => o.push(c),
        }
    }
    if o.is_empty() { "\\e".to_string() } else { o }
}

fn unescape(t: &str) -> String {
    if t == "\\e" {
        return String::new();
    }
    let mut o = String::new();
    let mut it = t.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            o.push(c);
            continue;
        }
        match it.next() {
            Some('n') => o.push('\n'),
            Some('t') => o.push('\t'),
            Some('r') => o.push('\r'),
            Some('s') => o.push(' '),
            Some(x) => o.push(x),
            None => o.push('\\'),
        }
    }
    o
}
