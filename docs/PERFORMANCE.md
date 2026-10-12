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
| Binary size (app + OCCT libs, compressed) | < 60 MB | not yet measured | release workflow |
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
