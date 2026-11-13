//! The PCB tab's board editor (ADR 0047 phase 1b): the open board drawn layer by layer
//! in KiCad-like colours on a dark canvas, panned and zoomed like the drawing sheet (wheel
//! zooms about the pointer, middle drag or a drag on empty board pans, double-click fits).
//!
//! Tools: Select (click picks a footprint, track or via; drag moves a footprint, snapped
//! to the grid; Delete removes tracks and vias, R turns a footprint by 90°, F flips it),
//! Route and Via. Routing (phase 2b) starts on a pad, track end or via and takes its net;
//! the preview walks around other nets' copper in 45° segments with the clearance rules
//! and the net class's width ([`gusset_pcb::route`]); a click lays it, a click on a pad
//! of the net finishes, V places a via and goes on on the other side, `/` turns the bend
//! the other way; Esc, a right click or a double click ends. A click where the preview
//! finds no way asks the router to shove one blocking track aside (phase 3). The picked item's net is
//! highlighted. Zone fills are drawn filled (triangulated). Ratsnest lines show the
//! connections still to route; design rule markers show where `pcb.drc` found problems.
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
    /// Click adds a board text from the panel; a click on a text picks it for editing.
    Text,
    /// Puts down the footprint picked in the panel's library list.
    Place,
    /// Clicks add zone corners; a double click, Enter or a click on the first corner
    /// closes the zone (on the active layer, with the panel's net and hatch).
    Zone,
    /// Clicks add outline corners; a double click, Enter or a click on the first corner
    /// replaces the board outline (`pcb.set_outline`).
    Outline,
}

/// A picked board item (indices into the board's lists).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sel {
    /// A pad on its own, index in the board's pads.
    Pad(usize),
    Footprint(usize),
    Track(usize),
    Via(usize),
    /// A board text (not a footprint's), index in the board's texts.
    Text(usize),
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
    /// The view still shows the fit and the canvas had this size then: a canvas that
    /// changes size (the panels settle in the first frames) fits again; a pan or zoom by
    /// the user ends this.
    auto_fit: Option<Vec2>,
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
    /// The net being routed (from where the route started).
    pub route_net: Option<String>,
    /// Two-segment routes take the diagonal first.
    pub diagonal_first: bool,
    /// Corners rounded into arcs of this radius (mm); 0 keeps them sharp.
    pub arc_radius: f64,
    /// A net drawn brighter than the rest.
    pub highlight: Option<String>,
    route_cache: Option<RouteCache>,
    fill_cache: FillCache,
    /// More picked items (Shift-click, Shift-drag box), beside `selected`.
    pub multi: Vec<Sel>,
    /// A box being dragged out (board mm, from where it started).
    boxing: Option<Pt>,
    /// The footprint library folder (holding Lib.pretty folders), the names found in it
    /// (`Lib:Name`), a filter, and the name the Place tool puts down.
    pub library: String,
    pub library_names: Vec<String>,
    library_read: String,
    pub library_filter: String,
    pub footprint: String,
    /// Zone tool: corners so far, the new zone's net, hatch and its grid (mm).
    pub zone_pts: Vec<Pt>,
    /// Outline tool: corners so far.
    pub outline_pts: Vec<Pt>,
    pub zone_net: String,
    /// Text tool: the words, height (mm), angle and side of a new text.
    pub text_words: String,
    pub text_size: f64,
    pub text_angle: f64,
    pub text_back: bool,
    /// The picked text as edited in the panel: (board hash, index, words, height, angle, mirrored).
    pub text_edit: Option<TextEdit>,
    pub zone_hatch: bool,
    pub hatch_thickness: f64,
    pub hatch_gap: f64,
    /// Pad nets of the picked footprint as edited in the panel: (board hash, footprint,
    /// (pad number, net) per pad).
    pub pad_nets: Option<PadNets>,
    /// The picked pad's net as typed: (board hash, pad, net).
    pub pad_edit: Option<(String, usize, String)>,
    /// The highlighted net's new name, as typed.
    pub rename_to: String,
    /// The net class edited in the panel and its sizes (mm).
    pub class: ClassEdit,
    /// The user's Freerouting (jar or program), Java for a jar, and the run's limit (s).
    pub freerouting: String,
    pub freerouting_java: String,
    pub freerouting_timeout: u64,
}

/// The picked text as edited: (board hash, index, words, height mm, angle, mirrored).
pub type TextEdit = (String, usize, String, f64, f64, bool);

/// The picked footprint's pad nets as edited: (board hash, footprint, (pad, net) rows).
pub type PadNets = (String, usize, Vec<(String, String)>);

/// The net class form: name and sizes (mm; 0 leaves a size as it is).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClassEdit {
    pub name: String,
    pub clearance: f64,
    pub track_width: f64,
    pub via_diameter: f64,
    pub via_drill: f64,
}

/// The router's obstacles for the board, layer, net and width last routed, and the last
/// preview.
#[derive(Debug)]
struct RouteCache {
    key: (String, String, Option<String>, u64),
    obs: gusset_pcb::route::Obstacles,
    last: Option<(Pt, Pt, bool, Option<Vec<Pt>>)>,
}

/// Triangles of zone fill polygons, by the polygon's contents (fills an edit does not
/// touch are not triangulated again).
#[derive(Debug, Default)]
pub struct FillCache {
    board: String,
    tris: std::collections::HashMap<u64, std::sync::Arc<Vec<[u32; 3]>>>,
}

impl Default for State {
    fn default() -> Self {
        State {
            zoom: 0.0,
            offset: Vec2::ZERO,
            auto_fit: None,
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
            route_net: None,
            diagonal_first: true,
            arc_radius: 0.0,
            highlight: None,
            route_cache: None,
            fill_cache: FillCache::default(),
            multi: Vec::new(),
            boxing: None,
            library: String::new(),
            library_names: Vec::new(),
            library_read: String::new(),
            library_filter: String::new(),
            footprint: String::new(),
            zone_pts: Vec::new(),
            outline_pts: Vec::new(),
            zone_net: String::new(),
            text_words: "TEXT".into(),
            text_size: 1.0,
            text_angle: 0.0,
            text_back: false,
            text_edit: None,
            zone_hatch: false,
            hatch_thickness: 0.5,
            hatch_gap: 1.0,
            pad_nets: None,
            pad_edit: None,
            rename_to: String::new(),
            class: ClassEdit { name: "Default".into(), ..ClassEdit::default() },
            freerouting: String::new(),
            freerouting_java: String::new(),
            freerouting_timeout: 600,
        }
    }
}

#[derive(Debug, Clone)]
struct Drag {
    /// The footprint being moved, or `None` while panning.
    footprint: Option<usize>,
    /// The track or via being moved (with the other picked ones), when not a footprint.
    item: Option<Sel>,
    /// Total movement in board mm.
    moved: Vec2,
}

/// What the user asked for: registry commands with their arguments, in order.
#[derive(Debug, Default, PartialEq)]
pub struct Out {
    pub commands: Vec<(&'static str, Value)>,
    /// A via that did not fit there (shown, nothing sent).
    pub blocked: Option<Pt>,
}

/// The footprints of a library folder: `Lib:Name` for each `Name.kicad_mod` in a
/// `Lib.pretty` folder in it, and `Name` for loose `.kicad_mod` files (sorted, at most
/// `max`, the first in name order). A folder that is itself `Lib.pretty` lists its own
/// files as `Lib:Name`.
pub fn library_footprints(folder: &str, max: usize) -> Vec<String> {
    let dir = std::path::Path::new(folder.trim());
    let mut out = Vec::new();
    let scan = |d: &std::path::Path, lib: Option<&str>, out: &mut Vec<String>| {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten().take(100_000) {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix(".kicad_mod")
                && !stem.is_empty()
                && out.len() < 200_000
            {
                out.push(match lib {
                    Some(l) => format!("{l}:{stem}"),
                    None => stem.to_string(),
                });
            }
        }
    };
    let own = dir.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".pretty")).map(str::to_string);
    if let Some(l) = own {
        scan(dir, Some(&l), &mut out);
    } else {
        scan(dir, None, &mut out);
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten().take(10_000) {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(l) = name.strip_suffix(".pretty") {
                    scan(&e.path(), Some(l), &mut out);
                }
            }
        }
    }
    out.sort();
    out.truncate(max);
    out
}

/// The command the Place tool sends for a click at `at`.
pub fn place_command(st: &State, at: Pt) -> Option<(&'static str, Value)> {
    if st.footprint.trim().is_empty() || st.library.trim().is_empty() {
        return None;
    }
    let folder = st.library.trim();
    // A `Lib.pretty` folder itself: its parent holds the library.
    let (dir, name) = match std::path::Path::new(folder).file_name().and_then(|n| n.to_str()) {
        Some(n) if n.ends_with(".pretty") => {
            let parent = std::path::Path::new(folder).parent().map(|p| p.to_string_lossy().into_owned());
            (parent.filter(|p| !p.is_empty()).unwrap_or_else(|| ".".into()), st.footprint.trim().to_string())
        }
        _ => (folder.to_string(), st.footprint.trim().to_string()),
    };
    Some(("pcb.add_footprint", json!({ "library": dir, "footprint": name, "at": at })))
}

/// Whether an item is picked (the main pick or one of the others).
pub fn is_picked(st: &State, s: Sel) -> bool {
    st.selected == Some(s) || st.multi.contains(&s)
}

/// The items wholly inside a box (board mm): footprints, tracks and vias.
pub fn in_box(board: &Board, a: Pt, b: Pt, shown: &dyn Fn(&str) -> bool) -> Vec<Sel> {
    let (lo, hi) = ([a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])]);
    let inside = |p: Pt| p[0] >= lo[0] && p[0] <= hi[0] && p[1] >= lo[1] && p[1] <= hi[1];
    let mut out = Vec::new();
    for (i, _) in board.footprints.iter().enumerate() {
        if footprint_box(board, i).is_some_and(|bx| inside(bx[0]) && inside(bx[1])) {
            out.push(Sel::Footprint(i));
        }
    }
    for (i, t) in board.tracks.iter().enumerate() {
        if shown(&t.layer) && inside(t.start) && inside(t.end) {
            out.push(Sel::Track(i));
        }
    }
    for (i, v) in board.vias.iter().enumerate() {
        if inside(v.at) {
            out.push(Sel::Via(i));
        }
    }
    out
}

/// One `pcb.delete` for every picked track and via.
pub fn delete_picked(st: &State) -> Option<(&'static str, Value)> {
    let all: Vec<Sel> = st.selected.into_iter().chain(st.multi.iter().copied()).collect();
    let mut tracks: Vec<usize> =
        all.iter().filter_map(|s| if let Sel::Track(i) = s { Some(*i) } else { None }).collect();
    let mut vias: Vec<usize> = all.iter().filter_map(|s| if let Sel::Via(i) = s { Some(*i) } else { None }).collect();
    tracks.sort_unstable();
    tracks.dedup();
    vias.sort_unstable();
    vias.dedup();
    (!tracks.is_empty() || !vias.is_empty()).then(|| ("pcb.delete", json!({ "tracks": tracks, "vias": vias })))
}

/// The zone the Zone tool's corners make (at least 3): `pcb.add_zone` on the active
/// layer with the panel's net and hatch.
pub fn zone_command(st: &State) -> Option<(&'static str, Value)> {
    if st.zone_pts.len() < 3 {
        return None;
    }
    let mut a = serde_json::Map::new();
    a.insert("outline".into(), json!(st.zone_pts));
    a.insert("layers".into(), json!([st.active]));
    let net = st.zone_net.trim();
    if !net.is_empty() {
        a.insert("net".into(), json!(net));
    }
    if st.zone_hatch {
        a.insert("hatch".into(), json!({ "thickness": st.hatch_thickness, "gap": st.hatch_gap }));
    }
    Some(("pcb.add_zone", Value::Object(a)))
}

/// The pads of footprint `i` with their nets (pads without a number left out).
pub fn pad_nets_of(board: &Board, i: usize) -> Vec<(String, String)> {
    let Some(f) = board.footprints.get(i) else { return Vec::new() };
    let mut v: Vec<(String, String)> = f
        .pads
        .iter()
        .filter_map(|&p| board.pads.get(p))
        .filter(|p| !p.number.is_empty())
        .map(|p| (p.number.clone(), board.net_name(&p.net).unwrap_or_default()))
        .collect();
    v.dedup_by(|a, b| a.0 == b.0);
    v
}

/// `pcb.set_net` for one pad of a footprint (an empty net takes the pad off its net).
pub fn pad_net_command(reference: &str, number: &str, net: &str) -> (&'static str, Value) {
    let net = net.trim();
    let mut a = serde_json::Map::new();
    a.insert("pads".into(), json!([format!("{reference}.{number}")]));
    if !net.is_empty() {
        a.insert("net".into(), json!(net));
    }
    ("pcb.set_net", Value::Object(a))
}

/// `pcb.net_class` for the panel's form, adding `nets` to the class.
pub fn class_command(c: &ClassEdit, nets: &[String]) -> Option<(&'static str, Value)> {
    let name = c.name.trim();
    if name.is_empty() {
        return None;
    }
    let mut a = serde_json::Map::new();
    a.insert("name".into(), json!(name));
    a.insert("nets".into(), json!(nets));
    for (k, v) in [
        ("clearance", c.clearance),
        ("track_width", c.track_width),
        ("via_diameter", c.via_diameter),
        ("via_drill", c.via_drill),
    ] {
        if v > 0.0 {
            a.insert(k.into(), json!(v));
        }
    }
    Some(("pcb.net_class", Value::Object(a)))
}

/// Ends the route in progress.
pub fn end_route(st: &mut State) {
    st.track_from = None;
    st.route_net = None;
    st.zone_pts.clear();
    st.outline_pts.clear();
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

/// How far board point `p` is from the strokes of text `i` (mm, the pen's half width
/// taken off), `None` for a text with nothing to draw.
pub fn text_dist(board: &Board, i: usize, p: Pt) -> Option<f64> {
    let t = board.texts.get(i)?;
    if t.hidden {
        return None;
    }
    let fp = t.footprint.and_then(|k| board.footprints.get(k));
    let words = gusset_pcb::font::shown(&t.text, fp.map_or("", |f| &f.reference), fp.map_or("", |f| &f.value));
    let r = gusset_pcb::font::thickness(t) / 2.0;
    gusset_pcb::font::strokes(t, &words)
        .iter()
        .flat_map(|l| {
            if let [q] = l.as_slice() {
                vec![dist_seg(p, *q, *q)]
            } else {
                l.windows(2).filter_map(|w| Some(dist_seg(p, *w.first()?, *w.get(1)?))).collect()
            }
        })
        .min_by(f64::total_cmp)
        .map(|d| d - r)
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
    let text = (0..board.texts.len())
        .filter(|&i| board.texts.get(i).is_some_and(|t| t.footprint.is_none() && shown(&t.layer)))
        .filter_map(|i| text_dist(board, i, p).map(|d| (i, d)))
        .filter(|(_, d)| *d <= tol)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((i, _)) = text {
        return Some(Sel::Text(i));
    }
    if let Some(k) = board.pads.iter().position(|pad| {
        board.copper_layers().iter().any(|l| shown(l) && board.on_layer(&pad.layers, l)) && pad_contains(pad, p, tol)
    }) {
        return Some(Sel::Pad(k));
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

/// Whether board point `p` is on the pad (or within `tol` mm of it).
fn pad_contains(pad: &gusset_pcb::board::Pad, p: Pt, tol: f64) -> bool {
    match geom::pad_contour(pad, 0.0) {
        Some(Contour::Circle { center, radius }) => libm::hypot(center[0] - p[0], center[1] - p[1]) <= radius + tol,
        Some(c) => {
            let poly = c.polygon(0.05);
            geom::inside(&poly, p)
                || poly.iter().zip(poly.iter().cycle().skip(1)).any(|(a, b)| dist_seg(p, *a, *b) <= tol)
        }
        None => false,
    }
}

/// The footprint a pad belongs to.
pub fn pad_owner(board: &Board, pad: usize) -> Option<usize> {
    board.footprints.iter().position(|f| f.pads.contains(&pad))
}

/// `(reference, number)` of a pad, when it has both.
pub fn pad_name(board: &Board, pad: usize) -> Option<(String, String)> {
    let number = board.pads.get(pad)?.number.clone();
    let reference = board.footprints.get(pad_owner(board, pad)?)?.reference.clone();
    (!number.is_empty() && !reference.is_empty()).then_some((reference, number))
}

/// How far a dragged track or via lands from where it is: its start (track) or centre
/// (via) moved by `moved` and snapped to the grid.
pub fn item_drop_delta(board: &Board, item: Sel, moved: Pt, grid: f64) -> Option<Pt> {
    let from = match item {
        Sel::Track(i) => board.tracks.get(i)?.start,
        Sel::Via(i) => board.vias.get(i)?.at,
        _ => return None,
    };
    let to = snap([from[0] + moved[0], from[1] + moved[1]], grid);
    Some([to[0] - from[0], to[1] - from[1]])
}

/// `pcb.move` for a dragged track or via and every other picked track and via.
pub fn move_items_command(st: &State, dragged: Sel, by: Pt) -> Option<(&'static str, Value)> {
    let (tracks, vias) = dragged_items(st, dragged);
    ((!tracks.is_empty() || !vias.is_empty()) && by.iter().any(|v| v.abs() > 1e-9))
        .then(|| ("pcb.move", json!({ "tracks": tracks, "vias": vias, "by": by })))
}

/// The tracks and vias a drag of `dragged` moves: all picked ones when it is one of them.
pub fn dragged_items(st: &State, dragged: Sel) -> (Vec<usize>, Vec<usize>) {
    let mut all: Vec<Sel> = st.selected.into_iter().chain(st.multi.iter().copied()).collect();
    if !all.contains(&dragged) {
        all = vec![dragged];
    }
    let mut tracks: Vec<usize> =
        all.iter().filter_map(|s| if let Sel::Track(i) = s { Some(*i) } else { None }).collect();
    let mut vias: Vec<usize> = all.iter().filter_map(|s| if let Sel::Via(i) = s { Some(*i) } else { None }).collect();
    tracks.sort_unstable();
    tracks.dedup();
    vias.sort_unstable();
    vias.dedup();
    (tracks, vias)
}

/// `pcb.set_outline` for the Outline tool's corners (at least 3).
pub fn outline_command(st: &State) -> Option<(&'static str, Value)> {
    (st.outline_pts.len() >= 3).then(|| ("pcb.set_outline", json!({ "points": st.outline_pts })))
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
        Sel::Text(i) => Some(("pcb.text", json!({ "index": i, "delete": true }))),
        Sel::Footprint(_) | Sel::Pad(_) => None,
    }
}

/// A new text from the Text tool's settings.
pub fn text_add_command(st: &State, at: Pt) -> (&'static str, Value) {
    let layer = if st.text_back { "B.SilkS" } else { "F.SilkS" };
    (
        "pcb.text",
        json!({ "text": st.text_words, "at": at, "layer": layer, "size": st.text_size, "angle": st.text_angle }),
    )
}

/// Changes of the picked text.
pub fn text_edit_command(i: usize, words: &str, size: f64, angle: f64, mirrored: bool) -> (&'static str, Value) {
    ("pcb.text", json!({ "index": i, "text": words, "size": size, "angle": angle, "mirrored": mirrored }))
}

pub fn track_command(a: Pt, b: Pt, layer: &str) -> (&'static str, Value) {
    ("pcb.add_track", json!({ "start": a, "end": b, "layer": layer }))
}

pub fn via_command(at: Pt) -> (&'static str, Value) {
    ("pcb.add_via", json!({ "at": at }))
}

/// A routed path laid as it was previewed (no new search), with arc corners when asked.
pub fn route_command(path: &[Pt], layer: &str, net: Option<&str>, arc_radius: f64) -> (&'static str, Value) {
    let mut v = json!({ "points": path, "layer": layer, "around": false });
    if let Some(m) = v.as_object_mut() {
        if let Some(n) = net {
            m.insert("net".into(), json!(n));
        }
        if arc_radius > 0.0 {
            m.insert("arc_radius".into(), json!(arc_radius));
        }
    }
    ("pcb.route", v)
}

/// `pcb.route` from `a` to `b` searched by the router itself, which shoves one blocking
/// track of another net aside when there is no way round (the Route tool's click where
/// the preview finds none).
pub fn shove_command(a: Pt, b: Pt, layer: &str, net: Option<&str>) -> (&'static str, Value) {
    let mut m = serde_json::Map::new();
    m.insert("points".into(), json!([a, b]));
    m.insert("layer".into(), json!(layer));
    if let Some(n) = net {
        m.insert("net".into(), json!(n));
    }
    ("pcb.route", Value::Object(m))
}

/// The via a layer change while routing places (on the route's net).
pub fn route_via_command(at: Pt, net: Option<&str>) -> (&'static str, Value) {
    match net {
        Some(n) => ("pcb.add_via", json!({ "at": at, "net": n })),
        None => via_command(at),
    }
}

/// Where a route click lands: the centre of a pad of the route's net under `p` (which
/// ends the route), another pad's centre, or the grid point.
pub fn route_target(board: &Board, p: Pt, layer: &str, grid: f64, net: Option<&str>) -> (Pt, bool) {
    for pad in board.pads.iter().filter(|pad| board.on_layer(&pad.layers, layer)) {
        let Some(c) = geom::pad_contour(pad, 0.0) else { continue };
        let inside = match c {
            Contour::Circle { center, radius } => libm::hypot(center[0] - p[0], center[1] - p[1]) <= radius,
            c => geom::inside(&c.polygon(0.05), p),
        };
        if inside {
            let same = net.is_some() && board.net_name(&pad.net).as_deref() == net;
            return (pad.at, same);
        }
    }
    (snap(p, grid), false)
}

/// The net of the copper where a route starts.
pub fn net_at(board: &Board, p: Pt, layer: &str) -> Option<String> {
    board.net_at(p, layer)
}

/// The next copper layer for a layer change while routing (front ↔ back on two layers;
/// on more, the next one down, back to the front from the bottom).
pub fn other_layer(board: &Board, layer: &str) -> Option<String> {
    let cu = board.copper_layers();
    let i = cu.iter().position(|l| l == layer)?;
    if cu.len() < 2 {
        return None;
    }
    cu.get((i + 1) % cu.len()).cloned()
}

/// The route preview from the route's start to `to`: 45° segments around other nets'
/// copper, or `None` when there is no clear way (cached while nothing changes).
pub fn route_preview(open: &OpenBoard, st: &mut State, to: Pt) -> Option<Vec<Pt>> {
    let from = st.track_from?;
    let net = st.route_net.clone();
    let width = open.rules.track_width_of(net.as_deref());
    let key = (open.board_hash.clone(), st.active.clone(), net.clone(), width.to_bits());
    if st.route_cache.as_ref().is_none_or(|c| c.key != key) {
        let obs = gusset_pcb::route::Obstacles::new(&open.board, &open.rules, &st.active, net.as_deref(), width);
        st.route_cache = Some(RouteCache { key, obs, last: None });
    }
    let cache = st.route_cache.as_mut()?;
    if let Some((f, t, d, path)) = &cache.last
        && same_pt(*f, from)
        && same_pt(*t, to)
        && *d == st.diagonal_first
    {
        return path.clone();
    }
    let grid = (open.rules.clearance_of(net.as_deref()).min(width) / 2.0).clamp(0.05, 0.25);
    let path = gusset_pcb::route::walkaround(&cache.obs, from, to, grid, st.diagonal_first);
    cache.last = Some((from, to, st.diagonal_first, path.clone()));
    path
}

fn same_pt(a: Pt, b: Pt) -> bool {
    a[0].to_bits() == b[0].to_bits() && a[1].to_bits() == b[1].to_bits()
}

/// Whether a via of the route's net fits at `at` (its clearances on every layer).
pub fn via_fits(open: &OpenBoard, at: Pt, net: Option<&str>) -> bool {
    let (size, _) = open.rules.via_of(net);
    gusset_pcb::route::via_clear(&open.board, &open.rules, at, net, size)
}

/// The net an item stands for, for highlighting.
pub fn net_of(board: &Board, sel: Sel) -> Option<String> {
    match sel {
        Sel::Track(i) => board.tracks.get(i).and_then(|t| board.net_name(&t.net)),
        Sel::Via(i) => board.vias.get(i).and_then(|v| board.net_name(&v.net)),
        Sel::Pad(i) => board.pads.get(i).and_then(|p| board.net_name(&p.net)),
        Sel::Footprint(_) | Sel::Text(_) => None,
    }
}

/// A fill polygon and its triangles.
pub type FillTris<'a> = (&'a [Pt], std::sync::Arc<Vec<[u32; 3]>>);

/// Triangles of every zone fill on `layer` (cached by polygon contents): (polygon,
/// triangles).
pub fn fill_triangles<'a>(cache: &mut FillCache, board_hash: &str, board: &'a Board, layer: &str) -> Vec<FillTris<'a>> {
    if cache.board != board_hash {
        // Keep triangles of fills the new board still has; drop the rest below.
        cache.board = board_hash.to_string();
        let keep: std::collections::HashSet<u64> =
            board.zones.iter().flat_map(|z| z.fills.iter()).map(|(l, p)| poly_key(l, p)).collect();
        cache.tris.retain(|k, _| keep.contains(k));
    }
    let mut out = Vec::new();
    for z in board.zones.iter().filter(|z| !z.keepout) {
        for (l, p) in z.fills.iter().filter(|(l, p)| l == layer && p.len() >= 3) {
            let k = poly_key(l, p);
            let t = cache
                .tris
                .entry(k)
                .or_insert_with(|| {
                    std::sync::Arc::new(
                        gusset_pcb::poly::triangulate(p, 2_000_000).into_iter().map(|t| t.map(|i| i as u32)).collect(),
                    )
                })
                .clone();
            out.push((p.as_slice(), t));
        }
    }
    out
}

fn poly_key(layer: &str, p: &[Pt]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    layer.hash(&mut h);
    p.len().hash(&mut h);
    for q in p {
        q[0].to_bits().hash(&mut h);
        q[1].to_bits().hash(&mut h);
    }
    h.finish()
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
        self.zoom = ((rect.width() - 40.0) / w).min((rect.height() - 40.0) / h).clamp(0.05, 2000.0).min(40.0);
        let c = Vec2::new((lo[0] + hi[0]) as f32 / 2.0, (lo[1] + hi[1]) as f32 / 2.0);
        self.offset = rect.size() / 2.0 - c * self.zoom;
        self.auto_fit = Some(rect.size());
    }

    fn shown(&self, layer: &str) -> bool {
        !self.hidden.contains(layer)
    }
}

/// The side panel: tools, the active layer, grid, layer visibility, ratsnest and DRC.
pub fn controls(ui: &mut Ui, pal: &Palette, open: Option<&OpenBoard>, st: &mut State) -> Out {
    let mut out = Out::default();
    let Some(open) = open else {
        // The canvas carries the explanation; the panel only offers the way out.
        ui.set_min_width(200.0);
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
        for (t, key, tip) in [
            (Tool::Select, "pcb.select", "pcb.tip_select"),
            (Tool::Route, "pcb.route", "pcb.tip_route"),
            (Tool::Via, "pcb.via", "pcb.tip_via"),
            (Tool::Text, "pcb.text", "pcb.tip_text"),
            (Tool::Place, "pcb.place", "pcb.tip_place"),
            (Tool::Zone, "pcb.zone", "pcb.tip_zone"),
            (Tool::Outline, "pcb.outline", "pcb.tip_outline"),
        ] {
            if ui.selectable_label(st.tool == t, tr(key)).on_hover_text(tr(tip)).clicked() {
                st.tool = t;
                st.track_from = None;
                st.outline_pts.clear();
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
        let is_cu = cu.contains(&l);
        let active = is_cu && st.active == l;
        // The active copper layer: a filled radio dot and a tinted row. Other copper layers
        // show an empty ring (click to make active); non-copper layers have no choice.
        egui::Frame::new()
            .fill(if active { pal.accent.gamma_multiply(0.18) } else { Color32::TRANSPARENT })
            .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_SM))
            .inner_margin(egui::Margin::symmetric(4, 1))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (r, resp) =
                        ui.allocate_exact_size(Vec2::splat(14.0), if is_cu { Sense::click() } else { Sense::hover() });
                    if is_cu {
                        let c = if active { pal.accent } else { pal.text_muted };
                        ui.painter().circle_stroke(r.center(), 5.5, Stroke::new(1.2, c));
                        if active {
                            ui.painter().circle_filled(r.center(), 3.0, c);
                        }
                        resp.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::RadioButton,
                                true,
                                active,
                                format!("{l} {}", tr("pcb.active")),
                            )
                        });
                        if resp.on_hover_text(tr("pcb.tip_active")).clicked() {
                            st.active = l.clone();
                        }
                    }
                    let (sw, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    ui.painter().rect_filled(sw, 2.0, layer_color(&l));
                    let mut on = st.shown(&l);
                    let name =
                        if active { egui::RichText::new(l.as_str()).strong() } else { egui::RichText::new(l.as_str()) };
                    if ui.checkbox(&mut on, name).changed() {
                        if on {
                            st.hidden.remove(&l);
                        } else {
                            st.hidden.insert(l.clone());
                        }
                    }
                });
            });
    }
    ui.checkbox(&mut st.show_ratsnest, tr("pcb.ratsnest"));
    ui.separator();
    ui.label(tr("pcb.library"));
    crate::theme::text_field(
        ui,
        egui::TextEdit::singleline(&mut st.library).hint_text(tr("pcb.library_hint")).desired_width(f32::INFINITY),
    );
    if st.library_read != st.library {
        st.library_read = st.library.clone();
        st.library_names =
            if st.library.trim().is_empty() { Vec::new() } else { library_footprints(&st.library, 20_000) };
    }
    if !st.library_names.is_empty() {
        crate::theme::text_field(
            ui,
            egui::TextEdit::singleline(&mut st.library_filter)
                .hint_text(tr("pcb.filter_hint"))
                .desired_width(f32::INFINITY),
        );
        let filter = st.library_filter.to_lowercase();
        egui::ScrollArea::vertical().id_salt("pcb_lib").max_height(140.0).show(ui, |ui| {
            for n in st.library_names.iter().filter(|n| n.to_lowercase().contains(&filter)).take(500) {
                if ui.selectable_label(st.footprint == *n, egui::RichText::new(n).small()).clicked() {
                    st.footprint = n.clone();
                    st.tool = Tool::Place;
                }
            }
        });
    }
    ui.separator();
    ui.label(tr("pcb.routing"));
    ui.horizontal(|ui| {
        ui.checkbox(&mut st.diagonal_first, tr("pcb.diagonal_first"));
    });
    ui.horizontal(|ui| {
        let mut arcs = st.arc_radius > 0.0;
        if ui.checkbox(&mut arcs, tr("pcb.arc_corners")).changed() {
            st.arc_radius = if arcs { 1.0 } else { 0.0 };
        }
        if arcs {
            ui.add(egui::DragValue::new(&mut st.arc_radius).range(0.05..=20.0).speed(0.05).suffix(" mm"));
        }
    });
    if let Some(n) = &st.route_net {
        ui.weak(format!("{}: {n}", tr("pcb.net")));
    }
    if st.tool == Tool::Text {
        ui.separator();
        ui.label(tr("pcb.text"));
        crate::theme::text_field(
            ui,
            egui::TextEdit::singleline(&mut st.text_words).hint_text(tr("pcb.text_words")).desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut st.text_size).range(0.2..=50.0).speed(0.05).suffix(" mm"));
            ui.add(egui::DragValue::new(&mut st.text_angle).range(-360.0..=360.0).speed(1.0).suffix("°"));
        });
        ui.checkbox(&mut st.text_back, tr("pcb.text_back"));
    }
    if let (Some(Sel::Text(i)), Some(t)) =
        (st.selected, st.selected.and_then(|s| if let Sel::Text(i) = s { b_text(open, i) } else { None }))
    {
        let fresh = st.text_edit.as_ref().is_none_or(|e| e.0 != open.board_hash || e.1 != i);
        if fresh {
            st.text_edit = Some((open.board_hash.clone(), i, t.text.clone(), t.size, t.angle, t.mirrored));
        }
        if let Some(e) = st.text_edit.as_mut() {
            ui.separator();
            ui.label(tr("pcb.text"));
            crate::theme::text_field(ui, egui::TextEdit::singleline(&mut e.2).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut e.3).range(0.2..=50.0).speed(0.05).suffix(" mm"));
                ui.add(egui::DragValue::new(&mut e.4).range(-360.0..=360.0).speed(1.0).suffix("°"));
            });
            ui.checkbox(&mut e.5, tr("pcb.text_back"));
            if ui.add_enabled(!e.2.trim().is_empty(), egui::Button::new(tr("pcb.text_apply"))).clicked() {
                out.commands.push(text_edit_command(i, &e.2, e.3, e.4, e.5));
            }
        }
    }
    if st.tool == Tool::Zone {
        ui.separator();
        ui.label(tr("pcb.zone"));
        ui.horizontal(|ui| {
            ui.label(tr("pcb.net"));
            crate::theme::text_field(
                ui,
                egui::TextEdit::singleline(&mut st.zone_net).hint_text(tr("pcb.no_net")).desired_width(120.0),
            );
        });
        ui.checkbox(&mut st.zone_hatch, tr("pcb.hatched"));
        if st.zone_hatch {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut st.hatch_thickness).range(0.05..=10.0).speed(0.05).suffix(" mm"));
                ui.add(egui::DragValue::new(&mut st.hatch_gap).range(0.05..=10.0).speed(0.05).suffix(" mm"));
            });
        }
        ui.weak(format!("{}: {}", tr("pcb.zone_corners"), st.zone_pts.len()));
    }
    ui.separator();
    nets_panel(ui, open, st, &mut out);
    ui.separator();
    if ui.button(tr("pcb.fill_zones")).clicked() {
        out.commands.push(("pcb.fill_zones", json!({})));
    }
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

/// The panel's net editing: the picked footprint's pad nets, renaming the highlighted
/// net, net classes, and the autorouter.
fn b_text(open: &OpenBoard, i: usize) -> Option<&gusset_pcb::board::Text> {
    open.board.texts.get(i)
}

fn nets_panel(ui: &mut Ui, open: &OpenBoard, st: &mut State, out: &mut Out) {
    let b = &open.board;
    ui.label(tr("pcb.nets"));
    if let Some(Sel::Footprint(i)) = st.selected {
        let fresh = st.pad_nets.as_ref().is_none_or(|(h, k, _)| *h != open.board_hash || *k != i);
        if fresh {
            st.pad_nets = Some((open.board_hash.clone(), i, pad_nets_of(b, i)));
        }
        let reference = b.footprints.get(i).map(|f| f.reference.clone()).unwrap_or_default();
        let now = pad_nets_of(b, i);
        if let Some((_, _, rows)) = st.pad_nets.as_mut() {
            egui::Grid::new("pcb_pad_nets").num_columns(2).show(ui, |ui| {
                for (k, (num, net)) in rows.iter_mut().enumerate().take(200) {
                    ui.label(format!("{reference}.{num}"));
                    let r = crate::theme::text_field(
                        ui,
                        egui::TextEdit::singleline(net).hint_text(tr("pcb.no_net")).desired_width(110.0),
                    );
                    let was = now.get(k).map(|x| x.1.as_str()).unwrap_or("");
                    if r.lost_focus() && net.trim() != was {
                        out.commands.push(pad_net_command(&reference, num, net));
                    }
                    ui.end_row();
                }
            });
        }
    }
    if let Some(Sel::Pad(k)) = st.selected
        && let Some((reference, number)) = pad_name(b, k)
    {
        let now = b.pads.get(k).and_then(|p| b.net_name(&p.net)).unwrap_or_default();
        if st.pad_edit.as_ref().is_none_or(|(h, i, _)| *h != open.board_hash || *i != k) {
            st.pad_edit = Some((open.board_hash.clone(), k, now.clone()));
        }
        if let Some((_, _, net)) = st.pad_edit.as_mut() {
            ui.horizontal(|ui| {
                ui.label(format!("{}: {reference}.{number}", tr("pcb.pad")));
                let r = crate::theme::text_field(
                    ui,
                    egui::TextEdit::singleline(net).hint_text(tr("pcb.no_net")).desired_width(110.0),
                );
                let send = r.lost_focus() && net.trim() != now;
                if (ui.add_enabled(net.trim() != now, egui::Button::new(tr("pcb.text_apply"))).clicked() || send)
                    && net.trim() != now
                {
                    out.commands.push(pad_net_command(&reference, &number, net));
                }
            });
        }
    }
    if let Some(h) = st.highlight.clone() {
        ui.horizontal(|ui| {
            crate::theme::text_field(
                ui,
                egui::TextEdit::singleline(&mut st.rename_to).hint_text(h.as_str()).desired_width(110.0),
            );
            let to = st.rename_to.trim().to_string();
            if ui.add_enabled(!to.is_empty() && to != h, egui::Button::new(tr("pcb.rename_net"))).clicked() {
                out.commands.push(("pcb.rename_net", json!({ "from": h, "to": to })));
                st.highlight = Some(to);
                st.rename_to.clear();
            }
        });
    }
    // Net classes: pick one (or type a new name), set its sizes, add the highlighted net.
    egui::CollapsingHeader::new(tr("pcb.net_classes")).id_salt("pcb_classes").show(ui, |ui| {
        let names: Vec<String> = std::iter::once("Default".to_string())
            .chain(open.rules.classes.iter().map(|c| c.name.clone()).filter(|n| n != "Default"))
            .collect();
        ui.horizontal_wrapped(|ui| {
            for n in &names {
                if ui.selectable_label(st.class.name == *n, n.as_str()).clicked() {
                    let c = open.rules.classes.iter().find(|c| c.name == *n);
                    st.class = ClassEdit {
                        name: n.clone(),
                        clearance: c.and_then(|c| c.clearance).unwrap_or(open.rules.clearance),
                        track_width: c.and_then(|c| c.track_width).unwrap_or(open.rules.track_width),
                        via_diameter: c.and_then(|c| c.via_diameter).unwrap_or(open.rules.via_diameter),
                        via_drill: c.and_then(|c| c.via_drill).unwrap_or(open.rules.via_drill),
                    };
                }
            }
        });
        crate::theme::text_field(ui, egui::TextEdit::singleline(&mut st.class.name).desired_width(f32::INFINITY));
        egui::Grid::new("pcb_class_sizes").num_columns(2).show(ui, |ui| {
            for (key, v) in [
                ("pcb.clearance", &mut st.class.clearance),
                ("pcb.track_width", &mut st.class.track_width),
                ("pcb.via_size", &mut st.class.via_diameter),
                ("pcb.via_drill", &mut st.class.via_drill),
            ] {
                ui.label(tr(key));
                ui.add(egui::DragValue::new(v).range(0.0..=10.0).speed(0.01).suffix(" mm"));
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            if ui.button(tr("pcb.apply")).clicked()
                && let Some(c) = class_command(&st.class, &[])
            {
                out.commands.push(c);
            }
            if let Some(h) = &st.highlight
                && ui.button(format!("+ {h}")).on_hover_text(tr("pcb.tip_add_to_class")).clicked()
                && let Some(c) = class_command(&st.class, std::slice::from_ref(h))
            {
                out.commands.push(c);
            }
            let known = open.rules.classes.iter().any(|c| c.name == st.class.name);
            if st.class.name != "Default" && known && ui.button(tr("pcb.delete_class")).clicked() {
                out.commands.push(("pcb.net_class", json!({ "name": st.class.name, "delete": true })));
            }
        });
    });
    // The autorouter: the user's own Freerouting, never downloaded.
    egui::CollapsingHeader::new(tr("pcb.autorouter")).id_salt("pcb_autorouter").show(ui, |ui| {
        ui.weak(tr("pcb.freerouting_hint"));
        crate::theme::text_field(
            ui,
            egui::TextEdit::singleline(&mut st.freerouting)
                .hint_text(tr("pcb.freerouting_path"))
                .desired_width(f32::INFINITY),
        );
        if st.freerouting.to_ascii_lowercase().ends_with(".jar") {
            crate::theme::text_field(
                ui,
                egui::TextEdit::singleline(&mut st.freerouting_java).hint_text("java").desired_width(f32::INFINITY),
            );
        }
        ui.horizontal(|ui| {
            ui.label(tr("pcb.time_limit"));
            ui.add(egui::DragValue::new(&mut st.freerouting_timeout).range(5..=86_400).suffix(" s"));
        });
        let ready = !st.freerouting.trim().is_empty();
        if ui
            .add_enabled(ready, egui::Button::new(tr("pcb.autoroute")))
            .on_disabled_hover_text(tr("pcb.freerouting_hint"))
            .clicked()
        {
            out.commands.push(("pcb.autoroute", json!({})));
        }
    });
}

/// The board tree in the left panel (instead of the solid-model browser): footprints, nets
/// and layers. A click on a footprint picks it, on a net highlights it, on a layer shows or
/// hides it.
pub fn tree(ui: &mut Ui, pal: &Palette, open: Option<&OpenBoard>, st: &mut State) {
    let Some(open) = open else {
        ui.weak(tr("tree.none_pcb"));
        return;
    };
    let b = &open.board;
    egui::CollapsingHeader::new(format!("{} ({})", tr("tree.footprints"), b.footprints.len()))
        .id_salt("tree_fp")
        .default_open(true)
        .show(ui, |ui| {
            for (i, f) in b.footprints.iter().enumerate() {
                let label = format!("{}  {}", f.reference, f.value);
                if ui.selectable_label(is_picked(st, Sel::Footprint(i)), label).clicked() {
                    st.selected = Some(Sel::Footprint(i));
                    st.multi.clear();
                }
            }
        });
    egui::CollapsingHeader::new(format!(
        "{} ({})",
        tr("tree.nets"),
        b.nets.iter().filter(|n| !n.name.is_empty()).count()
    ))
    .id_salt("tree_nets")
    .default_open(true)
    .show(ui, |ui| {
        for n in b.nets.iter().filter(|n| !n.name.is_empty()).take(300) {
            let on = st.highlight.as_deref() == Some(n.name.as_str());
            if ui.selectable_label(on, &n.name).clicked() {
                st.highlight = if on { None } else { Some(n.name.clone()) };
            }
        }
    });
    egui::CollapsingHeader::new(format!("{} ({})", tr("pcb.layers"), b.copper_layers().len()))
        .id_salt("tree_layers")
        .default_open(false)
        .show(ui, |ui| {
            for l in panel_layers(b) {
                ui.horizontal(|ui| {
                    let (sw, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    ui.painter().rect_filled(sw, 2.0, layer_color(&l));
                    let shown = st.shown(&l);
                    let t = egui::RichText::new(l.as_str()).color(if shown { pal.text } else { pal.text_muted });
                    if ui.add(egui::Label::new(t).sense(Sense::click())).clicked() {
                        if shown {
                            st.hidden.insert(l.clone());
                        } else {
                            st.hidden.remove(&l);
                        }
                    }
                });
            }
        });
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
            // The canvas keeps its own colours in both themes, so the hint does too.
            Color32::from_gray(150),
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
        Sel::Pad(i) => i < b.pads.len(),
        Sel::Footprint(i) => i < b.footprints.len(),
        Sel::Track(i) => i < b.tracks.len(),
        Sel::Via(i) => i < b.vias.len(),
        Sel::Text(i) => i < b.texts.len(),
    });
    if resp.double_clicked() && st.tool == Tool::Select || st.auto_fit.is_some_and(|s| s != rect.size()) {
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
            st.auto_fit = None;
        }
    }
    let map = Map { origin: rect.min + st.offset, zoom: st.zoom };
    let tol = f64::from(4.0 / st.zoom);
    let pointer = resp.hover_pos().map(|p| map.from(p));
    let hidden = st.hidden.clone();
    let shown = |l: &str| !hidden.contains(l);

    // Drags: a footprint under the pointer moves (Select tool), anything else pans.
    let middle = ui.input(|i| i.pointer.middle_down());
    let shift = ui.input(|i| i.modifiers.shift);
    if resp.drag_started()
        && st.tool == Tool::Select
        && shift
        && !middle
        && let Some(p) = ui.input(|i| i.pointer.press_origin()).map(|p| map.from(p))
    {
        st.boxing = Some(p);
    }
    if st.boxing.is_some() && resp.drag_stopped() {
        if let (Some(a), Some(b2)) = (st.boxing.take(), pointer) {
            for x in in_box(b, a, b2, &shown) {
                if !is_picked(st, x) {
                    st.multi.push(x);
                }
            }
        }
    } else if resp.drag_started() && st.boxing.is_none() {
        let press = ui.input(|i| i.pointer.press_origin()).map(|p| map.from(p));
        let under = match (st.tool, middle, press) {
            (Tool::Select, false, Some(p)) => hit(b, p, tol, &shown),
            _ => None,
        };
        // A pad drags its footprint.
        let fp = match under {
            Some(Sel::Footprint(i)) => Some(i),
            Some(Sel::Pad(k)) => pad_owner(b, k),
            _ => None,
        };
        let item = under.filter(|s| matches!(s, Sel::Track(_) | Sel::Via(_)));
        if let Some(i) = fp {
            st.selected = Some(Sel::Footprint(i));
        } else if let Some(x) = item
            && !is_picked(st, x)
        {
            st.multi.clear();
            st.selected = Some(x);
        }
        st.drag = Some(Drag { footprint: fp, item, moved: Vec2::ZERO });
    }
    if resp.dragged() && st.boxing.is_none() {
        let d = resp.drag_delta();
        match st.drag.as_mut() {
            Some(Drag { footprint: Some(_), moved, .. } | Drag { item: Some(_), moved, .. }) => *moved += d / st.zoom,
            _ => {
                st.offset += d;
                st.auto_fit = None;
            }
        }
    }
    if resp.drag_stopped() {
        if let Some(Drag { item: Some(x), moved, .. }) = st.drag.clone()
            && moved.length() > 1e-3
            && let Some(by) = item_drop_delta(b, x, [f64::from(moved.x), f64::from(moved.y)], st.grid)
        {
            out.commands.extend(move_items_command(st, x, by));
        }
        if let Some(Drag { footprint: Some(i), moved, .. }) = st.drag.take()
            && moved.length() > 1e-3
            && let Some(at) = drop_place(b, i, [f64::from(moved.x), f64::from(moved.y)], st.grid)
        {
            out.commands.push(move_command(i, at));
            // The other picked footprints move by as much.
            if st.multi.contains(&Sel::Footprint(i)) {
                let from = b.footprints.get(i).map_or(at, |f| f.at);
                let d = [at[0] - from[0], at[1] - from[1]];
                for &x in &st.multi {
                    if let Sel::Footprint(j) = x
                        && j != i
                        && let Some(f) = b.footprints.get(j)
                    {
                        out.commands.push(move_command(j, [f.at[0] + d[0], f.at[1] + d[1]]));
                    }
                }
            }
        }
    } else if !resp.dragged() {
        st.drag = None;
    }

    // Clicks.
    if resp.clicked()
        && let Some(p) = pointer
    {
        match st.tool {
            Tool::Select => {
                let h = hit(b, p, tol, &shown);
                if shift {
                    // Shift-click adds or takes away.
                    if let Some(x) = h {
                        if let Some(k) = st.multi.iter().position(|m| *m == x) {
                            st.multi.remove(k);
                        } else if st.selected != Some(x) {
                            st.multi.push(x);
                        }
                    }
                } else {
                    st.multi.clear();
                    st.selected = h;
                    st.highlight = st.selected.and_then(|s| net_of(b, s));
                }
            }
            Tool::Via => out.commands.push(via_command(snap(p, st.grid))),
            Tool::Text => match hit(b, p, tol, &shown) {
                Some(x @ Sel::Text(_)) => st.selected = Some(x),
                _ if st.text_words.trim().is_empty() => {}
                _ => out.commands.push(text_add_command(st, snap(p, st.grid))),
            },
            Tool::Place => out.commands.extend(place_command(st, snap(p, st.grid))),
            Tool::Zone => {
                let q = snap(p, st.grid);
                // A click on the first corner closes the zone.
                let closes =
                    st.zone_pts.len() >= 3 && st.zone_pts.first().is_some_and(|f| map.to(*f).distance(map.to(q)) < 6.0);
                if closes {
                    out.commands.extend(zone_command(st));
                    st.zone_pts.clear();
                } else if st.zone_pts.last() != Some(&q) {
                    st.zone_pts.push(q);
                }
            }
            Tool::Outline => {
                let q = snap(p, st.grid);
                // A click on the first corner closes the outline.
                let closes = st.outline_pts.len() >= 3
                    && st.outline_pts.first().is_some_and(|f| map.to(*f).distance(map.to(q)) < 6.0);
                if closes {
                    out.commands.extend(outline_command(st));
                    st.outline_pts.clear();
                } else if st.outline_pts.last() != Some(&q) {
                    st.outline_pts.push(q);
                }
            }
            Tool::Route => match st.track_from {
                None => {
                    let (q, _) = route_target(b, p, &st.active, st.grid, None);
                    st.route_net = net_at(b, q, &st.active);
                    st.highlight = st.route_net.clone();
                    st.track_from = Some(q);
                }
                Some(a) => {
                    let (q, finish) = route_target(b, p, &st.active, st.grid, st.route_net.as_deref());
                    if libm::hypot(q[0] - a[0], q[1] - a[1]) > 1e-6 {
                        match route_preview(open, st, q) {
                            Some(path) => {
                                out.commands.push(route_command(
                                    &path,
                                    &st.active,
                                    st.route_net.as_deref(),
                                    st.arc_radius,
                                ));
                                if finish {
                                    end_route(st);
                                } else {
                                    st.track_from = Some(q);
                                }
                            }
                            None => {
                                // No way round: the router may shove another net's track
                                // aside; the route ends there either way.
                                out.commands.push(shove_command(a, q, &st.active, st.route_net.as_deref()));
                                end_route(st);
                            }
                        }
                    }
                }
            },
        }
    }
    let enter = resp.hovered() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if st.tool == Tool::Zone && (resp.double_clicked() || enter) {
        out.commands.extend(zone_command(st));
        st.zone_pts.clear();
    }
    if st.tool == Tool::Outline && (resp.double_clicked() || enter) {
        out.commands.extend(outline_command(st));
        st.outline_pts.clear();
    }
    if resp.secondary_clicked() || (resp.double_clicked() && st.tool == Tool::Route) {
        end_route(st);
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
        let (v, slash) = ui.input(|i| (i.key_pressed(egui::Key::V), i.key_pressed(egui::Key::Slash)));
        if esc {
            end_route(st);
            st.selected = None;
            st.multi.clear();
            st.highlight = None;
        }
        if slash {
            st.diagonal_first = !st.diagonal_first;
        }
        // A layer change while routing: a via where the route stands, then the other side.
        if v && st.tool == Tool::Route
            && let Some(a) = st.track_from
            && let Some(next) = other_layer(b, &st.active)
        {
            if via_fits(open, a, st.route_net.as_deref()) {
                out.commands.push(route_via_command(a, st.route_net.as_deref()));
                st.active = next;
            } else {
                out.blocked = Some(a);
            }
        }
        if del && let Some(c) = delete_picked(st) {
            out.commands.push(c);
            st.selected = None;
            st.multi.clear();
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
    let moving = st
        .drag
        .as_ref()
        .and_then(|d| drag_preview(st, b, d.item, d.footprint, [f64::from(d.moved.x), f64::from(d.moved.y)]));
    paint(&painter, &map, open, st, dragged, moving.as_ref());

    // Route preview: the way around, or a thin red line when there is none.
    if let (Tool::Route, Some(a), Some(p)) = (st.tool, st.track_from, pointer) {
        let (q, _) = route_target(b, p, &st.active, st.grid, st.route_net.as_deref());
        let width = open.rules.track_width_of(st.route_net.as_deref());
        let w = (width as f32 * map.zoom).max(1.0);
        let c = layer_color(&st.active).gamma_multiply(0.8);
        match route_preview(open, st, q) {
            Some(path) => {
                let pts: Vec<Pos2> = path.iter().map(|x| map.to(*x)).collect();
                for s in pts.windows(2) {
                    if let [u, v] = s {
                        stroke_line(&painter, *u, *v, w, c);
                    }
                }
            }
            None => {
                painter.line_segment([map.to(a), map.to(q)], Stroke::new(1.5, Color32::from_rgb(255, 60, 60)));
            }
        }
        painter.circle_filled(map.to(q), w / 2.0, c);
    }
    if st.tool == Tool::Zone && !st.zone_pts.is_empty() {
        let c = layer_color(&st.active);
        let mut pts: Vec<Pos2> = st.zone_pts.iter().map(|x| map.to(*x)).collect();
        if let Some(p) = pointer {
            pts.push(map.to(snap(p, st.grid)));
        }
        if let Some(f) = pts.first().copied() {
            pts.push(f);
        }
        painter.add(Shape::line(pts, Stroke::new(1.5, c)));
        for q in &st.zone_pts {
            painter.circle_filled(map.to(*q), 3.0, c);
        }
    }
    if st.tool == Tool::Outline && !st.outline_pts.is_empty() {
        let c = layer_color("Edge.Cuts");
        let mut pts: Vec<Pos2> = st.outline_pts.iter().map(|x| map.to(*x)).collect();
        if let Some(p) = pointer {
            pts.push(map.to(snap(p, st.grid)));
        }
        painter.add(Shape::line(pts, Stroke::new(1.5, c)));
        for q in &st.outline_pts {
            painter.circle_filled(map.to(*q), 3.0, c);
        }
    }
    if let (Some(a), Some(p)) = (st.boxing, pointer) {
        let r = Rect::from_two_pos(map.to(a), map.to(p));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(120, 200, 255)), egui::StrokeKind::Middle);
    }
    if let Some(at) = out.blocked {
        painter.circle_stroke(map.to(at), 10.0, Stroke::new(2.0, Color32::from_rgb(255, 60, 60)));
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

/// Tracks, vias, the offset (board mm) of a drag in progress and the ends of other tracks
/// that follow it (stretching their tracks), as `pcb.move` and `pcb.move_footprint` do.
pub type Moving = (Vec<usize>, Vec<usize>, Pt, Vec<(usize, [bool; 2])>);

/// What a drag of a track or via (`item`) or of a footprint, by `moved` mm, would move.
pub fn drag_preview(st: &State, b: &Board, item: Option<Sel>, footprint: Option<usize>, moved: Pt) -> Option<Moving> {
    if let Some(x) = item {
        let by = item_drop_delta(b, x, moved, st.grid)?;
        let (t, v) = dragged_items(st, x);
        let places: Vec<Pt> = v.iter().filter_map(|&i| b.vias.get(i)).map(|v| v.at).collect();
        let ends = b.followers(&t, &places);
        return Some((t, v, by, ends));
    }
    // A dragged footprint carries the track ends on its pads.
    let i = footprint?;
    let to = drop_place(b, i, moved, st.grid)?;
    let at = b.footprints.get(i)?.at;
    let by = [to[0] - at[0], to[1] - at[1]];
    let mut ends: Vec<(usize, [bool; 2])> = Vec::new();
    for (t, k, _) in b.pad_ends(&b.footprint_pads(i)) {
        match ends.iter_mut().find(|(j, _)| *j == t) {
            Some((_, e)) => e[k] = true,
            None => ends.push((t, [k == 0, k == 1])),
        }
    }
    Some((Vec::new(), Vec::new(), by, ends))
}

fn paint(
    painter: &egui::Painter,
    map: &Map,
    open: &OpenBoard,
    st: &mut State,
    dragged: Option<(usize, Vec2)>,
    moving: Option<&Moving>,
) {
    let b = &open.board;
    // Tracks and vias being dragged are drawn where they would land; track ends on a
    // dragged via follow it.
    let shifted = |p: Pt, d: Pt| [p[0] + d[0], p[1] + d[1]];
    let end_of = |i: usize, is_start: bool, p: Pt| -> Pt {
        let Some((tracks, _, by, ends)) = moving else { return p };
        let follows = tracks.contains(&i) || ends.iter().any(|(j, e)| *j == i && e[usize::from(!is_start)]);
        if follows { shifted(p, *by) } else { p }
    };
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
            // Zone fills, filled (triangulated) and a little darker than the tracks.
            let fill_c = c.gamma_multiply(0.6);
            for (poly, tris) in fill_triangles(&mut st.fill_cache, &open.board_hash, b, &layer) {
                let mesh = egui::Mesh {
                    vertices: poly
                        .iter()
                        .map(|p| egui::epaint::Vertex { pos: map.to(*p), uv: egui::epaint::WHITE_UV, color: fill_c })
                        .collect(),
                    indices: tris.iter().flatten().copied().collect(),
                    ..Default::default()
                };
                painter.add(Shape::mesh(mesh));
            }
        }
        for g in b.graphics.iter().filter(|g| g.layer == layer) {
            paint_shape(painter, map, &g.shape, g.width, g.filled, c, off_of(g.footprint));
        }
        // Texts in Gusset's own stroke font (as plotted in the Gerbers), placed, turned and
        // mirrored as the text says.
        for (ti, t) in b.texts.iter().enumerate().filter(|(_, t)| t.layer == layer && !t.hidden && !t.text.is_empty()) {
            if t.size as f32 * map.zoom < 3.0 {
                continue;
            }
            let fp = t.footprint.and_then(|k| b.footprints.get(k));
            let words = gusset_pcb::font::shown(&t.text, fp.map_or("", |f| &f.reference), fp.map_or("", |f| &f.value));
            let col = if is_picked(st, Sel::Text(ti)) { Color32::WHITE } else { c };
            let w = (gusset_pcb::font::thickness(t) as f32 * map.zoom).max(1.0);
            let off = off_of(t.footprint);
            for line in gusset_pcb::font::strokes(t, &words) {
                let pts: Vec<Pos2> = line.iter().map(|q| map.to(*q) + off).collect();
                match pts.as_slice() {
                    [q] => {
                        painter.circle_filled(*q, w / 2.0, col);
                    }
                    _ => {
                        painter.add(Shape::line(pts, Stroke::new(w, col)));
                    }
                }
            }
        }
        if !is_cu {
            continue;
        }
        let lit = |net: &gusset_pcb::board::NetRef, c: Color32| match &st.highlight {
            Some(h) if b.net_name(net).as_deref() == Some(h.as_str()) => c.lerp_to_gamma(Color32::WHITE, 0.45),
            Some(_) => c.gamma_multiply(0.45),
            None => c,
        };
        for (i, t) in b.tracks.iter().enumerate().filter(|(_, t)| t.layer == layer) {
            let w = (t.width as f32 * map.zoom).max(1.0);
            let col = if is_picked(st, Sel::Track(i)) { Color32::WHITE } else { lit(&t.net, c) };
            let (ts, te) = (end_of(i, true, t.start), end_of(i, false, t.end));
            match t.mid {
                Some(m) => {
                    let m = if moving.is_some_and(|x| x.0.contains(&i)) { end_of(i, true, m) } else { m };
                    let pts: Vec<Pos2> = geom::arc_points(ts, m, te, f64::from(0.5 / map.zoom).max(1e-3))
                        .into_iter()
                        .map(|p| map.to(p))
                        .collect();
                    painter.add(Shape::line(pts, Stroke::new(w, col)));
                }
                None => {
                    let (a, e) = (map.to(ts), map.to(te));
                    if view.expand(w).intersects(Rect::from_two_pos(a, e)) {
                        stroke_line(painter, a, e, w, col);
                    }
                }
            }
        }
        for (k, pad) in b.pads.iter().enumerate().filter(|(_, p)| b.on_layer(&p.layers, &layer)) {
            let off = off_of(pad_fp.get(k).copied().flatten());
            let selected = is_picked(st, Sel::Pad(k))
                || pad_fp.get(k).copied().flatten().is_some_and(|i| is_picked(st, Sel::Footprint(i)));
            let col = if selected { Color32::WHITE.gamma_multiply(0.9) } else { lit(&pad.net, c) };
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
        let col = if is_picked(st, Sel::Via(i)) { Color32::WHITE } else { via_c.gamma_multiply(0.8) };
        let at = match moving {
            Some((_, vias, by, _)) if vias.contains(&i) => shifted(v.at, *by),
            _ => v.at,
        };
        painter.circle_filled(map.to(at), v.size as f32 / 2.0 * map.zoom, col);
        painter.circle_filled(map.to(at), (v.drill as f32 / 2.0 * map.zoom).max(0.5), Color32::from_rgb(0, 16, 35));
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
