# ADR 0030: Offset face (direct edit)

Status: accepted (2026-10-15)

## Context
"Direct edit (move/offset face)" is in the v1 tier (docs/RESEARCH.md): changing an
imported part or a finished model without going back through its history. Fusion has
Press Pull / Offset Face, SolidWorks Move Face (offset), Onshape Move face (offset), Solid
Edge synchronous editing; FreeCAD has no direct equivalent. The common core is "move these
faces along their normals by a distance, and let the neighbouring faces follow". OCCT does
it with `BRepOffset_MakeOffset` given a base offset of 0 and per-face offsets
(`SetOffsetOnFace`), which keeps the topology and handles planar and curved faces (a hole's
radius changes by the distance).

## Decision
1. Statement `offset_face <Name> (faces: [<face>], distance: <length>, [body: <Feature>])`,
   command `feature.offset_face`. Positive distances move faces outward (adding material).
   It is an ordinary history feature, so on an imported body it is a direct edit and on a
   modelled body it is one more step; both stay parametric in `distance`.
2. Kernel: `Kernel::offset_faces(shape, faces, distance)`; the shim's face history comes from
   the algorithm's `OffsetFacesFromShapes` image (its `Modified` is empty). Worker protocol 5
   (request 26). The mock kernel reports `Unsupported`.
3. Names: a modification; every face keeps its name.

## Consequences
- Golden tests: box faces up and down, two faces at once, a hole's radius
  (crates/gusset-kernel-occt/src/tests.rs, crates/gusset-cli/tests/modeling_offset.rs).
- Move face by a translation or rotation: ADR 0031. Not yet: replace
  face, delete face with healing.
