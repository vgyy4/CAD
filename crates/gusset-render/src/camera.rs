//! Viewport camera: perspective/orthographic projection, orbit/pan/zoom, fit-to-bounds
//! and named views. World space is Z-up (CAD convention).

use crate::math::{
    self, Aabb, Mat4, Vec3, add, cross, dot, look_at, normalize_or, orthographic, perspective, rotate, scale, sub,
};

/// Projection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Projection {
    /// Perspective with the camera's vertical field of view.
    #[default]
    Perspective,
    /// Orthographic. The visible height matches the perspective view at the target
    /// distance, so switching modes keeps the part the same size on screen.
    Orthographic,
}

/// Standard named views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedView {
    /// Looking along +Y (from -Y), Z up.
    Front,
    /// Looking along -Y (from +Y), Z up.
    Back,
    /// Looking down -Z (from +Z), Y up.
    Top,
    /// Looking up +Z (from -Z), -Y up (X stays to the right).
    Bottom,
    /// Looking along +X (from -X), Z up.
    Left,
    /// Looking along -X (from +X), Z up.
    Right,
    /// Isometric from front-right-top.
    Iso,
}

impl NamedView {
    /// All named views.
    pub const ALL: &'static [NamedView] = &[
        NamedView::Front,
        NamedView::Back,
        NamedView::Top,
        NamedView::Bottom,
        NamedView::Left,
        NamedView::Right,
        NamedView::Iso,
    ];

    /// Lower-case name (used by MCP `render_view`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Back => "back",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
            Self::Iso => "iso",
        }
    }

    /// Parse a name (case-insensitive).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|v| v.name().eq_ignore_ascii_case(s.trim()))
    }

    /// `(forward, up)` unit vectors for this view.
    #[must_use]
    pub fn orientation(self) -> (Vec3, Vec3) {
        let z = [0.0, 0.0, 1.0];
        match self {
            Self::Front => ([0.0, 1.0, 0.0], z),
            Self::Back => ([0.0, -1.0, 0.0], z),
            Self::Top => ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            Self::Bottom => ([0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
            Self::Left => ([1.0, 0.0, 0.0], z),
            Self::Right => ([-1.0, 0.0, 0.0], z),
            Self::Iso => {
                let f = normalize_or([-1.0, 1.0, -1.0], [0.0, 1.0, 0.0]);
                let right = normalize_or(cross(f, z), [1.0, 0.0, 0.0]);
                (f, cross(right, f))
            }
        }
    }
}

/// A ray in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// Origin.
    pub origin: Vec3,
    /// Unit direction.
    pub dir: Vec3,
}

/// An orbit camera around a target point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Orbit center.
    pub target: Vec3,
    /// Distance from the eye to the target (> 0).
    pub distance: f64,
    /// Unit view direction (eye → target).
    pub forward: Vec3,
    /// Unit up vector, orthogonal to `forward`.
    pub up: Vec3,
    /// Vertical field of view in radians (perspective; also sizes orthographic views).
    pub fov_y: f64,
    /// Projection mode.
    pub projection: Projection,
}

impl Default for Camera {
    fn default() -> Self {
        let mut c = Camera {
            target: [0.0; 3],
            distance: 100.0,
            forward: [0.0, 1.0, 0.0],
            up: [0.0, 0.0, 1.0],
            fov_y: 35.0_f64.to_radians(),
            projection: Projection::Perspective,
        };
        c.set_view(NamedView::Iso);
        c
    }
}

/// Near/far planes plus the view and projection matrices for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraMatrices {
    /// World → view.
    pub view: Mat4,
    /// View → clip.
    pub proj: Mat4,
    /// Near plane distance.
    pub near: f64,
    /// Far plane distance.
    pub far: f64,
}

impl Camera {
    /// Eye position.
    #[must_use]
    pub fn eye(&self) -> Vec3 {
        sub(self.target, scale(self.forward, self.distance))
    }

    /// Unit right vector.
    #[must_use]
    pub fn right(&self) -> Vec3 {
        normalize_or(cross(self.forward, self.up), [1.0, 0.0, 0.0])
    }

    /// Re-orthonormalize `forward`/`up` (after many incremental rotations).
    pub fn orthonormalize(&mut self) {
        self.forward = normalize_or(self.forward, [0.0, 1.0, 0.0]);
        let right = normalize_or(cross(self.forward, self.up), [1.0, 0.0, 0.0]);
        self.up = normalize_or(cross(right, self.forward), [0.0, 0.0, 1.0]);
    }

    /// Jump to a named view (keeps target and distance).
    pub fn set_view(&mut self, v: NamedView) {
        let (f, u) = v.orientation();
        self.forward = f;
        self.up = u;
        self.orthonormalize();
    }

    /// Turntable orbit: `yaw` around world Z, then `pitch` around the camera's right
    /// axis (radians). Positive yaw turns the view to the right, positive pitch tilts
    /// the view down (the camera moves up).
    pub fn orbit(&mut self, yaw: f64, pitch: f64) {
        let z = [0.0, 0.0, 1.0];
        self.forward = rotate(self.forward, z, -yaw);
        self.up = rotate(self.up, z, -yaw);
        let right = self.right();
        let f = rotate(self.forward, right, -pitch);
        let u = rotate(self.up, right, -pitch);
        self.forward = f;
        self.up = u;
        self.orthonormalize();
    }

    /// Orbit from a mouse drag in logical pixels (a drag across the viewport height
    /// turns about 180 degrees).
    pub fn orbit_pixels(&mut self, dx: f64, dy: f64, viewport_height: f64) {
        let k = std::f64::consts::PI / viewport_height.max(1.0);
        self.orbit(dx * k, dy * k);
    }

    /// World-space size of one pixel at the target distance.
    #[must_use]
    pub fn pixel_size_at_target(&self, viewport_height: f64) -> f64 {
        2.0 * self.half_height_at_target() / viewport_height.max(1.0)
    }

    fn half_height_at_target(&self) -> f64 {
        self.distance * (self.fov_y * 0.5).tan()
    }

    /// Pan by a mouse drag in pixels: the point under the cursor at the target depth
    /// follows the cursor.
    pub fn pan_pixels(&mut self, dx: f64, dy: f64, viewport_height: f64) {
        let s = self.pixel_size_at_target(viewport_height);
        let delta = add(scale(self.right(), -dx * s), scale(self.up, dy * s));
        self.target = add(self.target, delta);
    }

    /// Zoom by `factor` (> 1 zooms in) towards the target.
    pub fn zoom(&mut self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            self.distance = (self.distance / factor).clamp(1e-6, 1e12);
        }
    }

    /// Zoom by `factor` keeping the world point under `pixel` fixed on screen (at the
    /// target depth).
    pub fn zoom_at(&mut self, factor: f64, pixel: [f64; 2], viewport: [f64; 2]) {
        if !(factor.is_finite() && factor > 0.0) {
            return;
        }
        let s = self.pixel_size_at_target(viewport[1]);
        let ox = (pixel[0] - viewport[0] * 0.5) * s;
        let oy = (viewport[1] * 0.5 - pixel[1]) * s;
        let before = add(scale(self.right(), ox), scale(self.up, oy));
        self.zoom(factor);
        let after = scale(before, 1.0 / factor);
        self.target = add(self.target, sub(before, after));
    }

    /// Frame `bounds` so it fills the viewport (with a small margin), keeping the
    /// current orientation.
    pub fn fit(&mut self, bounds: &Aabb, aspect: f64) {
        if bounds.is_empty() {
            return;
        }
        let r = bounds.radius().max(1e-6);
        self.target = bounds.center();
        let half_v = self.fov_y * 0.5;
        let half_h = (half_v.tan() * aspect.max(1e-3)).atan();
        let half = half_v.min(half_h);
        self.distance = r * 1.08 / half.sin();
    }

    /// Near/far planes that enclose `bounds` tightly (better depth precision).
    #[must_use]
    pub fn clip_planes(&self, bounds: &Aabb) -> (f64, f64) {
        let (center, r) =
            if bounds.is_empty() { (self.target, self.distance) } else { (bounds.center(), bounds.radius()) };
        let d = dot(sub(center, self.eye()), self.forward);
        let r = r.max(1e-6) * 1.05;
        let far = (d + r).max(1e-3);
        let near = match self.projection {
            Projection::Perspective => (d - r).max(far * 1e-4),
            Projection::Orthographic => d - r,
        };
        (near, far)
    }

    /// The view and projection matrices for a viewport of `aspect` = width / height.
    #[must_use]
    pub fn matrices(&self, aspect: f64, bounds: &Aabb) -> CameraMatrices {
        let (near, far) = self.clip_planes(bounds);
        let view = look_at(self.eye(), self.target, self.up);
        let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
        let proj = match self.projection {
            Projection::Perspective => perspective(self.fov_y, aspect, near, far),
            Projection::Orthographic => {
                let h = self.half_height_at_target();
                orthographic(h * aspect, h, near, far)
            }
        };
        CameraMatrices { view, proj, near, far }
    }

    /// The ray through a pixel (`pixel` in the same units as `viewport`, origin at the
    /// top-left, pixel centers at +0.5).
    #[must_use]
    pub fn ray(&self, pixel: [f64; 2], viewport: [f64; 2]) -> Ray {
        let w = viewport[0].max(1.0);
        let h = viewport[1].max(1.0);
        let nx = 2.0 * pixel[0] / w - 1.0;
        let ny = 1.0 - 2.0 * pixel[1] / h;
        let hh = (self.fov_y * 0.5).tan();
        let hw = hh * w / h;
        let right = self.right();
        match self.projection {
            Projection::Perspective => {
                let dir = add(self.forward, add(scale(right, nx * hw), scale(self.up, ny * hh)));
                Ray { origin: self.eye(), dir: normalize_or(dir, self.forward) }
            }
            Projection::Orthographic => {
                let off = add(scale(right, nx * hw * self.distance), scale(self.up, ny * hh * self.distance));
                // Start well behind the eye so geometry between eye and target is hit.
                let origin = sub(add(self.eye(), off), scale(self.forward, self.distance * 1e3));
                Ray { origin, dir: self.forward }
            }
        }
    }

    /// Project a world point to pixel coordinates (top-left origin) and view depth.
    /// `None` if the point is behind the eye.
    #[must_use]
    pub fn project(&self, m: &CameraMatrices, p: Vec3, viewport: [f64; 2]) -> Option<([f64; 2], f64)> {
        let vp = math::transform_point(&m.view, p);
        let c = math::transform4(&m.proj, vp);
        if c[3] <= 1e-12 {
            return None;
        }
        let x = (c[0] / c[3] * 0.5 + 0.5) * viewport[0];
        let y = (0.5 - c[1] / c[3] * 0.5) * viewport[1];
        Some(([x, y], -vp[2]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Vec3, b: Vec3, eps: f64) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < eps)
    }

    #[test]
    fn named_views_are_orthonormal_and_x_right() {
        for v in NamedView::ALL {
            let mut c = Camera::default();
            c.set_view(*v);
            assert!(dot(c.forward, c.up).abs() < 1e-12, "{v:?}");
            assert!((math::length(c.forward) - 1.0).abs() < 1e-12);
            assert_eq!(NamedView::parse(v.name()), Some(*v));
        }
        let mut c = Camera::default();
        for v in [NamedView::Front, NamedView::Top, NamedView::Bottom] {
            c.set_view(v);
            assert!(near(c.right(), [1.0, 0.0, 0.0], 1e-12), "{v:?}");
        }
        c.set_view(NamedView::Iso);
        assert!(c.up[2] > 0.5);
    }

    #[test]
    fn orbit_keeps_orthonormal_and_full_turn_returns() {
        let mut c = Camera::default();
        let start = c;
        for _ in 0..360 {
            c.orbit(std::f64::consts::TAU / 360.0, 0.0);
        }
        assert!(near(c.forward, start.forward, 1e-9));
        c.orbit(0.0, 0.3);
        c.orbit(0.0, -0.3);
        assert!(near(c.forward, start.forward, 1e-9));
        assert!(dot(c.forward, c.up).abs() < 1e-12);
    }

    #[test]
    fn fit_contains_bounds_in_view() {
        let b = Aabb { min: [-10.0, -5.0, 0.0], max: [30.0, 5.0, 8.0] };
        for proj in [Projection::Perspective, Projection::Orthographic] {
            let mut c = Camera { projection: proj, ..Camera::default() };
            c.fit(&b, 16.0 / 9.0);
            let m = c.matrices(16.0 / 9.0, &b);
            for corner in b.corners() {
                let (px, depth) = c.project(&m, corner, [1600.0, 900.0]).expect("in front");
                assert!(px[0] > 0.0 && px[0] < 1600.0 && px[1] > 0.0 && px[1] < 900.0, "{proj:?} {px:?}");
                assert!(depth > m.near && depth < m.far);
            }
        }
    }

    #[test]
    fn ray_through_center_hits_target_and_projects_back() {
        for proj in [Projection::Perspective, Projection::Orthographic] {
            let mut c = Camera { projection: proj, target: [1.0, 2.0, 3.0], ..Camera::default() };
            c.orbit(0.4, 0.2);
            let vp = [800.0, 600.0];
            let r = c.ray([400.0, 300.0], vp);
            let to_t = sub(c.target, r.origin);
            let along = dot(to_t, r.dir);
            assert!(near(add(r.origin, scale(r.dir, along)), c.target, 1e-6), "{proj:?}");
            // A ray through an off-center pixel projects back to that pixel.
            let r = c.ray([123.0, 456.0], vp);
            let p = add(r.origin, scale(r.dir, along));
            let b = Aabb { min: [-5.0; 3], max: [5.0; 3] };
            let m = c.matrices(800.0 / 600.0, &b);
            let (px, _) = c.project(&m, p, vp).expect("visible");
            assert!((px[0] - 123.0).abs() < 1e-6 && (px[1] - 456.0).abs() < 1e-6, "{proj:?} {px:?}");
        }
    }

    #[test]
    fn pan_and_zoom_at_keep_cursor_point() {
        let mut c = Camera::default();
        let vp = [800.0, 600.0];
        let b = Aabb { min: [-50.0; 3], max: [50.0; 3] };
        // Point at target depth under pixel (600, 200).
        let world = |c: &Camera, px: [f64; 2]| {
            let r = c.ray(px, vp);
            add(r.origin, scale(r.dir, c.distance / dot(r.dir, c.forward)))
        };
        let p = world(&c, [600.0, 200.0]);
        c.zoom_at(2.0, [600.0, 200.0], vp);
        let m = c.matrices(800.0 / 600.0, &b);
        let (px, _) = c.project(&m, p, vp).expect("visible");
        assert!((px[0] - 600.0).abs() < 1e-6 && (px[1] - 200.0).abs() < 1e-6, "{px:?}");
        let p = world(&c, [400.0, 300.0]);
        c.pan_pixels(25.0, -10.0, vp[1]);
        let m = c.matrices(800.0 / 600.0, &b);
        let (px, _) = c.project(&m, p, vp).expect("visible");
        assert!((px[0] - 425.0).abs() < 1e-6 && (px[1] - 290.0).abs() < 1e-6, "{px:?}");
    }
}
