# ADR 0005: Native project file format — a custom checksummed chunk container (`.gus`)

- Status: accepted
- Date: 2026-10-10
- Research: `docs/research/mcp-simd-storage-crates.md` §3

## Requirements
- A single file per project. Crash safety; small on disk; fast open and save.
- Clean deletion: no sidecar files left after close.
- Portable, with long-term format stability and versioned forward migrations.
- **Every chunk/record carries a checksum and the header carries a sequence number**, so
  a torn file is always detected and intact parts can be salvaged.
- Hostile files must not be able to hurt the app (parser fuzzed, limits enforced).
- A deterministic readable text form (provided by the code-view text, ADR 0004).
- The history store has to hold content-addressed objects (ADR 0006).

## Options evaluated
| Criterion | SQLite (rusqlite, `VACUUM INTO` + rename) | redb | **Custom chunk container** |
|---|---|---|---|
| Crash safety of the user's file | good, if every save goes through `VACUUM INTO` + our own fsync + rename | good (copy-on-write) | good (whole-file temp + verify + rename) |
| Per-record checksums | no; page checksums need the `cksumvfs` extension, which is not in the bundled amalgamation. Row-level CRCs are possible, but internal B-tree pages stay unchecked | page checksums, yes | **yes, on every record, plus a header and an index checksum** |
| Sidecars | `-journal`/`-wal`/`-shm` during transactions; must be managed so none survive | none | none (only the transient atomic-write temp file) |
| Save cost | `VACUUM INTO` rewrites the whole DB | in-place | rewrites the whole file, copying unchanged compressed records verbatim |
| Format stability | excellent (SQLite is supported to 2050) | dropped its v2 format in 3.0, plus a data-loss fix in 4.0 | ours to keep stable; spec published in `docs/FORMAT.md` |
| Hostile-file attack surface | a large C engine; SQLite documents extra hardening needed for untrusted databases | moderate | **small, pure-Rust parser, fuzzed** |
| Salvage of a damaged file | `.recover` is possible but complex | limited | scan for record magics and verify CRCs to recover every intact object |
| Dependency | C (bundled) | pure Rust | none beyond zstd/blake3/crc |

SQLite is excellent, but the spec's specific demands tip the balance. Per-record
checksums, salvage, a small hostile-input surface and whole-file atomic replace (which
SQLite would also need, via `VACUUM INTO`) are all natural in a simple append-structured
container. Its main advantage, random-access in-place updates, doesn't apply: the
parametric data is small, and we rewrite the whole file atomically anyway.

## Decision
`.gus` is a custom container:
```
[Header 64 B] [Record]* [Index] [Trailer 32 B]
Header : magic "GUSSET\r\n\x1a\0" (10 B) | format_version u16 | flags u32 |
         save_seq u64 | created_unix u64 | app_version [16 B] | reserved | header_crc32c u32
Record : magic "GREC" | kind u8 | codec u8 (0=raw,1=zstd) | hash [32 B, BLAKE3 of the
         uncompressed payload] | stored_len u32 | raw_len u32 | payload | crc32c u32
         (over everything from the magic to the end of the payload)
Index  : record count + (kind, hash, offset) entries | roots (branches, events, settings,
         as record hashes) | crc32c
Trailer: index_offset u64 | index_len u32 | save_seq u64 | magic "GSEND\0\0\0" | crc32c
```
- All integers are little-endian. Records are content-addressed: a record with the same
  hash is stored once, no matter how many branches reference it.
- Record kinds: `Feature`, `Params`, `Snapshot`, `Node`, `Branches`, `Events`, `Blob`,
  `ProjectMeta`, `Settings`, `Thumbnail` (derived, optional). Objects are canonical text
  (ADR 0004 grammar) except `Blob` and `Thumbnail`.
- **Opening** checks the header CRC, the trailer, the index CRC and *every* record's CRC
  and hash (CRC32C runs at GB/s, so this is cheap). Any mismatch means a damaged file,
  which starts the recovery flow (ADR 0007). Limits: max file size (configurable,
  default 4 GiB), max records (10 M), max decompressed size per record (1 GiB for
  blobs, 16 MiB otherwise), and a cap on the zstd window. Decompression is bounded by
  `raw_len`.
- **Salvage** scans for `GREC` magics, keeps records whose CRC and hash verify, and
  rebuilds the reachable graph from them.
- **Saving** writes a complete new file through `durable_write` (ADR 0007). Unchanged
  records are copied byte-for-byte from the old file, without recompression.
- **Versioning:** `format_version` covers the container. Each canonical text object
  starts with its own `gusset <schema>` line, and loading runs the migration chain
  `vN → vN+1` per object. Old files always open; every schema bump adds a migration test
  with a frozen fixture file in `tests/fixtures/format/`.
- **Text export:** "Export as text" writes the current branch (or, optionally, every
  branch) as deterministic `.gus.txt`, the code-view language, for users who want to
  version projects in their own tools.
- Crates: `zstd` (BSD-3, libzstd), `blake3` (CC0/Apache-2.0) and `crc32c`, all verified
  in the research report. If `crc32c` stays unmaintained, CRC32C is about 30 lines of
  safe table-driven code that we can own.

## Consequences
- Each save costs one full sequential write. That is fine for parametric data; for
  projects with large imported blobs the cost is measured and budgeted in
  `PERFORMANCE.md`. Possible mitigation: keep blobs in an append-only section copied by
  `copy_file_range`.
- We own a file format. `docs/FORMAT.md` documents it fully, so others can read our files.
