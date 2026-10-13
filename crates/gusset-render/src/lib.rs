//! Gusset's viewport renderer (ADR 0009, DESIGN.md §5).
//!
//! * [`Renderer`]: wgpu renderer on a caller-provided device (shared with egui); draws
//!   into an offscreen texture view with [`Renderer::render_to`]. No render loop: it
//!   only works when called ("idle means idle").
//! * [`HeadlessRenderer`]: owns a device picked by the fallback chain (hardware →
//!   `force_fallback_adapter` → GL) and returns RGBA images / PNGs, plus the
//!   [`AdapterReport`] for diagnostics.
//! * [`SoftwareRenderer`]: CPU rasterizer used only when no adapter exists at all;
//!   [`ViewRenderer`] picks between the two so `render_view` always returns a PNG.
//! * [`Scene`]: meshes by id with transforms, materials, visibility and per-face /
//!   per-edge highlights, plus CPU picking ([`Scene::pick`], [`Scene::pick_edge`]).
//! * [`Camera`]: perspective/orthographic orbit camera with named views and fit.
//! * [`image`]: PNG encode/decode and a tolerant image diff for screenshot tests.
//!
//! The GPU path uses only WebGL2-class downlevel limits, no optional features, no
//! compute and no storage buffers, so it runs on old integrated GPUs, WARP, lavapipe and
//! llvmpipe.
//!
//! ```
//! use gusset_render::{Camera, MeshId, NamedView, Scene, SoftwareRenderer, demo};
//! let mut scene = Scene::new();
//! scene.insert_mesh(MeshId(1), &demo::box_mesh([10.0, 10.0, 10.0])).unwrap();
//! let mut camera = Camera::default();
//! camera.set_view(NamedView::Iso);
//! camera.fit(&scene.bounds(), 4.0 / 3.0);
//! let png = SoftwareRenderer::new().render_png(&scene, &camera, [80, 60]).unwrap();
//! assert!(png.starts_with(b"\x89PNG"));
//! ```
// Display-only code: platform float functions are fine here (PROMPT.md Determinism covers
// model-defining code).
#![allow(clippy::disallowed_methods)]

mod camera;
pub mod demo;
mod frame;
mod gpu;
mod headless;
pub mod image;
pub mod math;
mod mesh;
mod pick;
mod scene;
mod software;
mod theme;
mod view;

pub use camera::{Camera, CameraMatrices, NamedView, Projection, Ray};
pub use frame::ViewOptions;
pub use gpu::{FrameStats, Quality, Renderer};
pub use headless::{
    AdapterPreference, AdapterReport, HeadlessRenderer, RenderError, classify_adapter, required_limits, tier_for,
};
pub use image::{DiffReport, DiffTolerance, RgbaImage};
pub use mesh::{Bvh, LocalHit, MeshData, MeshError, pack_normal, unpack_normal};
pub use pick::{EdgeHit, PickHit};
pub use scene::{Highlight, Material, MeshId, Scene, SceneMesh};
pub use software::{SOFTWARE_MAX_SIDE, SoftwareRenderer};
pub use theme::{DEFAULT_PART_COLOR, Lighting, Srgb, Theme, ViewportColors, linear_to_srgb, srgb_to_linear};
pub use view::ViewRenderer;
/// Re-exported so callers use the same wgpu version.
pub use wgpu;
