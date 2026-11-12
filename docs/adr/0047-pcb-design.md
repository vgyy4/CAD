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

## Next (out of scope of this ADR's work; each needs its own ADR before work)
* **Phase 1b:** board in the `gusset-doc` model (history, undo, code view) instead of bodies only; STEP
  export of the board; text on silkscreen (KiCad stroke font is CC data, an own font or the
  `gusset-text` outlines); Gerber object attributes (`.N`, `.P`, `.C`); pick-and-place CSV and BOM;
  `.kicad_sch`/`.kicad_sym` readers; re-reading our Gerbers with a permissive parser in tests;
  a fuzz target for the reader; a corpus test over local KiCad sample boards.
* **DRC** (clearance, width, drill, annular ring, edge) as a pure function.
* **Phase 2: interactive 2D layout editor in the egui app** (board canvas in `gusset-render`,
  select/move footprints, routing with push-and-shove-lite, zone fill).
* **Schematic capture** (after layout works; netlist import from KiCad/atopile until then).
* **Routing/autorouting**: Topola (MIT, Rust) when mature, else Specctra DSN out and Freerouting as an
  external GPL process.
* **KiCad IPC bridge** (protobuf over NNG to a running KiCad, after the `.proto` licence is confirmed)
  and a `kicad-cli` wrapper for authoritative DRC/ERC and STEP/IPC-2581/ODB++ export.
* Library handling: KiCad libraries as an optional downloadable content pack with notices.

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

## Results (2026-11-12)
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
