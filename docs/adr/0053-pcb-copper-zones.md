# ADR 0053: PCB copper zones: several outlines, thermal spokes, fill on edit, outline editor

Status: accepted, implemented (2026-11-13). Follows ADR 0047 ("Next" → "Zones"). Behaviour
comes from KiCad's file-format and user documentation; no KiCad source was used.

## Decisions
* **Several outlines.** KiCad keeps several `(polygon (pts ...))` children in one zone.
  `Zone::outline` is the first, `Zone::more_outlines` the rest, `Zone::outlines()` all of them.
  A zone covers the union of its outlines (overlaps are one pour); the fill, the keep-out
  obstacles and the router read every outline. The tree is untouched by reading, so an
  untouched file writes back identically (tested byte for byte on the writer's layout).
  `Board::set_zone_outlines` replaces the polygons and drops the zone's stale fills.
  `pcb.add_zone` takes `more_outlines`.
* **Thermal spokes.** Spoke width is `thermal_bridge_width`, gap `thermal_gap`; spokes run
  along the pad's axes (45° for round pads) unless the zone has `thermal_bridge_angle`
  (read and written). Spokes narrower than the zone's minimum width do not survive the
  fill's opening, so such a zone leaves its pads starved. The fewest spokes a pad needs is
  `Rules::min_spokes` (default 2, the project's `board.design_settings.rules.min_resolved_spokes`).
  A pad of the zone's net inside its outline, joined by thermal relief, with fewer spokes
  is reported as the DRC item `starved_thermal` (same `Violation` shape: items zone and
  pad, at the pad, on the layer). A spoke counts as present when copper lies in the middle
  of the relief gap along its direction, so the check works on fills read from a file as
  well as on fresh ones. Fills also return `Filled::starved`; `pcb.fill_zones`,
  `pcb.zone_outline` and fill-on-edit list them as `starved_thermal`. Spokes that reach the
  fill are kept (a pad with one of two needed spokes stays partly connected and is
  reported, not silently accepted).
* **Fill on edit.** Session option `pcb_fill_on_edit`, default off; the PCB panel has a
  "Fill zones on edit" checkbox (kept in the preferences, handed to the session like the
  Freerouting setting; agents do not set it). In `commit_board` (every `pcb.*` edit), the
  edit is compared with the board before it (`gusset_pcb::refill`): tracks, vias, pads
  (footprints) and zones that are new, gone or different. Zones within reach of such an item
  on a shared layer, plus zones overlapping them (priorities), are poured again before the
  commit, so the edit and its fills are one undo step. A zone the command filled itself is
  not poured twice. Not done when the commit changes the project file (net class edits), as
  the rules then are the old ones. `fill` with a zone list skips lower-priority and distant
  zones, which keeps this fast on large boards.
* **Outline editor.** `pcb.zone_outline {zone, outlines}` replaces all polygons and fills
  the zone (one undo step). The board view's "Edit zone" tool picks a zone (smallest
  outline under the pointer), drags corners (grid snapped, one command on release), adds a
  corner at the circle in an edge's middle, deletes the picked corner with Delete or the
  panel button (an outline keeps 3 corners; a spare triangle goes whole). Helpers are in
  `gusset-app` `pcb_zone_edit`.

## Not done
KiCad's hatch smoothing and border options, spoke counting per KiCad's exact geometry (ours
tests the gap's middle), zone fills on layers a zone gains by `*.Cu` after edits other than
the above, selecting a single outline of a multi-outline zone to delete as a whole (only
via 3-corner deletion or `pcb.zone_outline`).

## Results
* Tests: `gusset-pcb` +5 (multi-outline round trip and fill, starved thermals in fill and
  DRC, spoke width and angle, fill-on-edit affected zones), `gusset-commands` +2
  (fill on edit with undo and redo, zone outline command with undo, `more_outlines`),
  `gusset-app` +1 (outline functions and the registry round trip with undo, the option
  reaching the session).
* MCP: `pcb.zone_outline` is behind `commands`/`run` like `pcb.add_zone`, so the `pcb`
  toolset grew only by the longer `pcb_fill_zones` description.
