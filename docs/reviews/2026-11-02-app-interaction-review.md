# Review: app interaction work (c16df78..adf0f0a)

Scope: section view (gusset-render), planes / sketch placement / picking / dragging / constraint
toolbar / region picking / live picks / feature edit form / live preview (gusset-app),
`query.measure`, `feature.edit`, preview mode (gusset-commands), `statement_args` (gusset-doc).

Summary: the section-plane maths is consistent between CPU picking, the software renderer and
the shader (sign of the kept side, ray/plane interval, view-space plane all check out), and
preview mode cannot commit or audit anything (the commit is intercepted before the history is
touched; `feature.edit` rejects newlines, non-identifier keys, name/kind changes and extra
statements, since the parser demands exactly one feature). The real defects are in app state
handling: live preview dies after a project switch, a failed click-placed command leaves the form
unusable, region picking chooses the wrong region for thin rings, and a drag can fail to finish.
`feature.edit` also bypasses the command-specific validation of import/component features.

## Findings

### R1 (medium) Live preview stops for good, and old model flashes, after a project switch
crates/gusset-app/src/app.rs:1170-1193 (project reset), 600 and 655 (`preview_requested`),
crates/gusset-app/src/viewport.rs:421-440.
The reset replaces `done_tx/done_rx` and sets `in_flight = 0`, but does not touch
`preview_requested`, `preview_shown`, or the viewport stash. A preview job in flight when the user
opens another project reports on the dropped channel, so `preview_requested` stays `Some` and
`request_preview` returns early forever: no live preview again in that window. Also, if a preview
was showing, `set_scene(empty)` leaves the old project's scene in `stash`; the next frame without a
form calls `end_preview`, which restores the OLD project's scene and bodies until the new
`Done::Scene` arrives (pickable, hover-able).
Fix: in the reset, clear `preview_requested/shown/since` and drop the stash (add a
`Viewport::clear_preview` that discards instead of restoring).

### R2 (medium) A failed click-placed command leaves the form unable to complete
crates/gusset-app/src/app.rs:2015-2045, forms.rs (`clicks`).
`form.clicks` is only cleared when the form closes. If the submit fails (radius 0 from two clicks
on one spot, an unknown sketch, a solver error), the form stays open with its clicks. The next click
pushes a third click; `fill` matches exactly two (line/rect/circle) or three (arc) clicks, returns
false, and no further click can ever submit. The user must cancel and reopen the tool. The same
path (`chain` set, then `apply_form` fails in `to_args`) leaves `self.chain` set, so a later
hand-typed OK of the same command unexpectedly restarts placing.
Fix: on an error result for a placed command (and when `apply_form` fails) clear `form.clicks` and
`self.chain`; make `fill` use the first `n` clicks.

### R3 (medium) Region click picks the annulus instead of the hole region for thin rings
crates/gusset-app/src/sketch_regions.rs `region_at`; `Region.area` is net of holes
(crates/gusset-sketch/src/profile.rs:102, "outer minus holes").
`region_at` tests only each region's outer loop and takes the smallest `area`. For a ring with a
hole larger than half of the outer area (outer R20, hole r15: net 549 mm2 vs hole disc 707 mm2) a
click inside the hole matches both and the ring wins, so the hole's own region cannot be picked.
Fix: compare gross area (outer loop area), or exclude regions whose holes contain the point.

### R4 (medium) feature.edit bypasses the validation of import and component features
crates/gusset-commands/src/commands/feature_edit.rs:35-66.
Name/kind are protected, but every other argument of any one-line feature can be set, including
`blob`/`format` of an import and `source`/`document`/`version`/`branch` of a component. Those are
meant to be set only by `import.file` / `assembly.insert_component` / `update_component`, which
pin real data, run `read_target` path confinement and check parameters/configurations. An agent can
point a component at any stored document hash or an import at any stored blob, with no check and no
approval beyond the generic EDIT flag. (Paths in `source` are re-validated when read, so this is not
a file read, but it breaks the pinning invariants.)
Fix: refuse `Import` and `Component` kinds (point to the dedicated commands), or whitelist keys.

### R5 (low) Sketch drag may never report `done`; ghost preview stays
crates/gusset-app/src/viewport.rs `drag_point`; app.rs `sketch_drag`.
`done` is only delivered inside `if let Some(p) = interact_pointer_pos().or(hover_pos())`. If the
pointer has left the window on release both are `None`: the viewport clears `dragging`, but the app
never receives `done`, so `drag_preview` keeps the ghost line and no `sketch.move_point` is sent.
Fix: remember the last ray in `Viewport` and emit `done` (or a cancel) even without a position.

### R6 (low) Line ending on an existing point is not connected
crates/gusset-app/src/sketch_place.rs `fill`; commands.rs:1185-1196 (`SketchAddLine` has only
`from_point`). The end click snaps its coordinates but its point id is dropped, so closing a
triangle creates a duplicate coincident point with no constraint. Profiles still close (tolerance
join), but dragging one of the pair tears the outline apart. Fix: add `to_point` to `sketch.add_line`
and fill it from `b.point`.

### R7 (low) Section cut face shows far-wall edges and treats open meshes as cut
crates/gusset-render/src/shader.wgsl fs_mesh / software.rs raster_triangle; pick.rs `pick_section`.
The "cap" is the back face of the far wall, so its depth is the far wall's: edges of that wall,
pulled toward the eye, draw over the cut face; `pick_edge_section` accepts them too. Every back face
is also treated as cut face, so open shells / reference meshes (single-sided surfaces) are tinted and
unpickable under a section. Fix: skip edges whose view point is behind a back-facing surface only
when sectioned (or draw the cap as a real plane); exempt non-closed meshes.

### R8 (low) query.measure: first match wins and angle is folded
crates/gusset-commands/src/commands/measure.rs `find`, `run`.
Without a `Body:` prefix the first body containing the name is used silently, so with several
bodies sharing face names the answer may be for the wrong body (report ambiguity). The angle uses
`|dot|`, so two planes at 120 deg report 60 deg with no note; an edge-to-plane "from center to plane"
uses the edge midpoint even for oblique edges. Fix: error on ambiguous names; document or return
the supplementary angle.

### R9 (low) Live picks overwrite fields of an open feature.edit form
crates/gusset-app/src/app.rs `fill_picks` (live) with `edit_feature`.
Edit-form fields are named after the statement's arguments, so any viewport click while editing a
hole/fillet/shell overwrites `plane`/`edges`/`faces` with plain picked names (possibly in a different
syntax than the stored `face(...)`/`edge(...)` form), even if the user was only inspecting.
Fix: skip live fill for `feature.edit` unless the field is focused, or format picks canonically.

### R10 (low) Preview jobs are unsuperseded and count as in-flight
crates/gusset-app/src/app.rs:617. `submit` increments `in_flight` and a preview rebuild of a heavy
model blocks the model thread (and any queued real command) while it runs; `busy()` is true during
it. Fix: use `supersede_rebuild` for previews or cancel on the next real command.

### R11 (low) statement_args mis-splits strings with escaped quotes
crates/gusset-doc/src/text.rs `split_top_level`. `"` toggles `quoted` even when preceded by a
backslash, so `text: "say \"a, b\""` splits at the inner comma; `split_once(':')` then fails or
produces garbage fields (feature.edit reports "not a one-line feature", the edit form shows broken
fields). Rejoining with ", " is lossless, so nothing is corrupted. Fix: skip escaped quotes.

## Checked, no defect found
- Section sign convention: `view_plane` negates the removed-side normal; `keeps`, `kept_interval`,
  shader `removed()` and software `plane_dist` agree; cap normal is `-clip.xyz` in both renderers.
- Preview mode: not reachable from MCP (`preview` is called only from the app); commit returns
  before history/audit; state is reset by `end_preview` on every path.
- `feature.edit`: single-feature parse blocks statement injection; `#` in a value only drops later
  arguments of the same feature (self-inflicted).
- Point-in-polygon, ray/plane (`on_plane`, `DrawPlane::hit`) and `SectionView::plane` are correct.
- Per-frame work is O(bodies) (bounds, plane clones), not O(triangles).


## Status (2026-11-03)
Fixed: R1 (a new scene drops any preview; the project reset clears the preview state), R2
(clicked points start over after a failed placed command), R3 (regions are chosen by the
area their outer loop encloses), R4 (`feature.edit` refuses imports and components), R5 (a
drag released off the viewport finishes at its last position), R6 (`sketch.add_line`
`to_point`; a snapped end joins the point), R8 (an ambiguous name in `query.measure` is an
error naming the bodies; the angle is documented as 0 to 90 deg), R9 (picks leave edit
forms alone), R10 (no preview while a command runs), R11 (escaped quotes in statements).
R7 fixed as well (2026-11-03): meshes know whether they are closed (watertight after
welding), and only a closed body's back faces seen through a section are its cut face. A
separate cap pass (`fs_cap`, and the software rasterizer's cap branch) draws them at the cut
plane's depth instead of the far wall's, so what lies behind the cut stays hidden. Picking
stops edges behind a cut face at the plane. Open meshes keep two-sided drawing and picking.
Tests: a red box behind the cut is hidden (GPU and CPU), and far edges are not picked.
