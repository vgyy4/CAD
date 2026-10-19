# ADR 0032: 2D drawings and DXF/SVG export

Status: accepted (2026-10-19)

## Context
2D drawings are in the v1 tier (docs/RESEARCH.md) and are the v0.4 milestone. Research in
docs/research/drawings.md. In short: users of Fusion, Onshape, SolidWorks and FreeCAD
TechDraw make sheets of projected views with dimensions and a title block, and export
PDF, DXF and SVG. Their most common complaint is that dimensions are lost or go wrong
after the model changes. Gusset's persistent names (ADR 0003) are the way to avoid that.
Exact OCCT hidden-line removal (`HLRBRep_Algo`) takes 0.1-0.7 s per view for parts of a
few hundred faces. It can be far slower on large assemblies, so it runs in the kernel
worker (ADR 0022) and its results are cached.

## Decision
1. **Drawings live in the document**, after the features, so branches, history, merge,
   undo and agents' read access work as they do for the model. A drawing changes no body
   and is not part of the rebuild; it is built on demand (when shown or exported) from the
   evaluated model.
   ```
   drawing Bracket_dwg (size: A3, landscape: true, projection: first, scale: 0.5,
                        title: "Bracket", number: "B-001", material: "PLA") {
     view Front (bodies: [Bracket], from: front, at: [130, 160])
     view Top (of: Front, side: top, gap: 20 mm)
     view Right (of: Front, side: right)
     view Iso (bodies: [Bracket], from: iso, at: [330, 210], scale: 0.25, hidden: false)
     dim D1 (view: Front, kind: distance, a: <edge>, b: <edge>, direction: horizontal, offset: 10 mm)
     dim D2 (view: Top, kind: diameter, a: <edge>)
     note N1 (text: "Break sharp edges", at: [20, 30])
   }
   ```
   - Sheet sizes are ISO A0-A4 and ANSI A-E. `projection` is `first` (ISO default) or
     `third` (ASME). `scale` is an expression, so a parameter can drive it, and is shown as
     a ratio (0.5 is 1:2). Positions are sheet millimetres from the lower-left corner.
   - Base views have `bodies` (default: every body), `from` (front, back, top, bottom,
     left, right, iso, iso_back) and `at`. A projected view has `of` and `side`. It stays
     aligned with its parent, `gap` apart (default 20 mm), on the side that `projection`
     gives: in first angle, the top view sits below the front view; in third angle, above.
     So views move with the model and never overlap their parent. Per view: `scale`
     (default the sheet's), `hidden` (hidden lines, default true; false for iso) and
     `tangent` (smooth edges, default false).
   - Dimensions are driven: their value is measured from the model every time and never
     stored. References are edge names, the same `edge(...)` and face-pair names joints
     use. Kinds: `length` (one straight edge), `distance` (two edges or their ends;
     `direction` horizontal, vertical or aligned in the view), `diameter` and `radius`
     (a circular edge), and `angle` (two straight edges). Values are measured on the
     projection in that view. `offset` and `text_at` place the dimension, and `tolerance`
     and `prefix` decorate its text.
   - A reference that does not resolve after a model change makes the dimension
     **dangling**. It is drawn in the dangling style with "?", listed by `query.drawing`
     with the missing name, and fixed with `drawing.edit_dimension`. A dangling dimension
     is never deleted silently.
2. **Kernel**: `Kernel::hidden_lines(shape, view)` (done). It returns lines, arcs and
   polylines in view coordinates, each visible or hidden, classed sharp, smooth or
   outline, with the model edge it comes from (`source`). The exact path walks
   `HLRBRep_Data` the way `HLRBRep_HLRToShape` does, and maps each fragment through the
   data's edge map to the shape's edge order. Silhouettes have no source. The mesh path
   (`HLRBRep_PolyAlgo`) stays in the protocol (`exact: false`) but the sheet does not use
   it: the research measured it as no faster once meshing is counted (amended after review
   N5). Worker protocol 7. Results are cached per (shape
   revision, view, deflection) in the session.
3. **Scene**: a new crate `gusset-drawing` turns a drawing and an evaluation into a
   renderer-neutral scene. A scene is sheets of primitives (line, arc, polyline, text,
   filled polygon) in sheet mm, each with a line style. Every backend draws this one scene:
   the app's egui painter, SVG, DXF and PDF. Line styles follow ISO 128: visible lines
   continuous wide (0.5 mm), hidden lines dashed narrow (0.25 mm, dashes 12d, gaps 3d),
   centrelines long-dash-dot narrow, dimension and leader lines continuous narrow. Arrows
   are filled, 3 mm long at 15 degrees. Text is 3.5 mm ISO 3098-like, from the app font.
   The title block is a built-in ISO 7200-style block filled from the drawing's
   properties. It also carries the projection symbol.
4. **Export**: SVG and DXF are written by our own code in `gusset-drawing`, with no new
   dependency. SVG has a `viewBox` in mm with `width`/`height` in mm, Y flipped in the
   data and no transforms. DXF is written as R2000 (AC1015) by default, with `$INSUNITS`
   = 4 (mm), layers per line style (VISIBLE, HIDDEN, CENTER, DIM, TEXT, BORDER), an LTYPE
   table, and dimensions exploded to lines and text. An R12 (AC1009) profile is offered
   for laser cutters and old readers; it flattens what R12 lacks. PDF (added in phase 4)
   is also written by our own code: one vector page in mm, arcs as Béziers, text in the
   standard Helvetica font with WinAnsi encoding (no font embedding, no dependency; this
   replaces the earlier plan to use `krilla`). Sketches export to DXF/SVG the
   same way, for laser cutting.
5. **Commands** (GUI = CLI = MCP, ADR 0012): `drawing.create`, `drawing.add_view`,
   `drawing.add_dimension`, `drawing.add_note`, `drawing.edit` (sheet properties),
   `drawing.edit_view`, `drawing.edit_dimension`, `drawing.delete` (an item, or the whole
   drawing), `drawing.export`
   (format, profile, path) and `query.drawing` (views with their sheet boxes, dimensions
   with values or dangling references, notes).
6. **Phases**:
   1. Document model, text form and validation; the scene with views, title block and
      line styles; SVG and DXF export; the commands and `query.drawing`.
   2. Dimensions and notes, centre marks and centrelines, dangling dimensions.
   3. The app: a Drawing workspace showing the sheet, view and dimension tools, and
      placing items by dragging; a PNG render of a sheet for agents.
   4. Full section and circular detail views, PDF, and DXF import into a sketch. Done
      2026-10-19. The DXF reader is our own safe Rust (ASCII DXF; LINE, CIRCLE, ARC,
      LWPOLYLINE and POLYLINE with bulges; capped, fuzzed) and runs in-process like the
      mesh readers, instead of the `dxf` crate in the import worker.

## Consequences
- Drawings travel with the model in one file. A drawing of a part inside another
  document's assembly uses components (ADR 0026); separate drawing documents can come
  later.
- Associativity rests on persistent names. A view keeps up with the model by itself. A
  dimension survives every change that keeps its edges' names, and is reported, not
  dropped, when a name goes away.
- Deferred: half, offset and aligned sections, break views, auxiliary views, GD&T,
  surface finish, BOM and balloons (v0.5), hole tables, editable templates, native DXF
  `DIMENSION` entities.
