//! `Adaptor3d_IsoCurve`: the iso curve (a curve of constant U or V) of a surface adaptor, as an `Adaptor3d_Curve`.
//!
//! Translated from `Adaptor3d_IsoCurve.hxx/.cxx` (OCCT 8.0.1).
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use super::{
    Curve3dAdaptor, GeomAbsCurveType, GeomAbsIsoType, GeomAbsShape, GeomAbsSurfaceType, Result, SurfaceAdaptor,
};
use crate::adaptor2d::AdaptorError;
use crate::elclib;
use crate::elslib;
use crate::geom::{self, Geometry, ResD1, ResD2, ResD3, Surface};
use crate::geom2d::confusion;
use crate::gp::{Ax2, Ax3, Circ, Dir, Elips, Hypr, Lin, Parab, Pnt, Vec};
use std::sync::Arc;

fn cmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}
fn cmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}
fn no_such_object<T>() -> Result<T> {
    Err(AdaptorError::NoSuchObject)
}

/// `Adaptor3d_IsoCurve`.
#[derive(Debug, Clone)]
pub struct IsoCurve {
    surface: Option<Arc<dyn SurfaceAdaptor>>,
    iso: GeomAbsIsoType,
    first: f64,
    last: f64,
    parameter: f64,
}

impl Default for IsoCurve {
    fn default() -> Self {
        Self::new()
    }
}

impl IsoCurve {
    /// `Adaptor3d_IsoCurve()`.
    pub fn new() -> Self {
        Self { surface: None, iso: GeomAbsIsoType::NoneIso, first: 0.0, last: 0.0, parameter: 0.0 }
    }

    /// `Adaptor3d_IsoCurve(S)`.
    pub fn with_surface(s: Arc<dyn SurfaceAdaptor>) -> Self {
        Self { surface: Some(s), iso: GeomAbsIsoType::NoneIso, first: 0.0, last: 0.0, parameter: 0.0 }
    }

    /// `Adaptor3d_IsoCurve(S, Iso, Param)`: `Load(Iso, Param)`.
    pub fn with_iso(s: Arc<dyn SurfaceAdaptor>, iso: GeomAbsIsoType, param: f64) -> Result<Self> {
        let mut c = Self::with_surface(s);
        c.load_iso(iso, param)?;
        Ok(c)
    }

    /// `Adaptor3d_IsoCurve(S, Iso, Param, WFirst, WLast)`.
    pub fn with_iso_range(
        s: Arc<dyn SurfaceAdaptor>,
        iso: GeomAbsIsoType,
        param: f64,
        w_first: f64,
        w_last: f64,
    ) -> Result<Self> {
        let mut c = Self { surface: Some(s), iso, first: w_first, last: w_last, parameter: param };
        c.load_iso_range(iso, param, w_first, w_last)?;
        Ok(c)
    }

    /// `ShallowCopy` with the concrete type: the surface is shallow-copied as well.
    pub fn shallow_copied(&self) -> Result<IsoCurve> {
        let surface = match &self.surface {
            Some(s) => Some(s.shallow_copy()?),
            None => None,
        };
        Ok(Self { surface, iso: self.iso, first: self.first, last: self.last, parameter: self.parameter })
    }

    /// `Trim` with the concrete type: a copy sharing the surface, loaded with the given range.
    pub fn trimmed(&self, first: f64, last: f64, _tol: f64) -> Result<IsoCurve> {
        let mut c = self.clone();
        c.load_iso_range(self.iso, self.parameter, first, last)?;
        Ok(c)
    }

    /// `Load(S)`.
    pub fn load_surface(&mut self, s: Arc<dyn SurfaceAdaptor>) {
        self.surface = Some(s);
        self.iso = GeomAbsIsoType::NoneIso;
    }

    /// `Load(Iso, Param)`: the other direction runs over the whole range of the surface; `NoneIso` is `Err(NoSuchObject)`.
    pub fn load_iso(&mut self, iso: GeomAbsIsoType, param: f64) -> Result<()> {
        let s = self.surf()?.clone();
        match iso {
            GeomAbsIsoType::IsoU => {
                let l = s.last_v_parameter()?;
                let f = s.first_v_parameter()?;
                self.load_iso_range(iso, param, f, l)
            }
            GeomAbsIsoType::IsoV => {
                let l = s.last_u_parameter()?;
                let f = s.first_u_parameter()?;
                self.load_iso_range(iso, param, f, l)
            }
            GeomAbsIsoType::NoneIso => no_such_object(),
        }
    }

    /// `Load(Iso, Param, WFirst, WLast)`: the range is cut to the range of the surface and, on a periodic surface, moved into the period.
    pub fn load_iso_range(&mut self, iso: GeomAbsIsoType, param: f64, w_first: f64, w_last: f64) -> Result<()> {
        self.iso = iso;
        self.parameter = param;
        self.first = w_first;
        self.last = w_last;
        let s = self.surf()?.clone();
        if self.iso == GeomAbsIsoType::IsoU {
            self.first = cmax(self.first, s.first_v_parameter()?);
            self.last = cmin(self.last, s.last_v_parameter()?);
        } else {
            self.first = cmax(self.first, s.first_u_parameter()?);
            self.last = cmin(self.last, s.last_u_parameter()?);
        }
        // adjust the parameters on periodic surfaces
        if s.is_u_periodic()? {
            // (the arguments of the C++ call are evaluated from the last to the first)
            let resolution = s.u_resolution(confusion())?;
            let first = s.first_u_parameter()?;
            let period = s.u_period()?;
            let first2 = s.first_u_parameter()?;
            if self.iso == GeomAbsIsoType::IsoU {
                (self.parameter, _) =
                    elclib::adjust_periodic(first2, first + period, resolution, self.parameter, self.parameter);
            } else {
                (self.first, self.last) =
                    elclib::adjust_periodic(first2, first + period, resolution, self.first, self.last);
            }
        }
        if s.is_v_periodic()? {
            let resolution = s.v_resolution(confusion())?;
            let first = s.first_v_parameter()?;
            let period = s.v_period()?;
            let first2 = s.first_v_parameter()?;
            if self.iso == GeomAbsIsoType::IsoV {
                (self.parameter, _) =
                    elclib::adjust_periodic(first2, first + period, resolution, self.parameter, self.parameter);
            } else {
                (self.first, self.last) =
                    elclib::adjust_periodic(first2, first + period, resolution, self.first, self.last);
            }
        }
        Ok(())
    }

    /// `Surface`.
    pub fn surface(&self) -> Option<&Arc<dyn SurfaceAdaptor>> {
        self.surface.as_ref()
    }

    /// `Iso`.
    pub fn iso(&self) -> GeomAbsIsoType {
        self.iso
    }

    /// `Parameter`.
    pub fn parameter(&self) -> f64 {
        self.parameter
    }

    fn surf(&self) -> Result<&Arc<dyn SurfaceAdaptor>> {
        self.surface.as_ref().ok_or(AdaptorError::NullObject)
    }

    // the intervals of the surface in the direction of the curve
    fn surface_intervals(&self, s: GeomAbsShape) -> Result<(i32, std::vec::Vec<f64>)> {
        if self.iso == GeomAbsIsoType::NoneIso {
            return no_such_object();
        }
        let surf = self.surf()?;
        let u_iso = self.iso == GeomAbsIsoType::IsoU;
        let nb_inter = if u_iso { surf.nb_v_intervals(s)? } else { surf.nb_u_intervals(s)? };
        let mut t = vec![0.0; (nb_inter + 1).max(0) as usize];
        if u_iso {
            surf.v_intervals(&mut t, s)?;
        } else {
            surf.u_intervals(&mut t, s)?;
        }
        Ok((nb_inter, t))
    }
}

fn at(t: &[f64], i: i32) -> Result<f64> {
    usize::try_from(i - 1).ok().and_then(|k| t.get(k)).copied().ok_or(AdaptorError::OutOfRange)
}

impl Curve3dAdaptor for IsoCurve {
    fn shallow_copy(&self) -> Result<Arc<dyn Curve3dAdaptor>> {
        Ok(Arc::new(self.shallow_copied()?))
    }

    fn first_parameter(&self) -> Result<f64> {
        Ok(self.first)
    }

    fn last_parameter(&self) -> Result<f64> {
        Ok(self.last)
    }

    fn continuity(&self) -> Result<GeomAbsShape> {
        match self.iso {
            GeomAbsIsoType::IsoU => self.surf()?.v_continuity(),
            GeomAbsIsoType::IsoV => self.surf()?.u_continuity(),
            GeomAbsIsoType::NoneIso => no_such_object(),
        }
    }

    fn nb_intervals(&self, s: GeomAbsShape) -> Result<i32> {
        let (nb_inter, t) = self.surface_intervals(s)?;
        if nb_inter == 1 {
            return Ok(nb_inter);
        }
        let mut first = 1;
        while at(&t, first)? <= self.first {
            first += 1;
        }
        let mut last = nb_inter + 1;
        while at(&t, last)? >= self.last {
            last -= 1;
        }
        Ok(last - first + 2)
    }

    fn intervals(&self, ti: &mut [f64], s: GeomAbsShape) -> Result<()> {
        let (nb_inter, t) = self.surface_intervals(s)?;
        if nb_inter == 1 {
            super::put(ti, 0, self.first)?;
            super::put(ti, 1, self.last)?;
            return Ok(());
        }
        let mut first = 1;
        while at(&t, first)? <= self.first {
            first += 1;
        }
        let mut last = nb_inter + 1;
        while at(&t, last)? >= self.last {
            last -= 1;
        }
        for (i, j) in ((first - 1)..=(last + 1)).enumerate() {
            super::put(ti, i, at(&t, j)?)?;
        }
        super::put(ti, 0, self.first)?;
        super::put(ti, (last - first + 2) as usize, self.last)?;
        Ok(())
    }

    fn trim(&self, first: f64, last: f64, tol: f64) -> Result<Arc<dyn Curve3dAdaptor>> {
        Ok(Arc::new(self.trimmed(first, last, tol)?))
    }

    fn is_closed(&self) -> Result<bool> {
        match self.iso {
            GeomAbsIsoType::IsoU => self.surf()?.is_v_closed(),
            GeomAbsIsoType::IsoV => self.surf()?.is_u_closed(),
            GeomAbsIsoType::NoneIso => no_such_object(),
        }
    }

    fn is_periodic(&self) -> Result<bool> {
        match self.iso {
            GeomAbsIsoType::IsoU => self.surf()?.is_v_periodic(),
            GeomAbsIsoType::IsoV => self.surf()?.is_u_periodic(),
            GeomAbsIsoType::NoneIso => no_such_object(),
        }
    }

    fn period(&self) -> Result<f64> {
        match self.iso {
            GeomAbsIsoType::IsoU => self.surf()?.v_period(),
            GeomAbsIsoType::IsoV => self.surf()?.u_period(),
            GeomAbsIsoType::NoneIso => no_such_object(),
        }
    }

    fn eval_d0(&self, u: f64) -> Result<Pnt> {
        match self.iso {
            GeomAbsIsoType::IsoU => self.surf()?.eval_d0(self.parameter, u),
            GeomAbsIsoType::IsoV => self.surf()?.eval_d0(u, self.parameter),
            GeomAbsIsoType::NoneIso => no_such_object(),
        }
    }

    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        match self.iso {
            GeomAbsIsoType::IsoU => {
                let r = self.surf()?.eval_d1(self.parameter, u)?;
                Ok(ResD1 { point: r.point, d1: r.d1v })
            }
            GeomAbsIsoType::IsoV => {
                let r = self.surf()?.eval_d1(u, self.parameter)?;
                Ok(ResD1 { point: r.point, d1: r.d1u })
            }
            GeomAbsIsoType::NoneIso => no_such_object(),
        }
    }

    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        match self.iso {
            GeomAbsIsoType::IsoU => {
                let r = self.surf()?.eval_d2(self.parameter, u)?;
                Ok(ResD2 { point: r.point, d1: r.d1v, d2: r.d2v })
            }
            GeomAbsIsoType::IsoV => {
                let r = self.surf()?.eval_d2(u, self.parameter)?;
                Ok(ResD2 { point: r.point, d1: r.d1u, d2: r.d2u })
            }
            GeomAbsIsoType::NoneIso => no_such_object(),
        }
    }

    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        match self.iso {
            GeomAbsIsoType::IsoU => {
                let r = self.surf()?.eval_d3(self.parameter, u)?;
                Ok(ResD3 { point: r.point, d1: r.d1v, d2: r.d2v, d3: r.d3v })
            }
            GeomAbsIsoType::IsoV => {
                let r = self.surf()?.eval_d3(u, self.parameter)?;
                Ok(ResD3 { point: r.point, d1: r.d1u, d2: r.d2u, d3: r.d3u })
            }
            GeomAbsIsoType::NoneIso => no_such_object(),
        }
    }

    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec> {
        match self.iso {
            GeomAbsIsoType::IsoU => self.surf()?.eval_dn(self.parameter, u, 0, n),
            GeomAbsIsoType::IsoV => self.surf()?.eval_dn(u, self.parameter, n, 0),
            GeomAbsIsoType::NoneIso => no_such_object(),
        }
    }

    fn resolution(&self, r3d: f64) -> Result<f64> {
        // can we do better?
        Ok(r3d * 0.01)
    }

    fn get_type(&self) -> Result<GeomAbsCurveType> {
        let surf = self.surf()?;
        Ok(match surf.get_type()? {
            GeomAbsSurfaceType::Plane => GeomAbsCurveType::Line,
            GeomAbsSurfaceType::Cylinder | GeomAbsSurfaceType::Cone => match self.iso {
                GeomAbsIsoType::IsoU => GeomAbsCurveType::Line,
                GeomAbsIsoType::IsoV => GeomAbsCurveType::Circle,
                GeomAbsIsoType::NoneIso => return no_such_object(),
            },
            GeomAbsSurfaceType::Sphere | GeomAbsSurfaceType::Torus => GeomAbsCurveType::Circle,
            GeomAbsSurfaceType::BezierSurface => GeomAbsCurveType::BezierCurve,
            GeomAbsSurfaceType::BSplineSurface => GeomAbsCurveType::BSplineCurve,
            GeomAbsSurfaceType::SurfaceOfRevolution => match self.iso {
                GeomAbsIsoType::IsoU => basis_curve(surf)?.get_type()?,
                GeomAbsIsoType::IsoV => GeomAbsCurveType::Circle,
                GeomAbsIsoType::NoneIso => return no_such_object(),
            },
            GeomAbsSurfaceType::SurfaceOfExtrusion => match self.iso {
                GeomAbsIsoType::IsoU => GeomAbsCurveType::Line,
                GeomAbsIsoType::IsoV => basis_curve(surf)?.get_type()?,
                GeomAbsIsoType::NoneIso => return no_such_object(),
            },
            _ => GeomAbsCurveType::OtherCurve,
        })
    }

    fn line(&self) -> Result<Lin> {
        let r = self.eval_d1(0.0)?;
        Ok(Lin::new(r.point, Dir::from_vec(&r.d1)?))
    }

    fn circle(&self) -> Result<Circ> {
        let surf = self.surf()?;
        match surf.get_type()? {
            GeomAbsSurfaceType::Cylinder => {
                let cyl = surf.cylinder()?;
                match self.iso {
                    GeomAbsIsoType::IsoU => no_such_object(),
                    GeomAbsIsoType::IsoV => Ok(elslib::cylinder_viso(cyl.position(), cyl.radius(), self.parameter)),
                    GeomAbsIsoType::NoneIso => no_such_object(),
                }
            }
            GeomAbsSurfaceType::Cone => {
                let cone = surf.cone()?;
                match self.iso {
                    GeomAbsIsoType::IsoU => no_such_object(),
                    GeomAbsIsoType::IsoV => {
                        Ok(elslib::cone_viso(cone.position(), cone.ref_radius(), cone.semi_angle(), self.parameter))
                    }
                    GeomAbsIsoType::NoneIso => no_such_object(),
                }
            }
            GeomAbsSurfaceType::Sphere => {
                let sph = surf.sphere()?;
                match self.iso {
                    GeomAbsIsoType::IsoU => Ok(elslib::sphere_uiso(sph.position(), sph.radius(), self.parameter)),
                    GeomAbsIsoType::IsoV => Ok(elslib::sphere_viso(sph.position(), sph.radius(), self.parameter)),
                    GeomAbsIsoType::NoneIso => no_such_object(),
                }
            }
            GeomAbsSurfaceType::Torus => {
                let tor = surf.torus()?;
                match self.iso {
                    GeomAbsIsoType::IsoU => {
                        Ok(elslib::torus_uiso(tor.position(), tor.major_radius(), tor.minor_radius(), self.parameter))
                    }
                    GeomAbsIsoType::IsoV => {
                        Ok(elslib::torus_viso(tor.position(), tor.major_radius(), tor.minor_radius(), self.parameter))
                    }
                    GeomAbsIsoType::NoneIso => no_such_object(),
                }
            }
            GeomAbsSurfaceType::SurfaceOfRevolution => {
                if self.iso == GeomAbsIsoType::IsoV {
                    let val0 = self.value(0.0)?;
                    let ax1 = surf.axe_of_revolution()?;
                    if Lin::from_axis(ax1).contains(&val0, confusion()) {
                        return Ok(Circ::new(Ax2::from_direction(val0, *ax1.direction()), 0.0)?);
                    }
                    let dx = Vec::between(ax1.location(), &val0);
                    let mut axes = Ax3::new(*ax1.location(), *ax1.direction(), Dir::from_vec(&dx)?)?;
                    // computeHR
                    let v = Vec::between(axes.location(), &val0);
                    let h = v.dot(&Vec::from_dir(axes.direction()));
                    let radius = v.dot(&Vec::from_dir(axes.x_direction()));
                    let vt = Vec::from_dir(axes.direction());
                    axes.translate(&vt.multiplied(h));
                    Ok(Circ::new(axes.ax2(), radius)?)
                } else {
                    let basis = basis_curve(surf)?.circle()?;
                    Ok(basis.rotated(&surf.axe_of_revolution()?, self.parameter))
                }
            }
            GeomAbsSurfaceType::SurfaceOfExtrusion => {
                let basis = basis_curve(surf)?.circle()?;
                let d = surf.direction()?;
                Ok(basis.translated(&Vec::from_dir(&d).multiplied(self.parameter)))
            }
            _ => no_such_object(),
        }
    }

    fn ellipse(&self) -> Result<Elips> {
        let surf = self.surf()?;
        match surf.get_type()? {
            GeomAbsSurfaceType::SurfaceOfExtrusion => {
                let basis = basis_curve(surf)?.ellipse()?;
                let d = surf.direction()?;
                Ok(basis.translated(&Vec::from_dir(&d).multiplied(self.parameter)))
            }
            _ => no_such_object(),
        }
    }

    fn hyperbola(&self) -> Result<Hypr> {
        no_such_object()
    }

    fn parabola(&self) -> Result<Parab> {
        no_such_object()
    }

    fn degree(&self) -> Result<i32> {
        let surf = self.surf()?;
        match surf.get_type()? {
            GeomAbsSurfaceType::BezierSurface | GeomAbsSurfaceType::BSplineSurface => match self.iso {
                GeomAbsIsoType::IsoU => surf.v_degree(),
                GeomAbsIsoType::IsoV => surf.u_degree(),
                GeomAbsIsoType::NoneIso => no_such_object(),
            },
            GeomAbsSurfaceType::SurfaceOfRevolution => match self.iso {
                GeomAbsIsoType::IsoU => basis_curve(surf)?.degree(),
                _ => no_such_object(),
            },
            GeomAbsSurfaceType::SurfaceOfExtrusion => match self.iso {
                GeomAbsIsoType::IsoV => basis_curve(surf)?.degree(),
                _ => no_such_object(),
            },
            _ => no_such_object(),
        }
    }

    fn is_rational(&self) -> Result<bool> {
        let surf = self.surf()?;
        match surf.get_type()? {
            GeomAbsSurfaceType::BezierSurface | GeomAbsSurfaceType::BSplineSurface => match self.iso {
                GeomAbsIsoType::IsoU => surf.is_v_rational(),
                GeomAbsIsoType::IsoV => surf.is_u_rational(),
                GeomAbsIsoType::NoneIso => no_such_object(),
            },
            GeomAbsSurfaceType::SurfaceOfRevolution => match self.iso {
                GeomAbsIsoType::IsoU => basis_curve(surf)?.is_rational(),
                _ => no_such_object(),
            },
            GeomAbsSurfaceType::SurfaceOfExtrusion => match self.iso {
                GeomAbsIsoType::IsoV => basis_curve(surf)?.is_rational(),
                _ => no_such_object(),
            },
            _ => no_such_object(),
        }
    }

    fn nb_poles(&self) -> Result<i32> {
        let surf = self.surf()?;
        match surf.get_type()? {
            GeomAbsSurfaceType::BezierSurface | GeomAbsSurfaceType::BSplineSurface => match self.iso {
                GeomAbsIsoType::IsoU => surf.nb_v_poles(),
                GeomAbsIsoType::IsoV => surf.nb_u_poles(),
                GeomAbsIsoType::NoneIso => no_such_object(),
            },
            GeomAbsSurfaceType::SurfaceOfRevolution => match self.iso {
                GeomAbsIsoType::IsoU => basis_curve(surf)?.nb_poles(),
                _ => no_such_object(),
            },
            GeomAbsSurfaceType::SurfaceOfExtrusion => match self.iso {
                GeomAbsIsoType::IsoV => basis_curve(surf)?.nb_poles(),
                _ => no_such_object(),
            },
            _ => no_such_object(),
        }
    }

    fn nb_knots(&self) -> Result<i32> {
        let surf = self.surf()?;
        match surf.get_type()? {
            GeomAbsSurfaceType::BSplineSurface => match self.iso {
                GeomAbsIsoType::IsoU => surf.nb_v_knots(),
                GeomAbsIsoType::IsoV => surf.nb_u_knots(),
                GeomAbsIsoType::NoneIso => no_such_object(),
            },
            GeomAbsSurfaceType::SurfaceOfRevolution => match self.iso {
                GeomAbsIsoType::IsoU => basis_curve(surf)?.nb_knots(),
                _ => no_such_object(),
            },
            GeomAbsSurfaceType::SurfaceOfExtrusion => match self.iso {
                GeomAbsIsoType::IsoV => basis_curve(surf)?.nb_knots(),
                _ => no_such_object(),
            },
            _ => no_such_object(),
        }
    }

    fn bezier(&self) -> Result<Option<Arc<geom::BezierCurve>>> {
        let surf = self.surf()?;
        let c: geom::BezierCurve = if surf.get_type()? == GeomAbsSurfaceType::SurfaceOfRevolution {
            let mut c = null_check(basis_curve(surf)?.bezier()?)?.as_ref().clone();
            c.rotate(&surf.axe_of_revolution()?, self.parameter);
            c
        } else if surf.get_type()? == GeomAbsSurfaceType::SurfaceOfExtrusion {
            let mut c = null_check(basis_curve(surf)?.bezier()?)?.as_ref().clone();
            c.translate(&Vec::from_dir(&surf.direction()?).multiplied(self.parameter));
            c
        } else {
            let s = null_check(surf.bezier()?)?;
            let iso =
                if self.iso == GeomAbsIsoType::IsoU { s.u_iso(self.parameter)? } else { s.v_iso(self.parameter)? };
            match iso.as_any().downcast_ref::<geom::BezierCurve>() {
                Some(b) => b.clone(),
                None => return Ok(None),
            }
        };
        Ok(Some(Arc::new(c)))
    }

    fn bspline(&self) -> Result<Option<Arc<geom::BSplineCurve>>> {
        let surf = self.surf()?;
        let c: geom::BSplineCurve = if surf.get_type()? == GeomAbsSurfaceType::SurfaceOfRevolution {
            let mut c = null_check(basis_curve(surf)?.bspline()?)?.as_ref().clone();
            c.rotate(&surf.axe_of_revolution()?, self.parameter);
            c
        } else if surf.get_type()? == GeomAbsSurfaceType::SurfaceOfExtrusion {
            let mut c = null_check(basis_curve(surf)?.bspline()?)?.as_ref().clone();
            c.translate(&Vec::from_dir(&surf.direction()?).multiplied(self.parameter));
            c
        } else {
            let s = null_check(surf.bspline()?)?;
            let iso =
                if self.iso == GeomAbsIsoType::IsoU { s.u_iso(self.parameter)? } else { s.v_iso(self.parameter)? };
            match iso.as_any().downcast_ref::<geom::BSplineCurve>() {
                Some(b) => b.clone(),
                None => return Ok(None),
            }
        };
        Ok(Some(Arc::new(c)))
    }
}

// the basis curve of a surface (a null handle is a null object)
fn basis_curve(surf: &Arc<dyn SurfaceAdaptor>) -> Result<Arc<dyn Curve3dAdaptor>> {
    null_check(surf.basis_curve()?)
}

fn null_check<T>(h: Option<T>) -> Result<T> {
    h.ok_or(AdaptorError::NullObject)
}
