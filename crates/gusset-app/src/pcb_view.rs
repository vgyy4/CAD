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
//! the other way; Esc, a right click or a double click ends. The picked item's net is
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
    /// Puts down the footprint picked in the panel's library list.
    Place,
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

/// Ends the route in progress.
pub fn end_route(st: &mut State) {
    st.track_from = None;
    st.route_net = None;
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
        Sel::Footprint(_) => None,
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
            (Tool::Place, "pcb.place", "pcb.tip_place"),
        ] {
            if ui.selectable_label(st.tool == t, tr(key)).on_hover_text(tr(tip)).clicked() {
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
            if cu.contains(&l)
                && ui.selectable_label(st.active == l, tr("pcb.active")).on_hover_text(tr("pcb.tip_active")).clicked()
            {
                st.active = l.clone();
            }
        });
    }
    ui.checkbox(&mut st.show_ratsnest, tr("pcb.ratsnest"));
    ui.separator();
    ui.label(tr("pcb.library"));
    ui.add(egui::TextEdit::singleline(&mut st.library).hint_text(tr("pcb.library_hint")).desired_width(f32::INFINITY));
    if st.library_read != st.library {
        st.library_read = st.library.clone();
        st.library_names =
            if st.library.trim().is_empty() { Vec::new() } else { library_footprints(&st.library, 20_000) };
    }
    if !st.library_names.is_empty() {
        ui.add(
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
    if resp.dragged() && st.boxing.is_none() {
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
            Tool::Place => out.commands.extend(place_command(st, snap(p, st.grid))),
            Tool::Route => match st.track_from {
                None => {
                    let (q, _) = route_target(b, p, &st.active, st.grid, None);
                    st.route_net = net_at(b, q, &st.active);
                    st.highlight = st.route_net.clone();
                    st.track_from = Some(q);
                }
                Some(a) => {
                    let (q, finish) = route_target(b, p, &st.active, st.grid, st.route_net.as_deref());
                    if libm::hypot(q[0] - a[0], q[1] - a[1]) > 1e-6
                        && let Some(path) = route_preview(open, st, q)
                    {
                        out.commands.push(route_command(&path, &st.active, st.route_net.as_deref(), st.arc_radius));
                        if finish {
                            end_route(st);
                        } else {
                            st.track_from = Some(q);
                        }
                    }
                }
            },
        }
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
    paint(&painter, &map, open, st, dragged);

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

fn paint(painter: &egui::Painter, map: &Map, open: &OpenBoard, st: &mut State, dragged: Option<(usize, Vec2)>) {
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
        // Texts, in the UI font at their height and angle (centred on their place).
        for t in b.texts.iter().filter(|t| t.layer == layer && !t.hidden && !t.text.is_empty()) {
            let px = t.size as f32 * map.zoom * 1.35;
            if !(4.0..=400.0).contains(&px) {
                continue;
            }
            let shown_text: String = t.text.chars().filter(|ch| !ch.is_control()).take(200).collect();
            let galley = painter.layout_no_wrap(shown_text, FontId::proportional(px), c);
            let a = -(t.angle as f32).to_radians();
            let half = galley.size() / 2.0;
            let rot = |v: Vec2| Vec2::new(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos());
            let pos = map.to(t.at) + off_of(t.footprint) - rot(half);
            painter.add(egui::epaint::TextShape::new(pos, galley, c).with_angle(a));
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
            let selected = pad_fp.get(k).copied().flatten().is_some_and(|i| is_picked(st, Sel::Footprint(i)));
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
