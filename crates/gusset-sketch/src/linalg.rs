//! Small linear algebra: an envelope Cholesky solve (reverse Cuthill–McKee order) for
//! the damped normal equations and an incremental Givens row-echelon form for rank,
//! redundancy and null-space analysis.
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

/// Reverse Cuthill–McKee order of the symmetric sparsity pattern of `a` (n×n, row
/// major): `order[new] = old`. Sketch constraint graphs are chains, loops and small
/// clusters, so this keeps the factor's envelope narrow (review M4). Ties are broken
/// by index, so the order is deterministic.
fn rcm_order(a: &[f64], n: usize) -> Vec<usize> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, nb) in adj.iter_mut().enumerate() {
        for j in 0..n {
            if j != i && a.get(i * n + j).is_some_and(|v| *v != 0.0) {
                nb.push(j);
            }
        }
    }
    rcm(&adj)
}

/// Reverse Cuthill–McKee order of a graph given as adjacency lists (`order[new] = old`).
pub(crate) fn rcm(adj: &[Vec<usize>]) -> Vec<usize> {
    let n = adj.len();
    let degree = |i: usize| adj.get(i).map_or(0, Vec::len);
    let mut seen = vec![false; n];
    let mut order = Vec::with_capacity(n);
    // Each component starts at its unvisited node of smallest degree.
    while let Some(start) = (0..n).filter(|&i| !seen.get(i).copied().unwrap_or(true)).min_by_key(|&i| (degree(i), i)) {
        if let Some(f) = seen.get_mut(start) {
            *f = true;
        }
        let first = order.len();
        order.push(start);
        let mut head = first;
        while let Some(&v) = order.get(head) {
            head += 1;
            let mut next: Vec<usize> = adj
                .get(v)
                .map(|nb| nb.iter().copied().filter(|&u| !seen.get(u).copied().unwrap_or(true)).collect())
                .unwrap_or_default();
            next.sort_by_key(|&u| (degree(u), u));
            for u in next {
                if let Some(f) = seen.get_mut(u) {
                    *f = true;
                }
                order.push(u);
            }
        }
    }
    order.reverse();
    order
}

/// Solves `A x = b` for symmetric positive definite `A` (n×n, row major, overwritten)
/// in place of `b`. Returns false when `A` is not numerically positive definite or a
/// result is not finite.
///
/// The matrix is reordered with reverse Cuthill–McKee and factored as an envelope
/// (profile) Cholesky: work and fill stay inside each row's first non-zero column,
/// which for sketch systems is `O(n·b²)` with a small bandwidth `b` instead of `O(n³)`.
pub(crate) fn cholesky_solve(a: &mut [f64], n: usize, b: &mut [f64]) -> bool {
    if a.len() != n.saturating_mul(n) || b.len() != n {
        return false;
    }
    let order = rcm_order(a, n);
    // Permuted copy: p[i][j] = a[order[i]][order[j]], lower triangle only.
    let mut p = vec![0.0; n * n];
    let mut first = vec![0usize; n];
    for i in 0..n {
        let oi = order.get(i).copied().unwrap_or(i);
        let mut f = i;
        for j in 0..=i {
            let oj = order.get(j).copied().unwrap_or(j);
            let v = a.get(oi * n + oj).copied().unwrap_or(0.0);
            if v != 0.0 && j < f {
                f = j;
            }
            if let Some(x) = p.get_mut(i * n + j) {
                *x = v;
            }
        }
        if let Some(x) = first.get_mut(i) {
            *x = f;
        }
    }
    let fst = |i: usize| first.get(i).copied().unwrap_or(0);
    // Row-by-row (bordering) Cholesky within the envelope.
    for i in 0..n {
        let fi = fst(i);
        for j in fi..i {
            let lo = fi.max(fst(j));
            let s = p.get(i * n + j).copied().unwrap_or(0.0)
                - dot(p.get(i * n + lo..i * n + j).unwrap_or(&[]), p.get(j * n + lo..j * n + j).unwrap_or(&[]));
            let d = p.get(j * n + j).copied().unwrap_or(1.0);
            if let Some(x) = p.get_mut(i * n + j) {
                *x = s / d;
            }
        }
        let ri = p.get(i * n + fi..i * n + i).unwrap_or(&[]);
        let s = p.get(i * n + i).copied().unwrap_or(0.0) - dot(ri, ri);
        if not_gt(s, 0.0) {
            return false;
        }
        if let Some(x) = p.get_mut(i * n + i) {
            *x = s.sqrt();
        }
    }
    // Forward substitution on the permuted right-hand side.
    let mut y: Vec<f64> = (0..n).map(|i| b.get(order.get(i).copied().unwrap_or(i)).copied().unwrap_or(0.0)).collect();
    for i in 0..n {
        let fi = fst(i);
        let s = dot(p.get(i * n + fi..i * n + i).unwrap_or(&[]), y.get(fi..i).unwrap_or(&[]));
        let d = p.get(i * n + i).copied().unwrap_or(1.0);
        if let Some(x) = y.get_mut(i) {
            *x = (*x - s) / d;
        }
    }
    // Back substitution, column oriented so only the envelope is touched.
    for i in (0..n).rev() {
        let d = p.get(i * n + i).copied().unwrap_or(1.0);
        let xi = y.get(i).copied().unwrap_or(0.0) / d;
        if let Some(x) = y.get_mut(i) {
            *x = xi;
        }
        for k in fst(i)..i {
            let l = p.get(i * n + k).copied().unwrap_or(0.0);
            if let Some(x) = y.get_mut(k) {
                *x -= l * xi;
            }
        }
    }
    for (i, v) in y.into_iter().enumerate() {
        if let Some(x) = order.get(i).and_then(|&o| b.get_mut(o)) {
            *x = v;
        }
    }
    b.iter().all(|x| x.is_finite())
}

/// Incremental rank analysis by Givens rotations into an upper echelon matrix (review
/// M4). Rows are added in order; a row is *dependent* when what remains of it after
/// eliminating it against the rows kept so far is at most `tol` times its norm. The
/// echelon form also gives the null space, hence which unknowns stay free.
///
/// Work per row is limited to the columns between a kept row's pivot and its last
/// non-zero, so with a banded column order (reverse Cuthill–McKee over the constraint
/// graph) sketches cost `O(m·b²)` instead of the `O(m·n²)` of dense Gram–Schmidt.
#[derive(Debug, Clone)]
pub(crate) struct RowEchelon {
    n: usize,
    /// Row `k` (dense, length `n`) when a kept row has its pivot in column `k`.
    rows: Vec<Option<Vec<f64>>>,
    /// Last non-zero column of each kept row.
    last: Vec<usize>,
    rank: usize,
    tol: f64,
}

impl RowEchelon {
    pub(crate) fn new(n: usize, tol: f64) -> Self {
        RowEchelon { n, rows: vec![None; n], last: vec![0; n], rank: 0, tol }
    }

    pub(crate) fn rank(&self) -> usize {
        self.rank
    }

    /// Adds a row; returns whether it is independent of the rows added before.
    pub(crate) fn add(&mut self, row: &[f64]) -> bool {
        if row.len() != self.n {
            return false;
        }
        let norm0 = dot(row, row).sqrt();
        if not_gt(norm0, 0.0) || !norm0.is_finite() {
            return false;
        }
        let mut v: Vec<f64> = row.iter().map(|x| x / norm0).collect();
        let mut hi = v.iter().rposition(|x| *x != 0.0).unwrap_or(0);
        let mut k = v.iter().position(|x| *x != 0.0).unwrap_or(self.n);
        while k <= hi && k < self.n {
            let vk = v.get(k).copied().unwrap_or(0.0);
            if vk != 0.0
                && let Some(Some(r)) = self.rows.get_mut(k)
            {
                let a = r.get(k).copied().unwrap_or(0.0);
                let rho = a.hypot(vk);
                let (c, s) = (a / rho, vk / rho);
                let end = hi.max(self.last.get(k).copied().unwrap_or(k));
                for j in k..=end {
                    let (rj, vj) = (r.get(j).copied().unwrap_or(0.0), v.get(j).copied().unwrap_or(0.0));
                    if let Some(x) = r.get_mut(j) {
                        *x = c * rj + s * vj;
                    }
                    if let Some(x) = v.get_mut(j) {
                        *x = -s * rj + c * vj;
                    }
                }
                if let Some(x) = v.get_mut(k) {
                    *x = 0.0;
                }
                if let Some(l) = self.last.get_mut(k) {
                    *l = end;
                }
                hi = end;
            }
            k += 1;
        }
        let rest = dot(&v, &v).sqrt();
        if not_gt(rest, self.tol) || !rest.is_finite() {
            return false;
        }
        // The remainder is zero in every pivot column: it becomes the row of its first
        // non-zero column.
        let Some(p) = v.iter().position(|x| x.abs() > 0.0) else { return false };
        let last = v.iter().rposition(|x| *x != 0.0).unwrap_or(p);
        if let Some(slot) = self.rows.get_mut(p) {
            *slot = Some(v);
        }
        if let Some(l) = self.last.get_mut(p) {
            *l = last;
        }
        self.rank += 1;
        true
    }

    /// For each column: whether its unit vector has a squared projection onto the null
    /// space above `free_tol`, i.e. the unknown is not fixed by the rows.
    pub(crate) fn free_columns(&self, free_tol: f64) -> Vec<bool> {
        let n = self.n;
        // One null-space vector per non-pivot column, by back substitution.
        let mut basis: Vec<Vec<f64>> = Vec::new();
        for f in (0..n).filter(|&c| self.rows.get(c).is_some_and(Option::is_none)) {
            let mut z = vec![0.0; n];
            if let Some(x) = z.get_mut(f) {
                *x = 1.0;
            }
            for k in (0..f).rev() {
                let Some(Some(r)) = self.rows.get(k) else { continue };
                let end = self.last.get(k).copied().unwrap_or(k);
                let s = dot(r.get(k + 1..=end).unwrap_or(&[]), z.get(k + 1..=end).unwrap_or(&[]));
                let d = r.get(k).copied().unwrap_or(1.0);
                if let Some(x) = z.get_mut(k) {
                    *x = -s / d;
                }
            }
            // Orthonormalise against the vectors so far (twice, for stability).
            for _ in 0..2 {
                for q in &basis {
                    let c = dot(q, &z);
                    if c != 0.0 {
                        for (zi, qi) in z.iter_mut().zip(q.iter()) {
                            *zi -= c * qi;
                        }
                    }
                }
            }
            let nz = dot(&z, &z).sqrt();
            if nz > 0.0 && nz.is_finite() {
                z.iter_mut().for_each(|x| *x /= nz);
                basis.push(z);
            }
        }
        (0..n).map(|j| basis.iter().map(|q| q.get(j).map_or(0.0, |x| x * x)).sum::<f64>() > free_tol).collect()
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
    fn row_echelon_rank_and_free_columns() {
        let mut rb = RowEchelon::new(3, 1e-10);
        assert!(rb.add(&[1.0, 0.0, 0.0]));
        assert!(rb.add(&[1.0, 1.0, 0.0]));
        assert!(!rb.add(&[2.0, 3.0, 0.0]));
        assert!(!rb.add(&[0.0, 0.0, 0.0]));
        assert_eq!(rb.rank(), 2);
        assert_eq!(rb.free_columns(1e-9), vec![false, false, true]);
        assert!(rb.add(&[0.0, 1.0, 1.0]));
        assert!(!rb.add(&[5.0, 5.0, 5.0]));
        assert_eq!(rb.free_columns(1e-9), vec![false, false, false]);
        // A coupled free direction: x0 + x1 fixed leaves both free.
        let mut rb = RowEchelon::new(2, 1e-10);
        assert!(rb.add(&[1.0, 1.0]));
        assert_eq!(rb.free_columns(1e-9), vec![true, true]);
    }

    #[test]
    fn envelope_cholesky_matches_a_permuted_chain() {
        // Tridiagonal SPD system given in a scrambled order.
        let n = 6;
        let perm = [3usize, 0, 5, 1, 4, 2];
        let mut a = vec![0.0; n * n];
        for i in 0..n {
            a[perm[i] * n + perm[i]] = 4.0;
            if i + 1 < n {
                a[perm[i] * n + perm[i + 1]] = -1.0;
                a[perm[i + 1] * n + perm[i]] = -1.0;
            }
        }
        let x_true: Vec<f64> = (0..n).map(|i| i as f64 + 1.0).collect();
        let mut b: Vec<f64> = (0..n).map(|i| (0..n).map(|j| a[i * n + j] * x_true[j]).sum()).collect();
        assert!(cholesky_solve(&mut a.clone(), n, &mut b));
        for (x, e) in b.iter().zip(&x_true) {
            assert!((x - e).abs() < 1e-12, "{b:?}");
        }
    }
}
