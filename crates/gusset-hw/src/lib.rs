//! Hardware detection and progressive enhancement for Gusset.
//!
//! * [`registry`]: the single source of truth for CPU features, dispatch levels and GPU
//!   quality tiers. `docs/HARDWARE.md` is partly generated from it ([`docgen`]).
//! * [`cpu`]: ISA features, core counts/topology, RAM and brand detection.
//! * [`dispatch`]: the runtime dispatch level (with a CLI / debug-env override).
//! * [`kernels`]: display-only dispatched kernels (scalar, SSE4.2, AVX2+FMA, AVX-512,
//!   NEON, SVE).
//! * [`budgets`]: thread-pool sizes, cache budget, default tessellation quality.
//! * [`priority`]: low-priority / efficiency-QoS background threads.
//! * [`stdio`]: keeps native libraries from writing into a stdout protocol stream.
//! * [`tier`]: automatic, user-overridable GPU quality tier.
//! * [`Diagnostics`]: the About panel / MCP diagnostics report.
//!
//! See ADR 0011 (`docs/adr/0011-hardware-tiers-and-dispatch.md`) for the policy,
//! notably: **model-defining code never uses a dispatched or FMA path**.
//!
//! ```
//! use gusset_hw::{Diagnostics, dispatch, kernels};
//! let level = dispatch::active().level;
//! let mut pts = vec![[1.0_f32, 2.0, 3.0]];
//! let ident = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
//! kernels::transform_points_f32(level, &ident, &mut pts);
//! assert!(Diagnostics::collect().to_text().contains("cpu.logical_cores"));
//! ```

#![deny(unsafe_code)]

pub mod budgets;
pub mod cpu;
pub mod diagnostics;
pub mod dispatch;
pub mod docgen;
pub mod kernels;
pub mod priority;
pub mod registry;
pub mod stdio;
pub mod tier;

mod sys;

pub use budgets::{Budgets, TessellationQuality};
pub use cpu::{CoreTopology, CpuFeatures, CpuInfo};
pub use diagnostics::{Diagnostics, GpuDiagnostics};
pub use dispatch::{ActiveDispatch, Dispatch, DispatchError};
pub use priority::{BackgroundClass, PriorityOutcome, lower_current_thread_priority, spawn_background};
pub use registry::{Determinism, GpuClass};
pub use tier::QualityTier;
