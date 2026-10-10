//! x86-64 CPUID.
#![allow(unsafe_code)]

use std::arch::x86_64::{__cpuid, CpuidResult};

/// Execute CPUID for `leaf` (sub-leaf 0).
// `__cpuid` is a safe fn in recent std but is still `unsafe` in Rust 1.89 (our MSRV),
// so the block is required there and "unused" on newer compilers.
#[allow(unused_unsafe)]
pub(crate) fn cpuid(leaf: u32) -> CpuidResult {
    // SAFETY: the CPUID instruction exists on every x86_64 CPU and only reads
    // identification registers; it has no memory-safety preconditions.
    unsafe { __cpuid(leaf) }
}
