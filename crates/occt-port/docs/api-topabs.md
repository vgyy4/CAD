# `topabs`: Rust API contract

Module `occt_port::topabs` translates the package `TopAbs` of OCCT 8.0.1: the enumerations `TopAbs_Orientation`, `TopAbs_ShapeEnum`, `TopAbs_State` and the class
`TopAbs` (`Compose`, `Reverse`, `Complement`, `Print`, the conversions to and from strings).

## API
```rust
#[repr(i32)] pub enum Orientation { Forward = 0, Reversed, Internal, External }      // TopAbs_Orientation
#[repr(i32)] pub enum ShapeEnum { Compound = 0, CompSolid, Solid, Shell, Face, Wire, Edge, Vertex, Shape }   // TopAbs_ShapeEnum
#[repr(i32)] pub enum State { In = 0, Out, On, Unknown }                             // TopAbs_State
pub fn compose(or1: Orientation, or2: Orientation) -> Orientation      // TopAbs::Compose: the table is indexed [or2][or1]
pub fn reverse(o: Orientation) -> Orientation, complement(o) -> Orientation
pub fn shape_type_to_string(t: ShapeEnum) -> &'static str              // "COMPOUND", "COMPSOLID", ..., "VERTEX", "SHAPE"
pub fn try_shape_type_from_string(s: &str) -> Option<ShapeEnum>         // the bool + out-parameter overload: the string is upper-cased (ASCII) first
pub fn shape_type_from_string(s: &str) -> ShapeEnum                    // the one-argument overload: Shape when the string is not a name
pub fn shape_orientation_to_string(o) -> &'static str, try_shape_orientation_from_string(s) -> Option<Orientation>, shape_orientation_from_string(s) -> Orientation (Forward if unknown)
pub fn print_shape(t) -> &'static str, print_orientation(o) -> &'static str, print_state(s: State) -> &'static str   // the text TopAbs::Print writes on the stream
```

## Behaviour notes
- `Print(TopAbs_State)` indexes its name table `{"ON", "IN", "OUT", "UNKNOWN"}` with the enumerator, whose order is `IN, OUT, ON, UNKNOWN`: `print_state(State::In)` is `"ON"`, `Out` is `"IN"`, `On` is `"OUT"`
  (reproduced; the table of the C++ is in a different order from the enumeration).
- The string conversions upper-case the argument with `TCollection_AsciiString::UpperCase` (`toupper` per byte); the translation uses `to_ascii_uppercase`, so the judge is ASCII only.
- The functions have no failure mode (an enumerator outside its range is undefined behaviour in C++ and not representable here): the judge has no `throws`.

## Judge
`oracle/topabs.cpp` (459 lines, 17 families: `compose, compose3, reverse, complement, reverse_complement, shape_to_string, shape_print, orientation_to_string, orientation_print, state_print,
shape_from_string_try, shape_from_string, orientation_from_string_try, orientation_from_string, shape_roundtrip, orientation_roundtrip, enum_values`) covers every enumerator and several hundred strings (names in
upper, lower and mixed case, with a suffix, truncated, random). All values are integers, compared exactly.
