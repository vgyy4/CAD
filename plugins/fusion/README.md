# Gusset exporter for Fusion (Autodesk Fusion 360)

**UNVERIFIED — not yet run against Fusion.** Written strictly from the published API
documentation; expect small name/behaviour fixes on the first real run.

Add-in that writes the active design's timeline to `<name>.gtree.json`
(format: `docs/adr/0042-feature-tree-interchange.md`, version 1). Gusset's `import_file`
turns that file into editable features. The add-in only reads the design; unit
conversion (cm to mm, radians to degrees) is the only processing, all mapping decisions
are in the Rust importer.

## API version targeted

Fusion API reference at https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/ as
published in 2026 (adsk.core / adsk.fusion, Python 3 embedded in Fusion). Used classes:
Design (timeline, userParameters, rootComponent), Timeline / TimelineObject
(`entity`, `isSuppressed`), Sketch (`sketchPoints`, `sketchCurves`, `geometricConstraints`,
`sketchDimensions`, `transform`), Profile (`areaProperties`, `profileLoops`),
ExtrudeFeature (`extentOne`, Distance/SymmetricExtentDefinition, `operation`),
RevolveFeature (`axis`, AngleExtentDefinition), FilletFeature / ChamferFeature
(`edgeSets`), BRepBody (`physicalProperties.volume`, `boundingBox`).

## Exported

sketch, extrude, revolve, fillet, chamfer, user parameters, final bodies. Everything else
in the timeline becomes `{"type": "unsupported", ...}`. Suppressed features and features
after the timeline marker (rolled back) are left out. Skipped silently: sketch constraint
classes without an ADR equivalent, curves that are not lines/circles/arcs (a warning is
shown). A fillet or chamfer with several edge sets of different size becomes one feature
per set (`Fillet1_1`, `Fillet1_2`). Extra informational field `reason` on unsupported
entries. Known gaps: two-sided and to-object extrudes, tapers and start offsets are not
checked (taper/offset are ignored, two-sided is unsupported); `target` is never written.

## Install

1. Copy the folder `GussetExport` (containing `GussetExport.py` and `GussetExport.manifest`)
   somewhere permanent.
2. In Fusion: Utilities > Add-Ins (Shift+S) > Add-Ins tab > green plus "Add new add-in
   from device" > select the `GussetExport` folder.
3. Select it in the list and press Run (tick "Run on Startup" if wanted). A button
   "Export to Gusset" appears in the Utilities > Add-Ins panel.

## Manual test (about ten minutes)

1. New design (units mm). Modify > Change Parameters > User Parameters > `+`: name
   `width`, unit mm, expression `40`.
2. Sketch on the XY plane: two-point rectangle from the origin to (40, 20). Dimension the
   bottom edge and set it to `width`; dimension the right edge to 20. Finish sketch.
3. Extrude the rectangle 10 mm, operation New Body.
4. Fillet 2 mm on the vertical edge at the corner (40, 20).
5. Sketch on the XZ plane: a rectangle x 75..80, z 0..10 (in Fusion's XZ sketch the
   second sketch axis may point toward -Z; read it from the exported `plane` in the
   result), plus a construction line at x = 70 from z 0 to z 10. Finish sketch.
6. Revolve that rectangle 360 degrees about the construction line, New Body.
7. Utilities > Add-Ins > Export to Gusset; save `GussetTest.gtree.json`.
8. Compare with `sample/expected.gtree.json`.

Expected: `format` `gusset-feature-tree`, `version` 1, `source.app` `fusion`; one parameter
`width` = 40 (length); features in order Sketch1, Extrude1, Fillet1, Sketch2, Revolve1;
Extrude1 `distance` 10, `profiles` `"all"`, `operation` `new`; Fillet1 `radius` 2 with one
edge midpoint `[40, 20, 5]`; Revolve1 `angle` 360, axis a sketch line; bodies: Body1
volume about 7991.4 mm3 and box `[[0,0,0],[40,20,10]]`, Body2 volume about 2356.2 mm3.

Differences expected in a real file (not errors): Fusion's origin point appears as an
extra sketch point (also at (0,0) coincident with a rectangle corner), additional
coincident constraints, different ids and `app_version`, the sketch-plane axes of
Sketch2 as described above, and Body2's box depends on the revolve (the sample uses axis
x = 70, outer radius 10).

Report any exception text (the add-in shows it in a message box) and the exported file so
the add-in can be corrected.
