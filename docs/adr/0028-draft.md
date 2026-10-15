# ADR 0028: Draft (taper faces for molding)

Status: accepted (2026-10-15)

## Context
Draft is in the v1 tier of the feature matrix (docs/RESEARCH.md): every molded, cast or
3D-printed-with-a-mold part needs its walls tapered so it releases. Fusion ("Draft": pull
direction from a plane, faces, angle, flip), SolidWorks ("Neutral plane" draft) and
Onshape ("Draft": neutral plane, faces, angle, opposite direction) agree on the same model:
faces turn about their intersection with a neutral plane; the pull direction is that
plane's normal and can be reversed. OCCT provides it as `BRepOffsetAPI_DraftAngle`
(planar, cylindrical and conical faces).

## Decision
1. Statement `draft <Name> (faces: [<face>], plane: <plane>, angle: <angle>, [flip: true],
   [body: <Feature>])` and command `feature.draft` (GUI = CLI = MCP). `plane` is a
   `PlaneRef` like mirror's: XY/XZ/YZ, a planar face, a datum plane.
2. Pull direction: the base or datum plane's normal; for a face of the body, into the body
   (selecting the bottom face pulls the part up out of its mold). `flip` reverses it.
   A positive angle leans the faces inward along the pull direction, so the part narrows
   that way; negative leans them outward. `0 < |angle| < 90°`.
3. Kernel: `Kernel::draft(shape, faces, &DraftSpec { pull, neutral, angle })`; the OCCT
   shim maps it to `BRepOffsetAPI_DraftAngle::Add(face, pull, angle, neutral plane)` and
   reports faces through `ModifiedShape` (`Modified` is empty for drafted faces). A face
   that cannot be drafted (parallel to the plane, a free-form face) is an error naming it.
   The mock kernel reports `Unsupported`. Worker protocol version 4 (request 25).
4. Naming: a draft is a modification; every face keeps its name (`OpContext::Modify`), so
   later features referencing the walls still resolve.

## Consequences
- Golden tests: a box tapered into a frustum (exact prismatoid volume) about a base plane
  and about its top face, flip/negative angle, a cylinder tapered into a cone frustum,
  errors, a shell after a draft, and the command (crates/gusset-cli/tests/modeling_draft.rs).
- Not yet: parting-line draft, step draft, draft analysis (colouring faces by their angle
  to the pull direction); they can build on the same kernel call.
