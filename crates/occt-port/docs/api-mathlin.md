# Rust API contract: `math` linear algebra subset

The new module is `occt_port::mathlin`; existing `gp`, `elclib`, and `bsplclib` modules and their APIs remain unchanged. `math::...` C++ classes map to the names below. Methods use `snake_case`; constructors are `new`. Indices and bounds are signed `i32` and preserve the OCCT lower and upper bounds exactly, including non-one lower bounds. Real scalars are `f64`, integer values are `i32`, and booleans are `bool`.

## Errors

Export `MathError` (derive `Debug, Clone, PartialEq, Eq`) with variants `Construction`, `Dimension`, `Range`, `Domain`, `Singular`, `NotSquare`, `NotDone`, `NoConvergence`, `InfiniteSolutions`, and `Argument`. Any OCCT exception becomes `Result<T, MathError>`. Methods whose OCCT counterpart cannot throw return their value directly. Mutating fallible operations return `Result<(), MathError>`. A failure must not be turned into a successful default value.

## Containers

- `Vector {}` is the real `math_Vector`; `IntegerVector {}` is `math_IntegerVector`; both are owned value types.
- `Vector` and `Matrix` are cloneable owned value types. `Vector::new(lower: i32, upper: i32) -> Result<Self, MathError>`, `Vector::filled(lower, upper, value: f64) -> Result<Self, MathError>`, `Vector::from_slice(lower, values: &[f64]) -> Result<Self, MathError>`, `Vector::from_xy(x,y) -> Vector`, and `Vector::from_xyz(&gp::XYZ) -> Vector`. The latter two mirror `math_VectorBase(gp_XY)` and `math_VectorBase(gp_XYZ)`; `from_xy` takes coordinates because this `gp` subset has no public `XY` type. `IntegerVector` provides the corresponding constructors, including `from_xy`/`from_xyz`, converting coordinates to `i32` as C++ does. Construction requires `upper >= lower`.
- Getters: `length() -> i32`, `lower() -> i32`, `upper() -> i32`, `value(index: i32) -> Result<f64, MathError>` / `set_value(index, value) -> Result<(), MathError>`. `IntegerVector` uses `i32` for values and otherwise provides the corresponding generic `math_VectorBase<int>` operations. `init(value)`, `resize(size) -> Result<(), MathError>`, and `as_slice()` are provided (`&[f64]` for `Vector`, `&[i32]` for `IntegerVector`); `resize` preserves the lower bound and overlapping values. C++ `SetLower` is protected and is not exported.
- `norm() -> f64`, `norm2() -> f64`, `max_index() -> i32`, `min_index() -> i32`, `normalize() -> Result<(), MathError>`, `normalized() -> Result<Vector, MathError>`, `invert()`, `inverse() -> Vector`, `opposite() -> Vector`.
- In-place and copied arithmetic are both exposed: `multiply(f64)`, `multiplied(f64) -> Vector`, `divide(f64) -> Result<(), MathError>`, `divided(f64) -> Result<Vector, MathError>`, `add(&Vector)`, `added(&Vector) -> Result<Vector, MathError>`, `subtract(&Vector)`, `subtracted(&Vector) -> Result<Vector, MathError>`, `dot(&Vector) -> Result<f64, MathError>`, and `scale_from(f64, &Vector)`.
- `set_slice(first, last, &Vector)`, `slice(first, last) -> Result<Vector, MathError>`, `initialized(&Vector)`, and `add_from(&Vector, &Vector)` / `subtract_from(&Vector, &Vector)` preserve OCCT's length and range checks.
- `dump() -> String` formats vector diagnostics. `as_slice()` provides the values in increasing OCCT index order.
- `Matrix::new(lower_row, upper_row, lower_col, upper_col) -> Result<Matrix, MathError>`, `Matrix::filled(lower_row, upper_row, lower_col, upper_col, value: f64) -> Result<Matrix, MathError>`, and `Matrix::from_slice(lower_row, lower_col, row_count, col_count, values: &[f64]) -> Result<Matrix, MathError>` (row major). `DoubleTab` has the same constructors and accessors as a matrix, plus `is_deletable() -> bool`, `copy_to(&mut DoubleTab) -> Result<(), MathError>`, `set_lower_row`, and `set_lower_col`.
- Matrix getters: `row_number`, `col_number`, `lower_row`, `upper_row`, `lower_col`, `upper_col`; `value(row,col) -> Result<f64, MathError>` and `set_value(row,col,value) -> Result<(), MathError>`; `init`.
- Matrix arithmetic: `determinant() -> f64` (OCCT returns zero for a nonsquare matrix when its Gauss decomposition is not done), `transpose() -> Result<(), MathError>`, `transposed() -> Result<Matrix, MathError>`, `invert() -> Result<(), MathError>`, `inverse() -> Result<Matrix, MathError>`, scalar `multiply/multiplied/t_multiplied/divide/divided`, matrix `add/added/subtract/subtracted`, `multiply_matrix(&Matrix)`, `multiplied_matrix(&Matrix)`, `t_multiply(&Matrix)`, `multiply_vector(&Vector) -> Result<Vector, MathError>`, `multiply_from(&Matrix,&Matrix)`, `t_multiply_from(&Matrix,&Matrix)`, `outer_product_from(&Vector,&Vector)`, `add_from`, `subtract_from`, `set_slice`, `set_row`, `set_col`, `set_diag`, `row`, `col`, `swap_row`, `swap_col`, and `opposite`. `dump() -> String` formats matrix diagnostics. All dimension/range constraints match OCCT.
- `Matrix::outer_product(&Vector,&Vector) -> Result<Matrix, MathError>` is the Rust form of the output-parameter `Multiply(Left,Right)` overload.

## Quadrature and status

`gauss_points_max() -> i32`, `kronrod_points_max() -> i32`. `gauss_points(index: i32, lower: i32) -> Result<Vector, MathError>` retains the table's OCCT order; `gauss_weights(index, lower)` likewise. `ordered_gauss_points_and_weights(index, lower) -> Result<Option<(Vector,Vector)>, MathError>` returns `None` for the C++ false result. `kronrod_points_and_weights(index, lower)` has the same contract. `ComputeGaussPointsAndWeights::new(index) -> Self`, `is_done`, `points`, `weights`; equivalent `ComputeKronrodPointsAndWeights` methods. `MathStatus` has `Ok`, `TooManyIterations`, `FunctionError`, `DirectionSearchError`, `NotBracketed`; recipe status constants are `STATUS_USER_ABORTED=-1`, `STATUS_OK=0`, `STATUS_SINGULAR_MATRIX=1`, `STATUS_ARGUMENT_ERROR=2`, `STATUS_NO_CONVERGENCE=3`.

## Decomposition recipes

All functions are in `occt_port::mathlin::recipes`. In/out C++ references are returned in tuples, with the updated matrix/vector in the same order as its C++ parameter position; output-only values follow inputs.

- `lu_decompose(a: Matrix, tiny: f64) -> Result<(i32, Matrix, IntegerVector, f64), MathError>`; `lu_decompose_with_scratch(a, indx, sign, vv, tiny) -> Result<(i32, Matrix, IntegerVector, f64, Vector), MathError>`.
- `lu_solve(a: &Matrix, indx: &IntegerVector, b: Vector) -> Result<Vector, MathError>`; `lu_invert(a: Matrix) -> Result<(i32, Matrix), MathError>`.
- `svd_decompose(a: Matrix) -> Result<(i32, Matrix, Vector, Matrix), MathError>`; `svd_decompose_with_scratch(a, rv1)` similarly returns status, updated A, singular values, V, scratch. `svd_solve(u,w,v,b) -> Result<Vector, MathError>`.
- `dactcl_decompose(a, indx, min_pivot) -> Result<(i32, Vector), MathError>` and `dactcl_solve(a,b,indx,min_pivot) -> Result<(i32,Vector),MathError>`.
- `jacobi(a: Matrix) -> Result<JacobiResult, MathError>` returns `JacobiResult { status: i32, rotations: i32, matrix: Matrix, values: Vector, vectors: Matrix }`.

## Solver and decomposition objects

`Gauss::new(&Matrix, min_pivot) -> Result<Gauss, MathError>`; `is_done`, `solve(&Vector) -> Result<Vector,MathError>`, `solve_in_place(Vector) -> Result<Vector,MathError>`, `determinant()`, `inverse() -> Result<Matrix,MathError>`, `invert_into(&mut Matrix)`, and `dump() -> String`. `Crout::new(&Matrix,min_pivot) -> Result<Crout,MathError>`; `is_done`, `solve(&Vector)`, `determinant`, `inverse`, `invert_into`, and `dump`. `GaussLeastSquare::new(&Matrix,min_pivot)`, `is_done`, `solve`, and `dump`. `Householder::new_vector(&Matrix,&Vector,eps)`, `new_matrix(&Matrix,&Matrix,eps)`, and `new_matrix_with_range(&Matrix,&Matrix,lower_row,upper_row,lower_col,upper_col,eps)`; `is_done`, `value(index) -> Result<Vector,MathError>`, `all_values()`, and `dump() -> String`.

`Svd::new(&Matrix)`, `is_done`, `solve(&Vector,eps)`, `pseudo_inverse(eps)`, with all computed values returned as owned `Matrix`/`Vector` values. `Jacobi::new(&Matrix)`, `is_done`, `values`, `value(index)`, `vectors`, `vector(index)`. `EigenValuesSearcher::new(diagonal: &[f64], off_diagonal: &[f64], lower: i32)`, `is_done`, `dimension`, `eigen_value(index)`, `eigen_vector(index)`.

## Polynomial roots

`DirectPolynomialRoots::new_linear(a,b)`, `new_quadratic(a,b,c)`, `new_cubic(a,b,c,d)`, and `new_quartic(a,b,c,d,e)` each return `Self` (these numerical constructors report completion through getters). Getters: `is_done`, `infinite_roots`, `number_of_solutions`, and `value(index) -> Result<f64,MathError>`. `dump() -> String` formats diagnostics. Roots preserve OCCT's returned ordering.

## Additional container operations

The generic vector API also includes `t_multiplied(f64) -> Vector`, `multiplied_matrix(&Matrix) -> Result<Vector, MathError>`, `multiply_matrix_from(&Vector, &Matrix)`, `multiply_from_matrix(&Matrix, &Vector)`, `t_multiply_from(&Vector, &Matrix)`, `initialized(&Vector) -> Result<(), MathError>`, and `multiply_from(f64, &Vector)`. `Vector::slice(first,last)` preserves the full vector bounds and reverses the selected interval, matching OCCT. `set_slice` replaces a range; the range length must match the source vector length. `IntegerVector` supports the same generic operations that OCCT's `math_VectorBase<int>` exposes, with integer multiplication, division, addition, subtraction, copying, slicing, resizing, indexed access, and diagnostic `dump()`.

`Matrix` additionally provides `initialize_from(&Matrix)`, `set_slice(first_row,last_row,first_col,last_col,&Matrix)`, and the output forms `add_from(&Matrix,&Matrix)` and `subtract_from(&Matrix,&Matrix)`. `transpose()` mutates; `transposed()` returns a copy. `t_multiply(&Matrix)` returns `selfᵀ * right`, while `t_multiply_from(left,right)` writes `leftᵀ * right`. `multiply_from(left,right)` writes `left * right`, and `outer_product_from(left,right)` writes the outer product. `set_row`, `set_col`, `row`, `col`, `swap_row`, `swap_col`, and `set_diag` use the preserved OCCT bounds and return `Result` where OCCT can raise.

`DoubleTab` is the separately exposed storage type, with `new`, `init`, `copy_to`, `is_deletable`, bound/count getters, indexed `value`/`set_value`, and lower-bound updates. `Matrix` uses the same storage behavior internally but does not expose protected C++ `SetLower*` methods.

## Header-by-header coverage checklist

The checklist follows each in-scope class header under `cpp/math/`. `math_VectorBase` covers both `math_Vector` and `math_IntegerVector`; C++ operator aliases and protected/private implementation details are represented by their named Rust methods or intentionally not exported.

- [x] `math.hxx`: `GaussPointsMax`, `GaussPoints`, `GaussWeights`, `KronrodPointsMax`, `OrderedGaussPointsAndWeights`, `KronrodPointsAndWeights`.
- [x] `math_VectorBase.hxx` / `math_Vector.hxx`: constructors (including `gp_XY`/`gp_XYZ` conversions), bounds/count/value access, init, norm/norm², min/max, normalize/normalized, invert/inverse, set/slice, scalar multiply/divide and copied forms, add/subtract and copied forms, dot product, matrix/vector multiply overloads, T-multiply overloads, initialized, opposite, resize, and integer-vector equivalents.
- [x] `math_IntegerVector.hxx`: alias constructors and all generic integer vector operations; included in `integer_vector` oracle record.
- [x] `math_Matrix.hxx`: constructors, init, counts/bounds/value, determinant, transpose/transposed, inverse/invert, scalar and matrix arithmetic, add/subtract output overloads, set-slice, row/column/diagonal access, swaps, T-multiply, vector/matrix products, outer product, initialized, and opposite.
- [x] `math_DoubleTab.hxx`: construction, init, copy, deletable state, bound/count getters and setters, and value access.
- [x] `math_Recipes.hxx`: status constants; both LU decomposition overloads, LU solve/invert; both SVD decomposition overloads and solve; DACTCL decomposition/solve; Jacobi recipe.
- [x] `math_Gauss.hxx`: construction, done state, both solve forms, determinant, invert, dump.
- [x] `math_GaussLeastSquare.hxx`: construction, done state, solve, dump.
- [x] `math_Crout.hxx`: construction, done state, solve, both inverse forms, determinant, dump.
- [x] `math_Householder.hxx`: vector and matrix constructors, done state, indexed/all values, dump.
- [x] `math_SVD.hxx`: construction, done state, solve, pseudo-inverse, dump.
- [x] `math_Jacobi.hxx`: construction, done state, values, indexed value/vector, vectors matrix, dump.
- [x] `math_EigenValuesSearcher.hxx`: construction, done state, dimension, eigenvalue, eigenvector.
- [x] `math_DirectPolynomialRoots.hxx`: linear, quadratic, cubic, quartic constructors; done/infinite/count/value/dump.
- [x] `math_ComputeGaussPointsAndWeights.hxx` / `math_ComputeKronrodPointsAndWeights.hxx`: construction, done state, points and weights.
- [x] `math_Status.hxx`: all five status enum values.

Oracle output is generated with the fixed xorshift seed and contains 817 unique records (623,817 bytes). Every record family has at least 20 distinct cases; the dump and status families have 30 and 21 cases, and both Gauss table families retain 63 cases each. The randomized vector and matrix arithmetic families span dimensions 1–8 (including rectangular matrices and negative lower bounds), and decomposition families use dimensions 2–8. Error records cover zero division, incompatible vector dimensions, out-of-range vector access, nonsquare determinant behavior, and singular inversion. The largest computed quadrature case also covers the computed (non-table) path.
