//! Sweep and loft commands (v0.2, ADR 0017).

use super::*;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<Sweep>(), spec::<Loft>()]
}

/// Sweeps a closed profile along a path.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sweep {
    /// Profile sketch.
    pub sketch: String,
    /// Region ids; empty = all.
    #[serde(default)]
    pub regions: Vec<String>,
    /// Path sketch `S`, its curves `S.curves(l1, a2)`, or edges `[edge(..)]`; an end lies in the profile plane.
    pub path: String,
    /// frenet (default, follows the path) or fixed.
    #[serde(default)]
    pub orientation: Option<String>,
    /// Turn about the path from start to end (frenet, open path).
    #[serde(default)]
    pub twist: Option<String>,
    /// new (default), add, cut, intersect.
    #[serde(default)]
    pub op: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub body_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Sweep {
    const ID: &'static str = "feature.sweep";
    const TITLE: &'static str = "Sweep";
    const DESCRIPTION: &'static str =
        "Sweep a closed profile along sketch curves or body edges (pipes, twisted bars). OCCT only.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let op = op_of(&self.op)?;
        let path = text::parse_path_ref(&self.path).map_err(|e| CommandError::args(format!("path: {e}")))?;
        let orientation = match self.orientation.as_deref() {
            None => Orientation::default(),
            Some(o) => Orientation::from_keyword(o)
                .ok_or_else(|| CommandError::args(format!("unknown orientation '{o}' (frenet, fixed)")))?,
        };
        let twist = self.twist.as_deref().map(|t| parse_expr("twist", t)).transpose()?;
        if twist.is_some() && orientation == Orientation::Fixed {
            return Err(CommandError::args("a twist needs orientation frenet")
                .hint("remove the twist or the fixed orientation"));
        }
        let (sketch_name, all_regions) = (self.sketch.clone(), self.regions.is_empty());
        let seen = std::cell::RefCell::new((String::new(), None::<String>));
        edit_post(
            s,
            ctx,
            "Sweep",
            |doc, rb| {
                let name = feature_name(doc, self.name, "Sweep")?;
                let body = target_body(doc, rb, op, self.body)?;
                let body_name = check_new_body_name(doc, op, &name, self.body_name)?;
                *seen.borrow_mut() = (name.clone(), body.as_deref().map(|t| body_display_name(doc, t)));
                let created = (op == BodyOp::New).then(|| body_name.clone().unwrap_or_else(|| name.clone()));
                let f = Feature::new(
                    name.clone(),
                    FeatureKind::Sweep(SweepDef {
                        profile: ProfileRef { sketch: self.sketch, regions: self.regions },
                        path,
                        orientation,
                        twist,
                        op,
                        body,
                        body_name,
                    }),
                );
                insert_feature(doc, rb, f);
                Ok(json!({ "feature": name, "body": created }))
            },
            |_, out, ch| {
                let (name, body) = &*seen.borrow();
                let w = combine_effect_warnings(name, op, body.as_deref(), &out.new_errors, ch);
                open_ends_warning(w, &out.evaluation, &sketch_name, all_regions)
            },
        )
    }
}

/// Lofts a solid through profiles on different planes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Loft {
    /// Sketches in order (2+): `S` or `S.regions(l5)`; one region, no holes.
    pub profiles: Vec<String>,
    /// Straight instead of smooth surfaces.
    #[serde(default)]
    pub ruled: bool,
    /// new (default), add, cut, intersect.
    #[serde(default)]
    pub op: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub body_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Loft {
    const ID: &'static str = "feature.loft";
    const TITLE: &'static str = "Loft";
    const DESCRIPTION: &'static str = "Loft a solid through 2+ closed profiles on different planes. OCCT only.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let op = op_of(&self.op)?;
        if self.profiles.len() < 2 {
            return Err(CommandError::args("a loft needs at least two profiles")
                .hint("list the sketches in order: [\"Sketch1\", \"Sketch2\"]"));
        }
        if self.profiles.len() > MAX_LOFT_PROFILES {
            return Err(CommandError::args(format!("at most {MAX_LOFT_PROFILES} profiles per loft")));
        }
        let profiles = self
            .profiles
            .iter()
            .map(|p| text::parse_profile_ref(p).map_err(|e| CommandError::args(format!("profile '{p}': {e}"))))
            .collect::<Result<Vec<_>, _>>()?;
        let seen = std::cell::RefCell::new((String::new(), None::<String>));
        edit_post(
            s,
            ctx,
            "Loft",
            |doc, rb| {
                let name = feature_name(doc, self.name, "Loft")?;
                let body = target_body(doc, rb, op, self.body)?;
                let body_name = check_new_body_name(doc, op, &name, self.body_name)?;
                *seen.borrow_mut() = (name.clone(), body.as_deref().map(|t| body_display_name(doc, t)));
                let created = (op == BodyOp::New).then(|| body_name.clone().unwrap_or_else(|| name.clone()));
                let f = Feature::new(
                    name.clone(),
                    FeatureKind::Loft(LoftDef { profiles, ruled: self.ruled, op, body, body_name }),
                );
                insert_feature(doc, rb, f);
                Ok(json!({ "feature": name, "body": created }))
            },
            |_, out, ch| {
                let (name, body) = &*seen.borrow();
                combine_effect_warnings(name, op, body.as_deref(), &out.new_errors, ch)
            },
        )
    }
}
