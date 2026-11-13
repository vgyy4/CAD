//! Board layout commands (ADR 0047 phase 2b): zone fills, routing, the board as STEP and
//! the Specctra hand-off to an external autorouter. Coordinates are KiCad's: mm, Y down.

use super::pcb::{PcbImport, board_source, no_board, write_outputs};
use super::pcb_edit::commit_board;
use super::*;
use crate::pcb_session;
use gusset_pcb::{Limits, route, shove, specctra, zones};
use std::sync::atomic::AtomicBool;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        spec::<PcbFillZones>(),
        spec::<PcbRoute>(),
        spec::<PcbAddFootprint>(),
        spec::<PcbExportStep>(),
        spec::<PcbExportDsn>(),
        spec::<PcbImportSes>(),
    ]
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

/// Routes a track through points, around other nets' copper.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbRoute {
    /// Start, waypoints, end (mm).
    pub points: Vec<[f64; 2]>,
    /// Copper layer (default F.Cu).
    #[serde(default)]
    pub layer: Option<String>,
    /// Default: the net at the start.
    #[serde(default)]
    pub net: Option<String>,
    /// mm (default: the net class's).
    #[serde(default)]
    pub width: Option<f64>,
    /// false: straight between the points.
    #[serde(default)]
    pub around: Option<bool>,
    /// Round corners into arcs of this radius (mm).
    #[serde(default)]
    pub arc_radius: Option<f64>,
    /// false: never move another net's track aside.
    #[serde(default)]
    pub shove: Option<bool>,
}

impl Command for PcbRoute {
    const ID: &'static str = "pcb.route";
    const TITLE: &'static str = "Route track";
    const DESCRIPTION: &'static str =
        "Lay a track through points in 45° segments, walking around other nets (or shoving one track aside).";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if !(2..=1000).contains(&self.points.len()) {
            return Err(CommandError::args("a route has 2 to 1000 points"));
        }
        if self.points.iter().flatten().any(|v| !v.is_finite() || v.abs() > 10_000.0) {
            return Err(CommandError::args("points must be within 10 m"));
        }
        if self.arc_radius.is_some_and(|r| !(r.is_finite() && (0.0..=100.0).contains(&r))) {
            return Err(CommandError::args("arc_radius is 0 to 100 mm"));
        }
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let b = &open.board;
        let layer = self.layer.clone().unwrap_or_else(|| "F.Cu".into());
        if !b.copper_layers().contains(&layer) {
            return Err(CommandError::args("the layer is not a copper layer of this board"));
        }
        let first = self.points.first().copied().unwrap_or_default();
        let net = self.net.clone().or_else(|| b.net_at(first, &layer));
        let width = self.width.unwrap_or_else(|| open.rules.track_width_of(net.as_deref()));
        if !(width.is_finite() && width > 0.0 && width <= 100.0) {
            return Err(CommandError::args("the width is above 0 and at most 100 mm"));
        }
        let grid = (open.rules.clearance_of(net.as_deref()).min(width) / 2.0).clamp(0.05, 0.25);
        let around = self.around.unwrap_or(true);
        // The board as other nets' tracks are shoved; obstacles rebuilt after a shove.
        let mut nb = b.clone();
        let mut obs = route::Obstacles::new(&nb, &open.rules, &layer, net.as_deref(), width);
        let mut shoved = 0usize;
        let mut pts: Vec<[f64; 2]> = vec![first];
        for (i, w) in self.points.windows(2).enumerate() {
            let &[a, c] = w else { continue };
            let leg = if !around {
                vec![a, c]
            } else if let Some(leg) = route::walkaround(&obs, a, c, grid, true) {
                leg
            } else {
                let sh = if self.shove.unwrap_or(true) {
                    shove::shove(&nb, &open.rules, &layer, net.as_deref(), width, a, c, grid, &pts)
                } else {
                    None
                };
                let Some(sh) = sh else {
                    return Err(CommandError::not_found(format!("no clear way from point {i} to point {}", i + 1))
                        .hint("move the points, route on another layer, or pass around: false"));
                };
                nb.apply_shove(&sh, &layer, &Limits::default()).map_err(pcb_err)?;
                obs = route::Obstacles::new(&nb, &open.rules, &layer, net.as_deref(), width);
                shoved += 1;
                sh.path
            };
            pts.extend(leg.into_iter().skip(1));
        }
        let pts = route::simplify(&pts);
        let segs = route::with_arcs(&obs, &pts, self.arc_radius.unwrap_or(0.0));
        let length = route::length(&segs);
        let index = nb.add_route(&segs, width, &layer, net.as_deref(), &Limits::default()).map_err(pcb_err)?;
        let extra = json!({
            "tracks": nb.tracks.len() - index,
            "first_track": index,
            "length_mm": (length * 1000.0).round() / 1000.0,
            "width": width,
            "net_user_text": net,
            "shoved_tracks": shoved,
        });
        commit_board(s, ctx, "Route track", open.file.clone(), &nb, None, extra)
    }
}

/// Places a footprint from a footprint library folder.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbAddFootprint {
    /// Folder holding Lib.pretty libraries (or .kicad_mod files).
    pub library: String,
    /// Lib:Name (or Name).
    pub footprint: String,
    pub at: [f64; 2],
    /// Degrees.
    #[serde(default)]
    pub angle: Option<f64>,
    /// Default: the next free FP<n>.
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

impl Command for PcbAddFootprint {
    const ID: &'static str = "pcb.add_footprint";
    const TITLE: &'static str = "Place footprint";
    const DESCRIPTION: &'static str = "Place a footprint from a KiCad library folder (Lib.pretty) on the open board.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if self.angle.is_some_and(|a| !a.is_finite()) {
            return Err(CommandError::args("the angle must be a number of degrees"));
        }
        let text = super::sch::footprint_candidates(&self.library, &self.footprint)
            .into_iter()
            .find_map(|p| super::sch::read_text(s, ctx, &p, super::sch::MAX_FOOTPRINT_BYTES).ok())
            .ok_or_else(|| {
                CommandError::not_found(format!(
                    "no footprint '{}' in the library folder",
                    crate::paths::display_name(&self.footprint)
                ))
                .hint("name it Lib:Name for Lib.pretty/Name.kicad_mod in that folder")
            })?;
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let mut b = open.board.clone();
        let reference = match &self.reference {
            Some(r) => r.clone(),
            None => (1..100_000).map(|n| format!("FP{n}")).find(|r| b.footprint_index(r).is_none()).unwrap_or_default(),
        };
        if b.footprint_index(&reference).is_some() {
            return Err(CommandError::args(format!(
                "'{}' is already on the board",
                crate::paths::display_name(&reference)
            )));
        }
        let name = self.footprint.rsplit(':').next().unwrap_or(&self.footprint).to_string();
        let value = self.value.clone().unwrap_or(name);
        let i = b
            .add_footprint(&text, &self.footprint, &reference, &value, self.at, &Limits::default())
            .map_err(pcb_err)?;
        if let Some(a) = self.angle.filter(|a| a.rem_euclid(360.0).abs() > 1e-9) {
            b.move_footprint(i, self.at, a.rem_euclid(360.0), &Limits::default()).map_err(pcb_err)?;
        }
        let pads = b.footprints.get(i).map_or(0, |f| f.pads.len());
        let label = format!("Place {}", crate::paths::display_name(&reference));
        let extra = json!({ "footprint": i, "reference_user_text": reference, "pads": pads });
        commit_board(s, ctx, &label, open.file.clone(), &b, None, extra)
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

/// Largest session file read.
const MAX_SES_BYTES: u64 = 64 << 20;

/// Writes the open board as a Specctra DSN design for an external autorouter.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbExportDsn {
    /// Output .dsn.
    pub file: String,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for PcbExportDsn {
    const ID: &'static str = "pcb.export_dsn";
    const TITLE: &'static str = "Export DSN";
    const DESCRIPTION: &'static str =
        "Write the open board as Specctra DSN for an external autorouter; read its .ses back with pcb_import_ses.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if !self.file.to_ascii_lowercase().ends_with(".dsn") {
            return Err(CommandError::args("the file name ends in .dsn"));
        }
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let (folder, name) = self.file.rsplit_once(['/', '\\']).unwrap_or(("", &self.file));
        let text = specctra::write_dsn(&open.board, &open.rules, name);
        let written = write_outputs(s, ctx, folder, self.overwrite, "dsn", &[(name.to_string(), text)])?;
        Ok(json!({
            "written": written,
            "nets": open.board.nets.len(),
            "unrouted": open.ratsnest.unrouted(),
        }))
    }
}

/// Lays the routes of a Specctra session file on the open board.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbImportSes {
    /// .ses from the autorouter.
    pub path: String,
    /// Keep the board's tracks and vias (default: replaced).
    #[serde(default)]
    pub keep_routes: bool,
}

impl Command for PcbImportSes {
    const ID: &'static str = "pcb.import_ses";
    const TITLE: &'static str = "Import SES";
    const DESCRIPTION: &'static str =
        "Lay an autorouter's Specctra session (.ses) on the open board, replacing its tracks and vias (undoable).";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if !self.path.to_ascii_lowercase().ends_with(".ses") {
            return Err(CommandError::args("pcb.import_ses reads .ses files"));
        }
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        crate::paths::refuse_remote(&self.path)?;
        let full = crate::paths::read_target(s.project_dir.as_deref(), &self.path, &ctx.actor)?;
        let f = crate::import::open_regular(&full, &self.path, MAX_SES_BYTES)?;
        let bytes = crate::import::read_capped(f, MAX_SES_BYTES, &AtomicBool::new(false))?;
        let text =
            String::from_utf8(bytes).map_err(|_| CommandError::new("io", "the session file is not UTF-8 text"))?;
        let ses = specctra::read_ses(&text, &specctra::Limits::default())
            .map_err(|e| CommandError::new("io", format!("Specctra session: {e}")))?;
        let unknown: Vec<String> = ses
            .nets
            .iter()
            .filter(|n| !open.board.nets.iter().any(|b| b.name == n.name))
            .map(|n| n.name.chars().take(80).collect())
            .take(20)
            .collect();
        let mut b = open.board.clone();
        let (tracks, vias) =
            b.apply_session(&ses, &open.rules, !self.keep_routes, &Limits::default()).map_err(pcb_err)?;
        let after = gusset_pcb::ratsnest::ratsnest(&b).unrouted();
        let extra = json!({
            "tracks": tracks,
            "vias": vias,
            "unrouted_after": after,
            "unknown_nets_user_text": unknown,
        });
        commit_board(s, ctx, "Import routes", open.file.clone(), &b, None, extra)
    }
}
