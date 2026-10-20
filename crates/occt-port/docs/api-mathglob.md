# `mathglob`: Rust API contract

This contract covers the global optimizers of OCCT 8.0.1's `math` package (`cpp/math/`): `math_GlobOptMin`, `math_PSO`,
`math_PSOParticlesPool` (with its struct `PSO_Particle`) and `math_BullardGenerator` (the random number generator of
`math_PSO`). It is the module `occt_port::mathglob` (add `pub mod mathglob;` to `src/lib.rs`; `src/` may use private
submodules). The tests are `tests/mathglob.rs` with the golden file `tests/mathglob.txt` printed by
`oracle/mathglob.cpp` (real OCCT 8.0.1).

Nothing that exists in `src/` changes its API. The module reuses `occt_port::mathlin` (`MathError`, `Vector`,
`Matrix`) and `occt_port::mathnd` (the traits `MultipleVarFunction`, `MultipleVarFunctionWithGradient`,
`MultipleVarFunctionWithHessian`; `NewtonMinimum`, `Bfgs`, `Powell` as the local optimizers), and must not copy them.

## Conventions

* Those of `docs/api-mathnd.md`: reals are `f64`, ints `i32`, booleans `bool`; `RealLast()` is `f64::MAX`,
  `Precision::Infinite()` is `2.0e100`, `Precision::PConfusion()` is `1.0e-7 * 0.01` (evaluated in `f64`),
  `Precision::Confusion()` is `1.0e-7`; `std::min(a, b)` is `if b < a { b } else { a }`, `std::max(a, b)` is
  `if a < b { b } else { a }`; `std::abs` is `f64::abs`; `sqrt` is `f64::sqrt`. All families are compared **bit for
  bit**: the operations must be done in the order of the C++, with no fused multiply-add and no reassociation.
  (`int(std::pow(3.0, double(myN)))` of the constructor is exact: compute `3^N` by integer multiplication.)
* Vectors are `mathlin::Vector` with OCCT's index ranges. `math_Vector::operator=` copies **by position** and keeps the
  bounds of its left side. The C++ reads the vectors a caller passes (`theLowerBorder(i)`, ...) at the absolute indices
  `1..=N`: such a vector must have `lower <= 1` and `upper >= N` (a longer vector, or one that starts below 1, is fine
  and the extra elements are ignored).
* Errors are `mathlin::MathError` (`Dimension` for a wrong vector, `Range` for a wrong index or count). The golden file
  prints `unchecked` where OCCT 8.0.1 as built has no check (it is built without the `Standard_*_Raise_if` checks of
  the library code) and the C++ result is unspecified or the call would read or write outside a container; the Rust API
  returns `Err` there. There is no `throws` line in the golden file: none of these classes raises.
* **No struct keeps a reference to a user function.** `math_GlobOptMin` stores `math_MultipleVarFunction*` in C++; in
  Rust the function is passed to every method that calls it (`new`, `set_global_params`, `perform`) as a
  `&mut GlobFunction`. The caller passes the same function to `perform` as to the last `new` / `set_global_params`
  (the golden file does; another function of another dimension is `Err(Dimension)`).
* The call trace is part of the behaviour: every construction line prints the number of calls of each callback and a hash
  of the whole call sequence (see `tests/mathglob.rs`), so the Rust optimizers must call the functions in the same order
  with the same arguments (same bits, same bounds) as OCCT, and the local optimizers must be the ones of `mathnd`, called
  as the C++ calls them.
* Nothing may panic (the lints forbid indexing and `unwrap`).

## What the function can do: `GlobFunction`

`math_GlobOptMin` takes a `math_MultipleVarFunction*` and asks it with `dynamic_cast` whether it is also a
`math_MultipleVarFunctionWithHessian` (then Newton may be used) or a `math_MultipleVarFunctionWithGradient` (then BFGS).
In Rust that is an enum over the three trait objects of `mathnd`:

```rust
pub enum GlobFunction<'a> {
    /// only a math_MultipleVarFunction (Powell only)
    Value(&'a mut dyn MultipleVarFunction),
    /// a math_MultipleVarFunctionWithGradient (BFGS, Powell)
    Gradient(&'a mut dyn MultipleVarFunctionWithGradient),
    /// a math_MultipleVarFunctionWithHessian (Newton, BFGS, Powell)
    Hessian(&'a mut dyn MultipleVarFunctionWithHessian),
}
impl GlobFunction<'_> {
    /// NbVariables of the function.
    pub fn nb_variables(&self) -> i32;
}
```

The golden file calls the same test function object through the three variants (the `level` word: 0 `Value`, 1
`Gradient`, 2 `Hessian`).

## `GlobOptMin` (`math_GlobOptMin`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `GlobOptMin::new` | `f: &mut GlobFunction, lower: &Vector, upper: &Vector, c: f64, discretization_tol: f64, same_tol: f64` | `Result<Self, MathError>` | `math_GlobOptMin(F, A, B, C = 9, DiscretizationTol = 1.0e-2, SameTol = 1.0e-7)` (Perform is not called) |
| `set_global_params` | `&mut self`, the same arguments | `Result<(), MathError>` | `SetGlobalParams` |
| `set_local_params` | `&mut self, a: &Vector, b: &Vector` | `Result<(), MathError>` | `SetLocalParams` |
| `set_tol` / `get_tol` | `(&mut self, discretization_tol: f64, same_tol: f64)` / `&self` | `()` / `(f64, f64)` | `SetTol` / `GetTol` |
| `perform` | `&mut self, f: &mut GlobFunction, find_single_solution: bool` | `Result<(), MathError>` | `Perform(isFindSingleSolution = false)` |
| `points` | `&self, index: i32, out: &mut Vector` | `Result<(), MathError>` | `Points(theIndex, theSol)` |
| `set_continuity` / `get_continuity` | `i32` | `()` / `i32` | `SetContinuity` / `GetContinuity` (default 2) |
| `set_functional_minimal_value` / `get_functional_minimal_value` | `f64` | `()` / `f64` | (default `-Precision::Infinite()`) |
| `set_lip_const_state` / `get_lip_const_state` | `bool` | `()` / `bool` | (default false) |
| `is_done`, `get_f`, `nb_extrema` | `&self` | `bool`, `f64`, `i32` | `isDone`, `GetF`, `NbExtrema` |

* `N` is `f.nb_variables()` of the constructor's function. `new` / `set_global_params` / `set_local_params` read their
  vectors at the absolute indices `1..=N` (see Conventions): `Err(Dimension)` when a vector does not cover them
  (`unchecked`). `perform` is `Err(Dimension)` when `f.nb_variables() != N`. `points(index, out)` writes `out(j)` for
  `j = 1..=N` (absolute indices, `out`'s bounds are unchanged and must cover `1..=N`, otherwise `Err(Dimension)`,
  `unchecked`) with the coordinates of solution `index`, `1 <= index <= nb_extrema()`, otherwise `Err(Range)`
  (`unchecked`: the C++ reads the sequence out of range). No accessor depends on `is_done`; all work right after `new`.
* **State that survives** (the golden file covers it with `glob_staged` and `glob_global`): `set_local_params` and
  `set_global_params` do not clear the container `myY` of solutions (a `Vec<f64>` of `N` values per solution);
  `set_global_params` resets only the solution count to 0 (not `myF`, not the container, not `myDone`'s
  `myIsFindSingleSolution`, not the continuity, functional minimal value or Lipschitz lock), so `points(i, ..)` after it
  reads the *first* entries of the old container. `myF` also survives (it starts at `Precision::Infinite()`). `myZ`
  is -1 after every `new`, `set_global_params` and `set_local_params`. `Perform` sets `myDone = false` first and
  returns (still not done) when `minLength < Precision::PConfusion()`; a degenerate or reversed box therefore gives
  `is_done() == false` and no callback call at all. `new` and `set_global_params` end with `myDone = false`.
* **Order of the C++ in `new`**: the members are initialised in declaration order, `myN = F.NbVariables()`, `myCont =
  2`, `myF = Precision::Infinite()`, `myIsConstLocked = false`, `myIsFindSingleSolution = false`,
  `myFunctionalMinimalValue = -Precision::Infinite()`, `myZ = -1`, `mySolCount = 0`; the borders are copied, `myMaxV(i) =
  (myB(i) - myA(i)) / 3.0`; `aSolNb = int(pow(3, N))`, `myMinCellFilterSol = max(2 * aSolNb, 200)`; `initCellSize()`;
  `ComputeInitSol()`. `ComputeInitSol` first evaluates the midpoint `(myGlobA + myGlobB) * 0.5` with `F.Value` (the
  flag is ignored: the value keeps its previous content, which is uninitialised in C++; the golden file has only
  cases in which it is reproducible) and calls `checkAddCandidate`, then `computeLocalExtremum` from the three points
  `myA + (myB - myA) * (i - 1) / 2.0`, `i = 1..=3` (compute the product with the `math_Vector * double` operators in that
  order: `(myB - myA) * (i - 1)` with `i - 1` converted to `f64`, then `/ 2.0`).
* **`computeLocalExtremum(thePnt, theVal, theOutPnt)` aliases its arguments in `ComputeInitSol`**: it is called with
  `theOutPnt` being the same `math_Vector` as `thePnt`. Each local optimizer writes its location into `theOutPnt` *before*
  `isInside` is tested, so a Newton result outside the global box leaves `thePnt` overwritten and the following BFGS and
  Powell start from that overwritten point. In `computeGlobalExtremum` the arguments are distinct (`myX`, `val`, `myTmp`).
  Reproduce this (the golden file has such cases). `theVal` is only assigned when a local optimizer reports `IsDone`.
* **The local optimizers** (in this order, each only when its condition holds; `NewtonMinimum`, `Bfgs`, `Powell` of
  `mathnd`, all created per call):
  1. `GlobFunction::Hessian` and `continuity >= 2`: `NewtonMinimum::new(f, 1.0e-7 /*Precision::Confusion()*/, 40, 1.0e-6,
     true)`, `set_boundary(myGlobA, myGlobB)`, `perform(f, thePnt)`; when `is_done()`: location and minimum are taken
     and, if inside the global box (`isInside`: `myGlobA(i) <= x(i) <= myGlobB(i)` for all `i`), return true.
  2. `Hessian` or `Gradient` and `continuity >= 1`: `Bfgs::new(N, 1.0e-8, 200, 1.0e-12)`, `set_boundary(myGlobA, myGlobB)`,
     `perform`; same handling.
  3. Always: `Powell::new(f, 1e-10, 200, 1.0e-12)` with the identity direction matrix (`1..=N`, `1..=N`), `perform(f,
     thePnt, m)`; same handling.
  4. return false.
  `mathnd`'s defaults apply where listed (the Newton convexity `1.0e-6`, `WithSingularity = true`, `ZEPS = 1.0e-12`).
  A `mathnd` construction error cannot occur (dimensions are right).
* `perform`, `computeInitialValues`, `computeGlobalExtremum` (the recursive scan over the dimensions), `isStored`,
  `checkAddCandidate`, `CheckFunctionalStopCriteria` are translated line by line. The cell filter of `isStored`
  (`NCollection_CellFilter`, used when the solution count is at least `myMinCellFilterSol`) answers `false` in every case
  and only adds points that are not near an existing one; as the filter's contents never change the answer, a port may
  skip the filter itself but must keep the effect (`isStored` returns `false`, so the candidate is appended) and the
  `isFirstCellFilterInvoke` bookkeeping is unobservable. The golden file has cases with thousands of solutions. All
  `myFunc->Value(x, d)` calls ignore the flag: `d` keeps its previous value when the callback returns `None`.
* The `Value` call of `computeInitialValues` is `myFunc->Value(myA, aPrevValDiag)` with an uninitialised
  `aPrevValDiag` when it fails; the golden file only has reproducible cases.

## `Pso` (`math_PSO`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `Pso::new` | `f: &dyn MultipleVarFunction, low: &Vector, upp: &Vector, steps: &Vector, nb_particles: i32, nb_iter: i32` | `Result<Self, MathError>` | `math_PSO(F, Low, Upp, Steps, NbParticles = 32, NbIter = 100)` |
| `perform` | `&mut self, f: &mut dyn MultipleVarFunction, steps: &Vector, out: &mut Vector, nb_iter: i32` | `Result<f64, MathError>` (the value) | `Perform(theSteps, theValue, theOutPnt, theNbIter = 100)` |
| `perform_with_pool` | `&mut self, f, pool: &mut PsoParticlesPool, nb_particles: i32, out: &mut Vector, nb_iter: i32` | `Result<f64, MathError>` | `Perform(theParticles, theNbParticles, theValue, theOutPnt, theNbIter = 100)` |

* `N = f.nb_variables()`. `new` is `Err(Dimension)` when `low`, `upp` or `steps` has a length other than `N` (`unchecked`;
  the C++ copies by position). `perform` / `perform_with_pool` are `Err(Dimension)` when `steps` (for `perform`) or `out`
  has a length other than `N`, or `f.nb_variables() != N` (`unchecked`); the vectors' lower bounds are free (copied by
  position, the golden file has lower bounds 0, -3, 5, ...). `out` keeps its bounds and receives the best position by
  position.
* `perform_with_pool` is `Err(Range)` when `nb_particles > pool.nb_particles()` (`unchecked`: the C++ writes out of the
  pool) and `Err(Dimension)` when `pool.nb_dimensions() != N`. `perform` is `Err(Range)` when the constructor's
  `nb_particles < 1` (the C++ crashes in `GetWorstParticle` of an empty pool; not in the golden file). `nb_particles = 0`
  in `perform_with_pool` is fine (the golden file has it). `nb_iter` 0 and 1 run no iteration.
* `perform` stores `nb_iter` and `steps` in the object (`myNbIter`, `mySteps`); `perform_with_pool` stores `nb_iter` and
  `nb_particles` (`myNbParticles`) and uses `mySteps` of the constructor or of the last `perform`. A `perform` first fills
  a fresh `PsoParticlesPool::new(myNbParticles, N)` by walking the regular grid (the C++ in `math_PSO::Perform`), then
  calls the pool version with the *constructor's* particle count.
* The function flag is ignored in the grid walk (`aCurrValue` keeps its previous value, uninitialised in C++ when the
  first call fails; the golden file only has reproducible cases) and in `performPSOWithGivenParticles`
  (`aParticle->Distance` keeps its value). A grid walk that never terminates (a zero or negative step in a coordinate
  other than the first, a step `<= 0` in the first: `max(mySteps(1), 1.0e-15)` bounds that one) is the caller's problem;
  the golden file has none (cases above 300000 callback calls are not printed).
* The random numbers are `BullardGenerator::new(1)` (default seed), `next_real()` called in the order of the C++ source:
  one per dimension per particle for the initial velocities, then two per particle per step (`aKsi1`, `aKsi2`), and one per
  particle in the restart branch.
* `GetBestParticle` / `GetWorstParticle` are `std::min_element` / `std::max_element` over `Distance` with `operator<`:
  **the first** minimal / maximal element on ties.

## `PsoParticlesPool` and `PsoParticle` (`math_PSOParticlesPool`, `PSO_Particle`)

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct PsoParticle {
    pub position: Vec<f64>,      // length = dimension
    pub velocity: Vec<f64>,
    pub best_position: Vec<f64>,
    pub distance: f64,
    pub best_distance: f64,
}
pub struct PsoParticlesPool { /* private */ }
impl PsoParticlesPool {
    /// (ParticlesCount, DimensionCount): every particle has zero vectors and `distance = best_distance = RealLast()`.
    /// `Err(Range)` for a count or dimension below 1 (not in the golden file: the C++ allocates an empty array).
    pub fn new(particles_count: i32, dimension_count: i32) -> Result<Self, MathError>;
    pub fn nb_particles(&self) -> i32;
    pub fn nb_dimensions(&self) -> i32;
    /// GetParticle(i), 1 <= i <= count; `Err(Range)` otherwise (`unchecked`: the C++ returns a pointer outside the array).
    pub fn get_particle(&self, index: i32) -> Result<&PsoParticle, MathError>;
    pub fn get_particle_mut(&mut self, index: i32) -> Result<&mut PsoParticle, MathError>;
    /// GetBestParticle: C++ returns the pointer; this returns its 1-based index (the first particle with the smallest `distance`).
    pub fn get_best_particle(&self) -> i32;
    /// GetWorstParticle: the 1-based index of the first particle with the largest `distance`.
    pub fn get_worst_particle(&self) -> i32;
}
```

The vectors of a particle are slices of one `Vec<f64>` in C++; a test only reads and writes their elements, so separate
`Vec`s are equivalent. The caller must not change their lengths.

## `BullardGenerator` (`math_BullardGenerator`)

```rust
#[derive(Clone, Debug)]
pub struct BullardGenerator { /* private: state_hi, state_lo: u32 */ }
impl BullardGenerator {
    pub fn new(seed: u32) -> Self;            // C++ default seed 1; also `impl Default` (seed 1)
    pub fn set_seed(&mut self, seed: u32);    // hi = seed, lo = seed ^ 0x49616E42
    pub fn next_int(&mut self) -> u32;        // hi = (hi >> 2) + (hi << 2); hi += lo; lo += hi; return hi  (wrapping u32)
    pub fn next_real(&mut self) -> f64;       // next_int() as f64 / 4294967295.0
}
```

The golden file stores seeds as signed `i32` (the bit pattern); read them back with `as u32`, and `next_int` results as
unsigned decimal.

## What the golden file contains

`tests/mathglob.txt` (see `FAMILIES`, `GOLDEN_LINES` and `MIN_DISTINCT` in `tests/mathglob.rs`): 10 families, every one
with inputs, no line repeated. `glob_*` families run the optimizers on the test functions of `oracle/mathglob.cpp`:
convex quadratics, Rosenbrock, sums of squares, trigonometric functions, double wells (4 or more equal minima),
periodic functions (several equal minima), a constant (every point a minimum; thousands of solutions, which uses the
cell filter), kinks, exponentials, functions without a minimum in the box, functions that fail (`None` / `false`) in a
box. Dimensions 1 to 4; the three function levels; continuity -1, 0, 1, 2, 3; Lipschitz constant 0.001 to 1000, locked
and not; discretization tolerance 1e-4 to 2; same-tolerance 0 to 0.1; boxes that contain the minimum, miss it, are
tiny, huge, degenerate (lower = upper), reversed or thinner than `Precision::PConfusion()`; both `find_single_solution`
values, with and without a reachable functional minimal value.

| family | Rust calls |
|---|---|
| `test_mv` | the judge's own test functions only |
| `bullard` | `BullardGenerator::new` / `set_seed` / `next_int` / `next_real` / `default` |
| `glob_new` | `GlobOptMin::new` and the getters (bad vector bounds are `unchecked`) |
| `glob_perform` | `new`, the setters, `set_tol`, `perform` |
| `glob_staged` | `new`, `perform`, `set_local_params`, `set_continuity`, `perform` |
| `glob_global` | `new`, `perform`, `set_global_params` with another function, `perform` |
| `glob_access` | `perform` (or not), `set_tol`, `set_continuity`, `points` with all indices and output vectors |
| `pso` | `Pso::new`, `perform` |
| `pso_pool` | `Pso::new`, `PsoParticlesPool::new` + `get_particle_mut`, `perform_with_pool` |
| `pso_pool_access` | the pool: `get_particle`, `get_best_particle`, `get_worst_particle` |

Cases are **not** in the golden file when they depend on uninitialised memory (the function fails at the first
evaluation of `ComputeInitSol`, `computeInitialValues` or the PSO grid), when a callback would see or return a
non-finite number, or when a run needs more than 300000 callback calls.

### Where the golden file says `unchecked`

All `unchecked` lines are cases in which OCCT 8.0.1's library has no range check: input vectors that do not cover `1..=N`
(`glob_new`, `glob_staged`, `glob_global`), `Points` with an index outside `1..=NbExtrema` or an output vector that does
not cover `1..=N` (`glob_access`), `math_PSO` with a vector of a length other than `N` (`pso`, `pso_pool`), `Perform` with more
particles than the pool has (`pso_pool`), `GetParticle` outside `1..=count` (`pso_pool_access`). The oracle runs the call
where that is harmless (a read) and does not run it where it would write outside a container (a short output vector, a
pool overrun): it only prints `unchecked` then. The probes behind this (no exception for these calls in the built
library) are in the job notes; the accessors never throw, so there is no `throws` line.

### Regenerating the golden file

```
mkdir -p oracle-bin && c++ -std=c++17 -O2 oracle/mathglob.cpp -I$OCCT/include/opencascade -L$OCCT/lib -lTKMath -lTKernel \
  -o oracle-bin/mathglob && LD_LIBRARY_PATH=$OCCT/lib oracle-bin/mathglob > tests/mathglob.txt
```

The output is deterministic (fixed xorshift seeds, one per family; 4 differently poisoned runs per case).
`MATHGLOB_SCHEMAS=1` prints every line's schema to stderr.
