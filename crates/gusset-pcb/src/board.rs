//! The typed board model, read from the S-expression tree of a `.kicad_pcb` or
//! `.kicad_mod` file (KiCad 5 to 10 layouts, from the published format description and
//! sample files).
//!
//! The tree stays the source of truth: [`Board`] keeps it, the typed view is derived from
//! it, and [`Board::write`] writes the tree back (so everything Gusset does not model
//! survives a round trip). Coordinates are the file's: millimetres, X right, **Y down**;
//! angles in degrees, counter-clockwise as seen on screen.

use crate::sexpr::{self, Node};
use std::fmt;

/// A point in board coordinates (mm, Y down).
pub type Pt = [f64; 2];

/// Largest coordinate accepted (mm): a board of 10 m is far past anything real.
const MAX_COORD: f64 = 10_000.0;

/// Why a board could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Sexpr(sexpr::Error),
    /// Not a board or footprint file (the top-level list's head).
    NotABoard(String),
    /// Past a count limit (footprints, pads, tracks...).
    TooMany(&'static str),
    /// An edit that cannot be made (bad argument, missing item).
    Edit(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sexpr(e) => write!(f, "{e}"),
            Error::NotABoard(h) => {
                let h: String = h.chars().filter(|c| c.is_ascii_graphic()).take(40).collect();
                write!(f, "not a KiCad board or footprint (the file starts with '{h}')")
            }
            Error::TooMany(what) => write!(f, "the board has more {what} than the reader's limit"),
            Error::Edit(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<sexpr::Error> for Error {
    fn from(e: sexpr::Error) -> Self {
        Error::Sexpr(e)
    }
}

/// Bounds on what is read (inputs are untrusted).
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub sexpr: sexpr::Limits,
    /// Most items of one kind (footprints, pads, tracks, vias, graphics, zone points).
    pub max_items: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { sexpr: sexpr::Limits::default(), max_items: 2_000_000 }
    }
}

/// What a layer is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Signal,
    Power,
    Mixed,
    Jumper,
    User,
}

/// A layer of the board's `(layers ...)` table.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// The ordinal as written (KiCad renumbered layers in version 9).
    pub ordinal: i64,
    /// Canonical name (`F.Cu`, `In1.Cu`, `Edge.Cuts`).
    pub name: String,
    pub kind: LayerKind,
    /// The name the user gave it, if any.
    pub user_name: Option<String>,
}

/// One layer of the physical stackup (`(setup (stackup ...))`).
#[derive(Debug, Clone, PartialEq)]
pub struct StackupLayer {
    pub name: String,
    /// `copper`, `core`, `prepreg`, `Top Solder Mask`...
    pub kind: String,
    /// mm; 0 when not given.
    pub thickness: f64,
    pub material: Option<String>,
    pub epsilon_r: Option<f64>,
}

/// A net (`(net 3 "GND")`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Net {
    pub id: i64,
    pub name: String,
}

/// A reference to a net from a pad, track, via or zone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NetRef {
    /// The number, when the file gives one (KiCad 10 may write only names).
    pub id: Option<i64>,
    pub name: Option<String>,
}

impl NetRef {
    /// No net (net 0 or none written).
    pub fn is_none(&self) -> bool {
        self.id.is_none_or(|i| i == 0) && self.name.as_deref().is_none_or(str::is_empty)
    }
}

/// The geometry of a drawing item, in board coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line {
        a: Pt,
        b: Pt,
    },
    /// Through three points (KiCad 6+ writes start/mid/end; KiCad 5 arcs are converted).
    Arc {
        start: Pt,
        mid: Pt,
        end: Pt,
    },
    Circle {
        center: Pt,
        radius: f64,
    },
    /// A closed polygon (rectangles are stored as four corners).
    Poly {
        pts: Vec<Pt>,
    },
}

/// A drawing on a layer: board outline, silkscreen, copper graphics...
#[derive(Debug, Clone, PartialEq)]
pub struct Graphic {
    pub shape: Shape,
    pub layer: String,
    /// Stroke width (mm); 0 for filled shapes without outline.
    pub width: f64,
    pub filled: bool,
    /// Index of the footprint it belongs to.
    pub footprint: Option<usize>,
}

/// A text item (kept for information; not plotted yet).
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub text: String,
    pub at: Pt,
    pub layer: String,
    pub footprint: Option<usize>,
}

/// What a pad is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadKind {
    Smd,
    /// Plated through hole.
    Tht,
    /// Non-plated hole.
    Npth,
    /// Copper without paste or mask opening (edge connectors).
    Connect,
}

/// The pad's copper shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PadShape {
    Circle,
    Rect,
    Oval,
    /// Corner radius as a ratio of the smaller side (0..0.5).
    RoundRect {
        ratio: f64,
    },
    /// `rect_delta` (dx, dy) of a trapezoid.
    Trapezoid {
        delta: Pt,
    },
    /// A custom pad: its anchor shape here, its primitives in [`Pad::primitives`].
    Custom {
        anchor_circle: bool,
    },
}

/// A drill: round, or an oval slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drill {
    /// Size along the pad's X and Y (equal for round holes).
    pub size: Pt,
    /// Offset of the hole from the pad centre, in the pad's frame.
    pub offset: Pt,
}

impl Drill {
    pub fn is_round(&self) -> bool {
        (self.size[0] - self.size[1]).abs() < 1e-9
    }
}

/// A pad of a footprint, in board coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Pad {
    pub number: String,
    pub kind: PadKind,
    pub shape: PadShape,
    /// Centre on the board.
    pub at: Pt,
    /// Orientation on the board (degrees, the footprint's included).
    pub angle: f64,
    pub size: Pt,
    pub drill: Option<Drill>,
    /// Layer names as written (`*.Cu`, `F.Mask`...); see [`Board::pad_on`].
    pub layers: Vec<String>,
    pub net: NetRef,
    /// Solder mask expansion (pad or footprint value; the board's otherwise).
    pub mask_margin: Option<f64>,
    pub paste_margin: Option<f64>,
    pub paste_ratio: Option<f64>,
    /// The pad's own zone connection (`zone_connect`), overriding the zone's.
    pub zone_connect: Option<ZoneConnect>,
    /// Custom pad primitives (filled polygons and circles), in board coordinates.
    pub primitives: Vec<Shape>,
}

/// A footprint's 3D model reference.
#[derive(Debug, Clone, PartialEq)]
pub struct Model3d {
    /// As written (may hold `${KICAD...}` variables; untrusted).
    pub path: String,
    /// mm.
    pub offset: [f64; 3],
    pub scale: [f64; 3],
    /// Degrees about X, Y and Z.
    pub rotate: [f64; 3],
    pub hidden: bool,
}

/// A placed footprint.
#[derive(Debug, Clone, PartialEq)]
pub struct Footprint {
    /// `Library:Name`.
    pub lib_id: String,
    pub reference: String,
    pub value: String,
    pub at: Pt,
    pub angle: f64,
    /// On the bottom side (`B.Cu`).
    pub bottom: bool,
    /// `smd`, `through_hole`, `board_only`, `exclude_from_bom`, `dnp`...
    pub attrs: Vec<String>,
    /// Indices into [`Board::pads`].
    pub pads: Vec<usize>,
    pub models: Vec<Model3d>,
    /// Position among the file's top-level items (0 for a footprint file).
    pub node: usize,
}

/// A track segment or arc.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub start: Pt,
    pub end: Pt,
    /// The middle point of an arc track.
    pub mid: Option<Pt>,
    pub width: f64,
    pub layer: String,
    pub net: NetRef,
    /// Position among the file's top-level items.
    pub node: usize,
}

/// A via.
#[derive(Debug, Clone, PartialEq)]
pub struct Via {
    pub at: Pt,
    pub size: f64,
    pub drill: f64,
    /// The two layers it connects (through vias: `F.Cu` and `B.Cu`).
    pub layers: [String; 2],
    pub net: NetRef,
    /// Position among the file's top-level items.
    pub node: usize,
}

/// A copper zone (pour), keep-out or rule area.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    pub net: NetRef,
    /// Layer names as written (`*.Cu` possible).
    pub layers: Vec<String>,
    /// The outline drawn by the user.
    pub outline: Vec<Pt>,
    /// Filled areas as KiCad computed them: (layer, polygon).
    pub fills: Vec<(String, Vec<Pt>)>,
    /// A keep-out/rule area (no copper of its own).
    pub keepout: bool,
    /// Fill settings (copper zones).
    pub settings: ZoneSettings,
    /// Position of the zone's node in the board's tree.
    pub node: usize,
}

/// How a zone joins the pads of its own net.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoneConnect {
    /// Spokes across a gap (KiCad's default).
    #[default]
    Thermal,
    /// Copper all round.
    Solid,
    /// Thermal reliefs for through-hole pads, solid for the others.
    ThtThermal,
    /// Not joined: the pad keeps the clearance.
    None,
}

/// A zone's fill settings (mm; area in mm²).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoneSettings {
    /// Gap to copper of other nets (`connect_pads (clearance)`).
    pub clearance: f64,
    /// Narrowest copper the fill keeps (`min_thickness`).
    pub min_width: f64,
    pub connect: ZoneConnect,
    pub thermal_gap: f64,
    pub thermal_width: f64,
    /// Higher fills first; lower ones keep clear of them.
    pub priority: i64,
    /// 0: remove islands, 1: keep them, 2: remove those under `island_min_area`.
    pub island_mode: u8,
    pub island_min_area: f64,
    /// Keep-out areas: whether copper pours are kept out.
    pub no_pour: bool,
}

impl Default for ZoneSettings {
    fn default() -> Self {
        ZoneSettings {
            clearance: 0.5,
            min_width: 0.25,
            connect: ZoneConnect::Thermal,
            thermal_gap: 0.5,
            thermal_width: 0.5,
            priority: 0,
            island_mode: 0,
            island_min_area: 10.0,
            no_pour: false,
        }
    }
}

/// A board (or one footprint read as a board of its own).
#[derive(Debug, Clone)]
pub struct Board {
    /// The whole file as read; written back by [`Board::write`].
    pub tree: Node,
    /// The `(version ...)` number.
    pub version: Option<i64>,
    pub generator: Option<String>,
    /// Board thickness (mm): `(general (thickness))`, else the stackup's sum, else 1.6.
    pub thickness: f64,
    pub layers: Vec<Layer>,
    pub stackup: Vec<StackupLayer>,
    pub nets: Vec<Net>,
    pub footprints: Vec<Footprint>,
    pub pads: Vec<Pad>,
    pub graphics: Vec<Graphic>,
    pub texts: Vec<Text>,
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
    pub zones: Vec<Zone>,
    /// The board-wide solder mask expansion (`pad_to_mask_clearance`).
    pub mask_margin: f64,
    pub paste_margin: f64,
    /// Whether the file lists its nets at the top (`(net 3 "GND")`, KiCad 5 to 9); items
    /// then refer to nets by number.
    pub numbered_nets: bool,
    /// KiCad 5 net classes written in the board (newer versions keep them in the project).
    pub net_classes: Vec<crate::rules::NetClass>,
    /// `(setup (aux_axis_origin x y))`: the origin of placement files.
    pub aux_origin: Pt,
    /// Things read but not understood, for the user (at most a few dozen).
    pub notes: Vec<String>,
}

/// Default thickness when the file gives none (KiCad's default).
pub const DEFAULT_THICKNESS: f64 = 1.6;

impl Board {
    /// Reads a `.kicad_pcb` or `.kicad_mod` file.
    ///
    /// # Errors
    /// [`Error`] for malformed files or files past `limits`.
    pub fn read(src: &str, limits: &Limits) -> Result<Board, Error> {
        let tree = sexpr::parse(src, &limits.sexpr)?;
        Board::from_tree(tree, limits)
    }

    /// The board of an already-parsed tree.
    ///
    /// # Errors
    /// As [`Board::read`].
    pub fn from_tree(tree: Node, limits: &Limits) -> Result<Board, Error> {
        let head = tree.head().unwrap_or_default().to_string();
        let mut b = Board {
            tree: Node::List(Vec::new()),
            version: None,
            generator: None,
            thickness: DEFAULT_THICKNESS,
            layers: Vec::new(),
            stackup: Vec::new(),
            nets: Vec::new(),
            footprints: Vec::new(),
            pads: Vec::new(),
            graphics: Vec::new(),
            texts: Vec::new(),
            tracks: Vec::new(),
            vias: Vec::new(),
            zones: Vec::new(),
            mask_margin: 0.0,
            paste_margin: 0.0,
            numbered_nets: false,
            net_classes: Vec::new(),
            aux_origin: [0.0, 0.0],
            notes: Vec::new(),
        };
        let mut r = Reader { limits, board: &mut b };
        match head.as_str() {
            "kicad_pcb" => r.board_items(&tree)?,
            // A footprint file is one footprint at the origin.
            "footprint" | "module" => {
                r.board.version = tree.child("version").and_then(|v| v.num_arg(0)).map(|v| v as i64);
                r.footprint(&tree, 0)?;
            }
            _ => return Err(Error::NotABoard(head)),
        }
        b.tree = tree;
        if b.layers.is_empty() {
            b.layers = default_layers();
        }
        // Newer files (KiCad 10) name nets on the items only: list them from there, in
        // first-seen order, numbered from 1 (0 is "no net").
        if b.nets.is_empty() {
            let mut seen = std::collections::HashSet::new();
            let names = b
                .pads
                .iter()
                .map(|p| &p.net)
                .chain(b.tracks.iter().map(|t| &t.net))
                .chain(b.vias.iter().map(|v| &v.net))
                .chain(b.zones.iter().map(|z| &z.net))
                .filter_map(|n| n.name.clone())
                .filter(|n| !n.is_empty());
            for name in names {
                if seen.insert(name.clone()) {
                    let id = b.nets.len() as i64 + 1;
                    b.nets.push(Net { id, name });
                }
            }
        }
        Ok(b)
    }

    /// The file's text: the tree as read (with any edits made to it).
    pub fn write(&self) -> String {
        sexpr::write(&self.tree)
    }

    /// Copper layer names from front to back (`F.Cu`, `In1.Cu`..., `B.Cu`).
    pub fn copper_layers(&self) -> Vec<String> {
        let mut inner: Vec<(u32, String)> = self
            .layers
            .iter()
            .filter_map(|l| {
                let n = l.name.strip_prefix("In")?.strip_suffix(".Cu")?.parse::<u32>().ok()?;
                Some((n, l.name.clone()))
            })
            .collect();
        inner.sort();
        let has = |n: &str| self.layers.iter().any(|l| l.name == n);
        let mut out = Vec::new();
        if has("F.Cu") {
            out.push("F.Cu".to_string());
        }
        out.extend(inner.into_iter().map(|x| x.1));
        if has("B.Cu") {
            out.push("B.Cu".to_string());
        }
        out
    }

    /// Whether a pad (or anything with this layer list) is on `layer`, expanding `*.Cu`,
    /// `*.Mask`, `F&B.Cu` and the like.
    pub fn on_layer(&self, layers: &[String], layer: &str) -> bool {
        layers.iter().any(|l| layer_matches(l, layer))
    }

    /// The copper layers between two via layers (inclusive), front to back.
    pub fn span(&self, a: &str, b: &str) -> Vec<String> {
        let cu = self.copper_layers();
        let i = cu.iter().position(|l| l == a);
        let j = cu.iter().position(|l| l == b);
        match (i, j) {
            (Some(i), Some(j)) => cu.get(i.min(j)..=i.max(j)).map(<[String]>::to_vec).unwrap_or_default(),
            _ => cu,
        }
    }

    /// The net's name for a reference (by number when it has one).
    pub fn net_name(&self, r: &NetRef) -> Option<String> {
        if let Some(n) = &r.name
            && !n.is_empty()
        {
            return Some(n.clone());
        }
        let id = r.id?;
        self.nets.iter().find(|n| n.id == id).map(|n| n.name.clone()).filter(|n| !n.is_empty())
    }

    /// Outline drawings (`Edge.Cuts`), board and footprint ones.
    pub fn outline(&self) -> impl Iterator<Item = &Graphic> {
        self.graphics.iter().filter(|g| g.layer == "Edge.Cuts")
    }

    /// Moves a footprint to `at` (board mm) and `angle`, in the tree; the typed view is
    /// re-read so pads and graphics follow.
    ///
    /// # Errors
    /// If the footprint does not exist or the re-read fails.
    pub fn move_footprint(&mut self, index: usize, at: Pt, angle: f64, limits: &Limits) -> Result<(), Error> {
        let heads = ["footprint", "module"];
        let mut seen = 0;
        let mut done = false;
        if let Some(items) = self.tree.items_mut() {
            for n in items.iter_mut().skip(1) {
                if !n.head().is_some_and(|h| heads.contains(&h)) {
                    continue;
                }
                if seen == index {
                    let mut at_node = vec![Node::sym("at"), Node::num(at[0]), Node::num(at[1])];
                    if angle.abs() > 1e-12 {
                        at_node.push(Node::num(angle));
                    }
                    if let Some(old) = n.child_mut("at") {
                        *old = Node::List(at_node);
                    } else if let Some(v) = n.items_mut() {
                        v.push(Node::List(at_node));
                    }
                    // Pad angles are absolute in the file: turn them with the footprint.
                    let old_angle = self.footprints.get(index).map_or(0.0, |f| f.angle);
                    rotate_pad_angles(n, angle - old_angle);
                    done = true;
                    break;
                }
                seen += 1;
            }
        }
        if !done {
            return Err(Error::TooMany("footprints (no such footprint)"));
        }
        let tree = std::mem::replace(&mut self.tree, Node::List(Vec::new()));
        *self = Board::from_tree(tree, limits)?;
        Ok(())
    }
}

fn rotate_pad_angles(fp: &mut Node, delta: f64) {
    if delta.abs() < 1e-12 {
        return;
    }
    let Some(items) = fp.items_mut() else { return };
    for pad in items.iter_mut().filter(|n| n.head() == Some("pad")) {
        let Some(at) = pad.child_mut("at") else { continue };
        let Some(v) = at.items_mut() else { continue };
        let old = v.get(3).and_then(Node::atom).and_then(sexpr::parse_num).unwrap_or(0.0);
        let mut a = (old + delta) % 360.0;
        if a < 0.0 {
            a += 360.0;
        }
        v.truncate(3);
        if a.abs() > 1e-9 {
            v.push(Node::num(a));
        }
    }
}

/// Whether a layer pattern as written (`*.Cu`, `F&B.Cu`, `F.Cu`) covers `layer`.
pub fn layer_matches(pattern: &str, layer: &str) -> bool {
    if pattern == layer {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return layer.ends_with(&format!(".{suffix}"));
    }
    if let Some(suffix) = pattern.strip_prefix("F&B.") {
        return layer == format!("F.{suffix}") || layer == format!("B.{suffix}");
    }
    false
}

fn default_layers() -> Vec<Layer> {
    let l = |o: i64, n: &str, k: LayerKind| Layer { ordinal: o, name: n.into(), kind: k, user_name: None };
    vec![
        l(0, "F.Cu", LayerKind::Signal),
        l(31, "B.Cu", LayerKind::Signal),
        l(36, "B.SilkS", LayerKind::User),
        l(37, "F.SilkS", LayerKind::User),
        l(38, "B.Mask", LayerKind::User),
        l(39, "F.Mask", LayerKind::User),
        l(34, "B.Paste", LayerKind::User),
        l(35, "F.Paste", LayerKind::User),
        l(44, "Edge.Cuts", LayerKind::User),
    ]
}

/// A footprint's placement: local (footprint) coordinates to board coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Place {
    pub at: Pt,
    /// Degrees, counter-clockwise on screen (Y down).
    pub angle: f64,
}

impl Place {
    pub const ORIGIN: Place = Place { at: [0.0, 0.0], angle: 0.0 };

    /// Board coordinates of a local point. With Y down, turning counter-clockwise on screen
    /// by `a` maps (x, y) to (x cos a + y sin a, -x sin a + y cos a).
    pub fn apply(&self, p: Pt) -> Pt {
        let (s, c) = (libm::sin(self.angle.to_radians()), libm::cos(self.angle.to_radians()));
        [self.at[0] + p[0] * c + p[1] * s, self.at[1] - p[0] * s + p[1] * c]
    }
}

struct Reader<'a> {
    limits: &'a Limits,
    board: &'a mut Board,
}

fn pt(n: Option<&Node>) -> Option<Pt> {
    let n = n?;
    let x = n.num_arg(0)?;
    let y = n.num_arg(1)?;
    (x.abs() <= MAX_COORD && y.abs() <= MAX_COORD).then_some([x, y])
}

fn num(n: &Node, head: &str) -> Option<f64> {
    n.child(head).and_then(|c| c.num_arg(0))
}

fn text_arg(n: &Node, i: usize) -> String {
    n.arg(i).unwrap_or_default().to_string()
}

/// `(net 3 "GND")`, `(net 3)` or `(net "GND")`.
fn net_ref(n: &Node) -> NetRef {
    let Some(net) = n.child("net") else { return NetRef::default() };
    let first = net.arg(0);
    match first.and_then(|t| t.parse::<i64>().ok()) {
        Some(id) if !matches!(net.args().first(), Some(Node::Atom(a)) if a.quoted) => {
            NetRef { id: Some(id), name: net.arg(1).map(str::to_string) }
        }
        _ => NetRef { id: None, name: first.map(str::to_string) },
    }
}

/// `(zone_connect 0..3)` of a pad or footprint: none, thermal, solid, thermal for
/// through-hole pads only.
fn zone_connect(n: &Node) -> Option<ZoneConnect> {
    match num(n, "zone_connect").map(|v| v as i64) {
        Some(0) => Some(ZoneConnect::None),
        Some(1) => Some(ZoneConnect::Thermal),
        Some(2) => Some(ZoneConnect::Solid),
        Some(3) => Some(ZoneConnect::ThtThermal),
        _ => None,
    }
}

/// A zone's fill settings, KiCad's defaults where the file says nothing.
fn zone_settings(n: &Node) -> ZoneSettings {
    let d = ZoneSettings::default();
    let len = |v: Option<f64>, def: f64| v.filter(|v| v.is_finite() && (0.0..=100.0).contains(v)).unwrap_or(def);
    let cp = n.child("connect_pads");
    let connect = match cp.and_then(|c| c.args().first()).and_then(Node::atom) {
        Some("yes") => ZoneConnect::Solid,
        Some("no") => ZoneConnect::None,
        Some("thru_hole_only") => ZoneConnect::ThtThermal,
        _ => ZoneConnect::Thermal,
    };
    let fill = n.child("fill");
    let keep = n.child("keepout");
    ZoneSettings {
        clearance: len(cp.and_then(|c| num(c, "clearance")), d.clearance),
        min_width: len(num(n, "min_thickness"), d.min_width),
        connect,
        thermal_gap: len(fill.and_then(|f| num(f, "thermal_gap")), d.thermal_gap),
        thermal_width: len(fill.and_then(|f| num(f, "thermal_bridge_width")), d.thermal_width),
        priority: num(n, "priority").filter(|v| v.is_finite()).map_or(0, |v| v.clamp(-1e9, 1e9) as i64),
        island_mode: fill
            .and_then(|f| num(f, "island_removal_mode"))
            .filter(|v| (0.0..=2.0).contains(v))
            .map_or(0, |v| v as u8),
        island_min_area: fill
            .and_then(|f| num(f, "island_area_min"))
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(d.island_min_area),
        no_pour: keep.and_then(|k| k.child("copperpour")).and_then(|c| c.arg(0)) == Some("not_allowed"),
    }
}

/// A stroke width: `(stroke (width w))` (KiCad 7+) or `(width w)`.
fn stroke_width(n: &Node) -> f64 {
    n.child("stroke").and_then(|s| num(s, "width")).or_else(|| num(n, "width")).unwrap_or(0.0).max(0.0)
}

/// `(fill solid|yes|none|no)` or `(fill (type ...))` in older files.
fn filled(n: &Node) -> bool {
    let Some(f) = n.child("fill") else { return false };
    let t = f.arg(0).or_else(|| f.child("type").and_then(|t| t.arg(0))).unwrap_or("none");
    matches!(t, "solid" | "yes" | "true")
}

/// A point list `(pts (xy x y) ... (arc (start) (mid) (end)))`, arcs as short chords.
fn pts(n: Option<&Node>, max: usize) -> Option<Vec<Pt>> {
    let n = n?;
    let mut out = Vec::new();
    for p in n.args() {
        match p.head() {
            Some("xy") => out.push(pt(Some(p))?),
            Some("arc") => {
                let (s, m, e) = (pt(p.child("start"))?, pt(p.child("mid"))?, pt(p.child("end"))?);
                out.extend(crate::geom::arc_points(s, m, e, 0.05));
            }
            _ => {}
        }
        if out.len() > max {
            return None;
        }
    }
    Some(out)
}

impl Reader<'_> {
    fn note(&mut self, t: String) {
        if self.board.notes.len() < 40 {
            self.board.notes.push(t);
        }
    }

    fn check(&self, n: usize, what: &'static str) -> Result<(), Error> {
        if n >= self.limits.max_items { Err(Error::TooMany(what)) } else { Ok(()) }
    }

    fn board_items(&mut self, tree: &Node) -> Result<(), Error> {
        let b = &mut *self.board;
        b.version = tree.child("version").and_then(|v| v.num_arg(0)).map(|v| v as i64);
        b.generator = tree.child("generator").and_then(|g| g.arg(0)).map(str::to_string);
        let general = tree.child("general").and_then(|g| num(g, "thickness"));
        if let Some(layers) = tree.child("layers") {
            for l in layers.args() {
                let (Some(o), Some(name)) = (l.items().first().and_then(Node::atom), l.arg(0)) else { continue };
                let Ok(ordinal) = o.parse::<i64>() else { continue };
                let kind = match l.arg(1).unwrap_or("user") {
                    "signal" => LayerKind::Signal,
                    "power" => LayerKind::Power,
                    "mixed" => LayerKind::Mixed,
                    "jumper" => LayerKind::Jumper,
                    _ => LayerKind::User,
                };
                b.layers.push(Layer { ordinal, name: name.to_string(), kind, user_name: l.arg(2).map(str::to_string) });
                if b.layers.len() > 1000 {
                    return Err(Error::TooMany("layers"));
                }
            }
        }
        if let Some(setup) = tree.child("setup") {
            b.mask_margin = num(setup, "pad_to_mask_clearance").unwrap_or(0.0);
            b.paste_margin = num(setup, "pad_to_paste_clearance").unwrap_or(0.0);
            b.aux_origin = pt(setup.child("aux_axis_origin")).unwrap_or([0.0, 0.0]);
            if let Some(st) = setup.child("stackup") {
                for l in st.children("layer").take(1000) {
                    b.stackup.push(StackupLayer {
                        name: text_arg(l, 0),
                        kind: l.child("type").map(|t| text_arg(t, 0)).unwrap_or_default(),
                        thickness: num(l, "thickness").unwrap_or(0.0).clamp(0.0, 1000.0),
                        material: l.child("material").map(|t| text_arg(t, 0)),
                        epsilon_r: num(l, "epsilon_r"),
                    });
                }
            }
        }
        let stack_sum: f64 = b.stackup.iter().map(|l| l.thickness).sum();
        b.thickness = match general {
            Some(t) if t > 0.0 && t < 100.0 => t,
            _ if stack_sum > 0.0 && stack_sum < 100.0 => stack_sum,
            _ => DEFAULT_THICKNESS,
        };
        for (i, n) in tree.args().iter().enumerate() {
            let node = i + 1;
            match n.head() {
                Some("net_class") => {
                    if self.board.net_classes.len() < 1000 {
                        self.board.net_classes.push(crate::rules::NetClass::from_kicad5(n));
                    }
                }
                Some("net") => {
                    self.check(self.board.nets.len(), "nets")?;
                    if let Some(id) = n.arg(0).and_then(|t| t.parse::<i64>().ok()) {
                        self.board.numbered_nets = true;
                        self.board.nets.push(Net { id, name: text_arg(n, 1) });
                    } else if let Some(name) = n.arg(0) {
                        let id = self.board.nets.len() as i64;
                        self.board.nets.push(Net { id, name: name.to_string() });
                    }
                }
                Some("footprint" | "module") => self.footprint(n, node)?,
                Some("segment") => self.track(n, false, node)?,
                Some("arc") => self.track(n, true, node)?,
                Some("via") => self.via(n, node)?,
                Some("zone") => self.zone(n, node)?,
                Some(h) if h.starts_with("gr_") => self.graphic(n, None, &Place::ORIGIN)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn track(&mut self, n: &Node, arc: bool, node: usize) -> Result<(), Error> {
        self.check(self.board.tracks.len(), "tracks")?;
        let (Some(start), Some(end)) = (pt(n.child("start")), pt(n.child("end"))) else {
            self.note("a track without start or end was skipped".into());
            return Ok(());
        };
        let mid = if arc { pt(n.child("mid")) } else { None };
        self.board.tracks.push(Track {
            start,
            end,
            mid,
            width: num(n, "width").unwrap_or(0.0).max(0.0),
            layer: n.child("layer").map(|l| text_arg(l, 0)).unwrap_or_default(),
            net: net_ref(n),
            node,
        });
        Ok(())
    }

    fn via(&mut self, n: &Node, node: usize) -> Result<(), Error> {
        self.check(self.board.vias.len(), "vias")?;
        let Some(at) = pt(n.child("at")) else { return Ok(()) };
        let layers = n.child("layers");
        let la = layers.and_then(|l| l.arg(0)).unwrap_or("F.Cu").to_string();
        let lb = layers.and_then(|l| l.arg(1)).unwrap_or("B.Cu").to_string();
        self.board.vias.push(Via {
            at,
            size: num(n, "size").unwrap_or(0.0).max(0.0),
            drill: num(n, "drill").unwrap_or(0.0).max(0.0),
            layers: [la, lb],
            net: net_ref(n),
            node,
        });
        Ok(())
    }

    fn zone(&mut self, n: &Node, node: usize) -> Result<(), Error> {
        self.check(self.board.zones.len(), "zones")?;
        let max = self.limits.max_items;
        let mut layers: Vec<String> = Vec::new();
        if let Some(l) = n.child("layer") {
            layers.push(text_arg(l, 0));
        }
        if let Some(l) = n.child("layers") {
            layers.extend(l.args().iter().filter_map(Node::atom).map(str::to_string));
        }
        let outline = n.child("polygon").and_then(|p| pts(p.child("pts"), max)).unwrap_or_default();
        let mut fills = Vec::new();
        for f in n.children("filled_polygon") {
            let layer =
                f.child("layer").map(|l| text_arg(l, 0)).or_else(|| layers.first().cloned()).unwrap_or_default();
            if let Some(p) = pts(f.child("pts"), max) {
                fills.push((layer, p));
            }
        }
        let keepout = n.child("keepout").is_some() || n.child("rule_area").is_some();
        let settings = zone_settings(n);
        self.board.zones.push(Zone { net: net_ref(n), layers, outline, fills, keepout, settings, node });
        Ok(())
    }

    fn graphic(&mut self, n: &Node, fp: Option<usize>, place: &Place) -> Result<(), Error> {
        self.check(self.board.graphics.len() + self.board.texts.len(), "drawings")?;
        let head = n.head().unwrap_or_default();
        let kind = head.split_once('_').map_or(head, |x| x.1);
        let layer = n.child("layer").map(|l| text_arg(l, 0)).unwrap_or_default();
        let width = stroke_width(n);
        let p = |x: Pt| place.apply(x);
        let shape = match kind {
            "line" => match (pt(n.child("start")), pt(n.child("end"))) {
                (Some(a), Some(b)) => Shape::Line { a: p(a), b: p(b) },
                _ => return Ok(()),
            },
            "arc" => {
                let (Some(s), Some(e)) = (pt(n.child("start")), pt(n.child("end"))) else { return Ok(()) };
                if let Some(m) = pt(n.child("mid")) {
                    Shape::Arc { start: p(s), mid: p(m), end: p(e) }
                } else if let Some(angle) = num(n, "angle") {
                    // KiCad 5: `start` is the centre, `end` the arc's start, `angle` the
                    // sweep (clockwise on screen for positive angles).
                    let Some((a, m, b)) = crate::geom::arc_from_center(s, e, angle) else { return Ok(()) };
                    Shape::Arc { start: p(a), mid: p(m), end: p(b) }
                } else {
                    return Ok(());
                }
            }
            "circle" => {
                let Some(c) = pt(n.child("center")) else { return Ok(()) };
                let Some(e) = pt(n.child("end")) else { return Ok(()) };
                let radius = libm::hypot(e[0] - c[0], e[1] - c[1]);
                Shape::Circle { center: p(c), radius }
            }
            "rect" => {
                let (Some(a), Some(b)) = (pt(n.child("start")), pt(n.child("end"))) else { return Ok(()) };
                Shape::Poly { pts: vec![p(a), p([b[0], a[1]]), p(b), p([a[0], b[1]])] }
            }
            "poly" => {
                let Some(v) = pts(n.child("pts"), self.limits.max_items) else { return Ok(()) };
                if v.len() < 2 {
                    return Ok(());
                }
                Shape::Poly { pts: v.into_iter().map(p).collect() }
            }
            "text" | "text_box" => {
                // `(gr_text "text" (at x y))`, `(fp_text reference "R1" (at x y))`.
                let t = if head.starts_with("fp_") { n.arg(1) } else { n.arg(0) };
                let at = pt(n.child("at")).map(p).unwrap_or([0.0, 0.0]);
                self.board.texts.push(Text { text: t.unwrap_or_default().to_string(), at, layer, footprint: fp });
                return Ok(());
            }
            "curve" | "bbox" | "vector" => return Ok(()),
            other => {
                let other: String = other.chars().filter(char::is_ascii_alphanumeric).take(20).collect();
                self.note(format!("drawing kind '{other}' is not read"));
                return Ok(());
            }
        };
        let filled = filled(n) || (kind == "poly" && head == "fp_poly" && n.child("fill").is_none() && width == 0.0);
        self.board.graphics.push(Graphic { shape, layer, width, filled, footprint: fp });
        Ok(())
    }

    fn footprint(&mut self, n: &Node, node: usize) -> Result<(), Error> {
        self.check(self.board.footprints.len(), "footprints")?;
        let index = self.board.footprints.len();
        let at_node = n.child("at");
        let at = pt(at_node).unwrap_or([0.0, 0.0]);
        let angle = at_node.and_then(|a| a.num_arg(2)).unwrap_or(0.0);
        let place = Place { at, angle };
        let layer = n.child("layer").map(|l| text_arg(l, 0)).unwrap_or_else(|| "F.Cu".into());
        let mut fp = Footprint {
            lib_id: text_arg(n, 0),
            reference: String::new(),
            value: String::new(),
            at,
            angle,
            bottom: layer == "B.Cu",
            attrs: n
                .child("attr")
                .map(|a| a.args().iter().filter_map(Node::atom).map(str::to_string).collect())
                .unwrap_or_default(),
            pads: Vec::new(),
            models: Vec::new(),
            node,
        };
        let fp_mask = num(n, "solder_mask_margin");
        let fp_paste = num(n, "solder_paste_margin");
        let fp_ratio = num(n, "solder_paste_ratio").or_else(|| num(n, "solder_paste_margin_ratio"));
        let fp_zone = zone_connect(n);
        for c in n.args() {
            match c.head() {
                Some("property") => match c.arg(0) {
                    Some("Reference") => fp.reference = text_arg(c, 1),
                    Some("Value") => fp.value = text_arg(c, 1),
                    _ => {}
                },
                Some("fp_text") => {
                    match c.arg(0) {
                        Some("reference") => fp.reference = text_arg(c, 1),
                        Some("value") => fp.value = text_arg(c, 1),
                        _ => {}
                    }
                    self.graphic(c, Some(index), &place)?;
                }
                Some("pad") => {
                    self.check(self.board.pads.len(), "pads")?;
                    if let Some(mut pad) = self.pad(c, &place) {
                        pad.mask_margin = pad.mask_margin.or(fp_mask);
                        pad.paste_margin = pad.paste_margin.or(fp_paste);
                        pad.paste_ratio = pad.paste_ratio.or(fp_ratio);
                        pad.zone_connect = pad.zone_connect.or(fp_zone);
                        fp.pads.push(self.board.pads.len());
                        self.board.pads.push(pad);
                    }
                }
                Some("model") => {
                    if fp.models.len() < 16
                        && let Some(m) = model(c)
                    {
                        fp.models.push(m);
                    }
                }
                Some(h) if h.starts_with("fp_") => self.graphic(c, Some(index), &place)?,
                _ => {}
            }
        }
        // KiCad 5 had no `attr` for SMD parts with pads only on one side.
        if fp.attrs.is_empty() {
            let th = fp.pads.iter().filter_map(|&i| self.board.pads.get(i)).any(|p| p.kind == PadKind::Tht);
            fp.attrs.push(if th { "through_hole" } else { "smd" }.into());
        }
        self.board.footprints.push(fp);
        Ok(())
    }

    fn pad(&mut self, n: &Node, place: &Place) -> Option<Pad> {
        let kind = match n.arg(1)? {
            "smd" => PadKind::Smd,
            "thru_hole" => PadKind::Tht,
            "np_thru_hole" => PadKind::Npth,
            "connect" => PadKind::Connect,
            _ => return None,
        };
        let shape = match n.arg(2)? {
            "circle" => PadShape::Circle,
            "rect" => PadShape::Rect,
            "oval" => PadShape::Oval,
            "roundrect" => PadShape::RoundRect { ratio: num(n, "roundrect_rratio").unwrap_or(0.25).clamp(0.0, 0.5) },
            "trapezoid" => PadShape::Trapezoid { delta: pt(n.child("rect_delta")).unwrap_or([0.0, 0.0]) },
            "custom" => {
                let anchor = n.child("options").and_then(|o| o.child("anchor")).and_then(|a| a.arg(0));
                PadShape::Custom { anchor_circle: anchor == Some("circle") }
            }
            _ => return None,
        };
        let at_node = n.child("at");
        let local = pt(at_node).unwrap_or([0.0, 0.0]);
        let angle = at_node.and_then(|a| a.num_arg(2)).unwrap_or(0.0);
        let size = pt(n.child("size")).unwrap_or([0.0, 0.0]);
        let size = [size[0].max(0.0), size[1].max(0.0)];
        let drill = n.child("drill").and_then(|d| {
            let offset = pt(d.child("offset")).unwrap_or([0.0, 0.0]);
            let oval = d.has_flag("oval");
            let nums: Vec<f64> = d.args().iter().filter_map(Node::atom).filter_map(sexpr::parse_num).collect();
            let a = nums.first().copied().unwrap_or(0.0).max(0.0);
            let b = if oval { nums.get(1).copied().unwrap_or(a) } else { a }.max(0.0);
            (a > 0.0).then_some(Drill { size: [a, b], offset })
        });
        let layers = n
            .child("layers")
            .map(|l| l.args().iter().filter_map(Node::atom).map(str::to_string).collect())
            .unwrap_or_default();
        let at = place.apply(local);
        // Custom pad primitives are in the pad's frame, turned by the pad's angle.
        let mut primitives = Vec::new();
        if let Some(prims) = n.child("primitives") {
            let pp = Place { at, angle };
            for p in prims.args().iter().take(1000) {
                match p.head() {
                    Some("gr_poly") => {
                        if let Some(v) = pts(p.child("pts"), 10_000) {
                            primitives.push(Shape::Poly { pts: v.into_iter().map(|x| pp.apply(x)).collect() });
                        }
                    }
                    Some("gr_circle") => {
                        if let (Some(c), Some(e)) = (pt(p.child("center")), pt(p.child("end"))) {
                            let r = libm::hypot(e[0] - c[0], e[1] - c[1]) + stroke_width(p) / 2.0;
                            primitives.push(Shape::Circle { center: pp.apply(c), radius: r });
                        }
                    }
                    Some("gr_rect") => {
                        if let (Some(a), Some(b)) = (pt(p.child("start")), pt(p.child("end"))) {
                            let v = [a, [b[0], a[1]], b, [a[0], b[1]]];
                            primitives.push(Shape::Poly { pts: v.into_iter().map(|x| pp.apply(x)).collect() });
                        }
                    }
                    _ => {}
                }
            }
        }
        Some(Pad {
            number: text_arg(n, 0),
            kind,
            shape,
            at,
            angle,
            size,
            drill,
            layers,
            net: net_ref(n),
            mask_margin: num(n, "solder_mask_margin"),
            paste_margin: num(n, "solder_paste_margin"),
            paste_ratio: num(n, "solder_paste_margin_ratio"),
            zone_connect: zone_connect(n),
            primitives,
        })
    }
}

fn model(n: &Node) -> Option<Model3d> {
    let xyz = |head: &str, default: [f64; 3]| -> [f64; 3] {
        let Some(x) = n.child(head).and_then(|c| c.child("xyz")) else { return default };
        let v = |i| x.num_arg(i).filter(|v: &f64| v.abs() < 1e5);
        [v(0).unwrap_or(default[0]), v(1).unwrap_or(default[1]), v(2).unwrap_or(default[2])]
    };
    // KiCad 5 wrote `(at (xyz ...))` in inches; KiCad 6+ `(offset (xyz ...))` in mm.
    let offset = if n.child("offset").is_some() {
        xyz("offset", [0.0; 3])
    } else {
        let a = xyz("at", [0.0; 3]);
        [a[0] * 25.4, a[1] * 25.4, a[2] * 25.4]
    };
    Some(Model3d {
        path: n.arg(0)?.chars().take(1024).collect(),
        offset,
        scale: xyz("scale", [1.0; 3]),
        rotate: xyz("rotate", [0.0; 3]),
        hidden: n.has_flag("hide") || n.child("hide").is_some_and(|h| h.arg(0) != Some("no")),
    })
}
