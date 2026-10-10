//! Small dense linear algebra: Cholesky solve for the damped normal equations and
//! an incremental orthonormal row basis (modified Gram–Schmidt with
//! re-orthogonalisation) for rank, redundancy and null-space analysis.
//!
//! Everything is written with plain scalar loops in a fixed order, so results are
//! bit-identical on every platform. Out-of-range accesses cannot happen by
//! construction, but are handled with `get` (returning zero / doing nothing)
//! rather than panicking.

use crate::geom2::not_gt;

fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut s = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        s += x * y;
    }
    s
}

fn row(a: &[f64], n: usize, i: usize, len: usize) -> &[f64] {
    let start = i.saturating_mul(n);
    a.get(start..start.saturating_add(len)).unwrap_or(&[])
}

/// Solves `A x = b` for a symmetric positive definite `n × n` matrix `a` (row
/// major; only the lower triangle is read, and it is overwritten with the
/// Cholesky factor). `b` is overwritten with `x`. Returns `false` when `a` is not
/// numerically positive definite or the result is not finite.
pub(crate) fn cholesky_solve(a: &mut [f64], n: usize, b: &mut [f64]) -> bool {
    if a.len() != n.saturating_mul(n) || b.len() != n {
        return false;
    }
    for j in 0..n {
        let rj = row(a, n, j, j);
        let s = a.get(j * n + j).copied().unwrap_or(0.0) - dot(rj, rj);
        // `!(s > 0)` also rejects NaN.
        if not_gt(s, 0.0) {
            return false;
        }
        let d = s.sqrt();
        if let Some(x) = a.get_mut(j * n + j) {
            *x = d;
        }
        for i in (j + 1)..n {
            let v = (a.get(i * n + j).copied().unwrap_or(0.0) - dot(row(a, n, i, j), row(a, n, j, j))) / d;
            if let Some(x) = a.get_mut(i * n + j) {
                *x = v;
            }
        }
    }
    // Forward substitution: L y = b.
    for i in 0..n {
        let s = dot(row(a, n, i, i), b.get(..i).unwrap_or(&[]));
        let d = a.get(i * n + i).copied().unwrap_or(1.0);
        if let Some(x) = b.get_mut(i) {
            *x = (*x - s) / d;
        }
    }
    // Back substitution: Lᵀ x = y.
    for i in (0..n).rev() {
        let mut s = 0.0;
        for k in (i + 1)..n {
            s += a.get(k * n + i).copied().unwrap_or(0.0) * b.get(k).copied().unwrap_or(0.0);
        }
        let d = a.get(i * n + i).copied().unwrap_or(1.0);
        if let Some(x) = b.get_mut(i) {
            *x = (*x - s) / d;
        }
    }
    b.iter().all(|x| x.is_finite())
}

/// An orthonormal basis of the span of the rows added so far.
#[derive(Debug, Clone)]
pub(crate) struct RowBasis {
    n: usize,
    q: Vec<Vec<f64>>,
    tol: f64,
}

impl RowBasis {
    /// `tol` is the relative rank tolerance: a row is dependent when the norm of
    /// its component orthogonal to the basis is at most `tol` times its own norm.
    pub(crate) fn new(n: usize, tol: f64) -> Self {
        RowBasis { n, q: Vec::new(), tol }
    }

    pub(crate) fn rank(&self) -> usize {
        self.q.len()
    }

    /// Adds `row` (length `n`). Returns `true` when it was linearly independent of
    /// the rows added before (and so increased the rank).
    pub(crate) fn add(&mut self, row: &[f64]) -> bool {
        if row.len() != self.n || self.q.len() >= self.n {
            return false;
        }
        let norm0 = dot(row, row).sqrt();
        if not_gt(norm0, 0.0) || !norm0.is_finite() {
            return false;
        }
        let mut v: Vec<f64> = row.iter().map(|x| x / norm0).collect();
        // Two passes of modified Gram–Schmidt ("twice is enough").
        for _ in 0..2 {
            for q in &self.q {
                let c = dot(q, &v);
                if c != 0.0 {
                    for (vi, qi) in v.iter_mut().zip(q.iter()) {
                        *vi -= c * qi;
                    }
                }
            }
        }
        let nv = dot(&v, &v).sqrt();
        if not_gt(nv, self.tol) {
            return false;
        }
        for x in &mut v {
            *x /= nv;
        }
        self.q.push(v);
        true
    }

    /// Squared norm of the projection of the unit vector `e_j` onto the row space,
    /// in `[0, 1]`. Column `j` is a free direction (has a non-zero component in the
    /// null space) when this is noticeably below 1.
    pub(crate) fn coverage(&self, j: usize) -> f64 {
        let mut s = 0.0;
        for q in &self.q {
            let x = q.get(j).copied().unwrap_or(0.0);
            s += x * x;
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cholesky_solves_spd() {
        // A = [[4,2,0],[2,5,1],[0,1,3]], x = [1,2,3] -> b = [8, 15, 11]
        let mut a = vec![4.0, 2.0, 0.0, 2.0, 5.0, 1.0, 0.0, 1.0, 3.0];
        let mut b = vec![8.0, 15.0, 11.0];
        assert!(cholesky_solve(&mut a, 3, &mut b));
        for (x, e) in b.iter().zip([1.0, 2.0, 3.0]) {
            assert!((x - e).abs() < 1e-12, "{b:?}");
        }
    }

    #[test]
    fn cholesky_rejects_indefinite_and_bad_shapes() {
        let mut a = vec![1.0, 2.0, 2.0, 1.0];
        let mut b = vec![1.0, 1.0];
        assert!(!cholesky_solve(&mut a, 2, &mut b));
        let mut a = vec![f64::NAN];
        let mut b = vec![1.0];
        assert!(!cholesky_solve(&mut a, 1, &mut b));
        assert!(!cholesky_solve(&mut [1.0], 2, &mut [1.0, 1.0]));
    }

    #[test]
    fn row_basis_rank_and_coverage() {
        let mut rb = RowBasis::new(3, 1e-10);
        assert!(rb.add(&[1.0, 0.0, 0.0]));
        assert!(rb.add(&[1.0, 1.0, 0.0]));
        assert!(!rb.add(&[2.0, 3.0, 0.0]));
        assert!(!rb.add(&[0.0, 0.0, 0.0]));
        assert_eq!(rb.rank(), 2);
        assert!((rb.coverage(0) - 1.0).abs() < 1e-12);
        assert!(rb.coverage(2).abs() < 1e-12);
        assert!(rb.add(&[0.0, 1.0, 1.0]));
        assert!(!rb.add(&[5.0, 5.0, 5.0]));
    }
}
