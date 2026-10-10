//! Runtime CPU dispatch level: chosen once at startup, optionally forced.
//!
//! The level only selects kernels for **display-only** work (see
//! [`crate::registry::Determinism`]). Model-defining code never consults it.
//!
//! # Forcing a level
//! * The app passes the value of its `--force-dispatch <level>` CLI flag to
//!   [`choose`]. This works in every build.
//! * [`env_override`] reads `GUSSET_FORCE_DISPATCH` **only in debug/test builds**
//!   (`debug_assertions`). Release builds never consult the environment, per the spec's
//!   rule that release builds must not take behavior from environment variables.
//!   The CI determinism job runs the golden suite once per supported level this way.

use std::fmt;
use std::sync::OnceLock;

use crate::cpu::CpuFeatures;
use crate::registry::{Arch, DISPATCH_PATHS, DispatchPath, dispatch_path};

/// Environment variable read by [`env_override`] in debug/test builds.
pub const FORCE_DISPATCH_ENV: &str = "GUSSET_FORCE_DISPATCH";

/// A runtime-dispatch level. The required CPU features of each level live in
/// [`crate::registry::DISPATCH_PATHS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dispatch {
    /// Portable reference code; the only level model-defining code may use.
    Scalar,
    /// x86-64-v2 baseline (SSE4.2, POPCNT).
    Sse42,
    /// AVX2 + FMA3 (x86-64-v3 class).
    Avx2Fma,
    /// AVX-512 F/BW/VL.
    Avx512,
    /// ARMv8.0-A Advanced SIMD baseline.
    Neon,
    /// NEON + dot product (ARMv8.2-A+dotprod, mandatory from ARMv8.4-A).
    NeonDotprod,
    /// Scalable Vector Extension.
    Sve,
}

impl Dispatch {
    /// Every level, in registry order.
    pub const ALL: &'static [Self] =
        &[Self::Scalar, Self::Sse42, Self::Avx2Fma, Self::Avx512, Self::Neon, Self::NeonDotprod, Self::Sve];

    /// Lower-case name used by the CLI flag, the env var and reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Sse42 => "sse42",
            Self::Avx2Fma => "avx2fma",
            Self::Avx512 => "avx512",
            Self::Neon => "neon",
            Self::NeonDotprod => "neon-dotprod",
            Self::Sve => "sve",
        }
    }

    /// Parse a name as produced by [`Dispatch::name`] (case-insensitive).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        Self::ALL.iter().copied().find(|d| d.name().eq_ignore_ascii_case(s))
    }

    /// Architecture this level belongs to (`None` = portable).
    #[must_use]
    pub const fn arch(self) -> Option<Arch> {
        match self {
            Self::Scalar => None,
            Self::Sse42 | Self::Avx2Fma | Self::Avx512 => Some(Arch::X86_64),
            Self::Neon | Self::NeonDotprod | Self::Sve => Some(Arch::Aarch64),
        }
    }

    /// The registry row.
    #[must_use]
    pub fn path(self) -> &'static DispatchPath {
        dispatch_path(self)
    }

    /// Whether a CPU with `features` can run this level on the running architecture.
    #[must_use]
    pub fn is_supported_by(self, features: &CpuFeatures) -> bool {
        let arch_ok = match self.arch() {
            None => true,
            Some(a) => Arch::current() == Some(a),
        };
        arch_ok && self.path().required_features.iter().all(|f| features.has(f))
    }
}

impl fmt::Display for Dispatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// All levels a CPU supports, least capable first (always starts with `Scalar`).
#[must_use]
pub fn supported_levels(features: &CpuFeatures) -> Vec<Dispatch> {
    DISPATCH_PATHS.iter().map(|p| p.level).filter(|l| l.is_supported_by(features)).collect()
}

/// The most capable supported level.
#[must_use]
pub fn best_supported(features: &CpuFeatures) -> Dispatch {
    supported_levels(features).last().copied().unwrap_or(Dispatch::Scalar)
}

/// Why a forced dispatch level was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    /// Not a known level name.
    Unknown(String),
    /// Known, but this CPU/architecture cannot run it.
    Unsupported(Dispatch),
}

impl fmt::Display for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(s) => {
                let names: Vec<&str> = Dispatch::ALL.iter().map(|d| d.name()).collect();
                write!(f, "unknown dispatch level {s:?}; expected one of: {}", names.join(", "))
            }
            Self::Unsupported(d) => write!(f, "dispatch level {d} is not supported by this CPU"),
        }
    }
}

impl std::error::Error for DispatchError {}

/// Choose the dispatch level: the forced one if given (it must be supported), else the
/// best supported.
///
/// # Errors
/// [`DispatchError`] if `forced` is unknown or unsupported. Callers should report the
/// error and exit rather than silently fall back, so a CI run never tests the wrong path.
pub fn choose(features: &CpuFeatures, forced: Option<&str>) -> Result<Dispatch, DispatchError> {
    match forced {
        None => Ok(best_supported(features)),
        Some(s) => {
            let level = Dispatch::parse(s).ok_or_else(|| DispatchError::Unknown(s.to_owned()))?;
            if level.is_supported_by(features) { Ok(level) } else { Err(DispatchError::Unsupported(level)) }
        }
    }
}

/// `GUSSET_FORCE_DISPATCH`, honored only in builds with `debug_assertions` (debug and
/// test profiles). Always `None` in release builds.
#[must_use]
pub fn env_override() -> Option<String> {
    if cfg!(debug_assertions) { std::env::var(FORCE_DISPATCH_ENV).ok().filter(|s| !s.trim().is_empty()) } else { None }
}

/// The process-wide level, plus whether it was forced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveDispatch {
    /// The level in use.
    pub level: Dispatch,
    /// `true` if it came from the CLI flag or the debug env var.
    pub forced: bool,
}

static ACTIVE: OnceLock<ActiveDispatch> = OnceLock::new();

/// Set the process-wide level once at startup. Returns the level that is in effect,
/// which is the earlier one if `init` (or [`active`]) already ran.
pub fn init(choice: ActiveDispatch) -> ActiveDispatch {
    *ACTIVE.get_or_init(|| choice)
}

/// The process-wide level. If [`init`] was never called, detects the CPU and uses the
/// best supported level (honoring [`env_override`] in debug builds when it is valid).
pub fn active() -> ActiveDispatch {
    *ACTIVE.get_or_init(|| {
        let features = CpuFeatures::detect();
        match env_override().map(|s| choose(&features, Some(&s))) {
            Some(Ok(level)) => ActiveDispatch { level, forced: true },
            _ => ActiveDispatch { level: best_supported(&features), forced: false },
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for d in Dispatch::ALL {
            assert_eq!(Dispatch::parse(d.name()), Some(*d));
        }
        assert_eq!(Dispatch::parse("AVX2FMA"), Some(Dispatch::Avx2Fma));
        assert_eq!(Dispatch::parse("avx3"), None);
    }

    #[test]
    fn scalar_always_supported_and_first() {
        let none = CpuFeatures::from_names(&[]);
        assert_eq!(supported_levels(&none), vec![Dispatch::Scalar]);
        assert_eq!(best_supported(&none), Dispatch::Scalar);
        assert_eq!(choose(&none, Some("scalar")), Ok(Dispatch::Scalar));
    }

    #[test]
    fn choose_rejects_unknown_and_unsupported() {
        let none = CpuFeatures::from_names(&[]);
        assert!(matches!(choose(&none, Some("bogus")), Err(DispatchError::Unknown(_))));
        assert_eq!(choose(&none, Some("avx512")), Err(DispatchError::Unsupported(Dispatch::Avx512)));
        assert_eq!(choose(&none, Some("sve")), Err(DispatchError::Unsupported(Dispatch::Sve)));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn x86_levels_from_feature_sets() {
        // Haswell-class desktop: AVX2 + FMA, no AVX-512.
        let hsw = CpuFeatures::from_names(&["sse4.2", "popcnt", "avx", "avx2", "fma"]);
        assert_eq!(best_supported(&hsw), Dispatch::Avx2Fma);
        // Raptor Lake: AVX-VNNI but no AVX-512 -> still AVX2.
        let rpl = CpuFeatures::from_names(&["sse4.2", "popcnt", "avx", "avx2", "fma", "avxvnni"]);
        assert_eq!(best_supported(&rpl), Dispatch::Avx2Fma);
        // AVX-512F alone is not enough; BW and VL are required too.
        let partial = CpuFeatures::from_names(&["sse4.2", "popcnt", "avx", "avx2", "fma", "avx512f"]);
        assert_eq!(best_supported(&partial), Dispatch::Avx2Fma);
        let zen4 =
            CpuFeatures::from_names(&["sse4.2", "popcnt", "avx", "avx2", "fma", "avx512f", "avx512bw", "avx512vl"]);
        assert_eq!(best_supported(&zen4), Dispatch::Avx512);
        // Sandy Bridge: AVX without AVX2 -> SSE4.2.
        let snb = CpuFeatures::from_names(&["sse4.2", "popcnt", "avx"]);
        assert_eq!(best_supported(&snb), Dispatch::Sse42);
        // ARM levels never apply on x86.
        assert!(!Dispatch::Neon.is_supported_by(&CpuFeatures::from_names(&["neon"])));
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn arm_levels_from_feature_sets() {
        let pi4 = CpuFeatures::from_names(&["neon"]);
        assert_eq!(best_supported(&pi4), Dispatch::Neon);
        let pi5 = CpuFeatures::from_names(&["neon", "dotprod"]);
        assert_eq!(best_supported(&pi5), Dispatch::NeonDotprod);
        let g3 = CpuFeatures::from_names(&["neon", "dotprod", "i8mm", "sve"]);
        assert_eq!(best_supported(&g3), Dispatch::Sve);
    }

    #[test]
    fn detected_best_is_supported() {
        let f = CpuFeatures::detect();
        let best = best_supported(&f);
        assert!(best.is_supported_by(&f));
        assert!(supported_levels(&f).contains(&Dispatch::Scalar));
    }
}
