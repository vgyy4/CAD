# Code review, 2026-11-07 (feature-tree import, tilting, mitres, shells, CAM 3D/rest)

Read-only review of the day's commits by a subagent; fixes by the main session.

| # | Finding | Status |
|---|---|---|
| R1 | Shell skin flood stopped at bends facetted coarser than 10 degrees | Fixed: facets join up to 37 degrees (strips and hole walls are at 90). |
| R2 | 3D finishing bounds took a whole circle for an arc and skipped other curves | Fixed: arcs by their sweep; other curve types refused. The raster still covers the regions' box (documented). |
| R3 | Shell face loads shared equally between nodes of very different areas | Fixed: shared by each node's tributary area. |
| R4 | No cap on the skin before refinement | Fixed: refused past `MAX_SKIN_TRIANGLES`. |
| R5 | Vertex weld could split points across a rounding cell border | Fixed: neighbouring cells searched within 1e-4 mm. |
| R6 | Rest machining: a tool too large for the region reads as "nothing left" | Fixed: the message names both causes. |
| R7 | Mitre angle off for bend radii below the half gap | Fixed: the wedge starts where the diagonal meets the flat (a triangle); OCCT test at r = 0.05. |
| R8 | `move_face onto` runs a full trial evaluation before committing | Accepted (cost of refusing failures up front). |
| R9 | Tree import rebuilds the document from its text | Accepted: the text form is the document's canonical form (round-trip tests in gusset-doc). |
| R10 | A mitre without `close` printed nothing | Fixed: printed as `close: mitre`. |
