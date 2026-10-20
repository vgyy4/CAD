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
    vec![spec::<GenerateGear>(), spec::<GenerateSpring>(), spec::<GenerateThread>(), spec::<GenerateNut>()]
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

/// Generates an external ISO metric thread (ISO 68-1 basic profile) on a rod along Z
/// (ADR 0034).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateThread {
    /// Major (nominal) diameter.
    #[serde(default = "d_thread_d")]
    #[schemars(extend("x-essential" = true))]
    pub diameter: String,
    #[serde(default = "d_thread_p")]
    #[schemars(extend("x-essential" = true))]
    pub pitch: String,
    /// Rod length.
    #[serde(default = "d_thread_l")]
    #[schemars(extend("x-essential" = true))]
    pub length: String,
    /// 45 deg chamfer at the free end(s).
    #[serde(default = "d_true")]
    pub chamfer: bool,
    /// Head across flats (a bolt; M10: 16 mm).
    #[serde(default)]
    pub head_width: Option<String>,
    /// Head height (M10: 6.4 mm).
    #[serde(default)]
    pub head_height: Option<String>,
    /// Hex socket width: a round socket head (M10: 8 mm).
    #[serde(default)]
    pub socket: Option<String>,
    #[serde(default)]
    pub socket_depth: Option<String>,
    #[serde(default)]
    pub left: bool,
    #[serde(default)]
    pub name: Option<String>,
}

/// A regular hexagon about the origin, `width` across flats (`w` its parameter), with
/// corners at 30 + 60 i degrees, as sketch entities and constraints.
fn hexagon(w: &str, width: f64) -> Result<(Vec<SkEntity>, Vec<SkConstraint>), CommandError> {
    let mut entities = Vec::new();
    let mut constraints = Vec::new();
    for i in 0..6 {
        let a = (30.0 + 60.0 * i as f64).to_radians();
        let id = format!("p{}", i + 1);
        let rr = width / 3f64.sqrt();
        entities.push(SkEntity {
            id: id.clone(),
            geom: SkGeom::Point { x: rr * libm::cos(a), y: rr * libm::sin(a) },
            construction: false,
        });
        let deg = 30 + 60 * i;
        for (k, kind, e) in [
            (0, SkConstraintKind::AtX, format!("{w} / sqrt(3) * cos({deg} deg)")),
            (1, SkConstraintKind::AtY, format!("{w} / sqrt(3) * sin({deg} deg)")),
        ] {
            constraints.push(SkConstraint {
                id: format!("k{}", 30 + 2 * i + k),
                kind,
                entities: vec![id.clone()],
                value: Some(parse_expr("hexagon", &e)?),
            });
        }
    }
    for i in 0..6 {
        entities.push(SkEntity {
            id: format!("l{}", 7 + i),
            geom: SkGeom::Line { a: format!("p{}", i + 1), b: format!("p{}", (i + 1) % 6 + 1) },
            construction: false,
        });
    }
    Ok((entities, constraints))
}

fn d_true() -> bool {
    true
}

/// A 45 degree chamfer cut at one end of a threaded rod: a triangle in the XZ plane from the
/// minor radius at the end face out past the crests, revolved about Z.
fn chamfer_features(
    name: &str,
    nm: &Names,
    end: &str,
    top: bool,
    core: &str,
    vals: [f64; 4],
) -> Result<Vec<Feature>, CommandError> {
    let [minor_r, outer_r, z_end, _] = vals;
    let (d, p, l) = (nm.n("diameter"), nm.n("pitch"), nm.n("length"));
    let inner = format!("({d} - 1.0825317547 * {p}) / 2");
    let outer = format!("{d} / 2 + 0.1 * {p}");
    let depth = format!("({outer}) - ({inner})");
    let (zx, zi) = if top { (l.clone(), format!("{l} - ({depth})")) } else { ("0 mm".to_string(), depth.clone()) };
    let zi_v = if top { z_end - (outer_r - minor_r) } else { outer_r - minor_r };
    let pts = [
        (inner.clone(), zx.clone(), [minor_r, z_end]),
        (outer.clone(), zx, [outer_r, z_end]),
        (outer, zi, [outer_r, zi_v]),
    ];
    let mut entities = Vec::new();
    let mut constraints = Vec::new();
    for (i, (x, z, v)) in pts.iter().enumerate() {
        let id = format!("p{}", i + 1);
        entities.push(SkEntity { id: id.clone(), geom: SkGeom::Point { x: v[0], y: v[1] }, construction: false });
        for (k, kind, e) in [(0, SkConstraintKind::AtX, x), (1, SkConstraintKind::AtY, z)] {
            constraints.push(SkConstraint {
                id: format!("k{}", 10 + 2 * i + k),
                kind,
                entities: vec![id.clone()],
                value: Some(parse_expr("chamfer", e)?),
            });
        }
    }
    for i in 0..3 {
        entities.push(SkEntity {
            id: format!("l{}", 4 + i),
            geom: SkGeom::Line { a: format!("p{}", i + 1), b: format!("p{}", (i + 1) % 3 + 1) },
            construction: false,
        });
    }
    let sk = format!("{name}_{end}_chamfer_sketch");
    Ok(vec![
        Feature::new(
            sk.clone(),
            FeatureKind::Sketch(SketchDef {
                plane: PlaneRef::XZ,
                next_id: 20,
                entities,
                constraints,
                patterns: Vec::new(),
                gear: None,
            }),
        ),
        Feature::new(
            format!("{name}_{end}_chamfer"),
            FeatureKind::Revolve(RevolveDef {
                profile: ProfileRef { sketch: sk, regions: Vec::new() },
                axis: gusset_doc::AxisRef::Y,
                angle: parse_expr("angle", "360 deg")?,
                op: BodyOp::Cut,
                body: Some(core.to_string()),
                body_name: None,
            }),
        ),
    ])
}

fn d_thread_d() -> String {
    "10 mm".into()
}
fn d_thread_p() -> String {
    "1.5 mm".into()
}
fn d_thread_l() -> String {
    "20 mm".into()
}

/// How far the tooth reaches into the core, per pitch, so the two unite cleanly.
const TOOTH_OVERLAP: f64 = 0.02;

/// The tooth of the ISO 68-1 basic profile in the axial plane (x from the axis, z along it)
/// for major radius `r` and pitch `p`, as parameter expressions over `d` and `p` names, with
/// their values: root flat P/4 at the minor diameter, crest flat P/8, 60 degree flanks.
/// `start` shifts it along the axis, in pitches.
pub(crate) fn thread_tooth(d: &str, p: &str, dv: f64, pv: f64, start: f64) -> Vec<(String, String, [f64; 2])> {
    // The flanks run on into the core by TOOTH_OVERLAP * pitch, so no tooth edge lies on the
    // core's surface (the union then cuts cleanly across it).
    let minor_v = (dv - 1.082_531_754_7 * pv) / 2.0;
    let inner = format!("({d} - 1.0825317547 * {p}) / 2 - {TOOTH_OVERLAP} * {p}");
    let inner_v = minor_v - TOOTH_OVERLAP * pv;
    let major = format!("{d} / 2");
    // A 60 degree flank rises 5/16 P over 5/8 H = 0.5413 P of depth: dz/dx = tan(30 deg).
    let run = TOOTH_OVERLAP / 3f64.sqrt();
    let z = |k: f64| (format!("{} * {p}", k + start), (k + start) * pv);
    let pts = [
        // Shifted up by `run` so the tooth does not poke out below the rod.
        (inner.clone(), inner_v, z(0.0)),
        (major.clone(), dv / 2.0, z(5.0 / 16.0 + run)),
        (major, dv / 2.0, z(7.0 / 16.0 + run)),
        (inner, inner_v, z(0.75 + 2.0 * run)),
    ];
    pts.into_iter().map(|(x, xv, (zs, zv))| (x, zs, [xv, zv])).collect()
}

impl Command for GenerateThread {
    const ID: &'static str = "generate.thread";
    const TITLE: &'static str = "Thread";
    const DESCRIPTION: &'static str =
        "Generate a threaded rod, bolt or socket screw along Z (ISO 68-1 thread, exact helix).";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let socket = self.socket.is_some();
        let head = match (&self.head_width, &self.head_height) {
            (None, None) if !socket && self.socket_depth.is_none() => None,
            (Some(w), h) => Some((
                w.clone(),
                h.clone().unwrap_or_else(|| if socket { self.diameter.clone() } else { format!("0.4 * ({w})") }),
            )),
            _ => return Err(CommandError::args("head_height, socket and socket_depth need head_width")),
        };
        if self.socket_depth.is_some() && !socket {
            return Err(CommandError::args("socket_depth needs socket"));
        }
        let mut given =
            vec![("diameter", self.diameter.clone()), ("pitch", self.pitch.clone()), ("length", self.length.clone())];
        if let Some((w, h)) = &head {
            given.push(("head_width", w.clone()));
            given.push(("head_height", h.clone()));
        }
        if let Some(sw) = &self.socket {
            given.push(("socket", sw.clone()));
            given.push((
                "socket_depth",
                self.socket_depth.clone().unwrap_or_else(|| format!("0.5 * ({})", self.diameter)),
            ));
        }
        let mut exprs: Vec<(&str, gusset_units::expr::Expr)> = Vec::new();
        for (what, src) in given {
            exprs.push((what, parse_expr(what, &src)?));
        }
        edit(s, ctx, "Generate thread", |doc, rb| {
            let name = feature_name(doc, self.name, "Thread")?;
            let nm = Names::from_feature(&name);
            let (core_sk, core, tooth_sk) =
                (format!("{name}_core_sketch"), format!("{name}_core"), format!("{name}_tooth"));
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
            let value = |what: &str| -> Result<f64, CommandError> {
                match vals.get(&nm.n(what)) {
                    Some(Ok(q)) if q.dim == Dim::LENGTH => Ok(q.value),
                    Some(Ok(q)) => Err(CommandError::args(format!("{what} must be a length, not {}", q.dim.name()))),
                    Some(Err(e)) => Err(CommandError::args(format!("{what}: {}", e.message))),
                    None => Err(CommandError::args(format!("{what} is missing"))),
                }
            };
            let (d, p, len) = (value("diameter")?, value("pitch")?, value("length")?);
            if !(d > 0.0 && p > 0.0 && p < d / 2.0 && len > 2.0 * p) {
                return Err(CommandError::args(
                    "a thread needs 0 < pitch < diameter / 2 and a length over two pitches",
                )
                .hint("for example diameter 10 mm, pitch 1.5 mm, length 20 mm (M10 coarse)"));
            }
            if len / p > 2000.0 {
                return Err(CommandError::args("the thread has more than 2000 turns"));
            }
            let mut new_params: Vec<Param> = Vec::new();
            let notes = [
                ("diameter", "major (nominal) diameter"),
                ("pitch", "pitch"),
                ("length", "rod length"),
                ("head_width", "head width across flats"),
                ("head_height", "head height"),
                ("socket", "hex socket across flats"),
                ("socket_depth", "hex socket depth"),
            ];
            if head.is_some() {
                let (hw, hh) = (value("head_width")?, value("head_height")?);
                if !(hw > d && hh > 0.0) {
                    return Err(CommandError::args("the head must be wider than the thread and have a height")
                        .hint("ISO 4017 M10: head_width 16 mm, head_height 6.4 mm"));
                }
                if socket {
                    let (sw, sd) = (value("socket")?, value("socket_depth")?);
                    if !(sw > 0.0 && 2.0 * sw / 3f64.sqrt() < hw && sd > 0.0 && sd < hh) {
                        return Err(CommandError::args("the socket must fit inside the head and be shallower than it")
                            .hint(
                                "ISO 4762 M10: head_width 16 mm, head_height 10 mm, socket 8 mm, socket_depth 5 mm",
                            ));
                    }
                }
            }
            for (what, e) in &exprs {
                let note = notes.iter().find(|(w, _)| w == what).map_or("", |(_, n)| n);
                new_params.push(Param {
                    name: nm.n(what),
                    expr: e.clone(),
                    comment: note.into(),
                    range: Default::default(),
                });
            }
            let minor = format!("{} - 1.0825317547 * {}", nm.n("diameter"), nm.n("pitch"));
            new_params.push(Param {
                name: nm.n("minor"),
                expr: parse_expr("minor", &minor)?,
                comment: "minor diameter (basic profile)".into(),
                range: Default::default(),
            });
            for p in &new_params {
                if doc.params.iter().any(|q| q.name == p.name) {
                    return Err(CommandError::args(format!("a parameter named '{}' already exists", p.name))
                        .hint("name the thread differently"));
                }
                validate_feature_name(&p.name).map_err(CommandError::args)?;
            }
            for f in [&core_sk, &core, &tooth_sk] {
                if doc.feature(f).is_some() {
                    return Err(CommandError::args(format!("a feature named '{f}' already exists"))
                        .hint("name the thread differently"));
                }
            }
            let created: Vec<String> = new_params.iter().map(|p| p.name.clone()).collect();
            doc.params.extend(new_params);
            let c = |id: String, kind, ents: &[&str], v: &str| -> Result<SkConstraint, CommandError> {
                Ok(SkConstraint {
                    id,
                    kind,
                    entities: ents.iter().map(|e| (*e).to_string()).collect(),
                    value: Some(parse_expr("thread", v)?),
                })
            };
            let ent = |id: &str, geom| SkEntity { id: id.into(), geom, construction: false };
            // The core: a circle of the minor diameter at the origin.
            let core_sketch = SketchDef {
                plane: PlaneRef::XY,
                next_id: 6,
                entities: vec![
                    ent("p1", SkGeom::Point { x: 0.0, y: 0.0 }),
                    ent("c2", SkGeom::Circle { center: "p1".into(), radius: (d - 1.082_531_754_7 * p) / 2.0 }),
                ],
                constraints: vec![
                    c("k3".into(), SkConstraintKind::AtX, &["p1"], "0 mm")?,
                    c("k4".into(), SkConstraintKind::AtY, &["p1"], "0 mm")?,
                    c("k5".into(), SkConstraintKind::Diameter, &["c2"], &nm.n("minor"))?,
                ],
                patterns: Vec::new(),
                gear: None,
            };
            // The tooth in the XZ plane: six points, each placed by its parameters.
            let tooth = thread_tooth(&nm.n("diameter"), &nm.n("pitch"), d, p, 0.0);
            let mut entities = Vec::new();
            let mut constraints = Vec::new();
            for (i, (x, z, v)) in tooth.iter().enumerate() {
                let id = format!("p{}", i + 1);
                entities.push(ent(&id, SkGeom::Point { x: v[0], y: v[1] }));
                constraints.push(c(format!("k{}", 20 + 2 * i), SkConstraintKind::AtX, &[&id], x)?);
                constraints.push(c(format!("k{}", 21 + 2 * i), SkConstraintKind::AtY, &[&id], z)?);
            }
            for i in 0..tooth.len() {
                let (a, b) = (format!("p{}", i + 1), format!("p{}", (i + 1) % tooth.len() + 1));
                entities.push(ent(&format!("l{}", 7 + i), SkGeom::Line { a, b }));
            }
            let tooth_sketch =
                SketchDef { plane: PlaneRef::XZ, next_id: 40, entities, constraints, patterns: Vec::new(), gear: None };
            // The helix stays inside the rod: its last turn's tooth ends 3/4 pitch above it.
            let height = format!("{} - {}", nm.n("length"), nm.n("pitch"));
            let features = vec![
                Feature::new(core_sk.clone(), FeatureKind::Sketch(core_sketch)),
                Feature::new(
                    core.clone(),
                    FeatureKind::Extrude(ExtrudeDef {
                        profile: ProfileRef { sketch: core_sk.clone(), regions: Vec::new() },
                        distance: parse_expr("length", &nm.n("length"))?,
                        symmetric: false,
                        op: BodyOp::New,
                        body: None,
                        body_name: Some(name.clone()),
                    }),
                ),
                Feature::new(tooth_sk.clone(), FeatureKind::Sketch(tooth_sketch)),
                Feature::new(
                    format!("{name}_helix"),
                    FeatureKind::Sweep(SweepDef {
                        profile: ProfileRef { sketch: tooth_sk.clone(), regions: Vec::new() },
                        path: PathRef::Helix(gusset_doc::HelixDef {
                            axis: gusset_doc::AxisRef::Z,
                            pitch: parse_expr("pitch", &nm.n("pitch"))?,
                            height: parse_expr("height", &height)?,
                            left: self.left,
                        }),
                        orientation: Orientation::Frenet,
                        twist: None,
                        op: BodyOp::Add,
                        body: Some(core.clone()),
                        body_name: None,
                    }),
                ),
            ];
            let mut features = features;
            if head.is_some() {
                // The head below the rod, from z = 0 down by head_height: a hexagon, or a
                // circle for a socket head.
                let (entities, constraints) = if socket {
                    let hw = value("head_width")?;
                    (
                        vec![
                            ent("p1", SkGeom::Point { x: 0.0, y: 0.0 }),
                            ent("c2", SkGeom::Circle { center: "p1".into(), radius: hw / 2.0 }),
                        ],
                        vec![
                            c("k3".into(), SkConstraintKind::AtX, &["p1"], "0 mm")?,
                            c("k4".into(), SkConstraintKind::AtY, &["p1"], "0 mm")?,
                            c("k5".into(), SkConstraintKind::Diameter, &["c2"], &nm.n("head_width"))?,
                        ],
                    )
                } else {
                    hexagon(&nm.n("head_width"), value("head_width")?)?
                };
                let head_sk = format!("{name}_head_sketch");
                if doc.feature(&head_sk).is_some() {
                    return Err(CommandError::args(format!("a feature named '{head_sk}' already exists")));
                }
                features.push(Feature::new(
                    head_sk.clone(),
                    FeatureKind::Sketch(SketchDef {
                        plane: PlaneRef::XY,
                        next_id: 50,
                        entities,
                        constraints,
                        patterns: Vec::new(),
                        gear: None,
                    }),
                ));
                features.push(Feature::new(
                    format!("{name}_head"),
                    FeatureKind::Extrude(ExtrudeDef {
                        profile: ProfileRef { sketch: head_sk, regions: Vec::new() },
                        distance: parse_expr("head_height", &format!("-{}", nm.n("head_height")))?,
                        symmetric: false,
                        op: BodyOp::Add,
                        body: Some(core.clone()),
                        body_name: None,
                    }),
                ));
                if socket {
                    // The socket, cut up from the head's bottom face on a datum plane there.
                    let (plane, sock_sk) = (format!("{name}_socket_plane"), format!("{name}_socket_sketch"));
                    for f in [&plane, &sock_sk] {
                        if doc.feature(f).is_some() {
                            return Err(CommandError::args(format!("a feature named '{f}' already exists")));
                        }
                    }
                    features.push(Feature::new(
                        plane.clone(),
                        FeatureKind::Plane(PlaneDef {
                            base: PlaneRef::XY,
                            kind: PlaneKind::Offset(parse_expr("head_height", &format!("-{}", nm.n("head_height")))?),
                        }),
                    ));
                    let (entities, constraints) = hexagon(&nm.n("socket"), value("socket")?)?;
                    features.push(Feature::new(
                        sock_sk.clone(),
                        FeatureKind::Sketch(SketchDef {
                            plane: PlaneRef::Datum(plane),
                            next_id: 50,
                            entities,
                            constraints,
                            patterns: Vec::new(),
                            gear: None,
                        }),
                    ));
                    features.push(Feature::new(
                        format!("{name}_socket"),
                        FeatureKind::Extrude(ExtrudeDef {
                            profile: ProfileRef { sketch: sock_sk, regions: Vec::new() },
                            distance: parse_expr("socket_depth", &nm.n("socket_depth"))?,
                            symmetric: false,
                            op: BodyOp::Cut,
                            body: Some(core.clone()),
                            body_name: None,
                        }),
                    ));
                }
            }
            if self.chamfer {
                let minor_r = (d - 1.082_531_754_7 * p) / 2.0;
                let outer_r = d / 2.0 + 0.1 * p;
                features.extend(chamfer_features(&name, &nm, "top", true, &core, [minor_r, outer_r, len, 0.0])?);
                if head.is_none() {
                    features.extend(chamfer_features(
                        &name,
                        &nm,
                        "bottom",
                        false,
                        &core,
                        [minor_r, outer_r, 0.0, 0.0],
                    )?);
                }
            }
            let mut at = rb;
            for f in features {
                insert_feature(doc, at, f);
                at = at.map(|r| r + 1);
            }
            Ok(json!({
                "feature": format!("{name}_helix"),
                "body": name,
                "parameters": created,
                "minor_diameter_mm": d - 1.082_531_754_7 * p,
                "turns": (len - p) / p,
            }))
        })
    }
}

/// Generates a hex nut with an internal ISO metric thread along Z (ADR 0034).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateNut {
    /// Thread major (nominal) diameter.
    #[serde(default = "d_thread_d")]
    #[schemars(extend("x-essential" = true))]
    pub diameter: String,
    #[serde(default = "d_thread_p")]
    #[schemars(extend("x-essential" = true))]
    pub pitch: String,
    /// Width across flats (ISO 4032 M10: 16 mm).
    #[serde(default = "d_nut_s")]
    #[schemars(extend("x-essential" = true))]
    pub width: String,
    #[serde(default = "d_nut_m")]
    #[schemars(extend("x-essential" = true))]
    pub height: String,
    #[serde(default)]
    pub left: bool,
    #[serde(default)]
    pub name: Option<String>,
}

fn d_nut_s() -> String {
    "16 mm".into()
}
fn d_nut_m() -> String {
    "8.4 mm".into()
}

impl Command for GenerateNut {
    const ID: &'static str = "generate.nut";
    const TITLE: &'static str = "Nut";
    const DESCRIPTION: &'static str = "Generate a hex nut along Z with an internal ISO metric thread (exact helix).";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let given =
            [("diameter", &self.diameter), ("pitch", &self.pitch), ("width", &self.width), ("height", &self.height)];
        let mut exprs: Vec<(&str, gusset_units::expr::Expr)> = Vec::new();
        for (what, src) in given {
            exprs.push((what, parse_expr(what, src)?));
        }
        edit(s, ctx, "Generate nut", |doc, rb| {
            let name = feature_name(doc, self.name, "Nut")?;
            let nm = Names::from_feature(&name);
            let (hex_sk, hex, groove_sk) =
                (format!("{name}_hex_sketch"), format!("{name}_hex"), format!("{name}_groove"));
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
            let value = |what: &str| -> Result<f64, CommandError> {
                match vals.get(&nm.n(what)) {
                    Some(Ok(q)) if q.dim == Dim::LENGTH => Ok(q.value),
                    Some(Ok(q)) => Err(CommandError::args(format!("{what} must be a length, not {}", q.dim.name()))),
                    Some(Err(e)) => Err(CommandError::args(format!("{what}: {}", e.message))),
                    None => Err(CommandError::args(format!("{what} is missing"))),
                }
            };
            let (d, p, w, m) = (value("diameter")?, value("pitch")?, value("width")?, value("height")?);
            if !(d > 0.0 && p > 0.0 && p < d / 2.0 && w > d * 1.1 && m > p) {
                return Err(CommandError::args(
                    "a nut needs 0 < pitch < diameter / 2, width over 1.1 x diameter and height over a pitch",
                )
                .hint("for example M10: diameter 10 mm, pitch 1.5 mm, width 16 mm, height 8.4 mm"));
            }
            let mut new_params: Vec<Param> = Vec::new();
            let notes = [
                ("diameter", "thread major (nominal) diameter"),
                ("pitch", "pitch"),
                ("width", "width across flats"),
                ("height", "nut height"),
            ];
            for (what, e) in &exprs {
                let note = notes.iter().find(|(w, _)| w == what).map_or("", |(_, n)| n);
                new_params.push(Param {
                    name: nm.n(what),
                    expr: e.clone(),
                    comment: note.into(),
                    range: Default::default(),
                });
            }
            let minor = format!("{} - 1.0825317547 * {}", nm.n("diameter"), nm.n("pitch"));
            new_params.push(Param {
                name: nm.n("minor"),
                expr: parse_expr("minor", &minor)?,
                comment: "thread minor diameter (basic profile)".into(),
                range: Default::default(),
            });
            for p in &new_params {
                if doc.params.iter().any(|q| q.name == p.name) {
                    return Err(CommandError::args(format!("a parameter named '{}' already exists", p.name))
                        .hint("name the nut differently"));
                }
                validate_feature_name(&p.name).map_err(CommandError::args)?;
            }
            for f in [&hex_sk, &hex, &groove_sk] {
                if doc.feature(f).is_some() {
                    return Err(CommandError::args(format!("a feature named '{f}' already exists"))
                        .hint("name the nut differently"));
                }
            }
            let created: Vec<String> = new_params.iter().map(|p| p.name.clone()).collect();
            doc.params.extend(new_params);
            let c = |id: String, kind, ents: &[&str], v: &str| -> Result<SkConstraint, CommandError> {
                Ok(SkConstraint {
                    id,
                    kind,
                    entities: ents.iter().map(|e| (*e).to_string()).collect(),
                    value: Some(parse_expr("nut", v)?),
                })
            };
            let ent = |id: &str, geom| SkEntity { id: id.into(), geom, construction: false };
            // The hexagon (corners on the circle of radius width / sqrt(3)) and the hole.
            let mut entities = Vec::new();
            let mut constraints = Vec::new();
            for i in 0..6 {
                let a = (30.0 + 60.0 * i as f64).to_radians();
                let rr = w / 3f64.sqrt();
                let id = format!("p{}", i + 1);
                entities.push(ent(&id, SkGeom::Point { x: rr * libm::cos(a), y: rr * libm::sin(a) }));
                let deg = 30 + 60 * i;
                constraints.push(c(
                    format!("k{}", 30 + 2 * i),
                    SkConstraintKind::AtX,
                    &[&id],
                    &format!("{} / sqrt(3) * cos({deg} deg)", nm.n("width")),
                )?);
                constraints.push(c(
                    format!("k{}", 31 + 2 * i),
                    SkConstraintKind::AtY,
                    &[&id],
                    &format!("{} / sqrt(3) * sin({deg} deg)", nm.n("width")),
                )?);
            }
            for i in 0..6 {
                entities.push(ent(
                    &format!("l{}", 7 + i),
                    SkGeom::Line { a: format!("p{}", i + 1), b: format!("p{}", (i + 1) % 6 + 1) },
                ));
            }
            entities.push(ent("p13", SkGeom::Point { x: 0.0, y: 0.0 }));
            entities.push(ent("c14", SkGeom::Circle { center: "p13".into(), radius: (d - 1.082_531_754_7 * p) / 2.0 }));
            constraints.push(c("k15".into(), SkConstraintKind::AtX, &["p13"], "0 mm")?);
            constraints.push(c("k16".into(), SkConstraintKind::AtY, &["p13"], "0 mm")?);
            constraints.push(c("k17".into(), SkConstraintKind::Diameter, &["c14"], &nm.n("minor"))?);
            let hex_sketch =
                SketchDef { plane: PlaneRef::XY, next_id: 50, entities, constraints, patterns: Vec::new(), gear: None };
            // The groove: the bolt's tooth, started a pitch below the nut and run a pitch past
            // its top, cut out of the hole's wall.
            let tooth = thread_tooth(&nm.n("diameter"), &nm.n("pitch"), d, p, -1.0);
            let mut entities = Vec::new();
            let mut constraints = Vec::new();
            for (i, (x, z, v)) in tooth.iter().enumerate() {
                let id = format!("p{}", i + 1);
                entities.push(ent(&id, SkGeom::Point { x: v[0], y: v[1] }));
                constraints.push(c(format!("k{}", 20 + 2 * i), SkConstraintKind::AtX, &[&id], x)?);
                constraints.push(c(format!("k{}", 21 + 2 * i), SkConstraintKind::AtY, &[&id], z)?);
            }
            for i in 0..tooth.len() {
                let (a, b) = (format!("p{}", i + 1), format!("p{}", (i + 1) % tooth.len() + 1));
                entities.push(ent(&format!("l{}", 7 + i), SkGeom::Line { a, b }));
            }
            let groove_sketch =
                SketchDef { plane: PlaneRef::XZ, next_id: 40, entities, constraints, patterns: Vec::new(), gear: None };
            let height = format!("{} + {}", nm.n("height"), nm.n("pitch"));
            let features = vec![
                Feature::new(hex_sk.clone(), FeatureKind::Sketch(hex_sketch)),
                Feature::new(
                    hex.clone(),
                    FeatureKind::Extrude(ExtrudeDef {
                        profile: ProfileRef { sketch: hex_sk.clone(), regions: Vec::new() },
                        distance: parse_expr("height", &nm.n("height"))?,
                        symmetric: false,
                        op: BodyOp::New,
                        body: None,
                        body_name: Some(name.clone()),
                    }),
                ),
                Feature::new(groove_sk.clone(), FeatureKind::Sketch(groove_sketch)),
                Feature::new(
                    format!("{name}_thread"),
                    FeatureKind::Sweep(SweepDef {
                        profile: ProfileRef { sketch: groove_sk.clone(), regions: Vec::new() },
                        path: PathRef::Helix(gusset_doc::HelixDef {
                            axis: gusset_doc::AxisRef::Z,
                            pitch: parse_expr("pitch", &nm.n("pitch"))?,
                            height: parse_expr("height", &height)?,
                            left: self.left,
                        }),
                        orientation: Orientation::Frenet,
                        twist: None,
                        op: BodyOp::Cut,
                        body: Some(hex.clone()),
                        body_name: None,
                    }),
                ),
            ];
            let mut at = rb;
            for f in features {
                insert_feature(doc, at, f);
                at = at.map(|r| r + 1);
            }
            Ok(json!({ "feature": format!("{name}_thread"), "body": name, "parameters": created }))
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
