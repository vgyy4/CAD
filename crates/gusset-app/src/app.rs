//! The application: one project window in front of the model thread (DESIGN.md §4).
//!
//! The [`Session`] lives on the `gusset-host` model thread, shared with the MCP server,
//! so the user and AI agents work on the same open project (ADR 0012). Every action goes
//! through the command registry as a detached host job; results come back over a channel
//! and the UI never waits for a long command. What the UI draws is a [`Snapshot`] read
//! with a short, non-blocking `try_with_session` whenever the host reports a change. The
//! UI is a plain function of [`App`] state, which is what the kittest automation drives.

use crate::agents::{Agents, plain};
use crate::forms::{self, Form};
use crate::i18n::tr;
use crate::link::{self, Snapshot, Waker};
use crate::theme::{self, ThemeKind};
use crate::viewport::{self, Camera, DrawBody, DrawSketch, Hit};
use egui::{RichText, Ui};
use gusset_commands::{CmdCtx, CommandError, Session, registry};
use gusset_eval::{Evaluation, FeatureStatus, SketchStatus};
use gusset_history::{Actor, History, NodeId};
use gusset_host::{Host, HostEvent, HostHandle};
use gusset_kernel::{Kernel, TessellationParams};
use gusset_project::{OpenReport, Project};
use gusset_store::paths::DataDirs;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// How often unsaved work is written to the recovery slot.
pub const RECOVERY_INTERVAL: Duration = Duration::from_secs(30);

/// How soon to retry a snapshot read while a long command holds the session.
const BUSY_RETRY: Duration = Duration::from_millis(50);

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

/// What to do with the result of a command the GUI sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Then {
    /// Report it in the status bar.
    Notice,
    /// It came from the open form: close the form, or show the error in it.
    Form,
    /// It came from the code view: clear or show the parse error there.
    Code,
    /// Only report errors.
    Quiet,
    /// A path dialog action: re-open the dialog with the error on failure.
    PathDialog(PathPurpose, String),
    /// A branch comparison for the agents panel (branch name, plain text).
    Compare(String),
}

/// What to do after a save finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterSave {
    Nothing,
    /// "Save and close" from the close dialog.
    Close,
}

/// Results of host jobs, delivered to the UI thread.
pub(crate) enum Done {
    Command {
        id: String,
        then: Then,
        result: Result<Value, CommandError>,
    },
    Scene(Result<Result<SceneData, CommandError>, CommandError>),
    Saved {
        result: Result<(), String>,
        then: AfterSave,
    },
    SavedAs {
        path: PathBuf,
        text: String,
        result: Result<(), String>,
    },
    Recovery(Result<Option<u64>, String>),
    /// A job whose result needs no handling beyond refreshing the snapshot.
    Refresh(Result<(), CommandError>),
}

/// Drawable scene computed on the model thread.
pub(crate) struct SceneData {
    key: (NodeId, Option<usize>),
    bodies: Vec<DrawBody>,
    sketches: Vec<DrawSketch>,
    eval: Arc<Evaluation>,
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
        f.debug_struct("App").field("theme", &self.theme).field("dirty", &self.snapshot.dirty).finish_non_exhaustive()
    }
}

pub struct App {
    host: HostHandle,
    kernel: Arc<dyn Kernel>,
    /// This frame's view of the model (refreshed when the host reports a change).
    pub snapshot: Snapshot,
    stale: bool,
    events: broadcast::Receiver<HostEvent>,
    done_tx: mpsc::Sender<Done>,
    done_rx: mpsc::Receiver<Done>,
    /// Host jobs sent and not yet answered.
    in_flight: usize,
    pub(crate) waker: Arc<Waker>,
    /// Number of change events from agents seen (for the status bar and tests).
    pub agent_changes: u64,
    has_project: bool,
    project_path: Option<PathBuf>,
    project_risky: Option<String>,
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
    scene_requested: Option<(NodeId, Option<usize>)>,
    last_snapshot: Instant,
    close_after_save: bool,
    /// Set when the window may close (after the close dialog).
    pub allow_close: bool,
    pub show_iterations: bool,
    /// The MCP server, its HTTP endpoint, approvals and the "AI agents" panel.
    pub agents: Agents,
}

impl App {
    /// A new, empty project.
    pub fn new(kernel: Arc<dyn Kernel>, dirs: Option<DataDirs>) -> App {
        let session = Session::new(&gusset_doc::Document::default(), kernel.clone(), &CmdCtx::user(now_unix()));
        let project = dirs.clone().and_then(|d| Project::create(d).ok());
        let has_project = project.is_some();
        let host = Host::spawn(session, project);
        let waker = Arc::new(Waker::default());
        link::spawn_listener(host.subscribe(), waker.clone());
        let events = host.subscribe();
        let (done_tx, done_rx) = mpsc::channel();
        let agents = Agents::new(&host, waker.clone());
        App {
            host,
            kernel,
            snapshot: Snapshot::default(),
            stale: true,
            events,
            done_tx,
            done_rx,
            in_flight: 0,
            waker,
            agent_changes: 0,
            has_project,
            project_path: None,
            project_risky: None,
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
            scene_requested: None,
            last_snapshot: Instant::now(),
            close_after_save: false,
            allow_close: false,
            show_iterations: false,
            agents,
        }
    }

    /// The model thread this window talks to (shared with the MCP server).
    pub fn host(&self) -> &HostHandle {
        &self.host
    }

    /// The project's file, once it was saved or opened.
    pub fn project_file(&self) -> Option<&std::path::Path> {
        self.project_path.as_deref()
    }

    /// Starts the localhost MCP endpoint for this window's project (ADR 0012: random
    /// port, bearer token, owner-only connection file). The app calls this at start-up;
    /// the endpoint follows the window when another project is opened.
    pub fn start_agents(&mut self, mut options: gusset_mcp::HttpOptions) {
        if options.project.is_none() {
            options.project.clone_from(&self.project_path);
        }
        self.agents.start_http(options);
    }

    /// **Hook point for `render_view`:** installs the renderer agents use to look at the
    /// model (the gusset-render adapter, when available). Without one, `render_view`
    /// answers with a clear "no renderer" error.
    pub fn set_renderer(&mut self, renderer: Option<Arc<dyn gusset_mcp::ViewRenderer>>) {
        self.agents.set_renderer(renderer);
    }

    pub(crate) fn ctx() -> CmdCtx {
        CmdCtx::user(now_unix())
    }

    // ------------------------------------------------------------ host jobs

    /// Queues `f` on the model thread; `wrap` turns its result into a [`Done`] for the UI.
    pub(crate) fn submit<R, F, W>(&mut self, f: F, wrap: W)
    where
        R: Send + 'static,
        F: FnOnce(&mut Session, Option<&mut Project>) -> R + Send + 'static,
        W: FnOnce(Result<R, CommandError>) -> Done + Send + 'static,
    {
        let tx = self.done_tx.clone();
        let waker = self.waker.clone();
        self.in_flight += 1;
        // The callback always runs exactly once (also on errors), so `in_flight` balances.
        let _ = self.host.exec_detached(f, move |r| {
            let _ = tx.send(wrap(r));
            waker.wake();
        });
    }

    /// Sends a registry command as the user without waiting; `then` says what to do with
    /// the result when it arrives.
    pub fn submit_command(&mut self, id: &str, args: Value, then: Then) {
        let ctx = Self::ctx();
        let id = id.to_string();
        let id2 = id.clone();
        self.submit(
            move |s, _| gusset_commands::execute(s, &ctx, &id2, args),
            move |r| Done::Command { id, then, result: r.and_then(|x| x) },
        );
    }

    /// Runs a registry command as the user and **waits** for the result (automation and
    /// tests; the UI itself uses [`App::submit_command`]). Reports it in the status bar.
    pub fn run(&mut self, id: &str, args: Value) -> Result<Value, CommandError> {
        let r = self.host.call(Self::ctx(), id, args);
        self.report(id, &r);
        self.stale = true;
        r
    }

    /// True while commands sent by the GUI are still running.
    pub fn busy(&self) -> bool {
        self.in_flight > 0
    }

    /// Waits until every job the GUI sent has finished and its result was applied, and
    /// the snapshot and scene are current (tests and shutdown paths; never per frame).
    pub fn wait_idle(&mut self) {
        for _ in 0..16 {
            // Jobs run in order, so once this one ran every earlier job has replied.
            let _ = self.host.exec(|_, _| ());
            self.poll(None);
            if self.in_flight == 0 && !self.stale {
                break;
            }
        }
    }

    fn report(&mut self, id: &str, r: &Result<Value, CommandError>) {
        match r {
            Ok(_) => {
                let title = gusset_commands::find(id).map_or(id, |s| s.title);
                self.notice = Some(Notice { text: format!("{title}: done"), hint: None, error: false });
            }
            Err(e) => self.notice = Some(Notice { text: e.message.clone(), hint: e.hint.clone(), error: true }),
        }
    }

    /// Applies finished jobs and host events, refreshes the snapshot and requests the
    /// scene when needed. `ctx` is `None` outside a frame (then reads may wait).
    fn poll(&mut self, ctx: Option<&egui::Context>) {
        loop {
            match self.events.try_recv() {
                Ok(HostEvent::Changed { actor, .. }) => {
                    if matches!(actor, Actor::Agent(_)) {
                        self.agent_changes += 1;
                    }
                    self.stale = true;
                }
                Ok(HostEvent::Saved) | Err(broadcast::error::TryRecvError::Lagged(_)) => self.stale = true,
                Ok(HostEvent::Shutdown)
                | Err(broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed) => break,
            }
        }
        while let Ok(done) = self.done_rx.try_recv() {
            self.in_flight = self.in_flight.saturating_sub(1);
            self.apply_done(done);
        }
        if self.stale {
            let snap = match ctx {
                Some(_) => self.host.try_with_session(|s| Snapshot::read(s)),
                None => self.host.with_session(|s| Snapshot::read(s)).map(Some),
            };
            match snap {
                Ok(Some(s)) => {
                    self.snapshot = s;
                    self.stale = false;
                }
                Ok(None) => {
                    if let Some(c) = ctx {
                        c.request_repaint_after(BUSY_RETRY);
                    }
                }
                Err(_) => self.stale = false, // shut down: keep the last snapshot
            }
        }
        self.request_scene();
    }

    /// Evaluates and tessellates the user's branch on the model thread when it changed.
    fn request_scene(&mut self) {
        let Some(key) = self.snapshot.scene_key() else { return };
        if self.scene.as_ref().is_some_and(|s| s.key == key) || self.scene_requested == Some(key) {
            return;
        }
        self.scene_requested = Some(key);
        self.submit(build_scene, Done::Scene);
    }

    fn apply_done(&mut self, done: Done) {
        match done {
            Done::Command { id, then, result } => {
                self.stale = true;
                self.apply_command(&id, then, result);
            }
            Done::Scene(Ok(Ok(data))) => {
                if self.scene.as_ref().is_none_or(|s| s.key != data.key) {
                    self.selection.clear();
                }
                self.scene =
                    Some(Scene { key: data.key, bodies: data.bodies, sketches: data.sketches, eval: data.eval });
            }
            Done::Scene(Ok(Err(e)) | Err(e)) => {
                self.notice = Some(Notice { text: e.message, hint: e.hint, error: true });
            }
            Done::Saved { result, then } => {
                self.stale = true;
                match result {
                    Ok(()) => {
                        self.notice = Some(Notice { text: tr("status.saved").into(), hint: None, error: false });
                        if then == AfterSave::Close {
                            self.close_after_save = true;
                        }
                    }
                    Err(e) => {
                        if then == AfterSave::Close {
                            self.dialog = Some(Dialog::Close { save_failed: Some(e.clone()) });
                        }
                        self.notice = Some(Notice { text: e, hint: None, error: true });
                    }
                }
            }
            Done::SavedAs { path, text, result } => {
                self.stale = true;
                match result {
                    Ok(()) => {
                        self.project_risky = gusset_store::paths::classify(&path).map(|r| format!("{r:?}"));
                        self.project_path = Some(path);
                        self.notice = Some(Notice { text: tr("status.saved").into(), hint: None, error: false });
                    }
                    Err(e) => {
                        self.dialog = Some(Dialog::Path { purpose: PathPurpose::SaveAs, text, error: Some(e) });
                    }
                }
            }
            Done::Recovery(r) => {
                if let Err(e) = r {
                    self.notice = Some(Notice { text: format!("Recovery copy failed: {e}"), hint: None, error: true });
                }
            }
            Done::Refresh(r) => {
                self.stale = true;
                if let Err(e) = r {
                    self.notice = Some(Notice { text: e.message, hint: e.hint, error: true });
                }
            }
        }
    }

    fn apply_command(&mut self, id: &str, then: Then, result: Result<Value, CommandError>) {
        match then {
            Then::Notice => self.report(id, &result),
            Then::Quiet => {
                if result.is_err() {
                    self.report(id, &result);
                }
            }
            Then::Form => {
                let Some(form) = self.form.as_mut().filter(|f| f.command == id) else {
                    self.report(id, &result);
                    return;
                };
                match result {
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
            Then::Code => {
                let Some(c) = self.code.as_mut() else { return };
                c.1 = match result {
                    Ok(_) => None,
                    Err(e) => {
                        let loc = e
                            .details
                            .as_ref()
                            .map(|d| format!(" (line {}, column {})", d["line"], d["column"]))
                            .unwrap_or_default();
                        Some(format!("{}{loc}", e.message))
                    }
                };
            }
            Then::Compare(branch) => self.show_compare(&branch, result),
            Then::PathDialog(purpose, text) => match result {
                Ok(_) => self.report(id, &result),
                Err(e) => self.dialog = Some(Dialog::Path { purpose, text, error: Some(e.message) }),
            },
        }
    }

    /// Opens the dialog for a command (or runs it directly when it takes no input).
    pub fn start_command(&mut self, id: &str) {
        let Some(spec) = gusset_commands::find(id) else { return };
        let mut form = Form::new(spec);
        if form.fields.is_empty() {
            self.submit_command(spec.id, json!({}), Then::Notice);
            return;
        }
        self.prefill(&mut form);
        self.form = Some(form);
    }

    /// Fills obvious fields from context: the latest sketch, selected edges and faces.
    fn prefill(&self, form: &mut Form) {
        let last_sketch = self
            .snapshot
            .doc
            .features
            .iter()
            .rev()
            .find(|f| matches!(f.kind, gusset_doc::FeatureKind::Sketch(_)))
            .map(|f| f.name.clone());
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

    /// Sends the open form's command; the form closes when it succeeded.
    pub fn apply_form(&mut self) {
        let Some(form) = self.form.as_mut() else { return };
        match form.to_args() {
            Ok(args) => {
                form.error = None;
                let id = form.command;
                self.submit_command(id, args, Then::Form);
            }
            Err(msg) => form.error = Some(msg),
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.snapshot.dirty
    }

    /// Starts saving to the current path; asks for one when the project was never saved.
    /// Returns false when nothing was started.
    pub fn save(&mut self) -> bool {
        self.save_then(AfterSave::Nothing)
    }

    fn save_then(&mut self, then: AfterSave) -> bool {
        if !self.has_project {
            self.notice = Some(Notice { text: "No data folder: cannot save".into(), hint: None, error: true });
            return false;
        }
        if self.project_path.is_none() {
            self.dialog = Some(Dialog::Path { purpose: PathPurpose::SaveAs, text: "part.gus".into(), error: None });
            return false;
        }
        self.notice = Some(Notice { text: "Saving…".into(), hint: None, error: false });
        self.submit(
            |s, p| {
                let p = p.ok_or("No data folder: cannot save")?;
                p.save(s.history()).map_err(|e| e.to_string())?;
                s.dirty = false;
                Ok(())
            },
            move |r| Done::Saved { result: r.map_err(|e| e.message).and_then(|x: Result<(), String>| x), then },
        );
        true
    }

    /// Starts "Save As" to `path`; a failure re-opens the path dialog with the error.
    pub fn save_as(&mut self, path: PathBuf) {
        if !self.has_project {
            self.notice = Some(Notice { text: "No data folder: cannot save".into(), hint: None, error: true });
            return;
        }
        let text = path.to_string_lossy().into_owned();
        let p2 = path.clone();
        self.submit(
            move |s, p| {
                let p = p.ok_or("No data folder: cannot save")?;
                p.save_as(&p2, s.history()).map_err(|e| e.to_string())?;
                s.project_dir = p2.parent().map(PathBuf::from);
                s.dirty = false;
                Ok(())
            },
            move |r| Done::SavedAs { path, text, result: r.map_err(|e| e.message).and_then(|x: Result<(), String>| x) },
        );
    }

    /// Replaces the hosted session: stops the current model thread (after the jobs already
    /// queued), lets `make` build the next session from the old one, and starts a new
    /// model thread for it.
    fn replace_host(&mut self, make: impl FnOnce(Option<(Session, Option<Project>)>) -> (Session, Option<Project>)) {
        // Agents are disconnected first; they reconnect to the new project (new token).
        self.agents.detach();
        let old = self.host.shutdown();
        // Results of the old host's jobs are all delivered by now; apply them first.
        while let Ok(done) = self.done_rx.try_recv() {
            self.in_flight = self.in_flight.saturating_sub(1);
            self.apply_done(done);
        }
        self.in_flight = 0;
        let (session, project) = make(old);
        self.has_project = project.is_some();
        self.host = Host::spawn(session, project);
        link::spawn_listener(self.host.subscribe(), self.waker.clone());
        self.events = self.host.subscribe();
        self.agents.attach(&self.host);
        self.stale = true;
        self.scene = None;
        self.scene_requested = None;
        self.selection.clear();
        self.fit_pending = true;
    }

    /// Opens a project file in this window (replacing the current project).
    pub fn open(&mut self, path: PathBuf) -> Result<(), String> {
        let dirs = self.dirs.clone().ok_or("No data folder")?;
        let (project, history, report) = Project::open(&path, dirs).map_err(|e| e.to_string())?;
        let mut session = Session::from_history(history, self.kernel.clone());
        session.project_dir = path.parent().map(PathBuf::from);
        if matches!(report, OpenReport::Damaged { .. }) {
            session.dirty = true;
        }
        self.project_risky = project.risky.as_ref().map(|r| format!("{r:?}"));
        self.project_path = Some(path);
        self.replace_host(move |old| {
            if let Some((_, Some(p))) = old {
                p.close();
            }
            (session, Some(project))
        });
        match report {
            OpenReport::Clean => {}
            OpenReport::RecoveryAvailable { recovered, .. } => self.dialog = Some(Dialog::Recovery { recovered }),
            OpenReport::Damaged { reason, source } => {
                self.dialog = Some(Dialog::Message {
                    title: tr("damaged.title").into(),
                    body: format!("{reason}\nOpened instead: {source:?}. Save to repair the file."),
                });
            }
        }
        Ok(())
    }

    /// Writes a recovery snapshot if there are unsaved changes and the interval passed.
    /// `force` (on exit) waits for it; otherwise it runs as a background job.
    pub fn maybe_snapshot(&mut self, force: bool) {
        if force {
            if let Err(e) = self.host.snapshot() {
                self.notice = Some(Notice { text: format!("Recovery copy failed: {e}"), hint: None, error: true });
            }
            return;
        }
        if !self.snapshot.dirty || self.last_snapshot.elapsed() < RECOVERY_INTERVAL {
            return;
        }
        self.last_snapshot = Instant::now();
        self.submit(
            |s, p| match p {
                Some(p) if s.dirty => p.snapshot(s.history()).map(Some).map_err(|e| e.to_string()),
                _ => Ok(None),
            },
            |r| Done::Recovery(r.map_err(|e| e.message).and_then(|x| x)),
        );
    }

    /// The evaluation of the drawn state (as of the last scene the host delivered).
    pub fn evaluation(&self) -> Option<Arc<Evaluation>> {
        self.scene.as_ref().map(|s| s.eval.clone())
    }

    /// Stops the model thread and closes the project (window closing).
    fn shutdown(&mut self, discard_recovery: bool) {
        self.agents.shutdown();
        if let Some((_, Some(p))) = self.host.shutdown() {
            if discard_recovery {
                p.discard_recovery();
            }
            p.close();
        }
    }

    // ------------------------------------------------------------------ UI

    /// Draws one frame.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.waker.set(&ctx);
        if self.theme_applied != Some(self.theme) {
            theme::apply(&ctx, self.theme, self.reduce_motion);
            self.theme_applied = Some(self.theme);
        }
        self.poll(Some(&ctx));
        if self.close_after_save {
            self.close_after_save = false;
            self.close_now(&ctx, false);
        }
        self.handle_close(&ctx);
        self.shortcuts(&ctx);
        self.maybe_snapshot(false);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::bottom("timeline").resizable(false).show(ui, |ui| self.timeline(ui));
        egui::Panel::left("browser").default_size(240.0).resizable(true).show(ui, |ui| self.browser(ui));
        if self.agents.show_panel {
            egui::Panel::right("agents").default_size(380.0).resizable(true).show(ui, |ui| self.agents_panel(ui));
        }
        if self.code.is_some() {
            egui::Panel::right("code").default_size(360.0).resizable(true).show(ui, |ui| self.code_view(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.viewport(ui));

        self.form_window(&ctx);
        self.palette_window(&ctx);
        self.dialogs(&ctx);
        // Last, so it is on top of every other dialog.
        self.approval_modal(&ctx);
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.is_dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.dialog = Some(Dialog::Close { save_failed: None });
            } else {
                self.allow_close = true;
                self.shutdown(false);
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
            self.submit_command("history.redo", json!({}), Then::Notice);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Z))) {
            self.submit_command("history.undo", json!({}), Then::Notice);
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
                    self.submit_command("history.undo", json!({}), Then::Notice);
                }
                if ui.button(tr("menu.redo")).clicked() {
                    self.submit_command("history.redo", json!({}), Then::Notice);
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
                ui.checkbox(&mut self.agents.show_panel, tr("agents.title"));
            });
            ui.menu_button(tr("menu.help"), |ui| {
                if ui.button(tr("menu.diagnostics")).clicked() {
                    self.dialog =
                        Some(Dialog::Message { title: tr("menu.diagnostics").into(), body: self.diagnostics() });
                }
                if ui.button(format!("{}…", tr("agents.title"))).clicked() {
                    self.agents.show_panel = true;
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
        if let Some(r) = &self.project_risky {
            s.push_str(&format!("Storage warning: {r}\n"));
        }
        s
    }

    pub fn toggle_code(&mut self) {
        self.code = match self.code {
            Some(_) => None,
            None => Some((self.snapshot.doc.to_text(), None)),
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
            if self.busy() {
                // A static indicator: an animated spinner would repaint every frame, and
                // the finished job wakes the UI anyway.
                ui.weak(format!("⏳ {}", tr("status.working")));
            }
            if let Some(eval) = self.scene.as_ref().map(|s| &s.eval)
                && !eval.errors().is_empty()
            {
                ui.colored_label(pal.danger, format!("⚠ {} ({})", tr("status.rebuild_errors"), eval.errors().len()));
            }
            if self.snapshot.audit_total > 0 || self.agent_changes > 0 {
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
        let doc = self.snapshot.doc.clone();
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
                let cur = self.snapshot.branch;
                let branches = self.snapshot.branches.clone();
                for b in branches {
                    let label = if b.agent.is_some() {
                        RichText::new(format!("◆ {}", plain(&b.name, 80))).color(pal.agent)
                    } else {
                        RichText::new(b.name.clone())
                    };
                    if ui.selectable_label(Some(b.id) == cur, label).clicked() && Some(b.id) != cur {
                        self.submit_command("branch.switch", json!({ "branch": b.id.to_string() }), Then::Quiet);
                    }
                }
            });
        });
    }

    fn timeline(&mut self, ui: &mut Ui) {
        let n = self.snapshot.doc.features.len();
        let rollback = self.snapshot.rollback;
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
                self.submit_command("timeline.set_rollback", json!({ "marker": m }), Then::Quiet);
            }
            egui::ScrollArea::horizontal().show(ui, |ui| {
                for (i, f) in self.snapshot.doc.features.iter().enumerate() {
                    let active = rollback.is_none_or(|m| i < m);
                    let t = RichText::new(&f.name);
                    ui.label(if active { t } else { t.weak() });
                }
            });
        });
        if self.show_iterations {
            ui.horizontal_wrapped(|ui| {
                for label in &self.snapshot.lineage {
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
            self.submit_command("document.set_text", json!({ "text": text }), Then::Code);
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
        let modal = egui::Modal::new(egui::Id::new("dialog"));
        match dialog {
            Dialog::Close { save_failed } => {
                let mut action = None;
                modal.show(ctx, |ui| {
                    ui.heading(tr("close.title"));
                    ui.label(tr("close.body"));
                    let name = self.project_path.as_ref().map_or_else(
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
                    // Closes once the save succeeded; on failure the dialog comes back
                    // with the error, and without a file Save As takes over.
                    Some(0) => {
                        self.save_then(AfterSave::Close);
                    }
                    Some(1) => self.close_now(ctx, true),
                    Some(_) => {}
                    None => self.dialog = Some(Dialog::Close { save_failed }),
                }
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
                        let kernel = self.kernel.clone();
                        self.replace_host(move |old| {
                            let (dir, project) = match old {
                                Some((s, p)) => (s.project_dir, p),
                                None => (None, None),
                            };
                            let mut session = Session::from_history(*recovered, kernel);
                            session.project_dir = dir;
                            session.dirty = true;
                            (session, project)
                        });
                    }
                    Some(1) => self.submit(
                        |_, p| {
                            if let Some(p) = p {
                                p.discard_recovery();
                            }
                            Ok(())
                        },
                        |r| Done::Refresh(r.and_then(|x| x)),
                    ),
                    Some(_) => {
                        // Keep both: the recovered state becomes a branch next to the saved one.
                        if let Ok(doc) = recovered.head_document(recovered.main()) {
                            let ctx = Self::ctx();
                            self.submit(
                                move |s, _| {
                                    gusset_commands::execute(
                                        s,
                                        &ctx,
                                        "branch.create",
                                        json!({ "name": "Recovered", "switch": true }),
                                    )?;
                                    s.commit(&ctx, "Recovered work", &doc).map(|_| ())
                                },
                                |r| Done::Refresh(r.and_then(|x| x)),
                            );
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
                    match self.path_action(purpose, &text) {
                        Ok(()) => return,
                        Err(e) => error = Some(e),
                    }
                }
                self.dialog = Some(Dialog::Path { purpose, text, error });
            }
            Dialog::Message { title, body } => {
                let mut keep = true;
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

    /// Runs a path dialog's action. Quick failures come back as `Err` (the dialog stays
    /// open); slower ones (saving, exporting) re-open the dialog with the error later.
    fn path_action(&mut self, purpose: PathPurpose, text: &str) -> Result<(), String> {
        let path = PathBuf::from(text.trim());
        if path.as_os_str().is_empty() {
            return Err("Enter a file path".into());
        }
        let abs = if path.is_absolute() {
            path
        } else {
            let base = self.snapshot.project_dir.clone().or_else(|| std::env::current_dir().ok()).unwrap_or_default();
            base.join(path)
        };
        let export = |app: &mut App, id: &str, extra: Value| {
            let mut args = json!({ "path": abs.to_string_lossy(), "overwrite": true });
            if let (Some(a), Some(e)) = (args.as_object_mut(), extra.as_object()) {
                a.extend(e.clone());
            }
            app.submit_command(id, args, Then::PathDialog(purpose, text.to_string()));
            Ok(())
        };
        match purpose {
            PathPurpose::SaveAs => {
                self.save_as(abs);
                Ok(())
            }
            PathPurpose::Open => self.open(abs),
            PathPurpose::ExportStep => export(self, "export.step", json!({})),
            PathPurpose::ExportStl => export(self, "export.mesh", json!({ "format": "stl" })),
            PathPurpose::Export3mf => export(self, "export.mesh", json!({ "format": "3mf" })),
        }
    }

    fn close_now(&mut self, ctx: &egui::Context, discard_recovery: bool) {
        self.allow_close = true;
        self.shutdown(discard_recovery);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // Stop the model thread; the project's lock is released when it drops.
        self.agents.shutdown();
        drop(self.host.shutdown());
    }
}

/// The scene job: evaluates the user's branch and tessellates its bodies (on the model
/// thread, so the UI thread never does the heavy work).
fn build_scene(s: &mut Session, _: Option<&mut Project>) -> Result<SceneData, CommandError> {
    let b = s.current_branch(&Actor::User);
    let (head, rollback) = {
        let br = s.history().branch(b).map_err(CommandError::history)?;
        (br.head, br.rollback)
    };
    let eval = s.evaluate(&Actor::User)?;
    let kernel = s.kernel().clone();
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
    Ok(SceneData { key: (head, rollback), bodies, sketches, eval })
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
