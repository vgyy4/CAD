//! `history.export_bundle` / `history.import_bundle`: move a branch's history between two
//! copies of a project as a `.gusb` file (ADR 0044 phase 1).

use super::*;
use gusset_history::BundleLimits;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<HistoryExportBundle>(), spec::<HistoryImportBundle>(), spec::<HistorySync>()]
}

/// Write a branch and its history to a `.gusb` bundle file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryExportBundle {
    /// Branch to export; default: your current branch.
    #[serde(default)]
    pub branch: Option<String>,
    /// File name in the project folder, ending in .gusb.
    pub path: String,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for HistoryExportBundle {
    const ID: &'static str = "history.export_bundle";
    const TITLE: &'static str = "Export history bundle";
    const DESCRIPTION: &'static str = "Write a branch with its iterations and data to a .gusb file for another copy of the project (history_import_bundle).";
    const CATEGORY: Category = Category::Branches;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let path = crate::paths::export_target(s.project_dir.as_deref(), &self.path, &ctx.actor, &["gusb"])?;
        if path.exists() {
            if !self.overwrite {
                return Err(CommandError::new("confirmation_required", format!("'{}' already exists", self.path))
                    .hint("pass overwrite: true to replace it"));
            }
            if ctx.is_agent() && s.config.approvals_required && !ctx.approved {
                return Err(CommandError::approval("overwriting an exported file"));
            }
        }
        let b = match &self.branch {
            Some(r) => find_branch(s, r)?,
            None => s.branch_for(ctx)?,
        };
        let bytes = s.history().export_bundle(b).map_err(CommandError::history)?;
        crate::paths::write_file(&path, &self.path, &bytes)?;
        let h = s.history();
        let nodes = h.lineage(h.head(b).map_err(CommandError::history)?).len();
        Ok(json!({ "written": self.path, "bytes": bytes.len(), "iterations": nodes }))
    }
}

/// Read a .gusb bundle into a new branch `from/<name>`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryImportBundle {
    /// A .gusb file (project folder for agents; any path for the user).
    pub path: String,
}

impl Command for HistoryImportBundle {
    const ID: &'static str = "history.import_bundle";
    const TITLE: &'static str = "Import history bundle";
    const DESCRIPTION: &'static str = "Read a .gusb bundle: adds its iterations and a branch from/<name> (never main); compare and merge it as any branch.";
    const CATEGORY: Category = Category::Branches;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        crate::paths::refuse_remote(&self.path)?;
        let full = crate::paths::read_target(s.project_dir.as_deref(), &self.path, &ctx.actor)?;
        if full.extension().and_then(|e| e.to_str()).is_none_or(|e| !e.eq_ignore_ascii_case("gusb")) {
            return Err(CommandError::args(format!(
                "'{}' is not a history bundle",
                crate::paths::display_name(&self.path)
            ))
            .hint("bundles are .gusb files from history.export_bundle"));
        }
        let limits = BundleLimits::default();
        let file = crate::import::open_regular(&full, &self.path, limits.max_bundle_bytes as u64)?;
        let bytes = crate::import::read_capped(file, limits.max_bundle_bytes as u64, &s.rebuild_cancel_flag())?;
        let r = s.history_mut_pub().import_bundle(&ctx.hctx(), &bytes, &limits).map_err(CommandError::history)?;
        s.note_change(ctx, r.branch, "Import history bundle");
        Ok(json!({
            "branch": r.branch.to_string(),
            "name_user_text": r.branch_name,
            "head": r.head.to_string(),
            "created": r.branch_created,
            "iterations_added": r.nodes_added,
            "iterations_reused": r.nodes_reused,
            "objects_added": r.objects_added,
        }))
    }
}

/// Folder sync (ADR 0044, between phases 1 and 2): every collaborator points the same
/// shared folder (a synced drive, a NAS) and their own short name at it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistorySync {
    /// Shared folder.
    pub folder: String,
    /// Your name there.
    pub who: String,
}

/// Most bundles one sync reads.
const MAX_SYNC_FILES: usize = 256;

impl Command for HistorySync {
    const ID: &'static str = "history.sync";
    const TITLE: &'static str = "Sync with a shared folder";
    const DESCRIPTION: &'static str =
        "Write your branches to a shared folder as bundles and import the others' (from/<who>/<branch>).";
    const CATEGORY: Category = Category::Branches;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if ctx.is_agent() {
            return Err(CommandError::new("not_allowed", "only the user syncs with a shared folder")
                .hint("use history_export_bundle in the project folder"));
        }
        let who = self.who.trim();
        if who.is_empty() || who.len() > 32 || !who.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(CommandError::args("who is 1 to 32 letters, digits, - or _"));
        }
        crate::paths::refuse_remote(&self.folder)?;
        let dir = std::path::PathBuf::from(&self.folder);
        if !dir.is_dir() {
            return Err(CommandError::not_found(format!(
                "'{}' is not a folder",
                crate::paths::display_name(&self.folder)
            )));
        }
        // Out: each of your own branches (not the ones received from others).
        let prefix = format!("{who}__");
        let mut written = Vec::new();
        let branches: Vec<(gusset_history::BranchId, String)> = s
            .history()
            .branches()
            .iter()
            .filter(|b| !b.name.starts_with("from/"))
            .map(|b| (b.id, b.name.clone()))
            .collect();
        for (id, name) in branches {
            let bytes = s.history().export_bundle_as(id, &format!("{who}/{name}")).map_err(CommandError::history)?;
            let safe: String =
                name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
            let file = format!("{prefix}{safe}.gusb");
            let path = dir.join(&file);
            if std::fs::read(&path).is_ok_and(|old| old == bytes) {
                continue;
            }
            crate::paths::write_file(&path, &file, &bytes)?;
            written.push(file);
        }
        // In: everyone else's bundles.
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| CommandError::new("io", format!("cannot list the folder: {}", e.kind())))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("gusb"))
                    && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| !n.starts_with(&prefix))
            })
            .collect();
        files.sort();
        files.truncate(MAX_SYNC_FILES);
        let limits = BundleLimits::default();
        let (mut imported, mut failed) = (Vec::new(), Vec::new());
        for path in files {
            let shown = path.file_name().and_then(|n| n.to_str()).unwrap_or("bundle").to_string();
            let read = crate::import::open_regular(&path, &shown, limits.max_bundle_bytes as u64)
                .and_then(|f| crate::import::read_capped(f, limits.max_bundle_bytes as u64, &s.rebuild_cancel_flag()));
            let r = read.and_then(|bytes| {
                s.history_mut_pub().import_bundle(&ctx.hctx(), &bytes, &limits).map_err(CommandError::history)
            });
            match r {
                Ok(r) => {
                    if r.nodes_added > 0 || r.branch_created {
                        s.note_change(ctx, r.branch, "Sync from shared folder");
                    }
                    imported.push(json!({"file_user_text": shown, "branch_user_text": r.branch_name, "iterations_added": r.nodes_added}));
                }
                Err(e) => failed.push(json!({"file_user_text": shown, "error": e.message})),
            }
        }
        Ok(json!({ "written_user_text": written, "imported": imported, "failed": failed }))
    }
}
