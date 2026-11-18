# ADR 0061: Extrude up to an extended face, variable-radius fillets

Status: accepted (2026-11-18)

## Context
Two modeling operations that importers need were missing:

* **Up to a face that does not cover the profile.** ADR 0004's `to:` end (an extrude that
  stops on a face of an earlier body) refused a face that does not reach past the whole end
  of the sweep. SolidWorks "up to surface" and Inventor "to face" with "extend" run to the
  face's *surface*, extended past its edges. The SolidWorks reader worked around it only for
  faces parallel to the sketch (a blind distance to the face's plane); Inventor parts fell
  back to the measured blind reach, wrong for tilted or curved faces (ADR 0046 amendment 19,
  note AR: 2 parts).
* **Variable-radius fillets.** SolidWorks `VarFillet_c` gives one radius per vertex of the
  edges (`D0`, `D01`, ...); Gusset read only those whose radii were all equal.

Written from OCCT's public API documentation and headers (`BRepFilletAPI_MakeFillet`,
`GeomLib::ExtendSurfByLength`); nothing from another CAD system's code.

## Decision
**Extend** (`ExtrudeEnd::ToFace { extend }`, text `to: <face>, extend: true`, command
`feature.up_to_face` `extend`, tree field `extend`):

* The kernel's `extrude_to_face` takes `extend`. The sweep is cut by the face's surface as
  before (planes, cylinders and cones were always extended to the whole box); with `extend`
  a sphere or torus is taken whole, another surface over its natural bounds clipped to the
  model's diagonal past the face, and a B-spline or Bezier surface first extended by that
  length across each non-periodic side (`GeomLib::ExtendSurfByLength`, curvature continuous).
  The check that the end lies inside the face is skipped.
* Either way the far end of the long prism must be cut off everywhere: a surface that does
  not cross the whole sweep is refused ("the face's surface does not cross the whole sweep"),
  where before a partial cut could pass as the sweep's end.
* Without `extend` nothing changes: a face that does not cover the end is refused, so that a
  pick that drifted onto a smaller face does not silently end somewhere else.

**Variable fillet** (`EdgeBlendDef::radii`, text
`fillet F (edges: [...], radius: 2 mm, at: [vertex(<face>, <face>, <face>) = 0.4 mm, ...])`,
command `feature.fillet` `at`, tree field `vertex_radii: [{point, radius}]`):

* **Vertex references** (`VertexRef`, `vertex(<faces>)#k`): a vertex is named by the faces
  that meet at it (sorted, as an edge by its two faces), with a `#k` index ordered by
  position where several vertices share the faces. Names are computed on demand from the
  body's face names and topology (`BodyNames::vertex_names`); resolution takes the exact
  name, else the only vertex of those faces with another index, else the only vertex where
  all named faces meet (a later feature added faces around it). Renames follow (ADR 0003).
* `radius` holds along the edges; `at` sets the radius at ends of the edges. The kernel's
  `fillet_variable(shape, edges, radius, at)` adds each edge with `radius` and sets each
  vertex radius on the contour (chain of tangent edges) holding it (`SetRadius(R, IC, V)`).
  OCCT interpolates a smooth law through the given values along each contour (a cubic
  spline, level at the contour's ends); `SetRadius` with a law function is not used (in the
  pinned OCCT it does not keep the law).
* Faces of the fillet are named as a constant fillet's (`Fillet/blend(edge(...))`).
* A vertex radius must be positive: a fillet that runs out to a point (SolidWorks allows a
  radius of 0 at a vertex) is not supported.

## Consequences
* Golden tests (OCCT): extended plane and tilted plane (exact volumes), an extended sphere
  patch (volume against a Simpson integral, 1e-7), a sphere too small to cross the sweep
  refused; a variable fillet's end rims (quarter circles of the two radii) and volume, which
  lies between the straight and the end-level law.
* The SolidWorks reader writes `extend` for curved end faces and for tilted planar faces
  smaller than the profile; the Inventor reader for every extrude to a face (where the face
  covers the end the result is the same).
* SolidWorks `VarFillet_c` is not wired yet: its radii come in pairs per edge (each
  `moEdgeDim_c` lists its two `moVertDim_c`), but which end of the edge each belongs to is not
  decoded. The r3 keycap family would also need its stems' end face (a face of the shell)
  and later chamfer edges resolved. See `docs/formats/solidworks-feature-tree.md`.
* Whether SolidWorks' "smooth transition" is OCCT's spline law is not measured; a straight
  transition (radius linear in arc length) would need intermediate points or a law OCCT keeps.
