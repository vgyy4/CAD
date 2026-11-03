# ADR 0036: Surface workspace (sheet bodies, patch, thicken, stitch)

Status: accepted (2026-11-03). DESIGN.md §2: the SURFACE tab appears once surface features
ship.

## Context
Every body so far is a solid. Industrial and consumer design often starts from surfaces:
an open profile extruded into a wall, a planar patch closing an opening, sheets stitched
into a closed solid, or a sheet thickened into a part. Fusion, Onshape and FreeCAD all offer
these four as the core of their surface tools.

## Decision
One document feature, `surface`, with four operations, each producing or consuming a
*sheet body* (an OCCT shell or face, not a solid). Sheet bodies are ordinary named bodies:
they are drawn, picked and exported like solids, report `kind: sheet` in `query.body`, and
features that need a solid (fillet, shell, booleans) refuse them with a clear error.

```
surface Wall (extrude: Sketch1/curves(l3, a5, l7), distance: 20 mm, symmetric: false)
surface Lid (patch: Sketch2/regions(l4))
surface Part (thicken: Wall, thickness: 2 mm, both_sides: false)
surface Closed (stitch: [Wall, Lid, Base], tolerance: 0.01 mm)
```

* **extrude**: sketch curves (open or closed chains) swept along the sketch normal into
  faces, one per curve (`BRepPrimAPI_MakePrism` on edges). Lateral faces are named
  `side(<sketch>/<entity>)` as for solid extrusions.
* **patch**: a sketch region filled as a planar face (`BRepBuilderAPI_MakeFace` on the
  region's wires), face named `patch(<sketch>/<region>)`.
* **thicken**: a sheet offset to one side, or half to each side, into a solid
  (`BRepOffsetAPI_MakeThickSolid` by simple offset); the sheet's faces keep their names
  on the solid's offset copies.
* **stitch**: sheets sewn along shared edges within a tolerance (`BRepBuilderAPI_Sewing`);
  when the result is closed it becomes a solid, else a sheet. The input bodies are consumed
  (as booleans consume tools).

The kernel trait gains `extrude_curves`, `patch`, `thicken` and `stitch` (default:
unsupported), implemented in the OCCT shim, the kernel worker protocol and the mock kernel.
The SURFACE tab lists the four commands (`feature.surface_extrude`, `feature.patch`,
`feature.thicken`, `feature.stitch`).

## Consequences
* Bodies carry a kind; sheet bodies have zero volume, and mass properties report area.
* Not in this step: surface lofts and sweeps, fill with boundary conditions (G1/G2),
  trimming and extending surfaces, untrim. They can follow as further `surface` operations.
