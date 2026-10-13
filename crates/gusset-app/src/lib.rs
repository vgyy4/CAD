//! The Gusset desktop application (ADR 0009, DESIGN.md).
// Display and input code: platform float functions are fine here (PROMPT.md Determinism
// covers model-defining code; edits reach the model through commands).
#![allow(clippy::disallowed_methods)]

pub mod agents;
pub mod app;
pub mod forms;
pub mod i18n;
pub mod icons;
pub mod link;
pub mod theme;
pub mod timeline;
pub mod viewport;
pub mod viewport_gpu;
pub mod viewport_nav;

#[cfg(test)]
mod screenshots;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_lifecycle;
