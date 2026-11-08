# Code review: OpenSCAD features, SolidWorks assemblies, renderer, app (2026-11-08)

Read-only correctness review of the day's commits (`git log --oneline -40`): `d598254`,
`c2622bf`, `b59c0a5` (OpenSCAD objects as features), `6fd74ea`, `2c0e93e` (`.sldasm`),
`affb6c8`, `c4940e5`, `c6e44cd`, `061bc33` (renderer: stripes, culling, LOD edges, textures),
`6ea721f`, `7bfc80c`, `91b8138`, `9f2cd55`, `fd8e1a3`, `cfb96f9` (app). Nothing was fixed.
"Verified" means a throwaway test outside the repo (scratch crate
`scratchpad/review2/scad/tests/scad.rs`, real OCCT kernel, `Session` + `execute("import.file")`,
compared with `scad_build::build` by volume, centroid and a boolean difference) reproduced it.

| # | Sev. | Finding | Status |
|---|---|---|---|
| R1 | high | **An OpenSCAD object with more than 501 children in one `union`/`difference`/`intersection` aborts the whole import.** `scad_text::check` only parses and evaluates the object's features. It never runs the document validator that `edit()` runs on commit, and that validator refuses a `combine` with more than `MAX_COMBINE_TOOLS` (500) tools. `check` passes, `commit_import` then fails with `invalid_document` and **nothing at all is imported**: not the other objects, not even as exact bodies (the exact route would have worked). A `for` loop over 502 shapes is ordinary OpenSCAD. The same class: the 100 000-features document limit (`text.rs` `MAX_FEATURES`) is hit by the combined text of many large objects (500 objects x 2000 features allowed), and again the parse error in `commit_import` aborts the import. | Verified (combine limit). Repro: file `cube(3); translate([20,0,0]) union(){ for(i=[0:501]) translate([i*2,10,0]) cube(1); }` into an empty document: `import.file` returns `invalid_document: Combine1: 501 tools; at most 500 per combine`. With `i=[0:500]` (500 tools) it imports. The feature-limit variant was derived from the code, not run to completion. |
| R2 | medium | **Features are committed that do not rebuild; only a warning says so.** The per-object `check` evaluates the object in an empty document, but the evaluator's budgets (`gusset-eval/src/budget.rs`: 20 000 tools, 1 000 000 boolean faces, per evaluation, summed over the whole document) accumulate over all features. An import into a document that already used most of the budget (or a file with enough tool-heavy objects, each checked alone) passes every `check`, is committed, and its combines fail with `TooComplex`; their tool bodies stay loose. `commit_import` notices (the "volume ... the exact build's ..." note) but keeps the features and still reports `tier 1: OpenSCAD objects as editable features`, so the file's solid is replaced by 501 loose cubes. | Verified. Repro: 40 copies of the features that `union(){ for(i=[0:500]) translate([i*3,0,0]) cube(1); }` makes (500 tools each, 20 000 in all, evaluates cleanly), then import that file once more: `scad_features:["Cube502"]`, warning `Cube502: volume 1.0000 mm3, the exact build's 501.0000 mm3`, evaluation error `Combine2: the part has more than 20000 pattern instances, holes and tool bodies in all`, 541 bodies where 41 are expected. |
| R3 | medium | **Automatic folder sync outlives the project.** `App::sync_args`, `auto_sync` and `last_sync` are set in `submit_command("history.sync")` and read in `auto_sync_tick`; `replace_host` (project open, new project, close) resets neither. After syncing project A with folder F and ticking "Sync automatically", opening project B makes the tick run `history.sync` for B with A's folder and `who`: B's branches are written to `F/<who>__<branch>.gusb`, replacing A's bundles there, and the others' bundles are imported into B as `from/...` branches. The sync is quiet unless it fails, so nothing tells the user. | Verified by reading: the only writes to the three fields are the constructor, `submit_command` and the menu checkbox (`grep -n "sync_args\|auto_sync\|last_sync" crates/gusset-app/src/app.rs`). Not run in the GUI. |
| R4 | low | A virtual component's embedded part is copied for every instance: on a cache hit `Some(Ok(Some(b.to_vec())))` copies the whole inflated stream, wraps it in a new `Arc`, and only then does the `failed` check run. The per-stream cache therefore saves the inflate (S4) but not an O(size) copy per instance; with the 2000-instance cap and a part of tens of MiB that is gigabytes of copying for a part that fails every time. | By reading (`import.rs` `sldasm`, `embedded_cache`). Not measured. |

## Suggested fixes
- **R1**: have `scad_text::check` run the same validation as the commit (`doc.validate()` /
  whatever `edit` calls) on the scratch document, so an object the document would refuse falls
  back to an exact body with a warning. Better, also make the generator respect the limits: chain
  `combine` in steps of at most 500 tools (`Combine`'s first body is the result of the previous
  one). In `commit_import`, on a parse or validation error of the appended text, retry with all
  objects as exact bodies instead of failing the import. Count the features of all objects
  against `MAX_FEATURES` minus the document's before writing them.
- **R2**: make the post-commit check act: if a written object's body is missing or its volume
  differs, roll the object's features back (remove them and add the exact `Import` feature in
  the same edit), or evaluate the committed document inside `edit` and fall back to exact bodies
  when any of the new features fails. Alternatively give `check` the document's current `Work`
  so the budget it tests is the real one.
- **R3**: clear `sync_args`, `auto_sync` and `last_sync` in `replace_host` (or key them by the
  project's path/id), and say in the menu which folder is synced automatically.
- **R4**: keep the cached `Arc<Vec<u8>>` and clone the `Arc`, test `failed` before touching the
  stream.

## Observation, not counted (unverified)
`sldasm` keys `embedded_cache` by `Arc::as_ptr(&asm_bytes)` plus the stream name. The top
assembly's `Arc` is dropped when its loop iteration ends while its keys stay in the map; an `Arc`
allocated later (a sub-assembly file read in `parts`) can get the same address, and then a
virtual component `VirtualComp/Part1` of the sub-assembly would hit the top assembly's cached
`Part1`. Whether it happens depends on the allocator; it could not be made to happen without a
sample file with virtual components at two levels. Keying by the file name (and depth-free,
since the same file always has the same streams) removes the question.

## Checked and sound
OpenSCAD to features (`scad_text.rs`, `import.rs` `scad()` / `commit_import`), all compared with
the exact build by volume, centroid (within 1e-8 mm; 3e-7 mm for a 600 mm cube turned 1e-5 degrees) and, where the boolean worked, the symmetric
difference; each either became features equal to the exact body or fell back with a warning:
- Placement split: half turns about oblique axes (`[1,1,0]`, `[1,2,3]`, `[1,-1,0.3]`, `[0,1,1]`)
  of centred and off-centre cones, angles 179.9999 and 1e-5 / 1e-7 degrees, `rotate([0,180,0])`
  of a wedge, scale 2 with rotation, negative determinants that cancel (two mirrors, mirror x
  rotate x mirror, `scale([-1,-1,2])` is refused as uneven, as intended).
- Half-turn axis extraction from `(Q + I) / 2` is right for all three diagonal cases.
- Booleans: nested difference inside union, intersection of three, union inside difference,
  disjoint union, touching (face, edge, 1e-9 overlap) unions, a cut that misses the target,
  coincident faces; empty results (cut everything, empty intersection) are skipped by the exact
  build with a warning before features are tried.
- `linear_extrude(center=true)` of polygons with holes: hole touching the outer edge, island in
  a hole, two outlines with holes, clockwise outline, collinear and repeated points; a hole
  that touches the outline at a vertex and a `paths` list with the hole first fall back with a
  warning ("do not rebuild", "land 6 mm away").
- `rotate_extrude`: shapes touching the axis along an edge or at a point, a hole beside the
  axis, a circle touching the axis, angles 45/90/270/360 (the quarter in the right quadrant),
  with a preceding `rotate`, in `in` and `cm` documents too (volumes agree).
- Cylinders with `r1=0` or `r2=0`, centred or not; `r2 = r1 + 1e-10` is a cylinder.
- Tiny and huge sizes: 1e-10 falls back (zero-length line), 1e-5 and 1e-4 mm come in right,
  1e7 and 8e8 mm come in right, 2e9 mm falls back ("not a usable number").
- Names: a document with a parameter `CUBE1`, a sketch `SKETCH1` and a feature `extrude1` gets
  `Cube2`, `Cylinder1`: lower-case clashes are avoided; sketches written are fully defined.
- Rollback handling and the volume/box check order in `commit_import` read correctly; the
  features go through `parse(to_text() + text)`, which keeps the document.
- 1500-point polygons import in 0.7 s (the sketch solver is not a cost).

SolidWorks assemblies: row-vector to column-vector transpose, `compose_xf` order
(`parent o local`), the unit of translations (metres in both levels, applied by
`to_step_placed`), rigid test (orthonormal, det +1, mirrored/scaled counted and reported), the
walk limits (`walked`, `instances`, depth 4, per-file caches for parts and sub-assemblies),
`Folder` (plain names, extension check, listing index), the XML reader (bounded, no panics on
odd UTF-8: `get()` returns `None`), root-model choice, suppressed/hidden skipped before any
file is read. A component's configuration is not applied; that is documented (ADR 0043).

Renderer: `screen_rows` is a superset of what a draw touches (convex box projected; a corner
with w <= 1e-9 gives the whole height; padding covers wide edges; perspective edge pull keeps
screen xy; ortho pull is along z), the stripe test `a < y1 && b > y0` and the per-stripe
`idx`/`miny`/`maxy` clamping agree, so no draw is skipped for a stripe it touches; image does not
depend on the stripe count (same draw order per pixel). Near-plane clipping of triangles
(`z >= 0`) and of edges is consistent with the far check in `plot`; glass is drawn farthest
first by reversing the nearest-first plan, as `gpu.rs` does; `depth` is pushed in step with
`draws`. LOD edges: ends kept, `edge_highlights.is_empty()` required for LOD, GPU syncs only the
copy it draws and looks edges up in the same cache. Textures: Rust, shader and tracer use the
same hash and patterns, `grain_axis` rotation matches the shader's swizzles, the `Draw`
uniform layout (144 bytes) matches `put`, barycentrics in the tracer are the same as for the
smooth normal.

App: workspace switcher (`current` chain and tab highlighting agree; every ribbon command id
of the Assemble and Render groups is registered), photo preview (generation handling: the new
generation is read after `stop()`, so a pass of an older view cannot be shown; hiding bodies
is part of the key; highlights are cleared in the copy), keyed origin rows (`XY` taken from the
key, not the translation), `command_title` fallback, `document.set_text` of identical text.

## Fix status (2026-11-08)

| # | Status |
|---|---|
| R1 | Fixed: a boolean with more tools than one `combine` may take is written as several combines into the same body; an OCCT test imports a 502-child union as features. |
| R2 | Fixed: before committing, the generated features are validated and evaluated together with the document (its limits and the evaluator's document-wide budgets); if they would not fit or would add rebuild errors, every object comes in as an exact body, with a warning. |
| R3 | Fixed: the automatic folder sync is switched off and forgotten when another project is opened. |
| R4 | Fixed: an embedded part found in the cache is shared, not copied. |
