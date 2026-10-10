//! Forward-mode dual numbers with a fixed, small gradient.
//!
//! Every constraint equation depends on at most [`N`] scalar unknowns. Evaluating
//! the equation on dual numbers yields its value and its exact (analytic) partial
//! derivatives in one pass, so Jacobians never use finite differences. All
//! operations are plain IEEE arithmetic, so results are deterministic.

use core::ops::{Add, Div, Mul, Neg, Sub};

/// Maximum number of local unknowns per equation.
pub(crate) const N: usize = 8;

/// Below this magnitude a norm or divisor is treated as zero and a fixed,
/// deterministic substitute derivative is used instead of dividing by zero.
pub(crate) const TINY: f64 = 1e-12;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Dual {
    pub(crate) v: f64,
    pub(crate) d: [f64; N],
}

impl Dual {
    pub(crate) const ZERO: Dual = Dual { v: 0.0, d: [0.0; N] };

    pub(crate) fn cst(v: f64) -> Dual {
        Dual { v, d: [0.0; N] }
    }

    pub(crate) fn var(v: f64, k: usize) -> Dual {
        let mut d = [0.0; N];
        if let Some(slot) = d.get_mut(k) {
            *slot = 1.0;
        }
        Dual { v, d }
    }

    fn map2(self, o: Dual, v: f64, fa: f64, fb: f64) -> Dual {
        let mut d = [0.0; N];
        for ((r, a), b) in d.iter_mut().zip(self.d.iter()).zip(o.d.iter()) {
            *r = fa * a + fb * b;
        }
        Dual { v, d }
    }

    pub(crate) fn scale(self, s: f64) -> Dual {
        let mut d = self.d;
        for x in &mut d {
            *x *= s;
        }
        Dual { v: self.v * s, d }
    }

    /// `sqrt(x² + y²)`. At the origin the derivative of `|x|` is used, which
    /// keeps the Jacobian finite and deterministic.
    pub(crate) fn hypot(x: Dual, y: Dual) -> Dual {
        let n = libm::hypot(x.v, y.v);
        if n < TINY {
            return Dual { v: n, d: x.d };
        }
        x.map2(y, n, x.v / n, y.v / n)
    }

    /// `atan2(y, x)` with derivative `(x dy - y dx) / (x² + y²)`.
    pub(crate) fn atan2(y: Dual, x: Dual) -> Dual {
        let v = libm::atan2(y.v, x.v);
        let r2 = x.v * x.v + y.v * y.v;
        if r2 < TINY * TINY {
            return Dual::cst(v);
        }
        y.map2(x, v, x.v / r2, -y.v / r2)
    }

    /// Division that never divides by (almost) zero: a divisor smaller than
    /// [`TINY`] in magnitude is replaced by `±TINY`.
    pub(crate) fn safe_div(self, o: Dual) -> Dual {
        let b = if o.v.abs() < TINY { if o.v < 0.0 { -TINY } else { TINY } } else { o.v };
        let v = self.v / b;
        self.map2(o, v, 1.0 / b, -v / b)
    }
}

impl Add for Dual {
    type Output = Dual;
    fn add(self, o: Dual) -> Dual {
        self.map2(o, self.v + o.v, 1.0, 1.0)
    }
}

impl Sub for Dual {
    type Output = Dual;
    fn sub(self, o: Dual) -> Dual {
        self.map2(o, self.v - o.v, 1.0, -1.0)
    }
}

impl Mul for Dual {
    type Output = Dual;
    fn mul(self, o: Dual) -> Dual {
        self.map2(o, self.v * o.v, o.v, self.v)
    }
}

impl Div for Dual {
    type Output = Dual;
    fn div(self, o: Dual) -> Dual {
        self.safe_div(o)
    }
}

impl Neg for Dual {
    type Output = Dual;
    fn neg(self) -> Dual {
        self.scale(-1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_rule() {
        let x = Dual::var(3.0, 0);
        let y = Dual::var(4.0, 1);
        let f = x * y + x;
        assert_eq!(f.v, 15.0);
        assert_eq!(&f.d[..2], &[5.0, 3.0]);
        let h = Dual::hypot(x, y);
        assert_eq!(h.v, 5.0);
        assert!((h.d[0] - 0.6).abs() < 1e-15 && (h.d[1] - 0.8).abs() < 1e-15);
    }

    #[test]
    fn degenerate_inputs_stay_finite() {
        let z = Dual::var(0.0, 0);
        let h = Dual::hypot(z, Dual::var(0.0, 1));
        assert!(h.d.iter().all(|d| d.is_finite()));
        let q = Dual::var(1.0, 0) / Dual::cst(0.0);
        assert!(q.v.is_finite() && q.d.iter().all(|d| d.is_finite()));
        let a = Dual::atan2(Dual::var(0.0, 0), Dual::var(0.0, 1));
        assert!(a.d.iter().all(|d| d.is_finite()));
    }
}
