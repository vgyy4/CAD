# ADR 0004: Document model and the code view

- Status: accepted
- Date: 2026-10-10

## Context
The spec wants a single source of truth that serves both the GUI feature tree and
editable code (OpenSCAD-style), round-tripping in both directions. It also wants a
deterministic, readable text form for tests, golden files, agents and "Export as text".

## Decision
1. **The document is a declarative, ordered list of features plus a parameter table.**
   ```
   Document {
     format_version, units (mm|in), params: [Param],
     features: [Feature { id, name, suppressed, group, kind: FeatureKind }],
   }
   ```
   `FeatureKind` is a closed enum: `Sketch`, `Extrude`, `Revolve`, `Fillet`, `Chamfer`,
   `Import`, ... Each variant holds only *inputs*: expressions, persistent references
   (ADR 0003) and blob hashes. It never holds derived geometry.
2. **The code view *is* the canonical text serialization ("Gusset script", `.gus.txt`).**
   There is exactly one printer and one parser. `print(parse(text)) == text` holds for
   canonical text, and `parse(print(doc)) == doc` holds for every document; both are
   property-tested. Each feature is one statement:
   ```text
   gusset 1
   units mm

   param width = 40 mm
   param depth = width / 2  # user comment (untrusted text)

   sketch Sketch1 on XY {
     p1 = point(0 mm, 0 mm)
     p2 = point(width, 0 mm)
     l1 = line(p1, p2)
     horizontal(l1)
     distance(p1, p2) = width
   }
   extrude Extrude1 (Sketch1, distance: depth, op: new)
   fillet Fillet1 (edges: [edge(Extrude1/side(Sketch1/l1), Extrude1/cap(end))], radius: 2 mm)
   ```
3. **Round-tripping is guaranteed because the language is declarative.** It has no loops,
   user-defined functions or conditionals in the MVP. Patterns, mirrors, etc. are
   features, so the tree and the text are always isomorphic. Editing the text and
   applying it is an ordinary document edit: a transaction in the history (ADR 0006).
   Later, OpenSCAD-style generative code becomes a `Script` feature: one opaque node in
   the tree whose body runs in a sandbox (no I/O, no clock, op/memory/recursion limits).
   It produces geometry, and its parameters are still visible.
   **v0.2 statements** (all arguments after the name are `key: value`, `body:` optional =
   latest body, stored explicitly by the commands):
   ```text
   shell Shell1 (body: Extrude1, faces: [Extrude1/cap(end)], thickness: 2 mm)
   mirror Mirror1 (body: Extrude1, plane: YZ)                 # new body "Mirror1"
   mirror Mirror2 (plane: Extrude1/side(Sketch1/l6), merge: true)  # united with the source
   pattern_linear Pattern1 (body: Extrude1, direction: X, count: n_bolts, spacing: 15 mm)
   pattern_circular Pattern2 (axis: Z, through: [0 mm, 0 mm, 0 mm], count: 6, angle: 360 deg)
   ```
   `thickness > 0` builds walls inward, `< 0` outward. `plane` is XY/XZ/YZ or a planar
   face reference. A direction/axis is `X`, `Y`, `Z` or `[x, y, z]` (unitless
   expressions). `count` includes the original and must evaluate to a whole number in
   2..=500. For `pattern_circular`, a full turn spaces instances `angle / count` apart,
   a smaller angle `angle / (count - 1)`; `through` (a point on the axis) is optional.
   `merge: false` mirrors create a body named after the feature, so later features can
   target them. Patterns repeat a whole body (not a single feature's result); use
   `combine` to merge a patterned feature-body into the part:
   ```text
   plane Plane1 (base: Extrude1/cap(end), offset: 2 mm)            # datum plane, no body
   plane Plane2 (base: Plane1, axis: X, angle: 30 deg)             # turned about X or an edge
   move Move1 (body: Extrude2, translate: [10 mm, 0 mm, 0 mm], axis: Z, angle: 45 deg, copy: true)
   combine Combine1 (body: Extrude1, tools: [Extrude2], op: cut)   # tools are consumed
   ```
   A `plane` has either `offset` (along the base plane's normal) or `axis` + `angle`; its
   base is XY/XZ/YZ, a planar face or another datum plane. A sketch's or mirror's
   `plane:` may be a datum plane's name (resolved by name, not by `/`, so a feature must
   not be called XY, XZ or YZ). `move` turns first (about `axis` through `through`,
   default the origin), then translates along the world axes; `copy: true` creates a body
   named after the feature. `combine` takes `tools` (the creating features' names) and
   `op: add|cut|intersect`; later features cannot target a consumed tool body (validation
   reports it, unless the combine is suppressed).
   **Sketch items (v0.2, ADR 0016):** besides points, lines, circles, arcs and constraints a
   sketch has splines and patterns:
   ```text
   s5 = spline(p1, p2, p3, p4) closed
   g9 = pattern_linear(of: [l5, c7], count: n, dx: 10 mm, dy: 0 mm)
   g10 = pattern_circular(of: [l5], center: p3, count: 6, angle: 360 deg)
   g11 = pattern_linear(of: [c7], count: 4, dx: 10 mm, dy: 0 mm, count2: rows, dx2: 0 mm, dy2: 10 mm)
   ```
   `Sketch1.regions(l5_r1, c7_2)` names regions by id (entity ids, `_rN` ranks, pattern
   copies `l5_2`). Patterns hold expressions, so they are parameter-driven declarative data
   and keep the text isomorphic to the tree.
4. **Sandboxing.** The language can only express data and pure unit-aware arithmetic
   (`gusset-units`). Parsing enforces limits on input size, nesting depth, statement
   count and name length. It is deterministic: `libm` for transcendental functions,
   and no hash-map iteration order anywhere in printing.
5. **Edits go through commands.** GUI, MCP and code view all mutate the document only
   through `Command`s from one registry (ADR 0012). A command validates, produces a new
   immutable `Document` value (structural sharing via `Arc`), and is recorded as one
   history transaction.
6. **Evaluation is separate from the document.** `gusset-eval` turns a `Document` into
   bodies through the `Kernel` trait. It caches per feature, keyed by the hash of
   (feature definition + resolved inputs), so only downstream features recompute after an
   edit. A failing feature yields a structured `FeatureError` with message, hint and
   affected references. Downstream features are marked "blocked by" it, and the
   history is unaffected.

## Alternatives considered
- *Full programming language in the code view* (OpenSCAD/CadQuery style): it cannot
  round-trip to a GUI tree in general, because loops and conditionals have no unique
  tree form. That is why it is a sandboxed `Script` feature later instead.
- *Embedding Rhai/Lua as the document language*: same round-trip problem, larger
  attack surface, non-deterministic float formatting.
- *Separate binary and text formats*: two serializers drift apart. The text form is the
  canonical encoding of each history object (ADR 0005/0006).

## Consequences
- The text format is a public, versioned API (`docs/FORMAT.md`). Breaking changes need
  migrations.
- The parser is security-critical and is fuzzed.

## Amendment (2026-11-09): unequal chamfers

`chamfer` keeps `edges` and `distance` and gains three optional arguments:
`distance2: <len>` or `angle: <deg>` (not both), and `faces: [<face>, ...]`. Without
`distance2`/`angle` the chamfer is equal and prints exactly as before, so existing files and
round-trips do not change. `FeatureKind::Chamfer(EdgeBlendDef)` carries `second:
ChamferSecond` (`Equal`, `Distance(Expr)`, `Angle(Expr)`) and `faces: Vec<FaceRef>`; fillets
always have `Equal` and no faces.

- **Meaning.** `distance` runs along the edge's *reference face*: the chamfer face meets that
  face `distance` from the edge. `distance2` is the same measure on the other face. With
  `angle`, the chamfer face leaves the reference face at that angle, so the other leg is
  `distance * tan(angle)`; the angle is in (0, 90) degrees exclusive (45 degrees is an equal
  chamfer). OCCT calls: `BRepFilletAPI_MakeChamfer::Add(d1, d2, edge, face)` and
  `AddDA(distance, angle, edge, face)` (`Kernel::chamfer_ref` with a `ChamferSpec`); golden
  tests check the direction by face areas, not only by volume.
- **Reference face rule.** `faces[i]` is the reference face of `edges[i]`; the list is empty
  or has one face per edge, and each must be one of the edge's two faces (else the
  evaluation fails with `invalid_reference`, "does not border"). When a chamfer has no
  `faces`, the reference face is the adjacent face whose persistent name is smallest in the
  `FaceRef` order (feature name, then `side` before `cap` before `blend` and the rest of
  `FaceKey`'s declaration order, then entity and index). Names, not kernel face indices,
  decide, so the choice is the same after regeneration; it does not depend on OCCT's
  numbering. `faces` without `distance2`/`angle` is a validation error (it would not be
  printed back).
- **Validation.** `distance`, `distance2` > 0 (length), `angle` in (0, 90) degrees; checked
  in `gusset-eval` before the kernel is called, and by `feature.chamfer` before it commits.
- **Naming.** Unchanged (ADR 0003): the chamfer face is `<feature>/blend(<edge name>)`; a
  reference face named in `faces` is an ordinary face reference, so it is renamed with the
  feature it points to, and counts as a dependency of the chamfer.
- **Command.** `feature.chamfer` gains `distance2`, `angle` and `faces` (strings; `faces` is
  a list). The MCP tool entry stays under the byte budget (about 750 bytes). The GUI form
  has a mode switch (equal / two distances / distance and angle) that shows only the fields
  of the chosen mode; `feature.edit` reaches the same arguments through the code view's names.
