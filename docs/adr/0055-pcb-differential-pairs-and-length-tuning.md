# ADR 0055: PCB differential pairs, net lengths and length tuning

Status: accepted, implemented (2026-11-13). Follows ADR 0047 ("Next" → Routing) and 0054 (the
class fields `diff_pair_width` / `diff_pair_gap`, stored until now). Behaviour comes from KiCad's
published documentation of pair naming and the usual meander practice; no KiCad source was used.

## Decisions
* **Pair recognition** (`gusset-pcb::pair`). A net is half of a pair when its name ends in `_P` /
  `_N` (also `_p` / `_n`) or `+` / `-`, with a non-empty base that does not end in `/`
  (`/hub/CLK+` is the pair `/hub/CLK`). A pair exists only when both nets are on the board.
  `pair::find` takes either net or the shared name.
* **Width and gap** (`Rules::diff_pair_of`). The class of either net gives `diff_pair_width` and
  `diff_pair_gap`; each falls back on its own to the board's track width and clearance. The gap
  is never below the nets' clearance (raised, and reported as `gap`).
* **Coupled routing** (`pair::route_pair`). One centre line is searched with the existing
  `walkaround`, for a track as wide as both tracks and the gap (`2w + gap`), crossing the copper
  of the pair's own two nets only (`Obstacles::skipping`). Each end gets a short straight stub
  along the line from the pads (nearest of eight 45° directions) so the fan-out meets the pair
  square; jogs the grid search leaves are tidied (a short inner segment becomes the corner where
  its neighbours meet, so every direction stays a multiple of 45°). The centre line is offset
  by (w + gap) / 2 to each side with mitred corners; the fan-out runs straight from the nearest
  pad of each net (within 10 mm) to the end of its offset line. Which offset line is P follows
  from the pads at both ends together, so the tracks never swap sides. Last, every piece is
  checked against its own net's obstacles and each P piece against each N piece (centre distance
  at least width + clearance); any failure refuses the route. A centre line that starts straight
  is tried first, then one that starts diagonal. No shove, no arcs, no via change of layer.
* **Command.** `pcb.route_pair {pair, points, layer?, width?, gap?}`: `points` is the centre
  line (start, waypoints, end), the first and last being where the coupled part begins and ends.
  One undo step. It reports both lengths and the skew.
* **Length** (`gusset-pcb::length`). A net's length is the sum of its tracks (arcs exact:
  radius times sweep) plus one board thickness per via (`thickness_mm`; the board's own value, 1.6
  when the file gives none). Zone fills and pads count nothing. `Skew` is |P - N| with the shorter
  net named. Command `pcb.lengths {net?}`: per net `tracks`, `vias`, `track_mm`, `via_mm`,
  `length_mm` (longest first, 200 at most) and per pair `p_mm`, `n_mm`, `skew_mm`,
  `shorter_user_text`. `pcb.info` is unchanged (its schema and result stay small).
* **Tuning** (`gusset-pcb::tune`, command `pcb.tune`). A straight track of the net is replaced
  by a meander of square bumps on one side, all legs one pitch apart (pitch = track width +
  `spacing`), corners chamfered at 45°. A bump of height h and chamfer c adds `2h - (8 - 4 sqrt2) c`;
  the bump count is the fewest that fits `amplitude`, then h is solved so the added length is
  exact. Each candidate track (longest first, eight at most), amplitude (1, 0.75, 0.5, 0.35 of the
  set one) and side is tried until every piece keeps the net's clearances against other nets'
  copper, edge and holes; the meander is centred in the track. Refused when: the net is already
  within `tolerance` of the target or above it, there is no straight track, the extra length is
  below the smallest meander (chamfer = track width) by more than the tolerance, or nothing fits
  ("no room"). Defaults: tolerance 0.05 mm, amplitude 1 mm, spacing = the net's clearance.
  `pcb.tune {net, target?, tolerance?, amplitude?, spacing?, track?}`; without a target the net
  must belong to a pair and the shorter of the two is tuned to the longer's length. One undo step
  (the track is deleted and the pieces added in one board edit).
* **MCP.** The three commands are not in the `pcb` toolset (it has 24 tools at 14 240 of 18 000
  bytes and is at the 600 bytes per tool average limit's edge); they are reached through
  `commands`/`run`, like the other commands outside the everyday list. The core list is unchanged.
  No UI was added (the route tool in the app still lays single tracks).

## Results (2026-11-13)
* Tests: `gusset-pcb` 109 -> 120 (`tests_pair`, 11: names and partners, class/board width and
  gap, a coupled route round a wall at the class width and gap with the DRC clean and the
  Gerber re-read, geometry of the coupled part and the 45° corner offsets, refusals, lengths
  with an arc and a via, skew, meanders of 0.9 to 40 mm all within 0.02 mm with the DRC clean,
  refusals for no room / already there / bad input, the shorter side of a pair tuned to zero
  skew); `gusset-commands` 290 -> 293 (`tests_pcb_pair`, 3: pair route from the class with undo
  and refusals, lengths, tune with undo); `gusset-app` 186 and `gusset-mcp` 40 + 34 unchanged
  and passing.
* MCP: core 17 912 bytes (unchanged by this work), `pcb` toolset 14 240 (unchanged, 24 tools);
  all command tools 174, 101 135 bytes (581 per tool; the three new ones are within the 600
  average).

## Not done / next
* Pairs: changing layers with via pairs, shove, arc corners, routing from the pads
  automatically (the user gives the centre line), uncoupled length limits and a pair-aware DRC
  rule (`diff_pair`, `skew`, `length` constraints are still skipped with a warning, ADR 0054).
* Tuning: several tracks of one net, trombone sections with arcs, tuning a pair's tracks
  together (phase tuning), and a limit on the coupled length lost to a meander.
* A GUI tool for pair routing and a tune dialog.
