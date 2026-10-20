# Progress

Living status document. Any session should be able to resume from here and CLAUDE.md.

## Current phase
Phase 2 is done: `v0.1.0-mvp` was tagged on 2026-10-12 on commit cfc0e35 (CI and
Fuzz + Miri green). The owner published it on GitHub as a pre-release (the session's git proxy
refuses tag pushes); the tag triggers the release workflow after the milestone review, the re-review and two dogfooding runs.
Phase 3 has started (see Next).

## Done
- Research and design docs: `docs/RESEARCH.md` (+5 reports), `docs/DESIGN.md`, threat
  model, SECURITY.md, HARDWARE.md, PERFORMANCE.md (budgets), INTEROP.md, FORMAT.md;
  ADRs 0000–0015.
- Crates (see CLAUDE.md): units, sketch solver, kernel + mock, OCCT shim + safe kernel,
  naming, doc (code view), history, eval, store, project, commands (registry, ~40
  commands, agent policy), meshio, hw, sandbox, worker, render (wgpu + CPU), host,
  mcp, app (egui GUI), update (signed manifests), cli.
- End to end with real OCCT 8.0.1: CLI build/export, `gusset self-test`; GUI renders the
  bracket example; an agent built 5 reference parts through MCP with exact volumes
  (docs/DOGFOODING.md).
- GUI: menus/toolbar/command search generated from the registry, schema-generated
  command dialogs, browser, timeline, code view, close/recovery dialogs, AI agents panel
  with approvals, audit log and agent branches; kittest automation; screenshot gallery
  in docs/screenshots/.
- MCP: rmcp server (HTTP on localhost with token + stdio bridge), tool parity with the
  registry, `render_view`, agent branches, approvals, read-only mode, persisted audit.
- Standing "always allow" approvals from the in-app prompt (per action / all actions,
  optionally remembered), revocable in the Agents panel, suspended by the kill switch
  (ADR 0012 "Standing approvals"). Milestone security fixes merged (MCP, sandbox,
  storage, app); see docs/SECURITY_REVIEWS.md.
- CI: fmt/clippy/test on Linux x64/ARM64, Windows x64/ARM64, macOS ARM; cargo-deny,
  cargo-audit, license inventory, native OCCT builds + kernel tests, fuzzing (11
  targets) and Miri; release workflow (packages, SBOM, attestations; signing off).

## In progress
- Dogfooding friction fixes for agents (F-M1/M2/M10/M11/M14/M15/M17/M18/M19, polyline
  tool) on a worktree branch.
- IGES import: OCCT reads IGES only from a file, so the sandboxed worker gets a
  Landlock read rule for one private temp file (ADR 0008 §4); in a worktree branch.

- GUI phases 1 and 2 of the Fusion-style layout (ADR 0023 §4, DESIGN.md §2): title bar,
  SOLID/SKETCH tabs, ribbon with dropdown groups, view cube and navigation bar (phase 1);
  browser tree with collapsible icon categories (Document settings, Origin, Bodies,
  Sketches, Features, Construction, Parameters, Branches), timeline icon chips with status
  badges and playback buttons that move the rollback marker, and a command line above the
  status bar (View menu toggle; registry id plus JSON or `key=value` args through
  `submit_command`, Tab completes ids, Up/Down recall) (phase 2). Bottom tidied after
  owner feedback: one timeline bar (playback, chips in a recessed track, graph toggle),
  command line hidden by default (View menu, Ctrl+Shift+K), confirmations fade after 5 s.
  Origin, bodies and datum planes are read-only in the browser (the scene has no
  visibility for them yet). Still to do: a properties panel instead of the floating
  command form, the SURFACE/INSPECT tabs, the SELECT group, and a wireframe view style
  (gusset-render has no wireframe mode; the navigation bar toggles edges instead). The
  screenshot gallery in docs/screenshots was not regenerated (it needs the real kernel
  for the bracket model); phase 2 looks were checked with mock-kernel renders.

## Done for v0.2 after its reviews (2026-10-13)
- Code and security review findings H1-H3, M1-M5, L1-L11, V1-V8 fixed.
- Dogfooding run 3 (docs/DOGFOODING.md): all 15 findings closed (the render "ticks"
  of F3-15 were real edges of the test model). New: `param_rename`, horizontal/vertical distance and position
  constraints, expression positions for circles and rectangles, orbit camera, paged
  large results, cancel = no effect, gear rebuilds 4x faster.
- Development: Codex offloading for simple tasks and the secret guard in CI and
  pre-push (ADR 0021).

## Done since the milestone review (2026-10-11)
- Security, storage, sandbox, MCP and GUI review fixes merged (docs/SECURITY_REVIEWS.md).
- Standing "always allow" approvals (ADR 0012).
- App: start-up routine (housekeeping, cache cap), "recover unsaved work" offer,
  SaveScheduler-driven auto-save/recovery, imported meshes drawn, cancellable imports.
- Naming: document-level golden scenarios with mock and OCCT kernels (H7); L3, L4 fixed.
- Solver: envelope Cholesky + Givens rank analysis, 19x faster on long chains (M4).
- Evaluation cache bounded by estimated memory, LRU, per-branch session cache (M10).
- History: snapshot schema 2 shares feature lists; ~370 B per parameter edit.
- Budgets measured: save 33 ms, snapshot 34 ms, rebuild_100 3.0 s / 0.12 s.
- CI green on all platforms (the GL backend is no longer used on Windows, ADR 0009).

## Next
1. Phase 3, in the feature-matrix order (docs/RESEARCH.md "v1" tier), starting with
   the modeling gaps agents and users hit first (dogfooding F-M4..M8):
   - v0.2: done: shell, mirror, linear/circular body patterns (OCCT golden tests;
     agent ergonomics F2-5..F2-11), offset/angled datum planes, body move/copy and
     combine (a single feature is patterned as its own body, then combined into the
     part; OCCT golden tests), sketch region arrangement (crossing curves form regions),
     `sketch.trim`, spline curves (centripetal Catmull-Rom through ordinary points) and
     sketch patterns (ADR 0016; OCCT golden tests in `modeling_sketch.rs`), sweep and
     loft (ADR 0017: profile along a sketch chain or edges, fixed/Frenet, twist; loft
     through sketches on datum planes; persistent names; OCCT only, golden tests in
     `modeling_sweep.rs` and the naming golden), holes (ADR 0019: `hole` statement and
     `feature.hole`; simple, counterbore, countersink and tapped with a cosmetic thread record
     in `query_body`; blind with drill point or through; `standard` presets, ISO 273 / 261-262 /
     2306 / 4762 + DIN 974-1 / 10642, written as named parameters; names `Hole1/wall(p5)`;
     OCCT golden tests in `modeling_hole.rs` and the naming golden `hole_names`). Next:
     DIN 974-1 / ISO 4762 / 10642 table values to be checked against the standards (ADR 0019 §5).
   - v0.2: generators (DESIGN.md §1, owner request): `generate.gear` (ADR 0018) writes
     named parameters, a parametric tooth sketch and an extrude/sweep for involute spur
     and helical gears (OCCT golden tests in `modeling_gear.rs`: volume 6e-6, tooth
     thickness, meshing). It is a command and a form but deliberately no text statement
     (ADR 0018 §6). `generate.spring` done (ADR 0018 amendment: round wire, twisted sweep up Z, exact volume; OCCT test). `generate.thread` done (ADR 0034: exact helical sweeps in the kernel, `path: helix(...)`; ISO 68-1 external thread on a rod, OCCT test of the exact volume, M10 x 20 in 0.9 s). `generate.nut` done (hex nut with an internal thread cut along the helix, run a pitch past both faces; OCCT exact-volume test, 0.7 s). Bolts: `generate.thread` with `head_width`/`head_height` adds a hex head. Thread ends are chamfered 45 deg from the minor diameter (`chamfer`, default on). Socket head cap screws: `socket` (hex socket across flats, `socket_depth`) makes a round head with the socket cut from its bottom face (ISO 4762 M10 exact-volume OCCT test). Next generators: thread runouts; gear gaps: internal
     gears, racks, bevel/worm, standard-centre-distance shifts.
   - v0.3 first: load resilience (owner request 2026-10-13, DESIGN.md §1 "Heavy or
     impossible work never crashes the app"): kernel work in a supervised worker process,
     memory/time budgets per rebuild, cancel inside OCCT operations, fault-injection tests.
     Done: worker process, crash containment and restart, evaluator crash handling,
     app/MCP/CLI wiring and packaging, Stop that kills a running kernel operation, MCP
     deadlines, a Stop button, rebuild progress (ADR 0022 "State").
   - v0.3: parallel feature evaluation across kernel workers (ADR 0024): done. Phase 1
     dependency keys (independent parts stay cached), phase 2 a pool of kernel workers
     with speculative builds replayed in document order (gear pair cold rebuild 6.2 s ->
     3.7 s). Phase 3 (tessellation on the pool) only if measurements call for it.
   - OCCT translated to Rust, owner request (ADR 0025): opencascade-rs and occt-rs are
     bindings, not ports (LGPL / AGPL); pilot translation of `gp` + `ElCLib` by Codex,
     judged bit for bit against the C++ original by oracle programs; the LGPL port may
     only link into the kernel worker. Pilots done (gp subset, ElCLib, BSplCLib curve
     evaluation), math linear algebra, all of gp, ElSLib and the rest of ElCLib, PLib, BSplSLib (and the BSplCLib parts it needs); CSLib / Convert next; now translating TKMath package by package with Codex (GPT-6 Luna,
     high effort): oracle job, translation job, review. Status: crates/occt-port/STATUS.md.
     2026-10-14: Codex reached its usage limit (until 2026-10-24) and Gemini CLI still
     refuses the account's tier, so math (linear algebra) and gp 3D continue with Sonnet
     subagents under the same judge rules; Codex takes the simple packages again after.
   - v0.3: joints (ADR 0026, after research on Fusion, Onshape, SolidWorks, FreeCAD):
     phase 1 done: `joint` feature (rigid, revolute, slider, cylindrical, pin_slot, planar,
     ball) placing a body (and carried bodies) frame-to-frame; frames on planar faces,
     revolved faces (axis from the kernel's new topology fields), circular/straight edges,
     planes and the origin; flip, offset, limits; values are expressions so parameters
     drive motion; `feature.joint` command (GUI/CLI/MCP), grammar docs, mock and OCCT tests
     (shaft in a bore); `query.joints` (values, limits, DOF left, status, unplaced bodies),
     the ASSEMBLE ribbon group and a Joints browser category; `query.interference`,
     `study.motion` and `query.bom` (phase 2 part, OCCT test); relations are expressions.
     Components from other documents (`component`, `assembly.insert_component` /
     `update_component`, pinned copies, `part(...)` face names; mock and OCCT tests).
     Parameters per component (`set:`, `assembly.configure_component`).
     `query.components` reports available updates; the app names them after opening.
     Closed loops: `free: [...]` values solved at the joint that closes a loop (four-bar
     test, OCCT four-bar golden). Three-way `branch.merge` (ADR 0006); the agents panel's Apply merges.
     Customizer and configurations (ADR 0027): parameter ranges (min/max/step), browser
     sliders, named configurations (save/apply/query, per component).
     The joint form takes its frames from viewport picks (first pick: moving body and
     `at`, next: `to`). v0.3 milestone code review (docs/reviews/2026-10-14-v0.3-assemblies-review.md):
     every high, medium and low finding fixed but N1. The v0.3 security review (2026-10-15)
     and dogfooding run 4 are done and their findings fixed (docs/DOGFOODING.md "Status of
     the run 4 findings").
   - v0.4 drawings (ADR 0032, research in docs/research/drawings.md): phases 1 and 2
     done. `drawing` blocks in the document (sheets, base and projected views, driven
     dimensions on edge names, notes) stored in history, compared and merged whole;
     `gusset-drawing` builds one scene (OCCT exact hidden lines, cached per view, ISO 128
     line styles, ISO 7200-style title block with the projection symbol, centre marks and
     centrelines, ISO 129 dimensions measured in the view plane, dangling dimensions
     reported) and writes SVG (mm) and DXF R2000 (default, `$INSUNITS` mm, line weights)
     or R12; commands `drawing.*` and `query.drawing`; OCCT test `modeling_drawing.rs`.
     Phase 3 done: the DRAWING tab (sheet painted in the app, pan/zoom, dragging views,
     notes and dimension texts, `drawing.move_item`) and `render_view` with `drawing` (a
     CPU raster PNG for agents). Phase 4 done: full sections (`section`, `cut_at`; cut with
     a half-space box, hatched from the mesh), circular details (`detail_of`), PDF export
     (own writer), `sketch.import_dxf` (own fuzzed reader). Milestone code review
     (docs/reviews/2026-10-19-v0.4-drawings-review.md): every finding D1-D14 and nit N1-N6
     fixed. Security review (docs/reviews/2026-10-19-v0.4-security-review.md): X1-X8 fixed
     but X6's off-thread DXF import (accepted). Dogfooding run 5 (docs/DOGFOODING.md): every finding fixed (dual dimensions done too: `dual: true`). Parts list done (`parts_list`, `drawing.add_parts_list`, the same grouping as `query.bom`). Next for drawings: deferred items
     (none). Half and offset sections done (`half: true`, `steps: [[at, cut_at], ...]`). GD&T done (`gdt` frames with the 14 ISO 1101 symbols drawn as lines, `datum` symbols, `drawing.add_gdt` / `add_datum`). Balloons done (`balloon`, `drawing.add_balloon`: the part's item number, leader to its centre).
   - v1 tier: `draft` done (ADR 0028: faces tapered about a neutral plane, pull direction
     from the plane or into the body, flip; OCCT golden tests in `modeling_draft.rs`).
     `text` done (ADR 0029: one line embossed or engraved on a plane or face, the app's two
     fonts through `gusset-text`/skrifa; OCCT tests in `modeling_text.rs`). `offset_face` done (ADR 0030: push/pull faces
     along their normals, holes change radius; OCCT tests in `modeling_offset.rs`). `move_face`
     done (ADR 0031: translate/turn faces rigidly, the faces around keep their surfaces and
     stretch; holes and bosses slide; OCCT tests in `modeling_move.rs`). Next in the tier:
     FreeCAD/OpenSCAD import, Parasolid XT.
   - v0.4: 2D drawings, DXF/SVG; FreeCAD/OpenSCAD import, Parasolid XT.
   - later: sheet metal, direct modeling, tier-2 native imports, CAM, simulation,
     plugins.
   Every milestone goes through the reviewer, security reviewer and dogfooding.
2. Post-MVP leftovers: dm-flakey power-loss tests on a real block device (M14; `FaultFs`
   covers reordered and torn writes in memory, a CI runner with the dm_flakey module is
   needed). Agent keys (F2-2) and saves off the model thread are done.
3. Release: v0.1.0-mvp is published (pre-release, tag at e3ee7e0, run 38271195031):
   AppImage/deb/tar.gz for Linux x64+arm64, MSI/zip for Windows x64+arm64, dmg for
   macOS arm64, SBOMs, OCCT source, SHA256SUMS, update manifest. Releases can be
   started by tag name through workflow_dispatch (ADR 0013 amendment). Unsigned
   (ADR 0015); no update signing key yet, so the in-app updater refuses the manifest.

## Known gaps (not MVP scope per the spec; planned for later phases)
- Feature-level patterns (patterns repeat whole bodies; combine merges one feature's body
  into the part) (dogfooding F-M4..M7). Sketch-level patterns exist (ADR 0016).
- Sketch arrangement limits (ADR 0016): curves that overlap along a stretch are an error;
  a spline's tangential touch with another curve is not detected; splines cannot be
  trimmed; pattern copies cannot carry constraints or be trimmed; no patterns of patterns.

## Decisions made without the owner (easy to reverse)
- **Branch workflow:** the session may only push to `claude/new-session-nedlj5`. That is
  also the repository's default branch, so it plays the role of "main". Feature
  branches are merged locally.
- **Repository visibility:** made public by the owner on 2026-10-11 (Actions minutes
  had run out). The OCCT build is still cached per target.
- The repository is named `CAD`, not `gusset`. Renaming is left to the owner.
- History-overhead budget restated per edit (≤ 450 bytes); a fixed fraction of the
  model size cannot hold for small models (PERFORMANCE.md).
- Performance budgets are enforced by release/OCCT tests instead of criterion benches
  (no new dependency).
- MSRV 1.89 for libraries (std file locks, rmcp); the GUI crate needs 1.95 (egui 0.36).
- Sketch coordinates stored in the document are snapped to 1e-6 mm when they are within
  1e-9 mm of the snapped value, for a readable code view.
- Parametric sketches after large edits (v0.2 review H1, ADR 0016 §5): `fixed` unknowns are
  held during the solve and a failed solve retries by dimension continuation; `param.set`
  does not write solved coordinates back (would turn parameter edits into sketch edits and
  cause branch-merge conflicts).
- Generated gears are re-checked on every rebuild through an explicit `gear: <prefix>` key
  on the profile sketch (v0.2 review M1, ADR 0018 §3), not by parameter naming conventions.
- The `doc_text` fuzz seeds live in `fuzz/seeds-src/` and grammar.md, copied by
  `make_seeds`; `fuzz/corpus/` stays CI-managed and untracked (v0.2 review L10).
- Container: a custom checksummed format instead of SQLite (ADR 0005). Two-slot
  recovery file with an extra slot marker; the Windows cache path follows the
  `directories` crate layout.

## Deviations from spec
- Right-to-left text rendering is a known egui gap (ADR 0009). Strings are
  externalized; RTL rendering waits on upstream.
- Mesh import (STL/3MF/OBJ, tier 3) produces **reference mesh bodies**, not solids: the
  kernel cannot build a B-rep from a triangle mesh. They are stored as binary STL blobs,
  displayed (`render_view`; the GUI viewport still has to draw `Evaluation::meshes`),
  listed by `query.model` (`reference_meshes`, with volume when watertight) and included
  in mesh exports; STEP export, booleans and edge/face references do not apply to them.
  Mesh files are parsed in-process by gusset-meshio (pure Rust, with size/count limits),
  off the model thread; STEP/IGES go to the sandboxed worker (ADR 0008 §4).

## Research gaps
- Reddit, and direct fetches of the FreeCAD and Onshape forums and HN, were blocked from
  this environment. User research relied on search-engine snippets of those sources.
  Re-run when access allows.
- WebFetch could not open vendor pages; some HARDWARE.md entries are marked unverified.
