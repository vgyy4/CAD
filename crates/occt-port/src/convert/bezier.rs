//! `Convert_CompBezierCurvesToBSplineCurveBase<PointType, VecType>` with its two instances
//! `Convert_CompBezierCurvesToBSplineCurve` (`gp_Pnt`, `gp_Vec`) and `Convert_CompBezierCurves2dToBSplineCurve2d`
//! (`gp_Pnt2d`, `gp_Vec2d`).

use super::{ConvertError, R, at};
use crate::bsplclib::{self, util::epsilon};
use crate::gp::{self, Pnt, Pnt2d, Vec as Vec3, Vec2d};

/// What the template needs of `PointType` and `VecType`.
trait BezierPoint: Copy {
    /// The number of coordinates.
    const DIM: usize;
    /// The 3D-specific epsilon guard on the accumulated knot values.
    const EPSILON_GUARD: bool;
    fn coords(&self, out: &mut Vec<f64>);
    fn from_coords(c: &[f64]) -> Option<Self>;
    /// `(D1, D2)`, the squared lengths of `P1P2` and `P2P3`, when both exceed `gp::Resolution()` and the two vectors
    /// are parallel within `angular`.
    fn junction(p1: &Self, p2: &Self, p3: &Self, angular: f64) -> R<Option<(f64, f64)>>;
}

impl BezierPoint for Pnt {
    const DIM: usize = 3;
    const EPSILON_GUARD: bool = true;
    fn coords(&self, out: &mut Vec<f64>) {
        out.extend([self.x(), self.y(), self.z()]);
    }
    fn from_coords(c: &[f64]) -> Option<Self> {
        if let [x, y, z] = c { Some(Pnt::new(*x, *y, *z)) } else { None }
    }
    fn junction(p1: &Self, p2: &Self, p3: &Self, angular: f64) -> R<Option<(f64, f64)>> {
        let v1 = Vec3::between(p1, p2);
        let v2 = Vec3::between(p2, p3);
        let d1 = v1.square_magnitude();
        let d2 = v2.square_magnitude();
        if d1 > gp::resolution() && d2 > gp::resolution() && v1.is_parallel(&v2, angular)? {
            Ok(Some((d1, d2)))
        } else {
            Ok(None)
        }
    }
}

impl BezierPoint for Pnt2d {
    const DIM: usize = 2;
    const EPSILON_GUARD: bool = false;
    fn coords(&self, out: &mut Vec<f64>) {
        out.extend([self.x(), self.y()]);
    }
    fn from_coords(c: &[f64]) -> Option<Self> {
        if let [x, y] = c { Some(Pnt2d::new(*x, *y)) } else { None }
    }
    fn junction(p1: &Self, p2: &Self, p3: &Self, angular: f64) -> R<Option<(f64, f64)>> {
        let v1 = Vec2d::between(p1, p2);
        let v2 = Vec2d::between(p2, p3);
        let d1 = v1.square_magnitude();
        let d2 = v2.square_magnitude();
        if d1 > gp::resolution() && d2 > gp::resolution() && v1.is_parallel(&v2, angular)? {
            Ok(Some((d1, d2)))
        } else {
            Ok(None)
        }
    }
}

/// The template base class.
#[derive(Debug, Clone)]
struct Base<P: BezierPoint> {
    sequence: Vec<Vec<P>>,
    curve_poles: Vec<P>,
    curve_knots: Vec<f64>,
    knots_mults: Vec<i32>,
    degree: i32,
    angular: f64,
}

impl<P: BezierPoint> Base<P> {
    fn new(angular_tolerance: f64) -> Self {
        Self {
            sequence: Vec::new(),
            curve_poles: Vec::new(),
            curve_knots: Vec::new(),
            knots_mults: Vec::new(),
            degree: 0,
            angular: angular_tolerance,
        }
    }

    fn add_curve(&mut self, poles: &[P]) -> R<()> {
        if poles.len() < 2 {
            return Err(ConvertError::Argument);
        }
        self.sequence.push(poles.to_vec());
        Ok(())
    }

    /// The poles of `curve` raised to `degree` (`BSplCLib::IncreaseDegree`, Bezier syntax).
    fn raised(curve: &[P], degree: i32) -> R<Vec<P>> {
        let own = i32::try_from(curve.len()).map_err(|_| ConvertError::Argument)? - 1;
        let mut flat = Vec::with_capacity(curve.len() * P::DIM);
        for p in curve {
            p.coords(&mut flat);
        }
        let dim = i32::try_from(P::DIM).map_err(|_| ConvertError::Argument)?;
        let (new_flat, _, _) =
            bsplclib::increase_degree(own, degree, false, dim, &flat, &[0.0, 1.0], &[own + 1, own + 1])?;
        let points: Vec<P> = new_flat.chunks(P::DIM).filter_map(P::from_coords).collect();
        if i64::try_from(points.len()).ok() != Some(i64::from(degree) + 1) {
            return Err(ConvertError::OutOfRange);
        }
        Ok(points)
    }

    fn perform(&mut self) -> R<()> {
        self.curve_poles.clear();
        self.curve_knots.clear();
        self.knots_mults.clear();
        if self.sequence.is_empty() {
            return Ok(());
        }
        let nb_curv = self.sequence.len();
        let upper = i64::try_from(nb_curv).map_err(|_| ConvertError::Argument)?;
        let mut curve_kn_vals = vec![0.0; nb_curv];

        self.degree = 0;
        for curve in &self.sequence {
            let d = i32::try_from(curve.len()).map_err(|_| ConvertError::Argument)? - 1;
            self.degree = self.degree.max(d);
        }

        let mut det = 0.0;
        let mut p1: Option<P> = None;
        let max_degree = self.degree;
        let md = i64::from(max_degree);
        for i in 1..=upper {
            let curve = self
                .sequence
                .get(usize::try_from(i - 1).map_err(|_| ConvertError::Argument)?)
                .ok_or(ConvertError::OutOfRange)?;
            // 1- Raise the Bezier curve to the maximum degree.
            let a_deg = i32::try_from(curve.len()).map_err(|_| ConvertError::Argument)? - 1;
            let points = if self.degree - a_deg > 0 { Self::raised(curve, self.degree)? } else { curve.clone() };

            // 2- Process the node of junction between 2 Bezier curves.
            if i == 1 {
                // Processing of the initial node of the BSpline.
                for j in 1..=md {
                    self.curve_poles.push(at(&points, j)?);
                }
                super::put(&mut curve_kn_vals, 1, 1.0)?; // To begin the series.
                self.knots_mults.push(max_degree + 1);
                det = 1.0;
            }

            if i != 1 {
                let p2 = at(&points, 1)?;
                let p3 = at(&points, 2)?;
                let prev = p1.ok_or(ConvertError::OutOfRange)?;
                // Processing of the tangency between Bezier and the previous. This allows to guarantee at least a C1
                // continuity if the tangents are coherent.
                let tangent = if max_degree > 1 { P::junction(&prev, &p2, &p3, self.angular)? } else { None };
                let before = at(&curve_kn_vals, i - 1)?;
                match tangent {
                    Some((d1, d2)) => {
                        let lambda = (d2 / d1).sqrt();
                        if !P::EPSILON_GUARD || before * lambda > 10.0 * epsilon(det) {
                            self.knots_mults.push(max_degree - 1);
                            super::put(&mut curve_kn_vals, i, before * lambda)?;
                        } else {
                            self.curve_poles.push(at(&points, 1)?);
                            self.knots_mults.push(max_degree);
                            super::put(&mut curve_kn_vals, i, 1.0)?;
                        }
                    }
                    None => {
                        self.curve_poles.push(at(&points, 1)?);
                        self.knots_mults.push(max_degree);
                        super::put(&mut curve_kn_vals, i, 1.0)?;
                    }
                }
                det += at(&curve_kn_vals, i)?;

                // Store the poles.
                for j in 2..=md {
                    self.curve_poles.push(at(&points, j)?);
                }
            }

            if i == upper {
                // Processing of the end node of the BSpline.
                self.curve_poles.push(at(&points, md + 1)?);
                self.knots_mults.push(max_degree + 1);
            }
            p1 = Some(at(&points, md)?);
        }

        // Correct nodal values to make them variable within [0.,1.].
        self.curve_knots.push(0.0);
        for i in 2..=upper {
            let last = at(&self.curve_knots, i - 1)?;
            self.curve_knots.push(last + (at(&curve_kn_vals, i - 1)? / det));
        }
        self.curve_knots.push(1.0);
        Ok(())
    }
}

macro_rules! comp_bezier {
    ($name:ident, $pt:ty, $what:literal) => {
        #[doc = $what]
        #[derive(Debug, Clone)]
        pub struct $name(Base<$pt>);

        impl Default for $name {
            fn default() -> Self {
                Self::new(1.0e-4)
            }
        }

        impl $name {
            /// A framework for converting a sequence of adjacent non-rational Bezier curves into a BSpline curve;
            /// `angular_tolerance` (radians, 1e-4 in C++) is used to check the parallelism of the tangents at the
            /// junction points.
            pub fn new(angular_tolerance: f64) -> Self {
                Self(Base::new(angular_tolerance))
            }

            /// Adds the Bezier curve defined by its poles to the sequence. `Err(Argument)` for fewer than 2 poles.
            pub fn add_curve(&mut self, poles: &[$pt]) -> Result<(), ConvertError> {
                self.0.add_curve(poles)
            }

            /// Computes all the data needed to build a BSpline curve equivalent to the adjacent Bezier curves.
            pub fn perform(&mut self) -> Result<(), ConvertError> {
                self.0.perform()
            }

            /// The degree of the BSpline curve (0 before `perform` or without curves).
            pub fn degree(&self) -> i32 {
                self.0.degree
            }

            /// The number of poles of the BSpline curve.
            pub fn nb_poles(&self) -> i32 {
                i32::try_from(self.0.curve_poles.len()).unwrap_or(i32::MAX)
            }

            /// The poles of the BSpline curve.
            pub fn poles(&self) -> Vec<$pt> {
                self.0.curve_poles.clone()
            }

            /// The number of knots of the BSpline curve.
            pub fn nb_knots(&self) -> i32 {
                i32::try_from(self.0.curve_knots.len()).unwrap_or(i32::MAX)
            }

            /// The knots and the corresponding multiplicities of the BSpline curve.
            pub fn knots_and_mults(&self) -> (Vec<f64>, Vec<i32>) {
                (self.0.curve_knots.clone(), self.0.knots_mults.clone())
            }
        }
    };
}

comp_bezier!(
    CompBezierCurvesToBSplineCurve,
    Pnt,
    "`Convert_CompBezierCurvesToBSplineCurve`: converts a sequence of adjacent non-rational Bezier curves into a BSpline curve."
);
comp_bezier!(
    CompBezierCurves2dToBSplineCurve2d,
    Pnt2d,
    "`Convert_CompBezierCurves2dToBSplineCurve2d`: the 2D version of `CompBezierCurvesToBSplineCurve`."
);
