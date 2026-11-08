# Security review: code added 2026-11-08

Read-only review of the code added on 2026-11-08 that parses untrusted input or touches files.
Scope: `.sldprt` (`gusset-sldprt`, the `Kind::Sldprt` import, the `gusset-xt` embedded schema),
history bundles and folder sync (`gusset-history/src/bundle.rs`, `commands/bundle.rs`), Fusion
`.f3d`/`.f3z` (`gusset-sat/src/fusion.rs`, the Fusion part of `import.rs`, zstd zip entries),
crash reports, the 3D mouse socket, `feature.bake`, the path tracer and `sim.optimize`.
"Verified" means a throwaway program outside the repo, built against the crates' public API,
reproduced the finding. Nothing was fixed in this review.

| # | Sev. | Finding | Status |
|---|---|---|---|
| S1 | high | `.sldprt` OLE2: many directory entries can share one sector chain. Every `Config-<n>-Partition` stream is read and kept (up to 8191 of them), with no total budget: `lib.rs:144-150`, `cfb.rs:166`. `partition_block` then runs on each, with a 512 MiB inflate budget per call rather than one shared budget (`lib.rs:108-119`). In-process and not cancellable. | Open. Verified: a 1.1 MiB file reached 517 MiB RSS, and a 1.4 MiB file reached 3.1 GiB. A 1.6 MiB file needs about 8 GiB. |
| S2 | high | Fusion: `design_info` (`import.rs:518-530`) merges the bodies of *every* `*/BulkStream.dat` entry (when none is `[Active]`). Each `read_design` gives up to 10 000 `Placed` (`fusion.rs:604`). A zip may hold 10 000 such entries: about 100 M placements, or about 20 GB. | Open. Verified per stream: a 58 KiB stream gives 10 000 placements (about 200 B each). The total is extrapolated. |
| S3 | medium | `read_design` is quadratic: for every `BREP.` reference it scans every occurrence (`fusion.rs:589-610`). There are also `blocks.iter().any` per reference and `out.bodies.iter().any` per block (`620-627`). A 256 MiB stream is allowed. In-process, with no cancel flag. | Open. Verified: a 4 MiB stream took 12 s and an 8 MiB stream 52 s (optimised build). 256 MiB would take hours. |
| S4 | medium | Fusion instances: every (body, occurrence, reference instance) is converted (SAT to STEP, in-process) and imported by the worker separately. The output keeps up to 4096 blobs with no byte cap (`import.rs:610-660`). A small `.f3d` with one large body and 4096 instances gives 4096 worker runs and 4096 copies of the blob in app memory. | Open. Not verified (needs OCCT). |
| S5 | medium | `history.sync` checks whether its own bundle changed with `std::fs::read(&path)` (`commands/bundle.rs:148`). That call follows symlinks, blocks on FIFOs and has no size cap. A collaborator who replaces `alice__main.gusb` with a FIFO hangs Alice's model thread for good. A symlink to `/dev/zero` or a huge file exhausts her memory. (The write itself is safe: `durable_write` creates a random-name temp file and renames it over the link.) | Open. Verified by reading. |
| S6 | medium | Bundle import parses the whole document for *every node*, not once per distinct snapshot (`bundle.rs:296-300`). A bundle with 100 000 nodes on one large snapshot compresses to almost nothing. It runs on the model thread and cannot be cancelled. A sync imports up to 256 such files. | Open. Verified: a 31 KiB bundle (1000 nodes, one 20 000-parameter snapshot) took 12 s. 100 000 nodes take about 20 min, and larger documents take hours. |
| S7 | medium | Bundles and sync have no cumulative byte limit. One bundle may hold 1 GiB of objects (blobs up to 256 MiB, all zeros compress to KiB), and they stay in the history. The import peaks at about 3 GiB (`read_to_end` growth plus the `staged` copy). A sync imports up to 256 bundles per run. A few small files planted in the shared folder exhaust the memory of everyone who syncs, and bloat their `.gus`. | Open. Verified by reading. |
| S8 | medium | `sim.optimize` lets an agent overwrite an existing `.stl` in the project folder with `overwrite: true` and no approval (`commands/sim.rs:677-686`). Every other export asks for approval (`CommandError::approval("overwriting an exported file")`). `WRITE_FILE` alone does not ask. | Open. Verified by reading. |
| S9 | low | `sim.optimize` filter neighbourhoods: `min_member` up to 16 voxels gives radius 8, about 2100 neighbours per element. With 200 000 elements the neighbour lists take about 6.8 GB (`(usize, f64)` = 16 B), plus about 1e9 binary searches to build them (`gusset-fea/src/topo.rs:115-140`). The command is agent-reachable and cannot be cancelled. | Open. Not verified (arithmetic only). |
| S10 | low | Photo renders: `MAX_PATHS` bounds the number of paths, not the work. Each ray loops over every mesh and inverts its transform (`pick.rs:83-92`). MCP `render_view` with style photo allows 2048² × 16 paths on all cores, with no cancel or timeout, and does not take the renderer lock. A model with thousands of bodies makes one agent call take hours of CPU. | Open. Verified by reading. |
| S11 | low | Sync has no authentication, but the code does not say so: (a) branch names come from the bundle's contents, so Bob can publish `from/alice/main`. (b) Imported nodes keep the bundle's actor and time, so a bundle can claim `User` or any agent name in history and the agents panel. (c) `sort` + `truncate(256)` lets a planted `0000*.gusb` crowd out the real bundles. (d) Every own branch is shared automatically, including agent working branches. (e) Different branch names map to the same file (`a b` / `a_b`, non-ASCII names). | Open. |
| S12 | low | `import_bundle` calls `room()`, which may compact (squash) the user's history *before* `fits()` refuses an over-large bundle (`bundle.rs:349`, `history.rs:588`). The documented "nothing changes when the bundle is refused" does not hold, and an automatic sync can trigger the compaction. | Open. Verified by reading. |
| S13 | low | Crash reports only replace `$HOME` with `~` (`crash.rs:21`). ADR 0008 §10 promises username removal and token redaction through a shared scrubber, but no such scrubber exists in the code. Paths outside the home folder (`/mnt/c/Users/alice`, `/Volumes/...`, `D:\alice\...`), Windows case and slash variants, and model names in panic messages remain. Mitigated: nothing is sent, and the report is shown word for word. | Open. |
| S14 | low | Trust boundary not documented: `.sldprt`, XT, SAT and the Fusion design-stream parsers run in the app process. So does the zstd *C* decoder (zip method 93, bundles), although ADR 0008 puts memory-unsafe parsers in the worker. THREAT_MODEL B1/B2 list none of these. There are no fuzz targets for `fusion::read_design` or `import_bundle`. | Open. |
| S15 | low | 3D mouse: an unbounded `mpsc` channel. A flood from the socket grows memory while frames are not drained. The socket path is fixed and root-owned (`/var/run/spnav.sock`), so this is informational. | Open. |

## Suggested fixes
- **S1**: sort the partition entries by configuration number and read them lazily, stopping at the first that yields a partition. Dedupe entries by `(start, size)`. Charge every `Cfb::read` and every `partition_block` inflate against one `max_total_bytes` budget per file. Check the cancel flag between streams.
- **S2/S3**: cap placements per design and per import, not per stream. Read only the `[Active]` stream, or the first N streams. Build a `comp -> owns` map and a `HashSet` of placed brep ids instead of the nested scans. Add a work budget and a cancel flag to `read_design`.
- **S4**: add an output byte cap like `MAX_FREECAD_BYTES`. Better: convert and import each part once, then place copies by transform, or by instances in the document.
- **S5**: open with `import::open_regular` and compare with `read_capped(bytes.len() + 1)`. If the file is not a regular file or has another size, rewrite it.
- **S6**: call `snapshot_closure` and `document` once per distinct snapshot.
- **S7**: add a cumulative budget per sync (and a total-object-bytes limit in `HistoryLimits`). Lower the decompressed limit for sync, or ask the user before a sync imports more than a set size. Decompress into a buffer sized by the declared payload.
- **S8**: add the same approval check as the other exports.
- **S9**: cap `ne × neighbourhood`, or apply the filter as a grid convolution without stored lists, and add a cancel flag.
- **S10**: add a work budget of paths × meshes or a top-level BVH, a lower photo limit for MCP, a timeout or cancel, and serialise photo renders.
- **S11**: name branches from the file's `<who>` prefix. Record the importer as the actor (keep the original in the label). Cap files per `who`. Let the user choose which branches to share. Document that the folder is not authenticated.
- **S12**: check `fits()` without compacting for imports, and refuse first.
- **S13**: also redact the user name (case-insensitively on Windows, both slash forms) and token-shaped strings, or build the shared scrubber ADR 0008 describes.
- **S14**: add rows to THREAT_MODEL B1/B2. Consider `ruzstd` (pure Rust) for untrusted zstd. Add fuzz targets for `read_design` and `import_bundle`.
- **S15**: use a bounded `sync_channel` that drops motion events when full.

## Checked and sound
- `cfb.rs`: the header is checked (signature, sector shifts), FAT and DIFAT counts are bounded by the file size, and every offset is computed with checked arithmetic. Chains stop on loops (step bound = FAT length) and are capped. Stream sizes are capped, and a short chain is an error.
- `chunks.rs`: markers are scanned linearly with a candidate cap. Name lengths are capped. Records are inflated only when wanted, with a ratio check, a cumulative inflate budget and a CRC check.
- `partition_block` slicing cannot overflow. The `gusset-xt` change is a static schema table.
- zip method 93: `zstd::bulk::decompress` is capped at the declared size. The declared size is at most `max_entry` and the budget, and passes the ratio check. Single-shot decoding allocates no window. zstd is built without default features (no legacy formats).
- Fusion references are resolved only inside the archive (`relativePath` is compared with entry names and never opened), so there is no path traversal. The JSON files are capped at 4 MiB and serde_json's recursion limit applies, and every list is `take`-capped. Reference chains: depth 8, each design is placed once, and placements are capped. Bodies still reach the app as worker output from STEP.
- Bundle format: magic, version, BLAKE3 of the payload, a zstd `window_log_max` of 25, a decompressed size cap, and per-field, count and object caps. Every object is re-hashed. The closure must be complete and nothing extra may be present. Parents come first (no cycles), id 0 is reserved, `main` is never written, names are free and checked, and control characters are stripped.
- Imported bundle blobs are not marked trusted: evaluation still sends them to the worker.
- `history.import_bundle`: agents are confined by `read_target`. `open_regular` uses `O_NOFOLLOW|O_NONBLOCK` and checks for a regular file on the handle. Reading is capped and cancellable.
- `history.sync` is user-only. `who` is validated, remote paths are refused, writes go through `durable_write` (random temp name, create-new, rename), and incoming files are opened with `open_regular`.
- `feature.bake`: blobs come from `write_native` of evaluated shapes, which ADR 0008's trust set already covers. It needs `confirm` and can be undone.
- Crash reports are local and never sent. At most 5 are kept, in the data folder, and each is shown up to 20 000 characters.
- 3D mouse: fixed 32-byte packets, saturating sums, and `Camera::zoom` rejects non-finite factors.
- Path tracer: size, samples, bounces and the path count are validated, and the thread count is capped at 64.
- `sim.optimize`: iterations at most 200, the keep share and loads are validated, at most 200 000 elements, a solver work budget, and smoothing at most 50 passes.

## Fix status (2026-11-08)

| # | Status |
|---|---|
| S1 | Fixed: configurations are listed without reading, deduplicated, read lowest first until a partition is found, against one byte budget for the whole file (streams and inflated blocks). Test: 40 configurations exceed a 16 KiB budget; the first found without reading the rest. |
| S2 | Fixed: at most 256 design streams and 200 000 placements per import, refused past either. |
| S3 | Fixed: `read_design` is linear (indexes built once), stops at its cap, and takes the cancel flag (`read_design_cancellable`). An 8 MiB stream reads in well under 5 s. |
| S4 | Partly fixed: at most 1 GiB of imported shapes per import. Each instance is still converted and imported separately; converting a part once and placing copies remains open. |
| S5 | Fixed: `history.sync` reads its own old file only through `open_regular` + `read_capped` (no links, pipes or devices, bounded); anything else is replaced by write + rename. Test: a planted link to /dev/zero is replaced. |
| S6 | Fixed: each snapshot is checked once, however many iterations share it. |
| S7 | Fixed: bundle defaults 128 MiB file / 256 MiB unpacked; one sync reads at most 256 MiB of bundles (the rest are skipped and reported). |
| S8 | Fixed: `sim.optimize` asks agents for approval before overwriting, as every export does. |
| S9 | Fixed: the filter refuses more than 60 M neighbour entries (voxels × (2r+1)³). |
| S10 | Accepted: photo renders are bounded by size, samples and bounces; cancelling them is left for the render job queue. |
| S11 | Accepted and documented (THREAT_MODEL.md): sync names are not authenticated; merging stays explicit. |
| S12 | Fixed: an import that does not fit is refused without compacting history. |
| S13 | Fixed: crash reports replace the user name (USER/USERNAME/LOGNAME) as well as the home folder. |
| S14 | Fixed: THREAT_MODEL.md lists the in-app parsers (sldprt, XT, SAT/Fusion, zstd, bundles, the shared folder). Fuzz target `bundle_import` added (and `sldprt_read` now runs in CI); one for `read_design` remains. |
| S15 | Accepted: the socket is root-owned and local. |

## Follow-up review (later on 2026-11-08, commits after 8dd5109)

Scope: the SolidWorks `LocalBodies` reader, the end-of-file checks, Parasolid names and
colours, the `appearance` statement, worker request 34 (`OffsetFacesOnly`), glass in the path
tracer, unit strings in CAM lengths and the larger `get_text` pages. Read, not run.

| ID | Sev | Finding | Status |
|---|---|---|---|
| T1 | medium | Up to 4096 `LocalBodies` transmits were all converted to STEP and held before any was imported; warnings were cut only at the end; cancel was not seen inside the block reader. | Fixed: one body at a time, its STEP dropped before the next, 1 GiB of STEP for all of them; warnings capped as added; cancel checked per block. |
| T2 | low | The 4096 cap counted only `PS` blocks; the chunk reader inflates every `LocalBodies` record before the first is used. | Fixed: every block counts; the record is chosen by name before anything is inflated. |
| T3 | info | Only one feature's `LocalBodies` is read, chosen differently per container. | Accepted (correctness, not security); noted in ADR 0043. |
| T4 | medium | The colour count per body was quadratic in distinct colours. | Fixed: a map; ties go to the lowest colour value. |
| T5 | low | `\X2\` escapes decoded after the control filter could put control, bidi or zero-width characters into labels. | Fixed: decoded first, then such characters removed and the name cut to 200. |
| T6 | medium | Imports and `body.appearance` could add more appearances than the parser accepts, so a saved document would not read back. | Fixed: both stop at `MAX_LIST_ITEMS`. |
| T7 | info | Appearance parsing, duplicates, compare and merge are bounded. | No change. |
| T8 | low | The glass shadow loop is bounded (16); NaN paths clamp to a speck. | No change. |
| T9 | low | `opt_mm` lets no NaN or infinity through; the CAM checks bound the rest. | No change. |
| T10 | info | `has_end_record` and `cut_short` are cheap and advisory. | No change. |
| T11 | info | Request 34 reuses the bounded readers. | No change. |
| T12 | info | 64k `get_text` pages stay clamped and wrapped as untrusted. | No change. |
