//! Localhost Streamable HTTP transport (ADR 0012 transport 1; threat model B3).
//!
//! Every request passes, in this order:
//! 1. `Host` must be `127.0.0.1:<port>`, `localhost:<port>` or `[::1]:<port>` (DNS
//!    rebinding protection) → otherwise 403 (400 if missing).
//! 2. `Origin` must be absent or `http://127.0.0.1:<port>` / `http://localhost:<port>` /
//!    `http://[::1]:<port>` → otherwise 403 (cross-site requests from browsers).
//! 3. Path must be `/mcp` → otherwise 404.
//! 4. `Authorization: Bearer <token>` with a 256-bit random token, compared in constant
//!    time → otherwise 401.
//! 5. Per-connection rate limit → 429; declared body size limit → 413.
//!
//! Then rmcp's `StreamableHttpService` handles it, configured with the same Host
//! allow-list, Origin enforcement switched on explicitly (`enforce_origin_validation`,
//! because its default empty allow-list does nothing) and a streaming body limit. rmcp's
//! default dual-era mode is kept: 2026-07-28 requests are stateless, legacy `initialize`
//! clients get sessions. Modern-only mode is deliberately not enabled.
//!
//! The listener binds 127.0.0.1 (and optionally ::1) on a random port, never 0.0.0.0.
//! The port and token are written to an owner-only connection file for the stdio bridge;
//! the token is never logged.
//!
//! [`crate::identity::IDENTITY_PATH`] answers the bridge's identity challenge without a
//! token (Host/Origin checks still apply), and every `/mcp` response carries a proof of
//! the request's nonce, so the bridge never talks to a process squatting a stale port.
//!
//! [`Endpoint::revoke`] is the kill switch: it invalidates the token, drops every
//! connection and session, and **pauses** the endpoint. The connection file then says
//! `paused` and holds no token, so a restarted bridge cannot reconnect. Only
//! [`Endpoint::resume`] (the user allowing agents again) issues a new token.

use crate::server::{GussetMcp, Shared, Transport};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::json;
use std::convert::Infallible;
use std::io::Write as _;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

/// Version of the connection-file format (2: `paused`, `generation`, identity proofs).
pub const CONNECTION_FILE_VERSION: u64 = 2;

/// Default limit for a request body (rmcp enforces it while streaming, too).
pub const DEFAULT_MAX_BODY_BYTES: usize = 4 << 20;

type Body = BoxBody<Bytes, Infallible>;
type Service = StreamableHttpService<GussetMcp, LocalSessionManager>;

/// Options for [`crate::McpServer::start_http`].
#[derive(Debug, Clone)]
pub struct HttpOptions {
    /// Where to write the connection file (`None`: the default per-user runtime
    /// directory, see [`default_connection_dir`]). Tests pass a temp dir.
    pub connection_dir: Option<PathBuf>,
    /// Write a connection file at all (the GUI does; tests may not).
    pub write_connection_file: bool,
    /// Instance name (file stem). Default: `<pid>-<random>`.
    pub instance: Option<String>,
    /// The project file this instance serves (recorded for the stdio bridge).
    pub project: Option<PathBuf>,
    /// Also listen on `[::1]` (same port), if available.
    pub ipv6: bool,
    /// Fixed port (0 = random; the default).
    pub port: u16,
    /// Request body limit in bytes.
    pub max_body_bytes: usize,
    /// Requests per second allowed per TCP connection (burst: twice this).
    pub requests_per_second: u32,
}

impl Default for HttpOptions {
    fn default() -> Self {
        HttpOptions {
            connection_dir: None,
            write_connection_file: true,
            instance: None,
            project: None,
            ipv6: true,
            port: 0,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            requests_per_second: 50,
        }
    }
}

/// Where the running endpoint can be reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionInfo {
    /// TCP port on 127.0.0.1.
    pub port: u16,
    /// `http://127.0.0.1:<port>/mcp`.
    pub url: String,
    /// The connection file, if one was written.
    pub connection_file: Option<PathBuf>,
}

/// `<runtime dir>/mcp` (Linux `$XDG_RUNTIME_DIR`), else `<per-user data dir>/mcp`.
#[must_use]
pub fn default_connection_dir() -> Option<PathBuf> {
    let d = directories::ProjectDirs::from("org", "Gusset", "Gusset")?;
    Some(d.runtime_dir().unwrap_or_else(|| d.data_local_dir()).join("mcp"))
}

/// A secret that never prints.
#[derive(Clone, PartialEq, Eq)]
struct Token(String);

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

fn new_token() -> Option<Token> {
    // 32 bytes = 256 bits from the OS CSPRNG.
    crate::approval::random_hex(32).map(Token)
}

/// Constant-time byte comparison (time depends only on the lengths).
#[must_use]
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let n = a.len().max(b.len());
    let mut diff = u8::from(a.len() != b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= std::hint::black_box(x ^ y);
    }
    std::hint::black_box(diff) == 0
}

struct RateWindow {
    tokens: f64,
    last: Instant,
}

impl RateWindow {
    fn allow(&mut self, per_second: u32) -> bool {
        let now = Instant::now();
        let rate = f64::from(per_second.max(1));
        let burst = rate * 2.0;
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f64() * rate).min(burst);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

struct State {
    port: u16,
    options: HttpOptions,
    token: RwLock<Option<Token>>,
    /// Kill switch engaged: no token is valid until [`Endpoint::resume`].
    paused: AtomicBool,
    /// Bumped on every new token (connection-file generation).
    generation: AtomicU64,
    epoch: tokio::sync::watch::Sender<u64>,
    service: RwLock<(Service, CancellationToken)>,
    shared: Arc<Shared>,
    file: Option<PathBuf>,
    instance: String,
    started: u64,
}

impl State {
    fn hosts(&self) -> [String; 3] {
        let p = self.port;
        [format!("127.0.0.1:{p}"), format!("localhost:{p}"), format!("[::1]:{p}")]
    }

    fn origins(&self) -> [String; 3] {
        self.hosts().map(|h| format!("http://{h}"))
    }
}

fn make_service(st_shared: &Arc<Shared>, port: u16, max_body: usize) -> (Service, CancellationToken) {
    let ct = CancellationToken::new();
    let hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")];
    let origins = hosts.clone().map(|h| format!("http://{h}"));
    let config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(hosts)
        .with_allowed_origins(origins)
        .enforce_origin_validation()
        .with_max_request_body_bytes(max_body)
        .with_cancellation_token(ct.clone());
    let shared = st_shared.clone();
    let svc = StreamableHttpService::new(
        move || Ok(GussetMcp::new(shared.clone(), Transport::Http)),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    (svc, ct)
}

fn plain(status: StatusCode, msg: &'static str) -> Response<Body> {
    let mut r = Response::new(Full::new(Bytes::from_static(msg.as_bytes())).boxed());
    *r.status_mut() = status;
    r
}

fn header_eq_any(v: &str, allowed: &[String]) -> bool {
    allowed.iter().any(|a| a.eq_ignore_ascii_case(v))
}

/// What a request that passed the pre-checks is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Mcp,
    Identity,
}

/// The pre-checks; `Err` is the response to send instead.
fn check(req: &Request<Incoming>, st: &State) -> Result<Route, Box<Response<Body>>> {
    let headers = req.headers();
    let mut hosts = headers.get_all(http::header::HOST).iter();
    let host = match (hosts.next(), hosts.next()) {
        (Some(h), None) => {
            h.to_str().map_err(|_| Box::new(plain(StatusCode::BAD_REQUEST, "Bad Request: invalid Host")))?
        }
        _ => return Err(Box::new(plain(StatusCode::BAD_REQUEST, "Bad Request: exactly one Host header is required"))),
    };
    if !header_eq_any(host, &st.hosts()) {
        return Err(Box::new(plain(StatusCode::FORBIDDEN, "Forbidden: Host not allowed")));
    }
    let mut origins = headers.get_all(http::header::ORIGIN).iter();
    match (origins.next(), origins.next()) {
        (None, _) => {}
        (Some(o), None) => {
            let ok = o.to_str().is_ok_and(|o| header_eq_any(o, &st.origins()));
            if !ok {
                return Err(Box::new(plain(StatusCode::FORBIDDEN, "Forbidden: Origin not allowed")));
            }
        }
        _ => return Err(Box::new(plain(StatusCode::FORBIDDEN, "Forbidden: Origin not allowed"))),
    }
    if req.uri().path() == crate::identity::IDENTITY_PATH {
        return Ok(Route::Identity);
    }
    if req.uri().path() != "/mcp" {
        return Err(Box::new(plain(StatusCode::NOT_FOUND, "Not Found")));
    }
    let token = st.token.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let mut auths = headers.get_all(http::header::AUTHORIZATION).iter();
    let presented = match (auths.next(), auths.next()) {
        (Some(a), None) => a.as_bytes(),
        _ => b"",
    };
    let ok = match (token, presented.get(..7)) {
        (Some(t), Some(scheme)) if scheme.eq_ignore_ascii_case(b"bearer ") => {
            ct_eq(presented.get(7..).unwrap_or_default(), t.0.as_bytes())
        }
        _ => false,
    };
    if !ok {
        let mut r = plain(StatusCode::UNAUTHORIZED, "Unauthorized");
        r.headers_mut()
            .insert(http::header::WWW_AUTHENTICATE, http::HeaderValue::from_static("Bearer realm=\"gusset\""));
        return Err(Box::new(r));
    }
    let len = headers
        .get(http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    if len > st.options.max_body_bytes as u64 {
        return Err(Box::new(plain(StatusCode::PAYLOAD_TOO_LARGE, "Payload Too Large")));
    }
    Ok(Route::Mcp)
}

fn nonce_of(req: &Request<Incoming>) -> Option<String> {
    req.headers()
        .get(crate::identity::NONCE_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|n| n.len() == 64 && n.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
}

/// Adds the identity proof for `nonce` to `resp`.
fn add_proof(resp: &mut Response<Body>, st: &State, nonce: Option<&str>) {
    let token = st.token.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    if let (Some(t), Some(n)) = (token, nonce)
        && let Some(p) = crate::identity::proof(&t.0, n)
        && let Ok(v) = http::HeaderValue::from_str(&p)
    {
        resp.headers_mut().insert(crate::identity::PROOF_HEADER, v);
    }
}

/// Answers an identity challenge: 200 with the proof, 503 while paused (no token).
fn identity(req: &Request<Incoming>, st: &State) -> Response<Body> {
    if st.paused.load(Ordering::SeqCst) {
        return plain(StatusCode::SERVICE_UNAVAILABLE, "Agents are paused in the Gusset app");
    }
    let Some(nonce) = nonce_of(req) else {
        return plain(StatusCode::BAD_REQUEST, "Bad Request: missing nonce");
    };
    let mut r = plain(StatusCode::OK, "gusset");
    add_proof(&mut r, st, Some(&nonce));
    r
}

async fn handle(
    req: Request<Incoming>,
    st: Arc<State>,
    rate: Arc<Mutex<RateWindow>>,
) -> Result<Response<Body>, Infallible> {
    let route = match check(&req, &st) {
        Ok(r) => r,
        Err(r) => return Ok(*r),
    };
    if !rate.lock().unwrap_or_else(std::sync::PoisonError::into_inner).allow(st.options.requests_per_second) {
        let mut r = plain(StatusCode::TOO_MANY_REQUESTS, "Too Many Requests");
        r.headers_mut().insert(http::header::RETRY_AFTER, http::HeaderValue::from_static("1"));
        return Ok(r);
    }
    if route == Route::Identity {
        return Ok(identity(&req, &st));
    }
    let nonce = nonce_of(&req);
    let svc = st.service.read().unwrap_or_else(std::sync::PoisonError::into_inner).0.clone();
    let mut resp = svc.handle(req).await;
    add_proof(&mut resp, &st, nonce.as_deref());
    Ok(resp)
}

async fn serve_conn(stream: TcpStream, st: Arc<State>, stop: CancellationToken) {
    let rate = Arc::new(Mutex::new(RateWindow {
        tokens: f64::from(st.options.requests_per_second.max(1)) * 2.0,
        last: Instant::now(),
    }));
    let mut epoch = st.epoch.subscribe();
    let st2 = st.clone();
    let svc = hyper::service::service_fn(move |req| handle(req, st2.clone(), rate.clone()));
    let conn = hyper::server::conn::http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(Duration::from_secs(30))
        .max_buf_size(64 * 1024)
        .serve_connection(TokioIo::new(stream), svc);
    tokio::select! {
        _ = conn => {}
        _ = epoch.changed() => {}
        () = stop.cancelled() => {}
    }
}

async fn accept_loop(listener: TcpListener, st: Arc<State>, stop: CancellationToken) {
    loop {
        tokio::select! {
            () = stop.cancelled() => return,
            r = listener.accept() => {
                let Ok((stream, peer)) = r else {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                };
                // Bound to loopback, so this always holds; checked anyway.
                if !peer.ip().is_loopback() {
                    continue;
                }
                let _ = stream.set_nodelay(true);
                tokio::spawn(serve_conn(stream, st.clone(), stop.clone()));
            }
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Writes the connection file atomically with owner-only permissions (0600 on Unix).
/// On Windows the file lives in the per-user profile, whose default ACL already limits
/// it to the user, SYSTEM and administrators; an explicit user-only DACL is a TODO that
/// needs `unsafe` Win32 calls (tracked in docs/THREAT_MODEL.md B3).
fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| std::io::Error::other("connection file has no parent directory"))?;
    gusset_store::fs::create_private_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("conn"),
        crate::approval::random_hex(8).unwrap_or_default()
    ));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = (|| {
        let mut f = opts.open(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
        gusset_store::fs::make_private_file(&tmp)?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

impl State {
    fn write_file(&self) -> std::io::Result<()> {
        let Some(path) = &self.file else { return Ok(()) };
        let token = self.token.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let body = json!({
            "version": CONNECTION_FILE_VERSION,
            "instance": self.instance,
            "pid": std::process::id(),
            "port": self.port,
            "url": format!("http://127.0.0.1:{}/mcp", self.port),
            "token": token.map(|t| t.0),
            "paused": self.paused.load(Ordering::SeqCst),
            "generation": self.generation.load(Ordering::SeqCst),
            "project": self.options.project.as_ref().map(|p| p.to_string_lossy().into_owned()),
            "started": self.started,
        });
        write_private(path, body.to_string().as_bytes())
    }
}

/// A running HTTP endpoint (owned by [`crate::McpServer`]).
pub(crate) struct Endpoint {
    st: Arc<State>,
    stop: CancellationToken,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint").field("port", &self.st.port).finish_non_exhaustive()
    }
}

impl Endpoint {
    pub(crate) async fn start(shared: Arc<Shared>, options: HttpOptions) -> Result<Endpoint, crate::McpError> {
        let token =
            new_token().ok_or_else(|| crate::McpError::Config("the OS random number generator failed".into()))?;
        let v4 = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, options.port))).await?;
        let port = v4.local_addr()?.port();
        let v6 = if options.ipv6 {
            TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, port))).await.ok()
        } else {
            None
        };
        let instance = options.instance.clone().unwrap_or_else(|| {
            format!("{}-{}", std::process::id(), crate::approval::random_hex(4).unwrap_or_default())
        });
        if !instance.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || instance.len() > 64 {
            return Err(crate::McpError::Config("invalid instance name".into()));
        }
        let file = if options.write_connection_file {
            let dir = match &options.connection_dir {
                Some(d) => d.clone(),
                None => default_connection_dir()
                    .ok_or_else(|| crate::McpError::Config("no per-user directory for the connection file".into()))?,
            };
            Some(dir.join(format!("{instance}.json")))
        } else {
            None
        };
        let service = make_service(&shared, port, options.max_body_bytes);
        let (epoch, _) = tokio::sync::watch::channel(0u64);
        let st = Arc::new(State {
            port,
            options,
            token: RwLock::new(Some(token)),
            paused: AtomicBool::new(false),
            generation: AtomicU64::new(1),
            epoch,
            service: RwLock::new(service),
            shared,
            file,
            instance,
            started: now_secs(),
        });
        st.write_file()?;
        let stop = CancellationToken::new();
        tokio::spawn(accept_loop(v4, st.clone(), stop.clone()));
        if let Some(v6) = v6 {
            tokio::spawn(accept_loop(v6, st.clone(), stop.clone()));
        }
        Ok(Endpoint { st, stop })
    }

    pub(crate) fn info(&self) -> ConnectionInfo {
        ConnectionInfo {
            port: self.st.port,
            url: format!("http://127.0.0.1:{}/mcp", self.st.port),
            connection_file: self.st.file.clone(),
        }
    }

    pub(crate) fn token(&self) -> Option<String> {
        self.st.token.read().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().map(|t| t.0.clone())
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.st.paused.load(Ordering::SeqCst)
    }

    /// Kill switch: no token is valid any more, every connection and MCP session is
    /// dropped, and the connection file is rewritten as `paused` without a token, so a
    /// restarted bridge cannot reconnect until [`Endpoint::resume`].
    pub(crate) fn revoke(&self) -> Result<(), crate::McpError> {
        self.st.paused.store(true, Ordering::SeqCst);
        *self.st.token.write().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        // Drop every open connection and MCP session.
        self.st.epoch.send_modify(|e| *e += 1);
        {
            let mut svc = self.st.service.write().unwrap_or_else(std::sync::PoisonError::into_inner);
            svc.1.cancel();
            *svc = make_service(&self.st.shared, self.st.port, self.st.options.max_body_bytes);
        }
        self.st.write_file()?;
        Ok(())
    }

    /// The user allows agents again: a new token (new connection-file generation).
    pub(crate) fn resume(&self) -> Result<(), crate::McpError> {
        let new = new_token().ok_or_else(|| crate::McpError::Config("the OS random number generator failed".into()))?;
        *self.st.token.write().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(new);
        self.st.generation.fetch_add(1, Ordering::SeqCst);
        self.st.paused.store(false, Ordering::SeqCst);
        self.st.write_file()?;
        Ok(())
    }

    pub(crate) fn stop(&self) {
        self.stop.cancel();
        self.st.service.read().unwrap_or_else(std::sync::PoisonError::into_inner).1.cancel();
        self.st.epoch.send_modify(|e| *e += 1);
        if let Some(f) = &self.st.file {
            let _ = std::fs::remove_file(f);
        }
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(!ct_eq(b"", b"a"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn token_never_prints() {
        let t = new_token().unwrap();
        assert_eq!(t.0.len(), 64);
        assert!(!format!("{t:?}").contains(&t.0));
    }

    #[cfg(unix)]
    #[test]
    fn connection_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("gusset-mcp-conn-{}", std::process::id()));
        let p = d.join("x.json");
        write_private(&p, b"{}").unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(&d);
    }
}
