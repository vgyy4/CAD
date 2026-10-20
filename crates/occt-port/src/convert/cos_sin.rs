//! `Convert_ConicToBSplineCurve::BuildCosAndSin` (both overloads) and `Convert_PolynomialCosAndSin`.

use super::conic::ConicToBSplineCurve;
use super::{ConvertError, PCONFUSION, ParameterisationType, R, at, cmax, cmin, nb_spans, put};
use crate::bsplclib::{self, Curve};
use crate::gp::{self, Pnt, Pnt2d, Trsf2d};
use crate::plib;
use std::f64::consts::PI;

/// The result of `BuildCosAndSin`: the B-spline representation of cos and sin as rational functions.
#[derive(Debug, Clone, PartialEq)]
pub struct CosAndSin {
    /// The poles of the cos numerator.
    pub cos_numerator: Vec<f64>,
    /// The poles of the sin numerator.
    pub sin_numerator: Vec<f64>,
    /// The poles (weights) of the denominator.
    pub denominator: Vec<f64>,
    /// The degree.
    pub degree: i32,
    /// The knots.
    pub knots: Vec<f64>,
    /// The multiplicities.
    pub mults: Vec<i32>,
}

/// The `Convert_CosAndSinEvalFunction`s used by `BuildCosAndSin`.
#[derive(Clone, Copy)]
enum Evaluator {
    QuasiAngular,
    RationalC1,
}

/// A 2D point as a 3D one (the evaluation of `BSplCLib::D0` is done coordinate by coordinate).
fn lift(p: &Pnt2d) -> Pnt {
    Pnt::new(p.x(), p.y(), 0.0)
}

/// `BSplCLib::D0(U, Poles, NoWeights, P)` for 2D poles, Bezier syntax (knots 0 and 1, multiplicities `degree + 1`).
fn bezier_d0(u: f64, poles: &[Pnt2d]) -> R<Pnt2d> {
    let degree = i32::try_from(poles.len()).map_err(|_| ConvertError::Argument)? - 1;
    let pts: Vec<Pnt> = poles.iter().map(lift).collect();
    let knots = [0.0, 1.0];
    let mults = [degree + 1, degree + 1];
    let curve = Curve { degree, periodic: false, poles: &pts, weights: None, knots: &knots, mults: &mults };
    let p = bsplclib::d0(u, &curve);
    Ok(Pnt2d::new(p.x(), p.y()))
}

/// `BSplCLib::D0` on scalar poles (with optional weights) of a B-spline given by knots and multiplicities.
fn scalar_d0(u: f64, degree: i32, poles: &[f64], weights: Option<&[f64]>, knots: &[f64], mults: &[i32]) -> f64 {
    let pts: Vec<Pnt> = poles.iter().map(|v| Pnt::new(*v, 0.0, 0.0)).collect();
    let curve = Curve { degree, periodic: false, poles: &pts, weights, knots, mults };
    bsplclib::d0(u, &curve).x()
}

/// `CosAndSinRationalC1` and `CosAndSinQuasiAngular`: `U(t)` and `V(t)` of the temporary representation.
fn evaluate(
    which: Evaluator,
    parameter: f64,
    eval_degree: i32,
    eval_poles: &[Pnt2d],
    eval_knots: &[f64],
    eval_mults: &[i32],
) -> R<[f64; 2]> {
    match which {
        Evaluator::RationalC1 => {
            let pts: Vec<Pnt> = eval_poles.iter().map(lift).collect();
            let curve = Curve {
                degree: eval_degree,
                periodic: false,
                poles: &pts,
                weights: None,
                knots: eval_knots,
                mults: eval_mults,
            };
            let p = bsplclib::d0(parameter, &curve);
            Ok([p.x(), p.y()])
        }
        Evaluator::QuasiAngular => {
            if i64::try_from(eval_poles.len()).ok() != Some(i64::from(eval_degree) + 1) {
                return Err(ConvertError::OutOfRange);
            }
            let mut coeffs = Vec::with_capacity(eval_poles.len() * 2);
            for p in eval_poles {
                coeffs.push(p.x());
                coeffs.push(p.y());
            }
            let param = parameter * 0.5;
            let r = plib::no_derivative_eval_polynomial(param, eval_degree, 2, eval_degree << 1, &coeffs)?;
            match r.as_slice() {
                [a, b] => Ok([*a, *b]),
                _ => Err(ConvertError::OutOfRange),
            }
        }
    }
}

/// `AlgorithmicCosAndSin`: the B-spline representation of an algorithmic description of cos and sin; returns the
/// cos numerator, the sin numerator and the denominator (`num_poles` values each).
#[allow(clippy::too_many_arguments)]
fn algorithmic_cos_and_sin(
    degree: i32,
    flat_knots: &[f64],
    eval_degree: i32,
    eval_poles: &[Pnt2d],
    eval_knots: &[f64],
    eval_mults: &[i32],
    which: Evaluator,
    num_poles: usize,
) -> R<(Vec<f64>, Vec<f64>, Vec<f64>)> {
    let order = degree + 1;
    let n = i64::try_from(flat_knots.len()).map_err(|_| ConvertError::Argument)? - i64::from(order);
    if i64::try_from(num_poles).ok() != Some(n) {
        return Err(ConvertError::Construction);
    }
    let parameters = bsplclib::build_schoenberg_points(
        degree,
        flat_knots,
        i32::try_from(num_poles).map_err(|_| ConvertError::Argument)?,
    )?;
    let mut contact = Vec::with_capacity(num_poles);
    let mut flat_poles = Vec::with_capacity(num_poles * 3);
    for u in &parameters {
        let result = evaluate(which, *u, eval_degree, eval_poles, eval_knots, eval_mults)?;
        contact.push(0);
        flat_poles.push(result[1] * result[1] - result[0] * result[0]);
        flat_poles.push(2.0 * result[1] * result[0]);
        flat_poles.push(result[1] * result[1] + result[0] * result[0]);
    }
    let (_, solved) = bsplclib::interpolate(degree, flat_knots, &parameters, &contact, 3, &flat_poles)?;
    let mut cos = Vec::with_capacity(num_poles);
    let mut sin = Vec::with_capacity(num_poles);
    let mut den = Vec::with_capacity(num_poles);
    for chunk in solved.chunks(3).take(num_poles) {
        if let [p1, p2, p3] = chunk {
            let inverse = 1.0 / *p3;
            cos.push(*p1 * inverse);
            sin.push(*p2 * inverse);
            den.push(*p3);
        }
    }
    if cos.len() != num_poles {
        return Err(ConvertError::OutOfRange);
    }
    Ok((cos, sin, den))
}

impl ConicToBSplineCurve {
    /// `BuildCosAndSin(Parameterisation, UFirst, ULast, CosNumerator, SinNumerator, Denominator, Degree, Knots, Mults)`.
    ///
    /// `Err(Construction)` for `TgtThetaOver2_1` with `delta > 0.9999*PI` and `TgtThetaOver2_2` with
    /// `delta > 1.9999*PI`; `Err(Argument)` for the inputs OCCT 8.0.1 does not check (an empty span count for
    /// `TgtThetaOver2`, `delta <= 0` for `QuasiAngular`, `RationalC1` and `Polynomial`).
    pub fn build_cos_and_sin(p: ParameterisationType, u_first: f64, u_last: f64) -> R<CosAndSin> {
        let delta = u_last - u_first;
        let mut num_poles: i32 = 0;
        let mut degree: i32 = 0;
        let mut num_knots: i32 = 1;
        let num_spans: i32;
        let mut order: i32 = 0;
        let mut tgt_theta = false;
        match p {
            ParameterisationType::TgtThetaOver2 => {
                num_spans = nb_spans(delta)?;
                if num_spans < 1 {
                    return Err(ConvertError::Argument);
                }
                tgt_theta = true;
            }
            ParameterisationType::TgtThetaOver2_1 => {
                num_spans = 1;
                if delta > 0.9999 * PI {
                    return Err(ConvertError::Construction);
                }
                tgt_theta = true;
            }
            ParameterisationType::TgtThetaOver2_2 => {
                num_spans = 2;
                if delta > 1.9999 * PI {
                    return Err(ConvertError::Construction);
                }
                tgt_theta = true;
            }
            ParameterisationType::TgtThetaOver2_3 => {
                num_spans = 3;
                tgt_theta = true;
            }
            ParameterisationType::TgtThetaOver2_4 => {
                num_spans = 4;
                tgt_theta = true;
            }
            ParameterisationType::QuasiAngular => {
                num_poles = 7;
                degree = 6;
                num_spans = 1;
                num_knots = 2;
                order = degree + 1;
            }
            ParameterisationType::RationalC1 => {
                degree = 4;
                order = degree + 1;
                num_poles = 8;
                num_knots = 3;
                num_spans = 2;
            }
            ParameterisationType::Polynomial => {
                degree = 7;
                num_poles = 8;
                num_knots = 2;
                num_spans = 1;
            }
        }
        if !tgt_theta && (delta.is_nan() || delta <= 0.0) {
            return Err(ConvertError::Argument);
        }
        let mut alpha = 0.0;
        if tgt_theta {
            alpha = delta / (2.0 * f64::from(num_spans));
            degree = 2;
            num_poles = 2 * num_spans + 1;
        }
        let np = usize::try_from(num_poles).map_err(|_| ConvertError::Argument)?;
        let nk = usize::try_from(num_spans + 1).map_err(|_| ConvertError::Argument)?;
        let mut cos_num = vec![0.0; np];
        let mut sin_num = vec![0.0; np];
        let mut den = vec![0.0; np];
        let mut knots = vec![0.0; nk];
        let mut mults = vec![0i32; nk];
        if tgt_theta {
            let mut param = u_first;
            put(&mut cos_num, 1, libm::cos(u_first))?;
            put(&mut sin_num, 1, libm::sin(u_first))?;
            put(&mut den, 1, 1.0)?;
            put(&mut knots, 1, param)?;
            put(&mut mults, 1, degree + 1)?;
            let direct = libm::cos(alpha);
            let inverse = 1.0 / direct;
            for ii in 1..=i64::from(num_spans) {
                put(&mut cos_num, 2 * ii, inverse * libm::cos(param + alpha))?;
                put(&mut sin_num, 2 * ii, inverse * libm::sin(param + alpha))?;
                put(&mut den, 2 * ii, direct)?;
                put(&mut cos_num, 2 * ii + 1, libm::cos(param + 2.0 * alpha))?;
                put(&mut sin_num, 2 * ii + 1, libm::sin(param + 2.0 * alpha))?;
                put(&mut den, 2 * ii + 1, 1.0)?;
                put(&mut knots, ii + 1, param + 2.0 * alpha)?;
                put(&mut mults, ii + 1, 2)?;
                param += 2.0 * alpha;
            }
            put(&mut mults, i64::from(num_spans) + 1, degree + 1)?;
        } else if p != ParameterisationType::Polynomial {
            let mut alpha = u_last - u_first;
            alpha *= 0.5;
            let mut beta = u_last + u_first;
            beta *= 0.5;
            let cos_beta = libm::cos(beta);
            let sin_beta = libm::sin(beta);
            let num_flat_knots = num_poles + order;
            let mut flat_knots = vec![0.0; usize::try_from(num_flat_knots).map_err(|_| ConvertError::Argument)?];
            let mut temp_poles = vec![Pnt2d::new(0.0, 0.0); 4];
            let mut temp_knots = vec![0.0; 3];
            let mut temp_mults = vec![0i32; 3];
            for ii in 1..=i64::from(order) {
                put(&mut flat_knots, ii, -alpha)?;
                put(&mut flat_knots, ii + i64::from(num_poles), alpha)?;
            }
            put(&mut knots, 1, u_first)?;
            put(&mut knots, i64::from(num_knots), u_last)?;
            put(&mut mults, 1, order)?;
            put(&mut mults, i64::from(num_knots), order)?;
            let temp_degree;
            let which;
            if p == ParameterisationType::QuasiAngular {
                // V(t) = t + c t^3 is coded in the first coordinate, U(t) = 1 + b t^2 in the second one
                let alpha_2 = alpha * 0.5;
                let mut p_param = -1.0 / (alpha_2 * alpha_2);
                if alpha_2 < PI * 0.5 {
                    if alpha_2 < 1.0e-7 {
                        // Fixed degenerate case, when obtain 0 / 0 uncertainty.
                        p_param = -6.0 / 15.0;
                    } else {
                        let tan_alpha_2 = libm::tan(alpha_2);
                        let mut value1 = 3.0 * (tan_alpha_2 - alpha_2);
                        value1 = alpha_2 / value1;
                        p_param += value1;
                    }
                }
                let q_param = (1.0 / 3.0) + p_param;
                temp_degree = 3;
                temp_poles = vec![
                    Pnt2d::new(0.0, 1.0),
                    Pnt2d::new(1.0, 0.0),
                    Pnt2d::new(0.0, p_param),
                    Pnt2d::new(q_param, 0.0),
                ];
                which = Evaluator::QuasiAngular;
            } else {
                // Convert_RationalC1
                for ii in (i64::from(order) + 1)..=i64::from(num_poles) {
                    put(&mut flat_knots, ii, 0.0)?;
                }
                put(&mut knots, 2, u_first + alpha)?;
                put(&mut mults, 2, degree - 1)?;
                temp_degree = 2;
                let alpha_2 = alpha * 0.5;
                let alpha_4 = alpha * 0.25;
                let tan_alpha_2 = libm::tan(alpha_2);
                let mut ys = [0.0; 4];
                let mut jj = 1usize;
                for ii in 1..=2usize {
                    if let Some(y) = ys.get_mut(ii) {
                        *y = 1.0 + alpha_4 * tan_alpha_2;
                    }
                    if let Some(y) = ys.get_mut(jj - 1) {
                        *y = 1.0;
                    }
                    jj += 3;
                }
                temp_poles = vec![
                    Pnt2d::new(-tan_alpha_2, ys[0]),
                    Pnt2d::new(alpha_4 - tan_alpha_2, ys[1]),
                    Pnt2d::new(-alpha_4 + tan_alpha_2, ys[2]),
                    Pnt2d::new(tan_alpha_2, ys[3]),
                ];
                temp_knots = vec![-alpha, 0.0, alpha];
                temp_mults = vec![temp_degree + 1, 1, temp_degree + 1];
                which = Evaluator::RationalC1;
            }
            let (c, s, d) = algorithmic_cos_and_sin(
                degree,
                &flat_knots,
                temp_degree,
                &temp_poles,
                &temp_knots,
                &temp_mults,
                which,
                np,
            )?;
            cos_num = c;
            sin_num = s;
            den = d;
            for ii in 0..np {
                if let (Some(c), Some(s)) = (cos_num.get_mut(ii), sin_num.get_mut(ii)) {
                    let value1 = cos_beta * *c - sin_beta * *s;
                    let value2 = sin_beta * *c + cos_beta * *s;
                    *c = value1;
                    *s = value2;
                }
            }
        } else {
            // Convert_Polynomial
            put(&mut knots, 1, 0.0)?;
            put(&mut knots, i64::from(num_knots), 1.0)?;
            put(&mut mults, 1, num_poles)?;
            put(&mut mults, i64::from(num_knots), num_poles)?;
            let (c, s, d) = build_polynomial_cos_and_sin(u_first, u_last, num_poles)?;
            cos_num = c;
            sin_num = s;
            den = d;
        }
        Ok(CosAndSin { cos_numerator: cos_num, sin_numerator: sin_num, denominator: den, degree, knots, mults })
    }

    /// `BuildCosAndSin(Parameterisation, CosNumerator, SinNumerator, Denominator, Degree, Knots, Mults)`: the periodic
    /// representation of the full circle. `Err(Construction)` unless `p` is `TgtThetaOver2` or `RationalC1`.
    pub fn build_cos_and_sin_periodic(p: ParameterisationType) -> R<CosAndSin> {
        match p {
            ParameterisationType::TgtThetaOver2 => {
                let t = Self::build_cos_and_sin(ParameterisationType::TgtThetaOver2_3, 0.0, 2.0 * PI)?;
                let n = t.cos_numerator.len().saturating_sub(1);
                let degree = t.degree;
                let mults = vec![degree; t.mults.len()];
                Ok(CosAndSin {
                    cos_numerator: t.cos_numerator.get(..n).map(<[f64]>::to_vec).unwrap_or_default(),
                    sin_numerator: t.sin_numerator.get(..n).map(<[f64]>::to_vec).unwrap_or_default(),
                    denominator: t.denominator.get(..n).map(<[f64]>::to_vec).unwrap_or_default(),
                    degree,
                    knots: t.knots,
                    mults,
                })
            }
            ParameterisationType::RationalC1 => {
                let first_param = 0.0;
                let last_param = PI;
                let t = Self::build_cos_and_sin(ParameterisationType::RationalC1, first_param, last_param)?;
                let degree: i32 = 4;
                let num_knots: i32 = 5;
                let num_flat_knots = (degree - 1) * num_knots + 2 * 2;
                let num_poles = num_flat_knots - degree - 1;
                let num_periodic_poles = num_poles - 2;
                let mut flat_knots = vec![0.0; usize::try_from(num_flat_knots).map_err(|_| ConvertError::Argument)?];
                let half_pi = PI * 0.5;
                let mut index = 1i64;
                for _ in 1..=2 {
                    put(&mut flat_knots, index, -half_pi)?;
                    index += 1;
                }
                for ii in 1..=num_knots {
                    for _ in 1..=(degree - 1) {
                        put(&mut flat_knots, index, f64::from(ii - 1) * half_pi)?;
                        index += 1;
                    }
                }
                for _ in 1..=2 {
                    put(&mut flat_knots, index, 2.0 * PI + half_pi)?;
                    index += 1;
                }
                let mut knots = Vec::new();
                let mut mults = Vec::new();
                for ii in 1..=num_knots {
                    knots.push(f64::from(ii - 1) * half_pi);
                    mults.push(degree - 1);
                }
                let parameters = bsplclib::build_schoenberg_points(degree, &flat_knots, num_poles)?;
                let mut contact = Vec::new();
                let mut flat_poles = Vec::new();
                let mut inverse = 1.0;
                for u in &parameters {
                    let mut param = *u;
                    if param > PI {
                        inverse = -1.0;
                        param -= PI;
                    }
                    let value1 = scalar_d0(param, t.degree, &t.cos_numerator, Some(&t.denominator), &t.knots, &t.mults);
                    let value2 = scalar_d0(param, t.degree, &t.sin_numerator, Some(&t.denominator), &t.knots, &t.mults);
                    let value3 = scalar_d0(param, t.degree, &t.denominator, None, &t.knots, &t.mults);
                    contact.push(0);
                    flat_poles.push(value1 * value3 * inverse);
                    flat_poles.push(value2 * value3 * inverse);
                    flat_poles.push(value3);
                }
                let (_, solved) = bsplclib::interpolate(degree, &flat_knots, &parameters, &contact, 3, &flat_poles)?;
                let mut cos = Vec::new();
                let mut sin = Vec::new();
                let mut den = Vec::new();
                for chunk in
                    solved.chunks(3).take(usize::try_from(num_periodic_poles).map_err(|_| ConvertError::Argument)?)
                {
                    if let [p1, p2, p3] = chunk {
                        let inverse = 1.0 / *p3;
                        cos.push(*p1 * inverse);
                        sin.push(*p2 * inverse);
                        den.push(*p3);
                    }
                }
                Ok(CosAndSin { cos_numerator: cos, sin_numerator: sin, denominator: den, degree, knots, mults })
            }
            _ => Err(ConvertError::Construction),
        }
    }
}

/// `Locate`: the parameter of the Bezier curve `poles` whose angle is `ang_fin`, by bisection in `[umin, umax]`.
fn locate(ang_fin: f64, poles: &[Pnt2d], umin: f64, umax: f64) -> R<f64> {
    let mut umin = umin;
    let mut umax = umax;
    let ptol = 1.0e-12; // Precision::Angular()
    let utol = PCONFUSION;
    // A bisection ends after about 60 steps; the bound only stops a NaN, which never narrows the interval.
    for _ in 0..4096 {
        if (umax - umin).abs() < utol {
            return Ok((umin + umax) / 2.0);
        }
        let ptest = (umax + umin) / 2.0;
        let val = bezier_d0(ptest, poles)?;
        let mut theta = libm::atan2(val.y(), val.x());
        if theta < 0.0 {
            theta += 2.0 * PI;
        }
        if (theta - ang_fin).abs() < ptol {
            return Ok(ptest);
        }
        if theta < ang_fin {
            umin = ptest;
        } else if theta > ang_fin {
            umax = ptest;
        }
    }
    Err(ConvertError::Argument)
}

/// `BuildPolynomialCosAndSin(UFirst, ULast, num_poles, CosNumerator, SinNumerator, Denominator)`: the degree-7
/// polynomial (Bezier) representation of the arc from `u_first` to `u_last`. `Err(Argument)` (a check OCCT 8.0.1 does
/// not make) unless `num_poles == 8` and `0 < u_last - u_first <= 2*PI + PConfusion`.
pub fn build_polynomial_cos_and_sin(u_first: f64, u_last: f64, num_poles: i32) -> R<(Vec<f64>, Vec<f64>, Vec<f64>)> {
    let delta = u_last - u_first;
    if num_poles != 8 || !(delta > 0.0 && delta <= 2.0 * PI + PCONFUSION) {
        return Err(ConvertError::Argument);
    }
    let pi2 = 2.0 * PI;
    let degree = num_poles - 1;
    let mut loc_u_first = u_first;
    // Return UFirst in [-2PI; 2PI] to make rotations without risk
    while loc_u_first > pi2 {
        let next = loc_u_first - pi2;
        if next.to_bits() == loc_u_first.to_bits() {
            return Err(ConvertError::Argument);
        }
        loc_u_first = next;
    }
    while loc_u_first < -pi2 {
        let next = loc_u_first + pi2;
        if next.to_bits() == loc_u_first.to_bits() {
            return Err(ConvertError::Argument);
        }
        loc_u_first = next;
    }
    // Return to the arc [0, Delta]
    let middle = 0.5 * delta;
    // Coincide the required bisector of the angular sector with axis -Ox.
    let angle = middle - PI;
    let mut t_poles = vec![
        Pnt2d::new(1.0, 0.0),
        Pnt2d::new(1.0, 1.013854),
        Pnt2d::new(-0.199043, 1.871905),
        Pnt2d::new(-1.937729, 1.057323),
        Pnt2d::new(-1.937729, -1.057323),
        Pnt2d::new(-0.199043, -1.871905),
        Pnt2d::new(1.0, -1.013854),
        Pnt2d::new(1.0, 0.0),
    ];
    let mut t = Trsf2d::new();
    t.set_rotation(&gp::origin2d(), angle);
    for p in &mut t_poles {
        p.transform(&t);
    }
    let mut t_min = 1.0 - (delta * 1.3 / PI);
    t_min *= 0.5;
    t_min = cmax(t_min, 0.0);
    let mut t_max = 1.0 + (delta * 1.3 / PI);
    t_max *= 0.5;
    t_max = cmin(t_max, 1.0);
    let trim_max = locate(delta, &t_poles, t_min, t_max)?;
    // as Bezier is symmetric correspondingly to the bissector of the angular sector ...
    let trim_min = 1.0 - trim_max;
    let knots = [0.0, 1.0];
    let mults = [degree + 1, degree + 1];
    let mut flat = Vec::with_capacity(16);
    for p in &t_poles {
        flat.push(p.x());
        flat.push(p.y());
    }
    let (new_flat, _, _) = bsplclib::trimming(degree, false, 2, &knots, &mults, &flat, trim_min, trim_max, 2, 16)?;
    let mut new_poles: Vec<Pnt2d> =
        new_flat.chunks(2).filter_map(|c| if let [x, y] = c { Some(Pnt2d::new(*x, *y)) } else { None }).collect();
    if new_poles.len() != 8 {
        return Err(ConvertError::OutOfRange);
    }
    // readjustment is obviously redundant
    let sin_d = libm::sin(delta);
    let cos_d = libm::cos(delta);
    let mut pdeb = Pnt2d::new(1.0, 0.0);
    let mut pfin = Pnt2d::new(cos_d, sin_d);
    let first = at(&new_poles, 1)?;
    let second = at(&new_poles, 2)?;
    let mut dtg = first.distance(&second);
    put(&mut new_poles, 1, pdeb)?;
    pdeb = Pnt2d::new(pdeb.x(), pdeb.y() + dtg);
    put(&mut new_poles, 2, pdeb)?;
    // readjustment to Euclid
    let n = i64::from(num_poles);
    dtg = at(&new_poles, n)?.distance(&at(&new_poles, n - 1)?);
    put(&mut new_poles, n, pfin)?;
    pfin = Pnt2d::new(pfin.x() + dtg * sin_d, pfin.y() + -dtg * cos_d);
    put(&mut new_poles, n - 1, pfin)?;
    // Rotation to return to the arc [LocUFirst, LocUFirst+Delta]
    t.set_rotation(&gp::origin2d(), loc_u_first);
    for p in &mut new_poles {
        p.transform(&t);
    }
    let cos: Vec<f64> = new_poles.iter().map(Pnt2d::x).collect();
    let sin: Vec<f64> = new_poles.iter().map(Pnt2d::y).collect();
    let den = vec![1.0; new_poles.len()];
    Ok((cos, sin, den))
}
