//! `Adaptor3d_CurveOnSurface`: a 2D curve adaptor in the parametric space of a surface adaptor, as an `Adaptor3d_Curve`.
//!
//! Translated from `Adaptor3d_CurveOnSurface.hxx/.cxx` (OCCT 8.0.1). The mutable cache of `NbIntervals` (`myIntervals`,
//! `myIntCont`, written through a `const_cast` in C++) is a `Mutex`.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use super::{Curve3dAdaptor, GeomAbsCurveType, GeomAbsShape, GeomAbsSurfaceType, InterFunc, Result, SurfaceAdaptor};
use crate::adaptor2d::{AdaptorError, Curve2dAdaptor};
use crate::elclib;
use crate::elslib;
use crate::geom::{self, ResD1, ResD2, ResD3};
use crate::geom2d::{Curve2d, angular, pconfusion};
use crate::gp::{
    self, Ax1, Ax2, Ax22d, Circ, Circ2d, Dir, Elips, Elips2d, Hypr, Hypr2d, Lin, Parab, Parab2d, Pln, Pnt, Pnt2d, Vec,
    Vec2d,
};
use crate::math1d::FunctionRoots;
use std::sync::{Arc, Mutex};

const M_PI: f64 = std::f64::consts::PI;

fn tol() -> f64 {
    pconfusion() / 10.0
}
fn not_impl<T>() -> Result<T> {
    Err(AdaptorError::NotImplemented)
}

// ---- to3d: a 2D curve of the plane as a 3D curve
fn to3d_pnt(pl: &Pln, p: &Pnt2d) -> Pnt {
    elslib::value_pln(p.x(), p.y(), pl)
}
fn to3d_vec(pl: &Pln, v: &Vec2d) -> Vec {
    let mut vx = Vec::from_dir(pl.x_axis().direction());
    let mut vy = Vec::from_dir(pl.y_axis().direction());
    vx.multiply(v.x());
    vy.multiply(v.y());
    vx.add(&vy);
    vx
}
fn to3d_ax2(pl: &Pln, a: &Ax22d) -> Result<Ax2> {
    let p = to3d_pnt(pl, a.location());
    let vx = to3d_vec(pl, &Vec2d::new(a.x_axis().direction().x(), a.x_axis().direction().y()));
    let vy = to3d_vec(pl, &Vec2d::new(a.y_axis().direction().x(), a.y_axis().direction().y()));
    Ok(Ax2::new(p, Dir::from_vec(&vx.crossed(&vy))?, &Dir::from_vec(&vx)?)?)
}
fn to3d_circ(pl: &Pln, c: &Circ2d) -> Result<Circ> {
    Ok(Circ::new(to3d_ax2(pl, c.axis())?, c.radius())?)
}
fn to3d_elips(pl: &Pln, e: &Elips2d) -> Result<Elips> {
    Ok(Elips::new(to3d_ax2(pl, e.axis())?, e.major_radius(), e.minor_radius())?)
}
fn to3d_hypr(pl: &Pln, h: &Hypr2d) -> Result<Hypr> {
    Ok(Hypr::new(to3d_ax2(pl, h.axis())?, h.major_radius(), h.minor_radius())?)
}
fn to3d_parab(pl: &Pln, p: &Parab2d) -> Result<Parab> {
    Ok(Parab::new(to3d_ax2(pl, p.axis())?, p.focal())?)
}

#[allow(clippy::too_many_arguments)]
fn set_linear_form(
    dw: &Vec2d,
    d2w: &Vec2d,
    d1u: &Vec,
    d1v: &Vec,
    d2u: &Vec,
    d2v: &Vec,
    d2uv: &Vec,
    d3u: &Vec,
    d3v: &Vec,
    d3uuv: &Vec,
    d3uvv: &Vec,
    d3w: &Vec2d,
) -> Vec {
    let mut v31 = Vec::new(0.0, 0.0, 0.0);
    let mut v32 = Vec::new(0.0, 0.0, 0.0);
    let mut v33 = Vec::new(0.0, 0.0, 0.0);
    let mut v34 = Vec::new(0.0, 0.0, 0.0);
    let mut v3 = Vec::new(0.0, 0.0, 0.0);
    v31.set_linear_form_3(dw.x(), d1u, d2w.x() * dw.x(), d2u, d2w.x() * dw.y(), d2uv);
    let old = v31;
    v31.set_linear_form_3_plus(d3w.y(), d1v, d2w.y() * dw.x(), d2uv, d2w.y() * dw.y(), d2v, &old);
    v32.set_linear_form_2(dw.x() * dw.x() * dw.y(), d3uuv, dw.x() * dw.y() * dw.y(), d3uvv);
    let old = v32;
    v32.set_linear_form_2_plus(d2w.x() * dw.y() + dw.x() * d2w.y(), d2uv, dw.x() * dw.y() * dw.y(), d3uvv, &old);
    v33.set_linear_form_3(2.0 * d2w.x() * dw.x(), d2u, dw.x() * dw.x() * dw.x(), d3u, dw.x() * dw.x() * dw.y(), d3uuv);
    v34.set_linear_form_3(2.0 * d2w.y() * dw.y(), d2v, dw.y() * dw.y() * dw.x(), d3uvv, dw.y() * dw.y() * dw.y(), d3v);
    v3.set_linear_form_3_plus(1.0, &v31, 2.0, &v32, 1.0, &v33, &v34);
    v3
}

fn compare_bounds(p1: &mut Pnt2d, p2: &mut Pnt2d) {
    let (lx, ly) = (p1.x(), p1.y());
    let (rx, ry) = (p2.x(), p2.y());
    if lx > rx {
        p1.set_x(rx);
        p2.set_x(lx);
    }
    if ly > ry {
        p1.set_y(ry);
        p2.set_y(ly);
    }
}

fn arr(a: &[f64], i: i32) -> Result<f64> {
    usize::try_from(i - 1).ok().and_then(|k| a.get(k)).copied().ok_or(AdaptorError::OutOfRange)
}

// Hunt: the number of the knot that equals the coordinate (when the coordinate definitely equals a knot only)
fn hunt(a: &[f64], coord: f64, iloc: &mut i32) -> Result<()> {
    let t = tol();
    let mut i = 1;
    while i <= a.len() as i32 && (coord - arr(a, i)?).abs() > t {
        i += 1;
    }
    let d = (coord - arr(a, i)?).abs();
    if d < t {
        *iloc = i;
    } else if d > t {
        return not_impl();
    }
    Ok(())
}

fn reverse_param(in1: f64, in2: f64) -> (f64, f64) {
    if in1 > in2 { (in2, in1) } else { (in1, in2) }
}
fn reverse_param_i(in1: i32, in2: i32) -> (i32, i32) {
    if in1 > in2 { (in2, in1) } else { (in1, in2) }
}

// FindBounds: returns whether the derivative is null
fn find_bounds(a: &[f64], coord: f64, der: f64, bound1: &mut i32, bound2: &mut i32) -> Result<bool> {
    let t = tol();
    let mut n = 0;
    hunt(a, coord, &mut n)?;
    let mut der_null = false;
    if n == *bound1 {
        der_null = der.abs() <= t;
        *bound1 = n;
        *bound2 = n + 1;
        return Ok(der_null);
    }
    if n == *bound2 {
        der_null = der.abs() <= t;
        *bound1 = n - 1;
        *bound2 = n;
        return Ok(der_null);
    }
    if der.abs() > t {
        if der > 0.0 {
            *bound1 = n;
            *bound2 = n + 1;
        } else {
            *bound1 = n - 1;
            *bound2 = n;
        }
        der_null = false;
    }
    if der.abs() <= t {
        der_null = true;
        *bound1 = n - 1;
        *bound2 = n + 1;
    }
    Ok(der_null)
}

fn set_xy(index: i32, lb: &mut Pnt2d, rt: &mut Pnt2d, lo: f64, hi: f64) {
    if index == 1 {
        lb.set_x(lo);
        rt.set_x(hi);
    } else if index == 2 {
        lb.set_y(lo);
        rt.set_y(hi);
    }
}

// Locate1Coord over the knots of a B-spline curve
fn locate_1coord_curve(
    index: i32,
    uv: &Pnt2d,
    duv: &Vec2d,
    bspl: &geom::BSplineCurve,
    lb: &mut Pnt2d,
    rt: &mut Pnt2d,
) -> Result<()> {
    let t = tol();
    let (mut comp1, mut dcomp1) = (0.0, 0.0);
    let arr_k = bspl.knots();
    if index == 1 {
        comp1 = uv.x();
        dcomp1 = duv.x();
    }
    if index == 2 {
        comp1 = uv.y();
        dcomp1 = duv.y();
    }
    let lo = bspl.first_u_knot_index();
    let up = bspl.last_u_knot_index();
    let mut i = lo;
    while (bspl.knot(i)? - comp1).abs() > t && i != up {
        i += 1;
    }
    let cur = bspl.knot(i)?;
    if (comp1 - cur).abs() <= t {
        let (mut bnd1, mut bnd2) = (lo, up);
        let dis_null = find_bounds(arr_k, cur, dcomp1, &mut bnd1, &mut bnd2)?;
        (bnd1, bnd2) = reverse_param_i(bnd1, bnd2);
        if !dis_null {
            set_xy(index, lb, rt, bspl.knot(bnd1)?, bspl.knot(bnd2)?);
        } else if (comp1 - bspl.knot(lo)?).abs() <= t {
            set_xy(index, lb, rt, bspl.knot(lo)?, bspl.knot(lo + 1)?);
        } else if (comp1 - bspl.knot(up)?).abs() <= t {
            set_xy(index, lb, rt, bspl.knot(up - 1)?, bspl.knot(up)?);
        } else {
            set_xy(index, lb, rt, bspl.knot(bnd1)?, bspl.knot(bnd2)?);
        }
    } else {
        // the coordinate is not a knot
        let (mut f, mut l) = (0.0, 0.0);
        i = lo;
        while i < up {
            f = bspl.knot(i)?;
            l = bspl.knot(i + 1)?;
            if f < comp1 && l > comp1 {
                break;
            }
            i += 1;
        }
        (f, l) = reverse_param(f, l);
        if i != up {
            if dcomp1.abs() < t {
                set_xy(index, lb, rt, f, l);
            } else if dcomp1.abs() > t {
                if dcomp1 > 0.0 {
                    set_xy(index, lb, rt, comp1, l);
                } else if dcomp1 < 0.0 {
                    set_xy(index, lb, rt, f, comp1);
                }
            }
        } else {
            set_xy(index, lb, rt, comp1, bspl.knot(i)?);
        }
    }
    Ok(())
}

// Locate1Coord over the knots of a B-spline surface: returns whether the derivative is null
fn locate_1coord_surface(
    index: i32,
    uv: &Pnt2d,
    duv: &Vec2d,
    bs: &geom::BSplineSurface,
    lb: &mut Pnt2d,
    rt: &mut Pnt2d,
) -> Result<bool> {
    let t = tol();
    let (mut comp1, mut dcomp1) = (0.0, 0.0);
    let mut dis_null = false;
    let up1 = bs.last_u_knot_index();
    let down1 = bs.first_u_knot_index();
    let up2 = bs.last_v_knot_index();
    let down2 = bs.first_v_knot_index();
    let (mut up, mut down) = (0, 0);
    let mut cur = 0.0;
    let knot = |i: i32| -> Result<f64> { Ok(if index == 1 { bs.u_knot(i)? } else { bs.v_knot(i)? }) };
    if index == 1 {
        let mut i = down1;
        comp1 = uv.x();
        dcomp1 = duv.x();
        up = up1;
        down = down1;
        while (bs.u_knot(i)? - comp1).abs() > t && i != up1 {
            i += 1;
        }
        cur = bs.u_knot(i)?;
    } else if index == 2 {
        let mut i = down2;
        comp1 = uv.y();
        dcomp1 = duv.y();
        up = up2;
        down = down2;
        while (bs.v_knot(i)? - comp1).abs() > t && i != up2 {
            i += 1;
        }
        cur = bs.v_knot(i)?;
    }
    if (comp1 - cur).abs() <= t {
        let (mut bnd1, mut bnd2) = (down, up);
        if index == 1 {
            dis_null = find_bounds(bs.u_knots(), cur, duv.x(), &mut bnd1, &mut bnd2)?;
        } else if index == 2 {
            dis_null = find_bounds(bs.v_knots(), cur, duv.y(), &mut bnd1, &mut bnd2)?;
        }
        (bnd1, bnd2) = reverse_param_i(bnd1, bnd2);
        if !dis_null {
            set_xy(index, lb, rt, knot(bnd1)?, knot(bnd2)?);
        }
    } else {
        // the coordinate is not a knot
        if index == 1 && comp1 < bs.u_knot(down)? {
            lb.set_x(bs.u_knot(down)?);
            rt.set_x(bs.u_knot(down + 1)?);
            return Ok(dis_null);
        } else if index == 2 && comp1 < bs.v_knot(down)? {
            lb.set_y(bs.v_knot(down)?);
            rt.set_y(bs.v_knot(down + 1)?);
            return Ok(dis_null);
        } else if index == 1 && comp1 > bs.u_knot(up)? {
            rt.set_x(bs.u_knot(up - 1)?);
            lb.set_x(bs.u_knot(up)?);
            return Ok(dis_null);
        } else if index == 2 && comp1 > bs.v_knot(up)? {
            rt.set_y(bs.v_knot(up - 1)?);
            lb.set_y(bs.v_knot(up)?);
            return Ok(dis_null);
        }
        let (mut f, mut l) = (0.0, 1.0);
        if index == 1 {
            f = bs.u_knot(down)?;
            l = bs.u_knot(up)?;
        } else if index == 2 {
            f = bs.v_knot(down)?;
            l = bs.v_knot(up)?;
        }
        let mut i = down;
        if comp1.partial_cmp(&f) != Some(std::cmp::Ordering::Less)
            && comp1.partial_cmp(&l) != Some(std::cmp::Ordering::Greater)
        {
            if index == 1 {
                loop {
                    f = bs.u_knot(i)?;
                    let c1 = f >= comp1;
                    let c2 = if c1 {
                        true
                    } else {
                        l = bs.u_knot(i + 1)?;
                        l <= comp1
                    };
                    if !(c2 && i < up) {
                        break;
                    }
                    i += 1;
                }
            } else if index == 2 {
                loop {
                    f = bs.v_knot(i)?;
                    let c1 = f >= comp1;
                    let c2 = if c1 {
                        true
                    } else {
                        l = bs.v_knot(i + 1)?;
                        l <= comp1
                    };
                    if !(c2 && i < up) {
                        break;
                    }
                    i += 1;
                }
            }
        } else {
            (f, l) = reverse_param(f, l);
        }
        if i != up {
            if dcomp1.abs() > t {
                if dcomp1 > 0.0 {
                    set_xy(index, lb, rt, comp1, l);
                } else if dcomp1 < 0.0 {
                    set_xy(index, lb, rt, f, comp1);
                }
            } else if dcomp1.abs() < t {
                set_xy(index, lb, rt, f, l);
            }
        } else if i == up {
            set_xy(index, lb, rt, comp1, knot(i)?);
        }
    }
    Ok(dis_null)
}

// Locate2Coord on the interval [i1, i2]
fn locate_2coord(index: i32, uv: &Pnt2d, duv: &Vec2d, i1: f64, i2: f64, lb: &mut Pnt2d, rt: &mut Pnt2d) {
    let t = tol();
    let (mut comp1, mut dcomp1) = (0.0, 0.0);
    if index == 1 {
        comp1 = uv.x();
        dcomp1 = duv.x();
    } else if index == 2 {
        comp1 = uv.y();
        dcomp1 = duv.y();
    }
    if comp1 != i1 && comp1 != i2 {
        if dcomp1.abs() > t {
            if dcomp1 < 0.0 {
                set_xy(index, lb, rt, i1, comp1);
            } else if dcomp1 > 0.0 {
                set_xy(index, lb, rt, comp1, i2);
            } else {
                set_xy(index, lb, rt, i1, i2);
            }
        } else if dcomp1.abs() <= t {
            set_xy(index, lb, rt, i1, i2);
        }
    } else if (comp1 - i1).abs() < t || (comp1 - i2).abs() < t {
        set_xy(index, lb, rt, i1, i2);
    }
}

// Locate2Coord over the knots of a B-spline surface (`a` the knots of the direction `index`)
fn locate_2coord_surface(
    index: i32,
    uv: &Pnt2d,
    duv: &Vec2d,
    bs: &geom::BSplineSurface,
    a: &[f64],
    lb: &mut Pnt2d,
    rt: &mut Pnt2d,
) -> Result<()> {
    let t = tol();
    let (mut comp, mut dcomp) = (0.0, 0.0);
    let (mut nup, mut nlo) = (0, 0);
    if index == 1 {
        comp = uv.x();
        dcomp = duv.y();
        nup = bs.last_u_knot_index();
        nlo = bs.first_u_knot_index();
    }
    if index == 2 {
        comp = uv.y();
        dcomp = duv.x();
        nup = bs.last_v_knot_index();
        nlo = bs.first_v_knot_index();
    }
    let knot = |i: i32| -> Result<f64> { Ok(if index == 1 { bs.u_knot(i)? } else { bs.v_knot(i)? }) };
    let mut n = 0;
    if dcomp > 0.0 && dcomp.abs() > t {
        hunt(a, comp, &mut n)?;
        if n >= nup {
            // limit case: Hunt() caught the upper knot. Take the last span.
            n = nup - 1;
        }
        let (t1, t2) = reverse_param(knot(n)?, knot(n + 1)?);
        set_xy(index, lb, rt, t1, t2);
    } else if dcomp < 0.0 && dcomp.abs() > t {
        hunt(a, comp, &mut n)?;
        if n <= nlo {
            // limit case: Hunt() caught the lower knot. Take the first span.
            n = nlo + 1;
        }
        let (t1, t2) = reverse_param(knot(n - 1)?, knot(n)?);
        set_xy(index, lb, rt, t1, t2);
    }
    Ok(())
}

/// `Adaptor3d_CurveOnSurface`.
#[derive(Debug)]
pub struct CurveOnSurface {
    surface: Option<Arc<dyn SurfaceAdaptor>>,
    curve: Option<Arc<dyn Curve2dAdaptor>>,
    ty: GeomAbsCurveType,
    circ: Circ,
    lin: Lin,
    first_surf: Option<Arc<dyn SurfaceAdaptor>>,
    last_surf: Option<Arc<dyn SurfaceAdaptor>>,
    // `myIntCont` and `myIntervals` (mutable in C++)
    intervals: Mutex<(GeomAbsShape, Option<std::vec::Vec<f64>>)>,
}

impl Default for CurveOnSurface {
    fn default() -> Self {
        Self::new()
    }
}

impl CurveOnSurface {
    /// `Adaptor3d_CurveOnSurface()`.
    pub fn new() -> Self {
        Self {
            surface: None,
            curve: None,
            ty: GeomAbsCurveType::OtherCurve,
            circ: Circ::default(),
            lin: Lin::default(),
            first_surf: None,
            last_surf: None,
            intervals: Mutex::new((GeomAbsShape::Cn, None)),
        }
    }

    /// `Adaptor3d_CurveOnSurface(S)`.
    pub fn with_surface(s: Arc<dyn SurfaceAdaptor>) -> Self {
        let mut c = Self::new();
        c.surface = Some(s);
        c
    }

    /// `Adaptor3d_CurveOnSurface(C, S)`: `Load(S)` then `Load(C)`.
    pub fn with_curve_surface(c: Arc<dyn Curve2dAdaptor>, s: Arc<dyn SurfaceAdaptor>) -> Result<Self> {
        let mut r = Self::new();
        r.load_surface(s)?;
        r.load_curve(c)?;
        Ok(r)
    }

    /// `ShallowCopy` with the concrete type.
    pub fn shallow_copied(&self) -> Result<CurveOnSurface> {
        let mut c = Self::new();
        if let Some(s) = &self.surface {
            c.surface = Some(s.shallow_copy()?);
        }
        if let Some(cu) = &self.curve {
            c.curve = Some(cu.shallow_copy()?);
        }
        c.ty = self.ty;
        c.circ = self.circ;
        c.lin = self.lin;
        if let Some(s) = &self.first_surf {
            c.first_surf = Some(s.shallow_copy()?);
        }
        if let Some(s) = &self.last_surf {
            c.last_surf = Some(s.shallow_copy()?);
        }
        let g = self.intervals.lock().map_err(|_| AdaptorError::Domain)?;
        *c.intervals.lock().map_err(|_| AdaptorError::Domain)? = g.clone();
        drop(g);
        Ok(c)
    }

    /// `Trim` with the concrete type.
    pub fn trimmed(&self, first: f64, last: f64, tol: f64) -> Result<CurveOnSurface> {
        let mut c = Self::new();
        c.load_surface(self.surface.clone().ok_or(AdaptorError::NullObject)?)?;
        c.load_curve(self.cur()?.trim(first, last, tol)?)?;
        Ok(c)
    }

    /// `Load(S)`.
    pub fn load_surface(&mut self, s: Arc<dyn SurfaceAdaptor>) -> Result<()> {
        self.surface = Some(s);
        if self.curve.is_some() {
            self.eval_k_part()?;
        }
        Ok(())
    }

    /// `Load(C)`.
    pub fn load_curve(&mut self, c: Arc<dyn Curve2dAdaptor>) -> Result<()> {
        self.curve = Some(c);
        if self.surface.is_none() {
            return Ok(());
        }
        self.eval_k_part()?;
        let mut stype = self.surf()?.get_type()?;
        if stype == GeomAbsSurfaceType::OffsetSurface {
            stype = null(self.surf()?.basis_surface()?)?.get_type()?;
        }
        if stype == GeomAbsSurfaceType::BSplineSurface
            || stype == GeomAbsSurfaceType::SurfaceOfExtrusion
            || stype == GeomAbsSurfaceType::SurfaceOfRevolution
        {
            self.eval_first_last_surf()?;
        }
        Ok(())
    }

    /// `Load(C, S)`: `Load(C)` then `Load(S)`.
    pub fn load(&mut self, c: Arc<dyn Curve2dAdaptor>, s: Arc<dyn SurfaceAdaptor>) -> Result<()> {
        self.load_curve(c)?;
        self.load_surface(s)
    }

    /// `GetCurve`.
    pub fn get_curve(&self) -> Option<&Arc<dyn Curve2dAdaptor>> {
        self.curve.as_ref()
    }

    /// `GetSurface`.
    pub fn get_surface(&self) -> Option<&Arc<dyn SurfaceAdaptor>> {
        self.surface.as_ref()
    }

    /// `ChangeCurve`.
    pub fn change_curve(&mut self) -> &mut Option<Arc<dyn Curve2dAdaptor>> {
        &mut self.curve
    }

    /// `ChangeSurface`.
    pub fn change_surface(&mut self) -> &mut Option<Arc<dyn SurfaceAdaptor>> {
        &mut self.surface
    }

    fn cur(&self) -> Result<&Arc<dyn Curve2dAdaptor>> {
        self.curve.as_ref().ok_or(AdaptorError::NullObject)
    }
    fn surf(&self) -> Result<&Arc<dyn SurfaceAdaptor>> {
        self.surface.as_ref().ok_or(AdaptorError::NullObject)
    }

    // `EvalKPart`: the 3D line or circle that the curve on a plane, a sphere, a cylinder, a cone or a torus is, if it is one
    fn eval_k_part(&mut self) -> Result<()> {
        self.ty = GeomAbsCurveType::OtherCurve;
        let sty = self.surf()?.get_type()?;
        let cty = self.cur()?.get_type()?;
        if sty == GeomAbsSurfaceType::Plane {
            self.ty = cty;
            if self.ty == GeomAbsCurveType::Circle {
                let pl = self.surf()?.plane()?;
                self.circ = to3d_circ(&pl, &self.cur()?.circle()?)?;
            } else if self.ty == GeomAbsCurveType::Line {
                let (puv, duv) = self.cur()?.d1(0.0)?;
                let r = self.surf()?.d1(puv.x(), puv.y())?;
                let mut v = Vec::new(0.0, 0.0, 0.0);
                v.set_linear_form_2(duv.x(), &r.d1u, duv.y(), &r.d1v);
                self.lin = Lin::new(r.point, Dir::from_vec(&v)?);
            }
        } else if cty == GeomAbsCurveType::Line {
            let d = *self.cur()?.line()?.direction();
            if d.is_parallel(&gp::dx2d(), angular()) {
                // iso V
                let flip = |c: &mut Circ, opposite: bool| -> Result<()> {
                    if opposite {
                        let mut ax = *c.position();
                        ax.set_direction(ax.direction().reversed())?;
                        c.set_position(ax);
                    }
                    Ok(())
                };
                let opposite = d.is_opposite(&gp::dx2d(), angular());
                match sty {
                    GeomAbsSurfaceType::Sphere => {
                        let p = *self.cur()?.line()?.location();
                        if (p.y().abs() - M_PI / 2.0).abs() >= pconfusion() {
                            self.ty = GeomAbsCurveType::Circle;
                            let sph = self.surf()?.sphere()?;
                            let axis = *sph.position();
                            self.circ = elslib::sphere_viso(&axis, sph.radius(), p.y());
                            let drev = axis.x_direction().crossed(axis.y_direction())?;
                            let axe_rev = Ax1::new(*axis.location(), drev);
                            self.circ = self.circ.rotated(&axe_rev, p.x());
                            flip(&mut self.circ, opposite)?;
                        }
                    }
                    GeomAbsSurfaceType::Cylinder => {
                        self.ty = GeomAbsCurveType::Circle;
                        let cyl = self.surf()?.cylinder()?;
                        let p = *self.cur()?.line()?.location();
                        let axis = *cyl.position();
                        self.circ = elslib::cylinder_viso(&axis, cyl.radius(), p.y());
                        let drev = axis.x_direction().crossed(axis.y_direction())?;
                        let axe_rev = Ax1::new(*axis.location(), drev);
                        self.circ = self.circ.rotated(&axe_rev, p.x());
                        flip(&mut self.circ, opposite)?;
                    }
                    GeomAbsSurfaceType::Cone => {
                        self.ty = GeomAbsCurveType::Circle;
                        let cone = self.surf()?.cone()?;
                        let p = *self.cur()?.line()?.location();
                        let axis = *cone.position();
                        self.circ = elslib::cone_viso(&axis, cone.ref_radius(), cone.semi_angle(), p.y());
                        let drev = axis.x_direction().crossed(axis.y_direction())?;
                        let axe_rev = Ax1::new(*axis.location(), drev);
                        self.circ = self.circ.rotated(&axe_rev, p.x());
                        flip(&mut self.circ, opposite)?;
                    }
                    GeomAbsSurfaceType::Torus => {
                        self.ty = GeomAbsCurveType::Circle;
                        let tore = self.surf()?.torus()?;
                        let p = *self.cur()?.line()?.location();
                        let axis = *tore.position();
                        self.circ = elslib::torus_viso(&axis, tore.major_radius(), tore.minor_radius(), p.y());
                        let drev = axis.x_direction().crossed(axis.y_direction())?;
                        let axe_rev = Ax1::new(*axis.location(), drev);
                        self.circ = self.circ.rotated(&axe_rev, p.x());
                        flip(&mut self.circ, opposite)?;
                    }
                    _ => {}
                }
            } else if d.is_parallel(&gp::dy2d(), angular()) {
                // iso U
                let flip = |c: &mut Circ, opposite: bool| -> Result<()> {
                    if opposite {
                        let mut ax = *c.position();
                        ax.set_direction(ax.direction().reversed())?;
                        c.set_position(ax);
                    }
                    Ok(())
                };
                let opposite = d.is_opposite(&gp::dy2d(), angular());
                match sty {
                    GeomAbsSurfaceType::Sphere => {
                        self.ty = GeomAbsCurveType::Circle;
                        let sph = self.surf()?.sphere()?;
                        let p = *self.cur()?.line()?.location();
                        let axis = *sph.position();
                        // compute the iso 0
                        self.circ = elslib::sphere_uiso(&axis, sph.radius(), 0.0);
                        // same parametrization (circle rotation - Y offset)
                        let mut drev = axis.x_direction().crossed(axis.direction())?;
                        let mut axe_rev = Ax1::new(*axis.location(), drev);
                        self.circ = self.circ.rotated(&axe_rev, p.y());
                        // transform to iso U (= P.X())
                        drev = axis.x_direction().crossed(axis.y_direction())?;
                        axe_rev = Ax1::new(*axis.location(), drev);
                        self.circ = self.circ.rotated(&axe_rev, p.x());
                        flip(&mut self.circ, opposite)?;
                    }
                    GeomAbsSurfaceType::Cylinder => {
                        self.ty = GeomAbsCurveType::Line;
                        let cyl = self.surf()?.cylinder()?;
                        let p = *self.cur()?.line()?.location();
                        self.lin = elslib::cylinder_uiso(cyl.position(), cyl.radius(), p.x());
                        let mut tr = Vec::from_dir(self.lin.direction());
                        tr.multiply(p.y());
                        self.lin = self.lin.translated(&tr);
                        if opposite {
                            self.lin = self.lin.reversed();
                        }
                    }
                    GeomAbsSurfaceType::Cone => {
                        self.ty = GeomAbsCurveType::Line;
                        let cone = self.surf()?.cone()?;
                        let p = *self.cur()?.line()?.location();
                        self.lin = elslib::cone_uiso(cone.position(), cone.ref_radius(), cone.semi_angle(), p.x());
                        let mut tr = Vec::from_dir(self.lin.direction());
                        tr.multiply(p.y());
                        self.lin = self.lin.translated(&tr);
                        if opposite {
                            self.lin = self.lin.reversed();
                        }
                    }
                    GeomAbsSurfaceType::Torus => {
                        self.ty = GeomAbsCurveType::Circle;
                        let tore = self.surf()?.torus()?;
                        let p = *self.cur()?.line()?.location();
                        let axis = *tore.position();
                        self.circ = elslib::torus_uiso(&axis, tore.major_radius(), tore.minor_radius(), p.x());
                        let ax = self.circ.position().axis();
                        self.circ = self.circ.rotated(&ax, p.y());
                        flip(&mut self.circ, opposite)?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    // `EvalFirstLastSurf`: the surface trimmed to the span in which the curve starts / ends
    fn eval_first_last_surf(&mut self) -> Result<()> {
        let t = tol();
        let mut ok = true;
        let mut lb = Pnt2d::new(0.0, 0.0);
        let mut rt = Pnt2d::new(0.0, 0.0);
        let first_par = self.cur()?.first_parameter()?;
        let (uv, duv) = self.cur()?.d1(first_par)?;
        if duv.magnitude() <= t {
            ok = false;
        }
        if ok {
            ok = self.locate_by_type(&uv, &duv, &mut lb, &mut rt)?;
        }
        if ok {
            compare_bounds(&mut lb, &mut rt);
            let s = self.surf()?.u_trim(lb.x(), rt.x(), t)?;
            self.first_surf = Some(s.v_trim(lb.y(), rt.y(), t)?);
        } else {
            self.first_surf = self.surface.clone();
        }
        let last_par = self.cur()?.last_parameter()?;
        ok = true;
        let (uv, mut duv) = self.cur()?.d1(last_par)?;
        duv.reverse(); // we want the other part
        if duv.magnitude() <= t {
            ok = false;
        }
        if ok {
            ok = self.locate_by_type(&uv, &duv, &mut lb, &mut rt)?;
        }
        if ok {
            compare_bounds(&mut lb, &mut rt);
            let s = self.surf()?.u_trim(lb.x(), rt.x(), t)?;
            self.last_surf = Some(s.v_trim(lb.y(), rt.y(), t)?);
        } else {
            self.last_surf = self.surface.clone();
        }
        Ok(())
    }

    fn locate_by_type(&self, uv: &Pnt2d, duv: &Vec2d, lb: &mut Pnt2d, rt: &mut Pnt2d) -> Result<bool> {
        let s = self.surf()?;
        match s.get_type()? {
            GeomAbsSurfaceType::BSplineSurface => {
                locate_part(uv, duv, s, lb, rt)?;
                Ok(true)
            }
            GeomAbsSurfaceType::SurfaceOfRevolution | GeomAbsSurfaceType::SurfaceOfExtrusion => {
                locate_part_rev_ext(uv, duv, s, lb, rt)
            }
            GeomAbsSurfaceType::OffsetSurface => locate_part_offset(uv, duv, s, lb, rt),
            _ => not_impl(),
        }
    }
}

fn null<T>(h: Option<T>) -> Result<T> {
    h.ok_or(AdaptorError::NullObject)
}

fn locate_part_rev_ext(
    uv: &Pnt2d,
    duv: &Vec2d,
    s: &Arc<dyn SurfaceAdaptor>,
    lb: &mut Pnt2d,
    rt: &mut Pnt2d,
) -> Result<bool> {
    let ahc = null(s.basis_curve()?)?;
    if ahc.get_type()? == GeomAbsCurveType::BSplineCurve {
        let bspl = null(ahc.bspline()?)?;
        if s.get_type()? == GeomAbsSurfaceType::SurfaceOfExtrusion {
            locate_1coord_curve(1, uv, duv, &bspl, lb, rt)?;
            let l = s.last_v_parameter()?;
            let f = s.first_v_parameter()?;
            locate_2coord(2, uv, duv, f, l, lb, rt);
        } else if s.get_type()? == GeomAbsSurfaceType::SurfaceOfRevolution {
            locate_1coord_curve(2, uv, duv, &bspl, lb, rt)?;
            let l = s.last_u_parameter()?;
            let f = s.first_u_parameter()?;
            locate_2coord(1, uv, duv, f, l, lb, rt);
        }
        let (u1, u2) = reverse_param(lb.x(), rt.x());
        lb.set_x(u1);
        rt.set_x(u2);
        let (v1, v2) = reverse_param(lb.y(), rt.y());
        lb.set_y(v1);
        rt.set_y(v2);
        return Ok(true);
    }
    Ok(false)
}

fn locate_part_offset(
    uv: &Pnt2d,
    duv: &Vec2d,
    s: &Arc<dyn SurfaceAdaptor>,
    lb: &mut Pnt2d,
    rt: &mut Pnt2d,
) -> Result<bool> {
    let ahs = null(s.basis_surface()?)?;
    match ahs.get_type()? {
        GeomAbsSurfaceType::SurfaceOfRevolution | GeomAbsSurfaceType::SurfaceOfExtrusion => {
            locate_part_rev_ext(uv, duv, &ahs, lb, rt)
        }
        GeomAbsSurfaceType::BSplineSurface => {
            locate_part(uv, duv, &ahs, lb, rt)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn locate_part(uv: &Pnt2d, duv: &Vec2d, s: &Arc<dyn SurfaceAdaptor>, lb: &mut Pnt2d, rt: &mut Pnt2d) -> Result<()> {
    let bs = null(s.bspline()?)?;
    let du_is_null = locate_1coord_surface(1, uv, duv, &bs, lb, rt)?;
    let dv_is_null = locate_1coord_surface(2, uv, duv, &bs, lb, rt)?;
    if du_is_null && !dv_is_null {
        locate_2coord_surface(1, uv, duv, &bs, bs.u_knots(), lb, rt)?;
    } else if dv_is_null && !du_is_null {
        locate_2coord_surface(2, uv, duv, &bs, bs.v_knots(), lb, rt)?;
    }
    Ok(())
}

fn add_intervals(params: &mut std::vec::Vec<f64>, roots: &FunctionRoots, t: f64) -> Result<()> {
    if !roots.is_done() || roots.is_all_null().map_err(|_| AdaptorError::Domain)? {
        return Ok(());
    }
    let nsol = roots.nb_solutions().map_err(|_| AdaptorError::Domain)?;
    for i in 1..=nsol {
        let param = roots.value(i).map_err(|_| AdaptorError::Domain)?;
        let first = *params.first().ok_or(AdaptorError::OutOfRange)?;
        if param - first < t {
            // skip the parameter if equal to or less than the first one
            continue;
        }
        for j in 2..=params.len() {
            let delta = params.get(j - 1).copied().ok_or(AdaptorError::Domain)? - param;
            if delta > t {
                params.insert(j - 1, param);
                break;
            } else if delta >= -t {
                // the parameter equals the j-th within the tolerance
                break;
            }
        }
    }
    Ok(())
}

impl Curve3dAdaptor for CurveOnSurface {
    fn shallow_copy(&self) -> Result<Arc<dyn Curve3dAdaptor>> {
        Ok(Arc::new(self.shallow_copied()?))
    }

    fn first_parameter(&self) -> Result<f64> {
        self.cur()?.first_parameter()
    }

    fn last_parameter(&self) -> Result<f64> {
        self.cur()?.last_parameter()
    }

    fn continuity(&self) -> Result<GeomAbsShape> {
        let mut cont_c = self.cur()?.continuity()?;
        let cont_su = self.surf()?.u_continuity()?;
        if (cont_su as i32) < (cont_c as i32) {
            cont_c = cont_su;
        }
        let cont_sv = self.surf()?.v_continuity()?;
        if (cont_sv as i32) < (cont_c as i32) {
            cont_c = cont_sv;
        }
        Ok(cont_c)
    }

    fn nb_intervals(&self, s: GeomAbsShape) -> Result<i32> {
        {
            let g = self.intervals.lock().map_err(|_| AdaptorError::Domain)?;
            if let (true, Some(v)) = (s == g.0, &g.1) {
                return Ok(v.len() as i32 - 1);
            }
        }
        let surf = self.surf()?;
        let cur = self.cur()?;
        let nu = surf.nb_u_intervals(s)?;
        let nv = surf.nb_v_intervals(s)?;
        let nc = cur.nb_intervals(s)?;
        let mut tab_u = vec![0.0; (nu + 1).max(0) as usize];
        let mut tab_v = vec![0.0; (nv + 1).max(0) as usize];
        let mut tab_c = vec![0.0; (nc + 1).max(0) as usize];
        let nb_sample = 20;
        let tdeb = cur.first_parameter()?;
        let tfin = cur.last_parameter()?;
        cur.intervals(&mut tab_c, s)?;
        let t = pconfusion() / 10.0;
        // sorted sequence of parameters defining continuity intervals: the own intervals of the curve completed by the
        // additional points coming from the discontinuities of the surface
        let mut intervals: std::vec::Vec<f64> = tab_c.clone();
        if nu > 1 {
            surf.u_intervals(&mut tab_u, s)?;
            for iu in 2..=nu {
                let u = arr(&tab_u, iu)?;
                let mut func = InterFunc::new(cur.clone(), u, 1)?;
                let resol = FunctionRoots::new(&mut func, tdeb, tfin, nb_sample, t, t, t, 0.0);
                add_intervals(&mut intervals, &resol, t)?;
            }
        }
        if nv > 1 {
            surf.v_intervals(&mut tab_v, s)?;
            for iv in 2..=nv {
                let v = arr(&tab_v, iv)?;
                let mut func = InterFunc::new(cur.clone(), v, 2)?;
                let resol = FunctionRoots::new(&mut func, tdeb, tfin, nb_sample, t, t, t, 0.0);
                add_intervals(&mut intervals, &resol, t)?;
            }
        }
        // one interval whose first point equals its last point: the sequence has one value, add a second one
        if intervals.len() == 1 {
            let v0 = intervals.first().copied().ok_or(AdaptorError::Domain)?;
            intervals.push(v0);
        }
        let n = intervals.len() as i32 - 1;
        *self.intervals.lock().map_err(|_| AdaptorError::Domain)? = (s, Some(intervals));
        Ok(n)
    }

    fn intervals(&self, t: &mut [f64], s: GeomAbsShape) -> Result<()> {
        self.nb_intervals(s)?;
        let g = self.intervals.lock().map_err(|_| AdaptorError::Domain)?;
        let v = g.1.as_ref().ok_or(AdaptorError::NullObject)?;
        if t.len() != v.len() {
            // Standard_ASSERT_RAISE: wrong size of the array buffer
            return Err(AdaptorError::Domain);
        }
        for (i, x) in v.iter().enumerate() {
            super::put(t, i, *x)?;
        }
        Ok(())
    }

    fn trim(&self, first: f64, last: f64, tol: f64) -> Result<Arc<dyn Curve3dAdaptor>> {
        Ok(Arc::new(self.trimmed(first, last, tol)?))
    }

    fn is_closed(&self) -> Result<bool> {
        self.cur()?.is_closed()
    }

    fn is_periodic(&self) -> Result<bool> {
        if self.ty == GeomAbsCurveType::Circle || self.ty == GeomAbsCurveType::Ellipse {
            return Ok(true);
        }
        self.cur()?.is_periodic()
    }

    fn period(&self) -> Result<f64> {
        if self.ty == GeomAbsCurveType::Circle || self.ty == GeomAbsCurveType::Ellipse {
            return Ok(2.0 * M_PI);
        }
        self.cur()?.period()
    }

    fn eval_d0(&self, u: f64) -> Result<Pnt> {
        if self.ty == GeomAbsCurveType::Line {
            Ok(elclib::value_lin(u, &self.lin))
        } else if self.ty == GeomAbsCurveType::Circle {
            Ok(elclib::value_circ(u, &self.circ))
        } else {
            let puv = self.cur()?.d0(u)?;
            self.surf()?.d0(puv.x(), puv.y())
        }
    }

    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let fp = self.cur()?.first_parameter()?;
        let lp = self.cur()?.last_parameter()?;
        let t = tol();
        let from_surface = |surf: &Arc<dyn SurfaceAdaptor>| -> Result<ResD1> {
            let (puv, duv) = self.cur()?.d1(u)?;
            let r = surf.d1(puv.x(), puv.y())?;
            let mut d1 = Vec::new(0.0, 0.0, 0.0);
            d1.set_linear_form_2(duv.x(), &r.d1u, duv.y(), &r.d1v);
            Ok(ResD1 { point: r.point, d1 })
        };
        if (u - fp).abs() < t && self.first_surf.is_some() {
            from_surface(self.first_surf.as_ref().ok_or(AdaptorError::NullObject)?)
        } else if (u - lp).abs() < t && self.last_surf.is_some() {
            from_surface(self.last_surf.as_ref().ok_or(AdaptorError::NullObject)?)
        } else if self.ty == GeomAbsCurveType::Line {
            let (point, d1) = elclib::d1_lin(u, &self.lin);
            Ok(ResD1 { point, d1 })
        } else if self.ty == GeomAbsCurveType::Circle {
            let (point, d1) = elclib::d1_circ(u, &self.circ);
            Ok(ResD1 { point, d1 })
        } else {
            from_surface(self.surf()?)
        }
    }

    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let fp = self.cur()?.first_parameter()?;
        let lp = self.cur()?.last_parameter()?;
        let t = tol();
        let from_surface = |surf: &Arc<dyn SurfaceAdaptor>| -> Result<ResD2> {
            let (uv, dw, d2w) = self.cur()?.d2(u)?;
            let _ = uv;
            let r = surf.d2(uv.x(), uv.y())?;
            let mut d1 = Vec::new(0.0, 0.0, 0.0);
            d1.set_linear_form_2(dw.x(), &r.d1u, dw.y(), &r.d1v);
            let mut d2 = Vec::new(0.0, 0.0, 0.0);
            d2.set_linear_form_3(d2w.x(), &r.d1u, d2w.y(), &r.d1v, 2.0 * dw.x() * dw.y(), &r.d2uv);
            let old = d2;
            d2.set_linear_form_2_plus(dw.x() * dw.x(), &r.d2u, dw.y() * dw.y(), &r.d2v, &old);
            Ok(ResD2 { point: r.point, d1, d2 })
        };
        if (u - fp).abs() < t && self.first_surf.is_some() {
            from_surface(self.first_surf.as_ref().ok_or(AdaptorError::NullObject)?)
        } else if (u - lp).abs() < t && self.last_surf.is_some() {
            from_surface(self.last_surf.as_ref().ok_or(AdaptorError::NullObject)?)
        } else if self.ty == GeomAbsCurveType::Line {
            let (point, d1) = elclib::d1_lin(u, &self.lin);
            Ok(ResD2 { point, d1, d2: Vec::new(0.0, 0.0, 0.0) })
        } else if self.ty == GeomAbsCurveType::Circle {
            let (point, d1, d2) = elclib::d2_circ(u, &self.circ);
            Ok(ResD2 { point, d1, d2 })
        } else {
            from_surface(self.surf()?)
        }
    }

    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let fp = self.cur()?.first_parameter()?;
        let lp = self.cur()?.last_parameter()?;
        let t = tol();
        let from_surface = |surf: &Arc<dyn SurfaceAdaptor>| -> Result<ResD3> {
            let (uv, dw, d2w, d3w) = self.cur()?.d3(u)?;
            let r = surf.d3(uv.x(), uv.y())?;
            let mut d1 = Vec::new(0.0, 0.0, 0.0);
            d1.set_linear_form_2(dw.x(), &r.d1u, dw.y(), &r.d1v);
            let mut d2 = Vec::new(0.0, 0.0, 0.0);
            d2.set_linear_form_3(d2w.x(), &r.d1u, d2w.y(), &r.d1v, 2.0 * dw.x() * dw.y(), &r.d2uv);
            let old = d2;
            d2.set_linear_form_2_plus(dw.x() * dw.x(), &r.d2u, dw.y() * dw.y(), &r.d2v, &old);
            let d3 = set_linear_form(
                &dw, &d2w, &r.d1u, &r.d1v, &r.d2u, &r.d2v, &r.d2uv, &r.d3u, &r.d3v, &r.d3uuv, &r.d3uvv, &d3w,
            );
            Ok(ResD3 { point: r.point, d1, d2, d3 })
        };
        if (u - fp).abs() < t && self.first_surf.is_some() {
            from_surface(self.first_surf.as_ref().ok_or(AdaptorError::NullObject)?)
        } else if (u - lp).abs() < t && self.last_surf.is_some() {
            from_surface(self.last_surf.as_ref().ok_or(AdaptorError::NullObject)?)
        } else if self.ty == GeomAbsCurveType::Line {
            let (point, d1) = elclib::d1_lin(u, &self.lin);
            Ok(ResD3 { point, d1, d2: Vec::new(0.0, 0.0, 0.0), d3: Vec::new(0.0, 0.0, 0.0) })
        } else if self.ty == GeomAbsCurveType::Circle {
            let (point, d1, d2, d3) = elclib::d3_circ(u, &self.circ);
            Ok(ResD3 { point, d1, d2, d3 })
        } else {
            from_surface(self.surf()?)
        }
    }

    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec> {
        match n {
            1 => Ok(self.eval_d1(u)?.d1),
            2 => Ok(self.eval_d2(u)?.d2),
            3 => Ok(self.eval_d3(u)?.d3),
            _ => not_impl(),
        }
    }

    fn resolution(&self, r3d: f64) -> Result<f64> {
        let ru = self.surf()?.u_resolution(r3d)?;
        let rv = self.surf()?.v_resolution(r3d)?;
        self.cur()?.resolution(if rv < ru { rv } else { ru })
    }

    fn get_type(&self) -> Result<GeomAbsCurveType> {
        Ok(self.ty)
    }

    fn line(&self) -> Result<Lin> {
        Ok(self.lin)
    }

    fn circle(&self) -> Result<Circ> {
        Ok(self.circ)
    }

    fn ellipse(&self) -> Result<Elips> {
        let pl = self.surf()?.plane()?;
        to3d_elips(&pl, &self.cur()?.ellipse()?)
    }

    fn hyperbola(&self) -> Result<Hypr> {
        let pl = self.surf()?.plane()?;
        to3d_hypr(&pl, &self.cur()?.hyperbola()?)
    }

    fn parabola(&self) -> Result<Parab> {
        let pl = self.surf()?.plane()?;
        to3d_parab(&pl, &self.cur()?.parabola()?)
    }

    fn degree(&self) -> Result<i32> {
        self.cur()?.degree()
    }

    fn is_rational(&self) -> Result<bool> {
        Ok(self.cur()?.is_rational()? || self.surf()?.is_u_rational()? || self.surf()?.is_v_rational()?)
    }

    fn nb_poles(&self) -> Result<i32> {
        self.cur()?.nb_poles()
    }

    fn nb_knots(&self) -> Result<i32> {
        if self.surf()?.get_type()? == GeomAbsSurfaceType::Plane {
            self.cur()?.nb_knots()
        } else {
            Err(AdaptorError::NoSuchObject)
        }
    }

    fn bezier(&self) -> Result<Option<Arc<geom::BezierCurve>>> {
        // (the `Raise_if` check that the surface is a plane is not made by the library as built)
        let bez2d = null(self.cur()?.bezier()?)?;
        let pl = self.surf()?.plane()?;
        let poles: std::vec::Vec<Pnt> = bez2d.poles().iter().map(|p| to3d_pnt(&pl, p)).collect();
        Ok(Some(Arc::new(match bez2d.weights() {
            Some(w) if bez2d.is_rational() => geom::BezierCurve::with_weights(&poles, w)?,
            _ => geom::BezierCurve::new(&poles)?,
        })))
    }

    fn bspline(&self) -> Result<Option<Arc<geom::BSplineCurve>>> {
        let bsp2d = null(self.cur()?.bspline()?)?;
        let pl = self.surf()?.plane()?;
        let poles: std::vec::Vec<Pnt> = bsp2d.poles().iter().map(|p| to3d_pnt(&pl, p)).collect();
        Ok(Some(Arc::new(match bsp2d.weights() {
            Some(w) if bsp2d.is_rational() => geom::BSplineCurve::with_weights(
                &poles,
                w,
                bsp2d.knots(),
                bsp2d.multiplicities(),
                bsp2d.degree(),
                bsp2d.is_periodic(),
            )?,
            _ => geom::BSplineCurve::new(
                &poles,
                bsp2d.knots(),
                bsp2d.multiplicities(),
                bsp2d.degree(),
                bsp2d.is_periodic(),
            )?,
        })))
    }
}
