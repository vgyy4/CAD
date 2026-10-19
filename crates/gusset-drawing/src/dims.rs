//! Driven dimensions (ADR 0032 §2): measured from the model each time the sheet is built
//! and drawn after ISO 129-1. A length is measured in the view's plane (what the view
//! shows), so a foreshortened edge reads as drawn.

use crate::scene::{Anchor, PlacedView, Prim, Scene, Style};
use gusset_doc::EdgeRef;
use gusset_doc::drawing::{DimDef, DimDirection, DimKind};
use gusset_eval::Body;
use gusset_kernel::{Circle3, CurveKind, V3};
use gusset_naming::Resolution;
use gusset_units::quantity::LengthUnit;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

/// Arrow length and half width (ISO 129: filled, 30° included angle), sheet mm.
const ARROW_LEN: f64 = 3.0;
const ARROW_HALF: f64 = 0.8;
/// Dimension text cap height (ISO 3098), sheet mm.
pub const TEXT_H: f64 = 3.5;
/// Gap between the geometry and an extension line, and how far the extension line runs
/// past the dimension line.
const EXT_GAP: f64 = 1.0;
const EXT_OVER: f64 = 2.0;
/// Distance of the dimension line from the geometry when the drawing gives none.
pub const DEFAULT_OFFSET: f64 = 10.0;

/// A dimension as built: its value in document units (degrees for angles), or why not.
#[derive(Debug, Clone, PartialEq)]
pub struct DimResult {
    pub name: String,
    pub value: Option<f64>,
    pub text: String,
    pub error: Option<String>,
}

/// An edge as the dimension sees it, in model coordinates.
enum Geo {
    Line(V3, V3),
    Circle(Circle3),
    Other(V3),
}

fn resolve(bodies: &[Arc<Body>], r: &EdgeRef) -> Result<Geo, String> {
    let mut missing = None;
    for b in bodies.iter().rev() {
        match b.names.resolve_edge(r) {
            Resolution::Resolved(i) => {
                let e = b.topology.edges.get(i as usize).ok_or_else(|| format!("edge {r} has no geometry"))?;
                let vtx =
                    |k: usize| e.vertices.get(k).and_then(|v| b.topology.vertices.get(*v as usize)).map(|v| v.point);
                return Ok(match (e.kind, &e.circle, vtx(0), vtx(1)) {
                    (CurveKind::Line, _, Some(a), Some(z)) => Geo::Line(a, z),
                    (CurveKind::Circle, Some(c), _, _) => Geo::Circle(*c),
                    _ => Geo::Other(e.midpoint),
                });
            }
            Resolution::Ambiguous(c) => return Err(format!("edge {r} matches {} edges; re-select it", c.len())),
            Resolution::Missing { reason, .. } => missing = Some(reason),
        }
    }
    Err(format!("edge {r} is not in this view's bodies ({})", missing.unwrap_or_else(|| "no bodies".into())))
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(a: [f64; 2], s: f64) -> [f64; 2] {
    [a[0] * s, a[1] * s]
}
fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn len(a: [f64; 2]) -> f64 {
    libm::sqrt(dot(a, a))
}
fn unit(a: [f64; 2]) -> Option<[f64; 2]> {
    let l = len(a);
    (l > 1e-9).then(|| mul(a, 1.0 / l))
}
fn perp(a: [f64; 2]) -> [f64; 2] {
    [-a[1], a[0]]
}

/// A filled arrow with its tip at `tip`, pointing along `d` (unit).
fn arrow(scene: &mut Scene, style: Style, tip: [f64; 2], d: [f64; 2]) {
    let base = sub(tip, mul(d, ARROW_LEN));
    let w = mul(perp(d), ARROW_HALF);
    scene.push(style, Prim::Fill { points: vec![tip, add(base, w), sub(base, w)] });
}

/// Text along direction `d`, turned to read from the bottom or the right (ISO 129).
fn aligned_text(scene: &mut Scene, at: [f64; 2], d: [f64; 2], text: &str) {
    let mut a = libm::atan2(d[1], d[0]);
    if a > FRAC_PI_2 + 1e-9 {
        a -= PI;
    } else if a <= -FRAC_PI_2 + 1e-9 {
        a += PI;
    }
    let up = [-libm::sin(a), libm::cos(a)];
    let at = add(at, mul(up, 1.0));
    scene.push(Style::Text, Prim::Text { at, height: TEXT_H, text: text.into(), anchor: Anchor::Center, angle: a });
}

/// Rough text width (sheet mm) for placing leaders.
fn text_width(t: &str) -> f64 {
    t.chars().count() as f64 * TEXT_H * 0.75
}

/// A linear dimension between sheet points `p1` and `p2`, measured along `dir` (unit).
/// The dimension line sits `offset` beyond `reach` (how far the view extends on the
/// side `n`, unit and perpendicular to `dir`), or passes through `text_at`.
#[allow(clippy::too_many_arguments)]
fn linear(
    scene: &mut Scene,
    p1: [f64; 2],
    p2: [f64; 2],
    dir: [f64; 2],
    n: [f64; 2],
    reach: f64,
    offset: f64,
    text_at: Option<[f64; 2]>,
    text: &str,
) {
    let base = match text_at {
        Some(t) => dot(t, n),
        None => dot(p1, n).max(dot(p2, n)).max(reach) + offset,
    };
    let foot = |p: [f64; 2]| add(p, mul(n, base - dot(p, n)));
    let (f1, f2) = (foot(p1), foot(p2));
    for (p, f) in [(p1, f1), (p2, f2)] {
        let run = base - dot(p, n);
        let s = if run >= 0.0 { 1.0 } else { -1.0 };
        if run.abs() > EXT_GAP {
            scene.line(Style::Dimension, add(p, mul(n, s * EXT_GAP)), add(f, mul(n, s * EXT_OVER)));
        }
    }
    // Arrows point at the extension lines: inside when there is room, else outside.
    let along = dot(sub(f2, f1), dir);
    let d = if along >= 0.0 { dir } else { mul(dir, -1.0) };
    if along.abs() >= 2.5 * ARROW_LEN {
        scene.line(Style::Dimension, f1, f2);
        arrow(scene, Style::Dimension, f1, mul(d, -1.0));
        arrow(scene, Style::Dimension, f2, d);
    } else {
        scene.line(Style::Dimension, sub(f1, mul(d, 2.0 * ARROW_LEN)), add(f2, mul(d, 2.0 * ARROW_LEN)));
        arrow(scene, Style::Dimension, f1, d);
        arrow(scene, Style::Dimension, f2, mul(d, -1.0));
    }
    let mid = match text_at {
        Some(t) => foot(t),
        None => mul(add(f1, f2), 0.5),
    };
    aligned_text(scene, mid, dir, text);
}

/// The value as written: 2 decimals in mm, 3 in inches, no trailing zeros.
fn number(v: f64, decimals: i32) -> String {
    let s = format!("{:.*}", decimals as usize, v);
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if s == "-0" { "0".into() } else { s }
}

/// What a dimension needs from its view.
#[derive(Debug)]
pub struct ViewCtx<'a> {
    pub bodies: &'a [Arc<Body>],
    pub view: &'a PlacedView,
    pub units: LengthUnit,
}

/// Builds and draws dimension `d`. `offset` and `text_at` are already evaluated.
pub fn draw(
    scene: &mut Scene,
    d: &DimDef,
    v: &ViewCtx<'_>,
    offset: Option<f64>,
    text_at: Option<[f64; 2]>,
) -> DimResult {
    match build(scene, d, v, offset.unwrap_or(DEFAULT_OFFSET), text_at) {
        Ok((value, text)) => DimResult { name: d.name.clone(), value: Some(value), text, error: None },
        Err(e) => {
            if let Some(t) = text_at {
                scene.push(
                    Style::Dangling,
                    Prim::Text {
                        at: t,
                        height: TEXT_H,
                        text: format!("{}?", d.name),
                        anchor: Anchor::Center,
                        angle: 0.0,
                    },
                );
            }
            DimResult { name: d.name.clone(), value: None, text: String::new(), error: Some(e) }
        }
    }
}

fn build(
    scene: &mut Scene,
    d: &DimDef,
    v: &ViewCtx<'_>,
    offset: f64,
    text_at: Option<[f64; 2]>,
) -> Result<(f64, String), String> {
    let pv = v.view;
    let to = |p: V3| pv.to_sheet(p);
    let (factor, decimals) = match v.units {
        LengthUnit::Millimetre => (1.0, 2),
        LengthUnit::Inch => (1.0 / 25.4, 3),
    };
    let label = |value: String| {
        let mut t = String::new();
        if let Some(p) = &d.prefix {
            t.push_str(p);
        }
        t.push_str(&value);
        if let Some(tol) = &d.tolerance {
            t.push(' ');
            t.push_str(tol);
        }
        t
    };
    let center = [(pv.min[0] + pv.max[0]) / 2.0, (pv.min[1] + pv.max[1]) / 2.0];
    // The side away from the view's centre; at the centre, above or to the left.
    let away = |n: [f64; 2], at: [f64; 2]| {
        let side = dot(n, sub(at, center));
        let flip = if side.abs() > 1e-9 {
            side < 0.0
        } else if n[1].abs() > 1e-9 {
            n[1] < 0.0
        } else {
            n[0] > 0.0
        };
        if flip { mul(n, -1.0) } else { n }
    };
    let corners = [pv.min, [pv.max[0], pv.min[1]], pv.max, [pv.min[0], pv.max[1]]];
    let reach = |n: [f64; 2]| corners.iter().map(|c| dot(*c, n)).fold(f64::NEG_INFINITY, f64::max);
    let a = resolve(v.bodies, &d.a)?;
    let b = d.b.as_ref().map(|r| resolve(v.bodies, r)).transpose()?;
    let anchor = |g: &Geo| match g {
        Geo::Line(p, q) => mul(add(to(*p), to(*q)), 0.5),
        Geo::Circle(c) => to(c.center),
        Geo::Other(m) => to(*m),
    };
    let fixed_dir = match d.direction {
        DimDirection::Aligned => None,
        DimDirection::Horizontal => Some([1.0, 0.0]),
        DimDirection::Vertical => Some([0.0, 1.0]),
    };
    match d.kind {
        DimKind::Length | DimKind::Distance => {
            let (p1, p2, aligned_dir) = match (d.kind, &a, &b) {
                (DimKind::Length, Geo::Line(p, q), _) => {
                    let (p1, p2) = (to(*p), to(*q));
                    (p1, p2, unit(sub(p2, p1)))
                }
                (DimKind::Length, _, _) => return Err("a length dimension takes a straight edge".into()),
                (_, Geo::Line(p, q), Some(Geo::Line(r, s))) => {
                    let (a1, a2, b1, b2) = (to(*p), to(*q), to(*r), to(*s));
                    match (unit(sub(a2, a1)), unit(sub(b2, b1))) {
                        // Parallel lines: the distance between them.
                        // Parallel lines: the distance between them, from their ends on the
                        // dimension's side.
                        (Some(da), Some(db)) if cross(da, db).abs() < 1e-6 && fixed_dir.is_none() => {
                            let m = mul(add(a1, a2), 0.5);
                            let n = perp(da);
                            let dir = if dot(sub(b1, m), n) < 0.0 { mul(n, -1.0) } else { n };
                            let side = away(da, mul(add(m, mul(add(b1, b2), 0.5)), 0.5));
                            let end = |x: [f64; 2], y: [f64; 2]| if dot(x, side) >= dot(y, side) { x } else { y };
                            (end(a1, a2), end(b1, b2), Some(dir))
                        }
                        _ => {
                            let (m1, m2) = (anchor(&a), mul(add(b1, b2), 0.5));
                            (m1, m2, unit(sub(m2, m1)))
                        }
                    }
                }
                (_, _, Some(g)) => {
                    let (m1, m2) = (anchor(&a), anchor(g));
                    (m1, m2, unit(sub(m2, m1)))
                }
                (_, _, None) => return Err("a distance dimension takes two edges".into()),
            };
            let dir = fixed_dir.or(aligned_dir).ok_or("the two points coincide in this view")?;
            let value = dot(sub(p2, p1), dir).abs() / pv.scale * factor;
            let text = label(number(value, decimals));
            let n = away(perp(dir), mul(add(p1, p2), 0.5));
            linear(scene, p1, p2, dir, n, reach(n), offset, text_at, &text);
            Ok((value, text))
        }
        DimKind::Diameter | DimKind::Radius => {
            let Geo::Circle(c) = &a else {
                return Err(format!("a {} dimension takes a circular edge", d.kind.keyword()));
            };
            let face_on = c.normal[0] * pv.toward[0] + c.normal[1] * pv.toward[1] + c.normal[2] * pv.toward[2];
            if face_on.abs() < 1.0 - 1e-6 {
                return Err("the circle is not seen face-on in this view".into());
            }
            let cs = to(c.center);
            let r = c.radius * pv.scale;
            let dia = d.kind == DimKind::Diameter;
            let value = if dia { 2.0 * c.radius } else { c.radius } * factor;
            let text = label(format!("{}{}", if dia { "Ø" } else { "R" }, number(value, decimals)));
            let u = text_at.and_then(|t| unit(sub(t, cs))).unwrap_or([libm::sqrt(0.5), libm::sqrt(0.5)]);
            let rim = add(cs, mul(u, r));
            let knee = match text_at {
                Some(t) if len(sub(t, cs)) > r => t,
                _ => add(cs, mul(u, r + offset)),
            };
            let start = if dia { sub(cs, mul(u, r)) } else { cs };
            scene.line(Style::Dimension, start, knee);
            arrow(scene, Style::Dimension, rim, u);
            if dia {
                arrow(scene, Style::Dimension, sub(cs, mul(u, r)), mul(u, -1.0));
            }
            // A horizontal shoulder with the text on it, on the leader's side.
            let w = text_width(&text);
            let s = if u[0] >= 0.0 { 1.0 } else { -1.0 };
            let end = add(knee, [s * w, 0.0]);
            scene.line(Style::Dimension, knee, end);
            scene.push(
                Style::Text,
                Prim::Text {
                    at: [(knee[0] + end[0]) / 2.0, knee[1] + 1.0],
                    height: TEXT_H,
                    text: text.clone(),
                    anchor: Anchor::Center,
                    angle: 0.0,
                },
            );
            Ok((value, text))
        }
        DimKind::Angle => {
            let (Geo::Line(p, q), Some(Geo::Line(r, s))) = (&a, &b) else {
                return Err("an angle dimension takes two straight edges".into());
            };
            let (a1, a2, b1, b2) = (to(*p), to(*q), to(*r), to(*s));
            let (da, db) = match (unit(sub(a2, a1)), unit(sub(b2, b1))) {
                (Some(x), Some(y)) => (x, y),
                _ => return Err("an edge is seen end-on in this view".into()),
            };
            let den = cross(da, db);
            if den.abs() < 1e-9 {
                return Err("the edges are parallel in this view".into());
            }
            let o = add(a1, mul(da, cross(sub(b1, a1), db) / den));
            // Rays from the corner towards each edge's far end.
            let far = |x: [f64; 2], y: [f64; 2]| if len(sub(x, o)) > len(sub(y, o)) { x } else { y };
            let (fa, fb) = (far(a1, a2), far(b1, b2));
            let (ra, rb) = (unit(sub(fa, o)).ok_or("degenerate edge")?, unit(sub(fb, o)).ok_or("degenerate edge")?);
            let value = libm::acos(dot(ra, rb).clamp(-1.0, 1.0)).to_degrees();
            let text = label(format!("{}°", number(value, 2)));
            let radius = match text_at {
                Some(t) => len(sub(t, o)),
                None => (0.6 * len(sub(fa, o)).min(len(sub(fb, o)))).max(8.0) + offset - DEFAULT_OFFSET,
            }
            .max(2.0 * ARROW_LEN);
            let (t1, t2) = (libm::atan2(ra[1], ra[0]), libm::atan2(rb[1], rb[0]));
            let (mut s0, mut e0) = (t1, t2);
            while e0 <= s0 {
                e0 += TAU;
            }
            if e0 - s0 > PI {
                (s0, e0) = (t2, t1);
                while e0 <= s0 {
                    e0 += TAU;
                }
            }
            scene.push(Style::Dimension, Prim::Arc { center: o, radius, start: s0, end: e0 });
            let at = |t: f64| add(o, [radius * libm::cos(t), radius * libm::sin(t)]);
            // Tangents at the arc's ends, pointing outwards along the arc.
            arrow(scene, Style::Dimension, at(s0), [libm::sin(s0), -libm::cos(s0)]);
            arrow(scene, Style::Dimension, at(e0), [-libm::sin(e0), libm::cos(e0)]);
            // Extension lines when the arc lies beyond an edge's end.
            for (ray, f) in [(ra, fa), (rb, fb)] {
                let reach = len(sub(f, o));
                if radius > reach + EXT_GAP {
                    scene.line(
                        Style::Dimension,
                        add(o, mul(ray, reach + EXT_GAP)),
                        add(o, mul(ray, radius + EXT_OVER)),
                    );
                }
            }
            let m = (s0 + e0) / 2.0;
            let tp = match text_at {
                Some(t) => t,
                None => {
                    add(o, [(radius + 2.0 + TEXT_H / 2.0) * libm::cos(m), (radius + 2.0 + TEXT_H / 2.0) * libm::sin(m)])
                }
            };
            scene.push(
                Style::Text,
                Prim::Text {
                    at: [tp[0], tp[1] - TEXT_H / 2.0],
                    height: TEXT_H,
                    text: text.clone(),
                    anchor: Anchor::Center,
                    angle: 0.0,
                },
            );
            Ok((value, text))
        }
    }
}
