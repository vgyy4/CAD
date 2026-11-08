//! `feature.bake`: history-free modelling (DESIGN.md later tier). The model's bodies, as they
//! are now, become exact B-rep imports and the features that made them go: what follows is
//! direct editing (move, offset or delete faces, fillets, cuts) on dumb solids. Sketches and
//! datum planes that stand on base or datum planes stay, with the CAM jobs that use them;
//! parameters stay. One undo brings the history back, and the branch keeps it.

use super::*;
use gusset_doc::{ImportDef, ImportFormat, PlaneAxis, PlaneKind, PlaneRef};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<Bake>()]
}

/// Replace the feature history with the bodies it makes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bake {
    /// Must be true.
    pub confirm: bool,
}

/// A feature that does not depend on bodies and is kept by a bake.
fn kept(f: &Feature, kept_so_far: &[String]) -> bool {
    let free = |p: &PlaneRef| match p {
        PlaneRef::XY | PlaneRef::XZ | PlaneRef::YZ => true,
        PlaneRef::Face(_) => false,
        PlaneRef::Datum(d) => kept_so_far.contains(d),
    };
    match &f.kind {
        FeatureKind::Sketch(s) => free(&s.plane),
        FeatureKind::Plane(p) => free(&p.base) && !matches!(&p.kind, PlaneKind::Angle { axis: PlaneAxis::Edge(_), .. }),
        FeatureKind::CamJob(j) => kept_so_far.contains(&j.sketch),
        _ => false,
    }
}

impl Command for Bake {
    const ID: &'static str = "feature.bake";
    const TITLE: &'static str = "Bake history";
    const DESCRIPTION: &'static str =
        "History-free: the bodies become exact imports; features that made them are removed (undoable).";
    const CATEGORY: Category = Category::Timeline;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if !self.confirm {
            return Err(CommandError::args("baking removes the feature history: pass confirm: true")
                .hint("history_undo brings it back"));
        }
        let ev = s.evaluate(&ctx.actor)?;
        if !ev.errors().is_empty() {
            return Err(CommandError::args("the model has rebuild errors: fix them before baking")
                .hint("query_model lists them"));
        }
        if ev.bodies.is_empty() {
            return Err(CommandError::args("there are no bodies to bake"));
        }
        let k = s.kernel().clone();
        let mut blobs = Vec::new();
        for b in &ev.bodies {
            let bytes = k.write_native(&b.shape).map_err(|e| CommandError::new("evaluation", e.to_string()))?;
            blobs.push((b.name.clone(), s.put_blob(&bytes).to_hex()));
        }
        edit(s, ctx, "Bake history", |doc, rb| {
            if rb.is_some() {
                return Err(CommandError::args("the timeline is rolled back: move the marker to the end first")
                    .hint("timeline_set_rollback with no position"));
            }
            let mut keep: Vec<String> = Vec::new();
            let mut out = Vec::new();
            let mut dropped = 0usize;
            for f in std::mem::take(&mut doc.features) {
                if kept(&f, &keep) {
                    keep.push(f.name.clone());
                    out.push(f);
                } else {
                    dropped += 1;
                }
            }
            doc.features = out;
            let mut made = Vec::new();
            for (body, blob) in &blobs {
                // The import keeps the body's name where it is free (ids, not user text).
                let name = if doc.feature(body).is_none() && validate_feature_name(body).is_ok() {
                    body.clone()
                } else {
                    doc.unique_feature_name("Baked")
                };
                let mut f = Feature::new(
                    name.clone(),
                    FeatureKind::Import(ImportDef {
                        blob: blob.clone(),
                        format: ImportFormat::Brep,
                        file_name: "baked".into(),
                    }),
                );
                f.label = Some(format!("baked {body}").chars().take(200).collect());
                doc.features.push(f);
                made.push(name);
            }
            Ok(json!({ "bodies": made, "removed_features": dropped, "kept_features": keep }))
        })
    }
}
