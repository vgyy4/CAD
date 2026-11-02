//! Differential judge for OCCT's package `TopAbs` (OCCT 8.0.1): replays `tests/topabs.txt`, printed by `oracle/topabs.cpp`
//! against the real library. See `docs/api-topabs.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
use occt_port::topabs::{self, Orientation, ShapeEnum, State};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const DATA: &str = include_str!("topabs.txt");
const GOLDEN_LINES: usize = 459;
// the smallest families (an enumeration of four values) have four distinct inputs
const MIN_DISTINCT: usize = 4;
const FAMILIES: &[&str] = &[
    "complement",
    "compose",
    "compose3",
    "enum_values",
    "orientation_from_string",
    "orientation_from_string_try",
    "orientation_print",
    "orientation_roundtrip",
    "orientation_to_string",
    "reverse",
    "reverse_complement",
    "shape_from_string",
    "shape_from_string_try",
    "shape_print",
    "shape_roundtrip",
    "shape_to_string",
    "state_print",
];

const ORIENTATIONS: [Orientation; 4] =
    [Orientation::Forward, Orientation::Reversed, Orientation::Internal, Orientation::External];
const SHAPES: [ShapeEnum; 9] = [
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
const STATES: [State; 4] = [State::In, State::Out, State::On, State::Unknown];

struct Rd<'a> {
    w: Vec<&'a str>,
    p: usize,
}
impl<'a> Rd<'a> {
    fn new(words: &[&'a str]) -> Self {
        Self { w: words.to_vec(), p: 0 }
    }
    fn i(&mut self) -> i64 {
        let s = self.w[self.p];
        self.p += 1;
        s.parse().unwrap()
    }
    fn finish(&self) {
        assert_eq!(self.p, self.w.len(), "unread input tokens");
    }
    /// A string: its length and the codes of its characters.
    fn string(&mut self) -> String {
        let n = self.i();
        (0..n).map(|_| char::from(u8::try_from(self.i()).unwrap())).collect()
    }
}

fn orientation(v: i64) -> Orientation {
    ORIENTATIONS[usize::try_from(v).unwrap()]
}

/// A string result as its length and the codes of its characters.
fn text(s: &str) -> Vec<i64> {
    let mut v = vec![i64::try_from(s.len()).unwrap()];
    v.extend(s.bytes().map(i64::from));
    v
}

fn run(family: &str, words: &[&str]) -> Vec<i64> {
    let mut rd = Rd::new(words);
    let rd = &mut rd;
    let out = match family {
        "compose" => {
            let (a, b) = (rd.i(), rd.i());
            vec![topabs::compose(orientation(a), orientation(b)) as i64]
        }
        "compose3" => {
            let (a, b, c) = (rd.i(), rd.i(), rd.i());
            vec![topabs::compose(topabs::compose(orientation(a), orientation(b)), orientation(c)) as i64]
        }
        "reverse" => vec![topabs::reverse(orientation(rd.i())) as i64],
        "complement" => vec![topabs::complement(orientation(rd.i())) as i64],
        "reverse_complement" => {
            let a = orientation(rd.i());
            vec![topabs::reverse(topabs::complement(a)) as i64, topabs::complement(topabs::reverse(a)) as i64]
        }
        "shape_to_string" => text(topabs::shape_type_to_string(SHAPES[usize::try_from(rd.i()).unwrap()])),
        "shape_print" => text(topabs::print_shape(SHAPES[usize::try_from(rd.i()).unwrap()])),
        "orientation_to_string" => text(topabs::shape_orientation_to_string(orientation(rd.i()))),
        "orientation_print" => text(topabs::print_orientation(orientation(rd.i()))),
        "state_print" => text(topabs::print_state(STATES[usize::try_from(rd.i()).unwrap()])),
        "shape_from_string_try" => {
            let s = rd.string();
            match topabs::try_shape_type_from_string(&s) {
                Some(t) => vec![1, t as i64],
                None => vec![0, ShapeEnum::Vertex as i64],
            }
        }
        "shape_from_string" => vec![topabs::shape_type_from_string(&rd.string()) as i64],
        "orientation_from_string_try" => {
            let s = rd.string();
            match topabs::try_shape_orientation_from_string(&s) {
                Some(t) => vec![1, t as i64],
                None => vec![0, Orientation::External as i64],
            }
        }
        "orientation_from_string" => vec![topabs::shape_orientation_from_string(&rd.string()) as i64],
        "shape_roundtrip" => {
            let t = SHAPES[usize::try_from(rd.i()).unwrap()];
            match topabs::try_shape_type_from_string(topabs::shape_type_to_string(t)) {
                Some(b) => vec![1, b as i64],
                None => vec![0, ShapeEnum::Shape as i64],
            }
        }
        "orientation_roundtrip" => {
            let t = orientation(rd.i());
            match topabs::try_shape_orientation_from_string(topabs::shape_orientation_to_string(t)) {
                Some(b) => vec![1, b as i64],
                None => vec![0, Orientation::Forward as i64],
            }
        }
        "enum_values" => {
            let a = rd.i();
            let o = [Orientation::Forward, Orientation::Reversed, Orientation::Internal, Orientation::External];
            let s = [State::In, State::Out, State::On, State::Unknown];
            let t = [ShapeEnum::Compound, ShapeEnum::Vertex, ShapeEnum::Shape, ShapeEnum::Face];
            let k = usize::try_from(a).unwrap();
            vec![o[k] as i64, s[k] as i64, t[k] as i64]
        }
        _ => panic!("unknown family {family}"),
    };
    rd.finish();
    out
}

#[test]
fn topabs_matches_occt() {
    let mut seen = HashSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut distinct = BTreeMap::<String, BTreeSet<String>>::new();
    let mut lines = 0;
    for (n, line) in DATA.lines().enumerate() {
        let (left, right) = line.split_once(" |").unwrap_or_else(|| panic!("line {} malformed", n + 1));
        let mut it = left.split_whitespace();
        let family = it.next().unwrap();
        let words: Vec<&str> = it.collect();
        assert!(seen.insert(line.to_owned()), "duplicate line {}", n + 1);
        *counts.entry(family.to_owned()).or_default() += 1;
        distinct.entry(family.to_owned()).or_default().insert(left.to_owned());
        let want: Vec<i64> = right.split_whitespace().map(|w| w.parse().unwrap()).collect();
        assert_ne!(right.trim(), "throws", "TopAbs has no exception: {line}");
        assert_eq!(run(family, &words), want, "{family}\n{line}");
        lines += 1;
    }
    assert_eq!(lines, GOLDEN_LINES);
    let mut want_fams = FAMILIES.to_vec();
    want_fams.sort_unstable();
    assert_eq!(counts.keys().map(String::as_str).collect::<Vec<_>>(), want_fams);
    for name in FAMILIES {
        assert!(distinct[*name].len() >= MIN_DISTINCT, "{name} needs {MIN_DISTINCT} distinct inputs");
    }
    println!("topabs: {} families, {lines} lines, all values identical", counts.len());
}
