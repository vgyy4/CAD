//! Structured command errors (shared by GUI messages and MCP tool errors).

use gusset_history::HistoryError;
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandError {
    /// Stable machine-readable code: `invalid_args`, `not_found`, `invalid_document`,
    /// `approval_required`, `confirmation_required`, `read_only`, `rate_limited`,
    /// `not_allowed`, `history`, `evaluation`, `io`.
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Extra structured detail (e.g. validation issues).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl CommandError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        CommandError { code: code.into(), message: message.into(), hint: None, details: None }
    }

    pub fn hint(mut self, h: impl Into<String>) -> Self {
        self.hint = Some(h.into());
        self
    }

    pub fn details(mut self, d: serde_json::Value) -> Self {
        self.details = Some(d);
        self
    }

    pub fn args(msg: impl Into<String>) -> Self {
        CommandError::new("invalid_args", msg)
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        CommandError::new("not_found", msg)
    }

    pub fn history(e: HistoryError) -> Self {
        match e {
            HistoryError::ConfirmationRequired { reason } => CommandError::new("confirmation_required", reason)
                .hint("repeat the command with confirm: true after the user agrees"),
            HistoryError::UnknownBranch(_) | HistoryError::UnknownNode(_) => CommandError::not_found(e.to_string()),
            // Branch names are user/agent text: keep them out of `message` (agents see it
            // unlabelled) and return them only as `*_user_text` detail.
            HistoryError::BranchDeleted(name) => {
                CommandError::new("history", "that branch is deleted; restore it first")
                    .details(serde_json::json!({ "name_user_text": name }))
            }
            HistoryError::NameTaken(name) => CommandError::new("history", "a branch with that name already exists")
                .hint("pick another name; names are unique ignoring case")
                .details(serde_json::json!({ "name_user_text": name })),
            HistoryError::LimitReached(_) => CommandError::new("limit_reached", e.to_string())
                .hint("the change was not made; delete branches that are no longer needed or compact history"),
            other => CommandError::new("history", other.to_string()),
        }
    }

    pub fn invalid_document(issues: &[gusset_doc::Issue]) -> Self {
        let msgs: Vec<String> = issues.iter().map(ToString::to_string).collect();
        CommandError::new("invalid_document", msgs.first().cloned().unwrap_or_default())
            .details(serde_json::json!({ "issues": msgs }))
    }

    /// Approval for a change to the main branch: never covered by an agent's blanket
    /// "always allow all actions" grant (details `approval: main_branch`).
    pub fn approval_main() -> Self {
        Self::approval("editing the main branch").details(serde_json::json!({ "approval": "main_branch" }))
    }

    pub fn approval(action: &str) -> Self {
        CommandError::new("approval_required", format!("{action} needs the user's approval"))
            .hint("the request was sent to the user; do not try to work around it")
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(h) = &self.hint {
            write!(f, " ({h})")?;
        }
        Ok(())
    }
}

impl std::error::Error for CommandError {}
