//! The timeline strip (DESIGN.md rule 4): feature chips in history order with a
//! draggable rollback marker between them. Failed features use the danger color,
//! suppressed and rolled-back ones are dimmed. The marker is keyboard accessible:
//! focus it (Tab) and use Left/Right, Home/End.

use crate::theme::{self, Palette};
use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo};
use gusset_eval::FeatureStatus;

/// One feature as the timeline shows it.
#[derive(Debug, Clone)]
pub struct Item<'a> {
    pub name: &'a str,
    pub keyword: &'a str,
    pub suppressed: bool,
    pub status: Option<&'a FeatureStatus>,
}

/// Colors of one chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChipStyle {
    pub fill: Color32,
    pub stroke: Color32,
    pub text: Color32,
}

/// Chip colors: failed features in the danger color, features blocked by a failure in
/// the warning color, and suppressed or rolled-back features dimmed.
pub fn chip_style(item: &Item<'_>, active: bool, pal: &Palette) -> ChipStyle {
    let (fill, stroke, text) = match item.status {
        Some(FeatureStatus::Failed(_)) => (pal.danger.gamma_multiply(0.18), pal.danger, pal.danger),
        Some(FeatureStatus::Blocked { .. }) => (pal.bg_raised, pal.warning, pal.warning),
        _ => (pal.bg_raised, pal.border, pal.text),
    };
    let dimmed = !active || item.suppressed || matches!(item.status, Some(FeatureStatus::Suppressed));
    if dimmed {
        ChipStyle { fill: fill.gamma_multiply(0.45), stroke: stroke.gamma_multiply(0.45), text: pal.text_muted }
    } else {
        ChipStyle { fill, stroke, text }
    }
}

/// Status text for a chip's tooltip.
fn describe(item: &Item<'_>, active: bool) -> String {
    let state = match item.status {
        Some(FeatureStatus::Failed(e)) => match &e.hint {
            Some(h) => format!("failed: {}\n{}: {h}", e.message, crate::i18n::tr("error.fix")),
            None => format!("failed: {}", e.message),
        },
        Some(FeatureStatus::Blocked { by }) => format!("not built: depends on {by}, which failed"),
        _ if item.suppressed => "suppressed".into(),
        _ if !active => "rolled back".into(),
        _ => "ok".into(),
    };
    format!("{} {}: {state}", item.keyword, item.name)
}

const GAP: f32 = 14.0;
const CHIP_H: f32 = 22.0;

/// Draws the strip. `marker` is the number of active features (`items.len()` = all).
/// Returns the new marker when the user moved it (drag released, or a key press).
pub fn show(ui: &mut Ui, items: &[Item<'_>], marker: usize, pal: &Palette) -> Option<usize> {
    let n = items.len();
    let marker = marker.min(n);
    let id = ui.make_persistent_id("timeline-marker");
    let font = egui::TextStyle::Button.resolve(ui.style());
    let pad = theme::SPACE[3];
    let galleys: Vec<_> =
        items.iter().map(|it| ui.painter().layout_no_wrap(it.name.to_string(), font.clone(), pal.text)).collect();
    // Gap centers (marker positions) and chip rects, left to right.
    let height = CHIP_H + 2.0 * theme::SPACE[2];
    let width = GAP * (n as f32 + 1.0) + galleys.iter().map(|g| g.size().x + 2.0 * pad).sum::<f32>();
    let (strip, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let mut gaps = Vec::with_capacity(n + 1);
    let mut chips = Vec::with_capacity(n);
    let mut x = strip.left();
    for g in &galleys {
        gaps.push(x + GAP * 0.5);
        x += GAP;
        let w = g.size().x + 2.0 * pad;
        chips.push(Rect::from_min_size(Pos2::new(x, strip.center().y - CHIP_H * 0.5), Vec2::new(w, CHIP_H)));
        x += w;
    }
    gaps.push(x + GAP * 0.5);
    let nearest_gap = |px: f32| -> usize {
        gaps.iter().enumerate().min_by(|a, b| (a.1 - px).abs().total_cmp(&(b.1 - px).abs())).map_or(n, |(i, _)| i)
    };

    // The marker handle.
    let preview: Option<usize> = ui.data(|d| d.get_temp(id));
    let shown = preview.unwrap_or(marker);
    let gx = gaps.get(shown).copied().unwrap_or(strip.right());
    let handle = Rect::from_center_size(Pos2::new(gx, strip.center().y), Vec2::new(GAP, height));
    let resp = ui.interact(handle, id, Sense::click_and_drag());
    resp.widget_info(|| WidgetInfo::slider(true, shown as f64, crate::i18n::tr("timeline.marker")));
    let mut result = None;
    if resp.dragged() {
        if let Some(p) = ui.ctx().pointer_interact_pos() {
            ui.data_mut(|d| d.insert_temp(id, nearest_gap(p.x)));
        }
    } else if resp.drag_stopped() {
        if let Some(m) = preview
            && m != marker
        {
            result = Some(m);
        }
        ui.data_mut(|d| d.remove::<usize>(id));
    } else if preview.is_some() {
        ui.data_mut(|d| d.remove::<usize>(id));
    }
    if resp.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter { horizontal_arrows: true, vertical_arrows: false, tab: false, escape: false },
            );
        });
        let (left, right, home, end) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::Home),
                i.key_pressed(egui::Key::End),
            )
        });
        let target = if home {
            Some(0)
        } else if end {
            Some(n)
        } else if left {
            Some(marker.saturating_sub(1))
        } else if right {
            Some((marker + 1).min(n))
        } else {
            None
        };
        if let Some(t) = target.filter(|t| *t != marker) {
            result = Some(t);
        }
    }
    let marker_now = result.unwrap_or(ui.data(|d| d.get_temp(id)).unwrap_or(marker));

    // Chips.
    let painter = ui.painter_at(strip.expand(2.0));
    for (i, ((it, g), rect)) in items.iter().zip(&galleys).zip(&chips).enumerate() {
        let active = i < marker_now;
        let st = chip_style(it, active, pal);
        let chip = ui.interact(*rect, id.with(("chip", i)), Sense::hover());
        let hover = chip.hovered();
        painter.rect(
            *rect,
            CornerRadius::same(theme::RADIUS_MD),
            if hover { pal.bg_hover } else { st.fill },
            Stroke::new(1.0, st.stroke),
            StrokeKind::Inside,
        );
        let tp = Pos2::new(rect.left() + pad, rect.center().y - g.size().y * 0.5);
        painter.galley_with_override_text_color(tp, g.clone(), st.text);
        chip.on_hover_text(describe(it, active));
    }

    // Marker: an accent bar with a grip, and a focus ring for keyboard users.
    let gx = gaps.get(marker_now).copied().unwrap_or(strip.right());
    let accent = pal.accent;
    painter.line_segment(
        [Pos2::new(gx, strip.top() + 2.0), Pos2::new(gx, strip.bottom() - 2.0)],
        Stroke::new(2.0, accent),
    );
    let grip = Rect::from_center_size(Pos2::new(gx, strip.top() + 5.0), Vec2::new(10.0, 8.0));
    painter.rect_filled(grip, CornerRadius::same(theme::RADIUS_SM), accent);
    if resp.has_focus() {
        painter.rect_stroke(
            handle.shrink(1.0),
            CornerRadius::same(theme::RADIUS_SM),
            Stroke::new(1.5, accent),
            StrokeKind::Inside,
        );
    }
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    resp.on_hover_text(crate::i18n::tr("timeline.rollback"));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::DARK;

    #[test]
    fn failed_chips_use_danger_and_inactive_ones_are_dimmed() {
        let err = gusset_eval::EvalError::new("F", gusset_eval::ErrorCode::Expression, "boom");
        let failed = FeatureStatus::Failed(err);
        let item = |status| Item { name: "F", keyword: "fillet", suppressed: false, status };
        let ok = chip_style(&item(Some(&FeatureStatus::Ok)), true, &DARK);
        assert_eq!(ok.text, DARK.text);
        let bad = chip_style(&item(Some(&failed)), true, &DARK);
        assert_eq!((bad.stroke, bad.text), (DARK.danger, DARK.danger));
        let rolled = chip_style(&item(Some(&FeatureStatus::RolledBack)), false, &DARK);
        assert_eq!(rolled.text, DARK.text_muted);
        assert!(rolled.fill.a() < ok.fill.a());
        let sup = chip_style(&Item { suppressed: true, ..item(Some(&FeatureStatus::Suppressed)) }, true, &DARK);
        assert_eq!(sup.text, DARK.text_muted);
    }
}
