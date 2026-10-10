# gusset-meshio

Mesh exchange for Gusset (tier 3 of the interop plan in `PROMPT.md`). It reads and
writes **STL** (binary and ASCII), **3MF** (core spec) and **OBJ**. Every input file is
treated as hostile.

## API

```rust
use gusset_meshio::*;

// Import. The format is auto-detected from content, then from the extension.
let imported = import_file(path, &ImportOptions::default())?;   // or import_bytes(&bytes, None, &opts)
for m in &imported.meshes {
    // m.name, m.mesh (gusset_kernel::Mesh in mm), m.units_note, m.validation
}
imported.report.warnings; // e.g. "\"Body\": 12 degenerate triangles removed", "non-manifold edges: 3"

// Export: named meshes in mm. The output is deterministic.
let (bytes, report) = export_to_vec(&[ExportBody { name: "Body", mesh: &mesh }], ExportFormat::ThreeMf)?;
// ExportFormat::{StlBinary, StlAscii, ThreeMf, Obj { normals }}; or export(.., &mut impl Write)
```

- `ImportOptions` has these fields:
  - `limits: Limits`
  - `normals: NormalMode`: `Crease { angle_deg: 30 }` (the default), `Smooth` or `Flat`
  - `keep_degenerate`
  - `name_hint`
  - `cancel: Option<Arc<AtomicBool>>`
- `ImportedMesh.mesh.face_ranges` holds one triangle range per source object. That is one
  range per STL solid or OBJ object, and one per component mesh of a 3MF build item.
  `edges` is empty.
- `MeshValidation` reports `boundary_edges`, `non_manifold_edges`, `inconsistent_edges`,
  `degenerate_triangles` and `signed_volume`, plus the checks `is_watertight()` and
  `can_become_solid()`.
- `utils` provides these helpers: `weld_exact`, `weld_with_tolerance`, `weld_mesh`,
  `apply_normals`, `smooth_normals`, `validate`, `remove_degenerate_triangles` and
  `triangulate_polygon` (ear clipping).
- `MeshIoError` is a structured error with human-readable messages that say what went
  wrong and where, for example `STL triangle 1234 has a NaN coordinate`,
  `OBJ line 12: face index 0 is invalid` or
  `3MF part "3D/3dmodel.model" object 5 triangle 7: references vertex 900 but the object has 800 vertices`.

## Formats

| Format | Import | Export |
|---|---|---|
| STL | Binary and ASCII. Detection: ASCII only if the file starts with `solid` and is text, so binary files whose header says `solid` work. Multiple ASCII solids, polygons with more than 3 vertices (fan-triangulated), a missing `endsolid`, and trailing junk are each reported as warnings. | Binary (the header never starts with `solid`) or ASCII (one `solid` per body, shortest round-trip floats). |
| 3MF | OPC ZIP in memory. Root part from `_rels/.rels`. Objects with meshes or components, Production-extension `p:path` parts, build items with transforms (mirrored transforms flip winding), units converted to mm. Materials, colours and other extensions are ignored with a warning. | One object per body (exactly welded, collapsed triangles dropped), one build item each, `unit="millimeter"`. Fixed ZIP entry order, 1980-01-01 timestamps and a fixed deflate level. |
| OBJ | `v`, `f`/`fo` with `v`, `v/vt`, `v//vn` and `v/vt/vn`. Negative indices. Polygons are ear-clipped (fan fallback). Objects come from `o`, or from `g` when a file has no `o`. Materials, textures, lines and free-form geometry are ignored and reported. | `o` per body, welded `v`, optional `vn`, no MTL. |

STL and OBJ have no units, so they are assumed to be mm, and `units_note` says so.

## Hostile input

Every limit lives in `Limits`, with defaults as follows:

| Limit | Default |
|---|---|
| File size | 1 GiB |
| Triangles, counted after 3MF instancing | 50 M |
| Vertices | 50 M |
| Objects | 100 k |
| ZIP entries | 10 k |
| Total decompressed bytes | 2 GiB |
| Compression ratio (entries over 1 MiB) | 200:1 |
| XML depth | 64 |
| Attributes per element | 128 |
| Attribute length | 64 KiB |
| Component depth | 32 |
| Polygon corners | 64 k |

The importers apply these rules on top of the limits:

- **No panics, no unbounded allocation.** Declared counts (binary STL triangle count,
  ZIP sizes) are checked against the file size and limits before anything is
  allocated. Inflation stops at the declared size, and the CRC-32 is verified.
- **Zip bombs.** The importers check the ratio, enforce a total decompressed-size
  budget, and only inflate the parts they need.
- **Overlapping or lying entries.** These fail the size or CRC checks.
- **Paths.** Absolute paths, `..`, backslashes, drive letters, URLs, control characters
  and external relationship targets are rejected, both in ZIP entry names and in OPC
  targets. Nothing is ever extracted to disk.
- **XML.** The importers reject a DOCTYPE outright, so there is no DTD and no entity
  expansion.
- **Component graphs.** 3MF component graphs are checked for cycles and depth. The
  total number of instances is capped at 10× `max_objects`, which defeats exponential
  fan-out.
- **Bounded work in helpers.** Ear clipping has a per-file work budget. Crease-normal
  clustering is capped at 64 splits per vertex. The tolerance weld uses a grid.
- **Fast hashing.** Exact welding uses a fast multiply-mix hash with a per-process
  random seed. It is only used for lookups, so the output order stays deterministic.
- **Cancellation.** Long loops poll the cancel flag.

The following tests and fuzz targets cover this:

- `tests/` holds the round-trip, detection, fixture and hostile-input tests. Every 3MF
  fixture is a hand-written string, and no files are downloaded.
- `tests/mutation.rs` is a stable-toolchain random-mutation smoke test. Each mutated
  input that is accepted must re-export deterministically and re-import cleanly. It has
  run 1 M release iterations and 100 k debug iterations (with overflow checks) with no
  failures.
- `fuzz/` is a cargo-fuzz crate, excluded from the workspace. Its targets are
  `import_stl`, `import_obj`, `import_3mf` and `import_any` (import, then re-export in
  every format and re-import).

  Run it with:

  ```
  cd crates/gusset-meshio/fuzz && cargo +nightly fuzz run import_3mf
  ```

  The targets type-check on stable with `cargo check --bins`. They have not yet been run
  under libFuzzer because no nightly toolchain or cargo-fuzz was available in the
  development container.

## Performance and determinism

All code is scalar. Exports are deterministic: identical meshes always give
byte-identical files. That holds for a given `miniz_oxide` version, which `Cargo.lock`
pins. Floats use Rust's shortest round-trip formatting, so export followed by import is
bit-exact for f32 positions.

Measured with `cargo test --release -p gusset-meshio --test roundtrip -- --ignored --nocapture`
on a 2 M-triangle grid, using a 4-vCPU cloud container:

| Operation | Time |
|---|---|
| Binary STL write (95 MB) | ~0.15 s |
| Binary STL import (read, weld, validate, crease normals) | ~1.0 s |
| 3MF write (deflate level 3) | ~2.2 s |
| 3MF read | ~2.3 s |

**Future SIMD and parallel dispatch targets.** `PROMPT.md` lists STL and 3MF export as
dispatch targets. These hot spots could use it later, but output must stay
byte-identical, so the work belongs in display-independent integer and formatting code
paths, with scalar fallbacks and golden-file tests:

- **Face normals.** The f64 cross products in `utils::face_normal_raw` and the binary
  STL writer. They are used only for STL facet normals and display normals, not for
  model-defining data, but exported files must still be byte-identical across CPUs. Use
  a non-FMA formulation, or keep this path scalar.
- **Float-to-text formatting.** Formatting for 3MF, OBJ and ASCII STL, which is the
  dominant cost of 3MF export. A vectorised shortest-float formatter with identical
  output would help.
- **Deflate and CRC-32.** Deflate could run in parallel chunks with a fixed chunking
  scheme. CRC-32 could use PCLMUL or ARMv8 CRC instructions; the results are
  bit-identical by definition.
- **Weld hashing and edge sorting.** A radix sort on the packed edge keys in
  `validate_indexed`.

## Dependencies

| Crate | Version | License | Why | Vetting |
|---|---|---|---|---|
| `quick-xml` | 0.42.0 | MIT | 3MF and OPC XML parsing | Maintained by tafia, Mingun and dralley. About 460 M downloads. RUSTSEC-2026-0194 and RUSTSEC-2026-0195 (both DoS) are fixed in 0.41 or later. Used with default features off. Namespaces are resolved by local name, and `NsReader` is not used. |
| `miniz_oxide` | 0.9.1 | MIT OR Zlib OR Apache-2.0 | Deflate and inflate for the ZIP container | Pure Rust with `#![forbid(unsafe_code)]`. About 970 M downloads. No RustSec advisories. Only the `with-alloc` feature is enabled. |

Both crates pull in `memchr` (Unlicense OR MIT) and `adler2` (0BSD OR MIT OR Apache-2.0)
transitively.

The ZIP reader and writer are our own (about 300 lines), which keeps the zip-bomb and
path policies in one place. The `zip` crate's latest release (9.0.1) was published on
the day of vetting, and it brings far more surface than 3MF needs. `stl_io`, `tobj` and
`threemf` were not needed: the formats are small, and owning the parsers keeps the
limits and error messages uniform.

## Known gaps

- 3MF:
  - No materials, colours or textures.
  - No beam lattice, slice or other extensions (a warning is issued).
  - No thumbnails or metadata export.
  - No ZIP64 writing (parts over 4 GiB).
- OBJ:
  - `\` line continuations are not supported.
  - Faces that reference vertices defined later in the file are accepted.
- PLY and glTF are not implemented yet.
- `Mesh.edges` is left empty on import. Feature-edge extraction is not implemented.
