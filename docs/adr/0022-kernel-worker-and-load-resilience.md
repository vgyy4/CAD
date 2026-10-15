# ADR 0022: Kernel worker process and load resilience

- Status: accepted (implementation in phases, v0.3)
- Date: 2026-10-13
- Owner request (2026-10-13): "when I give Fusion a complex thing to do my computer can't
  handle, or something just impossible that it doesn't notice at first, it crashes most
  of the time instead of stopping the operation and telling me the reason." DESIGN.md §1
  "Heavy or impossible work never crashes the app".

## Context
What already holds: work limits refuse runaway evaluations up front (`too_complex`,
ADR 0020); rebuilds can be cancelled between features; a cancelled MCP request has no
effect; OCCT exceptions are caught at the shim boundary (`GS_GUARD`) and become kernel
errors; imports run in a sandboxed, killable worker process (ADR 0008 §4).

What does not: OCCT runs inside the app process. A segfault, `abort`, stack overflow or
infinite loop inside one OCCT call takes the whole app down or freezes the model thread,
and nothing bounds the memory a rebuild may take, so a heavy fillet or boolean can push
the machine into swap and the OS may kill the app. A survey of OCCT 8.0.1 (2026-10-13)
shows that almost no modeling algorithm polls a user break: booleans check a progress
scope between stages, fillets, offsets, sweeps, meshing and validity checks never do. A
reliable "stop now" therefore cannot be cooperative; it has to be able to kill the
computation.

## Decision

### 1. Geometry runs in a kernel worker process
`gusset-worker` gains a second mode, `--kernel`: a long-lived process that owns OCCT
shapes and serves the `Kernel` trait. In the app, `RemoteKernel` implements `Kernel` by
sending each call to the worker; `RemoteShape` is a `ShapeData` holding a shape id and the
worker generation it lives in. Everything above the trait (evaluator, naming, commands,
MCP, GUI) is unchanged. Dropping a `RemoteShape` queues a `free` for its id (batched with
the next request). The in-process `OcctKernel` stays for tests, the CLI's `build`, and as a
fallback when the worker binary is missing (a warning in Settings and `self-test`).

- Protocol: request/response frames over the worker's stdin/stdout, length-prefixed with a
  magic and a BLAKE3 checksum like the import protocol (stray OCCT output on stdout cannot
  corrupt it). Arguments and results use a small explicit binary codec in a new
  `gusset-kernel` module (profiles, specs, topology, meshes as raw `f32`/`u32` arrays), no
  new dependency. One request in flight: the model thread is the only caller.
- The worker applies the same sandbox as an import worker (no filesystem except what an
  import or export call names, no network, dies with the app), so kernel and import
  isolation share one mechanism; imports may later run in the kernel worker directly.
- Cost: a call adds one round trip (tens of microseconds) plus copying results; shapes stay
  in the worker. The perf reports (`perf_gears`, `perf_booleans`, `rebuild_100_budget`)
  must stay within 10 % of the in-process numbers (PERFORMANCE.md rule).

### 2. A crash becomes an error on the feature, never on the app
If the worker dies (signal, abort, broken pipe) or misses a deadline, the call returns
`KernelErrorKind::Crashed { reason }`, the parent reaps it, and the next call starts a new
worker (new generation). Shapes of the old generation are gone: the evaluator drops its
cached states and rebuilds from the last state it still holds as data (the document), so
the model shows again, without the failing feature.

- The evaluator reports the feature that was running as failed with code `kernel_crashed`
  ("the geometry kernel crashed while building Fillet3: <reason>"), and the hint suggests
  the usual fixes for that operation (a smaller fillet radius, fewer pattern copies...).
- A crash is memoised by the feature's content hash and its inputs' hashes: the next
  rebuilds do not run that operation again (they report the same error at once) until the
  feature or something before it changes. Otherwise every rebuild would crash once more.
- Two crashes within a short window while rebuilding unchanged features (a crash outside
  any feature, e.g. in tessellation) mark the worker unhealthy: the app keeps the last
  good meshes and offers "Retry" instead of restarting it in a loop.

### 3. Memory and time budgets
- Memory: each worker gets a budget from the hardware tier (docs/HARDWARE.md), by default
  60 % of physical memory minus what the app itself uses, at most 32 GB, configurable in
  Settings. Linux: `RLIMIT_DATA` (counts heap and anonymous mappings, unlike `RLIMIT_AS`
  which TBB's address-space reservations would trip). Windows: the job object's
  per-process memory limit. macOS has no enforceable limit: the parent polls the worker's
  resident size every 100 ms and kills it at the budget. An allocation failure surfaces as
  OCCT's `Standard_OutOfMemory` (caught: an ordinary error) or as a crash; either way the
  error says "needs more memory than the N GB allowed" when the worker's peak was within
  10 % of the budget.
- Amendment (2026-10-13, review L6): as built, the budget is 3/4 of RAM, at least 2 GiB
  (`gusset_hw::budgets::kernel_worker_bytes`), enforced with `RLIMIT_AS` on Linux
  (`RLIMIT_DATA` on macOS, where `RLIMIT_AS` is not enforced) and the job object's
  per-process limit on Windows; TBB is not used by our OCCT build, so address-space
  reservations stay small. macOS RSS polling and the "needs more memory than N GB"
  message are not implemented yet: running out shows as an abort or an OCCT error.
- Time: no silent timeout for interactive work: the user sees progress and a Stop button.
  MCP calls already have a cancel (ADR 0012) and agents get a per-call deadline (default
  10 minutes, server setting) after which the worker is stopped like a cancel.
- Stop / cancel: if the running call does not return within 200 ms of a cancel, the parent
  kills the worker. The call fails with `cancelled`, the rebuild ends there, and the model
  shows the last complete state. Booleans additionally get OCCT's progress scope, so they
  stop without a kill when they can.

### 4. Refuse before starting where it is predictable
ADR 0020's limits stay the first line (faces per boolean, tools, pattern counts). They are
extended per operation as dogfooding and fuzzing find new explosive inputs (for example
fillet radius against the shortest edge, already checked in `feature.fillet`).

### 5. Tested
- The kernel worker has a test-only request (`debug_fault`, compiled only with the
  `fault-injection` feature, never in release builds) that segfaults, aborts, overflows the
  stack, allocates without bound or sleeps. Tests check that each one becomes the right
  error, that the app session survives, that the model rebuilds without the feature, and
  that the memo prevents a second crash.
- The full OCCT test suite runs once through `RemoteKernel` in CI (same goldens).

## Phases
1. Protocol, codec, `RemoteKernel` for every `Kernel` method; the OCCT suite through it.
2. Crash detection, restart with generations, evaluator invalidation, crash memo.
3. Memory budgets per platform; deadlines for MCP; Stop kills the worker.
4. Fault-injection tests; GUI: progress, Stop, the crash and memory errors in the feature
   tree; docs (HARDWARE.md budgets, USER guide).

### State (2026-10-13)
- Phase 1 done: `gusset-kernel-remote` (protocol, server, `RemoteKernel`) and
  `gusset-kernel-worker` (the sandboxed OCCT process); OCCT parity and fault-injection tests
  (abort, stack overflow, memory exhaustion, exit) pass.
- Phase 2 done: the evaluator fails only the feature whose call crashed the worker
  (`kernel_crashed`), memoises it, and rebuilds the rest on the new worker.
- Wiring done: the app, `gusset mcp`, `gusset build` and `gusset self-test` use
  `gusset_kernel_worker::production_kernel()`: the worker next to the executable with a
  memory budget of 3/4 of RAM (at least 2 GiB), else OCCT in-process with a visible note
  (`GUSSET_KERNEL_IN_PROCESS=1` forces it). Every installer ships the worker; the release
  smoke tests require `self-test` to report "in its worker process".
- Native B-rep trust stays in the app (ADR 0008 §4): `RemoteKernel` takes a `NativeGate`;
  `OcctGate` vets blobs (trusted, or re-serialized by the import worker) before they are
  sent, and records the worker's own `write_native` output as trusted. The worker parses
  what it is sent (`OcctKernel::with_vetted_inputs`), inside its sandbox.
- Phase 3 done: callers that can be stopped run their kernel calls in a cancel scope
  (`gusset_kernel::cancel`; the evaluator uses its cancel flag). While a call runs,
  `RemoteKernel` watches the flag on one helper thread (10 ms polling, only during a
  watched call) and kills the worker when it is set: the call fails as `Cancelled`, the
  generation moves on, and the next rebuild starts from scratch on a new worker. A Stop
  ends even an operation that would never return. MCP tool calls have a deadline
  (`DEFAULT_REQUEST_DEADLINE`, 300 s, from queueing to result, approval waits excluded):
  past it the call is dropped, which stops its rebuild and undoes its changes, and the
  agent gets `deadline_exceeded` with a hint.
- Phase 4 started: while a job runs the status bar names the feature being built
  ("Building Fillet1 (3/12)", from `Evaluator::progress_cell` through
  `HostHandle::rebuild_progress`) and offers Stop (`cancel_rebuild`). Still open: crash
  and memory errors styled in the feature tree, USER guide and HARDWARE.md budget lines.

## Consequences
- The app survives anything OCCT does; a crash costs a rebuild of the earlier features.
- Memory per session roughly doubles for meshes (the worker keeps shapes, the app keeps
  meshes and topology summaries, as today).
- A second long-lived process per open project; on low-end machines (HARDWARE.md tier 1)
  the worker budget is what keeps the app responsive.

## Amendment (2026-10-15): trust between pool workers
The workers of a pool share one trust gate: a native blob one worker wrote is read by
another without the import worker's vetting. Accepted: all workers run in the same
sandbox, on the same session's shapes, so a compromised worker could already corrupt
every result it returns; re-vetting each cross-worker copy would cost an import-worker
round trip per shape for no gain. Blobs that come from files are vetted as before
(security review v0.3 W10).
