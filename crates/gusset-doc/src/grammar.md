# Gusset code-view text: grammar

The model as text. Read it with `document_get_text`, replace it with
`document_set_text` (one undo step; the whole text is validated first and a bad text
changes nothing). Everything below is what the parser accepts; each ```gusset block is a
complete document that parses and validates.

## Document

```
gusset 1                      # header, always first
units mm                      # mm or in (display unit and bare numbers), at most once
param <name> = <expr>         # parameters, any order, may refer to each other
param <name> = <expr>, min: <expr>, max: <expr>, step: <expr>   # with a range (each optional)
configuration <Name> (set: [<param> = <expr>, ...])            # a named parameter preset
sketch <Name> (<args>) { ... }
extrude|revolve|sweep|loft|fillet|chamfer|hole|import <Name> (<args>)
```

- One statement per line. `#` starts a comment: a comment on its own line(s) before a
  statement becomes that statement's note, a trailing `# text` on a `param` its comment.
- Names (parameters, features, bodies): ASCII letters, digits and `_`, starting with a
  letter or `_`, at most 64 characters. Unit names (`mm`, `in`, `deg`, `s`, ...) and
  built-ins (`pi`, `e`, `sin`, `sqrt`, ...) are not allowed. Feature names are unique.
- Features run in order; a feature may only refer to earlier ones.
- Arguments are `key: value`, separated by commas, each key at most once. Every
  feature also accepts `label: "text"` and `suppressed: true`.
- Expressions are arithmetic with units and parameter names: `40 mm`, `width / 2`,
  `2 * thickness + 1 in`, `30 deg`, `sqrt(2) * r`. Lengths: mm cm m um in ft thou;
  angles: deg rad. A bare number in a length slot uses the document unit.

## Sketches

```
sketch Sketch1 (plane: XY, next_id: 12) {
  p1 = point(0, 0)             # x, y in mm in the sketch's own u/v frame
  l5 = line(p1, p2)            # between two points
  c7 = circle(p1, 5)           # centre point, radius in mm
  a9 = arc(p1, p2, p3)         # centre, start, end: counter-clockwise from start to end
  s4 = spline(p1, p2, p3) closed   # smooth curve through the points (see "Splines")
  k10 = horizontal(l5)         # constraints
}
```

- `plane:` is `XY`, `XZ`, `YZ`, or a face reference such as `Extrude1/cap(end)`. A sketch
  point (u, v) maps to world `origin + u * x_axis + v * y_axis`; `sketch_create` and
  `query_sketch` report that frame.
- Entity and constraint ids share ONE counter per sketch and start at 1: points `p`,
  lines `l`, circles `c`, arcs `a`, constraints `k`, followed by the number (`p1`, `l5`,
  `c7`, `a9`, `k10`). `next_id` is the next unused number; it is optional in hand-written
  text and derived from the largest id when left out. Ids must be unique.
- An entity can be followed by `construction` (solved but never part of a profile):
  `p3 = point(5, 5) construction`.
- Lines, circles and arcs refer to points by id; lines that share a point are connected.
  Closed loops of lines/arcs and circles form the regions that features extrude. Curves
  may cross, touch or end on one another: the sketch is split at every crossing and each
  bounded face is a region (two overlapping rectangles give three regions, a line across
  a circle two). Dangling stubs bound nothing and are ignored; two curves that overlap
  along a stretch are an error.

### Regions

`query_sketch` lists the regions with `id`, `area_mm2`, `outer` (entities on the outer
boundary) and `at` (a point inside, in sketch mm). A region's id is its *primary entity*,
the smallest id on its outer boundary (`l5`), plus `_r1`, `_r2`, ... when crossing curves
give several regions with the same primary entity. Pass ids in `regions(...)` or the
`regions` argument of `feature_extrude`/`feature_revolve`. Ids survive dimension and
parameter edits; they can change when edits change which curves cross. An entity id that
is not itself a region id selects the one region it bounds, and fails (listing the ids)
if it bounds several. In a sketch where no curve is cut by another, an entity selects
every region whose boundary contains it, as before.

`sketch_trim(sketch, entity, x, y)` deletes the piece of a line/arc/circle around (x, y)
between the nearest crossings, keeping the new ends on the crossing curves.

### Splines

`s5 = spline(p1, p2, p3, p4)` is a smooth curve (centripetal Catmull-Rom) through at least
three ordinary sketch points; add `closed` to join the last point back to the first. The
points are normal points, so constraints (`fixed`, `coincident`, `distance`, `on_line`, ...)
apply to them; the spline itself takes no constraints and cannot be trimmed. A closed spline
is a region; an open one cuts regions like any other curve. It must not cross itself. Create
one with `sketch_add_polyline` and `spline: true`.

```gusset
gusset 1
units mm

sketch Sketch1 (plane: XY) {
  p1 = point(0, 0)
  p2 = point(10, 6)
  p3 = point(20, 0)
  p4 = point(10, -8)
  s5 = spline(p1, p2, p3, p4) closed
  k6 = fixed(p1)
}

extrude Extrude1 (profile: Sketch1, distance: 4 mm, op: new)
```

Constraints (`k<n> = kind(entities..., value)`; a value is an expression):

| kind | entities | meaning |
|---|---|---|
| `coincident` | point, point | same location |
| `on_line` | point, line | point on the infinite line |
| `on_circle` | point, circle/arc | point on the circle |
| `midpoint` | point, line | point is the midpoint |
| `horizontal`, `vertical` | line | parallel to the u / v axis |
| `parallel`, `perpendicular` | line, line | |
| `tangent` | line or circle/arc, circle/arc | |
| `equal` | line, line or circle/arc, circle/arc | equal length / radius |
| `distance` | point, point + value | Euclidean distance |
| `distance` | point, line (either order) + value | perpendicular distance |
| `distance` | line, same line + value | the line's length |
| `h_distance`, `v_distance` | point a, point b + value | signed `u(b) − u(a)` / `v(b) − v(a)` |
| `at_x`, `at_y` | point + value | the point's u / v coordinate in the sketch: drives a position by a parameter, e.g. `at_x(p5, width / 2)` |
| `angle` | line a, line b + value | directed angle, see below |
| `radius`, `diameter` | circle/arc + value | |
| `fixed` | point or circle | keep at its current position |
| `symmetric` | point, point, line | mirror images across the line |

Directed angle: `angle(a, b, v)` takes two LINES (not three points). Each line has a
direction from its first point to its second. `v` is the angle that turns the direction
of `a` onto the direction of `b`, measured counter-clockwise in the sketch's u/v frame
(the solver uses atan2(cross(a, b), dot(a, b)) wrapped to (-180, 180] degrees). So with
`l1` along +u, `angle(l1, l2, 90 deg)` has `l2` pointing along +v and `angle(l2, l1,
90 deg)` points `l2` along -v. Swapping the lines negates the angle, and reversing a
line's point order adds 180 degrees. Use `deg` or `rad` in the value.

```gusset
gusset 1
units mm

param width = 40 mm
param tilt = 30 deg

# Base outline and a sloped arm, fully described by constraints
sketch Sketch1 (plane: XY) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(0, 0) construction
  p4 = point(30, 17)
  l5 = line(p1, p2)
  l6 = line(p3, p4) construction
  c7 = circle(p4, 3)
  k8 = fixed(p1)
  k9 = horizontal(l5)
  k10 = distance(l5, l5, width)
  k11 = coincident(p3, p1)
  k12 = angle(l5, l6, tilt)
  k13 = radius(c7, 3 mm)
}
```

### Sketch patterns

```
g9 = pattern_linear(of: [l5, c7], count: n, dx: 10 mm, dy: 0 mm)
g10 = pattern_circular(of: [l5], center: p3, count: 6, angle: 360 deg)
```

A pattern repeats lines, circles, arcs and splines (not points); `count` includes the
original and must evaluate to a whole number from 2 to 500. Copy `j` (1, 2, ...) of entity
`l5` is `l5_j`, of point `p3` is `p3_j`: they are derived after solving, move with the
sources and parameters, take part in regions (`regions(l5_2)`) and extrusions, and have
lateral faces `side(Sketch1/l5_2)`. A circular pattern spaces copies `angle / count` apart
for a full turn, else `angle / (count - 1)`; its centre is an ordinary point (it can be
constrained). Copies are not solver geometry: constraints cannot refer to them. A curve
belongs to at most one pattern. Ids `g<n>` share the sketch's counter.

```gusset
gusset 1
units mm

param holes = 4

sketch Sketch1 (plane: XY) {
  p1 = point(0, 0)
  p2 = point(60, 0)
  p3 = point(60, 40)
  p4 = point(0, 40)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
  p9 = point(12, 20)
  c10 = circle(p9, 4)
  g11 = pattern_linear(of: [c10], count: holes, dx: 12 mm, dy: 0 mm)
  p12 = point(30, 20)
  p13 = point(30, 32)
  l14 = line(p12, p13) construction
}

extrude Extrude1 (profile: Sketch1.regions(l5), distance: 5 mm, op: new)
```

## Features

```
extrude  <Name> (profile: <P>, distance: <expr>, [symmetric: true], [op: new|add|cut|intersect], [body: <Feature>], [body_name: <Body>])
revolve  <Name> (profile: <P>, axis: X|Y|Z|<Sketch>.<line>, angle: <expr>, [op: ...], [body: ...], [body_name: ...])
fillet   <Name> (edges: [<edge>, ...], radius: <expr>)
chamfer  <Name> (edges: [<edge>, ...], distance: <expr>)
import   <Name> (blob: "<64 hex>", format: step|iges|stl|3mf|obj|brep, file: "name.step")
```

- Profile `<P>` is a sketch name (all its closed regions) or `Sketch1.regions(l5, c7)`
  naming the wanted regions (see "Regions" below).
- `op` defaults to `new`, which creates a body named after the feature, or after
  `body_name` when given (a unique identifier; only with `op: new`). `add`, `cut` and
  `intersect` combine with the body created by the feature named in `body:` (default:
  the latest body). A sketch on a face extrudes along the face's outward normal, so a
  `cut` into the material needs a negative distance.
- A revolve axis `X`, `Y`, `Z` is a world axis; `Sketch1.l5` is a line of a sketch.
- `fillet` and `chamfer` take persistent edge names (copy them from `query_body` with
  `names: true`; never guess them).

## References

- Face: `<Feature>/cap(start)`, `<Feature>/cap(end)`, `<Feature>/side(<Sketch>/<line id>)`;
  with several regions `cap(end, <region entity>)`; a trailing `#2` picks one of several
  split faces.
- Edge: `edge(<face>, <face>)`, e.g.
  `edge(Extrude1/cap(end), Extrude1/side(Sketch1/l6))`, optionally followed by `#2`.

```gusset
gusset 1
units mm

param width = 40 mm
param depth = width / 2
param wall = 4 mm

sketch Sketch1 (plane: XY) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: wall, op: new, body_name: Plate)

# a hole through the plate, sketched on its top face
sketch Sketch2 (plane: Extrude1/cap(end)) {
  p1 = point(20, 10)
  c2 = circle(p1, 4)
}

extrude Extrude2 (profile: Sketch2, distance: -wall, op: cut, body: Extrude1)

fillet Fillet1 (edges: [edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5))], radius: 1 mm)

chamfer Chamfer1 (edges: [edge(Extrude1/cap(start), Extrude1/side(Sketch1/l7))], distance: 0.5 mm)
```

Revolve, with a sketch line as the axis:

```gusset
gusset 1
units mm

sketch Sketch1 (plane: XZ) {
  p1 = point(0, 0)
  p2 = point(10, 0)
  p3 = point(10, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

revolve Revolve1 (profile: Sketch1, axis: Sketch1.l8, angle: 360 deg, op: new, body_name: Pin)
```

## Sweep and loft

```
sweep <Name> (profile: <P>, path: <path>, [orientation: frenet|fixed], [twist: <angle>], [op: ...], [body: ...], [body_name: ...])
loft  <Name> (profiles: [<P>, <P>, ...], [ruled: true], [op: ...], [body: ...], [body_name: ...])
```

- `sweep` moves a closed profile along a path. `<path>` is a sketch (`Path`: all its curves must
  form one chain), some of its curves (`Path.curves(l3, a4)`), or edges of an earlier body
  (`[edge(...), edge(...)]`, in any order). One end of the path must lie in the profile's plane
  (the sweep starts there); a closed path (a circle) makes a ring without caps. Where path
  curves meet at an angle the corner is mitred.
- `orientation: frenet` (the default) turns the profile with the path; `fixed` only slides it.
  `twist` turns the profile about the path by that angle (at most 100 turns) between the start and the end (it
  needs `frenet` and an open path); a helical gear is a sweep of the tooth outline along a
  straight line with a twist.
- `loft` joins two or more closed profiles on different planes (a sketch on a datum plane per
  profile; `Sketch.regions(l5)` picks the region). Each must select exactly one region without
  holes. `ruled: true` joins neighbours with straight (ruled) surfaces, else one smooth
  surface passes through all of them.
- Faces: both give `<Feature>/cap(start)` and `cap(end)`. Sweep sides are
  `<Feature>/side(<profile sketch>/<entity>)`, plus `_p1`, `_p2`, ... on the entity when the
  path has several edges (`side(Profile/c2_p1)` is the second path edge). Loft sides are named
  after the profile edge they come from, `side(<sketch>/<entity>)` of the first profile that has
  one.

```gusset
gusset 1
units mm

param len = 20 mm

sketch Profile (plane: XY, next_id: 9) {
  p1 = point(-5, -5)
  p2 = point(5, -5)
  p3 = point(5, 5)
  p4 = point(-5, 5)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

# the path starts at the profile's plane (z = 0) and goes up
sketch Path (plane: XZ, next_id: 7) {
  p1 = point(0, 0)
  p2 = point(0, 20)
  l3 = line(p1, p2)
  k4 = fixed(p1)
  k5 = vertical(l3)
  k6 = distance(l3, l3, len)
}

sweep Sweep1 (profile: Profile, path: Path, orientation: frenet, twist: 90 deg, op: new)

plane Up (base: XY, offset: len)

sketch Top (plane: Up, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 4)
}

loft Loft1 (profiles: [Profile, Top], ruled: true, op: new, body_name: Funnel)
```

## Datum planes, bodies and patterns

```
plane  <Name> (base: <plane>, offset: <length>)
plane  <Name> (base: <plane>, axis: X|Y|Z|<edge>, angle: <angle>)
move   <Name> (translate: [<x>, <y>, <z>], axis: X|Y|Z|[x, y, z], angle: <angle>, [through: [x, y, z]], [copy: true], [body: <Feature>])
combine <Name> (tools: [<Feature>, ...], [op: add|cut|intersect], [body: <Feature>])
shell, draft, offset_face, mirror, pattern_linear, pattern_circular   # whole-body operations (see below)
text   <Name> (text: "<string>", plane: <plane>, at: [<x>, <y>], height: <length>, distance: <length>, [font: sans|mono], [align: left|center|right], [angle: <angle>], [op: ...], [body: ...], [body_name: ...])
```

- `<plane>` is `XY`, `XZ`, `YZ`, a planar face reference or the name of an earlier `plane`.
  A datum plane creates no body. A sketch or a mirror can use it: `plane: Plane1`.
- `offset` moves the plane along its normal (XY: +Z, XZ: -Y, YZ: +X; a negative value goes
  the other way). `axis` + `angle` turns the base plane about that axis (right-hand rule);
  X, Y, Z pass through the origin, an edge reference must be a straight edge and points
  towards +X (else +Y, else +Z).
- `move` turns a body about `axis` (through `through`, default the origin), then translates
  it. With `copy: true` the original stays and the moved copy is a new body named after the
  feature (its faces: `Move1/copy(<face>)`); otherwise the body moves and keeps its names.
  `body:` defaults to the latest body.
- `combine` merges the tool bodies (named by the feature that created them) into the target
  with a boolean operation and consumes them: a tool cannot be used afterwards. `body:`
  defaults to the latest body that is not a tool. This is how one feature is patterned: make
  it as its own body (`op: new`), pattern that body, then `combine` it into the base part.
- `shell (faces: [<face>], thickness: <length>)` hollows a body; `mirror (plane: <plane>,
  [merge: true])` copies it across a plane; `pattern_linear (direction: X|Y|Z|[x, y, z],
  count: <n>, spacing: <length>)` and `pattern_circular (axis: ..., [through: [x, y, z]],
  count: <n>, angle: <angle>)` repeat a body. All take an optional `body:`.
- `draft (faces: [<face>], plane: <plane>, angle: <angle>, [flip: true])` tapers faces for
  molding: each face turns by `angle` about its line on the neutral `plane`, leaning inward
  along the pull direction (the part narrows that way; a negative angle leans it outward).
  The pull direction is the plane's normal (XY: +Z), or into the body for a face of it;
  `flip: true` reverses it. Faces must be planar, cylindrical or conical and cross the
  plane; they keep their names.
- `offset_face (faces: [<face>], distance: <length>)` pushes or pulls faces along their
  normals (direct edit, ADR 0030): positive adds material, negative removes it; the
  neighbouring faces stretch to follow and every face keeps its name. A cylindrical hole's
  wall moved by `distance` changes its radius by that much (outward from the material).
- `text` embosses or engraves one line (at most 200 characters) on a plane: `at` is the start
  (or centre / end, with `align`) of the baseline in the plane's coordinates, `height` the
  height of capital letters, `angle` turns it about `at`; `distance` and `op` work as for
  `extrude` (engrave into a face with `op: cut` and a negative distance). Fonts: `sans`
  (Ubuntu Light) and `mono` (Hack). Faces: `Text1/cap(end)`, sides
  `Text1/side(Text1/g<char>_<contour>_g<segment>)`.

```gusset
gusset 1
units mm

param pitch = 12 mm

sketch Sketch1 (plane: XY) {
  p1 = point(0, 0)
  p2 = point(60, 0)
  p3 = point(60, 30)
  p4 = point(0, 30)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new, body_name: Base)

plane Plane1 (base: Extrude1/cap(end), offset: 0 mm)

# one pin, patterned, then united with the base
sketch Sketch2 (plane: Plane1) {
  p1 = point(10, 10)
  c2 = circle(p1, 3)
}

extrude Extrude2 (profile: Sketch2, distance: 8 mm, op: new, body_name: Pin)

pattern_linear Pattern1 (body: Extrude2, direction: X, count: 4, spacing: pitch)

combine Combine1 (body: Extrude1, tools: [Extrude2], op: add)

move Move1 (body: Extrude1, translate: [0 mm, 0 mm, 5 mm], copy: true)
```

## Joints

```
joint <Name> (type: rigid|revolute|slider|cylindrical|pin_slot|planar|ball, body: <Feature>, [with: [<Feature>, ...]],
              at: <frame>, to: <frame>, [angle: <angle>], [distance: <length>], [x: <length>, y: <length>],
              [angles: [<rx>, <ry>, <rz>]], [flip: true], [offset: <length>],
              [angle_limits: [<lo>, <hi>]], [distance_limits: [<lo>, <hi>]],
              [free: [angle|distance|x|y|angles, ...]])
```

- A joint places the body `body` (and the bodies in `with`, carried rigidly): its frame
  `at` is brought onto the frame `to` with the joint's motion in between. `to` is on a body
  that is already in place (earlier in the timeline), a plane, or `origin`.
- Frames (`<frame>`): a planar face (its center, Z out of the face), a cylindrical, conical
  or toroidal face (on its axis, Z along it), a spherical face (its center), `edge(...)`
  (a circle: its center and normal; a straight edge: its midpoint and direction), `XY`,
  `XZ`, `YZ`, a datum plane, or `origin`. X is the world X axis projected onto the frame.
- Motion: revolute `angle` about Z; slider `distance` along Z; cylindrical both; pin_slot
  `angle` about Z and `distance` along X; planar `x`, `y` and `angle`; ball `angles` about
  X, Y and Z; rigid none. Values not used by the type are refused.
- `flip: true` reverses the target frame's Z (put a face against a face); `offset` moves
  along the target frame's Z. Values outside `angle_limits` / `distance_limits` fail the
  joint. Values can be parameters: setting the parameter moves the mechanism.
- Relations are expressions: a gear pair turns together with `angle: turn` on one joint
  and `angle: -turn * 20 / 40` on the other (teeth 20 and 40); a rack moves
  `distance: turn / 360 deg * pi * 40 mm`; a screw `distance: turn / 360 deg * 1.5 mm`.
- Closed loops (linkages): a joint whose `body` an earlier joint already placed (or
  carried) closes a loop and moves nothing itself. The values listed in `free` (on any
  joint of the timeline so far) are then solved so that every closing joint meets; a
  written value is the starting guess, which picks the assembly mode. A four-bar with a
  driven crank needs `free: [angle]` on the other three pivots. A loop that cannot close
  fails the closing joint with the gap; limits apply to solved values too. `query.joints`
  reports the values as built (`built_values`).

```gusset
gusset 1
units mm

param open = 45 deg

sketch Box (plane: XY) {
  p1 = point(0, 0)
  p2 = point(20, 0)
  p3 = point(20, 10)
  p4 = point(0, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Base (profile: Box, distance: 8 mm, op: new)

extrude Lid (profile: Box, distance: 2 mm, op: new)

joint Hinge (type: revolute, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true, angle: open, angle_limits: [0 deg, 110 deg])
```

## Components

```
component <Name> (source: "<path>", version: "<node>", document: "<hash>", [branch: "<branch>"],
                  [body: <Body>], [configuration: <Name>], [set: [<param> = <expr>, ...]])
```

- A component brings in one body of another Gusset document (`body`, default: its last
  body), at that document's own coordinates; place it with a joint. Insert it with
  `assembly.insert_component`, never by hand: `document` is the hash of a stored copy of
  that document, so the component stays the same until `assembly.update_component` pins
  the source's newer version (of `branch`, default: its main branch). The stored copy is the whole
  source document, its notes, labels and comments included. A source inside the project
  folder is stored relative to it.
- `set` gives parameters of the component's document values computed here (expressions
  of this document's parameters): two components of one part can have different sizes,
  and a parameter here can drive a part (`assembly.configure_component`).
- Its faces are named `<Name>/part(<face in the component's document>)`, for example
  `Wheel/part(Extrude1/cap(end))`; `query_body` lists them.
## Holes

```
hole <Name> (face: <face>, points: <Sketch> | at: [e1: [<u>, <v>], ...], [type: simple|counterbore|countersink|tapped],
             diameter: <length>, depth: <length> | through: true, [point_angle: <angle>],
             [cbore_diameter: <length>, cbore_depth: <length>], [csink_diameter: <length>, csink_angle: <angle>],
             [thread: "M6", pitch: <length>, thread_depth: <length>])
```

- One `hole` makes a hole at every site, cutting the body that owns `face` (a planar face; the
  holes go into the material, against the outward normal). Sites are the points of a sketch
  (`points: Sketch2`: every point that is not construction geometry and not an end of a line,
  arc or spline, plus the centre of every circle, whose size is ignored; pattern copies count;
  the sketch must be parallel to the face and its points are projected onto it), or explicit
  `(u, v)` lengths in the face's own sketch frame (`at: [e1: [10 mm, 5 mm], e2: [...]]`, the
  frame a sketch placed on that face gets). Each point carries its id (`e1`, `e2`, ...), which
  names its hole's faces; deleting or reordering points keeps the ids of the others. Give a
  new point an id the list does not use yet. A bare `[u, v]` (files from before the ids) is
  `e<position in the list>`. A hole must lie within the face.
- `depth` is measured from the face to the end of the full-diameter part; a blind hole adds the
  drill point (a cone of `point_angle`, default 118 deg; 180 deg is flat). `through: true` cuts
  all the way through with a flat end.
- `counterbore` needs `cbore_diameter` (larger than `diameter`) and `cbore_depth` (less than
  `depth`); `countersink` needs `csink_diameter` and the full cone angle `csink_angle` (90 deg
  for ISO 10642); `tapped` is modelled at the tap-drill `diameter` and records a cosmetic thread
  (`thread` designation, `pitch`, `thread_depth` from the face, default the whole hole) that
  `query_body` lists under `holes`; there is no helical geometry. Other combinations are errors.
- Faces of hole `p5` (the id of its sketch point, or the `e<n>` id of an `at:` point):
  `Hole1/wall(p5)` (the full-diameter cylinder), `Hole1/floor(p5)` (drill-point cone, or the flat
  end), `Hole1/cbore_wall(p5)`, `Hole1/cbore_floor(p5)` (the step), `Hole1/csink(p5)`. A wall cut
  into several faces by the material gets `#1`, `#2`, ... The face the hole starts on keeps its name.
- `feature_hole` with a `standard` such as `"ISO M6 clearance normal"` fills the dimensions from a
  built-in table and writes them as ordinary parameters (`hole1_diameter`, ...) that the statement
  refers to. Like the gear generator there is no `standard` in the text: the statement only holds
  expressions, so edit those parameters (or the text) to change a preset hole.

```gusset
gusset 1
units mm

param hole1_diameter = 6.6 mm   # ISO 273 clearance for M6, normal series
param hole1_cbore_diameter = 11 mm
param hole1_cbore_depth = 6.5 mm

sketch Sketch1 (plane: XY) {
  p1 = point(0, 0)
  p2 = point(60, 0)
  p3 = point(60, 40)
  p4 = point(0, 40)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: 12 mm, op: new, body_name: Plate)

sketch Sketch2 (plane: Extrude1/cap(end)) {
  p1 = point(15, 20)
  p2 = point(45, 20)
}

hole Hole1 (face: Extrude1/cap(end), points: Sketch2, type: counterbore, diameter: hole1_diameter, through: true, cbore_diameter: hole1_cbore_diameter, cbore_depth: hole1_cbore_depth)

hole Hole2 (face: Extrude1/cap(end), at: [e1: [10 mm, 5 mm], e2: [50 mm, 35 mm]], type: tapped, diameter: 5 mm, depth: 9 mm, point_angle: 118 deg, thread: "M6", pitch: 1 mm, thread_depth: 7 mm)

hole Hole3 (face: Extrude1/cap(end), at: [e1: [30 mm, 8 mm]], type: countersink, diameter: 4.5 mm, depth: 10 mm, csink_diameter: 9 mm, csink_angle: 90 deg)

fillet Fillet1 (edges: [edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5))], radius: 1 mm)
```

## Generated features (gears)

There is no `gear` statement. `generate_gear` (module, teeth, pressure_angle, face_width, bore,
shift, backlash, fillet, helix, name; every option has a default, units are required: `2 mm`,
`20 deg`) writes ordinary statements, so read the result with `document_get_text` and edit
parameters or text like anything else:

* `param gear1_module = 2 mm  # normal module`, `gear1_teeth`, `gear1_pressure_angle`,
  `gear1_face_width`, `gear1_bore`, `gear1_shift`, `gear1_backlash`, `gear1_fillet` (and
  `gear1_helix` when helical), then derived ones (`gear1_pitch_diameter`, `gear1_base_diameter`,
  `gear1_tip_diameter`, `gear1_root_diameter`, ...). Change these, not the sketch: the tooth
  flanks follow them.
* `sketch Gear1_profile`: one tooth in polar coordinates: construction lines from the centre,
  each flank point placed by a `distance` and an `angle` constraint whose values are expressions of
  the gear parameters, two splines per flank, a tip arc, a root arc, a `pattern_circular(of: [...],
  center: p1, count: gear1_teeth, angle: 360 deg)` for all teeth and the bore circle with
  `diameter(c, gear1_bore)`.
* `extrude Gear1 (profile: Gear1_profile.regions(a5), distance: gear1_face_width, op: new)` for
  a spur gear. A helical gear (helix not 0) also has `sketch Gear1_path` (a vertical line of the
  face width on XZ) and `sweep Gear1 (profile: Gear1_profile.regions(a5), path: Gear1_path,
  orientation: frenet, twist: gear1_face_width * tan(gear1_helix) / (gear1_pitch_diameter / 2) *
  1 rad, op: new)`.
* Gears mesh when their modules match, their centre distance is `m (z1 + z2) / 2` (`m / cos(helix)`
  for helical gears), helical pairs have opposite hands (helix angles of opposite sign) and an even
  tooth count turns half a pitch (`move_body` with an axis and angle `180 deg / z`).
* The profile sketch carries `gear: gear1` (`sketch Gear1_profile (plane: XY, next_id: 412, gear:
  gear1)`): on every rebuild the gear's inputs (`gear1_module`, `_teeth`, `_pressure_angle`,
  `_shift`, `_backlash`, `_fillet`, `_bore`, and `_transverse_module` for the helix) are checked as
  `generate_gear` checks them. Parameters that cannot make a gear (undercut, pointed teeth, a
  fillet that folds over, a bore larger than the root circle, a tooth count that is not whole)
  fail the sketch with error code `invalid_gear` and the generator's hint, e.g. "use at least N
  teeth, or a profile shift of about X or more". These parameters count as used by the sketch
  (they cannot be deleted while it has the key). Remove `gear: ...` to turn the check off.

## Common mistakes

- A header other than `gusset 1`, a second `units`, a duplicate feature or id.
- Sketch contents on the same line as the opening `{`; the closing `}` must be alone.
- `angle` with points instead of lines, or with a bare number where degrees are meant.
- `body:` names the feature that created the body (`Extrude1`); `body_name:` names the
  body a new extrude or revolve creates.
