//! The steps that `GProp_CelGProps`, `GProp_SelGProps` and `GProp_VelGProps` repeat: the 3x3 `math_Matrix` of the
//! local moments, the `math_Jacobi` eigen decomposition, and the change of frame of the eigen vectors.

use super::{Error, GProps, h_operator};
use crate::gp::{Dir, Mat, XYZ};
use crate::mathlin::{Jacobi, Matrix};

/// A 3x3 `math_Matrix(1, 3, 1, 3)` as rows (`m[row][col]`, 0-based).
pub(crate) type Rows3 = [[f64; 3]; 3];

/// Maps a `Standard_Failure` of the mathlin helpers to the GProp error.
fn fail<E>(_: E) -> Error {
    Error
}

/// The `Passage` matrix of the C++ code: `(1,1) = Xa1, (1,2) = Xa2, (1,3) = Xa3`, row 2 from the Y components and
/// row 3 from the Z components of the axes `x`, `y`, `z` (the X direction, Y direction and main direction).
pub(crate) fn passage(x: &Dir, y: &Dir, z: &Dir) -> Rows3 {
    let (xa, ya, za) = (x.xyz(), y.xyz(), z.xyz());
    [[xa.x(), ya.x(), za.x()], [xa.y(), ya.y(), za.y()], [xa.z(), ya.z(), za.z()]]
}

/// `math_Jacobi J(Dm)`: the eigen values, and the eigen vectors `vecs[k]` (eigen vector `k + 1`).
pub(crate) fn eigen(dm: &Rows3) -> Result<([f64; 3], [[f64; 3]; 3]), Error> {
    let mut m = Matrix::new(1, 3, 1, 3).map_err(fail)?;
    for (i, row) in dm.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            m.set_value(i as i32 + 1, j as i32 + 1, *v).map_err(fail)?;
        }
    }
    let jac = Jacobi::new(&m).map_err(fail)?;
    let mut values = [0.0; 3];
    let mut vectors = [[0.0; 3]; 3];
    for (k, (value, vector)) in values.iter_mut().zip(vectors.iter_mut()).enumerate() {
        let num = k as i32 + 1;
        *value = jac.value(num).map_err(fail)?;
        let v = jac.vector(num).map_err(fail)?;
        for (r, slot) in vector.iter_mut().enumerate() {
            *slot = v.value(r as i32 + 1).map_err(fail)?;
        }
    }
    Ok((values, vectors))
}

/// `V_k.Multiply(Passage, V_k); V_k.Multiply(scale_k)` for k = 1..3, then
/// `gp_Mat(gp_XYZ(V1(1), V2(1), V3(1)), gp_XYZ(V1(2), V2(2), V3(2)), gp_XYZ(V1(3), V2(3), V3(3)))`.
///
/// `math_Vector::Multiply(M, V)` with `V` as both operands works in place: component `i` is overwritten before the
/// components `j > i` are summed, so they see the new value of component `i`. This is reproduced here on purpose.
pub(crate) fn inertia_from_eigen(passage: &Rows3, vecs: &[[f64; 3]; 3], scales: [f64; 3]) -> Mat {
    let mut v = [[0.0; 3]; 3];
    for ((out, vec), scale) in v.iter_mut().zip(vecs.iter()).zip(scales) {
        let mut w = *vec;
        for (r, p_row) in passage.iter().enumerate() {
            let mut acc = 0.0;
            for (c, p) in p_row.iter().enumerate() {
                acc += p * w.get(c).copied().unwrap_or(0.0);
            }
            if let Some(slot) = w.get_mut(r) {
                *slot = acc;
            }
        }
        for x in w.iter_mut() {
            *x *= scale;
        }
        *out = w;
    }
    let [v1, v2, v3] = v;
    let [a1, b1, c1] = v1;
    let [a2, b2, c2] = v2;
    let [a3, b3, c3] = v3;
    Mat::from_columns(XYZ::new(a1, a2, a3), XYZ::new(b1, b2, b3), XYZ::new(c1, c2, c3))
}

/// A symmetric 3x3 `Dm` of the C++ code from its six distinct entries.
pub(crate) fn sym(d11: f64, d22: f64, d33: f64, d12: f64, d13: f64, d23: f64) -> Rows3 {
    [[d11, d12, d13], [d12, d22, d23], [d13, d23, d33]]
}

/// The last steps of the `Perform` functions: `math_Jacobi J(Dm)`, the eigen vectors multiplied by `passage` and by
/// `factor * J.Value(k)` (`factor` is 1 when the C++ code multiplies by the eigen value alone), then
/// `inertia = inertia + HOperator(g, loc, dim)`.
pub(crate) fn finish_inertia(base: &mut GProps, passage: &Rows3, dm: &Rows3, factor: f64) -> Result<(), Error> {
    let (vals, vecs) = eigen(dm)?;
    base.inertia = inertia_from_eigen(passage, &vecs, vals.map(|v| factor * v));
    let hop = h_operator(&base.g, &base.loc, base.dim);
    base.inertia = base.inertia.added(&hop);
    Ok(())
}
