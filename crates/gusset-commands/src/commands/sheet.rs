//! Sheet-metal commands (ADR 0037): contour flanges, tabs and the flat pattern.

use super::*;
use gusset_doc::{SheetDef, SheetOp};
use gusset_drawing::scene::{Anchor, Prim, Scene, Style};
use gusset_eval::sheet::FlatPattern;
use gusset_units::expr::Expr;
use std::sync::Arc;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<ContourFlange>(), spec::<Tab>(), spec::<FlatPatternQuery>(), spec::<ExportFlat>()]
}

fn sheet(name: String, op: SheetOp, thickness: Expr, flip: bool) -> Feature {
    Feature::new(name, FeatureKind::Sheet(SheetDef { op, thickness, flip, body_name: None }))
}

/// Bends a chain of sketch lines into a sheet-metal part.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContourFlange {
    /// `Sketch1.curves(l3, l4, l5)`: an open chain of lines, from the first listed.
    pub curves: String,
    /// Depth along the sketch normal.
    pub width: String,
    pub thickness: String,
    /// Inside bend radius (default: the thickness).
    #[serde(default)]
    pub bend_radius: Option<String>,
    /// Neutral fibre position 0..1 (default 0.44).
    #[serde(default)]
    pub k_factor: Option<String>,
    /// Material right of the lines (default left).
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub symmetric: bool,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for ContourFlange {
    const ID: &'static str = "feature.contour_flange";
    const TITLE: &'static str = "Contour flange";
    const DESCRIPTION: &'static str =
        "Sheet metal from an open chain of sketch lines, bent at every corner; see query_flat_pattern.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let curves = gusset_doc::text::parse_path_ref(&self.curves)
            .map_err(|e| CommandError::args(format!("curves: {}", e.message)))?;
        if !matches!(curves, PathRef::Sketch { .. }) {
            return Err(CommandError::args("curves must be sketch lines, e.g. Sketch1.curves(l3, l4, l5)"));
        }
        let width = parse_expr("width", &self.width)?;
        let thickness = parse_expr("thickness", &self.thickness)?;
        let bend_radius = parse_expr("bend_radius", self.bend_radius.as_deref().unwrap_or(&self.thickness))?;
        let k_factor = parse_expr("k_factor", self.k_factor.as_deref().unwrap_or("0.44"))?;
        edit(s, ctx, "Contour flange", |doc, rb| {
            let name = feature_name(doc, self.name, "Flange")?;
            let op = SheetOp::Contour {
                curves,
                width: Box::new(width),
                symmetric: self.symmetric,
                bend_radius: Box::new(bend_radius),
                k_factor: Box::new(k_factor),
            };
            insert_feature(doc, rb, sheet(name.clone(), op, thickness, self.flip));
            Ok(json!({ "feature": name }))
        })
    }
}

/// A flat sheet-metal plate from sketch regions.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tab {
    /// `Sketch1` (every region) or `Sketch1.regions(l4)`.
    pub profile: String,
    pub thickness: String,
    /// Below the sketch plane (default above).
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Tab {
    const ID: &'static str = "feature.tab";
    const TITLE: &'static str = "Tab";
    const DESCRIPTION: &'static str = "A flat sheet-metal plate of the given thickness from closed sketch regions.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let profile = gusset_doc::text::parse_profile_ref(&self.profile)
            .map_err(|e| CommandError::args(format!("profile: {}", e.message)))?;
        let thickness = parse_expr("thickness", &self.thickness)?;
        edit(s, ctx, "Tab", |doc, rb| {
            let name = feature_name(doc, self.name, "Tab")?;
            insert_feature(doc, rb, sheet(name.clone(), SheetOp::Tab { profile }, thickness, self.flip));
            Ok(json!({ "feature": name }))
        })
    }
}

/// The developed shape of a sheet-metal body.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlatPatternQuery {
    /// Default: the latest sheet-metal body.
    #[serde(default)]
    pub body: Option<String>,
}

/// The flat pattern of `body` (default: the latest sheet-metal body).
fn flat_of(s: &mut Session, ctx: &CmdCtx, body: Option<&str>) -> Result<(String, Arc<FlatPattern>), CommandError> {
    let ev = s.evaluate(&ctx.actor)?;
    let body = match body {
        Some(n) => ev.body(n).cloned().ok_or_else(|| CommandError::not_found(format!("no body '{n}'")))?,
        None => ev.bodies.iter().rev().find(|b| b.sheet.is_some()).cloned().ok_or_else(|| {
            CommandError::not_found("the model has no sheet-metal body").hint("make one with feature_contour_flange")
        })?,
    };
    match body.sheet.clone() {
        Some(f) => Ok((body.name.clone(), f)),
        None => Err(CommandError::args(format!("'{}' is not a sheet-metal body", body.name))
            .hint("flat patterns exist for contour_flange and tab bodies")),
    }
}

impl Command for FlatPatternQuery {
    const ID: &'static str = "query.flat_pattern";
    const TITLE: &'static str = "Flat pattern";
    const DESCRIPTION: &'static str =
        "Unfolded size and bend table (angle, radius, allowance, position) of a sheet-metal body.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (body, flat) = flat_of(s, ctx, self.body.as_deref())?;
        let size = flat.size();
        let bends: Vec<Value> = flat
            .bends
            .iter()
            .map(|b| {
                json!({
                    "after": b.line,
                    "angle_deg": tidy(b.angle.to_degrees()),
                    "inner_radius_mm": tidy(b.inner_radius),
                    "k_factor": b.k_factor,
                    "allowance_mm": tidy(b.allowance),
                    "zone_mm": [tidy(b.x0), tidy(b.x1)],
                })
            })
            .collect();
        Ok(json!({
            "body": body,
            "thickness_mm": tidy(flat.thickness),
            "size_mm": [tidy(size[0]), tidy(size[1])],
            "bends": bends,
        }))
    }
}

/// Writes the flat pattern of a sheet-metal body as DXF or SVG.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportFlat {
    /// `.dxf` or `.svg` in the project folder.
    pub path: String,
    /// Default: the latest sheet-metal body.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for ExportFlat {
    const ID: &'static str = "sheet.export_flat";
    const TITLE: &'static str = "Export flat pattern";
    const DESCRIPTION: &'static str =
        "Write a sheet-metal body's flat pattern 1:1 as DXF or SVG (outline, bend lines, bend notes).";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let p = &self.path;
        let path = crate::paths::export_target(s.project_dir.as_deref(), p, &ctx.actor, &["dxf", "svg"])?;
        let dxf = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("dxf"));
        if path.exists() {
            if !self.overwrite {
                return Err(CommandError::new("confirmation_required", format!("'{p}' already exists"))
                    .hint("pass overwrite: true to replace it"));
            }
            if ctx.is_agent() && s.config.approvals_required && !ctx.approved {
                return Err(CommandError::approval("overwriting an exported file"));
            }
        }
        let (body, flat) = flat_of(s, ctx, self.body.as_deref())?;
        let sc = scene(&flat);
        let bytes = if dxf {
            gusset_drawing::dxf::to_dxf(&sc, gusset_drawing::dxf::Profile::default()).into_bytes()
        } else {
            gusset_drawing::svg::to_svg(&sc).into_bytes()
        };
        crate::paths::write_file(&path, p, &bytes)?;
        Ok(json!({ "written": p, "bytes": bytes.len(), "body": body }))
    }
}

fn tidy(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// The flat pattern at 1:1 on a sheet with a 10 mm margin: outlines as visible lines,
/// bend zone edges as centre lines, a note per bend.
pub(crate) fn scene(flat: &FlatPattern) -> Scene {
    let size = flat.size();
    let margin = 10.0;
    let mut lo = [f64::INFINITY; 2];
    for p in flat.outlines.iter().flatten() {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
    }
    if !lo[0].is_finite() {
        lo = [0.0, 0.0];
    }
    let at = |p: [f64; 2]| [p[0] - lo[0] + margin, p[1] - lo[1] + margin];
    let mut sc = Scene { size: [size[0] + 2.0 * margin, size[1] + 2.0 * margin], ..Scene::default() };
    for l in &flat.outlines {
        sc.push(Style::Visible, Prim::Polyline { points: l.iter().map(|p| at(*p)).collect(), closed: true });
    }
    for b in &flat.bends {
        for x in [b.x0, b.x1] {
            sc.line(Style::Center, at([x, b.y0]), at([x, b.y1]));
        }
        let dir = if b.angle >= 0.0 { "UP" } else { "DOWN" };
        let text = format!("{dir} {:.1}° R{:.2}", b.angle.abs().to_degrees(), b.inner_radius);
        sc.push(
            Style::Text,
            Prim::Text {
                at: at([(b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0]),
                height: 2.5,
                text,
                anchor: Anchor::Center,
                angle: std::f64::consts::FRAC_PI_2,
            },
        );
    }
    sc
}
