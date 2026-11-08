//! Procedural textures (ADR 0045 amendment): patterns computed from a point in the body's
//! own coordinates, so they need no UV maps and move with the body. Deterministic: the same
//! point always gives the same colour (no random state), so renders stay reproducible.

use crate::math::Vec3;

/// A pattern over a material's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Texture {
    pub kind: TextureKind,
    /// Size of the pattern's feature (ring spacing, weave cell, grain) in micrometres.
    pub size_um: u32,
}

/// What the pattern looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureKind {
    /// Growth rings and grain along the body's X axis.
    Wood,
    /// Fine streaks along the body's X axis.
    Brushed,
    /// A 2x2 twill weave.
    Carbon,
    /// Small light and dark specks.
    Speckle,
}

impl TextureKind {
    /// From the document's keyword (`wood`, `brushed`, `carbon`, `speckle`).
    #[must_use]
    pub fn from_keyword(k: &str) -> Option<TextureKind> {
        match k {
            "wood" => Some(TextureKind::Wood),
            "brushed" => Some(TextureKind::Brushed),
            "carbon" => Some(TextureKind::Carbon),
            "speckle" => Some(TextureKind::Speckle),
            _ => None,
        }
    }
}

impl Texture {
    /// The shader's texture word: kind (1 wood, 2 brushed, 3 carbon, 4 speckle; 0 none) and
    /// the inverse size in 1/mm.
    #[must_use]
    pub(crate) fn shader_word(t: Option<Texture>) -> [f32; 4] {
        match t {
            None => [0.0; 4],
            Some(t) => {
                let kind = match t.kind {
                    TextureKind::Wood => 1.0,
                    TextureKind::Brushed => 2.0,
                    TextureKind::Carbon => 3.0,
                    TextureKind::Speckle => 4.0,
                };
                [kind, (1000.0 / f64::from(t.size_um.max(1))) as f32, 0.0, 0.0]
            }
        }
    }

    /// The factor (per linear RGB channel, about 0.3 to 1.3) that the pattern multiplies the
    /// albedo by at model point `p` (mm) on a surface with model normal `n`.
    #[must_use]
    pub fn factor(&self, p: Vec3, n: Vec3) -> [f64; 3] {
        let s = (f64::from(self.size_um) / 1000.0).max(1e-3);
        let q = [p[0] / s, p[1] / s, p[2] / s];
        let k = match self.kind {
            TextureKind::Wood => wood(q),
            TextureKind::Brushed => brushed(q),
            TextureKind::Carbon => carbon(q, n),
            TextureKind::Speckle => speckle(q),
        };
        [k; 3].map(|v: f64| v.clamp(0.0, 1.6))
    }
}

/// Growth rings around an axis along X (off the origin, so a part does not show the pith),
/// wobbled by noise, with fine grain streaks along X.
fn wood(q: Vec3) -> f64 {
    let (y, z) = (q[1] + 3.7, q[2] + 5.3);
    let wobble = 0.8 * fbm([q[0] * 0.08, q[1] * 0.35, q[2] * 0.35]);
    let r = (y * y + z * z).sqrt() + wobble;
    let ring = r - r.floor();
    // Late wood: a darker band at the end of each ring.
    let late = smoothstep(0.62, 0.9, ring) * (1.0 - smoothstep(0.93, 1.0, ring));
    let grain = noise3([q[0] * 0.15, q[1] * 9.0, q[2] * 9.0]);
    0.78 + 0.3 * (1.0 - late) - 0.28 * late + 0.12 * (grain - 0.5)
}

/// Streaks along X: noise stretched far along the brushing direction.
fn brushed(q: Vec3) -> f64 {
    let fine = noise3([q[0] * 0.04, q[1] * 12.0, q[2] * 12.0]);
    let coarse = noise3([q[0] * 0.02, q[1] * 2.0, q[2] * 2.0]);
    0.86 + 0.18 * (fine - 0.5) + 0.1 * (coarse - 0.5)
}

/// A 2x2 twill on the plane facing the normal's largest axis: tows over two, under two,
/// shifted one each row; each tow shaded across its width.
fn carbon(q: Vec3, n: Vec3) -> f64 {
    let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
    let (u, v) = if az >= ax && az >= ay {
        (q[0], q[1])
    } else if ax >= ay {
        (q[1], q[2])
    } else {
        (q[0], q[2])
    };
    let (i, j) = (u.floor(), v.floor());
    let warp = (i + j).rem_euclid(4.0) < 2.0;
    // Across the tow: 0 at its edges, 1 in its middle.
    let across = if warp { v - j } else { u - i };
    let bulge = (std::f64::consts::PI * across).sin();
    // Warp and weft catch the light differently (the fibres run across each other).
    let tone = if warp { 1.15 } else { 0.8 };
    tone * (0.55 + 0.45 * bulge)
}

/// Light and dark specks on a fine noise ground: a ball in some lattice cells, jittered in
/// all three axes, so any surface through the body cuts discs of varied sizes.
fn speckle(q: Vec3) -> f64 {
    let cell = [q[0].floor(), q[1].floor(), q[2].floor()];
    let h = [
        hash(cell),
        hash([cell[0] + 17.0, cell[1] - 31.0, cell[2] + 7.0]),
        hash([cell[0] - 5.0, cell[1] + 11.0, cell[2] - 23.0]),
    ];
    let centre = [cell[0] + 0.2 + 0.6 * h[1], cell[1] + 0.2 + 0.6 * h[2], cell[2] + 0.2 + 0.6 * h[0]];
    let d = ((q[0] - centre[0]).powi(2) + (q[1] - centre[1]).powi(2) + (q[2] - centre[2]).powi(2)).sqrt();
    // About a third of the cells hold a speck.
    let speck = if h[0] < 0.35 { 1.0 - smoothstep(0.22, 0.3, d) } else { 0.0 };
    let tone = if h[1] < 0.6 { -0.5 } else { 0.35 };
    let ground = noise3([q[0] * 0.5, q[1] * 0.5, q[2] * 0.5]);
    0.97 + 0.12 * (ground - 0.5) + tone * speck
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A lattice point's value in [0, 1). 32-bit integer mixing, so the viewport shader
/// (`tex_hash` in shader.wgsl) computes the same values.
fn hash(c: Vec3) -> f64 {
    // Lattice coordinates as 32-bit two's complement (the shader's `bitcast<u32>(i32(x))`).
    let [x, y, z] = c.map(|v| v as i32 as u32);
    let mut h = x.wrapping_mul(0x8DA6_B343) ^ y.wrapping_mul(0xD816_3841) ^ z.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    f64::from(h >> 8) / 16_777_216.0
}

/// Value noise in [0, 1], smooth between lattice points.
fn noise3(p: Vec3) -> f64 {
    let f = [p[0].floor(), p[1].floor(), p[2].floor()];
    let t = [p[0] - f[0], p[1] - f[1], p[2] - f[2]].map(|v| v * v * (3.0 - 2.0 * v));
    let at = |dx: f64, dy: f64, dz: f64| hash([f[0] + dx, f[1] + dy, f[2] + dz]);
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let x00 = lerp(at(0.0, 0.0, 0.0), at(1.0, 0.0, 0.0), t[0]);
    let x10 = lerp(at(0.0, 1.0, 0.0), at(1.0, 1.0, 0.0), t[0]);
    let x01 = lerp(at(0.0, 0.0, 1.0), at(1.0, 0.0, 1.0), t[0]);
    let x11 = lerp(at(0.0, 1.0, 1.0), at(1.0, 1.0, 1.0), t[0]);
    lerp(lerp(x00, x10, t[1]), lerp(x01, x11, t[1]), t[2])
}

/// Three octaves of value noise, centred on 0 (about -0.5 to 0.5).
fn fbm(p: Vec3) -> f64 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut q = p;
    for _ in 0..3 {
        sum += amp * (noise3(q) - 0.5);
        q = q.map(|v| v * 2.03);
        amp *= 0.5;
    }
    sum * 1.75
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tex(kind: TextureKind) -> Texture {
        Texture { kind, size_um: 2000 }
    }

    #[test]
    fn patterns_vary_stay_bounded_and_repeat_exactly() {
        for kind in [TextureKind::Wood, TextureKind::Brushed, TextureKind::Carbon, TextureKind::Speckle] {
            let t = tex(kind);
            let mut seen = Vec::new();
            for i in 0..400 {
                let p = [f64::from(i) * 0.37, f64::from(i % 17) * 0.91, f64::from(i % 5) * 1.3];
                let f = t.factor(p, [0.0, 0.0, 1.0]);
                assert!(f.iter().all(|v| (0.0..=1.6).contains(v)), "{kind:?} {f:?}");
                assert_eq!(f.map(f64::to_bits), t.factor(p, [0.0, 0.0, 1.0]).map(f64::to_bits));
                seen.push(f[0]);
            }
            let (lo, hi) = seen.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
            assert!(hi - lo > 0.1, "{kind:?} is flat: {lo}..{hi}");
            // About neutral on average, so a texture keeps the body's colour.
            let mean = seen.iter().sum::<f64>() / seen.len() as f64;
            assert!((0.7..=1.1).contains(&mean), "{kind:?} mean {mean}");
        }
    }

    #[test]
    fn size_scales_the_pattern() {
        let small = Texture { kind: TextureKind::Carbon, size_um: 1000 };
        let big = Texture { kind: TextureKind::Carbon, size_um: 2000 };
        let n = [0.0, 0.0, 1.0];
        for i in 0..50 {
            let p = [f64::from(i) * 0.13, f64::from(i) * 0.29, 0.0];
            assert_eq!(small.factor(p, n).map(f64::to_bits), big.factor(p.map(|v| v * 2.0), n).map(f64::to_bits));
        }
    }

    #[test]
    fn noise_is_in_range_and_continuous() {
        let mut prev = noise3([0.0, 0.3, 0.7]);
        for i in 1..1000 {
            let v = noise3([f64::from(i) * 0.001, 0.3, 0.7]);
            assert!((0.0..=1.0).contains(&v));
            assert!((v - prev).abs() < 0.02);
            prev = v;
        }
    }
}
