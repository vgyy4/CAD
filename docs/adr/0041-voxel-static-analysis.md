# ADR 0041: Simulation phase 1 — linear static analysis on a voxel grid

Status: accepted (2026-11-04). PROMPT.md "Phase 3+": simulation after CAM.

## Context
The first question a designer asks of a part is "will it bend or break under this load?".
Fusion, Onshape (with SimScale) and FreeCAD FEM answer it with tetrahedral meshes, which need
a robust 3D mesher (Gmsh, Netgen, TetGen); none is in Gusset's dependency set, and OCCT ships
only surface meshing. A voxel model needs only the solid's closed triangle mesh, which the
kernel already gives, and is robust on any solid the kernel can tessellate.

## Decision
A new crate, `gusset-fea`, pure Rust:

* **Model**: the body's mesh is filled with cubic voxels of edge `voxel` from its bounding
  box's corner; a voxel is in when its centre is (ray parity along X, one ray per voxel row,
  triangles binned by row, the ray nudged off edges). Each voxel is an 8-node hexahedron of an
  isotropic linear elastic material (E, ν), stiffness by 2×2×2 Gauss points.
* **Conditions**: faces are named as everywhere (`Extrude1/cap(start)`); a face acts on the
  voxel faces on the model's boundary whose centre lies within half a voxel of its triangles and
  whose normal is not across it. Fixed faces hold those nodes; a load (total force, N) is spread
  over them by area (each boundary quad a quarter to each corner), which is the consistent load
  of a bilinear face.
* **Solve**: matrix-free conjugate gradients with a Jacobi preconditioner, element by element;
  results: the largest displacement and the largest von Mises stress at an element centre, with
  where they are. Limits: 200 000 voxels, 20 000 iterations (an unheld model does not converge
  and says so).
* **Command**: `sim.static` (body, fixed faces, loads, E default 210 000 MPa, ν 0.3, voxel default
  the longest side / 30); in the SOLID tab's Inspect group as "Stress".

## Consequences
* Tests: a bar in tension stretches by exactly `PL/EA` with stress `P/A` (with ν = 0; the
  hexahedra are exact there), through the solver and through the command with face names; a
  cantilever bends to within 20 % of beam theory (full-integration bricks are a little stiff).
* Results are an estimate: curved faces become staircases (stress peaks there are not to be
  trusted) and the mesh is uniform. Next: refinement near loads and fixes (octree), stress
  colours in the viewport, modal analysis, and a tetrahedral path if a mesher is added.

## Amendment (2026-11-04): stress picture

`Solution::surface` holds every outer voxel face (wound to face outwards) with its voxel's von
Mises stress. `sim.static` with `field: true` sorts them into 8 equal bands from 0 to the
largest stress (`stress_bands: [{upto_mpa, quads}]`, at most 100000 faces). The app asks for
it, builds one mesh per band coloured blue to red, shows it in place of the model (the
model's edges stay faintly drawn) while the result dialog is open, and lists the colours in
the dialog.
