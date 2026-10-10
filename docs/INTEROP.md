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
| Parasolid XT | import | 2 | planned (v1) | – | – | 0 |
| SolidWorks .sldprt | import | 2 | planned | – | – | 0 |
| Fusion .f3d | import | 2 | planned | – | – | 0 |
| FreeCAD .FCStd | import | 1/2 | planned (v1) | – | – | 0 |
| OpenSCAD .scad | import | 1 | planned (v1) | – | – | 0 |

## Feature-tree exporter plugins
| Plugin | Status |
|---|---|
| `plugins/fusion` | not started; will be marked **UNVERIFIED** until run against real Fusion |
| `plugins/solidworks` | not started; **UNVERIFIED** until run against real SolidWorks |
| `plugins/onshape` | not started; testable via REST (needs API keys from the owner) |

## Reverse-engineering log
Interoperability-only work from public documentation and our own sample files.
Entries record the source documents and samples used.
