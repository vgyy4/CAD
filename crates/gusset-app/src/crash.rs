//! Crash reports (DESIGN.md UX defaults: no telemetry; opt-in crash reports at most,
//! showing exactly what would be sent). A panic writes a plain-text report into the data
//! folder: version, platform, the panic message and where it happened, and a backtrace of
//! function names. Nothing is ever sent: the app says a report exists, shows it word for
//! word, and the user may attach it to an issue or delete it. The home folder is written as
//! `~` and the user name as `<user>` so the report does not carry them.

use std::path::{Path, PathBuf};

/// Reports kept (the oldest go first).
pub const MAX_REPORTS: usize = 5;

/// The folder reports live in.
#[must_use]
pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("crash-reports")
}

/// `text` with the home folder written as `~`.
#[must_use]
pub fn redact(text: &str) -> String {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    let mut out = if home.len() > 1 { text.replace(&home, "~") } else { text.to_string() };
    // The user name elsewhere too (other users' folders, temp paths; security review S13).
    for var in ["USER", "USERNAME", "LOGNAME"] {
        if let Ok(name) = std::env::var(var)
            && name.len() >= 3
        {
            out = out.replace(&name, "<user>");
        }
    }
    out
}

/// The report for a panic with `message` at `location`.
#[must_use]
pub fn report(message: &str, location: &str, backtrace: &str, unix_time: u64) -> String {
    redact(&format!(
        "Gusset crash report\nversion: {}\nplatform: {} {}\ntime (unix): {unix_time}\n\npanic: {message}\nat: {location}\n\nbacktrace:\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    ))
}

/// Writes a report file, dropping the oldest beyond [`MAX_REPORTS`].
///
/// # Errors
/// The folder or file cannot be written.
pub fn write(dir: &Path, text: &str, unix_time: u64) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("crash-{unix_time}.txt"));
    std::fs::write(&path, text)?;
    let mut all = list(dir);
    while all.len() > MAX_REPORTS {
        if let Some(oldest) = all.pop() {
            let _ = std::fs::remove_file(oldest);
        }
    }
    Ok(path)
}

/// The reports in `dir`, newest first.
#[must_use]
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

/// Installs the panic hook that writes reports into `dir` (the default hook still prints).
pub fn install(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let location = info.location().map_or_else(|| "unknown".into(), |l| format!("{}:{}", l.file(), l.line()));
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let now = crate::app::now_unix();
        let _ = write(&dir, &report(&message, &location, &bt, now), now);
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn reports_are_redacted_kept_to_five_and_listed_newest_first() {
        let d = std::env::temp_dir().join(format!("gusset-crash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        if let Ok(home) = std::env::var("HOME")
            && home.len() > 1
        {
            let r = report(&format!("cannot open {home}/secret.gus"), "x.rs:1", "", 1);
            assert!(r.contains("~/secret.gus") && !r.contains(&home), "{r}");
        }
        for t in 1..=7u64 {
            write(&d, &report("boom", "a.rs:2", "frames", t), t).unwrap();
        }
        let l = list(&d);
        assert_eq!(l.len(), MAX_REPORTS);
        assert!(l[0].ends_with("crash-7.txt"), "{l:?}");
        let text = std::fs::read_to_string(&l[0]).unwrap();
        assert!(text.contains("panic: boom") && text.contains("version: "), "{text}");
        let _ = std::fs::remove_dir_all(d);
    }
}
