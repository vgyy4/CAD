# ADR 0035: Parasolid XT import (clean-room reader to STEP)

Status: accepted (2026-11-03). Research: docs/research/parasolid-xt.md.

## Context
Parasolid XT (`.x_t` text, `.x_b` neutral binary) is the native exchange format of
SolidWorks, NX, Solid Edge and Onshape. The public *Parasolid XT Format Reference*
(schema 12006, Parasolid V12) describes the file layout for translator writers. No Rust
reader with a clear licence exists.

## Decision
A new pure-Rust crate `gusset-xt` reads XT and writes STEP AP214 text; `import.file`
passes that text to the sandboxed STEP importer (no new OCCT code, no new unsafe).

* **Clean room.** Written from the public Reference in our own words. No schema file
  (`sch_*`), SDK, DLL, or third-party reader is used, read or vendored; the Reference PDF is
  not committed. Layouts of schemas older than V12 were worked out from public-domain NIST
  sample files (usnistgov/engineering-design-models), which are not committed either.
* **Schema-driven decoding.** Node layouts (field order and type codes) are data: a table for
  schema 12006 plus per-family overrides. A file's schema number picks the family:
  * V4 (4000-8999), inferred: no regions (shells hang off the body and hold the geometry
    chains), fins without sense (the edge's own fin is the positive one), curves with an
    extra double after their sense, shorter BODY/FACE/EDGE/VERTEX/LIST/CHART/ATTRIB_DEF.
  * V9 (9000-11999): as V12, but fins have no attribute field.
  * V12 (12000 on). Files with an embedded schema (three-part key, V14 on) apply each node
    type's edit script (copy/delete/insert/append) to the base layouts; the V12 table stands
    in for the V13 base, which the Reference does not list.
  * Older than V4: refused with a clear message (their node types are undocumented).
* **Encodings.** Text (lines joined, trailing spaces dropped, characters glued, `?` unset,
  string escapes) and neutral binary (big-endian, one- or two-word node indices). Bare
  binary is refused.
* **Topology.** Solid regions' shells (V4: the body's shells); faces oriented by
  face sense XOR surface sense; loops of fins with their senses; edge start/end from the
  positive fin's forward vertex and the other fin's; ring edges get one vertex on the
  curve; isolated loops become vertex loops. Solids become MANIFOLD_SOLID_BREP (or
  BREP_WITH_VOIDS, the largest shell outside), sheets SHELL_BASED_SURFACE_MODEL.
* **Geometry.** Exact: plane, cylinder, cone (axis reversed; the natural normal flips),
  sphere, doughnut torus, B-surface, swept, spun and offset surfaces; line, circle,
  ellipse, B-curve, trimmed curve. Fitted: intersection curves (a C1 cubic through the chart
  points); SP-curves (the 2D B-curve sampled through its surface; tolerant edges use their
  fins' SP-curves); rolling-ball blends (circular arcs of the ball from its closest points
  on the two support surfaces, along the spine, as a bicubic B-spline; line spines are
  bounded by the blend faces' vertices). Skipped with a warning: cliff-edge blends, foreign
  geometry, self-intersecting tori. Metres are written as millimetres.
* **B-surface poles.** The Reference does not state their order: both readings are tried
  against the vertices of the faces on the surface and the one they lie on is kept
  (u-major when there is nothing to test).
* **Names and assemblies.** A body's `SDL/TYSA_NAME` attribute names its STEP product.
  Assemblies are walked instance by instance (nested up to 32 deep), each body written in
  place with `x' = (R x + t) * scale` (R row by row; unverified on a sample).
* **Untrusted input.** Node and element counts are capped, element counts are bounded by the
  bytes left, chains stop at repeats, geometry references cannot recurse forever; a fuzz
  target (`xt_read`) and a mutation test cover the decoder.

## Consequences
* NIST samples (schemas 4035, 4039, 9008) convert; an OCCT golden test checks that the
  converted solids have NIST's face count and every vertex of NIST's own STEP export.
* Unverified without sample files: V12 attribute definitions, embedded-delta details,
  neutral binary, the rotation matrix's storage order. Wrong guesses in the first three fail
  loudly (an unknown node type or a misread stream).
* Colours are not carried: the STEP path into the kernel keeps no styles.

## Amendment (2026-11-09): cones widen along their axis

The reader had XT's cone widening away from its axis (`P - vA`). Countersinks in SolidWorks
and Solid Edge parts showed otherwise: read that way, the faces' own edges were off their
surfaces, OCCT split them, and the solids' volumes disagreed with their own meshes
(finger_aperture: 2712 vs 1170 mm3). Read as widening along the axis (`P + vA + (r + v tan
a)`, the same placement as STEP's cone), solid and mesh volumes agree. Over all sample
SolidWorks and Solid Edge parts, bodies whose solid and mesh volumes differ by more than 1 %
went from 55 of 129 to 23. Of those 23, 21 are imported surface bodies of one third-party
set and 2 are SolidWorks parts (the idler arms) still to be explained. The earlier controlled
samples had no cones, which is why this was not seen.

## Amendment (2026-11-09): helical forms, binary unset vectors, padded knot lists

Seven SolidWorks parts (six `Angle_support` revisions, `setScrew_M2x2_5`) and two Solid Edge
parts stopped in the reader. Causes, all worked out from the sample bytes:

* **Unset vectors in binary** are three unset reals, not one (text keeps a single `?`). Read
  as one, the next two reals shifted every later field by 16 bytes after the first TRANSFORM
  of a schema-30000 file and the node type came out as -15684. The `Source` trait now says
  which (`UNSET_VECTOR_IS_TRIPLE`).
* **Two node types the base schema lacks**, found in schema 25001 (embedded, base 13006: the
  embedded schema marks them 255 "as the base", so no description is transmitted) and in
  schema 31100 (not embedded) files of threaded parts. Both are the analytic form of a
  B-curve / B-surface that is a helix (the surface data's `analytic_form_type` is `H`), and
  are fixed-size, so the layouts follow from the bytes (no pointers; the three samples agree):
  * 184 `HELIX` (105 bytes): `axis_point:v axis:v start_point:v sense:c parm_range:i
    pitch:f tolerance:f`. In the screw the axis is +Z, the start point is the NURBS curve's
    first pole, the range is 0 to 7.25 (turns) and the pitch is 0.4 mm (M2 x 0.4).
  * 163 `HELIX_SURF` (89 bytes): `axis_point:v axis:v sense:c parm_range:i pitch:f taper:f
    tolerance:f`; `taper` is the unset real in all samples (its role is a guess from the
    position). Neither node is used for the geometry: the NURBS data carries the shape.
* **Padded knot lists.** In schema 25001 the NURBS surface's knot and multiplicity lists are
  longer than `n_u_knots` / `n_v_knots` (`[2,2,0,0]` for two knots, knots `[0,1,unset,unset]`).
  They are cut to the declared count; the surface and curve checks then hold, where before the
  helical thread surfaces were dropped as "cannot be converted".

Result: all 6 `Angle_support` revisions and `setScrew_M2x2_5` read (the screw with no
warnings, 5.2 mm3); the Angle_support bodies still warn that BLENDED_EDGE surfaces are left
out, as their mirrored twins (`Symetrie_Angle_support`) do, and their volumes agree with those
twins to 2e-5. Both Solid Edge parts that stopped at 163/184 (`M1.2x3 screw`,
`Plastic-handle-Loaded-Felfer-holder`) read too.

## Amendment (2026-11-09): lemon (spindle) tori

The reader refused every torus whose minor radius was not below its major radius ("self-
intersecting (apple or lemon)") and left its faces out, so the idler arms (SolidWorks fillets of
r1 on r1.5 lugs, major 0.5 mm, minor 1 mm) lost the upper halves of both lugs (204.9 mm3). From
the bytes, Parasolid stores a lemon like an apple: both radii positive (no negative-radius
convention occurs in the samples), the face's edges select the part. STEP's TOROIDAL_SURFACE
takes the same radii and OCCT accepts minor > major, so a lemon is written as it is, with the
face's sense unchanged. The fillet of a convex lug edge is the outer part of the tube, whose
natural normal is XT's. A first version reversed `same_sense` for minor > major; it gave a
closed shell with matching mesh volume (190.6 mm3) but the feature-history rebuild, an
independent path, gives 230.1106 mm3 (long arm 336.0977), which the unreversed import matches
(230.1168, 336.1048; relative 2.7e-5 and 2.1e-5, the history gate is 2e-5). Closure and mesh
volume do not decide the orientation of a spindle-torus face. Non-positive radii are still left
out with a warning. Equal radii (horn torus) are written as an apple; no sample has one.
Over all samples only the three torus parts changed (both idler arms and one zenodo
`Combination` body, 166766 to 167950 mm3, not checked face by face); history counts are
unchanged (34 as history).
