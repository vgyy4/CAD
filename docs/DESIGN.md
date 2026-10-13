# Gusset design guide

The binding rules for Gusset's UI. Derived from `docs/research/users-and-ux.md` (what users
praise and hate in Fusion, SolidWorks, Solid Edge, Onshape, FreeCAD and OpenSCAD) and
from the spec's "UI design" section. A UI change that breaks a rule here needs an ADR.

Target: a beginner models a first part within minutes without a tutorial, and an expert
never feels slowed down. We get there through progressive disclosure, not modes.

## 1. Interaction rules

1. **No workbenches.** Tools appear by *context* (part, sketch, later assembly/drawing).
   Each command exists exactly once in the command registry, and the toolbar, marking
   menu, palette and MCP are all views of that registry.
2. **Command search (Ctrl+K, also `/`).** Fuzzy search over every command, setting,
   parameter, feature and recent file, with each command's shortcut shown. It is the
   universal escape hatch, so nothing is ever buried.
3. **Cursor-local tools:** an S-key quick bar (context-sensitive, customizable) and a
   radial marking menu on right-click or right-drag (gesture mode: flick toward an
   item). A quick right-click release never fires a command by accident.
4. **One history, two views:** the feature tree (model browser) and the horizontal
   timeline at the bottom show the same history, with a draggable rollback marker,
   suppress, reorder (validated against dependencies), group, and dependency
   highlighting on hover.
5. **Edit in context.** Editing a feature keeps the full current model visible, with a
   live preview of the change. Only sketch editing rolls back, and it ghosts the
   downstream geometry.
6. **Live, transparent previews** of every feature before confirming. They update as
   values change and fade between before and after.
7. **Numbers everywhere accept expressions with units** (`width/2 + 3 mm`), and you can
   type dimensions in the viewport while drawing (on-canvas value entry, Tab to the next
   field).
8. **Parameters are first-class:** a variables panel and a batch-edit table. Later,
   parameters can be exposed as "customizer" sliders.
9. **Broken references never silently re-pick.** The feature is marked, the old
   reference is shown ghosted, and the user gets a one-click re-pick or "accept
   suggestion".
10. **Errors say what broke, where, and how to fix it**, and link to the feature and the
    entity. The MCP server uses the same error objects.
11. **No save anxiety:** infinite non-destructive undo (a tree), named iterations,
    branches, and auto-save by default. Manual-save mode still keeps a recovery snapshot.
12. **Local-first, offline, open format, no account. Exports are never gated:** STEP,
    STL and 3MF are always available.
13. **Direct edits are recorded features** (move/offset face) layered on history. There
    is no destructive mode switch.
14. **Selection never fights the user:** pre-highlight on hover, a "select other" picker
    for overlapping entities, selection filters in the quick bar, and temporary
    see-through.
15. **Stable layouts:** no surprise reshuffles. Panels are dockable and collapsible and
    remember their layout. One key (F11 by default) toggles a clean full-screen viewport.
16. **Navigation presets** (Gusset, Fusion, SolidWorks, FreeCAD, Onshape) are chosen at
    first launch and can be changed in Settings. SpaceMouse support.
17. **Dialogs show essentials first.** Advanced options sit in a collapsed "More"
    section, and every command works with zero settings changed.
18. **Tab/Enter flow** through every dialog without the mouse. Enter confirms, Esc
    cancels, and Ctrl+Enter confirms and repeats the command.
19. **Rich tooltips:** one-line description plus a small illustration, and a longer
    explanation after a further delay (default 1.2 s). Tooltips never cover the item
    under the cursor.
20. **Helpful empty states:** an empty document shows three starting actions (new sketch
    on a plane, import, open sample) and the command-search hint.
21. **Agents are visible:** agent branches, live agent edits and the audit log are
    visible in the UI. Agents never act invisibly.
22. **Accessibility:** full keyboard navigation, screen-reader labels through the
    toolkit's accessibility tree (AccessKit), WCAG AA contrast in both themes, UI scale
    from 75 % to 250 %, and "reduce motion" respected.

### Easy for beginners, never in the way of experts (owner request, 2026-10-12)
Hard or intimidating tasks (gears, threads, fasteners, springs, sheet-metal flanges)
get **generators**: a short form with sensible defaults and a live preview, such as
module, tooth count and pressure angle for a gear. A generator never produces a
black box. It writes ordinary parametric features and named parameters into the
document (an involute gear is a sketch plus an extrude, driven by `gear_module`,
`gear_teeth` and so on). Afterwards:
- every value stays editable in the parameters table, the code view and over MCP, and
  the user can open the generated sketch and change anything by hand;
- experts skip the form entirely: the same generator is a registry command
  (palette, keyboard, MCP) and a text-language statement, with all options as
  arguments, so there is no wizard to click through;
- advanced options (profile shift, backlash, root fillet, helix angle) sit behind an
  "Advanced" disclosure with the defaults shown, never hidden or locked;
- defaults come from the standards (ISO 53 gear profile, ISO metric threads) and are
  named in the form, so beginners learn the vocabulary instead of guessing.

### Heavy or impossible work never crashes the app (owner request, 2026-10-13)
When an operation is too heavy for the machine, or impossible in a way that only shows
during the computation, Gusset stops it and says why, and the model and the app stay as
they were. It never crashes, freezes without a way out, or exhausts the computer's memory.
- **Limits before work:** operations predicted too large are refused up front with the
  numbers and how to reduce them (done: evaluation work limits, `too_complex`, ADR 0020).
- **Stop at any time:** every long operation shows progress and can be cancelled; a
  cancel has no effect on the model (done between features and for MCP requests;
  inside a single OCCT operation still open, through OCCT's progress/user-break hook).
- **Bounded memory and time:** each rebuild runs under a memory and time budget derived
  from the machine (docs/HARDWARE.md tiers); exceeding it stops the rebuild with "needs
  more memory than available (about N GB)" instead of swapping or being killed.
- **Crashes stay contained:** a fault inside the geometry kernel (OCCT segfault, abort,
  stack overflow) cannot be caught in-process, so kernel work moves to a supervised
  worker process, like imports already are. A dead worker becomes an error on the
  feature that triggered it ("the geometry kernel crashed while building Fillet3"),
  the worker restarts, and the rest of the model keeps working.
- **Tested:** fault-injection tests kill the worker, exhaust its memory and time it out
  mid-operation, and check that the app reports it and keeps the model intact.

## 2. Layout

Fusion-style, after SolidCraft's layout which the owner likes (ADR 0023), in Gusset's own
design system:

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ ◆ Gusset  📂 💾 ↶ ↷        [ document tab ]                     agents ● v0.x │  title bar
├───────────────────────────────────────────────────────────────────────────────┤
│        SOLID   SURFACE   SKETCH   INSPECT   (tabs appear as features ship)     │  workspace tabs
│ [DESIGN ▾]  ⬚ ⬆ ↻ ⬡ │ ◜ ◇ ⧉ ✥ │ ⊞ │ 📏 │ ⇩ │ ⌖     (Ctrl+K search)          │  ribbon: icon tools
│ workspace   CREATE ▾  MODIFY ▾ CONSTRUCT▾ INSPECT▾ INSERT▾ SELECT▾             │  in dropdown groups
├────────────┬───────────────────────────────────────────────────┬──────────────┤
│ BROWSER    │                                         ┌──────┐  │ Properties / │
│ ▾ Document │                                         │ TOP  │  │ command panel│
│ ▸ Settings │                 viewport                │FRONT │  │ (essentials, │
│ ▸ Origin   │                                         └──────┘  │  "More" ▸)   │
│ ▾ Bodies   │                                                   │              │
│ ▾ Sketches │            ⟳ ✥ 🔍 ⛶ ⌂ ◫ ⚙   (navigation bar)       │              │
│ ▸ Params   │                                                   │              │
│ ▸ Branches │                                                   │              │
├────────────┴───────────────────────────────────────────────────┴──────────────┤
│ ⏮ ◀ ▶ ⏭ ╭[▱S1][⬆Ext1][◜Fil1]┃ rollback marker ─────────────────────────╮ (ⓘ) │  timeline
│ › feature.extrude sketch=S1 distance="10 mm"            (only when shown)     │  command line
│ ● Unsaved changes │ Building Fillet1 (3/12) [Stop] │ notices (fade)           │  status bar
└───────────────────────────────────────────────────────────────────────────────┘
```
- Title bar: app mark, open/save/undo/redo, the document tab(s), agent activity, version.
- Workspace tabs switch tool sets: Solid first; Sketch replaces the ribbon while a sketch
  is active (with a prominent "Finish sketch"); Surface, Assemble, Drawing, Render and
  Manufacture appear as they ship. The workspace switcher (Design ▾) sits at the left.
- Ribbon: large icon tools for the most used commands, each group a dropdown with the
  rest (Create: sketch, extrude, revolve, sweep, loft, hole, generators...; Modify:
  fillet, chamfer, shell, combine, move, pattern, mirror; Construct: planes; Inspect:
  measure, section; Insert: import; Select: filters). Every tool is also in Ctrl+K search.
- Browser: Document settings (units), Origin (planes, axes), Bodies (visibility eye),
  Sketches (with DOF), Parameters, Branches; "show unused sketches" toggle.
- Viewport: view cube (top right, clickable faces/edges/corners), navigation bar (bottom
  centre: orbit, pan, zoom, fit, home, view style, settings), axis triad.
- Timeline: one bar: playback controls, the features as icon chips in a recessed track
  (newest kept in view), rollback marker; the button at the right end expands the full
  iteration graph (branches, stars, colors).
- Command line: typed commands with key=value or JSON arguments, the same registry as
  MCP. Hidden by default (View menu, Ctrl+Shift+K).
- Status bar: saved state, rebuild progress with Stop, errors; confirmations fade after
  5 s, errors stay.
- Icons are code-drawn (no image assets) and themed with our tokens. Widgets adapted from
  SolidCraft keep its MIT/Apache attribution (ADR 0023).

## 3. Design tokens

The tokens are defined in code (`gusset-ui` `theme` module). No ad-hoc colors or sizes
anywhere in the UI. This table is the source of truth, and a test checks that the code
matches it.

### Spacing (logical px)
`space-0: 0, space-1: 2, space-2: 4, space-3: 8, space-4: 12, space-5: 16, space-6: 24, space-7: 32`

### Radii
`radius-sm: 3, radius-md: 6, radius-lg: 10, radius-pill: 999`

### Typography
- UI font: Inter (OFL-1.1), bundled. Fallbacks come from the system font stack.
- Monospace (code view, parameters): JetBrains Mono (OFL-1.1), bundled.
- Scale: `caption 11, body 13, body-strong 13/600, title 15/600, heading 18/600`.
- Line height 1.35. Tabular numbers in value fields.

### Color: dark theme (default)
| Token | Value | Use |
|---|---|---|
| `bg-app` | `#16181C` | window background |
| `bg-panel` | `#1D2026` | panels |
| `bg-raised` | `#252932` | popovers, menus |
| `bg-hover` | `#2D323C` | hover |
| `border` | `#323844` | separators |
| `text` | `#E6E8EC` | primary text (contrast 13.9:1 on bg-panel) |
| `text-muted` | `#A0A7B4` | secondary text (6.9:1) |
| `accent` | `#4C9AFF` | primary actions, focus ring |
| `accent-text` | `#0B1220` | text on accent |
| `success` | `#3FB27F` | |
| `warning` | `#E3A33B` | |
| `danger` | `#E5534B` | errors, broken features |
| `agent` | `#B07CFF` | agent branches and agent activity |
| `vp-bg-top` / `vp-bg-bottom` | `#2A2F38` / `#1A1D22` | viewport gradient |
| `vp-preselect` | `#7FB8FF` | hover highlight |
| `vp-select` | `#FFB000` | selection (distinct from accent for color-blind users) |
| `vp-edge` | `#0E1013` | model edges |

### Color: light theme
| Token | Value |
|---|---|
| `bg-app` | `#EEF0F3` |
| `bg-panel` | `#F8F9FB` |
| `bg-raised` | `#FFFFFF` |
| `bg-hover` | `#E4E8EE` |
| `border` | `#D3D8E0` |
| `text` | `#16181C` (16.6:1) |
| `text-muted` | `#545C69` (6.6:1) |
| `accent` | `#1F6FEB` |
| `accent-text` | `#FFFFFF` |
| `success` | `#1E8A5A` |
| `warning` | `#A86A00` |
| `danger` | `#C93C37` |
| `agent` | `#7A4FD6` |
| `vp-bg-top` / `vp-bg-bottom` | `#F4F6F9` / `#D9DEE6` |
| `vp-preselect` | `#2F7DEB` |
| `vp-select` | `#D98300` |
| `vp-edge` | `#20242B` |

### Elevation
`elev-0` flat; `elev-1` 1 px border; `elev-2` border + shadow (0 4 12 rgba(0,0,0,.25)) for
popovers and menus. Shadows are disabled on the low quality tier.

### Motion
`motion-fast 90 ms`, `motion-base 160 ms`, `motion-slow 260 ms`, ease-out cubic. Camera
moves to a named view take 260 ms. All motion drops to 0 under "reduce motion". Animation
frames are requested only while an animation is running (idle means idle).

## 4. Icons
An original SVG icon set in `assets/icons/`. 24×24 grid, 1.5 px stroke, rounded joins,
two-tone (stroke `text`, fill accent at 20 %). Icons are drawn from scratch; no vendor
icons and no copied trade dress. They are rendered to textures at the exact device scale
so they stay crisp at every DPI.

## 5. Viewport look (low tier must look good)
- Gradient background; matcap-style shaded faces with a soft key light and fill; black
  feature edges at 1 px device-independent width; silhouette edges.
- Selection color and pre-highlight are distinct from the accent and from each other,
  and also differ in luminance.
- High tier adds SSAO, a ground shadow and MSAA 4×.

## 6. Verification
- Screenshot tests of key screens in both themes (`tests/screenshots`), diffed in CI
  under the software renderer.
- At every milestone the reviewer subagent critiques fresh screenshots against this
  document. Issues are fixed like bugs, and the gallery in `docs/screenshots/` is updated.
