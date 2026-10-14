//! Gusset sessions and the single command registry (ADR 0012).
//!
//! Every user-visible action is a [`Command`]; the GUI, the CLI and the MCP server all
//! call [`execute`], which enforces agent policy and records the audit log.

pub mod commands;
pub mod error;
pub mod gear;
#[doc(hidden)]
pub mod golden;
pub mod hole_standards;
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
mod tests_datum;
#[cfg(test)]
mod tests_gear;
#[cfg(test)]
mod tests_gear_math;
#[cfg(test)]
mod tests_hole;
#[cfg(test)]
mod tests_joint;
#[cfg(test)]
mod tests_ops;
#[cfg(test)]
mod tests_sketch;
#[cfg(test)]
mod tests_sweep;
