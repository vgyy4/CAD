# ADR 0003: Topological naming and persistent references

- Status: accepted
- Date: 2026-10-10

## Context
Parametric CAD breaks when a later feature refers to "face #7" and an upstream edit
renumbers the faces. Before FreeCAD 1.0 this "topological naming problem" was its most
repeated complaint (see `docs/RESEARCH.md`). The spec makes persistent naming a core
requirement that has to survive branching, reordering and cherry-picking.

## Decision
We use **generative (history-based) naming with geometric disambiguation**. It is the
approach of Onshape's "queries", the FreeCAD 1.0 element maps and the academic
literature (Kripac 1997; Capoyleas, Chen & Hoffmann 1996). We never use indices.

1. **Every topological entity gets a name derived from how it was created**, never from
   its index in the B-rep:
   - Faces of a sketch-based feature: `<feature>/side(<sketch entity id>)` for lateral
     faces, `<feature>/cap(start|end)` for caps. When the sketch has several regions,
     the caps of every region but the first get the region's identifying entity
     (`cap(end, l12)`); regions are ordered by identifying entity, so the first is the
     oldest loop and adding a loop later never renames existing caps (amended
     2026-10-11, review L4).
   - Faces of a fillet/chamfer: `<feature>/blend(<edge name>)`.
   - Primitives: `<feature>/face(top|bottom|side...)`.
   - Faces carried through a boolean or modification keep their original name. Faces
     the operation splits get `#k` suffixes, ordered by a *geometric* key (see 3).
2. **Edges and vertices are named by their adjacent faces.** An edge is
   `edge{faceA, faceB}`, with the face names sorted, plus a disambiguation suffix when
   two faces share several edges. A vertex is named by its sorted set of incident
   faces. These names are stable whenever the faces are.
3. **Disambiguation is geometric and deterministic.** When a name matches several
   entities (a split face, or two edges between the same faces), candidates are ordered
   by a key computed from rounded geometry: centroid projected on the sketch/feature
   frame, then area/length. That order is stable under small parameter changes.
4. **References are stored as names, never as indices.** A `Ref` is serialized as text,
   e.g. `Extrude1/side(Sketch1/l3) & Extrude1/cap(end)`. It is human-readable in the
   code view and in MCP output.
5. **Resolution has three outcomes:** `Resolved(entity)`, `Ambiguous(candidates)` or
   `Missing(reason)`. A missing or ambiguous reference makes the *feature* fail with an
   actionable error ("Fillet1: edge `Extrude1/side(Sketch1/l3) & …` no longer exists
   because line l3 was deleted from Sketch1; pick a new edge"). It never silently picks
   a different entity. One exception: a fuzzy fallback (the nearest candidate by
   geometric key, within a tolerance) is offered as a *suggested fix*, never applied
   automatically.
6. **Kernel requirement:** the `Kernel` trait must report operation history (for every
   output face: the inputs that *generated* or *modified* it, and deleted inputs). OCCT
   provides this through `BRepBuilderAPI_MakeShape::Generated/Modified/IsDeleted`. The
   naming layer lives in our Rust code (`gusset-naming`), above the kernel adapter, so
   it is kernel-independent and fully unit-testable with a mock kernel.
7. **Sketch entity ids are stable.** Sketch entities carry ids (`l3`, `a2`) that are
   never reused within a sketch, even after deletion. That is what makes `side(...)`
   names stable across sketch edits.

## Golden tests (required)
Tests in `gusset-naming` and at document level check that references survive:
changing parameters (dimensions), adding or deleting unrelated sketch entities,
reordering independent features, branching then editing, and cherry-picking a feature
onto another branch.

## Consequences
- Every feature implementation must name its outputs. That is a small, mechanical cost.
- Names are longer than indices. They are compressed on disk and readable everywhere else.
- Some edits legitimately invalidate references (deleting the generating sketch line).
  Those become clear errors with suggested fixes, which is the honest behavior.
