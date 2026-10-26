//! OCCT's `Geom2d` package (toolkit TKG2d): the 2D geometry classes with the parts of `Geom2dEval` they use
//! (`Geom2dEval_RepCurveDesc`, `Geom2dEval_RepUtils`).
//!
//! The class hierarchy is a set of object-safe traits ([`Geometry2d`], [`Point2d`], [`Vector2d`], [`Curve2d`],
//! [`BoundedCurve2d`], [`Conic2d`]) implemented by one struct per concrete class; the default bodies of the C++ base
//! classes are trait default methods, and an `occ::handle<Geom2d_Curve>` held by another curve is an
//! `Arc<dyn Curve2d>`. See `docs/api-geom2d.md`.
#![allow(clippy::float_cmp)]

use crate::bsplclib::BSplError;
use crate::gp::{self, Ax2d, Ax22d, Pnt2d, Trsf2d, Vec2d};
use crate::mathlin::MathError;
use crate::plib::PLibError;
use std::any::Any;

mod axis;
mod bezier;
mod bspline;
mod conics;
mod line;
mod offset;
mod point;
pub mod rep_utils;
mod trimmed;
mod vector;

pub use crate::plib::GeomAbsShape;
pub use axis::AxisPlacement;
pub use bezier::BezierCurve;
pub use bspline::BSplineCurve;
pub use conics::{Circle, Ellipse, Hyperbola, Parabola};
pub use line::Line;
pub use offset::OffsetCurve;
pub use point::CartesianPoint;
pub use rep_utils::{EvalRepDesc, EvalRepKind};
pub use trimmed::TrimmedCurve;
pub use vector::{Direction, VectorWithMagnitude};

mod transformation;
pub use transformation::Transformation;

/// The error type of `Geom2d`: the exceptions OCCT raises and the checks OCCT 8.0.1 as built does not make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Geom2dError {
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
    /// `Geom2d_UndefinedValue`.
    UndefinedValue,
    /// `Geom2d_UndefinedDerivative`.
    UndefinedDerivative,
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

impl From<gp::ConstructionError> for Geom2dError {
    fn from(e: gp::ConstructionError) -> Self {
        Geom2dError::Gp(e)
    }
}
impl From<BSplError> for Geom2dError {
    fn from(e: BSplError) -> Self {
        Geom2dError::BSpl(e)
    }
}
impl From<MathError> for Geom2dError {
    fn from(e: MathError) -> Self {
        Geom2dError::Math(e)
    }
}
impl From<PLibError> for Geom2dError {
    fn from(e: PLibError) -> Self {
        Geom2dError::PLib(e)
    }
}

/// The result type of `Geom2d`.
pub type Result<T> = std::result::Result<T, Geom2dError>;

pub use crate::bsplclib::curve2d::BSplKnotDistribution;

/// `Geom2d_Curve::ResD1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResD1 {
    /// The point.
    pub point: Pnt2d,
    /// The first derivative.
    pub d1: Vec2d,
}
/// `Geom2d_Curve::ResD2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResD2 {
    /// The point.
    pub point: Pnt2d,
    /// The first derivative.
    pub d1: Vec2d,
    /// The second derivative.
    pub d2: Vec2d,
}
/// `Geom2d_Curve::ResD3`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResD3 {
    /// The point.
    pub point: Pnt2d,
    /// The first derivative.
    pub d1: Vec2d,
    /// The second derivative.
    pub d2: Vec2d,
    /// The third derivative.
    pub d3: Vec2d,
}

// Precision
pub(crate) const PRECISION_INFINITE: f64 = 2.0e100;
pub(crate) fn confusion() -> f64 {
    1.0e-7
}
pub(crate) fn pconfusion() -> f64 {
    confusion() / 100.0
}
pub(crate) fn angular() -> f64 {
    1.0e-12
}
pub(crate) fn computational() -> f64 {
    f64::EPSILON
}
pub(crate) fn is_infinite(u: f64) -> bool {
    u.abs() >= PRECISION_INFINITE
}
pub(crate) const M_PI: f64 = std::f64::consts::PI;

/// `Geom2d_Geometry`.
pub trait Geometry2d: Any + std::fmt::Debug + Send + Sync {
    /// The object as `Any` (for `DownCast`).
    fn as_any(&self) -> &dyn Any;
    /// The object as mutable `Any`.
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// `Transform`.
    fn transform(&mut self, t: &Trsf2d);
    /// `Copy`.
    fn copy_geometry(&self) -> Box<dyn Geometry2d>;

    /// `Mirror(gp_Pnt2d)`.
    fn mirror_point(&mut self, p: &Pnt2d) {
        let mut t = Trsf2d::new();
        t.set_mirror(p);
        self.transform(&t);
    }
    /// `Mirror(gp_Ax2d)`.
    fn mirror_axis(&mut self, a: &Ax2d) {
        let mut t = Trsf2d::new();
        t.set_mirror(a);
        self.transform(&t);
    }
    /// `Rotate`.
    fn rotate(&mut self, p: &Pnt2d, ang: f64) {
        let mut t = Trsf2d::new();
        t.set_rotation(p, ang);
        self.transform(&t);
    }
    /// `Scale`.
    fn scale(&mut self, p: &Pnt2d, s: f64) {
        let mut t = Trsf2d::new();
        t.set_scale(p, s);
        self.transform(&t);
    }
    /// `Translate(gp_Vec2d)`.
    fn translate(&mut self, v: &Vec2d) {
        let mut t = Trsf2d::new();
        t.set_translation(v);
        self.transform(&t);
    }
    /// `Translate(gp_Pnt2d, gp_Pnt2d)`.
    fn translate_points(&mut self, p1: &Pnt2d, p2: &Pnt2d) {
        let v = Vec2d::between(p1, p2);
        self.translate(&v);
    }
    /// `Mirrored(gp_Pnt2d)`.
    fn mirrored_point(&self, p: &Pnt2d) -> Box<dyn Geometry2d> {
        let mut g = self.copy_geometry();
        g.mirror_point(p);
        g
    }
    /// `Mirrored(gp_Ax2d)`.
    fn mirrored_axis(&self, a: &Ax2d) -> Box<dyn Geometry2d> {
        let mut g = self.copy_geometry();
        g.mirror_axis(a);
        g
    }
    /// `Rotated`.
    fn rotated(&self, p: &Pnt2d, ang: f64) -> Box<dyn Geometry2d> {
        let mut g = self.copy_geometry();
        g.rotate(p, ang);
        g
    }
    /// `Scaled`.
    fn scaled(&self, p: &Pnt2d, s: f64) -> Box<dyn Geometry2d> {
        let mut g = self.copy_geometry();
        g.scale(p, s);
        g
    }
    /// `Transformed`.
    fn transformed(&self, t: &Trsf2d) -> Box<dyn Geometry2d> {
        let mut g = self.copy_geometry();
        g.transform(t);
        g
    }
    /// `Translated(gp_Vec2d)`.
    fn translated(&self, v: &Vec2d) -> Box<dyn Geometry2d> {
        let mut g = self.copy_geometry();
        g.translate(v);
        g
    }
    /// `Translated(gp_Pnt2d, gp_Pnt2d)`.
    fn translated_points(&self, p1: &Pnt2d, p2: &Pnt2d) -> Box<dyn Geometry2d> {
        let mut g = self.copy_geometry();
        g.translate_points(p1, p2);
        g
    }
}

/// `Geom2d_Point`.
pub trait Point2d: Geometry2d {
    /// `Coord`.
    fn coord(&self) -> (f64, f64);
    /// `Pnt2d`.
    fn pnt2d(&self) -> Pnt2d;
    /// `X`.
    fn x(&self) -> f64;
    /// `Y`.
    fn y(&self) -> f64;
    /// `Distance`.
    fn distance(&self, other: &dyn Point2d) -> f64 {
        let p1 = self.pnt2d();
        let p2 = other.pnt2d();
        p1.distance(&p2)
    }
    /// `SquareDistance`.
    fn square_distance(&self, other: &dyn Point2d) -> f64 {
        let p1 = self.pnt2d();
        let p2 = other.pnt2d();
        p1.square_distance(&p2)
    }
}

/// `Geom2d_Vector`.
pub trait Vector2d: Geometry2d {
    /// `Vec2d` (the protected `gpVec2d`).
    fn vec2d(&self) -> Vec2d;
    /// Sets the protected `gpVec2d`.
    fn set_vec2d_raw(&mut self, v: Vec2d);
    /// `Copy` as a vector.
    fn copy_vector(&self) -> Box<dyn Vector2d>;
    /// `Magnitude`.
    fn magnitude(&self) -> f64;
    /// `SquareMagnitude`.
    fn square_magnitude(&self) -> f64;
    /// `Crossed`.
    fn crossed(&self, other: &dyn Vector2d) -> f64;
    /// `Reverse`.
    fn reverse(&mut self) {
        let mut v = self.vec2d();
        v.reverse();
        self.set_vec2d_raw(v);
    }
    /// `Reversed`.
    fn reversed(&self) -> Box<dyn Vector2d> {
        let mut v = self.copy_vector();
        v.reverse();
        v
    }
    /// `Angle`.
    fn angle(&self, other: &dyn Vector2d) -> Result<f64> {
        Ok(self.vec2d().angle(&other.vec2d())?)
    }
    /// `Coord`.
    fn coord(&self) -> (f64, f64) {
        self.vec2d().coords()
    }
    /// `X`.
    fn x(&self) -> f64 {
        self.vec2d().x()
    }
    /// `Y`.
    fn y(&self) -> f64 {
        self.vec2d().y()
    }
    /// `Dot`.
    fn dot(&self, other: &dyn Vector2d) -> f64 {
        self.vec2d().dot(&other.vec2d())
    }
}

/// `Geom2d_Curve`.
pub trait Curve2d: Geometry2d {
    /// `Copy` as a curve.
    fn copy_curve(&self) -> Box<dyn Curve2d>;
    /// `Reverse`.
    fn reverse(&mut self);
    /// `ReversedParameter`.
    fn reversed_parameter(&self, u: f64) -> f64;
    /// `TransformedParameter`.
    fn transformed_parameter(&self, u: f64, _t: &Trsf2d) -> f64 {
        u
    }
    /// `ParametricTransformation`.
    fn parametric_transformation(&self, _t: &Trsf2d) -> f64 {
        1.0
    }
    /// `Reversed`.
    fn reversed(&self) -> Box<dyn Curve2d> {
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
            return Err(Geom2dError::NoSuchObject);
        }
        Ok(self.last_parameter() - self.first_parameter())
    }
    /// `Continuity`.
    fn continuity(&self) -> GeomAbsShape;
    /// `IsCN`.
    fn is_cn(&self, n: i32) -> Result<bool>;
    /// `EvalD0`.
    fn eval_d0(&self, u: f64) -> Result<Pnt2d>;
    /// `EvalD1`.
    fn eval_d1(&self, u: f64) -> Result<ResD1>;
    /// `EvalD2`.
    fn eval_d2(&self, u: f64) -> Result<ResD2>;
    /// `EvalD3`.
    fn eval_d3(&self, u: f64) -> Result<ResD3>;
    /// `EvalDN`.
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d>;
    /// `Value`.
    fn value(&self, u: f64) -> Result<Pnt2d> {
        self.eval_d0(u)
    }
}

/// `Geom2d_BoundedCurve`.
pub trait BoundedCurve2d: Curve2d {
    /// `StartPoint`.
    fn start_point(&self) -> Pnt2d;
    /// `EndPoint`.
    fn end_point(&self) -> Pnt2d;
}

/// `Geom2d_Conic`.
pub trait Conic2d: Curve2d {
    /// The protected `pos`.
    fn pos(&self) -> &Ax22d;
    /// The protected `pos`, mutable.
    fn pos_mut(&mut self) -> &mut Ax22d;
    /// `Eccentricity`.
    fn eccentricity(&self) -> Result<f64>;
    /// `SetAxis`.
    fn set_axis(&mut self, a: Ax22d) {
        self.pos_mut().set_axis(a);
    }
    /// `SetXAxis`.
    fn set_x_axis(&mut self, a: Ax2d) {
        self.pos_mut().set_x_axis(a);
    }
    /// `SetYAxis`.
    fn set_y_axis(&mut self, a: Ax2d) {
        self.pos_mut().set_y_axis(a);
    }
    /// `SetLocation`.
    fn set_location(&mut self, p: Pnt2d) {
        self.pos_mut().set_location(p);
    }
    /// `XAxis`.
    fn x_axis(&self) -> Ax2d {
        Ax2d::new(*self.pos().location(), *self.pos().x_direction())
    }
    /// `YAxis`.
    fn y_axis(&self) -> Ax2d {
        Ax2d::new(*self.pos().location(), *self.pos().y_direction())
    }
    /// `Location`.
    fn location(&self) -> Pnt2d {
        *self.pos().location()
    }
    /// `Position`.
    fn position(&self) -> &Ax22d {
        self.pos()
    }
}

/// `Geom2d_Conic::Reverse`.
pub(crate) fn conic_reverse(pos: &mut Ax22d) {
    let mut temp = *pos.y_direction();
    temp.reverse();
    pos.set_axis(Ax22d::from_dirs(*pos.location(), *pos.x_direction(), temp));
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
