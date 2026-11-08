//! Parasolid XT reader (ADR 0035). A transmit file (`.x_t` text or `.x_b` neutral binary)
//! is decoded node by node, driven by per-schema node layouts, into a graph; its bodies'
//! topology is collected and written as STEP AP214 text, which the sandboxed STEP importer
//! then turns into kernel shapes.
//!
//! Written from the public *Parasolid XT Format Reference* (schema 12006) in our own words;
//! layouts of older schemas were worked out from public-domain NIST sample files. No
//! Parasolid schema file, SDK or third-party reader was used. Files are untrusted: node
//! counts, element counts and chains are bounded.

mod decode;
mod eval;
mod geom;
mod model;
mod reader;
mod schema;
mod step;

pub use decode::{Graph, Node, decode};
pub use model::{BodyKind, Model, Xf};
pub use reader::Val;

/// Why a file cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XtError {
    Malformed(String),
    Unsupported(String),
    TooLarge(String),
}

impl std::fmt::Display for XtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XtError::Malformed(m) => write!(f, "the XT file is damaged or not an XT file: {m}"),
            XtError::Unsupported(m) => write!(f, "{m}"),
            XtError::TooLarge(m) => write!(f, "the XT file is too large: {m}"),
        }
    }
}

impl std::error::Error for XtError {}

/// Bounds on what a file may make the reader allocate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_nodes: usize,
    /// Values in variable-length fields, all nodes together.
    pub max_elements: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_nodes: 20_000_000, max_elements: 200_000_000 }
    }
}

/// The result of a conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converted {
    pub step: String,
    pub bodies: usize,
    /// Per body, in the STEP file's order: its name and colour (`0xRRGGBB`) from the
    /// file's attributes, when it has them.
    pub looks: Vec<(Option<String>, Option<u32>)>,
    pub schema: u32,
    pub warnings: Vec<String>,
}

/// Reads an XT file and writes its bodies as STEP text (products named after `name`).
///
/// # Errors
/// Malformed, unsupported (schema older than V4, bare binary, unknown node types) or too
/// large files.
pub fn to_step(bytes: &[u8], name: &str, limits: &Limits) -> Result<Converted, XtError> {
    to_step_placed(bytes, name, limits, None)
}

/// [`to_step`] with every body moved by `place` (a rigid placement in the file's own length
/// unit, metres for XT: `x' = M x + t`) after its own placements. Used to put a component
/// of an assembly where the assembly stores it.
///
/// # Errors
/// As [`to_step`].
pub fn to_step_placed(bytes: &[u8], name: &str, limits: &Limits, place: Option<&Xf>) -> Result<Converted, XtError> {
    let g = decode(bytes, limits)?;
    let mut m = model::build(&g)?;
    if let Some(p) = place {
        for b in &mut m.bodies {
            b.placement = Some(match &b.placement {
                Some(own) => model::compose(p, own),
                None => *p,
            });
        }
    }
    let (step, warnings) = step::write(&g, &m, name);
    let (names, colours) = (model::names(&g), model::colours(&g));
    let looks = m
        .bodies
        .iter()
        .map(|b| {
            // The colour most of its faces have (face colours override the body's), else the
            // body's own.
            let rgb = |[r, g, b]: [f64; 3]| {
                let byte = |x: f64| (x * 255.0).round().clamp(0.0, 255.0) as u32;
                (byte(r) << 16) | (byte(g) << 8) | byte(b)
            };
            // A map, so a file with a colour per face stays linear (security review T4); ties
            // go to the lowest colour value, the same on every run.
            let mut count: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
            for f in b.shells.iter().flat_map(|s| &s.faces) {
                if let Some(c) = colours.get(&f.node).copied().map(rgb) {
                    *count.entry(c).or_default() += 1;
                }
            }
            let rgb = count
                .into_iter()
                .max_by_key(|(c, n)| (*n, std::cmp::Reverse(*c)))
                .map(|(c, _)| c)
                .or_else(|| colours.get(&b.node).copied().map(rgb));
            (names.get(&b.node).cloned(), rgb)
        })
        .collect();
    Ok(Converted { step, bodies: m.bodies.len(), looks, schema: g.schema_number, warnings })
}

#[cfg(test)]
mod tests;
