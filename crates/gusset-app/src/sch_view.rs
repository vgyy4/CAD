//! The Schematic tab's editor (ADR 0047 phase 2a): a sheet of the open schematic drawn
//! in KiCad-like colours on a light canvas, panned and zoomed like the board editor
//! (wheel zooms about the pointer, middle drag pans, double-click fits).
//!
//! Tools: Select (click picks a symbol, wire, label, junction or no-connect flag; Shift or
//! Ctrl adds to or removes from the picks; a drag on empty paper draws a selection box,
//! left to right picking what lies fully inside and right to left what it touches;
//! dragging a picked item moves all picks as one step; R turns, X and Y mirror one item,
//! Delete removes every pick as one step), Wire
//! (clicks add corners, a double click, right click or Esc ends the wire), Label (click
//! places the label text from the panel) and Place (click places the panel's symbol from
//! its library file). ERC markers show where `sch.erc` found problems on this sheet.
//! Selecting a symbol names its reference for cross-probing with the board tab.
//!
//! The widget never edits the schematic: it reports registry commands ([`Out::commands`]),
//! which the app sends like any other command (ADR 0012), so every edit is undoable.

use crate::i18n::tr;
use crate::pcb_view::{Map, snap};
use crate::theme::Palette;
use egui::{Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use gusset_commands::sch_session::OpenSchematic;
use gusset_pcb::board::Pt;
use gusset_pcb::sch::{Fill, LabelKind, LibSymbol, Mirror, Schematic, Shape as SShape, Symbol};
use gusset_pcb::sch_edit::Item;
use serde_json::{Value, json};

/// What clicks do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Wire,
    Label,
    Place,
}

/// Grid choices (mm): KiCad's 50 mil grid and its halves.
pub const GRIDS: [f64; 4] = [0.635, 1.27, 2.54, 5.08];

/// An ERC marker on the shown sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub at: Pt,
    pub text: String,
    pub error: bool,
}

/// Editor state kept across frames.
#[derive(Debug)]
pub struct State {
    zoom: f32,
    offset: Vec2,
    /// The view still shows the fit and the canvas had this size then: a canvas that
    /// changes size (the panels settle in the first frames) fits again; a pan or zoom by
    /// the user ends this.
    auto_fit: Option<Vec2>,
    /// The sheet the view was fitted for.
    fitted: Option<String>,
    /// The shown sheet's file (`None`: the root).
    pub sheet: Option<String>,
    pub tool: Tool,
    pub grid: f64,
    pub selected: Option<Item>,
    /// Picks beyond `selected` (Shift-click, box select); dropped when `selected` is
    /// changed from outside (`extra_owner` differs).
    pub extra: Vec<Item>,
    extra_owner: Option<Item>,
    drag: Option<Drag>,
    /// Corners of the wire being drawn.
    pub wire: Vec<Pt>,
    /// Text and kind of the next label.
    pub label_text: String,
    pub label_kind: LabelKind,
    /// The symbol library file and symbol name the Place tool puts down.
    pub library: String,
    pub symbol: String,
    pub show_markers: bool,
    /// The user named a `kicad-cli` (ADR 0056): the KiCad ERC button is offered.
    pub kicad_ready: bool,
    /// The last `sch.kicad_erc` findings, one line each.
    pub kicad_issues: Vec<String>,
    /// The reference of the symbol last picked (for cross-probing).
    pub probe: Option<String>,
    /// A net drawn highlighted (from a picked wire or label, or the board).
    pub highlight: Option<String>,
    /// The picked symbol's fields being edited: (symbol, value, footprint).
    pub fields: Option<(usize, String, String)>,
}

impl Default for State {
    fn default() -> Self {
        State {
            zoom: 0.0,
            offset: Vec2::ZERO,
            auto_fit: None,
            fitted: None,
            sheet: None,
            tool: Tool::Select,
            grid: 1.27,
            selected: None,
            extra: Vec::new(),
            extra_owner: None,
            drag: None,
            wire: Vec::new(),
            label_text: "NET".into(),
            label_kind: LabelKind::Local,
            library: String::new(),
            symbol: String::new(),
            show_markers: true,
            kicad_ready: false,
            kicad_issues: Vec::new(),
            probe: None,
            highlight: None,
            fields: None,
        }
    }
}

#[derive(Debug, Clone)]
struct Drag {
    /// The pick being moved (with the other picks), or `None` while panning or boxing.
    item: Option<Item>,
    moved: Vec2,
    /// A selection box from its start to the pointer, in sheet mm.
    boxed: Option<[Pt; 2]>,
}

impl State {
    /// Every picked item: `selected` first, then the others.
    pub fn picked(&self) -> Vec<Item> {
        let mut v: Vec<Item> = self.selected.into_iter().collect();
        for i in &self.extra {
            if !v.contains(i) {
                v.push(*i);
            }
        }
        v
    }

    /// Picks exactly these items.
    pub fn set_picked(&mut self, items: &[Item]) {
        self.selected = items.first().copied();
        self.extra = items.iter().skip(1).copied().collect();
        self.extra_owner = self.selected;
    }
}

/// What a selection box takes: dragged left to right (`b` right of `a`), the items that lie
/// fully inside; right to left, every item it touches. Symbols count by their body box,
/// wires by their segment, labels, junctions and no-connects by their anchor.
pub fn box_select(sch: &Schematic, a: Pt, b: Pt) -> Vec<Item> {
    let window = b[0] >= a[0];
    let (lo, hi) = ([a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])]);
    let inside = |p: Pt| p[0] >= lo[0] && p[0] <= hi[0] && p[1] >= lo[1] && p[1] <= hi[1];
    let rects = |r: [Pt; 2]| r[0][0] <= hi[0] && r[1][0] >= lo[0] && r[0][1] <= hi[1] && r[1][1] >= lo[1];
    let seg = |p: Pt, q: Pt| {
        if window {
            return inside(p) && inside(q);
        }
        if inside(p) || inside(q) {
            return true;
        }
        // Crosses the box without an end inside: against its four sides.
        let corners = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]];
        let next = [corners[1], corners[2], corners[3], corners[0]];
        corners.iter().zip(next).any(|(c, d)| segments_cross(p, q, *c, d))
    };
    let mut out = Vec::new();
    for (i, s) in sch.symbols.iter().enumerate() {
        let r = symbol_box(sch, s);
        let hit = if window { inside(r[0]) && inside(r[1]) } else { rects(r) };
        if hit {
            out.push(Item::Symbol(i));
        }
    }
    out.extend(sch.wires.iter().enumerate().filter(|(_, w)| seg(w.a, w.b)).map(|(i, _)| Item::Wire(i)));
    out.extend(sch.buses.iter().enumerate().filter(|(_, w)| seg(w.a, w.b)).map(|(i, _)| Item::Bus(i)));
    out.extend(sch.labels.iter().enumerate().filter(|(_, l)| inside(l.at)).map(|(i, _)| Item::Label(i)));
    out.extend(sch.junctions.iter().enumerate().filter(|(_, j)| inside(j.at)).map(|(i, _)| Item::Junction(i)));
    out.extend(sch.no_connects.iter().enumerate().filter(|(_, j)| inside(j.at)).map(|(i, _)| Item::NoConnect(i)));
    out
}

/// Whether segments `p`-`q` and `c`-`d` cross (touching counts).
fn segments_cross(p: Pt, q: Pt, c: Pt, d: Pt) -> bool {
    let cross = |o: Pt, a: Pt, b: Pt| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let (d1, d2, d3, d4) = (cross(c, d, p), cross(c, d, q), cross(p, q, c), cross(p, q, d));
    d1 * d2 <= 0.0 && d3 * d4 <= 0.0
}

/// The picks after a click on `hit`: with `add` (Shift or Ctrl) it joins them or, when
/// already picked, leaves them; without, it is the only pick.
pub fn click_picks(picked: &[Item], hit: Option<Item>, add: bool) -> Vec<Item> {
    match (hit, add) {
        (None, true) => picked.to_vec(),
        (None, false) => Vec::new(),
        (Some(h), false) => vec![h],
        (Some(h), true) => {
            let mut v = picked.to_vec();
            match v.iter().position(|x| *x == h) {
                Some(k) => {
                    v.remove(k);
                }
                None => v.push(h),
            }
            v
        }
    }
}

/// The picks after a box: replaced by the box's items, or (`add`) joined by them.
pub fn box_picks(picked: &[Item], boxed: Vec<Item>, add: bool) -> Vec<Item> {
    if !add {
        return boxed;
    }
    let mut v = picked.to_vec();
    for i in boxed {
        if !v.contains(&i) {
            v.push(i);
        }
    }
    v
}

/// The shift (mm) a dragged pick makes: its drop place on the grid when it is a symbol or
/// label, else the raw drag snapped to the grid.
pub fn drag_delta(sch: &Schematic, item: Item, moved: Pt, grid: f64) -> Pt {
    let at = match item {
        Item::Symbol(i) => sch.symbols.get(i).map(|s| s.at),
        Item::Label(i) => sch.labels.get(i).map(|l| l.at),
        _ => None,
    };
    match (at, drop_place(sch, item, moved, grid)) {
        (Some(a), Some(b)) => [((b[0] - a[0]) * 1e6).round() / 1e6, ((b[1] - a[1]) * 1e6).round() / 1e6],
        _ => snap(moved, grid),
    }
}

/// One `sch.move` shifting every pick by `delta` (the dragged one named first), or
/// `None` for no shift.
pub fn group_move_command(st: &State, primary: Item, picks: &[Item], delta: Pt) -> Option<(&'static str, Value)> {
    if delta[0].abs() < 1e-9 && delta[1].abs() < 1e-9 {
        return None;
    }
    let with: Vec<String> = picks.iter().filter(|i| **i != primary).map(|i| i.name()).collect();
    Some(("sch.move", with_sheet(st, json!({ "item": primary.name(), "by": delta, "with": with }))))
}

/// One `sch.delete` for every pick.
pub fn delete_picks_command(st: &State, picks: &[Item]) -> (&'static str, Value) {
    let items: Vec<String> = picks.iter().map(|i| i.name()).collect();
    ("sch.delete", with_sheet(st, json!({ "items": items })))
}

/// What the user asked for: registry commands with their arguments, in order.
#[derive(Debug, Default, PartialEq)]
pub struct Out {
    pub commands: Vec<(&'static str, Value)>,
}

const PAPER: Color32 = Color32::from_rgb(245, 244, 239);
const BODY: Color32 = Color32::from_rgb(132, 0, 0);
const BODY_FILL: Color32 = Color32::from_rgb(255, 255, 194);
const PIN: Color32 = Color32::from_rgb(132, 0, 0);
const PIN_TEXT: Color32 = Color32::from_rgb(0, 100, 100);
const WIRE: Color32 = Color32::from_rgb(0, 132, 0);
const BUS: Color32 = Color32::from_rgb(0, 0, 132);
const LABEL: Color32 = Color32::from_rgb(15, 15, 15);
const FIELD: Color32 = Color32::from_rgb(0, 100, 100);
const SHEET: Color32 = Color32::from_rgb(132, 0, 132);
const SELECT: Color32 = Color32::from_rgb(255, 140, 0);

/// The sheet shown: the chosen file, else the root.
pub fn shown_sheet<'a>(open: &'a OpenSchematic, st: &State) -> Option<(usize, &'a Schematic)> {
    open.sheet(st.sheet.as_deref()).or_else(|| open.sheet(None))
}

/// The extent of a placed symbol: its drawings and pins.
pub fn symbol_box(sch: &Schematic, s: &Symbol) -> [Pt; 2] {
    let (mut lo, mut hi) = ([s.at[0], s.at[1]], [s.at[0], s.at[1]]);
    let mut add = |p: Pt| {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    };
    if let Some(l) = sch.lib_symbol(s) {
        for u in l.units_of(s.unit, s.style) {
            for g in &u.graphics {
                match &g.shape {
                    SShape::Rect { a, b } => {
                        add(s.place(*a));
                        add(s.place(*b));
                    }
                    SShape::Poly { pts } => pts.iter().for_each(|p| add(s.place(*p))),
                    SShape::Circle { center, radius } => {
                        add(s.place([center[0] - radius, center[1] - radius]));
                        add(s.place([center[0] + radius, center[1] + radius]));
                    }
                    SShape::Arc { start, mid, end } => [start, mid, end].into_iter().for_each(|p| add(s.place(*p))),
                    SShape::Text { .. } => {}
                }
            }
            for p in &u.pins {
                add(s.place(p.at));
            }
        }
    }
    [lo, hi]
}

fn dist(a: Pt, b: Pt) -> f64 {
    libm::hypot(a[0] - b[0], a[1] - b[1])
}

/// What is under sheet point `p` (`tol` mm of slack): junctions, no-connects and
/// labels first, then wires, then the smallest symbol around it.
pub fn hit(sch: &Schematic, p: Pt, tol: f64) -> Option<Item> {
    if let Some(i) = sch.junctions.iter().position(|j| dist(j.at, p) <= tol.max(0.5)) {
        return Some(Item::Junction(i));
    }
    if let Some(i) = sch.no_connects.iter().position(|j| dist(j.at, p) <= tol.max(0.8)) {
        return Some(Item::NoConnect(i));
    }
    if let Some(i) = sch.labels.iter().position(|l| dist(l.at, p) <= tol.max(1.0)) {
        return Some(Item::Label(i));
    }
    let near = |w: &gusset_pcb::sch::Wire| gusset_pcb::netlist::point_segment(p, w.a, w.b);
    if let Some((i, _)) = sch
        .wires
        .iter()
        .enumerate()
        .map(|(i, w)| (i, near(w)))
        .filter(|x| x.1 <= tol)
        .min_by(|a, b| a.1.total_cmp(&b.1))
    {
        return Some(Item::Wire(i));
    }
    if let Some((i, _)) = sch
        .buses
        .iter()
        .enumerate()
        .map(|(i, w)| (i, near(w)))
        .filter(|x| x.1 <= tol)
        .min_by(|a, b| a.1.total_cmp(&b.1))
    {
        return Some(Item::Bus(i));
    }
    sch.symbols
        .iter()
        .enumerate()
        .map(|(i, s)| (i, symbol_box(sch, s)))
        .filter(|(_, b)| {
            p[0] >= b[0][0] - tol && p[0] <= b[1][0] + tol && p[1] >= b[0][1] - tol && p[1] <= b[1][1] + tol
        })
        .min_by(|a, b| {
            let area = |x: &[Pt; 2]| (x[1][0] - x[0][0]) * (x[1][1] - x[0][1]);
            area(&a.1).total_cmp(&area(&b.1))
        })
        .map(|(i, _)| Item::Symbol(i))
}

/// The nearest connection point (pin end, wire end, label) within `tol`, else the grid
/// point: where a wire corner goes.
pub fn wire_snap(sch: &Schematic, p: Pt, tol: f64, grid: f64) -> Pt {
    let mut best: Option<(f64, Pt)> = None;
    let mut offer = |q: Pt| {
        let d = dist(p, q);
        if d <= tol && best.is_none_or(|b| d < b.0) {
            best = Some((d, q));
        }
    };
    for s in &sch.symbols {
        for pin in sch.pins(s) {
            offer(pin.at);
        }
    }
    for w in &sch.wires {
        offer(w.a);
        offer(w.b);
    }
    for l in &sch.labels {
        offer(l.at);
    }
    best.map_or_else(|| snap(p, grid), |b| b.1)
}

/// The `sheet` argument of a command: the shown sub-sheet, nothing for the root.
fn with_sheet(st: &State, mut args: Value) -> Value {
    if let (Some(f), Value::Object(m)) = (&st.sheet, &mut args) {
        m.insert("sheet".into(), json!(f));
    }
    args
}

/// Where a dragged symbol or label lands.
pub fn drop_place(sch: &Schematic, item: Item, moved: Pt, grid: f64) -> Option<Pt> {
    let at = match item {
        Item::Symbol(i) => sch.symbols.get(i)?.at,
        Item::Label(i) => sch.labels.get(i)?.at,
        _ => return None,
    };
    Some(snap([at[0] + moved[0], at[1] + moved[1]], grid))
}

pub fn move_command(st: &State, item: Item, at: Pt) -> (&'static str, Value) {
    ("sch.move", with_sheet(st, json!({ "item": item.name(), "at": at })))
}

/// R: a quarter turn counter-clockwise.
pub fn rotate_command(st: &State, sch: &Schematic, item: Item) -> Option<(&'static str, Value)> {
    let angle = match item {
        Item::Symbol(i) => sch.symbols.get(i)?.angle,
        Item::Label(i) => sch.labels.get(i)?.angle,
        _ => return None,
    };
    Some(("sch.move", with_sheet(st, json!({ "item": item.name(), "angle": (angle + 90.0).rem_euclid(360.0) }))))
}

/// X or Y: mirror a symbol (again: back to unmirrored).
pub fn mirror_command(st: &State, sch: &Schematic, item: Item, axis: Mirror) -> Option<(&'static str, Value)> {
    let Item::Symbol(i) = item else { return None };
    let cur = sch.symbols.get(i)?.mirror;
    let m = match (cur == axis, axis) {
        (true, _) | (_, Mirror::None) => "none",
        (false, Mirror::X) => "x",
        (false, Mirror::Y) => "y",
    };
    Some(("sch.move", with_sheet(st, json!({ "item": item.name(), "mirror": m }))))
}

pub fn delete_command(st: &State, item: Item) -> (&'static str, Value) {
    ("sch.delete", with_sheet(st, json!({ "items": [item.name()] })))
}

pub fn wire_command(st: &State, pts: &[Pt]) -> Option<(&'static str, Value)> {
    (pts.len() >= 2).then(|| ("sch.add_wire", with_sheet(st, json!({ "points": pts }))))
}

pub fn label_command(st: &State, at: Pt) -> Option<(&'static str, Value)> {
    let text = st.label_text.trim();
    if text.is_empty() {
        return None;
    }
    let kind = match st.label_kind {
        LabelKind::Local => "local",
        LabelKind::Global => "global",
        LabelKind::Hierarchical => "hierarchical",
    };
    Some(("sch.add_label", with_sheet(st, json!({ "text": text, "at": at, "kind": kind }))))
}

pub fn place_command(st: &State, at: Pt) -> Option<(&'static str, Value)> {
    if st.library.trim().is_empty() || st.symbol.trim().is_empty() {
        return None;
    }
    Some((
        "sch.add_symbol",
        with_sheet(st, json!({ "library": st.library.trim(), "symbol": st.symbol.trim(), "at": at })),
    ))
}

/// ERC markers on the shown sheet: issues of the first use of that sheet file.
pub fn markers(open: &OpenSchematic, st: &State) -> Vec<Marker> {
    let Some((sheet, _)) = shown_sheet(open, st) else { return Vec::new() };
    let Some(u) = open.netlist.uses.iter().position(|x| x.sheet == sheet) else { return Vec::new() };
    open.erc
        .iter()
        .filter(|i| i.sheet_use == u)
        .map(|i| Marker {
            at: i.at,
            text: i.message.chars().filter(|c| !c.is_control()).take(200).collect(),
            error: i.severity == gusset_pcb::erc::Severity::Error,
        })
        .collect()
}

/// The net of a picked wire or label on the shown sheet (its first use), by name.
pub fn net_of(open: &OpenSchematic, sheet: usize, sch: &Schematic, item: Item) -> Option<String> {
    let u = open.netlist.uses.iter().position(|x| x.sheet == sheet)?;
    let net = match item {
        Item::Wire(k) => open.netlist.wire_nets.get(u)?.get(k).copied().flatten()?,
        Item::Label(k) => {
            let at = sch.labels.get(k)?.at;
            open.netlist.nets.iter().position(|n| {
                n.marks.iter().any(|(mu, p, _)| *mu == u && (p[0] - at[0]).abs() < 1e-6 && (p[1] - at[1]).abs() < 1e-6)
            })?
        }
        _ => return None,
    };
    open.netlist.nets.get(net).map(|n| n.name.clone())
}

/// The commands that set a symbol's changed value and footprint.
pub fn field_commands(st: &State, symbol: usize, old: (&str, &str), new: (&str, &str)) -> Vec<(&'static str, Value)> {
    let mut out = Vec::new();
    for (field, was, now) in [("Value", old.0, new.0), ("Footprint", old.1, new.1)] {
        if was != now.trim() {
            out.push((
                "sch.set_field",
                with_sheet(st, json!({ "symbol": format!("symbol#{symbol}"), "field": field, "value": now.trim() })),
            ));
        }
    }
    out
}

/// The symbol of a reference on the shown sheet (cross-probing from the board).
pub fn symbol_of(sch: &Schematic, reference: &str) -> Option<Item> {
    sch.symbols.iter().position(|s| s.reference() == reference).map(Item::Symbol)
}

impl State {
    fn fit(&mut self, rect: Rect, sch: &Schematic) {
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        let mut add = |p: Pt| {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        };
        for s in &sch.symbols {
            let b = symbol_box(sch, s);
            add(b[0]);
            add(b[1]);
        }
        sch.wires.iter().chain(&sch.buses).for_each(|w| {
            add(w.a);
            add(w.b);
        });
        sch.labels.iter().for_each(|l| add(l.at));
        for s in &sch.sheets {
            add(s.at);
            add([s.at[0] + s.size[0], s.at[1] + s.size[1]]);
        }
        if lo[0] > hi[0] {
            // An empty sheet: A4 landscape.
            (lo, hi) = ([0.0, 0.0], [297.0, 210.0]);
        }
        let (w, h) = (((hi[0] - lo[0]) as f32).max(10.0), ((hi[1] - lo[1]) as f32).max(10.0));
        self.zoom = ((rect.width() - 40.0) / w).min((rect.height() - 40.0) / h).clamp(0.2, 400.0).min(24.0);
        let c = Vec2::new((lo[0] + hi[0]) as f32 / 2.0, (lo[1] + hi[1]) as f32 / 2.0);
        self.offset = rect.size() / 2.0 - c * self.zoom;
        self.auto_fit = Some(rect.size());
    }
}

/// The lines for a `sch.kicad_erc` result: severity, sheet and KiCad's message.
pub fn kicad_issues_from(result: &Value) -> Vec<String> {
    result
        .get("issues")
        .and_then(Value::as_array)
        .map(|v| {
            v.iter()
                .map(|x| {
                    let s = |k: &str| x.get(k).and_then(Value::as_str).unwrap_or_default();
                    let line = format!("{} {}: {}", s("severity"), s("sheet_user_text"), s("message_user_text"));
                    line.chars().filter(|c| !c.is_control()).take(240).collect()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The side panel: sheet, tools, grid, label and symbol to place, ERC summary.
pub fn controls(ui: &mut Ui, pal: &Palette, open: Option<&OpenSchematic>, st: &mut State) -> Out {
    let mut out = Out::default();
    let Some(open) = open else {
        // The canvas carries the explanation; the panel only offers the way out.
        ui.set_min_width(200.0);
        if ui.button(tr("sch.new")).clicked() {
            out.commands.push(("sch.new", json!({})));
        }
        return out;
    };
    ui.strong(open.file());
    if open.design.sheets.len() > 1 {
        let shown = st.sheet.clone().unwrap_or_else(|| open.file().to_string());
        egui::ComboBox::from_id_salt("sch_sheet").selected_text(shown).show_ui(ui, |ui| {
            for (k, (f, _)) in open.design.sheets.iter().enumerate() {
                let v = (k > 0).then(|| f.clone());
                if ui.selectable_label(st.sheet == v, f.as_str()).clicked() {
                    st.sheet = v;
                    st.selected = None;
                    st.wire.clear();
                }
            }
        });
    }
    if let Some((_, s)) = shown_sheet(open, st) {
        ui.weak(format!(
            "{} {} · {} {} · {} {}",
            s.symbols.len(),
            tr("sch.symbols"),
            s.wires.len(),
            tr("sch.wires"),
            open.netlist.nets.len(),
            tr("sch.nets")
        ));
    }
    let n = open.erc.len();
    ui.colored_label(
        if n == 0 { pal.success } else { pal.warning },
        if n == 0 { tr("sch.erc_clean").to_string() } else { format!("{}: {n}", tr("sch.erc_issues")) },
    );
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        for (t, key, tip) in [
            (Tool::Select, "pcb.select", "sch.tip_select"),
            (Tool::Wire, "sch.wire", "sch.tip_wire"),
            (Tool::Label, "sch.label", "sch.tip_label"),
            (Tool::Place, "sch.place", "sch.tip_place"),
        ] {
            if ui.selectable_label(st.tool == t, tr(key)).on_hover_text(tr(tip)).clicked() {
                st.tool = t;
                st.wire.clear();
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(tr("pcb.grid"));
        egui::ComboBox::from_id_salt("sch_grid").selected_text(format!("{} mm", st.grid)).show_ui(ui, |ui| {
            for g in GRIDS {
                ui.selectable_value(&mut st.grid, g, format!("{g} mm"));
            }
        });
    });
    // The picked symbol's fields.
    if let (Some(Item::Symbol(i)), Some((_, sch))) = (st.selected, shown_sheet(open, st))
        && let Some(sym) = sch.symbols.get(i)
    {
        if st.fields.as_ref().is_none_or(|f| f.0 != i) {
            st.fields = Some((i, sym.value().to_string(), sym.footprint().to_string()));
        }
        ui.separator();
        ui.strong(sym.reference());
        let mut apply = false;
        if let Some((_, value, fp)) = st.fields.as_mut() {
            ui.horizontal(|ui| {
                ui.label(tr("sch.value"));
                apply |= crate::theme::text_field(ui, egui::TextEdit::singleline(value)).lost_focus()
                    && ui.input(|x| x.key_pressed(egui::Key::Enter));
            });
            ui.horizontal(|ui| {
                ui.label(tr("sch.footprint"));
                apply |= crate::theme::text_field(ui, egui::TextEdit::singleline(fp)).lost_focus()
                    && ui.input(|x| x.key_pressed(egui::Key::Enter));
            });
        }
        if (ui.small_button(tr("sch.apply")).clicked() || apply)
            && let Some((_, v, f)) = st.fields.clone()
        {
            out.commands.extend(field_commands(st, i, (sym.value(), sym.footprint()), (&v, &f)));
        }
    } else {
        st.fields = None;
    }
    ui.separator();
    ui.label(tr("sch.label"));
    crate::theme::text_field(
        ui,
        egui::TextEdit::singleline(&mut st.label_text).hint_text(tr("sch.label_hint")).desired_width(f32::INFINITY),
    );
    ui.horizontal(|ui| {
        for (k, key) in
            [(LabelKind::Local, "sch.local"), (LabelKind::Global, "sch.global"), (LabelKind::Hierarchical, "sch.hier")]
        {
            ui.selectable_value(&mut st.label_kind, k, tr(key));
        }
    });
    ui.separator();
    ui.label(tr("sch.library"));
    crate::theme::text_field(
        ui,
        egui::TextEdit::singleline(&mut st.library).hint_text(tr("sch.library_hint")).desired_width(f32::INFINITY),
    );
    ui.label(tr("sch.symbol"));
    crate::theme::text_field(
        ui,
        egui::TextEdit::singleline(&mut st.symbol).hint_text(tr("sch.symbol_hint")).desired_width(f32::INFINITY),
    );
    ui.separator();
    ui.checkbox(&mut st.show_markers, tr("sch.markers"));
    if ui.button(tr("sch.run_erc")).clicked() {
        out.commands.push(("sch.erc", json!({})));
    }
    if st.kicad_ready && ui.button(tr("sch.kicad_erc")).on_hover_text(tr("pcb.kicad_hint")).clicked() {
        out.commands.push(("sch.kicad_erc", json!({})));
    }
    if !st.kicad_issues.is_empty() {
        ui.colored_label(pal.danger, format!("{}: {}", tr("sch.kicad_issues"), st.kicad_issues.len()));
        egui::ScrollArea::vertical().id_salt("sch_kicad_issues").max_height(160.0).show(ui, |ui| {
            for t in st.kicad_issues.iter().take(200) {
                ui.label(egui::RichText::new(t).small());
            }
        });
        if ui.small_button(tr("pcb.clear_markers")).clicked() {
            st.kicad_issues.clear();
        }
    }
    if st.show_markers {
        let m = markers(open, st);
        egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
            for x in m.iter().take(200) {
                ui.label(egui::RichText::new(&x.text).small());
            }
        });
    }
    out
}

/// The sheet tree in the left panel (instead of the solid-model browser): symbols, labels,
/// sheets and nets. A click on a symbol or label picks it, on a net highlights it.
pub fn tree(ui: &mut Ui, open: Option<&OpenSchematic>, st: &mut State) {
    let Some(open) = open else {
        ui.weak(tr("tree.none_sch"));
        return;
    };
    let Some((_, sch)) = shown_sheet(open, st) else { return };
    egui::CollapsingHeader::new(format!("{} ({})", tr("tree.symbols"), sch.symbols.len()))
        .id_salt("tree_sym")
        .default_open(true)
        .show(ui, |ui| {
            for (i, s) in sch.symbols.iter().enumerate() {
                let label = format!("{}  {}", s.reference(), s.value());
                if ui.selectable_label(st.selected == Some(Item::Symbol(i)), label).clicked() {
                    st.selected = Some(Item::Symbol(i));
                }
            }
        });
    egui::CollapsingHeader::new(format!("{} ({})", tr("tree.labels"), sch.labels.len()))
        .id_salt("tree_lbl")
        .default_open(true)
        .show(ui, |ui| {
            for (i, l) in sch.labels.iter().enumerate().take(300) {
                if ui.selectable_label(st.selected == Some(Item::Label(i)), &l.text).clicked() {
                    st.selected = Some(Item::Label(i));
                }
            }
        });
    egui::CollapsingHeader::new(format!("{} ({})", tr("tree.sheets"), open.design.sheets.len()))
        .id_salt("tree_sheets")
        .default_open(true)
        .show(ui, |ui| {
            for (k, (f, _)) in open.design.sheets.iter().enumerate() {
                let v = (k > 0).then(|| f.clone());
                if ui.selectable_label(st.sheet == v, f.as_str()).clicked() {
                    st.sheet = v;
                    st.selected = None;
                    st.wire.clear();
                }
            }
        });
    egui::CollapsingHeader::new(format!("{} ({})", tr("tree.nets"), open.netlist.nets.len()))
        .id_salt("tree_snets")
        .default_open(false)
        .show(ui, |ui| {
            for n in open.netlist.nets.iter().take(300) {
                let on = st.highlight.as_deref() == Some(n.name.as_str());
                if ui.selectable_label(on, &n.name).clicked() {
                    st.highlight = if on { None } else { Some(n.name.clone()) };
                }
            }
        });
}

/// Draws the shown sheet and handles the editing tools.
pub fn show(ui: &mut Ui, pal: &Palette, open: Option<&OpenSchematic>, st: &mut State) -> Out {
    let mut out = Out::default();
    let rect = ui.max_rect();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, PAPER);
    let Some((sheet_index, sch)) = open.and_then(|o| shown_sheet(o, st)) else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            tr("sch.none"),
            FontId::proportional(14.0),
            // The canvas keeps its own colours in both themes, so the hint does too.
            Color32::from_gray(90),
        );
        return out;
    };
    let key = format!("{}#{sheet_index}", open.map_or("", |o| o.file()));
    if st.fitted.as_deref() != Some(key.as_str()) || st.zoom <= 0.0 {
        st.fit(rect, sch);
        st.fitted = Some(key);
        st.selected = None;
        st.wire.clear();
    }
    // A selection past the lists (after an undo) is dropped; picks set from outside
    // (the tree, the board) replace the extra ones.
    st.selected = st.selected.filter(|s| Item::parse(sch, &s.name()).is_some());
    if st.extra_owner != st.selected {
        st.extra.clear();
        st.extra_owner = st.selected;
    }
    st.extra.retain(|s| Item::parse(sch, &s.name()).is_some());
    if resp.double_clicked() && st.tool == Tool::Select || st.auto_fit.is_some_and(|s| s != rect.size()) {
        st.fit(rect, sch);
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 200.0);
        if scroll != 0.0
            && let Some(p) = resp.hover_pos()
        {
            let new = (st.zoom * (scroll * 0.002).exp()).clamp(0.2, 400.0);
            let rel = p - rect.min - st.offset;
            st.offset = p - rect.min - rel * (new / st.zoom);
            st.zoom = new;
            st.auto_fit = None;
        }
    }
    let map = Map { origin: rect.min + st.offset, zoom: st.zoom };
    let tol = f64::from(5.0 / st.zoom);
    let pointer = resp.hover_pos().map(|p| map.from(p));

    // Drags (Select tool): a pick, or a symbol or label under the pointer, moves with the
    // other picks; empty paper draws a selection box; other tools and the middle button pan.
    let middle = ui.input(|i| i.pointer.middle_down());
    let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
    if resp.drag_started() {
        let press = ui.input(|i| i.pointer.press_origin()).map(|p| map.from(p));
        let picked = st.picked();
        let under = match (st.tool, middle, press) {
            (Tool::Select, false, Some(p)) => hit(sch, p, tol),
            _ => None,
        };
        let item = under.filter(|i| matches!(i, Item::Symbol(_) | Item::Label(_)) || picked.contains(i));
        let boxed = match (st.tool, middle, press, item) {
            (Tool::Select, false, Some(p), None) => Some([p, p]),
            _ => None,
        };
        if let Some(i) = item
            && !picked.contains(&i)
        {
            st.set_picked(&[i]);
        }
        st.drag = Some(Drag { item, moved: Vec2::ZERO, boxed });
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        let here = pointer;
        match st.drag.as_mut() {
            Some(Drag { item: Some(_), moved, .. }) => *moved += d / st.zoom,
            Some(Drag { boxed: Some(b), .. }) => {
                if let Some(p) = here {
                    b[1] = p;
                }
            }
            _ => {
                st.offset += d;
                st.auto_fit = None;
            }
        }
    }
    if resp.drag_stopped() {
        match st.drag.take() {
            Some(Drag { item: Some(i), moved, .. }) if moved.length() > 1e-3 => {
                let delta = drag_delta(sch, i, [f64::from(moved.x), f64::from(moved.y)], st.grid);
                let picks = st.picked();
                if picks.len() > 1 {
                    out.commands.extend(group_move_command(st, i, &picks, delta));
                } else if let Some(at) = drop_place(sch, i, [f64::from(moved.x), f64::from(moved.y)], st.grid) {
                    out.commands.push(move_command(st, i, at));
                }
            }
            Some(Drag { boxed: Some([a, b]), .. }) if dist(a, b) * f64::from(st.zoom) > 3.0 => {
                let picks = box_picks(&st.picked(), box_select(sch, a, b), add);
                st.set_picked(&picks);
                st.highlight = None;
            }
            _ => {}
        }
    } else if !resp.dragged() {
        st.drag = None;
    }

    // Clicks.
    if resp.double_clicked() && st.tool == Tool::Wire {
        if let Some(c) = wire_command(st, &st.wire) {
            out.commands.push(c);
        }
        st.wire.clear();
    } else if resp.clicked()
        && let Some(p) = pointer
    {
        match st.tool {
            Tool::Select => {
                let picks = click_picks(&st.picked(), hit(sch, p, tol), add);
                st.set_picked(&picks);
                st.probe = match st.selected {
                    Some(Item::Symbol(i)) => sch.symbols.get(i).map(|s| s.reference().to_string()),
                    _ => None,
                };
                if let Some(o) = open {
                    st.highlight = st.selected.and_then(|it| net_of(o, sheet_index, sch, it));
                }
            }
            Tool::Wire => {
                let q = wire_snap(sch, p, tol, st.grid);
                if st.wire.last().is_none_or(|l| dist(*l, q) > 1e-6) {
                    st.wire.push(q);
                }
            }
            Tool::Label => out.commands.extend(label_command(st, snap(p, st.grid))),
            Tool::Place => out.commands.extend(place_command(st, snap(p, st.grid))),
        }
    }
    if resp.secondary_clicked() && st.tool == Tool::Wire {
        if let Some(c) = wire_command(st, &st.wire) {
            out.commands.push(c);
        }
        st.wire.clear();
    }
    // Keys while the pointer is over the sheet.
    if resp.hovered() {
        let (del, esc, r, x, y) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::R),
                i.key_pressed(egui::Key::X),
                i.key_pressed(egui::Key::Y),
            )
        });
        if esc {
            if let Some(c) = wire_command(st, &st.wire) {
                out.commands.push(c);
            }
            st.wire.clear();
            st.set_picked(&[]);
        }
        let picks = st.picked();
        if del && !picks.is_empty() {
            out.commands.push(delete_picks_command(st, &picks));
            st.set_picked(&[]);
        } else if let [item] = picks[..] {
            if r && let Some(c) = rotate_command(st, sch, item) {
                out.commands.push(c);
            }
            for (pressed, axis) in [(x, Mirror::X), (y, Mirror::Y)] {
                if pressed && let Some(c) = mirror_command(st, sch, item, axis) {
                    out.commands.push(c);
                }
            }
        }
    }

    let dragged = st.drag.as_ref().and_then(|d| {
        d.item.map(|i| {
            let delta = drag_delta(sch, i, [f64::from(d.moved.x), f64::from(d.moved.y)], st.grid);
            (st.picked(), Vec2::new(delta[0] as f32 * map.zoom, delta[1] as f32 * map.zoom))
        })
    });
    paint(&painter, &map, sch, st, dragged);
    if let Some(Drag { boxed: Some([a, b]), .. }) = &st.drag {
        // Blue and solid takes what lies inside; green and dashed what it touches.
        let r = Rect::from_two_pos(map.to(*a), map.to(*b));
        let (fill, line) = if b[0] >= a[0] {
            (Color32::from_rgba_unmultiplied(40, 90, 220, 30), Color32::from_rgb(40, 90, 220))
        } else {
            (Color32::from_rgba_unmultiplied(40, 170, 70, 30), Color32::from_rgb(40, 170, 70))
        };
        painter.rect_filled(r, 0.0, fill);
        if b[0] >= a[0] {
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, line), egui::StrokeKind::Middle);
        } else {
            let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(Shape::dashed_line(&corners, Stroke::new(1.0, line), 5.0, 3.0));
        }
    }
    if let (Some(h), Some(o)) = (&st.highlight, open)
        && let Some(u) = o.netlist.uses.iter().position(|x| x.sheet == sheet_index)
        && let Some(n) = o.netlist.nets.iter().position(|n| n.name == *h)
    {
        let glow = Color32::from_rgba_unmultiplied(255, 170, 0, 150);
        let w = (0.5 * map.zoom).max(3.0);
        let nets = o.netlist.wire_nets.get(u);
        for (k, wire) in sch.wires.iter().enumerate() {
            if nets.and_then(|v| v.get(k)).copied().flatten() == Some(n) {
                painter.line_segment([map.to(wire.a), map.to(wire.b)], Stroke::new(w, glow));
            }
        }
        if let Some(net) = o.netlist.nets.get(n) {
            for node in net.nodes.iter().filter(|x| x.sheet_use == u) {
                painter.circle_filled(map.to(node.at), w, glow);
            }
        }
    }
    if st.show_markers
        && let Some(o) = open
    {
        let ms = markers(o, st);
        for m in &ms {
            let p = map.to(m.at);
            let c = if m.error { Color32::from_rgb(230, 30, 30) } else { Color32::from_rgb(230, 150, 0) };
            painter.add(Shape::convex_polygon(
                vec![p, p + Vec2::new(5.0, -12.0), p + Vec2::new(12.0, -5.0)],
                c,
                Stroke::new(1.0, Color32::BLACK),
            ));
        }
        if let Some(pp) = resp.hover_pos()
            && let Some(m) = ms.iter().find(|m| map.to(m.at).distance(pp) < 10.0)
        {
            resp.clone().on_hover_text(m.text.clone());
        }
    }
    // Wire preview.
    if let (Tool::Wire, Some(_), Some(p)) = (st.tool, st.wire.last(), pointer) {
        let q = wire_snap(sch, p, tol, st.grid);
        let mut pts: Vec<Pos2> = st.wire.iter().map(|x| map.to(*x)).collect();
        pts.push(map.to(q));
        painter.add(Shape::line(pts, Stroke::new(1.5, WIRE.gamma_multiply(0.7))));
    }
    if let Some(p) = pointer {
        let q = snap(p, st.grid);
        painter.text(
            rect.left_bottom() + Vec2::new(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            format!("X {:.2}  Y {:.2} mm", q[0], q[1]),
            FontId::monospace(12.0),
            pal.text_muted,
        );
    }
    out
}

/// Text at a sheet place, `size` mm high, turned by `angle` (0 or 90 degrees).
#[allow(clippy::too_many_arguments)]
fn text(painter: &egui::Painter, map: &Map, at: Pos2, s: &str, size: f64, angle: f64, align: egui::Align2, c: Color32) {
    let px = size as f32 * map.zoom;
    if !(4.0..=200.0).contains(&px) || s.is_empty() {
        return;
    }
    let galley = painter.layout_no_wrap(s.to_string(), FontId::proportional(px), c);
    let vertical = (angle.rem_euclid(180.0) - 90.0).abs() < 1.0;
    let size = galley.size();
    // Anchor the galley's box by the alignment, before turning.
    let off = Vec2::new(
        match align.x() {
            egui::Align::Min => 0.0,
            egui::Align::Center => -size.x / 2.0,
            egui::Align::Max => -size.x,
        },
        match align.y() {
            egui::Align::Min => 0.0,
            egui::Align::Center => -size.y / 2.0,
            egui::Align::Max => -size.y,
        },
    );
    if vertical {
        let pos = at + Vec2::new(off.y, -off.x);
        painter.add(egui::epaint::TextShape::new(pos, galley, c).with_angle(-std::f32::consts::FRAC_PI_2));
    } else {
        painter.galley(at + off, galley, c);
    }
}

fn paint_symbol(painter: &egui::Painter, map: &Map, s: &Symbol, lib: &LibSymbol, off: Vec2, sel: bool) {
    let to = |p: Pt| map.to(s.place(p)) + off;
    let w = |width: f64| ((width.max(0.152)) as f32 * map.zoom).max(1.0);
    let body = if sel { SELECT } else { BODY };
    for u in lib.units_of(s.unit, s.style) {
        // Background fills first, then outlines.
        for g in &u.graphics {
            let fill = match g.fill {
                Fill::Background => Some(BODY_FILL),
                Fill::Outline => Some(body),
                Fill::None => None,
            };
            let stroke = Stroke::new(w(g.width), body);
            match &g.shape {
                SShape::Rect { a, b } => {
                    let r = Rect::from_two_pos(to(*a), to(*b));
                    if let Some(f) = fill {
                        painter.rect_filled(r, 0.0, f);
                    }
                    painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
                }
                SShape::Poly { pts } => {
                    let v: Vec<Pos2> = pts.iter().map(|p| to(*p)).collect();
                    let closed = pts.len() >= 3 && pts.first() == pts.last();
                    if let (Some(f), true) = (fill, v.len() >= 3) {
                        painter.add(Shape::convex_polygon(v.clone(), f, Stroke::NONE));
                    }
                    painter.add(if closed { Shape::closed_line(v, stroke) } else { Shape::line(v, stroke) });
                }
                SShape::Circle { center, radius } => {
                    let c = to(*center);
                    let r = *radius as f32 * map.zoom;
                    if let Some(f) = fill {
                        painter.circle_filled(c, r, f);
                    }
                    painter.circle_stroke(c, r, stroke);
                }
                SShape::Arc { start, mid, end } => {
                    let pts: Vec<Pos2> =
                        gusset_pcb::geom::arc_points(*start, *mid, *end, 0.05).into_iter().map(to).collect();
                    painter.add(Shape::line(pts, stroke));
                }
                SShape::Text { text: t, at, size } => {
                    text(painter, map, to(*at), t, *size, s.angle, egui::Align2::CENTER_CENTER, body);
                }
            }
        }
        for p in &u.pins {
            if p.hidden {
                continue;
            }
            let (sn, cs) = gusset_pcb::sch::sin_cos(p.angle);
            let inner = [p.at[0] + cs * p.length, p.at[1] + sn * p.length];
            let (a, b) = (to(p.at), to(inner));
            painter.line_segment([a, b], Stroke::new(w(0.152), if sel { SELECT } else { PIN }));
            if p.length <= 0.0 {
                continue;
            }
            // Number above the middle of the pin; name inside the body past its end.
            let dir = (b - a).normalized();
            let horizontal = dir.x.abs() > dir.y.abs();
            let angle = if horizontal { 0.0 } else { 90.0 };
            if !lib.hide_pin_numbers {
                // Beside the pin (above a horizontal one, left of a vertical one), clear of
                // its stroke, and no bigger than the pin is long.
                let gap = w(0.152) / 2.0 + 2.0;
                let mid = a + (b - a) / 2.0 + if horizontal { Vec2::new(0.0, -gap) } else { Vec2::new(-gap, 0.0) };
                let size = (p.length * 0.7).min(0.9);
                text(painter, map, mid, &p.number, size, angle, egui::Align2::CENTER_BOTTOM, PIN_TEXT);
            }
            if !lib.hide_pin_names && !p.name.is_empty() && p.name != "~" {
                let gap = (lib.pin_name_offset.max(0.3) as f32) * map.zoom;
                let at = b + dir * gap;
                let align = match (horizontal, dir.x > 0.0, dir.y > 0.0) {
                    (true, true, _) => egui::Align2::LEFT_CENTER,
                    (true, false, _) => egui::Align2::RIGHT_CENTER,
                    (false, _, true) => egui::Align2::LEFT_CENTER,
                    (false, _, false) => egui::Align2::RIGHT_CENTER,
                };
                text(painter, map, at, &p.name, 1.1, angle, align, PIN_TEXT);
            }
        }
    }
    // Visible fields (reference, value).
    for f in s.properties.iter().filter(|f| !f.hidden && !f.value.is_empty()) {
        if !matches!(f.key.as_str(), "Reference" | "Value") {
            continue;
        }
        let a = if (f.angle - 90.0).abs() < 1.0 || (f.angle - 270.0).abs() < 1.0 { 90.0 } else { 0.0 };
        text(painter, map, map.to(f.at) + off, &f.value, 1.27, a, egui::Align2::CENTER_CENTER, FIELD);
    }
}

/// The directions (unit vectors, y down) the wires and pins that end at `at` run away from
/// it: where a label there has things attached.
fn attach_dirs(sch: &Schematic, at: Pt) -> Vec<[f32; 2]> {
    let near = |p: Pt| (p[0] - at[0]).abs() < 1e-3 && (p[1] - at[1]).abs() < 1e-3;
    let unit = |from: Pt, to: Pt| {
        let (dx, dy) = ((to[0] - from[0]) as f32, (to[1] - from[1]) as f32);
        let n = dx.hypot(dy).max(1e-6);
        [dx / n, dy / n]
    };
    let mut out = Vec::new();
    for w in &sch.wires {
        if near(w.a) {
            out.push(unit(w.a, w.b));
        } else if near(w.b) {
            out.push(unit(w.b, w.a));
        }
    }
    for s in &sch.symbols {
        let Some(lib) = sch.lib_symbol(s) else { continue };
        for u in lib.units_of(s.unit, s.style) {
            for p in u.pins.iter().filter(|p| !p.hidden && p.length > 0.0) {
                let (sn, cs) = gusset_pcb::sch::sin_cos(p.angle);
                let (a, b) = (s.place(p.at), s.place([p.at[0] + cs * p.length, p.at[1] + sn * p.length]));
                if near(a) {
                    out.push(unit(a, b));
                }
            }
        }
    }
    out
}

/// `dragged`: the picks being dragged and how far (screen units, snapped).
fn paint(painter: &egui::Painter, map: &Map, sch: &Schematic, st: &State, dragged: Option<(Vec<Item>, Vec2)>) {
    let off_of = |item: Item| match &dragged {
        Some((items, by)) if items.contains(&item) => *by,
        _ => Vec2::ZERO,
    };
    let picked = st.picked();
    let sel = |item: Item| picked.contains(&item);
    let lw = (0.152 * map.zoom).max(1.2);
    for s in &sch.sheets {
        let r = Rect::from_two_pos(map.to(s.at), map.to([s.at[0] + s.size[0], s.at[1] + s.size[1]]));
        painter.rect_stroke(r, 0.0, Stroke::new(lw, SHEET), egui::StrokeKind::Middle);
        text(painter, map, r.left_top() + Vec2::new(0.0, -2.0), &s.name, 1.27, 0.0, egui::Align2::LEFT_BOTTOM, SHEET);
        for p in &s.pins {
            painter.circle_filled(map.to(p.at), 2.0, SHEET);
            text(painter, map, map.to(p.at), &p.name, 1.27, 0.0, egui::Align2::LEFT_CENTER, SHEET);
        }
    }
    for t in &sch.texts {
        let a = if (t.angle - 90.0).abs() < 1.0 { 90.0 } else { 0.0 };
        text(painter, map, map.to(t.at), &t.text, t.size, a, egui::Align2::LEFT_BOTTOM, LABEL);
    }
    for l in &sch.lines {
        let v: Vec<Pos2> = l.iter().map(|p| map.to(*p)).collect();
        painter.add(Shape::line(v, Stroke::new(1.0, Color32::from_rgb(70, 70, 70))));
    }
    for (i, w) in sch.buses.iter().enumerate() {
        let c = if sel(Item::Bus(i)) { SELECT } else { BUS };
        let o = off_of(Item::Bus(i));
        painter.line_segment([map.to(w.a) + o, map.to(w.b) + o], Stroke::new((0.3 * map.zoom).max(2.5), c));
    }
    for (a, size) in &sch.bus_entries {
        painter.line_segment([map.to(*a), map.to([a[0] + size[0], a[1] + size[1]])], Stroke::new(lw, WIRE));
    }
    for (i, w) in sch.wires.iter().enumerate() {
        let c = if sel(Item::Wire(i)) { SELECT } else { WIRE };
        let o = off_of(Item::Wire(i));
        painter.line_segment([map.to(w.a) + o, map.to(w.b) + o], Stroke::new(lw, c));
    }
    for (i, s) in sch.symbols.iter().enumerate() {
        let item = Item::Symbol(i);
        match sch.lib_symbol(s) {
            Some(lib) => paint_symbol(painter, map, s, lib, off_of(item), sel(item)),
            None => {
                // A symbol whose library symbol is missing: a box with its reference.
                let p = map.to(s.at) + off_of(item);
                painter.rect_stroke(
                    Rect::from_center_size(p, Vec2::splat(5.08 * map.zoom)),
                    0.0,
                    Stroke::new(1.0, Color32::RED),
                    egui::StrokeKind::Middle,
                );
                text(painter, map, p, s.reference(), 1.27, 0.0, egui::Align2::CENTER_CENTER, Color32::RED);
            }
        }
        if sel(item) {
            let b = symbol_box(sch, s);
            let r = Rect::from_two_pos(map.to(b[0]), map.to(b[1])).translate(off_of(item)).expand(3.0);
            painter.rect_stroke(r, 2.0, Stroke::new(1.0, SELECT), egui::StrokeKind::Outside);
        }
    }
    for (i, j) in sch.junctions.iter().enumerate() {
        let c = if sel(Item::Junction(i)) { SELECT } else { WIRE };
        painter.circle_filled(map.to(j.at) + off_of(Item::Junction(i)), (0.5 * map.zoom).max(2.5), c);
    }
    for (i, n) in sch.no_connects.iter().enumerate() {
        let c = if sel(Item::NoConnect(i)) { SELECT } else { BUS };
        let p = map.to(n.at) + off_of(Item::NoConnect(i));
        let d = (0.635 * map.zoom).max(3.0);
        painter.line_segment([p + Vec2::new(-d, -d), p + Vec2::new(d, d)], Stroke::new(lw, c));
        painter.line_segment([p + Vec2::new(-d, d), p + Vec2::new(d, -d)], Stroke::new(lw, c));
    }
    for (i, l) in sch.labels.iter().enumerate() {
        let item = Item::Label(i);
        let c = if sel(item) { SELECT } else { LABEL };
        let anchor = map.to(l.at) + off_of(item);
        let p = anchor;
        let vertical = (l.angle.rem_euclid(180.0) - 90.0).abs() < 1.0;
        let right = l.angle > 135.0;
        let mut align = match (vertical, right) {
            (false, false) => egui::Align2::LEFT_BOTTOM,
            (false, true) => egui::Align2::RIGHT_BOTTOM,
            (true, _) => egui::Align2::LEFT_BOTTOM,
        };
        // Clear of the wire or pin the label hangs on: the text goes to the side the
        // connection does not use.
        let gap = lw / 2.0 + 2.5;
        let mut p = p;
        if !vertical {
            let dirs = attach_dirs(sch, l.at);
            let up = dirs.iter().any(|d| d[1] < -0.5);
            let rightward = dirs.iter().any(|d| d[0] > 0.5);
            if rightward && !right {
                align = egui::Align2::RIGHT_BOTTOM;
            }
            let x_away = if matches!(align, egui::Align2::RIGHT_BOTTOM | egui::Align2::RIGHT_TOP) { -gap } else { gap };
            if up {
                align = if align.x() == egui::Align::Max { egui::Align2::RIGHT_TOP } else { egui::Align2::LEFT_TOP };
                p += Vec2::new(x_away, gap);
            } else {
                p += Vec2::new(x_away, -gap);
            }
        } else {
            p += Vec2::new(gap, 0.0);
        }
        let shown = match l.kind {
            LabelKind::Local => l.text.clone(),
            LabelKind::Global => format!("◆ {}", l.text),
            LabelKind::Hierarchical => format!("▶ {}", l.text),
        };
        text(painter, map, p, &shown, 1.27, if vertical { 90.0 } else { 0.0 }, align, c);
        painter.circle_filled(anchor, 1.5, c);
    }
}
