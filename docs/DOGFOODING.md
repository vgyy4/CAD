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
| F5-3 | **Fixed**: an automatically placed dimension steps out (7 mm, up to 6 times) while its text meets an earlier one; overlaps that remain are listed in `issues` |
| F5-4 | **Fixed**: views outside the frame, over the title block or over each other, and notes outside the frame are in `issues`; a second view on the same side goes further out. Picking a standard scale that fits for `standard_views` is open |
| F5-5 | **Fixed**: drawing items the sheet cannot draw are refused before committing |
| F5-6 | **Fixed**: foreshortened lengths and axonometric distances are refused |
| F5-7 | **Fixed**: an empty region selection leaves holes open (even-odd) |
| F5-8 | **Fixed**: `feature_delete` returns `dangling_dimensions` (drawing/dimension) |
| F5-9 | **Fixed**: `polyhedron` and twisted or scaled `linear_extrude` are skipped with a warning; the primitive limit is stated. Error codes stay `io` |
| F5-10 | **Fixed**: hidden objects and a missing `GuiDocument.xml` are warned about; the ZIP error names FreeCAD. Per-body labels in the result are open |
| F5-11 | **Fixed**: `drawing_add_view` fields are described (within the tool-list budget); the export hint names the right format. Positions in `query_drawing` are open |
| F5-12 | **Fixed**: `--read-only` on a project another window or server holds reads its last save, unlocked; the lock message says who holds it once |
| F5-13 | **Fixed**: SPLINE (fit points, or samples of the rational B-spline) and ELLIPSE (64 samples per turn) import as sketch splines through those points (close, not exact); a DXF without `$INSUNITS` gives a warning |
| F5-14 | **Fixed**: drawings take `units: mm | in`, `decimals: 0..6` and `dual: true` (the other unit in brackets) |

## Run 6 — sheet metal, CAM, simulation, direct modelling, F3D/SAT import (2026-11-07)

Setup: debug `gusset` with real OCCT and workers, `gusset mcp --no-attach` over stdio, run 5's
stdlib Python client; 309 calls, files in the session's scratch directory (`dogfood6/`). Learned
from `tools/list`, `commands`, schemas, resources and errors.

| # | Part | Result | Calls | Notes |
|---|---|---|---|---|
| 1 | Sheet metal: tab + flanges (`close`), `F1_end`, U + `l6_start`, holes | modelling exact; cut-outs failed with several flanges (F6-1) | 142 | Volumes, allowances and flat sizes match hand. |
| 2 | CAM: pocket, profile with tabs and ramp, drilling | works, G-code issues | 44 | Climb direction, I/J, tabs, G83 checked. |
| 3 | Simulation: cantilever static and modal | right at fine voxels, wrong by default (F6-5) | 43 | 1 mm voxel: 0.754 vs 0.762 mm; modal within 1 %. |
| 4 | Direct modelling: `onto` | works; bad targets committed (F6-7) | 33 | |
| 5 | Import: .f3d, .f3z, .sat | placed correctly; bodies and names poor (F6-8) | 35 | Bounding boxes match the STEP twin. |

| ID | Sev | Item | Status |
|---|---|---|---|
| F6-1 | major | Cuts not unfolded with two flanges or a side flange | Fixed: faces of the sheet's own features (flange end caps) are sheet faces, not the cut's (OCCT test). |
| F6-2 | major | A second cut wrote the first cut's outline again | Fixed: outlines already in the pattern are not added again (OCCT test). |
| F6-3 | major | Export hint named a missing feature; no cut-outs in `query_flat_pattern` | Fixed: hint says what unfolds; `cutouts: [{bbox_mm, area_mm2}]`. |
| F6-4 | minor | Bend cut-outs mapped from the sketch-side face, circles as polylines | Fixed: bend cut-outs are mapped on the neutral surface along the cut direction (width = neutral arc length); full circles on flats are carried as circles into `query.flat_pattern` and the DXF (ADR 0037). |
| F6-5 | major | Default voxel gave one voxel through a 10 mm bar, 118 % off, no warning | Fixed: default is the smaller of longest/30 and shortest/4 (within the cell cap); `caveats` when fewer than 3 voxels span the thinnest side or the peak sits at a held face. |
| F6-6 | minor | Loose-part refusal advised a smaller voxel | Fixed: advises joining the parts by faces or fixing each. |
| F6-7 | minor | `onto` a tilted, coplanar or opposite face committed a failing feature | Fixed: refused before committing (test). |
| F6-8 | major | F3D import: construction sheets as bodies, generic names | Sheets left out when the file has solids (warned; a surfaces-only file keeps them); placed bodies take their occurrence's name. Hidden or superseded solids: open. |
| F6-9 | major | A line with `NaN`/`1e999` was dropped silently, the client hung | Fixed: answered with a JSON-RPC parse error (−32700, id null); the session goes on (test). |
| F6-10 | minor | Ramp stopped at the tab top then plunged | Fixed: ramps down along stretches outside the tabs, re-cut at depth (tests). |
| F6-11 | minor | G83 `R` at the stock top | Fixed: R = top + min(1 mm, (safe - top) / 2); a drill job whose holes are not the tool's size says so in `warnings` (use `bore`). |
| F6-12 | minor | Duplicate G-code lines, no tool line, one operation per file | Duplicates and `(T1 D.. flat end mill)` fixed; several jobs in the document now (`cam_job`), one program per job. |
| F6-13 | minor | Reconnecting starts on Main with an empty undo stack | A reconnecting client is told its earlier branches in the initialize instructions (names are still never reused); 2026-07-28 discover clients not yet. |
| F6-14 | minor | Grammar resource lacked the new syntax; modal shape has no scale | Fixed: grammar updated (sheet sides/ends, `close`, cut-outs, `onto`, `cam_job`); `sim.modal` with `shape` returns `shape_scale` (drawn size of the largest motion, where it is and its direction). |

## Run 7 — feature-tree import, tilting, adaptive CAM, sheet circles (2026-11-07)

Setup: debug `gusset` with real OCCT and workers (`CARGO_INCREMENTAL=0`, built in 16 s from a warm
cache), `gusset mcp --no-attach` over stdio behind run 6's stdlib Python client (a daemon per
project plus a unix-socket caller); about 330 calls, files in the session's scratch directory
(`dogfood7/`, projects in `dogfood7/proj/`). Learned from `tools/list`, `commands` (with a
`name` for the schema), the grammar resource and error messages; ADR 0042 was needed for the
`.gtree.json` format, since nothing in the MCP surface describes it (F7-13).

| # | Part | Result | Calls | Notes |
|---|---|---|---|---|
| 1 | Feature-tree import: 4 parameters (two with expressions), base on XY with parameter-driven dimensions, fillet and chamfer by edge midpoint, ring boss on an offset sketch plane (`join`), pocket on a flipped YZ-parallel plane (`cut`), 270 deg revolve, an unsupported feature, a tilted-plane sketch + extrude | exact; skips reported per feature; 21 malformed trees | 97 | Import is one undo step; a second tree with a rotated plane, an arc, `symmetric` and `intersect` was exact too. |
| 2 | `move_face onto` a tilted plane: a box's top onto a wedge's 14 deg face, a cylinder's cap onto the same face; refusals | works, refusals right but two late failures (F7-8, F7-9) | 46 | Volume, area and centroid match hand. |
| 3 | CAM on an L pocket: `pocket` vs `pocket_adaptive`, G-code, `cam_job` x2 in the document, drilling with the wrong tool | works; adaptive is 5x the length | 42 | Tool path swept on a 0.25 mm grid; no gouge, same coverage, adaptive radial cut within the stepover. |
| 4 | Sheet metal: tab + 90 deg flange, round hole in the tab, hole across the bend, hole in the flange wall, flat pattern, DXF/SVG | circles reach the DXF; two bugs with several cut-outs (F7-1, F7-2) | 69 | |
| 5 | Simulation: 100x20x5 cantilever, static (tip load) and modal with `shape` | within a few % of beam theory, converging | 21 | `shape_scale` right. |

Hand checks:

| Check | Hand | Gusset |
|---|---|---|
| Imported base volume: 60x40x12 less fillet r3 (`(1-pi/4) r^2 h` = 23.18) less chamfer 2 (24) plus ring boss `pi (10^2-5^2) 8` less pocket `6x20x15` | 28837.7786 mm3 | 28837.778594 |
| 270 deg ring, `0.75 pi (20^2-10^2) 10`, bbox x 80..120, y -20..20 | 7068.5835 mm3 | 7068.583471, bbox as hand |
| `w` 60 -> 80 (so `h` = 16; boss now sticks out 4 mm; the pocket becomes a void): 80x40x16 - 30.90 - 32 + `pi 75 4` - 1800 | 50279.58 mm3 | 50279.575132; undo gives 28837.778594 again |
| Rotated-plane symmetric extrude intersected with a 5x5x2 box (triangle of 12.5 mm2 x 2); semicircle `pi 50 4` on a sketch origin-shifted to x = 200 | 25 / 628.3185 mm3 | 25.0 / 628.318531 |
| Box top moved onto the wedge plane z = 15 + x/4: volume stays 32000 (mean top height 20), z max 25, area `1649.24 + 600 + 1000 + 1600 + 1600`, centroid x 21.667 | 32000 / 25 / 6449.24 / 21.667 | 32000 / 25 / 6449.24225 / 21.666667 |
| Cylinder (r10, h30) cap onto the same plane at x = 20 (plane crosses at z = 20): volume `pi 100 20`, cap area `314.159 x 1.030776`, z max 22.5 | 6283.19 / 323.83 / 22.5 | 6283.185307 / 323.827959 / 22.5 |
| L pocket area (60x20 + 20x30) | 1800 mm2 | region 1800; swept coverage 1771.1 of 1827.6 grid mm2 for both operations (the rest are the five convex corners and grid edge), nothing outside the region |
| Flat pattern of 100x60x2 tab + 20 mm flange (r2, k 0.44): 100 + (pi/2)(2 + 0.88) + 20 | 124.5239 x 60 | 124.523893 x 60; allowance 4.523893 |
| Plate volume: flat 124.5239x60x2 = 14942.86 plus (quarter annulus 9.4248 - 4.5239x2) x 60 = 22.62 | 14965.49 mm3 | 14965.486678 |
| Round hole d10 at (30,30): area / volume removed | 78.54 / 157.08 | circle r5 at (30,30), 78.539816 / 157.079633 |
| Hole across the bend (d1.8 at x = 101, neutral radius 2.88): x extent `2.88 asin(1.9/2.88)` + 100 | 102.0749 | bbox 100.100..102.0749; area 2.7627 vs 2.7713 numeric (0.3 %, polyline) |
| Hole d6 in the flange wall at (30, z = 14): flat x = 100 + 4.5239 + 10 | circle at (114.5239, 30) | circle at (114.523893, 30) r3 (when alone, see F7-2) |
| DXF circle for the d10 hole | (30, 30) | `CIRCLE` at (40, 40) r5: the pattern sits 10 mm in from the origin (F7-12) |
| Tip load 100 N: `PL^3/(3EI)` = 0.7619 + shear 0.0015 | 0.7634 mm | 0.7269 / 0.7358 / 0.7483 at voxel 1.25 (default) / 1 / 0.5: -4.8 / -3.6 / -2.0 %; voxel 2.5 warns "fewer than 3 voxels across the thinnest side" |
| (By mistake I loaded the top face: uniform 1 N/mm, `wL^4/(8EI)`) | 0.2857 mm | 0.2716 / 0.2749 / 0.2796 (converging) |
| First bending frequency `(3.516/2 pi) sqrt(EI/(rho A L^4))` | 417.8 Hz (then 1671 lateral, 2618 second bend) | 425.8 (+1.9 %) / 1629.6 (-2.5 %) / 2636.4 (+0.7 %) at voxel 1 |
| `shape: 1` / `shape: 2` | bending in z / in y, largest motion at the tip, drawn 10 mm | `shape_scale` direction (0.035, 0, 0.999) / (-0.13, -0.99, 0) at (100, 10, 0) / (100, 0, 2.5), `drawn_mm` 10 |

Feature-tree import, observed: a `fixed` + `distance` sketch with a `parameter` keeps the
dimension linked to the imported parameter (`distance(p1, p2, w)`); a plane parallel to
XY/YZ becomes a datum plane (`Boss_plane`, offset 12 mm; `Pocket_plane`, base YZ offset 60 mm)
with the sketch re-projected and the cut distance sign flipped for the flipped normal; a
tilted plane, an `unsupported` feature, a missing sketch or target, a fillet whose midpoint
matches no edge, a profile pick outside every region, an unknown `operation`/`type`, a
radius too big for the edge, a zero or missing distance, and an injection-style sketch name
(`a b){ evil\nparam x = 1`) each skip just that feature with a reason in
`skipped_user_text` (the injected name never reached the document text). Malformed
JSON, a wrong `format`, `version: 2`, a truncated file, `../` paths and a missing file are
refused with an error; names that clash get `_2`, `_3` suffixes (so importing twice works).

CAM, observed (6 mm flat end mill, L pocket, defaults, depth 4): `pocket` 2239 mm cut, 3.99 min,
126 moves, 3 kB of G-code; `pocket_adaptive` 11420 mm, 19.55 min, 3266 moves, 108 kB (35x the
lines). Stepover 0.4 explicit, depth 12, one 12 mm level: adaptive 4435 mm / 7.6 min against
pocket (four 3 mm levels) 4576 mm / 8.5 min; stepover 0.2 + stepdown 12 + feed 1500: 8942 mm /
6.3 min. Walking the exported single-level paths over a 0.2 mm grid: `pocket` cuts more than
5.9 mm wide over 11.9 % of its length (up to 6.2 mm), `pocket_adaptive` never over 5.8 mm
and above the 2.4 mm stepover over 0.8 % (plunge, slot starts), mean 0.37 mm. Arc I/J radii
agree to 0.0001 mm, floor at -4, header/footer fine, climb direction matches ADR 0040
(material on the right, M3; my first reading of it was backwards, the code is right).
`drill` with a 4 mm tool over holes of 4, 4, 4 and 8 mm: `warnings: ["1 of 4 holes are not
4 mm across: a drill cuts its own size (use bore to mill them)"]` (3 mm tool: 4 of 4); G83
with `R1 Q2`. `cam_job` blocks (`Clear`, `Holes`) written with `document_set_text`, then
`cam_toolpath`/`cam_export_gcode` with `job:` and one program per job; extra fields next to
`job` are refused with a clear message, so is an unknown job.

| ID | Sev | Item | Expected |
|---|---|---|---|
| F7-1 | major | Flat pattern lists a cut-out again (as a polygon tagged `circle`) when a later hole is made: tab + flange, hole in the flange, then a hole in the tab gives 3 cut-outs, then a fifth after another hole; the DXF has two `CIRCLE`s at the same place (laser double cut). | Each hole once (F6-2 said outlines already in the pattern are not added again). Repro: `proj/e3.gus`. |
| F7-2 | major | After a cut across a bend (hole at x = 101 on the bend), a later hole through the flange wall is not unfolded: `changed_by`, warning, and `sheet_export_flat` refuses without `allow_changed`. The same wall hole on its own is unfolded to a circle at (114.52, 30). Also a `feature_extrude` cut through the wall in the same model was refused. | Wall hole mapped as without the bend cut (the grammar says flat regions and bends are unfolded). Repro: `proj/d.gus`, `proj/e3.gus`. |
| F7-3 | minor | `.gtree.json` import: two parameters with the same name are merged silently, the later value wins (`Base` became 40 wide, the chamfer's edge was not found); no warning. A dimension `parameter` naming no parameter, a constraint of unknown type and a `value` on `horizontal` are dropped or ignored silently too. | A warning for each dropped constraint/duplicate. |
| F7-4 | minor | A parameter expression that parses but cannot be evaluated (`1/0`) is stored as `param w_2 = 1 / 0`; every dependent feature errors afterwards (`Base_2: constraint 'k14': unknown parameter 'w_2'` in `query_model.errors`) and the import report mentions nothing but the volume mismatch. Unparsable ones (`w +`) are handled (value used, warning). | Fall back to the value with a warning; report rebuild errors in the import result. |
| F7-5 | minor | No bound on an extrude `distance`: 1e13 imported, volume 2.4e16 mm3 (the ADR amendment says numbers below 1e12). | Skip with a reason. |
| F7-6 | minor | Two imported trees compare the whole model's volume with the source's `bodies`: a second import into a non-empty document, or a tree with a `bodies` list shorter than its result, warns "differs" with wrong numbers (35906 against 28838 because I listed one of two bodies; 71836 on the second import). The ADR describes per-body `mismatches`; the result has one total. An empty tree reports success with 0 features and the same warning. | Per-body comparison of the bodies the import made; refuse or say so for an empty tree. |
| F7-7 | minor | `feature_move_face` without `body`: the body defaults to the latest one, not the face's, and the feature is committed with `Tilt1: face BoxA/cap(end) no longer exists (re-select the face to move)`. A missing `onto` face (`Wedge/side(W/l99)`) lists candidates from a different body (`Cyl/...`). | Body taken from the face reference; candidates from the named body; at least "body: X is not the face's body". |
| F7-8 | minor | `onto` a plane the moved face would cross the body at (the bottom face onto the top tilted plane, the cylinder cap onto a plane at z 0..5) is committed with `committed_with_errors` (kernel: "draft produced an invalid solid ..."), while perpendicular, curved (moved or target), coplanar and `translate` + `onto` are refused before committing (good). | Refuse these too, or at least say why in one hint; the undo hint is there. |
| F7-9 | nit | Adaptive pocket: the default `stepover` is 0.25 (results equal 0.25 exactly), the tool description says 0.4; at the same stepdown and feed it is 5x longer and slower, and 36x the G-code, and nothing says its benefit comes from stepdown/feed (a 12 mm level at 1500 mm/min cut the time to a third of `pocket`). | State the adaptive default and a hint about stepdown/feed in the description or result. |
| F7-10 | nit | `cam_job` can only be created by `document_set_text` with the whole document (no command; `cam_toolpath`'s `job` says only "A cam_job."). | A `cam_job` command or the grammar section named in the tool description. |
| F7-11 | nit | `feature_hole` `at` wants strings (`invalid type: integer 30, expected a string`) while `sketch_add_circle` takes numbers. | Numbers accepted as mm. |
| F7-12 | nit | `sheet_export_flat` (DXF and SVG) places the pattern 10 mm in from the origin (`CIRCLE` at (40, 40) for a hole at (30, 30)); neither the result nor the description says so ("1:1"). | Say it (or put the pattern at 0,0 / return `offset_mm`). |
| F7-13 | nit | The `.gtree.json` format is documented nowhere in the MCP surface (no resource, no schema; the "not a Gusset feature tree" hint points at the exporter plugins); an agent writing one must read ADR 0042 in the repository. | A short resource or the hint naming the format. |
| F7-14 | nit | `drill` G-code moves to the first hole twice (`G0 X5 Y5` then `G98 G83 X5 Y5`); warning says "1 of 4 holes are not". | Cosmetic. |

What worked well: the import turns a hand-written tree into ordinary editable features
(readable text, parameter-driven dimensions, datum planes) in one undoable step, with exact
volumes, and every skip names its reason (`skipped_user_text`, untrusted strings wrapped as
such); one bad feature never spoils the rest; the file limits and path checks hold. Moving a
face onto a tilted plane is exact (area, centroid) on a box and on a cylinder, and the
refusals for perpendicular, curved, coplanar and `onto` + `translate` come before anything is
committed with a hint. Adaptive clearing does what the ADR says (measured). `drill` warns about
holes that are not the tool's size, and `cam_job`s give one program each. Circles survive into
the flat pattern and the DXF (`CIRCLE`), a hole across a bend comes out as the right-sized
polygon (neutral arc length). `sim_static` warns about too coarse voxels, `sim_modal`
refuses oversized grids with the limit, `shape_scale` gives direction, size and place of the
largest motion, and `shape_bands` (250 kB) is not inlined but paged by `get_text`. Field-name
errors list the accepted fields. Output sizes: `import_file` 1.4-2.2 kB, `query_model` about
1 kB per 4 features, `query_flat_pattern` 0.4-0.8 kB, `cam_toolpath` 0.2 kB.

Not tested: a tree with sketch constraints beyond `fixed`/`distance`/`radius`/`horizontal`/`vertical`,
`chamfer` and `fillet` of several edges, `revolve` with a world axis, 2000-feature and 32 MiB
limits, the plugins themselves; `profile_inside`, tabs/ramps, `bore` and islands in the adaptive
pocket; `pocket_adaptive` in a `cam_job` with `regions`; a flange off a contour flange with
cut-outs; `contour_flange` holes; hems; `sim.static` on the L bracket and with `field: true`
or `render_view` of a mode shape; `move_face` with `angle`/`axis` on a tilted target, several
faces at once, and `onto` on a face of a body that is later moved.

### Run 7 fix status (2026-11-07)

| # | Status |
|---|---|
| F7-1 | Fixed: only the cutting feature's own faces are unfolded as a new cut-out (earlier holes were listed again). OCCT test `modeling_sheet_holes.rs`. |
| F7-2 | Fixed by the same change (an earlier cut across a bend made later cuts look unfoldable). |
| F7-3 | Fixed: duplicate parameters, unknown constraint types and dimensions naming missing parameters are warned about. |
| F7-4 | Fixed: each parameter is evaluated on import; one that does not evaluate falls back to its value with a warning. |
| F7-5 | Fixed: extrude distances are bounded like the other numbers (below 1e12). |
| F7-6 | Fixed: the volume check counts only the bodies the import made; an empty tree is refused. |
| F7-7 | Fixed: face commands without `body` take the body holding the first face; missing-face candidates come from the named feature's body. |
| F7-8 | Fixed: `move_face onto` is tried before committing and refused with the kernel's reason. |
| F7-9 | Fixed: the description states the adaptive default stepover (0.25). |
| F7-10 | Fixed: `feature_add` adds any one-line feature from its code-view statement (e.g. `cam_job`), and `cam_toolpath`'s `job` names it. |
| F7-11 | Fixed: `feature_hole` `at` takes numbers (mm) as well as lengths. |
| F7-12 | Fixed: `sheet_export_flat` returns `offset_mm` (drawing point = pattern point + offset). |
| F7-13 | Fixed: the grammar resource summarises the `.gtree.json` format. |
| F7-14 | Fixed: no extra move before the first drill cycle; "1 of 4 holes is". |


## Run 8 — shells on sheet metal, rest and 3D CAM, angled corners (2026-11-07)

Setup: debug `gusset` with real OCCT and workers (`CARGO_INCREMENTAL=0`, built in 14 s from a
warm cache), `gusset mcp --no-attach` over stdio behind run 7's stdlib Python client (daemon per
project plus a unix-socket caller); about 370 tool calls (94 `sim_static`, 72 `document_set_text`,
60 `run`), files in the session's scratch directory (`dogfood8/`, projects and exports in
`dogfood8/proj/`). Learned from `tools/list` (31 tools, 18.9 kB), `commands` (12.4 kB; a `name`
for the schema), the grammar resource and error messages. Flat patterns were checked by parsing
the exported DXF (polygon area, vertices), CAM paths by sweeping the tool over a 0.1 mm grid and
against the analytic dome, simulation against beam theory. Everything ran on a debug build, so
the timings are upper bounds.

| # | Part | Result | Calls | Notes |
|---|---|---|---|---|
| 1 | Sheet metal: tab 100x60x2 with two 20 mm flanges (r2, k 0.44), no corner treatment / `close` / `close: mitre`, at 90 and at 60 deg flange angle; plan corners of 60 and 120 deg | closed and mitred corners exact at 90 and 60 deg flange angle; plan corners that are not 90 deg are refused (F8-3) | 62 | DXF area, size and bends checked. |
| 2 | Holes one after another (tab d10, wall d6, tab d8), a cut across a bend, a cut straddling bend and wall | each hole once in the pattern and the DXF (F7-1/F7-2 stay fixed); the straddling cut is not unfolded (F8-7) | 24 | |
| 3 | Simulation on shells: 100x20x2 strip (static tip load, in-plane, axial, `field`), 100x10x2 bracket with a 30 mm flange (loads on the flange end face, support on the flange end face), `sim_modal` on the strip and the bracket | `method: shell`; strip within 2 % of beam theory; most brackets fail to mesh (F8-1) | 120 | Timings in F8-10. |
| 4 | CAM: `pocket_rest` after a 10 mm tool in a 40x30 rectangle; `surface_ball`/`surface_flat` over a 30 mm hemisphere; G-code; four `cam_job`s in the document | heights match the analytic dome to 0.02 mm (F8-8 for the rim); rest leaves only the tool's corner radius | 36 | |
| 5 | `move_face onto` a tilted plane, face commands without `body` in a 3-body model, a face list over two bodies | exact; one misleading late failure (F8-12) | 14 | |
| 6 | `.gtree.json` of my own design: 3 parameters (one an expression), constrained sketch (parallel, perpendicular, fixed, distance via parameters, circle with `diameter` via parameter), extrude, 4-edge fillet, chamfer, offset-plane sketch + cut, revolve about a world axis | exact, then `W` 80 -> 100 and `HD` 12 -> 8 rebuild exactly; the world-axis revolve is wrong (F8-2) | 18 | |

Hand checks:

| Check | Hand | Gusset |
|---|---|---|
| Two 90 deg flanges, `close: true`: tab 12000 + bends `pi/4 (16-4) x (60+100)` + F1 wall 64 x 2 x 20 + F2 wall 102 x 2 x 20 | 20147.965 mm3 | 20147.964474 (F2 wall 0..102 inside, 0..104 outside, F1 wall 0..64, as the grammar says) |
| Flat pattern of that part: 124.5239 x 84.5239; polygon `6000 + 4.5239 x 160 + 62 x 20 + 104 x 20` | 10043.823 mm2 | size 124.523893 x 84.523893; DXF polygon (14 vertices) area 10043.8229 |
| Same with `mitre`: 0.1 mm gap on the diagonal, `0.1 x 2 sqrt 2 x 20` | -5.6569 mm3 | 20142.307619, delta -5.656854 |
| One flange at 60 deg: `12000 + pi/3 (16-4)/2 x 60 + 20 x 2 x 60`, allowance `pi/3 (2 + 0.88)`, bbox x `103.464 + 10`, z `3 + 20 sin 60` | 14776.991 / 3.015929 / 113.464 / 20.3205 | 14776.991118 / 3.015929 / 113.464102 / 20.320508 |
| Two 60 deg flanges, `close: true`: walls end on the mirror plane `x - 100 = y - 60`, bends stay 60 and 100 wide: `12000 + 6.2832 x 160 + 40 (60 + 7.598) + 40 (100 + 7.598)` | 20013.15 mm3 | 20013.155746; with `mitre` 20007.498892 (-5.6569, as at 90 deg) |
| Flat pattern of the 60 deg pair: trapezoid walls at the k-factor surface (`2.88 cos 30` = 2.494): `6000 + 3.0159 x 160 + 20 x 67.494 + 20 x 107.494` | 9982.31 mm2 | DXF polygon 9982.3148 (wall edges 62.494..72.494 and 102.494..112.494) |
| Hole d10 (30,30), d6 in the wall (z 14), d8 (60,15) | circles at (30,30), (114.5239,30), (60,15) once each | `cutouts` with the three circles; DXF has three `CIRCLE`s; volume steps 157.080 / 56.549 / 100.531 |
| Vertical cut d1.8 at x = 101 (bend only) | x 100.100..102.0749, area 2.7713 | bbox 100.100019..102.074895, area 2.7608 (polyline, -0.4 %) |
| Cut d5 straddling bend and wall (axis x, depth 4, at y 45, z 4): volume `19.635 + 22.923` | 42.558 mm3 | 42.556941; but `changed_by` (F8-7), hand flat area would be 21.03 mm2 |
| Strip 100x20x2, 10 N at the tip, `PL^3/(3EI)` (+0.0004 shear) | 1.1905 mm | 1.16989 / 1.17028 / 1.17039 / 1.17042 at element 5 / 2.5 (default) / 1.25 / 1 mm (-1.7 %, converged) |
| Root stress `M c / I` | 75 MPa | 76.4 / 78.0 / 78.7 / 79.0 (+4 %, at x = 0.33) |
| 100 N across the width: `PL^3/(3 E I_y)` 0.1190 + shear 0.0037 | 0.1228 mm | 0.123585 |
| 1000 N axial, `PL/(EA)` | 0.011905 mm | 0.012293 (+3.3 %; stress 36.6 against 25 nominal at the clamped corners) |
| `sim_modal` strip: `1.8751^2/(2 pi L^2) sqrt(EI/(rho A))`, 2nd and 3rd bending, torsion, in-plane | 167.10 / 1047.2 / 2932.3 Hz; torsion 1595, in-plane 1671 | 168.93 / 1055.6 / 2951.8 (+1.1 / +0.8 / +0.7 %); two modes at 1625.21 and 1625.63 (I did not tell which is which); element 5 mm: 168.96 / 1055.63 / 1624.1 / 1627.2 / 2951.6 |
| Bracket (tab 100x10x2, r2, flange 30), 10 N down on the flange end face, fixed tab end: tab beam to the wall mid `P (103^3 - 3^3)/(3EI)` = 2.602 plus the wall turning `theta = P 5300/EI` = 0.0379 rad, x motion `theta x flange length` | 3 / 10 / 30 / 60 mm flange: 2.604 / 2.629 / 2.839 / 3.456 mm | 2.593 / 2.629 / 2.867 / 3.508 (max displacement is the magnitude, at the flange top) |
| Same bracket, 10 N along -x at the flange top (`h` = 33): `sqrt(across^2 + up^2)` = 1.461 | 1.461 mm | 1.5198 (+4 %) |
| `pocket_rest`: corners left by a 10 mm tool in 40x30, `4 (1 - pi/4) 25`; left by the 4 mm rest tool, `4 (1 - pi/4) 4` | 21.46 / 3.43 mm2 | swept uncut area 21.40 / 3.36; floor z = 5.0; rest tool centre within x 2..38, y 2..28 |
| Dome R30 (revolve about the sketch's own axis): volume `2/3 pi R^3`, area `3 pi R^2`, centroid z `3R/8` | 56548.668 / 8482.300 / 11.25 | 56548.667765 / 8482.300165 / 11.25 |
| `surface_ball` D6 tip z `sqrt(33^2 - rho^2) - 3`: apex, rho 10.2, 19.8, 27 | 30.0 / 28.384 / 23.400 / 15.974 | 30.0 / 28.384 / 23.383 / 15.948 |
| `surface_flat` D6 tip z `sqrt(900 - (rho - 3)^2)`: rho 3 (first sample), 10.2, 20.4, 27 | 30.0 / 29.123 / 24.438 / 18.0 | 30.0 / 29.116 / 24.427 / 17.977 |
| `cam_job` x4 (`Rough`, `Rest`, `FinishBall`, `FinishFlat`) against the direct calls | same numbers | identical (548.33 / 624.47 / 4067.2 / 4117.23 mm cut); G-code identical apart from the comment line |
| Box top (z 20) onto the wedge's plane `z = 15 + x/4` at x 60..100: volume `40 x 30 x 35`, area `1200 + 2800 + 2100 + 1200 sqrt(17/16)`, zmax, centroid x | 42000 / 7336.93 / 40 / 80.952 | 42000 / 7336.931688 / 40 / 80.952381 |
| `.gtree.json` base: 80x50x10 - hole `pi 36 x 10` - four fillets `4 (1 - pi/4) 25 x 10` - slot 400 - chamfer 1.5 along the whole top loop `1.125 (220 + 4 (pi/2) 4.5)` | 37975.12 mm3 | 37975.116182 |
| Same after `W` = 100 (so `D` = 62.5, loop 285 + arcs), then `HD` = 8 | 60401.991 / +628.319 | 60401.991182 / +628.318531 |
| Ring about world Z (`pi (110^2 - 100^2) 15`) | 98960.169 mm3 | 98960.168588 with text `axis: Y` on the XZ sketch; the tree importer cannot make it (F8-2) |

Observed behaviour worth knowing. `close: true` at 90 deg: F1's wall is 64 long outside and
62 inside, F2's wall 104 outside / 102 inside, with a square relief where the bends stop at 60
and 100 mm; the flat pattern has the matching 14-vertex outline with a 2 mm notch. With `mitre`
the notch moves to the diagonal. The flat pattern places an obliquely cut wall edge at the
k-factor surface (0.88 mm in from the inside), which is why a mitred wall measures 62.809
against 62.929 at mid-thickness (F8-6). Holes: `feature_hole` takes numbers for `at`, and
refuses a hole centred on a face edge with the `fit inside face` hint. `sheet_export_flat`
returns `offset_mm` [10, 10] every time. The shell solver reports `element_mm` (2.5 for the 100x20 strip's default)
and refuses a too fine size with `too_large` (more than 40000 triangles at 0.5 mm, says to give a
larger element size). `cam_job`s: `job` plus an extra field is refused
(the overwrite of an existing file asks for approval, `approval_denied` when declined).

| ID | Sev | Item | Expected |
|---|---|---|---|
| F8-1 | major | `sim_static`/`sim_modal` on a sheet body with a bend fail with `shell element N is degenerate, inverted or too warped` (code `invalid_args`, no hint) for most width/element-size combinations, including the default. Tab 100x20x2 + 36 mm flange (r2): fails at element 1, 1.5, 2, 2.5, 3, 4, 5 and the default, works only at 8; flange 10 or 20, r 1/3/4, width 30 or tab 60 fail at 2 and 4 mm and the default; 100x10x2 + 30 flange: default (2.6), 3.3 and 5 work, 1, 1.25, 2, 2.5 fail; contour flange 100+30 x width 10: default fails, 3 and 5 work; width 20: default, 3 and 5 all fail. Plain strips did not fail at any size tried. The same bracket works with other element sizes, so the failure is in meshing the bend, not the part. | Mesh the bend robustly, or retry with a slightly different size, or at least say "try voxel: N" and name the face/place; the bracket is the main use of this feature. |
| F8-2 | major | `.gtree.json` `revolve` with `axis: {origin, direction}` passes the world axis into Gusset's X/Y/Z, which are the sketch's own axes (text `axis: Y` on an XZ sketch revolves about world Z; `Z` is the sketch normal, error `the Z axis is normal to the sketch`), although the grammar says "X, Y, Z is a world axis". On an XZ sketch a tree axis of world Z is skipped (`it does not build here: the Z axis is normal to the sketch`) and a tree axis of world Y (impossible in the source) silently builds a ring about world Z. Only `{"line": ...}` and sketches on XY are safe. | Convert the world axis into the sketch frame (refuse when it is normal to the sketch), and fix the grammar sentence. |
| F8-3 | minor | `close: true` and `close: mitre` on a corner whose plan angle is not 90 deg (tab with a 60 or 120 deg corner, both flanges 90 deg, same direction) fail with `no 90 deg flange meets l6 at an outside corner to close (flange the neighbouring line first (same direction, 90 deg), then close)` although exactly that flange exists. Without `close` they build (volume 16741.42 for the 60 deg quad). The grammar does not say the plan corner must be 90 deg. | Say "corner is 60 deg, closing needs 90" (or support it) and state it in the grammar. |
| F8-4 | minor | After a failed rebuild a later `document_set_text` that still contains the failing feature reports `rebuild_ok: true`, `new_errors: []` (the error is old), while `query_model.errors` lists it. Body and pattern disagree meanwhile: `query_body` kept the volume of the unclosed variant (16741.42) while `query_flat_pattern` and `sheet_export_flat` listed only F1's bend and wrote a pattern without F2, no warning. | `rebuild_ok` false while any feature fails; export and pattern refuse or warn on a failed feature. |
| F8-5 | minor | The corner option is spelled three ways: text `close: true|mitre` (grammar; the edit-flange tool call writes `mitre: true` out as `close: mitre`), tool/`run` fields `close` (boolean) and `mitre` (boolean), so `close: "mitre"` from the tool is refused (`invalid type: string "mitre", expected a boolean`) and text `mitre: true` is `has no argument 'mitre' (allowed: ... close ...)`. Similar: `feature_extrude` takes `sketch` where the text has `profile`; `param_set` takes `expr` where trees use `expression`; the tool schema says nothing of either. | One spelling; the error could name the other. |
| F8-6 | nit | A mitred or non-90 closed corner is drawn in the flat pattern at the k-factor surface of the wall edge: 62.809 / 102.809 for the 90 deg mitre (mid-thickness 62.929 / 102.929), 62.494 for the 60 deg pair (mid 62.598). The flat area then differs from the solid's by 4.8 mm2 (0.05 %) for the mitre; closed 90 deg is exact. Not stated anywhere. | State the convention in the grammar. |
| F8-7 | minor | A cut that straddles the bend line and the wall (hole d5 centred on the bend/wall line, `extrude cut` from the wall face, depth 4) leaves the pattern marked `changed_by: ["Xb"]` with a warning, and `sheet_export_flat` refuses without `allow_changed`; a hole wholly in the bend (x = 101) or wholly in the wall unfolds. `feature_hole` cannot make it (face edge). | Unfold it (pattern x 101.50..107.02, area 21.03 by hand) or list it in the pattern as a polygon. |
| F8-8 | minor | 3D finishing is exact against the surface mesh but the mesh sag is not told or settable: against the analytic dome the path points are 0.01-0.08 mm below the surface on the flank (mean -0.02, ball and flat), and up to 0.33 mm (ball) / 1.0 mm (flat) below within 3 mm of the dome's foot (rho > 32.5) where the wall is steep; linear interpolation between the 0.6 mm samples is up to 0.5 / 1.3 mm lower. The tool does not go below `top - depth` (z min 0.0). | Report the mesh tolerance in the result (or `tolerance`/`sag` field), and say so in the description. |
| F8-9 | nit | G-code of `surface_ball` (and of rest jobs) says `(Gusset 2.5D toolpath: flat end mill D6 mm)` and `(T1 D6 flat end mill)` for every operation; nothing says ball. | Name the tool by operation. |
| F8-10 | minor | Timings (debug build): `sim_static` strip, 5760 elements 0.8 s (1 s with `field`); bracket, supported at the flange end, 2094 elements 87 s (263 iterations); `sim_modal` strip 5 modes 38 s at the default (5760 elements), 5.7 s at 5 mm; bracket at 5 mm 62 s and at the default (about 6000 elements) `deadline_exceeded` after 300 s with no partial result. The default element size is the finest that works for static, the same for modal. | A modal default coarser than static, or a size/time estimate in the `too_large`/warning. |
| F8-11 | nit | Output cost: `sim_static field: true` 405 kB and `sim_modal shape` 450 kB of bands (paged by `get_text`, 4096 characters per page: 100 calls to read one); `cam_toolpath path: true` for the dome 64 kB (16 pages), the same for the 40x30 pocket 13.8 kB inline; `query_body names: true` 8-10 kB for a bracket with 2 flanges (40 edges, 18 faces). | Bigger pages, min/max per band, or a summary form. |
| F8-12 | minor | `feature_move_face` with faces of two bodies (`BoxB/cap(end)`, `BoxC/cap(end)`) is committed with `committed_with_errors` and `face BoxC/cap(end) no longer exists (re-select the face to move)`; the face exists. `onto` refusals (invalid solid, perpendicular) come before committing as in run 7; missing `onto` faces list candidates of the right body. | Refuse before committing: "faces belong to different bodies". |
| F8-13 | nit | `sim_static` with `loads: []` runs and returns 0 displacement, 0 stress, no warning (0 iterations). | A warning for no load. |
| F8-14 | nit | `.gtree.json`: a circle with `center` as coordinates (the grammar says `{center, radius}`) is reported `invalid type: sequence, expected a string` without the field or feature (the sketch is skipped with its extrude, fillet and cut following); the center is a point id. A chamfer of one edge midpoint chamfers the whole tangent chain (the top loop, 279.3 mm3 as by hand) with no note on how many edges were taken. The skipped revolve's volume warning compares with the source and does not name which body is missing. | Field path in the error; list the edges a fillet/chamfer expanded to. |
| F8-15 | nit | `surface_*` without `top` fails with `the safe height is below the surface` (default top 0, dome up to 30) while `depth` missing says `depth is missing`; the result of a surface job reuses `rings` (51 = raster lines) and `levels_mm [0.0]`. | `top` hint in the error ("give the stock's top"). |

Run 7 fixes seen holding: F7-1 and F7-2 (three holes one after another, each once; the wall hole
after a tab hole is unfolded), F7-7 (face commands without `body` take the face's body in a
3-body model; missing-face candidates come from the right body), F7-8 (`onto` that would give an
invalid solid is refused with the kernel's reason before committing), F7-9 (the tool description
now says adaptive default 0.25), F7-11 (`feature_hole at` numbers), F7-12 (`offset_mm`), F7-13
(grammar summary: it was enough to write a tree that imported, apart from the circle `center`
and the revolve axis above).

What worked well: closed and mitred corners are exact at 90 and 60 deg flange angles (volume
and DXF area to 6 digits), including the 0.1 mm mitre gap; the flat pattern's trapezoid walls
for the 60 deg corner; holes across a bend; the shell solver on straight strips (within 2 % of
beam theory for displacement, 1 % for modal frequencies, `method: shell` reported, the max
displacement reported at the flange top for a bracket) and `too_large` with a clear cause;
`pocket_rest` leaves exactly the tool's own corner radius and refuses a missing or smaller
`previous` with a message; 3D finishing heights match the sphere to 0.02 mm and the cutter
never goes under `top - depth`; `cam_job`s equal the direct calls; the gtree import keeps
parameters and sketch constraints live (`W` and `HD` edits rebuild to the hand volume) and
every skip has a reason; unknown-id errors in `document_set_text` list the allowed prefixes.

Not tested: `close` on three or more flanges (a box), hems and `angle` over 90 deg with `close`,
`close` with flanges off a contour flange, `contour_flange` holes and `field` stress bands read
out; sheet simulation of a U channel hand-checked (only runs: the width-10 channel works at
element 3 and 5); `sim_modal` shapes of the bracket; `surface_*` with `regions`, `angle`
other than the sketch's x, `stock_to_leave` (not a field), a non-hemisphere or filleted block,
and the ball cutter against a concave surface; `pocket_rest` with islands or after a
profile; G-code on a real controller; `move_face` with `angle`/`axis` on a tilted target,
several faces at once; trees with `symmetric`, `intersect`, arcs in this run (run 7 did them).

### Run 8 fix status (2026-11-07)

| # | Status |
|---|---|
| F8-1 | Fixed: nodes move along the mesh's exact normals, short edges collapse, flat caps are flipped and a folded quad is left out; a bent tab and contour flanges solve at every size tried (OCCT test). |
| F8-2 | Fixed: world revolve axes map onto the sketch's own x/y axis; the grammar says Gusset's X/Y are sketch axes. A part turn about the reversed axis is skipped with its reason. |
| F8-3 | Fixed: closed and mitred corners at tab corners that are not square (mirror plane; OCCT test at 60 and 120 degrees). |
| F8-4 | Fixed: edits list `still_failing`; flat-pattern query and export report rebuild errors. |
| F8-5 | Fixed (hints): text errors show the text spelling of a tool field and tool errors the tool's. |
| F8-6 | Open (documented in ADR 0037): flaps are drawn at the neutral layer. |
| F8-7 | Fixed: a hole across a bend and its wall (and a cut across a tab and a flange bend, a rectangle over a straight and its bend) unfolds as one outline; blind slivers at the bend's thin edge no longer refuse it. |
| F8-8 | Fixed: 3D finishing tessellates the model at 0.01 mm. |
| F8-9 | Fixed: ball operations' G-code says ball end mill. |
| F8-10 | Fixed: shell modes default to the longest side / 25 elements. |
| F8-11 | Fixed for agents: `field`/`shape` bands hold at most 2000 quads with coordinates to 0.01 mm (the app keeps 20000). |
| F8-12 | Fixed: `move_face` with faces on two bodies is refused up front. |
| F8-13 | Fixed: `sim_static` with no loads is refused. |
| F8-14 | Open (nits). |
| F8-15 | Fixed: 3D finishing without `top` is refused with what to give. |


## Run 9 — 3D CAM strategies, live rules, corners past 90, tree v2 (2026-11-07)

Setup: debug `gusset` with real OCCT and workers (`CARGO_INCREMENTAL=0`, 14 s from a warm cache),
`gusset mcp --no-attach` over stdio behind run 8's stdlib Python client (one daemon per project
plus a unix-socket caller, five projects in parallel); 608 tool calls (120 `document_set_text`,
119 `run`, 109 `query_body`, 100 `get_text`, 47 `cam_toolpath`, 39 `sim_static`), files in the
session's scratch directory (`dogfood9/`, projects and exports in `dogfood9/proj/`). Learned from
`tools/list` (31 tools, 18.9 kB), `commands` (88 commands, 11.9 kB), the grammar resource (44.8 kB)
and error messages. CAM paths were checked by sampling every cutting move at 0.05 mm against the
analytic surface (ball: centre-to-sphere distance; flat: highest surface point under the disc),
flat patterns by parsing the exported DXF, simulation against beam theory. Debug build, so the
timings are upper bounds.

| # | Part | Result | Calls | Notes |
|---|---|---|---|---|
| 1 | CAM on a R30 revolved dome (box +-40 and +-30): `waterline_ball/flat`, `steep_ball/flat`, `cam_job`s | contour radii exact, no gouge to 0.02 mm with the box clear of the part; boxed in, the path gouges (F9-1) | 40 | |
| 2 | CAM on an 80x60x10 plate with a d30 x 20 boss, base filleted r3: `waterline_*`, `steep_*`, `surface_*`, `pencil_ball`, G-code | contours and heights exact; `pencil_ball` finds no crease at a vertical wall (F9-2) | 60 | steep: 73-173 s each |
| 3 | `pencil_ball` on a V-groove, on floor + wall at 90/89.99/89.9/89/85/80/70/60/45 deg, on a square boss | V-groove exact; walls of 90 deg are missed (F9-2) | 25 | |
| 4 | `move_face` rules: block with a slot (coplanar tops), counterbore wall and floor, two separate pads | `also_moved` correct; `rules: false` gives the old behaviour | 30 | |
| 5 | Sheet metal: 100x60x2 tab, two 20 mm flanges (r2, k 0.5) at 90/120/150 deg, none / `close: true` / `close: mitre`; plan corners of 60 and 120 deg at 60/90/120/150 deg flange angle | volume by hand and flat area x t agree for every closed and mitred variant (24 + 9 builds) | 110 | |
| 6 | Cut d5 straddling bend and wall at 90, 120, 150 deg | unfolds as one cut-out each | 20 | |
| 7 | `.gtree.json` v2 of my own design: counterbore hole + `pattern_linear`, hole + `pattern_circular` (360 and 4 over 180), `mirror` of a cut (`features`) and of a body (`target`, `merge`), `shell` inside/outside; ten broken variants; the three plugin samples | volumes exact; every broken variant skipped with a reason | 60 | |
| 8 | `sim_static`/`sim_modal` on a 100x20x2 tab + 36 mm flange (r2) at element 6..2 and default; closed corners at 60/90/120/150 deg; `field`, `shape` | meshes at every size tried; within 2 % of beam theory | 90 | |

Hand checks:

| Check | Hand | Gusset |
|---|---|---|
| Dome R30, `waterline_ball` D6 stepdown 5, box +-40: tip z 25..0, centre on the R33 sphere, `rho = sqrt(33^2 - (z + 3)^2)` | 17.464 / 23.664 / 27.659 / 30.332 / 32.016 / 32.863 | 17.455 / 23.657 / 27.654 / 30.325 / 32.012 / 32.857; `cut_mm` 1030.11 against `2 pi sum rho` 1030.4; worst gap -0.018 (chord) |
| Same, `waterline_flat` (disc clears the dome at `rho - 3`): `rho = sqrt(900 - z^2) + 3` | 19.583 / 25.361 / 28.981 / 31.284 / 32.58 / 33.0 | 19.574 / 25.353 / 28.974 / 31.277 / 32.575 / 32.998; `cut_mm` 1072.78 against 1073.1 |
| `steep_ball`/`steep_flat` on the dome: 45 deg at `rho = 33 sin 45` = 23.3, tip z 20.3; raster inside, contours below | first level 18 (ball) / 21 (flat), raster `z = sqrt(33^2 - rho^2) - 3` | levels 18..0 / 21..0; raster z within 0.01 of the formula; worst gap -0.018 / (flat: the -0.89 is 0.01 mm of radius on the vertical rim); `cam_job` JW/JS equal the direct calls (1030.11 / 5329.84 mm) |
| Plate + boss R15 h20 on z 10, base fillet r3, ball D4 (r2): centre on a circle r1 round the fillet axis (18, 13): `rho = 17` for tip z >= 11, 18 at the floor level 10 | 17.0 x7, 18.0 | 16.996-17.000 for 27.5..12.5, 18.02-18.05 at 10; worst gap -0.014 |
| Same, `waterline_flat` D6, stepdown 5: `rho = 18` (z >= 13), 21 at z 10 | 18 / 18 / 18 / 21 | 17.995-18.0 x3, 20.995-21.0 |
| Boss, `steep_ball` D6 = fillet radius: centre on the fillet axis, `rho = 18` at every level; `steep_flat` at z 12 (disc over the fillet: `13 - sqrt(9 - (18 - rho')^2) = 12`, `rho' = 15.17`, `rho = rho' + 3`) | 18.0 / 18.17 | 17.995-18.0 for 27..12 and 18.03-18.07 at 10; flat z 12: 18.167-18.172 |
| Boss `surface_ball` at `rho` 17.8 (ball over the top edge): `30 - 3 + sqrt(9 - 2.8^2)` | 28.077 | 28.065; no sample under the surface by more than 0.004 (ball) / 0.06 (flat, my reference's grid) |
| V-groove (slopes 15/20, valley at z 5), `pencil_ball` D6: centre `r / cos(36.87)` = 3.75 above the valley | tip z 5.75 | 5.75 on x = 20 (F9-3 for the ends) |
| Waterline on a 60x40x20 block, sketch = footprint, D6 at the floor level: contour 100 x 2 + `2 pi 3` | 218.85 mm | 199.84 mm, the path runs along the box edge (F9-1) |
| Plate 80x60x10 + boss d30 x 20 (`48000 + pi 225 x 20`, 62137.17); fillet r3 `r^2 (1 - pi/4) 2 pi (15 + 0.2234 x 3)` | 62137.17 / +190.16 | 62137.166941 / +190.163729 |
| Block 60x40x20, slot 10 wide 5 deep across; move the left top +5 with rules: `46000 + 2 x 1000 x 5` | 56000 | 56000, `also_moved: [E1/cap(end)#2]`; `rules: false` 51000 |
| Counterbore d6/d12 x 5 in 60x40x20: `48000 - pi 9 x 20 - pi 27 x 5` | 47010.398 | 47010.398314; move `cbore_wall` +5 in x: `also_moved: [H1/wall(e1)]`, volume and area unchanged, hole at x 35; floor -3: `-pi 27 x 3` = -254.469 |
| Tab 100x60x2 + two 20 mm flanges r2 k 0.5, unclosed, 90 deg: `12000 + theta x 960 + 6400` | 19907.964 | 19907.964474; 120: hand 20410.6 / 150: 20913.3, solid 20346.688 / 20777.908 (F9-5) |
| Closed corner at 120 deg: walls cut at the plane `y = x - 40` (and `x = y + 40`); wall 1 = `integral (60 + 0.866 rho - 0.5 s)` over rho 2..4, s 0..20 = 2303.92, wall 2 = 3903.92, bends `2.0944 x 6 x 160` = 2010.62 | 20218.46 | 20218.465395 |
| Closed corner at 150 deg (`x = 100 + 0.5 rho - 0.866 s`): walls 2113.59 + 3713.59, bends 2513.27 | 20340.45 | 20340.4538 |
| Mitre gap, 0.1 mm on the diagonal: `0.1 x 40 / sin(corner / 2)` | 90 / 120 / 150 deg flange, square tab: -5.657; 60 deg tab corner: -8.0; 120 deg tab corner: -4.619 | -5.657 at all three flange angles; -8.000 (corner 60) / -4.618 (corner 120) for every flange angle |
| Flat area x t = volume at k 0.5, closed and mitred, 3 flange angles + 8 plan x angle cases | equal | equal to 0.001 mm3 in all 33 cases; DXF outlines 12 vertices (past 90) / 14 (90 and below) |
| Flat size at 120 deg: `100 + 2.0944 x 3 + 20` | 126.283 x 86.283 | 126.283185 x 86.283185 |
| Cut d5 across bend and wall (axis x, depth 4, centre at the bend end): bend `22.923` + wall `19.635` | 42.558 | 42.557033 at 90, 120 and 150 deg; flat cut-out area 20.857 by hand, 20.781 / 20.777 / 20.778 (polyline, -0.4 %), bbox x 101.7575..107.2124 by hand, 101.7575..107.2124 at 90 deg |
| Tree T1: 60x40x12 block, 3 counterbore holes (d6, d12 x 4) by `pattern_linear` | 26764.248 | 26764.24796 |
| T2: d60 disk x 10, d4 hole at r20, `pattern_circular` 6 x 360 / 4 x 180 | 27520.352 / 27771.679 | 27520.351645 / 27771.679058 |
| T3: 20x20x10, slot 4x10x4 mirrored across x = 10 (`features`); the same across the tilted plane `x + y = 10` (overlaps the first slot and the edge: `4000 - 160 - 80 + 36`) | 3680 / 3796 | 3680.0 / 3796.0 |
| T3b: base + boss 6x10x5, whole body mirrored across x = 0 with `merge` | 8600 | 8600.0 (bbox x -20..20) |
| T4: 40x30x20 `shell` inside 2, top removed: `24000 - 36 x 26 x 18`; outside: `44 x 34 x 22 - 24000` | 7152 / 8912 | 7152.0 / 8912.0 (bbox -2..42) |
| T1 with a countersink d10 x 90 deg (cone frustum 102.63, cylinder 56.55 per hole) / blind depth 8 with a 118 deg point | 27643.894 / 27052.573 | 27643.893903 / 27052.573166 |
| Plugin samples: Fusion 7740.0886 + 2356.194; SolidWorks 8538.053 | same | same; Onshape imports one body of 10096.283 (its `revolve` says `join`, the file's `bodies` list two parts), the `loft` is skipped with its reason |
| Bracket 100x20x2 + 36 mm flange (r2, k 0.5), 10 N down on the flange end: beam `P 103^3 / 3EI` 1.3009 and turning `P 103^2 / 2EI` 0.018945 rad x 39 | 1.496 mm | element 6 / 5 / 4 / 3 / 2.5 / 2 / default 2.6: 1.4573 / 1.4603 / 1.4357 / 1.4686 / 1.4700 / 1.4677 / 1.4702 (-1.8 %, scatter 2.4 %) |
| Same, root stress `M c / I` = 1030 x 1 / 13.33 | 77.3 MPa | 78.3 / 78.4 / 78.7 / 80.2 / 80.3 / 80.7 / 80.3 (+1.3 to +4.5 %) |
| Same, `sim_modal` first mode: `1/(2 pi) sqrt(3EI/L^3 / (0.2357 m_beam + m_wall))` with 0.0324 kg beam and 0.0122 kg wall | 99.2 Hz | 98.74 (default, 14.8 s) / 97.09 (5 mm); 379.1, 495.8 |

Observed behaviour worth knowing. The 3D finishing operations only cut inside the box of the
sketch's regions and tell nothing about it; a box tighter than the part's footprint plus the tool
radius clips contours (F9-1). Levels: `waterline_*` and `steep_*` start one `stepdown` below
`top` and end on `top - depth`; a level above the part is not listed. `steep_flat` stops at
the foot of a boss (no floor level), `steep_ball` has it. The result of a 3D operation reuses
`rings` (raster lines + contours). `cam_export_gcode` computes the path again (73 s for the
boss job, 72 s for the dome job, as long as `cam_toolpath`). Paths over about 4 kB come back as
a `full_text` pointer: the steep dome path is 40 kB (10 `get_text` calls), the refs from earlier
calls stayed valid for the whole session. `move_face` with `rules` pulls along every coplanar
face of the body, also a separate pad of the same height (`also_moved` lists it); the tool
defaults `rules` to true while the text language treats absent as off (the tool writes
`rules: true` into the text). After `close` the end faces of flanges past 90 deg are split, so
a load or support must name `F1/end(l6)#1`; the plain name is `not on 'Plate'` without
candidates. Sheet metal: past 90 deg a flange leans over the tab; `close: true` cuts each wall at
the mirror plane (walls shorter than the tab edge), `mitre` leaves the same 0.1 mm diagonal gap.
`sketch_create` on a leaning flange face puts the sketch origin at the world origin's projection
onto that plane (87 mm away from the wall at 120 deg), which needs a frame calculation to place
a hole.

| ID | Sev | Item | Expected |
|---|---|---|---|
| F9-1 | major | `waterline_*` (and the contour part of `steep_*`) keep the part of a contour inside the box of the regions and join the ends with a straight cutting move along the box edge, so the tool is dragged through the part. 60x40x20 block, sketch = its footprint, `waterline_ball` D6 `top: 20, depth: 20, stepdown: 100`: path `(0,0,0) -> (0,40,0) -> (59.77,40,0) -> (60,39.68,0) -> (60,0,0) -> (0,0,0)` at tip z 0, i.e. the ball centre on the wall plane (3 mm into the block); the contour round the block would be 218.85 mm, `cut_mm` is 199.84. Dome R30 in a box +-30: for the levels 10, 5, 0 (`rho` 30.3 .. 32.9, beyond the box) one chord of 8.9 / 22.3 / 26.8 mm joins the clipped ends and cuts 0.3 / 1.95 / 2.85 mm into the rim (worst gap, centre to sphere). With a box +-40 the same job has no gouge. The natural region (the part's own outline) triggers it. | Drop the parts of a contour that leave the box and lift (rapid) over the gap, or grow the box by the tool radius; say what was clipped. |
| F9-2 | major | `pencil_ball` finds no crease at an inside corner between a floor and a vertical wall, the most common one: plate + boss d30 (D6 and D4, with the box of the plate), plate + square boss, floor + wall: `no concave crease sharper than the bitangency angle`, hint `use a smaller tool or a larger region` (neither helps). A wall of 89.9 deg gives `only isolated crease points found`; 89 / 85 / 80 / 70 / 60 / 45 deg give one line (about 37 mm). A V-groove gives the exact valley (tip z 5.75). The ADR's test is a corner of two 45 deg planes. | Find the 90 deg corner (the profile jump at a vertical wall), or say that vertical walls are not seen; fix the hint. |
| F9-3 | minor | `pencil_ball` path shape and cost: along the V-groove (30 mm long) the path runs 2.75 mm past each end (y -2.75 .. 32.9) with the tip falling to z 3.5 / 3.07 below the valley's 5.75, `cut_mm` 38.02 for a 30 mm crease; with a fillet r3 and a ball r2 (smooth, expected nothing) it returns 8 pieces of 0.45 mm at `rho` 17, one per 45 deg of the boss, `cut_mm` 3.65, `rings` 8, after 140 s (debug). | Stop at the part's end; drop pieces shorter than a few grid steps; unconnected pieces should not be reported as a path. |
| F9-4 | minor | `feature_move_face` that cannot be built (`rules: false` on a counterbore wall, a sideways or turning move of a pad top with rules) is committed with `committed_with_errors` (`move face produced an invalid solid ...`, `the faces next to the moved ones would have to change shape ...`) and `rebuild_ok: false`; the result also lists `also_moved` although nothing moved. Run 8's F8-12 refused up front. Related: the tool's `rules` defaults to true, the grammar says absent = off. | Refuse before committing; say in the tool description that text without `rules` is off. |
| F9-5 | minor | Flanges past 90 deg without `close` overlap each other in the corner: the solid is smaller than the sum of its parts (20346.688 at 120 deg and 20777.908 at 150 deg, against 20410.619 / 20913.274 by hand), while `query_flat_pattern`/`sheet_export_flat` draw both flanges in full (DXF area x t = 20410.6 / 20913.3) with `changed_by: []` and no warning. | Warn that the flanges collide and suggest `close`. |
| F9-6 | minor | 3D CAM time (debug build): `steep_ball` 73 s (dome, 75 rings) and 173 s (boss, stepover 0.1, 125 rings), `waterline_ball` D4 over the boss 75 s but 3 s for `waterline_flat`, `pencil_ball` 140 s (F9-3); `cam_export_gcode` repeats the whole computation; steep results carry 11.6 m of rapids for 5.3 m of cutting (dome), 10.1 m for 11.0 m (boss). No progress, no estimate. | Reuse the last path for the export; report an estimate or `too_slow` before minutes of work. |
| F9-7 | nit | Agent-sized outputs are smaller but not small: `sim_static field` on the 6888-element bracket 111 kB (`stress_bands_thinned: true`, 28 pages of `get_text`), `sim_modal shape: 1` 82 kB (21 pages), a steep 3D path 40 kB; the quads of the stress bands are unordered samples. | A min/max/percentile summary with the location, bands on request. |
| F9-8 | nit | `sim_static`/`sim_modal` descriptions still say "on voxels"; on a sheet body the `voxel` argument is the shell element size (the result says `element_mm`, `too_large` says "give a larger element size"). `shape` is a mode number (1-based, integer) while `field` is a boolean; `shape: true` is `expected usize`, `shape: 0` is `shape names a mode from 1 to modes`. A load on a face that does not exist (`F9/end(l6)`) is `face F9/end(l6) is not on 'Br'` without the candidate names that the feature tools give. | Mention shells and `element` in the schema; candidates in the error. |
| F9-9 | nit | Mesh scatter on the bent bracket: displacement 1.4357..1.4702 across element sizes 6..2 (non-monotonic: element 4 is lowest), peak stress at the clamp +1.3 to +4.5 % of `M c / I`. Closed 120 deg corner at 8 / 5 / 4 mm: 0.0637 / 0.0716 / 0.0729 mm and 6.9 / 8.2 / 8.2 MPa (8 mm is 13 % / 16 % off 4 mm, 5 mm is 2 % / 1 % off). | Probably fine for a shell model; say that the stress at a clamped edge does not converge. |
| F9-10 | nit | The Onshape plugin sample's `bodies` (two parts, 7740.0886 + 2356.194) disagree with its own `revolve` (`operation: join`): one body of 10096.283 comes out and the import warns nothing about the changed body count. | Warn when the body count differs from `bodies`. |

Run 8 fixes seen holding: F8-1 (the bent bracket meshes at every element size tried, 6..2 mm and
the default; default solve 2 s against 87 s; modal default 14.8 s instead of `deadline_exceeded`),
F8-3 (closed and mitred corners at 60 and 120 deg plan corners), F8-7 (holes across a bend and
its wall unfold, also past 90 deg), F8-8 (surface finishing within 0.01 mm of the dome and boss),
F8-9 (G-code says `ball end mill D6 mm`; the first line still says `Gusset 2.5D toolpath`),
F8-10 (shell modal defaults), F8-13 (no loads is refused), F8-15 (`top` missing is refused with
`e.g. top: the model's highest point, depth: down to the lowest to finish`; `stepdown: 0` is
`stepdown must be positive`; `pencil_flat` is refused with the operation list).

What worked well: contour radii of `waterline_*` and `steep_*` match the analytic offset surfaces
to 0.01 mm (ball and flat, dome and boss with a fillet), the raster part of `steep_*` follows the
drop-cutter heights, `cam_job`s equal the direct calls, G-code headers name the ball mill and the
G-code z range equals the path's; the `rules` option does what the grammar says, reports
`also_moved`, and `rules: false` is the old behaviour; closed and mitred corners at 90, 120 and
150 deg (and at 60 / 120 deg plan corners) agree with a hand integral of the cut walls to 6 digits
and the flat pattern equals the volume at k 0.5 in all 33 cases, the mitre gap follows the corner
angle; a hole across a bend and its wall unfolds with the right bounding box; the v2 tree
importer is exact for hole variants, linear and circular patterns, feature and body mirrors (also
across a tilted vertical plane) and both shell sides, and every broken variant (no face at the
point, a position off the face, a count of 1, an unknown feature, v2 types in a v1 file, a mirror
or shell face that does not exist) is skipped with a precise reason and its dependants named; bent
sheet bodies now solve in seconds.

Not tested: `steep_*` with `regions`; `surface_*` with a rotated sketch; `pencil_ball` on a part
that has a crease between curved faces (boss on a sphere); a `cam_job` with a holder; G-code on a
controller; closed corners past 90 deg on three or more flanges, hems with `close`, `close` off a
contour flange; simulation of closed 120 / 150 deg corners against a hand model (only that they
solve and agree across sizes); `sim_modal` shapes read out; `move_face` with `rules` on a tilted
`onto` target, in a model with several bodies, or on cones; the `revolve` axis fix of run 8;
trees with parameters driving `count` or `spacing`, `pattern_*` of a fillet or chamfer, `mirror`
of a revolve, 256 or more positions in one hole.
### Run 9 fix status (2026-11-07)

| # | Status |
|---|---|
| F9-1 | Fixed: a contour cut by the bounds box becomes open runs that end on the box edge; they are linked by retract and rapid, never by a cutting move along the box (ADR 0040 amendment). A box that holds only the unreachable part of a contour gives `NothingToCut` for that level. An explicit box limits the tool centre and is not grown; the default box is the mesh grown by the tool radius. Tests: the 60x40x20 block with its footprint as the box, a dome in a tight box (brute-force gouge check, no segment along the box). |
| F9-2 | Fixed: `pencil_ball` finds the floor/wall corner at a vertical wall (and a wall leaning by 0.1 deg) by bisecting the step of the cutter-location field; a boss on a plate gives a closed loop at the floor height at distance r from its walls (cylinder: circle of R + r; square boss: rounded square). |
| F9-3 | Fixed: crease points must be held by two surfaces, contacts on the mesh's free boundary do not count, so the line stops at the end of the part (tip never below the crease); chains shorter than 4 grid steps are dropped. The faceted-fillet case now costs seconds (see F9-6). |
| F9-6 | Fixed in part: ball-edge contact in closed form and a bounding-box skip make the drop-cutter about 40x faster (dome R30, D6, stepdown 0.4: `steep_shallow` 328 s -> 8.5 s, `waterline` 225 s -> 5.7 s, release; the whole `gusset-cam` test suite 67 s -> 2.6 s release, 14.6 s debug); shallow raster runs are ordered nearest first (rapid 42.4 -> 38.9 m on the dome). Open: lower retract heights, reuse of the last path for `cam_export_gcode`, a progress/time estimate. |
| F9-4 | Fixed: every move_face is tried before committing; one the kernel cannot build is refused (OCCT test). The command defaults `rules` to true; in the text an absent `rules` means false (old documents). |
| F9-5 | Open: flanges bent past 90 deg without `close` collide in the corner silently. |
| F9-7 | Partly: agent fields are capped at 2000 quads; paths stay as computed. |
| F9-8 | Fixed: sim_static says sheet metal runs on shells. |
| F9-9 | Open (nit): element-size scatter on bent sheets within 3 %. |
| F9-10 | Fixed: the import warns when it makes a different number of bodies than the source lists. |

## Run 10 — run 9 fixes and three end-to-end parts (2026-11-07)

Setup: debug `gusset` with real OCCT and workers (`CARGO_INCREMENTAL=0`, 3 min rebuild after the
run 9 fixes), `gusset mcp --no-attach` over stdio behind run 9's Python client (a daemon and
unix socket per project; three projects, `dogfood10/`). 183 client calls (28 `run`, 37
`cam_toolpath`, 35 `query_body`, 20 `get_text`, 14 `sim_static`, 14 `document_set_text`), learned
from `tools/list` (31 tools, 18.9 kB), `commands` (12.4 kB, `commands {name}` gives one command's
schema) and error messages. Paths were checked by sampling every move at 0.05 mm against the analytic
surface (ball: centre-to-profile distance in the (rho, z) plane), flat patterns by parsing the DXF,
the shell by beam/plate theory. Debug build, so timings are upper bounds.

| # | Part | Result | Notes |
|---|---|---|---|
| 1 | Run 9 fixes: 60x40x20 block (box = footprint, +-5, +-10), dome R30 in a +-30 box, plate + d30 boss, plate + square boss, 45 deg V-groove, counterbore block, 60x40x20 | F9-1/2/3/4/6 hold (hand checks below) | 40 calls |
| 2 | (a) Sheet-metal enclosure: tab 120x80x1.5, four 25 mm flanges (r1.5), F2..F4 `close: true` (also `mitre`, k 0.44), 4 x d6 in the base, d8 in a flange, flat pattern + DXF, `sim_static` | volume, flat size, outline area, cut-outs and DXF circle positions equal the hand values; shell static within 25 % of a strip estimate (F10-5, F10-6) | 45 calls |
| 3 | (b) Machined bracket 100x60x10 + d24 x 12 boss (base fillet r3) + pocket 40x40x6 + 4 x d6 through: `pocket`, `pocket_adaptive`, `pocket_rest`, `drill`, `waterline_ball/flat`, `steep_ball`, `surface_ball`, `pencil_ball`, six `cam_job`s in the document, G-code | contours, levels and G83 cycle exact; `surface_ball` gouges 0.48 mm on the steep boss wall (F10-1) | 70 calls |
| 4 | (c) `.gtree.json` v2 of my own (block, `shell` inside/bottom open, `Pad` join, `mirror` of the pad, counterbore `hole` + `pattern_linear` x3) imported, then `move_face` with rules (default, `false`, cbore wall, top face, refusals), undo/redo | all 11 features ok, volume exact; every move equals area x distance; 3 refusals commit nothing | 25 calls |

Hand checks:

| Check | Hand | Gusset |
|---|---|---|
| `waterline_ball` D6 on the 60x40x20 block, box -10..70 x -10..50, level z 0: contour round the block at 3 mm | `2 (60 + 40) + 2 pi 3` = 218.85 | 218.83, one piece, x -3..63, y -3..43 |
| Same, box 0..60 x -5..45 (the long sides inside, the ends clipped) | two runs of 60 | two pieces at y -3 and y 43, x 0..59.768, joined by rapid, `cut_mm` 119.54 (F10-7) |
| Same, box = footprint or +-1 | tool centre can never reach the contour | `no waterline contour crosses the surface` (waterline); `steep_*` returns raster only, `levels_mm: []` (F10-2) |
| Dome R30, box +-30, `waterline_ball` D6 stepdown 5: rho at z 25..0 = `sqrt(33^2 - (z + 3)^2)` | 17.464 / 23.664 / 27.659 / 30.332 / 32.016 / 32.863 | 17.455 / 23.657 / 27.654 / 30.325 / 32.012 / 32.857 as before; the four levels past the box are four open runs each (rho 30.32, 32.01, 32.86), no segment on the box edge; worst gap over all moves -0.0138; `steep_ball` worst gap -0.023 |
| Plate + boss d30, `pencil_ball` D6 / D4 with the plate as box: loop at floor z 10, rho = 15 + 3 / 15 + 2 | 113.10 / 106.81 | 113.08 (rho 17.995..18.0, tip z 10) / 107.06 (rho 16.996..17.0; +0.2 %) |
| Plate + 30x30 square boss, D6: rounded square, x 22..58, y 12..48 | 138.85 | 138.84, x 22..58, y 12..48, z 10 |
| 45 deg V-groove, valley z 5, `pencil_ball` D6: centre `3 / sin 45` above the valley | tip z 6.2426 | 6.243 along x 20, y 0.375..29.625 (ends one grid step short, never past) |
| Boss d24 with base fillet r3 on a plate (fillet axis rho 15, z 13), D6 = fillet radius: `waterline_ball` levels 18/14/10, `waterline_flat` | rho 15 / 15 / 15; flat 15 / 15 / 18 (disc clears the fillet foot); `2 pi (15 + 15 + 18)` = 301.6 | ball 14.996..15.0 x2 and 15.032..15.037 at 10; flat 14.996..15.0 x2 and 17.995..18.0; `cut_mm` 282.9 / 301.53 |
| Same boss, `pencil_ball` D4 (ball r2 inside the r3 fillet, smooth), depth to z 10 | nothing | `no concave crease sharper than the bitangency angle` (hint wrong, F10-4); D6 (= fillet radius) gives the loop rho 15, 94.46 vs 94.25 |
| Same part, D4 with depth to z 4 (pocket floor): inset rectangle x 14..50, y 12..48 at tip z 4 | `2 (36 + 36)` = 144 | 144.7 incl. plunge, x 14..50, y 12..48, z 4; plus four 3-4 mm stubs at the pocket's rim corners (F10-4) |
| Pocket 40x40 D8, stepover 0.4 (3.2 mm), two levels 7, 4 (top 10, stepdown 3) | rings 32/25.6/19.2/12.8/6.4 square: 128 + 102.4 + 76.8 + 51.2 + 25.6 = 384 per level | 384.0 per level at z 7 and 4, tool centre x 16..48, y 14..46; `cut_mm` 1741 includes 973 mm of ramp (F10-3) |
| `pocket_rest` D4 after D10 (corner arc r5): leg 15 - 12 = 3 (2.92), arc of centre radius 3, leg | 10.4 per corner per level, 83.2 | 41.6 per level (x 14..50, y 12..48), corner loops 2.92 + 4.6 + 2.92; `cut_mm` 582.95, 500 of it ramp |
| `drill` D6 peck 4 in 10: four holes | G83 `Z0 R11 Q4` | `G98 G83 X8 Y8 Z0 R11 Q4 F200`, then X8 Y52, X92 Y52, X92 Y8, `G80` |
| G-code z / x range of `Fin` (waterline) | z 10..27, x 75 +- 15 | z 10..27 (safe 27), x 59.966..90.034 (+0.034); 407 lines |
| Bracket: plate 100x60x10 + boss d24 x 12 (`65428.672`), fillet r3 (`r^2 (1 - pi/4) 2 pi (12 + 0.2234 r)` = 153.76), pocket 9600, 4 holes `4 pi 9 x 10` | 54851.459 | 54851.456136 (fillet +153.757386) |
| Enclosure, k 0.5, 4 closed corners (1.5 mm, r1.5): `14400 + 400 (pi/4)(2 r t + t^2) + 400 x 25 x 1.5` + 4 corners x 2 x 3.375 x 25 | 31520.575 + 675 = 32195.575 | 32195.575041 (`close: true` on F2..F4) |
| Holes: 4 x d6 and d8 in the flange (t 1.5) | -169.646 / -75.398 | -169.646003 / -75.398224 |
| Flat size: `120 + 2 (25 + pi/2 (1.5 + 0.5 x 1.5))`, k 0.44 | 177.0686 x 137.0686 / 176.7858 x 136.7858 | same; outline area (DXF, 36 vertices) 21463.7166 x 1.5 = 32195.57 (k 0.5); k 0.44 true / mitre 21407.168 / 21375.026 |
| Flat hole in the flange at height 15: `-(3.534 + 15 - 3)` | y -15.534 (DXF 23.0 with the 38.534 offset) | circle (98.534, 23.0) r 4; the four base circles at 20/100/20/100 + 38.534 |
| Mitre gap, 0.1 mm on the diagonal: `4 x 0.1 x (1.5 sqrt 2) x 25` | 21.21 | 31950.531 - 31929.318 = 21.213 |
| Enclosure shell static (all flange faces, base underside held), 10 N on `F1/flat(l5)`: root stress `6 M / t^2` with `M = w L / 2`, `w = 10 / 120`, `L = 26.5` | 2.94 MPa; tip `w L^3 / (8 D)` = 0.0030 mm (D = E t^3 / 12 (1 - nu^2)) | element 12 / 10 / 8 / 6 / 5: 0.00364 / 0.00352 / 0.00369 / 0.00402 / 0.00392 mm; 2.36 / 2.39 / 2.54 / 3.07 / 3.96 MPa (6.8 s at 5 mm) |
| Tree: base 80x50x20 `shell` inside 2, bottom open (`80000 - 76 x 46 x 18`), two pads 20x20x8 (mirror at x 40), 3 counterbore holes cut through the 2 mm top (`3 pi 36 x 2`) | `17072 + 6400 - 678.584` = 22793.416 | 22793.415987, imported in 0.42 s, 11 features all ok |
| `move_face` of `Pad/cap(end)` +3 z, rules default / `false` | +2400 (both pad tops, `also_moved: [Pad_Mir1/cap(end)]`) / +1200 | +2400.0 / +1200.0; undo and redo return 22793.416 and 28514.832 |
| `move_face` of the top face (area 2860.708) +2 / -1 | +5721.416 / -2860.708 | +5721.415987 / -2860.707993 |
| Counterbore block: floor -3 / wall +5 with rules | -254.469 / 0 | -254.469005 / 0, `also_moved: [H1/wall(e1)]` |

Run 9 fixes seen holding: F9-1 (an explicit box limits the tool centre; contours are cut into runs
that end on the box edge and are joined by rapids; no segment along the box in any case tried), F9-2
(cylinder, square boss; exact radii), F9-3 (the crease line stops inside the part, a smooth fillet
gives nothing, no 0.45 mm pieces), F9-4 (a cbore wall with `rules: false`, a sideways or turning move,
and a pad top 10 mm down are refused; query_model shows no new feature each time), F9-6 (debug
build: `steep_ball` on the boss 6.7 s against 173 s, waterline 4 s against 75 s, `waterline_flat`
0.7 s, `surface_ball` 0.5 s, `pencil_ball` 8-12 s against 140 s; the G-code export of a job still
recomputes: 3.9 s for the waterline job), F9-8 (the description says "sheet metal on shells").

| ID | Sev | Item | Expected |
|---|---|---|---|
| F10-1 | major | `surface_ball` (D6, stepover 0.1 and 0.05, box = the bracket's 100x60, top 22, depth 12) joins samples across a steep wall with one straight move: the segment (89.222, 27.6, 20.765) -> (89.82, 27.6, 10.0) drops 10.8 mm in 0.6 mm of x and cuts 0.484 mm into the boss's rim and wall (centre-to-profile distance 2.516 for r 3). 107 of 959 segments gouge by more than 0.05 mm; the vertices themselves are within 0.004 mm of the surface, and the worst gap does not change with the stepover (-0.4839 at both), so the x sampling is the cause. `steep_ball` on the same part: worst -0.023, waterline -0.034. | Refine the samples where the height jumps (or split at the CL surface's steep part) so the chord error stays under the tolerance; `surface_flat` was not checked. |
| F10-2 | minor | A box equal to (or a little bigger than) the part's footprint, the natural region, gives `no waterline contour crosses the surface`, hint `use a smaller tool or a larger region`: the message says nothing about the box limiting the tool centre (the contour lies 3 mm outside); `steep_*` in the same case returns the raster over the top and `levels_mm: []` without a note. The 3 mm of box growth is the fix, not "a smaller tool". | Say "the contour lies outside the region (it limits the tool centre); grow it by the tool radius" and mention the empty levels in `steep_*`. |
| F10-3 | minor | 2.5D entries are mostly air: the pocket (D8, 2 levels) is 1741 mm of which 973 mm is ramp at 3 deg, every ring ramps down again from the stock top (10) through a ring the inner ring has already cleared (ring 2 at x 25.6 lies inside the +-4 cleared round ring 1); `pocket_rest` 583 mm of which 500 mm is the same spiral, 10 turns of a 10 mm corner for 3 mm of depth. Estimates (3.26 min for the pocket) count it at feed. | Enter each ring from the depth already cut, or at the previous ring's z; drop the spiral for a short corner pass. |
| F10-4 | minor | `pencil_ball`: (a) four 3-4 mm stubs per pocket at the rim corners (x 12.75..14, y 10.75..12, z 8..10.85 and the three mirror positions, two of them asymmetric: z top 10.854 / 9.854): the ball is not touching two surfaces there; (b) `depth` that ends above the creases (here: pocket floor at z 4, depth to z 10) reports `no concave crease sharper than the bitangency angle` after 11.6 s with the hint `use a smaller tool or a larger region`; neither is the answer; a smooth r3 fillet with D4 correctly gives nothing. | Drop pieces that are not held by two surfaces at their ends; hint "no crease between top and top - depth; increase depth". |
| F10-5 | minor | `sim_static` on the 177x137x28 enclosure: the default element size fails with `too_large` ("the shell model needs more than 40000 triangles: give a larger element size") although the longest side / 30 is 5.9 mm and `voxel` 6 is 12.5k triangles; the argument is called `voxel` ("Voxel edge mm (default longest side/30)") and nothing says it is the shell element size. The size check runs before the face lookup, so a wrong face name appears only at a coarse size. After `close` on all flanges the end faces merge: only `F1/end(l5)` exists (area 627 = four ends + corner pieces, centroid at the middle of the part), `F3/end(l7)` is `face F3/end(l7) is not on 'Plate'` without candidates, so a load on one flange's end is not possible (a load on `F1/end(l5)` is spread over all four). | Pick the default from the thickness/area so that it fits, or fall back; describe `voxel` as the shell element size; list candidate faces in the error; name the merged face for what it is. |
| F10-6 | nit | Mesh scatter on the closed enclosure: tip 0.00364 / 0.00352 / 0.00369 / 0.00402 / 0.00392 mm for element 12 / 10 / 8 / 6 / 5 (+-7 %, 20-30 % above the strip estimate 0.0030 with the d8 hole and the open corners), root stress 2.36 .. 3.96 MPa (hand 2.94) rising with refinement at the clamp; the location of the peak moves. | Say that the clamp stress does not converge (as F9-9). |
| F10-7 | nit | A waterline run clipped by the box ends 0.23 mm short at one end (x 59.768 of 60.0, 0.0 at the other); the cut at that level is 0.46 mm shorter than the contour inside the box. | End on the box edge. |
| F10-8 | nit | `feature_move_face` with numbers (`translate: [5, 0, 0]`): `invalid type: integer `5`, expected a string`, no field name and no hint that lengths are strings with units (the sketch tools take numbers); `commands {name}` shows it. For `rules: false` on a counterbore wall the hint is `move the faces less far, or together with the faces around them` (the answer is `rules: true`). | Name the field and an example (`"5 mm"`); mention `rules`. |
| F10-9 | nit | G-code of 3D jobs still starts `(Gusset 2.5D toolpath: ball end mill D6 mm)`; the export recomputes the path (3.9 s, as long as the toolpath; F9-6's open part); `cam_toolpath` with `path: true` costs up to 23 kB for a waterline and a steep path comes as a pointer of 28 kB (7 `get_text` pages), `query_body names` of the enclosure is 15 kB. | |

What worked well: the closed-corner enclosure equals the hand volume to 1e-6 (the 2 x 3.375 x 25
corner pieces), holes in the base and across a flange unfold to the right DXF positions, k 0.44
and 0.5 give the hand flat size, the mitre gap equals `4 x 0.1 x 1.5 sqrt 2 x 25`; `commands {name}`
makes each rarely used tool's schema cheap to learn; `cam_job`s in the text equal the direct calls
and `drill` exports a G83 cycle with the right `R` and `Q`; the v2 tree with a shell, mirror and
pattern imports in 0.4 s with exact volume and `move_face` live rules move exactly the coplanar pad
tops, also after undo and redo; `onto` a plane the face already lies in is refused with a clear
message; 3D finishing is now interactive (seconds) in a debug build.

Not tested: `surface_flat` and `steep_flat` for chord gouges; `pencil_ball` on a creased curved
part; mitre/close at 120 and 150 deg with the holes of this run; `sim_modal` of the enclosure;
bores (`bore`) and `profile_*` tabs on the bracket; G-code run on a controller; a `cam_job` with
a holder; F9-5 (flange collisions past 90 deg without `close`), F9-10 not re-checked.

### Run 10 fix status (2026-11-07)

| # | Status |
|---|---|
| F10-1 | Fixed: raster chords are checked against the true cutter-location height (midpoint, quarter and end-halving probes, split to depth 9) and line simplification keeps any point the chord would cut below; a brute-force check samples every cutting segment every 0.05 mm (gap under 0.005 mm) for ball, flat and bull on a vertical-wall boss and a steep fillet. |
| F10-2 | Fixed: an explicit box that holds no contour says the box limits the tool centre and to grow it by the tool radius; steep_* reports the same instead of a raster alone. |
| F10-3 | Partly: rings after the first start nearest the previous ring and are entered by a feed move at depth (ramp under 25 % of cutting on a square pocket); the first ring of a rest pocket still spirals in. |
| F10-4 | Partly: a ball held only by sharp rim edges no longer makes a pencil crease (synthetic rim test; the run-10 part not re-run); a depth above the creases fails at once with what to change. |
| F10-5 | Fixed: the shell element size grows until a large sheet part fits when none is given; unknown faces list the body's faces of that feature. |
| F10-6 | Open (nit): mesh scatter on the enclosure shell. |
| F10-7 | Fixed: clipped waterline runs reach the box edge. |
| F10-8 | Fixed: move_face refusals with rules off say what rules would move; argument errors name the field (`translate[0]`) and say lengths are strings with units. |
| F10-9 | Fixed: the G-code header says "Gusset toolpath"; exports reuse the last toolpath for the same job and model; output sizes as before. |

## Run 11 — holder checks, feature_add, run 10 follow-ups (2026-11-07)

Setup: debug `gusset` with real OCCT and workers (`CARGO_INCREMENTAL=0`, 14 s rebuild), `gusset mcp
--no-attach` over stdio behind run 10's Python client (a daemon and unix socket per project; four
projects, `dogfood11/`). 308 client calls (99 `cam_toolpath`, 64 `get_text` pages to read the long raster
paths, 57 `run`, 24 `sim_static`, 14 `history_undo`, 10 `query_body`, 5 `cam_export_gcode`), more than
planned because the raster paths come as pointers and every holder case was swept. Learned from `tools/list`
(31 tools, 18.3 kB), `commands` and `commands {name}` and error messages only. Every path was sampled at
0.1-0.5 mm against my own height function of the part (plate, cylinder boss, pocket): holder bottom
`tip z + stickout` must be at or above the highest material within holder radius, shank bottom `tip z + flute`
above the highest material within shank radius (0.03 mm tolerance on the radii). Debug build, so timings are
upper bounds.

| # | Part | Result | Notes |
|---|---|---|---|
| 1 | `holder` on 3D ops: plate 100x60x10 + d20 boss to z 30, `BOX` -10..110 x -10..70, D6 ball, top 30, depth 20, stepdown 5; `waterline_ball/flat`, `surface_ball/flat`, `steep_ball/flat`, `pencil_ball`; holders `[6,10,20,12]`, `[6,10,40,14]`, `[10,8,12,30]`, `[6,12,8,30]`, `[6,10,20,40]` | Warning appears exactly when the holder or shank can touch; lift amounts equal the hand values; waterline and pencil paths are clear at every sample; long stickout gives no warning and an unchanged path; raster paths (surface/steep) are not clear between samples (F11-1); a shank of the tool's own diameter next to a plane wall is lifted at random (F11-2) | 95 calls |
| 2 | Refusals and errors of `holder`: 3 / 5 / 0 values, string, negative, 0, stickout < flute, `[8,10,6,12]`, `[3,10,6,12]`, stickout 1e9; 2D ops (`pocket`, `profile_outside`, `drill`) | wrong lengths and non-positive values give clear `invalid_args`; 2D ops warn `holder is checked only for 3D finishing; ignored`; `holder D < shank D` and a shank thinner than the tool are accepted silently (F11-7) | 14 calls |
| 3 | `feature_add`: `cam_job` x 9 (pocket, adaptive, rest, waterline, surface, pencil, bore), `fillet`, `chamfer`, `extrude` cut, a suppressed job, `label`/`group`; refusals (duplicate name, sketch, import x3, malformed x4, unknown sketch/profile/operation, empty, two statements, trailing text, `param`); undo/redo; at a rollback point | all adds rebuild and run with `cam_toolpath {job}`; every refusal commits nothing; undo/redo restore the text byte for byte; rollback 3 inserts after the first three items | 55 calls |
| 4 | Run 10 open items on a rebuilt bracket (100x60x10 + d24 boss to z 22, fillet r3, pocket 40x40x6, 4 x d6) and the enclosure: F10-3, F10-4, F10-6 | F10-3 still reproduces (pocket_rest first ring); F10-4 stubs are gone on the real part, the depth message misleads (F11-4); F10-6 is mostly run-to-run noise in `sim_static` on shells (F11-5) | 75 calls |
| 5 | Fresh part (d): block 80x50x30 + d16 x 10 boss to z 40, 30x30 pocket 20 deep, d6 through hole, fillet r2; `sim_static`, `pocket_adaptive`, `surface_ball` floor finish with `[6,12,16,15]` (holder must stay out of the 20 mm deep pocket's rim band), `bore`, G-code with holder | volumes equal the hand values to 1e-6; solid static stress within 2 % of beam theory; the floor finish is lifted by exactly the hand 5 mm in the 8 mm rim band (with 1.3 mm extra) and 10 of 613 segments clip the rim (F11-1); G-code z 10..35 | 60 calls |

Hand checks:

| Check | Hand | Gusset |
|---|---|---|
| Plate 100x60x10 + d20 boss (sketch on z 0, extrude 30: top z 30, so boss is 20 above the plate) | `60000 + pi 100 20` = 66283.185 | 66283.185307; bbox z 0..30 (my first guess of 40 was wrong: the boss sketch is on the XY plane, `distance` is from the sketch) |
| Base fillet r2 on the boss, `feature_add` | `r^2 (1 - pi/4) 2 pi (R + 0.2234 r)` = 56.345 | +56.344809 |
| Chamfer 1 mm on the boss top edge (R 8) | `0.5 x 1 x 1 x 2 pi (8 - 1/3)` = 24.0855 | -24.085544 |
| Pocket 30x20x6 | -3600 | -3600.0 |
| Bracket (plate 100x60x10, boss d24 to z 22, fillet r3, pocket 40x40x6, 4 x d6 through) | 54851.456 | 54851.456136 (as run 10) |
| Part d: block 80x50x30 + boss d16 x 10 - pocket 30x30x20 - d6 x 40 hole + fillet r2 (45.5577) | `120000 + 2010.619 - 18000 - 1130.973 + 45.558` = 102925.204 | 102925.203687 |
| `waterline_ball` D6, holder `[6,10,20,12]`, levels 25/20/15/10, boss top 30 at rho 13 (holder radius 10 overlaps the boss): bottom `tip + 12` must reach 30 | lift 0 / 0 / 3 / 8 (level 20: bottom 32; level 15: 27; level 10: 22) | 0 / 0 / +3.0 / +8.0 (`deepest 8.000 mm`), rings 3 and 4 both end up at z 18 |
| Same, `[6,10,40,14]` (holder D 40) | level 10: bottom 24, lift 6 | `deepest 6.000 mm` |
| Same, shank D 10 over a D6 ball, `[10,8,12,30]`: shank bottom `tip + 8` must reach 30 within radius 5 | level 10 tip 22, lift 12 | `deepest 12.000 mm`, 378 positions lifted |
| `[6,10,20,40]` and `[6,12,8,30]` (stickout 40 / holder D 8 with stickout 30) | no contact | no warning, path identical to the no-holder path (326.65 mm, 515 moves) |
| Deep pocket (rim 30, walls x 10..40), `surface_ball` D6, `[6,12,16,15]`, tool centre x 13..37: holder reaches the rim when the centre is within 8 mm, bottom 25 against 30 | lift 5 for x < 18 or x > 32 (and y); unlifted square 18..32 | `deepest 5.000 mm`; points at tip z 10 lie in x 19.3..30.7, y 19..31 (1.3 mm of extra lift), the others at z 15 (x 13.6..36.4) |
| Same pocket, waterline levels 10/15 with `[6,12,16,15]` | level 10: pocket wall and block outside 5; level 15: 0 | pocket ring +5.0 (10 of 10 positions); outer ring +5.0 or +8.0, level 15 +0 or +3.0 (F11-2: no holder can touch there) |
| Raster lift ramps (`surface_ball`, plate + boss, `[6,10,20,12]`): segment (56, 10.7, 10) to (53, 10.7, 18) | the holder bottom must clear 30 where the centre is within 20 mm of (50, 30): crossing at x 55.6 is at z 11.3 + 12 = 23.3 | segment check at 0.1 mm: bottom 5.65 mm below the boss top (88 of 3442 segments); `steep_ball` 130 of 3923 (worst -4.61), `surface_flat` 44 of 1088 (-5.55), `steep_flat` 132 of 3256, pocket `surface_ball` 10 of 613 (-2.61) |
| `pencil_ball` D4 on the bracket, depth to z 4 | rectangle x 14..50, y 12..48 at z 4: 144 | 144.0 (path 145 with the 1 mm plunge); no stubs |
| `pencil_ball` D6 (= fillet r) | loop rho 15: 94.25 | 94.45 (x 59.968..90.027, y 14.972..45.028, z 10) |
| Rest pocket (30x20, D8 then D4, stepdown 3, two levels), first ring | corner loop of a 1.94 mm corner: a few mm | 8 loops (4 corners x 2 levels), each x 36.06..38.0, y 12..13.94, 73.9 mm incl. approach, 112 points; 496.5 mm of 551.5 mm is sloped ramp (3 deg, 57.6 mm per level) |
| `sim_static` part d (50 N x on the boss top, base fixed): root `M c / I`, `I = pi/64 (16^4 - 6^4)` = 3153 mm^4, M 500 N mm | 1.27 MPa; tip: flexure 2.5e-5 + shear 6e-5 + root rotation 3e-5 = 1.2e-4 mm | 1.251 / 1.243 / 1.460 MPa and 0.000152 / 0.000146 / 0.000167 mm at default / 2 / 1.5 mm (5.5k / 12.9k / 30k elements), 0.2-1 s |
| Enclosure 177x137 (k 0.5, closed corners), volume | 32195.575 | 32195.575041 |

| ID | Sev | Finding | Repro | Suggestion |
|---|---|---|---|---|
| F11-1 | major | The holder lift of raster paths (`surface_*`, `steep_*`) is applied per position, but the moves between a lifted and an unlifted position stay straight, so the lift ramp cuts through the zone it was meant to clear: the holder bottom (or shank) is inside material for the middle of the move. The warning ("lifted by up to 8 mm") says the path is safe. Waterline and pencil are clean because whole rings lift together. | Plate 100x60x10 + d20 boss to z 30, box -10..110 x -10..70, D6, top 30, depth 20, `holder: [6,10,20,12]`: `surface_ball` stepover 0.15 has the segment (56, 10.7, 10.0) -> (53, 10.7, 18.0) (8.5 mm long): at its crossing of the 20 mm radius round the boss axis the tip is at z 11.3, holder bottom 23.3, boss top 30 (6.7 mm in); 88 of 3442 segments clip by more than 0.02 mm, worst 5.65. Same for `steep_ball` 130/3923, `surface_flat` 44/1088 and `steep_flat` 132/3256 segments. Deep pocket (rim 30, `[6,12,16,15]`, `surface_ball` on the pocket sketch): 10 of 613, worst 2.61. | Check each move (not only each vertex) against the same clearance, split a lift ramp where it crosses the clearance boundary (rise vertically at the boundary), or lift the whole stroke that contains a collision. |
| F11-2 | major | Tangent contact counts as a collision: with a shank of the ball's diameter (`[6,10,16,60]` on a D6 ball, the plain straight-shank tool) the contour round a planar wall touches the shank at every height, and positions are lifted by up to 10 mm or not at random (floating-point noise decides); shank 5.9 gives no warning, 6.1 lifts the whole ring. The result is a half lifted contour (a zig-zag of 15 / 20 mm rings) and a warning about collisions that are not there. A round boss gave none (chords stay inside), so only plane walls show it. | Block 80x50x30 with a 30x30x20 pocket and boss, `waterline_ball` D6, top 40, depth 30, stepdown 5, `holder: [6,10,16,60]`: `84 collisions (deepest 10.000 mm)`; level 15 outer ring: 29 of 53 positions +5, the rest +0; level 10: 29 of 53 +10. `[5.9,...]`: no warning; `[6.1,...]`: 346 positions lifted. With `[6,12,16,15]` the outer ring is lifted by 8 (level 10) / 3 (level 15) where the hand value is 5 / 0 (the shank, not the holder, decides: holder D 12, 16 and 20 give identical lifts). | Treat a clearance of less than a small tolerance (say 1e-3 mm, or the sampling error) as touching but not colliding; add a note when the shank is no wider than the tool. |
| F11-3 | minor | A `cam_job` in the document cannot carry a holder: `holder: [...]` gives `'cam_job' has no argument 'holder' (allowed: sketch, regions, operation, ...)`, so a job saved with the part is checked only when someone passes `holder` again to `cam_toolpath {job, holder}` / `cam_export_gcode {job, holder}` (both accept it and key the cache on it). Overrides next to `job` are inconsistent: `depth` is refused (`job takes its fields from the cam_job feature`), `ramp_deg` is accepted and ignored (`pocket_rest` job with `ramp_deg: 0` still 551.54 mm; the same call without `job` gives 55.12 mm). | `feature_add 'cam_job Fin (..., holder: [6, 10, 20, 12])'`; `cam_toolpath {job: Rest, ramp_deg: 0}`. | Add `holder` (and `ramp`, already allowed in the grammar, as the way to set it) to the job; refuse or honour every override next to `job`. |
| F11-4 | minor | `pencil_ball` with a depth that ends above the creases says `no crease is higher than z X`, but X follows the stock bottom, not the creases: bracket, D4, top 22: depth 12 (bottom 10) says 9.854, depth 13 says 8.968, depth 14, 15, 17 say 4; depth 18 works. Following the message ("increase the depth") by the amount it implies fails again twice. | `cam_toolpath {sketch: BOX, operation: pencil_ball, tool_diameter: 4, top: 22, depth: 12}` on the bracket (pocket floor z 4); the hint `use a smaller tool or a larger region` is also wrong here. | Report the real highest crease (4) and the depth that reaches it (`depth >= top - 4`); drop the tool/region hint for this error. |
| F11-5 | minor | `sim_static` on a sheet-metal shell is not deterministic: identical calls give different element counts and results. Voxel solids are (three identical runs). F10-6's "scatter" is therefore partly run-to-run noise, not only mesh dependence. `max_displacement_at_mm` is bit-identical (48.3663, -2.24297, 28.0) for element 12 / 10 / 8 / 6 / 4 and sits 11.6 mm off the symmetric midspan (x 60). | Enclosure 120x80 base, 25 mm flanges, closed corners (32195.575 mm^3), `fixed: [Plate/cap(start)]`, 10 N on `F1/flat(l5)`, `voxel: 8` four times: elements 6093 / 6105 / 6099 / 6093, tip 0.004227 / 0.004264 / 0.004207 / 0.004291 mm, root stress 3.213 / 3.008 / 3.144 / 3.110 MPa at x 42.7 / 39.8 / 80.9 / 39.8; `voxel: 12` three times: 2712 elements each, tip 0.004091 / 0.004314 / 0.004127, stress 3.387 / 3.221 / 3.411. Across sizes 12..4: tip 0.00409..0.00437 (+-3 %), stress 3.04..3.56 (+-8 %). | Make the shell mesh and the solve reproducible (fixed thread reduction order, seeded mesher); until then say in the result that shell stresses vary by several percent. |
| F11-6 | nit | The holder result says little: no levels or locations, so the agent cannot tell what was left uncut (the lifted waterline rings of levels 15 and 10 end up as the same ring at z 18, 82 mm cut twice, and `cut_mm` still counts 326.65 for the waterline while the raster `cut_mm` drops from 1134.5 to 933.6); `254 collisions ... 252 positions lifted` (two not lifted, unexplained); no minimum stickout is suggested (here `stickout >= 20` reaches the plate); the exported G-code carries no comment that it was lifted for a holder (`G-code z 18..35` against 10..35, no `holder` text). | `cam_export_gcode {job: Fin, holder: [6,10,20,12], path: fin_h.nc}` on the plate + boss. | Return `lifted_levels` or boxes and the stickout that avoids it; drop duplicate rings; put the holder and "lifted" in the G-code header. |
| F11-7 | nit | `holder` docs and checks: the schema says `3D [shank D, flute, holder D, stickout]` and nothing says stickout is measured from the tip to the holder's bottom face (verified: bottom = tip z + stickout, shank spans tip + flute .. tip + stickout); the length error `holder takes [shank D, flute length, holder D, stickout]` does not say how many were given; `holder D < shank D` (`[8,10,6,12]`) and a shank thinner than the tool (`[3,10,6,12]`) are accepted without a note; for `drill` the holder warning comes next to the real one (`1 of 1 holes is not 6 mm across`), fine. | `cam_toolpath {..., holder: [8,10,6,12]}`. | Document the reference points; refuse holder D < shank D. |
| F11-8 | nit | `feature_add` messages: error positions are for the wrapped text (`line 2, column 124` for a one-line statement; the column is right, the line is one too high); an unquoted `group: Finish` says `expected a quoted string` (numbers, names and units are unquoted elsewhere); a `param` statement gives `a feature object must contain exactly one feature` with no hint to `param_set`; a syntactically bad `import` gets a parse error, a valid one the helpful `imports, components and sketches have their own commands` + hint; an unknown reference is refused (`invalid_document`, nothing committed) but a valid-looking `chamfer` of an edge that no longer exists (the same edge chamfered twice) is committed with `committed_with_errors` and later adds report it as `still_failing`; `cam_job` with `depth: -3 mm` or `tool_diameter: 0 mm` is accepted and fails only in `cam_toolpath` (`depth and stepdown must be positive lengths`). | `feature_add` with the statements named. | Offset the line, hint at `param_set`, parse before the "own commands" check, validate job numbers when added. |
| F11-9 | minor | An `extrude` cut that removes nothing is accepted without a word: a pocket sketched on the XY plane (z 0) with `distance: -6 mm` (away from the material) and holes with `-10 mm`: `rebuild_ok: true`, `new_errors: []`, no `body_changes` key (volume 65582.429 before and after), twice in this run. The only sign is the absent `body_changes`. | `feature_add 'extrude Pocket (profile: PK, distance: -6 mm, op: cut, body: E1)'` with `PK` on `XY` under a plate that starts at z 0. | A warning `cut removed no material (the profile lies outside the body in that direction)`. |
| F11-10 | nit | `cam_toolpath {job}` of a suppressed `cam_job` runs without a note; a job without `top` takes the stock top as z 0 even when the part stands at z 0..40 (`pocket` with depth 3: `levels_mm: [-3.0]`, 2787 mm of cutting in air), also silent. | `cam_job Sup (sketch: BOX, operation: pocket, tool_diameter: 6 mm, depth: 3 mm, suppressed: true)`, then `cam_toolpath {job: Sup}`. | Say `suppressed`; default `top` to the body's top or warn when the cut starts above the highest point. |

What worked well: `holder` on waterline and pencil paths equals the hand clearance to 0.001 mm at every
sampled point and every lift amount (3, 5, 6, 8, 12) equals `required - tip`; a long stickout or a thin
holder gives no warning and the unchanged path; 2D operations say the holder is ignored; the export reuses
the right path (the same job with and without holder gives different G-code, z 10..35 and 18..35); `feature_add`
takes `cam_job`, `fillet`, `chamfer` and `extrude` statements with `label`, `group` and `suppressed`,
returns the volume change (fillet +56.345 and chamfer -24.0855 exact), inserts at the rollback point, shows
`committed_with_errors` with an `undo_hint`, and undo/redo return the text byte for byte; every refusal
(duplicate name, sketch, import, malformed, unknown sketch/operation) commits nothing and the messages say
what to use instead; the part d volume (fillet, pocket, hole, boss) equals the hand value to 4e-7; F10-4's stubs
are gone and the D4 pencil on the bracket gives exactly the 144 mm pocket-floor rectangle.

Run 10 open items, re-checked:

| # | Status |
|---|---|
| F10-3 | Still reproduces for the first ring of `pocket_rest` (the other rings were fixed): 30x20 pocket, D8 then D4, stepdown 3, two levels: `cut_mm` 551.54, of which 496.5 mm is 3 deg ramp; every corner loop (8: 4 corners x 2 levels) is a 1.94 mm square (x 36.06..38.0, y 12.0..13.94) cut by 66 mm of spiral; `ramp_deg: 0` gives 55.12 mm, 10 deg 221.5, 20 deg 166.8 (minutes 1.22 / 0.51 / 0.67 / 0.58). Adaptive and plain pockets are fine (adaptive level 1 ramps 57.6 mm once). Suggest: a rest loop smaller than the ramp length plunges (or ramps over the loop's own length) instead of spiralling a whole 3 deg ramp. |
| F10-4 | (a) Fixed on the real part: bracket, D4 depth 18: one rectangle x 14..50, y 12..48 at z 4, 144.0 mm, no stubs. (b) The message is clearer (`the creases lie below the depth`) but its number is wrong (F11-4). |
| F10-6 | Still open, and worse than scatter: shell results are not reproducible run to run (F11-5). Without holes on the closed enclosure the tip is 0.0041..0.0044 mm (+-3 %) and the clamp stress 3.0..3.6 MPa (+-8 %, hand 2.94, location moves along the bend). |

Not tested: holders on `bull` ops (none exist here), `holder` through the GUI, holder with `regions`/`tabs`,
`cam_job` G-code run on a controller, `sim_modal` of the enclosure, mitre/close at 120 and 150 deg.

### Run 11 fix status (2026-11-07)

| # | Status |
|---|---|
| F11-1 | Fixed: next to lifted samples each step is sampled at an eighth of the tool radius and the tool climbs before moving across (or moves across before dropping); a unit test samples every move at 0.03 mm against the requirement. |
| F11-2 | Fixed: the shank and holder get a 0.02 mm contact allowance, so a shank tangent to a wall is not a collision. |
| F11-5 | Fixed: the mid-surface mesher iterated hash maps (random order per map) and sorted equal-length edges without a tie-break; it now uses ordered maps and breaks ties by vertex ids. An OCCT test runs the enclosure five times and gets identical element counts and results. |
| F11-3 | Fixed: `cam_job` takes `holder: [...]` (4 lengths); values next to `job` override the job's instead of being refused or ignored (sketch, operation, tool and depth stay the job's). |
| F11-9 | Fixed: `feature_add` of an extrude or revolve that adds to or cuts a body warns when it changed nothing, as `feature_extrude` does. |
| F11-10 | Partly fixed: a suppressed `cam_job` is refused with a hint; the default `top` is unchanged. |
