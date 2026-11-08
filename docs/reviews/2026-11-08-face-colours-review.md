# Review: face colours (2026-11-08)

Scope: commits 6f25353..a01e016 (ADR 0045 amendment: face colours from STEP, Parasolid and
SolidWorks; `body.appearance` with `faces`; 3MF export). Read-only review by a subagent,
fixes by the main session. The face-index chain (OCCT `TopExp::MapShapes` order in the reader,
writer, tessellation and naming) was found consistent.

| id | severity | finding | status |
|---|---|---|---|
| 1 | high | A body without its own colour took the colour most of its *coloured* faces had, so three red faces on a 100-face body turned the body red. | Fixed: uncoloured faces count; when they win, the body takes the default colour and the coloured faces are listed (OCCT test). |
| 2 | high | The app's list fields split on every comma; face names such as `blend(edge(A, B))` fell apart. | Fixed for every list field: commas inside brackets and quotes stay (test). |
| 3 | medium | A face in two groups showed one colour in the viewport and exported another. | Fixed: validation refuses a face in two groups and a colour listed twice; one rule (first colour) everywhere through `face_color_indices`. |
| 4 | medium | The 10 000 limit was per appearance; imports added no document-wide limit. | Fixed: validation and import keep the document's total within 10 000. |
| 5 | low-medium | `body.appearance` with many faces compared lists pairwise. | Fixed: sets. |
| 6 | low | 3MF export looked colours up linearly. | Fixed: a map. |
| 7 | low | Import grouping was linear per colour; repeated face indices were kept. | Fixed: a map; the first colour of a repeated index. |
| 8 | low | Merges treat an appearance as one unit (finish on one side, face colours on the other conflict); face colours of deleted features stay in the text (unused). | Merge fixed: colour and finish merge apart from face colours, so the two combine. Deleting a feature drops the colours of the faces it made. |
| 9 | low | Triangles outside every face range lost their colour once a face was coloured. | Fixed: they keep the base colour. |
