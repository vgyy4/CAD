//! The application: one project window with a [`Session`] behind it (DESIGN.md §4).
//!
//! Every action goes through the command registry, so what the GUI can do, the CLI and
//! the MCP server can do too (ADR 0012). The UI is a plain function of [`App`] state,
//! which is what the kittest automation drives.

use crate::forms::{self, Form};
use crate::i18n::tr;
use crate::theme::{self, ThemeKind};
use crate::viewport::{self, Camera, DrawBody, DrawSketch, Hit};
use egui::{RichText, Ui};
use gusset_commands::{CmdCtx, CommandError, Session, registry};
use gusset_eval::{Evaluation, FeatureStatus, SketchStatus};
use gusset_history::{Actor, History, NodeId};
use gusset_kernel::{Kernel, TessellationParams};
use gusset_project::{OpenReport, Project};
use gusset_store::paths::DataDirs;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often unsaved work is written to the recovery slot.
pub const RECOVERY_INTERVAL: Duration = Duration::from_secs(30);

pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A modal dialog.
#[derive(Debug)]
pub enum Dialog {
    /// Close requested with unsaved changes.
    Close {
        save_failed: Option<String>,
    },
    /// A recovery copy newer than the saved file exists.
    Recovery {
        recovered: Box<History>,
    },
    /// Path entry for Save As / Open / Export (a native file picker comes with the
    /// portal/IFileDialog integration).
    Path {
        purpose: PathPurpose,
        text: String,
        error: Option<String>,
    },
    Message {
        title: String,
        body: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathPurpose {
    SaveAs,
    Open,
    ExportStep,
    ExportStl,
    Export3mf,
}

/// Status-bar message.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub text: String,
    pub hint: Option<String>,
    pub error: bool,
}

/// Cached drawable scene for one evaluated state.
struct Scene {
    key: (NodeId, Option<usize>),
    bodies: Vec<DrawBody>,
    sketches: Vec<DrawSketch>,
    eval: Arc<Evaluation>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App").field("theme", &self.theme).field("dirty", &self.session.dirty).finish_non_exhaustive()
    }
}

pub struct App {
    pub session: Session,
    pub project: Option<Project>,
    dirs: Option<DataDirs>,
    pub theme: ThemeKind,
    pub reduce_motion: bool,
    theme_applied: Option<ThemeKind>,
    pub camera: Camera,
    fit_pending: bool,
    pub selection: Vec<Hit>,
    pub form: Option<Form>,
    pub palette: Option<String>,
    pub code: Option<(String, Option<String>)>,
    pub dialog: Option<Dialog>,
    pub notice: Option<Notice>,
    /// Shown once when the real kernel is unavailable.
    pub kernel_note: Option<String>,
    scene: Option<Scene>,
    last_snapshot: Instant,
    /// Set when the window may close (after the close dialog).
    pub allow_close: bool,
    pub show_iterations: bool,
}

impl App {
    /// A new, empty project.
    pub fn new(kernel: Arc<dyn Kernel>, dirs: Option<DataDirs>) -> App {
        let session = Session::new(&gusset_doc::Document::default(), kernel, &CmdCtx::user(now_unix()));
        let project = dirs.clone().and_then(|d| Project::create(d).ok());
        App {
            session,
            project,
            dirs,
            theme: ThemeKind::Dark,
            reduce_motion: false,
            theme_applied: None,
            camera: Camera::default(),
            fit_pending: true,
            selection: Vec::new(),
            form: None,
            palette: None,
            code: None,
            dialog: None,
            notice: None,
            kernel_note: None,
            scene: None,
            last_snapshot: Instant::now(),
            allow_close: false,
            show_iterations: false,
        }
    }

    fn ctx() -> CmdCtx {
        CmdCtx::user(now_unix())
    }

    /// Runs a registry command as the user and reports the outcome in the status bar.
    pub fn run(&mut self, id: &str, args: Value) -> Result<Value, CommandError> {
        let r = gusset_commands::execute(&mut self.session, &Self::ctx(), id, args);
        match &r {
            Ok(_) => {
                let title = gusset_commands::find(id).map_or(id, |s| s.title);
                self.notice = Some(Notice { text: format!("{title}: done"), hint: None, error: false });
            }
            Err(e) => self.notice = Some(Notice { text: e.message.clone(), hint: e.hint.clone(), error: true }),
        }
        r
    }

    /// Opens the dialog for a command (or runs it directly when it takes no input).
    pub fn start_command(&mut self, id: &str) {
        let Some(spec) = gusset_commands::find(id) else { return };
        let mut form = Form::new(spec);
        if form.fields.is_empty() {
            let _ = self.run(spec.id, json!({}));
            return;
        }
        self.prefill(&mut form);
        self.form = Some(form);
    }

    /// Fills obvious fields from context: the latest sketch, selected edges and faces.
    fn prefill(&self, form: &mut Form) {
        let doc = self.session.document(&Actor::User).ok();
        let last_sketch = doc.as_ref().and_then(|d| {
            d.features
                .iter()
                .rev()
                .find(|f| matches!(f.kind, gusset_doc::FeatureKind::Sketch(_)))
                .map(|f| f.name.clone())
        });
        if let Some(s) = last_sketch {
            form.set("sketch", &s);
        }
        let edges: Vec<&str> = self
            .selection
            .iter()
            .filter_map(|h| if let Hit::Edge { name, .. } = h { Some(name.as_str()) } else { None })
            .collect();
        if !edges.is_empty() {
            form.set("edges", &edges.join(", "));
        }
        let face =
            self.selection.iter().find_map(|h| if let Hit::Face { name, .. } = h { Some(name.as_str()) } else { None });
        form.set("plane", face.unwrap_or("XY"));
    }

    pub fn apply_form(&mut self) {
        let Some(form) = self.form.as_mut() else { return };
        match form.to_args() {
            Ok(args) => {
                let id = form.command;
                match gusset_commands::execute(&mut self.session, &Self::ctx(), id, args) {
                    Ok(_) => {
                        let title = form.title;
                        self.form = None;
                        self.notice = Some(Notice { text: format!("{title}: done"), hint: None, error: false });
                    }
                    Err(e) => {
                        form.error = Some(match &e.hint {
                            Some(h) => format!("{} — {}: {h}", e.message, tr("error.fix")),
                            None => e.message.clone(),
                        });
                    }
                }
            }
            Err(msg) => form.error = Some(msg),
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.session.dirty
    }

    /// Saves to the current path; asks for one when the project was never saved.
    pub fn save(&mut self) -> bool {
        let Some(p) = self.project.as_mut() else {
            self.notice = Some(Notice { text: "No data folder: cannot save".into(), hint: None, error: true });
            return false;
        };
        if p.path().is_none() {
            self.dialog = Some(Dialog::Path { purpose: PathPurpose::SaveAs, text: "part.gus".into(), error: None });
            return false;
        }
        match p.save(self.session.history()) {
            Ok(()) => {
                self.session.dirty = false;
                self.notice = Some(Notice { text: tr("status.saved").into(), hint: None, error: false });
                true
            }
            Err(e) => {
                self.notice = Some(Notice { text: e.to_string(), hint: None, error: true });
                false
            }
        }
    }

    pub fn save_as(&mut self, path: PathBuf) -> Result<(), String> {
        let p = self.project.as_mut().ok_or("No data folder: cannot save")?;
        p.save_as(&path, self.session.history()).map_err(|e| e.to_string())?;
        self.session.project_dir = path.parent().map(PathBuf::from);
        self.session.dirty = false;
        self.notice = Some(Notice { text: tr("status.saved").into(), hint: None, error: false });
        Ok(())
    }

    pub fn open(&mut self, path: PathBuf) -> Result<(), String> {
        let dirs = self.dirs.clone().ok_or("No data folder")?;
        let (project, history, report) = Project::open(&path, dirs).map_err(|e| e.to_string())?;
        let kernel = self.session.kernel().clone();
        self.session = Session::from_history(history, kernel);
        self.session.project_dir = path.parent().map(PathBuf::from);
        if let Some(old) = self.project.replace(project) {
            old.close();
        }
        self.scene = None;
        self.selection.clear();
        self.fit_pending = true;
        match report {
            OpenReport::Clean => {}
            OpenReport::RecoveryAvailable { recovered, .. } => self.dialog = Some(Dialog::Recovery { recovered }),
            OpenReport::Damaged { reason, source } => {
                self.dialog = Some(Dialog::Message {
                    title: tr("damaged.title").into(),
                    body: format!("{reason}\nOpened instead: {source:?}. Save to repair the file."),
                });
                self.session.dirty = true;
            }
        }
        Ok(())
    }

    /// Writes a recovery snapshot if there are unsaved changes and the interval passed.
    pub fn maybe_snapshot(&mut self, force: bool) {
        if !self.session.dirty || (!force && self.last_snapshot.elapsed() < RECOVERY_INTERVAL) {
            return;
        }
        self.last_snapshot = Instant::now();
        if let Some(p) = self.project.as_mut()
            && let Err(e) = p.snapshot(self.session.history())
        {
            self.notice = Some(Notice { text: format!("Recovery copy failed: {e}"), hint: None, error: true });
        }
    }

    /// Rebuilds the drawable scene when the model changed.
    fn refresh_scene(&mut self) {
        let b = self.session.current_branch(&Actor::User);
        let h = self.session.history();
        let (Ok(head), Ok(br)) = (h.head(b), h.branch(b)) else { return };
        let key = (head, br.rollback);
        if self.scene.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        let eval = match self.session.evaluate(&Actor::User) {
            Ok(e) => e,
            Err(e) => {
                self.notice = Some(Notice { text: e.message, hint: e.hint, error: true });
                return;
            }
        };
        let kernel = self.session.kernel().clone();
        let tp = TessellationParams::default();
        let bodies = eval
            .bodies
            .iter()
            .filter_map(|body| {
                let mesh = kernel.tessellate(&body.shape, &tp).ok()?;
                Some(DrawBody {
                    name: body.name.clone(),
                    mesh,
                    face_names: body.names.faces.iter().map(ToString::to_string).collect(),
                    edge_names: body.names.edges.iter().map(ToString::to_string).collect(),
                })
            })
            .collect();
        let sketches = eval.sketches.iter().map(|(name, s)| sketch_drawing(name, s)).collect();
        self.scene = Some(Scene { key, bodies, sketches, eval });
        self.selection.clear();
    }

    pub fn evaluation(&mut self) -> Option<Arc<Evaluation>> {
        self.refresh_scene();
        self.scene.as_ref().map(|s| s.eval.clone())
    }

    // ------------------------------------------------------------------ UI

    /// Draws one frame.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.theme_applied != Some(self.theme) {
            theme::apply(&ctx, self.theme, self.reduce_motion);
            self.theme_applied = Some(self.theme);
        }
        self.handle_close(&ctx);
        self.shortcuts(&ctx);
        self.refresh_scene();
        self.maybe_snapshot(false);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::bottom("timeline").resizable(false).show(ui, |ui| self.timeline(ui));
        egui::Panel::left("browser").default_size(240.0).resizable(true).show(ui, |ui| self.browser(ui));
        if self.code.is_some() {
            egui::Panel::right("code").default_size(360.0).resizable(true).show(ui, |ui| self.code_view(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.viewport(ui));

        self.form_window(&ctx);
        self.palette_window(&ctx);
        self.dialogs(&ctx);
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.is_dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.dialog = Some(Dialog::Close { save_failed: None });
            } else if let Some(p) = self.project.take() {
                p.close();
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let sc = |m, k| KeyboardShortcut::new(m, k);
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::COMMAND, Key::K))) {
            self.palette = Some(String::new());
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::COMMAND, Key::S))) {
            self.save();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)))
            || ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Y)))
        {
            let _ = self.run("history.redo", json!({}));
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Z))) {
            let _ = self.run("history.undo", json!({}));
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::NONE, Key::Escape))) {
            self.form = None;
            self.palette = None;
            self.selection.clear();
        }
        // Single-key command shortcuts only when no text field has focus.
        if !ctx.egui_wants_keyboard_input() && self.form.is_none() && self.palette.is_none() {
            for spec in registry() {
                let Some(key) = spec.shortcut.and_then(Key::from_name) else { continue };
                if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::NONE, key))) {
                    self.start_command(spec.id);
                }
            }
        }
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tr("menu.file"), |ui| {
                if ui.button(tr("menu.open")).clicked() {
                    self.dialog = Some(Dialog::Path { purpose: PathPurpose::Open, text: String::new(), error: None });
                }
                if ui.button(tr("menu.save")).clicked() {
                    self.save();
                }
                if ui.button(tr("menu.save_as")).clicked() {
                    self.dialog =
                        Some(Dialog::Path { purpose: PathPurpose::SaveAs, text: "part.gus".into(), error: None });
                }
                ui.menu_button(tr("menu.export"), |ui| {
                    for (label, purpose, ext) in [
                        ("STEP…", PathPurpose::ExportStep, "step"),
                        ("STL…", PathPurpose::ExportStl, "stl"),
                        ("3MF…", PathPurpose::Export3mf, "3mf"),
                    ] {
                        if ui.button(label).clicked() {
                            self.dialog = Some(Dialog::Path { purpose, text: format!("part.{ext}"), error: None });
                        }
                    }
                });
                ui.separator();
                if ui.button(tr("menu.quit")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(tr("menu.edit"), |ui| {
                if ui.button(tr("menu.undo")).clicked() {
                    let _ = self.run("history.undo", json!({}));
                }
                if ui.button(tr("menu.redo")).clicked() {
                    let _ = self.run("history.redo", json!({}));
                }
            });
            ui.menu_button(tr("menu.view"), |ui| {
                if ui.radio(self.theme == ThemeKind::Dark, tr("menu.theme_dark")).clicked() {
                    self.theme = ThemeKind::Dark;
                }
                if ui.radio(self.theme == ThemeKind::Light, tr("menu.theme_light")).clicked() {
                    self.theme = ThemeKind::Light;
                }
                if ui.checkbox(&mut self.reduce_motion, "Reduce motion").changed() {
                    self.theme_applied = None;
                }
                if ui.button(tr("menu.code")).clicked() {
                    self.toggle_code();
                }
                if ui.button("Fit view").clicked() {
                    self.fit_pending = true;
                }
            });
            ui.menu_button(tr("menu.help"), |ui| {
                if ui.button(tr("menu.diagnostics")).clicked() {
                    self.dialog =
                        Some(Dialog::Message { title: tr("menu.diagnostics").into(), body: self.diagnostics() });
                }
            });
        });
    }

    pub fn diagnostics(&self) -> String {
        let hw = gusset_hw::Diagnostics::collect();
        let mut s = format!("Gusset {}\n{}\n", env!("CARGO_PKG_VERSION"), hw.to_text());
        if let Some(n) = &self.kernel_note {
            s.push_str(n);
            s.push('\n');
        }
        if let Some(p) = &self.project
            && let Some(r) = &p.risky
        {
            s.push_str(&format!("Storage warning: {r:?}\n"));
        }
        s
    }

    pub fn toggle_code(&mut self) {
        self.code = match self.code {
            Some(_) => None,
            None => {
                let text = self.session.document(&Actor::User).map(|d| d.to_text()).unwrap_or_default();
                Some((text, None))
            }
        };
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        use gusset_commands::Category;
        ui.horizontal_wrapped(|ui| {
            for (cat, label) in [
                (Category::Sketch, tr("tab.sketch")),
                (Category::Solid, tr("tab.solid")),
                (Category::Parameters, tr("browser.parameters")),
                (Category::Exchange, tr("menu.export")),
            ] {
                ui.label(RichText::new(label).small().weak());
                for spec in registry().iter().filter(|s| s.category == cat && s.flags.mutating) {
                    let b = ui.button(spec.title).on_hover_text(spec.description);
                    if b.clicked() {
                        self.start_command(spec.id);
                    }
                }
                ui.separator();
            }
            if ui.button(format!("🔍 {}", tr("search.hint"))).clicked() {
                self.palette = Some(String::new());
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let pal = theme::palette(self.theme);
            if self.is_dirty() {
                ui.colored_label(pal.warning, format!("● {}", tr("status.unsaved")));
            } else {
                ui.colored_label(pal.success, format!("✔ {}", tr("status.saved")));
            }
            if let Some(eval) = self.scene.as_ref().map(|s| &s.eval)
                && !eval.errors().is_empty()
            {
                ui.colored_label(pal.danger, format!("⚠ {} ({})", tr("status.rebuild_errors"), eval.errors().len()));
            }
            if self.session.audit_log().iter().any(|a| matches!(a.actor, Actor::Agent(_))) {
                ui.colored_label(pal.agent, format!("◆ {}", tr("status.agent")));
            }
            if let Some(n) = &self.notice {
                ui.separator();
                let col = if n.error { pal.danger } else { pal.text_muted };
                let r = ui.colored_label(col, &n.text);
                if let Some(h) = &n.hint {
                    r.on_hover_text(format!("{}: {h}", tr("error.fix")));
                }
            }
            if let Some(k) = &self.kernel_note {
                ui.separator();
                ui.colored_label(pal.warning, k);
            }
        });
    }

    fn browser(&mut self, ui: &mut Ui) {
        let doc = self.session.document(&Actor::User).ok().unwrap_or_default();
        let eval = self.scene.as_ref().map(|s| s.eval.clone());
        let pal = *theme::palette(self.theme);
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::CollapsingHeader::new(tr("browser.parameters")).default_open(true).show(ui, |ui| {
                for p in &doc.params {
                    let value = eval
                        .as_ref()
                        .and_then(|e| e.params.get(&p.name))
                        .map(|v| match v {
                            Ok(q) => q.to_string(),
                            Err(e) => format!("⚠ {e}"),
                        })
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.monospace(&p.name);
                        ui.label(format!("= {}", p.expr));
                        ui.weak(value);
                    });
                }
                if ui.small_button(format!("+ {}", tr("browser.add_param"))).clicked() {
                    self.start_command("param.set");
                }
            });
            egui::CollapsingHeader::new(tr("browser.features")).default_open(true).show(ui, |ui| {
                for f in &doc.features {
                    let status = eval.as_ref().and_then(|e| e.status(&f.name));
                    let (icon, col) = match status {
                        Some(FeatureStatus::Ok) | None => ("✔", pal.success),
                        Some(FeatureStatus::Failed(_)) => ("⚠", pal.danger),
                        Some(FeatureStatus::Suppressed | FeatureStatus::RolledBack) => ("◌", pal.text_muted),
                        Some(_) => ("…", pal.warning),
                    };
                    let mut text = format!("{} {}", f.kind.keyword(), f.name);
                    if let Some(l) = &f.label {
                        text.push_str(&format!(" — {l}"));
                    }
                    let r = ui.horizontal(|ui| {
                        ui.colored_label(col, icon);
                        ui.label(text)
                    });
                    if let Some(FeatureStatus::Failed(e)) = status {
                        let tip = match &e.hint {
                            Some(h) => format!("{}\n{}: {h}", e.message, tr("error.fix")),
                            None => e.message.clone(),
                        };
                        r.inner.on_hover_text(tip);
                    }
                    if let Some(sk) = eval.as_ref().and_then(|e| e.sketches.get(&f.name)) {
                        let (t, c) = match sk.status {
                            SketchStatus::WellConstrained => ("fully constrained".to_string(), pal.success),
                            SketchStatus::UnderConstrained => (format!("{} DOF free", sk.dof), pal.text_muted),
                            SketchStatus::OverConstrained => ("over-constrained".to_string(), pal.danger),
                            SketchStatus::Failed => ("solve failed".to_string(), pal.danger),
                        };
                        ui.indent(&f.name, |ui| ui.colored_label(c, RichText::new(t).small()));
                    }
                }
            });
            egui::CollapsingHeader::new(tr("browser.bodies")).default_open(true).show(ui, |ui| {
                for b in eval.iter().flat_map(|e| e.bodies.iter()) {
                    ui.label(&b.name);
                }
            });
            egui::CollapsingHeader::new("Branches").default_open(false).show(ui, |ui| {
                let cur = self.session.current_branch(&Actor::User);
                let branches: Vec<(gusset_history::BranchId, String, bool)> = self
                    .session
                    .history()
                    .branches()
                    .iter()
                    .map(|b| (b.id, b.name.clone(), matches!(b.created_by, Actor::Agent(_))))
                    .collect();
                for (id, name, agent) in branches {
                    let label =
                        if agent { RichText::new(format!("◆ {name}")).color(pal.agent) } else { RichText::new(name) };
                    if ui.selectable_label(id == cur, label).clicked() && id != cur {
                        let _ = self.session.switch_branch(&Actor::User, id);
                        self.scene = None;
                    }
                }
            });
        });
    }

    fn timeline(&mut self, ui: &mut Ui) {
        let doc = self.session.document(&Actor::User).ok().unwrap_or_default();
        let b = self.session.current_branch(&Actor::User);
        let rollback = self.session.history().branch(b).ok().and_then(|br| br.rollback);
        let n = doc.features.len();
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("timeline.title")).strong());
            if ui
                .small_button(if self.show_iterations { tr("timeline.collapse") } else { tr("timeline.expand") })
                .clicked()
            {
                self.show_iterations = !self.show_iterations;
            }
            let mut marker = rollback.unwrap_or(n);
            let r =
                ui.add(egui::Slider::new(&mut marker, 0..=n).show_value(false)).on_hover_text(tr("timeline.rollback"));
            if r.changed() {
                let m = if marker >= n { Value::Null } else { json!(marker) };
                let _ = self.run("timeline.set_rollback", json!({ "marker": m }));
            }
            egui::ScrollArea::horizontal().show(ui, |ui| {
                for (i, f) in doc.features.iter().enumerate() {
                    let active = rollback.is_none_or(|m| i < m);
                    let t = RichText::new(&f.name);
                    ui.label(if active { t } else { t.weak() });
                }
            });
        });
        if self.show_iterations {
            let h = self.session.history();
            let head = h.head(b).ok();
            ui.horizontal_wrapped(|ui| {
                for node in head.map(|n| h.lineage(n)).unwrap_or_default().iter().rev().take(30).rev() {
                    let label =
                        h.node(*node).map(|n| n.name.clone().unwrap_or_else(|| n.label.clone())).unwrap_or_default();
                    ui.label(RichText::new(format!("● {label}")).small());
                }
            });
        }
    }

    fn code_view(&mut self, ui: &mut Ui) {
        ui.label(RichText::new(tr("code.title")).heading());
        let mut apply = false;
        let mut revert = false;
        ui.horizontal(|ui| {
            apply = ui.button(tr("code.apply")).clicked();
            revert = ui.button(tr("code.revert")).clicked();
        });
        if let Some((text, err)) = self.code.as_mut() {
            if let Some(e) = err {
                ui.colored_label(ui.visuals().error_fg_color, e.as_str());
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(text).code_editor().desired_width(f32::INFINITY));
            });
        }
        if apply && let Some((text, _)) = self.code.clone() {
            match gusset_commands::execute(
                &mut self.session,
                &Self::ctx(),
                "document.set_text",
                json!({ "text": text }),
            ) {
                Ok(_) => {
                    if let Some(c) = self.code.as_mut() {
                        c.1 = None;
                    }
                }
                Err(e) => {
                    let loc = e
                        .details
                        .as_ref()
                        .map(|d| format!(" (line {}, column {})", d["line"], d["column"]))
                        .unwrap_or_default();
                    if let Some(c) = self.code.as_mut() {
                        c.1 = Some(format!("{}{loc}", e.message));
                    }
                }
            }
        }
        if revert {
            self.code = None;
            self.toggle_code();
        }
    }

    fn viewport(&mut self, ui: &mut Ui) {
        let pal = *theme::palette(self.theme);
        let (bodies, sketches) =
            self.scene.as_ref().map(|s| (s.bodies.as_slice(), s.sketches.as_slice())).unwrap_or_default();
        if self.fit_pending
            && let Some((lo, hi)) = viewport::bounds(bodies, sketches)
        {
            self.camera.fit(lo, hi);
            self.fit_pending = false;
        }
        let out = viewport::show(ui, &mut self.camera, &pal, bodies, sketches, &self.selection);
        if let Some(clicked) = out.clicked {
            let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
            match clicked {
                Some(h) if add => {
                    if let Some(pos) = self.selection.iter().position(|s| *s == h) {
                        self.selection.remove(pos);
                    } else {
                        self.selection.push(h);
                    }
                }
                Some(h) => self.selection = vec![h],
                None => self.selection.clear(),
            }
        }
        if bodies.is_empty() && sketches.is_empty() {
            self.empty_state(ui);
        }
    }

    fn empty_state(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let area = egui::Rect::from_center_size(rect.center(), egui::vec2(320.0, 180.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
            ui.vertical_centered(|ui| {
                ui.heading(tr("empty.title"));
                if ui.button(tr("empty.sketch")).clicked() {
                    self.start_command("sketch.create");
                }
                if ui.button(tr("empty.import")).clicked() {
                    self.start_command("import.file");
                }
                if ui.button(tr("empty.code")).clicked() {
                    self.toggle_code();
                }
                ui.weak(tr("empty.hint"));
            });
        });
    }

    fn form_window(&mut self, ctx: &egui::Context) {
        let Some(form) = self.form.as_mut() else { return };
        let mut open = true;
        let mut submit = false;
        egui::Window::new(form.title)
            .id(egui::Id::new("command_form"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 80.0))
            .show(ctx, |ui| submit = forms::show(ui, form));
        if submit {
            self.apply_form();
        }
        if !open {
            self.form = None;
        }
    }

    fn palette_window(&mut self, ctx: &egui::Context) {
        let Some(query) = self.palette.as_mut() else { return };
        let mut chosen = None;
        let mut close = false;
        egui::Modal::new(egui::Id::new("palette")).show(ctx, |ui| {
            ui.set_width(460.0);
            let r = ui.add(egui::TextEdit::singleline(query).hint_text(tr("search.hint")).desired_width(f32::INFINITY));
            r.request_focus();
            let matches = search(query);
            if matches.is_empty() {
                ui.weak(tr("search.none"));
            }
            for (i, spec) in matches.iter().take(12).enumerate() {
                let label = format!("{}  —  {}", spec.title, spec.id);
                if ui.selectable_label(i == 0, label).on_hover_text(spec.description).clicked() {
                    chosen = Some(spec.id);
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                chosen = matches.first().map(|s| s.id);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
        if close {
            self.palette = None;
        }
        if let Some(id) = chosen {
            self.palette = None;
            self.start_command(id);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else { return };
        let mut keep = true;
        let modal = egui::Modal::new(egui::Id::new("dialog"));
        match dialog {
            Dialog::Close { save_failed } => {
                let mut action = None;
                modal.show(ctx, |ui| {
                    ui.heading(tr("close.title"));
                    ui.label(tr("close.body"));
                    let name = self.project.as_ref().and_then(|p| p.path()).map_or_else(
                        || "Untitled".to_string(),
                        |p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    );
                    ui.label(format!("• {name}"));
                    if let Some(e) = &save_failed {
                        ui.colored_label(ui.visuals().error_fg_color, format!("{} {e}", tr("close.save_failed")));
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tr("close.save")).clicked() {
                            action = Some(0);
                        }
                        if ui.button(tr("close.discard")).clicked() {
                            action = Some(1);
                        }
                        if ui.button(tr("dialog.cancel")).clicked() {
                            action = Some(2);
                        }
                    });
                });
                match action {
                    Some(0) => {
                        if self.save() {
                            self.close_now(ctx);
                            keep = false;
                        } else if !matches!(self.dialog, Some(Dialog::Path { .. })) {
                            let msg = self.notice.as_ref().map(|n| n.text.clone());
                            self.dialog = Some(Dialog::Close { save_failed: msg });
                            return;
                        } else {
                            return; // Save As takes over.
                        }
                    }
                    Some(1) => {
                        if let Some(p) = self.project.as_ref() {
                            p.discard_recovery();
                        }
                        self.close_now(ctx);
                        keep = false;
                    }
                    Some(_) => keep = false,
                    None => self.dialog = Some(Dialog::Close { save_failed }),
                }
                if !keep {}
            }
            Dialog::Recovery { recovered } => {
                let mut action = None;
                modal.show(ctx, |ui| {
                    ui.heading(tr("recovery.title"));
                    ui.label(tr("recovery.body"));
                    ui.horizontal(|ui| {
                        if ui.button(tr("recovery.recover")).clicked() {
                            action = Some(0);
                        }
                        if ui.button(tr("recovery.open_saved")).clicked() {
                            action = Some(1);
                        }
                        if ui.button(tr("recovery.keep_both")).clicked() {
                            action = Some(2);
                        }
                    });
                });
                match action {
                    Some(0) => {
                        let kernel = self.session.kernel().clone();
                        let dir = self.session.project_dir.clone();
                        self.session = Session::from_history(*recovered, kernel);
                        self.session.project_dir = dir;
                        self.session.dirty = true;
                        self.scene = None;
                    }
                    Some(1) => {
                        if let Some(p) = self.project.as_ref() {
                            p.discard_recovery();
                        }
                    }
                    Some(_) => {
                        // Keep both: the recovered state becomes a branch next to the saved one.
                        if let Ok(doc) = recovered.head_document(recovered.main()) {
                            let ctx = Self::ctx();
                            let _ = gusset_commands::execute(
                                &mut self.session,
                                &ctx,
                                "branch.create",
                                json!({ "name": "Recovered", "switch": true }),
                            );
                            let _ = self.session.commit(&ctx, "Recovered work", &doc);
                            self.scene = None;
                        }
                    }
                    None => self.dialog = Some(Dialog::Recovery { recovered }),
                }
            }
            Dialog::Path { purpose, mut text, mut error } => {
                let mut submit = false;
                let mut cancel = false;
                modal.show(ctx, |ui| {
                    ui.heading(match purpose {
                        PathPurpose::SaveAs => tr("menu.save_as"),
                        PathPurpose::Open => tr("menu.open"),
                        _ => tr("menu.export"),
                    });
                    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(360.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit = true;
                    }
                    if let Some(e) = &error {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                    ui.horizontal(|ui| {
                        submit |= ui.button(tr("dialog.ok")).clicked();
                        cancel = ui.button(tr("dialog.cancel")).clicked();
                    });
                });
                if cancel {
                    return;
                }
                if submit {
                    match self.path_action(purpose, PathBuf::from(text.trim())) {
                        Ok(()) => return,
                        Err(e) => error = Some(e),
                    }
                }
                self.dialog = Some(Dialog::Path { purpose, text, error });
            }
            Dialog::Message { title, body } => {
                modal.show(ctx, |ui| {
                    ui.heading(&title);
                    ui.label(&body);
                    if ui.button(tr("dialog.ok")).clicked() {
                        keep = false;
                    }
                });
                if keep {
                    self.dialog = Some(Dialog::Message { title, body });
                }
            }
        }
    }

    fn path_action(&mut self, purpose: PathPurpose, path: PathBuf) -> Result<(), String> {
        if path.as_os_str().is_empty() {
            return Err("Enter a file path".into());
        }
        let abs = if path.is_absolute() {
            path
        } else {
            let base = self.session.project_dir.clone().or_else(|| std::env::current_dir().ok()).unwrap_or_default();
            base.join(path)
        };
        let export = |app: &mut App, id: &str, extra: Value| {
            let mut args = json!({ "path": abs.to_string_lossy(), "overwrite": true });
            if let (Some(a), Some(e)) = (args.as_object_mut(), extra.as_object()) {
                a.extend(e.clone());
            }
            app.run(id, args).map(|_| ()).map_err(|e| e.message)
        };
        match purpose {
            PathPurpose::SaveAs => self.save_as(abs),
            PathPurpose::Open => self.open(abs),
            PathPurpose::ExportStep => export(self, "export.step", json!({})),
            PathPurpose::ExportStl => export(self, "export.mesh", json!({ "format": "stl" })),
            PathPurpose::Export3mf => export(self, "export.mesh", json!({ "format": "3mf" })),
        }
    }

    fn close_now(&mut self, ctx: &egui::Context) {
        self.allow_close = true;
        if let Some(p) = self.project.take() {
            p.close();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// Builds polylines for a solved sketch on its plane.
fn sketch_drawing(name: &str, s: &gusset_eval::SketchResult) -> DrawSketch {
    let w = |p: [f64; 2]| {
        let v = s.frame.to_world(p);
        [v[0] as f32, v[1] as f32, v[2] as f32]
    };
    let pt = |id: &str| s.points.get(id).copied();
    let mut d = DrawSketch { name: name.to_string(), ..DrawSketch::default() };
    for (_, a, b) in &s.lines {
        if let (Some(a), Some(b)) = (pt(a), pt(b)) {
            d.polylines.push(vec![w(a), w(b)]);
        }
    }
    let arc = |c: [f64; 2], r: f64, a0: f64, a1: f64| -> Vec<[f32; 3]> {
        let n = 48;
        (0..=n)
            .map(|i| {
                let t = a0 + (a1 - a0) * f64::from(i) / f64::from(n);
                w([c[0] + r * t.cos(), c[1] + r * t.sin()])
            })
            .collect()
    };
    for (id, c) in &s.circles {
        if let (Some(c), Some(&r)) = (pt(c), s.radii.get(id)) {
            d.polylines.push(arc(c, r, 0.0, std::f64::consts::TAU));
        }
    }
    for (_, c, st, en) in &s.arcs {
        if let (Some(c), Some(st), Some(en)) = (pt(c), pt(st), pt(en)) {
            let r = (st[0] - c[0]).hypot(st[1] - c[1]);
            let a0 = (st[1] - c[1]).atan2(st[0] - c[0]);
            let mut a1 = (en[1] - c[1]).atan2(en[0] - c[0]);
            if a1 <= a0 {
                a1 += std::f64::consts::TAU;
            }
            d.polylines.push(arc(c, r, a0, a1));
        }
    }
    d.points = s.points.values().map(|&p| w(p)).collect();
    d
}

/// Command search: every query word must appear in the title, id or description;
/// title matches rank first.
pub fn search(query: &str) -> Vec<&'static gusset_commands::CommandSpec> {
    let q = query.to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    let mut hits: Vec<(u8, &'static gusset_commands::CommandSpec)> = registry()
        .iter()
        .filter_map(|s| {
            let title = s.title.to_lowercase();
            let hay = format!("{title} {} {}", s.id, s.description.to_lowercase());
            if !words.iter().all(|w| hay.contains(w)) {
                return None;
            }
            let rank = if words.iter().all(|w| title.contains(w)) { 0 } else { 1 };
            Some((rank, s))
        })
        .collect();
    hits.sort_by_key(|(r, _)| *r);
    hits.into_iter().map(|(_, s)| s).collect()
}
