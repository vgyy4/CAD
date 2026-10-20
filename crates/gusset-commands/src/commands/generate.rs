//! Generators (docs/DESIGN.md "Easy for beginners, never in the way of experts", ADR 0018).
//!
//! A generator is one registry command with sensible defaults. It writes ordinary named
//! parameters, sketches and features into the document and nothing else: no feature type
//! of its own, so the result is edited in the parameter table, the code view and over MCP
//! like anything drawn by hand.

use super::*;
use crate::gear::{self, GearValues, Names};
use gusset_units::Dim;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<GenerateGear>(), spec::<GenerateSpring>()]
}

fn d_module() -> String {
    "2 mm".into()
}
fn d_teeth() -> String {
    "20".into()
}
fn d_angle() -> String {
    "20 deg".into()
}
fn d_width() -> String {
    "10 mm".into()
}
fn d_bore() -> String {
    "6 mm".into()
}
fn d_zero() -> String {
    "0".into()
}
fn d_zero_mm() -> String {
    "0 mm".into()
}
fn d_fillet() -> String {
    "0.38".into()
}
fn d_zero_deg() -> String {
    "0 deg".into()
}

/// Generates an involute spur or helical gear.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateGear {
    /// Module; gears mesh when equal.
    #[serde(default = "d_module")]
    #[schemars(extend("x-essential" = true))]
    pub module: String,
    /// Tooth count, 6 to 500.
    #[serde(default = "d_teeth")]
    #[schemars(extend("x-essential" = true))]
    pub teeth: String,
    /// Rack angle (ISO 53: 20 deg).
    #[serde(default = "d_angle")]
    #[schemars(extend("x-essential" = true))]
    pub pressure_angle: String,
    /// Gear thickness.
    #[serde(default = "d_width")]
    #[schemars(extend("x-essential" = true))]
    pub face_width: String,
    /// Bore diameter (0 = solid).
    #[serde(default = "d_bore")]
    #[schemars(extend("x-essential" = true))]
    pub bore: String,
    /// Profile shift x (positive: thicker teeth).
    #[serde(default = "d_zero")]
    pub shift: String,
    /// Tooth thinning at the pitch circle.
    #[serde(default = "d_zero_mm")]
    pub backlash: String,
    /// Root fillet / module (ISO 53: 0.38).
    #[serde(default = "d_fillet")]
    pub fillet: String,
    /// Helix angle; nonzero makes it helical.
    #[serde(default = "d_zero_deg")]
    pub helix: String,
    /// Feature name; lowercase is the parameter prefix.
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for GenerateGear {
    const ID: &'static str = "generate.gear";
    const TITLE: &'static str = "Gear";
    const DESCRIPTION: &'static str = "Generate an involute spur or helical gear (ISO 53) as named parameters, a tooth sketch and an extrude or sweep.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let given = [
            ("module", &self.module),
            ("teeth", &self.teeth),
            ("pressure_angle", &self.pressure_angle),
            ("face_width", &self.face_width),
            ("bore", &self.bore),
            ("shift", &self.shift),
            ("backlash", &self.backlash),
            ("fillet", &self.fillet),
            ("helix", &self.helix),
        ];
        let mut exprs: Vec<(&str, gusset_units::expr::Expr)> = Vec::new();
        for (what, src) in given {
            exprs.push((what, parse_expr(what, src)?));
        }
        edit(s, ctx, "Generate gear", |doc, rb| {
            let name = feature_name(doc, self.name, "Gear")?;
            let nm = Names::from_feature(&name);
            let sketch_name = format!("{name}_profile");
            let path_name = format!("{name}_path");
            // Evaluate the options in the context of the document first, to see whether the
            // gear is helical and to place the sketch's points.
            let mut all = doc.params.clone();
            for (what, e) in &exprs {
                all.push(Param {
                    name: nm.n(what),
                    expr: e.clone(),
                    comment: String::new(),
                    range: Default::default(),
                });
            }
            let vals = params::evaluate(&all, doc.units);
            let value = |what: &str, dim: Dim| -> Result<f64, CommandError> {
                match vals.get(&nm.n(what)) {
                    // The value is written into a parameter as given, so its kind must be
                    // right as it stands: no bare number where an angle or a length is meant.
                    Some(Ok(q)) if q.dim == dim => Ok(q.value),
                    Some(Ok(q)) => {
                        let hint = if dim == Dim::LENGTH {
                            "write the unit, for example 2 mm"
                        } else if dim == Dim::ANGLE {
                            "write the unit, for example 20 deg"
                        } else {
                            "give a plain number"
                        };
                        Err(CommandError::args(format!("{what} must be {}, not {}", dim.name(), q.dim.name()))
                            .hint(hint))
                    }
                    Some(Err(e)) => Err(CommandError::args(format!("{what}: {}", e.message))),
                    None => Err(CommandError::args(format!("{what} is missing"))),
                }
            };
            let teeth = value("teeth", Dim::NONE)?;
            if !teeth.is_finite() || (teeth - teeth.round()).abs() > 1e-9 || teeth < 0.0 {
                return Err(CommandError::args(format!("teeth is {teeth}, not a whole number"))
                    .hint("give the number of teeth, for example 24"));
            }
            let v = GearValues {
                module: value("module", Dim::LENGTH)?,
                teeth: teeth.round() as u32,
                pressure_angle: value("pressure_angle", Dim::ANGLE)?,
                shift: value("shift", Dim::NONE)?,
                backlash: value("backlash", Dim::LENGTH)?,
                fillet: value("fillet", Dim::NONE)?,
                helix: value("helix", Dim::ANGLE)?,
                face_width: value("face_width", Dim::LENGTH)?,
                bore: value("bore", Dim::LENGTH)?,
            };
            let helical = v.helix != 0.0;
            if !(v.face_width > 0.0 && v.face_width.is_finite()) {
                return Err(
                    CommandError::args("the face width must be positive").hint("give the gear a width such as 10 mm")
                );
            }
            let g = gear::Geometry::new(&v).map_err(|e| CommandError::args(e.message).hint(e.hint))?;
            gear::check_bore(&v, &g).map_err(|e| CommandError::args(e.message).hint(e.hint))?;
            let mut warnings: Vec<String> = Vec::new();
            if g.tip_land() < gear::NARROW_TIP_LAND * v.module {
                warnings.push(format!(
                    "the tip land is only {:.3} mm ({:.2} x module): narrow tips chip easily, especially on hardened \
                     gears; a smaller profile shift or less backlash widens it",
                    g.tip_land(),
                    g.tip_land() / v.module
                ));
            }
            let flank = gear::choose_flank(&g).map_err(|e| CommandError::args(e.message).hint(e.hint))?;
            let sketch = gear::profile_sketch(&nm, &g, &flank, v.bore / 2.0)
                .map_err(|e| CommandError::args(e.message).hint(e.hint))?;

            // Parameters: the options, then the derived dimensions.
            let mut new_params: Vec<Param> = Vec::new();
            let notes: [(&str, &str); 9] = [
                ("module", "normal module"),
                ("teeth", "number of teeth"),
                ("pressure_angle", "normal pressure angle (ISO 53: 20 deg)"),
                ("face_width", "thickness of the gear"),
                ("bore", "bore diameter"),
                ("shift", "profile shift coefficient x"),
                ("backlash", "tooth thinning at the pitch circle"),
                ("fillet", "root fillet radius / module (ISO 53: 0.38)"),
                ("helix", "helix angle at the pitch cylinder"),
            ];
            for (what, e) in &exprs {
                if *what == "helix" && !helical {
                    continue;
                }
                let note = notes.iter().find(|(w, _)| w == what).map_or("", |(_, n)| n);
                new_params.push(Param {
                    name: nm.n(what),
                    expr: e.clone(),
                    comment: note.to_string(),
                    range: Default::default(),
                });
            }
            for (pname, src, note) in gear::derived_params(&nm, helical) {
                let expr = parse_expr(&pname, &src)?;
                new_params.push(Param { name: pname, expr, comment: note.to_string(), range: Default::default() });
            }
            for p in &new_params {
                if doc.params.iter().any(|q| q.name == p.name) {
                    return Err(CommandError::args(format!("a parameter named '{}' already exists", p.name))
                        .hint("name the gear differently, for example name: \"Pinion\""));
                }
                validate_feature_name(&p.name).map_err(CommandError::args)?;
            }
            for f in [&sketch_name, &path_name] {
                if doc.feature(f).is_some() && (f == &sketch_name || helical) {
                    return Err(CommandError::args(format!("a feature named '{f}' already exists"))
                        .hint("name the gear differently"));
                }
            }
            let created: Vec<String> = new_params.iter().map(|p| p.name.clone()).collect();
            doc.params.extend(new_params);

            let mut features = vec![Feature::new(sketch_name.clone(), FeatureKind::Sketch(sketch.def))];
            let profile = ProfileRef { sketch: sketch_name.clone(), regions: vec![sketch.region] };
            let width = parse_expr("face_width", &nm.n("face_width"))?;
            if helical {
                features.push(Feature::new(path_name.clone(), FeatureKind::Sketch(path_sketch(&nm, v.face_width)?)));
                let twist = format!(
                    "{} * tan({}) / ({} / 2) * 1 rad",
                    nm.n("face_width"),
                    nm.n("helix"),
                    nm.n("pitch_diameter")
                );
                features.push(Feature::new(
                    name.clone(),
                    FeatureKind::Sweep(SweepDef {
                        profile,
                        path: PathRef::Sketch { sketch: path_name.clone(), curves: Vec::new() },
                        orientation: Orientation::Frenet,
                        twist: Some(parse_expr("twist", &twist)?),
                        op: BodyOp::New,
                        body: None,
                        body_name: None,
                    }),
                ));
            } else {
                features.push(Feature::new(
                    name.clone(),
                    FeatureKind::Extrude(ExtrudeDef {
                        profile,
                        distance: width,
                        symmetric: false,
                        op: BodyOp::New,
                        body: None,
                        body_name: None,
                    }),
                ));
            }
            let mut at = rb;
            for f in features {
                insert_feature(doc, at, f);
                at = at.map(|r| r + 1);
            }
            Ok(json!({
                "feature": name,
                "body": name,
                "sketch": sketch_name,
                "helical": helical,
                "parameters": created,
                "pitch_diameter_mm": 2.0 * g.pitch_radius,
                "base_diameter_mm": 2.0 * g.base_radius,
                "tip_diameter_mm": 2.0 * g.tip_radius,
                "root_diameter_mm": 2.0 * g.root_radius,
                "flank_points": flank.points.len(),
                "flank_error_mm": flank.error,
                "warnings": warnings,
            }))
        })
    }
}

/// Generates a helical compression spring of round wire along Z (ADR 0018 amendment).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateSpring {
    #[serde(default = "d_wire")]
    #[schemars(extend("x-essential" = true))]
    pub wire: String,
    /// Mean coil diameter.
    #[serde(default = "d_mean")]
    #[schemars(extend("x-essential" = true))]
    pub diameter: String,
    /// Axial distance per turn.
    #[serde(default = "d_pitch")]
    #[schemars(extend("x-essential" = true))]
    pub pitch: String,
    #[serde(default = "d_turns")]
    #[schemars(extend("x-essential" = true))]
    pub turns: String,
    #[serde(default)]
    pub name: Option<String>,
}

fn d_wire() -> String {
    "1.5 mm".into()
}
fn d_mean() -> String {
    "12 mm".into()
}
fn d_pitch() -> String {
    "4 mm".into()
}
fn d_turns() -> String {
    "6".into()
}

impl Command for GenerateSpring {
    const ID: &'static str = "generate.spring";
    const TITLE: &'static str = "Spring";
    const DESCRIPTION: &'static str = "Generate a helical spring (round wire along Z) as named parameters, a wire sketch, an axis and a twisted sweep.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let given =
            [("wire", &self.wire), ("diameter", &self.diameter), ("pitch", &self.pitch), ("turns", &self.turns)];
        let mut exprs: Vec<(&str, gusset_units::expr::Expr)> = Vec::new();
        for (what, src) in given {
            exprs.push((what, parse_expr(what, src)?));
        }
        edit(s, ctx, "Generate spring", |doc, rb| {
            let name = feature_name(doc, self.name, "Spring")?;
            let nm = Names::from_feature(&name);
            let (sketch_name, path_name) = (format!("{name}_wire"), format!("{name}_axis"));
            let mut all = doc.params.clone();
            for (what, e) in &exprs {
                all.push(Param {
                    name: nm.n(what),
                    expr: e.clone(),
                    comment: String::new(),
                    range: Default::default(),
                });
            }
            let vals = params::evaluate(&all, doc.units);
            let value = |what: &str, dim: Dim| -> Result<f64, CommandError> {
                match vals.get(&nm.n(what)) {
                    Some(Ok(q)) if q.dim == dim => Ok(q.value),
                    Some(Ok(q)) => {
                        Err(CommandError::args(format!("{what} must be {}, not {}", dim.name(), q.dim.name())))
                    }
                    Some(Err(e)) => Err(CommandError::args(format!("{what}: {}", e.message))),
                    None => Err(CommandError::args(format!("{what} is missing"))),
                }
            };
            let (wire, mean, pitch, turns) = (
                value("wire", Dim::LENGTH)?,
                value("diameter", Dim::LENGTH)?,
                value("pitch", Dim::LENGTH)?,
                value("turns", Dim::NONE)?,
            );
            if !(wire > 0.0 && mean > wire && turns > 0.0 && turns <= 200.0 && pitch.is_finite()) {
                return Err(CommandError::args("a spring needs wire > 0, diameter > wire and 0 < turns <= 200")
                    .hint("for example wire 1.5 mm, diameter 12 mm, 6 turns"));
            }
            // Coils that touch make a self-intersecting sweep.
            if pitch <= wire * 1.05 {
                return Err(CommandError::args(format!("the pitch ({pitch} mm) must exceed the wire ({wire} mm)"))
                    .hint("a closed-coil spring is not supported; use a pitch a little over the wire"));
            }
            let notes = [
                ("wire", "wire diameter"),
                ("diameter", "mean coil diameter"),
                ("pitch", "axial distance per turn"),
                ("turns", "number of turns"),
            ];
            let mut new_params: Vec<Param> = Vec::new();
            for (what, e) in &exprs {
                let note = notes.iter().find(|(w, _)| w == what).map_or("", |(_, n)| n);
                new_params.push(Param {
                    name: nm.n(what),
                    expr: e.clone(),
                    comment: note.into(),
                    range: Default::default(),
                });
            }
            let length = format!("{} * {}", nm.n("pitch"), nm.n("turns"));
            new_params.push(Param {
                name: nm.n("length"),
                expr: parse_expr("length", &length)?,
                comment: "free length".into(),
                range: Default::default(),
            });
            for p in &new_params {
                if doc.params.iter().any(|q| q.name == p.name) {
                    return Err(CommandError::args(format!("a parameter named '{}' already exists", p.name))
                        .hint("name the spring differently"));
                }
                validate_feature_name(&p.name).map_err(CommandError::args)?;
            }
            for f in [&sketch_name, &path_name] {
                if doc.feature(f).is_some() {
                    return Err(CommandError::args(format!("a feature named '{f}' already exists"))
                        .hint("name the spring differently"));
                }
            }
            let created: Vec<String> = new_params.iter().map(|p| p.name.clone()).collect();
            doc.params.extend(new_params);
            let c = |id: &str, kind, ents: &[&str], v: Option<String>| -> Result<SkConstraint, CommandError> {
                Ok(SkConstraint {
                    id: id.into(),
                    kind,
                    entities: ents.iter().map(|e| (*e).to_string()).collect(),
                    value: v.map(|v| parse_expr("spring", &v)).transpose()?,
                })
            };
            // The wire's section in the plane of the coil axis' foot (the sweep turns it).
            let wire_sketch = SketchDef {
                plane: PlaneRef::XY,
                next_id: 6,
                entities: vec![
                    SkEntity { id: "p1".into(), geom: SkGeom::Point { x: mean / 2.0, y: 0.0 }, construction: false },
                    SkEntity {
                        id: "c2".into(),
                        geom: SkGeom::Circle { center: "p1".into(), radius: wire / 2.0 },
                        construction: false,
                    },
                ],
                constraints: vec![
                    c("k3", SkConstraintKind::AtX, &["p1"], Some(format!("{} / 2", nm.n("diameter"))))?,
                    c("k4", SkConstraintKind::AtY, &["p1"], Some("0 mm".into()))?,
                    c("k5", SkConstraintKind::Diameter, &["c2"], Some(nm.n("wire")))?,
                ],
                patterns: Vec::new(),
                gear: None,
            };
            let twist = format!("{} * 360 deg", nm.n("turns"));
            let features = vec![
                Feature::new(sketch_name.clone(), FeatureKind::Sketch(wire_sketch)),
                Feature::new(path_name.clone(), FeatureKind::Sketch(axis_sketch(&nm.n("length"), pitch * turns)?)),
                Feature::new(
                    name.clone(),
                    FeatureKind::Sweep(SweepDef {
                        profile: ProfileRef { sketch: sketch_name.clone(), regions: Vec::new() },
                        path: PathRef::Sketch { sketch: path_name.clone(), curves: Vec::new() },
                        orientation: Orientation::Frenet,
                        twist: Some(parse_expr("twist", &twist)?),
                        op: BodyOp::New,
                        body: None,
                        body_name: None,
                    }),
                ),
            ];
            let mut at = rb;
            for f in features {
                insert_feature(doc, at, f);
                at = at.map(|r| r + 1);
            }
            let helix = libm::atan(pitch / (std::f64::consts::PI * mean)).to_degrees();
            Ok(json!({
                "feature": name,
                "body": name,
                "sketch": sketch_name,
                "parameters": created,
                "free_length_mm": pitch * turns,
                "helix_angle_deg": helix,
                // The section is the wire's circle across the axis, not across the wire.
                "note": "the wire section is round in the plane across the axis (exact for a small helix angle)",
            }))
        })
    }
}

/// A straight path along the gear axis, as long as the face width, driven by its parameter.
fn path_sketch(nm: &Names, face_width: f64) -> Result<SketchDef, CommandError> {
    axis_sketch(&nm.n("face_width"), face_width)
}

/// A straight path up the Z axis from the origin, `length` long, driven by parameter `param`.
fn axis_sketch(param: &str, length: f64) -> Result<SketchDef, CommandError> {
    let point = |id: &str, y: f64| SkEntity { id: id.into(), geom: SkGeom::Point { x: 0.0, y }, construction: false };
    let constraint = |id: &str,
                      kind: SkConstraintKind,
                      entities: &[&str],
                      value: Option<&str>|
     -> Result<SkConstraint, CommandError> {
        Ok(SkConstraint {
            id: id.into(),
            kind,
            entities: entities.iter().map(|e| (*e).to_string()).collect(),
            value: value.map(|v| parse_expr("face_width", v)).transpose()?,
        })
    };
    Ok(SketchDef {
        plane: PlaneRef::XZ,
        next_id: 7,
        entities: vec![
            point("p1", 0.0),
            point("p2", length),
            SkEntity { id: "l3".into(), geom: SkGeom::Line { a: "p1".into(), b: "p2".into() }, construction: false },
        ],
        constraints: vec![
            constraint("k4", SkConstraintKind::Fixed, &["p1"], None)?,
            constraint("k5", SkConstraintKind::Vertical, &["l3"], None)?,
            constraint("k6", SkConstraintKind::Distance, &["l3", "l3"], Some(param))?,
        ],
        patterns: Vec::new(),
        gear: None,
    })
}
