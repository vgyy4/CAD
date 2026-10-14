//! `math_EigenValuesSearcher` (QL algorithm with implicit shifts for a symmetric tridiagonal
//! matrix) and the Gauss / Gauss-Kronrod points and weights computed from it.
// The deflation test `|e| + s == s` is part of the OCCT algorithm.
#![allow(clippy::float_cmp)]
use super::{MathError, Matrix, Vector};

/// Maximum number of QL iterations per eigenvalue before the search fails.
const MAX_ITERATIONS: i32 = 30;

/// `computeHypotenuseLength(x, y)`.
fn hypotenuse(x: f64, y: f64) -> f64 {
    (x * x + y * y).sqrt()
}

/// `shiftSubdiagonalElements`.
fn shift_subdiagonal(sub: &mut Vector, size: i32) {
    for i in 2..=size {
        sub.put(i - 1, sub.at(i));
    }
    sub.put(size, 0.0);
}

/// `findSubmatrixEnd`: the end of the current unreduced submatrix (deflation test).
fn find_submatrix_end(diag: &Vector, sub: &Vector, start: i32, size: i32) -> i32 {
    let mut end = start;
    while end < size {
        let diag_sum = diag.at(end).abs() + diag.at(end + 1).abs();
        if sub.at(end).abs() + diag_sum == diag_sum {
            break;
        }
        end += 1;
    }
    end
}

/// `computeWilkinsonShift`.
fn wilkinson_shift(diag: &Vector, sub: &Vector, start: i32, end: i32) -> f64 {
    let mut shift = (diag.at(start + 1) - diag.at(start)) / (2.0 * sub.at(start));
    let radius = hypotenuse(1.0, shift);
    if shift < 0.0 {
        shift = diag.at(end) - diag.at(start) + sub.at(start) / (shift - radius);
    } else {
        shift = diag.at(end) - diag.at(start) + sub.at(start) / (shift + radius);
    }
    shift
}

/// `performQLStep`: a single QL step with implicit shift.
fn ql_step(
    diag: &mut Vector,
    sub: &mut Vector,
    vecs: &mut Matrix,
    start: i32,
    end: i32,
    shift: f64,
    size: i32,
) -> bool {
    let mut sine = 1.0;
    let mut cosine = 1.0;
    let mut prev_diag = 0.0;
    let mut shift = shift;
    let mut radius = 0.0;
    let mut row = end - 1;
    while row >= start {
        let temp_val = sine * sub.at(row);
        let subdiag_temp = cosine * sub.at(row);
        radius = hypotenuse(temp_val, shift);
        sub.put(row + 1, radius);
        if radius == 0.0 {
            diag.put(row + 1, diag.at(row + 1) - prev_diag);
            sub.put(end, 0.0);
            break;
        }
        sine = temp_val / radius;
        cosine = shift / radius;
        shift = diag.at(row + 1) - prev_diag;
        let radius_temp = (diag.at(row) - shift) * sine + 2.0 * cosine * subdiag_temp;
        prev_diag = sine * radius_temp;
        diag.put(row + 1, shift + prev_diag);
        shift = cosine * radius_temp - subdiag_temp;
        // Update the eigenvector matrix.
        for k in 1..=size {
            let temp_vec = vecs.at(k, row + 1);
            vecs.put(k, row + 1, sine * vecs.at(k, row) + cosine * temp_vec);
            vecs.put(k, row, cosine * vecs.at(k, row) - sine * temp_vec);
        }
        row -= 1;
    }
    // Special case: the last radius vanished.
    if radius == 0.0 && row >= 1 {
        return true;
    }
    diag.put(start, diag.at(start) - prev_diag);
    sub.put(start, shift);
    sub.put(end, 0.0);
    true
}

/// `performQLAlgorithm`.
fn ql_algorithm(diag: &mut Vector, sub: &mut Vector, vecs: &mut Matrix, size: i32) -> bool {
    for start in 1..=size {
        let mut iter_count = 0;
        loop {
            let end = find_submatrix_end(diag, sub, start, size);
            if end != start {
                let tried = iter_count;
                iter_count += 1;
                if tried == MAX_ITERATIONS {
                    return false;
                }
                let shift = wilkinson_shift(diag, sub, start, end);
                if !ql_step(diag, sub, vecs, start, end, shift, size) {
                    return false;
                }
            }
            if end == start {
                break;
            }
        }
    }
    true
}

/// `math_EigenValuesSearcher`: eigenvalues and eigenvectors of a symmetric tridiagonal matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct EigenValuesSearcher {
    is_done: bool,
    n: i32,
    eigen_values: Vector,
    eigen_vectors: Matrix,
}

impl EigenValuesSearcher {
    /// `math_EigenValuesSearcher(theDiagonal, theSubdiagonal)`; the arrays are given as slices
    /// with the lower bound `lower` (only their lengths and order matter). If the lengths differ,
    /// the search is not done, as in OCCT. The first subdiagonal item is not used.
    pub fn new(diagonal: &[f64], subdiagonal: &[f64], lower: i32) -> Result<Self, MathError> {
        let _ = lower;
        let n = i32::try_from(diagonal.len()).map_err(|_| MathError::Construction)?;
        let mut eigen_values = Vector::new(1, n)?;
        let mut eigen_vectors = Matrix::new(1, n, 1, n)?;
        if subdiagonal.len() != diagonal.len() {
            return Ok(Self { is_done: false, n, eigen_values, eigen_vectors });
        }
        let mut diag = Vector::from_slice(1, diagonal)?;
        let mut sub = Vector::from_slice(1, subdiagonal)?;
        shift_subdiagonal(&mut sub, n);
        for row in 1..=n {
            for col in 1..=n {
                eigen_vectors.put(row, col, if row == col { 1.0 } else { 0.0 });
            }
        }
        let mut converged = true;
        if n != 1 {
            converged = ql_algorithm(&mut diag, &mut sub, &mut eigen_vectors, n);
        }
        if converged {
            for i in 1..=n {
                eigen_values.put(i, diag.at(i));
            }
        }
        Ok(Self { is_done: converged, n, eigen_values, eigen_vectors })
    }
    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.is_done
    }
    /// `Dimension()`.
    pub fn dimension(&self) -> i32 {
        self.n
    }
    /// `EigenValue(theIndex)`.
    pub fn eigen_value(&self, index: i32) -> Result<f64, MathError> {
        self.eigen_values.value(index)
    }
    /// `EigenVector(theIndex)`: column `index` of the eigenvector matrix, bounds `1..=n`.
    pub fn eigen_vector(&self, index: i32) -> Result<Vector, MathError> {
        self.eigen_vectors.col(index)
    }
}

/// Eigenvalue/weight pairs sorted by eigenvalue (`std::sort` of `math_ValueAndWeight`).
fn sorted_points_and_weights(search: &EigenValuesSearcher, count: i32) -> Result<(Vec<f64>, Vec<f64>), MathError> {
    let mut pairs = Vec::new();
    for i in 1..=count {
        let eigen_vector = search.eigen_vector(i)?;
        let mut weight = eigen_vector.value(1)?;
        weight = 2.0 * weight * weight;
        pairs.push((search.eigen_value(i)?, weight));
    }
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
    Ok(pairs.into_iter().unzip())
}

/// `math_ComputeGaussPointsAndWeights`.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputeGaussPointsAndWeights {
    points: Vector,
    weights: Vector,
    done: bool,
}

/// Placeholder returned by `points()`/`weights()` when the computation is not done.
fn not_done_vector() -> Vector {
    Vector { lower: 1, data: vec![0.0] }
}

impl ComputeGaussPointsAndWeights {
    /// `math_ComputeGaussPointsAndWeights(Number)`.
    pub fn new(number: i32) -> Self {
        match Self::compute(number) {
            Some((points, weights)) => Self { points, weights, done: true },
            None => Self { points: not_done_vector(), weights: not_done_vector(), done: false },
        }
    }
    fn compute(number: i32) -> Option<(Vector, Vector)> {
        if number < 1 {
            return None;
        }
        let mut diag = Vec::new();
        let mut sub = Vec::new();
        for i in 1..=number {
            diag.push(0.0);
            if i == 1 {
                sub.push(0.0);
            } else {
                let sqr_im1 = (i - 1) * (i - 1);
                sub.push((f64::from(sqr_im1) / (4.0 * f64::from(sqr_im1) - 1.0)).sqrt());
            }
        }
        let search = EigenValuesSearcher::new(&diag, &sub, 1).ok()?;
        if !search.is_done() {
            return None;
        }
        let (points, weights) = sorted_points_and_weights(&search, number).ok()?;
        Some((Vector::from_slice(1, &points).ok()?, Vector::from_slice(1, &weights).ok()?))
    }
    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    /// `Points()` (a placeholder when the computation is not done).
    pub fn points(&self) -> Vector {
        self.points.clone()
    }
    /// `Weights()` (a placeholder when the computation is not done).
    pub fn weights(&self) -> Vector {
        self.weights.clone()
    }
}

/// A work array whose first item has the index `-1`: the `s`/`t` pointers of the Kronrod algorithm
/// (`s = new double[n]; s++`) and the `a`/`b` pointers (`a = aa + 1`).
struct Shifted(Vec<f64>);

impl Shifted {
    fn at(&self, i: i32) -> f64 {
        usize::try_from(i + 1).ok().and_then(|k| self.0.get(k)).copied().unwrap_or(0.0)
    }
    fn put(&mut self, i: i32, value: f64) {
        if let Some(slot) = usize::try_from(i + 1).ok().and_then(|k| self.0.get_mut(k)) {
            *slot = value;
        }
    }
}

/// `math_ComputeKronrodPointsAndWeights`.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputeKronrodPointsAndWeights {
    points: Vector,
    weights: Vector,
    done: bool,
}

impl ComputeKronrodPointsAndWeights {
    /// `math_ComputeKronrodPointsAndWeights(Number)`: `2 * Number + 1` points.
    pub fn new(number: i32) -> Self {
        match Self::compute(number) {
            Some((points, weights)) => Self { points, weights, done: true },
            None => Self { points: not_done_vector(), weights: not_done_vector(), done: false },
        }
    }
    fn compute(number: i32) -> Option<(Vector, Vector)> {
        if number < 1 {
            return None;
        }
        let a2np1 = 2 * number + 1;
        // Initialize the symmetric tridiagonal matrix.
        let n = number;
        let kronrod_n = 2 * number + 1;
        let a3kn2p1 = (3 * (number + 1) / 2 + 1).min(kronrod_n);
        let mut diag = vec![0.0; usize::try_from(a2np1).ok()?];
        let mut sub = vec![0.0; usize::try_from(a2np1).ok()?];
        for i in 2..=a3kn2p1 {
            let sqr_im1 = (i - 1) * (i - 1);
            *sub.get_mut(usize::try_from(i - 1).ok()?)? = f64::from(sqr_im1) / (4.0 * f64::from(sqr_im1) - 1.0);
        }

        // The algorithm calculates weights and points symmetrically and uses the index -1 by design.
        let nd2 = number / 2;
        let mut s = Shifted(vec![0.0; usize::try_from(nd2 + 2).ok()?]);
        let mut t = Shifted(vec![0.0; usize::try_from(nd2 + 2).ok()?]);

        // Generation of the Jacobi-Kronrod matrix.
        let mut aa = vec![0.0; usize::try_from(a2np1 + 1).ok()?];
        let mut bb = vec![0.0; usize::try_from(a2np1 + 1).ok()?];
        for i in 1..=a2np1 {
            let k = usize::try_from(i).ok()?;
            *aa.get_mut(k)? = *diag.get(k - 1)?;
            *bb.get_mut(k)? = *sub.get(k - 1)?;
        }
        let mut a = Shifted(aa);
        let mut b = Shifted(bb);

        // Eastward phase.
        t.put(0, b.at(number + 1));
        for m in 0..=n - 2 {
            let mut u = 0.0;
            for k in (0..=(m + 1) / 2).rev() {
                let l = m - k;
                u += (a.at(k + n + 1) - a.at(l)) * t.at(k) + b.at(k + n + 1) * s.at(k - 1) - b.at(l) * s.at(k);
                s.put(k, u);
            }
            core::mem::swap(&mut t, &mut s);
        }
        for j in (0..=nd2).rev() {
            s.put(j, s.at(j - 1));
        }

        // Southward phase.
        let mut j = -1;
        for m in n - 1..=2 * n - 3 {
            let mut u = 0.0;
            for k in m + 1 - n..=(m - 1) / 2 {
                let l = m - k;
                j = n - 1 - l;
                u += -(a.at(k + n + 1) - a.at(l)) * t.at(j) - b.at(k + n + 1) * s.at(j) + b.at(l) * s.at(j + 1);
                s.put(j, u);
            }
            if m % 2 == 0 {
                let k = m / 2;
                a.put(k + n + 1, a.at(k) + (s.at(j) - b.at(k + n + 1) * s.at(j + 1)) / t.at(j + 1));
            } else {
                let k = (m + 1) / 2;
                b.put(k + n + 1, s.at(j) / s.at(j + 1));
            }
            core::mem::swap(&mut t, &mut s);
        }

        // Termination phase.
        a.put(2 * number, a.at(n - 1) - b.at(2 * number) * s.at(0) / t.at(0));

        for i in 1..=a2np1 {
            let k = usize::try_from(i).ok()?;
            *diag.get_mut(k - 1)? = *a.0.get(k)?;
            *sub.get_mut(k - 1)? = *b.0.get(k)?;
        }
        for x in &mut sub {
            *x = x.sqrt();
        }

        let search = EigenValuesSearcher::new(&diag, &sub, 1).ok()?;
        if !search.is_done() {
            return None;
        }
        let (points, weights) = sorted_points_and_weights(&search, a2np1).ok()?;
        Some((Vector::from_slice(1, &points).ok()?, Vector::from_slice(1, &weights).ok()?))
    }
    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    /// `Points()` (a placeholder when the computation is not done).
    pub fn points(&self) -> Vector {
        self.points.clone()
    }
    /// `Weights()` (a placeholder when the computation is not done).
    pub fn weights(&self) -> Vector {
        self.weights.clone()
    }
}
