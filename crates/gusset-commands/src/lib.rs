//! Gusset sessions and the single command registry (ADR 0012).
//!
//! Every user-visible action is a [`Command`]; the GUI, the CLI and the MCP server all
//! call [`execute`], which enforces agent policy and records the audit log.

pub mod commands;
pub mod error;
#[doc(hidden)]
pub mod golden;
pub mod import;
pub mod paths;
pub mod registry;
pub mod session;

pub use error::CommandError;
pub use registry::{Category, Command, CommandSpec, Flags, execute, find, registry};
pub use session::{AuditEntry, CmdCtx, CommitOutcome, Session, SessionConfig};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_ops;
