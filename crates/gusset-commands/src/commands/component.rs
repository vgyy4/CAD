//! Components (ADR 0026 §7): bodies of other Gusset documents, pinned to a version.

use super::*;
use gusset_doc::ComponentDef;
use gusset_history::{Hash, History};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<InsertComponent>(), spec::<UpdateComponent>(), spec::<ConfigureComponent>(), spec::<QueryComponents>()]
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
    crate::paths::refuse_remote(path)?;
    let full = crate::paths::read_target(s.project_dir.as_deref(), path, &ctx.actor)?;
    let shown = crate::paths::display_name(path);
    if full.extension().and_then(|e| e.to_str()).is_none_or(|e| !e.eq_ignore_ascii_case("gus")) {
        return Err(
            CommandError::args(format!("'{shown}' is not a Gusset document")).hint("components come from .gus files")
        );
    }
    let file = crate::import::open_regular(&full, path, crate::import::MAX_IMPORT_BYTES)?;
    // Stop cancels a slow read (review v0.3 H1).
    let bytes = crate::import::read_capped(file, crate::import::MAX_IMPORT_BYTES, &s.rebuild_cancel_flag())?;
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
/// `seen` holds the component documents already copied: each is parsed and walked once
/// however often it is used (review v0.3 H3).
fn copy_blobs(
    s: &mut Session,
    from: &History,
    doc: &Document,
    depth: u32,
    seen: &mut std::collections::BTreeSet<String>,
) -> Result<(), CommandError> {
    if depth > MAX_NESTING {
        return Err(CommandError::args(format!("components are nested more than {MAX_NESTING} deep")));
    }
    if s.rebuild_cancel_flag().load(std::sync::atomic::Ordering::Relaxed) {
        return Err(CommandError::new("cancelled", "inserting the component was cancelled"));
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
                if !seen.insert(c.document.clone()) {
                    continue;
                }
                let bytes = get(&c.document)?;
                let inner =
                    std::str::from_utf8(&bytes).ok().and_then(|t| Document::from_text(t).ok()).ok_or_else(|| {
                        CommandError::new("io", format!("component '{}' of the source cannot be read", f.name))
                    })?;
                copy_blobs(s, from, &inner, depth + 1, seen)?;
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

/// `set` parsed, after checking the parameters exist in the component's document.
fn assignments(
    doc: &Document,
    set: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<(String, gusset_units::expr::Expr)>, CommandError> {
    set.iter()
        .map(|(n, e)| {
            if !doc.params.iter().any(|p| p.name == *n) {
                let names: Vec<&str> = doc.params.iter().map(|p| p.name.as_str()).collect();
                return Err(CommandError::not_found(format!("the component has no parameter '{n}'"))
                    .hint(format!("its parameters: {}", names.join(", "))));
            }
            Ok((n.clone(), parse_expr(n, e)?))
        })
        .collect()
}

/// The stored document of component `d`.
fn stored_document(s: &Session, d: &ComponentDef) -> Result<Document, CommandError> {
    let h = Hash::from_hex(&d.document).ok_or_else(|| CommandError::args("the component's hash is invalid"))?;
    let bytes =
        s.history().objects().get(h).map_err(|_| CommandError::not_found("the component's document is missing"))?;
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|t| Document::from_text(t).ok())
        .ok_or_else(|| CommandError::new("io", "the component's document cannot be read"))
}

/// Pins `src` into the project: its blobs, then its text. Returns the document hash.
fn pin(s: &mut Session, src: &Source) -> Result<String, CommandError> {
    copy_blobs(s, &src.history, &src.doc, 0, &mut Default::default())?;
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
    /// Its parameters to set, as expressions of this document: {"teeth": "30"}.
    #[serde(default)]
    pub set: std::collections::BTreeMap<String, String>,
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
        let set = assignments(&src.doc, &self.set)?;
        let def = ComponentDef {
            source: self.path.clone(),
            version: src.node.to_string(),
            document,
            body: self.body,
            set,
            configuration: None,
        };
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

/// Sets or clears parameters of a component.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigureComponent {
    /// The component feature.
    pub component: String,
    /// Parameter -> expression of this document; null clears it (the component's own
    /// value again).
    #[serde(default)]
    pub set: std::collections::BTreeMap<String, Option<String>>,
    /// A configuration of the component's document, applied before `set` ("" clears it).
    #[serde(default)]
    pub configuration: Option<String>,
}

impl Command for ConfigureComponent {
    const ID: &'static str = "assembly.configure_component";
    const TITLE: &'static str = "Configure component";
    const DESCRIPTION: &'static str = "Set parameters of a component's document from this one ({\"teeth\": \"30\", \"w\": \"2 * a\"}; null restores its own value) and/or pick one of its configurations, e.g. two sizes of one part.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let doc = s.document(&ctx.actor)?;
        let Some(FeatureKind::Component(old)) = doc.feature(&self.component).map(|f| &f.kind) else {
            return Err(CommandError::not_found(format!("no component '{}'", self.component)));
        };
        let inner = stored_document(s, old)?;
        let configuration = match self.configuration.as_deref() {
            None => old.configuration.clone(),
            Some("") => None,
            Some(c) if inner.configurations.iter().any(|x| x.name == c) => Some(c.to_string()),
            Some(c) => {
                let have: Vec<&str> = inner.configurations.iter().map(|x| x.name.as_str()).collect();
                return Err(CommandError::not_found(format!("the component has no configuration '{c}'"))
                    .hint(format!("its configurations: {}", have.join(", "))));
            }
        };
        let mut set = old.set.clone();
        for (n, e) in &self.set {
            set.retain(|(m, _)| m != n);
            if let Some(e) = e {
                let one = std::collections::BTreeMap::from([(n.clone(), e.clone())]);
                set.extend(assignments(&inner, &one)?);
            }
        }
        edit(s, ctx, "Configure component", |doc, _| {
            if let Some(FeatureKind::Component(d)) =
                doc.features.iter_mut().find(|f| f.name == self.component).map(|f| &mut f.kind)
            {
                d.set = set;
                d.configuration = configuration;
            }
            Ok(json!({ "feature": self.component }))
        })
    }
}

/// Lists the components and whether their sources changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryComponents {
    /// Only read sources inside the project folder; others report `source_error` (the
    /// app's check after opening a project, which must not touch other files).
    #[serde(default)]
    pub local_only: bool,
}

impl Command for QueryComponents {
    const ID: &'static str = "query.components";
    const TITLE: &'static str = "Components";
    const DESCRIPTION: &'static str = "Every component: source, pinned version, body, parameters set, and whether the source now has a newer version (update_available; null when the source cannot be read, with the reason).";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let doc = s.document(&ctx.actor)?;
        let mut out = Vec::new();
        // Each source is read once, however many components name it (review v0.3 H1).
        let mut read: std::collections::BTreeMap<&str, Result<String, String>> = std::collections::BTreeMap::new();
        for f in &doc.features {
            let FeatureKind::Component(d) = &f.kind else { continue };
            let text = read.entry(d.source.as_str()).or_insert_with(|| {
                if self.local_only {
                    crate::paths::confined(s.project_dir.as_deref(), &d.source)
                        .map_err(|_| "not checked: the source is outside the project folder".to_string())?;
                }
                read_source(s, ctx, &d.source, None).map(|src| src.doc.to_text()).map_err(|e| e.message)
            });
            let (available, error) = match text {
                Ok(t) => (json!(*t != stored_text(s, d)), Value::Null),
                Err(e) => (Value::Null, json!(e)),
            };
            let set: serde_json::Map<String, Value> =
                d.set.iter().map(|(n, e)| (n.clone(), json!(e.to_string()))).collect();
            out.push(json!({
                "name": f.name, "source": d.source, "version": d.version, "body": d.body,
                "set": set, "update_available": available, "source_error": error,
            }));
        }
        Ok(json!({ "components": out }))
    }
}

/// The text stored for component `d` (empty when missing).
fn stored_text(s: &Session, d: &ComponentDef) -> String {
    Hash::from_hex(&d.document)
        .and_then(|h| s.history().objects().get(h).ok())
        .and_then(|b| std::str::from_utf8(b).ok().map(str::to_string))
        .unwrap_or_default()
}
