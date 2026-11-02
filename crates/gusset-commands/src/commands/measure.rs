//! `query.measure`: measures one or two named faces or edges from the evaluated topology
//! (no extra kernel work). One item: area, or length and radius. Two items: the angle
//! between their directions (face normals, line directions, axes) and the distances that
//! apply: between parallel planes, from a center to a plane, between parallel lines, and
//! between centers.

use super::*;
use gusset_kernel::{CurveKind, EdgeInfo, FaceInfo, SurfaceKind};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<QueryMeasure>()]
}

/// Measure one or two named faces or edges (`Body: name` picks a body).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryMeasure {
    /// A face or edge name from query_body.
    pub a: String,
    /// A second face or edge: angle and distances to `a`.
    #[serde(default)]
    pub b: Option<String>,
}

impl Command for QueryMeasure {
    const ID: &'static str = "query.measure";
    const TITLE: &'static str = "Measure";
    const DESCRIPTION: &'static str =
        "Area, length or radius of a face or edge; for two, their angle and distances (planes, lines, centers).";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let ev = s.evaluate(&ctx.actor)?;
        let a = find(&ev, &self.a)?;
        let Some(b) = self.b.as_deref() else { return Ok(json!({ "a": a.describe() })) };
        let b = find(&ev, b)?;
        let mut out = serde_json::Map::new();
        out.insert("a".into(), a.describe());
        out.insert("b".into(), b.describe());
        if let (Some(da), Some(db)) = (a.direction(), b.direction()) {
            let c = dot(da, db).abs().min(1.0);
            out.insert("angle_deg".into(), json!(round(libm::acos(c).to_degrees())));
            let parallel = 1.0 - c < 1e-9;
            if let (Some(pa), true) = (a.plane(), parallel)
                && b.plane().is_some()
            {
                out.insert("distance_mm".into(), json!(round(dot(sub(b.center(), pa.0), pa.1).abs())));
                out.insert("distance_kind".into(), json!("between parallel planes"));
            } else if let (Item::Edge(ea), Item::Edge(eb), true) = (&a, &b, parallel)
                && ea.0.kind == CurveKind::Line
                && eb.0.kind == CurveKind::Line
            {
                out.insert("distance_mm".into(), json!(round(norm(cross(sub(b.center(), a.center()), da)))));
                out.insert("distance_kind".into(), json!("between parallel lines"));
            }
        }
        // A center (circle, hole axis, edge middle) to a plane.
        let to_plane = |p: &Item, q: &Item| q.plane().map(|(o, n)| dot(sub(p.center(), o), n).abs());
        if !out.contains_key("distance_mm")
            && let Some(d) = to_plane(&a, &b)
                .filter(|_| b.plane().is_some() && a.plane().is_none())
                .or_else(|| to_plane(&b, &a).filter(|_| a.plane().is_some() && b.plane().is_none()))
        {
            out.insert("distance_mm".into(), json!(round(d)));
            out.insert("distance_kind".into(), json!("from center to plane"));
        }
        out.insert("center_distance_mm".into(), json!(round(norm(sub(b.center(), a.center())))));
        Ok(Value::Object(out))
    }
}

/// A measured face or edge (with the edge's end points).
enum Item {
    Face(FaceInfo),
    Edge((EdgeInfo, Vec<[f64; 3]>)),
}

impl Item {
    /// A face's centroid; an edge's circle center or middle.
    fn center(&self) -> [f64; 3] {
        match self {
            Item::Face(f) => f.centroid,
            Item::Edge((e, _)) => e.circle.map_or(e.midpoint, |c| c.center),
        }
    }

    /// A plane's point and unit normal.
    fn plane(&self) -> Option<([f64; 3], [f64; 3])> {
        match self {
            Item::Face(f) if f.kind == SurfaceKind::Plane => Some((f.centroid, f.normal)),
            _ => None,
        }
    }

    /// The direction angles are measured between: a plane's normal, a revolved face's
    /// axis, a line's direction, a circle's normal.
    fn direction(&self) -> Option<[f64; 3]> {
        let d = match self {
            Item::Face(f) if f.kind == SurfaceKind::Plane => f.normal,
            Item::Face(f) => f.axis?.dir,
            Item::Edge((e, ends)) => match (e.kind, e.circle, ends.as_slice()) {
                (_, Some(c), _) => c.normal,
                (CurveKind::Line, _, [p, q]) => sub(*q, *p),
                _ => return None,
            },
        };
        let n = norm(d);
        (n > 1e-12).then(|| d.map(|v| v / n))
    }

    fn describe(&self) -> Value {
        match self {
            Item::Face(f) => json!({
                "kind": "face",
                "surface": format!("{:?}", f.kind).to_lowercase(),
                "area_mm2": round(f.area),
                "centroid": f.centroid.map(round),
            }),
            Item::Edge((e, _)) => {
                let mut v = serde_json::Map::new();
                v.insert("kind".into(), json!("edge"));
                v.insert("curve".into(), json!(format!("{:?}", e.kind).to_lowercase()));
                v.insert("length_mm".into(), json!(round(e.length)));
                if let Some(c) = e.circle {
                    v.insert("radius_mm".into(), json!(round(c.radius)));
                    v.insert("diameter_mm".into(), json!(round(2.0 * c.radius)));
                    v.insert("center".into(), json!(c.center.map(round)));
                }
                Value::Object(v)
            }
        }
    }
}

/// Finds a face or edge by its name in any body; `Body: name` restricts it to one body.
fn find(ev: &gusset_eval::Evaluation, name: &str) -> Result<Item, CommandError> {
    let (body, name) = match name.split_once(": ") {
        Some((b, n)) => (Some(b.trim()), n.trim()),
        None => (None, name.trim()),
    };
    for b in ev.bodies.iter().filter(|b| body.is_none_or(|n| b.name == n)) {
        if let Some(i) = b.names.faces.iter().position(|f| f.to_string() == name)
            && let Some(f) = b.topology.faces.get(i)
        {
            return Ok(Item::Face(f.clone()));
        }
        if let Some(i) = b.names.edges.iter().position(|e| e.to_string() == name)
            && let Some(e) = b.topology.edges.get(i)
        {
            let ends =
                e.vertices.iter().filter_map(|v| b.topology.vertices.get(*v as usize)).map(|v| v.point).collect();
            return Ok(Item::Edge((e.clone(), ends)));
        }
    }
    Err(CommandError::not_found(format!("no face or edge '{name}'"))
        .hint("query_body with names: true lists the face and edge names"))
}

fn round(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm(a: [f64; 3]) -> f64 {
    libm::sqrt(dot(a, a))
}
