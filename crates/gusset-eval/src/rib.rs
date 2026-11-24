//! Ribs (ADR 0062): sketch lines thickened and extended up to the body, joined to it.
//!
//! Each rib is built from rectangles (in the plane) or one mitred outline (along the normal)
//! whose free ends reach past the body's box, swept up to the next face of the body
//! ([`Kernel::rib_to_next`]); the pieces from the extended ends outside the part are dropped by
//! the kernel, the rest is joined to the body.

use super::*;
use gusset_doc::{RibDef, RibGrowth, RibSide, SheetPart};
use gusset_kernel::{Frame, Loop2, P2, Profile, ProfileEdge, Region2, Seg2, V3, add, norm, scale, sub};
use gusset_naming::sheet_tag;

/// Most lines one rib may have.
const MAX_RIB_LINES: usize = 64;

/// One straight line of the rib's chain, in sketch coordinates, with its sketch entity.
struct Line {
    a: P2,
    b: P2,
    entity: String,
}

fn edge(seg: Seg2, part: SheetPart, line: usize) -> ProfileEdge {
    ProfileEdge { seg, tag: sheet_tag(part, line) }
}

fn rect(x0: f64, x1: f64, y0: f64, y1: f64, line: usize) -> Region2 {
    let l = |a: P2, b: P2| Seg2::Line { a, b };
    let edges = vec![
        edge(l([x0, y0], [x1, y0]), SheetPart::FlatBack, line),
        edge(l([x1, y0], [x1, y1]), SheetPart::End, line),
        edge(l([x1, y1], [x0, y1]), SheetPart::Flat, line),
        edge(l([x0, y1], [x0, y0]), SheetPart::Start, line),
    ];
    Region2 { outer: Loop2 { edges }, holes: Vec::new() }
}

/// The point where the lines through `p` along `u` and through `q` along `v` cross.
fn cross_point(p: P2, u: P2, q: P2, v: P2) -> Option<P2> {
    let den = u[0] * v[1] - u[1] * v[0];
    if den.abs() < 1e-9 {
        return None;
    }
    let w = [q[0] - p[0], q[1] - p[1]];
    let t = (w[0] * v[1] - w[1] * v[0]) / den;
    Some([p[0] + u[0] * t, p[1] + u[1] * t])
}

impl Evaluator {
    pub(crate) fn run_rib(
        &self,
        s: &mut State,
        name: &str,
        d: &RibDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let t = eval_expr(name, &d.thickness, Dim::LENGTH, params, units)?;
        if !(t.is_finite() && t > 1e-9) {
            return Err(EvalError::new(name, ErrorCode::Expression, "the rib's thickness must be positive")
                .with_hint("enter a thickness such as 3 mm"));
        }
        let sv = s.solved.get(&d.sketch).cloned().ok_or_else(|| {
            EvalError::new(name, ErrorCode::InvalidReference, format!("'{}' is not an evaluated sketch", d.sketch))
        })?;
        let chain = sketch::path_chain(name, &d.sketch, &sv, &d.curves)?;
        let mut lines = Vec::new();
        for e in &chain.edges {
            let Seg2::Line { a, b } = e.seg else {
                return Err(EvalError::new(name, ErrorCode::InvalidReference, "a rib is drawn with straight lines")
                    .with_hint("draw the rib's lines without arcs or splines"));
            };
            let entity =
                sv.ids.values().find(|id| gusset_naming::entity_tag(id) == Some(e.tag)).cloned().unwrap_or_default();
            if (b[0] - a[0]).hypot(b[1] - a[1]) < 1e-9 {
                continue;
            }
            lines.push(Line { a, b, entity });
        }
        if lines.is_empty() || lines.len() > MAX_RIB_LINES {
            return Err(EvalError::new(
                name,
                ErrorCode::InvalidReference,
                format!("a rib takes 1 to {MAX_RIB_LINES} lines; {} has {}", d.sketch, lines.len()),
            ));
        }
        let closed = match (lines.first(), lines.last()) {
            (Some(f), Some(l)) => lines.len() > 1 && (f.a[0] - l.b[0]).hypot(f.a[1] - l.b[1]) < 1e-6,
            _ => false,
        };
        let (_, body) = ops::target_body(s, name, d.body.as_deref())?;
        let bb = k.bbox(&body.shape).map_err(|ke| kernel_err(name, "bounding box", ke))?;
        let f = chain.frame;
        let centre = scale(add(bb.min, bb.max), 0.5);
        let from = f.to_world(lines.first().map_or([0.0, 0.0], |l| l.a));
        // Far enough that the extended ends leave the body's box on every side.
        let reach = 2.0 * (norm(sub(bb.max, bb.min)) + norm(sub(from, centre))) + 10.0;
        let at: Vec<String> = lines.iter().map(|l| l.entity.clone()).collect();
        let ctx = OpContext::Sheet { feature: name, at: &at };
        let mut tools = Vec::new();
        match &d.growth {
            RibGrowth::InPlane { toward } => {
                let tx = eval_expr(name, &toward[0], Dim::NONE, params, units)?;
                let ty = eval_expr(name, &toward[1], Dim::NONE, params, units)?;
                let (y0, y1) = match d.side {
                    RibSide::Center => (-t / 2.0, t / 2.0),
                    RibSide::Front => (0.0, t),
                    RibSide::Back => (-t, 0.0),
                    RibSide::Left | RibSide::Right => {
                        return Err(EvalError::new(
                            name,
                            ErrorCode::Expression,
                            "a rib in the plane takes side center, front or back",
                        ));
                    }
                };
                let n = f.normal();
                let count = lines.len();
                for (i, l) in lines.iter().enumerate() {
                    let len = (l.b[0] - l.a[0]).hypot(l.b[1] - l.a[1]);
                    let u = [(l.b[0] - l.a[0]) / len, (l.b[1] - l.a[1]) / len];
                    // Left of the line in the sketch; the rib grows to the side `toward` points to.
                    let m = [-u[1], u[0]];
                    let side = m[0] * tx + m[1] * ty;
                    if !(side.abs() > 1e-9 * tx.hypot(ty).max(1e-300)) {
                        return Err(EvalError::new(
                            name,
                            ErrorCode::Expression,
                            format!("toward [{tx}, {ty}] runs along line {}", l.entity),
                        )
                        .with_hint("point toward to the side of the lines the rib grows to"));
                    }
                    let x3: V3 = add(scale(f.x, u[0]), scale(f.y, u[1]));
                    let Some(pf) = Frame::new(f.to_world(l.a), x3, n) else {
                        return Err(EvalError::new(name, ErrorCode::KernelFailure, "the rib's line has no direction"));
                    };
                    // The profile's normal is x3 x n, the line's right: grow left = reverse.
                    let reverse = side > 0.0;
                    let e0 = if i == 0 && !closed { reach } else { 0.0 };
                    let e1 = if i + 1 == count && !closed { reach } else { 0.0 };
                    let profile = Profile { frame: pf, regions: vec![rect(-e0, len + e1, y0, y1, i)] };
                    tools.push(profile_tool(k, name, &profile, &body.shape, reverse)?);
                }
            }
            RibGrowth::Normal { reverse } => {
                let (hl, hr) = match d.side {
                    RibSide::Center => (t / 2.0, t / 2.0),
                    RibSide::Left => (t, 0.0),
                    RibSide::Right => (0.0, t),
                    RibSide::Front | RibSide::Back => {
                        return Err(EvalError::new(
                            name,
                            ErrorCode::Expression,
                            "a rib along the normal takes side center, left or right",
                        ));
                    }
                };
                let region = outline(&lines, hl, hr, if closed { 0.0 } else { reach }, closed);
                let profile = Profile { frame: f, regions: vec![region] };
                tools.push(profile_tool(k, name, &profile, &body.shape, *reverse)?);
            }
        }
        let target = body.feature.clone();
        for tool in tools {
            self.combine(s, name, &ctx, tool, BodyOp::Add, (Some(&target), None))?;
        }
        Ok(())
    }
}

fn profile_tool(
    k: &dyn Kernel,
    name: &str,
    profile: &Profile,
    target: &gusset_kernel::Shape,
    reverse: bool,
) -> Result<OpResult, EvalError> {
    k.rib_to_next(profile, target, reverse).map_err(|ke| {
        let mut err = kernel_err(name, "rib", ke);
        if err.hint.is_none() {
            err.hint =
                Some("the body must lie on the side the rib grows to (toward, or reverse along the normal)".into());
        }
        err
    })
}

/// The outline of a chain of lines thickened `hl` to its left and `hr` to its right, mitred at
/// the joints, its free ends extended by `reach`. A closed chain gives a ring.
fn outline(lines: &[Line], hl: f64, hr: f64, reach: f64, closed: bool) -> Region2 {
    let n = lines.len();
    let dirs: Vec<(P2, P2)> = lines
        .iter()
        .map(|l| {
            let len = (l.b[0] - l.a[0]).hypot(l.b[1] - l.a[1]);
            let u = [(l.b[0] - l.a[0]) / len, (l.b[1] - l.a[1]) / len];
            (u, [-u[1], u[0]])
        })
        .collect();
    // Joint k (0..=n): offset point of the side `h` (signed: + left) where line k-1 meets line k.
    let joint = |k: usize, h: f64| -> P2 {
        let off = |i: usize, p: P2| -> (P2, P2) {
            let (u, m) = dirs.get(i).copied().unwrap_or(([1.0, 0.0], [0.0, 1.0]));
            ([p[0] + m[0] * h, p[1] + m[1] * h], u)
        };
        let prev = if k == 0 { closed.then(|| n - 1) } else { Some(k - 1) };
        let next = if k == n { closed.then_some(0) } else { Some(k) };
        match (prev, next) {
            (Some(i), Some(j)) => {
                let p = lines.get(j).map_or([0.0, 0.0], |l| l.a);
                let (pi, ui) = off(i, p);
                let (pj, uj) = off(j, p);
                cross_point(pi, ui, pj, uj).unwrap_or(pj)
            }
            (None, Some(j)) => {
                let p = lines.get(j).map_or([0.0, 0.0], |l| l.a);
                let (q, u) = off(j, p);
                [q[0] - u[0] * reach, q[1] - u[1] * reach]
            }
            (Some(i), None) => {
                let p = lines.get(i).map_or([0.0, 0.0], |l| l.b);
                let (q, u) = off(i, p);
                [q[0] + u[0] * reach, q[1] + u[1] * reach]
            }
            (None, None) => [0.0, 0.0],
        }
    };
    let l = |a: P2, b: P2| Seg2::Line { a, b };
    let last = if closed { n } else { n + 1 };
    let left: Vec<P2> = (0..last).map(|k| joint(k % (n + 1), hl)).collect();
    let right: Vec<P2> = (0..last).map(|k| joint(k % (n + 1), -hr)).collect();
    if closed {
        // Two loops: the left side outside or inside depending on the chain's turn; the
        // kernel's face builder takes the larger as the outer loop.
        let ring = |pts: &[P2], part: SheetPart| -> Loop2 {
            let edges = (0..pts.len())
                .map(|i| {
                    let a = pts.get(i).copied().unwrap_or_default();
                    let b = pts.get((i + 1) % pts.len()).copied().unwrap_or_default();
                    edge(l(a, b), part, i)
                })
                .collect();
            Loop2 { edges }
        };
        let (lo, ro) = (ring(&left, SheetPart::Flat), ring(&right, SheetPart::FlatBack));
        let area = |pts: &[P2]| -> f64 {
            (0..pts.len())
                .map(|i| {
                    let a = pts.get(i).copied().unwrap_or_default();
                    let b = pts.get((i + 1) % pts.len()).copied().unwrap_or_default();
                    a[0] * b[1] - a[1] * b[0]
                })
                .sum::<f64>()
                .abs()
        };
        return if area(&left) >= area(&right) {
            Region2 { outer: lo, holes: vec![ro] }
        } else {
            Region2 { outer: ro, holes: vec![lo] }
        };
    }
    let mut edges = Vec::new();
    for i in 0..n {
        if let (Some(&a), Some(&b)) = (right.get(i), right.get(i + 1)) {
            edges.push(edge(l(a, b), SheetPart::FlatBack, i));
        }
    }
    if let (Some(&a), Some(&b)) = (right.get(n), left.get(n)) {
        edges.push(edge(l(a, b), SheetPart::End, n.saturating_sub(1)));
    }
    for i in (0..n).rev() {
        if let (Some(&a), Some(&b)) = (left.get(i + 1), left.get(i)) {
            edges.push(edge(l(a, b), SheetPart::Flat, i));
        }
    }
    if let (Some(&a), Some(&b)) = (left.first(), right.first()) {
        edges.push(edge(l(a, b), SheetPart::Start, 0));
    }
    Region2 { outer: Loop2 { edges }, holes: Vec::new() }
}
