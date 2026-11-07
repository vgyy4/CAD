//! Surface commands (ADR 0036): sheets from sketch curves or regions, thicken, stitch.

use super::*;
use gusset_doc::{SurfaceDef, SurfaceOp};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<SurfaceExtrude>(), spec::<Patch>(), spec::<Thicken>(), spec::<Stitch>()]
}

fn surface(name: String, op: SurfaceOp, body_name: Option<String>) -> Feature {
    Feature::new(name, FeatureKind::Surface(SurfaceDef { op, body_name }))
}

/// Extrudes sketch curves into a sheet.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceExtrude {
    /// `Sketch1` (every curve) or `Sketch1.curves(l3, a5)`: one chain, open or closed.
    pub curves: String,
    pub distance: String,
    #[serde(default)]
    pub symmetric: bool,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for SurfaceExtrude {
    const ID: &'static str = "feature.surface_extrude";
    const TITLE: &'static str = "Extrude surface";
    const DESCRIPTION: &'static str = "Extrude open or closed sketch curves into a sheet body (no caps).";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let curves = gusset_doc::text::parse_path_ref(&self.curves)
            .map_err(|e| CommandError::args(format!("curves: {}", e.message)))?;
        if !matches!(curves, PathRef::Sketch { .. }) {
            return Err(CommandError::args("curves must be sketch curves, e.g. Sketch1.curves(l3, a5)"));
        }
        let distance = parse_expr("distance", &self.distance)?;
        edit(s, ctx, "Extrude surface", |doc, rb| {
            let name = feature_name(doc, self.name, "Surface")?;
            let op = SurfaceOp::Extrude { curves, distance, symmetric: self.symmetric };
            insert_feature(doc, rb, surface(name.clone(), op, None));
            Ok(json!({ "feature": name }))
        })
    }
}

/// Fills sketch regions with planar sheets.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    /// `Sketch1` (every region) or `Sketch1.regions(l4)`.
    pub profile: String,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Patch {
    const ID: &'static str = "feature.patch";
    const TITLE: &'static str = "Patch";
    const DESCRIPTION: &'static str = "Fill closed sketch regions with a planar sheet body.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let profile = gusset_doc::text::parse_profile_ref(&self.profile)
            .map_err(|e| CommandError::args(format!("profile: {}", e.message)))?;
        edit(s, ctx, "Patch", |doc, rb| {
            let name = feature_name(doc, self.name, "Patch")?;
            insert_feature(doc, rb, surface(name.clone(), SurfaceOp::Patch { profile }, None));
            Ok(json!({ "feature": name }))
        })
    }
}

/// Thickens a sheet body into a solid.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Thicken {
    /// `2 mm`; negative thickens to the other side.
    pub thickness: String,
    /// Half the thickness to each side.
    #[serde(default)]
    pub both_sides: bool,
    /// Sheet body (default: latest).
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Thicken {
    const ID: &'static str = "feature.thicken";
    const TITLE: &'static str = "Thicken";
    const DESCRIPTION: &'static str = "Thicken a sheet body into a solid along its normals.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let thickness = parse_expr("thickness", &self.thickness)?;
        edit(s, ctx, "Thicken", |doc, rb| {
            let name = feature_name(doc, self.name, "Thicken")?;
            let body = ops::body_for(doc, rb, self.body)?;
            let op = SurfaceOp::Thicken { body, thickness, both_sides: self.both_sides };
            insert_feature(doc, rb, surface(name.clone(), op, None));
            Ok(json!({ "feature": name }))
        })
    }
}

/// Sews bodies together along shared edges.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Stitch {
    /// Bodies (feature names), at least two.
    pub bodies: Vec<String>,
    /// Gap to close (default `0.01 mm`).
    #[serde(default)]
    pub tolerance: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Stitch {
    const ID: &'static str = "feature.stitch";
    const TITLE: &'static str = "Stitch";
    const DESCRIPTION: &'static str = "Sew sheet bodies along shared edges into one body; closed becomes a solid.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let tolerance = parse_expr("tolerance", self.tolerance.as_deref().unwrap_or("0.01 mm"))?;
        if self.bodies.len() < 2 {
            return Err(CommandError::args("stitch needs at least two bodies"));
        }
        edit(s, ctx, "Stitch", |doc, rb| {
            let name = feature_name(doc, self.name, "Stitch")?;
            let op = SurfaceOp::Stitch { bodies: self.bodies.clone(), tolerance };
            insert_feature(doc, rb, surface(name.clone(), op, None));
            Ok(json!({ "feature": name }))
        })
    }
}
