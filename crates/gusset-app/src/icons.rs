// Adapted from SolidCraft (https://github.com/bherbruck/solidcraft), Copyright (c) 2026 SolidCraft contributors, MIT OR Apache-2.0.
// The tool icons (the `Tool` section below) adapt SolidCraft's code-drawn icon grid and
// `Pen` helper; the colors come from Gusset's design tokens. See docs/ATTRIBUTION-THIRD-PARTY.md.

//! Small status icons painted with egui shapes, so they never depend on a font having
//! the glyph (egui's bundled fonts lack ●, ◆, ▸, ▾ and others, which then render as
//! boxes). Crisp at every DPI. A test checks that every non-ASCII character in the UI's
//! string literals has a real glyph in the loaded fonts.

use egui::{Color32, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, Ui, Vec2, WidgetInfo, WidgetType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Filled dot (unsaved, listening, iteration).
    Dot,
    /// Check mark (saved, feature OK).
    Check,
    /// Warning triangle with "!" (errors).
    Warning,
    /// Filled diamond (agents).
    Diamond,
    /// Hollow ring (suppressed / rolled back).
    Ring,
    /// Three dots (pending / blocked).
    Pending,
    /// Disclosure triangle pointing right (collapsed).
    ChevronRight,
    /// Disclosure triangle pointing down (expanded).
    ChevronDown,
    /// Open eye (shown).
    Eye,
    /// Struck-through eye (hidden).
    EyeOff,
}

/// Paints `icon` centered in `rect`.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let stroke = Stroke::new((r * 0.22).max(1.2), color);
    match icon {
        Icon::Dot => {
            painter.circle_filled(c, r * 0.45, color);
        }
        Icon::Check => {
            let pts = vec![c + Vec2::new(-0.55, 0.0) * r, c + Vec2::new(-0.15, 0.4) * r, c + Vec2::new(0.6, -0.45) * r];
            painter.add(Shape::line(pts, stroke));
        }
        Icon::Warning => {
            let top = c + Vec2::new(0.0, -0.75) * r;
            let pts = vec![top, c + Vec2::new(0.8, 0.65) * r, c + Vec2::new(-0.8, 0.65) * r];
            painter.add(Shape::closed_line(pts, Stroke::new(stroke.width * 0.8, color)));
            painter.line_segment([c + Vec2::new(0.0, -0.25) * r, c + Vec2::new(0.0, 0.18) * r], stroke);
            painter.circle_filled(c + Vec2::new(0.0, 0.42) * r, stroke.width * 0.6, color);
        }
        Icon::Diamond => {
            let pts = vec![
                c + Vec2::new(0.0, -0.6) * r,
                c + Vec2::new(0.6, 0.0) * r,
                c + Vec2::new(0.0, 0.6) * r,
                c + Vec2::new(-0.6, 0.0) * r,
            ];
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::Ring => {
            painter.circle_stroke(c, r * 0.45, Stroke::new(stroke.width * 0.8, color));
        }
        Icon::Pending => {
            for dx in [-0.5, 0.0, 0.5] {
                painter.circle_filled(c + Vec2::new(dx * r, 0.2 * r), r * 0.13, color);
            }
        }
        Icon::ChevronRight | Icon::ChevronDown => {
            let pts = if icon == Icon::ChevronRight {
                vec![c + Vec2::new(-0.3, -0.5) * r, c + Vec2::new(0.4, 0.0) * r, c + Vec2::new(-0.3, 0.5) * r]
            } else {
                vec![c + Vec2::new(-0.5, -0.3) * r, c + Vec2::new(0.5, -0.3) * r, c + Vec2::new(0.0, 0.4) * r]
            };
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::Eye | Icon::EyeOff => {
            let n = 12;
            let lid = |sign: f32| -> Vec<Pos2> {
                (0..=n)
                    .map(|i| {
                        let t = i as f32 / n as f32 * std::f32::consts::PI;
                        c + Vec2::new(-t.cos() * 0.85, sign * t.sin() * 0.5) * r
                    })
                    .collect()
            };
            let thin = Stroke::new(stroke.width * 0.75, color);
            painter.add(Shape::line(lid(-1.0), thin));
            painter.add(Shape::line(lid(1.0), thin));
            painter.circle_filled(c, r * 0.24, color);
            if icon == Icon::EyeOff {
                painter.line_segment([c + Vec2::new(-0.75, 0.75) * r, c + Vec2::new(0.75, -0.75) * r], thin);
            }
        }
    }
}

/// Icon size matching body text.
fn icon_size(ui: &Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Body).max(12.0)
}

/// Allocates and paints a decorative icon (no accessibility node of its own).
pub fn icon(ui: &mut Ui, icon: Icon, color: Color32) -> Response {
    let s = icon_size(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(s), Sense::hover());
    paint(ui.painter(), rect, icon, color);
    resp
}

/// An icon followed by a colored label; returns the label's response.
pub fn labeled(ui: &mut Ui, ic: Icon, color: Color32, text: impl Into<RichText>) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::SPACE[2];
        icon(ui, ic, color);
        ui.label(text.into().color(color))
    })
    .inner
}

/// A button with a disclosure triangle and a label (the dialogs' "More" toggle).
pub fn disclosure_button(ui: &mut Ui, open: bool, label: &str) -> Response {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, ui.visuals().text_color());
    let pad = ui.spacing().button_padding;
    let s = icon_size(ui);
    let gap = crate::theme::SPACE[2];
    let size = Vec2::new(pad.x * 2.0 + s + gap + galley.size().x, (pad.y * 2.0 + galley.size().y).max(s));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact(&resp);
        ui.painter().rect(rect, v.corner_radius, v.weak_bg_fill, v.bg_stroke, egui::StrokeKind::Inside);
        let icon_rect =
            Rect::from_center_size(Pos2::new(rect.left() + pad.x + s * 0.5, rect.center().y), Vec2::splat(s));
        paint(ui.painter(), icon_rect, if open { Icon::ChevronDown } else { Icon::ChevronRight }, v.fg_stroke.color);
        let text_pos = Pos2::new(icon_rect.right() + gap, rect.center().y - galley.size().y * 0.5);
        ui.painter().galley(text_pos, galley, v.fg_stroke.color);
    }
    resp
}

/// Width of a [`toggle`] (to align rows that have none).
pub fn toggle_size(ui: &Ui) -> f32 {
    icon_size(ui) + crate::theme::SPACE[2]
}

/// A small icon toggle (e.g. the browser's sketch visibility eye). `label` names it for
/// screen readers and tests; `on` is its checked state.
pub fn toggle(ui: &mut Ui, on: bool, on_icon: Icon, off_icon: Icon, label: &str) -> Response {
    let s = toggle_size(ui);
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::splat(s), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), on, label));
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact(&resp);
        if resp.hovered() || resp.has_focus() {
            ui.painter().rect_filled(rect, v.corner_radius, v.weak_bg_fill);
        }
        let color = if on { v.fg_stroke.color } else { ui.visuals().weak_text_color() };
        paint(ui.painter(), rect.shrink(crate::theme::SPACE[1]), if on { on_icon } else { off_icon }, color);
    }
    if resp.clicked() {
        resp.mark_changed();
    }
    resp
}

// ------------------------------------------------------------------------ tool icons

/// Icons for the ribbon, the title bar and the navigation bar. Each is designed on a 24 x 24
/// grid and scaled into the target rectangle (DESIGN.md §4: stroke `text`, fill `accent` at
/// 20 %, the part the tool acts on in `accent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    Sketch,
    FinishSketch,
    Extrude,
    Revolve,
    Sweep,
    Loft,
    Hole,
    Gear,
    Fillet,
    Chamfer,
    Shell,
    Combine,
    MoveBody,
    PatternLinear,
    PatternCircular,
    Mirror,
    Plane,
    Measure,
    Info,
    Import,
    Export,
    Open,
    Save,
    Undo,
    Redo,
    Line,
    Polyline,
    Rect,
    Circle,
    Arc,
    Constraint,
    Trim,
    SketchPattern,
    MovePoint,
    DeleteItems,
    Orbit,
    Pan,
    Zoom,
    Fit,
    Home,
    Perspective,
    Edges,
    Search,
    Params,
    /// A solid body (the browser's Bodies).
    Body,
    /// The coordinate origin with its axes.
    Origin,
    /// A history branch.
    Branch,
    /// Document settings.
    Settings,
    /// Timeline playback: to the start, one step back, one step forward, to the end.
    ToStart,
    StepBack,
    StepForward,
    ToEnd,
}

impl Tool {
    /// Every icon (tests draw them all).
    pub const ALL: &'static [Tool] = &[
        Tool::Sketch,
        Tool::FinishSketch,
        Tool::Extrude,
        Tool::Revolve,
        Tool::Sweep,
        Tool::Loft,
        Tool::Hole,
        Tool::Gear,
        Tool::Fillet,
        Tool::Chamfer,
        Tool::Shell,
        Tool::Combine,
        Tool::MoveBody,
        Tool::PatternLinear,
        Tool::PatternCircular,
        Tool::Mirror,
        Tool::Plane,
        Tool::Measure,
        Tool::Info,
        Tool::Import,
        Tool::Export,
        Tool::Open,
        Tool::Save,
        Tool::Undo,
        Tool::Redo,
        Tool::Line,
        Tool::Polyline,
        Tool::Rect,
        Tool::Circle,
        Tool::Arc,
        Tool::Constraint,
        Tool::Trim,
        Tool::SketchPattern,
        Tool::MovePoint,
        Tool::DeleteItems,
        Tool::Orbit,
        Tool::Pan,
        Tool::Zoom,
        Tool::Fit,
        Tool::Home,
        Tool::Perspective,
        Tool::Edges,
        Tool::Search,
        Tool::Params,
        Tool::Body,
        Tool::Origin,
        Tool::Branch,
        Tool::Settings,
        Tool::ToStart,
        Tool::StepBack,
        Tool::StepForward,
        Tool::ToEnd,
    ];
}

/// The icon for a registry command, when it has a dedicated one.
pub fn tool_for_command(id: &str) -> Option<Tool> {
    Some(match id {
        "sketch.create" => Tool::Sketch,
        "feature.extrude" => Tool::Extrude,
        "feature.revolve" => Tool::Revolve,
        "feature.sweep" => Tool::Sweep,
        "feature.loft" => Tool::Loft,
        "feature.hole" => Tool::Hole,
        "generate.gear" => Tool::Gear,
        "feature.fillet" => Tool::Fillet,
        "feature.chamfer" => Tool::Chamfer,
        "feature.shell" => Tool::Shell,
        "feature.combine" => Tool::Combine,
        "feature.move_body" => Tool::MoveBody,
        "feature.pattern_linear" => Tool::PatternLinear,
        "feature.pattern_circular" => Tool::PatternCircular,
        "feature.mirror" => Tool::Mirror,
        "feature.plane" => Tool::Plane,
        "query.body" => Tool::Measure,
        "query.model" | "query.sketch" => Tool::Info,
        "import.file" => Tool::Import,
        "export.step" | "export.mesh" => Tool::Export,
        "history.undo" => Tool::Undo,
        "history.redo" => Tool::Redo,
        "sketch.add_line" => Tool::Line,
        "sketch.add_polyline" => Tool::Polyline,
        "sketch.add_rectangle" => Tool::Rect,
        "sketch.add_circle" => Tool::Circle,
        "sketch.add_arc" => Tool::Arc,
        "sketch.add_constraint" => Tool::Constraint,
        "sketch.trim" => Tool::Trim,
        "sketch.pattern" => Tool::SketchPattern,
        "sketch.move_point" => Tool::MovePoint,
        "sketch.delete_items" => Tool::DeleteItems,
        "param.set" | "param.rename" | "param.delete" => Tool::Params,
        _ => return None,
    })
}

/// The icon for a feature kind, by its document keyword (`FeatureKind::keyword`).
pub fn tool_for_keyword(keyword: &str) -> Tool {
    match keyword {
        "sketch" => Tool::Sketch,
        "extrude" => Tool::Extrude,
        "revolve" => Tool::Revolve,
        "sweep" => Tool::Sweep,
        "loft" => Tool::Loft,
        "fillet" => Tool::Fillet,
        "chamfer" => Tool::Chamfer,
        "import" => Tool::Import,
        "shell" => Tool::Shell,
        "mirror" => Tool::Mirror,
        "pattern_linear" => Tool::PatternLinear,
        "pattern_circular" => Tool::PatternCircular,
        "plane" => Tool::Plane,
        "move" => Tool::MoveBody,
        "combine" => Tool::Combine,
        "hole" => Tool::Hole,
        _ => Tool::Body,
    }
}

/// Colors an icon is drawn with: all from the theme tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ink {
    /// Outlines.
    pub stroke: Color32,
    /// Translucent body fill.
    pub fill: Color32,
    /// What the tool acts on.
    pub accent: Color32,
    /// Cut-outs (the hole in a body): the surface behind the icon.
    pub hole: Color32,
}

impl Ink {
    /// The normal look.
    pub fn new(pal: &crate::theme::Palette) -> Ink {
        Ink { stroke: pal.text, fill: pal.accent.gamma_multiply(0.2), accent: pal.accent, hole: pal.bg_panel }
    }

    /// A disabled tool: no accent.
    pub fn disabled(pal: &crate::theme::Palette) -> Ink {
        Ink {
            stroke: pal.text_muted,
            fill: pal.text_muted.gamma_multiply(0.15),
            accent: pal.text_muted,
            hole: pal.bg_panel,
        }
    }

    /// Everything in one color (e.g. an icon on an accent-filled button).
    pub fn mono(c: Color32) -> Ink {
        Ink { stroke: c, fill: c.gamma_multiply(0.2), accent: c, hole: Color32::TRANSPARENT }
    }
}

/// Paint helper on the 24 x 24 grid.
struct Pen<'a> {
    p: &'a egui::Painter,
    r: Rect,
    ink: Color32,
    fill: Color32,
    accent: Color32,
    hole: Color32,
    w: f32,
}

const CLEAR: Color32 = Color32::TRANSPARENT;

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.r.left() + x / 24.0 * self.r.width(), self.r.top() + y / 24.0 * self.r.height())
    }

    fn s(&self) -> f32 {
        self.r.width() / 24.0
    }

    fn pts(&self, pts: &[(f32, f32)]) -> Vec<Pos2> {
        pts.iter().map(|&(x, y)| self.at(x, y)).collect()
    }

    fn line(&self, pts: &[(f32, f32)], c: Color32) {
        self.p.add(Shape::line(self.pts(pts), Stroke::new(self.w, c)));
    }

    /// A filled convex polygon with an outline.
    fn poly(&self, pts: &[(f32, f32)], fill: Color32, c: Color32) {
        self.p.add(Shape::convex_polygon(self.pts(pts), fill, Stroke::new(self.w, c)));
    }

    fn closed(&self, pts: &[(f32, f32)], c: Color32) {
        self.p.add(Shape::closed_line(self.pts(pts), Stroke::new(self.w, c)));
    }

    fn circle(&self, x: f32, y: f32, r: f32, fill: Color32, c: Color32) {
        self.p.circle(self.at(x, y), r * self.s(), fill, Stroke::new(self.w, c));
    }

    fn dot(&self, x: f32, y: f32, c: Color32) {
        self.p.circle_filled(self.at(x, y), 1.6 * self.s(), c);
    }

    /// An arc around (x, y), angles in radians counter-clockwise from +x (y points down on
    /// screen, so the sign is flipped).
    fn arc(&self, x: f32, y: f32, r: f32, a0: f32, a1: f32, c: Color32) {
        let n = 24;
        let pts: Vec<(f32, f32)> =
            (0..=n).map(|i| a0 + (a1 - a0) * i as f32 / n as f32).map(|a| (x + r * a.cos(), y - r * a.sin())).collect();
        self.line(&pts, c);
    }

    /// A filled ellipse (a circle seen at an angle: the rim of a hole or a cylinder).
    fn ellipse(&self, x: f32, y: f32, rx: f32, ry: f32, fill: Color32, c: Color32) {
        let pts: Vec<(f32, f32)> = (0..32)
            .map(|i| i as f32 / 32.0 * std::f32::consts::TAU)
            .map(|t| (x + rx * t.cos(), y + ry * t.sin()))
            .collect();
        self.poly(&pts, fill, c);
    }

    fn arrow(&self, from: (f32, f32), to: (f32, f32), c: Color32) {
        self.line(&[from, to], c);
        let d = Vec2::new(to.0 - from.0, to.1 - from.1).normalized();
        let n = Vec2::new(-d.y, d.x);
        let tip = Vec2::new(to.0, to.1);
        let a = tip - d * 3.5 + n * 2.2;
        let b = tip - d * 3.5 - n * 2.2;
        self.poly(&[to, (a.x, a.y), (b.x, b.y)], c, c);
    }

    /// An isometric box with its left corner at (x0, y0).
    fn iso_box(&self, x0: f32, y0: f32, w: f32, d: f32, h: f32) {
        let top = [(x0, y0), (x0 + w, y0 - d * 0.5), (x0 + w + d, y0), (x0 + d, y0 + d * 0.5)];
        let front = [(x0, y0), (x0 + d, y0 + d * 0.5), (x0 + d, y0 + d * 0.5 + h), (x0, y0 + h)];
        let side = [(x0 + d, y0 + d * 0.5), (x0 + w + d, y0), (x0 + w + d, y0 + h), (x0 + d, y0 + d * 0.5 + h)];
        self.poly(&front, self.fill, self.ink);
        self.poly(&side, self.fill.gamma_multiply(0.7), self.ink);
        self.poly(&top, self.fill.gamma_multiply(1.3), self.ink);
    }
}

/// Paints `tool` into `rect`.
#[allow(clippy::too_many_lines)]
pub fn paint_tool(painter: &egui::Painter, rect: Rect, tool: Tool, c: Ink) {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    let side = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), Vec2::splat(side));
    let pen = Pen {
        p: painter,
        r,
        ink: c.stroke,
        fill: c.fill,
        accent: c.accent,
        hole: c.hole,
        w: (side / 24.0 * 1.5).max(1.0),
    };
    let (ink, a) = (pen.ink, pen.accent);
    match tool {
        Tool::Sketch => {
            pen.poly(&[(3.0, 15.0), (13.0, 10.0), (21.0, 14.0), (11.0, 19.0)], pen.fill, ink);
            pen.line(&[(7.0, 13.0), (16.0, 4.0), (18.0, 6.0), (9.0, 15.0), (6.5, 15.5), (7.0, 13.0)], a);
        }
        Tool::FinishSketch => {
            pen.circle(12.0, 12.0, 9.0, CLEAR, a);
            pen.line(&[(7.0, 12.5), (10.5, 16.0), (17.0, 8.5)], a);
        }
        Tool::Extrude => {
            pen.iso_box(4.0, 12.0, 9.0, 7.0, 6.0);
            pen.arrow((12.0, 9.0), (12.0, 2.5), a);
        }
        Tool::Revolve => {
            pen.line(&[(12.0, 2.0), (12.0, 22.0)], ink);
            pen.poly(&[(13.0, 7.0), (19.0, 7.0), (19.0, 17.0), (13.0, 17.0)], pen.fill, ink);
            pen.arc(12.0, 12.0, 9.0, 3.6, 5.8, a);
            pen.arrow((15.0, 20.4), (18.5, 19.0), a);
        }
        Tool::Sweep => {
            pen.arc(20.0, 20.0, 14.0, FRAC_PI_2, PI, ink);
            pen.circle(6.0, 20.0, 3.0, pen.fill, a);
            pen.circle(20.0, 6.0, 3.0, pen.fill, a);
        }
        Tool::Loft => {
            pen.poly(&[(3.0, 20.0), (15.0, 20.0), (21.0, 15.0), (9.0, 15.0)], pen.fill, ink);
            pen.circle(12.0, 6.0, 3.5, CLEAR, a);
            pen.line(&[(3.0, 20.0), (8.5, 6.0)], a);
            pen.line(&[(21.0, 15.0), (15.5, 6.0)], a);
        }
        Tool::Hole => {
            pen.poly(&[(3.0, 8.0), (21.0, 8.0), (21.0, 20.0), (3.0, 20.0)], pen.fill, ink);
            pen.ellipse(12.0, 8.0, 5.0, 2.0, pen.hole, a);
            pen.line(&[(7.0, 8.0), (7.0, 16.0)], a);
            pen.line(&[(17.0, 8.0), (17.0, 16.0)], a);
        }
        Tool::Gear => {
            let (teeth, root, tip) = (8usize, 6.5f32, 9.5f32);
            let pitch = TAU / teeth as f32;
            let polar = |ang: f32, rad: f32| (12.0 + rad * ang.cos(), 12.0 + rad * ang.sin());
            let mut outline = Vec::new();
            for i in 0..teeth {
                let t = i as f32 * pitch;
                outline.extend([
                    polar(t - pitch * 0.30, root),
                    polar(t - pitch * 0.17, tip),
                    polar(t + pitch * 0.17, tip),
                    polar(t + pitch * 0.30, root),
                ]);
            }
            pen.circle(12.0, 12.0, root, pen.fill, CLEAR);
            pen.closed(&outline, ink);
            pen.circle(12.0, 12.0, 2.8, pen.hole, a);
        }
        Tool::Fillet => {
            pen.poly(
                &[(4.0, 20.0), (4.0, 4.0), (10.0, 4.0), (10.0, 14.0), (20.0, 14.0), (20.0, 20.0)],
                pen.fill,
                CLEAR,
            );
            pen.line(&[(4.0, 20.0), (4.0, 4.0), (10.0, 4.0)], ink);
            pen.line(&[(4.0, 20.0), (20.0, 20.0), (20.0, 14.0)], ink);
            pen.arc(20.0, 4.0, 10.0, PI, PI * 1.5, a);
        }
        Tool::Chamfer => {
            pen.poly(&[(4.0, 20.0), (4.0, 4.0), (12.0, 4.0), (20.0, 12.0), (20.0, 20.0)], pen.fill, ink);
            pen.line(&[(12.0, 4.0), (20.0, 12.0)], a);
        }
        Tool::Shell => {
            pen.iso_box(3.0, 10.0, 10.0, 8.0, 9.0);
            pen.poly(&[(6.0, 10.0), (13.0, 6.5), (18.0, 9.0), (11.0, 12.5)], pen.hole, a);
        }
        Tool::Combine => {
            pen.poly(&[(3.0, 8.0), (14.0, 8.0), (14.0, 19.0), (3.0, 19.0)], pen.fill, ink);
            pen.poly(&[(9.0, 4.0), (21.0, 4.0), (21.0, 15.0), (9.0, 15.0)], pen.fill, a);
        }
        Tool::MoveBody => {
            for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
                pen.arrow((12.0, 12.0), (12.0 + 9.0 * dx, 12.0 + 9.0 * dy), a);
            }
            pen.poly(&[(9.5, 9.5), (14.5, 9.5), (14.5, 14.5), (9.5, 14.5)], pen.fill, ink);
        }
        Tool::PatternLinear => {
            for (i, (x, y)) in [(4.0, 4.0), (14.0, 4.0), (4.0, 14.0), (14.0, 14.0)].into_iter().enumerate() {
                let first = i == 0;
                pen.poly(
                    &[(x, y), (x + 6.0, y), (x + 6.0, y + 6.0), (x, y + 6.0)],
                    if first { pen.fill } else { CLEAR },
                    if first { ink } else { a },
                );
            }
        }
        Tool::PatternCircular => {
            for i in 0..6 {
                let t = i as f32 / 6.0 * TAU;
                let first = i == 0;
                pen.circle(
                    12.0 + 7.5 * t.cos(),
                    12.0 + 7.5 * t.sin(),
                    2.4,
                    if first { pen.fill } else { CLEAR },
                    if first { ink } else { a },
                );
            }
        }
        Tool::Mirror => {
            pen.p.add(Shape::dashed_line(
                &[pen.at(12.0, 2.0), pen.at(12.0, 22.0)],
                Stroke::new(pen.w, ink),
                2.5 * pen.s(),
                1.8 * pen.s(),
            ));
            pen.poly(&[(3.0, 7.0), (9.0, 10.0), (9.0, 17.0), (3.0, 17.0)], pen.fill, ink);
            pen.poly(&[(21.0, 7.0), (15.0, 10.0), (15.0, 17.0), (21.0, 17.0)], CLEAR, a);
        }
        Tool::Plane => {
            pen.poly(&[(3.0, 15.0), (12.0, 8.0), (21.0, 11.0), (12.0, 18.0)], pen.fill, a);
            pen.line(&[(12.0, 18.0), (12.0, 22.0)], ink);
            pen.dot(12.0, 22.0, ink);
        }
        Tool::Measure => {
            pen.poly(&[(2.0, 15.0), (15.0, 2.0), (22.0, 9.0), (9.0, 22.0)], pen.fill, ink);
            for i in 0..4 {
                let t = 5.0 + i as f32 * 3.4;
                pen.line(&[(t, 15.0 - (t - 2.0)), (t + 2.0, 15.0 - (t - 2.0) + 2.0)], a);
            }
        }
        Tool::Info => {
            pen.circle(12.0, 12.0, 9.0, pen.fill, ink);
            pen.dot(12.0, 7.5, a);
            pen.line(&[(12.0, 11.0), (12.0, 17.0)], a);
        }
        Tool::Import => {
            pen.iso_box(4.0, 13.0, 12.0, 7.0, 6.0);
            pen.arrow((20.0, 3.0), (12.0, 11.0), a);
        }
        Tool::Export => {
            pen.poly(&[(4.0, 10.0), (4.0, 20.0), (16.0, 20.0), (16.0, 10.0)], pen.fill, ink);
            pen.arrow((10.0, 14.0), (20.0, 4.0), a);
        }
        Tool::Open => {
            pen.poly(&[(3.0, 7.0), (9.0, 7.0), (11.0, 9.0), (20.0, 9.0), (20.0, 19.0), (3.0, 19.0)], pen.fill, ink);
        }
        Tool::Save => {
            pen.poly(&[(4.0, 4.0), (17.0, 4.0), (20.0, 7.0), (20.0, 20.0), (4.0, 20.0)], pen.fill, ink);
            pen.poly(&[(8.0, 4.0), (16.0, 4.0), (16.0, 9.0), (8.0, 9.0)], pen.hole, ink);
        }
        Tool::Undo | Tool::Redo => {
            let (x0, x1) = if tool == Tool::Undo { (19.0, 5.0) } else { (5.0, 19.0) };
            pen.line(&[(x0, 18.0), (x0, 12.0), (12.0, 9.0), (x1, 9.0)], ink);
            pen.arrow((x1 + (x0 - x1) * 0.15, 9.0), (x1, 9.0), ink);
        }
        Tool::Line => {
            pen.line(&[(4.0, 19.0), (20.0, 5.0)], a);
            pen.dot(4.0, 19.0, ink);
            pen.dot(20.0, 5.0, ink);
        }
        Tool::Polyline => {
            pen.line(&[(3.0, 18.0), (9.0, 7.0), (15.0, 16.0), (21.0, 5.0)], a);
            for (x, y) in [(3.0, 18.0), (9.0, 7.0), (15.0, 16.0), (21.0, 5.0)] {
                pen.dot(x, y, ink);
            }
        }
        Tool::Rect => {
            pen.closed(&[(4.0, 6.0), (20.0, 6.0), (20.0, 18.0), (4.0, 18.0)], a);
            pen.dot(4.0, 18.0, ink);
            pen.dot(20.0, 6.0, ink);
        }
        Tool::Circle => {
            pen.circle(12.0, 12.0, 8.0, CLEAR, a);
            pen.dot(12.0, 12.0, ink);
            pen.line(&[(12.0, 12.0), (17.6, 6.4)], ink);
        }
        Tool::Arc => {
            pen.arc(12.0, 16.0, 9.0, 0.0, PI, a);
            pen.dot(3.0, 16.0, ink);
            pen.dot(21.0, 16.0, ink);
            pen.dot(12.0, 7.0, ink);
        }
        Tool::Constraint => {
            pen.line(&[(5.0, 9.0), (19.0, 9.0)], a);
            pen.line(&[(5.0, 15.0), (19.0, 15.0)], a);
            pen.line(&[(9.0, 4.0), (9.0, 20.0)], ink);
        }
        Tool::Trim => {
            pen.line(&[(12.0, 3.0), (12.0, 21.0)], ink);
            pen.line(&[(3.0, 12.0), (12.0, 12.0)], ink);
            pen.p.add(Shape::dashed_line(
                &[pen.at(12.0, 12.0), pen.at(21.0, 12.0)],
                Stroke::new(pen.w, a),
                2.0 * pen.s(),
                1.6 * pen.s(),
            ));
            pen.line(&[(15.0, 15.0), (19.0, 19.0)], a);
            pen.line(&[(19.0, 15.0), (15.0, 19.0)], a);
        }
        Tool::SketchPattern => {
            for (i, x) in [6.0f32, 12.0, 18.0].into_iter().enumerate() {
                pen.circle(x, 10.0, 2.6, if i == 0 { pen.fill } else { CLEAR }, if i == 0 { ink } else { a });
            }
            pen.arrow((4.0, 19.0), (20.0, 19.0), a);
        }
        Tool::MovePoint => {
            pen.circle(12.0, 12.0, 2.4, pen.fill, ink);
            for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
                pen.arrow((12.0 + 3.5 * dx, 12.0 + 3.5 * dy), (12.0 + 9.5 * dx, 12.0 + 9.5 * dy), a);
            }
        }
        Tool::DeleteItems => {
            pen.line(&[(6.0, 6.0), (18.0, 18.0)], a);
            pen.line(&[(18.0, 6.0), (6.0, 18.0)], a);
        }
        Tool::Orbit => {
            pen.arc(12.0, 12.0, 8.0, 0.3, 5.5, ink);
            pen.arrow((17.0, 17.5), (19.6, 14.5), a);
            pen.dot(12.0, 12.0, ink);
        }
        Tool::Pan => {
            for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
                pen.arrow((12.0 + 3.0 * dx, 12.0 + 3.0 * dy), (12.0 + 9.0 * dx, 12.0 + 9.0 * dy), ink);
            }
        }
        Tool::Zoom | Tool::Search => {
            pen.circle(10.0, 10.0, 6.0, pen.fill, ink);
            pen.line(&[(14.5, 14.5), (20.0, 20.0)], ink);
        }
        Tool::Fit => {
            for (cx, cy, dx, dy) in
                [(4.0, 4.0, 1.0, 1.0), (20.0, 4.0, -1.0, 1.0), (4.0, 20.0, 1.0, -1.0), (20.0, 20.0, -1.0, -1.0)]
            {
                pen.line(&[(cx + dx * 5.0, cy), (cx, cy), (cx, cy + dy * 5.0)], ink);
            }
            pen.poly(&[(9.0, 9.0), (15.0, 9.0), (15.0, 15.0), (9.0, 15.0)], pen.fill, a);
        }
        Tool::Home => {
            pen.poly(&[(4.0, 12.0), (12.0, 4.0), (20.0, 12.0)], pen.fill, ink);
            pen.poly(&[(6.0, 12.0), (18.0, 12.0), (18.0, 20.0), (6.0, 20.0)], pen.fill, ink);
        }
        Tool::Perspective => {
            pen.closed(&[(4.0, 6.0), (20.0, 4.0), (20.0, 20.0), (4.0, 18.0)], ink);
            pen.closed(&[(9.0, 9.0), (15.0, 8.5), (15.0, 15.5), (9.0, 15.0)], a);
        }
        Tool::Params => {
            pen.line(&[(8.0, 20.0), (9.0, 8.0), (10.0, 5.0), (13.0, 4.0)], ink);
            pen.line(&[(6.0, 11.0), (12.0, 11.0)], ink);
            pen.line(&[(14.0, 13.0), (20.0, 20.0)], a);
            pen.line(&[(20.0, 13.0), (14.0, 20.0)], a);
        }
        Tool::Body => pen.iso_box(4.0, 8.0, 9.0, 8.0, 8.0),
        Tool::Origin => {
            pen.arrow((12.0, 12.0), (21.0, 12.0), a);
            pen.arrow((12.0, 12.0), (12.0, 3.0), ink);
            pen.arrow((12.0, 12.0), (5.0, 19.0), ink);
            pen.circle(12.0, 12.0, 2.0, pen.fill, ink);
        }
        Tool::Branch => {
            pen.line(&[(7.0, 5.0), (7.0, 19.0)], ink);
            pen.line(&[(7.0, 15.0), (7.0, 12.0), (17.0, 8.0)], a);
            pen.circle(7.0, 5.0, 2.4, pen.fill, ink);
            pen.circle(7.0, 19.0, 2.4, pen.fill, ink);
            pen.circle(17.0, 8.0, 2.4, pen.fill, a);
        }
        Tool::Settings => {
            for (y, x) in [(6.0f32, 15.0f32), (12.0, 8.0), (18.0, 13.0)] {
                pen.line(&[(4.0, y), (20.0, y)], ink);
                pen.circle(x, y, 2.4, pen.hole, a);
            }
        }
        Tool::ToStart => {
            pen.line(&[(6.0, 5.0), (6.0, 19.0)], ink);
            pen.poly(&[(19.0, 5.0), (19.0, 19.0), (9.0, 12.0)], pen.fill, ink);
        }
        Tool::StepBack => pen.poly(&[(17.0, 5.0), (17.0, 19.0), (7.0, 12.0)], pen.fill, ink),
        Tool::StepForward => pen.poly(&[(7.0, 5.0), (7.0, 19.0), (17.0, 12.0)], pen.fill, ink),
        Tool::ToEnd => {
            pen.line(&[(18.0, 5.0), (18.0, 19.0)], ink);
            pen.poly(&[(5.0, 5.0), (5.0, 19.0), (15.0, 12.0)], pen.fill, ink);
        }
        Tool::Edges => {
            pen.poly(&[(12.0, 3.0), (20.0, 7.5), (12.0, 12.0), (4.0, 7.5)], pen.fill, ink);
            pen.poly(&[(4.0, 7.5), (12.0, 12.0), (12.0, 21.0), (4.0, 16.5)], pen.fill.gamma_multiply(0.7), ink);
            pen.poly(&[(12.0, 12.0), (20.0, 7.5), (20.0, 16.5), (12.0, 21.0)], CLEAR, a);
        }
    }
}

/// A square icon (no accessibility node of its own) of `size` logical px.
pub fn tool_icon(ui: &mut Ui, tool: Tool, size: f32, c: Ink) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_tool(ui.painter(), rect, tool, c);
    resp
}

#[cfg(test)]
mod tests {
    /// Non-ASCII characters inside string literals of `src` (comments skipped).
    fn literal_chars(src: &str) -> Vec<char> {
        let mut out = Vec::new();
        let mut chars = src.chars().peekable();
        let mut in_str = false;
        while let Some(c) = chars.next() {
            if in_str {
                match c {
                    '\\' => {
                        chars.next();
                    }
                    '"' => in_str = false,
                    c if !c.is_ascii() && !out.contains(&c) => out.push(c),
                    _ => {}
                }
                continue;
            }
            match c {
                '/' if chars.peek() == Some(&'/') => {
                    for n in chars.by_ref() {
                        if n == '\n' {
                            break;
                        }
                    }
                }
                '"' => in_str = true,
                '\'' => {
                    // A char literal ('"', '\'', 'x'): skip it; a lifetime has no closing quote.
                    let mut ahead = chars.clone();
                    match (ahead.next(), ahead.next(), ahead.next()) {
                        (Some('\\'), Some(_), Some('\'')) => {
                            chars.nth(2);
                        }
                        (Some(_), Some('\''), _) => {
                            chars.nth(1);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        out
    }

    #[test]
    fn literal_scanner_skips_comments_and_chars() {
        let src = "// ● comment\nlet a = \"⚠ x\"; let b = '\"'; let c = \"y → \\\" z\"; fn f<'a>() {}";
        assert_eq!(literal_chars(src), vec!['⚠', '→']);
    }

    /// Every non-ASCII character the UI puts into text must have a real glyph in the
    /// loaded fonts, not the replacement box. (egui's `has_glyphs` reports false
    /// negatives for glyphs that live in the same font as the replacement character, so
    /// this compares the laid-out glyph against the replacement glyph instead.)
    #[test]
    fn every_ui_string_glyph_exists_in_the_loaded_fonts() {
        let sources = [
            include_str!("app.rs"),
            include_str!("agents.rs"),
            include_str!("forms.rs"),
            include_str!("i18n.rs"),
            include_str!("viewport.rs"),
        ];
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::theme::apply(ui.ctx(), crate::theme::ThemeKind::Dark, false)
        });
        out.textures_delta.clear();
        let mut missing = Vec::new();
        for family in [egui::FontFamily::Proportional] {
            let font = egui::FontId::new(13.0, family.clone());
            let uv = |t: &str| -> Vec<([u16; 2], [u16; 2])> {
                ctx.fonts_mut(|f| f.layout_no_wrap(t.into(), font.clone(), egui::Color32::WHITE))
                    .rows
                    .iter()
                    .flat_map(|r| r.row.glyphs.iter().map(|g| (g.uv_rect.min, g.uv_rect.max)))
                    .collect()
            };
            let replacement = uv("\u{25FB}");
            assert!(!replacement.is_empty());
            for src in sources {
                for c in literal_chars(src) {
                    if c == '\u{25FB}' || c.is_whitespace() {
                        continue;
                    }
                    if uv(&c.to_string()).iter().any(|u| replacement.contains(u)) {
                        missing.push((family.clone(), c));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "characters without a glyph (render as boxes): {missing:?}");
    }
}
