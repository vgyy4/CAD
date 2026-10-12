# ADR 0007: Saving, crash recovery and storage hygiene

- Status: accepted
- Date: 2026-10-10

## Decision

### `durable_write` (crate `gusset-store`, module `durable`): the one audited write path
Used for project saves, auto-saves, recovery snapshots, last-known-good copies and
settings:
1. Create a temp file **in the target's directory** with an unpredictable name and
   create-new (`O_EXCL`) semantics. The name is `~$<stem>.<random>.gtmp`, which OneDrive
   and Office-style sync clients skip, and which our housekeeping recognises. Write all
   bytes.
2. Flush: `File::sync_all`. That maps to `fsync` on Linux, `FlushFileBuffers` on Windows
   and **`F_FULLFSYNC`** on macOS; Rust std uses `F_FULLFSYNC` for `sync_all` on Apple
   targets, which a test asserts.
3. **Verify before replacing:** reopen the temp file by path, read it back, and run the
   caller-supplied validator (for `.gus`, full CRC/hash verification). On failure, delete
   the temp file and return an error. A good file is never replaced by an unverified one.
4. Replace atomically:
   - Unix: `rename(2)`, then `fsync` the parent directory.
   - Windows: `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`, in a
     small `unsafe` FFI module in `gusset-store` that is listed as an FFI crate.
5. Return only after the directory sync. A save counts as "acknowledged" only then.

### Last-known-good (LKG) generation
Before step 4 of a project save, if the existing project file verifies, it is copied
with `durable_write` to `<data_dir>/lkg/<project-id>.gus` (exactly one per project,
owner-only permissions). The LKG is deleted together with the project.

### Recovery file: one per open project, two slots
- Location: `<data_dir>/recovery/<project-id>.grec` on the local disk. The project ID is
  a random ID stored in the project file, so recovery data stays local even when the
  project sits on a network or cloud-sync folder.
- Layout: `header | slot A | slot B`. Each slot holds `seq u64 | len | crc32c | payload`,
  and the payload is a full `.gus` image of the in-memory project (history included).
  Every write creates a new file through `durable_write` containing **the previous
  newest slot plus the new snapshot**, so the older slot is the one replaced. The loader
  picks the newest slot whose CRC and contents verify. A crash at any point therefore
  leaves the previous snapshot intact, and there is still exactly one file.
- When: only at consistent points. The command layer exposes a `quiescent` signal, and
  snapshots never happen during an import, rebuild, export or open agent transaction;
  they wait for the next consistent state. If an operation runs for more than the
  interval, the last consistent state from before it is snapshotted.
- Manual-save mode: a snapshot every N seconds (default 60) or after M changes (default
  20), whichever comes first. Auto-save mode: the project file itself is saved,
  debounced by 2 s after the last completed operation, and the recovery file is deleted
  after each successful auto-save.
- Large projects: an append-only journal was considered. We start with whole snapshots,
  measure them, and record the result in `PERFORMANCE.md`; the format leaves room for a
  journal slot kind.
- A clean close (saved, or "don't save") deletes the recovery file.

### Startup recovery
If `recovery/<id>.grec` is newer than the project's last save (compared by the save
sequence number embedded in each, not by mtimes), the user gets a dialog with both
timestamps: *Recover unsaved work* / *Open last saved version* / *Keep both*. The dialog
includes a parameter/feature diff (using the iteration compare engine) and a thumbnail
comparison. Afterwards the recovery file is removed, or, for "Keep both", written out as
`<name> (recovered).gus` first.

### On open: damage handling
Validation fails → never crash, never half-load. Explain what is wrong, then offer in
order: (1) the newest valid recovery slot, (2) the LKG generation, (3) salvage of all
intact records into a new project.

### Locking
`<project>.gus` gets a lock file `<data_dir>/locks/<project-id>.lock`, held with an
exclusive OS lock (std `File::try_lock`, stable since Rust 1.89). The file contains the
PID, a random instance ID, the host name and the start time. A lock whose OS lock can be
acquired is stale (the holder crashed) and is taken over. We never delete files whose
lock is held. Locks live in the data dir rather than next to the project, so nothing is
scattered there and network shares are handled.

### Risky storage
On open, the project path is classified as network share, cloud-sync folder (known
OneDrive, Dropbox, iCloud Drive and Google Drive roots or markers) or FAT/exFAT
removable media. The user is warned once per project, and recovery/LKG data is always
kept on the local disk anyway.

### Cache
One cache directory (XDG cache / `%LOCALAPPDATA%\Gusset\cache` / `~/Library/Caches/Gusset`),
owner-only. Content-addressed entries (`<blake3>.bin`) hold tessellations and thumbnails
keyed by input hash. A size cap (default: 2 % of free disk, clamped to 256 MiB–4 GiB)
with LRU eviction. Everything in it is regenerable.

### Startup housekeeping
Removes orphaned `~$*.gtmp` temp files that we created (in our data dirs and in the
directories of recently opened projects), recovery files of projects that no longer
exist, stale locks, and cache entries of deleted projects. It never touches anything
whose lock is held by a running instance.

### Deleting projects from inside the app
After confirmation, the project goes to the OS Trash via the `trash` crate (default), or
is deleted permanently. Its recovery file, LKG, lock, cache entries and thumbnails are
removed in the same operation.

### Power-loss testing
- `gusset-store` has a pluggable `Fs` trait. `FaultFs` simulates a page cache: it drops
  unflushed writes, reorders writes, and tears pages at random points. Thousands of
  seeded iterations assert that the newest *acknowledged* save, or the one before it,
  opens cleanly.
- Process-kill tests spawn a helper binary that saves in a loop, kill it at random
  times, then verify.
- The Linux CI job additionally runs `dm-flakey` / `dm-log-writes` replays (root is
  available on the runner).

## Amendment (2026-10-11): verification cost of saves and snapshots (review M3)
Step 3 of `durable_write` ("verify before replacing") for project saves, LKG copies and
recovery snapshots now runs `container::verify_framing`: header, trailer, index,
layout and the CRC32C of every record (so every byte is checksummed), together with the
existing byte-for-byte comparison of the read-back with the in-memory image. The
BLAKE3 hash of each record is verified once, when the record is first encoded
(`ContainerWriter::add_cached`), and unchanged records are reused from a per-project
`RecordCache` instead of being recompressed. Opening a file (and reading a recovery
slot on open) still performs the full verification. Measurements: `docs/PERFORMANCE.md`.

## Amendment (2026-10-12): recovery writes off the model thread (review M3)
A recovery snapshot is encoded on the model thread (a consistent point) and written by
another thread. Writes are ordered by a per-project generation counter: preparing a
snapshot, saving, discarding the recovery file, closing and releasing the project bump
it under a lock that writers hold while writing, and a writer whose generation is no
longer current skips its write. The recovery file format and the write procedure
(two slots, `durable_write`, read-back) are unchanged.

## Amendment (2026-10-11): identity, untitled recovery, save modes (reviews H1, H2, M1, M2)
- Save As to a new path gives the project a new ID; opening a copy (same ID, the
  registry or the recovery copy names another existing file) re-identifies it and
  writes the new ID into it. Recovery images record their file (`origin` root) and are
  never offered for another file.
- Housekeeping keeps recovery and LKG copies that no registered project owns (never
  saved projects); `gusset_project::startup` lists them for "Recover unsaved work".
- `gusset_project::settings` holds the save modes and timings above (per user, with
  per-project overrides in the file) and a `SaveScheduler` the front ends drive.
- Deleted-branch retention and the history size limits are applied when a project is
  opened; the headless MCP bridge refuses projects with unresolved recovery.
