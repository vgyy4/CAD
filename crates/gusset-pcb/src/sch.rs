//! KiCad schematics (`.kicad_sch`) and symbol libraries (`.kicad_sym`), ADR 0047 phase 2a.
//!
//! As for boards, the S-expression tree stays the source of truth: [`Schematic`] and
//! [`SymbolLib`] keep it, the typed view is derived from it, and `write` writes the tree
//! back, so everything not modelled (fonts, colours, images, buses, simulation fields...)
//! survives a round trip. Written from KiCad's published format description and sample
//! files (clean room).
//!
//! Coordinates are the sheet's: millimetres, X right, **Y down**. Library symbol graphics
//! and pins are in the library's coordinates, **Y up**; [`Symbol::place`] maps them onto
//! the sheet (the symbol's angle, counter-clockwise on screen, then its mirror).

use crate::board::{Limits, Pt};
use crate::sexpr::{self, Node};
use std::fmt;

/// Largest coordinate accepted (mm).
const MAX_COORD: f64 = 10_000.0;
/// Longest text kept from a field (characters).
const MAX_TEXT: usize = 2000;

/// Why a schematic or library could not be read or edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Sexpr(sexpr::Error),
    /// Not a schematic or symbol library (the top-level list's head).
    WrongKind(String),
    /// Past a count limit.
    TooMany(&'static str),
    /// An edit that cannot be made (bad argument, missing item).
    Edit(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sexpr(e) => write!(f, "{e}"),
            Error::WrongKind(h) => {
                let h: String = h.chars().filter(|c| c.is_ascii_graphic()).take(40).collect();
                write!(f, "not a KiCad schematic or symbol library (the file starts with '{h}')")
            }
            Error::TooMany(what) => write!(f, "the file has more {what} than the reader's limit"),
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

/// An electrical pin type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PinType {
    Input,
    Output,
    Bidirectional,
    TriState,
    Passive,
    Free,
    Unspecified,
    PowerIn,
    PowerOut,
    OpenCollector,
    OpenEmitter,
    NoConnect,
}

impl PinType {
    /// From the file's keyword; unknown keywords read as unspecified.
    pub fn parse(s: &str) -> PinType {
        match s {
            "input" => PinType::Input,
            "output" => PinType::Output,
            "bidirectional" => PinType::Bidirectional,
            "tri_state" => PinType::TriState,
            "passive" => PinType::Passive,
            "free" => PinType::Free,
            "power_in" => PinType::PowerIn,
            "power_out" => PinType::PowerOut,
            "open_collector" => PinType::OpenCollector,
            "open_emitter" => PinType::OpenEmitter,
            "no_connect" => PinType::NoConnect,
            _ => PinType::Unspecified,
        }
    }

    /// The file's keyword.
    pub fn name(self) -> &'static str {
        match self {
            PinType::Input => "input",
            PinType::Output => "output",
            PinType::Bidirectional => "bidirectional",
            PinType::TriState => "tri_state",
            PinType::Passive => "passive",
            PinType::Free => "free",
            PinType::Unspecified => "unspecified",
            PinType::PowerIn => "power_in",
            PinType::PowerOut => "power_out",
            PinType::OpenCollector => "open_collector",
            PinType::OpenEmitter => "open_emitter",
            PinType::NoConnect => "no_connect",
        }
    }
}

/// How a library shape is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fill {
    #[default]
    None,
    /// In the outline's colour.
    Outline,
    /// In the body background colour.
    Background,
}

/// A library symbol's drawing (library coordinates, Y up).
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Rect { a: Pt, b: Pt },
    Poly { pts: Vec<Pt> },
    Circle { center: Pt, radius: f64 },
    Arc { start: Pt, mid: Pt, end: Pt },
    Text { text: String, at: Pt, size: f64 },
}

/// A drawing of a library symbol with its stroke and fill.
#[derive(Debug, Clone, PartialEq)]
pub struct LibGraphic {
    pub shape: Shape,
    pub width: f64,
    pub fill: Fill,
}

/// A pin of a library symbol (library coordinates, Y up).
#[derive(Debug, Clone, PartialEq)]
pub struct LibPin {
    pub kind: PinType,
    /// Graphic style as written (`line`, `inverted`, `clock`...).
    pub style: String,
    /// The connection point (the pin's outer end).
    pub at: Pt,
    /// Direction from the connection point towards the body (degrees, counter-clockwise).
    pub angle: f64,
    pub length: f64,
    pub name: String,
    pub number: String,
    pub hidden: bool,
}

/// The drawings and pins of one unit and body style (`R_1_1`); unit or style 0 is
/// common to all.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LibUnit {
    pub unit: u32,
    pub style: u32,
    pub graphics: Vec<LibGraphic>,
    pub pins: Vec<LibPin>,
}

/// A symbol of a library, or one embedded in a schematic's `lib_symbols`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LibSymbol {
    /// `R` in a library, `Device:R` in a schematic.
    pub name: String,
    /// The symbol it derives from (libraries only; graphics and pins are the parent's).
    pub extends: Option<String>,
    /// A power symbol (its value names a global net).
    pub power: bool,
    /// (key, value, position in library coordinates, angle, hidden).
    pub properties: Vec<LibProperty>,
    pub units: Vec<LibUnit>,
    pub hide_pin_names: bool,
    pub hide_pin_numbers: bool,
    /// How far pin names sit inside the body (0: outside, beside the pin).
    pub pin_name_offset: f64,
}

/// A property of a library symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct LibProperty {
    pub key: String,
    pub value: String,
    pub at: Pt,
    pub angle: f64,
    pub hidden: bool,
}

impl LibSymbol {
    /// A property's value.
    pub fn property(&self, key: &str) -> Option<&str> {
        self.properties.iter().find(|p| p.key == key).map(|p| p.value.as_str())
    }

    /// The number of units (at least 1).
    pub fn unit_count(&self) -> u32 {
        self.units.iter().map(|u| u.unit).max().unwrap_or(1).max(1)
    }

    /// The units drawn for `unit` and body style `style` (common ones included).
    pub fn units_of(&self, unit: u32, style: u32) -> impl Iterator<Item = &LibUnit> {
        self.units.iter().filter(move |u| (u.unit == 0 || u.unit == unit) && (u.style == 0 || u.style == style.max(1)))
    }

    /// The pins of a unit and body style.
    pub fn pins_of(&self, unit: u32, style: u32) -> impl Iterator<Item = &LibPin> {
        self.units_of(unit, style).flat_map(|u| u.pins.iter())
    }

    /// The reference prefix (`R` for `R?`).
    pub fn prefix(&self) -> String {
        let r = self.property("Reference").unwrap_or("U");
        let p: String = r.chars().take_while(|c| !c.is_ascii_digit() && *c != '?').collect();
        if p.is_empty() { "U".into() } else { p }
    }
}

/// A placed symbol's property (sheet coordinates).
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub key: String,
    pub value: String,
    pub at: Pt,
    pub angle: f64,
    pub hidden: bool,
}

/// How a symbol is mirrored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mirror {
    #[default]
    None,
    /// About the X axis (top and bottom swap).
    X,
    /// About the Y axis (left and right swap).
    Y,
}

/// A symbol's reference in one use of its sheet (`(instances (project (path ...)))`).
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// Sheet uuids from the root, `/root-uuid/sheet-uuid`.
    pub path: String,
    pub reference: String,
    pub unit: Option<u32>,
}

/// A symbol placed on a sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    /// `Library:Name`.
    pub lib_id: String,
    /// The `lib_symbols` entry when it differs from `lib_id`.
    pub lib_name: Option<String>,
    pub at: Pt,
    /// Degrees, counter-clockwise on screen (0, 90, 180 or 270 in KiCad).
    pub angle: f64,
    pub mirror: Mirror,
    pub unit: u32,
    /// Body style (De Morgan alternate: 2).
    pub style: u32,
    pub uuid: Option<String>,
    pub properties: Vec<Property>,
    pub instances: Vec<Instance>,
    pub in_bom: bool,
    pub on_board: bool,
    pub dnp: bool,
    /// Position among the file's top-level items.
    pub node: usize,
}

impl Symbol {
    pub fn property(&self, key: &str) -> Option<&str> {
        self.properties.iter().find(|p| p.key == key).map(|p| p.value.as_str())
    }
    /// The `Reference` property (`R1`).
    pub fn reference(&self) -> &str {
        self.property("Reference").unwrap_or("")
    }
    pub fn value(&self) -> &str {
        self.property("Value").unwrap_or("")
    }
    /// The footprint (`Lib:Name`), empty when none.
    pub fn footprint(&self) -> &str {
        self.property("Footprint").unwrap_or("")
    }

    /// The reference in a use of the sheet (`path`), else the `Reference` property.
    pub fn reference_in(&self, path: &str) -> &str {
        self.instances.iter().find(|i| i.path == path).map_or_else(|| self.reference(), |i| i.reference.as_str())
    }

    /// The `lib_symbols` name this symbol draws from.
    pub fn lib_key(&self) -> &str {
        self.lib_name.as_deref().unwrap_or(&self.lib_id)
    }

    /// A library point (Y up) on the sheet: Y flipped, turned by the angle, mirrored,
    /// moved to the symbol's place.
    pub fn place(&self, p: Pt) -> Pt {
        let o = self.orient([p[0], -p[1]]);
        [self.at[0] + o[0], self.at[1] + o[1]]
    }

    /// A sheet-oriented vector (Y down) turned and mirrored like the symbol.
    pub fn orient(&self, v: Pt) -> Pt {
        let (s, c) = sin_cos(self.angle);
        let v = [v[0] * c + v[1] * s, -v[0] * s + v[1] * c];
        match self.mirror {
            Mirror::None => v,
            Mirror::X => [v[0], -v[1]],
            Mirror::Y => [-v[0], v[1]],
        }
    }
}

/// Sine and cosine of an angle in degrees, exact at multiples of 90°.
pub fn sin_cos(deg: f64) -> (f64, f64) {
    let d = deg.rem_euclid(360.0);
    for (a, s, c) in [(0.0, 0.0, 1.0), (90.0, 1.0, 0.0), (180.0, 0.0, -1.0), (270.0, -1.0, 0.0), (360.0, 0.0, 1.0)] {
        if (d - a).abs() < 1e-9 {
            return (s, c);
        }
    }
    let r = d.to_radians();
    (libm::sin(r), libm::cos(r))
}

/// A wire or bus segment.
#[derive(Debug, Clone, PartialEq)]
pub struct Wire {
    pub a: Pt,
    pub b: Pt,
    /// Position among the file's top-level items.
    pub node: usize,
}

/// What a label connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LabelKind {
    /// Same name on the same sheet.
    Local,
    /// Same name anywhere in the design.
    Global,
    /// To the sheet pin of the same name on the parent sheet.
    Hierarchical,
}

impl LabelKind {
    /// The file's keyword.
    pub fn head(self) -> &'static str {
        match self {
            LabelKind::Local => "label",
            LabelKind::Global => "global_label",
            LabelKind::Hierarchical => "hierarchical_label",
        }
    }
}

/// A net label.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub kind: LabelKind,
    pub text: String,
    pub at: Pt,
    pub angle: f64,
    /// `input`, `output`, `bidirectional`, `tri_state`, `passive` (global/hierarchical).
    pub shape: Option<String>,
    pub node: usize,
}

/// A junction dot or no-connect flag.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub at: Pt,
    pub node: usize,
}

/// A sheet pin (connects to the hierarchical label of the same name in the sheet).
#[derive(Debug, Clone, PartialEq)]
pub struct SheetPin {
    pub name: String,
    pub kind: String,
    pub at: Pt,
    pub angle: f64,
}

/// A hierarchical sheet placed on this sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub name: String,
    /// The sheet's file, relative to this one (untrusted).
    pub file: String,
    pub at: Pt,
    pub size: Pt,
    pub uuid: Option<String>,
    pub pins: Vec<SheetPin>,
    pub node: usize,
}

/// Free text on the sheet (drawn, not connected).
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub text: String,
    pub at: Pt,
    pub angle: f64,
    pub size: f64,
}

/// A schematic sheet file.
#[derive(Debug, Clone, Default)]
pub struct Schematic {
    /// The whole file as read; written back by [`Schematic::write`].
    pub tree: Node,
    pub version: Option<i64>,
    pub uuid: Option<String>,
    pub lib_symbols: Vec<LibSymbol>,
    pub symbols: Vec<Symbol>,
    pub wires: Vec<Wire>,
    /// Bus segments: members join through sheet pins on them (see [`crate::netlist`]).
    pub buses: Vec<Wire>,
    /// Bus entries: (position, size).
    pub bus_entries: Vec<(Pt, Pt)>,
    pub junctions: Vec<Marker>,
    pub no_connects: Vec<Marker>,
    pub labels: Vec<Label>,
    pub sheets: Vec<Sheet>,
    pub texts: Vec<Text>,
    /// Drawn lines (`polyline` at the top level), not connected.
    pub lines: Vec<Vec<Pt>>,
    /// Things read but not understood (at most a few dozen).
    pub notes: Vec<String>,
}

/// A point `(at x y ...)` / `(xy x y)` within range.
pub(crate) fn pt(n: Option<&Node>) -> Option<Pt> {
    let n = n?;
    let (x, y) = (n.num_arg(0)?, n.num_arg(1)?);
    (x.abs() <= MAX_COORD && y.abs() <= MAX_COORD).then_some([x, y])
}

fn num(n: &Node, head: &str) -> Option<f64> {
    n.child(head).and_then(|c| c.num_arg(0))
}

fn text(s: &str) -> String {
    s.chars().take(MAX_TEXT).collect()
}

/// `(hide yes)` / bare `hide` inside a node or its `effects`.
fn hidden(n: &Node) -> bool {
    let yes = |h: &Node| h.arg(0).is_none_or(|v| v == "yes");
    n.has_flag("hide")
        || n.child("hide").is_some_and(yes)
        || n.child("effects").is_some_and(|e| e.has_flag("hide") || e.child("hide").is_some_and(yes))
}

/// `(flag yes|no)`; `default` when absent.
fn yes_no(n: &Node, head: &str, default: bool) -> bool {
    n.child(head).map_or(default, |c| c.arg(0).is_none_or(|v| v == "yes"))
}

fn stroke_width(n: &Node) -> f64 {
    n.child("stroke").and_then(|s| num(s, "width")).unwrap_or(0.0).clamp(0.0, 100.0)
}

fn fill(n: &Node) -> Fill {
    let t = n.child("fill").and_then(|f| f.child("type").and_then(|t| t.arg(0)).or_else(|| f.arg(0)));
    match t {
        Some("outline") => Fill::Outline,
        Some("background" | "color") => Fill::Background,
        _ => Fill::None,
    }
}

fn pts_list(n: Option<&Node>, max: usize) -> Option<Vec<Pt>> {
    let mut out = Vec::new();
    for p in n?.children("xy") {
        out.push(pt(Some(p))?);
        if out.len() > max {
            return None;
        }
    }
    Some(out)
}

/// Unit and body style from a sub-symbol name `Name_U_S`.
fn unit_style(name: &str) -> (u32, u32) {
    let mut it = name.rsplitn(3, '_');
    let s = it.next().and_then(|t| t.parse().ok()).unwrap_or(0);
    let u = it.next().and_then(|t| t.parse().ok()).unwrap_or(0);
    (u.min(10_000), s.min(10))
}

fn lib_shape(n: &Node) -> Option<Shape> {
    Some(match n.head()? {
        "rectangle" => Shape::Rect { a: pt(n.child("start"))?, b: pt(n.child("end"))? },
        "polyline" => Shape::Poly { pts: pts_list(n.child("pts"), 100_000)? },
        "bezier" => Shape::Poly { pts: pts_list(n.child("pts"), 16)? },
        "circle" => {
            let r = num(n, "radius")?;
            (r.is_finite() && (0.0..MAX_COORD).contains(&r)).then_some(())?;
            Shape::Circle { center: pt(n.child("center"))?, radius: r }
        }
        "arc" => {
            let (start, end) = (pt(n.child("start"))?, pt(n.child("end"))?);
            let mid = match pt(n.child("mid")) {
                Some(m) => m,
                // KiCad 5 style `(radius (at) (length) (angles))`: a centre and two ends.
                None => {
                    let c = pt(n.child("radius").and_then(|r| r.child("at")))?;
                    let r = libm::hypot(start[0] - c[0], start[1] - c[1]);
                    let a0 = libm::atan2(start[1] - c[1], start[0] - c[0]);
                    let mut a1 = libm::atan2(end[1] - c[1], end[0] - c[0]);
                    if a1 < a0 {
                        a1 += std::f64::consts::TAU;
                    }
                    let m = (a0 + a1) / 2.0;
                    [c[0] + r * libm::cos(m), c[1] + r * libm::sin(m)]
                }
            };
            Shape::Arc { start, mid, end }
        }
        "text" => {
            let size = n.child("effects").and_then(|e| e.child("font")).and_then(|f| f.child("size"));
            Shape::Text {
                text: text(n.arg(0)?),
                at: pt(n.child("at"))?,
                size: size.and_then(|s| s.num_arg(0)).unwrap_or(1.27).clamp(0.01, 100.0),
            }
        }
        _ => return None,
    })
}

fn lib_pin(n: &Node) -> Option<LibPin> {
    let at = n.child("at")?;
    Some(LibPin {
        kind: PinType::parse(n.arg(0)?),
        style: text(n.arg(1).unwrap_or("line")),
        at: pt(Some(at))?,
        angle: at.num_arg(2).unwrap_or(0.0),
        length: num(n, "length").unwrap_or(2.54).clamp(0.0, 1000.0),
        name: n.child("name").and_then(|x| x.arg(0)).map(text).unwrap_or_default(),
        number: n.child("number").and_then(|x| x.arg(0)).map(text).unwrap_or_default(),
        hidden: hidden(n),
    })
}

fn lib_property(n: &Node) -> Option<LibProperty> {
    let at = n.child("at");
    Some(LibProperty {
        key: text(n.arg(0)?),
        value: text(n.arg(1).unwrap_or("")),
        at: pt(at).unwrap_or([0.0, 0.0]),
        angle: at.and_then(|a| a.num_arg(2)).unwrap_or(0.0),
        hidden: hidden(n),
    })
}

impl LibSymbol {
    /// Reads a `(symbol "Name" ...)` library node.
    ///
    /// # Errors
    /// [`Error::TooMany`] past `max_items` drawings or pins.
    pub fn from_node(n: &Node, max_items: usize) -> Result<Option<LibSymbol>, Error> {
        let Some(name) = n.arg(0) else { return Ok(None) };
        let mut s = LibSymbol {
            name: text(name),
            extends: n.child("extends").and_then(|e| e.arg(0)).map(text),
            power: n.child("power").is_some(),
            pin_name_offset: 0.508,
            ..LibSymbol::default()
        };
        if let Some(p) = n.child("pin_names") {
            s.hide_pin_names = hidden(p);
            s.pin_name_offset = num(p, "offset").unwrap_or(0.508).clamp(0.0, 100.0);
        }
        s.hide_pin_numbers = n.child("pin_numbers").is_some_and(hidden);
        s.properties = n.children("property").filter_map(lib_property).take(1000).collect();
        let mut count = 0usize;
        let mut add = |unit: &mut LibUnit, child: &Node| -> Result<(), Error> {
            count += 1;
            if count > max_items {
                return Err(Error::TooMany("symbol drawings"));
            }
            if child.head() == Some("pin") {
                if let Some(p) = lib_pin(child) {
                    unit.pins.push(p);
                }
            } else if let Some(shape) = lib_shape(child) {
                unit.graphics.push(LibGraphic { shape, width: stroke_width(child), fill: fill(child) });
            }
            Ok(())
        };
        // Items directly in the symbol (KiCad 6 allows them) belong to every unit.
        let mut common = LibUnit::default();
        for c in n.args() {
            if c.head() != Some("symbol") {
                add(&mut common, c)?;
            }
        }
        if !common.graphics.is_empty() || !common.pins.is_empty() {
            s.units.push(common);
        }
        for sub in n.children("symbol") {
            let (unit, style) = unit_style(sub.arg(0).unwrap_or(""));
            let mut u = LibUnit { unit, style, ..LibUnit::default() };
            for c in sub.args() {
                add(&mut u, c)?;
            }
            s.units.push(u);
        }
        Ok(Some(s))
    }
}

/// A `.kicad_sym` symbol library.
#[derive(Debug, Clone)]
pub struct SymbolLib {
    pub tree: Node,
    pub symbols: Vec<LibSymbol>,
}

impl SymbolLib {
    /// Reads a `.kicad_sym` file.
    ///
    /// # Errors
    /// [`Error`] for malformed files or files past `limits`.
    pub fn read(src: &str, limits: &Limits) -> Result<SymbolLib, Error> {
        let tree = sexpr::parse(src, &limits.sexpr)?;
        if tree.head() != Some("kicad_symbol_lib") {
            return Err(Error::WrongKind(tree.head().unwrap_or("").to_string()));
        }
        let mut symbols = Vec::new();
        for n in tree.children("symbol") {
            if symbols.len() >= limits.max_items {
                return Err(Error::TooMany("symbols"));
            }
            symbols.extend(LibSymbol::from_node(n, limits.max_items)?);
        }
        Ok(SymbolLib { tree, symbols })
    }

    /// Writes the library back.
    pub fn write(&self) -> String {
        let mut s = sexpr::write(&self.tree);
        s.push('\n');
        s
    }

    /// The names of the library's symbols.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.symbols.iter().map(|s| s.name.as_str())
    }

    /// The symbol's node with `extends` resolved (the parent's drawings and pins, the
    /// derived symbol's properties), named `lib_id` and ready for a schematic's
    /// `lib_symbols`. `None` if the symbol (or a parent) is missing or the chain loops.
    pub fn embedded_node(&self, name: &str, lib_id: &str) -> Option<Node> {
        let find = |n: &str| self.tree.children("symbol").find(|s| s.arg(0) == Some(n));
        let own = find(name)?;
        // The root of the `extends` chain gives the drawings.
        let mut base = own;
        for _ in 0..16 {
            let Some(parent) = base.child("extends").and_then(|e| e.arg(0)) else { break };
            base = find(parent)?;
        }
        if base.child("extends").is_some() {
            return None;
        }
        let base_name = base.arg(0)?;
        let mut items = vec![Node::sym("symbol"), Node::str(lib_id)];
        for c in base.args().iter().skip(1) {
            match c.head() {
                Some("property") => {}
                Some("symbol") => {
                    // Sub-symbols are named after the symbol (`R_0_1`), without library.
                    let sub = c.arg(0).unwrap_or("");
                    let suffix = sub.strip_prefix(base_name).unwrap_or(sub);
                    let mut c = c.clone();
                    if let Some(Node::Atom(a)) = c.items_mut().and_then(|v| v.get_mut(1)) {
                        a.text = format!("{name}{suffix}");
                    }
                    items.push(c);
                }
                _ => items.push(c.clone()),
            }
        }
        // Flags of the derived symbol (power, pin_names...) win over the parent's.
        if !std::ptr::eq(base, own) {
            for c in own.args().iter().skip(1) {
                if let Some(h) = c.head()
                    && !matches!(h, "property" | "symbol" | "extends")
                {
                    items.retain(|x| x.head() != Some(h));
                    items.push(c.clone());
                }
            }
        }
        // Properties: the symbol's own, else the parent's.
        let props = if own.children("property").next().is_some() { own } else { base };
        let at = items.iter().position(|x| x.head() == Some("symbol")).unwrap_or(items.len());
        let p: Vec<Node> = props.children("property").cloned().collect();
        items.splice(at..at, p);
        Some(Node::List(items))
    }
}

/// The reader's progress.
struct Reader<'a> {
    s: &'a mut Schematic,
    limits: &'a Limits,
}

impl Reader<'_> {
    fn note(&mut self, t: String) {
        if self.s.notes.len() < 40 {
            self.s.notes.push(t);
        }
    }

    fn check(&self, n: usize, what: &'static str) -> Result<(), Error> {
        if n >= self.limits.max_items { Err(Error::TooMany(what)) } else { Ok(()) }
    }

    fn symbol(&mut self, n: &Node, node: usize) -> Result<(), Error> {
        let Some(lib_id) = n.child("lib_id").and_then(|l| l.arg(0)) else {
            self.note("a symbol without lib_id was skipped".into());
            return Ok(());
        };
        let Some(at_node) = n.child("at") else { return Ok(()) };
        let Some(at) = pt(Some(at_node)) else { return Ok(()) };
        self.check(self.s.symbols.len(), "symbols")?;
        let mirror = match n.child("mirror").and_then(|m| m.arg(0)) {
            Some("x") => Mirror::X,
            Some("y") => Mirror::Y,
            _ => Mirror::None,
        };
        let properties = n
            .children("property")
            .take(1000)
            .filter_map(|p| {
                let a = p.child("at");
                Some(Property {
                    key: text(p.arg(0)?),
                    value: text(p.arg(1).unwrap_or("")),
                    at: pt(a).unwrap_or(at),
                    angle: a.and_then(|a| a.num_arg(2)).unwrap_or(0.0),
                    hidden: hidden(p),
                })
            })
            .collect();
        let mut instances = Vec::new();
        if let Some(inst) = n.child("instances") {
            for project in inst.children("project").take(100) {
                for path in project.children("path").take(10_000) {
                    instances.push(Instance {
                        path: text(path.arg(0).unwrap_or("")),
                        reference: text(path.child("reference").and_then(|r| r.arg(0)).unwrap_or("")),
                        unit: path.child("unit").and_then(|u| u.num_arg(0)).map(|u| u.clamp(0.0, 10_000.0) as u32),
                    });
                }
            }
        }
        let unit = num(n, "unit").unwrap_or(1.0).clamp(1.0, 10_000.0) as u32;
        let style = num(n, "body_style").or_else(|| num(n, "convert")).unwrap_or(1.0).clamp(1.0, 10.0) as u32;
        self.s.symbols.push(Symbol {
            lib_id: text(lib_id),
            lib_name: n.child("lib_name").and_then(|l| l.arg(0)).map(text),
            at,
            angle: at_node.num_arg(2).unwrap_or(0.0),
            mirror,
            unit,
            style,
            uuid: n.child("uuid").and_then(|u| u.arg(0)).map(text),
            properties,
            instances,
            in_bom: yes_no(n, "in_bom", true),
            on_board: yes_no(n, "on_board", true),
            dnp: yes_no(n, "dnp", false),
            node,
        });
        Ok(())
    }

    fn item(&mut self, n: &Node, node: usize) -> Result<(), Error> {
        let s = &mut *self.s;
        match n.head().unwrap_or("") {
            "lib_symbols" => {
                for l in n.children("symbol") {
                    self.check(self.s.lib_symbols.len(), "library symbols")?;
                    let l = LibSymbol::from_node(l, self.limits.max_items)?;
                    self.s.lib_symbols.extend(l);
                }
            }
            "symbol" => self.symbol(n, node)?,
            h @ ("wire" | "bus") => {
                let Some(p) = pts_list(n.child("pts"), 1000) else { return Ok(()) };
                let list = if h == "wire" { &mut s.wires } else { &mut s.buses };
                for w in p.windows(2) {
                    if let [a, b] = *w {
                        list.push(Wire { a, b, node });
                    }
                }
                let n = list.len();
                self.check(n, "wires")?;
            }
            "bus_entry" => {
                if let (Some(a), Some(sz)) = (pt(n.child("at")), pt(n.child("size"))) {
                    s.bus_entries.push((a, sz));
                    let n = s.bus_entries.len();
                    self.check(n, "bus entries")?;
                }
            }
            h @ ("junction" | "no_connect") => {
                if let Some(at) = pt(n.child("at")) {
                    let list = if h == "junction" { &mut s.junctions } else { &mut s.no_connects };
                    list.push(Marker { at, node });
                    let n = list.len();
                    self.check(n, "junctions")?;
                }
            }
            h @ ("label" | "global_label" | "hierarchical_label") => {
                let kind = match h {
                    "label" => LabelKind::Local,
                    "global_label" => LabelKind::Global,
                    _ => LabelKind::Hierarchical,
                };
                let at = n.child("at");
                if let (Some(t), Some(p)) = (n.arg(0), pt(at)) {
                    s.labels.push(Label {
                        kind,
                        text: text(t),
                        at: p,
                        angle: at.and_then(|a| a.num_arg(2)).unwrap_or(0.0),
                        shape: n.child("shape").and_then(|x| x.arg(0)).map(text),
                        node,
                    });
                    let n = s.labels.len();
                    self.check(n, "labels")?;
                }
            }
            "sheet" => {
                let (Some(at), Some(size)) = (pt(n.child("at")), pt(n.child("size"))) else { return Ok(()) };
                let prop =
                    |k: &str| n.children("property").find(|p| p.arg(0) == Some(k)).and_then(|p| p.arg(1)).map(text);
                // KiCad 6 wrote `Sheet name` / `Sheet file`.
                let name = prop("Sheetname").or_else(|| prop("Sheet name")).unwrap_or_default();
                let file = prop("Sheetfile").or_else(|| prop("Sheet file")).unwrap_or_default();
                let pins = n
                    .children("pin")
                    .take(10_000)
                    .filter_map(|p| {
                        let a = p.child("at");
                        Some(SheetPin {
                            name: text(p.arg(0)?),
                            kind: text(p.arg(1).unwrap_or("passive")),
                            at: pt(a)?,
                            angle: a.and_then(|a| a.num_arg(2)).unwrap_or(0.0),
                        })
                    })
                    .collect();
                s.sheets.push(Sheet {
                    name,
                    file,
                    at,
                    size,
                    uuid: n.child("uuid").and_then(|u| u.arg(0)).map(text),
                    pins,
                    node,
                });
                let n = s.sheets.len();
                self.check(n, "sheets")?;
            }
            "text" => {
                let at = n.child("at");
                if let (Some(t), Some(p)) = (n.arg(0), pt(at)) {
                    let size = n.child("effects").and_then(|e| e.child("font")).and_then(|f| f.child("size"));
                    s.texts.push(Text {
                        text: text(t),
                        at: p,
                        angle: at.and_then(|a| a.num_arg(2)).unwrap_or(0.0),
                        size: size.and_then(|s| s.num_arg(0)).unwrap_or(1.27).clamp(0.01, 100.0),
                    });
                    let n = s.texts.len();
                    self.check(n, "texts")?;
                }
            }
            "polyline" => {
                if let Some(p) = pts_list(n.child("pts"), 10_000) {
                    s.lines.push(p);
                    let n = s.lines.len();
                    self.check(n, "lines")?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Schematic {
    /// Reads a `.kicad_sch` file.
    ///
    /// # Errors
    /// [`Error`] for malformed files or files past `limits`.
    pub fn read(src: &str, limits: &Limits) -> Result<Schematic, Error> {
        let tree = sexpr::parse(src, &limits.sexpr)?;
        Schematic::from_tree(tree, limits)
    }

    /// The schematic of an already-parsed tree.
    ///
    /// # Errors
    /// [`Error`] for a tree that is not a schematic or is past `limits`.
    pub fn from_tree(tree: Node, limits: &Limits) -> Result<Schematic, Error> {
        if tree.head() != Some("kicad_sch") {
            return Err(Error::WrongKind(tree.head().unwrap_or("").to_string()));
        }
        let mut s = Schematic {
            version: tree.child("version").and_then(|v| v.num_arg(0)).map(|v| v as i64),
            uuid: tree.child("uuid").and_then(|u| u.arg(0)).map(text),
            ..Schematic::default()
        };
        let mut r = Reader { s: &mut s, limits };
        for (i, n) in tree.items().iter().enumerate().skip(1) {
            r.item(n, i)?;
        }
        if let Some(v) = s.version
            && v < 20_200_000
        {
            s.notes.push(format!("file version {v} is older than KiCad 6; it may not read fully"));
        }
        s.tree = tree;
        Ok(s)
    }

    /// Writes the schematic (the tree as read and edited).
    pub fn write(&self) -> String {
        let mut s = sexpr::write(&self.tree);
        s.push('\n');
        s
    }

    /// The library symbol a placed symbol draws from.
    pub fn lib_symbol(&self, s: &Symbol) -> Option<&LibSymbol> {
        let key = s.lib_key();
        self.lib_symbols.iter().find(|l| l.name == key)
    }

    /// Whether a symbol is a power symbol (its library symbol says so, or its reference
    /// starts with `#PWR`).
    pub fn is_power(&self, s: &Symbol) -> bool {
        self.lib_symbol(s).is_some_and(|l| l.power) || s.reference().starts_with("#PWR")
    }

    /// The pins of a placed symbol on the sheet.
    pub fn pins(&self, s: &Symbol) -> Vec<PlacedPin> {
        let Some(l) = self.lib_symbol(s) else { return Vec::new() };
        l.pins_of(s.unit, s.style)
            .map(|p| PlacedPin {
                number: p.number.clone(),
                name: p.name.clone(),
                kind: p.kind,
                at: s.place(p.at),
                hidden: p.hidden,
            })
            .collect()
    }

    /// A symbol by reference (`R1`) or `#index`.
    pub fn symbol_index(&self, name: &str) -> Option<usize> {
        if let Some(i) = name.strip_prefix('#').and_then(|t| t.parse::<usize>().ok()) {
            return (i < self.symbols.len()).then_some(i);
        }
        self.symbols.iter().position(|s| s.reference() == name)
    }

    /// The project name the symbols' instances use, if any.
    pub fn project_name(&self) -> Option<String> {
        let inst = self.tree.children("symbol").find_map(|s| s.child("instances"))?;
        inst.child("project").and_then(|p| p.arg(0)).map(text)
    }
}

/// A pin of a placed symbol, on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedPin {
    pub number: String,
    pub name: String,
    pub kind: PinType,
    pub at: Pt,
    pub hidden: bool,
}
