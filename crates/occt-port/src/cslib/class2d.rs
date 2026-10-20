//! `CSLib_Class2d`: classification of a point against a polygon in the parametric space.
use crate::{gp::Pnt2d, mathlin::MathError};

/// `CSLib_Class2d::Result`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class2dResult {
    Inside = 1,
    Outside = -1,
    Uncertain = 0,
}

/// `Precision::PConfusion()` (evaluated in `f64`).
const P_CONFUSION: f64 = 1.0e-7 * 0.01;
/// `THE_MIN_RANGE`.
const MIN_RANGE: f64 = 1e-10;

/// `transformToNormalized`.
fn transform_to_normalized(u: f64, u_min: f64, u_range: f64) -> f64 {
    if u_range > MIN_RANGE {
        return (u - u_min) / u_range;
    }
    u
}

/// `CSLib_Class2d`.
#[derive(Debug, Clone, Default)]
pub struct Class2d {
    pnts_2d_x: Vec<f64>,
    pnts_2d_y: Vec<f64>,
    tol_u: f64,
    tol_v: f64,
    points_count: usize,
    u_min: f64,
    v_min: f64,
    u_max: f64,
    v_max: f64,
}

impl Class2d {
    /// `CSLib_Class2d()`: no polygon.
    pub fn new() -> Self {
        Self::default()
    }

    /// The constructors from an array, a sequence or a dynamic array of points (`init`).
    pub fn from_points(
        points: &[Pnt2d],
        tol_u: f64,
        tol_v: f64,
        u_min: f64,
        v_min: f64,
        u_max: f64,
        v_max: f64,
    ) -> Self {
        let mut c = Self { u_min, v_min, u_max, v_max, ..Self::default() };
        if u_max <= u_min || v_max <= v_min || points.len() < 3 {
            c.points_count = 0;
            return c;
        }
        c.points_count = points.len();
        c.tol_u = tol_u;
        c.tol_v = tol_v;
        let du = u_max - u_min;
        let dv = v_max - v_min;
        for p in points {
            c.pnts_2d_x.push(transform_to_normalized(p.x(), u_min, du));
            c.pnts_2d_y.push(transform_to_normalized(p.y(), v_min, dv));
        }
        if let Some(&first) = c.pnts_2d_x.first() {
            c.pnts_2d_x.push(first);
        }
        if let Some(&first) = c.pnts_2d_y.first() {
            c.pnts_2d_y.push(first);
        }
        if du > MIN_RANGE {
            c.tol_u /= du;
        }
        if dv > MIN_RANGE {
            c.tol_v /= dv;
        }
        c
    }

    fn x(&self, i: usize) -> Result<f64, MathError> {
        self.pnts_2d_x.get(i).copied().ok_or(MathError::Range)
    }

    fn y(&self, i: usize) -> Result<f64, MathError> {
        self.pnts_2d_y.get(i).copied().ok_or(MathError::Range)
    }

    /// `SiDans`.
    pub fn si_dans(&self, point: &Pnt2d) -> Class2dResult {
        if self.points_count == 0 {
            return Class2dResult::Uncertain;
        }
        let mut x = point.x();
        let mut y = point.y();
        let tol_u = self.tol_u * (self.u_max - self.u_min);
        let tol_v = self.tol_v * (self.v_max - self.v_min);
        if x < (self.u_min - tol_u) || x > (self.u_max + tol_u) || y < (self.v_min - tol_v) || y > (self.v_max + tol_v)
        {
            return Class2dResult::Outside;
        }
        x = transform_to_normalized(x, self.u_min, self.u_max - self.u_min);
        y = transform_to_normalized(y, self.v_min, self.v_max - self.v_min);
        let Ok(result) = self.internal_si_dans_ou_on(x, y) else {
            return Class2dResult::Uncertain;
        };
        if result == Class2dResult::Uncertain {
            return Class2dResult::Uncertain;
        }
        if self.tol_u > 0.0 || self.tol_v > 0.0 {
            let is_inside = result == Class2dResult::Inside;
            let corners = [
                (x - self.tol_u, y - self.tol_v),
                (x + self.tol_u, y - self.tol_v),
                (x - self.tol_u, y + self.tol_v),
                (x + self.tol_u, y + self.tol_v),
            ];
            for (cx, cy) in corners {
                match self.internal_si_dans(cx, cy) {
                    Ok(inside) if inside == is_inside => {}
                    _ => return Class2dResult::Uncertain,
                }
            }
        }
        result
    }

    /// `SiDans_OnMode`.
    pub fn si_dans_on_mode(&self, point: &Pnt2d, tol: f64) -> Class2dResult {
        if self.points_count == 0 {
            return Class2dResult::Uncertain;
        }
        let mut x = point.x();
        let mut y = point.y();
        if x < (self.u_min - tol) || x > (self.u_max + tol) || y < (self.v_min - tol) || y > (self.v_max + tol) {
            return Class2dResult::Outside;
        }
        x = transform_to_normalized(x, self.u_min, self.u_max - self.u_min);
        y = transform_to_normalized(y, self.v_min, self.v_max - self.v_min);
        let Ok(result) = self.internal_si_dans_ou_on(x, y) else {
            return Class2dResult::Uncertain;
        };
        if tol > 0.0 {
            let is_inside = result == Class2dResult::Inside;
            let corners = [(x - tol, y - tol), (x + tol, y - tol), (x - tol, y + tol), (x + tol, y + tol)];
            for (cx, cy) in corners {
                match self.internal_si_dans(cx, cy) {
                    Ok(inside) if inside == is_inside => {}
                    _ => return Class2dResult::Uncertain,
                }
            }
        }
        result
    }

    /// `internalSiDans` (coordinates in the normalized space).
    pub fn internal_si_dans(&self, px: f64, py: f64) -> Result<bool, MathError> {
        let mut nb_crossings = 0i32;
        let mut prev_dx = self.x(0)? - px;
        let mut prev_dy = self.y(0)? - py;
        let mut prev_y_is_negative = prev_dy < 0.0;
        for next_idx in 1..=self.points_count {
            let curr_dx = self.x(next_idx)? - px;
            let curr_dy = self.y(next_idx)? - py;
            let curr_y_is_negative = curr_dy < 0.0;
            if curr_y_is_negative != prev_y_is_negative {
                if prev_dx > 0.0 && curr_dx > 0.0 {
                    nb_crossings += 1;
                } else if prev_dx > 0.0 || curr_dx > 0.0 {
                    let x_intersect = prev_dx - prev_dy * (curr_dx - prev_dx) / (curr_dy - prev_dy);
                    if x_intersect > 0.0 {
                        nb_crossings += 1;
                    }
                }
                prev_y_is_negative = curr_y_is_negative;
            }
            prev_dx = curr_dx;
            prev_dy = curr_dy;
        }
        Ok((nb_crossings & 1) != 0)
    }

    /// `internalSiDansOuOn` (coordinates in the normalized space).
    pub fn internal_si_dans_ou_on(&self, px: f64, py: f64) -> Result<Class2dResult, MathError> {
        let mut nb_crossings = 0i32;
        let mut prev_dx = self.x(0)? - px;
        let mut prev_dy = self.y(0)? - py;
        let mut prev_y_is_negative = prev_dy < 0.0;
        for next_idx in 1..=self.points_count {
            let prev_idx = next_idx - 1;
            let curr_dx = self.x(next_idx)? - px;
            let curr_dy = self.y(next_idx)? - py;
            if curr_dx < self.tol_u && curr_dx > -self.tol_u && curr_dy < self.tol_v && curr_dy > -self.tol_v {
                return Ok(Class2dResult::Uncertain);
            }
            let edge_dx = self.x(next_idx)? - self.x(prev_idx)?;
            if (self.x(prev_idx)? - px) * curr_dx < 0.0 && edge_dx.abs() > P_CONFUSION {
                let interp_y = self.y(next_idx)? - (self.y(next_idx)? - self.y(prev_idx)?) / edge_dx * curr_dx;
                let delta_y = interp_y - py;
                if delta_y >= -self.tol_v && delta_y <= self.tol_v {
                    return Ok(Class2dResult::Uncertain);
                }
            }
            let curr_y_is_negative = curr_dy < 0.0;
            if curr_y_is_negative != prev_y_is_negative {
                if prev_dx > 0.0 && curr_dx > 0.0 {
                    nb_crossings += 1;
                } else if prev_dx > 0.0 || curr_dx > 0.0 {
                    let x_intersect = prev_dx - prev_dy * (curr_dx - prev_dx) / (curr_dy - prev_dy);
                    if x_intersect > 0.0 {
                        nb_crossings += 1;
                    }
                }
                prev_y_is_negative = curr_y_is_negative;
            }
            prev_dx = curr_dx;
            prev_dy = curr_dy;
        }
        Ok(if (nb_crossings & 1) != 0 { Class2dResult::Inside } else { Class2dResult::Outside })
    }
}
