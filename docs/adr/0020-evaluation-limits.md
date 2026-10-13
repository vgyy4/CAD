# ADR 0020: Evaluation work limits and cancellation

- Status: accepted
- Date: 2026-10-13
- Review findings: H3 (v0.2 code review), V1 and V2 (v0.2 security review)

## Context
The text parser has size, nesting and count limits, but geometric evaluation had none. A
few lines of a document could keep the model thread busy for minutes or hours: one
`pattern_linear` with 500 disjoint copies took 315 s, every line of a chain of count-2
patterns doubled the body and the time (30 lines would never finish), a plate with 400
holes took 105 s, and twenty close 1,000-point splines took 106 s in the sketch
arrangement. The model thread serves the GUI, saving and every agent, so such a document
freezes the app on open, on every reopen and on recovery (threat model T1, T5).

## Decisions

### 1. One boolean per feature, not per tool
`Kernel::boolean_many(op, target, tools)` unites or cuts all tools in one operation (OCCT:
one `BRepAlgoAPI_Fuse`/`BRepAlgoAPI_Cut` with the tool list, sequential mode for
determinism, history recorded per input: 0 = target, `i + 1` = tool `i`). Holes, body
patterns, mirror with merge and `combine` use it and name the result once with
`OpContext::Modify { inputs: [target, tool_1, ...] }`. The trait's default (and the
intersect case, whose OCCT tool-list semantics differ) applies the tools one at a time and
composes the face histories. If the single operation fails, the evaluator applies the tools
one at a time to name the failing hole, instance or tool body in the error.

The naming golden scenarios (mock and OCCT) and every modeling test give the same face names
as before: a face's name depends on its origin and, for split faces, on geometric keys, not
on the order the tools were applied in, so ADR 0003 is unchanged. Times: docs/PERFORMANCE.md.

### 2. Work limits per evaluation (`gusset_eval::EvalLimits`)
Every feature that can multiply a body (holes, body patterns, mirror with merge, `combine`)
is charged before the kernel is asked:

| Limit | Default | Counts |
|---|---|---|
| `max_body_faces` | 10,000 | faces put into the feature's boolean: the target's plus every tool's (for a pattern: instances x body faces). This also bounds the result's faces. |
| `max_boolean_work` | 1,000,000 | the same number summed over the features of one evaluation |
| `max_tools` | 20,000 | pattern instances, holes and tool bodies in one evaluation |

Also: at most 1,000 holes per feature, now also for holes from a sketch (`MAX_HOLE_SITES`),
and at most 500 instances per pattern (unchanged).

Exceeding a limit fails the feature with error code `too_complex` and a hint; later features
on that body fail or are blocked as usual. The defaults allow a plate with 1,000
counterbored holes or a 500-instance pattern of a 20-face body; a chain of doubling patterns
of a box stops at its 11th line (6,144 faces built; with OCCT that is a few seconds instead
of hours). Ordinary sketch-based features (extrude/revolve/sweep/loft with `op: add/cut`)
cannot multiply a body and are not charged.

The running totals live in the per-feature evaluation state, so whether a feature is within
the limits depends only on the document and the limits, never on what the incremental
cache held. There is deliberately **no wall-clock limit**: results must not depend on the
machine's speed. Long but legitimate rebuilds are stopped by cancellation instead.

### 3. Sketch arrangement work (`MAX_INTERSECT_WORK`)
Spline intersection tests only the cell pairs whose boxes can overlap (cells sorted by their
left edge, binary search for the reach in x; the pairs are processed in the old order, so
results are bit-identical) and skips segment pairs whose boxes are further apart than the
search distance. Cell-pair tests, segment distance tests and Newton iterations are counted;
past 5,000,000 (about 1 s) the sketch fails with "the sketch's splines run too close to each
other ...". The densest sketches in the tests need at most 70,000.

### 4. Cancellation
`Evaluator::cancel_flag()` is an `Arc<AtomicBool>`, checked between features and before each
charged boolean (one kernel operation always runs to its end). A cancelled evaluation has
`Evaluation::cancelled` set, marks the features from the interrupted one on as
`cancelled` failures, and caches nothing it did after the cancel. The flag stays set until
its owner clears it, so all evaluations of the command being cancelled stop.

`gusset-host` owns it (`Session::rebuild_cancel_flag`) and clears it when a job starts. It is
set by `HostHandle::cancel_rebuild` ("Stop rebuild"), by queueing a mutating command while
another job runs (`call`, `call_async`, and the GUI's `submit_command` through
`supersede_rebuild`: the newer edit makes that rebuild moot), by dropping a running
`call_async` future (an MCP client cancelling its request) and by `shutdown`. The cancelled
job still completes: an edit is committed, a cancelled evaluation is not cached by the
session and does not mark the node broken, and the next job rebuilds.

## Consequences
- Very large legitimate parts (more than 10,000 faces from one pattern or hole feature) need
  higher limits (`Evaluator::set_limits`, which clears the cache); there is no UI for that yet.
- A single kernel operation cannot be interrupted; the face limit bounds how long one can take.
