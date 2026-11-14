# ADR 0060: PCB neck-down near tight pads

Status: accepted (2026-11-14)

## Context
ADR 0059 left 18 coldfire pad pairs (and one StickHub pair) failing with `the end is not
clear on its layers`, diagnosed as fine-pitch pads whose centre is closer to their
neighbours than the track's half width plus the clearance: they would need the track
necked down near the pad. ADR 0047's Next lists it. ADR 0054 gives every net a minimum
track width (the board's, raised by its class's); the DRC's `track_width` rule checks it.

Written clean room from the general published idea of neck-down (a track narrows for a
short stretch at a tight pad and widens again); no KiCad or other GPL router code was read.

## What the measurement found first
On the coldfire QFP (0.3 × 1.6 mm pads, 0.5 mm pitch), the default rules (0.25 mm track,
0.2 mm clearance) leave 0.025 mm to spare along the pad's centre line: the full width
*does* fit. What failed was the grid: a grid point is free only with half a grid diagonal
to spare (0.08 mm on a 0.11 mm grid once the window coarsens), and the link from an end
searched 6 grid steps only, still inside the pad row. So ADR 0059's diagnosis was right
for few pairs; most needed a longer, exact escape from the pad, at full width. Both are
done here, as one search near pads.

## Decision
**Search near pads** (`gusset-pcb` `layers::route_necked`, `Layered::with_neck`):

* **Neck width** (`layers::neck_width`): the net's minimum track width (the board's,
  raised by its class's, ADR 0054), raised to the largest `min` of any custom
  `track_width` rule (conservative: a rule that does not match the net still raises it).
  Never below; when it is not below the track width there is no neck-down.
* **Neck radius** (`layers::neck_radius`): two times the larger side of the net's pad at
  the end, at most 2.5 mm (`NECK_PADS`, `NECK_CAP_MM`); 0 where the end is on no pad of
  the net (no neck there).
* **In the search**: a grid point is free (full width), free only necked down (within a
  neck radius of an end and free for the neck width's obstacles, thermal reliefs
  included), or blocked. A step at the neck width (either end only free necked down)
  costs twice a full step (`NECK_COST`), so the route stays at full width where it can.
  Vias may sit where the track is necked; their own checks are unchanged.
* **Escape**: near a pad an end may be joined to the grid by a straight (octilinear, two
  pieces at most) link as long as the neck radius, checked exactly, at full width first,
  else necked down inside the radius; the grid is twice as fine (within ADR 0059's caps,
  coarsening as before) and puts the necked end (else the other) on a grid point.
* **Output**: the grid path is cut into pieces of one width (a step is narrow when either
  end is); each piece is pulled tight with its own obstacles, narrow pieces kept within
  the radius plus two grid steps. `LayerRoute` gains `widths` (per leg, per segment) and
  `pieces()` (layer, width, points).
* **Transition**: the width changes at a grid point where the full width is clear, so the
  wide track's round end is the shoulder (no taper polygon: KiCad tracks have one width per
  segment, and a stepped taper adds segments without clearance gain). The narrow stretch
  is octilinear like the rest.
* Bounds and determinism as ADR 0059 (the same caps; link search at most 200 rings).

**Command** `pcb.route`: after the walkaround and the layered search fail for a leg and an
end of the leg is on a pad of the net, the search near pads runs (on the allowed layers,
one or more), before push-and-shove. It is on by default: it necks only where the full
width does not fit, never below the minimum the DRC checks, so it cannot add a
`track_width` problem. `neck: false` keeps the full width (the escape still runs). The
narrow and full tracks, vias and any shove are one edit and one undo step; the result
adds `neck_width` and `necked_mm` when the route necked. Arcs (`arc_radius`) round full-
width corners only. MCP: `neck` has no description; `layers`, `net` and `pcb.route_pair`'s
`width`/`gap` docs were shortened; the pcb toolset is 24 tools, 14 387 bytes (599 per
tool; 14 395 before).

## Results (2026-11-14)
Local check (`tests_local::local_samples_neck` against `local_samples_layers`,
`GUSSET_KICAD_SAMPLES`, KiCad demo boards, CC-BY-SA, not in the repository), release
build, exactly as ADR 0059: pad pairs of up to 30 nets on F.Cu, every other track in
place, F.Cu and B.Cu, each pair alone on the board as it is; each route laid, the zones it
touches poured again before and after, and the check must find no more problems after
(asserted) and no new `track_width` problem (asserted). Order: walkaround; layered search;
search near pads (counted first at full width with the escape alone, then with neck-down);
push and shove.

Minimum applied: the demo boards are KiCad 6+ files without their `.kicad_pro` (where KiCad
keeps the design rules) and without net classes, so every board used Gusset's default board
minimum, **0.15 mm** (track 0.25 mm, clearance 0.2 mm).

| board | pairs | DRC (after) | without (ADR 0059) | with | near pads: full-width escape / necked | time near pads |
|---|---|---|---|---|---|---|
| ecc83-pp | 9 | 0 (0) | 9 | 9 | – | – |
| pic_programmer | 30 | 4 (4) | 30 | 30 | – | – |
| interf_u | 30 | 67 (67) | 30 | 30 | – | – |
| complex_hierarchy | 30 | 45 (45) | 30 | 30 | – | – |
| StickHub | 24 | 1438 (1438) | 14 | **22** | +1 / +7 | 0.23 s (10 tried) |
| coldfire | 30 | 206 (206) | 12 | **26** | +12 / +2 | 8.8 s (18 tried) |
| video | 30 | 12 (12) | 21 | **26** | +7 / 0 | 2.5 s (11 tried) |

* coldfire 12 → 26, StickHub 14 → 22, video 21 → 26 (the 2 pairs ADR 0059 pushed now route
  without moving anything, plus 5 PCI bus nets); no DRC change on any board, worst case
  equal, no `track_width` problem. The search near pads routes 29 pairs (20 escaped at full width, 9 necked), 27 more routed in all.
* What actually necks: StickHub's seven LED nets (`Net-(D15-1)` … `D21`, 0.52 mm at
  0.15 mm each) and coldfire `/CLKMOD0` (0.46 mm) and `/DDAT0` (0.97 mm); the other 20 gains
  are the exact escape at full width (see "What the measurement found first").
* Still failing: coldfire 4 (`/AN2`, `/DTIN1`, `/PST0`, `/PST2`: no way across the two layers in the
  window), video 4 (EA5, EQ1, EQ2, EQ6: as ADR 0059), StickHub 2 (`+5V`, `/LC`).
* Time is the router's for the search near pads, one search per pair as `pcb.route` runs
  it (about 0.5 s per coldfire pair: the twice-finer grid). Walkaround and layered times
  are unchanged from ADR 0059.
* Tests: `gusset-pcb` `tests_neck` 4 (a pad in a 0.5 mm row reached only by necking: the
  walkaround, the layered search and the full-width escape all fail at 0.4 mm, the necked
  route is 0.15 / 0.4 mm, DRC unchanged, joined, deterministic; a class minimum of 0.2 mm
  and a custom `track_width` rule raise the neck width and no `track_width` problem
  appears; the transition: one narrow piece from the pad, joined at one point where the
  full width is clear, within the radius, under 1.5 mm, 45° throughout; refused at 0.4 mm
  pitch and with no minimum below the width); `gusset-commands` `tests_pcb_flow`
  (`neck: false` refused, the default necks to 0.15 mm then 0.4 mm, DRC clean, one undo
  restores the board).

## Consequences
* Fine-pitch pads route at full width where the geometry allows and necked down only
  where it must; the narrow copper is bounded to two pad sizes (2.5 mm) from the pad.
* The search near pads runs only after the existing searches fail, so routes that already
  worked are unchanged (ADR 0059's counts are reproduced by `local_samples_layers`).
* Necking at a point along the path that is not a pad (between two vias, say) is not done.
* The editor's Route tool previews the one-layer walkaround and the push as before; its
  click asks `pcb.route`, which necks.

## Alternatives considered
* A finer grid everywhere: the window's caps coarsen it again on long routes, and every
  route would pay for it; the escape fixes the ends where the grid is too coarse.
* A tapered polygon or stepped widths at the transition: not a track in KiCad's model and
  no clearance gain over the round end at a point where the full width is clear.
* Necking without a radius (anywhere the full width is blocked): would thread narrow
  tracks through whole fields of pads; the bounded radius keeps it a neck at the pad.
