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

## Amendment (2026-11-08): history-free modelling (`feature.bake`)

`feature.bake {confirm: true}` turns a part into dumb solids, the "history-free mode" of
the later tier: each body, as evaluated now, is written in the kernel's native B-rep form
(trusted: this process wrote it) and becomes an `import` feature named after the body; the
features that made the bodies go. Sketches and datum planes standing on XY/XZ/YZ or on such
datum planes stay (they do not depend on bodies), and so do the CAM jobs on kept sketches;
parameters stay. What follows is direct editing (`move_face`, `offset_face`, `delete_face`,
fillets, cuts on the imports). Refused while the model has rebuild errors or the timeline is
rolled back. One edit: `history_undo` brings the history back, and other branches keep it.
