//! Generates the registry-derived sections of `docs/HARDWARE.md`.
//!
//! The document contains marker pairs
//!
//! ```text
//! <!-- BEGIN GENERATED: dispatch-paths -->
//! ...generated content...
//! <!-- END GENERATED -->
//! ```
//!
//! [`update_document`] rewrites the content between each pair from the registry. The
//! integration test `hardware_doc_is_up_to_date` fails when the committed document
//! differs from the regenerated one, so CI catches drift. Regenerate with
//! `cargo run -p gusset-hw --bin gen-hardware-doc`.

use std::fmt::{self, Write as _};

use crate::budgets::{Budgets, EXAMPLE_MACHINES, gib};
use crate::cpu::CoreTopology;
use crate::dispatch::{Dispatch, FORCE_DISPATCH_ENV};
use crate::registry::{CPU_FEATURES, DISPATCH_PATHS, GPU_TIERS};

const BEGIN: &str = "<!-- BEGIN GENERATED: ";
const BEGIN_END: &str = " -->";
const END: &str = "<!-- END GENERATED -->";

/// Names of all generated sections, in document order.
pub const SECTIONS: &[&str] = &["cpu-features", "dispatch-paths", "gpu-tiers", "budgets"];

/// Generate the body of one section (without markers). `None` for unknown names.
#[must_use]
pub fn generate_section(name: &str) -> Option<String> {
    match name {
        "cpu-features" => Some(cpu_features()),
        "dispatch-paths" => Some(dispatch_paths()),
        "gpu-tiers" => Some(gpu_tiers()),
        "budgets" => Some(budgets()),
        _ => None,
    }
}

/// Problems with the marker structure of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocGenError {
    /// A required section has no BEGIN marker.
    MissingSection(String),
    /// A BEGIN marker names a section the generator does not know.
    UnknownSection(String),
    /// A BEGIN marker without a following END marker, or a malformed BEGIN marker.
    Unterminated(String),
    /// The same section appears twice.
    Duplicate(String),
}

impl fmt::Display for DocGenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSection(s) => write!(f, "generated section {s:?} is missing its markers"),
            Self::UnknownSection(s) => write!(f, "unknown generated section {s:?}"),
            Self::Unterminated(s) => write!(f, "generated section {s:?} has no END marker"),
            Self::Duplicate(s) => write!(f, "generated section {s:?} appears twice"),
        }
    }
}

impl std::error::Error for DocGenError {}

/// Rewrite every generated section of `doc`. Text outside the markers is unchanged.
///
/// # Errors
/// [`DocGenError`] if markers are missing, unknown, duplicated or unterminated.
pub fn update_document(doc: &str) -> Result<String, DocGenError> {
    let mut out = String::with_capacity(doc.len());
    let mut rest = doc;
    let mut seen: Vec<String> = Vec::new();
    while let Some(start) = rest.find(BEGIN) {
        let (before, from_marker) = rest.split_at(start);
        out.push_str(before);
        let after_begin = from_marker.get(BEGIN.len()..).unwrap_or_default();
        let Some(name_end) = after_begin.find(BEGIN_END) else {
            return Err(DocGenError::Unterminated(String::new()));
        };
        let name = after_begin.get(..name_end).unwrap_or_default().trim().to_owned();
        let body = generate_section(&name).ok_or_else(|| DocGenError::UnknownSection(name.clone()))?;
        if seen.contains(&name) {
            return Err(DocGenError::Duplicate(name));
        }
        let after_marker = after_begin.get(name_end + BEGIN_END.len()..).unwrap_or_default();
        let Some(end) = after_marker.find(END) else {
            return Err(DocGenError::Unterminated(name));
        };
        let _ = write!(out, "{BEGIN}{name}{BEGIN_END}\n{body}{END}");
        rest = after_marker.get(end + END.len()..).unwrap_or_default();
        seen.push(name);
    }
    out.push_str(rest);
    for s in SECTIONS {
        if !seen.iter().any(|n| n == s) {
            return Err(DocGenError::MissingSection((*s).to_owned()));
        }
    }
    Ok(out)
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|")
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn cpu_features() -> String {
    let mut s = String::from(
        "| Feature | Arch | Detector name | Guaranteed by build baseline | Required by dispatch levels |\n\
         |---|---|---|---|---|\n",
    );
    for f in CPU_FEATURES {
        let levels: Vec<String> = DISPATCH_PATHS
            .iter()
            .filter(|p| p.required_features.contains(&f.name))
            .map(|p| format!("`{}`", p.level))
            .collect();
        let levels = if levels.is_empty() { "detected and reported only".to_owned() } else { levels.join(", ") };
        let _ = writeln!(
            s,
            "| {} | {} | `{}` | {} | {} |",
            cell(f.display),
            f.arch.name(),
            f.name,
            yes_no(f.in_baseline),
            levels
        );
    }
    s
}

fn dispatch_paths() -> String {
    let mut s = String::from(
        "| Level | Arch | Required features | Code paths | Determinism | Measured speedup | Notes |\n\
         |---|---|---|---|---|---|---|\n",
    );
    for p in DISPATCH_PATHS {
        let feats = if p.required_features.is_empty() {
            "none".to_owned()
        } else {
            p.required_features.iter().map(|f| format!("`{f}`")).collect::<Vec<_>>().join(", ")
        };
        let paths = p.code_paths.iter().map(|c| cell(c)).collect::<Vec<_>>().join("<br>");
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {} | {} | {} | {} |",
            p.level,
            p.arch.map_or("any", |a| a.name()),
            feats,
            paths,
            p.determinism.label(),
            cell(&p.speedup.describe()),
            cell(p.notes)
        );
    }
    let names: Vec<String> = Dispatch::ALL.iter().map(|d| format!("`{d}`")).collect();
    let _ = write!(
        s,
        "\nForce a level with the CLI flag `--force-dispatch <level>` (any build) or, in debug/test builds \
         only, the environment variable `{FORCE_DISPATCH_ENV}`. Level names: {}. A forced level the CPU \
         cannot run is an error, never a silent fallback.\n",
        names.join(", ")
    );
    s
}

fn gpu_tiers() -> String {
    let mut s = String::from(
        "| Tier | Chosen automatically for | MSAA | SSAO | Shadows | LOD detail factor | LOD factor while moving | Render scale while moving | Notes |\n\
         |---|---|---|---|---|---|---|---|---|\n",
    );
    for t in GPU_TIERS {
        let auto = if t.auto_for.is_empty() {
            "manual only".to_owned()
        } else {
            t.auto_for.iter().map(|c| c.label()).collect::<Vec<_>>().join(", ")
        };
        let _ = writeln!(
            s,
            "| `{}` | {} | {}x | {} | {} | {:.2} | {:.2} | {:.2} | {} |",
            t.tier.name(),
            auto,
            t.msaa_samples,
            yes_no(t.ssao),
            yes_no(t.shadows),
            t.lod_detail_factor,
            t.interactive_lod_factor,
            t.interactive_render_scale,
            cell(t.notes)
        );
    }
    s
}

fn budgets() -> String {
    let mut s = String::from(
        "| Example machine | Interactive threads | Background threads | Cache budget | Default tessellation | Speculative precompute |\n\
         |---|---|---|---|---|---|\n",
    );
    for (name, logical, topo, ram) in EXAMPLE_MACHINES {
        let b = Budgets::compute(*logical, *topo, Some(gib(*ram)));
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} MiB | {} | {} |",
            cell(name),
            b.interactive_threads,
            b.background_threads,
            b.cache_bytes / (1024 * 1024),
            b.tessellation.name(),
            yes_no(b.speculative_precompute)
        );
    }
    let unknown = Budgets::compute(4, CoreTopology::Unknown, None);
    let _ = write!(
        s,
        "\nRules (from `gusset_hw::budgets`): interactive = max(1, logical - 1); background = efficiency \
         logical CPUs on hybrid CPUs, else interactive / 2, clamped to [1, interactive]; cache = RAM / 16 \
         clamped to [128 MiB, 4 GiB]; if RAM is unknown, {} GiB is assumed (cache {} MiB).\n",
        crate::budgets::ASSUMED_RAM_BYTES / gib(1),
        unknown.cache_bytes / (1024 * 1024)
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skeleton() -> String {
        let mut d = String::from("# Title\n\nIntro.\n");
        for s in SECTIONS {
            d.push_str(&format!("\n## {s}\n{BEGIN}{s}{BEGIN_END}\nstale\n{END}\n"));
        }
        d.push_str("\nTrailer.\n");
        d
    }

    #[test]
    fn regenerates_and_is_idempotent() {
        let once = update_document(&skeleton()).unwrap();
        assert!(!once.contains("stale"));
        assert!(once.starts_with("# Title\n\nIntro.\n"));
        assert!(once.ends_with("\nTrailer.\n"));
        assert_eq!(update_document(&once).unwrap(), once);
        assert!(once.contains("| `avx2fma` |"));
        assert!(once.contains("not yet measured"));
    }

    #[test]
    fn marker_errors() {
        assert_eq!(update_document("nothing"), Err(DocGenError::MissingSection("cpu-features".into())));
        let unknown = format!("{BEGIN}nope{BEGIN_END}\n{END}");
        assert_eq!(update_document(&unknown), Err(DocGenError::UnknownSection("nope".into())));
        let unterminated = format!("{BEGIN}budgets{BEGIN_END}\n");
        assert_eq!(update_document(&unterminated), Err(DocGenError::Unterminated("budgets".into())));
        let dup = format!("{0}budgets{1}\n{2}{0}budgets{1}\n{2}", BEGIN, BEGIN_END, END);
        assert_eq!(update_document(&dup), Err(DocGenError::Duplicate("budgets".into())));
    }

    #[test]
    fn every_section_generates() {
        for s in SECTIONS {
            assert!(generate_section(s).is_some_and(|b| b.ends_with('\n')), "{s}");
        }
    }
}
