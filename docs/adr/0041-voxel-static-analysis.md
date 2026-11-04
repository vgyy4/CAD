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

## Amendment (2026-11-04): natural frequencies

`sim.modal` finds the lowest 1–10 natural frequencies of a body held at its fixed faces
(density in kg/m³, default steel 7850). The mass is lumped (an eighth of each voxel's mass
on each of its nodes), so `M` is diagonal and the problem becomes the standard one for
`M^-1/2 K M^-1/2` on the free degrees of freedom. LOBPCG solves it (block of the modes asked
plus 4, Jacobi preconditioner, Rayleigh-Ritz on `[X, W, P]` with the operator's images
carried through the Gram-Schmidt and refreshed every 25 iterations, a cyclic-Jacobi dense
eigensolver for the small projected matrix), until each wanted residual is below 1e-4 of
its eigenvalue. A 100 × 10 × 10 mm steel cantilever at 2.5 mm voxels reads 848 Hz against
835.5 Hz from beam theory, the two bending modes equal. The Jacobi preconditioner leaves
the iteration count near the square root of the condition number (about 500 there); a
multigrid preconditioner is the next step for large grids. gusset-fea is built at
opt-level 2 in the dev profile so the tests stay fast.

## Amendment (2026-11-04): review fixes

A review found that a legal request could run for minutes to hours (a 200k-voxel static
solve took 360 s; an 80k-voxel modal solve did not finish in 20 min). Both solvers now
stop with `too_large` past a work budget of 1.5e8 element products (about half a minute
on one core), and a frequency analysis takes at most 50k voxels. Before solving, every
voxel must be reachable through shared faces from a held voxel: parts joined only along
an edge or at a corner (which thin diagonal features voxelise into) are free to turn, and
are refused with the count of loose voxels. The modal residual test has a floor at 1e-10
of the operator's scale, so a zero frequency converges and is reported as a part free to
move. A load whose nodes are all held is refused, the share of a load landing on held
nodes is reported (`held_load_share`), a request without loads is refused, Young's
modulus is bounded to 1e-3..1e9 MPa and forces to 1e12 N, and non-finite results are an
error. Results are rounded to 6 significant digits. The stress picture holds at most
20000 voxel faces and thins out evenly beyond that. The app ends the stress picture as
soon as another dialog replaces its result. Not done: shell elements for thin sheet
bodies, which cannot be voxelised finely enough.

## Amendment (2026-11-04): multigrid preconditioner

Both solvers now precondition with a geometric multigrid V-cycle (`mg.rs`) instead of
Jacobi. Coarser levels are made of voxels of twice the edge covering the finer ones, down
to 64 voxels or 10 levels; each coarse voxel's stiffness is rediscretised and scaled by
its filled share of the fine voxels below it (without that the coarse body is too stiff
where it overhangs the part, and the cycle helped only threefold). The prolongation is
trilinear on the shared grid nodes, restriction its transpose; smoothing is one weighted
Jacobi sweep before and after the coarse correction (weight 0.9 over a 15-step power
estimate of the largest eigenvalue), 40 sweeps on the coarsest level; held degrees of
freedom are zero on every level. The cycle is symmetric and positive, so CG and LOBPCG keep
their guarantees; LOBPCG applies it as `M^½ V M^½`. Measured (release, one core): an
80k-voxel cantilever took 77 CG iterations against 997, a 64k-voxel cube 27 against 382,
about 2.7 times faster in time with the element product rewritten to slices (5 times
faster than before both changes); the 100 × 10 × 10 mm cantilever's frequencies took 30
LOBPCG iterations against 525, same values. The work budget counts 6 stiffness products
per iteration.

