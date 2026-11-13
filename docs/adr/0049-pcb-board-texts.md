# ADR 0049: Board texts in the PCB editor

Status: accepted (2026-11-13). Follows ADR 0047 ("Next" → Editor): texts drawn in the
editor in our own stroke font, mirrored on the back side, and added and edited through the
single command registry (ADR 0012).

## Context
ADR 0047 phase 3 gave Gusset its own stroke font (`gusset-pcb::font`; KiCad's is not used)
and plotted texts into Gerbers with it, but the editor still drew texts in the UI font and
there was no way to add or change a text. A board shown in the editor therefore did not
look like the Gerbers it produced, and silkscreen labels could not be made without editing
the file by hand.

## Decision
* **Drawing**: the editor draws every shown text with `font::strokes` (the same polylines
  the Gerbers plot): placed at its place, height, width, angle and justification, mirrored
  when the text is, with the pen thickness in screen pixels (at least 1). `${REFERENCE}` and
  `${VALUE}` take the footprint's. Texts under 3 px high are skipped. Footprint texts follow
  a footprint being dragged.
* **Mirroring**: `Text::mirrored` (KiCad's `(justify mirror)`) is the one truth for editor and
  Gerbers. New texts on a `B.*` layer are mirrored unless told otherwise; moving a text to
  the other side turns its mirroring with it; **flipping a footprint** toggles `mirror` in
  every `effects` of its texts (it did not before, so flipped silkscreen read the wrong way).
* **Tree position**: `Text` now records its place in the tree (`node`, `sub`), so an edit
  changes the node and re-reads, as every board edit does; what is not touched is written back
  as read.
* **Library** (`gusset-pcb::text_edit`): `Board::add_text` (a `gr_text`, KiCad 6+ or 5 layout,
  uuid when the file has them), `edit_text` and `delete_texts`. Texts are 1 to 1000
  characters (line breaks allowed), height 0 to 100 mm, thickness 0 to 100 mm (default 0.15 of
  the height, never above the height), any finite angle, places within 10 m, on a copper layer
  of the board or a `F./B.` SilkS, Mask, Paste, Fab, CrtYd, Adhes, `User.1`-`9`, `*.User` or
  `Margin`/`Edge.Cuts` layer. Footprint texts can be changed in size, thickness, angle,
  place (given on the board, turned back into the footprint's frame), layer, mirroring and,
  except the reference and the value, their words; they cannot be deleted. Justification is
  in the library (`NewText`/`TextPatch`) but not in the command (MCP budget).
* **Command** `pcb.text` (one command for the three edits, to spend the MCP budget once):
  no `index` adds (`text` and `at` required), `index` changes what is given, `index` with
  `delete: true` deletes a board text. One command is one undo step (`history.undo`).
  `pcb.info` now lists the board texts with their `index` (`board_texts`, words in
  `text_user_text`, at most 100) and the total in `texts`.
* **Editor**: tool **Text** (panel: words, height, angle, back side): a click on empty board
  adds a text on `F.SilkS`/`B.SilkS` at the grid point; a click on a board text picks it. The
  Select tool also picks board texts (`Sel::Text`; hit = distance to the strokes less half
  the pen). A picked text shows a form (words, height, angle, mirrored, Apply) sending
  `pcb.text`; Delete removes it.

## MCP budget
The `pcb` toolset was at 17994 of 18000 bytes. `pcb_text` took 1001 bytes at first (with
`justify`, now left out) and about 750 after. To make room, wording of other pcb and sch
commands was tightened without losing meaning (descriptions of `pcb.import`, `pcb.new`,
`pcb.drc`, `pcb.net_class`, `pcb.info`, the Gerber and drill exports, `pcb.update_from_schematic`,
`sch.delete`, `sch.set_field`, `sch.add_wire`, `sch.open`, `sch.erc`, `pcb.open`,
`pcb.fill_zones`, and field docs such as folder, path and default notes). Result: pcb toolset
17998 of 18000 bytes (34 tools), modeling unchanged at 17971 (byte-identical list).

## Results
* gusset-pcb: 6 new tests (`tests_text`: add with defaults and round trip, refusals leave the
  board untouched, edit/move to back/delete, footprint texts in the footprint's frame,
  flipping mirrors texts, mirrored strokes reflect about the place).
* gusset-commands: `pcb.text` add, edit, delete, undo/redo, refusals, `pcb.info` listing.
* gusset-app: `texts_are_picked_and_added_through_the_registry` (hit by stroke distance, hidden layers, tool commands, add/edit/undo through the app).
* Counts: gusset-pcb 70, gusset-commands 279, gusset-mcp 40+34, gusset-app 180 passed; clippy `-D warnings`, fmt and check-secrets clean.

## Not done
* Justification, bold/italic, knockout and text boxes in the command and editor; dragging
  a text with the mouse (change its place in the form or with `pcb.text`); editing the
  reference and value words (rename the part); deleting footprint texts (hide flag).
* Rendering text with the UI font for characters outside printable ASCII (drawn as a box,
  as in the Gerbers).
