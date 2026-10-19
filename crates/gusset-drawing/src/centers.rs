//! Centre marks and centrelines (ADR 0032 phase 2, ISO 128-24): a cross on every
//! circle seen end-on that spans at least half a turn (holes, bosses, slot ends; not
//! fillets), and a line along every such cylinder seen from the side.

use crate::scene::{PlacedView, Scene, Style};
use gusset_eval::Body;
use gusset_kernel::{CurveKind, SurfaceKind, V3};
use std::f64::consts::PI;
use std::sync::Arc;

/// How far a centre line runs past the outline (sheet mm).
const OVERRUN: f64 = 2.5;
/// Positions closer than this on the sheet are one centre (mm).
const SAME: f64 = 0.01;

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Draws the centre marks and centrelines of `bodies` in view `pv`.
pub(crate) fn draw(scene: &mut Scene, bodies: &[Arc<Body>], pv: &PlacedView) {
    // Centre marks: (sheet centre, sheet radius), the largest of concentric circles.
    let mut marks: Vec<([f64; 2], f64)> = Vec::new();
    // Centrelines: sheet end points.
    let mut lines: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for b in bodies {
        let topo = &b.topology;
        let half_turn = |i: usize| {
            topo.edges.get(i).and_then(|e| match (e.kind, &e.circle) {
                (CurveKind::Circle, Some(c)) if e.length >= PI * c.radius * (1.0 - 1e-9) => Some(*c),
                _ => None,
            })
        };
        for i in 0..topo.edges.len() {
            let Some(c) = half_turn(i) else { continue };
            if dot(c.normal, pv.toward).abs() < 1.0 - 1e-9 {
                continue;
            }
            let at = pv.to_sheet(c.center);
            let r = c.radius * pv.scale;
            match marks.iter_mut().find(|(m, _)| (m[0] - at[0]).abs() < SAME && (m[1] - at[1]).abs() < SAME) {
                Some(m) => m.1 = m.1.max(r),
                None => marks.push((at, r)),
            }
        }
        for (fi, f) in topo.faces.iter().enumerate() {
            let (SurfaceKind::Cylinder, Some(axis)) = (f.kind, &f.axis) else { continue };
            if dot(axis.dir, pv.toward).abs() > 1e-9 {
                continue;
            }
            // The cylinder's extent along its axis, from its circular edges.
            let mut span: Option<(f64, f64)> = None;
            let mut whole = false;
            for (ei, e) in topo.edges.iter().enumerate() {
                if !e.faces.contains(&(fi as u32)) {
                    continue;
                }
                if let Some(c) = half_turn(ei) {
                    whole = true;
                    let t = dot(sub(c.center, axis.origin), axis.dir);
                    span = Some(span.map_or((t, t), |(lo, hi)| (lo.min(t), hi.max(t))));
                }
            }
            let (true, Some((lo, hi))) = (whole, span) else { continue };
            if hi - lo < 1e-9 {
                continue;
            }
            let p = |t: f64| {
                pv.to_sheet([
                    axis.origin[0] + t * axis.dir[0],
                    axis.origin[1] + t * axis.dir[1],
                    axis.origin[2] + t * axis.dir[2],
                ])
            };
            let (a, z) = (p(lo), p(hi));
            let (dx, dy) = (z[0] - a[0], z[1] - a[1]);
            let len = libm::sqrt(dx * dx + dy * dy);
            if len < 1e-9 {
                continue;
            }
            let u = [(z[0] - a[0]) / len, (z[1] - a[1]) / len];
            let a = [a[0] - u[0] * OVERRUN, a[1] - u[1] * OVERRUN];
            let z = [z[0] + u[0] * OVERRUN, z[1] + u[1] * OVERRUN];
            let same = |x: [f64; 2], y: [f64; 2]| (x[0] - y[0]).abs() < SAME && (x[1] - y[1]).abs() < SAME;
            if !lines.iter().any(|(p0, p1)| (same(*p0, a) && same(*p1, z)) || (same(*p0, z) && same(*p1, a))) {
                lines.push((a, z));
            }
        }
    }
    for (c, r) in marks {
        let h = r + OVERRUN;
        scene.line(Style::Center, [c[0] - h, c[1]], [c[0] + h, c[1]]);
        scene.line(Style::Center, [c[0], c[1] - h], [c[0], c[1] + h]);
    }
    for (a, z) in lines {
        scene.line(Style::Center, a, z);
    }
}
