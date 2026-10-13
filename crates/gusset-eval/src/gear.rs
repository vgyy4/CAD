//! Involute gear geometry (ADR 0018): the tooth profile of a gear from its parameters,
//! and the checks that say whether those parameters make a gear at all. Shared by the
//! generator (`generate.gear` in gusset-commands, which writes the parameters and the
//! sketch) and the evaluator (which repeats the checks on every rebuild of a generated
//! gear sketch).
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

use std::f64::consts::{FRAC_PI_2, PI, TAU};

pub const MIN_TEETH: u32 = 6;
pub const MAX_TEETH: u32 = 500;

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

/// Cartesian position of a polar one (radius, angle).
pub fn polar((r, th): (f64, f64)) -> [f64; 2] {
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
