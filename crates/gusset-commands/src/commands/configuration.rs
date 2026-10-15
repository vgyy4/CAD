//! Configurations (ADR 0027 §3): named parameter presets stored in the document.

use super::*;
use gusset_doc::Configuration;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        spec::<SaveConfiguration>(),
        spec::<ApplyConfiguration>(),
        spec::<DeleteConfiguration>(),
        spec::<QueryConfigurations>(),
    ]
}

/// Saves the current values of parameters as a named configuration.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveConfiguration {
    pub name: String,
    /// Parameters it sets (default: all).
    #[serde(default)]
    pub params: Option<Vec<String>>,
}

impl Command for SaveConfiguration {
    const ID: &'static str = "configuration.save";
    const TITLE: &'static str = "Save configuration";
    const DESCRIPTION: &'static str =
        "Save the current values of parameters (default: all) as a named configuration; replaces one of that name.";
    const CATEGORY: Category = Category::Parameters;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        gusset_doc::validate::validate_feature_name(&self.name).map_err(CommandError::args)?;
        edit(s, ctx, &format!("Save configuration {}", self.name), |doc, _| {
            let mut names = self.params.clone().unwrap_or_else(|| doc.params.iter().map(|p| p.name.clone()).collect());
            // A parameter listed twice is saved once (review v0.3 H2).
            let mut seen = std::collections::BTreeSet::new();
            names.retain(|n| seen.insert(n.clone()));
            if names.is_empty() {
                return Err(CommandError::args("there are no parameters to save").hint("add parameters first"));
            }
            let mut set = Vec::with_capacity(names.len());
            for n in &names {
                let p = doc
                    .params
                    .iter()
                    .find(|p| p.name == *n)
                    .ok_or_else(|| CommandError::not_found(format!("no parameter '{n}'")))?;
                set.push((n.clone(), p.expr.clone()));
            }
            let c = Configuration { name: self.name.clone(), set };
            match doc.configurations.iter_mut().find(|c| c.name == self.name) {
                Some(old) => *old = c,
                None => doc.configurations.push(c),
            }
            Ok(json!({ "configuration": self.name, "params": names }))
        })
    }
}

/// Sets the parameters to a configuration's values.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyConfiguration {
    pub name: String,
}

impl Command for ApplyConfiguration {
    const ID: &'static str = "configuration.apply";
    const TITLE: &'static str = "Apply configuration";
    const DESCRIPTION: &'static str = "Set the parameters to a configuration's values (one undoable edit).";
    const CATEGORY: Category = Category::Parameters;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        edit(s, ctx, &format!("Apply configuration {}", self.name), |doc, _| {
            let c = doc
                .configurations
                .iter()
                .find(|c| c.name == self.name)
                .cloned()
                .ok_or_else(|| CommandError::not_found(format!("no configuration '{}'", self.name)))?;
            for (n, e) in &c.set {
                if let Some(p) = doc.params.iter_mut().find(|p| p.name == *n) {
                    p.expr = e.clone();
                }
            }
            let values = params::evaluate(&doc.params);
            for (n, _) in &c.set {
                if let Some(Err(e)) = values.get(n) {
                    return Err(CommandError::args(format!("{n}: {e}")).hint("edit the configuration's values"));
                }
            }
            Ok(json!({ "configuration": self.name, "set": c.set.len() }))
        })
    }
}

/// Removes a configuration.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteConfiguration {
    pub name: String,
}

impl Command for DeleteConfiguration {
    const ID: &'static str = "configuration.delete";
    const TITLE: &'static str = "Delete configuration";
    const DESCRIPTION: &'static str = "Delete a configuration (parameters keep their values; undoable).";
    const CATEGORY: Category = Category::Parameters;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        edit(s, ctx, &format!("Delete configuration {}", self.name), |doc, _| {
            let n = doc.configurations.len();
            doc.configurations.retain(|c| c.name != self.name);
            if n == doc.configurations.len() {
                return Err(CommandError::not_found(format!("no configuration '{}'", self.name)));
            }
            Ok(json!({}))
        })
    }
}

/// Lists the configurations and which one the current values match.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryConfigurations {}

impl Command for QueryConfigurations {
    const ID: &'static str = "query.configurations";
    const TITLE: &'static str = "Configurations";
    const DESCRIPTION: &'static str =
        "Every configuration with the values it sets, and `current`: whether the parameters have those values now.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let doc = s.document(&ctx.actor)?;
        let list: Vec<Value> = doc
            .configurations
            .iter()
            .map(|c| {
                let set: serde_json::Map<String, Value> =
                    c.set.iter().map(|(n, e)| (n.clone(), json!(e.to_string()))).collect();
                let current = c.set.iter().all(|(n, e)| doc.params.iter().any(|p| p.name == *n && p.expr == *e));
                json!({ "name": c.name, "set": set, "current": current })
            })
            .collect();
        Ok(json!({ "configurations": list }))
    }
}
