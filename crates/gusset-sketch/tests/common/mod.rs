// Test-only helpers (see the note at the top of the test files).
#![allow(dead_code, unreachable_pub, clippy::unwrap_used, clippy::indexing_slicing)]

use gusset_sketch::{EntityId, Sketch};

pub struct Rect {
    /// Corners counter-clockwise from bottom-left.
    pub p: [EntityId; 4],
    /// bottom (p0→p1), right (p1→p2), top (p2→p3), left (p3→p0).
    pub l: [EntityId; 4],
}

pub fn rect(s: &mut Sketch, x: f64, y: f64, w: f64, h: f64) -> Rect {
    let p = [
        s.add_point([x, y]).unwrap(),
        s.add_point([x + w, y]).unwrap(),
        s.add_point([x + w, y + h]).unwrap(),
        s.add_point([x, y + h]).unwrap(),
    ];
    let l = [
        s.add_line_between(p[0], p[1]).unwrap(),
        s.add_line_between(p[1], p[2]).unwrap(),
        s.add_line_between(p[2], p[3]).unwrap(),
        s.add_line_between(p[3], p[0]).unwrap(),
    ];
    Rect { p, l }
}

pub fn close(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

/// Deterministic SplitMix64 generator for property tests.
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        lo + (hi - lo) * u
    }
}
