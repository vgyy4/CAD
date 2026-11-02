//! `Adaptor3d_TopolTool`: the topological tool of a surface adaptor (its boundary lines in the parametric space, a
//! point classifier, the sample points of the surface).
//!
//! Translated from `Adaptor3d_TopolTool.hxx/.cxx` (OCCT 8.0.1). The class is not final in C++ (its methods are virtual);
//! here it is a struct with inherent methods. The arrays of the C++ are 1-based, the handles of the arrays `Vec`s.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use super::{GeomAbsSurfaceType, HVertex, Result, SurfaceAdaptor};
use crate::adaptor2d::{AdaptorError, Curve2dAdaptor, Line2d};
use crate::geom;
use crate::gp::{self, Cone, Dir, Dir2d, Lin, Pnt, Pnt2d, Trsf, Vec};
use crate::topabs::{Orientation, State};
use std::sync::Arc;

// `Precision::Infinite()`
const INFINITE: f64 = 2.0e100;
// the absolute minimum of sample points (a restriction of IntPolyh)
const MIN_PNTS: i32 = 4;

fn is_neg_inf(x: f64) -> bool {
    x <= -INFINITE
}
fn is_pos_inf(x: f64) -> bool {
    x >= INFINITE
}
fn imin(a: i32, b: i32) -> i32 {
    if b < a { b } else { a }
}
fn imax(a: i32, b: i32) -> i32 {
    if a < b { b } else { a }
}

// a 1-based array of the C++
#[derive(Debug, Clone)]
struct A1<T: Copy + Default>(std::vec::Vec<T>);
impl<T: Copy + Default> A1<T> {
    fn new(n: i32) -> Self {
        Self(vec![T::default(); n.max(0) as usize])
    }
    fn at(&self, i: i32) -> Result<T> {
        usize::try_from(i - 1).ok().and_then(|k| self.0.get(k)).copied().ok_or(AdaptorError::OutOfRange)
    }
    fn set(&mut self, i: i32, x: T) -> Result<()> {
        *usize::try_from(i - 1).ok().and_then(|k| self.0.get_mut(k)).ok_or(AdaptorError::OutOfRange)? = x;
        Ok(())
    }
}

/// `Adaptor3d_TopolTool`.
#[derive(Debug)]
pub struct TopolTool {
    s: Option<Arc<dyn SurfaceAdaptor>>,
    nb_samples_u: i32,
    nb_samples_v: i32,
    u_pars: Option<std::vec::Vec<f64>>,
    v_pars: Option<std::vec::Vec<f64>>,
    nb_restr: usize,
    id_restr: usize,
    uinf: f64,
    usup: f64,
    vinf: f64,
    vsup: f64,
    restr: std::vec::Vec<Arc<Line2d>>,
    nb_vtx: usize,
    id_vtx: usize,
    vtx: std::vec::Vec<Arc<HVertex>>,
}

impl Default for TopolTool {
    fn default() -> Self {
        Self::new()
    }
}

// the flags of a point against the bounds of the parametric domain
struct Flags {
    dansu: bool,
    dansv: bool,
    surumin: bool,
    surumax: bool,
    survmin: bool,
    survmax: bool,
}

impl TopolTool {
    /// `Adaptor3d_TopolTool()`.
    pub fn new() -> Self {
        Self {
            s: None,
            nb_samples_u: -1,
            nb_samples_v: -1,
            u_pars: None,
            v_pars: None,
            nb_restr: 0,
            id_restr: 0,
            uinf: 0.0,
            usup: 0.0,
            vinf: 0.0,
            vsup: 0.0,
            restr: std::vec::Vec::new(),
            nb_vtx: 0,
            id_vtx: 0,
            vtx: std::vec::Vec::new(),
        }
    }

    /// `Adaptor3d_TopolTool(Surface)`: `Initialize(S)` (the members it does not set are 0 / -1 here, uninitialised in C++).
    pub fn with_surface(s: Arc<dyn SurfaceAdaptor>) -> Result<Self> {
        let mut t = Self::new();
        t.initialize_surface(s)?;
        Ok(t)
    }

    /// `Initialize()`: `Standard_NotImplemented`.
    pub fn initialize(&mut self) -> Result<()> {
        Err(AdaptorError::NotImplemented)
    }

    fn surf(&self) -> Result<&Arc<dyn SurfaceAdaptor>> {
        self.s.as_ref().ok_or(AdaptorError::NullObject)
    }

    /// `Initialize(S)`: the lines that bound the (finite part of the) parametric domain of the surface.
    pub fn initialize_surface(&mut self, s: Arc<dyn SurfaceAdaptor>) -> Result<()> {
        self.nb_samples_u = -1;
        self.uinf = s.first_u_parameter()?;
        self.vinf = s.first_v_parameter()?;
        self.usup = s.last_u_parameter()?;
        self.vsup = s.last_v_parameter()?;
        self.nb_restr = 0;
        self.id_restr = 0;
        self.restr.clear();
        let uinfinfinite = is_neg_inf(self.uinf);
        let usupinfinite = is_pos_inf(self.usup);
        let vinfinfinite = is_neg_inf(self.vinf);
        let vsupinfinite = is_pos_inf(self.vsup);
        let (uinf, usup, vinf, vsup) = (self.uinf, self.usup, self.vinf, self.vsup);
        let dir = |x: f64, y: f64| -> Result<Dir2d> { Ok(Dir2d::new(x, y)?) };
        // the range of the parameter of a line along the direction whose bounds are `lo` and `hi`
        let range = |lo: f64, hi: f64| -> (f64, f64) {
            let deltap = imin_f(hi - lo, 2.0 * INFINITE);
            if lo >= -INFINITE {
                (lo, lo + deltap)
            } else if hi <= INFINITE {
                (hi - deltap, hi)
            } else {
                (-INFINITE, INFINITE)
            }
        };
        if !vinfinfinite {
            let (pinf, psup) = range(uinf, usup);
            self.restr.push(Arc::new(Line2d::with_point_dir(Pnt2d::new(0.0, vinf), dir(1.0, 0.0)?, pinf, psup)));
            self.nb_restr += 1;
        }
        if !usupinfinite {
            let (pinf, psup) = range(vinf, vsup);
            self.restr.push(Arc::new(Line2d::with_point_dir(Pnt2d::new(usup, 0.0), dir(0.0, 1.0)?, pinf, psup)));
            self.nb_restr += 1;
        }
        if !vsupinfinite {
            let deltap = imin_f(usup - uinf, 2.0 * INFINITE);
            let (pinf, psup) = if -usup >= -INFINITE {
                (-usup, -usup + deltap)
            } else if -uinf <= INFINITE {
                (-uinf - deltap, -uinf)
            } else {
                (-INFINITE, INFINITE)
            };
            self.restr.push(Arc::new(Line2d::with_point_dir(Pnt2d::new(0.0, vsup), dir(-1.0, 0.0)?, pinf, psup)));
            self.nb_restr += 1;
        }
        if !uinfinfinite {
            let deltap = imin_f(vsup - vinf, 2.0 * INFINITE);
            let (pinf, psup) = if -vsup >= -INFINITE {
                (-vsup, -vsup + deltap)
            } else if -vinf <= INFINITE {
                (-vinf - deltap, -vinf)
            } else {
                (-INFINITE, INFINITE)
            };
            self.restr.push(Arc::new(Line2d::with_point_dir(Pnt2d::new(uinf, 0.0), dir(0.0, -1.0)?, pinf, psup)));
            self.nb_restr += 1;
        }
        self.s = Some(s.clone());
        if self.nb_restr == 2 && s.get_type()? == GeomAbsSurfaceType::Cone {
            let (u, v) = Self::get_cone_apex_param(&s.cone()?);
            let (pinf, psup) = range(uinf, usup);
            self.restr.push(Arc::new(Line2d::with_point_dir(Pnt2d::new(u, v), dir(1.0, 0.0)?, pinf, psup)));
            self.nb_restr += 1;
        }
        Ok(())
    }

    /// `Initialize(Curve)`: the vertices at the finite ends of the curve.
    pub fn initialize_curve(&mut self, c: Arc<dyn Curve2dAdaptor>) -> Result<()> {
        self.nb_vtx = 0;
        self.id_vtx = 0;
        self.vtx.clear();
        let theuinf = c.first_parameter()?;
        let theusup = c.last_parameter()?;
        if theuinf > -INFINITE {
            self.vtx.push(Arc::new(HVertex::new(c.value(theuinf)?, Orientation::Forward, 1.0e-8)));
            self.nb_vtx += 1;
        }
        if theusup < INFINITE {
            self.vtx.push(Arc::new(HVertex::new(c.value(theusup)?, Orientation::Reversed, 1.0e-8)));
            self.nb_vtx += 1;
        }
        Ok(())
    }

    /// `Init`: restarts the iteration on the boundary lines.
    pub fn init(&mut self) {
        self.id_restr = 0;
    }

    /// `More`.
    pub fn more(&self) -> bool {
        self.id_restr < self.nb_restr
    }

    /// `Value`: the current boundary line (`DomainError` after the last one).
    pub fn value(&self) -> Result<Arc<dyn Curve2dAdaptor>> {
        if self.id_restr >= self.nb_restr {
            return Err(AdaptorError::Domain);
        }
        let r = self.restr.get(self.id_restr).ok_or(AdaptorError::OutOfRange)?;
        Ok(r.clone())
    }

    /// `Next`.
    pub fn next(&mut self) {
        self.id_restr += 1;
    }

    /// `InitVertexIterator`.
    pub fn init_vertex_iterator(&mut self) {
        self.id_vtx = 0;
    }

    /// `MoreVertex`.
    pub fn more_vertex(&self) -> bool {
        self.id_vtx < self.nb_vtx
    }

    /// `Vertex` (`DomainError` after the last one).
    pub fn vertex(&self) -> Result<Arc<HVertex>> {
        if self.id_vtx >= self.nb_vtx {
            return Err(AdaptorError::Domain);
        }
        self.vtx.get(self.id_vtx).cloned().ok_or(AdaptorError::OutOfRange)
    }

    /// `NextVertex`.
    pub fn next_vertex(&mut self) {
        self.id_vtx += 1;
    }

    // the position of the point against the bounds of the domain, for a domain with infinite parts
    fn flags(&self, u: f64, v: f64, tol: f64) -> Flags {
        let (uinf, usup, vinf, vsup) = (self.uinf, self.usup, self.vinf, self.vsup);
        let (dansu, surumin, surumax);
        if is_neg_inf(uinf) && is_pos_inf(usup) {
            dansu = true;
            surumin = false;
            surumax = false;
        } else if is_neg_inf(uinf) {
            surumin = false;
            if u >= usup + tol {
                dansu = false;
                surumax = false;
            } else {
                dansu = true;
                surumax = (u - usup).abs() <= tol;
            }
        } else if is_pos_inf(usup) {
            surumax = false;
            if u < uinf - tol {
                dansu = false;
                surumin = false;
            } else {
                dansu = true;
                surumin = (u - uinf).abs() <= tol;
            }
        } else if u < uinf - tol || u > usup + tol {
            dansu = false;
            surumin = false;
            surumax = false;
        } else {
            dansu = true;
            if (u - uinf).abs() <= tol {
                surumin = true;
                surumax = false;
            } else {
                surumin = false;
                surumax = (u - usup).abs() <= tol;
            }
        }
        let (dansv, survmin, survmax);
        if is_neg_inf(vinf) && is_pos_inf(vsup) {
            dansv = true;
            survmin = false;
            survmax = false;
        } else if is_neg_inf(vinf) {
            survmin = false;
            if v > vsup + tol {
                dansv = false;
                survmax = false;
            } else {
                dansv = true;
                survmax = (v - vsup).abs() <= tol;
            }
        } else if is_pos_inf(vsup) {
            survmax = false;
            if v < vinf - tol {
                dansv = false;
                survmin = false;
            } else {
                dansv = true;
                survmin = (v - vinf).abs() <= tol;
            }
        } else if v < vinf - tol || v > vsup + tol {
            dansv = false;
            survmin = false;
            survmax = false;
        } else {
            dansv = true;
            if (v - vinf).abs() <= tol {
                survmin = true;
                survmax = false;
            } else {
                survmin = false;
                survmax = (v - vsup).abs() <= tol;
            }
        }
        Flags { dansu, dansv, surumin, surumax, survmin, survmax }
    }

    /// `Classify`: `In`, `Out` or `On` the boundary, within `tol`.
    pub fn classify(&self, p: &Pnt2d, tol: f64, _reacdre_on_periodic: bool) -> State {
        let (u, v) = (p.x(), p.y());
        if self.nb_restr == 4 {
            if u < self.uinf - tol || u > self.usup + tol || v < self.vinf - tol || v > self.vsup + tol {
                return State::Out;
            }
            if (u - self.uinf).abs() <= tol
                || (u - self.usup).abs() <= tol
                || (v - self.vinf).abs() <= tol
                || (v - self.vsup).abs() <= tol
            {
                return State::On;
            }
            State::In
        } else if self.nb_restr == 0 {
            State::In
        } else {
            let f = self.flags(u, v, tol);
            if !f.dansu || !f.dansv {
                return State::Out;
            }
            if f.surumin || f.survmin || f.surumax || f.survmax {
                return State::On;
            }
            State::In
        }
    }

    /// `IsThePointOn`.
    pub fn is_the_point_on(&self, p: &Pnt2d, tol: f64, _reacdre_on_periodic: bool) -> bool {
        let (u, v) = (p.x(), p.y());
        if self.nb_restr == 4 {
            if u < self.uinf - tol || u > self.usup + tol || v < self.vinf - tol || v > self.vsup + tol {
                return false;
            }
            (u - self.uinf).abs() <= tol
                || (u - self.usup).abs() <= tol
                || (v - self.vinf).abs() <= tol
                || (v - self.vsup).abs() <= tol
        } else if self.nb_restr == 0 {
            false
        } else {
            let f = self.flags(u, v, tol);
            if !f.dansu || !f.dansv {
                return false;
            }
            f.surumin || f.survmin || f.surumax || f.survmax
        }
    }

    /// `Orientation(C)`: always `Forward`.
    pub fn orientation_curve(&self, _c: &dyn Curve2dAdaptor) -> Orientation {
        Orientation::Forward
    }

    /// `Orientation(V)`.
    pub fn orientation_vertex(&self, v: &HVertex) -> Orientation {
        v.orientation()
    }

    /// `Identical`.
    pub fn identical(&self, v1: &HVertex, v2: &HVertex) -> bool {
        v1.is_same(v2)
    }

    /// `Has3d`: `false`.
    pub fn has_3d(&self) -> bool {
        false
    }

    /// `Tol3d(C)`: `DomainError` (no 3D representation).
    pub fn tol_3d_curve(&self, _c: &dyn Curve2dAdaptor) -> Result<f64> {
        Err(AdaptorError::Domain)
    }

    /// `Tol3d(V)`: `DomainError`.
    pub fn tol_3d_vertex(&self, _v: &HVertex) -> Result<f64> {
        Err(AdaptorError::Domain)
    }

    /// `Pnt(V)`: `DomainError`.
    pub fn pnt(&self, _v: &HVertex) -> Result<Pnt> {
        Err(AdaptorError::Domain)
    }

    /// `DomainIsInfinite`.
    pub fn domain_is_infinite(&self) -> bool {
        is_neg_inf(self.uinf) || is_pos_inf(self.usup) || is_neg_inf(self.vinf) || is_pos_inf(self.vsup)
    }

    /// `Edge`: the null pointer (`None`).
    pub fn edge(&self) -> Option<()> {
        None
    }

    /// `IsUniformSampling`: every surface but a B-spline one.
    pub fn is_uniform_sampling(&self) -> Result<bool> {
        Ok(self.surf()?.get_type()? != GeomAbsSurfaceType::BSplineSurface)
    }

    // the parameter range of the surface with the infinite parts replaced by 1e5-wide ones
    fn sample_range(&self) -> Result<(f64, f64, f64, f64)> {
        let s = self.surf()?;
        let mut uinf = s.first_u_parameter()?;
        let mut usup = s.last_u_parameter()?;
        let mut vinf = s.first_v_parameter()?;
        let mut vsup = s.last_v_parameter()?;
        if usup < uinf {
            std::mem::swap(&mut uinf, &mut usup);
        }
        if vsup < vinf {
            std::mem::swap(&mut vinf, &mut vsup);
        }
        let (rf, rl) = (-f64::MAX, f64::MAX);
        if uinf == rf && usup == rl {
            uinf = -1.0e5;
            usup = 1.0e5;
        } else if uinf == rf {
            uinf = usup - 2.0e5;
        } else if usup == rl {
            usup = uinf + 2.0e5;
        }
        if vinf == rf && vsup == rl {
            vinf = -1.0e5;
            vsup = 1.0e5;
        } else if vinf == rf {
            vinf = vsup - 2.0e5;
        } else if vsup == rl {
            vsup = vinf + 2.0e5;
        }
        Ok((uinf, usup, vinf, vsup))
    }

    /// `ComputeSamplePoints`: the numbers of sample points in U and V.
    pub fn compute_sample_points(&mut self) -> Result<()> {
        const MAX_NB_SAMPLE: i32 = 50;
        let (uinf, usup, vinf, vsup) = self.sample_range()?;
        let s = self.surf()?.clone();
        let typ = s.get_type()?;
        let (mut nbsu, mut nbsv);
        match typ {
            GeomAbsSurfaceType::Plane => {
                nbsv = 2;
                nbsu = 2;
            }
            GeomAbsSurfaceType::BezierSurface => {
                nbsv = 3 + s.nb_v_poles()?;
                nbsu = 3 + s.nb_u_poles()?;
            }
            GeomAbsSurfaceType::BSplineSurface => {
                nbsv = s.nb_v_knots()?;
                nbsv *= s.v_degree()?;
                if nbsv < 4 {
                    nbsv = 4;
                }
                nbsu = s.nb_u_knots()?;
                nbsu *= s.u_degree()?;
                if nbsu < 4 {
                    nbsu = 4;
                }
            }
            GeomAbsSurfaceType::Cylinder
            | GeomAbsSurfaceType::Cone
            | GeomAbsSurfaceType::Sphere
            | GeomAbsSurfaceType::Torus
            | GeomAbsSurfaceType::SurfaceOfRevolution
            | GeomAbsSurfaceType::SurfaceOfExtrusion => {
                nbsv = 15;
                nbsu = 15;
            }
            _ => {
                nbsu = 10;
                nbsv = 10;
            }
        }
        // if the number of points is too great... analyse
        if nbsu < 6 {
            nbsu = 6;
        }
        if nbsv < 6 {
            nbsv = 6;
        }
        if typ == GeomAbsSurfaceType::BSplineSurface {
            if nbsu > 8 || nbsv > 8 {
                let bspl = s.bspline()?.ok_or(AdaptorError::NullObject)?;
                analyse(bspl.poles(), &mut nbsu, &mut nbsv)?;
            }
            // check the anisotropy
            let an_u_len = (usup - uinf) / s.u_resolution(1.0)?;
            let an_v_len = (vsup - vinf) / s.v_resolution(1.0)?;
            let ratio = an_u_len / an_v_len;
            if ratio >= 10.0 {
                nbsu *= 2;
                nbsu = imin(nbsu, MAX_NB_SAMPLE);
            } else if ratio <= 0.1 {
                nbsv *= 2;
                nbsv = imin(nbsv, MAX_NB_SAMPLE);
            }
        } else if typ == GeomAbsSurfaceType::BezierSurface && (nbsu > 8 || nbsv > 8) {
            let bez = s.bezier()?.ok_or(AdaptorError::NullObject)?;
            analyse(bez.poles(), &mut nbsu, &mut nbsv)?;
        }
        self.nb_samples_u = nbsu;
        self.nb_samples_v = nbsv;
        Ok(())
    }

    /// `NbSamplesU`.
    pub fn nb_samples_u(&mut self) -> Result<i32> {
        if self.nb_samples_u < 0 {
            self.compute_sample_points()?;
        }
        Ok(self.nb_samples_u)
    }

    /// `NbSamplesV`.
    pub fn nb_samples_v(&mut self) -> Result<i32> {
        if self.nb_samples_u < 0 {
            self.compute_sample_points()?;
        }
        Ok(self.nb_samples_v)
    }

    /// `NbSamples`.
    pub fn nb_samples(&mut self) -> Result<i32> {
        if self.nb_samples_u < 0 {
            self.compute_sample_points()?;
        }
        Ok(self.nb_samples_u.wrapping_mul(self.nb_samples_v))
    }

    /// `UParameters`: the U parameters of the samples (the array is a copy; null before `SamplePnts`: `Err(NullObject)`).
    pub fn u_parameters(&self) -> Result<std::vec::Vec<f64>> {
        self.u_pars.clone().ok_or(AdaptorError::NullObject)
    }

    /// `VParameters`.
    pub fn v_parameters(&self) -> Result<std::vec::Vec<f64>> {
        self.v_pars.clone().ok_or(AdaptorError::NullObject)
    }

    /// `SamplePoint`: the sample `i` (1-based) as a parametric and a 3D point.
    pub fn sample_point(&mut self, i: i32) -> Result<(Pnt2d, Pnt)> {
        let (u, v);
        if let (None, _) | (_, None) = (&self.u_pars, &self.v_pars) {
            if self.nb_samples_u == 0 {
                return Err(AdaptorError::Domain);
            }
            let my_du = (self.usup - self.uinf) / f64::from(self.nb_samples_u + 1);
            let my_dv = (self.vsup - self.vinf) / f64::from(self.nb_samples_v + 1);
            let iv = 1 + i / self.nb_samples_u;
            let iu = 1 + i - (iv - 1) * self.nb_samples_u;
            u = self.uinf + f64::from(iu) * my_du;
            v = self.vinf + f64::from(iv) * my_dv;
        } else {
            if self.nb_samples_u == 0 {
                return Err(AdaptorError::Domain);
            }
            let iv = (i - 1) / self.nb_samples_u + 1;
            let iu = (i - 1) % self.nb_samples_u + 1;
            u = A1(self.u_pars.clone().ok_or(AdaptorError::NullObject)?).at(iu)?;
            v = A1(self.v_pars.clone().ok_or(AdaptorError::NullObject)?).at(iv)?;
        }
        let p3 = self.surf()?.value(u, v)?;
        Ok((Pnt2d::new(u, v), p3))
    }

    /// `SamplePnts`: the uniform sample parameters (a B-spline surface: `BSplSamplePnts`).
    pub fn sample_pnts(&mut self, defl: f64, nu_min: i32, nv_min: i32) -> Result<()> {
        let (uinf, usup, vinf, vsup) = self.sample_range()?;
        let typ = self.surf()?.get_type()?;
        if typ == GeomAbsSurfaceType::BSplineSurface {
            return self.bspl_sample_pnts(defl, nu_min, nv_min);
        }
        self.compute_sample_points()?;
        let (nu, nv) = (self.nb_samples_u, self.nb_samples_v);
        let mut up = A1::<f64>::new(nu);
        let mut vp = A1::<f64>::new(nv);
        let mut dt = (usup - uinf) / f64::from(nu - 1);
        up.set(1, uinf)?;
        up.set(nu, usup)?;
        let mut t = uinf + dt;
        let mut i = 2;
        while i < nu {
            up.set(i, t)?;
            i += 1;
            t += dt;
        }
        dt = (vsup - vinf) / f64::from(nv - 1);
        vp.set(1, vinf)?;
        vp.set(nv, vsup)?;
        t = vinf + dt;
        i = 2;
        while i < nv {
            vp.set(i, t)?;
            i += 1;
            t += dt;
        }
        self.u_pars = Some(up.0);
        self.v_pars = Some(vp.0);
        Ok(())
    }

    /// `BSplSamplePnts`: sample parameters of a B-spline surface that follow its knots and are thinned out where the surface is
    /// flat within `defl`.
    #[allow(clippy::cognitive_complexity)]
    pub fn bspl_sample_pnts(&mut self, defl: f64, nu_min: i32, nv_min: i32) -> Result<()> {
        const MAX_PNTS: i32 = 1001;
        let s = self.surf()?.clone();
        let abs = null(s.bspline()?)?;
        let uinf = s.first_u_parameter()?;
        let usup = s.last_u_parameter()?;
        let vinf = s.first_v_parameter()?;
        let vsup = s.last_v_parameter()?;
        let mut i;
        let mut j: i32;
        let mut ui1 = abs.first_u_knot_index();
        let mut ui2 = abs.last_u_knot_index();
        let mut vi1 = abs.first_v_knot_index();
        let mut vi2 = abs.last_v_knot_index();
        i = ui1;
        while i < ui2 {
            if uinf >= abs.u_knot(i)? && uinf < abs.u_knot(i + 1)? {
                ui1 = i;
                break;
            }
            i += 1;
        }
        i = ui2;
        while i > ui1 {
            if usup <= abs.u_knot(i)? && usup > abs.u_knot(i - 1)? {
                ui2 = i;
                break;
            }
            i -= 1;
        }
        i = vi1;
        while i < vi2 {
            if vinf >= abs.v_knot(i)? && vinf < abs.v_knot(i + 1)? {
                vi1 = i;
                break;
            }
            i += 1;
        }
        i = vi2;
        while i > vi1 {
            if vsup <= abs.v_knot(i)? && vsup > abs.v_knot(i - 1)? {
                vi2 = i;
                break;
            }
            i -= 1;
        }
        let mut nbsu = ui2 - ui1 + 1;
        nbsu += (nbsu - 1) * (abs.u_degree() - 1);
        let mut nbsv = vi2 - vi1 + 1;
        nbsv += (nbsv - 1) * (abs.v_degree() - 1);
        let mut b_uuniform = false;
        let mut b_vuniform = false;
        if nbsu < nu_min || nbsv < nv_min {
            let a_nb;
            if nbsu < nbsv {
                a_nb = imin((f64::from(nbsv) * f64::from(nu_min) / f64::from(nbsu)) as i32, 30);
                b_vuniform = if a_nb > nbsv { true } else { b_vuniform };
                nbsv = if b_vuniform { a_nb } else { nbsv };
            } else {
                a_nb = imin((f64::from(nbsu) * f64::from(nv_min) / f64::from(nbsv)) as i32, 30);
                b_uuniform = if a_nb > nbsu { true } else { b_uuniform };
                nbsu = if b_uuniform { a_nb } else { nbsu };
            }
        }
        if nbsu < nu_min {
            nbsu = nu_min;
            b_uuniform = true;
        } else if nbsu > MAX_PNTS {
            nbsu = MAX_PNTS;
            b_uuniform = true;
        }
        if nbsv < nv_min {
            nbsv = nv_min;
            b_vuniform = true;
        } else if nbsv > MAX_PNTS {
            nbsv = MAX_PNTS;
            b_vuniform = true;
        }
        let mut an_upars = A1::<f64>::new(nbsu);
        let mut an_uflg = A1::<bool>::new(nbsu);
        let mut a_vpars = A1::<f64>::new(nbsv);
        let mut a_vflg = A1::<bool>::new(nbsv);
        j = 1;
        // the parameters of one direction: uniform, or the knots with `degree` points in each span
        #[allow(clippy::too_many_arguments)]
        fn fill(
            uniform: bool,
            nbs: i32,
            inf: f64,
            sup: f64,
            pars: &mut A1<f64>,
            flg: &mut A1<bool>,
            degree: i32,
            i1: i32,
            i2: i32,
            knot: &dyn Fn(i32) -> Result<f64>,
            j: &mut i32,
        ) -> Result<()> {
            let mut t1 = inf;
            if uniform {
                let t2 = sup;
                let dt = (t2 - t1) / f64::from(nbs - 1);
                pars.set(1, t1)?;
                flg.set(1, false)?;
                pars.set(nbs, t2)?;
                flg.set(nbs, false)?;
                let mut i = 2;
                t1 += dt;
                while i < nbs {
                    pars.set(i, t1)?;
                    flg.set(i, false)?;
                    i += 1;
                    t1 += dt;
                }
            } else {
                let nbi = degree;
                let mut k = 0;
                for i in (i1 + 1)..=i2 {
                    let t2 = if i == i2 { sup } else { knot(i)? };
                    let dt = (t2 - t1) / f64::from(nbi);
                    *j = 1;
                    loop {
                        k += 1;
                        pars.set(k, t1)?;
                        flg.set(k, false)?;
                        t1 += dt;
                        *j += 1;
                        if *j > nbi {
                            break;
                        }
                    }
                    t1 = t2;
                }
                k += 1;
                pars.set(k, t1)?;
            }
            Ok(())
        }
        fill(
            b_uuniform,
            nbsu,
            uinf,
            usup,
            &mut an_upars,
            &mut an_uflg,
            abs.u_degree(),
            ui1,
            ui2,
            &|i| Ok(abs.u_knot(i)?),
            &mut j,
        )?;
        fill(
            b_vuniform,
            nbsv,
            vinf,
            vsup,
            &mut a_vpars,
            &mut a_vflg,
            abs.v_degree(),
            vi1,
            vi2,
            &|i| Ok(abs.v_knot(i)?),
            &mut j,
        )?;
        let a_defl2 = (defl * defl).max(1.0e-9);
        let tol = (0.01 * a_defl2).max(1.0e-9);
        an_uflg.set(1, true)?;
        an_uflg.set(nbsu, true)?;
        // thins out one direction: `pars`/`flg` the parameters of the direction, `other` those the lines are taken at
        #[allow(clippy::too_many_arguments)]
        fn thin(
            s: &Arc<dyn SurfaceAdaptor>,
            u_dir: bool,
            nbs: i32,
            others: &A1<f64>,
            nb_others: i32,
            pars: &A1<f64>,
            flg: &mut A1<bool>,
            a_defl2: f64,
            tol: f64,
            j: &mut i32,
        ) -> Result<()> {
            let val = |par: f64, other: f64| if u_dir { s.value(par, other) } else { s.value(other, par) };
            for i in 1..=nb_others {
                let t1 = others.at(i)?;
                *j = 1;
                let mut b_cont = true;
                while *j < nbs - 1 && b_cont {
                    if flg.at(*j + 1)? {
                        *j += 1;
                        continue;
                    }
                    let t2 = pars.at(*j)?;
                    let p1 = val(t2, t1)?;
                    let mut k = *j + 2;
                    while k <= nbs {
                        let t2 = pars.at(k)?;
                        let p2 = val(t2, t1)?;
                        if p1.square_distance(&p2) <= tol {
                            k += 1;
                            continue;
                        }
                        let lin = Lin::new(p1, Dir::from_vec(&Vec::between(&p1, &p2))?);
                        let mut ok = true;
                        let mut l = *j + 1;
                        while l < k {
                            if flg.at(l)? {
                                ok = false;
                                break;
                            }
                            let pp = val(pars.at(l)?, t1)?;
                            let d = lin.square_distance(&pp);
                            if d <= a_defl2 {
                                l += 1;
                                continue;
                            }
                            ok = false;
                            break;
                        }
                        if !ok {
                            *j = k - 1;
                            flg.set(*j, true)?;
                            break;
                        }
                        if flg.at(k)? {
                            *j = k;
                            break;
                        }
                        k += 1;
                    }
                    if k >= nbs {
                        b_cont = false;
                    }
                }
            }
            Ok(())
        }
        thin(&s, true, nbsu, &a_vpars, nbsv, &an_upars, &mut an_uflg, a_defl2, tol, &mut j)?;
        let mut nb_samples_u = 0;
        for i in 1..=nbsu {
            if an_uflg.at(i)? {
                nb_samples_u += 1;
            }
        }
        if nb_samples_u < MIN_PNTS {
            if nb_samples_u == 2 {
                let nn = nbsu / MIN_PNTS;
                an_uflg.set(1 + nn, true)?;
                an_uflg.set(nbsu - nn, true)?;
            } else {
                // 3 samples
                i = 2;
                loop {
                    let f = an_uflg.at(i)?;
                    i += 1;
                    if f {
                        break;
                    }
                }
                if i < nbsu / 2 {
                    j = imin(i + (nbsu - i) / 2, nbsu - 1);
                } else {
                    j = imax(i / 2, 2);
                }
            }
            an_uflg.set(j, true)?;
            nb_samples_u = MIN_PNTS;
        }
        a_vflg.set(1, true)?;
        a_vflg.set(nbsv, true)?;
        thin(&s, false, nbsv, &an_upars, nbsu, &a_vpars, &mut a_vflg, a_defl2, tol, &mut j)?;
        let mut nb_samples_v = 0;
        for i in 1..=nbsv {
            if a_vflg.at(i)? {
                nb_samples_v += 1;
            }
        }
        if nb_samples_v < MIN_PNTS {
            if nb_samples_v == 2 {
                let nn = nbsv / MIN_PNTS;
                a_vflg.set(1 + nn, true)?;
                a_vflg.set(nbsv - nn, true)?;
            } else {
                i = 2;
                loop {
                    let f = a_vflg.at(i)?;
                    i += 1;
                    if f {
                        break;
                    }
                }
                if i < nbsv / 2 {
                    j = imin(i + (nbsv - i) / 2, nbsv - 1);
                } else {
                    j = imax(i / 2, 2);
                }
            }
            nb_samples_v = MIN_PNTS;
            a_vflg.set(j, true)?;
        }
        let b_flag = nb_samples_u < nu_min;
        if b_flag {
            nb_samples_u = nbsu;
        }
        let mut upars = A1::<f64>::new(nb_samples_u);
        j = 0;
        for i in 1..=nbsu {
            if b_flag {
                upars.set(i, an_upars.at(i)?)?;
            } else if an_uflg.at(i)? {
                j += 1;
                upars.set(j, an_upars.at(i)?)?;
            }
        }
        let b_flag = nb_samples_v < nv_min;
        if b_flag {
            nb_samples_v = nbsv;
        }
        let mut vpars = A1::<f64>::new(nb_samples_v);
        j = 0;
        for i in 1..=nbsv {
            if b_flag {
                vpars.set(i, a_vpars.at(i)?)?;
            } else if a_vflg.at(i)? {
                j += 1;
                vpars.set(j, a_vpars.at(i)?)?;
            }
        }
        self.nb_samples_u = nb_samples_u;
        self.nb_samples_v = nb_samples_v;
        self.u_pars = Some(upars.0);
        self.v_pars = Some(vpars.0);
        Ok(())
    }

    /// `GetConeApexParam`: the parameters of the apex of the cone.
    pub fn get_cone_apex_param(c: &Cone) -> (f64, f64) {
        let pos = c.position();
        let radius = c.ref_radius();
        let s_angle = c.semi_angle();
        let p = c.apex();
        let mut t = Trsf::default();
        t.set_transformation(pos);
        let ploc = p.transformed(&t);
        let mut u;
        if ploc.x() == 0.0 && ploc.y() == 0.0 {
            u = 0.0;
        } else if -radius > ploc.z() * libm::tan(s_angle) {
            // the point is at the wrong side of the apex
            u = libm::atan2(-ploc.y(), -ploc.x());
        } else {
            u = libm::atan2(ploc.y(), ploc.x());
        }
        if u < -1.0e-16 {
            u += std::f64::consts::PI + std::f64::consts::PI;
        } else if u < 0.0 {
            u = 0.0;
        }
        let v = libm::sin(s_angle) * (ploc.x() * libm::cos(u) + ploc.y() * libm::sin(u) - radius)
            + libm::cos(s_angle) * ploc.z();
        (u, v)
    }
}

fn imin_f(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

fn null<T>(h: Option<T>) -> Result<T> {
    h.ok_or(AdaptorError::NullObject)
}

// `Analyse`: the numbers of sample points from the number of changes of the sign of the second differences of the poles
fn analyse(poles: &[std::vec::Vec<Pnt>], myu: &mut i32, myv: &mut i32) -> Result<()> {
    let nbup = poles.len() as i32;
    let nbvp = poles.first().map_or(0, |r| r.len()) as i32;
    let p = |i: i32, j: i32| -> Result<Pnt> {
        let r = usize::try_from(i - 1).ok().and_then(|k| poles.get(k)).ok_or(AdaptorError::OutOfRange)?;
        r.get(usize::try_from(j - 1).map_err(|_| AdaptorError::OutOfRange)?).copied().ok_or(AdaptorError::OutOfRange)
    };
    let second = |a: &Pnt, b: &Pnt, c: &Pnt| {
        Vec::new(c.x() - b.x() - b.x() + a.x(), c.y() - b.y() - b.y() + a.y(), c.z() - b.z() - b.z() + a.z())
    };
    let mut sh = 1;
    let mut nbch = 0;
    if nbvp > 2 {
        for i in 2..nbup {
            let mut vi = second(&p(i, 1)?, &p(i, 2)?, &p(i, 3)?);
            let mut locnbch = 0;
            for j in 3..nbvp {
                let vip1 = second(&p(i, j - 1)?, &p(i, j)?, &p(i, j + 1)?);
                let pd = vi.dot(&vip1);
                vi = vip1;
                if !(-1.0e-7..=1.0e-7).contains(&pd) {
                    if pd > 0.0 {
                        if sh == -1 {
                            sh = 1;
                            locnbch += 1;
                        }
                    } else if sh == 1 {
                        sh = -1;
                        locnbch += 1;
                    }
                }
            }
            if locnbch > nbch {
                nbch = locnbch;
            }
        }
    }
    *myv = nbch + 5;
    nbch = 0;
    if nbup > 2 {
        for j in 2..nbvp {
            let mut vi = second(&p(1, j)?, &p(2, j)?, &p(3, j)?);
            let mut locnbch = 0;
            for i in 3..nbup {
                let vip1 = second(&p(i - 1, j)?, &p(i, j)?, &p(i + 1, j)?);
                let pd = vi.dot(&vip1);
                vi = vip1;
                if !(-1.0e-7..=1.0e-7).contains(&pd) {
                    if pd > 0.0 {
                        if sh == -1 {
                            sh = 1;
                            locnbch += 1;
                        }
                    } else if sh == 1 {
                        sh = -1;
                        locnbch += 1;
                    }
                }
            }
            if locnbch > nbch {
                nbch = locnbch;
            }
        }
    }
    *myu = nbch + 5;
    Ok(())
}

// keeps the unused imports of this module's helpers honest
#[allow(dead_code)]
fn _unused(_: &geom::BezierSurface, _: &gp::Ax1) {}
