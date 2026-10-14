# ADR 0026: Joints and assemblies (phase 1: joints between bodies of one document)

- Status: accepted (phase 1 in implementation)
- Date: 2026-10-14
- Spec: v0.3 "assemblies and joints (research Fusion joints vs SolidWorks mates first)"

## Research
- **Fusion 360**: joints between components, defined on *joint origins* (frames snapped
  to faces, edges, points); types rigid, revolute, slider, cylindrical, pin-slot, planar,
  ball; the first selection moves; joints live in the timeline; joint limits and motion
  links; "as-built" joints keep the current position.
  ([Autodesk API JointTypes](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/fusion_JointTypes.htm),
  [Noble Desktop](https://www.nobledesktop.com/learn/cad/understanding-joints-in-fusion-360))
- **Onshape**: mates between *mate connectors* (coordinate systems on faces/edges);
  fastened, revolute, slider, cylindrical, planar, ball, parallel, pin slot, tangent;
  relations (gear, rack and pinion, screw) couple mates.
  ([Onshape help](https://cad.onshape.com/help/Content/mateconnector.htm))
- **SolidWorks**: geometric mates (coincident, concentric, parallel, distance, angle)
  solved together; flexible but over-/under-constraint is a common user problem.
- **FreeCAD 1.0** Assembly workbench: grounded part, joints with offsets and limits,
  the open-source Ondsel solver (also multibody dynamics); still young.
  ([FreeCAD wiki](https://wiki.freecad.org/Assembly_Workbench))

Conclusion: frame-to-frame joints (Fusion/Onshape) map to one rigid transform per joint
and are easy for agents to state and check; geometric mate systems need a global solver
and produce ambiguous states. We follow the frame model.

## Decision

### 1. Joints are timeline features that place bodies
A `joint` feature places a *moving* body (and the bodies listed in `with:`, carried
rigidly) so that its frame coincides with a frame on a *fixed* body (or the world origin
or a datum plane), with the joint's motion applied in between:

`X_moving' = F_fixed · M(type, values) · F_moving⁻¹ · X_moving`

Phase 1 is a tree: a body is placed by at most one joint (a second joint moving the same
body is an error naming the first), and a joint's fixed side must be a body that is
already in place (earlier in the timeline). Closed loops need a simultaneous solver
(phase 2, sketch solver technique in 3D).

### 2. Frames (joint origins) from persistent names
`frame(<face>)`: planar face → centroid, Z = outward normal; cylindrical/conical face →
the axis point nearest the face centroid, Z = axis; spherical face → center.
`frame(<edge>)`: circular edge → center, Z = circle normal; straight edge → midpoint,
Z = direction. `frame(origin)`, `frame(XY|XZ|YZ|<datum plane>)`. X is the projection of
the world X axis onto the frame plane (world Y when X is parallel to Z), so frames are
deterministic. Options: `flip: true` reverses Z, `offset:` moves along Z, `rotate:` turns
about Z. Faces and edges are persistent names (ADR 0003), so joints survive edits.

### 3. Types and values
rigid (none), revolute (`angle`), slider (`distance`), cylindrical (`angle`, `distance`),
pin_slot (`angle` about Z, `distance` along X), planar (`x`, `y`, `angle`), ball
(`angles: [rx, ry, rz]`, X-Y-Z order). Values are expressions, so parameters drive motion
(an agent or a GUI slider sets a parameter; thanks to dependency keys only the moved
bodies rebuild). `limits:` per value; a value outside its limits fails the joint with the
range in the error.

### 4. Kernel support
The topology summary reports, per face, the axis of revolved surfaces (cylinder, cone,
sphere center, torus) and, per edge, the center/normal/radius of circles. Worker protocol
version bump.

### 5. Commands and UI
`feature.joint` (registry → GUI form, CLI, MCP), `query.joints` (types, frames, values,
degrees of freedom left), browser category "Joints", timeline chips. Phase 2: components
loops.

Relations (gear, rack and pinion, screw) need no feature of their own: a joint value is an
expression, so `angle: -turn * 20 / 40` couples a gear pair (grammar "Joints").

### 6. Interference and motion studies (phase 2, done)
`query.interference` checks pairs of bodies whose boxes overlap by their common volume;
contact (an empty or zero-volume common part) is not interference. `study.motion` sweeps
a parameter `from + (to - from)·t` in equal steps on a copy of the document (the
evaluator's cache keeps unaffected features), and reports per step the rebuild errors,
body centroids and, optionally, interference; the document does not change. `query.bom` groups bodies into parts (same volume, area,
face and edge counts, wherever placed) and gives the rows as JSON and CSV.

## Consequences
- An assembly in phase 1 is a multi-body part whose bodies are positioned by joints:
  enough for mechanisms (hinges, sliders, gear pairs at a centre distance) and for agents
  to describe placements without coordinates.
- Moving parts stay parametric and cheap to animate.

### 7. Components from other documents (phase 2, done)
`component <Name> (source: "<path>", version: "<node>", document: "<hash>", [body: <B>])`
brings in one body of another `.gus` document. `assembly.insert_component` reads the file
(agents: confined to the project folder, like imports), takes the head of its main branch
(or `branch`), stores that document's text as a blob together with every blob it needs
(its imports, and the documents of its own components, recursively, at most 8 deep), and
records the source path and node. The evaluator rebuilds the stored document with a fresh
evaluator on the same kernel (same cancel flag and limits, no speculation) and takes the
body; so a component is pinned: it changes only when `assembly.update_component` reads
the source again and pins its new head. One body per component keeps "one feature, one
body", which validation and joints rely on; a second body is a second component.

Faces are named `<Name>/part(<face name in the component's document>)`: names inside a
component are as stable as in its own document, and the inner name is not a reference to
this document's features (dependencies and renames stop at the component). A component
starts at its own coordinates; joints place it.

Parameters per instance: `set: [teeth = 30, w = 2 * a]` evaluates each expression with
this document's parameters and gives the component's parameter that value as a literal in
base units (mm, rad or a plain number), so one part file can be used in several sizes and
a parameter of the assembly can drive a part (`assembly.configure_component`).

`query.components` lists the components with `update_available` (the source's head
differs from the stored copy), so an agent or the GUI can offer the update.
