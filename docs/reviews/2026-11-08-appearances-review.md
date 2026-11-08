# Code review: appearances, import colours, offset rules (2026-11-08)

Scope: `git diff 8dd5109..` at the time of the review (appearances ADR 0045, path-tracer
finishes and glass, viewport finish shading, the scene pick cache, STEP/3MF/glTF colours,
SolidWorks LocalBodies, `offset_face` `also_moved` and `rules: false`, the bake face map).
Read, not run. Findings and what was done:

| ID | Sev | Finding | Status |
|---|---|---|---|
| R1 | high | The `rules: false` and `move_face` pre-commit checks evaluated without the history's blobs, so models with imported or baked bodies were refused with a wrong message. | Fixed: `Session::evaluate_doc`; only the tangent case gets the tangent message. |
| R2 | medium | An appearance whose body was deleted could not be cleared, and stayed for a later feature of the same name. | Fixed: clear works on such entries; `feature.delete` drops its body's appearance. |
| R3 | medium | Renaming onto a name holding a stale appearance made two entries and a confusing refusal. | Fixed: the stale entry is dropped first. |
| R4 | medium | "Appearance..." on a sketch, fillet or hole asked for a body that does not exist. | Fixed: the app maps the feature to its body (or the body it changes) and says so when there is none. |
| R5 | medium | Glass at the last bounce ended the path black (MCP photos use 2 bounces). | Fixed: up to 12 passes through glass do not use up bounces. |
| R6 | medium | Glass on an open mesh bent light and never straightened it; Schlick used the incident angle when leaving. | Fixed: open glass is a thin pane; Fresnel uses the transmitted angle on exit. |
| R7 | medium | Cherry-picking a feature lost its appearance. | Fixed: the source's appearance comes along. Rename-plus-recolour across a merge is open. |
| R8 | medium/low | Any failure of the planar offset fell back to BRepOffset. | Fixed: only the "would have to change shape" case does. |
| R9 | low | The in-process STEP import gave different bodies from the worker. | Fixed: it uses the same XDE reader and naming. |
| R10 | low | A hostile name containing U+001F lost its colour; an all-invisible name gave an empty label. | Fixed: control characters go before the colour is appended; empty names fall back to "Body". |
| R11 | low | `also_moved` can list stretched blends; the bake's face map is capped at 200 silently. | Open (informational). |
| R12 | low | The plastic coat reflects only the sky and drops its share when blocked; the disc radiance convention. | Accepted (a 4 % coat; noted). |
| R13 | low | Shadow tint per glass surface differs from the camera path's tint per pass. | Fixed: a fourth root per surface, so a pass through a solid matches. |
| R14 | nit | Duplicated STEP colour naming in CLI and commands; dropped partition error with LocalBodies; whole-document clones to read appearances. | Fixed in part: one `step_names` helper; the rest open. |

The review found the text round trip, merge/diff, shader/Rust shading agreement, `color.w`
packing, the pick cache and the 3MF output correct.
