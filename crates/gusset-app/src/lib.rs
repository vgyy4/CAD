//! The Gusset desktop application (ADR 0009, DESIGN.md).
// Display and input code: platform float functions are fine here (PROMPT.md Determinism
// covers model-defining code; edits reach the model through commands).
#![allow(clippy::disallowed_methods)]

pub mod agents;
pub mod app;
pub mod browser;
pub mod cmdline;
pub mod crash;
pub mod forms;
pub mod i18n;
pub mod icons;
pub mod link;
pub mod palette;
pub mod prefs;
pub mod quick_menus;
pub mod results_panel;
pub mod ribbon;
pub mod sheet;
pub mod sketch_constrain;
pub mod sketch_place;
pub mod sketch_regions;
pub mod spacemouse;
pub mod theme;
pub mod timeline;
pub mod value_entry;
pub mod viewport;
pub mod viewport_gpu;
pub mod viewport_nav;

#[cfg(test)]
mod screenshots;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_lifecycle;
#[cfg(test)]
mod tests_panels;
#[cfg(test)]
mod tests_ribbon;
