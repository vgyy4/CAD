//! AI agents in the GUI (ADR 0012; spec "MCP server & agent safety").
//!
//! - [`Agents`] owns the MCP server of the open project and its localhost Streamable
//!   HTTP endpoint (bearer token, owner-only connection file), plus the small tokio
//!   runtime that serves it. The server drives the same `gusset-host` model thread as the
//!   window, so agents and the user work on one live model.
//! - [`GuiApprovals`] is the in-app [`ApprovalProvider`]: an agent request that needs
//!   approval waits (on an MCP worker thread, never the UI thread) until the user answers
//!   the modal, or until the timeout, which denies it.
//! - The "AI agents" panel shows the connection, the read-only switch, the kill switch
//!   (revoke token), the audit log and the agent branches.
//!
//! Text that came from agents or files (agent names, branch names, arguments, compare
//! results) is shown as plain labels only: hidden/bidi characters stripped, length
//! capped, never interpreted as markup or links.

use gusset_host::HostHandle;
use gusset_mcp::sanitize::clean_short;
use gusset_mcp::{
    ApprovalDecision, ApprovalProvider, ApprovalRequest, ConnectionInfo, HttpOptions, McpServer, ViewRenderer,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::app::{App, Done, Notice, Then};
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::link::Waker;
use crate::theme;
use egui::{RichText, Ui};
use gusset_commands::CommandError;
use gusset_history::{Actor, BranchId};
use serde_json::{Value, json};

/// How long an agent's approval request waits for the user before it is denied.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

/// At most this many approval requests wait at once; more are denied at once.
const MAX_PENDING: usize = 16;

/// Text from agents or files, made safe to show as a plain label.
#[must_use]
pub fn plain(s: &str, max: usize) -> String {
    clean_short(s, max)
}

/// One request waiting for the user.
struct Pending {
    id: u64,
    request: ApprovalRequest,
    reply: mpsc::SyncSender<ApprovalDecision>,
    since: Instant,
}

/// The approval request the modal shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownApproval {
    pub id: u64,
    pub request: ApprovalRequest,
    /// Seconds left before it is denied automatically.
    pub seconds_left: u64,
    /// Further requests waiting behind this one.
    pub queued: usize,
}

/// The GUI's [`ApprovalProvider`]: queues requests for the modal.
pub struct GuiApprovals {
    queue: Mutex<VecDeque<Pending>>,
    waker: Arc<Waker>,
    timeout: Mutex<Duration>,
    next: AtomicU64,
    closed: AtomicBool,
}

impl std::fmt::Debug for GuiApprovals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuiApprovals").field("pending", &self.pending()).finish_non_exhaustive()
    }
}

impl GuiApprovals {
    #[must_use]
    pub fn new(waker: Arc<Waker>) -> GuiApprovals {
        GuiApprovals {
            queue: Mutex::new(VecDeque::new()),
            waker,
            timeout: Mutex::new(APPROVAL_TIMEOUT),
            next: AtomicU64::new(1),
            closed: AtomicBool::new(false),
        }
    }

    fn q(&self) -> std::sync::MutexGuard<'_, VecDeque<Pending>> {
        self.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Changes the timeout for new requests (tests use a short one).
    pub fn set_timeout(&self, t: Duration) {
        *self.timeout.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = t;
    }

    /// Number of requests waiting.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.q().len()
    }

    /// The oldest waiting request (what the modal shows).
    #[must_use]
    pub fn current(&self) -> Option<ShownApproval> {
        let timeout = *self.timeout.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let q = self.q();
        q.front().map(|p| ShownApproval {
            id: p.id,
            request: p.request.clone(),
            seconds_left: timeout.saturating_sub(p.since.elapsed()).as_secs(),
            queued: q.len().saturating_sub(1),
        })
    }

    /// The user's answer to request `id`.
    pub fn answer(&self, id: u64, decision: ApprovalDecision) {
        let p = {
            let mut q = self.q();
            let i = q.iter().position(|p| p.id == id);
            i.and_then(|i| q.remove(i))
        };
        if let Some(p) = p {
            let _ = p.reply.send(decision);
        }
        self.waker.wake();
    }

    /// Denies everything waiting and every later request (the window is closing).
    pub fn close(&self, why: &str) {
        self.closed.store(true, Ordering::SeqCst);
        let all: Vec<Pending> = self.q().drain(..).collect();
        for p in all {
            let _ = p.reply.send(ApprovalDecision::Denied(why.into()));
        }
    }
}

impl ApprovalProvider for GuiApprovals {
    fn request_approval(&self, request: &ApprovalRequest) -> ApprovalDecision {
        if self.closed.load(Ordering::SeqCst) {
            return ApprovalDecision::Denied("the Gusset window is closing".into());
        }
        let timeout = *self.timeout.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (tx, rx) = mpsc::sync_channel(1);
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        {
            let mut q = self.q();
            if q.len() >= MAX_PENDING {
                return ApprovalDecision::Denied(
                    "too many approval requests are waiting for the user; try again later".into(),
                );
            }
            q.push_back(Pending { id, request: request.clone(), reply: tx, since: Instant::now() });
        }
        self.waker.wake();
        if let Ok(d) = rx.recv_timeout(timeout) {
            return d;
        }
        // Timed out: withdraw the request (unless the answer raced in just now).
        let removed = {
            let mut q = self.q();
            let i = q.iter().position(|p| p.id == id);
            i.and_then(|i| q.remove(i)).is_some()
        };
        self.waker.wake();
        if !removed && let Ok(d) = rx.try_recv() {
            return d;
        }
        ApprovalDecision::Denied(format!(
            "the user did not answer within {} seconds; the request was denied",
            timeout.as_secs()
        ))
    }
}

/// The MCP side of the window.
pub struct Agents {
    runtime: Option<tokio::runtime::Runtime>,
    server: Option<McpServer>,
    http: Option<HttpOptions>,
    /// The endpoint, while it runs.
    pub info: Option<ConnectionInfo>,
    /// Why the endpoint is not running.
    pub error: Option<String>,
    pub approvals: Arc<GuiApprovals>,
    renderer: Option<Arc<dyn ViewRenderer>>,
    /// The "AI agents" panel is open.
    pub show_panel: bool,
    /// Show the token in the connect command.
    pub show_token: bool,
    /// Last branch comparison: (branch name, plain-text lines).
    pub compare: Option<(String, Vec<String>)>,
    /// Standing approvals ("always allow"), shared by every project this window opens.
    pub grants: Arc<gusset_mcp::grants::Grants>,
    /// Agent keys (one credential per agent), shared like the grants.
    pub keys: Arc<gusset_mcp::agent_keys::AgentKeys>,
    /// The "new key" name field, and the token just created (shown once).
    pub new_key_name: String,
    pub new_key_token: Option<(String, String)>,
    /// The approval modal's "always allow" choices for the request it shows.
    pub always: AlwaysChoice,
    /// The kill switch is on for this window: endpoints for later projects start
    /// paused too, until the user allows agents again.
    killed: bool,
}

/// The "always allow" checkboxes of the approval modal (reset for every request).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AlwaysChoice {
    /// Request id these choices belong to.
    pub id: u64,
    pub this_action: bool,
    pub all_actions: bool,
    pub remember: bool,
}

impl AlwaysChoice {
    /// The decision for "Approve" with these choices. "All actions" wins over "this
    /// action"; without either it is a one-time approval.
    #[must_use]
    pub fn decision(&self, command: &str) -> ApprovalDecision {
        use gusset_mcp::grants::GrantScope;
        let scope = if self.all_actions {
            GrantScope::All
        } else if self.this_action {
            GrantScope::Command(command.to_string())
        } else {
            return ApprovalDecision::Approved;
        };
        ApprovalDecision::ApprovedAlways { scope, remember: self.remember }
    }
}

impl std::fmt::Debug for Agents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agents").field("info", &self.info).field("error", &self.error).finish_non_exhaustive()
    }
}

impl Agents {
    /// An MCP server for `host` (no HTTP endpoint yet; see [`Agents::start_http`]).
    #[must_use]
    pub fn new(host: &HostHandle, waker: Arc<Waker>) -> Agents {
        let mut a = Agents {
            runtime: None,
            server: None,
            http: None,
            info: None,
            error: None,
            approvals: Arc::new(GuiApprovals::new(waker)),
            renderer: None,
            show_panel: false,
            show_token: false,
            compare: None,
            grants: Arc::default(),
            keys: Arc::default(),
            new_key_name: String::new(),
            new_key_token: None,
            always: AlwaysChoice::default(),
            killed: false,
        };
        a.attach(host);
        a
    }

    /// The MCP server of the open project.
    #[must_use]
    pub fn server(&self) -> Option<&McpServer> {
        self.server.as_ref()
    }

    /// Creates the server for `host` (after a project was opened or recovered) and
    /// restarts the HTTP endpoint if it was running.
    pub fn attach(&mut self, host: &HostHandle) {
        let server = McpServer::new(host.clone());
        server.set_approval_provider(self.approvals.clone());
        server.set_grants(self.grants.clone());
        server.set_agent_keys(self.keys.clone());
        server.set_renderer(self.renderer.clone());
        self.server = Some(server);
        if let Some(opts) = self.http.clone() {
            self.start(opts);
        }
    }

    /// Stops the endpoint (deleting the connection file) before the host goes away.
    pub fn detach(&mut self) {
        if let Some(s) = self.server.take() {
            s.shutdown();
        }
        self.info = None;
    }

    /// **The single hook point for `render_view`.** The window installs the renderer
    /// adapter (gusset-render → [`ViewRenderer`]) here when one is available; without one,
    /// `render_view` answers agents with a clear "no renderer" error.
    pub fn set_renderer(&mut self, renderer: Option<Arc<dyn ViewRenderer>>) {
        self.renderer = renderer;
        if let Some(s) = &self.server {
            s.set_renderer(self.renderer.clone());
        }
    }

    /// Starts the localhost HTTP endpoint (ADR 0012 transport 1) and remembers the
    /// options, so it comes back when another project is opened in this window.
    pub fn start_http(&mut self, options: HttpOptions) {
        self.http = Some(options.clone());
        self.start(options);
    }

    fn start(&mut self, options: HttpOptions) {
        if self.runtime.is_none() {
            match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("gusset-mcp")
                .enable_all()
                .build()
            {
                Ok(rt) => self.runtime = Some(rt),
                Err(e) => {
                    self.error = Some(format!("cannot start the agent server: {e}"));
                    return;
                }
            }
        }
        let (Some(rt), Some(server)) = (self.runtime.as_ref(), self.server.as_ref()) else { return };
        let options = HttpOptions { start_paused: self.killed, ..options };
        match rt.block_on(server.start_http(options)) {
            Ok(info) => {
                self.info = Some(info);
                self.error = None;
            }
            Err(e) => {
                self.info = None;
                self.error = Some(e.to_string());
            }
        }
    }

    /// Kill switch: a new token at once; every agent connection is dropped.
    ///
    /// # Errors
    /// A message for the status bar.
    pub fn revoke(&mut self) -> Result<(), String> {
        // Standing approvals are suspended with the token (the server does it too), and
        // the pause outlives this project (security re-review 1).
        let _ = self.grants.set_suspended(true);
        self.killed = true;
        match &self.server {
            Some(s) => s.revoke().map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    /// Loads the remembered "always allow" grants (owner-only file in the user's data
    /// directory). Without this call grants last until the window closes.
    ///
    /// # Errors
    /// A message for the status bar (a damaged file gives no grants).
    pub fn load_grants(&self, path: &std::path::Path) -> Result<usize, String> {
        self.grants.load(path).map_err(|e| e.to_string())
    }

    /// Loads the agent keys (owner-only file in the user's data directory).
    ///
    /// # Errors
    /// A message for the status bar (a damaged file gives no keys).
    pub fn load_keys(&self, path: &std::path::Path) -> Result<usize, String> {
        self.keys.load(path)
    }

    /// True after the kill switch until the user allows agents again.
    #[must_use]
    pub fn paused(&self) -> bool {
        self.server.as_ref().is_some_and(McpServer::agents_paused)
    }

    /// "Allow agents again" after the kill switch: a new token. Grants stay suspended
    /// until resumed separately.
    ///
    /// # Errors
    /// A message for the status bar.
    pub fn allow_again(&mut self) -> Result<(), String> {
        self.killed = false;
        match &self.server {
            Some(s) => s.allow_agents().map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    /// The `claude mcp add` command for the running endpoint (token hidden unless
    /// `with_token`).
    #[must_use]
    pub fn connect_command(&self, with_token: bool) -> Option<String> {
        let info = self.info.as_ref()?;
        let token = if with_token {
            self.server.as_ref().and_then(McpServer::bearer_token).unwrap_or_default()
        } else {
            "<token>".into()
        };
        Some(format!("claude mcp add --transport http gusset {} --header \"Authorization: Bearer {token}\"", info.url))
    }

    /// Stops everything (window closing): pending approvals are denied.
    pub fn shutdown(&mut self) {
        self.approvals.close("the Gusset window is closing");
        self.detach();
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}

impl Drop for Agents {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// `hh:mm:ss` (UTC) of a unix time.
/// `HH:MM:SS` (UTC) of a Unix time, for lists.
pub(crate) fn clock_pub(t: u64) -> String {
    clock(t)
}

fn clock(t: u64) -> String {
    let d = t % 86_400;
    format!("{:02}:{:02}:{:02}", d / 3600, (d / 60) % 60, d % 60)
}

/// Plain-text lines of a `branch.compare` result.
fn compare_lines(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for (key, title) in [("parameters", "Parameter"), ("features", "Feature")] {
        for c in v.get(key).and_then(Value::as_array).into_iter().flatten() {
            let name = c.get("name").and_then(Value::as_str).unwrap_or("?");
            let change = c.get("change").and_then(Value::as_str).unwrap_or("?");
            let mut line = format!("{title} {}: {change}", plain(name, 80));
            if change == "modified" {
                let old = c.get("old_user_text").and_then(Value::as_str).unwrap_or("");
                let new = c.get("new_user_text").and_then(Value::as_str).unwrap_or("");
                line.push_str(&format!(" (was {}, now {})", plain(old, 80), plain(new, 80)));
            }
            out.push(line);
        }
    }
    if v.get("units_changed").and_then(Value::as_bool) == Some(true) {
        out.push("Units changed".into());
    }
    if out.is_empty() {
        out.push(tr("agents.no_diff").into());
    }
    out
}

impl App {
    pub(crate) fn show_compare(&mut self, branch: &str, result: Result<Value, CommandError>) {
        match result {
            Ok(v) => self.agents.compare = Some((branch.to_string(), compare_lines(&v))),
            Err(e) => self.notice = Some(Notice { text: e.message, hint: e.hint, error: true }),
        }
    }

    /// Compares the user's branch with `branch` (shown in the agents panel).
    pub fn compare_branch(&mut self, branch: BranchId, name: &str) {
        let Some(cur) = self.snapshot.branch else { return };
        self.submit_command(
            "branch.compare",
            json!({ "a": cur.to_string(), "b": branch.to_string() }),
            Then::Compare(name.to_string()),
        );
    }

    /// Applies an agent branch's changes to the user's branch: a three-way
    /// `branch.merge`, so the user's own edits since the branch started are kept and
    /// items both changed are reported as conflicts (validated by a rebuild, undoable).
    pub fn apply_branch(&mut self, branch: BranchId) {
        self.submit_command("branch.merge", json!({ "from": branch.to_string() }), Then::Notice);
    }

    /// The "AI agents" panel.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn agents_panel(&mut self, ui: &mut Ui) {
        let pal = *theme::palette(self.theme);
        ui.horizontal(|ui| {
            ui.heading(tr("agents.title"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("🗙").on_hover_text(tr("dialog.close")).clicked() {
                    self.agents.show_panel = false;
                }
            });
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            // Connection.
            match (&self.agents.info, &self.agents.error) {
                (Some(info), _) => {
                    icons::labeled(ui, Icon::Dot, pal.success, format!("{} {}", tr("agents.listening"), info.url));
                }
                (None, Some(e)) => {
                    ui.colored_label(pal.danger, format!("{} {}", tr("agents.not_running"), plain(e, 300)));
                }
                (None, None) => {
                    ui.colored_label(pal.text_muted, tr("agents.not_started"));
                }
            }
            if let Some(cmd) = self.agents.connect_command(self.agents.show_token) {
                ui.label(tr("agents.how"));
                ui.add(egui::Label::new(RichText::new(cmd).monospace()).wrap());
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.agents.show_token, tr("agents.show_token"));
                    if ui.button(tr("agents.copy")).clicked()
                        && let Some(full) = self.agents.connect_command(true)
                    {
                        ui.ctx().copy_text(full);
                        self.notice = Some(Notice { text: tr("agents.copied").into(), hint: None, error: false });
                    }
                });
            }
            ui.label(tr("agents.how_stdio"));
            let file = self.project_file().map(|p| format!(" {}", p.to_string_lossy())).unwrap_or_default();
            let stdio = format!("claude mcp add --transport stdio gusset -- gusset mcp{file}");
            ui.add(egui::Label::new(RichText::new(stdio).monospace()).wrap());
            ui.separator();

            // Safety controls.
            let mut ro = self.snapshot.read_only_agents;
            if ui.checkbox(&mut ro, tr("agents.read_only")).on_hover_text(tr("agents.read_only_help")).changed() {
                self.submit(move |s, _| s.config.read_only_agents = ro, Done::Refresh);
                self.snapshot.read_only_agents = ro;
            }
            if ui.button(tr("agents.revoke")).on_hover_text(tr("agents.revoke_help")).clicked() {
                self.notice = Some(match self.agents.revoke() {
                    Ok(()) => Notice { text: tr("agents.revoked").into(), hint: None, error: false },
                    Err(e) => Notice { text: e, hint: None, error: true },
                });
            }
            if self.agents.paused() && ui.button(tr("agents.allow_again")).clicked() {
                self.notice = Some(match self.agents.allow_again() {
                    Ok(()) => Notice { text: tr("agents.allowed_again").into(), hint: None, error: false },
                    Err(e) => Notice { text: e, hint: None, error: true },
                });
            }
            ui.separator();
            self.grants_section(ui, &pal);
            ui.separator();
            self.keys_section(ui, &pal);
            ui.separator();

            // Agent branches.
            ui.label(RichText::new(tr("agents.branches")).strong());
            let branches: Vec<_> = self.snapshot.branches.iter().filter(|b| b.agent.is_some()).cloned().collect();
            if branches.is_empty() {
                ui.weak(tr("agents.no_branches"));
            }
            for b in branches {
                let name = plain(&b.name, 80);
                ui.horizontal_wrapped(|ui| {
                    let mine = Some(b.id) == self.snapshot.branch;
                    let label = if mine { format!("{name} ({})", tr("agents.yours")) } else { name.clone() };
                    icons::labeled(ui, Icon::Diamond, pal.agent, label);
                    if !mine && ui.small_button(tr("agents.switch")).clicked() {
                        self.submit_command("branch.switch", json!({ "branch": b.id.to_string() }), Then::Notice);
                    }
                    if !mine && ui.small_button(tr("agents.compare")).clicked() {
                        self.compare_branch(b.id, &name);
                    }
                    if !mine && ui.small_button(tr("agents.apply")).on_hover_text(tr("agents.apply_help")).clicked() {
                        self.apply_branch(b.id);
                    }
                });
            }
            if let Some((name, lines)) = self.agents.compare.clone() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} {name}", tr("agents.compared"))).strong());
                        if ui.small_button("🗙").clicked() {
                            self.agents.compare = None;
                        }
                    });
                    for l in lines {
                        ui.label(l);
                    }
                });
            }
            ui.separator();

            // Audit log, newest first.
            ui.label(RichText::new(format!("{} ({})", tr("agents.audit"), self.snapshot.audit_total)).strong());
            if self.snapshot.audit.is_empty() {
                ui.weak(tr("agents.no_audit"));
            }
            for e in self.snapshot.audit.iter().rev().take(200) {
                let who = match &e.actor {
                    Actor::Agent(a) => plain(a, 64),
                    Actor::User => "user".into(),
                };
                let on = e.branch.as_deref().map(|b| format!(" on {}", plain(b, 80))).unwrap_or_default();
                ui.horizontal_wrapped(|ui| {
                    ui.weak(clock(e.time));
                    ui.colored_label(pal.agent, who);
                    let col = if e.outcome == "ok" { pal.text } else { pal.danger };
                    ui.colored_label(col, format!("{}{on}: {}", plain(&e.command, 64), plain(&e.outcome, 120)));
                });
            }
        });
    }

    /// Standing approvals: every grant with its own Revoke button, Revoke all, and
    /// Resume after the kill switch suspended them.
    fn grants_section(&mut self, ui: &mut Ui, pal: &theme::Palette) {
        use gusset_mcp::grants::GrantScope;
        let grants = self.agents.grants.list();
        ui.label(RichText::new(tr("agents.grants")).strong());
        if grants.is_empty() {
            ui.weak(tr("agents.no_grants"));
            return;
        }
        let suspended = self.agents.grants.suspended();
        if suspended {
            ui.colored_label(pal.warning, tr("agents.grants_suspended"));
        }
        let mut result = None;
        for g in &grants {
            ui.horizontal_wrapped(|ui| {
                let what = match &g.scope {
                    GrantScope::Command(c) => plain(c, 64),
                    GrantScope::All => tr("agents.grant_all").to_string(),
                };
                let when = if g.remember { tr("agents.grant_kept") } else { tr("agents.grant_session") };
                icons::labeled(ui, Icon::Diamond, pal.agent, plain(&g.agent, 64));
                ui.label(format!("{what} · {when}"));
                if ui.small_button(tr("agents.grant_revoke")).clicked() {
                    result = Some(self.agents.grants.revoke(&g.agent, &g.scope));
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button(tr("agents.grants_revoke_all")).clicked() {
                result = Some(self.agents.grants.revoke_all());
            }
            if suspended && ui.button(tr("agents.grants_resume")).clicked() {
                result = Some(self.agents.grants.set_suspended(false));
            }
        });
        if let Some(Err(e)) = result {
            self.notice = Some(Notice { text: e.to_string(), hint: None, error: true });
        }
    }

    /// Agent keys: one credential per agent, so it keeps its name, branch and grants.
    fn keys_section(&mut self, ui: &mut Ui, pal: &theme::Palette) {
        ui.label(RichText::new(tr("agents.keys")).strong()).on_hover_text(tr("agents.keys_help"));
        let mut notice = None;
        for k in self.agents.keys.list() {
            ui.horizontal_wrapped(|ui| {
                icons::labeled(ui, Icon::Diamond, pal.agent, plain(&k.name, 64));
                ui.weak(format!("branch agent/{}", k.agent()));
                if ui.small_button(tr("agents.key_remove")).clicked() {
                    if let Err(e) = self.agents.keys.remove(&k.name) {
                        notice = Some(Notice { text: e, hint: None, error: true });
                    }
                    if self.agents.new_key_token.as_ref().is_some_and(|(n, _)| *n == k.name) {
                        self.agents.new_key_token = None;
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.agents.new_key_name).hint_text("cad-bot").desired_width(120.0));
            if ui.button(tr("agents.key_add")).clicked() {
                let name = self.agents.new_key_name.trim().to_string();
                match self.agents.keys.create(&name, crate::app::now_unix()) {
                    Ok(token) => {
                        self.agents.new_key_token = Some((name, token));
                        self.agents.new_key_name.clear();
                    }
                    Err(e) => notice = Some(Notice { text: e, hint: None, error: true }),
                }
            }
        });
        if let Some((name, token)) = &self.agents.new_key_token {
            ui.colored_label(pal.warning, format!("{} {name}:", tr("agents.key_shown_once")));
            let mut t = token.clone();
            ui.add(egui::TextEdit::singleline(&mut t).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
        }
        if let Some(n) = notice {
            self.notice = Some(n);
        }
    }

    /// The approval modal: the exact action an agent wants to run, Approve / Deny.
    pub(crate) fn approval_modal(&mut self, ctx: &egui::Context) {
        let Some(shown) = self.agents.approvals.current() else { return };
        let r = &shown.request;
        let mut decision = None;
        let pal = *theme::palette(self.theme);
        egui::Modal::new(egui::Id::new("agent_approval")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading(tr("approval.title"));
            ui.colored_label(pal.agent, format!("{} {}", tr("approval.agent"), plain(&r.agent, 64)));
            egui::Grid::new("approval_grid").num_columns(2).show(ui, |ui| {
                ui.label(tr("approval.action"));
                ui.label(format!("{} ({})", plain(&r.title, 120), plain(&r.command, 64)));
                ui.end_row();
                ui.label(tr("approval.reason"));
                ui.add(egui::Label::new(plain(&r.reason, 500)).wrap());
                ui.end_row();
                ui.label(tr("approval.arguments"));
                ui.add(egui::Label::new(RichText::new(plain(&r.arguments, 1000)).monospace()).wrap());
                ui.end_row();
                if !r.context.is_empty() {
                    ui.label(tr("approval.affects"));
                    ui.add(egui::Label::new(RichText::new(plain(&r.context, 600)).strong()).wrap());
                    ui.end_row();
                }
            });
            // "Always allow" (ADR 0012 "Standing approvals"): only ever set here.
            if self.agents.always.id != shown.id {
                self.agents.always = AlwaysChoice { id: shown.id, ..AlwaysChoice::default() };
            }
            let who = plain(gusset_mcp::limits::identity_of(&r.agent), 64);
            let a = &mut self.agents.always;
            ui.add_space(theme::SPACE[2]);
            ui.checkbox(&mut a.this_action, format!("{} \"{who}\"", tr("approval.always_action")));
            let blanket = gusset_mcp::grants::blanket_ok(&r.command) && !r.main_edit;
            ui.add_enabled_ui(blanket, |ui| {
                ui.checkbox(&mut a.all_actions, format!("{} \"{who}\"", tr("approval.always_all")))
                    .on_hover_text(tr("approval.always_all_help"))
                    .on_disabled_hover_text(tr("approval.always_all_excluded"));
            });
            if !blanket {
                a.all_actions = false;
            }
            ui.add_enabled_ui(a.this_action || a.all_actions, |ui| {
                ui.checkbox(&mut a.remember, tr("approval.always_remember"))
                    .on_hover_text(tr("approval.always_remember_help"));
            });
            ui.weak(format!("{} {} s", tr("approval.timeout"), shown.seconds_left));
            if shown.queued > 0 {
                ui.weak(format!("{} {}", shown.queued, tr("approval.queued")));
            }
            ui.horizontal(|ui| {
                if ui.button(tr("approval.approve")).clicked() {
                    decision = Some(self.agents.always.decision(&r.command));
                }
                if ui.button(tr("approval.deny")).clicked() {
                    decision = Some(ApprovalDecision::Denied("the user denied the request".into()));
                }
            });
        });
        match decision {
            Some(d) => self.agents.approvals.answer(shown.id, d),
            // The countdown ticks: a delayed repaint, not a busy loop.
            None => ctx.request_repaint_after(Duration::from_secs(1)),
        }
    }
}
