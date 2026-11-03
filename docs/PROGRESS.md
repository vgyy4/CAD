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
  Bodies have an eye in the browser (hidden bodies are neither drawn nor picked, and stay
  hidden across rebuilds). Origin and datum planes are drawn as translucent squares over the
  model with eyes in the browser (origin planes start hidden); a click on a shown plane where
  no body is under the cursor picks it, and a picked plane fills a form's `plane` (sketches)
  or a joint's `at`/`to`.
  Sketch geometry placed with viewport clicks (`sketch_place.rs`): while a sketch is edited,
  the line, rectangle, circle, arc and polyline tools take their points from clicks on the
  sketch plane (snapping to sketch points within 8 px, else rounded to 0.01 mm), with a
  rubber-band preview; the filled form is submitted like a typed one, the tool stays on, and
  lines chain from the previous end point (`from_point`), so they join. A polyline closes on
  its first point or ends with OK. After the first click, typing a number opens fields at the cursor
  (`value_entry.rs`: a line's length and optional angle, a rectangle's width and height, a
  circle's radius; expressions with units and parameters, a bare number is mm); Enter places
  the point toward the cursor, Esc closes the fields.
  The edited sketch's points and curves are picked in the viewport before the model
  (`Hit::Sketch`; points within 6 px win over curves within 5 px) and highlighted; picks fill
  `sketch.add_constraint` entities, `sketch.delete_items` ids and `sketch.move_point`, and the
  Delete key removes the picked items. A left drag from a point of the edited sketch moves it
  (rubber-band preview of its lines; `sketch.move_point` on release, so the solver keeps the
  constraints). A constraint toolbar at the top of the viewport offers what fits the picked
  sketch items (`sketch_constrain.rs`: one line: horizontal, vertical, length; two lines:
  parallel, perpendicular, equal, angle; point and line: on line, midpoint, distance; ...):
  geometric constraints are added at once, dimensions open the form with kind and entities
  filled in. While a sketch is edited its constraints show as labels at their entities (ASCII
  glyphs H, V, ||, _|_, =, T, Fix, M, S; dimensions as their expression, R/D for radii).
  Editing an earlier sketch rolls the model back to just after it and ghosts the rest as faint
  edges (DESIGN.md §1.5); Finish sketch puts the marker back.
  Starting or editing a sketch turns the camera to look straight at its plane (animated,
  `NavRequest::LookAt`).
  Profile regions are picked in the viewport: while a form with `regions` (extrude, revolve)
  is open, its sketch is shown and a click inside a closed region toggles it in the field
  (`sketch_regions.rs`: outer loops chained from the region's entities, the smallest
  containing region wins, so holes work); picked regions are outlined in the selection color.
  Picks made while a form is open fill its fields live (`edges` for fillet/chamfer, `faces`
  for shell/draft/offset/move face, `plane`; a field changes only on picks of its kind).
  `query.measure` (the INSPECT group's Measure; `Body` keeps `query.body`): one face or edge
  gives area, or length and radius; two give their angle and the distances that apply
  (between parallel planes, center to plane, between parallel lines, between centers), all
  from the evaluated topology; the first two picks fill it.
  Features are edited in place (DESIGN.md §1.5): `feature.edit {name, set, remove}` splits the
  feature's one-line statement into its arguments (`gusset_doc::text::statement_args`),
  changes them, parses it back and replaces the feature keeping its name, kind and place (one
  undo); sketches are edited in sketch mode. In the app a double-click on a feature or datum
  plane in the browser, or on a timeline chip, opens its arguments as a form (a sketch's
  opens sketch mode); right-click on either gives edit, rename, suppress/unsuppress, roll back to
  here and delete, and for a feature whose rebuild broke a reference with a suggestion, "Use
  suggested ..." (`feature.replace_reference` swaps a whole face/edge reference in the statement;
  DESIGN.md §1.9). F11 (View menu too) gives a clean full-screen viewport; Ctrl+Enter in a form
  confirms and opens the command again with the same values. Cursor-local tools (DESIGN.md §1.3,
  `quick_menus.rs`): a right click in the viewport (no drag) opens a radial marking menu and S opens
  the quick bar at the pointer, both with the context tools (last command first; sketch tools and
  Finish sketch while sketching); `sketch.create` gave up its S shortcut for it. Mouse navigation
  presets (View > Mouse navigation: Gusset, Fusion, SolidWorks, FreeCAD, Onshape) and the theme
  and reduced motion are kept between runs in `ui-prefs.json` in the data folder. Select other
  (DESIGN.md §1.14): Alt+click lists everything under the pointer (the edge, every face along the
  ray through all bodies, `Scene::pick_all`, and planes) to pick from (Shift adds). Timeline
  chips reorder by dragging (`feature.move` refuses moves that break a dependency) and hovering a
  chip outlines what it uses (accent) and what uses it (blue). The command search (Ctrl+K) also
  finds features (opens the editor), parameters, window settings and recent project files (kept in
  `ui-prefs.json`; `palette.rs`). Parameters table (DESIGN.md §1.8; View > Parameters, the
  browser's "Table…" or the search): every expression editable plus a new row; Apply sends the
  changed ones as `param.set_many` (one undo step, all or nothing). Ribbon tooltips show the
  tool's icon, title, shortcut and first sentence; resting the pointer 1.2 s adds the rest of the
  description and the fields the command asks for (DESIGN.md §1.19). The browser's and the
  properties panel's widths are kept with the preferences (§1.15); holding X sees through the model
  (the wireframe style, §1.14). Live previews (DESIGN.md §1.6): while a feature or generator form has
  complete arguments, the model thread builds what it would make once typing pauses
  (300 ms, one at a time) and the viewport shows it tinted with a "Preview" badge; the
  model comes back when the form closes. `gusset_commands::preview` runs the command in the
  session's preview mode, where the single commit point (`commit_with_rollback`) evaluates
  the edited document instead of committing it (no history, no audit entry). The model
  as it was shows over the preview as accent-coloured edges, so before and after are seen
  together (no translucent faces: the renderer has no transparency). UI scale done (View > UI scale, 75 to 250 %, or Ctrl + plus/minus; kept in
  the preferences, DESIGN.md §1.22). Properties panel done (the running command's form docks at the right
  instead of floating; Close button). SELECT group done (a filter: faces and edges, faces only, edges only; it limits what viewport clicks pick). Still to do: the SURFACE/INSPECT tabs. Wireframe view style done (the navigation bar's
  Wireframe button: `ViewOptions::faces` off draws edges only, hidden ones too, on the GPU
  and the software renderer). Section view done (the navigation bar's Section button and
  a bar above it: a plane across X, Y or Z, flip, position slider; `ViewOptions::section`:
  the GPU discards the removed half, the CPU renderer clips the geometry, back faces seen
  through the cut of a closed body draw as a flat tinted cut face at the cut plane's depth (a cap pass); picking skips the removed half and the
  cut face; display only, no document change). The
  screenshot gallery in docs/screenshots was regenerated with the real kernel on
  2026-11-02 (properties panel, body eyes, wireframe button, drawing tab, section view, planes, click-placed sketch line, constraint toolbar, live preview, marking menu).

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
     DIN 974-1 / ISO 4762 / 10642 table values to be checked against the standards (ADR 0019 §5):
     needs the owner's copy (the standards are paywalled; a 2026-11-02 search found no public
     DIN 974-1 depth table; the ISO 4762 heads and DIN 974-1 diameters match known values).
   - v0.2: generators (DESIGN.md §1, owner request): `generate.gear` (ADR 0018) writes
     named parameters, a parametric tooth sketch and an extrude/sweep for involute spur
     and helical gears (OCCT golden tests in `modeling_gear.rs`: volume 6e-6, tooth
     thickness, meshing). It is a command and a form but deliberately no text statement
     (ADR 0018 §6). `generate.spring` done (ADR 0018 amendment: round wire, twisted sweep up Z, exact volume; OCCT test). `generate.thread` done (ADR 0034: exact helical sweeps in the kernel, `path: helix(...)`; ISO 68-1 external thread on a rod, OCCT test of the exact volume, M10 x 20 in 0.9 s). `generate.nut` done (hex nut with an internal thread cut along the helix, run a pitch past both faces; OCCT exact-volume test, 0.7 s). Bolts: `generate.thread` with `head_width`/`head_height` adds a hex head. Thread ends are chamfered 45 deg from the minor diameter (`chamfer`, default on). Socket head cap screws: `socket` (hex socket across flats, `socket_depth`) makes a round head with the socket cut from its bottom face (ISO 4762 M10 exact-volume OCCT test). Partly threaded rods and bolts: `thread_length` (plain shank) and `runout` (a cone where the thread fades out, default 2.5 P; OCCT test). Racks done (`generate.rack`, ISO 53 basic rack by a linear sketch pattern; OCCT exact volume). Internal gears done (`internal_rim`, ADR 0018 amendment; OCCT exact volume). Worm wheels: `generate.worm` suggests matching non-throated helical `generate_gear` arguments (`wheel`; negative helix for a left-hand worm); throated wheels are out of reach of profile sweeps (ADR 0018 amendment: they need the worm's envelope). Gear generators done (bevel gears done: `cone_angle`, a ruled loft heel to toe; worms done: `generate.worm`, 1 to 6 starts). Centre-distance shifts done (`query.gear_pair`, ISO 21771).
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
     high effort): oracle job, translation job, review. Status: crates/occt-port/STATUS.md. Mechanical cxx2rs drafts of all 32 toolkits Gusset links or could use (sources and headers) come from the GitHub workflows "OCCT drafts" / "OCCT header drafts" (no models, no secrets); Bulk translation stopped 2026-11-02 (owner): Geom2d, Geom and 21 more packages are done and judged (STATUS.md); remaining packages are translated only when a Gusset feature needs them (ADR 0025 amendment).
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
     stretch; holes and bosses slide; OCCT tests in `modeling_move.rs`). FreeCAD and OpenSCAD
     import done (`gusset-scad`, FCStd in the import worker). PLY and glTF 2.0 done (`gusset-meshio`:
     PLY ASCII/binary import and binary export; glTF `.glb` export and `.glb`/`.gltf` import with
     node transforms, metres/Y-up converted to mm/Z-up, only embedded buffers read; fuzz targets
     `mesh_ply`, `mesh_gltf`; File > Export lists OBJ, PLY and glTF). Next in the tier: Parasolid XT
     (clean-room reader; research done in docs/research/parasolid-xt.md: the public XT
     Format Reference covers schema 12 only, modern files are deltas against the V13 base
     schema whose field lists are not public; 38 public-domain samples exist in NIST's
     usnistgov/engineering-design-models (Bentley, Unigraphics, VDS; old schemas, e.g.
     SCH_901297). Plan: XT -> STEP text -> the sandboxed STEP import, ~5k lines; deferred
     behind the app work (owner 2026-11-02: "start working on the actual app").
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
- Feature-level patterns: done (2026-11-03). `pattern_linear` / `pattern_circular` take
  `features: [...]`: what each feature added to or cut from its body (after minus before, before
  minus after, recorded per feature during evaluation) is copied per instance and united with
  or cut from that body (OCCT exact-volume tests: a row of holes, a ring of bosses).
- Sketch arrangement limits (ADR 0016): a spline that runs along another curve for a
  stretch is not merged with it (lines and arcs that overlap now share the stretch, and a
  spline's tangential touch cuts both curves); a trimmed spline is refitted (close to, not exactly, the old curve); constraining or trimming a pattern copy, or patterning a pattern (a grid), explodes the
  inner pattern into ordinary entities, so its count is no longer parametric.

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
