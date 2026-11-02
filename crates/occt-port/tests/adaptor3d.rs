//! Differential judge for OCCT's package `Adaptor3d` (OCCT 8.0.1): replays `tests/adaptor3d.txt`, printed by
//! `oracle/adaptor3d.cpp` against the real library. See `docs/api-adaptor3d.md`.
//!
//! The adaptors the classes work on are the test adaptors of the oracle: `Tc3` (an `Adaptor3d_Curve`), `Ts` (an
//! `Adaptor3d_Surface`) and `Tc2` (an `Adaptor2d_Curve2d`), which answer from printed data (a polynomial curve / surface and
//! the data of the analytic curves and surfaces) and fold every call into a counter and a hash. They are defined by the
//! records of the families `def_curve3d`, `def_surface`, `def_curve2d` and referred to by index.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
#![allow(clippy::too_many_lines, clippy::type_complexity)]
use occt_port::adaptor2d::{AdaptorError, Curve2dAdaptor, GeomAbsCurveType, GeomAbsShape};
use occt_port::adaptor3d::hsurface_tool as surface_tool;
use occt_port::adaptor3d::{
    Curve3dAdaptor, CurveOnSurface, GeomAbsIsoType, GeomAbsSurfaceType, HVertex, InterFunc, IsoCurve, SurfaceAdaptor,
    TopolTool,
};
use occt_port::geom::{self, Curve, ResD1, ResD2, ResD3, SurfResD1, SurfResD2, SurfResD3};
use occt_port::geom2d::{BSplineCurve as BSplineCurve2d, BezierCurve as BezierCurve2d};
use occt_port::gp::{
    Ax1, Ax2, Ax3, Ax22d, Circ, Circ2d, Cone, Cylinder, Dir, Dir2d, Elips, Elips2d, Hypr, Hypr2d, Lin, Lin2d, Parab,
    Parab2d, Pln, Pnt, Pnt2d, Sphere, Torus, Vec, Vec2d,
};
use occt_port::math1d::{Function, FunctionWithDerivative};
use occt_port::topabs::Orientation;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex};

type AResult<T> = Result<T, AdaptorError>;

const DATA: &str = include_str!("adaptor3d.txt");
const GOLDEN_LINES: usize = 1941;
const MIN_DISTINCT: usize = 2;
const FAMILIES: &[&str] = &[
    "cos_copy",
    "cos_eval",
    "cos_info",
    "cos_intervals",
    "cos_nb_intervals",
    "cos_new",
    "cos_params",
    "def_curve2d",
    "def_curve3d",
    "def_surface",
    "hst_forward",
    "hst_g1",
    "hst_nb_samples_u",
    "hst_nb_samples_v",
    "hvertex",
    "interfunc",
    "iso_circle",
    "iso_conic",
    "iso_copy",
    "iso_eval",
    "iso_geom",
    "iso_info",
    "iso_intervals",
    "iso_line",
    "iso_nb_intervals",
    "iso_new",
    "iso_params",
    "iso_type",
    "topol_bspl_samplepnts",
    "topol_classify",
    "topol_cone_apex",
    "topol_init0",
    "topol_misc",
    "topol_on",
    "topol_restr",
    "topol_samplepnts",
    "topol_samples",
    "topol_vertices",
];
/// Families whose values come through trigonometric functions of the libm (circles, cones, spheres...): 8 ulps of the
/// largest value of the line.
const ULP_FAMILIES: &[&str] = &["cos_eval"];
const SHAPES: [GeomAbsShape; 7] = [
    GeomAbsShape::C0,
    GeomAbsShape::G1,
    GeomAbsShape::C1,
    GeomAbsShape::G2,
    GeomAbsShape::C2,
    GeomAbsShape::C3,
    GeomAbsShape::Cn,
];

#[derive(Debug, Clone, Copy)]
enum Tok {
    I(i128),
    F(f64),
}

struct Rd<'a> {
    w: std::vec::Vec<&'a str>,
    p: usize,
}
impl<'a> Rd<'a> {
    fn new(words: &[&'a str]) -> Self {
        Self { w: words.to_vec(), p: 0 }
    }
    fn next(&mut self) -> &'a str {
        let s = self.w[self.p];
        self.p += 1;
        s
    }
    fn i(&mut self) -> i32 {
        self.next().parse().unwrap()
    }
    fn b(&mut self) -> bool {
        self.i() != 0
    }
    fn f(&mut self) -> f64 {
        f64::from_bits(u64::from_str_radix(self.next(), 16).unwrap())
    }
    fn f3(&mut self) -> [f64; 3] {
        [self.f(), self.f(), self.f()]
    }
    fn vec_f(&mut self) -> std::vec::Vec<f64> {
        let n = self.i();
        (0..n).map(|_| self.f()).collect()
    }
    fn vec_i(&mut self) -> std::vec::Vec<i32> {
        let n = self.i();
        (0..n).map(|_| self.i()).collect()
    }
    fn finish(&self) {
        assert_eq!(self.p, self.w.len(), "unread input tokens");
    }
}

#[derive(Default)]
struct Out(std::vec::Vec<Tok>);
impl Out {
    fn i(&mut self, v: i64) {
        self.0.push(Tok::I(v.into()));
    }
    fn u(&mut self, v: u64) {
        self.0.push(Tok::I(v.into()));
    }
    fn f(&mut self, v: f64) {
        self.0.push(Tok::F(v));
    }
    fn b(&mut self, v: bool) {
        self.i(v.into());
    }
    fn p2(&mut self, p: &Pnt2d) {
        self.f(p.x());
        self.f(p.y());
    }
    fn d2(&mut self, d: &Dir2d) {
        self.f(d.x());
        self.f(d.y());
    }
    fn p3(&mut self, p: &Pnt) {
        self.f(p.x());
        self.f(p.y());
        self.f(p.z());
    }
    fn v3(&mut self, v: &Vec) {
        self.f(v.x());
        self.f(v.y());
        self.f(v.z());
    }
    fn d3(&mut self, d: &Dir) {
        self.f(d.x());
        self.f(d.y());
        self.f(d.z());
    }
    fn a1(&mut self, a: &Ax1) {
        self.p3(a.location());
        self.d3(a.direction());
    }
    fn a2(&mut self, a: &Ax2) {
        self.p3(a.location());
        self.d3(a.direction());
        self.d3(a.x_direction());
        self.d3(a.y_direction());
    }
    fn a3(&mut self, a: &Ax3) {
        self.p3(a.location());
        self.d3(a.direction());
        self.d3(a.x_direction());
        self.d3(a.y_direction());
    }
    fn lin(&mut self, l: &Lin) {
        self.p3(l.location());
        self.d3(l.direction());
    }
    fn circ(&mut self, c: &Circ) {
        self.a2(c.position());
        self.f(c.radius());
    }
    fn elips(&mut self, e: &Elips) {
        self.a2(e.position());
        self.f(e.major_radius());
        self.f(e.minor_radius());
    }
    fn hypr(&mut self, h: &Hypr) {
        self.a2(h.position());
        self.f(h.major_radius());
        self.f(h.minor_radius());
    }
    fn parab(&mut self, p: &Parab) {
        self.a2(p.position());
        self.f(p.focal());
    }
    fn lin2(&mut self, l: &Lin2d) {
        self.p2(l.location());
        self.d2(l.direction());
    }
    fn bez(&mut self, b: &Option<Arc<geom::BezierCurve>>) {
        self.b(b.is_none());
        let Some(b) = b else { return };
        self.i(b.nb_poles().into());
        self.b(b.is_rational());
        for i in 1..=b.nb_poles() {
            self.p3(&b.pole(i).unwrap());
            if b.is_rational() {
                self.f(b.weight(i).unwrap());
            }
        }
    }
    fn bsp(&mut self, b: &Option<Arc<geom::BSplineCurve>>) {
        self.b(b.is_none());
        let Some(b) = b else { return };
        self.i(b.degree().into());
        self.b(b.is_periodic());
        self.b(b.is_rational());
        self.i(b.nb_knots().into());
        self.i(b.nb_poles().into());
        for i in 1..=b.nb_knots() {
            self.f(b.knot(i).unwrap());
            self.i(b.multiplicity(i).unwrap().into());
        }
        for i in 1..=b.nb_poles() {
            self.p3(&b.pole(i).unwrap());
            if b.is_rational() {
                self.f(b.weight(i).unwrap());
            }
        }
    }
    fn bez_s(&mut self, b: &Option<Arc<geom::BezierSurface>>) {
        self.b(b.is_none());
        let Some(b) = b else { return };
        self.i(b.nb_u_poles().into());
        self.i(b.nb_v_poles().into());
        self.b(b.is_u_rational());
        self.b(b.is_v_rational());
        for i in 1..=b.nb_u_poles() {
            for j in 1..=b.nb_v_poles() {
                self.p3(&b.pole(i, j).unwrap());
                if b.is_u_rational() || b.is_v_rational() {
                    self.f(b.weight(i, j).unwrap());
                }
            }
        }
    }
    fn bsp_s(&mut self, b: &Option<Arc<geom::BSplineSurface>>) {
        self.b(b.is_none());
        let Some(b) = b else { return };
        self.i(b.u_degree().into());
        self.i(b.v_degree().into());
        self.i(b.nb_u_knots().into());
        self.i(b.nb_v_knots().into());
        self.i(b.nb_u_poles().into());
        self.i(b.nb_v_poles().into());
        for i in 1..=b.nb_u_knots() {
            self.f(b.u_knot(i).unwrap());
            self.i(b.u_multiplicity(i).unwrap().into());
        }
        for i in 1..=b.nb_v_knots() {
            self.f(b.v_knot(i).unwrap());
            self.i(b.v_multiplicity(i).unwrap().into());
        }
        for i in 1..=b.nb_u_poles() {
            for j in 1..=b.nb_v_poles() {
                self.p3(&b.pole(i, j).unwrap());
            }
        }
    }
    /// The call counter and the hash of a test adaptor.
    fn log(&mut self, l: (i32, u64)) {
        self.i(l.0.into());
        self.u(l.1);
    }
}

fn mixh(h: u64, x: u64) -> u64 {
    (h ^ x).wrapping_mul(0x100000001b3)
}
const H0: u64 = 0xcbf29ce484222325;

/// The state of a test adaptor: the number of calls and the hash.
#[derive(Debug)]
struct Log(Mutex<(i32, u64)>);
impl Log {
    fn new() -> Self {
        Self(Mutex::new((0, H0)))
    }
    fn touch(&self, id: u64, x: f64, y: f64) {
        let mut s = self.0.lock().unwrap();
        s.0 += 1;
        s.1 = mixh(s.1, id);
        s.1 = mixh(s.1, x.to_bits());
        s.1 = mixh(s.1, y.to_bits());
    }
    fn mix(&self, x: u64) {
        let mut s = self.0.lock().unwrap();
        s.1 = mixh(s.1, x);
    }
    fn get(&self) -> (i32, u64) {
        *self.0.lock().unwrap()
    }
}

fn shape_from_i32(v: i32) -> GeomAbsShape {
    SHAPES[v as usize]
}
fn curve_type_from_i32(v: i32) -> GeomAbsCurveType {
    match v {
        0 => GeomAbsCurveType::Line,
        1 => GeomAbsCurveType::Circle,
        2 => GeomAbsCurveType::Ellipse,
        3 => GeomAbsCurveType::Hyperbola,
        4 => GeomAbsCurveType::Parabola,
        5 => GeomAbsCurveType::BezierCurve,
        6 => GeomAbsCurveType::BSplineCurve,
        7 => GeomAbsCurveType::OffsetCurve,
        _ => GeomAbsCurveType::OtherCurve,
    }
}
fn surface_type_from_i32(v: i32) -> GeomAbsSurfaceType {
    match v {
        0 => GeomAbsSurfaceType::Plane,
        1 => GeomAbsSurfaceType::Cylinder,
        2 => GeomAbsSurfaceType::Cone,
        3 => GeomAbsSurfaceType::Sphere,
        4 => GeomAbsSurfaceType::Torus,
        5 => GeomAbsSurfaceType::BezierSurface,
        6 => GeomAbsSurfaceType::BSplineSurface,
        7 => GeomAbsSurfaceType::SurfaceOfRevolution,
        8 => GeomAbsSurfaceType::SurfaceOfExtrusion,
        9 => GeomAbsSurfaceType::OffsetSurface,
        _ => GeomAbsSurfaceType::OtherSurface,
    }
}
fn iso_from_i32(v: i32) -> GeomAbsIsoType {
    match v {
        0 => GeomAbsIsoType::IsoU,
        1 => GeomAbsIsoType::IsoV,
        _ => GeomAbsIsoType::NoneIso,
    }
}

fn put(t: &mut [f64], k: usize, v: f64) -> AResult<()> {
    *t.get_mut(k).ok_or(AdaptorError::OutOfRange)? = v;
    Ok(())
}
fn dir3(a: [f64; 3]) -> AResult<Dir> {
    Ok(Dir::new(a[0], a[1], a[2])?)
}
fn ax2_of(org: [f64; 3], zd: [f64; 3], xd: [f64; 3]) -> AResult<Ax2> {
    Ok(Ax2::new(Pnt::new(org[0], org[1], org[2]), dir3(zd)?, &dir3(xd)?)?)
}
fn ax3_of(org: [f64; 3], zd: [f64; 3], xd: [f64; 3]) -> AResult<Ax3> {
    Ok(Ax3::new(Pnt::new(org[0], org[1], org[2]), dir3(zd)?, dir3(xd)?)?)
}

// ---- the 2D test adaptor ("TC") ---------------------------------------------------------------------------------------------------
#[derive(Debug, Clone)]
struct C2Data {
    u0: f64,
    u1: f64,
    ctype: i32,
    cont: i32,
    nint: i32,
    brk: [f64; 5],
    closed: bool,
    periodic: bool,
    period: f64,
    p0: [f64; 2],
    a: [f64; 2],
    b: [f64; 2],
    c: [f64; 2],
    d: [f64; 2],
    cx: f64,
    cy: f64,
    dx: f64,
    dy: f64,
    sense: bool,
    r1: f64,
    r2: f64,
    degree: i32,
    rational: bool,
    nbpoles: i32,
    nbknots: i32,
    has_bezier: bool,
    bsp: Option<(i32, bool, std::vec::Vec<f64>, std::vec::Vec<i32>, std::vec::Vec<f64>, std::vec::Vec<f64>)>,
}
impl C2Data {
    fn read(rd: &mut Rd) -> Self {
        let (u0, u1, ctype, cont, nint) = (rd.f(), rd.f(), rd.i(), rd.i(), rd.i());
        let brk = [rd.f(), rd.f(), rd.f(), rd.f(), rd.f()];
        let (closed, periodic, period) = (rd.b(), rd.b(), rd.f());
        let p0 = [rd.f(), rd.f()];
        let a = [rd.f(), rd.f()];
        let b = [rd.f(), rd.f()];
        let c = [rd.f(), rd.f()];
        let d = [rd.f(), rd.f()];
        let (cx, cy, dx, dy, sense) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.b());
        let (r1, r2) = (rd.f(), rd.f());
        let (degree, rational, nbpoles, nbknots, has_bezier) = (rd.i(), rd.b(), rd.i(), rd.i(), rd.b());
        let bsp = if rd.b() {
            let (deg, per) = (rd.i(), rd.b());
            let k = rd.vec_f();
            let m = rd.vec_i();
            let p = rd.vec_f();
            let w = rd.vec_f();
            Some((deg, per, k, m, p, w))
        } else {
            None
        };
        Self {
            u0,
            u1,
            ctype,
            cont,
            nint,
            brk,
            closed,
            periodic,
            period,
            p0,
            a,
            b,
            c,
            d,
            cx,
            cy,
            dx,
            dy,
            sense,
            r1,
            r2,
            degree,
            rational,
            nbpoles,
            nbknots,
            has_bezier,
            bsp,
        }
    }
}

#[derive(Debug)]
struct Tc2 {
    d: C2Data,
    log: Log,
}
impl Tc2 {
    fn fresh(d: &C2Data) -> Arc<Self> {
        Arc::new(Self { d: d.clone(), log: Log::new() })
    }
    #[allow(clippy::type_complexity)]
    fn poly(&self, u: f64) -> (Pnt2d, Vec2d, Vec2d, Vec2d, Vec2d) {
        let (a, b, c, d, p0) = (&self.d.a, &self.d.b, &self.d.c, &self.d.d, &self.d.p0);
        let uu = u * u;
        let uuu = uu * u;
        (
            Pnt2d::new(
                p0[0] + u * a[0] + uu * b[0] + uuu * c[0] + uu * uu * d[0],
                p0[1] + u * a[1] + uu * b[1] + uuu * c[1] + uu * uu * d[1],
            ),
            Vec2d::new(
                a[0] + 2.0 * u * b[0] + 3.0 * uu * c[0] + 4.0 * uuu * d[0],
                a[1] + 2.0 * u * b[1] + 3.0 * uu * c[1] + 4.0 * uuu * d[1],
            ),
            Vec2d::new(2.0 * b[0] + 6.0 * u * c[0] + 12.0 * uu * d[0], 2.0 * b[1] + 6.0 * u * c[1] + 12.0 * uu * d[1]),
            Vec2d::new(6.0 * c[0] + 24.0 * u * d[0], 6.0 * c[1] + 24.0 * u * d[1]),
            Vec2d::new(24.0 * d[0], 24.0 * d[1]),
        )
    }
    fn axes(&self) -> Ax22d {
        Ax22d::from_dir(Pnt2d::new(self.d.cx, self.d.cy), Dir2d::new(self.d.dx, self.d.dy).unwrap(), self.d.sense)
    }
}
impl Curve2dAdaptor for Tc2 {
    fn shallow_copy(&self) -> AResult<Arc<dyn Curve2dAdaptor>> {
        self.log.touch(26, 0.0, 0.0);
        let (calls, hash) = self.log.get();
        let copy = Arc::new(Tc2 { d: self.d.clone(), log: Log(Mutex::new((calls, mixh(hash, 0xC0FFEE)))) });
        COPIES_TC2.lock().unwrap().push(copy.clone());
        Ok(copy)
    }
    fn first_parameter(&self) -> AResult<f64> {
        self.log.touch(1, 0.0, 0.0);
        Ok(self.d.u0)
    }
    fn last_parameter(&self) -> AResult<f64> {
        self.log.touch(2, 0.0, 0.0);
        Ok(self.d.u1)
    }
    fn continuity(&self) -> AResult<GeomAbsShape> {
        self.log.touch(3, 0.0, 0.0);
        Ok(shape_from_i32(self.d.cont))
    }
    fn nb_intervals(&self, s: GeomAbsShape) -> AResult<i32> {
        self.log.touch(4, f64::from(s as i32), 0.0);
        Ok(self.d.nint)
    }
    fn intervals(&self, t: &mut std::vec::Vec<f64>, s: GeomAbsShape) -> AResult<()> {
        self.log.touch(5, f64::from(s as i32), 0.0);
        for k in 0..=self.d.nint as usize {
            *t.get_mut(k).ok_or(AdaptorError::OutOfRange)? = self.d.brk[k];
        }
        Ok(())
    }
    fn trim(&self, first: f64, last: f64, tol: f64) -> AResult<Arc<dyn Curve2dAdaptor>> {
        self.log.touch(32, first, last);
        let (calls, hash) = self.log.get();
        let mut d = self.d.clone();
        d.u0 = first;
        d.u1 = last;
        let copy = Arc::new(Tc2 { d, log: Log(Mutex::new((calls, mixh(hash, tol.to_bits())))) });
        COPIES_TC2.lock().unwrap().push(copy.clone());
        Ok(copy)
    }
    fn is_closed(&self) -> AResult<bool> {
        self.log.touch(6, 0.0, 0.0);
        Ok(self.d.closed)
    }
    fn is_periodic(&self) -> AResult<bool> {
        self.log.touch(7, 0.0, 0.0);
        Ok(self.d.periodic)
    }
    fn period(&self) -> AResult<f64> {
        self.log.touch(8, 0.0, 0.0);
        Ok(self.d.period)
    }
    fn value(&self, u: f64) -> AResult<Pnt2d> {
        self.log.touch(9, u, 0.0);
        Ok(self.poly(u).0)
    }
    fn d0(&self, u: f64) -> AResult<Pnt2d> {
        self.log.touch(10, u, 0.0);
        Ok(self.poly(u).0)
    }
    fn d1(&self, u: f64) -> AResult<(Pnt2d, Vec2d)> {
        self.log.touch(11, u, 0.0);
        let r = self.poly(u);
        Ok((r.0, r.1))
    }
    fn d2(&self, u: f64) -> AResult<(Pnt2d, Vec2d, Vec2d)> {
        self.log.touch(12, u, 0.0);
        let r = self.poly(u);
        Ok((r.0, r.1, r.2))
    }
    fn d3(&self, u: f64) -> AResult<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        self.log.touch(13, u, 0.0);
        let r = self.poly(u);
        Ok((r.0, r.1, r.2, r.3))
    }
    fn dn(&self, u: f64, n: i32) -> AResult<Vec2d> {
        self.log.touch(14, u, f64::from(n));
        if n <= 0 {
            return Err(AdaptorError::OutOfRange);
        }
        let r = self.poly(u);
        Ok(match n {
            1 => r.1,
            2 => r.2,
            3 => r.3,
            4 => r.4,
            _ => Vec2d::new(0.0, 0.0),
        })
    }
    fn get_type(&self) -> AResult<GeomAbsCurveType> {
        self.log.touch(15, 0.0, 0.0);
        Ok(curve_type_from_i32(self.d.ctype))
    }
    fn line(&self) -> AResult<Lin2d> {
        self.log.touch(16, 0.0, 0.0);
        Ok(Lin2d::new(Pnt2d::new(self.d.cx, self.d.cy), Dir2d::new(self.d.dx, self.d.dy)?))
    }
    fn circle(&self) -> AResult<Circ2d> {
        self.log.touch(17, 0.0, 0.0);
        Ok(Circ2d::from_ax22d(self.axes(), self.d.r1)?)
    }
    fn ellipse(&self) -> AResult<Elips2d> {
        self.log.touch(18, 0.0, 0.0);
        Ok(Elips2d::from_ax22d(self.axes(), self.d.r1, self.d.r2)?)
    }
    fn hyperbola(&self) -> AResult<Hypr2d> {
        self.log.touch(19, 0.0, 0.0);
        Ok(Hypr2d::from_ax22d(self.axes(), self.d.r1, self.d.r2)?)
    }
    fn parabola(&self) -> AResult<Parab2d> {
        self.log.touch(20, 0.0, 0.0);
        Ok(Parab2d::from_ax22d(self.axes(), self.d.r1)?)
    }
    fn degree(&self) -> AResult<i32> {
        self.log.touch(21, 0.0, 0.0);
        Ok(self.d.degree)
    }
    fn is_rational(&self) -> AResult<bool> {
        self.log.touch(22, 0.0, 0.0);
        Ok(self.d.rational)
    }
    fn nb_poles(&self) -> AResult<i32> {
        self.log.touch(23, 0.0, 0.0);
        Ok(self.d.nbpoles)
    }
    fn nb_knots(&self) -> AResult<i32> {
        self.log.touch(24, 0.0, 0.0);
        Ok(self.d.nbknots)
    }
    fn bezier(&self) -> AResult<Option<Arc<BezierCurve2d>>> {
        self.log.touch(25, 0.0, 0.0);
        if !self.d.has_bezier {
            return Err(AdaptorError::NotImplemented);
        }
        let (p0, a, b) = (&self.d.p0, &self.d.a, &self.d.b);
        let poles =
            [Pnt2d::new(p0[0], p0[1]), Pnt2d::new(p0[0] + a[0], p0[1] + a[1]), Pnt2d::new(p0[0] + b[0], p0[1] + b[1])];
        Ok(Some(Arc::new(BezierCurve2d::new(&poles)?)))
    }
    fn bspline(&self) -> AResult<Option<Arc<BSplineCurve2d>>> {
        self.log.touch(31, 0.0, 0.0);
        let Some((deg, per, k, m, p, w)) = &self.d.bsp else {
            return Err(AdaptorError::NotImplemented);
        };
        let poles: std::vec::Vec<Pnt2d> = p.chunks(2).map(|c| Pnt2d::new(c[0], c[1])).collect();
        Ok(Some(Arc::new(if w.is_empty() {
            BSplineCurve2d::new(&poles, k, m, *deg, *per)?
        } else {
            BSplineCurve2d::with_weights(&poles, w, k, m, *deg, *per)?
        })))
    }
}

// ---- the 3D curve test adaptor ("TC3") -----------------------------------------------------------------------------------------
#[derive(Debug, Clone)]
struct BspC {
    deg: i32,
    per: bool,
    k: std::vec::Vec<f64>,
    m: std::vec::Vec<i32>,
    p: std::vec::Vec<f64>,
    w: std::vec::Vec<f64>,
}
#[derive(Debug, Clone)]
struct C3Data {
    u0: f64,
    u1: f64,
    ctype: i32,
    cont: i32,
    nint: i32,
    brk: [f64; 5],
    closed: bool,
    periodic: bool,
    period: f64,
    rf: f64,
    c: [[f64; 3]; 5],
    org: [f64; 3],
    zd: [f64; 3],
    xd: [f64; 3],
    ld: [f64; 3],
    r1: f64,
    r2: f64,
    degree: i32,
    rational: bool,
    nbpoles: i32,
    nbknots: i32,
    bez: Option<(std::vec::Vec<f64>, std::vec::Vec<f64>)>,
    bsp: Option<BspC>,
    off: Option<(f64, [f64; 3])>,
}
impl C3Data {
    fn read(rd: &mut Rd) -> Self {
        let (u0, u1, ctype, cont, nint) = (rd.f(), rd.f(), rd.i(), rd.i(), rd.i());
        let brk = [rd.f(), rd.f(), rd.f(), rd.f(), rd.f()];
        let (closed, periodic, period, rf) = (rd.b(), rd.b(), rd.f(), rd.f());
        let mut c = [[0.0; 3]; 5];
        for row in &mut c {
            *row = rd.f3();
        }
        let (org, zd, xd, ld) = (rd.f3(), rd.f3(), rd.f3(), rd.f3());
        let (r1, r2) = (rd.f(), rd.f());
        let (degree, rational, nbpoles, nbknots) = (rd.i(), rd.b(), rd.i(), rd.i());
        let bez = if rd.b() {
            let p = rd.vec_f();
            let w = rd.vec_f();
            Some((p, w))
        } else {
            None
        };
        let bsp = if rd.b() {
            let (deg, per) = (rd.i(), rd.b());
            let k = rd.vec_f();
            let m = rd.vec_i();
            let p = rd.vec_f();
            let w = rd.vec_f();
            Some(BspC { deg, per, k, m, p, w })
        } else {
            None
        };
        let off = if rd.b() { Some((rd.f(), rd.f3())) } else { None };
        Self {
            u0,
            u1,
            ctype,
            cont,
            nint,
            brk,
            closed,
            periodic,
            period,
            rf,
            c,
            org,
            zd,
            xd,
            ld,
            r1,
            r2,
            degree,
            rational,
            nbpoles,
            nbknots,
            bez,
            bsp,
            off,
        }
    }
}

fn pts3(p: &[f64]) -> std::vec::Vec<Pnt> {
    p.chunks(3).map(|c| Pnt::new(c[0], c[1], c[2])).collect()
}

#[derive(Debug)]
struct Tc3 {
    d: C3Data,
    log: Log,
}
impl Tc3 {
    fn fresh(d: &C3Data) -> Arc<Self> {
        Arc::new(Self { d: d.clone(), log: Log::new() })
    }
    fn poly(&self, u: f64) -> (Pnt, [Vec; 4]) {
        let c = &self.d.c;
        let uu = u * u;
        let uuu = uu * u;
        let u4 = uu * uu;
        let mut x = [0.0; 3];
        let mut d1 = [0.0; 3];
        let mut d2 = [0.0; 3];
        let mut d3 = [0.0; 3];
        let mut d4 = [0.0; 3];
        for k in 0..3 {
            x[k] = c[0][k] + u * c[1][k] + uu * c[2][k] + uuu * c[3][k] + u4 * c[4][k];
            d1[k] = c[1][k] + 2.0 * u * c[2][k] + 3.0 * uu * c[3][k] + 4.0 * uuu * c[4][k];
            d2[k] = 2.0 * c[2][k] + 6.0 * u * c[3][k] + 12.0 * uu * c[4][k];
            d3[k] = 6.0 * c[3][k] + 24.0 * u * c[4][k];
            d4[k] = 24.0 * c[4][k];
        }
        (
            Pnt::new(x[0], x[1], x[2]),
            [
                Vec::new(d1[0], d1[1], d1[2]),
                Vec::new(d2[0], d2[1], d2[2]),
                Vec::new(d3[0], d3[1], d3[2]),
                Vec::new(d4[0], d4[1], d4[2]),
            ],
        )
    }
    fn ax(&self) -> AResult<Ax2> {
        ax2_of(self.d.org, self.d.zd, self.d.xd)
    }
    fn make_bsp(&self) -> AResult<Arc<geom::BSplineCurve>> {
        let b = self.d.bsp.as_ref().ok_or(AdaptorError::NotImplemented)?;
        let poles = pts3(&b.p);
        Ok(Arc::new(if b.w.is_empty() {
            geom::BSplineCurve::new(&poles, &b.k, &b.m, b.deg, b.per)?
        } else {
            geom::BSplineCurve::with_weights(&poles, &b.w, &b.k, &b.m, b.deg, b.per)?
        }))
    }
}
impl Curve3dAdaptor for Tc3 {
    fn shallow_copy(&self) -> AResult<Arc<dyn Curve3dAdaptor>> {
        self.log.touch(26, 0.0, 0.0);
        let (calls, hash) = self.log.get();
        Ok(Arc::new(Tc3 { d: self.d.clone(), log: Log(Mutex::new((calls, mixh(hash, 0xC0FFEE)))) }))
    }
    fn first_parameter(&self) -> AResult<f64> {
        self.log.touch(1, 0.0, 0.0);
        Ok(self.d.u0)
    }
    fn last_parameter(&self) -> AResult<f64> {
        self.log.touch(2, 0.0, 0.0);
        Ok(self.d.u1)
    }
    fn continuity(&self) -> AResult<GeomAbsShape> {
        self.log.touch(3, 0.0, 0.0);
        Ok(shape_from_i32(self.d.cont))
    }
    fn nb_intervals(&self, s: GeomAbsShape) -> AResult<i32> {
        self.log.touch(4, f64::from(s as i32), 0.0);
        Ok(self.d.nint)
    }
    fn intervals(&self, t: &mut [f64], s: GeomAbsShape) -> AResult<()> {
        self.log.touch(5, f64::from(s as i32), 0.0);
        for k in 0..=self.d.nint as usize {
            put(t, k, self.d.brk[k])?;
        }
        Ok(())
    }
    fn trim(&self, first: f64, last: f64, tol: f64) -> AResult<Arc<dyn Curve3dAdaptor>> {
        self.log.touch(27, first, last);
        let (calls, hash) = self.log.get();
        let mut d = self.d.clone();
        d.u0 = first;
        d.u1 = last;
        Ok(Arc::new(Tc3 { d, log: Log(Mutex::new((calls, mixh(hash, tol.to_bits())))) }))
    }
    fn is_closed(&self) -> AResult<bool> {
        self.log.touch(6, 0.0, 0.0);
        Ok(self.d.closed)
    }
    fn is_periodic(&self) -> AResult<bool> {
        self.log.touch(7, 0.0, 0.0);
        Ok(self.d.periodic)
    }
    fn period(&self) -> AResult<f64> {
        self.log.touch(8, 0.0, 0.0);
        Ok(self.d.period)
    }
    fn resolution(&self, r3d: f64) -> AResult<f64> {
        self.log.touch(28, r3d, 0.0);
        Ok(r3d * self.d.rf)
    }
    fn eval_d0(&self, u: f64) -> AResult<Pnt> {
        self.log.touch(10, u, 0.0);
        Ok(self.poly(u).0)
    }
    fn eval_d1(&self, u: f64) -> AResult<ResD1> {
        self.log.touch(11, u, 0.0);
        let (p, v) = self.poly(u);
        Ok(ResD1 { point: p, d1: v[0] })
    }
    fn eval_d2(&self, u: f64) -> AResult<ResD2> {
        self.log.touch(12, u, 0.0);
        let (p, v) = self.poly(u);
        Ok(ResD2 { point: p, d1: v[0], d2: v[1] })
    }
    fn eval_d3(&self, u: f64) -> AResult<ResD3> {
        self.log.touch(13, u, 0.0);
        let (p, v) = self.poly(u);
        Ok(ResD3 { point: p, d1: v[0], d2: v[1], d3: v[2] })
    }
    fn eval_dn(&self, u: f64, n: i32) -> AResult<Vec> {
        self.log.touch(14, u, f64::from(n));
        if n <= 0 {
            return Err(AdaptorError::OutOfRange);
        }
        let (_, v) = self.poly(u);
        Ok(if n <= 4 { v[(n - 1) as usize] } else { Vec::new(0.0, 0.0, 0.0) })
    }
    fn get_type(&self) -> AResult<GeomAbsCurveType> {
        self.log.touch(15, 0.0, 0.0);
        Ok(curve_type_from_i32(self.d.ctype))
    }
    fn line(&self) -> AResult<Lin> {
        self.log.touch(16, 0.0, 0.0);
        Ok(Lin::new(Pnt::new(self.d.org[0], self.d.org[1], self.d.org[2]), dir3(self.d.ld)?))
    }
    fn circle(&self) -> AResult<Circ> {
        self.log.touch(17, 0.0, 0.0);
        Ok(Circ::new(self.ax()?, self.d.r1)?)
    }
    fn ellipse(&self) -> AResult<Elips> {
        self.log.touch(18, 0.0, 0.0);
        Ok(Elips::new(self.ax()?, self.d.r1, self.d.r2)?)
    }
    fn hyperbola(&self) -> AResult<Hypr> {
        self.log.touch(19, 0.0, 0.0);
        Ok(Hypr::new(self.ax()?, self.d.r1, self.d.r2)?)
    }
    fn parabola(&self) -> AResult<Parab> {
        self.log.touch(20, 0.0, 0.0);
        Ok(Parab::new(self.ax()?, self.d.r1)?)
    }
    fn degree(&self) -> AResult<i32> {
        self.log.touch(21, 0.0, 0.0);
        Ok(self.d.degree)
    }
    fn is_rational(&self) -> AResult<bool> {
        self.log.touch(22, 0.0, 0.0);
        Ok(self.d.rational)
    }
    fn nb_poles(&self) -> AResult<i32> {
        self.log.touch(23, 0.0, 0.0);
        Ok(self.d.nbpoles)
    }
    fn nb_knots(&self) -> AResult<i32> {
        self.log.touch(24, 0.0, 0.0);
        Ok(self.d.nbknots)
    }
    fn bezier(&self) -> AResult<Option<Arc<geom::BezierCurve>>> {
        self.log.touch(25, 0.0, 0.0);
        let Some((p, w)) = &self.d.bez else {
            return Err(AdaptorError::NotImplemented);
        };
        let poles = pts3(p);
        Ok(Some(Arc::new(if w.is_empty() {
            geom::BezierCurve::new(&poles)?
        } else {
            geom::BezierCurve::with_weights(&poles, w)?
        })))
    }
    fn bspline(&self) -> AResult<Option<Arc<geom::BSplineCurve>>> {
        self.log.touch(29, 0.0, 0.0);
        if self.d.bsp.is_none() {
            return Err(AdaptorError::NotImplemented);
        }
        Ok(Some(self.make_bsp()?))
    }
    fn offset_curve(&self) -> AResult<Option<Arc<geom::OffsetCurve>>> {
        self.log.touch(30, 0.0, 0.0);
        let Some((off, dir)) = &self.d.off else {
            return Err(AdaptorError::NotImplemented);
        };
        let basis: Arc<dyn geom::Curve> = if self.d.bsp.is_some() {
            self.make_bsp()?
        } else {
            Arc::new(geom::Line::new(Pnt::new(self.d.org[0], self.d.org[1], self.d.org[2]), dir3(self.d.ld)?))
        };
        Ok(Some(Arc::new(geom::OffsetCurve::new(basis, *off, dir3(*dir)?, false)?)))
    }
}

// ---- the surface test adaptor ("TS") ----------------------------------------------------------------------------------------
const MI: [i32; 10] = [0, 1, 0, 2, 1, 0, 3, 2, 1, 0];
const MJ: [i32; 10] = [0, 0, 1, 0, 1, 2, 0, 1, 2, 3];
fn falling(i: i32, n: i32) -> f64 {
    let mut r = 1.0;
    for k in 0..n {
        r *= f64::from(i - k);
    }
    r
}
fn powi(x: f64, e: i32) -> f64 {
    let mut r = 1.0;
    for _ in 0..e {
        r *= x;
    }
    r
}

#[derive(Debug, Clone)]
struct BezS {
    nu: usize,
    nv: usize,
    p: std::vec::Vec<f64>,
    w: std::vec::Vec<f64>,
}
#[derive(Debug, Clone)]
struct BspS {
    ud: i32,
    vd: i32,
    up: bool,
    vp: bool,
    nu: usize,
    nv: usize,
    uk: std::vec::Vec<f64>,
    vk: std::vec::Vec<f64>,
    um: std::vec::Vec<i32>,
    vm: std::vec::Vec<i32>,
    p: std::vec::Vec<f64>,
    w: std::vec::Vec<f64>,
}
#[derive(Debug, Clone)]
struct SData {
    u0: f64,
    u1: f64,
    v0: f64,
    v1: f64,
    ucont: i32,
    vcont: i32,
    nu: i32,
    nv: i32,
    ubrk: [f64; 5],
    vbrk: [f64; 5],
    uclosed: bool,
    vclosed: bool,
    uperiodic: bool,
    vperiodic: bool,
    uperiod: f64,
    vperiod: f64,
    ures: f64,
    vres: f64,
    stype: i32,
    cf: [[f64; 3]; 10],
    org: [f64; 3],
    zd: [f64; 3],
    xd: [f64; 3],
    r1: f64,
    r2: f64,
    semi: f64,
    ap: [f64; 3],
    ad: [f64; 3],
    ed: [f64; 3],
    offv: f64,
    udeg: i32,
    nupoles: i32,
    vdeg: i32,
    nvpoles: i32,
    nuknots: i32,
    nvknots: i32,
    urat: bool,
    vrat: bool,
    basis_curve: i32,
    basis_surf: i32,
    bez: Option<BezS>,
    bsp: Option<BspS>,
}
impl SData {
    fn read(rd: &mut Rd) -> Self {
        let (u0, u1, v0, v1) = (rd.f(), rd.f(), rd.f(), rd.f());
        let (ucont, vcont, nu, nv) = (rd.i(), rd.i(), rd.i(), rd.i());
        let ubrk = [rd.f(), rd.f(), rd.f(), rd.f(), rd.f()];
        let vbrk = [rd.f(), rd.f(), rd.f(), rd.f(), rd.f()];
        let (uclosed, vclosed, uperiodic, vperiodic) = (rd.b(), rd.b(), rd.b(), rd.b());
        let (uperiod, vperiod, ures, vres) = (rd.f(), rd.f(), rd.f(), rd.f());
        let stype = rd.i();
        let mut cf = [[0.0; 3]; 10];
        for row in &mut cf {
            *row = rd.f3();
        }
        let (org, zd, xd) = (rd.f3(), rd.f3(), rd.f3());
        let (r1, r2, semi) = (rd.f(), rd.f(), rd.f());
        let (ap, ad, ed) = (rd.f3(), rd.f3(), rd.f3());
        let offv = rd.f();
        let (udeg, nupoles, vdeg, nvpoles, nuknots, nvknots) = (rd.i(), rd.i(), rd.i(), rd.i(), rd.i(), rd.i());
        let (urat, vrat) = (rd.b(), rd.b());
        let (basis_curve, basis_surf) = (rd.i(), rd.i());
        let bez = if rd.b() {
            let (nu, nv) = (rd.i() as usize, rd.i() as usize);
            let p = rd.vec_f();
            let w = rd.vec_f();
            Some(BezS { nu, nv, p, w })
        } else {
            None
        };
        let bsp = if rd.b() {
            let (ud, vd, up, vp) = (rd.i(), rd.i(), rd.b(), rd.b());
            let (nu, nv) = (rd.i() as usize, rd.i() as usize);
            let uk = rd.vec_f();
            let vk = rd.vec_f();
            let um = rd.vec_i();
            let vm = rd.vec_i();
            let p = rd.vec_f();
            let w = rd.vec_f();
            Some(BspS { ud, vd, up, vp, nu, nv, uk, vk, um, vm, p, w })
        } else {
            None
        };
        Self {
            u0,
            u1,
            v0,
            v1,
            ucont,
            vcont,
            nu,
            nv,
            ubrk,
            vbrk,
            uclosed,
            vclosed,
            uperiodic,
            vperiodic,
            uperiod,
            vperiod,
            ures,
            vres,
            stype,
            cf,
            org,
            zd,
            xd,
            r1,
            r2,
            semi,
            ap,
            ad,
            ed,
            offv,
            udeg,
            nupoles,
            vdeg,
            nvpoles,
            nuknots,
            nvknots,
            urat,
            vrat,
            basis_curve,
            basis_surf,
            bez,
            bsp,
        }
    }
}

#[derive(Debug)]
struct Ts {
    d: SData,
    bc: Option<Arc<Tc3>>,
    bs: Option<Arc<Ts>>,
    log: Log,
}
impl Ts {
    fn deriv(&self, nu: i32, nv: i32, u: f64, v: f64) -> Vec {
        let mut x = [0.0; 3];
        for k in 0..10 {
            if MI[k] < nu || MJ[k] < nv {
                continue;
            }
            let w = falling(MI[k], nu) * powi(u, MI[k] - nu) * (falling(MJ[k], nv) * powi(v, MJ[k] - nv));
            for (xm, c) in x.iter_mut().zip(self.d.cf[k]) {
                *xm += w * c;
            }
        }
        Vec::new(x[0], x[1], x[2])
    }
    fn ax(&self) -> AResult<Ax3> {
        ax3_of(self.d.org, self.d.zd, self.d.xd)
    }
    fn point(&self, u: f64, v: f64) -> Pnt {
        let p = self.deriv(0, 0, u, v);
        Pnt::new(p.x(), p.y(), p.z())
    }
}
impl SurfaceAdaptor for Ts {
    fn shallow_copy(&self) -> AResult<Arc<dyn SurfaceAdaptor>> {
        self.log.touch(47, 0.0, 0.0);
        let (calls, hash) = self.log.get();
        let copy = Arc::new(Ts {
            d: self.d.clone(),
            bc: self.bc.clone(),
            bs: self.bs.clone(),
            log: Log(Mutex::new((calls, mixh(hash, 0xC0FFEE)))),
        });
        COPIES_TS.lock().unwrap().push(copy.clone());
        Ok(copy)
    }
    fn first_u_parameter(&self) -> AResult<f64> {
        self.log.touch(1, 0.0, 0.0);
        Ok(self.d.u0)
    }
    fn last_u_parameter(&self) -> AResult<f64> {
        self.log.touch(2, 0.0, 0.0);
        Ok(self.d.u1)
    }
    fn first_v_parameter(&self) -> AResult<f64> {
        self.log.touch(3, 0.0, 0.0);
        Ok(self.d.v0)
    }
    fn last_v_parameter(&self) -> AResult<f64> {
        self.log.touch(4, 0.0, 0.0);
        Ok(self.d.v1)
    }
    fn u_continuity(&self) -> AResult<GeomAbsShape> {
        self.log.touch(5, 0.0, 0.0);
        Ok(shape_from_i32(self.d.ucont))
    }
    fn v_continuity(&self) -> AResult<GeomAbsShape> {
        self.log.touch(6, 0.0, 0.0);
        Ok(shape_from_i32(self.d.vcont))
    }
    fn nb_u_intervals(&self, s: GeomAbsShape) -> AResult<i32> {
        self.log.touch(7, f64::from(s as i32), 0.0);
        Ok(self.d.nu)
    }
    fn nb_v_intervals(&self, s: GeomAbsShape) -> AResult<i32> {
        self.log.touch(8, f64::from(s as i32), 0.0);
        Ok(self.d.nv)
    }
    fn u_intervals(&self, t: &mut [f64], s: GeomAbsShape) -> AResult<()> {
        self.log.touch(9, f64::from(s as i32), 0.0);
        for k in 0..=self.d.nu as usize {
            put(t, k, self.d.ubrk[k])?;
        }
        Ok(())
    }
    fn v_intervals(&self, t: &mut [f64], s: GeomAbsShape) -> AResult<()> {
        self.log.touch(10, f64::from(s as i32), 0.0);
        for k in 0..=self.d.nv as usize {
            put(t, k, self.d.vbrk[k])?;
        }
        Ok(())
    }
    fn u_trim(&self, first: f64, last: f64, tol: f64) -> AResult<Arc<dyn SurfaceAdaptor>> {
        self.log.touch(11, first, last);
        let (calls, hash) = self.log.get();
        let mut d = self.d.clone();
        d.u0 = first;
        d.u1 = last;
        Ok(Arc::new(Ts {
            d,
            bc: self.bc.clone(),
            bs: self.bs.clone(),
            log: Log(Mutex::new((calls, mixh(hash, tol.to_bits())))),
        }))
    }
    fn v_trim(&self, first: f64, last: f64, tol: f64) -> AResult<Arc<dyn SurfaceAdaptor>> {
        self.log.touch(12, first, last);
        let (calls, hash) = self.log.get();
        let mut d = self.d.clone();
        d.v0 = first;
        d.v1 = last;
        Ok(Arc::new(Ts {
            d,
            bc: self.bc.clone(),
            bs: self.bs.clone(),
            log: Log(Mutex::new((calls, mixh(hash, tol.to_bits())))),
        }))
    }
    fn is_u_closed(&self) -> AResult<bool> {
        self.log.touch(13, 0.0, 0.0);
        Ok(self.d.uclosed)
    }
    fn is_v_closed(&self) -> AResult<bool> {
        self.log.touch(14, 0.0, 0.0);
        Ok(self.d.vclosed)
    }
    fn is_u_periodic(&self) -> AResult<bool> {
        self.log.touch(15, 0.0, 0.0);
        Ok(self.d.uperiodic)
    }
    fn u_period(&self) -> AResult<f64> {
        self.log.touch(16, 0.0, 0.0);
        Ok(self.d.uperiod)
    }
    fn is_v_periodic(&self) -> AResult<bool> {
        self.log.touch(17, 0.0, 0.0);
        Ok(self.d.vperiodic)
    }
    fn v_period(&self) -> AResult<f64> {
        self.log.touch(18, 0.0, 0.0);
        Ok(self.d.vperiod)
    }
    fn u_resolution(&self, r3d: f64) -> AResult<f64> {
        self.log.touch(19, r3d, 0.0);
        Ok(r3d * self.d.ures)
    }
    fn v_resolution(&self, r3d: f64) -> AResult<f64> {
        self.log.touch(20, r3d, 0.0);
        Ok(r3d * self.d.vres)
    }
    fn get_type(&self) -> AResult<GeomAbsSurfaceType> {
        self.log.touch(21, 0.0, 0.0);
        Ok(surface_type_from_i32(self.d.stype))
    }
    fn plane(&self) -> AResult<Pln> {
        self.log.touch(22, 0.0, 0.0);
        Ok(Pln::new(self.ax()?))
    }
    fn cylinder(&self) -> AResult<Cylinder> {
        self.log.touch(23, 0.0, 0.0);
        Ok(Cylinder::new(self.ax()?, self.d.r1)?)
    }
    fn cone(&self) -> AResult<Cone> {
        self.log.touch(24, 0.0, 0.0);
        Ok(Cone::new(self.ax()?, self.d.semi, self.d.r1)?)
    }
    fn sphere(&self) -> AResult<Sphere> {
        self.log.touch(25, 0.0, 0.0);
        Ok(Sphere::new(self.ax()?, self.d.r1)?)
    }
    fn torus(&self) -> AResult<Torus> {
        self.log.touch(26, 0.0, 0.0);
        Ok(Torus::new(self.ax()?, self.d.r1, self.d.r2)?)
    }
    fn u_degree(&self) -> AResult<i32> {
        self.log.touch(27, 0.0, 0.0);
        Ok(self.d.udeg)
    }
    fn nb_u_poles(&self) -> AResult<i32> {
        self.log.touch(28, 0.0, 0.0);
        Ok(self.d.nupoles)
    }
    fn v_degree(&self) -> AResult<i32> {
        self.log.touch(29, 0.0, 0.0);
        Ok(self.d.vdeg)
    }
    fn nb_v_poles(&self) -> AResult<i32> {
        self.log.touch(30, 0.0, 0.0);
        Ok(self.d.nvpoles)
    }
    fn nb_u_knots(&self) -> AResult<i32> {
        self.log.touch(31, 0.0, 0.0);
        Ok(self.d.nuknots)
    }
    fn nb_v_knots(&self) -> AResult<i32> {
        self.log.touch(32, 0.0, 0.0);
        Ok(self.d.nvknots)
    }
    fn is_u_rational(&self) -> AResult<bool> {
        self.log.touch(33, 0.0, 0.0);
        Ok(self.d.urat)
    }
    fn is_v_rational(&self) -> AResult<bool> {
        self.log.touch(34, 0.0, 0.0);
        Ok(self.d.vrat)
    }
    fn bezier(&self) -> AResult<Option<Arc<geom::BezierSurface>>> {
        self.log.touch(35, 0.0, 0.0);
        let Some(b) = &self.d.bez else {
            return Err(AdaptorError::NotImplemented);
        };
        let poles: std::vec::Vec<std::vec::Vec<Pnt>> = (0..b.nu)
            .map(|i| {
                (0..b.nv)
                    .map(|j| {
                        Pnt::new(b.p[3 * (i * b.nv + j)], b.p[3 * (i * b.nv + j) + 1], b.p[3 * (i * b.nv + j) + 2])
                    })
                    .collect()
            })
            .collect();
        Ok(Some(Arc::new(if b.w.is_empty() {
            geom::BezierSurface::new(&poles)?
        } else {
            let w: std::vec::Vec<std::vec::Vec<f64>> =
                (0..b.nu).map(|i| (0..b.nv).map(|j| b.w[i * b.nv + j]).collect()).collect();
            geom::BezierSurface::with_weights(&poles, &w)?
        })))
    }
    fn bspline(&self) -> AResult<Option<Arc<geom::BSplineSurface>>> {
        self.log.touch(36, 0.0, 0.0);
        let Some(b) = &self.d.bsp else {
            return Err(AdaptorError::NotImplemented);
        };
        let poles: std::vec::Vec<std::vec::Vec<Pnt>> = (0..b.nu)
            .map(|i| {
                (0..b.nv)
                    .map(|j| {
                        Pnt::new(b.p[3 * (i * b.nv + j)], b.p[3 * (i * b.nv + j) + 1], b.p[3 * (i * b.nv + j) + 2])
                    })
                    .collect()
            })
            .collect();
        Ok(Some(Arc::new(if b.w.is_empty() {
            geom::BSplineSurface::new(&poles, &b.uk, &b.vk, &b.um, &b.vm, b.ud, b.vd, b.up, b.vp)?
        } else {
            let w: std::vec::Vec<std::vec::Vec<f64>> =
                (0..b.nu).map(|i| (0..b.nv).map(|j| b.w[i * b.nv + j]).collect()).collect();
            geom::BSplineSurface::with_weights(&poles, &w, &b.uk, &b.vk, &b.um, &b.vm, b.ud, b.vd, b.up, b.vp)?
        })))
    }
    fn axe_of_revolution(&self) -> AResult<Ax1> {
        self.log.touch(37, 0.0, 0.0);
        Ok(Ax1::new(Pnt::new(self.d.ap[0], self.d.ap[1], self.d.ap[2]), dir3(self.d.ad)?))
    }
    fn direction(&self) -> AResult<Dir> {
        self.log.touch(38, 0.0, 0.0);
        dir3(self.d.ed)
    }
    fn basis_curve(&self) -> AResult<Option<Arc<dyn Curve3dAdaptor>>> {
        self.log.touch(39, 0.0, 0.0);
        Ok(self.bc.clone().map(|c| c as Arc<dyn Curve3dAdaptor>))
    }
    fn basis_surface(&self) -> AResult<Option<Arc<dyn SurfaceAdaptor>>> {
        self.log.touch(40, 0.0, 0.0);
        Ok(self.bs.clone().map(|c| c as Arc<dyn SurfaceAdaptor>))
    }
    fn offset_value(&self) -> AResult<f64> {
        self.log.touch(41, 0.0, 0.0);
        Ok(self.d.offv)
    }
    fn eval_d0(&self, u: f64, v: f64) -> AResult<Pnt> {
        self.log.touch(42, u, v);
        Ok(self.point(u, v))
    }
    fn eval_d1(&self, u: f64, v: f64) -> AResult<SurfResD1> {
        self.log.touch(43, u, v);
        Ok(SurfResD1 { point: self.point(u, v), d1u: self.deriv(1, 0, u, v), d1v: self.deriv(0, 1, u, v) })
    }
    fn eval_d2(&self, u: f64, v: f64) -> AResult<SurfResD2> {
        self.log.touch(44, u, v);
        Ok(SurfResD2 {
            point: self.point(u, v),
            d1u: self.deriv(1, 0, u, v),
            d1v: self.deriv(0, 1, u, v),
            d2u: self.deriv(2, 0, u, v),
            d2v: self.deriv(0, 2, u, v),
            d2uv: self.deriv(1, 1, u, v),
        })
    }
    fn eval_d3(&self, u: f64, v: f64) -> AResult<SurfResD3> {
        self.log.touch(45, u, v);
        Ok(SurfResD3 {
            point: self.point(u, v),
            d1u: self.deriv(1, 0, u, v),
            d1v: self.deriv(0, 1, u, v),
            d2u: self.deriv(2, 0, u, v),
            d2v: self.deriv(0, 2, u, v),
            d2uv: self.deriv(1, 1, u, v),
            d3u: self.deriv(3, 0, u, v),
            d3v: self.deriv(0, 3, u, v),
            d3uuv: self.deriv(2, 1, u, v),
            d3uvv: self.deriv(1, 2, u, v),
        })
    }
    fn eval_dn(&self, u: f64, v: f64, a: i32, b: i32) -> AResult<Vec> {
        self.log.touch(46, u, v);
        self.log.mix((i64::from(a) * 64 + i64::from(b)) as u64);
        if a < 0 || b < 0 || a + b < 1 {
            return Err(AdaptorError::OutOfRange);
        }
        Ok(self.deriv(a, b, u, v))
    }
}

/// The definitions printed by the families `def_*`, in order.
#[derive(Default)]
struct World {
    c3: std::vec::Vec<C3Data>,
    s: std::vec::Vec<SData>,
    c2: std::vec::Vec<C2Data>,
}
impl World {
    fn curve3(&self, i: usize) -> Arc<Tc3> {
        Tc3::fresh(&self.c3[i])
    }
    fn curve2(&self, i: usize) -> Arc<Tc2> {
        Tc2::fresh(&self.c2[i])
    }
    /// A surface with its counters at zero; the nested adaptors are fresh as well (as `TS::fresh` of the oracle).
    fn surface(&self, i: usize) -> Arc<Ts> {
        self.build_surface(&self.s[i])
    }
    fn build_surface(&self, d: &SData) -> Arc<Ts> {
        let bc = (d.basis_curve >= 0).then(|| self.curve3(d.basis_curve as usize));
        let bs = (d.basis_surf >= 0).then(|| self.surface(d.basis_surf as usize));
        Arc::new(Ts { d: d.clone(), bc, bs, log: Log::new() })
    }
}
fn ts_log(o: &mut Out, t: &Ts) {
    o.log(t.log.get());
    if let Some(c) = &t.bc {
        o.log(c.log.get());
    }
    if let Some(s) = &t.bs {
        ts_log(o, s);
    }
}

// ---- copies made by the classes: the oracle prints the counters of the copy as well -------------------------------------------------------
static COPIES_TS: Mutex<std::vec::Vec<Arc<Ts>>> = Mutex::new(std::vec::Vec::new());
static COPIES_TC2: Mutex<std::vec::Vec<Arc<Tc2>>> = Mutex::new(std::vec::Vec::new());

fn find_ts(a: *const ()) -> Arc<Ts> {
    COPIES_TS.lock().unwrap().iter().find(|t| addr(t) == a).cloned().expect("a copy of the surface adaptor")
}
fn find_tc2(a: *const ()) -> Arc<Tc2> {
    COPIES_TC2.lock().unwrap().iter().find(|t| addr(t) == a).cloned().expect("a copy of the curve adaptor")
}

fn addr<T: ?Sized>(a: &Arc<T>) -> *const () {
    Arc::as_ptr(a).cast::<()>()
}

fn run_iso_inputs(rd: &mut Rd) -> (usize, i32, f64, f64, f64) {
    (rd.i() as usize, rd.i(), rd.f(), rd.f(), rd.f())
}

/// The iso curve of the surface `s`: the iso curve of a surface without the kind (`iso == 2`), or loaded with the given data.
fn make_iso(world: &World, s: usize, iso: i32, p: f64, wf: f64, wl: f64) -> AResult<(Arc<Ts>, IsoCurve)> {
    let surf = world.surface(s);
    let mut c = IsoCurve::with_surface(surf.clone());
    if iso != 2 {
        c.load_iso_range(iso_from_i32(iso), p, wf, wl)?;
    }
    Ok((surf, c))
}

fn eval_curve3(o: &mut Out, c: &dyn Curve3dAdaptor, op: i32, n: i32, u: f64) -> AResult<()> {
    match op {
        0 => o.p3(&c.eval_d0(u)?),
        1 => {
            let r = c.eval_d1(u)?;
            o.p3(&r.point);
            o.v3(&r.d1);
        }
        2 => {
            let r = c.eval_d2(u)?;
            o.p3(&r.point);
            o.v3(&r.d1);
            o.v3(&r.d2);
        }
        3 => {
            let r = c.eval_d3(u)?;
            o.p3(&r.point);
            o.v3(&r.d1);
            o.v3(&r.d2);
            o.v3(&r.d3);
        }
        _ => o.v3(&c.eval_dn(u, n)?),
    }
    Ok(())
}

fn run(world: &mut World, family: &str, words: &[&str]) -> AResult<std::vec::Vec<Tok>> {
    COPIES_TS.lock().unwrap().clear();
    COPIES_TC2.lock().unwrap().clear();
    let mut rd = Rd::new(words);
    let rd = &mut rd;
    let mut o = Out::default();
    match family {
        "def_curve3d" => {
            let k = rd.i() as usize;
            let d = C3Data::read(rd);
            rd.finish();
            assert_eq!(k, world.c3.len());
            world.c3.push(d);
            let c = world.curve3(k);
            o.f(c.first_parameter()?);
            o.f(c.last_parameter()?);
            o.i(c.get_type()? as i64);
            o.log(c.log.get());
        }
        "def_surface" => {
            let k = rd.i() as usize;
            let d = SData::read(rd);
            rd.finish();
            assert_eq!(k, world.s.len());
            world.s.push(d);
            let c = world.surface(k);
            o.f(c.first_u_parameter()?);
            o.f(c.last_u_parameter()?);
            o.f(c.first_v_parameter()?);
            o.f(c.last_v_parameter()?);
            o.i(c.get_type()? as i64);
            ts_log(&mut o, &c);
        }
        "def_curve2d" => {
            let k = rd.i() as usize;
            let d = C2Data::read(rd);
            rd.finish();
            assert_eq!(k, world.c2.len());
            world.c2.push(d);
            let c = world.curve2(k);
            o.f(c.first_parameter()?);
            o.f(c.last_parameter()?);
            o.i(c.get_type()? as i64);
            o.log(c.log.get());
        }
        // ---- Adaptor3d_IsoCurve
        "iso_new" => {
            let (s, v, iso, p, wf, wl) = (rd.i() as usize, rd.i(), rd.i(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let surf = world.surface(s);
            let kind = iso_from_i32(iso);
            let mut c;
            match v {
                0 => c = IsoCurve::with_surface(surf.clone()),
                1 => c = IsoCurve::with_iso(surf.clone(), kind, p)?,
                2 => c = IsoCurve::with_iso_range(surf.clone(), kind, p, wf, wl)?,
                3 => {
                    c = IsoCurve::new();
                    c.load_surface(surf.clone());
                    c.load_iso(kind, p)?;
                }
                4 => {
                    c = IsoCurve::new();
                    c.load_surface(surf.clone());
                    c.load_iso_range(kind, p, wf, wl)?;
                }
                _ => {
                    c = IsoCurve::with_surface(surf.clone());
                    c.load_iso(kind, p)?;
                }
            }
            o.i(c.iso() as i64);
            o.f(c.parameter());
            o.f(c.first_parameter()?);
            o.f(c.last_parameter()?);
            ts_log(&mut o, &surf);
        }
        "iso_params" => {
            let (s, iso, p, wf, wl) = run_iso_inputs(rd);
            let (op, r) = (rd.i(), rd.f());
            rd.finish();
            let (surf, c) = make_iso(world, s, iso, p, wf, wl)?;
            match op {
                0 => o.i(c.continuity()? as i64),
                1 => o.b(c.is_closed()?),
                2 => o.b(c.is_periodic()?),
                3 => o.f(c.period()?),
                _ => o.f(c.resolution(r)?),
            }
            ts_log(&mut o, &surf);
        }
        "iso_nb_intervals" | "iso_intervals" => {
            let (s, iso, p, wf, wl) = run_iso_inputs(rd);
            let si = rd.i() as usize;
            rd.finish();
            let (surf, c) = make_iso(world, s, iso, p, wf, wl)?;
            let n = c.nb_intervals(SHAPES[si])?;
            if family == "iso_nb_intervals" {
                o.i(n.into());
            } else {
                let mut t = vec![-7.0; (n + 1) as usize];
                c.intervals(&mut t, SHAPES[si])?;
                o.i(n.into());
                for v in t {
                    o.f(v);
                }
            }
            ts_log(&mut o, &surf);
        }
        "iso_eval" => {
            let (s, iso, p, wf, wl) = run_iso_inputs(rd);
            let (op, n, u) = (rd.i(), rd.i(), rd.f());
            rd.finish();
            let (surf, c) = make_iso(world, s, iso, p, wf, wl)?;
            eval_curve3(&mut o, &c, op, n, u)?;
            ts_log(&mut o, &surf);
        }
        "iso_type" => {
            let (s, iso, p, wf, wl) = run_iso_inputs(rd);
            rd.finish();
            let (surf, c) = make_iso(world, s, iso, p, wf, wl)?;
            o.i(c.get_type()? as i64);
            ts_log(&mut o, &surf);
        }
        "iso_info" => {
            let (s, iso, p, wf, wl) = run_iso_inputs(rd);
            let op = rd.i();
            rd.finish();
            let (surf, c) = make_iso(world, s, iso, p, wf, wl)?;
            match op {
                0 => o.i(c.degree()?.into()),
                1 => o.b(c.is_rational()?),
                2 => o.i(c.nb_poles()?.into()),
                _ => o.i(c.nb_knots()?.into()),
            }
            ts_log(&mut o, &surf);
        }
        "iso_line" | "iso_circle" | "iso_conic" | "iso_geom" => {
            let (s, iso, p, wf, wl) = run_iso_inputs(rd);
            let op = if family == "iso_conic" || family == "iso_geom" { rd.i() } else { 0 };
            let _j = rd.i();
            rd.finish();
            let (surf, c) = make_iso(world, s, iso, p, wf, wl)?;
            match family {
                "iso_line" => o.lin(&c.line()?),
                "iso_circle" => o.circ(&c.circle()?),
                "iso_conic" => match op {
                    0 => o.elips(&c.ellipse()?),
                    1 => o.hypr(&c.hyperbola()?),
                    _ => o.parab(&c.parabola()?),
                },
                _ => {
                    if op == 0 {
                        o.bez(&c.bezier()?);
                    } else {
                        o.bsp(&c.bspline()?);
                    }
                }
            }
            ts_log(&mut o, &surf);
        }
        "iso_copy" => {
            let (s, iso, p, wf, wl) = run_iso_inputs(rd);
            let (trim, a, b) = (rd.b(), rd.f(), rd.f());
            rd.finish();
            let surf = world.surface(s);
            let c = IsoCurve::with_iso_range(surf.clone(), iso_from_i32(iso), p, wf, wl)?;
            let i2 = if trim { c.trimmed(a, b, 1e-7)? } else { c.shallow_copied()? };
            o.i(i2.iso() as i64);
            o.f(i2.parameter());
            o.f(i2.first_parameter()?);
            o.f(i2.last_parameter()?);
            let s2 = i2.surface().ok_or(AdaptorError::NullObject)?;
            o.b(addr(s2) == addr(&surf));
            ts_log(&mut o, &surf);
            if addr(s2) == addr(&surf) {
                ts_log(&mut o, &surf);
            } else {
                ts_log(&mut o, &find_ts(addr(s2)));
            }
        }
        // ---- Adaptor3d_CurveOnSurface
        "cos_new" => {
            let (si, ci, v) = (rd.i() as usize, rd.i() as usize, rd.i());
            rd.finish();
            let surf = world.surface(si);
            let cur = world.curve2(ci);
            let s_h: Arc<dyn SurfaceAdaptor> = surf.clone();
            let c_h: Arc<dyn Curve2dAdaptor> = cur.clone();
            let mut c;
            match v {
                0 => c = CurveOnSurface::with_surface(s_h),
                1 => c = CurveOnSurface::with_curve_surface(c_h, s_h)?,
                2 => {
                    c = CurveOnSurface::new();
                    c.load_surface(s_h)?;
                    c.load_curve(c_h)?;
                }
                3 => {
                    c = CurveOnSurface::new();
                    c.load_curve(c_h)?;
                    c.load_surface(s_h)?;
                }
                4 => {
                    c = CurveOnSurface::new();
                    c.load(c_h, s_h)?;
                }
                _ => {
                    c = CurveOnSurface::with_surface(s_h);
                    c.load_curve(c_h)?;
                }
            }
            pr_cos(&mut o, &c)?;
            o.b(match c.get_curve() {
                None => v == 0,
                Some(x) => v != 0 && addr(x) == addr(&cur),
            });
            ts_log(&mut o, &surf);
            o.log(cur.log.get());
        }
        "cos_params" => {
            let (si, ci, op, r) = (rd.i() as usize, rd.i() as usize, rd.i(), rd.f());
            rd.finish();
            let (surf, cur, c) = make_cos(world, si, ci)?;
            match op {
                0 => o.f(c.first_parameter()?),
                1 => o.f(c.last_parameter()?),
                2 => o.i(c.continuity()? as i64),
                3 => o.b(c.is_closed()?),
                4 => o.b(c.is_periodic()?),
                5 => o.f(c.period()?),
                _ => o.f(c.resolution(r)?),
            }
            o.i(c.get_type()? as i64);
            ts_log(&mut o, &surf);
            o.log(cur.log.get());
        }
        "cos_eval" => {
            let (si, ci, op, n, u) = (rd.i() as usize, rd.i() as usize, rd.i(), rd.i(), rd.f());
            rd.finish();
            let (surf, cur, c) = make_cos(world, si, ci)?;
            eval_curve3(&mut o, &c, op, n, u)?;
            o.i(c.get_type()? as i64);
            ts_log(&mut o, &surf);
            o.log(cur.log.get());
        }
        "cos_info" => {
            let (si, ci, op) = (rd.i() as usize, rd.i() as usize, rd.i());
            rd.finish();
            let (surf, cur, c) = make_cos(world, si, ci)?;
            match op {
                0 => o.elips(&c.ellipse()?),
                1 => o.hypr(&c.hyperbola()?),
                2 => o.parab(&c.parabola()?),
                3 => {
                    o.i(c.degree()?.into());
                    o.b(c.is_rational()?);
                    o.i(c.nb_poles()?.into());
                }
                4 => {
                    o.i(c.nb_knots()?.into());
                    o.bez(&c.bezier()?);
                }
                _ => o.bsp(&c.bspline()?),
            }
            ts_log(&mut o, &surf);
            o.log(cur.log.get());
        }
        "cos_nb_intervals" | "cos_intervals" => {
            let (si, ci, sh) = (rd.i() as usize, rd.i() as usize, rd.i() as usize);
            rd.finish();
            let (surf, cur, c) = make_cos(world, si, ci)?;
            if family == "cos_nb_intervals" {
                o.i(c.nb_intervals(SHAPES[sh])?.into());
            } else {
                let n = c.nb_intervals(SHAPES[sh])?;
                let mut t = vec![-7.0; (n + 1) as usize];
                c.intervals(&mut t, SHAPES[sh])?;
                o.i(n.into());
                for v in t {
                    o.f(v);
                }
            }
            ts_log(&mut o, &surf);
            o.log(cur.log.get());
        }
        "cos_copy" => {
            let (si, ci, trim, a, b, _j) = (rd.i() as usize, rd.i() as usize, rd.b(), rd.f(), rd.f(), rd.i());
            rd.finish();
            let (surf, cur, c) = make_cos(world, si, ci)?;
            let c2 = if trim { c.trimmed(a, b, 1e-7)? } else { c.shallow_copied()? };
            o.f(c2.first_parameter()?);
            o.f(c2.last_parameter()?);
            o.i(c2.get_type()? as i64);
            let s2 = c2.get_surface().ok_or(AdaptorError::NullObject)?;
            o.b(addr(s2) == addr(&surf));
            ts_log(&mut o, &surf);
            o.log(cur.log.get());
            if addr(s2) == addr(&surf) {
                ts_log(&mut o, &surf);
            } else {
                ts_log(&mut o, &find_ts(addr(s2)));
            }
            let c2c = c2.get_curve().ok_or(AdaptorError::NullObject)?;
            if addr(c2c) == addr(&cur) {
                o.log(cur.log.get());
            } else {
                o.log(find_tc2(addr(c2c)).log.get());
            }
        }
        // ---- Adaptor3d_TopolTool
        "topol_init0" => {
            let _j = rd.i();
            rd.finish();
            let mut t = TopolTool::new();
            t.initialize()?;
            o.i(_j.into());
        }
        "topol_restr" => {
            let (si, bad, _j) = (rd.i() as usize, rd.b(), rd.i());
            rd.finish();
            let surf = world.surface(si);
            let mut t = TopolTool::new();
            t.initialize_surface(surf.clone())?;
            let mut v = std::vec::Vec::new();
            t.init();
            while t.more() {
                v.push(t.value()?);
                t.next();
            }
            o.i(v.len() as i64);
            for c in &v {
                o.lin2(&c.line()?);
                o.f(c.first_parameter()?);
                o.f(c.last_parameter()?);
            }
            o.b(t.domain_is_infinite());
            o.b(t.is_uniform_sampling()?);
            if bad {
                t.value()?;
            }
            ts_log(&mut o, &surf);
        }
        "topol_vertices" => {
            let (ci, bad, _j) = (rd.i() as usize, rd.b(), rd.i());
            rd.finish();
            let cur = world.curve2(ci);
            let c_h: Arc<dyn Curve2dAdaptor> = cur.clone();
            let mut t = TopolTool::new();
            t.initialize_curve(c_h.clone())?;
            let mut v = std::vec::Vec::new();
            t.init_vertex_iterator();
            while t.more_vertex() {
                v.push(t.vertex()?);
                t.next_vertex();
            }
            o.i(v.len() as i64);
            for h in &v {
                o.p2(&h.value());
                o.i(h.orientation() as i64);
                o.f(h.resolution(c_h.as_ref()));
                if ci <= 4 || ci >= 12 {
                    o.f(h.parameter(c_h.as_ref())?);
                }
            }
            if v.len() == 2 {
                o.b(v[0].is_same(&v[1]));
                o.b(t.identical(&v[0], &v[1]));
            }
            if bad {
                t.next_vertex();
                t.vertex()?;
            }
            o.log(cur.log.get());
        }
        "topol_classify" | "topol_on" => {
            let (si, u, v, tol, rp) = (rd.i() as usize, rd.f(), rd.f(), rd.f(), rd.b());
            rd.finish();
            let surf = world.surface(si);
            let t = TopolTool::with_surface(surf.clone())?;
            if family == "topol_classify" {
                o.i(t.classify(&Pnt2d::new(u, v), tol, rp) as i64);
            } else {
                o.b(t.is_the_point_on(&Pnt2d::new(u, v), tol, rp));
            }
            ts_log(&mut o, &surf);
        }
        "topol_misc" => {
            let (si, ci, op, _j) = (rd.i() as usize, rd.i() as usize, rd.i(), rd.i());
            rd.finish();
            let surf = world.surface(si);
            let cur = world.curve2(ci);
            let c_h: Arc<dyn Curve2dAdaptor> = cur.clone();
            let mut t = TopolTool::new();
            t.initialize_surface(surf.clone())?;
            let v1 = Arc::new(HVertex::new(Pnt2d::new(0.25, 0.5), Orientation::Forward, 1e-8));
            let v2 =
                Arc::new(HVertex::new(Pnt2d::new(0.25 + f64::from(_j % 2) * 1e-8, 0.5), Orientation::Reversed, 1e-8));
            match op {
                0 => o.i(t.orientation_curve(&*c_h) as i64),
                1 => {
                    o.i(t.orientation_vertex(&v1) as i64);
                    o.i(t.orientation_vertex(&v2) as i64);
                }
                2 => o.b(t.identical(&v1, &v2)),
                3 => o.b(t.has_3d()),
                4 => o.f(t.tol_3d_curve(&*c_h)?),
                5 => o.f(t.tol_3d_vertex(&v1)?),
                6 => o.p3(&t.pnt(&v1)?),
                _ => o.b(t.edge().is_none()),
            }
            ts_log(&mut o, &surf);
            o.log(cur.log.get());
        }
        "topol_samples" => {
            let (si, flag) = (rd.i() as usize, rd.i());
            rd.finish();
            let surf = world.surface(si);
            let mut t = TopolTool::new();
            t.initialize_surface(surf.clone())?;
            if flag == 1 {
                t.compute_sample_points()?;
            }
            let (nu, nv) = (t.nb_samples_u()?, t.nb_samples_v()?);
            o.i(nu.into());
            o.i(nv.into());
            o.i(t.nb_samples()?.into());
            for idx in [0, 1, 7, 13] {
                let (p2, p3) = t.sample_point(idx % (nu * nv) + 1)?;
                o.p2(&p2);
                o.p3(&p3);
            }
            ts_log(&mut o, &surf);
        }
        "topol_samplepnts" | "topol_bspl_samplepnts" => {
            let (si, defl, nu, nv, j) = (rd.i() as usize, rd.f(), rd.i(), rd.i(), rd.i());
            rd.finish();
            let surf = world.surface(si);
            let mut t = TopolTool::new();
            t.initialize_surface(surf.clone())?;
            if family == "topol_samplepnts" {
                t.sample_pnts(defl, nu, nv)?;
            } else {
                t.bspl_sample_pnts(defl, nu, nv)?;
            }
            let (nsu, nsv) = (t.nb_samples_u()?, t.nb_samples_v()?);
            o.i(nsu.into());
            o.i(nsv.into());
            let up = t.u_parameters()?;
            let vp = t.v_parameters()?;
            for x in up.iter().take(nsu as usize) {
                o.f(*x);
            }
            for x in vp.iter().take(nsv as usize) {
                o.f(*x);
            }
            let (p2, p3) = t.sample_point(1 + (j % (nsu * nsv)))?;
            o.p2(&p2);
            o.p3(&p3);
            ts_log(&mut o, &surf);
        }
        "topol_cone_apex" => {
            let (org, zd, xd) = (rd.f3(), rd.f3(), rd.f3());
            let (semi, rad) = (rd.f(), rd.f());
            rd.finish();
            let cone = Cone::new(ax3_of(org, zd, xd)?, semi, rad)?;
            let (u, v) = TopolTool::get_cone_apex_param(&cone);
            o.f(u);
            o.f(v);
        }
        // ---- Adaptor3d_HSurfaceTool
        "hst_forward" => {
            let (si, op, sh, u, v, na, nb) = (rd.i() as usize, rd.i(), rd.i() as usize, rd.f(), rd.f(), rd.i(), rd.i());
            rd.finish();
            let surf = world.surface(si);
            let a: Arc<dyn SurfaceAdaptor> = surf.clone();
            match op {
                0 => o.f(surface_tool::first_u_parameter(&a)?),
                1 => o.f(surface_tool::first_v_parameter(&a)?),
                2 => o.f(surface_tool::last_u_parameter(&a)?),
                3 => o.f(surface_tool::last_v_parameter(&a)?),
                4 => o.i(surface_tool::nb_u_intervals(&a, SHAPES[sh])?.into()),
                5 => o.i(surface_tool::nb_v_intervals(&a, SHAPES[sh])?.into()),
                6 => {
                    let n = surface_tool::nb_u_intervals(&a, SHAPES[sh])?;
                    let mut t = vec![0.0; (n + 1) as usize];
                    surface_tool::u_intervals(&a, &mut t, SHAPES[sh])?;
                    o.i(n.into());
                    for x in t {
                        o.f(x);
                    }
                }
                7 => {
                    let n = surface_tool::nb_v_intervals(&a, SHAPES[sh])?;
                    let mut t = vec![0.0; (n + 1) as usize];
                    surface_tool::v_intervals(&a, &mut t, SHAPES[sh])?;
                    o.i(n.into());
                    for x in t {
                        o.f(x);
                    }
                }
                8 => {
                    let r = surface_tool::u_trim(&a, u, u + 1.0, 1e-7)?;
                    o.f(r.first_u_parameter()?);
                    o.f(r.last_u_parameter()?);
                }
                9 => {
                    let r = surface_tool::v_trim(&a, v, v + 1.0, 1e-7)?;
                    o.f(r.first_v_parameter()?);
                    o.f(r.last_v_parameter()?);
                }
                10 => o.b(surface_tool::is_u_closed(&a)?),
                11 => o.b(surface_tool::is_v_closed(&a)?),
                12 => o.b(surface_tool::is_u_periodic(&a)?),
                13 => o.f(surface_tool::u_period(&a)?),
                14 => o.b(surface_tool::is_v_periodic(&a)?),
                15 => o.f(surface_tool::v_period(&a)?),
                16 => o.p3(&surface_tool::value(&a, u, v)?),
                17 => o.p3(&surface_tool::d0(&a, u, v)?),
                18 => {
                    let r = surface_tool::d1(&a, u, v)?;
                    o.p3(&r.point);
                    o.v3(&r.d1u);
                    o.v3(&r.d1v);
                }
                19 => {
                    let r = surface_tool::d2(&a, u, v)?;
                    o.p3(&r.point);
                    for x in [r.d1u, r.d1v, r.d2u, r.d2v, r.d2uv] {
                        o.v3(&x);
                    }
                }
                20 => {
                    let r = surface_tool::d3(&a, u, v)?;
                    o.p3(&r.point);
                    for x in [r.d1u, r.d1v, r.d2u, r.d2v, r.d2uv, r.d3u, r.d3v, r.d3uuv, r.d3uvv] {
                        o.v3(&x);
                    }
                }
                21 => o.v3(&surface_tool::dn(&a, u, v, na, nb)?),
                22 => o.f(surface_tool::u_resolution(&a, u)?),
                23 => o.f(surface_tool::v_resolution(&a, v)?),
                24 => o.i(surface_tool::get_type(&a)? as i64),
                25 => o.a3(surface_tool::plane(&a)?.position()),
                26 => {
                    let c = surface_tool::cylinder(&a)?;
                    o.a3(c.position());
                    o.f(c.radius());
                }
                27 => {
                    let c = surface_tool::cone(&a)?;
                    o.a3(c.position());
                    o.f(c.ref_radius());
                    o.f(c.semi_angle());
                }
                28 => {
                    let c = surface_tool::torus(&a)?;
                    o.a3(c.position());
                    o.f(c.major_radius());
                    o.f(c.minor_radius());
                }
                29 => {
                    let c = surface_tool::sphere(&a)?;
                    o.a3(c.position());
                    o.f(c.radius());
                }
                30 => o.bez_s(&surface_tool::bezier(&a)?),
                31 => o.bsp_s(&surface_tool::bspline(&a)?),
                32 => o.a1(&surface_tool::axe_of_revolution(&a)?),
                33 => o.d3(&surface_tool::direction(&a)?),
                34 => {
                    let c = surface_tool::basis_curve(&a)?;
                    o.b(c.is_none());
                    if let Some(c) = c {
                        o.i(c.get_type()? as i64);
                    }
                }
                35 => {
                    let c = surface_tool::basis_surface(&a)?;
                    o.b(c.is_none());
                    if let Some(c) = c {
                        o.i(c.get_type()? as i64);
                    }
                }
                _ => o.f(surface_tool::offset_value(&a)?),
            }
            ts_log(&mut o, &surf);
        }
        "hst_g1" => {
            let (si, along_u, tol) = (rd.i() as usize, rd.b(), rd.f());
            rd.finish();
            let surf = world.surface(si);
            let a: Arc<dyn SurfaceAdaptor> = surf.clone();
            o.b(surface_tool::is_surf_g1(&a, along_u, tol)?);
            ts_log(&mut o, &surf);
        }
        "hst_nb_samples_u" | "hst_nb_samples_v" => {
            let (si, two, x1, x2) = (rd.i() as usize, rd.b(), rd.f(), rd.f());
            rd.finish();
            let surf = world.surface(si);
            let a: Arc<dyn SurfaceAdaptor> = surf.clone();
            let n = match (family == "hst_nb_samples_u", two) {
                (true, true) => surface_tool::nb_samples_u_range(&a, x1, x2)?,
                (true, false) => surface_tool::nb_samples_u(&a)?,
                (false, true) => surface_tool::nb_samples_v_range(&a, x1, x2)?,
                (false, false) => surface_tool::nb_samples_v(&a)?,
            };
            o.i(n.into());
            ts_log(&mut o, &surf);
        }
        // ---- Adaptor3d_HVertex, Adaptor3d_InterFunc
        "hvertex" => {
            let (x, y, ori, res, ci, dx) = (rd.f(), rd.f(), rd.i(), rd.f(), rd.i() as usize, rd.f());
            rd.finish();
            let cur = world.curve2(ci);
            let c_h: Arc<dyn Curve2dAdaptor> = cur.clone();
            let orientation =
                [Orientation::Forward, Orientation::Reversed, Orientation::Internal, Orientation::External]
                    [ori as usize];
            let v1 = HVertex::new(Pnt2d::new(x, y), orientation, res);
            let v2 = HVertex::new(Pnt2d::new(x + dx, y), Orientation::Reversed, 0.5);
            o.p2(&v1.value());
            o.i(v1.orientation() as i64);
            o.f(v1.resolution(c_h.as_ref()));
            o.f(v1.parameter(c_h.as_ref())?);
            o.b(v1.is_same(&v2));
            o.b(v2.is_same(&v1));
            o.log(cur.log.get());
        }
        "interfunc" => {
            let (ci, fix, fv, x) = (rd.i() as usize, rd.i(), rd.f(), rd.f());
            rd.finish();
            let cur = world.curve2(ci);
            let mut f = InterFunc::new(cur.clone(), fv, fix)?;
            let a = Function::value(&mut f, x);
            o.b(a.is_some());
            o.f(a.unwrap_or(0.0));
            let b = FunctionWithDerivative::derivative(&mut f, x);
            o.b(b.is_some());
            o.f(b.unwrap_or(0.0));
            let c = FunctionWithDerivative::values(&mut f, x);
            o.b(c.is_some());
            let (cv, cd) = c.unwrap_or((0.0, 0.0));
            o.f(cv);
            o.f(cd);
            o.log(cur.log.get());
        }
        _ => panic!("unknown family {family}"),
    }
    Ok(o.0)
}

fn make_cos(world: &World, si: usize, ci: usize) -> AResult<(Arc<Ts>, Arc<Tc2>, CurveOnSurface)> {
    let surf = world.surface(si);
    let cur = world.curve2(ci);
    let c = CurveOnSurface::with_curve_surface(cur.clone(), surf.clone())?;
    Ok((surf, cur, c))
}
fn pr_cos(o: &mut Out, c: &CurveOnSurface) -> AResult<()> {
    let t = c.get_type()?;
    o.i(t as i64);
    match t {
        GeomAbsCurveType::Line => o.lin(&c.line()?),
        GeomAbsCurveType::Circle => o.circ(&c.circle()?),
        _ => {}
    }
    Ok(())
}

#[derive(Default)]
struct Tally {
    values: usize,
    exact: usize,
}

fn compare(t: &mut Tally, family: &str, line: &str, got: &AResult<std::vec::Vec<Tok>>, want: &[&str]) {
    if want == ["throws"] {
        assert!(got.is_err(), "{family}: C++ throws but Rust returned Ok\n{line}");
        return;
    }
    let got = match got {
        Ok(g) => g,
        Err(e) => panic!("{family}: Rust returned Err({e:?}) but C++ did not throw\n{line}"),
    };
    assert_eq!(got.len(), want.len(), "{family}: output length\n{line}");
    let tolerant = ULP_FAMILIES.contains(&family);
    let scale = got
        .iter()
        .zip(want)
        .filter_map(|(g, w)| match g {
            Tok::F(_) => Some(f64::from_bits(u64::from_str_radix(w, 16).unwrap()).abs()),
            Tok::I(_) => None,
        })
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    for (k, (g, w)) in got.iter().zip(want).enumerate() {
        match g {
            Tok::I(v) => assert_eq!(*v, w.parse::<i128>().unwrap(), "{family} output {k}\n{line}"),
            Tok::F(v) => {
                let e = f64::from_bits(u64::from_str_radix(w, 16).unwrap());
                t.values += 1;
                t.exact += usize::from(v.to_bits() == e.to_bits());
                if v.is_nan() && e.is_nan() {
                    continue;
                }
                if tolerant {
                    let ulp = scale.max(f64::MIN_POSITIVE) * f64::EPSILON;
                    assert!(
                        v.to_bits() == e.to_bits() || (v - e).abs() <= 8.0 * ulp,
                        "{family} output {k}: {v:e} != {e:e} (scale {scale:e})\n{line}"
                    );
                } else {
                    assert_eq!(v.to_bits(), e.to_bits(), "{family} output {k}: {v:e} != {e:e}\n{line}");
                }
            }
        }
    }
}

#[test]
fn adaptor3d_matches_occt() {
    let mut seen = HashSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut distinct = BTreeMap::<String, BTreeSet<String>>::new();
    let mut tally = Tally::default();
    let mut world = World::default();
    let mut lines = 0;
    let mut throws = 0;
    for (n, line) in DATA.lines().enumerate() {
        let (left, right) = line.split_once(" |").unwrap_or_else(|| panic!("line {} malformed", n + 1));
        let mut it = left.split_whitespace();
        let family = it.next().unwrap();
        let words: std::vec::Vec<&str> = it.collect();
        assert!(seen.insert(line.to_owned()), "duplicate line {}", n + 1);
        *counts.entry(family.to_owned()).or_default() += 1;
        distinct.entry(family.to_owned()).or_default().insert(left.to_owned());
        let want: std::vec::Vec<&str> = right.split_whitespace().collect();
        let got = run(&mut world, family, &words);
        throws += usize::from(want == ["throws"]);
        compare(&mut tally, family, line, &got, &want);
        lines += 1;
    }
    assert_eq!(lines, GOLDEN_LINES);
    let mut want_fams = FAMILIES.to_vec();
    want_fams.sort_unstable();
    assert_eq!(counts.keys().map(String::as_str).collect::<std::vec::Vec<_>>(), want_fams);
    for name in FAMILIES {
        assert!(distinct[*name].len() >= MIN_DISTINCT, "{name} needs {MIN_DISTINCT} distinct inputs");
    }
    assert!(throws >= 1, "the oracle must exercise `throws`");
    println!(
        "adaptor3d: {} families, {lines} lines ({throws} throws), {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
