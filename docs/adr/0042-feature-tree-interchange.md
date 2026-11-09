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
