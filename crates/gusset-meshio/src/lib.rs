//! Mesh exchange for Gusset: STL (binary/ASCII), 3MF (core) and OBJ, both directions.
//!
//! * **Import** treats every file as hostile: size, triangle, vertex, ZIP and XML limits
//!   ([`Limits`]), no panics, no unbounded allocation from declared counts, structured
//!   errors ([`MeshIoError`]) and a report of what was repaired ([`ImportReport`]).
//! * **Export** takes named meshes in millimetres and writes deterministic bytes:
//!   exporting the same meshes twice gives byte-identical files.
//! * [`utils`] has the mesh helpers used along the way (welding, normals, validation,
//!   polygon triangulation).
//!
//! ```
//! use gusset_kernel::Mesh;
//! use gusset_meshio::{export_to_vec, import_bytes, ExportBody, ExportFormat, Format, ImportOptions};
//!
//! let tetra = Mesh {
//!     positions: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
//!     indices: vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
//!     ..Default::default()
//! };
//! let (bytes, _) = export_to_vec(&[ExportBody { name: "tetra", mesh: &tetra }], ExportFormat::ThreeMf).unwrap();
//! let imported = import_bytes(&bytes, Some(Format::ThreeMf), &ImportOptions::default()).unwrap();
//! assert_eq!(imported.meshes[0].name, "tetra");
//! assert!(imported.meshes[0].validation.can_become_solid());
//! ```

#![cfg_attr(test, allow(clippy::float_cmp))]

use std::fmt;
use std::io::{Read, Write};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gusset_kernel::Mesh;

pub mod archive;
pub mod error;
mod obj;
mod stl;
mod threemf;
pub mod utils;
mod zip;

pub use error::{MeshIoError, Result};
pub use utils::{MeshValidation, NormalMode};

use utils::Tri;

/// A mesh file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Stl,
    ThreeMf,
    Obj,
}

impl Format {
    /// Maps a file extension (without dot, any case) to a format.
    pub fn from_extension(ext: &str) -> Option<Format> {
        match ext.to_ascii_lowercase().as_str() {
            "stl" => Some(Format::Stl),
            "3mf" => Some(Format::ThreeMf),
            "obj" => Some(Format::Obj),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Format::Stl => "STL",
            Format::ThreeMf => "3MF",
            Format::Obj => "OBJ",
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Resource limits applied while importing untrusted files. Exceeding one is an error,
/// never a crash or an out-of-memory abort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Maximum input file size. Default 1 GiB.
    pub max_file_bytes: u64,
    /// Maximum number of triangles produced (after 3MF instancing). Default 50 M.
    pub max_triangles: u64,
    /// Maximum number of vertices declared in a file. Default 50 M.
    pub max_vertices: u64,
    /// Maximum number of objects / solids / build items. Default 100 000.
    pub max_objects: u64,
    /// Maximum number of ZIP entries in a 3MF package. Default 10 000.
    pub max_zip_entries: u64,
    /// Maximum total decompressed bytes read from a ZIP. Default 2 GiB.
    pub max_decompressed_bytes: u64,
    /// Maximum uncompressed:compressed ratio for entries over 1 MiB. Default 200.
    pub max_compression_ratio: u64,
    /// Maximum XML element nesting depth. Default 64.
    pub max_xml_depth: u32,
    /// Maximum attributes on one XML element. Default 128.
    pub max_xml_attributes: u32,
    /// Maximum length of one XML attribute value in bytes. Default 64 KiB.
    pub max_attribute_bytes: u64,
    /// Maximum nesting depth of 3MF components. Default 32.
    pub max_component_depth: u32,
    /// Maximum corners of one OBJ polygon. Default 65 536.
    pub max_polygon_vertices: u64,
    /// Names longer than this (bytes) are truncated. Default 1024.
    pub max_name_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_file_bytes: 1 << 30,
            max_triangles: 50_000_000,
            max_vertices: 50_000_000,
            max_objects: 100_000,
            max_zip_entries: 10_000,
            max_decompressed_bytes: 2 << 30,
            max_compression_ratio: 200,
            max_xml_depth: 64,
            max_xml_attributes: 128,
            max_attribute_bytes: 64 << 10,
            max_component_depth: 32,
            max_polygon_vertices: 65_536,
            max_name_bytes: 1024,
        }
    }
}

/// Options for [`import_bytes`] / [`import_file`].
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    pub limits: Limits,
    /// How display normals are generated. Default: crease angle 30°.
    pub normals: NormalMode,
    /// Keep degenerate triangles instead of removing them (they are removed by default).
    pub keep_degenerate: bool,
    /// Base name for meshes whose file carries no name (STL, OBJ). [`import_file`] uses
    /// the file stem when this is `None`.
    pub name_hint: Option<String>,
    /// Set to `true` from another thread to abort the import with
    /// [`MeshIoError::Cancelled`].
    pub cancel: Option<Arc<AtomicBool>>,
}

/// One imported body.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedMesh {
    pub name: String,
    /// Positions in millimetres. `face_ranges` holds one triangle range per source object
    /// (one for STL solids and OBJ objects; one per component mesh for 3MF items).
    /// `edges` is empty.
    pub mesh: Mesh,
    /// How units were interpreted, e.g. "3MF unit 'inch' converted to mm (×25.4)".
    pub units_note: String,
    /// Topology check of the imported mesh (watertight, manifold, orientation).
    pub validation: MeshValidation,
}

/// Non-fatal findings while importing (repairs, ignored content, topology problems).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportReport {
    pub warnings: Vec<String>,
}

/// Result of a successful import.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub format: Format,
    pub meshes: Vec<ImportedMesh>,
    pub report: ImportReport,
}

/// A mesh to export, positions in millimetres.
#[derive(Debug, Clone, Copy)]
pub struct ExportBody<'a> {
    pub name: &'a str,
    pub mesh: &'a Mesh,
}

/// Export target format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    StlBinary,
    StlAscii,
    ThreeMf,
    /// Wavefront OBJ without materials; `normals` also writes per-vertex `vn` lines.
    Obj {
        normals: bool,
    },
}

/// Non-fatal findings while exporting.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExportReport {
    pub warnings: Vec<String>,
    /// Triangles written.
    pub triangles: u64,
}

// ---------------------------------------------------------------------------------------
// shared import machinery

/// Cancellation check, polled every few thousand items.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cancel<'a>(Option<&'a AtomicBool>);

impl Cancel<'_> {
    pub(crate) fn check(&self) -> Result<()> {
        match self.0 {
            Some(flag) if flag.load(Ordering::Relaxed) => Err(MeshIoError::Cancelled),
            _ => Ok(()),
        }
    }
    /// Checks every 64 Ki iterations.
    #[inline]
    pub(crate) fn tick(&self, i: usize) -> Result<()> {
        if i & 0xffff == 0 { self.check() } else { Ok(()) }
    }
}

/// Raw triangles of one imported body before cleanup.
#[derive(Debug, Default)]
pub(crate) struct Soup {
    pub(crate) positions: Vec<[f32; 3]>,
    pub(crate) tris: Vec<Tri>,
    /// One range per source object, partitioning `tris`.
    pub(crate) ranges: Vec<Range<u32>>,
}

/// A single triangle range `0..n`.
pub(crate) fn one_range(n: u32) -> Vec<Range<u32>> {
    std::iter::once(0..n).collect()
}

/// Strips control characters and truncates to `max` bytes on a char boundary.
pub(crate) fn clean_name(s: &str, max: usize) -> String {
    let mut out = String::new();
    for c in s.trim().chars() {
        let c = if c.is_control() { ' ' } else { c };
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// Welds, removes degenerate triangles, validates, builds display normals and reports.
pub(crate) fn finish(
    name: String,
    soup: Soup,
    units_note: &str,
    opts: &ImportOptions,
    warnings: &mut Vec<String>,
) -> ImportedMesh {
    let Soup { positions, mut tris, mut ranges } = soup;
    let (welded, remap) = utils::weld_exact(&positions);
    drop(positions);
    for t in tris.iter_mut() {
        *t = t.map(|i| remap.get(i as usize).copied().unwrap_or(0));
    }
    drop(remap);
    if !opts.keep_degenerate {
        let removed = utils::remove_degenerate(&welded, &mut tris, &mut ranges);
        if removed > 0 {
            warnings.push(format!("{name:?}: {removed} degenerate triangles removed"));
        }
    }
    let compacted = utils::compact(&welded, &mut tris);
    drop(welded);
    let validation = utils::validate_indexed(&compacted, &tris);
    if tris.is_empty() {
        warnings.push(format!("{name:?}: contains no triangles"));
    } else {
        if validation.degenerate_triangles > 0 {
            warnings.push(format!("{name:?}: {} degenerate triangles kept", validation.degenerate_triangles));
        }
        if validation.boundary_edges > 0 {
            warnings.push(format!(
                "{name:?}: {} open (boundary) edges, the mesh is not watertight",
                validation.boundary_edges
            ));
        }
        if validation.non_manifold_edges > 0 {
            warnings.push(format!("{name:?}: non-manifold edges: {}", validation.non_manifold_edges));
        }
        if validation.inconsistent_edges > 0 {
            warnings.push(format!(
                "{name:?}: {} edges with inconsistent triangle orientation (flipped normals)",
                validation.inconsistent_edges
            ));
        }
        if validation.is_watertight() && validation.is_consistently_oriented() && validation.signed_volume < 0.0 {
            warnings.push(format!("{name:?}: mesh is inside-out (negative volume)"));
        }
    }
    let (p, n, i) = utils::build_display(compacted, &tris, opts.normals);
    ImportedMesh {
        name,
        mesh: Mesh { positions: p, normals: n, indices: i, face_ranges: ranges, edges: Vec::new() },
        units_note: units_note.to_string(),
        validation,
    }
}

// ---------------------------------------------------------------------------------------
// public import API

/// Guesses the format from content magic first (ZIP → 3MF), then the file name's
/// extension, then content heuristics (STL binary size / `solid` header, OBJ statements).
pub fn detect_format(file_name: Option<&str>, bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(b"PK\x03\x04") {
        return Some(Format::ThreeMf);
    }
    let ext =
        file_name.and_then(|n| Path::new(n).extension()).and_then(|e| e.to_str()).and_then(Format::from_extension);
    if ext.is_some() {
        return ext;
    }
    if stl::looks_like_stl(bytes) {
        return Some(Format::Stl);
    }
    if obj::looks_like_obj(bytes) {
        return Some(Format::Obj);
    }
    None
}

/// Imports a mesh file held in memory. `format` of `None` means auto-detect from content.
pub fn import_bytes(bytes: &[u8], format: Option<Format>, opts: &ImportOptions) -> Result<Imported> {
    let len = bytes.len() as u64;
    if len > opts.limits.max_file_bytes {
        return Err(error::limit("file size (bytes)", len, opts.limits.max_file_bytes));
    }
    let format = format.or_else(|| detect_format(None, bytes)).ok_or(MeshIoError::UnknownFormat)?;
    let cancel = Cancel(opts.cancel.as_deref());
    cancel.check()?;
    let mut report = ImportReport::default();
    let meshes = match format {
        Format::Stl => stl::import(bytes, opts, cancel, &mut report.warnings)?,
        Format::Obj => obj::import(bytes, opts, cancel, &mut report.warnings)?,
        Format::ThreeMf => threemf::import(bytes, opts, cancel, &mut report.warnings)?,
    };
    if meshes.iter().all(|m| m.mesh.indices.is_empty()) {
        return Err(MeshIoError::Empty { format });
    }
    Ok(Imported { format, meshes, report })
}

/// Reads and imports a file. The size limit is checked before reading, and the read is
/// capped, so a file that grows while being read cannot exceed it.
pub fn import_file(path: &Path, opts: &ImportOptions) -> Result<Imported> {
    let max = opts.limits.max_file_bytes;
    let file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len > max {
        return Err(error::limit("file size (bytes)", len, max));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    file.take(max.saturating_add(1)).read_to_end(&mut bytes)?;
    let name = path.file_name().and_then(|n| n.to_str());
    let format = detect_format(name, &bytes).ok_or(MeshIoError::UnknownFormat)?;
    let mut opts = opts.clone();
    if opts.name_hint.is_none() {
        opts.name_hint = path.file_stem().and_then(|s| s.to_str()).map(str::to_string);
    }
    import_bytes(&bytes, Some(format), &opts)
}

// ---------------------------------------------------------------------------------------
// public export API

/// Checks an export body: whole triangles, indices in range, finite coordinates.
pub(crate) fn check_body(b: &ExportBody<'_>) -> Result<()> {
    let err = |detail: String| MeshIoError::InvalidMesh { body: b.name.to_string(), detail };
    if !b.mesh.indices.len().is_multiple_of(3) {
        return Err(err(format!("index count {} is not a multiple of 3", b.mesh.indices.len())));
    }
    let n = b.mesh.positions.len();
    if let Some((k, i)) = b.mesh.indices.iter().enumerate().find(|(_, i)| **i as usize >= n) {
        return Err(err(format!("triangle {} references vertex {i} but the mesh has {n} vertices", k / 3)));
    }
    if let Some(k) = b.mesh.positions.iter().position(|p| p.iter().any(|c| !c.is_finite())) {
        return Err(err(format!("vertex {k} has a NaN or infinite coordinate")));
    }
    Ok(())
}

/// Exports `bodies` (positions in mm) to `out`. Output is deterministic.
pub fn export(bodies: &[ExportBody<'_>], format: ExportFormat, out: &mut dyn Write) -> Result<ExportReport> {
    for b in bodies {
        check_body(b)?;
    }
    match format {
        ExportFormat::StlBinary => stl::write_binary(bodies, out),
        ExportFormat::StlAscii => stl::write_ascii(bodies, out),
        ExportFormat::ThreeMf => threemf::write(bodies, out),
        ExportFormat::Obj { normals } => obj::write(bodies, normals, out),
    }
}

/// [`export`] into a byte vector.
pub fn export_to_vec(bodies: &[ExportBody<'_>], format: ExportFormat) -> Result<(Vec<u8>, ExportReport)> {
    let mut buf = Vec::new();
    let report = export(bodies, format, &mut buf)?;
    Ok((buf, report))
}
