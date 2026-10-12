# MVP re-review (2026-10-12)

Code and security re-review of `claude/new-session-nedlj5` at `8baa3f8`, after the
fixes for the 2026-10-11 milestone review. Both reviews were read-only; findings were
then fixed as listed.

## First-review findings
Verified fixed in code: C1–C4, H1–H8, M1, M2, M4–M8, M10–M12, L1–L4, L7.
Still open or partial after the re-review:
- **M9** (insert while rolled back recorded as two undo steps): fixed (one undo step).
- **M3** (save/snapshot as model-thread jobs, repeated verifies): partial. Saves are
  33 ms for 1,000 features (PERFORMANCE.md); moving them off the model thread is
  left for after the MVP.
- **M14** (benchmarks): budgets are now enforced by tests instead of criterion benches
  (`history_overhead_budget`, `rebuild_100_budget`, the release-mode solver test,
  `perf_save_and_snapshot`); dm-flakey style fault injection is still open.
- **L8** (evaluator clones state per rolled-back feature): open; negligible below
  thousands of features.

## New code findings

| # | Severity | Finding | Status |
|---|---|---|---|
| N1 | medium | Insert while rolled back evaluated as rolled back (wrong errors, node state, body changes) | Fixed |
| N2 | low-medium | Eval cache charges only bodies a state added; prefix eviction frees nothing | Fixed: bytes per unique live body |
| N3 | low | Up-front edge check rejects on the first ambiguous body | Fixed (no mock-kernel test possible) |
| N4 | low | Grants keyed by self-declared client name | By design, documented in ADR 0012 (token holder = local user) |
| N5 | low | Late detached-import result after opening another project | Fixed |
| N6 | low | GC of a damaged v2 snapshot could delete its children | Fixed: GC deletes nothing then |
| N7 | low | "Keep both" check-then-write race | Fixed |
| N8 | low | Polyline accepts zero-length segments | Fixed |
| N9 | unverified | Row-echelon pivot may be round-off | Fixed (relative pivot tolerance; regression test) |

## New security findings

| # | Severity | Finding | Status |
|---|---|---|---|
| S1 | medium | Kill switch undone by opening another project | Fixed: the window stays paused, new endpoints start paused |
| S2 | medium | Worker-validated blobs later parsed in-process from the original bytes | Fixed: only writer-produced bytes are trusted; `trusted-blobs-v2` |
| S3 | medium | "All actions" grant covered set main and edits of main | Fixed: never covered by "all actions" |
| S4 | low | Approval prompt showed clipped raw arguments | Fixed: server-computed summary (target, set_text diff) |
| S5 | low | Clients named like `gpt-5` could not keep grants | Fixed |
| S6 | low | JSON-RPC batches bypassed the read-only bridge | Fixed: batches refused in read-only mode |
| S7 | info | IGES temp copies left after an app crash | Fixed: start-up sweep of old `gusset-import-*` |
