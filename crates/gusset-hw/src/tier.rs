//! GPU quality tiers. The tier contents live in [`crate::registry::GPU_TIERS`].

use crate::registry::{GPU_TIERS, GpuClass, TierSpec, tier_spec};

/// Automatic, user-overridable rendering quality tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum QualityTier {
    /// Integrated GPUs and software renderers.
    Low,
    /// Between the two; currently manual only.
    Medium,
    /// Dedicated GPUs.
    High,
}

impl QualityTier {
    /// All tiers, lowest first.
    pub const ALL: &'static [Self] = &[Self::Low, Self::Medium, Self::High];

    /// Lower-case name used in settings, the CLI and reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Parse a lower-case name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.name().eq_ignore_ascii_case(s.trim()))
    }

    /// What this tier enables.
    #[must_use]
    pub fn spec(self) -> &'static TierSpec {
        tier_spec(self)
    }
}

/// The tier chosen automatically for a GPU class (from the registry's `auto_for`).
#[must_use]
pub fn auto_tier(class: GpuClass) -> QualityTier {
    GPU_TIERS.iter().find(|t| t.auto_for.contains(&class)).map_or(QualityTier::Low, |t| t.tier)
}

/// Final tier: the user's choice wins over the automatic one.
#[must_use]
pub fn select_tier(class: GpuClass, user_override: Option<QualityTier>) -> QualityTier {
    user_override.unwrap_or_else(|| auto_tier(class))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults() {
        assert_eq!(auto_tier(GpuClass::Software), QualityTier::Low);
        assert_eq!(auto_tier(GpuClass::Integrated), QualityTier::Low);
        assert_eq!(auto_tier(GpuClass::Discrete), QualityTier::High);
        assert_eq!(select_tier(GpuClass::Software, Some(QualityTier::High)), QualityTier::High);
    }

    #[test]
    fn parse_round_trips() {
        for t in QualityTier::ALL {
            assert_eq!(QualityTier::parse(t.name()), Some(*t));
        }
        assert_eq!(QualityTier::parse(" HIGH "), Some(QualityTier::High));
        assert_eq!(QualityTier::parse("ultra"), None);
    }
}
