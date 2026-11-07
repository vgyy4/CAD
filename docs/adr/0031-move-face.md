# ADR 0031: Move face (direct edit)

Status: accepted (2026-10-15)

## Context
After offset face (ADR 0030), the other common direct edit is moving faces rigidly: slide
a hole or boss to a new spot, turn a slot about an axis, push a side face out. Fusion
(Move, faces), SolidWorks (Move Face, translate/rotate) and Onshape (Move face, translate)
all have it. OCCT has no single API for it: `BRepOffset_MakeOffset` only moves along
normals, and the defeaturing algorithm removes faces instead of moving them.

## Decision
1. Statement `move_face <Name> (faces: [<face>], translate: [x, y, z], axis: <dir>,
   angle: <angle>, through: [x, y, z], [body: <Feature>])`, command `feature.move_face`.
   The motion is the same as `move`'s: a turn first, then the translation. A move that
   evaluates to nothing (a parameter at 0) leaves the body as it is, so parameter ranges
   may include 0.
2. Kernel: `Kernel::move_faces(shape, faces, steps)` with the steps as `Transform`s; worker
   protocol 6 (request 27). The shim uses a `BRepTools_Modification` that delegates to
   `BRepTools_TrsfModification` for the moved faces and their edges and vertices. Every
   other face keeps its surface and every other edge its curve. Moved edges get curves on
   the faces that stay by projection (`GeomProjLib::Curve2d`). Vertices that move along an
   edge that stays get their new parameter on it (`GeomLib_Tool::Parameter`).
3. The move is checked before anything is built. Each face that stays must contain the
   moved edges on it, and each edge that stays must contain its moved ends (5 samples per
   edge, tolerance 1e-6 mm). Otherwise the move is refused: the faces around would have to
   tilt or bend, which would need new surfaces and topology. In practice:
   - a hole or boss slides or turns along the face it sits on;
   - a side face moves along its normal while the faces next to it stretch;
   - a top face slid sideways is refused.
   The result must pass `BRepCheck_Analyzer` and a self-interference check
   (`BRepAlgoAPI_Check`), so a hole pushed through the part's side is refused too.
4. Names: a modification; every face keeps its name. The topology does not change.

## Consequences
- Golden tests: crates/gusset-kernel-occt/src/tests.rs `move_faces_golden` (a hole slid and
  turned, a side face, refusals) and crates/gusset-cli/tests/modeling_move.rs (a hole by a
  parameter, a turn, a boss with its top, a zero move, the command).
- Not yet: moves that need the neighbours to change shape (Fusion extends or tilts them),
  replace face, delete face with healing.

## Amendment (2026-11-05): onto (replace face)

`move_face` takes `onto: <face>` instead of a motion: the planar faces move along their
normal onto the plane of that face, a parallel planar face of any body (read when the
feature is rebuilt, so the motion follows the face when it moves). A tilted target is
refused before the kernel is asked: turning a face onto it would reshape the faces around
it, which a face move cannot do (use draft). Laying a face flush with a neighbouring face,
where a face between them would vanish, is refused by the kernel with a hint to extrude up
to the face and combine instead. OCCT tests in `modeling_move.rs`.

## Amendment (2026-11-07): onto a tilted plane

`onto` a planar face that is neither parallel nor perpendicular turns the faces about the
line where their plane meets the target's, by the smaller angle that makes them parallel (the
faces keep their side). A rigid move cannot do this, because the faces around would have to
change shape, so it is a draft: the pull lies in the faces, across that line, and the neutral
plane holds the line. The draft's sign is chosen by checking the turned face's normal.
Tested with OCCT for hinges on the face's edge, inside it and beyond it (exact volumes).


## Amendment (2026-11-07): live rules

`move_face (..., rules: true)` moves, besides the listed faces, the faces related to them by
a rule, so design intent survives a move:

- coplanar: other planar faces of the body in the same plane (same outward direction, same
  offset, 1e-6 mm) as a moved planar face;
- coaxial: cylindrical and conical faces whose axis is the line of a moved cylinder or cone
  (a counterbore's two walls, the halves of a split cylinder).

The evaluator derives the extra faces from the body's topology each time it resolves the
faces (`gusset_eval::rule_faces`), so they follow the model on rebuild; they are not stored.
Symmetric and equal-radius rules are not implemented. With `onto` tilting (a draft) the rules
do not apply.

Default: the document field is `rules: bool`, printed only when true, and absence means false,
so documents written before keep their behaviour. The `feature.move_face` command defaults
`rules` to true (design intent is what a direct edit is for) and reports the faces it added as
`also_moved`. Without rules, a slotted block whose one top face is lifted fails or only that
face moves, as before (the kernel decides). `offset_face` has no rules yet.
