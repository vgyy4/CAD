//! Writes a small, valid seed corpus for every fuzz target into `<out>/<target>/`.
//!
//! Several formats are checksummed (BLAKE3 per record in `.gus`, BLAKE3 per worker
//! frame), so a fuzzer starting from nothing would never get past the checksum checks.
//! Valid seeds let it mutate the structure behind them. Everything is generated from
//! code in this repository; nothing is downloaded.
//!
//! Usage (stable toolchain is fine): `cargo run --example make_seeds -- corpus`

use std::path::Path;

use gusset_history::{BranchId, Ctx, History, NodeState};
use gusset_kernel::Mesh;
use gusset_meshio::{ExportBody, ExportFormat, export_to_vec};
use gusset_project::ProjectMeta;
use gusset_store::container::{ContainerWriter, Kind, SaveInfo};
use gusset_store::paths::ProjectId;
use gusset_worker::{Body, Format, encode_request, encode_response};

const BRACKET: &str = include_str!("../../examples/bracket.gus.txt");

fn write(out: &Path, target: &str, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let dir = out.join(target);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(name), bytes)
}

fn cube() -> Mesh {
    let positions: Vec<[f32; 3]> = (0..8u8)
        .map(|i| [f32::from(i & 1) * 10.0, f32::from((i >> 1) & 1) * 10.0, f32::from((i >> 2) & 1) * 10.0])
        .collect();
    #[rustfmt::skip]
    let indices = vec![
        0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4,
        2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6, 1, 3, 5, 3, 7, 5,
    ];
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    Mesh { positions, normals, face_ranges: std::iter::once(0..12).collect(), indices, edges: Vec::new() }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).ok_or("usage: make_seeds <out-dir>")?;
    let out = Path::new(&out);

    // Code-view text and unit expressions.
    write(out, "doc_text", "bracket.gus.txt", BRACKET.as_bytes())?;
    write(out, "doc_text", "minimal.gus.txt", b"gusset 1\nunits in\n\nparam w = 2 in / 3\n")?;
    // Every complete example of grammar.md, and the extra v0.2 documents in seeds-src (body
    // operations, face keys, holes, splines, sketch patterns, a generated helical gear; the
    // test gusset-doc/tests/fuzz_seeds.rs keeps them valid and covering the grammar).
    let mut block: Option<String> = None;
    let mut n = 0;
    for line in gusset_doc::TEXT_GRAMMAR.lines() {
        match (&mut block, line.trim_end()) {
            (None, "```gusset") => block = Some(String::new()),
            (Some(_), "```") => {
                if let Some(b) = block.take() {
                    n += 1;
                    write(out, "doc_text", &format!("grammar{n}.gus.txt"), b.as_bytes())?;
                }
            }
            (Some(b), l) => {
                b.push_str(l);
                b.push('\n');
            }
            (None, _) => {}
        }
    }
    let extra = Path::new(env!("CARGO_MANIFEST_DIR")).join("seeds-src/doc_text");
    for entry in std::fs::read_dir(extra)? {
        let path = entry?.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            write(out, "doc_text", name, &std::fs::read(&path)?)?;
        }
    }
    for (i, e) in ["width/2 + 0.5 in", "atan2(h, w) * 2", "30 deg", "-(1 + 2) ^ 3 ^ 2", "sqrt(4 mm^2)", "1e-7 m"]
        .iter()
        .enumerate()
    {
        write(out, "units_expr", &format!("expr{i}"), e.as_bytes())?;
    }

    // A real project file (and its records as a bare container).
    let doc = gusset_doc::Document::from_text(BRACKET).map_err(|e| e.to_string())?;
    let ctx = Ctx::user(1_700_000_000);
    let mut history = History::new(&doc, &ctx);
    let mut edited = doc.clone();
    edited.features.pop();
    history.commit(&ctx, history.main(), &edited, "remove last feature", NodeState::Ok)?;
    let branch: BranchId = history.create_branch(&ctx, history.head(history.main())?, Some("idea"))?;
    history.commit(&ctx, branch, &doc, "restore", NodeState::Broken { message: "seed".into() })?;
    let meta = ProjectMeta { id: ProjectId([7; 16]), save_seq: 3, base_seq: 2, created: 1_700_000_000 };
    let gus = gusset_project::encode(&history, &meta)?;
    write(out, "project_decode", "bracket.gus", &gus)?;
    write(out, "container_open", "bracket.gus", &gus)?;
    write(out, "container_salvage", "bracket.gus", &gus)?;
    let mut w = ContainerWriter::new();
    w.add(Kind::BLOB, &[0u8; 4096])?;
    w.add(Kind::FEATURE, b"gusset feature 1\n")?;
    w.set_root("settings", b"units=mm")?;
    let small = w.finish(&SaveInfo { save_seq: 1, created_unix: 0, app_version: "seed" })?;
    write(out, "container_open", "small.gus", &small)?;
    let recovery = gusset_store::recovery::encode(&[(1, &small[..]), (2, &gus[..])]);
    write(out, "container_salvage", "recovery.grec", &recovery)?;

    // Meshes in every export format.
    let mesh = cube();
    let bodies = [ExportBody { name: "cube", mesh: &mesh }];
    for (target, file, format) in [
        ("mesh_stl", "cube_binary.stl", ExportFormat::StlBinary),
        ("mesh_stl", "cube_ascii.stl", ExportFormat::StlAscii),
        ("mesh_obj", "cube.obj", ExportFormat::Obj { normals: true }),
        ("mesh_3mf", "cube.3mf", ExportFormat::ThreeMf),
    ] {
        let (bytes, _) = export_to_vec(&bodies, format)?;
        write(out, target, file, &bytes)?;
        write(out, "mesh_any", file, &bytes)?;
    }

    // Worker protocol frames.
    write(out, "worker_request", "step", &encode_request(Format::Step, b"ISO-10303-21;\nEND-ISO-10303-21;\n"))?;
    write(out, "worker_request", "iges", &encode_request(Format::Iges, b""))?;
    write(out, "worker_request", "brep", &encode_request(Format::Brep, b"\x00\x01Open CASCADE Topology V3"))?;
    let ok = Ok(vec![
        Body { name: "Body".into(), brep: b"DBRep_DrawableShape\n".to_vec() },
        Body { name: String::new(), brep: Vec::new() },
    ]);
    let mut noisy = b"stray OCCT output on stdout\n".to_vec();
    noisy.extend_from_slice(&encode_response(&ok, "landlock: full; seccomp: on"));
    write(out, "worker_response", "ok", &noisy)?;
    write(out, "worker_response", "err", &encode_response(&Err("no shapes".into()), ""))?;

    Ok(())
}
