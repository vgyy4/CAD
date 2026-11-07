# Security review: feature-tree import v2 (2026-11-07)

Read-only review of `crates/gusset-commands/src/tree_import/v2.rs` (holes, patterns, mirror,
shell). No text injection, no panics and no untrusted text outside `_user_text` fields; the v1
limits hold. Findings and status:

| # | Finding | Status |
|---|---|---|
| S1 | Two mirrors named alike shared copy names; the second reused the first's sketch, reflected across the wrong plane | Fixed: a unique tag per mirror and a per-mirror record of the sketches it copied (test). |
| S2 | Mirrors re-emit sketches past the file's entity cap | Fixed: generated sketch entities are counted against `MAX_TOTAL_ENTITIES`. |
| S3 | Face picking scanned every edge per cylindrical face (quadratic) | Fixed: radii gathered in one pass per body. |
| S4 | Pattern directions printed with 6 decimals moved far copies | Fixed: 12 decimals for directions and axes. |
| S5 | Evaluations are not cumulative; many near-budget rebuilds | Mitigated: an import-wide time limit (300 s) on top of the 400-build cap. |
| S6 | Lengths up to 1e12 mm reach OCCT | Fixed: v2 lengths below 1e7 mm, and spacing x count bounded. |
| S7 | Faces matched by infinite plane/cylinder; mirrored features keep their `target` | Open (documented): a point-in-face test and target remapping are follow-ups. |
