# ADR 0059: PCB routing across copper layers in one search

Status: accepted (2026-11-14)

## Context
ADR 0058 measured what remains unroutable on one layer: of the demo boards' blocked pad
pairs, 16 on video would cross whole parallel buses and 24 (coldfire, StickHub) have no
way on F.Cu even with every other track gone. Its conclusion: routing across layers in
one search is what these boards need next (ADR 0047's Next). It also measured the editor's
push preview at about 0.7 s per blocked pair on dense boards, paid on the UI thread.

Written clean room from the general published idea of multi-layer maze routing (a grid
per layer, a layer change as a step with a via cost); no KiCad or other GPL router code was
read.

## Decision
**Layered search** (`gusset-pcb` `layers`: `Layered::new`, `route`, `via_size`):

* One octilinear grid per routable layer over the window of the one-layer walkaround
  (ADR 0047); a point is free on a layer as there. A step to the same point on another
  layer places a **through via** (blind and buried vias stay out of scope); it costs as
  much as 2 mm of track (at least 8 grid steps). A* with the octile distance, plus one via
  where the layer is not one the route may end on.
* **Via size**: the net class's diameter and drill (ADR 0054), else the board's, raised to
  the minimum drill and diameter and to drill + twice the minimum annular ring.
* **A via site** must keep the clearance (the larger of the two nets' classes) to other
  nets' copper on every copper layer, routable or not; the hole-to-hole gap (ADR 0048,
  the larger of the board's and the custom rules') to every drilled hole of any net,
  including the route's own other vias; the hole-to-edge gap; stay off surface pads (own
  net too: no via in pad) and out of keep-outs that forbid vias.
* **Thermal reliefs**: tracks and vias keep clear of other nets' zones' thermal pads by the
  clearance, the relief gap and a spoke (or the zone's narrowest copper), so the zone,
  poured again, still ties its pads in. Without this a B.Cu track on interf_u cut the
  spokes of seven GND pads (the check counted one more problem after pouring again).
* Zone fills themselves are not obstacles (as for every route since ADR 0047: they are
  poured again; filling on edit, ADR 0053, does it in the same step).
* Bounds: at most 3 000 000 grid points over all layers (1 500 000 per layer, the grid
  coarsens as in the walkaround) and 1 500 000 expanded; cancellable (checked every 1024
  expansions). Deterministic (index order, ties broken by state number).
* The grid path is cut into legs at the vias; each leg is pulled tight on its layer.

**Command** `pcb.route` (description unchanged): a `layers` option, `"any"` (every copper
layer) or a list (`"F.Cu,B.Cu"`; `layer` is always included). Default: unchanged
behaviour, except that when the end's copper of the net is only on other layers (an SMD
pad on the back), those layers are allowed. The start may begin on any allowed layer its
copper is on (THT pads). Per leg: the walkaround on the current layer (when the leg may
start and end there), then the layered search, then push-and-shove and the phase 3 shove
on the current layer (ADR 0058: pushes stay per layer). The new tracks on each layer and
the vias are one edit and one undo step; the result adds `vias`, and `first_via` and
`via_size` when there are any. Schema: the new field's doc is 43 bytes; other field docs of
`pcb.route` were shortened so the pcb toolset stays under its cap.

**Push preview off the UI thread** (`gusset-app` `pcb_view::push_preview_async`): the
Route tool's push preview runs on a worker thread holding the board's `Arc`; at most one
job runs; a new target sets the running job's cancel flag (`push::push_cancel` gives up at
its next checkpoint: between phases and every 4096 search steps) and starts once it has
ended; the result is kept for its target and shown when ready (a dim line meanwhile, with
a repaint asked for). Dropping the editor state cancels the job.

**Incremental, exact acceptance check**: a push is kept when `drc::count_near` (the
violations involving tracks, vias or holes whose box meets the box of the copper that
changed, `refill::changed_box`) is no higher after than before. Every violation outside
that box involves only unchanged items and is in both boards, and the checks no track or
via affects (fills against fills, starved thermals, courtyards, silkscreen) are left out of
both, so the difference of the two counts equals the difference of the whole checks: the
"no worse" guarantee is unchanged. Also, the push's line model found the pad under each
track end by scanning every pad; it now sweeps pads sorted by x (250 → 63 ms on video).

## Results (2026-11-14)
Local check (`tests_local::local_samples_layers`, `GUSSET_KICAD_SAMPLES`, KiCad demo
boards, CC-BY-SA, not in the repository), release build: the pad pairs of ADR 0058 (up to
30 nets per board, two pads on F.Cu, every other track in place, each pair alone on the
board as it is), now on F.Cu and B.Cu. Order: walkaround on F.Cu; the layered search;
push-and-shove, then the phase 3 shove, on F.Cu. Each layered route is laid, the zones it
touches are poured again on the board before and after, and the check must find no more
problems after (asserted; worst case equal on every board). Times are the router's for all
the board's pairs: walkaround / layered search / push and shove.

| board | pairs | DRC | walkaround | + layered (vias) | + push / shove | routed (ADR 0058) | time |
|---|---|---|---|---|---|---|---|
| ecc83-pp | 9 | 0 | 9 | 0 | 0 | 9 (9) | 2 ms |
| pic_programmer | 30 | 4 | 30 | 0 | 0 | 30 (30) | 0.15 s |
| interf_u | 30 | 67 → 67 | 27 | +3 (2) | 0 | **30** (27) | 0.41 / 0.05 s |
| StickHub | 24 | 1438 → 1438 | 14 | 0 | 0 | 14 (14) | 0.03 / 0.07 / 0.39 s |
| complex_hierarchy | 30 | 45 → 45 | 28 | +2 (0, on B.Cu) | 0 | 30 (30) | 0.17 / 0.01 s |
| coldfire (kit-dev-coldfire-xilinx_5213) | 30 | 206 → 206 | 7 | +5 (8) | 0 | **12** (8) | 0.28 / 0.41 / 1.37 s |
| video | 30 | 12 → 12 | 8 | +11 (16) | +2 (pushed) | **21** (11) | 0.83 / 3.23 / 4.70 s |

* coldfire gains 4 pairs and video 10 over ADR 0058, with 24 vias in all and no DRC
  regression; interf_u now routes all 30. On complex_hierarchy the two pairs ADR 0058
  pushed (or re-laid) on F.Cu run on B.Cu from their THT pads without moving anything.
* What still fails: coldfire 18 (`the end is not clear on its layers`: the pad's centre
  is closer to its neighbours than the track's half width plus clearance, fine-pitch pads
  with the class's track; ADR 0058 found the same 18 with no way even with every other
  track gone: they need the track necked down near the pad); video 9 (the PCI bus nets
  EA5, EA15, EQ1–EQ7: no way on the two outer layers within the window; push-and-shove
  on F.Cu does not reach them either); StickHub 10 (9 no way, 1 end not clear).
* Speed: the push alone on video averages 0.41 s per pair tried (22 pairs, most of them
  refused after the crossing search), 0.2 s for one that succeeds (line model 63 ms, check
  about 0.14 s); ADR 0058's preview paid about 0.7 s per pair, two whole checks included.
  The editor pays it on a worker thread now, the UI thread none of it.
* ADR 0058's measurement (`local_samples_phase3`) gives the same counts as before (14, 30,
  9, 27, 8, 30, 11) with the incremental check; for every push kept on the demo boards the
  windowed and whole differences are asserted equal.
* Tests: `gusset-pcb` `tests_layers` 6 (a route forced under a wall through two vias,
  F → B → F, DRC unchanged, joined, deterministic; an end on B.Cu takes one via; vias keep
  their clearance to a B.Cu track and an F.Cu pad; class via sizes and minimums; the
  hole-to-hole gap; refusal when no via site clears, with one layer, and when cancelled);
  `tests_push` 2 more (the windowed check against the whole one for better, worse and
  equal edits; a cancelled push); `gusset-commands` `tests_pcb_flow`
  (`layers: "any"` dives under a wall with two vias of the default size, DRC clean, one undo
  restores the board; an end on B.Cu routes with one via unasked; a bad layer refused);
  `gusset-app` `tests_pcb` (the threaded preview: waiting then ready, equal to the push
  computed in place, kept for its target, a moved pointer cancels and the new result
  arrives).
* MCP: `pcb.route`'s description is unchanged (72 bytes); the pcb toolset is 24 tools,
  14 395 bytes (599 per tool, cap 600; 14 360 before).
* Suites: `gusset-pcb` 142, `gusset-commands` 303, `gusset-app` 196 (1 ignored),
  `gusset-mcp` 40 + 34, all passing (the test-only Gerber re-reader included);
  `cargo clippy --workspace --all-targets -D warnings` clean.

## Consequences
* Two-layer routing is on by default only where the end needs it; callers ask for more
  with `layers`. Inner layers can be routed (`"any"`) but planes are not avoided beyond
  the thermal reliefs: a route on an inner plane layer cuts the plane (poured again).
* Push-and-shove still moves copper on one layer; a push that would use the other layer is
  not tried. The editor's Route tool previews the one-layer walkaround and the push; its
  click asks `pcb.route`, which uses the layered search only where the end is on another
  layer (a manual layer change places a via as before).
* `push::push_with` (the known check count) is gone: the incremental check needs no count
  of the whole board.

## Alternatives considered
* Routing each layer separately and joining the pieces with a via at a guessed place:
  misses ways that change layer more than once and puts vias where the search would not.
* A full check after each layered route (as pushes do): about 0.2 s more per route on
  video, and it would still need the reliefs to find routes the check accepts; the vias'
  and tracks' clearances hold by construction.
