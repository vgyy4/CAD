# ADR 0044: Collaboration by exchanging history (bundles first, live relay later)

Status: accepted (phase 1), 2026-11-08

## Context
"Real-time collaboration" is in the later tier (RESEARCH.md §4). Gusset's history (ADR 0006)
is already shaped for it: features, parameter tables, snapshots and blobs are immutable and
content-addressed (BLAKE3), nodes form a tree, branches are named heads, and full three-way
merge, compare and cherry-pick exist. Agents already work concurrently on their own branches
in one session (ADR 0012). What is missing is moving history between two copies of a project.

Two constraints shape the design:
- Node and branch ids are per-project counters (`NodeId(u64)`): two copies number their nodes
  independently, so ids cannot be shared.
- Projects are local files (ADR 0005); there is no server, and the security model (ADR 0008)
  forbids opening network listeners without the owner's say-so.

## Decision
Collaboration is replication of history, never shared mutable state: each person edits their
own branches, and changes meet by merge, as agents' branches do.

### Phase 1: bundles (files)
- `history.export_bundle {branch, path}` writes a `.gusb` file: the branch's node chain from
  the root to its head (ids, parents, labels, actors, times, states) and every object those
  nodes reach (snapshots, features, parameter tables, blobs), zstd-compressed, with a header
  naming the source project id, the branch name and a BLAKE3 of the content.
- `history.import_bundle {path}` reads it into another copy: objects are stored by hash
  (deduplicated for free); nodes are re-created with fresh local ids, parent links remapped;
  a node whose snapshot and remapped parent equal an existing node is reused, so importing
  the same bundle twice, or a bundle that shares a past, adds nothing new. The head becomes a
  branch `from/<source branch name>` (renamed on clash), never `main`; the user compares and
  merges it as any branch. Imports go through the same limits as files (sizes, counts,
  decompression budget) and every object is re-hashed on read: a bundle is untrusted input.
- Any file transport works (mail, a shared folder, git). This is useful on its own and is the
  wire format of phase 2.

**Implemented (2026-11-08)**: `History::export_bundle` / `import_bundle` (`gusset-history`
`bundle.rs`), commands `history.export_bundle {branch?, path, overwrite?}` and
`history.import_bundle {path}`. File: `GUSB`, version 1, BLAKE3 of the payload, zstd payload
(branch name, nodes root first, objects). Import limits (`BundleLimits`): file 512 MiB, decompressed
1 GiB, 100 000 nodes, 1 000 000 objects, 256 MiB per object (16 MiB for non-blob objects); every
object is re-hashed, each snapshot must read back and be complete, objects no node uses are refused,
parents must precede children and only the first node may be a root. A root with no local twin
becomes a second root (no common base, so no merge). The existing branch `from/<name>` at the same
head is reused (a re-import changes nothing); other clashes get a numeric suffix. The `.gus`
format is unchanged; the source project id is not carried (not needed to import).

**Folder sync (2026-11-08)**: `history.sync {folder, who}` (the user only) writes each of
your branches not received from others to a shared folder (a synced drive, a NAS) as
`<who>__<branch>.gusb`, naming them `<who>/<branch>` inside, and imports everyone else's
bundles there as `from/<who>/<branch>`; unchanged files are not rewritten, a re-import adds
nothing, a damaged file is reported and skipped, at most 256 files a sync. Run it now and
then (or from a timer) for near-live sharing without any network code. The app's Share
menu repeats the last sync every two minutes when asked to ("Sync automatically"; off by
default, only failures reported, a delayed wake-up rather than a render loop).

### Phase 2: live relay (later)
A relay (`gusset serve`, opt-in, TLS, per-project tokens) forwards bundles of new nodes as
they are made; clients mirror each other's branches read-only as `from/<user>/<branch>`
with presence (who is on which branch, selection). Edits never write into someone else's
branch; merging stays explicit. No CRDT is needed: nodes are immutable and only branch heads
move, each owned by one writer.

## Consequences
- No conflicts at the replication layer: the only merge is the existing feature-level merge.
- Blobs travel with the bundle; a later phase can send only the objects the receiver lacks
  (a have/want exchange over hashes), which is what the relay will do.
- Ids in messages and logs stay local; bundles carry the source ids only to remap parents.
