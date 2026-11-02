# `geom2dhash`: Rust API contract

Module `occt_port::geom2dhash` translates the package `Geom2dHash` of OCCT 8.0.1: the hashers and equality functors of the 2D geometry
(`Geom2dHash_PointHasher`, `_DirectionHasher`, `_AxisPlacement`, `_LineHasher`, `_CircleHasher`, `_EllipseHasher`, `_HyperbolaHasher`, `_ParabolaHasher`, `_BezierCurveHasher`,
`_BSplineCurveHasher`, `_TrimmedCurveHasher`, `_OffsetCurveHasher`, `_CurveHasher`), together with the pieces of the `Standard` package they use (`Standard_HashUtils`:
`MurmurHash64A`, `FNVHash64A`, `opencascade::hash`, `hashBytes`; `Standard_CStringHasher`), which the module contains as free functions.

## API
```rust
pub fn murmur_hash64a(key: &[u8], seed: u64) -> u64        // opencascade::MurmurHash::MurmurHash64A
pub fn fnv_hash64a(key: &[u8], seed: u64) -> u64            // opencascade::FNVHash::FNVHash64A
pub fn hash_i32(v: i32) -> u64, hash_i64(v: i64) -> u64     // opencascade::hash(int), opencascade::hash(int64_t): MurmurHash64A of the bytes, default seed
pub fn hash_bytes(words: &[u64]) -> u64                     // opencascade::hashBytes(array of size_t, sizeof): the same over the (little endian) bytes
pub fn cstring_hash(s: &[u8]) -> u64                        // Standard_CStringHasher{}(s): FNV for fewer than 4 characters, else Murmur (s without the terminating NUL)
// every hasher: Default (the tolerances Precision::Angular(), Precision::Confusion()), new(comp_tolerance, hash_tolerance), pub fields of those names, and
PointHasher::hash(&Pnt2d) -> u64 / equal(&Pnt2d, &Pnt2d) -> bool                    // DirectionHasher: Dir2d; AxisPlacement: Ax22d
LineHasher, CircleHasher, EllipseHasher, HyperbolaHasher, ParabolaHasher, BezierCurveHasher, BSplineCurveHasher, TrimmedCurveHasher, OffsetCurveHasher
    ::hash(&geom2d::Line ...) / equal(&.., &..)                                      // the class of the hasher
CurveHasher::hash(Option<&Arc<dyn Curve2d>>) -> u64 / equal(Option<&Arc<..>>, Option<&Arc<..>>) -> bool   // None is the null handle
```
`std::size_t` is `u64`. The handles are references to the (non-null) class; the null handle exists only for `CurveHasher`.

## Behaviour notes
- Every hash is `MurmurHash64A` over the bytes of an array of 64-bit hashes (`hashBytes`), whose elements are `hash_i64(round(x / hash_tolerance))` (`std::round`, half away from zero, converted to `int64_t`; `libm::round`
  and `as i64`, which saturates where C++ is undefined) or `hash_i32` of an integer (degree, number of poles, number of knots, rationality as `int`). The B-spline and Bezier hashers hash the metadata only.
- Equality compares with `comp_tolerance` (`<=`), the B-spline and Bezier ones the knots, multiplicities, poles and weights of the first curve's counts. The functor is `noexcept` and reads the second curve without a
  range check: where the second curve has fewer poles than the first (same knots, multiplicities and degree but a different periodicity) C++ reads out of bounds; Rust answers `false`, and the judge avoids such pairs.
- `CurveHasher` dispatches on the exact dynamic type (`Line`, `Circle`, `Ellipse`, `Hyperbola`, `Parabola`, `BezierCurve`, `BSplineCurve`, `TrimmedCurve`, `OffsetCurve`); a null handle hashes to 0; any other curve (the
  `Geom2dEval` curves) is hashed by `cstring_hash` of the name of its dynamic type (`Geom2dEval_ArchimedeanSpiralCurve`, `_CircleInvoluteCurve`, `_LogarithmicSpiralCurve`, `_SineWaveCurve`, `_TBezierCurve`,
  `_AhtBezierCurve`; the Rust curves have no run-time type name, so they are recognised by `downcast_ref`) and compared by identity (`Arc::ptr_eq`). Equality: two nulls are equal, a null and a curve are not,
  identical handles are equal, different dynamic types are not.
- Trimmed and offset curves hash the basis curve through `CurveHasher` and the parameters / offset through `hash_i64`.

## Judge
`oracle/geom2dhash.cpp` (750 lines, 31 families: the hash functions `murmur, fnv, cstring, hash_int, hash_int64`, and `_hash` / `_eq` of `point, dir, axis, line, circle, ellipse, hyperbola, parabola, bezier, bspline, trimmed, offset`,
`curve_hash, curve_eq`) builds the curves of the real library from printed data (lines, conics, Bezier, B-splines, offset / trimmed / nested curves, two `Geom2dEval` curves) with six tolerance sets (the default constructor
included), compares hashes bit for bit and equality with copies shifted by 0, 0.3 and 3 comparison tolerances. All values are integers, so every comparison is exact.
