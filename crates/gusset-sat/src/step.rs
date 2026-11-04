//! STEP AP214 text from the SAT records, in millimetres.
//!
//! Field layouts (after the attribute pointer `f[0]`), read from sample files:
//! `body`: lump, wire, transform. `lump`: next, shell, body. `shell`: next, subshell, face,
//! wire, lump. `face`: next, loop, shell, subshell, surface, sense, sidedness. `loop`: next,
//! coedge, face. `coedge`: next, previous, partner, edge, sense, loop. `edge`: start vertex,
//! start parameter, end vertex, end parameter, coedge, curve, sense. `vertex`: edge, point.
//! `point`: x y z. `transform`: a 3x3 matrix by rows (points are row vectors), translation,
//! scale. `straight-curve`: root, direction. `ellipse-curve`: centre, normal, major axis
//! (its length the major radius), radius ratio. `plane-surface`: root, normal, u direction.
//! `cone-surface`: centre, axis, major axis, ratio, an interval, then the sine and cosine of
//! the half angle: the radius at height `h` along the axis is `|major| + h sin / cos` (checked
//! on the circular edges of the samples' cones); a negative cosine turns the normal inward.

use crate::SatError;
use crate::parse::{Rec, Sat};
use std::collections::BTreeMap;
use std::fmt::Write as _;

type V3 = [f64; 3];

fn len(a: V3) -> f64 {
    libm::sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2])
}
fn unit(a: V3) -> Option<V3> {
    let l = len(a);
    (l > 1e-300).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

fn real(v: f64) -> String {
    if !v.is_finite() {
        return "0.".into();
    }
    let s = format!("{v:?}");
    let (mant, exp) = match s.split_once('e') {
        Some((m, e)) => (m.to_string(), Some(e.to_string())),
        None => (s, None),
    };
    let mant = if mant.contains('.') { mant } else { format!("{mant}.") };
    match exp {
        Some(e) => format!("{mant}E{e}"),
        None => mant,
    }
}

fn string(s: &str) -> String {
    let clean: String = s
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control())
        .map(|c| if c == '\'' || c == '\\' { ' ' } else { c })
        .take(200)
        .collect();
    format!("'{clean}'")
}

fn list(ids: &[usize]) -> String {
    let v: Vec<String> = ids.iter().map(|i| format!("#{i}")).collect();
    format!("({})", v.join(","))
}

/// Model to millimetres: `p' = s (p R) + t` scaled by the file's unit.
#[derive(Clone, Copy)]
struct Xf {
    r: [V3; 3],
    t: V3,
    s: f64,
    unit: f64,
}

impl Xf {
    fn point(&self, p: V3) -> V3 {
        let d = self.dir_raw(p);
        [
            self.unit * (self.s * d[0] + self.t[0]),
            self.unit * (self.s * d[1] + self.t[1]),
            self.unit * (self.s * d[2] + self.t[2]),
        ]
    }
    fn dir_raw(&self, d: V3) -> V3 {
        let [a, b, c] = self.r;
        [
            d[0] * a[0] + d[1] * b[0] + d[2] * c[0],
            d[0] * a[1] + d[1] * b[1] + d[2] * c[1],
            d[0] * a[2] + d[1] * b[2] + d[2] * c[2],
        ]
    }
    fn dir(&self, d: V3) -> Option<V3> {
        unit(self.dir_raw(d))
    }
    fn length(&self, l: f64) -> f64 {
        l * self.s.abs() * self.unit
    }
}

/// Most entries walked along one chain (faces of a shell, coedges of a loop, ...).
const MAX_CHAIN: usize = 10_000_000;
/// Most faces one file may produce.
const MAX_FACES: usize = 2_000_000;

struct Writer<'a> {
    sat: &'a Sat,
    out: String,
    next: usize,
    xf: Xf,
    vertices: BTreeMap<i64, usize>,
    edges: BTreeMap<i64, usize>,
    curves: BTreeMap<i64, usize>,
    surfaces: BTreeMap<i64, (usize, bool)>,
    faces: usize,
    /// Coedges per edge in the shell being written (an edge used once is a free edge).
    uses: BTreeMap<i64, u32>,
    warnings: Vec<String>,
}

impl Writer<'_> {
    fn add(&mut self, e: &str) -> usize {
        self.next += 1;
        let _ = writeln!(self.out, "#{}={e};", self.next);
        self.next
    }

    fn rec(&self, i: i64) -> Result<&Rec, SatError> {
        self.sat.rec(i).ok_or_else(|| SatError::Malformed(format!("record ${i} does not exist")))
    }

    fn rec_of(&self, i: i64, ty: &str) -> Result<&Rec, SatError> {
        let r = self.rec(i)?;
        if r.ty == ty { Ok(r) } else { Err(SatError::Malformed(format!("record ${i} is a {}, not a {ty}", r.ty))) }
    }

    fn chain(&self, first: Option<i64>, next_field: usize) -> Result<Vec<i64>, SatError> {
        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut at = first;
        while let Some(i) = at {
            if !seen.insert(i) {
                break;
            }
            if out.len() >= MAX_CHAIN {
                return Err(SatError::TooLarge("a chain of entities is too long".into()));
            }
            out.push(i);
            at = self.rec(i)?.ptr(next_field);
        }
        Ok(out)
    }

    fn point(&mut self, p: V3) -> usize {
        let p = self.xf.point(p);
        self.add(&format!("CARTESIAN_POINT('',({},{},{}))", real(p[0]), real(p[1]), real(p[2])))
    }

    fn direction(&mut self, d: V3) -> Result<usize, SatError> {
        let d = self.xf.dir(d).ok_or_else(|| SatError::Malformed("a zero direction".into()))?;
        Ok(self.add(&format!("DIRECTION('',({},{},{}))", real(d[0]), real(d[1]), real(d[2]))))
    }

    fn placement(&mut self, origin: V3, axis: V3, refd: V3) -> Result<usize, SatError> {
        let o = self.point(origin);
        let a = self.direction(axis)?;
        let r = self.direction(refd)?;
        Ok(self.add(&format!("AXIS2_PLACEMENT_3D('',#{o},#{a},#{r})")))
    }

    fn vertex(&mut self, v: i64) -> Result<usize, SatError> {
        if let Some(id) = self.vertices.get(&v) {
            return Ok(*id);
        }
        let r = self.rec_of(v, "vertex")?;
        let p = r.ptr(2).ok_or_else(|| SatError::Malformed(format!("vertex ${v} has no point")))?;
        let pr = self.rec_of(p, "point")?;
        let xyz = pr.vec3(1).ok_or_else(|| SatError::Malformed(format!("point ${p} has no coordinates")))?;
        let cp = self.point(xyz);
        let id = self.add(&format!("VERTEX_POINT('',#{cp})"));
        self.vertices.insert(v, id);
        Ok(id)
    }

    fn curve(&mut self, c: i64) -> Result<usize, SatError> {
        if let Some(id) = self.curves.get(&c) {
            return Ok(*id);
        }
        let r = self.rec(c)?.clone();
        let bad = || SatError::Malformed(format!("{} ${c} is incomplete", r.ty));
        let id = match r.ty.as_str() {
            "straight-curve" => {
                let root = r.vec3(1).ok_or_else(bad)?;
                let dir = r.vec3(4).ok_or_else(bad)?;
                let p = self.point(root);
                let d = self.direction(dir)?;
                let v = self.add(&format!("VECTOR('',#{d},1.)"));
                self.add(&format!("LINE('',#{p},#{v})"))
            }
            "ellipse-curve" => {
                let centre = r.vec3(1).ok_or_else(bad)?;
                let normal = r.vec3(4).ok_or_else(bad)?;
                let major = r.vec3(7).ok_or_else(bad)?;
                let ratio = r.num(10).unwrap_or(1.0);
                let a = self.xf.length(len(major));
                let pl = self.placement(centre, normal, major)?;
                if (ratio - 1.0).abs() < 1e-12 {
                    self.add(&format!("CIRCLE('',#{pl},{})", real(a)))
                } else {
                    self.add(&format!("ELLIPSE('',#{pl},{},{})", real(a), real(a * ratio.abs())))
                }
            }
            other => {
                return Err(SatError::Unsupported(format!(
                    "ACIS curves of type '{other}' are not read yet (only lines, circles and ellipses)"
                )));
            }
        };
        self.curves.insert(c, id);
        Ok(id)
    }

    fn edge(&mut self, e: i64) -> Result<usize, SatError> {
        if let Some(id) = self.edges.get(&e) {
            return Ok(*id);
        }
        let r = self.rec_of(e, "edge")?.clone();
        let bad = |w: &str| SatError::Malformed(format!("edge ${e} has no {w}"));
        let v0 = self.vertex(r.ptr(1).ok_or_else(|| bad("start vertex"))?)?;
        let v1 = self.vertex(r.ptr(3).ok_or_else(|| bad("end vertex"))?)?;
        let c = self.curve(r.ptr(6).ok_or_else(|| bad("curve"))?)?;
        let same = r.word(7) != Some("reversed");
        let id = self.add(&format!("EDGE_CURVE('',#{v0},#{v1},#{c},{})", if same { ".T." } else { ".F." }));
        self.edges.insert(e, id);
        Ok(id)
    }

    /// The STEP surface and whether its normal is the opposite of the ACIS surface's.
    fn surface(&mut self, s: i64) -> Result<(usize, bool), SatError> {
        if let Some(v) = self.surfaces.get(&s) {
            return Ok(*v);
        }
        let r = self.rec(s)?.clone();
        let bad = || SatError::Malformed(format!("{} ${s} is incomplete", r.ty));
        let v = match r.ty.as_str() {
            "plane-surface" => {
                let root = r.vec3(1).ok_or_else(bad)?;
                let normal = r.vec3(4).ok_or_else(bad)?;
                let udir = r.vec3(7).ok_or_else(bad)?;
                let pl = self.placement(root, normal, udir)?;
                (self.add(&format!("PLANE('',#{pl})")), false)
            }
            "cone-surface" => {
                let c = r.vec3(1).ok_or_else(bad)?;
                let n = r.vec3(4).ok_or_else(bad)?;
                let major = r.vec3(7).ok_or_else(bad)?;
                let ratio = r.num(10).ok_or_else(bad)?;
                if (ratio - 1.0).abs() > 1e-9 {
                    return Err(SatError::Unsupported("elliptical ACIS cones are not read yet".into()));
                }
                // The interval: two ends, each `I` or `F <value>`.
                let mut k = 11;
                for _ in 0..2 {
                    k += if r.word(k) == Some("F") { 2 } else { 1 };
                }
                let sin = r.num(k).ok_or_else(bad)?;
                let cos = r.num(k + 1).ok_or_else(bad)?;
                let radius = self.xf.length(len(major));
                let flipped = cos < 0.0;
                if sin.abs() < 1e-12 {
                    let pl = self.placement(c, n, major)?;
                    (self.add(&format!("CYLINDRICAL_SURFACE('',#{pl},{})", real(radius))), flipped)
                } else {
                    let slope = sin / cos;
                    let axis = if slope > 0.0 { n } else { [-n[0], -n[1], -n[2]] };
                    let pl = self.placement(c, axis, major)?;
                    let angle = libm::atan(slope.abs());
                    (self.add(&format!("CONICAL_SURFACE('',#{pl},{},{})", real(radius), real(angle))), flipped)
                }
            }
            other => {
                return Err(SatError::Unsupported(format!(
                    "ACIS surfaces of type '{other}' are not read yet (only planes, cylinders and cones)"
                )));
            }
        };
        self.surfaces.insert(s, v);
        Ok(v)
    }

    fn face(&mut self, f: i64) -> Result<usize, SatError> {
        self.faces += 1;
        if self.faces > MAX_FACES {
            return Err(SatError::TooLarge(format!("more than {MAX_FACES} faces")));
        }
        let r = self.rec_of(f, "face")?.clone();
        let surf = r.ptr(5).ok_or_else(|| SatError::Malformed(format!("face ${f} has no surface")))?;
        let (sid, flipped) = self.surface(surf)?;
        let mut bounds = Vec::new();
        for l in self.chain(r.ptr(2), 1)? {
            let lr = self.rec_of(l, "loop")?.clone();
            let mut oriented = Vec::new();
            let mut apex = None;
            for ce in self.chain(lr.ptr(2), 1)? {
                let cr = self.rec_of(ce, "coedge")?.clone();
                let e = cr.ptr(4).ok_or_else(|| SatError::Malformed(format!("coedge ${ce} has no edge")))?;
                // An edge without a curve is a point (a cone's apex): no edge in STEP.
                let er = self.rec_of(e, "edge")?.clone();
                if er.ptr(6).is_none() {
                    apex = er.ptr(1);
                    continue;
                }
                let eid = self.edge(e)?;
                *self.uses.entry(e).or_default() += 1;
                let fwd = cr.word(5) != Some("reversed");
                oriented.push(self.add(&format!("ORIENTED_EDGE('',*,*,#{eid},{})", if fwd { ".T." } else { ".F." })));
            }
            if oriented.is_empty() {
                if let Some(v) = apex {
                    let vp = self.vertex(v)?;
                    let vl = self.add(&format!("VERTEX_LOOP('',#{vp})"));
                    bounds.push(self.add(&format!("FACE_BOUND('',#{vl},.T.)")));
                }
                continue;
            }
            let lp = self.add(&format!("EDGE_LOOP('',{})", list(&oriented)));
            bounds.push(self.add(&format!("FACE_BOUND('',#{lp},.T.)")));
        }
        let reversed = r.word(6) == Some("reversed");
        let same = reversed == flipped;
        Ok(self.add(&format!("ADVANCED_FACE('',{},#{sid},{})", list(&bounds), if same { ".T." } else { ".F." })))
    }

    /// Faces of a shell and its subshells (nested at most 64 deep).
    fn shell_faces(&mut self, sh: i64, depth: u32, out: &mut Vec<i64>) -> Result<(), SatError> {
        if depth > 64 {
            return Err(SatError::Malformed("subshells nest too deep".into()));
        }
        let r = self.rec(sh)?.clone();
        let faces_at = if r.ty == "shell" { 3 } else { 2 };
        out.extend(self.chain(r.ptr(faces_at), 1)?);
        let sub_at = if r.ty == "shell" { 2 } else { 3 };
        for s in self.chain(r.ptr(sub_at), 1)? {
            self.shell_faces(s, depth + 1, out)?;
        }
        Ok(())
    }
}

fn transform(sat: &Sat, body: &Rec) -> Xf {
    let unit = sat.scale;
    let ident = Xf { r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3], s: 1.0, unit };
    let Some(t) = body.ptr(3).and_then(|i| sat.rec(i)).filter(|r| r.ty == "transform") else { return ident };
    let (Some(a), Some(b), Some(c), Some(tr)) = (t.vec3(1), t.vec3(4), t.vec3(7), t.vec3(10)) else { return ident };
    let s = t.num(13).filter(|s| s.abs() > 1e-300).unwrap_or(1.0);
    Xf { r: [a, b, c], t: tr, s, unit }
}

pub(crate) fn write(sat: &Sat, name: &str) -> Result<(String, usize, Vec<String>), SatError> {
    let ident = Xf { r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3], s: 1.0, unit: sat.scale };
    let mut w = Writer {
        sat,
        out: String::new(),
        next: 0,
        xf: ident,
        vertices: BTreeMap::new(),
        edges: BTreeMap::new(),
        curves: BTreeMap::new(),
        surfaces: BTreeMap::new(),
        faces: 0,
        uses: BTreeMap::new(),
        warnings: Vec::new(),
    };
    let app = w.add("APPLICATION_CONTEXT('automotive design')");
    w.add(&format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"));
    let pc = w.add(&format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let pdc = w.add(&format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
    let len_u = w.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))");
    let ang = w.add("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))");
    let sol = w.add("(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT())");
    let unc =
        w.add(&format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-05),#{len_u},'distance_accuracy_value','')"));
    let ctx = w.add(&format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{len_u},#{ang},#{sol})) REPRESENTATION_CONTEXT('',''))"
    ));
    let bodies: Vec<i64> = sat
        .recs
        .iter()
        .enumerate()
        .filter(|(_, r)| r.ty == "body")
        .filter_map(|(i, _)| i64::try_from(i).ok())
        .collect();
    let many = bodies.len() > 1;
    let mut written = 0;
    for (n, b) in bodies.iter().enumerate() {
        let br = w.rec(*b)?.clone();
        // Geometry is written per body (each has its own transform).
        w.xf = transform(sat, &br);
        w.vertices.clear();
        w.edges.clear();
        w.curves.clear();
        w.surfaces.clear();
        let mut solids = Vec::new();
        let mut sheets = Vec::new();
        for lump in w.chain(br.ptr(1), 1)? {
            let lr = w.rec_of(lump, "lump")?.clone();
            let mut shells = Vec::new();
            for sh in w.chain(lr.ptr(2), 1)? {
                let mut faces = Vec::new();
                w.shell_faces(sh, 0, &mut faces)?;
                let mut ids = Vec::new();
                w.uses.clear();
                for f in faces {
                    ids.push(w.face(f)?);
                }
                if ids.is_empty() {
                    continue;
                }
                // A shell with free edges bounds no volume: written as a sheet.
                if w.uses.values().any(|n| *n < 2) {
                    sheets.push(w.add(&format!("OPEN_SHELL('',{})", list(&ids))));
                } else {
                    shells.push(w.add(&format!("CLOSED_SHELL('',{})", list(&ids))));
                }
            }
            // The first shell of a lump is its outside; the others are cavities.
            let mut it = shells.into_iter();
            let Some(outer) = it.next() else { continue };
            let voids: Vec<usize> = it.map(|s| w.add(&format!("ORIENTED_CLOSED_SHELL('',*,#{s},.F.)"))).collect();
            solids.push(if voids.is_empty() {
                w.add(&format!("MANIFOLD_SOLID_BREP('',#{outer})"))
            } else {
                w.add(&format!("BREP_WITH_VOIDS('',#{outer},{})", list(&voids)))
            });
        }
        if !sheets.is_empty() {
            w.warnings.push(format!("body {} has open shells (free edges); they are imported as sheets", n + 1));
            let model = w.add(&format!("SHELL_BASED_SURFACE_MODEL('',{})", list(&sheets)));
            let label = string(&format!("{name} {} sheet", n + 1));
            let prod = w.add(&format!("PRODUCT({label},{label},'',(#{pc}))"));
            let pdf = w.add(&format!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"));
            let pd = w.add(&format!("PRODUCT_DEFINITION('design','',#{pdf},#{pdc})"));
            let pds = w.add(&format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
            let rep = w.add(&format!("MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#{model}),#{ctx})"));
            w.add(&format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})"));
            written += 1;
        }
        if solids.is_empty() {
            if sheets.is_empty() {
                w.warnings.push(format!("body {} has no faces", n + 1));
            }
            continue;
        }
        written += 1;
        let label = if many { string(&format!("{name} {}", n + 1)) } else { string(name) };
        let prod = w.add(&format!("PRODUCT({label},{label},'',(#{pc}))"));
        let pdf = w.add(&format!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"));
        let pd = w.add(&format!("PRODUCT_DEFINITION('design','',#{pdf},#{pdc})"));
        let pds = w.add(&format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
        let rep = w.add(&format!("ADVANCED_BREP_SHAPE_REPRESENTATION('',{},#{ctx})", list(&solids)));
        w.add(&format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})"));
    }
    if written == 0 {
        return Err(SatError::Unsupported("the file holds no body with faces".into()));
    }
    let mut text = String::from("ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ACIS SAT converted by Gusset'),'2;1');\n");
    let _ = writeln!(text, "FILE_NAME({},'',(''),(''),'gusset-sat','','');", string(name));
    text.push_str("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n");
    text.push_str(&w.out);
    text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok((text, written, w.warnings))
}
