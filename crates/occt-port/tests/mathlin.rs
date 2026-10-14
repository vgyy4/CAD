//! Differential judge for OCCT's `math` package (OCCT 8.0.1).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::mathlin::{self, DirectPolynomialRoots, Matrix, Vector};

fn f(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap_or_else(|e| panic!("{s}: {e}")))
}
fn ulps(a: f64, b: f64) -> u64 {
    let key = |x: f64| {
        let q = x.to_bits() as i64;
        if q < 0 { i64::MIN - q } else { q }
    };
    key(a).abs_diff(key(b))
}
const MAX_ULPS: f64 = 8.0;
#[derive(Default)]
struct Tally {
    values: usize,
    exact: usize,
    worst: u64,
    lines: usize,
}
fn compare(t: &mut Tally, line: &str, want: &[f64], got: &[f64], libm: bool) {
    assert_eq!(want.len(), got.len(), "{line}");
    let scale = want.iter().fold(1.0_f64, |a, x| a.max(x.abs()));
    let tol = if libm { MAX_ULPS * f64::EPSILON * scale } else { 0.0 };
    for (i, (w, g)) in want.iter().zip(got).enumerate() {
        let d = ulps(*w, *g);
        assert!((w - g).abs() <= tol, "value {i}: want {w:e}, got {g:e} ({d} ulps)\n{line}");
        t.values += 1;
        t.exact += usize::from(d == 0);
        t.worst = t.worst.max(d);
    }
}
fn check(t: &mut Tally, line: &str, want: &[f64], got: &[f64]) {
    compare(t, line, want, got, false)
}
fn check_libm(t: &mut Tally, line: &str, want: &[f64], got: &[f64]) {
    compare(t, line, want, got, true)
}
fn take_vec(w: &[&str], p: &mut usize) -> Vec<f64> {
    let lo: i32 = w[*p].parse().unwrap();
    let hi: i32 = w[*p + 1].parse().unwrap();
    *p += 2;
    assert!(hi >= lo);
    let n = (hi - lo + 1) as usize;
    let x = w[*p..*p + n].iter().map(|q| f(q)).collect();
    *p += n;
    x
}
fn take_vec_int(w: &[&str], p: &mut usize) -> Vec<f64> {
    let lo: i32 = w[*p].parse().unwrap();
    let hi: i32 = w[*p + 1].parse().unwrap();
    *p += 2;
    let n = (hi - lo + 1) as usize;
    let out = w[*p..*p + n].iter().map(|s| s.parse::<i32>().unwrap() as f64).collect();
    *p += n;
    out
}
fn take_integer_vector(w: &[&str], p: &mut usize) -> Vec<f64> {
    let lo: i32 = w[*p].parse().unwrap();
    let hi: i32 = w[*p + 1].parse().unwrap();
    *p += 2;
    assert!(hi >= lo);
    let n = (hi - lo + 1) as usize;
    let values = w[*p..*p + n].iter().map(|s| s.parse::<i32>().unwrap() as f64).collect::<Vec<_>>();
    *p += n;
    values
}
fn take_mat(w: &[&str], p: &mut usize) -> Vec<f64> {
    let lr: i32 = w[*p].parse().unwrap();
    let ur: i32 = w[*p + 1].parse().unwrap();
    let lc: i32 = w[*p + 2].parse().unwrap();
    let uc: i32 = w[*p + 3].parse().unwrap();
    *p += 4;
    let n = ((ur - lr + 1) * (uc - lc + 1)) as usize;
    let x = w[*p..*p + n].iter().map(|q| f(q)).collect();
    *p += n;
    x
}
fn vals(v: &Vector) -> Vec<f64> {
    (v.lower()..=v.upper()).map(|i| v.value(i).unwrap()).collect()
}
fn mats(m: &Matrix) -> Vec<f64> {
    let mut x = Vec::new();
    for r in m.lower_row()..=m.upper_row() {
        for c in m.lower_col()..=m.upper_col() {
            x.push(m.value(r, c).unwrap());
        }
    }
    x
}
fn vector(w: &[&str], p: &mut usize) -> Vector {
    let lo: i32 = w[*p].parse().unwrap();
    let hi: i32 = w[*p + 1].parse().unwrap();
    *p += 2;
    let x = w[*p..*p + (hi - lo + 1) as usize].iter().map(|q| f(q)).collect::<Vec<_>>();
    *p += x.len();
    Vector::from_slice(lo, &x).unwrap()
}
fn matrix(w: &[&str], p: &mut usize) -> Matrix {
    let lr: i32 = w[*p].parse().unwrap();
    let ur: i32 = w[*p + 1].parse().unwrap();
    let lc: i32 = w[*p + 2].parse().unwrap();
    let uc: i32 = w[*p + 3].parse().unwrap();
    *p += 4;
    let x = w[*p..*p + ((ur - lr + 1) * (uc - lc + 1)) as usize].iter().map(|q| f(q)).collect::<Vec<_>>();
    *p += x.len();
    Matrix::from_slice(lr, lc, ur - lr + 1, uc - lc + 1, &x).unwrap()
}
#[test]
fn mathlin_matches_occt() {
    let text = include_str!("mathlin.txt");
    let mut unique = std::collections::HashSet::new();
    let mut families = std::collections::HashMap::<&str, usize>::new();
    for line in text.lines() {
        assert!(unique.insert(line), "duplicate oracle record: {line}");
        *families.entry(line.split_whitespace().next().unwrap()).or_default() += 1;
    }
    for (family, count) in &families {
        assert!(*count >= 20, "family {family} has only {count} cases");
    }
    let mut t = Tally::default();
    for line in text.lines() {
        let mut halves = line.split(" | ");
        let lhs = halves.next().unwrap();
        let rhs = halves.next().unwrap_or("");
        let a = lhs.split_whitespace().collect::<Vec<_>>();
        let b = rhs.split_whitespace().collect::<Vec<_>>();
        let mut p: usize;
        let mut q = 0usize;
        let mut got = Vec::new();
        let mut want = Vec::new();
        match a[0] {
            "limits" => {
                let n = a[1].parse::<i32>().unwrap();
                assert_eq!(b[0].parse::<i32>().unwrap(), mathlin::gauss_points_max(), "{line}");
                assert_eq!(b[1].parse::<i32>().unwrap(), mathlin::kronrod_points_max(), "{line}");
                assert_eq!(b[2] == "1", mathlin::ordered_gauss_points_and_weights(n, 1).unwrap().is_some(), "{line}");
            }
            "ordered_gauss" => {
                let n = a[1].parse::<i32>().unwrap();
                let ok = a[2] == "1";
                let pair = mathlin::ordered_gauss_points_and_weights(n, 1).unwrap();
                assert_eq!(pair.is_some(), ok, "{line}");
                if let Some((x, y)) = pair {
                    q = 0;
                    want.extend(take_vec(&b, &mut q));
                    want.extend(take_vec(&b, &mut q));
                    got.extend(vals(&x));
                    got.extend(vals(&y));
                    if n > mathlin::gauss_points_max() {
                        check_libm(&mut t, line, &want, &got);
                    } else {
                        check(&mut t, line, &want, &got);
                    }
                }
            }
            "gauss_table" => {
                let n = a[1].parse::<i32>().unwrap();
                assert!(n <= mathlin::gauss_points_max(), "{line}");
                let (x, y) = (mathlin::gauss_points(n, 1).unwrap(), mathlin::gauss_weights(n, 1).unwrap());
                want.extend(take_vec(&b, &mut q));
                want.extend(take_vec(&b, &mut q));
                got.extend(vals(&x));
                got.extend(vals(&y));
                check(&mut t, line, &want, &got);
            }
            "kronrod" => {
                let n = a[1].parse::<i32>().unwrap();
                let pair = mathlin::kronrod_points_and_weights(n, 1);
                assert_eq!(pair.is_some(), a[2] == "1", "{line}");
                if let Some((x, y)) = pair {
                    q = 0;
                    want.extend(take_vec(&b, &mut q));
                    want.extend(take_vec(&b, &mut q));
                    got.extend(vals(&x));
                    got.extend(vals(&y));
                    if n > mathlin::kronrod_points_max() {
                        check_libm(&mut t, line, &want, &got);
                    } else {
                        check(&mut t, line, &want, &got);
                    }
                }
            }
            "compute_gauss" | "compute_kronrod" => {
                let n = a[1].parse::<i32>().unwrap();
                let isg = a[0] == "compute_gauss";
                let (done, x, y) = if isg {
                    let z = mathlin::ComputeGaussPointsAndWeights::new(n);
                    (z.is_done(), z.points(), z.weights())
                } else {
                    let z = mathlin::ComputeKronrodPointsAndWeights::new(n);
                    (z.is_done(), z.points(), z.weights())
                };
                assert_eq!(done, a[2] == "1", "{line}");
                if done {
                    want.extend(take_vec(&b, &mut q));
                    want.extend(take_vec(&b, &mut q));
                    got.extend(vals(&x));
                    got.extend(vals(&y));
                    if !isg && n > mathlin::kronrod_points_max() {
                        check_libm(&mut t, line, &want, &got);
                    } else {
                        check(&mut t, line, &want, &got);
                    }
                }
            }
            "vector" => {
                let k = a[1].parse::<i32>().unwrap();
                p = 2;
                let x = vector(&a, &mut p);
                let y = vector(&a, &mut p);
                let mut want_float = vec![f(b[0]), f(b[1])];
                let max: i32 = b[2].parse().unwrap();
                let min: i32 = b[3].parse().unwrap();
                assert_eq!(x.max_index(), max, "{line}");
                assert_eq!(x.min_index(), min, "{line}");
                q = 4;
                for _ in 0..4 {
                    want_float.extend(take_vec(&b, &mut q));
                }
                let dot = f(b[q]);
                q += 1;
                want_float.push(dot);
                let nvec = if k == 0 { 7 } else { 8 };
                for _ in 0..nvec {
                    want_float.extend(take_vec(&b, &mut q));
                }
                let mut z = x.clone();
                z.add(&y).unwrap();
                got.extend([x.norm(), x.norm2()]);
                got.extend(vals(&x.added(&y).unwrap()));
                got.extend(vals(&x.subtracted(&y).unwrap()));
                got.extend(vals(&x.multiplied(-2.)));
                got.extend(vals(&x.divided(2.).unwrap()));
                got.push(x.dot(&y).unwrap());
                got.extend(vals(&x.inverse()));
                got.extend(vals(&x.opposite()));
                got.extend(vals(&z));
                z = x.clone();
                z.subtract(&y).unwrap();
                got.extend(vals(&z));
                z = x.clone();
                z.multiply(-2.);
                got.extend(vals(&z));
                z = x.clone();
                z.divide(2.).unwrap();
                got.extend(vals(&z));
                z = x.clone();
                z.invert();
                got.extend(vals(&z));
                if k == 0 {
                    assert!(x.normalized().is_err(), "{line}");
                } else {
                    z = x.normalized().unwrap();
                    got.extend(vals(&z));
                }
                // This record contains all values in the order above; exceptional zero normalization is encoded as `throws`.
                if k == 0 {
                    assert_eq!(b.last().copied(), Some("throws"), "{line}");
                }
                check(&mut t, line, &want_float, &got);
            }
            "matrix_ops" => {
                p = 2;
                let x = matrix(&a, &mut p);
                let y = matrix(&a, &mut p);
                let mut w = Vec::new();
                assert_eq!(b[0].parse::<i32>().unwrap(), x.row_number(), "{line}");
                assert_eq!(b[1].parse::<i32>().unwrap(), x.col_number(), "{line}");
                q = 2;
                for _ in 0..8 {
                    w.extend(take_mat(&b, &mut q));
                }
                got.extend(mats(&x.added(&y).unwrap()));
                got.extend(mats(&x.subtracted(&y).unwrap()));
                got.extend(mats(&x.multiplied(-2.)));
                got.extend(mats(&x.divided(2.).unwrap()));
                let mut z = x.clone();
                z.add(&y).unwrap();
                got.extend(mats(&z));
                z = x.clone();
                z.subtract(&y).unwrap();
                got.extend(mats(&z));
                z = x.clone();
                z.multiply(-2.);
                got.extend(mats(&z));
                z = x;
                z.divide(2.).unwrap();
                got.extend(mats(&z));
                check(&mut t, line, &w, &got);
            }
            "matrix_linear" => {
                p = 2;
                let x = matrix(&a, &mut p);
                let rhs = vector(&a, &mut p);
                want.push(f(b[0]));
                q = 1;
                want.extend(take_vec(&b, &mut q));
                for _ in 0..4 {
                    want.extend(take_mat(&b, &mut q));
                }
                got.push(x.determinant());
                got.extend(vals(&x.multiply_vector(&rhs).unwrap()));
                got.extend(mats(&x.transposed().unwrap()));
                got.extend(mats(&x.t_multiply(&x).unwrap()));
                got.extend(mats(&x.multiplied_matrix(&x).unwrap()));
                got.extend(mats(&x.inverse().unwrap()));
                check(&mut t, line, &want, &got);
            }
            "lu" => {
                p = 2;
                let x = matrix(&a, &mut p);
                let (st, l, ix, sign) = mathlin::recipes::lu_decompose(x, 1e-20).unwrap();
                assert_eq!(b[0].parse::<i32>().unwrap(), st, "{line}");
                assert_eq!(b[1], format!("{:016x}", sign.to_bits()), "{line}");
                let mut w = Vec::new();
                q = 2;
                w.extend(take_mat(&b, &mut q));
                w.extend(take_vec_int(&b, &mut q));
                got.extend(mats(&l));
                got.extend((ix.lower()..=ix.upper()).map(|k| f64::from(ix.value(k).unwrap())));
                check(&mut t, line, &w, &got);
            }
            "lu_solve" => {
                p = 2;
                let lu = matrix(&a, &mut p);
                let lo: i32 = a[p].parse().unwrap();
                let hi: i32 = a[p + 1].parse().unwrap();
                p += 2;
                let n = (hi - lo + 1) as usize;
                let indx = a[p..p + n].iter().map(|s| s.parse::<i32>().unwrap()).collect::<Vec<_>>();
                p += n;
                let ix = mathlin::IntegerVector::from_slice(lo, &indx).unwrap();
                let rhs = vector(&a, &mut p);
                let solved = mathlin::recipes::lu_solve(&lu, &ix, rhs).unwrap();
                want.extend(take_vec(&b, &mut q));
                got.extend(vals(&solved));
                check(&mut t, line, &want, &got);
            }
            "svd" => {
                let _ = a[1];
                p = 2;
                let input = matrix(&a, &mut p);
                let rhs = vector(&a, &mut p);
                let (st, u, w, v) = mathlin::recipes::svd_decompose(input).unwrap();
                assert_eq!(b[0].parse::<i32>().unwrap(), st, "{line}");
                let x = mathlin::recipes::svd_solve(&u, &w, &v, &rhs).unwrap();
                q = 1;
                want.extend(take_mat(&b, &mut q));
                want.extend(take_vec(&b, &mut q));
                want.extend(take_mat(&b, &mut q));
                want.extend(take_vec(&b, &mut q));
                got.extend(mats(&u));
                got.extend(vals(&w));
                got.extend(mats(&v));
                got.extend(vals(&x));
                check(&mut t, line, &want, &got);
            }
            "jacobi" => {
                let _ = a[1];
                p = 2;
                let input = matrix(&a, &mut p);
                let res = mathlin::recipes::jacobi(input).unwrap();
                assert_eq!(res.status, b[0].parse::<i32>().unwrap(), "{line}");
                assert_eq!(res.rotations, b[1].parse::<i32>().unwrap(), "{line}");
                q = 2;
                want.extend(take_vec(&b, &mut q));
                want.extend(take_mat(&b, &mut q));
                want.extend(take_mat(&b, &mut q));
                got.extend(vals(&res.values));
                got.extend(mats(&res.vectors));
                got.extend(mats(&res.matrix));
                check(&mut t, line, &want, &got);
            }
            "poly3" | "poly2" => {
                let obj = if a[0] == "poly2" {
                    DirectPolynomialRoots::new_quadratic(f(a[2]), f(a[3]), f(a[4]))
                } else {
                    DirectPolynomialRoots::new_cubic(f(a[2]), f(a[3]), f(a[4]), f(a[5]))
                };
                let done = b[0] == "1";
                assert_eq!(obj.is_done(), done, "{line}");
                assert_eq!(obj.infinite_roots(), b[1] == "1", "{line}");
                let count = obj.number_of_solutions().unwrap();
                assert_eq!(count, b[2].parse::<i32>().unwrap(), "{line}");
                let mut w = Vec::new();
                for token in &b[3..] {
                    w.push(f(token));
                }
                let mut g = Vec::new();
                for i in 1..=count {
                    g.push(obj.value(i).unwrap());
                }
                if a[0] == "poly3" {
                    check_libm(&mut t, line, &w, &g);
                } else {
                    check(&mut t, line, &w, &g);
                }
            }
            "doubletab" => {
                let mut tab = mathlin::DoubleTab::new(-1, 1, 2, 4).unwrap();
                tab.init(0.0);
                q = 0;
                let mut input_pos = 8usize;
                for r in -1..=1 {
                    for c in 2..=4 {
                        tab.set_value(r, c, f(a[input_pos])).unwrap();
                        want.push(f(b[q]));
                        input_pos += 1;
                        q += 1;
                    }
                }
                assert_eq!(tab.lower_row(), a[1].parse::<i32>().unwrap());
                assert_eq!(tab.upper_row(), a[2].parse::<i32>().unwrap());
                assert_eq!(tab.lower_col(), a[3].parse::<i32>().unwrap());
                assert_eq!(tab.upper_col(), a[4].parse::<i32>().unwrap());
                assert_eq!(tab.row_number(), a[5].parse::<i32>().unwrap());
                assert_eq!(tab.col_number(), a[6].parse::<i32>().unwrap());
                assert_eq!(tab.is_deletable(), a[7] == "1");
                for r in -1..=1 {
                    for c in 2..=4 {
                        got.push(tab.value(r, c).unwrap());
                    }
                }
                tab.set_lower_row(1);
                tab.set_lower_col(-2);
                assert_eq!(tab.lower_row(), b[q].parse::<i32>().unwrap());
                q += 1;
                assert_eq!(tab.lower_col(), b[q].parse::<i32>().unwrap());
                q += 1;
                got.push(tab.value(1, -2).unwrap());
                want.push(f(b[q]));
                check(&mut t, line, &want, &got);
            }
            "matrix_extra" => {
                p = 2;
                let x = matrix(&a, &mut p);
                let y = matrix(&a, &mut p);
                let vx = vector(&a, &mut p);
                let vy = vector(&a, &mut p);
                q = 0;
                let mut w = Vec::new();
                for _ in 0..7 {
                    w.extend(take_mat(&b, &mut q));
                }
                let mut z = Matrix::filled(1, 3, 1, 3, 0.).unwrap();
                z.add_from(&x, &y).unwrap();
                got.extend(mats(&z));
                z.subtract_from(&x, &y).unwrap();
                got.extend(mats(&z));
                z.multiply_from(&x, &y).unwrap();
                got.extend(mats(&z));
                z.t_multiply_from(&x, &y).unwrap();
                got.extend(mats(&z));
                z.outer_product_from(&vx, &vy).unwrap();
                got.extend(mats(&z));
                z.set_row(2, &vy).unwrap();
                got.extend(mats(&z));
                z.set_col(1, &vx).unwrap();
                got.extend(mats(&z));
                got.extend(vals(&z.row(2).unwrap()));
                got.extend(vals(&z.col(1).unwrap()));
                z.swap_row(1, 3).unwrap();
                z.swap_col(1, 3).unwrap();
                z.set_diag(7.0).unwrap();
                got.extend(mats(&z));
                got.extend(mats(&x.opposite()));
                w.extend(take_vec(&b, &mut q));
                w.extend(take_vec(&b, &mut q));
                w.extend(take_mat(&b, &mut q));
                w.extend(take_mat(&b, &mut q));
                check(&mut t, line, &w, &got);
            }
            "solver_objects" => {
                p = 2;
                let x = matrix(&a, &mut p);
                let rhs = vector(&a, &mut p);
                let mut w = Vec::new();
                let mut q = 0;
                let g = mathlin::Gauss::new(&x, 1e-20).unwrap();
                assert_eq!(g.is_done(), b[q] == "1");
                q += 1;
                w.extend(take_vec(&b, &mut q));
                let sol = g.solve(&rhs).unwrap();
                got.extend(vals(&sol));
                w.extend(take_vec(&b, &mut q));
                let mut in_place = sol.clone();
                in_place = g.solve_in_place(in_place).unwrap();
                got.extend(vals(&in_place));
                w.push(f(b[q]));
                q += 1;
                got.push(g.determinant());
                w.extend(take_mat(&b, &mut q));
                let mut inv = Matrix::filled(1, 3, 1, 3, 0.).unwrap();
                g.invert_into(&mut inv).unwrap();
                got.extend(mats(&inv));
                let cr = mathlin::Crout::new(&x, 1e-20).unwrap();
                assert_eq!(cr.is_done(), b[q] == "1");
                q += 1;
                w.extend(take_vec(&b, &mut q));
                got.extend(vals(&cr.solve(&rhs).unwrap()));
                w.push(f(b[q]));
                q += 1;
                got.push(cr.determinant());
                w.extend(take_mat(&b, &mut q));
                got.extend(mats(&cr.inverse().unwrap()));
                w.extend(take_mat(&b, &mut q));
                cr.invert_into(&mut inv).unwrap();
                got.extend(mats(&inv));
                let ls = mathlin::GaussLeastSquare::new(&x, 1e-20).unwrap();
                assert_eq!(ls.is_done(), b[q] == "1");
                q += 1;
                w.extend(take_vec(&b, &mut q));
                got.extend(vals(&ls.solve(&rhs).unwrap()));
                let sv = mathlin::Svd::new(&x).unwrap();
                assert_eq!(sv.is_done(), b[q] == "1");
                q += 1;
                w.extend(take_vec(&b, &mut q));
                got.extend(vals(&sv.solve(&rhs, 1e-6).unwrap()));
                w.extend(take_mat(&b, &mut q));
                got.extend(mats(&sv.pseudo_inverse(1e-6).unwrap()));
                check(&mut t, line, &w, &got);
            }
            "householder" | "eigen_searcher" | "poly_ctor" => {
                // The dedicated calls below validate completion, indices, and returned values.
                if a[0] == "householder" {
                    let s = f(a[1]);
                    let aa = Matrix::from_slice(1, 1, 3, 2, &[s, s, s, 2. * s, s, 3. * s]).unwrap();
                    let bb = Matrix::from_slice(1, 1, 3, 1, &[s, 4. * s, 9. * s]).unwrap();
                    let hh = mathlin::Householder::new_matrix(&aa, &bb, 1e-20).unwrap();
                    let hv = mathlin::Householder::new_vector(
                        &aa,
                        &Vector::from_slice(1, &[s, 4. * s, 9. * s]).unwrap(),
                        1e-20,
                    )
                    .unwrap();
                    let hr = mathlin::Householder::new_matrix_with_range(&aa, &bb, 1, 3, 1, 2, 1e-20).unwrap();
                    assert_eq!(hh.is_done(), b[0] == "1");
                    assert_eq!(hv.is_done(), b[1] == "1");
                    assert_eq!(hr.is_done(), b[2] == "1");
                    q = 3;
                    want.extend(take_mat(&b, &mut q));
                    want.extend(take_vec(&b, &mut q));
                    want.extend(take_mat(&b, &mut q));
                    want.extend(take_mat(&b, &mut q));
                    got.extend(mats(&hh.all_values().unwrap()));
                    got.extend(vals(&hh.value(1).unwrap()));
                    got.extend(mats(&hv.all_values().unwrap()));
                    got.extend(mats(&hr.all_values().unwrap()));
                } else if a[0] == "eigen_searcher" {
                    let d = a[1..4].iter().map(|s| f(s)).collect::<Vec<_>>();
                    let off = a[4..7].iter().map(|s| f(s)).collect::<Vec<_>>();
                    let e = mathlin::EigenValuesSearcher::new(&d, &off, 1).unwrap();
                    assert_eq!(e.is_done(), b[0] == "1");
                    assert_eq!(e.dimension(), b[1].parse::<i32>().unwrap());
                    q = 2;
                    for j in 1..=e.dimension() {
                        want.push(f(b[q]));
                        q += 1;
                        want.extend(take_vec(&b, &mut q));
                        got.push(e.eigen_value(j).unwrap());
                        got.extend(vals(&e.eigen_vector(j).unwrap()));
                    }
                } else {
                    let k = a[1].parse::<usize>().unwrap();
                    let aa = f(a[2]);
                    let bb = f(a[3]);
                    let cc = f(a[4]);
                    let dd = f(a[5]);
                    let ee = f(a[6]);
                    let obj = match k % 4 {
                        0 => DirectPolynomialRoots::new_linear(aa, bb),
                        1 => DirectPolynomialRoots::new_quadratic(aa, bb, cc),
                        2 => DirectPolynomialRoots::new_cubic(aa, bb, cc, dd),
                        _ => DirectPolynomialRoots::new_quartic(aa, bb, cc, dd, ee),
                    };
                    assert_eq!(obj.is_done(), b[0] == "1");
                    assert_eq!(obj.infinite_roots(), b[1] == "1");
                    let n = obj.number_of_solutions().unwrap();
                    assert_eq!(n, b[2].parse::<i32>().unwrap());
                    for j in 1..=n {
                        want.push(f(b[2 + j as usize]));
                        got.push(obj.value(j).unwrap());
                    }
                }
                if a[0] == "poly_ctor" {
                    check_libm(&mut t, line, &want, &got);
                } else {
                    check(&mut t, line, &want, &got);
                }
            }
            "integer_vector" => {
                let xv = a[4..9].iter().map(|s| s.parse::<i32>().unwrap()).collect::<Vec<_>>();
                let k = xv[0] - 4;
                let x = mathlin::IntegerVector::from_slice(-2, &xv).unwrap();
                let yv = (-2..=2).map(|j| j - k).collect::<Vec<_>>();
                let y = mathlin::IntegerVector::from_slice(-2, &yv).unwrap();
                assert_eq!(a[1].parse::<i32>().unwrap(), x.length());
                assert_eq!(a[2].parse::<i32>().unwrap(), x.lower());
                assert_eq!(a[3].parse::<i32>().unwrap(), x.upper());
                for j in -2_i32..=2_i32 {
                    assert_eq!(a[4 + (j + 2) as usize].parse::<i32>().unwrap(), x.value(j).unwrap());
                }
                q = 0;
                let mut expected = Vec::new();
                for _ in 0..15 {
                    expected.push(b[q].parse::<i32>().unwrap());
                    q += 1;
                }
                assert_eq!(b[q].parse::<i32>().unwrap(), -2);
                q += 1;
                assert_eq!(b[q].parse::<i32>().unwrap(), 0);
                q += 1;
                let mut z = mathlin::IntegerVector::filled(-2, 2, 4).unwrap();
                for j in -2..=2 {
                    got.push(z.value(j).unwrap() as f64);
                } // Init
                z = x.clone();
                z.add(&y).unwrap();
                for j in -2..=2 {
                    got.push(z.value(j).unwrap() as f64);
                }
                z = x.clone();
                z.subtract(&y).unwrap();
                for j in -2..=2 {
                    got.push(z.value(j).unwrap() as f64);
                }
                z.resize(3).unwrap();
                assert_eq!(z.lower(), -2);
                assert_eq!(z.upper(), 0);
                for j in -2..=0 {
                    got.push(z.value(j).unwrap() as f64);
                }
                want.extend(expected.into_iter().map(f64::from));
                want.extend(b[q..].iter().map(|s| s.parse::<i32>().unwrap() as f64));
                check(&mut t, line, &want, &got);
            }
            "integer_vector_ops" => {
                p = 1;
                let xv = take_integer_vector(&a, &mut p).iter().map(|x| *x as i32).collect::<Vec<_>>();
                let yv = take_integer_vector(&a, &mut p).iter().map(|x| *x as i32).collect::<Vec<_>>();
                let x = mathlin::IntegerVector::from_slice(-2, &xv).unwrap();
                let y = mathlin::IntegerVector::from_slice(-2, &yv).unwrap();
                q = 0;
                want.push(f(b[q]));
                want.push(f(b[q + 1]));
                q += 2;
                assert_eq!(b[q].parse::<i32>().unwrap(), x.max_index());
                q += 1;
                assert_eq!(b[q].parse::<i32>().unwrap(), x.min_index());
                q += 1;
                for _ in 0..5 {
                    want.extend(take_integer_vector(&b, &mut q));
                }
                want.push(b[q].parse::<i32>().unwrap() as f64);
                q += 1;
                for _ in 0..4 {
                    want.extend(take_integer_vector(&b, &mut q));
                }
                got.extend([x.norm(), x.norm2()]);
                let mut z = x.clone();
                z.invert();
                for j in -2..=2 {
                    got.push(z.value(j).unwrap() as f64);
                }
                z = x.inverse();
                for j in -2..=2 {
                    got.push(z.value(j).unwrap() as f64);
                }
                z = x.opposite();
                for j in -2..=2 {
                    got.push(z.value(j).unwrap() as f64);
                }
                z = x.multiplied(-2);
                for j in -2..=2 {
                    got.push(z.value(j).unwrap() as f64);
                }
                z = x.divided(2).unwrap();
                for j in -2..=2 {
                    got.push(z.value(j).unwrap() as f64);
                }
                got.push(x.dot(&y).unwrap() as f64);
                z = x.slice(-1, 1).unwrap();
                for j in z.lower()..=z.upper() {
                    got.push(z.value(j).unwrap() as f64);
                }
                z = x.clone();
                z.multiply(3);
                for j in -2..=2 {
                    got.push(z.value(j).unwrap() as f64);
                }
                z = x.clone();
                z.divide(2).unwrap();
                for j in -2..=2 {
                    got.push(z.value(j).unwrap() as f64);
                }
                z.set_slice(-1, 1, &y.slice(-1, 1).unwrap()).unwrap();
                for j in -2..=2 {
                    got.push(z.value(j).unwrap() as f64);
                }
                check(&mut t, line, &want, &got);
            }
            "vector_extra" => {
                p = 1;
                let x = vector(&a, &mut p);
                let y = vector(&a, &mut p);
                q = 0;
                for _ in 0..7 {
                    want.extend(take_vec(&b, &mut q));
                }
                // The oracle leaves z = x + y before the recorded sequence starts.
                let mut z = Vector::filled(-2, 2, 0.).unwrap();
                z.set_slice(-1, 1, &y.slice(-1, 1).unwrap()).unwrap();
                z.initialized(&x).unwrap();
                z.add_from(&x, &y).unwrap();
                got.extend(vals(&x.slice(-1, 1).unwrap()));
                z.set_slice(-1, 1, &y.slice(-1, 1).unwrap()).unwrap();
                got.extend(vals(&z));
                z.initialized(&x).unwrap();
                got.extend(vals(&z));
                z.add_from(&x, &y).unwrap();
                got.extend(vals(&z));
                z.subtract_from(&x, &y).unwrap();
                got.extend(vals(&z));
                z.scale_from(2., &x).unwrap();
                got.extend(vals(&z));
                z.resize(3).unwrap();
                got.extend(vals(&z));
                check(&mut t, line, &want, &got);
            }
            "error_cases" => {
                assert_eq!(b.len(), 0); // All exceptions are reported in the input fields.
                let k: f64 = a[1].parse::<i32>().unwrap() as f64;
                let v = Vector::filled(1, 2, k + 1.).unwrap();
                assert!(v.divided(0.).is_err());
                let v = Vector::filled(1, 2, 1.).unwrap();
                assert!(v.value(3).is_err());
                let m = Matrix::filled(1, 2, 1, 3, k + 1.).unwrap();
                assert_eq!(m.determinant(), 0.0);
                let mut singular = Matrix::filled(1, 2, 1, 2, 0.).unwrap();
                assert!(singular.invert().is_err());
                let x = Vector::filled(1, 2, k + 1.).unwrap();
                let y = Vector::filled(1, 3, 1.).unwrap();
                assert!(x.added(&y).is_err());
                assert_eq!(&a[3..8], &["1", "1", "0", "1", "1"]);
            }
            "jacobi_object" => {
                p = 1;
                let x = matrix(&a, &mut p);
                let j = mathlin::Jacobi::new(&x).unwrap();
                assert_eq!(j.is_done(), b[0] == "1");
                q = 1;
                want.extend(take_vec(&b, &mut q));
                got.extend(vals(&j.values().unwrap()));
                for k in 1..=3 {
                    want.push(f(b[q]));
                    q += 1;
                    want.extend(take_vec(&b, &mut q));
                    got.push(j.value(k).unwrap());
                    got.extend(vals(&j.vector(k).unwrap()));
                }
                want.extend(take_mat(&b, &mut q));
                got.extend(mats(&j.vectors().unwrap()));
                check(&mut t, line, &want, &got);
            }
            "lu_invert" => {
                p = 1;
                let x = matrix(&a, &mut p);
                let (st, z) = mathlin::recipes::lu_invert(x).unwrap();
                assert_eq!(st, b[0].parse::<i32>().unwrap());
                q = 1;
                want.extend(take_mat(&b, &mut q));
                got.extend(mats(&z));
                check(&mut t, line, &want, &got);
            }
            "svd_scratch" => {
                p = 1;
                let x = matrix(&a, &mut p);
                let (st, u, w, v, scratch) =
                    mathlin::recipes::svd_decompose_with_scratch(x, Vector::filled(1, 3, 0.).unwrap()).unwrap();
                assert_eq!(st, b[0].parse::<i32>().unwrap());
                q = 1;
                want.extend(take_mat(&b, &mut q));
                want.extend(take_vec(&b, &mut q));
                want.extend(take_mat(&b, &mut q));
                want.extend(take_vec(&b, &mut q));
                got.extend(mats(&u));
                got.extend(vals(&w));
                got.extend(mats(&v));
                got.extend(vals(&scratch));
                check(&mut t, line, &want, &got);
            }
            "dactcl" => {
                p = 1;
                let packed = vector(&a, &mut p);
                let lo = a[p].parse::<i32>().unwrap();
                let hi = a[p + 1].parse::<i32>().unwrap();
                p += 2;
                let iv = a[p..p + (hi - lo + 1) as usize].iter().map(|s| s.parse::<i32>().unwrap()).collect::<Vec<_>>();
                p += (hi - lo + 1) as usize;
                let ix = mathlin::IntegerVector::from_slice(lo, &iv).unwrap();
                let rhs = vector(&a, &mut p);
                q = 0;
                let st = b[q].parse::<i32>().unwrap();
                q += 1;
                let ss = b[q].parse::<i32>().unwrap();
                q += 1;
                let (rust_st, packed) = mathlin::recipes::dactcl_decompose(packed, ix.clone(), 1e-20).unwrap();
                assert_eq!(rust_st, st);
                let (rust_ss, sol) = mathlin::recipes::dactcl_solve(&packed, rhs, ix, 1e-20).unwrap();
                assert_eq!(rust_ss, ss);
                want.extend(take_vec(&b, &mut q));
                want.extend(take_vec(&b, &mut q));
                got.extend(vals(&packed));
                got.extend(vals(&sol));
                check(&mut t, line, &want, &got);
            }
            "dump" => {
                let data = if a[1] == "matrix" && a.len() > 2 {
                    let n = a[2].parse::<i32>().unwrap();
                    let val = f(a[3]);
                    let mut m = Matrix::filled(1, n, 1, n, 0.).unwrap();
                    m.set_diag(val).unwrap();
                    m
                } else {
                    Matrix::from_slice(1, 1, 2, 2, &[3., 0., 0., 4.]).unwrap()
                };
                let nonempty = match a[1] {
                    "vector" => Vector::filled(-1, 1, 1.).unwrap().dump(),
                    "integer_vector" => mathlin::IntegerVector::filled(1, 2, 2).unwrap().dump(),
                    "matrix" => data.dump(),
                    "gauss" => mathlin::Gauss::new(&data, 1e-20).unwrap().dump(),
                    "crout" => mathlin::Crout::new(&data, 1e-20).unwrap().dump(),
                    "gauss_least_square" => mathlin::GaussLeastSquare::new(&data, 1e-20).unwrap().dump(),
                    "svd" => mathlin::Svd::new(&data).unwrap().dump(),
                    "jacobi" => mathlin::Jacobi::new(&data).unwrap().dump(),
                    "householder" => {
                        let aa = Matrix::from_slice(1, 1, 2, 1, &[1., 2.]).unwrap();
                        let bb = Matrix::from_slice(1, 1, 2, 1, &[3., 4.]).unwrap();
                        mathlin::Householder::new_matrix(&aa, &bb, 1e-20).unwrap().dump()
                    }
                    "polynomial" => DirectPolynomialRoots::new_quadratic(1., -3., 2.).dump(),
                    other => panic!("unknown dump {other}"),
                };
                assert_eq!(nonempty.is_empty(), b[0] == "0", "{line}");
            }
            "poly_infinite" => {
                let obj = DirectPolynomialRoots::new_linear(f(a[1]), f(a[2]));
                assert_eq!(obj.is_done(), b[0] == "1", "{line}");
                assert_eq!(obj.infinite_roots(), b[1] == "1", "{line}");
                if b[2] == "throws" {
                    assert!(obj.number_of_solutions().is_err(), "{line}");
                } else {
                    assert_eq!(obj.number_of_solutions().unwrap(), b[2].parse::<i32>().unwrap(), "{line}");
                }
            }
            "status_values" => {
                let case = if a.len() > 6 { Some(a[1].parse::<i32>().unwrap()) } else { None };
                let actual = [
                    mathlin::MathStatus::Ok as i32,
                    mathlin::MathStatus::TooManyIterations as i32,
                    mathlin::MathStatus::FunctionError as i32,
                    mathlin::MathStatus::DirectionSearchError as i32,
                    mathlin::MathStatus::NotBracketed as i32,
                    mathlin::STATUS_USER_ABORTED,
                    mathlin::STATUS_OK,
                    mathlin::STATUS_SINGULAR_MATRIX,
                    mathlin::STATUS_ARGUMENT_ERROR,
                    mathlin::STATUS_NO_CONVERGENCE,
                ];
                let expected_tokens =
                    if case.is_some() { a[2..].iter().chain(&b[1..]) } else { a[1..].iter().chain(&b[..]) };
                let expected = expected_tokens.map(|s| s.parse::<i32>().unwrap()).collect::<Vec<_>>();
                assert_eq!(actual.as_slice(), expected.as_slice(), "{line}");
                if let Some(n) = case {
                    assert_eq!(b[0] == "1", mathlin::ComputeGaussPointsAndWeights::new(n).is_done(), "{line}");
                }
            }
            "vector_products" => {
                p = 1;
                let x = vector(&a, &mut p);
                let m = matrix(&a, &mut p);
                let y = vector(&a, &mut p);
                q = 0;
                for _ in 0..5 {
                    want.extend(take_vec(&b, &mut q));
                }
                got.extend(vals(&x.t_multiplied(-2.)));
                got.extend(vals(&x.multiplied_matrix(&m).unwrap()));
                let mut out = Vector::filled(1, 3, 0.).unwrap();
                out.multiply_matrix_from(&x, &m).unwrap();
                got.extend(vals(&out));
                out.multiply_from_matrix(&m, &y).unwrap();
                got.extend(vals(&out));
                out.t_multiply_from(&x, &m).unwrap();
                got.extend(vals(&out));
                check(&mut t, line, &want, &got);
            }
            "vector_gp" => {
                let xy = [f(a[1]), f(a[2])];
                let xyz = [f(a[3]), f(a[1]), f(a[2])];
                q = 0;
                want.extend(take_vec(&b, &mut q));
                want.extend(take_vec(&b, &mut q));
                want.extend(take_integer_vector(&b, &mut q));
                want.extend(take_integer_vector(&b, &mut q));
                let gz = occt_port::gp::Pnt::new(xyz[0], xyz[1], xyz[2]);
                got.extend(vals(&Vector::from_xy(xy[0], xy[1])));
                got.extend(vals(&Vector::from_xyz(gz.xyz())));
                let ix = mathlin::IntegerVector::from_xy(xy[0], xy[1]);
                let iz = mathlin::IntegerVector::from_xyz(gz.xyz());
                for j in ix.lower()..=ix.upper() {
                    got.push(ix.value(j).unwrap() as f64);
                }
                for j in iz.lower()..=iz.upper() {
                    got.push(iz.value(j).unwrap() as f64);
                }
                check(&mut t, line, &want, &got);
            }
            "matrix_mutations" => {
                p = 1;
                let x = matrix(&a, &mut p);
                let y = matrix(&a, &mut p);
                let part = matrix(&a, &mut p);
                q = 0;
                for _ in 0..5 {
                    want.extend(take_mat(&b, &mut q));
                }
                let mut z = x.clone();
                z.transpose().unwrap();
                got.extend(mats(&z));
                z = x.clone();
                z.set_slice(2, 3, 2, 3, &part).unwrap();
                got.extend(mats(&z));
                z.initialized(&x).unwrap();
                got.extend(mats(&z));
                z.multiply_matrix(&y).unwrap();
                got.extend(mats(&z));
                got.extend(mats(&x.multiplied(-2.)));
                check(&mut t, line, &want, &got);
            }
            "lu_scratch" => {
                p = 1;
                let x = matrix(&a, &mut p);
                let ix = mathlin::IntegerVector::filled(1, 3, 0).unwrap();
                let vv = Vector::filled(1, 3, 0.).unwrap();
                let (st, l, ix, sign, vv) = mathlin::recipes::lu_decompose_with_scratch(x, ix, 0., vv, 1e-20).unwrap();
                assert_eq!(st, b[0].parse::<i32>().unwrap());
                q = 1;
                want.extend(take_mat(&b, &mut q));
                want.extend(take_vec_int(&b, &mut q));
                assert_eq!(format!("{:016x}", sign.to_bits()), b[q]);
                want.push(f(b[q]));
                q += 1;
                want.extend(take_vec(&b, &mut q));
                got.extend(mats(&l));
                for k in ix.lower()..=ix.upper() {
                    got.push(ix.value(k).unwrap() as f64);
                }
                got.push(sign);
                got.extend(vals(&vv));
                check(&mut t, line, &want, &got);
            }
            other => panic!("unhandled oracle record {other}: {line}"),
        }
        t.lines += 1;
    }
    assert_eq!(t.lines, text.lines().count(), "all golden records must be checked");
    println!("{} records, {} values, {} bit-identical, worst {} ulps", t.lines, t.values, t.exact, t.worst);
}
