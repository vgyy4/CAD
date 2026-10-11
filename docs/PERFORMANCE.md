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
| Rebuild, 100-feature part | < 1 s for a one-feature parameter change (incremental); < 5 s full | not yet measured | criterion `rebuild_100` |
| Binary size (app + OCCT libs, compressed) | < 60 MB | not yet measured | release workflow |
| History overhead | ≤ 15 % of the model's parametric size per 100 single-parameter edits | not yet measured | criterion `history_overhead` |
| Project save (parametric only) | < 50 ms for 1,000 features | 60 ms (was 166 ms) | `gusset-project` `perf_save_and_snapshot` (see log) |
| Recovery snapshot | < 100 ms for 1,000 features; never blocks the UI | 52 ms (was 168 ms); 0.65 s with a 64 MiB blob (was 2.8 s) | `gusset-project` `perf_save_and_snapshot` (see log) |

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
