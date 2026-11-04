# ADR 0038: Delete face (direct modeling)

Status: accepted (2026-11-04). PROMPT.md "Phase 3+": direct modeling (Solid Edge
synchronous-style) after sheet metal.

## Context
Direct modeling edits a solid's faces without its history: imported parts above all,
where there is no feature to change. Gusset already has the push/pull and move tools
(`offset_face`, ADR 0030; `move_face`, ADR 0031; `draft`, ADR 0028). The tool every direct
modeller pairs with them is *delete face*: remove a hole, a fillet, a chamfer, a boss or a
logo and heal the part, as if the detail had never been modelled (Fusion "Delete",
SolidWorks "Delete Face > Delete and Patch", Onshape "Delete part face", Solid Edge
"Delete faces").

## Decision
`delete_face DeleteFace1 (body: Extrude1, faces: [Extrude2/side(S2/c2)])` removes the
faces and extends the faces around them until they close the gap, with OCCT's
`BRepAlgoAPI_Defeaturing` (history kept, so every remaining face keeps its name through
`Modified`). The kernel trait gains `delete_faces` (default: unsupported), carried through the
shim, the worker protocol (v10) and the pool; the command is `feature.delete_face` in the
SOLID tab's Modify group.

When the neighbours cannot close the gap (the top face of a block, a face with nothing
around it to extend), OCCT leaves the face in place with a warning; the shim checks that
every requested face is gone and otherwise fails with "the faces cannot be removed".

## Consequences
* An OCCT test removes a through hole and a fillet from a block and gets the exact block
  back (6 faces, its original names), and the top-face case fails cleanly.
* Delete face on a feature's faces is the direct-modelling counterpart of suppressing that
  feature; on imported bodies it is the only way. Not here: replacing a face by another
  surface ("replace face") and live rules (keeping faces coplanar or concentric during a
  move); those may follow.
