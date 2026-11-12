//! Whether an import brings a native file's feature history (ADR 0042 amendment, "history
//! choice"). Parsing classifies the file's history:
//!
//! - **perfect**: the features rebuild the shapes the file stores (the strict checks of
//!   `rebuild_check`, unchanged);
//! - **imperfect**: the history was read, but some features fail or the result differs from
//!   the stored shapes (the reason says which feature or by how much);
//! - **none**: the history cannot be read at all (the reason says why).
//!
//! The `history` argument of `import.file` then decides: `final` (the stored shapes), `rebuild`
//! (the features even when imperfect; features that fail stay in the timeline suppressed, with
//! their error as a note), `ask` (report the class and import nothing while there is a choice)
//! or, by default, today's behaviour: the features only when perfect.

use super::PreparedImport;
use crate::tree_import::Tree;
use serde_json::{Value, json};

/// How an import treats the feature history of a native CAD file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum HistoryChoice {
    #[default]
    Auto,
    Final,
    Rebuild,
    Ask,
}

/// What parsing found about a file's feature history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryClass {
    /// The features rebuild the file's own shapes.
    Perfect,
    /// The features were read but some fail or the result differs from the file.
    Imperfect,
    /// The history cannot be read.
    None,
}

impl HistoryClass {
    /// The class as the import's result names it.
    #[must_use]
    pub fn keyword(self) -> &'static str {
        match self {
            HistoryClass::Perfect => "perfect",
            HistoryClass::Imperfect => "imperfect",
            HistoryClass::None => "none",
        }
    }
}

/// A file's history class and, unless perfect, why (untrusted text from the file included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryInfo {
    pub class: HistoryClass,
    pub why: String,
}

/// Tier note for a history imported although it differs from the file.
pub const TIER_PARTIAL_HISTORY: &str =
    "partial history: the file's features as editable features, differing from the file's own shapes";

/// The parsed history, beside the stored shapes in a [`PreparedImport`].
#[derive(Debug, Clone)]
pub(super) struct History {
    pub(super) info: HistoryInfo,
    /// Perfect: the checked tree. Imperfect: the tree as read (its first form).
    tree: Option<Tree>,
    /// The tier of a perfect history.
    tier: &'static str,
    /// The warnings that hold when the features come in.
    tree_warnings: Vec<String>,
}

/// The history of a format whose files carry one that Gusset does not read.
pub(super) fn not_read(why: &str) -> History {
    History {
        info: HistoryInfo { class: HistoryClass::None, why: why.to_string() },
        tree: None,
        tier: "",
        tree_warnings: Vec::new(),
    }
}

/// Classifies the outcome of a `*_history` reader: `r` as it returns it (`Err("")` for a file
/// without a history), `attempt` the tree it tried when the check failed. A failure is also
/// said in `body_warnings` as before ("the design history came in as {what}: ...").
pub(super) fn classify(
    r: Result<(Tree, Option<String>), String>,
    attempt: Option<Tree>,
    tier: &'static str,
    mut tree_warnings: Vec<String>,
    body_warnings: &mut Vec<String>,
    what: &str,
) -> History {
    let (class, why, tree) = match r {
        Ok((tree, note)) => {
            tree_warnings.extend(note);
            (HistoryClass::Perfect, String::new(), Some(tree))
        }
        Err(why) if why.is_empty() => (HistoryClass::None, "the file holds no feature history".to_string(), None),
        Err(why) => {
            body_warnings.push(format!("the design history came in as {what}: {why}"));
            let class = if attempt.is_some() { HistoryClass::Imperfect } else { HistoryClass::None };
            (class, readable(&why), attempt)
        }
    };
    History { info: HistoryInfo { class, why }, tree, tier, tree_warnings }
}

/// A rebuild check's reason with its first skipped feature as `name (reason)` (the check writes
/// it as JSON for the corpus tools).
fn readable(why: &str) -> String {
    let Some((head, rest)) = why.split_once("the first: ") else { return why.to_string() };
    let Ok(v) = serde_json::from_str::<Value>(rest) else { return why.to_string() };
    match (v.get("feature").and_then(Value::as_str), v.get("reason").and_then(Value::as_str)) {
        (Some(f), Some(r)) => format!("{head}the first: {f} ({r})"),
        _ => why.to_string(),
    }
}

/// What to commit for a prepared import.
pub(super) enum Plan {
    /// `ask` with a choice to make: the result to return (nothing is imported).
    Ask(Value),
    /// The import to commit (its tree set when the history comes in) and the `history` entry
    /// of its result.
    Commit(Box<PreparedImport>, Option<Value>),
}

/// The options a class offers, the default first.
fn options(class: HistoryClass) -> &'static [&'static str] {
    match class {
        HistoryClass::Perfect => &["rebuild", "final"],
        HistoryClass::Imperfect => &["final", "rebuild"],
        HistoryClass::None => &["final"],
    }
}

/// Applies the import's history choice.
pub(super) fn plan(mut p: PreparedImport) -> Plan {
    let Some(h) = p.history.take() else { return Plan::Commit(Box::new(p), None) };
    let class = h.info.class;
    let choice = p.choice;
    let rebuild = match (choice, class) {
        (_, HistoryClass::None) => false,
        (HistoryChoice::Ask, _) => {
            return Plan::Ask(json!({
                "choice_required": "history",
                "history": { "class": class.keyword(), "why_user_text": h.info.why, "options": options(class) },
                "features": [],
                "hint": "import again with history: rebuild (the features; one undo step) or final (the file's shapes)",
            }));
        }
        (HistoryChoice::Auto, HistoryClass::Perfect) | (HistoryChoice::Rebuild, _) => true,
        (HistoryChoice::Auto | HistoryChoice::Final, _) => false,
    };
    let mut imported = "final";
    match h.tree {
        Some(mut tree) if rebuild => {
            imported = "history";
            if class == HistoryClass::Imperfect {
                tree.keep_failed = true;
                p.tier = TIER_PARTIAL_HISTORY;
                let mut w = h.tree_warnings;
                w.push(format!(
                    "the model differs from the file ({}); features that fail are kept suppressed with their error; undo takes the whole import back",
                    h.info.why
                ));
                p.warnings = w;
            } else {
                p.tier = h.tier;
                p.warnings = h.tree_warnings;
            }
            p.tree = Some(tree);
        }
        _ if choice == HistoryChoice::Rebuild => {
            p.warnings.push(format!("the history cannot be imported ({}): only the final shape came in", h.info.why));
        }
        _ => {}
    }
    let report = json!({ "class": class.keyword(), "why_user_text": h.info.why, "imported": imported });
    Plan::Commit(Box::new(p), Some(report))
}
