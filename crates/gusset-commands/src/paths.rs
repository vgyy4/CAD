//! File access for commands (spec: MCP scope; ADR 0008 B3).
//!
//! - **Agents** read and write only inside the project folder ([`confined`]), and their
//!   export names are checked strictly ([`agent_export_name`]): an allow-listed
//!   extension, no hidden (`.git`, dotfile) components, no Windows device names, no
//!   trailing dots or spaces, no alternate data streams.
//! - **The user** (GUI file dialog, CLI) may export anywhere ([`export_target`]).
//! - Writes go through the store's audited `durable_write` ([`write_file`]), whose temp
//!   names housekeeping recognises.
//! - Error messages never contain absolute paths (they may reach agents and logs).

use crate::error::CommandError;
use gusset_history::Actor;
use std::path::{Component, Path, PathBuf};

/// Extensions agents may export to.
pub const AGENT_EXPORT_EXTENSIONS: &[&str] = &["step", "stp", "stl", "3mf", "obj"];

/// Windows device names, reserved in any folder and with any extension.
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "conin$", "conout$", "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
    "com8", "com9", "com¹", "com²", "com³", "lpt0", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8",
    "lpt9", "lpt¹", "lpt²", "lpt³",
];

/// Resolves `rel` inside `project_dir`, rejecting absolute paths, `..`, and symlink or
/// junction escapes (checked on the canonical parent directory).
///
/// # Errors
/// `not_allowed` for escapes or a project without a folder; `invalid_args`/`not_found`
/// for bad names or missing folders. Messages never contain absolute paths.
pub fn confined(project_dir: Option<&Path>, rel: &str) -> Result<PathBuf, CommandError> {
    let dir = project_dir.ok_or_else(|| {
        CommandError::new("not_allowed", "this project has no folder yet").hint("save the project first")
    })?;
    if rel.is_empty() || rel.len() > 1024 || rel.contains('\0') {
        return Err(CommandError::args("invalid file name"));
    }
    let p = Path::new(rel);
    // On Unix a backslash is an ordinary character, but on Windows it separates
    // components (and `C:\x` is absolute): refuse it everywhere so names mean the same
    // on every platform.
    if rel.contains('\\') || !p.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(CommandError::new("not_allowed", "paths must stay inside the project folder")
            .hint("use a plain relative name such as exports/part.step"));
    }
    let root = dir.canonicalize().map_err(|e| CommandError::new("io", format!("project folder: {}", e.kind())))?;
    let full = root.join(p);
    let parent = full.parent().ok_or_else(|| CommandError::args("invalid file name"))?;
    let cparent = parent.canonicalize().map_err(|_| {
        CommandError::not_found("the target folder does not exist").hint("create the folder inside the project first")
    })?;
    if !cparent.starts_with(&root) {
        return Err(CommandError::new("not_allowed", "that path leaves the project folder (link or junction)"));
    }
    let name = full.file_name().ok_or_else(|| CommandError::args("invalid file name"))?;
    let target = cparent.join(name);
    // An existing symlink at the target itself must not point outside either.
    if let Ok(meta) = std::fs::symlink_metadata(&target)
        && meta.file_type().is_symlink()
    {
        return Err(CommandError::new("not_allowed", "refusing to write through a symbolic link"));
    }
    Ok(target)
}

/// Strict checks on an agent-chosen export name (relative to the project folder):
/// every component must be a plain, visible, portable name, and the extension must be
/// one of `allowed` (lower-case, without the dot).
///
/// # Errors
/// `not_allowed` with a hint naming the rule.
pub fn agent_export_name(rel: &str, allowed: &[&str]) -> Result<(), CommandError> {
    let refuse = |why: &str| {
        CommandError::new("not_allowed", format!("export file name refused: {why}"))
            .hint("use a plain name inside the project folder such as exports/part.step")
    };
    for comp in rel.split('/') {
        if comp.is_empty() {
            return Err(refuse("empty path component"));
        }
        if comp.starts_with('.') {
            return Err(refuse("hidden files and folders (names starting with '.') are not allowed"));
        }
        if comp.ends_with('.') || comp.ends_with(' ') || comp.starts_with(' ') {
            return Err(refuse("names may not start with a space or end with a dot or space"));
        }
        if comp.chars().any(|c| c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\\')) {
            return Err(refuse("the name contains a character that is not allowed in file names"));
        }
        let base = comp.split('.').next().unwrap_or(comp).trim_end().to_lowercase();
        if RESERVED.contains(&base.as_str()) {
            return Err(refuse("that is a reserved device name on Windows"));
        }
    }
    let ext = Path::new(rel).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if !allowed.contains(&ext.as_str()) {
        return Err(CommandError::new(
            "not_allowed",
            format!("export file name refused: the extension must be one of .{}", allowed.join(", .")),
        )
        .hint("name the file after its format, e.g. part.step or part.stl"));
    }
    Ok(())
}

/// Where an export of `path` by `actor` goes.
///
/// - Agents: `path` is relative to the project folder, checked by
///   [`agent_export_name`] against `allowed` and confined by [`confined`].
/// - The user: any path they chose (absolute, or relative to the project folder).
///
/// # Errors
/// As [`confined`] and [`agent_export_name`].
pub fn export_target(
    project_dir: Option<&Path>,
    path: &str,
    actor: &Actor,
    allowed: &[&str],
) -> Result<PathBuf, CommandError> {
    match actor {
        Actor::Agent(_) => {
            // Check the plain-syntax rules first so `..` and absolute paths get the
            // confinement message rather than a name-rule one.
            let target = confined(project_dir, path)?;
            agent_export_name(path, allowed)?;
            Ok(target)
        }
        Actor::User => {
            if path.is_empty() || path.contains('\0') {
                return Err(CommandError::args("invalid file name"));
            }
            let p = Path::new(path);
            if p.is_absolute() {
                return Ok(p.to_path_buf());
            }
            let dir = project_dir.ok_or_else(|| {
                CommandError::new("not_allowed", "this project has no folder yet")
                    .hint("choose an absolute path, or save the project first")
            })?;
            Ok(dir.join(p))
        }
    }
}

/// The file `actor` asks to read (imports): confined for agents ([`confined`]), any
/// path the user chose (absolute, or relative to the project folder).
///
/// # Errors
/// As [`confined`].
pub fn read_target(project_dir: Option<&Path>, path: &str, actor: &Actor) -> Result<PathBuf, CommandError> {
    match actor {
        Actor::Agent(_) => confined(project_dir, path),
        Actor::User => export_target(project_dir, path, actor, &[]),
    }
}

/// Writes an exported file through the store's `durable_write` (temp file named
/// `~$<stem>.<16 hex>.gtmp`, fsync, read-back check, atomic replace, directory fsync).
/// `shown` is the name used in error messages (never an absolute path).
///
/// # Errors
/// `io` with the failing step.
pub fn write_file(path: &Path, shown: &str, bytes: &[u8]) -> Result<(), CommandError> {
    use gusset_store::durable::{DurableError, durable_write_unchecked};
    durable_write_unchecked(&gusset_store::fs::RealFs::default(), path, bytes).map_err(|e| {
        let why = match &e {
            DurableError::Io { step, source, .. } => format!("{step}: {}", source.kind()),
            DurableError::ReadBackMismatch { .. } => "the data read back from disk differs".to_string(),
            DurableError::Invalid(_) => "verification failed".to_string(),
        };
        CommandError::new("io", format!("could not write '{}': {why}", crate::paths::display_name(shown)))
    })
}

/// A short, path-free name for messages: the file name of `shown`.
#[must_use]
pub fn display_name(shown: &str) -> String {
    Path::new(shown).file_name().map_or_else(|| "file".to_string(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gusset-paths-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn rejects_escapes() {
        let d = tmp("esc");
        assert!(confined(Some(&d), "part.step").is_ok());
        for bad in ["../x.step", "/etc/passwd", "a/../../x", "", "C:\\x", "..\\x.step", "a/./b.step", "x\0.step"] {
            let r = confined(Some(&d), bad);
            assert!(r.is_err(), "{bad:?} was accepted: {r:?}");
        }
        assert!(confined(None, "x.step").is_err());
        #[cfg(unix)]
        {
            let outside = std::env::temp_dir();
            let link = d.join("out");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert_eq!(confined(Some(&d), "out/x.step").unwrap_err().code, "not_allowed");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn agent_names_are_strict() {
        let ok = ["part.step", "exports/part.STP", "a b.stl", "v1.2.3mf.3mf", "x.obj"];
        for n in ok {
            assert!(agent_export_name(n, AGENT_EXPORT_EXTENSIONS).is_ok(), "{n}");
        }
        let bad = [
            ".git/hooks/pre-commit",
            ".bashrc.stl",
            "a/.hidden/x.step",
            "evil.desktop",
            "noext",
            "CON.step",
            "con",
            "aux.stl",
            "nul.tar.step",
            "COM1.stl",
            "lpt9.obj",
            "x.step.",
            "x.step ",
            " x.step",
            "dir./x.step",
            "x.step:stream",
            "a//b.step",
            "x?.stl",
        ];
        for n in bad {
            assert_eq!(agent_export_name(n, AGENT_EXPORT_EXTENSIONS).unwrap_err().code, "not_allowed", "{n}");
        }
    }

    #[test]
    fn users_may_export_anywhere_and_writes_are_durable() {
        let d = tmp("user");
        let other = tmp("user-other");
        let abs = other.join("anywhere.step");
        let t = export_target(Some(&d), abs.to_str().unwrap(), &Actor::User, AGENT_EXPORT_EXTENSIONS).unwrap();
        assert_eq!(t, abs);
        let t = export_target(None, abs.to_str().unwrap(), &Actor::User, &[]).unwrap();
        assert_eq!(t, abs);
        let agent = Actor::Agent("a".into());
        assert_eq!(
            export_target(Some(&d), abs.to_str().unwrap(), &agent, AGENT_EXPORT_EXTENSIONS).unwrap_err().code,
            "not_allowed"
        );
        write_file(&abs, "anywhere.step", b"data").unwrap();
        assert_eq!(std::fs::read(&abs).unwrap(), b"data");
        write_file(&abs, "anywhere.step", b"new").unwrap();
        assert_eq!(std::fs::read(&abs).unwrap(), b"new");
        // No temp files are left behind, and failures name no absolute path.
        let names: Vec<_> = std::fs::read_dir(&other).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "{names:?}");
        let e = write_file(&other.join("missing/x.step"), "missing/x.step", b"x").unwrap_err();
        assert!(!e.message.contains(other.to_str().unwrap()), "{}", e.message);
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&other);
    }
}
