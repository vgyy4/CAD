//! Human approval for high-impact agent actions (ADR 0012 "Human approval").
//!
//! When a command answers `approval_required`, the MCP layer asks the **human**:
//! through MCP elicitation when the client supports it (2026-07-28: an
//! `input_required` result; legacy: `elicitation/create`), otherwise through an
//! [`ApprovalProvider`] (the GUI's in-app prompt; headless default: deny). Only an
//! approval obtained this way sets `CmdCtx::approved`; tool arguments never can.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What the human is asked to approve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    /// Agent name (sanitized, e.g. `claude-code-1`).
    pub agent: String,
    /// MCP tool name.
    pub tool: String,
    /// Registry command id.
    pub command: String,
    /// Command title (e.g. "Set as main").
    pub title: String,
    /// Why approval is needed (the command's message).
    pub reason: String,
    /// Short, sanitized summary of the arguments (agent-supplied: show as data).
    pub arguments: String,
    /// What the arguments refer to, resolved by the server (e.g. which branch is main).
    pub context: String,
}

impl ApprovalRequest {
    /// The prompt shown to the human.
    #[must_use]
    pub fn prompt(&self) -> String {
        format!(
            "The AI agent \"{}\" wants to run \"{}\" ({}). {}. Arguments: {}.{} Approve?",
            self.agent,
            self.title,
            self.tool,
            self.reason,
            self.arguments,
            if self.context.is_empty() { String::new() } else { format!(" Affects: {}.", self.context) }
        )
    }
}

/// The human's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Run the command with approval.
    Approved,
    /// Run it, and from now on approve what `scope` covers for this agent without asking
    /// (ADR 0012 "Standing approvals"). Only the app's in-app prompt may return this;
    /// `remember` keeps the grant after a restart.
    ApprovedAlways { scope: crate::grants::GrantScope, remember: bool },
    /// Do not run it; the reason is passed to the agent.
    Denied(String),
}

/// Asks the human. The GUI implements this with an in-app prompt that blocks only this
/// request. Called on a blocking worker thread; it may block until the human answers.
pub trait ApprovalProvider: Send + Sync + 'static {
    /// Shows `request` to the user and returns their decision.
    fn request_approval(&self, request: &ApprovalRequest) -> ApprovalDecision;
}

/// Headless default: nobody can be asked, so everything is denied with a clear message.
#[derive(Debug, Default, Clone, Copy)]
pub struct DenyApprovals;

impl ApprovalProvider for DenyApprovals {
    fn request_approval(&self, _request: &ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Denied(
            "no one can approve this here: the client does not support MCP elicitation and Gusset runs \
             headless. Ask the user to do it in the Gusset app, or to connect with a client that supports \
             elicitation"
                .into(),
        )
    }
}

/// A test/automation double that approves everything. Never install it in the app.
#[derive(Debug, Default, Clone, Copy)]
pub struct ApproveAll;

impl ApprovalProvider for ApproveAll {
    fn request_approval(&self, _request: &ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Approved
    }
}

#[derive(Debug)]
struct Pending {
    agent: String,
    tool: String,
    args: serde_json::Value,
    created: Instant,
}

/// Approvals asked through a 2026-07-28 `input_required` result, waiting for the
/// client's retry. The `requestState` the client echoes is only an opaque random
/// handle; everything that matters is kept here and re-checked (same agent, tool and
/// arguments, single use, expiry).
#[derive(Debug)]
pub(crate) struct PendingApprovals {
    map: Mutex<HashMap<String, Pending>>,
    ttl: Duration,
}

impl Default for PendingApprovals {
    fn default() -> Self {
        PendingApprovals { map: Mutex::new(HashMap::new()), ttl: Duration::from_secs(15 * 60) }
    }
}

const MAX_PENDING: usize = 64;

impl PendingApprovals {
    pub(crate) fn insert(&self, agent: &str, tool: &str, args: serde_json::Value) -> Option<String> {
        let handle = random_hex(16)?;
        let mut m = self.map.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let ttl = self.ttl;
        m.retain(|_, p| p.created.elapsed() < ttl);
        if m.len() >= MAX_PENDING {
            let oldest = m.iter().min_by_key(|(_, p)| p.created).map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                m.remove(&k);
            }
        }
        m.insert(handle.clone(), Pending { agent: agent.into(), tool: tool.into(), args, created: Instant::now() });
        Some(handle)
    }

    /// Takes (single use) the pending approval for `handle` if it matches exactly.
    pub(crate) fn take(&self, handle: &str, agent: &str, tool: &str, args: &serde_json::Value) -> bool {
        let mut m = self.map.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match m.remove(handle) {
            Some(p) => p.agent == agent && p.tool == tool && &p.args == args && p.created.elapsed() < self.ttl,
            None => false,
        }
    }
}

/// `n` random bytes from the OS CSPRNG as lower-case hex.
pub(crate) fn random_hex(n: usize) -> Option<String> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).ok()?;
    Some(b.iter().map(|x| format!("{x:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pending_is_single_use_and_bound() {
        let p = PendingApprovals::default();
        let h = p.insert("a-1", "branch_set_main", json!({"branch": "x"})).unwrap();
        assert_eq!(h.len(), 32);
        assert!(!p.take(&h, "a-2", "branch_set_main", &json!({"branch": "x"})), "other agent");
        let h = p.insert("a-1", "branch_set_main", json!({"branch": "x"})).unwrap();
        assert!(!p.take(&h, "a-1", "branch_set_main", &json!({"branch": "y"})), "other args");
        let h = p.insert("a-1", "branch_set_main", json!({"branch": "x"})).unwrap();
        assert!(p.take(&h, "a-1", "branch_set_main", &json!({"branch": "x"})));
        assert!(!p.take(&h, "a-1", "branch_set_main", &json!({"branch": "x"})), "single use");
        assert!(!p.take("forged", "a-1", "branch_set_main", &json!({"branch": "x"})));
    }
}
