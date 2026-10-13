# ADR 0016: Sketch arrangement, trim, splines and sketch patterns

- Status: accepted
- Date: 2026-10-12

## Context
Dogfooding (F-M8) showed that sketches whose curves cross did not produce regions: any
crossing, touch or T-junction was an error (`SelfIntersecting`), so two overlapping
rectangles or a circle crossing a line could not be extruded, and there was no way to
trim. v0.2 also adds splines and sketch-level patterns. All three touch how a sketch is
turned into regions, so they share this record.

## 1. Planar arrangement of the sketch's curves

### Decision
`gusset_sketch::arrange` computes the planar arrangement of the non-construction curves:

1. Endpoints closer than `tol` (1e-6 mm) are merged into vertices (as before).
2. Every pair of curves is intersected (lines, arcs, circles analytically; splines by
   subdivision plus Newton, see §3). A crossing, a touch (tangency) or an endpoint lying
   on another curve cuts both curves there. Intersection points within `tol` of an
   existing vertex use that vertex. Two curves that *overlap along a stretch* (collinear
   lines, arcs of one circle) are still an error (`SelfIntersecting`, now worded as an
   overlap): the arrangement would need a merged edge with two owners, and a hand-drawn
   overlap is nearly always a mistake.
3. Each curve becomes a chain of **pieces** between its cuts. A circle with one cut gets
   a second cut at the opposite point so every piece is a proper arc; an uncut circle
   stays a circle.
4. Faces are traced with the half-edge "turn most clockwise" rule. Edges with the same
   face on both sides (dangling stubs, bridges between loops) bound nothing and are
   dropped; the rest is unchanged by that. The entities that had a free end are reported
   as `open_ends`.
5. Every bounded face is a region; components nested inside a face are its holes
   (unchanged). If there is no region but there are free ends, the error is still
   "the profile is not closed (open at ...)", so an unfinished outline is reported as
   before; with at least one region, free ends are ignored and `feature.extrude` /
   `feature.revolve` add a warning when they use all regions.

### Region identity (how regions are named and selected)
Kernel profile edges carry the tag of the entity they came from, so lateral faces keep
the `side(<sketch>/<entity>)` names of ADR 0003; an entity cut into several pieces
yields several lateral faces with that name, told apart by the existing `#k` geometric
suffix.

A region's **primary entity** is the entity with the smallest id on its outer loop. The
**region id** is:

* **No curve is cut by another one** (every old document): the primary entity, exactly
  as before. `regions(l5)` keeps its old meaning "every region whose outer boundary
  contains `l5`".
* **Curves cross**: several regions can share a primary entity. They are ranked by their
  sorted list of boundary `(entity, piece number)`, where pieces are numbered along the
  entity (lines from `a`, arcs from `start`, circles from a canonical first cut: the
  lowest-id partner curve, entering before leaving, which is invariant under moving,
  scaling or rotating the sketch). The first keeps the plain id, the others become
  `<entity>_r1`, `<entity>_r2`, ... (`cap(end, l5_r1)` in face names; the first region
  keeps the unsuffixed cap name as before). In this mode `regions(X)` means: a region id
  `X` if one exists, else the single region whose outer boundary contains entity `X`; if
  that entity bounds several regions the feature fails with the list of ids (never a
  silent guess).

Ids depend only on the *combinatorics* of the arrangement (which entities cross in which
order), not on coordinates, so dimension and parameter edits keep them. Edits that change
the combinatorics (a crossing appears, disappears or changes order) can rename or remove
regions; a stored id that no longer exists makes the extrude fail with "no closed region
`l5_r1`" and the list of current ids (hint), as for any missing reference (ADR 0003 §5).
`query.sketch` lists each region's id, area, outer entities and `at`, a point inside it,
to recognise it.

## 2. Trim
`sketch.trim {sketch, entity, x, y}` removes the piece of a line, arc or circle that lies
between the nearest cuts around the pick point `(x, y)`. It is one `edit` (one undo step)
that rewrites the sketch definition:

* Cuts are the places where other non-construction curves meet the entity
  (`gusset_sketch::cuts`). No cut around the pick: the whole entity goes (with the points
  nothing else uses).
* **Line / arc**: the remainder before the piece keeps the entity id; the remainder after
  it is a new entity (same kind, same centre for arcs). Only one remainder: the entity
  itself is shortened. New ends are an existing point if one is at the cut (T-junction
  foot, another curve's end), otherwise a new point tied to the cutting curve with
  `on_line` / `on_circle` (a spline cutter gets no constraint). The two halves of a split
  line stay on one line (`on_line(new start, entity)`, `on_line(old end, entity)`); the
  two halves of an arc stay on one circle (`on_circle`).
* **Circle** with two or more cuts: replaced by the counter-clockwise arc from the end of
  the removed piece to its start (new id; `side(<sketch>/c3)` references to the circle
  are lost, as for any deleted entity). A circle with fewer cuts is deleted.
* **Constraints**: those on points stay with the points; an end point that nothing uses
  any more is deleted with its constraints (reported). Constraints on the trimmed entity
  stay on it (`horizontal`, `parallel`, `perpendicular`, `angle`, `tangent`, `radius`,
  `diameter`, `on_circle`, ...; they hold for the collinear/concyclic remainder too).
  Dropped, because they no longer mean the same: a trimmed line's length
  (`distance(l, l)`), `equal` and `midpoint` on a trimmed line. `fixed(entity)` becomes
  `fixed` on the surviving original end points (and, for a circle turned arc, on its
  centre plus a radius constraint with the current value). The result lists
  `constraints_added` and `constraints_dropped`.
* Pattern copies (§4) are not cut by trim and cannot be trim targets; splines cannot be
  trimmed (§3).

## 3. Splines

### Decision: an interpolating spline (centripetal Catmull-Rom) through fit points
`s7 = spline(p1, p2, p3, p4)` (optionally `closed`) is a curve through ordinary sketch
points. Because the points are ordinary points, every point constraint applies
(`fixed`, `coincident`, `on_line`, `distance`, ...), and the points show up in drags and
the DOF analysis. The spline itself adds no solver equations.

The curve is a **centripetal Catmull-Rom spline** stored as a chain of cubic Béziers
(`gusset_sketch::spline`): C1, passes through each fit point, local (a segment depends on
four neighbouring points), open ends leave along the end chords, closed splines are
periodic. Why this and not a B-spline with control points:

* Users and agents place points *on* the outline (a profile that must go through given
  holes, a measured point, an intersection); a control-point B-spline only approximates its
  polygon, so the drawn points would not lie on the curve and `on_line`/`coincident`
  would mean something surprising.
* A global interpolating cubic (natural/clamped, C2) would change the whole curve when one
  point moves, which makes region ids and fillets less stable; the local scheme keeps edits
  local. The cost is C1 instead of C2 (curvature jumps at fit points), documented.
* Centripetal knot spacing (chord^0.5) avoids the cusps and self-loops that uniform
  Catmull-Rom makes with uneven point spacing.
* The Béziers are exact in both worlds: the arrangement and the kernel see the same
  control points, so the solid's edge is the sketch's curve to rounding, not a tessellation.

### Kernel and naming: one profile edge per Bézier segment
A spline reaches the kernel as `Seg2::Bezier { p }` edges, one per Bézier segment (a piece
cut out of a spline by the arrangement keeps whole segments plus the sub-segments at its
cuts). The OCCT shim builds a `Geom_BezierCurve` edge for each. We tried the obvious
alternative, one clamped cubic B-spline edge per spline piece (knots with multiplicity 3),
and it is geometrically exact (a tessellation agrees to 5e-5) but **OCCT's mass
properties are wrong for it**: the volume of a spline prism was off by 1-3 % with the
default integrator and still by 1.8 % with the adaptive one (`BRepGProp` integrates a
multi-span edge/face with too few Gauss points); one edge per segment is exact (3e-9).
Volume and area are part of the contract (query tools, exports, golden tests), so the
representation follows the integrator.

Consequences for naming (ADR 0003): every Bézier segment sweeps its own lateral face,
named `<feature>/side(<sketch>/s9_g3)` where `g3` is the segment (segment `k` runs from
fit point `k` to the next one; the tag keeps `seg + 1` in the top 12 bits). The name
depends only on the spline and the fit point index, not on coordinates, so it survives
moving points; inserting a point in the middle renumbers the following segments (the
usual case of a deleted/renumbered input). A segment cut in two by a crossing curve
yields two faces with the same name, told apart by the usual `#k`. The mock kernel does
not support splines (only straight edges, as for arcs).

### In the arrangement
Splines are intersected numerically with every other curve: both curves are flattened to
cells (one per Bézier segment / arc quarter), cell pairs with overlapping boxes are
intersected as polylines and each candidate is refined by Newton iteration on the exact
curves (accepted when the residual is below `tol / 10`). Known limits: a *tangential*
touch of a spline with another curve is not detected (no sign change in the polyline);
overlap of a spline with another curve along a stretch is not detected; a spline that
crosses itself is an error (`SelfIntersecting`). Pieces are exact sub-curves of the chain
(de Casteljau); a piece is a run of Bézier segments (`Segment::sub` is the segment's
index). A closed spline without cuts is a loop of its own (like a circle); one cut
adds a second cut at the opposite parameter.

### Limits
Splines cannot be trimmed (a remaining piece is not a Catmull-Rom spline through a
subset of the points); `sketch.trim` explains this. Curves cut by a spline get a new
end point that is not constrained to the spline (no point-on-spline constraint yet).
2..=1000 fit points (open needs at least 3, closed at least 3).

## 4. Sketch patterns

### Decision: a sketch item that derives copies after solving
`g9 = pattern_linear(of: [l5, c7], count: n, dx: 10 mm, dy: 0 mm)` and
`g10 = pattern_circular(of: [l5], center: p3, count: 6, angle: 360 deg)` are a third kind
of sketch item beside entities and constraints (id prefix `g`, same counter). They hold
expressions, so `count`, `dx`/`dy` and `angle` follow parameter edits like constraint
values. After the sketch is solved, `gusset-eval` derives, for every copy `j = 1..count-1`
of every source curve `E`, a curve `E_j` and for every point `P` it refers to a point
`P_j` (shared by the curves of that copy), moved by `j·(dx, dy)` or rotated by `j·step`
about the solved centre point (`step = angle / count` for a full turn, else
`angle / (count - 1)`, as for body patterns; `count` includes the original).

* The copies are **not solver variables**: they are exact transforms of the solved source
  points, so they cannot drift, add no DOF and cannot conflict. The price: constraints
  cannot refer to copies. Because the *centre* of a circular pattern is an ordinary point,
  it can be constrained (e.g. `coincident` with another point or `fixed`).
* Copies appear in the sketch's points, lines, circles, arcs and splines (query/GUI),
  take part in regions (so a pattern of holes in a plate is a pattern of circles inside the
  plate outline) and in extrusions. Their lateral faces are named
  `side(<sketch>/<copy id>)`, e.g. `Extrude1/side(Sketch1/c7_2)`; `entity_tag` encodes the
  copy number in tag bits 40..52 (spline segments use 52..64). Copy numbers `j = 1..count-1`
  are stable; lowering the count removes the highest `j`, so references to the remaining
  copies survive. A region's id can name a copy (`regions(l5_2)`), the validator accepts
  `<source>_<j>` when `<source>` is in a pattern.
* A source curve may belong to one pattern only; patterns of patterns (grids) are not
  supported yet (use two sketches or a body pattern). Sources are lines, circles, arcs and
  splines, not points. Deleting a source removes it from the pattern; deleting the centre
  or the last source removes the pattern. Trimming a source keeps the new pieces in its
  pattern.
* Limits: 2..=500 copies, 20 000 derived curves per sketch.
* The count must be a whole number; otherwise the sketch fails with a hint, never rounds.

### Text and model
`SketchDef.patterns: Vec<SkPattern>`, printed after the constraints. `sketch.pattern`
creates or edits one (arguments are expression strings).

## 5. Solving parametric sketches after large edits (review v0.2 H1, 2026-10-13)

### Context
Constraint values are expressions, so one parameter edit can move every point of a sketch
far from the coordinates stored in the document (the generated gear: `gear1_teeth` 30 → 20
moves each of about 200 flank points by millimetres). Every solve starts from the stored
coordinates. Two things made such edits fail with a false `SketchConflict`:
1. `fixed` was a soft equation. A fixed point that hundreds of distances pull on (the gear
   centre) is a cheap direction for the damped least-squares step, and the iteration ended
   in a minimum with the "fixed" centre moved onto another point.
2. A single Levenberg–Marquardt run from the stored positions cannot reach every far
   shape (a chain of links curled from straight to a spiral does not converge).

### Decision
* **Fixed unknowns are held.** An unknown that a `fixed` constraint pins, and that is at
  its pinned value when the solve starts, is a constant of the iteration (its row and column
  are left out of the normal equations). The DOF, redundancy and conflict analysis still
  sees every equation. A dragged fixed point starts away from its value and is pulled back
  as before. With this, every reported gear edit (smaller teeth counts and modules, 100 → 20
  teeth, module 2 → 5 mm) solves directly.
* **Dimension continuation as the fallback** (`gusset_sketch::solve_continued`, used by the
  evaluator and by trim): only when the direct solve fails, every dimension (distance, angle,
  radius, diameter) is measured on the stored geometry and moved from that value to its
  requested value in steps; each step is solved from the previous step's solution. Steps
  adapt (start at half the way, halve after a failure, double after a success), at most 24
  extra solves and steps no smaller than 1/256 of the way. If it does not get through, the
  direct solve's result is reported (the user sees the same conflict analysis as before).
  The final step solves the sketch with exactly the requested values, so a success is a
  solution of the document's sketch. Measuring instead of remembering the previous
  parameter values keeps it a function of the document alone.
* **Determinism:** nothing is cached between solves; the same document gives the same
  result bit for bit, whether it was reached by parameter edits, by `document_set_text`, by
  undo or by opening a file.
* **No write-back on parameter edits.** `param.set` does not store re-solved coordinates in
  the sketch (the sketch editing commands still do, with `store_solution`). Write-back would
  not make evaluation independent of the stored start (text edits, merges and opened files
  jump anyway), and it would turn a parameter-only edit into an edit of every sketch that uses
  the parameter: larger history diffs and conflicts when two branches edit different
  parameters of the same sketch.

### Consequences
A failing parametric sketch costs up to 25 solves before the conflict is reported. The
release solver budget tests are unchanged (the continuation never runs for them).
