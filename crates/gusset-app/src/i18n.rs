//! Externalized UI strings (spec "UI language"). English only for now; every
//! user-facing string goes through [`tr`], so translations can be added as tables.

/// The English string table: (key, text). Keys are stable identifiers.
const EN: &[(&str, &str)] = &[
    ("app.title", "Gusset"),
    ("menu.file", "File"),
    ("menu.new", "New"),
    ("menu.open", "Open…"),
    ("menu.save", "Save"),
    ("menu.save_as", "Save As…"),
    ("menu.export", "Export"),
    ("menu.quit", "Quit"),
    ("menu.edit", "Edit"),
    ("menu.undo", "Undo"),
    ("menu.redo", "Redo"),
    ("menu.view", "View"),
    ("menu.theme_dark", "Dark theme"),
    ("menu.theme_light", "Light theme"),
    ("menu.code", "Code view"),
    ("menu.fullscreen", "Full-screen viewport"),
    ("menu.help", "Help"),
    ("menu.diagnostics", "Diagnostics"),
    ("workspace.design", "Design"),
    ("tab.solid", "Solid"),
    ("tab.sketch", "Sketch"),
    ("tab.inspect", "Inspect"),
    ("tab.iterations", "Iterations"),
    ("browser.title", "Browser"),
    ("browser.features", "Features"),
    ("browser.parameters", "Parameters"),
    ("browser.bodies", "Bodies"),
    ("browser.add_param", "Add parameter"),
    ("timeline.title", "Timeline"),
    ("timeline.rollback", "Drag the marker to roll back"),
    ("timeline.expand", "Show iteration graph"),
    ("timeline.collapse", "Hide iteration graph"),
    ("search.hint", "Search commands, parameters, features… (Ctrl+K)"),
    ("search.none", "No matches"),
    ("dialog.ok", "OK"),
    ("dialog.cancel", "Cancel"),
    ("dialog.more", "More"),
    ("dialog.apply", "Apply"),
    ("close.title", "Save changes before closing?"),
    ("close.body", "These projects have unsaved changes:"),
    ("close.save", "Save and close"),
    ("close.discard", "Close without saving"),
    ("close.save_failed", "Saving failed. The project stays open."),
    ("close.save_as", "Save As…"),
    ("recovery.title", "Recover unsaved work?"),
    ("recovery.body", "Gusset did not close cleanly. A newer recovery copy of this project exists."),
    ("recovery.recover", "Recover unsaved work"),
    ("recovery.open_saved", "Open last saved version"),
    ("recovery.keep_both", "Keep both"),
    ("damaged.title", "The project file is damaged"),
    ("empty.title", "Start your first part"),
    ("empty.sketch", "New sketch on XY"),
    ("empty.import", "Import a STEP file"),
    ("empty.code", "Open the code view"),
    ("empty.hint", "Tip: press Ctrl+K to find any command."),
    ("status.saved", "All changes saved"),
    ("status.unsaved", "Unsaved changes"),
    ("status.rebuild_errors", "Rebuild errors"),
    ("status.agent", "Agent activity"),
    ("status.working", "Working…"),
    ("code.title", "Code"),
    ("code.apply", "Apply code"),
    ("code.revert", "Revert"),
    ("error.fix", "Suggested fix"),
    ("agent.branch", "Agent branch"),
    ("dialog.close", "Close"),
    ("agents.title", "AI agents"),
    ("agents.listening", "Agents can connect at"),
    ("agents.not_running", "The agent server is not running:"),
    ("agents.not_started", "The agent server is not started."),
    ("agents.how", "Connect Claude Code to this window:"),
    ("agents.how_stdio", "Or through the stdio bridge:"),
    ("agents.show_token", "Show token"),
    ("agents.copy", "Copy command"),
    ("agents.copied", "Connect command copied (it contains the token: keep it private)"),
    ("agents.read_only", "Read-only agents"),
    ("agents.read_only_help", "Agents can look but cannot change anything"),
    ("agents.revoke", "Revoke token"),
    ("agents.revoke_help", "Disconnects every agent at once; they need the new token to reconnect"),
    ("agents.revoked", "Token revoked: all agents were disconnected"),
    ("agents.branches", "Agent branches"),
    ("agents.no_branches", "No agent has made changes yet."),
    ("agents.yours", "you are here"),
    ("agents.switch", "Switch to"),
    ("agents.compare", "Compare"),
    ("agents.apply", "Apply"),
    ("agents.apply_help", "Copy this branch's added and changed features and parameters onto your branch (undoable)"),
    ("agents.compared", "Your branch compared with"),
    ("agents.no_diff", "No differences"),
    ("agents.audit", "Agent actions"),
    ("agents.no_audit", "No agent actions yet."),
    ("approval.title", "An AI agent asks for approval"),
    ("approval.agent", "Agent:"),
    ("approval.action", "Action"),
    ("approval.reason", "Why approval is needed"),
    ("approval.arguments", "Arguments"),
    ("approval.timeout", "Denied automatically in"),
    ("approval.queued", "more request(s) waiting"),
    ("approval.approve", "Approve"),
    ("approval.deny", "Deny"),
];

/// Looks up a UI string; unknown keys return the key itself (visible in tests).
pub fn tr(key: &str) -> &str {
    EN.iter().find(|(k, _)| *k == key).map_or(key, |(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_resolve() {
        let mut keys: Vec<&str> = EN.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(n, keys.len(), "duplicate i18n keys");
        assert_eq!(tr("menu.file"), "File");
        assert_eq!(tr("no.such.key"), "no.such.key");
    }
}
