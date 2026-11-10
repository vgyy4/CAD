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
    /// Position in the document's object list.
    order: usize,
    props: HashMap<String, Vec<Elem>>,
}

/// Marks the body-combining features among `made` as unrefined when the FreeCAD object they
/// came from has `Refine = false` (ADR 0033, 0042). A feature that starts a body does no
/// boolean, so there is nothing to keep.
fn mark_unrefined(made: &mut [Value], unrefined: bool) {
    if !unrefined {
        return;
    }
    // A two-length pad, pocket, groove or revolution is two features here (`_back` is the second
    // side) where FreeCAD makes one solid: unrefined, their seam would be an edge FreeCAD's
    // shape does not have (and its dressups pick), so such an object stays refined.
    let two_sided = |f: &Value| f.get("name").and_then(Value::as_str).is_some_and(|n| n.ends_with("_back"));
    if made.iter().any(two_sided) {
        return;
    }
    for f in made {
        let combines = matches!(
            f.get("type").and_then(Value::as_str),
            Some(
                "extrude" | "revolve" | "sweep" | "loft" | "combine" | "pattern_linear" | "pattern_circular" | "mirror"
            )
        );
        if combines
            && f.get("operation").and_then(Value::as_str) != Some("new")
            && let Some(m) = f.as_object_mut()
        {
            m.insert("refine".into(), json!(false));
        }
    }
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
    /// FreeCAD's `Refine = false`: the feature keeps the split faces of its boolean. A missing
    /// property is refined (FreeCAD's default for the files that do not store it).
    fn unrefined(&self) -> bool {
        self.value("Refine", "Bool") == Some("false")
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
                            let order = objects.len();
                            let o = objects.entry(name).or_default();
                            o.kind = a.get("type").cloned().unwrap_or_default();
                            o.order = order;
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
                if s == t {
                    // A whole turn stored as an arc is a circle.
                    curves.push(json!({"id": format!("c{edge}"), "type": "circle", "center": c, "radius": r, "construction": construction}));
                    construction = false;
                    continue;
                }
                curves.push(json!({"id": format!("a{edge}"), "type": "arc", "center": c, "start": s, "end": t, "construction": construction}));
                construction = false;
            }
            "Point" => {
                edge += 1;
                construction = false;
            }
            "Ellipse" | "ArcOfEllipse" => {
                edge += 1;
                // The same refusal as for arcs: a mirrored sketch plane flips the normal.
                if e.attrs.get("NormalZ").and_then(|v| v.parse::<f64>().ok()).is_some_and(|z| z < 0.0) {
                    return Err(format!("{name}: a clockwise ellipse"));
                }
                let (cx, cy) = (num(e, "CenterX")?, num(e, "CenterY")?);
                let (a, b) = (num(e, "MajorRadius")?, num(e, "MinorRadius")?);
                let ang = e.attrs.get("AngleXU").and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite());
                let ang = ang.unwrap_or(0.0);
                if a <= 0.0 || b <= 0.0 {
                    return Err(format!("{name}: an ellipse without area"));
                }
                let (u, v) = ([libm::cos(ang), libm::sin(ang)], [-libm::sin(ang), libm::cos(ang)]);
                let at = |t: f64| {
                    (
                        cx + a * libm::cos(t) * u[0] + b * libm::sin(t) * v[0],
                        cy + a * libm::cos(t) * u[1] + b * libm::sin(t) * v[1],
                    )
                };
                let c = point(cx, cy, &mut pjson);
                let full = if e.tag == "Ellipse" {
                    true
                } else {
                    // The parameter is the eccentric anomaly, counter-clockwise about the normal.
                    let (t0, t1) = (num(e, "StartAngle")?, num(e, "EndAngle")?);
                    if (t1 - t0).abs() >= std::f64::consts::TAU - 1e-9 {
                        true
                    } else {
                        let (p0, p1) = (at(t0), at(t1));
                        let s = point(p0.0, p0.1, &mut pjson);
                        let t = point(p1.0, p1.1, &mut pjson);
                        if s == t {
                            return Err(format!("{name}: a degenerate elliptical arc"));
                        }
                        curves.push(json!({"id": format!("q{edge}"), "type": "ellipse_arc", "center": c, "major": u,
                            "a": a, "b": b, "start": s, "end": t, "construction": construction}));
                        false
                    }
                };
                if full {
                    curves.push(json!({"id": format!("o{edge}"), "type": "ellipse", "center": c, "major": u,
                        "a": a, "b": b, "construction": construction}));
                }
                construction = false;
            }
            t if matches!(t, "ArcOfHyperbola" | "ArcOfParabola" | "BSplineCurve" | "Hyperbola" | "Parabola") => {
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

/// Part workbench objects translated (primitives, booleans, extrusions of sketches).
const PART_KINDS: [&str; 13] = [
    "Part::Box",
    "Part::Cylinder",
    "Part::Sphere",
    "Part::Cone",
    "Part::Cut",
    "Part::Fuse",
    "Part::MultiFuse",
    "Part::Common",
    "Part::MultiCommon",
    "Part::Extrusion",
    "Part::Thickness",
    "Part::Helix",
    "Part::Sweep",
];

/// The `Part::Helix` an object stands for: itself, or the one a PartDesign base feature wraps.
fn helix_object<'a>(objects: &'a BTreeMap<String, Obj>, name: &str) -> Option<(&'a String, &'a Obj)> {
    let mut cur = objects.get_key_value(name)?;
    for _ in 0..8 {
        match cur.1.kind.as_str() {
            "Part::Helix" => return Some(cur),
            "PartDesign::FeatureBase" => cur = objects.get_key_value(&cur.1.link("BaseFeature")?.0)?,
            _ => return None,
        }
    }
    None
}

/// A `Part::Helix` used as a pipe's or sweep's spine, as the tree's `helix` (ADR 0042 amendment
/// 2026-11-10); `None` when the spine is not a helix. The spine must be the whole helix (every
/// edge). A stored `Angle` that leaves the helix on its cylinder (the stored edges all lie at the
/// radius) is ignored as FreeCAD did; one that does not is passed on, and the tree refuses it.
fn helix_spine(
    name: &str,
    objects: &BTreeMap<String, Obj>,
    spine: &str,
    subs: &[String],
    shape_edges: ShapeEdges<'_>,
) -> Result<Option<Value>, String> {
    let Some((hname, h)) = helix_object(objects, spine) else { return Ok(None) };
    let (Some(radius), Some(pitch), Some(height)) = (h.float("Radius"), h.float("Pitch"), h.float("Height")) else {
        return Err(format!("{name}: its helix {hname} has no radius, pitch or height"));
    };
    // The spine object's own stored shape and placement: a PartDesign base feature wraps the
    // helix's shape with its own placement, not the helix's.
    let holder = objects.get(spine).unwrap_or(h);
    let place = Place::of(holder);
    let file = holder
        .first("Shape", "Part")
        .and_then(|e| e.attrs.get("file"))
        .filter(|f| !f.is_empty())
        .ok_or(format!("{name}: its helix {hname} has no stored shape"))?;
    let shape = shape_edges(file).map_err(|e| format!("{name}: {e}"))?;
    let mut picked: Vec<usize> = Vec::new();
    for s in subs.iter().filter(|s| !s.is_empty()) {
        picked.push(
            s.strip_prefix("Edge")
                .and_then(|n| n.parse::<usize>().ok())
                .ok_or(format!("{name}: its path {s} is not an edge of the helix"))?,
        );
    }
    picked.sort_unstable();
    picked.dedup();
    if !picked.is_empty() && !picked.iter().copied().eq(1..=shape.edges.len()) {
        return Err(format!("{name}: a path of some edges of a helix is not translated yet"));
    }
    // The stored edges lie on the cylinder about the placed axis (a bare wire has no tessellated
    // path: its edges' midpoints stand in).
    let mut off = 0.0f64;
    for q in shape.edges.iter().flat_map(|e| e.path.iter().chain(std::iter::once(&e.mid))) {
        let r = [q[0] - place.p[0], q[1] - place.p[1], q[2] - place.p[2]];
        let a = r[0] * place.z[0] + r[1] * place.z[1] + r[2] * place.z[2];
        let c = [r[0] - a * place.z[0], r[1] - a * place.z[1], r[2] - a * place.z[2]];
        off = off.max((libm::sqrt(c[0] * c[0] + c[1] * c[1] + c[2] * c[2]) - radius).abs());
    }
    let tapered = h.float("Angle").is_some_and(|a| a.abs() > 1e-9);
    // FreeCAD keeps a stored `Angle` the helix ignored (a cylinder): only a shape off the
    // cylinder is tapered.
    if shape.edges.is_empty() || (!tapered && off > 2e-3) {
        return Err(format!("{name}: the stored shape of its helix {hname} does not match its parameters"));
    }
    let mut j = json!({"origin": place.p, "axis": place.z, "x": place.x, "radius": radius, "pitch": pitch,
        "height": height, "left_handed": h.int("LocalCoord") == Some(1)});
    if tapered
        && off > 2e-3
        && let (Some(m), Some(a)) = (j.as_object_mut(), h.float("Angle"))
    {
        m.insert("taper_angle".into(), json!(a));
    }
    Ok(Some(j))
}

/// Whether a placement is the identity.
fn is_identity(p: &Place) -> bool {
    let near = |a: V3, b: V3| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-9);
    near(p.p, [0.0; 3]) && near(p.x, IDENTITY.x) && near(p.y, IDENTITY.y) && near(p.z, IDENTITY.z)
}

/// A Draft array (`Part::FeaturePython` whose proxy is Draft's `_Array`).
fn is_draft_array(o: &Obj) -> bool {
    o.kind == "Part::FeaturePython"
        && o.first("Proxy", "Python").is_some_and(|e| e.attrs.get("class").is_some_and(|c| c == "_Array"))
}

/// A `PropertyVector` value.
fn vector(o: &Obj, prop: &str) -> Option<V3> {
    let e = o.first(prop, "PropertyVector")?;
    let g = |k: &str| e.attrs.get(k).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite());
    Some([g("valueX")?, g("valueY")?, g("valueZ")?])
}

/// A Draft array of a translated body as body patterns (polar: one circular pattern; ortho:
/// up to three linear ones).
fn draft_array(name: &str, o: &Obj, target: &str) -> Result<Vec<Value>, String> {
    let pl = Place::of(o);
    if pl.p.iter().any(|c| c.abs() > 1e-9) || (pl.x[0] - 1.0).abs() > 1e-12 || (pl.y[1] - 1.0).abs() > 1e-12 {
        return Err(format!("{name}: a placed Draft array is not translated yet"));
    }
    let polar =
        o.elems("ArrayType").iter().any(|e| e.tag == "Integer" && e.attrs.get("value").is_some_and(|v| v == "1"));
    let mut out = Vec::new();
    if polar {
        let count = o.int("NumberPolar").unwrap_or(1);
        if vector(o, "IntervalAxis").is_some_and(|v| v.iter().any(|c| c.abs() > 1e-12)) {
            return Err(format!("{name}: a polar array with an axial step is not translated yet"));
        }
        if count >= 2 {
            let axis = vector(o, "Axis").ok_or(format!("{name}: no axis"))?;
            let center = vector(o, "Center").unwrap_or([0.0; 3]);
            out.push(json!({"type": "pattern_circular", "name": name, "features": [], "target": target,
                "axis": {"origin": center, "direction": axis}, "angle": o.float("Angle").unwrap_or(360.0), "count": count}));
        }
    } else {
        for (i, (n, v)) in
            [("NumberX", "IntervalX"), ("NumberY", "IntervalY"), ("NumberZ", "IntervalZ")].iter().enumerate()
        {
            let count = o.int(n).unwrap_or(1);
            if count < 2 {
                continue;
            }
            let step = vector(o, v).ok_or(format!("{name}: no {v}"))?;
            let len = libm::sqrt(step[0] * step[0] + step[1] * step[1] + step[2] * step[2]);
            if len <= 1e-12 {
                return Err(format!("{name}: a zero array step"));
            }
            let suffix = ["x", "y", "z"].get(i).copied().unwrap_or("d");
            out.push(json!({"type": "pattern_linear", "name": format!("{name}_{suffix}"),
                "features": [], "target": target, "direction": step, "spacing": len, "count": count}));
        }
    }
    Ok(out)
}

/// The straight edges of stored shapes that an axis or direction names (`Feature/EdgeN`): a point
/// on the edge and its direction (as the stored edge runs). Others are left out.
fn edge_axes(objects: &BTreeMap<String, Obj>, shape_edges: ShapeEdges<'_>) -> HashMap<String, (V3, V3)> {
    let mut out = HashMap::new();
    for o in objects.values() {
        for prop in ["Direction", "Axis", "ReferenceAxis"] {
            let Some((obj, subs)) = o.link(prop) else { continue };
            let Some(file) = objects.get(&obj).filter(|t| t.kind.starts_with("PartDesign::")).and_then(|t| {
                t.first("Shape", "Part").and_then(|e| e.attrs.get("file")).filter(|f| !f.is_empty()).cloned()
            }) else {
                continue;
            };
            for sub in subs.iter().filter(|s| s.starts_with("Edge")) {
                let key = format!("{obj}/{sub}");
                if out.contains_key(&key) {
                    continue;
                }
                let Some(n) = sub.strip_prefix("Edge").and_then(|n| n.parse::<usize>().ok()).filter(|n| *n > 0) else {
                    continue;
                };
                let Ok(edges) = shape_edges(&file) else { continue };
                let Some(e) = edges.get(n - 1) else { continue };
                let (Some(a), Some(b)) = (e.path.first().copied(), e.path.last().copied()) else { continue };
                let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let len = libm::sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
                if len < 1e-9 {
                    continue;
                }
                let u = d.map(|c| c / len);
                // Straight: every point of the path lies on the line through the ends (the path
                // points are single precision).
                let straight = e.path.iter().chain(std::iter::once(&e.mid)).all(|p| {
                    let r = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
                    let t = r[0] * u[0] + r[1] * u[1] + r[2] * u[2];
                    let q = [r[0] - t * u[0], r[1] - t * u[1], r[2] - t * u[2]];
                    libm::sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2]) < 1e-3
                });
                if straight {
                    out.insert(key, (a, u));
                }
            }
        }
    }
    out
}

/// A PartDesign body's features up to an object, with the name of its first solid then.
type Snapshots = HashMap<String, (Vec<Value>, Option<String>)>;

/// The Part workbench objects, in document order, as separate bodies and combines.
fn part_features(
    objects: &BTreeMap<String, Obj>,
    features: &mut Vec<Value>,
    body_of: &mut HashMap<String, String>,
    snaps: Option<&Snapshots>,
    shape_edges: ShapeEdges<'_>,
) -> Result<(), String> {
    // The body a Part object works on: the PartDesign feature's own body as built, or (with
    // snapshots) a renamed copy of the features up to it.
    let mut copies = 0usize;
    let mut input = |features: &mut Vec<Value>, body_of: &HashMap<String, String>, who: &str, i: &str| {
        if let Some((slice, Some(first))) = snaps.and_then(|m| m.get(i)) {
            copies += 1;
            let suffix = format!("_c{copies}");
            let names: std::collections::HashSet<&str> =
                slice.iter().filter_map(|f| f.get("name").and_then(Value::as_str)).collect();
            let rename = |v: &Value| match v {
                Value::String(t) if names.contains(t.as_str()) => json!(format!("{t}{suffix}")),
                Value::Array(a) => Value::Array(
                    a.iter()
                        .map(|x| match x {
                            Value::String(t) if names.contains(t.as_str()) => json!(format!("{t}{suffix}")),
                            other => other.clone(),
                        })
                        .collect(),
                ),
                other => other.clone(),
            };
            for f in slice {
                let mut f = f.clone();
                if let Some(m) = f.as_object_mut() {
                    for k in ["name", "sketch", "target", "features", "tools", "sketches", "path", "profile"] {
                        if let Some(v) = m.get(k).map(&rename) {
                            m.insert(k.into(), v);
                        }
                    }
                }
                features.push(f);
            }
            return Ok(format!("{first}{suffix}"));
        }
        body_of.get(i).cloned().ok_or(format!("{who}: its input {i} is not translated"))
    };
    let mut parts: Vec<(&String, &Obj)> =
        objects.iter().filter(|(_, o)| PART_KINDS.contains(&o.kind.as_str()) || is_draft_array(o)).collect();
    parts.sort_by_key(|(_, o)| o.order);
    let mut sketched: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (name, o) in parts {
        let kind = o.kind.as_str();
        match kind {
            "Part::Box" | "Part::Cylinder" | "Part::Sphere" | "Part::Cone" => {
                for mut f in primitive(name, o, &IDENTITY)? {
                    if f.get("type").and_then(Value::as_str) != Some("sketch")
                        && let Some(m) = f.as_object_mut()
                    {
                        m.insert("operation".into(), json!("new"));
                    }
                    features.push(f);
                }
                body_of.insert(name.clone(), name.clone());
            }
            "Part::Cut" | "Part::Fuse" | "Part::MultiFuse" | "Part::Common" | "Part::MultiCommon" => {
                let inputs: Vec<String> = if kind.starts_with("Part::Multi") {
                    o.link_list("Shapes")
                } else {
                    [o.link("Base"), o.link("Tool")].into_iter().flatten().map(|(n, _)| n).collect()
                };
                let mut bodies: Vec<String> = Vec::new();
                for i in &inputs {
                    bodies.push(input(features, body_of, name, i)?);
                }
                let [target, tools @ ..] = bodies.as_slice() else { return Err(format!("{name}: no inputs")) };
                if tools.is_empty() {
                    return Err(format!("{name}: a boolean of one shape"));
                }
                let op = match kind {
                    "Part::Cut" => "cut",
                    "Part::Fuse" | "Part::MultiFuse" => "join",
                    _ => "intersect",
                };
                let mut combine =
                    json!({"type": "combine", "name": name, "target": target, "tools": tools, "operation": op});
                if o.unrefined()
                    && let Some(m) = combine.as_object_mut()
                {
                    m.insert("refine".into(), json!(false));
                }
                features.push(combine);
                body_of.insert(name.clone(), target.clone());
            }
            "Part::Extrusion" => {
                let (base, _) = o.link("Base").ok_or(format!("{name}: nothing to extrude"))?;
                let sk = objects.get(&base).filter(|b| b.kind == "Sketcher::SketchObject").ok_or(format!(
                    "{name}: an extrusion of a {} is not translated yet",
                    objects.get(&base).map_or("missing object", |b| b.kind.as_str())
                ))?;
                if !o.bool("Solid")
                    || o.float("TaperAngle").is_some_and(|a| a.abs() > 1e-12)
                    || o.float("TaperAngleRev").is_some_and(|a| a.abs() > 1e-12)
                {
                    return Err(format!("{name}: an open or tapered extrusion is not translated yet"));
                }
                let frame = Place::of(sk);
                if sketched.insert(base.clone()) {
                    let (f, _) = sketch(&base, sk, &IDENTITY)?;
                    features.push(f);
                }
                // The direction: the sketch normal (mode 2) or a custom vector along it.
                let dir = match o.int("DirMode").unwrap_or(0) {
                    2 => frame.z,
                    0 => {
                        let e = o.first("Dir", "PropertyVector").ok_or(format!("{name}: no direction"))?;
                        let g = |k: &str| e.attrs.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                        [g("valueX"), g("valueY"), g("valueZ")]
                    }
                    _ => return Err(format!("{name}: an extrusion along an edge is not translated yet")),
                };
                let dn = libm::sqrt(dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]);
                let along = (dir[0] * frame.z[0] + dir[1] * frame.z[1] + dir[2] * frame.z[2]) / dn.max(1e-300);
                if dn.is_nan() || dn <= 1e-12 || (along.abs() - 1.0).abs() > 1e-9 {
                    return Err(format!("{name}: an extrusion not along its sketch's normal is not translated yet"));
                }
                let mut fwd = o.float("LengthFwd").unwrap_or(0.0);
                if fwd.abs() < 1e-12 {
                    fwd = dn;
                }
                let rev = o.float("LengthRev").unwrap_or(0.0);
                let mut sign = along.signum();
                if o.bool("Reversed") {
                    sign = -sign;
                }
                if o.bool("Symmetric") {
                    features.push(json!({"type": "extrude", "name": name, "sketch": base, "profiles": "all", "distance": fwd, "symmetric": true, "operation": "new"}));
                } else {
                    features.push(json!({"type": "extrude", "name": name, "sketch": base, "profiles": "all", "distance": sign * fwd, "symmetric": false, "operation": "new"}));
                    if rev > 1e-12 {
                        features.push(json!({"type": "extrude", "name": format!("{name}_back"), "sketch": base, "profiles": "all", "distance": -sign * rev, "symmetric": false, "operation": "join", "target": name}));
                    }
                }
                body_of.insert(name.clone(), name.clone());
            }
            "Part::Sweep" => {
                let sections = o.link_list("Sections");
                let [section] = sections.as_slice() else {
                    return Err(format!("{name}: a sweep through several sections is not translated yet"));
                };
                let sk = objects.get(section).filter(|b| b.kind == "Sketcher::SketchObject").ok_or(format!(
                    "{name}: a sweep of a {} is not translated yet",
                    objects.get(section).map_or("missing object", |b| b.kind.as_str())
                ))?;
                let (spine, subs) = o.link("Spine").ok_or(format!("{name}: no path"))?;
                let helix = helix_spine(name, objects, &spine, &subs, shape_edges)?
                    .ok_or(format!("{name}: a sweep along a path other than a helix is not translated yet"))?;
                if sketched.insert(section.clone()) {
                    let (f, _) = sketch(section, sk, &IDENTITY)?;
                    features.push(f);
                }
                features.push(json!({"type": "sweep", "name": name, "sketch": section, "helix": helix,
                    "orientation": "frenet", "operation": "new"}));
                body_of.insert(name.clone(), name.clone());
            }
            "Part::Thickness" => {
                let (src, subs) = o.link("Faces").ok_or(format!("{name}: no faces"))?;
                let file = objects
                    .get(&src)
                    .and_then(|b| b.first("Shape", "Part"))
                    .and_then(|e| e.attrs.get("file"))
                    .filter(|f| !f.is_empty())
                    .ok_or(format!("{name}: its input shape is not stored in the file"))?;
                let shape = shape_edges(file).map_err(|e| format!("{name}: {e}"))?;
                if o.int("Mode").unwrap_or(0) != 0 || o.bool("Intersection") || o.bool("SelfIntersection") {
                    return Err(format!("{name}: only a thickness of the skin mode is translated"));
                }
                let mut faces = Vec::new();
                for sub in &subs {
                    let n = sub.strip_prefix("Face").and_then(|n| n.parse::<usize>().ok()).filter(|n| *n > 0);
                    let f =
                        n.and_then(|n| shape.faces.get(n - 1)).ok_or(format!("{name}: {sub} is not on its input"))?;
                    faces.push(json!({"point": f.point, "normal": f.normal}));
                }
                let v = o.float("Value").filter(|v| v.abs() > 1e-12).ok_or(format!("{name}: no thickness"))?;
                // FreeCAD: a negative value grows the wall inward; Gusset's default side is inside.
                let side = if v < 0.0 { "inside" } else { "outside" };
                let target = input(features, body_of, name, &src)?;
                features
                    .push(json!({"type": "shell", "name": name, "faces": faces, "thickness": v.abs(), "side": side}));
                body_of.insert(name.clone(), target);
            }
            "Part::FeaturePython" => {
                let (base, _) = o.link("Base").ok_or(format!("{name}: an array of nothing"))?;
                let target = input(features, body_of, name, &base)?;
                features.extend(draft_array(name, o, &target)?);
                body_of.insert(name.clone(), target);
            }
            _ => {}
        }
    }
    Ok(())
}

/// The face a pad or pocket up to a face ends on, as a pick of the model (`face`) and whether it
/// sweeps against its sketch's normal.
struct UpTo {
    point: V3,
    normal: V3,
    reverse: bool,
}

/// The `UpToFace` of a pad or pocket as a pick, when it is a plane ahead of the sweep with no offset:
/// the face of the named feature's stored shape (`FaceN`) as it stood there. Anything else (an
/// offset, a curved or missing face, a face behind the sketch, an extrusion that reaches both
/// sides of its sketch) is left to the distance read off the stored shape.
fn up_to_face(
    o: &Obj,
    pocket: bool,
    sk: &Place,
    objects: &BTreeMap<String, Obj>,
    shape_edges: ShapeEdges<'_>,
) -> Option<UpTo> {
    if o.float("Offset").is_some_and(|v| v.abs() > 1e-9) {
        return None;
    }
    let (obj, subs) = o.link("UpToFace")?;
    let n = subs.first()?.strip_prefix("Face")?.parse::<usize>().ok().filter(|n| *n > 0)?;
    let file = objects.get(&obj)?.first("Shape", "Part")?.attrs.get("file").filter(|f| !f.is_empty())?.clone();
    let shape = shape_edges(&file).ok()?;
    let face = shape.faces.get(n - 1).filter(|f| f.planar)?;
    let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let den = dot(sk.z, face.normal);
    if den.abs() < 1e-9 {
        return None;
    }
    // A pad grows along the sketch's normal and a pocket cuts against it; `Reversed` flips either.
    let reverse = pocket != o.bool("Reversed");
    let dir = if reverse { -1.0 } else { 1.0 };
    let rel = [face.point[0] - sk.p[0], face.point[1] - sk.p[1], face.point[2] - sk.p[2]];
    // Where the face's plane cuts the sketch's normal axis: ahead of the sweep.
    let ahead = dot(rel, face.normal) / den * dir;
    (ahead > 1e-6).then_some(UpTo { point: face.point, normal: face.normal, reverse })
}

/// What a reference can name: the document's objects, the body's placement and its sketches
/// read so far (frame and lines).
struct Refs<'a> {
    objects: &'a BTreeMap<String, Obj>,
    body: &'a Place,
    sketches: &'a HashMap<String, (Place, HashMap<String, String>)>,
    /// Straight edges of features' stored shapes, by `Feature/EdgeN`: a point and a direction.
    edge_axes: &'a HashMap<String, (V3, V3)>,
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
        if let Some(a) = self.edge_axes.get(&format!("{obj}/{sub}")) {
            return Ok(*a);
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
    let kind = o
        .kind
        .trim_start_matches("PartDesign::")
        .trim_start_matches("Part::")
        .trim_start_matches("Additive")
        .trim_start_matches("Subtractive");
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
        "Ellipsoid" => {
            // A spheroid: the half ellipse in the x-z plane (polar radius along z), turned about z.
            let (r1, r2) = (pos("Radius1")?, pos("Radius2")?);
            let r3 = o.float("Radius3").unwrap_or(0.0);
            if r3 != 0.0 && (r3 - r2).abs() > 1e-9 {
                return Err(format!("{name}: a three-axis ellipsoid is not translated yet"));
            }
            if !full("Angle3")
                || o.float("Angle1").is_some_and(|a| (a + 90.0).abs() > 1e-9)
                || o.float("Angle2").is_some_and(|a| (a - 90.0).abs() > 1e-9)
            {
                return Err(format!("{name}: a partial ellipsoid is not translated yet"));
            }
            let points = vec![
                json!({"id": "c", "x": 0.0, "y": 0.0}),
                json!({"id": "s", "x": 0.0, "y": -r1}),
                json!({"id": "t", "x": 0.0, "y": r1}),
            ];
            let curve = if (r1 - r2).abs() < 1e-12 {
                json!({"id": "a", "type": "arc", "center": "c", "start": "s", "end": "t"})
            } else if r1 > r2 {
                json!({"id": "a", "type": "ellipse_arc", "center": "c", "major": [0.0, 1.0], "a": r1, "b": r2, "start": "s", "end": "t"})
            } else {
                json!({"id": "a", "type": "ellipse_arc", "center": "c", "major": [1.0, 0.0], "a": r2, "b": r1, "start": "s", "end": "t"})
            };
            Ok(vec![
                json!({"type": "sketch", "name": sk, "plane": {"origin": pl.p, "x_axis": pl.x, "y_axis": pl.z}, "points": points,
                    "curves": [curve, {"id": "l", "type": "line", "start": "t", "end": "s"}]}),
                json!({"type": "revolve", "name": name, "sketch": sk, "profiles": "all", "axis": {"origin": pl.p, "direction": pl.z}, "angle": 360.0}),
            ])
        }
        _ => Err(format!("{name}: {} is not translated yet", o.kind)),
    }
}

/// Whether the document holds a PartDesign body worth translating.
#[must_use]
pub(crate) fn has_bodies(xml: &[u8]) -> bool {
    // Bodies, or PartDesign features from before FreeCAD had bodies.
    parse(xml).is_ok_and(|o| {
        o.values()
            .any(|o| o.kind.starts_with("PartDesign::") || PART_KINDS.contains(&o.kind.as_str()) || is_draft_array(o))
    })
}

/// A stored shape file: its edges in FreeCAD's numbering (`Edge1` first), each edge's midpoint and
/// the 0-based numbers of the faces it bounds, and its faces (`Face1` first). It derefs to the edges.
#[derive(Debug, Clone, Default)]
pub(crate) struct Edges {
    pub edges: Vec<SrcEdge>,
    pub faces: Vec<SrcFace>,
}

impl std::ops::Deref for Edges {
    type Target = Vec<SrcEdge>;
    fn deref(&self) -> &Self::Target {
        &self.edges
    }
}

impl IntoIterator for Edges {
    type Item = SrcEdge;
    type IntoIter = std::vec::IntoIter<SrcEdge>;
    fn into_iter(self) -> Self::IntoIter {
        self.edges.into_iter()
    }
}

impl From<Vec<SrcEdge>> for Edges {
    fn from(edges: Vec<SrcEdge>) -> Self {
        Edges { edges, faces: Vec::new() }
    }
}

/// A face of a stored FreeCAD shape: whether it is a plane, a point on it (inside the face) and
/// its outward normal.
#[derive(Debug, Clone, Default)]
pub(crate) struct SrcFace {
    pub planar: bool,
    pub point: [f64; 3],
    pub normal: [f64; 3],
}

/// An edge of a stored FreeCAD shape: its midpoint, the 0-based faces it bounds and points
/// along it (a refined shape's edge may be several of Gusset's).
#[derive(Debug, Clone, Default)]
pub(crate) struct SrcEdge {
    pub mid: [f64; 3],
    pub faces: Vec<u32>,
    pub path: Vec<[f64; 3]>,
    /// The edge joins two faces of one smooth surface (a split cylinder's seam line): no
    /// fillet or chamfer rounds it, and picking a whole face brings it along.
    pub smooth: bool,
}

/// Whether an edge between two faces lies inside one surface: coplanar planes, or cylinders
/// about one axis (their shared line then lies at one radius from it).
pub(crate) fn is_smooth_edge(
    kind: gusset_kernel::CurveKind,
    mid: [f64; 3],
    a: &gusset_kernel::FaceInfo,
    b: &gusset_kernel::FaceInfo,
) -> bool {
    use gusset_kernel::{CurveKind, SurfaceKind};
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let sub = |u: [f64; 3], v: [f64; 3]| [u[0] - v[0], u[1] - v[1], u[2] - v[2]];
    let tol = 1e-6;
    match (a.kind, b.kind, a.axis, b.axis) {
        (SurfaceKind::Plane, SurfaceKind::Plane, ..) => {
            kind == CurveKind::Line
                && dot(a.normal, b.normal) > 1.0 - tol
                && dot(sub(mid, a.centroid), a.normal).abs() < 1e-6
                && dot(sub(b.centroid, a.centroid), a.normal).abs() < 1e-6
        }
        (SurfaceKind::Cylinder, SurfaceKind::Cylinder, Some(x), Some(y)) => {
            let across = |u: [f64; 3], v: [f64; 3]| {
                [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
            };
            let n = |u: [f64; 3]| libm::sqrt(dot(u, u));
            let along = kind == CurveKind::Line && n(across(x.dir, y.dir)) < tol;
            // The same axis line (a line on a cylinder runs along it).
            let same_line = n(across(sub(y.origin, x.origin), x.dir)) < 1e-6;
            along && same_line
        }
        _ => false,
    }
}
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
    tree_with(xml, shape_edges, false)
}

/// [`tree`], optionally writing a pattern of pads and pockets the way FreeCAD computes it:
/// the whole extruded tool is copied, not only the material it added or removed from its
/// body (the two differ when a copy lands where the original did not reach the material).
pub(crate) fn tree_with(
    xml: &[u8],
    shape_edges: ShapeEdges<'_>,
    tool_patterns: bool,
) -> Result<crate::tree_import::Tree, String> {
    tree_variant(xml, shape_edges, tool_patterns, false)
}

/// [`tree_with`], optionally giving each Part object (boolean, Draft array) its own copy of the
/// PartDesign feature it takes as input, as the body stood at that feature. FreeCAD's Part
/// objects copy their inputs: the body goes on, and shows beside them.
pub(crate) fn tree_variant(
    xml: &[u8],
    shape_edges: ShapeEdges<'_>,
    tool_patterns: bool,
    copy_sources: bool,
) -> Result<crate::tree_import::Tree, String> {
    tree_options(xml, shape_edges, tool_patterns, copy_sources, true)
}

/// [`tree_variant`], with `up_to_faces` off: a pad or pocket up to a face is written as the
/// distance the stored shape shows, not as an extrusion up to that face.
pub(crate) fn tree_options(
    xml: &[u8],
    shape_edges: ShapeEdges<'_>,
    tool_patterns: bool,
    copy_sources: bool,
    up_to_faces: bool,
) -> Result<crate::tree_import::Tree, String> {
    let objects = parse(xml)?;
    // Solids made outside PartDesign bodies (Part workbench booleans, sweeps, arrays) are not
    // history this translates; the stored shapes stand in for the whole document.
    if let Some((n, o)) = objects.iter().find(|(_, o)| {
        o.props.contains_key("Shape")
            && !o.kind.starts_with("PartDesign::")
            && !o.kind.starts_with("Sketcher::")
            && !o.kind.starts_with("App::")
            && !PART_KINDS.contains(&o.kind.as_str())
            && !is_draft_array(o)
    }) {
        return Err(format!("{n}: a {} outside a PartDesign body is not translated yet", o.kind));
    }
    let mut features: Vec<Value> = Vec::new();
    let mut sketches: HashMap<String, (Place, HashMap<String, String>)> = HashMap::new();
    // The tree feature whose body each body object's result is (for Part booleans).
    let mut body_of: HashMap<String, String> = HashMap::new();
    let mut bodies_list: Vec<(String, Obj)> =
        objects.iter().filter(|(_, o)| o.kind == "PartDesign::Body").map(|(n, o)| (n.clone(), o.clone())).collect();
    // Files from before FreeCAD had bodies: the sketches and PartDesign features form one
    // implicit body, in document order.
    if bodies_list.is_empty() && objects.values().any(|o| o.kind.starts_with("PartDesign::")) {
        let mut chain: Vec<(&String, &Obj)> = objects
            .iter()
            .filter(|(_, o)| o.kind.starts_with("PartDesign::") || o.kind == "Sketcher::SketchObject")
            .collect();
        chain.sort_by_key(|(_, o)| o.order);
        let group = chain
            .iter()
            .map(|(n, _)| Elem { tag: "Link".into(), attrs: HashMap::from([("value".to_string(), (*n).clone())]) })
            .collect();
        let mut body = Obj { kind: "PartDesign::Body".into(), ..Obj::default() };
        body.props.insert("Group".into(), group);
        bodies_list.push(("Body".into(), body));
    }
    // The PartDesign objects a Part object takes as input.
    let wanted: std::collections::HashSet<String> = objects
        .values()
        .filter(|o| PART_KINDS.contains(&o.kind.as_str()) || is_draft_array(o))
        .flat_map(|o| {
            let mut v: Vec<String> = o.link_list("Shapes");
            v.extend(["Base", "Tool", "Faces"].iter().filter_map(|k| o.link(k)).map(|(n, _)| n));
            v
        })
        .collect();
    let mut snaps: Snapshots = HashMap::new();
    let edge_axes = edge_axes(&objects, shape_edges);
    for (body_name, body) in &bodies_list {
        let place = Place::of(body);
        let mut first_solid: Option<String> = None;
        let body_start = features.len();
        let mut prev: Option<String> = None;
        let mut group = body.link_list("Group");
        if group.len() > MAX_FEATURES {
            return Err(format!("{body_name}: too many features"));
        }
        // A fillet or chamfer comes after the feature it rounds, and a pad or pocket after its
        // sketch, whatever order the body's group lists (a document reordered by hand).
        for _ in 0..group.len() {
            let Some((i, j)) = group.iter().enumerate().find_map(|(i, n)| {
                let o = objects.get(n)?;
                let needs = match o.kind.as_str() {
                    "PartDesign::Fillet" | "PartDesign::Chamfer" => o.link("Base"),
                    "PartDesign::Pad" | "PartDesign::Pocket" => o.link("Profile").or_else(|| o.link("Sketch")),
                    _ => None,
                }?;
                let j = group.iter().position(|g| *g == needs.0)?;
                (j > i).then_some((i, j))
            }) else {
                break;
            };
            let moved = group.remove(i);
            group.insert(j, moved);
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
        // The features the object translated last made, and whether its `Refine` is off.
        let mut last_made: (usize, bool) = (features.len(), false);
        for name in &group {
            mark_unrefined(features.get_mut(last_made.0..).unwrap_or_default(), last_made.1);
            last_made = (features.len(), false);
            // The body as it stood after the previous feature, for a Part object that copies it.
            if let Some(p) = prev.take().filter(|p| wanted.contains(p)) {
                snaps.insert(p, (features.get(body_start..).unwrap_or_default().to_vec(), first_solid.clone()));
            }
            if past_tip {
                break;
            }
            prev = Some(name.clone());
            past_tip = tip.as_ref() == Some(name);
            let Some(o) = objects.get(name) else { continue };
            if steps.contains(name) {
                continue;
            }
            last_made.1 = o.unrefined();
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
                    // Up to a face: the extrusion ends on the face the object names (`UpToFace`).
                    if ty == 3
                        && up_to_faces
                        && !o.bool("UseCustomVector")
                        && !(o.bool("Midplane") || matches!(o.int("SideType"), Some(1 | 2)))
                        && let Some((sk, _)) = sketches.get(&profile)
                        && let Some(pick) = up_to_face(o, pocket, sk, &objects, shape_edges)
                    {
                        let op = match (&first_solid, pocket) {
                            (None, false) => "new",
                            (None, true) => return Err(format!("{name}: a pocket before any solid")),
                            (Some(_), false) => "join",
                            (Some(_), true) => "cut",
                        };
                        let mut f = json!({"type": "extrude", "name": name, "sketch": profile, "profiles": "all",
                            "end_condition": "to_face", "face": {"point": pick.point, "normal": pick.normal},
                            "operation": op});
                        if let Some(m) = f.as_object_mut() {
                            if pick.reverse {
                                m.insert("reverse".into(), json!(true));
                            }
                            if let Some(t) = &first_solid {
                                m.insert("target".into(), json!(t));
                            }
                        }
                        features.push(f);
                        first_solid.get_or_insert_with(|| name.clone());
                        continue;
                    }
                    let distance = match (pocket, ty) {
                        (_, 0 | 4) => len,
                        // Through all: far enough for any part (the volume check confirms).
                        (true, 1) => 100_000.0,
                        // Up to a face, the first or last face: the solid FreeCAD added or
                        // removed is stored (`AddSubShape`); its reach along the sketch normal is
                        // the depth (the rebuild check confirms a plain extrusion was meant).
                        (_, 1..=3) => {
                            let file = o
                                .first("AddSubShape", "Part")
                                .and_then(|e| e.attrs.get("file"))
                                .filter(|f| !f.is_empty())
                                .ok_or(format!("{name}: an up-to extrusion without its stored shape"))?;
                            let (sk, _) = sketches.get(&profile).ok_or(format!("{name}: no profile frame"))?;
                            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                            for e in shape_edges(file).map_err(|e| format!("{name}: {e}"))? {
                                for p in e.path.iter().chain(std::iter::once(&e.mid)) {
                                    let h = (p[0] - sk.p[0]) * sk.z[0]
                                        + (p[1] - sk.p[1]) * sk.z[1]
                                        + (p[2] - sk.p[2]) * sk.z[2];
                                    lo = lo.min(h);
                                    hi = hi.max(h);
                                }
                            }
                            // The stored solid is what was actually added or removed, which can
                            // start past empty space: the extrusion runs from the sketch to its
                            // far end. On both sides of the sketch it is something else.
                            if !(lo.is_finite() && hi.is_finite()) || (lo < -1e-4 && hi > 1e-4) {
                                return Err(format!(
                                    "{name}: its up-to extrusion reaches both sides of the sketch ({lo:.4} to {hi:.4} mm)"
                                ));
                            }
                            let signed = if hi.abs() >= lo.abs() { hi } else { lo };
                            // Undo the sign rules below so that the extrusion is `signed`.
                            let mut d = if pocket { -signed } else { signed };
                            if o.bool("Reversed") {
                                d = -d;
                            }
                            d
                        }
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
                    // (Two lengths ignores `Reversed`: FreeCAD offsets the second length instead.)
                    let mut d = if pocket { -distance } else { distance };
                    if o.bool("Reversed") && ty != 4 {
                        d = -d;
                    }
                    // FreeCAD 1.1 and later: `SideType` replaces `Midplane` (0 one side, 1 two sides,
                    // 2 symmetric).
                    let symmetric = o.bool("Midplane") || o.int("SideType") == Some(2);
                    if o.int("SideType") == Some(1) {
                        return Err(format!(
                            "{name}: a two-sided {} is not translated yet",
                            if pocket { "pocket" } else { "pad" }
                        ));
                    }
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
                            // The sketch's own axes, as construction lines: Gusset's sketch axes
                            // run through its plane's origin, which need not be this sketch's.
                            "V_Axis" | "H_Axis" => {
                                let sk = *sk_place;
                                let d = if sub == "V_Axis" { sk.y } else { sk.x };
                                axis_od = Some((sk.p, d));
                                match axis_line(&mut features, &profile, &sk, sk.p, d) {
                                    Some(id) => json!({"line": id}),
                                    None => json!({"origin": sk.p, "direction": d}),
                                }
                            }
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
                        let refs = Refs { objects: &objects, body: &place, sketches: &sketches, edge_axes: &edge_axes };
                        let (o3, d3) = refs.axis(name, &(axis_obj.clone(), subs.clone()))?;
                        axis_od = Some((o3, d3));
                        match axis_line(&mut features, &profile, &sk, o3, d3) {
                            Some(id) => json!({"line": id}),
                            None => json!({"origin": o3, "direction": d3}),
                        }
                    };
                    let angle = o.float("Angle").unwrap_or(360.0);
                    if o.int("Type").unwrap_or(0) != 0 {
                        return Err(format!("{name}: a bounded revolution is not translated yet"));
                    }
                    // Symmetric about the sketch plane: half the angle each way (a full turn is
                    // the same either way).
                    let symmetric = o.bool("Midplane") && angle < 360.0 - 1e-9;
                    // Gusset turns about its axis one way: a reversed part turn is a turn about
                    // the reversed axis, drawn into the sketch (a full turn needs neither).
                    let mut axis_back: Option<Value> = None;
                    if (o.bool("Reversed") || symmetric) && angle < 360.0 - 1e-9 {
                        let v3 = |k: &str| -> Option<V3> {
                            let a = axis.get(k)?.as_array()?;
                            Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
                        };
                        let (ao, ad) = axis_od
                            .or_else(|| Some((v3("origin")?, v3("direction")?)))
                            .ok_or(format!("{name}: a reversed turn about a sketch edge is not translated yet"))?;
                        let rev = ad.map(|c| -c);
                        let sk = *sk_place;
                        let reversed = match axis_line(&mut features, &profile, &sk, ao, rev) {
                            Some(id) => json!({"line": id}),
                            None => json!({"origin": ao, "direction": rev}),
                        };
                        if symmetric {
                            axis_back = Some(reversed);
                        } else {
                            axis = reversed;
                        }
                    }
                    let op = match (&first_solid, groove) {
                        (None, false) => "new",
                        (None, true) => return Err(format!("{name}: a groove before any solid")),
                        (Some(_), false) => "join",
                        (Some(_), true) => "cut",
                    };
                    let turn = if symmetric { angle / 2.0 } else { angle };
                    let mut f = json!({"type": "revolve", "name": name, "sketch": profile, "profiles": "all",
                        "axis": axis, "angle": turn, "operation": op});
                    if let (Some(t), Some(m)) = (&first_solid, f.as_object_mut()) {
                        m.insert("target".into(), json!(t));
                    }
                    first_solid.get_or_insert_with(|| name.clone());
                    features.push(f);
                    if let Some(back) = axis_back {
                        features.push(json!({"type": "revolve", "name": format!("{name}_back"), "sketch": profile,
                            "profiles": "all", "axis": back, "angle": turn, "operation": if groove { "cut" } else { "join" },
                            "target": first_solid.clone().unwrap_or_else(|| name.clone())}));
                    }
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
                    // FreeCAD copies the whole extruded or revolved tool: each pad, pocket, revolution or
                    // groove is repeated as a separate tool body that is patterned and then joined or cut.
                    let tools: Vec<String> = feats
                        .iter()
                        .flat_map(|n| [n.clone(), format!("{n}_back")])
                        .filter(|n| {
                            features.iter().any(|f| {
                                f.get("name").and_then(Value::as_str) == Some(n)
                                    && matches!(f.get("type").and_then(Value::as_str), Some("extrude" | "revolve"))
                                    && matches!(f.get("operation").and_then(Value::as_str), Some("cut" | "join"))
                            })
                        })
                        .collect();
                    if tool_patterns && !whole && !tools.is_empty() && tools.len() >= feats.len() {
                        let n_tools = tools.len();
                        for (ti, orig) in tools.iter().enumerate() {
                            let Some(src) =
                                features.iter().find(|f| f.get("name").and_then(Value::as_str) == Some(orig)).cloned()
                            else {
                                continue;
                            };
                            let op = src.get("operation").cloned().unwrap_or(json!("cut"));
                            let tool = format!("{orig}_tool");
                            let mut t = src;
                            if let Some(m) = t.as_object_mut() {
                                m.insert("name".into(), json!(tool));
                                m.insert("operation".into(), json!("new"));
                                m.remove("target");
                            }
                            features.push(t);
                            for (step_name, so) in &steps {
                                let refs = Refs {
                                    objects: &objects,
                                    body: &place,
                                    sketches: &sketches,
                                    edge_axes: &edge_axes,
                                };
                                let Some(mut f) = transform(&format!("{step_name}_{tool}"), so, &refs)? else {
                                    continue;
                                };
                                if let Some(m) = f.as_object_mut() {
                                    m.insert("target".into(), json!(tool));
                                }
                                features.push(f);
                            }
                            let cname = if ti + 1 == n_tools { name.clone() } else { format!("{name}_{}", ti + 1) };
                            features.push(json!({"type": "combine", "name": cname, "target": target,
                                "tools": [tool], "operation": op}));
                        }
                        continue;
                    }
                    for (step_name, so) in steps {
                        let refs = Refs { objects: &objects, body: &place, sketches: &sketches, edge_axes: &edge_axes };
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
                | "PartDesign::SubtractiveSphere"
                | "PartDesign::AdditiveEllipsoid"
                | "PartDesign::SubtractiveEllipsoid" => {
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
                    let mut picked: Vec<usize> = Vec::new();
                    let mut add = |i: usize| {
                        let m = edges.get(i).map(|e| e.mid).unwrap_or_default();
                        let same = |j: &usize| {
                            edges.get(*j).is_some_and(|p| p.mid.iter().zip(m).all(|(a, b)| (a - b).abs() < 1e-9))
                        };
                        if !picked.iter().any(same) {
                            picked.push(i);
                        }
                    };
                    if o.bool("UseAllEdges") {
                        (0..edges.len()).for_each(&mut add);
                    }
                    for sub in &subs {
                        let n = |p: &str| sub.strip_prefix(p).and_then(|n| n.parse::<usize>().ok()).filter(|n| *n > 0);
                        if let Some(i) = n("Edge") {
                            if i > edges.len() {
                                return Err(format!("{name}: {sub} is not on its base"));
                            }
                            add(i - 1);
                        } else if let Some(i) = n("Face") {
                            let f = u32::try_from(i - 1).map_err(|_| format!("{name}: a bad face"))?;
                            (0..edges.len())
                                .filter(|k| edges.get(*k).is_some_and(|e| e.faces.contains(&f) && !e.smooth))
                                .for_each(&mut add);
                        } else if !sub.is_empty() {
                            return Err(format!("{name}: a pick of {sub} is not translated yet"));
                        }
                    }
                    if picked.is_empty() || picked.len() > MAX_EDGE_PICKS {
                        return Err(format!("{name}: {} edges", picked.len()));
                    }
                    let picks: Vec<Value> = picked
                        .iter()
                        .filter_map(|i| edges.get(*i))
                        .map(|e| json!({"midpoint": e.mid, "path": e.path}))
                        .collect();
                    let f = if fillet {
                        let r = o.float("Radius").filter(|r| *r > 0.0).ok_or(format!("{name}: no radius"))?;
                        json!({"type": "fillet", "name": name, "edges": picks, "radius": r, "target": target})
                    } else {
                        let d = o.float("Size").filter(|r| *r > 0.0).ok_or(format!("{name}: no size"))?;
                        let mut c =
                            json!({"type": "chamfer", "name": name, "edges": picks, "distance": d, "target": target});
                        // 0 equal distance, 1 two distances, 2 a distance and an angle. The
                        // reference face is left to Gusset's default; the rebuild check confirms.
                        let extra = match o.int("ChamferType").unwrap_or(0) {
                            0 => None,
                            1 => Some((
                                "distance2",
                                o.float("Size2").filter(|v| *v > 0.0).ok_or(format!("{name}: no second size"))?,
                            )),
                            2 => Some((
                                "angle",
                                o.float("Angle")
                                    .filter(|v| *v > 0.0 && *v < 90.0)
                                    .ok_or(format!("{name}: no angle"))?,
                            )),
                            _ => return Err(format!("{name}: this chamfer type is not translated yet")),
                        };
                        if let (Some((k, v)), Some(m)) = (extra, c.as_object_mut()) {
                            m.insert(k.into(), json!(v));
                        }
                        c
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
                    let helix = helix_spine(name, &objects, &spine, &edges, shape_edges)?;
                    if helix.is_some() && !is_identity(&place) {
                        return Err(format!("{name}: a helix path in a placed body is not translated yet"));
                    }
                    let no_refs = HashMap::new();
                    let refs = if helix.is_some() {
                        &no_refs
                    } else {
                        &sketches.get(&spine).ok_or(format!("{name}: its path is not a sketch of the body"))?.1
                    };
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
                    for e in edges.iter().filter(|e| !e.is_empty() && helix.is_none()) {
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
                    if let (Some(h), Some(m)) = (helix, f.as_object_mut()) {
                        m.remove("path");
                        m.remove("path_curves");
                        m.insert("helix".into(), h);
                    }
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
                // A helix wrapped as the body's base feature is only a path for a pipe.
                "PartDesign::FeatureBase" if helix_object(&objects, name).is_some() => {}
                // Datums and the origin carry no geometry of the body.
                // A datum plane a pad or pocket takes as its profile is the rectangle of its
                // size, centred on its placement.
                "PartDesign::Plane" if objects.values().any(|p| p.link("Profile").is_some_and(|(n, _)| &n == name)) => {
                    let (Some(l), Some(w)) = (o.float("Length"), o.float("Width")) else {
                        return Err(format!("{name}: a datum plane without size"));
                    };
                    let (hx, hy) = (l / 2.0, w / 2.0);
                    let corners = [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)];
                    let points: Vec<Value> = corners
                        .iter()
                        .enumerate()
                        .map(|(i, (x, y))| json!({"id": format!("p{}", i + 1), "x": x, "y": y}))
                        .collect();
                    let curves: Vec<Value> = (0..4)
                        .map(|i| json!({"id": format!("l{}", i + 1), "type": "line", "start": format!("p{}", i + 1), "end": format!("p{}", (i + 1) % 4 + 1), "construction": false}))
                        .collect();
                    let pl = place.then(&Place::of(o));
                    features.push(json!({"type": "sketch", "name": name, "plane": {"origin": pl.p, "x_axis": pl.x, "y_axis": pl.y}, "points": points, "curves": curves}));
                    let lines = (0..4).map(|i| (format!("Edge{}", i + 1), format!("l{}", i + 1))).collect();
                    sketches.insert(name.clone(), (pl, lines));
                }
                "App::Origin"
                | "App::Line"
                | "App::Plane"
                | "PartDesign::Plane"
                | "PartDesign::Line"
                | "PartDesign::Point"
                | "PartDesign::ShapeBinder" => {}
                other => return Err(format!("{name}: {other} is not translated yet")),
            }
        }
        mark_unrefined(features.get_mut(last_made.0..).unwrap_or_default(), last_made.1);
        if let Some(p) = prev.take().filter(|p| wanted.contains(p)) {
            snaps.insert(p, (features.get(body_start..).unwrap_or_default().to_vec(), first_solid.clone()));
        }
        if wanted.contains(body_name) {
            snaps.insert(
                body_name.clone(),
                (features.get(body_start..).unwrap_or_default().to_vec(), first_solid.clone()),
            );
        }
        if let Some(f) = &first_solid {
            body_of.insert(body_name.clone(), f.clone());
            // A Part object may take a body's feature as input: the body as built (the
            // rebuild check catches a feature that is not the body's last).
            for n in &group {
                body_of.entry(n.clone()).or_insert_with(|| f.clone());
            }
        }
    }
    part_features(&objects, &mut features, &mut body_of, copy_sources.then_some(&snaps), shape_edges)?;
    if features.is_empty() {
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

    /// The test document with the pocket cut up to `Face1` of the pad (z = -3, normal -z) and the
    /// pad's shape stored, with the given offset.
    fn up_to_face_doc(offset: &str) -> String {
        DOC.replace(
            r#"<Property name="Length" type="App::PropertyLength"><Float value="2"/></Property>"#,
            r#"<Property name="Length" type="App::PropertyLength"><Float value="2"/></Property>
    <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape.brp"/></Property>"#,
        )
        .replace(
            r#"<Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>"#,
            &format!(
                r#"<Property name="UpToFace" type="App::PropertyLinkSub"><LinkSub value="Pad" count="1"><Sub value="Face1"/></LinkSub></Property>
    <Property name="Offset" type="App::PropertyLength"><Float value="{offset}"/></Property>"#
            ),
        )
        .replace(
            r#"<Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="0"></LinkSub></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#,
            r#"<Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="0"></LinkSub></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="3"/></Property>"#,
        )
    }

    fn pad_shape(_: &str) -> Result<Edges, String> {
        let face = |z: f64, nz: f64| SrcFace { planar: true, point: [5.0, 1.0, z], normal: [0.0, 0.0, nz] };
        Ok(Edges { edges: Vec::new(), faces: vec![face(-3.0, -1.0), face(2.0, 1.0)] })
    }

    #[test]
    fn a_pocket_up_to_a_face_names_the_face() {
        let t = tree(up_to_face_doc("0").as_bytes(), &mut pad_shape).unwrap();
        let f = &t.features[2];
        assert_eq!(f["end_condition"], "to_face");
        assert_eq!(f["face"], serde_json::json!({"point": [5.0, 1.0, -3.0], "normal": [0.0, 0.0, -1.0]}));
        assert_eq!(f["operation"], "cut");
        assert_eq!(f["reverse"], true, "a pocket cuts against its sketch's normal");
        assert!(f.get("distance").is_none());
        // Without faces (the fallback variant) and with an offset the distance path stands: with
        // no stored shape of the pocket it is refused.
        let off = tree_options(up_to_face_doc("0").as_bytes(), &mut pad_shape, false, false, false);
        assert!(off.unwrap_err().contains("without its stored shape"));
        let off = tree(up_to_face_doc("1.5").as_bytes(), &mut pad_shape);
        assert!(off.unwrap_err().contains("without its stored shape"));
    }

    #[test]
    fn a_face_behind_the_sweep_is_not_an_up_to_face() {
        // Face2 (z = 2, normal +z) lies on the other side of the sketch: the pocket cuts against +z.
        let doc = up_to_face_doc("0").replace(r#"<Sub value="Face1"/>"#, r#"<Sub value="Face2"/>"#);
        assert!(tree(doc.as_bytes(), &mut pad_shape).unwrap_err().contains("without its stored shape"));
    }

    #[test]
    fn refine_false_marks_the_joining_and_cutting_features() {
        let refine =
            |t: &crate::tree_import::Tree| t.features.iter().map(|f| f.get("refine").cloned()).collect::<Vec<_>>();
        // Refined (or no property): nothing is marked.
        let t = tree(DOC.as_bytes(), &mut no_shapes).unwrap();
        assert!(refine(&t).iter().all(Option::is_none));
        // Refine = false on the pad (a new body: nothing to keep) and on the pocket.
        let off = r#"<Property name="Refine" type="App::PropertyBool"><Bool value="false"/></Property>
    <Property name="Type" type="App::PropertyEnumeration">"#;
        let doc = DOC.replace(r#"<Property name="Type" type="App::PropertyEnumeration">"#, off);
        let t = tree(doc.as_bytes(), &mut no_shapes).unwrap();
        let marks = refine(&t);
        assert_eq!(marks[1], None, "a first pad does no boolean");
        assert_eq!(marks[2], Some(serde_json::json!(false)), "{:?}", t.features[2]);
    }

    #[test]
    fn a_group_listing_a_pad_before_its_sketch_is_put_in_order() {
        let doc = DOC.replace(
            r#"<Link value="Sketch"/><Link value="Pad"/><Link value="Pocket"/>"#,
            r#"<Link value="Pad"/><Link value="Sketch"/><Link value="Pocket"/>"#,
        );
        let t = tree(doc.as_bytes(), &mut no_shapes).unwrap();
        let kinds: Vec<&str> = t.features.iter().filter_map(|f| f["type"].as_str()).collect();
        assert_eq!(kinds, ["sketch", "extrude", "extrude"]);
    }

    #[test]
    fn an_old_pad_names_its_sketch_and_a_shape_binder_is_a_datum() {
        // Before bodies, a pad's link is `Sketch`, and the group may list the pad first.
        let doc = DOC
            .replace(r#"name="Profile" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="0">"#, r#"name="Sketch" type="App::PropertyLink"><Link value="Sketch" count="0">"#)
            .replace("</LinkSub></Property>", "</Link></Property>")
            .replace(
                r#"<Link value="Sketch"/><Link value="Pad"/><Link value="Pocket"/>"#,
                r#"<Link value="Pad"/><Link value="Sketch"/><Link value="Pocket"/>"#,
            )
            .replace(
                r#"<Object type="PartDesign::Pocket" name="Pocket"/>"#,
                r#"<Object type="PartDesign::Pocket" name="Pocket"/><Object type="PartDesign::ShapeBinder" name="Binder"/>"#,
            )
            .replace(r#"<Link value="Pocket"/></LinkList>"#, r#"<Link value="Pocket"/><Link value="Binder"/></LinkList>"#);
        let t = tree(doc.as_bytes(), &mut no_shapes).unwrap();
        let kinds: Vec<&str> = t.features.iter().filter_map(|f| f["type"].as_str()).collect();
        assert_eq!(kinds, ["sketch", "extrude", "extrude"]);
    }

    #[test]
    fn a_datum_plane_profile_is_a_rectangle_centred_on_the_plane() {
        let doc = DOC
            .replace(
                r#"<Object type="PartDesign::Pocket" name="Pocket"/>"#,
                r#"<Object type="PartDesign::Pocket" name="Pocket"/><Object type="PartDesign::Plane" name="Datum"/>"#,
            )
            .replace(r#"<Link value="Pad"/><Link value="Pocket"/>"#, r#"<Link value="Pad"/><Link value="Datum"/><Link value="Pocket"/>"#)
            .replace("</ObjectData>", r#"<Object name="Datum"><Properties>
    <Property name="Length" type="App::PropertyLength"><Float value="20"/></Property>
    <Property name="Width" type="App::PropertyLength"><Float value="10"/></Property>
    <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="3" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  </Properties></Object></ObjectData>"#)
            .replace(
                r#"<Object name="Pocket"><Properties>
    <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
    <Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="0">"#,
                r#"<Object name="Pocket"><Properties>
    <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
    <Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Datum" count="0">"#,
            );
        let t = tree(doc.as_bytes(), &mut no_shapes).unwrap();
        let sk = t.features.iter().find(|f| f["name"] == "Datum").unwrap();
        assert_eq!(sk["type"], "sketch");
        assert_eq!(sk["plane"]["origin"], json!([0.0, 0.0, 3.0]));
        assert_eq!(sk["points"][2]["x"], 10.0);
        assert_eq!(sk["points"][2]["y"], 5.0);
        assert_eq!(t.features.last().unwrap()["sketch"], "Datum");
    }

    #[test]
    fn a_symmetric_side_type_is_a_midplane_pad() {
        let doc = DOC.replace(
            r#"<Property name="Length" type="App::PropertyLength"><Float value="2"/></Property>"#,
            r#"<Property name="Length" type="App::PropertyLength"><Float value="2"/></Property>
    <Property name="SideType" type="App::PropertyEnumeration"><Integer value="2"/></Property>"#,
        );
        let t = tree(doc.as_bytes(), &mut no_shapes).unwrap();
        assert_eq!(t.features[1]["symmetric"], true);
        assert_eq!(t.features[1]["distance"], 2.0);
    }

    #[test]
    fn two_lengths_ignore_reversed() {
        // FreeCAD offsets the second length of a two-length pad or pocket; `Reversed` does not
        // turn the pair around (the stored solids of the examples show it).
        let lens = |reversed: &str| {
            let doc = DOC.replace(
                r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#,
                &format!(
                    r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="4"/></Property>
    <Property name="Length2" type="App::PropertyLength"><Float value="3"/></Property>
    <Property name="Reversed" type="App::PropertyBool"><Bool value="{reversed}"/></Property>"#
                ),
            );
            let t = tree(doc.as_bytes(), &mut no_shapes).unwrap();
            let d = |i: usize| t.features[i]["distance"].as_f64().unwrap();
            (d(2), d(3))
        };
        assert_eq!(lens("false"), (-1.0, 3.0));
        assert_eq!(lens("true"), lens("false"));
    }

    #[test]
    fn a_pattern_of_a_pocket_can_copy_the_whole_tool() {
        let doc = DOC
            .replace(
                r#"<Object type="PartDesign::Pocket" name="Pocket"/>"#,
                r#"<Object type="PartDesign::Pocket" name="Pocket"/>
  <Object type="PartDesign::LinearPattern" name="LinearPattern"/>"#,
            )
            .replace(
                r#"<Link value="Pocket"/></LinkList></Property>
  </Properties></Object>
  <Object name="Sketch">"#,
                r#"<Link value="Pocket"/><Link value="LinearPattern"/></LinkList></Property>
  </Properties></Object>
  <Object name="Sketch">"#,
            )
            .replace(
                "</ObjectData>",
                r#"  <Object name="LinearPattern"><Properties>
    <Property name="Direction" type="App::PropertyLinkSub"><LinkSub value="X_Axis" count="1"><Sub value=""/></LinkSub></Property>
    <Property name="Length" type="App::PropertyLength"><Float value="30"/></Property>
    <Property name="Occurrences" type="App::PropertyIntegerConstraint"><Integer value="4"/></Property>
    <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Pocket"/></LinkList></Property>
  </Properties></Object>
</ObjectData>"#,
            );
        let plain = tree_with(doc.as_bytes(), &mut no_shapes, false).unwrap();
        let kinds = |t: &crate::tree_import::Tree| {
            t.features.iter().map(|f| f["type"].as_str().unwrap().to_string()).collect::<Vec<_>>()
        };
        assert_eq!(kinds(&plain).last().map(String::as_str), Some("pattern_linear"));
        let tools = tree_with(doc.as_bytes(), &mut no_shapes, true).unwrap();
        let k = kinds(&tools);
        assert_eq!(&k[3..], ["extrude", "pattern_linear", "combine"], "{k:?}");
        let (tool, pat, comb) = (&tools.features[3], &tools.features[4], &tools.features[5]);
        assert_eq!(tool["operation"], "new");
        assert_eq!(tool["name"], "Pocket_tool");
        assert_eq!(pat["target"], "Pocket_tool");
        assert_eq!(pat["spacing"], 10.0);
        assert_eq!(comb["name"], "LinearPattern");
        assert_eq!(comb["operation"], "cut");
        assert_eq!(comb["tools"][0], "Pocket_tool");
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
        let doc = DOC.replace("PartDesign::Pocket", "PartDesign::AdditiveHelix");
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
  <Object type="Part::Offset2D" name="Cut"/>"#,
            )
            .replace(
                r#"<ObjectData Count="4">"#,
                r#"<ObjectData Count="5">
  <Object name="Cut"><Properties>
    <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape9.brp"/></Property>
  </Properties></Object>"#,
            );
        assert!(tree(doc.as_bytes(), &mut no_shapes).unwrap_err().contains("Part::Offset2D outside a PartDesign body"));
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

    #[test]
    fn part_primitives_and_booleans_become_bodies_and_combines() {
        let doc = r#"<Document SchemaVersion="4">
<Objects Count="3">
  <Object type="Part::Box" name="Box"/>
  <Object type="Part::Cylinder" name="Cylinder"/>
  <Object type="Part::Cut" name="Cut"/>
</Objects>
<ObjectData Count="3">
  <Object name="Box"><Properties>
    <Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
    <Property name="Width" type="App::PropertyLength"><Float value="10"/></Property>
    <Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>
    <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape.brp"/></Property>
  </Properties></Object>
  <Object name="Cylinder"><Properties>
    <Property name="Radius" type="App::PropertyLength"><Float value="2"/></Property>
    <Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>
    <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape1.brp"/></Property>
  </Properties></Object>
  <Object name="Cut"><Properties>
    <Property name="Base" type="App::PropertyLink"><Link value="Box"/></Property>
    <Property name="Tool" type="App::PropertyLink"><Link value="Cylinder"/></Property>
    <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape2.brp"/></Property>
  </Properties></Object>
</ObjectData>
</Document>"#;
        let t = tree(doc.as_bytes(), &mut no_shapes).unwrap();
        let kinds: Vec<&str> = t.features.iter().filter_map(|f| f["type"].as_str()).collect();
        assert_eq!(kinds, ["sketch", "extrude", "sketch", "extrude", "combine"]);
        let c = &t.features[4];
        assert_eq!((c["target"].as_str(), c["operation"].as_str()), (Some("Box"), Some("cut")));
        assert_eq!(c["tools"], serde_json::json!(["Cylinder"]));
    }

    #[test]
    fn a_part_object_can_take_a_copy_of_a_body_it_leaves_standing() {
        // A box cut from the body's pocket stage: the body (and its pocket) stay as they are.
        let doc = DOC
            .replace(
                r#"<Object type="PartDesign::Pocket" name="Pocket"/>
</Objects>"#,
                r#"<Object type="PartDesign::Pocket" name="Pocket"/>
  <Object type="Part::Box" name="Box"/>
  <Object type="Part::Cut" name="Cut"/>
</Objects>"#,
            )
            .replace(
                "</ObjectData>",
                r#"  <Object name="Box"><Properties>
    <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
    <Property name="Width" type="App::PropertyLength"><Float value="1"/></Property>
    <Property name="Height" type="App::PropertyLength"><Float value="1"/></Property>
  </Properties></Object>
  <Object name="Cut"><Properties>
    <Property name="Base" type="App::PropertyLink"><Link value="Body"/></Property>
    <Property name="Tool" type="App::PropertyLink"><Link value="Box"/></Property>
  </Properties></Object>
</ObjectData>"#,
            );
        let names = |t: &crate::tree_import::Tree| -> Vec<String> {
            t.features.iter().filter_map(|f| f["name"].as_str().map(String::from)).collect()
        };
        // Consumed: the cut works on the body itself.
        let plain = tree_variant(doc.as_bytes(), &mut no_shapes, false, false).unwrap();
        assert!(!names(&plain).iter().any(|n| n.ends_with("_c1")));
        assert_eq!(plain.features.last().unwrap()["target"], "Pad");
        // Copied: the body's features come again under new names, and the cut takes the copy.
        let copy = tree_variant(doc.as_bytes(), &mut no_shapes, false, true).unwrap();
        let n = names(&copy);
        assert!(n.contains(&"Pad".to_string()) && n.contains(&"Pad_c1".to_string()), "{n:?}");
        let cut = copy.features.last().unwrap();
        assert_eq!((cut["target"].as_str(), cut["operation"].as_str()), (Some("Pad_c1"), Some("cut")));
        let sketch_copy = copy.features.iter().find(|f| f["name"] == "Sketch_c1").expect("the sketch is copied");
        assert_eq!(sketch_copy["type"], "sketch");
        let pad_copy = copy.features.iter().find(|f| f["name"] == "Pad_c1").unwrap();
        assert_eq!(pad_copy["sketch"], "Sketch_c1");
    }

    #[test]
    fn a_pattern_direction_can_be_a_straight_edge_of_a_stored_shape() {
        let doc = DOC
            .replace(
                r#"<Object type="PartDesign::Pocket" name="Pocket"/>
</Objects>"#,
                r#"<Object type="PartDesign::Pocket" name="Pocket"/>
  <Object type="PartDesign::LinearPattern" name="LinearPattern"/>
</Objects>"#,
            )
            .replace(
                r#"<Link value="Pocket"/></LinkList></Property>"#,
                r#"<Link value="Pocket"/><Link value="LinearPattern"/></LinkList></Property>"#,
            )
            .replace(
                r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#,
                r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>
    <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape2.brp"/></Property>"#,
            )
            .replace(
                "</ObjectData>",
                r#"  <Object name="LinearPattern"><Properties>
    <Property name="Direction" type="App::PropertyLinkSub"><LinkSub value="Pocket" count="1"><Sub value="Edge2"/></LinkSub></Property>
    <Property name="Length" type="App::PropertyLength"><Float value="6"/></Property>
    <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
    <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Pocket"/></LinkList></Property>
  </Properties></Object>
</ObjectData>"#,
            );
        let mut edges = |_: &str| -> Result<Edges, String> {
            let edge = |a: [f64; 3], b: [f64; 3]| SrcEdge {
                mid: [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0],
                faces: vec![0, 1],
                path: vec![a, b],
                smooth: false,
            };
            Ok(vec![edge([0.0, 0.0, 0.0], [5.0, 0.0, 0.0]), edge([1.0, 1.0, 0.0], [1.0, 1.0, 4.0])].into())
        };
        let t = tree(doc.as_bytes(), &mut edges).unwrap();
        let p = t.features.iter().find(|f| f["type"] == "pattern_linear").expect("the pattern");
        assert_eq!(p["direction"], serde_json::json!([0.0, 0.0, 1.0]));
        assert_eq!(p["spacing"], 3.0);
    }

    /// A radius 1 circle profile, a helix (radius 9.4, pitch 8.67, height 26, along world Y from
    /// (0, 5.5, 0)) and `extra` objects; `props` are the helix's extra properties.
    fn helix_doc(props: &str, extra: &str, objects: &str) -> String {
        format!(
            r#"<Document SchemaVersion="4">
<Objects Count="4">
  <Object type="Sketcher::SketchObject" name="Profile"/>
  <Object type="Part::Helix" name="Helix"/>
  {objects}
</Objects>
<ObjectData Count="4">
  <Object name="Profile"><Properties>
    <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1">
      <Geometry type="Part::GeomCircle"><Construction value="0"/><Circle CenterX="5" CenterY="0" CenterZ="0" NormalX="0" NormalY="0" NormalZ="1" Radius="1"/></Geometry>
    </GeometryList></Property>
    <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="5" Pz="0" Q0="0" Q1="0.707106781187" Q2="0" Q3="-0.707106781187"/></Property>
  </Properties></Object>
  <Object name="Helix"><Properties>
    <Property name="Radius" type="App::PropertyLength"><Float value="9.4"/></Property>
    <Property name="Pitch" type="App::PropertyLength"><Float value="8.67"/></Property>
    <Property name="Height" type="App::PropertyLength"><Float value="26"/></Property>
    <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="5.5" Pz="0" Q0="0.5" Q1="0.5" Q2="0.5" Q3="-0.5"/></Property>
    <Property name="Shape" type="Part::PropertyPartShape"><Part file="Helix.brp"/></Property>
    {props}
  </Properties></Object>
  {extra}
</ObjectData>
</Document>"#
        )
    }

    const SWEEP: &str = r#"<Object name="Sweep"><Properties>
    <Property name="Sections" type="App::PropertyLinkList"><LinkList count="1"><Link value="Profile"/></LinkList></Property>
    <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Helix" count="3"><Sub value="Edge1"/><Sub value="Edge2"/><Sub value="Edge3"/></LinkSub></Property>
    <Property name="Frenet" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="Solid" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="Shape" type="Part::PropertyPartShape"><Part file="Sweep.brp"/></Property>
  </Properties></Object>"#;

    /// Three turns of the helix: one edge each, their midpoints on the cylinder (`radius`).
    fn helix_edges(radius: f64) -> impl FnMut(&str) -> Result<Edges, String> {
        move |_| {
            let mid = |p: [f64; 3]| SrcEdge { mid: p, ..SrcEdge::default() };
            Ok(Edges::from(vec![mid([radius, 10.0, 0.0]), mid([0.0, 18.0, radius]), mid([-radius, 26.0, 0.0])]))
        }
    }

    fn sweep_doc(props: &str) -> String {
        helix_doc(props, SWEEP, r#"<Object type="Part::Sweep" name="Sweep"/>"#)
    }

    #[test]
    fn a_part_sweep_along_a_helix_carries_its_axis_pitch_and_hand() {
        let t = tree(sweep_doc("").as_bytes(), &mut helix_edges(9.4)).unwrap();
        let kinds: Vec<&str> = t.features.iter().filter_map(|f| f["type"].as_str()).collect();
        assert_eq!(kinds, ["sketch", "sweep"]);
        let s = &t.features[1];
        assert_eq!((s["sketch"].as_str(), s["orientation"].as_str()), (Some("Profile"), Some("frenet")));
        assert!(s.get("path").is_none() && s.get("path_curves").is_none(), "{s}");
        let h = &s["helix"];
        let near = |v: &Value, w: [f64; 3]| (0..3).all(|i| (v[i].as_f64().unwrap() - w[i]).abs() < 1e-9);
        assert!(near(&h["origin"], [0.0, 5.5, 0.0]) && near(&h["axis"], [0.0, 1.0, 0.0]), "{h}");
        assert!(near(&h["x"], [0.0, 0.0, 1.0]), "{h}");
        assert_eq!(
            (h["radius"].as_f64(), h["pitch"].as_f64(), h["height"].as_f64()),
            (Some(9.4), Some(8.67), Some(26.0))
        );
        assert_eq!(h["left_handed"], false);
        assert!(h.get("taper_angle").is_none());
        let left = r#"<Property name="LocalCoord" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#;
        let t = tree(sweep_doc(left).as_bytes(), &mut helix_edges(9.4)).unwrap();
        assert_eq!(t.features[1]["helix"]["left_handed"], true);
    }

    #[test]
    fn a_stored_angle_is_ignored_only_when_the_stored_helix_is_a_cylinder() {
        let angle = r#"<Property name="Angle" type="App::PropertyAngle"><Float value="90"/></Property>"#;
        let t = tree(sweep_doc(angle).as_bytes(), &mut helix_edges(9.4)).unwrap();
        assert!(t.features[1]["helix"].get("taper_angle").is_none());
        // Edges off the cylinder: a cone; the tree refuses it.
        let angle = r#"<Property name="Angle" type="App::PropertyAngle"><Float value="10"/></Property>"#;
        let t = tree(sweep_doc(angle).as_bytes(), &mut helix_edges(7.0)).unwrap();
        assert_eq!(t.features[1]["helix"]["taper_angle"], 10.0);
        // Without an angle, a stored shape off its cylinder is a different helix than the parameters say.
        let e = tree(sweep_doc("").as_bytes(), &mut helix_edges(7.0)).unwrap_err();
        assert!(e.contains("does not match its parameters"), "{e}");
    }

    #[test]
    fn sweeps_that_are_not_a_whole_helix_are_not_translated() {
        let e = tree(sweep_doc("").as_bytes(), &mut no_shapes).unwrap_err();
        assert!(e.contains("no shapes"), "{e}");
        let partial = sweep_doc("").replace(r#"<Sub value="Edge3"/>"#, "");
        let e = tree(partial.as_bytes(), &mut helix_edges(9.4)).unwrap_err();
        assert!(e.contains("some edges of a helix"), "{e}");
        let two =
            sweep_doc("").replace(r#"<Link value="Profile"/>"#, r#"<Link value="Profile"/><Link value="Profile"/>"#);
        let e = tree(two.as_bytes(), &mut helix_edges(9.4)).unwrap_err();
        assert!(e.contains("several sections"), "{e}");
        let line = sweep_doc("").replace(r#"<LinkSub value="Helix""#, r#"<LinkSub value="Profile""#);
        let e = tree(line.as_bytes(), &mut helix_edges(9.4)).unwrap_err();
        assert!(e.contains("other than a helix"), "{e}");
    }

    #[test]
    fn a_pipe_along_a_helix_base_feature_uses_the_base_features_placement() {
        // A PartDesign body holding the helix as a base feature (placement and shape of its own).
        let doc = r#"<Document SchemaVersion="4">
<Objects Count="5">
  <Object type="PartDesign::Body" name="Body"/>
  <Object type="Part::Helix" name="Helix"/>
  <Object type="PartDesign::FeatureBase" name="BaseFeature"/>
  <Object type="Sketcher::SketchObject" name="Sketch"/>
  <Object type="PartDesign::AdditivePipe" name="AdditivePipe"/>
</Objects>
<ObjectData Count="5">
  <Object name="Body"><Properties>
    <Property name="Group" type="App::PropertyLinkList"><LinkList count="3"><Link value="BaseFeature"/><Link value="Sketch"/><Link value="AdditivePipe"/></LinkList></Property>
  </Properties></Object>
  <Object name="Helix"><Properties>
    <Property name="Radius" type="App::PropertyLength"><Float value="15"/></Property>
    <Property name="Pitch" type="App::PropertyLength"><Float value="16.67"/></Property>
    <Property name="Height" type="App::PropertyLength"><Float value="50"/></Property>
    <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0.5" Q1="0.5" Q2="0.5" Q3="0.5"/></Property>
    <Property name="Shape" type="Part::PropertyPartShape"><Part file="Helix.brp"/></Property>
  </Properties></Object>
  <Object name="BaseFeature"><Properties>
    <Property name="BaseFeature" type="App::PropertyLink"><Link value="Helix"/></Property>
    <Property name="Shape" type="Part::PropertyPartShape"><Part file="Base.brp"/></Property>
  </Properties></Object>
  <Object name="Sketch"><Properties>
    <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1">
      <Geometry type="Part::GeomCircle"><Construction value="0"/><Circle CenterX="15" CenterY="0" CenterZ="0" NormalX="0" NormalY="0" NormalZ="1" Radius="1.5"/></Geometry>
    </GeometryList></Property>
    <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0.707106781187" Q1="0" Q2="0" Q3="0.707106781187"/></Property>
  </Properties></Object>
  <Object name="AdditivePipe"><Properties>
    <Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="0"></LinkSub></Property>
    <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="BaseFeature" count="1"><Sub value="Edge1"/></LinkSub></Property>
    <Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  </Properties></Object>
</ObjectData>
</Document>"#;
        // The base feature's stored helix is about world Z (its own placement is the identity).
        let mut z_helix = |_: &str| -> Result<Edges, String> {
            Ok(Edges::from(vec![SrcEdge { mid: [0.0, 15.0, 25.0], ..SrcEdge::default() }]))
        };
        let t = tree(doc.as_bytes(), &mut z_helix).unwrap();
        let kinds: Vec<&str> = t.features.iter().filter_map(|f| f["type"].as_str()).collect();
        assert_eq!(kinds, ["sketch", "sweep"]);
        let s = &t.features[1];
        assert_eq!((s["operation"].as_str(), s["name"].as_str()), (Some("new"), Some("AdditivePipe")));
        assert_eq!(s["helix"]["axis"], serde_json::json!([0.0, 0.0, 1.0]));
        assert_eq!(s["helix"]["origin"], serde_json::json!([0.0, 0.0, 0.0]));
        // A fixed profile does not follow a helix: the tree refuses it later; the mode is carried.
        let fixed = doc.replace(r#"<Integer value="0"/>"#, r#"<Integer value="1"/>"#);
        let t = tree(fixed.as_bytes(), &mut z_helix).unwrap();
        assert_eq!(t.features[1]["orientation"], "fixed");
    }

    #[test]
    fn a_symmetric_revolution_is_two_half_turns() {
        let doc = DOC
            .replace(
                r#"<Object type="PartDesign::Pocket" name="Pocket"/>
</Objects>"#,
                r#"<Object type="PartDesign::Revolution" name="Revolution"/>
</Objects>"#,
            )
            .replace(r#"<Link value="Pad"/><Link value="Pocket"/>"#, r#"<Link value="Revolution"/>"#)
            .replace(
                r#"  <Object name="Pocket"><Properties>"#,
                r#"  <Object name="Revolution"><Properties>
    <Property name="Angle" type="App::PropertyAngle"><Float value="120"/></Property>
    <Property name="Midplane" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="0"></LinkSub></Property>
    <Property name="ReferenceAxis" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="1"><Sub value="H_Axis"/></LinkSub></Property>
  </Properties></Object>
  <Object name="Unused"><Properties>"#,
            );
        let t = tree(doc.as_bytes(), &mut no_shapes).unwrap();
        let turns: Vec<&Value> = t.features.iter().filter(|f| f["type"] == "revolve").collect();
        assert_eq!(turns.len(), 2, "{turns:?}");
        assert_eq!((turns[0]["angle"].as_f64(), turns[1]["angle"].as_f64()), (Some(60.0), Some(60.0)));
        assert_eq!((turns[0]["operation"].as_str(), turns[1]["operation"].as_str()), (Some("new"), Some("join")));
        assert_eq!(turns[1]["target"], "Revolution");
        assert_ne!(turns[0]["axis"], turns[1]["axis"], "the second half turns the other way");
    }

    #[test]
    fn an_arc_of_a_whole_turn_is_a_circle() {
        let doc = DOC.replace(
            r#"<Geometry type="Part::GeomCircle"><Construction value="0"/><Circle CenterX="5" CenterY="2.5" CenterZ="0" NormalX="0" NormalY="0" NormalZ="1" Radius="1"/></Geometry>"#,
            r#"<Geometry type="Part::GeomArcOfCircle"><Construction value="0"/><ArcOfCircle CenterX="5" CenterY="2.5" CenterZ="0" NormalX="0" NormalY="0" NormalZ="1" Radius="1" StartAngle="0" EndAngle="6.283185307179586"/></Geometry>"#,
        );
        let t = tree(doc.as_bytes(), &mut no_shapes).unwrap();
        let curves = t.features[0]["curves"].as_array().unwrap();
        assert!(curves.iter().any(|c| c["type"] == "circle" && c["radius"] == 1.0), "{curves:?}");
        assert!(curves.iter().all(|c| c["type"] != "arc"));
    }

    #[test]
    fn ellipses_and_their_arcs_become_tree_ellipses() {
        let circle = r#"<Geometry type="Part::GeomCircle"><Construction value="0"/><Circle CenterX="5" CenterY="2.5" CenterZ="0" NormalX="0" NormalY="0" NormalZ="1" Radius="1"/></Geometry>"#;
        let make = |g: &str| tree(DOC.replace(circle, g).as_bytes(), &mut no_shapes);
        let whole = make(r#"<Geometry type="Part::GeomEllipse"><Construction value="0"/><Ellipse CenterX="5" CenterY="2.5" CenterZ="0" NormalX="0" NormalY="0" NormalZ="1" MajorRadius="3" MinorRadius="1" AngleXU="0.5"/></Geometry>"#).unwrap();
        let c = whole.features[0]["curves"].as_array().unwrap().iter().find(|c| c["type"] == "ellipse").unwrap();
        assert_eq!(
            (c["type"].as_str(), c["a"].as_f64(), c["b"].as_f64()),
            (Some("ellipse"), Some(3.0), Some(1.0)),
            "{c}"
        );
        assert!((c["major"][0].as_f64().unwrap() - libm::cos(0.5)).abs() < 1e-12);
        // A quarter arc from the major to the minor end, about a major axis along y.
        let arc = make(r#"<Geometry type="Part::GeomArcOfEllipse"><Construction value="0"/><ArcOfEllipse CenterX="0" CenterY="0" CenterZ="0" NormalX="0" NormalY="0" NormalZ="1" MajorRadius="3" MinorRadius="1" AngleXU="1.5707963267948966" StartAngle="0" EndAngle="1.5707963267948966"/></Geometry>"#).unwrap();
        let f = &arc.features[0];
        let q = f["curves"].as_array().unwrap().iter().find(|c| c["type"] == "ellipse_arc").unwrap();
        let at = |id: &str| f["points"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
        let (s, e) = (at(q["start"].as_str().unwrap()), at(q["end"].as_str().unwrap()));
        assert!(s["x"].as_f64().unwrap().abs() < 1e-9 && (s["y"].as_f64().unwrap() - 3.0).abs() < 1e-9, "{s}");
        assert!((e["x"].as_f64().unwrap() + 1.0).abs() < 1e-9 && e["y"].as_f64().unwrap().abs() < 1e-9, "{e}");
        // A mirrored plane is refused.
        let mirrored = make(
            r#"<Geometry type="Part::GeomEllipse"><Construction value="0"/><Ellipse CenterX="0" CenterY="0" CenterZ="0" NormalX="0" NormalY="0" NormalZ="-1" MajorRadius="3" MinorRadius="1" AngleXU="0"/></Geometry>"#,
        );
        assert!(mirrored.is_err());
    }

    #[test]
    fn unequal_chamfers_carry_their_second_size_or_angle() {
        let make = |ty: u8| {
            let doc = DOC
                .replace(
                    r#"<Object type="PartDesign::Pocket" name="Pocket"/>
</Objects>"#,
                    r#"<Object type="PartDesign::Pocket" name="Pocket"/>
  <Object type="PartDesign::Chamfer" name="Chamfer"/>
</Objects>"#,
                )
                .replace(r#"<Link value="Pocket"/></LinkList>"#, r#"<Link value="Pocket"/><Link value="Chamfer"/></LinkList>"#)
                .replace(
                    "</ObjectData>",
                    &format!(
                        r#"  <Object name="Chamfer"><Properties>
    <Property name="Base" type="App::PropertyLinkSub"><LinkSub value="Pocket" count="1"><Sub value="Edge1"/></LinkSub></Property>
    <Property name="ChamferType" type="App::PropertyEnumeration"><Integer value="{ty}"/></Property>
    <Property name="Size" type="App::PropertyLength"><Float value="2"/></Property>
    <Property name="Size2" type="App::PropertyLength"><Float value="3"/></Property>
    <Property name="Angle" type="App::PropertyAngle"><Float value="30"/></Property>
  </Properties></Object>
</ObjectData>"#
                    ),
                )
                .replace(
                    r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#,
                    r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>
    <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape2.brp"/></Property>"#,
                );
            let mut edges = |_: &str| -> Result<Edges, String> {
                Ok(vec![SrcEdge {
                    mid: [0.0; 3],
                    faces: vec![0, 1],
                    path: vec![[0.0; 3], [1.0, 0.0, 0.0]],
                    smooth: false,
                }]
                .into())
            };
            tree(doc.as_bytes(), &mut edges).unwrap().features.last().unwrap().clone()
        };
        assert!(make(0).get("distance2").is_none());
        assert_eq!(make(1)["distance2"], 3.0);
        assert_eq!(make(2)["angle"], 30.0);
    }

    #[test]
    fn an_edge_inside_one_cylinder_or_plane_is_smooth() {
        use gusset_kernel::{Axis3, CurveKind, FaceInfo, SurfaceKind};
        let cyl = |nx: f64, ny: f64| FaceInfo {
            kind: SurfaceKind::Cylinder,
            area: 1.0,
            centroid: [0.0; 3],
            normal: [nx, ny, 0.0],
            axis: Some(Axis3 { origin: [0.0; 3], dir: [0.0, 0.0, 1.0] }),
        };
        // Two halves of one cylinder meet along a line.
        assert!(is_smooth_edge(CurveKind::Line, [5.0, 0.0, 3.0], &cyl(1.0, 0.0), &cyl(-1.0, 0.0)));
        // A circle between two cylinders is not a seam line.
        assert!(!is_smooth_edge(CurveKind::Circle, [5.0, 0.0, 3.0], &cyl(1.0, 0.0), &cyl(-1.0, 0.0)));
        // Cylinders about different axes are different surfaces.
        let mut other = cyl(-1.0, 0.0);
        other.axis = Some(Axis3 { origin: [1.0, 0.0, 0.0], dir: [0.0, 0.0, 1.0] });
        assert!(!is_smooth_edge(CurveKind::Line, [5.0, 0.0, 3.0], &cyl(1.0, 0.0), &other));
        let plane = |z: f64| FaceInfo {
            kind: SurfaceKind::Plane,
            area: 1.0,
            centroid: [0.0, 0.0, z],
            normal: [0.0, 0.0, 1.0],
            axis: None,
        };
        assert!(is_smooth_edge(CurveKind::Line, [0.0, 0.0, 0.0], &plane(0.0), &plane(0.0)));
        assert!(!is_smooth_edge(CurveKind::Line, [0.0, 0.0, 0.0], &plane(0.0), &plane(1.0)));
    }
}
