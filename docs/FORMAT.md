# Gusset file formats

This document specifies the binary container of Gusset project files (`.gus`) and the
recovery file (`.grec`), precisely enough to write an independent reader. The design
rationale is in ADR 0005 (container) and ADR 0007 (saving and recovery). The reference
implementation is `crates/gusset-store` (`container` and `recovery` modules).

## Conventions

- All integers are unsigned and **little-endian**. `u8`, `u16`, `u32` and `u64` are 1,
  2, 4 and 8 bytes. Offsets are bytes from the start of the file.
- **CRC32C** is CRC-32/ISCSI (Castagnoli): reflected polynomial `0x82F63B78` (normal
  form `0x1EDC6F41`), initial value `0xFFFFFFFF`, reflected input and output, final XOR
  `0xFFFFFFFF`. Check value: `CRC32C("123456789") = 0xE3069283`.
- **BLAKE3** is the standard 32-byte (256-bit) BLAKE3 hash, unkeyed.
- **zstd** is a Zstandard frame (RFC 8878).

## `.gus` container, format version 1

```
+-----------------+----------+-----+----------+---------+------------------+
| Header (64 B)   | Record 0 | ... | Record n | Index   | Trailer (32 B)   |
+-----------------+----------+-----+----------+---------+------------------+
```

Records are contiguous: record 0 starts at offset 64, each record starts where the
previous one ends, and the index starts where the last record ends (at 64 if there are
no records). Nothing else may appear in the file.

### Header (64 bytes, offset 0)

| Offset | Size | Field | Notes |
|---:|---:|---|---|
| 0 | 10 | magic | `47 55 53 53 45 54 0D 0A 1A 00` (`"GUSSET\r\n\x1a\0"`) |
| 10 | 2 | `format_version` | `1`. Readers reject 0 and versions newer than they support. |
| 12 | 4 | `flags` | `0`. Readers reject unknown flag bits. |
| 16 | 8 | `save_seq` | Save sequence number; increases with every save of the project. |
| 24 | 8 | `created_unix` | Project creation time, seconds since the Unix epoch. |
| 32 | 16 | `app_version` | UTF-8, NUL-padded, of the app that wrote the file. Informational. |
| 48 | 12 | reserved | Written as zero; ignored by readers (covered by the CRC). |
| 60 | 4 | `header_crc` | CRC32C of bytes 0–59. |

### Record

| Offset (relative) | Size | Field | Notes |
|---:|---:|---|---|
| 0 | 4 | magic | `"GREC"` (`47 52 45 43`) |
| 4 | 1 | `kind` | Record kind (table below). `0` is invalid. |
| 5 | 1 | `codec` | `0` = raw, `1` = zstd. Other values are invalid. |
| 6 | 32 | `hash` | BLAKE3 of the **uncompressed** payload. |
| 38 | 4 | `stored_len` | Length of `payload` in the file. |
| 42 | 4 | `raw_len` | Length of the uncompressed payload. |
| 46 | `stored_len` | `payload` | Raw bytes, or one or more zstd frames. |
| 46 + `stored_len` | 4 | `crc` | CRC32C of every byte from `magic` to the end of `payload`. |

Total size: `50 + stored_len` bytes.

Rules:

- codec 0 (raw): `stored_len == raw_len`.
- codec 1 (zstd): `stored_len < raw_len`. Writers only compress when it shrinks the
  payload. Decompressing must produce exactly `raw_len` bytes.
- `hash` must equal BLAKE3 of the decoded payload. Records are content-addressed: a
  file never contains two records with the same hash.
- Compressed bytes are not canonical (they depend on the zstd version and level). Only
  the uncompressed payload and its hash are. Writers copy unchanged records byte for
  byte from the previous file.

Record kinds:

| Value | Kind | Content |
|---:|---|---|
| 1 | Feature | Canonical text (ADR 0004 grammar) |
| 2 | Params | Canonical text |
| 3 | Snapshot | Canonical text |
| 4 | Node | Canonical text (history node, ADR 0006) |
| 5 | Branches | Canonical text |
| 6 | Events | Canonical text |
| 7 | Blob | Opaque binary (imported files, meshes) |
| 8 | ProjectMeta | Canonical text (includes the random project ID) |
| 9 | Settings | Canonical text |
| 10 | Thumbnail | Image, derived and optional |

Snapshot records (kind 3) come in two schemas. `snapshot 1` lists units, the params
hash and every feature hash inline. `snapshot 2` (written since 2026-10-11) has
`units`, `params <hash>` and `list <hash>`. The list object, also kind 3, is
`featurelist-root 1` followed by `chunk <hash>` lines. Each chunk, kind 3 too, is
`featurelist 1` followed by `feature <hash>` lines. A chunk ends after a feature whose
hash's first byte is a multiple of 16, or after 64 features. That way an edit changes
only its own chunk and the root, and a parameter edit reuses the whole list. Readers
accept both schemas (fixture `v1-project-snapshot2.gus`).

Readers accept unknown non-zero kinds, so newer minor additions stay readable, and apply
the "small object" size limit to them. Every canonical text object starts with its own
`gusset <schema> <version>` line, and schema migrations are per object.

### Index (at `index_offset`)

| Size | Field |
|---:|---|
| 4 | magic `"GIDX"` (`47 49 44 58`) |
| 4 | `record_count` (u32) |
| 41 × `record_count` | entries: `kind u8`, `hash [32]`, `offset u64` (record offset in the file) |
| 4 | `root_count` (u32) |
| variable | roots, `root_count` times: `name_len u16`, `name [name_len]` (UTF-8, 1–255 bytes), `value_len u32`, `value [value_len]` |
| 4 | `index_crc`: CRC32C of every index byte before this field |

Rules:

- Entries appear in file order, and entry *i* describes record *i*. Its `kind` and
  `hash` must match the record's own.
- Root names are strictly ascending by byte value, so they are unique. Roots name the
  entry points of the object graph (for example the branch table's hash). Values are
  opaque bytes; Gusset stores 32-byte record hashes.

### Trailer (last 32 bytes)

| Offset (relative) | Size | Field |
|---:|---:|---|
| 0 | 8 | `index_offset` |
| 8 | 4 | `index_len` (bytes, including `index_crc`) |
| 12 | 8 | `save_seq` (must equal the header's) |
| 20 | 8 | magic `"GSEND\0\0\0"` (`47 53 45 4E 44 00 00 00`) |
| 28 | 4 | `trailer_crc`: CRC32C of trailer bytes 0–27 |

`index_offset ≥ 64` and `index_offset + index_len = file_length − 32`.

### Validation (what "opens cleanly" means)

A reader must reject the file unless all of the following hold:

1. the header magic and CRC match, the version is supported and the flags are known;
2. the trailer magic and CRC match, and `save_seq` equals the header's;
3. the index lies exactly between the records and the trailer, and its CRC matches;
4. every record is where its index entry says, contiguous, with a matching CRC, a valid
   codec and lengths, and a decoded payload of `raw_len` bytes whose BLAKE3 equals
   `hash`;
5. there are no duplicate hashes and no bytes left over.

Together these checksums cover every byte of the file, so any single corrupted byte, a
torn write or a truncation is detected.

### Limits (reference reader defaults)

| Limit | Default |
|---|---|
| File size | 4 GiB |
| Records | 10 000 000 |
| `raw_len` per record, Blob and Thumbnail | 1 GiB |
| `raw_len` per record, other kinds | 16 MiB |
| Total decoded bytes while opening | 16 GiB |
| zstd window (`windowLogMax`) | 2^25 bytes (32 MiB). Writers use zstd levels 1–19, whose windows are at most 2^23 |
| Roots | 4096, each value ≤ 1 MiB |

Decompression is bounded by `raw_len`: a decoder stops after `raw_len + 1` output
bytes, and memory grows with the actual output rather than the declared size.

### Salvage

A damaged file can be salvaged by scanning for `"GREC"` and keeping each candidate that
parses, passes its CRC and decodes to its hash. The scan resumes after each intact
record, or one byte after a failed candidate. The roots can be recovered from the index
if the trailer and index CRCs still verify, or else from any `"GIDX"` candidate whose
CRC verifies.

### Roots written by Gusset

| Root | Value |
|---|---|
| `nodes`, `branches`, `events` | 32-byte hashes of the history text records |
| `meta` | `project 1` text: project ID, `save_seq`, `base_seq`, creation time |
| `settings` | optional, `project-settings 1` text: per-project save-mode overrides |
| `origin` | recovery images only, optional: the project file the image belongs to (UTF-8), so it is never offered for another file |

Readers ignore roots they do not know.

### Frozen fixtures

`tests/fixtures/format/` holds files written by each format/schema version
(`v1-project.gus`, `v1-recovery.grec`, `v1-project-snapshot2.gus`). `gusset-project`'s `format_v1_fixtures_still_open`
opens them and checks their content. Fixtures are never regenerated; every format or
schema bump adds new ones together with its migration.

## `.grec` recovery file, version 1

One per open project, at `<data_dir>/recovery/<project-id>.grec`, on the local disk.

```
Header (16 B) | Slot | [Slot]
```

Header:

| Offset | Size | Field |
|---:|---:|---|
| 0 | 8 | magic `"GUSREC\r\n"` (`47 55 53 52 45 43 0D 0A`) |
| 8 | 2 | version = 1 |
| 10 | 2 | reserved = 0 |
| 12 | 4 | CRC32C of bytes 0–11 |

Slot:

| Offset (relative) | Size | Field |
|---:|---:|---|
| 0 | 4 | magic `"GSLT"` (`47 53 4C 54`) |
| 4 | 8 | `seq` (u64) |
| 12 | 8 | `len` (u64) |
| 20 | 4 | CRC32C of `magic`, `seq`, `len` and `payload`, in that order |
| 24 | `len` | `payload`: a complete `.gus` container image of the project |

A writer always writes a whole new file, atomically. It contains the previous newest
valid slot followed by the new snapshot, whose `seq` is one more than the largest valid
`seq` in the old file. A reader scans for `"GSLT"` (so a damaged first slot cannot hide
the second) and uses the slot with the highest `seq` whose CRC matches and whose payload
opens as a valid container.

## Other files (informational)

- **Lock files** (`<data_dir>/locks/<project-id>.lock`) are text:
  `gusset-lock 1`, then `pid=…`, `instance=…`, `host=…` and `start=…` lines. They are
  held with an exclusive OS lock.
- **Project registry** (`<data_dir>/projects.txt`) is text: a `gusset-projects 1` line,
  then one `<32-hex id>\t<percent-encoded path>` line per project.
- **Cache entries** (`<cache>/{shared|<project-id>}/<blake3 hex>.bin`): magic `"GCE1"`,
  `len u64`, BLAKE3 of the payload `[32]`, then the payload.
- **Temporary files** are `~$<stem>.<16 lowercase hex>.gtmp`, created next to the
  target by the atomic-write path. They never survive a successful save.
