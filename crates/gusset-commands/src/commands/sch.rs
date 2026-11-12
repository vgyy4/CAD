//! Schematic commands (ADR 0047 phase 2a): open or start a KiCad schematic in the
//! document, edit it with undo (symbols from a user-given library, wires, labels), check
//! it, list its nets and bring the open board in line with it. Coordinates are KiCad's:
//! mm, Y down. Text from the files (references, net and label names) reaches agents only
//! in `_user_text` fields.

use super::pcb::{no_board, write_outputs};
use super::pcb_edit::commit_board;
use super::*;
use crate::sch_session::{self, OpenSchematic};
use gusset_pcb::Limits;
use gusset_pcb::sch::{LabelKind, Mirror, Schematic, SymbolLib};
use gusset_pcb::sch_edit::Item;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        spec::<SchOpen>(),
        spec::<SchNew>(),
        spec::<SchSave>(),
        spec::<SchAddSymbol>(),
        spec::<SchMove>(),
        spec::<SchSetField>(),
        spec::<SchAddWire>(),
        spec::<SchAddLabel>(),
        spec::<SchDelete>(),
        spec::<SchErc>(),
        spec::<SchNetlist>(),
        spec::<PcbUpdateFromSchematic>(),
    ]
}

/// Largest schematic sheet or symbol library read (bytes).
const MAX_SHEET_BYTES: u64 = 64 << 20;
/// Most sheet files a design reads.
const MAX_SHEETS: usize = 256;
/// Largest footprint file read for a board update.
const MAX_FOOTPRINT_BYTES: u64 = 16 << 20;
/// Most entries of each list in a result.
const MAX_LIST: usize = 200;

fn sch_err(e: gusset_pcb::sch::Error) -> CommandError {
    match e {
        gusset_pcb::sch::Error::Edit(m) => CommandError::args(m),
        e => CommandError::new("io", format!("KiCad schematic: {e}")),
    }
}

fn no_schematic() -> CommandError {
    CommandError::not_found("no schematic is open").hint("open one with sch.open or start one with sch.new")
}

fn limits() -> Limits {
    Limits::default()
}

/// A shown file name: the last part of a path, cut to 255 characters.
fn file_name(path: &str) -> String {
    let n = path.rsplit(['/', '\\']).next().unwrap_or(path);
    n.chars().filter(|c| !c.is_control()).take(255).collect()
}

/// Reads a text file the user or agent may read, up to `max` bytes.
fn read_text(s: &Session, ctx: &CmdCtx, path: &str, max: u64) -> Result<String, CommandError> {
    crate::paths::refuse_remote(path)?;
    let full = crate::paths::read_target(s.project_dir.as_deref(), path, &ctx.actor)?;
    let f = crate::import::open_regular(&full, path, max)?;
    let bytes = crate::import::read_capped(f, max, &AtomicBool::new(false))?;
    String::from_utf8(bytes).map_err(|_| CommandError::new("io", format!("'{}' is not UTF-8 text", file_name(path))))
}

/// A sheet file name from a schematic (untrusted): relative, without `..`, ending in
/// `.kicad_sch`.
fn safe_sheet_name(f: &str) -> bool {
    !f.is_empty()
        && f.len() <= 255
        && f.to_ascii_lowercase().ends_with(".kicad_sch")
        && !f.starts_with(['/', '\\'])
        && !f.contains(':')
        && !f.split(['/', '\\']).any(|p| p == ".." || p.is_empty())
        && !f.chars().any(char::is_control)
}

/// The open schematic, a sheet of it (`None`: the root) edited by `f`, committed.
fn sheet_edit(
    s: &mut Session,
    ctx: &CmdCtx,
    label: &str,
    sheet: Option<&str>,
    f: impl FnOnce(&mut Schematic, &OpenSchematic) -> Result<Value, CommandError>,
) -> Result<Value, CommandError> {
    let open = sch_session::open_schematic(s, &ctx.actor)?.ok_or_else(no_schematic)?;
    let (i, sch) = open.sheet(sheet).ok_or_else(|| {
        CommandError::not_found(format!(
            "the schematic has no sheet '{}'",
            crate::paths::display_name(sheet.unwrap_or(""))
        ))
    })?;
    let mut edited = sch.clone();
    let extra = f(&mut edited, &open)?;
    commit_sheets(s, ctx, label, open.doc.clone(), &[(i, &edited)], extra)
}

/// Stores edited sheets (by index: 0 the root) and commits a document naming them.
fn commit_sheets(
    s: &mut Session,
    ctx: &CmdCtx,
    label: &str,
    mut doc: gusset_doc::PcbSchematic,
    sheets: &[(usize, &Schematic)],
    extra: Value,
) -> Result<Value, CommandError> {
    for (i, sch) in sheets {
        let hash = s.put_blob(sch.write().as_bytes()).to_hex();
        match i.checked_sub(1) {
            None => doc.sheet = hash,
            Some(k) => {
                if let Some(slot) = doc.sheets.get_mut(k) {
                    slot.1 = hash;
                }
            }
        }
    }
    let mut out = edit(s, ctx, label, |d, _| {
        d.schematic = Some(doc);
        Ok(extra)
    })?;
    if let Some(open) = sch_session::open_schematic(s, &ctx.actor)?
        && let Value::Object(m) = &mut out
    {
        m.insert("erc_issues".into(), json!(open.erc.len()));
    }
    Ok(out)
}

/// Opens a KiCad schematic (and the sheets it uses) for editing.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchOpen {
    /// .kicad_sch (sub-sheets are read from beside it).
    pub path: String,
}

impl Command for SchOpen {
    const ID: &'static str = "sch.open";
    const TITLE: &'static str = "Open schematic";
    const DESCRIPTION: &'static str =
        "Open a KiCad schematic and its sheets for editing in the Schematic tab (undoable).";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if !self.path.to_ascii_lowercase().ends_with(".kicad_sch") {
            return Err(CommandError::args("sch.open reads .kicad_sch schematics"));
        }
        let folder = self.path.rsplit_once(['/', '\\']).map_or("", |x| x.0).to_string();
        let root_text = read_text(s, ctx, &self.path, MAX_SHEET_BYTES)?;
        let root = Schematic::read(&root_text, &limits()).map_err(sch_err)?;
        // The sheets it uses, breadth first, each file once.
        let mut sheets: Vec<(String, String, Schematic)> = Vec::new();
        let mut missing: Vec<String> = Vec::new();
        let mut todo: Vec<String> = root.sheets.iter().map(|x| x.file.clone()).collect();
        let mut k = 0;
        while let Some(f) = todo.get(k).cloned() {
            k += 1;
            if sheets.iter().any(|x| x.0 == f) || missing.contains(&f) || f == file_name(&self.path) {
                continue;
            }
            if !safe_sheet_name(&f) || sheets.len() >= MAX_SHEETS {
                missing.push(f);
                continue;
            }
            let path = if folder.is_empty() { f.clone() } else { format!("{folder}/{f}") };
            match read_text(s, ctx, &path, MAX_SHEET_BYTES).map(|t| (Schematic::read(&t, &limits()), t)) {
                Ok((Ok(sub), text)) => {
                    todo.extend(sub.sheets.iter().map(|x| x.file.clone()));
                    sheets.push((f, text, sub));
                }
                _ => missing.push(f),
            }
        }
        let doc = gusset_doc::PcbSchematic {
            file: file_name(&self.path),
            sheet: s.put_blob(root_text.as_bytes()).to_hex(),
            sheets: sheets.iter().map(|(f, t, _)| (f.clone(), s.put_blob(t.as_bytes()).to_hex())).collect(),
        };
        let label = format!("Open schematic {}", crate::paths::display_name(&doc.file));
        let symbols = root.symbols.len() + sheets.iter().map(|x| x.2.symbols.len()).sum::<usize>();
        let notes: Vec<String> = root.notes.iter().take(20).cloned().collect();
        let out = json!({
            "sheets": 1 + sheets.len(),
            "symbols": symbols,
            "missing_sheets_user_text": missing.iter().take(MAX_LIST).collect::<Vec<_>>(),
            "notes_user_text": notes,
        });
        commit_sheets(s, ctx, &label, doc, &[], out)
    }
}

/// Starts an empty schematic.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchNew {
    /// File name (default schematic.kicad_sch).
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for SchNew {
    const ID: &'static str = "sch.new";
    const TITLE: &'static str = "New schematic";
    const DESCRIPTION: &'static str = "Start an empty A4 schematic sheet.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let mut file = file_name(self.name.as_deref().unwrap_or("schematic.kicad_sch"));
        if !file.to_ascii_lowercase().ends_with(".kicad_sch") {
            file.push_str(".kicad_sch");
        }
        let sch = Schematic::new_empty(&file).map_err(sch_err)?;
        let doc =
            gusset_doc::PcbSchematic { file, sheet: s.put_blob(sch.write().as_bytes()).to_hex(), sheets: Vec::new() };
        commit_sheets(s, ctx, "New schematic", doc, &[], json!({}))
    }
}

/// Writes the open schematic's sheets as KiCad files.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchSave {
    /// Root .kicad_sch (default: its name); sub-sheets go beside it.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for SchSave {
    const ID: &'static str = "sch.save";
    const TITLE: &'static str = "Save schematic";
    const DESCRIPTION: &'static str = "Write the open schematic and its sheets as .kicad_sch files.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let open = sch_session::open_schematic(s, &ctx.actor)?.ok_or_else(no_schematic)?;
        let path = self.path.clone().unwrap_or_else(|| open.file().to_string());
        if !path.to_ascii_lowercase().ends_with(".kicad_sch") {
            return Err(CommandError::args("the schematic file name ends in .kicad_sch"));
        }
        let (folder, name) = path.rsplit_once(['/', '\\']).unwrap_or(("", &path));
        let mut files = Vec::new();
        for (k, (f, sch)) in open.design.sheets.iter().enumerate() {
            let n = if k == 0 { name.to_string() } else { f.clone() };
            if k > 0 && !safe_sheet_name(&n) {
                continue;
            }
            files.push((n, sch.write()));
        }
        let written = write_outputs(s, ctx, folder, self.overwrite, "kicad_sch", &files)?;
        Ok(json!({ "written": written }))
    }
}

fn mirror_of(m: Option<&str>, current: Mirror) -> Result<Mirror, CommandError> {
    match m {
        None => Ok(current),
        Some("none") => Ok(Mirror::None),
        Some("x") => Ok(Mirror::X),
        Some("y") => Ok(Mirror::Y),
        Some(_) => Err(CommandError::args("mirror is none, x or y")),
    }
}

/// Places a symbol from a KiCad symbol library.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchAddSymbol {
    /// .kicad_sym file.
    pub library: String,
    /// Symbol name in it.
    pub symbol: String,
    /// mm.
    pub at: [f64; 2],
    /// Default: next free (R1, R2...).
    #[serde(default)]
    pub reference: Option<String>,
    /// 0/90/180/270.
    #[serde(default)]
    pub angle: Option<f64>,
    /// none/x/y.
    #[serde(default)]
    pub mirror: Option<String>,
    #[serde(default)]
    pub unit: Option<u32>,
    /// Sub-sheet file (default: root).
    #[serde(default)]
    pub sheet: Option<String>,
}

impl Command for SchAddSymbol {
    const ID: &'static str = "sch.add_symbol";
    const TITLE: &'static str = "Add symbol";
    const DESCRIPTION: &'static str = "Place a symbol from a .kicad_sym library on the open schematic.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if !self.library.to_ascii_lowercase().ends_with(".kicad_sym") {
            return Err(CommandError::args("the library is a .kicad_sym file"));
        }
        let text = read_text(s, ctx, &self.library, MAX_SHEET_BYTES)?;
        let lib = SymbolLib::read(&text, &limits()).map_err(sch_err)?;
        let stem = file_name(&self.library);
        let stem = stem.get(..stem.len().saturating_sub(".kicad_sym".len())).unwrap_or("lib").to_string();
        let lib_id = format!("{stem}:{}", self.symbol);
        let node = lib.embedded_node(&self.symbol, &lib_id).ok_or_else(|| {
            CommandError::not_found(format!("the library has no symbol '{}'", crate::paths::display_name(&self.symbol)))
                .hint("sch.netlist lists placed parts; the library's symbols are its (symbol \"...\") entries")
        })?;
        let label = format!("Add {}", crate::paths::display_name(&self.symbol));
        let mirror = mirror_of(self.mirror.as_deref(), Mirror::None)?;
        sheet_edit(s, ctx, &label, self.sheet.as_deref(), |sch, open| {
            let prefix = lib.symbols.iter().find(|x| x.name == self.symbol).map_or_else(|| "U".into(), |l| l.prefix());
            let reference = match &self.reference {
                Some(r) => r.clone(),
                None => {
                    // Free across every sheet of the design.
                    let mut r = sch.next_reference(&prefix);
                    let used = |r: &str| open.netlist.components.iter().any(|c| c.reference == r);
                    let mut n = 1;
                    while used(&r) && n < 100_000 {
                        n += 1;
                        r = format!("{prefix}{n}");
                    }
                    r
                }
            };
            let project = sch.project_name().unwrap_or_else(|| {
                let f = open.file();
                f.get(..f.len().saturating_sub(".kicad_sch".len())).unwrap_or(f).to_string()
            });
            let i = sch
                .add_symbol(
                    &node,
                    &lib_id,
                    self.at,
                    self.angle.unwrap_or(0.0),
                    mirror,
                    self.unit.unwrap_or(1),
                    &reference,
                    &project,
                    &limits(),
                )
                .map_err(sch_err)?;
            let pins = sch.symbols.get(i).map_or(0, |x| sch.pins(x).len());
            Ok(json!({ "symbol": i, "reference_user_text": reference, "pins": pins }))
        })
    }
}

/// Moves, turns or mirrors a symbol or label.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchMove {
    /// Reference (R1), symbol#i or label#i.
    pub item: String,
    #[serde(default)]
    pub at: Option<[f64; 2]>,
    #[serde(default)]
    pub angle: Option<f64>,
    /// none/x/y (symbols).
    #[serde(default)]
    pub mirror: Option<String>,
    #[serde(default)]
    pub sheet: Option<String>,
}

impl Command for SchMove {
    const ID: &'static str = "sch.move";
    const TITLE: &'static str = "Move symbol";
    const DESCRIPTION: &'static str = "Move, rotate or mirror a symbol or label of the open schematic.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if self.at.is_none() && self.angle.is_none() && self.mirror.is_none() {
            return Err(CommandError::args("give at, angle or mirror"));
        }
        let label = format!("Move {}", crate::paths::display_name(&self.item));
        sheet_edit(s, ctx, &label, self.sheet.as_deref(), |sch, _| {
            let item = Item::parse(sch, &self.item).ok_or_else(|| {
                CommandError::not_found(format!("no item '{}'", crate::paths::display_name(&self.item)))
                    .hint("name a symbol by reference (R1) or symbol#i / label#i")
            })?;
            match item {
                Item::Symbol(i) => {
                    let x = sch.symbols.get(i).ok_or_else(no_schematic)?;
                    let mirror = mirror_of(self.mirror.as_deref(), x.mirror)?;
                    let (at, angle) = (self.at.unwrap_or(x.at), self.angle.unwrap_or(x.angle));
                    sch.move_symbol(i, at, angle, mirror, &limits()).map_err(sch_err)?;
                    let x = sch.symbols.get(i).ok_or_else(no_schematic)?;
                    Ok(json!({ "at": x.at, "angle": x.angle }))
                }
                Item::Label(i) => {
                    let l = sch.labels.get(i).ok_or_else(no_schematic)?;
                    let (at, angle) = (self.at.unwrap_or(l.at), self.angle.unwrap_or(l.angle));
                    sch.move_label(i, at, angle, &limits()).map_err(sch_err)?;
                    Ok(json!({ "at": at, "angle": angle }))
                }
                _ => Err(CommandError::args("only symbols and labels move; delete and redraw wires")),
            }
        })
    }
}

/// Sets a field of a symbol (value, footprint, reference or any other).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchSetField {
    /// Reference (R1) or symbol#i.
    pub symbol: String,
    /// Value, Footprint, Reference, ...
    pub field: String,
    pub value: String,
    #[serde(default)]
    pub sheet: Option<String>,
}

impl Command for SchSetField {
    const ID: &'static str = "sch.set_field";
    const TITLE: &'static str = "Set field";
    const DESCRIPTION: &'static str =
        "Set a symbol's field (Value, Footprint as Lib:Name, Reference...) in the open schematic.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let label =
            format!("Set {} of {}", crate::paths::display_name(&self.field), crate::paths::display_name(&self.symbol));
        sheet_edit(s, ctx, &label, self.sheet.as_deref(), |sch, _| {
            let Some(Item::Symbol(i)) = Item::parse(sch, &self.symbol) else {
                return Err(CommandError::not_found(format!(
                    "no symbol '{}'",
                    crate::paths::display_name(&self.symbol)
                ))
                .hint("name it by reference (R1) or symbol#i"));
            };
            sch.set_field(i, &self.field, &self.value, &limits()).map_err(sch_err)?;
            Ok(json!({ "symbol": format!("symbol#{i}") }))
        })
    }
}

/// Draws a wire.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchAddWire {
    /// Polyline corners, mm.
    pub points: Vec<[f64; 2]>,
    #[serde(default)]
    pub sheet: Option<String>,
}

impl Command for SchAddWire {
    const ID: &'static str = "sch.add_wire";
    const TITLE: &'static str = "Add wire";
    const DESCRIPTION: &'static str =
        "Draw a wire (polyline) on the open schematic; junction dots are added where it joins a wire's middle.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        sheet_edit(s, ctx, "Add wire", self.sheet.as_deref(), |sch, _| {
            let before = sch.junctions.len();
            let n = sch.add_wires(&self.points, &limits()).map_err(sch_err)?;
            Ok(json!({ "wires": n, "junctions_added": sch.junctions.len().saturating_sub(before) }))
        })
    }
}

/// Places a net label.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchAddLabel {
    pub text: String,
    pub at: [f64; 2],
    /// local (default), global, hierarchical.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub angle: Option<f64>,
    /// input/output/bidirectional/tri_state/passive.
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub sheet: Option<String>,
}

impl Command for SchAddLabel {
    const ID: &'static str = "sch.add_label";
    const TITLE: &'static str = "Add label";
    const DESCRIPTION: &'static str = "Place a net label (local, global or hierarchical) on the open schematic.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let kind = match self.kind.as_deref().unwrap_or("local") {
            "local" => LabelKind::Local,
            "global" => LabelKind::Global,
            "hierarchical" => LabelKind::Hierarchical,
            _ => return Err(CommandError::args("kind is local, global or hierarchical")),
        };
        sheet_edit(s, ctx, "Add label", self.sheet.as_deref(), |sch, _| {
            let i = sch
                .add_label(kind, &self.text, self.at, self.angle.unwrap_or(0.0), self.shape.as_deref(), &limits())
                .map_err(sch_err)?;
            Ok(json!({ "label": i }))
        })
    }
}

/// Deletes schematic items.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchDelete {
    /// R1, symbol#i, wire#i, label#i, junction#i, no_connect#i, bus#i.
    pub items: Vec<String>,
    #[serde(default)]
    pub sheet: Option<String>,
}

impl Command for SchDelete {
    const ID: &'static str = "sch.delete";
    const TITLE: &'static str = "Delete from schematic";
    const DESCRIPTION: &'static str =
        "Delete symbols, wires, labels, junctions or no-connect flags of the open schematic.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if self.items.is_empty() || self.items.len() > 100_000 {
            return Err(CommandError::args("name 1 to 100000 items to delete"));
        }
        sheet_edit(s, ctx, "Delete from schematic", self.sheet.as_deref(), |sch, _| {
            let mut items = Vec::with_capacity(self.items.len());
            for name in &self.items {
                items.push(Item::parse(sch, name).ok_or_else(|| {
                    CommandError::not_found(format!("no item '{}'", crate::paths::display_name(name)))
                })?);
            }
            let n = sch.delete(&items, &limits()).map_err(sch_err)?;
            Ok(json!({ "deleted": n }))
        })
    }
}

/// The sheet file of a sheet use.
fn use_file(open: &OpenSchematic, u: usize) -> String {
    open.netlist.uses.get(u).and_then(|su| open.design.sheets.get(su.sheet)).map(|(f, _)| f.clone()).unwrap_or_default()
}

/// Checks the open schematic's electrical rules.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchErc {}

impl Command for SchErc {
    const ID: &'static str = "sch.erc";
    const TITLE: &'static str = "Electrical rule check";
    const DESCRIPTION: &'static str =
        "Check the open schematic: unconnected pins, pin type conflicts, undriven power, labels, references.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let open = sch_session::open_schematic(s, &ctx.actor)?.ok_or_else(no_schematic)?;
        let issues: Vec<Value> = open
            .erc
            .iter()
            .take(MAX_LIST)
            .map(|i| {
                json!({
                    "code": i.code,
                    "severity": i.severity.name(),
                    "message_user_text": i.message,
                    "sheet_user_text": use_file(&open, i.sheet_use),
                    "at": [i.at[0], i.at[1]],
                })
            })
            .collect();
        Ok(json!({
            "issues": issues,
            "more": open.erc.len() > MAX_LIST,
            "errors": open.erc.iter().filter(|i| i.severity == gusset_pcb::erc::Severity::Error).count(),
        }))
    }
}

/// Lists the open schematic's nets.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchNetlist {
    /// Only nets whose name contains this.
    #[serde(default)]
    pub filter: Option<String>,
}

impl Command for SchNetlist {
    const ID: &'static str = "sch.netlist";
    const TITLE: &'static str = "Netlist";
    const DESCRIPTION: &'static str = "List the open schematic's nets (pins as REF.PIN) and its parts.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let open = sch_session::open_schematic(s, &ctx.actor)?.ok_or_else(no_schematic)?;
        let f = self.filter.as_deref().unwrap_or("");
        let nets: Vec<&gusset_pcb::netlist::Net> = open.netlist.nets.iter().filter(|n| n.name.contains(f)).collect();
        let list: Vec<Value> = nets
            .iter()
            .take(MAX_LIST * 5)
            .map(|n| {
                let pins: Vec<String> =
                    n.nodes.iter().take(MAX_LIST).map(|x| format!("{}.{}", x.reference, x.pin)).collect();
                json!({ "name_user_text": n.name, "pins_user_text": pins })
            })
            .collect();
        let parts: Vec<Value> = open
            .netlist
            .components
            .iter()
            .take(MAX_LIST * 5)
            .map(|c| json!({ "reference_user_text": c.reference, "value_user_text": c.value, "footprint_user_text": c.footprint }))
            .collect();
        Ok(json!({ "nets": list, "more_nets": nets.len() > MAX_LIST * 5, "parts": parts }))
    }
}

/// Footprint files for a part's `Lib:Name` under `dir`: `Lib.pretty/Name.kicad_mod`, then
/// `Name.kicad_mod`. Names with path parts are refused.
pub(crate) fn footprint_candidates(dir: &str, lib_id: &str) -> Vec<String> {
    let ok = |p: &str| {
        !p.is_empty()
            && p.len() <= 200
            && p != ".."
            && !p.contains(['/', '\\', ':'])
            && !p.chars().any(char::is_control)
    };
    let (lib, name) = lib_id.split_once(':').unwrap_or(("", lib_id));
    if !ok(name) {
        return Vec::new();
    }
    let dir = dir.trim_end_matches(['/', '\\']);
    let mut v = Vec::new();
    if ok(lib) {
        v.push(format!("{dir}/{lib}.pretty/{name}.kicad_mod"));
    }
    v.push(format!("{dir}/{name}.kicad_mod"));
    v
}

/// Brings the open board in line with the open schematic.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbUpdateFromSchematic {
    /// Folder of footprint libraries (Lib.pretty) for parts not yet on the board.
    #[serde(default)]
    pub footprints: Option<String>,
    /// Report only.
    #[serde(default)]
    pub dry_run: bool,
}

impl Command for PcbUpdateFromSchematic {
    const ID: &'static str = "pcb.update_from_schematic";
    const TITLE: &'static str = "Update PCB from schematic";
    const DESCRIPTION: &'static str = "Set the open board's pad nets from the open schematic, add missing footprints found in a folder, report differences.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let sch = sch_session::open_schematic(s, &ctx.actor)?.ok_or_else(no_schematic)?;
        let open = crate::pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let mut board = open.board.clone();
        let mut read = 0usize;
        let session: &Session = s;
        let mut find = |lib_id: &str| -> Option<String> {
            let dir = self.footprints.as_deref()?;
            if read >= 10_000 {
                return None;
            }
            footprint_candidates(dir, lib_id).into_iter().find_map(|p| {
                read += 1;
                read_text(session, ctx, &p, MAX_FOOTPRINT_BYTES).ok()
            })
        };
        let r =
            gusset_pcb::sync::update_board(&mut board, &sch.netlist, &mut find, &limits()).map_err(|e| match e {
                gusset_pcb::board::Error::Edit(m) => CommandError::args(m),
                e => CommandError::new("io", format!("KiCad board: {e}")),
            })?;
        let cap = |v: Vec<String>| v.into_iter().take(MAX_LIST).collect::<Vec<_>>();
        let pairs = |v: &[(String, String)]| v.iter().take(MAX_LIST).map(|(a, b)| json!([a, b])).collect::<Vec<_>>();
        let triples = |v: &[(String, String, String)]| {
            v.iter().take(MAX_LIST).map(|(a, b, c)| json!([a, b, c])).collect::<Vec<_>>()
        };
        let changed = r.pads_changed > 0 || !r.added.is_empty();
        let report = json!({
            "pads_changed": r.pads_changed,
            "added_user_text": cap(r.added.clone()),
            "not_found_user_text": pairs(&r.not_found),
            "no_footprint_user_text": cap(r.no_footprint.clone()),
            "extra_user_text": cap(r.extra.clone()),
            "value_differs_user_text": triples(&r.value_differs),
            "footprint_differs_user_text": triples(&r.footprint_differs),
            "pins_without_pad_user_text": pairs(&r.pins_without_pad),
            "changed": changed && !self.dry_run,
        });
        if self.dry_run || !changed {
            return Ok(report);
        }
        let open: Arc<crate::pcb_session::OpenBoard> = open;
        commit_board(s, ctx, "Update PCB from schematic", open.file.clone(), &board, None, report)
    }
}
