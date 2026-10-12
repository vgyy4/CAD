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
ISO 4762/10642 head sizes were written from memory of the standards' tables and should be checked
against the owner's copy before they are relied on for manufacturing (they are ordinary
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
