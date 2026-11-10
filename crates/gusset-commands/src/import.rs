//! `import.file`, split so the slow part never runs on the model thread (spec: imports
//! run in the background, are cancellable and never block; code review M6).
//!
//! 1. [`begin_import`] (model thread, fast): policy checks, path checks, and opening the
//!    file safely: no symlink at the final component (`O_NOFOLLOW`), never blocking on a
//!    FIFO (`O_NONBLOCK`), and only regular files ([`open_regular`]; security review #11).
//! 2. [`ImportJob::run`] (any other thread): reads at most [`MAX_IMPORT_BYTES`] and parses.
//!    STEP/IGES go to the session's [`Importer`] (the sandboxed worker process); STL, 3MF
//!    and OBJ are parsed by gusset-meshio (pure Rust, with limits). The [`CancelFlag`] is
//!    polled throughout; the worker is killed when it is set.
//! 3. [`complete_import`] (model thread): stores the blobs and commits one undoable
//!    "Import" step, audited like any other command.
//!
//! `gusset_host::HostHandle` runs these phases for every front end (GUI, CLI, MCP).

use crate::commands::{edit, insert_feature};
use crate::error::CommandError;
use crate::registry::{self, CommandSpec};
use crate::session::{CmdCtx, Importer, Session};
use gusset_doc::{Feature, FeatureKind, ImportDef, ImportFormat};
use serde_json::{Value, json};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set to `true` (from any thread) to cancel an import.
pub type CancelFlag = Arc<AtomicBool>;

/// Largest file accepted for import.
pub const MAX_IMPORT_BYTES: u64 = 1 << 30;

/// Tier note returned for exact-geometry imports.
pub const TIER_BREP: &str = "exact geometry (B-rep, no feature history)";
/// Tier note returned for mesh imports.
pub const TIER_MESH: &str =
    "reference mesh (display, measurement and mesh export only; not a solid, features cannot use it)";

#[derive(Debug, Clone, Copy)]
enum Kind {
    Brep(ImportFormat),
    Mesh(gusset_meshio::Format),
    /// A FreeCAD document: its visible shapes, each parsed by the worker (ADR 0033).
    FreeCad,
    /// An OpenSCAD file, evaluated here and built with the kernel (ADR 0033).
    Scad,
    /// A Parasolid XT file, converted here to STEP and imported by the worker (ADR 0035).
    Xt,
    /// A SolidWorks part: its Parasolid partition is read out of the container, then as `Xt`
    /// (ADR 0043).
    Sldprt,
    /// A SolidWorks assembly: its component list, each referenced `.sldprt` read from the
    /// same folder and placed (ADR 0043 amendment).
    Sldasm,
    /// An ACIS SAT file, converted here to STEP and imported by the worker (ADR 0039).
    Sat,
    /// A Solid Edge part (`.par`, `.psm`): the Parasolid transmit inside the OLE2 container,
    /// then as `Xt` (ADR 0046).
    SolidEdge,
    /// An Inventor part (`.ipt`): the ShapeManager body inside the OLE2 container, then as
    /// `Sat` (ADR 0046).
    Inventor,
    /// A Fusion 360 archive: its binary ACIS bodies (`.smb`), as SAT (ADR 0039).
    F3d,
    /// A feature tree written by an exporter plugin (`.gtree.json`, ADR 0042).
    Tree,
}

/// Tier note for Parasolid imports.
pub const TIER_XT: &str =
    "exact geometry from the Parasolid file (B-rep, no feature history; intersection curves are fitted)";

/// Tier note for ACIS SAT imports.
pub const TIER_SAT: &str = "exact geometry from the ACIS file (B-rep, no feature history)";

/// Tier note for OpenSCAD imports.
pub const TIER_SCAD: &str = "exact geometry built from the OpenSCAD program (B-rep, its parameters are not kept)";

/// Tier note for FreeCAD imports.
pub const TIER_FREECAD: &str = "exact geometry per visible FreeCAD object (B-rep, no FreeCAD history)";

/// An opened import file, ready to be read and parsed off the model thread.
pub struct ImportJob {
    file: std::fs::File,
    kind: Kind,
    file_name: String,
    importer: Option<Arc<dyn Importer>>,
    kernel: Arc<dyn gusset_kernel::Kernel>,
    /// The folder of an assembly (the only place its parts are read from).
    dir: Option<std::path::PathBuf>,
}

impl std::fmt::Debug for ImportJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportJob").field("kind", &self.kind).finish_non_exhaustive()
    }
}

/// One parsed body, ready to be stored.
#[derive(Debug, Clone)]
struct PreparedBody {
    /// Name from inside the file (untrusted).
    name: String,
    blob: Vec<u8>,
    format: ImportFormat,
    /// Colour from the file (`0xRRGGBB`), kept as the body's appearance (ADR 0045).
    color: Option<u32>,
    /// Faces in other colours (face index, `0xRRGGBB`), from a STEP file's face colours.
    faces: Vec<(u32, u32)>,
    /// An OpenSCAD object's tree and its exact volume: written as features when it can be
    /// (ADR 0033 amendment), else imported as this body.
    scad: Option<(gusset_scad::Csg, f64)>,
}

/// A worker body name `name\u{1f}#rrggbb` as the name and its colour (ADR 0045); the name
/// is the file's text, so control, bidi and zero-width characters go and it is cut to 200.
fn split_look(raw: &str) -> (String, Option<u32>, Vec<(u32, u32)>) {
    let mut parts = raw.split('\u{1f}');
    let name = parts.next().unwrap_or_default();
    let color = parts.next().and_then(gusset_doc::text::parse_color);
    // `<face>=<rrggbb>,...` after a second separator, only with a body colour; at most
    // the document's list limit.
    // One colour per face (the first; the list is the file's text) (face colours review #7).
    let mut faces: Vec<(u32, u32)> = Vec::new();
    if let (Some(_), Some(list)) = (color, parts.next()) {
        let mut seen = std::collections::HashSet::new();
        for e in list.split(',') {
            if faces.len() >= gusset_doc::text::MAX_LIST_ITEMS {
                break;
            }
            let Some((i, c)) = e.split_once('=') else { continue };
            let (Ok(i), Some(c)) = (i.parse::<u32>(), gusset_doc::text::parse_color(&format!("#{c}"))) else {
                continue;
            };
            if seen.insert(i) {
                faces.push((i, c));
            }
        }
    }
    let name: String = name
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(*c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
        })
        .take(200)
        .collect();
    // A name of only invisible characters keeps a readable fallback (review R10).
    let name = name.trim();
    (if name.is_empty() { "Body".to_string() } else { name.to_string() }, color, faces)
}

/// A worker body as a prepared body: its name, colour and face colours split off.
fn prepared(raw: &str, blob: Vec<u8>, format: ImportFormat) -> PreparedBody {
    let (name, color, faces) = split_look(raw);
    PreparedBody { name, blob, format, color, faces, scad: None }
}

/// The worker's bodies of one STEP text, with the names and colours the Parasolid file
/// gave them when the counts agree (the STEP keeps the file's body order).
fn with_looks(bodies: Vec<(String, Vec<u8>)>, looks: &[(Option<String>, Option<u32>)]) -> Vec<PreparedBody> {
    let same = bodies.len() == looks.len();
    bodies
        .into_iter()
        .enumerate()
        .map(|(i, (name, blob))| {
            let look = if same { looks.get(i) } else { None };
            let (own, own_color, faces) = split_look(&name);
            PreparedBody {
                name: look.and_then(|l| l.0.clone()).unwrap_or(own),
                blob,
                format: ImportFormat::Step,
                // The STEP's own colour first: its face colours are relative to it (the
                // converters write face colours as styled items).
                color: own_color.or(look.and_then(|l| l.1)),
                faces,
                scad: None,
            }
        })
        .collect()
}

/// The parsed result of [`ImportJob::run`], for [`complete_import`].
#[derive(Debug, Clone)]
pub struct PreparedImport {
    bodies: Vec<PreparedBody>,
    file_name: String,
    tier: &'static str,
    warnings: Vec<String>,
    /// Set for a `.gtree.json`: features to add instead of bodies.
    tree: Option<crate::tree_import::Tree>,
}

fn cancelled() -> CommandError {
    CommandError::new("cancelled", "the import was cancelled")
}

/// Opens `path` for reading only if it is a regular file: symlinks at the final
/// component are not followed, FIFOs and devices are refused without blocking, and the
/// type is checked on the opened handle (no check-then-open race).
///
/// # Errors
/// `not_found`, `not_allowed` (not a regular file) or `invalid_args` (too large).
/// Messages carry `shown`, never the absolute path.
pub fn open_regular(path: &Path, shown: &str, max: u64) -> Result<std::fs::File, CommandError> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: open a link itself, so the type check refuses it.
        o.custom_flags(0x0020_0000);
    }
    let shown = crate::paths::display_name(shown);
    let f = o.open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => CommandError::not_found(format!("'{shown}' does not exist")),
        _ => CommandError::new("not_allowed", format!("'{shown}' cannot be opened for import ({})", e.kind()))
            .hint("only regular files can be imported, not links, pipes or devices"),
    })?;
    let meta = f.metadata().map_err(|e| CommandError::new("io", format!("'{shown}': {}", e.kind())))?;
    if !meta.is_file() {
        return Err(CommandError::new("not_allowed", format!("'{shown}' is not a regular file"))
            .hint("only regular files can be imported, not links, pipes or devices"));
    }
    if meta.len() > max {
        return Err(CommandError::args(format!("'{shown}' is larger than {} MiB", max >> 20)));
    }
    Ok(f)
}

/// Reads at most `max` bytes (a file that grows past it is an error), checking `cancel`.
pub(crate) fn read_capped(mut f: std::fs::File, max: u64, cancel: &AtomicBool) -> Result<Vec<u8>, CommandError> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let n = match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(CommandError::new("io", format!("reading the file failed ({})", e.kind()))),
        };
        out.extend_from_slice(buf.get(..n).unwrap_or_default());
        if out.len() as u64 > max {
            return Err(CommandError::args(format!("the file is larger than {} MiB", max >> 20)));
        }
    }
    Ok(out)
}

/// Phase 1 without policy checks (used by `ImportFile::run` inside `execute`).
pub(crate) fn open_import(s: &Session, ctx: &CmdCtx, path: &str) -> Result<ImportJob, CommandError> {
    let full = crate::paths::read_target(s.project_dir.as_deref(), path, &ctx.actor)?;
    let ext = full.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let kind = match ext.as_str() {
        "step" | "stp" => Kind::Brep(ImportFormat::Step),
        "iges" | "igs" => Kind::Brep(ImportFormat::Iges),
        "fcstd" => Kind::FreeCad,
        "x_t" | "x_b" | "xmt_txt" | "xmt_bin" => Kind::Xt,
        "sldprt" => Kind::Sldprt,
        "sldasm" => Kind::Sldasm,
        "sat" | "sab" => Kind::Sat,
        "par" | "psm" => Kind::SolidEdge,
        "ipt" => Kind::Inventor,
        "f3d" | "f3z" => Kind::F3d,
        "scad" => Kind::Scad,
        "json" => Kind::Tree,
        other => match gusset_meshio::Format::from_extension(other) {
            Some(f) => Kind::Mesh(f),
            None => {
                return Err(CommandError::args(format!("'.{other}' files cannot be imported"))
                    .hint("supported: .step, .stp, .iges, .igs, .x_t, .x_b, .sldprt, .sldasm, .par, .psm, .ipt, .sat, .sab, .f3d, .f3z, .fcstd, .scad, .gtree.json, .stl, .3mf, .obj, .ply, .gltf, .glb"));
            }
        },
    };
    let importer = s.importer.clone();
    if matches!(
        kind,
        Kind::Brep(_)
            | Kind::FreeCad
            | Kind::Xt
            | Kind::Sldprt
            | Kind::Sldasm
            | Kind::Sat
            | Kind::F3d
            | Kind::SolidEdge
            | Kind::Inventor
    ) && importer.is_none()
    {
        return Err(CommandError::new(
            "not_allowed",
            "STEP/IGES/Parasolid/FreeCAD import is not available in this session",
        )
        .hint("the sandboxed import worker (gusset-worker) was not found next to the program"));
    }
    let file = open_regular(&full, path, MAX_IMPORT_BYTES)?;
    let file_name = full.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = matches!(kind, Kind::Sldasm).then(|| full.parent().map(Path::to_path_buf)).flatten();
    Ok(ImportJob { file, kind, file_name, importer, kernel: s.kernel().clone(), dir })
}

impl ImportJob {
    /// Phase 2: reads and parses the file. Call it off the model thread; it may take
    /// long (the worker's timeout bounds STEP/IGES). Setting `cancel` stops it.
    ///
    /// # Errors
    /// `cancelled`, `io` (parse or worker failures), `invalid_args` (too large).
    pub fn run(self, cancel: &CancelFlag) -> Result<PreparedImport, CommandError> {
        let cap =
            if matches!(self.kind, Kind::Tree) { crate::tree_import::MAX_TREE_BYTES as u64 } else { MAX_IMPORT_BYTES };
        let bytes = read_capped(self.file, cap, cancel)?;
        if matches!(self.kind, Kind::Tree) {
            let tree = crate::tree_import::parse(&bytes)?;
            return Ok(PreparedImport {
                bodies: Vec::new(),
                file_name: self.file_name,
                tier: crate::tree_import::TIER,
                warnings: Vec::new(),
                tree: Some(tree),
            });
        }
        let (bodies, tier, warnings) = match self.kind {
            Kind::Brep(format) => {
                let importer = self
                    .importer
                    .ok_or_else(|| CommandError::new("not_allowed", "STEP/IGES import is not available"))?;
                let bodies = importer.import(format.keyword(), &bytes, cancel).map_err(|e| {
                    if cancel.load(Ordering::Relaxed) { cancelled() } else { CommandError::new("io", e) }
                })?;
                let bodies = bodies.into_iter().map(|(name, blob)| prepared(&name, blob, format));
                (bodies.collect::<Vec<_>>(), TIER_BREP, Vec::new())
            }
            Kind::Scad => scad(&bytes, &self.kernel, cancel)?,
            Kind::Xt => {
                let importer = self
                    .importer
                    .ok_or_else(|| CommandError::new("not_allowed", "Parasolid import is not available"))?;
                let stem = Path::new(&self.file_name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let c = gusset_xt::to_step(&bytes, &stem, &gusset_xt::Limits::default())
                    .map_err(|e| CommandError::new("io", format!("Parasolid: {e}")))?;
                let bodies = importer.import("step", c.step.as_bytes(), cancel).map_err(|e| {
                    if cancel.load(Ordering::Relaxed) { cancelled() } else { CommandError::new("io", e) }
                })?;
                let mut warnings = c.warnings;
                warnings.truncate(50);
                (with_looks(bodies, &c.looks), TIER_XT, warnings)
            }
            Kind::Sldprt => {
                let importer = self
                    .importer
                    .ok_or_else(|| CommandError::new("not_allowed", "SolidWorks import is not available"))?;
                let stem = Path::new(&self.file_name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let (out, mut warnings) = sldprt_bodies(&bytes, &stem, importer.as_ref(), cancel, None)?;
                // The feature history as editable features, when it rebuilds to the part's
                // own bodies (ADR 0043 amendment); else the bodies themselves.
                let stock = sldprt_stock(&bytes, &stem, importer.as_ref(), cancel);
                match solidworks_history(&bytes, &out, &self.kernel, &self.file_name, stock) {
                    Ok((tree, note)) => {
                        warnings.extend(note);
                        return Ok(PreparedImport {
                            bodies: Vec::new(),
                            file_name: self.file_name,
                            tier: TIER_SOLIDWORKS_HISTORY,
                            warnings,
                            tree: Some(tree),
                        });
                    }
                    Err(why) if why.is_empty() => {}
                    Err(why) => warnings.push(format!("the design history came in as exact bodies: {why}")),
                }
                (out, TIER_XT, warnings)
            }
            Kind::Sldasm => {
                let importer = self
                    .importer
                    .ok_or_else(|| CommandError::new("not_allowed", "SolidWorks import is not available"))?;
                let dir =
                    self.dir.ok_or_else(|| CommandError::new("not_allowed", "the assembly's folder is not known"))?;
                sldasm(&bytes, &dir, importer.as_ref(), cancel)?
            }
            Kind::Sat => {
                let importer =
                    self.importer.ok_or_else(|| CommandError::new("not_allowed", "ACIS import is not available"))?;
                let stem = Path::new(&self.file_name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let c = gusset_sat::to_step(&bytes, &stem, &gusset_sat::Limits::default())
                    .map_err(|e| CommandError::new("io", format!("ACIS: {e}")))?;
                let bodies = importer.import("step", c.step.as_bytes(), cancel).map_err(|e| {
                    if cancel.load(Ordering::Relaxed) { cancelled() } else { CommandError::new("io", e) }
                })?;
                let bodies = bodies.into_iter().map(|(name, blob)| prepared(&name, blob, ImportFormat::Step));
                let mut warnings = c.warnings;
                warnings.truncate(50);
                (bodies.collect::<Vec<_>>(), TIER_SAT, warnings)
            }
            Kind::SolidEdge | Kind::Inventor => {
                let importer = self
                    .importer
                    .ok_or_else(|| CommandError::new("not_allowed", "Solid Edge/Inventor import is not available"))?;
                let stem = Path::new(&self.file_name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if matches!(self.kind, Kind::SolidEdge) {
                    solid_edge_part(&bytes, &stem, importer.as_ref(), cancel)?
                } else {
                    let (out, tier, mut warnings) = inventor_part(&bytes, &stem, importer.as_ref(), cancel)?;
                    // The feature history as editable features, when it rebuilds to the part's
                    // own body (ADR 0046 amendment); else the body itself.
                    match inventor_history(&bytes, &out, &self.kernel, &self.file_name) {
                        Ok((tree, note)) => {
                            warnings.extend(note);
                            return Ok(PreparedImport {
                                bodies: Vec::new(),
                                file_name: self.file_name,
                                tier: TIER_INVENTOR_HISTORY,
                                warnings,
                                tree: Some(tree),
                            });
                        }
                        Err(why) if why.is_empty() => {}
                        Err(why) => warnings.push(format!("the design history came in as an exact body: {why}")),
                    }
                    (out, tier, warnings)
                }
            }
            Kind::F3d => {
                let importer =
                    self.importer.ok_or_else(|| CommandError::new("not_allowed", "Fusion import is not available"))?;
                let (out, tier, mut warnings) = f3d(&bytes, importer.as_ref(), cancel)?;
                // The design timeline as editable features, when it rebuilds to the bodies the
                // file stores (ADR 0039 amendment); else the bodies themselves.
                match fusion_history(&bytes, &out, &self.kernel, &self.file_name, importer.as_ref(), cancel) {
                    Ok((tree, note)) => {
                        // The warnings of the exact-body import (placements, repeats) do not
                        // describe the features; the surfaces it leaves out still hold.
                        warnings.retain(|w| w.contains("sheet (surface)"));
                        warnings.extend(note);
                        return Ok(PreparedImport {
                            bodies: Vec::new(),
                            file_name: self.file_name,
                            tier: TIER_FUSION_HISTORY,
                            warnings,
                            tree: Some(tree),
                        });
                    }
                    Err(why) if why.is_empty() => {}
                    Err(why) => warnings.push(format!("the design history came in as exact bodies: {why}")),
                }
                (out, tier, warnings)
            }
            Kind::Tree => return Err(CommandError::args("a feature tree has no bodies")),
            Kind::FreeCad => {
                let importer =
                    self.importer.ok_or_else(|| CommandError::new("not_allowed", "FreeCAD import is not available"))?;
                let (bodies, tier, mut warnings) = freecad(&bytes, importer.as_ref(), cancel)?;
                // The PartDesign history as editable features, when it rebuilds to the
                // shapes FreeCAD stored (ADR 0033 amendment); else the shapes themselves.
                match freecad_history(&bytes, &bodies, &self.kernel, importer.as_ref(), cancel, &self.file_name) {
                    Ok((tree, note)) => {
                        warnings.extend(note);
                        return Ok(PreparedImport {
                            bodies: Vec::new(),
                            file_name: self.file_name,
                            tier: TIER_FREECAD_HISTORY,
                            warnings,
                            tree: Some(tree),
                        });
                    }
                    Err(why) if why.is_empty() => {}
                    Err(why) => warnings.push(format!("the design history came in as exact bodies: {why}")),
                }
                (bodies, tier, warnings)
            }
            Kind::Mesh(format) => {
                let opts = gusset_meshio::ImportOptions {
                    name_hint: Path::new(&self.file_name).file_stem().map(|s| s.to_string_lossy().into_owned()),
                    cancel: Some(cancel.clone()),
                    ..Default::default()
                };
                let imported = gusset_meshio::import_bytes(&bytes, Some(format), &opts).map_err(|e| match e {
                    gusset_meshio::MeshIoError::Cancelled => cancelled(),
                    other => CommandError::new("io", format!("the {format} file cannot be imported: {other}")),
                })?;
                let mut out = Vec::new();
                for m in imported.meshes.iter().filter(|m| !m.mesh.indices.is_empty()) {
                    // Stored as binary STL: compact, and the evaluator re-reads it with
                    // the same limits. A mesh with face colours is stored as 3MF, which keeps
                    // them (dogfooding F15-2).
                    let coloured = !m.face_colors.is_empty();
                    let body = gusset_meshio::ExportBody {
                        name: &m.name,
                        mesh: &m.mesh,
                        color: coloured.then(|| m.color.unwrap_or(0x00b4_bcc8)),
                        face_colors: &m.face_colors,
                    };
                    let (store, format) = if coloured {
                        (gusset_meshio::ExportFormat::ThreeMf, ImportFormat::ThreeMf)
                    } else {
                        (gusset_meshio::ExportFormat::StlBinary, ImportFormat::Stl)
                    };
                    let (blob, _) = gusset_meshio::export_to_vec(&[body], store)
                        .map_err(|e| CommandError::new("io", format!("storing the mesh failed: {e}")))?;
                    out.push(PreparedBody {
                        name: m.name.clone(),
                        blob,
                        format,
                        color: m.color,
                        faces: Vec::new(),
                        scad: None,
                    });
                }
                (out, TIER_MESH, imported.report.warnings)
            }
        };
        if bodies.is_empty() {
            return Err(CommandError::new("io", "the file contains no bodies"));
        }
        Ok(PreparedImport { bodies, file_name: self.file_name, tier, warnings, tree: None })
    }
}

/// Tier note for Solid Edge parts.
pub const TIER_SOLID_EDGE: &str = "exact geometry from the Solid Edge part's Parasolid data (B-rep, no feature history; intersection curves are fitted)";

/// Tier note for Inventor parts.
pub const TIER_INVENTOR: &str = "exact geometry from the Inventor part's ShapeManager body (B-rep, no feature history)";

/// A Solid Edge part (`.par`, `.psm`): the Parasolid transmit of its `PARASOLID` storage, as
/// for `.x_t` (ADR 0046).
fn solid_edge_part(
    bytes: &[u8],
    stem: &str,
    importer: &dyn Importer,
    cancel: &CancelFlag,
) -> Result<(Vec<PreparedBody>, &'static str, Vec<String>), CommandError> {
    let part = gusset_ole::solidedge::extract_xt(bytes, &gusset_ole::Limits::default())
        .map_err(|e| CommandError::new("io", format!("Solid Edge: {e}")))?;
    let c = gusset_xt::to_step(&part.xt, stem, &gusset_xt::Limits::default())
        .map_err(|e| CommandError::new("io", format!("Solid Edge (Parasolid): {e}")))?;
    let bodies = importer
        .import("step", c.step.as_bytes(), cancel)
        .map_err(|e| if cancel.load(Ordering::Relaxed) { cancelled() } else { CommandError::new("io", e) })?;
    let mut warnings = part.warnings;
    warnings.extend(c.warnings);
    warnings.truncate(50);
    Ok((with_looks(bodies, &c.looks), TIER_SOLID_EDGE, warnings))
}

/// An Inventor part (`.ipt`): the ShapeManager body of its result, as for a Fusion `.smb`
/// (ADR 0046).
fn inventor_part(
    bytes: &[u8],
    stem: &str,
    importer: &dyn Importer,
    cancel: &CancelFlag,
) -> Result<(Vec<PreparedBody>, &'static str, Vec<String>), CommandError> {
    let part = gusset_ole::inventor::extract_sab(bytes, &gusset_ole::Limits::default())
        .map_err(|e| CommandError::new("io", format!("Inventor: {e}")))?;
    let c = gusset_sat::to_step(&part.sab, stem, &gusset_sat::Limits::default())
        .map_err(|e| CommandError::new("io", format!("Inventor (ShapeManager): {e}")))?;
    let bodies = importer
        .import("step", c.step.as_bytes(), cancel)
        .map_err(|e| if cancel.load(Ordering::Relaxed) { cancelled() } else { CommandError::new("io", e) })?;
    let bodies = bodies.into_iter().map(|(name, blob)| prepared(&name, blob, ImportFormat::Step));
    let mut warnings = part.warnings;
    warnings.extend(c.warnings);
    warnings.truncate(50);
    Ok((bodies.collect(), TIER_INVENTOR, warnings))
}

/// The bodies of one SolidWorks part (`bytes`), moved by `place` (metres, as stored) when
/// given, and the warnings. The final partition, else the bodies of an imported feature.
fn sldprt_bodies(
    bytes: &[u8],
    stem: &str,
    importer: &dyn Importer,
    cancel: &CancelFlag,
    place: Option<&gusset_xt::Xf>,
) -> Result<(Vec<PreparedBody>, Vec<String>), CommandError> {
    let limits = gusset_sldprt::Limits::default();
    let sw = |e: gusset_sldprt::SldprtError| CommandError::new("io", format!("SolidWorks: {e}"));
    let xt_err = |e: gusset_xt::XtError| CommandError::new("io", format!("SolidWorks (Parasolid): {e}"));
    // The final partition; a part made by an imported feature keeps its bodies
    // in that feature's `LocalBodies` instead (one transmit each).
    // `failure`: why the final partition gave no body, reported if nothing else does.
    let (transmits, mut warnings, failure) = match gusset_sldprt::extract_xt(bytes, &limits) {
        Ok(part) => match gusset_xt::to_step_placed(&part.xt, stem, &gusset_xt::Limits::default(), place) {
            Ok(c) if c.bodies > 0 => {
                let mut w = part.warnings;
                w.extend(c.warnings);
                (vec![(c.step, c.looks)], w, None)
            }
            Ok(_) => (Vec::new(), part.warnings, None),
            Err(e) => (Vec::new(), part.warnings, Some(xt_err(e))),
        },
        Err(e @ gusset_sldprt::SldprtError::NoGeometry(_)) => (Vec::new(), Vec::new(), Some(sw(e))),
        Err(e) => return Err(sw(e)),
    };
    let mut out = Vec::new();
    for (step, looks) in &transmits {
        let bodies = importer
            .import("step", step.as_bytes(), cancel)
            .map_err(|e| if cancel.load(Ordering::Relaxed) { cancelled() } else { CommandError::new("io", e) })?;
        out.extend(with_looks(bodies, looks));
    }
    if transmits.is_empty() {
        let locals = gusset_sldprt::local_bodies(bytes, &limits, cancel)
            .map_err(|e| if cancel.load(Ordering::Relaxed) { cancelled() } else { sw(e) })?;
        if locals.is_empty() {
            return Err(failure.unwrap_or_else(|| {
                sw(gusset_sldprt::SldprtError::NoGeometry("no configuration has a solid body".into()))
            }));
        }
        warnings.push(format!(
                "the part's {} bodies come from its imported feature; feature_combine unites them, feature_group folds them in the timeline",
                locals.len()
            ));
        // The final partition failed to read: say why, the bodies above still come in
        // (review R14).
        if let Some(f) = &failure {
            warnings.push(format!("the part's final body was not read: {}", f.message));
        }
        // One body at a time, its STEP text dropped before the next, under one byte
        // budget for all of them (security review T1).
        let mut step_bytes = 0usize;
        for (i, xt) in locals.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(cancelled());
            }
            let c = gusset_xt::to_step_placed(xt, &format!("{stem}_{}", i + 1), &gusset_xt::Limits::default(), place)
                .map_err(xt_err)?;
            step_bytes = step_bytes.saturating_add(c.step.len());
            if step_bytes > MAX_SLDPRT_STEP_BYTES {
                return Err(CommandError::new(
                    "invalid_args",
                    format!("the part's bodies exceed {} MiB of geometry", MAX_SLDPRT_STEP_BYTES >> 20),
                ));
            }
            let room = 50usize.saturating_sub(warnings.len());
            warnings.extend(c.warnings.into_iter().take(room));
            let bodies = importer
                .import("step", c.step.as_bytes(), cancel)
                .map_err(|e| if cancel.load(Ordering::Relaxed) { cancelled() } else { CommandError::new("io", e) })?;
            out.extend(with_looks(bodies, &c.looks));
        }
    }
    warnings.truncate(50);
    Ok((out, warnings))
}

/// Most component instances (counting sub-assembly contents) one assembly import places.
const MAX_SLDASM_INSTANCES: usize = 2000;
/// Most components one assembly import looks at, through all its sub-assemblies (S1).
const MAX_SLDASM_WALK: usize = 100_000;
/// Deepest chain of sub-assembly files followed.
const MAX_SLDASM_DEPTH: usize = 4;
/// Most bytes of part files read from the assembly's folder.
const MAX_SLDASM_READ_BYTES: u64 = 2 << 30;
/// Most directory entries looked at when matching a file name.
const MAX_SLDASM_DIR_ENTRIES: usize = 100_000;

/// `a(b(x))` for rigid placements `x' = R x + t`.
fn compose_xf(a: &gusset_xt::Xf, b: &gusset_xt::Xf) -> gusset_xt::Xf {
    let at = |m: &[[f64; 3]; 3], i: usize, j: usize| m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
    let mut r = [[0.0; 3]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| at(&a.0, i, k) * at(&b.0, k, j)).sum();
        }
    }
    let mut t = a.1;
    for (i, v) in t.iter_mut().enumerate() {
        *v += (0..3).map(|k| at(&a.0, i, k) * b.1.get(k).copied().unwrap_or(0.0)).sum::<f64>();
    }
    (r, t)
}

/// A component's stored 4x4 as a rigid placement, if it is one (a rotation: orthonormal,
/// determinant +1; a mirror or a scale is not).
fn rigid(m: &[[f64; 4]; 4]) -> Option<gusset_xt::Xf> {
    let at = |i: usize, j: usize| m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
    let r = [0, 1, 2].map(|i| [0, 1, 2].map(|j| at(i, j)));
    let t = [0, 1, 2].map(|i| at(i, 3));
    let col = |j: usize| [0, 1, 2].map(|i| at(i, j));
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (x, y, z) = (col(0), col(1), col(2));
    let cross = [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
    let ok = (dot(x, x) - 1.0).abs() < 1e-6
        && (dot(y, y) - 1.0).abs() < 1e-6
        && (dot(z, z) - 1.0).abs() < 1e-6
        && dot(x, y).abs() < 1e-6
        && (dot(cross, z) - 1.0).abs() < 1e-6
        && t.iter().all(|v| v.is_finite() && v.abs() < 1e6);
    ok.then_some((r, t))
}

/// Reads parts out of the folder of the assembly, never from anywhere else: a stored name is
/// reduced to a plain file name by the reader, matched against the folder's own listing
/// (exact, then ignoring case, then with spaces and underscores alike: downloads often turn
/// one into the other) and opened without following links.
struct Folder<'a> {
    dir: &'a Path,
    /// The folder's names by exact name, by lower case and by lower case with spaces as
    /// underscores (first in sorted order wins), built once (security review S7).
    listing: Option<FolderIndex>,
    read: u64,
    cancel: &'a CancelFlag,
}

#[derive(Default)]
struct FolderIndex {
    exact: std::collections::HashSet<String>,
    lower: std::collections::HashMap<String, String>,
    loose: std::collections::HashMap<String, String>,
}

impl Folder<'_> {
    fn find(&mut self, name: &str) -> Option<String> {
        let dir = self.dir;
        let index = self.listing.get_or_insert_with(|| {
            let mut v: Vec<String> = std::fs::read_dir(dir)
                .map(|rd| {
                    rd.filter_map(Result::ok)
                        .take(MAX_SLDASM_DIR_ENTRIES)
                        .filter_map(|e| e.file_name().into_string().ok())
                        .collect()
                })
                .unwrap_or_default();
            v.sort();
            let mut ix = FolderIndex::default();
            for n in v {
                ix.lower.entry(n.to_ascii_lowercase()).or_insert_with(|| n.clone());
                ix.loose.entry(n.to_lowercase().replace(' ', "_")).or_insert_with(|| n.clone());
                ix.exact.insert(n);
            }
            ix
        });
        if index.exact.contains(name) {
            return Some(name.to_string());
        }
        index
            .lower
            .get(&name.to_ascii_lowercase())
            .or_else(|| index.loose.get(&name.to_lowercase().replace(' ', "_")))
            .cloned()
    }

    fn load(&mut self, name: &str) -> Result<Vec<u8>, CommandError> {
        let plain = !name.is_empty() && !name.contains(['/', '\\', ':', '\0']) && name != "." && name != "..";
        // Names come from the file: control characters out and cut for messages.
        let shown: String = name.chars().filter(|c| !c.is_control()).take(120).collect();
        if !plain {
            return Err(CommandError::args(format!("'{shown}' is not a plain file name")));
        }
        // Only SolidWorks files: a component cannot make the import probe or read other files
        // next to the assembly (security review S8).
        let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        if !matches!(ext.as_str(), "sldprt" | "sldasm") {
            return Err(CommandError::args(format!("'{shown}' is not a SolidWorks part or assembly file")));
        }
        let actual =
            self.find(name).ok_or_else(|| CommandError::not_found(format!("'{shown}' is not in the folder")))?;
        let f = open_regular(&self.dir.join(&actual), &actual, MAX_IMPORT_BYTES)?;
        self.read = self.read.saturating_add(f.metadata().map(|m| m.len()).unwrap_or(MAX_IMPORT_BYTES));
        if self.read > MAX_SLDASM_READ_BYTES {
            return Err(CommandError::args("the assembly's parts add up to more than 2 GiB"));
        }
        read_capped(f, MAX_IMPORT_BYTES, self.cancel)
    }
}

/// A SolidWorks assembly: the components listed in the file, each part file read from the same
/// folder and imported at its instance placement (a part used n times gives n sets of bodies,
/// named after the component; a virtual component's part is read from the assembly file itself).
/// Missing, unreadable, suppressed and hidden components are warnings, not errors.
fn sldasm(
    bytes: &[u8],
    dir: &Path,
    importer: &dyn Importer,
    cancel: &CancelFlag,
) -> Result<(Vec<PreparedBody>, &'static str, Vec<String>), CommandError> {
    use gusset_sldprt::ComponentKind;
    let limits = gusset_sldprt::Limits::default();
    let top = gusset_sldprt::read_assembly(bytes, &limits)
        .map_err(|e| CommandError::new("io", format!("SolidWorks assembly: {e}")))?;
    let mut folder = Folder { dir, listing: None, read: 0, cancel };
    let mut warnings = top.warnings.clone();
    let (mut out, mut kept_bytes, mut instances) = (Vec::<PreparedBody>::new(), 0usize, 0usize);
    let (mut suppressed, mut hidden, mut mirrored) = (0usize, 0usize, 0usize);
    let mut truncated = false;
    // Each assembly with the placement of the one that holds it, depth first.
    let identity: gusset_xt::Xf = ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0; 3]);
    // (assembly, holder's placement, depth, name prefix, the assembly file's bytes)
    type Pending = (Arc<gusset_sldprt::Assembly>, gusset_xt::Xf, usize, String, Arc<Vec<u8>>);
    let mut work: Vec<Pending> = vec![(Arc::new(top), identity, 0, String::new(), Arc::new(bytes.to_vec()))];
    // Part files read once each, sub-assemblies parsed once each, and a part that failed to
    // convert is not tried again for its other instances (security review S1, S4).
    let mut parts: std::collections::HashMap<String, Option<Arc<Vec<u8>>>> = std::collections::HashMap::new();
    let mut subs: std::collections::HashMap<String, Option<Arc<gusset_sldprt::Assembly>>> =
        std::collections::HashMap::new();
    let mut failed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut embedded_cache: std::collections::HashMap<String, Arc<Vec<u8>>> = std::collections::HashMap::new();
    // Every component looked at counts, suppressed and hidden ones too: nesting cannot
    // multiply the walk past this (S1).
    let mut walked = 0usize;
    while let Some((asm, parent, depth, prefix, asm_bytes)) = work.pop() {
        for c in &asm.components {
            if cancel.load(Ordering::Relaxed) {
                return Err(cancelled());
            }
            walked += 1;
            if walked > MAX_SLDASM_WALK {
                truncated = true;
                break;
            }
            let name = format!("{prefix}{}", c.name);
            if c.suppressed {
                suppressed += 1;
                continue;
            }
            if c.hidden {
                hidden += 1;
                continue;
            }
            let Some(local) = rigid(&c.placement) else {
                mirrored += 1;
                continue;
            };
            let world = compose_xf(&parent, &local);
            instances += 1;
            if instances > MAX_SLDASM_INSTANCES {
                truncated = true;
                break;
            }
            // A virtual component's part is a stream of this assembly file.
            let embedded: Option<Arc<Vec<u8>>> = if c.kind == ComponentKind::Virtual {
                // Inflated once per stream of each assembly file (S4).
                let stream_key =
                    format!("{:p}\u{0}{}", Arc::as_ptr(&asm_bytes), c.embedded.as_deref().unwrap_or_default());
                // Shared, not copied, on a cache hit (code review R4).
                let got: Option<Result<Arc<Vec<u8>>, String>> = match embedded_cache.get(&stream_key) {
                    Some(b) => Some(Ok(Arc::clone(b))),
                    None => match c.embedded.as_deref().map(|st| gusset_sldprt::embedded_part(&asm_bytes, &limits, st))
                    {
                        Some(Ok(Some(b))) => {
                            let b = Arc::new(b);
                            embedded_cache.insert(stream_key, Arc::clone(&b));
                            Some(Ok(b))
                        }
                        Some(Err(e)) => Some(Err(e.to_string())),
                        _ => None,
                    },
                };
                match got {
                    Some(Ok(b)) => Some(b),
                    other => {
                        let why = match other {
                            Some(Err(e)) => e,
                            _ => "its part is not stored in the assembly".to_string(),
                        };
                        warnings.push(format!("virtual component '{name}' not imported: {why}"));
                        continue;
                    }
                }
            } else {
                None
            };
            if embedded.is_none() && !parts.contains_key(&c.file) {
                let loaded = match folder.load(&c.file) {
                    Ok(b) => Some(Arc::new(b)),
                    Err(e) if e.code == "cancelled" => return Err(e),
                    Err(e) => {
                        warnings.push(format!(
                            "component '{name}': {} ({})",
                            crate::paths::display_name(&c.file),
                            e.message
                        ));
                        None
                    }
                };
                parts.insert(c.file.clone(), loaded);
            }
            let file = match &embedded {
                Some(b) => b,
                None => match parts.get(&c.file) {
                    Some(Some(b)) => b,
                    _ => continue,
                },
            };
            if c.kind == ComponentKind::Assembly {
                if depth + 1 > MAX_SLDASM_DEPTH {
                    warnings.push(format!(
                        "sub-assembly '{name}' is nested deeper than {MAX_SLDASM_DEPTH} levels; left out"
                    ));
                    continue;
                }
                let sub = subs
                    .entry(c.file.clone())
                    .or_insert_with(|| match gusset_sldprt::read_assembly(file, &limits) {
                        Ok(sub) => Some(Arc::new(sub)),
                        Err(e) => {
                            warnings.push(format!("sub-assembly '{name}': {e}"));
                            None
                        }
                    })
                    .clone();
                if let Some(sub) = sub {
                    work.push((sub, world, depth + 1, format!("{name}/"), Arc::clone(file)));
                }
                continue;
            }
            // The same part (or embedded stream) that failed once fails again.
            let key = match &c.embedded {
                Some(st) if c.kind == ComponentKind::Virtual => format!("{}\u{0}{st}", c.file),
                _ => c.file.clone(),
            };
            if failed.contains(&key) {
                continue;
            }
            match sldprt_bodies(file, &name, importer, cancel, Some(&world)) {
                Ok((bodies, w)) => {
                    let many = bodies.len() > 1;
                    for (i, mut b) in bodies.into_iter().enumerate() {
                        kept_bytes += b.blob.len();
                        b.name = if many { format!("{name} {}", i + 1) } else { name.clone() };
                        out.push(b);
                    }
                    let room = 50usize.saturating_sub(warnings.len());
                    warnings.extend(w.into_iter().take(room).map(|m| format!("component '{name}': {m}")));
                }
                Err(e) if cancel.load(Ordering::Relaxed) || e.code == "cancelled" => return Err(cancelled()),
                Err(e) => {
                    warnings.push(format!("component '{name}' not imported: {}", e.message));
                    failed.insert(key);
                }
            }
            if kept_bytes > MAX_F3D_OUTPUT_BYTES {
                return Err(CommandError::new(
                    "invalid_args",
                    format!("the assembly's bodies exceed {} MiB of shapes", MAX_F3D_OUTPUT_BYTES >> 20),
                ));
            }
        }
        if truncated {
            break;
        }
    }
    let mut notes = Vec::new();
    if suppressed > 0 {
        notes.push(format!("{suppressed} suppressed components left out"));
    }
    if hidden > 0 {
        notes.push(format!("{hidden} hidden components left out"));
    }
    if mirrored > 0 {
        notes.push(format!("{mirrored} components with a mirrored or scaled placement are not imported"));
    }
    if truncated {
        notes.push(format!("only the first {MAX_SLDASM_INSTANCES} component instances are imported"));
    }
    notes.append(&mut warnings);
    notes.truncate(50);
    if out.is_empty() {
        // The reasons quote names from the file: data, in a `_user_text` field (S5).
        let mut e = CommandError::new("io", "no component of the assembly could be imported");
        e.details = Some(serde_json::json!({ "warnings_user_text": notes }));
        return Err(e);
    }
    Ok((out, TIER_XT, notes))
}

/// An OpenSCAD program: one exact body per top-level statement.
fn scad(
    bytes: &[u8],
    kernel: &Arc<dyn gusset_kernel::Kernel>,
    cancel: &CancelFlag,
) -> Result<(Vec<PreparedBody>, &'static str, Vec<String>), CommandError> {
    let k = kernel.as_ref();
    let src = std::str::from_utf8(bytes).map_err(|_| CommandError::new("io", "the .scad file is not UTF-8 text"))?;
    let model = gusset_scad::read_cancellable(src, &gusset_scad::Limits::default(), Some(cancel)).map_err(|e| {
        if cancel.load(Ordering::Relaxed) {
            return cancelled();
        }
        let mut c = CommandError::new("io", format!("OpenSCAD: {e}"));
        if let Some(t) = e.user_text {
            c.details = Some(serde_json::json!({ "message_user_text": t }));
        }
        c
    })?;
    if model.objects.is_empty() {
        return Err(CommandError::new("io", "the OpenSCAD file makes no solid object"));
    }
    let mut out = Vec::new();
    let mut notes = Vec::new();
    for (i, c) in model.objects.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        // An object that cannot be built is skipped with its reason; the rest still come in
        // (dogfooding F17-2).
        let shape = match crate::scad_build::build(k, c, cancel) {
            Ok(shape) => shape,
            Err(_) if cancel.load(Ordering::Relaxed) => return Err(cancelled()),
            Err(e) => {
                notes.push(format!("Object {}: skipped ({e})", i + 1));
                continue;
            }
        };
        let blob = k.write_native(&shape).map_err(|e| CommandError::new("io", e.message))?;
        // The object as features (ADR 0033 amendment) when they rebuild to the exact build's
        // volume; else it stays this exact body.
        let volume = k.mass_properties(&shape).map(|m| m.volume).unwrap_or(f64::NAN);
        // Each check rebuilds the object's features: past a few hundred objects the rest come
        // in as exact bodies (security review S3).
        let checked = if i >= MAX_SCAD_FEATURE_OBJECTS {
            if i == MAX_SCAD_FEATURE_OBJECTS {
                notes.push(format!("objects after the first {MAX_SCAD_FEATURE_OBJECTS} come in as exact bodies"));
            }
            Err(String::new())
        } else {
            crate::scad_text::check(c, kernel.clone(), volume, k.bbox(&shape).ok())
        };
        let as_features = match checked {
            Err(why) if why.is_empty() => None,
            Ok(()) => Some((c.clone(), volume)),
            Err(why) => {
                notes.push(format!("Object {}: imported as one exact body ({why})", i + 1));
                None
            }
        };
        out.push(PreparedBody {
            name: format!("Object {}", i + 1),
            blob,
            format: ImportFormat::Scad,
            color: None,
            faces: Vec::new(),
            scad: as_features,
        });
    }
    if out.is_empty() {
        let mut e = CommandError::new("io", "no object of the OpenSCAD file could be built");
        let why: Vec<String> = model.warnings.into_iter().chain(notes).take(20).collect();
        e.details = Some(serde_json::json!({ "warnings_user_text": why }));
        return Err(e);
    }
    let warnings = model.warnings.into_iter().chain(notes).take(50).collect();
    Ok((out, TIER_SCAD, warnings))
}

/// Tier note for a FreeCAD document whose PartDesign history came in as features.
const TIER_FREECAD_HISTORY: &str = "tier 1: the FreeCAD PartDesign history as editable features";

/// A FreeCAD document's PartDesign bodies as a feature tree, rebuilt in a scratch session and
/// kept only when it gives the volume of the shapes FreeCAD stored (within a millionth) with
/// nothing skipped. `Err("")`: there is no PartDesign body (nothing to say).
fn freecad_history(
    bytes: &[u8],
    bodies: &[PreparedBody],
    kernel: &Arc<dyn gusset_kernel::Kernel>,
    importer: &dyn Importer,
    cancel: &CancelFlag,
    file_name: &str,
) -> Result<(crate::tree_import::Tree, Option<String>), String> {
    let limits = gusset_meshio::Limits::default();
    let mut budget = limits.max_decompressed_bytes;
    let xml = gusset_meshio::archive::read_entries_within(
        bytes,
        &limits,
        &|n| n == "Document.xml",
        crate::fcstd::MAX_XML as u64,
        &mut budget,
    )
    .map_err(|e| e.to_string())?;
    let doc = xml.first().map(|(_, v)| v.as_slice()).ok_or_else(String::new)?;
    if !crate::fcstd_tree::has_bodies(doc) {
        return Err(String::new());
    }
    // Fillets and chamfers name edges of their base feature's stored shape: each shape file
    // is read through the import worker once, and its edges taken from the kernel.
    let mut cache: std::collections::HashMap<String, crate::fcstd_tree::Edges> = std::collections::HashMap::new();
    let mut shape_edges = |file: &str| -> Result<crate::fcstd_tree::Edges, String> {
        if let Some(e) = cache.get(file) {
            return Ok(e.clone());
        }
        if cache.len() >= MAX_FREECAD_EDGE_SHAPES {
            return Err("too many base shapes".into());
        }
        let mut b = limits.max_decompressed_bytes;
        let data = gusset_meshio::archive::read_entries_within(bytes, &limits, &|n| n == file, u64::MAX, &mut b)
            .map_err(|e| e.to_string())?;
        let data = data.first().map(|(_, v)| v.as_slice()).ok_or("its base shape is missing")?;
        let format = if file.to_ascii_lowercase().ends_with(".bin") { "brep_bin" } else { "brp" };
        let blobs = importer.import(format, data, cancel).map_err(|e| e.to_string())?;
        let [(_, blob)] = blobs.as_slice() else { return Err("its base shape is not one solid".into()) };
        let shape = kernel.read_native(blob).map_err(|e| e.message)?;
        let topo = kernel.topology(&shape).map_err(|e| e.message)?;
        // Points along each edge, for edges a refined shape merged (Gusset keeps them split).
        let fine = gusset_kernel::TessellationParams { linear_deflection: 0.002, angular_deflection: 0.05 };
        let mesh = kernel.tessellate(&shape, &fine).map_err(|e| e.message)?;
        let e: Vec<_> = topo
            .edges
            .iter()
            .enumerate()
            .map(|(i, e)| crate::fcstd_tree::SrcEdge {
                smooth: match e.faces.as_slice() {
                    [f, g] if f != g => topo
                        .faces
                        .get(*f as usize)
                        .zip(topo.faces.get(*g as usize))
                        .is_some_and(|(a, b)| crate::fcstd_tree::is_smooth_edge(e.kind, e.midpoint, a, b)),
                    _ => false,
                },
                mid: e.midpoint,
                faces: e.faces.clone(),
                path: mesh
                    .edges
                    .get(i)
                    .map(|pl| {
                        // At most about 256 points, ends kept (a 50 mm arc stays within 0.004 mm).
                        let step = pl.len().div_ceil(256).max(1);
                        let mut v: Vec<[f64; 3]> = pl.iter().step_by(step).map(|p| p.map(f64::from)).collect();
                        if let Some(last) = pl.last().filter(|_| (pl.len() - 1) % step != 0) {
                            v.push(last.map(f64::from));
                        }
                        v
                    })
                    .unwrap_or_default(),
            })
            .collect();
        // The faces, for an extrusion up to one: a point inside each (the middle of its largest
        // triangle) and its normal.
        let faces: Vec<_> = topo
            .faces
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let at = |k: u32| -> [f64; 3] {
                    let p = mesh.indices.get(k as usize).and_then(|v| mesh.positions.get(*v as usize));
                    p.map_or([0.0; 3], |p| p.map(f64::from))
                };
                let area = |t: u32| -> f64 {
                    let (a, b, c) = (at(3 * t), at(3 * t + 1), at(3 * t + 2));
                    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
                    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                    n[0] * n[0] + n[1] * n[1] + n[2] * n[2]
                };
                let tris = mesh.face_ranges.get(i).cloned().unwrap_or_default();
                let best = tris.max_by(|a, b| area(*a).total_cmp(&area(*b)));
                let point = best.map_or(f.centroid, |t| {
                    let (a, b, c) = (at(3 * t), at(3 * t + 1), at(3 * t + 2));
                    [0, 1, 2]
                        .map(|k| (a.get(k).unwrap_or(&0.0) + b.get(k).unwrap_or(&0.0) + c.get(k).unwrap_or(&0.0)) / 3.0)
                });
                crate::fcstd_tree::SrcFace {
                    planar: f.kind == gusset_kernel::SurfaceKind::Plane,
                    point,
                    normal: f.normal,
                }
            })
            .collect();
        let e = crate::fcstd_tree::Edges { edges: e, faces };
        cache.insert(file.to_string(), e.clone());
        Ok(e)
    };
    let mut tree = crate::fcstd_tree::tree(doc, &mut shape_edges)?;
    let first = rebuild_check(&mut tree, bodies, kernel, file_name, "FreeCAD's");
    if let Err(why) = &first {
        // FreeCAD copies a pad's or pocket's whole tool in a pattern, and a Part object takes a
        // copy of its input (the body goes on beside it): try the tree written so.
        // A pad or pocket up to a face that does not rebuild as such is tried as the distance
        // its stored shape shows.
        // Sketch arcs and circles are written with 8 decimals; a boolean can depend on the
        // 6-decimal values older imports used (Page_033), so those are tried last.
        let up_to = tree.features.iter().any(|f| f.get("end_condition").and_then(Value::as_str) == Some("to_face"));
        for coarse in [false, true] {
            if coarse {
                let mut alt = tree.clone();
                if mark_coarse(&mut alt)
                    && let Ok(note) = rebuild_check(&mut alt, bodies, kernel, file_name, "FreeCAD's")
                {
                    return Ok((alt, note));
                }
            }
            for faces in [true, false] {
                if !faces && !up_to {
                    break;
                }
                for (tools, copies) in [(false, false), (true, false), (false, true), (true, true)] {
                    // The plain tree with faces is the first attempt.
                    if faces && !tools && !copies {
                        continue;
                    }
                    if let Ok(mut alt) = crate::fcstd_tree::tree_options(doc, &mut shape_edges, tools, copies, faces)
                        && alt.features != tree.features
                        && (!coarse || mark_coarse(&mut alt))
                        && let Ok(note) = rebuild_check(&mut alt, bodies, kernel, file_name, "FreeCAD's")
                    {
                        return Ok((alt, note));
                    }
                }
            }
        }
        return Err(why.clone());
    }
    let note = first?;
    Ok((tree, note))
}

/// Marks every sketch of `tree` to be written with 6-decimal arcs and circles. Whether there was
/// a sketch.
fn mark_coarse(tree: &mut crate::tree_import::Tree) -> bool {
    let mut any = false;
    for f in &mut tree.features {
        if f.get("type").and_then(Value::as_str) == Some("sketch") {
            f["coarse"] = Value::Bool(true);
            any = true;
        }
    }
    any
}

/// Tier note for a SolidWorks part whose feature history came in as features.
const TIER_SOLIDWORKS_HISTORY: &str = "tier 1: the SolidWorks feature history as editable features";

/// A SolidWorks part's feature history as a feature tree (ADR 0043 amendment), kept only when
/// it rebuilds to the part's own bodies like a FreeCAD history does. `Err("")`: the part has
/// no feature stream (nothing to say).
fn solidworks_history(
    bytes: &[u8],
    bodies: &[PreparedBody],
    kernel: &Arc<dyn gusset_kernel::Kernel>,
    file_name: &str,
    stock: Result<Vec<Vec<u8>>, String>,
) -> Result<(crate::tree_import::Tree, Option<String>), String> {
    let mut tree = gusset_sldprt::features::feature_tree(bytes, file_name, &gusset_sldprt::Limits::default())?
        .ok_or_else(String::new)?;
    let (stored, stock_note) = stored_stock(&mut tree, bodies, kernel, stock)?;
    let text = serde_json::to_vec(&tree).map_err(|e| e.to_string())?;
    let mut tree = crate::tree_import::parse(&text).map_err(|e| e.message)?;
    tree.stored_bodies = stored;
    let note = rebuild_check(&mut tree, bodies, kernel, file_name, "the part's")?;
    let note = match (stock_note, note) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (a, b) => a.or(b),
    };
    Ok((tree, note))
}

/// The native bodies of a part's `StockBodies` stream (the source body of a mirrored stock
/// part), as stored bodies for the tree.
fn sldprt_stock(
    bytes: &[u8],
    stem: &str,
    importer: &dyn Importer,
    cancel: &CancelFlag,
) -> Result<Vec<Vec<u8>>, String> {
    let Some(part) =
        gusset_sldprt::extract_stock(bytes, &gusset_sldprt::Limits::default()).map_err(|e| e.to_string())?
    else {
        return Ok(Vec::new());
    };
    let c = gusset_xt::to_step(&part.xt, stem, &gusset_xt::Limits::default()).map_err(|e| e.to_string())?;
    let bodies = importer.import("step", c.step.as_bytes(), cancel)?;
    Ok(bodies.into_iter().map(|(_, blob)| blob).collect())
}

/// A plane normal to axis `ax` at `c`: its origin and unit normal.
fn axis_plane(ax: usize, c: f64) -> ([f64; 3], [f64; 3]) {
    let mut o = [0.0; 3];
    let mut n = [0.0; 3];
    if let (Some(a), Some(b)) = (o.get_mut(ax), n.get_mut(ax)) {
        *a = c;
        *b = 1.0;
    }
    (o, n)
}

/// Hands the stock body to a tree that has a mirrored-stock feature, and adds the mirror
/// across the plane that carries the stock's box onto the part's final box (a plane parallel to
/// XY, XZ or YZ). The rebuild check then decides whether that is the part.
fn stored_stock(
    tree: &mut Value,
    bodies: &[PreparedBody],
    kernel: &Arc<dyn gusset_kernel::Kernel>,
    stock: Result<Vec<Vec<u8>>, String>,
) -> Result<(Vec<Vec<u8>>, Option<String>), String> {
    let Some(feats) = tree.get_mut("features").and_then(Value::as_array_mut) else { return Ok((Vec::new(), None)) };
    let Some(at) = feats.iter().position(|f| f.get("stock_mirror").is_some()) else {
        return Ok((Vec::new(), None));
    };
    let stock = stock.map_err(|e| format!("the stock body was not read: {e}"))?;
    // A part without a stock stream keeps only the finished body. Storing that body and
    // replaying the features after it would pass the rebuild check trivially (a cut or join
    // repeated on a body that has it changes nothing), so such a part comes in as bodies.
    if stock.is_empty() {
        return Err("the file keeps only the mirrored bodies, not the part they were mirrored from".into());
    }
    // Where no mirror feature is written (the stock is the part, or the image is pre-mirrored
    // below), a history of the stored body alone has nothing to edit: such a part is bodies.
    let editable = feats
        .iter()
        .filter(|f| !matches!(f.get("type").and_then(Value::as_str), Some("stored_body" | "unsupported")))
        .count();
    let only_stored = || -> Result<(), String> {
        if editable == 0 { Err("the history is the stored stock body only".into()) } else { Ok(()) }
    };
    let [blob] = stock.as_slice() else {
        return Err(format!("the part keeps {} stock bodies (one is expected)", stock.len()));
    };
    let ix = |a: [f64; 3], i: usize| a.get(i).copied().unwrap_or(0.0);
    let box_of = |blobs: &[&Vec<u8>]| -> Result<gusset_kernel::BBox, String> {
        let mut out: Option<gusset_kernel::BBox> = None;
        for b in blobs {
            let shape = kernel.read_native(b).map_err(|e| e.message)?;
            let bb = kernel.bbox(&shape).map_err(|e| e.message)?;
            out = Some(match out {
                None => bb,
                Some(a) => gusset_kernel::BBox {
                    min: [0, 1, 2].map(|k| ix(a.min, k).min(ix(bb.min, k))),
                    max: [0, 1, 2].map(|k| ix(a.max, k).max(ix(bb.max, k))),
                },
            });
        }
        out.ok_or_else(|| "no body".to_string())
    };
    let from = box_of(&[blob])?;
    let to = box_of(&bodies.iter().map(|b| &b.blob).collect::<Vec<_>>())?;
    let tol = 1e-4 * (1.0 + gusset_kernel::norm(to.size()));
    let close = |a: f64, b: f64| (a - b).abs() <= tol;
    let same = |ax: usize| close(ix(from.min, ax), ix(to.min, ax)) && close(ix(from.max, ax), ix(to.max, ax));
    let name = feats.get(at).and_then(|f| f.get("name")).and_then(Value::as_str).unwrap_or("Stock").to_string();
    // Not moved at all: the stock is the part.
    if (0..3).all(same) {
        only_stored()?;
        return Ok((stock, None));
    }
    for ax in 0..3 {
        let c = f64::midpoint(ix(from.max, ax), ix(to.min, ax));
        let flipped = close(ix(to.max, ax), 2.0 * c - ix(from.min, ax));
        if flipped && (0..3).filter(|a| *a != ax).all(same) {
            // The mirror unites the image with the stock (a body of its own would leave the
            // stock in the part): the half space of the stock is cut away afterwards.
            let below = ix(from.max, ax) <= c + tol;
            if !below && ix(from.min, ax) < c - tol {
                // The image overlaps the stock: no half space separates them, so the
                // stored body is the mirrored stock.
                only_stored()?;
                let shape = kernel.read_native(blob).map_err(|e| e.message)?;
                let (o, n) = axis_plane(ax, c);
                let r = kernel
                    .transform(&shape, &gusset_kernel::Transform::Mirror { origin: o, normal: n })
                    .map_err(|e| e.message)?;
                let mirrored = kernel.write_native(&r.shape).map_err(|e| e.message)?;
                return Ok((
                    vec![mirrored],
                    Some("the stored body is the stock body mirrored (the image overlaps the stock)".into()),
                ));
            }
            let (origin, normal) = axis_plane(ax, c);
            let mirror = format!("{name}_Mirror");
            let (xa, ya, (iu, iv)): ([f64; 3], [f64; 3], (usize, usize)) = match ax {
                0 => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], (1, 2)),
                1 => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], (2, 0)),
                _ => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], (0, 1)),
            };
            let mid = |i: usize| f64::midpoint(ix(from.min, i).min(ix(to.min, i)), ix(from.max, i).max(ix(to.max, i)));
            let half = |i: usize| 0.5 * (ix(from.max, i).max(ix(to.max, i)) - ix(from.min, i).min(ix(to.min, i))) + 1.0;
            let (hu, hv) = (2.0 * half(iu), 2.0 * half(iv));
            let (cu, cv) = (mid(iu), mid(iv));
            let size = 2.0 * (half(ax) + ix(from.max, ax).abs().max(ix(from.min, ax).abs()));
            let cut_sketch = format!("{name}_Half");
            feats.push(serde_json::json!({
                "type": "mirror", "name": mirror, "target": name, "merge": true,
                "features": [], "plane": {"origin": origin, "normal": normal}
            }));
            let corners = [(cu - hu, cv - hv), (cu + hu, cv - hv), (cu + hu, cv + hv), (cu - hu, cv + hv)];
            feats.push(serde_json::json!({
                "type": "sketch", "name": cut_sketch,
                "plane": {"origin": origin, "x_axis": xa, "y_axis": ya},
                "points": corners.iter().enumerate().map(|(i, (u, v))| serde_json::json!({"id": format!("p{i}"), "x": u, "y": v})).collect::<Vec<_>>(),
                "curves": (0..4).map(|i| serde_json::json!({"id": format!("c{i}"), "type": "line", "start": format!("p{i}"), "end": format!("p{}", (i + 1) % 4)})).collect::<Vec<_>>()
            }));
            feats.push(serde_json::json!({
                "type": "extrude", "name": format!("{name}_RemoveStock"), "sketch": cut_sketch, "profiles": "all",
                "operation": "cut", "target": name, "end_condition": "blind", "symmetric": false,
                "distance": if below { -size } else { size }
            }));
            return Ok((stock, None));
        }
    }
    Err("the mirror plane of the stock body was not found".into())
}

/// Tier note for an Inventor part whose feature history came in as features.
const TIER_INVENTOR_HISTORY: &str = "tier 1: the Inventor feature history as editable features";

/// The tree for an Inventor part's holes, kept or swapped for their plain-cut form: the tree
/// without the reader's `_` keys, each hole named in `swapped` replaced by its `_fallback`.
fn inventor_candidate(tree: &Value, swapped: &std::collections::HashSet<String>) -> Value {
    let mut out = tree.clone();
    let feats = tree.get("features").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut next = Vec::with_capacity(feats.len());
    for f in feats {
        let name = f.get("name").and_then(Value::as_str).unwrap_or_default();
        if f.get("type").and_then(Value::as_str) == Some("hole")
            && swapped.contains(name)
            && let Some(fb) = f.get("_fallback").and_then(Value::as_array)
        {
            next.extend(fb.iter().cloned());
            continue;
        }
        let mut f = f;
        if let Some(o) = f.as_object_mut() {
            o.retain(|k, _| !k.starts_with('_'));
        }
        next.push(f);
    }
    if let Some(o) = out.as_object_mut() {
        o.insert("features".into(), Value::Array(next));
    }
    out
}

/// The feature a failed [`rebuild_check`] reports first ("..., the first: {json}").
fn first_skipped(why: &str) -> Option<String> {
    let rest = why.split_once("the first: ")?.1;
    let v: Value = serde_json::from_str(rest).ok()?;
    v.get("feature").and_then(Value::as_str).map(str::to_string)
}

/// An Inventor part's feature history as a feature tree (ADR 0046 amendment), kept only when
/// it rebuilds to the part's own body like a FreeCAD history does. A hole the kernel refuses
/// is tried as plain cuts (one by one in order, then all of them): the rebuild decides, so the
/// outcome is the same every time. `Err("")`: nothing to say.
fn inventor_history(
    bytes: &[u8],
    bodies: &[PreparedBody],
    kernel: &Arc<dyn gusset_kernel::Kernel>,
    file_name: &str,
) -> Result<(crate::tree_import::Tree, Option<String>), String> {
    let tree = gusset_ole::inventor_features::feature_tree(bytes, file_name, &gusset_ole::Limits::default())?;
    let holes: Vec<String> = tree
        .get("features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|f| f.get("type").and_then(Value::as_str) == Some("hole") && f.get("_fallback").is_some())
        .filter_map(|f| f.get("name").and_then(Value::as_str).map(str::to_string))
        .collect();
    let omitted: Vec<String> = tree
        .pointer("/source/omitted")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|o| Some(format!("{} ({})", o.get("name")?.as_str()?, o.get("why")?.as_str()?)))
        .take(20)
        .collect();
    let mut swapped = std::collections::HashSet::new();
    let mut all_tried = false;
    for _ in 0..holes.len() + 2 {
        let text = serde_json::to_vec(&inventor_candidate(&tree, &swapped)).map_err(|e| e.to_string())?;
        let mut t = crate::tree_import::parse(&text).map_err(|e| e.message)?;
        match rebuild_check(&mut t, bodies, kernel, file_name, "the part's") {
            Ok(note) => {
                let mut notes: Vec<String> = note.into_iter().collect();
                if !swapped.is_empty() {
                    notes
                        .push(format!("{} hole(s) are plain cuts: the kernel refused the hole feature", swapped.len()));
                }
                if !omitted.is_empty() {
                    notes.push(format!("left out, no effect on the body: {}", omitted.join(", ")));
                }
                return Ok((t, (!notes.is_empty()).then(|| notes.join("; "))));
            }
            Err(why) => {
                if let Some(n) = first_skipped(&why).filter(|n| holes.contains(n) && !swapped.contains(n)) {
                    swapped.insert(n);
                } else if !all_tried && swapped.len() < holes.len() {
                    all_tried = true;
                    swapped = holes.iter().cloned().collect();
                } else {
                    // A feature the reader could not read says why, which the rebuild's own
                    // "not exported" does not.
                    let named = first_skipped(&why).and_then(|n| {
                        tree.get("features")?.as_array()?.iter().find_map(|f| {
                            (f.get("name")?.as_str()? == n && f.get("type")?.as_str()? == "unsupported")
                                .then(|| {
                                    let what = f.get("original_type")?.as_str()?;
                                    let reason = f.get("reason")?.as_str()?;
                                    Some(format!("the {what} feature {n} cannot be read: {reason}"))
                                })
                                .flatten()
                        })
                    });
                    return Err(named.unwrap_or(why));
                }
            }
        }
    }
    Err("the holes could not be rebuilt".into())
}

/// Tier note for a Fusion design whose timeline came in as features.
const TIER_FUSION_HISTORY: &str = "tier 1: the Fusion design timeline as editable features";

/// Most features of a Fusion timeline checked state by state against the stored bodies.
const MAX_FUSION_STATE_FEATURES: usize = 80;

/// A Fusion design's timeline as a feature tree (ADR 0039 amendment), kept only when it rebuilds
/// to the bodies the file stores, like a FreeCAD history does. A file that stores one body per
/// feature step (the result of each, and the cut tools) is checked step by step instead
/// ([`fusion_states_check`]). `Err("")`: nothing to say (a design without a timeline).
fn fusion_history(
    bytes: &[u8],
    bodies: &[PreparedBody],
    kernel: &Arc<dyn gusset_kernel::Kernel>,
    file_name: &str,
    importer: &dyn Importer,
    cancel: &CancelFlag,
) -> Result<(crate::tree_import::Tree, Option<String>), String> {
    let limits = gusset_meshio::Limits::default();
    let mut budget = limits.max_decompressed_bytes;
    let wanted = |n: &str| {
        n.ends_with("/MetaStream.dat")
            || n.ends_with("/BulkStream.dat")
            || (n.contains("Breps.BlobParts/")
                && (n.to_ascii_lowercase().ends_with(".smb") || n.to_ascii_lowercase().ends_with(".smbh")))
    };
    let read = |data: &[u8], budget: &mut u64| {
        gusset_meshio::archive::read_entries_within(data, &limits, &wanted, MAX_F3D_STREAM_BYTES, budget)
            .map_err(|e| e.to_string())
    };
    let has_stream = |e: &[(String, Vec<u8>)]| e.iter().any(|(n, _)| n.ends_with("/BulkStream.dat"));
    // "The design has no feature timeline": nothing to say about this design.
    let no_timeline = |why: &str| why.starts_with("the design has no") || why.starts_with("the file has no");
    let mut entries = read(bytes, &mut budget)?;
    let tree = if has_stream(&entries) {
        match gusset_sat::fusion_tree::feature_tree(&entries, file_name) {
            Ok(t) => t,
            Err(why) if no_timeline(&why) => return Err(String::new()),
            Err(why) => return Err(why),
        }
    } else {
        // An `.f3z`: the designs it holds. The root design named in `Manifest.json` is the one
        // whose timeline is the file's; where it has none, the single design that has one.
        let inner = gusset_meshio::archive::read_entries_within(
            bytes,
            &limits,
            &|n| n.to_ascii_lowercase().ends_with(".f3d"),
            MAX_F3D_PART_BYTES,
            &mut budget,
        )
        .map_err(|e| e.to_string())?;
        if inner.len() > MAX_F3D_PARTS {
            return Err(format!("the archive holds more than {MAX_F3D_PARTS} designs"));
        }
        let manifest = gusset_meshio::archive::read_entries_within(
            bytes,
            &limits,
            &|n| n == "Manifest.json" || n == "DesignDescription.json",
            MAX_F3D_JSON_BYTES,
            &mut budget,
        )
        .map_err(|e| e.to_string())?;
        let json_of = |name: &str| {
            manifest.iter().find(|(n, _)| n == name).and_then(|(_, b)| serde_json::from_slice::<Value>(b).ok())
        };
        let root = json_of("Manifest.json").and_then(|m| m.get("root").and_then(Value::as_str).map(str::to_string));
        // The designs the others insert (Component Insert): the file each reference key names.
        let xrefs = json_of("DesignDescription.json").map(|d| xref_files(&d)).unwrap_or_default();
        let mut order: Vec<&(String, Vec<u8>)> = inner.iter().collect();
        order.sort_by_key(|(n, _)| root.as_deref() != Some(n.as_str()));
        let root_first = order.first().is_some_and(|(n, _)| root.as_deref() == Some(n.as_str()));
        let mut with_timeline = Vec::new();
        for (k, (_, data)) in order.into_iter().enumerate() {
            let design = read(data, &mut budget)?;
            if !has_stream(&design) {
                continue;
            }
            // The inserted designs' entries (their blobs are stored bodies of this design's tree).
            let mut inserted: Vec<(String, Vec<u8>)> = Vec::new();
            let r = {
                let mut resolve = |keys: &[String]| -> Option<Vec<(String, Vec<u8>)>> {
                    let exact = keys.iter().find_map(|q| xrefs.get(q));
                    let file = exact.or_else(|| {
                        xrefs.iter().find_map(|(x, f)| keys.iter().any(|q| q.get(..36) == x.get(..36)).then_some(f))
                    })?;
                    let (_, data) = inner.iter().find(|(n, _)| n == file)?;
                    let e = read(data, &mut budget).ok()?;
                    inserted.extend(e.iter().filter(|(n, _)| n.contains("Breps.BlobParts/")).cloned());
                    Some(e)
                };
                gusset_sat::fusion_tree::feature_tree_with_inserts(&design, file_name, &mut resolve)
            };
            match r {
                Err(why) if no_timeline(&why) => {}
                r => {
                    let mut design = design;
                    design.extend(inserted);
                    if k == 0 && root_first {
                        // The root has a timeline: it is the file's.
                        with_timeline = vec![(design, r)];
                        break;
                    }
                    with_timeline.push((design, r));
                }
            }
        }
        match (with_timeline.pop(), with_timeline.is_empty()) {
            (Some((design, Ok(t))), true) => {
                entries = design;
                t
            }
            (Some((_, Err(why))), true) => return Err(why),
            _ => return Err(String::new()),
        }
    };
    let text = serde_json::to_vec(&tree).map_err(|e| e.to_string())?;
    let mut t = crate::tree_import::parse(&text).map_err(|e| e.message)?;
    let stored = fusion_stored_bodies(&tree, &entries, bodies, kernel, importer, cancel)?;
    t.stored_bodies = stored.clone();
    match rebuild_check(&mut t, bodies, kernel, file_name, "the file's stored") {
        Ok(note) => {
            // One entry per stored body, so the import's own body count check agrees.
            t.bodies = bodies
                .iter()
                .map(|b| {
                    let volume_mm3 = kernel
                        .read_native(&b.blob)
                        .ok()
                        .and_then(|s| kernel.mass_properties(&s).ok())
                        .map(|m| m.volume);
                    crate::tree_import::TreeBody { name: b.name.clone(), volume_mm3 }
                })
                .collect();
            Ok((t, note))
        }
        Err(why) if bodies.len() > 1 && why.contains("mm3") => {
            let mut t = crate::tree_import::parse(&text).map_err(|e| e.message)?;
            t.stored_bodies = stored;
            let note = fusion_states_check(&mut t, bodies, kernel, file_name)
                .map_err(|states| format!("{why}; checked step by step: {states}"))?;
            Ok((t, Some(note)))
        }
        Err(why) => Err(why),
    }
}

/// The kernel-native bodies the tree's `stored_body` features (Fusion Base Features) stand for, in
/// the order of their `body` numbers: body `index` of the history blob the feature names, moved by
/// the component's placement like the tree's other features. A history that has nothing but stored
/// bodies, or whose stored body is already the finished body that a later feature changes, is not a
/// history: the rebuild check would pass trivially.
fn fusion_stored_bodies(
    tree: &Value,
    entries: &[(String, Vec<u8>)],
    bodies: &[PreparedBody],
    kernel: &Arc<dyn gusset_kernel::Kernel>,
    importer: &dyn Importer,
    cancel: &CancelFlag,
) -> Result<Vec<Vec<u8>>, String> {
    let feats = tree.get("features").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let ty = |f: &Value| f.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    let stored_feats: Vec<&Value> = feats.iter().filter(|f| ty(f) == "stored_body").collect();
    if stored_feats.is_empty() {
        return Ok(Vec::new());
    }
    if feats.iter().all(|f| matches!(ty(f).as_str(), "stored_body" | "unsupported")) {
        return Err("the history is the stored base feature bodies only".into());
    }
    if stored_feats.len() > MAX_F3D_BODIES {
        return Err("the design has too many base feature bodies".into());
    }
    let mut out: Vec<Vec<u8>> = Vec::new();
    for (k, f) in stored_feats.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let name = f.get("name").and_then(Value::as_str).unwrap_or("?");
        if f.get("body").and_then(Value::as_u64) != Some(k as u64) {
            return Err(format!("the stored body {name} is not numbered in tree order"));
        }
        let blob_name = f.get("brep").and_then(Value::as_str).unwrap_or("");
        let blob = entries
            .iter()
            .find(|(n, _)| n.rsplit('/').next() == Some(blob_name))
            .map(|(_, b)| b)
            .ok_or_else(|| format!("the history blob {blob_name} of {name} is not in the file"))?;
        let num = |key: &str| f.get(key).and_then(Value::as_u64).map(|v| v as usize);
        let (Some(index), Some(count)) = (num("index"), num("count")) else {
            return Err(format!("the stored body {name} does not say which body of its blob it is"));
        };
        let place = f.get("placement").map(|p| -> Option<gusset_sat::fusion::Placement> {
            let mut r = [[0.0; 3]; 3];
            let mut t = [0.0; 3];
            for (i, row) in p.get("r")?.as_array()?.iter().enumerate().take(3) {
                for (j, v) in row.as_array()?.iter().enumerate().take(3) {
                    *r.get_mut(i)?.get_mut(j)? = v.as_f64()?;
                }
            }
            for (i, v) in p.get("t")?.as_array()?.iter().enumerate().take(3) {
                *t.get_mut(i)? = v.as_f64()?;
            }
            Some(gusset_sat::fusion::Placement { r, t })
        });
        let place = match place {
            Some(Some(p)) => Some(p),
            Some(None) => return Err(format!("the placement of {name} cannot be read")),
            None => None,
        };
        let c = gusset_sat::to_step_nth(blob, name, &gusset_sat::Limits::default(), place.as_ref(), index, count)
            .map_err(|e| format!("the stored body {name}: {e}"))?;
        let mut imported = importer.import("step", c.step.as_bytes(), cancel).map_err(|e| e.to_string())?;
        let (Some((_, blob)), true) = (imported.pop(), imported.is_empty()) else {
            return Err(format!("the stored body {name} did not come in as one body"));
        };
        out.push(blob);
    }
    // A later feature that changes a stored body must find it as it was before: when the stored
    // body is the very body the file ends with, the feature has nothing left to do and the
    // rebuild would agree with the file whatever the feature was.
    let names: std::collections::HashMap<&str, usize> =
        stored_feats.iter().enumerate().filter_map(|(k, f)| Some((f.get("name")?.as_str()?, k))).collect();
    let mut targeted: Vec<usize> = Vec::new();
    for f in feats.iter().filter(|f| ty(f) != "stored_body") {
        let t = f.get("target");
        let list: Vec<&str> = match t {
            Some(Value::String(s)) => vec![s.as_str()],
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        for n in list {
            // The feature's name stands for its first body.
            if let Some(k) = names.get(n) {
                targeted.push(*k);
            }
        }
    }
    if !targeted.is_empty() {
        let vol = |blob: &Vec<u8>| -> Result<f64, String> {
            let shape = kernel.read_native(blob).map_err(|e| e.message)?;
            Ok(kernel.mass_properties(&shape).map_err(|e| e.message)?.volume)
        };
        let finals = bodies.iter().map(|b| vol(&b.blob)).collect::<Result<Vec<_>, _>>()?;
        for k in targeted {
            let v = out.get(k).map_or(Ok(0.0), vol)?;
            if finals.iter().any(|f| (f - v).abs() <= 1e-9 * f.abs().max(v.abs())) {
                return Err("a feature changes a stored body that already is a finished body of the file".into());
            }
        }
    }
    Ok(out)
}

/// How many copies of its cut tool (or added body) each feature makes, from the tree: a cut or
/// join extrude one, a hole its positions, a linear pattern its count less one times the copies
/// of the features it repeats, a mirror the copies of the features it mirrors. `None` where it
/// cannot be said (blends, new bodies, a pattern of something unknown).
fn fusion_instances(features: &[Value]) -> Vec<Option<u64>> {
    let mut by_name: std::collections::HashMap<String, Option<u64>> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(features.len());
    for f in features {
        let get = |k: &str| f.get(k).and_then(Value::as_str);
        let inputs = || -> Option<u64> {
            let names = f.get("features")?.as_array()?;
            let mut sum = 0u64;
            for n in names {
                sum = sum.checked_add((*by_name.get(n.as_str()?)?)?)?;
            }
            (sum > 0).then_some(sum)
        };
        let n = match get("type") {
            Some("hole") => f.get("positions").and_then(Value::as_array).map(|p| p.len() as u64).filter(|n| *n > 0),
            // One tool per profile loop the extrude names (none where it takes "all" of its sketch).
            Some("extrude") if matches!(get("operation"), Some("cut" | "join")) => {
                Some(f.get("profiles").and_then(Value::as_array).map_or(1, |p| p.len().max(1)) as u64)
            }
            Some("pattern_linear") => {
                let count = f.get("count").and_then(Value::as_u64).filter(|c| *c >= 2);
                count.and_then(|c| inputs()?.checked_mul(c - 1))
            }
            Some("mirror") => inputs(),
            _ => None,
        };
        if let Some(name) = get("name") {
            by_name.insert(name.to_string(), n);
        }
        out.push(n);
    }
    out
}

/// Which features may make fewer tools than they have profile loops: a cut or join extrude of
/// several loops makes no tool for a loop that lies where there is no material to change (an
/// outline the board already has). Those are explained by the sum of their tools, not one tool
/// times the copies.
fn fusion_loose(features: &[Value]) -> Vec<bool> {
    features
        .iter()
        .map(|f| {
            f.get("type").and_then(Value::as_str) == Some("extrude")
                && matches!(f.get("operation").and_then(Value::as_str), Some("cut" | "join"))
                && f.get("profiles").and_then(Value::as_array).is_some_and(|p| p.len() > 1)
        })
        .collect()
}

/// The part of each stored tool (indices into the stored solids) that lies in the bodies feature
/// `k` changes, as they were before it; `None` where it cannot be measured.
type ToolsInside<'a> = dyn FnMut(usize, &[usize]) -> Result<Option<Vec<f64>>, String> + 'a;

/// The volumes, box and shapes of a model's bodies after some features.
type Built = (f64, Option<gusset_kernel::BBox>, Vec<f64>, Vec<gusset_kernel::Shape>);

/// The stored volumes against the bodies of the model after each feature (`per_body[k]` holds the
/// volume of every body after `k` features, `vols[k]` their sum) and the copies each feature
/// makes: every stored solid is a state (the volume of some body of the model after some feature)
/// or a tool whose volume times the copies of some feature is that feature's volume change, and
/// every feature after the last state changes the volume by exactly a stored tool times its
/// copies. A cut of several loops may make fewer tools than loops (`loose`): its volume change is
/// then the sum of the tools no other feature explains, or, where a tool lies (partly) where the
/// body already has no material (a hole drilled into a slot of the outline), the sum of the parts
/// of those tools that lie in the body the feature changes, as `inside(k, tools)` measures them
/// (`None`: not measurable). Returns the numbers of states and tools.
fn fusion_explain_steps(
    vols: &[f64],
    per_body: &[Vec<f64>],
    stored: &[f64],
    insts: &[Option<u64>],
    loose: &[bool],
    names: &[String],
    inside: &mut ToolsInside<'_>,
) -> Result<(usize, usize), String> {
    let same = |a: f64, b: f64| (a - b).abs() <= FREECAD_VOLUME_TOLERANCE * a.abs().max(b.abs());
    let delta = |k: usize| -> f64 {
        match (vols.get(k), k.checked_sub(1).and_then(|p| vols.get(p))) {
            (Some(a), Some(b)) => (a - b).abs(),
            _ => 0.0,
        }
    };
    let is_state = |v: f64| per_body.iter().skip(1).any(|bs| bs.iter().any(|b| same(*b, v)));
    let copies = |k: usize| insts.get(k - 1).copied().flatten();
    let explains = |tool: f64, k: usize| copies(k).is_some_and(|m| same(delta(k), tool * m as f64));
    let mut tool_volumes = Vec::new();
    // Where each tool is in `stored`.
    let mut tool_at = Vec::new();
    let mut states = 0;
    for (at, v) in stored.iter().enumerate() {
        if is_state(*v) {
            states += 1;
        } else {
            tool_volumes.push(*v);
            tool_at.push(at);
        }
    }
    if states == 0 {
        return Err("no stored solid is a state of the model".into());
    }
    // The tools one feature makes: one stored tool times its copies, or (a loose cut) the sum of
    // the tools no other feature explains, not more than its loops.
    let mut covered: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    let mut explained: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    for k in 1..vols.len() {
        if delta(k) <= 1e-9 {
            continue;
        }
        for (i, t) in tool_volumes.iter().enumerate() {
            if explains(*t, k) {
                covered.insert(i);
                explained.insert(k);
            }
        }
    }
    for k in 1..vols.len() {
        if delta(k) <= 1e-9 || explained.contains(&k) || !loose.get(k - 1).copied().unwrap_or(false) {
            continue;
        }
        let group: Vec<usize> = (0..tool_volumes.len()).filter(|i| !covered.contains(i)).collect();
        let sum: f64 = group.iter().filter_map(|i| tool_volumes.get(*i)).sum();
        if group.is_empty() || !copies(k).is_some_and(|m| group.len() as u64 <= m) {
            continue;
        }
        let fits = same(delta(k), sum) || {
            // Measured: the part of each tool inside the body before the feature.
            let at: Vec<usize> = group.iter().filter_map(|i| tool_at.get(*i).copied()).collect();
            inside(k, &at)?.is_some_and(|parts| parts.len() == at.len() && same(delta(k), parts.iter().sum()))
        };
        if fits {
            covered.extend(group);
            explained.insert(k);
        }
    }
    if let Some(i) = (0..tool_volumes.len()).find(|i| !covered.contains(i)) {
        let v = tool_volumes.get(i).copied().unwrap_or(0.0);
        return Err(format!(
            "a stored solid of {v:.4} mm3 is neither a state of the model nor the cut tool of a feature with the copies the tree makes"
        ));
    }
    let tools = tool_volumes.len();
    // The last step that added or changed a body to a stored volume.
    let changed = |k: usize| -> Vec<f64> {
        let mut before: Vec<f64> = k.checked_sub(1).and_then(|p| per_body.get(p)).cloned().unwrap_or_default();
        let mut out = Vec::new();
        for b in per_body.get(k).map_or(&[][..], Vec::as_slice) {
            match before.iter().position(|x| same(*x, *b)) {
                Some(at) => {
                    before.swap_remove(at);
                }
                None => out.push(*b),
            }
        }
        out
    };
    let last =
        (1..vols.len()).rev().find(|k| changed(*k).iter().any(|b| stored.iter().any(|s| same(*s, *b)))).unwrap_or(0);
    for k in last + 1..vols.len() {
        if delta(k) <= 1e-9 {
            continue;
        }
        let name = names.get(k - 1).map_or("?", String::as_str);
        let Some(m) = copies(k) else {
            return Err(format!(
                "how many copies {name} makes cannot be derived from the tree, and it follows the last stored state"
            ));
        };
        if !explained.contains(&k) {
            return Err(format!(
                "{name} changes the volume by {:.4} mm3, not {m} copies of a stored cut tool",
                delta(k)
            ));
        }
    }
    Ok((states, tools))
}

/// The check for a Fusion file that stores the bodies of every feature step: each stored solid
/// must be either the model after some feature (a state) or a cut tool, the volume a feature
/// removes or adds being exactly the tool times the number of copies the tree makes
/// ([`fusion_instances`]: a hole's positions, the counts of the patterns that repeat it). Every
/// feature after the last stored state must be explained that way, so a wrong pattern count is
/// refused; one whose copies cannot be derived is refused too. The finished model must also lie
/// in the box of the stored solids.
fn fusion_states_check(
    tree: &mut crate::tree_import::Tree,
    bodies: &[PreparedBody],
    kernel: &Arc<dyn gusset_kernel::Kernel>,
    file_name: &str,
) -> Result<String, String> {
    let n = tree.features.len();
    let kind = |f: &Value| f.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    // Sketches change no body and a stored body adds the one it holds: only the others are built.
    // A new-body extrude or sweep adds a body too: a run of them is built once (below), so only the
    // features that change a body are counted against the limit (it bounds the work, not the check).
    let steps = tree
        .features
        .iter()
        .filter(|f| {
            let new_body = matches!(kind(f).as_str(), "extrude" | "sweep")
                && f.get("operation").and_then(Value::as_str) == Some("new");
            !(new_body || matches!(kind(f).as_str(), "sketch" | "stored_body"))
        })
        .count();
    if steps > MAX_FUSION_STATE_FEATURES {
        return Err(format!("more than {MAX_FUSION_STATE_FEATURES} features"));
    }
    let mut stored: Vec<(f64, gusset_kernel::BBox)> = Vec::new();
    for b in bodies {
        let shape = kernel.read_native(&b.blob).map_err(|e| e.message)?;
        stored.push((
            kernel.mass_properties(&shape).map_err(|e| e.message)?.volume,
            kernel.bbox(&shape).map_err(|e| e.message)?,
        ));
    }
    let empty = gusset_doc::Document::from_text("gusset 1\nunits mm\n").map_err(|e| e.to_string())?;
    // The volume of every body (and the box of the whole) of the model after the first k features.
    // One evaluator for every prefix: each build reuses what the shorter ones built.
    let mut evaluator = gusset_eval::Evaluator::new(kernel.clone());
    let mut build = |k: usize, full: bool| -> Result<Built, String> {
        let mut t = tree.clone();
        t.features.truncate(k);
        t.bodies = vec![crate::tree_import::TreeBody { name: "all".into(), volume_mm3: Some(1.0) }];
        let ctx = crate::CmdCtx::user(0);
        let mut scratch = crate::Session::new(&empty, kernel.clone(), &ctx);
        let r =
            crate::tree_import::apply_with(&mut scratch, &ctx, &t, file_name, &mut evaluator).map_err(|e| e.message)?;
        if let Some(first) = r.get("skipped_user_text").and_then(Value::as_array).and_then(|a| a.first()) {
            let first = first.as_str().map_or_else(|| first.to_string(), str::to_string);
            return Err(format!("a feature could not be rebuilt, the first: {first}"));
        }
        let vol = r.pointer("/volume_mm3/imported").and_then(Value::as_f64).unwrap_or(0.0);
        let mut bb: Option<gusset_kernel::BBox> = None;
        let mut each = Vec::new();
        let ev = scratch.evaluate(&ctx.actor).map_err(|e| e.message)?;
        let shapes: Vec<gusset_kernel::Shape> = ev.bodies.iter().map(|b| b.shape.clone()).collect();
        for b in &ev.bodies {
            each.push(kernel.mass_properties(&b.shape).map_err(|e| e.message)?.volume);
            if full {
                let g = kernel.bbox(&b.shape).map_err(|e| e.message)?;
                bb = Some(bb.map_or(g, |a| gusset_kernel::BBox {
                    min: [a.min[0].min(g.min[0]), a.min[1].min(g.min[1]), a.min[2].min(g.min[2])],
                    max: [a.max[0].max(g.max[0]), a.max[1].max(g.max[1]), a.max[2].max(g.max[2])],
                }));
            }
        }
        Ok((vol, bb, each, shapes))
    };
    // States: the bodies after every feature. Sketches change no body; a run of features that
    // only add bodies (new-body extrudes and sweeps, stored bodies) is built once and its
    // bodies, in the order they were made, are shared out to its steps; a feature that changes
    // a body is built on its own.
    let adds = |f: &Value| match kind(f).as_str() {
        "sketch" => Some(0usize),
        "stored_body" => Some(1),
        "extrude" | "sweep" if f.get("operation").and_then(Value::as_str) == Some("new") => Some(1),
        _ => None,
    };
    let same_vol = |a: f64, b: f64| (a - b).abs() <= FREECAD_VOLUME_TOLERANCE * a.abs().max(b.abs());
    let mut vols = vec![0.0f64];
    let mut per_body: Vec<Vec<f64>> = vec![Vec::new()];
    let mut k = 1;
    while k <= n {
        let prev_bodies = per_body.last().cloned().unwrap_or_default();
        let mut end = k;
        while end <= n && tree.features.get(end - 1).and_then(adds).is_some() {
            end += 1;
        }
        // The run k..end-1 only adds bodies; it is built as one when it is longer than a step.
        if end > k + 1 {
            let (_, _, each, _) = build(end - 1, false)?;
            let made: Vec<usize> = (k..end).map(|j| tree.features.get(j - 1).and_then(adds).unwrap_or(0)).collect();
            let total: usize = made.iter().sum();
            let ordered =
                each.len() == prev_bodies.len() + total && prev_bodies.iter().zip(&each).all(|(a, b)| same_vol(*a, *b));
            if ordered {
                let mut have = prev_bodies.len();
                for m in made {
                    have += m;
                    let bodies: Vec<f64> = each.iter().take(have).copied().collect();
                    vols.push(bodies.iter().sum());
                    per_body.push(bodies);
                }
                k = end;
                continue;
            }
        }
        if tree.features.get(k - 1).map(kind).as_deref() == Some("sketch") {
            vols.push(vols.last().copied().unwrap_or(0.0));
            per_body.push(prev_bodies);
        } else {
            let (v, _, each, _) = build(k, false)?;
            vols.push(v);
            per_body.push(each);
        }
        k += 1;
    }
    let (final_vol, final_box, _, _) = build(n, true)?;
    let names: Vec<String> =
        tree.features.iter().map(|f| f.get("name").and_then(Value::as_str).unwrap_or("?").to_string()).collect();
    let insts = fusion_instances(&tree.features);
    let loose = fusion_loose(&tree.features);
    let volumes: Vec<f64> = stored.iter().map(|(v, _)| *v).collect();
    // The part of each stored tool that lies in the bodies feature `k` changes, as they were
    // before it (the bodies of the model after `k - 1` features that `k` leaves different).
    let mut inside = |k: usize, tools: &[usize]| -> Result<Option<Vec<f64>>, String> {
        let (_, _, _, before) = build(k - 1, false)?;
        let (_, _, mut after, _) = build(k, false)?;
        let mut changed = Vec::new();
        for body in before {
            let v = kernel.mass_properties(&body).map_err(|e| e.message)?.volume;
            match after.iter().position(|a| same_vol(*a, v)) {
                Some(at) => {
                    after.swap_remove(at);
                }
                None => changed.push((kernel.bbox(&body).map_err(|e| e.message)?, body)),
            }
        }
        if changed.is_empty() {
            return Ok(None);
        }
        let apart = |a: &gusset_kernel::BBox, b: &gusset_kernel::BBox| {
            (0..3).any(|i| a.max.get(i) < b.min.get(i) || b.max.get(i) < a.min.get(i))
        };
        let mut parts = Vec::with_capacity(tools.len());
        for t in tools {
            let (Some(b), Some((_, tool_box))) = (bodies.get(*t), stored.get(*t)) else { return Ok(None) };
            let tool = kernel.read_native(&b.blob).map_err(|e| e.message)?;
            let mut part = 0.0;
            for (body_box, body) in &changed {
                if apart(body_box, tool_box) {
                    continue;
                }
                // An empty common part is reported as an error: the tool lies in a void.
                match kernel.boolean(gusset_kernel::BoolOp::Intersect, body, &tool) {
                    Ok(common) => part += kernel.mass_properties(&common.shape).map_err(|e| e.message)?.volume,
                    Err(e) if e.message.contains("removed all material") => {}
                    Err(e) => return Err(e.message),
                }
            }
            parts.push(part);
        }
        Ok(Some(parts))
    };
    let (states, tools) = fusion_explain_steps(&vols, &per_body, &volumes, &insts, &loose, &names, &mut inside)?;
    let mut want_box: Option<gusset_kernel::BBox> = None;
    for (_, b) in &stored {
        want_box = Some(want_box.map_or(*b, |a| gusset_kernel::BBox {
            min: [a.min[0].min(b.min[0]), a.min[1].min(b.min[1]), a.min[2].min(b.min[2])],
            max: [a.max[0].max(b.max[0]), a.max[1].max(b.max[1]), a.max[2].max(b.max[2])],
        }));
    }
    if let (Some(w), Some(g)) = (want_box, final_box) {
        let tol = 1e-4 * (1.0 + gusset_kernel::norm(w.size()));
        let far = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).any(|(a, b)| (a - b).abs() > tol);
        if far(w.min, g.min) || far(w.max, g.max) {
            return Err(format!(
                "the features lie in {:?}..{:?}, the stored solids in {:?}..{:?}",
                g.min, g.max, w.min, w.max
            ));
        }
    }
    tree.bodies = vec![crate::tree_import::TreeBody { name: "all".into(), volume_mm3: Some(final_vol) }];
    Ok(format!(
        "the file stores the steps of the design, not its end result: {states} stored solids are states of the rebuilt model and {tools} are its cut tools; every feature after the last state removes exactly its tool times the copies the tree makes"
    ))
}

/// Builds `tree` in a scratch session and accepts it only when every feature builds and it
/// gives the volume of `bodies` (within [`FREECAD_VOLUME_TOLERANCE`]) and their bounding box.
/// `whose` names the bodies in the reasons ("FreeCAD's"). Returns a note when the volumes
/// differ measurably.
fn rebuild_check(
    tree: &mut crate::tree_import::Tree,
    bodies: &[PreparedBody],
    kernel: &Arc<dyn gusset_kernel::Kernel>,
    file_name: &str,
    whose: &str,
) -> Result<Option<String>, String> {
    // The stored shapes' volume is what the features must give.
    let mut want = 0.0;
    let mut want_box: Option<gusset_kernel::BBox> = None;
    let union = |a: Option<gusset_kernel::BBox>, b: gusset_kernel::BBox| {
        Some(a.map_or(b, |a| gusset_kernel::BBox {
            min: [a.min[0].min(b.min[0]), a.min[1].min(b.min[1]), a.min[2].min(b.min[2])],
            max: [a.max[0].max(b.max[0]), a.max[1].max(b.max[1]), a.max[2].max(b.max[2])],
        }))
    };
    for b in bodies {
        let shape = kernel.read_native(&b.blob).map_err(|e| e.message)?;
        want += kernel.mass_properties(&shape).map_err(|e| e.message)?.volume;
        want_box = union(want_box, kernel.bbox(&shape).map_err(|e| e.message)?);
    }
    tree.bodies = vec![crate::tree_import::TreeBody { name: "all".into(), volume_mm3: Some(want) }];
    let empty = gusset_doc::Document::from_text("gusset 1\nunits mm\n").map_err(|e| e.to_string())?;
    let ctx = crate::CmdCtx::user(0);
    let mut scratch = crate::Session::new(&empty, kernel.clone(), &ctx);
    let r = crate::tree_import::apply(&mut scratch, &ctx, tree, file_name).map_err(|e| e.message)?;
    let skipped = r.get("skipped_user_text").and_then(Value::as_array).cloned().unwrap_or_default();
    if let Some(first) = skipped.first() {
        let first = first.as_str().map_or_else(|| first.to_string(), str::to_string);
        return Err(format!("{} features could not be rebuilt, the first: {first}", skipped.len()));
    }
    let got = r.pointer("/volume_mm3/imported").and_then(Value::as_f64).unwrap_or(f64::NAN);
    // Blends and sweeps are approximated surfaces, computed by another modeller (FreeCAD's
    // OCCT build, Parasolid): their volumes agree to about 1e-5. A larger difference is a
    // different model.
    let rel = (got - want).abs() / want;
    let close = rel <= FREECAD_VOLUME_TOLERANCE;
    if want.is_nan() || want <= 0.0 || !close {
        return Err(format!("the features give {got:.4} mm3, {whose} shapes {want:.4} mm3"));
    }
    // The same volume in another place (a mirror about the wrong plane) is caught by the box.
    let ev = scratch.evaluate(&ctx.actor).map_err(|e| e.message)?;
    let mut got_box = None;
    for b in &ev.bodies {
        got_box = union(got_box, kernel.bbox(&b.shape).map_err(|e| e.message)?);
    }
    if let (Some(w), Some(g)) = (want_box, got_box) {
        let tol = 1e-4 * (1.0 + gusset_kernel::norm(w.size()));
        let far = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).any(|(a, b)| (a - b).abs() > tol);
        if far(w.min, g.min) || far(w.max, g.max) {
            return Err(format!(
                "the features lie in {:?}..{:?}, {whose} shapes in {:?}..{:?}",
                g.min, g.max, w.min, w.max
            ));
        }
    }
    let note = (rel > 1e-7).then(|| {
        format!("the features' volume differs from {whose} stored shapes by {rel:.1e} (approximated surfaces)")
    });
    Ok(note)
}

/// Largest relative volume difference between rebuilt features and the shapes stored with
/// them (FreeCAD documents, SolidWorks parts).
const FREECAD_VOLUME_TOLERANCE: f64 = 2e-5;

/// Most base shapes read for the edges of one FreeCAD document's fillets and chamfers.
const MAX_FREECAD_EDGE_SHAPES: usize = 64;

/// Most OpenSCAD objects written as features in one import (security review S3).
const MAX_SCAD_FEATURE_OBJECTS: usize = 500;

/// Most bytes of parsed shapes one FreeCAD import keeps (review X4).
const MAX_FREECAD_BYTES: usize = 2 << 30;

/// Most B-rep parts one Fusion archive may hold.
const MAX_F3D_PARTS: usize = 256;

/// Largest inflated entry of a Fusion archive (a part, or a design inside an `.f3z`).
const MAX_F3D_PART_BYTES: u64 = 256 << 20;

/// Largest design stream read for component placements.
const MAX_F3D_STREAM_BYTES: u64 = 256 << 20;

/// Largest `Manifest.json` / `DesignDescription.json` of an `.f3z`.
const MAX_F3D_JSON_BYTES: u64 = 4 << 20;

/// Most bodies one Fusion import creates (parts times the instances of their components and
/// of the designs that reference them).
const MAX_F3D_BODIES: usize = 4096;

/// Most STEP text the bodies of one SolidWorks part's imported feature make together.
const MAX_SLDPRT_STEP_BYTES: usize = 1 << 30;

/// Most design streams read from one Fusion file, and most placements kept over all of them
/// (the file is refused past either: review S2).
const MAX_F3D_STREAMS: usize = 256;
const MAX_F3D_PLACEMENTS: usize = 200_000;

/// Most appearance packages (`ProteinAsset.*.protein`) read from one Fusion file, the largest
/// `InstanceProperties.bin` read from one, and most asset colours and material colours kept.
const MAX_F3D_PROTEINS: usize = 4096;
const MAX_F3D_PROTEIN_BYTES: u64 = 32 << 20;
const MAX_F3D_COLOURS: usize = 200_000;

/// Most bytes of imported shapes one Fusion import keeps (review S4).
const MAX_F3D_OUTPUT_BYTES: usize = 1 << 30;

/// Deepest chain of designs referencing designs followed.
const MAX_F3D_XREF_DEPTH: usize = 8;

/// One design of a Fusion file: its entries (B-rep parts and design streams) and what the
/// streams say about placements.
struct F3dDesign {
    /// The design's file name inside an `.f3z` (empty for a single `.f3d`).
    file: String,
    entries: Vec<(String, Vec<u8>)>,
    info: gusset_sat::fusion::Design,
}

/// The file each external-reference key of an `.f3z` names: `DesignDescription.json` lists the
/// designs (`designObjects`, each with a `relativePath`) and for each the references it holds
/// (`references[].relationships[]`: a design object id and a `neutronRole` key).
fn xref_files(description: &Value) -> std::collections::HashMap<String, String> {
    let mut path_of = std::collections::HashMap::new();
    let graphs = description.pointer("/designDescription/designGraphs").and_then(Value::as_array);
    for g in graphs.into_iter().flatten().take(16) {
        for o in g.get("designObjects").and_then(Value::as_array).into_iter().flatten().take(4096) {
            if let (Some(id), Some(p)) =
                (o.get("id").and_then(Value::as_i64), o.get("relativePath").and_then(Value::as_str))
            {
                path_of.insert(id, p.to_string());
            }
        }
    }
    let mut out = std::collections::HashMap::new();
    for g in graphs.into_iter().flatten().take(16) {
        for o in g.get("designObjects").and_then(Value::as_array).into_iter().flatten().take(4096) {
            for r in o.get("references").and_then(Value::as_array).into_iter().flatten().take(64) {
                for rel in r.get("relationships").and_then(Value::as_array).into_iter().flatten().take(4096) {
                    let role = rel.pointer("/metadata/neutronRole").and_then(Value::as_str);
                    let file = rel.get("id").and_then(Value::as_i64).and_then(|id| path_of.get(&id));
                    if let (Some(role), Some(file)) = (role, file) {
                        out.insert(role.to_ascii_lowercase(), file.clone());
                    }
                }
            }
        }
    }
    out
}

/// One use of a design inside the root design: where its origin sits, and where the root put
/// the design's own context component (an electronics board) when it moved it.
type Use = (gusset_sat::fusion::Placement, Option<gusset_sat::fusion::Placement>, Vec<gusset_sat::fusion::Placement>);

/// Where each design of an `.f3z` stands in the root design: one [`Use`] per use of the design
/// (references to references composed). `None` for a design that cannot be reached from the
/// root.
fn design_instances(
    designs: &[F3dDesign],
    root: usize,
    files: &std::collections::HashMap<String, String>,
    warnings: &mut Vec<String>,
) -> Vec<Option<Vec<Use>>> {
    use gusset_sat::fusion::Placement;
    let mut out: Vec<Option<Vec<Use>>> = vec![None; designs.len()];
    if let Some(slot) = out.get_mut(root) {
        *slot = Some(vec![(Placement::IDENTITY, None, Vec::new())]);
    }
    let mut frontier = vec![root];
    for _ in 0..MAX_F3D_XREF_DEPTH {
        let mut next = Vec::new();
        for &p in &frontier {
            let Some(parent) = designs.get(p) else { continue };
            let Some(Some(ps)) = out.get(p).cloned() else { continue };
            for x in &parent.info.xrefs {
                let Some(file) = files.get(&x.role) else { continue };
                let Some(c) = designs.iter().position(|d| &d.file == file) else { continue };
                if c == p || out.get(c).is_some_and(Option::is_some) {
                    continue;
                }
                let mut placed = Vec::new();
                for q in &ps {
                    for (i, inst) in x.instances.iter().enumerate() {
                        if placed.len() < MAX_F3D_BODIES {
                            let over = x.nested.get(i).copied().flatten().map(|o| q.0.then(&o));
                            let parts = x.parts.iter().map(|o| q.0.then(o)).collect();
                            placed.push((q.0.then(inst), over, parts));
                        }
                    }
                }
                if let Some(slot) = out.get_mut(c) {
                    *slot = Some(placed);
                }
                next.push(c);
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    for (d, o) in designs.iter().zip(&out) {
        if o.is_none() {
            warnings.push(format!("the design {} is not placed in the archive's root design", d.file));
        }
    }
    out
}

/// Counts `add` more placements against [`MAX_F3D_PLACEMENTS`] (all streams of one file).
fn charge_placements(kept: &mut usize, add: usize) -> Result<(), CommandError> {
    *kept = kept.saturating_add(add);
    if *kept > MAX_F3D_PLACEMENTS {
        return Err(CommandError::new(
            "invalid_args",
            format!("the Fusion file places more than {MAX_F3D_PLACEMENTS} bodies and references"),
        ));
    }
    Ok(())
}

/// A Fusion 360 `.f3d` archive: every `Breps.BlobParts/*.smb` (binary ACIS of the design's
/// bodies; `.smbh` holds their history and is skipped), converted to STEP and imported.
/// Each body is placed by the occurrences of its component, nested components composed, read
/// from the design stream (`Design1/BulkStream.dat`, gusset_sat::fusion); a component used
/// several times gives the body several times; bodies of the root component stay put. An
/// `.f3z` holds several designs: the others are placed where the root design references them.
fn f3d(
    bytes: &[u8],
    importer: &dyn Importer,
    cancel: &CancelFlag,
) -> Result<(Vec<PreparedBody>, &'static str, Vec<String>), CommandError> {
    use gusset_sat::fusion::Placement;
    let limits = gusset_meshio::Limits::default();
    let bad = |e: gusset_meshio::MeshIoError| {
        let e = e.to_string().replace("3MF ZIP container", "Fusion file (ZIP)");
        CommandError::new("io", format!("the Fusion file cannot be read: {e}"))
    };
    let mut budget = limits.max_decompressed_bytes;
    let is_brep = |n: &str| n.contains("Breps.BlobParts/") && n.to_ascii_lowercase().ends_with(".smb");
    let is_stream = |n: &str| n.ends_with("/BulkStream.dat");
    let is_protein = |n: &str| n.contains("ProteinAssets.BlobParts/") && n.ends_with(".protein");
    // The history blobs (`BREP.<id>.smbh`) keep the face materials some body blobs lost.
    let is_hist = |n: &str| n.contains("Breps.BlobParts/") && n.to_ascii_lowercase().ends_with(".smbh");
    let wanted = |n: &str| is_brep(n) || is_hist(n) || is_stream(n) || is_protein(n);
    let read = |data: &[u8], budget: &mut u64| {
        gusset_meshio::archive::read_entries_within(data, &limits, &wanted, MAX_F3D_STREAM_BYTES, budget).map_err(bad)
    };
    // The streams that describe the design itself: an asset named `[Active]` when there is one
    // (the others, `Animation`, hold other states of the same parts).
    let (mut streams_read, mut placements_kept) = (0usize, 0usize);
    // Fusion colours (ADR 0039): material id to asset id from the streams, asset id to colour
    // from the appearance packages.
    let mut materials: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut asset_colours: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut proteins_read = 0usize;
    let mut design_info = |entries: &[(String, Vec<u8>)]| -> Result<gusset_sat::fusion::Design, CommandError> {
        let streams: Vec<&(String, Vec<u8>)> = entries.iter().filter(|(n, _)| is_stream(n)).collect();
        let active: Vec<&(String, Vec<u8>)> = streams.iter().filter(|(n, _)| n.contains("[Active]")).copied().collect();
        let chosen = if active.is_empty() { streams } else { active };
        let mut info = gusset_sat::fusion::Design::default();
        for (_, b) in chosen {
            streams_read += 1;
            if streams_read > MAX_F3D_STREAMS {
                return Err(CommandError::new(
                    "invalid_args",
                    format!("the Fusion file has more than {MAX_F3D_STREAMS} design streams"),
                ));
            }
            let Some(d) = gusset_sat::fusion::read_design_cancellable(b, cancel) else { return Err(cancelled()) };
            charge_placements(
                &mut placements_kept,
                d.bodies.len() + d.xrefs.iter().map(|x| x.instances.len()).sum::<usize>(),
            )?;
            for (m, a) in gusset_sat::fusion::material_assets(b) {
                if materials.len() < MAX_F3D_COLOURS {
                    materials.entry(m).or_insert(a);
                }
            }
            info.bodies.extend(d.bodies);
            info.xrefs.extend(d.xrefs);
            info.context = info.context.or(d.context);
            info.groups.extend(d.groups);
            info.history.extend(d.history);
            info.body_colours.extend(d.body_colours);
        }
        Ok(info)
    };
    // The designs: the file itself, or for an `.f3z` the `.f3d` designs it holds (one level
    // deep, sharing the decompression budget).
    let mut first = read(bytes, &mut budget)?;
    let mut take_proteins = |entries: &mut Vec<(String, Vec<u8>)>, budget: &mut u64| {
        let (proteins, rest): (Vec<_>, Vec<_>) = std::mem::take(entries).into_iter().partition(|(n, _)| is_protein(n));
        *entries = rest;
        for (_, p) in proteins {
            proteins_read += 1;
            if proteins_read > MAX_F3D_PROTEINS {
                break;
            }
            // A damaged package only loses its colours.
            let Ok(bins) = gusset_meshio::archive::read_entries_within(
                &p,
                &limits,
                &|n| n.ends_with("/AssetData/InstanceProperties.bin"),
                MAX_F3D_PROTEIN_BYTES,
                budget,
            ) else {
                continue;
            };
            for (_, bin) in bins {
                for (id, c) in gusset_sat::fusion::instance_colours(&bin) {
                    if asset_colours.len() < MAX_F3D_COLOURS {
                        asset_colours.entry(id).or_insert(c);
                    }
                }
            }
        }
    };
    take_proteins(&mut first, &mut budget);
    let mut designs: Vec<F3dDesign> = Vec::new();
    let mut root = 0usize;
    let mut files = std::collections::HashMap::new();
    let mut warnings = Vec::new();
    if first.iter().any(|(n, _)| is_brep(n)) {
        let info = design_info(&first)?;
        designs.push(F3dDesign { file: String::new(), entries: first, info });
    } else {
        let inner = gusset_meshio::archive::read_entries_within(
            bytes,
            &limits,
            &|n| n.to_ascii_lowercase().ends_with(".f3d"),
            MAX_F3D_PART_BYTES,
            &mut budget,
        )
        .map_err(bad)?;
        for (name, d) in inner.iter().take(MAX_F3D_PARTS) {
            let mut entries = read(d, &mut budget)?;
            take_proteins(&mut entries, &mut budget);
            let info = design_info(&entries)?;
            designs.push(F3dDesign { file: name.clone(), entries, info });
        }
        // `Manifest.json` names the root design, `DesignDescription.json` the references.
        let json = gusset_meshio::archive::read_entries_within(
            bytes,
            &limits,
            &|n| n == "Manifest.json" || n == "DesignDescription.json",
            MAX_F3D_JSON_BYTES,
            &mut budget,
        )
        .map_err(bad)?;
        let parse = |name: &str| {
            json.iter().find(|(n, _)| n == name).and_then(|(_, b)| serde_json::from_slice::<Value>(b).ok())
        };
        if let Some(r) = parse("Manifest.json").as_ref().and_then(|m| m.get("root")).and_then(Value::as_str)
            && let Some(i) = designs.iter().position(|d| d.file == r)
        {
            root = i;
        }
        files = parse("DesignDescription.json").map(|d| xref_files(&d)).unwrap_or_default();
    }
    let colours: gusset_sat::Colours = materials
        .into_iter()
        .filter_map(|(m, a)| asset_colours.get(&a).map(|c| (m, *c)))
        .take(MAX_F3D_COLOURS)
        .collect();
    let count = designs.iter().flat_map(|d| &d.entries).filter(|(n, _)| is_brep(n)).count();
    if count == 0 {
        return Err(CommandError::new("io", "the Fusion file holds no B-rep bodies (Breps.BlobParts/*.smb)"));
    }
    if count > MAX_F3D_PARTS {
        return Err(CommandError::new("io", format!("the Fusion file has more than {MAX_F3D_PARTS} B-rep parts")));
    }
    // Where each design stands: a single design is at the origin; the designs of an archive are
    // placed by the references of the root design.
    let instances: Vec<Option<Vec<Use>>> = if designs.len() > 1 {
        design_instances(&designs, root, &files, &mut warnings)
    } else {
        vec![Some(vec![(Placement::IDENTITY, None, Vec::new())]); designs.len()]
    };
    // Solid bodies only, when there are any: Fusion keeps construction and decal surfaces
    // (a board's printed canvas) as sheet bodies beside the solids (dogfooding F6-8). A file
    // of surfaces only is read again with its sheets.
    let (mut out, mut placed, mut skipped, mut truncated) = (Vec::new(), 0, 0, false);
    let mut part_warnings = Vec::new();
    for keep_sheets in [false, true] {
        (out, part_warnings, placed, skipped, truncated) = (Vec::new(), Vec::new(), 0, 0, false);
        let mut i = 0;
        let mut kept_bytes = 0usize;
        for (design, insts) in designs.iter().zip(&instances) {
            let default = [(Placement::IDENTITY, None, Vec::new())];
            let insts: &[Use] = insts.as_deref().unwrap_or(&default);
            // The root design's own bodies stay where they are; those of a referenced design
            // follow the reference.
            let referenced = designs.len() > 1 && design.file != designs.get(root).map_or("", |r| r.file.as_str());
            let mut places: std::collections::HashMap<&str, Vec<&gusset_sat::fusion::Placed>> =
                std::collections::HashMap::new();
            for p in &design.info.bodies {
                places.entry(p.brep.as_str()).or_default().push(p);
            }
            let hist_blobs: std::collections::HashMap<&str, &[u8]> = design
                .entries
                .iter()
                .filter(|(n, _)| is_hist(n))
                .filter_map(|(n, b)| {
                    let id = n.rsplit("BREP.").next()?.split('.').next()?;
                    Some((id, b.as_slice()))
                })
                .collect();
            // The component each body blob belongs to, when the design lists components that
            // the referencing design placed one by one (the Pico case, ADR 0039).
            let group_of: std::collections::HashMap<&str, usize> = design
                .info
                .groups
                .iter()
                .enumerate()
                .flat_map(|(k, (_, v))| v.iter().map(move |id| (id.as_str(), k)))
                .collect();
            for (entry, data) in design.entries.iter().filter(|(n, _)| is_brep(n)) {
                i += 1;
                let id = entry.rsplit("BREP.").next().and_then(|t| t.split('.').next()).unwrap_or_default();
                // The placements of this body inside its design, or its absence from them.
                let own: Vec<Option<&gusset_sat::fusion::Placed>> = match places.get(id) {
                    Some(v) => v.iter().copied().map(Some).collect(),
                    None => vec![None],
                };
                // The face materials of the body's history blob, for faces the body lost them on.
                let history = design
                    .info
                    .history
                    .get(id)
                    .filter(|_| !colours.is_empty())
                    .and_then(|h| hist_blobs.get(h.as_str()))
                    .and_then(|b| gusset_sat::History::read(b, &gusset_sat::Limits::default()));
                // The colours of an imported feature's bodies, from its body references.
                let history = match design.info.body_colours.get(id) {
                    Some(c) => Some(history.unwrap_or_default().with_body_colours(c.clone())),
                    None => history,
                };
                for p in own {
                    for (origin, moved, parts) in insts {
                        let (origin, moved) = (*origin, *moved);
                        // Design coordinates to the root's: the reference's matrix, or the root's
                        // own placement of the design's context component taken as the design's.
                        // A design without a context component (the HDMI connector) was moved as
                        // a whole: the placement the root stored for it is the design's own.
                        let part = match group_of.get(id) {
                            Some(k) if parts.len() == design.info.groups.len() => parts.get(*k).copied(),
                            _ => None,
                        };
                        let inst = match (part, moved, design.info.context) {
                            (Some(w), _, _) => w,
                            (None, Some(o), Some(ctx)) => o.then(&ctx.inverse()),
                            (None, Some(o), None) => o,
                            _ => origin,
                        };
                        if cancel.load(Ordering::Relaxed) {
                            return Err(cancelled());
                        }
                        if out.len() >= MAX_F3D_BODIES {
                            truncated = true;
                            break;
                        }
                        let stem = match p {
                            Some(p) => p.occurrence.clone(),
                            None if count > 1 => format!("Fusion {i}"),
                            None => "Fusion".to_string(),
                        };
                        let place = match (p, referenced) {
                            (Some(p), _) => Some(inst.then(&p.placement)),
                            (None, true) => Some(inst),
                            (None, false) => None,
                        };
                        let c = match gusset_sat::to_step_coloured_history(
                            data,
                            &stem,
                            &gusset_sat::Limits::default(),
                            place.as_ref(),
                            keep_sheets,
                            &colours,
                            history.as_ref(),
                        ) {
                            Ok(c) => c,
                            Err(e) => {
                                part_warnings.push(format!("B-rep part {i}: {e}"));
                                break;
                            }
                        };
                        placed += usize::from(place.is_some());
                        skipped += c.skipped_sheets;
                        part_warnings.extend(c.warnings);
                        if c.bodies == 0 {
                            break;
                        }
                        let bodies = importer.import("step", c.step.as_bytes(), cancel).map_err(|e| {
                            if cancel.load(Ordering::Relaxed) { cancelled() } else { CommandError::new("io", e) }
                        })?;
                        kept_bytes += bodies.iter().map(|(_, blob)| blob.len()).sum::<usize>();
                        if kept_bytes > MAX_F3D_OUTPUT_BYTES {
                            return Err(CommandError::new(
                                "invalid_args",
                                format!(
                                    "the Fusion file's bodies and their instances exceed {} MiB of shapes",
                                    MAX_F3D_OUTPUT_BYTES >> 20
                                ),
                            ));
                        }
                        out.extend(bodies.into_iter().map(|(name, blob)| prepared(&name, blob, ImportFormat::Step)));
                    }
                }
            }
        }
        if !out.is_empty() || skipped == 0 {
            break;
        }
    }
    warnings.extend(part_warnings);
    if skipped > 0 {
        warnings.insert(0, format!("{skipped} sheet (surface) bodies left out; the solids are imported"));
    }
    if out.is_empty() {
        return Err(CommandError::new(
            "io",
            format!("no body of the Fusion file could be read: {}", warnings.join("; ")),
        ));
    }
    if truncated {
        warnings.push(format!("only the first {MAX_F3D_BODIES} bodies are imported"));
    }
    if placed > 0 {
        warnings.insert(0, format!("{placed} bodies placed by their component occurrences (count includes repeats)"));
    }
    warnings.truncate(50);
    Ok((out, TIER_SAT, warnings))
}

/// The visible shapes of a FreeCAD document, each parsed by the sandboxed worker. A shape
/// the worker refuses is reported as a warning; none at all is an error.
fn freecad(
    bytes: &[u8],
    importer: &dyn Importer,
    cancel: &CancelFlag,
) -> Result<(Vec<PreparedBody>, &'static str, Vec<String>), CommandError> {
    let limits = gusset_meshio::Limits::default();
    // The ZIP reader is shared with 3MF; name the right format (F5-10).
    let bad = |e: gusset_meshio::MeshIoError| {
        let e = e.to_string().replace("3MF ZIP container", "FreeCAD file (ZIP)");
        CommandError::new("io", format!("the FreeCAD file cannot be read: {e}"))
    };
    // One decompression budget for both passes, and the XML refused by its declared size
    // before it is inflated (security review X4).
    let mut budget = limits.max_decompressed_bytes;
    let xml = gusset_meshio::archive::read_entries_within(
        bytes,
        &limits,
        &|n| n == "Document.xml" || n == "GuiDocument.xml",
        crate::fcstd::MAX_XML as u64,
        &mut budget,
    )
    .map_err(bad)?;
    let get = |n: &str| xml.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_slice());
    let doc = get("Document.xml")
        .ok_or_else(|| CommandError::new("io", "the file has no Document.xml: not a FreeCAD document"))?;
    let picks = crate::fcstd::picks(doc, get("GuiDocument.xml")).map_err(|e| CommandError::new("io", e))?;
    if picks.is_empty() {
        return Err(CommandError::new("io", "the FreeCAD document shows no solid objects"));
    }
    let files: std::collections::BTreeSet<&str> = picks.iter().map(|p| p.file.as_str()).collect();
    let shapes =
        gusset_meshio::archive::read_entries_within(bytes, &limits, &|n| files.contains(n), u64::MAX, &mut budget)
            .map_err(bad)?;
    // Each shape file is parsed once, however many objects name it (review X4).
    let mut parsed: std::collections::HashMap<&str, Result<Vec<Vec<u8>>, String>> = std::collections::HashMap::new();
    let (mut out, mut warnings) = (Vec::new(), Vec::new());
    match get("GuiDocument.xml") {
        Some(g) => {
            for l in crate::fcstd::hidden(doc, g).map_err(|e| CommandError::new("io", e))?.into_iter().take(50) {
                warnings.push(format!("skipped hidden object '{l}'"));
            }
        }
        None => warnings.push("no GuiDocument.xml: visibility unknown, every top-level object imported".into()),
    }
    let mut kept = 0usize;
    for p in &picks {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let Some((_, data)) = shapes.iter().find(|(n, _)| *n == p.file) else {
            warnings.push(format!("{}: its shape file is missing", p.label));
            continue;
        };
        if !parsed.contains_key(p.file.as_str()) {
            // FreeCAD writes BRep text (.brp), or OCCT binary (.bin) when asked to.
            let format = if p.file.to_ascii_lowercase().ends_with(".bin") { "brep_bin" } else { "brp" };
            let r = match importer.import(format, data, cancel) {
                Ok(bodies) => Ok(bodies.into_iter().map(|(_, blob)| blob).collect()),
                Err(_) if cancel.load(Ordering::Relaxed) => return Err(cancelled()),
                Err(e) => Err(e.to_string()),
            };
            parsed.insert(p.file.as_str(), r);
        }
        match parsed.get(p.file.as_str()) {
            Some(Ok(blobs)) => {
                for blob in blobs {
                    kept += blob.len();
                    if kept > MAX_FREECAD_BYTES {
                        return Err(CommandError::new("io", "the FreeCAD document's shapes are too large"));
                    }
                    out.push(PreparedBody {
                        name: p.label.clone(),
                        blob: blob.clone(),
                        format: ImportFormat::FreeCad,
                        color: None,
                        faces: Vec::new(),
                        scad: None,
                    });
                }
            }
            Some(Err(e)) => warnings.push(format!("{}: {e}", p.label)),
            None => {}
        }
    }
    if out.is_empty() {
        // The labels come from the file: in a `_user_text` field (review X5).
        let mut e = CommandError::new("io", "no FreeCAD shape could be read");
        e.details = Some(serde_json::json!({ "warnings_user_text": warnings }));
        return Err(e);
    }
    Ok((out, TIER_FREECAD, warnings))
}

/// Phase 3 without policy checks: stores the blobs and commits the import features.
pub(crate) fn commit_import(s: &mut Session, ctx: &CmdCtx, p: PreparedImport) -> Result<Value, CommandError> {
    if let Some(tree) = &p.tree {
        let mut out = crate::tree_import::apply(s, ctx, tree, &p.file_name)?;
        // A FreeCAD document's or SolidWorks part's history says where it came from (not an
        // exporter plugin).
        if (p.tier == TIER_FREECAD_HISTORY
            || p.tier == TIER_SOLIDWORKS_HISTORY
            || p.tier == TIER_INVENTOR_HISTORY
            || p.tier == TIER_FUSION_HISTORY)
            && let Some(o) = out.as_object_mut()
        {
            o.insert("tier".into(), json!(p.tier));
            if let Some(w) = o.get_mut("warnings_user_text").and_then(Value::as_array_mut) {
                w.extend(p.warnings.iter().take(20).map(|x| Value::from(x.as_str())));
            }
        }
        return Ok(out);
    }
    // (label, blob hash, format, colour, face colours)
    type Stored<'a> = (String, String, ImportFormat, Option<u32>, &'a [(u32, u32)]);
    let stored: Vec<Stored<'_>> = p
        .bodies
        .iter()
        .map(|b| (b.name.clone(), s.put_blob(&b.blob).to_hex(), b.format, b.color, b.faces.as_slice()))
        .collect();
    let file_name = p.file_name.clone();
    let (tier, mut warnings) = (p.tier, p.warnings);
    // OpenSCAD objects as features (ADR 0033 amendment): names that clash with nothing in
    // the document; an object the features cannot say stays an exact body.
    let mut written: Vec<(usize, String, String, f64)> = Vec::new();
    if p.bodies.iter().any(|b| b.scad.is_some()) {
        let doc = s.document(&ctx.actor)?;
        let mut taken: std::collections::HashSet<String> =
            doc.features.iter().map(|f| f.name.to_ascii_lowercase()).collect();
        taken.extend(doc.params.iter().map(|p| p.name.to_ascii_lowercase()));
        let mut names = crate::scad_text::Names::new(taken);
        for (i, b) in p.bodies.iter().enumerate() {
            let Some((tree, volume)) = &b.scad else { continue };
            match crate::scad_text::object_text(tree, &mut names) {
                Ok((text, body)) => written.push((i, text, body, *volume)),
                Err(why) => warnings.push(format!("{}: imported as one exact body ({why})", b.name)),
            }
        }
    }
    // The features must also stand in the whole document: its limits (validation) and the
    // evaluator's document-wide budgets apply to them together with what is there (code
    // review R1, R2). Checked on a candidate before anything is committed; if they do not,
    // every object comes in as an exact body.
    if !written.is_empty() {
        let doc = s.document(&ctx.actor)?;
        let text: String = written.iter().map(|w| w.1.as_str()).collect();
        let candidate = gusset_doc::Document::from_text(&format!("{}\n{text}", doc.to_text()));
        let why = match candidate {
            Err(e) => Some(format!("the features would not fit the document ({e})")),
            Ok(cand) => {
                let before = s.evaluate(&ctx.actor)?.errors().len();
                let ev = s.evaluate_doc(&cand, None);
                let bodies_ok = written.iter().all(|w| ev.bodies.iter().any(|b| b.feature == w.2));
                (ev.errors().len() > before || !bodies_ok)
                    .then(|| "the features would not rebuild in this document".to_string())
            }
        };
        if let Some(why) = why {
            warnings.push(format!("the OpenSCAD objects came in as exact bodies: {why}"));
            written.clear();
        }
    }
    let feature_text: String = written.iter().map(|w| w.1.as_str()).collect();
    let as_features: std::collections::HashSet<usize> = written.iter().map(|w| w.0).collect();
    let warnings_now = warnings.clone();
    let mut out = edit(s, ctx, &format!("Import {file_name}"), |doc, rb| {
        // Features go at the end; with the timeline rolled back they would land after the
        // marker, so the objects come in as exact bodies then.
        let features_ok = rb.is_none() && !feature_text.is_empty();
        let mut names = Vec::new();
        for (i, (body_name, blob, format, color, faces)) in stored.iter().enumerate() {
            if features_ok && as_features.contains(&i) {
                continue;
            }
            let name = doc.unique_feature_name("Import");
            let mut f = Feature::new(
                name.clone(),
                FeatureKind::Import(ImportDef { blob: blob.clone(), format: *format, file_name: file_name.clone() }),
            );
            f.label = Some(body_name.chars().take(200).collect());
            insert_feature(doc, rb.map(|r| r + i), f);
            // The file's colour, as the body's appearance (ADR 0045).
            // Within the document's appearance limit (security review T6): past it the
            // body keeps the default look.
            if let Some(c) = color
                && doc.appearances.len() < gusset_doc::text::MAX_LIST_ITEMS
            {
                doc.appearances.retain(|a| a.body != name);
                // Face colours within the document's total (face colours review #4); past it
                // the body keeps its one colour.
                let used: usize = doc.appearances.iter().flat_map(|a| &a.faces).map(|g| g.faces.len()).sum();
                let faces = if used + faces.len() <= gusset_doc::text::MAX_LIST_ITEMS { faces } else { &[][..] };
                doc.appearances.push(gusset_doc::Appearance {
                    body: name.clone(),
                    color: *c,
                    finish: gusset_doc::Finish::Plastic,
                    faces: face_groups(&name, faces),
                    texture: None,
                });
            }
            names.push(name);
        }
        let mut made = Vec::new();
        if features_ok {
            let text = format!("{}\n{feature_text}", doc.to_text());
            *doc = gusset_doc::text::parse(&text).map_err(|e| {
                CommandError::new(
                    "invalid_document",
                    format!("the features written for the OpenSCAD file do not parse: {e}"),
                )
            })?;
            made = written.iter().map(|w| w.2.clone()).collect();
            names.extend(made.iter().cloned());
        }
        let mut warnings: Vec<String> = warnings_now.iter().take(20).cloned().collect();
        if rb.is_some() && !feature_text.is_empty() {
            // Dogfooding F17-5: say why the objects are not features.
            warnings.push(
                "the timeline is rolled back, so the OpenSCAD objects came in as exact bodies; move the marker to the end and import again for features"
                    .into(),
            );
        }
        let tier = if made.is_empty() { tier } else { "tier 1: OpenSCAD objects as editable features" };
        Ok(json!({ "features": names, "tier": tier, "warnings_user_text": warnings, "scad_features": made }))
    })?;
    // Each object written as features against its exact build.
    let made: Vec<String> = out
        .get("scad_features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect();
    if !made.is_empty() {
        let ev = s.evaluate(&ctx.actor)?;
        let k = s.kernel().clone();
        let mut notes = Vec::new();
        for (_, _, body, want) in &written {
            let got = ev
                .bodies
                .iter()
                .find(|b| &b.feature == body)
                .and_then(|b| k.mass_properties(&b.shape).ok())
                .map(|m| m.volume);
            match got {
                Some(v) if want.is_finite() && (v - want).abs() > 1e-6 * want.abs().max(1.0) => {
                    notes.push(format!("{body}: volume {v:.4} mm3, the exact build's {want:.4} mm3"));
                }
                None => notes.push(format!("{body}: the features made no body")),
                _ => {}
            }
        }
        if let Some(w) = out.get_mut("warnings_user_text").and_then(Value::as_array_mut) {
            w.extend(notes.into_iter().map(Value::from));
        }
    }
    Ok(out)
}

/// Face colours as the appearance's groups, one per colour in first-seen order.
fn face_groups(feature: &str, faces: &[(u32, u32)]) -> Vec<gusset_doc::FaceColor> {
    let mut groups: Vec<gusset_doc::FaceColor> = Vec::new();
    let mut by_colour: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for &(i, c) in faces {
        let face = gusset_doc::FaceRef::new(feature, gusset_doc::FaceKey::Imported(i));
        let k = *by_colour.entry(c).or_insert_with(|| {
            groups.push(gusset_doc::FaceColor { color: c, faces: Vec::new() });
            groups.len() - 1
        });
        if let Some(g) = groups.get_mut(k) {
            g.faces.push(face);
        }
    }
    groups
}

fn import_spec() -> Result<&'static CommandSpec, CommandError> {
    registry::find("import.file").ok_or_else(|| CommandError::not_found("import.file is not registered"))
}

/// Phase 1 for hosts: policy (rate limit, read-only, approvals), argument parsing and
/// opening the file. Refusals are audited like any command.
///
/// # Errors
/// The command's policy, argument and file errors.
pub fn begin_import(s: &mut Session, ctx: &CmdCtx, args: Value) -> Result<ImportJob, CommandError> {
    let spec = import_spec()?;
    let detail = registry::audit_detail(ctx, &args);
    let r = registry::check_policy(s, ctx, spec, true).and_then(|()| {
        let a: crate::commands::ImportFile = serde_json::from_value(if args.is_null() { json!({}) } else { args })
            .map_err(|e| CommandError::args(format!("import.file: {e}")))?;
        open_import(s, ctx, &a.path)
    });
    if let Err(e) = &r {
        registry::audit(s, ctx, spec, &detail, Err(e));
    }
    r
}

/// Phase 3 for hosts: re-checks policy (the user may have switched agents to read-only
/// meanwhile), commits the import (or records the failure of phase 2) and audits it.
///
/// # Errors
/// The error of phase 2, a policy refusal, or a commit error.
pub fn complete_import(
    s: &mut Session,
    ctx: &CmdCtx,
    prepared: Result<PreparedImport, CommandError>,
) -> Result<Value, CommandError> {
    let spec = import_spec()?;
    let r = prepared.and_then(|p| registry::check_policy(s, ctx, spec, false).and_then(|()| commit_import(s, ctx, p)));
    registry::audit(s, ctx, spec, "", r.as_ref().map(|_| ()));
    r
}

#[cfg(test)]
mod f3d_tests {
    use super::*;

    struct Never;
    impl Importer for Never {
        fn import(&self, _: &str, _: &[u8], _: &AtomicBool) -> Result<Vec<(String, Vec<u8>)>, String> {
            Err("not expected".into())
        }
    }

    #[test]
    fn placements_are_capped_over_all_streams() {
        let mut kept = 0;
        assert!(charge_placements(&mut kept, MAX_F3D_PLACEMENTS).is_ok());
        let e = charge_placements(&mut kept, 1).unwrap_err();
        assert!(e.message.contains("places more than"), "{e:?}");
        assert!(charge_placements(&mut 0, usize::MAX).is_err());
    }

    #[test]
    fn a_file_with_too_many_design_streams_is_refused() {
        let mut names: Vec<String> = (0..=MAX_F3D_STREAMS).map(|n| format!("D{n}/BulkStream.dat")).collect();
        names.push("Breps.BlobParts/BREP.aa.smb".into());
        let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"noise"[..])).collect();
        let zip = gusset_meshio::archive::write(&entries).unwrap();
        let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
        let e = f3d(&zip, &Never, &cancel).unwrap_err();
        assert!(e.message.contains("design streams"), "{e:?}");
    }
}

#[cfg(test)]
mod look_tests {
    use super::*;

    #[test]
    fn worker_names_carry_body_and_face_colours() {
        assert_eq!(split_look("Gear"), ("Gear".into(), None, vec![]));
        assert_eq!(split_look("Gear\u{1f}#808080"), ("Gear".into(), Some(0x80_80_80), vec![]));
        let (n, c, f) = split_look("Gear\u{1f}#808080\u{1f}3=d02020,bad,4=zz0000,7=2020d0");
        assert_eq!((n.as_str(), c), ("Gear", Some(0x80_80_80)));
        assert_eq!(f, vec![(3, 0xd0_20_20), (7, 0x20_20_d0)]);
        // Face colours need a body colour.
        assert!(split_look("Gear\u{1f}-\u{1f}3=d02020").2.is_empty());
        let g = face_groups("Import1", &f[..].iter().chain(&[(9, 0xd0_20_20)]).copied().collect::<Vec<_>>());
        assert_eq!(g.len(), 2);
        assert_eq!(
            g[0].faces.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["Import1/face(3)", "Import1/face(9)"]
        );
    }
}

#[cfg(test)]
mod sldasm_tests {
    #![allow(clippy::float_cmp)]
    use super::*;

    #[test]
    fn placements_are_checked_and_composed() {
        // Quarter turn about z and a shift: only rigid placements pass.
        let m = [[0.0, -1.0, 0.0, 0.5], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let a = rigid(&m).unwrap();
        assert_eq!(a.1, [0.5, 0.0, 0.0]);
        let mut mirror = m;
        mirror[2][2] = -1.0;
        assert!(rigid(&mirror).is_none());
        let mut scaled = m;
        scaled[0][1] = -2.0;
        assert!(rigid(&scaled).is_none());
        let mut far = m;
        far[0][3] = f64::NAN;
        assert!(rigid(&far).is_none());
        // Two quarter turns make a half turn; the inner shift is turned by the outer.
        let (r, t) = compose_xf(&a, &a);
        assert_eq!(r[0], [-1.0, 0.0, 0.0]);
        assert!((t[0] - 0.5).abs() < 1e-12 && (t[1] - 0.5).abs() < 1e-12, "{t:?}");
    }

    #[test]
    fn parts_are_read_from_the_folder_only() {
        let dir = std::env::temp_dir().join(format!("gusset-sldasm-unit-{}", std::process::id()));
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.join("My_Part.SLDPRT"), b"part").unwrap();
        std::fs::write(sub.join("other.SLDPRT"), b"deep").unwrap();
        let cancel: CancelFlag = Arc::default();
        let mut f = Folder { dir: &dir, listing: None, read: 0, cancel: &cancel };
        // Exact, case-insensitive, and spaces as underscores.
        for n in ["My_Part.SLDPRT", "my_part.sldprt", "My Part.SLDPRT"] {
            assert_eq!(f.load(n).unwrap(), b"part", "{n}");
        }
        // Anything with a separator, a drive or a dot name is refused, and files of
        // sub-folders are not found by their plain name.
        for n in ["sub/other.SLDPRT", r"sub\other.SLDPRT", "../x.SLDPRT", "C:x.SLDPRT", "..", ".", "", "other.SLDPRT"] {
            assert!(f.load(n).is_err(), "{n}");
        }
        // Only SolidWorks files are read (S8), and the same name found again is the same file.
        std::fs::write(dir.join("notes.txt"), b"secret").unwrap();
        assert!(f.load("notes.txt").is_err());
        assert!(f.load("My_Part.SLDPRT").is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_assembly_without_parts_is_an_error() {
        struct Never;
        impl Importer for Never {
            fn import(&self, _: &str, _: &[u8], _: &AtomicBool) -> Result<Vec<(String, Vec<u8>)>, String> {
                Err("not expected".into())
            }
        }
        let cancel: CancelFlag = Arc::default();
        let dir = std::env::temp_dir();
        assert!(sldasm(b"not an assembly", &dir, &Never, &cancel).is_err());
    }
}

#[cfg(test)]
mod fusion_steps_tests {
    use super::*;
    use serde_json::json;

    /// The L-Bracket's steps: prism, blends, slot cut, 2 more slots, the first hole, 8 more.
    fn lbracket(v_count: u64) -> (Vec<Value>, Vec<f64>) {
        let tree = vec![
            json!({"type": "sketch", "name": "S1"}),
            json!({"type": "extrude", "name": "E1", "operation": "new"}),
            json!({"type": "fillet", "name": "F1"}),
            json!({"type": "extrude", "name": "E2", "operation": "cut"}),
            json!({"type": "pattern_linear", "name": "P1", "features": ["E2"], "count": 3}),
            json!({"type": "hole", "name": "H1", "positions": [[0, 0, 0]]}),
            json!({"type": "pattern_linear", "name": "P2", "features": ["H1"], "count": 2}),
            json!({"type": "pattern_linear", "name": "P2b", "features": ["H1"], "count": 2}),
            json!({"type": "pattern_linear", "name": "P2v", "features": ["H1", "P2", "P2b"], "count": v_count}),
        ];
        let (slot, hole) = (7941.786, 237.583);
        let mut v = vec![0.0, 0.0, 225500.0, 223479.535];
        v.push(v[3] - slot);
        v.push(v[4] - 2.0 * slot);
        v.push(v[5] - hole);
        v.push(v[6] - hole);
        v.push(v[7] - hole);
        v.push(v[8] - (v_count - 1) as f64 * 3.0 * hole);
        (tree, v)
    }

    /// A model of one body: its volume after each feature is the volume of the model.
    fn one_body(v: &[f64]) -> Vec<Vec<f64>> {
        v.iter().map(|x| if *x > 0.0 { vec![*x] } else { Vec::new() }).collect()
    }

    /// No tool can be measured against the model.
    fn none(_: usize, _: &[usize]) -> Result<Option<Vec<f64>>, String> {
        Ok(None)
    }

    fn names(t: &[Value]) -> Vec<String> {
        t.iter().map(|f| f["name"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn copies_come_from_the_tree() {
        let (t, _) = lbracket(3);
        assert_eq!(fusion_instances(&t), vec![None, None, None, Some(1), Some(2), Some(1), Some(1), Some(1), Some(6)]);
    }

    #[test]
    fn states_are_per_body_and_a_loose_cut_is_the_sum_of_its_tools() {
        // A board (100), a part (7), then a cut of three loops that made two tools (1 and 2.5).
        let t = vec![
            json!({"type": "extrude", "name": "Board", "operation": "new"}),
            json!({"type": "extrude", "name": "Part", "operation": "new"}),
            json!({"type": "extrude", "name": "Cut", "operation": "cut", "profiles": [{}, {}, {}]}),
        ];
        let per_body = vec![vec![], vec![100.0], vec![100.0, 7.0], vec![96.5, 7.0]];
        let vols = [0.0, 100.0, 107.0, 103.5];
        let stored = [100.0, 7.0, 1.0, 2.5];
        let got = fusion_explain_steps(
            &vols,
            &per_body,
            &stored,
            &fusion_instances(&t),
            &fusion_loose(&t),
            &names(&t),
            &mut none,
        );
        assert_eq!(got, Ok((2, 2)));
        // The tools do not add up to the cut: refused.
        let bad = [100.0, 7.0, 1.0, 2.0];
        let why = fusion_explain_steps(
            &vols,
            &per_body,
            &bad,
            &fusion_instances(&t),
            &fusion_loose(&t),
            &names(&t),
            &mut none,
        )
        .unwrap_err();
        assert!(why.contains("neither a state"), "{why}");
        // More tools than loops: refused.
        let many = [100.0, 7.0, 0.5, 0.5, 0.5, 0.5, 1.5];
        assert!(
            fusion_explain_steps(
                &vols,
                &per_body,
                &many,
                &fusion_instances(&t),
                &fusion_loose(&t),
                &names(&t),
                &mut none
            )
            .is_err()
        );
    }

    #[test]
    fn a_loose_cut_tool_in_a_void_counts_with_its_part_inside_the_body() {
        // A board with a slot (100), then a cut of three loops: the third tool (0.9) lies in the
        // slot and removes nothing.
        let t = vec![
            json!({"type": "extrude", "name": "Board", "operation": "new"}),
            json!({"type": "extrude", "name": "Cut", "operation": "cut", "profiles": [{}, {}, {}]}),
        ];
        let per_body = vec![vec![], vec![100.0], vec![96.5]];
        let vols = [0.0, 100.0, 96.5];
        let stored = [100.0, 1.0, 2.5, 0.9];
        let run = |parts: Vec<f64>| {
            let mut asked = Vec::new();
            let mut inside = |k: usize, tools: &[usize]| -> Result<Option<Vec<f64>>, String> {
                asked.push((k, tools.to_vec()));
                Ok(Some(parts.clone()))
            };
            let r = fusion_explain_steps(
                &vols,
                &per_body,
                &stored,
                &fusion_instances(&t),
                &fusion_loose(&t),
                &names(&t),
                &mut inside,
            );
            (r, asked)
        };
        let (got, asked) = run(vec![1.0, 2.5, 0.0]);
        assert_eq!(got, Ok((1, 3)));
        assert_eq!(asked, vec![(2, vec![1, 2, 3])]);
        // The measured parts do not add up to the cut: refused.
        assert!(run(vec![1.0, 2.4, 0.0]).0.is_err());
        // Not measurable: refused as before.
        let refused = fusion_explain_steps(
            &vols,
            &per_body,
            &stored,
            &fusion_instances(&t),
            &fusion_loose(&t),
            &names(&t),
            &mut none,
        );
        assert!(refused.unwrap_err().contains("neither a state"));
    }

    #[test]
    fn the_exact_pattern_count_is_accepted_and_a_wrong_one_refused() {
        let stored = [225500.0, 223479.535, 199654.177, 237.583, 7941.786];
        let (t, v) = lbracket(3);
        let ok = fusion_explain_steps(
            &v,
            &one_body(&v),
            &stored,
            &fusion_instances(&t),
            &fusion_loose(&t),
            &names(&t),
            &mut none,
        );
        assert_eq!(ok, Ok((3, 2)));
        // The file's volumes belong to three columns of holes; a tree making two refuses.
        let (t2, _) = lbracket(2);
        let why = fusion_explain_steps(
            &v,
            &one_body(&v),
            &stored,
            &fusion_instances(&t2),
            &fusion_loose(&t2),
            &names(&t2),
            &mut none,
        )
        .unwrap_err();
        assert!(why.contains("P2v") && why.contains("copies"), "{why}");
        // A tree whose copies cannot be derived (a pattern of a blend) is refused.
        let (mut t3, _) = lbracket(3);
        t3[8] = json!({"type": "pattern_linear", "name": "P2v", "features": ["F1"], "count": 3});
        let why = fusion_explain_steps(
            &v,
            &one_body(&v),
            &stored,
            &fusion_instances(&t3),
            &fusion_loose(&t3),
            &names(&t3),
            &mut none,
        )
        .unwrap_err();
        assert!(why.contains("cannot be derived"), "{why}");
    }
}
