//! Sheet-metal commands (ADR 0037): contour flanges, tabs and the flat pattern.

use super::*;
use gusset_doc::{SheetDef, SheetOp};
use gusset_drawing::scene::{Anchor, Prim, Scene, Style};
use gusset_eval::sheet::FlatPattern;
use gusset_units::expr::Expr;
use std::sync::Arc;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        spec::<ContourFlange>(),
        spec::<Tab>(),
        spec::<EdgeFlange>(),
        spec::<Bend>(),
        spec::<FlatPatternQuery>(),
        spec::<ExportFlat>(),
    ]
}

fn sheet(name: String, op: SheetOp, thickness: Option<Expr>, flip: bool) -> Feature {
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
            insert_feature(doc, rb, sheet(name.clone(), op, Some(thickness), self.flip));
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
            insert_feature(doc, rb, sheet(name.clone(), SheetOp::Tab { profile }, Some(thickness), self.flip));
            Ok(json!({ "feature": name }))
        })
    }
}

/// Bends a flange off one outline line of a tab.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdgeFlange {
    /// Tab or contour flange.
    pub body: String,
    /// `l5`, `l2_start`/`l2_end` or flange `F1_end`.
    pub line: String,
    /// Straight flange after the bend.
    pub length: String,
    /// Default `90 deg`.
    #[serde(default)]
    pub angle: Option<String>,
    /// Inside radius (default: the thickness).
    #[serde(default)]
    pub bend_radius: Option<String>,
    /// Default 0.44.
    #[serde(default)]
    pub k_factor: Option<String>,
    /// Bend down instead of up.
    #[serde(default)]
    pub flip: bool,
    /// Close corners.
    #[serde(default)]
    pub close: bool,
    /// Meet on the diagonal.
    #[serde(default)]
    pub mitre: bool,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for EdgeFlange {
    const ID: &'static str = "feature.edge_flange";
    const TITLE: &'static str = "Edge flange";
    const DESCRIPTION: &'static str = "Bend a flange off a tab line, contour side or flange end.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let length = parse_expr("length", &self.length)?;
        let angle = parse_expr("angle", self.angle.as_deref().unwrap_or("90 deg"))?;
        let k_factor = parse_expr("k_factor", self.k_factor.as_deref().unwrap_or("0.44"))?;
        let ident = |v: &str| {
            v.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && v.len() <= 64
        };
        if !ident(&self.line) || !ident(&self.body) {
            return Err(CommandError::args("body and line must be names such as Tab1 and l5"));
        }
        edit(s, ctx, "Edge flange", |doc, rb| {
            // The default radius is the tab's thickness, as an expression.
            let thickness = doc.features.iter().find(|f| f.name == self.body).and_then(|f| match &f.kind {
                FeatureKind::Sheet(d) => d.thickness.clone(),
                _ => None,
            });
            let bend_radius = match (&self.bend_radius, thickness) {
                (Some(r), _) => parse_expr("bend_radius", r)?,
                (None, Some(t)) => t,
                (None, None) => {
                    return Err(CommandError::args(format!("'{}' is not sheet metal", self.body))
                        .hint("edge flanges bend off feature_tab or feature_contour_flange"));
                }
            };
            let name = feature_name(doc, self.name, "EdgeFlange")?;
            let op = SheetOp::EdgeFlange {
                body: self.body,
                line: self.line,
                length: Box::new(length),
                angle: Box::new(angle),
                bend_radius: Box::new(bend_radius),
                k_factor: Box::new(k_factor),
                close: self.close || self.mitre,
                mitre: self.mitre,
            };
            insert_feature(doc, rb, sheet(name.clone(), op, None, self.flip));
            Ok(json!({ "feature": name }))
        })
    }
}

/// Bends a flat stretch of a sheet-metal body along a sketch line.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bend {
    /// The sheet-metal feature to bend (a tab, flange, or imported sheet).
    pub body: String,
    /// The bend line, `Sketch3.l2`.
    pub line: String,
    pub angle: String,
    /// Inside bend radius (default: the thickness, when the body is a tab or flange).
    #[serde(default)]
    pub bend_radius: Option<String>,
    /// Default 0.44.
    #[serde(default)]
    pub k_factor: Option<String>,
    /// Which side of the line moves, seen from the sketch normal: left (default) or right.
    #[serde(default)]
    pub side: Option<String>,
    /// Where the bend zone lies against the line: center (default), start or end.
    #[serde(default)]
    pub position: Option<String>,
    /// Bend down instead of up.
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Bend {
    const ID: &'static str = "feature.bend";
    const TITLE: &'static str = "Bend";
    const DESCRIPTION: &'static str = "Bend a flat stretch of a sheet-metal body along a sketch line.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let angle = parse_expr("angle", &self.angle)?;
        let k_factor = parse_expr("k_factor", self.k_factor.as_deref().unwrap_or("0.44"))?;
        let ident = |v: &str| {
            v.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && v.len() <= 64
        };
        let (sketch, line) = match self.line.split_once('.') {
            Some((a, b)) if ident(a) && ident(b) => (a.to_string(), b.to_string()),
            _ => return Err(CommandError::args("line must be a sketch line such as Sketch3.l2")),
        };
        if !ident(&self.body) {
            return Err(CommandError::args("body must be a feature name such as Tab1"));
        }
        let right = match self.side.as_deref() {
            None | Some("left") => false,
            Some("right") => true,
            Some(o) => return Err(CommandError::args(format!("unknown side '{o}' (left, right)"))),
        };
        let position = match self.position.as_deref() {
            None => gusset_doc::BendPosition::Center,
            Some(p) => gusset_doc::BendPosition::from_keyword(p)
                .ok_or_else(|| CommandError::args(format!("unknown position '{p}' (center, start, end)")))?,
        };
        edit(s, ctx, "Bend", |doc, rb| {
            let thickness = doc.features.iter().find(|f| f.name == self.body).and_then(|f| match &f.kind {
                FeatureKind::Sheet(d) => d.thickness.clone(),
                _ => None,
            });
            let bend_radius = match (&self.bend_radius, thickness) {
                (Some(r), _) => parse_expr("bend_radius", r)?,
                (None, Some(t)) => t,
                (None, None) => {
                    return Err(CommandError::args("bend_radius is needed for this body")
                        .hint("give bend_radius, e.g. \"2 mm\""));
                }
            };
            let name = feature_name(doc, self.name, "Bend")?;
            let op = SheetOp::Bend {
                body: self.body,
                sketch,
                line,
                angle: Box::new(angle),
                bend_radius: Box::new(bend_radius),
                k_factor: Box::new(k_factor),
                right,
                position,
            };
            insert_feature(doc, rb, sheet(name.clone(), op, None, self.flip));
            Ok(json!({ "feature": name }))
        })
    }
}

/// The developed shape of a sheet-metal body.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlatPatternQuery {
    /// Default: latest sheet-metal body.
    #[serde(default)]
    pub body: Option<String>,
}

/// Rebuild errors to report with a flat pattern: features that failed are not in it (F8-4).
fn failed_report(s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
    let ev = s.evaluate(&ctx.actor)?;
    Ok(if ev.errors().is_empty() { json!({}) } else { rebuild_report(&ev) })
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
                    "zone_start_mm": b.start.map(|p| p.map(tidy)),
                    "zone_end_mm": b.end.map(|p| p.map(tidy)),
                })
            })
            .collect();
        // The holes in the pattern (cuts unfolded from later features): box and area.
        let cutouts: Vec<Value> = flat
            .outlines
            .iter()
            .enumerate()
            .skip(1)
            .map(|(oi, o)| {
                let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
                for p in o {
                    lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                    hi = [hi[0].max(p[0]), hi[1].max(p[1])];
                }
                let n = o.len();
                let area = (0..n)
                    .map(|i| {
                        let (p, q) =
                            (o.get(i).copied().unwrap_or_default(), o.get((i + 1) % n).copied().unwrap_or_default());
                        p[0] * q[1] - q[0] * p[1]
                    })
                    .sum::<f64>()
                    .abs()
                    / 2.0;
                match flat.circles.iter().find(|c| c.outline == oi) {
                    Some(c) => json!({
                        "bbox_mm": [lo.map(tidy), hi.map(tidy)],
                        "area_mm2": tidy(std::f64::consts::PI * c.radius * c.radius),
                        "circle": {"centre_mm": c.centre.map(tidy), "radius_mm": tidy(c.radius)},
                    }),
                    None => json!({"bbox_mm": [lo.map(tidy), hi.map(tidy)], "area_mm2": tidy(area)}),
                }
            })
            .collect();
        let out = json!({
            "body": body,
            "thickness_mm": tidy(flat.thickness),
            "size_mm": [tidy(size[0]), tidy(size[1])],
            "bends": bends,
            "cutouts": cutouts,
            "changed_by": flat.changed_by,
            "warning": stale(&flat),
        });
        Ok(merge(out, failed_report(s, ctx)?))
    }
}

/// Writes the flat pattern of a sheet-metal body as DXF or SVG.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportFlat {
    /// `.dxf` or `.svg` in the project folder.
    pub path: String,
    /// Default: latest sheet-metal body.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
    /// Write even if later features changed the body (they are not in it).
    #[serde(default)]
    pub allow_changed: bool,
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
        if let Some(w) = stale(&flat)
            && !self.allow_changed
        {
            return Err(CommandError::args(format!("'{body}' was {w}")).hint(
                "cuts through flats and bends are unfolded; notches across an edge, cuts through edge-flange ends or \
                 added material are not: pass allow_changed: true to export anyway",
            ));
        }
        let sc = scene(&flat);
        let bytes = if dxf {
            gusset_drawing::dxf::to_dxf(&sc, gusset_drawing::dxf::Profile::default()).into_bytes()
        } else {
            gusset_drawing::svg::to_svg(&sc).into_bytes()
        };
        crate::paths::write_file(&path, p, &bytes)?;
        // The drawing's point = the pattern's point + offset (a 10 mm margin round the
        // pattern's box), so its coordinates can be traced back (F7-12).
        let offset = scene_offset(&flat).map(tidy);
        let out = json!({ "written": p, "bytes": bytes.len(), "body": body, "offset_mm": offset });
        Ok(merge(out, failed_report(s, ctx)?))
    }
}

/// Why the flat pattern may not match the part, if it may not.
fn stale(flat: &FlatPattern) -> Option<String> {
    (!flat.changed_by.is_empty()).then(|| {
        format!(
            "changed after it was made by {}: the flat pattern shows the sheet metal without those changes",
            flat.changed_by.join(", ")
        )
    })
}

fn tidy(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// The flat pattern at 1:1 on a sheet with a 10 mm margin: outlines as visible lines,
/// bend zone edges as centre lines, a note per bend.
/// The margin round a flat pattern on its drawing (mm).
const FLAT_MARGIN: f64 = 10.0;

/// What [`scene`] adds to a pattern point: the margin less the pattern's lowest corner.
fn scene_offset(flat: &FlatPattern) -> [f64; 2] {
    let mut lo = [f64::INFINITY; 2];
    for p in flat.outlines.iter().flatten() {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
    }
    if !lo[0].is_finite() {
        lo = [0.0, 0.0];
    }
    [FLAT_MARGIN - lo[0], FLAT_MARGIN - lo[1]]
}

pub(crate) fn scene(flat: &FlatPattern) -> Scene {
    let size = flat.size();
    let margin = FLAT_MARGIN;
    let off = scene_offset(flat);
    let at = |p: [f64; 2]| [p[0] + off[0], p[1] + off[1]];
    let mut sc = Scene { size: [size[0] + 2.0 * margin, size[1] + 2.0 * margin], ..Scene::default() };
    for (i, l) in flat.outlines.iter().enumerate() {
        match flat.circles.iter().find(|c| c.outline == i) {
            Some(c) => sc.push(
                Style::Visible,
                Prim::Arc { center: at(c.centre), radius: c.radius, start: 0.0, end: std::f64::consts::TAU },
            ),
            None => {
                sc.push(Style::Visible, Prim::Polyline { points: l.iter().map(|p| at(*p)).collect(), closed: true })
            }
        }
    }
    for b in &flat.bends {
        for [p, q] in [b.start, b.end] {
            sc.line(Style::Center, at(p), at(q));
        }
        let dir = if b.angle >= 0.0 { "UP" } else { "DOWN" };
        let mid = [
            (b.start[0][0] + b.start[1][0] + b.end[0][0] + b.end[1][0]) / 4.0,
            (b.start[0][1] + b.start[1][1] + b.end[0][1] + b.end[1][1]) / 4.0,
        ];
        let (dir_x, dir_y) = (b.start[1][0] - b.start[0][0], b.start[1][1] - b.start[0][1]);
        let text = format!("{dir} {:.1}° R{:.2}", b.angle.abs().to_degrees(), b.inner_radius);
        sc.push(
            Style::Text,
            Prim::Text { at: at(mid), height: 2.5, text, anchor: Anchor::Center, angle: libm::atan2(dir_y, dir_x) },
        );
    }
    sc
}
