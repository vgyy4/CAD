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
use gusset_render::{
    Camera, Highlight, MeshId, NamedView, Projection, Ray, Scene, Section, SoftwareRenderer, Theme, ViewOptions,
};
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
    /// Sketch entity id of each polyline (may be shorter: unnamed ones are not picked).
    pub ids: Vec<String>,
    /// Point id of each point (may be shorter).
    pub point_ids: Vec<String>,
    /// Constraint glyphs and dimension values, drawn while the sketch is edited.
    pub labels: Vec<([f32; 3], String)>,
    /// Drawn thick in the selection color (picked profile regions).
    pub emphasis: bool,
}

/// A construction plane (an origin plane or a datum plane), drawn as a translucent square
/// over the model.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawPlane {
    /// `XY`, `XZ`, `YZ` or the datum plane's name.
    pub name: String,
    pub origin: Vec3,
    /// Unit in-plane directions.
    pub x: Vec3,
    pub y: Vec3,
    /// Half the square's side.
    pub half: f64,
}

impl DrawPlane {
    fn corners(&self) -> [Vec3; 4] {
        let at = |a: f64, b: f64| {
            std::array::from_fn(|k| {
                self.origin.get(k).copied().unwrap_or(0.0)
                    + (self.x.get(k).copied().unwrap_or(0.0) * a + self.y.get(k).copied().unwrap_or(0.0) * b)
                        * self.half
            })
        };
        [at(-1.0, -1.0), at(1.0, -1.0), at(1.0, 1.0), at(-1.0, 1.0)]
    }

    /// Ray parameter where `ray` meets the square, if it does.
    fn hit(&self, ray: &Ray) -> Option<f64> {
        let n = cross(self.x, self.y);
        let denom = dot(ray.dir, n);
        if denom.abs() < 1e-12 {
            return None;
        }
        let d0: Vec3 = std::array::from_fn(|k| self.origin.get(k).zip(ray.origin.get(k)).map_or(0.0, |(o, r)| o - r));
        let t = dot(d0, n) / denom;
        let d: Vec3 = std::array::from_fn(|k| {
            ray.origin.get(k).zip(ray.dir.get(k)).map_or(0.0, |(o, r)| o + r * t)
                - self.origin.get(k).copied().unwrap_or(0.0)
        });
        let inside = dot(d, self.x).abs() <= self.half && dot(d, self.y).abs() <= self.half;
        (t >= 0.0 && inside).then_some(t)
    }
}

/// What the cursor is over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    Face {
        body: String,
        index: u32,
        name: String,
    },
    Edge {
        body: String,
        index: u32,
        name: String,
    },
    /// An origin or datum plane.
    Plane {
        name: String,
    },
    /// A point or curve of the sketch being edited (its id, e.g. `p3`, `l2`).
    Sketch {
        sketch: String,
        id: String,
    },
}

impl Hit {
    pub fn label(&self) -> String {
        match self {
            Hit::Face { body, name, .. } | Hit::Edge { body, name, .. } => format!("{body}: {name}"),
            Hit::Plane { name } => format!("{name} plane"),
            Hit::Sketch { sketch, id } => format!("{sketch}: {id}"),
        }
    }

    /// The body the hit is on (empty for planes).
    pub fn body(&self) -> &str {
        match self {
            Hit::Face { body, .. } | Hit::Edge { body, .. } => body,
            Hit::Plane { .. } | Hit::Sketch { .. } => "",
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
    /// The camera ray under the pointer (sketch placement).
    pub pointer_ray: Option<Ray>,
    /// A sketch point being dragged (left drag from a point of the edited sketch).
    pub drag: Option<PointDrag>,
    /// A right click without a drag: where to open the marking menu.
    pub menu_at: Option<Pos2>,
    /// World size of one logical pixel at the view's target (snapping tolerances).
    pub pixel_size: f64,
}

/// A sketch point dragged in the viewport: where the pointer is, and whether the drag
/// ended this frame.
#[derive(Debug, Clone)]
pub struct PointDrag {
    pub sketch: String,
    pub point: String,
    pub ray: Ray,
    pub done: bool,
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

/// What a click in the viewport may pick (the ribbon's SELECT group).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectFilter {
    /// Edges near the pointer first, else the face under it.
    #[default]
    All,
    Faces,
    Edges,
}

impl SelectFilter {
    pub const ALL: [SelectFilter; 3] = [SelectFilter::All, SelectFilter::Faces, SelectFilter::Edges];

    /// The i18n key of its name.
    pub fn key(self) -> &'static str {
        match self {
            SelectFilter::All => "select.all",
            SelectFilter::Faces => "select.faces",
            SelectFilter::Edges => "select.edges",
        }
    }
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
    /// Look straight at a plane (from its normal's side), `up` pointing up on screen
    /// (sketch editing: the sketch's plane and its Y axis).
    LookAt { normal: Vec3, up: Vec3 },
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

/// Mouse navigation presets (DESIGN.md §1.16): which button and modifier orbit, pan and
/// zoom with a drag. The wheel zooms at the pointer in every preset, and a plain left drag
/// stays selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavPreset {
    /// Right or middle drag orbits, with Shift pans.
    #[default]
    Gusset,
    /// Middle drag pans, Shift + middle orbits.
    Fusion,
    /// Middle drag orbits, Ctrl + middle pans, Shift + middle zooms.
    SolidWorks,
    /// FreeCAD's gesture style: right drag orbits, Ctrl + right pans.
    FreeCad,
    /// Right drag orbits, middle drag (or Ctrl + right) pans.
    Onshape,
}

/// What a drag does to the camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragAction {
    Orbit,
    Pan,
    Zoom,
}

impl NavPreset {
    pub const ALL: [NavPreset; 5] =
        [NavPreset::Gusset, NavPreset::Fusion, NavPreset::SolidWorks, NavPreset::FreeCad, NavPreset::Onshape];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            NavPreset::Gusset => "Gusset",
            NavPreset::Fusion => "Fusion",
            NavPreset::SolidWorks => "SolidWorks",
            NavPreset::FreeCad => "FreeCAD",
            NavPreset::Onshape => "Onshape",
        }
    }

    #[must_use]
    pub fn from_name(s: &str) -> Option<NavPreset> {
        NavPreset::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(s))
    }

    /// The camera action of a right or middle drag with these modifiers.
    #[must_use]
    pub fn drag(self, button: egui::PointerButton, m: egui::Modifiers) -> Option<DragAction> {
        use DragAction::{Orbit, Pan, Zoom};
        use egui::PointerButton::{Middle, Secondary};
        Some(match (self, button) {
            (NavPreset::Gusset, Middle | Secondary) => {
                if m.shift {
                    Pan
                } else {
                    Orbit
                }
            }
            (NavPreset::Fusion, Middle) => {
                if m.shift {
                    Orbit
                } else {
                    Pan
                }
            }
            (NavPreset::SolidWorks, Middle) => {
                if m.command {
                    Pan
                } else if m.shift {
                    Zoom
                } else {
                    Orbit
                }
            }
            (NavPreset::FreeCad | NavPreset::Onshape, Secondary) => {
                if m.command {
                    Pan
                } else {
                    Orbit
                }
            }
            (NavPreset::Onshape, Middle) => Pan,
            _ => return None,
        })
    }
}

/// A display-only section view: a plane across one world axis through the model's box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionView {
    /// World axis the plane is perpendicular to: 0 = X, 1 = Y, 2 = Z.
    pub axis: usize,
    /// Where the plane sits across the model's box, 0..=1.
    pub at: f32,
    /// Keep the other half: by default the half towards -axis is removed, which shows the
    /// cut from the front and the default isometric view.
    pub flip: bool,
}

impl Default for SectionView {
    fn default() -> Self {
        SectionView { axis: 1, at: 0.5, flip: false }
    }
}

impl SectionView {
    /// The world plane for a model box; `None` when the box is empty.
    #[must_use]
    pub fn plane(&self, bounds: &Aabb) -> Option<Section> {
        if bounds.is_empty() {
            return None;
        }
        let k = self.axis.min(2);
        let mut origin = bounds.center();
        let (lo, hi) = (bounds.min.get(k).copied()?, bounds.max.get(k).copied()?);
        *origin.get_mut(k)? = lo + (hi - lo) * f64::from(self.at.clamp(0.0, 1.0));
        let mut normal = [0.0; 3];
        *normal.get_mut(k)? = if self.flip { 1.0 } else { -1.0 };
        Some(Section { origin, normal })
    }
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
    /// Which right and middle drags orbit, pan and zoom.
    pub nav_preset: NavPreset,
    /// Draw model edges (the navigation bar's view style toggle).
    pub show_edges: bool,
    /// Wireframe style: edges only, hidden edges included (the navigation bar's toggle).
    pub wireframe: bool,
    /// What clicks pick (the ribbon's SELECT group).
    pub select_filter: SelectFilter,
    /// Section view (the navigation bar's toggle), if on.
    pub section: Option<SectionView>,
    /// Origin and datum planes (set by the app each frame); hidden ones are skipped.
    pub planes: Vec<DrawPlane>,
    /// The sketch being edited: its points and curves are picked before the model.
    pub edit_sketch: Option<String>,
    /// Where the viewport was drawn last frame (screen coordinates).
    last_rect: Rect,
    /// The model's scene and bodies while a live preview is shown in their place.
    stash: Option<(Arc<Scene>, Vec<BodyInfo>)>,
    /// The sketch point a left drag started on: (sketch, point id).
    dragging: Option<(String, String)>,
    /// The last pointer ray of the running drag.
    drag_ray: Option<Ray>,
    /// The hover when the button went down (a drag is reported only after the pointer
    /// moved off what it was pressed on).
    press_hover: Option<Hit>,
    /// Planes the user hid; the origin planes start hidden.
    hidden_planes: std::collections::BTreeSet<String>,
    /// Bodies the user hid (the browser's eye); kept across rebuilds by name.
    hidden_bodies: std::collections::BTreeSet<String>,
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
            nav_preset: NavPreset::Gusset,
            show_edges: true,
            wireframe: false,
            hidden_bodies: std::collections::BTreeSet::new(),
            select_filter: SelectFilter::All,
            section: None,
            planes: Vec::new(),
            edit_sketch: None,
            last_rect: Rect::NOTHING,
            stash: None,
            dragging: None,
            drag_ray: None,
            press_hover: None,
            hidden_planes: ["XY", "XZ", "YZ"].into_iter().map(String::from).collect(),
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
    pub fn set_scene(&mut self, mut scene: Scene, bodies: Vec<BodyInfo>) {
        for (i, b) in bodies.iter().enumerate() {
            if self.hidden_bodies.contains(&b.name) {
                scene.set_visible(MeshId(i as u64), false);
            }
        }
        // A new model replaces any preview (and the model it stood in for).
        self.stash = None;
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

    /// Shows a live preview in place of the model (kept aside until [`Self::end_preview`]).
    pub fn show_preview(&mut self, scene: Scene, bodies: Vec<BodyInfo>) {
        let stash = self.stash.take().unwrap_or_else(|| (self.scene.clone(), std::mem::take(&mut self.bodies)));
        self.set_scene(scene, bodies);
        self.stash = Some(stash);
    }

    /// Puts the model back after a preview.
    pub fn end_preview(&mut self) {
        if let Some((scene, bodies)) = self.stash.take() {
            self.scene = scene;
            self.bodies = bodies;
            self.generation += 1;
            self.applied = (None, Vec::new());
            self.hover = None;
            self.hover_for = None;
        }
    }

    /// Whether a live preview is shown.
    #[must_use]
    pub fn previewing(&self) -> bool {
        self.stash.is_some()
    }

    /// Whether a body is drawn (the browser's eye). Hidden bodies are not drawn or picked.
    pub fn body_visible(&self, name: &str) -> bool {
        !self.hidden_bodies.contains(name)
    }

    /// Shows or hides a body; the choice survives rebuilds of the model.
    pub fn set_body_visible(&mut self, name: &str, visible: bool) {
        if visible {
            self.hidden_bodies.remove(name);
        } else {
            self.hidden_bodies.insert(name.to_string());
        }
        if let Some(id) = self.mesh_of(name) {
            Arc::make_mut(&mut self.scene).set_visible(id, visible);
            self.revision += 1;
        }
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
        let section = self.section_plane();
        if self.select_filter != SelectFilter::Faces
            && let Some(e) = self.scene.pick_edge_section(&self.camera, vp, px, 5.0, section.as_ref())
            && let Some(h) = self.hit_for_edge(e.mesh, e.edge)
        {
            return Some((h, Some(e.point)));
        }
        if self.select_filter == SelectFilter::Edges {
            return None;
        }
        let ray = self.camera.ray(px, vp);
        if let Some(hit) = self.scene.pick_section(&ray, section.as_ref()) {
            return Some((self.hit_for_face(hit.mesh, hit.face)?, Some(hit.point)));
        }
        // Planes are see-through: they pick only where no body is under the cursor.
        let (t, p) =
            self.visible_planes().filter_map(|p| p.hit(&ray).map(|t| (t, p))).min_by(|a, b| a.0.total_cmp(&b.0))?;
        let point = std::array::from_fn(|k| ray.origin.get(k).zip(ray.dir.get(k)).map_or(0.0, |(o, d)| o + d * t));
        Some((Hit::Plane { name: p.name.clone() }, Some(point)))
    }

    /// A left drag from a point of the edited sketch moves that point (select mode).
    fn drag_point(&mut self, resp: &egui::Response, rect: Rect, sketches: &[&DrawSketch], out: &mut Output) {
        if self.nav_mode != NavMode::Select {
            self.dragging = None;
            return;
        }
        if !resp.is_pointer_button_down_on() {
            self.press_hover = self.hover.clone();
        }
        if resp.drag_started_by(egui::PointerButton::Primary)
            && let Some(Hit::Sketch { sketch, id }) = &self.press_hover
            && sketches.iter().any(|s| s.name == *sketch && s.point_ids.contains(id))
        {
            self.dragging = Some((sketch.clone(), id.clone()));
        }
        let Some((sketch, point)) = self.dragging.clone() else { return };
        let done = resp.drag_stopped() || !resp.dragged();
        if let Some(p) = resp.interact_pointer_pos().or(resp.hover_pos()) {
            let rel = p - rect.min;
            self.drag_ray = Some(
                self.camera
                    .ray([f64::from(rel.x), f64::from(rel.y)], [f64::from(rect.width()), f64::from(rect.height())]),
            );
        }
        // Released off the viewport (no pointer position): finish at the last position,
        // so the preview ends and the move is made (review R5).
        if let Some(ray) = self.drag_ray {
            out.drag = Some(PointDrag { sketch, point, ray, done });
        }
        if done {
            self.dragging = None;
            self.drag_ray = None;
        }
    }

    /// Where the viewport was drawn last frame.
    #[must_use]
    pub fn rect(&self) -> Rect {
        self.last_rect
    }

    /// Where a world point appears on screen in the last drawn frame.
    #[must_use]
    pub fn screen_pos(&self, world: Vec3) -> Option<Pos2> {
        let r = self.last_rect;
        let aspect = f64::from(r.width()) / f64::from(r.height().max(1.0));
        let cm = self.camera.matrices(aspect, &self.clip_bounds());
        let (s, _) = self.camera.project(&cm, world, [f64::from(r.width()), f64::from(r.height())])?;
        Some(r.min + Vec2::new(s[0] as f32, s[1] as f32))
    }

    /// The point (within 6 px, preferred) or curve (within 5 px) of a sketch under `p`.
    pub fn pick_sketch(&self, s: &DrawSketch, size: Vec2, p: Pos2) -> Option<Hit> {
        let aspect = f64::from(size.x) / f64::from(size.y.max(1.0));
        let cm = self.camera.matrices(aspect, &self.clip_bounds());
        let vp = [f64::from(size.x), f64::from(size.y)];
        let at = |q: &[f32; 3]| {
            self.camera.project(&cm, q.map(f64::from), vp).map(|(s, _)| Pos2::new(s[0] as f32, s[1] as f32))
        };
        let hit = |id: &String| Hit::Sketch { sketch: s.name.clone(), id: id.clone() };
        let point = s
            .points
            .iter()
            .zip(&s.point_ids)
            .filter_map(|(q, id)| Some((at(q)?.distance(p), id)))
            .filter(|(d, _)| *d <= 6.0)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, id)) = point {
            return Some(hit(id));
        }
        let seg = |a: Pos2, b: Pos2| {
            let ab = b - a;
            let t = if ab.length_sq() > 1e-9 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
            (a + ab * t).distance(p)
        };
        s.polylines
            .iter()
            .zip(&s.ids)
            .filter_map(|(pl, id)| {
                let pts: Vec<Pos2> = pl.iter().filter_map(at).collect();
                let d = pts.windows(2).filter_map(|w| Some(seg(*w.first()?, *w.get(1)?))).fold(f32::INFINITY, f32::min);
                (d <= 5.0).then_some((d, id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| hit(id))
    }

    /// Whether a plane is drawn and picked.
    #[must_use]
    pub fn plane_visible(&self, name: &str) -> bool {
        !self.hidden_planes.contains(name)
    }

    /// Shows or hides a plane (kept across rebuilds by name).
    pub fn set_plane_visible(&mut self, name: &str, on: bool) {
        if on {
            self.hidden_planes.remove(name);
        } else {
            self.hidden_planes.insert(name.to_string());
        }
        self.hover_for = None;
    }

    fn visible_planes(&self) -> impl Iterator<Item = &DrawPlane> {
        self.planes.iter().filter(|p| self.plane_visible(&p.name))
    }

    /// The section plane in world space, when the section view is on.
    #[must_use]
    pub fn section_plane(&self) -> Option<Section> {
        self.section.and_then(|s| s.plane(&self.scene.bounds()))
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
                Hit::Plane { .. } | Hit::Sketch { .. } => false,
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
        // The click-placement preview (no name) follows the cursor: not part of the model.
        for s in sketches.iter().filter(|s| !s.name.is_empty()) {
            for p in s.polylines.iter().flatten().chain(&s.points) {
                b.include(p.map(f64::from));
            }
        }
        if b.is_empty() {
            return None;
        }
        if b.radius() < 1e-3 {
            // A single point: frame 20 mm around it rather than collapsing onto it.
            let c = b.center();
            b.include(c.map(|v| v - 10.0));
            b.include(c.map(|v| v + 10.0));
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
            NavRequest::LookAt { normal, up } => {
                let mut c = self.camera;
                c.forward = normal.map(|v| -v);
                c.up = up;
                c.orthonormalize();
                self.go_to(c);
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
        let mods = ui.input(|i| i.modifiers);
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
        for button in [egui::PointerButton::Middle, egui::PointerButton::Secondary] {
            let Some(action) = resp.dragged_by(button).then(|| self.nav_preset.drag(button, mods)).flatten() else {
                continue;
            };
            let d = resp.drag_delta();
            if d != Vec2::ZERO {
                let (dx, dy) = (f64::from(d.x), f64::from(d.y));
                match action {
                    DragAction::Orbit => self.camera.orbit_pixels(dx, dy, h),
                    DragAction::Pan => self.camera.pan_pixels(dx, dy, h),
                    DragAction::Zoom => self.camera.zoom((-dy * 0.01).exp()),
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
        self.last_rect = rect;
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
        out.pixel_size = self.camera.pixel_size_at_target(f64::from(rect.height()));
        match pointer {
            Some(p) => {
                let rel = (p - rect.min).to_pos2();
                let vp = [f64::from(rect.width()), f64::from(rect.height())];
                out.pointer_ray = Some(self.camera.ray([f64::from(rel.x), f64::from(rel.y)], vp));
                let key = (rel, self.generation, rect.width().to_bits().into(), self.camera);
                if self.hover_for.as_ref() != Some(&key) {
                    let edited = sketches.iter().find(|s| self.edit_sketch.as_deref() == Some(s.name.as_str()));
                    let picked = match edited.and_then(|s| self.pick_sketch(s, rect.size(), rel)) {
                        Some(h) => Some((h, None)),
                        None => self.pick(rect.size(), rel),
                    };
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
        self.drag_point(&resp, rect, sketches, &mut out);
        if resp.secondary_clicked() {
            out.menu_at = resp.interact_pointer_pos();
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
            edges: self.show_edges || self.wireframe,
            faces: !self.wireframe,
            section: self.section_plane(),
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
        // Construction planes: translucent squares over the model, with their names.
        for p in self.visible_planes() {
            let pts: Vec<Pos2> = p.corners().iter().filter_map(|c| to_screen(c.map(|v| v as f32))).collect();
            let [_, _, _, label_at] = pts.as_slice() else { continue };
            let hit = Hit::Plane { name: p.name.clone() };
            let color = if selection.contains(&hit) {
                pal.vp_select
            } else if self.hover.as_ref() == Some(&hit) {
                pal.vp_preselect
            } else {
                pal.accent
            };
            let label_at = *label_at;
            painter.add(egui::Shape::convex_polygon(
                pts,
                color.gamma_multiply(0.06),
                Stroke::new(1.0, color.gamma_multiply(0.7)),
            ));
            painter.text(
                label_at + Vec2::new(4.0, 2.0),
                egui::Align2::LEFT_TOP,
                &p.name,
                egui::FontId::proportional(11.0),
                color,
            );
        }
        // Sketches on top of the model; the edited sketch's picked items highlighted.
        let shade = |s: &DrawSketch, id: Option<&String>| {
            let Some(id) = id else { return pal.accent };
            let h = Hit::Sketch { sketch: s.name.clone(), id: id.clone() };
            if selection.contains(&h) {
                pal.vp_select
            } else if self.hover.as_ref() == Some(&h) {
                pal.vp_preselect
            } else {
                pal.accent
            }
        };
        for s in sketches {
            if self.edit_sketch.as_deref() == Some(s.name.as_str()) {
                // Labels on a pill; several at one anchor stack downwards.
                let mut used: Vec<Pos2> = Vec::new();
                for (p, text) in &s.labels {
                    let Some(q) = to_screen(*p) else { continue };
                    let n = used.iter().filter(|u| u.distance(q) < 1.0).count();
                    used.push(q);
                    let g = painter.layout_no_wrap(text.clone(), egui::FontId::proportional(11.0), pal.accent);
                    let at = q + Vec2::new(6.0, 4.0 + n as f32 * (g.size().y + 4.0));
                    let r = Rect::from_min_size(at, g.size()).expand2(Vec2::new(3.0, 1.0));
                    painter.rect_filled(r, 3.0, pal.bg_raised.gamma_multiply(0.9));
                    painter.galley(at, g, pal.accent);
                }
            }
            for (i, pl) in s.polylines.iter().enumerate() {
                let pts: Vec<Pos2> = pl.iter().filter_map(|&v| to_screen(v)).collect();
                let c = if s.emphasis { pal.vp_select } else { shade(s, s.ids.get(i)) };
                let w = if c == pal.accent { 1.6 } else { 2.6 };
                painter.add(egui::Shape::line(pts, Stroke::new(w, c)));
            }
            for (i, &p) in s.points.iter().enumerate() {
                if let Some(q) = to_screen(p) {
                    let c = shade(s, s.point_ids.get(i));
                    painter.circle_filled(q, if c == pal.accent { 2.5 } else { 4.0 }, c);
                }
            }
        }
        self.paint_triad(&painter, rect, pal);
        if self.previewing() {
            let g = painter.layout_no_wrap(
                crate::i18n::tr("viewport.preview").into(),
                egui::FontId::proportional(12.0),
                pal.accent,
            );
            let at = rect.left_top() + Vec2::new(12.0, 12.0);
            painter.rect_filled(Rect::from_min_size(at, g.size()).expand2(Vec2::new(6.0, 3.0)), 4.0, pal.bg_raised);
            painter.galley(at, g, pal.accent);
        }
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
            wireframe: self.wireframe,
            section: self.section.is_some(),
        };
        if let Some(b) = viewport_nav::nav_bar(ui, rect, state, pal) {
            self.nav_button(b);
            ui.ctx().request_repaint();
        }
        if let Some(s) = &mut self.section
            && viewport_nav::section_bar(ui, rect, s, pal)
        {
            self.hover_for = None;
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
            NavButton::Wireframe => self.wireframe = !self.wireframe,
            NavButton::Section => {
                self.section = if self.section.is_some() { None } else { Some(SectionView::default()) };
                self.hover_for = None;
            }
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
        let near_edge = Pos2::new(p[0] as f32, p[1] as f32 - 2.0);
        let hit = v.pick(size, near_edge);
        assert!(matches!(hit, Some((Hit::Edge { ref name, .. }, _)) if name == "e0"), "{hit:?}");
        // The SELECT filter: faces only picks the face next to the edge, edges only picks
        // nothing in the middle of a face.
        v.select_filter = SelectFilter::Faces;
        assert!(matches!(v.pick(size, near_edge), Some((Hit::Face { ref name, .. }, _)) if name == "top"));
        v.select_filter = SelectFilter::Edges;
        assert_eq!(v.pick(size, center), None);
        assert!(matches!(v.pick(size, near_edge), Some((Hit::Edge { .. }, _))));
        // A section keeping y >= 5 (three quarters across the quad): the center and the
        // edge at y = -10 are cut away, the face beyond the plane still picks.
        v.select_filter = SelectFilter::All;
        v.section = Some(SectionView { axis: 1, at: 0.75, flip: false });
        assert_eq!(v.pick(size, center), None);
        assert_eq!(v.pick(size, near_edge), None);
        let (p, _) = v.camera.project(&cm, [0.0, 8.0, 0.0], [400.0, 400.0]).unwrap();
        let kept = Pos2::new(p[0] as f32, p[1] as f32);
        assert!(matches!(v.pick(size, kept), Some((Hit::Face { ref name, .. }, _)) if name == "top"));
        v.section = Some(SectionView { axis: 1, at: 0.75, flip: true });
        assert!(v.pick(size, center).is_some());
    }

    #[test]
    fn navigation_presets_map_drags() {
        use egui::PointerButton::{Middle, Primary, Secondary};
        let none = egui::Modifiers::NONE;
        let shift = egui::Modifiers::SHIFT;
        let ctrl = egui::Modifiers::COMMAND;
        assert_eq!(NavPreset::Gusset.drag(Secondary, none), Some(DragAction::Orbit));
        assert_eq!(NavPreset::Gusset.drag(Middle, shift), Some(DragAction::Pan));
        assert_eq!(NavPreset::Fusion.drag(Middle, none), Some(DragAction::Pan));
        assert_eq!(NavPreset::Fusion.drag(Middle, shift), Some(DragAction::Orbit));
        assert_eq!(NavPreset::Fusion.drag(Secondary, none), None);
        assert_eq!(NavPreset::SolidWorks.drag(Middle, ctrl), Some(DragAction::Pan));
        assert_eq!(NavPreset::SolidWorks.drag(Middle, shift), Some(DragAction::Zoom));
        assert_eq!(NavPreset::Onshape.drag(Middle, none), Some(DragAction::Pan));
        assert_eq!(NavPreset::FreeCad.drag(Secondary, ctrl), Some(DragAction::Pan));
        // A left drag is never navigation (it selects).
        assert!(NavPreset::ALL.iter().all(|p| p.drag(Primary, none).is_none()));
        assert_eq!(NavPreset::from_name("solidworks"), Some(NavPreset::SolidWorks));
    }

    #[test]
    fn planes_pick_where_no_body_is() {
        let (scene, bodies, _) = build_scene(&[quad()]);
        let mut v = Viewport::default();
        v.set_scene(scene, bodies);
        v.camera.set_view(gusset_render::NamedView::Top);
        let size = Vec2::new(400.0, 400.0);
        v.fit(&[], 1.0);
        let xy = DrawPlane { name: "XY".into(), origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], half: 50.0 };
        v.planes = vec![xy];
        let corner = Pos2::new(2.0, 2.0);
        // Origin planes start hidden.
        assert_eq!(v.pick(size, corner), None);
        v.set_plane_visible("XY", true);
        let hit = v.pick(size, corner);
        assert!(
            matches!(hit, Some((Hit::Plane { ref name }, Some(p))) if name == "XY" && p[2].abs() < 1e-6),
            "{hit:?}"
        );
        // The body in front still wins.
        assert!(matches!(v.pick(size, Pos2::new(200.0, 200.0)), Some((Hit::Face { .. }, _))));
        // Edge-on planes are not hit.
        v.planes[0].y = [0.0, 0.0, 1.0];
        assert_eq!(v.pick(size, corner), None);
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
