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
- **Later (ADR 0061).** `extend: true` extends the face's surface past its edges (the end may lie
  outside the face); fillets take radii at vertices (`at: [vertex(...) = <r>]`).

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

## Amendment (2026-11-11): body patterns that keep their copies apart
`pattern_linear` and `pattern_circular` take `merge: false` (default `true`, printed only when
false): the body is not changed and copy `i` (1 to count - 1) becomes a body of its own whose
name and feature are `<pattern>_copy<i>` (`gusset_doc::pattern_copy_name`; parsed back by
`pattern_copy_of`). Later features target it like any body (`body: Pattern1_copy2`, also
`combine` tools and `remove`); its faces keep the instance names of patterned faces (ADR
0003 `OpContext::Instance`), so they differ from the original's. Validation: a body reference
`<P>_copy<i>` is known when `P` is an earlier pattern with `merge: false` (a copy number past
the count is reported at evaluation: "body 'P_copy5' does not exist"); no feature may be named
like a copy of such a pattern; `merge: false` with `features:` is refused. Dependencies
(gusset-eval `deps.rs`): a copy no feature has changed yet was written by its pattern. The
`pattern_linear` / `pattern_circular` commands keep merging (no new parameter: MCP tool
budget); the code view and tree import write `merge: false`.

## Amendment (2026-11-12): `split` and closed shells
`split <Name> (plane: <Plane>, [body: <Feature>])` divides a body by a plane (a base plane, a datum
plane or a planar face): what lies on the side the plane's normal points away from stays the body
(its feature, name and records), what lies on the side it points to becomes a new body whose
name and feature are `<Name>`. The kernel cuts and intersects the body with a block on the
plane (named like an import, `OpContext::Import`), refined; a plane that does not cross the body
is an error (empty result). Validation treats it as a feature that creates a body. There is no
command for it yet: it comes from the Fusion tree import (Split Body) and the code view.
`shell` with `faces: []` and `body:` hollows the whole body: the body keeps its skin and gets the
offset skin as a closed void (`thickness < 0`: the offset skin outside, the old body the void).

## Amendment (2026-11-19): blends OCCT's fillet does not build
A `fillet` or `chamfer` means the rolling-ball (or setback) blend of its edges, as the CAD systems
we import make it. Where OCCT's `BRepFilletAPI` fails (or gives an invalid solid), the OCCT kernel
tries, in order: the edges one at a time, concave ones first, then the blend as booleans of its
corners (each edge's corner region cut away, or filled for a concave edge), then a solid of
revolution, a moved seam, a tapered loop (chamfers). The corner booleans are accepted only where
the material removed (added) equals the corners' own volume to 1e-7 of the body and every face of
the result has an origin, so a fallback gives the intended geometry or nothing. Rules of the
corners, as the importers' stored bodies show them (Solid Edge 040, 053, 065, 026, 105, 094):

- *Cliff edge.* Where a face beside the edge is narrower than the blend's touching line on it,
  the ball rolls on the other face and on that face's far edge: the blend takes the face whole and
  ends on the face beyond (straight edges between planes at any angle, whole circles between
  planes, cylinders and cones about their axis, straight edges between a plane and a cylinder
  along them).
- *Mitre.* Two straight fillet edges of one radius that meet at a vertex of three edges meet on
  the plane bisecting the faces beside them.
- *Ends on a slanting or curved face.* A straight corner whose end lies on one more face (a plane
  at an angle, a cylinder) runs on to that face's surface and is trimmed by it; the new faces on
  that surface keep that face's origin. Its volume is measured face by face (Gauss-Kronrod): the
  default quadrature is some 5e-6 off on cylinders bounded by B-spline intersections.
- *Contact along an edge.* A fillet whose touching line runs exactly along another edge of its face
  is not given to OCCT (it crashes there, `BRepBlend_SurfRstConstRad`); only the corners are tried.

## Amendment (2026-11-20): cliff-edge rounds of any section
Round se8 (ADR 0046 amendment 28) builds the cliff edge above for any faces and edges, as Solid
Edge's cliff-edge blends (Parasolid `blend_type` `E`) make it, among the corners `blend_by_corners`
tries where OCCT's fillet fails:

- *When.* At the edge's middle, the plain ball's contact (`r / tan(alpha / 2)` along each face,
  square to the edge) lies outside exactly one face (`b`), and `b` has one edge that touches no
  end of the rounded edge (its far edge), whose end planes pass through the rounded edge's ends
  (or both are closed). Convex and concave edges alike.
- *Construction.* At stations along the far edge, the ball's centre lies in the plane square to
  it at the radius from it and at the radius from the other face's surface (inside it for a
  convex edge, outside for a concave one), on the ball's side of `b`: a root of one function of
  the angle about the far edge, found by scanning and bisection; of several, the one whose
  contact lies in the face, nearest the edge. Each row is the ball's circular arc from its
  contact to the far edge; the corner is the loft (`BRepOffsetAPI_ThruSections`) of the rows'
  sections, the arc and both faces' traces back to the edge (points moved onto each surface
  within the row's plane), and the tool the loft of the arc and a polygon reaching out of the
  body (convex, cut) or into it (concave, added). Rows are doubled from 4 until the loft lies
  within 1e-5 of the radius at the arcs between rows; loft faces that lie in a plane are made
  plane faces (booleans against B-spline faces in a plane went wrong and slow).
- *Corners that meet.* Where two such corners' far edges share a vertex, both balls run on past
  it along the far edges' tangents (2 r) until their blends cross; each corner keeps its run
  past that end only where it lies in the other's, so the blends meet along their intersection
  curve with no face between them (Solid Edge 081: the three rounds about a tab's foot; its
  stored blends meet that way). Boxes, not OCCT half-spaces, trim the runs: half-space booleans
  gave wrong solids. The material changed must be the corners along their edges plus what both
  runs hold, to 1e-7 of the body, as for the other corners; the runs' end faces that stay on the
  result take the edge's origin.
- Not covered yet: a narrow face whose far side is several edges or ends at the rounded edge's
  own vertices (044's D-shaped face), a round whose contact leaves its face only near an end
  (067's pipe tee), both faces narrower than the ball (101, 016).

## Amendment (2026-11-20): closed cliff corners, concave arcs, unnamed slivers
Round se9 (ADR 0046 amendment 30):

- *Closed cliff corners* (a rounded circle whose narrow face's far edge is closed too) are lofted
  in two halves and fused into one tool, at all 64 rows: one loft whose end sections lie on one
  another left both in the body as a slit of two faces with no origin. Filled (concave) ones too;
  where the filled corners then do not measure as their own volume, each closed filled corner is
  tried again as one loft (Solid Edge 041's ring measures as its corner only that way).
- *Concave arcs.* A concave arc between a plane across its axis and a cylinder about it, whose
  ends carry on into planes tangent to the cylinder (the foot of a boss with rounded corners),
  has a corner: the section revolved over the arc, ending on the planes through the axis at its
  ends. A filling tool reaches less than a quarter of the radius into the material where that is
  thinner (halved until the section's reach points lie in the body), so it adds only its corner.
- *Far edge.* Of the narrow face's edges that touch no end of the rounded edge, the far edge is
  the one whose end planes pass through the rounded edge's ends; a far side of several edges is
  no longer refused outright.
- *Origins.* A corner cut that leaves a face with no origin fails (later fallbacks are tried)
  instead of failing in naming. A face of OCCT's fillet that its history names neither as
  modified nor as generated (a sliver split off a face it trims) takes the origin of the input
  face it lies on (same plane or cylinder, the nearest).

Golden tests: a hole's rim in a plate thinner than the radius, a rod's foot on a disc narrower
than the radius (closed cliff corners cut and filled, volumes by Simpson's rule over the
section), a thin-walled rounded boss filled round its foot to its full height.

## Amendment (2026-11-23): boxes reach the peaks of faces of revolution
Round se10 (ADR 0046 amendment 31). `gs_bbox` measured with `BRepBndLib::AddOptimal`, which
can miss a circle's peak on a face of revolution by far more than its tolerance: Solid Edge
163's stored mug (a B-spline profile spun about the axis, a boss on its seam) measured 0.045 mm
short of its 29.654 mm radius on one side, so the rebuild check's box test failed although the
volumes agreed to 4e-6. The box now also takes, for each face of revolution, the points where
its circles peak along each axis: the profile's centre and radius are searched along v (256
samples, then a golden-section search), the peak's angle follows from the axis, and the point
is added only when the face classifies it as on it, so the box never grows past the face.
Test: a cup whose spun Bezier side bulges past its rim, with a boss on its seam: its box is the
side's widest radius all round.
