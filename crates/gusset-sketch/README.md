# gusset-sketch

2D sketches for Gusset: geometry, a geometric constraint solver with
degree-of-freedom (DOF) analysis, and closed-profile extraction for features.

No dependencies besides `libm` (deterministic transcendental functions). All dense
linear algebra is implemented here (small, well-tested), and there is no `unsafe`.

## API

```rust
use gusset_sketch::{Constraint, Sketch, SolveOptions, solve, profiles};

let mut s = Sketch::new();
let l = s.add_line([0.0, 0.0], [9.0, 1.0])?;          // creates two points
let (a, _) = s.line_points(l).unwrap();
s.add_constraint(Constraint::Fixed(a))?;
s.add_constraint(Constraint::Horizontal(l))?;
s.add_constraint(Constraint::Distance { a: l, b: l, value: 10.0 })?;
let r = solve(&s, &SolveOptions::default());           // r.status, r.dof, r.sketch, ...
let regions = profiles(&r.sketch, 1e-6)?;              // closed regions only
```

* **Entities** (`Geom`): `Point`, `Line { a, b }`, `Circle { center, radius }`,
  `Arc { center, start, end }` (counter-clockwise; radius `|start − center|`).
  Lines, circles and arcs refer to point entities. Any entity can be marked
  construction (solved, but ignored by `profiles`).
* **Ids** (`EntityId`, `ConstraintId`) come from monotonic counters and are never
  reused, so they are stable names for persistent naming downstream.
* **Builder**: `add_point`, `add_line`, `add_line_between`, `add_circle(_at)`,
  `add_arc(_between)`, `add_constraint` (validated: kinds, finite values),
  `remove_constraint`, `remove_entity` (cascades), `set_point`, `set_radius`,
  `set_construction`, `validate`.
* **Constraints**: `Coincident`, `PointOnLine`, `PointOnCircle` (circle or arc),
  `Midpoint`, `Horizontal`, `Vertical`, `Parallel`, `Perpendicular`, `Tangent`
  (line–curve, curve–curve), `Equal` (line lengths or radii), `Distance`
  (point–point; point–line, keeping the starting side; `a == b` on a line = its
  length), `Angle` (directed, CCW from line `a` to line `b`, radians), `Radius`,
  `Diameter`, `Fixed` (all defining points, and a circle's radius), `Symmetric`
  (two points about a line).
* `solve(&Sketch, &SolveOptions) -> SolveResult` and `solve_drag(.., &[Drag])` for
  interactive dragging. `SolveResult` has `status` (`WellConstrained`,
  `UnderConstrained`, `OverConstrained`, `Failed`), `dof`, the solved `sketch`,
  `free_entities`, `redundant`, `conflicting`, `iterations`, `max_residual` and an
  `error` for failures.
* `profiles(&Sketch, tol) -> Result<Vec<Profile>, ProfileError>`: every bounded
  region as a CCW outer `Loop` plus CW hole loops. Each `Segment` names its source
  `entity`, whether it runs `reversed`, and its oriented `Curve` (line, arc or full
  circle). Errors name the entities involved: `Open { entities }`,
  `SelfIntersecting { a, b, at }`, `Degenerate { entity }`.

## Solver algorithm

1. **Unknowns**: `x, y` of every point and the radius of every circle. An arc's
   radius is derived, and each arc adds an internal equation
   `|start − c| = |end − c|`.
2. **Residuals**: one or more scalar equations per constraint. Lengths are in mm.
   Angles are in radians, and parallel/perpendicular use `sin`/`cos` of the angle,
   which match radians near the solution. Each equation has at most 8 local
   unknowns and is evaluated on forward-mode dual numbers. This gives exact
   (analytic) Jacobian rows without finite differences. Tests compare every
   constraint kind against finite differences.
3. **Branches**: discrete choices are fixed from the initial geometry so the solver
   cannot jump between solutions:
   * the side a point or tangent circle centre lies on relative to a line;
   * internal vs external circle tangency.

   Tangency between a line or arc and an arc that share an endpoint (the same
   point, or two points joined by `Coincident`) uses the first-order endpoint form:
   * line–arc: the radius is perpendicular to the line at the shared point;
   * arc–arc: the centres and the contact point are collinear.

   The distance form is singular there.
4. **Decomposition**: union-find over unknowns joined by equations. Every connected
   component is solved separately, in a deterministic order (smallest unknown
   first). A sketch of many independent shapes therefore costs a sum of small
   solves.
5. **Levenberg–Marquardt** per component: solve
   `(JᵀJ + λW) δ = −Jᵀr` by Cholesky, using Nielsen's λ update and a gain-ratio
   test.
   * With `W = I` and a tiny λ floor, steps tend to the minimum-norm Gauss–Newton
     step, so geometry moves as little as possible from where it was drawn.
   * `solve_drag` places dragged points at their targets and gives them weight
     `10⁴` in `W`, so the rest of the sketch moves to follow them.
   * Steps that would make a circle radius non-positive are rejected.
   * After convergence, up to 2 extra Newton steps polish the residual down to
     round-off.
6. **Rank analysis** at the final point: the Jacobian rows (an internal equation
   first, then constraints in id order) are added to an incremental orthonormal row
   basis. This is modified Gram–Schmidt with re-orthogonalisation, i.e. a QR
   factorisation of `Jᵀ`.
   * A row is **dependent** when its orthogonal remainder is ≤ `rank_tolerance`
     (default `1e-8`) times its norm.
   * `dof = unknowns − rank`.
   * An unknown is **free** when `1 − ‖projection of eᵢ onto the row space‖² >
     1e-9`, i.e. it has a component in the null space. An entity is free when any
     of its unknowns or defining points is free.
7. **Over-constraint**:
   * **Redundant**: the component converged, but a constraint contributed a
     dependent row. The later (higher-id) constraint is reported.
   * **Conflicting**: the component stopped at a least-squares stationary point
     (`‖Jᵀr‖∞ ≤ 1e-6·‖J‖·‖r‖`, or no descent possible) without converging. The
     constraints still unsatisfied there are reported (residual above the
     tolerance and above `1e-6` of the component's largest residual). This is the
     best-effort minimal set: the constraints that cannot all hold together. Their
     geometry is left at the least-squares compromise.
   * A component that neither converges nor reaches a stationary point gives
     `Failed` with `NotConverged`.

### Tolerances and defaults (`SolveOptions`)

| option | default | clamp | meaning |
| --- | --- | --- | --- |
| `tolerance` | `1e-10` | `[1e-14, 1e-3]` | max abs residual (mm / rad) for convergence |
| `max_iterations` | 100 | ≤ 10 000 | LM iterations per component |
| `rank_tolerance` | `1e-8` | `[1e-14, 1e-2]` | relative row-dependence threshold |
| `max_component_params` | 1000 | ≤ 4000 | largest component (dense `O(n²)` memory, `O(n³)` time) |

## Limits (hostile input)

* At most 50 000 entities and 100 000 constraints per sketch.
* Coordinates, radii and dimensions must be finite and at most `1e9` mm.
* Components above `max_component_params` unknowns are rejected with
  `ComponentTooLarge`.
* `profiles` accepts at most 10 000 curves, with `tol` in `[1e-9, 1]` mm.
* Every loop is bounded, so the solver cannot loop forever:
  * iterations: ≤ 10 000;
  * damping retries: 60 per iteration;
  * union-find and face tracing: bounded by the node and half-edge counts.
* Non-test code never panics. It uses no `unwrap`, `expect` or indexing (clippy
  enforces this). Degenerate geometry, such as zero-length lines or coincident
  centres, uses fixed substitute derivatives. Malformed input produces a
  structured error.

## Determinism

The solver is model-defining code and gives bit-identical output for the same
input:
* plain scalar IEEE arithmetic in a fixed order;
* no FMA or SIMD intrinsics, no threads;
* `libm` for `atan2`, `hypot`, `floor`, `round`, `sin` and `cos`;
* only `Vec` and `BTreeMap`/`BTreeSet`, never hash-ordered iteration.

A test solves the same 200-entity sketch twice and compares the bits.

## Profiles algorithm

1. Collect non-construction lines, arcs and circles.
2. Merge endpoints closer than `tol` into vertices (a grid lookup, first come
   first served).
3. Check for open ends (degree-1 vertices).
4. Reject any crossing, touching or overlap that is not at a shared vertex.
   Candidate pairs come from an x-sorted bounding-box sweep, and intersections are
   computed analytically for line–line, line–circle and circle–circle. Near a
   shared vertex the allowance is `tol + sqrt(2·r·tol)`, because near-tangent
   intersections scatter by that much.
5. Sort the half-edges around each vertex by quantised tangent angle, with
   curvature as the tie-break so tangent joins order correctly. Trace faces with
   `next(h) = previous-CCW(twin(h))`.
6. Per connected component, the most negative face is its outer boundary. Every
   positive face is a region, and a circle is its own component.
7. Each component's outer boundary becomes a hole of the smallest region of
   another component that contains it. The winding number is exact for arcs: a
   chord, plus 2π when the point lies in the circular segment.
8. Areas use Green's theorem with exact arc terms.
9. Loops start at their smallest `(entity, reversed)` segment, and profiles and
   holes are sorted by that key.

## Benchmarks

There are no `criterion` benches yet. criterion 0.8.2 is the genuine crate:
* repo `criterion-rs/criterion.rs`, about 300 M downloads, MIT/Apache-2.0;
* but it requires Rust 1.86, above the workspace MSRV of 1.85;
* and the RustSec advisory data for its dependency tree could not be checked from
  the development sandbox.

Instead, a test solves a 201-entity chain within a time budget: about 0.3 s in an
unoptimised debug build and about 20 ms in release.

## Known limitations

* Tangent branches are chosen from the initial geometry. A sketch drawn far from
  its intended configuration, for example a circle that starts outside the square
  it should be inscribed in, can be reported as conflicting.
* `Distance` between two lines (parallel offset) and point–circle distances are
  not supported yet. Use a construction point with `PointOnLine` /
  `PointOnCircle`.
* Endpoint tangency is detected only for the same point entity or a *direct*
  `Coincident` pair (not chains of coincidences).
* `profiles` does not split curves at crossings, as Fusion does. Crossings,
  T-junctions and circles touching other curves are errors.
* The linear algebra is dense per component, which is fine for up to about 1000
  unknowns per connected component. A sparse factorisation would be the next step
  for very large single components.
