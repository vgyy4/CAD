//! OCCT's `Geom` package (toolkit TKG3d): the 3D geometry classes with the parts of `GeomEval` they use
//! (`GeomEval_RepCurveDesc`, `GeomEval_RepSurfaceDesc`, `GeomEval_RepUtils`).
//!
//! The class hierarchy is a set of object-safe traits ([`Geometry`], [`Point`], [`Vector`], [`AxisPlacement`],
//! [`Curve`], [`BoundedCurve`], [`Conic`], [`Surface`], [`ElementarySurface`], [`SweptSurface`], [`BoundedSurface`])
//! implemented by one struct per concrete class; the default bodies of the C++ base classes are trait default
//! methods, and an `occ::handle<Geom_Curve>` / `occ::handle<Geom_Surface>` held by another object is an
//! `Arc<dyn Curve>` / `Arc<dyn Surface>`. See `docs/api-geom.md` and the 2D twin [`crate::geom2d`].
#![allow(clippy::float_cmp)]

use crate::bsplclib::BSplError;
use crate::gp::{self, Ax1, Ax2, Ax3, GTrsf2d, Pnt, Trsf};
use crate::mathlin::MathError;
use crate::plib::PLibError;
use std::any::Any;

mod axis;
mod bezier;
mod bezsurf;
mod bspline;
mod bsplsurf;
mod conics;
mod elementary;
mod line;
mod offset;
mod offsetsurf;
mod osculating;
mod point;
pub mod rep_utils;
mod rtrimmed;
mod swept;
mod transformation;
mod trimmed;
mod vector;

pub use crate::geom2d::BSplKnotDistribution;
pub use crate::plib::GeomAbsShape;
pub use axis::{Axis1Placement, Axis2Placement};
pub use bezier::BezierCurve;
pub use bezsurf::BezierSurface;
pub use bspline::BSplineCurve;
pub use bsplsurf::BSplineSurface;
pub use conics::{Circle, Ellipse, Hyperbola, Parabola};
pub use elementary::{ConicalSurface, CylindricalSurface, Plane, SphericalSurface, ToroidalSurface};
pub use line::Line;
pub use offset::OffsetCurve;
pub use offsetsurf::OffsetSurface;
pub use point::CartesianPoint;
pub use rep_utils::{EvalRepDesc, EvalRepKind, SurfaceEvalRepDesc};
pub use rtrimmed::RectangularTrimmedSurface;
pub use swept::{SurfaceOfLinearExtrusion, SurfaceOfRevolution};
pub use transformation::Transformation;
pub use trimmed::TrimmedCurve;
pub use vector::{Direction, VectorWithMagnitude};

/// The error type of `Geom`: the exceptions OCCT raises and the checks OCCT 8.0.1 as built does not make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeomError {
    /// `Standard_ConstructionError`.
    Construction,
    /// `Standard_OutOfRange`, and a Raise_if range check compiled out.
    OutOfRange,
    /// `Standard_DomainError`.
    Domain,
    /// `Standard_NoSuchObject`.
    NoSuchObject,
    /// `Standard_RangeError`.
    Range,
    /// `Geom_UndefinedValue`.
    UndefinedValue,
    /// `Geom_UndefinedDerivative`.
    UndefinedDerivative,
    /// `Standard_NotImplemented`.
    NotImplemented,
    /// `Standard_ProgramError`.
    Program,
    /// `Standard_DimensionError`.
    Dimension,
    /// A wrong shape of a Rust argument, or an unchecked input of OCCT 8.0.1 as built.
    Argument,
    /// An error of `gp`.
    Gp(gp::ConstructionError),
    /// An error of `bsplclib`.
    BSpl(BSplError),
    /// An error of `mathlin`.
    Math(MathError),
    /// An error of `plib`.
    PLib(PLibError),
}

impl From<gp::ConstructionError> for GeomError {
    fn from(e: gp::ConstructionError) -> Self {
        GeomError::Gp(e)
    }
}
impl From<BSplError> for GeomError {
    fn from(e: BSplError) -> Self {
        GeomError::BSpl(e)
    }
}
impl From<MathError> for GeomError {
    fn from(e: MathError) -> Self {
        GeomError::Math(e)
    }
}
impl From<PLibError> for GeomError {
    fn from(e: PLibError) -> Self {
        GeomError::PLib(e)
    }
}

/// The result type of `Geom`.
pub type Result<T> = std::result::Result<T, GeomError>;

/// `Geom_Curve::ResD1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResD1 {
    /// The point.
    pub point: Pnt,
    /// The first derivative.
    pub d1: gp::Vec,
}
/// `Geom_Curve::ResD2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResD2 {
    /// The point.
    pub point: Pnt,
    /// The first derivative.
    pub d1: gp::Vec,
    /// The second derivative.
    pub d2: gp::Vec,
}
/// `Geom_Curve::ResD3`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResD3 {
    /// The point.
    pub point: Pnt,
    /// The first derivative.
    pub d1: gp::Vec,
    /// The second derivative.
    pub d2: gp::Vec,
    /// The third derivative.
    pub d3: gp::Vec,
}
/// `Geom_Surface::ResD1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfResD1 {
    /// The point.
    pub point: Pnt,
    /// The first derivative in U.
    pub d1u: gp::Vec,
    /// The first derivative in V.
    pub d1v: gp::Vec,
}
/// `Geom_Surface::ResD2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfResD2 {
    /// The point.
    pub point: Pnt,
    /// The first derivative in U.
    pub d1u: gp::Vec,
    /// The first derivative in V.
    pub d1v: gp::Vec,
    /// The second derivative in U.
    pub d2u: gp::Vec,
    /// The second derivative in V.
    pub d2v: gp::Vec,
    /// The mixed second derivative.
    pub d2uv: gp::Vec,
}
/// `Geom_Surface::ResD3`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfResD3 {
    /// The point.
    pub point: Pnt,
    /// The first derivative in U.
    pub d1u: gp::Vec,
    /// The first derivative in V.
    pub d1v: gp::Vec,
    /// The second derivative in U.
    pub d2u: gp::Vec,
    /// The second derivative in V.
    pub d2v: gp::Vec,
    /// The mixed second derivative.
    pub d2uv: gp::Vec,
    /// The third derivative in U.
    pub d3u: gp::Vec,
    /// The third derivative in V.
    pub d3v: gp::Vec,
    /// `d3/du2dv`.
    pub d3uuv: gp::Vec,
    /// `d3/dudv2`.
    pub d3uvv: gp::Vec,
}

// Precision
pub(crate) use crate::geom2d::{angular, computational, confusion, is_infinite, pconfusion};
pub(crate) const PRECISION_INFINITE: f64 = 2.0e100;
pub(crate) const M_PI: f64 = std::f64::consts::PI;

/// `Geom_Geometry`.
pub trait Geometry: Any + std::fmt::Debug + Send + Sync {
    /// The object as `Any` (for `DownCast`).
    fn as_any(&self) -> &dyn Any;
    /// The object as mutable `Any`.
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// `Transform`.
    fn transform(&mut self, t: &Trsf);
    /// `Copy`.
    fn copy_geometry(&self) -> Box<dyn Geometry>;

    /// `Mirror(gp_Pnt)`.
    fn mirror_point(&mut self, p: &Pnt) {
        let mut t = Trsf::new();
        t.set_mirror(p);
        self.transform(&t);
    }
    /// `Mirror(gp_Ax1)`.
    fn mirror_axis1(&mut self, a: &Ax1) {
        let mut t = Trsf::new();
        t.set_mirror(a);
        self.transform(&t);
    }
    /// `Mirror(gp_Ax2)`.
    fn mirror_axis2(&mut self, a: &Ax2) {
        let mut t = Trsf::new();
        t.set_mirror(a);
        self.transform(&t);
    }
    /// `Rotate`.
    fn rotate(&mut self, a: &Ax1, ang: f64) {
        let mut t = Trsf::new();
        if t.set_rotation(a, ang).is_ok() {
            self.transform(&t);
        }
    }
    /// `Scale`.
    fn scale(&mut self, p: &Pnt, s: f64) {
        let mut t = Trsf::new();
        if t.set_scale(p, s).is_ok() {
            self.transform(&t);
        }
    }
    /// `Translate(gp_Vec)`.
    fn translate(&mut self, v: &gp::Vec) {
        let mut t = Trsf::new();
        t.set_translation(v);
        self.transform(&t);
    }
    /// `Translate(gp_Pnt, gp_Pnt)`.
    fn translate_points(&mut self, p1: &Pnt, p2: &Pnt) {
        let v = gp::Vec::between(p1, p2);
        self.translate(&v);
    }
    /// `Mirrored(gp_Pnt)`.
    fn mirrored_point(&self, p: &Pnt) -> Box<dyn Geometry> {
        let mut g = self.copy_geometry();
        g.mirror_point(p);
        g
    }
    /// `Mirrored(gp_Ax1)`.
    fn mirrored_axis1(&self, a: &Ax1) -> Box<dyn Geometry> {
        let mut g = self.copy_geometry();
        g.mirror_axis1(a);
        g
    }
    /// `Mirrored(gp_Ax2)`.
    fn mirrored_axis2(&self, a: &Ax2) -> Box<dyn Geometry> {
        let mut g = self.copy_geometry();
        g.mirror_axis2(a);
        g
    }
    /// `Rotated`.
    fn rotated(&self, a: &Ax1, ang: f64) -> Box<dyn Geometry> {
        let mut g = self.copy_geometry();
        g.rotate(a, ang);
        g
    }
    /// `Scaled`.
    fn scaled(&self, p: &Pnt, s: f64) -> Box<dyn Geometry> {
        let mut g = self.copy_geometry();
        g.scale(p, s);
        g
    }
    /// `Transformed`.
    fn transformed(&self, t: &Trsf) -> Box<dyn Geometry> {
        let mut g = self.copy_geometry();
        g.transform(t);
        g
    }
    /// `Translated(gp_Vec)`.
    fn translated(&self, v: &gp::Vec) -> Box<dyn Geometry> {
        let mut g = self.copy_geometry();
        g.translate(v);
        g
    }
    /// `Translated(gp_Pnt, gp_Pnt)`.
    fn translated_points(&self, p1: &Pnt, p2: &Pnt) -> Box<dyn Geometry> {
        let mut g = self.copy_geometry();
        g.translate_points(p1, p2);
        g
    }
}

/// `Geom_Point`.
pub trait Point: Geometry {
    /// `Coord`.
    fn coord(&self) -> (f64, f64, f64);
    /// `Pnt`.
    fn pnt(&self) -> Pnt;
    /// `X`.
    fn x(&self) -> f64;
    /// `Y`.
    fn y(&self) -> f64;
    /// `Z`.
    fn z(&self) -> f64;
    /// `Distance`.
    fn distance(&self, other: &dyn Point) -> f64 {
        let p1 = self.pnt();
        let p2 = other.pnt();
        p1.distance(&p2)
    }
    /// `SquareDistance`.
    fn square_distance(&self, other: &dyn Point) -> f64 {
        let p1 = self.pnt();
        let p2 = other.pnt();
        p1.square_distance(&p2)
    }
}

/// `Geom_Vector`.
pub trait Vector: Geometry {
    /// `Vec` (the protected `gpVec`).
    fn vec(&self) -> gp::Vec;
    /// Sets the protected `gpVec`.
    fn set_vec_raw(&mut self, v: gp::Vec);
    /// `Copy` as a vector.
    fn copy_vector(&self) -> Box<dyn Vector>;
    /// `Magnitude`.
    fn magnitude(&self) -> f64;
    /// `SquareMagnitude`.
    fn square_magnitude(&self) -> f64;
    /// `Cross`.
    fn cross(&mut self, other: &dyn Vector) -> Result<()>;
    /// `Crossed`.
    fn crossed(&self, other: &dyn Vector) -> Result<Box<dyn Vector>>;
    /// `CrossCross`.
    fn cross_cross(&mut self, v1: &dyn Vector, v2: &dyn Vector) -> Result<()>;
    /// `CrossCrossed`.
    fn cross_crossed(&self, v1: &dyn Vector, v2: &dyn Vector) -> Result<Box<dyn Vector>>;
    /// `Reverse`.
    fn reverse(&mut self) {
        let mut v = self.vec();
        v.reverse();
        self.set_vec_raw(v);
    }
    /// `Reversed`.
    fn reversed(&self) -> Box<dyn Vector> {
        let mut v = self.copy_vector();
        v.reverse();
        v
    }
    /// `Angle` (the check of `gp_Vec::Angle` is compiled out: a vector of magnitude `<= gp::Resolution()` is an error).
    fn angle(&self, other: &dyn Vector) -> Result<f64> {
        let a = self.vec();
        let b = other.vec();
        if a.magnitude() <= gp::resolution() || b.magnitude() <= gp::resolution() {
            return Err(GeomError::Argument);
        }
        Ok(a.angle(&b)?)
    }
    /// `AngleWithRef`.
    fn angle_with_ref(&self, other: &dyn Vector, vref: &dyn Vector) -> Result<f64> {
        let a = self.vec();
        let b = other.vec();
        if a.magnitude() <= gp::resolution() || b.magnitude() <= gp::resolution() {
            return Err(GeomError::Argument);
        }
        Ok(a.angle_with_ref(&b, &vref.vec())?)
    }
    /// `Coord`.
    fn coord(&self) -> (f64, f64, f64) {
        self.vec().coords()
    }
    /// `X`.
    fn x(&self) -> f64 {
        self.vec().x()
    }
    /// `Y`.
    fn y(&self) -> f64 {
        self.vec().y()
    }
    /// `Z`.
    fn z(&self) -> f64 {
        self.vec().z()
    }
    /// `Dot`.
    fn dot(&self, other: &dyn Vector) -> f64 {
        self.vec().dot(&other.vec())
    }
    /// `DotCross`.
    fn dot_cross(&self, v1: &dyn Vector, v2: &dyn Vector) -> f64 {
        self.vec().dot_cross(&v1.vec(), &v2.vec())
    }
}

/// `Geom_AxisPlacement`.
pub trait AxisPlacement: Geometry {
    /// The protected `axis`.
    fn ax1_ref(&self) -> &Ax1;
    /// The protected `axis`, mutable.
    fn ax1_mut(&mut self) -> &mut Ax1;
    /// `Axis`.
    fn axis(&self) -> Ax1 {
        *self.ax1_ref()
    }
    /// `SetAxis`.
    fn set_axis(&mut self, a: Ax1) -> Result<()> {
        *self.ax1_mut() = a;
        Ok(())
    }
    /// `SetDirection`.
    fn set_direction(&mut self, v: gp::Dir) -> Result<()>;
    /// `SetLocation`.
    fn set_location(&mut self, p: Pnt) {
        self.ax1_mut().set_location(p);
    }
    /// `Angle`.
    fn angle(&self, other: &dyn AxisPlacement) -> f64 {
        self.ax1_ref().angle(other.ax1_ref())
    }
    /// `Direction`.
    fn direction(&self) -> gp::Dir {
        *self.ax1_ref().direction()
    }
    /// `Location`.
    fn location(&self) -> Pnt {
        *self.ax1_ref().location()
    }
}

/// `Geom_Curve`.
pub trait Curve: Geometry {
    /// `Copy` as a curve.
    fn copy_curve(&self) -> Box<dyn Curve>;
    /// `Reverse`.
    fn reverse(&mut self);
    /// `ReversedParameter`.
    fn reversed_parameter(&self, u: f64) -> f64;
    /// `TransformedParameter`.
    fn transformed_parameter(&self, u: f64, _t: &Trsf) -> f64 {
        u
    }
    /// `ParametricTransformation`.
    fn parametric_transformation(&self, _t: &Trsf) -> f64 {
        1.0
    }
    /// `Reversed`.
    fn reversed(&self) -> Box<dyn Curve> {
        let mut c = self.copy_curve();
        c.reverse();
        c
    }
    /// `FirstParameter`.
    fn first_parameter(&self) -> f64;
    /// `LastParameter`.
    fn last_parameter(&self) -> f64;
    /// `IsClosed`.
    fn is_closed(&self) -> bool;
    /// `IsPeriodic`.
    fn is_periodic(&self) -> bool;
    /// `Period`.
    fn period(&self) -> Result<f64> {
        if !self.is_periodic() {
            return Err(GeomError::NoSuchObject);
        }
        Ok(self.last_parameter() - self.first_parameter())
    }
    /// `Continuity`.
    fn continuity(&self) -> GeomAbsShape;
    /// `IsCN`.
    fn is_cn(&self, n: i32) -> Result<bool>;
    /// `EvalD0`.
    fn eval_d0(&self, u: f64) -> Result<Pnt>;
    /// `EvalD1`.
    fn eval_d1(&self, u: f64) -> Result<ResD1>;
    /// `EvalD2`.
    fn eval_d2(&self, u: f64) -> Result<ResD2>;
    /// `EvalD3`.
    fn eval_d3(&self, u: f64) -> Result<ResD3>;
    /// `EvalDN`.
    fn eval_dn(&self, u: f64, n: i32) -> Result<gp::Vec>;
    /// `Value`.
    fn value(&self, u: f64) -> Result<Pnt> {
        self.eval_d0(u)
    }
}

/// `Geom_BoundedCurve`.
pub trait BoundedCurve: Curve {
    /// `StartPoint`.
    fn start_point(&self) -> Pnt;
    /// `EndPoint`.
    fn end_point(&self) -> Pnt;
}

/// `Geom_Conic`.
pub trait Conic: Curve {
    /// The protected `pos`.
    fn pos(&self) -> &Ax2;
    /// The protected `pos`, mutable.
    fn pos_mut(&mut self) -> &mut Ax2;
    /// `Eccentricity`.
    fn eccentricity(&self) -> Result<f64>;
    /// `SetAxis`.
    fn set_axis(&mut self, a: Ax1) -> Result<()> {
        self.pos_mut().set_axis(a)?;
        Ok(())
    }
    /// `SetLocation`.
    fn set_location(&mut self, p: Pnt) {
        self.pos_mut().set_location(p);
    }
    /// `SetPosition`.
    fn set_position(&mut self, a: Ax2) {
        *self.pos_mut() = a;
    }
    /// `Axis`.
    fn axis(&self) -> Ax1 {
        self.pos().axis()
    }
    /// `Location`.
    fn location(&self) -> Pnt {
        *self.pos().location()
    }
    /// `Position`.
    fn position(&self) -> &Ax2 {
        self.pos()
    }
    /// `XAxis`.
    fn x_axis(&self) -> Ax1 {
        Ax1::new(*self.pos().location(), *self.pos().x_direction())
    }
    /// `YAxis`.
    fn y_axis(&self) -> Ax1 {
        Ax1::new(*self.pos().location(), *self.pos().y_direction())
    }
}

/// `Geom_Conic::Reverse`.
pub(crate) fn conic_reverse(pos: &mut Ax2) {
    let mut vz = *pos.direction();
    vz.reverse();
    // gp_Ax2::SetDirection: a direction parallel to the x direction is kept valid by the double cross product.
    let _ = pos.set_direction(vz);
}

/// `Geom_Surface`.
pub trait Surface: Geometry {
    /// `Copy` as a surface.
    fn copy_surface(&self) -> Box<dyn Surface>;
    /// `UReverse`.
    fn u_reverse(&mut self);
    /// `UReversed`.
    fn u_reversed(&self) -> Box<dyn Surface> {
        let mut s = self.copy_surface();
        s.u_reverse();
        s
    }
    /// `UReversedParameter`.
    fn u_reversed_parameter(&self, u: f64) -> f64;
    /// `VReverse`.
    fn v_reverse(&mut self);
    /// `VReversed`.
    fn v_reversed(&self) -> Box<dyn Surface> {
        let mut s = self.copy_surface();
        s.v_reverse();
        s
    }
    /// `VReversedParameter`.
    fn v_reversed_parameter(&self, v: f64) -> f64;
    /// `TransformParameters`.
    fn transform_parameters(&self, u: f64, v: f64, _t: &Trsf) -> (f64, f64) {
        (u, v)
    }
    /// `ParametricTransformation`.
    fn parametric_transformation(&self, _t: &Trsf) -> GTrsf2d {
        GTrsf2d::new()
    }
    /// `Bounds`.
    fn bounds(&self) -> (f64, f64, f64, f64);
    /// `IsUClosed`.
    fn is_u_closed(&self) -> bool;
    /// `IsVClosed`.
    fn is_v_closed(&self) -> bool;
    /// `IsUPeriodic`.
    fn is_u_periodic(&self) -> bool;
    /// `UPeriod`.
    fn u_period(&self) -> Result<f64> {
        if !self.is_u_periodic() {
            return Err(GeomError::NoSuchObject);
        }
        let (u1, u2, _, _) = self.bounds();
        Ok(u2 - u1)
    }
    /// `IsVPeriodic`.
    fn is_v_periodic(&self) -> bool;
    /// `VPeriod`.
    fn v_period(&self) -> Result<f64> {
        if !self.is_v_periodic() {
            return Err(GeomError::NoSuchObject);
        }
        let (_, _, v1, v2) = self.bounds();
        Ok(v2 - v1)
    }
    /// `UIso`.
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>>;
    /// `VIso`.
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>>;
    /// `Continuity`.
    fn continuity(&self) -> GeomAbsShape;
    /// `IsCNu`.
    fn is_cn_u(&self, n: i32) -> Result<bool>;
    /// `IsCNv`.
    fn is_cn_v(&self, n: i32) -> Result<bool>;
    /// `EvalD0`.
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt>;
    /// `EvalD1`.
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1>;
    /// `EvalD2`.
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2>;
    /// `EvalD3`.
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3>;
    /// `EvalDN`.
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<gp::Vec>;
    /// `Value`.
    fn value(&self, u: f64, v: f64) -> Result<Pnt> {
        self.eval_d0(u, v)
    }
}

/// `Geom_ElementarySurface`.
pub trait ElementarySurface: Surface {
    /// The protected `pos`.
    fn pos(&self) -> &Ax3;
    /// The protected `pos`, mutable.
    fn pos_mut(&mut self) -> &mut Ax3;
    /// `SetAxis`.
    fn set_axis(&mut self, a: Ax1) -> Result<()> {
        self.pos_mut().set_axis(a)?;
        Ok(())
    }
    /// `SetLocation`.
    fn set_location(&mut self, p: Pnt) {
        self.pos_mut().set_location(p);
    }
    /// `SetPosition`.
    fn set_position(&mut self, a: Ax3) {
        *self.pos_mut() = a;
    }
    /// `Axis`.
    fn axis(&self) -> Ax1 {
        self.pos().axis()
    }
    /// `Location`.
    fn location(&self) -> Pnt {
        *self.pos().location()
    }
    /// `Position`.
    fn position(&self) -> &Ax3 {
        self.pos()
    }
}

/// `Geom_SweptSurface`.
pub trait SweptSurface: Surface {
    /// `Direction`.
    fn direction(&self) -> gp::Dir;
    /// `BasisCurve`.
    fn basis_curve(&self) -> std::sync::Arc<dyn Curve>;
}

/// `Geom_BoundedSurface`.
pub trait BoundedSurface: Surface {}

// `NCollection_Array2` helpers: the surface pole arrays are `Vec<Vec<T>>`, one entry per U index.
pub(crate) fn to_array2<T: Clone>(v: &[Vec<T>]) -> Result<crate::plib::Array2<T>> {
    let rows = v.len();
    let cols = v.first().map_or(0, Vec::len);
    if v.iter().any(|r| r.len() != cols) {
        return Err(GeomError::Construction);
    }
    let data: Vec<T> = v.iter().flat_map(|r| r.iter().cloned()).collect();
    Ok(crate::plib::Array2::new(rows, cols, data)?)
}

pub(crate) fn from_array2<T: Clone>(a: &crate::plib::Array2<T>) -> Vec<Vec<T>> {
    let cols = a.cols();
    if cols == 0 {
        return vec![Vec::new(); a.rows()];
    }
    a.as_slice().chunks(cols).map(<[T]>::to_vec).collect()
}

macro_rules! geometry_any {
    () => {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    };
}
pub(crate) use geometry_any;
