//! PCB editing commands (ADR 0047 phase 1b): open or start a board in the document, edit
//! it (footprints, tracks, vias, outline) with undo, save it, check it and write the
//! assembly files. Coordinates are KiCad's: mm, Y down. Text from the board file (net
//! names, references) reaches agents only in `_user_text` fields.

use super::pcb::{board_source, length_mm, no_board, read_board_text, write_outputs};
use super::*;
use crate::pcb_session::{self, OpenBoard};
use gusset_eval::BlobSource;
use gusset_pcb::text_edit::{NewText, TextPatch};
use gusset_pcb::{Board, Limits, drc, fab};
use std::sync::atomic::AtomicBool;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        spec::<PcbOpen>(),
        spec::<PcbNew>(),
        spec::<PcbSave>(),
        spec::<PcbMoveFootprint>(),
        spec::<PcbAddTrack>(),
        spec::<PcbAddVia>(),
        spec::<PcbDelete>(),
        spec::<PcbMove>(),
        spec::<PcbSetOutline>(),
        spec::<PcbText>(),
        spec::<PcbDrc>(),
        spec::<PcbExportPos>(),
        spec::<PcbExportBom>(),
    ]
}

/// Largest KiCad project file read beside a board (net classes, rules).
const MAX_PROJECT_BYTES: u64 = 8 << 20;
/// Most violations a DRC reports.
const MAX_VIOLATIONS: usize = 200;

fn pcb_err(e: gusset_pcb::board::Error) -> CommandError {
    match e {
        gusset_pcb::board::Error::Edit(m) => CommandError::args(m),
        e => CommandError::new("io", format!("KiCad board: {e}")),
    }
}

/// A shown file name: the last part of a path, cut to 255 characters.
fn file_name(path: &str) -> String {
    let n = path.rsplit(['/', '\\']).next().unwrap_or(path);
    n.chars().filter(|c| !c.is_control()).take(255).collect()
}

/// Stores a board (and its project file) and commits a document that names it. With the
/// session's fill-on-edit option on, the zones the edit touched are poured again first,
/// so the edit and its fills are one undo step (ADR 0053).
pub(super) fn commit_board(
    s: &mut Session,
    ctx: &CmdCtx,
    label: &str,
    file: String,
    board: &Board,
    project: Option<Option<String>>,
    mut extra: Value,
) -> Result<Value, CommandError> {
    let mut refilled = None;
    if s.pcb_fill_on_edit
        && project.is_none()
        && let Some(open) = pcb_session::open_board(s, &ctx.actor)?
    {
        let mut nb = board.clone();
        let (zones, filled) =
            gusset_pcb::refill::refill_after_edit(&open.board, &mut nb, &open.rules, &limits()).map_err(pcb_err)?;
        if !zones.is_empty() {
            if let Value::Object(m) = &mut extra {
                m.insert("refilled_zones".into(), json!(zones));
                m.insert("starved_thermal".into(), json!(super::pcb_layout::starved_json(&nb, &filled)));
            }
            refilled = Some(nb);
        }
    }
    let board = refilled.as_ref().unwrap_or(board);
    let hash = s.put_blob(board.write().as_bytes()).to_hex();
    let mut out = edit(s, ctx, label, |doc, _| {
        let project = match project {
            Some(p) => p,
            None => doc.pcb.as_ref().and_then(|p| p.project.clone()),
        };
        doc.pcb = Some(gusset_doc::PcbBoard { file, board: hash, project });
        Ok(extra)
    })?;
    if let Some(open) = pcb_session::open_board(s, &ctx.actor)?
        && let Value::Object(m) = &mut out
    {
        m.insert("unrouted".into(), json!(open.ratsnest.unrouted()));
    }
    Ok(out)
}

/// The open board, edited by `f` and committed.
fn board_edit(
    s: &mut Session,
    ctx: &CmdCtx,
    label: &str,
    f: impl FnOnce(&mut Board, &OpenBoard) -> Result<Value, CommandError>,
) -> Result<Value, CommandError> {
    let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
    let mut b = open.board.clone();
    let extra = f(&mut b, &open)?;
    commit_board(s, ctx, label, open.file.clone(), &b, None, extra)
}

fn limits() -> Limits {
    Limits::default()
}

/// Opens a KiCad board in the document for editing.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbOpen {
    /// .kicad_pcb (its .kicad_pro beside it gives net classes).
    pub path: String,
}

impl Command for PcbOpen {
    const ID: &'static str = "pcb.open";
    const TITLE: &'static str = "Open PCB";
    const DESCRIPTION: &'static str = "Open a KiCad board for editing (undoable; replaces an open one).";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if !self.path.to_ascii_lowercase().ends_with(".kicad_pcb") {
            return Err(CommandError::args("pcb.open reads .kicad_pcb boards"));
        }
        let (b, _, text) = read_board_text(s, ctx, &self.path)?;
        // The project file beside it, when there is one and it reads as JSON.
        let pro_path = format!("{}.kicad_pro", self.path.get(..self.path.len() - ".kicad_pcb".len()).unwrap_or(""));
        let project = crate::paths::read_target(s.project_dir.as_deref(), &pro_path, &ctx.actor)
            .ok()
            .filter(|p| p.is_file())
            .and_then(|full| crate::import::open_regular(&full, &pro_path, MAX_PROJECT_BYTES).ok())
            .and_then(|f| crate::import::read_capped(f, MAX_PROJECT_BYTES, &AtomicBool::new(false)).ok())
            .filter(|bytes| serde_json::from_slice::<Value>(bytes).is_ok_and(|v| v.is_object()));
        let project = project.map(|bytes| s.put_blob(&bytes).to_hex());
        let hash = s.put_blob(text.as_bytes()).to_hex();
        let file = file_name(&self.path);
        let label = format!("Open board {}", crate::paths::display_name(&file));
        let with_project = project.is_some();
        edit(s, ctx, &label, |doc, _| {
            doc.pcb = Some(gusset_doc::PcbBoard { file, board: hash, project });
            Ok(json!({
                "footprints": b.footprints.len(),
                "tracks": b.tracks.len(),
                "vias": b.vias.len(),
                "copper_layers_user_text": b.copper_layers(),
                "project_rules": with_project,
                "unrouted": gusset_pcb::ratsnest::ratsnest(&b).unrouted(),
            }))
        })
    }
}

/// Starts an empty two-layer board.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbNew {
    /// Outline width, e.g. "50 mm".
    pub width: String,
    pub height: String,
    /// Default board.kicad_pcb.
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for PcbNew {
    const ID: &'static str = "pcb.new";
    const TITLE: &'static str = "New PCB";
    const DESCRIPTION: &'static str = "Start an empty two-layer board (1.6 mm FR4).";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (w, h) = (length_mm("width", &self.width)?, length_mm("height", &self.height)?);
        let b = Board::new_two_layer(w, h).map_err(pcb_err)?;
        let mut file = file_name(self.name.as_deref().unwrap_or("board.kicad_pcb"));
        if !file.to_ascii_lowercase().ends_with(".kicad_pcb") {
            file.push_str(".kicad_pcb");
        }
        let extra = json!({ "size_mm": [w, h] });
        commit_board(s, ctx, "New board", file, &b, Some(None), extra)
    }
}

/// Writes the open board as a KiCad file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbSave {
    /// .kicad_pcb (default: its name).
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for PcbSave {
    const ID: &'static str = "pcb.save";
    const TITLE: &'static str = "Save PCB";
    const DESCRIPTION: &'static str = "Write the open board as a .kicad_pcb file; parts not edited stay as read.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let path = self.path.clone().unwrap_or_else(|| open.file.clone());
        if !path.to_ascii_lowercase().ends_with(".kicad_pcb") {
            return Err(CommandError::args("the board file name ends in .kicad_pcb"));
        }
        let (folder, name) = path.rsplit_once(['/', '\\']).unwrap_or(("", &path));
        let files = [(name.to_string(), open.board.write())];
        let mut written = write_outputs(s, ctx, folder, self.overwrite, "kicad_pcb", &files)?;
        // The project file (net classes, rules) beside it, when the board has one.
        if let Some(pro) = open.project_hash.as_ref().and_then(|h| s.blobs().blob(h)) {
            let stem = name.get(..name.len() - ".kicad_pcb".len()).unwrap_or(name);
            let text = String::from_utf8_lossy(&pro).into_owned();
            let files = [(format!("{stem}.kicad_pro"), text)];
            written.extend(write_outputs(s, ctx, folder, self.overwrite, "kicad_pro", &files)?);
        }
        Ok(json!({ "written": written }))
    }
}

/// Moves, turns or flips a footprint of the open board.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbMoveFootprint {
    /// Reference (R1) or #index.
    pub footprint: String,
    /// mm.
    #[serde(default)]
    pub at: Option<[f64; 2]>,
    /// Degrees.
    #[serde(default)]
    pub angle: Option<f64>,
    /// To the other side.
    #[serde(default)]
    pub flip: bool,
    /// Track ends on its pads follow (default true).
    #[serde(default)]
    pub carry: Option<bool>,
}

impl Command for PcbMoveFootprint {
    const ID: &'static str = "pcb.move_footprint";
    const TITLE: &'static str = "Move footprint";
    const DESCRIPTION: &'static str =
        "Move, rotate or flip a footprint of the open board; track ends on its pads follow.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if self.at.is_none() && self.angle.is_none() && !self.flip {
            return Err(CommandError::args("give at, angle or flip: true"));
        }
        if self.angle.is_some_and(|a| !a.is_finite()) {
            return Err(CommandError::args("the angle must be a number of degrees"));
        }
        let label = format!("Move {}", crate::paths::display_name(&self.footprint));
        board_edit(s, ctx, &label, |b, _| {
            let i = b.footprint_index(&self.footprint).ok_or_else(|| {
                CommandError::not_found(format!("no footprint '{}'", crate::paths::display_name(&self.footprint)))
                    .hint("name it by reference (R1) or #index")
            })?;
            let fp = b.footprints.get(i).ok_or_else(no_board)?;
            let (at, angle) = (self.at.unwrap_or(fp.at), self.angle.map_or(fp.angle, |a| a.rem_euclid(360.0)));
            if at.iter().any(|v| !v.is_finite() || v.abs() > 10_000.0) {
                return Err(CommandError::args("the place must be within 10 m"));
            }
            let old = b.footprint_pads(i);
            let turned = angle - fp.angle;
            b.move_footprint(i, at, angle, &limits()).map_err(pcb_err)?;
            if self.flip {
                b.flip_footprint(i, &limits()).map_err(pcb_err)?;
            }
            // A flip mirrors the pads: ends then move by the pad's displacement only.
            let carried = if self.carry.unwrap_or(true) {
                b.carry_track_ends(i, &old, if self.flip { 0.0 } else { turned }, &limits()).map_err(pcb_err)?
            } else {
                0
            };
            let fp = b.footprints.get(i).ok_or_else(no_board)?;
            let side = if fp.bottom { "bottom" } else { "top" };
            Ok(json!({ "at": fp.at, "angle": fp.angle, "side": side, "ends_carried": carried }))
        })
    }
}

/// Adds a straight track segment.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbAddTrack {
    pub start: [f64; 2],
    pub end: [f64; 2],
    /// Copper layer (default F.Cu).
    #[serde(default)]
    pub layer: Option<String>,
    /// mm (default: the net class's).
    #[serde(default)]
    pub width: Option<f64>,
    /// Default: the net at either end.
    #[serde(default)]
    pub net: Option<String>,
}

impl Command for PcbAddTrack {
    const ID: &'static str = "pcb.add_track";
    const TITLE: &'static str = "Add track";
    const DESCRIPTION: &'static str = "Add a track segment on a copper layer of the open board.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        board_edit(s, ctx, "Add track", |b, open| {
            let layer = self.layer.clone().unwrap_or_else(|| "F.Cu".into());
            let net = self.net.clone().or_else(|| b.net_at(self.start, &layer)).or_else(|| b.net_at(self.end, &layer));
            let width = self.width.unwrap_or_else(|| open.rules.track_width_of(net.as_deref()));
            let i = b.add_track(self.start, self.end, width, &layer, net.as_deref(), &limits()).map_err(pcb_err)?;
            Ok(json!({ "track": i, "width": width, "net_user_text": net }))
        })
    }
}

/// Adds a through via.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbAddVia {
    pub at: [f64; 2],
    /// Default: the net there.
    #[serde(default)]
    pub net: Option<String>,
    /// mm (default: the net class's).
    #[serde(default)]
    pub size: Option<f64>,
    #[serde(default)]
    pub drill: Option<f64>,
}

impl Command for PcbAddVia {
    const ID: &'static str = "pcb.add_via";
    const TITLE: &'static str = "Add via";
    const DESCRIPTION: &'static str = "Add a through via to the open board.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        board_edit(s, ctx, "Add via", |b, open| {
            let net = self.net.clone().or_else(|| b.copper_layers().iter().find_map(|l| b.net_at(self.at, l)));
            let (size, drill) = open.rules.via_of(net.as_deref());
            let (size, drill) = (self.size.unwrap_or(size), self.drill.unwrap_or(drill));
            let i = b.add_via(self.at, size, drill, net.as_deref(), &limits()).map_err(pcb_err)?;
            Ok(json!({ "via": i, "size": size, "drill": drill, "net_user_text": net }))
        })
    }
}

/// Deletes tracks and vias.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbDelete {
    /// Track indices.
    #[serde(default)]
    pub tracks: Vec<usize>,
    #[serde(default)]
    pub vias: Vec<usize>,
    #[serde(default)]
    pub zones: Vec<usize>,
}

impl Command for PcbDelete {
    const ID: &'static str = "pcb.delete";
    const TITLE: &'static str = "Delete tracks";
    const DESCRIPTION: &'static str = "Delete tracks, vias and zones of the open board by index.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if self.tracks.is_empty() && self.vias.is_empty() && self.zones.is_empty() {
            return Err(CommandError::args("name the tracks, vias or zones to delete"));
        }
        if self.tracks.len() + self.vias.len() + self.zones.len() > 100_000 {
            return Err(CommandError::args("at most 100000 items at once"));
        }
        board_edit(s, ctx, "Delete tracks", |b, _| {
            // Zones first: their indices are checked before anything moves.
            let z = if self.zones.is_empty() { 0 } else { b.delete_zones(&self.zones, &limits()).map_err(pcb_err)? };
            let n = if self.tracks.is_empty() && self.vias.is_empty() {
                0
            } else {
                b.delete_items(&self.tracks, &self.vias, &limits()).map_err(pcb_err)?
            };
            Ok(json!({ "deleted": n + z }))
        })
    }
}

/// Moves tracks and vias.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbMove {
    /// Track indices.
    #[serde(default)]
    pub tracks: Vec<usize>,
    #[serde(default)]
    pub vias: Vec<usize>,
    /// Offset, mm.
    pub by: [f64; 2],
    /// Track ends on a moved via follow it (default true).
    #[serde(default)]
    pub attached: Option<bool>,
}

impl Command for PcbMove {
    const ID: &'static str = "pcb.move";
    const TITLE: &'static str = "Move tracks and vias";
    const DESCRIPTION: &'static str =
        "Move tracks and vias of the open board by an offset; ends on a moved via follow.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if self.tracks.len() + self.vias.len() > 100_000 {
            return Err(CommandError::args("at most 100000 items at once"));
        }
        board_edit(s, ctx, "Move tracks", |b, _| {
            let (tracks, carried) = b
                .move_items(&self.tracks, &self.vias, self.by, self.attached.unwrap_or(true), &limits())
                .map_err(|e| match e {
                    gusset_pcb::board::Error::Edit(m) if m.contains("there is no") => CommandError::not_found(m),
                    e => pcb_err(e),
                })?;
            Ok(json!({ "tracks": tracks, "vias": self.vias.len(), "ends_carried": carried }))
        })
    }
}

/// Replaces the board outline.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbSetOutline {
    /// Corners, mm (4 axis-aligned: a rectangle).
    pub points: Vec<[f64; 2]>,
}

impl Command for PcbSetOutline {
    const ID: &'static str = "pcb.set_outline";
    const TITLE: &'static str = "Board outline";
    const DESCRIPTION: &'static str = "Replace the open board's outline (Edge.Cuts) with a polygon.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        board_edit(s, ctx, "Board outline", |b, _| {
            b.set_outline(&self.points, &limits()).map_err(pcb_err)?;
            Ok(json!({ "points": self.points.len() }))
        })
    }
}

/// Adds, changes or deletes a board text.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbText {
    /// Text to change (see pcb_info); none adds.
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub delete: bool,
    /// Words; \n breaks lines.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub at: Option<[f64; 2]>,
    /// Default F.SilkS.
    #[serde(default)]
    pub layer: Option<String>,
    /// Height, mm (default 1).
    #[serde(default)]
    pub size: Option<f64>,
    #[serde(default)]
    pub thickness: Option<f64>,
    /// Degrees.
    #[serde(default)]
    pub angle: Option<f64>,
    /// Default: on B.* layers.
    #[serde(default)]
    pub mirrored: Option<bool>,
}

impl Command for PcbText {
    const ID: &'static str = "pcb.text";
    const TITLE: &'static str = "Board text";
    const DESCRIPTION: &'static str = "Add a text to the open board, or change or delete one by index.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if self.delete {
            let i = self.index.ok_or_else(|| CommandError::args("name the text to delete with index"))?;
            return board_edit(s, ctx, "Delete text", |b, _| {
                let n = b.delete_texts(&[i], &limits()).map_err(pcb_err)?;
                Ok(json!({ "deleted": n }))
            });
        }
        if let Some(i) = self.index {
            let patch = TextPatch {
                text: self.text,
                at: self.at,
                layer: self.layer,
                size: self.size,
                thickness: self.thickness,
                angle: self.angle,
                mirrored: self.mirrored,
                justify: None,
            };
            if patch == TextPatch::default() {
                return Err(CommandError::args("give what to change"));
            }
            return board_edit(s, ctx, "Edit text", |b, _| {
                b.edit_text(i, &patch, &limits()).map_err(pcb_err)?;
                let t = b.texts.get(i).ok_or_else(no_board)?;
                Ok(json!({ "text": i, "at": t.at, "layer": t.layer, "mirrored": t.mirrored }))
            });
        }
        let (Some(text), Some(at)) = (self.text, self.at) else {
            return Err(CommandError::args("to add a text give text and at"));
        };
        let new = NewText {
            text,
            at,
            layer: self.layer,
            size: self.size,
            thickness: self.thickness,
            angle: self.angle,
            mirrored: self.mirrored,
            justify: None,
        };
        board_edit(s, ctx, "Add text", |b, _| {
            let i = b.add_text(&new, &limits()).map_err(pcb_err)?;
            let t = b.texts.get(i).ok_or_else(no_board)?;
            Ok(json!({ "text": i, "layer": t.layer, "mirrored": t.mirrored }))
        })
    }
}

/// Checks the open board against its design rules.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbDrc {
    /// Override, mm.
    #[serde(default)]
    pub clearance: Option<f64>,
    #[serde(default)]
    pub min_width: Option<f64>,
    #[serde(default)]
    pub min_ring: Option<f64>,
    #[serde(default)]
    pub hole_to_edge: Option<f64>,
}

impl Command for PcbDrc {
    const ID: &'static str = "pcb.drc";
    const TITLE: &'static str = "Design rule check";
    const DESCRIPTION: &'static str =
        "Check the open board: clearances, track width, annular rings, holes, copper near the edge.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let mut rules = open.rules.clone();
        for (v, slot) in [
            (self.clearance, &mut rules.clearance),
            (self.min_width, &mut rules.min_track_width),
            (self.min_ring, &mut rules.min_annular_ring),
            (self.hole_to_edge, &mut rules.hole_to_edge),
        ] {
            if let Some(v) = v {
                if !(v.is_finite() && (0.0..=10.0).contains(&v)) {
                    return Err(CommandError::args("rule values are 0 to 10 mm"));
                }
                *slot = v;
            }
        }
        let found = drc::check(&open.board, &rules, MAX_VIOLATIONS + 1);
        let items: Vec<Value> = found
            .iter()
            .take(MAX_VIOLATIONS)
            .map(|v| {
                json!({
                    "code": v.code,
                    "message_user_text": v.message,
                    "at": [round4(v.at[0]), round4(v.at[1])],
                    "layer_user_text": v.layer,
                })
            })
            .collect();
        Ok(json!({
            "violations": items,
            "more": found.len() > MAX_VIOLATIONS,
            "unrouted": open.ratsnest.unrouted(),
            "rules_mm": {
                "clearance": rules.clearance,
                "min_width": rules.min_track_width,
                "min_ring": rules.min_annular_ring,
                "hole_to_edge": rules.hole_to_edge,
                "copper_to_edge": rules.edge_clearance,
                "hole_to_hole": rules.hole_to_hole,
                "min_through_hole": rules.min_through_hole,
                "min_via_drill": rules.min_via_drill,
                "min_via_diameter": rules.min_via_diameter,
                "silk_clearance": rules.silk_clearance,
                "net_classes": rules.classes.len(),
            },
        }))
    }
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// Writes the pick-and-place file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbExportPos {
    /// Output .csv.
    pub file: String,
    /// .kicad_pcb (default: the open board).
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for PcbExportPos {
    const ID: &'static str = "pcb.export_pos";
    const TITLE: &'static str = "Export placement";
    const DESCRIPTION: &'static str =
        "Write the pick-and-place CSV: reference, value, footprint, x, y (Y up), rotation, side.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (b, _) = board_source(s, ctx, self.path.as_deref())?;
        let (text, n) = fab::positions(&b);
        csv_out(s, ctx, &self.file, self.overwrite, text, json!({ "components": n }))
    }
}

/// Writes the bill of materials.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbExportBom {
    /// Output .csv.
    pub file: String,
    /// .kicad_pcb (default: the open board).
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for PcbExportBom {
    const ID: &'static str = "pcb.export_bom";
    const TITLE: &'static str = "Export PCB BOM";
    const DESCRIPTION: &'static str = "Write the board's bill of materials as CSV, one line per value and footprint.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (b, _) = board_source(s, ctx, self.path.as_deref())?;
        let (text, n) = fab::bom(&b);
        csv_out(s, ctx, &self.file, self.overwrite, text, json!({ "lines": n }))
    }
}

fn csv_out(
    s: &Session,
    ctx: &CmdCtx,
    file: &str,
    overwrite: bool,
    text: String,
    mut extra: Value,
) -> Result<Value, CommandError> {
    if !file.to_ascii_lowercase().ends_with(".csv") {
        return Err(CommandError::args("the file name ends in .csv"));
    }
    let (folder, name) = file.rsplit_once(['/', '\\']).unwrap_or(("", file));
    let written = write_outputs(s, ctx, folder, overwrite, "csv", &[(name.to_string(), text)])?;
    if let Value::Object(m) = &mut extra {
        m.insert("written".into(), json!(written));
    }
    Ok(extra)
}
