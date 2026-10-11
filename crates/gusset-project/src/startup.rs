//! The single startup routine (review M1): run once when the app or the CLI starts,
//! before any project is opened.
//!
//! 1. Startup housekeeping (ADR 0007): orphaned temp files, stale locks, recovery/LKG
//!    copies and cache namespaces of projects deleted outside the app. Recovery copies
//!    that no registered project owns (never-saved projects) are **kept**, and nothing
//!    of a project whose lock a running instance holds is touched.
//! 2. The cache is brought under its size cap (LRU eviction; everything in it is
//!    regenerable).
//! 3. The recovery copies to offer the user are listed ([`crate::pending_recoveries`]).
//!
//! Deleted-branch retention and the history size limits are enforced per project when
//! it is opened (`Project::open`), because that data lives inside project files.

use crate::settings::Settings;
use crate::{PendingRecovery, ProjectError, pending_recoveries};
use gusset_store::cache::{self, Cache};
use gusset_store::housekeeping;
use gusset_store::paths::DataDirs;
use std::path::PathBuf;

/// What [`startup`] did and found.
#[derive(Debug, Clone)]
pub struct StartupReport {
    /// The user's settings.
    pub settings: Settings,
    /// What housekeeping removed (and kept).
    pub housekeeping: housekeeping::Report,
    /// Cache entries evicted to respect the cap.
    pub cache_evicted: usize,
    /// Recovery copies to offer ("Recover unsaved work"), newest first.
    pub pending_recoveries: Vec<PendingRecovery>,
}

/// Runs the startup routine. `extra_dirs` are more directories to clean of our own
/// orphaned temp files (for example recently used project folders).
///
/// # Errors
/// Only if the data directories cannot be created; failures of individual cleanup
/// steps are skipped (housekeeping is best effort and runs again next start).
pub fn startup(dirs: &DataDirs, extra_dirs: &[PathBuf]) -> Result<StartupReport, ProjectError> {
    dirs.ensure().map_err(|e| ProjectError::Io { path: dirs.data_dir().to_path_buf(), message: e.to_string() })?;
    let settings = Settings::load(dirs);
    let opts = housekeeping::Options { extra_dirs: extra_dirs.to_vec(), ..housekeeping::Options::default() };
    let housekeeping = housekeeping::run(dirs, &opts).unwrap_or_default();
    let cap = settings.cache_cap.unwrap_or_else(|| cache::default_cap(0));
    let cache_evicted = Cache::open(dirs.cache_dir(), cap).and_then(|c| c.evict()).unwrap_or(0);
    let pending_recoveries = pending_recoveries(dirs);
    Ok(StartupReport { settings, housekeeping, cache_evicted, pending_recoveries })
}
