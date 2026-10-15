//! Text outlines for the `text` feature (ADR 0029): a string in one of the fonts the app
//! ships becomes closed regions of lines and cubic Béziers, in millimetres, on a baseline
//! along +X at y = 0. Outer loops run counter-clockwise, holes clockwise.
//!
//! Outlines are read with `skrifa` unhinted, in font units, so the result is the same on
//! every machine. Quadratic segments (TrueType) are raised to cubic ones exactly. There is
//! no kerning and no line breaking.

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

/// A point in mm.
pub type P2 = [f64; 2];

/// Most characters in one text.
pub const MAX_CHARS: usize = 200;
/// Most outline segments in one text (all glyphs).
pub const MAX_SEGMENTS: usize = 50_000;

/// One outline segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Line {
        a: P2,
        b: P2,
    },
    /// Cubic Bézier from `p[0]` to `p[3]`.
    Cubic {
        p: [P2; 4],
    },
}

impl Seg {
    fn start(&self) -> P2 {
        match self {
            Seg::Line { a, .. } => *a,
            Seg::Cubic { p } => p[0],
        }
    }

    fn reversed(&self) -> Seg {
        match *self {
            Seg::Line { a, b } => Seg::Line { a: b, b: a },
            Seg::Cubic { p: [a, b, c, d] } => Seg::Cubic { p: [d, c, b, a] },
        }
    }

    /// Points along the segment, without its end (for areas and containment).
    fn sample(&self, out: &mut Vec<P2>) {
        match *self {
            Seg::Line { a, .. } => out.push(a),
            Seg::Cubic { p: [a, b, c, d] } => {
                for i in 0..16 {
                    let t = f64::from(i) / 16.0;
                    let u = 1.0 - t;
                    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                    out.push([
                        w[0] * a[0] + w[1] * b[0] + w[2] * c[0] + w[3] * d[0],
                        w[0] * a[1] + w[1] * b[1] + w[2] * c[1] + w[3] * d[1],
                    ]);
                }
            }
        }
    }
}

/// One closed outline of a glyph: contour `contour` (from 0, in the font's order) of the
/// character at `char_index` (from 0 in the string, spaces included).
#[derive(Debug, Clone, PartialEq)]
pub struct Contour {
    pub char_index: u32,
    pub contour: u32,
    pub segs: Vec<Seg>,
}

impl Contour {
    fn polygon(&self) -> Vec<P2> {
        let mut out = Vec::new();
        for s in &self.segs {
            s.sample(&mut out);
        }
        out
    }

    fn reverse(&mut self) {
        self.segs.reverse();
        for s in &mut self.segs {
            *s = s.reversed();
        }
    }
}

/// A filled area: an outer contour (counter-clockwise) and its holes (clockwise).
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub outer: Contour,
    pub holes: Vec<Contour>,
}

/// The bundled fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Font {
    /// Ubuntu Light (Ubuntu Font Licence).
    #[default]
    Sans,
    /// Hack (MIT / Bitstream Vera).
    Mono,
}

impl Font {
    pub fn keyword(self) -> &'static str {
        match self {
            Font::Sans => "sans",
            Font::Mono => "mono",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Font> {
        match s {
            "sans" => Some(Font::Sans),
            "mono" => Some(Font::Mono),
            _ => None,
        }
    }

    fn data(self) -> &'static [u8] {
        match self {
            Font::Sans => epaint_default_fonts::UBUNTU_LIGHT,
            Font::Mono => epaint_default_fonts::HACK_REGULAR,
        }
    }
}

/// Where `x = 0` is along the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    pub fn keyword(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Align> {
        match s {
            "left" => Some(Align::Left),
            "center" => Some(Align::Center),
            "right" => Some(Align::Right),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextError {
    Empty,
    TooLong {
        chars: usize,
    },
    /// A control character (newline, tab, ...).
    Control(char),
    /// Characters the font has no glyph for.
    Missing(Vec<char>),
    TooComplex {
        segments: usize,
    },
    BadHeight,
    Font(String),
}

impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextError::Empty => write!(f, "the text is empty"),
            TextError::TooLong { chars } => write!(f, "the text has {chars} characters (at most {MAX_CHARS})"),
            TextError::Control(c) => write!(f, "the text contains the control character U+{:04X}", u32::from(*c)),
            TextError::Missing(cs) => {
                let list: Vec<String> = cs.iter().map(|c| format!("'{c}'")).collect();
                write!(f, "the font has no glyph for {}", list.join(", "))
            }
            TextError::TooComplex { segments } => {
                write!(f, "the text has {segments} outline segments (at most {MAX_SEGMENTS})")
            }
            TextError::BadHeight => write!(f, "the text height must be positive"),
            TextError::Font(m) => write!(f, "the font cannot be read: {m}"),
        }
    }
}

impl std::error::Error for TextError {}

/// Collects one glyph's contours in font units, offset by `dx`.
struct Pen {
    dx: f64,
    scale: f64,
    contours: Vec<Vec<Seg>>,
    cur: Vec<Seg>,
    start: P2,
    at: P2,
}

// Points are compared exactly on purpose: they come from the same font units through the
// same scaling, so a closing point equals the start bit for bit.
#[allow(clippy::float_cmp)]
impl Pen {
    fn pt(&self, x: f32, y: f32) -> P2 {
        [(f64::from(x) + self.dx) * self.scale, f64::from(y) * self.scale]
    }

    fn finish(&mut self) {
        if self.at != self.start {
            self.cur.push(Seg::Line { a: self.at, b: self.start });
        }
        let segs = std::mem::take(&mut self.cur);
        if segs.len() >= 2 {
            self.contours.push(segs);
        }
        self.at = self.start;
    }
}

#[allow(clippy::float_cmp)]
impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        if !self.cur.is_empty() {
            self.finish();
        }
        self.start = self.pt(x, y);
        self.at = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let b = self.pt(x, y);
        if b != self.at {
            self.cur.push(Seg::Line { a: self.at, b });
            self.at = b;
        }
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        // Degree elevation: the cubic with these controls is the same curve.
        let (p0, q, p3) = (self.at, self.pt(cx0, cy0), self.pt(x, y));
        let c1 = [p0[0] + 2.0 / 3.0 * (q[0] - p0[0]), p0[1] + 2.0 / 3.0 * (q[1] - p0[1])];
        let c2 = [p3[0] + 2.0 / 3.0 * (q[0] - p3[0]), p3[1] + 2.0 / 3.0 * (q[1] - p3[1])];
        if p3 != p0 {
            self.cur.push(Seg::Cubic { p: [p0, c1, c2, p3] });
            self.at = p3;
        }
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let p3 = self.pt(x, y);
        if p3 != self.at {
            self.cur.push(Seg::Cubic { p: [self.at, self.pt(cx0, cy0), self.pt(cx1, cy1), p3] });
            self.at = p3;
        }
    }

    fn close(&mut self) {
        self.finish();
    }
}

fn signed_area(poly: &[P2]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| match (poly.get(i), poly.get((i + 1) % n)) {
            (Some(a), Some(b)) => a[0] * b[1] - b[0] * a[1],
            _ => 0.0,
        })
        .sum::<f64>()
        / 2.0
}

fn contains(poly: &[P2], p: P2) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            inside = !inside;
        }
    }
    inside
}

/// Groups one glyph's contours into regions: a contour inside an even number of others is
/// an outer loop, the others are holes of the smallest outer loop around them.
fn regions_of(contours: Vec<Contour>) -> Vec<Region> {
    let polys: Vec<Vec<P2>> = contours.iter().map(Contour::polygon).collect();
    let areas: Vec<f64> = polys.iter().map(|p| signed_area(p)).collect();
    // A point inside each contour's own band: a sample point nudged off the boundary would
    // be ambiguous, but a vertex of a contour never lies on another (outlines do not touch).
    let probe = |i: usize| polys.get(i).and_then(|p| p.first()).copied().unwrap_or([0.0, 0.0]);
    let around = |i: usize| -> Vec<usize> {
        (0..contours.len()).filter(|&j| j != i && polys.get(j).is_some_and(|p| contains(p, probe(i)))).collect()
    };
    let depth: Vec<usize> = (0..contours.len()).map(|i| around(i).len()).collect();
    let mut regions: Vec<(usize, Region)> = Vec::new();
    for (i, c) in contours.iter().enumerate() {
        if depth.get(i).is_some_and(|d| d % 2 == 0) {
            let mut outer = c.clone();
            if areas.get(i).is_some_and(|a| *a < 0.0) {
                outer.reverse();
            }
            regions.push((i, Region { outer, holes: Vec::new() }));
        }
    }
    for (i, c) in contours.iter().enumerate() {
        let d = depth.get(i).copied().unwrap_or(0);
        if d % 2 == 0 {
            continue;
        }
        let parent = around(i).into_iter().filter(|j| depth.get(*j).copied() == Some(d - 1)).min_by(|a, b| {
            let size = |k: &usize| areas.get(*k).map_or(f64::INFINITY, |x| x.abs());
            size(a).total_cmp(&size(b))
        });
        let Some(slot) = parent.and_then(|p| regions.iter_mut().find(|(k, _)| *k == p)) else { continue };
        let mut hole = c.clone();
        if areas.get(i).is_some_and(|a| *a > 0.0) {
            hole.reverse();
        }
        slot.1.holes.push(hole);
    }
    regions.into_iter().map(|(_, r)| r).collect()
}

/// The outline of `text` in `font`, capital letters `height` mm tall, on the baseline
/// y = 0 starting at x = 0 (or centred on / ending at x = 0 for `align`).
///
/// # Errors
/// An empty or too long text, control characters, characters the font lacks, too many
/// segments, a height that is not positive.
pub fn outline(text: &str, font: Font, height: f64, align: Align) -> Result<Vec<Region>, TextError> {
    if !(height.is_finite() && height > 0.0) {
        return Err(TextError::BadHeight);
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Err(TextError::Empty);
    }
    if chars.len() > MAX_CHARS {
        return Err(TextError::TooLong { chars: chars.len() });
    }
    if let Some(c) = chars.iter().find(|c| c.is_control()) {
        return Err(TextError::Control(*c));
    }
    let f = FontRef::new(font.data()).map_err(|e| TextError::Font(e.to_string()))?;
    let metrics = f.metrics(Size::unscaled(), LocationRef::default());
    let em = f64::from(metrics.units_per_em.max(1));
    let cap = metrics.cap_height.map_or(0.7 * em, f64::from);
    let scale = height / if cap > 0.0 { cap } else { 0.7 * em };
    let charmap = f.charmap();
    let advances = f.glyph_metrics(Size::unscaled(), LocationRef::default());
    let glyphs = f.outline_glyphs();
    let mut missing: Vec<char> = Vec::new();
    let mut placed = Vec::with_capacity(chars.len());
    let mut x = 0.0_f64;
    for &c in &chars {
        match charmap.map(c).filter(|g| g.to_u32() != 0 || c == ' ') {
            Some(g) => {
                placed.push((g, x));
                x += f64::from(advances.advance_width(g).unwrap_or(0.0));
            }
            None => {
                if !missing.contains(&c) {
                    missing.push(c);
                }
            }
        }
    }
    if !missing.is_empty() {
        return Err(TextError::Missing(missing));
    }
    let shift = match align {
        Align::Left => 0.0,
        Align::Center => -x / 2.0,
        Align::Right => -x,
    };
    let mut out = Vec::new();
    let mut segments = 0usize;
    for (i, (g, gx)) in placed.into_iter().enumerate() {
        let Some(glyph) = glyphs.get(g) else { continue };
        let mut pen =
            Pen { dx: gx + shift, scale, contours: Vec::new(), cur: Vec::new(), start: [0.0; 2], at: [0.0; 2] };
        glyph
            .draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut pen)
            .map_err(|e| TextError::Font(e.to_string()))?;
        if !pen.cur.is_empty() {
            pen.finish();
        }
        let ci = u32::try_from(i).unwrap_or(u32::MAX);
        let contours: Vec<Contour> = pen
            .contours
            .into_iter()
            .enumerate()
            .map(|(k, segs)| Contour { char_index: ci, contour: u32::try_from(k).unwrap_or(u32::MAX), segs })
            .collect();
        segments += contours.iter().map(|c| c.segs.len()).sum::<usize>();
        if segments > MAX_SEGMENTS {
            return Err(TextError::TooComplex { segments });
        }
        out.extend(regions_of(contours));
    }
    Ok(out)
}

/// The outline's bounding box `[min_x, min_y, max_x, max_y]` (from the segments' points,
/// controls included).
#[must_use]
pub fn bbox(regions: &[Region]) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for c in regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)) {
        for s in &c.segs {
            let pts: &[P2] = match s {
                Seg::Line { a, b } => &[*a, *b],
                Seg::Cubic { p } => p,
            };
            for q in pts {
                b = [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[0]), b[3].max(q[1])];
            }
        }
    }
    b
}

/// Each segment's start equals the previous one's end, and the last ends at the first's
/// start (the shape the kernel needs).
#[must_use]
#[allow(clippy::float_cmp)]
pub fn is_closed(c: &Contour) -> bool {
    let n = c.segs.len();
    (0..n).all(|i| {
        let end = match c.segs.get(i) {
            Some(Seg::Line { b, .. }) => *b,
            Some(Seg::Cubic { p }) => p[3],
            None => return false,
        };
        c.segs.get((i + 1) % n).is_some_and(|next| next.start() == end)
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn area(c: &Contour) -> f64 {
        signed_area(&c.polygon())
    }

    #[test]
    fn letters_have_their_holes_and_orientation() {
        // O: one region with one hole; i: two regions (stem and dot); B: one region, two holes.
        for (s, regions, holes) in [("O", 1, 1), ("i", 2, 0), ("B", 1, 2), ("8", 1, 2)] {
            let r = outline(s, Font::Sans, 10.0, Align::Left).unwrap();
            assert_eq!(r.len(), regions, "{s}");
            assert_eq!(r.iter().map(|x| x.holes.len()).sum::<usize>(), holes, "{s}");
            for x in &r {
                assert!(area(&x.outer) > 0.0, "{s}: outer must be counter-clockwise");
                assert!(is_closed(&x.outer), "{s}");
                for h in &x.holes {
                    assert!(area(h) < 0.0, "{s}: holes must be clockwise");
                    assert!(is_closed(h), "{s}");
                }
            }
        }
    }

    #[test]
    fn height_is_the_cap_height_and_alignment_shifts() {
        let b = bbox(&outline("H", Font::Sans, 8.0, Align::Left).unwrap());
        assert!((b[3] - b[1] - 8.0).abs() < 0.08, "{b:?}");
        assert!(b[1].abs() < 0.05, "H sits on the baseline: {b:?}");
        let l = bbox(&outline("ABC", Font::Mono, 5.0, Align::Left).unwrap());
        let c = bbox(&outline("ABC", Font::Mono, 5.0, Align::Center).unwrap());
        let r = bbox(&outline("ABC", Font::Mono, 5.0, Align::Right).unwrap());
        let (dc, dr) = (l[0] - c[0], l[0] - r[0]);
        assert!(dc > 0.0 && (dr - 2.0 * dc).abs() < 1e-9, "{dc} {dr}");
        // Centred text straddles x = 0 (advance-based, so only roughly by its ink).
        assert!((c[0] + c[2]).abs() < 1.0, "{c:?}");
        assert!(r[2] < 0.5 && r[2] > -1.0, "{r:?}");
        // Same text, same result: nothing depends on the machine.
        assert_eq!(
            outline("Gusset 42", Font::Sans, 3.0, Align::Center),
            outline("Gusset 42", Font::Sans, 3.0, Align::Center)
        );
    }

    #[test]
    fn spaces_advance_and_indices_count_them() {
        let r = outline("A B", Font::Sans, 4.0, Align::Left).unwrap();
        let idx: Vec<u32> = r.iter().map(|x| x.outer.char_index).collect();
        assert!(idx.contains(&0) && idx.contains(&2) && !idx.contains(&1), "{idx:?}");
        let w = |s: &str| bbox(&outline(s, Font::Sans, 4.0, Align::Left).unwrap())[2];
        assert!(w("A  B") > w("A B"));
    }

    #[test]
    fn bad_text_is_refused() {
        assert_eq!(outline("", Font::Sans, 1.0, Align::Left), Err(TextError::Empty));
        assert_eq!(outline("a\nb", Font::Sans, 1.0, Align::Left), Err(TextError::Control('\n')));
        assert_eq!(outline("x", Font::Sans, 0.0, Align::Left), Err(TextError::BadHeight));
        assert_eq!(outline("x", Font::Sans, f64::NAN, Align::Left), Err(TextError::BadHeight));
        assert_eq!(outline("a中中", Font::Sans, 1.0, Align::Left), Err(TextError::Missing(vec!['中'])));
        let long = "x".repeat(MAX_CHARS + 1);
        assert!(matches!(outline(&long, Font::Sans, 1.0, Align::Left), Err(TextError::TooLong { .. })));
        assert_eq!(Font::from_keyword("mono"), Some(Font::Mono));
        assert_eq!(Align::from_keyword(Align::Right.keyword()), Some(Align::Right));
    }
}
