# `advapprox`: Rust API contract

Module `occt_port::advapprox` translates the package `AdvApprox` of OCCT 8.0.1: `AdvApprox_EvaluatorFunction`, `AdvApprox_Cutting` with
`AdvApprox_DichoCutting`, `AdvApprox_PrefCutting` and `AdvApprox_PrefAndRec`, `AdvApprox_SimpleApprox` and `AdvApprox_ApproxAFunction`. It uses
`plib` (`JacobiPolynomial`, `jacobi_parameters`, `eval_polynomial`, `hermite_interpolate`, `Array2`, `GeomAbsShape`), `convert::CompPolynomialToPoles`,
`bsplclib` (`NbPoles`), `gp` (`Pnt`, `Pnt2d`) and `poly::fmt` (the `std::ostream` double format of the `Dump` methods). No existing module changes.

## API
```rust
pub enum AdvApproxError { Construction, OutOfRange, Domain, NullObject, PLib(PLibError), Convert(ConvertError), BSpl(BSplError) }
pub type Result<T> = std::result::Result<T, AdvApproxError>;

/// AdvApprox_EvaluatorFunction::Evaluate (and operator()): the values of the derivative `derivative_request` at `parameter` go to `result`
/// (`result.len() == dimension`); the return value is `*ErrorCode`.
pub trait EvaluatorFunction { fn evaluate(&mut self, dimension: i32, start_end: [f64; 2], parameter: f64, derivative_request: i32, result: &mut [f64]) -> i32; }

/// AdvApprox_Cutting::Value: (the answer, `cuttingvalue`).
pub trait Cutting { fn value(&self, a: f64, b: f64) -> (bool, f64); }
pub struct DichoCutting;                                         // AdvApprox_DichoCutting  (Default)
pub struct PrefCutting; PrefCutting::new(cut_points: &[f64])     // AdvApprox_PrefCutting
pub struct PrefAndRec;  PrefAndRec::new(rec_cut: &[f64], pref_cut: &[f64], weight: f64) -> Result<Self>   // Err(Domain) for weight <= 1

pub struct SimpleApprox<'a>;                                      // AdvApprox_SimpleApprox
SimpleApprox::new(total_dimension, total_num_ss, continuity: GeomAbsShape, work_degree, nb_gauss_points, jacobi: &JacobiPolynomial,
                  func: &'a mut dyn EvaluatorFunction) -> Result<Self>      // Err(Construction) for a continuity other than C0/C1/C2 or invalid Gauss points
SimpleApprox::perform(&mut self, local_dimension: &[i32], local_tolerances: &[f64], first, last, max_degree: i32) -> Result<()>
is_done, degree, coefficients() -> &[f64], first_constr() / last_constr() -> &Array2<f64> (row i - 1, column k = C++ (i, k)), som_tab() / dif_tab() -> &[f64],
max_error(index) / average_error(index) -> Result<f64> (index from 1), dump() -> String

pub struct ApproxAFunction;                                        // AdvApprox_ApproxAFunction
ApproxAFunction::new(num_1dss, num_2dss, num_3dss, one_d_tol: &[f64], two_d_tol: &[f64], three_d_tol: &[f64], first, last, continuity, max_deg, max_seg,
                     func: &mut dyn EvaluatorFunction) -> Result<Self>                    // DichoCutting
ApproxAFunction::with_cutting(.., func, cut_tool: &dyn Cutting) -> Result<Self>
ApproxAFunction::approximation(total_dimension, total_num_ss, local_dimension: &[i32], first, last, evaluator: &mut dyn EvaluatorFunction, cut_tool: &dyn Cutting,
                               continuity_order, num_max_coeffs, max_segments, local_tolerances: &[f64], code_precis) -> Result<ApproximationResult>
pub struct ApproximationResult { num_curves: i32, num_coeff_per_curve: Vec<i32>, coefficients: Vec<f64>, intervals: Vec<f64>, error_max: Vec<f64>, average_error: Vec<f64>, error_code: i32 }
is_done, has_result, poles_1d() -> Option<&Array2<f64>>, poles_2d() -> Option<&Array2<Pnt2d>>, poles() -> Option<&Array2<Pnt>>,
nb_poles() -> Result<i32>, poles_1d_at(index, lower, upper) -> Result<Vec<f64>> (and poles_2d_at, poles_at), degree(), nb_knots() -> Result<i32>,
num_sub_spaces(dimension) -> Result<i32> (Err(Construction) outside 1..=3), knots() -> Option<&[f64]>, multiplicities() -> Option<&[i32]>,
max_error_array(d) / average_error_array(d) -> Result<Option<&[f64]>> (Err(OutOfRange) outside 1..=3; None for an empty subspace group),
max_error(d, index) / average_error(d, index) -> Result<f64> (index from 1; a missing array is Err(NullObject)), dump() -> Result<String> (Err(NullObject) when a subspace group has no result)
```

## Behaviour notes
- `SimpleApprox` and `ApproxAFunction` keep the C++ handle semantics by value: the evaluator is a `&mut dyn EvaluatorFunction` borrowed for the life of the `SimpleApprox`
  (C++ stores `void*`), and the approximator copies what it needs during construction.
- `approximation` allocates the output arrays that the C++ caller passes in, all zero-filled: `num_coeff_per_curve` (`max_segments.max(1)` items), `coefficients`
  (`max_segments.max(1) * total_dimension * num_max_coeffs`), `intervals` (`max_segments.max(1) + 1`), `error_max` and `average_error` (`max_segments.max(1) * total_num_ss`).
  `num_curves` is 0 when the function returns early with `error_code` 1 (the value the caller passed in). Index `k` of an array here is the C++ index `k + 1`.
  `error_code` is 0 or 1; `-1` is never produced by OCCT either (the assignment is commented out in the C++ source), so `ApproxAFunction::is_done` is true whenever `has_result` is.
- `ContinuityOrder` other than 0, 1, 2 (and `PLib::JacobiParameters` failures: `num_max_coeffs - 1 < 2 * order + 1`, a work degree too large, a bad `code_precis`) are `Err(Construction)`.
- The library is built without `Raise_if` checks and `NCollection_Array1` has no range checks, so a few out-of-range accesses of the C++ are undefined; here they are `Err(OutOfRange)`
  (a tolerance array shorter than its subspace count; `SimpleApprox` with a Gauss point count that is not larger than the work degree; `poles_*_at` bounds outside the pole array).
  The judge stays away from them.
- `SimpleApprox::som_tab()/dif_tab()` row 0 (the first `total_dimension` items) is uninitialised in C++ for an even number of Gauss points; here it is zero.
  `ApproxAFunction::degree()` is 0 until a result exists (uninitialised in C++).
- `ApproxAFunction::max_error_array(d)` is `Ok(None)` for a subspace group with no members (the C++ returns a null handle; `max_error(d, i)` on it is a null dereference there and `Err(NullObject)` here).
- `PrepareConvert`'s `std::pow` goes through `libm::pow` (bit-identical to the glibc `pow` on every judge record).
- `dump()` reproduces the `std::ostream` default float format (6 significant digits) and the exact text of the C++ (`"Dump of SimpleApprox \n"` has a trailing space).

## Judge
`oracle/advapprox.cpp` (616 lines, 23 families). The function to approximate is a test evaluator of up to 8 scalar components (polynomial, rational, sine, exponential, cubic hinge,
square root, jump) described by the records `def_func_exact` (exact arithmetic only) and `def_func_trig` (sin / cos / exp, computed as fixed series so that the values do not depend on the libm); it folds every call into a counter and a hash, so the
sequence of calls is compared as well as the results. Families: `cut_dicho, cut_pref, cut_prefrec`; `simple_exact, simple_trig, simple_err, simple_bad`; `afunc_dicho_exact, afunc_dicho_trig,
afunc_cut_exact, afunc_cut_trig, afunc_jump, afunc_err, afunc_bad, afunc_access, afunc_poles, afunc_dump`; `approximation_exact, approximation_trig, approximation_err, approximation_bad`.
Long pole / coefficient arrays of the exact families are compared through a hash of their bit patterns plus the first and last row; the `*_trig` families print everything.
Line coverage of the C++ sources by the oracle is 100 % (Dicho/Pref/PrefAndRec/SimpleApprox) and 99.5 % (ApproxAFunction; the two uncovered lines handle the never produced `ErrorCode == -1`).
Values are bit-identical except every family (no tolerance).
