# ADR 0035: Parasolid XT import (clean-room reader to STEP)

Status: accepted (2026-11-03). Research: docs/research/parasolid-xt.md.

## Context
Parasolid XT (`.x_t` text, `.x_b` neutral binary) is the native exchange format of
SolidWorks, NX, Solid Edge and Onshape. The public *Parasolid XT Format Reference*
(schema 12006, Parasolid V12) describes the file layout for translator writers. No Rust
reader with a clear licence exists.

## Decision
A new pure-Rust crate `gusset-xt` reads XT and writes STEP AP214 text; `import.file`
passes that text to the sandboxed STEP importer (no new OCCT code, no new unsafe).

* **Clean room.** Written from the public Reference in our own words. No schema file
  (`sch_*`), SDK, DLL, or third-party reader is used, read or vendored; the Reference PDF is
  not committed. Layouts of schemas older than V12 were worked out from public-domain NIST
  sample files (usnistgov/engineering-design-models), which are not committed either.
* **Schema-driven decoding.** Node layouts (field order and type codes) are data: a table for
  schema 12006 plus per-family overrides. A file's schema number picks the family:
  * V4 (4000-8999), inferred: no regions (shells hang off the body and hold the geometry
    chains), fins without sense (the edge's own fin is the positive one), curves with an
    extra double after their sense, shorter BODY/FACE/EDGE/VERTEX/LIST/CHART/ATTRIB_DEF.
  * V9 (9000-11999): as V12, but fins have no attribute field.
  * V12 (12000 on). Files with an embedded schema (three-part key, V14 on) apply each node
    type's edit script (copy/delete/insert/append) to the base layouts; the V12 table stands
    in for the V13 base, which the Reference does not list.
  * Older than V4: refused with a clear message (their node types are undocumented).
* **Encodings.** Text (lines joined, trailing spaces dropped, characters glued, `?` unset,
  string escapes) and neutral binary (big-endian, one- or two-word node indices). Bare
  binary is refused.
* **Topology.** Solid regions' shells (V4: the body's shells); faces oriented by
  face sense XOR surface sense; loops of fins with their senses; edge start/end from the
  positive fin's forward vertex and the other fin's; ring edges get one vertex on the
  curve; isolated loops become vertex loops. Solids become MANIFOLD_SOLID_BREP (or
  BREP_WITH_VOIDS, the largest shell outside), sheets SHELL_BASED_SURFACE_MODEL.
* **Geometry.** Exact: plane, cylinder, cone (axis reversed; the natural normal flips),
  sphere, doughnut torus, B-surface, swept, spun and offset surfaces; line, circle,
  ellipse, B-curve, trimmed curve. Fitted: intersection curves (a C1 cubic through the chart
  points). Skipped with a warning: blends, SP-curves, foreign geometry, self-intersecting
  tori (the importer heals what it can). Metres are written as millimetres.
* **Untrusted input.** Node and element counts are capped, element counts are bounded by the
  bytes left, chains stop at repeats, geometry references cannot recurse forever; a fuzz
  target (`xt_read`) and a mutation test cover the decoder.

## Consequences
* NIST samples (schemas 4035, 4039, 9008) convert; an OCCT golden test checks that the
  converted solids have NIST's face count and every vertex of NIST's own STEP export.
* Unverified without sample files: V12 attribute definitions, B-surface pole order
  (u-major assumed), embedded-delta details, neutral binary. Wrong guesses there fail
  loudly (an unknown node type or a misread stream), they do not import wrong geometry
  silently, except the pole order.
* Later: blends and SP-curves by sampling and fitting, colours and names from attributes,
  assembly placements (assemblies are flattened today).
