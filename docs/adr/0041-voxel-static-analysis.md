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

## Amendment (2026-11-04): mode shapes

`sim.modal` with `shape: k` (1 = the lowest mode) also returns that mode's shape: the outer
voxel faces moved by the mode (the largest motion drawn as a tenth of the body's longest
side), sorted into 8 bands by how far each face moves relative to the largest motion
(`shape_bands`, the same `[{upto, quads}]` form as `stress_bands`, whose bound is now
`upto` for both). The app asks for the lowest mode's shape and shows it like the stress
picture, while the result dialog is open.

## Amendment (2026-11-04): refinement at the peak (submodel)

`sim.static` with `refine: true` solves again in a box around the stress peak (three coarse
voxels each way) with voxels four times finer. The box's grid starts at its own corner;
its voxels are found by the same ray parity (crossings counted along the whole ray). The
nodes on the box's faces that cut the material are held at the coarse displacement,
interpolated trilinearly in the coarse voxel holding them; the fine problem is solved for
the change from that (right-hand side less `K u0`). Held faces and loads inside the box
apply as before; loads outside it are skipped. The fine voxels along the cut faces are left
out of the peak, since they carry the coarse solution's error. A bar in tension keeps its
10 MPa; a 2.5 mm cantilever's root reads 41.6 MPa coarse and 56.2 MPa refined against
60 MPa from beam theory. Near sharp inside corners and held edges the stress has no
finite limit, so the refined value is a trend, not a number (said in the code's docs).


## Amendment (2026-11-07): shell elements

Voxels are poor for thin sheet metal (a 1-2 mm sheet needs tiny voxels), so
`gusset_fea::shell::shell_static` solves a mid-surface mesh of flat 4-node shells
(`ShellMesh { nodes, quads, tris }`, `ShellProblem { mesh, thickness, material, fixed, loads }`;
mm, N, MPa). Not wired into commands yet.

- Element: flat in its own plane (local frame from the quad's diagonals, nodes projected on
  the mean plane, so a folded mesh such as an L-bracket works: each element is rotated to
  global axes), six dofs per node. Membrane: bilinear plane stress with Wilson-Taylor
  incompatible modes condensed out (exact for rectangles in bending). Plate: Mindlin
  (shear factor 5/6) with MITC4 assumed natural shear strains (Bathe-Dvorkin), so no shear
  locking in the thin limit. Drilling: a penalty `0.05 G t A` on `theta_z - (v,x - u,y)/2`,
  which keeps the rotation about the normal defined without perturbing the membrane.
- Solver: 6x6-block CSR, Jacobi-preconditioned CG (relative residual 1e-10); fixed nodes
  clamp all six dofs; more than 200 000 nodes is refused (`TooLarge`). Jacobi CG needs
  about 100 iterations on a 16x16 plate and about 1600 on the t = 0.1 mm strip; large thin
  meshes would want a stronger preconditioner.
- Output: node displacements and rotations, per-quad von Mises at the element centre on the
  top and bottom surfaces (membrane plus or minus bending, transverse shear left out).
- Limits: triangles are rejected with a clear error (mesh quads only); uniform thickness;
  warped quads are flattened, not corrected; stresses are element-centre values, so the
  clamped-edge peak reads low on coarse meshes; folds are rigid joints (the shared node has
  one set of six dofs), which is right for bent sheet but not for hinges.
- Tests (`shell/tests.rs`), against closed form: cantilever strip 100 x 10 x 1 on a 20 x 2
  mesh, tip load across the thickness, 1.2 % off PL^3/3EI (tilted copy identical to 1e-9
  relative); in-plane loading, 0.4 % on 40 x 4 and 0.2 % on 20 x 2; clamped square plate
  under uniform nodal load vs `0.00126 q a^4 / D`, 0.9 % on 16 x 16; thickness 0.1 vs 1,
  deflection ratio 999 against 1000; an L-bracket under tip loads along either axis, 1.2 %
  and 0.9 % off beam superposition.

## Amendment (2026-11-07): sheet bodies on shells in `sim.static`

A sheet-metal body (one with a flat pattern) is analysed on the flat-shell elements:
`gusset_fea::skin::mid_surface` takes one side of the sheet from the body's surface mesh (the
faces reached from the largest one across smooth edges: flats and bends, not the thickness
strips or hole walls), refines it by splitting the longest edge first to the element size
(`voxel`, default the longest side / 40 but at least the thickness), cuts each triangle into
three quads and moves the nodes in by half the thickness. Held and loaded faces take the nodes
whose own point, or its point on either surface, lies on the face (loads shared equally). The
result says `method: shell`. OCCT tests: a 100 x 10 x 1 strip within 1 % of beam theory, and
the strip with a 90 degree flange within 10 % (the bend's own give left out). `refine` still
uses voxels.

Preconditioner (2026-11-07): the CG is preconditioned with a threshold incomplete Cholesky
factor (drop tolerance 1e-3) of the diagonally scaled stiffness `D^-1/2 K D^-1/2` in reverse
Cuthill-McKee order; the scaling evens out translation and rotation dofs, and a diagonal
shift is raised if a pivot fails. On the 4 745-node skin-built strip this takes 297
iterations instead of 10 902 (and 5 instead of 463 on the 20 x 2 strip); results agree to the
solver tolerance.

## Amendment (2026-11-07): Modes on shells

`gusset_fea::shell_modes(&ShellModalProblem)` gives the lowest natural frequencies (Hz,
ascending, 1 to 10 like the voxel `modes`) and per-mode node translations (largest node
translation scaled to length 1) of the flat-shell model, reusing the shell's element
stiffness, block CSR assembly and ICT preconditioner (`shell_static` itself is unchanged; its
assembly and input checks became private helpers). Units mm, N, t/mm^3: `f = sqrt(lambda) /
(2 pi)`.

- Mass: lumped. Each element gives every corner `rho t A / 4` on the three translations and
  `rho t^3 A / 48` on all three rotations (bending rotary inertia; the drilling rotation gets
  the same value, since it is a penalty dof with no inertia of its own and only needs `M`
  positive definite; its spurious modes lie far above the structural ones). Fixed dofs are
  eliminated.
- Eigen solver: inverse subspace iteration with Rayleigh-Ritz, not LOBPCG (the voxel LOBPCG is
  tied to its multigrid operator). Block of `modes + max(modes, 4)` vectors; each pass solves
  `K Y = M X` with the ICT-preconditioned CG (tolerance 1e-10), M-orthonormalises `Y`, and
  rotates by the eigenvectors of `Yᵀ K Y` (Jacobi). Stops when the wanted eigenvalues change
  by less than 1e-9 relative (5 passes on the 20 x 2 strip). Deterministic start block.
- Nothing fixed is an error ("a free shell has rigid-body modes at 0 Hz"), as in the static
  solver; a singular stiffness is reported as not converged.
- Tests (`shell/modal/tests.rs`), against closed form: cantilever strip 100 x 10 x 1 on 20 x 2:
  first bending 0.65 % off Euler-Bernoulli, second 0.58 %, first in-plane bending 0.60 %
  (identified by its shape); clamped square plate on 16 x 16 vs lambda^2 = 35.99, 0.11 %;
  strip built by `skin::mid_surface` (element size 5) 0.64 % on the first frequency.

## Amendment (2026-11-08): topology optimisation (generative design, phase 1)

`sim.optimize` finds the stiffest layout that keeps a share (`keep`, 0.05..0.95) of a solid
body: SIMP densities per voxel of the static model (penalty 3, lowest density 1e-3), minimum
compliance under the given supports and loads, optimality-criteria updates (move limit 0.2,
bisection on the volume multiplier) with a sensitivity filter of radius 1.5 voxels (Sigmund's
99-line method in 3D on our hexahedra). Voxels touching a held or loaded node stay solid. Each
iteration is one solve: the multigrid preconditioner takes the densities as element scales,
and the CG starts from the last displacement. The whole run may do eight analyses' work.
The result is the kept voxels (density >= 0.5): their share and volume, the compliance of
the first and last iteration, and their outer faces (`shape_quads`, shown in place of the
model in the app); `path` writes them as a binary STL. Steel is assumed (stiffness alone sets
the layout; the material only scales compliance). Not yet: manufacturing constraints
(draw direction, minimum member size beyond the filter), stress constraints, a
smoothed surface or a B-rep of the result, sheet bodies. Symmetry (2026-11-08): `symmetry:
["x", "y", "z"]` makes the layout its own mirror image about the voxel grid's middle across
those axes (each density and its mirror images share their mean after every update).
