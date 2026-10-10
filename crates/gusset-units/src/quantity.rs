//! Dimensioned quantities and the unit table.
//!
//! Every quantity is stored in canonical units: millimetres for length, radians for
//! angle, kilograms for mass and seconds for time. Conversions happen only at the
//! edges (parsing and display), so model-defining arithmetic never depends on the
//! user's display unit.

use std::fmt;

/// Physical dimension as integer exponents of the base dimensions.
///
/// Angle is tracked as its own dimension (even though it is formally dimensionless)
/// so that `sin(30)` can be rejected with a helpful hint instead of silently using
/// radians.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Dim {
    pub length: i8,
    pub angle: i8,
    pub mass: i8,
    pub time: i8,
}

impl Dim {
    pub const NONE: Dim = Dim { length: 0, angle: 0, mass: 0, time: 0 };
    pub const LENGTH: Dim = Dim { length: 1, angle: 0, mass: 0, time: 0 };
    pub const AREA: Dim = Dim { length: 2, angle: 0, mass: 0, time: 0 };
    pub const VOLUME: Dim = Dim { length: 3, angle: 0, mass: 0, time: 0 };
    pub const ANGLE: Dim = Dim { length: 0, angle: 1, mass: 0, time: 0 };
    pub const MASS: Dim = Dim { length: 0, angle: 0, mass: 1, time: 0 };
    pub const TIME: Dim = Dim { length: 0, angle: 0, mass: 0, time: 1 };

    pub fn is_none(self) -> bool {
        self == Dim::NONE
    }

    /// Combines dimensions for a product. Returns `None` on exponent overflow.
    pub fn times(self, o: Dim) -> Option<Dim> {
        Some(Dim {
            length: self.length.checked_add(o.length)?,
            angle: self.angle.checked_add(o.angle)?,
            mass: self.mass.checked_add(o.mass)?,
            time: self.time.checked_add(o.time)?,
        })
    }

    /// Combines dimensions for a quotient. Returns `None` on exponent overflow.
    pub fn per(self, o: Dim) -> Option<Dim> {
        Some(Dim {
            length: self.length.checked_sub(o.length)?,
            angle: self.angle.checked_sub(o.angle)?,
            mass: self.mass.checked_sub(o.mass)?,
            time: self.time.checked_sub(o.time)?,
        })
    }

    /// Raises the dimension to an integer power. Returns `None` on overflow.
    pub fn powi(self, n: i32) -> Option<Dim> {
        let p = |e: i8| -> Option<i8> { i8::try_from(i32::from(e).checked_mul(n)?).ok() };
        Some(Dim { length: p(self.length)?, angle: p(self.angle)?, mass: p(self.mass)?, time: p(self.time)? })
    }

    /// Halves every exponent; `None` if any exponent is odd.
    pub fn sqrt(self) -> Option<Dim> {
        let h = |e: i8| if e % 2 == 0 { Some(e / 2) } else { None };
        Some(Dim { length: h(self.length)?, angle: h(self.angle)?, mass: h(self.mass)?, time: h(self.time)? })
    }

    /// Short human name, used in error messages ("length", "area", "angle", ...).
    pub fn name(self) -> String {
        match self {
            Dim::NONE => "a plain number".into(),
            Dim::LENGTH => "a length".into(),
            Dim::AREA => "an area".into(),
            Dim::VOLUME => "a volume".into(),
            Dim::ANGLE => "an angle".into(),
            Dim::MASS => "a mass".into(),
            Dim::TIME => "a time".into(),
            d => {
                let mut parts = Vec::new();
                for (sym, e) in [("mm", d.length), ("rad", d.angle), ("kg", d.mass), ("s", d.time)] {
                    match e {
                        0 => {}
                        1 => parts.push(sym.to_string()),
                        e => parts.push(format!("{sym}^{e}")),
                    }
                }
                format!("a quantity in {}", parts.join("·"))
            }
        }
    }
}

/// A unit the user can type or display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unit {
    /// Canonical symbol (used for display).
    pub symbol: &'static str,
    /// Multiply a value in this unit by `factor` to get canonical units.
    pub factor: f64,
    pub dim: Dim,
}

/// All recognised unit spellings. Aliases map to the same factor.
const UNITS: &[(&str, Unit)] = &[
    ("mm", Unit { symbol: "mm", factor: 1.0, dim: Dim::LENGTH }),
    ("cm", Unit { symbol: "cm", factor: 10.0, dim: Dim::LENGTH }),
    ("m", Unit { symbol: "m", factor: 1000.0, dim: Dim::LENGTH }),
    ("um", Unit { symbol: "µm", factor: 0.001, dim: Dim::LENGTH }),
    ("µm", Unit { symbol: "µm", factor: 0.001, dim: Dim::LENGTH }),
    ("in", Unit { symbol: "in", factor: 25.4, dim: Dim::LENGTH }),
    ("inch", Unit { symbol: "in", factor: 25.4, dim: Dim::LENGTH }),
    ("ft", Unit { symbol: "ft", factor: 304.8, dim: Dim::LENGTH }),
    ("thou", Unit { symbol: "thou", factor: 0.0254, dim: Dim::LENGTH }),
    ("mil", Unit { symbol: "thou", factor: 0.0254, dim: Dim::LENGTH }),
    ("deg", Unit { symbol: "deg", factor: std::f64::consts::PI / 180.0, dim: Dim::ANGLE }),
    ("°", Unit { symbol: "deg", factor: std::f64::consts::PI / 180.0, dim: Dim::ANGLE }),
    ("rad", Unit { symbol: "rad", factor: 1.0, dim: Dim::ANGLE }),
    ("kg", Unit { symbol: "kg", factor: 1.0, dim: Dim::MASS }),
    ("g", Unit { symbol: "g", factor: 0.001, dim: Dim::MASS }),
    ("lb", Unit { symbol: "lb", factor: 0.453_592_37, dim: Dim::MASS }),
    ("s", Unit { symbol: "s", factor: 1.0, dim: Dim::TIME }),
];

/// Looks up a unit by any of its spellings.
pub fn unit(name: &str) -> Option<Unit> {
    UNITS.iter().find(|(n, _)| *n == name).map(|(_, u)| *u)
}

/// The two length unit systems a document can use for display and bare numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum LengthUnit {
    #[default]
    Millimetre,
    Inch,
}

impl LengthUnit {
    pub fn unit(self) -> Unit {
        match self {
            LengthUnit::Millimetre => Unit { symbol: "mm", factor: 1.0, dim: Dim::LENGTH },
            LengthUnit::Inch => Unit { symbol: "in", factor: 25.4, dim: Dim::LENGTH },
        }
    }
}

/// A value with a physical dimension, stored in canonical units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    pub value: f64,
    pub dim: Dim,
}

impl Quantity {
    pub const fn new(value: f64, dim: Dim) -> Self {
        Quantity { value, dim }
    }
    pub const fn number(value: f64) -> Self {
        Quantity { value, dim: Dim::NONE }
    }
    pub const fn mm(value: f64) -> Self {
        Quantity { value, dim: Dim::LENGTH }
    }
    pub fn deg(value: f64) -> Self {
        Quantity { value: value * std::f64::consts::PI / 180.0, dim: Dim::ANGLE }
    }
    pub const fn rad(value: f64) -> Self {
        Quantity { value, dim: Dim::ANGLE }
    }

    /// Value expressed in `unit`. The caller must ensure the dimensions match.
    pub fn in_unit(self, unit: Unit) -> f64 {
        self.value / unit.factor
    }

    /// Formats the quantity in the preferred display unit for its dimension.
    pub fn display(self, lengths: LengthUnit) -> String {
        let (v, sym) = match self.dim {
            Dim::NONE => (self.value, ""),
            Dim::LENGTH => {
                let u = lengths.unit();
                (self.in_unit(u), u.symbol)
            }
            Dim::ANGLE => (self.value.to_degrees(), "deg"),
            Dim::AREA => match lengths {
                LengthUnit::Millimetre => (self.value, "mm^2"),
                LengthUnit::Inch => (self.value / (25.4 * 25.4), "in^2"),
            },
            Dim::VOLUME => match lengths {
                LengthUnit::Millimetre => (self.value, "mm^3"),
                LengthUnit::Inch => (self.value / (25.4 * 25.4 * 25.4), "in^3"),
            },
            Dim::MASS => (self.value, "kg"),
            Dim::TIME => (self.value, "s"),
            _ => (self.value, ""),
        };
        let num = format_number(v);
        if sym.is_empty() {
            if self.dim.is_none() { num } else { format!("{num} ({})", self.dim.name()) }
        } else {
            format!("{num} {sym}")
        }
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display(LengthUnit::Millimetre))
    }
}

/// Formats a number with up to 6 significant decimals and no trailing zeros.
///
/// The output is deterministic and round-trips well enough for display; the canonical
/// text format uses [`format_exact`] instead.
pub fn format_number(v: f64) -> String {
    if !v.is_finite() {
        return format!("{v}");
    }
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

/// Formats a number so that parsing it back yields exactly the same `f64`.
pub fn format_exact(v: f64) -> String {
    // Rust's `{}` for f64 prints the shortest representation that round-trips.
    let s = format!("{v}");
    if s == "-0" { "0".to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dims_combine() {
        assert_eq!(Dim::LENGTH.times(Dim::LENGTH), Some(Dim::AREA));
        assert_eq!(Dim::VOLUME.per(Dim::LENGTH), Some(Dim::AREA));
        assert_eq!(Dim::AREA.sqrt(), Some(Dim::LENGTH));
        assert_eq!(Dim::VOLUME.sqrt(), None);
        assert_eq!(Dim::LENGTH.powi(3), Some(Dim::VOLUME));
        assert_eq!(Dim::LENGTH.powi(200), None);
    }

    #[test]
    fn display_units() {
        assert_eq!(Quantity::mm(25.4).display(LengthUnit::Inch), "1 in");
        assert_eq!(Quantity::mm(12.5).display(LengthUnit::Millimetre), "12.5 mm");
        assert_eq!(Quantity::deg(90.0).display(LengthUnit::Millimetre), "90 deg");
        assert_eq!(Quantity::number(-0.0).display(LengthUnit::Millimetre), "0");
    }

    #[test]
    fn exact_roundtrip() {
        for v in [0.1, 1.0 / 3.0, 1e-300, 123_456.789, -2.5] {
            assert_eq!(format_exact(v).parse::<f64>().unwrap().to_bits(), v.to_bits());
        }
    }
}
