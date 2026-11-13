//! The printed circuit board open in a session (ADR 0047 phase 1b).
//!
//! The document names the board by the hash of its `.kicad_pcb` text (and of the KiCad
//! project file, for net classes and rules); every `pcb.*` edit stores a new board and
//! commits a document that names it, so undo, redo and branches work on boards as on
//! features. The parsed board, its rules and its ratsnest are cached per hash.

use crate::{CommandError, Session};
use gusset_eval::BlobSource;
use gusset_history::Actor;
use gusset_pcb::Board;
use gusset_pcb::ratsnest::Ratsnest;
use gusset_pcb::rules::{NetClass, Rules};
use serde_json::Value;
use std::sync::Arc;

/// A board as the editor and the commands use it.
#[derive(Debug)]
pub struct OpenBoard {
    /// The file name shown (from the document; untrusted).
    pub file: String,
    pub board_hash: String,
    pub project_hash: Option<String>,
    pub board: Board,
    pub rules: Rules,
    pub ratsnest: Ratsnest,
}

/// The parsed board of the last hash read.
#[derive(Debug, Default)]
pub struct Cache(std::sync::Mutex<Option<Arc<OpenBoard>>>);

/// The board open in the actor's document, if any.
///
/// # Errors
/// When the stored board is missing or does not read.
pub fn open_board(s: &Session, actor: &Actor) -> Result<Option<Arc<OpenBoard>>, CommandError> {
    let Some(p) = s.document(actor)?.pcb else { return Ok(None) };
    board_of(s, &p).map(Some)
}

/// The board a document names.
///
/// # Errors
/// When the stored board is missing or does not read.
pub fn board_of(s: &Session, p: &gusset_doc::PcbBoard) -> Result<Arc<OpenBoard>, CommandError> {
    if let Ok(c) = s.pcb_cache.0.lock()
        && let Some(b) =
            c.as_ref().filter(|b| b.board_hash == p.board && b.project_hash == p.project && b.file == p.file)
    {
        return Ok(Arc::clone(b));
    }
    let missing = || {
        CommandError::new("io", "the board's stored data is missing from the project")
            .hint("open the board file again with pcb.open")
    };
    let bytes = s.blobs().blob(&p.board).ok_or_else(missing)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| CommandError::new("io", "the stored board is not text"))?;
    let board = Board::read(text, &gusset_pcb::Limits::default())
        .map_err(|e| CommandError::new("io", format!("KiCad board: {e}")))?;
    let mut rules = Rules::for_board(&board);
    if let Some(h) = &p.project
        && let Some(pro) = s.blobs().blob(h)
        && let Ok(v) = serde_json::from_slice::<Value>(&pro)
    {
        apply_project(&mut rules, &v);
    }
    let ratsnest = gusset_pcb::ratsnest::ratsnest(&board);
    let open = Arc::new(OpenBoard {
        file: p.file.clone(),
        board_hash: p.board.clone(),
        project_hash: p.project.clone(),
        board,
        rules,
        ratsnest,
    });
    if let Ok(mut c) = s.pcb_cache.0.lock() {
        *c = Some(Arc::clone(&open));
    }
    Ok(open)
}

/// A length from the project file (mm), when sane.
fn mm(v: Option<&Value>) -> Option<f64> {
    v?.as_f64().filter(|x| x.is_finite() && (0.0..100.0).contains(x))
}

static NULL: Value = Value::Null;

/// A member of a JSON object, or null.
fn at<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&NULL)
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(|t| t.chars().take(200).collect())
}

/// Net classes and minimums from a KiCad project file (`.kicad_pro`, KiCad 6 and later):
/// `net_settings.classes`, `netclass_assignments` and `netclass_patterns`, and
/// `board.design_settings.rules` (minimum track width, via annular ring, copper to edge, hole to hole, through-hole
/// and via drill, via diameter, silkscreen clearance).
/// Unknown or odd values are ignored.
pub fn apply_project(rules: &mut Rules, pro: &Value) {
    let ns = at(pro, "net_settings");
    if let Some(classes) = at(ns, "classes").as_array() {
        for c in classes.iter().take(1000) {
            let Some(name) = text(at(c, "name")) else { continue };
            let nets =
                at(c, "nets").as_array().map(|n| n.iter().take(100_000).filter_map(text).collect()).unwrap_or_default();
            let class = NetClass {
                name,
                clearance: mm(c.get("clearance")),
                track_width: mm(c.get("track_width")),
                via_diameter: mm(c.get("via_diameter")),
                via_drill: mm(c.get("via_drill")),
                nets,
            };
            rules.classes.retain(|x| x.name != class.name);
            rules.classes.push(class);
        }
    }
    if let Some(a) = at(ns, "netclass_assignments").as_object() {
        for (net, class) in a.iter().take(100_000) {
            // KiCad 8 may list several classes per net: the first counts here.
            let class = text(class).or_else(|| class.as_array().and_then(|v| v.first()).and_then(text));
            if let Some(c) = class {
                rules.assignments.push((net.chars().take(200).collect(), c));
            }
        }
    }
    if let Some(p) = at(ns, "netclass_patterns").as_array() {
        for x in p.iter().take(10_000) {
            if let (Some(pat), Some(c)) = (text(at(x, "pattern")), text(at(x, "netclass"))) {
                rules.patterns.push((pat, c));
            }
        }
    }
    rules.apply_default_class();
    let r = pro.pointer("/board/design_settings/rules").unwrap_or(&NULL);
    if let Some(v) = mm(r.get("min_track_width")) {
        rules.min_track_width = v;
    }
    if let Some(v) = mm(r.get("min_via_annular_width")) {
        rules.min_annular_ring = v;
    }
    if let Some(v) = mm(r.get("min_copper_edge_clearance")) {
        rules.edge_clearance = v;
    }
    // ADR 0048: hole to hole, drill sizes, via copper and silkscreen.
    if let Some(v) = mm(r.get("min_hole_to_hole")) {
        rules.hole_to_hole = v;
    }
    if let Some(v) = mm(r.get("min_through_hole_diameter")) {
        rules.min_through_hole = v;
        rules.min_via_drill = v;
    }
    if let Some(v) = mm(r.get("min_via_diameter")) {
        rules.min_via_diameter = v;
    }
    if let Some(v) = mm(r.get("min_silk_clearance")) {
        rules.silk_clearance = v;
    }
}

/// A Freerouting install the user named in Settings (ADR 0047 phase 3). Gusset never
/// downloads or bundles it (GPL; ADR 0001): `pcb.autoroute` runs it as a separate process
/// on a DSN export and reads its SES back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Freerouting {
    /// `freerouting.jar` (run with `java -jar`) or an executable.
    pub path: std::path::PathBuf,
    /// The Java runtime for a jar (default: `java` on the PATH).
    pub java: Option<std::path::PathBuf>,
    /// Longest run before it is stopped (seconds).
    pub timeout_s: u64,
    /// Extra arguments the user added (e.g. a headless switch for their version).
    pub args: Vec<String>,
}
