//! The Schematic tab's editor (ADR 0047 phase 2a): a sheet of the open schematic drawn
//! in KiCad-like colours on a light canvas, panned and zoomed like the board editor
//! (wheel zooms about the pointer, middle drag or a drag on empty paper pans, double-click
//! fits).
//!
//! Tools: Select (click picks a symbol, wire, label, junction or no-connect flag; drag
//! moves a symbol or label on the grid; R turns, X and Y mirror, Delete removes), Wire
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
    /// The sheet the view was fitted for.
    fitted: Option<String>,
    /// The shown sheet's file (`None`: the root).
    pub sheet: Option<String>,
    pub tool: Tool,
    pub grid: f64,
    pub selected: Option<Item>,
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
            fitted: None,
            sheet: None,
            tool: Tool::Select,
            grid: 1.27,
            selected: None,
            drag: None,
            wire: Vec::new(),
            label_text: "NET".into(),
            label_kind: LabelKind::Local,
            library: String::new(),
            symbol: String::new(),
            show_markers: true,
            probe: None,
            highlight: None,
            fields: None,
        }
    }
}

#[derive(Debug, Clone)]
struct Drag {
    /// The symbol or label being moved, or `None` while panning.
    item: Option<Item>,
    moved: Vec2,
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
        self.zoom = ((rect.width() - 40.0) / w).min((rect.height() - 40.0) / h).clamp(0.2, 400.0);
        let c = Vec2::new((lo[0] + hi[0]) as f32 / 2.0, (lo[1] + hi[1]) as f32 / 2.0);
        self.offset = rect.size() / 2.0 - c * self.zoom;
    }
}

/// The side panel: sheet, tools, grid, label and symbol to place, ERC summary.
pub fn controls(ui: &mut Ui, pal: &Palette, open: Option<&OpenSchematic>, st: &mut State) -> Out {
    let mut out = Out::default();
    let Some(open) = open else {
        ui.label(tr("sch.none"));
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
        for (t, key) in [
            (Tool::Select, "pcb.select"),
            (Tool::Wire, "sch.wire"),
            (Tool::Label, "sch.label"),
            (Tool::Place, "sch.place"),
        ] {
            if ui.selectable_label(st.tool == t, tr(key)).clicked() {
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
                apply |= ui.text_edit_singleline(value).lost_focus() && ui.input(|x| x.key_pressed(egui::Key::Enter));
            });
            ui.horizontal(|ui| {
                ui.label(tr("sch.footprint"));
                apply |= ui.text_edit_singleline(fp).lost_focus() && ui.input(|x| x.key_pressed(egui::Key::Enter));
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
    ui.text_edit_singleline(&mut st.label_text);
    ui.horizontal(|ui| {
        for (k, key) in
            [(LabelKind::Local, "sch.local"), (LabelKind::Global, "sch.global"), (LabelKind::Hierarchical, "sch.hier")]
        {
            ui.selectable_value(&mut st.label_kind, k, tr(key));
        }
    });
    ui.separator();
    ui.label(tr("sch.library"));
    ui.text_edit_singleline(&mut st.library);
    ui.label(tr("sch.symbol"));
    ui.text_edit_singleline(&mut st.symbol);
    ui.separator();
    ui.checkbox(&mut st.show_markers, tr("sch.markers"));
    if ui.button(tr("sch.run_erc")).clicked() {
        out.commands.push(("sch.erc", json!({})));
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
            pal.text_muted,
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
    // A selection past the lists (after an undo) is dropped.
    st.selected = st.selected.filter(|s| Item::parse(sch, &s.name()).is_some());
    if resp.double_clicked() && st.tool == Tool::Select {
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
        }
    }
    let map = Map { origin: rect.min + st.offset, zoom: st.zoom };
    let tol = f64::from(5.0 / st.zoom);
    let pointer = resp.hover_pos().map(|p| map.from(p));

    // Drags: a symbol or label under the pointer moves (Select tool), anything else pans.
    let middle = ui.input(|i| i.pointer.middle_down());
    if resp.drag_started() {
        let press = ui.input(|i| i.pointer.press_origin()).map(|p| map.from(p));
        let item = match (st.tool, middle, press) {
            (Tool::Select, false, Some(p)) => {
                hit(sch, p, tol).filter(|i| matches!(i, Item::Symbol(_) | Item::Label(_)))
            }
            _ => None,
        };
        if let Some(i) = item {
            st.selected = Some(i);
        }
        st.drag = Some(Drag { item, moved: Vec2::ZERO });
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        match st.drag.as_mut() {
            Some(Drag { item: Some(_), moved }) => *moved += d / st.zoom,
            _ => st.offset += d,
        }
    }
    if resp.drag_stopped() {
        if let Some(Drag { item: Some(i), moved }) = st.drag.take()
            && moved.length() > 1e-3
            && let Some(at) = drop_place(sch, i, [f64::from(moved.x), f64::from(moved.y)], st.grid)
        {
            out.commands.push(move_command(st, i, at));
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
                st.selected = hit(sch, p, tol);
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
            st.selected = None;
        }
        if let Some(item) = st.selected {
            if del {
                out.commands.push(delete_command(st, item));
                st.selected = None;
            }
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

    let dragged = st.drag.as_ref().and_then(|d| d.item.map(|i| (i, d.moved)));
    paint(&painter, &map, sch, st, dragged);
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
                let mid = a + (b - a) / 2.0 + if horizontal { Vec2::new(0.0, -2.0) } else { Vec2::new(-2.0, 0.0) };
                let align = if horizontal { egui::Align2::CENTER_BOTTOM } else { egui::Align2::RIGHT_CENTER };
                text(painter, map, mid, &p.number, 1.0, angle, align, PIN_TEXT);
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

fn paint(painter: &egui::Painter, map: &Map, sch: &Schematic, st: &State, dragged: Option<(Item, Vec2)>) {
    let off_of = |item: Item| match dragged {
        Some((i, moved)) if i == item => {
            let at = match item {
                Item::Symbol(k) => sch.symbols.get(k).map(|s| s.at),
                Item::Label(k) => sch.labels.get(k).map(|l| l.at),
                _ => None,
            };
            match (at, drop_place(sch, item, [f64::from(moved.x), f64::from(moved.y)], st.grid)) {
                (Some(a), Some(b)) => Vec2::new(((b[0] - a[0]) as f32) * map.zoom, ((b[1] - a[1]) as f32) * map.zoom),
                _ => Vec2::ZERO,
            }
        }
        _ => Vec2::ZERO,
    };
    let sel = |item: Item| st.selected == Some(item);
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
        painter.line_segment([map.to(w.a), map.to(w.b)], Stroke::new((0.3 * map.zoom).max(2.5), c));
    }
    for (a, size) in &sch.bus_entries {
        painter.line_segment([map.to(*a), map.to([a[0] + size[0], a[1] + size[1]])], Stroke::new(lw, WIRE));
    }
    for (i, w) in sch.wires.iter().enumerate() {
        let c = if sel(Item::Wire(i)) { SELECT } else { WIRE };
        painter.line_segment([map.to(w.a), map.to(w.b)], Stroke::new(lw, c));
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
        painter.circle_filled(map.to(j.at), (0.5 * map.zoom).max(2.5), c);
    }
    for (i, n) in sch.no_connects.iter().enumerate() {
        let c = if sel(Item::NoConnect(i)) { SELECT } else { BUS };
        let p = map.to(n.at);
        let d = (0.635 * map.zoom).max(3.0);
        painter.line_segment([p + Vec2::new(-d, -d), p + Vec2::new(d, d)], Stroke::new(lw, c));
        painter.line_segment([p + Vec2::new(-d, d), p + Vec2::new(d, -d)], Stroke::new(lw, c));
    }
    for (i, l) in sch.labels.iter().enumerate() {
        let item = Item::Label(i);
        let c = if sel(item) { SELECT } else { LABEL };
        let p = map.to(l.at) + off_of(item);
        let vertical = (l.angle.rem_euclid(180.0) - 90.0).abs() < 1.0;
        let right = l.angle > 135.0;
        let align = match (vertical, right) {
            (false, false) => egui::Align2::LEFT_BOTTOM,
            (false, true) => egui::Align2::RIGHT_BOTTOM,
            (true, _) => egui::Align2::LEFT_BOTTOM,
        };
        let shown = match l.kind {
            LabelKind::Local => l.text.clone(),
            LabelKind::Global => format!("◆ {}", l.text),
            LabelKind::Hierarchical => format!("▶ {}", l.text),
        };
        text(painter, map, p, &shown, 1.27, if vertical { 90.0 } else { 0.0 }, align, c);
        painter.circle_filled(p, 1.5, c);
    }
}
