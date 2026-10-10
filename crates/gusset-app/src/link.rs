//! The GUI side of the model thread (ADR 0012, "Session and concurrency").
//!
//! The GUI never owns the [`Session`]: it lives on the `gusset-host` model thread, shared
//! with the MCP server. The GUI
//! - sends commands as detached jobs ([`gusset_host::HostHandle::exec_detached`]) whose
//!   results come back over a channel, so a long rebuild never freezes the window;
//! - reads what it draws as a [`Snapshot`], taken with a short, non-blocking
//!   `try_with_session` whenever the model changed;
//! - repaints when the host broadcasts a change: a listener thread wakes egui
//!   ([`Waker`]), and the frame then drains its own event receiver.

use gusset_commands::Session;
use gusset_commands::session::AuditEntry;
use gusset_doc::Document;
use gusset_history::{Actor, BranchId, NodeId};
use gusset_host::HostEvent;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Wakes the egui event loop from other threads (host listener, approvals, jobs).
#[derive(Default)]
pub struct Waker {
    ctx: Mutex<Option<egui::Context>>,
}

impl std::fmt::Debug for Waker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Waker").finish_non_exhaustive()
    }
}

impl Waker {
    /// Remembers the egui context (the first frame provides it).
    pub fn set(&self, ctx: &egui::Context) {
        let mut g = self.ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if g.is_none() {
            *g = Some(ctx.clone());
        }
    }

    /// Requests a repaint (no-op before the first frame).
    pub fn wake(&self) {
        let ctx = self.ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        if let Some(c) = ctx {
            c.request_repaint();
        }
    }
}

/// Starts the thread that turns host change broadcasts into repaints. It ends when the
/// host shuts down (or is dropped).
pub fn spawn_listener(mut rx: broadcast::Receiver<HostEvent>, waker: Arc<Waker>) {
    let r = std::thread::Builder::new().name("gusset-gui-events".into()).spawn(move || {
        loop {
            match rx.blocking_recv() {
                Ok(HostEvent::Shutdown) | Err(broadcast::error::RecvError::Closed) => return,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => waker.wake(),
            }
        }
    });
    // Without the thread the GUI still refreshes on its own commands and on input.
    drop(r);
}

/// One branch as the GUI lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub id: BranchId,
    /// Authored name (may come from an agent: show as plain text).
    pub name: String,
    /// The agent that created it, if any.
    pub agent: Option<String>,
    pub head: NodeId,
}

/// Everything the GUI draws from the session, copied in one short read.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// The user's document (head of the user's current branch).
    pub doc: Document,
    pub branch: Option<BranchId>,
    pub main: Option<BranchId>,
    pub head: Option<NodeId>,
    pub rollback: Option<usize>,
    pub branches: Vec<BranchInfo>,
    /// Labels of the last iterations on the user's branch (oldest first).
    pub lineage: Vec<String>,
    pub dirty: bool,
    pub read_only_agents: bool,
    /// Agent actions (oldest first, at most [`AUDIT_SHOWN`]).
    pub audit: Vec<AuditEntry>,
    /// Total number of audit entries in the session.
    pub audit_total: usize,
    pub project_dir: Option<PathBuf>,
}

/// How many audit entries the GUI keeps in its snapshot.
pub const AUDIT_SHOWN: usize = 500;

impl Snapshot {
    /// Reads the snapshot. Cheap (clones the user's document and some metadata); called
    /// inside `with_session`, so it must stay short.
    #[must_use]
    pub fn read(s: &Session) -> Snapshot {
        let b = s.current_branch(&Actor::User);
        let h = s.history();
        let br = h.branch(b).ok();
        let head = h.head(b).ok();
        let lineage = head
            .map(|n| h.lineage(n))
            .unwrap_or_default()
            .iter()
            .rev()
            .take(30)
            .rev()
            .map(|n| h.node(*n).map(|n| n.name.clone().unwrap_or_else(|| n.label.clone())).unwrap_or_default())
            .collect();
        let branches = h
            .branches()
            .iter()
            .map(|x| BranchInfo {
                id: x.id,
                name: x.name.clone(),
                agent: match &x.created_by {
                    Actor::Agent(a) => Some(a.clone()),
                    Actor::User => None,
                },
                head: x.head,
            })
            .collect();
        let audit = s.audit_log();
        Snapshot {
            doc: s.document(&Actor::User).unwrap_or_default(),
            branch: Some(b),
            main: Some(h.main()),
            head,
            rollback: br.and_then(|x| x.rollback),
            branches,
            lineage,
            dirty: s.dirty,
            read_only_agents: s.config.read_only_agents,
            audit: audit.iter().skip(audit.len().saturating_sub(AUDIT_SHOWN)).cloned().collect(),
            audit_total: audit.len(),
            project_dir: s.project_dir.clone(),
        }
    }

    /// The key the drawn scene is cached by.
    #[must_use]
    pub fn scene_key(&self) -> Option<(NodeId, Option<usize>)> {
        self.head.map(|h| (h, self.rollback))
    }
}
