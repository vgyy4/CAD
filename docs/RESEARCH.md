# Phase 0 research summary

Synthesis of the research reports in `docs/research/`. They hold the full evidence,
with a source URL for every claim:

| Report | Topic |
|---|---|
| [`research/users-and-ux.md`](research/users-and-ux.md) | What users love and hate in Fusion, SolidWorks, Solid Edge, Onshape, FreeCAD and OpenSCAD; daily vs rare features; UI/UX; full feature matrix |
| [`research/kernels.md`](research/kernels.md) | Geometry kernels (truck, monstertruck, Fornjot, OCCT bindings, OCCT itself), STEP parsers |
| [`research/gui-and-solver.md`](research/gui-and-solver.md) | Native Rust GUI toolkits, wgpu and software fallback, 2D constraint solvers, linear algebra |
| [`research/mcp-simd-storage-crates.md`](research/mcp-simd-storage-crates.md) | MCP spec 2026-07-28, rmcp, Claude Code; SIMD dispatch; project container options; crate vetting; release tooling; CI runners |
| [`research/interop.md`](research/interop.md) | Readers for f3d, sldprt, Parasolid XT, FCStd, scad, STEP, IGES and meshes; vendor APIs; test corpora |

Research dates: 2026-10-10. **Access gaps:** Reddit was blocked, and so were direct
fetches of HN, the FreeCAD and Onshape forums, and several documentation sites. User
research therefore relies on search-result snippets of those sources (each cited). The
Reddit cluster should be re-run when access allows (tracked in PROGRESS.md).

## 1. Users: what matters (frequency-ranked)

**Very common, cross-tool:** no existing tool is at once *free, private, offline,
polished and commercially usable*. Fusion has cloud lock-in and personal-use limits
(10 editable documents, non-commercial). Onshape's free plan makes every document
public and needs a connection. SolidWorks and Solid Edge are expensive (SolidWorks
especially) and Windows-only. FreeCAD is free but rough. OpenSCAD has no B-rep, no
fillets and no STEP. **That gap is Gusset's position:** an open-source, local-first,
cross-platform tool with a polished UI and an open file format.

| Tool | Most-loved | Most-hated |
|---|---|---|
| Fusion | CAD+CAM in one; parametric timeline; marking menu; approachable | licensing changes and cloud dependence (very common); editing rolls the model back and loses context; destructive history/direct switch; large-assembly lag |
| SolidWorks | S-key quick bar; FeatureManager + rollback bar; in-context editing; classic mates | cost (very common); Windows-only; crashes on large/imported data |
| Solid Edge | synchronous/direct editing of imported solids (Live Rules) | steep learning curve; two modes; Windows-only |
| Onshape | branching and merging, micro-versions, no save button; speed; stability; mate connectors | free plan public (very common); online-only (very common); large-assembly lag |
| FreeCAD | free/open/offline; Python; VarSets; 1.1 transparent previews and draggers | UX (very common, still #1); workbench confusion; topological naming (mitigated in 1.0, but fillets/chamfers still break: issues #26826, #29154) |
| OpenSCAD | code-as-model, diffable; Customizer | no fillets, B-rep or face references (very common); no STEP |

**Daily for everyone:** sketch + constraints, extrude/cut/revolve, fillet/chamfer,
measure. Hobbyists export STL/3MF daily. Professionals use STEP import, patterns and
drawings daily. (Inferred; no CAD feature-usage survey exists.)

**Lessons for us:**
1. Persistent naming through fillet/chamfer is the hard part. We designed for it from
   day one (ADR 0003) and give broken references a repair UX instead of silent re-picks.
2. Local-first versioning is Onshape's killer feature without the cloud (ADR 0006).
3. Code-as-model (OpenSCAD) **plus** B-rep with stable edge names (ADR 0004).
4. Edit-in-context with live transparent previews; no workbenches; a command palette.

The 22 derived UI rules are in `docs/DESIGN.md`.

## 2. Technology decisions (current state verified 2026-10-10)

| Question | Finding | Decision |
|---|---|---|
| Geometry kernel | truck: no crates.io release since 2024-09, limited fillet; monstertruck promising but young; **Fornjot shut down (archived 2026-06)**; opencascade-rs crates are themselves LGPL (static); cadrum links OCCT statically and downloads prebuilt binaries; occt-rs is AGPL. OCCT 8.0.1 (2026-07-30) builds on Windows ARM64 MSVC upstream | OCCT 8.0.1, shared, through our own C shim (ADR 0002) |
| 2D solver | SolveSpace/slvs GPL (banned); PlaneGCS LGPL with Eigen/Boost; ezpz (MIT) and sketch-solver (MIT/Apache) are references | our own LM + DOF/rank solver (`gusset-sketch`); dense self-written linear algebra for determinism (faer's runtime SIMD could break bit-reproducibility) |
| GUI | egui 0.36 (idle-efficient, AccessKit, kittest, wgpu 30); iced lacks accessibility; Slint's license is incompatible; GPUI is not usable standalone | egui/eframe (ADR 0009); RTL text is a known gap |
| No-GPU fallback | wgpu `force_fallback_adapter` → WARP / lavapipe / llvmpipe; GL backend best effort | fallback chain in ADR 0009 |
| MCP | **spec 2026-07-28** (stateless, `server/discover`, MRTR input-required replaces server→client requests; Roots/Sampling/Logging deprecated); rmcp 3.5.1 is dual-era; Claude Code requests 2026-07-28 with legacy fallback | rmcp 3.5.x, dual-era, Origin enforcement on, bearer token (ADR 0012) |
| SIMD | safe `#[target_feature]` stable since 1.86; AVX-512 intrinsics since 1.89; `std::simd` still nightly | x86-64-v2 / ARMv8.0 baseline + runtime dispatch, display-only (ADR 0011) |
| Project container | SQLite has no page checksums without an extension and needs `VACUUM INTO` for atomic replace anyway; redb's format changed in 3.0 | custom checksummed chunk container (ADR 0005) |
| Packaging | cargo-dist community-maintained, gaps for deb/rpm/AppImage/dmg; WiX v6+ under the OSMF EULA | in-repo workflow with focused tools; WiX v5 MSI (ADR 0013) |
| Updater | tough (TUF) heavy; velopack's tooling unvetted | own updater on minisign-verify with TUF-style rules (ADR 0014) |
| Signing | SignPath Foundation free but needs a **public** repo; Azure Artifact Signing about $9.99/month (individuals US/CA only); Apple $99/year | unsigned for now; ask the owner (ADR 0015) |
| CI runners | arm64 runners available to private repos since 2026-01-29; `macos-14` being retired | matrix in `.github/workflows/ci.yml` |

## 3. Interoperability (tiers and priorities)

| Format | Achievable tier | Route | Priority |
|---|---|---|---|
| STEP AP203/214/242, IGES | 2 (exact B-rep, with names/colors via XDE) | OCCT in the sandboxed worker | MVP |
| STL, 3MF, OBJ | 3 (mesh) | own Rust readers/writers (`gusset-meshio`) | MVP |
| glTF, PLY | 3 | `gltf` (MIT/Apache), own PLY | v1 |
| Parasolid XT (.x_t/.x_b) | 2 | clean-room reader from the published XT Format Reference (a Siemens document whose stated purpose is letting translators read XT) plus our own samples; never the Parasolid SDK/DLL or schema files | P1: unlocks SolidWorks, Solid Edge, NX and Onshape exports |
| SolidWorks .sldprt/.sldasm (2015+) | 2 | extract the embedded Parasolid stream (OLE2/`cfb` crate for older files) → XT reader. `solid-diff` has no LICENSE file, so its code is not reused | after XT |
| Fusion .f3d/.f3z | 2 | ZIP + zstd container; B-rep in ShapeManager `.smb` (SAT/SAB-like). Clean-room from public ACIS SAT docs plus our own samples; `ezf3d` (MIT) can be studied | later |
| FreeCAD .FCStd | 1 for the PartDesign/Sketcher/Spreadsheet subset, else 2 (OCCT BRep) | own XML mapping | v1 |
| OpenSCAD .scad | 1 (translate to code view/`Script` feature) | own parser (the official tree-sitter grammar is MIT; `openscad-rs` is GPL and banned) | v1 |
| Feature-tree export | 1 | thin plugins (Fusion Python add-in, SolidWorks macro/add-in, Onshape REST) → open interchange JSON → Rust importer | Phase 3 |
| Blender .blend | 3 | user's Blender with `-b -Y --factory-startup`, glTF export | later |

**Test corpora:** NIST models (US government, public domain in the US), FreeCAD examples
(LGPL data files used as test inputs only), and Thingiverse items under
CC0/CC-BY(-SA). ABC and CAx-IF files are for local testing only and are not
redistributed. Fusion 360 Gallery (non-commercial) and GrabCAD are not used.

## 4. Feature matrix
The full 34-row matrix (features × six tools with Y/P/N) is in
[`research/users-and-ux.md` §5](research/users-and-ux.md#5-feature-matrix). Our tags,
reconciled with the spec's MVP scope:

| Tag | Features | Justification |
|---|---|---|
| **MVP** | sketch (line/arc/circle) + constraints + DOF display; extrude add/cut; revolve; fillet; chamfer; parameters/expressions with units; code view; history tree with rollback; timeline tree (branches, undo tree); STEP/STL/3MF export; STEP/IGES/STL/3MF/OBJ import; measure; headless CLI; MCP server; crash safety | the spec's vertical slice; the daily core for every user segment |
| **v1** | shell, mirror, linear/circular patterns, booleans as features, sweep/loft, draft, holes/threads, text/emboss, configurations, customizer sliders, assemblies + joints, 2D drawings, direct edit (move/offset face), FreeCAD/OpenSCAD import, Parasolid XT, branch merge, DXF/SVG | daily for pros or very popular for printing; each builds on the MVP core |
| **later** | sheet metal, surfacing, CAM, FEA, generative design, rendering, real-time collaboration, native f3d/sldprt beyond XT, history-free mode | big domains; done after the core is robust |

## 5. Full branch merge: feasibility
Snapshots are ordered feature lists with stable names, so a three-way merge (base = the
lowest common ancestor node) works per feature:
- Taken automatically: features changed on only one side; parameters changed on only
  one side; features added on one side, inserted after their dependencies.
- **Conflicts:** the same feature or parameter edited differently on both sides; a
  feature deleted on one side and edited on the other; reorders that interleave;
  persistent references that no longer resolve after merging (detected by rebuild).
- The UI would present conflicts per feature, with "take ours/theirs/edit", followed by
  a validating rebuild.

This is feasible on our model and planned for v1 (cherry-pick ships in the MVP).
