//! `Geom_ExtrusionUtils` and `Geom_RevolutionUtils` (the templates are instantiated with an `Adaptor3d_Curve` basis here),
//! and the `Geom_OffsetCurveUtils` evaluations over a `GeomAdaptor_Curve` basis.
use super::Result;
use crate::adaptor3d::Curve3dAdaptor;
use crate::geom::{ResD1, ResD2, ResD3, SurfResD1, SurfResD2, SurfResD3};
use crate::gp::{self, Ax1, Dir, Pnt, Trsf, Vec, XYZ};

fn rotation(axis: &Ax1, u: f64) -> Result<Trsf> {
    let mut t = Trsf::new();
    t.set_rotation(axis, u)?;
    Ok(t)
}

// ---- Geom_ExtrusionUtils ------------------------------------------------------------------------------------------------------

fn extrusion_point(curve_pt: &Pnt, v: f64, dir: &XYZ) -> Pnt {
    Pnt::from_xyz(curve_pt.xyz().added(&dir.multiplied(v)))
}
fn zero() -> Vec {
    Vec::new(0.0, 0.0, 0.0)
}

/// `Geom_ExtrusionUtils::D0`.
pub(super) fn extrusion_d0(u: f64, v: f64, basis: &dyn Curve3dAdaptor, dir: &XYZ) -> Result<Pnt> {
    let p = basis.eval_d0(u)?;
    Ok(extrusion_point(&p, v, dir))
}
/// `Geom_ExtrusionUtils::D1`.
pub(super) fn extrusion_d1(u: f64, v: f64, basis: &dyn Curve3dAdaptor, dir: &XYZ) -> Result<SurfResD1> {
    let c = basis.eval_d1(u)?;
    Ok(SurfResD1 { point: extrusion_point(&c.point, v, dir), d1u: c.d1, d1v: Vec::from_xyz(*dir) })
}
/// `Geom_ExtrusionUtils::D2`.
pub(super) fn extrusion_d2(u: f64, v: f64, basis: &dyn Curve3dAdaptor, dir: &XYZ) -> Result<SurfResD2> {
    let c = basis.eval_d2(u)?;
    Ok(SurfResD2 {
        point: extrusion_point(&c.point, v, dir),
        d1u: c.d1,
        d1v: Vec::from_xyz(*dir),
        d2u: c.d2,
        d2v: zero(),
        d2uv: zero(),
    })
}
/// `Geom_ExtrusionUtils::D3`.
pub(super) fn extrusion_d3(u: f64, v: f64, basis: &dyn Curve3dAdaptor, dir: &XYZ) -> Result<SurfResD3> {
    let c = basis.eval_d3(u)?;
    Ok(SurfResD3 {
        point: extrusion_point(&c.point, v, dir),
        d1u: c.d1,
        d1v: Vec::from_xyz(*dir),
        d2u: c.d2,
        d2v: zero(),
        d2uv: zero(),
        d3u: c.d3,
        d3v: zero(),
        d3uuv: zero(),
        d3uvv: zero(),
    })
}
/// `Geom_ExtrusionUtils::DN`.
pub(super) fn extrusion_dn(u: f64, basis: &dyn Curve3dAdaptor, dir: &XYZ, der_u: i32, der_v: i32) -> Result<Vec> {
    let mut curve_dn = Vec::new(0.0, 0.0, 0.0);
    if der_v == 0 {
        curve_dn = basis.eval_dn(u, der_u)?;
    }
    Ok(if der_v == 0 {
        curve_dn
    } else if der_u == 0 && der_v == 1 {
        Vec::from_xyz(*dir)
    } else {
        zero()
    })
}

// ---- Geom_RevolutionUtils -----------------------------------------------------------------------------------------------------

fn square_confusion() -> f64 {
    let c = crate::geom::confusion();
    c * c
}

/// `Geom_RevolutionUtils::D0`.
pub(super) fn revolution_d0(u: f64, v: f64, basis: &dyn Curve3dAdaptor, axis: &Ax1) -> Result<Pnt> {
    let curve_pt = basis.eval_d0(v)?;
    let rot = rotation(axis, u)?;
    Ok(curve_pt.transformed(&rot))
}
/// `Geom_RevolutionUtils::D1`.
pub(super) fn revolution_d1(u: f64, v: f64, basis: &dyn Curve3dAdaptor, axis: &Ax1) -> Result<SurfResD1> {
    let c = basis.eval_d1(v)?;
    let cq = c.point.xyz().subtracted(axis.location().xyz());
    let mut d1u = Vec::from_xyz(axis.direction().xyz().crossed(&cq));
    if d1u.square_magnitude() < square_confusion() {
        d1u = zero();
    }
    let rot = rotation(axis, u)?;
    Ok(SurfResD1 { point: c.point.transformed(&rot), d1u: d1u.transformed(&rot), d1v: c.d1.transformed(&rot) })
}
/// `Geom_RevolutionUtils::D2`.
pub(super) fn revolution_d2(u: f64, v: f64, basis: &dyn Curve3dAdaptor, axis: &Ax1) -> Result<SurfResD2> {
    let c = basis.eval_d2(v)?;
    let cq = c.point.xyz().subtracted(axis.location().xyz());
    let dir = axis.direction().xyz();
    let mut d1u = Vec::from_xyz(dir.crossed(&cq));
    if d1u.square_magnitude() < square_confusion() {
        d1u = zero();
    }
    let d2u = Vec::from_xyz(dir.multiplied(dir.dot(&cq)).subtracted(&cq));
    let d2uv = Vec::from_xyz(dir.crossed(c.d1.xyz()));
    let rot = rotation(axis, u)?;
    Ok(SurfResD2 {
        point: c.point.transformed(&rot),
        d1u: d1u.transformed(&rot),
        d1v: c.d1.transformed(&rot),
        d2u: d2u.transformed(&rot),
        d2v: c.d2.transformed(&rot),
        d2uv: d2uv.transformed(&rot),
    })
}
/// `Geom_RevolutionUtils::D3`.
pub(super) fn revolution_d3(u: f64, v: f64, basis: &dyn Curve3dAdaptor, axis: &Ax1) -> Result<SurfResD3> {
    let c = basis.eval_d3(v)?;
    let cq = c.point.xyz().subtracted(axis.location().xyz());
    let dir = axis.direction().xyz();
    let mut d1u = Vec::from_xyz(dir.crossed(&cq));
    if d1u.square_magnitude() < square_confusion() {
        d1u = zero();
    }
    let d2u = Vec::from_xyz(dir.multiplied(dir.dot(&cq)).subtracted(&cq));
    let d2uv = Vec::from_xyz(dir.crossed(c.d1.xyz()));
    let d3u = d1u.reversed();
    let d3uuv = Vec::from_xyz(dir.multiplied(dir.dot(c.d1.xyz())).subtracted(c.d1.xyz()));
    let d3uvv = Vec::from_xyz(dir.crossed(c.d2.xyz()));
    let rot = rotation(axis, u)?;
    Ok(SurfResD3 {
        point: c.point.transformed(&rot),
        d1u: d1u.transformed(&rot),
        d1v: c.d1.transformed(&rot),
        d2u: d2u.transformed(&rot),
        d2v: c.d2.transformed(&rot),
        d2uv: d2uv.transformed(&rot),
        d3u: d3u.transformed(&rot),
        d3v: c.d3.transformed(&rot),
        d3uuv: d3uuv.transformed(&rot),
        d3uvv: d3uvv.transformed(&rot),
    })
}
/// `Geom_RevolutionUtils::DN`.
pub(super) fn revolution_dn(
    u: f64,
    v: f64,
    basis: &dyn Curve3dAdaptor,
    axis: &Ax1,
    der_u: i32,
    der_v: i32,
) -> Result<Vec> {
    let curve_pt_or_dn = if der_u == 0 {
        basis.eval_dn(v, der_v)?
    } else if der_v == 0 {
        let p = basis.eval_d0(v)?;
        Vec::from_xyz(p.xyz().subtracted(axis.location().xyz()))
    } else {
        basis.eval_dn(v, der_v)?
    };
    let rot = rotation(axis, u)?;
    let result = if der_u == 0 {
        curve_pt_or_dn
    } else {
        let dir = axis.direction().xyz();
        let x = curve_pt_or_dn.xyz();
        if der_u % 4 == 1 {
            Vec::from_xyz(dir.crossed(x))
        } else if der_u % 4 == 2 {
            Vec::from_xyz(dir.multiplied(dir.dot(x)).subtracted(x))
        } else if der_u % 4 == 3 {
            Vec::from_xyz(dir.crossed(x)).multiplied(-1.0)
        } else {
            Vec::from_xyz(x.subtracted(&dir.multiplied(dir.dot(x))))
        }
    };
    Ok(result.transformed(&rot))
}

// ---- Geom_OffsetCurveUtils over a basis adaptor ------------------------------------------------------------------------------

/// `Geom_OffsetCurveUtils::AdjustDerivative`: replaces the derivatives where the first one vanishes; `d` holds
/// `D1 .. D4`.
fn adjust_derivative(
    curve: &dyn Curve3dAdaptor,
    max_derivative: i32,
    u: f64,
    d: &mut [Vec; 4],
    is_direction_change: &mut bool,
) -> Result<()> {
    let tol = gp::resolution();
    let min_step = 1e-7;
    let max_deriv_order = 3;
    *is_direction_change = false;
    let uinfium = curve.first_parameter()?;
    let usupremum = curve.last_parameter()?;
    let division_factor = 1.0e-3;
    let du = if usupremum >= f64::MAX || uinfium <= -f64::MAX { 0.0 } else { usupremum - uinfium };
    let delta = cmax(du * division_factor, min_step);
    // the derivative is approximated by a Taylor series
    let mut index = 1;
    let mut v;
    loop {
        index += 1;
        v = curve.eval_dn(u, index)?;
        if !(v.square_magnitude() <= tol && index < max_deriv_order) {
            break;
        }
    }
    let uu = if u - uinfium < delta { u + delta } else { u - delta };
    let p1 = curve.eval_d0(cmin(u, uu))?;
    let p2 = curve.eval_d0(cmax(u, uu))?;
    let v1 = Vec::between(&p1, &p2);
    *is_direction_change = v.dot(&v1) < 0.0;
    let sign = if *is_direction_change { -1.0 } else { 1.0 };
    if let Some(t) = d.get_mut(0) {
        *t = v.multiplied(sign);
    }
    for i in 1..max_derivative {
        let dn = curve.eval_dn(u, index + i)?;
        if let Some(t) = d.get_mut(i as usize) {
            *t = dn.multiplied(sign);
        }
    }
    Ok(())
}
fn cmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}
fn cmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `Geom_OffsetCurveUtils::EvaluateD0`; `None` is the C++ `false`.
pub(super) fn offset_d0(u: f64, basis: &dyn Curve3dAdaptor, dir: &Dir, offset: f64) -> Result<Option<Pnt>> {
    let r = basis.eval_d1(u)?;
    let mut p = r.point;
    Ok(crate::geom::offset_utils::calculate_d0(&mut p, &r.d1, dir.xyz(), offset).then_some(p))
}
/// `Geom_OffsetCurveUtils::EvaluateD1`.
pub(super) fn offset_d1(u: f64, basis: &dyn Curve3dAdaptor, dir: &Dir, offset: f64) -> Result<Option<ResD1>> {
    let r = basis.eval_d2(u)?;
    let (mut p, mut d1) = (r.point, r.d1);
    Ok(crate::geom::offset_utils::calculate_d1(&mut p, &mut d1, &r.d2, dir.xyz(), offset)
        .then_some(ResD1 { point: p, d1 }))
}
/// `Geom_OffsetCurveUtils::EvaluateD2`.
pub(super) fn offset_d2(u: f64, basis: &dyn Curve3dAdaptor, dir: &Dir, offset: f64) -> Result<Option<ResD2>> {
    let r = basis.eval_d3(u)?;
    let (mut p, mut d1, mut d2, mut d3) = (r.point, r.d1, r.d2, r.d3);
    let mut dir_change = false;
    if d1.square_magnitude() <= gp::resolution() {
        let mut ders = [d1, d2, d3, Vec::new(0.0, 0.0, 0.0)];
        adjust_derivative(basis, 3, u, &mut ders, &mut dir_change)?;
        [d1, d2, d3, _] = ders;
    }
    Ok(crate::geom::offset_utils::calculate_d2(&mut p, &mut d1, &mut d2, &d3, dir.xyz(), offset, dir_change)
        .then_some(ResD2 { point: p, d1, d2 }))
}
/// `Geom_OffsetCurveUtils::EvaluateD3`.
pub(super) fn offset_d3(u: f64, basis: &dyn Curve3dAdaptor, dir: &Dir, offset: f64) -> Result<Option<ResD3>> {
    let r = basis.eval_d3(u)?;
    let (mut p, mut d1, mut d2, mut d3) = (r.point, r.d1, r.d2, r.d3);
    let mut d4 = basis.eval_dn(u, 4)?;
    let mut dir_change = false;
    if d1.square_magnitude() <= gp::resolution() {
        let mut ders = [d1, d2, d3, d4];
        adjust_derivative(basis, 4, u, &mut ders, &mut dir_change)?;
        [d1, d2, d3, d4] = ders;
    }
    Ok(crate::geom::offset_utils::calculate_d3(&mut p, &mut d1, &mut d2, &mut d3, &d4, dir.xyz(), offset, dir_change)
        .then_some(ResD3 { point: p, d1, d2, d3 }))
}
/// `Geom_OffsetCurveUtils::EvaluateDN` with `N == 3`: `EvaluateD3(U, basis, dir, offset, aPnt, aDummy, aDummy, theDN)`; the first
/// and the second derivative are the same C++ variable, so both inputs of `CalculateD3` hold the last value written to it (the
/// second derivative) and the result is the third derivative computed from those.
fn offset_dn3(u: f64, basis: &dyn Curve3dAdaptor, dir: &Dir, offset: f64) -> Result<Option<Vec>> {
    let r = basis.eval_d3(u)?;
    let mut p = r.point;
    let mut dd = r.d2; // aDummy = D1, then aDummy = D2
    let mut d3 = r.d3;
    let mut d4 = basis.eval_dn(u, 4)?;
    let mut dir_change = false;
    if dd.square_magnitude() <= gp::resolution() {
        let mut ders = [dd, dd, d3, d4];
        adjust_derivative(basis, 4, u, &mut ders, &mut dir_change)?;
        // the writes to the first and to the second derivative go to the same variable: the second one stays
        [_, dd, d3, d4] = ders;
    }
    let (mut a, mut b) = (dd, dd);
    Ok(crate::geom::offset_utils::calculate_d3(&mut p, &mut a, &mut b, &mut d3, &d4, dir.xyz(), offset, dir_change)
        .then_some(d3))
}
/// `Geom_OffsetCurveUtils::EvaluateDN`; `None` is the C++ `false`.
pub(super) fn offset_dn(u: f64, basis: &dyn Curve3dAdaptor, dir: &Dir, offset: f64, n: i32) -> Result<Option<Vec>> {
    Ok(match n {
        1 => offset_d1(u, basis, dir, offset)?.map(|r| r.d1),
        2 => offset_d2(u, basis, dir, offset)?.map(|r| r.d2),
        3 => offset_dn3(u, basis, dir, offset)?,
        _ => Some(basis.eval_dn(u, n)?),
    })
}
