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
