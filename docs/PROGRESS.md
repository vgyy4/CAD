# Progress

Living status document. Any session should be able to resume from here and CLAUDE.md.

## Current phase
Phase 2 (MVP vertical slice), well under way. Phase 0 research and the Phase 1 ADRs are
written (0000–0015, except 0016, which is not needed yet).

## Done
- Research: `docs/RESEARCH.md` plus 5 sourced reports; design guide `docs/DESIGN.md`;
  threat model; SECURITY.md; HARDWARE.md (generated sections, drift test);
  PERFORMANCE.md (budgets); INTEROP.md; FORMAT.md (container).
- Crates (see CLAUDE.md): units, sketch (solver), kernel + mock, OCCT shim + safe
  kernel, naming, doc (code view), history, eval, store, project, commands (registry,
  37+ commands, agent policy), meshio, hw, cli.
- **End to end with real OCCT 8.0.1:** `gusset build examples/bracket.gus.txt --export
  x.step --export x.3mf`. Sketch → extrude → fillet → sketch on face → cut → exports;
  the volume matches the analytic value exactly. `gusset self-test` passes.
- CI: fmt/clippy/test on Linux x64/ARM64 and Windows x64/ARM64 (required), macOS ARM
  (allowed to fail), cargo-deny, cargo-audit, and a pinned native OCCT build + kernel
  tests on all targets.

- `gusset mcp [file] [--read-only] [--no-attach]`: relays to a running app or serves
  headless (OCCT, mock kernel with a warning without OCCT); fd 1 is pointed at stderr
  and the protocol uses a private duplicate. `render_view` uses gusset-render (GPU,
  CPU fallback) via gusset-mcp's `render` feature. CLI stdio end-to-end test.

## Next
1. Verify `gusset mcp` against Claude Code in practice (with OCCT); wire the app's
   HTTP endpoint and renderer (`SceneRenderer`) into `gusset-app`.
2. GUI app (`gusset-app`, egui): layout per DESIGN.md, viewport, sketch mode, timeline,
   iteration graph, code view, parameters, close/recovery dialogs, kittest automation.
3. Sandboxed import worker (`gusset-worker`): STEP/IGES via OCCT out of process.
4. Release workflow + installers + updater; Miri/fuzz CI jobs; THIRD_PARTY_LICENSES.
5. Milestone review subagents (code + security), dogfooding subagent (5 parts via MCP).

## Decisions made without the owner (easy to reverse)
- **Branch workflow:** the session may only push to `claude/new-session-nedlj5`. That is
  also the repository's default branch, so it plays the role of "main". Feature
  branches are merged locally.
- **Repository visibility is private:** Actions minutes are limited (Windows ×2, macOS
  ×10). The OCCT build is cached per target.
- The repository is named `CAD`, not `gusset`. Renaming is left to the owner.
- MSRV raised to 1.89 (std file locks, rmcp).
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
