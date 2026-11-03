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
mod geom;
mod model;
mod reader;
mod schema;
mod step;

pub use decode::{Graph, Node, decode};
pub use model::{BodyKind, Model};
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
    pub schema: u32,
    pub warnings: Vec<String>,
}

/// Reads an XT file and writes its bodies as STEP text (products named after `name`).
///
/// # Errors
/// Malformed, unsupported (schema older than V4, bare binary, unknown node types) or too
/// large files.
pub fn to_step(bytes: &[u8], name: &str, limits: &Limits) -> Result<Converted, XtError> {
    let g = decode(bytes, limits)?;
    let m = model::build(&g)?;
    let (step, warnings) = step::write(&g, &m, name);
    Ok(Converted { step, bodies: m.bodies.len(), schema: g.schema_number, warnings })
}

#[cfg(test)]
mod tests;
