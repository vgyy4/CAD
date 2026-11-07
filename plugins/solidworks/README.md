# Gusset exporter for SolidWorks (VBA macro)

**UNVERIFIED — not yet run against SolidWorks.** Written from the official API help only.

- Target API: **SolidWorks 2024 API** (help.solidworks.com/2024/english/api; `ISldWorks.RevisionNumber` 32.x).
- Output: `<part>.gtree.json`, format `gusset-feature-tree` version 2 (ADR 0042).
- Read-only: the macro never edits the model. Parts only (not assemblies).
- The macro is late bound (no type-library references). Enum values are numeric constants at the top
  of `GussetExport.bas`; several were written from memory and are marked `VERIFY`/`UNVERIFIED`.

## Install

1. Open SolidWorks, then Tools > Macro > New... and save an empty macro as `GussetExport.swp`
   (this opens the VBA editor), or Tools > Macro > Edit... on an existing `.swp`.
2. In the VBA editor: File > Import File... and choose `GussetExport.bas`. Delete the empty
   default module (`Module1`/`Macro1`) if it also defines `main`.
3. Debug > Compile to check for errors. Report any compile error line: it names an API member I got wrong.
4. Run: Tools > Macro > Run..., pick `GussetExport.swp`, procedure `main`.

## What it exports

sketch (lines, arcs, circles, points; horizontal/vertical/tangent/parallel/perpendicular relations),
extrude (blind, mid-plane, symmetric both-directions; boss and cut), revolve (one-direction or
mid-plane, axis = a sketch line), simple constant-radius fillet and equal-distance chamfer on edges
(by edge midpoint), Hole Wizard holes (simple, counterbore, countersink), linear and circular
patterns and mirrors of features, shell (UNVERIFIED member names; check on the first run), user parameters (global variables), final bodies (volume, bounding box).
Everything else is written as `"type": "unsupported"` with the SolidWorks type name; suppressed
features are skipped. Profiles are always `"all"`; dimensions are not exported as constraints (their
values are in the geometry); extrude depth links to a global variable only when an equation
`"D1@<Feature>" = "<global>"` exists.
Conversions: metres to mm, radians to degrees, and Y-up to Z-up (x' = x, y' = -z, z' = y) for every
point and direction. Sketch plane: origin and axes come from the inverse of `Sketch.ModelToSketchTransform`.

## Manual test (about ten minutes)

1. New part, units MMGS. Tools > Equations > add global variable `width` = `40mm`.
2. Sketch on the **Top** plane: Corner Rectangle from the origin to (40, 20) mm (the y direction of the
   sketch points towards the back of the model). Exit sketch (`Sketch1`).
3. Boss-Extrude1: blind, 10 mm, in the direction away from the Top plane's lower side (up, +Y).
4. Fillet1: 2 mm constant radius on the vertical edge at the far corner (sketch x = 40, y = 20).
5. Sketch on the **Front** plane (`Sketch2`): rectangle from (20, 10) to (25, 20), plus a vertical
   centerline from (20, 10) to (20, 20). Revolve1: Revolved Boss, axis = the centerline, 360 degrees.
6. Save as `GussetTest.sldprt`, run the macro. A message box lists the file and any warnings.
7. Compare `GussetTest.gtree.json` with `sample/expected.gtree.json`. Expected after the Y-up conversion:
   - Top plane sketch: `y_axis` is `[0, 1, 0]` (SolidWorks has it at `(0, 0, -1)`), Front plane
     sketch: `y_axis` is `[0, 0, 1]`.
   - Extrude `distance` 10 (positive); fillet midpoint `[40, 20, 5]` (SolidWorks: `(40, 5, -20)` mm),
     radius 2; revolve angle 360, axis `{ "line": <the construction line> }`.
   - If you add a Hole Wizard counterbore (4 mm hole, 8 mm x 3 mm counterbore, through all) at (32, 10)
     on the top face after Revolve1, it appears as a `hole` with `variant` `counterbore` and
     `positions` `[[32, 10, 10]]` (the sample has it).
   - Parameter `width`: 40 mm. One body, volume about 8538.1 mm3 (with that hole), bbox `[[0, -5, 0], [40, 20, 20]]`.
   - Allowed differences: ids and the order of points, curves and constraints (SolidWorks decides the
     line order of a rectangle), feature names (localised), `app_version`.
8. Tell the Gusset session what differs; each difference points at an API member to fix.

## Least certain API points (check first if the test fails)

- `IExtrudeFeatureData2`: `GetEndCondition/GetDepth/BothDirections/ReverseDirection/Merge`; extrusions
  read through `Feature.GetDefinition` (the object `GetSpecificFeature2` returns differs by feature type;
  sketches use `GetSpecificFeature2`).
- `IRevolveFeatureData2`: `Type`, `GetRevolutionAngle`, `ReverseDirection`, `Axis`.
- `ISimpleFilletFeatureData2.GetFilletItems` (may need `GetFilletItemAtIndex` instead), `IsMultipleRadius`,
  `DefaultRadius`; `IChamferFeatureData2.GetEdges/GetEdgeChamferDistance` (guessed names).
- Sketch relations through `ISketch.RelationManager.GetRelations`, entity identity through `GetID`;
  `swConstraintType_e` numbers; arc direction through `ICurve.CircleParams` axis versus the sketch normal.
- `IEquationMgr.Value` units for formula (non-literal) global variables; `swUnitsLinear` constant.
- Cut-extrude direction: the sign of `distance` for cuts follows `ReverseDirection` only.
- Consumed sketches are found as sub-features (`GetFirstSubFeature`); `GetNextFeature` skips them.
