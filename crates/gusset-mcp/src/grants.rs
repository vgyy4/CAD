//! Standing approvals ("always allow") for agents (ADR 0012 "Standing approvals").
//!
//! When the **app** hosts the MCP server, the user can answer an approval prompt with
//! "always allow this action for this agent" or "always allow all actions for this
//! agent", optionally remembered after a restart. Later requests that a grant covers
//! are approved without a prompt and audit-logged as auto-approved.
//!
//! Rules (each one has a test):
//! - Grants are created only from the in-app prompt ([`ApprovalDecision::ApprovedAlways`]
//!   returned by the app's provider). Tool arguments, MCP elicitation answers and the
//!   headless server can never create or use one.
//! - A grant names an agent **identity**: the sanitized client name without the
//!   per-connection `-<n>` ([`crate::limits::identity_of`]), so it survives reconnects
//!   and restarts.
//! - "All actions" never covers destructive or file-writing commands (deleting a
//!   branch, overwriting an exported file): those keep asking unless that one action
//!   was granted on its own.
//! - The kill switch suspends every grant until the user resumes them; a suspended
//!   store approves nothing.
//! - Remembered grants live in an owner-only file written with `durable_write`. A file
//!   that is not a regular file, too large or malformed is ignored (no grants), and at
//!   most [`MAX_GRANTS`] entries are kept.
//!
//! [`ApprovalDecision::ApprovedAlways`]: crate::approval::ApprovalDecision::ApprovedAlways

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// At most this many grants exist at once.
pub const MAX_GRANTS: usize = 256;
const MAX_FILE: u64 = 256 * 1024;
const VERSION: u32 = 1;

/// What a grant allows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantScope {
    /// One registry command (e.g. `branch.set_main`).
    Command(String),
    /// Every command that is neither destructive nor writes files.
    All,
}

/// One standing approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// Agent identity (sanitized client name, e.g. `claude-code`).
    pub agent: String,
    pub scope: GrantScope,
    /// Kept after the app closes.
    pub remember: bool,
    /// Unix seconds when the user granted it.
    pub created: u64,
}

impl Grant {
    /// Short description for the audit log and the Agents panel.
    #[must_use]
    pub fn describe(&self) -> String {
        let what = match &self.scope {
            GrantScope::Command(c) => format!("always allow {c}"),
            GrantScope::All => "always allow all actions".to_string(),
        };
        format!("{what} for {}{}", self.agent, if self.remember { "" } else { " (this session)" })
    }

    fn covers(&self, identity: &str, command: &str) -> bool {
        self.agent == identity
            && match &self.scope {
                GrantScope::Command(c) => c == command,
                GrantScope::All => blanket_ok(command),
            }
    }
}

/// Whether "all actions" may cover `command`: registered, and neither destructive,
/// writing files, nor a command that always needs approval (set main). Edits to the
/// main branch are excluded too, by the caller ([`Grants::covering`] `main_edit`).
#[must_use]
pub fn blanket_ok(command: &str) -> bool {
    gusset_commands::find(command)
        .is_some_and(|s| !s.flags.destructive && !s.flags.writes_files && !s.flags.user_only && !s.flags.agent_approval)
}

fn valid(g: &Grant) -> bool {
    // The identity is stored as derived once from the agent name (`identity_of`); it is
    // not re-derived here, so a client named e.g. "gpt-5" keeps identity "gpt-5".
    crate::agents::sanitize_client_name(&g.agent) == g.agent
        && match &g.scope {
            GrantScope::Command(c) => gusset_commands::find(c).is_some_and(|s| !s.flags.user_only),
            GrantScope::All => true,
        }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileData {
    version: u32,
    suspended: bool,
    grants: Vec<Grant>,
}

#[derive(Debug, Default)]
struct Inner {
    grants: Vec<Grant>,
    suspended: bool,
    file: Option<PathBuf>,
}

/// The grant store of one app instance.
#[derive(Debug, Default)]
pub struct Grants {
    inner: Mutex<Inner>,
}

/// Why the grant file could not be read or written.
#[derive(Debug)]
pub struct GrantFileError(pub String);

impl std::fmt::Display for GrantFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GrantFileError {}

impl Grants {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Loads remembered grants from `path` (missing file: none) and remembers the path
    /// for later saves. A damaged file yields no grants and an error; the next save
    /// replaces it.
    ///
    /// # Errors
    /// When the file is not a regular file of sane size or does not parse.
    pub fn load(&self, path: &Path) -> Result<usize, GrantFileError> {
        let mut g = self.lock();
        g.file = Some(path.to_path_buf());
        g.grants.retain(|x| !x.remember);
        let bytes = match std::fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() || !m.is_file() || m.len() > MAX_FILE => {
                return Err(GrantFileError("the agent grant file is not a regular file of sane size".into()));
            }
            Ok(_) => std::fs::read(path).map_err(|e| GrantFileError(e.to_string()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(GrantFileError(e.to_string())),
        };
        let data: FileData =
            serde_json::from_slice(&bytes).map_err(|e| GrantFileError(format!("agent grant file: {e}")))?;
        if data.version != VERSION {
            return Err(GrantFileError(format!("agent grant file version {} is not supported", data.version)));
        }
        g.suspended = data.suspended;
        let mut n = 0;
        for x in data.grants.into_iter().filter(|x| x.remember && valid(x)) {
            if g.grants.len() >= MAX_GRANTS {
                break;
            }
            if !g.grants.iter().any(|y| y.agent == x.agent && y.scope == x.scope) {
                g.grants.push(x);
                n += 1;
            }
        }
        Ok(n)
    }

    fn save(g: &Inner) -> Result<(), GrantFileError> {
        let Some(path) = &g.file else { return Ok(()) };
        let data = FileData {
            version: VERSION,
            suspended: g.suspended,
            grants: g.grants.iter().filter(|x| x.remember).cloned().collect(),
        };
        let bytes = serde_json::to_vec_pretty(&data).map_err(|e| GrantFileError(e.to_string()))?;
        let fs = gusset_store::fs::RealFs::private();
        if let Some(dir) = path.parent() {
            gusset_store::fs::create_private_dir_all(dir).map_err(|e| GrantFileError(e.to_string()))?;
        }
        gusset_store::durable::durable_write_unchecked(&fs, path, &bytes).map_err(|e| GrantFileError(e.to_string()))
    }

    /// Adds a grant for the agent named `agent` (its identity is derived here). The
    /// only caller is the server, on an in-app "always allow" answer.
    ///
    /// # Errors
    /// When a remembered grant cannot be saved (it still applies for this session).
    pub(crate) fn add(
        &self,
        agent: &str,
        scope: GrantScope,
        remember: bool,
        now: u64,
    ) -> Result<Grant, GrantFileError> {
        let grant = Grant { agent: crate::limits::identity_of(agent).to_string(), scope, remember, created: now };
        if !valid(&grant) {
            return Err(GrantFileError("that grant is not allowed".into()));
        }
        let mut g = self.lock();
        if let Some(old) = g.grants.iter_mut().find(|y| y.agent == grant.agent && y.scope == grant.scope) {
            old.remember |= grant.remember;
        } else {
            if g.grants.len() >= MAX_GRANTS {
                return Err(GrantFileError(format!("at most {MAX_GRANTS} agent grants can exist")));
            }
            g.grants.push(grant.clone());
        }
        if remember { Self::save(&g).map(|()| grant) } else { Ok(grant) }
    }

    /// The grant that approves `command` for the agent named `agent`, if any (none while
    /// suspended).
    #[must_use]
    /// `main_edit`: the approval is for changing the main branch, which only a grant
    /// for exactly that command may cover (security re-review 3).
    pub fn covering(&self, agent: &str, command: &str, main_edit: bool) -> Option<Grant> {
        let g = self.lock();
        if g.suspended {
            return None;
        }
        let id = crate::limits::identity_of(agent);
        // A single-command grant is the more specific reason; prefer it in the audit.
        g.grants
            .iter()
            .find(|x| matches!(x.scope, GrantScope::Command(_)) && x.covers(id, command))
            .or_else(|| if main_edit { None } else { g.grants.iter().find(|x| x.covers(id, command)) })
            .cloned()
    }

    /// Every grant, for the Agents panel.
    #[must_use]
    pub fn list(&self) -> Vec<Grant> {
        self.lock().grants.clone()
    }

    /// Whether grants are suspended (kill switch).
    #[must_use]
    pub fn suspended(&self) -> bool {
        self.lock().suspended
    }

    /// Removes one grant.
    ///
    /// # Errors
    /// When the grant file cannot be saved (the grant is gone from memory regardless).
    pub fn revoke(&self, agent: &str, scope: &GrantScope) -> Result<(), GrantFileError> {
        let mut g = self.lock();
        g.grants.retain(|x| !(x.agent == agent && &x.scope == scope));
        Self::save(&g)
    }

    /// Removes every grant.
    ///
    /// # Errors
    /// When the grant file cannot be saved.
    pub fn revoke_all(&self) -> Result<(), GrantFileError> {
        let mut g = self.lock();
        g.grants.clear();
        Self::save(&g)
    }

    /// Suspends (kill switch) or resumes every grant; persisted.
    ///
    /// # Errors
    /// When the grant file cannot be saved (the state applies in memory regardless).
    pub fn set_suspended(&self, suspended: bool) -> Result<(), GrantFileError> {
        let mut g = self.lock();
        g.suspended = suspended;
        Self::save(&g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gusset-grants-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("agent-grants.json")
    }

    #[test]
    fn grants_bind_identity_and_scope() {
        let g = Grants::default();
        g.add("claude-code-3", GrantScope::Command("branch.set_main".into()), false, 1).unwrap();
        // Another connection of the same client is the same identity.
        assert!(g.covering("claude-code-7", "branch.set_main", false).is_some());
        assert!(g.covering("claude-code-7", "branch.rename", false).is_none(), "other action");
        assert!(g.covering("cursor-1", "branch.set_main", false).is_none(), "other agent");
        assert!(g.covering("claude-code-x-1", "branch.set_main", false).is_none(), "prefix is not the identity");
    }

    #[test]
    fn blanket_grant_excludes_destructive_and_file_writes() {
        let g = Grants::default();
        g.add("claude-code-1", GrantScope::All, false, 1).unwrap();
        assert!(g.covering("claude-code-2", "document.set_text", false).is_some());
        // Never by "all actions": set main, and any edit of the main branch.
        assert!(g.covering("claude-code-2", "branch.set_main", false).is_none(), "set main");
        assert!(g.covering("claude-code-2", "document.set_text", true).is_none(), "main-branch edit");
        assert!(g.covering("claude-code-2", "branch.delete", false).is_none(), "destructive");
        let export = gusset_commands::registry().iter().find(|s| s.flags.writes_files).unwrap();
        assert!(g.covering("claude-code-2", export.id, false).is_none(), "file write");
        assert!(g.covering("claude-code-2", "no.such.command", false).is_none());
        // ...but a grant for exactly that action is honored.
        g.add("claude-code-1", GrantScope::Command("branch.delete".into()), false, 1).unwrap();
        assert!(g.covering("claude-code-2", "branch.delete", false).is_some());
    }

    #[test]
    fn clients_named_with_digits_keep_grants() {
        // Security re-review 5: connection "gpt-5-1" has identity "gpt-5".
        let g = Grants::default();
        g.add("gpt-5-1", GrantScope::Command("branch.set_main".into()), false, 1).unwrap();
        assert!(g.covering("gpt-5-2", "branch.set_main", false).is_some());
        assert!(g.covering("gpt-1", "branch.set_main", false).is_none());
    }

    #[test]
    fn invalid_grants_are_refused() {
        let g = Grants::default();
        assert!(g.add("claude-code-1", GrantScope::Command("no.such.command".into()), false, 1).is_err());
        let user_only = gusset_commands::registry().iter().find(|s| s.flags.user_only);
        if let Some(s) = user_only {
            assert!(g.add("claude-code-1", GrantScope::Command(s.id.into()), false, 1).is_err());
        }
        assert!(g.list().is_empty());
    }

    #[test]
    fn suspension_blocks_everything_and_persists() {
        let p = tmp("suspend");
        let g = Grants::default();
        g.load(&p).unwrap();
        g.add("claude-code-1", GrantScope::All, true, 1).unwrap();
        g.set_suspended(true).unwrap();
        assert!(g.covering("claude-code-1", "document.set_text", false).is_none());
        let g2 = Grants::default();
        assert_eq!(g2.load(&p).unwrap(), 1);
        assert!(g2.suspended() && g2.covering("claude-code-1", "document.set_text", false).is_none());
        g2.set_suspended(false).unwrap();
        assert!(g2.covering("claude-code-1", "document.set_text", false).is_some());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn only_remembered_grants_survive_a_restart() {
        let p = tmp("restart");
        let g = Grants::default();
        g.load(&p).unwrap();
        g.add("claude-code-1", GrantScope::Command("branch.set_main".into()), true, 1).unwrap();
        g.add("claude-code-1", GrantScope::All, false, 1).unwrap();
        let g2 = Grants::default();
        assert_eq!(g2.load(&p).unwrap(), 1);
        assert_eq!(g2.list().len(), 1);
        g2.revoke("claude-code", &GrantScope::Command("branch.set_main".into())).unwrap();
        let g3 = Grants::default();
        assert_eq!(g3.load(&p).unwrap(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o077, 0, "owner-only");
        }
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn damaged_or_forged_files_grant_nothing_dangerous() {
        let p = tmp("forged");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"{not json").unwrap();
        let g = Grants::default();
        assert!(g.load(&p).is_err());
        assert!(g.list().is_empty());
        // Entries with an unsanitized agent, an unknown or user-only command, or not
        // remembered are dropped.
        let forged = serde_json::json!({"version": 1, "suspended": false, "grants": [
            {"agent": "Claude Code", "scope": "all", "remember": true, "created": 0},
            {"agent": "claude-code-1", "scope": "all", "remember": true, "created": 0},
            {"agent": "x", "scope": {"command": "no.such"}, "remember": true, "created": 0},
            {"agent": "y", "scope": "all", "remember": false, "created": 0},
            {"agent": "ok", "scope": "all", "remember": true, "created": 0}
        ]});
        std::fs::write(&p, forged.to_string()).unwrap();
        // "claude-code-1" is a valid identity (a client may be named that way).
        assert_eq!(g.load(&p).unwrap(), 2);
        assert!(g.list().iter().any(|x| x.agent == "ok"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
