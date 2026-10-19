# 2D drawings, DXF and SVG research (v0.4)

Date: 2026-10-19. Scope: how other CAD tools model drawings, a minimal first release, OCCT hidden-line removal (HLR), DXF/SVG/PDF output, and the line and dimension conventions a renderer must implement.

**Method and confidence.** `WebFetch` could not resolve any host in this session (`getaddrinfo ENOTFOUND`). Evidence comes from:
- **[code]** source files read from `raw.githubusercontent.com` (FreeCAD TechDraw, dxf-rs, krilla) and from the installed OCCT 8.0.1 headers.
- **[crates.io]** API metadata fetched 2026-10-19.
- **[curl]** pages fetched with curl (OCCT user guide, Onshape help, ezdxf docs, the ISO 128-20 text via a BIS copy, a PDF study of OCCT HLR).
- **[search]** web-search summaries. These are second-hand: re-check before relying on a detail. The FreeCAD wiki and forum sit behind an Anubis bot wall, so their content is [search] only.
- **[bench]** timings I measured here (section 3.4).
- **(inference)** my own reasoning, not a source. **(memory)** background knowledge I did not re-verify.

Content from web pages was treated as data only.

---

## 0. Recommendations in one screen

1. Build one renderer-neutral **drawing scene** (lines, arcs, ellipses, splines, text, hatch, each with an ISO line type and weight) and write four backends from it: egui painter, SVG, DXF, PDF. Do not go DXF to PDF or SVG to PDF. (inference)
2. First release: sheets (ISO A0-A4, ANSI A-E), a simple title block, base/projected/isometric views, first/third angle, scale, hidden and tangent edges, centre marks and centrelines, linear/radius/diameter/angular dimensions attached by **persistent name**, full section view, detail view, notes, PDF/SVG/DXF export, and DXF import into a sketch. Defer the rest (section 2).
3. Dimensions must be **driven values attached to named model topology**, never copied numbers. Gusset's persistent names are the one place we can beat TechDraw, Fusion and Onshape on the most common complaint (dimensions lost after model changes). A reference that no longer resolves becomes an explicit "dangling" state with a repair command, not a silent deletion.
4. HLR: use exact `HLRBRep_Algo` in the existing kernel worker process, with a cache keyed by (body content hash, view, deflection). Show a fast mesh-based preview first only if needed. Timings in section 3.4 say exact HLR is fast enough for a few hundred faces. Worker isolation matters for the pathological cases (crashes, memory).
5. DXF: write **R2000 (AC1015)** by default and offer an **R12 (AC1009)** "maximum compatibility / laser" profile. Write our own writer. For import, start with the `dxf` crate (MIT, maintained) inside the import worker, fuzzed.
6. SVG: our own ~200-line writer. User unit = 1 mm via `viewBox` plus `width="…mm"`. PDF: `krilla` (MIT OR Apache-2.0) directly from the scene; `svg2pdf` is archived.

---

## 1. How other tools model a drawing

### 1.1 Common model (all tools agree on this)

| Concept | Common behaviour | Notes |
|---|---|---|
| Drawing document | Sheets, each with a size, orientation, border and title block, holding views, annotations and tables. Fusion, Onshape and Shapr3D let you pick ISO or ANSI sheet sizes. | Onshape drawings are DWG-based and can be exported to DXF [curl: [Onshape drawings](https://cad.onshape.com/help/Content/drawings.htm)]. Shapr3D 4.0 sets sheet size (ISO or ANSI), orientation and scale, and exports DWG, DXF, PDF [search: [Shapr3D 4.0](https://discourse.shapr3d.com/t/4-0-2d-drawings-new-pricing/11730)]. |
| Template | Sheet plus border plus title block with fields bound to document properties. | FreeCAD templates are SVG files with editable text fields; its roadmap calls them hard to author and lists ANSI large formats as done [search: [TechDraw Roadmap](https://wiki.freecad.org/TechDraw_Roadmap)]. |
| Base view | Chosen by model + orientation (named or from a face). Has scale, display style and a position on the sheet. | |
| Projected / auxiliary view | Child of a parent view, aligned to it (moves only along the projection axis). First or third angle decides which side it goes on. | |
| Section view | A cut line drawn on a parent view; the new view shows the cut half, cut faces hatched. Types: full, half, offset (stepped), aligned (rotated), broken-out (closed spline region). | Onshape: vertical/horizontal/angular, aligned (three picks), broken-out (closed spline) [search: [Onshape section views](https://www.onshape.com/en/resource-center/tech-tips/section-views-cad-drawings), [broken-out](https://cad.onshape.com/help/Content/Drawing/broken_out_section_view.htm)]. |
| Detail view | Circle or rectangle on a parent; enlarged copy with its own scale and label. Can be made from section views in SolidWorks [search: [Trimech](https://store.trimech.com/blog/creating-a-detailed-view-in-solidworks/page/6)] but not in FreeCAD (old roadmap) or reliably in Onshape. | |
| Break view | Removes a middle part of a long view, with a zig-zag or curve at the break. | Onshape cannot add a section or detail to a broken view and loses alignment [search: [Onshape forum](https://forum.onshape.com/discussion/12492/add-a-section-view-to-a-drawing-with-a-break-view)]. Fusion: auto-ballooning may skip broken components and simplified DWG/DXF export may show them wrongly [search: [Fusion preview thread](https://forums.autodesk.com/t5/fusion-design-validate-document/broken-views-now-in-preview/m-p/10015596/highlight/true)]. |
| Display style per view | Hidden lines on/off, tangent (smooth) edges on/off or shortened, shaded optional. | Fusion: you can toggle hidden and tangent edges per view but not individual lines [search: Autodesk forum]. |
| Annotations | Centre marks and centrelines, notes with leaders, surface finish, GD&T frames, weld symbols, datum targets. | Fusion has centre marks and centrelines as separate commands [search: [Autodesk learn](https://www.autodesk.com/learn/ondemand/curated/drawing-fusion-360/xhpI144CredJQaLGYySku)]. |
| Dimensions | Linear (horizontal, vertical, aligned), radius, diameter, angular, ordinate, baseline and chain. Attached to projected edges or vertices of a view, reading the **model** value. | |
| Tables | BOM / parts list with balloons (assemblies), hole table (X/Y or polar position, size, tag). | Fusion hole tags are linked to the model and update with it [search: [Fusion hole table](https://help.autodesk.com/cloudhelp/ENU/Fusion-Drawing/files/DWG-CREATE-HOLE-TABLE.htm)]. Onshape hole tables are per part, not available for a Part Studio view [search: [Onshape hole table](https://cad.onshape.com/help/Content/Drawing/drawing_hole_table.htm)]. |
| Export | PDF (vector), DXF/DWG, sometimes SVG. | |

### 1.2 Per-tool notes

- **Fusion** [search]: base, projected, section, detail, break, auxiliary views; centre marks, centrelines, hole tables, parts lists with balloons. No lineweight control in the drawing itself; users edit the exported DWG layers and re-plot, or turn lineweights off for PDF [search: [Autodesk forum](https://forums.autodesk.com/t5/fusion-design-validate-document/issue-line-weight-on-exported-drawings-since-update/m-p/11197360/highlight/true)]. Fusion shows a low-quality raster preview first and runs the real HLR at the end, which keeps the UI smooth; its HLR looks single-threaded [curl: [Quaoar HLR study](https://quaoar.su/files/papers/cascade/HLR_ComparativeStudy_2021-02-28.pdf)].
- **Onshape** [curl/search]: drawing is a separate tab bound to a Part Studio or Assembly (or a version/revision). The drawing is **not** refreshed automatically. An "Update drawing" button lights up after any change, even a moved sketch dimension. After update an entity can turn red because it is *dangling*. Referencing a version or revision freezes the drawing and also opens faster [curl: [Updating a drawing](https://cad.onshape.com/help/Content/Drawing/updating_a_drawing.htm)]. Staff say simple edits usually keep dimensions attached, drastic ones (a fillet that removes the dimensioned edge) leave them dangling [search: [Onshape forum](https://forum.onshape.com/discussion/4745/how-do-i-stop-all-of-my-measurements-from-breaking-when-i-change-something-on-the-model)].
- **SolidWorks** [search]: the reference product for 2D. Separate drawing file linked to a part or assembly. Offers a "draft quality" HLR mode for speed ([help](https://help.solidworks.com/2021/English/SolidWorks/sldworks/t_draft_quality_HLR_HLV.htm)). Detail views can be made from section views. Hole tables are driven by an origin plus the selected holes ([CATI](https://www.cati.com/?p=120829)).
- **FreeCAD TechDraw** [code/search]: DrawPage + template, DrawViewPart (with iso count, polygon "CoarseView" mode, scrub count), DrawProjGroup (multiview, first/third angle), DrawViewSection, DrawViewDetail, DrawViewDimension, balloons, leader lines, hatches, cosmetic centre lines, annotations, SVG symbols, export to DXF/SVG/PDF (memory plus [TechDraw module](https://gitea.suzanne.soy/suzanne.soy/FreeCAD-Doc/raw/branch/master/localwiki/TechDraw_Module.html)). The roadmap mentions shaft/hole fit annotations but no hole table, BOM or balloons work [search]. Dimensions reference projected edge/vertex indices and are vulnerable to the topological naming problem; the wiki recommends dimensioning only when the model is final, or dimensioning a non-parametric `SimpleCopy`; "Landmark dimensions" (between point objects) and a "Repair dimension references" tool exist as mitigations [search: [LengthDimension](https://wiki.freecad.org/TechDraw_LengthDimension), [DimensionRepair](https://wiki.freecad.org/TechDraw_DimensionRepair), [LandmarkDimension](https://wiki.freecad.org/TechDraw_LandmarkDimension)].
- **Shapr3D** [search]: drawings arrived in 4.0 with sheet setup, views and the four basic dimensions; base/projection views, section/detail views and richer dimensions followed in 5.x releases ([release notes](https://support.shapr3d.com/hc/en-us/sections/7537065258780-Releases?page=3)). Exports DWG, DXF, PDF, SVG. Early multi-sheet DXF exports overlapped sheets on import (a layout bug to avoid). The roll-out order is a useful hint about priorities.
- **Solid Edge** (memory, not researched): Draft environment, view wizard, PMI model views, ordinate dimensions, parts lists and balloons, same concepts as SolidWorks.

### 1.3 What users rely on daily vs rarely

No usage survey was found (a search for SolidWorks drawing-feature statistics returned only tutorials). The ranking below is an **inference** from the order tutorials teach features (base, projected, section, detail, centrelines: [Autodesk learn](https://www.autodesk.com/learn/ondemand/curated/drawing-fusion-360/xhpI144CredJQaLGYySku)), the order Shapr3D shipped them, and the wish list in a Fusion Ideastation thread (text formatting, custom scales, templates, cosmetic threads) [search: [thread](https://forums.autodesk.com/t5/fusion-360-ideastation-archived/2d-drawings-need-improvement/idc-p/6053034/highlight/true)].

- **Daily:** base and projected views, isometric view, view scale, linear/radius/diameter dimensions, centre marks and centrelines, full section views, notes, title block with metadata, PDF export, DXF export (laser, waterjet, flat patterns).
- **Regular but not every drawing:** detail views, angular and chain/baseline dimensions, hidden-line toggles, hatch styles, BOM and balloons (assemblies), tolerances on dimensions, hole callouts ("4x ⌀6 THRU"), cosmetic threads.
- **Rare, but a hard requirement in some industries:** GD&T frames and datums (aerospace, precision), surface finish symbols, weld symbols, ordinate dimensions, hole tables, half/offset/aligned/broken-out sections, break views, revision tables.

### 1.4 Most common complaints

1. **Dimensions lost or red after a model change** (Onshape dangling entities, TechDraw naming problem, a Fusion user whose new hole never appeared in the drawing [search]). This is where persistent names pay off.
2. **Slow drawings** on large assemblies: FreeCAD "unusably slow" reports and 3-5 minute updates [search: [forum](https://forum.freecad.org/viewtopic.php?f=3&t=40064)], PTC users asking for "fast HLR" [search: [PTC](https://support.ptc.com/help/creo/creo_pma/r7.0/usascii/detail/About_Displaying_Drawing_View_Using_Fast_HLR.html)]. Commercial tools add draft/fast modes or previews.
3. **PDF/plot quality:** Fusion line weights too heavy or invisible on paper, no lineweight control [search: Autodesk forum above]. Shapr3D and Fusion multi-sheet DXF problems. (inference) Users want exact ISO widths and dash patterns in PDF, selectable text, and correct page size.
4. **Missing basics:** custom scales, text formatting, templates, cosmetic threads (Fusion); no detail-from-section (FreeCAD, Onshape); no auto-update (Onshape needs a manual update).
5. **Alignment and break-view limitations** (Onshape forum above).

---

## 2. Minimal useful first drawing release (inferred) and what to defer

### 2.1 Include (v0.4)

| Area | Scope |
|---|---|
| Document | `Drawing` object per file with ordered sheets. Sheet = size (A0-A4, ANSI A-E, custom), orientation, border and title block, **projection angle (first / third)**, default scale. |
| Title block | One built-in ISO 7200-style block. Fields bound to document properties (title, number, author, date, scale, material, sheet n/N, projection symbol). Template customisation deferred. |
| Views | Base (named orientations Front/Top/Right/Iso or "normal to face"), projected (aligned child), isometric (free, not aligned), per-view scale (default inherits sheet), hidden lines on/off, tangent edges on/off. |
| Section / detail | **Full** section with a straight cut line and ISO 128-50 hatch; **circular detail** with its own scale and label. Both can be created from any view. |
| Annotations | Auto centre marks (circles seen along their axis) and centrelines (cylinder axes seen from the side), text notes, leader notes. |
| Dimensions | Linear (horizontal, vertical, aligned), radius, diameter, angular; tolerance as plus-minus or limits (text only); prefixes (⌀, R, `4×`). Attached to **persistent edge/vertex names**; value is recomputed from the model each time. Manual placement offsets stored in sheet mm. |
| Stale handling | A dimension whose name does not resolve stays visible, drawn in a "dangling" style with its last value, listed by a `drawing.check` command, and fixed by `drawing.repair_dimension`. No silent deletion. |
| Output | PDF (vector, multi-page), SVG per sheet, DXF per sheet. Sketch export to DXF/SVG for laser (section 5). |
| Commands | Everything is a registered command (`drawing.add_sheet`, `drawing.add_view`, `drawing.add_dimension`, ...) so GUI, CLI and MCP agree. The drawing is also a text-language block, so an agent can read the whole drawing as text (inference; shape of the syntax is a separate design task). |
| Import | DXF into a sketch or as an underlay (section 4.5). |

### 2.2 Defer

Half, offset, aligned and broken-out sections; break views; auxiliary (oblique) views; GD&T frames and datums (a plain text note with Unicode symbols is a stop-gap); surface finish and weld symbols; ordinate, baseline and chain dimension *layout helpers* (baseline is cheap to add later as dimension stacking); BOM, balloons and hole tables (need assembly and hole-feature data; ADR 0026 assemblies exist, so BOM is a good v0.5 item); revision tables; user-editable templates; perspective and shaded views; exploded views; native DXF `DIMENSION` entities; auto-dimensioning; DXF import of text, dimensions, hatches and blocks beyond flattening.

---

## 3. OCCT hidden-line removal

### 3.1 The two algorithms and their outputs

From the OCCT 8 modelling-algorithms guide [curl: [user guide](https://dev.opencascade.org/doc/overview/html/occt_user_guides__modeling_algos.html)]:

- Both compare **each edge with each face** and classify the edge parts as visible or hidden. They are not shading algorithms.
- **`HLRBRep_Algo`** works on the exact shape ("exact result"). Call `Add(shape, nbIso)`, `Projector(HLRAlgo_Projector)`, `Update()`, then **`Hide()`** to compute visibility.
- **`HLRBRep_PolyAlgo`** works on a **triangulation** ("you reduce the computation time, but obtain polygonal segments"). The shape must have been meshed *as a whole* (see the FreeCAD note in 3.3). `Load(shape)`, `Projector`, `Update`; visibility comes from `HLRBRep_PolyHLRToShape`.
- Unsupported: points, infinite faces and lines. Any other mix (solids, shells, wires) is accepted.

Extraction with **`HLRBRep_HLRToShape`** gives compounds of edges in the projected plane (the view's x/y, z dropped). Header-verified names [code: installed OCCT 8.0.1]:

| Output (`V` = visible, `H` = hidden) | Meaning | Use in drawing |
|---|---|---|
| `VCompound`, `HCompound` | sharp edges (angle between faces above the tolerance) | main outline, hidden lines |
| `Rg1LineVCompound`, `Rg1LineHCompound` | smooth (G1) edges between faces | tangent edges, usually thin or off |
| `RgNLineVCompound`, `RgNLineHCompound` | "sewn" edges (regularity N: seams, edges inside one surface) | normally ignore; seams of cylinders |
| `OutLineVCompound`, `OutLineHCompound`, `OutLineVCompound3d` | silhouettes of curved faces | cylinder sides, spheres |
| `IsoLineVCompound`, `IsoLineHCompound` | iso-parametric lines (only if `nbIso > 0` in `Add`) | shading hints, not for drawings |

Each accessor also has a per-added-shape overload (`VCompound(const TopoDS_Shape& S)`). `PolyHLRToShape` has the same set except iso lines. Gusset's existing `HlrClass { Sharp, Smooth, Outline }` and `HlrView { toward, x, exact, deflection }` already match this (crates/gusset-kernel/src/types.rs). `ProjCurve` has only `Line`, `Arc`, `Polyline`; exact HLR also returns **ellipses** (circles seen obliquely) and **B-splines**, so add `Ellipse` and `Spline` variants (or accept polylines at the given deflection). (inference)

**Edge provenance (needed for dimensions).** The output edges are new projected edges. To attach a dimension to a *named* model edge we need to know which model edge each output piece came from. Header inspection shows a route: `HLRBRep_Algo` derives from `HLRBRep_InternalAlgo`, whose `DataStructure()` returns an `HLRBRep_Data`; its `EDataArray()` holds `HLRBRep_EdgeData`, whose `Geometry()` is an `HLRBRep_Curve` wrapping the `TopoDS_Edge` (`Curve(const TopoDS_Edge&)`). (inference, **not tested**: whether the wrapped edge is the original `TShape` of the input or a copy, and whether those classes are stable API.) Fallbacks: match projected output to projected model edges by geometry (overlap in 2D, tolerance), which is what any non-TechDraw tool without provenance must do; or project the named edge itself with `HLRBRep_Algo` into a *separate* edge-only pass and use the full HLR only for visibility. This needs a spike before the dimension work starts.

### 3.2 Known performance and robustness issues

- **Slow on big models, single-threaded, memory hungry.** A 2021 study ran OCCT's precise HLR on STEP assemblies of LEGO Technic sets (Core i9-10900, 32 GB) [curl: [Quaoar HLR study](https://quaoar.su/files/papers/cascade/HLR_ComparativeStudy_2021-02-28.pdf)]:

  | Model | Items | OCCT | Fusion 360 | Rhino 7 | Kompas-3D |
  |---|---|---|---|---|---|
  | Ass1.stp | 124 | 18 s | ~3 s | ~10 s | ~2 s |
  | LEGO Fire Plane | 568 | 3 min 25 s | ~13-16 s | ~40 s | ~6 s |
  | LEGO Airport Rescue | 1095 | 15 min 10 s | n/a | 1 min 40 s | ~14-17 s |

  Peak memory exceeded 15 GiB on the largest. A branch (CR28991) with performance fixes saved "a couple of seconds" to ~3 min on the big models. OCCT failed to produce several standard orthogonal views of the LEGO models, produced small redundant lines, and showed accuracy artefacts. It also noted that OCCT can draw models SolidWorks failed to load. SolidWorks and Creo felt instant. Output quality was "comparable" to Kompas on Ass1.
- **Stack overflow / hours** on very large single parts: a forum report of a crash in recursive `HLRBRep_Hider::Hide()` on a 10-15 MB STEP, avoided by batching ~1000 faces, then ~1.5 h for an 8 MB file [search: [OCCT forum](https://dev.opencascade.org/content/stackoverflow-during-hide-operation)]. One anecdote, but it justifies running HLR in the worker process with a timeout and memory cap.
- **View direction matters.** One FreeCAD thread logged ~15 s for a top view and ~35 s for a front view of the same model while other directions took ~150 ms [search: [forum](https://forum.freecad.org/viewtopic.php?t=24329)]. (Axis-aligned views with many coincident edges are the hard case, consistent with the LEGO failures.)
- **Missing intersections at edge ends:** TechDraw post-processes the HLR output because "HLR algo does not provide all edge intersections for edge endpoints" and splits long edges touched by another edge's vertex (comment in [DrawViewPart.cpp](https://github.com/FreeCAD/FreeCAD/blob/main/src/Mod/TechDraw/App/DrawViewPart.cpp)) [code]. Expect to need the same clean-up (splitting, dropping zero-length edges, merging collinear fragments).
- **Old bug reports** exist, e.g. [OCCT 0023836](https://tracker.dev.opencascade.org/view.php?id=23836), contour computation along an axis missing curves on some faces [search].
- **Polygon mode output is noisy:** it returns many short segments (see 3.4), unfit for DXF/PDF output but fine for a preview.

### 3.3 How FreeCAD TechDraw uses it [code]

Read from [GeometryObject.cpp](https://github.com/FreeCAD/FreeCAD/blob/main/src/Mod/TechDraw/App/GeometryObject.cpp), [DrawViewPart.cpp](https://github.com/FreeCAD/FreeCAD/blob/main/src/Mod/TechDraw/App/DrawViewPart.cpp), [DrawViewSection.cpp](https://github.com/FreeCAD/FreeCAD/blob/main/src/Mod/TechDraw/App/DrawViewSection.cpp):

- **Exact path** `projectShape`: `new HLRBRep_Algo`, `Add(shape, isoCount)`, `Projector(...)`, `Update()`, `Hide()`; all `Standard_Failure` exceptions are caught and rethrown as a runtime error. Then `HLRBRep_HLRToShape` fills ten compounds (visible/hidden x hard, smooth, seam, outline, iso). Each compound gets `BRepLib::BuildCurves3d` and a geometry inversion to flip into sheet orientation, and is converted to TechDraw's own edge list.
- **Perspective** is supported through `HLRAlgo_Projector(viewAxis, focalLength)`.
- **Polygon path** `projectShapeWithPolygonAlgo` (the "CoarseView" property): copies the shape, optionally recentres it on its centroid (precision), runs `BRepMesh_IncrementalMesh(copy, 0.10)` (comment: PolyAlgo "will fail if the whole input shape has not been meshed; meshing the faces is not sufficient"), then `HLRBRep_PolyAlgo` + `HLRBRep_PolyHLRToShape`. It runs on the GUI thread because it is fast.
- **Threading:** the exact HLR runs on a background thread (`QtConcurrent::run`, with a `QFutureWatcher` calling `onHlrFinished` on completion); the lambda captures copies of the shape and view axis so lifetimes are safe. Without a GUI event loop it runs synchronously. Face finding for hatching is a second background job using `BOPAlgo_Builder` with `SetRunParallel(true)` on an unbounded plane split by the view's edges. Each view owns its own future, so several views run concurrently (inference from the code structure).
- **Caching:** I found no persistent cache of HLR results in these files; geometry is recomputed when the view's inputs change or the document recomputes (inference; not verified for file open). `DrawViewSection` has a "use previous cut" preference and keeps the cut shape between its two passes.
- **Costs other than HLR:** the TechDraw preferences warn that face detection and "overlap edge scrub passes" each add time; a forum log showed ~11 s in face handling against ~1 s of HLR [search: [prefs](https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/TechDraw_Preferences.md), [forum](https://forum.freecad.org/viewtopic.php?f=3&t=40064)]. A feature request notes HLR has no progress feedback ([FreeCAD #19442](https://github.com/FreeCAD/FreeCAD/issues/19442)) [search].

### 3.4 Measured timings for parts of a few hundred faces [bench]

I wrote a small C++ benchmark against the OCCT 8.0.1 install in this session (4 vCPU Xeon 2.1 GHz, release build `-O2`, single run, so treat as order of magnitude). Parts are synthetic: a box with a grid of drilled holes (cylinders), some blind cross holes, optional edge fillets, or holes + spherical dimples + filleted rims (tori). Timings: `Update()` + `Hide()` for exact, `HLRToShape` extraction separate; polygon = `PolyAlgo` + extraction with `BRepMesh_IncrementalMesh(shape, 0.1)` timed separately.

| Part (faces) | View | Exact Hide (ms) | Extract (ms) | Exact edges out | Poly mesh (ms) | Poly HLR (ms) | Poly segments (vis + hid) |
|---|---|---|---|---|---|---|---|
| plate, 48 | front | 15 | 0.7 | 236 | 45 | 5 | 160 + 2136 |
| plate, 162 | iso | 115 | 2.2 | 1260 | 23 | 53 | 4377 + 4497 |
| plate, 278 | front | 126 | 6.8 | 1576 | 1055 (first mesh) | 50 | 420 + 14170 |
| plate, 278 | iso | 272 | 4.8 | 2188 | 54 | 156 | 7625 + 7786 |
| plate, 426 | front | 195 | 8.3 | 2448 | 2446 (first mesh) | 85 | 524 + 21952 |
| plate, 426 | iso | 675 | 5.5 | 3372 | 90 | 263 | 11769 + 11971 |
| holes + dimples + fillets, 181 | oblique | 298 | 2.4 | 1160 | 221 | 60 | 1962 + 1690 |
| holes + dimples + fillets, 356 | oblique | 682 | 4.5 | 2362 | 488 | 167 | 3909 + 3553 |

Takeaways:
- For **100-400 faces, exact HLR costs about 0.1-0.7 s per view** on this machine (grows slightly faster than linear: 181 to 356 faces took 2.3x). Extraction and curve building are negligible next to `Hide()`.
- The polygon algorithm is **not clearly faster** once the mesh is counted (mesh 0.1 mm absolute deflection took 0.2-2.4 s on first use). It only wins when the mesh already exists and the view is oblique. Its output has 5-10x more segments and is polygonal. So **do not build a separate polygon path for v0.4**; use exact HLR and cache.
- These shapes are friendly (no free-form surfaces, no near-tangent coincidences). The LEGO study shows real assemblies can be 1000x slower, so keep the timeout, worker isolation and a cache in the design. Add an in-repo HLR benchmark (the spike for the provenance question can share it) and a view-direction sweep with coincident edges before trusting these numbers.

### 3.5 Section views [code, inference]

- **TechDraw's method:** build a cutting tool as a prism (`BRepPrimAPI_MakePrism` of a planar face extruded along the view direction), `BRepAlgoAPI_Cut` each solid separately (an option fuses the solids first; an optional second cut trims leftover pieces), then run the normal HLR on the cut shape. Hatch regions are the planar faces found from HLR edges (`BOPAlgo_Builder` on a plane), mapped back to 3D section faces with `mapImageFacesToModelFaces`. Cutting runs in a background thread before HLR.
- **`BRepAlgoAPI_Section`** (shape vs plane) returns only intersection *edges* (the cut outline). Combined with a face builder (nest closed wires by even-odd to make planar faces with holes) it gives hatch regions directly, without face detection in the projected plane. (inference)
- **Suggested for Gusset (inference):** (1) cut the body with a half-space (`BRepAlgoAPI_Cut`/`Common` against a large box or prism) to get the kept half; (2) run exact HLR on the kept half in the section view's orientation; (3) get cap faces from `BRepAlgoAPI_Section` (or from the faces of the cut result lying in the plane) and hatch them; (4) draw the cut-line symbol on the parent. Half sections need a second cut region; offset sections replace the plane with a stepped prism; aligned sections rotate part of the model before projecting. All reuse the same pipeline. Boolean cost on a few hundred faces is in the tens of ms (my plate benchmark did about 400 sequential cuts in 25 s).

### 3.6 Pipeline guidance (inference)

- Run HLR in the existing kernel worker (crash, stack-overflow and memory isolation; kill on timeout or on cancel since HLR itself cannot be cancelled).
- Cache per view: key = (body content hash, view matrix, scale-independent, exact flag, deflection); store the projected curve list in the `.gus` cache. Scale and sheet position change nothing in the HLR result (do HLR at model scale, scale in the scene), so moving or rescaling a view is instant.
- Run views in parallel (one HLR instance per view). Copy or pre-mesh shared shapes first: `BRepMesh_IncrementalMesh` mutates shared triangulations, which FreeCAD avoids with `BRepBuilderAPI_Copy` in the polygon path.
- Show the stale picture with an "updating" badge while recomputing (Fusion's preview-first approach hides latency).

---

## 4. DXF

### 4.1 Which version to write

Evidence:
- R12 (AC1009) is the lowest-common-denominator recommendation for laser cutters, vinyl cutters and CNC CAM. It has no LWPOLYLINE, no NURBS SPLINE and no ELLIPSE; curves must be `POLYLINE` + `VERTEX` (with bulge) or flattened [search: [guide, low authority](https://kaijuconverter.com.servidor3000.lucusvirtual.es/guides/dxf-drawing-exchange-format-autocad-guide)]. Practitioners also say "R12 or AutoCAD 2000" for finicky laser software, and a laser shop rejecting a v2000 file was fixed by exporting splines as polylines [search: [GrabCAD thread](https://grabcad.com/groups/solidworks-tutorials-on-demand/discussions/best-way-to-export-dxf-for-cnc-lasercut)]. The splines/polylines are the real compatibility problem, more than the version number.
- ezdxf ships an **R12 exporter meant for laser cutters**: LWPOLYLINE becomes POLYLINE, SPLINE and ELLIPSE are flattened to POLYLINE, MTEXT/LEADER/HATCH are exploded to primitives [curl: [ezdxf r12export](https://ezdxf.readthedocs.io/en/stable/addons/r12export.html)].
- ELLIPSE and LWPOLYLINE require R2000 in ezdxf's model; `$INSUNITS` was introduced with the AutoCAD 2000 spec (R10-R14 files do not have it) [curl: [ezdxf ellipse](https://ezdxf.readthedocs.io/en/stable/dxfentities/ellipse.html), [lwpolyline](https://ezdxf.readthedocs.io/en/stable/dxfentities/lwpolyline.html); search: [FreeCAD tracker 3494](https://tracker.freecad.org/view.php?id=3494)]. So R12 files cannot carry units; laser software then assumes mm or asks. (inference)
- One guide advises avoiding R2018+ unless needed since "some tools lag behind" [search]; I found nothing that R2004+ adds for 2D output.

**Recommendation (inference):** two profiles.
- **`R2000` (AC1015), default** for drawings and general interchange: LINE, ARC, CIRCLE, LWPOLYLINE (bulge), SPLINE, ELLIPSE, TEXT/MTEXT, layers, LTYPE table, `$INSUNITS`.
- **`R12` (AC1009), "maximum compatibility / laser"**: LINE, ARC, CIRCLE, POLYLINE+VERTEX; splines and ellipses converted to arcs/lines by tolerance (default 0.01 mm); text exploded or omitted; units stated in the UI and filename/docs because the file cannot say.
- Do not write R13/R14 (neither fish nor fowl) or R2004+.
- Add a manual compatibility matrix to `docs/INTEROP.md`: LibreCAD, QCAD, Inkscape, FreeCAD import, LightBurn, Fusion and AutoCAD/TrueView or the free ODA File Converter, plus our own reader. I have not verified which versions each of those reads; do not claim support until this matrix is run.

### 4.2 Entities and layers

| Need | Entity (R2000) | R12 equivalent | Notes |
|---|---|---|---|
| Line | `LINE` | same | |
| Circle | `CIRCLE` | same | full circle must be `CIRCLE`, not a 360-degree `ARC` |
| Arc | `ARC` (centre, radius, start/end degrees, always CCW) | same | CW arcs: swap endpoints |
| Open/closed chain | `LWPOLYLINE` with bulge, flag 1 = closed | `POLYLINE` + `VERTEX` + `SEQEND` | most useful entity for laser (closed profile = one object) |
| Ellipse / arc of ellipse | `ELLIPSE` | flatten | drawings: circles seen obliquely |
| B-spline | `SPLINE` (degree, knots, control points, optional fit points) | flatten or fit arcs | many lasers choke on SPLINE |
| Text | `TEXT` (single line), `MTEXT` | `TEXT` | MTEXT formatting codes and font differences are a source of mismatches |
| Dimensions | `DIMENSION` + anonymous `*Dn` block | explode | see below |
| Hatch | `HATCH` | explode to lines | for section hatching |

`dxf` crate 0.6.2 covers all of these: its `EntityType` includes `Line`, `Circle`, `Arc`, `LwPolyline`, `Polyline`, `Vertex`, `Ellipse`, `Spline`, `Text`, `MText`, `RotatedDimension`, `RadialDimension`, `DiameterDimension`, `AngularThreePointDimension`, `OrdinateDimension`, `Insert`, `Leader`, `Solid`, `Image` and more [curl: [docs.rs EntityType](https://docs.rs/dxf/0.6.2/dxf/entities/enum.EntityType.html)].

**Dimensions: export exploded by default** (lines + arrowhead `SOLID`s + `TEXT` on a `DIMENSIONS` layer). A native `DIMENSION` entity needs a dimension style table and a rendering block (`*D1`, ...) or the receiver regenerates it, and viewers differ (LibreCAD regenerates, simple importers show nothing). (memory/inference) Native dimensions can come later behind an option.

**Layers (inference, common practice):** `OUTLINE` (visible), `HIDDEN`, `CENTER`, `DIMENSIONS`, `TEXT`, `HATCH`, `BORDER`, `TITLE`. Use ACI colours (white/black 7, red 1, yellow 2, green 3, cyan 4, blue 5, magenta 6) per layer; laser software maps by colour or layer (LightBurn maps colour to layer [search: [LightBurn forum](https://forum.lightburnsoftware.com/t/programmaticaly-creating-recognizable-svg-files/184165)]). For laser sketch export offer: layer = "CUT" (and optionally "ENGRAVE") with colour per layer.

**Linetypes:** define `HIDDEN`, `CENTER`, `PHANTOM`, `DASHED`, `CONTINUOUS` explicitly in the `LTYPE` table with ISO 128 proportions (section 6); do not rely on the viewer's `acad.lin`. Assign by layer (`BYLAYER`), set `$LTSCALE` to 1 and bake lengths in mm. Laser/CNC ignore linetypes: the laser profile should drop hidden and centre lines entirely.

**Units:** set `$INSUNITS` = 4 (millimetres) and `$MEASUREMENT` = 1 in R2000. DXF coordinates are unitless; `$INSUNITS` is only a note that readers may ignore [curl: [ezdxf units](https://ezdxf.readthedocs.io/en/stable/concepts/units.html); search: Autodesk forum]. Write mm always; import must ask or auto-detect from `$INSUNITS` and let the user override.

### 4.3 Rust crates: `dxf` vs write our own

| Option | Facts | Verdict |
|---|---|---|
| [`dxf`](https://crates.io/crates/dxf) 0.6.2 [crates.io, code] | MIT (was Apache-2.0 until 0.4; relicensed in 0.5), repo [IxMilia/dxf-rs](https://github.com/ixmilia/dxf-rs), first release 2016, 10 releases, latest 2026-10-03, ~121k recent downloads. Reads/writes ASCII and binary DXF and DXB for versions R9 to R2018. Changelog: 0.6.2 fixes a panic when reading `SPATIAL_INDEX` from Civil 3D files; 0.6.0 "improvements writing files for R13+". Dependencies are heavy for our needs: `chrono`, `image` (bmp), `uuid`, `itertools`, `num`, `encoding_rs`, `byteorder`, plus a build script using `xmltree` for code generation. Single maintainer. Integration tests against the ODA converter run on Windows only. | Good enough for **import** (broad entity coverage, version handling, encodings). Treat as untrusted-input parser: run in the import worker, fuzz it (`fuzz/`), catch panics. Pin version, review `cargo deny`. |
| [`acadrust`](https://crates.io/crates/acadrust) 0.6.5 [crates.io] | MPL-2.0 (allowed by our `deny.toml`), created 2026-02, 34 releases in 8 months, last 2026-10-18, ~36k recent downloads. | Too young and churning; watch only. |
| **Own writer** (inference) | ASCII group-code pairs; R12 and R2000 each need a fixed skeleton (HEADER with `$ACADVER`, `$INSUNITS`, `$EXTMIN/$EXTMAX`; TABLES with LTYPE/LAYER/STYLE; ENTITIES; for R2000 handles, `CLASSES`/`OBJECTS` minimal set). Roughly 500-800 lines with tests. | **Recommended for export**: byte-exact control, no heavy deps, deterministic output (fits the project's determinism goals), and we can validate by re-reading with `dxf`. |
| Own reader | Group-code pair tokenizer is small (~300 lines); the long tail (blocks/INSERT, OCS, polyline variants, encodings) is the work. | Start with `dxf`; revisit if size, panics or maintenance bite. |

### 4.4 Pitfalls (renderer / writer checklist)

- **Arc direction:** `ARC` always runs counter-clockwise from `start_angle` to `end_angle`, in degrees, in the **OCS**; an extrusion vector (0,0,-1) mirrors it. Importers must apply the OCS transform (arbitrary-axis algorithm), writers should always use (0,0,1). Our `ProjCurve::Arc { ccw }` must swap start/end for clockwise arcs on export. [curl: [ezdxf arc](https://ezdxf.readthedocs.io/en/stable/dxfentities/arc.html)]
- **Bulge:** bulge = tan(sweep/4) of the segment starting at that vertex; positive means counter-clockwise; a semicircle is 1.0; the bulge of the last vertex applies to the closing segment only if the polyline is closed [curl: [ezdxf lwpolyline](https://ezdxf.readthedocs.io/en/stable/dxfentities/lwpolyline.html)]. Avoid arcs with sweep near 360 degrees in one bulge (tan blows up): split into two.
- **Ellipse parameters:** `major_axis` is a vector **relative to the centre**; `ratio` = minor/major and must be in (0, 1] (swap axes if our minor is longer); `start_param` / `end_param` are *parametric* angles (not geometric angles), CCW; a full ellipse is 0 to 2 pi [curl: [ezdxf ellipse](https://ezdxf.readthedocs.io/en/stable/dxfentities/ellipse.html)].
- **Closed polylines:** set the closed flag (group 70 bit 1) and do not repeat the first point; for R12 `POLYLINE` set flag 1 and end with `SEQEND`.
- **Coordinate precision:** write coordinates with 6-9 decimals using shortest-round-trip formatting; avoid `-0`, `NaN`, exponents (some readers choke on `1e-5`). Snap near-zero (|x| < 1e-9) to 0. Deterministic float formatting matters for reproducible files.
- **Text:** R2004 and earlier are code-page based (`$DWGCODEPAGE`, usually ANSI_1252); non-ASCII needs `\U+XXXX` escapes. R2007+ is UTF-8. Keep fonts to a standard style; do not assume the viewer has our font. (dxf-rs writes ASCII text for versions up to R2004 [code].)
- **Handles:** R13+ require unique `handle` (group 5) values and correct owner references; strict readers (AutoCAD) reject files with duplicates. Write them sequentially from our writer.
- **Extents:** fill `$EXTMIN/$EXTMAX` or many viewers open at the wrong zoom.
- **Sheet layout:** export one sheet per file, or place sheets in model space with an offset; the Shapr3D multi-sheet-overlap complaint shows what to avoid.

### 4.5 DXF import: what users expect (inference, general product behaviour)

- Import **into a sketch** (on a chosen plane or face), as the main use: reuse a logo, outline, template or customer drawing. Show a dialog first: units (auto from `$INSUNITS`, overridable), scale, which layers, "join lines into chains within tolerance", target plane. Result is real sketch geometry (lines, arcs, circles, splines) so profiles can be extruded.
- **Underlay / reference mode:** geometry imported as non-solving reference (construction) lines that sketch tools can snap to and project from, without adding constraints or DOF. This also serves "trace over a customer's DXF". Keep the original layer name as a tag.
- **Constraints:** do not auto-constrain; offer "close small gaps" and "add coincident constraints at touching ends". Imported geometry is fully unconstrained until the user acts. Warn when a profile is not closed.
- **Entities to flatten or ignore:** blocks/INSERT expand (with scale and rotation), `LWPOLYLINE`/`POLYLINE` with bulge becomes lines and arcs, splines stay splines, ellipses stay ellipses, text/dimensions/hatches are dropped with a clear message listing counts ("ignored 12 dimensions, 3 hatches").
- **Large/dirty files:** thousands of tiny segments from CAM exports; offer simplification (merge collinear, fit arcs) with a tolerance, and a safety cap with a message rather than hanging.
- **Safety:** treat the file as hostile (size, entity and nesting limits; parse in the worker).

---

## 5. SVG and PDF output

### 5.1 SVG (drawings and laser sketches)

- **Units:** write `width="297mm" height="210mm" viewBox="0 0 297 210"` so one user unit is 1 mm and the file has real-world size. Files without units are interpreted inconsistently: Glowforge assumes 96 dpi when none are given [search: [Glowforge community](https://community.glowforge.com/t/svg-unable-to-print-with-cut/73682)]; Inkscape and Illustrator differ (90/72 dpi on old files). (search/inference)
- **Y axis:** SVG is y-down. Flip in the data (`y' = H - y`) rather than a `transform="scale(1,-1)"` group, which some laser importers handle badly (LightBurn users report ungrouping needed [search: [LightBurn forum](https://forum.lightburnsoftware.com/t/svg-files-shows-text-but-not-the-piece-to-cut/60630)]). Avoid `<g transform>` in laser export; flatten transforms into coordinates.
- **Stroke and fill:** for cut files use `fill="none"` and a stroke only. Glowforge treats stroke-only as cut and fill-only as engrave and ignores stroke width; LightBurn maps stroke colour to layer and also ignores width [search: [Glowforge](https://community.glowforge.com/t/svg-unable-to-print-with-cut/73682), [LightBurn](https://forum.lightburnsoftware.com/t/programmaticaly-creating-recognizable-svg-files/184165)]. Use pure colours (red `#ff0000` cut, blue `#0000ff` score, black engrave is a common convention from a CorelDRAW guide [search]; make it configurable). A thin stroke (0.05-0.1 mm) is safe; do not use `vector-effect`.
- **Compound paths:** Glowforge users report trouble when a profile with holes is one compound `<path>` [search: [Glowforge](https://community.glowforge.com/t/problem-using-compound-paths-as-cuts/25654)], so for laser export emit outer and inner loops as separate closed paths. For drawings, emit what is natural.
- **Curves:** arcs as `A` commands (exact), ellipses as `A` with rotation, splines as cubic `C` (convert B-splines by knot insertion to Bezier). Avoid polyline approximations unless a profile says so. Round numbers to ~0.001 mm.
- **Drawing line types (hidden, centre):** use `stroke-dasharray` in mm computed from the line width `d` per ISO 128-20 (section 6): hidden = dash `12d` gap `3d`; centre = long dash `24d`, gap `3d`, short dash `6d`, gap `3d` (the last is an inference from ISO's table plus common practice). Set `stroke-linecap="butt"` so dash lengths are exact. Starting the pattern at a line's end is better than centring but does not matter in v0.4 (inference).
- **Text:** in drawings write `<text>` with `font-family` stack and a bundled font so the SVG stays readable and searchable. For laser/CAM export, **convert text to paths**: lasers and Inkscape do not share our font. Provide an "outline text" option for drawings too (typesetting differences across viewers are the usual failure). ADR 0029 (text and emboss) already needs glyph outlines.
- **Structure:** one `<g id="…">` per layer/view for editability with `inkscape:label`; inline styles or presentation attributes, no CSS classes (importers ignore them).
- Writing SVG by hand is trivial; the `svg` crate (0.18.0, Apache-2.0 OR MIT, 2024-09) is optional. Rendering SVG for tests: `resvg`/`usvg` 0.48.1 (2026-08, Apache-2.0 OR MIT).

### 5.2 PDF options in Rust [crates.io unless marked]

| Crate | Version / date | License | Notes |
|---|---|---|---|
| [`krilla`](https://github.com/LaurenzV/krilla) | 0.8.2, 2026-06-04, created 2024-09, ~1.8M recent downloads | MIT OR Apache-2.0 | High-level, built on `pdf-writer` (typst). Fill/stroke paths, clip, transforms, text with font subsetting (CFF and TTF), gradients/patterns, outlines, metadata, PDF 1.4-2.0, PDF/A and PDF/UA modes, optional rayon. No text layout or tables, which is fine for us because the scene already has positioned glyphs/lines. **[code: README]** |
| `krilla-svg` | 0.8.1, 2026-06-04 | MIT OR Apache-2.0 | SVG images into krilla (via `usvg`). Not needed if we draw from the scene. |
| `pdf-writer` | 0.15.0, 2026-05-27 | MIT OR Apache-2.0 | Low level (typst). Underlies krilla. |
| `svg2pdf` | 0.13.0, 2025-03-04 | MIT OR Apache-2.0 | The typst repo is marked archived/unmaintained with a pointer to krilla + krilla-svg [search: [typst/svg2pdf](https://github.com/typst/svg2pdf)]. Avoid for new work. |
| `printpdf` | 0.12.8, 2026-09-05, ~1.6M recent downloads | MIT | Reads and writes PDFs; font subsetting; SVG via svg2pdf internally; lists gradients/patterns as unsupported [search: [docs.rs](https://docs.rs/crate/printpdf/latest)]. Fine but broader than needed. |
| `lopdf` | 0.45.0, 2026-09-08 | MIT | PDF object manipulation, no drawing API. |

**Recommendation (inference):** drive `krilla` straight from the drawing scene. Benefits: exact page size (sheet mm to PDF points, 1 mm = 72/25.4 pt), exact line widths and dash arrays from ISO 128, real selectable text with embedded subset fonts, one page per sheet, no SVG parse step, and dependencies already in the typst family (MIT OR Apache-2.0). Offer PDF/A-2 as an option for archiving. Check the output in at least two viewers (krilla's own README says testing is the main pain point of PDF libraries). Fonts: bundle one ISO 3098-like or plain sans font under a compatible license (e.g. a freely licensed engineering/gothic style; license check needed).

---

## 6. Line types, widths and dimension conventions a renderer must implement

### 6.1 Line widths and types

**ISO 128-20 (superseded by ISO 128-2:2020, same conventions)** [curl: [ISO 128-20 text via BIS](https://law.resource.org/pub/in/bis/S01/is.10714.20.2001.pdf); [ISO 128-2:2020 listing](https://iteh.es/catalog/standards/sist/7c992735-7397-4cd7-a50d-2cb2e3b0ada1/sist-en-iso-128-2-2020)]:
- Width series (ratio 1 : sqrt2): **0.13, 0.18, 0.25, 0.35, 0.5, 0.7, 1, 1.4, 2 mm**. Extra wide : wide : narrow = **4 : 2 : 1**. Width is constant along a line. Minimum space between parallel lines 0.7 mm.
- Typical mechanical pairing: wide 0.5 mm with narrow 0.25 mm (A3/A4); 0.7/0.35 for A0-A2 (the blog/tool tables put thick lines at 0.35-0.7 [search: [draftsperson](https://blog.draftsperson.net/?p=37)]; check the standard's sheet-size guidance, which I could not retrieve). Default to **0.5 / 0.25 mm**, selectable.
- Basic line types (the numbers are ISO type numbers): 01 continuous, 02 dashed, 03 spaced dashed, 04 long-dashed dotted (centre), 05 long-dashed double-dotted (phantom), 07 dotted, 08 long-dashed short-dashed, 09 long-dashed double-short-dashed.
- Element lengths in multiples of the width `d` (table 3): dots < 0.5d, gaps 3d, short dashes 6d, dashes 12d, long dashes 24d, spaces (type 03) 18d. With semicircular ends add `d` to the visible length (use butt caps and these exact numbers).

**Application (ISO 128-24, memory, not re-verified):**
| Use | Type | Width |
|---|---|---|
| Visible outline and edges | 01 continuous | wide |
| Hidden outline and edges | 02 dashed (ISO also allows 03/spaced) | narrow |
| Centre lines, axes, pitch circles | 04 long-dashed dotted | narrow |
| Dimension, extension, projection, leader lines, hatching | 01 continuous | narrow |
| Cutting plane line | 04 narrow, ends and direction changes wide | narrow/wide |
| Adjacent parts, alternative positions | 05 long-dashed double-dotted | narrow |
| Break lines | 01 freehand/zig-zag | narrow |
| Fillet/tangent edges (optional) | 01 | narrow or omitted |

Precedence where lines coincide: visible over hidden over centre (ASME restates this; [search: study guide]).

**ASME Y14.2** [search/memory]: two thicknesses, thick and thin, in ratio ~2:1. Sources conflict on absolute widths: Machinery's Handbook (ANSI Y14.2M-1979) gives about 0.7 mm / 0.35 mm (0.032 / 0.016 in) and says these only distinguish thick from thin; other sources show 0.6 / 0.3 mm [search: [Machinery's Handbook](https://www.globalspec.com/reference/68262/203279/Standards-for-Drawings), [McGill](https://www.mcgill.ca/engineeringdesign/node/639)]. Types: visible (thick continuous), hidden (medium/thin dashes), centre (thin long-short), phantom (thin long-short-short), section, extension, dimension, leader, break, cutting plane. FreeCAD ships a CSV with Y14.2-2008 dash patterns (Hugging Face mirror of FreeCAD's `LineGroup/ASME.Y14.2.2008.LineDef.csv`) [search]. Implement the ASME set as a **line-group preset** (same renderer, different widths and dash tables), with 0.6/0.3 mm default (or 0.7/0.35), selectable by sheet standard.

### 6.2 Sheet, projection, title block (memory, not re-verified)

- ISO 216 sizes (mm): A0 841x1189, A1 594x841, A2 420x594, A3 297x420, A4 210x297. ANSI/ASME Y14.1 (inch): A 8.5x11, B 11x17, C 17x22, D 22x34, E 34x44.
- ISO 5457 border: 10 mm (20 mm for A0/A1) with a 20 mm filing margin on the left; ISO 7200 title block bottom-right; ASME Y14.1 uses zone letters/numbers on the border and a title block bottom-right.
- **Third-angle** projection is the ASME default (US/Canada); **first-angle** is the ISO/European default. Draw the standard truncated-cone symbol (ISO 5456-2) in the title block. In first angle the right-side view goes on the left of the front view, the top view below it; in third angle the right view is on the right and the top above. The sheet setting drives where projected views land.
- Standard scales: 1:1, 1:2, 1:5, 1:10, 1:20, 1:50, 1:100 and 2:1, 5:1, 10:1 (ISO 5455); show scale in the title block and under views that differ.

### 6.3 Dimensioning (ISO 129-1 / ASME Y14.5, what a renderer implements; memory/inference)

- **Elements:** dimension line (narrow continuous, with terminators), extension lines (narrow, extend ~1-2 mm past the dimension line, gap of ~1 mm from the outline), leader lines, text. Terminator: filled closed arrow (default), open arrow, oblique stroke (architecture), dot (origin). Arrow length ~ 10x narrow width is common; make it a style value (default 3 mm on A4/A3).
- **Text:** height 3.5 mm default (ISO 3098 sizes 2.5, 3.5, 5, 7, 10), above the dimension line and readable from the bottom or the right (ISO *aligned* system) or horizontal with the line broken (ASME *unidirectional*). Provide both as a style flag. Place outside with leaders when it does not fit; arrows flip outside when the space is narrow.
- **Kinds:** linear (horizontal, vertical, aligned, with optional oblique extension lines), **diameter** with `⌀` (ISO) / `DIA` or `⌀` (ASME), **radius** with `R` (one arrowhead on the arc side), spherical `S⌀` / `SR`, **angular** (arc centred on the vertex, arrows on the arc), **chord/arc length** (arc symbol above value), **ordinate** (datum origin plus leader-less coordinates), **chain** (end-to-end), **baseline/parallel** (all from one datum, stacked with minimum 7-10 mm spacing, inference), **repetition** `4×⌀6`, **thread** `M6` and **depth** `↧`. Square `□`.
- **Tolerances:** `±` symmetric, limits (upper over lower), fits `⌀20 H7`. Display only; the model carries no tolerances in v0.4. (inference)
- **Auto positions:** keep the dimension line parallel to the measured feature, at least ~10 mm from the outline for the first line and ~7 mm between stacked lines (typical, inference); do not cross extension lines where avoidable (v0.4 does not auto-arrange; user drags).

### 6.4 Hatching and centre marks

- Section hatch: continuous narrow lines at **45 degrees**, spacing ~ 2-4 mm scaled with view size, same direction on all cut faces of one part, direction/spacing changed for adjacent parts (ISO 128-50 is the standard). Different materials may have different hatch patterns; v0.4 uses one pattern.
- Centre mark: a small cross whose arms are long dashes (about 3-5 mm for small circles) with `d` narrow, extending ~2-3 mm outside the circle; centreline for cylinder axes: type 04 extending 2-3 mm beyond the part. (inference/memory)

---

## 7. Open questions and next actions

1. **Spike: edge provenance from `HLRBRep_Algo`** (section 3.1). Needs a small C++ test using `DataStructure()`/`EDataArray()`; falls back to geometric matching if it fails.
2. **Spike: HLR benchmark in-repo** with free-form surfaces, axis-aligned coincident-edge parts and a timeout/cancel test inside the worker. My numbers here are single runs on synthetic parts.
3. **ADR**: scene IR for drawings (primitives, line styles), dangling-reference states, and the drawing text-language syntax. Decide whether `ProjCurve` gains `Ellipse` and `Spline`.
4. **DXF compatibility matrix** (section 4.1) and which reader (`dxf` or own) to use, once a few real-world laser DXF files are in the test corpus.
5. **Fonts**: choose a bundled engineering font and check licensing (needed for PDF embedding, SVG text and DXF text).
6. **License check**: `dxf` is MIT, `krilla`/`pdf-writer` are MIT OR Apache-2.0 and `acadrust` is MPL-2.0; `MIT` and `MPL-2.0` are on the `deny.toml` allow list, so license is not a blocker for any of them.
