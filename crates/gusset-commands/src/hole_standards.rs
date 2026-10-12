//! Standard hole presets (docs/DESIGN.md "Easy for beginners, never in the way of experts",
//! ADR 0019): ISO metric clearance holes, tap drills, socket-head counterbores and
//! countersinks, looked up by a phrase such as `ISO M6 clearance normal`.
//!
//! The table is written from the published nominal values of
//! - ISO 273:1979 (clearance holes for bolts and screws): fine, medium ("normal") and coarse
//!   series;
//! - ISO 261 / ISO 262 (general-purpose metric screw threads): coarse pitch of the sizes
//!   M1.6 to M24;
//! - ISO 2306 (drills for use prior to tapping ISO metric coarse threads): tap-drill diameter;
//! - ISO 4762 (hexagon socket head cap screws): head diameter `dk` and height `k`, and
//!   DIN 974-1 (counterbores for them, the series of ISO 4762 screws): counterbore diameter
//!   and depth. ISO 4762 itself gives only the screw; the counterbore is the DIN 974-1
//!   recommendation;
//! - ISO 10642 (hexagon socket countersunk head screws, 90 degrees): theoretical head
//!   diameter `dk` of the countersink.
//!
//! Sizes a standard does not cover (for example M1.6 in DIN 974-1, or M24 in ISO 10642) are
//! refused with the list of sizes that exist. The default hole depth of a tapped hole is not
//! from a standard: the thread is 1.5 d long and the drill goes 0.5 d deeper than that.
//! Every value ends up as an ordinary parameter, so it can be changed afterwards.

use gusset_doc::HoleType;

/// ISO 273 series of clearance holes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Fine,
    Normal,
    Coarse,
}

impl Fit {
    pub fn word(self) -> &'static str {
        match self {
            Fit::Fine => "fine",
            Fit::Normal => "normal",
            Fit::Coarse => "coarse",
        }
    }
}

/// One thread size with everything the table knows about it (mm).
#[derive(Debug, Clone, Copy)]
pub struct Row {
    /// Designation, `M6`.
    pub size: &'static str,
    /// Nominal (major) diameter.
    pub d: f64,
    /// ISO 261/262 coarse pitch.
    pub pitch: f64,
    /// ISO 2306 tap-drill diameter.
    pub tap_drill: f64,
    /// ISO 273 clearance holes: fine, medium, coarse.
    pub clearance: [f64; 3],
    /// ISO 4762 head and DIN 974-1 counterbore: `(dk, k, counterbore diameter, depth)`.
    pub cbore: Option<(f64, f64, f64, f64)>,
    /// ISO 10642 theoretical head diameter (the countersink diameter).
    pub csink: Option<f64>,
}

const fn row(
    size: &'static str,
    d: f64,
    pitch: f64,
    tap_drill: f64,
    clearance: [f64; 3],
    cbore: Option<(f64, f64, f64, f64)>,
    csink: Option<f64>,
) -> Row {
    Row { size, d, pitch, tap_drill, clearance, cbore, csink }
}

/// The table, by increasing size.
pub const ROWS: &[Row] = &[
    row("M1.6", 1.6, 0.35, 1.25, [1.7, 1.8, 2.0], None, None),
    row("M2", 2.0, 0.4, 1.6, [2.2, 2.4, 2.6], None, None),
    row("M2.5", 2.5, 0.45, 2.05, [2.7, 2.9, 3.1], None, None),
    row("M3", 3.0, 0.5, 2.5, [3.2, 3.4, 3.6], Some((5.5, 3.0, 6.5, 3.4)), Some(6.72)),
    row("M3.5", 3.5, 0.6, 2.9, [3.7, 3.9, 4.2], None, None),
    row("M4", 4.0, 0.7, 3.3, [4.3, 4.5, 4.8], Some((7.0, 4.0, 8.0, 4.4)), Some(8.96)),
    row("M5", 5.0, 0.8, 4.2, [5.3, 5.5, 5.8], Some((8.5, 5.0, 10.0, 5.4)), Some(11.2)),
    row("M6", 6.0, 1.0, 5.0, [6.4, 6.6, 7.0], Some((10.0, 6.0, 11.0, 6.5)), Some(13.44)),
    row("M8", 8.0, 1.25, 6.8, [8.4, 9.0, 10.0], Some((13.0, 8.0, 15.0, 8.6)), Some(17.92)),
    row("M10", 10.0, 1.5, 8.5, [10.5, 11.0, 12.0], Some((16.0, 10.0, 18.0, 10.8)), Some(22.4)),
    row("M12", 12.0, 1.75, 10.2, [13.0, 13.5, 14.5], Some((18.0, 12.0, 20.0, 12.6)), Some(26.88)),
    row("M14", 14.0, 2.0, 12.0, [15.0, 15.5, 16.5], Some((21.0, 14.0, 24.0, 14.5)), Some(30.8)),
    row("M16", 16.0, 2.0, 14.0, [17.0, 17.5, 18.5], Some((24.0, 16.0, 26.0, 16.5)), Some(33.6)),
    row("M18", 18.0, 2.5, 15.5, [19.0, 20.0, 21.0], None, None),
    row("M20", 20.0, 2.5, 17.5, [21.0, 22.0, 24.0], Some((30.0, 20.0, 33.0, 20.5)), Some(40.32)),
    row("M22", 22.0, 2.5, 19.5, [23.0, 24.0, 26.0], None, None),
    row("M24", 24.0, 3.0, 21.0, [25.0, 26.0, 28.0], Some((36.0, 24.0, 40.0, 24.5)), None),
];

/// Why a phrase could not be turned into a preset.
#[derive(Debug, Clone, PartialEq)]
pub struct StandardError {
    pub message: String,
    pub hint: String,
}

fn err(message: impl Into<String>, hint: impl Into<String>) -> StandardError {
    StandardError { message: message.into(), hint: hint.into() }
}

const EXAMPLES: &str = "for example \"ISO M6 clearance normal\", \"ISO M6 tapped\", \"ISO M6 counterbore DIN 974/ISO 4762 head\" or \"ISO M6 countersink 90 deg\"";

/// A preset: a thread size, a kind of hole and the series of the clearance hole.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub row: &'static Row,
    pub kind: HoleType,
    pub fit: Fit,
}

/// One value the preset fills in: a feature argument by name, with its value as text with
/// a unit and a short comment for the parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub arg: &'static str,
    pub value: String,
    pub comment: String,
}

/// Everything a preset fills in.
#[derive(Debug, Clone, PartialEq)]
pub struct PresetValues {
    pub slots: Vec<Slot>,
    /// The hole goes all the way through unless a `depth` is filled in (tapped holes).
    pub through: bool,
    /// The thread designation of a tapped hole.
    pub designation: Option<String>,
    /// The standards the values come from, for the note on the feature.
    pub sources: &'static str,
}

/// Prints a number of millimetres without floating-point noise (`6.6 mm`, `0.35 mm`).
pub fn mm(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s} mm")
}

impl Preset {
    /// The phrase in canonical form, `ISO M6 clearance normal`.
    pub fn canonical(&self) -> String {
        let size = self.row.size;
        match self.kind {
            HoleType::Simple => format!("ISO {size} clearance {}", self.fit.word()),
            HoleType::Tapped => format!("ISO {size} tapped"),
            HoleType::Counterbore => format!("ISO {size} counterbore DIN 974-1/ISO 4762 head {}", self.fit.word()),
            HoleType::Countersink => format!("ISO {size} countersink 90 deg ISO 10642 head {}", self.fit.word()),
        }
    }

    /// The values of the preset.
    pub fn values(&self) -> PresetValues {
        let r = self.row;
        let slot = |arg, value: f64, comment: String| Slot { arg, value: mm(value), comment };
        let clearance = r
            .clearance
            .get(match self.fit {
                Fit::Fine => 0,
                Fit::Normal => 1,
                Fit::Coarse => 2,
            })
            .copied()
            .unwrap_or_default();
        let clear_comment = format!("ISO 273 {} clearance hole for {}", self.fit.word(), r.size);
        match self.kind {
            HoleType::Simple => PresetValues {
                slots: vec![slot("diameter", clearance, clear_comment)],
                through: true,
                designation: None,
                sources: "ISO 273",
            },
            HoleType::Counterbore => {
                let (dk, k, cd, ch) = r.cbore.unwrap_or_default();
                PresetValues {
                    slots: vec![
                        slot("diameter", clearance, clear_comment),
                        slot(
                            "cbore_diameter",
                            cd,
                            format!(
                                "DIN 974-1 counterbore for an ISO 4762 {} screw (head {} x {})",
                                r.size,
                                mm(dk),
                                mm(k)
                            ),
                        ),
                        slot("cbore_depth", ch, "DIN 974-1 counterbore depth".to_string()),
                    ],
                    through: true,
                    designation: None,
                    sources: "ISO 273, DIN 974-1 and ISO 4762",
                }
            }
            HoleType::Countersink => PresetValues {
                slots: vec![
                    slot("diameter", clearance, clear_comment),
                    slot(
                        "csink_diameter",
                        r.csink.unwrap_or_default(),
                        format!("ISO 10642 head diameter of an {} countersunk screw", r.size),
                    ),
                    Slot { arg: "csink_angle", value: "90 deg".into(), comment: "ISO 10642 countersink angle".into() },
                ],
                through: true,
                designation: None,
                sources: "ISO 273 and ISO 10642",
            },
            HoleType::Tapped => {
                let thread = 1.5 * r.d;
                PresetValues {
                    slots: vec![
                        slot("diameter", r.tap_drill, format!("ISO 2306 tap drill for {}", r.size)),
                        slot("pitch", r.pitch, format!("ISO 261/262 coarse pitch of {}", r.size)),
                        slot(
                            "thread_depth",
                            thread,
                            "threaded length: 1.5 x the nominal diameter (default, not from a standard)".to_string(),
                        ),
                        slot(
                            "depth",
                            thread + 0.5 * r.d,
                            "drilled depth: the thread plus half the nominal diameter (default, not from a standard)"
                                .to_string(),
                        ),
                    ],
                    through: false,
                    designation: Some(r.size.to_string()),
                    sources: "ISO 261/262 and ISO 2306",
                }
            }
        }
    }
}

/// Words that name a standard or the screw and carry no choice.
fn is_noise(token: &str) -> bool {
    const WORDS: [&str; 14] = [
        "iso", "din", "en", "head", "socket", "screw", "screws", "cap", "hole", "thread", "metric", "hex", "deg",
        "degrees",
    ];
    const NUMBERS: [&str; 9] = ["273", "261", "262", "2306", "4762", "10642", "974", "974-1", "7991"];
    token.split('/').all(|part| {
        WORDS.contains(&part)
            || NUMBERS.contains(&part)
            || part.strip_prefix("iso").or_else(|| part.strip_prefix("din")).is_some_and(|n| NUMBERS.contains(&n))
    })
}

/// Parses a phrase such as `ISO M6 clearance normal`.
///
/// # Errors
/// A message and a hint when the size, the kind or a word is not understood or the table has
/// no such row.
pub fn parse(text: &str) -> Result<Preset, StandardError> {
    let lower = text.to_lowercase();
    let mut size: Option<&'static Row> = None;
    let mut kind: Option<HoleType> = None;
    let mut fit: Option<Fit> = None;
    let mut angle: Option<f64> = None;
    for raw in lower.split_whitespace() {
        let token = raw.trim_matches(|c: char| matches!(c, ',' | ';' | '(' | ')'));
        if token.is_empty() || is_noise(token) {
            continue;
        }
        if let Some(spec) = token.strip_prefix('m').filter(|r| r.starts_with(|c: char| c.is_ascii_digit())) {
            // `m6` or `m6x1`: a pitch must be the coarse one.
            let (num, pitch) = match spec.split_once('x') {
                Some((n, p)) => (n, Some(p)),
                None => (spec, None),
            };
            let Some(row) = ROWS.iter().find(|r| r.size.eq_ignore_ascii_case(&format!("m{num}"))) else {
                let sizes: Vec<&str> = ROWS.iter().map(|r| r.size).collect();
                return Err(err(
                    format!("the table has no thread M{num}"),
                    format!("sizes: {} (ISO 261/262 coarse threads)", sizes.join(", ")),
                ));
            };
            if let Some(p) = pitch {
                let ok = p.parse::<f64>().is_ok_and(|p| (p - row.pitch).abs() < 1e-9);
                if !ok {
                    return Err(err(
                        format!(
                            "the table only has the coarse thread {}x{}, not {}",
                            row.size,
                            row.pitch,
                            token.to_uppercase()
                        ),
                        "give the dimensions yourself (diameter, thread, pitch) for a fine thread",
                    ));
                }
            }
            if size.replace(row).is_some_and(|prev| prev.size != row.size) {
                return Err(err("two thread sizes in the standard", EXAMPLES));
            }
            continue;
        }
        let new_kind = match token {
            "clearance" | "simple" | "through" => Some(HoleType::Simple),
            "tapped" | "tap" | "threaded" => Some(HoleType::Tapped),
            "counterbore" | "cbore" | "counterbored" => Some(HoleType::Counterbore),
            "countersink" | "csink" | "countersunk" => Some(HoleType::Countersink),
            _ => None,
        };
        if let Some(k) = new_kind {
            if kind.replace(k).is_some_and(|prev| prev != k) {
                return Err(err("two kinds of hole in the standard", EXAMPLES));
            }
            continue;
        }
        let new_fit = match token {
            "fine" => Some(Fit::Fine),
            "normal" | "medium" => Some(Fit::Normal),
            "coarse" => Some(Fit::Coarse),
            _ => None,
        };
        if let Some(f) = new_fit {
            if fit.replace(f).is_some_and(|prev| prev != f) {
                return Err(err("two clearance series in the standard", EXAMPLES));
            }
            continue;
        }
        let num = token.trim_end_matches("deg").trim_end_matches('°');
        if let Ok(a) = num.parse::<f64>() {
            angle = Some(a);
            continue;
        }
        return Err(err(format!("the word '{raw}' is not part of a hole standard"), EXAMPLES));
    }
    let row = size.ok_or_else(|| err("the standard names no thread size", EXAMPLES))?;
    let kind = kind.ok_or_else(|| {
        err("the standard does not say what kind of hole", "add clearance, tapped, counterbore or countersink")
    })?;
    if let Some(a) = angle {
        if kind != HoleType::Countersink {
            return Err(err("an angle only goes with a countersink", EXAMPLES));
        }
        if (a - 90.0).abs() > 1e-9 {
            return Err(err(
                format!("the table has only the 90 deg countersink of ISO 10642, not {a} deg"),
                "give csink_diameter and csink_angle yourself for other cones",
            ));
        }
    }
    if fit.is_some() && kind == HoleType::Tapped {
        return Err(err(
            "a tapped hole has no clearance series",
            "drop fine/normal/coarse, or use clearance, counterbore or countersink",
        ));
    }
    let preset = Preset { row, kind, fit: fit.unwrap_or(Fit::Normal) };
    match kind {
        HoleType::Counterbore if row.cbore.is_none() => {
            let sizes: Vec<&str> = ROWS.iter().filter(|r| r.cbore.is_some()).map(|r| r.size).collect();
            Err(err(
                format!("ISO 4762 / DIN 974-1 has no counterbore for {}", row.size),
                format!("sizes with a counterbore: {}", sizes.join(", ")),
            ))
        }
        HoleType::Countersink if row.csink.is_none() => {
            let sizes: Vec<&str> = ROWS.iter().filter(|r| r.csink.is_some()).map(|r| r.size).collect();
            Err(err(
                format!("ISO 10642 has no countersunk screw {}", row.size),
                format!("sizes with a countersink: {}", sizes.join(", ")),
            ))
        }
        _ => Ok(preset),
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn slot<'a>(v: &'a PresetValues, arg: &str) -> &'a str {
        v.slots.iter().find(|s| s.arg == arg).map_or("", |s| s.value.as_str())
    }

    #[test]
    fn table_is_consistent() {
        let mut last = 0.0;
        for r in ROWS {
            assert!(r.d > last, "{} out of order", r.size);
            last = r.d;
            assert_eq!(r.size, format!("M{}", r.d), "designation matches the diameter");
            // Fine < medium < coarse; clearance above the thread, tap drill below it.
            assert!(r.clearance[0] < r.clearance[1] && r.clearance[1] < r.clearance[2], "{}", r.size);
            assert!(r.clearance[0] > r.d, "{}", r.size);
            assert!(r.tap_drill < r.d && r.tap_drill > r.d - 1.3 * r.pitch - 1e-9, "{}", r.size);
            assert!(r.pitch < r.d / 4.0 + 0.2, "{}", r.size);
            if let Some((dk, k, cd, ch)) = r.cbore {
                assert!(dk > r.d && k >= r.d - 1e-9 && cd > dk && ch > k, "{}", r.size);
                assert!(cd > r.clearance[2], "{}: the counterbore is wider than the hole", r.size);
            }
            if let Some(dk) = r.csink {
                assert!(dk > r.clearance[1], "{}", r.size);
            }
        }
        // Spot checks against the published tables.
        let m = |s: &str| ROWS.iter().find(|r| r.size == s).unwrap();
        assert_eq!(m("M6").clearance, [6.4, 6.6, 7.0]);
        assert_eq!((m("M6").pitch, m("M6").tap_drill), (1.0, 5.0));
        assert_eq!((m("M8").pitch, m("M8").tap_drill), (1.25, 6.8));
        assert_eq!((m("M12").pitch, m("M12").tap_drill), (1.75, 10.2));
        assert_eq!(m("M6").cbore, Some((10.0, 6.0, 11.0, 6.5)));
        assert_eq!(m("M10").csink, Some(22.4));
    }

    #[test]
    fn phrases_are_understood() {
        let p = parse("ISO M6 clearance normal").unwrap();
        assert_eq!((p.row.size, p.kind, p.fit), ("M6", HoleType::Simple, Fit::Normal));
        assert_eq!(p.canonical(), "ISO M6 clearance normal");
        let v = p.values();
        assert_eq!(slot(&v, "diameter"), "6.6 mm");
        assert!(v.through && v.designation.is_none());

        let v = parse("iso m6 clearance fine").unwrap().values();
        assert_eq!(slot(&v, "diameter"), "6.4 mm");
        let v = parse("ISO M6 clearance coarse").unwrap().values();
        assert_eq!(slot(&v, "diameter"), "7 mm");
        // Medium is the normal series; the fit defaults to normal.
        assert_eq!(parse("M8 clearance medium").unwrap().fit, Fit::Normal);
        assert_eq!(slot(&parse("M8 clearance").unwrap().values(), "diameter"), "9 mm");

        let v = parse("ISO M6 tapped").unwrap().values();
        assert_eq!(slot(&v, "diameter"), "5 mm");
        assert_eq!(slot(&v, "pitch"), "1 mm");
        assert_eq!(slot(&v, "thread_depth"), "9 mm");
        assert_eq!(slot(&v, "depth"), "12 mm");
        assert_eq!(v.designation.as_deref(), Some("M6"));
        assert!(!v.through);
        // The thread size is also accepted with its coarse pitch.
        assert_eq!(parse("M6x1 tapped").unwrap().row.size, "M6");

        let v = parse("ISO M6 counterbore DIN 974/ISO 4762 head").unwrap().values();
        assert_eq!(slot(&v, "diameter"), "6.6 mm");
        assert_eq!(slot(&v, "cbore_diameter"), "11 mm");
        assert_eq!(slot(&v, "cbore_depth"), "6.5 mm");
        for phrase in ["ISO M6 counterbore DIN 974-1/ISO 4762 head", "M6 counterbore ISO 4762", "m6 cbore"] {
            assert_eq!(parse(phrase).unwrap().kind, HoleType::Counterbore, "{phrase}");
        }

        let v = parse("ISO M6 countersink 90°").unwrap().values();
        assert_eq!(slot(&v, "csink_diameter"), "13.44 mm");
        assert_eq!(slot(&v, "csink_angle"), "90 deg");
        assert_eq!(parse("ISO M6 countersink 90 deg").unwrap().kind, HoleType::Countersink);
        assert_eq!(parse("iso m6 countersink 90deg").unwrap().kind, HoleType::Countersink);
        // The canonical phrase parses back to the same preset.
        for p in ["M6 clearance fine", "M10 tapped", "M12 counterbore coarse", "M5 countersink fine"] {
            let a = parse(p).unwrap();
            let b = parse(&a.canonical()).unwrap();
            assert_eq!((a.row.size, a.kind, a.fit), (b.row.size, b.kind, b.fit), "{p}");
        }
    }

    #[test]
    fn mistakes_are_explained() {
        for (phrase, why) in [
            ("", "no thread size"),
            ("ISO clearance", "no thread size"),
            ("M6", "what kind of hole"),
            ("ISO M7 clearance", "no thread M7"),
            ("M6 clearance bigger", "not part of a hole standard"),
            ("M6x0.75 tapped", "coarse thread"),
            ("M6 M8 clearance", "two thread sizes"),
            ("M6 tapped clearance", "two kinds"),
            ("M6 clearance fine coarse", "two clearance series"),
            ("M6 tapped fine", "no clearance series"),
            ("M6 countersink 82 deg", "90 deg"),
            ("M6 clearance 90 deg", "angle only goes with a countersink"),
            ("M2 counterbore", "no counterbore for M2"),
            ("M24 countersink", "no countersunk screw M24"),
        ] {
            let e = parse(phrase).unwrap_err();
            assert!(e.message.contains(why), "{phrase}: {e:?}");
            assert!(!e.hint.is_empty(), "{phrase}");
        }
        // The hint lists the sizes that exist.
        assert!(parse("M2 counterbore").unwrap_err().hint.contains("M6"));
    }

    #[test]
    fn numbers_print_without_noise() {
        assert_eq!(mm(6.6), "6.6 mm");
        assert_eq!(mm(7.0), "7 mm");
        assert_eq!(mm(0.35), "0.35 mm");
        assert_eq!(mm(1.5 * 3.5), "5.25 mm");
        assert_eq!(mm(12.0 + 0.1 + 0.2 - 0.3), "12 mm");
    }
}
