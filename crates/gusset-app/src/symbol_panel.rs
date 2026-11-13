//! The symbol browser's panel (ADR 0057): folder, filter, list, preview and Place, drawn
//! over the state in [`crate::symbol_browser`].

use crate::i18n::tr;
use crate::symbol_browser::{Browser, MAX_SHOWN, Placement};

/// Draws the browser in the schematic panel. Returns what Place asks for (the Place
/// button, or a double click on a symbol).
pub fn panel(ui: &mut egui::Ui, b: &mut Browser) -> Option<Placement> {
    let mut placed = None;
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 60.0).max(40.0);
        crate::theme::text_field(
            ui,
            egui::TextEdit::singleline(&mut b.folder).hint_text(tr("sch.browser_folder_hint")).desired_width(w),
        );
        if ui.small_button(tr("sch.browser_reload")).clicked() {
            // The cache keeps unchanged files; changed ones are read again.
            b.rescan();
        }
    });
    let busy = b.poll();
    if busy {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    if b.folder.trim().is_empty() {
        return None;
    }
    let (handled, total, done) = b.progress();
    if !done {
        ui.weak(format!("{} {handled}/{total}", tr("sch.browser_loading")));
    } else if b.libraries() == 0 {
        ui.weak(tr("sch.browser_none"));
    }
    let errors = b.errors();
    if !errors.is_empty() {
        let tip: Vec<String> = errors.iter().take(20).map(|(f, why)| format!("{f}: {why}")).collect();
        ui.weak(format!("{}: {}", tr("sch.browser_skipped"), errors.len())).on_hover_text(tip.join("\n"));
    }
    crate::theme::text_field(
        ui,
        egui::TextEdit::singleline(&mut b.filter).hint_text(tr("sch.browser_filter_hint")).desired_width(f32::INFINITY),
    );
    let rows: Vec<(String, String, String)> =
        b.rows().map(|(l, s)| (l.file.clone(), s.name.clone(), format!("{}:{}", l.nickname(), s.name))).collect();
    let row_h = ui.text_style_height(&egui::TextStyle::Small) + 4.0;
    egui::ScrollArea::vertical().id_salt("sch_symbols").max_height(170.0).auto_shrink([false, true]).show_rows(
        ui,
        row_h,
        rows.len(),
        |ui, range| {
            for (file, name, shown) in rows.iter().skip(range.start).take(range.len()) {
                let on = b.picked.as_ref().is_some_and(|p| p.file == *file && p.name == *name);
                let r = ui.selectable_label(on, egui::RichText::new(shown).small());
                if r.clicked() || r.double_clicked() {
                    b.pick(file, name);
                }
                if r.double_clicked() {
                    placed = b.placement();
                }
            }
        },
    );
    if b.row_count() >= MAX_SHOWN {
        ui.weak(tr("sch.browser_more"));
    }
    if let Some(p) = b.picked.clone() {
        let desc = b
            .rows()
            .find(|(l, s)| l.file == p.file && s.name == p.name)
            .map(|(_, s)| s.description.clone())
            .unwrap_or_default();
        ui.strong(&p.name);
        if !desc.is_empty() {
            ui.weak(desc);
        }
        match b.preview() {
            None => {
                ui.spinner();
            }
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            Some(Ok(lib)) => {
                let lib = std::sync::Arc::clone(lib);
                let size = egui::vec2(ui.available_width(), 150.0);
                crate::sch_view::paint_preview(ui, &lib, b.unit, size);
                let units = lib.unit_count();
                if units > 1 {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tr("sch.browser_unit"));
                        for u in 1..=units.min(26) {
                            ui.selectable_value(&mut b.unit, u, u.to_string());
                        }
                    });
                }
            }
        }
        if ui.button(tr("sch.browser_place")).on_hover_text(tr("sch.tip_browser_place")).clicked() {
            placed = b.placement();
        }
    }
    placed
}
