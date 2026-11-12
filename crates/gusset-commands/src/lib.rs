//! Gusset sessions and the single command registry (ADR 0012).
//!
//! Every user-visible action is a [`Command`]; the GUI, the CLI and the MCP server all
//! call [`execute`], which enforces agent policy and records the audit log.

pub mod commands;
pub mod error;
pub mod fcstd;
mod fcstd_tree;
pub mod gear;
#[doc(hidden)]
pub mod golden;
pub mod hole_standards;
pub mod import;
pub mod paths;
pub mod pcb_session;
pub mod registry;
pub mod scad_build;
mod scad_text;
pub mod sch_session;
pub mod session;
pub mod tree_import;

pub use error::CommandError;
pub use registry::{Category, Command, CommandSpec, Flags, execute, find, preview, registry};
pub use session::{AuditEntry, CmdCtx, CommitOutcome, Session, SessionConfig};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_bundle;
#[cfg(test)]
mod tests_cam;
#[cfg(test)]
mod tests_component;
#[cfg(test)]
mod tests_datum;
#[cfg(test)]
mod tests_drawing;
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
mod tests_pcb;
#[cfg(test)]
mod tests_pcb_edit;
#[cfg(test)]
mod tests_pcb_layout;
#[cfg(test)]
mod tests_sch;
#[cfg(test)]
mod tests_sheet;
#[cfg(test)]
mod tests_sketch;
#[cfg(test)]
mod tests_sweep;
#[cfg(test)]
mod tests_tree;
