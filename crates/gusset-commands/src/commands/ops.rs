//! Shell, draft, mirror and pattern commands (v0.2). They change an existing body, so the target
//! is resolved to a concrete body name when the feature is created (like add/cut).

use super::*;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        spec::<Shell>(),
        spec::<Draft>(),
        spec::<OffsetFace>(),
        spec::<DeleteFace>(),
        spec::<MoveFace>(),
        spec::<Mirror>(),
        spec::<PatternLinear>(),
        spec::<PatternCircular>(),
    ]
}

/// Renames references to feature `from` inside the new feature kinds.
pub(super) fn rename_refs(kind: &mut FeatureKind, from: &str, to: &str) {
    let body_of = |body: &mut Option<String>| {
        if body.as_deref() == Some(from) {
            *body = Some(to.to_string());
        }
    };
    let sketch_of = |s: &mut String| {
        if s == from {
            *s = to.to_string();
        }
    };
    match kind {
        FeatureKind::Sweep(d) => {
            sketch_of(&mut d.profile.sketch);
            match &mut d.path {
                PathRef::Sketch { sketch, .. } => sketch_of(sketch),
                PathRef::Edges(edges) => edges.iter_mut().for_each(|e| e.rename_feature(from, to)),
                PathRef::Helix(h) => {
                    if let gusset_doc::AxisRef::SketchLine { sketch, .. } = &mut h.axis {
                        sketch_of(sketch);
                    }
                }
            }
            body_of(&mut d.body);
        }
        FeatureKind::Loft(d) => {
            d.profiles.iter_mut().for_each(|p| sketch_of(&mut p.sketch));
            body_of(&mut d.body);
        }
        FeatureKind::Shell(d) => {
            body_of(&mut d.body);
            for f in &mut d.faces {
                f.rename_feature(from, to);
            }
        }
        FeatureKind::Mirror(d) => {
            body_of(&mut d.body);
            d.plane.rename_feature(from, to);
        }
        FeatureKind::DeleteFace(d) => {
            body_of(&mut d.body);
            for f in &mut d.faces {
                f.rename_feature(from, to);
            }
        }
        FeatureKind::OffsetFace(d) => {
            body_of(&mut d.body);
            for f in &mut d.faces {
                f.rename_feature(from, to);
            }
        }
        FeatureKind::MoveFace(d) => {
            body_of(&mut d.body);
            for f in d.faces.iter_mut().chain(d.onto.as_mut()) {
                f.rename_feature(from, to);
            }
        }
        FeatureKind::Text(d) => {
            body_of(&mut d.body);
            d.plane.rename_feature(from, to);
        }
        FeatureKind::Draft(d) => {
            body_of(&mut d.body);
            for f in &mut d.faces {
                f.rename_feature(from, to);
            }
            d.plane.rename_feature(from, to);
        }
        FeatureKind::PatternLinear(d) => {
            body_of(&mut d.body);
            d.features.iter_mut().for_each(sketch_of);
        }
        FeatureKind::PatternCircular(d) => {
            body_of(&mut d.body);
            d.features.iter_mut().for_each(sketch_of);
        }
        FeatureKind::Plane(d) => {
            d.base.rename_feature(from, to);
            if let PlaneKind::Angle { axis: PlaneAxis::Edge(e), .. } = &mut d.kind {
                e.rename_feature(from, to);
            }
        }
        FeatureKind::Move(d) => body_of(&mut d.body),
        FeatureKind::Scale(d) => body_of(&mut d.body),
        FeatureKind::Remove(d) => sketch_of(&mut d.body),
        FeatureKind::CamJob(d) => sketch_of(&mut d.sketch),
        FeatureKind::Hole(d) => super::hole::rename_refs(d, from, to),
        FeatureKind::Joint(d) => d.rename_feature(from, to),
        FeatureKind::Combine(d) => {
            body_of(&mut d.body);
            for t in &mut d.tools {
                if t == from {
                    *t = to.to_string();
                }
            }
        }
        FeatureKind::Surface(d) => match &mut d.op {
            gusset_doc::SurfaceOp::Extrude { curves: PathRef::Sketch { sketch, .. }, .. } => sketch_of(sketch),
            gusset_doc::SurfaceOp::Extrude { .. } => {}
            gusset_doc::SurfaceOp::Patch { profile } => sketch_of(&mut profile.sketch),
            gusset_doc::SurfaceOp::Thicken { body, .. } => body_of(body),
            gusset_doc::SurfaceOp::Stitch { bodies, .. } => bodies.iter_mut().for_each(sketch_of),
        },
        FeatureKind::Sheet(d) => match &mut d.op {
            gusset_doc::SheetOp::Contour { curves: PathRef::Sketch { sketch, .. }, .. } => sketch_of(sketch),
            gusset_doc::SheetOp::Contour { .. } => {}
            gusset_doc::SheetOp::Tab { profile } => sketch_of(&mut profile.sketch),
            gusset_doc::SheetOp::EdgeFlange { body, .. } => sketch_of(body),
        },
        _ => {}
    }
}

/// Resolves the body to work on now and stores its name in the feature.
pub(crate) fn body_for(
    doc: &Document,
    rb: Option<usize>,
    explicit: Option<String>,
) -> Result<Option<String>, CommandError> {
    target_body(doc, rb, BodyOp::Add, explicit)
}

/// The body holding the first of `faces`, for commands given faces but no body (the latest
/// body is often another one: dogfooding F7-7).
fn owner_body(s: &mut Session, ctx: &CmdCtx, faces: &[String]) -> Result<Option<String>, CommandError> {
    let Some(first) = faces.first().and_then(|f| FaceRef::parse(f).ok()) else { return Ok(None) };
    let ev = s.evaluate(&ctx.actor)?;
    Ok(ev.bodies.iter().find(|b| b.names.face_index(&first).is_some()).map(|b| b.feature.clone()))
}

fn parse_faces(faces: &[String]) -> Result<Vec<FaceRef>, CommandError> {
    faces.iter().map(|f| FaceRef::parse(f).map_err(|e| CommandError::args(format!("face '{f}': {e}")))).collect()
}

/// Removes faces; the neighbours close the gap (ADR 0038).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteFace {
    /// Faces to remove (refs from query_body), e.g. a hole's wall or fillet.
    pub faces: Vec<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for DeleteFace {
    const ID: &'static str = "feature.delete_face";
    const TITLE: &'static str = "Delete face";
    const DESCRIPTION: &'static str =
        "Remove faces (holes, fillets, bosses); the faces around grow to close the gap. Needs OCCT.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let faces = parse_faces(&self.faces)?;
        if faces.is_empty() {
            return Err(CommandError::args("no faces to delete").hint("list the faces to remove"));
        }
        for f in &faces {
            check_face_resolves(s, ctx, f)?;
        }
        let owner = owner_body(s, ctx, &self.faces)?;
        edit(s, ctx, "Delete face", |doc, rb| {
            let name = feature_name(doc, self.name, "DeleteFace")?;
            let body = body_for(doc, rb, self.body.or(owner))?;
            let def = gusset_doc::DeleteFaceDef { body, faces };
            insert_feature(doc, rb, Feature::new(name.clone(), FeatureKind::DeleteFace(def)));
            Ok(json!({ "feature": name }))
        })
    }
}

/// Moves faces along their normals (ADR 0030).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OffsetFace {
    /// Faces to move (refs from query_body).
    pub faces: Vec<String>,
    /// Along the face normals (`2 mm`); negative moves them in.
    pub distance: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// false: only these (planar) faces move.
    #[serde(default)]
    pub rules: Option<bool>,
}

impl Command for OffsetFace {
    const ID: &'static str = "feature.offset_face";
    const TITLE: &'static str = "Offset face";
    const DESCRIPTION: &'static str = "Push or pull faces along their normals; neighbours follow. Needs OCCT.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let distance = parse_expr("distance", &self.distance)?;
        let faces = parse_faces(&self.faces)?;
        if faces.is_empty() {
            return Err(CommandError::args("no faces to move").hint("list the faces to push or pull"));
        }
        for f in &faces {
            check_face_resolves(s, ctx, f)?;
        }
        let owner = owner_body(s, ctx, &self.faces)?;
        let refs = faces.clone();
        // rules: false that the kernel cannot build (a tangent neighbour would have to change
        // shape) is tried first and refused, not committed broken (dogfooding F14-4).
        if self.rules == Some(false) {
            let added = std::cell::RefCell::new(String::new());
            return edit_checked(
                s,
                ctx,
                "Offset face",
                |doc, rb| {
                    let name = feature_name(doc, self.name, "OffsetFace")?;
                    let body = body_for(doc, rb, self.body.or(owner))?;
                    let def = OffsetFaceDef { body, faces, distance, rules: false };
                    insert_feature(doc, rb, Feature::new(name.clone(), FeatureKind::OffsetFace(def)));
                    added.replace(name.clone());
                    Ok(json!({ "feature": name, "also_moved": [] }))
                },
                |s, doc, _| {
                    let ev = s.evaluate_doc(doc, None);
                    let name = added.borrow();
                    match ev.errors().iter().find(|e| e.feature == *name && e.code != gusset_eval::ErrorCode::Unsupported) {
                        // Review R1: the tangent case has its own words, anything else its own.
                        Some(e) if e.message.contains("would have to change shape") => Err(CommandError::args(
                            "these faces cannot move alone: a neighbour tangent to them (a rounded corner) would have to change shape",
                        )
                        .hint("leave rules out to carry tangent faces along")),
                        Some(e) => Err(CommandError::args(e.message.clone())
                            .hint(e.hint.clone().unwrap_or_else(|| "move the faces less far".into()))),
                        None => Ok(()),
                    }
                },
                |_, _, _| Value::Null,
            );
        }
        let before = face_places(s, ctx, &refs);
        let mut r = edit(s, ctx, "Offset face", |doc, rb| {
            let name = feature_name(doc, self.name, "OffsetFace")?;
            let body = body_for(doc, rb, self.body.or(owner))?;
            let def = OffsetFaceDef { body, faces, distance, rules: self.rules.unwrap_or(true) };
            insert_feature(doc, rb, Feature::new(name.clone(), FeatureKind::OffsetFace(def)));
            Ok(json!({ "feature": name }))
        })?;
        // Faces that went along (tangent fillets and their neighbours; dogfooding F12-2): those
        // whose plane or axis moved, by name.
        if r.get("rebuild_ok") == Some(&json!(true)) {
            let after = face_places(s, ctx, &refs);
            let moved: Vec<&String> = before
                .iter()
                .filter(|(n, p)| after.get(*n).is_some_and(|q| !same_place(p, q)))
                .map(|(n, _)| n)
                .collect();
            if let Some(o) = r.as_object_mut() {
                o.insert("also_moved".into(), json!(moved));
            }
        }
        Ok(r)
    }
}

/// Where each face of the body holding `faces` lies, but `faces` themselves: a planar
/// face's normal and offset, a revolved face's axis; other faces are left out.
fn face_places(s: &mut Session, ctx: &CmdCtx, faces: &[FaceRef]) -> std::collections::BTreeMap<String, Place> {
    use gusset_kernel::SurfaceKind as K;
    let mut out = std::collections::BTreeMap::new();
    let Ok(ev) = s.evaluate(&ctx.actor) else { return out };
    let Some(b) = ev.bodies.iter().find(|b| faces.iter().any(|f| b.names.face_index(f).is_some())) else {
        return out;
    };
    let chosen: Vec<u32> = faces.iter().filter_map(|f| b.names.face_index(f)).collect();
    for ((i, n), f) in (0u32..).zip(&b.names.faces).zip(&b.topology.faces) {
        if chosen.contains(&i) {
            continue;
        }
        let place = match (f.kind, f.axis) {
            (K::Plane, _) => (f.normal, [gusset_kernel::dot(f.normal, f.centroid), 0.0, 0.0], 0.0),
            (K::Cylinder | K::Cone | K::Torus, Some(a)) => {
                // The axis line (its direction and the point on it nearest the origin) and how
                // far the face's centroid sits from it, which grows with the radius (a blend
                // that an offset makes larger keeps its axis).
                let t = gusset_kernel::dot(a.origin, a.dir);
                let foot = gusset_kernel::sub(a.origin, gusset_kernel::scale(a.dir, t));
                let rel = gusset_kernel::sub(f.centroid, a.origin);
                let off = gusset_kernel::sub(rel, gusset_kernel::scale(a.dir, gusset_kernel::dot(rel, a.dir)));
                (a.dir, foot, gusset_kernel::norm(off))
            }
            // Other surfaces (blends, offset surfaces): where they sit and how large they are.
            _ => (f.centroid, [0.0; 3], f.area),
        };
        out.insert(n.to_string(), (format!("{:?}", f.kind), place.0, place.1, place.2));
    }
    out
}

/// A face's surface kind and where it lies (see [`face_places`]).
type Place = (String, gusset_kernel::V3, gusset_kernel::V3, f64);

/// Same kind and place: a face whose kind changed (a cylinder became an offset surface)
/// moved too (dogfooding F13-2).
fn same_place(a: &Place, b: &Place) -> bool {
    let close = |p: gusset_kernel::V3, q: gusset_kernel::V3| gusset_kernel::norm(gusset_kernel::sub(p, q)) < 1e-6;
    a.0 == b.0 && close(a.1, b.1) && close(a.2, b.2) && (a.3 - b.3).abs() < 1e-6
}

/// Moves faces rigidly (ADR 0031).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveFace {
    /// Faces to move (refs from query_body).
    pub faces: Vec<String>,
    /// As in feature_move_body.
    #[serde(default)]
    pub translate: Option<[String; 3]>,
    #[serde(default)]
    pub axis: Option<String>,
    #[serde(default)]
    pub angle: Option<String>,
    #[serde(default)]
    pub through: Option<[String; 3]>,
    #[serde(default)]
    pub onto: Option<String>,
    /// Also move coplanar and coaxial faces (default true).
    #[serde(default)]
    pub rules: Option<bool>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for MoveFace {
    const ID: &'static str = "feature.move_face";
    const TITLE: &'static str = "Move face";
    const DESCRIPTION: &'static str =
        "Turn (first) and/or translate faces, or lay them onto a face's plane (onto); neighbours stretch or it fails.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let onto = match &self.onto {
            Some(f) => {
                if self.translate.is_some() || self.axis.is_some() || self.angle.is_some() {
                    return Err(CommandError::args("onto replaces translate and the turn: give one or the other"));
                }
                let r = parse_faces(std::slice::from_ref(f))?.into_iter().next();
                if let Some(r) = &r {
                    check_face_resolves(s, ctx, r)?;
                }
                r
            }
            None => None,
        };
        let (translate, rotation) = if onto.is_some() {
            (None, None)
        } else {
            super::datum::motion(&self.translate, &self.axis, &self.angle, &self.through)?
        };
        let faces = parse_faces(&self.faces)?;
        if faces.is_empty() {
            return Err(CommandError::args("no faces to move").hint("list the faces to move"));
        }
        for f in &faces {
            check_face_resolves(s, ctx, f)?;
        }
        if let Some(target) = &onto {
            // A target the move cannot reach is refused here, not committed as a failing
            // feature: planar, parallel, and not the face's own plane.
            let ev = s.evaluate(&ctx.actor)?;
            let plane = |r: &FaceRef| {
                ev.bodies.iter().find_map(|b| {
                    let f = b.topology.faces.get(b.names.face_index(r)? as usize)?;
                    matches!(f.kind, gusset_kernel::SurfaceKind::Plane).then_some((f.centroid, f.normal))
                })
            };
            let not_planar = |r: &FaceRef| {
                CommandError::args(format!("{r} is not a planar face"))
                    .hint("onto lays planar faces onto a planar face")
            };
            let (c2, n2) = plane(target).ok_or_else(|| not_planar(target))?;
            for f in &faces {
                let (c1, n1) = plane(f).ok_or_else(|| not_planar(f))?;
                if gusset_kernel::dot(n1, n2).abs() < 1e-6 {
                    return Err(CommandError::args(format!("{target} is perpendicular to {f}"))
                        .hint("onto turns a face at most short of a right angle; pick a face it can tilt onto"));
                }
                if gusset_kernel::norm(gusset_kernel::cross(n1, n2)) > 1e-9 {
                    // Tilted: turned about the line where the planes meet (evaluator).
                    continue;
                }
                if gusset_kernel::dot(gusset_kernel::sub(c2, c1), n2).abs() < 1e-9 {
                    return Err(CommandError::args(format!("{f} already lies in the plane of {target}")));
                }
                let same_body =
                    ev.bodies.iter().any(|b| b.names.face_index(f).is_some() && b.names.face_index(target).is_some());
                if same_body && gusset_kernel::dot(n1, n2) < 0.0 {
                    return Err(CommandError::args(format!(
                        "{target} faces {f} across the body: laying one onto the other leaves no material"
                    ))
                    .hint("lay a face onto a face that points the same way, or offset_face it"));
                }
            }
        }
        // Faces of one body only: a move edits one body (F8-12).
        {
            let ev = s.evaluate(&ctx.actor)?;
            let mut bodies: Vec<&str> = faces
                .iter()
                .filter_map(|f| ev.bodies.iter().find(|b| b.names.face_index(f).is_some()).map(|b| b.feature.as_str()))
                .collect();
            bodies.sort_unstable();
            bodies.dedup();
            if let [a, b, ..] = bodies.as_slice() {
                return Err(CommandError::args(format!("the faces are on more than one body ({a}, {b})"))
                    .hint("move the faces of each body in its own move_face"));
            }
        }
        let owner = owner_body(s, ctx, &self.faces)?;
        let rules = self.rules.unwrap_or(true);
        // Faces the rules add (reported; the evaluator re-derives them on every rebuild).
        let also_moved: Vec<String> = if rules {
            let ev = s.evaluate(&ctx.actor)?;
            let hit = ev.bodies.iter().find_map(|b| {
                let idx: Vec<u32> = faces.iter().filter_map(|f| b.names.face_index(f)).collect();
                (!idx.is_empty()).then_some((b, idx))
            });
            hit.map(|(b, idx)| {
                gusset_eval::rule_faces(&b.topology, &idx)
                    .iter()
                    .filter_map(|i| b.names.faces.get(*i as usize).map(ToString::to_string))
                    .collect()
            })
            .unwrap_or_default()
        } else {
            Vec::new()
        };
        let added = std::cell::RefCell::new(String::new());
        edit_checked(
            s,
            ctx,
            "Move face",
            |doc, rb| {
                let name = feature_name(doc, self.name, "MoveFace")?;
                let body = body_for(doc, rb, self.body.or(owner))?;
                let def = MoveFaceDef { body, faces, translate, rotation, onto, rules };
                insert_feature(doc, rb, Feature::new(name.clone(), FeatureKind::MoveFace(def)));
                added.replace(name.clone());
                Ok(if rules {
                    json!({ "feature": name, "also_moved": also_moved })
                } else {
                    json!({ "feature": name })
                })
            },
            |s, doc, _| {
                // A move the kernel cannot build (onto a plane across the body, faces whose
                // neighbours would have to change shape) is tried first and refused, rather
                // than committed as a failing feature (F7-8, F9-4).
                let ev = s.evaluate_doc(doc, None);
                let name = added.borrow();
                // A kernel without face moves (the mock) commits as before.
                let unsupported = |e: &&gusset_eval::EvalError| {
                    e.code == gusset_eval::ErrorCode::Unsupported || e.message.contains("is not supported by the")
                };
                match ev.errors().iter().find(|e| e.feature == *name).filter(|e| !unsupported(e)) {
                    Some(e) => {
                        let mut hint = e
                            .hint
                            .clone()
                            .unwrap_or_else(|| "move the faces less far, or with the faces around them".into());
                        if !rules {
                            // F10-8: say what rules would have done.
                            hint.push_str("; rules: true also moves coplanar and coaxial faces");
                        }
                        Err(CommandError::args(e.message.clone()).hint(hint))
                    }
                    None => Ok(()),
                }
            },
            |_, _, _| Value::Null,
        )
    }
}

/// Tapers faces of a body for molding (ADR 0028).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    /// Faces to taper (planar, cylindrical or conical, crossing the plane).
    pub faces: Vec<String>,
    /// Neutral plane: XY/XZ/YZ, planar face or datum plane.
    pub plane: String,
    /// Draft angle (`2 deg`); positive narrows the part along the pull direction.
    pub angle: String,
    /// Reverse the pull direction (plane normal, or into the body for a face).
    #[serde(default)]
    pub flip: bool,
    /// Body to change (default: latest).
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Draft {
    const ID: &'static str = "feature.draft";
    const TITLE: &'static str = "Draft";
    const DESCRIPTION: &'static str =
        "Taper faces by an angle about a neutral plane (mold/cast release). Needs OCCT. Returns body_changes.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let angle = parse_expr("angle", &self.angle)?;
        let faces = parse_faces(&self.faces)?;
        if faces.is_empty() {
            return Err(CommandError::args("no faces to draft").hint("list the side faces to taper"));
        }
        for f in &faces {
            check_face_resolves(s, ctx, f)?;
        }
        let plane = super::datum::parse_plane_arg(s, ctx, &self.plane)?;
        let owner = owner_body(s, ctx, &self.faces)?;
        edit(s, ctx, "Draft", |doc, rb| {
            let name = feature_name(doc, self.name, "Draft")?;
            let body = body_for(doc, rb, self.body.or(owner))?;
            let def = DraftDef { body, faces, plane, angle, flip: self.flip };
            insert_feature(doc, rb, Feature::new(name.clone(), FeatureKind::Draft(def)));
            Ok(json!({ "feature": name }))
        })
    }
}

/// Hollows a body, opening the given faces.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shell {
    /// Faces to remove (refs from query_body, e.g. `Extrude1/cap(end)`); empty = closed hollow. Inner walls: `<feature>/inner(<face>)`.
    #[serde(default)]
    pub faces: Vec<String>,
    /// Wall thickness (`2 mm`); positive inward, negative outward.
    pub thickness: String,
    /// Body to hollow (default: latest).
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Shell {
    const ID: &'static str = "feature.shell";
    const TITLE: &'static str = "Shell";
    const DESCRIPTION: &'static str = "Hollow a body to a wall thickness, removing the selected faces (e.g. the top: open box). Needs OCCT. Fails with a hint if walls are too thick. Returns body_changes.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let thickness = parse_expr("thickness", &self.thickness)?;
        let faces = parse_faces(&self.faces)?;
        for f in &faces {
            check_face_resolves(s, ctx, f)?;
        }
        let owner = owner_body(s, ctx, &self.faces)?;
        edit(s, ctx, "Shell", |doc, rb| {
            let name = feature_name(doc, self.name, "Shell")?;
            let body = body_for(doc, rb, self.body.or(owner))?;
            insert_feature(
                doc,
                rb,
                Feature::new(name.clone(), FeatureKind::Shell(ShellDef { body, faces, thickness })),
            );
            Ok(json!({ "feature": name }))
        })
    }
}

/// Mirrors a body across a plane.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Mirror {
    /// XY, XZ, YZ (through the origin), a planar face like
    /// `Extrude1/side(Sketch1/l6)` or a datum plane name (feature_plane).
    pub plane: String,
    /// true: unite with the source; false (default): a new body named after the feature. Faces: `<feature>/mirror(<face>)`.
    #[serde(default)]
    pub merge: bool,
    /// Body to mirror (default: latest).
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Mirror {
    const ID: &'static str = "feature.mirror";
    const TITLE: &'static str = "Mirror";
    const DESCRIPTION: &'static str = "Mirror a body across XY/XZ/YZ, a planar face or datum plane, as a new body or united with it. Uniting needs OCCT.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let plane = super::datum::parse_plane_arg(s, ctx, &self.plane)?;
        edit(s, ctx, "Mirror", |doc, rb| {
            let name = feature_name(doc, self.name, "Mirror")?;
            let body = body_for(doc, rb, self.body)?;
            insert_feature(
                doc,
                rb,
                Feature::new(
                    name.clone(),
                    FeatureKind::Mirror(MirrorDef { body, plane, merge: self.merge, refine: true }),
                ),
            );
            Ok(json!({ "feature": name }))
        })
    }
}

/// The features a pattern repeats must exist.
fn check_features(doc: &Document, features: &[String]) -> Result<(), CommandError> {
    match features.iter().find(|f| doc.feature(f).is_none()) {
        Some(f) => Err(CommandError::not_found(format!("no feature '{f}'"))),
        None => Ok(()),
    }
}

/// Repeats a body along a direction.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternLinear {
    /// `X`, `Y`, `Z` or a vector such as `[1, 0.5, 0]` (unitless components).
    pub direction: String,
    /// Instances incl. the original, whole number >= 2 (may be an expression).
    pub count: String,
    /// Distance between neighbours, e.g. `15 mm` (negative reverses).
    pub spacing: String,
    /// Body to repeat (default: latest). Faces: `<feature>/instance(k, <face>)`.
    #[serde(default)]
    pub body: Option<String>,
    /// Repeat only these features (holes, cuts, bosses), not the body.
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for PatternLinear {
    const ID: &'static str = "feature.pattern_linear";
    const TITLE: &'static str = "Linear pattern";
    const DESCRIPTION: &'static str =
        "Repeat a body or some of its features along a direction. Uniting needs OCCT unless the copies stay apart.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let direction =
            text::parse_direction(&self.direction).map_err(|e| CommandError::args(format!("direction: {e}")))?;
        let count = parse_expr("count", &self.count)?;
        let spacing = parse_expr("spacing", &self.spacing)?;
        edit(s, ctx, "Linear pattern", |doc, rb| {
            let name = feature_name(doc, self.name, "Pattern")?;
            check_features(doc, &self.features)?;
            let body = if self.features.is_empty() { body_for(doc, rb, self.body)? } else { None };
            let features = self.features;
            insert_feature(
                doc,
                rb,
                Feature::new(
                    name.clone(),
                    FeatureKind::PatternLinear(PatternLinearDef {
                        body,
                        direction,
                        count,
                        spacing,
                        features,
                        refine: true,
                        merge: true,
                    }),
                ),
            );
            Ok(json!({ "feature": name }))
        })
    }
}

/// Repeats a body around an axis.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternCircular {
    /// `X`, `Y`, `Z` or a vector like `[0, 1, 0]`.
    pub axis: String,
    /// Point on the axis, three lengths (default: origin).
    #[serde(default)]
    pub through: Option<[String; 3]>,
    /// Instances incl. the original, whole number >= 2 (may be an expression).
    pub count: String,
    /// Total angle, default `360 deg` (full turn: spaced angle/count; less: last at the angle).
    #[serde(default)]
    pub angle: Option<String>,
    /// Body to repeat (default: latest). Faces: `<feature>/instance(k, <face>)`.
    #[serde(default)]
    pub body: Option<String>,
    /// Repeat only these features (holes, cuts, bosses), not the body.
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for PatternCircular {
    const ID: &'static str = "feature.pattern_circular";
    const TITLE: &'static str = "Circular pattern";
    const DESCRIPTION: &'static str = "Repeat a body or some of its features (e.g. a bolt circle of holes) around an axis. Uniting needs OCCT unless the copies stay apart.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let axis = text::parse_direction(&self.axis).map_err(|e| CommandError::args(format!("axis: {e}")))?;
        let count = parse_expr("count", &self.count)?;
        let angle = parse_expr("angle", self.angle.as_deref().unwrap_or("360 deg"))?;
        let through = match &self.through {
            None => None,
            Some([x, y, z]) => Some([parse_expr("through", x)?, parse_expr("through", y)?, parse_expr("through", z)?]),
        };
        edit(s, ctx, "Circular pattern", |doc, rb| {
            let name = feature_name(doc, self.name, "Pattern")?;
            check_features(doc, &self.features)?;
            let body = if self.features.is_empty() { body_for(doc, rb, self.body)? } else { None };
            let features = self.features;
            insert_feature(
                doc,
                rb,
                Feature::new(
                    name.clone(),
                    FeatureKind::PatternCircular(PatternCircularDef {
                        body,
                        axis,
                        through,
                        count,
                        angle,
                        features,
                        refine: true,
                        merge: true,
                    }),
                ),
            );
            Ok(json!({ "feature": name }))
        })
    }
}
