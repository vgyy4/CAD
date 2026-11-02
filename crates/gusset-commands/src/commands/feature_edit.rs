//! `feature.edit`: changes arguments of an existing feature in place (DESIGN.md §1.5,
//! "edit in context"). The feature's one-line statement (as in the code view) is split into
//! its named arguments, the given ones are replaced, added or removed, and the statement is
//! parsed back; the name, kind and place in the history stay. Sketches are edited with the
//! sketch commands instead.

use super::*;
use gusset_doc::text::{parse_feature_object, statement_args, statement_with_args};
use std::collections::BTreeMap;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<FeatureEdit>()]
}

/// Change arguments of an existing feature in place.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureEdit {
    pub name: String,
    /// Arguments to set, as in the code view (`{"distance": "15 mm"}`).
    #[serde(default)]
    pub set: BTreeMap<String, String>,
    /// Arguments to remove (back to their defaults).
    #[serde(default)]
    pub remove: Vec<String>,
}

impl Command for FeatureEdit {
    const ID: &'static str = "feature.edit";
    const TITLE: &'static str = "Edit feature";
    const DESCRIPTION: &'static str =
        "Change arguments of an existing feature in place; values as in the code view. Not for sketches.";
    const CATEGORY: Category = Category::Timeline;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let name = self.name.clone();
        edit(s, ctx, &format!("Edit {name}"), |doc, _| {
            let i = doc.feature_index(&name).ok_or_else(|| CommandError::not_found(format!("no feature '{name}'")))?;
            let f = doc.features.get(i).ok_or_else(|| CommandError::not_found(format!("no feature '{name}'")))?;
            let (head, mut args) = statement_args(f).ok_or_else(|| {
                CommandError::args(format!("{name} is not a one-line feature"))
                    .hint("edit sketches with the sketch commands (sketch_add_constraint, sketch_move_point, ...)")
            })?;
            for k in &self.remove {
                args.retain(|(key, _)| key != k);
            }
            let ident =
                |k: &str| !k.is_empty() && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if let Some(k) = self.set.keys().chain(&self.remove).find(|k| !ident(k)) {
                return Err(CommandError::args(format!("'{k}' is not an argument name")));
            }
            for (k, v) in &self.set {
                if v.contains(['\n', '\r']) {
                    return Err(CommandError::args(format!("'{k}' must be one line")));
                }
                match args.iter_mut().find(|(key, _)| key == k) {
                    Some(slot) => slot.1.clone_from(v),
                    None => args.push((k.clone(), v.clone())),
                }
            }
            let text = statement_with_args(&head, &args);
            let new = parse_feature_object(&text).map_err(|e| CommandError::args(format!("{}: {e}", text.trim())))?;
            if new.name != f.name || std::mem::discriminant(&new.kind) != std::mem::discriminant(&f.kind) {
                return Err(CommandError::args("the name and kind of a feature cannot change"));
            }
            if let Some(slot) = doc.features.get_mut(i) {
                *slot = new;
            }
            Ok(json!({ "feature": name, "statement": text.trim() }))
        })
    }
}
