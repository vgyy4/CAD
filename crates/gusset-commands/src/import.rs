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
    /// An ACIS SAT file, converted here to STEP and imported by the worker (ADR 0039).
    Sat,
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
    PreparedBody { name, blob, format, color, faces }
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
        "sat" | "sab" => Kind::Sat,
        "f3d" | "f3z" => Kind::F3d,
        "scad" => Kind::Scad,
        "json" => Kind::Tree,
        other => match gusset_meshio::Format::from_extension(other) {
            Some(f) => Kind::Mesh(f),
            None => {
                return Err(CommandError::args(format!("'.{other}' files cannot be imported"))
                    .hint("supported: .step, .stp, .iges, .igs, .x_t, .x_b, .sldprt, .sat, .sab, .f3d, .f3z, .fcstd, .scad, .gtree.json, .stl, .3mf, .obj, .ply, .gltf, .glb"));
            }
        },
    };
    let importer = s.importer.clone();
    if matches!(kind, Kind::Brep(_) | Kind::FreeCad | Kind::Xt | Kind::Sldprt | Kind::Sat | Kind::F3d)
        && importer.is_none()
    {
        return Err(CommandError::new(
            "not_allowed",
            "STEP/IGES/Parasolid/FreeCAD import is not available in this session",
        )
        .hint("the sandboxed import worker (gusset-worker) was not found next to the program"));
    }
    let file = open_regular(&full, path, MAX_IMPORT_BYTES)?;
    let file_name = full.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(ImportJob { file, kind, file_name, importer, kernel: s.kernel().clone() })
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
            Kind::Scad => scad(&bytes, self.kernel.as_ref(), cancel)?,
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
                let limits = gusset_sldprt::Limits::default();
                let sw = |e: gusset_sldprt::SldprtError| CommandError::new("io", format!("SolidWorks: {e}"));
                let xt_err = |e: gusset_xt::XtError| CommandError::new("io", format!("SolidWorks (Parasolid): {e}"));
                // The final partition; a part made by an imported feature keeps its bodies
                // in that feature's `LocalBodies` instead (one transmit each).
                // `failure`: why the final partition gave no body, reported if nothing else does.
                let (transmits, mut warnings, failure) = match gusset_sldprt::extract_xt(&bytes, &limits) {
                    Ok(part) => match gusset_xt::to_step(&part.xt, &stem, &gusset_xt::Limits::default()) {
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
                    let bodies = importer.import("step", step.as_bytes(), cancel).map_err(|e| {
                        if cancel.load(Ordering::Relaxed) { cancelled() } else { CommandError::new("io", e) }
                    })?;
                    out.extend(with_looks(bodies, looks));
                }
                if transmits.is_empty() {
                    let locals = gusset_sldprt::local_bodies(&bytes, &limits, cancel)
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
                        let c = gusset_xt::to_step(xt, &format!("{stem}_{}", i + 1), &gusset_xt::Limits::default())
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
                        let bodies = importer.import("step", c.step.as_bytes(), cancel).map_err(|e| {
                            if cancel.load(Ordering::Relaxed) { cancelled() } else { CommandError::new("io", e) }
                        })?;
                        out.extend(with_looks(bodies, &c.looks));
                    }
                }
                warnings.truncate(50);
                (out, TIER_XT, warnings)
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
            Kind::F3d => {
                let importer =
                    self.importer.ok_or_else(|| CommandError::new("not_allowed", "Fusion import is not available"))?;
                f3d(&bytes, importer.as_ref(), cancel)?
            }
            Kind::Tree => return Err(CommandError::args("a feature tree has no bodies")),
            Kind::FreeCad => {
                let importer =
                    self.importer.ok_or_else(|| CommandError::new("not_allowed", "FreeCAD import is not available"))?;
                freecad(&bytes, importer.as_ref(), cancel)?
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
                    out.push(PreparedBody { name: m.name.clone(), blob, format, color: m.color, faces: Vec::new() });
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

/// An OpenSCAD program: one exact body per top-level statement.
fn scad(
    bytes: &[u8],
    k: &dyn gusset_kernel::Kernel,
    cancel: &CancelFlag,
) -> Result<(Vec<PreparedBody>, &'static str, Vec<String>), CommandError> {
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
    for (i, c) in model.objects.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let shape = crate::scad_build::build(k, c, cancel).map_err(|e| {
            if cancel.load(Ordering::Relaxed) { cancelled() } else { CommandError::new("io", format!("OpenSCAD: {e}")) }
        })?;
        let blob = k.write_native(&shape).map_err(|e| CommandError::new("io", e.message))?;
        out.push(PreparedBody {
            name: format!("Object {}", i + 1),
            blob,
            format: ImportFormat::Scad,
            color: None,
            faces: Vec::new(),
        });
    }
    let warnings = model.warnings.into_iter().take(50).collect();
    Ok((out, TIER_SCAD, warnings))
}

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
type Use = (gusset_sat::fusion::Placement, Option<gusset_sat::fusion::Placement>);

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
        *slot = Some(vec![(Placement::IDENTITY, None)]);
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
                            placed.push((q.0.then(inst), over));
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
    let wanted = |n: &str| is_brep(n) || is_stream(n);
    let read = |data: &[u8], budget: &mut u64| {
        gusset_meshio::archive::read_entries_within(data, &limits, &wanted, MAX_F3D_STREAM_BYTES, budget).map_err(bad)
    };
    // The streams that describe the design itself: an asset named `[Active]` when there is one
    // (the others, `Animation`, hold other states of the same parts).
    let (mut streams_read, mut placements_kept) = (0usize, 0usize);
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
            info.bodies.extend(d.bodies);
            info.xrefs.extend(d.xrefs);
            info.context = info.context.or(d.context);
        }
        Ok(info)
    };
    // The designs: the file itself, or for an `.f3z` the `.f3d` designs it holds (one level
    // deep, sharing the decompression budget).
    let first = read(bytes, &mut budget)?;
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
            let entries = read(d, &mut budget)?;
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
        vec![Some(vec![(Placement::IDENTITY, None)]); designs.len()]
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
            let default = [(Placement::IDENTITY, None)];
            let insts: &[Use] = insts.as_deref().unwrap_or(&default);
            // The root design's own bodies stay where they are; those of a referenced design
            // follow the reference.
            let referenced = designs.len() > 1 && design.file != designs.get(root).map_or("", |r| r.file.as_str());
            let mut places: std::collections::HashMap<&str, Vec<&gusset_sat::fusion::Placed>> =
                std::collections::HashMap::new();
            for p in &design.info.bodies {
                places.entry(p.brep.as_str()).or_default().push(p);
            }
            for (entry, data) in design.entries.iter().filter(|(n, _)| is_brep(n)) {
                i += 1;
                let id = entry.rsplit("BREP.").next().and_then(|t| t.split('.').next()).unwrap_or_default();
                // The placements of this body inside its design, or its absence from them.
                let own: Vec<Option<&gusset_sat::fusion::Placed>> = match places.get(id) {
                    Some(v) => v.iter().copied().map(Some).collect(),
                    None => vec![None],
                };
                for p in own {
                    for &(origin, moved) in insts {
                        // Design coordinates to the root's: the reference's matrix, or the root's
                        // own placement of the design's context component taken as the design's.
                        let inst = match (moved, design.info.context) {
                            (Some(o), Some(ctx)) => o.then(&ctx.inverse()),
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
                        let c = match gusset_sat::to_step_placed(
                            data,
                            &stem,
                            &gusset_sat::Limits::default(),
                            place.as_ref(),
                            keep_sheets,
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
        return crate::tree_import::apply(s, ctx, tree, &p.file_name);
    }
    // (label, blob hash, format, colour, face colours)
    type Stored<'a> = (String, String, ImportFormat, Option<u32>, &'a [(u32, u32)]);
    let stored: Vec<Stored<'_>> = p
        .bodies
        .iter()
        .map(|b| (b.name.clone(), s.put_blob(&b.blob).to_hex(), b.format, b.color, b.faces.as_slice()))
        .collect();
    let file_name = p.file_name.clone();
    let (tier, warnings) = (p.tier, p.warnings);
    edit(s, ctx, &format!("Import {file_name}"), |doc, rb| {
        let mut names = Vec::new();
        for (i, (body_name, blob, format, color, faces)) in stored.iter().enumerate() {
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
                });
            }
            names.push(name);
        }
        let warnings: Vec<String> = warnings.iter().take(20).cloned().collect();
        Ok(json!({ "features": names, "tier": tier, "warnings_user_text": warnings }))
    })
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
