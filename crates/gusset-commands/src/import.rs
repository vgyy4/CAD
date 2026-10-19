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
}

/// Tier note for FreeCAD imports.
pub const TIER_FREECAD: &str = "exact geometry per visible FreeCAD object (B-rep, no FreeCAD history)";

/// An opened import file, ready to be read and parsed off the model thread.
pub struct ImportJob {
    file: std::fs::File,
    kind: Kind,
    file_name: String,
    importer: Option<Arc<dyn Importer>>,
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
}

/// The parsed result of [`ImportJob::run`], for [`complete_import`].
#[derive(Debug, Clone)]
pub struct PreparedImport {
    bodies: Vec<PreparedBody>,
    file_name: String,
    tier: &'static str,
    warnings: Vec<String>,
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
        other => match gusset_meshio::Format::from_extension(other) {
            Some(f) => Kind::Mesh(f),
            None => {
                return Err(CommandError::args(format!("'.{other}' files cannot be imported"))
                    .hint("supported: .step, .stp, .iges, .igs, .fcstd, .stl, .3mf, .obj"));
            }
        },
    };
    let importer = s.importer.clone();
    if matches!(kind, Kind::Brep(_) | Kind::FreeCad) && importer.is_none() {
        return Err(CommandError::new("not_allowed", "STEP/IGES/FreeCAD import is not available in this session")
            .hint("the sandboxed import worker (gusset-worker) was not found next to the program"));
    }
    let file = open_regular(&full, path, MAX_IMPORT_BYTES)?;
    let file_name = full.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(ImportJob { file, kind, file_name, importer })
}

impl ImportJob {
    /// Phase 2: reads and parses the file. Call it off the model thread; it may take
    /// long (the worker's timeout bounds STEP/IGES). Setting `cancel` stops it.
    ///
    /// # Errors
    /// `cancelled`, `io` (parse or worker failures), `invalid_args` (too large).
    pub fn run(self, cancel: &CancelFlag) -> Result<PreparedImport, CommandError> {
        let bytes = read_capped(self.file, MAX_IMPORT_BYTES, cancel)?;
        let (bodies, tier, warnings) = match self.kind {
            Kind::Brep(format) => {
                let importer = self
                    .importer
                    .ok_or_else(|| CommandError::new("not_allowed", "STEP/IGES import is not available"))?;
                let bodies = importer.import(format.keyword(), &bytes, cancel).map_err(|e| {
                    if cancel.load(Ordering::Relaxed) { cancelled() } else { CommandError::new("io", e) }
                })?;
                let bodies = bodies.into_iter().map(|(name, blob)| PreparedBody { name, blob, format });
                (bodies.collect::<Vec<_>>(), TIER_BREP, Vec::new())
            }
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
                    // the same limits.
                    let body = gusset_meshio::ExportBody { name: &m.name, mesh: &m.mesh };
                    let (blob, _) = gusset_meshio::export_to_vec(&[body], gusset_meshio::ExportFormat::StlBinary)
                        .map_err(|e| CommandError::new("io", format!("storing the mesh failed: {e}")))?;
                    out.push(PreparedBody { name: m.name.clone(), blob, format: ImportFormat::Stl });
                }
                (out, TIER_MESH, imported.report.warnings)
            }
        };
        if bodies.is_empty() {
            return Err(CommandError::new("io", "the file contains no bodies"));
        }
        Ok(PreparedImport { bodies, file_name: self.file_name, tier, warnings })
    }
}

/// The visible shapes of a FreeCAD document, each parsed by the sandboxed worker. A shape
/// the worker refuses is reported as a warning; none at all is an error.
fn freecad(
    bytes: &[u8],
    importer: &dyn Importer,
    cancel: &CancelFlag,
) -> Result<(Vec<PreparedBody>, &'static str, Vec<String>), CommandError> {
    let limits = gusset_meshio::Limits::default();
    let bad = |e: gusset_meshio::MeshIoError| CommandError::new("io", format!("the FreeCAD file cannot be read: {e}"));
    let xml = gusset_meshio::archive::read_entries(bytes, &limits, &|n| n == "Document.xml" || n == "GuiDocument.xml")
        .map_err(bad)?;
    let get = |n: &str| xml.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_slice());
    let doc = get("Document.xml")
        .ok_or_else(|| CommandError::new("io", "the file has no Document.xml: not a FreeCAD document"))?;
    let picks = crate::fcstd::picks(doc, get("GuiDocument.xml")).map_err(|e| CommandError::new("io", e))?;
    if picks.is_empty() {
        return Err(CommandError::new("io", "the FreeCAD document shows no solid objects"));
    }
    let files: std::collections::BTreeSet<&str> = picks.iter().map(|p| p.file.as_str()).collect();
    let shapes = gusset_meshio::archive::read_entries(bytes, &limits, &|n| files.contains(n)).map_err(bad)?;
    let (mut out, mut warnings) = (Vec::new(), Vec::new());
    for p in &picks {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let Some((_, data)) = shapes.iter().find(|(n, _)| *n == p.file) else {
            warnings.push(format!("{}: its shape file is missing", p.label));
            continue;
        };
        // FreeCAD writes BRep text (.brp), or OCCT binary (.bin) when asked to.
        let format = if p.file.to_ascii_lowercase().ends_with(".bin") { "brep" } else { "brp" };
        match importer.import(format, data, cancel) {
            Ok(bodies) => out.extend(bodies.into_iter().map(|(_, blob)| PreparedBody {
                name: p.label.clone(),
                blob,
                format: ImportFormat::FreeCad,
            })),
            Err(_) if cancel.load(Ordering::Relaxed) => return Err(cancelled()),
            Err(e) => warnings.push(format!("{}: {e}", p.label)),
        }
    }
    if out.is_empty() {
        return Err(CommandError::new("io", format!("no FreeCAD shape could be read: {}", warnings.join("; "))));
    }
    Ok((out, TIER_FREECAD, warnings))
}

/// Phase 3 without policy checks: stores the blobs and commits the import features.
pub(crate) fn commit_import(s: &mut Session, ctx: &CmdCtx, p: PreparedImport) -> Result<Value, CommandError> {
    let stored: Vec<(String, String, ImportFormat)> =
        p.bodies.iter().map(|b| (b.name.clone(), s.put_blob(&b.blob).to_hex(), b.format)).collect();
    let file_name = p.file_name.clone();
    let (tier, warnings) = (p.tier, p.warnings);
    edit(s, ctx, &format!("Import {file_name}"), |doc, rb| {
        let mut names = Vec::new();
        for (i, (body_name, blob, format)) in stored.iter().enumerate() {
            let name = doc.unique_feature_name("Import");
            let mut f = Feature::new(
                name.clone(),
                FeatureKind::Import(ImportDef { blob: blob.clone(), format: *format, file_name: file_name.clone() }),
            );
            f.label = Some(body_name.chars().take(200).collect());
            insert_feature(doc, rb.map(|r| r + i), f);
            names.push(name);
        }
        let warnings: Vec<String> = warnings.iter().take(20).cloned().collect();
        Ok(json!({ "features": names, "tier": tier, "warnings_user_text": warnings }))
    })
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
