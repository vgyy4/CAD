//! Board layout commands (ADR 0047 phase 2b): zone fills, routing, the board as STEP and
//! the Specctra hand-off to an external autorouter. Coordinates are KiCad's: mm, Y down.

use super::pcb::{PcbImport, board_source, no_board, write_outputs};
use super::pcb_edit::commit_board;
use super::*;
use crate::pcb_session;
use gusset_pcb::{Limits, zones};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<PcbFillZones>(), spec::<PcbExportStep>()]
}

fn pcb_err(e: gusset_pcb::board::Error) -> CommandError {
    match e {
        gusset_pcb::board::Error::Edit(m) => CommandError::args(m),
        e => CommandError::new("io", format!("KiCad board: {e}")),
    }
}

/// Fills the open board's copper zones.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbFillZones {
    /// Zone indices (default: all).
    #[serde(default)]
    pub zones: Vec<usize>,
}

impl Command for PcbFillZones {
    const ID: &'static str = "pcb.fill_zones";
    const TITLE: &'static str = "Fill zones";
    const DESCRIPTION: &'static str =
        "Pour the open board's copper zones: clearances, thermal reliefs, minimum width, islands removed.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        if let Some(z) = self.zones.iter().find(|&&z| z >= open.board.zones.len()) {
            return Err(CommandError::not_found(format!("there is no zone {z}"))
                .hint(format!("the board has {} zones", open.board.zones.len())));
        }
        let only = (!self.zones.is_empty()).then_some(self.zones.as_slice());
        let filled = zones::fill(&open.board, &open.rules, only);
        if filled.fills.is_empty() {
            return Err(CommandError::new("not_found", "the board has no copper zones to fill"));
        }
        let mut b = open.board.clone();
        let polygons = b.set_zone_fills(&filled.fills, &Limits::default()).map_err(pcb_err)?;
        let mut per_zone: std::collections::BTreeMap<usize, f64> = std::collections::BTreeMap::new();
        for f in &filled.fills {
            *per_zone.entry(f.zone).or_default() += zones::fill_area(f);
        }
        let areas: Vec<Value> =
            per_zone.iter().map(|(z, a)| json!({ "zone": z, "area_mm2": (a * 100.0).round() / 100.0 })).collect();
        let extra = json!({ "zones": areas, "polygons": polygons, "notes_user_text": filled.notes });
        commit_board(s, ctx, "Fill zones", open.file.clone(), &b, None, extra)
    }
}

/// Writes the board and its components as a STEP file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbExportStep {
    /// Output .step.
    pub file: String,
    /// .kicad_pcb (default: the open board).
    #[serde(default)]
    pub path: Option<String>,
    /// Outer copper as thin bodies.
    #[serde(default)]
    pub copper: bool,
    /// Folder with the footprints' STEP models.
    #[serde(default)]
    pub models_dir: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for PcbExportStep {
    const ID: &'static str = "pcb.export_step";
    const TITLE: &'static str = "Export PCB STEP";
    const DESCRIPTION: &'static str =
        "Write the board (outline, holes) and its components (STEP models from models_dir, else boxes) as STEP.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let lower = self.file.to_ascii_lowercase();
        if !(lower.ends_with(".step") || lower.ends_with(".stp")) {
            return Err(CommandError::args("the file name ends in .step or .stp"));
        }
        let (b, _) = board_source(s, ctx, self.path.as_deref())?;
        let imp = PcbImport {
            path: None,
            holes: None,
            vias: false,
            copper: self.copper,
            components: None,
            component_height: None,
            models_dir: self.models_dir.clone(),
        };
        let (bodies, notes) = imp.bodies(s, ctx, &b)?;
        let names: Vec<String> = bodies.iter().map(|(n, _, c)| format!("{n}\u{1f}#{:06x}", c & 0xff_ffff)).collect();
        let list: Vec<(&gusset_kernel::Shape, &str)> =
            bodies.iter().zip(&names).map(|((_, sh, _), n)| (sh, n.as_str())).collect();
        // Check the target before the (slow) kernel write.
        let (folder, name) = self.file.rsplit_once(['/', '\\']).unwrap_or(("", &self.file));
        let ext = if lower.ends_with(".stp") { "stp" } else { "step" };
        let full = crate::paths::export_target(s.project_dir.as_deref(), &self.file, &ctx.actor, &[ext])?;
        if full.exists() && !self.overwrite {
            return Err(CommandError::new("confirmation_required", format!("'{}' already exists", self.file))
                .hint("pass overwrite: true to replace it"));
        }
        let bytes = s.kernel().export_step(&list).map_err(|e| CommandError::new("io", e.to_string()))?;
        let text = String::from_utf8(bytes).map_err(|_| CommandError::new("io", "the STEP writer gave binary data"))?;
        let written = write_outputs(s, ctx, folder, self.overwrite, ext, &[(name.to_string(), text)])?;
        Ok(json!({ "written": written, "bodies": list.len(), "notes_user_text": notes }))
    }
}
