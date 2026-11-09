// Test-only code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

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

#[test]
fn a_rolling_ball_blend_is_arcs_between_its_supports() {
    let r = 0.002;
    let mut f = TextFile::new();
    f.node(50, 2, &[("pvec", v3([0.0; 3])), ("normal", v3([0.0, 0.0, 1.0])), ("x_axis", v3([1.0, 0.0, 0.0]))]);
    f.node(50, 3, &[("pvec", v3([0.0; 3])), ("normal", v3([1.0, 0.0, 0.0])), ("x_axis", v3([0.0, 1.0, 0.0]))]);
    f.node(
        56,
        4,
        &[
            ("blend_type", vec![W::C(b'R')]),
            ("surface", vec![n(2), n(3)]),
            ("spine", vec![n(5)]),
            ("range", vec![n(r), n(r)]),
        ],
    );
    f.node(30, 5, &[("pvec", v3([r, 0.0, r])), ("direction", v3([0.0, 1.0, 0.0]))]);
    let g = decode(&f.finish(), &Limits::default()).unwrap();
    let hint = [[0.0, 0.0, 0.0], [0.0, 0.01, 0.0]];
    let grid = crate::eval::blend_grid(&g, 4, &hint).unwrap();
    assert_eq!(grid.len(), 9);
    for row in &grid {
        for p in row {
            let d = ((p[0] - r).powi(2) + (p[2] - r).powi(2)).sqrt();
            assert!((d - r).abs() < 1e-12, "{p:?}");
        }
        let (a, b) = (row[0], row[row.len() - 1]);
        assert!(a[2].abs() < 1e-12 && b[0].abs() < 1e-12, "{a:?} {b:?}");
        // The arc faces the corner.
        let mid = row[row.len() / 2];
        assert!(mid[0] < r && mid[2] < r, "{mid:?}");
    }
    let s = crate::eval::surface_through(&grid).unwrap();
    assert_eq!((s.degree, s.n), ([3, 3], [25, 25]));
}

#[test]
fn an_sp_curve_follows_its_surface() {
    let mut f = TextFile::new();
    f.node(50, 2, &[("pvec", v3([0.0, 0.0, 1.0])), ("normal", v3([0.0, 0.0, 1.0])), ("x_axis", v3([1.0, 0.0, 0.0]))]);
    f.node(137, 3, &[("surface", vec![n(2)]), ("b_curve", vec![n(4)])]);
    f.node(134, 4, &[("nurbs", vec![n(5)])]);
    f.node(
        136,
        5,
        &[
            ("degree", vec![n(1)]),
            ("n_vertices", vec![n(2)]),
            ("vertex_dim", vec![n(2)]),
            ("n_knots", vec![n(2)]),
            ("knot_type", vec![n(1)]),
            ("rational", vec![W::C(b'F')]),
            ("curve_form", vec![n(1)]),
            ("bspline_vertices", vec![n(6)]),
            ("knot_mult", vec![n(7)]),
            ("knots", vec![n(8)]),
        ],
    );
    f.node(45, 6, &[("vertices", vec![n(0), n(0), n(2), n(3)])]);
    f.node(127, 7, &[("mult", vec![n(2), n(2)])]);
    f.node(128, 8, &[("knots", vec![n(0), n(1)])]);
    let g = decode(&f.finish(), &Limits::default()).unwrap();
    let pts = crate::eval::curve_samples(&g, 3, 5, 0).unwrap();
    assert_eq!(pts.len(), 5);
    for (i, p) in pts.iter().enumerate() {
        let t = i as f64 / 4.0;
        assert!(
            (p[0] - 2.0 * t).abs() < 1e-12 && (p[1] - 3.0 * t).abs() < 1e-12 && (p[2] - 1.0).abs() < 1e-12,
            "{p:?}"
        );
    }
}

#[test]
fn assemblies_place_their_bodies() {
    let mut f = TextFile::new();
    // Assembly 1 holds body 4 twice (instance 2 turned 90 degrees about z and moved, then
    // scaled 2x; instance 5 as it is) and sub-assembly 6, moved by (0, 0, 1), holding it
    // once more.
    f.node(10, 1, &[("sub_instance", vec![n(2)])]);
    f.node(
        11,
        2,
        &[("type", vec![n(1)]), ("part", vec![n(4)]), ("transform", vec![n(3)]), ("next_in_part", vec![n(5)])],
    );
    let rot = [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0].map(n).to_vec();
    f.node(100, 3, &[("rotation_matrix", rot), ("translation_vector", v3([1.0, 0.0, 0.0])), ("scale", vec![n(2)])]);
    f.node(12, 4, &[("body_type", vec![n(1)])]);
    f.node(11, 5, &[("type", vec![n(1)]), ("part", vec![n(4)]), ("next_in_part", vec![n(7)])]);
    f.node(10, 6, &[("sub_instance", vec![n(9)])]);
    f.node(11, 7, &[("type", vec![n(1)]), ("part", vec![n(6)]), ("transform", vec![n(8)])]);
    let id = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0].map(n).to_vec();
    f.node(100, 8, &[("rotation_matrix", id), ("translation_vector", v3([0.0, 0.0, 1.0])), ("scale", vec![n(1)])]);
    f.node(11, 9, &[("type", vec![n(1)]), ("part", vec![n(4)]), ("transform", vec![n(3)])]);
    let g = decode(&f.finish(), &Limits::default()).unwrap();
    let mut w = Vec::new();
    let bodies = crate::model::body_nodes(&g, &mut w);
    assert_eq!(bodies.len(), 3, "{w:?}");
    let apply =
        |xf: &Option<crate::Xf>, p: [f64; 3]| xf.map_or(p, |(m, t)| crate::geom::add(crate::model::mat_vec(&m, p), t));
    // (R x + t) * 2: x = (1, 0, 0) -> R x = (0, 1, 0) -> + t = (1, 1, 0) -> (2, 2, 0).
    assert_eq!(apply(&bodies[0].1, [1.0, 0.0, 0.0]), [2.0, 2.0, 0.0]);
    assert_eq!(apply(&bodies[1].1, [1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
    assert_eq!(apply(&bodies[2].1, [1.0, 0.0, 0.0]), [2.0, 2.0, 1.0]);
}

#[test]
fn the_pole_order_that_fits_the_vertices_is_found() {
    // A bilinear patch with 2 x 3 poles (u-major), curved in z along v.
    let poles =
        vec![[0.0, 0.0, 0.0], [0.0, 1.0, 1.0], [0.0, 2.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 1.0], [1.0, 2.0, 0.0]];
    let s = crate::geom::Nurbs {
        degree: [1, 2],
        n: [2, 3],
        poles,
        weights: None,
        mults: [vec![2, 2], vec![3, 3]],
        knots: [vec![0.0, 1.0], vec![0.0, 1.0]],
        closed: [false, false],
    };
    let corner = [0.5, 1.0, 0.5];
    let right = crate::eval::nurbs_distance(&s, corner).unwrap();
    let wrong = crate::eval::nurbs_distance(&crate::eval::transposed_poles(&s), corner).unwrap();
    assert!(right < 1e-9 && wrong > 0.01, "{right} {wrong}");
}

#[test]
fn step_escaped_names_are_decoded() {
    assert_eq!(crate::model::decode_step_text("Corps de pi\\X2\\00E8\\X0\\ce.27"), "Corps de pièce.27");
    assert_eq!(crate::model::decode_step_text("plain"), "plain");
    assert_eq!(crate::model::decode_step_text("open \\X2\\00E8"), "open \\X2\\00E8");
}

#[test]
fn a_placement_moves_every_body() {
    // 90 degrees about z, then 0.005 m along x: the corner (10,10,10) mm of
    // the cube goes to (-10 + 5, 10, 10) mm.
    let p: crate::Xf = ([[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], [0.005, 0.0, 0.0]);
    let c = crate::to_step_placed(&cube(), "cube", &Limits::default(), Some(&p)).unwrap();
    assert!(c.step.contains("(-5.0,10.0,10.0)"), "{}", c.step);
    assert!(!c.step.contains("(10.0,10.0,10.0)"));
    let same = crate::to_step_placed(&cube(), "cube", &Limits::default(), None).unwrap();
    assert_eq!(same.step, to_step(&cube(), "cube", &Limits::default()).unwrap().step);
}

#[test]
fn a_cone_widens_along_its_axis() {
    // Radius 1 at the base point, half angle 45 deg: one unit along the axis the radius is 2
    // (checked on countersinks of SolidWorks and Solid Edge parts: read the other way, their
    // volumes disagree with their meshes).
    let mut f = TextFile::new();
    let h = std::f64::consts::FRAC_1_SQRT_2;
    f.node(
        52,
        2,
        &[
            ("pvec", v3([0.0, 0.0, 0.0])),
            ("axis", v3([0.0, 0.0, 1.0])),
            ("radius", vec![n(1)]),
            ("sin_half_angle", vec![n(h)]),
            ("cos_half_angle", vec![n(h)]),
            ("x_axis", v3([1.0, 0.0, 0.0])),
        ],
    );
    let g = decode(&f.finish(), &Limits::default()).unwrap();
    let p = crate::eval::surface_at(&g, 2, 0.0, 1.0, 0).unwrap();
    assert!((p[0] - 2.0).abs() < 1e-12 && p[1].abs() < 1e-12 && (p[2] - 1.0).abs() < 1e-12, "{p:?}");
    let q = crate::eval::project(&g, 2, [0.0, 2.0, 1.0]).unwrap();
    assert!((q[1] - 2.0).abs() < 1e-9 && (q[2] - 1.0).abs() < 1e-9, "a point on the cone stays: {q:?}");
}
