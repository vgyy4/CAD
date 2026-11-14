# ADR 0047: PCB design

Status: accepted (2026-11-12). Owner request: add PCB design to Gusset, built on the best open-source
PCB work rather than from scratch, inside Gusset's UI. Research: `docs/research/pcb-design.md`.

## Context
The mature open-source PCB tools (KiCad, LibrePCB, Horizon EDA, pcb-rnd, gEDA, Fritzing) and the
de-facto autorouter (Freerouting) are all GPL (2 or 3), in C++ or Java. Gusset is MIT OR Apache-2.0
(ADR 0001) and every user action is a command in one registry shared by GUI, CLI and MCP (ADR 0012);
the GUI is egui on wgpu (ADR 0009). KiCad's file formats (`.kicad_pcb`, `.kicad_mod`, `.kicad_sch`,
`.kicad_sym`) are open S-expression formats with published documentation
(dev-docs.kicad.org, "File Formats"), and KiCad's libraries are CC-BY-SA-4.0 with an exception for
designs and generated files.

## Decision
**Licence rules.**
* GPL PCB code (KiCad, LibrePCB, Horizon, pcb-rnd, gEDA, Fritzing, Freerouting) is never linked,
  embedded, bundled, ported or translated into Gusset, and none of its source is copied. Reading it
  is not needed and is not done for this work: the readers and writers are **clean room**, from the
  published format documentation and sample files only.
* Gusset gets a **native Rust PCB model on KiCad's open file formats**, so users keep the KiCad
  ecosystem (files, libraries, `kicad-cli`) while Gusset adds the mechanical side.
* Permissive crates are used where they vet clean in `deny.toml`: `gerber-types` 0.7
  (MIT OR Apache-2.0; its dependencies `chrono`, `uuid`, `thiserror`, `num-rational`/`num-bigint`/
  `num-integer`, `strum`/`strum_macros`/`heck` are MIT or MIT OR Apache-2.0) builds the Gerber X2
  output. The Excellon writer is our own (no crate found; the format is small).
* KiCad, `kicad-cli` and Freerouting may later be driven as **separate, user-installed processes**
  (detect, never bundle), each with its own ADR as ADR 0001 requires for GPL tools.
* **KiCad library rule.** The KiCad symbol, footprint and 3D-model libraries
  (CC-BY-SA-4.0 with exception) are used **as data**: a user's board may reference them and Gusset may
  read them from the user's KiCad install or a folder the user names. They are **never vendored into
  this repository** (no footprints, symbols, models or demo boards in the source tree or test data).
  A downloadable content pack with its own licence notices is a later, separate decision. KiCad's
  demo boards (CC-BY-SA-4.0) may be fetched for local checking only, outside the repository.

**Phase 0 + 1a (this ADR's work).**
* New crate `gusset-pcb`:
  * `sexpr`: lossless S-expression reader/writer. Every node is kept, known or not, with its atom
    text verbatim (numbers keep `1.000`) and whether it was quoted; a round trip changes only layout.
    Iterative (no recursion on input depth), bounded by `Limits` (256 MiB, depth 128, 30 M nodes).
  * `board`: the typed model over the tree, which stays the source of truth (`Board::write` writes
    the tree; `Board::move_footprint` edits it and re-reads). Layers and stackup, thickness, nets,
    footprints (reference, value, attributes, 3D model references with offset/scale/rotation),
    pads (SMD/THT/NPTH/connect; circle, rect, oval, roundrect, trapezoid, custom with polygon and
    circle primitives; round and oval drills with offset; nets; mask/paste margins), tracks
    (segments and arcs), vias, zones (outline and filled polygons; keep-outs), drawings (lines, arcs,
    circles, rects, polygons, on any layer, footprint ones placed on the board) and text (kept, not
    plotted). Reads `.kicad_pcb` and `.kicad_mod`, KiCad 5 (`module`, centre+angle arcs, `width`) to
    10 (`stroke`, `property`, net names without numbers). Item counts are bounded (`max_items`).
    Board coordinates stay KiCad's (mm, Y down); outputs flip Y.
  * `gerber`: Gerber X2 per layer: every copper layer, `F/B.Mask` (grown by the pad or board mask
    margin), `F/B.Paste` (pad paste margin and ratio), `F/B.SilkS` (drawings) and `Edge.Cuts`
    (`Profile,NP`). Pads at multiples of 90° are flashed apertures (C, R, O); others and roundrects
    are regions with exact arcs. X2 file attributes (`GenerationSoftware`, `FileFunction`,
    `FilePolarity`) and aperture functions (SMD/component/via pads, conductors). Deterministic output
    (no dates). `%FSLAX46Y46*%`, mm.
  * `drill`: Excellon, plated (THT pads and vias) and non-plated in separate files, metric,
    decimal, oval holes as `G85` slots, tools sorted by diameter.
  * `check`: a basic sanity check: the outline (`Edge.Cuts`) chains into closed loops, every pad is on
    a copper layer, every net referenced is listed, tracks are on copper layers, holes lie inside the
    outline. Full DRC is later.
  * `board3d`: the board as kernel bodies: the outline (several outer loops and cut-outs allowed)
    extruded to the board thickness (bottom at z = 0) with pad holes (round and slots; vias optional)
    as profile holes, holes that cross the outline or each other left out with a note; optional
    outer copper (pads, tracks, vias, zone fills, at the stackup's copper thickness); a box per
    footprint from its fabrication drawing (`F.Fab`/`B.Fab`, the part's body) or else its pads'
    extents, at a given height on the top or below the bottom. Footprint STEP models are placed by
    `model_steps` (model scale, rotation about X then Y then Z, offset; flip for the bottom side;
    footprint rotation; position on the board face).
* Registry commands (ADR 0012; GUI = CLI = MCP; not in the core MCP tool list): see "Commands" below.

**Commands.**
* `pcb.info`: reads a `.kicad_pcb`/`.kicad_mod`; counts, layers, thickness, size of the outline,
  and the sanity check's issues.
* `pcb.import`: the board into the document as bodies (board with holes; copper off by default;
  component boxes, or a footprint's STEP model when `models_dir` is given and the model is found
  there and readable through the sandboxed STEP import).
* `pcb.export_gerbers`: one `.gbr` per layer into a folder, named `<board>-<layer>.gbr`.
* `pcb.export_drill`: `<board>-PTH.drl` and `<board>-NPTH.drl`.

## Phase 1b (2026-11-12): the board in the session and a 2D editor
* **The board in the document.** A `pcb (file: "<name>.kicad_pcb", board: "<hash>", project:
  "<hash>")` statement (code view; stored in the history's parameter object) names the board's
  `.kicad_pcb` text, and optionally the KiCad project file beside it, as blobs. Every board edit
  stores a new board text and commits a document naming it, so `history.undo`/`redo`, branches,
  compare (one `pcb` item) and merge (the board as one item, a conflict when both sides edited it)
  work on boards as on features (ADR 0006). Snapshot closures keep both blobs. A board blob is
  the whole file: a 5.8 MB board costs 5.8 MB per edit before compression (zstd in `.gus`);
  per-item objects are later work if that matters.
* **Edits in `gusset-pcb` (`edit`)** change the tree and re-read it, so untouched parts are written
  back as read: `Board::new_two_layer` (rectangular outline, 1.6 mm FR4 stackup), move/rotate
  (existing), `flip_footprint` (layers F↔B, local Y and angles mirrored: KiCad's default top/bottom
  flip; mirrored text justification is not set), `add_track`, `add_via` (through), `delete_items`,
  `set_outline` (`gr_rect` for axis-aligned rectangles, `gr_poly` otherwise, `gr_line`s for KiCad 5
  files). New items get deterministic UUIDs (KiCad 6+); a net unknown to a numbered-nets file is
  declared. Tracks, vias and footprints carry their tree position, so a skipped malformed item
  does not shift indices.
* **Rules (`rules`)**: board clearance, minimum track width, annular ring, hole-to-edge,
  copper-to-edge, default track/via sizes and net classes (KiCad 5 `net_class` in the board;
  KiCad 6+ `.kicad_pro` `net_settings.classes`, `netclass_assignments`, `netclass_patterns` with
  `*`/`?`, and `board.design_settings.rules` minimums). Defaults: clearance 0.2, width 0.15
  minimum and 0.25 new, ring 0.1, hole/copper to edge 0.3, via 0.6/0.3 mm.
* **Connectivity and ratsnest (`copper`, `ratsnest`)**: pads (polygons, round pads as discs),
  tracks (capsules, arcs as chords) and vias per copper layer; same-net copper that touches is
  joined, and zone fills join the copper that touches them (pads in thermal reliefs touch the
  spokes; edges bucketed on a grid). The ratsnest is a minimum spanning tree per net over the
  islands' anchors (pad centres, via centres, track ends). On the 7 local demo boards (all fully
  routed in KiCad) it reports 0 unrouted connections; the largest (7932 tracks) takes 32 ms.
* **DRC (`drc`)**: copper clearance per layer between different nets (no-net copper counts as its
  own net; the larger of the two nets' class clearances, else the board's), shorts, track width,
  annular rings (vias and plated pads), hole-to-edge and copper-to-edge, with positions and the
  items involved; 0.5 µm slack for file rounding. Zones are not checked. 52 ms on the largest demo
  board. Boards whose project file sets tighter classes show clearance reports until it is read.
* **Assembly files (`fab`)**: pick-and-place CSV (`Ref,Val,Package,PosX,PosY,Rot,Side`, mm from the
  auxiliary origin with Y up, `exclude_from_pos_files`/`board_only` left out) and BOM CSV
  (`Qty,Value,Footprint,References,DNP`, grouped by value + footprint + DNP, natural reference
  order); fields quoted when needed, a leading `= + - @` escaped against spreadsheet formulas.
* **Commands** (registry, not in the core MCP tool list): `pcb.open` (a `.kicad_pcb`, and the
  `.kicad_pro` beside it), `pcb.new`, `pcb.save` (writes `.kicad_pcb`, overwrite confirmed),
  `pcb.move_footprint` (`at`, `angle`, `flip`; by reference or `#index`), `pcb.add_track` (net from
  the copper at either end, width from the net class), `pcb.add_via`, `pcb.delete` (tracks, vias by
  index), `pcb.set_outline` (polygon), `pcb.drc` (rule overrides; at most 200 violations),
  `pcb.export_pos`, `pcb.export_bom`. `pcb.info`, `pcb.import` (the board to 3D: "`pcb.to_cad`"),
  `pcb.export_gerbers` and `pcb.export_drill` take the open board when `path` is left out.
  Board text reaches agents only in `_user_text` fields.
* **GUI (`gusset-app` `pcb_view`)**: a PCB tab whose central view is the board editor: layers in
  KiCad-like colours back to front with the active copper layer on top, a side panel with
  per-layer visibility, the active layer, grid (0.1 to 2.54 mm), tools (Select, Route, Via),
  ratsnest toggle, the unrouted count, a DRC button and its problem list; pan/zoom/fit; select
  footprints, tracks and vias; drag a footprint (snapped, shown where it lands); R turns, F flips,
  Delete removes; Route clicks lay segments on the active layer, snapping to pad centres; markers
  at DRC violations with their messages on hover. The widget only reports registry commands,
  which the app sends like every other edit. The ribbon's PCB tab holds open/new/save, the edit
  commands, DRC/info and the exports. Zone fills are drawn as outlines (egui fills convex shapes
  only).

## Results of phase 1b (2026-11-12)
* Tests: `gusset-pcb` 32 unit tests (10 new: new board, track/via add and delete give the file back,
  net declaration, move/turn/flip and flip back, outline, ratsnest with tracks and vias, zone fills
  joining pads, DRC codes and places, net classes and patterns, placement and BOM);
  `gusset-history` the board stored, compared, merged and kept by GC; `gusset-commands`
  `tests_pcb_edit` 5 (open, edit, undo/redo, save with an identical tree for an unedited board,
  project rules, DRC through the registry, assembly files with agent path rules, open board in
  info/import/drill); `gusset-app` `tests_pcb` 3 (snapping, picking, drop places, every editor
  action a registry command, the tab editing with undo and DRC markers through the app).
* MCP: core tool list unchanged (31 tools, 17901 of 18000 bytes); the command tools average 581
  bytes (142 tools; was 593, cap 600); the largest new one is `pcb_add_track` at 615 bytes.

## Phase 2a (2026-11-12): schematic capture
* **Formats (`gusset-pcb` `sch`)**: `.kicad_sch` and `.kicad_sym` read and written losslessly over
  the same S-expression tree as boards (KiCad 6 to 10 layouts; older files get a note). Modelled:
  library symbols (embedded `lib_symbols` and libraries) with units and body styles (`Name_U_S`,
  0 = common), rectangles, polylines, Béziers (as their control polygon), circles, arcs, text and
  pins (type, style, length, name, number, hidden; `pin_names`/`pin_numbers` hiding and name
  offset); derived symbols (`extends`) resolved when embedding; placed symbols (`lib_id`,
  `lib_name`, place, angle, mirror, unit, body style, properties with places, per-sheet-use
  references from `instances`, `in_bom`/`on_board`/`dnp`); wires, buses and bus entries (kept and
  drawn), junctions, no-connect flags, local/global/hierarchical labels, sheets with pins, text and
  graphic lines. Item counts are bounded like boards; fuzz target `sch_read`.
* **Placement convention (checked on samples)**: a library point (Y up) is flipped to Y down,
  turned by the symbol's angle (counter-clockwise on screen), then mirrored (`mirror x` flips Y,
  `mirror y` flips X), then moved to the symbol's place. The other order (mirror, then turn)
  connects fewer pins on the demo designs.
* **Netlist (`netlist`)**: items connect where connection points meet; a point (pin, label, sheet
  pin, junction, no-connect, wire end) anywhere on a wire joins it; crossing wires join only at a
  junction. Local labels join per sheet use, global labels and power symbols (by value, through
  their power-input pin; `PWR_FLAG`'s output pin joins nothing) across the design, hidden
  power-input pins of ordinary symbols by pin name, hierarchical labels with the parent's sheet
  pin. Label text compares with `/` written as `{slash}`. Net names as KiCad's: power/global name;
  else the local label highest in the hierarchy (`/sub/NAME`, `/NAME` at the root), then
  hierarchical labels; else `Net-(REF-PIN)` from the first pin with a name of its own (not `~` or
  its number; reference order, then unit, then pin), else `Net-(REF-PadN)`; a lone pin
  `unconnected-(REF[-NAME]-PadN)`; multi-unit references get the unit letter. Wire-grid work is
  bounded (8 M cells).
* **ERC (`erc`)**, kept simple: `pin_not_connected`, `no_connect_connected`, `pin_conflict`
  (output/power-output against each other, a no-connect pin on a net), `power_not_driven` (power
  inputs on a net without a power output: the `PWR_FLAG` check), `label_dangling` (bus labels
  skipped), `duplicate_reference`, `unannotated`, `missing_symbol`, `no_footprint`.
* **Schematic to board (`sync`)**: `update_board` adds footprints the board lacks when the caller
  finds the part's `Lib:Name` footprint file (never bundled; placed in a column right of the
  board), then sets every pad's net by reference and pad number (pads of a listed part whose
  number has no pin lose their net; unnumbered pads are left alone), and reports extra
  footprints, value and footprint differences, pins without a pad and parts without a footprint.
  Tracks keep their nets: renamed nets show as DRC problems until rerouted.
* **Session and history**: `schematic (file, sheet, sheets: {"sub.kicad_sch": hash})` in the
  parameter object names the root sheet and every sub-sheet text; closures keep them, compare
  lists a changed schematic with the board, merge takes it as one item. Each edit stores the one
  sheet it changed.
* **Commands** (registry, not in the core MCP tool list): `sch.open` (the root and the sheets it
  uses from beside it; sheet names that are absolute or climb with `..` are not followed and are
  reported), `sch.new`, `sch.save` (all sheets), `sch.add_symbol` (from a user-given `.kicad_sym`;
  reference defaults to the next free one across the design; library id `<file stem>:<symbol>`),
  `sch.move` (symbols: place, angle, mirror, fields follow; labels: place, angle),
  `sch.add_wire` (polyline; junction dots where it meets a wire's middle or three ends meet),
  `sch.add_label` (local, global, hierarchical), `sch.delete` (by `R1` or `kind#index`; unused
  library symbols go too), `sch.erc`, `sch.netlist`, and `pcb.update_from_schematic`
  (`footprints` folder of `Lib.pretty` libraries, `dry_run`). Edits on a sub-sheet name it with
  `sheet`. File text reaches agents only in `_user_text` fields.
* **GUI (`gusset-app` `sch_view`)**: a Schematic tab beside the PCB tab: the shown sheet on a light
  canvas in KiCad-like colours (symbol bodies with background fills, pins with names and numbers,
  visible reference/value fields, wires, buses, junctions, no-connects, labels, sheets with their
  pins, text); pan/zoom/fit; a sheet chooser; tools Select (pick, drag symbols and labels on the
  grid, R turns, X/Y mirror, Delete), Wire (corners snap to pin ends, double/right click or Esc
  ends), Label and Place (the panel's library file and symbol); ERC markers with their messages on
  hover. Cross-probing: picking a symbol selects its footprint on the PCB tab and picking a
  footprint selects its symbol. Every action is a registry command (undoable). The ribbon's
  Schematic tab holds open/new/save, the edits, ERC/netlist and the board update.

## Results of phase 2a (2026-11-12)
* Local check, not in the repository (CC-BY-SA data): 5 KiCad demo designs (KiCad 9 files, 14
  sheets, hierarchical ones included) read and write back with an identical tree. For
  `complex_hierarchy`, `pic_programmer`, `ecc83-pp` and `interf_u` the netlist gives every pad of
  KiCad's own board the same net, names included (164, 236, 33 and 373 pads), and
  `update_board` on those boards changes no pad; their ERC is clean except two bus labels (now
  skipped). `video` (buses across sheets) matches 1300 of 2046 pads: bus members that cross sheets
  through bus sheet pins are not joined yet. The largest design connects in 60 ms (debug build).
* Tests: `gusset-pcb` `tests_sch` 9 (library read and embedding with `extends`, round trip and pin
  placement with turn/mirror, nets from wires/labels/power, crossing wires and junctions,
  hierarchy with two uses of one sheet, ERC codes, edits, board update with an added footprint
  and a second update changing nothing, hostile inputs); `gusset-history` the schematic stored,
  compared, merged and kept by GC; `gusset-commands` `tests_sch` 3 (draw, netlist, ERC, move and
  undo, save; a hierarchical open with an unsafe sheet name refused, sub-sheet edits, save;
  the board updated from the schematic with dry run and undo, footprint names with path parts
  never looked up); `gusset-app` `tests_sch` 3 (picking, snapping, drops; every editor action a
  registry command; the tab editing with undo, ERC markers, cross-probing both ways).
* MCP: core tool list unchanged (31 tools, 17901 of 18000 bytes); the command tools average 570
  bytes (153 tools; was 581); the largest new one is `sch_add_symbol` at 767 bytes.

## Phase 2b (2026-11-12): board layout
* **Polygon areas (`gusset-pcb` `poly`)**: booleans and round offsets through `i_overlay` 9.1
  (MIT OR Apache-2.0, with `i_float`, `i_shape`, `i_tree`, `i_key_sort` from the same author,
  all MIT OR Apache-2.0; `cargo deny` clean, no duplicates); round shapes as polygons whose edges
  are tangent to them (they cover the true shape, at most 5 µm beyond); holes bridged into
  their outline ("fractured", as Gerber regions and KiCad's filled polygons are written, the
  bridge picked at the copy of a repeated vertex whose inside angle faces the hole); our own
  ear clipping (vertices bucketed on a grid; a copy of an ear's corner blocks it only when one
  of its edges runs into the ear) for drawing, as egui fills convex shapes only.
* **Zone fills (`zones`, `pcb.fill_zones`, undoable)**, per zone and copper layer, highest
  priority first: the outline inside the board outline shrunk by copper-to-edge; minus other
  nets' copper (pads, tracks, vias, copper drawings, fills of zones filled before) grown by the
  larger of the zone's and the net classes' clearances (+6 µm for chords and rounding),
  non-plated holes and pour keep-outs; minus the zone net's pads grown by the thermal gap
  (thermal), or by the clearance (`connect_pads no`), or nothing (solid; `thru_hole_only`:
  thermal for through-hole pads, solid for the others; a pad's or footprint's `zone_connect`
  overrides); opened by the minimum width; thermal spokes of the bridge width along the pad's
  axes (45° turned for round pads) where their ends land in the fill, clipped clear again;
  islands not touching the zone's net removed (mode 0), kept (1) or kept from the minimum
  area (2); zones without a net keep every island. Written back as `filled_polygon`s (`fill
  yes`), so Gerbers, connectivity and 3D take them as before. Hatched zones are filled solid
  (noted). Zone DRC: fills against other nets' copper (clearance or short) and against each
  other's corners.
* **Routing (`route`, `pcb.route`)**: A* over an octilinear grid in a window round the two ends
  (at most 1.5 M points; the step is half the smaller of clearance and width, 0.05 to 0.25 mm);
  a point is free when its gap to every obstacle is at least the needed gap (the larger net
  class clearance plus half the width; copper-to-edge for the outline; keep-outs that forbid
  tracks) plus half a grid diagonal, so the straight moves between free points are clear; then
  pulled tight: from each corner the farthest corner reachable by one or two 45° segments
  that are clear (checked exactly) is joined, so tracks hug obstacles. Corners can be rounded
  into arcs of a radius where the arc stays clear (written as KiCad `arc` tracks). A route is
  added in one edit (`Board::add_route`/`add_batch`). `pcb.route` lays a track through points,
  around obstacles (or straight with `around: false`), width from the net class, net from the
  start. Push-and-shove is not done.
* **Specctra (`specctra`, `pcb.export_dsn`, `pcb.import_ses`)**, from the published format:
  the DSN in µm at 0.1 µm, Y up; layers, the largest outline loop as the boundary and the
  others and non-plated holes as keep-outs, the rule and the via padstacks (KiCad-style
  `Via[0-1]_600:300_um` names), one image per footprint placed at its origin unrotated on the
  front with pins where the board has them (bottom-side pads keep their layers in their
  padstacks, so no rotation or mirroring convention of the reader matters), padstacks by shape
  (circles, axis-aligned rectangles, else polygons), nets with `REF-PIN` pins (repeated pad
  numbers get `@n`), net classes with width, clearance and via, and the tracks and vias
  already laid. The SES reader takes any `(resolution unit n)`, the session's padstack sizes
  and our via names for drills; it is bounded (64 MiB, depth 64, 8 M nodes) and fuzzed
  (`ses_read`). Applying a session replaces the routes (or keeps them) in one undoable edit;
  layers the board lacks are refused, nets it lacks are reported.
* **Topola** (codeberg.org/topola, crates.io 0.1.0): every source file carries
  `SPDX-License-Identifier: MIT OR Apache-2.0` (its `topola_specctra` parser MIT) and its
  dependencies are permissive, so it would vet; but 0.1.0's autorouter is a stub (the
  `Autorouter` type wraps a router of 26 lines and routes nothing), its API is unstable and it
  brings about a hundred crates (`spade`, `rstar`, `i_triangle` 0.40 with an older `i_overlay`,
  `rand` 0.10, `undoredo`, `derive_more`...). **Not integrated**; the DSN/SES hand-off works
  with any autorouter, Topola included once it routes. Freerouting (GPL) is never bundled: the
  user runs it on the DSN and imports the SES.
* **STEP (`pcb.export_step`)**: the board with its holes and the components (their STEP models
  from `models_dir`, else boxes), optionally outer copper, written by the kernel's STEP writer,
  each body named with its colour.
* **Editor (`pcb_view`)**: the Route tool starts on a pad, track end or via and takes its net;
  the preview walks around obstacles with the net class's width (cached obstacles and path); a
  click lays the previewed path (`pcb.route`, `around: false`), a click on a pad of the net
  finishes, V places a via where it fits and goes on on the next copper layer, `/` turns the
  bend, arc corners optional. Zone fills drawn filled (triangles cached by polygon contents).
  Texts drawn at their height and angle in the UI font (KiCad's stroke font is not used: its
  licence was not settled here; plotting texts would use `gusset-text` outlines). The picked
  track's or via's net is highlighted. Shift-click adds to the picks, a Shift-drag box picks
  what lies inside; Delete removes every picked track and via; dragging a picked footprint
  moves the others. A footprint browser lists a library folder (`Lib.pretty` folders, or one
  `.pretty` folder) for the Place tool (`pcb.add_footprint`: free reference `FP<n>`, angle).
* **Schematic**: moving a symbol moves the wire ends on its pins (straight); `sch.set_field`
  edits Value, Footprint, Reference (and its instances) or any field, from the panel too; a
  picked wire or label highlights its net, and the shown tab's highlighted net is highlighted
  in the other. Bus members: a bus sheet pin joins the child's member labels to the parent's
  of the same names; sheet pins on one parent bus join member by member in order (vectors of
  one width), named by the bus's label.

## Results of phase 2b (2026-11-12)
* Local check, not in the repository (CC-BY-SA data), release build, the boards' own rules
  without their project files: refilling the zones of the 7 demo boards gives areas within 0.1
  to 1.7% of KiCad's own fills (ecc83-pp 1256 vs 1254 mm², interf_u 5622 vs 5570,
  pic_programmer 10726 vs 10619, complex_hierarchy 5445 vs 5356, the 4-layer coldfire board
  29982 vs 30018, video 40770 vs 40959), StickHub 12% less (639 vs 725: its project file's
  rules are not read here), and **no zone DRC finding** on any of them (KiCad's stored fills give 726 on StickHub against our rules without
  its project file); the largest (video, 2 zones of 19 000 mm²) fills in 2.3 s, its 258 000
  vertices triangulate in 0.7 s, and every triangulation covers its fill exactly. Routing pad
  pairs of 30 nets on F.Cu with every other track in place: ecc83-pp 9 of 9, pic_programmer
  30 of 30 (the slowest 28 ms), video 8 of 30 (a dense, fully routed board: the others have no
  way on one layer). The video schematic now gives 2021 of 2046 pads KiCad's net (was 1300);
  the other four designs still match every pad.
* Not checked: Freerouting on our DSN (running a downloaded program was not allowed here); the
  DSN follows the published format and round-trips through our SES reader in tests.
* Tests: `gusset-pcb` 58 (new: `tests_zone` 8 — polygon booleans, offsets and exact
  triangulation of plates with many holes, zone settings, fill areas against hand-computed
  values for an empty zone, a crossing track, a thermal relief with spokes, solid and no
  connection, priorities and keep-outs, written fills read back and passing the DRC, a bad
  fill caught as a short; `tests_route` 4 — free 45° routes, walkaround round a wall passing
  the DRC and completing the ratsnest, arc corners written and read, blocked ends and via
  checks; `tests_dsn` 3 — DSN structure, routes back from a session at another resolution,
  hostile sessions; `tests_sch` 2 — wires following a symbol and field edits, bus members);
  `gusset-commands` `tests_pcb_layout` 5 (fill with undo and Gerbers, STEP target checks,
  route, DSN/SES with undo and agent path rules, footprint placement) and the `sch.set_field`
  steps; `gusset-app` `tests_pcb` 3 more (route previews and finishing, cached fill triangles,
  box select, multi delete and the footprint browser) and highlighting and field edits in
  `tests_sch`; OCCT `gusset-cli/tests/pcb_import.rs` 1 more (the board as STEP read back
  through the import worker: exact board volume, labels, colours). Fuzz target `ses_read`.
* MCP: core tool list unchanged (31 tools, 17901 of 18000 bytes); the command tools average 568
  bytes (160 tools; was 570); the largest new one is `pcb_route` at 798 bytes.

## Phase 3 (2026-11-13): the workflow complete from nothing
Goal: a user who starts from an empty session can draw a schematic, bring it to a board, give
it nets and rules, route, pour, check and write every fabrication file without KiCad.
* **Nets (`gusset-pcb` `nets`; `pcb.set_net`, `pcb.rename_net`)**: pads take a net by
  `REF.PAD` (every pad of that number on the footprint; a new name declares the net, none
  takes the pad off); a net is renamed on pads, tracks, vias, zones (`net_name`) and KiCad 5
  `add_net`, in numbered (KiCad 6 to 9) and named (KiCad 10) files; renaming onto an
  existing name merges the two (numbered files: the old declaration goes, references take
  the other number). The project's class assignments follow a rename.
* **Net classes (`pcb.net_class`)**: clearance, track width, via size and drill, nets
  (KiCad 8+ `netclass_assignments`) and name patterns, or delete (not `Default`). Kept where
  KiCad keeps them, in the `.kicad_pro` beside the board: a board without one gets a minimal
  project (`meta`, `net_settings`), stored in the history like the board; `pcb.save` writes
  it beside the `.kicad_pcb`. The rules the router, fills and DRC use are read back from it
  as for an opened project.
* **Zones (`pcb.add_zone`, `pcb.delete` zones)**: a polygon on one or more copper layers,
  net, clearance, pad connection (thermal, solid, none), priority, solid or **hatched**
  (bar width, hole size, angle), written as KiCad writes zones and filled at once.
  **Hatched fills**: after the solid fill and its spokes (steps 1 to 5 above), square holes
  of the hole size on a grid of hole + bar, turned by the angle, are cut where they lie
  inside the fill shrunk by the bar width (the border) and away from the zone's own pads and
  vias (grown by the thermal gap and the wider of spoke and bar), so every connection the
  solid fill made stays; holes cut to less than `hatch_min_hole_area` of a whole one stay
  copper; islands are removed after hatching. At most 200 000 holes per fill (else solid,
  noted). KiCad's own smoothing and border options are written with defaults, not applied.
* **Texts plotted (`font`)**: KiCad's stroke font was not used (its licence was not settled
  here, and its source is GPL). Gusset has **its own stroke font**, drawn for this work:
  printable ASCII as polylines on a 7 × 13 grid (cap height 9, x-height 6, descenders 3),
  advance = glyph width + 3, unknown characters as a box. Board and footprint texts are
  plotted into Gerbers with a round pen of the text's thickness (else an eighth of its
  height) at their height and width, angle (counter-clockwise on screen), justification
  (left/centre/right, top/centre/bottom; KiCad's default centre) and mirroring, lines split at
  `\n`; `${REFERENCE}`/`${VALUE}` (and KiCad 5 `%R`/`%V`) take the footprint's. Copper texts
  are obstacles for fills and routes. The editor still draws texts in the UI font.
* **Push-and-shove, kept simple (`shove`)**: when the walkaround finds no way, the route is
  searched as if other nets' tracks on the layer were not there; the tracks it would cross
  must all belong to one **chain** of one net (segments joined end to end, straight, one
  width, two free ends at pads, vias or branches). That chain is taken up, the new track
  laid (searched again without the chain), and the chain laid again between its two ends
  around the new track (and the legs of the new track laid before); the result is kept only
  when the DRC reports no more problems than before, else nothing changes. Several blocking
  nets, branches or arcs are refused. `pcb.route` shoves by default (`shove: false` turns
  it off); the track and the moved one are one undoable edit; the result says how many
  tracks were shoved. Moving a track's segments a little aside while keeping its shape
  (true shove), and shoving vias, are later work.
* **Freerouting as a separate process (`pcb.autoroute`)**: the user names their own
  install (a `freerouting.jar`, run with `java -jar`, Java optional; or an executable) and a
  time limit in the PCB panel's Autorouter section (saved in the window preferences, never
  settable by agents; the app hands it to the session). The command writes the board's DSN
  into a private temporary folder (0700), runs `<freerouting> -de board.dsn -do board.ses
  -mp <passes>` with no stdin and its output to files there, polls it, kills it at the time
  limit or when the session's cancel flag is set (the status bar's Cancel), reads the SES
  back (64 MiB cap, the fuzzed reader) and lays it, replacing the routes, in one undoable
  edit; the folder is removed. Not set up: `not_available`, nothing run. Agents need the
  user's approval (it starts a program). Gusset never downloads, bundles or finds
  Freerouting by itself (GPL; ADR 0001). Tested with fake routers (shell scripts in a temp
  folder): success, failure with its stderr, timeout, cancel, a missing program.
* **Schematic: `sch.annotate`**: symbols whose reference has no number (`R?`, `#PWR?`), or
  every symbol with `all`, get the next free number of their prefix across the design,
  sheet by sheet, left to right then top to bottom.
* **Editor**: a Zone tool (click corners; double click, Enter or the first corner closes;
  the panel's net, hatch, bar and hole); a Nets section: the picked footprint's pad nets as
  editable fields (`pcb.set_net` on leaving a field), renaming the highlighted net, net
  classes (pick or name one, sizes, add the highlighted net, delete), and the Autorouter
  (path, Java, time limit, Autoroute; Cancel in the status bar while it runs). A Route
  click where the preview finds no way asks `pcb.route` to shove (the route ends there). The ribbon's
  PCB tab lists the new commands; the Schematic tab Annotate.
* **End to end (`gusset-commands` `tests_pcb_flow`)**: from an empty session: a symbol
  library (resistor, connector, `+5V`, `GND`, `PWR_FLAG`), `sch.new`, symbols placed as
  `R?`, wired and labelled, ERC (unannotated) → `sch.annotate` → ERC clean, netlist; `pcb.new`,
  `pcb.update_from_schematic` (footprints from a `.pretty` folder, nets), placement, a test
  point placed by hand and put on a net with `pcb.set_net`, a `Power` class (0.5 mm) for
  `+5V`/`GND`, routes (the class width taken), a stub and via into a hatched `GND` zone on
  B.Cu (filled at once; the through-hole connector joins it), 0 unrouted, DRC clean;
  Gerbers (silkscreen references plotted, the zone a region), drill (3 plated holes), BOM,
  placement; STEP refused cleanly by the mock kernel (the real kernel's board STEP is
  `gusset-cli/tests/pcb_import.rs`); `pcb.save` (board and `.kicad_pro`) and `sch.save`,
  opened again: the same tree, the class read back, nothing unrouted, the board in step with
  the schematic; then rename, merge, undo, class delete, zone delete. Gaps found and fixed
  on the way: no command set a pad's net, made a zone, edited classes or annotated; the
  board's project file (classes) was not saved.

## Results of phase 3 (2026-11-13)
* Local check, not in the repository (CC-BY-SA data), release build (`tests_local`, run with
  `GUSSET_KICAD_SAMPLES`): on the 7 demo boards every net renamed and renamed back gives the
  identical tree; Gerbers with every shown text plotted take 1 to 66 ms (video: 541 texts).
  Routing pad pairs of up to 30 nets on F.Cu with every other track in place, as in phase
  2b: walkaround as before (ecc83-pp 9/9, pic_programmer 30/30, interf_u 27/30, StickHub
  14/24, complex_hierarchy 28/30, coldfire 7/30, video 8/30); the shove adds 2 on
  complex_hierarchy and 1 on coldfire, each with the DRC no worse; on video and the others
  the blocked pairs are blocked by several nets or by pads, which this shove does not move.
  Slowest board 3.6 s for its 30 pairs (video).
* The stroke font drawn and checked by eye (every glyph rasterised): legible capitals,
  lowercase, digits and punctuation.
* Tests: `gusset-pcb` 64 (new `tests_phase3` 4 — a shove in a closed channel with the DRC
  clean after and both nets joined, two blocking nets; pad nets, rename, merge, KiCad 10
  named nets; zones added, solid and hatched fills (area against the hole grid), hatch read
  back, delete; texts in Gerbers, placement, turning, field variables, copper text as a fill
  obstacle; `font` glyph grid check; `tests_local` skipped without samples);
  `gusset-commands` `tests_pcb_flow` 3 (the end-to-end flow above; a shove through
  `pcb.route` undone in one step; Freerouting faked); `gusset-app` `tests_pcb` 1 more (zone
  tool, pad-net, class and autorouter commands from the panel, the setting reaching the
  session) and the preferences round trip with the Freerouting setting. Fuzz target
  `pcb_nets` (renames, pad nets, fills with hatching, the stroke font on any board).
* MCP: core tool list unchanged (31 tools, 17971 of 18000 bytes); the command tools average
  577 bytes (168 tools; was 568); the largest new one is `pcb_add_zone` at 1005 bytes.

## Next (out of scope of this ADR's work; each needs its own ADR before work)
* **Routing**: a true shove (segments moved aside keeping their shape, several tracks and
  vias pushed in a chain: done, ADR 0058), dragging tracks (with the push of ADR 0058), differential pairs and length tuning (done, ADR 0055), blind and
  buried vias, routing across layers in one search; a larger walkaround window for dense
  boards (video: 8 of 30 pad pairs). Freerouting runs from the user's setting (phase 3);
  finding it on the PATH is not done on purpose (the user names the install).
* **Zones**: several outlines, spoke rules with `starved_thermal`, filling on edit and a
  zone outline editor: done, see ADR 0053. Still open: hatch smoothing and border options.
* **Schematic, after phase 2b**: wires dragged orthogonally with symbols, multi-select and
  box select, annotation options: done, see ADR 0052. Still open: bus aliases and
  KiCad's naming of bus member nets in every case (25 of 2046 video pads differ in the name
  chosen); a symbol library
  browser and sheet creation and editing (done, see ADR 0057); KiCad 5 `.sch` files;
  back-annotation from the board (`kicad-cli sch erc` for the authoritative check: done,
  ADR 0056).
* **KiCad library content pack** (footprints, symbols, 3D models; CC-BY-SA-4.0 with exception) as
  an optional download with notices; placing footprints from it.
* DRC: hole-to-hole, courtyard overlap, silk on pads and drill sizes are done (ADR 0048);
  per-class rules beyond clearance and a subset of KiCad's custom rules (`.kicad_dru`): done, see
  ADR 0054. Still open there: area functions, `severity`, `B.` in single-item constraints.
* Editor: (texts in our stroke font, mirrored on flip, adding and editing board texts: done,
  see ADR 0049; a pad picked on its own, moving tracks and vias, and the board outline drawn
  with the mouse: done, see ADR 0050; arc tracks drawn freehand, a pair routing tool and a
  length tuning tool: done, see the ADR 0050 amendment).
* From phase 1a: Gerber object attributes (`.N`, `.P`, `.C`), a test-only re-reader and a local
  corpus test: done, see ADR 0051.
* **KiCad IPC bridge** (protobuf over NNG to a running KiCad, after the `.proto` licence is confirmed).
  The `kicad-cli` wrapper for authoritative DRC/ERC and STEP/IPC-2581/ODB++ export is done, see
  ADR 0056 (still to verify against a real KiCad).

## Consequences
* Users can bring a KiCad board into Gusset to design an enclosure around it, and write fabrication
  files from Gusset. Gusset stays MIT OR Apache-2.0; `cargo deny` gates every new crate.
* KiCad's format moves with every release (`version` numbers): the reader keeps unknown nodes and
  must be checked against new versions (a local corpus test is a Next item).
* Conventions read from samples, not from KiCad code, are recorded here so they can be checked:
  footprint and pad angles are counter-clockwise on screen with Y down; pad `at` angles in the file
  include the footprint's rotation; bottom-side footprints are stored already mirrored; KiCad 5
  `(model (at ...))` is in inches, `(offset ...)` in mm. The 3D model rotation order and sign,
  and the trapezoid `rect_delta` direction, are **unconfirmed** against KiCad's output.

## Results of phase 0 + 1a (2026-11-12)
* Tests: `gusset-pcb` 22 unit tests (hand-written boards: round trip, KiCad 5 and 10 layouts, Gerber
  layers, Excellon, sanity check, profile, mock-kernel build, hostile inputs); `gusset-commands`
  `tests_pcb` 5 (info, import, STEP models through a fake importer, model path confinement,
  exports with the agent rules); OCCT `gusset-cli/tests/pcb_import.rs` 2 (board volume with a rounded
  outline, cut-out, holes and a slot within 1e-3; copper on the faces; a STEP model through the real
  worker replaces its box at the footprint's place). Fuzz target `pcb_read`.
* Local check, not in the repository (CC-BY-SA data): 7 KiCad demo boards (KiCad 9 and 10 files, 2 and
  4 copper layers, up to 8000 tracks) and 2 library footprints read, written back with an identical
  tree, and plotted; rasterised Gerbers looked right (zone fills with thermal reliefs, arcs, outlines).
  The largest (5.8 MB) reads, round-trips and plots in 0.3 s (release).
* `cargo deny check`: advisories, bans, licences and sources ok; the new crates add no duplicates.
* MCP: the four commands are not in the core tool list (17901 of 18000 bytes, unchanged); the
  command tools average 593 bytes (cap 600).

## Alternatives considered
* Embedding or porting KiCad (or LibrePCB/Horizon): the combined work would be GPL (ADR 0001).
* `kiutils-rs` (MIT) for the S-expression layer: young (about a thousand downloads), and the core model
  should be our own and fit the workspace lints and limits; kept as a reference for behaviour only.
* Our own Gerber writer instead of `gerber-types`: small, but the crate vets clean and covers the X2
  attribute vocabulary.
