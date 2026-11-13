# ADR 0050: PCB editor interactions (pads, dragging tracks and vias, outline tool)

Status: accepted (2026-11-13). Follows ADR 0047 ("Next" → Editor) and ADR 0049: three
interactions the editor lacked, all through the single command registry (ADR 0012).

## Context
Pad nets could only be edited per footprint in the panel, a track or via could be picked
and deleted but not moved, and the board outline needed `pcb.set_outline` with typed
corners.

## Decision
* **Pads**: `Sel::Pad(index in board.pads)`. The Select tool picks a pad that lies on a
  shown copper layer (hit = inside its contour, or within the click tolerance of its edge).
  Order of picking: vias, tracks, board texts, pads, then the footprint box, so a click on
  a pad is the pad and a click beside it in the part's box is the part. A picked pad
  highlights its net and shows a small form (`REF.PAD`, net field, Apply or focus loss)
  that sends the existing `pcb.set_net` for that one pad (empty = no net). A drag that
  starts on a pad moves its footprint (as before, when a click on the pad picked the part).
  Pads without a number or a reference cannot be named for `pcb.set_net`; they are picked
  but show no form. Pads sharing a number in one footprint change together (the command's
  rule).
* **Moving tracks and vias**: new command `pcb.move` (`tracks`, `vias`, `by` in mm, `attached`
  default true) over `Board::move_items` in `gusset-pcb`. A track moves whole (start, mid of
  an arc, end); a via moves its place. With `attached`, a track not named whose end lies on
  a moved via (within 1e-4 mm, any layer) has that end moved too (only that end; an arc's
  mid stays). Ends of a moved track are not followed by their neighbours (the neighbour
  keeps its end, so the connection is left open; routing again or moving the neighbour
  mends it). One command is one undo step; indices past the lists, an empty request, a
  non-finite offset or a place beyond 10 m are refused and leave the board untouched.
* **Editor drag**: dragging a track or via with the Select tool moves it (and every other
  picked track and via when it is one of them), the drop snapped by the track's start or the
  via's centre to the grid; the preview draws the items where they would land, with ends
  on a dragged via following. A drag that lands on the same place sends nothing.
* **Outline tool**: clicks collect corners at the grid; a click on the first corner (3+
  corners), a double click or Enter sends `pcb.set_outline` with them; Esc or leaving the
  tool drops the corners. The outline so far is drawn in the Edge.Cuts colour with a rubber
  band to the pointer. The command replaces the board's outline (rectangles stay `gr_rect`).

## MCP budget
`pcb_move` is in the `pcb` toolset. The `pcb` toolset went from about 13.6 KB to
14171 of 18000 bytes (24 tools, 590 per tool); modeling stays byte-identical at 17971.

## Results
* gusset-pcb: 2 new tests (`tests_edit`: tracks and vias move with attached ends, with and
  without `attached`, read-back; refusals leave the board); 81 tests in all.
* gusset-commands: `tracks_and_vias_move_with_attached_ends_in_one_undo_step` (280 tests).
* gusset-app: `a_pad_is_picked_on_its_own_and_its_net_set_through_the_registry`,
  `tracks_and_vias_are_dragged_through_pcb_move` (through the app, one undo),
  `the_outline_tool_sends_set_outline` (183 tests). gusset-mcp 40 + 34.

## Not done
* A true drag that stretches the neighbours of a moved track (rubber-banding the track's
  ends along their neighbours), moving zones and outline corners, moving a footprint's
  attached tracks with it.
* No drag test drives egui pointer events end to end: the logic (hit, drop delta, command,
  preview sets) is tested as functions and the commands through the app, as for ADR 0049.
* A pad form through the app is not tested (the `pcb.new` board has no pads; `pcb.set_net`
  itself is tested with undo in gusset-commands).
