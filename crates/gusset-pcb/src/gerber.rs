//! Gerber X2 output, one file per layer (copper, solder mask, paste, silkscreen, outline),
//! built with the `gerber-types` crate (MIT OR Apache-2.0).
//!
//! Gerber's Y axis points up: board Y (down) is negated. Coordinates are millimetres with
//! six decimals (`%FSLAX46Y46*%`). Text is not plotted yet (it needs the stroke font).

use crate::board::{Board, Graphic, Pad, PadKind, PadShape, Pt, Shape};
use crate::geom::{self, Contour, Piece};
use gerber_types::{
    Aperture, ApertureAttribute, ApertureDefinition, ApertureFunction, AttributeDeletionCriterion, Circle, Command,
    CommentContent, CoordinateFormat, CoordinateMode, CoordinateNumber, CoordinateOffset, Coordinates, DCode,
    ExtendedCode, ExtendedPosition, FileAttribute, FileFunction, FilePolarity, FunctionCode, GCode, GenerationSoftware,
    GerberCode, InterpolationMode, MCode, Operation, Polarity, Position, Profile, QuadrantMode, Rectangular,
    SmdPadType, Unit, ZeroOmission,
};
use std::fmt;

/// Why a layer could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A coordinate past ±9999 mm (the file format's range).
    OutOfRange,
    Write(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::OutOfRange => write!(f, "a coordinate is outside the Gerber range (±9999 mm)"),
            Error::Write(e) => write!(f, "Gerber output: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// One written layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerFile {
    /// The board layer (`F.Cu`).
    pub layer: String,
    /// A file-name suffix (`F_Cu`), as KiCad names its plots.
    pub suffix: String,
    pub text: String,
}

/// The layers a fabrication set needs: every copper layer, both masks, pastes and
/// silkscreens, and the outline.
pub fn fab_layers(board: &Board) -> Vec<String> {
    let mut v = board.copper_layers();
    for l in ["F.Mask", "B.Mask", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "Edge.Cuts"] {
        v.push(l.to_string());
    }
    v
}

/// Writes every layer of [`fab_layers`].
///
/// # Errors
/// [`Error`] from any layer.
pub fn write_all(board: &Board) -> Result<Vec<LayerFile>, Error> {
    fab_layers(board)
        .into_iter()
        .map(|l| Ok(LayerFile { suffix: l.replace('.', "_"), text: write_layer(board, &l)?, layer: l }))
        .collect()
}

/// The X2 file function of a layer.
fn function(board: &Board, layer: &str) -> FileFunction {
    let cu = board.copper_layers();
    if let Some(i) = cu.iter().position(|l| l == layer) {
        let pos = if i == 0 {
            ExtendedPosition::Top
        } else if i + 1 == cu.len() {
            ExtendedPosition::Bottom
        } else {
            ExtendedPosition::Inner
        };
        return FileFunction::Copper { layer: i as i32 + 1, pos, copper_type: None };
    }
    let side = if layer.starts_with("B.") { Position::Bottom } else { Position::Top };
    match layer.split_once('.').map_or("", |x| x.1) {
        "Mask" => FileFunction::SolderMask { pos: side, index: None },
        "Paste" => FileFunction::Paste(side),
        "SilkS" => FileFunction::Legend { pos: side, index: None },
        "Cuts" => FileFunction::Profile(Some(Profile::NonPlated)),
        _ => FileFunction::Other(clean(layer)),
    }
}

/// Text safe inside a Gerber attribute: printable ASCII without the format's delimiters.
fn clean(t: &str) -> String {
    t.chars()
        .map(|c| if c.is_ascii_graphic() && !matches!(c, '%' | '*' | ',' | '\\') { c } else { '_' })
        .take(64)
        .collect()
}

/// An aperture, keyed by its shape in nanometres and its function.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Circle(i64),
    Rect(i64, i64),
    Obround(i64, i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Func {
    None,
    SmdPad,
    ComponentPad,
    ViaPad,
    Conductor,
    NonConductor,
    Profile,
}

impl Func {
    /// The X2 `.AperFunction` value.
    fn attribute(self) -> Option<ApertureFunction> {
        match self {
            Func::None => None,
            Func::SmdPad => Some(ApertureFunction::SmdPad(SmdPadType::CopperDefined)),
            Func::ComponentPad => Some(ApertureFunction::ComponentPad),
            Func::ViaPad => Some(ApertureFunction::ViaPad),
            Func::Conductor => Some(ApertureFunction::Conductor),
            Func::NonConductor => Some(ApertureFunction::NonConductor),
            Func::Profile => Some(ApertureFunction::Profile),
        }
    }
}

fn nm(v: f64) -> i64 {
    (v * 1e6).round() as i64
}

struct Writer {
    fmt: CoordinateFormat,
    apertures: Vec<(Key, Func, i32)>,
    body: Vec<Command>,
    current: Option<i32>,
    mode: Option<InterpolationMode>,
}

impl Writer {
    fn new() -> Self {
        Writer {
            fmt: CoordinateFormat::new(ZeroOmission::Leading, CoordinateMode::Absolute, 4, 6),
            apertures: Vec::new(),
            body: Vec::new(),
            current: None,
            mode: None,
        }
    }

    fn xy(&self, p: Pt) -> Result<Coordinates, Error> {
        // Board Y down → Gerber Y up.
        let (x, y) = (p[0], -p[1]);
        if !(x.abs() < 9999.0 && y.abs() < 9999.0) {
            return Err(Error::OutOfRange);
        }
        Ok(Coordinates::new(CoordinateNumber::new(nm(x)), CoordinateNumber::new(nm(y)), self.fmt))
    }

    fn select(&mut self, key: Key, func: Func) {
        let code = match self.apertures.iter().find(|a| a.0 == key && a.1 == func) {
            Some(a) => a.2,
            None => {
                let code = 10 + self.apertures.len() as i32;
                self.apertures.push((key, func, code));
                code
            }
        };
        if self.current != Some(code) {
            self.body.push(DCode::SelectAperture(code).into());
            self.current = Some(code);
        }
    }

    fn mode(&mut self, m: InterpolationMode) {
        if self.mode != Some(m) {
            self.body.push(GCode::InterpolationMode(m).into());
            self.mode = Some(m);
        }
    }

    fn move_to(&mut self, p: Pt) -> Result<(), Error> {
        let c = self.xy(p)?;
        self.body.push(DCode::Operation(Operation::Move(Some(c))).into());
        Ok(())
    }

    fn line_to(&mut self, p: Pt) -> Result<(), Error> {
        self.mode(InterpolationMode::Linear);
        let c = self.xy(p)?;
        self.body.push(DCode::Operation(Operation::Interpolate(Some(c), None)).into());
        Ok(())
    }

    /// An arc from the current point (`start`) through `mid` to `end`, in board coordinates.
    fn arc_to(&mut self, start: Pt, mid: Pt, end: Pt) -> Result<(), Error> {
        let f = |p: Pt| [p[0], -p[1]];
        let (s, m, e) = (f(start), f(mid), f(end));
        let Some((c, _)) = geom::circle_through(s, m, e).or_else(|| {
            // A full circle given as start = end: the centre is halfway to the middle point.
            (libm::hypot(s[0] - e[0], s[1] - e[1]) < 1e-9).then(|| ([(s[0] + m[0]) / 2.0, (s[1] + m[1]) / 2.0], 0.0))
        }) else {
            return self.line_to(end);
        };
        let ccw = if libm::hypot(s[0] - e[0], s[1] - e[1]) < 1e-9 {
            // A full circle: the direction does not change the image.
            true
        } else {
            geom::arc_ccw(s, m, e)
        };
        self.mode(if ccw { InterpolationMode::CounterclockwiseCircular } else { InterpolationMode::ClockwiseCircular });
        let to = self.xy(end)?;
        let off = CoordinateOffset::new(
            CoordinateNumber::new(nm(c[0] - s[0])),
            CoordinateNumber::new(nm(c[1] - s[1])),
            self.fmt,
        );
        self.body.push(DCode::Operation(Operation::Interpolate(Some(to), Some(off))).into());
        Ok(())
    }

    fn flash(&mut self, p: Pt) -> Result<(), Error> {
        let c = self.xy(p)?;
        self.body.push(DCode::Operation(Operation::Flash(Some(c))).into());
        Ok(())
    }

    /// A stroked line with a round aperture.
    fn stroke(&mut self, a: Pt, b: Pt, width: f64, func: Func) -> Result<(), Error> {
        self.select(Key::Circle(nm(width.max(0.001))), func);
        self.move_to(a)?;
        self.line_to(b)
    }

    fn stroke_arc(&mut self, s: Pt, m: Pt, e: Pt, width: f64, func: Func) -> Result<(), Error> {
        self.select(Key::Circle(nm(width.max(0.001))), func);
        self.move_to(s)?;
        self.arc_to(s, m, e)
    }

    fn contour_path(&mut self, c: &Contour) -> Result<(), Error> {
        match c {
            Contour::Circle { center, radius } => {
                let s = [center[0] + radius, center[1]];
                let m = [center[0] - radius, center[1]];
                self.move_to(s)?;
                self.arc_to(s, [center[0], center[1] + radius], m)?;
                self.arc_to(m, [center[0], center[1] - radius], s)
            }
            Contour::Pieces(p) => {
                let Some(first) = p.first() else { return Ok(()) };
                self.move_to(first.start())?;
                for x in p {
                    match *x {
                        Piece::Line { b, .. } => self.line_to(b)?,
                        Piece::Arc { start, mid, end } => self.arc_to(start, mid, end)?,
                    }
                }
                Ok(())
            }
        }
    }

    /// A filled region (G36/G37). A region takes the aperture attributes in force when it
    /// is made, so its function is set around it.
    fn region(&mut self, c: &Contour, func: Func) -> Result<(), Error> {
        let attr = func.attribute();
        if let Some(f) = &attr {
            self.body.push(ExtendedCode::ApertureAttribute(ApertureAttribute::ApertureFunction(f.clone())).into());
        }
        self.body.push(GCode::RegionMode(true).into());
        self.contour_path(c)?;
        self.body.push(GCode::RegionMode(false).into());
        if attr.is_some() {
            let d = AttributeDeletionCriterion::SingleApertureAttribute(".AperFunction".into());
            self.body.push(ExtendedCode::DeleteAttribute(d).into());
        }
        Ok(())
    }

    fn polygon_region(&mut self, pts: &[Pt], func: Func) -> Result<(), Error> {
        if pts.len() < 3 {
            return Ok(());
        }
        let n = pts.len();
        let pieces: Vec<Piece> =
            (0..n).filter_map(|i| Some(Piece::Line { a: *pts.get(i)?, b: *pts.get((i + 1) % n)? })).collect();
        self.region(&Contour::Pieces(pieces), func)
    }

    /// A pad's copper (or mask/paste opening), grown by `grow`.
    fn pad(&mut self, pad: &Pad, grow: f64, func: Func) -> Result<(), Error> {
        let [w, h] = [pad.size[0] + 2.0 * grow, pad.size[1] + 2.0 * grow];
        if w <= 0.0 || h <= 0.0 {
            return Ok(());
        }
        let quarter = geom::right_angle(pad.angle);
        match (pad.shape, quarter) {
            (PadShape::Circle, _) | (PadShape::Custom { anchor_circle: true }, _) => {
                self.select(Key::Circle(nm(w)), func);
                self.flash(pad.at)?;
            }
            (PadShape::Rect | PadShape::Custom { anchor_circle: false }, Some(q)) => {
                let (a, b) = if q % 2 == 1 { (h, w) } else { (w, h) };
                self.select(Key::Rect(nm(a), nm(b)), func);
                self.flash(pad.at)?;
            }
            (PadShape::Oval, Some(q)) => {
                let (a, b) = if q % 2 == 1 { (h, w) } else { (w, h) };
                if (a - b).abs() < 1e-9 {
                    self.select(Key::Circle(nm(a)), func);
                } else {
                    self.select(Key::Obround(nm(a), nm(b)), func);
                }
                self.flash(pad.at)?;
            }
            _ => {
                if let Some(c) = geom::pad_contour(pad, grow) {
                    self.region(&c, func)?;
                }
            }
        }
        for p in &pad.primitives {
            match p {
                Shape::Poly { pts } => self.polygon_region(pts, func)?,
                Shape::Circle { center, radius } => {
                    self.select(Key::Circle(nm(2.0 * radius + 2.0 * grow)), func);
                    self.flash(*center)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn graphic(&mut self, g: &Graphic, func: Func) -> Result<(), Error> {
        let w = if g.width > 0.0 { g.width } else { 0.1 };
        match &g.shape {
            Shape::Line { a, b } => self.stroke(*a, *b, w, func),
            Shape::Arc { start, mid, end } => self.stroke_arc(*start, *mid, *end, w, func),
            Shape::Circle { center, radius } => {
                if g.filled {
                    self.select(Key::Circle(nm(2.0 * radius + g.width)), func);
                    self.flash(*center)
                } else {
                    let s = [center[0] + radius, center[1]];
                    let m = [center[0] - radius, center[1]];
                    self.select(Key::Circle(nm(w)), func);
                    self.move_to(s)?;
                    self.arc_to(s, [center[0], center[1] + radius], m)?;
                    self.arc_to(m, [center[0], center[1] - radius], s)
                }
            }
            Shape::Poly { pts } => {
                if g.filled {
                    self.polygon_region(pts, func)?;
                }
                if !g.filled || g.width > 0.0 {
                    self.select(Key::Circle(nm(w)), func);
                    let Some(first) = pts.first() else { return Ok(()) };
                    self.move_to(*first)?;
                    for p in pts.iter().skip(1).chain(std::iter::once(first)) {
                        self.line_to(*p)?;
                    }
                }
                Ok(())
            }
        }
    }

    fn finish(self, function: FileFunction, layer: &str) -> Result<String, Error> {
        let mut cmds: Vec<Command> = vec![
            FunctionCode::GCode(GCode::Comment(CommentContent::String(format!(
                "Gusset {} layer {}",
                env!("CARGO_PKG_VERSION"),
                clean(layer)
            ))))
            .into(),
            ExtendedCode::FileAttribute(FileAttribute::GenerationSoftware(GenerationSoftware::new(
                "Gusset",
                "gusset-pcb",
                Some(env!("CARGO_PKG_VERSION")),
            )))
            .into(),
            ExtendedCode::FileAttribute(FileAttribute::FileFunction(function)).into(),
            ExtendedCode::FileAttribute(FileAttribute::FilePolarity(FilePolarity::Positive)).into(),
            ExtendedCode::CoordinateFormat(self.fmt).into(),
            ExtendedCode::Unit(Unit::Millimeters).into(),
            ExtendedCode::LoadPolarity(Polarity::Dark).into(),
            GCode::QuadrantMode(QuadrantMode::Multi).into(),
        ];
        for (key, func, code) in &self.apertures {
            let f = func.attribute();
            if let Some(f) = &f {
                cmds.push(ExtendedCode::ApertureAttribute(ApertureAttribute::ApertureFunction(f.clone())).into());
            }
            let mm = |v: i64| v as f64 / 1e6;
            let ap = match *key {
                Key::Circle(d) => Aperture::Circle(Circle::new(mm(d))),
                Key::Rect(a, b) => Aperture::Rectangle(Rectangular::new(mm(a), mm(b))),
                Key::Obround(a, b) => Aperture::Obround(Rectangular::new(mm(a), mm(b))),
            };
            cmds.push(ExtendedCode::ApertureDefinition(ApertureDefinition::new(*code, ap)).into());
            if f.is_some() {
                cmds.push(
                    ExtendedCode::DeleteAttribute(AttributeDeletionCriterion::AllApertureAndObjectAttributes).into(),
                );
            }
        }
        cmds.extend(self.body);
        cmds.push(MCode::EndOfFile.into());
        let mut out: Vec<u8> = Vec::new();
        cmds.serialize(&mut out).map_err(|e| Error::Write(e.to_string()))?;
        String::from_utf8(out).map_err(|e| Error::Write(e.to_string()))
    }
}

/// The pad's solder mask opening growth.
fn mask_grow(board: &Board, pad: &Pad) -> f64 {
    pad.mask_margin.unwrap_or(board.mask_margin)
}

/// The pad's paste opening growth (negative shrinks).
fn paste_grow(board: &Board, pad: &Pad) -> f64 {
    let ratio = pad.paste_ratio.unwrap_or(0.0);
    pad.paste_margin.unwrap_or(board.paste_margin) + ratio * pad.size[0].min(pad.size[1])
}

/// Writes one layer as a Gerber X2 file.
///
/// # Errors
/// [`Error::OutOfRange`] for coordinates past the format's range.
pub fn write_layer(board: &Board, layer: &str) -> Result<String, Error> {
    let mut w = Writer::new();
    let copper = board.copper_layers().iter().any(|l| l == layer);
    let kind = layer.split_once('.').map_or("", |x| x.1);
    // Zone fills first (pads and tracks are drawn over them).
    if copper {
        for z in board.zones.iter().filter(|z| !z.keepout) {
            for (l, poly) in &z.fills {
                if l == layer {
                    w.polygon_region(poly, Func::Conductor)?;
                }
            }
        }
    }
    let gfunc = if copper {
        Func::NonConductor
    } else if layer == "Edge.Cuts" {
        Func::Profile
    } else {
        Func::None
    };
    for g in board.graphics.iter().filter(|g| g.layer == layer) {
        w.graphic(g, gfunc)?;
    }
    // Texts in our stroke font.
    for t in board.texts.iter().filter(|t| t.layer == layer && !t.hidden) {
        let fp = t.footprint.and_then(|i| board.footprints.get(i));
        let text = crate::font::shown(&t.text, fp.map_or("", |f| &f.reference), fp.map_or("", |f| &f.value));
        let pen = crate::font::thickness(t);
        for line in crate::font::strokes(t, &text) {
            let Some(first) = line.first() else { continue };
            w.select(Key::Circle(nm(pen.max(0.001))), gfunc);
            w.move_to(*first)?;
            if line.len() == 1 {
                // A dot: one flash of the pen.
                w.flash(*first)?;
            }
            for p in line.iter().skip(1) {
                w.line_to(*p)?;
            }
        }
    }
    if copper {
        for t in board.tracks.iter().filter(|t| t.layer == layer && t.width > 0.0) {
            match t.mid {
                Some(m) => w.stroke_arc(t.start, m, t.end, t.width, Func::Conductor)?,
                None => w.stroke(t.start, t.end, t.width, Func::Conductor)?,
            }
        }
        for v in &board.vias {
            if v.size > 0.0 && board.span(&v.layers[0], &v.layers[1]).iter().any(|l| l == layer) {
                w.select(Key::Circle(nm(v.size)), Func::ViaPad);
                w.flash(v.at)?;
            }
        }
    }
    for pad in &board.pads {
        if !board.on_layer(&pad.layers, layer) {
            continue;
        }
        if copper {
            // A non-plated hole's "pad" is only copper when it is larger than its hole.
            if pad.kind == PadKind::Npth && pad.drill.is_some_and(|d| d.size[0] >= pad.size[0] - 1e-9) {
                continue;
            }
            let f = if pad.kind == PadKind::Smd { Func::SmdPad } else { Func::ComponentPad };
            w.pad(pad, 0.0, f)?;
        } else if kind == "Mask" {
            w.pad(pad, mask_grow(board, pad), Func::None)?;
        } else if kind == "Paste" {
            if pad.kind != PadKind::Npth {
                w.pad(pad, paste_grow(board, pad), Func::None)?;
            }
        } else {
            w.pad(pad, 0.0, Func::None)?;
        }
    }
    w.finish(function(board, layer), layer)
}
