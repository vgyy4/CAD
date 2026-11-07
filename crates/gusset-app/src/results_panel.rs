//! The results panel (DESIGN.md §2): a simulation or CAM result as a docked panel next to
//! the model instead of a text dialog. It lives until the model changes, then reads "out of
//! date" with a re-run button.

use crate::i18n::tr;
use crate::theme::{self, Palette};
use egui::{Color32, RichText, Ui};
use serde_json::Value;

/// Which command made the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Static,
    Modal,
    Cam,
}

impl Kind {
    /// The kind of a command id, if its result gets a panel.
    pub fn of(id: &str) -> Option<Kind> {
        match id {
            "sim.static" => Some(Kind::Static),
            "sim.modal" => Some(Kind::Modal),
            "cam.toolpath" => Some(Kind::Cam),
            _ => None,
        }
    }
}

/// One colour band of the legend.
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    pub rgb: u32,
    pub text: String,
}

/// A result being shown.
#[derive(Debug, Clone)]
pub struct Results {
    pub kind: Kind,
    pub title: String,
    /// The arguments the command was sent with (re-run, mode picker, G-code form).
    pub job: Value,
    /// The result without its bulky `stress_bands` / `shape_bands` / `path`.
    pub data: Value,
    pub legend: Vec<Band>,
    /// The model changed since: the numbers no longer describe it.
    pub stale: bool,
    /// Whether the toolpath is drawn in the viewport.
    pub path_visible: bool,
}

/// What the user asked for in the panel.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Frame the camera on this point (mm).
    Frame([f64; 3]),
    Rerun,
    /// Show this mode's shape (1-based).
    Mode(usize),
    ShowPath(bool),
    ExportGcode,
    Close,
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

/// A point `[x, y, z]` in mm.
pub fn point(v: &Value, key: &str) -> Option<[f64; 3]> {
    let a = v.get(key)?.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
}

fn fmt(x: f64) -> String {
    let t = format!("{x:.3}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn at_text(p: [f64; 3]) -> String {
    format!("({}, {}, {}) mm", fmt(p[0]), fmt(p[1]), fmt(p[2]))
}

/// The label/value rows of the numbers block.
pub fn rows(r: &Results) -> Vec<(String, String)> {
    let d = &r.data;
    let mut out = Vec::new();
    let mut add = |k: &str, v: String| out.push((k.to_string(), v));
    match r.kind {
        Kind::Static | Kind::Modal => {
            let method = d.get("method").and_then(Value::as_str).unwrap_or("voxel");
            add(tr("results.method"), method.to_string());
            if let Some(h) = num(d, "element_mm").or_else(|| num(d, "voxel_mm")) {
                add(tr("results.element"), format!("{} mm", fmt(h)));
            }
            if let Some(n) = d.get("elements").and_then(Value::as_u64) {
                add(tr("results.elements"), n.to_string());
            }
        }
        Kind::Cam => {
            if let Some(op) = r.job.get("operation").and_then(Value::as_str) {
                add(tr("results.operation"), op.to_string());
            }
            if let Some(t) = num(&r.job, "tool_diameter") {
                add(tr("results.tool"), format!("{} mm", fmt(t)));
            }
            if let Some(l) = d.get("levels_mm").and_then(Value::as_array) {
                let l: Vec<String> = l.iter().filter_map(Value::as_f64).map(fmt).collect();
                add(tr("results.levels"), format!("{} ({})", l.len(), l.join(", ")));
            }
            for (key, label) in
                [("cut_mm", "results.cut"), ("plunge_mm", "results.plunge"), ("rapid_mm", "results.rapid")]
            {
                if let Some(x) = num(d, key) {
                    add(tr(label), format!("{} mm", fmt(x)));
                }
            }
            if let Some(m) = num(d, "minutes") {
                add(tr("results.time"), format!("{} min", fmt(m)));
            }
        }
    }
    out
}

/// The notes to show in warning colour: warnings, thinned colour bands.
pub fn warnings(r: &Results) -> Vec<String> {
    let mut w = crate::app::result_notes(&r.data);
    if r.data.get("stress_bands_thinned") == Some(&Value::Bool(true)) {
        w.push(tr("results.thinned").to_string());
    }
    w
}

fn peak(ui: &mut Ui, out: &mut Vec<Action>, label: &str, value: String, at: Option<[f64; 3]>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("{label}: {value}"));
        if let Some(p) = at {
            ui.label(at_text(p));
            if ui.small_button(tr("results.frame")).on_hover_text(tr("results.frame_hint")).clicked() {
                out.push(Action::Frame(p));
            }
        }
    });
}

/// Draws the panel; returns what the user asked for.
pub fn show(ui: &mut Ui, r: &Results, pal: &Palette) -> Vec<Action> {
    let mut out = Vec::new();
    ui.horizontal(|ui| {
        ui.strong(&r.title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("✕").on_hover_text(tr("results.close")).clicked() {
                out.push(Action::Close);
            }
        });
    });
    if r.stale {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(pal.warning, tr("results.stale"));
            if ui.button(tr("results.rerun")).clicked() {
                out.push(Action::Rerun);
            }
        });
    }
    ui.add_space(theme::SPACE[2]);
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("results_rows").num_columns(2).show(ui, |ui| {
            for (k, v) in rows(r) {
                ui.label(RichText::new(k).color(pal.text_muted));
                ui.label(v);
                ui.end_row();
            }
        });
        let d = &r.data;
        if let Some(x) = num(d, "max_displacement_mm") {
            peak(ui, &mut out, tr("results.max_disp"), format!("{} mm", fmt(x)), point(d, "max_displacement_at_mm"));
        }
        if let Some(x) = num(d, "max_von_mises_mpa") {
            peak(ui, &mut out, tr("results.max_vm"), format!("{} MPa", fmt(x)), point(d, "max_von_mises_at_mm"));
        }
        if let Some(f) = d.get("frequencies_hz").and_then(Value::as_array) {
            ui.add_space(theme::SPACE[3]);
            ui.strong(tr("results.modes"));
            let shown = r.job.get("shape").and_then(Value::as_u64).unwrap_or(1) as usize;
            for (i, hz) in f.iter().filter_map(Value::as_f64).enumerate() {
                let k = i + 1;
                let label = format!("{k}: {} Hz", fmt(hz));
                if ui.selectable_label(k == shown, label).on_hover_text(tr("results.mode_hint")).clicked() && k != shown
                {
                    out.push(Action::Mode(k));
                }
            }
        }
        if r.kind == Kind::Cam {
            ui.add_space(theme::SPACE[3]);
            let mut vis = r.path_visible;
            if ui.checkbox(&mut vis, tr("results.show_path")).changed() {
                out.push(Action::ShowPath(vis));
            }
            if ui.button(tr("results.export_gcode")).clicked() {
                out.push(Action::ExportGcode);
            }
        }
        let w = warnings(r);
        if !w.is_empty() {
            ui.add_space(theme::SPACE[3]);
            ui.strong(tr("results.warnings"));
            for n in w {
                ui.colored_label(pal.warning, n);
            }
        }
        if !r.legend.is_empty() {
            ui.add_space(theme::SPACE[3]);
            ui.strong(tr("results.legend"));
            for b in &r.legend {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    let c = Color32::from_rgb((b.rgb >> 16) as u8, (b.rgb >> 8) as u8, b.rgb as u8);
                    ui.painter().rect_filled(rect, 2.0, c);
                    ui.label(&b.text);
                });
            }
        }
    });
    out
}
