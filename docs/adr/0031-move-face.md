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
