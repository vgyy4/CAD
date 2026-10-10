//! `gusset mcp [file]`: the stdio entry point (ADR 0012 transport 2).
//!
//! 1. If a running Gusset app published a connection file (see [`crate::http`]) — the
//!    one serving the requested project, or the most recent one when no file is given —
//!    stdio is relayed to its HTTP endpoint with the bearer token, so the agent works on
//!    the same live model as the human.
//! 2. Otherwise the project file (or a new empty project) is opened headless in a
//!    [`gusset_host::Host`] and served over stdio with the same tools. Approvals are
//!    denied unless the client supports elicitation. On exit, unsaved changes (agent
//!    work lives on agent branches) are saved to the file; if that fails a recovery
//!    snapshot is kept.
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
    /// Read-only agent mode (`gusset mcp --read-only`): mutating tools are rejected.
    /// Headless it maps to `SessionConfig::read_only_agents`; when relaying to a running
    /// app the bridge itself refuses mutating `tools/call` requests.
    pub read_only: bool,
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
            read_only: false,
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
pub async fn run_stdio_async<R, W>(options: StdioOptions, read: R, write: W) -> Result<(), McpError>
where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    if options.attach {
        let dir = options.connection_dir.clone().or_else(crate::http::default_connection_dir);
        if let Some(dir) = dir
            && let Some(app) = find_instance(&dir, options.project.as_deref()).await
        {
            eprintln!("gusset mcp: attaching to the running Gusset app on port {}", app.port);
            return relay(app, options.read_only, read, write).await;
        }
    }
    headless(options, read, write).await
}

/// A running app found through its connection file.
#[derive(Clone)]
pub(crate) struct AppInstance {
    pub(crate) port: u16,
    token: String,
    project: Option<PathBuf>,
    started: u64,
}

impl std::fmt::Debug for AppInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppInstance").field("port", &self.port).field("project", &self.project).finish_non_exhaustive()
    }
}

fn read_connection_file(path: &Path) -> Option<AppInstance> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() || meta.len() > 64 * 1024 {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Only trust files nobody else could have written or read.
        if meta.permissions().mode() & 0o077 != 0 {
            return None;
        }
    }
    let v: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if v.at("version").as_u64() != Some(1) {
        return None;
    }
    let port = u16::try_from(v.at("port").as_u64()?).ok().filter(|p| *p != 0)?;
    let token = v.at("token").as_str()?.to_string();
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(AppInstance {
        port,
        token,
        project: v.at("project").as_str().map(PathBuf::from),
        started: v.at("started").as_u64().unwrap_or(0),
    })
}

/// Finds a reachable running app: the one serving `project` if given, else the most
/// recently started one.
pub(crate) async fn find_instance(dir: &Path, project: Option<&Path>) -> Option<AppInstance> {
    let want = project.map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));
    let mut found: Vec<AppInstance> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| read_connection_file(&p))
        .filter(|a| match (&want, &a.project) {
            (None, _) => true,
            (Some(w), Some(p)) => p.canonicalize().unwrap_or_else(|_| p.clone()) == *w,
            (Some(_), None) => false,
        })
        .collect();
    found.sort_by_key(|a| std::cmp::Reverse(a.started));
    for a in found {
        let addr = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, a.port));
        if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(1), tokio::net::TcpStream::connect(addr)).await {
            return Some(a);
        }
    }
    None
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
        Some(path) if path.exists() => {
            let (p, history, report) = gusset_project::Project::open(path, dirs)
                .map_err(|e| McpError::Command(gusset_commands::CommandError::new("io", e.to_string())))?;
            if !matches!(report, gusset_project::OpenReport::Clean) {
                eprintln!(
                    "gusset mcp: note: the project has a recovery snapshot or was damaged; open it in the app to review"
                );
            }
            let mut s = Session::from_history(history, options.kernel.clone());
            s.project_dir = path.parent().map(Path::to_path_buf);
            (s, Some(p), None)
        }
        Some(path) => {
            let p = gusset_project::Project::create(dirs)
                .map_err(|e| McpError::Command(gusset_commands::CommandError::new("io", e.to_string())))?;
            let mut s = Session::new(&empty()?, options.kernel.clone(), &CmdCtx::user(0));
            s.project_dir = path.parent().map(Path::to_path_buf);
            (s, Some(p), Some(path.clone()))
        }
        None => {
            eprintln!("gusset mcp: no project file given and no running app found: using a new, unsaved project");
            (Session::new(&empty()?, options.kernel.clone(), &CmdCtx::user(0)), None, None)
        }
    };
    let host = Host::spawn(session, project);
    let server = McpServer::new(host.clone());
    server.set_renderer(options.renderer.clone());
    server.set_read_only(options.read_only).map_err(McpError::Command)?;
    if let Some(a) = options.approvals.clone() {
        server.set_approval_provider(a);
    }
    let served = server.serve_io(read, write).await;
    // Persist agent work (it lives on agent branches; main is untouched).
    if let Some((session, project)) = host.shutdown()
        && let Some(mut project) = project
    {
        if session.dirty {
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
    let name = msg.at("params").at("name").as_str()?;
    let spec = gusset_commands::registry().iter().find(|s| s.tool_name() == name)?;
    if !spec.flags.mutating {
        return None;
    }
    let e =
        gusset_commands::CommandError::new("read_only", "this MCP connection is read-only (gusset mcp --read-only)")
            .hint(
                "use the read tools (query_*, render_view, get_text); restart the bridge without --read-only to edit",
            );
    let result = rmcp::model::CallToolResult::structured_error(json!({ "error": e }));
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
    let addr = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, app.port));
    let stream = tokio::net::TcpStream::connect(addr).await.map_err(|e| format!("cannot reach the app: {e}"))?;
    let (mut sender, conn) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await.map_err(|e| e.to_string())?;
    tokio::spawn(conn);
    let mut req = hyper::Request::builder()
        .method(method)
        .uri("/mcp")
        .header(http::header::HOST, format!("127.0.0.1:{}", app.port))
        .header(http::header::AUTHORIZATION, format!("Bearer {}", app.token))
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json, text/event-stream");
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let req = req.body(Full::new(body)).map_err(|e| e.to_string())?;
    sender.send_request(req).await.map_err(|e| e.to_string())
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
            "the Gusset app refused the connection (the token was revoked or the app restarted); restart the bridge"
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
