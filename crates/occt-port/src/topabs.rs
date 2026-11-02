//! OCCT's package `TopAbs`: the enumerations `TopAbs_Orientation`, `TopAbs_ShapeEnum`, `TopAbs_State` and the class `TopAbs`
//! (`Compose`, `Reverse`, `Complement`, `Print`, the conversions to and from strings).
//!
//! Translated from `TopAbs*.hxx/.cxx` (OCCT 8.0.1). See `docs/api-topabs.md`.

/// `TopAbs_Orientation`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// `TopAbs_FORWARD`.
    Forward = 0,
    /// `TopAbs_REVERSED`.
    Reversed = 1,
    /// `TopAbs_INTERNAL`.
    Internal = 2,
    /// `TopAbs_EXTERNAL`.
    External = 3,
}

/// `TopAbs_ShapeEnum`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeEnum {
    /// `TopAbs_COMPOUND`.
    Compound = 0,
    /// `TopAbs_COMPSOLID`.
    CompSolid = 1,
    /// `TopAbs_SOLID`.
    Solid = 2,
    /// `TopAbs_SHELL`.
    Shell = 3,
    /// `TopAbs_FACE`.
    Face = 4,
    /// `TopAbs_WIRE`.
    Wire = 5,
    /// `TopAbs_EDGE`.
    Edge = 6,
    /// `TopAbs_VERTEX`.
    Vertex = 7,
    /// `TopAbs_SHAPE`.
    Shape = 8,
}

/// `TopAbs_State`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// `TopAbs_IN`.
    In = 0,
    /// `TopAbs_OUT`.
    Out = 1,
    /// `TopAbs_ON`.
    On = 2,
    /// `TopAbs_UNKNOWN`.
    Unknown = 3,
}

const ALL_ORIENTATIONS: [Orientation; 4] =
    [Orientation::Forward, Orientation::Reversed, Orientation::Internal, Orientation::External];
const ALL_SHAPES: [ShapeEnum; 9] = [
    ShapeEnum::Compound,
    ShapeEnum::CompSolid,
    ShapeEnum::Solid,
    ShapeEnum::Shell,
    ShapeEnum::Face,
    ShapeEnum::Wire,
    ShapeEnum::Edge,
    ShapeEnum::Vertex,
    ShapeEnum::Shape,
];

// `TopAbs_Table_PrintShapeEnum`
const SHAPE_NAMES: [&str; 9] = ["COMPOUND", "COMPSOLID", "SOLID", "SHELL", "FACE", "WIRE", "EDGE", "VERTEX", "SHAPE"];
// `TopAbs_Table_PrintOrientation`
const ORIENTATION_NAMES: [&str; 4] = ["FORWARD", "REVERSED", "INTERNAL", "EXTERNAL"];

/// `TopAbs::Compose`: the table of the C++ is indexed `[Or2][Or1]`; the row of a forward `Or2` leaves `Or1` unchanged, the row of
/// a reversed one reverses a forward or reversed `Or1`, and an internal / external `Or2` gives itself.
pub fn compose(or1: Orientation, or2: Orientation) -> Orientation {
    use Orientation::{External, Forward, Internal, Reversed};
    match (or2, or1) {
        (Forward, o) => o,
        (Reversed, Forward) => Reversed,
        (Reversed, Reversed) => Forward,
        (Reversed, o) => o,
        (Internal, _) => Internal,
        (External, _) => External,
    }
}

/// `TopAbs::Reverse`.
pub fn reverse(o: Orientation) -> Orientation {
    match o {
        Orientation::Forward => Orientation::Reversed,
        Orientation::Reversed => Orientation::Forward,
        Orientation::Internal => Orientation::Internal,
        Orientation::External => Orientation::External,
    }
}

/// `TopAbs::Complement`.
pub fn complement(o: Orientation) -> Orientation {
    match o {
        Orientation::Forward => Orientation::Reversed,
        Orientation::Reversed => Orientation::Forward,
        Orientation::Internal => Orientation::External,
        Orientation::External => Orientation::Internal,
    }
}

/// `TopAbs::ShapeTypeToString`.
pub fn shape_type_to_string(t: ShapeEnum) -> &'static str {
    match t {
        ShapeEnum::Compound => "COMPOUND",
        ShapeEnum::CompSolid => "COMPSOLID",
        ShapeEnum::Solid => "SOLID",
        ShapeEnum::Shell => "SHELL",
        ShapeEnum::Face => "FACE",
        ShapeEnum::Wire => "WIRE",
        ShapeEnum::Edge => "EDGE",
        ShapeEnum::Vertex => "VERTEX",
        ShapeEnum::Shape => "SHAPE",
    }
}

/// `TopAbs::ShapeTypeFromString(string, type)`: the name is upper-cased first; `None` (C++: `false`, `type` unchanged)
/// when it is not the name of a type.
pub fn try_shape_type_from_string(s: &str) -> Option<ShapeEnum> {
    let name = s.to_ascii_uppercase();
    SHAPE_NAMES.iter().position(|&n| n == name).and_then(|i| ALL_SHAPES.get(i)).copied()
}

/// `TopAbs::ShapeTypeFromString(string)`: `TopAbs_SHAPE` when the string is not the name of a type.
pub fn shape_type_from_string(s: &str) -> ShapeEnum {
    try_shape_type_from_string(s).unwrap_or(ShapeEnum::Shape)
}

/// `TopAbs::ShapeOrientationToString`.
pub fn shape_orientation_to_string(o: Orientation) -> &'static str {
    match o {
        Orientation::Forward => "FORWARD",
        Orientation::Reversed => "REVERSED",
        Orientation::Internal => "INTERNAL",
        Orientation::External => "EXTERNAL",
    }
}

/// `TopAbs::ShapeOrientationFromString(string, orientation)`: the name is upper-cased first; `None` when it is not
/// the name of an orientation.
pub fn try_shape_orientation_from_string(s: &str) -> Option<Orientation> {
    let name = s.to_ascii_uppercase();
    ORIENTATION_NAMES.iter().position(|&n| n == name).and_then(|i| ALL_ORIENTATIONS.get(i)).copied()
}

/// `TopAbs::ShapeOrientationFromString(string)`: `TopAbs_FORWARD` when the string is not the name of an orientation.
pub fn shape_orientation_from_string(s: &str) -> Orientation {
    try_shape_orientation_from_string(s).unwrap_or(Orientation::Forward)
}

/// The text `TopAbs::Print(TopAbs_ShapeEnum, stream)` writes.
pub fn print_shape(t: ShapeEnum) -> &'static str {
    shape_type_to_string(t)
}

/// The text `TopAbs::Print(TopAbs_Orientation, stream)` writes.
pub fn print_orientation(o: Orientation) -> &'static str {
    shape_orientation_to_string(o)
}

/// The text `TopAbs::Print(TopAbs_State, stream)` writes (its name table is not in the order of the enumeration:
/// `State::In` prints `ON`, `Out` prints `IN`, `On` prints `OUT`).
pub fn print_state(s: State) -> &'static str {
    match s {
        State::In => "ON",
        State::Out => "IN",
        State::On => "OUT",
        State::Unknown => "UNKNOWN",
    }
}
