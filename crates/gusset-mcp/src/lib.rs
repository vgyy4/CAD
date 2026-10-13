//! Gusset's built-in MCP server (ADR 0012; threat model B3/B4).
//!
//! - **Tools** are generated from [`gusset_commands::registry()`] (name = `tool_name()`),
//!   plus [`server::RENDER_VIEW`] and [`server::GET_TEXT`].
//! - **Resources** under `gusset://project/...` ([`server::resource_list`]).
//! - **Agent identity:** each connection gets an agent name (`<client>-<n>`) and works
//!   on its own branch; `CmdCtx::approved` is never taken from tool arguments.
//! - **Approvals:** `approval_required` → the in-app [`ApprovalProvider`] when the GUI
//!   hosts the session (never client elicitation, which the client can answer itself);
//!   headless: MCP elicitation (2026-07-28 `input_required`, or legacy
//!   `elicitation/create`), else deny.
//! - **Output sanitization:** [`sanitize`].
//! - **Transports:** in-process/stdio ([`McpServer::serve_io`]), localhost Streamable
//!   HTTP with bearer token, Host and Origin checks ([`McpServer::start_http`]), and the
//!   `gusset mcp [file]` stdio bridge ([`run_stdio`]).
//!
//! The server never offers shell, network, settings or arbitrary file access: the only
//! tools are registry commands (file commands are confined to the project folder) and
//! the two read-only helpers.

pub mod agents;
pub mod approval;
pub mod b64;
pub mod bridge;
pub mod grants;
pub mod guidance;
pub mod http;
pub mod identity;
mod json_ext;
pub mod limits;
pub mod render;
pub mod sanitize;
#[cfg(feature = "render")]
pub mod scene_renderer;
pub mod server;

pub use approval::{ApprovalDecision, ApprovalProvider, ApprovalRequest, ApproveAll, DenyApprovals};
pub use bridge::{StdioOptions, run_stdio, run_stdio_async, run_stdio_with_output};
pub use http::{ConnectionInfo, HttpOptions};
pub use render::{Camera, NamedView, RenderBody, RenderRequest, RenderScene, RenderStyle, StubRenderer, ViewRenderer};
#[cfg(feature = "render")]
pub use scene_renderer::{RenderBackend, SceneRenderer};
pub use server::GussetMcp;

use gusset_commands::CommandError;
use gusset_host::HostHandle;
use rmcp::ServiceExt;
use server::{Shared, Transport};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncWrite};

/// Errors of the MCP server's transports.
#[derive(Debug)]
pub enum McpError {
    /// Binding, accepting or relaying failed.
    Io(std::io::Error),
    /// The MCP protocol layer failed (e.g. the initial handshake).
    Protocol(String),
    /// Opening the project or another command failed.
    Command(CommandError),
    /// Invalid configuration (e.g. HTTP already running).
    Config(String),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpError::Io(e) => write!(f, "I/O error: {e}"),
            McpError::Protocol(m) => write!(f, "MCP protocol error: {m}"),
            McpError::Command(e) => write!(f, "{e}"),
            McpError::Config(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for McpError {}

impl From<std::io::Error> for McpError {
    fn from(e: std::io::Error) -> Self {
        McpError::Io(e)
    }
}

/// The MCP server of one open project. Cheap to clone; all clones share state.
#[derive(Clone, Debug)]
pub struct McpServer {
    shared: Arc<Shared>,
    http: Arc<Mutex<Option<http::Endpoint>>>,
}

impl McpServer {
    /// A server for the project hosted by `host`. No renderer and the headless
    /// [`DenyApprovals`] provider until configured.
    #[must_use]
    pub fn new(host: HostHandle) -> McpServer {
        McpServer { shared: Arc::new(Shared::new(host)), http: Arc::new(Mutex::new(None)) }
    }

    /// The host this server drives.
    #[must_use]
    pub fn host(&self) -> &HostHandle {
        &self.shared.host
    }

    /// Installs (or removes) the `render_view` renderer.
    pub fn set_renderer(&self, renderer: Option<Arc<dyn ViewRenderer>>) {
        self.shared.set_renderer(renderer);
    }

    /// Installs the in-app approval prompt (the GUI's [`ApprovalProvider`]). From then on
    /// every high-impact approval (set main, delete branch, overwrite, bulk, edits to
    /// main) is asked **only** through it: MCP elicitation is never used, because an
    /// elicitation answer comes from the client, which can answer for itself (security
    /// review #1). Without a provider (headless `gusset mcp`), elicitation is the only
    /// channel, and clients that cannot elicit are denied.
    pub fn set_approval_provider(&self, provider: Arc<dyn ApprovalProvider>) {
        self.shared.set_approvals(Some(provider));
    }

    /// Back to headless approvals (elicitation, else deny).
    pub fn clear_approval_provider(&self) {
        self.shared.set_approvals(None);
    }

    /// Replaces the diagnostics text (the GUI adds GPU details once the renderer chose an
    /// adapter). Defaults to `gusset_hw::Diagnostics::collect()`.
    pub fn set_diagnostics(&self, text: String) {
        self.shared.set_diagnostics(text);
    }

    /// Longest a tool call may run before it is stopped and undone
    /// ([`server::DEFAULT_REQUEST_DEADLINE`] unless set).
    pub fn set_request_deadline(&self, deadline: std::time::Duration) {
        self.shared.set_request_deadline(deadline);
    }

    /// Read-only agent mode (maps to `SessionConfig::read_only_agents`).
    ///
    /// # Errors
    /// When the host has shut down.
    pub fn set_read_only(&self, read_only: bool) -> Result<(), CommandError> {
        self.shared.host.with_session(|s| s.config.read_only_agents = read_only)
    }

    /// Standing approvals ("always allow", ADR 0012): list, revoke, suspend/resume and
    /// load the remembered ones. New grants come only from the in-app prompt.
    #[must_use]
    pub fn grants(&self) -> Arc<grants::Grants> {
        self.shared.grants()
    }

    /// Shares one grant store between the servers of the projects a window opens.
    pub fn set_grants(&self, grants: Arc<grants::Grants>) {
        self.shared.set_grants(grants);
    }

    /// A handler for one stream connection (stdio or in-process).
    #[must_use]
    pub fn handler(&self) -> GussetMcp {
        GussetMcp::new(self.shared.clone(), Transport::Stream)
    }

    /// Serves one MCP connection over a byte stream (stdio, a pipe or an in-memory duplex)
    /// until the peer disconnects. Dual-era: answers both `server/discover`
    /// (2026-07-28) and `initialize` (2025-11-25 and earlier).
    ///
    /// # Errors
    /// When the connection fails before or during the handshake.
    pub async fn serve_io<R, W>(&self, read: R, write: W) -> Result<(), McpError>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let running = self.handler().serve((read, write)).await.map_err(|e| McpError::Protocol(e.to_string()))?;
        running.waiting().await.map_err(|e| McpError::Protocol(e.to_string()))?;
        Ok(())
    }

    /// Starts the localhost Streamable HTTP endpoint (`http://127.0.0.1:<port>/mcp`) with
    /// bearer-token auth, Host/Origin validation, size and rate limits, and writes the
    /// owner-only connection file. Requires a tokio runtime.
    ///
    /// # Errors
    /// `Config` if already running; I/O errors binding or writing the connection file.
    pub async fn start_http(&self, options: HttpOptions) -> Result<ConnectionInfo, McpError> {
        if self.http.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_some() {
            return Err(McpError::Config("the MCP HTTP endpoint is already running".into()));
        }
        let ep = http::Endpoint::start(self.shared.clone(), options).await?;
        let info = ep.info();
        *self.http.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(ep);
        Ok(info)
    }

    /// Port, URL and connection-file path of the running HTTP endpoint.
    #[must_use]
    pub fn http_info(&self) -> Option<ConnectionInfo> {
        self.http.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().map(http::Endpoint::info)
    }

    /// The current bearer token, for the GUI to show in its "Connect an agent" dialog.
    /// `None` while agents are paused by the kill switch. Never log it.
    #[must_use]
    pub fn bearer_token(&self) -> Option<String> {
        self.http.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().and_then(http::Endpoint::token)
    }

    /// Live legacy MCP sessions on the HTTP endpoint.
    #[must_use]
    pub fn http_session_count(&self) -> usize {
        self.http
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map_or(0, http::Endpoint::session_count)
    }

    /// True after [`McpServer::revoke`] until [`McpServer::allow_agents`].
    #[must_use]
    pub fn agents_paused(&self) -> bool {
        self.http
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(http::Endpoint::is_paused)
    }

    /// Kill switch: invalidates the token at once, drops every HTTP connection and MCP
    /// session, and **pauses** agent access. The connection file is rewritten without a
    /// token (`"paused": true`), so a restarted `gusset mcp` bridge cannot silently
    /// reconnect; it reports that agents are switched off in the app. Agents get access
    /// again only after the user calls [`McpServer::allow_agents`].
    ///
    /// # Errors
    /// When the connection file cannot be rewritten (the token is invalidated and
    /// connections dropped regardless).
    pub fn revoke(&self) -> Result<(), McpError> {
        // Standing approvals are suspended too, until the user resumes them explicitly
        // (the in-memory state changes even if the grant file cannot be written).
        let _ = self.shared.grants().set_suspended(true);
        match self.http.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
            Some(ep) => ep.revoke(),
            None => Ok(()),
        }
    }

    /// The user allows agents again after [`McpServer::revoke`]: a new token and a new
    /// connection-file generation. The GUI calls this from an explicit "Allow agents
    /// again" action, never automatically.
    ///
    /// # Errors
    /// When no token can be generated or the connection file cannot be written.
    pub fn allow_agents(&self) -> Result<(), McpError> {
        match self.http.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
            Some(ep) => ep.resume(),
            None => Ok(()),
        }
    }

    /// Stops the HTTP endpoint and deletes the connection file.
    pub fn shutdown(&self) {
        let ep = self.http.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(ep) = ep {
            ep.stop();
        }
    }
}
