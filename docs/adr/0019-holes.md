# ADR 0019: Holes and standard presets

- Status: accepted
- Date: 2026-10-12

## Context
Holes are the most common feature in mechanical parts and the first place beginners meet
standards (clearance series, tap drills, screw heads). Docs/DESIGN.md §1 ("Easy for beginners,
never in the way of experts") asks for presets that fill every dimension from a standard
without becoming a black box. The feature goes through the usual layers (document, text
language, evaluator, command registry) and its faces need persistent names (ADR 0003).

## Decisions

### 1. One statement, four types
```
hole <Name> (face: <face>, points: <Sketch> | at: [[u, v], ...], [type: simple|counterbore|countersink|tapped],
             diameter: <len>, depth: <len> | through: true, [point_angle: <angle>],
             [cbore_diameter, cbore_depth], [csink_diameter, csink_angle], [thread, pitch, thread_depth])
```
A `hole` cuts the body that owns `face` (a planar face; the holes run against its outward normal),
one hole per site. Sites are the points of a sketch parallel to the face (every point that is not
construction geometry and not part of a line, arc or spline, plus every circle centre, pattern
copies included; the points are projected onto the face) or explicit `(u, v)` lengths in the frame
a sketch placed on that face gets. A hole that does not lie within the face is an error.
`depth` is that of the full-diameter part; a blind hole adds the drill point (118 deg by
default; 180 deg is flat); `through: true` has a flat end. Counterbore and countersink are
recesses at the mouth; a countersink is given by its diameter and full cone angle. A tapped hole is
**modelled at the tap-drill diameter**; `thread` (designation), `pitch` and `thread_depth` are a
cosmetic thread record, listed by `query_body` under `holes` (with the hole's wall face for
reference) and not helical geometry (threads and fasteners remain a later generator).

### 2. Kernel: revolved profile plus boolean cut
Each hole is a revolve (`Kernel::revolve`, 360 deg) of one closed profile in (radius, depth)
coordinates: mouth disc, counterbore wall and step or countersink cone, wall, drill-point cone
(or flat end). It is placed with its axis at the site along the inward normal and cut with
`Kernel::boolean`. Alternatives considered: OCCT's `BRepFeat_MakeCylindricalHole`/`BRepPrimAPI`
primitives (one call per shape kind, no tagged faces for stepped holes, no history for the
naming layer); `BRepFeat_MakeRevol` (needs the face and the sketch inside OCCT). The revolve
route needs no new FFI, works for every type with one code path, and reports per-edge origins
(`FaceOrigin::Lateral`) that the naming layer already understands. Cost: one boolean per hole
(up to 1000 per feature), each on the result of the previous one. Through holes reach the far
side of the body's bounding box plus 1 mm. Limits: the drill point cone may leave the far side of
a thin part (the cut then just breaks through).

**Fit check.** Before cutting, the body is tessellated (0.01 mm) and every hole's mouth (the
centre and 32 points of a circle 0.01 mm inside its largest radius) must be covered by the
triangles of the face: a hole outside the face, across its edge or into another face's area is an
`invalid_reference` error ("hole e1 (diameter 8 mm) at (58 mm, 20 mm) does not fit inside face ...")
with a hint, without touching the kernel's booleans. Curved face edges are approximated by the
tessellation, so a mouth up to about 0.01 mm over a curved edge is accepted.

### 3. Face names
`<feature>/<part>(<at>)` with part `wall`, `floor`, `cbore_wall`, `cbore_floor`, `csink` (and
`top`, the tool's disc in the mouth plane, which never survives) and `at` the hole's id: the
sketch point's id (`p5`, `p7_2` for a pattern copy) or `e1`, `e2`, ... for explicit points.
Names depend only on the feature, the id and the part, so they survive every parameter edit
(golden scenario `hole_names`: sketches on a counterbore's step and a blind hole's floor follow
edits of the hole dimensions, the plate thickness, a reordering and a branch). The face the hole
starts on keeps its own name. A wall cut into several faces by the material (a hole through two
legs of a bracket) gets the usual geometric `#k` suffixes (ADR 0003 §3). The tool is named with
`OpContext::Hole` (edge tag = `HolePart` index) and the boolean result with `OpContext::Modify`,
like every cut.

### 4. Presets: `standard` fills parameters, nothing hidden
`feature.hole` has an optional `standard` such as `"ISO M6 clearance normal"`, `"ISO M6 tapped"`,
`"ISO M6 counterbore DIN 974/ISO 4762 head"`, `"ISO M6 countersink 90 deg"`. The command (not the
statement; like ADR 0018 §6 the text language stays declarative) looks every dimension up in
`gusset-commands/src/hole_standards.rs` and writes it as an ordinary named parameter with a
comment naming its source (`hole1_diameter = 6.6 mm  # ISO 273 normal clearance hole for M6`;
prefix = the feature name in lowercase), and the statement refers to those parameters; a
`#` note on the feature names the preset. Any explicit dimension argument overrides the preset
(its parameter then carries the explicit expression and a comment with the standard's value), so
the same command works for experts with every number given. Without a `standard`, the
expressions go directly into the statement. A parameter that already exists, a size or type the
standard does not cover, or a phrase that is not understood is an `invalid_args` error with a hint
(sizes that exist, an example phrase). The command first tries the feature on a copy of the
document: an input problem (hole outside the face, counterbore not wider than the hole, thread
deeper than the hole, ...) is refused with the evaluator's message and hint and nothing is
committed; a kernel failure is committed like any other feature (broken node, `new_errors`,
`undo_hint`). A blind preset hole also gets `hole1_point_angle = 118 deg`.

### 5. The table
Written from the published nominal values (no code or table text copied):
- **ISO 273** clearance holes, fine / medium ("normal") / coarse series, M1.6 to M24 (M6: 6.4,
  6.6, 7.0 mm; M10: 10.5, 11, 12 mm);
- **ISO 261/262** coarse pitch and **ISO 2306** tap-drill diameter, M1.6 to M24 (M6: 1 mm and
  5.0 mm; M8: 1.25 and 6.8; M12: 1.75 and 10.2);
- **ISO 4762** head diameter and height with the **DIN 974-1** counterbore (diameter, depth),
  M3 to M24 except M3.5, M18 and M22 (M6: head 10 x 6, counterbore 11 x 6.5);
- **ISO 10642** theoretical head diameter as the countersink diameter, 90 deg, M3 to M20 (M6:
  13.44 mm).
ISO 4762 itself prescribes only the screw; the counterbore values are the DIN 974-1
recommendation, which is why the preset phrase names both. The values of DIN 974-1 and the
ISO 4762/10642 head sizes were first written from memory of the standards' tables; see "Values checked
below (2026-11-09)" for what has since been compared with public sources. Counterbore depths in
particular should be checked against the owner's copy before they are relied on for manufacturing (they are ordinary
parameters, so correcting one is an edit, not a code change); the table's internal consistency
is tested (ordering, clearance above thread, tap drill below it, counterbore above head) and
spot values are asserted. Tapped presets add two defaults that are *not* from a standard: the
thread is 1.5 d long and the drill is 0.5 d deeper. Threads other than the coarse pitch, other
countersink angles and fine clearance series for other standards are given by hand (arguments).

### 6. Records and limits
The body keeps a `HoleInfo` per hole (`Body::holes`): type, dimensions, cosmetic thread and the
wall face's name. Records follow in-place modifications (fillet, shell, move, combine merges the
tool bodies' records) but a mirrored body, a moved copy or a pattern instance starts without
records (their faces are renamed; the cosmetic thread of such a copy is lost). Threads are never
geometry (no helical sweep): fasteners and real threads are a later generator.

### 7. MCP
`feature.hole` is a non-core command (reached through the generic command tools). Its schema is
short (about 1.1 KB as a tool definition): the essentials carry the form-only `x-essential`
hint (face, points, standard, diameter, depth, through).

## Not included (later)
Slots and obround holes; holes on curved faces or at an angle to the face; thread geometry and
fine-pitch tables (ISO 261 fine, UN/imperial, NPT); countersunk-and-counterbored (spot-face)
combinations; hole tables in drawings; copying a hole to patterns with cosmetic threads.

## Amendment (2026-10-13): v0.2 review H3, L1, L2, L7
- **One boolean per feature (H3).** All of a feature's tools are built first and cut with one
  `Kernel::boolean_many` (ADR 0020), named once with `OpContext::Modify { inputs: [body,
  tool_1, ...] }`. Face names are the same as with one cut per hole. If the single cut fails,
  the holes are cut one at a time to name the failing one. 400 holes: 110 s before, 2 s now.
  Holes from a sketch are limited to 1,000 per feature like explicit points, and every hole
  feature is charged against the evaluation's work limits (ADR 0020).
- **Fit check (L7).** §2's check sampled the centre and 32 points of one circle, so a small hole
  or slot inside the mouth could pass. Now the face's triangles and boundary (triangle edges
  that only one triangle of the face has, so the outer loop and every inner loop) are built once
  per feature; a mouth fits when its centre is on a triangle and no boundary edge comes closer
  than its radius (less 0.01 mm, as before).
- **Stable ids for explicit points (L2).** §3 named `at:` holes by list position, so deleting or
  reordering a point renamed every later hole's faces and moved references to another hole.
  Each point now stores its id: `at: [e1: [10 mm, 5 mm], e3: [50 mm, 35 mm]]`. The text always
  prints the ids, so deleting `e2` in the code view keeps `e3`. Files written before list bare
  pairs (`at: [[10 mm, 5 mm], ...]`), which still parse: a bare pair is `e<position>`, which
  is exactly the old naming, so existing references keep resolving; from the next save on
  the ids are written out. Ids are unique within a feature (`e0`, `e01` and duplicates are
  refused); `feature.hole` numbers its points `e1`, `e2`, ... Sketch-point ids were already
  stable.
- **Records (L1).** §6's "combine merges the tool bodies' records" now holds only for
  `op: add`: a tool body that is cut away or intersected leaves no holes in the target. After
  every feature, records whose wall face no longer exists on the body (a pocket or a later cut
  removed the hole) are dropped, so `query_body` lists only holes that are there.

## Amendment (2026-11-09): Values checked

Each group of the section 5 table was compared, number by number, with at least two public
sources that reproduce the standards' dimensions (manufacturer/distributor and engineering
reference pages; no table text was copied, only numbers compared). The standards themselves
were not consulted (paywalled), so "verified" means "two independent public sources agree",
not "checked against the standard". No value in `hole_standards.rs` was changed.

Verified (two or more sources agree with our value):
- **ISO 4762 `dk` and `k`**, M3 to M24 (all rows): Misumi metric socket head cap screw table
  (us.misumi-ec.com/blog/metric-socket-head-cap-screws-dimensions/) and the DIN EN ISO 4762
  dimension table on fasten.it (fasten.it/norms/norm/din_en_iso_4762); `k` also by the
  Noramark and AmesWeb counterbore charts (below).
- **ISO 261/262 coarse pitch**, M1.6 to M24: Misumi (above, M1.6 to M24 without M3.5, M18,
  M22), Optimas tapping-size table (optimas.com/technical-resources/tapping-sizes/) and the
  get-it-made.co.uk metric thread chart (get-it-made.co.uk/resources/metric-thread-chart).
- **ISO 2306 tap-drill diameter**, M1.6 to M24: Optimas and get-it-made (above) list the same
  values, including M8 6.8 and M12 10.2 (Optimas gives 6.75 for M8, the unrounded d - P).
- **ISO 10642 `dk` (theoretical maximum)**, M3 to M20: fasten.it DIN EN ISO 10642 page
  (fasten.it/norms/norm/din_en_iso_10642, editions 2004, 2013 and 2020 agree) and the
  Westfield Fasteners ISO 10642 data sheet (westfieldfasteners.co.uk/Standards/ScrewBolt-SHCsk-M.pdf).
  The standard stops at M20, which is why M24 has no countersink row.
- **ISO 273 clearance holes**, M1.6 to M10, M20 and M24 (all three series): Optimas, The
  Engineer's Bible counterbore chart (engineersbible.com/counterbore-socket-iso/) and AmesWeb's
  clearance chart (amesweb.info/Screws/Metric-Clearance-Hole-Chart.aspx, ASME B18.2.8, which
  coincides with ISO 273 here). M3.5 (3.7, 3.9, 4.2): Optimas and Fractory
  (fractory.com/?p=21620).
- **DIN 974-1 counterbore diameter** for M3, M4, M5, M6, M8, M10, M12, M16, M20, M24 (6.5, 8,
  10, 11, 15, 18, 20, 26, 33, 40): The Engineer's Bible and the Noramark counterbore chart
  (noramark.com/resources/charts/counterbore-dimensions/, which names DIN 974-1 for ISO 4762).

Changed: nothing.

Uncertain (sources disagree or only one source found); the values stay as they are:
- **Counterbore depth** (all sizes). Sources use different conventions and do not agree:
  Noramark and AmesWeb give the flush depth `k` (M6: 6.0); whole-spec.com (whole-spec.com/en/
  counterbore/counterbore) gives about k + 0.5 (M6: 6.5, M10: 10.5, M12: 13); The Engineer's
  Bible gives k + 0.8 (M6: 6.8, M10: 10.8, M12: 12.8). Ours (k + 0.4 to 0.6, M6 6.5, M10 10.8)
  matches one of them only for M6 and M10, but lies within the range of all sources (k to
  k + 0.8) for every size, so a screw never stands proud. The DIN 974-1 depth table itself was
  not reachable; take the owner's copy as authoritative. Depths are ordinary parameters.
- **Counterbore diameter M14 (24 mm)**: only tooling catalogues (Hoffmann Group, Intercity
  Industrial counterbore tools for M14) found; whole-spec.com gives 23. Other whole-spec.com
  rows (M5 9.5, M8 14, M10 17.5) also differ from the two sources above, which look like a
  different (KS) series; we follow DIN 974-1 as the two sources do.
- **ISO 273 M12, M14, M16 medium and coarse**: our 13.5/14.5, 15.5/16.5, 17.5/18.5 are given by
  the Engineer's Bible, AmesWeb and whole-spec.com, while Optimas gives 14/15, 16/17, 18/19
  and Fractory 14/15 (M12) and 18/19 (M16). Kept ours (two to three sources against two).
  Fractory also lists M3 medium 3.35 and M6 medium 6.5, against 3.4 and 6.6 everywhere else.
- **ISO 273 M18 and M22** (19, 20, 21 and 23, 24, 26): only Optimas found; the sources
  consulted do not list these sizes. M14 fine/medium/coarse is covered by AmesWeb only.

## Amendment (2026-11-13): Fit check meshes the face alone

§2's fit check tessellated the whole body at 0.01 mm. On large bodies with curved faces that
was the cost of the feature (a dome 2 m across: 60 s), and on an imported KUKA iiwa link
(`Link_2.SLDPRT`, a hole wizard on an exact body whose planar face has a broken outline) BRepMesh's
Delaunay grew past 8 GB in a minute. The check now meshes only the face the holes start on
(`Kernel::tessellate_face`, remote request 38: the face is copied and meshed alone, so the body's
own mesh is untouched; the OCCT kernel refuses a face that takes more than 10 s). The
tolerance grows with the body: 0.01 mm up to a 400 mm bounding-box diagonal, then 1/40000 of
the diagonal, at most 0.1 mm; the margin a mouth may come over a curved edge is the same
tolerance (still 0.01 mm on parts below 400 mm). The dome's hole now takes 0.02 s.
