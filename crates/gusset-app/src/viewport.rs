//! The 3D viewport (ADR 0009, DESIGN.md §5).
//!
//! Bodies are drawn by `gusset-render`: on the GPU through an `egui_wgpu` paint callback
//! on eframe's own device (real depth buffer, hidden edges removed, MSAA by tier), or,
//! when eframe has no wgpu device, by the CPU [`SoftwareRenderer`] into an egui texture.
//! Either way a frame is rendered only when something it shows changed (camera, size,
//! model, highlights, theme); otherwise the last image is reused, so idle means idle.
//! Picking uses the scene's BVH ([`Scene::pick`]) and occlusion-aware edge picking.
//! Sketches and the axis triad are drawn on top with egui's painter.
//!
//! Orbit: middle/right drag. Pan: Shift + right drag. Zoom: scroll (to cursor). Pick: click.
//! The navigation bar's Orbit, Pan and Zoom tools make the left drag do the same.
//! The view cube (top right) and the navigation bar (bottom centre) are overlays drawn
//! last; camera moves they cause are animated (DESIGN.md §3 motion) unless the user chose
//! "reduce motion".

use crate::theme::{self, Palette, ThemeKind};
use crate::viewport_gpu::{self, GpuTarget};
use crate::viewport_nav::{self, CubeTarget, NavButton, NavMode, NavState};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};
use gusset_render::math::{Aabb, Vec3, cross, dot, normalize_or, rotate};
use gusset_render::{Camera, Highlight, MeshId, NamedView, Projection, Scene, SoftwareRenderer, Theme, ViewOptions};
use std::sync::Arc;

/// Names of one body's faces and edges, indexed like its mesh (`MeshId` = body index).
#[derive(Debug, Clone, Default)]
pub struct BodyInfo {
    pub name: String,
    pub face_names: Vec<String>,
    pub edge_names: Vec<String>,
}

/// A sketch drawn as polylines in 3D (already on its plane).
#[derive(Debug, Clone, Default)]
pub struct DrawSketch {
    pub name: String,
    pub polylines: Vec<Vec<[f32; 3]>>,
    pub points: Vec<[f32; 3]>,
}

/// What the cursor is over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    Face { body: String, index: u32, name: String },
    Edge { body: String, index: u32, name: String },
}

impl Hit {
    pub fn label(&self) -> String {
        match self {
            Hit::Face { body, name, .. } | Hit::Edge { body, name, .. } => format!("{body}: {name}"),
        }
    }

    fn body(&self) -> &str {
        match self {
            Hit::Face { body, .. } | Hit::Edge { body, .. } => body,
        }
    }
}

/// Result of one viewport frame.
#[derive(Debug, Default)]
pub struct Output {
    pub hover: Option<Hit>,
    pub clicked: Option<Option<Hit>>,
    /// World point under the cursor on the model, if any (for the status bar).
    pub cursor_world: Option<Vec3>,
}

/// Everything that determines the rendered image. A frame whose key equals the last
/// one reuses the last image.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrameKey {
    /// Scene generation: changes when the model was re-tessellated.
    pub generation: u64,
    /// Bumped on every highlight change.
    pub revision: u64,
    pub camera: Camera,
    pub size: [u32; 2],
    pub options: ViewOptions,
    pub interacting: bool,
}

/// A camera move the UI asked for (the view cube, the navigation bar, the app).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NavRequest {
    /// Look at the model from a standard view or direction (keeps target and distance).
    View(CubeTarget),
    /// Frame the whole model, keeping the orientation.
    Fit,
    /// The isometric view, framed.
    Home,
}

/// A camera move in progress.
#[derive(Debug, Clone, Copy)]
struct CameraAnim {
    from: Camera,
    to: Camera,
    /// Frame time at which it started (set on the first frame that sees it).
    start: Option<f64>,
}

/// The camera `t` (0..1) of the way from `a` to `b`: the view direction turns along the
/// shortest arc while the up vector follows, then the remaining roll is blended in.
pub(crate) fn blend(a: &Camera, b: &Camera, t: f64) -> Camera {
    let mix = |p: Vec3, q: Vec3| {
        let ([px, py, pz], [qx, qy, qz]) = (p, q);
        [px + (qx - px) * t, py + (qy - py) * t, pz + (qz - pz) * t]
    };
    let mut cam = *b;
    cam.target = mix(a.target, b.target);
    cam.distance = a.distance + (b.distance - a.distance) * t;
    let c = dot(a.forward, b.forward).clamp(-1.0, 1.0);
    let angle = c.acos();
    let axis = normalize_or(cross(a.forward, b.forward), a.up);
    let (f, u) = if angle < 1e-6 {
        (a.forward, a.up)
    } else {
        (rotate(a.forward, axis, angle * t), rotate(a.up, axis, angle * t))
    };
    // Roll left over once the direction arrived: the angle from the rotated up to b's up.
    let end_up = if angle < 1e-6 { a.up } else { rotate(a.up, axis, angle) };
    let roll = dot(cross(end_up, b.up), b.forward).atan2(dot(end_up, b.up));
    cam.forward = f;
    cam.up = rotate(u, f, roll * t);
    cam.orthonormalize();
    cam
}

/// CPU-rendered image and the key it was rendered for.
struct SoftwareImage {
    texture: egui::TextureHandle,
    key: FrameKey,
}

/// Viewport state kept across frames.
pub struct Viewport {
    pub camera: Camera,
    /// The renderable scene (shared with the GPU paint callback of the current frame).
    scene: Arc<Scene>,
    bodies: Vec<BodyInfo>,
    generation: u64,
    revision: u64,
    /// Highlights currently applied to `scene`: (hover, selection).
    applied: (Option<Hit>, Vec<Hit>),
    hover: Option<Hit>,
    /// Pointer position and frame key the hover was computed for.
    hover_for: Option<(Pos2, u64, u64, Camera)>,
    interacting: bool,
    pub fit_pending: bool,
    /// What a plain left drag does (the navigation bar's Orbit, Pan and Zoom tools).
    pub nav_mode: NavMode,
    /// Draw model edges (the navigation bar's view style toggle).
    pub show_edges: bool,
    /// Camera moves jump instead of animating.
    pub reduce_motion: bool,
    request: Option<NavRequest>,
    anim: Option<CameraAnim>,
    /// Bottom of the view cube in screen coordinates (windows stay below it).
    pub overlay_bottom: f32,
    gpu: Option<GpuTarget>,
    software: Option<SoftwareImage>,
    /// Number of CPU renders so far (tests check that idle frames render nothing).
    pub software_renders: u64,
}

impl std::fmt::Debug for Viewport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Viewport")
            .field("bodies", &self.bodies.len())
            .field("generation", &self.generation)
            .field("gpu", &self.gpu)
            .finish_non_exhaustive()
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            camera: Camera::default(),
            scene: Arc::new(Scene::new()),
            bodies: Vec::new(),
            generation: 0,
            revision: 0,
            applied: (None, Vec::new()),
            hover: None,
            hover_for: None,
            interacting: false,
            fit_pending: true,
            nav_mode: NavMode::Select,
            show_edges: true,
            reduce_motion: false,
            request: None,
            anim: None,
            overlay_bottom: 0.0,
            gpu: None,
            software: None,
            software_renders: 0,
        }
    }
}

impl Viewport {
    /// Uses eframe's wgpu device (`None`: no GPU, render on the CPU).
    pub fn set_gpu(&mut self, gpu: Option<GpuTarget>) {
        self.gpu = gpu;
    }

    /// Whether frames are rendered on the GPU.
    pub fn uses_gpu(&self) -> bool {
        self.gpu.is_some()
    }

    /// Replaces the drawn model (a scene built on the model thread).
    pub fn set_scene(&mut self, scene: Scene, bodies: Vec<BodyInfo>) {
        self.scene = Arc::new(scene);
        self.bodies = bodies;
        self.generation += 1;
        self.applied = (None, Vec::new());
        self.hover = None;
        self.hover_for = None;
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn bodies(&self) -> &[BodyInfo] {
        &self.bodies
    }

    fn mesh_of(&self, body: &str) -> Option<MeshId> {
        self.bodies.iter().position(|b| b.name == body).map(|i| MeshId(i as u64))
    }

    fn hit_for_face(&self, mesh: MeshId, face: u32) -> Option<Hit> {
        let b = self.bodies.get(usize::try_from(mesh.0).ok()?)?;
        let name = b.face_names.get(face as usize).cloned().unwrap_or_else(|| format!("face {face}"));
        Some(Hit::Face { body: b.name.clone(), index: face, name })
    }

    fn hit_for_edge(&self, mesh: MeshId, edge: u32) -> Option<Hit> {
        let b = self.bodies.get(usize::try_from(mesh.0).ok()?)?;
        let name = b.edge_names.get(edge as usize).cloned().unwrap_or_else(|| format!("edge {edge}"));
        Some(Hit::Edge { body: b.name.clone(), index: edge, name })
    }

    /// The face or edge under `p` (logical pixels relative to the viewport's top-left).
    /// Edges within 5 px win over faces unless a face hides them.
    pub fn pick(&self, size: Vec2, p: Pos2) -> Option<(Hit, Option<Vec3>)> {
        let vp = [f64::from(size.x), f64::from(size.y)];
        let px = [f64::from(p.x), f64::from(p.y)];
        if let Some(e) = self.scene.pick_edge(&self.camera, vp, px, 5.0)
            && let Some(h) = self.hit_for_edge(e.mesh, e.edge)
        {
            return Some((h, Some(e.point)));
        }
        let hit = self.scene.pick(&self.camera.ray(px, vp))?;
        Some((self.hit_for_face(hit.mesh, hit.face)?, Some(hit.point)))
    }

    /// Applies hover and selection highlights to the scene (only when they changed).
    fn apply_highlights(&mut self, selection: &[Hit]) {
        if self.applied.0 == self.hover && self.applied.1 == selection {
            return;
        }
        let mut targets: Vec<(MeshId, &Hit, Highlight)> = Vec::new();
        for h in selection {
            if let Some(m) = self.mesh_of(h.body()) {
                targets.push((m, h, Highlight::Select));
            }
        }
        if let Some(h) = &self.hover
            && !selection.contains(h)
            && let Some(m) = self.mesh_of(h.body())
        {
            targets.push((m, h, Highlight::Preselect));
        }
        let scene = Arc::make_mut(&mut self.scene);
        scene.clear_highlights(Highlight::None);
        for (m, h, kind) in targets {
            match h {
                Hit::Face { index, .. } => scene.set_face_highlight(m, *index, kind),
                Hit::Edge { index, .. } => scene.set_edge_highlight(m, *index, kind),
            };
        }
        self.applied = (self.hover.clone(), selection.to_vec());
        self.revision += 1;
    }

    /// Frames the model and the given sketches.
    fn fit(&mut self, sketches: &[&DrawSketch], aspect: f64) {
        if let Some(c) = self.fitted(&self.camera, sketches, aspect) {
            self.camera = c;
            self.fit_pending = false;
        }
    }

    /// `cam` framing the model and the given sketches (`None` when there is nothing).
    fn fitted(&self, cam: &Camera, sketches: &[&DrawSketch], aspect: f64) -> Option<Camera> {
        let mut b = self.scene.bounds();
        for s in sketches {
            for p in s.polylines.iter().flatten().chain(&s.points) {
                b.include(p.map(f64::from));
            }
        }
        if b.is_empty() {
            return None;
        }
        let mut c = *cam;
        c.fit(&b, aspect);
        Some(c)
    }

    /// Asks for a camera move; it is carried out (animated) on the next frame.
    pub fn request(&mut self, r: NavRequest) {
        self.request = Some(r);
    }

    /// Moves the camera to `to`: animated, or at once under "reduce motion".
    fn go_to(&mut self, to: Camera) {
        if self.reduce_motion || to == self.camera {
            self.camera = to;
            self.anim = None;
        } else {
            self.anim = Some(CameraAnim { from: self.camera, to, start: None });
        }
    }

    /// The camera looking from `t` (keeps target, distance and projection).
    fn camera_for(&self, t: CubeTarget) -> Camera {
        let mut c = self.camera;
        match t {
            CubeTarget::Named(v) => c.set_view(v),
            CubeTarget::Dir(d) => {
                c.forward = d.map(|v| -v);
                c.up = [0.0, 0.0, 1.0];
                c.orthonormalize();
            }
        }
        c
    }

    fn carry_out(&mut self, r: NavRequest, sketches: &[&DrawSketch], aspect: f64) {
        match r {
            NavRequest::View(t) => {
                let to = self.camera_for(t);
                self.go_to(to);
            }
            NavRequest::Fit => {
                if let Some(c) = self.fitted(&self.camera, sketches, aspect) {
                    self.go_to(c);
                }
            }
            NavRequest::Home => {
                let mut base = self.camera;
                base.set_view(NamedView::Iso);
                let to = self.fitted(&base, sketches, aspect).unwrap_or(base);
                self.go_to(to);
            }
        }
    }

    /// Advances a running camera move; true while one is running.
    fn step_anim(&mut self, ui: &egui::Ui) -> bool {
        let Some(a) = self.anim.as_mut() else { return false };
        let now = ui.input(|i| i.time);
        let start = *a.start.get_or_insert(now);
        let t = ((now - start) / f64::from(theme::MOTION_SLOW)).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        let (from, to) = (a.from, a.to);
        if t >= 1.0 {
            self.camera = to;
            self.anim = None;
        } else {
            self.camera = blend(&from, &to, eased);
            ui.ctx().request_repaint();
        }
        true
    }

    /// Bounds used for the near/far planes (the meshes; sketches lie within them or are
    /// drawn as overlays).
    fn clip_bounds(&self) -> Aabb {
        self.scene.bounds()
    }

    fn camera_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect) -> bool {
        let mut moved = false;
        let shift = ui.input(|i| i.modifiers.shift);
        let h = f64::from(rect.height());
        if self.nav_mode != NavMode::Select && resp.dragged_by(egui::PointerButton::Primary) {
            let d = resp.drag_delta();
            match self.nav_mode {
                NavMode::Orbit => self.camera.orbit_pixels(f64::from(d.x), f64::from(d.y), h),
                NavMode::Pan => self.camera.pan_pixels(f64::from(d.x), f64::from(d.y), h),
                NavMode::Zoom => self.camera.zoom((-f64::from(d.y) * 0.01).exp()),
                NavMode::Select => {}
            }
            moved = true;
        }
        if resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) {
            let d = resp.drag_delta();
            if d != Vec2::ZERO {
                if shift {
                    self.camera.pan_pixels(f64::from(d.x), f64::from(d.y), h);
                } else {
                    self.camera.orbit_pixels(f64::from(d.x), f64::from(d.y), h);
                }
            }
            moved = true;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(p) = resp.hover_pos() {
                    let rel = p - rect.min;
                    let factor = (f64::from(scroll) * 0.002).exp();
                    self.camera.zoom_at(factor, [f64::from(rel.x), f64::from(rel.y)], [f64::from(rect.width()), h]);
                }
                moved = true;
            }
        }
        moved
    }

    /// Paints the viewport and handles camera input and picking.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        pal: &Palette,
        theme: ThemeKind,
        sketches: &[&DrawSketch],
        selection: &[Hit],
    ) -> Output {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let mut out = Output::default();
        if rect.width() < 1.0 || rect.height() < 1.0 {
            return out;
        }
        let aspect = f64::from(rect.width()) / f64::from(rect.height());
        if self.fit_pending {
            self.fit(sketches, aspect);
        }
        if let Some(r) = self.request.take() {
            self.carry_out(r, sketches, aspect);
        }
        let animating = self.step_anim(ui);
        let was_interacting = self.interacting;
        self.interacting = self.camera_input(ui, &resp, rect) || animating;
        if self.interacting {
            // Refine the image once the view stops moving (the renderer never schedules
            // frames on its own).
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
        }

        // Hover: recomputed only when the pointer or the view changed.
        let pointer = resp.hover_pos().filter(|_| !self.interacting);
        match pointer {
            Some(p) => {
                let rel = (p - rect.min).to_pos2();
                let key = (rel, self.generation, rect.width().to_bits().into(), self.camera);
                if self.hover_for.as_ref() != Some(&key) {
                    let picked = self.pick(rect.size(), rel);
                    self.hover = picked.as_ref().map(|(h, _)| h.clone());
                    self.hover_for = Some(key);
                    out.cursor_world = picked.and_then(|(_, p)| p);
                }
            }
            None => {
                self.hover = None;
                self.hover_for = None;
            }
        }
        if was_interacting && !self.interacting {
            self.hover_for = None;
        }
        self.apply_highlights(selection);

        let ppp = ui.ctx().pixels_per_point();
        let size = [(rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32];
        let options = ViewOptions {
            theme: match theme {
                ThemeKind::Dark => Theme::Dark,
                ThemeKind::Light => Theme::Light,
            },
            pixels_per_point: ppp,
            edges: self.show_edges,
            ..ViewOptions::default()
        };
        let key = FrameKey {
            generation: self.generation,
            revision: self.revision,
            camera: self.camera,
            size,
            options,
            interacting: self.interacting,
        };
        let painter = ui.painter_at(rect);
        if self.scene.is_empty() {
            paint_background(&painter, rect, pal);
        } else if let Some(gpu) = &self.gpu {
            painter.add(viewport_gpu::callback(rect, self.scene.clone(), key, gpu.clone()));
        } else {
            self.paint_software(ui.ctx(), &painter, rect, key, pal);
        }

        let cm = self.camera.matrices(aspect, &self.clip_bounds());
        let vp = [f64::from(rect.width()), f64::from(rect.height())];
        let to_screen = |p: [f32; 3]| {
            self.camera.project(&cm, p.map(f64::from), vp).map(|(s, _)| rect.min + Vec2::new(s[0] as f32, s[1] as f32))
        };
        // Sketches on top of the model.
        for s in sketches {
            for pl in &s.polylines {
                let pts: Vec<Pos2> = pl.iter().filter_map(|&v| to_screen(v)).collect();
                painter.add(egui::Shape::line(pts, Stroke::new(1.6, pal.accent)));
            }
            for &p in &s.points {
                if let Some(q) = to_screen(p) {
                    painter.circle_filled(q, 2.5, pal.accent);
                }
            }
        }
        self.paint_triad(&painter, rect, pal);
        self.overlays(ui, rect, pal);

        if let Some(h) = &self.hover {
            resp.clone().on_hover_text_at_pointer(h.label());
        }
        if resp.clicked() && self.nav_mode == NavMode::Select {
            out.clicked = Some(self.hover.clone());
        }
        out.hover = self.hover.clone();
        out
    }

    /// The view cube and the navigation bar, and what they ask for.
    fn overlays(&mut self, ui: &mut egui::Ui, rect: Rect, pal: &Palette) {
        let cube = viewport_nav::view_cube(ui, rect, &self.camera, pal);
        self.overlay_bottom = cube.rect.bottom();
        if let Some(t) = cube.clicked {
            self.request = Some(NavRequest::View(t));
            ui.ctx().request_repaint();
        }
        if let Some(d) = cube.orbit {
            self.anim = None;
            self.camera.orbit_pixels(f64::from(d.x), f64::from(d.y), f64::from(rect.height()));
        }
        let state = NavState {
            mode: self.nav_mode,
            orthographic: self.camera.projection == Projection::Orthographic,
            edges: self.show_edges,
        };
        if let Some(b) = viewport_nav::nav_bar(ui, rect, state, pal) {
            self.nav_button(b);
            ui.ctx().request_repaint();
        }
    }

    /// A navigation bar button was pressed.
    pub fn nav_button(&mut self, b: NavButton) {
        let toggle = |cur: NavMode, to: NavMode| if cur == to { NavMode::Select } else { to };
        match b {
            NavButton::Orbit => self.nav_mode = toggle(self.nav_mode, NavMode::Orbit),
            NavButton::Pan => self.nav_mode = toggle(self.nav_mode, NavMode::Pan),
            NavButton::Zoom => self.nav_mode = toggle(self.nav_mode, NavMode::Zoom),
            NavButton::Fit => self.request = Some(NavRequest::Fit),
            NavButton::Home => self.request = Some(NavRequest::Home),
            NavButton::Projection => {
                self.camera.projection = match self.camera.projection {
                    Projection::Perspective => Projection::Orthographic,
                    Projection::Orthographic => Projection::Perspective,
                };
            }
            NavButton::Edges => self.show_edges = !self.show_edges,
        }
    }

    /// CPU path: re-rasterizes only when the frame key changed.
    fn paint_software(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        key: FrameKey,
        pal: &Palette,
    ) {
        let fresh = self.software.as_ref().is_some_and(|s| s.key == key);
        if !fresh {
            // Half resolution while the view moves keeps orbiting responsive.
            let scale = if key.interacting { 0.5 } else { 1.0 };
            let size = key.size.map(|s| ((s as f32 * scale).round() as u32).clamp(1, gusset_render::SOFTWARE_MAX_SIDE));
            let mut sw = SoftwareRenderer::new();
            sw.set_options(key.options);
            match sw.render_rgba(&self.scene, &key.camera, size) {
                Ok(img) => {
                    self.software_renders += 1;
                    let image =
                        egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data);
                    match &mut self.software {
                        Some(s) => {
                            s.texture.set(image, egui::TextureOptions::LINEAR);
                            s.key = key;
                        }
                        None => {
                            let texture = ctx.load_texture("gusset-viewport", image, egui::TextureOptions::LINEAR);
                            self.software = Some(SoftwareImage { texture, key });
                        }
                    }
                }
                Err(_) => {
                    paint_background(painter, rect, pal);
                    return;
                }
            }
        }
        if let Some(s) = &self.software {
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(s.texture.id(), rect, uv, Color32::WHITE);
        }
    }

    /// Axis triad (bottom-left), colored by convention with labels for color-blind users.
    fn paint_triad(&self, painter: &egui::Painter, rect: Rect, pal: &Palette) {
        let origin = rect.left_bottom() + Vec2::new(36.0, -36.0);
        let r = self.camera.right();
        let u = self.camera.up;
        for (axis, col, name) in [
            ([1.0, 0.0, 0.0], pal.danger, "X"),
            ([0.0, 1.0, 0.0], pal.success, "Y"),
            ([0.0, 0.0, 1.0], pal.accent, "Z"),
        ] {
            let d = Vec2::new(dot(axis, r) as f32, -dot(axis, u) as f32);
            let tip = origin + d * 22.0;
            painter.line_segment([origin, tip], Stroke::new(2.0, col));
            painter.text(
                origin + d * 31.0,
                egui::Align2::CENTER_CENTER,
                name,
                egui::FontId::proportional(11.0),
                pal.text,
            );
        }
    }
}

/// The viewport gradient (DESIGN.md `vp-bg-top` / `vp-bg-bottom`) for an empty scene.
fn paint_background(painter: &egui::Painter, rect: Rect, pal: &Palette) {
    let mut bg = egui::Mesh::default();
    bg.colored_vertex(rect.left_top(), pal.vp_bg_top);
    bg.colored_vertex(rect.right_top(), pal.vp_bg_top);
    bg.colored_vertex(rect.right_bottom(), pal.vp_bg_bottom);
    bg.colored_vertex(rect.left_bottom(), pal.vp_bg_bottom);
    bg.add_triangle(0, 1, 2);
    bg.add_triangle(0, 2, 3);
    painter.add(bg);
}

/// Builds the renderable scene for a set of tessellated bodies (`MeshId` = index).
/// Meshes the renderer rejects are skipped and reported.
pub fn build_scene(meshes: &[(BodyInfo, gusset_kernel::Mesh)]) -> (Scene, Vec<BodyInfo>, Vec<String>) {
    let mut scene = Scene::new();
    let mut bodies = Vec::with_capacity(meshes.len());
    let mut errors = Vec::new();
    for (info, mesh) in meshes {
        let id = MeshId(bodies.len() as u64);
        match scene.insert_mesh(id, mesh) {
            Ok(()) => bodies.push(info.clone()),
            Err(e) => errors.push(format!("{}: {e}", info.name)),
        }
    }
    (scene, bodies, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gusset_kernel::Mesh;

    fn quad() -> (BodyInfo, Mesh) {
        let mesh = Mesh {
            positions: vec![[-10.0, -10.0, 0.0], [10.0, -10.0, 0.0], [10.0, 10.0, 0.0], [-10.0, 10.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            face_ranges: std::iter::once(0..2).collect(),
            edges: vec![vec![[-10.0, -10.0, 0.0], [10.0, -10.0, 0.0]]],
        };
        (BodyInfo { name: "B".into(), face_names: vec!["top".into()], edge_names: vec!["e0".into()] }, mesh)
    }

    #[test]
    fn picks_faces_and_edges_through_the_bvh() {
        let (scene, bodies, errors) = build_scene(&[quad()]);
        assert!(errors.is_empty(), "{errors:?}");
        let mut v = Viewport::default();
        v.set_scene(scene, bodies);
        v.camera.set_view(gusset_render::NamedView::Top);
        let size = Vec2::new(400.0, 400.0);
        v.fit(&[], 1.0);
        let center = Pos2::new(200.0, 200.0);
        assert!(matches!(v.pick(size, center), Some((Hit::Face { ref name, .. }, _)) if name == "top"));
        assert_eq!(v.pick(size, Pos2::new(2.0, 2.0)), None);
        // The edge at y = -10 projects below the center in a top view.
        let cm = v.camera.matrices(1.0, &v.clip_bounds());
        let (p, _) = v.camera.project(&cm, [0.0, -10.0, 0.0], [400.0, 400.0]).unwrap();
        let hit = v.pick(size, Pos2::new(p[0] as f32, p[1] as f32 - 2.0));
        assert!(matches!(hit, Some((Hit::Edge { ref name, .. }, _)) if name == "e0"), "{hit:?}");
    }

    #[test]
    fn highlights_change_the_revision_only_when_needed() {
        let (scene, bodies, _) = build_scene(&[quad()]);
        let mut v = Viewport::default();
        v.set_scene(scene, bodies);
        let sel = vec![Hit::Face { body: "B".into(), index: 0, name: "top".into() }];
        v.apply_highlights(&sel);
        let r = v.revision;
        v.apply_highlights(&sel);
        assert_eq!(v.revision, r);
        assert_eq!(v.scene().mesh(MeshId(0)).map(|m| m.face_highlight(0)), Some(Highlight::Select));
        v.apply_highlights(&[]);
        assert_eq!(v.scene().mesh(MeshId(0)).map(|m| m.face_highlight(0)), Some(Highlight::None));
        assert!(v.revision > r);
    }
}
