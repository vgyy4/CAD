//! Differential judge for OCCT's package `GeomAdaptor` (OCCT 8.0.1): replays `tests/geomadaptor.txt`, printed by
//! `oracle/geomadaptor.cpp` against the real library. See `docs/api-geomadaptor.md`.
//!
//! The geometry the adaptors work on is a closed set of Geom curves, surfaces and 2D curves printed in full by the records
//! `def_gcurve`, `def_gsurface` and `def_gcurve2d` and referred to by index. A call that may raise is recorded as `1 <results>`
//! or `0`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
#![allow(clippy::too_many_lines, clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
use occt_port::adaptor2d::{AdaptorError, Curve2dAdaptor, GeomAbsShape};
use occt_port::adaptor3d::{Curve3dAdaptor, CurveOnSurface, SurfaceAdaptor};
use occt_port::geom::{self, Curve, EvalRepDesc, EvalRepKind, Surface, SurfaceEvalRepDesc};
use occt_port::geom2d::{self, Curve2d};
use occt_port::geom2dadaptor;
use occt_port::geomadaptor::{
    self, SurfaceOfLinearExtrusion, SurfaceOfRevolution, TransformedCurve, TransformedSurface,
};
use occt_port::gp::{
    Ax1, Ax2, Ax2d, Ax3, Circ, Cone, Cylinder, Dir, Dir2d, Elips, Hypr, Lin, Parab, Pln, Pnt, Pnt2d, Sphere, Torus,
    Trsf, Vec,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

type AResult<T> = Result<T, AdaptorError>;

const DATA: &str = include_str!("geomadaptor.txt");
const GOLDEN_LINES: usize = 1973;
const MIN_DISTINCT: usize = 2;
const FAMILIES: &[&str] = &[
    "def_gcurve",
    "def_gcurve2d",
    "def_gsurface",
    "gac_eval",
    "gac_info",
    "gac_intervals",
    "gac_load",
    "gac_load_bad",
    "gac_resolution",
    "gac_trim",
    "gas_eval",
    "gas_info",
    "gas_intervals",
    "gas_load",
    "gas_load_bad",
    "gas_resolution",
    "gas_trim",
    "gse_eval",
    "gse_info",
    "gse_new",
    "gse_trim",
    "gsr_eval",
    "gsr_info",
    "gsr_new",
    "gsr_trim",
    "gtc_copy",
    "gtc_eval",
    "gtc_info",
    "gts_eval",
    "gts_info",
    "gts_trim",
    "make_curve",
    "make_surface",
];
/// Families whose results come through trigonometric functions of the libm (circles, spheres, ...): 8 ulps of the largest value
/// of the line.
const ULP_FAMILIES: &[&str] =
    &["gac_eval", "gac_resolution", "gas_eval", "gas_resolution", "gse_eval", "gse_info", "gtc_eval", "gts_eval"];
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
    fn fs(&mut self, n: i32) -> std::vec::Vec<f64> {
        (0..n.max(0)).map(|_| self.f()).collect()
    }
    fn is(&mut self, n: i32) -> std::vec::Vec<i32> {
        (0..n.max(0)).map(|_| self.i()).collect()
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
    fn f(&mut self, v: f64) {
        self.0.push(Tok::F(v));
    }
    fn b(&mut self, v: bool) {
        self.i(v.into());
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
    fn pln(&mut self, p: &Pln) {
        self.a3(p.position());
    }
    fn cyl(&mut self, c: &Cylinder) {
        self.a3(c.position());
        self.f(c.radius());
    }
    fn cone(&mut self, c: &Cone) {
        self.a3(c.position());
        self.f(c.ref_radius());
        self.f(c.semi_angle());
    }
    fn sph(&mut self, s: &Sphere) {
        self.a3(s.position());
        self.f(s.radius());
    }
    fn tor(&mut self, t: &Torus) {
        self.a3(t.position());
        self.f(t.major_radius());
        self.f(t.minor_radius());
    }
}

/// `ATT(...)`: ` 1 <results>`, or ` 0` when the body failed.
fn att(o: &mut Out, f: impl FnOnce(&mut Out) -> AResult<()>) {
    let keep = o.0.len();
    o.i(1);
    if f(o).is_err() {
        o.0.truncate(keep);
        o.i(0);
    }
}

// ---- the geometry ---------------------------------------------------------------------------------------------------------
fn pnt(a: [f64; 3]) -> Pnt {
    Pnt::new(a[0], a[1], a[2])
}
fn dir(a: [f64; 3]) -> Dir {
    Dir::new(a[0], a[1], a[2]).unwrap()
}
fn ax2(loc: [f64; 3], n: [f64; 3], x: [f64; 3]) -> Ax2 {
    Ax2::new(pnt(loc), dir(n), &dir(x)).unwrap()
}
fn ax3(loc: [f64; 3], n: [f64; 3], x: [f64; 3]) -> Ax3 {
    Ax3::new(pnt(loc), dir(n), dir(x)).unwrap()
}

#[derive(Default)]
struct World {
    curves: std::vec::Vec<Arc<dyn Curve>>,
    surfaces: std::vec::Vec<Arc<dyn Surface>>,
    curves2d: std::vec::Vec<(Arc<dyn Curve2d>, f64, f64)>,
    range0: std::vec::Vec<(f64, f64, f64, f64)>,
}

fn read_poles(rd: &mut Rd, n: usize) -> std::vec::Vec<Pnt> {
    (0..n).map(|_| pnt(rd.f3())).collect()
}

fn build_curve(world: &World, w: &[&str]) -> Arc<dyn Curve> {
    let mut rd = Rd::new(w);
    let kind = rd.i();
    let (loc, n, x) = (rd.f3(), rd.f3(), rd.f3());
    let (r1, r2) = (rd.f(), rd.f());
    let basis = rd.i();
    let (u1, u2) = (rd.f(), rd.f());
    let (deg, per) = (rd.i(), rd.b());
    let np = rd.i();
    let poles = read_poles(&mut rd, np as usize);
    let nw = rd.i();
    let wts = rd.fs(nw);
    let nk = rd.i();
    let knots = rd.fs(nk);
    let mults = rd.is(nk);
    let rep = rd.i();
    let mut c: Arc<dyn Curve> = match kind {
        0 => Arc::new(geom::Line::new(pnt(loc), dir(n))),
        1 => Arc::new(geom::Circle::new(ax2(loc, n, x), r1).unwrap()),
        2 => Arc::new(geom::Ellipse::new(ax2(loc, n, x), r1, r2).unwrap()),
        3 => Arc::new(geom::Hyperbola::new(ax2(loc, n, x), r1, r2).unwrap()),
        4 => Arc::new(geom::Parabola::new(ax2(loc, n, x), r1).unwrap()),
        5 => {
            let mut b = if wts.is_empty() {
                geom::BezierCurve::new(&poles).unwrap()
            } else {
                geom::BezierCurve::with_weights(&poles, &wts).unwrap()
            };
            if rep >= 0 {
                b.set_eval_representation(Some(full_rep(world.curves[rep as usize].clone()))).unwrap();
            }
            Arc::new(b)
        }
        6 => {
            let mut b = if wts.is_empty() {
                geom::BSplineCurve::new(&poles, &knots, &mults, deg, per).unwrap()
            } else {
                geom::BSplineCurve::with_weights(&poles, &wts, &knots, &mults, deg, per).unwrap()
            };
            if rep >= 0 {
                b.set_eval_representation(Some(full_rep(world.curves[rep as usize].clone()))).unwrap();
            }
            Arc::new(b)
        }
        7 => {
            let mut o = geom::OffsetCurve::new(world.curves[basis as usize].clone(), r1, dir(n), false).unwrap();
            if rep >= 0 {
                o.set_eval_representation(Some(full_rep(world.curves[rep as usize].clone()))).unwrap();
            }
            Arc::new(o)
        }
        _ => Arc::new(geom::TrimmedCurve::new(world.curves[basis as usize].clone(), u1, u2, true, true).unwrap()),
    };
    if kind > 8 {
        c = Arc::new(geom::Line::new(pnt(loc), dir(n)));
    }
    c
}
fn full_rep(c: Arc<dyn Curve>) -> EvalRepDesc {
    EvalRepDesc {
        kind: EvalRepKind::Full,
        representation: Some(c),
        max_deriv_order: 3,
        domain: None,
        scale: 1.0,
        offset: 0.0,
    }
}
fn full_srep(s: Arc<dyn Surface>) -> SurfaceEvalRepDesc {
    SurfaceEvalRepDesc {
        kind: EvalRepKind::Full,
        representation: Some(s),
        max_deriv_order: 3,
        domain: None,
        scale_u: 1.0,
        offset_u: 0.0,
        scale_v: 1.0,
        offset_v: 0.0,
        swap_uv: false,
    }
}

fn build_surface(world: &World, w: &[&str]) -> Arc<dyn Surface> {
    let mut rd = Rd::new(w);
    let kind = rd.i();
    let (loc, n, x) = (rd.f3(), rd.f3(), rd.f3());
    let (r1, r2, ang) = (rd.f(), rd.f(), rd.f());
    let basis = rd.i();
    let (u1, u2, v1, v2) = (rd.f(), rd.f(), rd.f(), rd.f());
    let (ud, vd, up, vp) = (rd.i(), rd.i(), rd.b(), rd.b());
    let (nu, nv) = (rd.i() as usize, rd.i() as usize);
    let flat = read_poles(&mut rd, nu * nv);
    let grid: std::vec::Vec<std::vec::Vec<Pnt>> = (0..nu).map(|i| flat[i * nv..(i + 1) * nv].to_vec()).collect();
    let nw = rd.i();
    let wf = rd.fs(nw);
    let wgrid: std::vec::Vec<std::vec::Vec<f64>> =
        if wf.is_empty() { std::vec::Vec::new() } else { (0..nu).map(|i| wf[i * nv..(i + 1) * nv].to_vec()).collect() };
    let nuk = rd.i();
    let uk = rd.fs(nuk);
    let um = rd.is(nuk);
    let nvk = rd.i();
    let vk = rd.fs(nvk);
    let vm = rd.is(nvk);
    let rep = rd.i();
    let rep_s = |world: &World| full_srep(world.surfaces[rep as usize].clone());
    match kind {
        0 => Arc::new(geom::Plane::from_ax3(ax3(loc, n, x))),
        1 => Arc::new(geom::CylindricalSurface::new(ax3(loc, n, x), r1).unwrap()),
        2 => Arc::new(geom::ConicalSurface::new(ax3(loc, n, x), ang, r1).unwrap()),
        3 => Arc::new(geom::SphericalSurface::new(ax3(loc, n, x), r1).unwrap()),
        4 => Arc::new(geom::ToroidalSurface::new(ax3(loc, n, x), r1, r2).unwrap()),
        5 => {
            let mut b = if wgrid.is_empty() {
                geom::BezierSurface::new(&grid).unwrap()
            } else {
                geom::BezierSurface::with_weights(&grid, &wgrid).unwrap()
            };
            if rep >= 0 {
                b.set_eval_representation(Some(rep_s(world))).unwrap();
            }
            Arc::new(b)
        }
        6 => {
            let mut b = if wgrid.is_empty() {
                geom::BSplineSurface::new(&grid, &uk, &vk, &um, &vm, ud, vd, up, vp).unwrap()
            } else {
                geom::BSplineSurface::with_weights(&grid, &wgrid, &uk, &vk, &um, &vm, ud, vd, up, vp).unwrap()
            };
            if rep >= 0 {
                b.set_eval_representation(Some(rep_s(world))).unwrap();
            }
            Arc::new(b)
        }
        7 => {
            let mut s =
                geom::SurfaceOfRevolution::new(world.curves[basis as usize].clone(), Ax1::new(pnt(loc), dir(n)))
                    .unwrap();
            if rep >= 0 {
                s.set_eval_representation(Some(rep_s(world))).unwrap();
            }
            Arc::new(s)
        }
        8 => {
            let mut s = geom::SurfaceOfLinearExtrusion::new(world.curves[basis as usize].clone(), dir(n)).unwrap();
            if rep >= 0 {
                s.set_eval_representation(Some(rep_s(world))).unwrap();
            }
            Arc::new(s)
        }
        9 => {
            let mut s = geom::OffsetSurface::new(world.surfaces[basis as usize].clone(), r1, false).unwrap();
            if rep >= 0 {
                s.set_eval_representation(Some(rep_s(world))).unwrap();
            }
            Arc::new(s)
        }
        _ => Arc::new(
            geom::RectangularTrimmedSurface::new(world.surfaces[basis as usize].clone(), u1, u2, v1, v2, true, true)
                .unwrap(),
        ),
    }
}

/// Rebuilds the geometry from the golden records.
fn read_world() -> World {
    let mut world = World::default();
    for line in DATA.lines() {
        let (left, right) = line.split_once(" |").unwrap();
        let family = left.split_whitespace().next().unwrap();
        let out: std::vec::Vec<&str> = right.split_whitespace().collect();
        match family {
            "def_gcurve" => {
                let c = build_curve(&world, &out);
                world.curves.push(c);
            }
            "def_gsurface" => {
                let s = build_surface(&world, &out);
                world.surfaces.push(s);
                let k = out.len();
                let mut rd = Rd::new(&out[k - 4..]);
                world.range0.push((rd.f(), rd.f(), rd.f(), rd.f()));
            }
            "def_gcurve2d" => {
                // kind, p[2], d[2], r, f, l
                let mut rd = Rd::new(&out[..8]);
                let kind = rd.i();
                let (p, d, r, f, l) = ([rd.f(), rd.f()], [rd.f(), rd.f()], rd.f(), rd.f(), rd.f());
                let c: Arc<dyn Curve2d> = if kind == 0 {
                    Arc::new(geom2d::Line::new(Pnt2d::new(p[0], p[1]), Dir2d::new(d[0], d[1]).unwrap()))
                } else {
                    Arc::new(
                        geom2d::Circle::new(
                            Ax2d::new(Pnt2d::new(p[0], p[1]), Dir2d::new(d[0], d[1]).unwrap()),
                            r,
                            true,
                        )
                        .unwrap(),
                    )
                };
                world.curves2d.push((c, f, l));
            }
            _ => {}
        }
    }
    world
}

// ---- transformations --------------------------------------------------------------------------------------------------------
fn read_ts(rd: &mut Rd) -> Trsf {
    let kind = rd.i();
    let (p, a, v) = (rd.f3(), rd.f3(), rd.f());
    let mut t = Trsf::new();
    match kind {
        1 => t.set_translation(&Vec::new(a[0], a[1], a[2])),
        2 => t.set_rotation(&Ax1::new(pnt(p), dir(a)), v).unwrap(),
        3 => t.set_scale(&pnt(p), v).unwrap(),
        4 => t.set_mirror(&pnt(p)),
        5 => t.set_mirror(&Ax1::new(pnt(p), dir(a))),
        6 => t.set_mirror(&Ax2::from_direction(pnt(p), dir(a))),
        _ => {}
    }
    t
}

// ---- printing helpers -------------------------------------------------------------------------------------------------------
fn curve_basics(o: &mut Out, a: &dyn Curve3dAdaptor) -> AResult<()> {
    o.i(a.get_type()? as i64);
    o.f(a.first_parameter()?);
    o.f(a.last_parameter()?);
    Ok(())
}
fn pr_d0(o: &mut Out, a: &dyn Curve3dAdaptor, u: f64) -> AResult<()> {
    o.p3(&a.value(u)?);
    Ok(())
}
fn curve_full(o: &mut Out, a: &dyn Curve3dAdaptor) -> AResult<()> {
    curve_basics(o, a)?;
    o.b(a.is_closed()?);
    o.b(a.is_periodic()?);
    att(o, |o| {
        let _: () = o.f(a.period()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.i(a.continuity()? as i64);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.f(a.resolution(1.0e-3)?);
        Ok(())
    });
    Ok(())
}
fn surf_basics(o: &mut Out, a: &dyn SurfaceAdaptor) -> AResult<()> {
    o.i(a.get_type()? as i64);
    o.f(a.first_u_parameter()?);
    o.f(a.last_u_parameter()?);
    o.f(a.first_v_parameter()?);
    o.f(a.last_v_parameter()?);
    Ok(())
}
fn surf_full(o: &mut Out, a: &dyn SurfaceAdaptor) -> AResult<()> {
    surf_basics(o, a)?;
    o.b(a.is_u_closed()?);
    o.b(a.is_v_closed()?);
    o.b(a.is_u_periodic()?);
    att(o, |o| {
        let _: () = o.f(a.u_period()?);
        Ok(())
    });
    o.b(a.is_v_periodic()?);
    att(o, |o| {
        let _: () = o.f(a.v_period()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.i(a.u_continuity()? as i64);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.i(a.v_continuity()? as i64);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.f(a.u_resolution(1.0e-3)?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.f(a.v_resolution(1.0e-3)?);
        Ok(())
    });
    Ok(())
}
fn curve_handle(o: &mut Out, c: Option<Arc<dyn Curve3dAdaptor>>) -> AResult<()> {
    o.b(c.is_none());
    if let Some(c) = c {
        curve_basics(o, c.as_ref())?;
    }
    Ok(())
}
fn surf_handle(o: &mut Out, c: Option<Arc<dyn SurfaceAdaptor>>) -> AResult<()> {
    o.b(c.is_none());
    if let Some(c) = c {
        surf_basics(o, c.as_ref())?;
    }
    Ok(())
}
fn is_infinite(x: f64) -> bool {
    x.abs() >= 1.0e100
}
fn curve_tag(c: &dyn Curve) -> i64 {
    let a = c.as_any();
    if a.is::<geom::Line>() {
        0
    } else if a.is::<geom::Circle>() {
        1
    } else if a.is::<geom::Ellipse>() {
        2
    } else if a.is::<geom::Parabola>() {
        3
    } else if a.is::<geom::Hyperbola>() {
        4
    } else if a.is::<geom::BezierCurve>() {
        5
    } else if a.is::<geom::BSplineCurve>() {
        6
    } else if a.is::<geom::TrimmedCurve>() {
        7
    } else if a.is::<geom::OffsetCurve>() {
        8
    } else {
        9
    }
}
fn surf_tag(s: &dyn Surface) -> i64 {
    let a = s.as_any();
    if a.is::<geom::Plane>() {
        0
    } else if a.is::<geom::CylindricalSurface>() {
        1
    } else if a.is::<geom::ConicalSurface>() {
        2
    } else if a.is::<geom::SphericalSurface>() {
        3
    } else if a.is::<geom::ToroidalSurface>() {
        4
    } else if a.is::<geom::BezierSurface>() {
        5
    } else if a.is::<geom::BSplineSurface>() {
        6
    } else if a.is::<geom::SurfaceOfRevolution>() {
        7
    } else if a.is::<geom::SurfaceOfLinearExtrusion>() {
        8
    } else if a.is::<geom::OffsetSurface>() {
        9
    } else if a.is::<geom::RectangularTrimmedSurface>() {
        10
    } else {
        11
    }
}

/// The typed accessors of a curve adaptor, in the order of the oracle (`cos` skips the Bezier and B-spline accessors).
fn curve_typed(o: &mut Out, a: &dyn Curve3dAdaptor, with_splines: bool, rational_att: bool) -> AResult<()> {
    att(o, |o| {
        let _: () = o.lin(&a.line()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.circ(&a.circle()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.elips(&a.ellipse()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.hypr(&a.hyperbola()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.parab(&a.parabola()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.i(a.degree()?.into());
        Ok(())
    });
    if rational_att {
        att(o, |o| {
            let _: () = o.b(a.is_rational()?);
            Ok(())
        });
    } else {
        o.b(a.is_rational()?);
    }
    att(o, |o| {
        let _: () = o.i(a.nb_poles()?.into());
        Ok(())
    });
    att(o, |o| {
        let _: () = o.i(a.nb_knots()?.into());
        Ok(())
    });
    if with_splines {
        att(o, |o| {
            let b = a.bezier()?;
            o.b(b.is_none());
            let b = b.ok_or(AdaptorError::NullObject)?;
            o.i(b.nb_poles().into());
            o.p3(&b.pole(1)?);
            Ok(())
        });
        att(o, |o| {
            let b = a.bspline()?;
            o.b(b.is_none());
            let b = b.ok_or(AdaptorError::NullObject)?;
            o.i(b.nb_poles().into());
            o.p3(&b.pole(1)?);
            o.f(b.knot(1)?);
            Ok(())
        });
    }
    att(o, |o| {
        let b = a.offset_curve()?;
        o.b(b.is_none());
        let b = b.ok_or(AdaptorError::NullObject)?;
        o.f(b.offset());
        o.d3(&b.direction());
        Ok(())
    });
    Ok(())
}

fn curve_eval_all(o: &mut Out, a: &dyn Curve3dAdaptor, u: f64, ord: i32, dn_to: i32) {
    for q in 0..4 {
        match (q + ord) % 4 {
            0 => att(o, |o| {
                let _: () = o.p3(&a.value(u)?);
                Ok(())
            }),
            1 => att(o, |o| {
                let (p, d1) = a.d1(u)?;
                o.p3(&p);
                o.v3(&d1);
                Ok(())
            }),
            2 => att(o, |o| {
                let (p, d1, d2) = a.d2(u)?;
                o.p3(&p);
                o.v3(&d1);
                o.v3(&d2);
                Ok(())
            }),
            _ => att(o, |o| {
                let (p, d1, d2, d3) = a.d3(u)?;
                o.p3(&p);
                o.v3(&d1);
                o.v3(&d2);
                o.v3(&d3);
                Ok(())
            }),
        }
    }
    for n in 1..=dn_to {
        att(o, |o| {
            let _: () = o.v3(&a.dn(u, n)?);
            Ok(())
        });
    }
}
fn curve_eval_plain(o: &mut Out, a: &dyn Curve3dAdaptor, u: f64, dn_to: i32) {
    curve_eval_all(o, a, u, 0, dn_to);
}

fn surf_eval_all(o: &mut Out, a: &dyn SurfaceAdaptor, u: f64, v: f64, ord: i32, dns: &[[i32; 2]]) {
    for q in 0..4 {
        match (q + ord) % 4 {
            0 => att(o, |o| {
                let _: () = o.p3(&a.value(u, v)?);
                Ok(())
            }),
            1 => att(o, |o| {
                let r = a.d1(u, v)?;
                o.p3(&r.point);
                o.v3(&r.d1u);
                o.v3(&r.d1v);
                Ok(())
            }),
            2 => att(o, |o| {
                let r = a.d2(u, v)?;
                o.p3(&r.point);
                for x in [r.d1u, r.d1v, r.d2u, r.d2v, r.d2uv] {
                    o.v3(&x);
                }
                Ok(())
            }),
            _ => att(o, |o| {
                let r = a.d3(u, v)?;
                o.p3(&r.point);
                for x in [r.d1u, r.d1v, r.d2u, r.d2v, r.d2uv, r.d3u, r.d3v, r.d3uuv, r.d3uvv] {
                    o.v3(&x);
                }
                Ok(())
            }),
        }
    }
    for n in dns {
        att(o, |o| {
            let _: () = o.v3(&a.dn(u, v, n[0], n[1])?);
            Ok(())
        });
    }
}

fn surf_typed(o: &mut Out, a: &dyn SurfaceAdaptor, with_handles: bool, rational_att: bool) -> AResult<()> {
    att(o, |o| {
        let _: () = o.pln(&a.plane()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.cyl(&a.cylinder()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.cone(&a.cone()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.sph(&a.sphere()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.tor(&a.torus()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.i(a.u_degree()?.into());
        Ok(())
    });
    att(o, |o| {
        let _: () = o.i(a.nb_u_poles()?.into());
        Ok(())
    });
    att(o, |o| {
        let _: () = o.i(a.v_degree()?.into());
        Ok(())
    });
    att(o, |o| {
        let _: () = o.i(a.nb_v_poles()?.into());
        Ok(())
    });
    att(o, |o| {
        let _: () = o.i(a.nb_u_knots()?.into());
        Ok(())
    });
    att(o, |o| {
        let _: () = o.i(a.nb_v_knots()?.into());
        Ok(())
    });
    if rational_att {
        att(o, |o| {
            let _: () = o.b(a.is_u_rational()?);
            Ok(())
        });
        att(o, |o| {
            let _: () = o.b(a.is_v_rational()?);
            Ok(())
        });
    } else {
        o.b(a.is_u_rational()?);
        o.b(a.is_v_rational()?);
    }
    att(o, |o| {
        let b = a.bezier()?;
        o.b(b.is_none());
        let b = b.ok_or(AdaptorError::NullObject)?;
        if with_handles {
            o.i(b.nb_u_poles().into());
            o.i(b.nb_v_poles().into());
            o.p3(&b.pole(1, 1)?);
        }
        Ok(())
    });
    att(o, |o| {
        let b = a.bspline()?;
        o.b(b.is_none());
        let b = b.ok_or(AdaptorError::NullObject)?;
        if with_handles {
            o.i(b.nb_u_poles().into());
            o.i(b.nb_v_poles().into());
            o.p3(&b.pole(1, 1)?);
        }
        Ok(())
    });
    att(o, |o| {
        let _: () = o.a1(&a.axe_of_revolution()?);
        Ok(())
    });
    att(o, |o| {
        let _: () = o.d3(&a.direction()?);
        Ok(())
    });
    Ok(())
}

// ---- construction helpers ---------------------------------------------------------------------------------------------------
fn gcurve(world: &World, c: usize, full: bool, f: f64, l: f64) -> AResult<geomadaptor::Curve> {
    if full {
        geomadaptor::Curve::with_curve(world.curves[c].clone())
    } else {
        geomadaptor::Curve::with_range(world.curves[c].clone(), f, l)
    }
}
fn basis_curve(world: &World, c: usize, full: bool, f: f64, l: f64) -> AResult<Arc<dyn Curve3dAdaptor>> {
    Ok(Arc::new(gcurve(world, c, full, f, l)?))
}
fn cos_of(world: &World, g: usize, s: usize) -> AResult<Arc<CurveOnSurface>> {
    let (c2, f, l) = &world.curves2d[g];
    let a2 = geom2dadaptor::Curve::with_range(c2.clone(), *f, *l)?;
    let r = world.range0[s];
    let sa = geomadaptor::Surface::with_range(world.surfaces[s].clone(), r.0, r.1, r.2, r.3, 0.0, 0.0)?;
    Ok(Arc::new(CurveOnSurface::with_curve_surface(
        Arc::new(a2) as Arc<dyn Curve2dAdaptor>,
        Arc::new(sa) as Arc<dyn SurfaceAdaptor>,
    )?))
}
/// The transformed curve of the oracle: a curve on a surface (`cs >= 0`) or a Geom curve.
fn tcurve(
    world: &World,
    tr: &Trsf,
    cs: i32,
    g: i32,
    s: i32,
    c: i32,
    full: bool,
    f: f64,
    l: f64,
) -> AResult<TransformedCurve> {
    if cs >= 0 {
        let mut a = TransformedCurve::new();
        a.set_trsf(*tr);
        a.load_curve_on_surface(cos_of(world, g as usize, s as usize)?);
        Ok(a)
    } else if full {
        TransformedCurve::with_curve(world.curves[c as usize].clone(), *tr)
    } else {
        TransformedCurve::with_range(world.curves[c as usize].clone(), f, l, *tr)
    }
}
fn dn_list() -> [[i32; 2]; 8] {
    [[1, 0], [0, 1], [1, 1], [2, 0], [0, 2], [2, 1], [1, 2], [3, 1]]
}
/// The `n` values the oracle reads from the array after `Intervals`; `replaced` is the size of the array when the C++ call
/// replaced it (`BSplCLib::Intervals`), so that reading past it fails.
fn intervals_n(
    a: &dyn Curve3dAdaptor,
    s: GeomAbsShape,
    n: usize,
    replaced: Option<usize>,
) -> AResult<std::vec::Vec<f64>> {
    let mut t = vec![0.0; n.max(replaced.unwrap_or(0))];
    a.intervals(&mut t, s)?;
    if replaced.is_some_and(|k| k < n) {
        return Err(AdaptorError::OutOfRange);
    }
    t.truncate(n);
    Ok(t)
}
fn surf_intervals_n(x: &geomadaptor::Surface, v: bool, s: GeomAbsShape) -> AResult<std::vec::Vec<f64>> {
    let replaced = x.intervals_replace_array(v, s)?;
    let mut t = vec![0.0; 8usize.max(replaced.unwrap_or(0))];
    if v {
        x.v_intervals(&mut t, s)?;
    } else {
        x.u_intervals(&mut t, s)?;
    }
    if replaced.is_some_and(|k| k < 8) {
        return Err(AdaptorError::OutOfRange);
    }
    t.truncate(8);
    Ok(t)
}

/// The tokens of a definition record: the printed specification is data, the values after it are computed.
fn tok_of(w: &str) -> Tok {
    if w.len() == 16 && w.bytes().all(|b| b.is_ascii_hexdigit()) {
        Tok::F(f64::from_bits(u64::from_str_radix(w, 16).unwrap()))
    } else {
        Tok::I(w.parse::<i128>().unwrap())
    }
}

fn run_def(world: &World, family: &str, idx: usize, want: &[&str]) -> AResult<std::vec::Vec<Tok>> {
    let mut o = Out::default();
    match family {
        "def_gcurve" => {
            let k = want.len() - 5;
            o.0.extend(want[..k].iter().map(|w| tok_of(w)));
            let c = &world.curves[idx];
            let (cf, cl) = (c.first_parameter(), c.last_parameter());
            o.f(cf);
            o.f(cl);
            let (f, l) = if is_infinite(cf) || is_infinite(cl) { (-2.0, 3.0) } else { (cf, cl) };
            o.p3(&c.eval_d0(f + 0.4 * (l - f))?);
        }
        "def_gsurface" => {
            let k = want.len() - 11;
            o.0.extend(want[..k].iter().map(|w| tok_of(w)));
            let s = &world.surfaces[idx];
            let (a, b, c, d) = s.bounds();
            for x in [a, b, c, d] {
                o.f(x);
            }
            let (u1, u2, v1, v2) = world.range0[idx];
            o.p3(&s.eval_d0(u1 + 0.4 * (u2 - u1), v1 + 0.3 * (v2 - v1))?);
            o.0.extend(want[want.len() - 4..].iter().map(|w| tok_of(w)));
        }
        _ => {
            let k = want.len() - 4;
            o.0.extend(want[..k].iter().map(|w| tok_of(w)));
            let (c, f, l) = &world.curves2d[idx];
            o.f(c.first_parameter());
            o.f(c.last_parameter());
            let p = c.value(0.5 * (f + l))?;
            o.f(p.x());
            o.f(p.y());
        }
    }
    Ok(o.0)
}

fn run(world: &World, family: &str, words: &[&str]) -> AResult<std::vec::Vec<Tok>> {
    let mut rd = Rd::new(words);
    let mut o = Out::default();
    let ngc = world.curves.len();
    let ngs = world.surfaces.len();
    match family {
        // ---- GeomAdaptor_Curve
        "gac_load" => {
            let (c, v, full, f, l) = (rd.i() as usize, rd.i(), rd.b(), rd.f(), rd.f());
            rd.finish();
            let load = |x: &mut geomadaptor::Curve| -> AResult<()> {
                if full { x.load(world.curves[c].clone()) } else { x.load_range(world.curves[c].clone(), f, l) }
            };
            let mut a = match v {
                0 => geomadaptor::Curve::new(),
                1 => gcurve(world, c, full, f, l)?,
                2 => geomadaptor::Curve::with_curve(world.curves[c].clone())?,
                3 => {
                    let mut a = geomadaptor::Curve::with_range(world.curves[c].clone(), f, l)?;
                    a.reset();
                    a
                }
                _ => geomadaptor::Curve::with_curve(world.curves[(c + 1) % ngc].clone())?,
            };
            if v == 0 || v == 2 || v == 4 {
                load(&mut a)?;
            }
            if v == 3 {
                curve_basics(&mut o, &a)?;
                o.b(a.curve().is_none());
            } else {
                curve_full(&mut o, &a)?;
                o.b(a.curve().is_some_and(|x| std::ptr::addr_eq(Arc::as_ptr(x), Arc::as_ptr(&world.curves[c]))));
            }
        }
        "gac_load_bad" => {
            let (c, k, f, l) = (rd.i() as usize, rd.i(), rd.f(), rd.f());
            rd.finish();
            let mut a = geomadaptor::Curve::new();
            match k {
                0 => a.load_opt(None)?,
                1 => a.load_range(world.curves[c].clone(), l + 0.01, f)?,
                2 => a.load_range(world.curves[c].clone(), f, f - 0.5e-7)?,
                _ => a = geomadaptor::Curve::with_range_opt(None, f, l)?,
            }
            curve_basics(&mut o, &a)?;
        }
        "gac_eval" => {
            let (ord, c, full, f, l) = (rd.i(), rd.i() as usize, rd.b(), rd.f(), rd.f());
            let t = rd.f3();
            rd.finish();
            let a = gcurve(world, c, full, f, l)?;
            for u in t {
                curve_eval_all(&mut o, &a, u, ord, 5);
            }
        }
        "gac_intervals" => {
            let (c, full, f, l, s) = (rd.i() as usize, rd.b(), rd.f(), rd.f(), rd.i() as usize);
            rd.finish();
            let a = gcurve(world, c, full, f, l)?;
            att(&mut o, |o| {
                let _: () = o.i(a.continuity()? as i64);
                Ok(())
            });
            att(&mut o, |o| {
                let _: () = o.i(a.nb_intervals(SHAPES[s])?.into());
                Ok(())
            });
            att(&mut o, |o| {
                for x in intervals_n(&a, SHAPES[s], 8, a.intervals_replace_array(SHAPES[s])?)? {
                    o.f(x);
                }
                Ok(())
            });
        }
        "gac_info" => {
            let (c, full, f, l) = (rd.i() as usize, rd.b(), rd.f(), rd.f());
            rd.finish();
            let a = gcurve(world, c, full, f, l)?;
            curve_basics(&mut o, &a)?;
            curve_typed(&mut o, &a, true, false)?;
        }
        "gac_resolution" => {
            let (c, full, f, l, rr) = (rd.i() as usize, rd.b(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let a = gcurve(world, c, full, f, l)?;
            att(&mut o, |o| {
                let _: () = o.f(a.resolution(rr)?);
                Ok(())
            });
        }
        "gac_trim" => {
            let (c, full, f, l, t1, t2, tol) = (rd.i() as usize, rd.b(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let a = gcurve(world, c, full, f, l)?;
            let t = a.trim(t1, t2, tol)?;
            curve_basics(&mut o, t.as_ref())?;
            att(&mut o, |o| pr_d0(o, t.as_ref(), 0.5 * (t1 + t2)));
            let cp = a.shallow_copy()?;
            curve_basics(&mut o, cp.as_ref())?;
            att(&mut o, |o| pr_d0(o, cp.as_ref(), 0.5 * (f + l)));
            att(&mut o, |o| {
                let _: () = o.i(cp.nb_intervals(GeomAbsShape::C1)?.into());
                Ok(())
            });
        }
        // ---- GeomAdaptor_Surface
        "gas_load" => {
            let (s, v, full) = (rd.i() as usize, rd.i(), rd.b());
            let (u1, u2, v1, v2, tu, tv) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let load = |x: &mut geomadaptor::Surface| -> AResult<()> {
                if full {
                    x.load(world.surfaces[s].clone())
                } else {
                    x.load_range(world.surfaces[s].clone(), u1, u2, v1, v2, tu, tv)
                }
            };
            let mut x = match v {
                0 | 4 => geomadaptor::Surface::new(),
                1 => {
                    if full {
                        geomadaptor::Surface::with_surface(world.surfaces[s].clone())?
                    } else {
                        geomadaptor::Surface::with_range(world.surfaces[s].clone(), u1, u2, v1, v2, tu, tv)?
                    }
                }
                2 => geomadaptor::Surface::with_surface(world.surfaces[s].clone())?,
                _ => geomadaptor::Surface::with_surface(world.surfaces[(s + 1) % ngs].clone())?,
            };
            if v != 1 {
                load(&mut x)?;
            }
            if v == 4 {
                let cp = x.shallow_copy()?;
                surf_basics(&mut o, cp.as_ref())?;
            }
            surf_full(&mut o, &x)?;
            o.f(x.tolerance_u());
            o.f(x.tolerance_v());
            o.b(x.surface().is_some_and(|y| std::ptr::addr_eq(Arc::as_ptr(y), Arc::as_ptr(&world.surfaces[s]))));
        }
        "gas_load_bad" => {
            let (s, k) = (rd.i() as usize, rd.i());
            let (u1, u2, v1, v2) = (rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let mut x = geomadaptor::Surface::new();
            match k {
                0 => x.load_opt(None)?,
                1 => x.load_range(world.surfaces[s].clone(), u2 + 0.1, u1, v1, v2, 0.0, 0.0)?,
                2 => x.load_range(world.surfaces[s].clone(), u1, u2, v2 + 0.1, v1, 0.0, 0.0)?,
                _ => x = geomadaptor::Surface::with_range(world.surfaces[s].clone(), u1, u1, v1, v1, 0.0, 0.0)?,
            }
            surf_basics(&mut o, &x)?;
        }
        "gas_eval" => {
            let (ord, s) = (rd.i(), rd.i() as usize);
            let (u1, u2, v1, v2, tu, tv) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            let pts = [[rd.f(), rd.f()], [rd.f(), rd.f()]];
            rd.finish();
            let x = geomadaptor::Surface::with_range(world.surfaces[s].clone(), u1, u2, v1, v2, tu, tv)?;
            for q in pts {
                surf_eval_all(&mut o, &x, q[0], q[1], ord, &dn_list());
            }
        }
        "gas_intervals" => {
            let (s, u1, u2, v1, v2, sh) = (rd.i() as usize, rd.f(), rd.f(), rd.f(), rd.f(), rd.i() as usize);
            rd.finish();
            let x = geomadaptor::Surface::with_range(world.surfaces[s].clone(), u1, u2, v1, v2, 0.0, 0.0)?;
            att(&mut o, |o| {
                let _: () = o.i(x.nb_u_intervals(SHAPES[sh])?.into());
                Ok(())
            });
            att(&mut o, |o| {
                surf_intervals_n(&x, false, SHAPES[sh])?.iter().for_each(|v| o.f(*v));
                Ok(())
            });
            att(&mut o, |o| {
                let _: () = o.i(x.nb_v_intervals(SHAPES[sh])?.into());
                Ok(())
            });
            att(&mut o, |o| {
                surf_intervals_n(&x, true, SHAPES[sh])?.iter().for_each(|v| o.f(*v));
                Ok(())
            });
        }
        "gas_info" => {
            let (s, u1, u2, v1, v2) = (rd.i() as usize, rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let x = geomadaptor::Surface::with_range(world.surfaces[s].clone(), u1, u2, v1, v2, 0.0, 0.0)?;
            surf_basics(&mut o, &x)?;
            surf_typed(&mut o, &x, true, false)?;
            att(&mut o, |o| curve_handle(o, x.basis_curve()?));
            att(&mut o, |o| surf_handle(o, x.basis_surface()?));
            att(&mut o, |o| {
                let _: () = o.f(x.offset_value()?);
                Ok(())
            });
        }
        "gas_resolution" => {
            let (s, u1, u2, v1, v2, rr) = (rd.i() as usize, rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let x = geomadaptor::Surface::with_range(world.surfaces[s].clone(), u1, u2, v1, v2, 0.0, 0.0)?;
            att(&mut o, |o| {
                let _: () = o.f(x.u_resolution(rr)?);
                Ok(())
            });
            att(&mut o, |o| {
                let _: () = o.f(x.v_resolution(rr)?);
                Ok(())
            });
        }
        "gas_trim" => {
            let (s, u1, u2, v1, v2, tu, tv) = (rd.i() as usize, rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            let (a1, a2, b1, b2) = (rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let x = geomadaptor::Surface::with_range(world.surfaces[s].clone(), u1, u2, v1, v2, tu, tv)?;
            let t = x.u_trim(a1, a2, 1.0e-3)?;
            surf_basics(&mut o, t.as_ref())?;
            att(&mut o, |o| {
                let _: () = o.p3(&t.value(0.5 * (a1 + a2), 0.5 * (v1 + v2))?);
                Ok(())
            });
            let w = x.v_trim(b1, b2, 2.0e-3)?;
            surf_basics(&mut o, w.as_ref())?;
            att(&mut o, |o| {
                let _: () = o.p3(&w.value(0.5 * (u1 + u2), 0.5 * (b1 + b2))?);
                Ok(())
            });
            let cp = x.shallow_copy()?;
            surf_basics(&mut o, cp.as_ref())?;
            att(&mut o, |o| {
                let _: () = o.p3(&cp.value(0.5 * (u1 + u2), 0.5 * (v1 + v2))?);
                Ok(())
            });
            att(&mut o, |o| {
                o.i(cp.nb_u_intervals(GeomAbsShape::C1)?.into());
                o.i(cp.nb_v_intervals(GeomAbsShape::C1)?.into());
                Ok(())
            });
        }
        // ---- GeomAdaptor_TransformedCurve
        "gtc_eval" | "gtc_info" | "gtc_copy" => {
            if family == "gtc_info" {
                let _j = rd.i();
            }
            let tr = read_ts(&mut rd);
            let (cs, g, s, c, full, f, l) = (rd.i(), rd.i(), rd.i(), rd.i(), rd.b(), rd.f(), rd.f());
            match family {
                "gtc_eval" => {
                    let t = [rd.f(), rd.f()];
                    rd.finish();
                    let a = tcurve(world, &tr, cs, g, s, c, full, f, l)?;
                    curve_basics(&mut o, &a)?;
                    o.b(a.is_3d_curve());
                    o.b(a.is_curve_on_surface());
                    o.b(a.is_closed()?);
                    o.b(a.is_periodic()?);
                    att(&mut o, |o| {
                        let _: () = o.f(a.period()?);
                        Ok(())
                    });
                    att(&mut o, |o| {
                        let _: () = o.i(a.continuity()? as i64);
                        Ok(())
                    });
                    att(&mut o, |o| {
                        let _: () = o.i(a.nb_intervals(GeomAbsShape::C1)?.into());
                        Ok(())
                    });
                    att(&mut o, |o| {
                        for x in intervals_n(
                            &a,
                            GeomAbsShape::C1,
                            8,
                            if a.is_3d_curve() { a.curve().intervals_replace_array(GeomAbsShape::C1)? } else { None },
                        )? {
                            o.f(x);
                        }
                        Ok(())
                    });
                    att(&mut o, |o| {
                        let _: () = o.f(a.resolution(1.0e-3)?);
                        Ok(())
                    });
                    for u in t {
                        curve_eval_plain(&mut o, &a, u, 3);
                    }
                }
                "gtc_info" => {
                    rd.finish();
                    let a = tcurve(world, &tr, cs, g, s, c, full, f, l)?;
                    curve_basics(&mut o, &a)?;
                    curve_typed(&mut o, &a, cs < 0, true)?;
                    o.b(a.curve().curve().is_none());
                    o.b(a.geom_curve().is_none());
                }
                _ => {
                    let (t1, t2) = (rd.f(), rd.f());
                    rd.finish();
                    let a = tcurve(world, &tr, cs, g, s, c, full, f, l)?;
                    let cp = a.shallow_copy()?;
                    curve_basics(&mut o, cp.as_ref())?;
                    att(&mut o, |o| pr_d0(o, cp.as_ref(), 0.5 * (f + l)));
                    let tc = a.trimmed(t1, t2, 1.0e-3)?;
                    curve_basics(&mut o, &tc)?;
                    att(&mut o, |o| pr_d0(o, &tc, 0.5 * (t1 + t2)));
                    att(&mut o, |o| {
                        let _: () = o.f(tc.trsf().scale_factor());
                        Ok(())
                    });
                    let mut b = TransformedCurve::new();
                    b.load(world.curves[(c as usize + 2) % ngc].clone())?;
                    b.set_trsf(tr);
                    curve_basics(&mut o, &b)?;
                    att(&mut o, |o| {
                        let (bf, bl) = (b.first_parameter()?, b.last_parameter()?);
                        let uu = if is_infinite(bf) || is_infinite(bl) { 0.25 } else { 0.5 * (bf + bl) };
                        pr_d0(o, &b, uu)
                    });
                    let c3 = &world.curves[(c as usize + 3) % ngc];
                    if !is_infinite(c3.first_parameter()) && !is_infinite(c3.last_parameter()) {
                        let mut d = TransformedCurve::new();
                        d.load_range(c3.clone(), c3.first_parameter(), c3.last_parameter())?;
                        curve_basics(&mut o, &d)?;
                    }
                }
            }
        }
        // ---- GeomAdaptor_TransformedSurface
        "gts_eval" => {
            let tr = read_ts(&mut rd);
            let s = rd.i() as usize;
            let (u1, u2, v1, v2, tu, tv) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            let pts = [[rd.f(), rd.f()], [rd.f(), rd.f()]];
            rd.finish();
            let x = TransformedSurface::with_range(world.surfaces[s].clone(), u1, u2, v1, v2, tr, tu, tv)?;
            surf_full(&mut o, &x)?;
            o.f(x.tolerance_u());
            o.f(x.tolerance_v());
            o.b(x.has_trsf());
            let dns = [[1, 0], [0, 1], [1, 1], [2, 1], [0, 2]];
            for q in pts {
                surf_eval_all(&mut o, &x, q[0], q[1], 0, &dns);
            }
        }
        "gts_info" => {
            let tr = read_ts(&mut rd);
            let (s, u1, u2, v1, v2) = (rd.i() as usize, rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let x = TransformedSurface::with_range(world.surfaces[s].clone(), u1, u2, v1, v2, tr, 0.0, 0.0)?;
            surf_basics(&mut o, &x)?;
            surf_typed(&mut o, &x, true, true)?;
            att(&mut o, |o| curve_handle(o, x.basis_curve()?));
            att(&mut o, |o| surf_handle(o, x.basis_surface()?));
            att(&mut o, |o| {
                let _: () = o.f(x.offset_value()?);
                Ok(())
            });
            att(&mut o, |o| {
                let g = x.geom_surface_transformed()?.ok_or(AdaptorError::NullObject)?;
                o.i(surf_tag(g.as_ref()));
                o.i(x.adaptor_surface_transformed()?.get_type()? as i64);
                Ok(())
            });
            o.i(x.geom_surface_original().map_or(-1, |g| surf_tag(g.as_ref())));
            o.i(x.adaptor_surface_original().get_type()? as i64);
        }
        "gts_trim" => {
            let tr = read_ts(&mut rd);
            let tr2 = read_ts(&mut rd);
            let s = rd.i() as usize;
            let (u1, u2, v1, v2) = (rd.f(), rd.f(), rd.f(), rd.f());
            let (a1, a2, b1, b2) = (rd.f(), rd.f(), rd.f(), rd.f());
            let sh = rd.i() as usize;
            rd.finish();
            let x = TransformedSurface::with_range(world.surfaces[s].clone(), u1, u2, v1, v2, tr, 0.0, 0.0)?;
            att(&mut o, |o| {
                let nb = x.nb_u_intervals(SHAPES[sh])?;
                o.i(nb.into());
                let mut t = vec![0.0; nb as usize + 1];
                x.u_intervals(&mut t, SHAPES[sh])?;
                t.iter().for_each(|v| o.f(*v));
                Ok(())
            });
            att(&mut o, |o| {
                let nb = x.nb_v_intervals(SHAPES[sh])?;
                o.i(nb.into());
                let mut t = vec![0.0; nb as usize + 1];
                x.v_intervals(&mut t, SHAPES[sh])?;
                t.iter().for_each(|v| o.f(*v));
                Ok(())
            });
            let t = x.u_trim(a1, a2, 1.0e-3)?;
            surf_basics(&mut o, t.as_ref())?;
            att(&mut o, |o| {
                let _: () = o.p3(&t.value(0.5 * (a1 + a2), 0.5 * (v1 + v2))?);
                Ok(())
            });
            let w = x.v_trim(b1, b2, 2.0e-3)?;
            surf_basics(&mut o, w.as_ref())?;
            let cp = x.shallow_copy()?;
            surf_basics(&mut o, cp.as_ref())?;
            att(&mut o, |o| {
                let _: () = o.p3(&cp.value(0.5 * (u1 + u2), 0.5 * (v1 + v2))?);
                Ok(())
            });
            let mut y = TransformedSurface::new();
            y.load(world.surfaces[(s + 3) % ngs].clone(), tr)?;
            surf_basics(&mut o, &y)?;
            y.set_trsf(tr2);
            o.b(y.has_trsf());
            att(&mut o, |o| {
                let _: () = o.p3(&y.value(0.25, 0.5)?);
                Ok(())
            });
            let mut z = TransformedSurface::new();
            z.load_range(world.surfaces[s].clone(), u1, u2, v1, v2, tr, 0.0, 0.0)?;
            surf_basics(&mut o, &z)?;
            let z2 = TransformedSurface::with_surface(world.surfaces[s].clone(), tr)?;
            surf_basics(&mut o, &z2)?;
            let mut z3 = TransformedSurface::new();
            z3.set_trsf(tr);
            att(&mut o, |o| {
                let _: () = o.pln(&z3.plane()?);
                Ok(())
            });
            o.b(z3.has_trsf());
            att(&mut o, |o| {
                o.f(z.u_resolution(1.0e-3)?);
                o.f(z.v_resolution(1.0e-3)?);
                Ok(())
            });
        }
        // ---- GeomAdaptor_SurfaceOfRevolution
        "gsr_new" | "gsr_eval" | "gsr_info" | "gsr_trim" => {
            let (c, full, f, l) = (rd.i() as usize, rd.b(), rd.f(), rd.f());
            let ax = Ax1::new(pnt(rd.f3()), dir(rd.f3()));
            let hc = basis_curve(world, c, full, f, l)?;
            match family {
                "gsr_new" => {
                    let v = rd.i();
                    rd.finish();
                    let x = match v {
                        0 => SurfaceOfRevolution::with_axis(hc, ax)?,
                        1 => {
                            let mut x = SurfaceOfRevolution::new();
                            x.load_curve(hc)?;
                            x.load_axis(ax)?;
                            x
                        }
                        _ => {
                            let mut x = SurfaceOfRevolution::with_curve(hc)?;
                            x.load_axis(ax)?;
                            x
                        }
                    };
                    surf_full(&mut o, &x)?;
                    att(&mut o, |o| {
                        let _: () = o.a1(&x.axe_of_revolution()?);
                        Ok(())
                    });
                    att(&mut o, |o| {
                        let _: () = o.a3(x.axis());
                        Ok(())
                    });
                    att(&mut o, |o| curve_handle(o, x.basis_curve()?));
                    att(&mut o, |o| {
                        let nb = x.nb_v_intervals(GeomAbsShape::C2)?;
                        o.i(nb.into());
                        let mut t = vec![0.0; nb as usize + 1];
                        x.v_intervals(&mut t, GeomAbsShape::C2)?;
                        t.iter().for_each(|v| o.f(*v));
                        Ok(())
                    });
                    att(&mut o, |o| {
                        let nb = x.nb_u_intervals(GeomAbsShape::C2)?;
                        o.i(nb.into());
                        let mut t = vec![0.0; nb as usize + 1];
                        x.u_intervals(&mut t, GeomAbsShape::C2)?;
                        t.iter().for_each(|v| o.f(*v));
                        Ok(())
                    });
                }
                "gsr_eval" => {
                    let pts = [[rd.f(), rd.f()], [rd.f(), rd.f()]];
                    rd.finish();
                    let x = SurfaceOfRevolution::with_axis(hc, ax)?;
                    let dns = [[1, 0], [0, 1], [1, 1], [2, 1], [0, 2], [0, 0]];
                    for q in pts {
                        surf_eval_all(&mut o, &x, q[0], q[1], 0, &dns);
                    }
                }
                "gsr_info" => {
                    rd.finish();
                    let x = SurfaceOfRevolution::with_axis(hc, ax)?;
                    surf_basics(&mut o, &x)?;
                    surf_typed(&mut o, &x, false, true)?;
                    att(&mut o, |o| surf_handle(o, x.basis_surface()?));
                    att(&mut o, |o| {
                        let _: () = o.f(x.offset_value()?);
                        Ok(())
                    });
                }
                _ => {
                    let (t1, t2) = (rd.f(), rd.f());
                    let c2 = rd.i() as usize;
                    let (f2, l2) = (rd.f(), rd.f());
                    let ax2 = Ax1::new(pnt(rd.f3()), dir(rd.f3()));
                    rd.finish();
                    let x = SurfaceOfRevolution::with_axis(hc.clone(), ax)?;
                    let u = x.u_trim(0.0, 2.0 * std::f64::consts::PI, 1.0e-3)?;
                    surf_basics(&mut o, u.as_ref())?;
                    let w = x.v_trim(t1, t2, 1.0e-3)?;
                    surf_basics(&mut o, w.as_ref())?;
                    att(&mut o, |o| {
                        let _: () = o.p3(&w.value(1.0, 0.5 * (t1 + t2))?);
                        Ok(())
                    });
                    let cp = x.shallow_copy()?;
                    surf_basics(&mut o, cp.as_ref())?;
                    att(&mut o, |o| {
                        let _: () = o.p3(&cp.value(2.0, 0.5 * (f + l))?);
                        Ok(())
                    });
                    let mut y = SurfaceOfRevolution::new();
                    y.load_curve(hc)?;
                    y.load_axis(ax2)?;
                    surf_basics(&mut o, &y)?;
                    y.load_curve(basis_curve(world, c2, false, f2, l2)?)?;
                    surf_basics(&mut o, &y)?;
                }
            }
        }
        // ---- GeomAdaptor_SurfaceOfLinearExtrusion
        "gse_new" | "gse_eval" | "gse_info" | "gse_trim" => {
            if family == "gse_new" || family == "gse_trim" {
                let _j = rd.i();
            }
            let (c, full, f, l) = (rd.i() as usize, rd.b(), rd.f(), rd.f());
            let d = dir(rd.f3());
            let hc = basis_curve(world, c, full, f, l)?;
            match family {
                "gse_new" => {
                    let v = rd.i();
                    rd.finish();
                    let x = match v {
                        0 => SurfaceOfLinearExtrusion::with_direction(hc, d)?,
                        1 => {
                            let mut x = SurfaceOfLinearExtrusion::new();
                            x.load_curve(hc)?;
                            x.load_direction(d)?;
                            x
                        }
                        _ => {
                            let mut x = SurfaceOfLinearExtrusion::with_curve(hc)?;
                            x.load_direction(d)?;
                            x
                        }
                    };
                    surf_full(&mut o, &x)?;
                    att(&mut o, |o| {
                        let _: () = o.d3(&x.direction()?);
                        Ok(())
                    });
                    att(&mut o, |o| curve_handle(o, x.basis_curve()?));
                    att(&mut o, |o| {
                        let nb = x.nb_u_intervals(GeomAbsShape::C2)?;
                        o.i(nb.into());
                        let mut t = vec![0.0; nb as usize + 1];
                        x.u_intervals(&mut t, GeomAbsShape::C2)?;
                        t.iter().for_each(|v| o.f(*v));
                        Ok(())
                    });
                    att(&mut o, |o| {
                        let nb = x.nb_v_intervals(GeomAbsShape::C2)?;
                        o.i(nb.into());
                        let mut t = vec![0.0; nb as usize + 1];
                        x.v_intervals(&mut t, GeomAbsShape::C2)?;
                        t.iter().for_each(|v| o.f(*v));
                        Ok(())
                    });
                }
                "gse_eval" => {
                    let pts = [[rd.f(), rd.f()], [rd.f(), rd.f()]];
                    rd.finish();
                    let x = SurfaceOfLinearExtrusion::with_direction(hc, d)?;
                    let dns = [[1, 0], [0, 1], [1, 1], [2, 1], [0, 2], [0, 0]];
                    for q in pts {
                        surf_eval_all(&mut o, &x, q[0], q[1], 0, &dns);
                    }
                }
                "gse_info" => {
                    rd.finish();
                    let x = SurfaceOfLinearExtrusion::with_direction(hc, d)?;
                    surf_basics(&mut o, &x)?;
                    surf_typed(&mut o, &x, false, true)?;
                    att(&mut o, |o| surf_handle(o, x.basis_surface()?));
                    att(&mut o, |o| {
                        let _: () = o.f(x.offset_value()?);
                        Ok(())
                    });
                }
                _ => {
                    let (t1, t2) = (rd.f(), rd.f());
                    rd.finish();
                    let x = SurfaceOfLinearExtrusion::with_direction(hc.clone(), d)?;
                    let u = x.u_trim(t1, t2, 1.0e-3)?;
                    surf_basics(&mut o, u.as_ref())?;
                    let w = x.v_trim(t1, t2, 1.0e-3)?;
                    surf_basics(&mut o, w.as_ref())?;
                    att(&mut o, |o| {
                        let _: () = o.p3(&w.value(0.5 * (t1 + t2), 0.5)?);
                        Ok(())
                    });
                    let cp = x.shallow_copy()?;
                    surf_basics(&mut o, cp.as_ref())?;
                    att(&mut o, |o| {
                        let _: () = o.p3(&cp.value(0.5 * (f + l), 0.25)?);
                        Ok(())
                    });
                    let mut y = SurfaceOfLinearExtrusion::new();
                    y.load_curve(hc.clone())?;
                    y.load_direction(Dir::new(0.0, 0.3, 1.0).unwrap())?;
                    surf_basics(&mut o, &y)?;
                    y.load_curve(hc)?;
                    surf_basics(&mut o, &y)?;
                }
            }
        }
        // ---- GeomAdaptor::MakeCurve, GeomAdaptor::MakeSurface
        "make_curve" => {
            let (c, v, full, f, l) = (rd.i() as usize, rd.i(), rd.b(), rd.f(), rd.f());
            rd.finish();
            let a = gcurve(world, c, full, f, l)?;
            let g = match v {
                0 => geomadaptor::make_curve(&a)?,
                1 => {
                    let t = TransformedCurve::with_range(world.curves[c].clone(), f, l, Trsf::new())?;
                    geomadaptor::make_curve(&t)?
                }
                _ => {
                    let t = a.trim(f + 0.1 * (l - f), l - 0.2 * (l - f), 0.0)?;
                    geomadaptor::make_curve(t.as_ref())?
                }
            };
            o.i(curve_tag(g.as_ref()));
            o.f(g.first_parameter());
            o.f(g.last_parameter());
            if let Some(tc) = g.as_any().downcast_ref::<geom::TrimmedCurve>() {
                o.i(curve_tag(tc.basis_curve().as_ref()));
            }
            o.p3(&g.eval_d0(f + 0.5 * (l - f))?);
        }
        "make_surface" => {
            let (s, v, trim) = (rd.i() as usize, rd.i(), rd.b());
            let (u1, u2, v1, v2) = (rd.f(), rd.f(), rd.f(), rd.f());
            let cc = rd.i() as usize;
            let (ff, ll) = (rd.f(), rd.f());
            let dd = dir(rd.f3());
            let axm = Ax1::new(pnt(rd.f3()), dir(rd.f3()));
            rd.finish();
            let g = match v {
                0 => {
                    let a = geomadaptor::Surface::with_range(world.surfaces[s].clone(), u1, u2, v1, v2, 0.0, 0.0)?;
                    geomadaptor::make_surface(&a, trim)?
                }
                1 => {
                    let a = TransformedSurface::with_range(
                        world.surfaces[s].clone(),
                        u1,
                        u2,
                        v1,
                        v2,
                        Trsf::new(),
                        0.0,
                        0.0,
                    )?;
                    geomadaptor::make_surface(&a, trim)?
                }
                2 => {
                    let a = SurfaceOfRevolution::with_axis(basis_curve(world, cc, false, ff, ll)?, axm)?;
                    geomadaptor::make_surface(&a, trim)?
                }
                _ => {
                    let a = SurfaceOfLinearExtrusion::with_direction(basis_curve(world, cc, false, ff, ll)?, dd)?;
                    geomadaptor::make_surface(&a, trim)?
                }
            };
            o.i(surf_tag(g.as_ref()));
            let (a1, a2, b1, b2) = g.bounds();
            for x in [a1, a2, b1, b2] {
                o.f(x);
            }
            if let Some(tc) = g.as_any().downcast_ref::<geom::RectangularTrimmedSurface>() {
                o.i(surf_tag(tc.basis_surface().as_ref()));
            }
            att(&mut o, |o| {
                let _: () = o.p3(&g.eval_d0(u1 + 0.4 * (u2 - u1), v1 + 0.3 * (v2 - v1))?);
                Ok(())
            });
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
fn geomadaptor_matches_occt() {
    let world = read_world();
    let mut seen = HashSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut distinct = BTreeMap::<String, BTreeSet<String>>::new();
    let mut tally = Tally::default();
    let mut lines = 0;
    let mut throws = 0;
    let mut def_index = BTreeMap::<String, usize>::new();
    for (n, line) in DATA.lines().enumerate() {
        let (left, right) = line.split_once(" |").unwrap_or_else(|| panic!("line {} malformed", n + 1));
        let mut it = left.split_whitespace();
        let family = it.next().unwrap();
        let words: std::vec::Vec<&str> = it.collect();
        assert!(seen.insert(line.to_owned()), "duplicate line {}", n + 1);
        *counts.entry(family.to_owned()).or_default() += 1;
        distinct.entry(family.to_owned()).or_default().insert(left.to_owned());
        let want: std::vec::Vec<&str> = right.split_whitespace().collect();
        let got = if family.starts_with("def_") {
            let idx: usize = words[0].parse().unwrap();
            *def_index.entry(family.to_owned()).or_default() += 1;
            run_def(&world, family, idx, &want)
        } else {
            run(&world, family, &words)
        };
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
        "geomadaptor: {} families, {lines} lines ({throws} throws), {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
