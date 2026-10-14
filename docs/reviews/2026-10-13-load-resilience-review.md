# Review: ADR 0022 load-resilience milestone (01cabf0..f7d2b90)

## Resolution (2026-10-13)
- H1 fixed: workers start from one long-lived spawner thread per connector; `Kernel::probe`
  notices a worker that died while idle before a rebuild (no feature is blamed).
- H2 fixed: a crash is always memoised (a `kernel_crashed` error is made if the feature
  reported something else); up to 32 restarts; the crashed-pass result is `transient`
  and never cached.
- H3 fixed: every host job runs in the cancel scope; scene tessellation stops at a crash
  or stop and reports it, keeping the last picture.
- M1 fixed: 200 ms grace (`CANCEL_GRACE`) before the worker is killed.
- M2 fixed: a crash without a new generation (start failure) makes the evaluation
  `transient`: nothing from it is cached by the evaluator or the session.
- M3 mitigated: replies are capped at 1 GiB (`MAX_REPLY`) and reserved fallibly; decode
  amplification is left as is (bounded by the cap).
- M4 fixed: `render_view` has the deadline and stops its job when dropped; the deadline
  message no longer claims "no effect" for every command. Remaining: (b) the race is
  narrowed, not closed.
- L1, L2, L3, L7, L8, L9 fixed; L10: `model.rebuild` ("Rebuild all") retries memoised
  crashes. L6: ADR 0022 §3 amended. N1 (CI order), N4 (ping timeout), N5 (error text
  capped) fixed.
- Fixed 2026-10-14: L4 (the gate is a required `RemoteKernel::new` argument, raw STEP/IGES
  requests are refused by client and worker) and L5 (a `Status` request, protocol 3, returns
  the worker's sandbox report; shown by `self-test` and in the app's diagnostics).
- Fixed 2026-10-14: N2 (the worker drops the shapes of a reply it cannot encode; a reply
  the app cannot decode restarts the worker) and N3 (a call whose deadline passes before it
  started is `busy`, "nothing changed", not "simplify the model").

Code and security review, read-only. Line numbers are at f7d2b90. `af51715` (ADR 0024) and the
uncommitted changes to gusset-eval in the working tree are out of scope. For gusset-eval
lib.rs I used the f7d2b90 version. The other files cited match HEAD.

Counts: **3 high, 4 medium, 9 low, 5 nit**.

---

## HIGH

### H1. On Linux the worker dies when the thread that started it exits, and the next project blames an innocent feature for the crash
- `crates/gusset-sandbox/src/lib.rs:75` and `src/unix.rs:11-19`: `lockdown()` sets
  `PR_SET_PDEATHSIG(SIGKILL)`. Linux sends this signal when the **thread** that forked the child
  exits, not when the process exits (see prctl(2)).
- `crates/gusset-kernel-remote/src/client.rs:271-277`: `connect()` runs on whichever thread makes
  the first call after a restart. That is the `gusset-model` thread of the current Host. Only the
  very first worker is started from the main thread, by `production_kernel()`'s ping.
- `crates/gusset-app/src/app.rs:817-841` (`replace_host`) and `crates/gusset-host/src/lib.rs:649-662`:
  opening, creating or recovering a project shuts that model thread down and starts a new one.
  The `Arc<dyn Kernel>` (the same `RemoteKernel`) is shared by both.
- Scenario:
  1. The user presses Stop once, or a feature crashes, or an MCP deadline fires. The worker is
     restarted from the model thread (generation 2).
  2. The user opens another project. The old model thread exits and the kernel SIGKILLs worker
     generation 2.
  3. The new project's first kernel call gets EPIPE, then `Died`, reported as "was killed
     (SIGKILL: out of memory, or stopped)". The generation goes up during that feature's `apply`.
  4. The evaluator reports that feature as `kernel_crashed` and memoises it. It stays failed
     (and its dependants stay blocked) until the user edits it.
- Same root cause: a worker that died while **idle** is blamed on whatever feature makes the next
  call. Causes include the system OOM killer or an external kill.
- Fix:
  - Start workers from one dedicated long-lived thread (for example a spawner thread owned by
    `RemoteKernel`, reached over a channel), or stop using PDEATHSIG for this worker. It already
    exits on stdin EOF; it could also watch stdin from a thread.
  - Before writing a request, check `has_exited()`. If the worker is already dead, restart it and
    report that differently (bump the generation, but not as a crash of this call), so the
    evaluator does not memoise it.

### H2. The crash memo fails whenever the feature's final error code is not `KernelCrashed`: the worker crashes 5 times and the whole model disappears
- `crates/gusset-eval/src/lib.rs:544-564`: a crash is memoised only when the last status is
  `Failed` with `code == KernelCrashed`. A restart is triggered whenever the epoch changed.
- `lib.rs:928-962` (`blend_error`): **any** kernel error from fillet or chamfer becomes
  `FilletTooLarge` when `size >= shortest_selected_edge * 0.5` (line 944). That includes
  `Crashed`. Nothing earlier enforces this limit: the command-side check
  (`commands.rs:~1840`) only rejects sizes at or above the body's extent, and parameter or
  code-view edits skip it anyway.
- Scenario: a fillet whose radius is at least half its shortest edge crashes OCCT. This is the
  classic OCCT crash.
  1. The status is `fillet_too_large`, so nothing is memoised and `crashed = true`.
  2. `evaluate()` (`lib.rs:492-506`) restarts. The feature runs again and crashes again. This
     repeats `MAX_CRASH_RESTARTS` = 4 times, so 5 worker crashes and 5 full rebuilds from
     scratch.
  3. It then returns the crashed-pass `Evaluation` with `bodies: [], meshes: [], sketches: {}`,
     `cancelled: false`. Later features have no status at all.
  4. `Session::remember_eval` (`session.rs:384-391`) caches it, because it is not cancelled. The
     model vanishes from the viewport and MCP, and it stays that way for that node.
- Any other path that rewrites the error code after a crash does the same.
- Fix:
  - In the epoch-changed branch, always memoise the step key. If the status code is not
    `KernelCrashed`, build a `kernel_crashed` error.
  - Have `blend_error` and the `on_error` closures return `kernel_err` unchanged when
    `ke.kind == Crashed`.
  - When restarts run out, never return an empty model. Return the pre-crash prefix with the
    remaining features marked blocked or failed, and do not cache it.

### H3. Kernel calls outside `Evaluator::evaluate` have no cancel scope: Stop and the MCP deadline cannot end them, and crashes there are swallowed
- The only production `cancel::scope` is `gusset-eval/src/lib.rs:492`. These calls all run on
  the model thread with `cancel::current() == None`, so `RemoteKernel` never watches them
  (`client.rs:300-306`):
  - GUI `build_scene` tessellation, which runs on every scene refresh (`app.rs:2107`)
  - MCP `render_view` (`mcp/src/server.rs:868-870`)
  - export STEP/IGES (`commands.rs:2841`)
  - mesh export (`commands.rs:2879`)
  - measure / mass properties (`commands.rs:191-192, 2680-2681`)
- Hang scenario: BRepMesh, which is not cooperative, loops forever on a bad body during
  `build_scene`.
  - `cancel_rebuild()` sets the flag, but nobody watches it, so the model thread blocks forever
    and every later job queues behind it.
  - Over MCP, the deadline drops the future and the agent is told "was stopped; it has no
    effect". The model thread is still stuck.
  - This contradicts the ADR State text: "A Stop ends even an operation that would never return".
- Crash scenario: tessellation of body 1 crashes the worker.
  - `.ok()?` (`app.rs:2107`) swallows the error. Every later body fails with the STALE error, so
    the viewport goes empty with no message.
  - Nothing is memoised and nothing is marked unhealthy (ADR §2 "two crashes ... offer Retry" is
    not implemented). The next scene job drops the Session cache because of the epoch change,
    rebuilds everything and crashes again.
- Fix:
  - Wrap every host job in `gusset_kernel::cancel::scope(&inner.rebuild_cancel, ...)` in
    `model_loop` (`host/src/lib.rs:~300-310`). The flag is already cleared per job.
  - In `build_scene` and `render_view`, stop at the first `Crashed` and surface it.
  - Implement the "unhealthy, keep last meshes, Retry" state.

---

## MEDIUM

### M1. Cancel kills the worker at once, with no 200 ms grace (ADR §3), so every Stop or supersede throws away the whole cache
- `client.rs:81-100`: the watch thread calls `interrupt()` on the first 10 ms poll that sees the
  flag. ADR §3 says the worker is killed only if the call has not returned within 200 ms.
- Effect: a cancel during even a millisecond `topology` call kills the worker. The epoch changes,
  so `sync_epoch` clears the evaluator cache and Session drops every cached evaluation. The next
  rebuild starts from nothing.
- `HostHandle::supersede_rebuild` cancels the running job whenever a mutating command is queued,
  for example by an agent. An agent editing every few seconds can therefore keep a slow model from
  ever finishing a warm rebuild. That is a livelock, and a regression from cooperative
  between-features cancel.
- Fix: record when the flag was first seen and kill only after a grace period (200 ms per the
  ADR). Consider OCCT's progress scope for booleans.

### M2. A failed worker start is not a generation change, so failed states are cached and stay after the worker works again
- `client.rs:271-277`: when `connect()` fails, `call` returns `Crashed` ("could not start the
  geometry kernel") without bumping the generation.
- In `evaluate_pass` the epoch is unchanged, so this counts as an ordinary failure. The failed
  state is cached (`lib.rs:573`), and the `Evaluation` is cached by Session (same epoch).
- Scenario: a transient spawn failure (EAGAIN or ENOMEM on fork, ETXTBSY while the updater
  replaces the binary, or a Windows job assignment failure) makes every feature fail with
  `kernel_crashed`. Those failures stay cached after the worker can start again, until the
  document upstream changes.
- Fix: bump the generation on a connect failure, or never cache a state or `Evaluation` that
  contains an error derived from `Crashed`.

### M3. Memory in the app from worker replies is not bounded by RAM, and running out aborts the app
- `frame.rs:10`: `MAX_FRAME` is 4 GiB for replies too. `frame.rs:66-68`: `read_to_end` keeps
  growing (by doubling) up to the length the worker announced.
- Then the decode makes more copies:
  - `Reply::Bytes(r.bytes()?.to_vec())` (`proto.rs:335`) copies the payload once more.
  - `get_v3f32s` (`wire.rs:408-417`) first builds a full `Vec<f32>` and then a `Vec<[f32;3]>`.
  - `Mesh.edges` (`wire.rs:788-792`): `seq_len(4)` followed by one `Vec` (24 bytes) per 4 input
    bytes is 6x amplification, up to `MAX_SEQ*24` ≈ 1.2 GB.
  - `Vec<T>::MIN_SIZE = 4` gives the same amplification for nested sequences.
- Peak app memory for one multi-GiB mesh or export reply is several times the payload. Rust aborts
  when an allocation fails, so the **app** crashes. That is exactly what the milestone is meant to
  prevent.
- Triggers: a legitimately huge tessellation or export, or a compromised worker. The app also has
  no limit of its own matching the worker's budget.
- Fix:
  - Cap reply frames separately, for example at min(1 GiB, RAM/8), and refuse larger ones as
    `Crashed` or `Internal`.
  - Use `try_reserve` for the payload.
  - Decode f32 triples straight into `Vec<[f32;3]>`.
  - Keep one running allocation budget per message instead of per-sequence limits.

### M4. MCP deadline: "it has no effect" is not always true, and `render_view` has no deadline
- `mcp/src/server.rs:672-684` returns "did not finish ... and was stopped; it has no effect". It is
  false in four cases:
  - (a) `revert_point` (`host/src/lib.rs:222-230`) returns `None` for `history.*`, `branch.*` and
    non-mutating commands, so their effects persist.
  - (b) A race: the job checks `abandoned` (`host/src/lib.rs:502`) and returns. The timeout fires
    before the oneshot reply is delivered. The commit stays, but the agent is told it had no
    effect.
  - (c) The hang case in H3: the call is not stopped at all.
  - (d) `render_view` (`server.rs:861-870`) calls `exec_async` directly, so it has no deadline
    and no cancel. A dropped request leaves the evaluation and tessellation running.
- Fix:
  - Return a different message when nothing was reverted.
  - Make complete-or-abandon atomic, for example a state machine under one mutex in the job
    closure.
  - Route `render_view` through the deadline and a cancel scope.

---

## LOW

### L1. Pending frees are lost when the request fails before the Free is sent, so shapes leak in the long-lived worker
- `client.rs:282-287` drains `frees` before:
  - encoding, which can fail with STALE (`288-290`)
  - the cancel pre-check in `send` (`293-295`), which returns `stopped()` before the `Free`
    request goes out (`329-333`)
- The worker keeps those shapes until it restarts. Every Stop or supersede with pending drops
  leaks more. In a long session this uses up the memory budget, and the resulting OOM crash is
  then blamed on (and memoised for) an innocent feature.
- Fix: drain only after the `Free` succeeds, or re-queue on failure.

### L2. macOS and other non-Linux Unix: pgid kill after reap
- `sandbox/src/lib.rs:436-445`: outside Linux, `has_exited` is `try_wait`, which **reaps**.
- `kernel-worker/src/lib.rs:100-113`: `reap()` then calls `Guard::kill()` → `killpg(pgid)` while
  `reaped` is still false. The watch thread's interrupter can also fire in that window.
- This is the pgid-reuse hazard the `Guard` comment says it avoids. The window is small, but it
  is real.
- Fix: call `has_exited` while holding the Guard lock and set `reaped = true` as soon as
  `try_wait` reaps.

### L3. `OcctGate::written` trusts and persists whatever the worker returns from `write_native` (latent)
- `kernel-worker/src/lib.rs:215-217` calls `gusset_worker::trust::trust`. The app persists that
  list to `trusted-blobs-v2` (`app/src/main.rs:47`).
- There is no production caller of `write_native` today. Once one exists, a compromised kernel
  worker could get attacker-chosen blobs into the installation's trust list.
- Those blobs would then be parsed unvetted by `BinTools::Read` in an unsandboxed process: the
  in-process fallback (`GUSSET_KERNEL_IN_PROCESS`, missing worker, failed ping) or the CLI.
- This breaks the ADR 0008 §4 invariant that only our own writer in a trusted process, or the
  import worker's canonical output, is trusted.
- Fix: keep worker-written hashes in a separate set that lives only in this process and is never
  persisted. Honour it only in `OcctGate`, for sending blobs back to the worker.

### L4. `RemoteKernel` without a gate, and raw import requests, reach the long-lived worker (API footgun)
- `client.rs:220-236`: the gate is optional, and `RemoteKernel::new` has none. With no gate,
  `read_native` forwards unvetted blobs (`477-486`).
- `import_step` and `import_iges` (`457-467`) send raw file bytes to the worker that holds every
  user shape.
- Not reachable in production: `production_kernel` sets the gate, and imports go through
  `WorkerImporter`.
- Separately, `import_iges` in the worker writes a temp file, which the worker's Landlock and
  seccomp forbid, so it always fails there.
- Fix: make the gate a constructor argument. Refuse the import requests in `RemoteKernel`, or
  delegate them to the import worker.

### L5. Nobody can see whether the kernel worker's sandbox was applied
- The worker's stderr goes to `Stdio::null()` (`kernel-worker/src/lib.rs:74`). `main.rs:287-292`
  only `eprintln!`s what is missing.
- `self-test` and the release smoke tests check "in its worker process", but not whether
  Landlock, seccomp and the rlimits were applied.
- Fix: return `report.to_text()` in the Ping reply, and show it in `self-test` and Settings.

### L6. ADR §3 does not match the code
- Linux uses `RLIMIT_AS` (`sandbox unix_limits`), not `RLIMIT_DATA`. `budgets.rs` admits this,
  but the ADR still argues against AS.
- macOS has no 100 ms RSS polling.
- There is no "needs more memory than N GB" message. Under `RLIMIT_AS`, running out usually shows
  up as SIGABRT or `bad_alloc` ("aborted"), with no hint about memory.
- Fix: update the ADR State section, or implement these.

### L7. `CancelRunning::drop` race (`host/src/lib.rs:211-217`)
- The sequence: drop reads `running == true`. Job A then finishes, and `model_loop` starts job B
  and clears the flag. Drop then stores `cancel = true`, which cancels job B (an unrelated job,
  possibly the user's).
- Since this milestone that also kills the worker and wipes the cache.
- Fix: one cancel token per job, or check a job id under a lock.

### L8. After `boolean_many` crashes, the fallback blames the wrong cause (`eval/src/ops.rs:225-245`)
- After `boolean_many` crashes, the fallback calls `k.boolean` with the now-stale target. That
  fails with the STALE `Crashed` error.
- The memoised error therefore reads "...this shape was made by a geometry kernel that has since
  restarted", prefixed with the label of tool 1. The real crash reason is lost.
- Fix: when `ke.kind == Crashed`, skip the fallback.

### L9. Bad `face_ranges` from the worker cause a long loop on the model thread that cannot be cancelled
- `eval/src/hole.rs:517-519` iterates `range.start..range.end` from the worker's mesh. A range of
  `0..u32::MAX` means about 4e9 iterations of `.get()` misses on the model thread, with no cancel
  check.
- Needs a buggy or compromised worker. The renderer already validates meshes
  (`MeshData::from_mesh`).
- Fix: validate meshes once in `RemoteKernel::tessellate` (indices < vertex count, ranges inside
  the triangle count), or clamp the range here.

### L10. Crash memo is permanent for transient causes
- Not counted in the totals above.
- The memo (`lib.rs:544-551`) has no Retry and no expiry. A crash caused by memory pressure from
  other apps, an external kill, or H1 keeps the feature failed until it is edited.
- ADR §2 mentions "Retry", which is not implemented.

---

## NIT

- **N1.** `ci.yml:177-178`: `cargo run -p gusset-cli -- self-test` runs right after
  `cargo test -p gusset-kernel-worker --features fault-injection`. The `target/debug` worker it
  uses is therefore the fault-injection build. Not a release risk: I checked that nothing else
  enables the feature and the release build does not. Still, CI self-test then runs a
  non-production binary. Rebuild the bins, or reorder the steps.
- **N2.** Shapes leak inside the worker:
  - `server.rs:57-61`: when `Reply::encode` fails after `id_of` has registered shapes, those
    table entries are never freed.
  - Likewise, when the client fails to decode a reply, the worker-side shapes are never freed.
- **N3.** The deadline counts time spent queued (`server.rs:672-684`). An agent call stuck behind
  a long user rebuild expires with a "simplify the model" hint, although it never ran.
- **N4.** `production_kernel()`'s `ping()` (`kernel-worker/src/lib.rs:246`) has no timeout. A
  worker that starts but never answers hangs app startup.
- **N5.** Error messages and hints from the worker go verbatim to MCP agents. A compromised worker
  could use them for prompt injection. Consider capping their length.

---

## Checked and found sound
- **Frame layer:** the length is checked against `MAX_FRAME` before reading, with no
  pre-allocation (the payload grows with the data that actually arrives). The BLAKE3 check covers
  the payload. Junk before the magic is capped at 1 MiB.
- **Wire decoding:** `seq_len` checks against `MAX_SEQ` and the bytes left. Decoding is total, and
  trailing bytes are rejected.
- **Linux kill-after-reap:** handled with `waitid(WNOWAIT)` plus the `reaped` flag under the Guard
  mutex.
- **Lock order:** Link → Watch.state → Guard. `reap` holds Guard but never Watch, and the watch
  thread never takes Link, so there is no cycle. `fired` handles the
  interrupted-just-after-answer case (`client.rs:311-315`).
- **Stale shapes:** `Refs::id_of` rejects shapes from another generation. Session drops
  evaluations from other epochs. Frees from older generations are discarded.
- **Spawn hardening:** absolute worker path next to `current_exe`, `env_clear()`, stderr to null,
  stdin/stdout pipes only (std uses CLOEXEC), Windows job object (kill on close, one process,
  memory limit, die on unhandled exception), `/DEPENDENTLOADFLAG:0xA00`, `codeload::harden()`.
- **Native blobs in production:** every blob passes `OcctGate::vet` (trusted hash, or the import
  worker's canonical re-serialisation) before the kernel worker parses it. An untrusted original
  never reaches `BinTools::Read` outside a sandbox.
- **Fault injection:** compiled out without the feature (it returns `Unsupported`), and no
  release path enables it.
- **Packaging and release:** `stage.sh` adds the worker on every platform, with patchelf rpath and
  install_name/rpath fixes. The release changes touch no secrets or permissions.
