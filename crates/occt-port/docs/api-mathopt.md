# `mathopt`: Rust API contract

Module `occt_port::mathopt` translates the header-only package `MathOpt` of OCCT 8.0.1 (`cpp/MathOpt/`: Brent, Powell,
FRPR, BFGS, Newton, Uzawa, PSO, GlobOpt). It reuses `mathutils` (Config, NDimConfig, results, callback traits, line
searches, numerical derivatives, RandomGenerator), `mathlin::{Vector, Matrix, MathError}` and `mathlin_pkg::{solve,
invert_crout}`. No existing API changes. C++ default arguments are listed as `[default]`; Rust passes every argument.
Vector functions return `Result<_, MathError>` (`Err` exactly where C++ throws a range error); scalar ones return the
result directly.

## Callbacks
`ScalarFunction`, `VectorFunction`, `VectorFunctionWithGradient` come from `mathutils`. New:
`trait VectorFunctionWithHessian: VectorFunctionWithGradient { fn hessian(&mut self, x: &Vector, h: &mut Matrix) -> bool; }`
(`h` is n x n with the start vector's lower bound in rows and columns).

## Functions
```rust
pub fn brent<F: ScalarFunction + ?Sized>(f, lower, upper, cfg: &Config) -> ScalarResult;
pub fn golden<F: ScalarFunction + ?Sized>(f, lower, upper, cfg: &Config) -> ScalarResult;
pub fn brent_with_bracket<F: ScalarFunction + ?Sized>(f, guess, step /*[1.0]*/, cfg) -> ScalarResult; // default MinBracketOptions
pub fn powell<F: VectorFunction + ?Sized>(f, start: &Vector, cfg: &Config) -> R<VectorResult>;
pub fn powell_with_directions(f, start, dirs: &Matrix, cfg) -> R<VectorResult>;   // dirs not n x n: InvalidInput
pub fn frpr<F: VectorFunctionWithGradient>(f, start, cfg: &FrprConfig) -> R<VectorResult>;
pub fn frpr_numerical<F: VectorFunction>(f, start, grad_step /*[1e-8]*/, cfg: &FrprConfig) -> R<VectorResult>;
pub fn bfgs<F: VectorFunctionWithGradient>(f, start, cfg: &Config) -> R<VectorResult>;
pub fn bfgs_numerical<F: VectorFunction>(f, start, grad_step /*[1e-8]*/, cfg) -> R<VectorResult>;
pub fn lbfgs<F: VectorFunctionWithGradient>(f, start, memory_size /*[10]*/, cfg) -> R<VectorResult>;
pub fn bfgs_bounded<F: VectorFunctionWithGradient>(f, start, lo: &Vector, hi: &Vector, cfg) -> R<VectorResult>;
pub fn newton<F: VectorFunctionWithHessian>(f, start, cfg: &NewtonConfig) -> R<VectorResult>;
pub fn newton_modified(..) // = newton
pub fn newton_numerical_hessian<F: VectorFunctionWithGradient>(f, start, hess_step /*[1e-6]*/, cfg) -> R<VectorResult>;
pub fn newton_numerical<F: VectorFunction>(f, start, grad_step /*[1e-8]*/, hess_step /*[1e-6]*/, cfg) -> R<VectorResult>;
pub fn newton_bounded<F: VectorFunctionWithHessian>(f, start, lo, hi, cfg: &NewtonConfig) -> R<VectorResult>;
pub fn uzawa(cont: &Matrix, secont: &Vector, start: &Vector, nce: i32, nci: i32, cfg: &UzawaConfig) -> R<UzawaResult>;
pub fn uzawa_equality(cont, secont, start, cfg) -> R<UzawaResult>;
pub fn polish_coordinate_wise<F: VectorFunction>(f, pos: &mut Vector, value: &mut f64, lo, hi, tol, max_evals: i32, eval_count: &mut i32) -> R<()>;
pub fn pso<F: VectorFunction>(f, lo, hi, cfg: &PsoConfig, seeds: Option<&[PsoSeedParticle]>, stats: Option<&mut PsoStats>) -> R<VectorResult>;
pub fn pso_simple(f, lo, hi, cfg) -> R<VectorResult>;                     // the overload without seeds/stats
pub fn differential_evolution<F: VectorFunction>(f, lo, hi, cfg: &GlobalConfig) -> R<VectorResult>;
pub fn multi_start<F: VectorFunction>(f, lo, hi, cfg: &GlobalConfig) -> R<VectorResult>;
pub fn global_minimum(f, lo, hi, cfg) -> R<VectorResult>;
pub fn global_minimum_with(f, lo, hi, cfg, pso_config: Option<&PsoConfig>, seeds: Option<&[PsoSeedParticle]>, stats: Option<&mut PsoStats>) -> R<VectorResult>;
```
## Types
`ConjugateGradientFormula {FletcherReeves=0, PolakRibiere=1, HestenesStiefel=2, DaiYuan=3}`; `FrprConfig {base: Config,
formula [PolakRibiere], restart_interval [0]}` + `new(tol, max_iter)`; `NewtonConfig {base, regularization [1e-8],
use_line_search [true]}` + `new`; `UzawaConfig {eps_lix, eps_lic [1e-6], max_iterations [500]}`; `UzawaResult {status
[NotConverged], solution, dual, error, initial_error: Option<Vector>, inverse_ctc: Option<Matrix>, nb_iterations: i32}`
+ `is_done`; enums `PsoInitMode`, `PsoBoundaryMode`, `PsoInertiaSchedule`, `GlobalStrategy {Pso, MultiStart, PsoHybrid,
DifferentialEvolution}` (C++ order, `as i32`); `PsoSeedParticle {position, value, velocity}` + `new`, `with_value`;
`PsoStats` (Default: counters 0, bests `f64::MAX`); `PsoConfig {base: NDimConfig, nb_particles [40], omega [0.7],
phi_personal, phi_global [1.5], velocity_clamp [0.5], seed: u32 [6], init_mode, boundary_mode, inertia_schedule, omega_min
[0.4], min_iterations, target_value: Option<f64>, no_improve_tol, no_improve_iters [10], restart_fraction, max_restarts,
polish_budget_per_dim [50]}` (Default base `NDimConfig::new(1e-8, 100, true)`) + `new(nb_particles, max_iter, tol)`;
`GlobalConfig {base [new(1e-8, 200, true)], strategy [PsoHybrid], nb_population [40], nb_starts [10], mutation_scale
[0.8], crossover_prob [0.9], seed [6], polish_budget_per_dim [50]}` + `new(strategy, max_iter)`.

## Behaviour notes (OCCT quirks reproduced)
- Bounded variants index both bound vectors with `lo.lower() + (i - lower)`: an upper-bound vector with another lower
  bound gives `Err` (C++ throws).
- `newton_numerical_hessian` uses `NumericalHessian`, which indexes from 1: a start vector with another lower bound gives `Err`.
- `lbfgs` with `memory_size <= 0`, and `multi_start` with `nb_starts == 0`, divide by zero in C++ (process dies, not
  judged); Rust returns `Err(Construction)` / `Err(Argument)`.
- `pso` is undefined in C++ if every initial evaluation fails (uninitialised global best); not judged.
- All results are bit-identical to C++ (only + - * / sqrt, fmod, ceil are used).

## Judge
`oracle/mathopt.cpp` prints 590 lines over 26 families (`build-oracle.sh`); callbacks are a closed set (a scalar quartic
with a failure interval; a vector function with analytic gradient/Hessian, failure thresholds/window and Hessian modes
exact/zero/negated), counted and hashed. Run deterministic twice, with `SMASH` stack fill, `MALLOC_PERTURB_` and ASAN/UBSAN
with identical output. 9 lines are `throws`.
