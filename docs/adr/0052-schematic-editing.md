# ADR 0052: Schematic editing: orthogonal wires, multi-select, annotation options

Status: accepted, implemented (2026-11-13). Follows ADR 0047 ("Next" → "Schematic, after phase 2b").

## Context
Moving a symbol dragged its wire ends straight to the new pin places (diagonal wires), only
one item could be picked, and `sch.annotate` had one fixed order. KiCad users expect
orthogonal wires, box selection and annotation options. Behaviour comes from KiCad's
documentation; no KiCad source was used.

## Decisions
* **Orthogonal following** (`gusset-pcb` `sch_edit`): a wire end on a pin (or on any point
  that moves: wire end, label, junction, no-connect of the moved selection) follows it. A
  horizontal wire keeps its row up to the new column and a vertical segment goes to the new
  place (vertical wires: the other way round). If the pin moves straight off the wire's end
  the wire stays and a segment is added. A wire that is neither (diagonal), or has both ends
  moving, or would shrink to nothing, is stretched straight as before. Added segments are
  deduplicated and skipped where an existing wire already covers them. Existing junctions
  stay; a junction is added where three wire ends meet or an end lands in a wire's middle
  (the `add_wires` rule; pins do not count there). The whole edit is one `Schematic` value,
  so one command is one undo step.
* **`Schematic::move_items(items, delta)`** translates symbols, labels, wires, buses,
  junctions and no-connects together; unselected wires on moved points follow as above.
* **`sch_move`** gained `by` (shift in mm, any item) and `with` (more items). With either,
  it calls `move_items`, one undo step ("Move selection"); `angle`/`mirror` stay single-item.
  `sch_delete` already took a list (one step).
* **Editor** (`sch_view`): Shift or Ctrl click toggles a pick; a drag on empty paper draws a
  selection box (left to right: items fully inside, blue solid; right to left: items touched,
  green dashed; a wire crossing the box counts); with Shift/Ctrl the box adds. Dragging any
  pick moves all picks (one `sch.move` with `by`/`with`, previewed live, wires too); Delete
  sends one `sch.delete`; R/X/Y act on a single pick only. Panning a sheet is now middle
  drag or wheel (a left drag on empty paper used to pan; other tools still pan with it).
  Symbols count by their body box. `State::picked()`: `selected` plus `extra`; `extra` is
  dropped when `selected` is set from outside (tree, board cross-probe).
* **Annotation** (`gusset-pcb` `annotate::plan`, a pure plan the command applies): `order`
  `x` (default; X then Y) or `y`; `sheet_step` (sheet n, the root file being 1, starts at
  n times it: 100 or 1000; numbers already used by kept symbols are skipped); `all`
  renumbers everything, otherwise numbered symbols keep their number. Multi-unit parts: the
  units of one part (same library symbol, same sheet) share one number. An unnumbered unit
  joins a kept or earlier unit group of the same library symbol and prefix that lacks its
  unit number, else starts a new number; with `all`, units that share a reference stay
  together. The reference field is stored without a letter (KiCad's way); the result names
  `U1A`, `U1B` (unit letter A to Z, then AA).
* MCP: `sch` toolset 9497 bytes (was about 9.2 KB; 18000 budget, 593 per tool), modeling
  list unchanged. New argument docs are one short line each.

## Tests
`gusset-pcb` +6 (orthogonal move incl. vertical wire and junctions, move of several items,
annotation orders, ranges and keeping, units); `gusset-commands` +2 (diagonal move and a
selection move with undo, delete of a selection, annotation options through the command,
unit letters); `gusset-app` +1 (box window and crossing, click and box picks, group commands
and a run with undo); `every_toolset_fits_the_budget` passes.

## Not done / Next
* Labels do not follow a single-symbol move (they are free text anchors); symbols moved
  with `at` still do not carry labels on their pins.
* Bus entries do not follow; wires crossing a moved wire's middle are not split.
* Rotate or mirror of a selection; copy/paste; annotation by sheet use (the same sheet file
  used twice gets one range); back-annotation.
* Pan with left drag on empty paper is gone in the Select tool (box select took it).
