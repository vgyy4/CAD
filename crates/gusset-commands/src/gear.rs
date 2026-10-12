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
use std::f64::consts::{FRAC_PI_2, PI, TAU};

pub const MIN_TEETH: u32 = 6;
pub const MAX_TEETH: u32 = 500;

/// How far the spline flanks may stray from the exact curves (mm).
pub const FLANK_TOLERANCE: f64 = 0.001;

/// A gear's numeric inputs: millimetres and radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GearValues {
    /// Normal module `m`.
    pub module: f64,
    pub teeth: u32,
    /// Normal pressure angle.
    pub pressure_angle: f64,
    /// Profile shift coefficient `x` (tooth thicker for positive values).
    pub shift: f64,
    /// Reduction of the normal tooth thickness at the pitch circle.
    pub backlash: f64,
    /// Radius of the rack's tip as a multiple of `m` (ISO 53: 0.38).
    pub fillet: f64,
    /// Helix angle at the pitch cylinder (0 = spur).
    pub helix: f64,
    pub face_width: f64,
    pub bore: f64,
}

/// A problem with the inputs, with a way out.
#[derive(Debug, Clone, PartialEq)]
pub struct GearError {
    pub message: String,
    pub hint: String,
}

fn err(message: impl Into<String>, hint: impl Into<String>) -> GearError {
    GearError { message: message.into(), hint: hint.into() }
}

/// The derived dimensions of the transverse profile (all in mm / rad).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    teeth: f64,
    module: f64,
    backlash: f64,
    pub transverse_module: f64,
    pub transverse_angle: f64,
    pub pitch_radius: f64,
    pub base_radius: f64,
    pub tip_radius: f64,
    pub root_radius: f64,
    /// Half the angle the tooth spans at the base circle.
    pub tooth_angle: f64,
    pub fillet_radius: f64,
    /// Depth of the rack tip's centre below the rolling line (mm).
    pub cutter_depth: f64,
    /// Offset of the rack tip's centre from the tooth-space axis (mm).
    pub cutter_offset: f64,
    /// Angle between the rack's flank normal and its tip normal.
    pub fillet_span: f64,
    /// Roll angle of the involute where the fillet ends.
    pub flank_roll: f64,
    pub tip_roll: f64,
    pub junction_radius: f64,
}

impl Geometry {
    /// Checks the inputs and works out the profile.
    pub fn new(v: &GearValues) -> Result<Geometry, GearError> {
        let finite = [v.module, v.pressure_angle, v.shift, v.backlash, v.fillet, v.helix, v.face_width, v.bore];
        if finite.iter().any(|x| !x.is_finite()) {
            return Err(err("a gear value is not a finite number", "check the parameters of the gear"));
        }
        if v.module <= 0.0 || v.module > 1000.0 {
            return Err(err(
                format!("module {} mm is out of range", v.module),
                "use a module between 0.01 and 1000 mm",
            ));
        }
        if !(MIN_TEETH..=MAX_TEETH).contains(&v.teeth) {
            return Err(err(
                format!("{} teeth is out of range", v.teeth),
                format!("a gear needs {MIN_TEETH} to {MAX_TEETH} teeth"),
            ));
        }
        if !(0.05..=0.7).contains(&v.pressure_angle) {
            return Err(err(
                format!("pressure angle {:.2} deg is out of range", v.pressure_angle.to_degrees()),
                "use an angle between 3 and 40 degrees (ISO 53: 20 deg)",
            ));
        }
        if v.helix.abs() > 0.8 {
            return Err(err(
                format!("helix angle {:.1} deg is too steep", v.helix.to_degrees()),
                "use a helix angle between -45 and 45 degrees",
            ));
        }
        if !(-2.0..=2.0).contains(&v.shift) {
            return Err(err(format!("profile shift {} is out of range", v.shift), "use a shift between -2 and 2"));
        }
        if !(0.0..=0.6).contains(&v.fillet) {
            return Err(err(
                format!("root fillet {} is out of range", v.fillet),
                "give the fillet radius as a fraction of the module, 0 to 0.6 (ISO 53: 0.38)",
            ));
        }
        if v.backlash < 0.0 || v.backlash > 0.5 * v.module {
            return Err(err(
                format!("backlash {} mm is out of range", v.backlash),
                "use 0 to half a module; it thins each tooth by that much at the pitch circle",
            ));
        }
        if v.face_width <= 0.0 {
            return Err(err("the face width must be positive", "give the gear a width such as 10 mm"));
        }
        let z = f64::from(v.teeth);
        let cb = v.helix.cos();
        let mt = v.module / cb;
        let at = (v.pressure_angle.tan() / cb).atan();
        let pitch_radius = mt * z / 2.0;
        let base_radius = pitch_radius * at.cos();
        let tip_radius = pitch_radius + v.module * (1.0 + v.shift);
        let root_radius = pitch_radius - v.module * (1.25 - v.shift);
        if root_radius <= 0.0 {
            return Err(err("the root circle would be inside the gear centre", "use more teeth or a larger shift"));
        }
        let half = (FRAC_PI_2 + 2.0 * v.shift * v.pressure_angle.tan() - v.backlash / v.module) / z;
        let tooth_angle = half + (at.tan() - at);
        let fillet_radius = v.fillet * v.module;
        let cutter_depth = 1.25 * v.module - fillet_radius - v.shift * v.module;
        let cutter_offset = PI * mt / 4.0 - (cutter_depth + v.shift * v.module) * at.tan() - fillet_radius / at.cos();
        let d_j = cutter_depth + fillet_radius * at.sin();
        let junction_radius = ((pitch_radius - d_j).powi(2) + (d_j / at.tan()).powi(2)).sqrt();
        if d_j > pitch_radius * at.sin().powi(2) || junction_radius < base_radius {
            // The straight part of the rack's flank reaches deeper than the tangent point of
            // the line of action: the rack cuts away the foot of the involute (undercut).
            let x_min = min_shift(v);
            return Err(err(
                format!(
                    "{} teeth with a {:.0} deg pressure angle would be undercut",
                    v.teeth,
                    v.pressure_angle.to_degrees()
                ),
                format!("use at least {} teeth, or a profile shift of about {:.2} or more", min_teeth(v), x_min),
            ));
        }
        let flank_roll = (base_radius / junction_radius).acos();
        if tip_radius <= junction_radius {
            return Err(err(
                "the teeth are too short to have an involute flank",
                "increase the profile shift, or use a smaller root fillet",
            ));
        }
        let tip_roll = (base_radius / tip_radius).acos();
        let g = Geometry {
            teeth: z,
            module: v.module,
            backlash: v.backlash,
            transverse_module: mt,
            transverse_angle: at,
            pitch_radius,
            base_radius,
            tip_radius,
            root_radius,
            tooth_angle,
            fillet_radius,
            cutter_depth,
            cutter_offset,
            fillet_span: FRAC_PI_2 - at,
            flank_roll,
            tip_roll,
            junction_radius,
        };
        // The tooth must have a tip land, and the fillet must run cleanly down to the root.
        let (_, tip_angle) = g.involute_polar(tip_roll);
        if -tip_angle * tip_radius * 2.0 < 0.1 * v.module {
            return Err(err(
                "the teeth come to a point at the tip",
                "use a smaller profile shift, more backlash is not needed; try fewer shift or more teeth",
            ));
        }
        let mut last = g.fillet_polar(at);
        for k in 1..=64 {
            let psi = at + g.fillet_span * f64::from(k) / 64.0;
            let (r, th) = g.fillet_polar(psi);
            if r > last.0 + 1e-12 || th > last.1 + 1e-12 {
                return Err(err(
                    "the root fillet folds over itself",
                    "use a smaller fillet radius, more teeth or a larger profile shift",
                ));
            }
            last = (r, th);
        }
        let (_, root_angle) = g.fillet_polar(FRAC_PI_2);
        if root_radius * (TAU / z + 2.0 * root_angle) < 0.01 * v.module {
            return Err(err(
                "neighbouring teeth touch at the root",
                "use a smaller profile shift or fillet, or fewer teeth",
            ));
        }
        Ok(g)
    }

    /// Polar position (radius, angle) of the lower flank where the involute has rolled
    /// through `phi` radians; the tooth is centred on angle 0.
    pub fn involute_polar(&self, phi: f64) -> (f64, f64) {
        (self.base_radius / phi.cos(), -(self.tooth_angle - (phi.tan() - phi)))
    }

    /// Polar position of the lower flank on the root fillet, for the rack tip's contact
    /// normal at angle `psi` from the pitch line (`pi/2` at the root circle, the
    /// transverse pressure angle where the fillet meets the involute).
    pub fn fillet_polar(&self, psi: f64) -> (f64, f64) {
        let d = self.cutter_depth + self.fillet_radius * psi.sin();
        let a = self.pitch_radius - d;
        let b = d / psi.tan();
        let u_i = self.cutter_offset + self.fillet_radius * psi.cos() - b;
        let theta =
            -PI / self.teeth + u_i / self.pitch_radius + b.atan2(a) + self.backlash / (self.module * self.teeth);
        (a.hypot(b), theta)
    }

    /// Roll angle at position `num / DEN` along the involute. The positions are even in arc
    /// length (which grows with the square of the roll angle).
    pub fn roll_at(&self, num: u32) -> f64 {
        let (a, b) = (self.flank_roll, self.tip_roll);
        (a * a + (b * b - a * a) * f64::from(num) / f64::from(DEN)).sqrt()
    }

    /// Contact normal angle at position `num / DEN` along the fillet (`DEN` is the junction).
    pub fn psi_at(&self, num: u32) -> f64 {
        FRAC_PI_2 - self.fillet_span * f64::from(num) / f64::from(DEN)
    }

    pub fn involute_xy(&self, phi: f64) -> [f64; 2] {
        polar(self.involute_polar(phi))
    }

    pub fn fillet_xy(&self, psi: f64) -> [f64; 2] {
        polar(self.fillet_polar(psi))
    }

    /// Area enclosed by the exact tooth outline, all teeth, without a bore (mm2). For
    /// checking the generated sketch and solid against the formulas.
    pub fn outline_area(&self) -> f64 {
        let (fillet, involute) = exact_flank(self);
        let flank: Vec<[f64; 2]> = fillet.into_iter().chain(involute.into_iter().skip(1)).collect();
        // Area swept by a ray from the centre along the lower flank, root to tip.
        let swept: f64 = flank
            .windows(2)
            .filter_map(|w| match *w {
                [a, b] => Some(a[0] * b[1] - b[0] * a[1]),
                _ => None,
            })
            .sum::<f64>()
            / 2.0;
        let angle = |p: Option<&[f64; 2]>| p.map_or(0.0, |p| p[1].atan2(p[0]));
        let tip_sector = 0.5 * self.tip_radius.powi(2) * (-2.0 * angle(flank.last()));
        let root_sector = 0.5 * self.root_radius.powi(2) * (self.pitch_angle() + 2.0 * angle(flank.first()));
        self.teeth * (2.0 * swept + tip_sector + root_sector)
    }

    /// Angle of one tooth pitch.
    pub fn pitch_angle(&self) -> f64 {
        TAU / self.teeth
    }
}

fn polar((r, th): (f64, f64)) -> [f64; 2] {
    [r * th.cos(), r * th.sin()]
}

/// Smallest tooth count without undercut for these settings (searching upwards).
pub fn min_teeth(v: &GearValues) -> u32 {
    (MIN_TEETH..=MAX_TEETH).find(|&z| undercut_free(&GearValues { teeth: z, ..*v })).unwrap_or(MAX_TEETH)
}

/// Smallest profile shift without undercut for these settings.
pub fn min_shift(v: &GearValues) -> f64 {
    let mut lo = v.shift;
    let mut hi = 2.0;
    if !undercut_free(&GearValues { shift: hi, ..*v }) {
        return hi;
    }
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if undercut_free(&GearValues { shift: mid, ..*v }) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

fn undercut_free(v: &GearValues) -> bool {
    let z = f64::from(v.teeth);
    let cb = v.helix.cos();
    let mt = v.module / cb;
    let at = (v.pressure_angle.tan() / cb).atan();
    let pr = mt * z / 2.0;
    let rho = v.fillet * v.module;
    let depth = 1.25 * v.module - rho - v.shift * v.module;
    let d_j = depth + rho * at.sin();
    d_j <= pr * at.sin().powi(2)
}

/// Fit points along a flank part sit at multiples of `1/DEN` of its parameter range.
pub const DEN: u32 = 4096;

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

const FILLET_SAMPLES: u32 = 300;
const INVOLUTE_SAMPLES: u32 = 600;

/// The exact fillet (root to junction) and involute (junction to tip), densely sampled at
/// `k / 300` and `k / 600` of their parameter ranges.
pub fn exact_flank(g: &Geometry) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
    let at = |k: u32, n: u32| (u64::from(k) * u64::from(DEN) / u64::from(n)) as u32;
    let fillet = (0..=FILLET_SAMPLES).map(|k| g.fillet_xy(g.psi_at(at(k, FILLET_SAMPLES)))).collect();
    let involute = (0..=INVOLUTE_SAMPLES).map(|k| g.involute_xy(g.roll_at(at(k, INVOLUTE_SAMPLES)))).collect();
    (fillet, involute)
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
            "where the involute starts (fails when the teeth would be undercut)",
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
    };
    Ok(GearSketch { def, region })
}
