//! 2D drawings (ADR 0032): sheets of projected views, dimensions and notes, kept in the
//! document after the features. A drawing changes no body; it is built on demand from the
//! evaluated model.
//!
//! ```text
//! drawing Plate_dwg (size: A3, projection: first, scale: 0.5, title: "Plate") {
//!   view Front (bodies: [Plate], from: front, at: [130, 160])
//!   view Top (of: Front, side: top)
//!   dim D1 (view: Front, kind: length, a: edge(...))
//!   note N1 (text: "Break sharp edges", at: [20, 30])
//! }
//! ```

use crate::cursor::{Cursor, ParseError, quote};
use crate::refs::{EdgeRef, parse_edge};
use crate::text::{missing, parse_args, parse_bool, parse_ident_list, parse_pair, perr, split_comment, unknown_arg};
use gusset_units::expr::Expr;
use std::fmt::Write as _;

/// Items one drawing may hold (views, dimensions and notes together).
pub const MAX_DRAWING_ITEMS: usize = 10_000;

/// A drawing: one sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    /// Identifier, unique among features and drawings.
    pub name: String,
    pub size: SheetSize,
    /// Landscape (default) or portrait.
    pub landscape: bool,
    pub projection: Projection,
    /// Sheet scale: an expression without unit (0.5 is 1:2). `None`: 1.
    pub scale: Option<Expr>,
    /// Unit of dimension values; `None`: the document's (dogfooding F5-14).
    pub units: Option<gusset_units::quantity::LengthUnit>,
    /// Decimal places of dimension values (0 to 6); `None`: 2 in mm, 3 in inches.
    pub decimals: Option<u32>,
    /// Title block fields (user text).
    pub title: Option<String>,
    pub number: Option<String>,
    pub material: Option<String>,
    pub author: Option<String>,
    pub items: Vec<DrawingItem>,
}

impl Drawing {
    /// An empty A3 landscape sheet in first-angle projection.
    pub fn new(name: impl Into<String>) -> Drawing {
        Drawing {
            name: name.into(),
            size: SheetSize::A3,
            landscape: true,
            projection: Projection::First,
            scale: None,
            units: None,
            decimals: None,
            title: None,
            number: None,
            material: None,
            author: None,
            items: Vec::new(),
        }
    }

    /// The item named `name`.
    pub fn item(&self, name: &str) -> Option<&DrawingItem> {
        self.items.iter().find(|i| i.name() == name)
    }

    /// The view named `name`.
    pub fn view(&self, name: &str) -> Option<&ViewDef> {
        self.items.iter().find_map(|i| match i {
            DrawingItem::View(v) if v.name == name => Some(v),
            _ => None,
        })
    }
}

/// ISO 216 A sizes and ANSI Y14.1 sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetSize {
    A0,
    A1,
    A2,
    A3,
    A4,
    AnsiA,
    AnsiB,
    AnsiC,
    AnsiD,
    AnsiE,
}

impl SheetSize {
    pub const ALL: [SheetSize; 10] = [
        SheetSize::A0,
        SheetSize::A1,
        SheetSize::A2,
        SheetSize::A3,
        SheetSize::A4,
        SheetSize::AnsiA,
        SheetSize::AnsiB,
        SheetSize::AnsiC,
        SheetSize::AnsiD,
        SheetSize::AnsiE,
    ];

    pub fn keyword(self) -> &'static str {
        match self {
            SheetSize::A0 => "A0",
            SheetSize::A1 => "A1",
            SheetSize::A2 => "A2",
            SheetSize::A3 => "A3",
            SheetSize::A4 => "A4",
            SheetSize::AnsiA => "ANSI_A",
            SheetSize::AnsiB => "ANSI_B",
            SheetSize::AnsiC => "ANSI_C",
            SheetSize::AnsiD => "ANSI_D",
            SheetSize::AnsiE => "ANSI_E",
        }
    }

    pub fn from_keyword(s: &str) -> Option<SheetSize> {
        SheetSize::ALL.into_iter().find(|z| z.keyword().eq_ignore_ascii_case(s))
    }

    /// Width and height in mm, long side first.
    pub fn mm(self) -> [f64; 2] {
        match self {
            SheetSize::A0 => [1189.0, 841.0],
            SheetSize::A1 => [841.0, 594.0],
            SheetSize::A2 => [594.0, 420.0],
            SheetSize::A3 => [420.0, 297.0],
            SheetSize::A4 => [297.0, 210.0],
            SheetSize::AnsiA => [279.4, 215.9],
            SheetSize::AnsiB => [431.8, 279.4],
            SheetSize::AnsiC => [558.8, 431.8],
            SheetSize::AnsiD => [863.6, 558.8],
            SheetSize::AnsiE => [1117.6, 863.6],
        }
    }
}

/// Where projected views go (ISO 128-30): first angle puts the top view below the front
/// view; third angle (ASME) puts it above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projection {
    First,
    Third,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DrawingItem {
    View(ViewDef),
    Dim(DimDef),
    Note(NoteDef),
    /// The bill of materials as a table (one row per part).
    PartsList(PartsListDef),
    /// A balloon: the parts-list item number of `body`, with a leader to it in `view`.
    Balloon(BalloonDef),
    /// A GD&T feature control frame with a leader to an edge.
    Gdt(GdtDef),
    /// A datum feature symbol on an edge.
    Datum(DatumDef),
}

impl DrawingItem {
    pub fn name(&self) -> &str {
        match self {
            DrawingItem::View(v) => &v.name,
            DrawingItem::Dim(d) => &d.name,
            DrawingItem::Note(n) => &n.name,
            DrawingItem::PartsList(p) => &p.name,
            DrawingItem::Balloon(b) => &b.name,
            DrawingItem::Gdt(g) => &g.name,
            DrawingItem::Datum(d) => &d.name,
        }
    }
}

/// A view of bodies on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewDef {
    pub name: String,
    pub placement: ViewPlacement,
    /// `None`: the sheet's scale.
    pub scale: Option<Expr>,
    /// Hidden lines; `None`: shown, except in iso views.
    pub hidden: Option<bool>,
    /// Smooth (tangent) edges.
    pub tangent: bool,
    /// Centre marks on circles and centrelines on cylinders (default on).
    pub centers: bool,
    /// A full section (projected views only): the parent is cut by a plane and the part
    /// between the plane and the viewer is removed.
    pub section: Option<SectionDef>,
}

/// A full section view (ISO 128-40): `label` names the cutting plane ("A" gives "A–A"),
/// `cut_at` places it along the axis of the parent view that points at this view's side
/// (a model length from the origin, plain numbers in the document unit; `None`: through the middle of the bodies).
#[derive(Debug, Clone, PartialEq)]
pub struct SectionDef {
    pub label: String,
    pub cut_at: Option<Expr>,
    /// A half section: only the half beyond the part's middle along the view's X is cut,
    /// the other half is drawn whole.
    pub half: bool,
    /// An offset section: `[at, cut_at]` pairs, in order. From `at` along the cutting line
    /// (model length, the parent's axis the line runs along) the plane moves to `cut_at`.
    pub steps: Vec<[Expr; 2]>,
}

/// Most steps of one offset section.
pub const MAX_SECTION_STEPS: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub enum ViewPlacement {
    /// A base view: `bodies` (empty: every body) seen `from`, centred at `at` (sheet mm).
    Base { bodies: Vec<String>, from: ViewDir, at: [Expr; 2] },
    /// A view projected from `of`, aligned with it on `side`, `gap` apart (default 20 mm).
    Projected { of: String, side: Side, gap: Option<Expr> },
    /// A circular detail of `of`: the circle of `radius` (a model length) around `center` (the
    /// parent view's axes; plain numbers in the document unit), enlarged and centred at `at` (sheet mm).
    Detail { of: String, center: [Expr; 2], radius: Expr, at: [Expr; 2] },
}

impl ViewPlacement {
    /// The view this one is derived from.
    pub fn parent(&self) -> Option<&str> {
        match self {
            ViewPlacement::Base { .. } => None,
            ViewPlacement::Projected { of, .. } | ViewPlacement::Detail { of, .. } => Some(of),
        }
    }
}

/// The named view directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewDir {
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
    Iso,
    IsoBack,
}

impl ViewDir {
    pub const ALL: [ViewDir; 8] = [
        ViewDir::Front,
        ViewDir::Back,
        ViewDir::Top,
        ViewDir::Bottom,
        ViewDir::Left,
        ViewDir::Right,
        ViewDir::Iso,
        ViewDir::IsoBack,
    ];

    pub fn keyword(self) -> &'static str {
        match self {
            ViewDir::Front => "front",
            ViewDir::Back => "back",
            ViewDir::Top => "top",
            ViewDir::Bottom => "bottom",
            ViewDir::Left => "left",
            ViewDir::Right => "right",
            ViewDir::Iso => "iso",
            ViewDir::IsoBack => "iso_back",
        }
    }

    pub fn from_keyword(s: &str) -> Option<ViewDir> {
        ViewDir::ALL.into_iter().find(|d| d.keyword() == s)
    }

    /// The direction towards the viewer and the view's X axis on the sheet (Z up in the
    /// front, side and iso views; Y up in the top view).
    pub fn axes(self) -> ([f64; 3], [f64; 3]) {
        match self {
            ViewDir::Front => ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
            ViewDir::Back => ([0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]),
            ViewDir::Top => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            ViewDir::Bottom => ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0]),
            ViewDir::Right => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ViewDir::Left => ([-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
            ViewDir::Iso => ([1.0, -1.0, 1.0], [1.0, 1.0, 0.0]),
            ViewDir::IsoBack => ([-1.0, 1.0, 1.0], [-1.0, -1.0, 0.0]),
        }
    }

    pub fn is_iso(self) -> bool {
        matches!(self, ViewDir::Iso | ViewDir::IsoBack)
    }
}

/// The side of a parent view a projected view is seen from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    pub fn keyword(self) -> &'static str {
        match self {
            Side::Top => "top",
            Side::Bottom => "bottom",
            Side::Left => "left",
            Side::Right => "right",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Side> {
        [Side::Top, Side::Bottom, Side::Left, Side::Right].into_iter().find(|d| d.keyword() == s)
    }
}

/// A driven dimension: measured from the model every time, never stored.
#[derive(Debug, Clone, PartialEq)]
pub struct DimDef {
    pub name: String,
    /// The view it is measured in.
    pub view: String,
    pub kind: DimKind,
    pub a: EdgeRef,
    pub b: Option<EdgeRef>,
    pub direction: DimDirection,
    /// Distance of the dimension line from the measured geometry (sheet mm).
    pub offset: Option<Expr>,
    /// Text position (sheet mm), when moved by hand.
    pub text_at: Option<[Expr; 2]>,
    /// Tolerance text, e.g. "±0.1" (user text).
    pub tolerance: Option<String>,
    /// Prefix, e.g. "4×" (user text).
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimKind {
    Length,
    Distance,
    Diameter,
    Radius,
    Angle,
}

impl DimKind {
    pub fn keyword(self) -> &'static str {
        match self {
            DimKind::Length => "length",
            DimKind::Distance => "distance",
            DimKind::Diameter => "diameter",
            DimKind::Radius => "radius",
            DimKind::Angle => "angle",
        }
    }

    pub fn from_keyword(s: &str) -> Option<DimKind> {
        [DimKind::Length, DimKind::Distance, DimKind::Diameter, DimKind::Radius, DimKind::Angle]
            .into_iter()
            .find(|d| d.keyword() == s)
    }

    /// Whether the kind measures between two references.
    pub fn takes_two(self) -> bool {
        matches!(self, DimKind::Distance | DimKind::Angle)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DimDirection {
    /// Along the measured geometry.
    #[default]
    Aligned,
    Horizontal,
    Vertical,
}

impl DimDirection {
    pub fn keyword(self) -> &'static str {
        match self {
            DimDirection::Aligned => "aligned",
            DimDirection::Horizontal => "horizontal",
            DimDirection::Vertical => "vertical",
        }
    }

    pub fn from_keyword(s: &str) -> Option<DimDirection> {
        [DimDirection::Aligned, DimDirection::Horizontal, DimDirection::Vertical].into_iter().find(|d| d.keyword() == s)
    }
}

/// A parts list: the bodies grouped into parts, as a table whose top-left corner is `at`
/// (sheet mm).
#[derive(Debug, Clone, PartialEq)]
pub struct PartsListDef {
    pub name: String,
    pub at: [Expr; 2],
}

/// A balloon at `at` (sheet mm) naming the part `body` is, with a leader to the body's
/// centre as `view` shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct BalloonDef {
    pub name: String,
    pub view: String,
    pub body: String,
    pub at: [Expr; 2],
}

/// A geometric characteristic of ISO 1101 / ASME Y14.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GdtSymbol {
    Straightness,
    Flatness,
    Circularity,
    Cylindricity,
    ProfileLine,
    ProfileSurface,
    Perpendicularity,
    Angularity,
    Parallelism,
    Position,
    Concentricity,
    Symmetry,
    Runout,
    TotalRunout,
}

impl GdtSymbol {
    pub const ALL: [GdtSymbol; 14] = [
        GdtSymbol::Straightness,
        GdtSymbol::Flatness,
        GdtSymbol::Circularity,
        GdtSymbol::Cylindricity,
        GdtSymbol::ProfileLine,
        GdtSymbol::ProfileSurface,
        GdtSymbol::Perpendicularity,
        GdtSymbol::Angularity,
        GdtSymbol::Parallelism,
        GdtSymbol::Position,
        GdtSymbol::Concentricity,
        GdtSymbol::Symmetry,
        GdtSymbol::Runout,
        GdtSymbol::TotalRunout,
    ];

    pub fn keyword(self) -> &'static str {
        match self {
            GdtSymbol::Straightness => "straightness",
            GdtSymbol::Flatness => "flatness",
            GdtSymbol::Circularity => "circularity",
            GdtSymbol::Cylindricity => "cylindricity",
            GdtSymbol::ProfileLine => "profile_line",
            GdtSymbol::ProfileSurface => "profile_surface",
            GdtSymbol::Perpendicularity => "perpendicularity",
            GdtSymbol::Angularity => "angularity",
            GdtSymbol::Parallelism => "parallelism",
            GdtSymbol::Position => "position",
            GdtSymbol::Concentricity => "concentricity",
            GdtSymbol::Symmetry => "symmetry",
            GdtSymbol::Runout => "runout",
            GdtSymbol::TotalRunout => "total_runout",
        }
    }

    pub fn from_keyword(w: &str) -> Option<GdtSymbol> {
        GdtSymbol::ALL.into_iter().find(|s| s.keyword() == w)
    }

    /// Form tolerances take no datum.
    pub fn takes_datums(self) -> bool {
        !matches!(
            self,
            GdtSymbol::Straightness | GdtSymbol::Flatness | GdtSymbol::Circularity | GdtSymbol::Cylindricity
        )
    }
}

/// A feature control frame at `at` (its lower-left corner, sheet mm): `symbol`, the
/// `tolerance` text (user text, e.g. "Ø0.1 M") and up to three datum letters, with a leader to
/// edge `a` in `view`.
#[derive(Debug, Clone, PartialEq)]
pub struct GdtDef {
    pub name: String,
    pub view: String,
    pub a: EdgeRef,
    pub symbol: GdtSymbol,
    pub tolerance: String,
    pub datums: Vec<String>,
    pub at: [Expr; 2],
}

/// A datum feature symbol: its letter is the item's name, boxed at `at`, joined to edge `a`
/// in `view` by a filled triangle.
#[derive(Debug, Clone, PartialEq)]
pub struct DatumDef {
    pub name: String,
    pub view: String,
    pub a: EdgeRef,
    pub at: [Expr; 2],
}

/// A text note on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteDef {
    pub name: String,
    /// User text.
    pub text: String,
    pub at: [Expr; 2],
}

// ------------------------------------------------------------------ text

fn pair(p: &[Expr; 2]) -> String {
    format!("[{}, {}]", p[0], p[1])
}

/// Most decimal places a drawing shows.
pub const MAX_DECIMALS: u32 = 6;

/// `mm` or `in`.
pub fn unit_keyword(u: gusset_units::quantity::LengthUnit) -> &'static str {
    match u {
        gusset_units::quantity::LengthUnit::Millimetre => "mm",
        gusset_units::quantity::LengthUnit::Inch => "in",
    }
}

/// The unit a drawing keyword names.
pub fn unit_of_keyword(w: &str) -> Option<gusset_units::quantity::LengthUnit> {
    match w {
        "mm" => Some(gusset_units::quantity::LengthUnit::Millimetre),
        "in" => Some(gusset_units::quantity::LengthUnit::Inch),
        _ => None,
    }
}

/// Appends the canonical text of `d`.
pub(crate) fn print_drawing(out: &mut String, d: &Drawing) {
    let mut args = vec![format!("size: {}", d.size.keyword())];
    if !d.landscape {
        args.push("landscape: false".into());
    }
    args.push(format!("projection: {}", if d.projection == Projection::First { "first" } else { "third" }));
    if let Some(s) = &d.scale {
        args.push(format!("scale: {s}"));
    }
    if let Some(u) = d.units {
        args.push(format!("units: {}", unit_keyword(u)));
    }
    if let Some(n) = d.decimals {
        args.push(format!("decimals: {n}"));
    }
    for (k, v) in [("title", &d.title), ("number", &d.number), ("material", &d.material), ("author", &d.author)] {
        if let Some(v) = v {
            args.push(format!("{k}: {}", quote(v)));
        }
    }
    let _ = writeln!(out, "drawing {} ({}) {{", d.name, args.join(", "));
    for item in &d.items {
        let _ = writeln!(out, "  {}", print_item(item));
    }
    out.push_str("}\n");
}

fn print_item(item: &DrawingItem) -> String {
    match item {
        DrawingItem::View(v) => {
            let mut args = Vec::new();
            match &v.placement {
                ViewPlacement::Base { bodies, from, at } => {
                    if !bodies.is_empty() {
                        args.push(format!("bodies: [{}]", bodies.join(", ")));
                    }
                    args.push(format!("from: {}", from.keyword()));
                    args.push(format!("at: {}", pair(at)));
                }
                ViewPlacement::Projected { of, side, gap } => {
                    args.push(format!("of: {of}"));
                    args.push(format!("side: {}", side.keyword()));
                    if let Some(g) = gap {
                        args.push(format!("gap: {g}"));
                    }
                }
                ViewPlacement::Detail { of, center, radius, at } => {
                    args.push(format!("detail_of: {of}"));
                    args.push(format!("center: {}", pair(center)));
                    args.push(format!("radius: {radius}"));
                    args.push(format!("at: {}", pair(at)));
                }
            }
            if let Some(s) = &v.scale {
                args.push(format!("scale: {s}"));
            }
            if let Some(h) = v.hidden {
                args.push(format!("hidden: {h}"));
            }
            if let Some(sec) = &v.section {
                args.push(format!("section: {}", sec.label));
                if let Some(e) = &sec.cut_at {
                    args.push(format!("cut_at: {e}"));
                }
                if sec.half {
                    args.push("half: true".into());
                }
                if !sec.steps.is_empty() {
                    let s: Vec<String> = sec.steps.iter().map(pair).collect();
                    args.push(format!("steps: [{}]", s.join(", ")));
                }
            }
            if !v.centers {
                args.push("centers: false".into());
            }
            if v.tangent {
                args.push("tangent: true".into());
            }
            format!("view {} ({})", v.name, args.join(", "))
        }
        DrawingItem::Dim(d) => {
            let mut args = vec![format!("view: {}", d.view), format!("kind: {}", d.kind.keyword())];
            args.push(format!("a: {}", d.a));
            if let Some(b) = &d.b {
                args.push(format!("b: {b}"));
            }
            if d.direction != DimDirection::Aligned {
                args.push(format!("direction: {}", d.direction.keyword()));
            }
            if let Some(o) = &d.offset {
                args.push(format!("offset: {o}"));
            }
            if let Some(t) = &d.text_at {
                args.push(format!("text_at: {}", pair(t)));
            }
            if let Some(t) = &d.tolerance {
                args.push(format!("tolerance: {}", quote(t)));
            }
            if let Some(p) = &d.prefix {
                args.push(format!("prefix: {}", quote(p)));
            }
            format!("dim {} ({})", d.name, args.join(", "))
        }
        DrawingItem::Note(n) => format!("note {} (text: {}, at: {})", n.name, quote(&n.text), pair(&n.at)),
        DrawingItem::PartsList(p) => format!("parts_list {} (at: {})", p.name, pair(&p.at)),
        DrawingItem::Balloon(b) => {
            format!("balloon {} (view: {}, body: {}, at: {})", b.name, b.view, b.body, pair(&b.at))
        }
        DrawingItem::Gdt(g) => {
            let datums =
                if g.datums.is_empty() { String::new() } else { format!(", datums: [{}]", g.datums.join(", ")) };
            format!(
                "gdt {} (view: {}, a: {}, symbol: {}, tolerance: {}{datums}, at: {})",
                g.name,
                g.view,
                g.a,
                g.symbol.keyword(),
                quote(&g.tolerance),
                pair(&g.at)
            )
        }
        DrawingItem::Datum(d) => format!("datum {} (view: {}, a: {}, at: {})", d.name, d.view, d.a, pair(&d.at)),
    }
}

fn keyword<T>(c: &mut Cursor<'_>, what: &str, list: &str, f: impl Fn(&str) -> Option<T>) -> Result<T, ParseError> {
    let w = c.ident()?;
    f(w).ok_or_else(|| c.err_hint(format!("unknown {what} '{w}'"), format!("use {list}")))
}

/// Parses `drawing Name (args) {` and its block. Returns the drawing and the index of the
/// line after the closing `}`.
pub(crate) fn parse_drawing(c: &mut Cursor<'_>, lines: &[&str], mut i: usize) -> Result<(Drawing, usize), ParseError> {
    const KW: &str = "drawing";
    let mut d = Drawing::new(c.ident()?);
    parse_args(c, |key, c| {
        match key {
            "size" => {
                d.size = keyword(c, "sheet size", "A0-A4 or ANSI_A-ANSI_E", SheetSize::from_keyword)?;
            }
            "landscape" => d.landscape = parse_bool(c)?,
            "projection" => {
                d.projection = keyword(c, "projection", "first or third", |w| match w {
                    "first" => Some(Projection::First),
                    "third" => Some(Projection::Third),
                    _ => None,
                })?;
            }
            "scale" => d.scale = Some(c.expr()?),
            "units" => d.units = Some(keyword(c, "drawing units", "mm or in", unit_of_keyword)?),
            "decimals" => {
                let n = c.uint()?;
                if n > MAX_DECIMALS {
                    return Err(c.err(format!("decimals is {n}; at most {MAX_DECIMALS}")));
                }
                d.decimals = Some(n);
            }
            "title" => d.title = Some(c.string()?),
            "number" => d.number = Some(c.string()?),
            "material" => d.material = Some(c.string()?),
            "author" => d.author = Some(c.string()?),
            key => {
                return Err(unknown_arg(
                    c,
                    KW,
                    key,
                    "size, landscape, projection, scale, units, decimals, title, number, material, author",
                ));
            }
        }
        Ok(())
    })?;
    c.expect('{')?;
    if !c.at_end() {
        return Err(c.err("drawing contents start on the next line after '{'"));
    }
    let open_line = c.line;
    loop {
        let Some(raw) = lines.get(i).copied() else {
            let mut e = perr(open_line, format!("drawing '{}' is not closed", d.name));
            e.hint = Some("add a line with '}'".into());
            return Err(e);
        };
        let lineno = i + 1;
        i += 1;
        let (code, _) = split_comment(raw);
        if code.trim().is_empty() {
            continue;
        }
        if code.trim() == "}" {
            break;
        }
        if d.items.len() >= MAX_DRAWING_ITEMS {
            return Err(perr(lineno, format!("drawing has more than {MAX_DRAWING_ITEMS} items")));
        }
        let mut lc = Cursor::new(code, lineno);
        let item = match lc.ident()? {
            "view" => DrawingItem::View(parse_view(&mut lc)?),
            "dim" => DrawingItem::Dim(parse_dim(&mut lc)?),
            "note" => DrawingItem::Note(parse_note(&mut lc)?),
            "parts_list" => DrawingItem::PartsList(parse_parts_list(&mut lc)?),
            "balloon" => DrawingItem::Balloon(parse_balloon(&mut lc)?),
            "gdt" => DrawingItem::Gdt(parse_gdt(&mut lc)?),
            "datum" => DrawingItem::Datum(parse_datum(&mut lc)?),
            other => {
                return Err(lc.err_hint(
                    format!("unknown drawing item '{other}'"),
                    "items are: view, dim, note, parts_list, balloon, gdt, datum",
                ));
            }
        };
        if !lc.at_end() {
            return Err(lc.err("unexpected text after ')'"));
        }
        if d.items.iter().any(|x| x.name() == item.name()) {
            return Err(lc.err(format!("'{}' is defined twice in drawing '{}'", item.name(), d.name)));
        }
        d.items.push(item);
    }
    Ok((d, i))
}

fn parse_view(c: &mut Cursor<'_>) -> Result<ViewDef, ParseError> {
    const KW: &str = "view";
    let name = c.ident()?.to_string();
    let (mut bodies, mut from, mut at) = (None, None, None);
    let (mut of, mut side, mut gap) = (None, None, None);
    let (mut scale, mut hidden, mut tangent, mut centers) = (None, None, false, true);
    let (mut section, mut cut_at, mut half, mut steps) = (None, None, false, Vec::new());
    let (mut detail_of, mut center, mut radius) = (None, None, None);
    parse_args(c, |key, c| {
        match key {
            "bodies" => bodies = Some(parse_ident_list(c, "bodies")?),
            "from" => {
                from = Some(keyword(
                    c,
                    "view direction",
                    "front, back, top, bottom, left, right, iso or iso_back",
                    ViewDir::from_keyword,
                )?);
            }
            "at" => at = Some(parse_pair(c)?),
            "of" => of = Some(c.ident()?.to_string()),
            "side" => side = Some(keyword(c, "side", "top, bottom, left or right", Side::from_keyword)?),
            "gap" => gap = Some(c.expr()?),
            "scale" => scale = Some(c.expr()?),
            "hidden" => hidden = Some(parse_bool(c)?),
            "tangent" => tangent = parse_bool(c)?,
            "centers" => centers = parse_bool(c)?,
            "section" => section = Some(c.ident()?.to_string()),
            "detail_of" => detail_of = Some(c.ident()?.to_string()),
            "center" => center = Some(parse_pair(c)?),
            "radius" => radius = Some(c.expr()?),
            "cut_at" => cut_at = Some(c.expr()?),
            "half" => half = parse_bool(c)?,
            "steps" => {
                c.expect('[')?;
                if !c.eat(']') {
                    loop {
                        steps.push(parse_pair(c)?);
                        if steps.len() > MAX_SECTION_STEPS {
                            return Err(c.err(format!("a section has at most {MAX_SECTION_STEPS} steps")));
                        }
                        if !c.eat(',') {
                            break;
                        }
                    }
                    c.expect(']')?;
                }
            }
            key => {
                return Err(unknown_arg(
                    c,
                    KW,
                    key,
                    "bodies, from, at, of, side, gap, section, cut_at, half, steps, detail_of, center, radius, scale, hidden, \
                     tangent, centers",
                ));
            }
        }
        Ok(())
    })?;
    if let Some(dof) = detail_of {
        // `of` and `cut_at` too: they would be dropped on the next print (review N2).
        if from.is_some()
            || of.is_some()
            || side.is_some()
            || gap.is_some()
            || bodies.is_some()
            || section.is_some()
            || cut_at.is_some()
        {
            return Err(c.err_hint(
                "a detail view takes detail_of, center, radius and at",
                "remove from/of/side/gap/bodies/section/cut_at",
            ));
        }
        let placement = ViewPlacement::Detail {
            of: dof,
            center: center.ok_or_else(|| missing(c, KW, "center"))?,
            radius: radius.ok_or_else(|| missing(c, KW, "radius"))?,
            at: at.ok_or_else(|| missing(c, KW, "at"))?,
        };
        return Ok(ViewDef { name, placement, scale, hidden, tangent, centers, section: None });
    }
    if center.is_some() || radius.is_some() {
        return Err(c.err_hint("center and radius belong to detail views", "add detail_of: <view>"));
    }
    let placement = match (of, from) {
        (Some(_), Some(_)) => {
            return Err(c.err_hint("a view is either projected (of, side) or a base view (from, at)", "remove one"));
        }
        (Some(of), None) => {
            if bodies.is_some() || at.is_some() {
                return Err(
                    c.err_hint("a projected view takes its bodies and place from its parent", "remove bodies/at")
                );
            }
            ViewPlacement::Projected { of, side: side.ok_or_else(|| missing(c, KW, "side"))?, gap }
        }
        (None, Some(from)) => {
            if side.is_some() || gap.is_some() {
                return Err(c.err_hint("side and gap belong to projected views", "add of: <view>, or remove them"));
            }
            ViewPlacement::Base {
                bodies: bodies.unwrap_or_default(),
                from,
                at: at.ok_or_else(|| missing(c, KW, "at"))?,
            }
        }
        (None, None) => return Err(missing(c, KW, "from (or of)")),
    };
    let section = match (section, cut_at) {
        (Some(label), cut_at) => {
            if matches!(placement, ViewPlacement::Base { .. }) {
                return Err(c.err_hint("a section is projected from the view it cuts", "add of: <view>, side: <side>"));
            }
            if half && !steps.is_empty() {
                return Err(c.err("a section is either a half section or an offset section"));
            }
            Some(SectionDef { label, cut_at, half, steps })
        }
        (None, Some(_)) => return Err(c.err_hint("cut_at belongs to section views", "add section: <letter>")),
        (None, None) if half || !steps.is_empty() => {
            return Err(c.err_hint("half and steps belong to section views", "add section: <letter>"));
        }
        (None, None) => None,
    };
    Ok(ViewDef { name, placement, scale, hidden, tangent, centers, section })
}

fn parse_dim(c: &mut Cursor<'_>) -> Result<DimDef, ParseError> {
    const KW: &str = "dim";
    let name = c.ident()?.to_string();
    let (mut view, mut kind, mut a, mut b) = (None, None, None, None);
    let (mut direction, mut offset, mut text_at, mut tolerance, mut prefix) =
        (DimDirection::Aligned, None, None, None, None);
    parse_args(c, |key, c| {
        match key {
            "view" => view = Some(c.ident()?.to_string()),
            "kind" => {
                kind = Some(keyword(
                    c,
                    "dimension kind",
                    "length, distance, diameter, radius or angle",
                    DimKind::from_keyword,
                )?);
            }
            "a" => a = Some(parse_edge(c)?),
            "b" => b = Some(parse_edge(c)?),
            "direction" => {
                direction = keyword(c, "direction", "aligned, horizontal or vertical", DimDirection::from_keyword)?;
            }
            "offset" => offset = Some(c.expr()?),
            "text_at" => text_at = Some(parse_pair(c)?),
            "tolerance" => tolerance = Some(c.string()?),
            "prefix" => prefix = Some(c.string()?),
            key => {
                return Err(unknown_arg(c, KW, key, "view, kind, a, b, direction, offset, text_at, tolerance, prefix"));
            }
        }
        Ok(())
    })?;
    let kind = kind.ok_or_else(|| missing(c, KW, "kind"))?;
    if kind.takes_two() != b.is_some() {
        let want = if kind.takes_two() { "two references (a and b)" } else { "one reference (a)" };
        return Err(c.err(format!("a {} dimension takes {want}", kind.keyword())));
    }
    if direction != DimDirection::Aligned && !matches!(kind, DimKind::Length | DimKind::Distance) {
        return Err(c.err("direction applies to length and distance dimensions"));
    }
    Ok(DimDef {
        name,
        view: view.ok_or_else(|| missing(c, KW, "view"))?,
        kind,
        a: a.ok_or_else(|| missing(c, KW, "a"))?,
        b,
        direction,
        offset,
        text_at,
        tolerance,
        prefix,
    })
}

fn parse_parts_list(c: &mut Cursor<'_>) -> Result<PartsListDef, ParseError> {
    const KW: &str = "parts_list";
    let name = c.ident()?.to_string();
    let mut at = None;
    parse_args(c, |key, c| {
        match key {
            "at" => at = Some(parse_pair(c)?),
            key => return Err(unknown_arg(c, KW, key, "at")),
        }
        Ok(())
    })?;
    Ok(PartsListDef { name, at: at.ok_or_else(|| missing(c, KW, "at"))? })
}

fn parse_gdt(c: &mut Cursor<'_>) -> Result<GdtDef, ParseError> {
    const KW: &str = "gdt";
    let name = c.ident()?.to_string();
    let (mut view, mut a, mut symbol, mut tolerance, mut datums, mut at) = (None, None, None, None, Vec::new(), None);
    parse_args(c, |key, c| {
        match key {
            "view" => view = Some(c.ident()?.to_string()),
            "a" => a = Some(parse_edge(c)?),
            "symbol" => {
                let w = c.ident()?;
                symbol = Some(GdtSymbol::from_keyword(w).ok_or_else(|| {
                    c.err_hint(
                        format!("unknown GD&T symbol '{w}'"),
                        "flatness, position, perpendicularity, ... (see the grammar)",
                    )
                })?);
            }
            "tolerance" => tolerance = Some(c.string()?),
            "datums" => datums = parse_ident_list(c, "datums")?,
            "at" => at = Some(parse_pair(c)?),
            key => return Err(unknown_arg(c, KW, key, "view, a, symbol, tolerance, datums, at")),
        }
        Ok(())
    })?;
    Ok(GdtDef {
        name,
        view: view.ok_or_else(|| missing(c, KW, "view"))?,
        a: a.ok_or_else(|| missing(c, KW, "a"))?,
        symbol: symbol.ok_or_else(|| missing(c, KW, "symbol"))?,
        tolerance: tolerance.ok_or_else(|| missing(c, KW, "tolerance"))?,
        datums,
        at: at.ok_or_else(|| missing(c, KW, "at"))?,
    })
}

fn parse_datum(c: &mut Cursor<'_>) -> Result<DatumDef, ParseError> {
    const KW: &str = "datum";
    let name = c.ident()?.to_string();
    let (mut view, mut a, mut at) = (None, None, None);
    parse_args(c, |key, c| {
        match key {
            "view" => view = Some(c.ident()?.to_string()),
            "a" => a = Some(parse_edge(c)?),
            "at" => at = Some(parse_pair(c)?),
            key => return Err(unknown_arg(c, KW, key, "view, a, at")),
        }
        Ok(())
    })?;
    Ok(DatumDef {
        name,
        view: view.ok_or_else(|| missing(c, KW, "view"))?,
        a: a.ok_or_else(|| missing(c, KW, "a"))?,
        at: at.ok_or_else(|| missing(c, KW, "at"))?,
    })
}

fn parse_balloon(c: &mut Cursor<'_>) -> Result<BalloonDef, ParseError> {
    const KW: &str = "balloon";
    let name = c.ident()?.to_string();
    let (mut view, mut body, mut at) = (None, None, None);
    parse_args(c, |key, c| {
        match key {
            "view" => view = Some(c.ident()?.to_string()),
            "body" => body = Some(c.ident()?.to_string()),
            "at" => at = Some(parse_pair(c)?),
            key => return Err(unknown_arg(c, KW, key, "view, body, at")),
        }
        Ok(())
    })?;
    Ok(BalloonDef {
        name,
        view: view.ok_or_else(|| missing(c, KW, "view"))?,
        body: body.ok_or_else(|| missing(c, KW, "body"))?,
        at: at.ok_or_else(|| missing(c, KW, "at"))?,
    })
}

fn parse_note(c: &mut Cursor<'_>) -> Result<NoteDef, ParseError> {
    const KW: &str = "note";
    let name = c.ident()?.to_string();
    let (mut text, mut at) = (None, None);
    parse_args(c, |key, c| {
        match key {
            "text" => text = Some(c.string()?),
            "at" => at = Some(parse_pair(c)?),
            key => return Err(unknown_arg(c, KW, key, "text, at")),
        }
        Ok(())
    })?;
    Ok(NoteDef { name, text: text.ok_or_else(|| missing(c, KW, "text"))?, at: at.ok_or_else(|| missing(c, KW, "at"))? })
}

/// Canonical text of one drawing (a history object).
pub fn print_drawing_object(d: &Drawing) -> String {
    let mut s = String::new();
    print_drawing(&mut s, d);
    s
}

/// Parses a drawing history object.
///
/// # Errors
/// Text that is not exactly one drawing.
pub fn parse_drawing_object(s: &str) -> Result<Drawing, ParseError> {
    let lines: Vec<&str> = s.lines().collect();
    let first = lines.first().copied().unwrap_or("");
    let mut c = Cursor::new(first, 1);
    if c.ident()? != "drawing" {
        return Err(c.err("not a drawing"));
    }
    let (d, next) = parse_drawing(&mut c, &lines, 1)?;
    if lines.iter().skip(next).any(|l| !l.trim().is_empty()) {
        return Err(perr(next + 1, "unexpected text after the drawing"));
    }
    Ok(d)
}
