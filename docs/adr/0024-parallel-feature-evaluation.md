# ADR 0024: Parallel feature evaluation across kernel workers

- Status: accepted (implementation in phases, v0.3)
- Date: 2026-10-13
- Builds on: ADR 0022 (kernel worker), ADR 0023 §3 (multithreading where it pays), ADR 0020
  (work limits), ADR 0003 (naming)

## Context
The evaluator builds features one after another on one kernel. Most of the time of a
heavy model is a few long kernel operations that do not depend on each other: the two
gears of a pair (5-7 s each when helical), several imported or generated parts, bodies
that are only combined at the end. OCCT parallelises some algorithms internally (and we
added parallel validity checks and measures), but a single sweep or boolean mostly runs
on one core. OCCT keeps global state, so two operations cannot safely run in one process;
since ADR 0022 each kernel is a process of its own, so several can run at once.

The cache is keyed by document prefix: a feature's key hashes the key of the feature
before it. Editing the first gear therefore rebuilds the second one too, although nothing
in it changed.

## Decision

### 1. A dependency graph of features
Each feature gets a read set and a write set:
- reads: `FeatureKind::dependencies()` (sketches, planes, edges and faces by naming
  reference, bodies), the parameters it uses, and the current state of every body it
  modifies;
- writes: the body it creates or modifies (`op: new|join|cut|intersect`, `body:`), the
  sketch or plane it defines.
Feature `j` depends on an earlier feature `i` when `j` reads anything `i` writes, or both
write the same body. Features whose kind cannot be analysed (future kinds) depend on
every earlier feature, so the graph is always safe.

### 2. Dependency keys instead of prefix keys
A feature's cache key hashes the feature text, its parameter values and the keys of the
features it depends on, not the key of the feature before it. Editing one gear then keeps
the other gear and everything built only from it cached. Statuses, bodies and planes are
still assembled in document order, so results do not change.

### 3. Waves on a pool of workers
Features run as soon as their dependencies are done, on a pool of `N` kernel workers
(`RemoteKernel`s, each its own process). `N` = min(background threads, kernel memory
budget / 2 GiB, 4); the memory budget of ADR 0022 is split between them. With `N = 1`
(and for in-process kernels) evaluation is exactly today's sequential loop. A shape needed
on another worker moves with `write_native`/`read_native` (its own output, so trusted;
the measured cost is milliseconds for gear-sized bodies).

### 4. Determinism
Each kernel operation runs single-threaded per worker with the same inputs as in the
sequential order, and naming depends only on operation history (ADR 0003), so faces,
names, volumes and meshes are identical. Acceptance: the naming golden scenarios and the
OCCT goldens give bit-identical topology summaries and volumes with `N = 1` and `N = 4`.

### 5. Limits, crashes, cancel
- Work limits (ADR 0020) stay global and are charged in document order. A feature's
  charge depends only on its inputs, so the totals are the same as before; booleans may
  start speculatively, and after the wave the totals are summed in document order: a
  feature beyond a limit fails as today and its result is discarded. The extra work is
  bounded by `N` features.
- A crash ends one worker; the crash memo and restart of ADR 0022 work per feature as
  today. The evaluator's epoch becomes the combination of the pool's generations.
- The cancel flag scope (ADR 0022 §3) is entered on every pool thread, so Stop and
  deadlines kill every worker that is busy.

## Phases
1. Read/write sets and the graph (with tests over every feature kind); dependency keys
   with the sequential loop (incremental rebuilds improve on their own).
2. The worker pool and waves; equivalence tests `N = 1` vs `N = 4`; perf report on the
   gear pair (target: the pair rebuilds in about the time of its slower gear).
3. Tessellation and topology summaries of independent bodies on the pool.

## State (2026-10-13)
- Phase 1 done: `deps::graph` (read/write analysis, barriers) and a second cache tier:
  next to the state after each document prefix, the evaluator stores what each feature
  changed (`delta::Delta`: bodies replaced, removed and appended, mesh bodies, solved
  sketch, datum plane, status, work) under its dependency key. When the prefix misses,
  a delta whose dependencies are unchanged is replayed instead of building the feature;
  a delta is stored only if replaying it onto the state it came from reproduces the
  result, and replayed only when the work limits see the same totals. Crashes are
  memoised by dependency key. Tests: an edit to one part rebuilds only that part; a warm
  evaluator equals a fresh one over a sequence of edits and rollbacks with barriers,
  combine, copies and a failing feature.

- Phase 2 done: `PoolKernel` (several `RemoteKernel`s; an operation runs on the worker
  holding its first input, other inputs are copied with write/read native, new shapes go
  to the calling thread's worker slot, `gusset_kernel::slot`) and speculative builds in
  the evaluator: while a heavy feature is built, up to `parallelism - 1` later features
  that depend only on earlier features (plus cheap sketches/planes of their own) are
  built on other threads and workers on the same starting state; their results are kept
  as deltas (phase 1) and replayed when their turn comes, so the result is exactly the
  sequential one (tested against a sequential evaluator across edits and rollbacks).
  Work limits: a replayed delta's charge is added to the current totals and must stay
  within the limits, the same outcome as building it in place. After a crash or a cancel
  nothing built ahead is kept. The app's pool has one worker per interactive thread,
  each with at least 2 GiB of the memory budget, at most 4; the first starts with the
  app, the others when first used.
- Measured: cold rebuild of a helical gear pair, 6.2 s on one worker, 3.7 s on two (the
  time of the slower gear; 4-core container).

## Consequences
- Heavy models with independent parts rebuild up to `N` times faster on multi-core
  machines; a one-core machine behaves as today.
- More memory: up to `N` workers, each holding the shapes it built (bounded by the split
  budget).
- The evaluator gains a scheduler; the sequential path stays the reference and the
  default for in-process kernels and tests.
