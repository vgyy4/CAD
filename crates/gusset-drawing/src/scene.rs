//! The renderer-neutral drawing scene (ADR 0032 §3): primitives in sheet millimetres
//! (origin at the sheet's lower-left corner, Y up), each with a line style. Every output
//! (the app's painter, SVG, DXF, later PDF) draws this one scene.

/// A line style after ISO 128-20 / 128-24.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Style {
    /// Visible edges and outlines: continuous wide.
    Visible,
    /// Hidden edges: dashed narrow.
    Hidden,
    /// Tangent (smooth) edges: continuous narrow.
    Tangent,
    /// Centre lines and centre marks: long-dash-dot narrow.
    Center,
    /// Dimension, extension and leader lines, arrows: continuous narrow.
    Dimension,
    /// The sheet frame and the title block's outer lines.
    Border,
    /// Title block cells and other thin sheet lines.
    Thin,
    /// Text.
    Text,
    /// A dimension whose model reference does not resolve.
    Dangling,
}

impl Style {
    pub const ALL: [Style; 9] = [
        Style::Visible,
        Style::Hidden,
        Style::Tangent,
        Style::Center,
        Style::Dimension,
        Style::Border,
        Style::Thin,
        Style::Text,
        Style::Dangling,
    ];

    /// Line width in mm (ISO line group 0.5).
    pub fn width(self) -> f64 {
        match self {
            Style::Visible => 0.5,
            Style::Border => 0.7,
            _ => 0.25,
        }
    }

    /// The dash pattern in mm (dash, gap, ...); empty for continuous lines. ISO 128-20
    /// with d the line width: dashes 12d, gaps 3d, long dashes 24d, dots 0.5d.
    pub fn dashes(self) -> &'static [f64] {
        match self {
            Style::Hidden => &[3.0, 0.75],
            Style::Center => &[6.0, 0.75, 0.125, 0.75],
            Style::Dangling => &[1.5, 0.75],
            _ => &[],
        }
    }

    /// The layer name in DXF and the class name in SVG.
    pub fn layer(self) -> &'static str {
        match self {
            Style::Visible => "VISIBLE",
            Style::Hidden => "HIDDEN",
            Style::Tangent => "TANGENT",
            Style::Center => "CENTER",
            Style::Dimension => "DIM",
            Style::Border => "BORDER",
            Style::Thin => "THIN",
            Style::Text => "TEXT",
            Style::Dangling => "DANGLING",
        }
    }
}

/// Horizontal text anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    Line {
        a: [f64; 2],
        b: [f64; 2],
    },
    /// Counterclockwise from `start` to `end` (radians, `end > start`); a full circle
    /// when `end - start >= 2π`.
    Arc {
        center: [f64; 2],
        radius: f64,
        start: f64,
        end: f64,
    },
    Polyline {
        points: Vec<[f64; 2]>,
        closed: bool,
    },
    /// A filled polygon (arrow heads).
    Fill {
        points: Vec<[f64; 2]>,
    },
    /// Text with its baseline starting (or centred, or ending) at `at`; `height` is the
    /// cap height in mm; `angle` in radians.
    Text {
        at: [f64; 2],
        height: f64,
        text: String,
        anchor: Anchor,
        angle: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub style: Style,
    pub prim: Prim,
}

/// A view as placed on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedView {
    pub name: String,
    /// Its box on the sheet (mm): min, max.
    pub min: [f64; 2],
    pub max: [f64; 2],
    pub scale: f64,
    /// Sheet position of the view's origin (model point projected to (0, 0)).
    pub origin: [f64; 2],
    /// The direction towards the viewer and the view's X and Y axes, in model coordinates.
    pub toward: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    /// Why the view is empty, if it is.
    pub error: Option<String>,
}

impl PlacedView {
    /// Sheet position of a model point.
    pub fn to_sheet(&self, p: [f64; 3]) -> [f64; 2] {
        let d = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        [self.origin[0] + self.scale * d(p, self.x), self.origin[1] + self.scale * d(p, self.y)]
    }
}

/// One sheet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    /// Sheet width and height in mm.
    pub size: [f64; 2],
    pub items: Vec<Item>,
    pub views: Vec<PlacedView>,
    /// Problems that left something out (a view that could not be projected, ...).
    pub issues: Vec<String>,
}

impl Scene {
    pub fn push(&mut self, style: Style, prim: Prim) {
        self.items.push(Item { style, prim });
    }

    pub fn line(&mut self, style: Style, a: [f64; 2], b: [f64; 2]) {
        self.push(style, Prim::Line { a, b });
    }

    pub fn rect(&mut self, style: Style, min: [f64; 2], max: [f64; 2]) {
        let points = vec![min, [max[0], min[1]], max, [min[0], max[1]]];
        self.push(style, Prim::Polyline { points, closed: true });
    }

    pub fn text(&mut self, at: [f64; 2], height: f64, text: impl Into<String>, anchor: Anchor) {
        self.push(Style::Text, Prim::Text { at, height, text: text.into(), anchor, angle: 0.0 });
    }
}
