//! OpenSCAD CSG trees (from `gusset-scad`) built as exact B-rep with the kernel
//! (ADR 0033): cubes and cylinders are extrusions, cones and spheres revolutions, 2D
//! shapes are profiles, placements are rigid kernel transforms after scaling the
//! primitive itself (scales must keep the primitive's shape: no shear, and spheres,
//! circles and revolutions only scale uniformly).

use gusset_kernel::{
    BoolOp, ExtrudeSpec, Frame, Kernel, Loop2, Profile, ProfileEdge, Region2, RevolveSpec, Seg2, Shape, Transform,
};
use gusset_scad::{Csg, M4, Op, Prim, Shape2};
use std::f64::consts::TAU;

type R<T> = Result<T, String>;

fn at(line: u32, m: impl std::fmt::Display) -> String {
    if line > 0 { format!("line {line}: {m}") } else { m.to_string() }
}

fn line_loop(pts: &[[f64; 2]]) -> Loop2 {
    let edges = pts
        .iter()
        .zip(pts.iter().cycle().skip(1))
        .zip(0u64..)
        .map(|((a, b), tag)| ProfileEdge { seg: Seg2::Line { a: *a, b: *b }, tag })
        .collect();
    Loop2 { edges }
}

fn area(pts: &[[f64; 2]]) -> f64 {
    pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum::<f64>() / 2.0
}

/// Drops repeated points (also a closing repeat of the first).
fn clean(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::new();
    for p in pts {
        if out.last().is_none_or(|q| (q[0] - p[0]).abs() > 1e-9 || (q[1] - p[1]).abs() > 1e-9) {
            out.push(*p);
        }
    }
    if out.len() > 1
        && let (Some(f), Some(l)) = (out.first(), out.last())
        && (f[0] - l[0]).abs() < 1e-9
        && (f[1] - l[1]).abs() < 1e-9
    {
        out.pop();
    }
    out
}

/// A 2D shape as profile regions, scaled by `s`.
fn regions(shape: &Shape2, s: [f64; 2], line: u32) -> R<Vec<Region2>> {
    match shape {
        Shape2::Circle { center, radius } => {
            if (s[0] - s[1]).abs() > 1e-9 * s[0].max(s[1]) {
                return Err(at(line, "a circle scaled unevenly (an ellipse) is not supported"));
            }
            let c = [center[0] * s[0], center[1] * s[1]];
            let seg = Seg2::Circle { center: c, radius: radius * s[0] };
            Ok(vec![Region2 { outer: Loop2 { edges: vec![ProfileEdge { seg, tag: 0 }] }, holes: vec![] }])
        }
        Shape2::Polygon(paths) => {
            let mut loops: Vec<Vec<[f64; 2]>> = paths
                .iter()
                .map(|p| clean(&p.iter().map(|q| [q[0] * s[0], q[1] * s[1]]).collect::<Vec<_>>()))
                .filter(|p| p.len() >= 3 && area(p).abs() > 1e-12)
                .collect();
            if loops.is_empty() {
                return Err(at(line, "a polygon has no area"));
            }
            // The outside counterclockwise, holes clockwise.
            for (i, l) in loops.iter_mut().enumerate() {
                if (i == 0) == (area(l) < 0.0) {
                    l.reverse();
                }
            }
            let mut it = loops.iter();
            let outer = it.next().map(|l| line_loop(l)).unwrap_or_default();
            Ok(vec![Region2 { outer, holes: it.map(|l| line_loop(l)).collect() }])
        }
    }
}

/// The scale (column lengths), rotation and translation of a placement.
/// Scale, rotation and translation of a placement.
type Parts = ([f64; 3], [[f64; 3]; 3], [f64; 3]);

fn decompose(m: &M4, line: u32) -> R<Parts> {
    let g = |i: usize, j: usize| m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
    let col = |j: usize| [g(0, j), g(1, j), g(2, j)];
    let cols = [col(0), col(1), col(2)];
    let len = |v: [f64; 3]| libm::sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
    let s = [len(cols[0]), len(cols[1]), len(cols[2])];
    if s.iter().any(|v| !(*v > 1e-12 && v.is_finite())) {
        return Err(at(line, "a scale of zero flattens the object"));
    }
    let d = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    for (a, b) in [(cols[0], cols[1]), (cols[0], cols[2]), (cols[1], cols[2])] {
        if d(a, b).abs() > 1e-9 * len(a) * len(b) {
            return Err(at(line, "a skewed transform (shear) is not supported"));
        }
    }
    let r = [
        [cols[0][0] / s[0], cols[1][0] / s[1], cols[2][0] / s[2]],
        [cols[0][1] / s[0], cols[1][1] / s[1], cols[2][1] / s[2]],
        [cols[0][2] / s[0], cols[1][2] / s[1], cols[2][2] / s[2]],
    ];
    Ok((s, r, [g(0, 3), g(1, 3), g(2, 3)]))
}

/// Axis and angle (radians) of a proper rotation matrix.
fn axis_angle(r: &[[f64; 3]; 3]) -> Option<([f64; 3], f64)> {
    let tr = r[0][0] + r[1][1] + r[2][2];
    let angle = libm::acos(((tr - 1.0) / 2.0).clamp(-1.0, 1.0));
    if angle.abs() < 1e-12 {
        return None;
    }
    let v = [r[2][1] - r[1][2], r[0][2] - r[2][0], r[1][0] - r[0][1]];
    let n = libm::sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
    if n > 1e-9 {
        return Some(([v[0] / n, v[1] / n, v[2] / n], angle));
    }
    // A half turn: the axis from the symmetric part.
    let d = [((r[0][0] + 1.0) / 2.0).max(0.0), ((r[1][1] + 1.0) / 2.0).max(0.0), ((r[2][2] + 1.0) / 2.0).max(0.0)];
    let mut a = [libm::sqrt(d[0]), libm::sqrt(d[1]), libm::sqrt(d[2])];
    if a[0] > 1e-6 {
        a[1] = a[1].copysign(r[0][1]);
        a[2] = a[2].copysign(r[0][2]);
    } else if a[1] > 1e-6 {
        a[2] = a[2].copysign(r[1][2]);
    }
    Some((a, angle))
}

fn extrude(k: &dyn Kernel, regions: Vec<Region2>, frame: Frame, h: f64, z0: f64) -> R<Shape> {
    let frame = Frame { origin: [frame.origin[0], frame.origin[1], frame.origin[2] + z0], ..frame };
    let p = Profile { frame, regions };
    Ok(k.extrude(&p, &ExtrudeSpec { distance: h, symmetric: false }).map_err(|e| e.message)?.shape)
}

fn revolve(k: &dyn Kernel, regions: Vec<Region2>, angle: f64) -> R<Shape> {
    // Profile (x = radius, y = height) in the XZ plane, turned about Z.
    let p = Profile { frame: Frame::XZ, regions };
    let spec = RevolveSpec { axis_origin: [0.0, 0.0], axis_dir: [0.0, 1.0], angle };
    Ok(k.revolve(&p, &spec).map_err(|e| e.message)?.shape)
}

fn primitive(k: &dyn Kernel, prim: &Prim, s: [f64; 3], line: u32) -> R<Shape> {
    let uniform = |what: &str| -> R<f64> {
        if (s[0] - s[1]).abs() > 1e-9 * s[0] || (s[0] - s[2]).abs() > 1e-9 * s[0] {
            Err(at(line, format!("{what} can only be scaled evenly")))
        } else {
            Ok(s[0])
        }
    };
    let r = match prim {
        Prim::Cube { size, center } => {
            let sz = [size[0] * s[0], size[1] * s[1], size[2] * s[2]];
            let o = if *center { [-sz[0] / 2.0, -sz[1] / 2.0, -sz[2] / 2.0] } else { [0.0; 3] };
            let pts = [[o[0], o[1]], [o[0] + sz[0], o[1]], [o[0] + sz[0], o[1] + sz[1]], [o[0], o[1] + sz[1]]];
            extrude(k, vec![Region2 { outer: line_loop(&pts), holes: vec![] }], Frame::XY, sz[2], o[2])
        }
        Prim::Cylinder { h, r1, r2, center } => {
            if (s[0] - s[1]).abs() > 1e-9 * s[0] {
                return Err(at(line, "a cylinder scaled unevenly across its axis is not supported"));
            }
            let (h, r1, r2) = (h * s[2], r1 * s[0], r2 * s[0]);
            let z0 = if *center { -h / 2.0 } else { 0.0 };
            if (r1 - r2).abs() < 1e-12 {
                let seg = Seg2::Circle { center: [0.0, 0.0], radius: r1 };
                let reg = Region2 { outer: Loop2 { edges: vec![ProfileEdge { seg, tag: 0 }] }, holes: vec![] };
                extrude(k, vec![reg], Frame::XY, h, z0)
            } else {
                let pts = vec![[0.0, z0], [r1, z0], [r2, z0 + h], [0.0, z0 + h]];
                revolve(k, vec![Region2 { outer: line_loop(&clean(&pts)), holes: vec![] }], TAU)
            }
        }
        Prim::Sphere { radius } => {
            let r = radius * uniform("a sphere")?;
            let edges = vec![
                ProfileEdge { seg: Seg2::Line { a: [0.0, -r], b: [0.0, r] }, tag: 0 },
                ProfileEdge {
                    seg: Seg2::Arc { center: [0.0, 0.0], start: [0.0, r], end: [0.0, -r], ccw: false },
                    tag: 1,
                },
            ];
            revolve(k, vec![Region2 { outer: Loop2 { edges }, holes: vec![] }], TAU)
        }
        Prim::Prism { shape, h, center } => {
            let h = h * s[2];
            let z0 = if *center { -h / 2.0 } else { 0.0 };
            extrude(k, regions(shape, [s[0], s[1]], line)?, Frame::XY, h, z0)
        }
        Prim::Revolve { shape, angle } => {
            let f = uniform("a rotate_extrude")?;
            let regs = regions(shape, [f, f], line)?;
            let crosses = match shape {
                Shape2::Circle { center, radius } => center[0] - radius < -1e-9,
                Shape2::Polygon(p) => p.iter().flatten().any(|q| q[0] < -1e-9),
            };
            if crosses {
                return Err(at(line, "rotate_extrude: the shape must lie on one side of the axis (x >= 0)"));
            }
            revolve(k, regs, angle.to_radians())
        }
    };
    r.map_err(|e| at(line, e))
}

fn leaf(k: &dyn Kernel, prim: &Prim, m: &M4, line: u32) -> R<Shape> {
    let (s, mut r, t) = decompose(m, line)?;
    let mut shape = primitive(k, prim, s, line)?;
    let det = r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1]) - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
        + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
    let tr = |shape: &Shape, t: &Transform| k.transform(shape, t).map(|o| o.shape).map_err(|e| at(line, e.message));
    if det < 0.0 {
        // A mirror: reflect across XY first, then the rest is a proper rotation.
        shape = tr(&shape, &Transform::Mirror { origin: [0.0; 3], normal: [0.0, 0.0, 1.0] })?;
        for row in &mut r {
            row[2] = -row[2];
        }
    }
    if let Some((axis, angle)) = axis_angle(&r) {
        shape = tr(&shape, &Transform::Rotate { origin: [0.0; 3], axis, angle })?;
    }
    if t.iter().any(|v| v.abs() > 0.0) {
        shape = tr(&shape, &Transform::Translate(t))?;
    }
    Ok(shape)
}

/// Builds one top-level object.
///
/// # Errors
/// An unsupported placement or a kernel failure, with the line of the primitive.
pub fn build(k: &dyn Kernel, c: &Csg) -> R<Shape> {
    match c {
        Csg::Leaf { prim, at, line } => leaf(k, prim, at, *line),
        Csg::Op(op, kids) => {
            let shapes: Vec<Shape> = kids.iter().map(|c| build(k, c)).collect::<R<_>>()?;
            let Some((first, rest)) = shapes.split_first() else { return Err("an empty operation".into()) };
            if rest.is_empty() {
                return Ok(first.clone());
            }
            match op {
                Op::Union => k.boolean_many(BoolOp::Union, first, rest).map(|o| o.shape).map_err(|e| e.message),
                Op::Difference => k.boolean_many(BoolOp::Cut, first, rest).map(|o| o.shape).map_err(|e| e.message),
                Op::Intersection => rest.iter().try_fold(first.clone(), |acc, s| {
                    k.boolean(BoolOp::Intersect, &acc, s).map(|o| o.shape).map_err(|e| e.message)
                }),
            }
        }
    }
}
