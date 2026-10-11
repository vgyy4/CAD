# Progress

Living status document. Any session should be able to resume from here and CLAUDE.md.

## Current phase
Phase 2 (MVP vertical slice): feature-complete apart from the gaps below; the milestone
review (code review, security review, MCP dogfooding with the real kernel) ran on
2026-10-11 and its findings are being fixed before tagging `v0.1.0-mvp`.

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
- CI: fmt/clippy/test on Linux x64/ARM64, Windows x64/ARM64, macOS ARM; cargo-deny,
  cargo-audit, license inventory, native OCCT builds + kernel tests, fuzzing (11
  targets) and Miri; release workflow (packages, SBOM, attestations; signing off).

## In progress (milestone fixes; see docs/SECURITY_REVIEWS.md and docs/reviews/)
- GPU viewport in the GUI (gusset-render via egui_wgpu) + visual fixes; app-level review
  findings (open/close/recovery flows, file I/O off the UI thread).
- Storage/recovery fixes (untitled recovery, Save As project ids, housekeeping/GC,
  save modes, incremental snapshots, history limits).
- MCP/agent security (forged elicitation, kill switch, rate limits, export names) and
  wiring STEP/IGES/mesh import into CLI/app sessions.
- Sandbox allow-list, Windows DLL hardening, untrusted B-rep blobs in project files.

## Next
1. Modeling-core review items: persistent-naming golden tests (H7), sketch solver
   performance (M4), evaluation cache memory bound (M10), naming robustness (L3/L4).
2. Agent ergonomics from dogfooding (face-frame reporting, cut direction, zero-volume
   warnings, larger code-view text, per-body export, polyline/batch sketch tools).
3. Benchmarks + CI regression gate (rebuild_100, history_overhead, save_1000).
4. Re-run review + dogfooding; tag `v0.1.0-mvp` when CI is green.

## Known gaps (not MVP scope per the spec; planned for later phases)
- Pattern, mirror, shell, splines, offset/datum planes, body move (dogfooding F-M4..M7).
- Sketch trim / region arrangement from crossing curves (F-M8).

## Decisions made without the owner (easy to reverse)
- **Branch workflow:** the session may only push to `claude/new-session-nedlj5`. That is
  also the repository's default branch, so it plays the role of "main". Feature
  branches are merged locally.
- **Repository visibility is private:** Actions minutes are limited (Windows ×2, macOS
  ×10). The OCCT build is cached per target.
- The repository is named `CAD`, not `gusset`. Renaming is left to the owner.
- MSRV 1.89 for libraries (std file locks, rmcp); the GUI crate needs 1.95 (egui 0.36).
- Sketch coordinates stored in the document are snapped to 1e-6 mm when they are within
  1e-9 mm of the snapped value, for a readable code view.
- Container: a custom checksummed format instead of SQLite (ADR 0005). Two-slot
  recovery file with an extra slot marker; the Windows cache path follows the
  `directories` crate layout.

## Deviations from spec
- Right-to-left text rendering is a known egui gap (ADR 0009). Strings are
  externalized; RTL rendering waits on upstream.

## Research gaps
- Reddit, and direct fetches of the FreeCAD and Onshape forums and HN, were blocked from
  this environment. User research relied on search-engine snippets of those sources.
  Re-run when access allows.
- WebFetch could not open vendor pages; some HARDWARE.md entries are marked unverified.
