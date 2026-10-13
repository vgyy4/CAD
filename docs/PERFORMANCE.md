# Performance budgets

Budgets are enforced, not aspirational. Baselines are measured as features land; until
then the spec's starting targets apply. **Reference machine:** a 4-core laptop with an
Intel HD 620-class iGPU and 8 GB RAM (spec). CI measures what it can on GitHub runners,
under the software renderer for frame times.

| Metric | Budget (starting target) | Measured | How measured |
|---|---|---|---|
| Cold start to interactive window | < 1.5 s | not yet measured | `gusset --bench-startup` |
| Idle RAM, empty document | < 250 MB (incl. GPU memory on iGPU) | not yet measured | |
| Idle CPU | ~0 % (no continuous redraw) | not yet measured | automated idle test (no frames requested over 5 s) |
| Orbit frame time, typical part | 60 fps (16.7 ms) | not yet measured | scripted scenario |
| Large assembly | ≥ 30 fps | not yet measured | |
| Software renderer | ≥ 15 fps | not yet measured | CI (llvmpipe) |
| Rebuild, 100-feature part | < 1 s for a one-feature parameter change (incremental); < 5 s full | 0.12 s incremental, 3.0 s full (OCCT, plate + 49 bosses, dev build) | `gusset-cli` test `rebuild_100_budget` (OCCT CI job) |
| Binary size (app + OCCT libs, compressed) | < 60 MB | 26–41 MB per package (v0.1.0-mvp: MSI 26–28, zip 31–32, dmg 35, AppImage 36–38, tar.gz 39–41, deb 26–30) | release workflow |
| History overhead | ≤ 450 bytes per single-parameter edit, independent of model size (revised from "≤ 15 % of the parametric size per 100 edits", see below) | ~370 bytes per edit (29 % of a 1,000-feature model per 100 edits; was ~7.5 KB per edit with 200 features) | `gusset-project` test `history_overhead_budget` |
| Project save (parametric only) | < 50 ms for 1,000 features | 7 ms (was 166 ms) | `gusset-project` `perf_save_and_snapshot` (see log) |
| Recovery snapshot | < 100 ms for 1,000 features; never blocks the UI | 6.5 ms (was 168 ms), of which 3 ms on the model thread; with a 64 MiB blob 37 ms on the model thread, the 0.5 s write on its own thread (was 2.8 s, all blocking) | `gusset-project` `perf_save_and_snapshot` (see log) |

## Rules
- A CI benchmark regression of more than 10 % fails the build unless the commit
  message justifies it.
- Profile before optimizing (cargo flamegraph, tracy, heaptrack). Findings go below.

## Profiling log

### 2026-10-11: saves and recovery snapshots (review M3)
Benchmark: `cargo test --release -p gusset-project perf_save_and_snapshot -- --ignored
--nocapture`. A 1,000-feature document with 100 parameter edits in its history (3.8 MB
`.gus`), with and without an extra 64 MiB blob (47 MB `.gus`). Each figure is the best
of three "commit one edit, then snapshot/save" runs, wall-clock, on the CI-like
development container (4 vCPU, virtual disk; fsync included).

| | Snapshot before | Snapshot after | Save before | Save after |
|---|---|---|---|---|
| 1,000 features | 168 ms | 52 ms | 166 ms | 60 ms |
| + 64 MiB blob | 2.78 s | 0.65 s | 2.07 s | 0.61 s |

What changed:
- Every save and snapshot re-hashed and re-compressed every history object, then fully
  decoded the image 3–5 times (pre-write check, LKG check, two read-backs, recovery's
  previous slot). Now each `Project` keeps a `RecordCache` of encoded records (primed
  from the file when it is opened), so unchanged objects are copied byte-for-byte, as
  ADR 0005 intended. A fresh record is decoded back and hash-checked once, when created.
- Write-side validation uses `container::verify_framing` (header, index, layout and the
  CRC32C of every record: every byte stays covered) plus the byte-for-byte read-back
  comparison; full BLAKE3 verification remains on every open (ADR 0007 amendment).
- What remains is I/O: the recovery file holds two full images, and a save writes the
  LKG copy and the project file, each with fsync and read-back. Next steps if needed:
  the append-only journal slot ADR 0007 leaves room for, a chunked snapshot vector
  (the 100 snapshot objects of 73 KB each dominate the parametric file), and running
  the encode off the model thread.

### 2026-10-12: recovery writes off the model thread (review M3)
Same benchmark, after snapshot format 2. 1,000 features: snapshot 6.5 ms, save 7.0 ms,
encoding alone 3.1 ms. With the 64 MiB blob: snapshot 545 ms, save 496 ms, encoding
alone 37 ms, so the disk write (two slots, fsync, read-back) is over 90 % of the cost.
The app now only encodes on the model thread (`Project::prepare_snapshot`); the write
runs on a short-lived thread (`SnapshotWrite::write`). A shared generation counter
keeps the writes ordered: a later snapshot, a save, a discard or closing the project
supersedes pending writes (a superseded write is skipped; one in progress is waited
for), so a late writer never overwrites newer state or brings back an obsolete
recovery file. Saves still write on the model thread (explicit saves and auto-save
mode); with huge imports that is ~0.5 s.

### 2026-10-12: evaluator state per feature (review L8)
`cargo test --release -p gusset-eval perf_many_features -- --ignored --nocapture`:
n one-point sketches, MockKernel. Each recomputed feature clones the previous state
(states are cached per step), and that clone copied every status and solved sketch
so far, string by string: quadratic. Statuses are now a list with a shared tail,
solved sketches a shared base plus up to 32 recent entries, and rolled-back features
no longer clone the state at all.

| | Full, before | Full, after | Rolled back, before | Rolled back, after |
|---|---|---|---|---|
| 1,000 features | 198 ms | 9.8 ms | 49 ms | 0.25 ms |
| 4,000 features | 6.16 s | 63 ms | 581 ms | 0.9 ms |

### 2026-10-13: many-tool booleans and evaluation limits (review H3, security review V1, V2)
Benchmark: `cargo test --release -p gusset-cli --test perf_booleans -- --include-ignored
--nocapture --test-threads 1` (OCCT, release build, 4 vCPU container; wall clock of one
evaluation without cache). Holes: a 5 mm plate with an n x n grid of 3 mm through holes
from one sketch. Pattern: disjoint copies of a 10 mm cube. Combine: one cut of a bar with
n pin bodies.

| Document | Before | After |
|---|---|---|
| 25 holes | 0.47 s | 0.08 s |
| 100 holes | 6.25 s | 0.34 s |
| 400 holes | 109.7 s | 1.98 s |
| 961 holes | (about 10 min) | 6.08 s |
| `pattern_linear` count 100 | 9.94 s | 0.52 s |
| `pattern_linear` count 500 | 345.5 s | 2.35 s |
| `combine` cut, 10 tools | 0.16 s | 0.04 s |
| `combine` cut, 50 tools | 1.49 s | 0.17 s |
| 30 chained count-2 patterns | never finishes (x2.1 per line) | 8.5 s, fails at line 11 (`too_complex`) |

What changed (ADR 0020):
- Holes, body patterns, mirror with merge and `combine` (union, cut) build all tools first and
  run one OCCT boolean with the tool list (`Kernel::boolean_many`) instead of one boolean per
  tool on the growing body, then name the result once. Face names are unchanged (naming
  goldens). `SetRunParallel` stays off: model-defining operations must be deterministic, and
  the speedup comes from the single operation anyway.
- The hole fit check builds the face's triangles and boundary once per feature instead of
  once per hole.
- Corner names of a body format each edge name once (they were formatted twice per
  comparison, quadratic in the name depth of patterns of patterns).
- Work limits stop what is still too big: a feature may put at most 10,000 faces into its
  boolean, an evaluation at most 1,000,000 in all and 20,000 tools. The mock-kernel test
  `chained_doubling_patterns_hit_the_budget_quickly` (40 chained lines) stops in about
  0.3 s (release) / 0.9 s (debug); `holes_400_budget` (< 15 s) and
  `chained_patterns_stop_at_the_budget` enforce the OCCT side.
- Sketch arrangement (V2): twenty 1,000-point splines 0.0001 mm apart took 106 s (35 s after
  the spatial index alone); with the work limit they fail cleanly after about 1 s (release),
  2.7 s (debug). Six crossing 60-point splines arrange bit-identically to before (test
  `crossing_splines_arrange_as_before`); six parallel 1,000-point splines 1 mm apart take
  0.08 s.

### 2026-10-13: gear models (dogfooding F3-5)
`gusset build` of the dogfooding gear pairs (release, OCCT, 4 vCPU container) and
`cargo test --release -p gusset-cli --test perf_gears -- --ignored --nocapture`.
Release was barely faster than debug, so the time was in OCCT, not in Rust; gdb stack
samples and timers around the shim's entry points showed where:

| | Before | After |
|---|---|---|
| Spur pair (20 + 30 teeth), full rebuild | 8.6 s | 1.4 s |
| Helical pair, full rebuild | 27.1 s | 8.7 s |
| Generating one helical gear (`perf_gears`) | 12-15 s | 5.3-6.7 s |
| Edit on the gear model (`param.set`, `feature.delete` of the last feature) | 11-18 s (debug, dogfooding) | 8-14 ms |

What changed:
- `BRepCheck_Analyzer` took 1-2 s per operation on a gear (the 280-edge cap wires; even
  the topology-only check took over 1 s). Extrusions are now checked for closed shells
  in linear time (every edge bounds two face sides): the prism of a planar face whose
  loops the sketch arrangement already checked is valid by construction. A rigid move,
  rotation or mirror of a valid shape is not re-checked. Every other operation (booleans,
  fillets, shells, revolve, sweep, loft) keeps the full check.
- Every operation extracted the full topology (per-face area integrals, 0.3-0.6 s on a
  gear) just to count faces, and the evaluator then extracted it again. The OCCT shape
  now computes its topology, mass properties and bounding box at most once.
- OCCT's pipe shell spent 40 % of a helical gear's sweep in `BuildHistory`, which was
  quadratic in the profile's edge count. Fixed with our first OCCT patch (ADR 0002
  amendment): helical pair rebuild 24 s -> 15 s, generating one helical gear 12-15 s ->
  8-11 s, same shapes.
- Holes in a swept profile were cut with a boolean (2.6-3.4 s per gear bore). They are
  swept along the same spine, so their ends lie in the outer sweep's end planes: the
  holed solid is now assembled from the side faces and two caps with the holes as inner
  loops, checked for closure and exact volume, with the boolean as the fallback.
  Applies to every sweep with holes.
- Second OCCT patch: `MakeSolid` orients the solid by the sign of its volume instead of
  classifying a point at infinity (1-1.8 s per gear).
- Validity checks (`BRepCheck_Analyzer`) and the per-face/per-edge measures of the
  topology summary now run on all cores; every value is computed into its own slot,
  so results do not depend on the thread count. This speeds up every operation on
  large shapes, scaling with the machine.
- Left per helical gear: the pipe shell itself (~1 s), the validity check (~1 s), the
  topology summary and mass properties (0.3-1.7 s, once per shape).

### 2026-10-13: the kernel in its worker process (ADR 0022)
`perf_gears` in release, same container, in-process kernel vs the kernel worker (shapes
stay in the worker; requests, topology summaries and meshes cross a pipe):

| Step | In process | In the worker |
|---|---|---|
| Spur gear 20 / 30 teeth | 1.64 / 2.06 s | 1.75 / 2.14 s |
| Helical gear 20 / 30 teeth | 5.06 / 7.09 s | 5.39 / 6.43 s |
| `param.set`, `feature.delete`, undo, queries | 2-21 ms | 2-16 ms |

The process boundary costs 0-6 % on heavy operations (within run-to-run noise) and
nothing measurable on edits; a worker starts in ~30 ms (`gusset build` of the bracket:
62 ms in process, 90 ms with the worker).

## History overhead (2026-10-11)

Each history node used to store a snapshot object listing every feature hash, so a
one-parameter edit of a 200-feature part added ~7.5 KB (30× the model size per 100
edits). Snapshot format 2 (docs/FORMAT.md) points to a feature-list root with
content-defined chunks: a parameter edit now shares the whole list and adds only the
node line, the parameter table and a ~170-byte snapshot, about 370 bytes after
container compression. A feature edit adds its feature, one chunk and a new root.

That cost does not depend on model size, so the original budget ("≤ 15 % of the
parametric size per 100 edits") cannot hold for small models: 100 edits of a 25 KB
model add ~37 KB whatever the format, as long as nodes are kept. The budget is
therefore stated per edit (≤ 450 bytes); for a 1,000-feature part it amounts to
~30 % per 100 edits. History pruning (ADR 0006) bounds the total.
