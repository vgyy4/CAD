//! Components (ADR 0026 §7): bodies of other Gusset documents, pinned to a version.

use super::*;
use gusset_doc::ComponentDef;
use gusset_history::{Hash, History};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<InsertComponent>(), spec::<UpdateComponent>()]
}

/// How deep the documents a component brings along may nest.
const MAX_NESTING: u32 = 8;

/// A source document read for a component: its history and the chosen version.
struct Source {
    history: History,
    node: gusset_history::NodeId,
    doc: Document,
}

fn read_source(s: &Session, ctx: &CmdCtx, path: &str, branch: Option<&str>) -> Result<Source, CommandError> {
    let full = crate::paths::read_target(s.project_dir.as_deref(), path, &ctx.actor)?;
    let shown = crate::paths::display_name(path);
    if full.extension().and_then(|e| e.to_str()).is_none_or(|e| !e.eq_ignore_ascii_case("gus")) {
        return Err(
            CommandError::args(format!("'{shown}' is not a Gusset document")).hint("components come from .gus files")
        );
    }
    let file = crate::import::open_regular(&full, path, crate::import::MAX_IMPORT_BYTES)?;
    let bytes = crate::import::read_capped(file, crate::import::MAX_IMPORT_BYTES, &Default::default())?;
    let (history, _) = gusset_project::decode(&bytes)
        .map_err(|e| CommandError::new("io", format!("'{shown}' cannot be read: {e}")))?;
    let b = match branch {
        Some(n) => history
            .branch_by_name(n)
            .map(|b| b.id)
            .ok_or_else(|| CommandError::not_found(format!("'{shown}' has no branch '{n}'")))?,
        None => history.main(),
    };
    let node = history.head(b).map_err(CommandError::history)?;
    let doc = history.document(node).map_err(CommandError::history)?;
    Ok(Source { history, node, doc })
}

/// Copies every blob `doc` needs (imported files, the documents of nested components and
/// what they need) from `from` into the session's project.
fn copy_blobs(s: &mut Session, from: &History, doc: &Document, depth: u32) -> Result<(), CommandError> {
    if depth > MAX_NESTING {
        return Err(CommandError::args(format!("components are nested more than {MAX_NESTING} deep")));
    }
    let get = |hex: &str| {
        Hash::from_hex(hex).and_then(|h| from.objects().get(h).ok().cloned()).ok_or_else(|| {
            CommandError::new("io", "the source document is missing stored data").hint("open and save it in Gusset")
        })
    };
    for f in &doc.features {
        match &f.kind {
            FeatureKind::Import(i) => {
                s.put_blob(&get(&i.blob)?);
            }
            FeatureKind::Component(c) => {
                let bytes = get(&c.document)?;
                let inner =
                    std::str::from_utf8(&bytes).ok().and_then(|t| Document::from_text(t).ok()).ok_or_else(|| {
                        CommandError::new("io", format!("component '{}' of the source cannot be read", f.name))
                    })?;
                copy_blobs(s, from, &inner, depth + 1)?;
                s.put_blob(&bytes);
            }
            _ => {}
        }
    }
    Ok(())
}

/// The body `body` (default: the last one) of `doc`, checked before pinning.
fn check_body(doc: &Document, body: Option<&str>, shown: &str) -> Result<(), CommandError> {
    let names: Vec<&str> = doc.features.iter().filter_map(created_body_name).collect();
    let ok = match body {
        Some(b) => names.contains(&b),
        None => !names.is_empty(),
    };
    if ok {
        return Ok(());
    }
    let what = body.map_or_else(|| "has no bodies".to_string(), |b| format!("has no body '{b}'"));
    Err(CommandError::not_found(format!("'{shown}' {what}")).hint(format!("its bodies: {}", names.join(", "))))
}

/// Pins `src` into the project: its blobs, then its text. Returns the document hash.
fn pin(s: &mut Session, src: &Source) -> Result<String, CommandError> {
    copy_blobs(s, &src.history, &src.doc, 0)?;
    Ok(s.put_blob(src.doc.to_text().as_bytes()).to_hex())
}

/// A feature name made from a file name: letters, digits and `_`, starting with a capital
/// letter (`Component` when nothing is left).
fn component_name(stem: &str) -> String {
    let kept: String = stem.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    let mut chars = kept.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase().to_string() + chars.as_str(),
        _ => "Component".to_string(),
    }
}

/// Brings in a body of another document.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InsertComponent {
    /// The .gus file, relative to the project folder (agents) or any path (the user).
    pub path: String,
    /// Its body to bring in (default: its last body).
    #[serde(default)]
    pub body: Option<String>,
    /// Its branch (default: main).
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for InsertComponent {
    const ID: &'static str = "assembly.insert_component";
    const TITLE: &'static str = "Insert component";
    const DESCRIPTION: &'static str = "Bring in a body of another .gus document as a component, pinned to that document's current version (a stored copy), at its own coordinates; place it with feature_joint. Faces are named <Name>/part(<face in that document>).";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let src = read_source(s, ctx, &self.path, self.branch.as_deref())?;
        check_body(&src.doc, self.body.as_deref(), &crate::paths::display_name(&self.path))?;
        let document = pin(s, &src)?;
        let def = ComponentDef { source: self.path.clone(), version: src.node.to_string(), document, body: self.body };
        let stem = std::path::Path::new(&self.path).file_stem().and_then(|n| n.to_str()).unwrap_or_default();
        let base = component_name(stem);
        edit(s, ctx, "Insert component", |doc, rb| {
            // The file's name when it is free, else a numbered one.
            let name = match self.name {
                None if doc.feature(&base).is_none() => base,
                given => feature_name(doc, given, &base)?,
            };
            let version = def.version.clone();
            insert_feature(doc, rb, Feature::new(name.clone(), FeatureKind::Component(def)));
            Ok(json!({ "feature": name, "version": version }))
        })
    }
}

/// Pins a component to its source's newest version.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateComponent {
    /// The component feature.
    pub component: String,
}

impl Command for UpdateComponent {
    const ID: &'static str = "assembly.update_component";
    const TITLE: &'static str = "Update component";
    const DESCRIPTION: &'static str = "Read a component's source document again and pin its current version (main branch). Reports whether anything changed.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let doc = s.document(&ctx.actor)?;
        let old = match doc.feature(&self.component).map(|f| &f.kind) {
            Some(FeatureKind::Component(d)) => d.clone(),
            Some(_) => return Err(CommandError::args(format!("'{}' is not a component", self.component))),
            None => return Err(CommandError::not_found(format!("no feature '{}'", self.component))),
        };
        let src = read_source(s, ctx, &old.source, None)?;
        check_body(&src.doc, old.body.as_deref(), &crate::paths::display_name(&old.source))?;
        let document = pin(s, &src)?;
        if document == old.document {
            return Ok(json!({ "feature": self.component, "changed": false, "version": old.version }));
        }
        let version = src.node.to_string();
        edit(s, ctx, "Update component", |doc, _| {
            if let Some(FeatureKind::Component(d)) =
                doc.features.iter_mut().find(|f| f.name == self.component).map(|f| &mut f.kind)
            {
                d.document = document;
                d.version = version.clone();
            }
            Ok(json!({ "feature": self.component, "changed": true, "version": version }))
        })
    }
}
