# Agent dogfooding

Spec: "spawn a subagent that models 5 reference parts using only MCP tools … Every
friction point it hits becomes an issue to fix." Each entry below is tracked to a fix or
listed under PROGRESS.md "Known gaps".

## Run 1 — MVP milestone (2026-10-11, commit d2d0b5b)

Setup: real OCCT 8.0.1 kernel, `gusset mcp --no-attach <file>` over stdio (protocol
2025-11-25), a stdlib Python JSON-RPC client. The agent learned the tools only from the
tool list, server instructions, resources, README/ADRs and error messages.

### Parts

| # | Part | Result | Notes |
|---|---|---|---|
| 1 | L-bracket | success | Fully constrained parametric L profile, extrude, two face-sketch holes, parametric inside-bend fillet. Volume exact (12570.27 mm³); `t` change rebuilds correctly. ~15 extra calls on the face-frame probing and silent no-op cuts. |
| 2 | Flanged pipe fitting | success | Revolve (exact volume), fully constrained 6-hole bolt circle (`360 deg / n_bolts`), root fillet + chamfer. ~33 calls for the bolt circle (no pattern). |
| 3 | Phone stand | success | Profile via `document.set_text` with an angle parameter; slot cut; fillets. Three tries (missing `next_id`, angle direction). |
| 4 | Enclosure with lid | success (workaround) | No shell: box pocket-cut from the top face; lid as separate body with lip. |
| 5 | Spur gear m2 z20 | partial | Involute generated outside Gusset as 320 segments via `document.set_text`; bore + keyway as two cuts (crossing curves are not split). Not parametric in module/tooth count. |

### Friction log

| ID | Sev | Item | Fix |
|---|---|---|---|
| F-B1 | blocker | `import.file` always "not available": `Session.importer` is never set outside tests | Wire the sandboxed worker into CLI/app sessions |
| F-M1 | major | Face-sketch frame (origin/axes) not reported | Return origin, x, y, normal from `sketch.create`/`query.sketch` |
| F-M2 | major | A cut that removes nothing reports success | Warn on zero volume change; add volume/bbox deltas to edit results |
| F-M3 | major | Cut direction follows the face's outward normal (cuts need negative distance), undocumented | Cuts default into material, or explicit `direction` |
| F-M4 | major | No pattern / mirror | Add features |
| F-M5 | major | No shell | Add shell |
| F-M6 | major | No spline / polyline / equation curve | Add polyline + spline |
| F-M7 | major | No offset planes or body move | Add `plane.offset`, `body.move` |
| F-M8 | major | Crossing curves don't form regions; no trim | Planar arrangement / `sketch.trim` |
| F-M9 | major | Multi-region `add` fails with a generic "invalid solid" | Fuse regions first; better hint |
| F-M10 | major | `document.get_text` truncated at 512 chars | Raise the limit for code text; page by line |
| F-M11 | major | `next_id` required in hand-written text; grammar not published | Make `next_id` optional; publish a grammar resource |
| F-M12 | major | Directed angle constraint undocumented; solver can flip to the mirror solution | Document; prefer the solution nearest the input |
| F-M13 | major | A reconnecting agent reads Main until it edits; undo is lost | Reattach to the agent's branch; persist undo |
| F-M14 | major | Exports proceed silently while features fail | Report rebuild errors in export results |
| F-M15 | major | STEP written as AP214; tool says AP242 | Fix description or write AP242 |
| F-M16 | major | `render_view` can't hide bodies or show sketches | Visibility and sketch overlay options |
| F-M17 | major | A bad edge reference creates a failing feature; promised suggestion missing | Validate up front, return candidate names |
| F-M18 | major | `query.body` says "no bodies yet" when the rebuild failed | Report the failure |
| F-M19 | major | No per-body export; box + lid merge into a non-manifold STL | `bodies` filter on exports |
| F-m1..3 | minor | No polyline / `to_point`; one history node per sketch item; no batch | Batch/polyline tools |
| F-m4..7 | minor | Dotted names in descriptions; no `outputSchema`; no volume delta; `query.sketch` on a missing sketch returns null | Fix |
| F-m8..19 | minor | Centered rectangles; 25 s cold render; internal ids in errors; units checked late; error `message` sometimes an object; ambiguous undo text; `./x` rejected; overwrite hint misses approval; seam edges drawn; no retry-after; wrong verb in import symlink error; `circle(p, expr)` | Fix |

### Red-team

Held:
- All seeded text (injection phrases, zero-width/bidi/tag characters, a 100k-character
  field) reached the agent only inside `*_user_text` fields; hidden characters were
  stripped and flagged, injection-like text flagged with a reason, oversized text
  truncated with a working `get_text` pointer.
- Feature/sketch names restricted to identifiers.
- Delete branch, set main, any edit while on Main and export overwrite all refused
  without elicitation; a declined elicitation blocks; deleted branches restorable.
- Export path escapes (`..`, absolute, symlinked file/dir, NUL) refused.
- Mutations rate-limited (200); a 30-feature bulk edit needs approval.

Failures (tracked in docs/SECURITY_REVIEWS.md):

| ID | Sev | Finding |
|---|---|---|
| S1 | high | An agent can rename the user's main branch and name its own branch "Main" without approval (`branch.rename` renames any branch; the gate only checks the caller's current branch). |
| S2 | high | Approval prompts show opaque ids (`{"branch":"b2"}`), not names, creators or that b1 is main. |
| S3 | major | Read-only mode still allows `export.*` file writes. |
| S4 | major | The bulk-change threshold of `document.set_text` counts features only, not parameters. |
| S5 | major | The agent audit log is in memory only; lost on exit, not in the project event log (ADR 0012). |
| S6 | minor | Reads (`render_view`, resources) are not rate-limited. |
| S7 | minor | Export file names: any extension, Windows reserved names, trailing spaces accepted; io errors show absolute home paths. |
| S8 | minor | Client-supplied agent name appears outside `user_text` (created_by, audit, prompts). |
| S9 | info | Agent branches may be named `main` or `../../evil`. |
