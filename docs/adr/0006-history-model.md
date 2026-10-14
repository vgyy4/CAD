# ADR 0006: One history model — undo tree, feature timeline and iteration branches

- Status: accepted
- Date: 2026-10-10

## Context
The spec requires undo/redo, the feature timeline and the iteration tree to be ONE
system, shared by the GUI and agents. Undo must be a tree. Branches must be first-class
and cheap, and storage has to be small (only parametric definitions, structural
sharing, content addressing). The spec rules out git and embedded git libraries.

## Decision

### Objects (immutable, content-addressed)
Each object is hashed with BLAKE3 over its canonical bytes and stored once, zstd-compressed:
- `Feature`: canonical text of one feature statement (ADR 0004).
- `Params`: canonical text of the parameter table.
- `Blob`: imported files (STEP, meshes, images), stored by hash, shared by all branches.
- `Snapshot`: document header plus the ordered list of `Feature` hashes and the `Params`
  hash. A typical edit changes one feature, so it costs one small feature object and one
  snapshot (32 bytes × features). If profiling shows snapshot size matters for very
  large models, the list becomes a chunked persistent vector; that is a format-compatible
  change.

### Nodes
`Node { id, parent: Option<NodeId>, snapshot, label, actor, time, state }`
- `state` is `Ok` or `Broken(error)`. A failing edit is still recorded, as a broken node
  the user can fix or discard. A rebuild failure never mutates existing nodes.
- `actor` is `User` or `Agent(session name)`, which drives the audit log and per-actor undo.
- Nodes form a tree (each has one parent). Children are ordered by creation, so the
  "redo future" is never lost: undo followed by a new edit creates a sibling.

### Branches
`Branch { id, name, head: NodeId, color, notes, starred, pinned, created_by, deleted_at }`.
Exactly one branch is `main`. Default names are "Iteration N". Agents get
`agent/<session-name>` branches, based on main by default.

### Events: the single log that makes it one system
Every user-visible change appends an `Event` to the project's event log:
`Edit{branch, from, to}`, `Undo{branch, from, to}`, `Redo{...}`,
`CreateBranch`, `RenameBranch`, `SetMain{old,new}`, `DeleteBranch` (soft),
`RestoreBranch`, `CherryPick{...}`, `SetRollback{branch, index}`, `Switch{...}`.
- **Undo** (Ctrl+Z) acts on the actor's most recent undoable event in the current branch
  context. Undoing an `Edit` moves the branch head to the parent node; nothing is
  deleted. Undoing `DeleteBranch` or `SetMain` restores the previous state. Undo of
  structural events works within the session.
- The **feature timeline** shows the snapshot at the current branch head. The
  **rollback marker** is per-branch state (`SetRollback` event). Edits made while rolled
  back insert at the marker.
- The **iteration tree** view is the node tree with branch heads, stars and colors.

### Branch operations (GUI and MCP share the same commands)
create (from any node), rename, switch (instant: the head snapshot is rebuilt from the
feature cache), compare (parameter/feature diff from snapshots; geometric overlay from
evaluated bodies), cherry-pick (apply chosen feature objects or parameter changes onto
another head, validated by a rebuild; the result is a normal node, broken if the rebuild
fails), set as main (logged, undoable, notification; needs approval for agents), delete
(confirm flag; main cannot be deleted; soft-delete into "Recently deleted" with
retention, default 7 days; warns if it contains starred nodes).

### Garbage collection and pruning
- Reachability: nodes are reachable from branch heads, including soft-deleted branches
  until retention expires, and from starred/named nodes. Objects are reference-counted
  through reachable snapshots. Compaction drops everything else.
- Pruning (configurable, visible, can be turned off): unnamed intermediate nodes older
  than N days, or beyond M per branch, are squashed. The kept node inherits the newest
  snapshot of the squashed run. Starred and named nodes are never pruned.
- "Compact history" runs GC and pruning immediately. The history size per project is
  shown in Settings → Storage.

### Full merge (done 2026-10-14)
`branch.merge` (`from`, optional `prefer: ours|theirs`) merges another branch or iteration
into the caller's branch, three-way by name against the merge base: units, every
parameter and every feature is taken from the side that changed it since the base;
the same change on both sides is no conflict; different changes (including removed on
one side, edited on the other) are conflicts. Features keep our order; features only
they added go right after the feature that precedes them on their side; a reorder of
shared features on one side only is taken, on both sides it conflicts. Without `prefer`
any conflict fails the merge with the list (nothing changes). The result is validated and
committed as an ordinary node (undoable, rebuilt; a failing rebuild is committed broken
like a cherry-pick). Nodes have one parent, so the merge node's label records the merged
iteration (`Merge idea (n12)`); the next merge from that branch uses it as the base, so
changes merged once never conflict again.

## Consequences
- Derived geometry is never stored in history; inactive branches rebuild on demand from
  the evaluation cache.
- The event log grows with activity. Events are tiny, and pruning compacts the log too
  (keeping structural events within the retention window).
