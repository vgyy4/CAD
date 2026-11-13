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
  mid stays). One command is one undo step; indices past the lists, an empty request, a
  non-finite offset or a place beyond 10 m are refused and leave the board untouched.
* **Editor drag**: dragging a track or via with the Select tool moves it (and every other
  picked track and via when it is one of them), the drop snapped by the track's start or the
  via's centre to the grid; the preview draws the items where they would land, with ends
  on a dragged via following. A drag that lands on the same place sends nothing.
* **Outline tool**: clicks collect corners at the grid; a click on the first corner (3+
  corners), a double click or Enter sends `pcb.set_outline` with them; Esc or leaving the
  tool drops the corners. The outline so far is drawn in the Edge.Cuts colour with a rubber
  band to the pointer. The command replaces the board's outline (rectangles stay `gr_rect`).

## Amendment (2026-11-13): edits that keep connections
* **Moving a track stretches its neighbours** (`pcb.move`, `attached` default true, the
  existing flag): the end of a track that is not moved and lies within 1e-4 mm of an end of
  a moved track, on the same layer and net, follows that end by the same offset, so the
  neighbour stretches (an arc neighbour keeps its mid). Rule kept plain: only coincident
  ends follow (no end on the middle of a segment, no tracks of other layers or nets, no
  chains: a neighbour's far end and its own neighbours stay), vias never move with a track
  (a via at the moved end stays; the moved track's end leaves it), and tracks named in the
  request never count as neighbours. `ends_carried` counts the ends moved by vias and
  tracks together. `Board::followers` is the one place that decides this, also used by the
  editor's preview.
* **Moving a footprint carries the track ends on its pads** (`pcb.move_footprint`, new
  `carry`, default true, false disables): a track end on a pad of that footprint (on a layer
  of the pad, same net, inside the pad's copper or at its centre) keeps its place relative to
  the pad: moved with it, and turned about the pad centre by the angle change when the
  footprint is turned (`angle`). A flip carries ends by the pad's displacement only (the
  mirror is not applied to the offset). The result has `ends_carried`. Pads are matched by
  their position before the change (`Board::footprint_pads` / `pad_ends`, then
  `carry_track_ends` after); each end moves once, arc mids stay. The place-from-library
  command does not carry (a new part has no tracks).
* **Editor**: dragging a track or via previews the neighbours' stretched ends, dragging a
  footprint previews the ends on its pads following the drop (`pcb_view::drag_preview`); the
  drop is the same single command, so one undo step. Rotating with the key also carries.

## Amendment (2026-11-13): arc segments, the Pair tool and the Tune tool
All three send existing registry commands (ADR 0012), one undo step each; the status bar shows
a refusal (`pcb.route_pair` and `pcb.tune` also report their result there).
* **Arc segments** (Route tool). The panel switch "Arc segments (A)" or the `A` key makes each
  segment an arc: after the route has started, one click gives a point the arc passes through
  (snapped), the next its end (a pad of the route's net ends the route, as for straight
  segments); start, through and end are three points on a circle, and the end becomes the next
  segment's start. The preview is a guide line to the pointer, then the arc through the point
  to the pointer (a red line when the three points are collinear, which no arc fits). The
  command is `pcb.add_track` with the new optional `mid` (an arc piece through `Board::add_route`;
  collinear points are refused with `invalid_args`), with the route's net. This is not "Arc
  corners" (that setting rounds the corners of a `pcb.route` by radius); the two are independent.
  The arc is a three-point arc, not constrained tangent to the previous segment.
* **Pair tool.** The first click must be on a pad whose net is half of a pair (ADR 0055
  recognition: `pair::find`); it names the pair (and highlights its positive net). Later clicks
  add the coupled centre line at the grid (the first one where the coupled part begins; the
  fan-out to the pads is the command's). Double click or Enter sends `pcb.route_pair`
  `{pair, points, layer}` (class width and gap); Esc, a right click or leaving the tool drops
  it. The preview is `pair::route_pair` itself, both offset tracks drawn at their width, cached
  by (board, layer, pair, points), so what is shown is what the command lays; when it cannot be
  laid, its message shows at the bottom of the canvas and in the panel, and a send that fails
  reports in the status bar. Only the first pad click names the pair; there is no pair picker
  in the panel.
* **Tune tool and lengths.** A click on a track picks it (and highlights its net). The PCB panel
  shows, for the highlighted net in every tool, the net's length with tracks and vias, and for a
  pair the partner's length and the skew (`gusset-pcb::length`, the sums `pcb.lengths` reports,
  computed in the app from the open board instead of one command round trip per frame). With the
  Tune tool the panel adds a target (set to the net's length when the net changes), "Match
  pair" (no target is sent; `pcb.tune` then tunes the shorter of the two, which is the partner
  when the picked net is the longer), amplitude and spacing (0 = the net's clearance, not sent),
  and Apply sends `pcb.tune {net, target?, amplitude, spacing?, track?}` with the picked track
  when it belongs to the net (else the command chooses).
* **Budget.** `pcb.add_track` gained `mid` (the description is unchanged): the `pcb` toolset is
  14360 of 18000 bytes (24 tools, 598 per tool, the limit being 600); modeling is unchanged.

## MCP budget
`pcb_move` is in the `pcb` toolset. The `pcb` toolset went from about 13.6 KB to
14171 of 18000 bytes (24 tools, 590 per tool); modeling stays byte-identical at 17971.
With the amendment (`carry` on `pcb_move_footprint`, a longer description) `pcb` is 14291 of
18000 bytes (595 per tool). The modeling list is unchanged by it (the test prints 17996 for
it both before and after the amendment).

## Results
* gusset-pcb: 2 new tests (`tests_edit`: tracks and vias move with attached ends, with and
  without `attached`, read-back; refusals leave the board); 81 tests in all.
* gusset-commands: `tracks_and_vias_move_with_attached_ends_in_one_undo_step` (280 tests).
* gusset-app: `a_pad_is_picked_on_its_own_and_its_net_set_through_the_registry`,
  `tracks_and_vias_are_dragged_through_pcb_move` (through the app, one undo),
  `the_outline_tool_sends_set_outline` (183 tests). gusset-mcp 40 + 34.
* Amendment: gusset-pcb 2 new tests (`moving_a_track_stretches_its_neighbours`,
  `a_footprint_carries_the_track_ends_on_its_pads`: move, turn 90 degrees, layers, nets, an
  end inside the copper, nothing carried for another part; 89 tests in all); gusset-commands
  2 (stretch with undo; footprint carry with undo and `carry: false`; 284 tests);
  gusset-app `drag_previews_show_stretched_tracks`.

## Not done
* Moving zones and outline corners; tracks that only touch a pad edge-on without an end
  inside it are not carried; vias under a moved footprint's pads and the other pad of a
  dragged track are not followed; stretch is one level (no chains, no push-and-shove).
* No drag test drives egui pointer events end to end: the logic (hit, drop delta, command,
  preview sets) is tested as functions and the commands through the app, as for ADR 0049.
* A pad form through the app is not tested (the `pcb.new` board has no pads; `pcb.set_net`
  itself is tested with undo in gusset-commands).

## Results of the arcs, pairs and tuning amendment
* gusset-commands `add_track_takes_a_mid_point_for_an_arc`
  (undo; collinear refused; 294 tests); gusset-app `arc_segments_are_previewed_and_laid_through_add_track`,
  `the_pair_tool_previews_both_tracks_and_routes_the_pair`,
  `the_tune_tool_shows_lengths_and_tunes_through_the_registry` (preview geometry, command
  construction, runs through the app with undo and refusals in the status bar; 189 tests);
  gusset-pcb 120, gusset-mcp 40 + 34.
* Not done there: arcs constrained tangent to the previous segment, arc corners for pairs, a pair
  picker in the panel (the pair is named by its first pad click), tuning several tracks of a net
  or both nets of a pair together, a "tune" preview of the meander, and driving egui pointer
  events end to end.
