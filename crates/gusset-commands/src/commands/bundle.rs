//! `history.export_bundle` / `history.import_bundle`: move a branch's history between two
//! copies of a project as a `.gusb` file (ADR 0044 phase 1).

use super::*;
use gusset_history::BundleLimits;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<HistoryExportBundle>(), spec::<HistoryImportBundle>()]
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
