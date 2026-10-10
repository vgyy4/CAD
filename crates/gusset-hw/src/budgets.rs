//! Resource budgets scaled from the detected machine: thread-pool sizes, cache size,
//! default tessellation quality and speculative work.
//!
//! All functions are pure, so the budget table in `docs/HARDWARE.md` is generated from
//! them for a set of example machines.

use std::fmt::Write as _;

use crate::cpu::{CoreTopology, CpuInfo};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// Default display-tessellation quality (the user can override per document).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TessellationQuality {
    /// For <= 2 cores or < 6 GiB RAM.
    Coarse,
    /// Reference machine (4 cores, 8 GiB).
    Standard,
    /// >= 8 logical CPUs and >= 16 GiB.
    Fine,
    /// >= 16 logical CPUs and >= 32 GiB.
    ExtraFine,
}

impl TessellationQuality {
    /// Lower-case name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Coarse => "coarse",
            Self::Standard => "standard",
            Self::Fine => "fine",
            Self::ExtraFine => "extra-fine",
        }
    }
}

/// Budgets for one machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budgets {
    /// Worker threads for interactive work (rebuilds, tessellation the user waits for).
    pub interactive_threads: usize,
    /// Worker threads for low-priority background work.
    pub background_threads: usize,
    /// Byte budget for the tessellation/rebuild caches (CPU side).
    pub cache_bytes: u64,
    /// Default display tessellation quality.
    pub tessellation: TessellationQuality,
    /// Whether speculative precomputation (e.g. pre-tessellating hidden bodies) is on.
    pub speculative_precompute: bool,
}

/// RAM assumed when it cannot be detected (the reference machine has 8 GiB, shared with
/// the GPU; assume less to stay safe).
pub const ASSUMED_RAM_BYTES: u64 = 4 * GIB;

/// Interactive pool: all logical CPUs but one, which stays free for the UI thread.
#[must_use]
pub fn interactive_threads(logical: usize) -> usize {
    logical.saturating_sub(1).max(1)
}

/// Background pool: on hybrid CPUs, the number of efficiency logical CPUs (that is where
/// low-QoS threads should land); otherwise half of the interactive pool. Never more
/// than the interactive pool, at least 1.
#[must_use]
pub fn background_threads(logical: usize, topology: CoreTopology) -> usize {
    let interactive = interactive_threads(logical);
    let n = match topology {
        CoreTopology::Hybrid { efficiency, .. } if efficiency > 0 => efficiency,
        _ => interactive / 2,
    };
    n.clamp(1, interactive)
}

/// Cache budget: 1/16 of RAM (GPU memory is system RAM on the reference machine, so
/// stay modest), clamped to [128 MiB, 4 GiB].
#[must_use]
pub fn cache_bytes(total_ram: Option<u64>) -> u64 {
    (total_ram.unwrap_or(ASSUMED_RAM_BYTES) / 16).clamp(128 * MIB, 4 * GIB)
}

/// Default tessellation quality from cores and RAM.
#[must_use]
pub fn tessellation_quality(logical: usize, total_ram: Option<u64>) -> TessellationQuality {
    let ram = total_ram.unwrap_or(ASSUMED_RAM_BYTES);
    if logical <= 2 || ram < 6 * GIB {
        TessellationQuality::Coarse
    } else if logical >= 16 && ram >= 32 * GIB {
        TessellationQuality::ExtraFine
    } else if logical >= 8 && ram >= 16 * GIB {
        TessellationQuality::Fine
    } else {
        TessellationQuality::Standard
    }
}

/// Speculative precomputation needs spare cores and RAM.
#[must_use]
pub fn speculative_precompute(logical: usize, total_ram: Option<u64>) -> bool {
    logical >= 8 && total_ram.unwrap_or(0) >= 16 * GIB
}

impl Budgets {
    /// Budgets from raw machine facts.
    #[must_use]
    pub fn compute(logical: usize, topology: CoreTopology, total_ram: Option<u64>) -> Self {
        let logical = logical.max(1);
        Self {
            interactive_threads: interactive_threads(logical),
            background_threads: background_threads(logical, topology),
            cache_bytes: cache_bytes(total_ram),
            tessellation: tessellation_quality(logical, total_ram),
            speculative_precompute: speculative_precompute(logical, total_ram),
        }
    }

    /// Budgets for a detected machine.
    #[must_use]
    pub fn for_cpu(cpu: &CpuInfo) -> Self {
        Self::compute(cpu.logical_cores, cpu.topology, cpu.total_ram_bytes)
    }

    /// Deterministic report lines.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "budget.interactive_threads: {}", self.interactive_threads);
        let _ = writeln!(s, "budget.background_threads: {}", self.background_threads);
        let _ = writeln!(s, "budget.cache: {} MiB", self.cache_bytes / MIB);
        let _ = writeln!(s, "budget.tessellation: {}", self.tessellation.name());
        let _ = writeln!(s, "budget.speculative_precompute: {}", self.speculative_precompute);
        s
    }
}

/// Example machines for the generated budget table in `docs/HARDWARE.md`.
/// (name, logical CPUs, topology, RAM GiB)
pub const EXAMPLE_MACHINES: &[(&str, usize, CoreTopology, u64)] = &[
    ("Raspberry Pi 4-class: 4 cores, 4 GB", 4, CoreTopology::Homogeneous, 4),
    ("Reference machine: 4 cores, 8 GB", 4, CoreTopology::Homogeneous, 8),
    ("4C/8T desktop (e.g. Core i7-6700), 8 GB", 8, CoreTopology::Homogeneous, 8),
    ("Core i7-13700 (8P+8E, 24 threads), 32 GB", 24, CoreTopology::Hybrid { performance: 16, efficiency: 8 }, 32),
    ("Hybrid laptop: 4P + 4E cores, 16 GB", 8, CoreTopology::Hybrid { performance: 4, efficiency: 4 }, 16),
    ("Workstation: 16 cores / 32 threads, 64 GB", 32, CoreTopology::Homogeneous, 64),
];

/// GiB helper for [`EXAMPLE_MACHINES`].
#[must_use]
pub const fn gib(n: u64) -> u64 {
    n * GIB
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_headroom() {
        assert_eq!(interactive_threads(0), 1);
        assert_eq!(interactive_threads(1), 1);
        assert_eq!(interactive_threads(4), 3);
        assert_eq!(background_threads(1, CoreTopology::Unknown), 1);
        assert_eq!(background_threads(4, CoreTopology::Homogeneous), 1);
        assert_eq!(background_threads(16, CoreTopology::Homogeneous), 7);
        assert_eq!(background_threads(24, CoreTopology::Hybrid { performance: 16, efficiency: 8 }), 8);
        // Never more than the interactive pool.
        assert_eq!(background_threads(2, CoreTopology::Hybrid { performance: 0, efficiency: 8 }), 1);
    }

    #[test]
    fn cache_scales_with_ram_and_is_clamped() {
        assert_eq!(cache_bytes(Some(gib(8))), 512 * MIB);
        assert_eq!(cache_bytes(Some(gib(1))), 128 * MIB);
        assert_eq!(cache_bytes(Some(gib(256))), 4 * GIB);
        assert_eq!(cache_bytes(None), 256 * MIB);
    }

    #[test]
    fn tessellation_scales_with_machine() {
        assert_eq!(tessellation_quality(2, Some(gib(16))), TessellationQuality::Coarse);
        assert_eq!(tessellation_quality(4, Some(gib(4))), TessellationQuality::Coarse);
        assert_eq!(tessellation_quality(4, Some(gib(8))), TessellationQuality::Standard);
        assert_eq!(tessellation_quality(8, Some(gib(16))), TessellationQuality::Fine);
        assert_eq!(tessellation_quality(32, Some(gib(64))), TessellationQuality::ExtraFine);
        assert_eq!(tessellation_quality(8, None), TessellationQuality::Coarse);
    }

    #[test]
    fn stronger_machines_never_get_smaller_budgets() {
        let mut prev: Option<Budgets> = None;
        for (logical, ram) in [(1, 2), (2, 4), (4, 8), (8, 16), (16, 32), (64, 256)] {
            let b = Budgets::compute(logical, CoreTopology::Homogeneous, Some(gib(ram)));
            if let Some(p) = prev {
                assert!(b.interactive_threads >= p.interactive_threads);
                assert!(b.background_threads >= p.background_threads);
                assert!(b.cache_bytes >= p.cache_bytes);
                assert!(b.tessellation >= p.tessellation);
                assert!(b.speculative_precompute >= p.speculative_precompute);
            }
            prev = Some(b);
        }
    }
}
