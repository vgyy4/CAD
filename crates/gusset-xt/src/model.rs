//! The topology of the file's bodies (ADR 0035): bodies, their shells (solid regions'
//! shells; for V4 files, the body's shell chain), faces with their orientation, loops of
//! oriented edges, and edges with their end vertices.

use crate::XtError;
use crate::decode::Graph;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Solid,
    Sheet,
    Wire,
    General,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub node: u32,
    /// Where an assembly places this body: `x' = M x + T` (model space).
    pub placement: Option<Xf>,
    pub kind: BodyKind,
    /// Outer shells first; void shells (cavities) are marked.
    pub shells: Vec<Shell>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shell {
    pub faces: Vec<Face>,
    /// A cavity inside a solid.
    pub void: bool,
    /// The region (lump) the shell bounds; shells of one lump are one solid (0: unknown).
    pub lump: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub node: u32,
    pub surface: u32,
    /// The face normal (pointing out of the solid) agrees with the surface's natural normal.
    pub same_sense: bool,
    pub loops: Vec<Loop>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loop {
    /// Edges in loop order, each with its sense (true: along the edge).
    Edges(Vec<(u32, bool)>),
    /// An isolated vertex.
    Vertex(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub curve: u32,
    /// Start and end vertices; `None` for a ring edge (a closed curve without vertices).
    pub ends: Option<(u32, u32)>,
    /// The edge runs along the curve's parameter direction.
    pub same_sense: bool,
}

/// What the file's bodies are made of.
#[derive(Debug, Clone, Default)]
pub struct Model {
    pub bodies: Vec<Body>,
    pub edges: BTreeMap<u32, Edge>,
    pub warnings: Vec<String>,
    /// Shells, faces and loops already read for the body being built: a hostile file that
    /// points many regions at one shell (or many loops at one fin chain) is read once.
    seen: BTreeSet<u32>,
    /// Faces read over all bodies and placements.
    faces: usize,
}

/// Most faces one import may produce, over every body and assembly placement.
const MAX_FACES: usize = 2_000_000;

/// Longest chain walked (faces of a shell, fins of a loop and so on).
const MAX_CHAIN: usize = 10_000_000;

/// Follows `field` from `start` until null or back at the start.
fn chain(g: &Graph, start: u32, field: &str) -> Result<Vec<u32>, XtError> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut at = start;
    while at != 0 && seen.insert(at) {
        out.push(at);
        if out.len() > MAX_CHAIN {
            return Err(XtError::TooLarge("a chain of nodes is too long".into()));
        }
        at = g.ptr(at, field);
    }
    Ok(out)
}

fn sense(g: &Graph, node: u32) -> bool {
    g.ch(node, "sense") != Some(b'-')
}

/// Builds the model from the decoded nodes.
pub(crate) fn build(g: &Graph) -> Result<Model, XtError> {
    let mut m = Model::default();
    for (b, placement) in body_nodes(g, &mut m.warnings) {
        let kind = match g.int(b, "body_type") {
            Some(1) => BodyKind::Solid,
            Some(2) => BodyKind::Wire,
            Some(3) => BodyKind::Sheet,
            _ => BodyKind::General,
        };
        if kind == BodyKind::Wire {
            m.warnings.push("a wire body was skipped (only solids and sheets are imported)".into());
            continue;
        }
        let mut shells = Vec::new();
        m.seen.clear();
        let regions = chain(g, g.ptr(b, "region"), "next")?;
        if regions.is_empty() {
            // V4 files: the body's shells directly.
            for s in chain(g, g.ptr(b, "shell"), "next")? {
                shells.push(shell(g, s, 0, &mut m)?);
            }
        } else {
            for r in regions {
                if g.ch(r, "type") != Some(b'S') && kind == BodyKind::Solid {
                    continue;
                }
                for s in chain(g, g.ptr(r, "shell"), "next")? {
                    shells.push(shell(g, s, r, &mut m)?);
                }
            }
            // In a solid, every shell but the one with the largest extent bounds a cavity:
            // the importer works that out from the geometry; keep them all as outer shells.
        }
        shells.retain(|s: &Shell| !s.faces.is_empty());
        if shells.is_empty() {
            m.warnings.push(format!("body {b} has no faces"));
            continue;
        }
        m.bodies.push(Body { node: b, placement, kind, shells });
    }
    if m.bodies.is_empty() {
        return Err(XtError::Unsupported("the file holds no solid or sheet body".into()));
    }
    Ok(m)
}

/// The bodies to import: the root body, the bodies of a root list of parts, or else every
/// body node in the file (assemblies are flattened without their placements).
/// An affine placement: matrix and translation.
pub type Xf = ([[f64; 3]; 3], [f64; 3]);

/// `m x`.
pub(crate) fn mat_vec(m: &[[f64; 3]; 3], x: [f64; 3]) -> [f64; 3] {
    let [r0, r1, r2] = m;
    let row = |r: &[f64; 3]| {
        let [a, b, c] = r;
        a * x[0] + b * x[1] + c * x[2]
    };
    [row(r0), row(r1), row(r2)]
}

/// `a(b(x))`.
fn compose(a: &Xf, b: &Xf) -> Xf {
    let [b0, b1, b2] = b.0;
    // Columns of b, mapped by a.
    let col = |j: usize| {
        mat_vec(
            &a.0,
            [b0.get(j).copied().unwrap_or(0.0), b1.get(j).copied().unwrap_or(0.0), b2.get(j).copied().unwrap_or(0.0)],
        )
    };
    let (c0, c1, c2) = (col(0), col(1), col(2));
    let m = [[c0[0], c1[0], c2[0]], [c0[1], c1[1], c2[1]], [c0[2], c1[2], c2[2]]];
    let t = mat_vec(&a.0, b.1);
    (m, [t[0] + a.1[0], t[1] + a.1[1], t[2] + a.1[2]])
}

/// An instance's transform: `x' = (R x + t) * scale`, R stored row by row.
fn placement_of(g: &Graph, tf: u32) -> Option<Xf> {
    if tf == 0 {
        return None;
    }
    let r: Vec<f64> = g
        .list(tf, "rotation_matrix")
        .iter()
        .map(|v| if let crate::reader::Val::Real(x) = v { *x } else { 0.0 })
        .collect();
    let s = g.real(tf, "scale").filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0);
    let t = g.vec3(tf, "translation_vector").unwrap_or([0.0; 3]);
    let at = |i: usize| r.get(i).copied().unwrap_or(0.0) * s;
    let m = if r.len() == 9 {
        [[at(0), at(1), at(2)], [at(3), at(4), at(5)], [at(6), at(7), at(8)]]
    } else {
        [[s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]]
    };
    Some((m, t.map(|x| x * s)))
}

/// Most instances one file's assembly tree may visit (shared sub-assemblies count once per
/// use, so a hostile file cannot make the walk exponential).
const MAX_INSTANCE_VISITS: usize = 100_000;

/// The bodies of an assembly with their placements (instances nest up to 32 deep; an
/// assembly that contains itself is not entered again).
fn assembly_bodies(
    g: &Graph,
    asm: u32,
    xf: Option<Xf>,
    path: &mut Vec<u32>,
    budget: &mut usize,
    out: &mut Vec<(u32, Option<Xf>)>,
) {
    if path.len() > 32 || path.contains(&asm) {
        return;
    }
    path.push(asm);
    let mut seen = BTreeSet::new();
    let mut inst = g.ptr(asm, "sub_instance");
    while inst != 0 && seen.insert(inst) {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let part = g.ptr(inst, "part");
        let local = placement_of(g, g.ptr(inst, "transform"));
        let total = match (&xf, &local) {
            (Some(a), Some(b)) => Some(compose(a, b)),
            (Some(a), None) => Some(*a),
            (None, b) => *b,
        };
        match g.type_name(part) {
            Some("BODY") => out.push((part, total)),
            Some("ASSEMBLY") => assembly_bodies(g, part, total, path, budget, out),
            _ => {}
        }
        inst = g.ptr(inst, "next_in_part");
    }
    path.pop();
}

pub(crate) fn body_nodes(g: &Graph, warnings: &mut Vec<String>) -> Vec<(u32, Option<Xf>)> {
    match g.type_name(1) {
        Some("BODY") => return vec![(1, None)],
        Some("POINTER_LIS_BLOCK") => {
            let mut out = Vec::new();
            let mut block = 1;
            let mut seen = BTreeSet::new();
            while block != 0 && seen.insert(block) {
                for v in g.list(block, "entries") {
                    if let crate::reader::Val::Ptr(p) = v
                        && g.type_name(*p) == Some("BODY")
                    {
                        out.push((*p, None));
                    }
                }
                block = g.ptr(block, "next_block");
            }
            if !out.is_empty() {
                return out;
            }
        }
        Some("ASSEMBLY") => {
            let mut out = Vec::new();
            assembly_bodies(g, 1, None, &mut Vec::new(), &mut { MAX_INSTANCE_VISITS }, &mut out);
            if !out.is_empty() {
                return out;
            }
            warnings.push("an assembly without instances: its bodies are imported where they lie".into());
        }
        _ => {}
    }
    g.nodes.iter().filter(|(i, _)| g.type_name(**i) == Some("BODY")).map(|(i, _)| (*i, None)).collect()
}

fn shell(g: &Graph, s: u32, lump: u32, m: &mut Model) -> Result<Shell, XtError> {
    let mut faces = Vec::new();
    if !m.seen.insert(s) {
        return Ok(Shell { faces, void: false, lump });
    }
    // Back faces: the shell's region is behind them, so their normals point away from it.
    for f in chain(g, g.ptr(s, "face"), "next")? {
        if let Some(face) = face(g, f, false, m)? {
            faces.push(face);
        }
    }
    // Front faces (only in shells whose region is in front of them): flipped.
    for f in chain(g, g.ptr(s, "front_face"), "next_front")? {
        if let Some(face) = face(g, f, true, m)? {
            faces.push(face);
        }
    }
    Ok(Shell { faces, void: false, lump })
}

fn face(g: &Graph, f: u32, flip: bool, m: &mut Model) -> Result<Option<Face>, XtError> {
    if !m.seen.insert(f) {
        return Ok(None);
    }
    m.faces += 1;
    if m.faces > MAX_FACES {
        return Err(XtError::TooLarge(format!("more than {MAX_FACES} faces over all bodies and placements")));
    }
    let surface = g.ptr(f, "surface");
    if surface == 0 {
        m.warnings.push(format!("face {f} has no surface and was skipped"));
        return Ok(None);
    }
    // The face normal is the surface's natural normal when both senses agree.
    let same = sense(g, f) == sense(g, surface);
    let mut loops = Vec::new();
    for l in chain(g, g.ptr(f, "loop"), "next")? {
        let first = g.ptr(l, "fin");
        if !m.seen.insert(l) || (first != 0 && !m.seen.insert(first)) {
            continue;
        }
        let fins = chain(g, first, "forward")?;
        let mut out = Vec::new();
        let mut vertex = None;
        for fin in fins {
            let e = g.ptr(fin, "edge");
            if e == 0 {
                // An isolated loop: one fin, a vertex, no edge.
                vertex = Some(g.ptr(fin, "vertex")).filter(|v| *v != 0);
                continue;
            }
            let along = fin_sense(g, fin, e);
            edge(g, e, m);
            out.push((e, along));
        }
        if !out.is_empty() {
            loops.push(Loop::Edges(out));
        } else if let Some(v) = vertex {
            loops.push(Loop::Vertex(v));
        }
    }
    Ok(Some(Face { node: f, surface, same_sense: same != flip, loops }))
}

/// A fin's sense; files without the field (V4) make the edge's own fin the positive one.
fn fin_sense(g: &Graph, fin: u32, edge: u32) -> bool {
    match g.ch(fin, "sense") {
        Some(c) => c != b'-',
        None => g.ptr(edge, "fin") == fin,
    }
}

/// Records an edge: the positive fin's forward vertex is the end, the other fin's is the
/// start.
fn edge(g: &Graph, e: u32, m: &mut Model) {
    if m.edges.contains_key(&e) {
        return;
    }
    let head = g.ptr(e, "fin");
    let end = g.ptr(head, "vertex");
    let start = g.ptr(g.ptr(head, "other"), "vertex");
    // A tolerant edge has no curve of its own; its fins carry trimmed SP-curves.
    let curve = match g.ptr(e, "curve") {
        0 => g.ptr(head, "curve"),
        c => c,
    };
    let ends = if start != 0 && end != 0 { Some((start, end)) } else { None };
    let same_sense = curve == 0 || sense(g, curve);
    m.edges.insert(e, Edge { curve, ends, same_sense });
}

/// Names written with STEP's `\X2\hhhh\X0\` escapes (SolidWorks does) as plain text.
pub(crate) fn decode_step_text(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("\\X2\\") {
        out.push_str(rest.get(..i).unwrap_or(""));
        let after = rest.get(i + 4..).unwrap_or("");
        let Some(j) = after.find("\\X0\\") else {
            out.push_str(rest.get(i..).unwrap_or(""));
            return out;
        };
        let hex = after.get(..j).unwrap_or("");
        let units: Vec<u16> = hex
            .as_bytes()
            .chunks(4)
            .filter_map(|c| std::str::from_utf8(c).ok().and_then(|h| u16::from_str_radix(h, 16).ok()))
            .collect();
        out.extend(char::decode_utf16(units).map(|c| c.unwrap_or('?')));
        rest = after.get(j + 4..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// The `SDL/TYSA_COLOUR` attributes (red, green, blue from 0 to 1), by owner node.
pub(crate) fn colours(g: &Graph) -> BTreeMap<u32, [f64; 3]> {
    let mut out = BTreeMap::new();
    for (idx, n) in &g.nodes {
        if g.layouts.get(&n.ty).map(|l| l.name.as_str()) != Some("ATTRIBUTE") {
            continue;
        }
        let def = g.ptr(*idx, "definition");
        if g.string(g.ptr(def, "identifier"), "string") != "SDL/TYSA_COLOUR" {
            continue;
        }
        let reals: Vec<f64> = g
            .list(*idx, "fields")
            .iter()
            .filter_map(|v| if let crate::reader::Val::Ptr(p) = v { Some(*p) } else { None })
            .flat_map(|p| g.list(p, "values").iter())
            .filter_map(|v| if let crate::reader::Val::Real(r) = v { Some(*r) } else { None })
            .collect();
        if let [r, gr, b] = reals.as_slice()
            && [r, gr, b].iter().all(|x| x.is_finite() && (0.0..=1.0).contains(*x))
        {
            out.entry(g.ptr(*idx, "owner")).or_insert([*r, *gr, *b]);
        }
    }
    out
}

/// The `SDL/TYSA_NAME` attributes of the file (names the application gave), by owner node;
/// the first non-empty name of each owner wins.
pub(crate) fn names(g: &Graph) -> BTreeMap<u32, String> {
    let mut out = BTreeMap::new();
    for (idx, n) in &g.nodes {
        if g.layouts.get(&n.ty).map(|l| l.name.as_str()) != Some("ATTRIBUTE") {
            continue;
        }
        let owner = g.ptr(*idx, "owner");
        if owner == 0 || out.contains_key(&owner) {
            continue;
        }
        let def = g.ptr(*idx, "definition");
        if g.string(g.ptr(def, "identifier"), "string") != "SDL/TYSA_NAME" {
            continue;
        }
        let Some(value) = g
            .list(*idx, "fields")
            .iter()
            .find_map(|v| if let crate::reader::Val::Ptr(p) = v { Some(*p) } else { None })
        else {
            continue;
        };
        let s: String = g.string(value, "values").chars().filter(|c| !c.is_control()).take(200).collect();
        let s = decode_step_text(s.trim());
        if !s.is_empty() {
            out.insert(owner, s);
        }
    }
    out
}
