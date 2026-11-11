# ADR 0042: Feature-tree interchange (exporter plugins)

Status: accepted (2026-11-07)

## Context

Proprietary CAD files read natively (tier 2: `.f3d`, `.sldprt`, Parasolid, ACIS) give exact
bodies but no history. PROMPT.md asks for tier 1 as well: small exporter plugins that run
inside the user's own copy of the source application (a Fusion Python add-in, a SolidWorks
macro, an Onshape REST script) and dump the feature tree to an open JSON file that Gusset
imports as editable features. The plugins cannot be run here (Windows/macOS, licences, GUI),
so they stay thin dumpers written against the official API documentation, marked
UNVERIFIED until a person runs their manual test script, and every mapping decision lives
in the Rust importer, which is tested with hand-written files.

## Decision

### The file

UTF-8 JSON, extension `.gtree.json`. All lengths are millimetres and all angles degrees,
whatever the source document's units; coordinates are world coordinates of the source
document (Z up for Fusion and Onshape as exported; SolidWorks' Y-up is converted by the
plugin to Z-up: x' = x, y' = -z, z' = y).

```json
{
  "format": "gusset-feature-tree",
  "version": 1,
  "source": { "app": "fusion | solidworks | onshape", "app_version": "...", "exporter": "plugins/fusion 0.1.0", "document": "Bracket" },
  "parameters": [ { "name": "width", "expression": "40 mm", "value": 40.0, "kind": "length | angle | number" } ],
  "features": [ { "type": "...", "name": "...", "...": "..." } ],
  "bodies": [ { "name": "Body1", "volume_mm3": 12345.6, "bbox_mm": [[0, 0, 0], [40, 20, 10]] } ]
}
```

- `parameters`: the user parameters. `value` is in mm / degrees / plain; `expression` is
  the source app's text and is used only when it parses as a Gusset expression over the
  parameters already defined (else the value is used and a warning names it).
- `features`: in timeline order. Suppressed features are left out by the plugin.
- `bodies`: the final bodies' volumes and boxes, for the importer to compare against and
  report (`mismatches`), never to build from.

### Feature types (version 1)

`sketch`:
```json
{ "type": "sketch", "name": "Sketch1",
  "plane": { "origin": [0,0,0], "x_axis": [1,0,0], "y_axis": [0,1,0] },
  "points":  [ { "id": "p1", "x": 0, "y": 0, "construction": false } ],
  "curves":  [ { "id": "l1", "type": "line", "start": "p1", "end": "p2", "construction": false },
               { "id": "c1", "type": "circle", "center": "p3", "radius": 5 },
               { "id": "a1", "type": "arc", "center": "p4", "start": "p5", "end": "p6" } ],
  "constraints": [ { "type": "horizontal", "entities": ["l1"] },
                   { "type": "distance", "entities": ["p1", "p2"], "value": 40, "parameter": "width" } ] }
```
Point coordinates are in the sketch's own frame (`plane`: world origin and unit axes). An
arc runs counter-clockwise (about the plane normal x × y) from `start` to `end`. Ids are
the plugin's own (any `[A-Za-z0-9_]{1,32}`); the importer renumbers them. Constraint types
are Gusset's: `coincident, on_line, on_circle, midpoint, horizontal, vertical, parallel,
perpendicular, tangent, equal, distance, angle, radius, diameter, fixed, symmetric`
(`angle`: the directed angle of Gusset's grammar; lengths in mm, angles in degrees).
`parameter` names the user parameter driving a dimension, when there is one. Plugins may
leave constraints out; the sketch then imports with its geometry fixed.

`extrude`:
```json
{ "type": "extrude", "name": "Extrude1", "sketch": "Sketch1",
  "profiles": "all" | [ { "point": [u, v] } ],
  "distance": 10, "symmetric": false, "distance_parameter": "depth",
  "operation": "new | join | cut | intersect", "target": "Extrude1" }
```
`profiles` picks sketch regions by a point inside each (sketch coordinates). `distance` is
signed along the sketch normal (x × y); `symmetric` extends half each way. `target` is the
name of the feature whose body a join/cut/intersect changes (default: the latest body).

`revolve`: like `extrude` with `"axis": { "line": "l3" }` (a sketch line) or
`{ "origin": [x,y,z], "direction": [x,y,z] }` (world) and `"angle": 360`.

`fillet` / `chamfer`:
```json
{ "type": "fillet", "name": "Fillet1", "edges": [ { "midpoint": [x, y, z] } ], "radius": 2 }
{ "type": "chamfer", "name": "Chamfer1", "edges": [ { "midpoint": [x, y, z] } ], "distance": 1 }
```
Edges are found by their midpoint (world mm) on the model as built so far: the importer
picks the nearest edge within 0.01 mm (else the feature is skipped with a warning).

`unsupported`: `{ "type": "unsupported", "name": "Loft3", "original_type": "LoftFeature" }`;
the plugin writes this for every feature it does not export, so the report can list them.

### The importer

`import_file` reads a `.gtree.json` (the `format` field decides) and appends Gusset features
to the document instead of an imported body: parameters first (names kept when free, else
suffixed), then the features in order, generated as code-view text so they are parsed and
validated like any other edit. Sketch planes: a plane that coincides with XY, XZ or YZ uses
it (points re-projected into Gusset's frame for that plane, so any in-plane rotation or
origin works); a plane parallel to one of them becomes a datum `plane` with an offset; any
other plane becomes up to three chained datum planes: XY turned about X by a, then about Y by
b (normal (cos a sin b, -sin a, cos a cos b)), then offset along the normal, angles and offset
written to 12 decimals (amendment 2026-11-07; previously reported unsupported). Fillet and
chamfer edges are resolved after evaluating the features before them. The result reports
features made, features skipped (with reasons), parameters, and for each source body its
volume against the imported model's.

### The plugins

`plugins/fusion` (Python add-in, Fusion API), `plugins/solidworks` (VBA macro, SolidWorks API),
`plugins/onshape` (Python script, Onshape REST API v6, keys from the environment, never
committed). Each: README marked UNVERIFIED with the API version targeted, a manual test
script (install, a test model, expected output) a person can follow in about ten minutes,
and an exporter that only reads the source model and writes the file. docs/INTEROP.md lists
them as UNVERIFIED until run against the real application.

## Consequences

Editable history from the three most used proprietary tools, for the common features
(sketch, extrude, revolve, fillet, chamfer); everything else is reported per feature. The
format is versioned; later versions add holes, patterns, shells and assemblies.

## Amendment (2026-11-07): limits after the security review

The file is untrusted. A tree is at most 32 MiB, 2000 features, 5000 parameters, 200 000
sketch entities in all (20 000 per sketch) and 256 profile or edge picks per feature, and an
import builds the model at most 400 times; past that, features that need a build are
skipped. Every feature's text is taken back out when it fails, and a sketch is kept only if
the text still parses, so one bad feature never spoils the rest. Numbers must be finite
(below 1e12), dimension values go only on `distance`, `angle`, `radius` and `diameter`, and
curves end on points only. Parameter expressions are re-printed from their parse with their
variables renamed to the imported names; unit and built-in names get a prefix. The report's
untrusted strings are in `skipped_user_text` and `warnings_user_text`, truncated to 200
characters and 50 entries; a `source` block larger than 2 KB is dropped.

## Amendment (2026-11-07): format version 2 (holes, patterns, mirrors, shells)

`"version": 2` adds five feature types. The importer accepts versions 1 and 2; a version 1
file that holds a version 2 type has that feature skipped ("is in version 2 files"). All
lengths mm, angles degrees, world coordinates, as before. The security properties of the
amendment above hold unchanged: the same caps (256 picks per feature, 400 builds), finite
numbers below 1e12, each feature built where it stands and taken back out if it fails, and
untrusted text only in the `_user_text` report fields.

Faces are found like edges: by a point on them, on the model as built so far. The importer
evaluates the features before the one in hand and takes the planar face whose plane holds the
point within 0.01 mm (when a `normal` is given, its outward normal must also agree within
about 2.5 degrees), or the cylindrical face whose radius from its axis matches; of several
faces on one surface the one whose region (by the body's tessellation, within 0.01 mm) contains the point
wins, then the one whose centre is nearest; when no face region contains it the nearest centre is
taken with a warning. A point that no face's surface holds skips
the feature. Face picks are `{ "point": [x, y, z], "normal": [nx, ny, nz] }`, `normal`
optional (outward).

`hole`:
```json
{ "type": "hole", "name": "Hole1",
  "face": { "point": [20, 10, 10], "normal": [0, 0, 1] },
  "positions": [ [10, 10, 10], [30, 10, 10] ],
  "diameter": 6, "through": true, "depth": 12,
  "variant": "simple | counterbore | countersink",
  "cbore_diameter": 10, "cbore_depth": 4,
  "csink_diameter": 12, "csink_angle": 90, "point_angle": 118 }
```
`positions` are world points on the face (within 0.01 mm of its plane; at most 256), each
becoming one hole. `depth` is from the face to the end of the full-diameter part (a blind
hole adds a drill point of `point_angle`, default 118, 180 is flat); `through: true` replaces
it. `counterbore` needs `cbore_diameter` (wider than `diameter`) and `cbore_depth`;
`countersink` needs `csink_diameter` and `csink_angle` (full cone angle, default 90).
Threaded and slotted holes are exported by the plugins as `simple` at the tap-drill diameter
or as `unsupported`. The face must be planar. The importer converts positions into Gusset's
`at: [e1: [u, v], ...]` in the face's own frame.

`pattern_linear` / `pattern_circular`:
```json
{ "type": "pattern_linear", "name": "Pattern1", "features": ["Boss1"], "target": "Extrude1",
  "direction": [1, 0, 0], "count": 3, "spacing": 20 }
{ "type": "pattern_circular", "name": "Pattern2", "features": ["Hole1"],
  "axis": { "origin": [0, 0, 0], "direction": [0, 0, 1] }, "count": 6, "angle": 360 }
```
`features` names earlier features (extrude/revolve joins and cuts, holes, fillets...) whose
effect on their body is repeated (Gusset's `features:` pattern, ADR 0034); without it the body
named by `target` (default the latest) is repeated as a whole. `count` includes the original
(2 to 500). Linear: `spacing` mm between neighbours along `direction` (any length, it is
normalised). Circular: `angle` is the total turn, as in Gusset: 360 spreads the copies evenly
around the circle, a smaller angle spans first to last copy. The listed features must have
been imported, and must add to or cut from one body.

`mirror`:
```json
{ "type": "mirror", "name": "Mirror1", "features": ["Boss1"],
  "plane": { "origin": [0, 0, 0], "normal": [1, 0, 0] } }
{ "type": "mirror", "name": "Mirror2", "target": "Extrude1", "merge": true,
  "plane": { "origin": [20, 0, 0], "normal": [1, 0, 0] } }
```
With `features` (extrude, revolve, hole, fillet, chamfer; revolves only of 360 degrees) the
importer builds each feature again with its world geometry reflected across the plane, the
sketches included, named `<name>_<mirror name>`; a copy's `target` naming another copied feature becomes that
feature's copy; distances become plain numbers (a driving
parameter's sign does not carry over). Reflected sketch planes that end up tilted land on
tilted datum planes as above. Whole-body mirrors across a tilted plane mirror across such a datum plane too. Without `features` the whole
body of `target` is mirrored with Gusset's `mirror` across a base or offset datum plane (the
plane must be parallel to XY, XZ or YZ); `merge` (default true) unites the copy with the body.

`shell`:
```json
{ "type": "shell", "name": "Shell1", "faces": [ { "point": [15, 10, 10], "normal": [0, 0, 1] } ],
  "thickness": 2, "side": "inside | outside" }
```
`faces` are the faces to remove (planar or cylindrical, all on one body); `thickness` is the
wall, positive; `side` (default `inside`) says which way the wall grows from the original
surface.

Everything else about a feature that the importer cannot express (variable-radius patterns,
pattern seed bodies, tilted mirror planes, tapped holes with helical geometry) is skipped
with its reason. The plugins export what the source API gives directly and write `unsupported`
for the rest.

## Amendment (2026-11-09): sweeps in format version 2

`{"type": "sweep", "name", "sketch", "path", "path_curves": [curve ids, all when empty],
"orientation": "frenet" | "fixed", "operation", "target"}` sweeps the profile sketch along
curves of the path sketch (Gusset's `sweep`, ADR 0017). FreeCAD's pipes are written this way
(ADR 0033 amendment 2026-11-09).

`{"type": "loft", "name", "sketches": [two or more, in order], "ruled", "operation", "target"}`
lofts through the sketches' closed profiles (Gusset's `loft`); FreeCAD's additive and
subtractive lofts are written this way.

A hole's positions may lie on several faces of its face's plane (separate pocket floors, as
SolidWorks hole-wizard features with head clearance give): each face gets its own Gusset
`hole`, named after the first.

`{"type": "combine", "name", "target", "tools": [names], "operation": "join" | "cut" |
"intersect"}` combines bodies made by earlier features into the target body (Gusset's
`combine`); FreeCAD's Part workbench booleans are written this way.

## Amendment (2026-11-09): unequal chamfers

A `chamfer` feature (either format version) may add `distance2` (mm, > 0) or `angle` (degrees,
0 to 90 exclusive), not both, and `face: {"point": [x, y, z], "normal": [x, y, z]}` (the
`normal` is optional), the *reference face*: the face the first `distance` runs along, and the
one the angle is measured from. Exporters map their two-distance and distance-angle chamfers to it, naming the face their first distance lies on. Without
`distance2`/`angle` the chamfer is equal and `face` is ignored with a warning.

- The face is found like a hole's face (a point on the surface within 0.01 mm, the normal
  within 2.5 degrees when given) and written as a persistent name in the chamfer's `faces`
  list, one per edge. An edge that does not border the picked face takes the default
  reference face (the adjacent face whose persistent name sorts first, ADR 0004 amendment)
  and a warning says so; a face on another body than the edges is an error.
- Without a `face`, no `faces` list is written and the default applies.
- A bad `distance2` or `angle`, or both together, skips the feature with the reason in
  `skipped_user_text`, like any other feature that does not build.
- Exporter plugins do not need a new format version: the fields are optional and a version 1
  reader that ignores unknown fields still builds an equal chamfer from `distance`.
  Gusset's reader takes them in version 1 files too.

## Amendment (2026-11-09): ellipses and elliptical arcs in sketches

Two more sketch curve types (either format version):

```json
{ "id": "e1", "type": "ellipse", "center": "p1", "major": [0, 1], "a": 3.24, "b": 2.15 },
{ "id": "e2", "type": "ellipse_arc", "center": "p1", "major": [1, 0], "a": 4, "b": 2,
  "start": "p2", "end": "p3" }
```

`center` is a sketch point; `major` is the direction of the semi-axis `a` in the sketch's own
frame (a unit vector, normalised on reading; only its direction counts); `a` and `b` are the
semi-axes in mm along and across it. Either may be the larger: the importer keeps them as
given and Gusset normalises when it builds the profile. An `ellipse_arc` runs
counter-clockwise (about the plane normal) from `start` to `end`, two points the solver keeps
on the ellipse (the importer does not check that they lie on it; a point off the curve is
moved onto it). Like arcs, ellipse arcs are written from `end` to `start` on a sketch whose
plane is mirrored. Inventor O-ring profiles are a whole `ellipse` revolved 360 degrees:
centre x = 7.85 mm, `major` = [0, 1], `a` = 3.24, `b` = 2.15 gives a ring of
volume `2 pi R * pi a b`.

- Gusset text: `o3 = ellipse(p1, angle, a, b)` and `q4 = ellipse_arc(p1, angle, a, b, p2,
  p3)` with `angle` in radians (the direction of `major` in the Gusset sketch) and the
  ids prefixes `o` and `q` (ADR 0004 amendment of the same date).
- A missing or non-positive `a`/`b`, or a zero `major`, skips the sketch with the reason in
  `skipped_user_text`.
- Constraints on ellipses are not in the exchange format: a source's `radius`-like
  dimensions on ellipses are left out like any unknown constraint; the geometry is imported
  as it is drawn.

## Amendment (2026-11-09): stored bodies

Some histories start from geometry the source keeps as a body, not as features: Fusion's
Base Feature (its bodies are the last N bodies of a `BREP.*.smbh` stream, ADR 0039 section
13 of the format notes) and SolidWorks' mirrored stock parts (the stock body sits in its own
Parasolid partition). Version 2 adds

```json
{"type": "stored_body", "name": "Base Feature1", "body": 0}
```

- `body` indexes the B-rep bodies a native importer hands over with the tree
  (`Tree::stored_bodies`, kernel-native bytes). It is never read from JSON: a `.gtree.json`
  file on its own cannot bring geometry in, and its `stored_body` features are skipped with
  "a stored body comes only from a native importer".
- It becomes an ordinary `import <Name> (blob: ..., format: brep)` feature, so later
  features take it as `target` (joins, cuts, fillets on its edges, whole-body mirrors and
  patterns) exactly as they take an extrude's body.
- The body is placed as the importer gives it; placement (a component's transform) is the
  importer's job.
- The trial builds during an import now evaluate with the project's blobs, so a stored
  body is visible to the features after it.
- OCCT test: a stored 40 x 20 x 10 box, a disc boss joined on its top and the body mirrored
  across x = 0 give twice (8000 + 100 pi) mm3.

## Amendment (2026-11-09): full-round fillets

A `fillet` feature (version 2) may carry `"full_round": {"point": [x, y, z], "normal": [x, y, z]}`
(`normal` optional) instead of `edges` and `radius`: the *middle face* of a full round, the face
that lies between two parallel faces (a wall's end, a slot's end) and is replaced by a half
cylinder of half their distance (ADR 0004 amendment). Exporters map their full-round blends
(Inventor: a blend whose two supports are parallel planes at twice its radius) to it, picking the
face the blend removes, i.e. the face of the model as built *before* the fillet.

- The face is found like a hole's face (a point on the surface within 0.01 mm, the normal within
  2.5 degrees when given) and written as a persistent name in `full_round: [...]`.
- `edges`, `radius` or `distance` next to `full_round`, or `full_round` on a `chamfer`, skips the
  feature with the reason in `skipped_user_text`; so does a face that is not planar between two
  parallel planar faces ("it does not build here: ...").
- The feature needs no `radius`: it is half the distance between the sides.
- A reader that does not know `full_round` has no edges and no size, so it skips the fillet.

### Amendment: `refine` (2026-11-09)

`extrude`, `revolve`, `sweep`, `loft`, `combine`, `mirror` and `pattern_linear`/`pattern_circular` features take `"refine": false` (default
true): the body-combining boolean then keeps the split faces of its result (ADR 0004 amendment,
`refine: false` in the generated text). It is written only for operations other than `new`. A
reader that does not know `refine` unifies the result, which may change later face and edge
picks of a source that was unrefined.

### Amendment: extrude up to a face (2026-11-09)

An `extrude` feature with `"end_condition": "to_face"` (default `"blind"`) ends on a face of the
model (ADR 0004 amendment): SolidWorks "up to surface" / "offset from surface", FreeCAD
`UpToFace`, Fusion "To object". Fields: `"face"` (`{"point": [x, y, z], "normal": [x, y, z]}`,
the same pick as a hole's `face`, resolved by `pick_face_name`; a point on the face as it is
before this feature, the normal optional but telling coplanar faces apart), `"offset"` (mm,
positive moves the end past the face along the sweep, negative stops short; default 0) and
`"offset_parameter"` (a named length), `"reverse"` (sweep against the source sketch's normal,
like a negative `distance`; the importer combines it with the sketch's orientation).
`distance` is ignored with a warning; `symmetric`, a missing or unresolvable face, a face behind the
profile or too small to cover it skip the feature with the reason. `operation` and `target` work
as for a blind extrude (a pocket up to a face is `"operation": "cut"`).

### Amendment: whole-body edits `move_body`, `scale_body`, `remove_body`, `draft` (2026-11-10)

Version 2 trees may carry four more feature types. All of them name bodies by the *source name*
of the feature that made or last edited them (`target`), like `combine`.

```json
{"type": "move_body", "name": "Lift", "target": "Box", "translate": [0, 0, 5],
 "rotate": {"axis": [0, 0, 1], "origin": [10, 5, 0], "angle": 90}, "copy": false}
{"type": "scale_body", "name": "Big", "target": "Box", "factor": 2.0, "center": [5, 10, 15]}
{"type": "scale_body", "name": "Stretch", "target": "Box", "factors": [2.0, 1.0, 0.5]}
{"type": "remove_body", "name": "Drop", "target": "Pin"}
{"type": "draft", "name": "Taper", "faces": [{"point": [10, 5, 5], "normal": [1, 0, 0]}],
 "plane": {"origin": [0, 0, 0], "normal": [0, 0, 1]}, "angle": 5, "flip": false}
```

- `move_body` becomes the document's `move` (ADR 0004). `translate` (mm) and `rotate` (`axis`, `origin`
  default the world origin, `angle` degrees, right-hand rule) are optional but one must be given;
  the turn is applied first, then the translation (the order of `move`). A moved body keeps its
  tree name for later `target`s, as a combine keeps its target's. With `"copy": true` the original
  stays and `name` names the new body.
- `scale_body` becomes `scale`: `factor` (positive) or `factors` (three positive), `center` in mm
  (default the origin); exactly one of `factor` and `factors`. The body keeps its name.
  Non-uniform factors are along the world axes and turn curved faces into B-splines.
- `remove_body` becomes `remove`. Every source name of that body (the original and any
  feature that edited it) is forgotten; a later `target` naming one is skipped with "its target
  'X' was removed by 'Drop'". The geometry of its earlier features stays in the history.
- `draft` becomes the document's `draft`. Faces are picked like a hole's (`pick_face_name`; all on
  one body); `plane` is the neutral plane (a base plane, an offset datum plane or a tilted one,
  generated like a mirror's); `angle` degrees as in the document (positive leans inward along the
  pull, the plane's normal; `flip` reverses the pull). Faces that are not planar, cylindrical or
  conical, or do not cross the plane, skip the feature with the kernel's reason.
- In a version 1 file these types are skipped like the other version 2 types. An angle outside
  the kernel's range, a zero factor, a missing target or an empty motion skips the feature with
  the reason in `skipped_user_text`. A reader that does not know them skips them and later
  features that depend on the changed geometry may not resolve their picks.

## Amendment (2026-11-10): helical sweeps
A `sweep` takes either `path` (a path sketch, with optional `path_curves`) or `helix`, exactly one:
```json
{"type": "sweep", "name": "Coil", "sketch": "Wire", "operation": "new",
 "helix": {"origin": [0, 0, 0], "axis": [0, 0, 1], "x": [1, 0, 0], "radius": 10,
           "pitch": 5, "height": 20, "left_handed": false}}
```
It becomes the document's `path: helix(axis: Z, pitch: 5 mm, height: 20 mm[, left: true])` (ADR 0034).
`height` or `turns` (height = turns x pitch, not both) gives the axial travel; at most 2000 turns.
The document's helix is the screw motion that carries the profile, so the solid depends on the
axis, pitch, height and hand only: `radius` (positive) and `x` (start direction, across the axis)
describe the source's helix and are checked for sense, not used (the document takes the radius
and start from the profile, which is how the profile stays where the source drew it). What the
document cannot say is skipped with its reason: an axis that is not a world axis in its positive
direction through the origin (`axis` along X, Y or Z, `origin` on that line), a non-zero
`taper_angle` (the document's helix is cylindrical), `orientation: fixed`, `path_curves` with a
helix, both or neither of `path` and `helix`, a non-positive pitch, radius or height, or unknown
fields in `helix`. Volumes: a profile in a plane through the axis gives area x 2 pi x centroid
radius per turn (Pappus), one across the axis gives area x height (tests in
`modeling_tree_import.rs`).

## Amendment (2026-11-10): import work bounds
The importer's bounds limit work on untrusted trees; the 150-design Fusion corpus
(`docs/formats/fusion-corpus-tally.md`) showed two of them cutting off real designs.
- **Profile picks**: an extrude or revolve may pick up to 4096 profile regions
  (`MAX_PROFILE_PICKS`; edges, faces, hole positions, pattern and combine lists stay at 256).
  The Perma-Proto boards pick every hole of the board (1606: 840, 723: 488). Each region's
  loops are tessellated once with a bounding box, instead of once per pick and region, so a
  pick costs one box test per region and a point-in-polygon test only where it lands; both
  boards now import as history (48 s together, debug build).
- **Evaluations**: 2000 per import (was 400). One evaluator keeps every feature built so far,
  so a trial evaluation builds only the new feature (otherwise tens of milliseconds); TFP401
  HDMI Breakout needed 423 and MEMENTO 494 (both now reach the step check, which refuses them
  for other reasons). `MAX_IMPORT_TIME` (300 s) still bounds the building itself.
- **Commit**: the import commits with the evaluator of its trial builds (and `apply` lends the
  session's own evaluator to the trials), so the commit rebuilds nothing and reports the very
  shapes the trials measured. Shapes read from equal native bytes share their volume and box
  (kernel, keyed by the bytes' hash, at most 16384 names): a Fusion import reads each stored
  body about six times. Terminal Block FeatherWing: 1219 volume/box computations for 181
  bodies became 448, its import 257 s to 87 s. What is checked is unchanged.

## Amendment (2026-11-10): stored bodies of several lumps

A stored body can be several solids apart (the copper of a board from a Fusion PCB design: one
lump per pad and track). A native importer hands such a body over as one stored body per
solid, and the reader's tree says which: `{"type": "stored_body", ..., "lump": 3, "lumps": 57}`
next to the body's place in its blob. Like `body`, the keys mean something only to the native
importer that wrote them (Fusion: `fusion_stored_bodies` converts the blob body once, refuses
it when it does not come in as `lumps` solids, and gives lump `lump` its solid); the tree
importer itself sees an ordinary stored body.

The evaluator unites the regions an extrude, revolve or sheet feature takes when they share
whole edges (`gusset-eval` `merge.rs`): a tree feature with several profile picks that lie side
by side builds one solid, as the source application does, where it built one solid per pick
before. Picks apart from each other still give one body of several solids.

## Amendment (2026-11-11): exact B-spline curves in sketches

One more sketch curve type (either format version):

```json
{ "id": "s1", "type": "bspline", "degree": 3,
  "knots": [0, 0, 0, 0, 1, 1, 1, 1],
  "poles": ["p1", "p2", "p3", "p4"],
  "weights": [1, 1, 1, 1],
  "periodic": false }
```

- `degree` 1 to 9; `poles` are ids of sketch points (in the sketch's own frame, like every
  point; a pole may repeat, e.g. a closed clamped curve lists its first pole again last);
  `knots` is the **flat** knot vector, every knot repeated by its multiplicity:
  `poles + degree + 1` values for a non-periodic curve (the curve runs from knot `degree` to
  knot `poles`, 0-based), `poles + 1` for a periodic one (period = last - first; pole `i`
  belongs to the basis function starting `degree` knots before knot `i`, as in OCCT).
  `weights` is one positive number per pole, or omitted / `[]` for a polynomial curve.
  `periodic` defaults to false.
- The curve is exact: Gusset keeps degree, knots, weights and poles and cuts the curve at its
  knots into rational Bézier spans; nothing is approximated. Each span is one edge, so an
  extruded B-spline has one lateral face per non-empty span, named `<extrude>/side(<sketch>/
  b<n>_g<span>)`.
- Gusset text: `b9 = bspline(degree: 3, poles: [p1, p2, p3, p4], knots: [...], weights:
  [...])` (+ ` periodic`); pole points keep 8 decimals, knots and weights all digits. On a
  mirrored plane only the poles are mapped.
- Bad data (a degree outside 1 to 9, too few poles, a wrong knot count, decreasing knots, an
  interior knot repeated more than `degree` times, an empty domain, a weight that is not
  positive, an unknown pole) skips the sketch with the reason in `skipped`.

What the Fusion reader should emit for its `D82E012F` records (design-stream notes section 23:
`u32 degree`, knots, weights, control points in cm): the control points as sketch points (mm,
the sketch's frame, as for every other point), then

```json
{ "id": "<curve id>", "type": "bspline", "degree": <degree>,
  "knots": [<the record's knots as stored, unchanged>],
  "poles": ["<point id of control point 0>", ..., "<point id of control point n-1>"],
  "weights": [<the record's weights; may be omitted when all are 1>],
  "periodic": false }
```

(all 69 decoded records are clamped with `knots = poles + degree + 1`, so `periodic` is false;
a record whose first and last control points coincide is a closed curve and should reuse the
first point's id as its last pole so the region closes exactly).

## Amendment (2026-11-11): body patterns whose copies stay separate bodies

`pattern_linear` and `pattern_circular` of a body take `"merge": false` (default `true`):
Fusion's and Onshape's patterns of bodies make one new body per copy. Copy `i` (1 to
`count - 1`, in the order of the pattern's instances; the original is instance 0 and keeps
its own name) is then the body `"<pattern name>[i]"` for every later `target` (extrude
join/cut, `combine` target and tools, `move_body`, `scale_body`, `remove_body`, patterns,
mirrors):

```json
{ "type": "pattern_linear", "name": "Row", "target": "Box", "direction": [1, 0, 0],
  "spacing": 20, "count": 3, "merge": false },
{ "type": "move_body", "name": "Lift", "target": "Row[2]", "translate": [0, 0, 5] }
```

Gusset writes `merge: false` and names the copies `Row_copy1`, `Row_copy2` (ADR 0004
amendment of the same date). `"merge": false` on a pattern of `features` skips it with the
reason.

## Amendment (2026-11-11): `offset_face`, and the points of a curved blend edge

Version 2 trees may carry `offset_face` (Fusion's OffsetFaces / Press Pull on faces):

```json
{ "type": "offset_face", "name": "Lower", "faces": [ { "point": [5, 5, 10], "normal": [0, 0, 1] } ],
  "distance": -1.5 }
```

- It becomes the document's `offset_face` (ADR 0030). `faces` are picked like a draft's
  (`pick_face_name`; all on one body; `normal` optional); `distance` is in mm along the faces'
  outward normals, positive adding material. The body keeps its tree name for later `target`s.
- No faces, more than 256, a zero or non-finite distance, faces on several bodies or a face the
  point does not find skip the feature with the reason. In a version 1 file the type is skipped
  like the other version 2 types.

A `fillet` / `chamfer` edge may carry `"path": [[x, y, z], ...]`, points along the source edge
from end to end, next to its `midpoint`. The midpoint decides first (within 0.01 mm). Where it
names no edge, the model's edges lying on the path stand for it, or the one edge (a line, or a
circle's arc) that holds the midpoint and both ends of the path: a source that merges edges
Gusset keeps apart (FreeCAD's refine), or a whole circle whose seam, and so whose middle, lies
elsewhere in the source than in the model (Fusion).

## Amendment (2026-11-11): faces picked by their boundary points and surface

A face pick (shell, draft, offset_face, hole, chamfer reference) may carry `points`, points of
the face's boundary (mm, at most 256), and `surface`, the surface the face lies on:

```json
{ "point": [33.5, 0, 0], "points": [[33.5, 0, 0], [0, 33.5, 0]],
  "surface": { "type": "plane", "origin": [33.5, 0, 0], "normal": [0, 0, -1] } }
```

- `surface` is `{"type": "plane", "origin", "normal"}`, `{"type": "cylinder", "origin", "axis",
  "radius"}`, `{"type": "cone", "origin", "axis", "radius", "sin", "cos"}` (the radius at height
  `h` along the axis from `origin` is `radius + h * sin / cos`) or `{"type": "other"}`; absent is
  `other`. `point` stays required (the first of `points`) and is not used when `points` is given.
- The importer takes, over all bodies, the faces that lie on the surface (every node of the
  face's tessellation within 0.001 mm of it; any face for `other`) and of them the face whose
  region holds the most of the points (by the tessellation, within 0.01 mm on a plane and the
  tessellation's deflection plus 0.01 mm otherwise). Exactly one face must hold the most and at
  least one point: none, or a tie, skips the feature with the reason. Faces of any surface type
  (revolves, blends, splines) are picked this way; a face the source moved without renaming it
  is picked by the points of all its states, of which the model holds the right ones.
