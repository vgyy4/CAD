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
use gusset_pcb::annotate;
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
        spec::<SchAddSheet>(),
        spec::<SchSheetPin>(),
        spec::<SchErc>(),
        spec::<SchNetlist>(),
        spec::<SchAnnotate>(),
        spec::<PcbUpdateFromSchematic>(),
    ]
}

/// Largest schematic sheet or symbol library read (bytes).
const MAX_SHEET_BYTES: u64 = 64 << 20;
/// Most sheet files a design reads.
const MAX_SHEETS: usize = 256;
/// Largest footprint file read for a board update.
pub(super) const MAX_FOOTPRINT_BYTES: u64 = 16 << 20;
/// Most entries of each list in a result.
const MAX_LIST: usize = 200;

fn sch_err(e: gusset_pcb::sch::Error) -> CommandError {
    match e {
        gusset_pcb::sch::Error::Edit(m) => CommandError::args(m),
        e => CommandError::new("io", format!("KiCad schematic: {e}")),
    }
}

pub(super) fn no_schematic() -> CommandError {
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
pub(super) fn read_text(s: &Session, ctx: &CmdCtx, path: &str, max: u64) -> Result<String, CommandError> {
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
    sheet_edit_ex(s, ctx, label, sheet, |sch, open, _| f(sch, open))
}

/// Changes an edit may make beyond its own sheet (ADR 0057): sheet files to start and
/// sheet files to forget.
#[derive(Default)]
struct Files {
    /// New sub-sheet files and their (empty) schematics.
    add: Vec<(String, Schematic)>,
    /// Sheet files no sheet uses any more.
    drop: Vec<String>,
}

/// [`sheet_edit`] where the edit may also add or drop sheet files of the document.
fn sheet_edit_ex(
    s: &mut Session,
    ctx: &CmdCtx,
    label: &str,
    sheet: Option<&str>,
    f: impl FnOnce(&mut Schematic, &OpenSchematic, &mut Files) -> Result<Value, CommandError>,
) -> Result<Value, CommandError> {
    let open = sch_session::open_schematic(s, &ctx.actor)?.ok_or_else(no_schematic)?;
    let (i, sch) = open.sheet(sheet).ok_or_else(|| {
        CommandError::not_found(format!(
            "the schematic has no sheet '{}'",
            crate::paths::display_name(sheet.unwrap_or(""))
        ))
    })?;
    let mut edited = sch.clone();
    let mut files = Files::default();
    let extra = f(&mut edited, &open, &mut files)?;
    let mut doc = open.doc.clone();
    let edited_file = open.design.sheets.get(i).map(|x| x.0.clone()).unwrap_or_default();
    doc.sheets.retain(|(f, _)| !files.drop.contains(f));
    for (f, _) in &files.add {
        doc.sheets.push((f.clone(), String::new()));
    }
    // Indexes into the document (0: the root, then its sheets in order).
    let at = |file: &str| doc.sheets.iter().position(|(f, _)| f == file).map(|k| k + 1);
    let mut list: Vec<(usize, &Schematic)> = Vec::new();
    match if i == 0 { Some(0) } else { at(&edited_file) } {
        Some(k) => list.push((k, &edited)),
        None => return Err(CommandError::args("the edit removed the sheet it was made on")),
    }
    for (f, sch) in &files.add {
        list.extend(at(f).map(|k| (k, sch)));
    }
    commit_sheets(s, ctx, label, doc.clone(), &list, extra)
}

/// The sheet files no sheet reaches from the root, given the design's files with one
/// sheet replaced by its edited version.
fn unreachable_files(open: &OpenSchematic, edited_file: &str, edited: &Schematic) -> Vec<String> {
    let root = open.design.sheets.first().map(|x| x.0.clone()).unwrap_or_default();
    let sheet_of = |f: &str| -> Option<&Schematic> {
        if f == edited_file { Some(edited) } else { open.design.sheets.iter().find(|x| x.0 == f).map(|x| &x.1) }
    };
    let mut seen = vec![root];
    let mut k = 0;
    while let Some(f) = seen.get(k).cloned() {
        k += 1;
        for sh in sheet_of(&f).map(|s| s.sheets.as_slice()).unwrap_or_default() {
            if !seen.contains(&sh.file) && seen.len() < 100_000 {
                seen.push(sh.file.clone());
            }
        }
    }
    open.design.sheets.iter().skip(1).map(|x| x.0.clone()).filter(|f| !seen.contains(f)).collect()
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
    const DESCRIPTION: &'static str = "Open a KiCad schematic and its sheets for editing (undoable).";
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
    /// Default schematic.kicad_sch.
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
    /// Root .kicad_sch (default: its name).
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

/// Numbers the design's symbols.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchAnnotate {
    /// Renumber every symbol, not only R? ones.
    #[serde(default)]
    pub all: bool,
    /// x (default) or y: which axis goes first.
    #[serde(default)]
    pub order: Option<String>,
    /// Sheet n starts at n times this (100, 1000).
    #[serde(default)]
    pub sheet_step: Option<u32>,
}

impl Command for SchAnnotate {
    const ID: &'static str = "sch.annotate";
    const TITLE: &'static str = "Annotate";
    const DESCRIPTION: &'static str =
        "Number unannotated symbols (R? to R3) across the open schematic's sheets; units of a part share a number.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let y_first = match self.order.as_deref() {
            None | Some("x") => false,
            Some("y") => true,
            Some(_) => return Err(CommandError::args("order is x or y")),
        };
        let sheet_step = self.sheet_step.unwrap_or(0);
        if sheet_step > 1_000_000 {
            return Err(CommandError::args("sheet_step is at most 1000000"));
        }
        let open = sch_session::open_schematic(s, &ctx.actor)?.ok_or_else(no_schematic)?;
        let sheet_refs: Vec<&Schematic> = open.design.sheets.iter().map(|(_, x)| x).collect();
        let opts = annotate::Options { all: self.all, y_first, sheet_step };
        let plan = annotate::plan(&sheet_refs, opts);
        let mut sheets: Vec<Option<Schematic>> = vec![None; open.design.sheets.len()];
        let mut renamed = Vec::new();
        for r in &plan {
            let Some((_, orig)) = open.design.sheets.get(r.sheet) else { continue };
            let Some(slot) = sheets.get_mut(r.sheet) else { continue };
            let sch = slot.get_or_insert_with(|| orig.clone());
            sch.set_field(r.symbol, "Reference", &r.new, &limits()).map_err(sch_err)?;
            let shown = format!("{}{}", r.new, r.unit_letter.as_deref().unwrap_or(""));
            renamed.push(json!([r.old, shown]));
        }
        let edited: Vec<(usize, &Schematic)> =
            sheets.iter().enumerate().filter_map(|(i, x)| x.as_ref().map(|x| (i, x))).collect();
        if edited.is_empty() {
            return Ok(json!({ "renamed_user_text": [], "changed": false }));
        }
        let shown: Vec<&Value> = renamed.iter().take(MAX_LIST).collect();
        let extra = json!({ "renamed_user_text": shown, "changed": true, "count": renamed.len() });
        commit_sheets(s, ctx, "Annotate", open.doc.clone(), &edited, extra)
    }
}

/// Places a symbol from a KiCad symbol library.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchAddSymbol {
    /// .kicad_sym file.
    pub library: String,
    pub symbol: String,
    /// mm.
    pub at: [f64; 2],
    /// Default: next free.
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
    /// Sub-sheet (default: root).
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
    /// Reference (R1), symbol#i, label#i (wire#i, ... with by).
    pub item: String,
    #[serde(default)]
    pub at: Option<[f64; 2]>,
    /// Shift in mm (any item).
    #[serde(default)]
    pub by: Option<[f64; 2]>,
    /// More items shifted the same; wires on their pins follow.
    #[serde(default)]
    pub with: Vec<String>,
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
    const DESCRIPTION: &'static str = "Move, rotate or mirror a symbol or label of the open schematic; with/by shifts several items in one undo step.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if self.at.is_none() && self.by.is_none() && self.angle.is_none() && self.mirror.is_none() {
            return Err(CommandError::args("give at, by, angle or mirror"));
        }
        if self.with.len() > 100_000 {
            return Err(CommandError::args("at most 100000 items move together"));
        }
        let group = self.by.is_some() || !self.with.is_empty();
        if group && (self.angle.is_some() || self.mirror.is_some()) {
            return Err(CommandError::args("angle and mirror turn one symbol or label; not with by or with"));
        }
        let label = if self.with.is_empty() {
            format!("Move {}", crate::paths::display_name(&self.item))
        } else {
            "Move selection".to_string()
        };
        sheet_edit(s, ctx, &label, self.sheet.as_deref(), |sch, _| {
            let parse = |name: &str| {
                Item::parse(sch, name).ok_or_else(|| {
                    CommandError::not_found(format!("no item '{}'", crate::paths::display_name(name)))
                        .hint("name a symbol by reference (R1) or symbol#i / label#i")
                })
            };
            let item = parse(&self.item)?;
            if group {
                let delta = match (self.by, self.at) {
                    (Some(d), None) => d,
                    (None, Some(at)) => {
                        let from = match item {
                            Item::Symbol(i) => sch.symbols.get(i).map(|x| x.at),
                            Item::Label(i) => sch.labels.get(i).map(|x| x.at),
                            Item::Sheet(i) => sch.sheets.get(i).map(|x| x.at),
                            _ => None,
                        }
                        .ok_or_else(|| CommandError::args("at names a place for a symbol or label; use by"))?;
                        [at[0] - from[0], at[1] - from[1]]
                    }
                    _ => return Err(CommandError::args("give by or at, not both")),
                };
                let mut items = vec![item];
                for name in &self.with {
                    items.push(parse(name)?);
                }
                let n = sch.move_items(&items, delta, &limits()).map_err(sch_err)?;
                return Ok(json!({ "moved": n, "by": delta }));
            }
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
                Item::Sheet(i) => {
                    let at = self.at.ok_or_else(|| CommandError::args("a sheet moves by at or by"))?;
                    if self.angle.is_some() || self.mirror.is_some() {
                        return Err(CommandError::args("sheets move; they do not turn or mirror"));
                    }
                    let from = sch.sheets.get(i).ok_or_else(no_schematic)?.at;
                    let by = [at[0] - from[0], at[1] - from[1]];
                    sch.move_items(&[item], by, &limits()).map_err(sch_err)?;
                    Ok(json!({ "moved": 1, "by": by }))
                }
                _ => Err(CommandError::args("only symbols, labels and sheets move; delete and redraw wires")),
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
    pub field: String,
    pub value: String,
    #[serde(default)]
    pub sheet: Option<String>,
}

impl Command for SchSetField {
    const ID: &'static str = "sch.set_field";
    const TITLE: &'static str = "Set field";
    const DESCRIPTION: &'static str = "Set a symbol's field (Value, Footprint as Lib:Name, ...).";
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
    const DESCRIPTION: &'static str = "Draw a wire (polyline); junctions are added where it joins a wire's middle.";
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
    const DESCRIPTION: &'static str = "Delete items of the open schematic.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if self.items.is_empty() || self.items.len() > 100_000 {
            return Err(CommandError::args("name 1 to 100000 items to delete"));
        }
        sheet_edit_ex(s, ctx, "Delete from schematic", self.sheet.as_deref(), |sch, open, files| {
            let mut items = Vec::with_capacity(self.items.len());
            for name in &self.items {
                items.push(Item::parse(sch, name).ok_or_else(|| {
                    CommandError::not_found(format!("no item '{}'", crate::paths::display_name(name)))
                })?);
            }
            let n = sch.delete(&items, &limits()).map_err(sch_err)?;
            // A sheet file nobody uses any more is dropped from the design (undo brings it back).
            if items.iter().any(|x| matches!(x, Item::Sheet(_))) {
                let file = self.sheet.clone().unwrap_or_else(|| open.file().to_string());
                files.drop = unreachable_files(open, &file, sch);
            }
            Ok(json!({ "deleted": n }))
        })
    }
}

/// Whether the sheet file `from` uses `target` (directly or through other sheets).
fn reaches(open: &OpenSchematic, from: &str, target: &str) -> bool {
    let mut seen = vec![from.to_string()];
    let mut k = 0;
    while let Some(f) = seen.get(k).cloned() {
        k += 1;
        if f == target {
            return true;
        }
        for sh in open.design.sheets.iter().find(|x| x.0 == f).map(|x| x.1.sheets.as_slice()).unwrap_or_default() {
            if !seen.contains(&sh.file) && seen.len() < 100_000 {
                seen.push(sh.file.clone());
            }
        }
    }
    false
}

/// Places a hierarchical sheet.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchAddSheet {
    /// Shown name.
    pub name: String,
    /// Sub-sheet file (.kicad_sch beside the root); made if the design lacks it, written by sch_save.
    pub file: String,
    /// Top left, mm.
    pub at: [f64; 2],
    /// Width and height, mm (default 30 by 20).
    #[serde(default)]
    pub size: Option<[f64; 2]>,
    /// Sheet to place it on (default: root).
    #[serde(default)]
    pub sheet: Option<String>,
}

impl Command for SchAddSheet {
    const ID: &'static str = "sch.add_sheet";
    const TITLE: &'static str = "Add sheet";
    const DESCRIPTION: &'static str =
        "Place a hierarchical sheet on the open schematic and start its empty sub-sheet file.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let mut file = self.file.trim().to_string();
        if !file.to_ascii_lowercase().ends_with(".kicad_sch") {
            file.push_str(".kicad_sch");
        }
        if !safe_sheet_name(&file) {
            return Err(CommandError::args(
                "the sheet file is a relative .kicad_sch name without '..' (beside the root file)",
            ));
        }
        let label = format!("Add sheet {}", crate::paths::display_name(&self.name));
        let size = self.size.unwrap_or([30.0, 20.0]);
        let on = self.sheet.clone();
        sheet_edit_ex(s, ctx, &label, on.as_deref(), |sch, open, files| {
            let here = on.clone().unwrap_or_else(|| open.file().to_string());
            if file == open.file() || file == here || reaches(open, &file, &here) {
                return Err(CommandError::args("a sheet cannot contain itself").hint("name another sub-sheet file"));
            }
            let known = open.design.sheet_named(&file).is_some();
            if !known {
                files.add.push((file.clone(), Schematic::new_empty(&file).map_err(sch_err)?));
            }
            // The path of the sheet this one goes on, from its first use.
            let parent_path = match open.sheet(on.as_deref()) {
                Some((0, root)) => format!("/{}", root.uuid.as_deref().unwrap_or("")),
                Some((i, _)) => {
                    open.netlist.uses.iter().find(|u| u.sheet == i).map_or_else(|| "/".to_string(), |u| u.path.clone())
                }
                None => "/".to_string(),
            };
            let project = open
                .design
                .sheets
                .iter()
                .find_map(|(_, x)| x.project_name())
                .unwrap_or_else(|| open.file().strip_suffix(".kicad_sch").unwrap_or(open.file()).to_string());
            let page = (open.netlist.uses.len() + 1).to_string();
            let i = sch
                .add_sheet(&self.name, &file, self.at, size, &project, &parent_path, &page, &limits())
                .map_err(sch_err)?;
            Ok(json!({ "sheet": i, "file_user_text": file, "new_file": !known }))
        })
    }
}

/// Adds, renames or removes a pin on a sheet.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchSheetPin {
    /// sheet#i or the sheet's name.
    pub item: String,
    /// add, rename or remove.
    pub op: String,
    /// The pin (joins the hierarchical label of that name inside).
    pub name: String,
    /// New name (rename).
    #[serde(default)]
    pub new_name: Option<String>,
    /// input/output/bidirectional/tri_state/passive (add; default bidirectional).
    #[serde(default)]
    pub kind: Option<String>,
    /// left/right/top/bottom (add; default left).
    #[serde(default)]
    pub side: Option<String>,
    /// mm along the edge (add; default: next free step).
    #[serde(default)]
    pub offset: Option<f64>,
    /// Sheet the sheet item is on (default: root).
    #[serde(default)]
    pub sheet: Option<String>,
}

impl Command for SchSheetPin {
    const ID: &'static str = "sch.sheet_pin";
    const TITLE: &'static str = "Sheet pin";
    const DESCRIPTION: &'static str = "Add, rename or remove a pin on a hierarchical sheet of the open schematic.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let side = match self.side.as_deref() {
            None => gusset_pcb::sch_sheet::Side::Left,
            Some(x) => gusset_pcb::sch_sheet::Side::parse(x)
                .ok_or_else(|| CommandError::args("side is left, right, top or bottom"))?,
        };
        if !matches!(self.op.as_str(), "add" | "rename" | "remove") {
            return Err(CommandError::args("op is add, rename or remove"));
        }
        let label = format!("Sheet pin {} {}", self.op, crate::paths::display_name(&self.name));
        sheet_edit(s, ctx, &label, self.sheet.as_deref(), |sch, _| {
            let i = match Item::parse(sch, &self.item) {
                Some(Item::Sheet(i)) => i,
                Some(_) => return Err(CommandError::args("item names a sheet (sheet#i or its name)")),
                None => {
                    let mut named = sch.sheets.iter().enumerate().filter(|(_, x)| x.name == self.item);
                    match (named.next(), named.next()) {
                        (Some((i, _)), None) => i,
                        (Some(_), Some(_)) => {
                            return Err(CommandError::args("two sheets have that name; use sheet#i"));
                        }
                        _ => {
                            return Err(CommandError::not_found(format!(
                                "no sheet '{}'",
                                crate::paths::display_name(&self.item)
                            )));
                        }
                    }
                }
            };
            match self.op.as_str() {
                "add" => {
                    let kind = self.kind.as_deref().unwrap_or("bidirectional");
                    let p = sch.add_sheet_pin(i, &self.name, kind, side, self.offset, &limits()).map_err(sch_err)?;
                    let at = sch.sheets.get(i).and_then(|x| x.pins.get(p)).map(|x| x.at);
                    Ok(json!({ "sheet": i, "pin": p, "at": at }))
                }
                "rename" => {
                    let to = self.new_name.as_deref().ok_or_else(|| CommandError::args("rename needs new_name"))?;
                    sch.rename_sheet_pin(i, &self.name, to, &limits()).map_err(sch_err)?;
                    Ok(json!({ "sheet": i, "renamed": true }))
                }
                _ => {
                    sch.remove_sheet_pin(i, &self.name, &limits()).map_err(sch_err)?;
                    Ok(json!({ "sheet": i, "removed": true }))
                }
            }
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
        "Check the open schematic: unconnected and conflicting pins, undriven power, labels, references.";
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
    /// Folder of Lib.pretty libraries.
    #[serde(default)]
    pub footprints: Option<String>,
    /// Report only.
    #[serde(default)]
    pub dry_run: bool,
}

impl Command for PcbUpdateFromSchematic {
    const ID: &'static str = "pcb.update_from_schematic";
    const TITLE: &'static str = "Update PCB from schematic";
    const DESCRIPTION: &'static str =
        "Set pad nets from the open schematic, add missing footprints from a folder, report differences.";
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
