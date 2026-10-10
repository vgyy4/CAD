# gusset-store

Durable storage for Gusset: the `.gus` project container, crash-safe saves, recovery
files, project locks, the cache directory and startup housekeeping. Design: ADR 0005
(container) and ADR 0007 (saving, recovery, hygiene). Byte layouts: `docs/FORMAT.md`.

| Module | What it does |
|---|---|
| `container` | `ContainerWriter` (content-addressed, BLAKE3-deduplicated records; zstd with raw fallback; byte-for-byte copy-through of unchanged records; named roots), `open`/`open_with` (validates header, trailer, index and **every** record's CRC and hash under `Limits`; lazy `get`), `verify`, `salvage`. Never panics; errors say what is damaged and where. |
| `durable` | `durable_write(fs, path, bytes, validate)`: the one audited write path. It writes a `~$<stem>.<random>.gtmp` temp file (create-new), runs `sync_all`, reads it back and validates it, replaces the target atomically (`rename` or `MoveFileExW` with write-through) and then fsyncs the directory. `save_project` adds the last-known-good copy. |
| `fs` | The `Fs` trait, `RealFs`, and `FaultFs`, a seeded page-cache simulator that drops, reorders and tears (512-byte sectors) unflushed writes, and loses unsynced directory entries, at a chosen crash point. |
| `recovery` | The two-slot recovery file: `write_snapshot`, `read_newest`, `remove`. |
| `lock` | `ProjectLock` uses an OS exclusive lock (`File::try_lock`) and writes PID, instance ID, host and start time into the file. It detects and takes over stale locks, and `probe` checks a lock without taking it. |
| `paths` | `DataDirs` (per-user data and cache dirs via `directories`), `ProjectId`, owner-only permissions, and `classify` (network share, cloud-sync folder, FAT/exFAT). |
| `cache` | `Cache` is a content-addressed (`<hash>.bin`), size-capped, LRU (mtime) cache with shared and per-project scopes. It is safe for concurrent instances: the worst case is a miss. |
| `housekeeping` | Project `Registry`, `run` (removes orphaned temps, stale locks, and recovery/LKG/cache data of deleted projects; never touches a locked project) and `delete_project` (trash or permanent delete, plus all app data). |

## Safety notes

- This crate is a listed FFI crate. `#![deny(unsafe_code)]` applies at the root, and
  only the `cfg(windows)` `win` module allows `unsafe`, for `MoveFileExW`,
  `GetFileInformationByHandle` and the volume queries. Every block has a `// SAFETY:`
  comment.
- Temp files are created with `O_EXCL` semantics, so a planted symlink is never
  followed. Housekeeping and cache code use `symlink_metadata`, and they refuse
  symlinked lock files and app directories.
- On Unix, data directories are `0700` and their files `0600`.

## Testing

`cargo test -p gusset-store` runs:

- unit tests;
- container tests: round trip, dedupe, copy-through, flipping every byte (three
  masks), truncation at every offset, hand-crafted hostile files, decompression bombs,
  window cap, and 3000 random mutations;
- power-loss simulations: 4000 seeded `FaultFs` crashes across every operation of
  four project saves with LKG, and 3000 across five recovery snapshots. Two control
  tests check that `FaultFs` catches a missing file fsync and a missing directory
  fsync;
- lock, cache, housekeeping and deletion tests, and a 40-session hygiene simulation.

Fuzzing (nightly plus `cargo-fuzz`; the targets live in the repository-level `fuzz/`
crate, see `fuzz/README.md`):

```sh
cd fuzz
cargo +nightly fuzz run container_open
cargo +nightly fuzz run container_salvage
```
