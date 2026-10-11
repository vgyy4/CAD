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

/// A small icon toggle (e.g. the browser's sketch visibility eye). `label` names it for
/// screen readers and tests; `on` is its checked state.
pub fn toggle(ui: &mut Ui, on: bool, on_icon: Icon, off_icon: Icon, label: &str) -> Response {
    let s = icon_size(ui) + crate::theme::SPACE[2];
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
