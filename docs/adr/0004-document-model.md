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

## Amendment (2026-11-09): ellipses in the text language
Sketch entities `o5 = ellipse(p1, angle, a, b)` (id prefix `o`) and `q6 = ellipse_arc(p1,
angle, a, b, p2, p3)` (prefix `q`, counter-clockwise from `p2` to `p3`, both points kept on
the ellipse). `angle` is the direction of the `a` semi-axis in radians from the sketch's u
axis, `a` and `b` are mm; like circle radii they are solver state, printed exactly
and written back after solving. New constraint keywords `semi_major(o5, 5 mm)` and
`semi_minor(o5, 3 mm)`; `horizontal`, `vertical`, `on_circle` and `fixed` accept ellipses.
Validation: finite angle, positive `a`/`b`, an arc with three different points, references
to earlier points. Ellipses are not pattern sources and cannot be trimmed. Persistent names
need nothing new: entity tags are generic over the id prefix, so the lateral faces of an
extruded or revolved ellipse are named `Sketch1/o5` like those of circles (ADR 0003).
Commands `sketch.add_ellipse` (cx, cy, a, b, angle in degrees) and `sketch.add_ellipse_arc`
(plus `from`/`to`, parametric angles in degrees); the GUI Ellipse tool is two clicks (centre,
bounding-box corner; axes along u / v).

## Amendment (2026-11-09): full-round fillets

A full round replaces a face lying between two parallel faces by a tangent half cylinder
across the whole wall (Inventor, SolidWorks and Fusion have it; OCCT's fillet of both edges at
r = t/2 fails with "fillet failed" because the two blends are tangent to each other).

- **Syntax.** `fillet <Name> (full_round: [<face>, ...])`. It replaces `edges` and `radius`
  (giving them too is a parse error; `full_round: []` is one too). `chamfer` has no such form.
  The faces are ordinary persistent face references, so they are renamed with the feature they
  point to and count as dependencies, and the round is editable (`feature.edit`, code view).
  The model keeps it in `EdgeBlendDef::full_round: Vec<FaceRef>` (empty for every other
  fillet or chamfer, which print exactly as before); `size` is an unused `0 mm` placeholder.
- **Meaning.** Each named face must be planar and have two planar neighbours with antiparallel
  outward normals (parallel planes, the material between them) at distance t, both
  perpendicular to the face. The face is replaced by a half cylinder of radius t/2 whose axis
  runs along the face's edges with the two sides, tangent to both sides, with its crest on the
  old face's plane: the material removed is the two corner pieces, volume
  `(t * t/2 - pi * (t/2)^2 / 2) * L` for a face of length L along the axis. When the face has
  several pairs of parallel neighbours (a cube face has two), the pair with the smaller distance
  is rounded, the first found on a tie. Other cases (non-planar middle face, slanted sides,
  no parallel pair) fail with a hint; they are not approximated.
- **Kernel.** `Kernel::full_round(shape, faces)`; OCCT builds, per face, a prism along the axis
  over the face's extent whose section is the rectangle across the wall (from just outside the
  old face down to the cylinder's centre line) minus the half disc, and cuts all of them in one
  boolean (`gs_full_round`, exact result, no near-tangent offset). The remote protocol carries it
  as request 36. The mock kernel does not support it.
- **Naming.** The cylindrical face reports `FromEdge` for the old face's edges with its two
  sides, so it is named `<feature>/blend(<edge name>)` with the smaller edge name (ADR 0003);
  the sides keep their names, the old face disappears.
- **Command.** `feature.fillet` gains `full_round` (list of face names); `edges` and `radius`
  are then left out (and `radius` is required otherwise). The MCP tool entry stays under the
  byte budget. No GUI form yet.

### Amendment: `refine: false` keeps a boolean's split faces (2026-11-09)

FreeCAD unifies the coplanar faces and collinear edges of a pad, pocket or boolean only when the
feature's `Refine` is on; a file with `Refine = false` is rebuilt (and later filleted) on the
unrefined shape. Gusset unified always, so such a file got fewer faces and a fillet found
different material (FreeCAD Page_108: 90 faces instead of 174, volume 1.1e-4 off).

- **Syntax.** `refine: false` on `extrude`, `revolve`, `sweep`, `loft`, `combine`, `mirror` (with
  `merge`), `pattern_linear` and `pattern_circular` (default `true`, printed only when false; the
  definitions gain `refine: bool`). It governs the boolean of `op: add|cut|intersect` (and of a combine); with
  `op: new` there is nothing to unify and it is accepted and ignored. Dressups and holes still
  refine (the FreeCAD files at hand need no more). Grammar and round trip are tested.
- **Kernel.** `Kernel::boolean_unrefined` and `boolean_many_unrefined` (defaults call the refined
  forms, so the mock is unchanged). OCCT passes `refine` to `gs_boolean`/`gs_boolean_many`, which
  run the boolean without `SimplifyResult`; the "unify, and retry without if the result is
  invalid" fallback of `run_boolean_checked` is skipped for it. Remote protocol 12: requests 6
  and 7 carry the flag.
- **Eval.** `combine_refined` and `boolean_tools_refined` take the flag; each feature passes its
  own.
- **Naming.** An unrefined result keeps the boolean's own face history, so each split face has
  its own origin and, with the ADR 0003 ties, its own persistent name; golden tests check that
  all 10 faces of two touching cubes are named distinctly, survive a parameter change and take a
  fillet by name. A fillet of the flat seam edge itself fails as in FreeCAD (tangent faces).

### Amendment: extrude end condition "up to a face" (2026-11-09)

SolidWorks' "up to surface" and "offset from surface", FreeCAD's Pad/Pocket `UpToFace` and
Fusion's "To object" end an extrude on a face of the model instead of a distance. FreeCAD's
up-to types used to be translated to a computed distance, which broke when the target moved.

- **Syntax.** `extrude E (profile: S, to: Base/cap(end), offset: 2 mm, reverse: true, op: ...)`.
  `to:` takes a persistent face reference (ADR 0003) and excludes `distance:` and `symmetric:`;
  `offset:` (a length expression, default 0) and `reverse:` (default false) go with `to:` only
  (each misuse is a parse error and, for programmatic documents, a validation issue). The
  model replaces `ExtrudeDef.distance` by `ExtrudeDef.end: ExtrudeEnd` (`Distance(Expr)` or
  `ToFace { face, offset, reverse }`). The face is a dependency of the feature (renames follow it
  through `rename_feature`; the offset reads parameters), and the feature reads all bodies at
  evaluation time (dependency analysis marks it dynamic).
- **Semantics.** The profile is swept along the sketch normal (against it with `reverse`) and ends
  on the surface of the face: a plane (also tilted), a cylinder or another surface bounded as the
  face is. `offset` moves the end further along the sweep (negative: short of it), measured
  perpendicular to a planar face; curved faces take none. The target body may be the body being
  cut (a pocket up to a face).
- **Refusals** (clear messages, nothing is guessed): the face does not exist any more (missing
  reference with a closest-name suggestion); the face is edge-on to the sweep; it lies behind the
  sketch or only partly ahead of the profile (planar faces: every corner of the profile's box must
  be reached; `reverse` flips the direction); the sweep never reaches the surface; the end of the
  sweep is not inside the face itself (the face is too small to "cover" the profile); an offset on
  a curved face.
- **Kernel.** `Kernel::extrude_to_face(profile, target, face, offset, reverse)` returns the swept
  tool solid with the face origins of an ordinary extrude (laterals by edge tag, start cap, end
  cap), which the evaluator combines as for any extrude; the default is "unsupported" (the mock
  kernel refuses). OCCT (`gs_extrude_to_face`) makes the prism longer than needed, splits it with
  `BRepAlgoAPI_Splitter` by the face's surface (a plane or cylinder extended past the face, any
  other surface as bounded, a planar surface shifted by the offset), keeps the piece holding the
  start cap and checks that the cut-off end lies inside the (shifted) face by comparing its area
  with its common part with the face. BRepFeat_MakePrism was not used: it works on the body it
  fuses to and gives no tool solid for cuts, offsets or new bodies. Remote protocol request 37.
- **Naming.** The end cap is `Feature/cap(end)` like any extrude's; the lateral faces
  (`Feature/side(Sketch/l5)`) keep their names. Where the face cuts a lateral face in two, the
  piece in the result keeps its origin.
- **Commands.** `feature.up_to_face` (sketch, regions, to, offset, reverse, op, body,
  body_name, name). `feature.extrude` keeps its schema: it is one of the always-listed tools
  and the MCP tool list is capped at 18 000 bytes; the new command is reached through
  `commands`/`run`.
- **Not done.** No GUI form yet; curved target faces other than cylinders are cut at their own
  parametric bounds; `symmetric` and "up to next / up to body / through all" are not covered.

## Amendment (2026-11-10): `scale` and `remove`

Two whole-body features complete the set of body edits a feature tree (ADR 0042) can express.

- `scale <Name> (factor: <n> | factors: [<x>, <y>, <z>], [about: [x, y, z]], [body: <Feature>])`
  scales a body about a point (default the origin). Factors are dimensionless and must be
  positive (a mirror flips); `factor` is one for all axes, `factors` one per world axis. The
  kernel transform is `Transform::Scale { center, factors }`: equal factors are a `gp_Trsf`
  similarity (every surface keeps its type, volumes grow by factor^3), unequal ones a `gp_GTrsf`
  through `BRepBuilderAPI_GTransform` (curved faces become B-spline surfaces; volume is the
  product of the factors, to integration accuracy). Like `move` the body stays in place in the
  document: its feature, name and face names do not change (`OpContext::Modify`), and holes
  recorded on it keep their records. The mock kernel scales uniformly only.
- `remove <Name> (body: <Feature>)` drops the body from the evaluated bodies. `body:` is required
  (a deletion by default would be too easy to write by mistake). Validation treats it like a
  consumed combine tool: a later feature naming the body is an error ("body 'X' was removed by
  R", hint: move the feature before R or delete R); a feature without `body:` works on the latest
  body that remains. A suppressed `remove` removes nothing. Appearances of a removed body stay
  unused, as for any body that is gone. The body's features before the `remove` still build
  (the history is kept); only the result lacks it.

## Amendment (2026-11-11): exact B-splines in the text language
Sketch entity `b9 = bspline(degree: 2, poles: [p1, p2, ...], knots: [...], weights: [...])`
(id prefix `b`), optionally followed by `periodic`, then `construction`. `poles` are ordinary
point ids (a pole may repeat: a clamped closed curve lists its first point again last),
`knots` the flat knot vector (`poles + degree + 1` numbers, `poles + 1` when periodic),
`weights` one per pole or left out (then empty in the model, meaning all 1; the text prints
them only when the model has them, so text round-trips exactly). Knots and weights are
printed exactly. Validation: degree 1 to 9, at least `degree + 1` and at most 1000 poles, the
knot count, finite non-decreasing knots, positive finite weights, poles that are earlier
points; multiplicities and the non-empty domain are checked when the sketch is built (the
sketch crate's error names the entity). B-splines are pattern sources (copies map the poles,
knots and weights stay); they cannot be trimmed. Persistent names need nothing new beyond the
per-segment tags of splines: each knot span is one profile edge tagged
`with_segment(tag, span)`, so the lateral faces of an extruded B-spline are named
`Extrude1/side(Sketch1/b9_g0)`, `..._g1`, ... (ADR 0003). No `sketch.add_bspline` command and
no GUI tool yet: B-splines come from importers and the code view.
