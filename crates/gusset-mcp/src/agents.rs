//! Agent identity: every MCP connection (or, for stateless 2026-07-28 HTTP requests,
//! every client name) gets a stable agent name `<client>-<n>`, used for
//! `CmdCtx::agent` and thus for the agent's branch `agent/<client>-<n>`.

use std::collections::HashMap;
use std::sync::Mutex;

/// Turns a client-supplied name into `[a-z0-9-]{1,32}` (default `agent`).
#[must_use]
pub fn sanitize_client_name(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.len() >= 32 {
            break;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "agent".into() } else { out }
}

#[derive(Debug, Default)]
struct Inner {
    by_key: HashMap<String, String>,
    counters: HashMap<String, u32>,
}

/// Maps connection keys to agent names.
#[derive(Debug, Default)]
pub struct AgentRegistry {
    inner: Mutex<Inner>,
}

const MAX_KEYS: usize = 10_000;

impl AgentRegistry {
    /// The agent name for connection `key` whose client calls itself `client`.
    pub fn name_for(&self, key: &str, client: &str) -> String {
        let mut g = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(n) = g.by_key.get(key) {
            return n.clone();
        }
        let base = sanitize_client_name(client);
        let c = g.counters.entry(base.clone()).or_insert(0);
        *c += 1;
        let name = format!("{base}-{c}");
        if g.by_key.len() >= MAX_KEYS {
            g.by_key.clear();
        }
        g.by_key.insert(key.to_string(), name.clone());
        name
    }

    /// Forgets connection `key` (the connection closed).
    pub fn forget(&self, key: &str) {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner).by_key.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_sanitized_and_counted() {
        assert_eq!(sanitize_client_name("Claude Code"), "claude-code");
        assert_eq!(sanitize_client_name("../../evil\u{202E}name"), "evil-name");
        assert_eq!(sanitize_client_name(""), "agent");
        assert_eq!(sanitize_client_name(&"x".repeat(100)).len(), 32);
        let r = AgentRegistry::default();
        assert_eq!(r.name_for("conn:1", "Claude Code"), "claude-code-1");
        assert_eq!(r.name_for("conn:2", "claude-code"), "claude-code-2");
        assert_eq!(r.name_for("conn:1", "other"), "claude-code-1");
    }
}
