//! Involute gear generator (ADR 0018): the tooth geometry, and the parameters and the
//! sketch that `generate.gear` writes into a document.
//!
//! A gear is described in its transverse plane by a basic rack (ISO 53: pitch `pi * m`,
//! pressure angle 20 deg, addendum `m`, dedendum `1.25 m`, tip radius `0.38 m`) rolling on
//! the pitch circle, displaced by the profile shift `x * m`:
//!
//! * the straight flank of the rack generates the **involute** of the base circle;
//! * the rounded tip of the rack generates the **root fillet**, a trochoid that runs tangent
//!   into the involute and into the root circle;
//! * the flat tip of the rack generates the root circle, the addendum circle is the tip.
//!
//! Helical gears use the same profile in the transverse plane, with the transverse module
//! `m / cos(beta)` and pressure angle `atan(tan(a) / cos(beta))`; radial dimensions keep
//! the normal module.
//!
//! The generated sketch is driven by parameters: every fit point of a flank is placed in
//! polar coordinates (distance and angle from the gear centre) by constraints whose values
//! are expressions of the gear's parameters, so editing `gear1_module` or `gear1_teeth` in
//! the parameter table reshapes the teeth.

use gusset_doc::{SkConstraint, SkConstraintKind, SkEntity, SkGeom, SkPattern, SkPatternKind, SketchDef};
use gusset_units::expr::{Expr, parse};
use std::f64::consts::FRAC_PI_2;

pub use gusset_eval::gear::*;

/// How far the spline flanks may stray from the exact curves (mm).
pub const FLANK_TOLERANCE: f64 = 0.001;

fn err(message: impl Into<String>, hint: impl Into<String>) -> GearError {
    GearError { message: message.into(), hint: hint.into() }
}

/// The most fit points one part of a flank (fillet or involute) may use.
const MAX_PART_POINTS: usize = 160;

/// Which curve a flank point lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    /// Position `num / DEN` along the fillet, from the root circle (0, tangent to it) to the
    /// involute (`DEN`, which is the junction and belongs to the involute).
    Fillet,
    /// Position `num / DEN` along the involute, from where the fillet ends (0) to the tip
    /// circle (`DEN`), in steps that are even in arc length.
    Involute,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlankPoint {
    pub on: Curve,
    pub num: u32,
    pub radius: f64,
    pub angle: f64,
}

impl FlankPoint {
    pub fn xy(&self) -> [f64; 2] {
        polar((self.radius, self.angle))
    }
}

/// The fit points of the lower flank of the tooth centred on angle 0, from the root circle
/// to the tip circle. The fillet and the involute are two splines that share the point where
/// they meet: their curvature jumps there, which one spline would smooth over.
#[derive(Debug, Clone, PartialEq)]
pub struct Flank {
    /// Fillet points (root first), then involute points (junction first, tip last).
    pub points: Vec<FlankPoint>,
    junction: usize,
    /// Largest distance between the splines and the exact curves (mm).
    pub error: f64,
}

impl Flank {
    fn from_nums(g: &Geometry, fillet: &[u32], involute: &[u32]) -> Flank {
        let mut points = Vec::with_capacity(fillet.len() + involute.len());
        for &num in fillet.iter().filter(|&&n| n < DEN) {
            let (radius, angle) = g.fillet_polar(g.psi_at(num));
            points.push(FlankPoint { on: Curve::Fillet, num, radius, angle });
        }
        let junction = points.len();
        for &num in involute {
            let (radius, angle) = g.involute_polar(g.roll_at(num));
            points.push(FlankPoint { on: Curve::Involute, num, radius, angle });
        }
        Flank { points, junction, error: f64::INFINITY }
    }

    /// Index of the point where the fillet meets the involute: the last point of the fillet
    /// spline and the first of the involute spline.
    pub fn junction(&self) -> usize {
        self.junction
    }

    /// The points of the fillet spline, root first, ending at the junction.
    pub fn fillet_points(&self) -> &[FlankPoint] {
        self.points.get(..=self.junction).unwrap_or_default()
    }

    /// The points of the involute spline, from the junction to the tip circle.
    pub fn involute_points(&self) -> &[FlankPoint] {
        self.points.get(self.junction..).unwrap_or_default()
    }
}

fn xy_of(points: &[FlankPoint]) -> Vec<[f64; 2]> {
    points.iter().map(FlankPoint::xy).collect()
}

pub fn dist_to_polyline(p: [f64; 2], line: &[[f64; 2]]) -> f64 {
    let mut best = f64::INFINITY;
    for w in line.windows(2) {
        let &[a, b] = w else { continue };
        let ab = [b[0] - a[0], b[1] - a[1]];
        let l2 = ab[0] * ab[0] + ab[1] * ab[1];
        let t = if l2 > 0.0 { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let q = [a[0] + t * ab[0], a[1] + t * ab[1]];
        best = best.min((p[0] - q[0]).hypot(p[1] - q[1]));
    }
    best
}

/// Adds fit points (bisecting intervals) to `nums` until the spline stays within `tol` of
/// `exact`; `make` gives the point at a position. Returns the worst deviation reached.
fn refine(nums: &mut Vec<u32>, exact: &[[f64; 2]], make: &dyn Fn(u32) -> [f64; 2], tol: f64) -> f64 {
    let n_exact = exact.len().saturating_sub(1).max(1) as u64;
    loop {
        let pts: Vec<[f64; 2]> = nums.iter().map(|&n| make(n)).collect();
        let spline = gusset_sketch::spline_polyline(&pts, false, 24);
        // Deviation per interval between fit points.
        let mut worst = vec![0.0f64; nums.len().saturating_sub(1)];
        for (k, &p) in exact.iter().enumerate() {
            let pos = (k as u64 * u64::from(DEN) / n_exact) as u32;
            let i = nums.partition_point(|&n| n <= pos).saturating_sub(1).min(worst.len().saturating_sub(1));
            if let Some(w) = worst.get_mut(i) {
                *w = w.max(dist_to_polyline(p, &spline));
            }
        }
        let max = worst.iter().copied().fold(0.0, f64::max);
        if max <= tol || nums.len() >= MAX_PART_POINTS {
            return max;
        }
        let mut added = false;
        let mut next = Vec::with_capacity(nums.len() * 2);
        for (i, &n) in nums.iter().enumerate() {
            next.push(n);
            let m = nums.get(i + 1).copied();
            if let (Some(m), Some(&w)) = (m, worst.get(i))
                && w > tol
                && m - n >= 2
            {
                next.push(n + (m - n) / 2);
                added = true;
            }
        }
        if !added {
            return max;
        }
        *nums = next;
    }
}

/// Picks fit points so that the splines stay within [`FLANK_TOLERANCE`] of the exact flank:
/// a coarse even spacing, refined where a spline strays.
pub fn choose_flank(g: &Geometry) -> Result<Flank, GearError> {
    let (exact_fillet, exact_involute) = exact_flank(g);
    let coarse: Vec<u32> = (0..=6).map(|k| k * DEN / 6).collect();
    let mut fillet = coarse.clone();
    let mut involute = coarse;
    // Refine to well inside the tolerance: the check below also measures the other way.
    let target = 0.5 * FLANK_TOLERANCE;
    refine(&mut fillet, &exact_fillet, &|n| g.fillet_xy(g.psi_at(n)), target);
    refine(&mut involute, &exact_involute, &|n| g.involute_xy(g.roll_at(n)), target);
    let mut flank = Flank::from_nums(g, &fillet, &involute);
    let measure = |pts: &[FlankPoint], exact: &[[f64; 2]]| {
        let spline = gusset_sketch::spline_polyline(&xy_of(pts), false, 24);
        let a = exact.iter().map(|&p| dist_to_polyline(p, &spline)).fold(0.0, f64::max);
        let b = spline.iter().map(|&p| dist_to_polyline(p, exact)).fold(0.0, f64::max);
        a.max(b)
    };
    flank.error = measure(flank.fillet_points(), &exact_fillet).max(measure(flank.involute_points(), &exact_involute));
    if flank.error > FLANK_TOLERANCE {
        return Err(err(
            format!("the tooth flanks could not be approximated within {} mm", FLANK_TOLERANCE),
            "try different gear settings, or report this as a bug",
        ));
    }
    Ok(flank)
}

// ------------------------------------------------------------------ parameters

/// The names of a gear's parameters: `<prefix>_module`, ...
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Names(pub String);

impl Names {
    /// A parameter prefix from a feature name: `Gear1` -> `gear1`.
    pub fn from_feature(name: &str) -> Names {
        let p: String =
            name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        Names(p)
    }

    pub fn n(&self, what: &str) -> String {
        format!("{}_{what}", self.0)
    }
}

/// A parameter to add: name, expression, short note.
pub type NewParam = (String, String, &'static str);

/// The derived parameters, in dependency-free order (the document sorts them out).
pub fn derived_params(nm: &Names, helical: bool) -> Vec<NewParam> {
    let n = |s: &str| nm.n(s);
    let (module, teeth, angle, shift, backlash, fillet) =
        (n("module"), n("teeth"), n("pressure_angle"), n("shift"), n("backlash"), n("fillet"));
    let (tm, ta, pd, bd, td) =
        (n("transverse_module"), n("transverse_angle"), n("pitch_diameter"), n("base_diameter"), n("tip_diameter"));
    let helix = n("helix");
    let fr = n("fillet_radius");
    let cd = n("cutter_depth");
    let dj = format!("({cd} + {fr} * sin({ta}))");
    vec![
        (
            tm.clone(),
            if helical { format!("{module} / cos({helix})") } else { module.clone() },
            "module in the plane of rotation",
        ),
        (
            ta.clone(),
            if helical { format!("atan(tan({angle}) / cos({helix}))") } else { angle.clone() },
            "pressure angle in the plane of rotation",
        ),
        (pd.clone(), format!("{tm} * {teeth}"), "pitch circle"),
        (bd.clone(), format!("{pd} * cos({ta})"), "base circle: the involute unrolls from it"),
        (td.clone(), format!("{pd} + 2 * {module} * (1 + {shift})"), "tip (outside) circle"),
        (n("root_diameter"), format!("{pd} - 2 * {module} * (1.25 - {shift})"), "root circle"),
        (
            n("tooth_angle"),
            format!(
                "(pi / 2 + 2 * {shift} * tan({angle}) - {backlash} / {module}) / {teeth} * 1 rad + (tan({ta}) * 1 rad - {ta})"
            ),
            "half the angle a tooth spans at the base circle",
        ),
        (fr.clone(), format!("{fillet} * {module}"), "radius of the rack tip that cuts the root fillet"),
        (
            cd.clone(),
            format!("1.25 * {module} - {fr} - {shift} * {module}"),
            "depth of the rack tip's centre below the pitch line",
        ),
        (
            n("cutter_offset"),
            format!("pi * {tm} / 4 - ({cd} + {shift} * {module}) * tan({ta}) - {fr} / cos({ta})"),
            "offset of the rack tip's centre from the tooth space",
        ),
        (n("fillet_span"), format!("90 deg - {ta}"), "angle the root fillet turns through"),
        (
            n("flank_roll"),
            format!("acos({bd} / 2 / sqrt(({pd} / 2 - {dj}) ^ 2 + ({dj} / tan({ta})) ^ 2))"),
            "where the involute starts (above the base circle)",
        ),
        (n("tip_roll"), format!("acos({bd} / {td})"), "where the involute ends, at the tip circle"),
    ]
}

/// Expressions for a point of the lower flank: (radius, angle).
pub fn point_exprs(nm: &Names, p: &FlankPoint) -> (String, String) {
    let n = |s: &str| nm.n(s);
    match p.on {
        Curve::Involute => {
            let (fr, tr) = (n("flank_roll"), n("tip_roll"));
            let phi = if p.num == 0 {
                fr
            } else {
                format!("sqrt({fr} * {fr} + ({tr} * {tr} - {fr} * {fr}) * {} / {DEN})", p.num)
            };
            (
                format!("{} / 2 / cos({phi})", n("base_diameter")),
                format!("-({} - (tan({phi}) * 1 rad - {phi}))", n("tooth_angle")),
            )
        }
        Curve::Fillet => {
            let psi = if p.num == 0 {
                "90 deg".to_string()
            } else {
                format!("(90 deg - {} * {} / {DEN})", n("fillet_span"), p.num)
            };
            let d = format!("({} + {} * sin({psi}))", n("cutter_depth"), n("fillet_radius"));
            let pr = format!("{} / 2", n("pitch_diameter"));
            (
                format!("sqrt(({pr} - {d}) ^ 2 + ({d} / tan({psi})) ^ 2)"),
                format!(
                    "-pi / {z} * 1 rad + ({co} + {fr} * cos({psi}) - {d} / tan({psi})) / ({pr}) * 1 rad + atan2({d} / tan({psi}), {pr} - {d}) + {bl} / ({m} * {z}) * 1 rad",
                    z = n("teeth"),
                    co = n("cutter_offset"),
                    fr = n("fillet_radius"),
                    bl = n("backlash"),
                    m = n("module"),
                ),
            )
        }
    }
}

// ------------------------------------------------------------------ the sketch

struct Builder {
    next: u32,
    entities: Vec<SkEntity>,
    constraints: Vec<SkConstraint>,
}

impl Builder {
    fn id(&mut self, prefix: char) -> String {
        let id = format!("{prefix}{}", self.next);
        self.next += 1;
        id
    }

    fn entity(&mut self, prefix: char, geom: SkGeom, construction: bool) -> String {
        let id = self.id(prefix);
        self.entities.push(SkEntity { id: id.clone(), geom, construction });
        id
    }

    fn point(&mut self, p: [f64; 2]) -> String {
        self.entity('p', SkGeom::Point { x: p[0], y: p[1] }, false)
    }

    fn line(&mut self, a: &str, b: &str, construction: bool) -> String {
        self.entity('l', SkGeom::Line { a: a.into(), b: b.into() }, construction)
    }

    fn constrain(&mut self, kind: SkConstraintKind, entities: &[&String], value: Option<Expr>) {
        let id = self.id('k');
        self.constraints.push(SkConstraint {
            id,
            kind,
            entities: entities.iter().map(|s| (*s).clone()).collect(),
            value,
        });
    }
}

fn expr(src: &str) -> Result<Expr, GearError> {
    parse(src).map_err(|e| {
        err(format!("internal error: generated expression '{src}' does not parse: {e}"), "report this as a bug")
    })
}

/// The sketch of one gear profile and the id of the region to extrude.
#[derive(Debug, Clone, PartialEq)]
pub struct GearSketch {
    pub def: SketchDef,
    /// A curve on the outer boundary of the toothed region (names that region).
    pub region: String,
}

/// Builds the profile sketch: the lower flank by polar constraints, the upper flank as its
/// mirror image, the tip and root arcs, a circular pattern for all teeth and the bore.
pub fn profile_sketch(nm: &Names, g: &Geometry, flank: &Flank, bore_radius: f64) -> Result<GearSketch, GearError> {
    let mut b = Builder { next: 1, entities: Vec::new(), constraints: Vec::new() };
    let n = |s: &str| nm.n(s);
    let centre = b.point([0.0, 0.0]);
    let on_x = b.point([1.0, 0.0]);
    let axis = b.line(&centre, &on_x, true);
    b.constrain(SkConstraintKind::Fixed, &[&centre], None);
    b.constrain(SkConstraintKind::Fixed, &[&on_x], None);
    let mut lower: Vec<String> = Vec::new();
    let mut upper: Vec<String> = Vec::new();
    let last = flank.points.len().saturating_sub(1);
    let mut first_angle = String::new();
    for (i, p) in flank.points.iter().enumerate() {
        let (r_expr, a_expr) = point_exprs(nm, p);
        let xy = p.xy();
        let lo = b.point(xy);
        let ray = b.line(&centre, &lo, true);
        b.constrain(SkConstraintKind::Distance, &[&centre, &lo], Some(expr(&r_expr)?));
        b.constrain(SkConstraintKind::Angle, &[&axis, &ray], Some(expr(&a_expr)?));
        let up = b.point([xy[0], -xy[1]]);
        if i == last {
            // The tip arc fixes the distance of its end; only the angle is left.
            let ray_up = b.line(&centre, &up, true);
            let negated = format!("-({a_expr})");
            b.constrain(SkConstraintKind::Angle, &[&axis, &ray_up], Some(expr(&negated)?));
        } else {
            b.constrain(SkConstraintKind::Symmetric, &[&lo, &up, &axis], None);
        }
        if i == 0 {
            first_angle = a_expr;
        }
        lower.push(lo);
        upper.push(up);
    }
    // The root point of the next tooth: its distance follows from the root arc.
    let root_xy = polar((g.root_radius, g.pitch_angle() + g.fillet_polar(FRAC_PI_2).1));
    let next_root = b.point(root_xy);
    let ray_next = b.line(&centre, &next_root, true);
    let next_angle = format!("360 deg / {} + ({first_angle})", n("teeth"));
    b.constrain(SkConstraintKind::Angle, &[&axis, &ray_next], Some(expr(&next_angle)?));
    let (Some(lo_tip), Some(up_root), Some(up_tip)) = (lower.last(), upper.first(), upper.last()) else {
        return Err(err("the flank has no points", "report this as a bug"));
    };
    // Each flank is two splines (fillet, involute) that share the point where they meet.
    let j = flank.junction();
    let spline =
        |b: &mut Builder, pts: &[String]| b.entity('s', SkGeom::Spline { points: pts.to_vec(), closed: false }, false);
    let s_lo_fillet = spline(&mut b, lower.get(..=j).unwrap_or_default());
    let s_lo_involute = spline(&mut b, lower.get(j..).unwrap_or_default());
    let s_up_fillet = spline(&mut b, upper.get(..=j).unwrap_or_default());
    let s_up_involute = spline(&mut b, upper.get(j..).unwrap_or_default());
    let tip = b.entity('a', SkGeom::Arc { center: centre.clone(), start: lo_tip.clone(), end: up_tip.clone() }, false);
    let root =
        b.entity('a', SkGeom::Arc { center: centre.clone(), start: up_root.clone(), end: next_root.clone() }, false);
    let curves = vec![s_lo_fillet, s_lo_involute, tip, s_up_involute, s_up_fillet, root];
    let pattern = SkPattern {
        id: b.id('g'),
        kind: SkPatternKind::Circular { center: centre.clone(), angle: expr("360 deg")? },
        of: curves.clone(),
        count: expr(&n("teeth"))?,
    };
    if bore_radius > 0.0 {
        let c = b.entity('c', SkGeom::Circle { center: centre.clone(), radius: bore_radius }, false);
        b.constrain(SkConstraintKind::Diameter, &[&c], Some(expr(&n("bore"))?));
    }
    // The region's name is the curve with the smallest id on its outer boundary.
    let region =
        curves.iter().min_by_key(|id| gusset_naming::entity_tag(id).unwrap_or(u64::MAX)).cloned().unwrap_or_default();
    let def = SketchDef {
        plane: gusset_doc::PlaneRef::XY,
        next_id: b.next,
        entities: b.entities,
        constraints: b.constraints,
        patterns: vec![pattern],
        gear: Some(nm.0.clone()),
    };
    Ok(GearSketch { def, region })
}
