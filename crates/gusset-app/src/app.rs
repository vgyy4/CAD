//! The application: one project window in front of the model thread (DESIGN.md §4).
//!
//! The [`Session`] lives on the `gusset-host` model thread, shared with the MCP server,
//! so the user and AI agents work on the same open project (ADR 0012). Every action goes
//! through the command registry as a detached host job; results come back over a channel
//! and the UI never waits for a long command. What the UI draws is a [`Snapshot`] read
//! with a short, non-blocking `try_with_session` whenever the host reports a change. The
//! UI is a plain function of [`App`] state, which is what the kittest automation drives.

use crate::agents::{Agents, plain};
use crate::browser;
use crate::cmdline;
use crate::forms::{self, Form};
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::link::{self, Snapshot, Waker};
use crate::ribbon::{self, Action, Ribbon};
use crate::sketch_place::{self, Click};
use crate::theme::{self, ThemeKind};
use crate::timeline;
use crate::viewport::{BodyInfo, DrawSketch, Hit, NavRequest, Viewport};
use crate::viewport_nav::NavMode;
use egui::{RichText, Ui};
use gusset_commands::{CmdCtx, CommandError, Session, registry};
use gusset_eval::Evaluation;
use gusset_history::{Actor, History, NodeId};
use gusset_host::{Host, HostEvent, HostHandle};
use gusset_kernel::{Kernel, KernelErrorKind, TessellationParams};
use gusset_project::settings::{SaveAction, SaveScheduler, Settings};
use gusset_project::{OpenReport, PendingRecovery, Project, SaveWrite, SnapshotWrite};
use gusset_store::paths::DataDirs;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// How soon to retry a snapshot read while a long command holds the session.
const BUSY_RETRY: Duration = Duration::from_millis(50);

pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A modal dialog.
#[derive(Debug)]
pub enum Dialog {
    /// The project has unsaved changes and `pending` (close, open another file) would
    /// drop them: Save / Don't save / Cancel.
    Unsaved {
        pending: Pending,
        save_failed: Option<String>,
    },
    /// An operation is still running: wait for it, then continue with `pending`, or
    /// cancel.
    Busy {
        pending: Pending,
    },
    /// An export would replace an existing file.
    Overwrite {
        purpose: PathPurpose,
        path: PathBuf,
    },
    /// Start-up: recovery copies of projects that were not closed cleanly (never-saved
    /// ones, or newer than their file). Recover, discard, or decide later.
    Unrecovered {
        items: Vec<PendingRecovery>,
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
    /// Another mesh format, by its `export.mesh` format name (`obj`, `ply`, `glb`).
    ExportMesh(&'static str),
    /// A path-traced picture of the current view (PNG).
    Render,
    /// The settings (layout, tools, navigation; no recent files) as a JSON file.
    ExportSettings,
    ImportSettings,
}

/// Status-bar message.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub text: String,
    pub hint: Option<String>,
    pub error: bool,
}

/// How long a confirmation stays in the status bar.
const NOTICE_SECONDS: f64 = 5.0;

/// What to do with the result of a command the GUI sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Then {
    /// Report it in the status bar.
    Notice,
    /// It came from the open form: close the form, or show the error in it.
    Form,
    /// A read-only inspection (measure, overview): like `Form`, then show the result in a
    /// dialog.
    Show,
    /// It came from the code view: clear or show the parse error there.
    Code,
    /// Only report errors.
    Quiet,
    /// It came from the command line: report the result (or the error) in the status bar.
    CmdLine,
    /// A path dialog action: re-open the dialog with the error on failure.
    PathDialog(PathPurpose, String),
    /// A branch comparison for the agents panel (branch name, plain text).
    Compare(String),
    /// `query.components` after opening: a notice when components have newer sources.
    ComponentUpdates,
}

/// An action that replaces or ends the open project, run once unsaved changes and
/// running work were dealt with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    /// Close the window.
    Close,
    /// Open this file in the window.
    Open(PathBuf),
}

/// What to do after a save finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterSave {
    Nothing,
    /// "Save" from the unsaved-changes dialog: continue with the pending action.
    Then(Pending),
}

/// Results of host jobs, delivered to the UI thread.
/// A drawing's scene is current for this model state and drawing.
pub(crate) type SheetKey = ((gusset_history::NodeId, Option<usize>), String);

pub(crate) enum Done {
    /// A never-saved project was recovered from its recovery copy.
    RecoveredUntitled(Result<Box<(Project, Session)>, String>),
    Command {
        id: String,
        then: Then,
        result: Result<Value, CommandError>,
    },
    Scene(Result<Result<SceneData, CommandError>, CommandError>),
    /// A live preview of the open form (its key, and the tessellated would-be model).
    Preview(String, Result<Result<(gusset_render::Scene, Vec<BodyInfo>), CommandError>, CommandError>),
    /// A drawing's scene (ADR 0032).
    Sheet(SheetKey, Result<gusset_drawing::Scene, CommandError>),
    Saved {
        result: Result<(), String>,
        then: AfterSave,
        /// The save scheduler's mark when the save was queued (review M2).
        mark: u64,
    },
    SavedAs {
        path: PathBuf,
        text: String,
        result: Result<(), String>,
    },
    /// A recovery snapshot encoded on the model thread, to be written off it (M3).
    RecoveryPrepared(Result<Option<SnapshotWrite>, String>, u64),
    /// A save encoded on the model thread, to be written off it.
    SavePrepared {
        result: Result<SaveWrite, String>,
        then: AfterSave,
        mark: u64,
    },
    /// A recovery write finished; the second field is the scheduler mark it covers.
    Recovery(Result<Option<u64>, String>, u64),
    /// A project file read on a background thread (`discard`: the user chose not to keep
    /// the current project's unsaved changes).
    Opened {
        path: PathBuf,
        discard: bool,
        result: Result<Box<(Project, Session, OpenReport)>, String>,
    },
    /// "Keep both" wrote the recovered work to this file.
    KeptBoth(Result<PathBuf, String>),
    /// A job whose result needs no handling beyond refreshing the snapshot.
    Refresh(Result<(), CommandError>),
}

/// Drawable scene computed on the model thread (tessellation, render meshes and their
/// BVHs are all built there).
pub(crate) struct SceneData {
    key: (NodeId, Option<usize>),
    render: gusset_render::Scene,
    bodies: Vec<BodyInfo>,
    /// Bodies the renderer could not take.
    mesh_errors: Vec<String>,
    sketches: Vec<DrawSketch>,
    eval: Arc<Evaluation>,
}

/// What is drawn for one evaluated state (the bodies live in the [`Viewport`]).
struct Drawn {
    key: (NodeId, Option<usize>),
    sketches: Vec<DrawSketch>,
    eval: Arc<Evaluation>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App").field("theme", &self.theme).field("dirty", &self.snapshot.dirty).finish_non_exhaustive()
    }
}

pub struct App {
    /// A path-traced picture being made off the UI thread: where to, and its outcome.
    render_job: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    /// A 3D mouse through spacenavd, when one is there.
    spacemouse: Option<crate::spacemouse::SpaceMouse>,
    /// Size and paths a pixel of a rendered image.
    pub render_quality: ([u32; 2], u32),
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
    /// Save and recovery-copy writers running on their own threads (also counted in
    /// `in_flight`).
    writers: usize,
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
    /// UI scale (DESIGN.md §1.22) and the one last given to egui.
    pub ui_scale: f32,
    scale_applied: Option<f32>,
    /// The S quick bar's chosen tools outside and inside sketches (empty: the defaults).
    pub quick_tools: [Vec<&'static str>; 2],
    /// Panel layout (DESIGN.md §1.15), kept with the preferences.
    pub browser_collapsed: bool,
    pub timeline_collapsed: bool,
    pub docks_swapped: bool,
    /// The Surface tab is chosen (ADR 0036).
    pub surface_tab: bool,
    /// The Sheet Metal tab is chosen (ADR 0037).
    pub sheet_tab: bool,
    /// The Manufacture tab is chosen (ADR 0040).
    pub cam_tab: bool,
    /// The Render tab is chosen.
    pub render_tab: bool,
    /// The Render tab's photo preview of the viewport.
    pub photo: crate::photo_preview::PhotoPreview,
    /// The 3D view: camera, render scene, picking and the GPU/CPU renderer.
    pub viewport: Viewport,
    pub selection: Vec<Hit>,
    /// F11: a clean full-screen viewport (only the status bar and an open form stay).
    pub full_screen: bool,
    /// The marking menu's center (a right click in the viewport), while it is open.
    pub marking: Option<egui::Pos2>,
    /// The S-key quick bar's position, while it is open.
    pub quick: Option<egui::Pos2>,
    /// The last command started (the menus offer it first).
    pub(crate) last_command: Option<&'static str>,
    /// Recently opened or saved project files, newest first.
    pub recent: Vec<String>,
    /// The parameters table, while open: (name, expression being edited, expression when
    /// opened) per row; the last row adds a parameter.
    pub param_table: Option<Vec<(String, String, String)>>,
    /// The browser's and the properties panel's widths, kept between runs.
    pub panel_widths: (Option<f32>, Option<f32>),
    /// The rollback marker before an earlier sketch was edited (restored on Finish sketch).
    sketch_rollback: Option<Option<usize>>,
    /// "Select other": everything under an Alt+click, listed at that point.
    pub picker: Option<(egui::Pos2, Vec<Hit>)>,
    /// Dimensions being typed at the cursor while placing sketch geometry, with the last
    /// error.
    pub entry: Option<(crate::value_entry::Entry, Option<String>)>,
    /// The snapped point under the cursor while placing sketch geometry (preview).
    place_cursor: Option<Click>,
    /// A sketch to turn the camera to once its plane is known (editing starts).
    look_at_sketch: Option<String>,
    /// The last toolpath asked for (ADR 0040), drawn over the model until the model changes.
    pub(crate) toolpath: Option<DrawSketch>,
    /// A stress picture (ADR 0041) is shown in place of the model while its result is open.
    pub(crate) stress_view: bool,
    /// The title of the result dialog the picture belongs to.
    /// The simulation or CAM result shown in the results panel (until the model changes).
    pub(crate) results: Option<crate::results_panel::Results>,
    /// The arguments the latest result commands were sent with (for the panel's re-run).
    last_jobs: std::collections::BTreeMap<String, Value>,
    /// A sketch point being dragged: (sketch, point id, where it would go).
    drag_preview: Option<(String, String, [f64; 2])>,
    /// The placed command to start again when its result arrives (sketch tools repeat,
    /// lines chain from their end).
    chain: Option<Chain>,
    pub form: Option<Form>,
    pub palette: Option<String>,
    pub code: Option<(String, Option<String>)>,
    pub dialog: Option<Dialog>,
    pub notice: Option<Notice>,
    /// Shown once when the real kernel is unavailable.
    pub kernel_note: Option<String>,
    /// The kernel worker's sandbox report, read once at startup (review L5).
    pub kernel_status: Option<String>,
    scene: Option<Drawn>,
    scene_requested: Option<(NodeId, Option<usize>)>,
    /// Live preview (DESIGN.md §1.6): the key shown, the key asked for, and when the
    /// current form's key was first seen (previews wait until typing pauses).
    preview_shown: Option<String>,
    preview_requested: Option<String>,
    preview_since: Option<(String, f64)>,
    /// When to auto-save or write a recovery snapshot (settings: save mode, interval).
    save: SaveScheduler,
    /// The running import's cancel flag (the status bar offers Cancel).
    import_cancel: Option<gusset_commands::import::CancelFlag>,
    /// Continue with this action after the save that is running.
    after_save: Option<Pending>,
    /// Continue with this action once no job is running ("Wait" in the busy dialog).
    after_idle: Option<Pending>,
    /// Ask the window to close on the next frame.
    send_close: bool,
    /// Set when the window may close (after the close dialog).
    pub allow_close: bool,
    pub show_iterations: bool,
    /// The sketch being edited: the Sketch tab and its tools are active (DESIGN.md §2).
    active_sketch: Option<String>,
    /// The active sketch was seen in the document (a new one is not in the snapshot yet).
    active_sketch_seen: bool,
    /// The drawing shown in the Drawing tab (ADR 0032), its scene for the current model,
    /// the scene being built, and the sheet's pan and zoom.
    active_drawing: Option<String>,
    pub(crate) sheet: Option<(SheetKey, gusset_drawing::Scene)>,
    sheet_requested: Option<SheetKey>,
    pub(crate) sheet_state: crate::sheet::State,
    /// Sketch visibility the user chose in the browser (overrides the default: sketches
    /// used as a profile are hidden, others shown).
    pub sketch_visibility: std::collections::BTreeMap<String, bool>,
    /// Which browser categories are expanded.
    pub browser: browser::State,
    /// The command line above the status bar.
    pub cmdline: cmdline::State,
    /// The success notice on show and when it appeared (UI time): it fades after a while.
    notice_since: Option<(String, f64)>,
    /// The MCP server, its HTTP endpoint, approvals and the "AI agents" panel.
    pub agents: Agents,
}

/// The sandboxed STEP/IGES import worker installed next to the app (review H5).
fn importer() -> Option<Arc<dyn gusset_commands::session::Importer>> {
    gusset_worker::WorkerImporter::next_to_exe().map(|w| Arc::new(w) as Arc<dyn gusset_commands::session::Importer>)
}

impl App {
    /// A new, empty project.
    pub fn new(kernel: Arc<dyn Kernel>, dirs: Option<DataDirs>) -> App {
        let mut session = Session::new(&gusset_doc::Document::default(), kernel.clone(), &CmdCtx::user(now_unix()));
        session.importer = importer();
        // The single start-up routine (review M1): housekeeping, cache cap, and the
        // recovery copies to offer. It runs before this window's own project exists.
        let report = dirs.as_ref().and_then(|d| gusset_project::startup(d, &[]).ok());
        let settings = report.as_ref().map_or_else(Settings::default, |r| r.settings.clone());
        let unrecovered = report.map(|r| r.pending_recoveries).unwrap_or_default();
        let project = dirs.clone().and_then(|d| Project::create(d).ok());
        let has_project = project.is_some();
        let host = Host::spawn(session, project);
        let waker = Arc::new(Waker::default());
        link::spawn_listener(host.subscribe(), waker.clone());
        let events = host.subscribe();
        let (done_tx, done_rx) = mpsc::channel();
        let agents = Agents::new(&host, waker.clone());
        let prefs = dirs.as_ref().map(|d| crate::prefs::load(&crate::prefs::path(d.data_dir()))).unwrap_or_default();
        let mut app = App {
            render_job: None,
            spacemouse: crate::spacemouse::SpaceMouse::connect(None, Some(waker.clone())),
            render_quality: ([1600, 1200], 64),
            host,
            kernel,
            snapshot: Snapshot::default(),
            stale: true,
            events,
            done_tx,
            done_rx,
            in_flight: 0,
            writers: 0,
            waker,
            agent_changes: 0,
            has_project,
            project_path: None,
            project_risky: None,
            dirs,
            theme: ThemeKind::Dark,
            reduce_motion: false,
            ui_scale: 1.0,
            scale_applied: None,
            quick_tools: [Vec::new(), Vec::new()],
            browser_collapsed: false,
            timeline_collapsed: false,
            docks_swapped: false,
            surface_tab: false,
            sheet_tab: false,
            cam_tab: false,
            render_tab: false,
            photo: crate::photo_preview::PhotoPreview::default(),
            theme_applied: None,
            viewport: Viewport::default(),
            selection: Vec::new(),
            full_screen: false,
            marking: None,
            quick: None,
            last_command: None,
            sketch_rollback: None,
            recent: Vec::new(),
            param_table: None,
            panel_widths: (None, None),
            picker: None,
            entry: None,
            place_cursor: None,
            drag_preview: None,
            look_at_sketch: None,
            toolpath: None,
            stress_view: false,
            results: None,
            last_jobs: std::collections::BTreeMap::new(),
            chain: None,
            form: None,
            palette: None,
            code: None,
            dialog: (!unrecovered.is_empty()).then_some(Dialog::Unrecovered { items: unrecovered }),
            notice: None,
            kernel_note: None,
            kernel_status: None,
            scene: None,
            scene_requested: None,
            preview_shown: None,
            preview_requested: None,
            preview_since: None,
            save: SaveScheduler::new(settings),
            import_cancel: None,
            after_save: None,
            after_idle: None,
            send_close: false,
            allow_close: false,
            show_iterations: false,
            active_sketch: None,
            active_sketch_seen: false,
            active_drawing: None,
            sheet: None,
            sheet_requested: None,
            sheet_state: crate::sheet::State::default(),
            sketch_visibility: std::collections::BTreeMap::new(),
            browser: browser::State::default(),
            cmdline: cmdline::State::default(),
            notice_since: None,
            agents,
        };
        app.recent = prefs.recent.clone();
        app.apply_prefs(prefs);
        app
    }

    /// Takes over the look, layout, tools and navigation of `prefs` (not the recent files).
    pub fn apply_prefs(&mut self, prefs: crate::prefs::Prefs) {
        self.theme = prefs.theme;
        self.reduce_motion = prefs.reduce_motion;
        self.ui_scale = prefs.ui_scale;
        let ids = |v: &[String]| v.iter().filter_map(|id| gusset_commands::find(id)).map(|s| s.id).collect();
        self.quick_tools = [ids(&prefs.quick_solid), ids(&prefs.quick_sketch)];
        self.browser_collapsed = prefs.browser_collapsed;
        self.timeline_collapsed = prefs.timeline_collapsed;
        self.docks_swapped = prefs.docks_swapped;
        self.viewport.nav_preset = prefs.nav;
        self.panel_widths = (prefs.browser_width, prefs.properties_width);
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

    /// Renders the viewport on eframe's wgpu device. Without one (`None`) the viewport
    /// falls back to the CPU rasterizer.
    pub fn set_wgpu(&mut self, render_state: Option<&eframe::egui_wgpu::RenderState>) {
        self.viewport.set_gpu(render_state.map(crate::viewport_gpu::GpuTarget::from_render_state));
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
    pub fn submit_command(&mut self, id: &str, mut args: Value, then: Then) {
        let ctx = Self::ctx();
        if crate::results_panel::Kind::of(id).is_some() {
            self.last_jobs.insert(id.to_string(), args.clone());
        }
        if id == "cam.toolpath"
            && let Some(o) = args.as_object_mut()
        {
            // The app previews the cut in the viewport.
            o.entry("path").or_insert(Value::Bool(true));
        }
        if id == "sim.static"
            && let Some(o) = args.as_object_mut()
        {
            // The app colours the body by stress.
            o.entry("field").or_insert(Value::Bool(true));
        }
        if id == "sim.modal"
            && let Some(o) = args.as_object_mut()
        {
            // And shows the lowest mode's shape.
            o.entry("shape").or_insert(json!(1));
        }
        if id == "import.file" {
            // The worker parses on its own thread without holding the model, and the
            // user can cancel it (the sandboxed worker process is killed).
            let cancel = gusset_commands::import::CancelFlag::default();
            self.import_cancel = Some(cancel.clone());
            let (tx, waker) = (self.done_tx.clone(), self.waker.clone());
            self.in_flight += 1;
            let started = self.host.import_detached(ctx, args, cancel, move |r| {
                let _ = tx.send(Done::Command { id: "import.file".into(), then, result: r });
                waker.wake();
            });
            if let Err(e) = started {
                self.in_flight = self.in_flight.saturating_sub(1);
                self.import_cancel = None;
                self.notice = Some(Notice { text: e.message, hint: e.hint, error: true });
            }
            return;
        }
        // A newer edit stops the rebuild of the one still running (security review V1).
        self.host.supersede_rebuild(id);
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
            // Saves and recovery copies are written outside the model thread: wait for them too.
            if self.writers > 0
                && let Ok(done) = self.done_rx.recv_timeout(std::time::Duration::from_secs(10))
            {
                self.in_flight = self.in_flight.saturating_sub(1);
                self.apply_done(done);
            }
        }
    }

    fn report(&mut self, id: &str, r: &Result<Value, CommandError>) {
        match r {
            Ok(v) => {
                let title = gusset_commands::find(id).map_or(id, |s| s.title);
                self.notice = Some(Notice { text: done_text(title, v), hint: None, error: false });
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
                    self.save.changed(Instant::now());
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
        self.request_sheet();
        if let Some(c) = ctx {
            self.request_preview(c);
        }
    }

    /// The open form's preview key and command: feature and generator forms whose
    /// arguments are complete.
    fn preview_wanted(&self) -> Option<(String, &'static str, Value)> {
        let form = self.form.as_ref()?;
        let id = form.command;
        if !(id.starts_with("feature.") || id.starts_with("generate.")) {
            return None;
        }
        let args = if id == "feature.edit" && form.fields.iter().any(|f| f.name != "name") {
            Self::feature_edit_args(form)
        } else {
            form.to_args().ok()?
        };
        let key = format!("{id}|{args}|{:?}", self.snapshot.scene_key());
        Some((key, id, args))
    }

    /// Asks the model thread for a live preview of the open form once typing pauses
    /// (300 ms), one at a time; ends the preview when the form closes.
    fn request_preview(&mut self, ctx: &egui::Context) {
        let Some((key, id, args)) = self.preview_wanted() else {
            self.preview_since = None;
            if self.stress_view {
                // The stress picture stays while its result is open and current (not under
                // a dialog).
                if self.results.as_ref().is_some_and(|r| !r.stale) && self.dialog.is_none() {
                    return;
                }
                self.stress_view = false;
            }
            if self.viewport.previewing() || self.preview_shown.is_some() {
                self.viewport.end_preview();
                self.preview_shown = None;
                ctx.request_repaint();
            }
            return;
        };
        // One preview at a time, and none while a command runs: previews must not delay
        // real work on the model thread (review R10).
        if self.preview_shown.as_ref() == Some(&key) || self.preview_requested.is_some() || self.busy() {
            return;
        }
        let now = ctx.input(|i| i.time);
        match &self.preview_since {
            Some((k, t)) if *k == key => {
                if now - t < PREVIEW_DELAY {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(PREVIEW_DELAY));
                    return;
                }
            }
            _ => {
                self.preview_since = Some((key, now));
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(PREVIEW_DELAY));
                return;
            }
        }
        self.preview_requested = Some(key.clone());
        self.submit(move |s, _| build_preview(s, id, args), move |r| Done::Preview(key, r));
    }

    /// Builds the open drawing's scene on the model thread when the model changed.
    fn request_sheet(&mut self) {
        let (Some(name), Some(key)) = (self.active_drawing.clone(), self.snapshot.scene_key()) else { return };
        let key: SheetKey = (key, name.clone());
        if self.sheet.as_ref().is_some_and(|(k, _)| *k == key) || self.sheet_requested.as_ref() == Some(&key) {
            return;
        }
        self.sheet_requested = Some(key.clone());
        self.submit(
            move |s, _| gusset_commands::commands::drawing_scene(s, &gusset_history::Actor::User, &name),
            move |r| Done::Sheet(key, r.and_then(|x| x)),
        );
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
                if id == "import.file" {
                    self.import_cancel = None;
                }
                self.apply_command(&id, then, result);
            }
            Done::Preview(key, r) => {
                self.preview_requested = None;
                let current = self.preview_wanted().map(|(k, ..)| k);
                if current.as_ref() != Some(&key) {
                    return; // the form changed meanwhile: the next frame asks again
                }
                self.stress_view = false;
                match r {
                    Ok(Ok((render, bodies))) => self.viewport.show_preview(render, bodies),
                    // Not buildable as typed: the model stays; OK reports the error.
                    _ => self.viewport.end_preview(),
                }
                self.preview_shown = Some(key);
            }
            Done::Scene(Ok(Ok(data))) => {
                self.viewport.end_preview();
                self.stress_view = false;
                self.preview_shown = None;
                if self.scene.as_ref().is_none_or(|s| s.key != data.key) {
                    self.selection.clear();
                }
                if !data.mesh_errors.is_empty() {
                    self.notice = Some(Notice {
                        text: format!("Some bodies cannot be displayed: {}", data.mesh_errors.join("; ")),
                        hint: None,
                        error: true,
                    });
                }
                if self.scene.as_ref().is_some_and(|d| d.key != data.key) {
                    self.toolpath = None;
                    if let Some(r) = &mut self.results {
                        r.stale = true;
                    }
                }
                self.viewport.set_scene(data.render, data.bodies);
                self.scene = Some(Drawn { key: data.key, sketches: data.sketches, eval: data.eval });
            }
            Done::Scene(Ok(Err(e)) | Err(e)) => {
                self.notice = Some(Notice { text: e.message, hint: e.hint, error: true });
            }
            Done::Sheet(key, Ok(scene)) => self.sheet = Some((key, scene)),
            Done::Sheet(_, Err(e)) => {
                self.sheet_requested = None;
                if self
                    .active_drawing
                    .as_ref()
                    .is_some_and(|d| !self.snapshot.doc.drawings.iter().any(|x| &x.name == d))
                {
                    // Deleted or undone: back to the model.
                    self.active_drawing = None;
                } else {
                    self.notice = Some(Notice { text: e.message, hint: e.hint, error: true });
                }
            }
            Done::Saved { result, then, mark } => {
                self.writers = self.writers.saturating_sub(1);
                self.stale = true;
                match result {
                    Ok(()) => {
                        self.save.written_up_to(mark);
                        self.notice = Some(Notice { text: tr("status.saved").into(), hint: None, error: false });
                        if let AfterSave::Then(p) = then {
                            self.after_save = Some(p);
                        }
                    }
                    Err(e) => self.save_failed(e, Some(then)),
                }
            }
            Done::SavePrepared { result, then, mark } => match result {
                Ok(w) => self.write_save(w, then, mark),
                Err(e) => {
                    if let AfterSave::Then(pending) = then {
                        self.dialog = Some(Dialog::Unsaved { pending, save_failed: Some(e.clone()) });
                    }
                    self.notice = Some(Notice { text: e, hint: None, error: true });
                }
            },
            Done::SavedAs { path, text, result } => {
                self.stale = true;
                match result {
                    Ok(()) => {
                        self.project_risky = gusset_store::paths::classify(&path).map(|r| format!("{r:?}"));
                        self.remember_project(&path);
                        self.project_path = Some(path);
                        self.notice = Some(Notice { text: tr("status.saved").into(), hint: None, error: false });
                    }
                    Err(e) => {
                        self.dialog = Some(Dialog::Path { purpose: PathPurpose::SaveAs, text, error: Some(e) });
                    }
                }
            }
            Done::Opened { path, discard, result } => match result {
                Ok(loaded) => {
                    let (project, session, report) = *loaded;
                    self.finish_open(path, project, session, report, discard);
                }
                Err(e) => {
                    self.dialog = Some(Dialog::Message { title: tr("open.failed").into(), body: e });
                }
            },
            Done::KeptBoth(r) => match r {
                Ok(p) => {
                    self.notice = Some(Notice {
                        text: format!("{} {}", tr("recovery.kept"), p.to_string_lossy()),
                        hint: None,
                        error: false,
                    });
                }
                Err(e) => {
                    self.dialog = Some(Dialog::Message { title: tr("recovery.keep_failed").into(), body: e });
                }
            },
            Done::RecoveredUntitled(r) => match r {
                Ok(loaded) => {
                    let (project, session) = *loaded;
                    self.project_risky = None;
                    self.project_path = None;
                    self.replace_host(move |old| {
                        if let Some((s, Some(p))) = old {
                            retire_project(&s, p, false);
                        }
                        (session, Some(project))
                    });
                    self.notice = Some(Notice { text: tr("unrecovered.done").into(), hint: None, error: false });
                }
                Err(e) => self.dialog = Some(Dialog::Message { title: tr("open.failed").into(), body: e }),
            },
            Done::RecoveryPrepared(Ok(None), _) => {}
            Done::RecoveryPrepared(Ok(Some(w)), mark) => self.write_recovery(w, mark),
            Done::RecoveryPrepared(Err(e), _) => {
                self.notice = Some(Notice { text: format!("Recovery copy failed: {e}"), hint: None, error: true });
            }
            Done::Recovery(r, mark) => {
                self.writers = self.writers.saturating_sub(1);
                if r.is_ok() {
                    self.save.written_up_to(mark);
                }
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
            Then::CmdLine => match &result {
                Ok(v) => {
                    let mut text = format!("{id}: ok");
                    let shown = v.to_string();
                    if shown != "null" && shown != "{}" {
                        text.push_str(&format!(" {}", clip_result(&shown, 160)));
                    }
                    self.notice = Some(Notice { text, hint: None, error: false });
                }
                Err(_) => self.report(id, &result),
            },
            Then::Show => {
                let title = gusset_commands::find(id).map_or(id, |s| s.title);
                if id == "cam.toolpath" && result.is_err() {
                    self.toolpath = None; // an old toolpath is not this job's
                }
                match result {
                    Ok(mut v) => {
                        if self.form.as_ref().is_some_and(|f| f.command == id) {
                            self.form = None;
                        }
                        if id == "cam.toolpath"
                            && let Some(path) = v.as_object_mut().and_then(|o| o.remove("path"))
                        {
                            self.toolpath = Some(toolpath_drawing(&path, theme::palette(self.theme).warning));
                        }
                        let panel = crate::results_panel::Kind::of(id);
                        if panel.is_none() && !result_notes(&v).is_empty() {
                            self.notice = Some(Notice { text: done_text(title, &v), hint: None, error: false });
                        }
                        let mut legend_of = Vec::new();
                        let picture = match id {
                            "sim.static" => Some(("stress_bands", "MPa")),
                            "sim.modal" => Some(("shape_bands", "of the largest motion")),
                            _ => None,
                        };
                        // Draft analysis: faces by class, red (negative), yellow, green.
                        if id == "query.draft_analysis"
                            && let Some(o) = v.as_object_mut()
                            && let Some(bands) = o.remove("draft_bands")
                        {
                            let (scene, bodies, _) = bands_scene(&bands, "", &DRAFT_COLOURS);
                            self.viewport.show_preview(scene, bodies);
                            self.stress_view = true;
                        }
                        // An optimised shape: its kept voxel faces, drawn in place of the model.
                        if id == "sim.optimize"
                            && let Some(o) = v.as_object_mut()
                            && let Some(quads) = o.remove("shape_quads")
                        {
                            let (scene, bodies, _) = stress_scene(&json!([{"upto": 1.0, "quads": quads}]), "");
                            self.viewport.show_preview(scene, bodies);
                            self.stress_view = true;
                        }
                        if let Some((key, unit)) = picture
                            && let Some(o) = v.as_object_mut()
                            && let Some(bands) = o.remove(key)
                        {
                            let (scene, bodies, legend) = stress_scene(&bands, unit);
                            self.viewport.show_preview(scene, bodies);
                            self.stress_view = true;
                            legend_of = legend;
                        }
                        if let Some(kind) = panel {
                            let job = self.last_jobs.get(id).cloned().unwrap_or(Value::Null);
                            let path_visible = self.results.as_ref().is_none_or(|r| r.kind != kind || r.path_visible);
                            self.results = Some(crate::results_panel::Results {
                                kind,
                                title: title.to_string(),
                                job,
                                data: v,
                                legend: legend_of,
                                stale: false,
                                path_visible,
                            });
                            return;
                        }
                        let mut body = inspect_text(&v);
                        if let Some(head) = simulation_summary(&v) {
                            body = format!("{head}\n{body}");
                        }
                        self.dialog = Some(Dialog::Message { title: title.into(), body });
                    }
                    Err(e) => match self.form.as_mut().filter(|f| f.command == id) {
                        Some(form) => {
                            form.error = Some(match &e.hint {
                                Some(h) => format!("{} — {}: {h}", e.message, tr("error.fix")),
                                None => e.message.clone(),
                            });
                        }
                        None => self.report(id, &Err(e)),
                    },
                }
            }
            Then::Form => {
                let chain = self.chain.take().filter(|c| c.command == id);
                let Some(form) = self.form.as_mut().filter(|f| f.command == id) else {
                    self.report(id, &result);
                    return;
                };
                match result {
                    Ok(v) => {
                        let title = form.title;
                        // Ctrl+Enter: the same command again, with the values just used.
                        let again = form.repeat.then(|| {
                            let mut f = form.clone();
                            f.repeat = false;
                            f.error = None;
                            f.clicks.clear();
                            f
                        });
                        self.form = None;
                        self.notice = Some(Notice { text: done_text(title, &v), hint: None, error: false });
                        if let Some(ch) = chain {
                            self.restart_placing(ch, &v);
                        } else if let Some(f) = again {
                            self.form = Some(f);
                        }
                        if id == "drawing.create"
                            && let Some(name) = v.get("drawing").and_then(Value::as_str)
                        {
                            // A new drawing is shown right away.
                            self.open_drawing(name);
                        } else if id == "sketch.create"
                            && let Some(name) = v.get("sketch").and_then(Value::as_str)
                        {
                            // A new sketch is edited right away, looked at straight on.
                            self.active_sketch = Some(name.to_string());
                            self.look_at_sketch = Some(name.to_string());
                            self.active_sketch_seen = false;
                        } else if id.starts_with("feature.") && self.active_sketch.is_some() {
                            // A solid feature made from the sketch: its editing is over (it
                            // was inserted at the marker; the marker stays after it).
                            self.active_sketch = None;
                            self.viewport.ghost.clear();
                            self.sketch_rollback = None;
                        }
                    }
                    Err(e) => {
                        form.error = Some(match &e.hint {
                            Some(h) => format!("{} — {}: {h}", e.message, tr("error.fix")),
                            None => e.message.clone(),
                        });
                        // Clicked points start over (review R2): the next clicks place anew.
                        form.clicks.clear();
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
            Then::ComponentUpdates => {
                if let Some(text) = result.ok().as_ref().and_then(component_updates) {
                    self.notice = Some(Notice { text, hint: Some(tr("components.update_hint").into()), error: false });
                }
            }
            Then::PathDialog(purpose, text) => match result {
                Ok(_) => self.report(id, &result),
                Err(e) => self.dialog = Some(Dialog::Path { purpose, text, error: Some(e.message) }),
            },
        }
    }

    /// Opens the dialog for a command (or runs it directly when it takes no input).
    pub fn start_command(&mut self, id: &str) {
        let Some(spec) = gusset_commands::find(id) else { return };
        self.last_command = Some(spec.id);
        let mut form = Form::new(spec);
        if form.fields.is_empty() {
            let then = if inspects(spec.id) { Then::Show } else { Then::Notice };
            self.submit_command(spec.id, json!({}), then);
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
        // The sketch being edited first, else the newest one.
        if let Some(s) = self.active_sketch.clone().or(last_sketch) {
            form.set("sketch", &s);
        }
        self.fill_picks(form, false);
        // A joint from two picks (ADR 0026): the first is the moving body's frame, the
        // second the target.
        if form.command == "feature.joint" {
            for h in self.selection.iter().take(2) {
                pick_joint_frame(form, h);
            }
        }
        // The open drawing is the one drawing commands work on.
        if let Some(d) = &self.active_drawing {
            form.set("drawing", d);
        }
        // The sketch being edited wins over "the latest sketch".
        if let Some(s) = &self.active_sketch {
            form.set("sketch", s);
        }
    }

    /// Puts the picked edges, faces, plane and sketch items into the form's fields that
    /// take them. `live`: the form is already open and a pick changed the selection, so a
    /// field is only overwritten by picks of its own kind (and `plane` has no XY default).
    fn fill_picks(&self, form: &mut Form, live: bool) {
        let names = |want: fn(&Hit) -> Option<&str>| -> Vec<&str> { self.selection.iter().filter_map(want).collect() };
        let edges = names(|h| if let Hit::Edge { name, .. } = h { Some(name) } else { None });
        if !edges.is_empty() {
            form.set("edges", &edges.join(", "));
        }
        let faces = names(|h| if let Hit::Face { name, .. } = h { Some(name) } else { None });
        if !faces.is_empty() {
            form.set("faces", &faces.join(", "));
        }
        let face = self.selection.iter().rev().find_map(|h| match h {
            Hit::Face { name, .. } | Hit::Plane { name } => Some(name.as_str()),
            Hit::Edge { .. } | Hit::Sketch { .. } => None,
        });
        match face {
            Some(f) => {
                form.set("plane", f);
            }
            None if !live => {
                form.set("plane", "XY");
            }
            None => {}
        }
        // Measure: the first two picked faces or edges.
        if form.command == "query.measure" {
            let picks: Vec<String> = self
                .selection
                .iter()
                .filter(|h| matches!(h, Hit::Face { .. } | Hit::Edge { .. }))
                .map(Hit::label)
                .collect();
            for (field, pick) in ["a", "b"].into_iter().zip(&picks) {
                form.set(field, pick);
            }
        }
        // Picked sketch items: what a constraint applies to, what is deleted, the point moved.
        let sketch = form.value("sketch").map(str::to_string);
        let items: Vec<&str> = self
            .selection
            .iter()
            .filter_map(|h| match h {
                Hit::Sketch { sketch: s, id } if Some(s) == sketch.as_ref() => Some(id.as_str()),
                _ => None,
            })
            .collect();
        if !items.is_empty() {
            match form.command {
                "sketch.add_constraint" => {
                    form.set("entities", &items.join(", "));
                }
                "sketch.delete_items" => {
                    form.set("ids", &items.join(", "));
                }
                "sketch.move_point" => {
                    if let Some(p) = items.first() {
                        form.set("point", p);
                    }
                }
                _ => {}
            }
        }
    }

    /// Notes a panel's width once a resize ends (the pointer is up), and keeps it.
    fn keep_width(&mut self, ui: &Ui, which: usize, w: f32) {
        if ui.input(|i| i.pointer.any_down()) || !w.is_finite() {
            return;
        }
        let slot = if which == 0 { &mut self.panel_widths.0 } else { &mut self.panel_widths.1 };
        // The first frame records the width; a change of more than a pixel is a resize.
        match *slot {
            Some(old) if (old - w).abs() <= 1.0 => {}
            None => *slot = Some(w),
            Some(_) => {
                *slot = Some(w);
                self.save_prefs();
            }
        }
    }

    /// Gives egui the UI scale; a zoom made with the keyboard (Ctrl + plus / minus) becomes
    /// the new scale, kept within 75..=250 % and saved.
    fn apply_scale(&mut self, ctx: &egui::Context) {
        let zoom = ctx.zoom_factor();
        if let Some(applied) = self.scale_applied
            && (zoom - applied).abs() > 1e-3
            && (self.ui_scale - applied).abs() < 1e-3
        {
            self.ui_scale = zoom.clamp(crate::prefs::MIN_SCALE, crate::prefs::MAX_SCALE);
            self.save_prefs();
        }
        let want = self.ui_scale.clamp(crate::prefs::MIN_SCALE, crate::prefs::MAX_SCALE);
        if self.scale_applied != Some(want) || (zoom - want).abs() > 1e-3 {
            ctx.set_zoom_factor(want);
            self.scale_applied = Some(want);
        }
    }

    /// The window preferences as they stand.
    fn prefs(&self) -> crate::prefs::Prefs {
        crate::prefs::Prefs {
            theme: self.theme,
            reduce_motion: self.reduce_motion,
            nav: self.viewport.nav_preset,
            recent: self.recent.clone(),
            browser_width: self.panel_widths.0,
            properties_width: self.panel_widths.1,
            ui_scale: self.ui_scale,
            quick_solid: self.quick_tools[0].iter().map(|s| (*s).to_string()).collect(),
            quick_sketch: self.quick_tools[1].iter().map(|s| (*s).to_string()).collect(),
            browser_collapsed: self.browser_collapsed,
            timeline_collapsed: self.timeline_collapsed,
            docks_swapped: self.docks_swapped,
        }
    }

    /// Puts a project file first in the recent list (kept between runs).
    fn remember_project(&mut self, path: &std::path::Path) {
        crate::prefs::remember(&mut self.recent, &path.to_string_lossy());
        self.save_prefs();
    }

    /// The saved crash reports word for word (nothing is sent anywhere).
    fn crash_text(&self) -> String {
        let Some(d) = &self.dirs else { return tr("crash.none").into() };
        let dir = crate::crash::dir(d.data_dir());
        let files = crate::crash::list(&dir);
        if files.is_empty() {
            return tr("crash.none").into();
        }
        let mut out = format!("{} {}\n\n", tr("crash.where"), crate::crash::redact(&dir.to_string_lossy()));
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap_or_default();
            out.push_str(&text.chars().take(20_000).collect::<String>());
            out.push_str("\n----\n");
        }
        out
    }

    /// Keeps the preferences for the next run (a failure only costs the preference).
    fn save_prefs(&mut self) {
        let Some(dirs) = &self.dirs else { return };
        if let Err(e) = crate::prefs::save(&crate::prefs::path(dirs.data_dir()), &self.prefs()) {
            self.notice = Some(Notice { text: format!("Preferences not saved: {e}"), hint: None, error: true });
        }
    }

    /// The "select other" list, when open: a click picks (Shift adds), anything else closes.
    fn select_other(&mut self, ctx: &egui::Context) {
        let Some((at, hits)) = self.picker.clone() else { return };
        let mut chosen = None;
        // (Just up-left of the click, so the click that opened it lands inside.)
        let area = egui::Area::new(egui::Id::new("select_other"))
            .order(egui::Order::Foreground)
            .fixed_pos(at - egui::vec2(6.0, 6.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new(tr("select.other")).small().weak());
                    for h in &hits {
                        if ui.selectable_label(self.selection.contains(h), h.label()).clicked() {
                            chosen = Some(h.clone());
                        }
                    }
                });
            });
        if let Some(h) = chosen {
            if ctx.input(|i| i.modifiers.shift) {
                if !self.selection.contains(&h) {
                    self.selection.push(h);
                }
            } else {
                self.selection = vec![h];
            }
            self.picker = None;
            if let Some(mut form) = self.form.take() {
                if form.command == "feature.joint" {
                    if let Some(h) = self.selection.last() {
                        pick_joint_frame(&mut form, h);
                    }
                } else if form.command != "feature.edit" {
                    self.fill_picks(&mut form, true);
                }
                self.form = Some(form);
            }
        } else if ctx.input(|i| {
            i.pointer.primary_clicked() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p))
        }) {
            self.picker = None;
        }
    }

    /// The marking menu and the quick bar, when open, and what was chosen in them.
    fn quick_menus(&mut self, ctx: &egui::Context) {
        use crate::quick_menus as qm;
        let tools = qm::tools(self.editing_sketch().is_some(), self.last_command, self.quick_custom());
        let mut chosen = None;
        if let Some(at) = self.marking {
            match qm::marking(ctx, at, &tools) {
                Some(Some(t)) => chosen = Some(t),
                Some(None) => self.marking = None,
                None => {}
            }
        }
        if let Some(at) = self.quick {
            match qm::quick_bar(ctx, at, &tools) {
                Some(Some(t)) => chosen = Some(t),
                Some(None) => self.quick = None,
                None => {}
            }
        }
        let Some(t) = chosen else { return };
        self.run_quick_tool(t);
    }

    /// The quick bar's chosen tools in the current context (empty: the defaults).
    fn quick_custom(&self) -> &[&'static str] {
        let i = usize::from(self.editing_sketch().is_some());
        self.quick_tools.get(i).map_or(&[], Vec::as_slice)
    }

    /// Adds a tool to the current context's quick bar (at most [`qm::MAX_TOOLS`]; the
    /// oldest added goes) or removes it; the choice is kept with the preferences.
    pub fn set_quick_tool(&mut self, id: &'static str, add: bool) {
        use crate::quick_menus as qm;
        let sketching = self.editing_sketch().is_some();
        let Some(list) = self.quick_tools.get_mut(usize::from(sketching)) else { return };
        if list.is_empty() {
            *list = qm::defaults(sketching).to_vec();
        }
        list.retain(|t| *t != id);
        if add {
            list.push(id);
            while list.len() > qm::MAX_TOOLS {
                list.remove(0);
            }
        }
        self.save_prefs();
    }

    /// Runs a tool chosen in the marking menu, the quick bar or by a flick.
    fn run_quick_tool(&mut self, t: crate::quick_menus::Tool) {
        use crate::quick_menus::Tool;
        self.marking = None;
        self.quick = None;
        match t {
            Tool::Command(id) => self.start_command(id),
            Tool::FinishSketch => self.finish_sketch(),
            Tool::Search => self.palette = Some(String::new()),
        }
    }

    /// Carries out a feature context-menu choice (browser rows and timeline chips).
    pub fn feature_choice(&mut self, name: &str, choice: browser::FeatureChoice) {
        use browser::FeatureChoice as C;
        match choice {
            C::Edit => self.edit_feature(name),
            C::Rename => {
                self.start_command("feature.rename");
                if let Some(form) = self.form.as_mut() {
                    form.set("from", name);
                    form.set("to", name);
                }
            }
            C::Suppress(on) => {
                self.submit_command("feature.suppress", json!({ "name": name, "suppressed": on }), Then::Notice);
            }
            C::Delete => self.submit_command("feature.delete", json!({ "name": name }), Then::Notice),
            C::Group => {
                self.start_command("feature.group");
                let group = self.snapshot.doc.feature(name).and_then(|f| f.group.clone());
                if let Some(form) = self.form.as_mut() {
                    form.set("features", name);
                    form.set("group", group.as_deref().unwrap_or("Group"));
                }
            }
            C::Ungroup => self.submit_command("feature.group", json!({ "features": [name] }), Then::Notice),
            C::Appearance => {
                // The body this feature makes, or the one it changes (review R4): a fillet
                // or hole colours its target body; a sketch has none.
                let body = self.scene.as_ref().and_then(|sc| {
                    let ev = &sc.eval;
                    ev.bodies
                        .iter()
                        .find(|b| b.name == name || b.feature == name)
                        .map(|b| b.name.clone())
                        .or_else(|| ev.meshes.iter().find(|m| m.name == name).map(|m| m.name.clone()))
                        .or_else(|| {
                            let f = self.snapshot.doc.feature(name)?;
                            f.kind.dependencies().into_iter().find(|d| ev.body(d).is_some())
                        })
                });
                let Some(body) = body else {
                    self.notice = Some(Notice {
                        text: format!("{name} makes no body to colour"),
                        hint: Some("choose Appearance on a body or on the feature that makes it".into()),
                        error: true,
                    });
                    return;
                };
                let name = body.as_str();
                let look = self.snapshot.doc.appearance(name).cloned();
                // Faces of this body picked: colour just them (a face takes no finish).
                let picked: Vec<String> = self
                    .selection
                    .iter()
                    .filter_map(|h| match h {
                        Hit::Face { body: b, name: f, .. } if b == name => Some(f.clone()),
                        _ => None,
                    })
                    .collect();
                self.start_command("body.appearance");
                if let Some(form) = self.form.as_mut() {
                    form.set("body", name);
                    let mut color = look.as_ref().map_or(0x00b4_bcc8, |a| a.color);
                    if picked.is_empty() {
                        form.set("faces", "");
                        form.set("finish", look.as_ref().map_or("plastic", |a| a.finish.keyword()));
                    } else {
                        // The first picked face's own colour, if it has one.
                        if let Some(c) = look.as_ref().and_then(|a| {
                            a.faces
                                .iter()
                                .find(|g| g.faces.iter().any(|f| picked.first() == Some(&f.to_string())))
                                .map(|g| g.color)
                        }) {
                            color = c;
                        }
                        form.set("faces", &picked.join(", "));
                        form.set("finish", "");
                    }
                    form.set("color", &format!("#{color:06x}"));
                    form.show_more = true;
                }
            }
            C::AcceptSuggestion => {
                let status = self.scene.as_ref().and_then(|s| s.eval.status(name).cloned());
                if let Some((from, to)) = browser::suggestion(status.as_ref()) {
                    let args = json!({ "name": name, "from": from, "to": to });
                    self.submit_command("feature.replace_reference", args, Then::Notice);
                }
            }
            C::RollHere => {
                if let Some(i) = self.snapshot.doc.feature_index(name) {
                    let n = self.snapshot.doc.features.len();
                    self.set_rollback(if i + 1 >= n { None } else { Some(i + 1) });
                }
            }
        }
    }

    /// Opens a feature's arguments in a form (`feature.edit`); sketches open in sketch mode.
    pub fn edit_feature(&mut self, name: &str) {
        let Some(f) = self.snapshot.doc.feature(name) else { return };
        if matches!(f.kind, gusset_doc::FeatureKind::Sketch(_)) {
            self.edit_sketch(name);
            return;
        }
        let Some((_, args)) = gusset_doc::text::statement_args(f) else { return };
        let Some(spec) = gusset_commands::find("feature.edit") else { return };
        let mut form = Form::new(spec);
        form.fields.retain(|f| f.name == "name");
        form.set("name", name);
        for (k, v) in args {
            form.fields.push(forms::Field {
                label: forms::humanize(&k),
                name: k,
                help: String::new(),
                kind: forms::FieldKind::Text,
                required: true,
                value: v,
            });
        }
        self.form = Some(form);
    }

    /// The `feature.edit` arguments of an edit form: the changed values to set, the
    /// emptied ones to remove.
    fn feature_edit_args(form: &Form) -> Value {
        let mut set = serde_json::Map::new();
        let mut remove = Vec::new();
        for f in form.fields.iter().filter(|f| f.name != "name") {
            if f.value.trim().is_empty() {
                remove.push(f.name.clone());
            } else {
                set.insert(f.name.clone(), Value::String(f.value.trim().to_string()));
            }
        }
        json!({ "name": form.value("name").unwrap_or_default(), "set": set, "remove": remove })
    }

    /// Sends the open form's command; the form closes when it succeeded.
    pub fn apply_form(&mut self) {
        let Some(form) = self.form.as_mut() else { return };
        if form.command == "feature.edit" && form.fields.iter().any(|f| f.name != "name") {
            let args = Self::feature_edit_args(form);
            form.error = None;
            self.submit_command("feature.edit", args, Then::Form);
            return;
        }
        match form.to_args() {
            Ok(args) => {
                form.error = None;
                let id = form.command;
                self.submit_command(id, args, if inspects(id) { Then::Show } else { Then::Form });
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
        let mark = self.save.mark();
        // Only the encoding runs on the model thread; the durable write runs on its own
        // thread, so edits are not held up (post-MVP item M3, saves).
        self.submit(
            |s, p| {
                let p = p.ok_or("No data folder: cannot save")?;
                let w = p.prepare_save(s.history()).map_err(|e| e.to_string())?;
                // Edits from now on mark it dirty again; a failed write restores it.
                s.dirty = false;
                Ok(w)
            },
            move |r| Done::SavePrepared {
                result: r.map_err(|e| e.message).and_then(|x: Result<SaveWrite, String>| x),
                then,
                mark,
            },
        );
        true
    }

    /// Writes a prepared save on its own thread; the result arrives as [`Done::Saved`].
    fn write_save(&mut self, w: SaveWrite, then: AfterSave, mark: u64) {
        let (tx, waker) = (self.done_tx.clone(), self.waker.clone());
        let spawned = std::thread::Builder::new().name("gusset-save".into()).spawn(move || {
            let result = w.write().map(|_| ()).map_err(|e| e.to_string());
            let _ = tx.send(Done::Saved { result, then, mark });
            waker.wake();
        });
        match spawned {
            Ok(_) => {
                self.in_flight += 1;
                self.writers += 1;
            }
            Err(e) => self.save_failed(e.to_string(), None),
        }
    }

    /// A save did not reach the disk: the project is unsaved again.
    fn save_failed(&mut self, e: String, then: Option<AfterSave>) {
        self.submit(|s, _| s.dirty = true, |_| Done::Refresh(Ok(())));
        if let Some(AfterSave::Then(pending)) = then {
            self.dialog = Some(Dialog::Unsaved { pending, save_failed: Some(e.clone()) });
        }
        self.notice = Some(Notice { text: e, hint: None, error: true });
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
        // A detached import of the old project may still be running: cancel it, and give
        // the new project its own result channel, so a late result can neither be
        // applied to it nor miscount its running jobs (code re-review N5).
        if let Some(c) = self.import_cancel.take() {
            c.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let (tx, rx) = mpsc::channel();
        self.done_tx = tx;
        self.done_rx = rx;
        self.in_flight = 0;
        // The old project's writers report to the old channel; closing it (below, in
        // `make`) waits for a write in progress and cancels the others.
        self.writers = 0;
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
        self.viewport.set_scene(gusset_render::Scene::new(), Vec::new());
        // A preview of the old project is gone with it (review R1).
        self.preview_shown = None;
        self.preview_requested = None;
        self.preview_since = None;
        self.viewport.fit_pending = true;
    }

    /// Reads a project file and builds its session (any thread).
    fn load(
        path: &std::path::Path,
        dirs: DataDirs,
        kernel: Arc<dyn Kernel>,
    ) -> Result<(Project, Session, OpenReport), String> {
        let (project, history, report) = Project::open(path, dirs).map_err(|e| e.to_string())?;
        let mut session = Session::from_history(history, kernel);
        session.importer = importer();
        session.project_dir = path.parent().map(PathBuf::from);
        if matches!(report, OpenReport::Damaged { .. }) {
            session.dirty = true;
        }
        Ok((project, session, report))
    }

    /// Opens a project file in this window, waiting for it (start-up, automation and
    /// tests; the File menu uses [`App::request`] with [`Pending::Open`]). The current
    /// project's unsaved changes are kept in its recovery file.
    pub fn open(&mut self, path: PathBuf) -> Result<(), String> {
        let dirs = self.dirs.clone().ok_or("No data folder")?;
        let (project, session, report) = Self::load(&path, dirs, self.kernel.clone())?;
        self.finish_open(path, project, session, report, false);
        Ok(())
    }

    /// Reads `path` on a background thread; the window switches to it when it is loaded.
    fn open_in_background(&mut self, path: PathBuf, discard: bool) {
        let Some(dirs) = self.dirs.clone() else {
            self.notice = Some(Notice { text: "No data folder".into(), hint: None, error: true });
            return;
        };
        let kernel = self.kernel.clone();
        let tx = self.done_tx.clone();
        let waker = self.waker.clone();
        self.in_flight += 1;
        self.notice = Some(Notice { text: tr("status.opening").into(), hint: None, error: false });
        let spawned = std::thread::Builder::new().name("gusset-open".into()).spawn(move || {
            let result = Self::load(&path, dirs, kernel).map(Box::new);
            let _ = tx.send(Done::Opened { path, discard, result });
            waker.wake();
        });
        if let Err(e) = spawned {
            self.in_flight = self.in_flight.saturating_sub(1);
            self.notice = Some(Notice { text: e.to_string(), hint: None, error: true });
        }
    }

    /// Recovers a copy offered at start-up: a saved project opens normally (its open
    /// path offers the newer recovery copy); a never-saved one is restored as Untitled.
    fn recover_pending(&mut self, it: PendingRecovery) {
        if let Some(path) = it.path {
            self.open_in_background(path, false);
            return;
        }
        let Some(dirs) = self.dirs.clone() else { return };
        let kernel = self.kernel.clone();
        let tx = self.done_tx.clone();
        let waker = self.waker.clone();
        self.in_flight += 1;
        let spawned = std::thread::Builder::new().name("gusset-recover".into()).spawn(move || {
            let r = Project::recover_unsaved(dirs, it.id).map_err(|e| e.to_string()).map(|(project, history)| {
                let mut session = Session::from_history(history, kernel);
                session.importer = importer();
                session.dirty = true;
                Box::new((project, session))
            });
            let _ = tx.send(Done::RecoveredUntitled(r));
            waker.wake();
        });
        if let Err(e) = spawned {
            self.in_flight = self.in_flight.saturating_sub(1);
            self.notice = Some(Notice { text: e.to_string(), hint: None, error: true });
        }
    }

    /// Switches the window to a loaded project. The old project is closed; if it has
    /// changes the user did not choose to discard (for example an agent edit that came
    /// in after the question), they stay in its recovery file instead.
    fn finish_open(&mut self, path: PathBuf, project: Project, session: Session, report: OpenReport, discard: bool) {
        self.project_risky = project.risky.as_ref().map(|r| format!("{r:?}"));
        self.remember_project(&path);
        self.project_path = Some(path);
        self.replace_host(move |old| {
            if let Some((s, Some(p))) = old {
                retire_project(&s, p, discard);
            }
            (session, Some(project))
        });
        // Components whose source documents changed since they were pinned (ADR 0026 §7).
        // Only sources inside the project folder: opening a file reads nothing else (review
        // v0.3 H1); Update reads the others when the user asks.
        self.submit_command("query.components", json!({ "local_only": true }), Then::ComponentUpdates);
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
        let has_file = self.project_path.is_some();
        match self.save.due(Instant::now(), has_file, !self.busy()) {
            SaveAction::Nothing => return,
            SaveAction::SaveProject => {
                // Auto-save mode: the project file itself (the scheduler waits for a
                // quiet moment after the last change).
                self.save_then(AfterSave::Nothing);
                return;
            }
            SaveAction::WriteRecovery => {}
        }
        // Only the encoding runs on the model thread; the disk write (up to half a
        // second for large imports) runs on its own thread (M3).
        let mark = self.save.mark();
        self.submit(
            |s, p| match p {
                Some(p) if s.dirty => p.prepare_snapshot(s.history()).map(Some).map_err(|e| e.to_string()),
                _ => Ok(None),
            },
            move |r| Done::RecoveryPrepared(r.map_err(|e| e.message).and_then(|x| x), mark),
        );
    }

    /// Writes a prepared recovery snapshot on its own thread. A newer snapshot, a save
    /// or closing the project makes it skip the write (see [`SnapshotWrite`]).
    fn write_recovery(&mut self, w: SnapshotWrite, mark: u64) {
        let (tx, waker) = (self.done_tx.clone(), self.waker.clone());
        let spawned = std::thread::Builder::new().name("gusset-recovery".into()).spawn(move || {
            let _ = tx.send(Done::Recovery(w.write().map_err(|e| e.to_string()), mark));
            waker.wake();
        });
        match spawned {
            Ok(_) => {
                self.in_flight += 1;
                self.writers += 1;
            }
            Err(e) => {
                self.notice = Some(Notice { text: format!("Recovery copy failed: {e}"), hint: None, error: true });
            }
        }
    }

    /// The evaluation of the drawn state (as of the last scene the host delivered).
    pub fn evaluation(&self) -> Option<Arc<Evaluation>> {
        self.scene.as_ref().map(|s| s.eval.clone())
    }

    /// A sketch as drawn in the viewport (curves, points, constraint labels).
    #[must_use]
    pub fn drawn_sketch(&self, name: &str) -> Option<&DrawSketch> {
        self.scene.as_ref()?.sketches.iter().find(|d| d.name == name)
    }

    /// Stops the model thread and closes the project (window closing).
    fn shutdown(&mut self, discard_recovery: bool) {
        // Agents first, so no edit lands after the final dirty check below.
        self.agents.shutdown();
        if let Some((s, Some(p))) = self.host.shutdown() {
            retire_project(&s, p, discard_recovery);
        }
    }

    /// Runs `pending` (close, open another file) once unsaved changes and running work
    /// were dealt with. Decides from the host's current state, not from the last
    /// snapshot, so an agent edit or a job that just finished is not lost.
    pub fn request(&mut self, pending: Pending) {
        if self.busy() {
            self.dialog = Some(Dialog::Busy { pending });
            return;
        }
        match self.host.try_with_session(|s| s.dirty) {
            Ok(Some(true)) => self.dialog = Some(Dialog::Unsaved { pending, save_failed: None }),
            Ok(Some(false)) | Err(_) => self.proceed(pending, false),
            // The model thread is busy (an agent's command): ask, as for our own jobs.
            Ok(None) => self.dialog = Some(Dialog::Busy { pending }),
        }
    }

    /// Runs `pending` now. `discard`: the user chose not to keep unsaved changes.
    fn proceed(&mut self, pending: Pending, discard: bool) {
        match pending {
            Pending::Close => self.close_now(discard),
            Pending::Open(path) => self.open_in_background(path, discard),
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
        self.apply_scale(&ctx);
        self.poll(Some(&ctx));
        self.poll_render(&ctx);
        if let Some(sm) = &self.spacemouse {
            match sm.drain() {
                Some((Some((t, r)), _)) => {
                    let h = f64::from(ctx.content_rect().height());
                    crate::spacemouse::apply(&mut self.viewport.camera, t, r, h);
                    ctx.request_repaint();
                }
                Some(_) => {}
                None => self.spacemouse = None,
            }
        }
        if let Some(p) = self.after_save.take() {
            // Saved; check again (an agent may have edited since) before continuing.
            self.request(p);
        }
        if !self.busy()
            && let Some(p) = self.after_idle.take()
        {
            self.request(p);
        }
        self.handle_close(&ctx);
        if self.send_close {
            self.send_close = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.shortcuts(&ctx);
        self.maybe_snapshot(false);
        // Wake up when an auto-save or recovery snapshot becomes due, without polling.
        if let Some(t) = self.save.next_deadline(self.project_path.is_some()) {
            ctx.request_repaint_after(t.saturating_duration_since(Instant::now()));
        }

        self.viewport.reduce_motion = self.reduce_motion;
        self.check_active_sketch();
        let pal = *theme::palette(self.theme);
        let bar = |fill: egui::Color32, top: i8, bottom: i8| {
            egui::Frame::new().fill(fill).inner_margin(egui::Margin { left: 8, right: 8, top, bottom })
        };
        let full = self.full_screen;
        if !full {
            egui::Panel::top("titlebar").frame(bar(pal.bg_app, 3, 3)).show(ui, |ui| self.title_bar(ui));
            egui::Panel::top("tabs")
                .frame(bar(pal.bg_panel, 4, 2))
                .show_separator_line(false)
                .show(ui, |ui| self.workspace_tabs(ui));
            egui::Panel::top("ribbon").frame(bar(pal.bg_panel, 2, 4)).show(ui, |ui| self.ribbon(ui));
        }
        // Bottom, from the edge up: status bar, command line (when shown), timeline.
        egui::Panel::bottom("status").frame(bar(pal.bg_app, 3, 3)).show(ui, |ui| self.status_bar(ui));
        if self.cmdline.visible && !full {
            egui::Panel::bottom("cmdline")
                .frame(bar(pal.bg_panel, 4, 4))
                .show_separator_line(false)
                .show(ui, |ui| self.command_line(ui));
        }
        // The browser docks at the left and the properties panel at the right, or the other
        // way round (View > Browser on the right).
        let swapped = self.docks_swapped;
        let side = move |id: &'static str, main: bool| {
            if main != swapped { egui::Panel::left(id) } else { egui::Panel::right(id) }
        };
        let before = self.prefs();
        if !full {
            if self.timeline_collapsed {
                egui::Panel::bottom("timeline_rail").resizable(false).frame(bar(pal.bg_panel, 1, 1)).show(ui, |ui| {
                    if ui.small_button(format!("+ {}", tr("panel.timeline"))).clicked() {
                        self.timeline_collapsed = false;
                    }
                });
            } else {
                egui::Panel::bottom("timeline")
                    .resizable(false)
                    .frame(bar(pal.bg_panel, 4, 4))
                    .show(ui, |ui| self.timeline(ui));
            }
            if self.browser_collapsed {
                side("browser_rail", true).exact_size(26.0).resizable(false).show(ui, |ui| {
                    let arrow = if swapped { "‹" } else { "›" };
                    if ui.button(arrow).on_hover_text(tr("panel.show_browser")).clicked() {
                        self.browser_collapsed = false;
                    }
                });
            } else {
                let w = side("browser", true)
                    .default_size(self.panel_widths.0.unwrap_or(240.0))
                    .min_size(200.0)
                    .resizable(true)
                    .show(ui, |ui| self.browser(ui))
                    .response
                    .rect
                    .width();
                self.keep_width(ui, 0, w);
            }
        }
        // The properties panel (DESIGN.md §2): the running command's form, docked at the
        // right like Fusion's command dialog instead of floating over the model.
        if self.form.is_some() {
            let w = side("properties", false)
                .default_size(self.panel_widths.1.unwrap_or(320.0))
                .min_size(260.0)
                .resizable(true)
                .show(ui, |ui| self.properties_panel(ui))
                .response
                .rect
                .width();
            self.keep_width(ui, 1, w);
        }
        if self.prefs() != before {
            self.save_prefs();
        }
        if let Some(r) = self.results.clone() {
            let pal = *theme::palette(self.theme);
            let actions = side("results", false)
                .default_size(300.0)
                .min_size(240.0)
                .resizable(true)
                .show(ui, |ui| crate::results_panel::show(ui, &r, &pal))
                .inner;
            for a in actions {
                self.results_action(a);
            }
        }
        if self.agents.show_panel && !full {
            egui::Panel::right("agents").default_size(380.0).resizable(true).show(ui, |ui| self.agents_panel(ui));
        }
        if self.code.is_some() && !full {
            egui::Panel::right("code").default_size(360.0).resizable(true).show(ui, |ui| self.code_view(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            if self.active_drawing.is_some() {
                self.sheet_view(ui);
            } else {
                self.viewport(ui);
            }
        });

        self.palette_window(&ctx);
        self.param_table_window(&ctx);
        self.dialogs(&ctx);
        // Last, so it is on top of every other dialog.
        self.approval_modal(&ctx);
        self.quick_menus(&ctx);
        self.select_other(&ctx);
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            // Always cancel first: the window closes on our own Close command once the
            // project was saved, discarded or found clean.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.dialog.is_none() {
                self.request(Pending::Close);
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let sc = |m, k| KeyboardShortcut::new(m, k);
        // Ctrl+Shift+K shows the command line and puts the cursor in it.
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::COMMAND | Modifiers::SHIFT, Key::K))) {
            self.cmdline.visible = true;
            self.cmdline.focus = true;
        }
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
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::NONE, Key::F11))) {
            self.full_screen = !self.full_screen;
        }
        // Esc first closes typed dimensions, and only them.
        if self.entry.is_some() && ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::NONE, Key::Escape))) {
            self.entry = None;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::NONE, Key::Escape))) {
            self.marking = None;
            self.quick = None;
            self.picker = None;
            self.form = None;
            self.palette = None;
            self.selection.clear();
            self.viewport.nav_mode = NavMode::Select;
        }
        // Delete removes the picked items of the edited sketch.
        // (Only when sketch items are picked: otherwise Delete stays feature.delete's key.)
        let picked: Vec<String> = match self.editing_sketch() {
            Some(sketch) => self
                .selection
                .iter()
                .filter_map(|h| match h {
                    Hit::Sketch { sketch: s, id } if s == sketch => Some(id.clone()),
                    _ => None,
                })
                .collect(),
            None => Vec::new(),
        };
        if !picked.is_empty()
            && !ctx.egui_wants_keyboard_input()
            && self.form.is_none()
            && let Some(sketch) = self.editing_sketch().map(str::to_string)
            && ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::NONE, Key::Delete)))
        {
            self.selection.clear();
            self.submit_command("sketch.delete_items", json!({ "sketch": sketch, "ids": picked }), Then::Notice);
        }
        // X held: see through the model (DESIGN.md §1.14).
        let see = !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_down(Key::X) && i.modifiers.is_none());
        if see != self.viewport.see_through {
            self.viewport.see_through = see;
            ctx.request_repaint();
        }
        // S: the quick bar at the pointer (DESIGN.md §1.3).
        if !ctx.egui_wants_keyboard_input()
            && self.form.is_none()
            && self.palette.is_none()
            && ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::NONE, Key::S)))
        {
            let at = ctx.pointer_hover_pos().unwrap_or_else(|| self.viewport.rect().center());
            self.quick = Some(at);
            self.marking = None;
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

    /// The title bar: app mark, menus, open/save/undo/redo, the document tab, the agents
    /// toggle and the version.
    fn title_bar(&mut self, ui: &mut Ui) {
        let pal = *theme::palette(self.theme);
        egui::MenuBar::new().ui(ui, |ui| {
            icons::icon(ui, Icon::Diamond, pal.accent);
            ui.label(RichText::new(tr("app.title")).strong());
            ui.add_space(theme::SPACE[2]);
            ui.menu_button(tr("menu.file"), |ui| {
                if ui.button(tr("menu.open")).clicked() {
                    self.dialog = Some(Dialog::Path { purpose: PathPurpose::Open, text: String::new(), error: None });
                    // (The path dialog then asks about unsaved changes: `request(Pending::Open)`.)
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
                        ("OBJ…", PathPurpose::ExportMesh("obj"), "obj"),
                        ("PLY…", PathPurpose::ExportMesh("ply"), "ply"),
                        ("glTF (GLB)…", PathPurpose::ExportMesh("glb"), "glb"),
                        ("Rendered image (PNG)…", PathPurpose::Render, "png"),
                    ] {
                        if ui.button(label).clicked() {
                            self.dialog = Some(Dialog::Path { purpose, text: format!("part.{ext}"), error: None });
                        }
                    }
                });
                ui.menu_button(tr("menu.settings_file"), |ui| {
                    if ui.button(tr("menu.settings_export")).clicked() {
                        self.dialog = Some(Dialog::Path {
                            purpose: PathPurpose::ExportSettings,
                            text: "gusset-settings.json".into(),
                            error: None,
                        });
                    }
                    if ui.button(tr("menu.settings_import")).clicked() {
                        self.dialog = Some(Dialog::Path {
                            purpose: PathPurpose::ImportSettings,
                            text: "gusset-settings.json".into(),
                            error: None,
                        });
                    }
                });
                // Collaboration (ADR 0044): bundles and a shared folder.
                ui.menu_button(tr("menu.share"), |ui| {
                    for (label, id) in [
                        ("menu.share_export", "history.export_bundle"),
                        ("menu.share_import", "history.import_bundle"),
                        ("menu.share_sync", "history.sync"),
                    ] {
                        if ui.button(tr(label)).clicked() {
                            self.start_command(id);
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
                let before = self.prefs();
                if ui.radio(self.theme == ThemeKind::Dark, tr("menu.theme_dark")).clicked() {
                    self.theme = ThemeKind::Dark;
                }
                if ui.radio(self.theme == ThemeKind::Light, tr("menu.theme_light")).clicked() {
                    self.theme = ThemeKind::Light;
                }
                if ui.checkbox(&mut self.reduce_motion, "Reduce motion").changed() {
                    self.theme_applied = None;
                }
                ui.menu_button(tr("menu.ui_scale"), |ui| {
                    for s in crate::prefs::SCALES {
                        if ui.radio((self.ui_scale - s).abs() < 1e-3, format!("{:.0} %", s * 100.0)).clicked() {
                            self.ui_scale = s;
                        }
                    }
                });
                ui.menu_button(tr("menu.navigation"), |ui| {
                    for p in crate::viewport::NavPreset::ALL {
                        if ui.radio(self.viewport.nav_preset == p, p.name()).clicked() {
                            self.viewport.nav_preset = p;
                        }
                    }
                });
                if self.prefs() != before {
                    self.save_prefs();
                }
                if ui.button(tr("menu.code")).clicked() {
                    self.toggle_code();
                }
                if ui.button("Fit view").clicked() {
                    self.viewport.request(NavRequest::Fit);
                }
                ui.checkbox(&mut self.full_screen, tr("menu.full_screen"));
                let layout = self.prefs();
                let mut shown = !self.browser_collapsed;
                if ui.checkbox(&mut shown, tr("panel.browser")).changed() {
                    self.browser_collapsed = !shown;
                }
                let mut shown = !self.timeline_collapsed;
                if ui.checkbox(&mut shown, tr("panel.timeline")).changed() {
                    self.timeline_collapsed = !shown;
                }
                ui.checkbox(&mut self.docks_swapped, tr("panel.browser_right"));
                if self.prefs() != layout {
                    self.save_prefs();
                }
                if ui.button(tr("params.title")).clicked() {
                    self.open_param_table();
                }
                ui.checkbox(&mut self.agents.show_panel, tr("agents.title"));
                if ui.checkbox(&mut self.cmdline.visible, tr("menu.cmdline")).changed() && self.cmdline.visible {
                    self.cmdline.focus = true;
                }
            });
            ui.menu_button(tr("menu.help"), |ui| {
                if ui.button(tr("menu.diagnostics")).clicked() {
                    self.dialog =
                        Some(Dialog::Message { title: tr("menu.diagnostics").into(), body: self.diagnostics() });
                }
                if ui.button(format!("{}…", tr("agents.title"))).clicked() {
                    self.agents.show_panel = true;
                }
                if ui.button(tr("menu.crash_reports")).clicked() {
                    self.dialog =
                        Some(Dialog::Message { title: tr("menu.crash_reports").into(), body: self.crash_text() });
                }
                if ui.button(tr("menu.crash_delete")).clicked() {
                    if let Some(d) = &self.dirs {
                        for f in crate::crash::list(&crate::crash::dir(d.data_dir())) {
                            let _ = std::fs::remove_file(f);
                        }
                    }
                    self.notice = Some(Notice { text: tr("crash.deleted").into(), hint: None, error: false });
                }
            });
            ui.separator();
            if ribbon::icon_button(ui, &pal, icons::Tool::Open, "Open project").clicked() {
                self.dialog = Some(Dialog::Path { purpose: PathPurpose::Open, text: String::new(), error: None });
            }
            if ribbon::icon_button(ui, &pal, icons::Tool::Save, "Save project (Ctrl+S)").clicked() {
                self.save();
            }
            if ribbon::icon_button(ui, &pal, icons::Tool::Undo, "Undo (Ctrl+Z)").clicked() {
                self.submit_command("history.undo", json!({}), Then::Notice);
            }
            if ribbon::icon_button(ui, &pal, icons::Tool::Redo, "Redo (Ctrl+Shift+Z)").clicked() {
                self.submit_command("history.redo", json!({}), Then::Notice);
            }
            ui.separator();
            let name = self
                .project_path
                .as_deref()
                .and_then(|p| p.file_stem())
                .map_or_else(|| tr("title.untitled").to_string(), |n| n.to_string_lossy().into_owned());
            egui::Frame::new()
                .fill(pal.bg_panel)
                .stroke(egui::Stroke::new(1.0, pal.border))
                .corner_radius(theme::RADIUS_SM)
                .inner_margin(egui::Margin::symmetric(10, 1))
                .show(ui, |ui| {
                    ui.label(name);
                    if self.is_dirty() {
                        icons::icon(ui, Icon::Dot, pal.warning);
                    }
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(format!("v{}", env!("CARGO_PKG_VERSION")));
                let shown = self.agents.show_panel;
                let agents = ribbon::icon_button_with(ui, &pal, "Agents panel", Some(shown), |p, r| {
                    icons::paint(p, r.shrink(6.0), Icon::Diamond, pal.agent);
                });
                if agents.clicked() {
                    self.agents.show_panel = !self.agents.show_panel;
                }
            });
        });
    }

    /// The Design workspace switcher, the SOLID / SKETCH tabs and the command search.
    fn workspace_tabs(&mut self, ui: &mut Ui) {
        let pal = *theme::palette(self.theme);
        let action = ribbon::tabs(
            ui,
            &Ribbon {
                pal: &pal,
                sketch: self.active_sketch.as_deref(),
                drawing: self.active_drawing.as_deref(),
                select: self.viewport.select_filter,
                quick: &[],
                surface: self.surface_tab,
                sheet: self.sheet_tab,
                cam: self.cam_tab,
                render: self.render_tab,
                photo_preview: self.photo.on,
            },
        );
        if let Some(a) = action {
            self.ribbon_action(a);
        }
    }

    /// The ribbon of the active tab.
    fn ribbon(&mut self, ui: &mut Ui) {
        let pal = *theme::palette(self.theme);
        let quick: Vec<&'static str> = match self.quick_custom() {
            [] => crate::quick_menus::defaults(self.editing_sketch().is_some()).to_vec(),
            c => c.to_vec(),
        };
        let action = ribbon::show(
            ui,
            &Ribbon {
                pal: &pal,
                sketch: self.active_sketch.as_deref(),
                drawing: self.active_drawing.as_deref(),
                select: self.viewport.select_filter,
                quick: &quick,
                surface: self.surface_tab,
                sheet: self.sheet_tab,
                cam: self.cam_tab,
                render: self.render_tab,
                photo_preview: self.photo.on,
            },
        );
        if let Some(a) = action {
            self.ribbon_action(a);
        }
    }

    /// Carries out what the tabs or the ribbon reported.
    pub fn ribbon_action(&mut self, action: Action) {
        match action {
            // "Render image" in the app renders the current view (`render_to`), not a named view.
            Action::Run("export.image") => {
                self.dialog = Some(Dialog::Path { purpose: PathPurpose::Render, text: "part.png".into(), error: None });
            }
            Action::Run(id) => self.start_command(id),
            Action::Search => self.palette = Some(String::new()),
            Action::SelectFilter(f) => self.viewport.select_filter = f,
            Action::QuickBar(id, add) => self.set_quick_tool(id, add),
            Action::SolidTab | Action::FinishSketch => {
                self.active_drawing = None;
                self.surface_tab = false;
                self.sheet_tab = false;
                self.cam_tab = false;
                self.render_tab = false;
                self.photo.set(false);
                self.finish_sketch();
            }
            Action::SurfaceTab => {
                self.active_drawing = None;
                (self.surface_tab, self.sheet_tab, self.cam_tab, self.render_tab) = (true, false, false, false);
                self.photo.set(false);
                self.finish_sketch();
            }
            Action::SheetTab => {
                self.active_drawing = None;
                (self.surface_tab, self.sheet_tab, self.cam_tab, self.render_tab) = (false, true, false, false);
                self.photo.set(false);
                self.finish_sketch();
            }
            Action::PhotoPreview(on) => self.photo.set(on),
            Action::RenderTab => {
                self.active_drawing = None;
                (self.surface_tab, self.sheet_tab, self.cam_tab, self.render_tab) = (false, false, false, true);
                self.finish_sketch();
            }
            Action::CamTab => {
                self.active_drawing = None;
                (self.surface_tab, self.sheet_tab, self.cam_tab, self.render_tab) = (false, false, true, false);
                self.photo.set(false);
                self.finish_sketch();
            }
            Action::DrawingTab => match self.snapshot.doc.drawings.last().map(|d| d.name.clone()) {
                Some(name) => self.open_drawing(&name),
                None => self.start_command("drawing.create"),
            },
            Action::SketchTab => {
                self.active_drawing = None;
                let latest = self
                    .snapshot
                    .doc
                    .features
                    .iter()
                    .rev()
                    .find(|f| matches!(f.kind, gusset_doc::FeatureKind::Sketch(_)))
                    .map(|f| f.name.clone());
                match latest {
                    Some(name) => self.edit_sketch(&name),
                    None => self.start_command("sketch.create"),
                }
            }
        }
    }

    /// The drawing shown in the Drawing tab, if any.
    pub fn active_drawing(&self) -> Option<&str> {
        self.active_drawing.as_deref()
    }

    /// Shows a drawing in the Drawing tab.
    pub fn open_drawing(&mut self, name: &str) {
        self.active_drawing = Some(name.to_string());
        self.request_sheet();
    }

    /// The Drawing tab's sheet: the open drawing, panned, zoomed and edited by dragging.
    fn sheet_view(&mut self, ui: &mut Ui) {
        let pal = *theme::palette(self.theme);
        let Some(name) = self.active_drawing.clone() else { return };
        let base: Vec<String> = self
            .snapshot
            .doc
            .drawings
            .iter()
            .find(|d| d.name == name)
            .map(|d| {
                d.items
                    .iter()
                    .filter_map(|i| match i {
                        gusset_doc::drawing::DrawingItem::View(v)
                            if matches!(
                                v.placement,
                                gusset_doc::drawing::ViewPlacement::Base { .. }
                                    | gusset_doc::drawing::ViewPlacement::Detail { .. }
                            ) =>
                        {
                            Some(v.name.clone())
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        // The last scene stays up while the next one builds.
        let scene = self.sheet.as_ref().filter(|(k, _)| k.1 == name).map(|(_, s)| s);
        let out = crate::sheet::show(ui, &pal, &name, scene, &base, &mut self.sheet_state);
        if let Some((item, at)) = out.moved {
            let at = [at[0].to_string(), at[1].to_string()];
            self.submit_command("drawing.move_item", json!({"drawing": name, "item": item, "at": at}), Then::Quiet);
        }
    }

    /// The sketch being edited, if any.
    pub fn active_sketch(&self) -> Option<&str> {
        self.active_sketch.as_deref()
    }

    /// Starts editing an existing sketch: the Sketch tab and its tools become active.
    pub fn edit_sketch(&mut self, name: &str) {
        // An earlier sketch is edited with the model rolled back to just after it, and the
        // rest ghosted (DESIGN.md §1.5); Finish sketch puts the marker back.
        let active = self.snapshot.rollback.unwrap_or(self.snapshot.doc.features.len());
        if let Some(i) = self.snapshot.doc.feature_index(name)
            && i + 1 < active
            && self.sketch_rollback.is_none()
        {
            self.sketch_rollback = Some(self.snapshot.rollback);
            self.viewport.ghost = self
                .viewport
                .scene()
                .meshes()
                .filter(|(_, m)| m.visible())
                .flat_map(|(_, m)| m.world_edges())
                .collect();
            self.set_rollback(Some(i + 1));
        }
        self.look_at_sketch = Some(name.to_string());
        self.active_sketch = Some(name.to_string());
        self.active_sketch_seen = true;
        self.sketch_visibility.insert(name.to_string(), true);
    }

    /// Ends sketch editing (the Solid tab comes back).
    pub fn finish_sketch(&mut self) {
        self.active_sketch = None;
        self.end_sketch_rollback();
        if self.form.as_ref().is_some_and(|f| f.command.starts_with("sketch.")) {
            self.form = None;
        }
    }

    /// Puts the rollback marker back after editing an earlier sketch, and drops the ghost.
    fn end_sketch_rollback(&mut self) {
        self.viewport.ghost.clear();
        if let Some(prev) = self.sketch_rollback.take() {
            self.set_rollback(prev);
        }
    }

    /// Leaves sketch editing when the sketch is gone (undo, another document).
    fn check_active_sketch(&mut self) {
        let Some(name) = &self.active_sketch else { return };
        let present = self
            .snapshot
            .doc
            .features
            .iter()
            .any(|f| &f.name == name && matches!(f.kind, gusset_doc::FeatureKind::Sketch(_)));
        if present {
            self.active_sketch_seen = true;
        } else if self.active_sketch_seen {
            self.active_sketch = None;
            self.viewport.ghost.clear();
            self.sketch_rollback = None;
        }
    }

    pub fn diagnostics(&self) -> String {
        let hw = gusset_hw::Diagnostics::collect();
        let mut s = format!("Gusset {}\n{}\n", env!("CARGO_PKG_VERSION"), hw.to_text());
        if let Some(n) = &self.kernel_note {
            s.push_str(n);
            s.push('\n');
        }
        if let Some(k) = &self.kernel_status {
            s.push_str(&format!("Kernel worker: {k}\n"));
        }
        if let Some(r) = &self.project_risky {
            s.push_str(&format!("Storage warning: {r}\n"));
        }
        s
    }

    /// The sketch the open command form edits, if any (its sketch stays visible).
    pub fn editing_sketch(&self) -> Option<&str> {
        let from_form = self
            .form
            .as_ref()
            .filter(|f| f.command.starts_with("sketch.") && f.command != "sketch.create")
            .and_then(|form| form.fields.iter().find(|f| f.name == "sketch"))
            .map(|f| f.value.trim())
            .filter(|n| !n.is_empty());
        from_form.or(self.active_sketch.as_deref())
    }

    /// Whether a sketch is drawn in the viewport: the one being edited, one the user
    /// made visible, or (by default) one no feature consumes as a profile.
    pub fn sketch_visible(&self, name: &str) -> bool {
        if self.editing_sketch() == Some(name) || self.region_form_sketch() == Some(name) {
            return true;
        }
        match self.sketch_visibility.get(name) {
            Some(&v) => v,
            None => !consumed_sketches(&self.snapshot.doc).contains(name),
        }
    }

    pub fn toggle_code(&mut self) {
        self.code = match self.code {
            Some(_) => None,
            None => Some((self.snapshot.doc.to_text(), None)),
        };
    }

    /// The command line: parses what was typed and runs it through the registry like the
    /// ribbon does.
    fn command_line(&mut self, ui: &mut Ui) {
        let pal = *theme::palette(self.theme);
        if let Some(line) = cmdline::show(ui, &mut self.cmdline, &pal) {
            self.run_command_line(&line);
        }
    }

    /// Runs a command line (`feature.extrude sketch=S1 distance="10 mm"`) through
    /// [`App::submit_command`]; a line that does not parse is reported in the status bar.
    pub fn run_command_line(&mut self, line: &str) {
        match cmdline::parse(line) {
            Ok(p) => {
                let then = if inspects(p.id) { Then::Show } else { Then::CmdLine };
                self.submit_command(p.id, p.args, then);
            }
            Err(msg) => self.notice = Some(Notice { text: msg, hint: None, error: true }),
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let pal = theme::palette(self.theme);
            if self.is_dirty() {
                icons::labeled(ui, Icon::Dot, pal.warning, tr("status.unsaved"));
            } else {
                icons::labeled(ui, Icon::Check, pal.success, tr("status.saved"));
            }
            if self.busy() {
                // A static indicator: an animated spinner would repaint every frame, and
                // the finished job wakes the UI anyway. During a rebuild it names the
                // feature being built, refreshed a few times a second (ADR 0022 §4).
                match self.host.rebuild_progress() {
                    Some(p) => {
                        ui.weak(format!("⏳ {} {} ({}/{})", tr("status.building"), p.feature, p.index, p.total));
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                    }
                    None => {
                        ui.weak(format!("⏳ {}", tr("status.working")));
                    }
                }
                // Stops the rebuild, also inside a long kernel operation (the kernel worker
                // is killed, ADR 0022 §3). Imports have their own Cancel below.
                if self.import_cancel.is_none()
                    && ui.small_button(tr("status.stop")).on_hover_text(tr("status.stop_hint")).clicked()
                {
                    self.host.cancel_rebuild();
                }
            }
            if let Some(c) = &self.import_cancel {
                ui.weak(tr("status.importing"));
                if ui.small_button(tr("dialog.cancel")).clicked() {
                    c.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
            if let Some(eval) = self.scene.as_ref().map(|s| &s.eval)
                && !eval.errors().is_empty()
            {
                icons::labeled(
                    ui,
                    Icon::Warning,
                    pal.danger,
                    format!("{} ({})", tr("status.rebuild_errors"), eval.errors().len()),
                );
            }
            if self.snapshot.audit_total > 0 || self.agent_changes > 0 {
                icons::labeled(ui, Icon::Diamond, pal.agent, tr("status.agent"));
            }
            // Confirmations ("saved", "done") go away after a few seconds; errors stay.
            let now = ui.input(|i| i.time);
            match &self.notice {
                Some(n) if !n.error => match &self.notice_since {
                    Some((text, t)) if *text == n.text => {
                        if now - t > NOTICE_SECONDS {
                            self.notice = None;
                            self.notice_since = None;
                        } else {
                            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(
                                NOTICE_SECONDS + 0.1 - (now - t),
                            ));
                        }
                    }
                    _ => {
                        self.notice_since = Some((n.text.clone(), now));
                        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(NOTICE_SECONDS + 0.1));
                    }
                },
                _ => self.notice_since = None,
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

    /// Carries out a click in the results panel.
    pub(crate) fn results_action(&mut self, a: crate::results_panel::Action) {
        use crate::results_panel::{Action, Kind};
        let Some(r) = self.results.as_mut() else { return };
        match a {
            Action::Frame(p) => self.viewport.request(NavRequest::FrameOn { point: p }),
            Action::Close => self.results = None,
            Action::ShowPath(on) => r.path_visible = on,
            Action::Rerun | Action::Mode(_) => {
                let id = match r.kind {
                    Kind::Static => "sim.static",
                    Kind::Modal => "sim.modal",
                    Kind::Cam => "cam.toolpath",
                    Kind::Optimize => "sim.optimize",
                    Kind::Draft => "query.draft_analysis",
                };
                let mut job = r.job.clone();
                // A re-run shows the result again; it does not write the STL a second time.
                if let Some(o) = job.as_object_mut()
                    && r.kind == Kind::Optimize
                {
                    o.remove("path");
                }
                if let (Action::Mode(k), Some(o)) = (&a, job.as_object_mut()) {
                    o.insert("shape".into(), json!(k));
                }
                self.submit_command(id, job, Then::Show);
            }
            Action::ExportGcode => {
                let job = r.job.clone();
                self.start_command("cam.export_gcode");
                if let (Some(form), Some(o)) = (self.form.as_mut(), job.as_object()) {
                    for (k, v) in o.iter().filter(|(k, _)| k.as_str() != "path") {
                        match v {
                            Value::String(t) => form.set(k, t),
                            Value::Number(_) | Value::Bool(_) => form.set(k, &v.to_string()),
                            _ => false,
                        };
                    }
                }
            }
        }
    }

    fn browser(&mut self, ui: &mut Ui) {
        let doc = self.snapshot.doc.clone();
        let eval = self.scene.as_ref().map(|s| s.eval.clone());
        let pal = *theme::palette(self.theme);
        let mut state = std::mem::take(&mut self.browser);
        let actions = egui::ScrollArea::vertical()
            .show(ui, |ui| {
                let visible = |name: &str| self.sketch_visible(name);
                let body_visible = |name: &str| self.viewport.body_visible(name);
                let plane_visible = |name: &str| self.viewport.plane_visible(name);
                let view = browser::View {
                    pal: &pal,
                    doc: &doc,
                    eval: eval.as_deref(),
                    sketch_visible: &visible,
                    body_visible: &body_visible,
                    plane_visible: &plane_visible,
                    branch: self.snapshot.branch,
                    branches: &self.snapshot.branches,
                };
                browser::show(ui, &mut state, &view)
            })
            .inner;
        self.browser = state;
        for a in actions {
            self.browser_action(a);
        }
    }

    /// The edited sketch's plane and solved points while the open form places geometry
    /// with clicks.
    fn placing(&self) -> Option<(gusset_kernel::Frame, std::collections::BTreeMap<String, [f64; 2]>)> {
        let form = self.form.as_ref()?;
        sketch_place::clicks_needed(form.command)?;
        let name = self.editing_sketch()?;
        let s = self.scene.as_ref()?.eval.sketches.get(name)?;
        Some((s.frame, s.points.clone()))
    }

    /// A click on the sketch plane while placing: adds the point, and fills and submits the
    /// form once the command has all its points.
    fn place_click(&mut self, c: Click, tol: f64) {
        let Some(form) = self.form.as_mut() else { return };
        let Some(needed) = sketch_place::clicks_needed(form.command) else { return };
        let submit = match needed {
            Some(n) => {
                form.clicks.push(c);
                form.clicks.len() >= n && {
                    let clicks = form.clicks.clone();
                    sketch_place::fill(form, &clicks, false)
                }
            }
            None if sketch_place::closes(&form.clicks, &c, tol) => {
                let clicks = form.clicks.clone();
                sketch_place::fill(form, &clicks, true)
            }
            None => {
                form.clicks.push(c);
                let clicks = form.clicks.clone();
                // An open polyline is complete whenever the user presses OK.
                sketch_place::fill(form, &clicks, false);
                false
            }
        };
        if submit {
            let start = (form.command == "sketch.add_line").then(|| form.clicks.last().cloned()).flatten();
            let sketch = form.value("sketch").unwrap_or_default().to_string();
            self.chain = Some(Chain { command: form.command, sketch, start });
            self.apply_form();
        }
    }

    /// Sketch mode with picked sketch items and no form open: the constraints that fit them.
    fn constrain_bar(&mut self, ui: &Ui, pal: &theme::Palette) {
        if self.form.is_some() {
            return;
        }
        let Some(sketch) = self.editing_sketch().map(str::to_string) else { return };
        let ids: Vec<String> = self
            .selection
            .iter()
            .filter_map(|h| match h {
                Hit::Sketch { sketch: s, id } if *s == sketch => Some(id.clone()),
                _ => None,
            })
            .collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let offered = crate::sketch_constrain::suggestions(&refs);
        let Some(s) = crate::sketch_constrain::toolbar(ui, self.viewport.rect(), &offered, pal) else { return };
        if s.value {
            // A dimension: the form, filled in but for the value.
            self.start_command("sketch.add_constraint");
            if let Some(form) = self.form.as_mut() {
                form.set("sketch", &sketch);
                form.set("kind", s.kind);
                form.set("entities", &s.entities.join(", "));
            }
        } else {
            let args = json!({ "sketch": sketch, "kind": s.kind, "entities": s.entities });
            self.submit_command("sketch.add_constraint", args, Then::Notice);
            self.selection.clear();
        }
    }

    /// A dragged sketch point follows the pointer on the sketch plane; on release it is
    /// moved with `sketch.move_point` (the solver then keeps the constraints).
    /// Returns whether the preview changed.
    fn sketch_drag(&mut self, d: &crate::viewport::PointDrag) -> bool {
        let to = self
            .scene
            .as_ref()
            .and_then(|s| s.eval.sketches.get(&d.sketch))
            .and_then(|s| sketch_place::on_plane(&s.frame, &d.ray))
            .map(|p| p.map(|c| (c * 100.0).round() / 100.0));
        if d.done {
            self.drag_preview = None;
            if let Some([x, y]) = to {
                let args = json!({ "sketch": d.sketch, "point": d.point, "x": x, "y": y });
                self.submit_command("sketch.move_point", args, Then::Notice);
            }
            true
        } else {
            let next = to.map(|p| (d.sketch.clone(), d.point.clone(), p));
            let changed = next != self.drag_preview;
            self.drag_preview = next;
            changed
        }
    }

    /// Typed dimensions while placing (DESIGN.md §1.7): a number typed after the first
    /// click opens fields at the cursor; Enter places the point they give.
    fn typed_dimensions(&mut self, ctx: &egui::Context, tol: f64) {
        use crate::value_entry::{self as ve, Outcome};
        let Some(form) = self.form.as_ref() else {
            self.entry = None;
            return;
        };
        let Some(first) = form.clicks.first().map(|c| c.at) else { return };
        if self.entry.as_ref().is_some_and(|(e, _)| e.command != form.command) {
            self.entry = None;
        }
        if self.entry.is_none() && !ctx.egui_wants_keyboard_input() {
            let typed = ctx.input(|i| {
                i.events.iter().find_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None })
            });
            if let Some(t) = typed.filter(|t| ve::starts_value(t)) {
                self.entry = ve::start(form.command, &t).map(|e| (e, None));
            }
        }
        let Some((mut e, err)) = self.entry.take() else { return };
        let at = ctx.pointer_hover_pos().unwrap_or_else(|| self.viewport.rect().center());
        match ve::show(ctx, at, &mut e, err.as_deref()) {
            Outcome::Typing => self.entry = Some((e, err)),
            Outcome::Cancel => {}
            Outcome::Commit => {
                let cursor = self.place_cursor.as_ref().map(|c| c.at);
                let params = self.scene.as_ref().map(|s| s.eval.params.clone()).unwrap_or_default();
                match ve::point(&e, first, cursor, &params) {
                    Ok(p) => self.place_click(Click { at: p, point: None }, tol),
                    Err(msg) => self.entry = Some((e, Some(msg))),
                }
            }
        }
    }

    /// Opens the placed command again after it succeeded; a line starts at the previous
    /// line's end point (`v["points"][1]`), so the lines join.
    fn restart_placing(&mut self, ch: Chain, v: &Value) {
        self.start_command(ch.command);
        let Some(form) = self.form.as_mut() else { return };
        form.set("sketch", &ch.sketch);
        if let Some(mut start) = ch.start {
            start.point = v.pointer("/points/1").and_then(Value::as_str).map(String::from).or(start.point);
            form.clicks.push(start);
        }
    }

    /// The sketch whose regions the open form picks (it has `sketch` and `regions` fields).
    fn region_form_sketch(&self) -> Option<&str> {
        let form = self.form.as_ref()?;
        form.fields.iter().find(|f| f.name == "regions")?;
        form.value("sketch").map(str::trim).filter(|s| !s.is_empty())
    }

    /// The picked regions of the region form's sketch, as thick outlines.
    fn region_outlines(&self) -> Option<DrawSketch> {
        let name = self.region_form_sketch()?;
        let s = self.scene.as_ref()?.eval.sketches.get(name)?;
        let picked: Vec<&str> = self.form.as_ref()?.value("regions")?.split(',').map(str::trim).collect();
        let w = |p: [f64; 2]| s.frame.to_world(p).map(|c| c as f32);
        Some(DrawSketch {
            polylines: crate::sketch_regions::outlines(s, |id| picked.contains(&id))
                .into_iter()
                .map(|(_, pl)| pl.into_iter().map(w).collect())
                .collect(),
            emphasis: true,
            ..DrawSketch::default()
        })
    }

    /// A click on the region form's sketch plane inside a region toggles that region.
    fn region_click(&mut self, ray: &gusset_render::Ray) -> bool {
        let Some(name) = self.region_form_sketch() else { return false };
        let Some(s) = self.scene.as_ref().and_then(|d| d.eval.sketches.get(name)) else { return false };
        let Some(id) = sketch_place::on_plane(&s.frame, ray).and_then(|p| crate::sketch_regions::region_at(s, p))
        else {
            return false;
        };
        let Some(form) = self.form.as_mut() else { return false };
        let list = crate::sketch_regions::toggle(form.value("regions").unwrap_or_default(), &id);
        form.set("regions", &list);
        true
    }

    /// The origin planes and the datum planes, sized to the model.
    fn construction_planes(&self) -> Vec<crate::viewport::DrawPlane> {
        let b = self.viewport.scene().bounds();
        let reach = if b.is_empty() {
            0.0
        } else {
            (0..3)
                .map(|k| b.min.get(k).copied().unwrap_or(0.0).abs().max(b.max.get(k).copied().unwrap_or(0.0).abs()))
                .fold(0.0, f64::max)
        };
        let half = (reach * 0.6).max(10.0);
        let plane = |name: &str, f: &gusset_kernel::Frame| crate::viewport::DrawPlane {
            name: name.to_string(),
            origin: f.origin,
            x: f.x,
            y: f.y,
            half,
        };
        let base =
            [("XY", gusset_kernel::Frame::XY), ("XZ", gusset_kernel::Frame::XZ), ("YZ", gusset_kernel::Frame::YZ)];
        let datums = self.scene.as_ref().map(|s| s.eval.planes.clone()).unwrap_or_default();
        base.iter().map(|(n, f)| plane(n, f)).chain(datums.iter().map(|(n, f)| plane(n, f))).collect()
    }

    /// Applies what the user chose in the browser tree.
    pub fn browser_action(&mut self, action: browser::Action) {
        match action {
            browser::Action::SetSketchVisible(name, on) => {
                self.sketch_visibility.insert(name, on);
            }
            browser::Action::SetBodyVisible(name, on) => self.viewport.set_body_visible(&name, on),
            browser::Action::SetPlaneVisible(name, on) => self.viewport.set_plane_visible(&name, on),
            browser::Action::EditFeature(name) => self.edit_feature(&name),
            browser::Action::FeatureMenu(name, choice) => self.feature_choice(&name, choice),
            browser::Action::EditSketch(name) => self.edit_sketch(&name),
            browser::Action::AddParam => self.start_command("param.set"),
            browser::Action::ParamTable => self.open_param_table(),
            browser::Action::EditParam(name) => {
                self.start_command("param.set");
                let expr = self.snapshot.doc.params.iter().find(|p| p.name == name).map(|p| p.expr.to_string());
                if let (Some(form), Some(expr)) = (self.form.as_mut(), expr) {
                    form.set("name", &name);
                    form.set("expr", &expr);
                }
            }
            browser::Action::ApplyConfiguration(name) => {
                self.submit_command("configuration.apply", json!({ "name": name }), Then::Notice);
            }
            browser::Action::SetParam(name, expr) => {
                self.submit_command("param.set", json!({ "name": name, "expr": expr }), Then::Quiet);
            }
            browser::Action::SwitchBranch(id) => {
                self.submit_command("branch.switch", json!({ "branch": id.to_string() }), Then::Quiet);
            }
            browser::Action::OpenDrawing(name) => {
                self.finish_sketch();
                self.open_drawing(&name);
            }
        }
    }

    fn timeline(&mut self, ui: &mut Ui) {
        let n = self.snapshot.doc.features.len();
        let rollback = self.snapshot.rollback;
        let pal = *theme::palette(self.theme);
        // One bar: playback controls, the feature track, and the iteration-graph toggle at
        // the right end (Fusion-style).
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SPACE[2];
            // Playback moves the rollback marker one feature at a time.
            if let Some(m) = timeline::playback(ui, rollback.unwrap_or(n), n, &pal) {
                self.set_rollback(if m >= n { None } else { Some(m) });
            }
            ui.add_space(theme::SPACE[2]);
            let toggle_w = 26.0 + theme::SPACE[3];
            let track_w = (ui.available_width() - toggle_w).max(80.0);
            let eval = self.scene.as_ref().map(|s| s.eval.clone());
            let doc = &self.snapshot.doc;
            let items: Vec<timeline::Item<'_>> = doc
                .features
                .iter()
                .map(|f| timeline::Item {
                    name: &f.name,
                    keyword: f.kind.keyword(),
                    suppressed: f.suppressed,
                    status: eval.as_ref().and_then(|e| e.status(&f.name)),
                    deps: f.kind.dependencies(),
                    group: f.group.as_deref(),
                })
                .collect();
            let track = egui::Frame::new()
                .fill(pal.bg_app)
                .stroke(egui::Stroke::new(1.0, pal.border))
                .corner_radius(theme::RADIUS_MD)
                .inner_margin(egui::Margin::symmetric(4, 0));
            let moved = track
                .show(ui, |ui| {
                    ui.set_width(track_w - 10.0);
                    ui.set_height(timeline::TRACK_H);
                    if n == 0 {
                        ui.centered_and_justified(|ui| ui.weak(tr("timeline.empty")));
                        return timeline::Out::default();
                    }
                    egui::ScrollArea::horizontal()
                        .stick_to_right(true)
                        .show(ui, |ui| timeline::show(ui, &items, rollback.unwrap_or(n), &pal))
                        .inner
                })
                .inner;
            let edit = moved.edit.and_then(|i| self.snapshot.doc.features.get(i)).map(|f| f.name.clone());
            let menu = moved.menu.and_then(|(i, c)| self.snapshot.doc.features.get(i).map(|f| (f.name.clone(), c)));
            let reorder =
                moved.reorder.and_then(|(i, to)| self.snapshot.doc.features.get(i).map(|f| (f.name.clone(), to)));
            if let Some(m) = moved.marker {
                self.set_rollback(if m >= n { None } else { Some(m) });
            }
            if let Some(name) = edit {
                self.edit_feature(&name);
            }
            if let Some((name, c)) = menu {
                self.feature_choice(&name, c);
            }
            if let Some((name, index)) = reorder {
                // Refused (with the reason in the status bar) when it breaks a dependency.
                self.submit_command("feature.move", json!({ "name": name, "index": index }), Then::Notice);
            }
            let label = if self.show_iterations { tr("timeline.collapse") } else { tr("timeline.expand") };
            let on = self.show_iterations;
            let ink = icons::Ink::new(&pal);
            if crate::ribbon::icon_button_with(ui, &pal, label, Some(on), |p, r| {
                icons::paint_tool(p, r.shrink(5.0), icons::Tool::Info, ink);
            })
            .clicked()
            {
                self.show_iterations = !on;
            }
        });
        if self.show_iterations {
            ui.horizontal_wrapped(|ui| {
                for label in &self.snapshot.lineage {
                    icons::labeled(ui, Icon::Dot, pal.text_muted, RichText::new(label).small());
                }
            });
        }
    }

    /// Moves the rollback marker: `None` = all features active.
    pub fn set_rollback(&mut self, index: Option<usize>) {
        // Shown at once; the next snapshot confirms it (or the error is reported).
        self.snapshot.rollback = index;
        self.submit_command("timeline.set_rollback", json!({ "index": index }), Then::Quiet);
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
        let sketches: Vec<&DrawSketch> = self
            .scene
            .as_ref()
            .map(|s| s.sketches.iter().filter(|d| self.sketch_visible(&d.name)).collect())
            .unwrap_or_default();
        self.viewport.planes = self.construction_planes();
        if let Some(name) = &self.look_at_sketch
            && let Some(s) = self.scene.as_ref().and_then(|d| d.eval.sketches.get(name))
        {
            let f = s.frame;
            let n = [
                f.x[1] * f.y[2] - f.x[2] * f.y[1],
                f.x[2] * f.y[0] - f.x[0] * f.y[2],
                f.x[0] * f.y[1] - f.x[1] * f.y[0],
            ];
            self.viewport.request(NavRequest::LookAt { normal: n, up: f.y });
            self.look_at_sketch = None;
        }
        self.viewport.edit_sketch = self.editing_sketch().map(str::to_string);
        let placing = self.placing();
        let preview = placing.as_ref().zip(self.form.as_ref()).map(|((frame, _), form)| {
            let cursor = self.place_cursor.as_ref().map(|c| c.at);
            let w = |p: [f64; 2]| frame.to_world(p).map(|c| c as f32);
            DrawSketch {
                name: String::new(),
                polylines: sketch_place::preview(form.command, &form.clicks, cursor)
                    .into_iter()
                    .map(|pl| pl.into_iter().map(w).collect())
                    .collect(),
                points: form.clicks.iter().map(|c| w(c.at)).chain(cursor.map(w)).collect(),
                ..DrawSketch::default()
            }
        });
        let dragged = self.drag_preview.as_ref().and_then(|(sk, id, to)| {
            let s = self.scene.as_ref()?.eval.sketches.get(sk)?;
            let w = |p: [f64; 2]| s.frame.to_world(p).map(|c| c as f32);
            let ends = s.lines.iter().filter_map(|(_, a, b)| match (a == id, b == id) {
                (true, _) => s.points.get(b),
                (_, true) => s.points.get(a),
                _ => None,
            });
            Some(DrawSketch {
                polylines: ends.map(|&o| vec![w(o), w(*to)]).collect(),
                points: vec![w(*to)],
                ..DrawSketch::default()
            })
        });
        let regions = self.region_outlines();
        let mut sketches = sketches;
        sketches.extend(preview.as_ref());
        sketches.extend(dragged.as_ref());
        sketches.extend(regions.as_ref());
        if self.results.as_ref().is_none_or(|r| r.path_visible) {
            sketches.extend(self.toolpath.as_ref());
        }
        let out = self.viewport.show(ui, &pal, self.theme, &sketches, &self.selection);
        // The Render tab's photo preview, painted over the viewport.
        if self.render_tab && self.photo.on {
            let moving = self.viewport.is_interacting();
            self.photo.frame(ui, self.viewport.scene_arc(), self.viewport.camera, self.viewport.rect(), moving);
        }

        if let Some((frame, points)) = &placing {
            // Placing sketch geometry: clicks are points on the sketch plane, not picks.
            let tol = out.pixel_size * 8.0;
            let cursor = out
                .pointer_ray
                .and_then(|r| sketch_place::on_plane(frame, &r))
                .map(|p| sketch_place::snap(p, points.iter(), tol));
            if cursor != self.place_cursor {
                // The preview follows the cursor in the next frame.
                ui.ctx().request_repaint();
            }
            self.place_cursor = cursor;
            if out.clicked.is_some()
                && let Some(c) = self.place_cursor.clone()
            {
                self.place_click(c, tol);
            }
            self.typed_dimensions(ui.ctx(), tol);
            return;
        }
        self.place_cursor = None;
        let empty = self.viewport.scene().is_empty() && sketches.is_empty();
        self.constrain_bar(ui, &pal);
        if let Some(d) = &out.drag
            && self.sketch_drag(d)
        {
            ui.ctx().request_repaint();
        }
        // With a form that takes regions, a click inside one toggles it.
        let region_picked = out.clicked.is_some() && out.pointer_ray.is_some_and(|r| self.region_click(&r));
        if let Some(at) = out.menu_at {
            self.marking = Some(at);
            self.quick = None;
        }
        // A right drag flicked toward a marking-menu item runs it without the menu.
        if let Some(dir) = out.flick {
            let tools =
                crate::quick_menus::tools(self.editing_sketch().is_some(), self.last_command, self.quick_custom());
            if let Some(t) = crate::quick_menus::flicked(dir, tools.len()).and_then(|i| tools.get(i).copied()) {
                self.run_quick_tool(t);
            }
        }
        // Alt+click: choose among everything under the pointer.
        let alt = ui.input(|i| i.modifiers.alt);
        if alt
            && !region_picked
            && let Some(at) = out.click_at
        {
            let all = self.viewport.pick_all(at);
            if all.len() > 1 {
                self.picker = Some((at, all));
                return;
            }
        }
        if let Some(clicked) = out.clicked.filter(|_| !region_picked) {
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
            // With the joint form open, picks fill its frames; other forms take the picks
            // their fields want (edges for a fillet, faces for a shell, a plane, ...).
            if let Some(mut form) = self.form.take() {
                if form.command == "feature.joint" {
                    if let Some(h) = self.selection.last() {
                        pick_joint_frame(&mut form, h);
                    }
                } else if form.command != "feature.edit" {
                    // (An edit form's fields are statement text: picks leave them, review R9.)
                    self.fill_picks(&mut form, true);
                }
                self.form = Some(form);
            }
        }
        if empty {
            self.empty_state(ui);
        }
    }

    fn empty_state(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let area = egui::Rect::from_center_size(rect.center(), egui::vec2(320.0, 180.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
            ui.vertical_centered(|ui| {
                ui.heading(tr("empty.title"));
                ui.add_space(theme::SPACE[3]);
                if theme::primary_button(ui, tr("empty.sketch"), 200.0).clicked() {
                    self.start_command("sketch.create");
                }
                if ui.add(egui::Button::new(tr("empty.import")).min_size(egui::vec2(200.0, 0.0))).clicked() {
                    self.start_command("import.file");
                }
                if ui.add(egui::Button::new(tr("empty.code")).min_size(egui::vec2(200.0, 0.0))).clicked() {
                    self.toggle_code();
                }
                ui.add_space(theme::SPACE[2]);
                ui.weak(tr("empty.hint"));
            });
        });
    }

    fn properties_panel(&mut self, ui: &mut egui::Ui) {
        let Some(form) = self.form.as_mut() else { return };
        let mut close = false;
        ui.horizontal(|ui| {
            ui.strong(form.title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.small_button(tr("dialog.close")).clicked();
            });
        });
        ui.separator();
        let submit = egui::ScrollArea::vertical().show(ui, |ui| forms::show(ui, form)).inner;
        if submit {
            self.apply_form();
        }
        if close {
            self.form = None;
        }
    }

    fn palette_window(&mut self, ctx: &egui::Context) {
        let Some(mut query) = self.palette.take() else { return };
        let mut chosen = None;
        let mut close = false;
        let doc = &self.snapshot.doc;
        let recent = &self.recent;
        egui::Modal::new(egui::Id::new("palette")).show(ctx, |ui| {
            ui.set_width(460.0);
            let r = ui
                .add(egui::TextEdit::singleline(&mut query).hint_text(tr("search.hint")).desired_width(f32::INFINITY));
            r.request_focus();
            let matches = crate::palette::entries(&query, doc, recent);
            if matches.is_empty() {
                ui.weak(tr("search.none"));
            }
            for (i, e) in matches.iter().take(12).enumerate() {
                let label = format!("{}  —  {}", e.label, e.detail);
                if ui.selectable_label(i == 0, label).clicked() {
                    chosen = Some(e.target.clone());
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                chosen = matches.first().map(|e| e.target.clone());
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
        if !close && chosen.is_none() {
            self.palette = Some(query);
        }
        if let Some(t) = chosen {
            self.palette_choice(t);
        }
    }

    /// Opens the parameters table with the document's parameters and an empty row.
    pub fn open_param_table(&mut self) {
        let mut rows: Vec<(String, String, String)> =
            self.snapshot.doc.params.iter().map(|p| (p.name.clone(), p.expr.to_string(), p.expr.to_string())).collect();
        rows.push((String::new(), String::new(), String::new()));
        self.param_table = Some(rows);
    }

    /// The parameters table (DESIGN.md §1.8): every expression editable; Apply sets the
    /// changed ones (and a new one from the last row) in one step.
    fn param_table_window(&mut self, ctx: &egui::Context) {
        let Some(mut rows) = self.param_table.take() else { return };
        let values = self.scene.as_ref().map(|s| s.eval.params.clone()).unwrap_or_default();
        let (mut open, mut apply) = (true, false);
        egui::Window::new(tr("params.title")).open(&mut open).collapsible(false).show(ctx, |ui| {
            egui::Grid::new("param_table").num_columns(3).striped(true).show(ui, |ui| {
                let last = rows.len().saturating_sub(1);
                for (i, (name, expr, _)) in rows.iter_mut().enumerate() {
                    if i == last {
                        ui.add(egui::TextEdit::singleline(name).hint_text(tr("params.new")).desired_width(110.0));
                    } else {
                        ui.monospace(name.as_str());
                    }
                    ui.add(egui::TextEdit::singleline(expr).desired_width(180.0).font(egui::TextStyle::Monospace));
                    let shown = match values.get(name.as_str()) {
                        Some(Ok(q)) => q.to_string(),
                        Some(Err(e)) => format!("error: {e}"),
                        None => String::new(),
                    };
                    ui.weak(shown);
                    ui.end_row();
                }
            });
            if ui.button(tr("params.apply")).clicked() {
                apply = true;
            }
        });
        if apply {
            let changed: serde_json::Map<String, Value> = rows
                .iter()
                .filter(|(name, expr, was)| !name.trim().is_empty() && !expr.trim().is_empty() && expr != was)
                .map(|(name, expr, _)| (name.trim().to_string(), Value::String(expr.trim().to_string())))
                .collect();
            if !changed.is_empty() {
                self.submit_command("param.set_many", json!({ "values": changed }), Then::Notice);
            }
            open = false;
        }
        if open {
            self.param_table = Some(rows);
        }
    }

    /// Carries out a command-search choice.
    fn palette_choice(&mut self, t: crate::palette::Target) {
        use crate::palette::{Setting as S, Target as T};
        match t {
            T::Command(id) => self.start_command(id),
            T::Feature(name) => self.edit_feature(&name),
            T::Param(name) => self.browser_action(browser::Action::EditParam(name)),
            T::Recent(path) => self.request(Pending::Open(PathBuf::from(path))),
            T::Setting(s) => {
                match s {
                    S::Theme(k) => self.theme = k,
                    S::FullScreen => self.full_screen = !self.full_screen,
                    S::Wireframe => self.viewport.wireframe = !self.viewport.wireframe,
                    S::Section => {
                        self.viewport.section =
                            if self.viewport.section.is_some() { None } else { Some(Default::default()) };
                    }
                    S::CodeView => self.toggle_code(),
                    S::CommandLine => {
                        self.cmdline.visible = !self.cmdline.visible;
                        self.cmdline.focus = self.cmdline.visible;
                    }
                    S::Nav(p) => self.viewport.nav_preset = p,
                    S::ParamTable => self.open_param_table(),
                }
                self.save_prefs();
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else { return };
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(theme::SPACE[5] as i8));
        let modal = egui::Modal::new(egui::Id::new("dialog")).frame(frame);
        match dialog {
            Dialog::Unsaved { pending, save_failed } => {
                let mut action = None;
                modal.show(ctx, |ui| {
                    ui.heading(match pending {
                        Pending::Close => tr("close.title"),
                        Pending::Open(_) => tr("open.unsaved_title"),
                    });
                    ui.label(tr("close.body"));
                    let name = self.project_path.as_ref().map_or_else(
                        || "Untitled".to_string(),
                        |p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    );
                    ui.set_min_width(400.0);
                    ui.label(format!("• {name}"));
                    if let Some(e) = &save_failed {
                        ui.colored_label(ui.visuals().error_fg_color, format!("{} {e}", tr("close.save_failed")));
                    }
                    ui.add_space(theme::SPACE[4]);
                    ui.horizontal(|ui| {
                        let (save, discard) = match pending {
                            Pending::Close => (tr("close.save"), tr("close.discard")),
                            Pending::Open(_) => (tr("open.save"), tr("open.discard")),
                        };
                        if theme::primary_button(ui, save, 72.0).clicked() {
                            action = Some(0);
                        }
                        if ui.button(discard).clicked() {
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
                        self.save_then(AfterSave::Then(pending));
                    }
                    Some(1) => self.proceed(pending, true),
                    Some(_) => {}
                    None => self.dialog = Some(Dialog::Unsaved { pending, save_failed }),
                }
            }
            Dialog::Busy { pending } => {
                let mut action = None;
                modal.show(ctx, |ui| {
                    ui.heading(tr("busy.title"));
                    ui.label(tr("busy.body"));
                    ui.horizontal(|ui| {
                        if ui.button(tr("busy.wait")).clicked() {
                            action = Some(true);
                        }
                        if ui.button(tr("dialog.cancel")).clicked() {
                            action = Some(false);
                        }
                    });
                });
                match action {
                    Some(true) => self.after_idle = Some(pending),
                    Some(false) => {}
                    None => {
                        if !self.busy() && self.host.try_with_session(|_| ()).is_ok_and(|r| r.is_some()) {
                            // The work finished while the dialog was up: go on.
                            self.request(pending);
                        } else {
                            if !self.busy() {
                                // Someone else's command (an agent) holds the model thread and
                                // may not broadcast when it ends: look again shortly. Our own
                                // jobs wake the window when they finish.
                                ctx.request_repaint_after(BUSY_RETRY);
                            }
                            self.dialog = Some(Dialog::Busy { pending });
                        }
                    }
                }
            }
            Dialog::Overwrite { purpose, path } => {
                let mut action = None;
                modal.show(ctx, |ui| {
                    ui.heading(tr("overwrite.title"));
                    ui.label(format!("{}\n{}", path.to_string_lossy(), tr("overwrite.body")));
                    ui.horizontal(|ui| {
                        if ui.button(tr("overwrite.replace")).clicked() {
                            action = Some(true);
                        }
                        if ui.button(tr("dialog.cancel")).clicked() {
                            action = Some(false);
                        }
                    });
                });
                match action {
                    Some(true) => self.export(purpose, &path, true),
                    Some(false) => {
                        let text = path.to_string_lossy().into_owned();
                        self.dialog = Some(Dialog::Path { purpose, text, error: None });
                    }
                    None => self.dialog = Some(Dialog::Overwrite { purpose, path }),
                }
            }
            Dialog::Unrecovered { mut items } => {
                let mut action: Option<(usize, bool)> = None;
                let mut later = false;
                modal.show(ctx, |ui| {
                    ui.set_max_width(520.0);
                    ui.heading(tr("unrecovered.title"));
                    ui.label(tr("unrecovered.body"));
                    for (i, it) in items.iter().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            let name = it.path.as_ref().map_or_else(
                                || tr("unrecovered.untitled").to_string(),
                                |p| plain(&p.to_string_lossy(), 200),
                            );
                            let when = it
                                .modified
                                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| format!(" · {}", crate::agents::clock_pub(d.as_secs())))
                                .unwrap_or_default();
                            let damaged = if it.recovery_seq == 0 { tr("unrecovered.damaged") } else { "" };
                            ui.label(format!("{name}{when}{damaged}"));
                            if ui.button(tr("unrecovered.recover")).clicked() {
                                action = Some((i, true));
                            }
                            if ui.button(tr("unrecovered.discard")).clicked() {
                                action = Some((i, false));
                            }
                        });
                    }
                    if ui.button(tr("unrecovered.later")).clicked() {
                        later = true;
                    }
                });
                match action {
                    Some((i, recover)) if i < items.len() => {
                        let it = items.remove(i);
                        if !items.is_empty() {
                            self.dialog = Some(Dialog::Unrecovered { items });
                        }
                        if recover {
                            self.recover_pending(it);
                        } else if let Some(d) = &self.dirs
                            && let Err(e) = gusset_project::discard_pending_recovery(d, it.id)
                        {
                            self.notice = Some(Notice { text: e.to_string(), hint: None, error: true });
                        }
                    }
                    _ if later => {}
                    _ => self.dialog = Some(Dialog::Unrecovered { items }),
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
                            session.importer = importer();
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
                        // Keep both (ADR 0007): the recovered work, with all its branches and
                        // iterations, goes to "<name> (recovered).gus" as a separate project;
                        // the recovery file is removed only once that copy is safely written.
                        let dirs = self.dirs.clone();
                        self.submit(
                            move |_, p| -> Result<PathBuf, String> {
                                let p = p.ok_or("No project")?;
                                let dirs = dirs.ok_or("No data folder")?;
                                let original = p.path().ok_or("The project has no file")?.to_path_buf();
                                let path = keep_both(&original, &recovered, &dirs)?;
                                p.discard_recovery();
                                Ok(path)
                            },
                            |r| Done::KeptBoth(r.map_err(|e| e.message).and_then(|x| x)),
                        );
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
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| ui.label(&body));
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
    pub(crate) fn path_action(&mut self, purpose: PathPurpose, text: &str) -> Result<(), String> {
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
        match purpose {
            PathPurpose::SaveAs => {
                self.save_as(abs);
                Ok(())
            }
            PathPurpose::ImportSettings => {
                if !abs.is_file() {
                    return Err(format!("{} {}", tr("open.not_found"), abs.to_string_lossy()));
                }
                let p = crate::prefs::load(&abs);
                self.apply_prefs(p);
                self.save_prefs();
                self.notice = Some(Notice { text: tr("settings.imported").into(), hint: None, error: false });
                Ok(())
            }
            PathPurpose::Open => {
                if !abs.is_file() {
                    return Err(format!("{} {}", tr("open.not_found"), abs.to_string_lossy()));
                }
                self.request(Pending::Open(abs));
                Ok(())
            }
            PathPurpose::ExportStep
            | PathPurpose::ExportStl
            | PathPurpose::Export3mf
            | PathPurpose::ExportMesh(_)
            | PathPurpose::Render
            | PathPurpose::ExportSettings => {
                if abs.exists() {
                    // Never replace a file without asking (review L7).
                    self.dialog = Some(Dialog::Overwrite { purpose, path: abs });
                } else {
                    self.export(purpose, &abs, false);
                }
                Ok(())
            }
        }
    }

    /// Starts an export to `path`; a failure re-opens the path dialog with the error.
    pub(crate) fn export(&mut self, purpose: PathPurpose, path: &std::path::Path, overwrite: bool) {
        if purpose == PathPurpose::Render {
            self.render_to(path);
            return;
        }
        if purpose == PathPurpose::ExportSettings {
            let mut p = self.prefs();
            p.recent.clear(); // paths stay private
            self.notice = Some(match crate::prefs::save(path, &p) {
                Ok(()) => Notice {
                    text: format!("{} {}", tr("settings.exported"), path.to_string_lossy()),
                    hint: None,
                    error: false,
                },
                Err(e) => Notice { text: e, hint: None, error: true },
            });
            return;
        }
        let (id, extra) = match purpose {
            PathPurpose::ExportStl => ("export.mesh", json!({ "format": "stl" })),
            PathPurpose::Export3mf => ("export.mesh", json!({ "format": "3mf" })),
            PathPurpose::ExportMesh(f) => ("export.mesh", json!({ "format": f })),
            _ => ("export.step", json!({})),
        };
        let text = path.to_string_lossy().into_owned();
        let mut args = json!({ "path": text, "overwrite": overwrite });
        if let (Some(a), Some(e)) = (args.as_object_mut(), extra.as_object()) {
            a.extend(e.clone());
        }
        self.submit_command(id, args, Then::PathDialog(purpose, text));
    }

    /// Path-traces the current view (`render_quality`) on a worker thread and
    /// writes it as PNG; [`Self::poll_render`] reports the outcome.
    fn render_to(&mut self, path: &std::path::Path) {
        let scene = self.viewport.scene().clone();
        let camera = self.viewport.camera;
        let path = path.to_path_buf();
        let (size, samples) = self.render_quality;
        let (tx, rx) = std::sync::mpsc::channel();
        self.render_job = Some(rx);
        self.notice = Some(Notice { text: tr("render.running").into(), hint: None, error: false });
        std::thread::spawn(move || {
            let out = gusset_render::render_traced(
                &scene,
                &camera,
                size,
                gusset_render::TraceSettings { samples, ..Default::default() },
            )
            .map_err(|e| e.to_string())
            .and_then(|img| img.to_png().map_err(|e| e.to_string()))
            .and_then(|png| std::fs::write(&path, png).map_err(|e| e.to_string()))
            .map(|()| path);
            let _ = tx.send(out);
        });
    }

    /// The outcome of a finished render, as a notice.
    fn poll_render(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.render_job else { return };
        match rx.try_recv() {
            Ok(r) => {
                self.render_job = None;
                self.notice = Some(match r {
                    Ok(p) => Notice {
                        text: format!("{} {}", tr("render.done"), p.to_string_lossy()),
                        hint: None,
                        error: false,
                    },
                    Err(e) => Notice { text: format!("{}: {e}", tr("render.failed")), hint: None, error: true },
                });
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(200))
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.render_job = None,
        }
    }

    fn close_now(&mut self, discard_recovery: bool) {
        self.allow_close = true;
        self.shutdown(discard_recovery);
        self.send_close = true;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        App::ui(self, ui);
    }

    fn on_exit(&mut self) {
        // Unsaved work survives a forced exit through the recovery slot.
        self.maybe_snapshot(true);
        // Stop the MCP endpoint (deletes the connection file) and deny pending approvals.
        self.agents.shutdown();
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
/// How long typing must pause before a live preview is built (seconds).
const PREVIEW_DELAY: f64 = 0.3;

/// Tint of previewed bodies (sRGB): the accent family, so a preview never passes for the
/// model.
const PREVIEW_TINT: gusset_render::Srgb = gusset_render::Srgb(0x8FB8E8);

/// A live preview of a feature or generator command: what it would build, tessellated
/// and tinted. Nothing is committed (`gusset_commands::preview`).
fn build_preview(
    s: &mut Session,
    id: &str,
    args: Value,
) -> Result<(gusset_render::Scene, Vec<BodyInfo>), CommandError> {
    let eval = gusset_commands::preview(s, &App::ctx(), id, args)?;
    let (mut render, bodies, _) = tessellated(s, &eval)?;
    let ids: Vec<gusset_render::MeshId> = render.meshes().map(|(m, _)| m).collect();
    for m in ids {
        render.set_material(m, gusset_render::Material { color: PREVIEW_TINT, ..Default::default() });
    }
    Ok((render, bodies))
}

fn build_scene(s: &mut Session, _: Option<&mut Project>) -> Result<SceneData, CommandError> {
    let b = s.current_branch(&Actor::User);
    let (head, rollback) = {
        let br = s.history().branch(b).map_err(CommandError::history)?;
        (br.head, br.rollback)
    };
    let eval = s.evaluate(&Actor::User)?;
    let (mut render, bodies, mesh_errors) = tessellated(s, &eval)?;
    let doc = s.document(&Actor::User).ok();
    // Body colours and finishes from the document (ADR 0045); the ids are body positions.
    if let Some(d) = &doc {
        for (i, b) in bodies.iter().enumerate() {
            if let Some(a) = d.appearance(&b.name) {
                let id = gusset_render::MeshId(i as u64);
                render.set_material(id, gusset_mcp::render_material(a));
                apply_face_colors(&mut render, id, &b.face_names, a);
            }
        }
        // Imported mesh files' own face colours (3MF triangles coloured one by one).
        for m in eval.meshes.iter().filter(|m| !m.face_colors.is_empty()) {
            if let Some(i) = bodies.iter().position(|b| b.name == m.name) {
                for &(f, c) in &m.face_colors {
                    render.set_face_color(gusset_render::MeshId(i as u64), f, Some(gusset_render::Srgb(c)));
                }
            }
        }
    }
    let def = |name: &str| {
        doc.as_ref()?.features.iter().find(|f| f.name == name).and_then(|f| match &f.kind {
            gusset_doc::FeatureKind::Sketch(d) => Some(d),
            _ => None,
        })
    };
    let sketches = eval
        .sketches
        .iter()
        .map(|(name, s)| {
            let mut d = sketch_drawing(name, s);
            if let Some(def) = def(name) {
                d.labels = constraint_labels(def, s);
            }
            d
        })
        .collect();
    Ok(SceneData { key: (head, rollback), render, bodies, mesh_errors, sketches, eval })
}

/// An appearance's face colours on the faces the body still has (by face name).
pub(crate) fn apply_face_colors(
    scene: &mut gusset_render::Scene,
    id: gusset_render::MeshId,
    face_names: &[String],
    a: &gusset_doc::Appearance,
) {
    for (i, c) in gusset_commands::commands::face_color_indices(a, face_names) {
        scene.set_face_color(id, i, Some(gusset_render::Srgb(c)));
    }
}

/// The bodies and reference meshes of an evaluation as a render scene.
fn tessellated(
    s: &Session,
    eval: &Evaluation,
) -> Result<(gusset_render::Scene, Vec<BodyInfo>, Vec<String>), CommandError> {
    let tp = TessellationParams::default();
    let mut meshes: Vec<(BodyInfo, Arc<gusset_kernel::Mesh>)> = Vec::with_capacity(eval.bodies.len());
    for body in &eval.bodies {
        // Through the session's cache: unchanged bodies are not tessellated again, and
        // keep their display geometry (`GEOMETRY`).
        let mesh = match s.tessellate_cached(&body.shape, &tp) {
            Ok(m) => m,
            // The kernel worker crashed or was stopped: the other bodies' shapes went with
            // it. Keep the scene on screen and say why (review H3).
            Err(e) if matches!(e.kind, KernelErrorKind::Crashed | KernelErrorKind::Cancelled) => {
                let code = if e.kind == KernelErrorKind::Crashed { "kernel_crashed" } else { "cancelled" };
                return Err(CommandError::new(code, format!("showing {} failed: {}", body.name, e.message))
                    .hint("the view keeps the last picture; edit or rebuild the model to try again"));
            }
            Err(_) => continue,
        };
        let info = BodyInfo {
            name: body.name.clone(),
            face_names: body.names.faces.iter().map(ToString::to_string).collect(),
            edge_names: body.names.edges.iter().map(ToString::to_string).collect(),
        };
        meshes.push((info, mesh));
    }
    // Imported reference meshes (STL/3MF/OBJ) are drawn and pickable as one face each.
    for m in &eval.meshes {
        let mut mesh = m.mesh.clone();
        if mesh.face_ranges.is_empty() {
            mesh.face_ranges = std::iter::once(0..u32::try_from(mesh.indices.len() / 3).unwrap_or(u32::MAX)).collect();
        }
        let info =
            BodyInfo { name: m.name.clone(), face_names: vec![format!("{}.mesh", m.name)], edge_names: Vec::new() };
        meshes.push((info, Arc::new(mesh)));
    }
    Ok(crate::viewport::build_scene_cached(&meshes, &GEOMETRY))
}

/// Display geometry of the bodies on screen, reused across rebuilds (PERFORMANCE.md).
static GEOMETRY: std::sync::LazyLock<crate::viewport::GeometryCache> =
    std::sync::LazyLock::new(crate::viewport::GeometryCache::default);

/// A placed sketch command to open again after its result (see `App::place_click`).
#[derive(Debug, Clone)]
struct Chain {
    command: &'static str,
    sketch: String,
    /// A line's end, where the next line starts.
    start: Option<Click>,
}

/// Closes a project the window is done with. Changes the user did not choose to
/// discard stay in its recovery file (the lock is still released), so nothing that
/// arrived after the user's decision, such as an agent edit, is lost.
fn retire_project(session: &Session, mut project: Project, discard: bool) {
    if session.dirty && !discard {
        let _ = project.snapshot(session.history());
        drop(project);
    } else {
        project.close();
    }
}

/// "Keep both": writes `recovered` next to `original` as "<name> (recovered).gus"
/// (or "(recovered 2)", ...), a separate project with its own id, and returns its path.
pub fn keep_both(original: &std::path::Path, recovered: &History, dirs: &DataDirs) -> Result<PathBuf, String> {
    let dir = original.parent().unwrap_or_else(|| std::path::Path::new("."));
    let stem = original.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
    let id = gusset_store::paths::ProjectId::new_random().map_err(|e| e.to_string())?;
    let meta = gusset_project::ProjectMeta { id, save_seq: 1, base_seq: 1, created: now_unix() };
    let bytes = gusset_project::encode(recovered, &meta).map_err(|e| e.to_string())?;
    // Claim the name by creating it exclusively, so another writer (or a second Gusset
    // window) can never be overwritten between choosing the name and writing it
    // (code re-review N7); the durable write then replaces the empty placeholder.
    let mut path = None;
    for i in 1..1000 {
        let name = if i == 1 { format!("{stem} (recovered).gus") } else { format!("{stem} (recovered {i}).gus") };
        let p = dir.join(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(_) => {
                path = Some(p);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    let path = path.ok_or("Too many recovered copies next to the project")?;
    let fs = gusset_store::fs::RealFs::default();
    if let Err(e) = gusset_store::durable::save_project(&fs, &path, &fs, None, &bytes) {
        let _ = std::fs::remove_file(&path);
        return Err(e.to_string());
    }
    let _ = gusset_store::housekeeping::register(dirs, id, &path);
    Ok(path)
}

/// Sketches used as a profile or sweep path by a solid feature (hidden by default, as in
/// Fusion and Onshape).
pub fn consumed_sketches(doc: &gusset_doc::Document) -> std::collections::BTreeSet<String> {
    use gusset_doc::{FeatureKind, PathRef};
    doc.features
        .iter()
        .flat_map(|f| match &f.kind {
            FeatureKind::Extrude(e) => vec![e.profile.sketch.clone()],
            FeatureKind::Revolve(r) => vec![r.profile.sketch.clone()],
            FeatureKind::Sweep(d) => {
                let mut v = vec![d.profile.sketch.clone()];
                if let PathRef::Sketch { sketch, .. } = &d.path {
                    v.push(sketch.clone());
                }
                v
            }
            FeatureKind::Loft(d) => d.profiles.iter().map(|p| p.sketch.clone()).collect(),
            FeatureKind::Hole(d) => match &d.sites {
                gusset_doc::HoleSites::Sketch(s) => vec![s.clone()],
                gusset_doc::HoleSites::Points(_) => Vec::new(),
            },
            _ => Vec::new(),
        })
        .collect()
}

/// The colours of a stress picture's bands, low to high (blue, cyan, green, yellow, red).
const STRESS_COLOURS: [(u32, &str); 8] = [
    (0x2c_4b_d6, "blue"),
    (0x2a_8c_e0, "light blue"),
    (0x1f_b8_c4, "cyan"),
    (0x3c_c2_6a, "green"),
    (0xa6_d0_35, "yellow-green"),
    (0xf2_c9_1c, "yellow"),
    (0xf0_7c_1a, "orange"),
    (0xd9_2b_2b, "red"),
];

/// A `sim.static` or `sim.modal` result's bands (`[{upto, quads}]`) as one coloured mesh per
/// band, and the legend ("red: 8.75 to 10 MPa").
fn stress_scene(bands: &Value, unit: &str) -> (gusset_render::Scene, Vec<BodyInfo>, Vec<crate::results_panel::Band>) {
    bands_scene(bands, unit, &STRESS_COLOURS)
}

/// Draft analysis: negative, insufficient, positive.
const DRAFT_COLOURS: [(u32, &str); 3] = [(0xd6_3a_2c, "red"), (0xe8_c2_2a, "yellow"), (0x3c_b0_4a, "green")];

/// Coloured quads in bands (`[{upto?, quads}]`), one colour per band from `colours`.
fn bands_scene(
    bands: &Value,
    unit: &str,
    colours: &[(u32, &str)],
) -> (gusset_render::Scene, Vec<BodyInfo>, Vec<crate::results_panel::Band>) {
    let mut scene = gusset_render::Scene::new();
    let mut bodies = Vec::new();
    let mut legend = Vec::new();
    let mut from = 0.0;
    for (i, b) in bands.as_array().map(Vec::as_slice).unwrap_or_default().iter().enumerate() {
        let upto = b["upto"].as_f64().unwrap_or(0.0);
        let (rgb, name) = colours.get(i).copied().unwrap_or((0x80_80_80, "grey"));
        legend.push(crate::results_panel::Band { rgb, text: format!("{name}: {from} to {upto} {unit}") });
        from = upto;
        let mut mesh = gusset_kernel::Mesh::default();
        for q in b["quads"].as_array().map(Vec::as_slice).unwrap_or_default() {
            let c: Vec<f32> = q
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|v| v.as_f64().map(|x| x as f32))
                .collect();
            let [ax, ay, az, bx, by, bz, cx, cy, cz, dx, dy, dz] = c.as_slice() else { continue };
            let (u, w) = ([bx - ax, by - ay, bz - az], [cx - ax, cy - ay, cz - az]);
            let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            let base = mesh.positions.len() as u32;
            mesh.positions.extend([[*ax, *ay, *az], [*bx, *by, *bz], [*cx, *cy, *cz], [*dx, *dy, *dz]]);
            mesh.normals.extend([[n[0] / len, n[1] / len, n[2] / len]; 4]);
            mesh.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        if mesh.indices.is_empty() {
            continue;
        }
        mesh.face_ranges = std::iter::once(0..(mesh.indices.len() / 3) as u32).collect();
        let id = gusset_render::MeshId(bodies.len() as u64);
        if scene.insert_mesh(id, &mesh).is_ok() {
            scene.set_material(id, gusset_render::Material { color: gusset_render::Srgb(rgb), ..Default::default() });
            bodies.push(BodyInfo {
                name: format!("up to {upto} {unit}"),
                face_names: vec![name.to_string()],
                edge_names: Vec::new(),
            });
        }
    }
    (scene, bodies, legend)
}

/// A `cam.toolpath` path (polylines of [x, y, z]) as an overlay.
pub(crate) fn toolpath_drawing(path: &Value, tint: egui::Color32) -> DrawSketch {
    let point = |v: &Value| -> Option<[f32; 3]> {
        let a = v.as_array()?;
        let c = |i: usize| a.get(i).and_then(Value::as_f64).map(|x| x as f32);
        Some([c(0)?, c(1)?, c(2)?])
    };
    let polylines = path
        .as_array()
        .map(|ls| ls.iter().filter_map(|l| Some(l.as_array()?.iter().filter_map(point).collect())).collect())
        .unwrap_or_default();
    DrawSketch { name: "toolpath".into(), polylines, tint: Some(tint), ..DrawSketch::default() }
}

/// Builds polylines for a solved sketch on its plane.
fn sketch_drawing(name: &str, s: &gusset_eval::SketchResult) -> DrawSketch {
    let w = |p: [f64; 2]| {
        let v = s.frame.to_world(p);
        [v[0] as f32, v[1] as f32, v[2] as f32]
    };
    let pt = |id: &str| s.points.get(id).copied();
    let mut d = DrawSketch { name: name.to_string(), ..DrawSketch::default() };
    for (id, a, b) in &s.lines {
        if let (Some(a), Some(b)) = (pt(a), pt(b)) {
            d.polylines.push(vec![w(a), w(b)]);
            d.ids.push(id.clone());
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
            d.ids.push(id.clone());
        }
    }
    for (id, c, st, en) in &s.arcs {
        if let (Some(c), Some(st), Some(en)) = (pt(c), pt(st), pt(en)) {
            let r = (st[0] - c[0]).hypot(st[1] - c[1]);
            let a0 = (st[1] - c[1]).atan2(st[0] - c[0]);
            let mut a1 = (en[1] - c[1]).atan2(en[0] - c[0]);
            if a1 <= a0 {
                a1 += std::f64::consts::TAU;
            }
            d.polylines.push(arc(c, r, a0, a1));
            d.ids.push(id.clone());
        }
    }
    for (id, pts) in &s.splines {
        d.polylines.push(pts.iter().map(|&p| w(p)).collect());
        d.ids.push(id.clone());
    }
    d.points = s.points.values().map(|&p| w(p)).collect();
    d.point_ids = s.points.keys().cloned().collect();
    d
}

/// Constraint glyphs (ASCII: the UI fonts lack the drafting symbols) and dimension values
/// at the entities they constrain, in world coordinates.
fn constraint_labels(def: &gusset_doc::SketchDef, s: &gusset_eval::SketchResult) -> Vec<([f32; 3], String)> {
    use gusset_doc::SkConstraintKind as K;
    let pt = |id: &str| s.points.get(id).copied();
    // Where an entity's label goes: a point itself, a line's middle, a circle's center.
    let anchor = |id: &str| -> Option<[f64; 2]> {
        if let Some(p) = pt(id) {
            return Some(p);
        }
        if let Some((_, a, b)) = s.lines.iter().find(|(l, _, _)| l == id) {
            let (a, b) = (pt(a)?, pt(b)?);
            return Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]);
        }
        let center = s
            .circles
            .iter()
            .find(|(c, _)| c == id)
            .map(|(_, c)| c)
            .or_else(|| s.arcs.iter().find(|(a, ..)| a == id).map(|(_, c, ..)| c))?;
        pt(center)
    };
    def.constraints
        .iter()
        .filter_map(|c| {
            let text = match c.kind {
                K::Horizontal => "H".to_string(),
                K::Vertical => "V".to_string(),
                K::Parallel => "||".to_string(),
                K::Perpendicular => "_|_".to_string(),
                K::Equal => "=".to_string(),
                K::Tangent => "T".to_string(),
                K::Fixed => "Fix".to_string(),
                K::Midpoint => "M".to_string(),
                K::Symmetric => "S".to_string(),
                // Coincident and on-curve points sit on their geometry: no label.
                K::Coincident | K::PointOnLine | K::PointOnCircle => return None,
                K::Radius => format!("R {}", c.value.as_ref()?),
                K::Diameter => format!("D {}", c.value.as_ref()?),
                _ => c.value.as_ref()?.to_string(),
            };
            let anchors: Vec<[f64; 2]> = c.entities.iter().filter_map(|e| anchor(e)).collect();
            let n = anchors.len() as f64;
            if n == 0.0 {
                return None;
            }
            let mid = anchors.iter().fold([0.0, 0.0], |m, a| [m[0] + a[0] / n, m[1] + a[1] / n]);
            Some((s.frame.to_world(mid).map(|v| v as f32), text))
        })
        .collect()
}

/// Whether a command only reads the model and reports a result (measure, overview).
/// A command result on one line, cut to `max` characters.
fn clip_result(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.push_str("...");
    }
    out
}

fn inspects(id: &str) -> bool {
    gusset_commands::find(id).is_some_and(|s| s.category == gusset_commands::Category::Inspect)
}

/// What a result asks the user to notice: CAM `warnings` (drill sizes and the like),
/// faces a move dragged along (`also_moved`) and a cut-short preview path.
pub(crate) fn result_notes(v: &Value) -> Vec<String> {
    let list = |key: &str| -> Vec<String> {
        v.get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string)).collect())
            .unwrap_or_default()
    };
    let mut notes: Vec<String> = list("warnings");
    let moved = list("also_moved");
    if !moved.is_empty() {
        notes.push(format!("also moved: {}", moved.join(", ")));
    }
    if v.get("path_truncated") == Some(&Value::Bool(true)) {
        notes.push("the previewed toolpath is cut short".into());
    }
    notes
}

/// "Title: done", with the result's notes after it.
pub(crate) fn done_text(title: &str, v: &Value) -> String {
    let notes = result_notes(v);
    if notes.is_empty() { format!("{title}: done") } else { format!("{title}: done. {}", notes.join("; ")) }
}

/// The first line of a simulation result: shell results say so and give the element size.
pub(crate) fn simulation_summary(v: &Value) -> Option<String> {
    if v.get("method").and_then(Value::as_str)? != "shell" {
        return None;
    }
    let size = v.get("element_mm").and_then(Value::as_f64).map_or(String::new(), |h| format!(", elements {h} mm"));
    let count = v.get("elements").and_then(Value::as_u64).map_or(String::new(), |n| format!(", {n} elements"));
    Some(format!("Sheet body: shell model{size}{count}"))
}

/// A command result as readable text for the inspection dialog.
fn inspect_text(v: &Value) -> String {
    const MAX_LINES: usize = 60;
    let text = serde_json::to_string_pretty(v).unwrap_or_default();
    let mut lines: Vec<&str> = text.lines().collect();
    let more = lines.len().saturating_sub(MAX_LINES);
    lines.truncate(MAX_LINES);
    let mut out = lines.join("\n");
    if more > 0 {
        out.push_str(&format!("\n… {more} more lines"));
    }
    out
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

/// The notice for a `query.components` result, if any component has a newer source.
fn component_updates(v: &Value) -> Option<String> {
    let names: Vec<&str> = v["components"]
        .as_array()?
        .iter()
        .filter(|c| c["update_available"] == true)
        .filter_map(|c| c["name"].as_str())
        .collect();
    (!names.is_empty()).then(|| format!("{}: {}", tr("components.updates"), names.join(", ")))
}

/// Puts a picked face or edge into the joint form: `at` (and the moving `body`) while `at`
/// is empty, else `to`.
pub(crate) fn pick_joint_frame(form: &mut Form, h: &Hit) {
    let (body, name) = match h {
        Hit::Face { body, name, .. } | Hit::Edge { body, name, .. } => (Some(body), name),
        Hit::Plane { name } => (None, name),
        Hit::Sketch { .. } => return,
    };
    if form.value("at").is_none_or(|v| v.trim().is_empty()) {
        form.set("at", name);
        if let Some(body) = body {
            form.set("body", body);
        }
    } else if form.value("at") != Some(name.as_str()) {
        form.set("to", name);
    }
}
