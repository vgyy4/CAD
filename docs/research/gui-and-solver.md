# Research: Rust GUI toolkits, wgpu, and 2D constraint solvers

- **Date:** 2026-10-10
- **Scope:** Task A covers native Rust GUI toolkits, wgpu and software fallback. Task B covers 2D geometric constraint solvers and linear algebra.
- **Method:** Crate versions, licenses, owners and dependencies come from the crates.io API (`https://crates.io/api/v1/crates/<name>`), queried 2026-10-10. Changelogs, READMEs and source came from `raw.githubusercontent.com` on the default branch, same date. The RustSec advisory DB is a shallow clone of `github.com/rustsec/advisory-db`, HEAD dated 2026-10-09. Web searches are cited inline. Anything I inferred rather than read in a source is marked **(inference)**.
- **Licensing policy applied:** The project is MIT OR Apache-2.0. GPL and AGPL may never be linked. LGPL is allowed only if dynamically linked and relinkable.

---

## Executive summary

| Decision | Recommendation | Confidence |
|---|---|---|
| GUI toolkit | **egui 0.36 + eframe (wgpu backend)**. Use **egui_dock** or **egui_tiles** for docking and **egui_kittest** for GUI tests. The 3D viewport goes through `egui_wgpu::CallbackTrait` or a registered native texture. | High |
| Runner-up | **iced 0.14**. It lacks AccessKit, and master depends on a forked winit and cosmic-text via git. | Medium |
| Rejected | **Slint**: license (GPL-3 or a proprietary royalty-free license with field-of-use and API-exposure limits). **GPUI**: crates.io is stale, and on Windows the renderer is D3D11 with no cross-platform external-texture element. **Xilem, Blitz, Freya**: too immature for a CAD shell, or a Skia C++ dependency. | High |
| Graphics | **wgpu 30.0.1**. Pass `force_fallback_adapter: true` as a runtime fallback; it maps to WARP on DX12 and llvmpipe/lavapipe on Mesa. Keep the eframe **glow** renderer as a second fallback. A pure-CPU egui renderer is a last resort that we should own. | High |
| Sketch solver | **Write our own** in Rust (sparse LM / Gauss-Newton with Jacobian-rank DOF analysis and graph decomposition). Use **KittyCAD `ezpz` (MIT)** and **`sketch-solver` (MIT OR Apache-2.0)** as references and test oracles, and do not depend on them at first. | High |
| Linear algebra | **faer 0.24** for the solver (sparse LLT/QR, rank-revealing QR, fast), configured `default-features = false` with sequential parallelism. Keep **nalgebra 0.35** (or glam) for small fixed-size geometry types. | Medium-High |

---

# Task A: native Rust GUI toolkits

## A.1 Current versions (crates.io, 2026-10-10)

| Crate | Latest | Released | License | Total / recent downloads | Repository |
|---|---|---|---|---|---|
| egui / eframe / egui-wgpu | 0.36.2 | 2026-09-08 | MIT OR Apache-2.0 | 25.8M / 6.2M (egui) | github.com/emilk/egui |
| egui_dock | 0.21.1 (egui ^0.36) | 2026-08-06 | MIT | 4.8M | github.com/anhosh/egui_dock |
| egui_tiles (Rerun) | 0.17.1 (egui ^0.36) | 2026-08-18 | MIT OR Apache-2.0 | 2.4M | github.com/rerun-io/egui_tiles |
| egui_kittest | 0.36.2 | 2026-09-08 | MIT OR Apache-2.0 | 0.97M | github.com/emilk/egui |
| egui_software_backend | 0.0.3 (egui **^0.34**) | 2026-03-29 | MIT OR Apache-2.0 | 3.3k | github.com/DGriffin91/egui_software_backend |
| iced | 0.14.0 (master is 0.15.0-dev) | 2025-12-07 | MIT | 2.9M | github.com/iced-rs/iced |
| slint | 1.18.1 | 2026-09-21 | `GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0` | 1.9M | github.com/slint-ui/slint |
| gpui (official, Zed) | 0.2.2 | **2025-10-22** (stale) | Apache-2.0 | 345k | github.com/zed-industries/zed |
| gpui-pre / gpui-component (third-party snapshots, owner `huacnlee`) | 0.3.8 / 0.7.1 | 2026-10-05 | Apache-2.0 | 118k / 215k | longbridge/gpui-kit |
| gpui-ce (community fork, owner `philocalyst`) | 0.2.2 | 2026-08-28 | Apache-2.0 | 13k | github.com/gpui-ce/gpui-ce |
| xilem / masonry | 0.4.0 | 2025-10-29 | Apache-2.0 | 11k / 22k | github.com/linebender/xilem |
| freya | 0.4.3 (0.5.0-rc.9 next) | 2026-10-09 | MIT | 44k | github.com/marc2332/freya |
| dioxus / dioxus-native | 0.7.10 | 2026-07-31 | MIT OR Apache-2.0 | 3.0M / 44k | github.com/DioxusLabs/dioxus |
| blitz-dom | 0.2.4 (main is 0.3.0-beta.2) | 2026-08-24 | MIT OR Apache-2.0 | 276k | github.com/dioxuslabs/blitz |
| floem (Lapce) | 0.2.0 | 2024-11-14 (stale on crates.io) | MIT | 43k | github.com/lapce/floem |
| makepad-widgets | 1.0.0 | 2025-05-13 | MIT OR Apache-2.0 | 20k | github.com/makepad/makepad |
| vizia | 0.4.0 | 2026-04-23 | MIT | 6.7k | github.com/vizia/vizia |
| wgpu | **30.0.1** | 2026-08-21 (v30.0.0 on 2026-07-01) | MIT OR Apache-2.0 | 39.8M | github.com/gfx-rs/wgpu |
| winit | 0.30.13 stable; 0.31.0-beta.3 | 2026-03-02 / 2026-09-04 | Apache-2.0 | 57M | github.com/rust-windowing/winit |
| accesskit | 0.25.1 | 2026-09-25 | MIT OR Apache-2.0 | 28M | github.com/AccessKit/accesskit |
| softbuffer | 0.4.8 | 2025-12-13 | MIT OR Apache-2.0 | 27M | github.com/rust-windowing/softbuffer |
| tiny-skia | 0.12.0 | 2026-02-02 | BSD-3-Clause | 54M | github.com/linebender/tiny-skia |
| vello / vello_cpu | 0.11.0 / 0.3.0 | 2026-10-02 | Apache-2.0 OR MIT | 0.98M / 5.7M | github.com/linebender/vello |

Notes:
- eframe 0.36.2 depends on `wgpu ^30.0`, `winit ^0.30.13`, `glow ^0.17` and `egui_inspection`, according to the crates.io dependency API. egui is therefore on the current wgpu.
- iced **0.14.0** (the released version) uses wgpu 27 (iced CHANGELOG, "Updated `wgpu` to `27.0`"). iced master uses `wgpu = "30"`, but also `winit = { git = "https://github.com/iced-rs/winit.git", rev = ... }` and a git `cosmic-text` (iced master `Cargo.toml`). A release can't ship git dependencies, so the next iced release will need those published, or we would have to pin git revisions.
- RustSec: none of egui, eframe, egui-wgpu, wgpu, wgpu-core, wgpu-hal, naga, iced, slint, gpui, winit, accesskit, tiny-skia, softbuffer, vello, egui_dock, egui_tiles, faer, argmin, levenberg-marquardt, ezpz or cassowary has an advisory directory in advisory-db (HEAD 2026-10-09). nalgebra has only RUSTSEC-2021-0070, patched in versions >= 0.27.1.

## A.2 Toolkit comparison

Legend: ✅ good / built in, 🟡 partial / third-party / caveats, ❌ missing or blocking.

| Criterion | egui/eframe 0.36 | iced 0.14 | Slint 1.18 | GPUI (Zed) | Xilem/Masonry 0.4 | Freya 0.4 | Dioxus-native / Blitz |
|---|---|---|---|---|---|---|---|
| Paradigm | Immediate mode | Elm architecture, retained | Declarative DSL (`.slint`) plus Rust | Hybrid immediate/retained, Tailwind-like | Reactive (Xilem) over a retained widget tree (Masonry) | Reactive components, Skia | React-like, with an HTML/CSS engine |
| Idle CPU | ✅ Reactive: "only repaints when there is interaction ... or an animation, so if your app is idle, no CPU is wasted" (egui README) | ✅ "Reactive rendering" added in 0.14 (CHANGELOG #2662) | ✅ Property-binding driven redraw | ✅ Redraws on `notify` (Zed is an editor) **(inference)** | ✅ Event-driven **(inference)** | ✅ | ✅ |
| Memory / startup | No current independent benchmark. A 2023 measurement found a window in about 33 ms for iced and total startup of about 230 ms (iced) and 280 ms (egui) ([Lukas Kalbertodt, 2023-02-03](http://lukaskalbertodt.github.io/2023/02/03/tauri-iced-egui-performance-comparison.html)) | see egui | No current benchmark | No current benchmark | No current benchmark | Skia adds binary size | Larger (CSS engine) |
| Accessibility (AccessKit) | ✅ Always on since 0.34 ("Remove `accesskit` feature and always depend on `accesskit`" #7701). 0.37 maps `WidgetType` to `accesskit::Role` and covers panels, popups and modals | ❌ Issue #552 "Implement accessibility support" still **open** since 2020 (GitHub issues search, fetched 2026-10-10). Only the MPL-2.0 libcosmic fork has `a11y` | ✅ `accessibility` feature with `accesskit_winit` (winit backend Cargo.toml). 1.18 exposes text-input content | ✅ On main: `accesskit_windows`, `accesskit_unix`, `accesskit_macos` (Zed workspace Cargo.toml) | ✅ AccessKit is built in (README) | ✅ accesskit 0.24 | ✅ "Accessibility using AccessKit" (Blitz README) |
| Docking / panels | ✅ `egui_dock` (tabs and splits) and `egui_tiles` (Rerun's tiling, used in Rerun Viewer). Native panels with drag-to-open and drag-to-close (0.35, 0.36) | 🟡 Built-in `pane_grid` (tiling, no tabs). `iced_aw` 0.14.1 adds tabs | ❌ No dock widget; you write your own | 🟡 Zed's dock lives in the app, not in gpui. `gpui-component` has a Dock (third party) | ❌ | ❌ | 🟡 Possible with CSS |
| Theming | 🟡 `Style`/`Visuals`, plus `Classes` (0.35) and an experimental `StyleProvider` theme plugin (0.37, unreleased) | ✅ Theme/palette, follows the system theme (0.14) | ✅ Strong styling; Fluent, Material, Cupertino and Qt styles | ✅ Tailwind-like styling | 🟡 | ✅ | ✅ CSS |
| Custom wgpu 3D viewport | ✅ `egui_wgpu::CallbackTrait` (`prepare`/`finish_prepare`/`paint`) or `Renderer::register_native_texture` (egui-wgpu `renderer.rs`). Official `custom3d_wgpu` demo. Same `wgpu::Device` | ✅ `shader` widget plus the `shader::Pipeline` trait (#2876) | 🟡 Only behind the `unstable-wgpu-30` feature (`slint::wgpu_30`, `Image::to_wgpu_30_texture()`). The API is unstable and pinned to a wgpu major | ❌ The `surface()` element is **macOS/iOS-only** (CVPixelBuffer, `gpui/src/elements/surface.rs`). Windows uses **Direct3D 11**, not wgpu (`gpui_windows` Cargo.toml); Linux uses wgpu 29 | 🟡 masonry_core is "designed to be embedded into existing apps ... which use wgpu", but there is no documented external-texture widget ([Linebender, Apr 2025](https://linebender.org/blog/tmil-15/)) | ❌ Skia GL/Vulkan, not wgpu | 🟡 `wgpu_texture` example in Blitz |
| Software rendering | 🟡 Not in eframe. wgpu fallback (WARP/llvmpipe), the glow backend on Mesa llvmpipe, or a third-party CPU renderer (A.4) | ✅ `iced_tiny_skia` is "a software alternative as a fallback" and is a default feature (README, Cargo.toml) | ✅ Built-in software renderer plus `renderer-skia-software` | ❌ | 🟡 vello_cpu exists; Xilem wiring unclear | 🟡 Skia CPU raster | 🟡 `anyrender_vello_cpu` |
| Windows ARM64 / Linux ARM64 | ✅/✅ Pure Rust over winit and wgpu. wgpu CI builds `aarch64-pc-windows-msvc` and `aarch64-unknown-linux-gnu` (`wgpu ci.yml`). egui's own CI doesn't list aarch64-windows **(build-only evidence)** | ✅/✅ same stack | ✅/✅ | 🟡 | ✅/✅ | 🟡 Prebuilt Skia binaries exist for both (rust-skia README) | ✅ |
| Wayland + X11 | ✅ via winit (FEATURES.md lists X11 and Wayland) | ✅ `x11` and `wayland` features (#2869) | ✅ winit or LinuxKMS | ✅ `gpui_platform` features `wayland`, `x11` | ✅ | ✅ | ✅ |
| IME | ✅ Overhauled in 0.35 (composition underline, `owns_ime_events`); mobile web in 0.36 | ✅ "Input method support" (#2777) | ✅ | ✅ | 🟡 | 🟡 | 🟡 |
| RTL / bidi | 🟡 Shaping via `harfrust` since 0.35, and 0.37 adds system-font fallback for "non-Latin scripts". No full bidi layout: open #8576 "TextEdit caret is lost in Arabic/Hebrew text since 0.35" (opened 2026-09-17). A move to Parley is in progress (0.33 changelog) | ✅ cosmic-text (shaping and bidi) | ✅ | ✅ | ✅ Parley (bidi) | ✅ Skia paragraph | ✅ Parley |
| GUI test harness | ✅ **egui_kittest**: queries the AccessKit tree (`get_by_label`, `click`), runs headless, does wgpu snapshot tests. **egui_inspection + egui_mcp** (0.35) let an agent drive a running app over a local port | ✅ "Headless mode testing" (#2698) and "First-class end-to-end testing" (#3059); `selector` API | ✅ `slint::testing`/`ElementHandle` (click, hover, drag) | 🟡 `TestAppContext` (internal style) | 🟡 Masonry test harness | 🟡 `freya-testing` | 🟡 |
| License | ✅ MIT OR Apache-2.0 | ✅ MIT | ❌ for us (see A.3) | ✅ Apache-2.0 | ✅ Apache-2.0 | ✅ MIT (Skia BSD-3, C++) | ✅ MIT OR Apache-2.0 |
| Maturity | ✅ Very widely used (25.8M downloads; Rerun Viewer). Breaking changes every release | 🟡 "Iced is currently experimental software" (README); releases about yearly | ✅ Commercially backed, 1.x API stability | 🟡 Pre-1.0. Official crates.io release about a year stale; the README tells users to add `gpui_platform`, which is **not on crates.io** (API 404) | ❌ "experimental" (README); last release Oct 2025 | 🟡 Single main maintainer | 🟡 Blitz "is currently in a **beta** state" (README) |

Other toolkits considered:
- **Floem**: crates.io release is from 2024-11; uses wgpu 27.
- **Makepad 1.0**: own DSL and shader stack, small crates.io footprint (20k downloads).
- **Vizia 0.4**, **Ribir 0.3**: small user bases.
- **libcosmic**: iced fork under **MPL-2.0** (LICENSE file) and not on crates.io. It has AccessKit, but file-level copyleft plus an unpublished fork makes it unattractive.

None of these beats egui or iced on the criteria above.

### Why egui fits a CAD shell
1. **wgpu interop is first class.** We share one `wgpu::Device` between the UI and the 3D viewport. `CallbackTrait` lets the viewport render inside egui's render pass. We can also render offscreen (for MSAA, picking buffers or OIT) and register the texture.
2. **Docking.** egui_dock and egui_tiles are mature and both track egui 0.36. egui_tiles is used in production by Rerun.
3. **Agent automation fits the project goal.** egui 0.35 added `egui_inspection` and `egui_mcp`. They read the AccessKit tree of a running app and send events to it (egui CHANGELOG 0.35.0). egui_kittest gives deterministic, headless, AccessKit-driven tests. The same accessibility tree serves screen readers, tests and our built-in MCP server.
4. **Idle CPU is zero** in reactive mode.

**Caveats to plan for:**
- Breaking API changes every 1-3 months (0.34 → 0.35 → 0.36 → 0.37). Pin and upgrade deliberately.
- Immediate-mode layout costs CPU with very large trees (egui README "CPU usage"). The feature tree and property tables must virtualize rows (`ScrollArea::show_rows`).
- Bidi is weak. If RTL locales are a requirement, track the Parley migration.
- egui 0.37 (unreleased) makes `default_fonts` non-default and deprecates `Id::new`.

## A.3 Slint licensing: is it compatible with MIT/Apache distribution?

From `LICENSE.md` and `LICENSES/LicenseRef-Slint-Royalty-free-2.0.md` (master, fetched 2026-10-10):
- **GPLv3 option.** "Use Slint for free under the GPLv3 ... your own files can stay MIT or Apache-2.0." The combined binary is still GPL-3. **This is banned by project policy.**
- **Royalty-free 2.0 option.** This is a proprietary, non-OSI license.
  - It grants use "as part of a Desktop, Mobile, or Web Application".
  - It requires attribution: an `AboutSlint` widget in the About dialog, or a badge on the download page.
  - It **excludes Embedded Systems**.
  - It forbids distributing Slint "alone and without integration into an Application".
  - It states "**The License does not permit the distribution of Application that exposes the APIs, in part or in total, of the Software.**"

  These terms do not pass through to downstream users the way MIT/Apache terms do. A plugin or scripting API that exposes UI primitives could breach the API clause. Anyone who reuses the app on a kiosk or machine-tool controller (an embedded system) would also be out of scope. Distros with DFSG/OSI rules would treat the dependency as non-free **(inference)**.
- **Commercial option.** Paid.

**Verdict:** Slint can't be used without either GPL (banned) or a proprietary license with field-of-use limits. That makes the distributed binary not "MIT OR Apache-2.0" in practice. **Rejected.**

## A.4 Rendering without a GPU ("stay usable with no GPU")

Ordered fallback chain for eframe:

1. **wgpu on a hardware adapter**: Vulkan or DX12 on Windows, Vulkan on Linux, Metal on macOS.
2. **wgpu fallback adapter.** `RequestAdapterOptions { force_fallback_adapter: true, .. }` exists in wgpu 30. The doc says "Indicates that only a fallback adapter can be returned. This is generally a 'software' implementation" (`wgpu-types/src/adapter.rs`). wgpu-core keeps only adapters with `device_type == DeviceType::Cpu` when the flag is set (`wgpu-core/src/instance.rs` ~L503).
   - **Windows: WARP.** The DX12 backend detects `"Microsoft Basic Render Driver"` and `DXGI_ADAPTER_FLAG_SOFTWARE` and reports `DeviceType::Cpu` (`wgpu-hal/src/dx12/adapter.rs` ~L171-199). WARP ships with Windows. A newer `Microsoft.Direct3D.WARP` NuGet package exists but is described as "for testing and development purposes" ([NuGet](https://www.nuget.org/packages/microsoft.direct3d.warp)), so rely on the inbox copy.
   - **Linux: Mesa lavapipe (Vulkan) and llvmpipe (GL).** wgpu-hal maps `vk::PhysicalDeviceType::CPU` to `DeviceType::Cpu` and has llvmpipe-specific handling (`wgpu-hal/src/vulkan/adapter.rs` ~L2323, ~L2455). llvmpipe is OpenGL 4.5 conformant ([Khronos](https://www.khronos.org/news/permalink/mesas-llvmpipe-is-opengl-4.5-conformant)). Mesa 25.2 added Vulkan 1.4 to lavapipe ([habr summary](https://habr.com/ru/articles/935032)), and Mesa 26.x release notes still list lavapipe fixes ([Mesa 26.1.1](https://docs.mesa3d.org/relnotes/26.1.1.html)).
   - **CI evidence.** wgpu's own GPU test CI installs WARP on Windows and Mesa on Windows and Linux (`.github/workflows/ci.yml`, "Install WARP", "Install Mesa"). Our CI can do the same for snapshot tests.
   - **How egui selects adapters.** egui-wgpu honours `WGPU_BACKEND` (`vulkan|dx12|metal|opengl|webgpu`) and `WGPU_POWER_PREF` (`egui-wgpu/src/setup.rs`). It doesn't set `force_fallback_adapter` itself, but `WgpuSetup::CreateNew` lets us pass a native adapter selector, and `WgpuSetup::Existing` lets us bring our own device. We should implement: try the hardware adapter → retry with `force_fallback_adapter` → GL.
3. **wgpu GL backend.**
   - Windows: "🆗 (GL 3.3+)" downlevel/best-effort via WGL. ANGLE becomes an opt-in on Windows via `cfg(windows_angle)` (wgpu README on trunk; CHANGELOG "Unreleased", **not yet in 30.0.1**).
   - Linux: GLES 3.0+ via EGL.
   - Alternatively, use eframe's **glow** renderer (`Renderer::Glow`), which runs on Mesa llvmpipe.
4. **Pure CPU egui.** egui only emits textured triangles ("egui can be used anywhere you can draw textured triangles", README), so a CPU rasterizer into a `softbuffer` surface works.
   - `egui_software_backend` 0.0.3 is MIT OR Apache-2.0 and uses softbuffer, winit and rayon. It is **pinned to egui ^0.34**, has one owner, and has 3.3k downloads.
   - `egui_backend_selector` 0.0.2 picks wgpu, glow or software at runtime. It is also on egui 0.34 and depends on `glfw`.
   - `dream-soft-render` 1.0.0 (2026-09-28, 104 downloads) is too new to trust.

   **Recommendation:** treat these as reference designs only. If we need tier 4, write a small owned `egui` → `softbuffer` painter (triangle rasterizer with texture sampling, about 1-2k LOC) so we aren't blocked on a third party's egui version. Our 3D viewport then needs its own CPU path: a simple flat- or Gouraud-shaded triangle rasterizer, or reuse of the same rasterizer.

**Alternative:** iced ships a maintained tiny-skia software renderer, which is a real point in iced's favour for the no-GPU requirement. But the 3D viewport needs its own CPU path either way, and iced lacks accessibility, so egui still wins overall.

### wgpu 30 notes relevant to a CAD viewport
- **Breaking changes in v30.0.0 (2026-07-01):**
  - `VertexState.buffers` is now `&[Option<VertexBufferLayout>]`.
  - Integer shader I/O needs explicit `@interpolate(flat)`.
  - Empty `BufferSlice` is allowed.
  - HDR/wide-gamut surface colour spaces via `SurfaceConfiguration.color_space`.

  Source: wgpu CHANGELOG.
- **Backends** (README table): Vulkan ✅ (Windows, Linux), DX12 ✅ (Windows), Metal ✅, OpenGL 🆗 downlevel, WebGPU ✅, WebGL2 🆗.
- **WebGL2 / downlevel limits** (`Limits::downlevel_webgl2_defaults`, wgpu-types `limits.rs`): **0 storage buffers, 0 storage textures, no compute** (all compute limits are 0), 11 uniform buffers per stage, `max_vertex_buffer_array_stride` 255, 15 inter-stage variables. `@interpolate(linear)` is rejected on GLES/WebGL2 (new `DownlevelFlags::LINEAR_INTERPOLATION`).
  - **Implication:** the core viewport path (shaded B-rep tessellation, edges, picking) must work without compute and storage buffers if we want GL or web fallback. Compute-based features (GPU picking via storage buffers, OIT) need a downlevel alternative.
- **Instance flags** come from environment variables: `WGPU_VALIDATION`, `WGPU_DEBUG`, `WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER`, and others (`wgpu-types/src/instance.rs`). `WGPU_ADAPTER_NAME` is in `wgpu::util` (`wgpu/src/util/init.rs`).

### GPUI detail (asked specifically)
- **crates.io:** official `gpui` is 0.2.2 from 2025-10-22, owned by Zed team members and `github:zed-industries:crates-io`. The current README requires `gpui_platform`, which is **not published** (crates.io API returns 404).
- **Third-party snapshots** exist: `gpui-pre` 0.3.8 ("snapshot of zed@279fe07", owner `huacnlee`), `gpui-pre-linux`, and `gpui-ce`. Depending on them means trusting a re-publisher, not Zed. That is a supply-chain concern under our dependency rules.
- **Windows:** Win32 + DirectWrite + **Direct3D 11** renderer (`crates/gpui_windows/Cargo.toml`). **Linux:** `gpui_wgpu` on wgpu 29, with X11/Wayland features. A secondary source reports that the crates.io build used Blade/Vulkan while git builds used wgpu after zed PR #46758 (2026-02-13) (blog post baeseokjae.github.io/posts/native-app-for-coding-agents-rust-gpui, seen only as a search snippet; the page was unreachable, so this is unverified).
- **No cross-platform API** to composite an external GPU texture: `surface()` is macOS-only.
- **Verdict:** not suitable as a standalone dependency for a wgpu-based CAD app today.

---

# Task B: 2D geometric constraint solvers usable from Rust

## B.1 Candidates

| Option | Language / license | Status (2026-10-10) | Usable by us? |
|---|---|---|---|
| **ezpz** (KittyCAD / Zoo Design Studio) | Rust, **MIT**. Owners `adamchalmers`, `jessfraz`, `github:kittycad:crate-owners` | 0.2.29 (2026-08-05), 214k downloads, 13 versions | ✅ License OK. Points, lines, circles, arcs; distance, H/V, angle, tangent (line-circle, circle-circle), midpoint, symmetric, point-on-arc, arc length/angle, equal length, priorities (`ezpz/src/constraints.rs`). Algorithm: **Levenberg–Marquardt with Tikhonov regularization toward the initial guess**, sparse Jacobian, faer sparse **LLT** of the normal equations (`ezpz/src/solver.rs`). Has `FreedomAnalysis`. Depends on faer 0.24 `sparse-linalg`. Pre-1.0 and API-unstable. No ellipses or splines |
| **sketch-solver** (neoscad) | Rust, **MIT OR Apache-2.0**. Only dependency is `libm` | 0.1.0, **published 2026-10-07**, 115 downloads | 🟡 Very good design write-up, but only 3 days old. Pipeline: connected-component split → "clean the drawing" → fixed-schedule LM → Gauss-Newton polish → continuation to stay on the drawn branch (orientation checks) → "exactify" → **Householder QR of Jacobian rows in constraint order** for rank, free coordinates, redundant vs conflicting constraints (README). Dense O(n³). Its unpublished `corpus/` holds GPL/LGPL-derived test data; **don't copy that corpus** |
| arael / arael-sketch-solver | Rust, MIT | 0.8.3 (2026-08-23), < 1k downloads, single author | 🟡 Interesting (compile-time symbolic differentiation) but tiny adoption |
| vcad-kernel-constraints, acs-solver, dotloom-constraints, solverang | Rust, MIT | Brand new, < 200 downloads | ❌ Too immature |
| **brepkit-sketch** | Rust, **AGPL-3.0-only** | 4.1.48 | ❌ **Banned** |
| **slvs** (Rust wrapper of SolveSpace) | **GPL-3.0** | 0.6.0 (2023) | ❌ **Banned** |
| **SolveSpace / libslvs** | C++, **GPL-3.0-or-later** (`COPYING.txt`, README "GPL v3 or later") | Active | ❌ **Banned**. Reading the GPL source for ideas is risky for clean-room purposes; use papers instead |
| **PlaneGCS** (FreeCAD Sketcher) | C++, **LGPL-2.1-or-later** (SPDX header in `GCS.cpp`) | Active in FreeCAD | 🟡 Allowed only as a **dynamically linked, relinkable** library. Depends on Eigen (dense/sparse QR), Boost.Graph headers, fmt, and FreeCAD headers (`Base/Tools.h`, `Base/Console`). Standalone extractions: [2wiceUponATime/PlaneGCS](https://github.com/2wiceUponATime/PlaneGCS) (CMake, needs Eigen3, Boost and fmt, plus a Console shim) and [Salusoft89/planegcs](https://github.com/Salusoft89/planegcs) (WASM). Algorithms: **DogLeg (default), LM, BFGS, SQP**; **Eigen FullPivHouseholderQR / SparseQR of Jᵀ** to find dependent parameters and redundant/conflicting constraints (`GCS.h`). Richest geometry set (ellipses, hyperbolas, parabolas, B-splines). Cost: C++ toolchain, a C shim, shipping .so/.dll/.dylib per platform, LGPL relinking obligations, and a cross-language FFI attack surface |
| fornjot (`fj-core`) | Rust, 0BSD | 0.49.0 (2024-03-21). README: mainline "has not been developed in over a year" | ❌ No sketch constraint solver; code-first kernel |
| cassowary | Rust, MIT/Apache | 0.3.0 (2017) | ❌ Linear-arithmetic UI layout (simplex); can't express distance/tangency |
| argmin | Rust, MIT OR Apache-2.0 | 0.11.0 (2025-09-28) | 🟡 Generic optimization framework (Gauss-Newton, line searches). Could host our solver loop, but adds abstraction we don't need |
| levenberg-marquardt | Rust, MIT | 0.15.0 (2025-08-03). Depends on **nalgebra ^0.34** (one major behind) | 🟡 MINPACK port, dense only, no rank diagnostics |

## B.2 How CAD sketch solvers work (and what we should build)

1. **Model.** Unknowns are point coordinates, radii and angles. Each constraint contributes one or more residual equations fᵢ(x) = 0 (for example, coincidence gives 2 equations and distance gives 1). Solve F(x) = 0 in the least-squares sense from the user's drawn positions.
2. **Decomposition.**
   - Split the constraint graph into **connected components** (cheap, always worth it). Every Rust solver above does this, and PlaneGCS uses Boost.Graph.
   - Optionally go further with structural decomposition: **Dulmage–Mendelsohn / block-triangular ordering** of the bipartite variable-equation incidence graph, solving small strongly-coupled blocks in sequence. This gives speed and better diagnostics on large sketches.
   - Classic geometric decomposition (Owen's / Hoffmann's rigid-cluster methods, see [Wikipedia: Geometric constraint solving](https://en.wikipedia.org/wiki/Geometric_constraint_solving)) is powerful but complex. **Recommendation:** connected components plus DM block-triangularization first.
3. **Numerics.**
   - **Newton / Gauss-Newton** on J Δx = −F, with **LM damping** (JᵀJ + λD)Δx = −JᵀF, or **DogLeg trust region**, which is FreeCAD's default.
   - Damped minimal-norm steps keep under-constrained directions near the drawing, which is what users expect ("least surprise").
   - Tikhonov regularization toward the initial guess (ezpz) does the same.
4. **DOF analysis and diagnostics** via **rank-revealing QR (or SVD) of the Jacobian**:
   - DOF = n_vars − rank(J).
   - The free directions are the null space of J. Use them to colour under-constrained geometry and to drive the drag solver.
   - Process rows in constraint order to find dependent rows. A dependent row is **redundant** if its residual is consistent and **conflicting** otherwise. Both PlaneGCS (QR of Jᵀ) and sketch-solver (Householder QR of rows) do this.
5. **Robustness features users notice:**
   - Branch preservation: don't flip a tangent arc to the other side or mirror a triangle. Use orientation checks plus **continuation/homotopy** from drawn values to target dimensions (sketch-solver).
   - A dedicated **drag mode**: a soft constraint pulls the dragged point to the mouse, and other entities move minimally.
   - Snapping exactly equal values ("exactify").
   - Deterministic iteration order: stable entity IDs and ordered maps (`indexmap`, as in ezpz).
6. **Scale.** Typical sketches have 10–1,000 unknowns. Dense QR at O(n³) is fine below a few hundred. Above that, use sparse Cholesky of the normal equations (ezpz) or sparse QR (PlaneGCS's `EigenSparseQR`). The rank decision needs pivoted QR; dense per component is usually affordable.

**Recommendation: write our own** `cad-sketch-solver` crate.
- **Why:**
  - Full control of determinism, diagnostics and MCP-facing explanations: "which constraints conflict", "DOF = 2, free: p3.x, arc1.r".
  - Feature coverage (ellipses and splines later).
  - A pure Rust, MIT/Apache, WASM-able core.
  - None of the existing permissive crates is both mature and stable.
- **Approach:**
  - Copy no code. Use **ezpz** and **sketch-solver** (both permissively licensed, so reading them is fine) as design references.
  - Use them as **differential-test oracles** in `dev-dependencies` after the usual vetting.
  - Write our own regression corpus. Don't import sketch-solver's GPL/LGPL-derived `corpus/`.
- **PlaneGCS:** keep it as a documented fallback option (LGPL, dynamic) only if feature parity with FreeCAD becomes urgent. It would need an ADR covering the C shim, dynamic linking, source offer and relinking.

## B.3 Linear algebra: nalgebra vs faer

| Aspect | **nalgebra 0.35.0** | **faer 0.24.4** |
|---|---|---|
| License | **Apache-2.0**. Changed from BSD-3-Clause at 0.33.0 (2024-06-23), per crates.io version history. Compatible with our dual license | **MIT** |
| Maintenance | Dimforge (`sebcrozet`, `milibopp`). 124 versions; latest 2026-05-24; MSRV 1.89 | Single maintainer (`sarah-quinones`). 53 versions; latest 2026-06-24. Repo moved to Codeberg; JOSS paper ([JOSS 06099](https://www.theoj.org/joss-papers/joss.06099/10.21105.joss.06099.pdf)). **Bus factor 1** |
| Adoption | 94M downloads (16.5M recent) | 5.0M (1.5M recent). **Used by ezpz (Zoo)** |
| Strengths | Static-size small matrices (`Matrix3`, `Isometry3`), geometry types, dense decompositions (QR, SVD, Cholesky, column-pivoted QR). The `nalgebra-sparse` crate is basic | State-of-the-art dense **and sparse** decompositions (sparse LLT/LDLT/LU/QR with symbolic analysis reuse), rank-revealing QR, SVD. Much faster on medium and large matrices |
| Dependency weight | Light: `simba`, `num-*`, `approx`, `typenum` (defaults `std`, `macros`) | Heavier: `gemm`, `nano-gemm`, `pulp`, `dyn-stack`, `equator`, `generativity`, `reborrow`, `faer-traits`. Defaults include `rayon`, `rand`, `npy`, so use `default-features = false, features = ["std","sparse-linalg"]` like ezpz |
| Determinism | Scalar loops with no runtime CPU dispatch, so results are reproducible across machines for the same binary and target **(inference from design)** | `pulp` dispatches "to equivalent vectorized versions based on the features detected at runtime" (pulp README). AVX2, AVX-512 and NEON paths may sum in different orders, so results can differ bitwise **across CPUs**. Parallel (`rayon`) reductions can also vary. **Mitigation:** `Par::Seq`, plus the solver's own snapping/exactify and tolerance-based convergence; test for tolerance, not bits **(inference)** |
| RustSec | RUSTSEC-2021-0070 only (patched in versions >= 0.27.1) | None |

**Recommendation:**
- Use **faer** (sequential, minimal features) inside the sketch solver for sparse normal equations and pivoted QR for rank and DOF.
- Use **nalgebra** or **glam** for the small fixed-size geometry types used throughout the app; that is a separate decision for the kernel team.
- Hide faer behind a thin internal `linalg` module so we can swap to nalgebra's column-pivoted QR if faer's single-maintainer risk materializes or if bitwise cross-machine determinism becomes a hard requirement. For document reproducibility, the persisted model should store solved values, so re-solving on another machine only needs agreement within tolerance.

---

## Recommendations (actionable)

1. **ADR: GUI = egui/eframe 0.36 with wgpu 30.**
   - Docking via egui_tiles or egui_dock. Prototype both in week 1 and pick one.
   - GUI tests with egui_kittest.
   - Evaluate `egui_inspection`/`egui_mcp` as a complement to our own MCP server.
   - Pin exact egui versions; upgrade on a schedule.
2. **ADR: rendering fallback chain:** hardware wgpu → `force_fallback_adapter` (WARP / lavapipe) → wgpu GL or eframe glow (llvmpipe) → owned CPU painter (later milestone). CI runs snapshot tests on WARP and Mesa as wgpu does. Keep the viewport's base path within downlevel limits (no compute, no storage buffers).
3. **Spike (measure, don't assume):** an empty eframe window plus a docked 3D viewport, measured on Windows x64, Windows ARM64 and Linux (X11 and Wayland). Record idle CPU, RSS, cold start, and the wgpu fallback behaviour. No current independent benchmark exists ([search, 2026-10-10](https://github.com/jordanfelle/nicti/issues/68) is an unfinished comparison).
4. **ADR: own sketch solver.**
   - Crate `cad-sketch-solver`.
   - LM/DogLeg on sparse J with faer.
   - Per-component pivoted QR for DOF, redundancy and conflict.
   - Continuation to preserve branches, plus a drag mode.
   - Differential tests against ezpz and sketch-solver.
   - Reject SolveSpace and slvs (GPL) and brepkit-sketch (AGPL). Keep PlaneGCS (LGPL, dynamic) as a documented, unused fallback.
5. **Rejected toolkits (for the ADR):**
   - Slint: license.
   - GPUI: unpublished platform crates, D3D11 on Windows, no external texture.
   - iced: no accessibility, git-pinned dependencies on master; revisit if 0.15 ships AccessKit.
   - Xilem, Blitz, Freya: maturity, and Skia C++ for Freya.

## Sources (all fetched 2026-10-10 unless noted)
- crates.io API: `https://crates.io/api/v1/crates/{egui,eframe,egui-wgpu,egui_dock,egui_tiles,egui_kittest,egui_software_backend,egui_backend_selector,iced,slint,gpui,gpui-pre,gpui-ce,gpui-component,xilem,masonry,freya,dioxus,dioxus-native,blitz-dom,vello,vello_cpu,wgpu,winit,accesskit,softbuffer,tiny-skia,nalgebra,faer,argmin,levenberg-marquardt,cassowary,fj-core,ezpz,sketch-solver,slvs,brepkit-sketch,arael-sketch-solver,floem,makepad-widgets,vizia,ribir,pulp,gemm}` (plus `/owners` and `/<ver>/dependencies`)
- egui CHANGELOG: https://raw.githubusercontent.com/emilk/egui/main/CHANGELOG.md
- egui README: https://raw.githubusercontent.com/emilk/egui/main/README.md
- egui_kittest README: https://github.com/emilk/egui/tree/main/crates/egui_kittest
- egui-wgpu: https://github.com/emilk/egui/blob/main/crates/egui-wgpu/src/renderer.rs and https://github.com/emilk/egui/blob/main/crates/egui-wgpu/src/setup.rs
- egui issues (bidi): https://github.com/emilk/egui/issues?q=is%3Aissue+bidi+OR+RTL+OR+right-to-left (#8576, opened 2026-09-17)
- egui_software_backend: https://github.com/DGriffin91/egui_software_backend
- wgpu CHANGELOG: https://github.com/gfx-rs/wgpu/blob/trunk/CHANGELOG.md (v30.0.1 dated 2026-08-21)
- wgpu README: https://github.com/gfx-rs/wgpu/blob/trunk/README.md
- wgpu-types: https://github.com/gfx-rs/wgpu/blob/trunk/wgpu-types/src/adapter.rs and https://github.com/gfx-rs/wgpu/blob/trunk/wgpu-types/src/limits.rs
- wgpu-core: https://github.com/gfx-rs/wgpu/blob/trunk/wgpu-core/src/instance.rs
- wgpu-hal: https://github.com/gfx-rs/wgpu/blob/trunk/wgpu-hal/src/dx12/adapter.rs and https://github.com/gfx-rs/wgpu/blob/trunk/wgpu-hal/src/vulkan/adapter.rs
- wgpu CI: https://github.com/gfx-rs/wgpu/blob/trunk/.github/workflows/ci.yml
- WARP NuGet: https://www.nuget.org/packages/microsoft.direct3d.warp
- llvmpipe conformance: https://www.khronos.org/news/permalink/mesas-llvmpipe-is-opengl-4.5-conformant
- Mesa 26.1.1 notes (2026-05-19): https://docs.mesa3d.org/relnotes/26.1.1.html
- iced CHANGELOG, README and Cargo.toml: https://github.com/iced-rs/iced
- iced accessibility issue: https://github.com/iced-rs/iced/issues/552
- Slint: https://github.com/slint-ui/slint/blob/master/LICENSE.md, https://github.com/slint-ui/slint/blob/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md, https://github.com/slint-ui/slint/blob/master/CHANGELOG.md (1.18.1, 2026-09-21), https://github.com/slint-ui/slint/blob/master/internal/backends/winit/Cargo.toml
- GPUI: https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md, `crates/gpui/src/elements/surface.rs`, `crates/gpui_windows/Cargo.toml`, `crates/gpui_linux/Cargo.toml`, `crates/gpui_wgpu/Cargo.toml`
- gpui-component: https://github.com/longbridge/gpui-component
- Xilem: https://github.com/linebender/xilem
- Linebender blog: https://linebender.org/blog/tmil-15/
- Freya: https://github.com/marc2332/freya
- rust-skia: https://github.com/rust-skia/rust-skia
- Blitz: https://github.com/DioxusLabs/blitz
- libcosmic: https://github.com/pop-os/libcosmic (LICENSE: MPL-2.0)
- Floem: https://github.com/lapce/floem
- Makepad: https://github.com/makepad/makepad
- AccessKit README: https://github.com/AccessKit/accesskit
- winit FEATURES: https://github.com/rust-windowing/winit/blob/v0.30.x/FEATURES.md
- Benchmarks: http://lukaskalbertodt.github.io/2023/02/03/tauri-iced-egui-performance-comparison.html (2023-02-03); https://tritium.legal/blog/desktop (practitioner report, Feb 2026)
- ezpz: https://github.com/KittyCAD/ezpz (README, `ezpz/src/solver.rs`, `ezpz/src/constraints.rs`, `ezpz/Cargo.toml`)
- sketch-solver: https://github.com/neoscad/sketch-solver
- PlaneGCS: https://github.com/FreeCAD/FreeCAD/tree/main/src/Mod/Sketcher/App/planegcs (`GCS.cpp` SPDX LGPL-2.1-or-later; `GCS.h`)
- PlaneGCS standalone builds: https://github.com/2wiceUponATime/PlaneGCS and https://github.com/Salusoft89/planegcs
- SolveSpace: https://github.com/solvespace/solvespace (COPYING.txt GPLv3; README "GPL v3 or later")
- Fornjot: https://github.com/hannobraun/fornjot
- Geometric constraint solving overview: https://en.wikipedia.org/wiki/Geometric_constraint_solving
- pulp: https://github.com/sarah-quinones/pulp
- faer JOSS paper: https://www.theoj.org/joss-papers/joss.06099/10.21105.joss.06099.pdf
- RustSec advisory-db: https://github.com/rustsec/advisory-db (shallow clone, HEAD 2026-10-09)

## Injection attempts observed
None. One note for completeness: the egui 0.35 CHANGELOG contains ordinary install instructions (`cargo install --git https://github.com/rerun-io/kittest_inspector egui_mcp`, `claude mcp add egui egui-mcp`). These are documentation, not directed at us, and **were not executed**. Any adoption of egui_mcp goes through normal dependency vetting.
