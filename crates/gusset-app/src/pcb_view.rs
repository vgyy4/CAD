//! The PCB tab's board editor (ADR 0047 phase 1b): the open board drawn layer by layer
//! in KiCad-like colours on a dark canvas, panned and zoomed like the drawing sheet (wheel
//! zooms about the pointer, middle drag or a drag on empty board pans, double-click fits).
//!
//! Tools: Select (click picks a footprint, track or via; drag moves a footprint, snapped
//! to the grid; Delete removes tracks and vias, R turns a footprint by 90°, F flips it),
//! Route (clicks lay track segments on the active copper layer, snapping to pad centres;
//! Esc or a right click ends) and Via. Ratsnest lines show the connections still to route
//! and the panel counts them; design rule markers show where `pcb.drc` found problems.
//!
//! The widget never edits the board: it reports registry commands ([`Out::commands`]),
//! which the app sends like any other command (ADR 0012), so every edit is undoable.

use crate::i18n::tr;
use crate::theme::Palette;
use egui::{Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use gusset_commands::pcb_session::OpenBoard;
use gusset_pcb::board::{Board, Pt, Shape as BShape};
use gusset_pcb::geom::{self, Contour};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// What clicks do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Route,
    Via,
}

/// A picked board item (indices into the board's lists).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sel {
    Footprint(usize),
    Track(usize),
    Via(usize),
}

/// A design rule marker from the last `pcb.drc`.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub at: Pt,
    pub text: String,
}

/// Grid choices (mm).
pub const GRIDS: [f64; 6] = [0.1, 0.25, 0.5, 1.0, 1.27, 2.54];

/// Editor state kept across frames.
#[derive(Debug)]
pub struct State {
    zoom: f32,
    offset: Vec2,
    /// The board the view was fitted for (its file name).
    fitted: Option<String>,
    pub hidden: BTreeSet<String>,
    /// The copper layer new tracks go on.
    pub active: String,
    pub tool: Tool,
    pub grid: f64,
    pub selected: Option<Sel>,
    drag: Option<Drag>,
    /// Where the next track segment starts (Route tool).
    pub track_from: Option<Pt>,
    pub markers: Vec<Marker>,
    pub show_ratsnest: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            zoom: 0.0,
            offset: Vec2::ZERO,
            fitted: None,
            hidden: BTreeSet::new(),
            active: "F.Cu".into(),
            tool: Tool::Select,
            grid: 0.5,
            selected: None,
            drag: None,
            track_from: None,
            markers: Vec::new(),
            show_ratsnest: true,
        }
    }
}

#[derive(Debug, Clone)]
struct Drag {
    /// The footprint being moved, or `None` while panning.
    footprint: Option<usize>,
    /// Total movement in board mm.
    moved: Vec2,
}

/// What the user asked for: registry commands with their arguments, in order.
#[derive(Debug, Default, PartialEq)]
pub struct Out {
    pub commands: Vec<(&'static str, Value)>,
}

/// Board mm (Y down, like the screen) to screen points and back.
#[derive(Debug, Clone, Copy)]
pub struct Map {
    pub origin: Pos2,
    pub zoom: f32,
}

impl Map {
    pub fn to(&self, p: Pt) -> Pos2 {
        Pos2::new(self.origin.x + p[0] as f32 * self.zoom, self.origin.y + p[1] as f32 * self.zoom)
    }
    pub fn from(&self, p: Pos2) -> Pt {
        [f64::from((p.x - self.origin.x) / self.zoom), f64::from((p.y - self.origin.y) / self.zoom)]
    }
}

/// The nearest grid point.
pub fn snap(p: Pt, grid: f64) -> Pt {
    if grid <= 0.0 {
        return p;
    }
    let r = |v: f64| ((v / grid).round() * grid * 1e6).round() / 1e6;
    [r(p[0]), r(p[1])]
}

/// A layer's colour, close to KiCad's defaults.
pub fn layer_color(name: &str) -> Color32 {
    match name {
        "F.Cu" => Color32::from_rgb(200, 52, 52),
        "B.Cu" => Color32::from_rgb(77, 127, 196),
        "In1.Cu" => Color32::from_rgb(127, 200, 127),
        "In2.Cu" => Color32::from_rgb(206, 125, 44),
        "F.SilkS" => Color32::from_rgb(242, 237, 161),
        "B.SilkS" => Color32::from_rgb(232, 178, 167),
        "F.Mask" => Color32::from_rgba_unmultiplied(216, 100, 255, 80),
        "B.Mask" => Color32::from_rgba_unmultiplied(2, 255, 238, 80),
        "F.Paste" => Color32::from_rgb(180, 160, 154),
        "B.Paste" => Color32::from_rgb(0, 194, 194),
        "F.Fab" => Color32::from_rgb(175, 175, 175),
        "B.Fab" => Color32::from_rgb(88, 93, 132),
        "F.CrtYd" => Color32::from_rgb(255, 38, 226),
        "B.CrtYd" => Color32::from_rgb(38, 233, 255),
        "Edge.Cuts" => Color32::from_rgb(208, 210, 205),
        n if n.ends_with(".Cu") => Color32::from_rgb(194, 194, 0),
        _ => Color32::from_rgb(140, 140, 140),
    }
}

/// Layers in drawing order, back to front, with the active copper layer above the others.
pub fn draw_order(board: &Board, active: &str) -> Vec<String> {
    let cu = board.copper_layers();
    let mut out: Vec<String> =
        ["B.Fab", "B.CrtYd", "B.SilkS", "B.Paste", "B.Mask"].iter().map(|s| s.to_string()).collect();
    out.extend(cu.iter().rev().filter(|l| *l != active).cloned());
    if cu.iter().any(|l| l == active) {
        out.push(active.to_string());
    }
    out.extend(["F.Mask", "F.Paste", "F.SilkS", "F.CrtYd", "F.Fab", "Edge.Cuts"].iter().map(|s| s.to_string()));
    out
}

/// Layers the panel lists: copper first, then the others drawn.
pub fn panel_layers(board: &Board) -> Vec<String> {
    let mut v = board.copper_layers();
    v.extend(
        ["F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "F.Fab", "B.Fab", "F.CrtYd", "B.CrtYd", "Edge.Cuts"]
            .map(String::from),
    );
    v
}

fn dist_seg(p: Pt, a: Pt, b: Pt) -> f64 {
    gusset_pcb::copper::point_segment(p, a, b).0
}

/// The footprint's extent: its pads and drawings.
fn footprint_box(board: &Board, i: usize) -> Option<[Pt; 2]> {
    let fp = board.footprints.get(i)?;
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    let mut add = |p: Pt, r: f64| {
        lo = [lo[0].min(p[0] - r), lo[1].min(p[1] - r)];
        hi = [hi[0].max(p[0] + r), hi[1].max(p[1] + r)];
    };
    for p in fp.pads.iter().filter_map(|&k| board.pads.get(k)) {
        add(p.at, p.size[0].max(p.size[1]) / 2.0);
    }
    for g in board
        .graphics
        .iter()
        .filter(|g| g.footprint == Some(i) && (g.layer.ends_with(".Fab") || g.layer.ends_with(".CrtYd")))
    {
        match &g.shape {
            BShape::Line { a, b } => {
                add(*a, 0.0);
                add(*b, 0.0);
            }
            BShape::Arc { start, mid, end } => [start, mid, end].into_iter().for_each(|p| add(*p, 0.0)),
            BShape::Circle { center, radius } => add(*center, *radius),
            BShape::Poly { pts } => pts.iter().for_each(|p| add(*p, 0.0)),
        }
    }
    (lo[0] <= hi[0]).then_some([lo, hi])
}

/// What is under board point `p` (`tol` mm of slack): vias, then tracks on shown layers,
/// then the smallest footprint around it.
pub fn hit(board: &Board, p: Pt, tol: f64, shown: &dyn Fn(&str) -> bool) -> Option<Sel> {
    if let Some(i) = board.vias.iter().position(|v| libm::hypot(v.at[0] - p[0], v.at[1] - p[1]) <= v.size / 2.0 + tol) {
        return Some(Sel::Via(i));
    }
    let track = board
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| shown(&t.layer))
        .map(|(i, t)| (i, dist_seg(p, t.start, t.end) - t.width / 2.0))
        .filter(|(_, d)| *d <= tol)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((i, _)) = track {
        return Some(Sel::Track(i));
    }
    (0..board.footprints.len())
        .filter_map(|i| footprint_box(board, i).map(|b| (i, b)))
        .filter(|(_, b)| {
            p[0] >= b[0][0] - tol && p[0] <= b[1][0] + tol && p[1] >= b[0][1] - tol && p[1] <= b[1][1] + tol
        })
        .min_by(|a, b| {
            let area = |x: &[Pt; 2]| (x[1][0] - x[0][0]) * (x[1][1] - x[0][1]);
            area(&a.1).total_cmp(&area(&b.1))
        })
        .map(|(i, _)| Sel::Footprint(i))
}

/// The centre of a pad on `layer` under `p`, for routing from pad to pad.
pub fn pad_center_at(board: &Board, p: Pt, layer: &str) -> Option<Pt> {
    board.pads.iter().filter(|pad| board.on_layer(&pad.layers, layer)).find_map(|pad| {
        let c = geom::pad_contour(pad, 0.0)?;
        let inside = match c {
            Contour::Circle { center, radius } => libm::hypot(center[0] - p[0], center[1] - p[1]) <= radius,
            c => geom::inside(&c.polygon(0.05), p),
        };
        inside.then_some(pad.at)
    })
}

/// Where a footprint dragged by `moved` lands: its place moved and snapped to the grid.
pub fn drop_place(board: &Board, i: usize, moved: Pt, grid: f64) -> Option<Pt> {
    let fp = board.footprints.get(i)?;
    Some(snap([fp.at[0] + moved[0], fp.at[1] + moved[1]], grid))
}

/// The command a key or tool action stands for.
pub fn move_command(i: usize, at: Pt) -> (&'static str, Value) {
    ("pcb.move_footprint", json!({ "footprint": format!("#{i}"), "at": at }))
}

pub fn rotate_command(board: &Board, i: usize) -> Option<(&'static str, Value)> {
    let fp = board.footprints.get(i)?;
    Some(("pcb.move_footprint", json!({ "footprint": format!("#{i}"), "angle": (fp.angle + 90.0).rem_euclid(360.0) })))
}

pub fn flip_command(i: usize) -> (&'static str, Value) {
    ("pcb.move_footprint", json!({ "footprint": format!("#{i}"), "flip": true }))
}

pub fn delete_command(sel: Sel) -> Option<(&'static str, Value)> {
    match sel {
        Sel::Track(i) => Some(("pcb.delete", json!({ "tracks": [i] }))),
        Sel::Via(i) => Some(("pcb.delete", json!({ "vias": [i] }))),
        Sel::Footprint(_) => None,
    }
}

pub fn track_command(a: Pt, b: Pt, layer: &str) -> (&'static str, Value) {
    ("pcb.add_track", json!({ "start": a, "end": b, "layer": layer }))
}

pub fn via_command(at: Pt) -> (&'static str, Value) {
    ("pcb.add_via", json!({ "at": at }))
}

/// Markers from a `pcb.drc` result.
pub fn markers_from(result: &Value) -> Vec<Marker> {
    result
        .get("violations")
        .and_then(Value::as_array)
        .map(|v| {
            v.iter()
                .filter_map(|x| {
                    let at = x.get("at")?.as_array()?;
                    let p = [at.first()?.as_f64()?, at.get(1)?.as_f64()?];
                    let text = x.get("message_user_text").and_then(Value::as_str).unwrap_or_default();
                    let text = text.chars().filter(|c| !c.is_control()).take(200).collect();
                    Some(Marker { at: p, text })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The panel's line about routing.
pub fn unrouted_text(n: usize) -> String {
    if n == 0 { tr("pcb.all_routed").to_string() } else { format!("{}: {n}", tr("pcb.unrouted")) }
}

impl State {
    fn fit(&mut self, rect: Rect, board: &Board) {
        let (loops, _) = geom::outline_loops(board, 1e-3);
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        let pts = loops.iter().flat_map(|l| l.polygon(0.1)).chain(board.pads.iter().map(|p| p.at));
        for p in pts {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        if lo[0] > hi[0] {
            (lo, hi) = ([0.0, 0.0], [100.0, 100.0]);
        }
        let (w, h) = (((hi[0] - lo[0]) as f32).max(1.0), ((hi[1] - lo[1]) as f32).max(1.0));
        self.zoom = ((rect.width() - 40.0) / w).min((rect.height() - 40.0) / h).clamp(0.05, 2000.0);
        let c = Vec2::new((lo[0] + hi[0]) as f32 / 2.0, (lo[1] + hi[1]) as f32 / 2.0);
        self.offset = rect.size() / 2.0 - c * self.zoom;
    }

    fn shown(&self, layer: &str) -> bool {
        !self.hidden.contains(layer)
    }
}

/// The side panel: tools, the active layer, grid, layer visibility, ratsnest and DRC.
pub fn controls(ui: &mut Ui, pal: &Palette, open: Option<&OpenBoard>, st: &mut State) -> Out {
    let mut out = Out::default();
    let Some(open) = open else {
        ui.label(tr("pcb.no_board"));
        if ui.button(tr("pcb.new")).clicked() {
            out.commands.push(("pcb.new", json!({ "width": "100 mm", "height": "80 mm" })));
        }
        return out;
    };
    let b = &open.board;
    ui.strong(&open.file);
    ui.weak(format!(
        "{} {} · {} {} · {} {}",
        b.footprints.len(),
        tr("pcb.footprints"),
        b.tracks.len(),
        tr("pcb.tracks"),
        b.vias.len(),
        tr("pcb.vias")
    ));
    let n = open.ratsnest.unrouted();
    ui.colored_label(if n == 0 { pal.success } else { pal.warning }, unrouted_text(n));
    ui.separator();
    ui.horizontal(|ui| {
        for (t, key) in [(Tool::Select, "pcb.select"), (Tool::Route, "pcb.route"), (Tool::Via, "pcb.via")] {
            if ui.selectable_label(st.tool == t, tr(key)).clicked() {
                st.tool = t;
                st.track_from = None;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(tr("pcb.grid"));
        egui::ComboBox::from_id_salt("pcb_grid").selected_text(format!("{} mm", st.grid)).show_ui(ui, |ui| {
            for g in GRIDS {
                ui.selectable_value(&mut st.grid, g, format!("{g} mm"));
            }
        });
    });
    ui.separator();
    ui.label(tr("pcb.layers"));
    let cu = b.copper_layers();
    for l in panel_layers(b) {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, layer_color(&l));
            let mut on = st.shown(&l);
            if ui.checkbox(&mut on, l.as_str()).changed() {
                if on {
                    st.hidden.remove(&l);
                } else {
                    st.hidden.insert(l.clone());
                }
            }
            if cu.contains(&l) && ui.selectable_label(st.active == l, tr("pcb.active")).clicked() {
                st.active = l.clone();
            }
        });
    }
    ui.checkbox(&mut st.show_ratsnest, tr("pcb.ratsnest"));
    ui.separator();
    if ui.button(tr("pcb.run_drc")).clicked() {
        out.commands.push(("pcb.drc", json!({})));
    }
    if !st.markers.is_empty() {
        ui.colored_label(pal.danger, format!("{}: {}", tr("pcb.problems"), st.markers.len()));
        egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
            for m in st.markers.iter().take(200) {
                ui.label(egui::RichText::new(&m.text).small());
            }
        });
        if ui.small_button(tr("pcb.clear_markers")).clicked() {
            st.markers.clear();
        }
    }
    out
}

/// Draws the board and handles the editing tools.
pub fn show(ui: &mut Ui, pal: &Palette, open: Option<&OpenBoard>, st: &mut State) -> Out {
    let mut out = Out::default();
    let rect = ui.max_rect();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(0, 16, 35));
    let Some(open) = open else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            tr("pcb.no_board"),
            FontId::proportional(14.0),
            pal.text_muted,
        );
        return out;
    };
    let b = &open.board;
    if st.fitted.as_deref() != Some(open.file.as_str()) || st.zoom <= 0.0 {
        st.fit(rect, b);
        st.fitted = Some(open.file.clone());
        st.selected = None;
        st.markers.clear();
    }
    if !b.copper_layers().contains(&st.active)
        && let Some(f) = b.copper_layers().first()
    {
        st.active = f.clone();
    }
    // A selection past the lists (after an undo) is dropped.
    st.selected = st.selected.filter(|s| match *s {
        Sel::Footprint(i) => i < b.footprints.len(),
        Sel::Track(i) => i < b.tracks.len(),
        Sel::Via(i) => i < b.vias.len(),
    });
    if resp.double_clicked() && st.tool == Tool::Select {
        st.fit(rect, b);
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 200.0);
        if scroll != 0.0
            && let Some(p) = resp.hover_pos()
        {
            let new = (st.zoom * (scroll * 0.002).exp()).clamp(0.05, 2000.0);
            let rel = p - rect.min - st.offset;
            st.offset = p - rect.min - rel * (new / st.zoom);
            st.zoom = new;
        }
    }
    let map = Map { origin: rect.min + st.offset, zoom: st.zoom };
    let tol = f64::from(4.0 / st.zoom);
    let pointer = resp.hover_pos().map(|p| map.from(p));
    let hidden = st.hidden.clone();
    let shown = |l: &str| !hidden.contains(l);

    // Drags: a footprint under the pointer moves (Select tool), anything else pans.
    let middle = ui.input(|i| i.pointer.middle_down());
    if resp.drag_started() {
        let press = ui.input(|i| i.pointer.press_origin()).map(|p| map.from(p));
        let fp = match (st.tool, middle, press) {
            (Tool::Select, false, Some(p)) => match hit(b, p, tol, &shown) {
                Some(Sel::Footprint(i)) => Some(i),
                _ => None,
            },
            _ => None,
        };
        if let Some(i) = fp {
            st.selected = Some(Sel::Footprint(i));
        }
        st.drag = Some(Drag { footprint: fp, moved: Vec2::ZERO });
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        match st.drag.as_mut() {
            Some(Drag { footprint: Some(_), moved }) => *moved += d / st.zoom,
            _ => st.offset += d,
        }
    }
    if resp.drag_stopped() {
        if let Some(Drag { footprint: Some(i), moved }) = st.drag.take()
            && moved.length() > 1e-3
            && let Some(at) = drop_place(b, i, [f64::from(moved.x), f64::from(moved.y)], st.grid)
        {
            out.commands.push(move_command(i, at));
        }
    } else if !resp.dragged() {
        st.drag = None;
    }

    // Clicks.
    if resp.clicked()
        && let Some(p) = pointer
    {
        match st.tool {
            Tool::Select => st.selected = hit(b, p, tol, &shown),
            Tool::Via => out.commands.push(via_command(snap(p, st.grid))),
            Tool::Route => {
                let q = pad_center_at(b, p, &st.active).unwrap_or_else(|| snap(p, st.grid));
                if let Some(a) = st.track_from
                    && libm::hypot(q[0] - a[0], q[1] - a[1]) > 1e-6
                {
                    out.commands.push(track_command(a, q, &st.active));
                }
                st.track_from = Some(q);
            }
        }
    }
    if resp.secondary_clicked() {
        st.track_from = None;
    }
    // Keys while the pointer is over the board.
    if resp.hovered() {
        let (del, esc, r, f) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::R),
                i.key_pressed(egui::Key::F),
            )
        });
        if esc {
            st.track_from = None;
            st.selected = None;
        }
        if del && let Some(c) = st.selected.and_then(delete_command) {
            out.commands.push(c);
            st.selected = None;
        }
        if let Some(Sel::Footprint(i)) = st.selected {
            if r && let Some(c) = rotate_command(b, i) {
                out.commands.push(c);
            }
            if f {
                out.commands.push(flip_command(i));
            }
        }
    }

    let dragged = st.drag.as_ref().and_then(|d| d.footprint.map(|i| (i, d.moved)));
    paint(&painter, &map, open, st, dragged);

    // Route preview.
    if let (Tool::Route, Some(a), Some(p)) = (st.tool, st.track_from, pointer) {
        let q = pad_center_at(b, p, &st.active).unwrap_or_else(|| snap(p, st.grid));
        let w = (open.rules.track_width as f32 * map.zoom).max(1.0);
        let c = layer_color(&st.active).gamma_multiply(0.7);
        painter.line_segment([map.to(a), map.to(q)], Stroke::new(w, c));
        painter.circle_filled(map.to(q), w / 2.0, c);
    }
    // The pointer's place on the grid.
    if let Some(p) = pointer {
        let q = snap(p, st.grid);
        painter.text(
            rect.left_bottom() + Vec2::new(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            format!("X {:.3}  Y {:.3} mm", q[0], q[1]),
            FontId::monospace(12.0),
            pal.text_muted,
        );
        if let Some(m) = st.markers.iter().find(|m| map.to(m.at).distance(map.to(p)) < 8.0) {
            resp.clone().on_hover_text(m.text.clone());
        }
    }
    out
}

fn stroke_line(painter: &egui::Painter, a: Pos2, b: Pos2, w: f32, c: Color32) {
    painter.line_segment([a, b], Stroke::new(w, c));
    if w > 2.0 {
        painter.circle_filled(a, w / 2.0, c);
        painter.circle_filled(b, w / 2.0, c);
    }
}

fn paint_shape(painter: &egui::Painter, map: &Map, s: &BShape, width: f64, filled: bool, c: Color32, off: Vec2) {
    let to = |p: Pt| map.to(p) + off;
    let w = (width as f32 * map.zoom).max(1.0);
    match s {
        BShape::Line { a, b } => stroke_line(painter, to(*a), to(*b), w, c),
        BShape::Arc { start, mid, end } => {
            let pts: Vec<Pos2> =
                geom::arc_points(*start, *mid, *end, f64::from(0.5 / map.zoom).max(1e-3)).into_iter().map(to).collect();
            painter.add(Shape::line(pts, Stroke::new(w, c)));
        }
        BShape::Circle { center, radius } => {
            let r = *radius as f32 * map.zoom;
            if filled {
                painter.circle_filled(to(*center), r, c);
            } else {
                painter.circle_stroke(to(*center), r, Stroke::new(w, c));
            }
        }
        BShape::Poly { pts } => {
            let v: Vec<Pos2> = pts.iter().map(|p| to(*p)).collect();
            if filled && v.len() >= 3 {
                painter.add(Shape::convex_polygon(v, c, Stroke::NONE));
            } else {
                painter.add(Shape::closed_line(v, Stroke::new(w, c)));
            }
        }
    }
}

fn paint(painter: &egui::Painter, map: &Map, open: &OpenBoard, st: &State, dragged: Option<(usize, Vec2)>) {
    let b = &open.board;
    let view = painter.clip_rect();
    // Items of the dragged footprint are drawn where they would land.
    let off_of = |fp: Option<usize>| match (fp, dragged) {
        (Some(i), Some((j, moved))) if i == j => {
            let at = b.footprints.get(i).map_or([0.0; 2], |f| f.at);
            let to = drop_place(b, i, [f64::from(moved.x), f64::from(moved.y)], st.grid).unwrap_or(at);
            Vec2::new(((to[0] - at[0]) as f32) * map.zoom, ((to[1] - at[1]) as f32) * map.zoom)
        }
        _ => Vec2::ZERO,
    };
    let pad_fp: Vec<Option<usize>> = {
        let mut v = vec![None; b.pads.len()];
        for (i, f) in b.footprints.iter().enumerate() {
            for &p in &f.pads {
                if let Some(slot) = v.get_mut(p) {
                    *slot = Some(i);
                }
            }
        }
        v
    };
    let cu = b.copper_layers();
    for layer in draw_order(b, &st.active) {
        if !st.shown(&layer) {
            continue;
        }
        let is_cu = cu.contains(&layer);
        let dim = if is_cu && layer != st.active { 0.55 } else { 1.0 };
        let c = layer_color(&layer).gamma_multiply(dim);
        if is_cu {
            for z in b.zones.iter().filter(|z| !z.keepout) {
                for (_, poly) in z.fills.iter().filter(|(l, _)| *l == layer) {
                    let v: Vec<Pos2> = poly.iter().map(|p| map.to(*p)).collect();
                    painter.add(Shape::closed_line(v, Stroke::new(1.0, c.gamma_multiply(0.6))));
                }
            }
        }
        for g in b.graphics.iter().filter(|g| g.layer == layer) {
            paint_shape(painter, map, &g.shape, g.width, g.filled, c, off_of(g.footprint));
        }
        if !is_cu {
            continue;
        }
        for (i, t) in b.tracks.iter().enumerate().filter(|(_, t)| t.layer == layer) {
            let w = (t.width as f32 * map.zoom).max(1.0);
            let col = if st.selected == Some(Sel::Track(i)) { Color32::WHITE } else { c };
            match t.mid {
                Some(m) => {
                    let pts: Vec<Pos2> = geom::arc_points(t.start, m, t.end, f64::from(0.5 / map.zoom).max(1e-3))
                        .into_iter()
                        .map(|p| map.to(p))
                        .collect();
                    painter.add(Shape::line(pts, Stroke::new(w, col)));
                }
                None => {
                    let (a, e) = (map.to(t.start), map.to(t.end));
                    if view.expand(w).intersects(Rect::from_two_pos(a, e)) {
                        stroke_line(painter, a, e, w, col);
                    }
                }
            }
        }
        for (k, pad) in b.pads.iter().enumerate().filter(|(_, p)| b.on_layer(&p.layers, &layer)) {
            let off = off_of(pad_fp.get(k).copied().flatten());
            let selected =
                matches!(st.selected, Some(Sel::Footprint(i)) if pad_fp.get(k).copied().flatten() == Some(i));
            let col = if selected { Color32::WHITE.gamma_multiply(0.9) } else { c };
            match geom::pad_contour(pad, 0.0) {
                Some(Contour::Circle { center, radius }) => {
                    painter.circle_filled(map.to(center) + off, radius as f32 * map.zoom, col);
                }
                Some(cont) => {
                    let v: Vec<Pos2> = cont
                        .polygon(f64::from(0.5 / map.zoom).max(1e-3))
                        .into_iter()
                        .map(|p| map.to(p) + off)
                        .collect();
                    painter.add(Shape::convex_polygon(v, col, Stroke::NONE));
                }
                None => {}
            }
            for s in &pad.primitives {
                paint_shape(painter, map, s, 0.0, true, col, off);
            }
        }
    }
    // Vias above the copper, holes above everything.
    let via_c = Color32::from_rgb(236, 236, 236);
    for (i, v) in b.vias.iter().enumerate() {
        let col = if st.selected == Some(Sel::Via(i)) { Color32::WHITE } else { via_c.gamma_multiply(0.8) };
        painter.circle_filled(map.to(v.at), v.size as f32 / 2.0 * map.zoom, col);
        painter.circle_filled(map.to(v.at), (v.drill as f32 / 2.0 * map.zoom).max(0.5), Color32::from_rgb(0, 16, 35));
    }
    for (k, pad) in b.pads.iter().enumerate() {
        let Some(d) = pad.drill else { continue };
        let h = gusset_pcb::drill::pad_hole(pad.at, pad.angle, &d);
        let off = off_of(pad_fp.get(k).copied().flatten());
        let r = (h.diameter as f32 / 2.0 * map.zoom).max(0.5);
        match h.slot_end {
            Some(e) => {
                stroke_line(painter, map.to(h.at) + off, map.to(e) + off, 2.0 * r, Color32::from_rgb(20, 20, 20))
            }
            None => {
                painter.circle_filled(map.to(h.at) + off, r, Color32::from_rgb(20, 20, 20));
            }
        }
    }
    if st.show_ratsnest {
        let rc = Color32::from_rgba_unmultiplied(245, 245, 245, 160);
        for l in &open.ratsnest.lines {
            painter.line_segment([map.to(l.a), map.to(l.b)], Stroke::new(1.0, rc));
        }
    }
    // The selected footprint's box.
    if let Some(Sel::Footprint(i)) = st.selected
        && let Some(bx) = footprint_box(b, i)
    {
        let off = off_of(Some(i));
        let r = Rect::from_two_pos(map.to(bx[0]) + off, map.to(bx[1]) + off).expand(3.0);
        painter.rect_stroke(r, 2.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
    }
    for m in &st.markers {
        let p = map.to(m.at);
        let red = Color32::from_rgb(255, 60, 60);
        painter.circle_stroke(p, 6.0, Stroke::new(2.0, red));
        painter.line_segment([p + Vec2::new(-4.0, -4.0), p + Vec2::new(4.0, 4.0)], Stroke::new(2.0, red));
        painter.line_segment([p + Vec2::new(-4.0, 4.0), p + Vec2::new(4.0, -4.0)], Stroke::new(2.0, red));
    }
}
