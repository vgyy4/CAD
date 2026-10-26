# OCCT → Rust translation status (ADR 0025)

Every translated package is judged by an oracle: a C++ program that calls the real OCCT
8.0.1 and prints exact bit patterns (`oracle/`), checked by `tests/`. Bit-identical for
`+ - * /` and `sqrt`; at most 8 ulps of the line's largest value where OCCT calls the C
math library (the port uses `libm`, the same bits on every OS).

Pipeline per package: (1) Codex (GPT-6 Luna, high effort) writes the oracle, the Rust API
and the test, reviewed here; (2) a separate Codex run translates, without access to the
judge; (3) review, merge. Infrastructure packages are not translated literally: `Standard`
handles → `Arc`/ownership, `NCollection` → Rust collections, exceptions → `Result`.

Draft translator (2026-10-19): `tools/cxx2rs.py` turns clang's JSON AST of a `.cxx` file
into a Rust draft, function by function, keeping the C++ order of floating-point
operations (what bit-identity rests on) and marking what it cannot translate with
`todo!`. No tokens, seconds per file: ElCLib's 66 functions come out with no gaps;
math_FunctionRoots with 21. The translating agent starts from the draft and only fixes
the marked spots, overload names (from the API doc) and ownership.

Drafts of everything Gusset links (2026-10-24/26): the GitHub workflows "OCCT drafts" and
"OCCT header drafts" run `tools/draft-toolkit.sh` over 32 toolkits (no network, no models;
artifacts kept 90 days). First run: 4,744 `.cxx` and about 5,000 headers, 938 k draft lines,
55,941 functions, 59 % without a `todo!`. Then cxx2rs learned handle `->`, streams,
comparisons, strings, `new`, `sizeof` and `switch` (most of the 115 k gaps; `GeomAdaptor`
headers went from 199 to 6), and header units include the source-tree header with a stream
prelude, falling back to the package's `.cxx` (IGES headers no longer fail), with the
toolkit's own directories on the include path (glTF drafts now).
Measured on mathnd (12 files, about 4,000 C++ lines; drafts with 223 `todo!`s): three
Codex runs (about 0.7 M tokens) stopped early with the judge still failing (the general
`FunctionSetRoot`, and Powell/FRPR/BFGS with wrong update orders); a Sonnet agent then
finished it from their code in 15 minutes and about 0.27 M tokens. Drafts help with the
straight-line numeric code; solvers with state machines need a careful translator.
bnd (Bnd + CSLib, about 5,000 lines with the BVH parts the optimal OBB needs): Codex stopped after
11 minutes with only `Bnd_Range` done, as on every package since mathnd; the judge agent's
throwaway port had already passed every line. From here translations go to Sonnet agents, and
Codex keeps small mechanical edits.

## Toolkits needed by a CAD app (lines incl. headers, without OCCT's own tests)
| Toolkit | Packages | Lines | Status |
|---|---|---|---|
| TKernel | 19 | 132 k | mapped to Rust std (no literal translation) |
| TKMath | 22 | 138 k | in progress (below) |
| TKG2d | 7 | 23 k | Geom2d (+ Geom2dEval representations) done: 71 families, 1,278 oracle lines, 34,762 of 34,863 values bit-identical (worst 64 ulps); first class hierarchy (traits + `Arc<dyn Curve2d>`); 2D BSplCLib forms in `bsplclib::curve2d` |
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
| math | 25 k | linear algebra done (vectors, matrices, LU, Crout, Householder, SVD, Jacobi, eigen, polynomial roots, Gauss/Kronrod; 29,068 of 29,069 values bit-identical). One-variable solvers done: `FunctionSample`, `FunctionRoots`, `FunctionAllRoots`, `FunctionRoot` (on `FunctionSetRoot`), `BracketedRoot`, `NewtonFunctionRoot`, `BracketMinimum`, `BrentMinimum`, `TrigonometricFunctionRoots`, `TrigonometricEquationFunction`. 35 families, 5,220 oracle lines that also compare call counts and a hash of the callback call trace; 9,607 of 9,643 values bit-identical, worst 3 ulps (trigonometric families, libm). Mutation fuzzing (1 M runs) found no panic; `BracketMinimum`'s bracketing loop, unbounded in OCCT, which spins forever when the bracket stops moving (as real OCCT does on the same input), gives up after 100,000 steps. Root insertion stays O(n²) like OCCT's sequence. Multi-variable solvers next Several-variable solvers and integration done (mathnd): `FunctionSetRoot` (all constructors, bounded and unbounded `Perform`), `NewtonFunctionSetRoot`, `NewtonMinimum`, `BFGS`, `FRPR`, `Powell`, `Uzawa`, `BissecNewton`, Gauss single/multiple/set and Kronrod integration, with the callback traits. 56 families, 2,600 oracle lines with call counts and trace hashes; 8,959 of 8,975 values bit-identical, worst 1 ulp. Global optimizers done (mathglob): `GlobOptMin` (value, gradient and Hessian levels, Lipschitz state, single or all solutions), `PSO`, `PSOParticlesPool`, `BullardGenerator`; 10 families, 820 oracle lines, all 11,766 values bit-identical (judge and translation by Sonnet agents, 6 + 4 minutes). |
| PLib | 4 k | done: polynomial evaluation, Lagrange and Hermite interpolation, poles/coefficients, trimming, rational derivatives, `JacobiPolynomial`, `HermitJacobi`. 54 families, 3,140 oracle lines, all 37,372 values bit-identical. Note: `EvalPoly2Var`'s coefficient layout is `(i*(vdeg+1)+j)*dim`, not what `PLib.hxx` says |
| BSplSLib | 3.6 k | done: `D0`–`D3`, `DN`, homogeneous and rational derivatives, the cache (`BuildCache`, `CacheD0`–`D2`), iso curves, reverse, poles/weights, knot insertion and removal, degree elevation, unperiodizing, `Resolution`, `Interpolate`, `FunctionMultiply`, `MovePoint`. 1,295 oracle lines, all 21,964 values bit-identical. Checks OCCT 8.0.1 compiles out are `Err(OutOfRange)`; mutation fuzzing found three `int` overflows (fixed with wider arithmetic) and an unbounded allocation (capped at 2^24 elements) |
| Bnd, CSLib (+ the BVH parts the optimal OBB uses) | 9 k | done: Box, Box2d, Range, Sphere, OBB (plain and optimal), BoundSortBox, CSLib normals, Class2d, NormalPolyDef; 102 families, 3,532 oracle lines, all 17,180 values bit-identical (judge and translation by Sonnet agents after Codex stopped early) |
| TopLoc, Poly | 7 k | done: Location, Datum3D, ItemLocation, SList; Triangulation, Triangle, polygons, PolygonOnTriangulation, Catenate/PointOnTriangle/Intersect, the text Write/Read, Connect, MergeNodesTool, MakeLoops 3D/2D, CoherentTriangulation (with OCCT's packed map and integer heap); 53 families, 1,512 oracle lines, all 21,226 values bit-identical. The judge's text tokens first lost underscores and empty texts (found by the translating agent); they are escaped now |
| Convert | 4 k | done: circle, ellipse, hyperbola and parabola to B-splines with every parametrisation, cos/sin builders, cone/cylinder/sphere/torus surfaces, composite Bezier curves (3D and 2D), polynomial to poles (composite and grid), with `BSplCLib::Trimming` added; 28 families, 1,106 oracle lines, 40,640 of 41,614 values bit-identical, the rest within the 8-ulp line tolerance (libm) |
| BVH (rest), Math* | 61 k | — |
