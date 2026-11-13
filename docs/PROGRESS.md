# Progress

Living status document. Any session should be able to resume from here and CLAUDE.md.

## Current phase
Phase 2 is done: `v0.1.0-mvp` was tagged on 2026-10-12 on commit cfc0e35 (CI and
Fuzz + Miri green). The owner published it on GitHub as a pre-release (the session's git proxy
refuses tag pushes); the tag triggers the release workflow after the milestone review, the re-review and two dogfooding runs.
Phase 3 has started (see Next).

## Done
- Thread, bend and face profiles as commands (2026-11-13; ADR 0012): `feature.thread`
  (title "Face thread"; cosmetic or `modeled`), `feature.bend` (sheet metal, `line: Sketch.lN`,
  `side`, `position`), and `face: <planar body face>` as the profile of `feature.extrude` and
  `feature.sweep` (loft's `profiles` already took faces). GUI: Face thread in SOLID > CREATE
  next to Hole, Bend in SHEET METAL, new icons; a picked body face fills the form's `face`
  (hole, thread; replaces the sketch for extrude/sweep), a picked sketch line fills Bend's
  `line`. Mock-kernel command tests check text and refusals; modeled geometry stays covered
  by the OCCT tests. MCP core list 17996/18000 bytes, 580 per command (tight; the revolve axis on a body edge added 25).
- Import history choice (owner request, 2026-11-12; ADR 0042 amendment): Fusion, SolidWorks,
  Inventor and FreeCAD imports classify their feature history as perfect (the unchanged strict
  check), imperfect (read, but differs: the reason says which feature or how much) or none
  (with the reason). `import.file` `history: final | rebuild | ask` (default unchanged): rebuild
  keeps failed features suppressed with their error as a note, one undo step; ask reports the
  class without committing. The GUI asks in a modal after parsing and offers Undo after a
  "try anyway" import.
- PCB design (ADR 0047, 2026-11-12; owner request: build on the best-loved open-source
  tools instead of from scratch, research in docs/research/pcb-design.md). `gusset-pcb`
  reads and writes KiCad boards and schematics losslessly (S-expressions; KiCad libraries
  are data only, never vendored). Board: Gerber (gerber-types) and Excellon drill export,
  pick-and-place and BOM, ratsnest, DRC, board as a 3D body and STEP export with component
  models, footprint placement from `.pretty` libraries, interactive 45-degree routing round
  other nets' copper (A* plus exact clearance; vias, arc corners), zone fills with thermal
  reliefs, priorities and island removal (i_overlay), Specctra DSN export and SES import
  for an external autorouter (Freerouting). Schematic: symbols, wires, labels, buses and
  hierarchical sheets, netlist, ERC, field edits, sync to the board. Boards and schematics
  live in the document history (undoable); `pcb.*` / `sch.*` registry commands sit
  outside the core MCP list; egui PCB and Schematic tabs with net highlighting shared
  between them. Fuzz targets `pcb_read`, `sch_read`, `ses_read`, `pcb_nets`.
  Phase 3 (2026-11-13): the workflow is complete from an empty session (one end-to-end test:
  schematic from a symbol library, `sch.annotate`, ERC, board update, placement, pad nets,
  net classes, routing, a hatched zone, DRC, Gerbers/drill/BOM/placement, save and reopen):
  `pcb.set_net`, `pcb.rename_net` (merges), `pcb.net_class` (in the `.kicad_pro`, saved
  with the board), `pcb.add_zone` (solid or hatched), a simple shove in `pcb.route` (one
  blocking track of one net laid again round the new one, kept only when the DRC is no
  worse), `pcb.autoroute` running the user's own Freerouting (set in the PCB panel; never
  downloaded) with timeout and Cancel, texts plotted into Gerbers in Gusset's own stroke
  font (copper texts are obstacles); editor Zone tool, pad-net, net-class and autorouter
  panels. Next (ADR 0047): a true shove (several tracks, vias), texts in the editor's
  stroke font, a STEP of the flow's board under OCCT (covered by `pcb_import.rs` today).
- Body patterns that keep their copies apart (2026-11-11): `merge: false` on
  `pattern_linear` / `pattern_circular` makes copy `i` a body `<pattern>_copy<i>` that later
  features target; tree import `"merge": false` with copies `<name>[i]` (ADRs 0004, 0042).
- Exact (rational) B-spline sketch curves (2026-11-11), end to end: `Geom::BSpline` (degree
  1 to 9, flat knots, poles that are sketch points, weights, periodic) cut by blossoming into
  rational Bézier spans that the spline machinery (crossings, areas, windings) runs on;
  `Seg2::RationalBezier` through the kernel wire and the OCCT shim (rational
  `Geom_BezierCurve`, fine quadrature for rational swept faces); text `bspline(degree:, poles:,
  knots:, weights:) [periodic]`; sketch patterns copy them; tree import `{type: bspline}`
  (ADR 0042). Golden volumes: NURBS circle cylinder and torus, periodic quadratic
  (10 a²/3), quintic edge, Fusion-style clamped cubic, a shelled NURBS cup with per-span face
  names. Left: the Fusion reader emitting `bspline` for its 69 decoded `D82E012F` records,
  on-curve constraints, trimming, a command and GUI tool. ADRs 0004, 0016, 0042 amended.
- Ellipses and elliptical arcs (2026-11-09), end to end: sketch entities with solver
  support (angle, `a`, `b` as plain unknowns, arc ends kept on the curve, point-on-ellipse,
  fixed, horizontal/vertical, `semi_major`/`semi_minor`), arrangement and exact areas,
  `Seg2::Ellipse`/`EllipseArc` through the kernel wire and OCCT (`gp_Elips` edges), text
  language (`ellipse`, `ellipse_arc`), `sketch.add_ellipse`/`add_ellipse_arc`, a GUI
  Ellipse tool, tree import (`ellipse`, `ellipse_arc`; ADR 0042) and OCCT golden volumes
  (extruded ellipse, 360-degree O-ring revolve). Left: tangent/equal/radius constraints and
  a general axis-angle dimension on ellipses, ellipse patterns and trimming, a GUI
  elliptical-arc tool, DXF/drawing and CAM import of ellipses, FreeCAD/Inventor importers
  emitting the new curve types (other agents). ADRs 0004, 0016, 0042 amended.
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
  together (glass bodies are drawn see-through since 2026-11-08; the preview itself stays opaque). Panels collapse and swap sides (View > Browser / Timeline / Browser on the right; a
  collapsed panel leaves a rail button; kept with the preferences, DESIGN.md §1.15). Timeline groups done (DESIGN.md §1.4: `group: "name"` on features, `feature.group`, the
  chip/browser menu's Group…/Ungroup; a run of grouped chips is framed with its name, which
  folds it to slivers). The S quick bar is customizable (right click a ribbon tool: add to or remove from
  the quick bar, per context, at most seven, kept with the preferences). Marking-menu flicks done (with a navigation preset that leaves the right button free, a
  right drag of 40 px or more runs the menu item in that direction without opening it,
  DESIGN.md §1.3). UI scale done (View > UI scale, 75 to 250 %, or Ctrl + plus/minus; kept in
  the preferences, DESIGN.md §1.22). Properties panel done (the running command's form docks at the right
  instead of floating; Close button). SELECT group done (a filter: faces and edges, faces only, edges only; it limits what viewport clicks pick). SURFACE tab done (ADR 0036: `surface_extrude`, `patch`, `thicken`, `stitch`;
  sheet bodies; kernel ops through the shim, the worker protocol v9 and the OCCT tests;
  `modeling_surface.rs`). INSPECT stays a ribbon group (measure, body, overview, ...). Wireframe view style done (the navigation bar's
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
     OCCT golden tests in `modeling_hole.rs` and the naming golden `hole_names`). The preset
     values were checked against public catalogue tables on 2026-11-09 (ADR 0019 amendment; none
     changed).
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
     import done (`gusset-scad`, FCStd in the import worker); OpenSCAD objects come in as editable
     features checked against the exact build (ADR 0033 amendment, 2026-11-08); FreeCAD PartDesign
     history comes in as editable features when it rebuilds to FreeCAD's stored shapes (ADR 0033
     amendment 2026-11-09). PLY and glTF 2.0 done (`gusset-meshio`:
     PLY ASCII/binary import and binary export; glTF `.glb` export and `.glb`/`.gltf` import with
     node transforms, metres/Y-up converted to mm/Z-up, only embedded buffers read; fuzz targets
     `mesh_ply`, `mesh_gltf`; File > Export lists OBJ, PLY and glTF). Parasolid XT done (ADR 0035, `gusset-xt`: clean-room
     schema-driven reader of text and neutral binary, V4/V9 layouts inferred from NIST
     samples, V12 from the public Reference, V14+ embedded deltas; converted to STEP and imported
     by the sandboxed worker; `.x_t`/`.x_b`/`.xmt_txt` in `import.file`; fuzz target `xt_read`;
     OCCT golden test against NIST's samples). Blends and SP-curves are sampled and fitted, body names
     come from attributes, assemblies keep their placements, the B-surface pole order is
     chosen by the faces' vertices. Colours done (face and body colours as STEP styles, ADR 0045). Not yet: V12
     attribute definitions, deltas and binary are unverified without sample files.
   - v0.4: 2D drawings, DXF/SVG; FreeCAD/OpenSCAD import, Parasolid XT.
   - ACIS SAT import done (ADR 0039, `gusset-sat`: clean-room reader of versions 4 to 7 from
     the NIST samples, versions 1 to 7; planes, cylinders, cones, lines, circles, ellipses,
     B-spline intersection curves; via STEP in the
     worker; fuzz target `sat_read`; OCCT test against Spatial's own STEP translations).
     Spheres, tori and B-spline surfaces (procedural ones by their stored approximation)
     and versions 2 and 3, periodic B-splines done too. Binary ACIS (`.sab`) and Fusion 360
     `.f3d`/`.f3z` archives done (their `.smb` bodies), helices too. Fusion component placements
     done (rigid occurrence matrices read from `Design1/BulkStream.dat`, gusset_sat::fusion;
     36 of the Crickit HAT's parts land on its board). Nested components (occurrence records,
     parents composed, a component used n times gives n bodies) and `.f3z` designs placed by
     the root's references done (ADR 0039 amendment 2026-11-08; per-body check against the
     authors' STEP: LoRa 82/83, MacroPad 167/185, Trinkey 63/65, QT Py 138/142). Buck case placed
     2026-11-08 (0 -> 65/90: a reference at the origin whose board the root moved). HDMI cable
     2026-11-08 (0 -> 53/54): its pins are real bodies, not pattern instances (a rational
     `exact_spl_sur` layout the SAT reader refused, and a moved-design placement of the 2025 layout);
     the same reader fix lifted Buck 65 -> 71, Crickit 48 -> 54, MacroPad 167 -> 168, Trinkey
     63 -> 64, QT Py 138 -> 139. Helical land surfaces (`helix_spl_line`,
     one SMT sample) and the first-surface-`nubs` rule done 2026-11-08 (MacroPad 173/185, Buck 86/90).
     Next: true pattern instances (none found in any sample). Pico case
     2026-11-08 (0 -> 213/216): not joints; the root stores each referenced component's world placement
     in a run of `CE2913AA` records, the referenced design lists its components by versioned name
     (ADR 0039 "Pico case"; grouping rests on one sample). Fusion colours
     done (ADR 0039 amendment: `.smb` attribute -> design stream -> `.protein` package -> STEP
     styles); the history blob (`.smbh`) before a body gives the face materials its `.smb` lost
     (matched by face tag, ADR 0039); the `BASEFEAT_PM_BODY_REF` records colour the imported
     sub-bodies (RFM69, OLED, switch: record index k -> k-th transform-carrying body after the
     helper, only when the counts agree, ADR 0039 "Body references"); next there: the board's
     plate takes the component-named `Board` material (no id link found), single-record
     features' colours (parse as black, unread).
     Fusion design timeline as features done 2026-11-09 (ADR 0039 amendment, `gusset_sat::fusion_tree`,
     `fusion_history` in `import.rs`): port of the clean-room prototype (sketches, profile regions,
     extrudes, fillets/chamfers by edge midpoint, holes, linear patterns, mirrors; ACIS blobs name
     edges); kept only when it rebuilds to the stored bodies (`rebuild_check`) or, for a file that
     stores the bodies of each feature step, when every stored solid is a state or a cut tool of the
     rebuilt model (`fusion_states_check`; weaker for the features after the last stored state, see the
     ADR). 2 of 19 samples come in as history (L-Bracket, MacroPad keyplate; Rust trees equal the
     prototype's); test `f3d_history.rs` (`GUSSET_F3D_SAMPLES`, `GUSSET_F3D_MIN_HISTORY`,
     `GUSSET_F3D_PROTO`), fuzz target `fusion_design` extended. Next there: designs inside `.f3z`
     (the Assembly's keyplate component), revolve axes and counterbore/countersink holes (need samples),
     profile pieces and loops that do not close (Crickit, BQ25185), pattern of bodies/components,
     base features (PCB bodies), a component used several times (bottom plate), Draft/Shell/Sweep.
     Round 16 (2026-11-11): 100 of the 150 corpus designs as history (99 before; extrude starts,
     OffsetFaces), blobs placed several times imported once (Crickit CPX Assembly 180 s to 88 s),
     edges past a closed curve's end (the Buck Case body); tally in `docs/formats/fusion-corpus-tally.md`.
     Open: an extrude whose start and to-object end are both faces (2190, 5358, 5360), the stored bodies
     of a timeline still converted one per worker (`fusion_stored_bodies`).
   - SolidWorks `.sldprt` import done (ADR 0043, `gusset-sldprt`: OLE2 and chunk containers read clean-room
     from samples; the Parasolid partition goes through `gusset-xt`, whose V13-base BODY gained
     the extra byte found in these files; 12 blussyya pairs match SolidWorks' STEP to 1e-15; fuzz
     target `sldprt_read`; OCCT test `sldprt_samples.rs`). Parts whose bodies come from an
     imported feature (`LocalBodies`) done (ADR 0043 amendment; all 23 samples import); a file that
     ends early warns. `.sldasm` assemblies import too (amendment 2026-11-08: component list + placements from `swXmlContents/COMPINSTANCETREE`, parts read from the assembly's folder). Virtual components are read from their embedded part streams (amendment 2; the sample's two are empty parts). The row-vector rotation reading is confirmed on 54 public assemblies (ADR 0043 limits). Feature history (amendment 4): `gusset_sldprt::features` ports the `scripts/re/solidworks` prototype (sketches, extrudes, revolves, fillets/chamfers by edge midpoint); the tree is kept only when it rebuilds to the Parasolid body (`rebuild_check`, shared with FreeCAD), else exact bodies with the reason; amendment 5 adds the hole wizard (with head clearance), mirror planes, face-sketch frames, generalised edge records and stale-edge dropping, so 17 of the 35 Slip-Pad parts come in as history (OCCT test `sldprt_history.rs`, `GUSSET_SLDPRT_MIN_HISTORY`); the prototype's 18th, motorClip_v1, is 4.1e-5 off in volume against the shared 2e-5 check (owner decision: loosen it for SolidWorks, or find the 1e-3 residual). Amendment 6 ports the prototype's newer-format gains (Config-N streams, learnt unit-component tag, flags-word boss/cut, MirrorSolid, distance-angle chamfers as corner cuts, sketch normalization, localised hole wizards): plaque_base_roulante 5/5 and plaque_superieure 5/6 of the ARIG revision set come in as history (`GUSSET_SLDPRT_SAMPLES` takes several folders), Rust trees equal the prototype's on 99 parts. Next there: chamfer edge records (3 parts), end type 4 (FrameBracket), pattern records, the typed walker past the sketch lists; for the newer format loft cut, move face, thin boss and boss end type 2 (Angle_support; its Parasolid is damaged in Gusset too) and the Congé1 no-op of plaque_superieure 1618664833 (kernel side). Not yet: OLE2-era assemblies. Solid Edge and Inventor parts: see the next entry.
   - Solid Edge `.par`/`.psm` and Inventor `.ipt` import done (ADR 0046, `gusset-ole`; samples from Zenodo found
     2026-11-09, CC-BY): both are OLE2 files (the CFB reader of `gusset-sldprt` is shared). Solid Edge keeps a plain
     Parasolid transmit in `PARASOLID/STREAM0.X_B` (15 of 15 parts import via `gusset-xt`; node types 163/184,
     the helix forms, were decoded on 2026-11-09, ADR 0035 amendment). Inventor keeps the part's body
     as binary ShapeManager in a deflated `RSeStorage/B*` stream (35 of 35 parts import via `gusset-sat`; 6 of the 8
     parts with a STEP twin match it to 1e-15, the other two are different revisions of the model).
     `gusset-sat` now stops at an end record without terminator. Fuzz target `ole_read`; OCCT test `ole_samples.rs`
     (env `GUSSET_OLE_SAMPLES`). Not done: `.asm`/`.iam` assemblies, `.psm` (no sample), feature history of Solid Edge parts.
   - Inventor feature history (ADR 0046 amendment 2, 2026-11-09): the clean-room Python prototype is ported to
     Rust (`gusset_ole::inventor_features`, modules `inventor_dc`, `acis`, `inventor_profile`; `inventor_history`
     in `import.rs`, tier `TIER_INVENTOR_HISTORY`, the shared `rebuild_check`). 16 of 35 sample parts come in as
     editable features (OCCT test `ipt_history.rs`, env `GUSSET_IPT_SAMPLES`, floor `GUSSET_IPT_MIN_HISTORY`), the
     same 16 as the prototype; all 35 Rust trees equal the prototype's to 1e-6. Holes the kernel refuses are retried as plain cuts, decided by
     the rebuild (deterministic). Cosmetic threads and suppressed features are left out and listed in the warnings.
     Fuzz target `ole_read` covers the reader. Round 2 (amendment 3, 2026-11-09): fillet/chamfer picks on the
     true pre-fillet edge with a 0.1 mm path, cylinder blends with any two plane/cylinder supports, edge-blend
     tags, the +-pi fix, seam picks for whole circles, hole fallback axis as a profile line, rectangular
     patterns (direction, count, spacing, chained second direction, midplane): 20 of 47 as history
     (L-Bracket, side, Conecting plate 1, back accses panel added); Rust trees equal the prototype's except
     fp ties in a degenerate thumbscrew arc. Rounds 3-4 (amendments 4-6, 2026-11-09): distance-angle chamfers
     with their reference face (M8), ellipses (the 4 O-rings), full-round fillets and thicken as a join extrude:
     25 of 47 as history. Next here: chamfers chained round a cylinder with cone corner patches (blocks the 3
     thicken parts), rolling-ball/vertex blends, the knurl pattern count of the thumbscrews.
   - Full-round fillets done (2026-11-09; ADR 0004 amendment, ADR 0042 amendment): `fillet F (full_round: [<face>])`
     replaces a planar face between two parallel planar faces by a tangent half cylinder (radius t/2). `Kernel::full_round`
     (OCCT `gs_full_round`: corner prisms cut in one boolean, exact; remote protocol request 36; mock: none), eval
     (`gusset-eval/src/full_round.rs`), `feature.fillet` `full_round` argument, tree import `"full_round": {point, normal}`.
     Golden tests (20 x 4 x 10 slab = 800 - (80 - 20 pi)) in `gusset-kernel-occt`, `modeling_full_round.rs`,
     `modeling_tree_import.rs`. Left: the Inventor reader must emit it, no GUI form, only the planar perpendicular
     case (slanted or curved middle faces, a choice of side pair for square faces).
   - Unequal chamfers done (2026-11-09; ADR 0004 amendment, ADR 0042 amendment): `chamfer` takes
     `distance2: <len>` or `angle: <deg>` and an optional `faces: [..]` (one reference face per edge;
     default: the adjacent face whose persistent name sorts first). `Kernel::chamfer_ref` +
     `ChamferSpec` (mock: none, as for equal chamfers; OCCT: `Add(d1, d2, E, F)` / `AddDA`; remote protocol request 35),
     eval validation (`gusset-eval/src/chamfer.rs`), `feature.chamfer` arguments, GUI mode switch
     (equal / two distances / distance and angle), tree import (`distance2`, `angle`, `face` pick).
     OCCT golden tests (2 x 3 on a 10 mm cube = 1000 - 0.5 * 2 * 3 * 10) in `gusset-kernel-occt`,
     `modeling_chamfer.rs` and `modeling_tree_import.rs`. Left: the Inventor/SolidWorks/FreeCAD
     importers still write equal chamfers only (M8: they need to emit `distance2`/`angle` and the face
     pick), variable-distance chamfers, a viewport preview of the reference face.
   - Sheet metal (ADR 0037) phase 1 done: `contour_flange` (open line chain bent at every
     corner; thickness, inside bend radius, K-factor, flip, symmetric), `tab` (flat plate),
     faces named `flat/flat_back/bend_in/bend_out/start/end(<line>)`, flat pattern on the body
     (`query.flat_pattern`, `sheet.export_flat` DXF/SVG 1:1), SHEET METAL tab; OCCT exact-volume
     test `modeling_sheet.rs`. Phase 2 done: `edge_flange` off a tab's outline line (angle, up or
     down, length; the tab's flat pattern grows; OCCT exact-volume test). Flanges at outside
     corners leave an open corner; at inside corners they are refused. Edge flanges off a contour
     flange's sides done (`line: l6_start` / `l6_end`; OCCT exact-volume test). Flanges off
     flanges done (`line: F1_end`). Later cuts through flat regions are unfolded into cut-outs.
     Cuts across contour bends
     done. Cuts across edge-flange bends done too.
     Closed corners done (`close: true`,
     90 degree flanges at outside tab corners, square relief). Mitred corners done (`close: mitre`, walls cut on the diagonal 0.1 mm apart). Closed corners at other angles done (up to 90 deg, walls meeting on the mirror plane). Corners of flanges leaning back past 90 deg (up to 150) done. Hems done (`angle: 180 deg`).
   - Direct modeling: push/pull (`offset_face`), `move_face`, `draft`, and `delete_face` (ADR
     0038: remove holes, fillets, bosses and heal with OCCT defeaturing; OCCT test). Replace face done
     (`move_face ... onto:` a parallel face); tilting onto a tilted plane done (turned about the
     planes' common line through the kernel's draft). Live rules done (`rules`, default on in the command: coplanar and coaxial faces follow the moved ones).
   - CAM phase 1 done (ADR 0040, `gusset-cam` on `cavalier_contours`): 2.5D pocket (islands
     kept clear) and outside/inside profiles of sketch regions with a flat end mill in depth
     passes; `cam.toolpath` (rings, levels, length, time) and `cam.export_gcode` (metric G-code,
     G2/G3 arcs), ramp entry along the ring (default 3 deg), drilling (G81/G83 at circle centres), holding tabs on profiles. Viewport preview done (`cam.toolpath` with `path: true`
     returns the cut as world polylines; the app draws the last toolpath until the model
     changes). Review fixes done (ramps spare tabs, bounded inputs, no centre ridge). Adaptive
     clearing done (`pocket_adaptive`: rings spaced for the sharpest corner, trochoidal slots
     where a ring has nothing inside; radial cut within the stepover, ADR 0040 amendment).
     Rest machining done (`pocket_rest` with `previous`, the earlier tool's diameter). 3D finishing done (`surface_ball`/`surface_flat`: drop-cutter raster over the model's mesh). Waterline and steep/shallow done (`waterline_*`, `steep_*`). Pencil finishing done (`pencil_ball`); holder/shank collision check done: `holder: [shank D, flute, holder D, stickout]` on the 3D operations checks the path and lifts the tool where the shank or holder would touch, with a warning that material is left there. A `cam_job` stores a `holder` too; values given next to `job` override the job's (suppressed jobs are refused). Jobs in the document done (`cam_job`, `job: Job1`); helical bores in
     gusset-cam. Climb milling done (the default;
     `conventional: true`).
   - Simulation phase 1 done (ADR 0041, `gusset-fea`): linear static analysis on a voxel grid
     (hexahedra, matrix-free PCG), fixed faces and face loads by name, max displacement and von
     Mises stress; `sim.static`. Stress colours done (`field: true` returns the voxel surface in
     8 stress bands; the app shows them blue to red in place of the model while the result is
     open). Natural frequencies done (`sim.modal`: lumped
     mass, LOBPCG with a Jacobi preconditioner; a cantilever reads 1.5 % above beam theory).
     Mode shapes in the viewport done (`shape: k`). Refinement at the stress peak done (`refine: true`, a submodel four times finer). Next:
     shell elements for thin sheet bodies done (sheet bodies in `sim.static` run on MITC4 flat shells
     over their mid-surface; ICT preconditioner and modes on shells done too). Topology optimisation done (`sim.optimize`, SIMP on the voxel model,
     ADR 0041 amendment; generative design phase 1). Multigrid preconditioner
     done (both solvers; 13–17× fewer iterations, 2.7× faster). Note: the MCP tool list is at
     its 600-byte average budget; new fields need trimming elsewhere.
   - Feature-tree interchange done (ADR 0042): `import_file` on a `.gtree.json` appends
     editable sketches, extrudes, revolves, fillets, chamfers and parameters; exporter
     plugins written for Fusion (Python add-in), SolidWorks (VBA macro) and Onshape (REST
     script), all **UNVERIFIED** until a person runs their README's manual test; each
     sample file imports with its stated volume under OCCT. Format v2 done (holes,
     patterns, mirrors, shells); sketch planes at any angle done (chained datum planes; tilted
     feature mirrors with them).
   Every milestone goes through the reviewer, security reviewer and dogfooding.
2. Post-MVP leftovers: dm-flakey power-loss tests on a real block device (M14) done:
   `scripts/powerloss.sh` (CI job `powerloss`) saves on ext4 over device-mapper, cuts the disk
   off with dm-flakey `drop_writes` at a random moment, remounts and checks that the last
   acknowledged save is whole (`gusset-store/tests/powerloss.rs`). Agent keys (F2-2) and saves off the model thread are done.
3. Release: v0.1.0-mvp is published (pre-release, tag at e3ee7e0, run 38271195031):
   AppImage/deb/tar.gz for Linux x64+arm64, MSI/zip for Windows x64+arm64, dmg for
   macOS arm64, SBOMs, OCCT source, SHA256SUMS, update manifest. Releases can be
   started by tag name through workflow_dispatch (ADR 0013 amendment). Unsigned
   (ADR 0015); no update signing key yet, so the in-app updater refuses the manifest.

- SolidCore tracking (ADR 0023, monthly): 2026-11-07 second look (docs/research/solidcore-2026-11.md):
  correct end to end but 2-57x slower than OCCT, no face history, platform sin/cos; nothing
  routed. Adapter in `kernels/solidcore` (opt-in, own workspace). Next look: 2026-12.

- History-free mode done 2026-11-08: `feature.bake` (ADR 0038 amendment) turns the bodies into
  exact imports for direct editing; one undo restores the history.
- Collaboration phase 1 done 2026-11-08 (ADR 0044): `history.export_bundle` / `history.import_bundle`
  move a branch (nodes + reachable objects, zstd, BLAKE3) between project copies as `.gusb`;
  the import re-hashes everything, reuses nodes it already has and adds `from/<name>`.
  Live relay (phase 2) is later.
- Rendering (later tier) started 2026-11-08: path-traced stills (`render_traced`, ADR 0009
  amendment; `gusset build -e out.png`).
  Appearances done 2026-11-08 (ADR 0045): `appearance <Body> (color, finish)` in the document
  (plastic, metal, glass, rubber), `body.appearance`, a swatch in the browser; the viewport,
  `render_view` and PNG export use them. Imports keep the file's names and colours: Parasolid
  and SolidWorks (`SDL/TYSA_*` attributes), and STEP through XDE, whose assemblies now come in
  as one body per solid, named after its own part; STEP export writes the colours back. The
  viewport shades the finishes (metal mirrors sky and ground; glass lightened; rubber matte), the
  form has a colour picker, and the photo render's clear coat is added rather than sampled (no
  grain). Dogfooding run 14 is fixed but its open nits. 3MF and glTF carry colours both ways
  (base materials, materials); reference meshes take appearances. The code review of the day's
  work (docs/reviews/2026-11-08-appearances-review.md) is fixed (R11 and R12 accepted). STEP
  face colours done (ADR 0045 amendment): stored per face name in the appearance, drawn by the
  viewport, `render_view` and the path tracer, written back by STEP export. Display geometry of
  unchanged bodies is reused across rebuilds (PERFORMANCE.md). Face colours also from Parasolid
  and SolidWorks (gusset-xt writes them as STEP styled items), in 3MF and glTF export, and set
  by hand (`body.appearance` with `faces`; the app's Appearance on picked faces); merges take
  them apart from colour and finish; reviewed (docs/reviews/2026-11-08-face-colours-review.md).
  A top-level BVH speeds picking and photo renders of many bodies (PERFORMANCE.md). A RENDER
  tab (Appearance, Render image) and the `export.image` command, and Photo preview in the
  viewport. The workspace switcher names the workspace and switches between Design, Assemble
  (an Assembly tab of component, joint and assembly-study tools), Render, Manufacture and
  Drawing (spec layout). The render-workspace review is fixed
  (docs/reviews/2026-11-08-render-workspace-review.md). Procedural textures (wood, brushed, carbon, speckle; ADR 0045 amendment) in the viewport
  and photo renders. Next: Fusion body/component appearances without
  a face attribute. 3MF per-triangle colours are read (run 15).
- Dogfooding runs 12 and 13 and the follow-up security review (T1-T12) are fixed but T2's
  single-record read (bounded) and T3 (one `LocalBodies` stream).

## Known gaps (not MVP scope per the spec; planned for later phases)
- Feature-level patterns: done (2026-11-03). `pattern_linear` / `pattern_circular` take
  `features: [...]`: what each feature added to or cut from its body (after minus before, before
  minus after, recorded per feature during evaluation) is copied per instance and united with
  or cut from that body (OCCT exact-volume tests: a row of holes, a ring of bosses).
- Sketch arrangement limits (ADR 0016): a spline that runs along another curve for a
  stretch is not merged with it (lines and arcs that overlap now share the stretch, and a
  spline's tangential touch cuts both curves); a trimmed spline is refitted (close to, not exactly, the old curve); constraining or trimming a pattern copy, or patterning a pattern copy (grids are a linear pattern's second direction, `count2/dx2/dy2`), explodes the
  inner pattern into ordinary entities, so its count is no longer parametric.

## Waiting on the owner (2026-11-08)
Work that cannot go further without something only the owner can give:
- **Sample files.** Solid Edge `.asm`/`.psm` and Inventor `.iam` with a known placement (the
  parts came in with ADR 0046; assemblies need ground truth). (FreeCAD examples, ARIG's
  SolidWorks assemblies and public Inventor/Solid Edge parts were fetched on 2026-11-09.)
- **Network listener.** The live collaboration relay (ADR 0044 phase 2) opens a port: the
  owner's say-so (folder sync, now also automatic, covers sharing until then).
- **Hardware.** A real integrated-GPU measurement of the large-assembly frame time
  (PERFORMANCE.md), and a SpaceMouse on Windows/macOS to test a HID reader against.
- **Signing.** An update-signing key (the updater refuses unsigned manifests) and code signing.
- **Manual tests.** The feature-tree exporter plugins (Fusion, SolidWorks, Onshape) need a person
  to run their README's test in the real apps.

## Decisions made without the owner (easy to reverse)
- **cargo-audit ignores three "unmaintained" notices** (2026-11-03): RUSTSEC-2026-0370/0371/0372
  (zvariant, zbus_names, zvariant_derive), reached only through egui's Linux accessibility
  stack (accesskit -> AT-SPI -> zbus). Not vulnerabilities; no maintained replacement exists
  and removing AccessKit would drop screen-reader support (DESIGN.md §1.22). `.cargo/audit.toml`.
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
- Benchmarks in CI (spec: a regression of more than 10 % fails the build): the CI `perf` job
  enforces absolute frame-time budgets (`gusset-render` test `perf`, about 1.5 x the spec's
  software-renderer target) instead of a relative 10 % gate, because shared GitHub runners vary
  by more than 10 % between runs. A relative gate needs a dedicated runner (the owner's call).
- Right-to-left text rendering is a known egui gap (ADR 0009). Strings are
  externalized; RTL rendering waits on upstream.
- Mesh import (STL/3MF/OBJ, tier 3) produces **reference mesh bodies**, not solids: the
  kernel cannot build a B-rep from a triangle mesh. They are stored as binary STL blobs,
  displayed (`render_view` and the GUI viewport, pickable as one face each),
  listed by `query.model` (`reference_meshes`, with volume when watertight) and included
  in mesh exports; STEP export, booleans and edge/face references do not apply to them.
  Mesh files are parsed in-process by gusset-meshio (pure Rust, with size/count limits),
  off the model thread; STEP/IGES go to the sandboxed worker (ADR 0008 §4).

## Research gaps
- Reddit, and direct fetches of the FreeCAD and Onshape forums and HN, were blocked from
  this environment. User research relied on search-engine snippets of those sources.
  Re-run when access allows.
- WebFetch could not open vendor pages; some HARDWARE.md entries are marked unverified.
