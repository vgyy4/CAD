# gusset-render

The Gusset viewport renderer (ADR 0009, DESIGN.md §3/§5): wgpu 30, WGSL, downlevel
(WebGL2-class) limits only, no optional features, no compute, no storage buffers.

## API
- `Renderer::new(&device, &queue, format)` + `render_to(&scene, &camera, &view, [w, h])`:
  draws into an offscreen texture on a device shared with egui. No render loop; work
  happens only inside `render_to` (idle means idle). `set_quality_tier(tier)`,
  `set_interacting(bool)` (1x MSAA + the tier's reduced render scale, upscaled with a
  blit), `set_options(ViewOptions { theme, pixels_per_point, edges, edge_width, lighting })`.
- `HeadlessRenderer::new()`: adapter chain hardware (Vulkan/DX12/Metal) →
  `force_fallback_adapter` (WARP/lavapipe) → GL (llvmpipe via EGL). `adapter()` returns an
  `AdapterReport`; `gpu_diagnostics(override)` fills `gusset_hw::GpuDiagnostics`;
  `render_rgba` / `render_png`. The tier comes from `gusset_hw::tier::auto_tier`
  (`classify_adapter` / `tier_for`).
- `SoftwareRenderer`: CPU rasterizer (z-buffer, near clipping, same lighting and colors,
  lines) for the no-adapter case. `ViewRenderer` picks GPU or CPU so `render_view` always
  returns a PNG.
- `Scene`: `insert_mesh(id, &gusset_kernel::Mesh)`, `remove_mesh`, `set_transform`,
  `set_material`, `set_visible`, `set_face_highlight`, `set_edge_highlight`,
  `clear_highlights`, `bounds`, `pick(&ray)` (per-mesh triangle BVH) and
  `pick_edge(&camera, viewport, pixel, max_px)` (screen-space distance, occlusion-aware).
- `Camera`: perspective/orthographic, `orbit`, `orbit_pixels`, `pan_pixels`, `zoom`,
  `zoom_at`, `fit`, `set_view(NamedView::{Front,Back,Top,Bottom,Left,Right,Iso})`, `ray`,
  `project`. World is Z-up.
- `image`: `RgbaImage` PNG encode/decode, `diff` with `DiffTolerance` (per-channel
  threshold + max differing ratio) and a diff visualization.

## Memory
One CPU copy per mesh (needed for picking and the CPU fallback): positions `f32x3`,
normals `snorm16x4`, `u32` indices, face/edge ranges and the BVH. GPU vertices are 20
bytes (`f32x3` + `snorm16x4`). Highlights are drawn by splitting index ranges into extra
draw calls, so changing a highlight never re-uploads geometry.

## Tests
`cargo test -p gusset-render`. Golden PNGs (192x144) live in `tests/golden/`;
`GUSSET_UPDATE_GOLDEN=1` rewrites them, mismatches go to `target/golden-failures/`.
Without any adapter the GPU tests print `SKIP: ...`; set `GUSSET_REQUIRE_GPU=1` in CI so a
missing adapter fails instead.

CI requirements:
- Ubuntu: `sudo apt-get install -y mesa-vulkan-drivers libegl1 libegl-mesa0`
  (lavapipe for Vulkan, llvmpipe through EGL for the GL backend).
- Windows: WARP ships with Windows; nothing to install.
- macOS runners: Metal (paravirtual GPU on GitHub runners).

Example: `cargo run -p gusset-render --example render_view -- out.png 640 480 dark iso`.

## Not yet
Silhouette edges, SSAO / ground shadow (high tier), instancing of repeated parts, LOD
switching while interacting (the renderer only lowers MSAA and render scale), SIMD
dispatch for picking, MSAA counts other than 1 and 4 (need an optional wgpu feature).
