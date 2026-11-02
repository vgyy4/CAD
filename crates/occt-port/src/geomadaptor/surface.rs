//! `GeomAdaptor_Surface`: the adaptor over a `Geom_Surface` (elementary surfaces evaluated by `ElSLib`, Bezier and B-spline
//! surfaces by a `BSplSLib_Cache`, extrusion, revolution and offset surfaces through the adaptors of their basis).
//!
//! Translated from `GeomAdaptor_Surface.hxx/.cxx` (OCCT 8.0.1). The C++ `std::variant` of surface-specific data is the private enum
//! `SurfaceData`; the mutable `BSplSLib_Cache` handles are `Mutex<Option<Cache>>`.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::too_many_arguments)]
#![allow(clippy::float_cmp)]
use super::Result;
use super::curve::Curve;
use super::utils;
use crate::adaptor2d::{AdaptorError, GeomAbsShape};
use crate::adaptor3d::{Curve3dAdaptor, GeomAbsSurfaceType, SurfaceAdaptor};
use crate::bsplclib::flat::locate_core;
use crate::bsplclib_cache::flat_bezier_knots;
use crate::bsplslib::Cache;
use crate::elslib;
use crate::geom::offset_surface_utils::{OffsetBasis, evaluate_d0, evaluate_d1, evaluate_d2, evaluate_d3, evaluate_dn};
use crate::geom::{
    self, BSplineSurface, BezierSurface, Circle, ConicalSurface, CylindricalSurface, GeomError, OffsetSurface, Plane,
    RectangularTrimmedSurface, SphericalSurface, SurfResD1, SurfResD2, SurfResD3, Surface as GeomSurface,
    SurfaceOfLinearExtrusion, SurfaceOfRevolution, SweptSurface, ToroidalSurface, confusion, pconfusion,
};
use crate::gp::{Ax1, Cone, Cylinder, Dir, Pln, Pnt, Sphere, Torus, Vec, XYZ};
use std::sync::{Arc, Mutex};

fn pos_tol() -> f64 {
    pconfusion() * 0.5
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn bspl(e: crate::bsplclib::BSplError) -> AdaptorError {
    AdaptorError::Geom3d(GeomError::BSpl(e))
}

fn rd1<T: Copy>(a: &[T], i: i32) -> Result<T> {
    usize::try_from(i - 1).ok().and_then(|k| a.get(k)).copied().ok_or(AdaptorError::OutOfRange)
}

/// The Rust errors of the adaptors seen by the `Geom_OffsetSurfaceUtils` functions (which return `GeomError`).
fn to_geom_error(e: AdaptorError) -> GeomError {
    match e {
        AdaptorError::Geom3d(g) => g,
        AdaptorError::UndefinedValue => GeomError::UndefinedValue,
        AdaptorError::UndefinedDerivative => GeomError::UndefinedDerivative,
        AdaptorError::OutOfRange => GeomError::OutOfRange,
        AdaptorError::NoSuchObject => GeomError::NoSuchObject,
        AdaptorError::Domain => GeomError::Domain,
        _ => GeomError::Argument,
    }
}

/// Extrusion data (`ExtrusionData`).
#[derive(Debug, Clone)]
pub(super) struct ExtrusionData {
    pub(super) basis: Arc<dyn Curve3dAdaptor>,
    pub(super) direction: XYZ,
    pub(super) has_eval_rep: bool,
}

/// Revolution data (`RevolutionData`).
#[derive(Debug, Clone)]
pub(super) struct RevolutionData {
    pub(super) basis: Arc<dyn Curve3dAdaptor>,
    pub(super) axis: Ax1,
    pub(super) has_eval_rep: bool,
}

/// Offset data (`OffsetData`).
#[derive(Debug, Clone)]
pub(super) struct OffsetData {
    basis: Arc<Surface>,
    equivalent: Option<Arc<Surface>>,
    offset_surface: Arc<OffsetSurface>,
    offset: f64,
    has_eval_rep: bool,
}

/// Bezier surface data (`BezierData`).
#[derive(Debug)]
pub(super) struct BezierData {
    surface: Arc<BezierSurface>,
    cache: Mutex<Option<Cache>>,
    has_eval_rep: bool,
}

/// B-spline surface data (`BSplineData`).
#[derive(Debug)]
pub(super) struct BSplineData {
    surface: Arc<BSplineSurface>,
    cache: Mutex<Option<Cache>>,
    has_eval_rep: bool,
}

/// The surface-specific data (`SurfaceDataVariant`).
#[derive(Debug)]
pub(super) enum SurfaceData {
    None,
    Pln(Pln),
    Cylinder(Cylinder),
    Cone(Cone),
    Sphere(Sphere),
    Torus(Torus),
    Extrusion(ExtrusionData),
    Revolution(RevolutionData),
    Offset(OffsetData),
    Bezier(BezierData),
    BSpline(BSplineData),
}

/// `GeomAdaptor_Surface`.
#[derive(Debug)]
pub struct Surface {
    pub(super) surface: Option<Arc<dyn GeomSurface>>,
    pub(super) u_first: f64,
    pub(super) u_last: f64,
    pub(super) v_first: f64,
    pub(super) v_last: f64,
    pub(super) tol_u: f64,
    pub(super) tol_v: f64,
    pub(super) surface_type: GeomAbsSurfaceType,
    pub(super) data: SurfaceData,
}

impl Default for Surface {
    fn default() -> Self {
        Self::new()
    }
}

/// The knot span information of `IfUVBound`.
struct Spans {
    local: bool,
    u_deb: i32,
    u_fin: i32,
    v_deb: i32,
    v_fin: i32,
}

impl Surface {
    /// `GeomAdaptor_Surface()`: no surface, type `OtherSurface`, ranges and tolerances 0.
    pub fn new() -> Self {
        Self {
            surface: None,
            u_first: 0.0,
            u_last: 0.0,
            v_first: 0.0,
            v_last: 0.0,
            tol_u: 0.0,
            tol_v: 0.0,
            surface_type: GeomAbsSurfaceType::OtherSurface,
            data: SurfaceData::None,
        }
    }

    /// `GeomAdaptor_Surface(S)`: `Load(S)`.
    pub fn with_surface(s: Arc<dyn GeomSurface>) -> Result<Self> {
        let mut a = Self::new();
        a.load(s)?;
        Ok(a)
    }

    /// `GeomAdaptor_Surface(S, UFirst, ULast, VFirst, VLast, TolU, TolV)`.
    pub fn with_range(
        s: Arc<dyn GeomSurface>,
        u1: f64,
        u2: f64,
        v1: f64,
        v2: f64,
        tol_u: f64,
        tol_v: f64,
    ) -> Result<Self> {
        let mut a = Self::new();
        a.load_range(s, u1, u2, v1, v2, tol_u, tol_v)?;
        Ok(a)
    }

    /// `GeomAdaptor_Surface(S)` with a possibly null handle: a null handle is `Err(NullObject)`.
    pub fn with_surface_opt(s: Option<Arc<dyn GeomSurface>>) -> Result<Self> {
        let mut a = Self::new();
        a.load_opt(s)?;
        Ok(a)
    }

    /// `Load(S)` with a possibly null handle.
    pub fn load_opt(&mut self, s: Option<Arc<dyn GeomSurface>>) -> Result<()> {
        self.load(s.ok_or(AdaptorError::NullObject)?)
    }

    /// `Load(S, UFirst, ULast, VFirst, VLast, TolU, TolV)` with a possibly null handle.
    pub fn load_range_opt(
        &mut self,
        s: Option<Arc<dyn GeomSurface>>,
        u1: f64,
        u2: f64,
        v1: f64,
        v2: f64,
        tol_u: f64,
        tol_v: f64,
    ) -> Result<()> {
        self.load_range(s.ok_or(AdaptorError::NullObject)?, u1, u2, v1, v2, tol_u, tol_v)
    }

    /// `Load(S)`: the whole parameter range of the surface.
    pub fn load(&mut self, s: Arc<dyn GeomSurface>) -> Result<()> {
        let (u1, u2, v1, v2) = s.bounds();
        self.load_impl(s, u1, u2, v1, v2, 0.0, 0.0)
    }

    /// `Load(S, UFirst, ULast, VFirst, VLast, TolU, TolV)`: `ConstructionError` if a range is reversed.
    pub fn load_range(
        &mut self,
        s: Arc<dyn GeomSurface>,
        u1: f64,
        u2: f64,
        v1: f64,
        v2: f64,
        tol_u: f64,
        tol_v: f64,
    ) -> Result<()> {
        if u1 > u2 || v1 > v2 {
            return Err(AdaptorError::Construction);
        }
        self.load_impl(s, u1, u2, v1, v2, tol_u, tol_v)
    }

    /// `Surface`.
    pub fn surface(&self) -> Option<&Arc<dyn GeomSurface>> {
        self.surface.as_ref()
    }

    /// `ToleranceU`.
    pub fn tolerance_u(&self) -> f64 {
        self.tol_u
    }

    /// `ToleranceV`.
    pub fn tolerance_v(&self) -> f64 {
        self.tol_v
    }

    /// `Bounds`: (`FirstUParameter`, `LastUParameter`, `FirstVParameter`, `LastVParameter`).
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        (self.u_first, self.u_last, self.v_first, self.v_last)
    }

    fn surf(&self) -> Result<&Arc<dyn GeomSurface>> {
        self.surface.as_ref().ok_or(AdaptorError::NullObject)
    }

    // GeomAdaptor_Surface::load
    fn load_impl(
        &mut self,
        s: Arc<dyn GeomSurface>,
        u1: f64,
        u2: f64,
        v1: f64,
        v2: f64,
        tol_u: f64,
        tol_v: f64,
    ) -> Result<()> {
        self.tol_u = tol_u;
        self.tol_v = tol_v;
        self.u_first = u1;
        self.u_last = u2;
        self.v_first = v1;
        self.v_last = v2;
        let same = self
            .surface
            .as_ref()
            .is_some_and(|m| std::ptr::eq(Arc::as_ptr(m).cast::<()>(), Arc::as_ptr(&s).cast::<()>()));
        if same {
            return Ok(());
        }
        self.surface = Some(s.clone());
        self.data = SurfaceData::None;
        let any = s.as_any();
        if let Some(t) = any.downcast_ref::<RectangularTrimmedSurface>() {
            return self.load_range(t.basis_surface(), u1, u2, v1, v2, 0.0, 0.0);
        } else if let Some(x) = any.downcast_ref::<Plane>() {
            self.surface_type = GeomAbsSurfaceType::Plane;
            self.data = SurfaceData::Pln(x.pln());
        } else if let Some(x) = any.downcast_ref::<CylindricalSurface>() {
            self.surface_type = GeomAbsSurfaceType::Cylinder;
            self.data = SurfaceData::Cylinder(x.cylinder());
        } else if let Some(x) = any.downcast_ref::<ConicalSurface>() {
            self.surface_type = GeomAbsSurfaceType::Cone;
            self.data = SurfaceData::Cone(x.cone());
        } else if let Some(x) = any.downcast_ref::<SphericalSurface>() {
            self.surface_type = GeomAbsSurfaceType::Sphere;
            self.data = SurfaceData::Sphere(x.sphere());
        } else if let Some(x) = any.downcast_ref::<ToroidalSurface>() {
            self.surface_type = GeomAbsSurfaceType::Torus;
            self.data = SurfaceData::Torus(x.torus());
        } else if let Some(x) = any.downcast_ref::<SurfaceOfRevolution>() {
            self.surface_type = GeomAbsSurfaceType::SurfaceOfRevolution;
            let basis = Curve::with_range(x.basis_curve(), self.v_first, self.v_last)?;
            self.data = SurfaceData::Revolution(RevolutionData {
                basis: Arc::new(basis),
                axis: x.axis(),
                has_eval_rep: x.eval_representation().is_some(),
            });
        } else if let Some(x) = any.downcast_ref::<SurfaceOfLinearExtrusion>() {
            self.surface_type = GeomAbsSurfaceType::SurfaceOfExtrusion;
            let basis = Curve::with_range(x.basis_curve(), self.u_first, self.u_last)?;
            self.data = SurfaceData::Extrusion(ExtrusionData {
                basis: Arc::new(basis),
                direction: *x.direction().xyz(),
                has_eval_rep: x.eval_representation().is_some(),
            });
        } else if let Some(x) = any.downcast_ref::<BezierSurface>() {
            self.surface_type = GeomAbsSurfaceType::BezierSurface;
            self.data = SurfaceData::Bezier(BezierData {
                has_eval_rep: x.eval_representation().is_some(),
                surface: Arc::new(x.clone()),
                cache: Mutex::new(None),
            });
        } else if let Some(x) = any.downcast_ref::<BSplineSurface>() {
            self.surface_type = GeomAbsSurfaceType::BSplineSurface;
            self.data = SurfaceData::BSpline(BSplineData {
                has_eval_rep: x.eval_representation().is_some(),
                surface: Arc::new(x.clone()),
                cache: Mutex::new(None),
            });
        } else if let Some(x) = any.downcast_ref::<OffsetSurface>() {
            self.surface_type = GeomAbsSurfaceType::OffsetSurface;
            // reuse the original surface for osculating queries
            let basis = Surface::with_range(
                x.basis_surface(),
                self.u_first,
                self.u_last,
                self.v_first,
                self.v_last,
                self.tol_u,
                self.tol_v,
            )?;
            // an equivalent canonical surface evaluates faster
            let equivalent = match x.surface() {
                Some(e) => Some(Arc::new(Surface::with_range(
                    e,
                    self.u_first,
                    self.u_last,
                    self.v_first,
                    self.v_last,
                    self.tol_u,
                    self.tol_v,
                )?)),
                None => None,
            };
            self.data = SurfaceData::Offset(OffsetData {
                basis: Arc::new(basis),
                equivalent,
                offset_surface: Arc::new(x.clone()),
                offset: x.offset(),
                has_eval_rep: x.eval_representation().is_some(),
            });
        } else {
            self.surface_type = GeomAbsSurfaceType::OtherSurface;
        }
        Ok(())
    }

    fn has_eval_rep(&self) -> bool {
        match &self.data {
            SurfaceData::Bezier(d) => d.has_eval_rep,
            SurfaceData::BSpline(d) => d.has_eval_rep,
            SurfaceData::Extrusion(d) => d.has_eval_rep,
            SurfaceData::Revolution(d) => d.has_eval_rep,
            SurfaceData::Offset(d) => d.has_eval_rep,
            _ => false,
        }
    }

    fn bspline_data(&self) -> Result<&BSplineData> {
        match &self.data {
            SurfaceData::BSpline(d) => Ok(d),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn offset_data(&self) -> Result<&OffsetData> {
        match &self.data {
            SurfaceData::Offset(d) => Ok(d),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }

    /// The `USide`, `VSide` and snapped parameters of `EvalD1 .. EvalDN`.
    fn snap(&self, u: f64, v: f64) -> (f64, f64, i32, i32) {
        let (mut uu, mut vv, mut us, mut vs) = (u, v, 0, 0);
        if (u - self.u_first).abs() <= self.tol_u {
            us = 1;
            uu = self.u_first;
        } else if (u - self.u_last).abs() <= self.tol_u {
            us = -1;
            uu = self.u_last;
        }
        if (v - self.v_first).abs() <= self.tol_v {
            vs = 1;
            vv = self.v_first;
        } else if (v - self.v_last).abs() <= self.tol_v {
            vs = -1;
            vv = self.v_last;
        }
        (uu, vv, us, vs)
    }

    // GeomAdaptor_Surface::LocalContinuity (the function of the unnamed namespace)
    fn local_continuity(
        degree: i32,
        nb: i32,
        tk: &[f64],
        tm: &[i32],
        pfirst: f64,
        plast: f64,
        periodic: bool,
    ) -> Result<GeomAbsShape> {
        if tk.len() != usize::try_from(nb).map_err(|_| AdaptorError::OutOfRange)? || tm.len() != tk.len() {
            return Err(AdaptorError::Domain);
        }
        let (uf, ul) =
            if periodic { (tk.first().copied().unwrap_or(0.0), tk.last().copied().unwrap_or(1.0)) } else { (0.0, 1.0) };
        let (mut index1, new_first) = locate_core(tk, pfirst, periodic, 1, nb, uf, ul).map_err(bspl)?;
        let (mut index2, new_last) = locate_core(tk, plast, periodic, 1, nb, uf, ul).map_err(bspl)?;
        let eps_knot = pconfusion();
        if (new_first - rd1(tk, index1 + 1)?).abs() < eps_knot {
            index1 += 1;
        }
        if (new_last - rd1(tk, index2)?).abs() < eps_knot {
            index2 -= 1;
        }
        // handle periodic curves
        if periodic && index1 == nb {
            index1 = 1;
        }
        if index2 != index1 {
            let mut multmax = rd1(tm, index1 + 1)?;
            for i in (index1 + 1)..=index2 {
                if rd1(tm, i)? > multmax {
                    multmax = rd1(tm, i)?;
                }
            }
            multmax = degree - multmax;
            if multmax <= 0 {
                return Ok(GeomAbsShape::C0);
            }
            match multmax {
                1 => return Ok(GeomAbsShape::C1),
                2 => return Ok(GeomAbsShape::C2),
                3 => return Ok(GeomAbsShape::C3),
                _ => {}
            }
        }
        Ok(GeomAbsShape::Cn)
    }

    // the continuity of the basis surface of an offset surface, as seen by UContinuity / VContinuity
    fn offset_continuity(basis: GeomAbsShape) -> Result<GeomAbsShape> {
        match basis {
            GeomAbsShape::Cn | GeomAbsShape::C3 => Ok(GeomAbsShape::Cn),
            GeomAbsShape::G2 | GeomAbsShape::C2 => Ok(GeomAbsShape::C1),
            GeomAbsShape::G1 | GeomAbsShape::C1 | GeomAbsShape::C0 => Ok(GeomAbsShape::C0),
        }
    }

    fn offset_base_shape(s: GeomAbsShape) -> Result<GeomAbsShape> {
        match s {
            GeomAbsShape::G1 | GeomAbsShape::G2 => Err(AdaptorError::Domain),
            GeomAbsShape::C0 => Ok(GeomAbsShape::C1),
            GeomAbsShape::C1 => Ok(GeomAbsShape::C2),
            GeomAbsShape::C2 => Ok(GeomAbsShape::C3),
            GeomAbsShape::C3 | GeomAbsShape::Cn => Ok(GeomAbsShape::Cn),
        }
    }

    // GeomAdaptor_Surface::RebuildCache
    fn rebuild_cache(&self, u: f64, v: f64) -> Result<()> {
        match &self.data {
            SurfaceData::Bezier(d) => {
                let b = &d.surface;
                let (deg_u, deg_v) = (b.u_degree(), b.v_degree());
                let flat_u = flat_bezier_knots(deg_u).map_err(bspl)?;
                let flat_v = flat_bezier_knots(deg_v).map_err(bspl)?;
                let poles = geom::to_array2(b.poles())?;
                let weights = b.weights().map(geom::to_array2).transpose()?;
                let mut g = lock(&d.cache);
                if g.is_none() {
                    *g = Some(
                        Cache::new(
                            deg_u,
                            b.is_u_periodic(),
                            &flat_u,
                            deg_v,
                            b.is_v_periodic(),
                            &flat_v,
                            weights.as_ref(),
                        )
                        .map_err(bspl)?,
                    );
                }
                if let Some(c) = g.as_mut() {
                    c.build_cache(u, v, &flat_u, &flat_v, &poles, weights.as_ref()).map_err(bspl)?;
                }
            }
            SurfaceData::BSpline(d) => {
                let b = &d.surface;
                let poles = geom::to_array2(b.poles())?;
                let weights = b.weights().map(geom::to_array2).transpose()?;
                let mut g = lock(&d.cache);
                if g.is_none() {
                    *g = Some(
                        Cache::new(
                            b.u_degree(),
                            b.is_u_periodic(),
                            b.u_knot_sequence(),
                            b.v_degree(),
                            b.is_v_periodic(),
                            b.v_knot_sequence(),
                            weights.as_ref(),
                        )
                        .map_err(bspl)?,
                    );
                }
                if let Some(c) = g.as_mut() {
                    c.build_cache(u, v, b.u_knot_sequence(), b.v_knot_sequence(), &poles, weights.as_ref())
                        .map_err(bspl)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Runs `f` on the cache of a Bezier / B-spline surface, rebuilding it when it is missing or (B-spline) not valid at `(u, v)`.
    fn with_cache<T>(
        &self,
        u: f64,
        v: f64,
        f: impl FnOnce(&Cache) -> std::result::Result<T, crate::bsplclib::BSplError>,
    ) -> Result<T> {
        let (cache, check_valid) = match &self.data {
            SurfaceData::Bezier(d) => (&d.cache, false),
            SurfaceData::BSpline(d) => (&d.cache, true),
            _ => return Err(AdaptorError::NoSuchObject),
        };
        let invalid = {
            let g = lock(cache);
            match g.as_ref() {
                None => true,
                Some(c) => check_valid && !c.is_cache_valid(u, v),
            }
        };
        if invalid {
            self.rebuild_cache(u, v)?;
        }
        let g = lock(cache);
        let c = g.as_ref().ok_or(AdaptorError::NoSuchObject)?;
        f(c).map_err(bspl)
    }

    // GeomAdaptor_Surface::Span
    #[allow(clippy::too_many_arguments)]
    fn span(side: i32, ideb: i32, ifin: i32, fk: i32, lk: i32) -> (i32, i32) {
        if ideb != ifin {
            // not a knot
            if ideb < fk {
                (fk, fk + 1)
            } else if ifin > lk || ideb >= lk - 1 {
                (lk - 1, lk)
            } else if ifin <= fk + 1 {
                (fk, fk + 1)
            } else if ideb > ifin {
                (ifin - 1, ifin)
            } else {
                (ideb, ifin)
            }
        } else if ideb <= fk {
            // first knot
            (fk, fk + 1)
        } else if ifin >= lk {
            // last knot
            (lk - 1, lk)
        } else if side == -1 {
            (ideb - 1, ifin)
        } else {
            (ideb, ifin + 1)
        }
    }

    // GeomAdaptor_Surface::IfUVBound
    fn if_uv_bound(&self, u: f64, v: f64, u_side: i32, v_side: i32) -> Result<Spans> {
        let b = &self.bspline_data()?.surface;
        let (anufk, anulk, avfk, avlk) =
            (b.first_u_knot_index(), b.last_u_knot_index(), b.first_v_knot_index(), b.last_v_knot_index());
        let (i0, i1) = b.locate_u(u, pos_tol(), false);
        let mut local = i0 == i1;
        let (ideb, ifin) = Self::span(u_side, i0, i1, anufk, anulk);
        let (j0, j1) = b.locate_v(v, pos_tol(), false);
        if j0 == j1 {
            local = true;
        }
        let (ivdeb, ivfin) = Self::span(v_side, j0, j1, avfk, avlk);
        Ok(Spans { local, u_deb: ideb, u_fin: ifin, v_deb: ivdeb, v_fin: ivfin })
    }

    /// `ShallowCopy` with the concrete type.
    pub fn shallow_copied(&self) -> Result<Surface> {
        let mut copy = Surface::new();
        copy.surface = self.surface.clone();
        copy.u_first = self.u_first;
        copy.u_last = self.u_last;
        copy.v_first = self.v_first;
        copy.v_last = self.v_last;
        copy.tol_u = self.tol_u;
        copy.tol_v = self.tol_v;
        copy.surface_type = self.surface_type;
        copy.data = match &self.data {
            SurfaceData::Extrusion(d) => SurfaceData::Extrusion(ExtrusionData {
                basis: d.basis.shallow_copy()?,
                direction: d.direction,
                has_eval_rep: d.has_eval_rep,
            }),
            SurfaceData::Revolution(d) => SurfaceData::Revolution(RevolutionData {
                basis: d.basis.shallow_copy()?,
                axis: d.axis,
                has_eval_rep: d.has_eval_rep,
            }),
            SurfaceData::Offset(d) => SurfaceData::Offset(OffsetData {
                basis: Arc::new(d.basis.shallow_copied()?),
                equivalent: match &d.equivalent {
                    Some(e) => Some(Arc::new(e.shallow_copied()?)),
                    None => None,
                },
                offset_surface: d.offset_surface.clone(),
                offset: d.offset,
                has_eval_rep: d.has_eval_rep,
            }),
            // the caches are not copied: they are rebuilt on demand
            SurfaceData::BSpline(d) => SurfaceData::BSpline(BSplineData {
                surface: d.surface.clone(),
                cache: Mutex::new(None),
                has_eval_rep: d.has_eval_rep,
            }),
            SurfaceData::Bezier(d) => SurfaceData::Bezier(BezierData {
                surface: d.surface.clone(),
                cache: Mutex::new(None),
                has_eval_rep: d.has_eval_rep,
            }),
            SurfaceData::None => SurfaceData::None,
            SurfaceData::Pln(x) => SurfaceData::Pln(*x),
            SurfaceData::Cylinder(x) => SurfaceData::Cylinder(*x),
            SurfaceData::Cone(x) => SurfaceData::Cone(*x),
            SurfaceData::Sphere(x) => SurfaceData::Sphere(*x),
            SurfaceData::Torus(x) => SurfaceData::Torus(*x),
        };
        Ok(copy)
    }

    /// `UTrim` with the concrete type.
    pub fn u_trimmed(&self, first: f64, last: f64, tol: f64) -> Result<Surface> {
        Surface::with_range(self.surf()?.clone(), first, last, self.v_first, self.v_last, tol, self.tol_v)
    }

    /// `VTrim` with the concrete type.
    pub fn v_trimmed(&self, first: f64, last: f64, tol: f64) -> Result<Surface> {
        Surface::with_range(self.surf()?.clone(), self.u_first, self.u_last, first, last, self.tol_u, tol)
    }

    // the offset evaluation functions of the unnamed namespace
    fn offset_d0(&self, u: f64, v: f64, d: &OffsetData) -> Result<Option<Pnt>> {
        if let Some(e) = &d.equivalent {
            return Ok(Some(e.eval_d0(u, v)?));
        }
        Ok(evaluate_d0(u, v, d.basis.as_ref(), d.offset, d.offset_surface.osculating())?)
    }
    fn offset_d1(&self, u: f64, v: f64, d: &OffsetData) -> Result<Option<SurfResD1>> {
        if let Some(e) = &d.equivalent {
            return Ok(Some(e.eval_d1(u, v)?));
        }
        Ok(evaluate_d1(u, v, d.basis.as_ref(), d.offset, d.offset_surface.osculating())?)
    }
    fn offset_d2(&self, u: f64, v: f64, d: &OffsetData) -> Result<Option<SurfResD2>> {
        if let Some(e) = &d.equivalent {
            return Ok(Some(e.eval_d2(u, v)?));
        }
        Ok(evaluate_d2(u, v, d.basis.as_ref(), d.offset, d.offset_surface.osculating())?)
    }
    fn offset_d3(&self, u: f64, v: f64, d: &OffsetData) -> Result<Option<SurfResD3>> {
        if let Some(e) = &d.equivalent {
            return Ok(Some(e.eval_d3(u, v)?));
        }
        Ok(evaluate_d3(u, v, d.basis.as_ref(), d.offset, d.offset_surface.osculating())?)
    }
    fn offset_dn(&self, u: f64, v: f64, nu: i32, nv: i32, d: &OffsetData) -> Result<Option<Vec>> {
        if let Some(e) = &d.equivalent {
            return Ok(Some(e.eval_dn(u, v, nu, nv)?));
        }
        Ok(evaluate_dn(u, v, nu, nv, d.basis.as_ref(), d.offset, d.offset_surface.osculating())?)
    }
}

impl OffsetBasis for Surface {
    fn ob_d1(&self, u: f64, v: f64) -> geom::Result<SurfResD1> {
        self.eval_d1(u, v).map_err(to_geom_error)
    }
    fn ob_d2(&self, u: f64, v: f64) -> geom::Result<SurfResD2> {
        self.eval_d2(u, v).map_err(to_geom_error)
    }
    fn ob_d3(&self, u: f64, v: f64) -> geom::Result<SurfResD3> {
        self.eval_d3(u, v).map_err(to_geom_error)
    }
    fn ob_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> geom::Result<Vec> {
        self.eval_dn(u, v, nu, nv).map_err(to_geom_error)
    }
    fn ob_bounds(&self) -> (f64, f64, f64, f64) {
        self.bounds()
    }
    fn ob_is_u_periodic(&self) -> bool {
        self.surface.as_ref().is_some_and(|s| s.is_u_periodic())
    }
    fn ob_is_v_periodic(&self) -> bool {
        self.surface.as_ref().is_some_and(|s| s.is_v_periodic())
    }
}

impl Surface {
    /// `Some(nb + 1)` when `UIntervals` (`v == false`) or `VIntervals` replaces the array it is given by one of exactly that
    /// size (the B-spline paths), else `None`; see [`Curve::intervals_replace_array`].
    pub fn intervals_replace_array(&self, v: bool, s: GeomAbsShape) -> Result<Option<usize>> {
        let (first, last) = if v { (self.v_first, self.v_last) } else { (self.u_first, self.u_last) };
        match self.surface_type {
            GeomAbsSurfaceType::BSplineSurface => {
                let b = &self.bspline_data()?.surface;
                let iso = if v {
                    b.u_iso(b.u_knot(b.first_u_knot_index())?)?
                } else {
                    b.v_iso(b.v_knot(b.first_v_knot_index())?)?
                };
                Curve::with_range(Arc::from(iso), first, last)?.intervals_replace_array(s)
            }
            GeomAbsSurfaceType::SurfaceOfExtrusion if !v => {
                let x = self
                    .surf()?
                    .as_any()
                    .downcast_ref::<SurfaceOfLinearExtrusion>()
                    .ok_or(AdaptorError::NoSuchObject)?;
                Curve::with_range(x.basis_curve(), first, last)?.intervals_replace_array(s)
            }
            GeomAbsSurfaceType::SurfaceOfRevolution if v => {
                let x =
                    self.surf()?.as_any().downcast_ref::<SurfaceOfRevolution>().ok_or(AdaptorError::NoSuchObject)?;
                Curve::with_range(x.basis_curve(), first, last)?.intervals_replace_array(s)
            }
            GeomAbsSurfaceType::OffsetSurface => {
                let base_s = Self::offset_base_shape(s)?;
                self.offset_data()?.basis.intervals_replace_array(v, base_s)
            }
            _ => Ok(None),
        }
    }
}

impl SurfaceAdaptor for Surface {
    fn shallow_copy(&self) -> Result<Arc<dyn SurfaceAdaptor>> {
        Ok(Arc::new(self.shallow_copied()?))
    }

    fn first_u_parameter(&self) -> Result<f64> {
        Ok(self.u_first)
    }
    fn last_u_parameter(&self) -> Result<f64> {
        Ok(self.u_last)
    }
    fn first_v_parameter(&self) -> Result<f64> {
        Ok(self.v_first)
    }
    fn last_v_parameter(&self) -> Result<f64> {
        Ok(self.v_last)
    }

    fn u_continuity(&self) -> Result<GeomAbsShape> {
        match self.surface_type {
            GeomAbsSurfaceType::BSplineSurface => {
                let b = &self.bspline_data()?.surface;
                Self::local_continuity(
                    b.u_degree(),
                    b.nb_u_knots(),
                    b.u_knots(),
                    b.u_multiplicities(),
                    self.u_first,
                    self.u_last,
                    self.is_u_periodic()?,
                )
            }
            GeomAbsSurfaceType::OffsetSurface => {
                let basis = self.basis_surface()?.ok_or(AdaptorError::NullObject)?;
                Self::offset_continuity(basis.u_continuity()?)
            }
            GeomAbsSurfaceType::SurfaceOfExtrusion => match &self.data {
                SurfaceData::Extrusion(d) => d.basis.continuity(),
                _ => Err(AdaptorError::NoSuchObject),
            },
            GeomAbsSurfaceType::OtherSurface => Err(AdaptorError::NoSuchObject),
            _ => Ok(GeomAbsShape::Cn),
        }
    }

    fn v_continuity(&self) -> Result<GeomAbsShape> {
        match self.surface_type {
            GeomAbsSurfaceType::BSplineSurface => {
                let b = &self.bspline_data()?.surface;
                Self::local_continuity(
                    b.v_degree(),
                    b.nb_v_knots(),
                    b.v_knots(),
                    b.v_multiplicities(),
                    self.v_first,
                    self.v_last,
                    self.is_v_periodic()?,
                )
            }
            GeomAbsSurfaceType::OffsetSurface => {
                let basis = self.basis_surface()?.ok_or(AdaptorError::NullObject)?;
                Self::offset_continuity(basis.v_continuity()?)
            }
            GeomAbsSurfaceType::SurfaceOfRevolution => match &self.data {
                SurfaceData::Revolution(d) => d.basis.continuity(),
                _ => Err(AdaptorError::NoSuchObject),
            },
            GeomAbsSurfaceType::OtherSurface => Err(AdaptorError::NoSuchObject),
            _ => Ok(GeomAbsShape::Cn),
        }
    }

    fn nb_u_intervals(&self, s: GeomAbsShape) -> Result<i32> {
        match self.surface_type {
            GeomAbsSurfaceType::BSplineSurface => {
                let b = &self.bspline_data()?.surface;
                let iso = b.v_iso(b.v_knot(b.first_v_knot_index())?)?;
                Curve::with_range(Arc::from(iso), self.u_first, self.u_last)?.nb_intervals(s)
            }
            GeomAbsSurfaceType::SurfaceOfExtrusion => {
                let x = self
                    .surf()?
                    .as_any()
                    .downcast_ref::<SurfaceOfLinearExtrusion>()
                    .ok_or(AdaptorError::NoSuchObject)?;
                let c = Curve::with_range(x.basis_curve(), self.u_first, self.u_last)?;
                if c.get_type()? == crate::adaptor2d::GeomAbsCurveType::BSplineCurve {
                    return c.nb_intervals(s);
                }
                Ok(1)
            }
            GeomAbsSurfaceType::OffsetSurface => {
                let base_s = Self::offset_base_shape(s)?;
                self.offset_data()?.basis.nb_u_intervals(base_s)
            }
            _ => Ok(1),
        }
    }

    fn nb_v_intervals(&self, s: GeomAbsShape) -> Result<i32> {
        match self.surface_type {
            GeomAbsSurfaceType::BSplineSurface => {
                let b = &self.bspline_data()?.surface;
                let iso = b.u_iso(b.u_knot(b.first_u_knot_index())?)?;
                Curve::with_range(Arc::from(iso), self.v_first, self.v_last)?.nb_intervals(s)
            }
            GeomAbsSurfaceType::SurfaceOfRevolution => {
                let x =
                    self.surf()?.as_any().downcast_ref::<SurfaceOfRevolution>().ok_or(AdaptorError::NoSuchObject)?;
                let c = Curve::with_range(x.basis_curve(), self.v_first, self.v_last)?;
                if c.get_type()? == crate::adaptor2d::GeomAbsCurveType::BSplineCurve {
                    return c.nb_intervals(s);
                }
                Ok(1)
            }
            GeomAbsSurfaceType::OffsetSurface => {
                let base_s = Self::offset_base_shape(s)?;
                self.offset_data()?.basis.nb_v_intervals(base_s)
            }
            _ => Ok(1),
        }
    }

    fn u_intervals(&self, t: &mut [f64], s: GeomAbsShape) -> Result<()> {
        fn put(t: &mut [f64], k: usize, v: f64) -> Result<()> {
            *t.get_mut(k).ok_or(AdaptorError::OutOfRange)? = v;
            Ok(())
        }
        match self.surface_type {
            GeomAbsSurfaceType::BSplineSurface => {
                let b = &self.bspline_data()?.surface;
                let iso = b.v_iso(b.v_knot(b.first_v_knot_index())?)?;
                return Curve::with_range(Arc::from(iso), self.u_first, self.u_last)?.intervals(t, s);
            }
            GeomAbsSurfaceType::SurfaceOfExtrusion => {
                let x = self
                    .surf()?
                    .as_any()
                    .downcast_ref::<SurfaceOfLinearExtrusion>()
                    .ok_or(AdaptorError::NoSuchObject)?;
                let c = Curve::with_range(x.basis_curve(), self.u_first, self.u_last)?;
                if c.get_type()? == crate::adaptor2d::GeomAbsCurveType::BSplineCurve {
                    return c.intervals(t, s);
                }
            }
            GeomAbsSurfaceType::OffsetSurface => {
                let base_s = Self::offset_base_shape(s)?;
                return self.offset_data()?.basis.u_intervals(t, base_s);
            }
            _ => {}
        }
        put(t, 0, self.u_first)?;
        put(t, 1, self.u_last)
    }

    fn v_intervals(&self, t: &mut [f64], s: GeomAbsShape) -> Result<()> {
        fn put(t: &mut [f64], k: usize, v: f64) -> Result<()> {
            *t.get_mut(k).ok_or(AdaptorError::OutOfRange)? = v;
            Ok(())
        }
        match self.surface_type {
            GeomAbsSurfaceType::BSplineSurface => {
                let b = &self.bspline_data()?.surface;
                let iso = b.u_iso(b.u_knot(b.first_u_knot_index())?)?;
                return Curve::with_range(Arc::from(iso), self.v_first, self.v_last)?.intervals(t, s);
            }
            GeomAbsSurfaceType::SurfaceOfRevolution => {
                let x =
                    self.surf()?.as_any().downcast_ref::<SurfaceOfRevolution>().ok_or(AdaptorError::NoSuchObject)?;
                let c = Curve::with_range(x.basis_curve(), self.v_first, self.v_last)?;
                if c.get_type()? == crate::adaptor2d::GeomAbsCurveType::BSplineCurve {
                    return c.intervals(t, s);
                }
            }
            GeomAbsSurfaceType::OffsetSurface => {
                let base_s = Self::offset_base_shape(s)?;
                return self.offset_data()?.basis.v_intervals(t, base_s);
            }
            _ => {}
        }
        put(t, 0, self.v_first)?;
        put(t, 1, self.v_last)
    }

    fn u_trim(&self, first: f64, last: f64, tol: f64) -> Result<Arc<dyn SurfaceAdaptor>> {
        Ok(Arc::new(self.u_trimmed(first, last, tol)?))
    }

    fn v_trim(&self, first: f64, last: f64, tol: f64) -> Result<Arc<dyn SurfaceAdaptor>> {
        Ok(Arc::new(self.v_trimmed(first, last, tol)?))
    }

    fn is_u_closed(&self) -> Result<bool> {
        let s = self.surf()?;
        if !s.is_u_closed() {
            return Ok(false);
        }
        let (u1, u2, _, _) = s.bounds();
        if s.is_u_periodic() {
            return Ok(((u1 - u2).abs() - (self.u_first - self.u_last).abs()).abs() < pconfusion());
        }
        Ok((u1 - self.u_first).abs() < pconfusion() && (u2 - self.u_last).abs() < pconfusion())
    }

    fn is_v_closed(&self) -> Result<bool> {
        let s = self.surf()?;
        if !s.is_v_closed() {
            return Ok(false);
        }
        let (_, _, v1, v2) = s.bounds();
        if s.is_v_periodic() {
            return Ok(((v1 - v2).abs() - (self.v_first - self.v_last).abs()).abs() < pconfusion());
        }
        Ok((v1 - self.v_first).abs() < pconfusion() && (v2 - self.v_last).abs() < pconfusion())
    }

    fn is_u_periodic(&self) -> Result<bool> {
        Ok(self.surf()?.is_u_periodic())
    }

    fn u_period(&self) -> Result<f64> {
        // `Geom_Surface::UPeriod` as built: the difference of the bounds (the periodicity check is compiled out)
        let (u1, u2, _, _) = self.surf()?.bounds();
        Ok(u2 - u1)
    }

    fn is_v_periodic(&self) -> Result<bool> {
        Ok(self.surf()?.is_v_periodic())
    }

    fn v_period(&self) -> Result<f64> {
        let (_, _, v1, v2) = self.surf()?.bounds();
        Ok(v2 - v1)
    }

    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        match &self.data {
            SurfaceData::Pln(x) => Ok(elslib::d0_pln(u, v, x)),
            SurfaceData::Cylinder(x) => Ok(elslib::d0_cylinder(u, v, x)),
            SurfaceData::Cone(x) => Ok(elslib::d0_cone(u, v, x)),
            SurfaceData::Sphere(x) => Ok(elslib::d0_sphere(u, v, x)),
            SurfaceData::Torus(x) => Ok(elslib::d0_torus(u, v, x)),
            SurfaceData::Bezier(_) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d0(u, v)?);
                }
                self.with_cache(u, v, |c| c.d0(u, v))
            }
            SurfaceData::BSpline(_) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d0(u, v)?);
                }
                self.with_cache(u, v, |c| c.d0(u, v))
            }
            SurfaceData::Extrusion(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d0(u, v)?);
                }
                utils::extrusion_d0(u, v, d.basis.as_ref(), &d.direction)
            }
            SurfaceData::Revolution(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d0(u, v)?);
                }
                utils::revolution_d0(u, v, d.basis.as_ref(), &d.axis)
            }
            SurfaceData::Offset(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d0(u, v)?);
                }
                self.offset_d0(u, v, d)?.ok_or(AdaptorError::UndefinedValue)
            }
            SurfaceData::None => Ok(self.surf()?.eval_d0(u, v)?),
        }
    }

    fn eval_d1(&self, uu: f64, vv: f64) -> Result<SurfResD1> {
        let (u, v, us, vs) = self.snap(uu, vv);
        let r = |(point, d1u, d1v): (Pnt, Vec, Vec)| SurfResD1 { point, d1u, d1v };
        match &self.data {
            SurfaceData::Pln(x) => Ok(r(elslib::d1_pln(u, v, x))),
            SurfaceData::Cylinder(x) => Ok(r(elslib::d1_cylinder(u, v, x))),
            SurfaceData::Cone(x) => Ok(r(elslib::d1_cone(u, v, x))),
            SurfaceData::Sphere(x) => Ok(r(elslib::d1_sphere(u, v, x))),
            SurfaceData::Torus(x) => Ok(r(elslib::d1_torus(u, v, x))),
            SurfaceData::Bezier(_) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d1(uu, vv)?);
                }
                Ok(r(self.with_cache(uu, vv, |c| c.d1(uu, vv))?))
            }
            SurfaceData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d1(uu, vv)?);
                }
                if us != 0 || vs != 0 {
                    let sp = self.if_uv_bound(u, v, us, vs)?;
                    if sp.local {
                        return Ok(r(d.surface.local_d1(u, v, sp.u_deb, sp.u_fin, sp.v_deb, sp.v_fin)?));
                    }
                }
                Ok(r(self.with_cache(uu, vv, |c| c.d1(uu, vv))?))
            }
            SurfaceData::Extrusion(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d1(uu, vv)?);
                }
                utils::extrusion_d1(u, v, d.basis.as_ref(), &d.direction)
            }
            SurfaceData::Revolution(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d1(uu, vv)?);
                }
                utils::revolution_d1(u, v, d.basis.as_ref(), &d.axis)
            }
            SurfaceData::Offset(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d1(uu, vv)?);
                }
                self.offset_d1(u, v, d)?.ok_or(AdaptorError::UndefinedDerivative)
            }
            SurfaceData::None => Ok(self.surf()?.eval_d1(u, v)?),
        }
    }

    fn eval_d2(&self, uu: f64, vv: f64) -> Result<SurfResD2> {
        let (u, v, us, vs) = self.snap(uu, vv);
        let zero = Vec::new(0.0, 0.0, 0.0);
        let from = |(point, d1u, d1v, d2u, d2v, d2uv): elslib::D2Result| SurfResD2 { point, d1u, d1v, d2u, d2v, d2uv };
        match &self.data {
            SurfaceData::Pln(x) => {
                let (point, d1u, d1v) = elslib::d1_pln(u, v, x);
                Ok(SurfResD2 { point, d1u, d1v, d2u: zero, d2v: zero, d2uv: zero })
            }
            SurfaceData::Cylinder(x) => Ok(from(elslib::d2_cylinder(u, v, x))),
            SurfaceData::Cone(x) => Ok(from(elslib::d2_cone(u, v, x))),
            SurfaceData::Sphere(x) => Ok(from(elslib::d2_sphere(u, v, x))),
            SurfaceData::Torus(x) => Ok(from(elslib::d2_torus(u, v, x))),
            SurfaceData::Bezier(_) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d2(uu, vv)?);
                }
                Ok(from(self.with_cache(uu, vv, |c| c.d2(uu, vv))?))
            }
            SurfaceData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d2(uu, vv)?);
                }
                if us != 0 || vs != 0 {
                    let sp = self.if_uv_bound(u, v, us, vs)?;
                    if sp.local {
                        return Ok(from(d.surface.local_d2(u, v, sp.u_deb, sp.u_fin, sp.v_deb, sp.v_fin)?));
                    }
                }
                Ok(from(self.with_cache(uu, vv, |c| c.d2(uu, vv))?))
            }
            SurfaceData::Extrusion(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d2(uu, vv)?);
                }
                utils::extrusion_d2(u, v, d.basis.as_ref(), &d.direction)
            }
            SurfaceData::Revolution(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d2(uu, vv)?);
                }
                utils::revolution_d2(u, v, d.basis.as_ref(), &d.axis)
            }
            SurfaceData::Offset(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d2(uu, vv)?);
                }
                self.offset_d2(u, v, d)?.ok_or(AdaptorError::UndefinedDerivative)
            }
            SurfaceData::None => Ok(self.surf()?.eval_d2(u, v)?),
        }
    }

    fn eval_d3(&self, uu: f64, vv: f64) -> Result<SurfResD3> {
        let (u, v, us, vs) = self.snap(uu, vv);
        let zero = Vec::new(0.0, 0.0, 0.0);
        let from = |(point, d1u, d1v, d2u, d2v, d2uv, d3u, d3v, d3uuv, d3uvv): elslib::D3Result| SurfResD3 {
            point,
            d1u,
            d1v,
            d2u,
            d2v,
            d2uv,
            d3u,
            d3v,
            d3uuv,
            d3uvv,
        };
        match &self.data {
            SurfaceData::Pln(x) => {
                let (point, d1u, d1v) = elslib::d1_pln(u, v, x);
                Ok(SurfResD3 {
                    point,
                    d1u,
                    d1v,
                    d2u: zero,
                    d2v: zero,
                    d2uv: zero,
                    d3u: zero,
                    d3v: zero,
                    d3uuv: zero,
                    d3uvv: zero,
                })
            }
            SurfaceData::Cylinder(x) => Ok(from(elslib::d3_cylinder(u, v, x))),
            SurfaceData::Cone(x) => Ok(from(elslib::d3_cone(u, v, x))),
            SurfaceData::Sphere(x) => Ok(from(elslib::d3_sphere(u, v, x))),
            SurfaceData::Torus(x) => Ok(from(elslib::d3_torus(u, v, x))),
            SurfaceData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d3(uu, vv)?);
                }
                if us == 0 && vs == 0 {
                    return Ok(d.surface.eval_d3(u, v)?);
                }
                let sp = self.if_uv_bound(u, v, us, vs)?;
                if sp.local {
                    Ok(d.surface.local_d3(u, v, sp.u_deb, sp.u_fin, sp.v_deb, sp.v_fin)?)
                } else {
                    Ok(d.surface.eval_d3(u, v)?)
                }
            }
            SurfaceData::Extrusion(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d3(uu, vv)?);
                }
                utils::extrusion_d3(u, v, d.basis.as_ref(), &d.direction)
            }
            SurfaceData::Revolution(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d3(uu, vv)?);
                }
                utils::revolution_d3(u, v, d.basis.as_ref(), &d.axis)
            }
            SurfaceData::Offset(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_d3(uu, vv)?);
                }
                self.offset_d3(u, v, d)?.ok_or(AdaptorError::UndefinedDerivative)
            }
            SurfaceData::Bezier(_) | SurfaceData::None => Ok(self.surf()?.eval_d3(u, v)?),
        }
    }

    fn eval_dn(&self, uu: f64, vv: f64, nu: i32, nv: i32) -> Result<Vec> {
        let (u, v, us, vs) = self.snap(uu, vv);
        match &self.data {
            SurfaceData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_dn(uu, vv, nu, nv)?);
                }
                if us == 0 && vs == 0 {
                    return Ok(d.surface.eval_dn(u, v, nu, nv)?);
                }
                let sp = self.if_uv_bound(u, v, us, vs)?;
                if sp.local {
                    Ok(d.surface.local_dn(u, v, sp.u_deb, sp.u_fin, sp.v_deb, sp.v_fin, nu, nv)?)
                } else {
                    Ok(d.surface.eval_dn(u, v, nu, nv)?)
                }
            }
            SurfaceData::Extrusion(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_dn(uu, vv, nu, nv)?);
                }
                utils::extrusion_dn(u, d.basis.as_ref(), &d.direction, nu, nv)
            }
            SurfaceData::Revolution(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_dn(uu, vv, nu, nv)?);
                }
                utils::revolution_dn(u, v, d.basis.as_ref(), &d.axis, nu, nv)
            }
            SurfaceData::Offset(d) => {
                if self.has_eval_rep() {
                    return Ok(self.surf()?.eval_dn(uu, vv, nu, nv)?);
                }
                self.offset_dn(u, v, nu, nv, d)?.ok_or(AdaptorError::UndefinedDerivative)
            }
            SurfaceData::Pln(x) => Ok(elslib::dn_pln(u, v, x, nu, nv)),
            SurfaceData::Cylinder(x) => Ok(elslib::dn_cylinder(u, v, x, nu, nv)),
            SurfaceData::Cone(x) => Ok(elslib::dn_cone(u, v, x, nu, nv)),
            SurfaceData::Sphere(x) => Ok(elslib::dn_sphere(u, v, x, nu, nv)),
            SurfaceData::Torus(x) => Ok(elslib::dn_torus(u, v, x, nu, nv)),
            SurfaceData::Bezier(_) | SurfaceData::None => Ok(self.surf()?.eval_dn(u, v, nu, nv)?),
        }
    }

    fn u_resolution(&self, r3d: f64) -> Result<f64> {
        let mut res = 0.0;
        match &self.data {
            SurfaceData::Extrusion(d) => return d.basis.resolution(r3d),
            SurfaceData::Torus(_) => {
                let s = self.surf()?.as_any().downcast_ref::<ToroidalSurface>().ok_or(AdaptorError::NoSuchObject)?;
                let r = s.major_radius() + s.minor_radius();
                if r > confusion() {
                    res = r3d / (2.0 * r);
                }
            }
            SurfaceData::Sphere(_) => {
                let s = self.surf()?.as_any().downcast_ref::<SphericalSurface>().ok_or(AdaptorError::NoSuchObject)?;
                let r = s.radius();
                if r > confusion() {
                    res = r3d / (2.0 * r);
                }
            }
            SurfaceData::Cylinder(_) => {
                let s = self.surf()?.as_any().downcast_ref::<CylindricalSurface>().ok_or(AdaptorError::NoSuchObject)?;
                let r = s.radius();
                if r > confusion() {
                    res = r3d / (2.0 * r);
                }
            }
            SurfaceData::Cone(_) => {
                if self.v_last - self.v_first > 1.0e10 {
                    // not truly bounded: unknown resolution
                    return Ok(r3d * 0.01);
                }
                let s = self.surf()?;
                let radius_of = |v: f64| -> Result<f64> {
                    let c = s.v_iso(v)?;
                    Ok(c.as_any().downcast_ref::<Circle>().ok_or(AdaptorError::NoSuchObject)?.radius())
                };
                let rayon1 = radius_of(self.v_last)?;
                let rayon2 = radius_of(self.v_first)?;
                let r = if rayon1 > rayon2 { rayon1 } else { rayon2 };
                return Ok(if r > confusion() { r3d / r } else { 0.0 });
            }
            SurfaceData::Pln(_) => return Ok(r3d),
            SurfaceData::Bezier(d) => return Ok((*d.surface).clone().resolution(r3d).0),
            SurfaceData::BSpline(d) => return Ok((*d.surface).clone().resolution(r3d).0),
            SurfaceData::Offset(d) => return d.basis.u_resolution(r3d),
            _ => return Ok(r3d * 0.01),
        }
        if res <= 1.0 { Ok(2.0 * libm::asin(res)) } else { Ok(2.0 * std::f64::consts::PI) }
    }

    fn v_resolution(&self, r3d: f64) -> Result<f64> {
        let mut res = 0.0;
        match &self.data {
            SurfaceData::Revolution(d) => return d.basis.resolution(r3d),
            SurfaceData::Torus(_) => {
                let s = self.surf()?.as_any().downcast_ref::<ToroidalSurface>().ok_or(AdaptorError::NoSuchObject)?;
                let r = s.minor_radius();
                if r > confusion() {
                    res = r3d / (2.0 * r);
                }
            }
            SurfaceData::Sphere(_) => {
                let s = self.surf()?.as_any().downcast_ref::<SphericalSurface>().ok_or(AdaptorError::NoSuchObject)?;
                let r = s.radius();
                if r > confusion() {
                    res = r3d / (2.0 * r);
                }
            }
            SurfaceData::Extrusion(_) | SurfaceData::Cylinder(_) | SurfaceData::Cone(_) | SurfaceData::Pln(_) => {
                return Ok(r3d);
            }
            SurfaceData::Bezier(d) => return Ok((*d.surface).clone().resolution(r3d).1),
            SurfaceData::BSpline(d) => return Ok((*d.surface).clone().resolution(r3d).1),
            SurfaceData::Offset(d) => return d.basis.v_resolution(r3d),
            _ => return Ok(r3d * 0.01),
        }
        if res <= 1.0 { Ok(2.0 * libm::asin(res)) } else { Ok(2.0 * std::f64::consts::PI) }
    }

    fn get_type(&self) -> Result<GeomAbsSurfaceType> {
        Ok(self.surface_type)
    }

    fn plane(&self) -> Result<Pln> {
        match &self.data {
            SurfaceData::Pln(x) if self.surface_type == GeomAbsSurfaceType::Plane => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn cylinder(&self) -> Result<Cylinder> {
        match &self.data {
            SurfaceData::Cylinder(x) if self.surface_type == GeomAbsSurfaceType::Cylinder => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn cone(&self) -> Result<Cone> {
        match &self.data {
            SurfaceData::Cone(x) if self.surface_type == GeomAbsSurfaceType::Cone => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn sphere(&self) -> Result<Sphere> {
        match &self.data {
            SurfaceData::Sphere(x) if self.surface_type == GeomAbsSurfaceType::Sphere => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn torus(&self) -> Result<Torus> {
        match &self.data {
            SurfaceData::Torus(x) if self.surface_type == GeomAbsSurfaceType::Torus => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }

    fn u_degree(&self) -> Result<i32> {
        match &self.data {
            SurfaceData::BSpline(d) => Ok(d.surface.u_degree()),
            SurfaceData::Bezier(d) => Ok(d.surface.u_degree()),
            SurfaceData::Extrusion(d) => d.basis.degree(),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn nb_u_poles(&self) -> Result<i32> {
        match &self.data {
            SurfaceData::BSpline(d) => Ok(d.surface.nb_u_poles()),
            SurfaceData::Bezier(d) => Ok(d.surface.nb_u_poles()),
            SurfaceData::Extrusion(d) => d.basis.nb_poles(),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn v_degree(&self) -> Result<i32> {
        match &self.data {
            SurfaceData::BSpline(d) => Ok(d.surface.v_degree()),
            SurfaceData::Bezier(d) => Ok(d.surface.v_degree()),
            SurfaceData::Revolution(d) => d.basis.degree(),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn nb_v_poles(&self) -> Result<i32> {
        match &self.data {
            SurfaceData::BSpline(d) => Ok(d.surface.nb_v_poles()),
            SurfaceData::Bezier(d) => Ok(d.surface.nb_v_poles()),
            SurfaceData::Revolution(d) => d.basis.nb_poles(),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn nb_u_knots(&self) -> Result<i32> {
        match &self.data {
            SurfaceData::BSpline(d) => Ok(d.surface.nb_u_knots()),
            SurfaceData::Extrusion(d) => d.basis.nb_knots(),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn nb_v_knots(&self) -> Result<i32> {
        match &self.data {
            SurfaceData::BSpline(d) => Ok(d.surface.nb_v_knots()),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn is_u_rational(&self) -> Result<bool> {
        match &self.data {
            SurfaceData::BSpline(d) => Ok(d.surface.is_u_rational()),
            SurfaceData::Bezier(d) => Ok(d.surface.is_u_rational()),
            _ => Ok(false),
        }
    }
    fn is_v_rational(&self) -> Result<bool> {
        match &self.data {
            SurfaceData::BSpline(d) => Ok(d.surface.is_v_rational()),
            SurfaceData::Bezier(d) => Ok(d.surface.is_v_rational()),
            _ => Ok(false),
        }
    }
    fn bezier(&self) -> Result<Option<Arc<BezierSurface>>> {
        match &self.data {
            SurfaceData::Bezier(d) if self.surface_type == GeomAbsSurfaceType::BezierSurface => {
                Ok(Some(d.surface.clone()))
            }
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn bspline(&self) -> Result<Option<Arc<BSplineSurface>>> {
        match &self.data {
            SurfaceData::BSpline(d) if self.surface_type == GeomAbsSurfaceType::BSplineSurface => {
                Ok(Some(d.surface.clone()))
            }
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn axe_of_revolution(&self) -> Result<Ax1> {
        if self.surface_type != GeomAbsSurfaceType::SurfaceOfRevolution {
            return Err(AdaptorError::NoSuchObject);
        }
        let s = self.surf()?.as_any().downcast_ref::<SurfaceOfRevolution>().ok_or(AdaptorError::NoSuchObject)?;
        Ok(s.axis())
    }
    fn direction(&self) -> Result<Dir> {
        if self.surface_type != GeomAbsSurfaceType::SurfaceOfExtrusion {
            return Err(AdaptorError::NoSuchObject);
        }
        let s = self.surf()?.as_any().downcast_ref::<SurfaceOfLinearExtrusion>().ok_or(AdaptorError::NoSuchObject)?;
        Ok(s.direction())
    }
    fn basis_curve(&self) -> Result<Option<Arc<dyn Curve3dAdaptor>>> {
        let c = if self.surface_type == GeomAbsSurfaceType::SurfaceOfExtrusion {
            self.surf()?
                .as_any()
                .downcast_ref::<SurfaceOfLinearExtrusion>()
                .ok_or(AdaptorError::NoSuchObject)?
                .basis_curve()
        } else if self.surface_type == GeomAbsSurfaceType::SurfaceOfRevolution {
            self.surf()?.as_any().downcast_ref::<SurfaceOfRevolution>().ok_or(AdaptorError::NoSuchObject)?.basis_curve()
        } else {
            return Err(AdaptorError::NoSuchObject);
        };
        Ok(Some(Arc::new(Curve::with_curve(c)?)))
    }
    fn basis_surface(&self) -> Result<Option<Arc<dyn SurfaceAdaptor>>> {
        if self.surface_type != GeomAbsSurfaceType::OffsetSurface {
            return Err(AdaptorError::NoSuchObject);
        }
        let s = self.surf()?.as_any().downcast_ref::<OffsetSurface>().ok_or(AdaptorError::NoSuchObject)?;
        let a = Surface::with_range(s.basis_surface(), self.u_first, self.u_last, self.v_first, self.v_last, 0.0, 0.0)?;
        Ok(Some(Arc::new(a)))
    }
    fn offset_value(&self) -> Result<f64> {
        if self.surface_type != GeomAbsSurfaceType::OffsetSurface {
            return Err(AdaptorError::NoSuchObject);
        }
        let s = self.surf()?.as_any().downcast_ref::<OffsetSurface>().ok_or(AdaptorError::NoSuchObject)?;
        Ok(s.offset())
    }
}
