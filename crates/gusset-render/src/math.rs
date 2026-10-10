//! Small, dependency-free vector and matrix helpers for display code.
//!
//! Camera math runs in `f64` (CAD coordinates can be large); matrices are converted to
//! `f32` only when they are written to GPU uniforms. Matrices are column-major
//! (`m[col][row]`), matching WGSL `mat4x4<f32>`, and map to wgpu clip space
//! (right-handed view space, depth in `[0, 1]`).

/// A 3D vector in `f64`.
pub type Vec3 = [f64; 3];
/// A column-major 4x4 matrix in `f64`: `m[col][row]`.
pub type Mat4 = [[f64; 4]; 4];

/// `a + b`.
#[must_use]
pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `a - b`.
#[must_use]
pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `a * s`.
#[must_use]
pub fn scale(a: Vec3, s: f64) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// Dot product.
#[must_use]
pub fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Cross product.
#[must_use]
pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Euclidean length.
#[must_use]
pub fn length(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}

/// Unit vector, or `fallback` for (near-)zero or non-finite input.
#[must_use]
pub fn normalize_or(a: Vec3, fallback: Vec3) -> Vec3 {
    let l = length(a);
    if l > 1e-300 && l.is_finite() { scale(a, 1.0 / l) } else { fallback }
}

/// Rotate `v` around the unit axis `axis` by `angle` radians (Rodrigues).
#[must_use]
pub fn rotate(v: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (s, c) = angle.sin_cos();
    add(add(scale(v, c), scale(cross(axis, v), s)), scale(axis, dot(axis, v) * (1.0 - c)))
}

/// The identity matrix.
pub const IDENTITY: Mat4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// Element-wise `f(a[i], b[i])`.
pub fn zip_with<T: Copy, U: Copy + Default, const N: usize>(a: [T; N], b: [T; N], f: impl Fn(T, T) -> U) -> [U; N] {
    let mut out = [U::default(); N];
    for ((o, x), y) in out.iter_mut().zip(a).zip(b) {
        *o = f(x, y);
    }
    out
}

/// `m * v` for a homogeneous vector.
#[must_use]
pub fn mat_vec(m: &Mat4, v: [f64; 4]) -> [f64; 4] {
    let [c0, c1, c2, c3] = m;
    let [x, y, z, w] = v;
    [
        c0[0] * x + c1[0] * y + c2[0] * z + c3[0] * w,
        c0[1] * x + c1[1] * y + c2[1] * z + c3[1] * w,
        c0[2] * x + c1[2] * y + c2[2] * z + c3[2] * w,
        c0[3] * x + c1[3] * y + c2[3] * z + c3[3] * w,
    ]
}

/// `a * b`.
#[must_use]
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    b.map(|col| mat_vec(a, col))
}

/// Transform a point (w = 1) and return the homogeneous result.
#[must_use]
pub fn transform4(m: &Mat4, p: Vec3) -> [f64; 4] {
    mat_vec(m, [p[0], p[1], p[2], 1.0])
}

/// Transform a point (affine, ignores the projective row).
#[must_use]
pub fn transform_point(m: &Mat4, p: Vec3) -> Vec3 {
    let r = transform4(m, p);
    [r[0], r[1], r[2]]
}

/// Transform a direction (w = 0).
#[must_use]
pub fn transform_dir(m: &Mat4, d: Vec3) -> Vec3 {
    let r = mat_vec(m, [d[0], d[1], d[2], 0.0]);
    [r[0], r[1], r[2]]
}

/// A translation matrix.
#[must_use]
pub fn translation(t: Vec3) -> Mat4 {
    let mut m = IDENTITY;
    m[3] = [t[0], t[1], t[2], 1.0];
    m
}

/// Right-handed look-at view matrix (camera looks down -Z).
#[must_use]
pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    let f = normalize_or(sub(target, eye), [0.0, 0.0, -1.0]);
    let s = normalize_or(cross(f, up), [1.0, 0.0, 0.0]);
    let u = cross(s, f);
    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0],
    ]
}

/// Right-handed perspective projection to wgpu clip space (depth 0..1).
#[must_use]
pub fn perspective(fov_y: f64, aspect: f64, near: f64, far: f64) -> Mat4 {
    let f = 1.0 / (fov_y * 0.5).tan();
    let r = far / (near - far);
    [[f / aspect, 0.0, 0.0, 0.0], [0.0, f, 0.0, 0.0], [0.0, 0.0, r, -1.0], [0.0, 0.0, r * near, 0.0]]
}

/// Right-handed orthographic projection to wgpu clip space (depth 0..1).
#[must_use]
pub fn orthographic(half_w: f64, half_h: f64, near: f64, far: f64) -> Mat4 {
    let r = 1.0 / (near - far);
    [[1.0 / half_w, 0.0, 0.0, 0.0], [0.0, 1.0 / half_h, 0.0, 0.0], [0.0, 0.0, r, 0.0], [0.0, 0.0, r * near, 1.0]]
}

/// General 4x4 inverse (cofactor expansion). `None` if singular.
#[must_use]
pub fn invert(m: &Mat4) -> Option<Mat4> {
    // Flatten column-major into a[16] with a[c*4+r].
    let a: [f64; 16] = [
        m[0][0], m[0][1], m[0][2], m[0][3], m[1][0], m[1][1], m[1][2], m[1][3], m[2][0], m[2][1], m[2][2], m[2][3],
        m[3][0], m[3][1], m[3][2], m[3][3],
    ];
    let mut inv = [0.0; 16];
    inv[0] =
        a[5] * a[10] * a[15] - a[5] * a[11] * a[14] - a[9] * a[6] * a[15] + a[9] * a[7] * a[14] + a[13] * a[6] * a[11]
            - a[13] * a[7] * a[10];
    inv[4] =
        -a[4] * a[10] * a[15] + a[4] * a[11] * a[14] + a[8] * a[6] * a[15] - a[8] * a[7] * a[14] - a[12] * a[6] * a[11]
            + a[12] * a[7] * a[10];
    inv[8] =
        a[4] * a[9] * a[15] - a[4] * a[11] * a[13] - a[8] * a[5] * a[15] + a[8] * a[7] * a[13] + a[12] * a[5] * a[11]
            - a[12] * a[7] * a[9];
    inv[12] =
        -a[4] * a[9] * a[14] + a[4] * a[10] * a[13] + a[8] * a[5] * a[14] - a[8] * a[6] * a[13] - a[12] * a[5] * a[10]
            + a[12] * a[6] * a[9];
    inv[1] =
        -a[1] * a[10] * a[15] + a[1] * a[11] * a[14] + a[9] * a[2] * a[15] - a[9] * a[3] * a[14] - a[13] * a[2] * a[11]
            + a[13] * a[3] * a[10];
    inv[5] =
        a[0] * a[10] * a[15] - a[0] * a[11] * a[14] - a[8] * a[2] * a[15] + a[8] * a[3] * a[14] + a[12] * a[2] * a[11]
            - a[12] * a[3] * a[10];
    inv[9] =
        -a[0] * a[9] * a[15] + a[0] * a[11] * a[13] + a[8] * a[1] * a[15] - a[8] * a[3] * a[13] - a[12] * a[1] * a[11]
            + a[12] * a[3] * a[9];
    inv[13] =
        a[0] * a[9] * a[14] - a[0] * a[10] * a[13] - a[8] * a[1] * a[14] + a[8] * a[2] * a[13] + a[12] * a[1] * a[10]
            - a[12] * a[2] * a[9];
    inv[2] =
        a[1] * a[6] * a[15] - a[1] * a[7] * a[14] - a[5] * a[2] * a[15] + a[5] * a[3] * a[14] + a[13] * a[2] * a[7]
            - a[13] * a[3] * a[6];
    inv[6] =
        -a[0] * a[6] * a[15] + a[0] * a[7] * a[14] + a[4] * a[2] * a[15] - a[4] * a[3] * a[14] - a[12] * a[2] * a[7]
            + a[12] * a[3] * a[6];
    inv[10] =
        a[0] * a[5] * a[15] - a[0] * a[7] * a[13] - a[4] * a[1] * a[15] + a[4] * a[3] * a[13] + a[12] * a[1] * a[7]
            - a[12] * a[3] * a[5];
    inv[14] =
        -a[0] * a[5] * a[14] + a[0] * a[6] * a[13] + a[4] * a[1] * a[14] - a[4] * a[2] * a[13] - a[12] * a[1] * a[6]
            + a[12] * a[2] * a[5];
    inv[3] =
        -a[1] * a[6] * a[11] + a[1] * a[7] * a[10] + a[5] * a[2] * a[11] - a[5] * a[3] * a[10] - a[9] * a[2] * a[7]
            + a[9] * a[3] * a[6];
    inv[7] = a[0] * a[6] * a[11] - a[0] * a[7] * a[10] - a[4] * a[2] * a[11] + a[4] * a[3] * a[10] + a[8] * a[2] * a[7]
        - a[8] * a[3] * a[6];
    inv[11] = -a[0] * a[5] * a[11] + a[0] * a[7] * a[9] + a[4] * a[1] * a[11] - a[4] * a[3] * a[9] - a[8] * a[1] * a[7]
        + a[8] * a[3] * a[5];
    inv[15] = a[0] * a[5] * a[10] - a[0] * a[6] * a[9] - a[4] * a[1] * a[10] + a[4] * a[2] * a[9] + a[8] * a[1] * a[6]
        - a[8] * a[2] * a[5];
    let det = a[0] * inv[0] + a[1] * inv[4] + a[2] * inv[8] + a[3] * inv[12];
    if det.abs() < 1e-300 || !det.is_finite() {
        return None;
    }
    let d = 1.0 / det;
    Some([
        [inv[0] * d, inv[1] * d, inv[2] * d, inv[3] * d],
        [inv[4] * d, inv[5] * d, inv[6] * d, inv[7] * d],
        [inv[8] * d, inv[9] * d, inv[10] * d, inv[11] * d],
        [inv[12] * d, inv[13] * d, inv[14] * d, inv[15] * d],
    ])
}

/// Inverse-transpose of the upper 3x3 (normal matrix), as three columns.
#[must_use]
pub fn normal_matrix(m: &Mat4) -> [Vec3; 3] {
    let c0 = [m[0][0], m[0][1], m[0][2]];
    let c1 = [m[1][0], m[1][1], m[1][2]];
    let c2 = [m[2][0], m[2][1], m[2][2]];
    // Rows of the inverse are cross products of columns / det; transposed → columns.
    let r0 = cross(c1, c2);
    let r1 = cross(c2, c0);
    let r2 = cross(c0, c1);
    let det = dot(c0, r0);
    if det.abs() < 1e-300 || !det.is_finite() {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    let d = 1.0 / det;
    [scale(r0, d), scale(r1, d), scale(r2, d)]
}

/// Convert to `f32` column-major for GPU upload.
#[must_use]
pub fn to_f32(m: &Mat4) -> [[f32; 4]; 4] {
    m.map(|c| c.map(|v| v as f32))
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Minimum corner.
    pub min: Vec3,
    /// Maximum corner.
    pub max: Vec3,
}

impl Aabb {
    /// An empty box (min > max) that grows on [`Aabb::include`].
    pub const EMPTY: Aabb = Aabb { min: [f64::INFINITY; 3], max: [f64::NEG_INFINITY; 3] };

    /// Whether nothing was included yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.min.iter().zip(&self.max).any(|(a, b)| a > b)
    }

    /// Grow to include `p` (non-finite points are ignored).
    pub fn include(&mut self, p: Vec3) {
        if p.iter().all(|c| c.is_finite()) {
            self.min = zip_with(self.min, p, f64::min);
            self.max = zip_with(self.max, p, f64::max);
        }
    }

    /// Union of two boxes.
    #[must_use]
    pub fn union(&self, o: &Aabb) -> Aabb {
        let mut r = *self;
        if !o.is_empty() {
            r.include(o.min);
            r.include(o.max);
        }
        r
    }

    /// Center point.
    #[must_use]
    pub fn center(&self) -> Vec3 {
        scale(add(self.min, self.max), 0.5)
    }

    /// Radius of the bounding sphere around [`Aabb::center`].
    #[must_use]
    pub fn radius(&self) -> f64 {
        0.5 * length(sub(self.max, self.min))
    }

    /// The eight corners.
    #[must_use]
    pub fn corners(&self) -> [Vec3; 8] {
        let (a, b) = (self.min, self.max);
        [
            [a[0], a[1], a[2]],
            [b[0], a[1], a[2]],
            [a[0], b[1], a[2]],
            [b[0], b[1], a[2]],
            [a[0], a[1], b[2]],
            [b[0], a[1], b[2]],
            [a[0], b[1], b[2]],
            [b[0], b[1], b[2]],
        ]
    }

    /// Box of the transformed corners.
    #[must_use]
    pub fn transformed(&self, m: &Mat4) -> Aabb {
        let mut r = Aabb::EMPTY;
        if !self.is_empty() {
            for c in self.corners() {
                r.include(transform_point(m, c));
            }
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &Mat4, b: &Mat4) -> bool {
        a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn inverse_round_trips() {
        let m = mul(&translation([1.0, -2.0, 3.0]), &look_at([3.0, 4.0, 5.0], [0.0; 3], [0.0, 0.0, 1.0]));
        let inv = invert(&m).expect("invertible");
        assert!(close(&mul(&m, &inv), &IDENTITY));
        assert!(invert(&[[0.0; 4]; 4]).is_none());
    }

    #[test]
    fn perspective_maps_near_far_to_0_1() {
        let p = perspective(1.0, 1.5, 2.0, 50.0);
        let n = transform4(&p, [0.0, 0.0, -2.0]);
        let f = transform4(&p, [0.0, 0.0, -50.0]);
        assert!((n[2] / n[3]).abs() < 1e-12);
        assert!((f[2] / f[3] - 1.0).abs() < 1e-12);
        let o = orthographic(1.0, 1.0, 2.0, 50.0);
        assert!(transform4(&o, [0.0, 0.0, -2.0])[2].abs() < 1e-12);
        assert!((transform4(&o, [0.0, 0.0, -50.0])[2] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn look_at_puts_target_on_negative_z() {
        let v = look_at([0.0, -10.0, 0.0], [0.0; 3], [0.0, 0.0, 1.0]);
        let t = transform_point(&v, [0.0; 3]);
        assert!(t[0].abs() < 1e-12 && t[1].abs() < 1e-12 && (t[2] + 10.0).abs() < 1e-12);
        // World +Z is screen up, world +X is screen right.
        let up = transform_dir(&v, [0.0, 0.0, 1.0]);
        let right = transform_dir(&v, [1.0, 0.0, 0.0]);
        assert!((up[1] - 1.0).abs() < 1e-12 && (right[0] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn rotate_and_normal_matrix() {
        let r = rotate([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2);
        assert!(r[0].abs() < 1e-12 && (r[1] - 1.0).abs() < 1e-12);
        let mut s = IDENTITY;
        s[0][0] = 2.0;
        let n = normal_matrix(&s);
        assert!((n[0][0] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn aabb_ops() {
        let mut b = Aabb::EMPTY;
        assert!(b.is_empty());
        b.include([1.0, 2.0, 3.0]);
        b.include([-1.0, 0.0, f64::NAN]);
        b.include([-1.0, 0.0, 1.0]);
        assert!(!b.is_empty());
        assert!(length(sub(b.center(), [0.0, 1.0, 2.0])) < 1e-12);
        let t = b.transformed(&translation([1.0, 1.0, 1.0]));
        assert!(length(sub(t.min, [0.0, 1.0, 2.0])) < 1e-12);
    }
}
