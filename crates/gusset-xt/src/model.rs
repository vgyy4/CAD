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
    pub kind: BodyKind,
    /// Outer shells first; void shells (cavities) are marked.
    pub shells: Vec<Shell>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shell {
    pub faces: Vec<Face>,
    /// A cavity inside a solid.
    pub void: bool,
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
}

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
    for b in body_nodes(g, &mut m.warnings) {
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
        let regions = chain(g, g.ptr(b, "region"), "next")?;
        if regions.is_empty() {
            // V4 files: the body's shells directly.
            for s in chain(g, g.ptr(b, "shell"), "next")? {
                shells.push(shell(g, s, false, &mut m)?);
            }
        } else {
            for r in regions {
                if g.ch(r, "type") != Some(b'S') && kind == BodyKind::Solid {
                    continue;
                }
                for s in chain(g, g.ptr(r, "shell"), "next")? {
                    shells.push(shell(g, s, false, &mut m)?);
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
        m.bodies.push(Body { node: b, kind, shells });
    }
    if m.bodies.is_empty() {
        return Err(XtError::Unsupported("the file holds no solid or sheet body".into()));
    }
    Ok(m)
}

/// The bodies to import: the root body, the bodies of a root list of parts, or else every
/// body node in the file (assemblies are flattened without their placements).
fn body_nodes(g: &Graph, warnings: &mut Vec<String>) -> Vec<u32> {
    match g.type_name(1) {
        Some("BODY") => return vec![1],
        Some("POINTER_LIS_BLOCK") => {
            let mut out = Vec::new();
            let mut block = 1;
            let mut seen = BTreeSet::new();
            while block != 0 && seen.insert(block) {
                for v in g.list(block, "entries") {
                    if let crate::reader::Val::Ptr(p) = v
                        && g.type_name(*p) == Some("BODY")
                    {
                        out.push(*p);
                    }
                }
                block = g.ptr(block, "next_block");
            }
            if !out.is_empty() {
                return out;
            }
        }
        Some("ASSEMBLY") => {
            warnings.push("an assembly was flattened: its parts are imported without their placements".into())
        }
        _ => {}
    }
    g.nodes.iter().filter(|(i, _)| g.type_name(**i) == Some("BODY")).map(|(i, _)| *i).collect()
}

fn shell(g: &Graph, s: u32, void: bool, m: &mut Model) -> Result<Shell, XtError> {
    let mut faces = Vec::new();
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
    Ok(Shell { faces, void })
}

fn face(g: &Graph, f: u32, flip: bool, m: &mut Model) -> Result<Option<Face>, XtError> {
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
    let curve = g.ptr(e, "curve");
    let ends = if start != 0 && end != 0 { Some((start, end)) } else { None };
    let same_sense = curve == 0 || sense(g, curve);
    m.edges.insert(e, Edge { curve, ends, same_sense });
}
