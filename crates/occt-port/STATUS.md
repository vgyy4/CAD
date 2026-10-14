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
| gp | 20 k | subset done (ElCLib needs); 3D algebra and transforms: in progress |
| ElCLib | 2.4 k | 3D done (bit-identical / libm) |
| BSplCLib | 13 k | 3D curve evaluation done (bit-identical) |
| math | 25 k | linear algebra: in progress; solvers next |
| ElSLib, PLib, BSplSLib, CSLib, Convert, Bnd, BVH, Poly, TopLoc, Math* | 85 k | — |
