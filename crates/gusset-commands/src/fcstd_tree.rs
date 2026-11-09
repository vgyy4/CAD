//! FreeCAD PartDesign history as editable features (ADR 0033 amendment, spec: "FreeCAD
//! .FCStd, aim for tier 1"). FreeCAD is open source and its document format is plain XML:
//! each `PartDesign::Body` lists its features in order (`Group`), sketches store their
//! geometry in their own frame and their placement, and pads, pockets, revolutions and grooves
//! name their profile sketch and their sizes. They are written as a Gusset feature tree
//! (`.gtree.json`, ADR 0042), which the tree importer turns into features; the import checks
//! the result against the shapes FreeCAD stored and keeps the exact shapes when they differ.

use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

/// Most objects, elements per property and features per body read.
const MAX_OBJECTS: usize = 20_000;
const MAX_ELEMENTS: usize = 100_000;
const MAX_FEATURES: usize = 2000;

/// One element inside a property: its tag and attributes.
#[derive(Debug, Clone, Default)]
struct Elem {
    tag: String,
    attrs: HashMap<String, String>,
}

/// An object: its type and its properties (each a list of the elements inside it).
#[derive(Debug, Clone, Default)]
struct Obj {
    kind: String,
    props: HashMap<String, Vec<Elem>>,
}

impl Obj {
    fn elems(&self, prop: &str) -> &[Elem] {
        self.props.get(prop).map_or(&[], Vec::as_slice)
    }
    fn first(&self, prop: &str, tag: &str) -> Option<&Elem> {
        self.elems(prop).iter().find(|e| e.tag == tag)
    }
    fn value(&self, prop: &str, tag: &str) -> Option<&str> {
        self.first(prop, tag).and_then(|e| e.attrs.get("value")).map(String::as_str)
    }
    fn float(&self, prop: &str) -> Option<f64> {
        self.value(prop, "Float").and_then(|v| v.parse().ok()).filter(|v: &f64| v.is_finite())
    }
    fn bool(&self, prop: &str) -> bool {
        self.value(prop, "Bool") == Some("true")
    }
    fn int(&self, prop: &str) -> Option<i64> {
        self.value(prop, "Integer").and_then(|v| v.parse().ok())
    }
    /// The object a Link / LinkSub property names, and its sub-element names.
    fn link(&self, prop: &str) -> Option<(String, Vec<String>)> {
        let e = self.elems(prop);
        let target = e.iter().find(|e| e.tag == "Link" || e.tag == "LinkSub")?.attrs.get("value")?.clone();
        if target.is_empty() {
            return None;
        }
        let subs = e.iter().filter(|e| e.tag == "Sub").filter_map(|e| e.attrs.get("value").cloned()).collect();
        Some((target, subs))
    }
    /// The objects a LinkList property names, in order.
    fn link_list(&self, prop: &str) -> Vec<String> {
        self.elems(prop).iter().filter(|e| e.tag == "Link").filter_map(|e| e.attrs.get("value").cloned()).collect()
    }
}

/// Parses `Document.xml` into objects with all their property elements.
fn parse(xml: &[u8]) -> Result<BTreeMap<String, Obj>, String> {
    if xml.len() > crate::fcstd::MAX_XML {
        return Err("Document.xml is too large".into());
    }
    let mut r = Reader::from_reader(xml);
    r.config_mut().check_end_names = false;
    let mut buf = Vec::new();
    let mut objects: BTreeMap<String, Obj> = BTreeMap::new();
    let (mut in_data, mut current, mut property) = (false, None::<String>, None::<String>);
    let (mut depth, mut elements) = (0u32, 0usize);
    loop {
        let ev = r.read_event_into(&mut buf).map_err(|e| format!("Document.xml: {e}"))?;
        match &ev {
            Event::Start(e) | Event::Empty(e) => {
                if matches!(ev, Event::Start(_)) {
                    depth += 1;
                    if depth > 64 {
                        return Err("Document.xml is nested too deeply".into());
                    }
                }
                let attrs = || -> HashMap<String, String> {
                    e.attributes()
                        .flatten()
                        .take(64)
                        .filter_map(|a| {
                            let k: &str = a.key.as_ref();
                            let k = k.to_string();
                            let v = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?;
                            Some((k, v.chars().take(1000).collect()))
                        })
                        .collect()
                };
                match e.name().as_ref() {
                    "ObjectData" => in_data = true,
                    "Object" => {
                        let a = attrs();
                        let Some(name) = a.get("name").cloned() else { continue };
                        if in_data {
                            current = Some(name);
                        } else {
                            if objects.len() >= MAX_OBJECTS {
                                return Err("the document has too many objects".into());
                            }
                            objects.entry(name).or_default().kind = a.get("type").cloned().unwrap_or_default();
                        }
                    }
                    "Property" => property = attrs().get("name").cloned(),
                    tag => {
                        let (Some(obj), Some(prop)) = (current.as_ref(), property.as_ref()) else { continue };
                        let Some(o) = objects.get_mut(obj) else { continue };
                        elements += 1;
                        if elements > MAX_ELEMENTS {
                            return Err("Document.xml has too many elements".into());
                        }
                        let tag = tag.to_string();
                        o.props.entry(prop.clone()).or_default().push(Elem { tag, attrs: attrs() });
                    }
                }
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                match e.name().as_ref() {
                    "ObjectData" => in_data = false,
                    "Object" => current = None,
                    "Property" => property = None,
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(objects)
}

type V3 = [f64; 3];

/// A rigid placement: rotation (columns are the frame's axes) and translation.
#[derive(Debug, Clone, Copy)]
struct Place {
    x: V3,
    y: V3,
    z: V3,
    p: V3,
}

const IDENTITY: Place = Place { x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0], p: [0.0; 3] };

impl Place {
    /// FreeCAD's placement: position and a unit quaternion (Q0..Q2 the vector part, Q3 the
    /// scalar).
    fn of(o: &Obj) -> Place {
        let Some(e) = o.first("Placement", "PropertyPlacement") else { return IDENTITY };
        let g = |k: &str| e.attrs.get(k).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0);
        let (qx, qy, qz, qw) = (g("Q0"), g("Q1"), g("Q2"), g("Q3"));
        let n = libm::sqrt(qx * qx + qy * qy + qz * qz + qw * qw);
        if n < 1e-12 {
            return Place { p: [g("Px"), g("Py"), g("Pz")], ..IDENTITY };
        }
        let (x, y, z, w) = (qx / n, qy / n, qz / n, qw / n);
        Place {
            x: [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w)],
            y: [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w)],
            z: [2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y)],
            p: [g("Px"), g("Py"), g("Pz")],
        }
    }

    fn dir(&self, v: V3) -> V3 {
        [
            self.x[0] * v[0] + self.y[0] * v[1] + self.z[0] * v[2],
            self.x[1] * v[0] + self.y[1] * v[1] + self.z[1] * v[2],
            self.x[2] * v[0] + self.y[2] * v[1] + self.z[2] * v[2],
        ]
    }

    fn point(&self, v: V3) -> V3 {
        let d = self.dir(v);
        [d[0] + self.p[0], d[1] + self.p[1], d[2] + self.p[2]]
    }

    /// `self` after `inner` (inner's frame given in self's).
    fn then(&self, inner: &Place) -> Place {
        Place { x: self.dir(inner.x), y: self.dir(inner.y), z: self.dir(inner.z), p: self.point(inner.p) }
    }
}

/// A sketch written as a tree sketch; returns the feature JSON and its curve ids by FreeCAD's
/// names for them (`Edge1`.., `Axis0`..), for axes and paths.
fn sketch(name: &str, o: &Obj, body: &Place) -> Result<(Value, HashMap<String, String>), String> {
    let pl = body.then(&Place::of(o));
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut pjson = Vec::new();
    let mut point = |x: f64, y: f64, pjson: &mut Vec<Value>| -> String {
        // Ends that meet share one point, so loops close.
        if let Some(i) = points.iter().position(|(a, b)| (a - x).abs() < 1e-7 && (b - y).abs() < 1e-7) {
            return format!("p{}", i + 1);
        }
        points.push((x, y));
        let id = format!("p{}", points.len());
        pjson.push(json!({"id": id, "x": x, "y": y}));
        id
    };
    let (mut curves, mut lines): (Vec<Value>, HashMap<String, String>) = (Vec::new(), HashMap::new());
    let mut construction = false;
    let mut edge = 0usize;
    let num = |e: &Elem, k: &str| -> Result<f64, String> {
        e.attrs
            .get(k)
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("{name}: a bad {k}"))
    };
    for e in o.elems("Geometry") {
        match e.tag.as_str() {
            "Construction" => construction = e.attrs.get("value").is_some_and(|v| v == "1"),
            "GeoExtension" if e.attrs.get("type").is_some_and(|t| t.contains("SketchGeometryExtension")) => {
                // Newer files keep the construction flag here (geometry mode bit 0).
                if let Some(m) = e.attrs.get("geometryModeFlags") {
                    construction = m.ends_with('1');
                }
            }
            "LineSegment" => {
                edge += 1;
                let a = point(num(e, "StartX")?, num(e, "StartY")?, &mut pjson);
                let b = point(num(e, "EndX")?, num(e, "EndY")?, &mut pjson);
                if a == b {
                    return Err(format!("{name}: a line of zero length"));
                }
                let id = format!("l{edge}");
                curves.push(json!({"id": id, "type": "line", "start": a, "end": b, "construction": construction}));
                construction = false;
            }
            "Circle" => {
                edge += 1;
                let c = point(num(e, "CenterX")?, num(e, "CenterY")?, &mut pjson);
                let r = num(e, "Radius")?;
                curves.push(json!({"id": format!("c{edge}"), "type": "circle", "center": c, "radius": r, "construction": construction}));
                construction = false;
            }
            "ArcOfCircle" => {
                edge += 1;
                let (cx, cy, r) = (num(e, "CenterX")?, num(e, "CenterY")?, num(e, "Radius")?);
                let (a0, a1) = (num(e, "StartAngle")?, num(e, "EndAngle")?);
                // Counter-clockwise from the start angle to the end angle about the sketch
                // normal (a mirrored arc's normal points the other way: refused).
                if e.attrs.get("NormalZ").and_then(|v| v.parse::<f64>().ok()).is_some_and(|z| z < 0.0) {
                    return Err(format!("{name}: a clockwise arc"));
                }
                let c = point(cx, cy, &mut pjson);
                let s = point(cx + r * libm::cos(a0), cy + r * libm::sin(a0), &mut pjson);
                let t = point(cx + r * libm::cos(a1), cy + r * libm::sin(a1), &mut pjson);
                curves.push(json!({"id": format!("a{edge}"), "type": "arc", "center": c, "start": s, "end": t, "construction": construction}));
                construction = false;
            }
            "Point" => {
                edge += 1;
                construction = false;
            }
            t if matches!(
                t,
                "Ellipse"
                    | "ArcOfEllipse"
                    | "ArcOfHyperbola"
                    | "ArcOfParabola"
                    | "BSplineCurve"
                    | "Hyperbola"
                    | "Parabola"
            ) =>
            {
                return Err(format!("{name}: {t} curves are not translated yet"));
            }
            _ => {}
        }
    }
    if curves.is_empty() {
        return Err(format!("{name}: an empty sketch"));
    }
    let f = json!({
        "type": "sketch",
        "name": name,
        "plane": {"origin": pl.p, "x_axis": pl.x, "y_axis": pl.y},
        "points": pjson,
        "curves": curves,
    });
    // FreeCAD names a sketch's curves `Edge1`.. over the non-construction ones, in order, and
    // its construction lines `Axis0`..
    let (mut ne, mut na) = (0usize, 0usize);
    for c in f.get("curves").and_then(Value::as_array).into_iter().flatten() {
        let id = c.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        if c.get("construction").and_then(Value::as_bool) == Some(true) {
            if c.get("type").and_then(Value::as_str) == Some("line") {
                lines.insert(format!("Axis{na}"), id);
                na += 1;
            }
        } else {
            ne += 1;
            lines.insert(format!("Edge{ne}"), id);
        }
    }
    Ok((f, lines))
}

/// What a reference can name: the document's objects, the body's placement and its sketches
/// read so far (frame and lines).
struct Refs<'a> {
    objects: &'a BTreeMap<String, Obj>,
    body: &'a Place,
    sketches: &'a HashMap<String, (Place, HashMap<String, String>)>,
}

impl Refs<'_> {
    /// An axis (point and direction) a LinkSub names: a sketch's own axes, the body origin's
    /// axes or a datum line.
    fn axis(&self, who: &str, (obj, subs): &(String, Vec<String>)) -> Result<(V3, V3), String> {
        let sub = subs.first().map(String::as_str).unwrap_or_default();
        if let Some((sk, _)) = self.sketches.get(obj) {
            return match sub {
                "H_Axis" => Ok((sk.p, sk.x)),
                "V_Axis" => Ok((sk.p, sk.y)),
                "N_Axis" => Ok((sk.p, sk.z)),
                _ => Err(format!("{who}: an axis on a sketch edge is not translated yet")),
            };
        }
        let kind = self.objects.get(obj).map(|o| o.kind.as_str()).unwrap_or_default();
        if let Some(d) = self.objects.get(obj).filter(|d| d.kind == "PartDesign::Line" && sub.is_empty()) {
            let pl = self.body.then(&Place::of(d));
            return Ok((pl.p, pl.z));
        }
        let up = obj.to_ascii_uppercase();
        let b = self.body;
        if up.starts_with("X_AXIS") {
            Ok((b.p, b.x))
        } else if up.starts_with("Y_AXIS") {
            Ok((b.p, b.y))
        } else if up.starts_with("Z_AXIS") {
            Ok((b.p, b.z))
        } else {
            Err(format!(
                "{who}: an axis on {} is not translated yet",
                if kind.is_empty() { "a missing object" } else { kind }
            ))
        }
    }

    /// A plane (point and normal) a LinkSub names: a sketch axis (the plane holds that axis
    /// and the sketch normal), the sketch plane, the origin's planes or a datum plane.
    fn plane(&self, who: &str, (obj, subs): &(String, Vec<String>)) -> Result<(V3, V3), String> {
        let sub = subs.first().map(String::as_str).unwrap_or_default();
        if let Some((sk, _)) = self.sketches.get(obj) {
            return match sub {
                "H_Axis" => Ok((sk.p, sk.y)),
                "V_Axis" => Ok((sk.p, sk.x)),
                "" | "N_Axis" => Ok((sk.p, sk.z)),
                _ => Err(format!("{who}: a plane on a sketch edge is not translated yet")),
            };
        }
        let kind = self.objects.get(obj).map(|o| o.kind.as_str()).unwrap_or_default();
        if let Some(d) = self.objects.get(obj).filter(|d| d.kind == "PartDesign::Plane" && sub.is_empty()) {
            let pl = self.body.then(&Place::of(d));
            return Ok((pl.p, pl.z));
        }
        let up = obj.to_ascii_uppercase();
        let b = self.body;
        if up.starts_with("XY_PLANE") {
            Ok((b.p, b.z))
        } else if up.starts_with("XZ_PLANE") {
            Ok((b.p, b.y))
        } else if up.starts_with("YZ_PLANE") {
            Ok((b.p, b.x))
        } else {
            Err(format!(
                "{who}: a mirror plane on {} is not translated yet",
                if kind.is_empty() { "a missing object" } else { kind }
            ))
        }
    }
}

/// Adds a construction line along an axis to a sketch feature already written, when the axis
/// lies in the sketch's plane; returns its id.
fn axis_line(features: &mut [Value], sketch: &str, sk: &Place, o: V3, d: V3) -> Option<String> {
    let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let rel = [o[0] - sk.p[0], o[1] - sk.p[1], o[2] - sk.p[2]];
    if dot(d, sk.z).abs() > 1e-9 || dot(rel, sk.z).abs() > 1e-6 {
        return None;
    }
    let f = features.iter_mut().rev().find(|f| {
        f.get("type").and_then(Value::as_str) == Some("sketch") && f.get("name").and_then(Value::as_str) == Some(sketch)
    })?;
    let (u, v, du, dv) = (dot(rel, sk.x), dot(rel, sk.y), dot(d, sk.x), dot(d, sk.y));
    let n = f.get("curves").and_then(Value::as_array).map_or(0, Vec::len);
    let id = format!("axis{n}");
    let (a, b) = (format!("{id}a"), format!("{id}b"));
    f.get_mut("points")?
        .as_array_mut()?
        .extend([json!({"id": a, "x": u, "y": v}), json!({"id": b, "x": u + 10.0 * du, "y": v + 10.0 * dv})]);
    f.get_mut("curves")?
        .as_array_mut()?
        .push(json!({"id": id, "type": "line", "start": a, "end": b, "construction": true}));
    Some(id)
}

/// One transformation of a pattern as a tree feature (without its features and target), or
/// none when it makes no copies.
fn transform(name: &str, o: &Obj, refs: &Refs<'_>) -> Result<Option<Value>, String> {
    let count = o.int("Occurrences").unwrap_or(2);
    match o.kind.as_str() {
        "PartDesign::Mirrored" => {
            let link = o.link("MirrorPlane").ok_or(format!("{name}: no mirror plane"))?;
            let (origin, normal) = refs.plane(name, &link)?;
            Ok(Some(
                json!({"type": "mirror", "name": name, "plane": {"origin": origin, "normal": normal}, "merge": true}),
            ))
        }
        "PartDesign::LinearPattern" => {
            if count < 2 {
                return Ok(None);
            }
            let link = o.link("Direction").ok_or(format!("{name}: no direction"))?;
            let (_, mut dir) = refs.axis(name, &link)?;
            if o.bool("Reversed") {
                dir = dir.map(|c| -c);
            }
            // Mode 0: the overall length; mode 1: the spacing (`Offset`).
            let spacing = match o.int("Mode").unwrap_or(0) {
                0 => o.float("Length").ok_or(format!("{name}: no length"))? / (count - 1) as f64,
                1 => o.float("Offset").ok_or(format!("{name}: no offset"))?,
                _ => return Err(format!("{name}: this pattern mode is not translated yet")),
            };
            Ok(Some(
                json!({"type": "pattern_linear", "name": name, "direction": dir, "spacing": spacing, "count": count}),
            ))
        }
        "PartDesign::PolarPattern" => {
            if count < 2 {
                return Ok(None);
            }
            let link = o.link("Axis").ok_or(format!("{name}: no axis"))?;
            let (origin, mut dir) = refs.axis(name, &link)?;
            if o.bool("Reversed") {
                dir = dir.map(|c| -c);
            }
            // Mode 0: the overall angle (a full turn spaces evenly, as in Gusset); mode 1:
            // the angle between copies (`Offset`).
            let angle = match o.int("Mode").unwrap_or(0) {
                0 => o.float("Angle").unwrap_or(360.0),
                1 => o.float("Offset").ok_or(format!("{name}: no offset"))? * (count - 1) as f64,
                _ => return Err(format!("{name}: this pattern mode is not translated yet")),
            };
            Ok(Some(json!({"type": "pattern_circular", "name": name, "axis": {"origin": origin, "direction": dir},
                "angle": angle, "count": count})))
        }
        other => Err(format!("{name}: {other} is not translated yet")),
    }
}

/// A PartDesign primitive as a profile sketch and an extrusion or revolution (placed by the
/// primitive's placement in the body).
fn primitive(name: &str, o: &Obj, body: &Place) -> Result<Vec<Value>, String> {
    let pl = body.then(&Place::of(o));
    let pos = |k: &str| o.float(k).filter(|v| *v > 0.0).ok_or(format!("{name}: no positive {k}"));
    let sk = format!("{name}_profile");
    let kind =
        o.kind.trim_start_matches("PartDesign::").trim_start_matches("Additive").trim_start_matches("Subtractive");
    let full = |k: &str| o.float(k).is_none_or(|a| (a - 360.0).abs() < 1e-9);
    match kind {
        "Box" => {
            let (l, w, h) = (pos("Length")?, pos("Width")?, pos("Height")?);
            let pts = [(0.0, 0.0), (l, 0.0), (l, w), (0.0, w)];
            let points: Vec<Value> = pts
                .iter()
                .enumerate()
                .map(|(i, (x, y))| json!({"id": format!("p{}", i + 1), "x": x, "y": y}))
                .collect();
            let curves: Vec<Value> = (0..4)
                .map(|i| json!({"id": format!("l{}", i + 1), "type": "line", "start": format!("p{}", i + 1), "end": format!("p{}", (i + 1) % 4 + 1)}))
                .collect();
            Ok(vec![
                json!({"type": "sketch", "name": sk, "plane": {"origin": pl.p, "x_axis": pl.x, "y_axis": pl.y}, "points": points, "curves": curves}),
                json!({"type": "extrude", "name": name, "sketch": sk, "profiles": "all", "distance": h, "symmetric": false}),
            ])
        }
        "Cylinder" => {
            let (r, h) = (pos("Radius")?, pos("Height")?);
            if !full("Angle")
                || o.float("FirstAngle").is_some_and(|a| a != 0.0)
                || o.float("SecondAngle").is_some_and(|a| a != 0.0)
            {
                return Err(format!("{name}: a partial or slanted cylinder is not translated yet"));
            }
            Ok(vec![
                json!({"type": "sketch", "name": sk, "plane": {"origin": pl.p, "x_axis": pl.x, "y_axis": pl.y},
                    "points": [{"id": "c", "x": 0.0, "y": 0.0}], "curves": [{"id": "k", "type": "circle", "center": "c", "radius": r}]}),
                json!({"type": "extrude", "name": name, "sketch": sk, "profiles": "all", "distance": h, "symmetric": false}),
            ])
        }
        "Cone" | "Sphere" => {
            // A half section in the plane of the primitive's x and z axes, turned about z.
            let (points, curves) = if kind == "Cone" {
                let (r1, r2, h) =
                    (o.float("Radius1").unwrap_or(0.0), o.float("Radius2").unwrap_or(0.0), pos("Height")?);
                if r1 < 0.0 || r2 < 0.0 || (r1 <= 0.0 && r2 <= 0.0) {
                    return Err(format!("{name}: a cone without radius"));
                }
                let mut pts = vec![(0.0, 0.0)];
                if r1 > 0.0 {
                    pts.push((r1, 0.0));
                }
                if r2 > 0.0 {
                    pts.push((r2, h));
                }
                pts.push((0.0, h));
                let n = pts.len();
                let points: Vec<Value> = pts
                    .iter()
                    .enumerate()
                    .map(|(i, (x, y))| json!({"id": format!("p{}", i + 1), "x": x, "y": y}))
                    .collect();
                let curves: Vec<Value> = (0..n)
                    .map(|i| json!({"id": format!("l{}", i + 1), "type": "line", "start": format!("p{}", i + 1), "end": format!("p{}", (i + 1) % n + 1)}))
                    .collect();
                (points, curves)
            } else {
                let r = pos("Radius")?;
                if !full("Angle3")
                    || o.float("Angle1").is_some_and(|a| (a + 90.0).abs() > 1e-9)
                    || o.float("Angle2").is_some_and(|a| (a - 90.0).abs() > 1e-9)
                {
                    return Err(format!("{name}: a partial sphere is not translated yet"));
                }
                (
                    vec![
                        json!({"id": "c", "x": 0.0, "y": 0.0}),
                        json!({"id": "s", "x": 0.0, "y": -r}),
                        json!({"id": "t", "x": 0.0, "y": r}),
                    ],
                    vec![
                        json!({"id": "a", "type": "arc", "center": "c", "start": "s", "end": "t"}),
                        json!({"id": "l", "type": "line", "start": "t", "end": "s"}),
                    ],
                )
            };
            if kind == "Cone" && !full("Angle") {
                return Err(format!("{name}: a partial cone is not translated yet"));
            }
            Ok(vec![
                json!({"type": "sketch", "name": sk, "plane": {"origin": pl.p, "x_axis": pl.x, "y_axis": pl.z}, "points": points, "curves": curves}),
                json!({"type": "revolve", "name": name, "sketch": sk, "profiles": "all", "axis": {"origin": pl.p, "direction": pl.z}, "angle": 360.0}),
            ])
        }
        _ => Err(format!("{name}: {} is not translated yet", o.kind)),
    }
}

/// Whether the document holds a PartDesign body worth translating.
#[must_use]
pub(crate) fn has_bodies(xml: &[u8]) -> bool {
    parse(xml).is_ok_and(|o| o.values().any(|o| o.kind == "PartDesign::Body"))
}

/// The edges of a stored shape file, in FreeCAD's numbering (`Edge1` first): each edge's
/// midpoint and the 0-based numbers of the faces it bounds.
pub(crate) type Edges = Vec<([f64; 3], Vec<u32>)>;
/// Reads [`Edges`] of a shape file.
pub(crate) type ShapeEdges<'a> = &'a mut dyn FnMut(&str) -> Result<Edges, String>;

/// Most edges one fillet or chamfer picks.
const MAX_EDGE_PICKS: usize = 256;

/// The document's PartDesign bodies as a feature tree (format version 2), or why not. Every
/// feature of every body must be translated: one that is not makes the whole tree refused
/// (the exact shapes then stand in).
///
/// # Errors
/// The first feature that cannot be written, or a damaged document.
pub(crate) fn tree(xml: &[u8], shape_edges: ShapeEdges<'_>) -> Result<crate::tree_import::Tree, String> {
    let objects = parse(xml)?;
    // Solids made outside PartDesign bodies (Part workbench booleans, sweeps, arrays) are not
    // history this translates; the stored shapes stand in for the whole document.
    if let Some((n, o)) = objects.iter().find(|(_, o)| {
        o.props.contains_key("Shape")
            && !o.kind.starts_with("PartDesign::")
            && !o.kind.starts_with("Sketcher::")
            && !o.kind.starts_with("App::")
    }) {
        return Err(format!("{n}: a {} outside a PartDesign body is not translated yet", o.kind));
    }
    let mut features: Vec<Value> = Vec::new();
    let mut sketches: HashMap<String, (Place, HashMap<String, String>)> = HashMap::new();
    let mut bodies = 0usize;
    for (body_name, body) in objects.iter().filter(|(_, o)| o.kind == "PartDesign::Body") {
        bodies += 1;
        let place = Place::of(body);
        let mut first_solid: Option<String> = None;
        let group = body.link_list("Group");
        if group.len() > MAX_FEATURES {
            return Err(format!("{body_name}: too many features"));
        }
        // A MultiTransform's steps are listed in the body too; they act through it.
        let steps: std::collections::HashSet<String> = group
            .iter()
            .filter_map(|n| objects.get(n))
            .filter(|o| o.kind == "PartDesign::MultiTransform")
            .flat_map(|o| o.link_list("Transformations"))
            .collect();
        // The body's shape is its Tip's: features after it are not part of the result.
        let tip = body.link("Tip").map(|(t, _)| t).filter(|t| group.contains(t));
        let mut past_tip = false;
        for name in &group {
            if past_tip {
                break;
            }
            past_tip = tip.as_ref() == Some(name);
            let Some(o) = objects.get(name) else { continue };
            if steps.contains(name) {
                continue;
            }
            let kind = o.kind.as_str();
            match kind {
                "Sketcher::SketchObject" => {
                    let (f, lines) = sketch(name, o, &place)?;
                    sketches.insert(name.clone(), (place.then(&Place::of(o)), lines));
                    features.push(f);
                }
                "PartDesign::Pad" | "PartDesign::Pocket" => {
                    let pocket = kind == "PartDesign::Pocket";
                    let (profile, _) =
                        o.link("Profile").or_else(|| o.link("Sketch")).ok_or(format!("{name}: no profile"))?;
                    if !sketches.contains_key(&profile) {
                        return Err(format!("{name}: its profile is not a sketch of the body"));
                    }
                    let ty = o.int("Type").unwrap_or(0);
                    let len = o.float("Length").unwrap_or(0.0);
                    let distance = match (pocket, ty) {
                        (_, 0 | 4) => len,
                        // Through all: far enough for any part (the volume check confirms).
                        (true, 1) => 100_000.0,
                        _ => {
                            return Err(format!(
                                "{name}: this {} type is not translated yet",
                                if pocket { "pocket" } else { "pad" }
                            ));
                        }
                    };
                    if o.bool("UseCustomVector") {
                        return Err(format!("{name}: a custom direction is not translated yet"));
                    }
                    // A pad grows along the sketch normal, a pocket cuts against it;
                    // `Reversed` flips either.
                    let mut d = if pocket { -distance } else { distance };
                    if o.bool("Reversed") {
                        d = -d;
                    }
                    let symmetric = o.bool("Midplane");
                    let op = match (&first_solid, pocket) {
                        (None, false) => "new",
                        (None, true) => return Err(format!("{name}: a pocket before any solid")),
                        (Some(_), false) => "join",
                        (Some(_), true) => "cut",
                    };
                    let mut f = json!({"type": "extrude", "name": name, "sketch": profile, "profiles": "all",
                        "distance": if symmetric { distance } else { d }, "symmetric": symmetric, "operation": op});
                    if let (Some(t), Some(m)) = (&first_solid, f.as_object_mut()) {
                        m.insert("target".into(), json!(t));
                    }
                    features.push(f);
                    // Two lengths: the second goes the other way from the sketch, written as a
                    // second extrusion of the same profile into the same body.
                    if ty == 4 && !symmetric {
                        let len2 = o.float("Length2").unwrap_or(0.0);
                        if len2 > 0.0 {
                            let back = format!("{name}_back");
                            let op2 = if pocket { "cut" } else { "join" };
                            let t = first_solid.clone().unwrap_or_else(|| name.clone());
                            features
                                .push(json!({"type": "extrude", "name": back, "sketch": profile, "profiles": "all",
                                "distance": -d.signum() * len2, "symmetric": false, "operation": op2, "target": t}));
                        }
                    }
                    first_solid.get_or_insert_with(|| name.clone());
                }
                "PartDesign::Revolution" | "PartDesign::Groove" => {
                    let groove = kind == "PartDesign::Groove";
                    let (profile, _) =
                        o.link("Profile").or_else(|| o.link("Sketch")).ok_or(format!("{name}: no profile"))?;
                    let (sk_place, lines) =
                        sketches.get(&profile).ok_or(format!("{name}: its profile is not a sketch of the body"))?;
                    let (axis_obj, subs) = o.link("ReferenceAxis").ok_or(format!("{name}: no axis"))?;
                    let sub = subs.first().map(String::as_str).unwrap_or_default();
                    let mut axis_od: Option<(V3, V3)> = None;
                    let mut axis = if axis_obj == profile {
                        match sub {
                            "V_Axis" => json!({"origin": sk_place.p, "direction": sk_place.y}),
                            "H_Axis" => json!({"origin": sk_place.p, "direction": sk_place.x}),
                            "N_Axis" => json!({"origin": sk_place.p, "direction": sk_place.z}),
                            s => match lines.get(s).filter(|l| l.starts_with('l')) {
                                Some(l) => json!({"line": l}),
                                None => return Err(format!("{name}: its axis is not a sketch line")),
                            },
                        }
                    } else {
                        // Another object's axis: drawn into the profile sketch as a construction
                        // line (Gusset revolves about a sketch line or the sketch's own axes).
                        let sk = *sk_place;
                        let refs = Refs { objects: &objects, body: &place, sketches: &sketches };
                        let (o3, d3) = refs.axis(name, &(axis_obj.clone(), subs.clone()))?;
                        axis_od = Some((o3, d3));
                        match axis_line(&mut features, &profile, &sk, o3, d3) {
                            Some(id) => json!({"line": id}),
                            None => json!({"origin": o3, "direction": d3}),
                        }
                    };
                    let angle = o.float("Angle").unwrap_or(360.0);
                    if o.bool("Midplane") || o.int("Type").unwrap_or(0) != 0 {
                        return Err(format!("{name}: a symmetric or bounded revolution is not translated yet"));
                    }
                    // Gusset turns about its axis one way: a reversed part turn is a turn about
                    // the reversed axis, drawn into the sketch (a full turn needs neither).
                    if o.bool("Reversed") && angle < 360.0 - 1e-9 {
                        let v3 = |k: &str| -> Option<V3> {
                            let a = axis.get(k)?.as_array()?;
                            Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
                        };
                        let (ao, ad) = axis_od
                            .or_else(|| Some((v3("origin")?, v3("direction")?)))
                            .ok_or(format!("{name}: a reversed turn about a sketch edge is not translated yet"))?;
                        let rev = ad.map(|c| -c);
                        let sk = *sk_place;
                        axis = match axis_line(&mut features, &profile, &sk, ao, rev) {
                            Some(id) => json!({"line": id}),
                            None => json!({"origin": ao, "direction": rev}),
                        };
                    }
                    let op = match (&first_solid, groove) {
                        (None, false) => "new",
                        (None, true) => return Err(format!("{name}: a groove before any solid")),
                        (Some(_), false) => "join",
                        (Some(_), true) => "cut",
                    };
                    let mut f = json!({"type": "revolve", "name": name, "sketch": profile, "profiles": "all",
                        "axis": axis, "angle": angle, "operation": op});
                    if let (Some(t), Some(m)) = (&first_solid, f.as_object_mut()) {
                        m.insert("target".into(), json!(t));
                    }
                    first_solid.get_or_insert_with(|| name.clone());
                    features.push(f);
                }
                "PartDesign::Mirrored"
                | "PartDesign::LinearPattern"
                | "PartDesign::PolarPattern"
                | "PartDesign::MultiTransform" => {
                    let target = first_solid.clone().ok_or(format!("{name}: a pattern before any solid"))?;
                    // Mode 1 transforms the whole body; otherwise the listed features.
                    let whole = o.int("TransformMode").unwrap_or(0) == 1;
                    let originals = o.link_list("Originals");
                    // Gusset repeats features that change a body; repeating the body's first
                    // feature is a transform of the whole body (the check confirms).
                    let whole = whole || originals.contains(&target);
                    let mut feats: Vec<String> = if whole { Vec::new() } else { originals };
                    if !whole && feats.is_empty() {
                        return Err(format!("{name}: a pattern of no features"));
                    }
                    let steps: Vec<(String, &Obj)> = if kind == "PartDesign::MultiTransform" {
                        let list = o.link_list("Transformations");
                        let n = list.len();
                        list.into_iter()
                            .enumerate()
                            .map(|(i, t)| {
                                let so = objects.get(&t).ok_or(format!("{name}: a missing transformation"))?;
                                // The last step carries the feature's own name, for later references.
                                Ok((if i + 1 == n { name.clone() } else { t }, so))
                            })
                            .collect::<Result<_, String>>()?
                    } else {
                        vec![(name.clone(), o)]
                    };
                    if steps.is_empty() {
                        return Err(format!("{name}: no transformations"));
                    }
                    for (step_name, so) in steps {
                        let refs = Refs { objects: &objects, body: &place, sketches: &sketches };
                        let Some(mut f) = transform(&step_name, so, &refs)? else { continue };
                        if let Some(m) = f.as_object_mut() {
                            m.insert("features".into(), json!(feats));
                            m.insert("target".into(), json!(target));
                        }
                        features.push(f);
                        if !whole {
                            feats.push(step_name);
                        }
                    }
                }
                "PartDesign::AdditiveBox"
                | "PartDesign::SubtractiveBox"
                | "PartDesign::AdditiveCylinder"
                | "PartDesign::SubtractiveCylinder"
                | "PartDesign::AdditiveCone"
                | "PartDesign::SubtractiveCone"
                | "PartDesign::AdditiveSphere"
                | "PartDesign::SubtractiveSphere" => {
                    let sub = kind.starts_with("PartDesign::Subtractive");
                    let op = match (&first_solid, sub) {
                        (None, false) => "new",
                        (None, true) => return Err(format!("{name}: a subtractive primitive before any solid")),
                        (Some(_), false) => "join",
                        (Some(_), true) => "cut",
                    };
                    for mut f in primitive(name, o, &place)? {
                        if f.get("type").and_then(Value::as_str) != Some("sketch")
                            && let Some(m) = f.as_object_mut()
                        {
                            m.insert("operation".into(), json!(op));
                            if let Some(t) = &first_solid {
                                m.insert("target".into(), json!(t));
                            }
                        }
                        features.push(f);
                    }
                    first_solid.get_or_insert_with(|| name.clone());
                }
                "PartDesign::Fillet" | "PartDesign::Chamfer" => {
                    let fillet = kind == "PartDesign::Fillet";
                    let target = first_solid.clone().ok_or(format!("{name}: an edge feature before any solid"))?;
                    let (base, subs) = o.link("Base").ok_or(format!("{name}: no edges"))?;
                    let file = objects
                        .get(&base)
                        .and_then(|b| b.first("Shape", "Part"))
                        .and_then(|e| e.attrs.get("file"))
                        .filter(|f| !f.is_empty())
                        .ok_or(format!("{name}: its base shape is not stored in the file"))?;
                    let edges = shape_edges(file).map_err(|e| format!("{name}: {e}"))?;
                    let mut mids: Vec<[f64; 3]> = Vec::new();
                    let mut add = |m: [f64; 3]| {
                        if !mids.iter().any(|p| p.iter().zip(m).all(|(a, b)| (a - b).abs() < 1e-9)) {
                            mids.push(m);
                        }
                    };
                    if o.bool("UseAllEdges") {
                        edges.iter().for_each(|(m, _)| add(*m));
                    }
                    for sub in &subs {
                        let n = |p: &str| sub.strip_prefix(p).and_then(|n| n.parse::<usize>().ok()).filter(|n| *n > 0);
                        if let Some(i) = n("Edge") {
                            add(edges.get(i - 1).ok_or(format!("{name}: {sub} is not on its base"))?.0);
                        } else if let Some(i) = n("Face") {
                            let f = u32::try_from(i - 1).map_err(|_| format!("{name}: a bad face"))?;
                            edges.iter().filter(|(_, fs)| fs.contains(&f)).for_each(|(m, _)| add(*m));
                        } else if !sub.is_empty() {
                            return Err(format!("{name}: a pick of {sub} is not translated yet"));
                        }
                    }
                    if mids.is_empty() || mids.len() > MAX_EDGE_PICKS {
                        return Err(format!("{name}: {} edges", mids.len()));
                    }
                    let picks: Vec<Value> = mids.iter().map(|m| json!({"midpoint": m})).collect();
                    let f = if fillet {
                        let r = o.float("Radius").filter(|r| *r > 0.0).ok_or(format!("{name}: no radius"))?;
                        json!({"type": "fillet", "name": name, "edges": picks, "radius": r, "target": target})
                    } else {
                        if o.int("ChamferType").unwrap_or(0) != 0 {
                            return Err(format!("{name}: an unequal chamfer is not translated yet"));
                        }
                        let d = o.float("Size").filter(|r| *r > 0.0).ok_or(format!("{name}: no size"))?;
                        json!({"type": "chamfer", "name": name, "edges": picks, "distance": d, "target": target})
                    };
                    features.push(f);
                }
                "PartDesign::AdditivePipe" | "PartDesign::SubtractivePipe" => {
                    let sub = kind == "PartDesign::SubtractivePipe";
                    let (profile, _) = o.link("Profile").ok_or(format!("{name}: no profile"))?;
                    if !sketches.contains_key(&profile) {
                        return Err(format!("{name}: its profile is not a sketch of the body"));
                    }
                    let (spine, edges) = o.link("Spine").ok_or(format!("{name}: no path"))?;
                    let (_, refs) =
                        sketches.get(&spine).ok_or(format!("{name}: its path is not a sketch of the body"))?;
                    if !o.link_list("Sections").is_empty() || o.link("AuxillerySpine").is_some() {
                        return Err(format!(
                            "{name}: a pipe through sections or with a second path is not translated yet"
                        ));
                    }
                    if o.int("Transition").unwrap_or(0) != 0 || o.int("Transformation").unwrap_or(0) != 0 {
                        return Err(format!("{name}: this pipe transition is not translated yet"));
                    }
                    // Mode 1 keeps the profile's orientation; the others follow the path.
                    let orientation = match o.int("Mode").unwrap_or(0) {
                        1 => "fixed",
                        0 | 2 => "frenet",
                        _ => return Err(format!("{name}: this pipe mode is not translated yet")),
                    };
                    let mut curves = Vec::new();
                    for e in edges.iter().filter(|e| !e.is_empty()) {
                        curves.push(
                            refs.get(e).ok_or(format!("{name}: its path edge {e} is not a sketch curve"))?.clone(),
                        );
                    }
                    let op = match (&first_solid, sub) {
                        (None, false) => "new",
                        (None, true) => return Err(format!("{name}: a subtractive pipe before any solid")),
                        (Some(_), false) => "join",
                        (Some(_), true) => "cut",
                    };
                    let mut f = json!({"type": "sweep", "name": name, "sketch": profile, "path": spine,
                        "path_curves": curves, "orientation": orientation, "operation": op});
                    if let (Some(t), Some(m)) = (&first_solid, f.as_object_mut()) {
                        m.insert("target".into(), json!(t));
                    }
                    first_solid.get_or_insert_with(|| name.clone());
                    features.push(f);
                }
                "PartDesign::AdditiveLoft" | "PartDesign::SubtractiveLoft" => {
                    let sub = kind == "PartDesign::SubtractiveLoft";
                    let (profile, _) = o.link("Profile").ok_or(format!("{name}: no profile"))?;
                    let mut list = vec![profile];
                    for sec in o.link_list("Sections") {
                        // Newer files list the profile again as the first section.
                        if list.last() != Some(&sec) {
                            list.push(sec);
                        }
                    }
                    if list.iter().any(|sk| !sketches.contains_key(sk)) {
                        return Err(format!("{name}: a loft section is not a sketch of the body"));
                    }
                    if o.bool("Closed") {
                        return Err(format!("{name}: a closed loft is not translated yet"));
                    }
                    let op = match (&first_solid, sub) {
                        (None, false) => "new",
                        (None, true) => return Err(format!("{name}: a subtractive loft before any solid")),
                        (Some(_), false) => "join",
                        (Some(_), true) => "cut",
                    };
                    let mut f = json!({"type": "loft", "name": name, "sketches": list, "ruled": o.bool("Ruled"), "operation": op});
                    if let (Some(t), Some(m)) = (&first_solid, f.as_object_mut()) {
                        m.insert("target".into(), json!(t));
                    }
                    first_solid.get_or_insert_with(|| name.clone());
                    features.push(f);
                }
                // Datums and the origin carry no geometry of the body.
                "App::Origin" | "App::Line" | "App::Plane" | "PartDesign::Plane" | "PartDesign::Line"
                | "PartDesign::Point" => {}
                other => return Err(format!("{name}: {other} is not translated yet")),
            }
        }
    }
    if bodies == 0 || features.is_empty() {
        return Err("no PartDesign body".into());
    }
    let v = json!({
        "format": "gusset-feature-tree",
        "version": 2,
        "source": {"app": "freecad", "exporter": "gusset fcstd_tree"},
        "features": features,
    });
    serde_json::from_value(v).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_shapes(_: &str) -> Result<Edges, String> {
        Err("no shapes".into())
    }

    const DOC: &str = r#"<?xml version='1.0' encoding='utf-8'?>
<Document SchemaVersion="4">
<Objects Count="4">
  <Object type="PartDesign::Body" name="Body"/>
  <Object type="Sketcher::SketchObject" name="Sketch"/>
  <Object type="PartDesign::Pad" name="Pad"/>
  <Object type="PartDesign::Pocket" name="Pocket"/>
</Objects>
<ObjectData Count="4">
  <Object name="Body"><Properties>
    <Property name="Group" type="App::PropertyLinkList"><LinkList count="3"><Link value="Sketch"/><Link value="Pad"/><Link value="Pocket"/></LinkList></Property>
  </Properties></Object>
  <Object name="Sketch"><Properties>
    <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="5">
      <Geometry type="Part::GeomLineSegment"><Construction value="0"/><LineSegment StartX="0" StartY="0" StartZ="0" EndX="10" EndY="0" EndZ="0"/></Geometry>
      <Geometry type="Part::GeomLineSegment"><Construction value="0"/><LineSegment StartX="10" StartY="0" StartZ="0" EndX="10" EndY="5" EndZ="0"/></Geometry>
      <Geometry type="Part::GeomLineSegment"><Construction value="0"/><LineSegment StartX="10" StartY="5" StartZ="0" EndX="0" EndY="5" EndZ="0"/></Geometry>
      <Geometry type="Part::GeomLineSegment"><Construction value="0"/><LineSegment StartX="0" StartY="5" StartZ="0" EndX="0" EndY="0" EndZ="0"/></Geometry>
      <Geometry type="Part::GeomCircle"><Construction value="0"/><Circle CenterX="5" CenterY="2.5" CenterZ="0" NormalX="0" NormalY="0" NormalZ="1" Radius="1"/></Geometry>
    </GeometryList></Property>
    <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  </Properties></Object>
  <Object name="Pad"><Properties>
    <Property name="Length" type="App::PropertyLength"><Float value="2"/></Property>
    <Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="0"></LinkSub></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  </Properties></Object>
  <Object name="Pocket"><Properties>
    <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
    <Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="0"></LinkSub></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  </Properties></Object>
</ObjectData>
</Document>"#;

    #[test]
    fn a_pad_and_a_pocket_become_tree_features() {
        let t = tree(DOC.as_bytes(), &mut no_shapes).unwrap();
        assert_eq!(t.features.len(), 3);
        assert_eq!(t.features[0]["type"], "sketch");
        assert_eq!(t.features[0]["points"].as_array().unwrap().len(), 5, "shared corners plus the circle centre");
        assert_eq!(t.features[1]["operation"], "new");
        assert_eq!(t.features[1]["distance"], 2.0);
        assert_eq!(t.features[2]["operation"], "cut");
        assert!(t.features[2]["distance"].as_f64().unwrap() < -1000.0, "through all, against the normal");
    }

    #[test]
    fn placements_turn_the_sketch_frame() {
        // Half a turn about X: the sketch's y axis points down.
        let doc = DOC.replace(r#"Q0="0" Q1="0" Q2="0" Q3="1""#, r#"Q0="1" Q1="0" Q2="0" Q3="0""#);
        let t = tree(doc.as_bytes(), &mut no_shapes).unwrap();
        let y = &t.features[0]["plane"]["y_axis"];
        assert!((y[1].as_f64().unwrap() + 1.0).abs() < 1e-12, "{y}");
    }

    #[test]
    fn what_is_not_translated_refuses_the_whole_tree() {
        let doc = DOC.replace("PartDesign::Pocket", "PartDesign::AdditiveLoft");
        assert!(tree(doc.as_bytes(), &mut no_shapes).unwrap_err().contains("not translated"));
        assert!(tree(b"<Document/>", &mut no_shapes).is_err());
        assert!(tree(b"\xff\xfe garbage", &mut no_shapes).is_err());
    }

    #[test]
    fn the_body_ends_at_its_tip() {
        let doc = DOC.replace(
            r#"<Link value="Pocket"/></LinkList></Property>"#,
            r#"<Link value="Pocket"/></LinkList></Property>
    <Property name="Tip" type="App::PropertyLink"><Link value="Pad"/></Property>"#,
        );
        let t = tree(doc.as_bytes(), &mut no_shapes).unwrap();
        assert_eq!(t.features.len(), 2, "the pocket after the tip is left out");
    }

    #[test]
    fn solids_outside_bodies_keep_the_stored_shapes() {
        let doc = DOC
            .replace(
                r#"<Objects Count="4">"#,
                r#"<Objects Count="5">
  <Object type="Part::Cut" name="Cut"/>"#,
            )
            .replace(
                r#"<ObjectData Count="4">"#,
                r#"<ObjectData Count="5">
  <Object name="Cut"><Properties>
    <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape9.brp"/></Property>
  </Properties></Object>"#,
            );
        assert!(tree(doc.as_bytes(), &mut no_shapes).unwrap_err().contains("Part::Cut outside a PartDesign body"));
    }

    #[test]
    fn sketch_edges_are_named_without_construction_geometry() {
        // The first line is construction: FreeCAD's Edge1 is the second line, Axis0 the first.
        let doc = DOC.replacen(r#"<Construction value="0"/>"#, r#"<Construction value="1"/>"#, 1);
        let o = parse(doc.as_bytes()).unwrap();
        let (_, names) = sketch("Sketch", o.get("Sketch").unwrap(), &IDENTITY).unwrap();
        assert_eq!(names.get("Axis0").map(String::as_str), Some("l1"));
        assert_eq!(names.get("Edge1").map(String::as_str), Some("l2"));
        assert_eq!(names.get("Edge4").map(String::as_str), Some("c5"));
    }
}
