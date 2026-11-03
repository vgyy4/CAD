// Test-only code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use crate::schema::{self, Code, Count, Family};
use crate::{Limits, decode, to_step};
use std::collections::BTreeMap;

/// A value to write: a number token or a glued character.
#[derive(Clone)]
enum W {
    N(String),
    C(u8),
}

fn n(v: impl ToString) -> W {
    W::N(v.to_string())
}

/// Writes nodes as schema-12006 text, filling fields not given with null / unset values.
struct TextFile {
    s: schema::Schema,
    body: String,
}

impl TextFile {
    fn new() -> Self {
        let mut body = String::from(
            "**ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz***\n**PART2;SCH=SCH_1200000_12006;USFLD_SIZE=0;\n**END_OF_HEADER*****\n",
        );
        let m = ": TRANSMIT FILE created by modeller version 1200000";
        let k = "SCH_1200000_12006";
        body.push_str(&format!("T{} {m}{} {k}0 ", m.len(), k.len()));
        TextFile { s: schema::schema(Family::V12), body }
    }

    fn push(&mut self, w: &W) {
        match w {
            W::N(t) => {
                self.body.push_str(t);
                self.body.push(' ');
            }
            W::C(c) => self.body.push(char::from(*c)),
        }
    }

    fn node(&mut self, ty: u16, idx: u32, fields: &[(&str, Vec<W>)]) {
        let layout = self.s.get(&ty).cloned().unwrap();
        let given: BTreeMap<&str, &Vec<W>> = fields.iter().map(|(k, v)| (*k, v)).collect();
        self.push(&n(ty));
        if layout.variable() {
            let var = layout.fields.iter().find(|f| f.count == Count::Var).unwrap();
            self.push(&n(given.get(var.name.as_str()).map_or(0, |v| v.len())));
        }
        self.push(&n(idx));
        for f in &layout.fields {
            if let Some(v) = given.get(f.name.as_str()) {
                for w in v.iter() {
                    self.push(w);
                }
                continue;
            }
            let k = match f.count {
                Count::One => 1,
                Count::Fixed(k) => usize::from(k),
                Count::Var => 0,
            };
            for _ in 0..k {
                let d = match f.code {
                    Code::C => W::C(b'+'),
                    Code::L => W::C(b'F'),
                    Code::F | Code::V | Code::H => n("?"),
                    Code::I => {
                        self.push(&n(0));
                        n(0)
                    }
                    _ => n(0),
                };
                self.push(&d);
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        self.body.push_str("1 0");
        // Wrap like real files: 80 columns, splitting tokens anywhere.
        let mut out = String::new();
        let (head, data) = self.body.split_at(self.body.find("T5").unwrap());
        out.push_str(head);
        // A separating space never ends a line (trailing spaces are ignored): it starts
        // the next one.
        let mut line = String::new();
        for c in data.chars() {
            if line.len() >= 80 && c != ' ' && !line.ends_with(' ') {
                out.push_str(&line);
                out.push('\n');
                line.clear();
            }
            line.push(c);
        }
        out.push_str(&line);
        out.into_bytes()
    }
}

fn v3(p: [f64; 3]) -> Vec<W> {
    p.iter().map(|x| n(format!("{x:?}"))).collect()
}

/// A 10 mm cube as a V12 text file.
fn cube() -> Vec<u8> {
    let s = 0.01;
    let pos = |v: usize| [(v & 1) as f64 * s, ((v >> 1) & 1) as f64 * s, ((v >> 2) & 1) as f64 * s];
    // Faces: corners counter-clockwise seen from outside, and the outward normal.
    let faces: [([usize; 4], [f64; 3]); 6] = [
        ([0, 2, 3, 1], [0.0, 0.0, -1.0]),
        ([4, 5, 7, 6], [0.0, 0.0, 1.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
        ([2, 6, 7, 3], [0.0, 1.0, 0.0]),
        ([0, 4, 6, 2], [-1.0, 0.0, 0.0]),
        ([1, 3, 7, 5], [1.0, 0.0, 0.0]),
    ];
    let mut next = 2u32;
    let mut id = || {
        let i = next;
        next += 1;
        i
    };
    let (shell, solid, void) = (id(), id(), id());
    let vert: Vec<u32> = (0..8).map(|_| id()).collect();
    let point: Vec<u32> = (0..8).map(|_| id()).collect();
    let face: Vec<u32> = (0..6).map(|_| id()).collect();
    let plane: Vec<u32> = (0..6).map(|_| id()).collect();
    let lp: Vec<u32> = (0..6).map(|_| id()).collect();
    let fin: Vec<Vec<u32>> = (0..6).map(|_| (0..4).map(|_| id()).collect()).collect();
    // Edges from lower to higher vertex.
    let mut edges: BTreeMap<(usize, usize), (u32, u32)> = BTreeMap::new();
    for (corners, _) in &faces {
        for k in 0..4 {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let key = (a.min(b), a.max(b));
            if let std::collections::btree_map::Entry::Vacant(slot) = edges.entry(key) {
                let e = id();
                slot.insert((e, id()));
            }
        }
    }
    // Fins of each edge: (positive, negative), and each fin's face, position and forward vertex.
    let mut pos_fin: BTreeMap<(usize, usize), u32> = BTreeMap::new();
    let mut neg_fin: BTreeMap<(usize, usize), u32> = BTreeMap::new();
    for (fi, (corners, _)) in faces.iter().enumerate() {
        for k in 0..4 {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let key = (a.min(b), a.max(b));
            if a < b {
                pos_fin.insert(key, fin[fi][k])
            } else {
                neg_fin.insert(key, fin[fi][k])
            };
        }
    }
    let mut f = TextFile::new();
    f.node(
        12,
        1,
        &[
            ("highest_node_id", vec![n(next)]),
            ("res_size", vec![n("1e3")]),
            ("res_linear", vec![n("1e-8")]),
            ("body_type", vec![n(1)]),
            ("shell", vec![n(shell)]),
            ("region", vec![n(void)]),
        ],
    );
    f.node(19, void, &[("body", vec![n(1)]), ("next", vec![n(solid)]), ("type", vec![W::C(b'V')])]);
    f.node(19, solid, &[("body", vec![n(1)]), ("shell", vec![n(shell)]), ("type", vec![W::C(b'S')])]);
    f.node(13, shell, &[("body", vec![n(1)]), ("face", vec![n(face[0])]), ("region", vec![n(solid)])]);
    for v in 0..8 {
        f.node(29, point[v], &[("pvec", v3(pos(v)))]);
        f.node(18, vert[v], &[("point", vec![n(point[v])])]);
    }
    for (fi, (corners, normal)) in faces.iter().enumerate() {
        let next_face = face.get(fi + 1).copied().unwrap_or(0);
        f.node(
            14,
            face[fi],
            &[
                ("next", vec![n(next_face)]),
                ("loop", vec![n(lp[fi])]),
                ("shell", vec![n(shell)]),
                ("surface", vec![n(plane[fi])]),
                ("sense", vec![W::C(b'+')]),
            ],
        );
        let x = if normal[0].abs() > 0.5 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
        f.node(50, plane[fi], &[("pvec", v3(pos(corners[0]))), ("normal", v3(*normal)), ("x_axis", v3(x))]);
        f.node(15, lp[fi], &[("fin", vec![n(fin[fi][0])]), ("face", vec![n(face[fi])])]);
        for k in 0..4 {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let key = (a.min(b), a.max(b));
            let (e, _) = edges[&key];
            let other = if a < b { neg_fin[&key] } else { pos_fin[&key] };
            f.node(
                17,
                fin[fi][k],
                &[
                    ("loop", vec![n(lp[fi])]),
                    ("forward", vec![n(fin[fi][(k + 1) % 4])]),
                    ("backward", vec![n(fin[fi][(k + 3) % 4])]),
                    ("vertex", vec![n(vert[b])]),
                    ("other", vec![n(other)]),
                    ("edge", vec![n(e)]),
                    ("sense", vec![W::C(if a < b { b'+' } else { b'-' })]),
                ],
            );
        }
    }
    for ((a, b), (e, l)) in &edges {
        f.node(16, *e, &[("fin", vec![n(pos_fin[&(*a, *b)])]), ("curve", vec![n(*l)])]);
        let d = crate::geom::sub(pos(*b), pos(*a));
        f.node(30, *l, &[("pvec", v3(pos(*a))), ("direction", v3(crate::geom::scale(d, 1.0 / s)))]);
    }
    f.finish()
}

#[test]
fn a_synthetic_v12_cube_becomes_a_closed_step_solid() {
    let bytes = cube();
    let g = decode(&bytes, &Limits::default()).unwrap();
    assert_eq!(g.schema_number, 12006);
    let c = to_step(&bytes, "cube", &Limits::default()).unwrap();
    assert_eq!(c.bodies, 1);
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    let count = |k: &str| c.step.matches(&format!("={k}(")).count();
    assert_eq!(count("ADVANCED_FACE"), 6);
    assert_eq!(count("EDGE_CURVE"), 12);
    assert_eq!(count("VERTEX_POINT"), 8);
    assert_eq!(count("MANIFOLD_SOLID_BREP"), 1);
    assert_eq!(count("CLOSED_SHELL"), 1);
    // Millimetres: a corner at 10 mm.
    assert!(c.step.contains("(10.0,10.0,10.0)"), "{}", c.step);
    // Every edge is used once in each direction.
    let along = c.step.matches(",.T.);").count();
    assert!(along >= 12, "{along}");
}

#[test]
fn damaged_and_unsupported_files_are_errors_not_panics() {
    let bytes = cube();
    for cut in [10, 200, 600, bytes.len() / 2, bytes.len() - 3] {
        assert!(to_step(&bytes[..cut], "x", &Limits::default()).is_err(), "{cut}");
    }
    let old = String::from_utf8(bytes.clone()).unwrap().replace("SCH_1200000_12006", "SCH_30000000_3000");
    let e = to_step(old.as_bytes(), "x", &Limits::default()).unwrap_err();
    assert!(e.to_string().contains("older than Parasolid V4"), "{e}");
    let tight = Limits { max_nodes: 5, ..Limits::default() };
    assert!(matches!(to_step(&bytes, "x", &tight), Err(crate::XtError::TooLarge(_))));
    assert!(to_step(b"hello", "x", &Limits::default()).is_err());
    // Random bytes after a valid header.
    let mut junk = bytes[..bytes.iter().position(|&b| b == b'T').unwrap()].to_vec();
    junk.extend((0..2000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8));
    let _ = to_step(&junk, "x", &Limits::default());
}

/// The public-domain NIST samples, when a checkout is named by `GUSSET_XT_SAMPLES`
/// (usnistgov/engineering-design-models/models).
#[test]
fn nist_samples_decode_when_available() {
    let Ok(dir) = std::env::var("GUSSET_XT_SAMPLES") else { return };
    let dir = std::path::Path::new(&dir);
    for (file, faces) in [
        ("Bentley/nut.x_t", 0),
        ("Parasolid/NIST/Pete_Brown/foo/foo.xmt_txt", 88),
        ("Parasolid/NIST/Pete_Brown/flange/flange.xmt_txt", 29),
        ("Parasolid/NIST/Bill_Regli/keyway_ex/keyway_ex.xmt_txt", 25),
    ] {
        let bytes = std::fs::read(dir.join(file)).unwrap();
        let c = to_step(&bytes, "sample", &Limits::default()).unwrap_or_else(|e| panic!("{file}: {e}"));
        if let Ok(out) = std::env::var("GUSSET_XT_OUT") {
            let stem = std::path::Path::new(file).file_stem().unwrap().to_string_lossy().into_owned();
            std::fs::write(std::path::Path::new(&out).join(format!("{stem}.step")), &c.step).unwrap();
        }
        let n = c.step.matches("=ADVANCED_FACE(").count();
        if faces > 0 {
            assert_eq!(n, faces, "{file}: {:?}", c.warnings);
        }
    }
}

#[test]
fn mutated_files_never_panic() {
    let base = cube();
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let limits = Limits { max_nodes: 10_000, max_elements: 100_000 };
    for _ in 0..3000 {
        let mut b = base.clone();
        for _ in 0..(rnd() % 4 + 1) {
            let i = (rnd() as usize) % b.len();
            b[i] = match rnd() % 4 {
                0 => b'0' + (rnd() % 10) as u8,
                1 => b' ',
                2 => b"?+-TF.e"[(rnd() % 7) as usize],
                _ => rnd() as u8,
            };
        }
        let _ = to_step(&b, "x", &limits);
    }
}
