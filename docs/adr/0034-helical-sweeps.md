# ADR 0034: Helical sweeps

Status: accepted (2026-10-20)

## Context
Threads and springs are helical. A twisted sweep of a section across the axis (how helical gears
and `generate.spring` work) is exact only when the section is the true cross-section of the
helical solid. For a thread that section is two arcs and two Archimedean spirals; the sketch has
no spiral, and spline flanks were measured (M10 x 1.5, 15 mm, real OCCT): 1.0 s at 6 % volume
error with 2 points per flank, 4.3 s at 0.01 % with 16, because every spline piece becomes a face
of the sweep. Straight segments are faster (0.6 s for 24) but faceted. None is good enough for a
thread generator.

## Decision
1. **Kernel**: `SweepPath::Helix { origin, axis, x, radius, pitch, height }` (negative pitch:
   left-handed). OCCT builds an exact helix edge as a line on a cylinder's (angle, height)
   parameter plane (`Geom2d_Line` on `Geom_CylindricalSurface`, then `BuildCurves3d`) and sweeps
   with `BRepOffsetAPI_MakePipeShell` in fixed-binormal mode along the axis, so the profile keeps
   its angle to the axis. A section in a plane through the axis then sweeps the exact helical
   solid: volume = area x 2 pi x centroid radius per turn (golden test, 2e-6 relative, 0.1 s for 5
   turns). New shim entry `gs_sweep_helix`; the sweep body is shared with `gs_sweep`. Kernel
   worker protocol 8. The mock kernel does not sweep helices.
2. **Document**: `path: helix(axis: X | Y | Z | Sketch.l3, pitch: <length>, height: <length>,
   [left: true])` on `sweep`. The radius and start direction are the profile's first region's
   inside point seen from the axis, and the helix starts level with it, so the profile is on the
   helix. A twist with a helix, a non-positive pitch or height, more than 2000 turns, or a
   profile on the axis are errors. Pitch and height are expressions (parameters drive them).
3. **Generators** use it: `generate.thread` writes an ISO 68-1 external thread: a core at the minor
   diameter and the basic tooth (crest P/8, root 3P/4, 60 degree flanks run 2 % of P into the core so
   no tooth edge lies on the core surface, which made the union invalid) swept up `length - pitch`
   and added; every point is driven by the parameters (M10 x 1.5 x 20: 0.9 s, exact volume). Springs keep the twisted section
   (exact volume, round wire across the axis; ADR 0018 amendment).

## Consequences
Modelled threads are exact and fast. Self-intersecting helices (a profile taller than the pitch)
are the user's to avoid; OCCT then fails with a hint. Thread runouts and chamfered starts are
later.
