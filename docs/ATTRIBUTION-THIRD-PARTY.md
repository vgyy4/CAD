# Third-party attribution: adapted source code

Gusset is licensed under `MIT OR Apache-2.0`. This file lists source files that contain code
adapted from other projects (as opposed to dependencies, which `THIRD_PARTY_LICENSES.md`
lists and `cargo-deny` checks). Every adapted file starts with a header comment naming the
source. The licence notice below also ships in `THIRD_PARTY_LICENSES.md` (it is part of
`about.hbs`).

## SolidCraft

- **Project:** SolidCraft, <https://github.com/bherbruck/solidcraft>
- **Copyright:** (c) 2026 SolidCraft contributors
- **Licence:** `MIT OR Apache-2.0` (we use it under the MIT licence; the text is below)
- **Why:** ADR 0023 §4: the owner likes SolidCraft's Fusion-style layout; where its
  code-drawn widgets fit, they are adapted with attribution and restyled with Gusset's design
  tokens (DESIGN.md §3).
- **What is adapted:** drawing code and layout ideas only, written against egui. Nothing was
  copied wholesale, no assets (images, fonts) were taken, and no dependency was added.

| Gusset file | Adapted from (SolidCraft `crates/ui-egui/src/`) | What |
|---|---|---|
| `crates/gusset-app/src/icons.rs` (the `Tool` icons: `Pen`, `paint_tool`) | `icons.rs` | The 24 x 24 code-drawn icon grid, the `Pen` helper (lines, polygons, arcs, arrows, isometric boxes) and the drawings of the extrude, revolve, sweep, loft, hole, fillet, chamfer, shell, combine, patterns, mirror, measure, import/export, sketch tools, trim, orbit, pan, zoom, fit, home, undo/redo glyphs. Recoloured from Gusset's theme tokens (stroke `text`, fill `accent` at 20 %), plus new icons (gear, datum plane, move body, info, parameters, finish sketch, edges). |
| `crates/gusset-app/src/viewport_nav.rs` | `navigation_cube.rs`, navigation bar part of `viewport.rs` | The view cube's scheme: nine coplanar picking patches per visible face (face, edge and corner targets), shading by how directly a face looks at the viewer, hover highlighting of the whole target. Redrawn with Gusset's tokens and gusset-render's camera; the navigation bar is Gusset's own layout (orbit, pan, zoom, fit all, home, projection, edges). |

The ribbon itself (`ribbon.rs`) and the title bar are Gusset's own code; they follow the
layout of DESIGN.md §2 and use the command registry.

SolidCraft states that it contains no third-party iconography or artwork: its icons are
drawn in code by its contributors.

### Licence text (MIT)

```text
MIT License

Copyright (c) 2026 SolidCraft contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
