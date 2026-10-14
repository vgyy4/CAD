//! Direct translations of OCCT's `math_Recipes.cxx` (LU, SVD, Jacobi, DACTCL).
//!
//! The C++ code indexes with 1-based absolute indices (`a(i, j)`); the translation keeps those
//! indices and reads/writes through the 1-based position accessors of [`Matrix`] and [`Vector`].
// The comparisons with 0 and the `x + y == x` tests are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use super::{IntegerVector, MathError, Matrix, STATUS_NO_CONVERGENCE, STATUS_OK, STATUS_SINGULAR_MATRIX, Vector};

/// `PYTHAG(a, b)`.
fn pythag(a: f64, b: f64) -> f64 {
    let at = a.abs();
    let bt = b.abs();
    let mut ct = 0.0;
    if at > bt {
        ct = bt / at;
        ct = at * (1.0 + ct * ct).sqrt();
    } else if bt != 0.0 {
        ct = at / bt;
        ct = bt * (1.0 + ct * ct).sqrt();
    }
    ct
}

/// `SIGN(a, b)`.
fn sign(a: f64, b: f64) -> f64 {
    if b >= 0.0 { a.abs() } else { -a.abs() }
}

/// `LU_Decompose(a, indx, d, TINY)`: returns `(status, a, indx, d)`.
pub fn lu_decompose(a: Matrix, tiny: f64) -> Result<(i32, Matrix, IntegerVector, f64), MathError> {
    let n = a.row_number();
    let indx = IntegerVector::filled(1, n, 0)?;
    let vv = Vector::new(1, n)?;
    let (status, a, indx, d, _) = lu_decompose_with_scratch(a, indx, 1.0, vv, tiny)?;
    Ok((status, a, indx, d))
}

/// `LU_Decompose(a, indx, d, vv, TINY)`: returns `(status, a, indx, d, vv)`.
pub fn lu_decompose_with_scratch(
    mut a: Matrix,
    mut indx: IntegerVector,
    _sign: f64,
    mut vv: Vector,
    tiny: f64,
) -> Result<(i32, Matrix, IntegerVector, f64, Vector), MathError> {
    let n = a.row_number();
    if a.col_number() != n || indx.length() < n || vv.length() < n {
        return Err(MathError::Dimension);
    }
    let mut imax = 0;
    let mut d = 1.0;
    for i in 1..=n {
        let mut big = 0.0;
        for j in 1..=n {
            let temp = a.at(i, j).abs();
            if temp > big {
                big = temp;
            }
        }
        if big <= tiny {
            return Ok((STATUS_SINGULAR_MATRIX, a, indx, d, vv));
        }
        vv.put(i, 1.0 / big);
    }
    for j in 1..=n {
        for i in 1..j {
            let mut sum = a.at(i, j);
            for k in 1..i {
                sum -= a.at(i, k) * a.at(k, j);
            }
            a.put(i, j, sum);
        }
        let mut big = 0.0;
        for i in j..=n {
            let mut sum = a.at(i, j);
            for k in 1..j {
                sum -= a.at(i, k) * a.at(k, j);
            }
            a.put(i, j, sum);
            // The comparison is made so that imax is updated even if the argument is NaN or Inf.
            let dum = vv.at(i) * sum.abs();
            if dum < big {
                continue;
            }
            big = dum;
            imax = i;
        }
        if j != imax {
            for k in 1..=n {
                let dum = a.at(imax, k);
                a.put(imax, k, a.at(j, k));
                a.put(j, k, dum);
            }
            d = -d;
            vv.put(imax, vv.at(j));
        }
        indx.put(j, imax);
        if a.at(j, j).abs() <= tiny {
            return Ok((STATUS_SINGULAR_MATRIX, a, indx, d, vv));
        }
        if j != n {
            let dum = 1.0 / a.at(j, j);
            for i in j + 1..=n {
                a.put(i, j, a.at(i, j) * dum);
            }
        }
    }
    Ok((STATUS_OK, a, indx, d, vv))
}

/// `LU_Solve(a, indx, b)`: returns the solution (the updated `b`).
pub fn lu_solve(a: &Matrix, indx: &IntegerVector, mut b: Vector) -> Result<Vector, MathError> {
    let n = a.row_number();
    let nblow = b.lower() - 1;
    if indx.length() < n || b.length() < n || a.col_number() < n {
        return Err(MathError::Dimension);
    }
    let mut ii = 0;
    for i in 1..=n {
        let ip = indx.at(i);
        let mut sum = b.value(ip + nblow)?;
        b.set_value(ip + nblow, b.value(i + nblow)?)?;
        if ii != 0 {
            for j in ii..i {
                sum -= a.at(i, j) * b.value(j + nblow)?;
            }
        } else if sum != 0.0 {
            ii = i;
        }
        b.set_value(i + nblow, sum)?;
    }
    for i in (1..=n).rev() {
        let mut sum = b.value(i + nblow)?;
        for j in i + 1..=n {
            sum -= a.at(i, j) * b.value(j + nblow)?;
        }
        b.set_value(i + nblow, sum / a.at(i, i))?;
    }
    Ok(b)
}

/// `LU_Invert(a)`: returns `(status, a)`; `a` is the inverse when the status is 0.
pub fn lu_invert(a: Matrix) -> Result<(i32, Matrix), MathError> {
    let n = a.row_number();
    let mut inv = Matrix::new(1, n, 1, n)?;
    let (error, mut a, indx, _) = lu_decompose(a, 1.0e-20)?;
    if error == 0 {
        for j in 1..=n {
            let mut col = Vector::filled(1, n, 0.0)?;
            col.put(j, 1.0);
            let col = lu_solve(&a, &indx, col)?;
            for i in 1..=n {
                inv.put(i, j, col.at(i));
            }
        }
        for j in 1..=n {
            for i in 1..=n {
                a.put(i, j, inv.at(i, j));
            }
        }
    }
    Ok((error, a))
}

/// `EigenSort(d, v)`: descending order.
fn eigen_sort(d: &mut Vector, v: &mut Matrix) {
    let n = d.length();
    for i in 1..n {
        let mut k = i;
        let mut p = d.at(i);
        for j in i + 1..=n {
            if d.at(j) >= p {
                k = j;
                p = d.at(j);
            }
        }
        if k != i {
            d.put(k, d.at(i));
            d.put(i, p);
            for j in 1..=n {
                let p = v.at(j, i);
                v.put(j, i, v.at(j, k));
                v.put(j, k, p);
            }
        }
    }
}

/// The result of [`jacobi`]: the status, the number of rotations, the (destroyed) input matrix,
/// the eigenvalues and the eigenvectors (columns).
#[derive(Debug, Clone, PartialEq)]
pub struct JacobiResult {
    /// `math_Status_OK` or `math_Status_NoConvergence`.
    pub status: i32,
    /// Number of Jacobi rotations (`nrot`).
    pub rotations: i32,
    /// The input matrix after the call (its upper triangle is modified).
    pub matrix: Matrix,
    /// The eigenvalues `d`.
    pub values: Vector,
    /// The eigenvectors `v`, one per column.
    pub vectors: Matrix,
}

/// `ROTATE(a, i, j, k, l)`.
fn rotate(a: &mut Matrix, s: f64, tau: f64, (i, j): (i32, i32), (k, l): (i32, i32)) {
    let g = a.at(i, j);
    let h = a.at(k, l);
    a.put(i, j, g - s * (h + g * tau));
    a.put(k, l, h + s * (g - h * tau));
}

/// `Jacobi(a, d, v, nrot)`: eigenvalues and eigenvectors of a symmetric matrix.
pub fn jacobi(mut a: Matrix) -> Result<JacobiResult, MathError> {
    let n = a.row_number();
    let mut d = Vector::new(1, n)?;
    let mut v = Matrix::new(1, n, 1, n)?;
    let mut b = Vector::new(1, n)?;
    let mut z = Vector::new(1, n)?;
    for ip in 1..=n {
        for iq in 1..=n {
            v.put(ip, iq, 0.0);
        }
        v.put(ip, ip, 1.0);
    }
    for ip in 1..=n {
        d.put(ip, a.at(ip, ip));
        b.put(ip, a.at(ip, ip));
        z.put(ip, 0.0);
    }
    let mut nrot = 0;
    for i in 1..=50 {
        let mut sm = 0.0;
        for ip in 1..n {
            for iq in ip + 1..=n {
                sm += a.at(ip, iq).abs();
            }
        }
        if sm == 0.0 {
            eigen_sort(&mut d, &mut v);
            return Ok(JacobiResult { status: STATUS_OK, rotations: nrot, matrix: a, values: d, vectors: v });
        }
        let tresh = if i < 4 { 0.2 * sm / f64::from(n * n) } else { 0.0 };
        for ip in 1..n {
            for iq in ip + 1..=n {
                let g = 100.0 * a.at(ip, iq).abs();
                if i > 4 && d.at(ip).abs() + g == d.at(ip).abs() && d.at(iq).abs() + g == d.at(iq).abs() {
                    a.put(ip, iq, 0.0);
                } else if a.at(ip, iq).abs() > tresh {
                    let mut h = d.at(iq) - d.at(ip);
                    let mut t;
                    if h.abs() + g == h.abs() {
                        t = a.at(ip, iq) / h;
                    } else {
                        let theta = 0.5 * h / a.at(ip, iq);
                        t = 1.0 / (theta.abs() + (1.0 + theta * theta).sqrt());
                        if theta < 0.0 {
                            t = -t;
                        }
                    }
                    let c = 1.0 / (1.0 + t * t).sqrt();
                    let s = t * c;
                    let tau = s / (1.0 + c);
                    h = t * a.at(ip, iq);
                    z.put(ip, z.at(ip) - h);
                    z.put(iq, z.at(iq) + h);
                    d.put(ip, d.at(ip) - h);
                    d.put(iq, d.at(iq) + h);
                    a.put(ip, iq, 0.0);
                    for j in 1..ip {
                        rotate(&mut a, s, tau, (j, ip), (j, iq));
                    }
                    for j in ip + 1..iq {
                        rotate(&mut a, s, tau, (ip, j), (j, iq));
                    }
                    for j in iq + 1..=n {
                        rotate(&mut a, s, tau, (ip, j), (iq, j));
                    }
                    for j in 1..=n {
                        rotate(&mut v, s, tau, (j, ip), (j, iq));
                    }
                    nrot += 1;
                }
            }
        }
        for ip in 1..=n {
            b.put(ip, b.at(ip) + z.at(ip));
            d.put(ip, b.at(ip));
            z.put(ip, 0.0);
        }
    }
    eigen_sort(&mut d, &mut v);
    Ok(JacobiResult { status: STATUS_NO_CONVERGENCE, rotations: nrot, matrix: a, values: d, vectors: v })
}

/// `SVD_Decompose(a, w, v)`: returns `(status, a (= U), w, v)`.
pub fn svd_decompose(a: Matrix) -> Result<(i32, Matrix, Vector, Matrix), MathError> {
    let n = a.col_number();
    let rv1 = Vector::new(1, n)?;
    let (status, a, w, v, _) = svd_decompose_with_scratch(a, rv1)?;
    Ok((status, a, w, v))
}

/// `SVD_Decompose(a, w, v, rv1)`: returns `(status, a (= U), w, v, rv1)`.
pub fn svd_decompose_with_scratch(
    mut a: Matrix,
    mut rv1: Vector,
) -> Result<(i32, Matrix, Vector, Matrix, Vector), MathError> {
    let n = a.col_number();
    if rv1.length() < n {
        return Err(MathError::Dimension);
    }
    let mut w = Vector::new(1, n)?;
    let mut v = Matrix::new(1, n, 1, n)?;
    let status = svd_core(&mut a, &mut w, &mut v, &mut rv1);
    Ok((status, a, w, v, rv1))
}

fn svd_core(a: &mut Matrix, w: &mut Vector, v: &mut Matrix, rv1: &mut Vector) -> i32 {
    let m = a.row_number();
    let n = a.col_number();
    let mut l: i32 = 0;
    let mut anorm = 0.0;
    let mut g = 0.0;
    let mut scale = 0.0;
    let (mut s, mut f, mut h);
    for i in 1..=n {
        l = i + 1;
        rv1.put(i, scale * g);
        g = 0.0;
        s = 0.0;
        scale = 0.0;
        if i <= m {
            for k in i..=m {
                let aki = a.at(k, i);
                if aki > 0.0 {
                    scale += aki;
                } else {
                    scale -= aki;
                }
            }
            if scale != 0.0 {
                for k in i..=m {
                    a.put(k, i, a.at(k, i) / scale);
                    s += a.at(k, i) * a.at(k, i);
                }
                f = a.at(i, i);
                g = -sign(s.sqrt(), f);
                h = f * g - s;
                a.put(i, i, f - g);
                if i != n {
                    for j in l..=n {
                        s = 0.0;
                        for k in i..=m {
                            s += a.at(k, i) * a.at(k, j);
                        }
                        f = s / h;
                        for k in i..=m {
                            a.put(k, j, a.at(k, j) + f * a.at(k, i));
                        }
                    }
                }
                for k in i..=m {
                    a.put(k, i, a.at(k, i) * scale);
                }
            }
        }
        w.put(i, scale * g);
        g = 0.0;
        s = 0.0;
        scale = 0.0;
        if i <= m && i != n {
            for k in l..=n {
                let aik = a.at(i, k);
                if aik > 0.0 {
                    scale += aik;
                } else {
                    scale -= aik;
                }
            }
            if scale != 0.0 {
                for k in l..=n {
                    a.put(i, k, a.at(i, k) / scale);
                    s += a.at(i, k) * a.at(i, k);
                }
                f = a.at(i, l);
                g = -sign(s.sqrt(), f);
                h = f * g - s;
                a.put(i, l, f - g);
                for k in l..=n {
                    rv1.put(k, a.at(i, k) / h);
                }
                if i != m {
                    for j in l..=m {
                        s = 0.0;
                        for k in l..=n {
                            s += a.at(j, k) * a.at(i, k);
                        }
                        for k in l..=n {
                            a.put(j, k, a.at(j, k) + s * rv1.at(k));
                        }
                    }
                }
                for k in l..=n {
                    a.put(i, k, a.at(i, k) * scale);
                }
            }
        }
        let mut aw = w.at(i);
        if aw < 0.0 {
            aw = -aw;
        }
        let mut ar = rv1.at(i);
        if ar > 0.0 {
            ar += aw;
        } else {
            ar = aw - ar;
        }
        if anorm < ar {
            anorm = ar;
        }
    }
    for i in (1..=n).rev() {
        if i < n {
            if g != 0.0 {
                for j in l..=n {
                    v.put(j, i, (a.at(i, j) / a.at(i, l)) / g);
                }
                for j in l..=n {
                    s = 0.0;
                    for k in l..=n {
                        s += a.at(i, k) * v.at(k, j);
                    }
                    for k in l..=n {
                        v.put(k, j, v.at(k, j) + s * v.at(k, i));
                    }
                }
            }
            for j in l..=n {
                v.put(j, i, 0.0);
                v.put(i, j, 0.0);
            }
        }
        v.put(i, i, 1.0);
        g = rv1.at(i);
        l = i;
    }
    for i in (1..=n).rev() {
        l = i + 1;
        g = w.at(i);
        if i < n {
            for j in l..=n {
                a.put(i, j, 0.0);
            }
        }
        if g != 0.0 {
            g = 1.0 / g;
            if i != n {
                for j in l..=n {
                    s = 0.0;
                    for k in l..=m {
                        s += a.at(k, i) * a.at(k, j);
                    }
                    f = (s / a.at(i, i)) * g;
                    for k in i..=m {
                        a.put(k, j, a.at(k, j) + f * a.at(k, i));
                    }
                }
            }
            for j in i..=m {
                a.put(j, i, a.at(j, i) * g);
            }
        } else {
            for j in i..=m {
                a.put(j, i, 0.0);
            }
        }
        a.put(i, i, a.at(i, i) + 1.0);
    }
    for k in (1..=n).rev() {
        for its in 1..=30 {
            let mut flag = true;
            let mut nm = 0;
            l = k;
            while l >= 1 {
                nm = l - 1;
                if rv1.at(l).abs() + anorm == anorm {
                    flag = false;
                    break;
                }
                if w.at(nm).abs() + anorm == anorm {
                    break;
                }
                l -= 1;
            }
            if flag {
                s = 1.0;
                for i in l..=k {
                    f = s * rv1.at(i);
                    if f.abs() + anorm != anorm {
                        g = w.at(i);
                        h = pythag(f, g);
                        w.put(i, h);
                        h = 1.0 / h;
                        let c = g * h;
                        s = -f * h;
                        for j in 1..=m {
                            let y = a.at(j, nm);
                            let z = a.at(j, i);
                            a.put(j, nm, y * c + z * s);
                            a.put(j, i, z * c - y * s);
                        }
                    }
                }
            }
            let mut z = w.at(k);
            if l == k {
                if z < 0.0 {
                    w.put(k, -z);
                    for j in 1..=n {
                        v.put(j, k, -v.at(j, k));
                    }
                }
                break;
            }
            if its == 30 {
                return STATUS_NO_CONVERGENCE;
            }
            let mut x = w.at(l);
            nm = k - 1;
            let mut y = w.at(nm);
            g = rv1.at(nm);
            h = rv1.at(k);
            f = ((y - z) * (y + z) + (g - h) * (g + h)) / (2.0 * h * y);
            g = pythag(f, 1.0);
            f = ((x - z) * (x + z) + h * ((y / (f + sign(g, f))) - h)) / x;
            let mut c = 1.0;
            s = 1.0;
            for j in l..=nm {
                let i = j + 1;
                g = rv1.at(i);
                y = w.at(i);
                h = s * g;
                g *= c;
                z = pythag(f, h);
                rv1.put(j, z);
                c = f / z;
                s = h / z;
                f = x * c + g * s;
                g = g * c - x * s;
                h = y * s;
                y *= c;
                for jj in 1..=n {
                    x = v.at(jj, j);
                    z = v.at(jj, i);
                    v.put(jj, j, x * c + z * s);
                    v.put(jj, i, z * c - x * s);
                }
                z = pythag(f, h);
                w.put(j, z);
                if z != 0.0 {
                    z = 1.0 / z;
                    c = f * z;
                    s = h * z;
                }
                f = (c * g) + (s * y);
                x = (c * y) - (s * g);
                for jj in 1..=m {
                    y = a.at(jj, j);
                    z = a.at(jj, i);
                    a.put(jj, j, y * c + z * s);
                    a.put(jj, i, z * c - y * s);
                }
            }
            rv1.put(l, 0.0);
            rv1.put(k, f);
            w.put(k, x);
        }
    }
    STATUS_OK
}

/// `SVD_Solve(u, w, v, b, x)`: returns `x` (bounds `1..=n`).
pub fn svd_solve(u: &Matrix, w: &Vector, v: &Matrix, b: &Vector) -> Result<Vector, MathError> {
    let m = u.row_number();
    let n = u.col_number();
    if w.length() < n || v.row_number() < n || v.col_number() < n || b.length() < m {
        return Err(MathError::Dimension);
    }
    let mut tmp = Vector::new(1, n)?;
    let mut x = Vector::new(1, n)?;
    for j in 1..=n {
        let mut s = 0.0;
        if w.at(j) != 0.0 {
            for i in 1..=m {
                s += u.at(i, j) * b.at(i);
            }
            s /= w.at(j);
        }
        tmp.put(j, s);
    }
    for j in 1..=n {
        let mut s = 0.0;
        for jj in 1..=n {
            s += v.at(j, jj) * tmp.at(jj);
        }
        x.put(j, s);
    }
    Ok(x)
}

/// `DACTCL_Decompose(a, indx, MinPivot)`: returns `(status, a)`.
pub fn dactcl_decompose(mut a: Vector, indx: IntegerVector, min_pivot: f64) -> Result<(i32, Vector), MathError> {
    let neq = indx.length();
    let mut jr = 0;
    for j in 1..=neq {
        let mut diag = false;
        let jd = indx.at(j);
        let jh = jd - jr;
        let is = j - jh + 2;
        if jh - 2 == 0 {
            diag = true;
        }
        if jh - 2 > 0 {
            let ie = j - 1;
            let k0 = jr + 2;
            let mut id = indx.at(is - 1);
            // Reduction of the off-diagonal coefficients.
            for i in is..=ie {
                let ir = id;
                id = indx.at(i);
                let mut ih = id - ir - 1;
                let mh = i - is + 1;
                if ih > mh {
                    ih = mh;
                }
                let k = k0 + (i - is);
                if ih > 0 {
                    let mut dot = 0.0;
                    let idot1 = k - ih - 1;
                    let idot2 = id - ih - 1;
                    for idot in 1..=ih {
                        dot += a.at(idot1 + idot) * a.at(idot2 + idot);
                    }
                    a.put(k, a.at(k) - dot);
                }
            }
            diag = true;
        }
        if diag {
            // Reduction of the diagonal coefficients.
            let ir = jr + 1;
            let ie = jd - 1;
            let k = j - jd;
            for i in ir..=ie {
                let id = indx.at(k + i);
                let mut aa = a.at(id);
                if aa < 0.0 {
                    aa = -aa;
                }
                if aa <= min_pivot {
                    return Ok((STATUS_SINGULAR_MATRIX, a));
                }
                let d = a.at(i);
                a.put(i, d / a.at(id));
                a.put(jd, a.at(jd) - d * a.at(i));
            }
        }
        jr = jd;
    }
    Ok((STATUS_OK, a))
}

/// `DACTCL_Solve(a, b, indx, MinPivot)`: returns `(status, b)`.
pub fn dactcl_solve(
    a: &Vector,
    mut b: Vector,
    indx: IntegerVector,
    min_pivot: f64,
) -> Result<(i32, Vector), MathError> {
    let neq = indx.length();
    let mut jr = 0;
    for j in 1..=neq {
        let jd = indx.at(j);
        let jh = jd - jr;
        let is = j - jh + 2;
        // Reduction of the right-hand side.
        let mut dot = 0.0;
        let idot1 = jr;
        let idot2 = is - 2;
        let jh1 = jh - 1;
        for idot in 1..=jh1 {
            dot += a.at(idot1 + idot) * b.at(idot2 + idot);
        }
        b.put(j, b.at(j) - dot);
        jr = jd;
    }
    // Division by the diagonal pivots.
    for i in 1..=neq {
        let id = indx.at(i);
        let mut aa = a.at(id);
        if aa < 0.0 {
            aa = -aa;
        }
        if aa <= min_pivot {
            return Ok((STATUS_SINGULAR_MATRIX, b));
        }
        b.put(i, b.at(i) / a.at(id));
    }
    // Back substitution.
    let mut jd = indx.at(neq);
    for j in (1..neq).rev() {
        let d = b.at(j + 1);
        let jr = indx.at(j);
        if jd - jr > 1 {
            let is = j - jd + jr + 2;
            let k = jr - is + 1;
            for i in is..=j {
                b.put(i, b.at(i) - a.at(i + k) * d);
            }
        }
        jd = jr;
    }
    Ok((STATUS_OK, b))
}
