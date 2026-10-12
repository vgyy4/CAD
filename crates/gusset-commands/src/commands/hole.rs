//! The hole command (ADR 0019): holes on a planar face, optionally from a standard preset.
//!
//! A preset never becomes a feature of its own (DESIGN.md "never a black box", ADR 0018 §6):
//! the command looks the values up, writes each as an ordinary named parameter
//! (`hole1_diameter`, ...) and a plain `hole` statement that refers to them. Explicit
//! arguments override the preset's values; without a `standard` the expressions go straight
//! into the statement.

use super::*;
use crate::hole_standards::{self as standards, Preset};
use gusset_eval::{ErrorCode, HoleInfo};
use std::collections::BTreeMap;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<Hole>()]
}

/// Renames references to feature `from` inside a hole feature.
pub(super) fn rename_refs(d: &mut HoleDef, from: &str, to: &str) {
    d.face.rename_feature(from, to);
    if let HoleSites::Sketch(s) = &mut d.sites
        && s == from
    {
        *s = to.to_string();
    }
}

/// Drills holes on a planar face.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Hole {
    /// e.g. Extrude1/cap(end)
    #[schemars(extend("x-essential" = true))]
    pub face: String,
    /// Sketch: points, circle centres.
    #[serde(default)]
    #[schemars(extend("x-essential" = true))]
    pub points: Option<String>,
    /// Or [["10 mm","5 mm"]].
    #[serde(default)]
    pub at: Option<Vec<[String; 2]>>,
    /// "ISO M6 clearance normal"
    #[serde(default)]
    #[schemars(extend("x-essential" = true))]
    pub standard: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    #[schemars(extend("x-essential" = true))]
    pub diameter: Option<String>,
    #[serde(default)]
    #[schemars(extend("x-essential" = true))]
    pub depth: Option<String>,
    #[serde(default)]
    #[schemars(extend("x-essential" = true))]
    pub through: Option<bool>,
    #[serde(default)]
    pub point_angle: Option<String>,
    #[serde(default)]
    pub cbore_diameter: Option<String>,
    #[serde(default)]
    pub cbore_depth: Option<String>,
    #[serde(default)]
    pub csink_diameter: Option<String>,
    #[serde(default)]
    pub csink_angle: Option<String>,
    #[serde(default)]
    pub thread: Option<String>,
    #[serde(default)]
    pub pitch: Option<String>,
    #[serde(default)]
    pub thread_depth: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

/// One dimension of the new feature: where it came from and what it says.
struct DimPlan {
    arg: &'static str,
    /// The value as text.
    value: String,
    /// Comment of the parameter written for it.
    comment: String,
}

/// Everything decided before the document is touched.
struct Plan {
    face: FaceRef,
    sites: HoleSites,
    kind: HoleType,
    through: bool,
    designation: Option<String>,
    dims: Vec<DimPlan>,
    preset: Option<Preset>,
    sources: &'static str,
}

const DIM_ARGS: [&str; 9] = [
    "diameter",
    "depth",
    "point_angle",
    "cbore_diameter",
    "cbore_depth",
    "csink_diameter",
    "csink_angle",
    "pitch",
    "thread_depth",
];

impl Hole {
    fn explicit(&self, arg: &str) -> Option<&String> {
        match arg {
            "diameter" => &self.diameter,
            "depth" => &self.depth,
            "point_angle" => &self.point_angle,
            "cbore_diameter" => &self.cbore_diameter,
            "cbore_depth" => &self.cbore_depth,
            "csink_diameter" => &self.csink_diameter,
            "csink_angle" => &self.csink_angle,
            "pitch" => &self.pitch,
            "thread_depth" => &self.thread_depth,
            _ => &None,
        }
        .as_ref()
    }

    fn plan(&self) -> Result<Plan, CommandError> {
        let face = FaceRef::parse(&self.face).map_err(|e| {
            CommandError::args(format!("face: {e}"))
                .hint("name a face like Extrude1/cap(end); query_body with names: true lists them")
        })?;
        let sites = match (&self.points, &self.at) {
            (Some(p), None) => HoleSites::Sketch(p.clone()),
            (None, Some(list)) => {
                if list.is_empty() {
                    return Err(CommandError::args("at lists no points").hint("give at least one [u, v] pair"));
                }
                if list.len() > MAX_HOLE_SITES {
                    return Err(CommandError::args(format!("at most {MAX_HOLE_SITES} holes per feature")));
                }
                HoleSites::Points(
                    list.iter().map(|[u, v]| Ok([parse_expr("at", u)?, parse_expr("at", v)?])).collect::<Result<
                        _,
                        CommandError,
                    >>(
                    )?,
                )
            }
            _ => {
                return Err(CommandError::args("give either points (a sketch) or at (a list of [u, v])")
                    .hint("points: \"Sketch2\", or at: [[\"10 mm\", \"5 mm\"]]"));
            }
        };
        let preset = match self.standard.as_deref() {
            Some(text) => Some(
                standards::parse(text)
                    .map_err(|e| CommandError::args(format!("standard: {}", e.message)).hint(e.hint))?,
            ),
            None => None,
        };
        let explicit_kind = match self.kind.as_deref() {
            Some(k) => Some(HoleType::from_keyword(k).ok_or_else(|| {
                CommandError::args(format!("unknown hole type '{k}'"))
                    .hint("use simple, counterbore, countersink or tapped")
            })?),
            None => None,
        };
        let kind = match (explicit_kind, preset.as_ref().map(|p| p.kind)) {
            (Some(a), Some(b)) if a != b => {
                return Err(CommandError::args(format!(
                    "the standard is a {} hole but type is {}",
                    b.keyword(),
                    a.keyword()
                ))
                .hint("drop type, or change the standard"));
            }
            (Some(k), _) | (None, Some(k)) => k,
            (None, None) => HoleType::Simple,
        };
        let values = preset.as_ref().map(Preset::values);

        // Dimensions: explicit values win over the preset's.
        let mut dims: BTreeMap<&'static str, DimPlan> = BTreeMap::new();
        for arg in DIM_ARGS {
            let from_preset = values.as_ref().and_then(|v| v.slots.iter().find(|s| s.arg == arg));
            let plan = match (self.explicit(arg), from_preset) {
                (Some(e), Some(p)) => DimPlan {
                    arg,
                    value: e.clone(),
                    comment: format!("set explicitly; the standard has {} ({})", p.value, p.comment),
                },
                (Some(e), None) => DimPlan { arg, value: e.clone(), comment: String::new() },
                (None, Some(p)) => DimPlan { arg, value: p.value.clone(), comment: p.comment.clone() },
                (None, None) => continue,
            };
            dims.insert(arg, plan);
        }
        // Through or blind.
        if self.through == Some(true) && self.depth.is_some() {
            return Err(CommandError::args("give either depth or through, not both")
                .hint("through: true cuts all the way; depth stops short"));
        }
        let through = match (self.through, &self.depth, &values) {
            (Some(true), _, _) => true,
            (_, Some(_), _) => false,
            (_, None, Some(v)) => v.through,
            _ => {
                return Err(CommandError::args("give a depth or through: true")
                    .hint("depth: \"12 mm\" for a blind hole, or through: true"));
            }
        };
        if through {
            // A preset's blind depth and thread length do not apply to a through hole, but
            // a thread length given by hand stays.
            dims.remove("depth");
            if self.thread_depth.is_none() {
                dims.remove("thread_depth");
            }
            if self.point_angle.is_some() {
                return Err(CommandError::args("point_angle only applies to a blind hole")
                    .hint("remove it, or give a depth instead of through"));
            }
        } else if preset.is_some() && !dims.contains_key("point_angle") {
            dims.insert(
                "point_angle",
                DimPlan {
                    arg: "point_angle",
                    value: "118 deg".into(),
                    comment: "drill point angle of a standard twist drill".into(),
                },
            );
        }
        let designation = self.thread.clone().or_else(|| values.as_ref().and_then(|v| v.designation.clone()));

        // What each kind needs, and what it must not have.
        let need = |args: &[&str]| -> Result<(), CommandError> {
            for a in args {
                if !dims.contains_key(a) {
                    return Err(CommandError::args(format!("a {} hole needs {a}", kind.keyword()))
                        .hint("or give a standard such as \"ISO M6 clearance normal\""));
                }
            }
            Ok(())
        };
        need(&["diameter"])?;
        let (cbore, csink, tapped) =
            (kind == HoleType::Counterbore, kind == HoleType::Countersink, kind == HoleType::Tapped);
        if cbore {
            need(&["cbore_diameter", "cbore_depth"])?;
        }
        if csink {
            need(&["csink_diameter", "csink_angle"])?;
        }
        if tapped {
            need(&["pitch"])?;
            if designation.is_none() {
                return Err(CommandError::args("a tapped hole needs thread, for example \"M6\"")
                    .hint("or give a standard such as \"ISO M6 tapped\""));
            }
        }
        for (arg, ok, ty) in [
            ("cbore_diameter", cbore, "counterbore"),
            ("cbore_depth", cbore, "counterbore"),
            ("csink_diameter", csink, "countersink"),
            ("csink_angle", csink, "countersink"),
            ("pitch", tapped, "tapped"),
            ("thread_depth", tapped, "tapped"),
        ] {
            if !ok && dims.contains_key(arg) {
                return Err(CommandError::args(format!("{arg} only applies to type {ty}"))
                    .hint(format!("remove {arg}, or set type: {ty}")));
            }
        }
        if !tapped && self.thread.is_some() {
            return Err(CommandError::args("thread only applies to type tapped").hint("set type: tapped"));
        }
        Ok(Plan {
            face,
            sites,
            kind,
            through,
            designation,
            dims: DIM_ARGS.iter().filter_map(|a| dims.remove(a)).collect(),
            sources: values.as_ref().map_or("", |v| v.sources),
            preset,
        })
    }

    /// Adds the parameters and the feature to `doc`; returns the feature name and the
    /// parameters created.
    fn apply(&self, plan: &Plan, doc: &mut Document, rb: Option<usize>) -> Result<(String, Vec<String>), CommandError> {
        let name = feature_name(doc, self.name.clone(), "Hole")?;
        let prefix = name.to_lowercase();
        let mut created: Vec<String> = Vec::new();
        let mut exprs: BTreeMap<&str, gusset_units::expr::Expr> = BTreeMap::new();
        for d in &plan.dims {
            let value = parse_expr(d.arg, &d.value)?;
            if plan.preset.is_none() {
                exprs.insert(d.arg, value);
                continue;
            }
            let pname = format!("{prefix}_{}", d.arg);
            validate_feature_name(&pname).map_err(CommandError::args)?;
            if doc.params.iter().any(|p| p.name == pname) {
                return Err(CommandError::args(format!("a parameter named '{pname}' already exists"))
                    .hint("name the hole differently, for example name: \"Hole2\""));
            }
            exprs.insert(d.arg, parse_expr(d.arg, &pname)?);
            doc.params.push(Param { name: pname.clone(), expr: value, comment: d.comment.clone() });
            created.push(pname);
        }
        let take = |exprs: &mut BTreeMap<&str, gusset_units::expr::Expr>, arg: &str| exprs.remove(arg);
        let missing = |arg: &str| CommandError::args(format!("{arg} is missing"));
        let diameter = take(&mut exprs, "diameter").ok_or_else(|| missing("diameter"))?;
        let depth = if plan.through {
            HoleDepth::Through
        } else {
            HoleDepth::Blind(take(&mut exprs, "depth").ok_or_else(|| missing("depth"))?)
        };
        let counterbore = match (take(&mut exprs, "cbore_diameter"), take(&mut exprs, "cbore_depth")) {
            (Some(diameter), Some(depth)) => Some(CounterboreDef { diameter, depth }),
            _ => None,
        };
        let countersink = match (take(&mut exprs, "csink_diameter"), take(&mut exprs, "csink_angle")) {
            (Some(diameter), Some(angle)) => Some(CountersinkDef { diameter, angle }),
            _ => None,
        };
        let thread = match (&plan.designation, take(&mut exprs, "pitch")) {
            (Some(designation), Some(pitch)) => {
                Some(ThreadDef { designation: designation.clone(), pitch, depth: take(&mut exprs, "thread_depth") })
            }
            _ => None,
        };
        let mut feature = Feature::new(
            name.clone(),
            FeatureKind::Hole(HoleDef {
                face: plan.face.clone(),
                sites: plan.sites.clone(),
                kind: plan.kind,
                diameter,
                depth,
                point_angle: take(&mut exprs, "point_angle"),
                counterbore,
                countersink,
                thread,
            }),
        );
        if let Some(p) = &plan.preset {
            feature.note = Some(format!(
                "{} ({}). The dimensions are the parameters {prefix}_*; change them there.",
                p.canonical(),
                plan.sources
            ));
        }
        insert_feature(doc, rb, feature);
        Ok((name, created))
    }
}

/// A hole as `query_body` and the command report it (mm and degrees).
pub(crate) fn hole_json(h: &HoleInfo) -> Value {
    let mut v = json!({
        "feature": h.feature,
        "at": h.at,
        "type": h.kind.keyword(),
        "diameter_mm": h.diameter,
        "through": h.depth.is_none(),
        "wall": h.wall.to_string(),
    });
    if let Value::Object(m) = &mut v {
        if let Some(d) = h.depth {
            m.insert("depth_mm".into(), json!(d));
        }
        if let Some(a) = h.point_angle {
            m.insert("point_angle_deg".into(), json!(a.to_degrees()));
        }
        if let Some((d, depth)) = h.counterbore {
            m.insert("counterbore".into(), json!({ "diameter_mm": d, "depth_mm": depth }));
        }
        if let Some((d, a)) = h.countersink {
            m.insert("countersink".into(), json!({ "diameter_mm": d, "angle_deg": a.to_degrees() }));
        }
        if let Some(t) = &h.thread {
            m.insert(
                "thread".into(),
                json!({
                    "designation_user_text": t.designation,
                    "pitch_mm": t.pitch,
                    "depth_mm": t.depth,
                    "cosmetic": true,
                }),
            );
        }
    }
    v
}

impl Command for Hole {
    const ID: &'static str = "feature.hole";
    const TITLE: &'static str = "Hole";
    const DESCRIPTION: &'static str = "Holes on a planar face: type simple|counterbore|countersink|tapped (thread: recorded only). standard writes the dimensions as parameters <name>_diameter etc.; arguments override. Depth excludes the 118 deg point.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let plan = self.plan()?;
        check_face_resolves(s, ctx, &plan.face)?;

        // Try the feature on a copy first: a hole that does not fit, a counterbore that is
        // too small and the like are refused with the reason instead of being committed.
        let branch = s.branch_for(ctx)?;
        let mut trial = s.history().head_document(branch).map_err(CommandError::history)?;
        let rollback = s.history().branch(branch).map_err(CommandError::history)?.rollback;
        let (name, _) = self.apply(&plan, &mut trial, rollback)?;
        let issues = gusset_doc::validate::validate(&trial);
        if !issues.is_empty() {
            return Err(CommandError::invalid_document(&issues));
        }
        let ev = s.evaluate_doc(&trial, rollback.map(|r| r + 1));
        if let Some(FeatureStatus::Failed(e)) = ev.status(&name) {
            let user_input = matches!(
                e.code,
                ErrorCode::Expression
                    | ErrorCode::InvalidReference
                    | ErrorCode::MissingReference
                    | ErrorCode::AmbiguousReference
                    | ErrorCode::NoTargetBody
                    | ErrorCode::InvalidSketch
                    | ErrorCode::SketchSolve
                    | ErrorCode::SketchConflict
            );
            if user_input {
                let mut err = CommandError::args(e.message.clone());
                err.hint = e.hint.clone();
                err.details =
                    Some(json!({ "error": e.code.as_str(), "reference": e.reference, "suggestion": e.suggestion }));
                return Err(err);
            }
        }

        let created = std::cell::RefCell::new((String::new(), Vec::<String>::new()));
        edit_post(
            s,
            ctx,
            "Hole",
            |doc, rb| {
                let (name, params) = self.apply(&plan, doc, rb)?;
                *created.borrow_mut() = (name.clone(), params.clone());
                let mut v = json!({ "feature": name, "type": plan.kind.keyword(), "parameters": params });
                if let (Some(p), Value::Object(m)) = (&plan.preset, &mut v) {
                    m.insert("standard".into(), json!(p.canonical()));
                }
                Ok(v)
            },
            |_, out, _| {
                let name = created.borrow().0.clone();
                let holes: Vec<Value> = out
                    .evaluation
                    .bodies
                    .iter()
                    .flat_map(|b| b.holes.iter())
                    .filter(|h| h.feature == name)
                    .map(hole_json)
                    .collect();
                let mut v = json!({ "holes": holes });
                if !out.new_errors.is_empty()
                    && let Value::Object(m) = &mut v
                {
                    m.insert("committed_with_errors".into(), json!(true));
                    m.insert(
                        "undo_hint".into(),
                        json!(
                            "the hole was committed but fails to rebuild: call history_undo to remove it, then retry"
                        ),
                    );
                }
                v
            },
        )
    }
}
