//! The feature/tier registry: the single source of truth for every CPU feature the app
//! detects, every dispatch path it can take, and every GPU quality tier.
//!
//! The dispatcher ([`crate::dispatch`]), the detector ([`crate::cpu`]), the tier
//! selection ([`crate::tier`]) and the `docs/HARDWARE.md` generator
//! ([`crate::docgen`]) all read these tables. Changing a row here changes the
//! documentation, and the `hardware_doc_is_up_to_date` test fails until the document
//! is regenerated with `cargo run -p gusset-hw --bin gen-hardware-doc`.

use crate::dispatch::Dispatch;
use crate::tier::QualityTier;

/// CPU architecture families the registry knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Arch {
    /// 64-bit x86 (`x86_64`).
    X86_64,
    /// 64-bit Arm (`aarch64`).
    Aarch64,
}

impl Arch {
    /// The architecture this binary was compiled for, if the registry covers it.
    #[must_use]
    pub const fn current() -> Option<Self> {
        if cfg!(target_arch = "x86_64") {
            Some(Self::X86_64)
        } else if cfg!(target_arch = "aarch64") {
            Some(Self::Aarch64)
        } else {
            None
        }
    }

    /// Rust `target_arch` spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }
}

/// A CPU feature that the app detects at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuFeature {
    /// Feature name exactly as `is_x86_feature_detected!` / `is_aarch64_feature_detected!`
    /// and `#[target_feature(enable = ...)]` spell it.
    pub name: &'static str,
    /// Architecture the feature belongs to.
    pub arch: Arch,
    /// Human-readable name for the About panel and the docs.
    pub display: &'static str,
    /// Whether the build baseline already guarantees this feature (x86-64-v2 on x64,
    /// ARMv8.0-A + NEON on ARM64), so it is always present on supported CPUs.
    pub in_baseline: bool,
}

/// Every CPU feature the app detects, in report order.
pub static CPU_FEATURES: &[CpuFeature] = &[
    CpuFeature { name: "sse4.2", arch: Arch::X86_64, display: "SSE4.2", in_baseline: true },
    CpuFeature { name: "popcnt", arch: Arch::X86_64, display: "POPCNT", in_baseline: true },
    CpuFeature { name: "avx", arch: Arch::X86_64, display: "AVX", in_baseline: false },
    CpuFeature { name: "avx2", arch: Arch::X86_64, display: "AVX2", in_baseline: false },
    CpuFeature { name: "fma", arch: Arch::X86_64, display: "FMA3", in_baseline: false },
    CpuFeature { name: "avx512f", arch: Arch::X86_64, display: "AVX-512F", in_baseline: false },
    CpuFeature { name: "avx512bw", arch: Arch::X86_64, display: "AVX-512BW", in_baseline: false },
    CpuFeature { name: "avx512vl", arch: Arch::X86_64, display: "AVX-512VL", in_baseline: false },
    CpuFeature { name: "avxvnni", arch: Arch::X86_64, display: "AVX-VNNI", in_baseline: false },
    CpuFeature { name: "neon", arch: Arch::Aarch64, display: "NEON (Advanced SIMD)", in_baseline: true },
    CpuFeature { name: "dotprod", arch: Arch::Aarch64, display: "DotProd", in_baseline: false },
    CpuFeature { name: "i8mm", arch: Arch::Aarch64, display: "I8MM", in_baseline: false },
    CpuFeature { name: "sve", arch: Arch::Aarch64, display: "SVE", in_baseline: false },
    CpuFeature { name: "sve2", arch: Arch::Aarch64, display: "SVE2", in_baseline: false },
];

/// Whether a code path may influence the model (and therefore must be bit-identical
/// on every CPU) or only what is shown on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Determinism {
    /// Only viewport meshes, rendering and picking. SIMD/FMA rounding differences are
    /// allowed (within the tolerance documented on the kernel).
    DisplayOnly,
    /// Defines geometry, topology, naming or saved files. Must never use a dispatched
    /// or FMA path; always the scalar reference.
    ModelDefining,
}

impl Determinism {
    /// Short label for reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::DisplayOnly => "display-only",
            Self::ModelDefining => "model-defining",
        }
    }
}

/// Measured benefit of a dispatch path. Filled in from the criterion benchmarks; a path
/// that never shows a measurable speedup is removed (PROMPT.md, "Runtime dispatch").
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Speedup {
    /// No benchmark result has been recorded yet.
    NotYetMeasured,
    /// The reference path itself.
    Reference,
    /// Speedup over the scalar reference.
    Measured {
        /// Criterion benchmark id.
        benchmark: &'static str,
        /// Test machine (CPU model, OS).
        machine: &'static str,
        /// Throughput ratio versus scalar (e.g. 2.4 = 2.4x faster).
        factor: f32,
    },
}

impl Speedup {
    /// Report text.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::NotYetMeasured => "not yet measured".to_owned(),
            Self::Reference => "reference (1.00x)".to_owned(),
            Self::Measured { benchmark, machine, factor } => {
                format!("{factor:.2}x on `{benchmark}` ({machine})")
            }
        }
    }
}

/// One runtime-dispatch level and everything the app does with it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DispatchPath {
    /// The dispatch level.
    pub level: Dispatch,
    /// Architecture, or `None` for the portable scalar path.
    pub arch: Option<Arch>,
    /// Every feature (by [`CpuFeature::name`]) the CPU must report for this level.
    pub required_features: &'static [&'static str],
    /// Code paths that have a kernel compiled for this level.
    pub code_paths: &'static [&'static str],
    /// Determinism status of all of those code paths.
    pub determinism: Determinism,
    /// Measured speedup.
    pub speedup: Speedup,
    /// Free-form notes for the docs.
    pub notes: &'static str,
}

/// Code paths that currently have dispatched kernels. Keep in sync with
/// `crate::kernels` (a unit test checks the names).
pub const KERNEL_TRANSFORM: &str = "kernels::transform_points_f32 (viewport mesh transform)";
/// See [`KERNEL_TRANSFORM`].
pub const KERNEL_BBOX: &str = "kernels::bbox_f32 (viewport bounds, picking pre-pass)";

const DEMO_KERNELS: &[&str] = &[KERNEL_TRANSFORM, KERNEL_BBOX];

/// Every dispatch level, ordered from least to most capable within each architecture.
/// [`crate::dispatch::best_supported`] picks the last supported entry for the running
/// architecture.
pub static DISPATCH_PATHS: &[DispatchPath] = &[
    DispatchPath {
        level: Dispatch::Scalar,
        arch: None,
        required_features: &[],
        code_paths: DEMO_KERNELS,
        determinism: Determinism::ModelDefining,
        speedup: Speedup::Reference,
        notes: "Reference path. No explicit SIMD and no FMA contraction; the only path model-defining code may use.",
    },
    DispatchPath {
        level: Dispatch::Sse42,
        arch: Some(Arch::X86_64),
        required_features: &["sse4.2", "popcnt"],
        code_paths: DEMO_KERNELS,
        determinism: Determinism::DisplayOnly,
        speedup: Speedup::NotYetMeasured,
        notes: "x86-64-v2 baseline. Same arithmetic as scalar (no FMA), autovectorized with SSE4.2.",
    },
    DispatchPath {
        level: Dispatch::Avx2Fma,
        arch: Some(Arch::X86_64),
        required_features: &["sse4.2", "popcnt", "avx", "avx2", "fma"],
        code_paths: DEMO_KERNELS,
        determinism: Determinism::DisplayOnly,
        speedup: Speedup::NotYetMeasured,
        notes: "x86-64-v3 class. Uses fused multiply-add, so results may differ from scalar in the last bits.",
    },
    DispatchPath {
        level: Dispatch::Avx512,
        arch: Some(Arch::X86_64),
        required_features: &["sse4.2", "popcnt", "avx", "avx2", "fma", "avx512f", "avx512bw", "avx512vl"],
        code_paths: DEMO_KERNELS,
        determinism: Determinism::DisplayOnly,
        speedup: Speedup::NotYetMeasured,
        notes: "Optional bonus path; never expected (absent on consumer Intel 12th-14th gen and Core Ultra).",
    },
    DispatchPath {
        level: Dispatch::Neon,
        arch: Some(Arch::Aarch64),
        required_features: &["neon"],
        code_paths: DEMO_KERNELS,
        determinism: Determinism::DisplayOnly,
        speedup: Speedup::NotYetMeasured,
        notes: "ARMv8.0-A baseline (Raspberry Pi 4 and up). Uses fused multiply-add.",
    },
    DispatchPath {
        level: Dispatch::NeonDotprod,
        arch: Some(Arch::Aarch64),
        required_features: &["neon", "dotprod"],
        code_paths: DEMO_KERNELS,
        determinism: Determinism::DisplayOnly,
        speedup: Speedup::NotYetMeasured,
        notes: "Float kernels reuse the NEON code; dotprod/i8mm are reserved for future int8 kernels.",
    },
    DispatchPath {
        level: Dispatch::Sve,
        arch: Some(Arch::Aarch64),
        required_features: &["neon", "sve"],
        code_paths: DEMO_KERNELS,
        determinism: Determinism::DisplayOnly,
        speedup: Speedup::NotYetMeasured,
        notes: "Mostly servers (Graviton3/4, Neoverse V/N2). LLVM autovectorizes to SVE; SVE intrinsics are nightly-only.",
    },
];

/// Look up the registry row for a dispatch level.
///
/// Every [`Dispatch`] variant has exactly one row (checked by a unit test); the
/// fallback to the scalar row only exists to keep this function total.
#[must_use]
pub fn dispatch_path(level: Dispatch) -> &'static DispatchPath {
    DISPATCH_PATHS.iter().find(|p| p.level == level).unwrap_or(&SCALAR_FALLBACK)
}

static SCALAR_FALLBACK: DispatchPath = DispatchPath {
    level: Dispatch::Scalar,
    arch: None,
    required_features: &[],
    code_paths: DEMO_KERNELS,
    determinism: Determinism::ModelDefining,
    speedup: Speedup::Reference,
    notes: "",
};

/// Classes of GPU adapters, mapped from wgpu's `DeviceType` by the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuClass {
    /// Software rasterizer (WARP, llvmpipe, lavapipe, SwiftShader).
    Software,
    /// Integrated GPU sharing system RAM.
    Integrated,
    /// Dedicated GPU with its own VRAM.
    Discrete,
    /// Virtualized/paravirtual GPU (VMs, remote desktop).
    Virtual,
    /// Driver did not say.
    Unknown,
}

impl GpuClass {
    /// Report label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Software => "software",
            Self::Integrated => "integrated",
            Self::Discrete => "discrete",
            Self::Virtual => "virtual",
            Self::Unknown => "unknown",
        }
    }
}

/// What one GPU quality tier enables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TierSpec {
    /// The tier.
    pub tier: QualityTier,
    /// GPU classes that get this tier automatically (the user can override).
    pub auto_for: &'static [GpuClass],
    /// MSAA sample count requested. wgpu guarantees 1 and 4; any other count is used only
    /// when the adapter reports support for it, otherwise the next lower supported count.
    pub msaa_samples: u32,
    /// Screen-space ambient occlusion.
    pub ssao: bool,
    /// Shadow mapping.
    pub shadows: bool,
    /// Multiplier on the default tessellation density for display meshes (1.0 = the
    /// budget's default quality).
    pub lod_detail_factor: f32,
    /// Additional coarsening while orbiting/panning/zooming (multiplied onto
    /// `lod_detail_factor`; refined again when the view is still).
    pub interactive_lod_factor: f32,
    /// Render scale while the view is moving (1.0 = native resolution).
    pub interactive_render_scale: f32,
    /// Free-form notes for the docs.
    pub notes: &'static str,
}

/// GPU quality tiers, from lowest to highest.
pub static GPU_TIERS: &[TierSpec] = &[
    TierSpec {
        tier: QualityTier::Low,
        auto_for: &[GpuClass::Software, GpuClass::Integrated, GpuClass::Virtual, GpuClass::Unknown],
        msaa_samples: 1,
        ssao: false,
        shadows: false,
        lod_detail_factor: 1.0,
        interactive_lod_factor: 0.5,
        interactive_render_scale: 0.75,
        notes: "Reference-machine tier (Intel HD 520-class iGPU, software renderers). Downlevel wgpu limits only.",
    },
    TierSpec {
        tier: QualityTier::Medium,
        auto_for: &[],
        msaa_samples: 4,
        ssao: false,
        shadows: false,
        lod_detail_factor: 1.5,
        interactive_lod_factor: 0.5,
        interactive_render_scale: 1.0,
        notes: "Manual choice for strong iGPUs until benchmarks justify auto-selection.",
    },
    TierSpec {
        tier: QualityTier::High,
        auto_for: &[GpuClass::Discrete],
        msaa_samples: 8,
        ssao: true,
        shadows: true,
        lod_detail_factor: 2.0,
        interactive_lod_factor: 0.75,
        interactive_render_scale: 1.0,
        notes: "Dedicated GPU. Falls back to 4x MSAA if the adapter does not support 8x for the target format.",
    },
];

/// Look up the registry row for a tier. Every tier has a row (unit-tested).
#[must_use]
pub fn tier_spec(tier: QualityTier) -> &'static TierSpec {
    GPU_TIERS.iter().find(|t| t.tier == tier).unwrap_or(&LOW_FALLBACK)
}

static LOW_FALLBACK: TierSpec = TierSpec {
    tier: QualityTier::Low,
    auto_for: &[],
    msaa_samples: 1,
    ssao: false,
    shadows: false,
    lod_detail_factor: 1.0,
    interactive_lod_factor: 0.5,
    interactive_render_scale: 0.75,
    notes: "",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_dispatch_level_has_exactly_one_row() {
        for level in Dispatch::ALL {
            let n = DISPATCH_PATHS.iter().filter(|p| p.level == *level).count();
            assert_eq!(n, 1, "{level:?}");
        }
        assert_eq!(DISPATCH_PATHS.len(), Dispatch::ALL.len());
    }

    #[test]
    fn required_features_are_registered_for_the_right_arch() {
        for p in DISPATCH_PATHS {
            for f in p.required_features {
                let feat = CPU_FEATURES.iter().find(|c| c.name == *f);
                let feat = feat.unwrap_or_else(|| panic!("{f} not in CPU_FEATURES"));
                assert_eq!(Some(feat.arch), p.arch, "{f} in {:?}", p.level);
            }
            assert_eq!(p.level.arch(), p.arch);
        }
    }

    #[test]
    fn only_scalar_is_model_defining() {
        for p in DISPATCH_PATHS {
            let model = p.determinism == Determinism::ModelDefining;
            assert_eq!(model, p.level == Dispatch::Scalar, "{:?}", p.level);
        }
    }

    #[test]
    fn every_tier_has_one_row_and_classes_are_covered_once() {
        for t in QualityTier::ALL {
            assert_eq!(GPU_TIERS.iter().filter(|s| s.tier == *t).count(), 1);
        }
        for class in
            [GpuClass::Software, GpuClass::Integrated, GpuClass::Discrete, GpuClass::Virtual, GpuClass::Unknown]
        {
            let n = GPU_TIERS.iter().filter(|s| s.auto_for.contains(&class)).count();
            assert_eq!(n, 1, "{class:?}");
        }
    }

    #[test]
    fn tiers_scale_up_monotonically() {
        for w in GPU_TIERS.windows(2) {
            let [a, b] = w else { continue };
            assert!(a.tier < b.tier);
            assert!(a.msaa_samples <= b.msaa_samples);
            assert!(a.lod_detail_factor <= b.lod_detail_factor);
            assert!(!a.ssao || b.ssao);
            assert!(!a.shadows || b.shadows);
        }
    }
}
