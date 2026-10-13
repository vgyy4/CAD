//! Save modes and storage settings (PROMPT "Saving, crash recovery & storage hygiene",
//! ADR 0007), and the scheduler that turns them into "save now" / "write a recovery
//! snapshot now" decisions (review M2).
//!
//! - [`Settings`]: per-user, persisted in `<data>/settings.txt` through `durable_write`.
//! - [`ProjectSettings`]: per-project overrides, stored inside the project file (root
//!   `settings`), so they travel with the project.
//! - [`SaveScheduler`]: pure timing logic. The front end tells it when an operation
//!   completed ([`SaveScheduler::changed`]) and asks it what is due
//!   ([`SaveScheduler::due`]) whenever the model is quiescent; it never reads the clock
//!   itself, so it is deterministic and testable.
//!
//! Settings file format (unknown keys are ignored, bad values fall back to defaults):
//! ```text
//! gusset-settings 1
//! save_mode manual|auto
//! autosave_debounce_ms 2000
//! recovery_interval_s 60
//! recovery_after_changes 20
//! deleted_retention_days 7
//! prune_after_days 30|off
//! cache_cap_mib 0            (0 = automatic: 2 % of free space, 256 MiB to 4 GiB)
//! ```

use gusset_history::GcPolicy;
use gusset_store::durable::durable_write_unchecked;
use gusset_store::fs::RealFs;
use gusset_store::paths::DataDirs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const HEADER: &str = "gusset-settings 1";

/// How the project file gets written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveMode {
    /// The project file changes only when the user saves; recovery snapshots are written
    /// every `recovery_interval` or after `recovery_after_changes` changes.
    Manual,
    /// The project file is saved automatically, `autosave_debounce` after the last
    /// completed operation. Never-saved projects get recovery snapshots instead.
    Auto,
}

impl SaveMode {
    fn as_str(self) -> &'static str {
        match self {
            SaveMode::Manual => "manual",
            SaveMode::Auto => "auto",
        }
    }

    fn parse(s: &str) -> Option<SaveMode> {
        match s {
            "manual" => Some(SaveMode::Manual),
            "auto" => Some(SaveMode::Auto),
            _ => None,
        }
    }
}

/// Per-user settings (global defaults).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub save_mode: SaveMode,
    pub autosave_debounce: Duration,
    pub recovery_interval: Duration,
    pub recovery_after_changes: u32,
    /// "Recently deleted" branches are purged after this long.
    pub deleted_retention: Duration,
    /// Unnamed intermediate iterations older than this are squashed by "Compact
    /// history"; `None` turns pruning off.
    pub prune_after: Option<Duration>,
    /// Cache size cap in bytes; `None` = automatic.
    pub cache_cap: Option<u64>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            save_mode: SaveMode::Manual,
            autosave_debounce: Duration::from_secs(2),
            recovery_interval: Duration::from_secs(60),
            recovery_after_changes: 20,
            deleted_retention: Duration::from_secs(7 * 24 * 3600),
            prune_after: Some(Duration::from_secs(30 * 24 * 3600)),
            cache_cap: None,
        }
    }
}

const DAY: u64 = 24 * 3600;

/// Bounds that keep a hand-edited file from disabling recovery or spinning the CPU.
fn clamp_interval(d: Duration) -> Duration {
    d.clamp(Duration::from_secs(5), Duration::from_secs(3600))
}

impl Settings {
    /// The settings file.
    #[must_use]
    pub fn path(dirs: &DataDirs) -> PathBuf {
        dirs.data_dir().join("settings.txt")
    }

    /// Loads the user's settings (missing or unreadable file: defaults).
    #[must_use]
    pub fn load(dirs: &DataDirs) -> Settings {
        std::fs::read(Self::path(dirs)).map(|b| Self::parse(&String::from_utf8_lossy(&b))).unwrap_or_default()
    }

    /// Saves the settings durably.
    ///
    /// # Errors
    /// I/O errors.
    pub fn save(&self, dirs: &DataDirs) -> std::io::Result<()> {
        gusset_store::paths::create_private_dir_all(dirs.data_dir())?;
        durable_write_unchecked(&RealFs::private(), &Self::path(dirs), self.to_text().as_bytes())
            .map_err(|e| std::io::Error::other(e.to_string()))
    }

    /// Serialises the settings.
    #[must_use]
    pub fn to_text(&self) -> String {
        format!(
            "{HEADER}\nsave_mode {}\nautosave_debounce_ms {}\nrecovery_interval_s {}\nrecovery_after_changes {}\n\
             deleted_retention_days {}\nprune_after_days {}\ncache_cap_mib {}\n",
            self.save_mode.as_str(),
            self.autosave_debounce.as_millis(),
            self.recovery_interval.as_secs(),
            self.recovery_after_changes,
            self.deleted_retention.as_secs() / DAY,
            self.prune_after.map_or_else(|| "off".to_string(), |d| (d.as_secs() / DAY).to_string()),
            self.cache_cap.map_or(0, |b| b >> 20),
        )
    }

    /// Parses settings text leniently.
    #[must_use]
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return s;
        }
        for line in lines {
            let Some((k, v)) = line.split_once(' ') else { continue };
            let n = v.trim().parse::<u64>().ok();
            match (k, n) {
                ("save_mode", _) => s.save_mode = SaveMode::parse(v.trim()).unwrap_or(s.save_mode),
                ("autosave_debounce_ms", Some(n)) => {
                    s.autosave_debounce = Duration::from_millis(n.clamp(250, 60_000));
                }
                ("recovery_interval_s", Some(n)) => s.recovery_interval = clamp_interval(Duration::from_secs(n)),
                ("recovery_after_changes", Some(n)) => {
                    s.recovery_after_changes = u32::try_from(n.clamp(1, 10_000)).unwrap_or(20);
                }
                ("deleted_retention_days", Some(n)) => s.deleted_retention = Duration::from_secs(n.min(3650) * DAY),
                ("prune_after_days", _) if v.trim() == "off" => s.prune_after = None,
                ("prune_after_days", Some(n)) => s.prune_after = Some(Duration::from_secs(n.clamp(1, 3650) * DAY)),
                ("cache_cap_mib", Some(0)) => s.cache_cap = None,
                ("cache_cap_mib", Some(n)) => s.cache_cap = Some(n.clamp(16, 1 << 20) << 20),
                _ => {}
            }
        }
        s
    }

    /// The history GC policy these settings describe. `prune` = include squashing of
    /// old intermediate iterations ("Compact history"); retention alone otherwise.
    #[must_use]
    pub fn gc_policy(&self, prune: bool) -> GcPolicy {
        GcPolicy {
            deleted_retention: self.deleted_retention.as_secs(),
            squash_after: if prune { self.prune_after.map(|d| d.as_secs()) } else { None },
        }
    }

    /// These settings with a project's overrides applied.
    #[must_use]
    pub fn with_overrides(&self, o: &ProjectSettings) -> Settings {
        let mut s = self.clone();
        if let Some(m) = o.save_mode {
            s.save_mode = m;
        }
        if let Some(i) = o.recovery_interval {
            s.recovery_interval = clamp_interval(i);
        }
        if let Some(n) = o.recovery_after_changes {
            s.recovery_after_changes = n.clamp(1, 10_000);
        }
        s
    }
}

/// Per-project overrides of the save settings (stored in the project file).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSettings {
    pub save_mode: Option<SaveMode>,
    pub recovery_interval: Option<Duration>,
    pub recovery_after_changes: Option<u32>,
}

impl ProjectSettings {
    /// Whether nothing is overridden.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == ProjectSettings::default()
    }

    pub(crate) fn to_text(&self) -> String {
        let mut t = String::from("project-settings 1\n");
        if let Some(m) = self.save_mode {
            t.push_str(&format!("save_mode {}\n", m.as_str()));
        }
        if let Some(i) = self.recovery_interval {
            t.push_str(&format!("recovery_interval_s {}\n", i.as_secs()));
        }
        if let Some(n) = self.recovery_after_changes {
            t.push_str(&format!("recovery_after_changes {n}\n"));
        }
        t
    }

    pub(crate) fn parse(text: &str) -> ProjectSettings {
        let mut o = ProjectSettings::default();
        let mut lines = text.lines();
        if lines.next() != Some("project-settings 1") {
            return o;
        }
        for line in lines {
            let Some((k, v)) = line.split_once(' ') else { continue };
            match k {
                "save_mode" => o.save_mode = SaveMode::parse(v.trim()),
                "recovery_interval_s" => o.recovery_interval = v.trim().parse().ok().map(Duration::from_secs),
                "recovery_after_changes" => o.recovery_after_changes = v.trim().parse().ok(),
                _ => {}
            }
        }
        o
    }
}

/// What is due according to the save mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveAction {
    /// Nothing to do now.
    Nothing,
    /// Auto-save: write the project file (`Project::save`).
    SaveProject,
    /// Write a recovery snapshot (`Project::snapshot`).
    WriteRecovery,
}

/// Decides when to auto-save or snapshot. Only ever acts at consistent points: the
/// caller passes `quiescent` (no command, import, rebuild or agent transaction running)
/// and nothing is due otherwise; the last consistent state is snapshotted once the
/// running operation finished.
#[derive(Debug, Clone)]
pub struct SaveScheduler {
    settings: Settings,
    /// Changes not yet in the project file or a recovery snapshot: sequence number and
    /// completion time, oldest first. A write clears only the changes up to the mark
    /// taken when it started (review M2: edits during a background write stay pending).
    unwritten: std::collections::VecDeque<(u64, Instant)>,
    /// Sequence number of the latest change.
    seq: u64,
    /// When the last change completed.
    last_change: Option<Instant>,
}

impl SaveScheduler {
    /// A scheduler using `settings` (already merged with project overrides).
    #[must_use]
    pub fn new(settings: Settings) -> Self {
        SaveScheduler { settings, unwritten: std::collections::VecDeque::new(), seq: 0, last_change: None }
    }

    /// The settings in effect.
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Changes the settings (e.g. the user switched to auto-save).
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    /// An operation completed and changed the project.
    pub fn changed(&mut self, now: Instant) {
        self.seq += 1;
        self.unwritten.push_back((self.seq, now));
        self.last_change = Some(now);
    }

    /// What should happen now. `has_file`: the project was saved before (auto-save
    /// needs a file; never-saved projects get recovery snapshots).
    #[must_use]
    pub fn due(&self, now: Instant, has_file: bool, quiescent: bool) -> SaveAction {
        if !quiescent || self.unwritten.is_empty() {
            return SaveAction::Nothing;
        }
        let s = &self.settings;
        if s.save_mode == SaveMode::Auto && has_file {
            let quiet = self.last_change.is_none_or(|t| now.saturating_duration_since(t) >= s.autosave_debounce);
            return if quiet { SaveAction::SaveProject } else { SaveAction::Nothing };
        }
        let old = self.pending_since().is_some_and(|t| now.saturating_duration_since(t) >= s.recovery_interval);
        if old || self.pending_changes() >= s.recovery_after_changes {
            SaveAction::WriteRecovery
        } else {
            SaveAction::Nothing
        }
    }

    /// When [`SaveScheduler::due`] next becomes true without further changes, so an idle
    /// front end can sleep until then instead of polling. `None` = nothing pending.
    #[must_use]
    pub fn next_deadline(&self, has_file: bool) -> Option<Instant> {
        if self.unwritten.is_empty() {
            return None;
        }
        if self.settings.save_mode == SaveMode::Auto && has_file {
            return self.last_change.map(|t| t + self.settings.autosave_debounce);
        }
        self.pending_since().map(|t| t + self.settings.recovery_interval)
    }

    /// The due action (or a manual save) succeeded: everything so far is on disk.
    pub fn written(&mut self) {
        self.written_up_to(self.seq);
    }

    /// Marks the changes so far, when a write (save or recovery snapshot) starts; pass
    /// the mark to [`SaveScheduler::written_up_to`] when it succeeds.
    #[must_use]
    pub fn mark(&self) -> u64 {
        self.seq
    }

    /// A write that started at `mark` succeeded: the changes up to it are on disk, later
    /// ones (made while it was running) stay pending with their own times.
    pub fn written_up_to(&mut self, mark: u64) {
        while self.unwritten.front().is_some_and(|&(n, _)| n <= mark) {
            self.unwritten.pop_front();
        }
    }

    /// When the oldest unwritten change completed.
    fn pending_since(&self) -> Option<Instant> {
        self.unwritten.front().map(|&(_, t)| t)
    }

    /// Changes not yet written anywhere.
    #[must_use]
    pub fn pending_changes(&self) -> u32 {
        u32::try_from(self.unwritten.len()).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trip_and_lenient_parse() {
        let s = Settings {
            save_mode: SaveMode::Auto,
            autosave_debounce: Duration::from_millis(1500),
            recovery_interval: Duration::from_secs(90),
            recovery_after_changes: 7,
            deleted_retention: Duration::from_secs(3 * DAY),
            prune_after: None,
            cache_cap: Some(512 << 20),
        };
        assert_eq!(Settings::parse(&s.to_text()), s);
        assert_eq!(Settings::parse("garbage"), Settings::default());
        let t = "gusset-settings 1\nsave_mode bogus\nrecovery_interval_s 0\nunknown 5\nrecovery_after_changes x\n";
        let p = Settings::parse(t);
        assert_eq!(p.save_mode, SaveMode::Manual);
        assert_eq!(p.recovery_interval, Duration::from_secs(5), "clamped: recovery cannot be disabled");
        assert_eq!(p.recovery_after_changes, 20);
        let o = ProjectSettings {
            save_mode: Some(SaveMode::Auto),
            recovery_interval: None,
            recovery_after_changes: Some(3),
        };
        assert_eq!(ProjectSettings::parse(&o.to_text()), o);
        let m = Settings::default().with_overrides(&o);
        assert_eq!((m.save_mode, m.recovery_after_changes), (SaveMode::Auto, 3));
    }

    #[test]
    fn edits_during_a_write_stay_pending() {
        // Review M2: a background recovery write that finishes must not mark edits made
        // while it ran as written.
        let t0 = Instant::now();
        let mut s = SaveScheduler::new(Settings::default());
        s.changed(t0);
        s.changed(t0 + Duration::from_secs(1));
        let mark = s.mark();
        s.changed(t0 + Duration::from_secs(5)); // during the write
        s.written_up_to(mark);
        assert_eq!(s.pending_changes(), 1);
        // Its own clock: due 60 s after *that* change, not after the first one.
        assert_eq!(s.next_deadline(true), Some(t0 + Duration::from_secs(65)));
        // An older write finishing late changes nothing more; a full write clears all.
        s.written_up_to(mark - 1);
        assert_eq!(s.pending_changes(), 1);
        s.written();
        assert_eq!(s.pending_changes(), 0);
        assert_eq!(s.next_deadline(true), None);
    }

    #[test]
    fn manual_mode_snapshots_after_interval_or_changes() {
        let t0 = Instant::now();
        let mut s = SaveScheduler::new(Settings::default());
        assert_eq!(s.due(t0, true, true), SaveAction::Nothing);
        s.changed(t0);
        assert_eq!(s.due(t0 + Duration::from_secs(59), true, true), SaveAction::Nothing);
        assert_eq!(s.next_deadline(true), Some(t0 + Duration::from_secs(60)));
        assert_eq!(s.due(t0 + Duration::from_secs(60), true, true), SaveAction::WriteRecovery);
        // Never in the middle of an operation.
        assert_eq!(s.due(t0 + Duration::from_secs(60), true, false), SaveAction::Nothing);
        s.written();
        for i in 0..19 {
            s.changed(t0 + Duration::from_secs(61 + i));
        }
        assert_eq!(s.due(t0 + Duration::from_secs(80), true, true), SaveAction::Nothing);
        s.changed(t0 + Duration::from_secs(81));
        assert_eq!(s.due(t0 + Duration::from_secs(81), true, true), SaveAction::WriteRecovery);
    }

    #[test]
    fn auto_mode_saves_after_debounce_and_snapshots_untitled() {
        let t0 = Instant::now();
        let mut s = SaveScheduler::new(Settings { save_mode: SaveMode::Auto, ..Settings::default() });
        s.changed(t0);
        s.changed(t0 + Duration::from_secs(1));
        assert_eq!(s.due(t0 + Duration::from_secs(2), true, true), SaveAction::Nothing, "debounced");
        assert_eq!(s.due(t0 + Duration::from_secs(3), true, true), SaveAction::SaveProject);
        assert_eq!(s.due(t0 + Duration::from_secs(3), true, false), SaveAction::Nothing);
        // A never-saved project cannot be auto-saved: recovery snapshots instead.
        assert_eq!(s.due(t0 + Duration::from_secs(61), false, true), SaveAction::WriteRecovery);
        s.written();
        assert_eq!(s.due(t0 + Duration::from_secs(99), true, true), SaveAction::Nothing);
        assert_eq!(s.next_deadline(true), None);
    }
}
