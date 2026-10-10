# Import / interop research

Date: 2026-10-10. Scope: the import and interop routes in PROMPT.md ("Interoperability & import"), covering readers, licenses, specs, vendor APIs and test corpora.

**Method and confidence.** WebFetch could not resolve any host in this session (`getaddrinfo ENOTFOUND` for wikipedia.org and others). Evidence therefore comes from three places:
- **[raw]**: files read directly from `raw.githubusercontent.com`, such as LICENSE, Cargo.toml and README files.
- **[crates.io]**: crates.io API metadata, fetched 2026-10-10.
- **[search]**: web-search result summaries. These are second-hand. Re-check them before relying on a detail.

The cited URL is the source in each case. No code was copied from any project, and none should be. Our license policy is MIT OR Apache-2.0. GPL and AGPL code may never be used. LGPL may only be dynamically linked.

Tiers follow PROMPT.md:
- **T1**: parametric (editable features).
- **T2**: exact B-rep.
- **T3**: mesh.

---

## 1. Summary table

| Format | Tier achievable | Open-source reader(s) + license | Our approach | Priority |
|---|---|---|---|---|
| STEP AP203/214/242 (.step/.stp) | T2 | `ruststep` 0.4.0 Apache-2.0 (last release 2024-09); `truck-stepio` 0.3.0 Apache-2.0 (2024-09); `step-io` 0.2.5 MIT OR Apache-2.0 (2026, "experimental"); `step-p21` 0.1.1 Apache-2.0; `monstertruck-io` 0.4.1 Apache-2.0; `cadmpeg-codec-step` 0.5.5 Apache-2.0. **Forbidden:** `openbim-step` AGPL-3.0, `brepkit-io` AGPL-3.0 [crates.io] | Write our own Part 21 lexer/parser (fuzzed), plus an entity mapper for AP203/214/242 geometry, topology, colors, names and assemblies. Generate types from the free EXPRESS longform schemas (MBx-IF / STEP Tools). Use `step-io` / `truck-stepio` only as reference behaviour, or as a dependency after an ADR. | P0 |
| IGES 5.3 (.igs/.iges) | T2 (often trimmed surfaces / shells, not solids) | `cadmpeg-codec-iges` Apache-2.0; `monstertruck-io` Apache-2.0 [crates.io]; no mature Rust reader | Our own fixed-column parser for entities 100–144, 186 (MSBO), 502–514 (B-rep) and 314 (color). Sew faces into solids when possible. | P1 |
| STL | T3 | `stl_io` 0.11.0 MIT (5.0M downloads) [crates.io] | Use `stl_io` (after dependency vetting) or ~200 LOC of our own. Weld vertices. | P0 |
| 3MF | T3 (+ T2-ish for color/material/components) | `threemf` 0.8.0 **0BSD** (hannobraun/3mf-rs); `threemf2` 0.4.1 MIT/Apache-2.0; `lib3mf` 0.1.6 MIT (telecos; **not** the 3MF Consortium's C++ lib3mf); `lib3mf-core` 0.4.0 BSD-2-Clause [crates.io] | Read/write with our own OPC-zip + XML code, or `threemf`. Spec is open (BSD-2 repo, royalty-free patent pledge). | P0 |
| OBJ | T3 | `tobj` 4.0.5 MIT (2.9M downloads, updated 2026-08) [crates.io] | Use `tobj`. | P1 |
| glTF 2.0 | T3 | `gltf` 1.4.1 MIT OR Apache-2.0 (10.3M downloads; last release 2024-05) [crates.io] | Use `gltf`. Also the target format of the Blender bridge. | P1 |
| PLY | T3 | `ply-rs` 0.1.3 MIT (unmaintained since 2020); maintained fork `ply-rs-bw` 4.0.1 MIT (2026-07); `serde-ply` 0.2.2 Apache-2.0 [crates.io] | `ply-rs-bw`, or our own reader (simple format). | P2 |
| Parasolid XT (.x_t/.x_b) | T2 | `parasolid-core` 0.3.7 "MIT AND Apache-2.0" (monozukuri-ai/parasolid-kit, 2026-09); `akiselev/xt-parser` (Cargo says MIT OR Apache-2.0, but **no LICENSE file**; schema code generated from Parasolid schema files and validated against a Parasolid DLL) [raw]; `cadmpeg` Apache-2.0 | **Clean-room** reader from the published *Parasolid XT Format Reference*, our own samples and the public schema description. Shared with the SolidWorks / Solid Edge / NX routes. | P0 (it unlocks SolidWorks, Solid Edge, NX and Onshape exports) |
| SolidWorks .sldprt/.sldasm | T2 (parts); assembly structure later; T1 only via the API add-in | `solid-diff` (Rust; Cargo `license = "MIT"`, **no LICENSE file**; links OCCT via `opencascade-sys`); `sldprt2step` Apache-2.0 (Python); `openswx` MIT (C++, metadata/BOM only); `blussyya/sldprt-format-research` MIT (Node); `cadmpeg-codec-sldprt` Apache-2.0 [raw, crates.io] | Clean-room container reader (2015+ chunk format + legacy OLE2/CFB via `cfb` MIT) that extracts the embedded XT partition and feeds it to our XT reader. Reading the MIT/Apache projects' **docs** is fine; copy no code from `solid-diff` until it ships a LICENSE file. | P1 |
| SolidWorks via API add-in | T1 | none (we write it) | C# add-in / VBA macro dumper to interchange JSON. "UNVERIFIED" until run against the real app. | P1 |
| Fusion .f3d/.f3z | T2 (analytic faces now; spline surfaces are hard); T1 partially possible from the design stream (params, timeline, sketches; ezf3d decodes these) | `ezf3d` 0.1.1 **MIT**, Python, clean-room, alpha [raw]; `InventorLoader` **GPL-2.0** (do not use) [raw]; `cadmpeg` Apache-2.0 (f3d "L4") [raw] | Clean-room Rust: ZIP (method 93 = zstd; `zip` MIT + `zstd` BSD-3 or pure-Rust `ruzstd` MIT) → Manifest/segments → ASM `.smb` B-rep reader. ezf3d's MIT format notes may be consulted with attribution. | P1 |
| Fusion via API add-in | T1 | none | Python add-in dumper (timeline, sketches, parameters) to interchange JSON. "UNVERIFIED". | P1 |
| Inventor .ipt/.iam | T2 (hard) | `InventorLoader` GPL-2.0 (not usable); `cadmpeg` Apache-2.0 (Inventor "L1") | OLE2/CFB container + ShapeManager/ACIS-family B-rep. Shares the SAT/SMB reader with Fusion. Later. | P3 |
| Solid Edge .par/.psm/.asm | T2 | none open-source found | Parasolid-based (per HOOPS docs); container reportedly OLE structured storage. Reuse CFB + XT readers. | P3 |
| ACIS SAT/SAB, ASM SMT/SMB (bare) | T2 | `cadmpeg-codec-sat` Apache-2.0 [crates.io] | Same entity reader as the Fusion route. Old (≈R7-era) SAT format description is public (Paul Bourke mirror). | P2 |
| FreeCAD .FCStd | T1 for PartDesign/Sketcher/Spreadsheet subset; T2 for everything else (embedded OCCT `.brp`) | `cadmpeg-codec-freecad` Apache-2.0 (L5) [crates.io]; FreeCAD itself LGPL-2.1+ (reference only) | Own ZIP + XML (`quick-xml`) reader → map PartDesign::Body/Pad/Pocket/Revolution/Fillet/Chamfer, Sketcher geometry + constraints, Spreadsheet aliases/expressions. Fall back to an own OCCT BRep-text parser (spec public in OCCT docs) per object. | P1 |
| OpenSCAD .scad | T1 (as code in our code view) | `tree-sitter-openscad-ng` 0.7.1 **MIT** (official openscad org); `openscad-lsp` MIT OR Apache-2.0; `openscad-rs` **GPL-3.0** (forbidden); OpenSCAD itself GPL-2.0+ (reference only) [crates.io, raw] | Our own parser, written from the user manual and observed behaviour. tree-sitter grammar (MIT) is usable for the editor/syntax. Transpile the AST to our code language; implement OpenSCAD semantics (special `$` vars with dynamic scope, `children()`, `let`, list comprehensions). | P2 |
| Blender .blend | T3 | none (by design: never parse .blend) | Spawn user's Blender: `blender -b -Y --factory-startup file.blend --python-expr "..export_scene.gltf.."`; import resulting glTF. | P3 |
| Onshape (REST) | T1 (+ T2 via its Parasolid/STEP export) | none | REST client in our importer: features list, variables, sketches, plus export of Parasolid for geometry fallback. | P1 |

Priority key:
- **P0**: needed for MVP exchange (PROMPT.md: import STEP, IGES, STL, 3MF, OBJ).
- **P1**: next.
- **P2 and P3**: later.

---

## 2. Proprietary native formats

### 2.1 Fusion .f3d / .f3z

**ezf3d** ([repo](https://github.com/AlexSabaka/ezf3d); [README](https://raw.githubusercontent.com/AlexSabaka/ezf3d/main/README.md), [LICENSE](https://raw.githubusercontent.com/AlexSabaka/ezf3d/main/LICENSE), [pyproject](https://raw.githubusercontent.com/AlexSabaka/ezf3d/main/pyproject.toml)) [raw]
- **License:** MIT, "Copyright (c) 2026 ezf3d contributors". The README calls it "a clean-room MIT implementation of the format".
- **Language and version:** Python ≥3.13, version 0.1.1, status alpha.
- **Container:** a ZIP whose entries are "**Zstandard**-compressed (method 93 — stdlib `zipfile` cannot open them)". Contents:
  - `Manifest.dat`
  - `<Asset>[Active]/<Segment>/{Meta,Bulk}Stream.dat`: "typed object graph — the parametric timeline"
  - `Breps.BlobParts/*.smb|*.smbh`: "`ASM BinaryFile8` — Autodesk Shape Manager B-Rep (`.smbh` carries rollback history)"
  - `OGS.BlobFolder`: a cached display mesh
  - `ProteinAssets` (materials) and `Previews/small.png`
- **Format notes:** in [docs/format/](https://raw.githubusercontent.com/AlexSabaka/ezf3d/main/docs/format/README.md).
  - Segment folder names drift between versions. Resolve them by segment *type* through the asset manifest.
  - The kernel works in **centimetres** (`CmMKS`).
- **Capabilities it claims:**
  - Container, B-rep traversal, analytic geometry, spline *curves*, tessellation, and STL/OBJ/glTF export.
  - Commands `params`, `timeline` and `sketches`, which decode the parametric design stream (feature kinds, parameter expressions, extrude operation).
  - Spline *surfaces* are "read but not yet trusted".
- **Validation it claims:** tested against four real designs ("42 B-Rep bodies, 99.8 MB of Shape Manager data… no unknown tokens"), with vertex-on-curve error below 2.2e-07 cm.
- **Reuse:** we may read its docs and could port ideas, with attribution under MIT. Cleaner still: implement in Rust from our own samples and use ezf3d only as a differential oracle in tests (it runs out of process, so no linking).

**InventorLoader** (jmplonka) ([LICENSE](https://raw.githubusercontent.com/jmplonka/InventorLoader/master/LICENSE), [README](https://raw.githubusercontent.com/jmplonka/InventorLoader/master/README.md)) [raw]
- License is **GPL-2.0**. Never copy it, and do not read its source for implementation purposes.
- Its README says Fusion and Inventor files contain an ACIS model. ezf3d notes that it predates Fusion's zstd ZIP entries.

**cadmpeg** ([README](https://raw.githubusercontent.com/cadmpeg/cadmpeg/main/README.md)) [raw]
- License: Apache-2.0 for code; CC BY 4.0 for docs and specs.
- A Rust multi-format converter. Its `LEGAL.md` sets clean-room rules ("Vendor SDKs, decompiled binaries, and confidential material are prohibited").
- Self-reported support levels on its ladder:
  - f3d L4; FCStd L5; STEP and IGES L9.
  - L1 for sldprt, ipt/iam, sat/smb, NX, CATIA and Creo.
- Young: created 2026-07, about 300–500 downloads per crate [crates.io].
- A good candidate to evaluate as a dependency or to cross-check against, after full vetting. Its CC BY 4.0 format docs are usable with attribution.

**SMB / SAB / SAT documentation**
- ShapeManager forked from ACIS around 2001. No public SMB spec was found ([search: fileinfo](https://fileinfo.com/extension/smb)).
- Fusion's import help says SAT/SMT/SMB is supported "only up to ACIS version 7" ([Autodesk help](https://help.autodesk.com/cloudhelp/ENU/Fusion-Designs/files/TPD-SUPPORTED-FILE-FORMATS.htm), [search]).
- **Public SAT docs:** Spatial's old "SAT Save File Format" document (ACIS R7-era) is mirrored at [paulbourke.net/dataformats/sat](https://paulbourke.net/dataformats/sat/) (with `sat.pdf` and a sample). It states the files "have an open format so that external applications, even those not based on ACIS, can have access to the ACIS geometric model" [search].
- CAD Exchanger says later revisions are not public ([blog](https://cadexchanger.com/blog/3d-formats-overview-acis/)) [search].
- Structure: a 1–2 line header, entity records (type name, params, terminated by `#`), and an `End-of-ACIS-data` marker [search].
- The Scribd copy ([link](https://www.scribd.com/document/323130267/04-frmt)) has unclear copyright. Do not use it.

**Clean-room needs**
1. ZIP reader with zstd (method 93).
2. Neutron stream decoder (length-prefixed serializer) for the manifest and design segments.
3. ASM BinaryFile reader:
   - The token stream is like SAB: tagged ints, doubles, strings and pointers.
   - Entity types: body, lump, shell, face, loop, coedge, edge, vertex, plus the geometry types `plane`, `cone`, `sphere`, `torus`, `spline` (procedural surfaces need evaluators) and `intcurve`.
4. A NURBS evaluator.
5. Optional parametric mapping of the design stream.
6. Corpus: our own Fusion files (from a contributor with a Fusion licence, dedicated CC0), since the Fusion 360 Gallery data is non-commercial (§5).

### 2.2 SolidWorks .sldprt / .sldasm

**solid-diff** (CosmicFrontierLabs; [README](https://raw.githubusercontent.com/CosmicFrontierLabs/solid-diff/main/README.md), [Cargo.toml](https://raw.githubusercontent.com/CosmicFrontierLabs/solid-diff/main/rust/Cargo.toml), [STATUS](https://raw.githubusercontent.com/CosmicFrontierLabs/solid-diff/main/docs/STATUS.md)) [raw]
- **License:** Cargo.toml says `license = "MIT"`, but the repo has **no LICENSE file** (LICENSE, LICENSE.md, LICENSE-MIT and COPYING all 404). GitHub search reports no detected license.
  - Treat it as **not yet safely reusable**. Read it only for orientation, or ask the authors to add a LICENSE.
- **Approach:** "chunk container → zlib sections → Parasolid XT → node graph → surface/curve evaluation → tessellation".
  - It meshes through **OpenCASCADE** (`opencascade-sys`, LGPL-2.1) by exporting STEP.
  - Under our policy we could not take that path as a static link.
- **Results:** on a 1,536-file PDM vault, 99.0% meshed; failures were pre-2015 OLE2 containers (1.0%). It reads 2015+ files with "no SolidWorks, no Windows and no commercial SDK".

**Other readers** [raw, links above]

| Project | License | What it does |
|---|---|---|
| `BlinkingSun/sldprt2step` | Apache-2.0 (LICENSE present) | Pure-Python XT neutral-binary reader that writes STEP AP214 exactly for analytic and B-spline geometry, and approximates intersection/SP curves and blends. Its README says it parses an OLE/CFB container. |
| `blussyya/sldprt-format-research` | MIT | Node.js reader with a format write-up for the modern container (CRC-32 per stream), the legacy OLE2 container, the display mesh, and the Parasolid B-rep "matches SolidWorks' own STEP export". **Feature tree not decoded.** |
| `schwitters/openswx` | MIT | C++20. Modern chunk-format detection by the byte marker `14 00 06 00 08 00` (2015+), OLE2 for ≤2014. Extracts properties, configurations, mass properties, previews and BOM. No B-rep. |
| `cadmpeg-codec-sldprt` | Apache-2.0 | Rust, L1. |

There is no public SolidWorks container spec. The geometry is Parasolid ([HOOPS docs](https://docs.techsoft3d.com/exchange/2024/start/format/solidworks_reader.html)) [search].

**Clean-room needs:**
1. A container walker for 2015+ chunks (zlib/deflate sections) and ≤2014 OLE2 (crate `cfb` 0.15.0 MIT, 70M downloads).
2. Locating the `Config-N-Partition` XT transmit.
3. Our XT reader, including the V14+ embedded schema deltas over the base `SCH_13006`.
4. Mapping XT topology and geometry onto our kernel.
5. Assemblies need reference resolution (paths to other `.sldprt` files) and transforms.

### 2.3 Parasolid XT (.x_t / .x_b) and the "Parasolid XT Format Reference"

**The document**
- Siemens' *Parasolid XT Format Reference* is not hosted on an official public Siemens page that we could find [search].
- Copies circulate:
  - October 2006 edition: [13thmonkey.org mirror](http://www.13thmonkey.org/documentation/CAD/Parasolid-XT-format-reference.pdf) (the copy cited by Wikipedia) and a [CNRS WordPress upload](https://stef2cnrs.wordpress.com/wp-content/uploads/2008/05/xt_format_oct06.pdf).
  - April 2008 edition on silo.tips.
  - Newer HTML/PDF copies on q-solid.com ([xt_index](http://www.q-solid.com/Parasolid_Docs/xt_index.html), [V35 PDF](http://www.q-solid.com/Parasolid_Docs_V35/pdf/xt.pdf)), described as labelled July 2022 and carrying the notice that the software and documentation "are proprietary to Siemens Industry Software Inc." [search].
- **Purpose, as quoted in search snippets of the manual:**
  - "these files have a published format so that applications can have access to Parasolid models without necessarily using the Parasolid kernel"
  - "The main audience for this manual is people who intend to write translators"
  - Four encodings: text, neutral binary, bare binary and typed binary.
- **Scope:**
  - Header and transmit structure; schema-driven node/field layout (`SCH_<version>_<schema>`; "it is not necessary to have a copy of this file [the schema] to understand the XT data format").
  - Topology: body, region, shell, face, loop, fin, edge, vertex.
  - Geometry: plane, cylinder, cone, sphere, torus, B-curve/B-surface, offset, swept, spun, intersection, SP-curve, blends.
  - Attributes.
- **Terms of use:** we found no explicit licence grant; the document is © Siemens. Implementing a reader from a format description is the normal clean-room route, and the format is explicitly published for translator writers.
  - **Recommendation:** record in an ADR that we implement from the published reference. We do **not** redistribute the PDF, do **not** commit Siemens schema files (`sch_*.s_t` ship with Parasolid / SolidWorks installs), and generate nothing from them. Derive field layouts from the reference text plus our own sample files.
  - Note: xt-parser's [SCHEMA-RECOVERY.md](https://raw.githubusercontent.com/akiselev/xt-parser/main/docs/SCHEMA-RECOVERY.md) says its schema code was generated from schema files found in a SolidWorks install (and "public PKToy copies"). Its [ORACLE.md](https://raw.githubusercontent.com/akiselev/xt-parser/main/docs/ORACLE.md) says it is validated with a Parasolid 37 DLL. Combined with the missing LICENSE file, that is a **provenance risk; do not reuse**.
- **Existing Rust code:**
  - `parasolid-core` 0.3.7 ("MIT AND Apache-2.0", monozukuri-ai/parasolid-kit, created 2026-09) claims Onshape V30/V13 and SolidWorks extracted-part profiles ([README](https://raw.githubusercontent.com/monozukuri-ai/parasolid-kit/main/README.md)) [raw, crates.io].
  - Its schema provenance is not yet checked. Vet it before any use.
- **Clean-room needs:**
  - Text and neutral-binary tokenisers.
  - Schema delta handling (V14+ embedded edits).
  - Node graph with pointer resolution.
  - Geometry evaluators, including intersection curves and blend surfaces, which need our own approximations or the chart data stored in the file.
  - Unit: metres.

### 2.4 Solid Edge (.par / .psm / .asm)
- Parasolid is the geometry engine, and HOOPS reads the Solid Edge B-rep "based on our Parasolid Reader" ([HOOPS](https://docs.techsoft3d.com/exchange/2024/start/format/solid_edge_reader.html)) [search].
- The container is reportedly OLE Structured Storage ([convert.guru](https://convert.guru/par-to-txt)) [search, unverified].
- No open-source reader was found. Approach: CFB + XT reader, the same pipeline as SolidWorks ≤2014.

### 2.5 Inventor (.ipt / .iam)
- Autodesk's ShapeManager kernel, derived from ACIS ([CAD Exchanger](https://cadexchanger.com/acis-to-ipt/)) [search].
- InventorLoader (GPL-2.0) treats it as OLE storage with an embedded ACIS model and imports parts only [raw].
- No permissive mature reader exists; `cadmpeg` reports L1 [raw].
- Approach: reuse the Fusion SMB/SAT reader inside a CFB container. Low priority.

---

## 3. Open formats

### 3.1 FreeCAD .FCStd
- **Format** ([FreeCAD wiki: File Format FCStd](https://wiki.freecad.org/File_Format_FCStd)) [search]:
  - A standard zip.
  - `Document.xml`: objects, their properties and parametric definitions. It is the only file used in console mode.
  - `GuiDocument.xml`: per-object view properties (color, line width, visibility).
  - Per-object shapes in OCCT native BRep text (`*.brp`/`*.brep`).
  - Usually also `thumbnails/Thumbnail.png`, and spreadsheet and other blobs.
- **OCCT BRep text format:** documented by Open CASCADE ([BRep Format spec, OCCT 7.9](https://dev.opencascade.org/doc/overview/html/specification__brep_format.html); source `dox/specification/brep_format.md`). It is ASCII with the header `CASCADE Topology V1/V2/V3` and sections for locations, curves, surfaces, triangulations and shapes [search]. Reading the spec to write our own parser is fine. Do not copy OCCT code (LGPL-2.1).
- **Mapping feasibility (T1):** Document.xml stores each object's type and properties.
  - `PartDesign::Body`, `Pad`/`Pocket` (Type, Length, Reversed, Midplane, Profile link), `Revolution`/`Groove`, `Fillet`/`Chamfer` (edge refs such as `Edge12`), `LinearPattern`/`PolarPattern`, `Mirrored`.
  - `Sketcher::SketchObject` with `<Geometry>` (LineSegment, Circle, ArcOfCircle, BSpline…) and `<Constraints>` (type codes, First/Second/Third geo IDs + PosIds, Value, expressions).
  - `Spreadsheet::Sheet` cells and aliases; `ExpressionEngine` on properties.
  - These map naturally onto a sketch + feature model.
  - The hard part is topological references (`Edge12`, and FreeCAD 1.0 element-map names). Plan: re-resolve them geometrically against the stored `.brp` per feature, and fall back to the T2 shape per object when that fails.
  - **Verdict:** T1 is feasible for the PartDesign + Sketcher + Spreadsheet subset. Everything else falls back to T2 from the embedded BRep.
- **Readers:** `cadmpeg-codec-freecad` Apache-2.0, L5 [crates.io/raw]. FreeCAD is LGPL-2.1+; use it for reference behaviour only and copy no code.
- **Samples:** FreeCAD ships `data/examples/{PartDesignExample,EngineBlock,AssemblyExample,…}.FCStd` ([CMakeLists](https://raw.githubusercontent.com/FreeCAD/FreeCAD/main/data/examples/CMakeLists.txt)) [raw]. Their licence is assumed to be the repo licence (LGPL-2.1+); verify before committing them as test fixtures. Using them as data in a test corpus (not linked code) is acceptable, but record it in THIRD_PARTY_LICENSES.

### 3.2 OpenSCAD .scad
- **Grammar:** there is no formal spec. The authoritative grammar is `src/core/lexer.l` + `src/core/parser.y` in the OpenSCAD repo (GPL-2.0+, [parser.y header](https://raw.githubusercontent.com/openscad/openscad/master/src/core/parser.y)) [raw]. Builtins are defined separately in C++ ([mailing list](https://lists.openscad.org/empathy/thread/ZEOKQS7RUN6LJWKKWXLU4SAEDUHKTDM5)) [search].
- **Usable documentation:** the OpenSCAD User Manual on Wikibooks (CC BY-SA, prose only). **Do not translate parser.y into Rust**, because that would create a GPL derivative.
- **Rust and other parsers:**

| Project | License | Note |
|---|---|---|
| `tree-sitter-openscad-ng` 0.7.1 (official `openscad/tree-sitter-openscad`) | MIT ([LICENSE](https://raw.githubusercontent.com/openscad/tree-sitter-openscad/main/LICENSE), "Copyright (c) 2021 Ian Boll") [raw] | Usable |
| `tree-sitter-openscad` 0.5.1 | MIT | Usable |
| `openscad-lsp` 2.0.2 | MIT OR Apache-2.0 | Usable |
| `openscad-rs` 0.1.0 | **GPL-3.0-or-later** | Forbidden [crates.io] |

- **Feasibility:** good for T1-as-code.
  - Write our own recursive-descent parser (or use the MIT tree-sitter grammar).
  - Transpile modules, functions, `for`/`if`/`let`, list comprehensions, `children()` and special `$fn/$fa/$fs` variables (dynamically scoped) into our code language.
  - Map CSG primitives (cube, sphere, cylinder, polyhedron, union/difference/intersection, linear_extrude, rotate_extrude, transforms).
  - Mesh-only operations (`minkowski`, `hull`, `offset`, `import`, `surface`, `projection`) need corresponding operations in our kernel or are flagged as unsupported.
  - Build a semantic test-suite by running real OpenSCAD out of process (GPL program used as a tool, not linked) to compare volumes.

### 3.3 STEP (ISO 10303-21; AP203/214/242)
- **Part 21:** ISO 10303-21 is sold by ISO; the current edition is 2016, and IANA notes "available from the ISO website… for a nominal fee" ([IANA p21](https://www.iana.org/assignments/media-types/application/p21)) [search].
  - The syntax is small and widely described: HEADER/DATA sections, `#id=ENTITY(...)`, typed parameters, complex instances.
  - **Free EXPRESS longform schemas:** AP242 Ed.1–Ed.4 at [MBx-IF EXPRESS Schemas](https://www.mbx-if.org/home/?p=613) ("courtesy of Jotne"), and AP203/214/242 from [STEP Tools](https://steptools.com/stds/step) [search].
- **Rust crates** [crates.io]:

| Crate | License | Status |
|---|---|---|
| `ruststep` 0.4.0 | Apache-2.0 | Release 2024-09. EXPRESS→Rust codegen plus a Part 21 deserializer; README lists write support as TODO ([repo](https://github.com/ricosjp/ruststep)) [search] |
| `truck-stepio` 0.3.0 | Apache-2.0 | 2024-09. truck shapes ↔ STEP, step-to-mesh sample [raw] |
| `step-io` 0.2.5 | MIT OR Apache-2.0 | 2026. "reads every mainstream AP, writes only modern AP242", "⚠️ Experimental" ([README](https://raw.githubusercontent.com/elgar328/step-io/main/README.md)) [raw] |
| `step-p21` 0.1.1 | Apache-2.0 | |
| `stepperoni` 0.1.0 | MIT OR Apache-2.0 | |
| `monstertruck-io` | Apache-2.0 | |
| `oxideav-step` 0.0.1 | MIT | Brand new |
| `openbim-step` | **AGPL** | Forbidden |
| `brepkit-io` | **AGPL** | Forbidden |

  - Note: the crate named `step` is unrelated (a numeric "stepping" trait).
- **Approach:**
  - Own zero-copy Part 21 tokenizer (fuzzed).
  - A typed subset generated from the EXPRESS longform at build time by our own small generator (tables are data, not code).
  - Mapping for geometry and topology, `STYLED_ITEM` colors, `PRODUCT` names, `NEXT_ASSEMBLY_USAGE_OCCURRENCE` assemblies, units and tolerances.
  - Use `step-io` / `truck-stepio` as differential oracles in tests.

### 3.4 IGES 5.3
- Version 5.3 (1996) is the last published IGES ([Wikipedia](https://en.wikipedia.org/wiki/IGES)) [search].
- Historically published by US Product Data Association / NIST.
- Possible open copy: eScholarship entry "Initial Graphics Exchange Specification IGES 5.3" ([link](https://escholarship.org/uc/item/3b41v8cd)) [search, terms unverified].
- FIPS 177 adopts IGES ([DTIC](https://apps.dtic.mil/sti/tr/pdf/ADA270049.pdf)) [search].
- As US-government work, older NIST IGES docs are likely public domain, but verify the specific PDF.
- **Readers:** `cadmpeg-codec-iges` Apache-2.0 and `monstertruck-io` Apache-2.0 [crates.io]. Nothing mature.
- **Approach:** fixed 80-column S/G/D/P/T sections. Support:
  - Analytic and NURBS curves and surfaces (100, 102, 104, 110, 112, 114, 116, 120, 122, 124, 126, 128).
  - Trimmed surfaces (142/144).
  - MSBO (186) and B-rep (502–514).
  - Then sew.

### 3.5 Mesh formats

| Format | Spec | Crate (version, license, downloads) |
|---|---|---|
| STL | de facto | `stl_io` 0.11.0 MIT, 5.0M |
| 3MF | [3MF Core Spec](https://raw.githubusercontent.com/3MFConsortium/spec_core/master/3MF%20Core%20Specification.md) **v1.4.0** "Published" (the spec page lists v1.3.0 as of 2025-02); repo [LICENSE](https://raw.githubusercontent.com/3MFConsortium/spec_core/master/LICENSE) BSD-2-Clause; members pledge royalty-free patent licensing ([3mf.io/spec](https://3mf.io/spec/)) [raw, search]; also ISO/IEC 25422 | `threemf` 0.8.0 0BSD (45k); `threemf2` 0.4.1 MIT/Apache; `lib3mf-core` 0.4.0 BSD-2; `lib3mf` 0.1.6 MIT (telecos). Beware the name confusion with the official C++ lib3mf. |
| OBJ | de facto (Wavefront) | `tobj` 4.0.5 MIT, 2.9M |
| glTF 2.0 | Khronos | `gltf` 1.4.1 MIT OR Apache-2.0, 10.3M |
| PLY | de facto | `ply-rs-bw` 4.0.1 MIT (maintained fork); `ply-rs` 0.1.3 MIT (2020) |

All are [crates.io] metadata as of 2026-10-10. Each still needs the PROMPT.md dependency vetting (repo, maintainers, RustSec) before it is added.

**Containers and compression:**
- `zip` 9.0.1 MIT.
- `zstd` 0.14.1 BSD-3-Clause (C binding).
- `ruzstd` 0.9.0 MIT (pure-Rust decoder; preferred for fuzzing and safety).
- `cfb` 0.15.0 MIT (OLE2).

**OCCT warning:** `occt-sys` / `opencascade-sys` are LGPL-2.1 and build OCCT **statically** ("Static build of the C++ OpenCascade CAD Kernel"). Static LGPL linking violates our policy, so do not depend on them.

---

## 4. Vendor APIs for feature-tree export (T1 plugins)

### 4.1 Autodesk Fusion API
- **Docs:** [Fusion API reference](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/fusion_Timeline.htm) (help.autodesk.com/cloudhelp/ENU/Fusion-360-API/…).
- **Language:** Python add-ins or scripts run inside Fusion. There is no headless mode (ezf3d README) [raw].
- **Versioning:** Fusion ships monthly; the API has no separate semver. Pin it in the add-in README as the "Fusion release month" (e.g. July 2026 update). Recent API changes include Electronics Python API (May 2026) and preview PMI timeline APIs "Introduced in version April 2026" ([May 2026 update](https://www.autodesk.com/products/fusion-360/blog/may-2026-product-update-whats-new/), [PMI timelineObject](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/PMIImportedNote_timelineObject.htm)) [search].
- **Enumeration:**
  - `app.activeProduct` → `adsk.fusion.Design`.
  - `design.timeline`: `Timeline.count`, `Timeline.item(i)` "in the order they appear", and `markerPosition` (read the current marker, do not assume it is at the end) ([Timeline](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/Timeline.htm)).
  - `TimelineObject.entity`: the feature, sketch, etc. It is null for groups, and some types throw ([TimelineObject](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/TimelineObject_entity.htm), [forum](https://forums.autodesk.com/t5/fusion-api-and-scripts-forum/unable-to-access-timelineobject-entity/m-p/7057329/highlight/true)). Wrap each call in try/except and record it as "unsupported".
  - Features: `ExtrudeFeature` (profiles, extents, operation), `Sketch.profiles`, `Sketch.sketchCurves`, `sketchPoints`, `geometricConstraints`, `sketchDimensions` ([ExtrudeFeature](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/fusion_ExtrudeFeature.htm), [Sketch.profiles](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/Sketch_profiles.htm)) [search].
  - Parameters: `design.userParameters` and `design.allParameters` (name, expression, unit, value). These were not confirmed by search in this session, so check them against the reference.
  - Rollback: `TimelineObject.rollTo` ([rollTo](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/TimelineObject_rollTo.htm)).

### 4.2 SOLIDWORKS API
- **Docs:** [help.solidworks.com API](https://help.solidworks.com/2022/english/api/sldworksapi/SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.IFeature~GetNextFeature.html). Use the version-specific tree (`/2025/` or `/2026/`) matching the target release; the 2026 page was not retrieved in this session.
- **Languages:** VBA macro (.swp), or a C#/VB.NET add-in via `SolidWorks.Interop.sldworks` (COM, Windows only).
- **Traversal:**
  - `ModelDoc2.FirstFeature()` → `IFeature.GetNextFeature()`. It returns features in *model definition order*, which can differ from FeatureManager order, and includes suppressed features.
  - Identify features via `GetTypeName2` + `GetSpecificFeature2`, not by name.
  - Sub-features via `GetFirstSubFeature`/`GetNextSubFeature`; `ProfileFeature` → `ISketch` ([GetNextFeature](https://help.solidworks.com/2022/english/api/sldworksapi/SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.IFeature~GetNextFeature.html), [Get Sketches example](https://help.solidworks.com/2022/English/api/sldworksapi/Get_Sketches_Example_CPlusPlus_COM.htm), [traversal C++](https://help.solidworks.com/2023/english/api/sldworksapi/FeatureTraversalCPP.htm)) [search].
- **Feature parameters:** `IFeature.GetDefinition()` → `I…FeatureData` (e.g. `IExtrudeFeatureData2`) ([forum](https://forum.mycad.visiativ.com/t/lire-les-propriete-dun-percage-taraudage-fraisurage-via-solidworks-document-manager-api/107145)) [search].
- **Dimensions:** `IFeature.GetFirstDisplayDimension`/`GetNextDisplayDimension` → `IDisplayDimension.GetDimension2` (name, `SystemValue`, equations via `IEquationMgr`). Not verified by search this session.
- **Sketch geometry:** `ISketch.GetSketchSegments`, `GetSketchPoints2`, `GetSketchRelations` (via `ISketchRelationManager`).
- Pin the target as "SOLIDWORKS 2024+ API, Interop assembly version X".

### 4.3 Onshape REST API
- **Docs:**
  - [onshape-public.github.io/docs](https://onshape-public.github.io/docs/api-intro/), with the API Explorer at cad.onshape.com/glassworks/explorer ([explorer doc](https://onshape-public.github.io/docs/api-intro/explorer/)).
  - The OpenAPI 3.0.1 base URL is **`https://cad.onshape.com/api/v16`** (per [API Evangelist listing](https://apis.apievangelist.com/store/onshape-partstudio-api/)) [search; confirm in the Explorer].
  - Pin the version in the path. Behaviour can differ by version: a forum expert notes v12 returns compressed queries where v1 returns geometry IDs ([forum](https://forum.onshape.com/discussion/comment/121643)) [search].
- **Auth** ([API Keys](https://onshape-public.github.io/docs/auth/apikeys/)):
  - API keys (access + secret) from the Developer Portal.
  - Basic auth `Authorization: Basic base64(access:secret)` is "Lowest security. For local testing only".
  - HMAC request signature (`On <accessKey>:HmacSHA256:<sig>`) is medium security.
  - OAuth2 is required for App Store apps.
  - Read keys from env only (PROMPT.md).
- **Endpoints:**
  - Features: `GET /partstudios/d/{did}/{wvm}/{wvmid}/e/{eid}/features` (confirmed by [forum use](https://forum.onshape.com/discussion/comment/82389)). It returns the feature list including sketches as BTMSketch features with entities and constraints.
  - Variables: `GET /variables/d/{did}/{wv}/{wvid}/e/{eid}/variables` ([forum](https://forum.onshape.com/discussion/25605/api-get-request-get-all-variables-over-all-configurations)).
  - Configurations: `GET /elements/d/{did}/{wvm}/{wvmid}/e/{eid}/configuration`.
  - Body details: `GET /partstudios/.../bodydetails` ([Tulip](https://support.tulip.co/docs/onshape-connector)).
  - Sketch info: `GET /partstudios/.../sketches`. Unverified this session; check in the Explorer.
  - FeatureScript evaluation: `POST /partstudios/.../featurescript`.
  - Parasolid/STEP export for the T2 fallback.
- **FeatureScript:** all Onshape features (including standard ones) are FeatureScript. Feature JSON carries `featureType` + parameters (BTMParameterQuantity expressions etc.), which our importer maps.
- **Limits** ([API Limits](https://onshape-public.github.io/docs/auth/limits/)) [search]:
  - Annual per-company allowances: about 2,500 calls per user per year (Free/Standard/EDU), 5,000 (Professional), 10,000 (Enterprise).
  - Over the limit, the API returns **402**. Rate limiting returns **429** with rate-limit headers ([Onshape blog](https://www.onshape.com/de/blog/cloud-native-cad-rest-api), [onshape-to-robot issue](https://github.com/Rhoban/onshape-to-robot/issues/170)).
  - Design the importer to use few calls: one features call, one variables call, one export call per Part Studio. Cache results, and back off on 429.

### 4.4 Blender (mesh only, out-of-process)
- **Command:** `blender -b -Y --factory-startup <file.blend> --python-expr "import bpy; bpy.ops.export_scene.gltf(filepath='<out>.glb', export_format='GLB')"` ([TIL example](https://til.jakelazaroff.com/blender/export-a-blender-file-to-glb-from-the-command-line/), [Khronos tutorial](https://github.khronos.org/glTF-Tutorials/BlenderGltfConverter/)) [search].
  - Pass explicit exporter options, since unset options take the user's settings.
  - A known regression made the exporter fail in background mode on macOS 2.91 ([T83188](https://developer.blender.org/T83188)).
- **Security:**
  - `.blend` files can embed Python (text blocks set to auto-run, drivers). `-Y`/`--disable-autoexec` blocks auto-run and overrides Preferences; `-y` enables it ([Blender manual: Scripting & Security](https://docs.blender.org/manual/en/2.80/advanced/scripting/security.html)) [search]. **Always pass `-Y`.**
  - Also use `--factory-startup`, so user add-ons and startup scripts don't run, and the deterministic exporter defaults.
  - Pass paths as argv after `--`, never interpolated into the expression.
  - Write our export script to our own temp dir, run Blender with a timeout, then parse the resulting glTF with our fuzzed importer.
  - Never parse `.blend` ourselves.
  - Blender is GPL, but invoking the user's installed binary as a separate process is not linking.

---

## 5. Test corpora (licence status)

| Source | Licence / terms | Usable for our repo corpus? |
|---|---|---|
| NIST MBE PMI test cases (CAD + STEP AP203/AP242, multiple CAD systems) ([NIST download](https://www.nist.gov/el/systems-integration-division-73400/mbe-pmi-validation-and-conformance-testing/download-free-cad), [data.gov record](https://catalog.data.gov/dataset/nist-cad-models-and-step-files-with-pmi)) | Catalog lists `nist.gov/open/license`. NIST-employee works are not copyrightable in the US (17 U.S.C. §105) but "may be subject to foreign copyright" ([NIST licence notes](https://pages.nist.gov/trojai/docs/license.html), [Fedora NISTSL](https://fedoraproject.org/wiki/Licensing/NISTSL)) [search] | **Yes**: best permissive STEP/native corpus. Record the provenance. Check the per-file notice because some native files may involve contractor work. |
| CAx-IF test files / STEP library ([mbx-if.org](https://www.mbx-if.org/home/?p=1310), [STEP Tools samples](https://www.steptools.com/docs/stpfiles/ap214/index.html)) | Terms not found [search] | Fetch for local quarantine testing. **Don't commit** until terms are confirmed; ask CAx-IF. |
| ABC dataset (1M Onshape models, STEP/Parasolid/meshes) ([NYU archive](https://archive.nyu.edu/handle/2451/44368), [paper](https://cims.nyu.edu/gcl/papers/2019-ABC-Dataset.pdf)) | Dataset release MIT, but "the copyright of these CAD models is owned by their creators", governed by Onshape Terms 1.g.ii [search] | Local testing (quarantine). **Do not redistribute** models in the repo. Store hashes and download scripts only. Good for XT (Onshape-written) and STEP fuzz seeds. |
| Fusion 360 Gallery Dataset ([LICENSE](https://raw.githubusercontent.com/AutodeskAILab/Fusion360GalleryDataset/master/LICENSE.md)) | **Non-commercial research only**; no full redistribution; Autodesk may terminate; indemnity clause [raw] | **No** for the repo or CI artifacts (our project allows commercial use). At most private local research. Avoid. |
| FreeCAD bundled examples ([CMakeLists](https://raw.githubusercontent.com/FreeCAD/FreeCAD/main/data/examples/CMakeLists.txt)) | FreeCAD repo LGPL-2.1+ (verify per file) | Yes, as test data with attribution (data, not linked code). |
| Thingiverse | Per-thing licence chosen by uploader; mostly CC BY / BY-SA / BY-NC / BY-NC-SA ([Jee & Sohn 2018 summary](https://copyrightevidence.org/wiki/index.php/Jee_and_Sohn_(2018))) [search] | Only CC0 / CC BY / CC BY-SA items. Record the licence and attribution per file; skip NC. Mostly STL, some .scad (good for the OpenSCAD route). |
| GrabCAD ([terms](https://grabcad.com/terms), [help: how models can be used](https://help.grabcad.com/article/246-how-can-models-be-used-and-shared)) | Site licence "non-commercial, internal use"; commercial use requires the designer's permission [search] | **No.** Do not use in repo or CI. |
| `sldprt-format-research` samples, `cadmpeg` corpus (CC0 donations) | MIT repo / CC0 corpus ([cadmpeg corpus](https://raw.githubusercontent.com/cadmpeg/cadmpeg/main/README.md)) [raw] | Likely yes; verify each file's licence. |
| Our own files | We author them | Best for proprietary formats. Ask contributors with SolidWorks/Fusion licences to create and dedicate CC0 samples. Keep a manifest (format, version, author, licence, sha256). |

All downloaded corpus files go into a quarantine dir (e.g. `corpus/quarantine/`, git-ignored). They are only parsed by our code and tests, and never opened with external programs (PROMPT.md).

---

## 6. Recommendations
1. **P0:** our own Part 21 STEP reader plus STL/3MF/OBJ readers. Use the vetted crates `stl_io`, `tobj`, `gltf` and `threemf`. Add an IGES reader next.
2. **P0/P1:** a clean-room **Parasolid XT reader**, the single biggest lever. It covers SolidWorks (and its embedded XT), Solid Edge, NX, Onshape exports and the ABC corpus. Write an ADR on the provenance rules:
   - Work from the published reference and our own samples.
   - Never use the Parasolid SDK/DLL or Siemens schema files.
   - Never copy code from `xt-parser` or `solid-diff` (no LICENSE files).
3. **P1:** FreeCAD T1 importer; Fusion .f3d T2 reader (ASM/SMB, shared with SAT); Onshape REST T1 importer (testable); Fusion and SolidWorks dumper plugins ("UNVERIFIED").
4. **P2:** OpenSCAD transpiler (own parser; the MIT tree-sitter grammar is fine for the editor), PLY, bare SAT/SMB.
5. **P3:** Inventor, Solid Edge, Blender bridge.
6. Evaluate `cadmpeg` (Apache-2.0, explicit clean-room policy, CC BY 4.0 format docs) as a reference or oracle. Do this before deciding whether to depend on it: it is very young, with about 300 downloads.
7. Use no OCCT crate: `occt-sys` statically links LGPL code.

---

## Injection attempts observed
None of the fetched content tried to direct the agent.
- **cadmpeg README:** contains ordinary `curl … | sh` and `irm … | iex` installer instructions for its own tool. These are normal README content, not directed at us. They were not run, and per PROMPT.md they must not be.
- **xt-parser docs:** describe validating against a Parasolid DLL under Wine and generating code from vendor schema files. This is a provenance concern recorded above, not an instruction.

## Gaps / to verify
These could not be fetched directly because WebFetch failed in this session.
- Exact Onshape endpoint list, sketch endpoint and current API version: confirm in the API Explorer.
- Fusion `userParameters` / `allParameters` pages.
- SOLIDWORKS 2026 `IDisplayDimension` pages.
- The XT reference front-matter terms: read the PDF itself before the ADR.
- CAx-IF file terms.
- IGES 5.3 PDF provenance.
- The `nist.gov/open/license` text.
