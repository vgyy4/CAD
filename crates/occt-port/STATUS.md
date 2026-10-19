# OCCT → Rust translation status (ADR 0025)

Every translated package is judged by an oracle: a C++ program that calls the real OCCT
8.0.1 and prints exact bit patterns (`oracle/`), checked by `tests/`. Bit-identical for
`+ - * /` and `sqrt`; at most 8 ulps of the line's largest value where OCCT calls the C
math library (the port uses `libm`, the same bits on every OS).

Pipeline per package: (1) Codex (GPT-6 Luna, high effort) writes the oracle, the Rust API
and the test, reviewed here; (2) a separate Codex run translates, without access to the
judge; (3) review, merge. Infrastructure packages are not translated literally: `Standard`
handles → `Arc`/ownership, `NCollection` → Rust collections, exceptions → `Result`.

## Toolkits needed by a CAD app (lines incl. headers, without OCCT's own tests)
| Toolkit | Packages | Lines | Status |
|---|---|---|---|
| TKernel | 19 | 132 k | mapped to Rust std (no literal translation) |
| TKMath | 22 | 138 k | in progress (below) |
| TKG2d | 7 | 23 k | — |
| TKG3d | 9 | 70 k | — |
| TKGeomBase | 28 | 317 k | — |
| TKBRep | 11 | 90 k | — |
| TKGeomAlgo | 34 | 219 k | — |
| TKTopAlgo | 17 | 84 k | — |
| TKPrim | 6 | 13 k | — |
| TKBO | 6 | 71 k | — |
| TKBool | 8 | 153 k | — |
| TKFillet | 10 | 97 k | — |
| TKOffset | 5 | 46 k | — |
| TKMesh | 5 | 20 k | — |
| TKShHealing | 12 | 88 k | — |
| TKXSBase | 11 | 79 k | — |
| TKDESTEP | 43 | 342 k | — |

## TKMath packages
| Package | Lines | Status |
|---|---|---|
| gp | 20 k | 3D done: XYZ, Pnt, Vec, Dir, Ax1/2/3, Mat, Trsf, GTrsf, Quaternion (+ NLerp/SLerp, Euler sequences), 2,425 oracle lines, 44,850 of 45,889 values bit-identical, the rest within 8 ulps (libm). 2D done: XY, Pnt2d, Vec2d, Dir2d, Ax2d, Ax22d, Mat2d, Trsf2d, GTrsf2d, Lin2d and the 2D conics, 3,102 lines, 48,424 of 48,511 bit-identical, worst 9 ulps; OCCT's `gp_Mat2d::Multiply(*this)` aliasing (used by `Power`) is reproduced on purpose. 3D curves and elementary surfaces done: Lin, Circ, Elips, Hypr, Parab, Pln, Cylinder, Cone, Sphere, Torus, 1,088 lines, 55,180 of 55,272 bit-identical, worst 16 ulps (`Lin::distance(&Lin)` returns NaN where OCCT would throw on an underflowing cross product; no golden line reaches it). gp is complete |
| ElCLib | 2.4 k | done: 3D and 2D evaluation, parameters, every position-based overload and `To3d`. OCCT's hyperbola `DN` of a negative odd order is the zero vector (`IsOdd` is `n % 2 == 1`); the port follows it |
| ElSLib | 1.6 k | done: values, D0–D3, DN, parameters and iso curves of plane, cylinder, cone, sphere and torus. With ElCLib's rest: 83 families, 2,739 oracle lines, 39,913 of 40,681 values bit-identical, worst 271 ulps of the line's largest value (within 8 ε of it). `No_Exception` keeps negative radii in iso circles; the port reproduces them |
| BSplCLib | 13 k | 3D curve evaluation done (bit-identical); the flat-array functions BSplSLib needs: `Eval`/`Bohm` on buffers, knot location, basis matrix and banded solver, interpolation, knot insertion/removal, degree elevation, unperiodizing |
| math | 25 k | linear algebra done (vectors, matrices, LU, Crout, Householder, SVD, Jacobi, eigen, polynomial roots, Gauss/Kronrod; 29,068 of 29,069 values bit-identical). One-variable solvers done: `FunctionSample`, `FunctionRoots`, `FunctionAllRoots`, `FunctionRoot` (on a private port of `FunctionSetRoot`'s one-variable path), `BracketedRoot`, `NewtonFunctionRoot`, `BracketMinimum`, `BrentMinimum`, `TrigonometricFunctionRoots`, `TrigonometricEquationFunction`. 35 families, 5,220 oracle lines that also compare call counts and a hash of the callback call trace; 9,607 of 9,643 values bit-identical, worst 3 ulps (trigonometric families, libm). Mutation fuzzing (1 M runs) found no panic; `BracketMinimum`'s bracketing loop, unbounded in OCCT, which spins forever when the bracket stops moving (as real OCCT does on the same input), gives up after 100,000 steps. Root insertion stays O(n²) like OCCT's sequence. Multi-variable solvers next |
| PLib | 4 k | done: polynomial evaluation, Lagrange and Hermite interpolation, poles/coefficients, trimming, rational derivatives, `JacobiPolynomial`, `HermitJacobi`. 54 families, 3,140 oracle lines, all 37,372 values bit-identical. Note: `EvalPoly2Var`'s coefficient layout is `(i*(vdeg+1)+j)*dim`, not what `PLib.hxx` says |
| BSplSLib | 3.6 k | done: `D0`–`D3`, `DN`, homogeneous and rational derivatives, the cache (`BuildCache`, `CacheD0`–`D2`), iso curves, reverse, poles/weights, knot insertion and removal, degree elevation, unperiodizing, `Resolution`, `Interpolate`, `FunctionMultiply`, `MovePoint`. 1,295 oracle lines, all 21,964 values bit-identical. Checks OCCT 8.0.1 compiles out are `Err(OutOfRange)`; mutation fuzzing found three `int` overflows (fixed with wider arithmetic) and an unbounded allocation (capped at 2^24 elements) |
| CSLib, Convert, Bnd, BVH, Poly, TopLoc, Math* | 81 k | — |
