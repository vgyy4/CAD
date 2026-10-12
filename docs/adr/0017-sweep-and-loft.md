# ADR 0017: Sweep and loft

- Status: accepted
- Date: 2026-10-12

## Context
v0.2 needs the two freeform solid features that extrude and revolve cannot express: pipes,
rails and twisted bars (sweep) and transitions between outlines on different planes (loft).
The helical gears of ADR 0018 are twisted sweeps. Both go through the same layers as the other
features (document model, text language, evaluator, command registry, OCCT shim), and their
faces must keep persistent names (ADR 0003).

## Decisions

### 1. Two feature kinds
```
sweep <Name> (profile: <P>, path: <path>, orientation: frenet|fixed, [twist: <angle>], op: ..., [body: ...], [body_name: ...])
loft  <Name> (profiles: [<P>, <P>, ...], [ruled: true], op: ..., [body: ...], [body_name: ...])
```
`op`, `body` and `body_name` behave exactly as for extrude and revolve (a new body, or
add/cut/intersect with a target body), so one `combine` step in the evaluator serves all four.
`profile` is the usual `Sketch.regions(...)` reference and may select several regions (they
are swept side by side, like an extrude does); a profile region with holes works for a sweep
(a tube) and is refused by a loft.

### 2. The path
`path:` is one of
* `Path` (a sketch): every non-construction curve of that sketch must form **one chain**,
  open or closed;
* `Path.curves(l3, a4)`: only these curves (construction curves may be named);
* `[edge(...), ...]`: edges of an earlier body, by persistent edge reference, in any order.

`gusset_sketch::chain` joins the curves end to end (endpoints within 1e-6 mm are one joint)
and reports a branch (more than two curves at a point), a gap (the curves not joined) or a
circle/closed spline mixed with other curves, each with the entities involved. A lone circle
or closed spline is a closed path. Pattern copies of curves (ADR 0016) are curves like any
other. The kernel receives the chain as `Path2` (a plane frame plus ordered curve edges) or as
edge indices of a shape; the Bézier pieces of one spline are joined into a single edge so a
spline path does not break the sweep into a face per piece.

**Direction.** The sweep starts at the end of the path that lies in the profile's plane
(within 1e-3 mm); the kernel reverses the chain if needed, so users never choose a direction.
A path with no end in that plane is an error with the distance and a hint. A closed path
may cross the profile plane anywhere and has no start/end caps.

### 3. Orientation and twist
* `frenet` (default): the profile follows the path using OCCT's *corrected* Frenet frame
  (`BRepOffsetAPI_MakePipeShell::SetMode(false)`): it stays put on straight stretches and does
  not flip at inflection points, the usual meaning of "perpendicular to the path".
* `fixed`: the profile keeps its orientation and slides (`SetMode(gp_Ax2)`).
* Corners where path edges meet at an angle are mitred (`BRepBuilderAPI_RightCorner`), so a
  swept polyline keeps A × path length as its volume and the faces stay planar/cylindrical.
* `twist` is the total turn of the profile about the path from start to end (right-hand rule
  about the travel direction), spread evenly along the path. It needs `frenet` and an open path
  (a closed path has no end to turn towards). OCCT has no twist option for a pipe shell, so
  the kernel builds an **auxiliary spine**: a curve at a small distance from the path
  (5 % of the length, at most 5 mm) in a rotation-minimising frame, turned by the twist along the
  way, and passes it to `SetMode(auxiliary, false)`; the shell's trihedron points at that curve.
  Parametric (not curvilinear) correspondence is used, and the guide carries the path's own
  parameter at each station: curvilinear pairing measurably lost volume (0.1 to 0.4 %) in
  experiments, parametric pairing is exact for a straight path (volume A × L to 1e-7) and for
  bends (Pappus' value to 1e-6). Along a path of several edges the twist is spread by the
  path's own parameter, which is evenly by length only when all edges have the same parameter
  speed; the end orientation is always exact.

### 4. Loft
Two or more sections, each exactly one outline without holes. OCCT's `BRepOffsetAPI_ThruSections`
makes a solid; `ruled` joins neighbours with ruled surfaces, otherwise one smooth surface passes
through all of them. Sections are made to run the same way round the loft direction before
joining (profiles on planes with opposite normals would otherwise twist), and OCCT chooses the
start vertex that twists least. Profiles with different edge counts work (a square lofted to a
circle); OCCT splits edges as needed.

### 5. Face names
* Caps are `<feature>/cap(start)` and `cap(end)` (the profile at the path start / the first
  section, and the other end). A closed path has none.
* **Sweep sides** are `side(<profile sketch>/<entity>)` from the profile edge that was swept
  (`FaceOrigin::Swept { tag, path }`). A path of several edges gives one face per path edge:
  the first keeps the plain name, later ones append `_p<k>` to the entity (`side(Profile/c2_p1)`),
  k counted from the path's start. That makes the names independent of geometry, so parameter
  edits keep them (golden scenario `sweep_and_loft_names`). A twisted or bent sweep of a
  straight profile edge has a freeform side face, so these names are used for edge references
  and fillets, not as sketch planes.
* **Loft sides** are `side(<sketch>/<entity>)` of the section edge that generated the face
  (`FaceOrigin::Section { section, tag }`, naming each profile by its sketch). Because
  `ThruSections` reports all edges that feed a face, and an edge may feed several faces when the
  edge counts differ, a face takes the origin shared with the **fewest other faces**, then the
  first section, then the smallest tag. A square lofted to a circle is thus named after the
  square's edges. Faces that still share a name get geometric `#k` indices (ADR 0003 §4).

### 6. Holes in a swept profile
`MakePipeShell` sweeps wires, not faces. Each region is swept as its outer loop; its hole loops
are swept separately and cut out in one boolean, and the origin records (profile edge, caps) are
carried through that boolean's history. Both loops' lateral faces keep their `side(...)` names
and the caps keep `cap(start)`/`cap(end)`.

### 7. Mass properties
Swept and lofted faces are B-spline surfaces and OCCT's default quadrature is off by a few
tenths of a percent on twisted ones. `gs_mass` integrates adaptively (relative error 1e-7) when
the shape has a face that is not a plane, cylinder, cone, sphere or torus, and keeps the fast
default otherwise, so existing numbers do not change.

### 8. Kernels
`Kernel::sweep` and `Kernel::loft` default to `Unsupported`; only OCCT implements them. With
the mock kernel the features fail with `KernelFailure: sweep is not supported by the mock kernel`,
the rest of the rebuild goes on and features that depend on them are blocked. Everything that
does not need geometry (text, validation, path chains, argument checks, command registry,
renaming) is tested without OCCT; geometry is tested in `gusset-cli/tests/modeling_sweep.rs`.

### 9. MCP surface
`feature.sweep` and `feature.loft` (two tools; `path` is one string in the text-language form,
`profiles` a list of profile references). Their schemas are kept short to respect the tool-list
budget (about 600 bytes per tool on average).

## Not included (later)
Guide curves and multiple guide rails, scaling or other laws along a sweep, tangency
conditions at loft ends, closed (periodic) lofts, lofts with holes, twisting closed paths,
sweeping a profile along a path that starts elsewhere than the profile plane (use a plane
through the path start).
