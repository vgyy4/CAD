# Gusset: Phase 2 (MVP) milestone code review

- Reviewer: fresh, independent subagent. No repository files were modified.
- Revision reviewed: `d2d0b5b` (HEAD of `claude/new-session-nedlj5`). This already contains 692a265..d2d0b5b: the Windows lock sidecar, the occt-sys RPATH, the aarch64 seccomp gating and the font-licence exception. Those four commits are reviewed in §5.
- Environment: Linux x64, **no OCCT** (`GUSSET_OCCT_DIR` unset). Every geometry path ran against `MockKernel`. The OCCT golden tests and the cfg-gated OCCT code were not exercised.
- Reproductions live in `scratchpad/review/repro/` (a standalone crate that depends on the workspace crates by path, with its own target dir). Run with `CARGO_TARGET_DIR=$PWD/target cargo test --offline --test <commands|project|app|bridge|perf> -- --nocapture`. Every finding marked **[verified]** has a passing reproduction there or a fully traced code path, as stated in the finding. **[suspicion]** marks findings that were not reproduced.

## 0. Checks run

| Check | Result |
|---|---|
| `cargo test --workspace --no-fail-fast` | 401 passed, 0 failed, 1 ignored (`perf_binary_stl_2m_triangles`). 26 s on a warm target. `gusset-kernel-occt` and `worker` ran **0 tests** (cfg-gated on OCCT). |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| Benchmarks | **none exist.** No `benches/`, no criterion, no CI benchmark job. PERFORMANCE.md lists every budget as "not yet measured". |
| Ad-hoc perf probe (`repro/tests/perf.rs`, release build) | Sketch solver on a 400-segment chain (~800 unknowns): **4.2 s** per solve; 200 segments: 0.34 s. Growth is cubic. |

## 1. Findings summary

| Severity | Count |
|---|---|
| Critical | 4 |
| High | 8 |
| Medium | 14 |
| Low | 10 |

## 2. Findings (ranked)

### Critical

**C1. Opening another file discards the current project's unsaved work and deletes its recovery file, with no prompt. [verified]**
- Code: `crates/gusset-app/src/app.rs:630-658` (`App::open`). It calls `replace_host`, which closes the old project with `p.close()`. `Project::close` (`crates/gusset-project/src/lib.rs:402`) calls `discard_recovery()`. Nothing checks `is_dirty()` first.
- Scenario: the user edits a project, then chooses File → Open → other.gus. The edits are gone and so is the recovery snapshot. The spec requires the close dialog (Save / Don't save / Cancel) for every close.
- Reproduction: `repro/tests/app.rs::open_while_dirty_discards_unsaved_work` prints `recovery files before=1 after=0`, `dialog after open: None`.
- Fix: route Open, and every other path that replaces the host, through the same unsaved-changes dialog as window close. Only call `close()` after the user chose "Save" (and it succeeded) or "Don't save".

**C2. Running the headless `gusset mcp model.gus` bridge deletes a pending crash-recovery file, even if the agent does nothing. [verified]**
- Code: `crates/gusset-mcp/src/bridge.rs:213-262`. When `Project::open` reports `RecoveryAvailable`, the only reaction is a note on stderr. On exit the bridge calls `project.close()` (not dirty) or `project.save()` (dirty). Both delete the recovery file (`gusset-project/src/lib.rs:383,403`).
- Scenario: the app crashes with unsaved work. Later, Claude Code is configured to run `gusset mcp model.gus` while the app is closed. The agent connects (or merely fails to initialize), the session ends, and the only copy of the user's unsaved work is deleted.
- Reproduction: `repro/tests/bridge.rs` prints `recovery file exists afterwards: false` and `reopen offers recovery: false`. The connection never even initialized.
- Fix: when the report is not `Clean`, the headless bridge must not delete or replace the recovery file. Either refuse to start ("open the project in the app to resolve recovery first"), or leave the recovery file in place and save agent work without `recovery::remove`. Never auto-`close()` a project whose recovery was not reviewed.

**C3. An agent can write to the main branch without approval by renaming main to its own future branch name. [verified]**
- Code: `crates/gusset-commands/src/session.rs:170-194` (`branch_for` reuses any live branch named `agent/<name>`). `branch.rename` is plain `EDIT` and never calls `branch_for` (`commands.rs:1272-1286`). `check_policy` (`registry.rs:158-163`) only asks for approval when the agent's current branch is *explicitly* main. Agent names are predictable (`<client>-<n>`, `gusset-mcp/src/agents.rs:44-57`).
- Scenario: a prompt-injected agent's first call is `branch_rename {branch:"main", name:"agent/claude-code-1"}`. Every later edit goes to main with no human approval, and the user's main is now called `agent/claude-code-1`. This breaks the spec's "server-side enforcement, whatever the agent does".
- Reproduction: `repro/tests/commands.rs::agent_writes_main_without_approval_via_rename` passes. The edit lands on main.
- Fix:
  - Bind agent branches by `BranchId` and creator (`created_by == Actor::Agent(name)`), never by name.
  - Treat renaming main, or a branch the agent did not create, as `agent_approval`.
  - Make the main-write check unconditional: an agent edit whose resolved target branch is main needs approval, whether or not the choice was explicit. Do the check after `branch_for` resolves the target.

**C4. The recovery dialog's "Keep both" always fails, and the recovered work is then lost on close. [verified]**
- Code: `crates/gusset-app/src/app.rs:1252-1268` sends `branch.create {"name":"Recovered","switch":true}`. `BranchCreate` has `deny_unknown_fields` and no `switch` field (`commands.rs:1229-1237`).
- Scenario: the user picks "Keep both", the command fails with `invalid_args`, and nothing is kept. The dialog closes and the recovery file is deleted at the next clean close, so the unsaved work is lost.
- Three further defects in the same path:
  1. Even if it worked, it keeps only `recovered.head_document(recovered.main())`. Work on non-main branches, and the iteration tree, are dropped.
  2. The spec and ADR 0007 say "Keep both" writes the recovery as a separate copy (`<name> (recovered).gus`).
  3. A pre-existing branch named "Recovered" would also make it fail.
- Reproduction: `repro/tests/commands.rs::keep_both_branch_create_args_are_rejected` prints `branch.create: unknown field 'switch'`.
- Fix: write the recovered history to `<name> (recovered).gus` with `durable_write`, using a new project ID (see H2). Delete the recovery file only after that succeeds. Add a GUI test that clicks each recovery button.

### High

**H1. A never-saved ("Untitled") project's crash recovery is unreachable, and housekeeping would delete it. [verified]**
- Code:
  - `Project::create` registers nothing in the registry (`gusset-project/src/lib.rs:246-254`).
  - `housekeeping::run` treats an id with no registry entry as gone and deletes its recovery file (`gusset-store/src/housekeeping.rs:358,378-379`).
  - The app never scans the recovery dir at startup. It also never calls `housekeeping::run` (see M1).
- Scenario: the user models for an hour without saving and the app crashes. Next launch shows no recovery prompt. Once housekeeping is wired in, the file is deleted.
- Reproduction: `repro/tests/project.rs::untitled_recovery_is_deleted_by_housekeeping`. The report shows `recovery_files: 1`, i.e. deleted.
- Fix:
  - Register untitled projects (for example under a sentinel path), or mark untitled recovery files in their header.
  - At startup, offer "Recover unsaved work" for every orphan recovery file before housekeeping runs.
  - Never let housekeeping delete a recovery file whose project has no registry entry.

**H2. Save As (and file-manager copies) keep the project ID, so two files share one lock, one recovery file and one LKG slot. [verified]**
- Code:
  - `Project::save_as` → `save_to` keeps `self.meta.id` (`gusset-project/src/lib.rs:365-385`).
  - The lock, recovery and LKG paths are all keyed by id (`gusset-store/src/paths.rs:136-150`).
  - The recovery check on open does not compare paths (`lib.rs:294-302`).
- Scenarios:
  1. After Save As to b.gus, a.gus cannot be opened ("already open in another Gusset window"). The same happens for any file-manager copy opened beside its original.
  2. b.gus crashes with unsaved work. Opening **a.gus** offers b's recovery as a's. If the user chooses "Open last saved version", b's only recovery copy is deleted.
  3. Saving a.gus overwrites b's LKG generation.
- Reproduction: `repro/tests/project.rs::save_as_copies_share_identity` and `copied_file_cannot_be_opened_next_to_original`. Output: `opening a.gus offers recovery: true`, then `opening b.gus afterwards: recovery available = false`.
- Fix:
  - Give Save As a fresh `ProjectId`.
  - On open, detect id collisions: if the registry maps this id to a different existing path, re-key the opened file to a new id.
  - Store the project path (or a path hash) in recovery and LKG metadata and check it before offering them.

**H3. GUI exports (STEP/STL/3MF) always fail. [verified]**
- Code: `app.rs:1325-1352` (`path_action`) always builds an *absolute* path and passes it to `export.step` / `export.mesh`. `paths::confined` rejects anything that is not all `Component::Normal` (`gusset-commands/src/paths.rs:16-20`).
- Scenario: File → Export → STEP… → OK fails with "paths must stay inside the project folder". For an unsaved project it fails with "this project has no folder yet". "STEP export is always available" is a core spec promise.
- Reproduction: `repro/tests/commands.rs::gui_style_absolute_export_path_is_rejected`.
- Fix: user (GUI/CLI) exports must not go through the agent confinement. Add a trusted-caller path (`ctx.actor == User`) that accepts any absolute path the user chose, or a separate user-only export entry point. Also add a GUI test that exports.

**H4. The GUI rollback marker (timeline slider) never works. [verified]**
- Code: `app.rs:1027` sends `timeline.set_rollback {"marker": …}`, but the field is `index` (`commands.rs:1129-1135`, `deny_unknown_fields`). The error is reported "quietly" in the status bar.
- Scenario: a basic Fusion-style timeline requirement (rollback marker) is broken in the GUI.
- Reproduction: `repro/tests/commands.rs::gui_rollback_slider_args_are_rejected` prints `unknown field 'marker', expected 'index'`.
- Fix: send `index`, and add a kittest that drags the slider. More generally, add a test that validates every JSON payload the GUI builds against the command schemas.

**H5. STEP/IGES import is not wired anywhere, and mesh import (STL/3MF/OBJ) is missing. [verified]**
- Code:
  - `Session.importer` is `None` everywhere. The only `impl Importer` is a test double (`gusset-commands/src/tests.rs:246`).
  - `gusset-worker` exists but nothing creates a `WorkerImporter`.
  - `import.file` accepts only STEP/IGES (`commands.rs:1770-1779`).
  - The evaluator rejects mesh imports with `Unsupported` (`gusset-eval/src/lib.rs:290-296`).
  - gusset-meshio has the parsers, but no command uses them.
- Scenario: every front end (GUI, CLI, MCP) returns "importing is not available in this session". The Phase 2 "import STEP, IGES, STL, 3MF, OBJ" requirement is unmet, and there is no ADR or PROGRESS deviation entry.
- Fix: plug the sandboxed worker in as `Importer` in the app, CLI and bridge. Add mesh import (a tier-3 mesh body; display and export at minimum). Record the gap in PROGRESS until then.

**H6. Closing can silently drop agent edits and running work. [verified by code path]**
- Code: `handle_close` (`app.rs:736-747`) decides from `self.snapshot.dirty`. That value is stale while `try_with_session` returns `None` (a job is running) or before the change event is polled. `shutdown()` then drains queued jobs and calls `p.close()`, which deletes the recovery file.
- The same happens after "Save and close": `close_after_save` closes without re-checking dirty. An agent edit committed between the save job and shutdown is lost.
- There is no "an operation or agent transaction is still running: wait / cancel / cancel close" dialog, which the spec requires.
- Fix:
  - After `host.shutdown()`, check `session.dirty` on the returned session. If dirty, do not `close()`: snapshot instead, or re-open the dialog.
  - Detach agents before the final dirty check.
  - Add the running-operation dialog.

**H7. Persistent-naming golden tests required by ADR 0003 and the spec are missing. [verified]**
- Only parameter changes and sketch-loop rotation are covered (`gusset-naming/src/lib.rs` tests).
- There are no document-level tests for reordering independent features, branching then editing, or cherry-picking onto another branch.
- The OCCT golden suite did not run here (0 tests without OCCT).
- Fix: add document-level golden tests in gusset-eval / gusset-commands that work with the mock kernel (resolving references after `feature.move`, `branch.create` + edit, and `branch.cherry_pick`), and the same tests in the OCCT suite.

**H8. Deleting or reordering a body feature silently retargets later add/cut/intersect features that use the default target. [verified]**
- Code:
  - `FeatureDelete` contains a dead check: `if let Some(u) = later_default { let _ = u; }` (`commands.rs:983-994`).
  - `combine()` uses "the latest body" when `body` is `None` (`gusset-eval/src/lib.rs:356-361`).
  - `validate` and `feature.move` do not model this implicit dependency.
- Scenario: Extrude1 (body A), Extrude2 (body B), then Extrude3 `op: cut` (cuts B). The user deletes Extrude2. The cut now silently applies to A and changes the part without any error.
- Reproduction: `repro/tests/commands.rs::delete_retargets_implicit_body`. The delete is accepted and Extrude3 now targets Extrude1. The mock kernel cannot do booleans, so the geometric effect was shown by code path.
- Fix: resolve `body: None` to an explicit body name when the feature is created (store it in the document), or make "latest body" a real dependency that `validate` checks. Remove the dead code.

### Medium

**M1. Startup housekeeping is never run, so retention and GC are never enforced.**
- `housekeeping::run` has no caller outside gusset-store, so orphan temp files, stale locks and orphan cache/LKG entries are never cleaned.
- `History::gc` is never called. Soft-deleted branches are never purged (the 7-day retention is not implemented in practice), history grows without bound, and there is no "compact history" or Settings → Storage UI.
- `grep` confirms both. All of these are Phase 2 ("storage hygiene, complete").
- Fix: run housekeeping at app start (after H1's recovery scan). Add compact-history and storage UI that run `gc`.

**M2. Save modes are not implemented.**
- There is no auto-save mode, no global or per-project setting, and no "after N changes" trigger. Only a fixed 30 s recovery interval exists (`app.rs:31`; the ADR says 60 s / 20 changes).
- There is no OS logout/shutdown handling.
- Not listed under deviations.

**M3. Each save and each 30 s recovery snapshot recompresses and re-verifies the whole history on the model thread.**
- `gusset-project::encode` builds a new `ContainerWriter` and `add`s (zstd-compresses) every object. `ContainerWriter::copy_from` exists but is unused (ADR 0005: "unchanged records are copied byte-for-byte").
- `save_project` then fully decodes the file 4–5 times (verify new bytes, verify old for LKG, LKG read-back verify, project read-back verify).
- With a 1 GiB imported STEP blob, every snapshot blocks all GUI and agent commands for seconds. Budgets (`save_1000`, recovery < 100 ms, "never blocks the UI") are unmeasured.
- Fix:
  - Keep the opened `Container` (or the compressed records) and use `copy_from`.
  - Encode on a worker thread from an immutable `History` clone (the history is `Arc`-based).
  - Benchmark.

**M4. The sketch solver is dense O(n³) up to 1000 unknowns per component and runs several times per edit on the model thread. [verified]**
- `HARD_MAX_COMPONENT_PARAMS = 4000`, default 1000 (`gusset-sketch/src/solver.rs:15-53`).
- Measured (release build): 400-segment chain → 4.2 s per solve.
- Each sketch command solves at least three times: `store_solution`, `commit` → `evaluate_doc`, and `sketch_status` → `evaluate`. That freezes agents and queues GUI commands for 10+ s on a moderately complex sketch.
- Fix: sparse factorization, or a smaller default component limit with a clear error; reuse the solve from `store_solution` in evaluation.

**M5. The viewport is CPU-rasterized via the egui painter on the UI thread, with no BVH picking.**
- `viewport.rs:236-300` projects every triangle and sorts them back to front each frame. `pick()` (`viewport.rs:156`) is a linear scan over all triangles on every hover.
- This diverges from ADR 0009 (gusset-render offscreen wgpu texture) and the spec (wgpu viewport, CPU-side BVH picking, LOD while orbiting).
- `SceneRenderer` exists but is used only for `render_view`.
- On a 1M-triangle part every mouse move is O(T) and every frame is O(T log T) on the UI thread.
- Measure tool missing: "measure" exists only as `query.body`; the viewport has no distance/angle measure tool.

**M6. Import (once wired) runs synchronously on the model thread and cannot be cancelled.**
- `ImportFile::run` reads up to 1 GiB and calls the importer inside the command job (`commands.rs:1781-1789`). Every other command waits behind it.
- The spec requires imports to run in the background, be cancellable, and never block.

**M7. The bulk-change approval threshold counts features only.**
- `document.set_text` counts changed features but not parameters (`commands.rs:282-286`). An agent can rewrite 200 parameters in one call with no approval [verified: `set_text_bulk_threshold_ignores_params`]. The spec and ADR say "features or parameters".

**M8. Undoing a branch deletion (or branch creation) does not check name collisions. [verified]**
- `History::apply` restores `deleted_at = None` without the rename logic of `restore_branch` (`gusset-history/src/history.rs:731-743`).
- Result: two live branches named "Iteration 2" (`repro ... undo_delete_branch_duplicates_names` prints `["Main","Iteration 2","agent/bot-1","Iteration 2"]`). `find_branch` / `branch_by_name` then pick an arbitrary one. Branch tools address branches by name, so the wrong branch can be switched, renamed or deleted.
- Fix: share the `restore_branch` collision handling.

**M9. Inserting a feature while rolled back records two undo events.**
- `edit()` commits, then records a separate `SetRollback` (`commands.rs:73-81`). One Ctrl+Z moves only the marker and leaves the inserted feature.
- Fix: commit the marker move in the same transaction (one event).

**M10. The evaluation cache is bounded by entry count, not memory.**
- `DEFAULT_CACHE_ENTRIES = 4096` states, each holding `Arc` B-rep bodies (`gusset-eval/src/lib.rs:108-140`). Every intermediate shape of every edit stays alive (OCCT shapes plus triangulation caches), so memory is effectively unbounded during long sessions.
- The session-level cache is single-entry, so a GUI and an agent on different branches evict each other and re-run the incremental walk.
- Fix: a byte-estimated LRU; keep cached states per branch head.

**M11. Seccomp is a deny-list, contrary to ADR 0008 ("seccomp-bpf allow-list"). [verified by reading]**
- `gusset-sandbox/src/lib.rs:105-125`. The new aarch64 change removes `fork`/`vfork` there and relies on the `execve` denial, and `clone` is allowed.
- Neither the ADR nor THREAT_MODEL documents the deny-list decision.
- Fix: an ADR amendment, or move to an allow-list.

**M12. `unsafe` code in `gusset-kernel-occt`, which ADR 0008's allow-list omits.**
- `crates/gusset-kernel-occt/src/ffi.rs:12` has `#![allow(unsafe_code)]` with 25 unsafe blocks.
- ADR 0008 lists occt-sys, store, hw, sandbox and app. ADR 0002 says kernel-occt is "safe Rust".
- The SAFETY comments are present and look correct. The issue is conformance plus Miri/review coverage.
- Fix: move these wrappers into gusset-occt-sys, or amend ADR 0008.

**M13. GUI file I/O blocks the UI thread.**
- `App::open` runs `Project::open` (read, full decode, recovery decode, lock) on the UI thread (`app.rs:632`).
- `maybe_snapshot(true)` and `wait_idle` call blocking `exec`.
- For large projects the window freezes, contrary to "the UI never blocks".

**M14. Missing required test categories.**
- No process-kill crash tests (ADR 0007 promises a helper binary).
- No dm-flakey / dm-log-writes CI.
- No GUI automation for the close/recovery/save-failure matrix. `close_with_unsaved_changes_asks_first` sets `dialog` directly instead of sending a close request, so it never exercises `handle_close` (vacuous).
- No screenshot tests and no `docs/screenshots/`.
- No benchmarks (`rebuild_100`, `history_overhead`, `save_1000`) and no CI regression gate.
- No migration fixtures (`tests/fixtures/format/` does not exist).

### Low

- **L1.** The `paths.rs` test `rejects_escapes` asserts `r.is_err() || !bad.contains("..")`. This passes vacuously for `/etc/passwd` and `C:\x` (`gusset-commands/src/paths.rs:55-58`). Assert `is_err()` for every case.
- **L2.** Export writes use `~$export.<nanos>.gtmp`. That name does not match `temp_stem` (16 hex digits), so housekeeping never cleans it. It also bypasses `durable_write` (no directory fsync, no read-back), against the spec's "one audited durable_write that everything uses" (`paths.rs:43-63`). There is also a TOCTOU window between `confined()` and the write, which a local attacker could use with a symlink swap.
- **L3.** `corner_edge` names every vertex-blend face after the globally smallest edge name, relying on `#k` geometric disambiguation (`gusset-naming/src/lib.rs:163-167`). Corner-face references are fragile. The geometric key uses raw world XYZ lexicographic order, not the sketch-frame projection that ADR 0003 describes.
- **L4.** Cap face names gain a region suffix as soon as a sketch has more than one region (`naming/src/lib.rs:94-98`). Adding an unrelated closed loop to a sketch used by an all-regions extrude renames `cap(end)` to `cap(end, l1)` and breaks downstream fillet references [suspicion; traced, not reproduced]. ADR 0003 golden tests require this case to survive.
- **L5.** Legacy- and 2026 elicitation approvals are answered by the MCP client itself. A malicious local client holding the token can self-approve. Consider preferring the in-app prompt when the GUI is running (design note).
- **L6.** HTTP stateless requests without client info all share the agent name `agent-<n>` per client name. After an app restart, counters reset and new connections inherit old `agent/<client>-1` branches.
- **L7.** The GUI overwrites existing export files without confirmation (`"overwrite": true` is hard-coded, `app.rs:1338`).
- **L8.** `Evaluator::evaluate` clones the whole `State` (including the growing `statuses` vector) per recomputed or rolled-back feature, which is O(n²) in the feature count. Negligible at 100 features; it matters at thousands.
- **L9.** Locks live in the per-user data dir, so two users on a shared network folder can both open the same project (ADR 0007 accepted this; worth a THREAT_MODEL note).
- **L10.** `docs/PROGRESS.md` "Next" still lists the GUI app, the worker, release, Miri/fuzz and others as to-do, while they exist. It does not list the gaps in H5, M1, M2 and M5. The status doc is stale.

## 3. Phase 2 requirement table

Legend: ✅ implemented · 🟡 partial · ❌ missing. ⚠ = no ADR or "Deviations from spec" entry covers the gap (counts as a finding).

| Requirement | Status | Evidence / notes |
|---|---|---|
| Sketch geometry: lines, arcs, circles | ✅ | `gusset-sketch`, `sketch.add_line/arc/circle/rectangle` |
| Constraints (coincident, parallel, perpendicular, tangent, H/V, equal, distance, angle, radius) | ✅ | `eval/src/sketch.rs:116-133`; plus on_line, midpoint, symmetric, fixed, diameter |
| Show DOF, flag over-constrained | ✅ | `SolveResult.dof/redundant/conflicting`; `query.sketch`; MCP sketch status |
| Extrude add/cut | 🟡 | Works; implicit-target retargeting (H8) |
| Revolve, fillet, chamfer | ✅ (mock + OCCT, OCCT untested here) | `eval/src/lib.rs` |
| History tree: edit any feature, rebuild, clear failure messages | 🟡 | Structured `EvalError` with hints; edits are via code view/commands; broken nodes recorded |
| Named parameters & expressions | ✅ | `gusset-units::params` |
| Code view of the same model | ✅ | `document.get_text/set_text`, GUI code panel |
| Export STEP/STL/3MF | 🟡 ⚠ | CLI and MCP OK; **GUI export broken (H3)** |
| Import STEP, IGES | ❌ ⚠ | Command exists, no importer wired (H5) |
| Import STL, 3MF, OBJ | ❌ ⚠ | Parsers exist; no command, eval rejects (H5) |
| Units mm default, inch, `10 mm + 0.5 in` | ✅ | `gusset-units` tests |
| Viewport orbit/pan/zoom/selection | 🟡 ⚠ | CPU painter, not wgpu; no BVH (M5) |
| Viewport measure | ❌ ⚠ | Only `query.body` (M5) |
| Headless CLI `build --export` | ✅ | `gusset-cli/src/main.rs` |
| Timeline with rollback marker | 🟡 ⚠ | Command OK; **GUI slider broken (H4)**; no drag/reorder UI |
| Branching iterations | 🟡 | Commands complete; GUI has a lineage list only; compare overlay/side-by-side ❌ |
| Non-destructive undo tree | ✅ | `gusset-history` (with M8, M9) |
| Save modes (auto/manual) | ❌ ⚠ | M2 |
| Crash recovery (two-slot file, startup prompt) | 🟡 | Store layer good; app flows lose data (C1, C2, C4, H1, H2, H6) |
| Durable write, LKG, damage fallbacks | ✅ | `gusset-store` (well tested, 4000-iteration FaultFs) |
| Storage hygiene: housekeeping, cache cap, GC, Settings → Storage | ❌ ⚠ | Code exists, never invoked (M1) |
| Kill / power-loss tests | 🟡 ⚠ | FaultFs yes; process-kill and dm-flakey no (M14) |
| MCP: rmcp, dual protocol revision | ✅ | rmcp =3.5.1, legacy + 2026-07-28 |
| MCP localhost HTTP in the GUI + stdio bridge | ✅ | `http.rs`, `bridge.rs` (C2) |
| Command parity GUI = MCP from one registry | ✅ | `registry()`; but GUI payload drift (C4, H4) shows no schema test of GUI calls |
| Resources (tree, params, sketch status, mass, bbox, errors, diagnostics) | ✅ | `server.rs:resource_list` |
| `render_view` PNG | ✅ | `scene_renderer.rs` |
| Agent edits undoable & live in GUI | ✅ | host events → GUI |
| Agent branches by default | 🟡 | Bypass C3 |
| Branch tools (create/rename/switch/compare/cherry-pick/set main/delete+confirm) | ✅ | commands.rs |
| Structured actionable errors | ✅ | `CommandError{code,message,hint}`; fillet-too-large message |
| Approvals, rate limit, audit log, read-only, kill switch | 🟡 | Present; bypass C3, M7 |
| Persistent naming golden tests (branching/reorder/cherry-pick) | ❌ ⚠ | H7 |
| Benchmarks / PERFORMANCE budgets measured | ❌ ⚠ | No benchmarks at all |

## 4. ADR conformance

- **ADR 0008 unsafe list:** violated by `gusset-kernel-occt` (M12). The other crates match (occt-sys, store/win.rs, hw/sys/*, sandbox/win.rs). gusset-app is listed but contains no unsafe code; that is fine.
- **ADR 0008 seccomp allow-list:** deny-list in code (M11).
- **ADR 0005 / FORMAT.md:** the reader enforces the documented layout (contiguity, index match, duplicate hashes, limits, trailer). The format matches FORMAT.md. Copy-through of unchanged records is not used (M3). Migration fixtures are missing (M14).
- **ADR 0007:**
  - Recovery interval 60 s / 20 changes → code uses 30 s and no change count (M2).
  - Housekeeping not run (M1).
  - "Keep both" semantics differ (C4).
  - Process-kill tests missing.
- **ADR 0009:** viewport not via gusset-render texture (M5).
- **ADR 0012:**
  - The policy hole in C3.
  - The bulk threshold ignores parameters (M7).
  - Agent branch name is `agent/<client>-<n>` as documented.
  - Origin/Host/token/constant-time compare: implemented correctly (`http.rs:205-256`).
  - Instructions under 2048 characters.
  - `user_text` labelling and sanitizer: present.
- **ADR 0003:** golden tests missing (H7); the geometric key differs (L3).

## 5. Review of 692a265..d2d0b5b

- **`gusset-store/src/lock.rs` (Windows `.lock.info` sidecar):** functionally fine. On a crash the sidecar stays until the next acquire overwrites it or `release` removes it. Housekeeping only lists `*.lock` (the sidecar is not orphan-tracked), but `remove` happens via `release` after a stale takeover, so that is acceptable. It is written with `std::fs::write` (follows symlinks) inside the owner-only `locks` dir; low risk. Display-only, so no correctness impact. The test now uses `holder_info`, which is good.
- **`gusset-occt-sys/build.rs` RPATH:** an absolute build-prefix RPATH, but it applies only to this crate's own test targets (`rustc-link-arg` does not propagate). The release workflow checks staged binaries for non-`$ORIGIN` RPATHs (`release.yml:137-150`). OK.
- **`gusset-sandbox` aarch64 gating:** correct (no `SYS_fork`/`SYS_vfork` on aarch64). It highlights that the filter is a deny-list (M11). `clone`/`clone3` with namespace flags are not restricted; Landlock and the `execve` denial remain the real barriers.
- **`gusset-hw/src/cpu.rs` cfg on `clean_string`:** fine (dead-code gating).
- **`deny.toml` font exception:** scoped to `epaint_default_fonts` and documented in ADR 0001. OK.

## 6. Prompt-injection check

No injection attempts were found in the files, comments, fixtures or tool output read during this review.
