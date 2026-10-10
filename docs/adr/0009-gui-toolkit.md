# ADR 0009: GUI toolkit — egui/eframe on wgpu

- Status: accepted
- Date: 2026-10-10
- Research: `docs/research/gui-and-solver.md`

## Requirements (from the spec)
Native Rust (no web views); runs natively on Windows x64/ARM64, Linux x64/ARM64
(Wayland and X11) and macOS ARM; docking; theming; rich tooltips; a radial menu;
accessibility; smooth animation while staying idle-efficient; a wgpu 3D viewport; a
software fallback; GUI automation tests; a permissive license.

## Decision
**egui + eframe** (0.36.x, MIT OR Apache-2.0) with the **wgpu** renderer (wgpu 30).
- **Idle:** eframe repaints only on input, an explicit `request_repaint`, or an animation.
  This meets "idle means idle".
- **Accessibility:** AccessKit integration ships by default (Windows UIA, macOS AX,
  Linux AT-SPI).
- **Docking:** `egui_tiles` (Rerun, MIT/Apache) for panel layout and persistence.
- **Viewport:** our renderer (`gusset-render`) draws into an offscreen wgpu texture on the
  *same* device and registers it with egui as a native texture. The same renderer
  produces offscreen PNGs for `render_view` and screenshot tests, with no window.
- **Theming, tooltips, radial menu:** egui `Style` comes from our design tokens
  (`docs/DESIGN.md`). The radial marking menu and rich tooltips are custom widgets,
  painted with egui shapes (cheap, and DPI-correct).
- **GUI automation tests:** `egui_kittest` drives the real UI through its AccessKit tree,
  headless, with snapshot images rendered through wgpu, which in CI is the software
  adapter.
- **No-GPU fallback chain:** wgpu hardware adapter → wgpu `force_fallback_adapter` (WARP
  on Windows, lavapipe/llvmpipe on Linux) → wgpu GL backend → (last resort) a CPU
  rasterizer path for the viewport image, shown as an egui texture. The UI itself always
  needs some wgpu adapter. In practice WARP ships with Windows 10/11 and llvmpipe with
  Mesa, so a working software adapter is always present on first-class targets.

## Alternatives rejected
- **Slint**: the free licenses are GPL-3 (banned) or a royalty-free license with field and
  API-exposure restrictions that are incompatible with our MIT/Apache distribution.
- **GPUI**: the crates.io release is stale; it renders with D3D11 on Windows (not wgpu)
  and embeds external GPU content only on macOS.
- **iced**: no accessibility yet (issue #552), and depends on git forks.
- **Xilem/Masonry, Blitz/dioxus-native, Freya, Floem**: experimental, or depend on Skia (C++).

## Risks and mitigations
- **API churn** (breaking releases every 1–3 months): versions are pinned in the
  workspace, upgrades happen deliberately one at a time, and egui types stay inside
  `gusset-ui`.
- **Right-to-left text is weak** (egui issue #8576). All strings are externalized from
  day one and layout uses logical start/end. Full RTL rendering is a known gap: it is
  listed under "Deviations from spec" until egui or cosmic-text integration fixes it,
  and English is the only shipped language for the MVP.
- **Idle/startup/memory numbers are unmeasured** for our use. The first GUI milestone
  measures them on the reference-class hardware available in CI and records them in
  `docs/PERFORMANCE.md`.
