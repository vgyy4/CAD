# Progress

Living status document. Any session should be able to resume from here and CLAUDE.md.

## Current phase
Phase 0 (research) → Phase 1 (architecture ADRs). Work on Phase 2 crates that don't
depend on open decisions (units/expressions, sketch solver) is starting in parallel.

## Done
- Repository initialised; spec in `PROMPT.md`.
- ADR 0000 name (**Gusset**), 0001 license, 0003 persistent naming, 0004 document model,
  0006 history model.
- `gusset-units`: unit-aware expressions, parameter tables, deterministic math.

## In progress
- Research reports in `docs/research/` (kernels, GUI and solver, MCP/SIMD/storage/crates,
  users and UX, interop).
- `gusset-sketch` (subagent, branch `feat/sketch-solver`).

## Next
- Synthesize `docs/RESEARCH.md`, `docs/DESIGN.md` and the feature matrix.
- Remaining ADRs: kernel, container format, save/recovery, threat model, GUI toolkit,
  design system, hardware tiers, MCP, release tooling, updater, code signing.
- CI skeleton (fmt, clippy, test, cargo-deny), pinned actions.

## Decisions made without the owner (easy to reverse)
- **Branch workflow:** the session may only push to `claude/new-session-nedlj5`. That is
  also the repository's default branch, so it plays the role of "main" in the spec.
  Short-lived feature branches (`feat/...`) are merged into it locally and never pushed
  separately.
- **Repository visibility is private.** That affects GitHub Actions minutes (2,000/month
  on the Free plan; Windows ×2 and macOS ×10 multipliers) and the availability of ARM
  runners. See the CI ADR.
- The repository is named `CAD`, not `gusset`. Renaming it is left to the owner.

## Deviations from spec
(none yet)

## Research gaps
- Reddit, and direct fetches of the FreeCAD and Onshape forums and HN, were blocked from
  this environment. User research relied on search-engine snippets of those sources.
  Re-run when access allows.
