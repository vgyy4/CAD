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
    vec![
        spec::<GenerateGear>(),
        spec::<GenerateRack>(),
        spec::<GenerateWorm>(),
        spec::<GenerateSpring>(),
        spec::<GenerateThread>(),
        spec::<GenerateNut>(),
        spec::<QueryGearPair>(),
    ]
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
    /// Module.
    #[serde(default = "d_module")]
    #[schemars(extend("x-essential" = true))]
    pub module: String,
    /// Tooth count, 6 to 500.
    #[serde(default = "d_teeth")]
    #[schemars(extend("x-essential" = true))]
    pub teeth: String,
    /// Pressure angle.
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
    /// Profile shift x.
    #[serde(default = "d_zero")]
    pub shift: String,
    /// Tooth thinning.
    #[serde(default = "d_zero_mm")]
    pub backlash: String,
    /// Root fillet / module.
    #[serde(default = "d_fillet")]
    pub fillet: String,
    /// Helix angle (0: spur).
    #[serde(default = "d_zero_deg")]
    pub helix: String,
    /// Pitch cone angle: a bevel gear.
    #[serde(default)]
    pub cone_angle: Option<String>,
    /// Ring diameter: an internal gear.
    #[serde(default)]
    pub internal_rim: Option<String>,
    /// Feature name (parameter prefix).
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for GenerateGear {
    const ID: &'static str = "generate.gear";
    const TITLE: &'static str = "Gear";
    const DESCRIPTION: &'static str = "Generate an involute spur, helical, bevel or internal gear (ISO 53).";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        // An internal gear (ADR 0018 amendment): its tooth spaces are the teeth of an
        // external gear with profile shift +0.25 (tip at r + 1.25 m, root at r - m: the
        // addendum and dedendum swapped) thinned by 0.5 m tan(a) back to p / 2 at the pitch
        // circle, cut from a ring. The parameters describe that tooth-space gear.
        let internal = self.internal_rim.is_some();
        let (mut shift, mut backlash, mut bore) = (self.shift.clone(), self.backlash.clone(), self.bore.clone());
        if internal {
            if self.helix.trim() != "0 deg" && self.helix.trim() != "0" {
                return Err(CommandError::args("internal gears are spur only").hint("leave helix at 0 deg"));
            }
            if self.shift.trim() != "0" {
                return Err(CommandError::args("internal gears take no profile shift").hint("leave shift at 0"));
            }
            shift = "0.25".into();
            backlash = format!("0.5 * ({}) * tan({}) - ({})", self.module, self.pressure_angle, self.backlash);
            bore = "0 mm".into();
        }
        let rim = self.internal_rim.clone().unwrap_or_default();
        let bevel = self.cone_angle.is_some();
        if bevel {
            if internal {
                return Err(CommandError::args("a gear is bevel or internal, not both"));
            }
            if self.helix.trim() != "0 deg" && self.helix.trim() != "0" {
                return Err(CommandError::args("bevel gears are straight only").hint("leave helix at 0 deg"));
            }
            // The loft takes one region per profile: a bore is cut afterwards.
            bore = "0 mm".into();
        }
        let given = [
            ("module", &self.module),
            ("teeth", &self.teeth),
            ("pressure_angle", &self.pressure_angle),
            ("face_width", &self.face_width),
            ("bore", &bore),
            ("shift", &shift),
            ("backlash", &backlash),
            ("fillet", &self.fillet),
            ("helix", &self.helix),
        ];
        let mut exprs: Vec<(&str, gusset_units::expr::Expr)> = Vec::new();
        for (what, src) in given {
            exprs.push((what, parse_expr(what, src)?));
        }
        if internal {
            exprs.push(("rim", parse_expr("internal_rim", &rim)?));
        }
        if let Some(c) = &self.cone_angle {
            exprs.push(("cone_angle", parse_expr("cone_angle", c)?));
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
            let notes: Vec<(&str, &str)> =
                notes.into_iter().chain([("cone_angle", "bevel gear: pitch cone angle (heel profile here)")]).collect();
            let internal_notes = [
                ("shift", "internal gear: its tooth spaces are this tooth (x = 0.25: tip and root swapped)"),
                ("backlash", "internal gear: tooth thinning that brings the space back to p / 2, less the backlash"),
                ("rim", "outer diameter of the ring"),
                ("bore", "internal gear: none (the ring is the gear)"),
            ];
            for (what, e) in &exprs {
                if *what == "helix" && !helical {
                    continue;
                }
                let note = internal_notes
                    .iter()
                    .filter(|_| internal)
                    .chain(notes.iter())
                    .find(|(w, _)| w == what)
                    .map_or("", |(_, n)| n);
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
            if bevel {
                // A straight bevel gear: the heel profile here, a toe profile scaled to the
                // cone apex on a plane b cos(d) up, and a ruled loft between them (ADR 0018
                // amendment). The toe has its own parameter set so its sketch is checked
                // like any generated gear.
                let delta = value("cone_angle", Dim::ANGLE)?;
                let k = 1.0 - v.face_width * libm::sin(delta) / g.pitch_radius;
                if !(delta > 0.0 && delta < std::f64::consts::FRAC_PI_2 && k >= 0.5) {
                    return Err(CommandError::args(
                        "a bevel gear needs a cone angle between 0 and 90 deg and a face width at most half the cone distance",
                    )
                    .hint("the usual limit is a third of the cone distance: pitch radius / sin(cone angle) / 3"));
                }
                let toe_name = format!("{name}_toe");
                let nt = Names::from_feature(&toe_name);
                let (toe_plane, toe_sk) = (format!("{name}_toe_plane"), format!("{name}_toe_profile"));
                for f in [&toe_plane, &toe_sk] {
                    if doc.feature(f).is_some() {
                        return Err(CommandError::args(format!("a feature named '{f}' already exists")));
                    }
                }
                let kx = format!(
                    "(1 - {} * sin({}) / ({} / 2))",
                    nm.n("face_width"),
                    nm.n("cone_angle"),
                    nm.n("pitch_diameter")
                );
                let toe_src = [
                    ("module", format!("{} * {kx}", nm.n("module"))),
                    ("teeth", nm.n("teeth")),
                    ("pressure_angle", nm.n("pressure_angle")),
                    ("face_width", nm.n("face_width")),
                    ("bore", "0 mm".to_string()),
                    ("shift", nm.n("shift")),
                    ("backlash", format!("{} * {kx}", nm.n("backlash"))),
                    ("fillet", nm.n("fillet")),
                ];
                let mut toe_params = Vec::new();
                for (what, src) in &toe_src {
                    toe_params.push(Param {
                        name: nt.n(what),
                        expr: parse_expr(what, src)?,
                        comment: "bevel gear toe (scaled toward the cone apex)".into(),
                        range: Default::default(),
                    });
                }
                for (pname, src, note) in gear::derived_params(&nt, false) {
                    toe_params.push(Param {
                        name: pname.clone(),
                        expr: parse_expr(&pname, &src)?,
                        comment: note.to_string(),
                        range: Default::default(),
                    });
                }
                for p in &toe_params {
                    if doc.params.iter().any(|q| q.name == p.name) {
                        return Err(CommandError::args(format!("a parameter named '{}' already exists", p.name)));
                    }
                }
                doc.params.extend(toe_params);
                let vt = GearValues { module: v.module * k, backlash: v.backlash * k, bore: 0.0, ..v };
                let gt = gear::Geometry::new(&vt).map_err(|e| CommandError::args(format!("toe: {}", e.message)))?;
                let ft = gear::flank_like(&gt, &flank, k);
                let mut toe = gear::profile_sketch(&nt, &gt, &ft, 0.0)
                    .map_err(|e| CommandError::args(format!("toe: {}", e.message)))?;
                toe.def.plane = PlaneRef::Datum(toe_plane.clone());
                features.push(Feature::new(
                    toe_plane.clone(),
                    FeatureKind::Plane(PlaneDef {
                        base: PlaneRef::XY,
                        kind: PlaneKind::Offset(parse_expr(
                            "toe",
                            &format!("{} * cos({})", nm.n("face_width"), nm.n("cone_angle")),
                        )?),
                    }),
                ));
                features.push(Feature::new(toe_sk.clone(), FeatureKind::Sketch(toe.def)));
                features.push(Feature::new(
                    name.clone(),
                    FeatureKind::Loft(LoftDef {
                        profiles: vec![profile, ProfileRef { sketch: toe_sk, regions: vec![toe.region] }],
                        ruled: true,
                        op: BodyOp::New,
                        body: None,
                        body_name: None,
                    }),
                ));
            } else if internal {
                let rim_d = value("rim", Dim::LENGTH)?;
                if rim_d < 2.0 * g.tip_radius + 2.0 * v.module {
                    return Err(CommandError::args(format!(
                        "the ring needs an outer diameter over {:.2} mm (tooth roots at {:.2} mm plus a wall of 1 module)",
                        2.0 * g.tip_radius + 2.0 * v.module,
                        2.0 * g.tip_radius
                    )));
                }
                let ring_sketch = format!("{name}_ring");
                if doc.feature(&ring_sketch).is_some() {
                    return Err(CommandError::args(format!("a feature named '{ring_sketch}' already exists")));
                }
                let c = |id: &str, kind, ents: &[&str], v: &str| -> Result<SkConstraint, CommandError> {
                    Ok(SkConstraint {
                        id: id.into(),
                        kind,
                        entities: ents.iter().map(|e| (*e).to_string()).collect(),
                        value: Some(parse_expr("ring", v)?),
                    })
                };
                let ring = SketchDef {
                    plane: PlaneRef::XY,
                    next_id: 6,
                    entities: vec![
                        SkEntity { id: "p1".into(), geom: SkGeom::Point { x: 0.0, y: 0.0 }, construction: false },
                        SkEntity {
                            id: "c2".into(),
                            geom: SkGeom::Circle { center: "p1".into(), radius: rim_d / 2.0 },
                            construction: false,
                        },
                    ],
                    constraints: vec![
                        c("k3", SkConstraintKind::AtX, &["p1"], "0 mm")?,
                        c("k4", SkConstraintKind::AtY, &["p1"], "0 mm")?,
                        c("k5", SkConstraintKind::Diameter, &["c2"], &nm.n("rim"))?,
                    ],
                    patterns: Vec::new(),
                    gear: None,
                };
                features.insert(0, Feature::new(ring_sketch.clone(), FeatureKind::Sketch(ring)));
                features.insert(
                    1,
                    Feature::new(
                        name.clone(),
                        FeatureKind::Extrude(ExtrudeDef {
                            profile: ProfileRef { sketch: ring_sketch, regions: Vec::new() },
                            distance: width.clone(),
                            symmetric: false,
                            op: BodyOp::New,
                            body: None,
                            body_name: None,
                        }),
                    ),
                );
                features.push(Feature::new(
                    format!("{name}_teeth"),
                    FeatureKind::Extrude(ExtrudeDef {
                        profile,
                        distance: width,
                        symmetric: false,
                        op: BodyOp::Cut,
                        body: Some(name.clone()),
                        body_name: None,
                    }),
                ));
            } else if helical {
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
                "internal": internal,
                "bevel": bevel,
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

/// Generates a ZA worm along Z: the basic-rack tooth (straight flanks at the pressure
/// angle in the axial section, axial pitch pi m, addendum m, dedendum 1.25 m) swept along
/// an exact helix of lead `starts` * pi m around a root-diameter core; a multi-start worm
/// sweeps `starts` teeth one axial pitch apart.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateWorm {
    /// Axial.
    #[serde(default = "d_module")]
    #[schemars(extend("x-essential" = true))]
    pub module: String,
    /// Default 10 m.
    #[serde(default)]
    pub diameter: Option<String>,
    #[serde(default = "d_thread_l")]
    #[schemars(extend("x-essential" = true))]
    pub length: String,
    #[serde(default = "d_angle")]
    pub angle: String,
    /// Starts, 1 to 6.
    #[serde(default = "d_one")]
    pub starts: u32,
    #[serde(default)]
    pub left: bool,
    #[serde(default)]
    pub name: Option<String>,
}

fn d_one() -> u32 {
    1
}

impl Command for GenerateWorm {
    const ID: &'static str = "generate.worm";
    const TITLE: &'static str = "Worm";
    const DESCRIPTION: &'static str = "Generate a ZA worm along Z (exact helix).";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let diameter = self.diameter.clone().unwrap_or_else(|| format!("10 * ({})", self.module));
        let given =
            [("module", &self.module), ("diameter", &diameter), ("length", &self.length), ("angle", &self.angle)];
        let mut exprs: Vec<(&str, gusset_units::expr::Expr)> = Vec::new();
        for (what, src) in given {
            exprs.push((what, parse_expr(what, src)?));
        }
        edit(s, ctx, "Generate worm", |doc, rb| {
            let name = feature_name(doc, self.name, "Worm")?;
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
            let (m, d1, len, alpha) = (
                value("module", Dim::LENGTH)?,
                value("diameter", Dim::LENGTH)?,
                value("length", Dim::LENGTH)?,
                value("angle", Dim::ANGLE)?,
            );
            let (pitch, t) = (std::f64::consts::PI * m, libm::tan(alpha));
            let root = d1 - 2.5 * m;
            // Half the tooth's axial width at the core (flanks run on into it, as threads do).
            let hw_core = pitch / 4.0 + (1.25 * m + TOOTH_OVERLAP * pitch) * t;
            let starts = self.starts;
            let span = 2.0 * hw_core + f64::from(starts.saturating_sub(1)) * pitch;
            if !(1..=6).contains(&starts) {
                return Err(CommandError::args("a worm has 1 to 6 starts"));
            }
            if !(m > 0.0 && root > 2.0 * m && alpha > 0.0 && pitch / 4.0 > m * t && len > span + pitch) {
                return Err(CommandError::args(
                    "a worm needs module > 0, a diameter over 4.5 modules, a pressure angle under about 38 deg and a length over two pitches",
                )
                .hint("for example module 2 mm, diameter 20 mm, length 30 mm"));
            }
            if len / pitch > 2000.0 {
                return Err(CommandError::args("the worm has more than 2000 turns"));
            }
            let notes = [
                ("module", "axial module"),
                ("diameter", "reference diameter"),
                ("length", "worm length"),
                ("angle", "axial pressure angle"),
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
            let (mn, dn, ln, an) = (nm.n("module"), nm.n("diameter"), nm.n("length"), nm.n("angle"));
            new_params.push(Param {
                name: nm.n("root"),
                expr: parse_expr("root", &format!("{dn} - 2.5 * {mn}"))?,
                comment: "root diameter".into(),
                range: Default::default(),
            });
            for p in &new_params {
                if doc.params.iter().any(|q| q.name == p.name) {
                    return Err(CommandError::args(format!("a parameter named '{}' already exists", p.name))
                        .hint("name the worm differently"));
                }
                validate_feature_name(&p.name).map_err(CommandError::args)?;
            }
            for f in [&core_sk, &core, &tooth_sk] {
                if doc.feature(f).is_some() {
                    return Err(CommandError::args(format!("a feature named '{f}' already exists"))
                        .hint("name the worm differently"));
                }
            }
            let created: Vec<String> = new_params.iter().map(|p| p.name.clone()).collect();
            doc.params.extend(new_params);
            let c = |id: String, kind, ents: &[&str], v: &str| -> Result<SkConstraint, CommandError> {
                Ok(SkConstraint {
                    id,
                    kind,
                    entities: ents.iter().map(|e| (*e).to_string()).collect(),
                    value: Some(parse_expr("worm", v)?),
                })
            };
            let ent = |id: &str, geom| SkEntity { id: id.into(), geom, construction: false };
            let core_sketch = SketchDef {
                plane: PlaneRef::XY,
                next_id: 6,
                entities: vec![
                    ent("p1", SkGeom::Point { x: 0.0, y: 0.0 }),
                    ent("c2", SkGeom::Circle { center: "p1".into(), radius: root / 2.0 }),
                ],
                constraints: vec![
                    c("k3".into(), SkConstraintKind::AtX, &["p1"], "0 mm")?,
                    c("k4".into(), SkConstraintKind::AtY, &["p1"], "0 mm")?,
                    c("k5".into(), SkConstraintKind::Diameter, &["c2"], &nm.n("root"))?,
                ],
                patterns: Vec::new(),
                gear: None,
            };
            // The tooth in the axial (XZ) section: a trapezoid from inside the core to the tip,
            // half-width p/4 at the reference radius, flanks at the pressure angle.
            let pe = format!("pi * {mn}");
            let tan = format!("tan({an})");
            let hw_c = format!("({pe} / 4 + (1.25 * {mn} + {TOOTH_OVERLAP} * {pe}) * {tan})");
            let hw_t = format!("({pe} / 4 - {mn} * {tan})");
            let hw_tip = pitch / 4.0 - m * t;
            let (ri, ra) = (format!("{dn} / 2 - 1.25 * {mn} - {TOOTH_OVERLAP} * {pe}"), format!("{dn} / 2 + {mn}"));
            let (ri_v, ra_v) = (d1 / 2.0 - 1.25 * m - TOOTH_OVERLAP * pitch, d1 / 2.0 + m);
            let tooth = [
                (ri.clone(), "0 mm".to_string(), [ri_v, 0.0]),
                (ra.clone(), format!("{hw_c} - {hw_t}"), [ra_v, hw_core - hw_tip]),
                (ra, format!("{hw_c} + {hw_t}"), [ra_v, hw_core + hw_tip]),
                (ri, format!("2 * {hw_c}"), [ri_v, 2.0 * hw_core]),
            ];
            // One tooth sketch per start, one axial pitch apart: a helix is placed through its
            // profile, so each start gets its own sketch and sweep.
            let tooth_sketch = |s: u32| -> Result<SketchDef, CommandError> {
                let dz = f64::from(s) * pitch;
                let mut entities = Vec::new();
                let mut constraints = Vec::new();
                for (i, (x, z, v)) in tooth.iter().enumerate() {
                    let id = format!("p{}", i + 1);
                    entities.push(ent(&id, SkGeom::Point { x: v[0], y: v[1] + dz }));
                    let zs = if s == 0 { z.clone() } else { format!("{z} + {s} * {pe}") };
                    constraints.push(c(format!("k{}", 20 + 2 * i), SkConstraintKind::AtX, &[&id], x)?);
                    constraints.push(c(format!("k{}", 21 + 2 * i), SkConstraintKind::AtY, &[&id], &zs)?);
                }
                for i in 0..tooth.len() {
                    let (a, b) = (format!("p{}", i + 1), format!("p{}", (i + 1) % tooth.len() + 1));
                    entities.push(ent(&format!("l{}", 7 + i), SkGeom::Line { a, b }));
                }
                Ok(SketchDef {
                    plane: PlaneRef::XZ,
                    next_id: 40,
                    entities,
                    constraints,
                    patterns: Vec::new(),
                    gear: None,
                })
            };
            // The helix keeps every tooth inside the worm's length.
            let height = format!("{ln} - 2 * {hw_c} - {} * {pe}", starts - 1);
            let features = vec![
                Feature::new(core_sk.clone(), FeatureKind::Sketch(core_sketch)),
                Feature::new(
                    core.clone(),
                    FeatureKind::Extrude(ExtrudeDef {
                        profile: ProfileRef { sketch: core_sk.clone(), regions: Vec::new() },
                        distance: parse_expr("length", &ln)?,
                        symmetric: false,
                        op: BodyOp::New,
                        body: None,
                        body_name: Some(name.clone()),
                    }),
                ),
            ];
            let mut features = features;
            for st in 0..starts {
                let suffix = if st == 0 { String::new() } else { (st + 1).to_string() };
                let sk = format!("{tooth_sk}{suffix}");
                if st > 0 && doc.feature(&sk).is_some() {
                    return Err(CommandError::args(format!("a feature named '{sk}' already exists")));
                }
                features.push(Feature::new(sk.clone(), FeatureKind::Sketch(tooth_sketch(st)?)));
                features.push(Feature::new(
                    format!("{name}_helix{suffix}"),
                    FeatureKind::Sweep(SweepDef {
                        profile: ProfileRef { sketch: sk, regions: Vec::new() },
                        path: PathRef::Helix(gusset_doc::HelixDef {
                            axis: gusset_doc::AxisRef::Z,
                            pitch: parse_expr("pitch", &format!("{starts} * {pe}"))?,
                            height: parse_expr("height", &height)?,
                            left: self.left,
                        }),
                        orientation: Orientation::Frenet,
                        twist: None,
                        op: BodyOp::Add,
                        body: Some(core.clone()),
                        body_name: None,
                    }),
                ));
            }
            let mut at = rb;
            for f in features {
                insert_feature(doc, at, f);
                at = at.map(|r| r + 1);
            }
            let lead = f64::from(starts) * pitch;
            let lead_angle = libm::atan(lead / (std::f64::consts::PI * d1)).to_degrees();
            Ok(json!({
                "feature": format!("{name}_helix"),
                "body": name,
                "parameters": created,
                "axial_pitch_mm": pitch,
                "lead_mm": lead,
                "starts": starts,
                // A non-throated (helical) worm wheel: helix = lead angle, same hand, and a
                // transverse module equal to the worm's axial module (normal = m cos(lead)).
                "wheel": {
                    "tool": "generate_gear",
                    "module": format!("{} mm", m * libm::cos(lead_angle.to_radians())),
                    "helix": format!("{lead_angle} deg"),
                    "left_hand": self.left,
                    "center_distance_mm_for_teeth": "(diameter + module * teeth) / 2",
                },
                "lead_angle_deg": lead_angle,
                "tip_diameter_mm": d1 + 2.0 * m,
                "root_diameter_mm": root,
            }))
        })
    }
}

/// Spur pair shifts for a centre distance (ISO 21771).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryGearPair {
    /// Module in mm.
    pub module: f64,
    pub teeth1: u32,
    pub teeth2: u32,
    /// Centre distance in mm.
    pub center_distance: f64,
    /// Degrees (default 20).
    #[serde(default)]
    pub pressure_angle: Option<f64>,
    /// Shift of gear 1 (the rest to gear 2).
    #[serde(default)]
    pub shift1: Option<f64>,
}

fn involute(a: f64) -> f64 {
    libm::tan(a) - a
}

impl Command for QueryGearPair {
    const ID: &'static str = "query.gear_pair";
    const TITLE: &'static str = "Gear pair shifts";
    const DESCRIPTION: &'static str = "Profile shifts for two spur gears at a centre distance.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, _s: &mut Session, _ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (m, a) = (self.module, self.center_distance);
        let (z1, z2) = (f64::from(self.teeth1), f64::from(self.teeth2));
        let alpha = self.pressure_angle.unwrap_or(20.0).to_radians();
        if !(m > 0.0 && a > 0.0 && z1 >= 6.0 && z2 >= 6.0 && alpha > 0.0 && alpha < 0.7) {
            return Err(CommandError::args(
                "needs module > 0, centre distance > 0, at least 6 teeth each and a pressure angle under 40 deg",
            ));
        }
        let a0 = m * (z1 + z2) / 2.0;
        let cos_aw = a0 * libm::cos(alpha) / a;
        if !(cos_aw > 0.0 && cos_aw < 1.0) {
            return Err(CommandError::args(format!(
                "no working pressure angle for {a} mm: the standard centre distance is {a0} mm"
            ))
            .hint("the centre distance must be close to m (z1 + z2) / 2"));
        }
        // ISO 21771: cos(aw) = a0 cos(a) / a and inv(aw) - inv(a) = 2 tan(a) (x1 + x2) / (z1 + z2);
        // the default split gives each gear a share in proportion to the other's teeth.
        let aw = libm::acos(cos_aw);
        let sum = (z1 + z2) * (involute(aw) - involute(alpha)) / (2.0 * libm::tan(alpha));
        if sum.abs() > 3.0 {
            return Err(CommandError::args(format!(
                "{a} mm would need a shift sum of {sum:.2}; the standard centre distance is {a0} mm"
            ))
            .hint("shifts beyond about +-1.5 per gear are not usable"));
        }
        let x1 = self.shift1.unwrap_or(sum * z2 / (z1 + z2));
        let x2 = sum - x1;
        // Undercut-free minimum shift: z_min = 2 / sin^2(a) (17.1 teeth at 20 deg).
        let z_min = 2.0 / (libm::sin(alpha) * libm::sin(alpha));
        let undercut: Vec<u32> = [(1, z1, x1), (2, z2, x2)]
            .into_iter()
            .filter(|&(_, z, x)| x < (z_min - z) / z_min - 1e-9)
            .map(|(i, _, _)| i)
            .collect();
        // With standard addenda the tips would close the clearance by (sum - (a - a0) / m) m.
        let tip_shortening = sum - (a - a0) / m;
        Ok(json!({
            "standard_center_distance_mm": a0,
            "working_pressure_angle_deg": aw.to_degrees(),
            "shift_sum": sum,
            "shift1": x1,
            "shift2": x2,
            "tip_shortening_mm": tip_shortening * m,
            "undercut_gears": undercut,
            "next": "pass shift1/shift2 as `shift` to generate_gear for each gear",
        }))
    }
}

/// Generates a straight spur rack (ISO 53 basic rack: straight flanks at the pressure
/// angle, addendum m, dedendum 1.25 m) that meshes with `generate_gear` of the same module.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateRack {
    #[serde(default = "d_module")]
    #[schemars(extend("x-essential" = true))]
    pub module: String,
    #[serde(default = "d_teeth")]
    #[schemars(extend("x-essential" = true))]
    pub teeth: String,
    #[serde(default = "d_angle")]
    pub angle: String,
    #[serde(default = "d_width")]
    pub width: String,
    /// Below the roots (default 2 m).
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for GenerateRack {
    const ID: &'static str = "generate.rack";
    const TITLE: &'static str = "Rack";
    const DESCRIPTION: &'static str = "Generate a spur rack along X (ISO 53).";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let base = self.base.clone().unwrap_or_else(|| format!("2 * ({})", self.module));
        let given = [
            ("module", &self.module),
            ("teeth", &self.teeth),
            ("angle", &self.angle),
            ("width", &self.width),
            ("base", &base),
        ];
        let mut exprs: Vec<(&str, gusset_units::expr::Expr)> = Vec::new();
        for (what, src) in given {
            exprs.push((what, parse_expr(what, src)?));
        }
        edit(s, ctx, "Generate rack", |doc, rb| {
            let name = feature_name(doc, self.name, "Rack")?;
            let nm = Names::from_feature(&name);
            let sketch_name = format!("{name}_profile");
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
            let (m, n, alpha, width, base) = (
                value("module", Dim::LENGTH)?,
                value("teeth", Dim::NONE)?,
                value("angle", Dim::ANGLE)?,
                value("width", Dim::LENGTH)?,
                value("base", Dim::LENGTH)?,
            );
            let t = libm::tan(alpha);
            let pitch = std::f64::consts::PI * m;
            // The root gap must stay open: pitch / 2 > 2 * dedendum * tan(angle).
            if !(m > 0.0 && n.fract() == 0.0 && (2.0..=500.0).contains(&n) && width > 0.0 && base > 0.0)
                || !(alpha > 0.0 && pitch / 2.0 > 2.5 * m * t)
            {
                return Err(CommandError::args(
                    "a rack needs module > 0, 2 to 500 whole teeth, width and base > 0 and a pressure angle under about 32 deg",
                )
                .hint("for example module 2 mm, 20 teeth, 20 deg"));
            }
            let notes = [
                ("module", "module"),
                ("teeth", "number of teeth"),
                ("angle", "pressure angle"),
                ("width", "face width"),
                ("base", "material below the tooth roots"),
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
            for p in &new_params {
                if doc.params.iter().any(|q| q.name == p.name) {
                    return Err(CommandError::args(format!("a parameter named '{}' already exists", p.name))
                        .hint("name the rack differently"));
                }
                validate_feature_name(&p.name).map_err(CommandError::args)?;
            }
            if doc.feature(&sketch_name).is_some() {
                return Err(CommandError::args(format!("a feature named '{sketch_name}' already exists"))
                    .hint("name the rack differently"));
            }
            let created: Vec<String> = new_params.iter().map(|p| p.name.clone()).collect();
            doc.params.extend(new_params);
            let sketch = rack_sketch(&nm, [m, n, t, base])?;
            let features = vec![
                Feature::new(sketch_name.clone(), FeatureKind::Sketch(sketch)),
                Feature::new(
                    name.clone(),
                    FeatureKind::Extrude(ExtrudeDef {
                        profile: ProfileRef { sketch: sketch_name.clone(), regions: Vec::new() },
                        distance: parse_expr("width", &nm.n("width"))?,
                        symmetric: false,
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
            Ok(json!({
                "feature": name,
                "body": name,
                "parameters": created,
                "pitch_mm": pitch,
                "length_mm": pitch * n,
                "pitch_line_y_mm": 0.0,
            }))
        })
    }
}

/// The rack profile in the XY plane: one tooth period (root, flank, tip, flank, root) from
/// x = 0 repeated `teeth` times along X by a linear pattern, closed by a base strip. The
/// pitch line is y = 0; roots at y = -1.25 m, tips at y = m. Every point is placed by an
/// expression in the rack's parameters, so editing them reshapes the rack.
fn rack_sketch(nm: &Names, vals: [f64; 4]) -> Result<SketchDef, CommandError> {
    let [m, n, t, base] = vals;
    let (mn, an, tn, bn) = (nm.n("module"), nm.n("angle"), nm.n("teeth"), nm.n("base"));
    let p = std::f64::consts::PI * m;
    let (ha, hf) = (m, 1.25 * m);
    let pe = format!("pi * {mn}");
    let tan = format!("tan({an})");
    let hfe = format!("1.25 * {mn}");
    let pts: [(String, String, [f64; 2]); 9] = [
        ("0 mm".into(), format!("-{hfe}"), [0.0, -hf]),
        (format!("{pe} / 4 - {hfe} * {tan}"), format!("-{hfe}"), [p / 4.0 - hf * t, -hf]),
        (format!("{pe} / 4 + {mn} * {tan}"), mn.clone(), [p / 4.0 + ha * t, ha]),
        (format!("3 * {pe} / 4 - {mn} * {tan}"), mn.clone(), [3.0 * p / 4.0 - ha * t, ha]),
        (format!("3 * {pe} / 4 + {hfe} * {tan}"), format!("-{hfe}"), [3.0 * p / 4.0 + hf * t, -hf]),
        (pe.clone(), format!("-{hfe}"), [p, -hf]),
        (format!("{tn} * {pe}"), format!("-{hfe}"), [n * p, -hf]),
        (format!("{tn} * {pe}"), format!("-{hfe} - {bn}"), [n * p, -hf - base]),
        ("0 mm".into(), format!("-{hfe} - {bn}"), [0.0, -hf - base]),
    ];
    let mut entities = Vec::new();
    let mut constraints = Vec::new();
    for (i, (x, y, v)) in pts.iter().enumerate() {
        let id = format!("p{}", i + 1);
        entities.push(SkEntity { id: id.clone(), geom: SkGeom::Point { x: v[0], y: v[1] }, construction: false });
        for (k, kind, e) in [(0, SkConstraintKind::AtX, x), (1, SkConstraintKind::AtY, y)] {
            constraints.push(SkConstraint {
                id: format!("k{}", 20 + 2 * i + k),
                kind,
                entities: vec![id.clone()],
                value: Some(parse_expr("rack", e)?),
            });
        }
    }
    let line = |id: usize, a: usize, b: usize| SkEntity {
        id: format!("l{id}"),
        geom: SkGeom::Line { a: format!("p{a}"), b: format!("p{b}") },
        construction: false,
    };
    // One period: p1 -> p2 -> p3 -> p4 -> p5 -> p6; the base: p7 -> p8 -> p9 -> p1.
    for (i, (a, b)) in [(1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (7, 8), (8, 9), (9, 1)].into_iter().enumerate() {
        entities.push(line(10 + i, a, b));
    }
    let pattern = SkPattern {
        id: "g40".into(),
        kind: SkPatternKind::Linear { dx: parse_expr("rack", &pe)?, dy: parse_expr("rack", "0 mm")? },
        of: (10..15).map(|i| format!("l{i}")).collect(),
        count: parse_expr("rack", &tn)?,
    };
    Ok(SketchDef { plane: PlaneRef::XY, next_id: 50, entities, constraints, patterns: vec![pattern], gear: None })
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
    /// Chamfer the free ends.
    #[serde(default = "d_true")]
    pub chamfer: bool,
    /// Head across flats (a bolt).
    #[serde(default)]
    pub head_width: Option<String>,
    /// Head height.
    #[serde(default)]
    pub head_height: Option<String>,
    /// Hex socket width (round head).
    #[serde(default)]
    pub socket: Option<String>,
    #[serde(default)]
    pub socket_depth: Option<String>,
    /// Threaded length; the rest is plain shank.
    #[serde(default)]
    pub thread_length: Option<String>,
    /// Thread run-out (default 2.5 P).
    #[serde(default)]
    pub runout: Option<String>,
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

/// The plain shank of a partly threaded rod: a solid revolved from the rod's root (z = 0)
/// to where the thread starts at the major diameter, then a cone down to the minor
/// diameter over the run-out, so the thread groove fades into the shank instead of
/// stopping at a step. `vals` = [major, minor, shank length, run-out] (for the sketch's
/// initial positions; the constraints carry the parameters).
fn shank_features(name: &str, nm: &Names, core: &str, vals: [f64; 4]) -> Result<Vec<Feature>, CommandError> {
    let [d, minor, z0, ro] = vals;
    let (dn, pn, ln, tl, rn) = (nm.n("diameter"), nm.n("pitch"), nm.n("length"), nm.n("thread_length"), nm.n("runout"));
    let shank_end = format!("{ln} - {tl}");
    let pts = [
        ("0 mm".to_string(), "0 mm".to_string(), [0.0, 0.0]),
        (format!("{dn} / 2"), "0 mm".to_string(), [d / 2.0, 0.0]),
        (format!("{dn} / 2"), shank_end.clone(), [d / 2.0, z0]),
        (format!("({dn} - 1.0825317547 * {pn}) / 2"), format!("{shank_end} + {rn}"), [minor / 2.0, z0 + ro]),
        ("0 mm".to_string(), format!("{shank_end} + {rn}"), [0.0, z0 + ro]),
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
                value: Some(parse_expr("shank", e)?),
            });
        }
    }
    for i in 0..5 {
        entities.push(SkEntity {
            id: format!("l{}", 6 + i),
            geom: SkGeom::Line { a: format!("p{}", i + 1), b: format!("p{}", (i + 1) % 5 + 1) },
            construction: false,
        });
    }
    let sk = format!("{name}_shank_sketch");
    Ok(vec![
        Feature::new(
            sk.clone(),
            FeatureKind::Sketch(SketchDef {
                plane: PlaneRef::XZ,
                next_id: 30,
                entities,
                constraints,
                patterns: Vec::new(),
                gear: None,
            }),
        ),
        Feature::new(
            format!("{name}_shank"),
            FeatureKind::Revolve(RevolveDef {
                profile: ProfileRef { sketch: sk, regions: Vec::new() },
                axis: gusset_doc::AxisRef::Y,
                angle: parse_expr("angle", "360 deg")?,
                op: BodyOp::Add,
                body: Some(core.to_string()),
                body_name: None,
            }),
        ),
    ])
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
    const DESCRIPTION: &'static str = "Generate a threaded rod, bolt or socket screw along Z (ISO 68-1, exact helix).";
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
        if let Some(tl) = &self.thread_length {
            given.push(("thread_length", tl.clone()));
            given.push(("runout", self.runout.clone().unwrap_or_else(|| format!("2.5 * ({})", self.pitch))));
        } else if self.runout.is_some() {
            return Err(CommandError::args("runout needs thread_length"));
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
                ("thread_length", "threaded length from the free end"),
                ("runout", "thread run-out into the shank"),
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
            if self.thread_length.is_some() {
                let (tl, ro) = (value("thread_length")?, value("runout")?);
                if !(tl > 2.0 * p && tl < len && ro >= 0.0 && tl - ro > p) {
                    return Err(CommandError::args(
                        "thread_length must be over two pitches and under the rod length, with room for the run-out",
                    )
                    .hint("ISO 4014 M10 x 60: thread_length 26 mm (b), runout 3.75 mm (2.5 P)"));
                }
                features.extend(shank_features(&name, &nm, &core, [d, d - 1.082_531_754_7 * p, len - tl, ro])?);
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
    /// Width across flats.
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
