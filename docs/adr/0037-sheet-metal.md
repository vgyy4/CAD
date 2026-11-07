# ADR 0037: Sheet metal (contour flanges, tabs, flat patterns)

Status: accepted (2026-11-04), phase 1. PROMPT.md "Phase 3+": sheet metal after drawings;
DESIGN.md §2: the SHEET METAL tab.

## Context
Fusion, Onshape, SolidWorks and FreeCAD all build sheet-metal parts from a few features
that know the thickness, the bend radius and the K-factor (where the neutral fibre sits),
and derive a flat pattern from them: a base flange or tab, flanges bent off edges, and an
unfold for the laser or the press brake. The flat pattern is the deliverable; its length
is the sum of the straight parts plus one bend allowance `BA = |θ| (r + k t)` per bend.

## Decision
Phase 1 adds two document features that make sheet-metal bodies, and the flat pattern as
data on the body.

```
contour_flange U (curves: S.curves(l5, l6, l7), width: 40 mm, thickness: 2 mm, bend_radius: 2 mm, k_factor: 0.44)
tab Plate (profile: S2.regions(l4), thickness: 2 mm)
```

* **contour_flange**: an open chain of sketch lines (the mould lines of one side) bent at
  every corner. The cross-section is built in Rust: the lines, the same lines one thickness
  away on the material side, concentric arcs at the corners whose inside radius is
  `bend_radius`, two end faces; it is extruded by `width` along the sketch normal
  (`symmetric` centres it). No new kernel operation is needed. Corners whose bend does not
  fit the line (setback `R tan(θ/2)` on both ends), lines in line and folds back onto the
  sheet are refused with the line named. Only lines: arcs in the chain are refused (a bend is
  added at each corner instead).
* **tab**: closed sketch regions extruded by the thickness (a flat plate).
* **Names**: a new face key `<part>(<line>)` with parts `flat`, `flat_back`, `bend_in`,
  `bend_out`, `start`, `end` (OpContext::Sheet, tags `8 · line + part`), plus `cap(start|end)`
  for the width faces. They follow the sketch lines, so they survive parameter edits.
* **Flat pattern**: computed with the section (straight lengths after the setbacks, bend
  zones of length `BA`) and kept on the body (`Body::sheet`). `query.flat_pattern` reports
  size and the bend table; `sheet.export_flat` writes it 1:1 as DXF (layers per line type,
  bend lines as centre lines with an "UP/DOWN 90° R2" note) or SVG through the drawing
  writers (ADR 0032).

* **edge_flange** (phase 2, same day): a flange bent off one outline line of a tab,
  `edge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: 2 mm,
  k_factor: 0.44)`. Its cross-section (outer and inner bend arcs about one centre, the flange
  `length` beyond) is built in the plane across the line, at the tab's side face, extruded
  along the line and united with the tab; the line's outward side is found from the flat
  outline. The tab's flat pattern gains the bend zone and the flange as a strip on that
  outline edge. Lines inside the outline or already flanged are refused.

## Consequences
* Volumes are exact (the OCCT test checks a U channel against
  `w (t Σ straights + Σ θ t (r + t/2))`, and at k = 0.5 the flat area times `t` equals it).
* Hems: an edge flange of up to 180 deg; near 180 deg it needs a bend radius and lies one bend
  gap (`2 r`) above the tab (OCCT test).
* Flanges on two lines meeting at an outside corner of a tab leave an open corner (each stays
  on its own side; their volumes add exactly, OCCT test); at an inside corner the second would
  run into the first and is refused.
* Not yet: edge flanges off a contour flange or off another flange, bend reliefs, hems,
  jogs, corner reliefs, and unfolding features that cut the sheet after it was made (the flat
  pattern shows the flange as its feature built it). They follow as further `sheet` features,
  with a geometric unfold (planar and cylindrical faces of constant thickness) for the cuts.
* Review (2026-11-04): a body changed by any non-sheet feature after its sheet-metal
  feature (a cut, a hole, a move, a joint) records that feature in the flat pattern's
  `changed_by`; `query.flat_pattern` warns, `sheet.export_flat` refuses without
  `allow_changed: true`, and edge flanges refuse such a tab (they would be built where the tab
  was). Edge flanges grow the flat outline before uniting, so a refused line changes nothing.
  Arcs in flat outlines are sampled within 0.005 mm.

## Amendment (2026-11-04): edge flanges on contour flanges

`edge_flange` also bends off a contour flange: `line: <line>_start` or `<line>_end` names
the side edge of that line's straight at the low or high end of the width along the
sketch normal (the sketch plane is the low end for a positive, non-symmetric width). The
flange runs the straight's flat length (between its bend tangent lines), "up" is towards
the material side of the straight, and the flat pattern grows from that straight's stretch
of the developed rectangle's edge. The contour flange keeps its straights (trimmed sketch
line, material direction, developed start) in the flat pattern for this. Flanges on
neighbouring lines' same side are refused: they would meet inside the bend between them
(corner reliefs and mitres are later work). A bare line name on a contour flange is refused
with the two side names as the hint.

## Amendment (2026-11-05): flanges off flanges

`edge_flange` with `line: <flange>_end` bends off the far edge of an earlier edge flange on
the same body. Every edge flange records that edge in the flat pattern (`flange_ends`): the
line its strip ends on in the flat layout, and the frame of a flange bent off it (origin at
the end of its outer face, x along it, y across its thickness towards the inner face, the
same run along the line). "Up" is then towards the first flange's inner face: two flanges up
fold back over the tab, a flipped second one turns outward. A second flange on the same end
is refused. OCCT test `a_flange_bends_off_another_flanges_far_edge`: exact volumes and the
flat area at k = 0.5 for three up/down combinations.

## Amendment (2026-11-05): unfolding later cuts

A feature that changes a sheet body no longer always marks its flat pattern stale. If the
change is a cut through flat regions only, it is unfolded: every face the feature made must
meet both a flat's sketch-side face (a tab's start cap, `flat(..)` of a contour flange or an
edge flange) and its back, and no other sheet face (a bend, an end, the tab's sides). The
edges between the new faces and the sketch-side flats are then mapped into the flat pattern
(the tab's sketch coordinates; a contour straight's developed start plus the distance along
it and the height across the width; a flange's far edge less the distance back from it)
and joined into cut-out outlines. Every mapped point must lie on its face's plane (a moved
sheet is not a cut) and inside the pattern. Anything else, a notch across an edge, a cut
through a bend, added material, is marked as before (`changed_by`). OCCT tests: a hole
through a tab becomes a circle of the same radius in the pattern; a hole through a U
channel's bottom lands where that straight develops.

Cuts across a contour flange's bends are unfolded too (2026-11-05): each bend keeps its
centre, the direction to its start on the sketch side, its turn, its radius there and its
zone in the pattern (`ContourBase::bends`). A point on the bend's sketch-side face maps to
the zone's start plus the share of the turn its angle makes, times the allowance, and the
height across the width as for straights. The outline is the cut's outline on the sketch
side: exact for cuts square to the sheet, an approximation for oblique ones (whose outlines
on the two faces differ). Edge flanges' bends are unfolded the same way, each flange keeping its bend
(`FlangeEnd::bend`: the profile frame, the line it starts from in the pattern, centre, start,
outer radius, turn and allowance); points on its outer face (`bend_out`) are mapped.

Neutral surface and circles (2026-11-07, dogfooding F6-4): a point on a bend face is first
moved along the extrude cut's direction onto the neutral surface (radius `allowance / |turn|`
= r + k t) and then mapped, so a cut across a bend is as wide as the neutral arc whichever
face its loop came from (if that does not close the loops, e.g. a slanted cut at a bend's
edge, the sketch-side face is used as before). Cut-outs on flat faces are unchanged. A full
circle on a flat face (a closed circular edge, or a circle hole of a tab's sketch) is kept as
`FlatPattern::circles` (centre, radius, index into `outlines`), reported by
`query.flat_pattern` as `cutouts[].circle` and written to the DXF as a CIRCLE entity (SVG:
a circle too); its polyline stays in `outlines` for sizes and areas.

## Amendment (2026-11-05): closed corners

`edge_flange ... close: true` closes the outside corners the new flange makes with flanges
already bent off the neighbouring tab lines. Both must be 90 degree flanges bent the same
way, off a tab's outline; the lines must share an end at an outside corner. The walls butt:
the new flange's wall runs on past the corner by the other's inside radius plus the
thickness (covering the corner), the other's wall by the new one's inside radius (up to the
new wall's inside face). The bends are not extended, which leaves a square relief at the
corner. Each extension is a strip of the wall's section extruded past the line's end and
added to the body (faces named `<feature>/flat(<line>_corner)` and so on); the flat pattern
gains a matching flap on each strip's side. Without such a neighbour the flange is refused.
OCCT test `a_closed_corner_butts_the_walls_and_keeps_a_relief`: exact volumes and flat
areas, up and down.

## Amendment (2026-11-07): mitred corners

`close: mitre` (command: `mitre: true`) closes an outside corner between two 90 degree
flanges with both walls cut on the corner's diagonal, the plane through its inner and outer
vertical edges, instead of one wall running past the other. Each wall gets a wedge past its
line's end, bounded by that plane shifted 0.05 mm back, so the walls are 0.1 mm apart.
Welded shut, a mitre would be the same solid as the butt joint. In the flat pattern each flap
is as wide as its wedge at the neutral layer. Unequal bend radii shift the diagonal. OCCT
test: volume, the two mitre faces and the flat area, with the flange bent up and down.

## Amendment (2026-11-07): closed corners at other angles

Flanges bent the same way at less than 90 degrees (equal radii) close their corner on the
plane that mirrors one wall into the other: each wall's flat part is carried well past its
line's end and cut off at that plane (0.05 mm short of it for a mitre). In the flat pattern
each flap is a trapezoid: at a height u up the wall it reaches the neutral layer's distance
from the line, rn sin(angle) + u cos(angle), less the half gap. Walls leaning back past 90
degrees would have to be trimmed, not extended, and are refused. OCCT test at 60 degrees:
volume and flat area for close and mitre, up and down.

