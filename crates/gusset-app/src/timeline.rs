//! The timeline strip (DESIGN.md §2): playback controls and one icon chip per feature in
//! history order, with a draggable rollback marker between them. A chip shows its state by
//! color and a small badge: failed (danger), cancelled and blocked (warning), suppressed and
//! rolled back (dimmed ring). The feature's name and state are in the tooltip. The marker
//! is keyboard accessible: focus it (Tab) and use Left/Right, Home/End; the playback
//! buttons move it too.

use crate::icons::{self, Icon, Ink, Tool};
use crate::theme::{self, Palette};
use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType};
use gusset_eval::{ErrorCode, FeatureStatus};

/// One feature as the timeline shows it.
#[derive(Debug, Clone)]
pub struct Item<'a> {
    pub name: &'a str,
    pub keyword: &'a str,
    pub suppressed: bool,
    pub status: Option<&'a FeatureStatus>,
}

/// What a chip says about its feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipState {
    Ok,
    Failed,
    /// The rebuild was cancelled before this feature ran.
    Cancelled,
    /// Not built: a feature it depends on failed.
    Blocked,
    Suppressed,
    /// After the rollback marker.
    RolledBack,
}

impl ChipState {
    /// The state in words (the chip's accessible name).
    pub fn name(self) -> &'static str {
        match self {
            ChipState::Ok => "ok",
            ChipState::Failed => "failed",
            ChipState::Cancelled => "cancelled",
            ChipState::Blocked => "blocked",
            ChipState::Suppressed => "suppressed",
            ChipState::RolledBack => "rolled back",
        }
    }
}

/// The state a chip shows. `active` is whether the marker is after the feature.
pub fn chip_state(item: &Item<'_>, active: bool) -> ChipState {
    match item.status {
        Some(FeatureStatus::Failed(e)) if e.code == ErrorCode::Cancelled => ChipState::Cancelled,
        Some(FeatureStatus::Failed(_)) => ChipState::Failed,
        Some(FeatureStatus::Blocked { .. }) => ChipState::Blocked,
        _ if item.suppressed || matches!(item.status, Some(FeatureStatus::Suppressed)) => ChipState::Suppressed,
        _ if !active || matches!(item.status, Some(FeatureStatus::RolledBack)) => ChipState::RolledBack,
        _ => ChipState::Ok,
    }
}

/// Colors of one chip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChipStyle {
    pub fill: Color32,
    pub stroke: Color32,
    pub text: Color32,
    /// The feature icon's colors.
    pub ink: Ink,
    /// A status badge in the chip's corner.
    pub badge: Option<(Icon, Color32)>,
}

/// Chip colors: failed features in the danger color, features blocked by a failure or a
/// cancelled rebuild in the warning color, and suppressed or rolled-back features dimmed.
pub fn chip_style(item: &Item<'_>, active: bool, pal: &Palette) -> ChipStyle {
    let state = chip_state(item, active);
    let (fill, stroke, text) = match state {
        ChipState::Failed => (pal.danger.gamma_multiply(0.18), pal.danger, pal.danger),
        ChipState::Cancelled | ChipState::Blocked => (pal.bg_raised, pal.warning, pal.warning),
        _ => (pal.bg_raised, pal.border, pal.text),
    };
    let badge = match state {
        ChipState::Failed => Some((Icon::Warning, pal.danger)),
        ChipState::Cancelled | ChipState::Blocked => Some((Icon::Pending, pal.warning)),
        ChipState::Suppressed | ChipState::RolledBack => Some((Icon::Ring, pal.text_muted)),
        ChipState::Ok => None,
    };
    // Anything the marker passed over, and suppressed features, are dimmed.
    if !active || matches!(state, ChipState::Suppressed | ChipState::RolledBack) {
        ChipStyle {
            fill: fill.gamma_multiply(0.45),
            stroke: stroke.gamma_multiply(0.45),
            text: pal.text_muted,
            ink: Ink::disabled(pal),
            badge,
        }
    } else {
        let ink = match state {
            ChipState::Failed | ChipState::Cancelled | ChipState::Blocked => Ink::mono(text),
            _ => Ink::new(pal),
        };
        ChipStyle { fill, stroke, text, ink, badge }
    }
}

/// Status text for a chip's tooltip.
fn describe(item: &Item<'_>, active: bool) -> String {
    let state = match item.status {
        Some(FeatureStatus::Failed(e)) if e.code == ErrorCode::Cancelled => "cancelled: the rebuild was stopped".into(),
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

/// A playback button of the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    ToStart,
    StepBack,
    StepForward,
    ToEnd,
}

impl Playback {
    pub const ALL: [Playback; 4] = [Playback::ToStart, Playback::StepBack, Playback::StepForward, Playback::ToEnd];

    pub fn label(self) -> &'static str {
        crate::i18n::tr(match self {
            Playback::ToStart => "timeline.to_start",
            Playback::StepBack => "timeline.step_back",
            Playback::StepForward => "timeline.step_forward",
            Playback::ToEnd => "timeline.to_end",
        })
    }

    fn tool(self) -> Tool {
        match self {
            Playback::ToStart => Tool::ToStart,
            Playback::StepBack => Tool::StepBack,
            Playback::StepForward => Tool::StepForward,
            Playback::ToEnd => Tool::ToEnd,
        }
    }

    /// Where the marker (the number of active features, `n` = all) goes, if it moves.
    pub fn target(self, marker: usize, n: usize) -> Option<usize> {
        let marker = marker.min(n);
        let t = match self {
            Playback::ToStart => 0,
            Playback::StepBack => marker.saturating_sub(1),
            Playback::StepForward => (marker + 1).min(n),
            Playback::ToEnd => n,
        };
        (t != marker).then_some(t)
    }
}

/// The playback buttons (to start, step back, step forward, to end). Returns the new marker
/// when one was pressed; buttons that would not move it are disabled.
pub fn playback(ui: &mut Ui, marker: usize, n: usize, pal: &Palette) -> Option<usize> {
    let mut result = None;
    for b in Playback::ALL {
        let enabled = b.target(marker, n).is_some();
        let ink = if enabled { Ink::new(pal) } else { Ink::disabled(pal) };
        let resp = ui
            .add_enabled_ui(enabled, |ui| {
                crate::ribbon::icon_button_with(ui, pal, b.label(), None, |p, r| {
                    icons::paint_tool(p, r.shrink(5.0), b.tool(), ink);
                })
            })
            .inner;
        if resp.clicked() {
            result = b.target(marker, n);
        }
    }
    result
}

const GAP: f32 = 14.0;
const CHIP_H: f32 = 26.0;
const CHIP_W: f32 = 30.0;
const CHIP_ICON: f32 = 18.0;

/// Draws the strip. `marker` is the number of active features (`items.len()` = all).
/// Returns the new marker when the user moved it (drag released, or a key press).
pub fn show(ui: &mut Ui, items: &[Item<'_>], marker: usize, pal: &Palette) -> Option<usize> {
    let n = items.len();
    let marker = marker.min(n);
    let id = ui.make_persistent_id("timeline-marker");
    // Gap centers (marker positions) and chip rects, left to right.
    let height = CHIP_H + 2.0 * theme::SPACE[2];
    let width = GAP * (n as f32 + 1.0) + CHIP_W * n as f32;
    let (strip, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let mut gaps = Vec::with_capacity(n + 1);
    let mut chips = Vec::with_capacity(n);
    let mut x = strip.left();
    for _ in items {
        gaps.push(x + GAP * 0.5);
        x += GAP;
        chips.push(Rect::from_min_size(Pos2::new(x, strip.center().y - CHIP_H * 0.5), Vec2::new(CHIP_W, CHIP_H)));
        x += CHIP_W;
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
    for (i, (it, rect)) in items.iter().zip(&chips).enumerate() {
        let active = i < marker_now;
        let st = chip_style(it, active, pal);
        let chip = ui.interact(*rect, id.with(("chip", i)), Sense::hover());
        let label = format!("{} ({})", it.name, chip_state(it, active).name());
        chip.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &label));
        let hover = chip.hovered();
        painter.rect(
            *rect,
            CornerRadius::same(theme::RADIUS_MD),
            if hover { pal.bg_hover } else { st.fill },
            Stroke::new(1.0, st.stroke),
            StrokeKind::Inside,
        );
        let ic = Rect::from_center_size(rect.center(), Vec2::splat(CHIP_ICON));
        icons::paint_tool(&painter, ic, icons::tool_for_keyword(it.keyword), st.ink);
        if let Some((badge, col)) = st.badge {
            let b = Rect::from_center_size(Pos2::new(rect.right() - 5.0, rect.bottom() - 5.0), Vec2::splat(10.0));
            painter.circle_filled(b.center(), 6.0, pal.bg_panel);
            icons::paint(&painter, b, badge, col);
        }
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

    #[test]
    fn chip_states_and_badges() {
        let err = |code| gusset_eval::EvalError::new("F", code, "x");
        let failed = FeatureStatus::Failed(err(gusset_eval::ErrorCode::Expression));
        let cancelled = FeatureStatus::Failed(err(gusset_eval::ErrorCode::Cancelled));
        let blocked = FeatureStatus::Blocked { by: "A".into() };
        let item = |status| Item { name: "F", keyword: "fillet", suppressed: false, status };
        let state = |s, active| chip_state(&item(s), active);
        assert_eq!(state(Some(&FeatureStatus::Ok), true), ChipState::Ok);
        assert_eq!(state(Some(&failed), true), ChipState::Failed);
        assert_eq!(state(Some(&cancelled), true), ChipState::Cancelled);
        assert_eq!(state(Some(&blocked), true), ChipState::Blocked);
        assert_eq!(state(Some(&FeatureStatus::Suppressed), true), ChipState::Suppressed);
        assert_eq!(state(Some(&FeatureStatus::RolledBack), false), ChipState::RolledBack);
        assert_eq!(state(None, false), ChipState::RolledBack);
        // Failed and cancelled differ in color and badge; healthy chips have no badge.
        let badge = |s| chip_style(&item(s), true, &DARK).badge;
        assert_eq!(badge(Some(&failed)), Some((Icon::Warning, DARK.danger)));
        assert_eq!(badge(Some(&cancelled)), Some((Icon::Pending, DARK.warning)));
        assert_eq!(badge(Some(&FeatureStatus::Ok)), None);
        assert_eq!(badge(Some(&FeatureStatus::Suppressed)).map(|b| b.0), Some(Icon::Ring));
        assert!(describe(&item(Some(&cancelled)), true).contains("cancelled"));
    }

    #[test]
    fn playback_targets_stay_in_range_and_only_move_when_they_can() {
        use Playback::*;
        assert_eq!(ToStart.target(3, 3), Some(0));
        assert_eq!(ToStart.target(0, 3), None);
        assert_eq!(StepBack.target(3, 3), Some(2));
        assert_eq!(StepBack.target(0, 3), None);
        assert_eq!(StepForward.target(0, 3), Some(1));
        assert_eq!(StepForward.target(3, 3), None);
        assert_eq!(ToEnd.target(1, 3), Some(3));
        assert_eq!(ToEnd.target(3, 3), None);
        // An out-of-range marker is clamped; an empty timeline has nowhere to go.
        assert_eq!(StepForward.target(9, 3), None);
        assert!(Playback::ALL.iter().all(|p| p.target(0, 0).is_none()));
    }
}
