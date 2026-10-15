//! The workspace tabs and the ribbon (DESIGN.md §2).
//!
//! The ribbon is a row of large icon tools grouped under dropdown labels (CREATE, MODIFY,
//! CONSTRUCT, INSPECT, INSERT). Every tool is a registry command (ADR 0012): a tool whose
//! command does not exist is not shown, and each dropdown lists every command of its
//! group. The Solid tab shows the solid tool set; while a sketch is being edited the Sketch
//! tab replaces it, with a prominent "Finish sketch". The widgets only report what was
//! chosen ([`Action`]); the app launches the command.

use crate::i18n::tr;
use crate::icons::{self, Ink, Tool};
use crate::theme::{self, Palette};
use egui::{Pos2, Rect, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType};
use gusset_commands::CommandSpec;

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Start a registry command.
    Run(&'static str),
    /// The Solid tab: leaves sketch editing.
    SolidTab,
    /// The Sketch tab: edits the latest sketch (or starts a new one).
    SketchTab,
    /// "Finish sketch".
    FinishSketch,
    /// The command search (Ctrl+K).
    Search,
}

/// One tool of a group.
struct Def {
    id: &'static str,
    /// Short label under the large icon.
    label: &'static str,
}

const fn d(id: &'static str, label: &'static str) -> Def {
    Def { id, label }
}

/// A ribbon group: a dropdown with every command and large buttons for the first `large`.
struct Group {
    /// i18n key of its name.
    key: &'static str,
    tools: &'static [Def],
    large: usize,
}

const SOLID: &[Group] = &[
    Group {
        key: "ribbon.create",
        large: 4,
        tools: &[
            d("sketch.create", "Sketch"),
            d("feature.extrude", "Extrude"),
            d("feature.revolve", "Revolve"),
            d("feature.hole", "Hole"),
            d("feature.sweep", "Sweep"),
            d("feature.loft", "Loft"),
            d("generate.gear", "Gear"),
        ],
    },
    Group {
        key: "ribbon.modify",
        large: 4,
        tools: &[
            d("feature.fillet", "Fillet"),
            d("feature.chamfer", "Chamfer"),
            d("feature.shell", "Shell"),
            d("feature.combine", "Combine"),
            d("feature.draft", "Draft"),
            d("feature.offset_face", "Offset face"),
            d("feature.text", "Text"),
            d("feature.move_body", "Move"),
            d("feature.pattern_linear", "Pattern"),
            d("feature.pattern_circular", "Circular"),
            d("feature.mirror", "Mirror"),
            d("param.set", "Parameter"),
            d("param.rename", "Rename"),
            d("param.delete", "Delete"),
        ],
    },
    Group {
        key: "ribbon.assemble",
        large: 2,
        tools: &[
            d("feature.joint", "Joint"),
            d("assembly.insert_component", "Component"),
            d("assembly.update_component", "Update"),
        ],
    },
    Group { key: "ribbon.construct", large: 1, tools: &[d("feature.plane", "Plane")] },
    Group { key: "ribbon.inspect", large: 1, tools: &[d("query.body", "Measure"), d("query.model", "Overview")] },
    Group { key: "ribbon.insert", large: 1, tools: &[d("import.file", "Import")] },
];

const SKETCH: &[Group] = &[
    Group {
        key: "ribbon.create",
        large: 4,
        tools: &[
            d("sketch.add_line", "Line"),
            d("sketch.add_rectangle", "Rectangle"),
            d("sketch.add_circle", "Circle"),
            d("sketch.add_arc", "Arc"),
            d("sketch.add_polyline", "Polyline"),
            d("sketch.pattern", "Pattern"),
        ],
    },
    Group {
        key: "ribbon.modify",
        large: 3,
        tools: &[d("sketch.trim", "Trim"), d("sketch.move_point", "Move"), d("sketch.delete_items", "Delete")],
    },
    Group { key: "ribbon.constraints", large: 1, tools: &[d("sketch.add_constraint", "Constrain")] },
    Group { key: "ribbon.inspect", large: 1, tools: &[d("query.sketch", "Status")] },
];

/// Width of a large button.
const BTN_W: f32 = 56.0;
/// Height of a large button (icon above its label).
const BTN_H: f32 = 52.0;
/// Edge of a large icon.
const ICON: f32 = 28.0;
/// Narrowest group (its dropdown label).
const DROPDOWN_W: f32 = 76.0;
const GROUP_GAP: f32 = 8.0;

fn spec(id: &str) -> Option<&'static CommandSpec> {
    gusset_commands::find(id)
}

/// The tools of a group that exist in the registry.
fn present(g: &Group) -> Vec<(&'static Def, &'static CommandSpec)> {
    g.tools.iter().filter_map(|t| spec(t.id).map(|s| (t, s))).collect()
}

/// The command ids the ribbon offers in a workspace (tests check them against the registry).
pub fn command_ids(sketch: bool) -> Vec<&'static str> {
    let groups = if sketch { SKETCH } else { SOLID };
    groups.iter().flat_map(|g| present(g)).map(|(_, s)| s.id).collect()
}

/// What the ribbon needs to know.
#[derive(Debug)]
pub struct Ribbon<'a> {
    pub pal: &'a Palette,
    /// The sketch being edited (the Sketch tab is active), if any.
    pub sketch: Option<&'a str>,
}

/// A tool's tooltip heading: the command title and its shortcut.
pub fn tooltip_title(s: &CommandSpec) -> String {
    match s.shortcut {
        Some(k) => format!("{} ({k})", s.title),
        None => s.title.to_string(),
    }
}

fn tooltip(ui: &mut Ui, s: &CommandSpec) {
    ui.set_max_width(280.0);
    ui.strong(tooltip_title(s));
    ui.label(s.description);
}

/// A small square icon button (the title bar's open, save, undo, redo). `label` is its
/// tooltip and accessible name.
pub fn icon_button(ui: &mut Ui, pal: &Palette, tool: Tool, label: &str) -> egui::Response {
    icon_button_with(ui, pal, label, None, |p, r| icons::paint_tool(p, r.shrink(4.0), tool, Ink::new(pal)))
}

/// An icon button that paints its own icon (`selected`: a toggle that is on).
pub fn icon_button_with(
    ui: &mut Ui,
    pal: &Palette,
    label: &str,
    selected: Option<bool>,
    paint: impl FnOnce(&egui::Painter, Rect),
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
    resp.widget_info(|| match selected {
        Some(on) => WidgetInfo::selected(WidgetType::Button, true, on, label),
        None => WidgetInfo::labeled(WidgetType::Button, true, label),
    });
    if selected == Some(true) {
        ui.painter().rect_filled(rect, theme::RADIUS_SM, pal.accent.gamma_multiply(0.25));
    } else if resp.hovered() || resp.has_focus() {
        ui.painter().rect_filled(rect, theme::RADIUS_SM, pal.bg_hover);
    }
    paint(ui.painter(), rect);
    resp.on_hover_text(label)
}

/// The Design workspace switcher, the tabs and the search button.
pub fn tabs(ui: &mut Ui, r: &Ribbon<'_>) -> Option<Action> {
    let pal = r.pal;
    let mut action = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE[4];
        workspace_switcher(ui, pal);
        let sketching = r.sketch.is_some();
        for (label, active, act) in
            [(tr("tab.solid"), !sketching, Action::SolidTab), (tr("tab.sketch"), sketching, Action::SketchTab)]
        {
            if tab(ui, &label.to_uppercase(), active, pal).clicked() {
                action = Some(act);
            }
        }
        if let Some(name) = r.sketch {
            ui.weak(name);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if search_button(ui, pal).clicked() {
                action = Some(Action::Search);
            }
        });
    });
    action
}

/// "Design" with a dropdown arrow: the workspace switcher (Design is the only workspace so
/// far; others are listed as they ship).
fn workspace_switcher(ui: &mut Ui, pal: &Palette) {
    let name = tr("workspace.design").to_uppercase();
    let font = egui::FontId::proportional(theme::FONT_BODY);
    let galley = ui.painter().layout_no_wrap(name.clone(), font, pal.text);
    let size = Vec2::new(galley.size().x + 38.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.as_str()));
    let fill = if resp.hovered() { pal.bg_hover } else { pal.bg_raised };
    ui.painter().rect(rect, theme::RADIUS_SM, fill, Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
    ui.painter().galley(Pos2::new(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0), galley, pal.text);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(Pos2::new(rect.right() - 12.0, rect.center().y), Vec2::splat(12.0)),
        icons::Icon::ChevronDown,
        pal.text_muted,
    );
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(180.0);
        ui.add_enabled(true, egui::Button::selectable(true, tr("workspace.design")));
        ui.separator();
        ui.weak("More workspaces arrive as they ship.");
    });
}

/// A workspace tab: caption text with an accent underline when active.
fn tab(ui: &mut Ui, label: &str, active: bool, pal: &Palette) -> egui::Response {
    let font = egui::FontId::proportional(theme::FONT_BODY);
    let color = if active { pal.text } else { pal.text_muted };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    let size = Vec2::new(galley.size().x + 16.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, label));
    if resp.hovered() && !active {
        ui.painter().rect_filled(rect, theme::RADIUS_SM, pal.bg_hover);
    }
    ui.painter().galley(
        Pos2::new(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    if active {
        let y = rect.bottom() + 1.0;
        ui.painter().line_segment(
            [Pos2::new(rect.left() + 4.0, y), Pos2::new(rect.right() - 4.0, y)],
            Stroke::new(2.0, pal.accent),
        );
    }
    resp
}

fn search_button(ui: &mut Ui, pal: &Palette) -> egui::Response {
    let text = tr("ribbon.search");
    let font = egui::FontId::proportional(theme::FONT_BODY);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, pal.text_muted);
    let size = Vec2::new(galley.size().x + 40.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, text));
    let fill = if resp.hovered() { pal.bg_hover } else { pal.bg_raised };
    ui.painter().rect(rect, theme::RADIUS_SM, fill, Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
    icons::paint_tool(
        ui.painter(),
        Rect::from_center_size(Pos2::new(rect.left() + 15.0, rect.center().y), Vec2::splat(15.0)),
        Tool::Search,
        Ink::new(pal),
    );
    ui.painter().galley(Pos2::new(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0), galley, pal.text_muted);
    resp
}

/// The ribbon: groups of large icon tools, each with its dropdown.
pub fn show(ui: &mut Ui, r: &Ribbon<'_>) -> Option<Action> {
    let groups = if r.sketch.is_some() { SKETCH } else { SOLID };
    let pal = r.pal;
    let mut action = None;
    // Fit the large buttons into the width: the widest groups give up buttons first (the
    // rest stays in their dropdowns).
    let shown: Vec<_> = groups.iter().map(present).collect();
    let mut large: Vec<usize> = groups.iter().zip(&shown).map(|(g, t)| g.large.min(t.len())).collect();
    let width = |large: &[usize]| -> f32 {
        large.iter().map(|&n| (n as f32 * BTN_W).max(DROPDOWN_W) + GROUP_GAP).sum::<f32>()
            + if r.sketch.is_some() { BTN_W * 2.0 + GROUP_GAP } else { 0.0 }
    };
    while width(&large) > ui.available_width() {
        let Some((i, _)) = large.iter().enumerate().filter(|(_, n)| **n > 1).max_by_key(|(i, n)| (**n, *i)) else {
            break;
        };
        if let Some(n) = large.get_mut(i) {
            *n -= 1;
        }
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(0.0, 0.0);
        for ((g, tools), &n_large) in groups.iter().zip(&shown).zip(&large) {
            if tools.is_empty() {
                continue;
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(0.0, theme::SPACE[1]);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (def, s) in tools.iter().take(n_large) {
                        if large_button(ui, def, s, pal).clicked() {
                            action = Some(Action::Run(s.id));
                        }
                    }
                });
                if let Some(a) = dropdown(ui, tr(g.key), tools, (n_large as f32 * BTN_W).max(DROPDOWN_W), pal) {
                    action = Some(a);
                }
            });
            group_separator(ui, pal);
        }
        if r.sketch.is_some() && finish_button(ui, pal).clicked() {
            action = Some(Action::FinishSketch);
        }
    });
    action
}

fn group_separator(ui: &mut Ui, pal: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(GROUP_GAP, BTN_H + 20.0), Sense::hover());
    let x = rect.center().x;
    ui.painter().line_segment(
        [Pos2::new(x, rect.top() + 4.0), Pos2::new(x, rect.bottom() - 4.0)],
        Stroke::new(1.0, pal.border),
    );
}

/// A large icon button with its short label below.
fn large_button(ui: &mut Ui, def: &Def, s: &CommandSpec, pal: &Palette) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(BTN_W, BTN_H), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, s.title));
    let painter = ui.painter();
    if resp.hovered() || resp.has_focus() {
        painter.rect(
            rect.shrink(1.0),
            theme::RADIUS_MD,
            pal.bg_hover,
            Stroke::new(1.0, if resp.has_focus() { pal.accent } else { pal.border }),
            egui::StrokeKind::Inside,
        );
    }
    let icon_rect =
        Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 5.0 + ICON / 2.0), Vec2::splat(ICON));
    match icons::tool_for_command(s.id) {
        Some(t) => icons::paint_tool(painter, icon_rect, t, Ink::new(pal)),
        None => {
            painter.circle_stroke(icon_rect.center(), ICON * 0.35, Stroke::new(1.5, pal.text));
        }
    }
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 7.0),
        egui::Align2::CENTER_CENTER,
        def.label,
        egui::FontId::proportional(theme::FONT_CAPTION),
        pal.text,
    );
    resp.on_hover_ui(|ui| tooltip(ui, s))
}

/// "CREATE" with a chevron: opens the list of every command of the group.
fn dropdown(
    ui: &mut Ui,
    name: &str,
    tools: &[(&'static Def, &'static CommandSpec)],
    width: f32,
    pal: &Palette,
) -> Option<Action> {
    let label = name.to_uppercase();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label.as_str()));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    if resp.hovered() || open {
        ui.painter().rect_filled(rect.shrink2(Vec2::new(4.0, 0.0)), theme::RADIUS_SM, pal.bg_hover);
    }
    let font = egui::FontId::proportional(theme::FONT_CAPTION);
    let galley = ui.painter().layout_no_wrap(label.clone(), font, pal.text_muted);
    let total = galley.size().x + 4.0 + 8.0;
    let x0 = rect.center().x - total / 2.0;
    ui.painter().galley(Pos2::new(x0, rect.center().y - galley.size().y / 2.0), galley, pal.text_muted);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(Pos2::new(x0 + total - 4.0, rect.center().y + 0.5), Vec2::splat(10.0)),
        icons::Icon::ChevronDown,
        pal.text_muted,
    );
    let mut chosen = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(230.0);
        ui.set_max_width(230.0);
        for (_, s) in tools {
            if menu_row(ui, s, pal).clicked() {
                chosen = Some(Action::Run(s.id));
            }
        }
    });
    chosen
}

/// One command in a dropdown: icon, title, shortcut.
fn menu_row(ui: &mut Ui, s: &CommandSpec, pal: &Palette) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, s.title));
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, theme::RADIUS_SM, pal.bg_hover);
    }
    if let Some(t) = icons::tool_for_command(s.id) {
        icons::paint_tool(
            painter,
            Rect::from_center_size(Pos2::new(rect.left() + 16.0, rect.center().y), Vec2::splat(18.0)),
            t,
            Ink::new(pal),
        );
    }
    painter.text(
        Pos2::new(rect.left() + 34.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        s.title,
        egui::FontId::proportional(theme::FONT_BODY),
        pal.text,
    );
    if let Some(k) = s.shortcut {
        painter.text(
            Pos2::new(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            k,
            egui::FontId::proportional(theme::FONT_CAPTION),
            pal.text_muted,
        );
    }
    resp.on_hover_ui(|ui| tooltip(ui, s))
}

/// "Finish sketch": the sketch workspace's prominent action.
fn finish_button(ui: &mut Ui, pal: &Palette) -> egui::Response {
    let label = tr("ribbon.finish_sketch");
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(BTN_W * 2.0, BTN_H + 20.0), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let rect = rect.shrink2(Vec2::new(2.0, 4.0));
    let fill = if resp.hovered() { pal.accent.gamma_multiply(0.9) } else { pal.accent };
    ui.painter().rect_filled(rect, theme::RADIUS_MD, fill);
    icons::paint_tool(
        ui.painter(),
        Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 8.0 + ICON / 2.0), Vec2::splat(ICON)),
        Tool::FinishSketch,
        Ink::mono(pal.accent_text),
    );
    ui.painter().text(
        Pos2::new(rect.center().x, rect.bottom() - 14.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(theme::FONT_BODY),
        pal.accent_text,
    );
    resp.on_hover_text(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ribbon_tool_is_a_registry_command_with_an_icon() {
        for sketch in [false, true] {
            for g in if sketch { SKETCH } else { SOLID } {
                for t in g.tools {
                    assert!(spec(t.id).is_some(), "ribbon tool {} is not in the command registry", t.id);
                    assert!(icons::tool_for_command(t.id).is_some(), "no icon for {}", t.id);
                }
            }
        }
    }

    #[test]
    fn tooltips_show_title_and_shortcut() {
        assert_eq!(spec("feature.extrude").map(tooltip_title).as_deref(), Some("Extrude (E)"));
        assert_eq!(spec("feature.sweep").map(tooltip_title).as_deref(), Some("Sweep"));
    }

    #[test]
    fn solid_tools_cover_the_requested_commands() {
        let ids = command_ids(false);
        for id in [
            "sketch.create",
            "feature.extrude",
            "feature.revolve",
            "feature.sweep",
            "feature.loft",
            "feature.hole",
            "generate.gear",
            "feature.fillet",
            "feature.chamfer",
            "feature.shell",
            "feature.combine",
            "feature.move_body",
            "feature.pattern_linear",
            "feature.pattern_circular",
            "feature.mirror",
            "feature.plane",
            "query.body",
            "import.file",
        ] {
            assert!(ids.contains(&id), "{id} missing from the Solid ribbon");
        }
    }
}
