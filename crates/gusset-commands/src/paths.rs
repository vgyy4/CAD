//! File access confined to the project folder (spec: MCP scope; ADR 0008 B3).

use crate::error::CommandError;
use std::path::{Component, Path, PathBuf};

/// Resolves `rel` inside `project_dir`, rejecting absolute paths, `..`, and symlink or
/// junction escapes (checked on the canonical parent directory).
pub fn confined(project_dir: Option<&Path>, rel: &str) -> Result<PathBuf, CommandError> {
    let dir = project_dir.ok_or_else(|| {
        CommandError::new("not_allowed", "this project has no folder yet").hint("save the project first")
    })?;
    if rel.is_empty() || rel.len() > 1024 || rel.contains('\0') {
        return Err(CommandError::args("invalid file name"));
    }
    let p = Path::new(rel);
    if !p.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(CommandError::new("not_allowed", "paths must stay inside the project folder")
            .hint("use a plain relative name such as exports/part.step"));
    }
    let root = dir.canonicalize().map_err(|e| CommandError::new("io", format!("project folder: {e}")))?;
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

/// Writes a file atomically (temp file in the same folder, then rename).
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), CommandError> {
    let dir = path.parent().ok_or_else(|| CommandError::args("invalid file name"))?;
    let tmp = dir.join(format!(
        "~$export.{}.gtmp",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    let io = |e: std::io::Error| CommandError::new("io", format!("could not write '{}': {e}", path.display()));
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(io)?;
        f.write_all(bytes).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_escapes() {
        let d = std::env::temp_dir().join(format!("gusset-paths-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(confined(Some(&d), "part.step").is_ok());
        for bad in ["../x.step", "/etc/passwd", "a/../../x", "", "C:\\x"] {
            let r = confined(Some(&d), bad);
            assert!(r.is_err() || !bad.contains(".."), "{bad}");
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
}
