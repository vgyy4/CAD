//! 3MF (core specification) reader and deterministic writer.
//!
//! Reader: OPC package via [`crate::zip`], root model part from `_rels/.rels`, objects
//! with meshes or components (including Production-extension `p:path` references to
//! other model parts), build items with transforms, and unit conversion to millimetres.
//! Materials, colours, beam lattices, slices and other extensions are ignored.
//!
//! Writer: one object per body (indexed, exactly welded), one build item per object,
//! `unit="millimeter"`, fixed ZIP layout and timestamps.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write;

use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use crate::error::{Result, limit, malformed, non_finite};
use crate::utils::{self, Tri};
use crate::zip::{Archive, check_entry_name, write_zip};
use crate::{
    Cancel, ExportBody, ExportReport, Format, ImportOptions, ImportedMesh, Limits, MeshIoError, Soup, clean_name,
    finish,
};

const F: Format = Format::ThreeMf;
const NS_CORE: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
const NS_PRODUCTION: &str = "http://schemas.microsoft.com/3dmanufacturing/production/2015/06";
const REL_MODEL: &str = "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";
const DEFAULT_MODEL: &str = "3D/3dmodel.model";

// ---------------------------------------------------------------------------------------
// transforms: 3MF uses row vectors, p' = p × M, M given as 12 numbers
// "m00 m01 m02 m10 m11 m12 m20 m21 m22 m30 m31 m32" (row 3 = translation).

type Mat = [[f64; 3]; 4];
const IDENTITY: Mat = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0]];

fn apply(m: &Mat, p: [f64; 3]) -> [f64; 3] {
    let mut o = m[3];
    for (k, row) in m.iter().take(3).enumerate() {
        let s = p.get(k).copied().unwrap_or(0.0);
        o = [o[0] + s * row[0], o[1] + s * row[1], o[2] + s * row[2]];
    }
    o
}

/// `a` then `b`: p × A × B.
fn compose(a: &Mat, b: &Mat) -> Mat {
    let lin = |r: [f64; 3]| -> [f64; 3] {
        let mut o = [0.0; 3];
        for (k, row) in b.iter().take(3).enumerate() {
            let s = r.get(k).copied().unwrap_or(0.0);
            o = [o[0] + s * row[0], o[1] + s * row[1], o[2] + s * row[2]];
        }
        o
    };
    [lin(a[0]), lin(a[1]), lin(a[2]), apply(b, a[3])]
}

fn det(m: &Mat) -> f64 {
    let [a, b, c] = [m[0], m[1], m[2]];
    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])
}

fn scale_mat(s: f64) -> Mat {
    [[s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s], [0.0, 0.0, 0.0]]
}

fn parse_transform(s: &str, at: &dyn Fn() -> String) -> Result<Mat> {
    let mut v = [0.0f64; 12];
    let mut it = s.split_ascii_whitespace();
    for slot in v.iter_mut() {
        let t = it.next().ok_or_else(|| malformed(F, at(), "transform needs 12 numbers"))?;
        let x: f64 = t.parse().map_err(|_| malformed(F, at(), format!("bad transform number {t:?}")))?;
        if !x.is_finite() {
            return Err(non_finite(F, format!("{} transform", at()), x));
        }
        *slot = x;
    }
    if it.next().is_some() {
        return Err(malformed(F, at(), "transform has more than 12 numbers"));
    }
    Ok([[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]], [v[9], v[10], v[11]]])
}

fn unit_scale(unit: &str) -> Option<f64> {
    Some(match unit {
        "micron" => 0.001,
        "millimeter" => 1.0,
        "centimeter" => 10.0,
        "inch" => 25.4,
        "foot" => 304.8,
        "meter" => 1000.0,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------
// model data

#[derive(Debug, Clone)]
struct Component {
    objectid: u32,
    path: Option<String>,
    transform: Mat,
}

#[derive(Debug, Default)]
struct Object {
    name: Option<String>,
    kind: Option<String>,
    vertices: Vec<[f64; 3]>,
    tris: Vec<Tri>,
    components: Vec<Component>,
}

#[derive(Debug, Clone)]
struct Item {
    objectid: u32,
    path: Option<String>,
    transform: Mat,
}

#[derive(Debug, Default)]
struct Model {
    unit: String,
    scale: f64,
    objects: HashMap<u32, Object>,
    items: Vec<Item>,
    /// Namespaces of required extensions.
    required: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ctx {
    Model,
    Resources,
    Object,
    Mesh,
    Vertices,
    Triangles,
    Components,
    Build,
    Other,
}

/// Running totals across all parts of a package (limits apply to the sum).
#[derive(Debug, Default)]
struct Totals {
    vertices: u64,
    triangles: u64,
    objects: u64,
}

fn for_each_attr(
    e: &BytesStart<'_>,
    lim: &Limits,
    at: &dyn Fn() -> String,
    mut f: impl FnMut(&str, &str, &str) -> Result<()>,
) -> Result<()> {
    let mut n = 0u32;
    for a in e.attributes() {
        let a = a.map_err(|err| malformed(F, at(), format!("bad XML attribute: {err}")))?;
        n += 1;
        if n > lim.max_xml_attributes {
            return Err(limit("XML attributes on one element", u64::from(n), u64::from(lim.max_xml_attributes)));
        }
        if a.value.len() as u64 > lim.max_attribute_bytes {
            return Err(limit("XML attribute length (bytes)", a.value.len() as u64, lim.max_attribute_bytes));
        }
        let v = a
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|err| malformed(F, at(), format!("bad XML attribute value: {err}")))?;
        let full: &str = a.key.as_ref();
        let local = a.key.local_name();
        f(full, local.as_ref(), &v)?;
    }
    Ok(())
}

fn xml_reader(xml: &[u8]) -> Reader<&[u8]> {
    let mut r = Reader::from_reader(xml);
    r.config_mut().check_end_names = true;
    r
}

fn xml_err(part: &str, pos: u64, err: impl std::fmt::Display) -> MeshIoError {
    malformed(F, format!("part {part:?} byte {pos}"), format!("XML error: {err}"))
}

fn parse_u32(v: &str, at: &dyn Fn() -> String, what: &str) -> Result<u32> {
    v.trim().parse::<u32>().map_err(|_| malformed(F, at(), format!("{what} {v:?} is not a valid index")))
}

fn parse_model(xml: &[u8], part: &str, lim: &Limits, cancel: Cancel<'_>, totals: &mut Totals) -> Result<Model> {
    let mut r = xml_reader(xml);
    let mut stack: Vec<Ctx> = Vec::new();
    let mut model = Model { unit: "millimeter".into(), scale: 1.0, ..Default::default() };
    let mut cur: Option<(u32, Object)> = None;
    let mut saw_model = false;
    let mut events = 0usize;
    let mut xmlns: Vec<(String, String)> = Vec::new();
    let mut required_prefixes: Vec<String> = Vec::new();

    loop {
        let pos = r.buffer_position();
        let ev = r.read_event().map_err(|e| xml_err(part, r.error_position(), e))?;
        events += 1;
        cancel.tick(events)?;
        let (e, is_start) = match ev {
            Event::Start(e) => (e, true),
            Event::Empty(e) => (e, false),
            Event::End(_) => {
                if stack.pop() == Some(Ctx::Object) {
                    if let Some((id, obj)) = cur.take() {
                        finish_object(&mut model, id, obj, part)?;
                    }
                }
                continue;
            }
            Event::DocType(_) => {
                return Err(malformed(F, format!("part {part:?}"), "DOCTYPE declarations are not allowed in 3MF"));
            }
            Event::Eof => break,
            _ => continue,
        };
        if is_start && stack.len() as u32 >= lim.max_xml_depth {
            return Err(limit("XML nesting depth", stack.len() as u64 + 1, u64::from(lim.max_xml_depth)));
        }
        let at = || format!("part {part:?} byte {pos}");
        let local = e.local_name();
        let name: &str = local.as_ref();
        let parent = stack.last().copied();
        let ctx = match (parent, name) {
            (None, "model") => {
                saw_model = true;
                for_each_attr(&e, lim, &at, |full, key, v| {
                    if let Some(prefix) = full.strip_prefix("xmlns:") {
                        xmlns.push((prefix.to_string(), v.to_string()));
                    } else if full == "xmlns" {
                        if v != NS_CORE {
                            return Err(malformed(F, at(), format!("unknown 3MF core namespace {v:?}")));
                        }
                    } else if key == "unit" {
                        model.scale = unit_scale(v).ok_or_else(|| malformed(F, at(), format!("unknown unit {v:?}")))?;
                        model.unit = v.to_string();
                    } else if key == "requiredextensions" {
                        required_prefixes = v.split_ascii_whitespace().map(str::to_string).collect();
                    }
                    Ok(())
                })?;
                Ctx::Model
            }
            (None, other) => {
                return Err(malformed(F, at(), format!("root element is <{other}>, expected <model>")));
            }
            (Some(Ctx::Model), "resources") => Ctx::Resources,
            (Some(Ctx::Model), "build") => Ctx::Build,
            (Some(Ctx::Resources), "object") => {
                totals.objects += 1;
                if totals.objects > lim.max_objects {
                    return Err(limit("3MF object count", totals.objects, lim.max_objects));
                }
                let mut id = None;
                let mut obj = Object::default();
                for_each_attr(&e, lim, &at, |_, key, v| {
                    match key {
                        "id" => id = Some(parse_u32(v, &at, "object id")?),
                        "name" => obj.name = Some(clean_name(v, lim.max_name_bytes)),
                        "type" => obj.kind = Some(v.to_string()),
                        _ => {}
                    }
                    Ok(())
                })?;
                let id = id.ok_or_else(|| malformed(F, at(), "object without an id"))?;
                if is_start {
                    cur = Some((id, obj));
                } else {
                    finish_object(&mut model, id, obj, part)?;
                }
                Ctx::Object
            }
            (Some(Ctx::Object), "mesh") => Ctx::Mesh,
            (Some(Ctx::Mesh), "vertices") => Ctx::Vertices,
            (Some(Ctx::Mesh), "triangles") => Ctx::Triangles,
            (Some(Ctx::Object), "components") => Ctx::Components,
            (Some(Ctx::Vertices), "vertex") => {
                let Some((id, obj)) = cur.as_mut() else { return Err(malformed(F, at(), "vertex outside an object")) };
                totals.vertices += 1;
                if totals.vertices > lim.max_vertices {
                    return Err(limit("vertex count", totals.vertices, lim.max_vertices));
                }
                let mut p = [f64::NAN; 3];
                let n = obj.vertices.len() + 1;
                for_each_attr(&e, lim, &at, |_, key, v| {
                    let k = match key {
                        "x" => 0,
                        "y" => 1,
                        "z" => 2,
                        _ => return Ok(()),
                    };
                    let x: f64 = v
                        .trim()
                        .parse()
                        .map_err(|_| malformed(F, at(), format!("object {id} vertex {n}: bad number {v:?}")))?;
                    if !x.is_finite() {
                        return Err(non_finite(F, format!("object {id} vertex {n}"), x));
                    }
                    if let Some(slot) = p.get_mut(k) {
                        *slot = x;
                    }
                    Ok(())
                })?;
                if p.iter().any(|c| c.is_nan()) {
                    return Err(malformed(F, at(), format!("object {id} vertex {n} lacks x, y or z")));
                }
                obj.vertices.push(p);
                Ctx::Other
            }
            (Some(Ctx::Triangles), "triangle") => {
                let Some((id, obj)) = cur.as_mut() else {
                    return Err(malformed(F, at(), "triangle outside an object"));
                };
                totals.triangles += 1;
                if totals.triangles > lim.max_triangles {
                    return Err(limit("triangle count", totals.triangles, lim.max_triangles));
                }
                let mut t = [u32::MAX; 3];
                let n = obj.tris.len() + 1;
                for_each_attr(&e, lim, &at, |_, key, v| {
                    let k = match key {
                        "v1" => 0,
                        "v2" => 1,
                        "v3" => 2,
                        _ => return Ok(()),
                    };
                    if let Some(slot) = t.get_mut(k) {
                        *slot = parse_u32(v, &|| format!("object {id} triangle {n}"), "vertex index")?;
                    }
                    Ok(())
                })?;
                if t.contains(&u32::MAX) {
                    return Err(malformed(F, at(), format!("object {id} triangle {n} lacks v1, v2 or v3")));
                }
                obj.tris.push(t);
                Ctx::Other
            }
            (Some(Ctx::Components), "component") => {
                let Some((_, obj)) = cur.as_mut() else {
                    return Err(malformed(F, at(), "component outside an object"));
                };
                totals.objects += 1;
                if totals.objects > lim.max_objects {
                    return Err(limit("3MF object/component count", totals.objects, lim.max_objects));
                }
                let mut c = Component { objectid: u32::MAX, path: None, transform: IDENTITY };
                for_each_attr(&e, lim, &at, |_, key, v| {
                    match key {
                        "objectid" => c.objectid = parse_u32(v, &at, "component objectid")?,
                        "transform" => c.transform = parse_transform(v, &at)?,
                        "path" => c.path = Some(v.to_string()),
                        _ => {}
                    }
                    Ok(())
                })?;
                if c.objectid == u32::MAX {
                    return Err(malformed(F, at(), "component without objectid"));
                }
                obj.components.push(c);
                Ctx::Other
            }
            (Some(Ctx::Build), "item") => {
                if model.items.len() as u64 >= lim.max_objects {
                    return Err(limit("3MF build item count", model.items.len() as u64 + 1, lim.max_objects));
                }
                let mut it = Item { objectid: u32::MAX, path: None, transform: IDENTITY };
                for_each_attr(&e, lim, &at, |_, key, v| {
                    match key {
                        "objectid" => it.objectid = parse_u32(v, &at, "item objectid")?,
                        "transform" => it.transform = parse_transform(v, &at)?,
                        "path" => it.path = Some(v.to_string()),
                        _ => {}
                    }
                    Ok(())
                })?;
                if it.objectid == u32::MAX {
                    return Err(malformed(F, at(), "build item without objectid"));
                }
                model.items.push(it);
                Ctx::Other
            }
            _ => Ctx::Other,
        };
        if is_start {
            stack.push(ctx);
        }
    }
    if !saw_model {
        return Err(malformed(F, format!("part {part:?}"), "no <model> element"));
    }
    if !stack.is_empty() {
        return Err(malformed(F, format!("part {part:?}"), "XML ends before all elements are closed"));
    }
    for p in required_prefixes {
        let ns = xmlns.iter().find(|(k, _)| *k == p).map_or(p.clone(), |(_, v)| v.clone());
        model.required.push(ns);
    }
    Ok(model)
}

fn finish_object(model: &mut Model, id: u32, obj: Object, part: &str) -> Result<()> {
    let nv = obj.vertices.len();
    if let Some((k, t)) = obj.tris.iter().enumerate().find(|(_, t)| t.iter().any(|&v| v as usize >= nv)) {
        let bad = t.iter().find(|&&v| v as usize >= nv).copied().unwrap_or(0);
        return Err(malformed(
            F,
            format!("part {part:?} object {id} triangle {}", k + 1),
            format!("references vertex {bad} but the object has {nv} vertices"),
        ));
    }
    if model.objects.insert(id, obj).is_some() {
        return Err(malformed(F, format!("part {part:?}"), format!("duplicate object id {id}")));
    }
    Ok(())
}

/// Finds the root model part from `_rels/.rels`.
fn root_part(ar: &Archive<'_>, budget: &mut u64, lim: &Limits, warnings: &mut Vec<String>) -> Result<String> {
    let Some(e) = ar.find("_rels/.rels") else {
        warnings.push(format!("3MF package has no _rels/.rels; using {DEFAULT_MODEL}"));
        return Ok(DEFAULT_MODEL.to_string());
    };
    let xml = ar.read(&e.clone(), budget, lim)?;
    let mut r = xml_reader(&xml);
    let mut depth = 0u32;
    let mut found: Option<String> = None;
    loop {
        let ev = r.read_event().map_err(|err| xml_err("_rels/.rels", r.error_position(), err))?;
        let (e, is_start) = match ev {
            Event::Start(e) => (e, true),
            Event::Empty(e) => (e, false),
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                continue;
            }
            Event::DocType(_) => {
                return Err(malformed(F, "part \"_rels/.rels\"", "DOCTYPE declarations are not allowed"));
            }
            Event::Eof => break,
            _ => continue,
        };
        if is_start {
            depth += 1;
            if depth > lim.max_xml_depth {
                return Err(limit("XML nesting depth", u64::from(depth), u64::from(lim.max_xml_depth)));
            }
        }
        if e.local_name().as_ref() != "Relationship" || found.is_some() {
            continue;
        }
        let (mut target, mut ty, mut external) = (None, None, false);
        for_each_attr(&e, lim, &|| "part \"_rels/.rels\"".to_string(), |_, key, v| {
            match key {
                "Target" => target = Some(v.to_string()),
                "Type" => ty = Some(v.to_string()),
                "TargetMode" => external = v.eq_ignore_ascii_case("External"),
                _ => {}
            }
            Ok(())
        })?;
        if ty.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(REL_MODEL)) {
            if external {
                return Err(MeshIoError::UnsafePath { path: target.unwrap_or_default() });
            }
            found = target;
        }
    }
    match found {
        Some(t) => resolve_part("", &t),
        None => {
            warnings.push(format!("3MF package has no 3D model relationship; using {DEFAULT_MODEL}"));
            Ok(DEFAULT_MODEL.to_string())
        }
    }
}

fn percent_decode(s: &str) -> Result<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%' {
            let hex = b
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| MeshIoError::UnsafePath { path: s.to_string() })?;
            out.push(hex);
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| MeshIoError::UnsafePath { path: s.to_string() })
}

/// Resolves an OPC part reference (`/3D/x.model` is package-absolute; anything else is
/// relative to `from`'s folder) to a ZIP entry name, rejecting `..`, drive letters,
/// URLs and other unsafe forms.
fn resolve_part(from: &str, target: &str) -> Result<String> {
    let t = percent_decode(target.trim())?;
    let name = match t.strip_prefix('/') {
        Some(rest) => rest.to_string(),
        None => match from.rfind('/') {
            Some(i) => format!("{}/{t}", from.get(..i).unwrap_or("")),
            None => t.clone(),
        },
    };
    check_entry_name(&name).map_err(|_| MeshIoError::UnsafePath { path: target.chars().take(200).collect() })?;
    Ok(name)
}

struct Loader<'a, 'o> {
    ar: Archive<'a>,
    budget: u64,
    opts: &'o ImportOptions,
    cancel: Cancel<'o>,
    totals: Totals,
    parts: HashMap<String, Model>,
    root_unit: f64,
    /// Triangles produced so far (after instancing).
    produced: u64,
    /// Objects visited while flattening (bounds component fan-out).
    visits: u64,
}

/// Per-build-item traversal state.
#[derive(Debug, Default)]
struct Walk {
    stack: Vec<(String, u32)>,
    soup: Soup,
}

impl Loader<'_, '_> {
    fn ensure(&mut self, part: &str, warnings: &mut Vec<String>) -> Result<()> {
        let key = part.to_ascii_lowercase();
        if self.parts.contains_key(&key) {
            return Ok(());
        }
        let lim = &self.opts.limits;
        let entry = self
            .ar
            .find(part)
            .cloned()
            .ok_or_else(|| malformed(F, format!("part {part:?}"), "model part is missing from the package"))?;
        let xml = self.ar.read(&entry, &mut self.budget, lim)?;
        let model = parse_model(&xml, part, lim, self.cancel, &mut self.totals)?;
        if !self.parts.is_empty() && (model.scale - self.root_unit).abs() > 1e-12 {
            warnings.push(format!("3MF part {part:?} declares unit {:?}; the root unit is used", model.unit));
        }
        self.parts.insert(key, model);
        Ok(())
    }

    fn collect(
        &mut self,
        part: &str,
        id: u32,
        m: &Mat,
        depth: u32,
        walk: &mut Walk,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        let lim = &self.opts.limits;
        self.visits += 1;
        let max_visits = lim.max_objects.saturating_mul(10);
        if self.visits > max_visits {
            return Err(limit("3MF object instances (components × items)", self.visits, max_visits));
        }
        if depth > lim.max_component_depth {
            return Err(limit("3MF component nesting depth", u64::from(depth), u64::from(lim.max_component_depth)));
        }
        let key = part.to_ascii_lowercase();
        if walk.stack.iter().any(|(p, i)| *p == key && *i == id) {
            return Err(malformed(F, format!("part {part:?} object {id}"), "components form a cycle"));
        }
        self.cancel.check()?;
        self.ensure(part, warnings)?;
        let obj = self
            .parts
            .get(&key)
            .and_then(|m| m.objects.get(&id))
            .ok_or_else(|| malformed(F, format!("part {part:?}"), format!("reference to missing object {id}")))?;

        if !obj.tris.is_empty() {
            self.produced += obj.tris.len() as u64;
            if self.produced > lim.max_triangles {
                return Err(limit("triangle count after instancing", self.produced, lim.max_triangles));
            }
            let soup = &mut walk.soup;
            let base = soup.positions.len() as u32;
            let start = soup.tris.len() as u32;
            for &v in &obj.vertices {
                let p = apply(m, v);
                let p32 = [p[0] as f32, p[1] as f32, p[2] as f32];
                if let Some(c) = p32.iter().find(|c| !c.is_finite()) {
                    return Err(non_finite(F, format!("part {part:?} object {id} (after transform)"), f64::from(*c)));
                }
                soup.positions.push(p32);
            }
            let flip = det(m) < 0.0;
            for &[a, b, c] in &obj.tris {
                soup.tris.push(if flip { [base + a, base + c, base + b] } else { [base + a, base + b, base + c] });
            }
            soup.ranges.push(start..soup.tris.len() as u32);
        }
        let comps = obj.components.clone();
        walk.stack.push((key, id));
        for c in comps {
            let target = match &c.path {
                Some(p) => resolve_part(part, p)?,
                None => part.to_string(),
            };
            let cm = compose(&c.transform, m);
            self.collect(&target, c.objectid, &cm, depth + 1, walk, warnings)?;
        }
        walk.stack.pop();
        Ok(())
    }
}

pub(crate) fn import(
    b: &[u8],
    opts: &ImportOptions,
    cancel: Cancel<'_>,
    warnings: &mut Vec<String>,
) -> Result<Vec<ImportedMesh>> {
    let lim = &opts.limits;
    let ar = Archive::parse(b, lim)?;
    let mut budget = lim.max_decompressed_bytes;
    if ar.find("[Content_Types].xml").is_none() {
        warnings.push("3MF package has no [Content_Types].xml".to_string());
    }
    let root = root_part(&ar, &mut budget, lim, warnings)?;
    let mut ld = Loader {
        ar,
        budget,
        opts,
        cancel,
        totals: Totals::default(),
        parts: HashMap::new(),
        root_unit: 1.0,
        produced: 0,
        visits: 0,
    };
    ld.ensure(&root, warnings)?;
    let root_key = root.to_ascii_lowercase();
    let (items, scale, unit, required) = match ld.parts.get(&root_key) {
        Some(m) => (m.items.clone(), m.scale, m.unit.clone(), m.required.clone()),
        None => return Err(malformed(F, format!("part {root:?}"), "root model part could not be loaded")),
    };
    ld.root_unit = scale;
    let unsupported: Vec<&String> = required.iter().filter(|ns| ns.as_str() != NS_PRODUCTION).collect();
    if !unsupported.is_empty() {
        warnings.push(format!(
            "3MF requires extensions this importer does not implement ({}); only core geometry was imported",
            unsupported.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    if items.is_empty() {
        warnings.push("3MF build has no items; nothing is placed on the build plate".to_string());
    }
    let units_note = if unit == "millimeter" {
        "3MF unit 'millimeter'".to_string()
    } else {
        format!("3MF unit '{unit}' converted to mm (×{scale})")
    };
    let unit_m = scale_mat(scale);
    let mut seen_names: HashMap<String, u32> = HashMap::new();
    let mut odd_types: Vec<String> = Vec::new();
    let mut out = Vec::with_capacity(items.len());
    for it in items {
        let part = match &it.path {
            Some(p) => resolve_part(&root, p)?,
            None => root.clone(),
        };
        let m = compose(&it.transform, &unit_m);
        let mut walk = Walk::default();
        ld.collect(&part, it.objectid, &m, 0, &mut walk, warnings)?;
        let soup = walk.soup;
        let obj = ld.parts.get(&part.to_ascii_lowercase()).and_then(|m| m.objects.get(&it.objectid));
        if let Some(k) = obj.and_then(|o| o.kind.clone()).filter(|k| k != "model") {
            if !odd_types.contains(&k) {
                odd_types.push(k);
            }
        }
        let base = obj
            .and_then(|o| o.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("Object {}", it.objectid));
        let count = seen_names.entry(base.clone()).or_insert(0);
        *count += 1;
        let name = if *count > 1 { format!("{base} ({count})") } else { base };
        out.push(finish(name, soup, &units_note, opts, warnings));
    }
    for k in odd_types {
        warnings.push(format!("3MF build contains objects of type {k:?}; imported as ordinary meshes"));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------
// writer

fn xml_escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
}

const CONTENT_TYPES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    "\n",
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
    r#"<Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>"#,
    "</Types>\n"
);

const RELS: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    "\n",
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    r#"<Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>"#,
    "</Relationships>\n"
);

pub(crate) fn write(bodies: &[ExportBody<'_>], out: &mut dyn Write) -> Result<ExportReport> {
    let mut report = ExportReport::default();
    let mut s = String::with_capacity(1 << 16);
    s.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    s.push('\n');
    let _ = writeln!(s, r#"<model unit="millimeter" xml:lang="en-US" xmlns="{NS_CORE}">"#);
    s.push_str(" <metadata name=\"Application\">Gusset</metadata>\n <resources>\n");
    let mut ids = Vec::new();
    for (k, b) in bodies.iter().enumerate() {
        let (positions, remap) = utils::weld_exact(&b.mesh.positions);
        let mut tris: Vec<Tri> =
            utils::triangles(b.mesh).map(|t| t.map(|i| remap.get(i as usize).copied().unwrap_or(0))).collect();
        let before = tris.len();
        // 3MF forbids triangles that reuse a vertex.
        tris.retain(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
        let dropped = before - tris.len();
        if dropped > 0 {
            report.warnings.push(format!("{:?}: {dropped} collapsed triangles were not written", b.name));
        }
        if tris.is_empty() {
            report.warnings.push(format!("{:?}: no triangles, body skipped", b.name));
            continue;
        }
        let positions = utils::compact(&positions, &mut tris);
        let id = k + 1;
        ids.push(id);
        let _ = write!(s, "  <object id=\"{id}\" name=\"");
        xml_escape(b.name, &mut s);
        s.push_str("\" type=\"model\">\n   <mesh>\n    <vertices>\n");
        for p in &positions {
            let _ = writeln!(s, "     <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", p[0], p[1], p[2]);
        }
        s.push_str("    </vertices>\n    <triangles>\n");
        for t in &tris {
            let _ = writeln!(s, "     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>", t[0], t[1], t[2]);
        }
        s.push_str("    </triangles>\n   </mesh>\n  </object>\n");
        report.triangles += tris.len() as u64;
    }
    s.push_str(" </resources>\n <build>\n");
    for id in ids {
        let _ = writeln!(s, "  <item objectid=\"{id}\"/>");
    }
    s.push_str(" </build>\n</model>\n");
    write_zip(
        &[
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", RELS.as_bytes()),
            ("3D/3dmodel.model", s.as_bytes()),
        ],
        out,
    )?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_compose_and_det() {
        let t = parse_transform("1 0 0 0 1 0 0 0 1 10 20 30", &|| String::new()).unwrap();
        assert_eq!(apply(&t, [1.0, 2.0, 3.0]), [11.0, 22.0, 33.0]);
        let s = scale_mat(2.0);
        let ts = compose(&t, &s); // translate then scale
        assert_eq!(apply(&ts, [1.0, 0.0, 0.0]), [22.0, 40.0, 60.0]);
        let mirror = parse_transform("-1 0 0 0 1 0 0 0 1 0 0 0", &|| String::new()).unwrap();
        assert!(det(&mirror) < 0.0);
        assert!(parse_transform("1 2 3", &|| String::new()).is_err());
        assert!(parse_transform("1 0 0 0 1 0 0 0 1 0 0 NaN", &|| String::new()).is_err());
    }

    #[test]
    fn part_resolution() {
        assert_eq!(resolve_part("", "/3D/3dmodel.model").unwrap(), "3D/3dmodel.model");
        assert_eq!(resolve_part("3D/3dmodel.model", "/3D/Objects/a%20b.model").unwrap(), "3D/Objects/a b.model");
        assert_eq!(resolve_part("3D/3dmodel.model", "Objects/x.model").unwrap(), "3D/Objects/x.model");
        for bad in ["/../etc/passwd", "../x.model", "//server/x", "C:/x.model", "/3D/%2e%2e/x", "/a\\b"] {
            assert!(resolve_part("3D/3dmodel.model", bad).is_err(), "{bad}");
        }
    }
}
