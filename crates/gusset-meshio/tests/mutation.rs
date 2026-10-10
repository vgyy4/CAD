//! Stable-toolchain mutation smoke test: random byte mutations of valid files must never
//! panic. (The real fuzzing lives in `fuzz/`, which needs nightly + cargo-fuzz.)
//! More iterations: `GUSSET_MUTATION_ITERS=200000 cargo test --release --test mutation`.

#![allow(
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::single_range_in_vec_init,
    reason = "integration-test helpers get the same allowances clippy.toml gives #[test] functions"
)]

mod common;

use common::{cube, model, package, tetra_object};
use gusset_meshio::{ExportBody, ExportFormat, ImportOptions, Limits, export_to_vec, import_bytes};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Seeds; `true` marks 3MF model XML that is packaged *after* mutation so mutations
/// reach the XML parser instead of failing the ZIP CRC check.
fn seeds() -> Vec<(bool, Vec<u8>)> {
    let c = cube(3.0);
    let b = [ExportBody { name: "c", mesh: &c }];
    let mut v: Vec<(bool, Vec<u8>)> =
        [ExportFormat::StlBinary, ExportFormat::StlAscii, ExportFormat::ThreeMf, ExportFormat::Obj { normals: true }]
            .iter()
            .map(|f| (false, export_to_vec(&b, *f).unwrap().0))
            .collect();
    let res = format!(
        r#"{}<object id="2"><components><component objectid="1" transform="1 0 0 0 1 0 0 0 1 1 2 3"/></components></object>"#,
        tetra_object(1, "t")
    );
    // Stored (uncompressed) entries so mutations hit the XML, not just the deflate stream.
    let xml = model("inch", &res, r#"<item objectid="2"/>"#);
    v.push((true, xml.clone().into_bytes()));
    v.push((false, package(&xml)));
    v.push((
        false,
        b"v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\no a\nf 1 2 3 4\nf -1 -2 -3\ng b\nf 1/1/1 3//2 5\n".to_vec(),
    ));
    v
}

#[test]
fn random_mutations_never_panic() {
    let iters: usize = std::env::var("GUSSET_MUTATION_ITERS").ok().and_then(|s| s.parse().ok()).unwrap_or(1500);
    let opts = ImportOptions {
        limits: Limits {
            max_triangles: 100_000,
            max_vertices: 100_000,
            max_decompressed_bytes: 16 << 20,
            ..Default::default()
        },
        ..Default::default()
    };
    let seeds = seeds();
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let tokens: &[&[u8]] = &[
        b"solid",
        b"facet",
        b"vertex",
        b"endloop",
        b"-1",
        b"0",
        b"4294967295",
        b"nan",
        b"1e39",
        b"\"",
        b"<",
        b"/>",
        b"<a>",
        b"objectid=\"1\"",
        b"transform=\"",
        b"f ",
        b"../",
        b"\n",
        b"\0",
        b"\xff\xff\xff\xff",
    ];
    for i in 0..iters {
        let (wrap, seed) = &seeds[i % seeds.len()];
        let mut d = seed.clone();
        for _ in 0..1 + rng.below(8) {
            match rng.below(5) {
                0 if !d.is_empty() => {
                    let k = rng.below(d.len());
                    d[k] = rng.next() as u8;
                }
                1 if !d.is_empty() => {
                    let k = rng.below(d.len());
                    d.truncate(k);
                }
                2 => {
                    let k = rng.below(d.len() + 1);
                    let t = tokens[rng.below(tokens.len())];
                    d.splice(k..k, t.iter().copied());
                }
                3 if d.len() > 4 => {
                    let k = rng.below(d.len() - 4);
                    let v = rng.next() as u32;
                    d[k..k + 4].copy_from_slice(&v.to_le_bytes());
                }
                _ if !d.is_empty() => {
                    let a = rng.below(d.len());
                    let b = (a + rng.below(64)).min(d.len());
                    d.drain(a..b);
                }
                _ => {}
            }
        }
        if *wrap {
            d = common::zip(&[
                common::ZEntry::new("_rels/.rels", common::RELS.as_bytes()),
                common::ZEntry { deflate: false, ..common::ZEntry::new("3D/3dmodel.model", &d) },
            ]);
        }
        // Any result is fine; panics fail the test. Accepted input must survive a
        // deterministic export and re-import in every format.
        let Ok(imp) = import_bytes(&d, None, &opts) else { continue };
        let bodies: Vec<ExportBody<'_>> =
            imp.meshes.iter().map(|m| ExportBody { name: &m.name, mesh: &m.mesh }).collect();
        for f in [
            ExportFormat::StlBinary,
            ExportFormat::StlAscii,
            ExportFormat::ThreeMf,
            ExportFormat::Obj { normals: false },
        ] {
            let a = export_to_vec(&bodies, f).unwrap().0;
            assert_eq!(a, export_to_vec(&bodies, f).unwrap().0, "{f:?} not deterministic");
            if let Err(e) = import_bytes(&a, None, &opts) {
                assert!(matches!(e, gusset_meshio::MeshIoError::Empty { .. }), "{f:?} re-import: {e}");
            }
        }
    }
}
