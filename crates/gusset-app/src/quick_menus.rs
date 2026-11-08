//! Cursor-local tools (DESIGN.md §1.3): a radial marking menu on a right click in the
//! viewport, and the S-key quick bar at the pointer. Both offer the same context tools:
//! the last command first, then the sketch tools while a sketch is edited (with Finish
//! sketch) or the main solid tools. A click elsewhere or Esc closes them; a quick right-click
//! release only opens the menu, it never runs anything.

use crate::i18n::tr;
use egui::{Pos2, Vec2};

/// A tool the menus offer: a registry command or a special action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Command(&'static str),
    FinishSketch,
    Search,
}

const SKETCH: [&str; 7] = [
    "sketch.add_line",
    "sketch.add_rectangle",
    "sketch.add_circle",
    "sketch.add_arc",
    "sketch.add_polyline",
    "sketch.trim",
    "sketch.add_constraint",
];

const SOLID: [&str; 7] = [
    "sketch.create",
    "feature.extrude",
    "feature.revolve",
    "feature.fillet",
    "feature.hole",
    "feature.shell",
    "query.measure",
];

/// The default tools of a context (the quick bar starts from these).
#[must_use]
pub fn defaults(sketching: bool) -> &'static [&'static str] {
    if sketching { &SKETCH } else { &SOLID }
}

/// The most tools a context's quick bar keeps.
pub const MAX_TOOLS: usize = 7;

/// The context tools, the last command first (at most eight). `custom` replaces the
/// context's defaults when the user has chosen tools (an empty list means the defaults).
#[must_use]
pub fn tools(sketching: bool, last: Option<&'static str>, custom: &[&'static str]) -> Vec<Tool> {
    let base: &[&'static str] = if custom.is_empty() { defaults(sketching) } else { custom };
    let mut out: Vec<Tool> = last.filter(|l| !base.contains(l)).map(Tool::Command).into_iter().collect();
    out.extend(base.iter().map(|id| Tool::Command(id)));
    if sketching {
        out.truncate(7);
        out.push(Tool::FinishSketch);
    }
    out.truncate(8);
    out
}

fn label(t: Tool) -> String {
    match t {
        Tool::Command(id) => {
            gusset_commands::find(id).map_or_else(|| id.to_string(), |s| crate::i18n::command_title(s).to_string())
        }
        Tool::FinishSketch => tr("ribbon.finish_sketch").to_string(),
        Tool::Search => format!("{}…", tr("quick.search")),
    }
}

/// Shortest right drag (logical px) that counts as a flick toward a marking-menu item.
pub const FLICK: f32 = 40.0;

/// The item a flick in direction `dir` (screen, y down) chooses among `n` items placed
/// clockwise from the top, as [`marking`] places them.
#[must_use]
pub fn flicked(dir: Vec2, n: usize) -> Option<usize> {
    if n == 0 || dir.length() < 1e-3 {
        return None;
    }
    let step = std::f32::consts::TAU / n as f32;
    let a = dir.y.atan2(dir.x) + std::f32::consts::FRAC_PI_2;
    let i = (a / step).round().rem_euclid(n as f32) as usize;
    Some(i % n)
}

/// A primary click outside `rect` closes a menu (the right click that opened the marking
/// menu does not).
fn closes(ctx: &egui::Context, rect: egui::Rect) -> bool {
    ctx.input(|i| i.pointer.primary_clicked() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)))
}

/// The marking menu around `center`: the tools on a ring. Returns the chosen tool, or
/// `Some(None)` when it should close (a click elsewhere).
pub fn marking(ctx: &egui::Context, center: Pos2, tools: &[Tool]) -> Option<Option<Tool>> {
    let mut chosen = None;
    let n = tools.len().max(1) as f32;
    // An ellipse wider than tall, so the labels on either side of the bottom never meet.
    let radius = Vec2::new(170.0, 110.0);
    let half = radius + Vec2::new(62.0, 16.0);
    let area = egui::Area::new(egui::Id::new("marking_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(center - half)
        .show(ctx, |ui| {
            let size = 2.0 * half;
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter().circle_stroke(rect.center(), 6.0, ui.visuals().widgets.noninteractive.fg_stroke);
            for (i, t) in tools.iter().enumerate() {
                // Clockwise from the top, as in Fusion's marking menu.
                let a = std::f32::consts::TAU * i as f32 / n - std::f32::consts::FRAC_PI_2;
                let at = rect.center() + Vec2::new(radius.x * a.cos(), radius.y * a.sin());
                let r = egui::Rect::from_center_size(at, Vec2::new(116.0, 26.0));
                if ui.put(r, egui::Button::new(label(*t))).clicked() {
                    chosen = Some(*t);
                }
            }
        });
    match chosen {
        Some(t) => Some(Some(t)),
        None if closes(ctx, area.response.rect) => Some(None),
        None => None,
    }
}

/// The S-key quick bar at `at`: the tools in a row and a search button.
pub fn quick_bar(ctx: &egui::Context, at: Pos2, tools: &[Tool]) -> Option<Option<Tool>> {
    let mut chosen = None;
    let area =
        egui::Area::new(egui::Id::new("quick_bar")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for t in tools.iter().copied().chain([Tool::Search]) {
                        if ui.button(label(t)).clicked() {
                            chosen = Some(t);
                        }
                    }
                });
            });
        });
    match chosen {
        Some(t) => Some(Some(t)),
        None if closes(ctx, area.response.rect) => Some(None),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_follow_the_context_and_lead_with_the_last_command() {
        let solid = tools(false, None, &[]);
        assert_eq!(solid.first(), Some(&Tool::Command("sketch.create")));
        let custom = tools(false, None, &["feature.shell"]);
        assert_eq!(custom, vec![Tool::Command("feature.shell")]);
        let sketch = tools(true, Some("feature.draft"), &[]);
        assert_eq!(sketch.first(), Some(&Tool::Command("feature.draft")));
        assert_eq!(sketch.len(), 8);
        // The last command is not listed twice.
        assert_eq!(
            tools(false, Some("feature.extrude"), &[])
                .iter()
                .filter(|t| **t == Tool::Command("feature.extrude"))
                .count(),
            1
        );
        assert!(tools(true, None, &[]).contains(&Tool::FinishSketch));
    }

    #[test]
    fn a_flick_picks_the_item_in_its_direction() {
        // Eight items clockwise from the top: up is 0, right 2, down 4, left 6.
        assert_eq!(flicked(Vec2::new(0.0, -50.0), 8), Some(0));
        assert_eq!(flicked(Vec2::new(50.0, 0.0), 8), Some(2));
        assert_eq!(flicked(Vec2::new(0.0, 50.0), 8), Some(4));
        assert_eq!(flicked(Vec2::new(-50.0, 0.0), 8), Some(6));
        assert_eq!(flicked(Vec2::new(-40.0, -45.0), 8), Some(7));
        assert_eq!(flicked(Vec2::ZERO, 8), None);
    }
}
