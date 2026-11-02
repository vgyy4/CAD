//! Differential judge for OCCT's package `Adaptor2d` (OCCT 8.0.1): replays `tests/adaptor2d.txt`, printed by
//! `oracle/adaptor2d.cpp` against the real library. See `docs/api-adaptor2d.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::adaptor2d::{
    AdaptorError, Curve2dAdaptor, GeomAbsCurveType, GeomAbsShape, Line2d, OffsetCurve, Result as AResult,
};
use occt_port::geom2d::BezierCurve;
use occt_port::gp::{Ax22d, Circ2d, Dir2d, Elips2d, Hypr2d, Lin2d, Parab2d, Pnt2d, Vec2d};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex};

const DATA: &str = include_str!("adaptor2d.txt");
const GOLDEN_LINES: usize = 609;
const MIN_DISTINCT: usize = 6;
const FAMILIES: &[&str] = &[
    "base_default",
    "base_eval",
    "line_copy_trim",
    "line_eval",
    "line_nosuch",
    "line_params",
    "line_period",
    "offset_bezier_info",
    "offset_circle",
    "offset_closed",
    "offset_conic",
    "offset_continuity",
    "offset_copy",
    "offset_ctor",
    "offset_eval",
    "offset_intervals",
    "offset_line",
    "offset_nb_intervals",
    "offset_nb_samples",
    "offset_null",
    "offset_period",
    "offset_resolution",
    "offset_trim",
    "offset_type",
];
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
    w: Vec<&'a str>,
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
    fn finish(&self) {
        assert_eq!(self.p, self.w.len(), "unread input tokens");
    }
}

#[derive(Default)]
struct Out(Vec<Tok>);
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
    fn v2(&mut self, v: &Vec2d) {
        self.f(v.x());
        self.f(v.y());
    }
    fn d2(&mut self, d: &Dir2d) {
        self.f(d.x());
        self.f(d.y());
    }
    fn a22(&mut self, a: &Ax22d) {
        self.p2(a.location());
        self.d2(a.x_direction());
        self.d2(a.y_direction());
    }
    fn lin(&mut self, l: &Lin2d) {
        self.p2(l.location());
        self.d2(l.direction());
    }
    fn circ(&mut self, c: &Circ2d) {
        self.a22(c.axis());
        self.f(c.radius());
    }
    fn elips(&mut self, e: &Elips2d) {
        self.a22(e.axis());
        self.f(e.major_radius());
        self.f(e.minor_radius());
    }
    fn hypr(&mut self, h: &Hypr2d) {
        self.a22(h.axis());
        self.f(h.major_radius());
        self.f(h.minor_radius());
    }
    fn parab(&mut self, p: &Parab2d) {
        self.a22(p.axis());
        self.f(p.focal());
    }
    fn tc(&mut self, t: &Tc) {
        let (calls, hash) = *t.state.lock().unwrap();
        self.i(calls.into());
        self.u(hash);
    }
}

fn mixh(h: u64, x: u64) -> u64 {
    (h ^ x).wrapping_mul(0x100000001b3)
}
const H0: u64 = 0xcbf29ce484222325;

/// The test adaptor of the oracle ("TC").
#[derive(Debug)]
struct Tc {
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
    state: Mutex<(i32, u64)>,
}
static LAST_COPY: Mutex<Option<Arc<Tc>>> = Mutex::new(None);

impl Tc {
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
            state: Mutex::new((0, H0)),
        }
    }
    fn touch(&self, id: u64, x: f64, y: f64) {
        let mut s = self.state.lock().unwrap();
        s.0 += 1;
        s.1 = mixh(s.1, id);
        s.1 = mixh(s.1, x.to_bits());
        s.1 = mixh(s.1, y.to_bits());
    }
    #[allow(clippy::type_complexity)]
    fn poly(&self, u: f64) -> (Pnt2d, Vec2d, Vec2d, Vec2d, Vec2d) {
        let (a, b, c, d, p0) = (&self.a, &self.b, &self.c, &self.d, &self.p0);
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
        Ax22d::from_dir(Pnt2d::new(self.cx, self.cy), Dir2d::new(self.dx, self.dy).unwrap(), self.sense)
    }
}
impl Curve2dAdaptor for Tc {
    fn shallow_copy(&self) -> AResult<Arc<dyn Curve2dAdaptor>> {
        self.touch(26, 0.0, 0.0);
        let (calls, hash) = *self.state.lock().unwrap();
        let copy = Arc::new(Tc {
            brk: self.brk,
            p0: self.p0,
            a: self.a,
            b: self.b,
            c: self.c,
            d: self.d,
            state: Mutex::new((calls, mixh(hash, 0xC0FFEE))),
            ..*self.clone_fields()
        });
        *LAST_COPY.lock().unwrap() = Some(copy.clone());
        Ok(copy)
    }
    fn first_parameter(&self) -> AResult<f64> {
        self.touch(1, 0.0, 0.0);
        Ok(self.u0)
    }
    fn last_parameter(&self) -> AResult<f64> {
        self.touch(2, 0.0, 0.0);
        Ok(self.u1)
    }
    fn continuity(&self) -> AResult<GeomAbsShape> {
        self.touch(3, 0.0, 0.0);
        Ok(SHAPES[self.cont as usize])
    }
    fn nb_intervals(&self, s: GeomAbsShape) -> AResult<i32> {
        self.touch(4, f64::from(s as i32), 0.0);
        Ok(self.nint)
    }
    fn intervals(&self, t: &mut Vec<f64>, s: GeomAbsShape) -> AResult<()> {
        self.touch(5, f64::from(s as i32), 0.0);
        for k in 0..=self.nint as usize {
            *t.get_mut(k).ok_or(AdaptorError::OutOfRange)? = self.brk[k];
        }
        Ok(())
    }
    fn is_closed(&self) -> AResult<bool> {
        self.touch(6, 0.0, 0.0);
        Ok(self.closed)
    }
    fn is_periodic(&self) -> AResult<bool> {
        self.touch(7, 0.0, 0.0);
        Ok(self.periodic)
    }
    fn period(&self) -> AResult<f64> {
        self.touch(8, 0.0, 0.0);
        Ok(self.period)
    }
    fn value(&self, u: f64) -> AResult<Pnt2d> {
        self.touch(9, u, 0.0);
        Ok(self.poly(u).0)
    }
    fn d0(&self, u: f64) -> AResult<Pnt2d> {
        self.touch(10, u, 0.0);
        Ok(self.poly(u).0)
    }
    fn d1(&self, u: f64) -> AResult<(Pnt2d, Vec2d)> {
        self.touch(11, u, 0.0);
        let r = self.poly(u);
        Ok((r.0, r.1))
    }
    fn d2(&self, u: f64) -> AResult<(Pnt2d, Vec2d, Vec2d)> {
        self.touch(12, u, 0.0);
        let r = self.poly(u);
        Ok((r.0, r.1, r.2))
    }
    fn d3(&self, u: f64) -> AResult<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        self.touch(13, u, 0.0);
        let r = self.poly(u);
        Ok((r.0, r.1, r.2, r.3))
    }
    fn dn(&self, u: f64, n: i32) -> AResult<Vec2d> {
        self.touch(14, u, f64::from(n));
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
        self.touch(15, 0.0, 0.0);
        Ok(match self.ctype {
            0 => GeomAbsCurveType::Line,
            1 => GeomAbsCurveType::Circle,
            2 => GeomAbsCurveType::Ellipse,
            3 => GeomAbsCurveType::Hyperbola,
            4 => GeomAbsCurveType::Parabola,
            5 => GeomAbsCurveType::BezierCurve,
            6 => GeomAbsCurveType::BSplineCurve,
            7 => GeomAbsCurveType::OffsetCurve,
            _ => GeomAbsCurveType::OtherCurve,
        })
    }
    fn line(&self) -> AResult<Lin2d> {
        self.touch(16, 0.0, 0.0);
        Ok(Lin2d::new(Pnt2d::new(self.cx, self.cy), Dir2d::new(self.dx, self.dy)?))
    }
    fn circle(&self) -> AResult<Circ2d> {
        self.touch(17, 0.0, 0.0);
        Ok(Circ2d::from_ax22d(self.axes(), self.r1)?)
    }
    fn ellipse(&self) -> AResult<Elips2d> {
        self.touch(18, 0.0, 0.0);
        Ok(Elips2d::from_ax22d(self.axes(), self.r1, self.r2)?)
    }
    fn hyperbola(&self) -> AResult<Hypr2d> {
        self.touch(19, 0.0, 0.0);
        Ok(Hypr2d::from_ax22d(self.axes(), self.r1, self.r2)?)
    }
    fn parabola(&self) -> AResult<Parab2d> {
        self.touch(20, 0.0, 0.0);
        Ok(Parab2d::from_ax22d(self.axes(), self.r1)?)
    }
    fn degree(&self) -> AResult<i32> {
        self.touch(21, 0.0, 0.0);
        Ok(self.degree)
    }
    fn is_rational(&self) -> AResult<bool> {
        self.touch(22, 0.0, 0.0);
        Ok(self.rational)
    }
    fn nb_poles(&self) -> AResult<i32> {
        self.touch(23, 0.0, 0.0);
        Ok(self.nbpoles)
    }
    fn nb_knots(&self) -> AResult<i32> {
        self.touch(24, 0.0, 0.0);
        Ok(self.nbknots)
    }
    fn bezier(&self) -> AResult<Option<Arc<BezierCurve>>> {
        self.touch(25, 0.0, 0.0);
        if !self.has_bezier {
            return Err(AdaptorError::NotImplemented);
        }
        let poles = [
            Pnt2d::new(self.p0[0], self.p0[1]),
            Pnt2d::new(self.p0[0] + self.a[0], self.p0[1] + self.a[1]),
            Pnt2d::new(self.p0[0] + self.b[0], self.p0[1] + self.b[1]),
        ];
        Ok(Some(Arc::new(BezierCurve::new(&poles).unwrap())))
    }
}
impl Tc {
    /// A field-by-field copy used for `..` in the shallow copy (the state is replaced by the caller).
    fn clone_fields(&self) -> Box<Tc> {
        Box::new(Tc {
            u0: self.u0,
            u1: self.u1,
            ctype: self.ctype,
            cont: self.cont,
            nint: self.nint,
            brk: self.brk,
            closed: self.closed,
            periodic: self.periodic,
            period: self.period,
            p0: self.p0,
            a: self.a,
            b: self.b,
            c: self.c,
            d: self.d,
            cx: self.cx,
            cy: self.cy,
            dx: self.dx,
            dy: self.dy,
            sense: self.sense,
            r1: self.r1,
            r2: self.r2,
            degree: self.degree,
            rational: self.rational,
            nbpoles: self.nbpoles,
            nbknots: self.nbknots,
            has_bezier: self.has_bezier,
            state: Mutex::new((0, H0)),
        })
    }
}

/// The base class with nothing overridden.
#[derive(Debug)]
struct Bare;
impl Curve2dAdaptor for Bare {}

struct Oc {
    tc: Arc<Tc>,
    off: f64,
    wf: f64,
    wl: f64,
}
impl Oc {
    fn read(rd: &mut Rd) -> Self {
        let tc = Arc::new(Tc::read(rd));
        Self { tc, off: rd.f(), wf: rd.f(), wl: rd.f() }
    }
    fn handle(&self) -> Arc<dyn Curve2dAdaptor> {
        self.tc.clone()
    }
    fn full(&self) -> OffsetCurve {
        OffsetCurve::with_range(self.handle(), self.off, self.wf, self.wl)
    }
}
fn same(c: Option<&Arc<dyn Curve2dAdaptor>>, tc: &Arc<Tc>) -> bool {
    c.is_some_and(|c| std::ptr::eq(Arc::as_ptr(c).cast::<()>(), Arc::as_ptr(tc).cast::<()>()))
}
fn pr_off(o: &mut Out, c: &OffsetCurve) {
    o.f(c.first_parameter().unwrap());
    o.f(c.last_parameter().unwrap());
    o.f(c.offset());
}

fn run(family: &str, words: &[&str]) -> Result<Vec<Tok>, AdaptorError> {
    let mut rd = Rd::new(words);
    let rd = &mut rd;
    let mut o = Out::default();
    match family {
        "base_default" => {
            let (m, sidx, u) = (rd.i(), rd.i(), rd.f());
            rd.finish();
            let sh = if sidx == 1 { GeomAbsShape::Cn } else { GeomAbsShape::C0 };
            let h = Bare;
            let mut arr = vec![0.0; 3];
            match m {
                0 => {
                    h.shallow_copy()?;
                }
                1 => o.f(h.first_parameter()?),
                2 => o.f(h.last_parameter()?),
                3 => o.i(h.continuity()? as i64),
                4 => o.i(h.nb_intervals(sh)?.into()),
                5 => {
                    h.intervals(&mut arr, sh)?;
                }
                6 => {
                    h.trim(0.0, 1.0, 1e-7)?;
                }
                7 => o.b(h.is_closed()?),
                8 => o.b(h.is_periodic()?),
                9 => o.f(h.period()?),
                10 => o.p2(&h.value(u)?),
                11 => o.p2(&h.d0(u)?),
                12 => o.p2(&h.d1(u)?.0),
                13 => o.p2(&h.d2(u)?.0),
                14 => o.p2(&h.d3(u)?.0),
                15 => o.v2(&h.dn(u, 2)?),
                16 => o.f(h.resolution(u)?),
                17 => o.i(h.get_type()? as i64),
                18 => o.lin(&h.line()?),
                19 => o.circ(&h.circle()?),
                20 => o.elips(&h.ellipse()?),
                21 => o.hypr(&h.hyperbola()?),
                22 => o.parab(&h.parabola()?),
                23 => o.i(h.degree()?.into()),
                24 => o.b(h.is_rational()?),
                25 => o.i(h.nb_poles()?.into()),
                26 => o.i(h.nb_knots()?.into()),
                27 => o.i(h.nb_samples()?.into()),
                28 => {
                    h.bezier()?;
                }
                29 => {
                    h.bspline()?;
                }
                30 => o.p2(&h.eval_d0(u)?),
                31 => o.p2(&h.eval_d1(u)?.point),
                32 => o.p2(&h.eval_d2(u)?.point),
                33 => o.p2(&h.eval_d3(u)?.point),
                _ => o.v2(&h.eval_dn(u, 1)?),
            }
        }
        "base_eval" => {
            let t = Tc::read(rd);
            let (kind, n, u) = (rd.i(), rd.i(), rd.f());
            rd.finish();
            match kind {
                0 => o.p2(&t.eval_d0(u)?),
                1 => {
                    let r = t.eval_d1(u)?;
                    o.p2(&r.point);
                    o.v2(&r.d1);
                }
                2 => {
                    let r = t.eval_d2(u)?;
                    o.p2(&r.point);
                    o.v2(&r.d1);
                    o.v2(&r.d2);
                }
                3 => {
                    let r = t.eval_d3(u)?;
                    o.p2(&r.point);
                    o.v2(&r.d1);
                    o.v2(&r.d2);
                    o.v2(&r.d3);
                }
                4 => o.v2(&t.eval_dn(u, n)?),
                _ => o.i(t.nb_samples()?.into()),
            }
            o.tc(&t);
        }
        "line_params" => {
            let (v, px, py, dx, dy, u0, u1, si, r3, tl) =
                (rd.i(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.i(), rd.f(), rd.i());
            rd.finish();
            let _ = tl;
            let mut l = Line2d::new();
            if v == 1 {
                l = Line2d::with_point_dir(Pnt2d::new(px, py), Dir2d::new(dx, dy)?, u0, u1);
            } else if v == 2 {
                l.load(&Lin2d::new(Pnt2d::new(px, py), Dir2d::new(dx, dy)?));
            } else if v == 3 {
                l.load_range(&Lin2d::new(Pnt2d::new(px, py), Dir2d::new(dx, dy)?), u0, u1);
            }
            let sh = SHAPES[si as usize];
            o.f(l.first_parameter()?);
            o.f(l.last_parameter()?);
            o.i(l.continuity()? as i64);
            o.i(l.nb_intervals(sh)?.into());
            let mut arr = vec![0.0; 2];
            l.intervals(&mut arr, sh)?;
            o.f(arr[0]);
            o.f(arr[1]);
            o.b(l.is_closed()?);
            o.b(l.is_periodic()?);
            o.f(l.resolution(r3)?);
            o.i(l.get_type()? as i64);
            o.i(l.nb_samples()?.into());
            o.lin(&l.line()?);
        }
        "line_period" => {
            let (px, py, _, _) = (rd.f(), rd.f(), rd.i(), rd.i());
            rd.finish();
            let l = Line2d::with_point_dir(Pnt2d::new(px, py), Dir2d::new(1.0, 2.0)?, 0.0, 1.0);
            o.f(l.period()?);
        }
        "line_nosuch" => {
            let (m, px, py) = (rd.i(), rd.f(), rd.f());
            rd.finish();
            let l = Line2d::with_point_dir(Pnt2d::new(px, py), Dir2d::new(0.5, 1.0)?, -1.0, 2.0);
            match m {
                0 => o.circ(&l.circle()?),
                1 => o.elips(&l.ellipse()?),
                2 => o.hypr(&l.hyperbola()?),
                3 => o.parab(&l.parabola()?),
                4 => o.i(l.degree()?.into()),
                5 => o.b(l.is_rational()?),
                6 => o.i(l.nb_poles()?.into()),
                7 => o.i(l.nb_knots()?.into()),
                8 => {
                    l.bezier()?;
                }
                _ => {
                    l.bspline()?;
                }
            }
        }
        "line_eval" => {
            let (px, py, dx, dy, kind, n, u) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.i(), rd.i(), rd.f());
            rd.finish();
            let l = Line2d::with_point_dir(Pnt2d::new(px, py), Dir2d::new(dx, dy)?, -2.0, 2.0);
            match kind {
                0 => o.p2(&l.value(u)?),
                1 => o.p2(&l.d0(u)?),
                2 => {
                    let (p, v1) = l.d1(u)?;
                    o.p2(&p);
                    o.v2(&v1);
                }
                3 => {
                    let (p, v1, v2) = l.d2(u)?;
                    o.p2(&p);
                    o.v2(&v1);
                    o.v2(&v2);
                }
                4 => {
                    let (p, v1, v2, v3) = l.d3(u)?;
                    o.p2(&p);
                    o.v2(&v1);
                    o.v2(&v2);
                    o.v2(&v3);
                }
                _ => o.v2(&l.dn(u, n)?),
            }
        }
        "line_copy_trim" => {
            let (px, py, dx, dy, f, l2, op) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.i());
            rd.finish();
            let l = Line2d::with_point_dir(Pnt2d::new(px, py), Dir2d::new(dx, dy)?, -1.0, 1.0);
            let h = if op == 0 { l.shallow_copy()? } else { l.trim(f, l2, 1e-7)? };
            o.i(0);
            o.f(h.first_parameter()?);
            o.f(h.last_parameter()?);
            o.lin(&h.line()?);
            o.i(h.get_type()? as i64);
        }
        "offset_ctor" => {
            let r = Oc::read(rd);
            let (v, off2, wf2, wl2) = (rd.i(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let mut c = match v {
                0 => OffsetCurve::with_curve(r.handle()),
                1 => OffsetCurve::with_offset(r.handle(), r.off)?,
                2 | 5 => r.full(),
                3 => {
                    let mut c = OffsetCurve::new();
                    c.load_curve(r.handle());
                    c
                }
                _ => OffsetCurve::with_curve(r.handle()),
            };
            if v == 4 {
                c.load_offset(off2)?;
            } else if v == 5 {
                c.load_range(off2, wf2, wl2);
            }
            pr_off(&mut o, &c);
            o.b(c.curve().is_none());
            o.b(same(c.curve(), &r.tc));
            o.tc(&r.tc);
        }
        "offset_continuity" => {
            let r = Oc::read(rd);
            rd.finish();
            o.i(r.full().continuity()? as i64);
            o.tc(&r.tc);
        }
        "offset_nb_intervals" => {
            let r = Oc::read(rd);
            let (si, _tl, _tn) = (rd.i(), rd.i(), rd.i());
            rd.finish();
            o.i(r.full().nb_intervals(SHAPES[si as usize])?.into());
            o.tc(&r.tc);
        }
        "offset_intervals" => {
            let r = Oc::read(rd);
            let (si, _tl, tn) = (rd.i(), rd.i(), rd.i());
            rd.finish();
            let mut ti = vec![-7.0; tn as usize];
            r.full().intervals(&mut ti, SHAPES[si as usize])?;
            for v in ti {
                o.f(v);
            }
            o.tc(&r.tc);
        }
        "offset_trim" => {
            let r = Oc::read(rd);
            let (f, l2, _tol) = (rd.f(), rd.f(), rd.f());
            rd.finish();
            let ho = r.full().trimmed(f, l2);
            let via_trait = r.full().trim(f, l2, 1e-7)?;
            assert_eq!(via_trait.first_parameter()?.to_bits(), f.to_bits());
            o.i(0);
            pr_off(&mut o, &ho);
            o.b(same(ho.curve(), &r.tc));
            // the trait call above went through the same adaptor and touched nothing of the test curve
            o.tc(&r.tc);
        }
        "offset_closed" => {
            let r = Oc::read(rd);
            rd.finish();
            let c = r.full();
            o.b(c.is_closed()?);
            o.b(c.is_periodic()?);
            o.tc(&r.tc);
        }
        "offset_period" => {
            let r = Oc::read(rd);
            rd.finish();
            o.f(r.full().period()?);
            o.tc(&r.tc);
        }
        "offset_eval" => {
            let r = Oc::read(rd);
            let (kind, u) = (rd.i(), rd.f());
            rd.finish();
            let c = r.full();
            match kind {
                0 => o.p2(&c.value(u)?),
                1 => o.p2(&c.d0(u)?),
                2 => {
                    let (p, v1) = c.d1(u)?;
                    o.p2(&p);
                    o.v2(&v1);
                }
                3 => {
                    let (p, v1, v2) = c.d2(u)?;
                    o.p2(&p);
                    o.v2(&v1);
                    o.v2(&v2);
                }
                4 => {
                    let (p, v1, v2, v3) = c.d3(u)?;
                    o.p2(&p);
                    o.v2(&v1);
                    o.v2(&v2);
                    o.v2(&v3);
                }
                _ => o.v2(&c.dn(u, 2)?),
            }
            o.tc(&r.tc);
        }
        "offset_type" => {
            let r = Oc::read(rd);
            rd.finish();
            o.i(r.full().get_type()? as i64);
            o.tc(&r.tc);
        }
        "offset_line" => {
            let r = Oc::read(rd);
            rd.finish();
            o.lin(&r.full().line()?);
            o.tc(&r.tc);
        }
        "offset_circle" => {
            let r = Oc::read(rd);
            rd.finish();
            o.circ(&r.full().circle()?);
            o.tc(&r.tc);
        }
        "offset_conic" => {
            let r = Oc::read(rd);
            let which = rd.i();
            rd.finish();
            let c = r.full();
            match which {
                0 => o.elips(&c.ellipse()?),
                1 => o.hypr(&c.hyperbola()?),
                _ => o.parab(&c.parabola()?),
            }
            o.tc(&r.tc);
        }
        "offset_bezier_info" => {
            let r = Oc::read(rd);
            let kind = rd.i();
            rd.finish();
            let c = r.full();
            match kind {
                0 => o.i(c.degree()?.into()),
                1 => o.b(c.is_rational()?),
                2 => o.i(c.nb_poles()?.into()),
                3 => o.i(c.nb_knots()?.into()),
                4 => {
                    let b = c.bezier()?.unwrap();
                    o.b(false);
                    o.i(b.nb_poles().into());
                    o.i(b.degree().into());
                    for i in 1..=b.nb_poles() {
                        o.p2(&b.pole(i).unwrap());
                    }
                }
                _ => {
                    c.bspline()?;
                    o.b(false);
                }
            }
            o.tc(&r.tc);
        }
        "offset_nb_samples" => {
            let r = Oc::read(rd);
            rd.finish();
            o.i(r.full().nb_samples()?.into());
            o.tc(&r.tc);
        }
        "offset_resolution" => {
            let r = Oc::read(rd);
            let rr = rd.f();
            rd.finish();
            o.f(r.full().resolution(rr)?);
            o.tc(&r.tc);
        }
        "offset_copy" => {
            let r = Oc::read(rd);
            let null = rd.b();
            rd.finish();
            *LAST_COPY.lock().unwrap() = None;
            let base = if null { OffsetCurve::new() } else { r.full() };
            let ho = base.shallow_copied()?;
            o.i(0);
            pr_off(&mut o, &ho);
            o.b(ho.curve().is_none());
            if ho.curve().is_some() {
                let cp = LAST_COPY.lock().unwrap().clone().unwrap();
                o.b(same(ho.curve(), &r.tc));
                o.tc(&cp);
            }
            o.tc(&r.tc);
        }
        "offset_null" => {
            let _ = rd.i();
            rd.finish();
            let c = OffsetCurve::new();
            pr_off(&mut o, &c);
            o.b(c.curve().is_none());
        }
        _ => panic!("unknown family {family}"),
    }
    Ok(o.0)
}

#[derive(Default)]
struct Tally {
    values: usize,
    exact: usize,
}

fn compare(t: &mut Tally, family: &str, line: &str, got: &Result<Vec<Tok>, AdaptorError>, want: &[&str]) {
    if want == ["throws"] {
        assert!(got.is_err(), "{family}: C++ throws but Rust returned Ok\n{line}");
        return;
    }
    let got = match got {
        Ok(g) => g,
        Err(e) => panic!("{family}: Rust returned Err({e:?}) but C++ did not throw\n{line}"),
    };
    assert_eq!(got.len(), want.len(), "{family}: output length\n{line}");
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
                assert_eq!(v.to_bits(), e.to_bits(), "{family} output {k}: {v:e} != {e:e}\n{line}");
            }
        }
    }
}

#[test]
fn adaptor2d_matches_occt() {
    let mut seen = HashSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut distinct = BTreeMap::<String, BTreeSet<String>>::new();
    let mut tally = Tally::default();
    let mut lines = 0;
    let mut throws = 0;
    for (n, line) in DATA.lines().enumerate() {
        let (left, right) = line.split_once(" |").unwrap_or_else(|| panic!("line {} malformed", n + 1));
        let mut it = left.split_whitespace();
        let family = it.next().unwrap();
        let words: Vec<&str> = it.collect();
        assert!(seen.insert(line.to_owned()), "duplicate line {}", n + 1);
        *counts.entry(family.to_owned()).or_default() += 1;
        distinct.entry(family.to_owned()).or_default().insert(left.to_owned());
        let want: Vec<&str> = right.split_whitespace().collect();
        let got = run(family, &words);
        throws += usize::from(want == ["throws"]);
        compare(&mut tally, family, line, &got, &want);
        lines += 1;
    }
    assert_eq!(lines, GOLDEN_LINES);
    let mut want_fams = FAMILIES.to_vec();
    want_fams.sort_unstable();
    assert_eq!(counts.keys().map(String::as_str).collect::<Vec<_>>(), want_fams);
    for name in FAMILIES {
        assert!(distinct[*name].len() >= MIN_DISTINCT, "{name} needs {MIN_DISTINCT} distinct inputs");
    }
    assert!(throws >= 1, "the oracle must exercise `throws`");
    println!(
        "adaptor2d: {} families, {lines} lines ({throws} throws), {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
