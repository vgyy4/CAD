# Interoperability status

Per-format status (spec: "Interoperability & import"). Tier: 1 = editable features,
2 = exact B-rep, 3 = mesh. The success-rate columns come from the test corpus
(`tests/corpus/`, licensed files only) and are measured, never estimated.

| Format | Direction | Tier | Status | Parse % | Valid solid % | Corpus size |
|---|---|---|---|---|---|---|
| STEP AP203/214/242 | import/export | 2 | planned (MVP, OCCT in worker) | – | – | 0 |
| IGES | import | 2 | planned (MVP) | – | – | 0 |
| STL | import/export | 3 | in progress (`gusset-meshio`) | – | n/a | 0 |
| 3MF | import/export | 3 | in progress | – | n/a | 0 |
| OBJ | import/export | 3 | in progress | – | n/a | 0 |
| Fusion 360 .f3d | import | 2 | done (ADR 0039: the archive's binary ACIS bodies; component placements not read yet) | – | – | 0 |
| ACIS SAT | import | 2 | done (ADR 0039, via STEP; versions 1 to 7; analytic and B-spline geometry, procedural surfaces by their stored approximation) | – | – | 0 |
| Parasolid XT | import | 2 | done (ADR 0035, via STEP; outside the corpus, 4 of 5 NIST samples read, V3 refused) | – | – | 0 |
| SolidWorks .sldprt | import | 2 | planned | – | – | 0 |
| Fusion .f3d | import | 2 | planned | – | – | 0 |
| FreeCAD .FCStd | import | 1/2 | planned (v1) | – | – | 0 |
| OpenSCAD .scad | import | 1 | planned (v1) | – | – | 0 |

## Feature-tree exporter plugins
| Plugin | Status |
|---|---|
| `plugins/fusion` | written (Python add-in, sample file in `plugins/fusion/sample`); **UNVERIFIED** until run against real Fusion |
| `plugins/solidworks` | written (VBA macro, SolidWorks 2024 API, manual test + sample); **UNVERIFIED** until run against real SolidWorks |
| `plugins/onshape` | written (Python, REST API v6; sketch/extrude/revolve/fillet/chamfer); offline-tested against hand-written responses only; **UNVERIFIED** until run against Onshape (needs API keys) |

The importer (`import_file` on a `.gtree.json`, ADR 0042) is tested with hand-written trees
and with each plugin's `sample/expected.gtree.json`: sketches on any plane parallel to XY/XZ/YZ
(offset planes become datum planes), extrude/revolve with profile picks, fillet/chamfer edges
by midpoint, parameters kept as expressions where they parse. Everything else is listed in
`skipped`, and source body volumes are compared with the imported model's.

## Reverse-engineering log
Interoperability-only work from public documentation and our own sample files.
Entries record the source documents and samples used.
