//! `math_DirectPolynomialRoots`: real roots of polynomials up to degree 4.
use super::MathError;

/// Threshold below which coefficients are considered effectively zero.
const ZERO_THRESHOLD: f64 = 1.0e-30;
/// Machine epsilon (`RealEpsilon()`).
const MACHINE_EPSILON: f64 = f64::EPSILON;
/// Floating-point radix used when scaling the coefficients.
const FLOATING_RADIX: f64 = 2.0;
/// Maximum number of Newton-Raphson refinement iterations.
const MAX_NEWTON_ITERATIONS: i32 = 10;
/// Upper limit to prevent numerical overflow.
const OVERFLOW_LIMIT: f64 = 1.0e+80;
/// `RealSmall()`.
const REAL_SMALL: f64 = f64::MIN_POSITIVE;

/// `Epsilon(value)`: the distance to the next representable double away from zero.
fn epsilon(value: f64) -> f64 {
    if value >= 0.0 { libm::nextafter(value, f64::MAX) - value } else { value - libm::nextafter(value, f64::MIN) }
}

/// `1.0 / std::log(2.0)`.
fn inv_log_radix() -> f64 {
    1.0 / libm::log(2.0)
}

/// Evaluates the polynomial with Horner's method.
fn evaluate_polynomial(poly: &[f64], x: f64) -> f64 {
    let mut iter = poly.iter();
    let mut result = iter.next().copied().unwrap_or(0.0);
    for c in iter {
        result = result * x + c;
    }
    result
}

/// Evaluates the polynomial and its derivative simultaneously.
fn evaluate_with_derivative(poly: &[f64], x: f64) -> (f64, f64) {
    let (c0, c1) = (poly.first().copied().unwrap_or(0.0), poly.get(1).copied().unwrap_or(0.0));
    let mut value = c0 * x + c1;
    let mut derivative = c0;
    for c in poly.iter().skip(2) {
        derivative = derivative * x + value;
        value = value * x + c;
    }
    (value, derivative)
}

/// Refines a root with Newton-Raphson iterations; the improved solution is only returned if it is better.
fn refine_root(poly: &[f64], initial_guess: f64) -> f64 {
    let mut value = 0.0;
    let mut solution = initial_guess;
    let initial_value = evaluate_polynomial(poly, initial_guess);
    for _ in 1..MAX_NEWTON_ITERATIONS {
        let (v, derivative) = evaluate_with_derivative(poly, solution);
        value = v;
        if derivative.abs() <= ZERO_THRESHOLD {
            break;
        }
        let delta = -value / derivative;
        if delta.abs() <= MACHINE_EPSILON * solution.abs() {
            break;
        }
        solution += delta;
    }
    if value.abs() <= initial_value.abs() { solution } else { initial_guess }
}

/// Scales the roots back and refines each of them.
fn scale_and_refine_all_roots(roots: &mut [f64; 4], nb_roots: usize, scale_factor: f64, poly: &[f64]) {
    for root in roots.iter_mut().take(nb_roots) {
        *root *= scale_factor;
        *root = refine_root(poly, *root);
    }
}

/// `ComputeBaseExponent(value)`: the base-2 exponent used for the coefficient scaling.
fn compute_base_exponent(value: f64) -> i32 {
    if value > 1.0 {
        (libm::log(value) * inv_log_radix()) as i32
    } else if value < -1.0 {
        (-libm::log(-value) * inv_log_radix()) as i32
    } else {
        0
    }
}

/// `ScaledCoefficients`.
struct ScaledCoefficients {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    scale_factor: f64,
}

impl ScaledCoefficients {
    fn scale_quartic(a: f64, b: f64, c: f64, d: f64, e: f64) -> Self {
        let exp = compute_base_exponent(e) / 4;
        let scale_factor = libm::pow(FLOATING_RADIX, f64::from(exp));
        let scale_factor2 = scale_factor * scale_factor;
        Self {
            a: a / scale_factor,
            b: b / scale_factor2,
            c: c / (scale_factor2 * scale_factor),
            d: d / (scale_factor2 * scale_factor2),
            scale_factor,
        }
    }
    fn scale_cubic(a: f64, b: f64, c: f64, d: f64) -> Self {
        let exp = compute_base_exponent(d) / 3;
        let scale_factor = libm::pow(FLOATING_RADIX, f64::from(exp));
        let scale_factor2 = scale_factor * scale_factor;
        Self {
            a: a / scale_factor,
            b: b / scale_factor2,
            c: c / (scale_factor2 * scale_factor),
            d, // Not used for the cubic, but kept for consistency.
            scale_factor,
        }
    }
}

/// `ComputeSpecialDiscriminant`: discriminant for the `P < 0` case with improved numerical stability.
fn compute_special_discriminant(beta: f64, gamma: f64, del: f64, a1: f64) -> f64 {
    let sigma = beta * gamma / 3.0 - 2.0 * beta * beta * beta / 27.0;
    let psi = gamma * gamma * (4.0 * gamma - beta * beta) / 27.0;
    let d1 = if sigma >= 0.0 { sigma + 2.0 * (-a1).sqrt() } else { sigma - 2.0 * (-a1).sqrt() };
    let d2 = psi / d1;
    if (del - d1).abs() >= 18.0 * MACHINE_EPSILON * (del.abs() + d1.abs())
        && (del - d2).abs() >= 24.0 * MACHINE_EPSILON * (del.abs() + d2.abs())
    {
        return (del - d1) * (del - d2) / 4.0;
    }
    0.0
}

/// `SolveCubicThreeRealRoots`: trigonometric method.
fn solve_cubic_three_real_roots(beta: f64, gamma: f64, del: f64, p: f64, q: f64, discr: f64, roots: &mut [f64; 4]) {
    if beta == 0.0 && q == 0.0 {
        // Special case: x^3 + Px = 0.
        roots[0] = (-p).sqrt();
        roots[1] = -roots[0];
        roots[2] = 0.0;
    } else {
        let sb = if beta >= 0.0 { 1.0 } else { -1.0 };
        let omega = libm::atan(0.5 * q / (-discr).sqrt());
        let sp3 = (-p / 3.0).sqrt();
        let y1 = -2.0 * sb * sp3 * libm::cos(core::f64::consts::PI / 6.0 - sb * omega / 3.0);
        roots[0] = -beta / 3.0 + y1;
        if beta * q <= 0.0 {
            roots[1] = -beta / 3.0 + 2.0 * sp3 * libm::sin(omega / 3.0);
        } else {
            // Alternative formula for better accuracy.
            let dbg = del - beta * gamma;
            let sdbg = if dbg >= 0.0 { 1.0 } else { -1.0 };
            let den1 = 8.0 * beta * beta / 9.0 - 4.0 * beta * y1 / 3.0 - 2.0 * q / y1;
            let den2 = 2.0 * y1 * y1 - q / y1;
            roots[1] = dbg / den1 + sdbg * (-27.0 * discr).sqrt() / den2;
        }
        // Vieta's formula for the third root.
        roots[2] = -del / (roots[0] * roots[1]);
    }
}

/// `SolveCubicOneRealRoot`: Cardano's formula.
fn solve_cubic_one_real_root(beta: f64, del: f64, p: f64, q: f64, discr: f64, roots: &mut [f64; 4]) {
    let mut u = discr.sqrt() + (q / 2.0).abs();
    u = if u >= 0.0 { libm::pow(u, 1.0 / 3.0) } else { -libm::pow(u.abs(), 1.0 / 3.0) };
    let h = if p >= 0.0 { u * u + p / 3.0 + (p / u) * (p / u) / 9.0 } else { u * q.abs() / (u * u - p / 3.0) };
    if beta * q >= 0.0 {
        if h.abs() <= REAL_SMALL && q.abs() <= REAL_SMALL {
            roots[0] = -beta / 3.0 - u + p / (3.0 * u);
        } else {
            roots[0] = -beta / 3.0 - q / h;
        }
    } else {
        roots[0] = -del / (beta * beta / 9.0 + h - beta * q / (3.0 * h));
    }
}

/// `SolveCubicMultipleRoots` (discriminant = 0); returns the number of roots.
fn solve_cubic_multiple_roots(beta: f64, gamma: f64, del: f64, p: f64, q: f64, roots: &mut [f64; 4]) -> usize {
    let sq = if q >= 0.0 { 1.0 } else { -1.0 };
    let sp3 = (-p / 3.0).sqrt();
    if beta * q <= 0.0 {
        roots[0] = -beta / 3.0 + sq * sp3;
        roots[1] = roots[0];
        if beta * q == 0.0 {
            roots[2] = -beta / 3.0 - 2.0 * sq * sp3;
        } else {
            roots[2] = -del / (roots[0] * roots[1]);
        }
    } else {
        roots[0] = -gamma / (beta + 3.0 * sq * sp3);
        roots[1] = roots[0];
        roots[2] = -beta / 3.0 - 2.0 * sq * sp3;
    }
    3
}

/// `ShouldReduceDegreeQuartic`.
fn should_reduce_degree_quartic(a: f64, b: f64, c: f64, d: f64, e: f64) -> bool {
    if a.abs() <= ZERO_THRESHOLD {
        return true;
    }
    let mut max_coeff = ZERO_THRESHOLD;
    max_coeff = max_coeff.max(b.abs());
    max_coeff = max_coeff.max(c.abs());
    max_coeff = max_coeff.max(d.abs());
    max_coeff = max_coeff.max(e.abs());
    if max_coeff > ZERO_THRESHOLD {
        max_coeff = epsilon(100.0 * max_coeff);
    }
    if a.abs() <= max_coeff {
        let max_coeff1000 = 1000.0 * max_coeff;
        let mut with_a = false;
        for x in [b, c, d, e] {
            if x.abs() > ZERO_THRESHOLD && x.abs() <= max_coeff1000 {
                with_a = true;
            }
        }
        return !with_a;
    }
    false
}

/// `SolveFerrariResolvent`: the largest root of the resolvent cubic, or `None` if it failed.
fn solve_ferrari_resolvent(a: f64, b: f64, c: f64, d: f64) -> Option<f64> {
    // Resolvent cubic: Y^3 + R3*Y^2 + S3*Y + T3 = 0.
    let r3 = -b;
    let s3 = a * c - 4.0 * d;
    let t3 = d * (4.0 * b - a * a) - c * c;
    let cubic = DirectPolynomialRoots::new_cubic(1.0, r3, s3, t3);
    if !cubic.is_done() {
        return None;
    }
    // Choose the largest root for numerical stability.
    let mut y0 = cubic.value(1).ok()?;
    for i in 2..=cubic.number_of_solutions().ok()? {
        let v = cubic.value(i).ok()?;
        if v > y0 {
            y0 = v;
        }
    }
    Some(y0)
}

/// `FactorQuarticViaFerrari`: returns `(P1, Q1, P2, Q2)` of the two quadratic factors.
fn factor_quartic_via_ferrari(a: f64, b: f64, c: f64, d: f64, y0: f64) -> (f64, f64, f64, f64) {
    let discr = a * y0 * 0.5 - c;
    let sdiscr = if discr >= 0.0 { 1.0 } else { -1.0 };
    let mut p0 = a * a * 0.25 - b + y0;
    p0 = if p0 < 0.0 { 0.0 } else { p0.sqrt() };
    let mut q0 = y0 * y0 * 0.25 - d;
    // Handle the case where Q0^2 is very close to zero more robustly.
    if q0.abs() < 10.0 * MACHINE_EPSILON {
        q0 = 0.0;
    } else {
        q0 = if q0 < 0.0 { 0.0 } else { q0.sqrt() };
    }
    let ademi = a * 0.5;
    let ydemi = y0 * 0.5;
    let sdiscr_q0 = sdiscr * q0;
    let mut p1 = ademi + p0;
    let mut q1 = ydemi + sdiscr_q0;
    let mut p2 = ademi - p0;
    let mut q2 = ydemi - sdiscr_q0;
    // Clean up near-zero coefficients.
    let eps = 100.0 * MACHINE_EPSILON;
    for x in [&mut p1, &mut p2, &mut q1, &mut q2] {
        if x.abs() <= eps {
            *x = 0.0;
        }
    }
    (p1, q1, p2, q2)
}

/// `math_DirectPolynomialRoots`.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectPolynomialRoots {
    done: bool,
    infinite: bool,
    nb_sol: usize,
    roots: [f64; 4],
}

impl DirectPolynomialRoots {
    fn blank() -> Self {
        Self { done: true, infinite: false, nb_sol: 0, roots: [0.0; 4] }
    }
    /// `math_DirectPolynomialRoots(A, B, C, D, E)`: `A*x^4 + B*x^3 + C*x^2 + D*x + E = 0`.
    pub fn new_quartic(a: f64, b: f64, c: f64, d: f64, e: f64) -> Self {
        let mut r = Self::blank();
        r.solve_quartic(a, b, c, d, e);
        r
    }
    /// `math_DirectPolynomialRoots(A, B, C, D)`: `A*x^3 + B*x^2 + C*x + D = 0`.
    pub fn new_cubic(a: f64, b: f64, c: f64, d: f64) -> Self {
        let mut r = Self::blank();
        r.solve_cubic(a, b, c, d);
        r
    }
    /// `math_DirectPolynomialRoots(A, B, C)`: `A*x^2 + B*x + C = 0`.
    pub fn new_quadratic(a: f64, b: f64, c: f64) -> Self {
        let mut r = Self::blank();
        r.solve_quadratic(a, b, c);
        r
    }
    /// `math_DirectPolynomialRoots(A, B)`: `A*x + B = 0`.
    pub fn new_linear(a: f64, b: f64) -> Self {
        let mut r = Self::blank();
        r.solve_linear(a, b);
        r
    }

    fn solve_quartic(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64) {
        // Check for degree reduction.
        if should_reduce_degree_quartic(a, b, c, d, e) {
            self.solve_cubic(b, c, d, e);
            return;
        }
        // Normalize the coefficients.
        let na = b / a;
        let nb = c / a;
        let nc = d / a;
        let nd = e / a;
        // Scale the coefficients to avoid overflow/underflow.
        let scaled = ScaledCoefficients::scale_quartic(na, nb, nc, nd, nd);
        // Solve Ferrari's resolvent cubic.
        let Some(y0) = solve_ferrari_resolvent(scaled.a, scaled.b, scaled.c, scaled.d) else {
            self.done = false;
            return;
        };
        // Factor into two quadratics.
        let (p1, q1, p2, q2) = factor_quartic_via_ferrari(scaled.a, scaled.b, scaled.c, scaled.d, y0);
        let quadratic1 = Self::new_quadratic(1.0, p1, q1);
        if !quadratic1.is_done() {
            self.done = false;
            return;
        }
        let quadratic2 = Self::new_quadratic(1.0, p2, q2);
        if !quadratic2.is_done() {
            self.done = false;
            return;
        }
        // Collect all the roots.
        self.nb_sol = quadratic1.nb_sol + quadratic2.nb_sol;
        let first = quadratic1.roots.iter().take(quadratic1.nb_sol);
        let second = quadratic2.roots.iter().take(quadratic2.nb_sol);
        for (slot, x) in self.roots.iter_mut().zip(first.chain(second)) {
            *slot = *x;
        }
        // Inverse scaling and Newton-Raphson refinement of all the roots.
        scale_and_refine_all_roots(&mut self.roots, self.nb_sol, scaled.scale_factor, &[a, b, c, d, e]);
    }

    fn solve_cubic(&mut self, a: f64, b: f64, c: f64, d: f64) {
        // Check for degree reduction.
        if a.abs() <= ZERO_THRESHOLD {
            self.solve_quadratic(b, c, d);
            return;
        }
        // Normalize the coefficients.
        let beta = b / a;
        let gamma = c / a;
        let del = d / a;
        // Scale to avoid overflow/underflow.
        let scaled = ScaledCoefficients::scale_cubic(beta, gamma, del, del);
        // Transform to the depressed cubic t^3 + Pt + Q = 0.
        let p1 = scaled.b;
        let p2 = -(scaled.a * scaled.a) / 3.0;
        let mut p = p1 + p2;
        let ep = 5.0 * MACHINE_EPSILON * (p1.abs() + p2.abs());
        if p.abs() <= ep {
            p = 0.0;
        }
        let q1 = scaled.c;
        let q2 = -scaled.a * scaled.b / 3.0;
        let q3 = 2.0 * (scaled.a * scaled.a * scaled.a) / 27.0;
        let mut q = q1 + q2 + q3;
        let eq = 10.0 * MACHINE_EPSILON * (q1.abs() + q2.abs() + q3.abs());
        if q.abs() <= eq {
            q = 0.0;
        }
        // Check for overflow.
        if p.abs() > OVERFLOW_LIMIT {
            self.done = false;
            return;
        }
        // Discriminant.
        let a1 = (p * p * p) / 27.0;
        let a2 = (q * q) / 4.0;
        let mut discr = a1 + a2;
        // Special handling for P < 0.
        if p < 0.0 {
            discr = compute_special_discriminant(scaled.a, scaled.b, scaled.c, a1);
        }
        if discr < 0.0 {
            // Three distinct real roots.
            self.nb_sol = 3;
            solve_cubic_three_real_roots(scaled.a, scaled.b, scaled.c, p, q, discr, &mut self.roots);
        } else if discr > 0.0 {
            // One real root.
            self.nb_sol = 1;
            solve_cubic_one_real_root(scaled.a, scaled.c, p, q, discr, &mut self.roots);
        } else {
            // Multiple roots.
            self.nb_sol = solve_cubic_multiple_roots(scaled.a, scaled.b, scaled.c, p, q, &mut self.roots);
        }
        // Inverse scaling and Newton-Raphson refinement of all the roots.
        scale_and_refine_all_roots(&mut self.roots, self.nb_sol, scaled.scale_factor, &[a, b, c, d]);
    }

    fn solve_quadratic(&mut self, a: f64, b: f64, c: f64) {
        // Check for degree reduction.
        if a.abs() <= ZERO_THRESHOLD {
            self.solve_linear(b, c);
            return;
        }
        // Normalized quadratic x^2 + P*x + Q = 0.
        let p = b / a;
        let q = c / a;
        // Discriminant with error bounds.
        let eps_d = 3.0 * MACHINE_EPSILON * (p * p + (4.0 * q).abs());
        let mut discrim = p * p - 4.0 * q;
        if discrim.abs() <= eps_d {
            discrim = 0.0;
        }
        let poly = [1.0, p, q];
        if discrim < 0.0 {
            // No real roots.
            self.nb_sol = 0;
        } else if discrim == 0.0 {
            // Double root.
            self.nb_sol = 2;
            let r = refine_root(&poly, -0.5 * p);
            self.roots[0] = r;
            self.roots[1] = r;
        } else {
            // Two distinct real roots, with the numerically stable formula.
            self.nb_sol = 2;
            let first = if p > 0.0 { -(p + discrim.sqrt()) / 2.0 } else { -(p - discrim.sqrt()) / 2.0 };
            let first = refine_root(&poly, first);
            self.roots[0] = first;
            self.roots[1] = refine_root(&poly, q / first);
        }
    }

    fn solve_linear(&mut self, a: f64, b: f64) {
        if a.abs() <= ZERO_THRESHOLD {
            if b.abs() <= ZERO_THRESHOLD {
                // 0 = 0: infinite solutions.
                self.infinite = true;
                return;
            }
            // 0*x + B = 0: no solution.
            self.nb_sol = 0;
            return;
        }
        // Normal case: a unique solution.
        self.nb_sol = 1;
        self.roots[0] = -b / a;
    }

    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    /// `InfiniteRoots()`.
    pub fn infinite_roots(&self) -> bool {
        self.infinite
    }
    /// `NbSolutions()`: fails when there are infinitely many solutions (`StdFail_InfiniteSolutions`),
    /// or when the computation is not done (OCCT then returns an undefined count).
    pub fn number_of_solutions(&self) -> Result<i32, MathError> {
        if self.infinite {
            return Err(MathError::InfiniteSolutions);
        }
        if !self.done {
            return Err(MathError::NotDone);
        }
        i32::try_from(self.nb_sol).map_err(|_| MathError::Range)
    }
    /// `Value(theIndex)`: the root `theIndex` (1-based).
    pub fn value(&self, index: i32) -> Result<f64, MathError> {
        if self.infinite {
            return Err(MathError::InfiniteSolutions);
        }
        let k = usize::try_from(index - 1).map_err(|_| MathError::Range)?;
        if k >= self.nb_sol {
            return Err(MathError::Range);
        }
        self.roots.get(k).copied().ok_or(MathError::Range)
    }
    /// `Dump(stream)`.
    pub fn dump(&self) -> String {
        let mut s = String::from("math_DirectPolynomialRoots ");
        if !self.done {
            s.push_str(" Not Done \n");
        } else if self.infinite {
            s.push_str(" Status = Infinity Roots \n");
        } else {
            s.push_str(" Status = Not Infinity Roots \n");
            s.push_str(&format!(" Number of solutions = {}\n", self.nb_sol));
            for (i, r) in self.roots.iter().take(self.nb_sol).enumerate() {
                s.push_str(&format!(" Solution number {} = {}\n", i + 1, r));
            }
        }
        s
    }
}
