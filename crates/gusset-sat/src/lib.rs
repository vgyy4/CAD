//! ACIS SAT text files (ADR 0039), read clean-room from sample files and converted to
//! STEP AP214 for the sandboxed importer, as Parasolid XT is (ADR 0035).
//!
//! Versions 1 to 7 (and later ones written in the version 7 layout) are read: bodies,
//! lumps, shells, faces, loops, coedges, edges, vertices and points (tolerant ones too),
//! with plane, cylinder, cone, sphere, torus and B-spline surfaces (procedural ones through
//! their stored approximation) and line, circle, ellipse, helix and B-spline curves. A face
//! on geometry not read yet (elliptical cones, helical sweeps) is left out with a warning,
//! so its shell comes in as a sheet.

#[doc(hidden)]
pub mod fixtures;
pub mod fusion;
pub mod fusion_tree;
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
        // Some 60 bytes per token in memory: 10M tokens stay well under a gigabyte (a
        // 100 MB text file of real geometry has fewer).
        Limits { max_records: 5_000_000, max_tokens: 10_000_000 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converted {
    pub step: String,
    pub bodies: usize,
    /// Sheet (open-shell) bodies left out when sheets were not asked for.
    pub skipped_sheets: usize,
    pub version: u32,
    pub warnings: Vec<String>,
}

/// Converts a SAT file to STEP text (millimetres).
pub fn to_step(bytes: &[u8], name: &str, limits: &Limits) -> Result<Converted, SatError> {
    to_step_placed(bytes, name, limits, None, true)
}

/// As [`to_step`], with every body moved by `place` (millimetres) after its own transform:
/// a Fusion component's occurrence.
pub fn to_step_placed(
    bytes: &[u8],
    name: &str,
    limits: &Limits,
    place: Option<&fusion::Placement>,
    keep_sheets: bool,
) -> Result<Converted, SatError> {
    to_step_coloured(bytes, name, limits, place, keep_sheets, &Colours::new())
}

/// Fusion material ids (lowercase UUIDs) to sRGB colours (`0xRRGGBB`).
pub type Colours = std::collections::HashMap<String, u32>;

/// As [`to_step_placed`], with each face that carries (or whose body carries) a
/// `NEUTRON_Material_attrib_def` material found in `colours` styled with that colour (AP214
/// `STYLED_ITEM`s, as the Parasolid converter writes them).
pub fn to_step_coloured(
    bytes: &[u8],
    name: &str,
    limits: &Limits,
    place: Option<&fusion::Placement>,
    keep_sheets: bool,
    colours: &Colours,
) -> Result<Converted, SatError> {
    to_step_coloured_history(bytes, name, limits, place, keep_sheets, colours, None)
}

/// The face materials of a Fusion body's history blob (`BREP.<id>.smbh`), by face tag. The
/// body blob written after it lost its material attributes in some files (a board whose
/// faces were painted before later features), but its faces keep their tags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    by_tag: std::collections::HashMap<String, String>,
    /// The colour of each body of the blob after the first, from the feature's body references
    /// ([`fusion::Design::body_colours`]).
    bodies: Vec<Option<u32>>,
}

impl History {
    /// Reads the history blob: `None` when it does not parse or holds no face material.
    /// A tag that two faces give different materials is dropped.
    pub fn read(bytes: &[u8], limits: &Limits) -> Option<History> {
        let sat = parse(bytes, limits).ok()?;
        let materials = sat.materials();
        if materials.is_empty() {
            return None;
        }
        let mut by_tag: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut clashing = std::collections::HashSet::new();
        for (face, tag) in sat.face_tags() {
            let Some(&m) = materials.get(&face) else { continue };
            if by_tag.get(&tag).is_some_and(|old| old != m) {
                clashing.insert(tag);
            } else {
                by_tag.entry(tag).or_insert_with(|| m.to_string());
            }
        }
        by_tag.retain(|t, _| !clashing.contains(t));
        (!by_tag.is_empty()).then_some(History { by_tag, bodies: Vec::new() })
    }

    /// Number of tagged materials.
    pub fn len(&self) -> usize {
        self.by_tag.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_tag.is_empty() && self.bodies.is_empty()
    }

    /// Adds the colours of a body blob's bodies, one per body that has a transform, not
    /// counting the first (the feature's own helper body). They colour the faces a body has no
    /// material for, when the blob has exactly that many bodies.
    #[must_use]
    pub fn with_body_colours(mut self, colours: Vec<Option<u32>>) -> History {
        self.bodies = colours;
        self
    }
}

/// As [`to_step_coloured`], with the materials of a body's [`History`] given to its faces
/// that have none of their own, matched by face tag. The history is used only when at least
/// half of its tags name exactly one face of this body (else it is another body's); a tag
/// that names several faces is skipped.
pub fn to_step_coloured_history(
    bytes: &[u8],
    name: &str,
    limits: &Limits,
    place: Option<&fusion::Placement>,
    keep_sheets: bool,
    colours: &Colours,
    history: Option<&History>,
) -> Result<Converted, SatError> {
    let sat = parse(bytes, limits)?;
    let (step, bodies, skipped_sheets, warnings) = step::write(&sat, name, place, keep_sheets, colours, history)?;
    Ok(Converted { step, bodies, skipped_sheets, version: sat.version, warnings })
}

#[cfg(test)]
mod tests;
