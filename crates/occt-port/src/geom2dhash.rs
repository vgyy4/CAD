//! OCCT's package `Geom2dHash`: the hashers and the equality functors of the 2D geometry (`Geom2dHash_PointHasher`,
//! `_DirectionHasher`, `_AxisPlacement`, `_LineHasher`, `_CircleHasher`, `_EllipseHasher`, `_HyperbolaHasher`,
//! `_ParabolaHasher`, `_BezierCurveHasher`, `_BSplineCurveHasher`, `_TrimmedCurveHasher`, `_OffsetCurveHasher`,
//! `_CurveHasher`), with the parts of the `Standard` package they use (`Standard_HashUtils`, `Standard_CStringHasher`).
//!
//! Translated from `Geom2dHash*.hxx/.pxx/.cxx` and `Standard_HashUtils.hxx/.lxx` (OCCT 8.0.1). `std::size_t` is `u64`.
// The comparisons `x <= tolerance` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use crate::geom2d::{
    BSplineCurve, BezierCurve, Circle, Conic2d, Curve2d, Ellipse, Hyperbola, Line, OffsetCurve, Parabola, TrimmedCurve,
    angular, confusion,
};
use crate::geom2deval::{
    AhtBezierCurve, ArchimedeanSpiralCurve, CircleInvoluteCurve, LogarithmicSpiralCurve, SineWaveCurve, TBezierCurve,
};
use crate::gp::{Ax22d, Dir2d, Pnt2d};
use std::any::Any;
use std::sync::Arc;

// ---------------------------------------------------------------------------------------------
// Standard_HashUtils, Standard_CStringHasher
// ---------------------------------------------------------------------------------------------

// `opencascade::MurmurHash::optimalSeed<uint64_t>()`
const MURMUR_SEED: u64 = 0xA329_F1D3_A586;
// `opencascade::FNVHash::optimalSeed<uint64_t>()`
const FNV_SEED: u64 = 14_695_981_039_346_656_037;

fn shift_mix(v: u64) -> u64 {
    v ^ (v >> 47)
}

/// `opencascade::MurmurHash::MurmurHash64A(theKey, theLen, theSeed)`.
pub fn murmur_hash64a(key: &[u8], seed: u64) -> u64 {
    const MUL: u64 = (0xc6a4_a793_u64 << 32) + 0x5bd1_e995;
    let mut hash = seed ^ (key.len() as u64).wrapping_mul(MUL);
    let mut chunks = key.chunks_exact(8);
    for chunk in &mut chunks {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(chunk);
        let data = shift_mix(u64::from_le_bytes(bytes).wrapping_mul(MUL)).wrapping_mul(MUL);
        hash ^= data;
        hash = hash.wrapping_mul(MUL);
    }
    let tail = chunks.remainder();
    if !tail.is_empty() {
        // `load_bytes`: the remaining 1..7 bytes, the first one the least significant
        let data = tail.iter().enumerate().fold(0u64, |acc, (i, &b)| acc | (u64::from(b) << (8 * i)));
        hash ^= data;
        hash = hash.wrapping_mul(MUL);
    }
    hash = shift_mix(hash).wrapping_mul(MUL);
    shift_mix(hash)
}

/// `opencascade::FNVHash::FNVHash64A(theKey, theLen, theSeed)`. The bytes are `char`s (signed): a byte of 128 or more is
/// sign-extended before it is mixed in.
pub fn fnv_hash64a(key: &[u8], seed: u64) -> u64 {
    let mut seed = seed;
    for &b in key {
        seed ^= i64::from(b as i8) as u64;
        seed = seed.wrapping_mul(1_099_511_628_211);
    }
    seed
}

/// `opencascade::hash(int)`: `MurmurHash64A` of the four bytes with the default seed.
pub fn hash_i32(v: i32) -> u64 {
    murmur_hash64a(&v.to_le_bytes(), MURMUR_SEED)
}

/// `opencascade::hash(int64_t)`: `MurmurHash64A` of the eight bytes with the default seed.
pub fn hash_i64(v: i64) -> u64 {
    murmur_hash64a(&v.to_le_bytes(), MURMUR_SEED)
}

/// `opencascade::hashBytes(array of std::size_t, sizeof)`: `MurmurHash64A` of the bytes of the array with the default seed.
pub fn hash_bytes(words: &[u64]) -> u64 {
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    murmur_hash64a(&bytes, MURMUR_SEED)
}

/// `Standard_CStringHasher{}(theString)` for the characters of the string (without the terminating NUL): FNV when there are
/// fewer than four of them, else `MurmurHash64A`.
pub fn cstring_hash(s: &[u8]) -> u64 {
    if s.len() < 4 { fnv_hash64a(s, FNV_SEED) } else { murmur_hash64a(s, MURMUR_SEED) }
}

// `static_cast<int64_t>(std::round(x * factor))` hashed
fn hash_rounded(x: f64, factor: f64) -> u64 {
    hash_i64(libm::round(x * factor) as i64)
}

// ---------------------------------------------------------------------------------------------
// Points, directions, axis placements
// ---------------------------------------------------------------------------------------------

macro_rules! tolerances {
    ($name:ident) => {
        impl Default for $name {
            fn default() -> Self {
                Self::new(angular(), confusion())
            }
        }
        impl $name {
            /// The hasher with the given comparison and hash tolerances.
            pub fn new(comp_tolerance: f64, hash_tolerance: f64) -> Self {
                Self { comp_tolerance, hash_tolerance }
            }
        }
    };
}

/// `Geom2dHash_PointHasher`.
#[derive(Debug, Clone, Copy)]
pub struct PointHasher {
    /// `CompTolerance`.
    pub comp_tolerance: f64,
    /// `HashTolerance`.
    pub hash_tolerance: f64,
}
tolerances!(PointHasher);

impl PointHasher {
    /// Hashes the point by its rounded coordinates.
    pub fn hash(&self, p: &Pnt2d) -> u64 {
        let factor = 1.0 / self.hash_tolerance;
        hash_bytes(&[hash_rounded(p.x(), factor), hash_rounded(p.y(), factor)])
    }
    /// Compares two points with the comparison tolerance.
    pub fn equal(&self, a: &Pnt2d, b: &Pnt2d) -> bool {
        (a.x() - b.x()).abs() <= self.comp_tolerance && (a.y() - b.y()).abs() <= self.comp_tolerance
    }
}

/// `Geom2dHash_DirectionHasher`.
#[derive(Debug, Clone, Copy)]
pub struct DirectionHasher {
    /// `CompTolerance`.
    pub comp_tolerance: f64,
    /// `HashTolerance`.
    pub hash_tolerance: f64,
}
tolerances!(DirectionHasher);

impl DirectionHasher {
    /// Hashes the direction by its rounded components.
    pub fn hash(&self, d: &Dir2d) -> u64 {
        let factor = 1.0 / self.hash_tolerance;
        hash_bytes(&[hash_rounded(d.x(), factor), hash_rounded(d.y(), factor)])
    }
    /// Compares two directions with the comparison tolerance.
    pub fn equal(&self, a: &Dir2d, b: &Dir2d) -> bool {
        (a.x() - b.x()).abs() <= self.comp_tolerance && (a.y() - b.y()).abs() <= self.comp_tolerance
    }
}

/// `Geom2dHash_AxisPlacement`.
#[derive(Debug, Clone, Copy)]
pub struct AxisPlacement {
    /// `CompTolerance`.
    pub comp_tolerance: f64,
    /// `HashTolerance`.
    pub hash_tolerance: f64,
}
tolerances!(AxisPlacement);

impl AxisPlacement {
    fn parts(&self) -> (PointHasher, DirectionHasher) {
        (
            PointHasher::new(self.comp_tolerance, self.hash_tolerance),
            DirectionHasher::new(self.comp_tolerance, self.hash_tolerance),
        )
    }
    /// Hashes the placement by its location and its two directions.
    pub fn hash(&self, a: &Ax22d) -> u64 {
        let (p, d) = self.parts();
        hash_bytes(&[p.hash(a.location()), d.hash(a.x_direction()), d.hash(a.y_direction())])
    }
    /// Compares two placements.
    pub fn equal(&self, a: &Ax22d, b: &Ax22d) -> bool {
        let (p, d) = self.parts();
        p.equal(a.location(), b.location())
            && d.equal(a.x_direction(), b.x_direction())
            && d.equal(a.y_direction(), b.y_direction())
    }
}

// ---------------------------------------------------------------------------------------------
// Lines and conics
// ---------------------------------------------------------------------------------------------

/// `Geom2dHash_LineHasher`.
#[derive(Debug, Clone, Copy)]
pub struct LineHasher {
    /// `CompTolerance`.
    pub comp_tolerance: f64,
    /// `HashTolerance`.
    pub hash_tolerance: f64,
}
tolerances!(LineHasher);

impl LineHasher {
    fn parts(&self) -> (PointHasher, DirectionHasher) {
        (
            PointHasher::new(self.comp_tolerance, self.hash_tolerance),
            DirectionHasher::new(self.comp_tolerance, self.hash_tolerance),
        )
    }
    /// Hashes the line by its location and direction.
    pub fn hash(&self, l: &Line) -> u64 {
        let (p, d) = self.parts();
        hash_bytes(&[p.hash(l.position().location()), d.hash(l.position().direction())])
    }
    /// Compares two lines by their positions.
    pub fn equal(&self, a: &Line, b: &Line) -> bool {
        let (p, d) = self.parts();
        p.equal(a.position().location(), b.position().location())
            && d.equal(a.position().direction(), b.position().direction())
    }
}

// a conic hasher: the axis placement and 1 or 2 radii (the focal distance of a parabola)
macro_rules! conic_hasher {
    ($(#[$doc:meta])* $name:ident, $ty:ty, $first:ident $(, $second:ident)?) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy)]
        pub struct $name {
            /// `CompTolerance`.
            pub comp_tolerance: f64,
            /// `HashTolerance`.
            pub hash_tolerance: f64,
        }
        tolerances!($name);

        impl $name {
            fn axis(&self) -> AxisPlacement {
                AxisPlacement::new(self.comp_tolerance, self.hash_tolerance)
            }
            /// Hashes the curve by its position and its radii.
            pub fn hash(&self, c: &$ty) -> u64 {
                let factor = 1.0 / self.hash_tolerance;
                hash_bytes(&[
                    self.axis().hash(c.pos()),
                    hash_rounded(c.$first(), factor),
                    $(hash_rounded(c.$second(), factor),)?
                ])
            }
            /// Compares two curves by their positions and radii.
            pub fn equal(&self, a: &$ty, b: &$ty) -> bool {
                self.axis().equal(a.pos(), b.pos())
                    && (a.$first() - b.$first()).abs() <= self.comp_tolerance
                    $(&& (a.$second() - b.$second()).abs() <= self.comp_tolerance)?
            }
        }
    };
}
conic_hasher!(
    /// `Geom2dHash_CircleHasher`.
    CircleHasher, Circle, radius
);
conic_hasher!(
    /// `Geom2dHash_EllipseHasher`.
    EllipseHasher, Ellipse, major_radius, minor_radius
);
conic_hasher!(
    /// `Geom2dHash_HyperbolaHasher`.
    HyperbolaHasher, Hyperbola, major_radius, minor_radius
);
conic_hasher!(
    /// `Geom2dHash_ParabolaHasher`.
    ParabolaHasher, Parabola, focal
);

// ---------------------------------------------------------------------------------------------
// Bezier and B-spline curves
// ---------------------------------------------------------------------------------------------

// the poles of the first curve compared with the poles of the second one; a pole the second curve does not have (C++ reads
// past its array) makes them different
fn poles_equal(points: &PointHasher, a: &[Pnt2d], b: &[Pnt2d]) -> bool {
    a.iter().enumerate().all(|(i, p)| b.get(i).is_some_and(|q| points.equal(p, q)))
}

fn weights_equal(tolerance: f64, a: Option<&[f64]>, b: Option<&[f64]>, nb_poles: usize) -> bool {
    (0..nb_poles).all(|i| {
        let w = |s: Option<&[f64]>| s.and_then(|s| s.get(i)).copied();
        match (w(a), w(b)) {
            (Some(x), Some(y)) => (x - y).abs() <= tolerance,
            _ => false,
        }
    })
}

/// `Geom2dHash_BezierCurveHasher`.
#[derive(Debug, Clone, Copy)]
pub struct BezierCurveHasher {
    /// `CompTolerance`.
    pub comp_tolerance: f64,
    /// `HashTolerance`.
    pub hash_tolerance: f64,
}
tolerances!(BezierCurveHasher);

impl BezierCurveHasher {
    /// Hashes the metadata of the curve: degree, number of poles, rationality.
    pub fn hash(&self, c: &BezierCurve) -> u64 {
        hash_bytes(&[hash_i32(c.degree()), hash_i32(c.nb_poles()), hash_i32(i32::from(c.is_rational()))])
    }
    /// Compares two curves by their full data.
    pub fn equal(&self, a: &BezierCurve, b: &BezierCurve) -> bool {
        if a.degree() != b.degree() || a.is_rational() != b.is_rational() {
            return false;
        }
        let points = PointHasher::new(self.comp_tolerance, self.hash_tolerance);
        if !poles_equal(&points, a.poles(), b.poles()) {
            return false;
        }
        !a.is_rational() || weights_equal(self.comp_tolerance, a.weights(), b.weights(), a.poles().len())
    }
}

/// `Geom2dHash_BSplineCurveHasher`.
#[derive(Debug, Clone, Copy)]
pub struct BSplineCurveHasher {
    /// `CompTolerance`.
    pub comp_tolerance: f64,
    /// `HashTolerance`.
    pub hash_tolerance: f64,
}
tolerances!(BSplineCurveHasher);

impl BSplineCurveHasher {
    /// Hashes the metadata of the curve: degree, numbers of poles and knots, rationality.
    pub fn hash(&self, c: &BSplineCurve) -> u64 {
        hash_bytes(&[
            hash_i32(c.degree()),
            hash_i32(c.nb_poles()),
            hash_i32(c.nb_knots()),
            hash_i32(i32::from(c.is_rational())),
        ])
    }
    /// Compares two curves by their full data.
    pub fn equal(&self, a: &BSplineCurve, b: &BSplineCurve) -> bool {
        if a.degree() != b.degree() || a.nb_knots() != b.nb_knots() {
            return false;
        }
        let knots_equal = a.knots().iter().zip(a.multiplicities()).enumerate().all(|(i, (&k, &m))| {
            match (b.knots().get(i), b.multiplicities().get(i)) {
                (Some(&k2), Some(&m2)) => (k - k2).abs() <= self.comp_tolerance && m == m2,
                _ => false,
            }
        });
        if !knots_equal || a.is_rational() != b.is_rational() {
            return false;
        }
        let points = PointHasher::new(self.comp_tolerance, self.hash_tolerance);
        if !poles_equal(&points, a.poles(), b.poles()) {
            return false;
        }
        !a.is_rational() || weights_equal(self.comp_tolerance, a.weights(), b.weights(), a.poles().len())
    }
}

// ---------------------------------------------------------------------------------------------
// Trimmed and offset curves, the dispatcher
// ---------------------------------------------------------------------------------------------

/// `Geom2dHash_TrimmedCurveHasher`.
#[derive(Debug, Clone, Copy)]
pub struct TrimmedCurveHasher {
    /// `CompTolerance`.
    pub comp_tolerance: f64,
    /// `HashTolerance`.
    pub hash_tolerance: f64,
}
tolerances!(TrimmedCurveHasher);

impl TrimmedCurveHasher {
    /// Hashes the trimmed curve by its basis curve and its parameters.
    pub fn hash(&self, c: &TrimmedCurve) -> u64 {
        let factor = 1.0 / self.hash_tolerance;
        let curves = CurveHasher::new(self.comp_tolerance, self.hash_tolerance);
        hash_bytes(&[
            curves.hash(Some(&c.basis_curve())),
            hash_rounded(c.first_parameter(), factor),
            hash_rounded(c.last_parameter(), factor),
        ])
    }
    /// Compares two trimmed curves.
    pub fn equal(&self, a: &TrimmedCurve, b: &TrimmedCurve) -> bool {
        let curves = CurveHasher::new(self.comp_tolerance, self.hash_tolerance);
        curves.equal(Some(&a.basis_curve()), Some(&b.basis_curve()))
            && (a.first_parameter() - b.first_parameter()).abs() <= self.comp_tolerance
            && (a.last_parameter() - b.last_parameter()).abs() <= self.comp_tolerance
    }
}

/// `Geom2dHash_OffsetCurveHasher`.
#[derive(Debug, Clone, Copy)]
pub struct OffsetCurveHasher {
    /// `CompTolerance`.
    pub comp_tolerance: f64,
    /// `HashTolerance`.
    pub hash_tolerance: f64,
}
tolerances!(OffsetCurveHasher);

impl OffsetCurveHasher {
    /// Hashes the offset curve by its basis curve and its offset.
    pub fn hash(&self, c: &OffsetCurve) -> u64 {
        let factor = 1.0 / self.hash_tolerance;
        let curves = CurveHasher::new(self.comp_tolerance, self.hash_tolerance);
        hash_bytes(&[curves.hash(Some(&c.basis_curve())), hash_rounded(c.offset(), factor)])
    }
    /// Compares two offset curves.
    pub fn equal(&self, a: &OffsetCurve, b: &OffsetCurve) -> bool {
        let curves = CurveHasher::new(self.comp_tolerance, self.hash_tolerance);
        curves.equal(Some(&a.basis_curve()), Some(&b.basis_curve()))
            && (a.offset() - b.offset()).abs() <= self.comp_tolerance
    }
}

// the name of the dynamic type of a curve the dispatcher does not know (`DynamicType()->Name()`)
fn unknown_type_name(any: &dyn Any) -> &'static str {
    if any.is::<ArchimedeanSpiralCurve>() {
        "Geom2dEval_ArchimedeanSpiralCurve"
    } else if any.is::<CircleInvoluteCurve>() {
        "Geom2dEval_CircleInvoluteCurve"
    } else if any.is::<LogarithmicSpiralCurve>() {
        "Geom2dEval_LogarithmicSpiralCurve"
    } else if any.is::<SineWaveCurve>() {
        "Geom2dEval_SineWaveCurve"
    } else if any.is::<TBezierCurve>() {
        "Geom2dEval_TBezierCurve"
    } else if any.is::<AhtBezierCurve>() {
        "Geom2dEval_AHTBezierCurve"
    } else {
        "Geom2d_Curve"
    }
}

// the address of the object behind a handle
fn address(c: &Arc<dyn Curve2d>) -> *const () {
    Arc::as_ptr(c).cast::<()>()
}

/// `Geom2dHash_CurveHasher`: dispatches on the exact type of the curve.
#[derive(Debug, Clone, Copy)]
pub struct CurveHasher {
    /// `CompTolerance`.
    pub comp_tolerance: f64,
    /// `HashTolerance`.
    pub hash_tolerance: f64,
}
tolerances!(CurveHasher);

impl CurveHasher {
    /// Hashes a curve (`None`, the null handle, hashes to 0).
    pub fn hash(&self, curve: Option<&Arc<dyn Curve2d>>) -> u64 {
        let Some(curve) = curve else {
            return 0;
        };
        let (c, h) = (self.comp_tolerance, self.hash_tolerance);
        let any = curve.as_any();
        if let Some(x) = any.downcast_ref::<Line>() {
            LineHasher::new(c, h).hash(x)
        } else if let Some(x) = any.downcast_ref::<Circle>() {
            CircleHasher::new(c, h).hash(x)
        } else if let Some(x) = any.downcast_ref::<Ellipse>() {
            EllipseHasher::new(c, h).hash(x)
        } else if let Some(x) = any.downcast_ref::<Hyperbola>() {
            HyperbolaHasher::new(c, h).hash(x)
        } else if let Some(x) = any.downcast_ref::<Parabola>() {
            ParabolaHasher::new(c, h).hash(x)
        } else if let Some(x) = any.downcast_ref::<BezierCurve>() {
            BezierCurveHasher::new(c, h).hash(x)
        } else if let Some(x) = any.downcast_ref::<BSplineCurve>() {
            BSplineCurveHasher::new(c, h).hash(x)
        } else if let Some(x) = any.downcast_ref::<TrimmedCurve>() {
            TrimmedCurveHasher::new(c, h).hash(x)
        } else if let Some(x) = any.downcast_ref::<OffsetCurve>() {
            OffsetCurveHasher::new(c, h).hash(x)
        } else {
            // an unknown curve type: the name of the type is hashed
            cstring_hash(unknown_type_name(any).as_bytes())
        }
    }

    /// Compares two curves: two null handles are equal, curves of different types are not, any other pair by the hasher of
    /// the type (by identity for an unknown type).
    pub fn equal(&self, a: Option<&Arc<dyn Curve2d>>, b: Option<&Arc<dyn Curve2d>>) -> bool {
        let (a, b) = match (a, b) {
            (Some(a), Some(b)) => (a, b),
            (None, None) => return true,
            _ => return false,
        };
        if address(a) == address(b) {
            return true;
        }
        let (x, y) = (a.as_any(), b.as_any());
        // must be the same type
        if x.type_id() != y.type_id() {
            return false;
        }
        let (c, h) = (self.comp_tolerance, self.hash_tolerance);
        macro_rules! pair {
            ($hasher:ident, $ty:ty) => {
                if let (Some(p), Some(q)) = (x.downcast_ref::<$ty>(), y.downcast_ref::<$ty>()) {
                    return $hasher::new(c, h).equal(p, q);
                }
            };
        }
        pair!(LineHasher, Line);
        pair!(CircleHasher, Circle);
        pair!(EllipseHasher, Ellipse);
        pair!(HyperbolaHasher, Hyperbola);
        pair!(ParabolaHasher, Parabola);
        pair!(BezierCurveHasher, BezierCurve);
        pair!(BSplineCurveHasher, BSplineCurve);
        pair!(TrimmedCurveHasher, TrimmedCurve);
        pair!(OffsetCurveHasher, OffsetCurve);
        // an unknown curve type: compared by identity (the handles differ)
        false
    }
}
