# ADR 0058: PCB push-and-shove routing

Status: accepted (2026-11-14)

## Context
ADR 0047 phase 3 added a shove kept simple: when the walkaround finds no way, one chain of
one other net that blocks the new track is taken up and laid again around it. It moves the
whole chain (its shape is lost), refuses when several nets or a via are in the way, and
never moves vias. ADR 0047's Next asked for a true shove: segments moved aside keeping
their shape, several tracks and vias pushed in a chain.

This is written clean room from the general published idea of push-and-shove routing
(the new track is laid, and the copper it comes too close to is moved aside just enough,
which may move further copper in turn); no KiCad or other GPL router code was read.

## Decision
`gusset-pcb` `push` (`push::push`, `push::push_with` with a known check count, and
`push::push_reason` saying why there is none; `Board::apply_push`):

* **Lines.** The tracks on the layer are cut into lines: chains of straight segments of one
  net and width between anchors (pads, vias, branches, width changes, arcs, free ends).
  Lines of nets other than the new track's, and vias of other nets, are pushable; pads,
  arcs, lines on no net or of the new track's own net, the edge, keep-outs and copper on
  other layers are fixed. A line's two ends never move (unless it ends on a via that is
  pushed).
* **The head (the new track)** is searched with fixed copper as walls (the walkaround of
  ADR 0047, `route::walkaround_soft`) and pushable copper soft: grid points inside the
  gap the rules ask cost a penalty per step (2), so clear ways are preferred, and the
  centre line keeps 0.01 mm plus half a grid diagonal from pushable centre lines, so it
  may run over that copper but never across it. Shortcuts while pulling the path tight
  keep every gap; only grid steps come closer.
* **A push** lays a line clear of the placed copper (the head, and copper pushed so far)
  it comes too close to: each offending piece of that copper gets an octagonal hull (the
  piece grown by the clearance of the two nets, the two half widths and 2 µm; edges at
  0°, 45° and 90°), the hulls are joined (`poly::union`), and the line is walked round the
  outline of each joined hull between the points where it enters and leaves it. Of the two
  ways round, the one that does not close round a point of the pushers is taken (both:
  the line crosses the pusher, refused), the shorter when both are free. So the pushed
  part runs parallel at exactly the needed gap and joins the untouched rest at 45°: the
  line keeps its shape. A line end inside a hull is refused, unless the end is a pushable
  via, which is pushed first.
* **Chains.** A pushed line that now comes too close to another line or via pushes it in
  turn, breadth first, across nets: at most 8 pushes deep and 96 pushes in all (counting
  repeats), lines of at most 400 points.
* **Vias.** A via in the way moves the least distance (steps of 10 µm, up to 3 mm, away
  from the nearest pusher and in the eight directions) that clears the placed copper and
  every layer's fixed copper. Lines ending on it are re-joined at 45° on this layer and
  then pushed if needed; tracks on other layers are stretched with it (`Board::move_items`
  with `attached`, ADR 0050).
* **Crossings.** When there is no way without crossing other tracks, the head is searched
  again with crossing allowed at a high cost (40 per step); at most 4 crossed lines are
  laid again between their ends round the head and the copper placed so far (other
  pushable copper soft), then pushing goes on as above. This keeps the phase 3 idea for
  the cases pushing cannot do, for several nets at once.
* **Checks.** Every pushed line and moved via is checked against fixed copper; the whole
  result (vias moved, lines replaced, the head laid) is kept only when the design rule
  check finds no more problems than before, else nothing changes. Everything is
  deterministic (ordered maps, index order, no clocks).
* **Command.** `pcb.route` (description unchanged in size): walkaround, else push-and-shove,
  else the phase 3 shove, else `not_found`; `shove: false` still turns both off. The new
  track, the pushed tracks and the moved vias are one undo step; the result adds
  `shoved_vias` (`shoved_tracks` counts pushed lines).
* **Editor.** Where the Route tool's preview finds no way round, `push_preview` shows what
  a click would do: the new track, the pushed tracks (amber) and moved vias, cached per
  target like the walkaround preview; the click asks `pcb.route` to search, which pushes
  the same way.

## Results (2026-11-14)
Local check (`tests_local`, `GUSSET_KICAD_SAMPLES`, KiCad demo boards, CC-BY-SA, not in the
repository), release build: pad pairs of up to 30 nets per board routed on F.Cu with every
other track in place, each pair alone on the board as it is, as in ADR 0047 phase 3.
Counts are pairs routed; every push kept was checked to leave the board's DRC count no
higher than before (asserted). Times are for all the board's pairs: walkaround; the phase 3
shove for the pairs the walkaround missed; the new order (push-and-shove, then the phase 3
shove) for the same pairs.

| board | pairs | DRC | walkaround | + phase 3 shove | + push-and-shove (by pushing) | times |
|---|---|---|---|---|---|---|
| ecc83-pp | 9 | 0 | 9 | 9 | 9 (0) | 2 ms |
| pic_programmer | 30 | 4 | 30 | 30 | 30 (0) | 0.15 s |
| interf_u | 30 | 67 | 27 | 27 | 27 (0) | 0.49 / 0.05 / 0.77 s |
| StickHub | 24 | 1438 | 14 | 14 | 14 (0) | 0.03 / 0.16 / 0.41 s |
| complex_hierarchy | 30 | 45 | 28 | 30 | 30 (2) | 0.18 / 0.30 / 0.25 s |
| coldfire (kit-dev-coldfire-xilinx_5213) | 30 | 206 | 7 | 8 | 8 (1) | 0.31 / 1.64 / 4.10 s |
| video | 30 | 12 | 8 | 8 | **11** (3) | 0.85 / 2.60 / 16.9 s |

* video gains 3 pairs (pushing 2, 1 and 4 lines across nets), with the DRC no worse; on
  complex_hierarchy both pairs the phase 3 shove re-laid are now pushed keeping B's shape,
  and on coldfire one; no regressions anywhere.
* Why the rest fail (`push_reason`, and a walkaround on the board without any other
  net's tracks and vias): 24 pairs (coldfire 18, StickHub 6) have no way on F.Cu even with
  every other track and via gone: pads wall them in, no router on one layer can lay them.
  Of the others, 16 (video 15, the PCI address and data buses) would cross more than four
  tracks (whole parallel buses: they need another layer), 9 cross tracks that have no way
  round the new one (their ends are walled in), 2 meet fixed copper, 2 would make the
  check worse, 1 has no way. So the target "clear gains on complex_hierarchy, coldfire and
  video" is met on video only: on complex_hierarchy the phase 3 shove already routed all
  30, and on coldfire at most 5 more pairs were reachable on one layer at all, each
  needing tracks laid across walled-in ends. Routing across layers in one search is what
  these boards need next.
* Speed: a push builds the board without pushable copper and runs the DRC once on the
  result; on video that is about 0.7 s per pair tried (DRC and re-reading the board
  dominate). The editor preview pays this once per target point (cached).
* Tests: `gusset-pcb` `tests_push` 5 (a push keeps the line's ends, runs parallel at the
  gap with 45° joins, DRC no worse, A joined; a chain of three tracks of three nets; a via
  pushed with its front tracks re-joined and its back track stretched; refusal when a pad
  blocks; DRC never worse over six targets); `gusset-commands` `tests_pcb_flow` (a via
  pushed through `pcb.route`, DRC clean, one undo restores the board exactly);
  `gusset-app` `tests_pcb` (the preview shows the push, cached, and the click command).
* MCP: `pcb.route`'s description is the same length (72 bytes), its schema unchanged; the
  pcb toolset stays at 24 tools, 14 360 bytes (598 per tool).
* Suites: `gusset-pcb` 133, `gusset-commands` 302, `gusset-app` 195, `gusset-mcp` 47 + 34,
  all passing (the test-only Gerber re-reader included).

## Consequences
* Pushing keeps other tracks' shapes and topology; what it cannot do (crossings) falls back
  to laying crossed tracks again, then to the phase 3 shove.
* Interactive dragging of tracks (push while dragging) can reuse `push` with the dragged
  segment as the head; not done.
* Pushes run on one layer; vias moved are checked on every layer, tracks stretched on other
  layers are left to the final DRC check.

## Alternatives considered
* Offsetting each blocking segment perpendicular by a computed distance and re-joining its
  neighbours: equivalent for parallel runs, but the hull walk also handles slanted runs,
  corners and several pushers at once without special cases.
* Exact round hulls: they give arcs where tracks are octilinear; octagons keep 45° work.
