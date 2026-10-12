# Progress

Living status document. Any session should be able to resume from here and CLAUDE.md.

## Current phase
Phase 2 is done: `v0.1.0-mvp` was tagged on 2026-10-12 on commit cfc0e35 (CI and
Fuzz + Miri green). The owner published it on GitHub as a pre-release (the session's git proxy
refuses tag pushes); the tag triggers the release workflow after the milestone review, the re-review and two dogfooding runs.
Phase 3 has started (see Next).

## Done
- Research and design docs: `docs/RESEARCH.md` (+5 reports), `docs/DESIGN.md`, threat
  model, SECURITY.md, HARDWARE.md, PERFORMANCE.md (budgets), INTEROP.md, FORMAT.md;
  ADRs 0000–0015.
- Crates (see CLAUDE.md): units, sketch solver, kernel + mock, OCCT shim + safe kernel,
  naming, doc (code view), history, eval, store, project, commands (registry, ~40
  commands, agent policy), meshio, hw, sandbox, worker, render (wgpu + CPU), host,
  mcp, app (egui GUI), update (signed manifests), cli.
- End to end with real OCCT 8.0.1: CLI build/export, `gusset self-test`; GUI renders the
  bracket example; an agent built 5 reference parts through MCP with exact volumes
  (docs/DOGFOODING.md).
- GUI: menus/toolbar/command search generated from the registry, schema-generated
  command dialogs, browser, timeline, code view, close/recovery dialogs, AI agents panel
  with approvals, audit log and agent branches; kittest automation; screenshot gallery
  in docs/screenshots/.
- MCP: rmcp server (HTTP on localhost with token + stdio bridge), tool parity with the
  registry, `render_view`, agent branches, approvals, read-only mode, persisted audit.
- Standing "always allow" approvals from the in-app prompt (per action / all actions,
  optionally remembered), revocable in the Agents panel, suspended by the kill switch
  (ADR 0012 "Standing approvals"). Milestone security fixes merged (MCP, sandbox,
  storage, app); see docs/SECURITY_REVIEWS.md.
- CI: fmt/clippy/test on Linux x64/ARM64, Windows x64/ARM64, macOS ARM; cargo-deny,
  cargo-audit, license inventory, native OCCT builds + kernel tests, fuzzing (11
  targets) and Miri; release workflow (packages, SBOM, attestations; signing off).

## In progress
- Dogfooding friction fixes for agents (F-M1/M2/M10/M11/M14/M15/M17/M18/M19, polyline
  tool) on a worktree branch.
- IGES import: OCCT reads IGES only from a file, so the sandboxed worker gets a
  Landlock read rule for one private temp file (ADR 0008 §4); in a worktree branch.

## Done since the milestone review (2026-10-11)
- Security, storage, sandbox, MCP and GUI review fixes merged (docs/SECURITY_REVIEWS.md).
- Standing "always allow" approvals (ADR 0012).
- App: start-up routine (housekeeping, cache cap), "recover unsaved work" offer,
  SaveScheduler-driven auto-save/recovery, imported meshes drawn, cancellable imports.
- Naming: document-level golden scenarios with mock and OCCT kernels (H7); L3, L4 fixed.
- Solver: envelope Cholesky + Givens rank analysis, 19x faster on long chains (M4).
- Evaluation cache bounded by estimated memory, LRU, per-branch session cache (M10).
- History: snapshot schema 2 shares feature lists; ~370 B per parameter edit.
- Budgets measured: save 33 ms, snapshot 34 ms, rebuild_100 3.0 s / 0.12 s.
- CI green on all platforms (the GL backend is no longer used on Windows, ADR 0009).

## Next
1. Phase 3, in the feature-matrix order (docs/RESEARCH.md "v1" tier), starting with
   the modeling gaps agents and users hit first (dogfooding F-M4..M8):
   - v0.2: done: shell, mirror, linear/circular body patterns (OCCT golden tests;
     agent ergonomics F2-5..F2-11), offset/angled datum planes, body move/copy and
     combine (a single feature is patterned as its own body, then combined into the
     part; OCCT golden tests). Next: sketch patterns, sketch trim and spline curves,
     sweep/loft, holes.
   - v0.3: assemblies and joints (research Fusion joints vs SolidWorks mates first),
     branch merge, configurations/customizer.
   - v0.4: 2D drawings, DXF/SVG; FreeCAD/OpenSCAD import, Parasolid XT.
   - later: sheet metal, direct modeling, tier-2 native imports, CAM, simulation,
     plugins.
   Every milestone goes through the reviewer, security reviewer and dogfooding.
2. Post-MVP leftovers: project saves off the model thread (recovery snapshots are
   done, M3), fault-injection storage tests (M14), per-agent credentials for grants
   and reconnect to an agent's own branch (F2-2).
3. Release: the first Release run failed at the version check (workspace was 0.0.1);
   fixed in e05714d. The owner re-creates v0.1.0-mvp on a green head.

## Known gaps (not MVP scope per the spec; planned for later phases)
- Splines and feature-level patterns (patterns repeat whole bodies; combine merges one
  feature's body into the part) (dogfooding F-M4..M7).
- Sketch trim / region arrangement from crossing curves (F-M8).

## Decisions made without the owner (easy to reverse)
- **Branch workflow:** the session may only push to `claude/new-session-nedlj5`. That is
  also the repository's default branch, so it plays the role of "main". Feature
  branches are merged locally.
- **Repository visibility:** made public by the owner on 2026-10-11 (Actions minutes
  had run out). The OCCT build is still cached per target.
- The repository is named `CAD`, not `gusset`. Renaming is left to the owner.
- History-overhead budget restated per edit (≤ 450 bytes); a fixed fraction of the
  model size cannot hold for small models (PERFORMANCE.md).
- Performance budgets are enforced by release/OCCT tests instead of criterion benches
  (no new dependency).
- MSRV 1.89 for libraries (std file locks, rmcp); the GUI crate needs 1.95 (egui 0.36).
- Sketch coordinates stored in the document are snapped to 1e-6 mm when they are within
  1e-9 mm of the snapped value, for a readable code view.
- Container: a custom checksummed format instead of SQLite (ADR 0005). Two-slot
  recovery file with an extra slot marker; the Windows cache path follows the
  `directories` crate layout.

## Deviations from spec
- Right-to-left text rendering is a known egui gap (ADR 0009). Strings are
  externalized; RTL rendering waits on upstream.
- Mesh import (STL/3MF/OBJ, tier 3) produces **reference mesh bodies**, not solids: the
  kernel cannot build a B-rep from a triangle mesh. They are stored as binary STL blobs,
  displayed (`render_view`; the GUI viewport still has to draw `Evaluation::meshes`),
  listed by `query.model` (`reference_meshes`, with volume when watertight) and included
  in mesh exports; STEP export, booleans and edge/face references do not apply to them.
  Mesh files are parsed in-process by gusset-meshio (pure Rust, with size/count limits),
  off the model thread; STEP/IGES go to the sandboxed worker (ADR 0008 §4).

## Research gaps
- Reddit, and direct fetches of the FreeCAD and Onshape forums and HN, were blocked from
  this environment. User research relied on search-engine snippets of those sources.
  Re-run when access allows.
- WebFetch could not open vendor pages; some HARDWARE.md entries are marked unverified.
