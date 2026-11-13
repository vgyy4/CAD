//! PCB commands (ADR 0047): read a KiCad board, bring it into the document as bodies, and
//! write its Gerber and drill files. The board file is untrusted: its text reaches agents
//! only in `_user_text` fields, and a 3D model path written in it is only looked up inside
//! the models folder the caller names.

use super::*;
use gusset_pcb::{Board, board3d, check, drill, gerber};
use std::sync::atomic::AtomicBool;

pub(super) fn specs() -> Vec<CommandSpec> {
    let mut v = vec![spec::<PcbInfo>(), spec::<PcbImport>(), spec::<PcbExportGerbers>(), spec::<PcbExportDrill>()];
    v.extend(super::pcb_edit::specs());
    v.extend(super::pcb_layout::specs());
    v.extend(super::pcb_nets::specs());
    v.extend(super::pcb_autoroute::specs());
    v
}

/// Largest board file read (the S-expression reader's own limit).
const MAX_BOARD_BYTES: u64 = 256 << 20;
/// Most distinct 3D model files read for one import, and their total size.
const MAX_MODELS: usize = 500;
const MAX_MODEL_BYTES: u64 = 512 << 20;

pub(crate) const TIER_PCB: &str =
    "board from the KiCad file: outline and holes exact; components as boxes or their STEP models";

/// Reads a `.kicad_pcb` or `.kicad_mod` named by the caller.
fn read_board(s: &Session, ctx: &CmdCtx, path: &str) -> Result<(Board, String), CommandError> {
    read_board_text(s, ctx, path).map(|(b, stem, _)| (b, stem))
}

/// The board a command works on: the file at `path`, or the session's open board.
pub(super) fn board_source(s: &Session, ctx: &CmdCtx, path: Option<&str>) -> Result<(Board, String), CommandError> {
    match path {
        Some(p) => read_board(s, ctx, p),
        None => {
            let open = crate::pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
            let stem = open.file.rsplit_once('.').map_or(open.file.as_str(), |x| x.0).to_string();
            Ok((open.board.clone(), stem))
        }
    }
}

/// No board is open and no file was named.
pub(super) fn no_board() -> CommandError {
    CommandError::new("not_found", "no board is open")
        .hint("open one with pcb_open (or start one with pcb_new), or give a path")
}

/// [`read_board`] with the file's text.
pub(super) fn read_board_text(s: &Session, ctx: &CmdCtx, path: &str) -> Result<(Board, String, String), CommandError> {
    crate::paths::refuse_remote(path)?;
    let full = crate::paths::read_target(s.project_dir.as_deref(), path, &ctx.actor)?;
    let ext = full.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext != "kicad_pcb" && ext != "kicad_mod" {
        return Err(CommandError::args("PCB files are .kicad_pcb (boards) or .kicad_mod (footprints)"));
    }
    let f = crate::import::open_regular(&full, path, MAX_BOARD_BYTES)?;
    let bytes = crate::import::read_capped(f, MAX_BOARD_BYTES, &AtomicBool::new(false))?;
    let text = String::from_utf8(bytes).map_err(|_| CommandError::new("io", "the board file is not UTF-8 text"))?;
    let board = Board::read(&text, &gusset_pcb::Limits::default())
        .map_err(|e| CommandError::new("io", format!("KiCad board: {e}")))?;
    let stem = full.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok((board, stem, text))
}

/// A file-name stem from the board's name: letters, digits, `-` and `_` only.
pub(super) fn clean_stem(stem: &str) -> String {
    let s: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(64)
        .collect();
    if s.trim_matches('_').is_empty() { "board".into() } else { s }
}

/// Writes generated files into `folder` with the export rules (agents: inside the project,
/// allow-listed extension; overwriting asks as for other exports).
pub(super) fn write_outputs(
    s: &Session,
    ctx: &CmdCtx,
    folder: &str,
    overwrite: bool,
    ext: &str,
    files: &[(String, String)],
) -> Result<Vec<String>, CommandError> {
    let folder = folder.trim_end_matches('/');
    let mut targets = Vec::new();
    for (name, _) in files {
        let rel = if folder.is_empty() { name.clone() } else { format!("{folder}/{name}") };
        let full = crate::paths::export_target(s.project_dir.as_deref(), &rel, &ctx.actor, &[ext])?;
        if full.exists() {
            if !overwrite {
                return Err(CommandError::new("confirmation_required", format!("'{rel}' already exists"))
                    .hint("pass overwrite: true to replace the files"));
            }
            if ctx.is_agent() && s.config.approvals_required && !ctx.approved {
                return Err(CommandError::approval("overwriting exported files"));
            }
        }
        targets.push((rel, full));
    }
    for ((rel, full), (_, text)) in targets.iter().zip(files) {
        crate::paths::write_file(full, rel, text.as_bytes())?;
    }
    Ok(targets.into_iter().map(|t| t.0).collect())
}

/// Reports a KiCad board or footprint: layers, counts, size and a sanity check.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbInfo {
    /// .kicad_pcb or .kicad_mod (default: the open board).
    #[serde(default)]
    pub path: Option<String>,
}

impl Command for PcbInfo {
    const ID: &'static str = "pcb.info";
    const TITLE: &'static str = "PCB info";
    const DESCRIPTION: &'static str =
        "Read a KiCad board (.kicad_pcb/.kicad_mod): layers, thickness, counts, outline size and sanity issues.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (b, _) = board_source(s, ctx, self.path.as_deref())?;
        let (loops, _) = gusset_pcb::geom::outline_loops(&b, check::OUTLINE_TOL);
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in loops.iter().flat_map(|l| l.polygon(0.01)) {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        let size = (hi[0] >= lo[0]).then(|| json!([round3(hi[0] - lo[0]), round3(hi[1] - lo[1])]));
        let issues: Vec<Value> =
            check::check(&b, 50).into_iter().map(|i| json!({"code": i.code, "message_user_text": i.message})).collect();
        let (plated, npth) = drill::holes(&b);
        let models = b.footprints.iter().map(|f| f.models.len()).sum::<usize>();
        Ok(json!({
            "version": b.version,
            "thickness_mm": round3(b.thickness),
            "copper_layers_user_text": b.copper_layers(),
            "size_mm": size,
            "outline_loops": loops.len(),
            "footprints": b.footprints.len(),
            "pads": b.pads.len(),
            "tracks": b.tracks.len(),
            "vias": b.vias.len(),
            "zones": b.zones.len(),
            "nets": b.nets.len(),
            "holes": {"plated": plated.len(), "non_plated": npth.len()},
            "model_references": models,
            "unrouted": gusset_pcb::ratsnest::ratsnest(&b).unrouted(),
            "issues": issues,
            "notes_user_text": b.notes,
        }))
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Brings a KiCad board into the document as bodies, for designing around it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbImport {
    /// .kicad_pcb (default: the open board).
    #[serde(default)]
    pub path: Option<String>,
    /// Cut pad holes (default true).
    #[serde(default)]
    pub holes: Option<bool>,
    /// Also cut via holes.
    #[serde(default)]
    pub vias: bool,
    /// Outer copper as thin bodies.
    #[serde(default)]
    pub copper: bool,
    /// Component bodies (default true).
    #[serde(default)]
    pub components: Option<bool>,
    /// Box height, e.g. "2 mm".
    #[serde(default)]
    pub component_height: Option<String>,
    /// Folder with the footprints' STEP models (variables in their paths are dropped).
    #[serde(default)]
    pub models_dir: Option<String>,
}

pub(super) fn length_mm(field: &str, t: &str) -> Result<f64, CommandError> {
    let bad = || CommandError::args(format!("`{field}` is a length such as \"2 mm\""));
    let e = gusset_units::expr::parse(t.trim()).map_err(|_| bad())?;
    gusset_eval::expr_value("length", &e, gusset_units::Dim::LENGTH, &Default::default(), Default::default())
        .map_err(|_| bad())
}

/// A model path from the board file as a path relative to the models folder: leading
/// `${VAR}` and absolute parts dropped, plain components only. The `.step`/`.stp` names to
/// try come first (KiCad's older libraries reference `.wrl` with a STEP file beside it).
pub(crate) fn model_candidates(raw: &str) -> Vec<String> {
    let norm = raw.replace('\\', "/");
    let parts: Vec<&str> =
        norm.split('/').filter(|p| !p.is_empty() && *p != "." && !p.starts_with('$') && !p.contains(':')).collect();
    if parts.iter().any(|p| *p == ".." || p.starts_with('.')) {
        return Vec::new();
    }
    let Some(file) = parts.last() else { return Vec::new() };
    let stem = file.rsplit_once('.').map_or(*file, |x| x.0);
    let dir = parts.get(..parts.len().saturating_sub(1)).unwrap_or_default().join("/");
    let join = |d: &str, f: &str| if d.is_empty() { f.to_string() } else { format!("{d}/{f}") };
    let mut out = Vec::new();
    for name in [format!("{stem}.step"), format!("{stem}.stp"), format!("{stem}.STEP"), file.to_string()] {
        for d in [dir.as_str(), ""] {
            let c = join(d, &name);
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

/// The shapes of one STEP model file, read once through the sandboxed importer.
type ModelCache = std::collections::HashMap<String, Option<Vec<gusset_kernel::Shape>>>;

impl PcbImport {
    fn model_shapes(
        &self,
        s: &Session,
        ctx: &CmdCtx,
        dir: &str,
        raw: &str,
        cache: &mut ModelCache,
        budget: &mut (usize, u64),
    ) -> Option<Vec<gusset_kernel::Shape>> {
        let importer = s.importer.clone()?;
        let k = s.kernel().clone();
        for rel in model_candidates(raw) {
            if !rel.to_ascii_lowercase().ends_with(".step") && !rel.to_ascii_lowercase().ends_with(".stp") {
                continue;
            }
            let shown = format!("{}/{rel}", dir.trim_end_matches('/'));
            if let Some(hit) = cache.get(&shown) {
                return hit.clone();
            }
            let Ok(full) = crate::paths::read_target(s.project_dir.as_deref(), &shown, &ctx.actor) else { continue };
            let Ok(f) = crate::import::open_regular(&full, &shown, crate::import::MAX_IMPORT_BYTES) else { continue };
            if budget.0 >= MAX_MODELS {
                return None;
            }
            budget.0 += 1;
            let cancel = AtomicBool::new(false);
            let shapes = crate::import::read_capped(f, crate::import::MAX_IMPORT_BYTES, &cancel)
                .ok()
                .filter(|b| {
                    budget.1 += b.len() as u64;
                    budget.1 <= MAX_MODEL_BYTES
                })
                .and_then(|bytes| importer.import("step", &bytes, &cancel).ok())
                .map(|bodies| bodies.iter().filter_map(|(_, blob)| k.read_native(blob).ok()).collect::<Vec<_>>())
                .filter(|v| !v.is_empty());
            cache.insert(shown, shapes.clone());
            return shapes;
        }
        None
    }
}

/// A body of a board: name, shape, colour.
pub(super) type BoardBody = (String, gusset_kernel::Shape, u32);

impl PcbImport {
    /// The board's bodies (name, shape, colour) as this import's options make them, with
    /// footprint STEP models in place of boxes where found.
    pub(super) fn bodies(
        &self,
        s: &Session,
        ctx: &CmdCtx,
        b: &Board,
    ) -> Result<(Vec<BoardBody>, Vec<String>), CommandError> {
        let height = match &self.component_height {
            Some(t) => length_mm("component_height", t)?,
            None => 2.0,
        };
        if !(height.is_finite() && height > 0.0 && height <= 1000.0) {
            return Err(CommandError::args("component_height must be above 0 and at most 1000 mm"));
        }
        let opts = board3d::Options {
            holes: self.holes.unwrap_or(true),
            vias: self.vias,
            components: self.components.unwrap_or(true),
            component_height: height,
            copper: self.copper,
        };
        let k = s.kernel().clone();
        let (bodies, mut notes) =
            board3d::build(b, k.as_ref(), &opts).map_err(|e| CommandError::new("io", format!("PCB: {e}")))?;
        let mut out = Vec::new();
        let mut cache = ModelCache::new();
        let mut budget = (0usize, 0u64);
        let (mut placed, mut boxed) = (0usize, 0usize);
        if self.models_dir.is_some() && s.importer.is_none() {
            notes.push("STEP models are not read in this session (no import worker); components are boxes".into());
        }
        for body in bodies {
            let mut shape = body.shape;
            let mut color = body.color;
            if let (Some(i), Some(dir)) = (body.footprint, self.models_dir.as_deref())
                && let Some(fp) = b.footprints.get(i)
            {
                let mut parts = Vec::new();
                for m in fp.models.iter().filter(|m| !m.hidden) {
                    let Some(shapes) = self.model_shapes(s, ctx, dir, &m.path, &mut cache, &mut budget) else {
                        continue;
                    };
                    for sh in shapes {
                        let mut cur = Some(sh);
                        for t in board3d::model_steps(b, fp, m) {
                            cur = cur.and_then(|c| k.transform(&c, &t).ok().map(|r| r.shape));
                        }
                        parts.extend(cur);
                    }
                }
                if !parts.is_empty() {
                    let joined = if parts.len() == 1 { parts.pop() } else { k.compound(&parts).ok().map(|r| r.shape) };
                    if let Some(j) = joined {
                        shape = j;
                        color = 0x80_80_80;
                        placed += 1;
                    }
                } else {
                    boxed += 1;
                }
            }
            out.push((body.name, shape, color));
        }
        if self.models_dir.is_some() {
            notes.push(format!("{placed} components from STEP models, {boxed} as boxes (no readable model found)"));
        }
        Ok((out, notes))
    }
}

impl Command for PcbImport {
    const ID: &'static str = "pcb.import";
    const TITLE: &'static str = "Import PCB";
    const DESCRIPTION: &'static str = "Bring a KiCad board in as bodies: the board with its holes, component boxes (or STEP models from models_dir), copper optional. For enclosure design.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (b, stem) = board_source(s, ctx, self.path.as_deref())?;
        let (bodies, notes) = self.bodies(s, ctx, &b)?;
        let k = s.kernel().clone();
        let mut out: Vec<(String, Vec<u8>, Option<u32>)> = Vec::new();
        for (name, shape, color) in bodies {
            let blob = k.write_native(&shape).map_err(|e| CommandError::new("io", e.message))?;
            out.push((name, blob, Some(color)));
        }
        let prepared = crate::import::PreparedImport::native(out, format!("{stem}.kicad_pcb"), TIER_PCB, notes);
        crate::import::commit_import(s, ctx, prepared)
    }
}

/// Writes a board's Gerber X2 files, one per layer.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbExportGerbers {
    /// .kicad_pcb (default: the open board).
    #[serde(default)]
    pub path: Option<String>,
    /// Existing folder for the files (<board>-<layer>.gbr).
    pub folder: String,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for PcbExportGerbers {
    const ID: &'static str = "pcb.export_gerbers";
    const TITLE: &'static str = "Export Gerbers";
    const DESCRIPTION: &'static str =
        "Write Gerber X2 files of a KiCad board: copper, mask, paste, silkscreen and outline layers.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (b, stem) = board_source(s, ctx, self.path.as_deref())?;
        let stem = clean_stem(&stem);
        let layers = gerber::write_all(&b).map_err(|e| CommandError::new("io", e.to_string()))?;
        let files: Vec<(String, String)> =
            layers.into_iter().map(|l| (format!("{stem}-{}.gbr", clean_stem(&l.suffix)), l.text)).collect();
        let written = write_outputs(s, ctx, &self.folder, self.overwrite, "gbr", &files)?;
        let issues: Vec<Value> =
            check::check(&b, 20).into_iter().map(|i| json!({"code": i.code, "message_user_text": i.message})).collect();
        Ok(json!({ "written": written, "issues": issues }))
    }
}

/// Writes a board's Excellon drill files (plated and non-plated).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbExportDrill {
    /// .kicad_pcb (default: the open board).
    #[serde(default)]
    pub path: Option<String>,
    /// Existing folder for <board>-PTH.drl and <board>-NPTH.drl.
    pub folder: String,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for PcbExportDrill {
    const ID: &'static str = "pcb.export_drill";
    const TITLE: &'static str = "Export drill files";
    const DESCRIPTION: &'static str =
        "Write Excellon drill files of a KiCad board: plated and non-plated holes, slots.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (b, stem) = board_source(s, ctx, self.path.as_deref())?;
        let stem = clean_stem(&stem);
        let (plated, npth) = drill::holes(&b);
        let files = vec![
            (format!("{stem}-PTH.drl"), drill::write(&b, &plated, true)),
            (format!("{stem}-NPTH.drl"), drill::write(&b, &npth, false)),
        ];
        let written = write_outputs(s, ctx, &self.folder, self.overwrite, "drl", &files)?;
        Ok(json!({ "written": written, "plated": plated.len(), "non_plated": npth.len() }))
    }
}
