# `mathpoly`: Rust API contract

Module `occt_port::mathpoly` translates the header-only package `MathPoly` of OCCT 8.0.1 (`cpp/MathPoly/`: Quadratic,
Cubic, Quartic, Laguerre). It reuses `mathutils::{PolyResult, Status, refine_poly_root, sort_roots,
remove_duplicate_roots, cube_root, clamp, sign_transfer, ...}`; nothing existing changes. No function throws.

```rust
pub fn linear(a: f64, b: f64) -> PolyResult;                                 // Linear: a x + b
pub fn quadratic(a: f64, b: f64, c: f64) -> PolyResult;                      // Quadratic
pub fn cubic(a: f64, b: f64, c: f64, d: f64) -> PolyResult;                  // Cubic (acos, cos, cbrt: libm)
pub fn quartic(a: f64, b: f64, c: f64, d: f64, e: f64) -> PolyResult;        // Quartic
pub fn quintic(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> PolyResult;          // Quintic (Laguerre; Quartic if a = 0)
pub fn sextic(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64) -> PolyResult;   // Sextic (Laguerre, 4 roots kept)
pub const THE_MAX_POLY_DEGREE: usize = 20;
pub struct GeneralPolyResult { pub status: Status, pub roots: [f64; 20], pub complex_roots: [Complex; 20],
                               pub nb_roots: usize, pub nb_complex_roots: usize }   // Default (NotConverged, zeros) + is_done()
pub fn laguerre(coeffs: &[f64], degree: i32, tol: f64) -> GeneralPolyResult;  // Laguerre [tol 1e-12]; ascending coefficients
pub fn laguerre_n(coeffs: &[f64], tol: f64) -> GeneralPolyResult;             // LaguerreN [1e-12]: degree = coeffs.len() - 1
pub fn octic(coeffs: &[f64; 9]) -> GeneralPolyResult;                         // Octic: laguerre(coeffs, 8, 1e-12)
// the helpers of namespace detail (public in the header)
pub fn evaluate_polynomial_with_derivatives(coeffs: &[f64], degree: i32, x: Complex) -> (Complex, Complex, Complex); // (P, P', P'')
pub fn laguerre_iteration(coeffs: &[f64], degree: i32, x0: Complex, tol: f64, max_iter: i32) -> Complex;
pub fn deflate_real(coeffs: &mut [f64], degree: &mut i32, root: f64);          // degree decremented
pub fn deflate_complex(coeffs: &mut [f64], degree: &mut i32, root: Complex);   // degree >= 2; degree -= 2
pub fn refine_real_root(orig_coeffs: &[f64], orig_degree: i32, root: f64) -> f64;
```
`PolyResult` has `status`, `nb_roots`, `roots: [f64; 4]` (entries past `nb_roots` keep whatever C++ leaves: zeros, or stale
values after `remove_duplicate_roots`). `Complex { re, im }` is `std::complex<double>` with `new`, `abs`, `conj`, `sqrt`,
`scale(f64)` and `+ - * /`. It reproduces the arithmetic of the C++ build bit for bit: multiplication is the plain formula
(no FMA), division is libgcc 13's `__divdc3` (Baudin/Smith scaling), `abs` is glibc 2.39's non-FMA `hypot` kernel
(`libm::hypot` differs in 13% of the cases), `sqrt` is glibc's `csqrt`. (Validated on 20000 random pairs, 0 mismatches.)
`laguerre` with a degree outside `1..=20` or a leading coefficient with `|c| < 1e-15` gives `InvalidInput`;
`laguerre_n` with fewer than two coefficients gives `InvalidInput`.

## Judge

`oracle/mathpoly.cpp` -> `tests/mathpoly.txt` (`build-oracle.sh`), `tests/mathpoly.rs`: 18 families of 16 lines. Coefficients are
built from dyadic roots / conjugate pairs (and random dyadic coefficients). Everything is compared bit for bit except
`cubic`, `cubic_edge`, `quartic*` and `quintic` with a zero leading coefficient (which call `acos`/`cos`/`cbrt`: 8 ulps of the
largest value on the line). A `GeneralPolyResult` is printed completely (status, counts, all 20 real and all 20 complex slots).
The oracle ran with random stack/heap fill and under AddressSanitizer and printed identical output.
