//! The browser tree (DESIGN.md §2): collapsible categories with icons. Document settings
//! (units), Origin (the base planes and the origin), Bodies, Sketches, Features, Construction
//! (datum planes), Joints, Parameters and Branches.
//!
//! The widgets only report what the user chose ([`Action`]); the app applies it. Origin,
//! bodies and datum planes have no visibility in the scene yet, so their rows are read-only
//! (no eye); sketches keep their eye toggle, their DOF line and "double-click edits".

use crate::agents::plain;
use crate::i18n::tr;
use crate::icons::{self, Icon, Ink, Tool};
use crate::link::BranchInfo;
use crate::theme::{self, Palette};
use egui::{Color32, FontId, Pos2, Rect, Sense, Ui, Vec2, WidgetInfo, WidgetType};
use gusset_doc::{Document, Feature, FeatureKind};
use gusset_eval::{Evaluation, FeatureStatus, SketchStatus};
use gusset_history::BranchId;
use std::collections::BTreeMap;

/// A browser category, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Document,
    Origin,
    Bodies,
    Sketches,
    /// The remaining features (extrude, fillet, ...): the timeline lists them too.
    Features,
    Construction,
    /// Joints placing bodies (ADR 0026).
    Joints,
    Parameters,
    Branches,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::Document,
        Category::Origin,
        Category::Bodies,
        Category::Sketches,
        Category::Features,
        Category::Construction,
        Category::Joints,
        Category::Parameters,
        Category::Branches,
    ];

    /// The name shown (and the header's accessible label).
    pub fn title(self) -> &'static str {
        tr(match self {
            Category::Document => "browser.document",
            Category::Origin => "browser.origin",
            Category::Bodies => "browser.bodies",
            Category::Sketches => "browser.sketches",
            Category::Features => "browser.features",
            Category::Construction => "browser.construction",
            Category::Joints => "browser.joints",
            Category::Parameters => "browser.parameters",
            Category::Branches => "browser.branches",
        })
    }

    pub fn tool(self) -> Tool {
        match self {
            Category::Document => Tool::Settings,
            Category::Origin => Tool::Origin,
            Category::Bodies => Tool::Body,
            Category::Sketches => Tool::Sketch,
            Category::Features => Tool::Extrude,
            Category::Construction => Tool::Plane,
            Category::Joints => Tool::Joint,
            Category::Parameters => Tool::Params,
            Category::Branches => Tool::Branch,
        }
    }

    /// Whether the category starts expanded: the working lists do, the reference ones don't.
    pub fn default_open(self) -> bool {
        !matches!(self, Category::Document | Category::Origin | Category::Branches)
    }

    /// Where a document feature is listed.
    pub fn of(kind: &FeatureKind) -> Category {
        match kind {
            FeatureKind::Sketch(_) => Category::Sketches,
            FeatureKind::Plane(_) => Category::Construction,
            FeatureKind::Joint(_) => Category::Joints,
            _ => Category::Features,
        }
    }
}

/// Which categories the user expanded or collapsed (the rest use their default).
#[derive(Debug, Clone, Default)]
pub struct State {
    open: BTreeMap<Category, bool>,
}

impl State {
    pub fn is_open(&self, c: Category) -> bool {
        self.open.get(&c).copied().unwrap_or_else(|| c.default_open())
    }

    pub fn set_open(&mut self, c: Category, open: bool) {
        self.open.insert(c, open);
    }

    pub fn toggle(&mut self, c: Category) {
        self.set_open(c, !self.is_open(c));
    }
}

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The eye of a sketch: show or hide it.
    SetSketchVisible(String, bool),
    /// Double-click on a sketch.
    EditSketch(String),
    /// "+ Add parameter".
    AddParam,
    /// Double-click on a parameter.
    EditParam(String),
    /// A customizer slider was released: set the parameter to this expression.
    SetParam(String, String),
    /// "Apply" on a configuration.
    ApplyConfiguration(String),
    /// Click on another branch.
    SwitchBranch(BranchId),
}

/// What the browser draws.
pub struct View<'a> {
    pub pal: &'a Palette,
    pub doc: &'a Document,
    pub eval: Option<&'a Evaluation>,
    /// Whether a sketch is drawn in the viewport.
    pub sketch_visible: &'a dyn Fn(&str) -> bool,
    pub branch: Option<BranchId>,
    pub branches: &'a [BranchInfo],
}

impl std::fmt::Debug for View<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("View").field("features", &self.doc.features.len()).finish_non_exhaustive()
    }
}

const ROW_H: f32 = 22.0;
const ICON: f32 = 16.0;
const INDENT: f32 = 14.0;

/// The features of `doc` that `category` lists, in history order.
pub fn features_of(doc: &Document, category: Category) -> Vec<&Feature> {
    doc.features.iter().filter(|f| Category::of(&f.kind) == category).collect()
}

/// Number of entries in a category (shown next to its name).
pub fn count(v: &View<'_>, c: Category) -> usize {
    match c {
        Category::Document => 1,
        Category::Origin => ORIGIN.len(),
        Category::Bodies => v.eval.map_or(0, |e| e.bodies.len()),
        Category::Parameters => v.doc.params.len(),
        Category::Branches => v.branches.len(),
        Category::Sketches | Category::Features | Category::Construction | Category::Joints => {
            features_of(v.doc, c).len()
        }
    }
}

/// The fixed origin entries: (label, icon).
const ORIGIN: [(&str, Tool); 4] =
    [("Origin point", Tool::Origin), ("XY plane", Tool::Plane), ("XZ plane", Tool::Plane), ("YZ plane", Tool::Plane)];

/// Draws the whole tree and returns what was chosen.
pub fn show(ui: &mut Ui, state: &mut State, v: &View<'_>) -> Vec<Action> {
    let mut out = Vec::new();
    ui.spacing_mut().item_spacing.y = 0.0;
    for c in Category::ALL {
        if header(ui, v.pal, c, count(v, c), state.is_open(c)).clicked() {
            state.toggle(c);
        }
        if !state.is_open(c) {
            continue;
        }
        match c {
            Category::Document => document(ui, v),
            Category::Origin => origin(ui, v),
            Category::Bodies => bodies(ui, v),
            Category::Sketches => sketches(ui, v, &mut out),
            Category::Features | Category::Construction | Category::Joints => features(ui, v, c),
            Category::Parameters => parameters(ui, v, &mut out),
            Category::Branches => branches(ui, v, &mut out),
        }
        ui.add_space(theme::SPACE[2]);
    }
    out
}

/// A category header: disclosure arrow, icon, name and entry count.
fn header(ui: &mut Ui, pal: &Palette, c: Category, n: usize, open: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H + 2.0), Sense::click());
    let title = c.title();
    resp.widget_info(|| WidgetInfo::selected(WidgetType::CollapsingHeader, true, open, title));
    let p = ui.painter();
    p.rect_filled(rect, theme::RADIUS_SM, if resp.hovered() { pal.bg_hover } else { pal.bg_raised });
    let arrow = Rect::from_center_size(Pos2::new(rect.left() + 11.0, rect.center().y), Vec2::splat(12.0));
    icons::paint(p, arrow, if open { Icon::ChevronDown } else { Icon::ChevronRight }, pal.text_muted);
    let ic = Rect::from_center_size(Pos2::new(rect.left() + 30.0, rect.center().y), Vec2::splat(ICON));
    icons::paint_tool(p, ic, c.tool(), Ink::new(pal));
    let font = FontId::proportional(theme::FONT_BODY);
    let g = p.layout_no_wrap(title.to_string(), font.clone(), pal.text);
    p.galley(Pos2::new(rect.left() + 44.0, rect.center().y - g.size().y / 2.0), g, pal.text);
    let g = p.layout_no_wrap(n.to_string(), font, pal.text_muted);
    p.galley(Pos2::new(rect.right() - 8.0 - g.size().x, rect.center().y - g.size().y / 2.0), g, pal.text_muted);
    resp
}

/// One row of a category.
struct Row {
    tool: Tool,
    /// Extra left indent in px.
    indent: f32,
    text: String,
    /// Muted text after the name.
    suffix: String,
    /// Accessible name (defaults to `text`).
    access: String,
    mono: bool,
    dimmed: bool,
    /// Text color override (danger, agent).
    color: Option<Color32>,
    /// A status icon at the right edge.
    status: Option<(Icon, Color32)>,
    clickable: bool,
    /// For rows that are a choice among siblings.
    selected: Option<bool>,
}

impl Row {
    fn new(tool: Tool, text: impl Into<String>) -> Row {
        let text = text.into();
        Row {
            tool,
            indent: 0.0,
            access: text.clone(),
            text,
            suffix: String::new(),
            mono: false,
            dimmed: false,
            color: None,
            status: None,
            clickable: false,
            selected: None,
        }
    }
}

/// Paints a row into the space that is left on the line.
fn row(ui: &mut Ui, pal: &Palette, r: &Row) -> egui::Response {
    let sense = if r.clickable { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width().max(40.0), ROW_H), sense);
    resp.widget_info(|| match r.selected {
        Some(on) => WidgetInfo::selected(WidgetType::SelectableLabel, true, on, &r.access),
        None if r.clickable => WidgetInfo::labeled(WidgetType::Button, true, &r.access),
        None => WidgetInfo::labeled(WidgetType::Label, true, &r.access),
    });
    let p = ui.painter_at(rect);
    if r.selected == Some(true) {
        p.rect_filled(rect, theme::RADIUS_SM, pal.accent.gamma_multiply(0.22));
    } else if r.clickable && resp.hovered() {
        p.rect_filled(rect, theme::RADIUS_SM, pal.bg_hover);
    }
    let ink = if r.dimmed { Ink::disabled(pal) } else { Ink::new(pal) };
    let x = rect.left() + r.indent;
    let ic = Rect::from_center_size(Pos2::new(x + ICON / 2.0 + 2.0, rect.center().y), Vec2::splat(ICON));
    icons::paint_tool(&p, ic, r.tool, ink);
    let color = r.color.unwrap_or(if r.dimmed { pal.text_muted } else { pal.text });
    let font = if r.mono { FontId::monospace(theme::FONT_BODY) } else { FontId::proportional(theme::FONT_BODY) };
    let right = rect.right() - if r.status.is_some() { ICON + 6.0 } else { 4.0 };
    let g = p.layout_no_wrap(r.text.clone(), font, color);
    let tx = x + ICON + 8.0;
    p.galley(Pos2::new(tx, rect.center().y - g.size().y / 2.0), g.clone(), color);
    if !r.suffix.is_empty() && tx + g.size().x + 6.0 < right {
        let s = p.layout_no_wrap(r.suffix.clone(), FontId::proportional(theme::FONT_CAPTION), pal.text_muted);
        p.galley(Pos2::new(tx + g.size().x + 6.0, rect.center().y - s.size().y / 2.0), s, pal.text_muted);
    }
    if let Some((icon, col)) = r.status {
        let sr = Rect::from_center_size(Pos2::new(rect.right() - ICON / 2.0 - 4.0, rect.center().y), Vec2::splat(12.0));
        icons::paint(&p, sr, icon, col);
    }
    resp
}

fn document(ui: &mut Ui, v: &View<'_>) {
    let mut r = Row::new(Tool::Settings, format!("{}: {}", tr("browser.units"), v.doc.units.unit().symbol));
    r.indent = INDENT;
    row(ui, v.pal, &r).on_hover_text(tr("browser.units_help"));
}

fn origin(ui: &mut Ui, v: &View<'_>) {
    for (label, tool) in ORIGIN {
        let mut r = Row::new(tool, label);
        r.indent = INDENT;
        row(ui, v.pal, &r).on_hover_text(tr("browser.read_only"));
    }
}

fn bodies(ui: &mut Ui, v: &View<'_>) {
    for b in v.eval.iter().flat_map(|e| e.bodies.iter()) {
        let mut r = Row::new(Tool::Body, &b.name);
        r.indent = INDENT;
        row(ui, v.pal, &r);
    }
}

/// The icon and color of a feature's status.
pub fn status_icon(status: Option<&FeatureStatus>, pal: &Palette) -> (Icon, Color32) {
    match status {
        Some(FeatureStatus::Ok) | None => (Icon::Check, pal.success),
        Some(FeatureStatus::Failed(_)) => (Icon::Warning, pal.danger),
        Some(FeatureStatus::Suppressed | FeatureStatus::RolledBack) => (Icon::Ring, pal.text_muted),
        Some(_) => (Icon::Pending, pal.warning),
    }
}

fn feature_row(f: &Feature, status: Option<&FeatureStatus>, pal: &Palette) -> Row {
    let kw = f.kind.keyword();
    let mut text = f.name.clone();
    if let Some(l) = &f.label {
        text.push_str(&format!(" - {}", plain(l, 60)));
    }
    let mut r = Row::new(icons::tool_for_keyword(kw), text);
    r.access = format!("{kw} {}", f.name);
    r.suffix = kw.replace('_', " ");
    r.status = Some(status_icon(status, pal));
    r.dimmed = f.suppressed || matches!(status, Some(FeatureStatus::Suppressed | FeatureStatus::RolledBack));
    r.indent = INDENT;
    r
}

fn failure_tip(status: Option<&FeatureStatus>) -> Option<String> {
    match status {
        Some(FeatureStatus::Failed(e)) => Some(match &e.hint {
            Some(h) => format!("{}\n{}: {h}", e.message, tr("error.fix")),
            None => e.message.clone(),
        }),
        _ => None,
    }
}

fn sketches(ui: &mut Ui, v: &View<'_>, out: &mut Vec<Action>) {
    for f in features_of(v.doc, Category::Sketches) {
        let status = v.eval.and_then(|e| e.status(&f.name));
        let shown = (v.sketch_visible)(&f.name);
        let mut r = feature_row(f, status, v.pal);
        r.clickable = true;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(theme::SPACE[1]);
            let label = format!("{} {}", tr("browser.show_sketch"), f.name);
            if icons::toggle(ui, shown, Icon::Eye, Icon::EyeOff, &label)
                .on_hover_text(tr("browser.show_sketch_help"))
                .clicked()
            {
                out.push(Action::SetSketchVisible(f.name.clone(), !shown));
            }
            r.indent = 0.0;
            let mut resp = row(ui, v.pal, &r).on_hover_text(tr("browser.edit_sketch"));
            if let Some(tip) = failure_tip(status) {
                resp = resp.on_hover_text(tip);
            }
            if resp.double_clicked() {
                out.push(Action::EditSketch(f.name.clone()));
            }
        });
        if let Some(sk) = v.eval.and_then(|e| e.sketches.get(&f.name)) {
            let (t, c) = match sk.status {
                SketchStatus::WellConstrained => ("fully constrained".to_string(), v.pal.success),
                SketchStatus::UnderConstrained => (format!("{} DOF free", sk.dof), v.pal.text_muted),
                SketchStatus::OverConstrained => ("over-constrained".to_string(), v.pal.danger),
                SketchStatus::Failed => ("solve failed".to_string(), v.pal.danger),
            };
            ui.horizontal(|ui| {
                ui.add_space(INDENT + ICON + 12.0);
                ui.colored_label(c, egui::RichText::new(t).small());
            });
        }
    }
}

/// Features and datum planes: read-only rows with their status and failure tooltip.
fn features(ui: &mut Ui, v: &View<'_>, c: Category) {
    for f in features_of(v.doc, c) {
        let status = v.eval.and_then(|e| e.status(&f.name));
        let resp = row(ui, v.pal, &feature_row(f, status, v.pal));
        if let Some(tip) = failure_tip(status) {
            resp.on_hover_text(tip);
        }
    }
}

fn parameters(ui: &mut Ui, v: &View<'_>, out: &mut Vec<Action>) {
    for p in &v.doc.params {
        let value = v
            .eval
            .and_then(|e| e.params.get(&p.name))
            .map(|q| match q {
                Ok(q) => q.to_string(),
                Err(e) => format!("error: {e}"),
            })
            .unwrap_or_default();
        let mut r = Row::new(Tool::Params, &p.name);
        r.mono = true;
        r.indent = INDENT;
        r.suffix = format!("= {}   {value}", p.expr);
        r.access = format!("{} = {} {value}", p.name, p.expr);
        r.clickable = true;
        if row(ui, v.pal, &r).on_hover_text(tr("browser.edit_param")).double_clicked() {
            out.push(Action::EditParam(p.name.clone()));
        }
        if let Some(s) = slider_of(p, v) {
            customizer(ui, p, s, out);
        }
    }
    // Configurations (ADR 0027 §3): named value sets, applied as one undoable edit.
    for c in &v.doc.configurations {
        let current = c.set.iter().all(|(n, e)| v.doc.params.iter().any(|p| p.name == *n && p.expr == *e));
        ui.horizontal(|ui| {
            ui.add_space(INDENT);
            let mut r = Row::new(Tool::Params, &c.name);
            r.selected = Some(current);
            r.suffix = tr("browser.configuration").into();
            r.access = format!("{} {}", tr("browser.configuration"), c.name);
            // The button first: a row takes the rest of the width.
            if !current && ui.small_button(tr("browser.apply_configuration")).clicked() {
                out.push(Action::ApplyConfiguration(c.name.clone()));
            }
            row(ui, v.pal, &r);
        });
    }
    ui.horizontal(|ui| {
        ui.add_space(INDENT);
        if ui.small_button(format!("+ {}", tr("browser.add_param"))).clicked() {
            out.push(Action::AddParam);
        }
    });
}

/// A parameter's customizer slider: bounds, step and current value in display units.
struct Slider {
    lo: f64,
    hi: f64,
    step: Option<f64>,
    value: f64,
    unit: &'static str,
}

/// The slider of `p` when its `min` and `max` evaluate (ADR 0027): lengths in the
/// document's unit, angles in degrees.
fn slider_of(p: &gusset_units::params::Param, v: &View<'_>) -> Option<Slider> {
    let eval = v.eval?;
    let value = eval.params.get(&p.name)?.as_ref().ok().copied()?;
    let scope = |n: &str| eval.params.get(n).and_then(|r| r.as_ref().ok().copied());
    // The same bounds the range check uses (review v0.3 M4).
    let [min, max, step] = gusset_units::params::range_bounds(p, value, &scope, v.doc.units).ok()?;
    let bound = |b: Option<gusset_units::Quantity>| b.map(|b| b.value);
    let (factor, unit) = if value.dim == gusset_units::Dim::LENGTH {
        let u = v.doc.units.unit();
        (u.factor, u.symbol)
    } else if value.dim == gusset_units::Dim::ANGLE {
        (std::f64::consts::PI / 180.0, "deg")
    } else {
        (1.0, "")
    };
    let (lo, hi) = (bound(min)? / factor, bound(max)? / factor);
    (lo < hi).then_some(Slider { lo, hi, step: bound(step).map(|s| s / factor), value: value.value / factor, unit })
}

fn customizer(ui: &mut Ui, p: &gusset_units::params::Param, s: Slider, out: &mut Vec<Action>) {
    let id = ui.id().with(("customizer", &p.name));
    // While dragging, the slider shows its own value; the model changes on release.
    let mut x = ui.data(|d| d.get_temp::<f64>(id)).unwrap_or(s.value);
    ui.horizontal(|ui| {
        ui.add_space(INDENT * 2.0);
        let mut slider = egui::Slider::new(&mut x, s.lo..=s.hi).suffix(format!(" {}", s.unit).trim_end().to_string());
        if let Some(step) = s.step.filter(|st| *st > 0.0) {
            slider = slider.step_by(step);
        }
        let r = ui.add(slider).on_hover_text(format!("{} ({})", p.name, tr("browser.customizer")));
        if r.dragged() {
            ui.data_mut(|d| d.insert_temp(id, x));
        } else {
            ui.data_mut(|d| d.remove::<f64>(id));
        }
        if (r.drag_stopped() || (r.changed() && !r.dragged())) && (x - s.value).abs() > 1e-12 {
            let expr = if s.unit.is_empty() { format!("{x}") } else { format!("{x} {}", s.unit) };
            out.push(Action::SetParam(p.name.clone(), expr));
        }
    });
}

fn branches(ui: &mut Ui, v: &View<'_>, out: &mut Vec<Action>) {
    for b in v.branches {
        let current = Some(b.id) == v.branch;
        let mut r = Row::new(Tool::Branch, plain(&b.name, 80));
        r.indent = INDENT;
        r.selected = Some(current);
        r.clickable = true;
        if b.agent.is_some() {
            r.color = Some(v.pal.agent);
        }
        if row(ui, v.pal, &r).clicked() && !current {
            out.push(Action::SwitchBranch(b.id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_are_sorted_into_categories() {
        let doc = Document::from_text(
            "gusset 1\nunits mm\n\nsketch S (plane: XY, next_id: 1) {\n}\n\nplane P (base: XY, offset: 5 mm)\n",
        )
        .ok();
        let Some(doc) = doc else { return };
        assert_eq!(features_of(&doc, Category::Sketches).len(), 1);
        assert_eq!(features_of(&doc, Category::Construction).len(), 1);
        assert!(features_of(&doc, Category::Features).is_empty());
    }

    #[test]
    fn categories_are_complete_and_open_state_toggles() {
        let titles: Vec<&str> = Category::ALL.iter().map(|c| c.title()).collect();
        assert_eq!(
            titles,
            [
                "Document settings",
                "Origin",
                "Bodies",
                "Sketches",
                "Features",
                "Construction",
                "Joints",
                "Parameters",
                "Branches"
            ]
        );
        let mut s = State::default();
        assert!(s.is_open(Category::Sketches) && !s.is_open(Category::Origin));
        s.toggle(Category::Origin);
        s.toggle(Category::Sketches);
        assert!(s.is_open(Category::Origin) && !s.is_open(Category::Sketches));
    }
}
