//! ACIS SAT text files (ADR 0039), read clean-room from sample files and converted to
//! STEP AP214 for the sandboxed importer, as Parasolid XT is (ADR 0035).
//!
//! Versions 4 to 7 (and later ones written in the version 7 layout) are read: bodies,
//! lumps, shells, faces, loops, coedges, edges, vertices and points, with plane,
//! cylinder and cone surfaces and line, circle and ellipse curves. Other geometry (spline
//! surfaces and curves, spheres, tori) is refused with its type named.

#[doc(hidden)]
pub mod fixtures;
mod parse;
mod step;

pub use parse::{Rec, Sat, Tok, parse};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SatError {
    Malformed(String),
    Unsupported(String),
    TooLarge(String),
}

impl std::fmt::Display for SatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SatError::Malformed(m) => write!(f, "the SAT file is damaged or not a SAT file: {m}"),
            SatError::Unsupported(m) => write!(f, "{m}"),
            SatError::TooLarge(m) => write!(f, "the SAT file is too large: {m}"),
        }
    }
}

impl std::error::Error for SatError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_records: usize,
    /// Tokens of all records together.
    pub max_tokens: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_records: 10_000_000, max_tokens: 200_000_000 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converted {
    pub step: String,
    pub bodies: usize,
    pub version: u32,
    pub warnings: Vec<String>,
}

/// Converts a SAT file to STEP text (millimetres).
pub fn to_step(bytes: &[u8], name: &str, limits: &Limits) -> Result<Converted, SatError> {
    let sat = parse(bytes, limits)?;
    let (step, bodies, warnings) = step::write(&sat, name)?;
    Ok(Converted { step, bodies, version: sat.version, warnings })
}

#[cfg(test)]
mod tests;
