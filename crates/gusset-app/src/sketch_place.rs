//! Placing sketch geometry with viewport clicks (DESIGN.md §2, sketch mode). While a sketch
//! is edited and the open form is a placeable sketch command, each click on the sketch plane
//! is a point: a line takes two, a rectangle two opposite corners, a circle its center and a
//! point on it, an arc its center, start and end, a polyline any number (it ends by clicking
//! its first point, which closes it, or Enter). Clicks within a few pixels of an existing
//! sketch point snap to it; a line that starts on one reuses it, so lines join. When the
//! clicks are complete the form is filled and submitted like a typed one, and lines chain:
//! the next line starts at the previous end.

use crate::forms::Form;
use gusset_kernel::Frame;
use gusset_render::Ray;
use serde_json::json;

/// One placed point: sketch coordinates (mm) and the existing point it snapped to.
#[derive(Debug, Clone, PartialEq)]
pub struct Click {
    pub at: [f64; 2],
    pub point: Option<String>,
}

/// How many clicks a command takes: `Some(None)` for open-ended (polyline), `None` when
/// the command is not placed by clicks.
#[must_use]
pub fn clicks_needed(command: &str) -> Option<Option<usize>> {
    match command {
        "sketch.add_line" | "sketch.add_rectangle" | "sketch.add_circle" | "sketch.add_ellipse" => Some(Some(2)),
        "sketch.add_arc" => Some(Some(3)),
        "sketch.add_polyline" => Some(None),
        _ => None,
    }
}

/// Where a ray meets the sketch plane, in sketch coordinates.
#[must_use]
pub fn on_plane(frame: &Frame, ray: &Ray) -> Option<[f64; 2]> {
    let n = cross(frame.x, frame.y);
    let denom = dot(ray.dir, n);
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = dot(sub(frame.origin, ray.origin), n) / denom;
    if !(t.is_finite() && t >= 0.0) {
        return None;
    }
    let p = sub(add(ray.origin, scale(ray.dir, t)), frame.origin);
    Some([dot(p, frame.x), dot(p, frame.y)])
}

/// Snaps to the nearest existing point within `tol` (mm), else rounds to 0.01 mm.
#[must_use]
pub fn snap<'a>(at: [f64; 2], points: impl IntoIterator<Item = (&'a String, &'a [f64; 2])>, tol: f64) -> Click {
    let d = |p: &[f64; 2]| (p[0] - at[0]).hypot(p[1] - at[1]);
    let near = points.into_iter().filter(|(_, p)| d(p) <= tol).min_by(|a, b| d(a.1).total_cmp(&d(b.1)));
    match near {
        Some((id, p)) => Click { at: *p, point: Some(id.clone()) },
        None => Click { at: at.map(|c| (c * 100.0).round() / 100.0), point: None },
    }
}

/// Whether a polyline click closes it (it is on the first point, after at least three).
#[must_use]
pub fn closes(clicks: &[Click], next: &Click, tol: f64) -> bool {
    let [first, .., _] = clicks else { return false };
    clicks.len() >= 3 && (first.at[0] - next.at[0]).hypot(first.at[1] - next.at[1]) <= tol
}

/// Fills the form's fields from complete clicks (`closed` for polylines). Returns false
/// when the clicks are not enough yet.
pub fn fill(form: &mut Form, clicks: &[Click], closed: bool) -> bool {
    let n = |v: f64| format!("{v}");
    match (form.command, clicks) {
        ("sketch.add_line", [a, b]) => {
            form.set("x1", &n(a.at[0]));
            form.set("y1", &n(a.at[1]));
            form.set("x2", &n(b.at[0]));
            form.set("y2", &n(b.at[1]));
            if let Some(p) = &a.point {
                form.set("from_point", p);
            }
            // An end snapped onto a point joins it, so outlines close (review R6).
            if let Some(p) = b.point.as_ref().filter(|p| a.point.as_ref() != Some(*p)) {
                form.set("to_point", p);
            }
            true
        }
        ("sketch.add_rectangle", [a, b]) => {
            form.set("x", &n(a.at[0].min(b.at[0])));
            form.set("y", &n(a.at[1].min(b.at[1])));
            form.set("width", &n(round((a.at[0] - b.at[0]).abs())));
            form.set("height", &n(round((a.at[1] - b.at[1]).abs())));
            true
        }
        ("sketch.add_circle", [c, p]) => {
            form.set("cx", &n(c.at[0]));
            form.set("cy", &n(c.at[1]));
            form.set("radius", &n(round((p.at[0] - c.at[0]).hypot(p.at[1] - c.at[1]))));
            true
        }
        ("sketch.add_ellipse", [c, p]) => {
            // The second click is a corner of the ellipse's bounding box (axes along u / v).
            form.set("cx", &n(c.at[0]));
            form.set("cy", &n(c.at[1]));
            form.set("a", &n(round((p.at[0] - c.at[0]).abs())));
            form.set("b", &n(round((p.at[1] - c.at[1]).abs())));
            true
        }
        ("sketch.add_arc", [c, s, e]) => {
            form.set("cx", &n(c.at[0]));
            form.set("cy", &n(c.at[1]));
            form.set("x1", &n(s.at[0]));
            form.set("y1", &n(s.at[1]));
            form.set("x2", &n(e.at[0]));
            form.set("y2", &n(e.at[1]));
            for (field, click) in [("start_point", s), ("end_point", e)] {
                if let Some(p) = &click.point {
                    form.set(field, p);
                }
            }
            true
        }
        ("sketch.add_polyline", pts) if pts.len() >= 2 => {
            let list: Vec<[f64; 2]> = pts.iter().map(|c| c.at).collect();
            form.set("points", &json!(list).to_string());
            form.set("closed", if closed { "true" } else { "false" });
            true
        }
        _ => false,
    }
}

/// The geometry the clicks so far would make with the cursor as the next point, as
/// polylines in sketch coordinates.
#[must_use]
pub fn preview(command: &str, clicks: &[Click], cursor: Option<[f64; 2]>) -> Vec<Vec<[f64; 2]>> {
    let mut pts: Vec<[f64; 2]> = clicks.iter().map(|c| c.at).collect();
    pts.extend(cursor);
    let circle = |c: [f64; 2], r: f64, a0: f64, a1: f64| -> Vec<[f64; 2]> {
        (0..=48)
            .map(|i| {
                let t = a0 + (a1 - a0) * f64::from(i) / 48.0;
                [c[0] + r * t.cos(), c[1] + r * t.sin()]
            })
            .collect()
    };
    match (command, pts.as_slice()) {
        ("sketch.add_line" | "sketch.add_polyline", p) if p.len() >= 2 => vec![p.to_vec()],
        ("sketch.add_rectangle", [a, b]) => vec![vec![*a, [b[0], a[1]], *b, [a[0], b[1]], *a]],
        ("sketch.add_circle", [c, p]) => vec![circle(*c, (p[0] - c[0]).hypot(p[1] - c[1]), 0.0, std::f64::consts::TAU)],
        ("sketch.add_ellipse", [c, p]) => {
            let (a, b) = ((p[0] - c[0]).abs(), (p[1] - c[1]).abs());
            vec![
                (0..=48)
                    .map(|i| {
                        let t = std::f64::consts::TAU * f64::from(i) / 48.0;
                        [c[0] + a * t.cos(), c[1] + b * t.sin()]
                    })
                    .collect(),
            ]
        }
        ("sketch.add_arc", [c, s]) => vec![vec![*c, *s]],
        ("sketch.add_arc", [c, s, e]) => {
            let r = (s[0] - c[0]).hypot(s[1] - c[1]);
            let a0 = (s[1] - c[1]).atan2(s[0] - c[0]);
            let mut a1 = (e[1] - c[1]).atan2(e[0] - c[0]);
            if a1 <= a0 {
                a1 += std::f64::consts::TAU;
            }
            vec![circle(*c, r, a0, a1)]
        }
        _ => Vec::new(),
    }
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|k| a.get(k).zip(b.get(k)).map_or(0.0, |(x, y)| x + y))
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|k| a.get(k).zip(b.get(k)).map_or(0.0, |(x, y)| x - y))
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    a.map(|x| x * s)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(&b).map(|(x, y)| x * y).sum()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(x: f64, y: f64) -> Click {
        Click { at: [x, y], point: None }
    }

    #[test]
    fn rays_meet_the_sketch_plane_in_sketch_coordinates() {
        let f = Frame { origin: [0.0, 0.0, 5.0], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0] };
        // A plane x = 0 seen along -X.
        let ray = Ray { origin: [10.0, 3.0, 7.0], dir: [-1.0, 0.0, 0.0] };
        assert_eq!(on_plane(&f, &ray), Some([3.0, 2.0]));
        assert_eq!(on_plane(&f, &Ray { origin: [10.0, 3.0, 7.0], dir: [1.0, 0.0, 0.0] }), None);
        assert_eq!(on_plane(&f, &Ray { origin: [10.0, 3.0, 7.0], dir: [0.0, 1.0, 0.0] }), None);
    }

    #[test]
    fn snapping_prefers_the_nearest_point_and_rounds_otherwise() {
        let pts = [("p1".to_string(), [0.0, 0.0]), ("p2".to_string(), [1.0, 0.0])];
        let c = snap([0.8, 0.1], pts.iter().map(|(a, b)| (a, b)), 0.5);
        assert_eq!(c, Click { at: [1.0, 0.0], point: Some("p2".into()) });
        let c = snap([4.56789, 2.0], pts.iter().map(|(a, b)| (a, b)), 0.5);
        assert_eq!(c, Click { at: [4.57, 2.0], point: None });
    }

    #[test]
    fn polylines_close_on_their_first_point_and_previews_follow_the_cursor() {
        let pts = [click(0.0, 0.0), click(10.0, 0.0), click(10.0, 10.0)];
        assert!(closes(&pts, &click(0.1, 0.0), 0.5));
        assert!(!closes(&pts[..2], &click(0.0, 0.0), 0.5));
        assert_eq!(preview("sketch.add_line", &pts[..1], Some([5.0, 5.0])), vec![vec![[0.0, 0.0], [5.0, 5.0]]]);
        assert_eq!(preview("sketch.add_rectangle", &pts[..1], Some([2.0, 3.0]))[0].len(), 5);
        assert!(preview("sketch.add_circle", &pts[..1], None).is_empty());
        let arc = preview("sketch.add_arc", &[click(0.0, 0.0), click(1.0, 0.0)], Some([0.0, 1.0]));
        let end = arc[0].last().copied().unwrap_or_default();
        assert!((end[0]).abs() < 1e-9 && (end[1] - 1.0).abs() < 1e-9, "{end:?}");
        let e = preview("sketch.add_ellipse", &pts[..1], Some([4.0, 2.0]));
        assert_eq!(e[0].len(), 49);
        assert!((e[0][0][0] - 4.0).abs() < 1e-9 && e[0][0][1].abs() < 1e-9);
        assert_eq!(clicks_needed("sketch.add_ellipse"), Some(Some(2)));
        assert_eq!(clicks_needed("sketch.add_polyline"), Some(None));
        assert_eq!(clicks_needed("feature.extrude"), None);
    }
}
