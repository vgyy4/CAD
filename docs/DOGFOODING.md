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
| F-M6 | major | No spline / polyline / equation curve | Add polyline + spline (both done; no equation curves) |
| F-M7 | major | No offset planes or body move | Add `plane.offset`, `body.move` |
| F-M8 | major | Crossing curves don't form regions; no trim | Planar arrangement / `sketch.trim` (done, ADR 0016) |
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

## Run 2 — after the milestone fixes (2026-10-12)

Same setup as run 1 (real OCCT 8.0.1, `gusset mcp --no-attach`, stdlib Python client,
tools learned from the tool list, resources, errors and docs).

### Parts

| # | Part | Result | Calls | Notes |
|---|---|---|---|---|
| 1 | L-bracket | success | ~20 | Fully constrained profile via `document_set_text`; holes on a face sketch (frame reported, right first time); volume 11272.58 mm³ exact; parameter change rebuilds. |
| 2 | Flanged fitting | success | ~12 | Revolve + 6-hole bolt circle in one sketch (generated text); fillet and chamfer; volumes hand-checked. |
| 3 | Phone stand | success | ~10 | First try; `tilt` 60/70° rebuild without flipping. |
| 4 | Enclosure + lid | success (no shell) | ~12 | Nested regions extruded by region id; lid as a separate body; wall change rebuilds both. |
| 5 | Spur gear m2 z20 | success (approximate) | ~8 | 400-point involute in ONE `sketch_add_polyline` call; bore + keyway. Not parametric in module/teeth. |

STEP/STL exports with the `bodies` filter worked; STEP re-import gives exact volume;
STL imports as a reference mesh.

### Run-1 items
Fixed: F-B1, F-M1, F-M2, F-M6/F-m1 (polyline), F-M10, F-M11, F-M14, F-M15 (description),
F-M17, F-M18, F-M19, F-m6. Partly: F-M3 (cuts still need a negative distance, now
documented and warned about). Still open: F-M4/M5/M7 (pattern, mirror, shell, offset
planes, body move: later phases), F-M13 (see F2-2). Not exercised: F-M12, F-M16.

### New friction

| ID | Sev | Item | Fix |
|---|---|---|---|
| F2-1 | major | No IGES export through MCP (the CLI has it) | Fixed: `export_step` writes IGES for `.igs`/`.iges` |
| F2-2 | major | A reconnecting agent starts on Main; its earlier undo stack is gone | By design for now: names are never reused (code review L6), so an agent continues old work with `branch_switch`; undo is session-only (ADR 0006). Revisit with per-agent credentials |
| F2-3 | major | Relative project path breaks exports | Fixed: absolutized at start-up |
| F2-4 | major | `op: add` with a disjoint region silently makes a multi-solid body | Fixed: warning when the solid count grows |
| F2-5 | minor | Over-large fillet radius commits a broken feature | Fixed: sizes validated up front (`details.max_usable_mm`); `committed_with_errors` + `undo_hint` |
| F2-6 | minor | "Nothing to export" does not mention rebuild errors | Fixed: `details.rebuild_errors` |
| F2-7 | minor | No pattern/mirror/shell | Later phases (F-M4/M5) |
| F2-8 | minor | Polyline result lists every id; frame repeated in constraint results | Fixed: `{first, last, count}` ranges; frame only in `sketch.create`/`query.sketch` |
| F2-9 | minor | Numeric noise (−5e−22), normals on curved faces, seam edges | Fixed: noise snapped to 0, null normals on curved faces, `seam` flag |
| F2-10 | minor | Text grammar not published as a resource; directed-angle convention inferred | Fixed: `gusset://docs/text-grammar` resource, angle convention documented and tested |
| F2-11 | minor | No `body_name` argument | Fixed: optional `body_name` on new extrude/revolve bodies |
| F2-12 | minor | STEP is AP214 (documented) | Later: AP242 |
| F2-13 | minor | Descriptions use dotted command ids, tools use underscores | Fixed (with a test) |

## Run 3 — v0.2 features (2026-10-13)

Setup: as in run 2 (real OCCT 8.0.1 kernel, `gusset mcp --no-attach`, one long-lived
stdlib Python client per project, protocol 2025-11-25), against the v0.2 tool surface
(31 tools plus the 25 `commands` run through `run`: gears, holes, shell, sweep, loft,
mirror, patterns, datum planes, body move, combine, trim, sketch patterns). Parts 1-5
were modelled by a first agent that a usage limit stopped before it wrote its notes; the
findings for them are reconstructed from `calls.log` (tool, bytes, seconds per call; no
arguments), `stderr.log`, `notifications.log` and the saved `.gus`/text files, so they
cover only what those can show. Part 6 was modelled by a second agent that started from
`tools/list` and `commands`.

### Parts

| # | Part | Result | Calls | Time | Notes |
|---|---|---|---|---|---|
| 1 | Spur + helical gear pair (`generate_gear`) | saved (`p1_gears.gus`, 3 renders) | 53 | 225 s | 14 `run` calls took 104 s (3.6-17.7 s each), 5 `feature_delete` took 73 s (11-18 s each), the one successful render of the pair took 32.8 s. Two bulk-edit approvals (42 parameters) declined. 2 renders crashed. |
| 2 | Plate with holes (`feature_hole`, sketch pattern) | saved | 45 | 1.1 s | 8 constraint calls; the only edit between the two text versions marks the three auto-projected face points `construction`. |
| 3 | Enclosure (`feature_shell`, lip) | saved | 45 | 0.9 s | One render crashed right after the shell (call 142 of the log), fine after a server restart. |
| 4 | Sweep + loft | saved | 48 | 0.8 s | 16 `sketch_add_constraint` and 3 `history_undo` calls. |
| 5 | Trim / cam sketch | saved | 31 | 0.5 s | `sketch_trim`, `sketch_pattern`; 11 `run` calls. |
| (6') | Last session of the first agent (parameters, fillets, undo, cancel probe; no model file) | unfinished | 31 | 215 s | A cancel probe was sent, then `query_model` took 213 s. |
| 6 | L bracket: datum planes, mirror, body pattern, combine, hole, pin | success | ~45 to the finished model, 89 logged with discovery, error probes and repros | 18 s server time | Base + leg by extrude, gusset triangle on an offset datum plane, `feature_mirror` across a second datum plane, 4-hole tool body patterned in X then Y, a pin body, three `feature_combine` calls. |

Part 6 volumes check by hand: base 12000 + leg 13500 = 25500 mm³; each gusset 450 x 4 =
1800 mm³ (mirror copy at x 43..47 for the original at 13..17, so exact); the four holes
cut 4 x pi x 3² x 5 = 565.49 mm³, the pin hole pi x 5² x 5 = 392.70 mm³: 28141.81 mm³
final, and 36641.81 mm³ after `base_w` 60 -> 80 (hand value matches). Images:
`dogfood3/img/p6_gussets.png`. The datum-plane, mirror, pattern and combine tools worked
first time with exact numbers; `feature_plane` and `sketch_create` return the frame, which
made the plane sign conventions checkable without reading the source. Missing names
(`NoPl`, `Nope`) were rejected with a hint listing the existing ones.

### Findings

| ID | Sev | Finding | What would have helped |
|---|---|---|---|
| F3-1 | high | `render_view` panics once the scene mesh is larger than an earlier render's: `thread 'tokio-rt-worker' panicked at crates/gusset-render/src/gpu.rs:682:50: slice offset 0 size 174648 is out of range for buffer of size 135192` (also 7872 vs 288 and 57744 vs 9720). The client sees only `render_failed: the renderer crashed` (178 bytes). Hit 3 times by the first agent (gear pair after edits, again after 3 undos, enclosure after `feature_shell`) and reproduced in part 6: render the bracket, `feature_pattern_linear` (6 copies, 90 mm) then `render_view` fails, and keeps failing on retry; after `history_undo` it renders again; part 3 only recovered after restarting the server. The result is that the visual check, the agent's main verification, silently disappears exactly when a model grows. | Grow (recreate) the vertex/index buffers when the mesh outgrows them; until then, say what is wrong in the error ("scene too large for the cached buffers, retry"). Add a regression test: render small, add geometry, render again. |
| F3-2 | medium | `sketch_add_rectangle` (x 0, y 0, `width_expr: base_w`) leaves 2 degrees of freedom (position) and does not say so. After `param_set base_w 80 mm` the solver widened it symmetrically: `bbox_min_mm` x = -10, `bbox_max_mm` x = 70 (`query_sketch`: `p1 = [-10, 0]`, `dof: 2`, `UnderConstrained`). The datum planes, the hole pattern and the pin sketched at absolute positions then no longer matched the plate (second hole on the plate edge), with `rebuild_ok: true` and no warning. Only found by checking the bbox after the parameter change. Fixed in the run with `sketch_add_constraint kind: fixed, entities: [p1]` on both sketches. | Pin the corner (`fixed`) when a rectangle is added at a given x/y with an expression dimension, or return `dof` and an `underconstrained` warning in the `sketch_add_rectangle` result as the constraint tools do. |
| F3-3 | medium | Features that cannot build are still committed, without the `committed_with_errors` / `undo_hint` that F2-5 added for fillets: `feature_extrude` of a sketch without a closed region returned `rebuild_ok: false, new_errors: ["Base: BaseSk has no closed regions ..."]`; `run feature_pattern_linear` with `count: "1"` committed `Pattern1` with `new_errors: ["... a pattern needs between 2 and 500 instances, not 1 ..."]`. Neither names the undo step, and later edits stack on the broken node. | Validate these arguments before committing (as for fillet sizes) or add the same `committed_with_errors` + `undo_hint` fields to every edit result with `rebuild_ok: false`. |
| F3-4 | medium | Cancellation: a `notifications/cancelled` for a running `run generate_gear` (teeth 60, module 1, 7203 edges) produced no response (the request never completes for the client, as the spec allows) but the edit was committed anyway (`Gear1` present in `query_model`; `query_model` sent right after waited 3.8 s behind the model lock). The first agent's cancel probe was followed by a `query_model` that took 213.13 s (`calls.log`; its own output file `cancel_out.txt` is empty, so what ran during those 213 s is not recorded). A cancelled edit that lands anyway is a surprise to the user who pressed cancel, and a 3.5 minute stall of a read-only call suggests the lock is held until the cancelled work finishes. | Abort evaluation at the next checkpoint and not commit; or document that cancel is best-effort and return a final `cancelled`/`committed` result. Reads should not wait on a cancelled write. |
| F3-5 | medium | Slow calls in part 1 (gears): `feature_delete` 11.4, 13.9, 13.9, 15.4, 18.2 s (73 s for 5 deletes), `run` 3.6-17.7 s (generate/sweep/edit of the helical gear), render of the finished pair 32.8 s (the 25 s cold render of run 1 is still there for heavy models; the simple bracket's first render was 4.8 s, later ones 0.05 s). Total 225 s for 53 calls where parts 2-5 took 1 s each. No progress notification was seen in `notifications.log` for these (only 2 server requests in it, both elicitations). | Progress notifications for calls over ~2 s; delete/undo should reuse the cached rebuild of unaffected features; tessellation cache for the render. |
| F3-6 | medium | No way to rename a parameter. The first agent changed the generator prefix `gear1_*` to `pinion_*`/`wheel_*` by rewriting the whole text (85 KB, 592 lines, read back in 3 pages of ~30 KB: 8 `document_get_text` calls of 25-31 KB) and `document_set_text` twice asked for approval (`notifications.log`: "changing 42 features or parameters at once needs the user's approval ... parameters removed: 42 (gear1_module, ...)"). The client policy declined both; the approval text shows the first 200 characters of arguments only, so the user cannot see why 42 parameters "disappear" (a rename). | A `param_rename` command (updating references, like `feature_rename`); count a rename as one change in the bulk-edit check. |
| F3-7 | medium | A patterned hole group takes 7 calls by the documented recipe (datum plane, sketch, circle, extrude as tool body, `pattern_linear` X, `pattern_linear` Y, `combine` cut) while `feature_hole` with `at: [[x, y], ...]` does it in one. Nothing in `feature_pattern_*`, `feature_combine` or `commands` points a user who thinks "pattern of holes" to `feature_hole` or `sketch_pattern`; I only found `feature_hole` by reading the whole `commands` list, and its `at` description is just `Or [["10 mm","5 mm"]].` and `points` is `Sketch: points, circle centres.` (what "Or" refers to, and which of `at`/`points` to use, is unclear). | In the pattern/combine descriptions: "for holes use feature_hole". Rewrite the `at`/`points` descriptions. |
| F3-8 | medium | `sketch_add_circle`, `sketch_add_polyline` and `sketch_add_arc` take numbers in mm only (only dimensions accept expressions), so a pin at `cx: 30` stayed at x = 30 after `base_w` went 60 -> 80 (the plate centre moved to 40). Making it follow needs an extra `distance` constraint call per feature. The mirror/gusset side worked because `feature_plane offset` takes expressions (`base_w / 4 - 2 mm`, `base_w / 2`). | Allow `cx_expr`/`cy_expr` (like `radius_expr`) or a `center: "base_w / 2"` form. |
| F3-9 | low | `commands` summaries are cut mid-sentence: `feature_shell`: "Hollow a body to a wall thickness, removing the selected faces (e.g." and `feature_pattern_circular`: "... unite the copies with it, e.g.". | Cut at the sentence boundary before the `e.g.`. |
| F3-10 | low | Default `iso` view looks from the -Y side, so for a part whose features sit on +Y (gussets, holes) it shows only the back of the leg (`img/p6_iso.png`); the gussets were visible only with an explicit camera (`eye [110,120,80]`). I guessed `camera: {azimuth, elevation}`; the error was clear (`unknown field azimuth, expected one of eye, target, up, orthographic`). | Named views `iso_back`/`iso2`, or an `azimuth`/`elevation` form; or render a second view automatically when features are hidden. |
| F3-11 | low | `history_undo` replies `Undo: edited b2 → n41` where `n41` is the node that was undone, not the new head (unchanged since run 1: ambiguous undo text). | "Undid n41; now at n40". |
| F3-12 | low | `run feature_combine` with `body: Bracket, tools: [Bracket]` says `'Base' is both the target and a tool`: the name of the first feature, while the hint beside it lists the body name `Bracket`. | Use the body name the caller passed. |
| F3-13 | low | `feature_plane` description does not give the offset direction; XZ goes along -Y (so `offset: "-1 mm"` is +Y), only stated in the text-grammar resource. The result's `frame` showed it, but only after the call. | One line in the description: "offset along the normal (XY +Z, XZ -Y, YZ +X)". |
| F3-14 | low | `sketch_add_rectangle` marks `width` and `height` (numbers) required even when `width_expr`/`height_expr` are given; the first call with only the expressions failed `missing field width`. | Make the numbers optional when an expression is given. |
| F3-15 | low | Render: small tick marks and an edge line continuing past the gusset corner on the base plate in `p6_gussets.png`; the default colours are dark grey on dark grey. A 56 KB `run` response (call 234 of the first agent's last session) and 30 KB `document_get_text` pages cost context. | Clip edge overlays at silhouette ends; cap or summarise very large `run` results. |

### Status of the run 3 findings (2026-10-13)
| ID | Status |
|---|---|
| F3-1 | fixed: geometry revisions unique per process (`render` regression test reproduces the panic) |
| F3-2 | fixed: a rectangle with any `*_expr` gets its corner dimensioned (`at_x`/`at_y`) |
| F3-3 | fixed: every edit that breaks the rebuild returns `committed_with_errors` and `undo_hint` |
| F3-4 | fixed: a cancelled tool call is skipped if queued, undone if it was running |
| F3-5 | fixed for edits (8-14 ms on the gear pair) and spur rebuilds (8.6 s -> 2.2 s); helical sweeps are OCCT-bound (see PERFORMANCE.md) |
| F3-6 | fixed: `param_rename` with prefix mode (a gear's parameters move together) |
| F3-7 | fixed: pattern/combine descriptions point to `feature_hole` |
| F3-8 | fixed: `cx_expr`/`cy_expr`, `x_expr`/`y_expr`; new `h_distance`, `v_distance`, `at_x`, `at_y` constraints |
| F3-9, F3-11 to F3-14 | fixed |
| F3-10 | fixed: `azimuth`/`elevation` camera and the `iso_back` view |
| F3-15 | large results are paged through `get_text`; the "ticks" are real 1 mm edges of the model (the gusset profile's corner sits at z 5.5 mm, 0.5 mm above the plate; `query_body` lists `edge(Gusset/cap(start))`, 1 mm), not a renderer fault |

Not seen in this run: crashes in the kernel, wrong volumes, mirror/pattern/combine/datum
plane errors, or a confusing error from the new commands beyond the items above. Not
exercised by part 6: `feature_move_body`, `pattern_circular`, `sketch_trim`,
`import_file`/exports (covered by parts 1-5 in the first agent's logs only by tool name).

## Run 4 — v0.3 features (2026-10-15)

Setup: real OCCT 8.0.1 kernel, `gusset mcp --no-attach <file>` over stdio (protocol 2025-11-25), a
long-lived stdlib Python client per project (run 3's `daemon.py`/`c.py`, plus a batch helper `cl.py`
and `g`/`start.sh` wrappers; the scripts, saved files and renders stayed in the session's scratch directory). The agent learned the tools from `tools/list`
(31 tools), `commands` (the 41 commands run through `run`), the `gusset://docs/text-grammar`
resource and error messages. Binary: `cargo build --release -p gusset-cli` with `GUSSET_OCCT_DIR`
(copied to `bin/gusset`). The repository working tree was being edited by another session while this
ran (HEAD moved to ca12830 during the run, 26 files modified and uncommitted at 10:19), so the binary is
"ca12830 plus whatever was uncommitted at 10:14-10:19". Log: `calls.log` (tool, bytes, seconds, no
arguments), 265 calls, 77.5 s server time in total, of which 42.5 s is one render (see F4-13).

### Parts

| # | Part | Result | Calls | Notes |
|---|---|---|---|---|
| 1 | Hinge: `leafA.gus`, `leafB.gus`, assembly `hinge_asm.gus` | success | 49 | Two leaves (knuckle + web + 25 x 40 x 2 plate, 2994.64 mm3 each) inserted with `assembly_insert_component`, one `revolute` joint (`angle: open`, limits 0..200 deg), `study_motion`, `query_interference`, `query_bom`, 2 renders. |
| 2 | Four-bar linkage: `bar.gus` (parametric bar with 2 holes), `fourbar.gus` | success | 37 | One bar document inserted 4 times with `set: {len: ...}` (ground 60, crank 30, coupler 70, rocker 50 mm); 3 free pivots; swept 0..360 deg in 13 steps; all 13 steps match the hand solution to 3 decimals. |
| 3 | Parametric box with ranges, 2+1 configurations, component using one: `box.gus`, `boxes.gus` | success | 55 | `min/max/step` on 3 parameters; `Small`, `Large`, `Tall` (partial); components `BoxS`/`BoxL` select configurations; a `set` override on top. |
| 4 | Drafted, shelled tray with embossed and engraved text: `tray.gus` | success | 51 (+20 in the rebuild, see F4-2) | 60 x 40 x 20 box, `feature_draft` 5 deg on the 4 sides, `feature_shell` 2 mm open top, `feature_text` GUSSET embossed on the slanted front face and engraved in the floor, `feature_hole` drain. |
| 5 | Branches: change, compare, merge, conflict with `prefer` | success | 30 | `thick_wall` branch (wall, tilt, drain hole) vs agent branch (tilt); `branch_compare`, conflict reported, `prefer: ours` and `theirs`, merge into Main needs approval (declined by the client policy). |

All `.gus` files rebuild with `gusset build <file> --branch agent/dogfood4-1` (checked at the end:
hinge 56 ms, four-bar, boxes 9 ms, tray 1.3 s). Note the work lives on the agent branch
`agent/dogfood4-1`; the files' Main is empty (see F4-1).

Renders in `img/`: `p1_hinge60.png`, `p1_hinge135.png`, `p2_fourbar.png`, `p3_boxes.png`,
`p4_tray_frontview.png`, `p4_tray_front.png`, `p4_tray_bottom.png`, `p4_tray_iso.png`, `p5_merged.png`.

### Hand checks

| Check | Hand | Gusset |
|---|---|---|
| Leaf volume: knuckle+web section (pi 16 + 10 - 7.91586) x 19 + plate 25 x 2 x 40 | 2994.64 | 2994.642686 |
| Hinge centroid of leaf B at 30/60/90/120 deg: (-11.7468 cos a, y, 11.7468 sin a) | (-10.173, 5.873), (-5.873, 10.173), (0, 11.747), (5.873, 10.173) | identical, 3 decimals |
| Hinge folded flat (180 deg): the two plates coincide | 2000 mm3 at centroid (17.5, 20, 0) | 2000.0 at (17.5, 20.0, 0.0) |
| Joint limit 120 deg, sweep to 150 and 180 | refused | refused with the limit in the message |
| Bar volume 38 x 8 x 3 - 2 pi 2^2 x 3 | 836.60 | 836.601776 (ground 60 mm: 1556.60) |
| Four-bar, crank 60 deg: coupler and rocker angles from the circle intersection (a 30, b 70, c 50, d 60) | 15.487 / 63.31 deg (mirrored, see F4-3) | `built_values` 15.487232 / 63.309814 |
| Four-bar, closing point C for 13 crank angles 0..360 | e.g. 60 deg: (82.458, -44.672); 180 deg: (28.333, -38.694) | coupler-end and rocker-end positions equal each other and the hand value at all 13 steps |
| Crank 200 mm cannot close: gap = \|BD\| - (b + c) = 177.76 - 120 | 57.76 mm | "57.7639 mm and 84.8227 deg apart" |
| Box 30 x 20 x 15 / 80 x 60 x 40: volume, area | 9000 / 192000; 2700 / 20800 | exact |
| Small box inside the large one: interference | 9000 mm3 at (15, 10, 7.5) | 9000.0 at (15.0, 10.0, 7.5) |
| Drafted prism (60 - 2 z t)(40 - 2 z t) integrated over 20 mm, t = tan 5 deg | 44582.0990 | 44582.098966 |
| Shell: drafted body minus cavity (floor 2 mm, walls 2 mm normal to the sloped faces, open top) | 11424.161774 | 11424.161774 (also tilt 8/wall 3: 16084.479, tilt 10/wall 4: 20162.088, both exact) |
| Emboss then engrave the same string | equal and opposite | +22.009312 / -22.009312 mm3 (letter area 44.02 mm2 x 0.5 mm) |
| Merge `prefer: ours` (tilt 4, wall 3, drain hole pi 3^2 x 3) | 16333.4359 | 16333.435947 |
| Merge `prefer: theirs` (tilt 6, wall 3, drain) | 16162.5391 | 16162.539149 |

### Findings

| ID | Sev | Finding | Repro | Suggested fix |
|---|---|---|---|---|
| F4-1 | medium | A component source whose model lives on an agent branch is "empty": `assembly_insert_component` (default branch main) fails with `'leafA.gus' has no bodies` and the hint `its bodies: ` is empty. The agent has to guess `branch: "agent/dogfood4-1"` (it is the only place the work exists, since agents do not edit Main). | Model in `leafA.gus`, then `run assembly_insert_component {path: "leafA.gus"}` in another project. | When main has no bodies but other branches do, say so: `hint: "main has no bodies; branches with bodies: agent/dogfood4-1"`. |
| F4-2 | medium | There is no save tool: the `.gus` is written only when the server exits normally (closing stdin). The tray session's server was killed during the interruption (usage-limit stop) and `tray.gus` never existed; the whole part had to be rebuilt by script (20 calls). No recovery file turned up next to it. The other six files were saved because their servers had been shut down before the stop. A source for `assembly_insert_component` also cannot be used while its server is still running. | Model, kill the process, look for the `.gus`. | A `project_save` command (and autosave on every edit or every N seconds in `--no-attach` mode), or tell the agent in the tool result/instructions that the file is written on exit. |
| F4-3 | medium | The sign of a revolute `angle` depends on the orientation of the frame's Z, which for a cylindrical face depends on whether it is a boss or a hole, and nothing says so or reports it. Barrel (boss) of the hinge: positive angle turns the leaf from -X towards +Z. Pin hole of a bar: `angle: 60 deg` put the crank at -60 deg (clockwise seen from +Z), and the solved free angles (+15.49, +63.31) are the mirror of the counter-clockwise hand solution. `query_joints` returns the values and the moved body's centroid but not the frames (origin, Z, X). | Four-bar: `feature_joint {type: revolute, body: Crank, at: Crank/part(Bar/side(BarSk/c10)), to: Ground/part(Bar/side(BarSk/c10)), angle: 60 deg}`, then `query_joints` / bbox. | Return `at_frame`/`to_frame` (origin_mm, z_axis, x_axis) in `query_joints`; document the sign rule in the grammar; or orient cylinder frames independent of boss/hole. |
| F4-4 | medium | Joint frame placement by trial: (a) the frame of a planar face is the face's centre (centroid), so the cap of a knuckle that carries a web is 0.18 mm off the barrel axis (centroid (-0.178, 21, 0)); the circle `edge(...)` (partial arc, uses the full circle's centre and normal) was needed. (b) Without `flip` the first joint turned the body 180 deg about X and put its knuckle on top of the other one (`rebuild_ok: true`, no warning); with `flip` a positive `offset` moves along the flipped Z, i.e. into the other body (a 2 mm gap needed `offset: -2 mm`). | Hinge: `feature_joint {at: edge(LeafB/part(Knuckle/cap(start)), ...c2)), to: edge(LeafA/part(Knuckle/cap(end)), ...c2)), flip, offset}`. | Result of `feature_joint` with the placed frame and the body's bbox change (like extrude's `body_changes`); document "offset is along the target Z after `flip`, positive = into the part"; warn when a joint creates interference (cheap with `query_interference`). |
| F4-5 | low | A closing joint without `free: [angle]` fails with `0.0001 mm and 0.1773 deg apart`, a mismatch that looks like a solver tolerance problem. The grammar does say the three other pivots need `free`, but the message does not point at it. With `free: [angle]` on the closing joint its `built_values.angle` is -0.177 deg, which means nothing physical for a pin. | `feature_joint J4 ... angle: 0 deg` closing the loop without `free`. | When the position gap is below ~1e-3 mm and only the angle is off: "the closing joint's own angle is fixed; add free: [angle] to it". Hide or explain the solved value. |
| F4-6 | medium | Range violations surface as misleading errors. (a) `study_motion` beyond the parameter's `max` returns for those steps `errors: ["Hinge: 'open': unknown parameter 'open'"]`, `value: {"error": "'open' is outside its range ..."}` (a string at the other steps, an object here) and the bodies at their unplaced position. (b) `assembly_configure_component set {h: "500 mm"}` (max 60) is committed (`committed_with_errors`), the body disappears and the message is `BoxExt: 'h': unknown parameter 'h' (fix it in its own document, then update the component)`, a wrong cause and a wrong remedy. | (a) `param_set open ... max 120 deg`, `study_motion open 0..180`. (b) see left. | Name the range and the limit in both places; refuse the component `set` before committing (as `param_set` does); keep `value` a string and add `error`. |
| F4-7 | low | `query_components` omits what its description promises (`body`) and the picked `configuration` and `branch`, which `document_get_text` shows. An agent cannot tell which configuration BoxS uses without reading the text. | `assembly_configure_component {component: BoxS, configuration: Small}`, `query_components`. | Add `body`, `branch`, `configuration`. |
| F4-8 | low | Parameter ranges: `step` is neither enforced (w = 42 and 43 accepted with min 20, step 5) nor described (a hint for sliders?); `step: 0 mm` and `min > max` are accepted, and the error for the latter is `outside its range (20 mm to 10 mm)`; the message repeats the name (`w: 'w' is outside its range`). `max` and `min` are enforced hard on `param_set` and in configurations/components, so for an agent a "customizer range" is a hard limit. | `param_set {name: w, expr: 42 mm, min: 20 mm, max: 100 mm, step: 5 mm}`. | Say in the `param_set` description that min/max are enforced and step is a UI hint (or enforce it); validate `step > 0`, `min <= max` up front. |
| F4-9 | medium | `feature_draft` with angle 0 fails (`the draft angle must be between 0 and 90 degrees (molded parts typically use 1 to 3 deg; negative leans the faces outward)`), so a parameter range that includes 0 breaks the model at its lower end; the message contradicts itself (negative angles are accepted: -3 deg built, bbox 61.05). 95 deg gives the same text. A face that cannot be drafted is reported as `face 2 cannot be drafted`, an index and not a name. | `param_set tilt 0 deg` with `feature_draft ... angle: tilt`; `feature_draft` on `cap(start)`. | Treat 0 as a no-op; one consistent range text (`-90 < angle < 90`); name the face. |
| F4-10 | low | `feature_text` and `feature_hole at:` take coordinates in the face's own frame, which the result does not show. For the slanted front face (frame origin (0,0,0), x +X, y up the slope) and the floor (x +X, y -Y, normal -Z) I had to call `sketch_create` on the face and undo it. | `feature_text {plane: TrayExt/cap(start), at: [...]}` | Return `frame` and the letters' bbox in the feature result (as `sketch_create` and `feature_plane` do). |
| F4-11 | low | Numerical noise in `body_changes`: `bbox_min_mm: [-1e-7, -1e-7, -1e-7]` (and -1.00000004e-7) after text/hole features on bodies that start at 0 (F2-9's snapping does not apply here). | tray after `feature_text`. | Snap bbox values in `body_changes` like the other results. |
| F4-12 | low (setup) | `cargo build --release -p gusset-cli` does not rebuild `gusset-kernel-worker` / `gusset-worker`; the stale worker (protocol 1, later 5) gives `note: the kernel worker did not start: the geometry kernel worker speaks protocol 1, the app 4` and `not installed next to this program`, and the kernel then runs inside the MCP process, so a kernel crash would end the session. That note goes to stderr only; the MCP client is not told. The built workers also disagreed with the app (5 vs 4) because the tree changed under the build. | Build as in the task, run `gusset mcp`. | Say in `diagnostics`/instructions that the kernel is in-process; build the workers with the CLI (workspace default-members or a build script). |
| F4-13 | low | One `render_view` (iso, 900x700, tray) took 42.5 s right after a server restart (the next renders of the same model took 0.03-0.1 s). The machine was loaded by a `find /` and another session's builds, so this is only a hint at the 25 s cold render of run 1, not a measurement. First renders of simple models: 2.1 s. | Start server, build tray, `render_view`. | Warm the renderer at start-up; progress notification after 2 s. |
| F4-14 | info | `branch_compare` shows differences between two heads only, not which side changed since the common iteration, so before `branch_merge` an agent cannot predict the conflicts (it did report them correctly and changed nothing without `prefer`). | `branch_compare` of the two branches. | Optional `base: true` three-way view. |

### Status of the run 4 findings (2026-10-19)

| ID | Status |
|---|---|
| F4-1 | fixed: a source with bodies only on other branches names them in the hint |
| F4-2 | fixed: headless `gusset mcp` saves 3 s after each change, not only on exit |
| F4-3 | fixed: `to_frame` / `at_frame` (origin, x, z as placed) in the joint result and `query_joints`; grammar says angles turn about that z |
| F4-4 | fixed: a joint whose moved body overlaps another one warns with the interference; frames are reported as placed |
| F4-5 | fixed: when freeing the closing joint's angle would close the loop, the hint says "add free: [angle] to J" |
| F4-6 | fixed: a parameter out of range is reported as such; `study_motion` keeps `value` a string and adds `value_error`; component `set` checks the part's ranges |
| F4-7 | fixed: `branch_user_text`, `configuration_user_text` in `query_components` |
| F4-8 | fixed: min above max and step <= 0 are errors; `step` documented as the slider step |
| F4-9 | fixed: angle 0 is a no-op, the range reads -90 to 90, faces are named |
| F4-10 | fixed: `feature_text` / `feature_hole` return the frame their positions are in |
| F4-11 | fixed: bbox rounded to 1e-6 mm |
| F4-12 | fixed: the diagnostics resource says when the kernel runs in-process; CLAUDE.md lists the worker binaries |
| F4-13 | mitigated: the headless server warms the renderer at start (device and pipelines; 93 ms here, first render then 5 ms). No progress notification yet |
| F4-14 | fixed: `branch_compare base: true` (each side since the base, items both changed) |

No prompt-injection attempt was seen in tool output, resources or files in this run (component paths,
branch names and parameter comments all arrived as `user_text` with `suspicious: false`).

### What worked well

- Assemblies end to end: `assembly_insert_component` (pinned stored copy, faces named `Name/part(...)`), `set` overrides (one bar document gave four different links), `assembly_configure_component` with configurations, `query_components` with `update_available`, and a `query_bom` that counts mirrored leaves as one part (2 x 2994.64 mm3).
- `feature_joint` + `study_motion`: exact positions, limits enforced on driven and solved values, interference at each step (3.28 mm3 at 160 deg, 145.75 at 170, 2000.0 at 180) and no change to the document.
- Closed loops: `free` angles solved a Grashof four-bar at 13 positions with all assembly modes consistent and equal to the hand solution; an impossible loop gives the gap in mm and degrees (57.76 mm, correct by hand).
- Parameter ranges are enforced everywhere (`param_set`, configurations, component `set`) and shown in `query_model`; configuration save/apply/`current` flag behave and a partial configuration (`Tall`, one parameter) works. `configuration_apply` of an unknown name and `assembly_configure_component` with an unknown configuration list the existing ones.
- Draft, shell, text and hole produced exact volumes against hand integrals (draft 44582.0990, shell 11424.1618, tilt/wall sweeps, text +/-22.009312 mm3); the text outline is readable from the outside of the face (not mirrored) on both slanted and flat faces; parameters (`tilt`, `wall`) rebuild the whole chain (draft, shell, text, hole) in about 1 s.
- Branches: `branch_create from node`, `branch_compare`, `branch_merge` (a conflict is reported with ours/theirs and changes nothing, `prefer` resolves it, "already contained" on a repeat, three-way semantics correct on a later merge), merge into Main needs approval and the prompt names the branch and who made it (S2 is fixed). `history_undo` text now says `now at n32` (F3-11).
- Errors were precise about argument names (`unknown field name, expected one of component, set, configuration`), `committed_with_errors` + `undo_hint` appear for every broken joint/draft.

### Not tested

- `assembly_update_component` and `update_available: true` (a source that changed after insertion).
- Cylindrical/slider/planar/ball joints, `distance_limits`, gear-style joint relations (only revolute).
- Components inside components, configurations on a component that is itself an assembly.
- Cancellation, rate limits and read-only mode with the new commands; the in-app GUI paths.
- STEP/3MF export of an assembly (bodies placed by joints) and re-import.
- `branch_cherry_pick` (only merge) and merging a feature changed on both sides (only a parameter conflict).
- The kernel worker isolation (the worker was stale, F4-12), so a kernel crash on draft/shell/text was not exercised.

## Run 5 — v0.4 features (2026-10-19)

Setup: real OCCT 8.0.1 kernel, release build at b9f0e2f of `gusset-cli`, `gusset-kernel-worker` and
`gusset-worker` together (`CARGO_TARGET_DIR=target/occt`, so the workers were current), `gusset mcp
--no-attach <file>` over stdio (protocol 2025-11-25). Client: run 4's stdlib Python daemon/`cl.py`
helpers, one server per project; scripts, inputs, `.gus` files, exports and renders in the session's
scratch directory (`dogfood5/`). The agent learned the tools from `tools/list` (31 tools), `commands`
(58 commands run through `run`, schemas via `commands {name}`), the server instructions and error
messages. Input files written for the run: `plate.dxf` (inches, `LWPOLYLINE` with a bulge, `CIRCLE`,
`LINE` + `ARC` slot), `odd.dxf` (no `$INSUNITS`, SPLINE/ELLIPSE/TEXT/INSERT), six `.scad` files, and
`.FCStd` files zipped by hand (`Document.xml`, `GuiDocument.xml`, BRep text). Gusset has no BRep export,
so the BRep text came from a 10-line C++ program against the OCCT install (`BRepPrimAPI_MakeBox` /
`MakeCylinder` + `BRepTools::Write`, which writes "Topology V3"). Log: `calls.log`, 209 calls,
2.3 s server time in total. The slowest call was a fillet at 0.14 s. Drawing renders took 0.05-0.10 s,
exports 0.01-0.05 s and imports 0.01-0.12 s. No call was slow.

### Parts

| # | Part | Result | Calls | Notes |
|---|---|---|---|---|
| 1 | L bracket `bracket.gus` → drawing D1 (A3, first angle) | success, with layout problems (F5-3, F5-4) | 93 | 60 x 40 x 40, t 5, 2 x Ø`hd` through holes (`feature_hole`), fillets R3 inside/R8 outside. `standard_views` (front/top/right/iso), 6 dimensions (2 lengths, distance, diameter, radius, angle 90°), 2 notes, title block, full section A-A through the holes (`of: Top, side: top, section: A, cut_at: -20 mm`), detail DetB 4:1 of the bend, bottom and back views. Exported PDF (A3, 1 page), SVG and DXF (R2000 and R12; layers VISIBLE/HIDDEN/THIN/DIM/TEXT/CENTER/BORDER). Views and dimension texts moved, `hd` 8 → 10 mm updated the Ø text, and deleting `FilOut` left a magenta "Rout?" mark plus an issue. Undo restored it. |
| 2 | DXF plate `plate.gus` (inches) → extrude 0.25 in → drawing PD (A4, 1:2) | success after a wrong first extrude (F5-7) | 31 | `sketch_import_dxf`: `unit_mm: 25.4`, 3 regions, bulge arc correct. Dimensions 76.2 / 63.5 / Ø12.7 / R31.75 / R5.08. PDF and DXF exported. Re-importing the exported drawing DXF into a sketch works (632 lines, SOLID/TEXT skipped and counted). |
| 3 | OpenSCAD `scad.gus` (`part3*.scad`) | success, but `$fn` is ignored (F5-1) | 12 | Modules, `for`, `function`, list comprehension, `let`, difference/union/intersection, `rotate_extrude` (also `angle: 90`), `linear_extrude`, `mirror`, `scale`, `cylinder r1/r2 center`, `echo` all match by hand (table below). Unsupported constructs: see F5-9. |
| 4 | FreeCAD `fc.gus` (`two.FCStd`, `inj.FCStd`, `nogui.FCStd`, broken variants) | success | 13 | The visible box was imported (6000 mm3) and the hidden cylinder skipped. Without `GuiDocument.xml` both were imported. A missing or damaged `.brp` gives a clear error per object. An injected label arrived as `label_user_text` with `suspicious: true`. |
| 5 | Agent-safety checks | mostly OK, but read-only mode writes (F5-2) | 47 | Export paths `../x`, absolute and `img/../x` were refused. A missing folder was refused with a hint. Overwriting an export asks the user through elicitation; it was declined and nothing was written. `--read-only` refuses every drawing and import command and `export_step`. Item names must be identifiers, so injected text cannot get into issue strings. Notes, OpenSCAD `echo` and FCStd labels were flagged `suspicious`. |

All four `.gus` files rebuild with `gusset build <file> --branch agent/dogfood5-1` (bracket 0.11 s,
plate 0.06 s, scad 0.25 s, fc 0.16 s). Renders are in `img/` (`p1_sheet0..5.png`, `p1_pdf-1.png`,
`p2_sheet.png`, `p3_scad.png`).

### Hand checks

| Check | Hand | Gusset |
|---|---|---|
| L profile 60x5 + 5x35, depth 40 | 19000 | 19000.0 |
| 2 holes Ø8 through 5 mm | -502.6548 | -502.654825 |
| Fillets: + (9 - 9π/4)·40 inside, - (64 - 64π/4)·40 outside | 18025.2211 | 18025.221135 |
| Drawing values: overall 60, top flat 60 - 5 - 3 = 52, Ø8 → Ø10 after `hd`, R8, 90°, hole pitch 20 | as hand | as hand; Ø changed to 10 at once |
| Section A-A (first angle, below the top view, viewed towards -Y): profile with +X to the right and +Z down | flange left, pointing down | as hand (checked against a `back` base view) |
| DXF plate area: 3 x 2.5 in + half disc r 1.25 - hole r 0.25 - slot (1 x 0.4 + π 0.2²) | 9.232356 in² = 5956.3468 mm² | region a6 5956.346806 |
| Plate volume x 6.35 mm | 37822.8022 | 37822.802217 (with `regions: [a6]`) |
| SCAD `bar(4)`: 40x20x5 - 4 π 2² 5 | 3748.6726 | 3748.672588 (exact circles, not 64-gons; OpenSCAD itself gives 3749.08) |
| SCAD ring (rotate_extrude of a 2x5 square at r 10) 220π | 691.1504 | 691.150384 |
| SCAD wedge, cube ∪ sphere (1000 + 523.599 - 523.599/8), rotated cube | 150 / 1458.1489 / 200 | 150.0 / 1458.148929 / 200.0, bbox ±7.071068 |
| SCAD rotate_extrude angle 90 of circle r2 at 10: ¼·2π10·4π | 197.3921 | 197.392088 |
| SCAD cone r1 5 r2 0 h 9 centred; intersection of cubes; mirror(scale(cube)) | 235.6194 / 125 / 48, x -4..0 | 235.619449 / 125.0 / 48.0, bbox 66..70 |
| SCAD `circle(d = 10, $fn = 6)` extruded 2 mm (hexagon) | 129.9038 | **157.079633** (a full circle, F5-1) |
| FCStd box 30x20x10 | 6000 | 6000.0, 12 edges, 6 faces |

### Findings

| ID | Sev | Finding | Repro | Suggested fix |
|---|---|---|---|---|
| F5-1 | high | OpenSCAD `$fn`/`$fa`/`$fs` are ignored and every circle, cylinder and sphere is exact, without a warning. For `$fn = 6` (the usual nut-trap and hex-socket idiom) this gives the wrong shape: 157.08 instead of 129.90 mm3 (+21 %), and the across-flats size becomes the diameter. For large `$fn` the difference is negligible (bar: 3748.67 vs OpenSCAD's 3749.08). The import result's `tier` says "exact geometry", but nothing says that the facet settings were dropped. | `linear_extrude(2) circle(d = 10, $fn = 6);` and `cylinder(h = 2, d = 10, $fn = 6);` → `import_file`. | Build an explicit small `$fn` (say ≤ 12, or any `$fn` set on the primitive) as the polygon OpenSCAD makes. Otherwise warn per line: "line 1: $fn = 6 ignored, an exact circle was used". |
| F5-2 | medium | Read-only mode writes the project. A refused write (`drawing_add_note` → `read_only`) still creates the agent's branch (`agent/dogfood5-3`, now current), and on exit the `.gus` is rewritten with it. A session that only called `branch_list` also changed the file's bytes (same size, different md5). | `cp bracket.gus t.gus; gusset mcp --no-attach t.gus --read-only`, then `run drawing_add_note …` and `branch_list`, close, `cmp`. | Do not create the agent branch in read-only mode (refuse before the branch step), and do not save a read-only session. |
| F5-3 | medium | Automatic placement of dimension and label text collides. On the bracket, "60", "52" and "20" were drawn on top of each other (Overall and Flat get the same offset, and Pitch from the top view reaches up to the same line). "R8", "90°" and the detail label "DetB" overlapped. On the plate, "Ø12.7" and "R5.08" overlapped, and two dangling marks were drawn at the same spot. `issues` stays empty, so an agent only sees this by rendering. | Part 1: add Overall (distance, horizontal), Flat (length) and Pitch with default offsets, then `render_view {drawing: D1}`. | Stack parallel dimensions of one view at increasing offsets, and move text that overlaps other text or geometry. Report the overlaps that remain in `issues`. |
| F5-4 | medium | Layout problems are not reported. `issues: []` came back while: (a) `standard_views` on A4 at 1:1 put the right view at x -12.8 mm (`off_sheet: true` in `views`, which is the only sign); (b) a projected view `of: Top, side: top` took exactly the box of the existing section view on the same side; (c) a base view and the detail's caption overlapped the title block; (d) a note at x = 900 mm on a 420 mm sheet was accepted. A projected view cannot be moved sideways (`drawing_move_item` says "a projected view follows its parent"), so (b) can only be fixed by deleting it or changing `gap`. | Part 2 `drawing_create {size: A4, standard_views: true}`; part 1 `drawing_add_view {of: Top, side: top}` twice. | Put `off_sheet`, view/view and view/title-block overlaps and off-sheet notes into `issues`. With `standard_views`, choose a standard scale that fits the sheet. Place a second projected view on the same side further out. |
| F5-5 | medium | Invalid drawing items are committed as successful edits. A diameter on a straight edge, an angle between parallel edges, sheet scale `abc` / `-1` / `0`, view scale `1e9`, detail `radius: -5 mm`, and a `cut_at` that misses the part or is `abc` all returned `rebuild_ok: true` and `new_errors: []`. The error shows only in `placed.error`, or later in `query_drawing.issues` / render / export (`dimension D1: a diameter dimension takes a circular edge`, `sheet scale: 'abc': unknown parameter 'abc'; using 1:1`). Feature commands put such problems in `new_errors` with an undo hint. | `drawing_add_dimension {kind: diameter, a: <line edge>}`; `drawing_create {scale: "abc"}`. | Refuse kind/edge mismatches and values that cannot be evaluated at add time. Report the rest in `new_errors` / `rebuild_ok: false` like features do. |
| F5-6 | medium | A length dimension in the isometric view shows the projected length: the 52 mm edge reads "42.46" (52·√(2/3)). This is a wrong value on the drawing, and no issue is raised. | `drawing_add_dimension {view: Iso, kind: length, a: edge(L/cap(start), L/side(Prof/l9))}`. | In axonometric views, measure the true 3D length (and draw it along the edge), or refuse with "dimension in an orthographic view". |
| F5-7 | medium | `feature_extrude` with the default `regions` (empty = all) fills the holes of an imported plate. The DXF gives three regions: a6, the plate with the hole and the slot already removed, and c11 and a19, the hole and slot themselves. "All" extrudes all three, so the plate came out solid: 40780.72 instead of 37822.80 mm3. The result shows only the volume. This is the normal case for an imported DXF part, and the schema note "empty = all" does not warn about it. | Part 2: `sketch_import_dxf plate.dxf`, `feature_extrude {sketch: S, distance: 0.25 in}`. | Default to the outermost regions with their holes (leave out regions that are holes of another region), or warn: "regions c11, a19 are holes in a6 and were filled; pass regions: [a6] to keep them open". |
| F5-8 | low | `feature_delete` of a feature that a drawing dimension measures succeeds without a word. The result has `new_errors: []`, `query_model.errors` is empty, and the dangling dimension shows only in `query_drawing` / render / export. Its message starts with the confusing part: `edge … is not in this view's bodies (face FilOut/blend(…) no longer exists)`. | Part 1: `feature_delete {name: FilOut}` while dimension Rout measures its arc. | Name the affected drawing items in the delete result (`dangling: ["D1/Rout"]`). Start the message with "the face … no longer exists". |
| F5-9 | low | OpenSCAD constructs that are not supported are handled in two different ways. `hull`, `minkowski`, `text`, `import` and `offset` are skipped with a clear warning per line. `polyhedron`, `linear_extrude(scale = …)` and `linear_extrude(twist = …)` abort the whole import (`OpenSCAD: line 13: polyhedron is not supported yet`), so one object stops all the others. Parse errors, "recursion is too deep" and "too many primitives" (good guards; the limit is not stated) all come with `code: "io"`. | `part3c.scad` with a `polyhedron`; `linear_extrude(height = 10, twist = 90) square(4);`. | Skip unsupported objects with a warning, as for `hull`. Use `invalid_input` / `unsupported` instead of `io`, and give the primitive limit in the message. |
| F5-10 | low | FCStd: hidden objects are skipped silently (`warnings_user_text: []`), so the agent cannot tell that "Hidden box" existed. Without `GuiDocument.xml`, everything is imported without a note. A non-ZIP file reports `3MF ZIP container: file is too small to be a ZIP archive`, which names the wrong format. The import result does not show object labels, so a suspicious label is visible only in `query_model`. | `two.FCStd` (one hidden object); `notzip.FCStd`. | Add warnings such as "skipped hidden object 'Hidden box'" and "no GuiDocument.xml: visibility unknown, all objects imported". Say "FreeCAD file (ZIP)". Return `label_user_text` per created body. |
| F5-11 | low | The drawing command schemas are thin. In `drawing_add_view`, `side`, `section`, `cut_at`, `gap`, `center`, `radius`, `scale` and `name` have no description: the values of `side` came from an error, and the frame of a detail's `center` ("doc units") had to be guessed. `drawing_edit` accepts `title` and it works, but `title` is not in its schema. `drawing_export` of a `.png` gives the hint "e.g. part.step or part.stl". `query_drawing` returns neither dimension and note positions nor `text_at`, so a `drawing_move_item` can only be checked by rendering. An automatic dimension name can equal the drawing name (dimension "D1" in drawing "D1"). | `commands {name: drawing_add_view}`. | Describe each field (side: top/bottom/left/right as seen from the parent; cut_at: offset along the parent's …; center: view-plane model coordinates). Add `title` to the schema. Fix the hint. Return `text_at`/`at` per dimension and note. Use a prefix such as `Dim1` for automatic names. |
| F5-12 | low | `--read-only` cannot open a project that a writer server already holds: `error: … is already open in another Gusset window (the project is open in another Gusset window (process 28150 on vm))`. The text is repeated and says "window" for a headless server. A read-only reviewer next to a working agent is a natural setup. | Start `gusset mcp --no-attach bracket.gus`, then `gusset mcp --no-attach bracket.gus --read-only`. | Let read-only sessions open a locked project from its last save. Fix the duplicated text. |
| F5-13 | low | DXF import skips SPLINE and ELLIPSE (counted in `skipped_user_text`, which is good). Both are common in DXF files from other CAD tools. A file without `$INSUNITS` is taken as mm (`unit_mm: 1.0`) without a note. | `odd.dxf`. | Import ellipses (exact) and splines (B-spline sketch curves). Warn when `$INSUNITS` is missing or 0. |
| F5-14 | low | Drawing dimensions are in mm only. The inch plate is dimensioned 76.2 / 63.5 / Ø12.7, and there is no unit, dual-dimension or decimal-places option in `drawing_create`, `drawing_edit` or `drawing_add_dimension`. | Part 2. | Add a drawing `units` (mm/in) and `decimals` setting, and optional dual dimensions. |

### What worked well

- **Values:** every model and drawing value matched the hand calculation. Dimensions are re-measured on each change (Ø8 → Ø10 at once), and a deleted feature turns its dimension into a visible "Rout?" mark that comes back on undo.
- **Views:** first-angle placement, hidden lines, centre marks and the section are correct. The section has its arrows, label A-A and hatching, and only cut faces are hatched. The detail view has a circle, a label and its scale. Moving a base view takes its projected views along.
- **Exports:** the PDF is a real one-page A3. The SVG has `mm` units and a group per line type, and the DXF has a layer per line type in both R2000 and R12. Exports and renders report the same `issues`, and the overwrite approval goes to the user.
- **DXF import:** units come from `$INSUNITS`, bulges become exact arcs, shared ends become shared points, region areas are exact, and skipped entities are counted.
- **OpenSCAD:** modules, recursion, loops, functions, comprehensions and the CSG operations are all right. The import has guards against runaway recursion and huge loops, `echo` output arrives as flagged `user_text`, and unsupported calls are named by line.
- **FCStd:** a hand-made file imports. Hidden objects are skipped and broken shape files are reported per object.
- **Safety and speed:** paths are confined to the project, item names must be identifiers, and read-only mode refuses every new command. All calls answered in 0.14 s or less.

### Not tested

- Third-angle sheets, portrait sheets, ANSI sizes, `drawing_edit` changing size or projection after views exist.
- Half sections, offset or aligned sections, broken-out sections, sections of assemblies or of several bodies.
- Drawings of assemblies or components, and drawings on a branch merged into another one.
- A real FreeCAD-written `.FCStd`: compounds, `Part::FeaturePython`, links, placements that differ from the shape location, and older "Topology V1/V2" BRep text.
- OpenSCAD `include`/`use` of other files, `children()`, `surface`, `projection` and `resize`.
- Cancelling a long import, rate limits, and the in-app GUI drawing editor.

### Run 5 fix status (2026-10-20)

| Finding | Status |
|---|---|
| F5-1 | **Fixed**: `$fn` 3–30 on a circle or cylinder builds OpenSCAD's polygon or prism; small `$fn` on a sphere or cone is warned about |
| F5-2 | **Fixed**: a read-only connection never saves, and a read-only agent creates no branch |
| F5-3 | **Partly fixed**: overlapping dimension and note texts are listed in `issues`; automatic stacking of parallel dimensions is open |
| F5-4 | **Fixed**: views outside the frame, over the title block or over each other, and notes outside the frame are in `issues`; a second view on the same side goes further out. Picking a standard scale that fits for `standard_views` is open |
| F5-5 | **Fixed**: drawing items the sheet cannot draw are refused before committing |
| F5-6 | **Fixed**: foreshortened lengths and axonometric distances are refused |
| F5-7 | **Fixed**: an empty region selection leaves holes open (even-odd) |
| F5-8 | Open: delete results do not name the drawing items they leave dangling |
| F5-9 | **Fixed**: `polyhedron` and twisted or scaled `linear_extrude` are skipped with a warning; the primitive limit is stated. Error codes stay `io` |
| F5-10 | **Fixed**: hidden objects and a missing `GuiDocument.xml` are warned about; the ZIP error names FreeCAD. Per-body labels in the result are open |
| F5-11 | **Fixed**: `drawing_add_view` fields are described (within the tool-list budget); the export hint names the right format. Positions in `query_drawing` are open |
| F5-12 | Open: read-only sessions cannot open a locked project |
| F5-13 | **Partly fixed**: a DXF without `$INSUNITS` gives a warning; SPLINE and ELLIPSE import is open |
| F5-14 | Open: drawing units and decimals (dimensions follow the document unit since review N4 only for model inputs) |
