//! `gusset mcp [file]`: the stdio entry point (ADR 0012 transport 2).
//!
//! 1. If a running Gusset app published a connection file (see [`crate::http`]) — the
//!    one serving the requested project, or the most recent one when no file is given —
//!    stdio is relayed to its HTTP endpoint with the bearer token, so the agent works on
//!    the same live model as the human.
//! 2. Otherwise the project file (or a new empty project) is opened headless in a
//!    [`gusset_host::Host`] and served over stdio with the same tools. Approvals are
//!    denied unless the client supports elicitation. A few seconds after each change
//!    and on exit, unsaved changes (agent work lives on agent branches) are saved to the
//!    file; if that fails a recovery snapshot is kept. A project with a pending crash-recovery copy or a damaged file
//!    is refused with a clear message: only the app's recovery dialog resolves those.
//!
//! Connection files are trusted only if they and their directory belong to the current
//! user and nobody else can read them. Before relaying anything, the bridge checks the
//! port really is that app (an identity challenge bound to the token; see
//! [`crate::identity`]), and every response must carry a proof. When the user revoked
//! the token in the app (kill switch), the file says `paused` and the bridge refuses to
//! start instead of reconnecting or falling back to the same project headless.
//!
//! Diagnostics go to stderr (stdout is the protocol stream). The token is never printed.

use crate::approval::ApprovalProvider;
use crate::json_ext::JsonExt;
use crate::render::ViewRenderer;
use crate::{McpError, McpServer};
use bytes::Bytes;
use gusset_commands::{CmdCtx, Session};
use gusset_host::Host;
use gusset_kernel::Kernel;
use gusset_store::paths::DataDirs;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

/// Options for [`run_stdio`].
#[derive(Clone)]
pub struct StdioOptions {
    /// Project file (`gusset mcp model.gus`). `None`: attach to the most recent running
    /// app, or run a new empty project headless.
    pub project: Option<PathBuf>,
    /// Kernel for headless mode (the CLI passes the OCCT kernel).
    pub kernel: Arc<dyn Kernel>,
    /// Data directories for headless mode (`None`: the OS defaults).
    pub data_dirs: Option<DataDirs>,
    /// Where running apps publish connection files (`None`: the default).
    pub connection_dir: Option<PathBuf>,
    /// Try to attach to a running app first (default true).
    pub attach: bool,
    /// Headless: renderer for `render_view` (usually none).
    pub renderer: Option<Arc<dyn ViewRenderer>>,
    /// Headless: approval provider when the client cannot elicit (default: deny).
    pub approvals: Option<Arc<dyn ApprovalProvider>>,
    /// Headless: the STEP/IGES importer (the CLI passes the sandboxed worker). Mesh
    /// formats need none.
    pub importer: Option<Arc<dyn gusset_commands::session::Importer>>,
    /// Read-only agent mode (`gusset mcp --read-only`): mutating tools are rejected.
    /// Headless it maps to `SessionConfig::read_only_agents`; when relaying to a running
    /// app the bridge itself refuses mutating `tools/call` requests.
    pub read_only: bool,
    /// Headless: why the kernel is not in its worker process, if it is not; the
    /// `diagnostics` resource says so (dogfooding F4-12).
    pub kernel_note: Option<String>,
}

impl std::fmt::Debug for StdioOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdioOptions")
            .field("project", &self.project)
            .field("attach", &self.attach)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl StdioOptions {
    /// Defaults for `gusset mcp [file]`.
    #[must_use]
    pub fn new(project: Option<PathBuf>, kernel: Arc<dyn Kernel>) -> Self {
        StdioOptions {
            project,
            kernel,
            data_dirs: None,
            connection_dir: None,
            attach: true,
            renderer: None,
            approvals: None,
            importer: None,
            read_only: false,
            kernel_note: None,
        }
    }
}

/// Blocking entry point for the CLI: builds a tokio runtime and runs
/// [`run_stdio_async`] on the process's stdin/stdout.
///
/// # Errors
/// Runtime, I/O, project-open and protocol errors.
pub fn run_stdio(options: StdioOptions) -> Result<(), McpError> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(run_stdio_async(options, tokio::io::stdin(), tokio::io::stdout()))
}

/// Like [`run_stdio`], but the protocol is written to `protocol_out` instead of the
/// process's stdout. The CLI passes the original stdout here after pointing descriptor 1
/// at stderr (`gusset_hw::stdio::take_stdout_for_protocol`), so native libraries that
/// print to stdout cannot corrupt the JSON-RPC stream.
///
/// # Errors
/// As [`run_stdio`].
pub fn run_stdio_with_output(options: StdioOptions, protocol_out: std::fs::File) -> Result<(), McpError> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(run_stdio_async(options, tokio::io::stdin(), tokio::fs::File::from_std(protocol_out)))
}

/// [`run_stdio`] on arbitrary streams (tests use in-memory duplex streams).
///
/// # Errors
/// As [`run_stdio`].
pub async fn run_stdio_async<R, W>(mut options: StdioOptions, read: R, write: W) -> Result<(), McpError>
where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    // A relative project path has the empty path as its parent, which is no project
    // folder at all (exports failed with "entity not found", F2-3).
    options.project = options.project.take().map(|p| absolutize(&p));
    if options.attach {
        let dir = options.connection_dir.clone().or_else(crate::http::default_connection_dir);
        if let Some(dir) = dir {
            match find_instance(&dir, options.project.as_deref()).await {
                Found::App(app) => {
                    eprintln!("gusset mcp: attaching to the running Gusset app on port {}", app.port);
                    return relay(app, options.read_only, read, write).await;
                }
                Found::Paused => {
                    return Err(McpError::Config(
                        "agents are switched off in the running Gusset app (the token was revoked); allow agents \
                         again in the app's AI agents panel, then restart this bridge"
                            .into(),
                    ));
                }
                Found::Nothing => {}
            }
        }
    }
    headless(options, read, write).await
}

/// A running app found through its connection file.
#[derive(Clone)]
pub(crate) struct AppInstance {
    pub(crate) port: u16,
    /// `None` while the app has agents paused (kill switch).
    token: Option<String>,
    project: Option<PathBuf>,
    started: u64,
}

/// What [`find_instance`] found.
#[derive(Debug)]
pub(crate) enum Found {
    /// A verified running app.
    App(AppInstance),
    /// The matching app is running with agents paused.
    Paused,
    /// No usable app.
    Nothing,
}

/// The current user's id, learnt safely (no FFI) from a probe file created in `dir`. A
/// directory we cannot create files in is not ours.
#[cfg(unix)]
fn our_uid(dir: &Path) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    let probe = dir.join(format!(".probe-{}", crate::approval::random_hex(8)?));
    let f = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).ok()?;
    let uid = f.metadata().ok().map(|m| m.uid());
    drop(f);
    let _ = std::fs::remove_file(&probe);
    uid
}

/// The connection directory must be a real directory owned by us and closed to others.
#[cfg(unix)]
pub(crate) fn trusted_dir(dir: &Path) -> Option<u32> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let meta = std::fs::symlink_metadata(dir).ok()?;
    if !meta.file_type().is_dir() || meta.permissions().mode() & 0o077 != 0 {
        return None;
    }
    let uid = our_uid(dir)?;
    (meta.uid() == uid).then_some(uid)
}

#[cfg(not(unix))]
pub(crate) fn trusted_dir(_dir: &Path) -> Option<u32> {
    // Per-user profile directory; its default ACL limits it to the user (THREAT_MODEL B3).
    Some(0)
}

impl std::fmt::Debug for AppInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppInstance").field("port", &self.port).field("project", &self.project).finish_non_exhaustive()
    }
}

pub(crate) fn read_connection_file(path: &Path, uid: u32) -> Option<AppInstance> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() || meta.len() > 64 * 1024 {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // Only trust files we own and nobody else could have written or read.
        if meta.permissions().mode() & 0o077 != 0 || meta.uid() != uid {
            return None;
        }
    }
    #[cfg(not(unix))]
    let _ = uid;
    let v: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if v.at("version").as_u64() != Some(crate::http::CONNECTION_FILE_VERSION) {
        return None;
    }
    let port = u16::try_from(v.at("port").as_u64()?).ok().filter(|p| *p != 0)?;
    let paused = v.at("paused").as_bool().unwrap_or(false);
    let token = if paused {
        None
    } else {
        let t = v.at("token").as_str()?.to_string();
        if t.len() != 64 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some(t)
    };
    Some(AppInstance {
        port,
        token,
        project: v.at("project").as_str().map(PathBuf::from),
        started: v.at("started").as_u64().unwrap_or(0),
    })
}

/// Finds a running app: the one serving `project` if given, else the most recently
/// started one. Files must be ours (see [`trusted_dir`]) and the port must prove it holds
/// the file's token ([`verify_identity`]) before anything else is sent to it.
pub(crate) async fn find_instance(dir: &Path, project: Option<&Path>) -> Found {
    let Some(uid) = trusted_dir(dir) else { return Found::Nothing };
    let want = project.map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));
    let Ok(entries) = std::fs::read_dir(dir) else { return Found::Nothing };
    let mut found: Vec<AppInstance> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| read_connection_file(&p, uid))
        .filter(|a| match (&want, &a.project) {
            (None, _) => true,
            (Some(w), Some(p)) => p.canonicalize().unwrap_or_else(|_| p.clone()) == *w,
            (Some(_), None) => false,
        })
        .collect();
    found.sort_by_key(|a| std::cmp::Reverse(a.started));
    let mut paused = false;
    for a in found {
        match verify_identity(&a).await {
            Identity::Verified => return Found::App(a),
            Identity::Paused => paused = true,
            Identity::Unverified => {}
        }
    }
    if paused { Found::Paused } else { Found::Nothing }
}

enum Identity {
    Verified,
    Paused,
    Unverified,
}

/// Asks the port to prove it knows the token, without sending the token. A paused app
/// answers 503; anything else that does not prove itself is ignored (a stale file whose
/// port another process now holds).
async fn verify_identity(app: &AppInstance) -> Identity {
    let attempt = async {
        let addr = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, app.port));
        let stream = tokio::net::TcpStream::connect(addr).await.ok()?;
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await.ok()?;
        tokio::spawn(conn);
        let nonce = crate::identity::new_nonce()?;
        let req = hyper::Request::builder()
            .method(http::Method::GET)
            .uri(crate::identity::IDENTITY_PATH)
            .header(http::header::HOST, format!("127.0.0.1:{}", app.port))
            .header(crate::identity::NONCE_HEADER, nonce.as_str())
            .body(Full::new(Bytes::new()))
            .ok()?;
        let resp = sender.send_request(req).await.ok()?;
        if resp.status() == hyper::StatusCode::SERVICE_UNAVAILABLE {
            return Some(Identity::Paused);
        }
        let proof = resp.headers().get(crate::identity::PROOF_HEADER).and_then(|v| v.to_str().ok())?;
        let token = app.token.as_deref()?;
        Some(if resp.status().is_success() && crate::identity::verify(token, &nonce, proof) {
            Identity::Verified
        } else {
            Identity::Unverified
        })
    };
    tokio::time::timeout(Duration::from_secs(2), attempt).await.ok().flatten().unwrap_or(Identity::Unverified)
}

/// `path` made absolute against the current directory (symlinks are left alone; the
/// export path checks canonicalize the folder themselves).
fn absolutize(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

async fn headless<R, W>(options: StdioOptions, read: R, write: W) -> Result<(), McpError>
where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let dirs = match options.data_dirs.clone() {
        Some(d) => d,
        None => DataDirs::from_os().ok_or_else(|| McpError::Config("no per-user data directory".into()))?,
    };
    let empty =
        || gusset_doc::Document::from_text("gusset 1\nunits mm\n").map_err(|e| McpError::Config(format!("{e:?}")));
    let (session, project, save_as) = match &options.project {
        // A read-only connection to a project another server or window holds reads its
        // last save, unlocked (dogfooding F5-12); it never writes.
        Some(path)
            if path.exists() && options.read_only && gusset_project::Project::is_locked_elsewhere(path, &dirs) =>
        {
            let history = gusset_project::Project::read_saved(path)
                .map_err(|e| McpError::Command(gusset_commands::CommandError::new("io", e.to_string())))?;
            eprintln!("gusset mcp: {} is open elsewhere; reading its last save", path.display());
            let mut s = Session::from_history(history, options.kernel.clone());
            s.project_dir = path.parent().map(Path::to_path_buf);
            s.importer = options.importer.clone();
            (s, None, None)
        }
        Some(path) if path.exists() => {
            let (p, history, report) = gusset_project::Project::open(path, dirs)
                .map_err(|e| McpError::Command(gusset_commands::CommandError::new("io", e.to_string())))?;
            // Never resolve (and so never delete) recovery data the user has not reviewed
            // (review C2): the app's recovery dialog is the only place that decides.
            let refusal = match &report {
                gusset_project::OpenReport::Clean if !p.recovery_pending => None,
                gusset_project::OpenReport::Clean | gusset_project::OpenReport::RecoveryAvailable { .. } => Some(
                    "it has unsaved work recovered after a crash. Open it in the Gusset app and choose \
                     Recover, Open saved or Keep both first; the agent bridge leaves recovery data alone"
                        .to_string(),
                ),
                gusset_project::OpenReport::Damaged { reason, .. } => {
                    Some(format!("the file is damaged ({reason}). Open it in the Gusset app to repair it first"))
                }
            };
            if let Some(why) = refusal {
                p.release();
                return Err(McpError::Config(format!("gusset mcp: not opening {}: {why}", path.display())));
            }
            let mut s = Session::from_history(history, options.kernel.clone());
            s.project_dir = path.parent().map(Path::to_path_buf);
            s.importer = options.importer.clone();
            (s, Some(p), None)
        }
        Some(path) => {
            let p = gusset_project::Project::create(dirs)
                .map_err(|e| McpError::Command(gusset_commands::CommandError::new("io", e.to_string())))?;
            let mut s = Session::new(&empty()?, options.kernel.clone(), &CmdCtx::user(0));
            s.project_dir = path.parent().map(Path::to_path_buf);
            s.importer = options.importer.clone();
            (s, Some(p), Some(path.clone()))
        }
        None => {
            eprintln!("gusset mcp: no project file given and no running app found: using a new, unsaved project");
            (Session::new(&empty()?, options.kernel.clone(), &CmdCtx::user(0)), None, None)
        }
    };
    let host = Host::spawn(session, project);
    let server = McpServer::new(host.clone());
    if let Some(n) = &options.kernel_note {
        server.set_diagnostics(format!("{}\nGeometry kernel: {n}\n", gusset_hw::Diagnostics::collect().to_text()));
    }
    // Device and pipelines are ready before the first render_view (dogfooding F4-13).
    if let Some(r) = options.renderer.clone() {
        let _ = std::thread::Builder::new().name("gusset-render-warm".into()).spawn(move || r.warm());
    }
    server.set_renderer(options.renderer.clone());
    server.set_read_only(options.read_only).map_err(McpError::Command)?;
    if let Some(a) = options.approvals.clone() {
        server.set_approval_provider(a);
    }
    // Agent work reaches the file a few seconds after each change, not only on a clean
    // exit: a killed server lost a whole part (dogfooding F4-2).
    // A read-only connection never writes the project (dogfooding F5-2).
    let saver = (!options.read_only).then(|| tokio::spawn(autosave(host.clone(), save_as.clone())));
    let served = server.serve_io(read, write).await;
    if let Some(saver) = saver {
        saver.abort();
    }
    // Persist agent work (it lives on agent branches; main is untouched).
    if let Some((session, project)) = host.shutdown()
        && let Some(mut project) = project
    {
        if session.dirty && !options.read_only {
            let saved = match &save_as {
                Some(p) => project.save_as(p, session.history()),
                None => project.save(session.history()),
            };
            match saved {
                Ok(()) => project.close(),
                Err(e) => {
                    eprintln!("gusset mcp: could not save the project ({e}); a recovery snapshot was kept");
                    let _ = project.snapshot(session.history());
                }
            }
        } else {
            project.close();
        }
    }
    served
}

/// How long after a change the headless server writes the file.
const AUTOSAVE_DELAY: std::time::Duration = std::time::Duration::from_secs(3);

/// Saves the project (to `save_as` while it has no file yet) after changes, at most once
/// per [`AUTOSAVE_DELAY`]. A failed save keeps a recovery snapshot instead.
async fn autosave(host: gusset_host::HostHandle, save_as: Option<PathBuf>) {
    use tokio::sync::broadcast::error::{RecvError, TryRecvError};
    let mut rx = host.subscribe();
    loop {
        match rx.recv().await {
            Ok(gusset_host::HostEvent::Changed { .. }) | Err(RecvError::Lagged(_)) => {}
            Ok(_) => continue,
            Err(RecvError::Closed) => return,
        }
        tokio::time::sleep(AUTOSAVE_DELAY).await;
        // Changes made meanwhile are in this save.
        loop {
            match rx.try_recv() {
                Ok(_) | Err(TryRecvError::Lagged(_)) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Closed) => return,
            }
        }
        let (h, path) = (host.clone(), save_as.clone());
        let saved = tokio::task::spawn_blocking(move || {
            h.exec(move |s, p| {
                let Some(p) = p else { return Ok(()) };
                if !s.dirty {
                    return Ok(());
                }
                let r = match (p.path().is_some(), &path) {
                    (true, _) => p.save(s.history()),
                    (false, Some(path)) => p.save_as(path, s.history()),
                    (false, None) => return Ok(()),
                };
                match r {
                    Ok(()) => {
                        s.dirty = false;
                        Ok(())
                    }
                    Err(e) => {
                        let _ = p.snapshot(s.history());
                        Err(e.to_string())
                    }
                }
            })
        })
        .await;
        match saved {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(e))) => eprintln!("gusset mcp: could not save the project ({e}); a recovery snapshot was kept"),
            // The session closed: the exit path saves.
            Ok(Err(_)) | Err(_) => return,
        }
    }
}

// ------------------------------------------------------------------ relay

#[derive(Default)]
struct RelayState {
    session_id: Option<String>,
    legacy_version: Option<String>,
}

type Out = Arc<tokio::sync::Mutex<Box<dyn AsyncWrite + Send + Unpin>>>;

async fn emit(out: &Out, v: &Value) {
    let mut line = v.to_string();
    line.push('\n');
    let mut w = out.lock().await;
    let _ = w.write_all(line.as_bytes()).await;
    let _ = w.flush().await;
}

fn header_value(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_graphic() || c == ' ') && !s.starts_with(' ') && !s.ends_with(' ') {
        s.to_string()
    } else {
        format!("=?base64?{}?=", crate::b64::encode(s.as_bytes()))
    }
}

fn mcp_name(method: &str, params: &Value) -> Option<String> {
    let key = match method {
        "tools/call" | "prompts/get" => "name",
        "resources/read" | "resources/subscribe" | "resources/unsubscribe" => "uri",
        "tasks/get" | "tasks/update" | "tasks/cancel" => "taskId",
        _ => return None,
    };
    params.get(key).and_then(Value::as_str).map(String::from)
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// `--read-only` while relaying: the app's own setting is the user's, so the bridge
/// refuses mutating registry tools itself. Returns the error result to send back.
fn read_only_refusal(msg: &Value) -> Option<Value> {
    if msg.at("method") != "tools/call" {
        return None;
    }
    let mut name = msg.at("params").at("name").as_str()?;
    // `run` calls a command by name: check that command. An unnamed or unknown one
    // is refused by the app's own argument check.
    if name == crate::server::RUN {
        name = msg.at("params").at("arguments").at("name").as_str()?;
    }
    let spec = gusset_commands::registry().iter().find(|s| s.tool_name() == name || s.id == name)?;
    if !spec.flags.mutating && !spec.flags.writes_files {
        return None;
    }
    let e =
        gusset_commands::CommandError::new("read_only", "this MCP connection is read-only (gusset mcp --read-only)")
            .hint(
                "use the read tools (query_*, render_view, get_text); restart the bridge without --read-only to edit",
            );
    let result = rmcp::model::CallToolResult::error(vec![rmcp::model::ContentBlock::text(
        crate::server::compact_result(json!({ "error": e })),
    )]);
    let result = serde_json::to_value(result).ok()?;
    Some(json!({"jsonrpc": "2.0", "id": msg.at("id"), "result": result}))
}

async fn relay<R, W>(app: AppInstance, read_only: bool, read: R, write: W) -> Result<(), McpError>
where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let out: Out = Arc::new(tokio::sync::Mutex::new(Box::new(write)));
    let state = Arc::new(Mutex::new(RelayState::default()));
    let app = Arc::new(app);
    let mut lines = BufReader::new(read).lines();
    let mut tasks = tokio::task::JoinSet::new();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                emit(&out, &rpc_error(&Value::Null, -32700, "parse error")).await;
                continue;
            }
        };
        // JSON-RPC batches would hide tool calls from the read-only check; MCP clients
        // do not need them, so the read-only bridge refuses them (security re-review 6).
        if read_only && msg.is_array() {
            emit(&out, &rpc_error(&Value::Null, -32600, "batched requests are not supported in read-only mode")).await;
            continue;
        }
        if read_only && let Some(refusal) = read_only_refusal(&msg) {
            emit(&out, &refusal).await;
            continue;
        }
        let is_init = msg.at("method") == "initialize";
        // Keep the order of the handshake: wait for `initialize` before sending more.
        let (out2, st2, app2) = (out.clone(), state.clone(), app.clone());
        let fut = async move {
            if let Err(e) = post(&app2, &st2, &out2, msg.clone()).await
                && msg.get("id").is_some()
                && msg.get("method").is_some()
            {
                emit(&out2, &rpc_error(msg.at("id"), -32603, &format!("gusset bridge: {e}"))).await;
            }
        };
        if is_init {
            fut.await;
        } else {
            tasks.spawn(fut);
        }
        while tasks.try_join_next().is_some() {}
    }
    while tasks.join_next().await.is_some() {}
    // End a legacy session politely.
    let sid = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).session_id.clone();
    if let Some(sid) = sid {
        let _ = send(&app, http::Method::DELETE, vec![("mcp-session-id".into(), sid)], Bytes::new()).await;
    }
    Ok(())
}

async fn send(
    app: &AppInstance,
    method: http::Method,
    headers: Vec<(String, String)>,
    body: Bytes,
) -> Result<hyper::Response<hyper::body::Incoming>, String> {
    let token = app.token.as_deref().ok_or("agents are switched off in the Gusset app")?;
    let nonce = crate::identity::new_nonce().ok_or("the OS random number generator failed")?;
    let addr = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, app.port));
    let stream = tokio::net::TcpStream::connect(addr).await.map_err(|e| format!("cannot reach the app: {e}"))?;
    let (mut sender, conn) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await.map_err(|e| e.to_string())?;
    tokio::spawn(conn);
    let mut req = hyper::Request::builder()
        .method(method)
        .uri("/mcp")
        .header(http::header::HOST, format!("127.0.0.1:{}", app.port))
        .header(http::header::AUTHORIZATION, format!("Bearer {token}"))
        .header(crate::identity::NONCE_HEADER, nonce.as_str())
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json, text/event-stream");
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let req = req.body(Full::new(body)).map_err(|e| e.to_string())?;
    let resp = sender.send_request(req).await.map_err(|e| e.to_string())?;
    if resp.status() == hyper::StatusCode::UNAUTHORIZED {
        // Nothing to prove with a dead token; the caller reports the revocation.
        return Ok(resp);
    }
    let proof = resp.headers().get(crate::identity::PROOF_HEADER).and_then(|v| v.to_str().ok()).unwrap_or("");
    if !crate::identity::verify(token, &nonce, proof) {
        return Err("the process on the app's port could not prove it is the Gusset app; stopping".into());
    }
    Ok(resp)
}

async fn post(app: &AppInstance, state: &Mutex<RelayState>, out: &Out, msg: Value) -> Result<(), String> {
    let method = msg.at("method").as_str().unwrap_or_default().to_string();
    let mut headers: Vec<(String, String)> = Vec::new();
    if !method.is_empty() {
        headers.push(("mcp-method".into(), header_value(&method)));
        if let Some(n) = mcp_name(&method, msg.at("params")) {
            headers.push(("mcp-name".into(), header_value(&n)));
        }
    }
    {
        let st = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let per_request = msg.at("params").at("_meta").at("io.modelcontextprotocol/protocolVersion").as_str();
        if method != "initialize"
            && let Some(v) = per_request.map(String::from).or_else(|| st.legacy_version.clone())
        {
            headers.push(("mcp-protocol-version".into(), v));
        }
        if per_request.is_none()
            && let Some(sid) = &st.session_id
        {
            headers.push(("mcp-session-id".into(), sid.clone()));
        }
    }
    let resp = send(app, http::Method::POST, headers, Bytes::from(msg.to_string())).await?;
    let status = resp.status();
    if status == hyper::StatusCode::UNAUTHORIZED || status == hyper::StatusCode::FORBIDDEN {
        return Err(
            "the Gusset app refused the connection (agents were switched off or the app restarted); allow agents in \
             the app if needed, then restart the bridge"
                .into(),
        );
    }
    if status == hyper::StatusCode::ACCEPTED {
        return Ok(());
    }
    if method == "initialize"
        && let Some(sid) = resp.headers().get("mcp-session-id").and_then(|v| v.to_str().ok())
    {
        state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).session_id = Some(sid.to_string());
    }
    let is_sse = resp
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|c| c.starts_with("text/event-stream"));
    let mut body = resp.into_body();
    if is_sse {
        let mut buf = String::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|e| e.to_string())?;
            if let Some(data) = frame.data_ref() {
                buf.push_str(&String::from_utf8_lossy(data));
                buf = buf.replace("\r\n", "\n");
                while let Some(end) = buf.find("\n\n") {
                    let event: String = buf.drain(..end + 2).collect();
                    let data: Vec<&str> = event
                        .lines()
                        .filter_map(|l| l.strip_prefix("data:"))
                        .map(|d| d.strip_prefix(' ').unwrap_or(d))
                        .collect();
                    if data.is_empty() {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<Value>(&data.join("\n")) {
                        note_init(state, &method, &v);
                        emit(out, &v).await;
                    }
                }
            }
        }
        return Ok(());
    }
    let bytes = body.collect().await.map_err(|e| e.to_string())?.to_bytes();
    if bytes.is_empty() {
        return if status.is_success() { Ok(()) } else { Err(format!("HTTP {status}")) };
    }
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(v) => {
            note_init(state, &method, &v);
            emit(out, &v).await;
            Ok(())
        }
        Err(_) => {
            Err(format!("HTTP {status}: {}", crate::sanitize::clean_short(&String::from_utf8_lossy(&bytes), 200)))
        }
    }
}

fn note_init(state: &Mutex<RelayState>, method: &str, v: &Value) {
    if method == "initialize"
        && let Some(ver) = v.at("result").at("protocolVersion").as_str()
    {
        state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).legacy_version = Some(ver.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gusset-bridge-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        gusset_store::fs::create_private_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn relative_project_paths_get_a_real_project_folder() {
        let rel = absolutize(Path::new("p1.gus"));
        assert!(rel.is_absolute(), "{rel:?}");
        let parent = rel.parent().unwrap();
        assert!(!parent.as_os_str().is_empty() && parent.is_dir(), "{parent:?}");
        assert_eq!(rel.file_name().unwrap(), "p1.gus");
        let abs = std::env::temp_dir().join("x.gus");
        assert_eq!(absolutize(&abs), abs);
    }

    fn write_conn(dir: &Path, port: u16, token: Option<&str>) -> PathBuf {
        let p = dir.join("app.json");
        let body = json!({"version": crate::http::CONNECTION_FILE_VERSION, "port": port, "token": token,
                          "paused": token.is_none(), "started": 1});
        std::fs::write(&p, body.to_string()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        p
    }

    /// Security review #13: a stale connection file whose port another process now
    /// holds is never used; the squatter never receives the token.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn squatted_port_is_not_trusted() {
        use tokio::io::AsyncReadExt;
        let dir = private_dir("squat");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(tokio::sync::Mutex::new(Vec::<u8>::new()));
        let seen2 = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                seen2.lock().await.extend_from_slice(buf.get(..n).unwrap_or_default());
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok").await;
            }
        });
        let token = "ab".repeat(32);
        write_conn(&dir, port, Some(&token));
        assert!(matches!(find_instance(&dir, None).await, Found::Nothing));
        let seen = seen.lock().await;
        assert!(!seen.is_empty(), "the challenge was sent");
        assert!(!String::from_utf8_lossy(&seen).contains(&token), "the token leaked to the squatter");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connection_files_need_private_owned_files_and_dir() {
        use std::os::unix::fs::PermissionsExt;
        let dir = private_dir("owner");
        let p = write_conn(&dir, 1, Some(&"ab".repeat(32)));
        let uid = trusted_dir(&dir).expect("our own private dir is trusted");
        assert!(read_connection_file(&p, uid).is_some());
        assert!(read_connection_file(&p, uid.wrapping_add(1)).is_none(), "another owner");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_connection_file(&p, uid).is_none(), "readable by others");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(trusted_dir(&dir).is_none(), "open directory");
        // No probe file is left behind.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
