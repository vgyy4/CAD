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
sketch <Name> (<args>) { ... }
extrude|revolve|fillet|chamfer|import <Name> (<args>)
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

## Datum planes, bodies and patterns

```
plane  <Name> (base: <plane>, offset: <length>)
plane  <Name> (base: <plane>, axis: X|Y|Z|<edge>, angle: <angle>)
move   <Name> (translate: [<x>, <y>, <z>], axis: X|Y|Z|[x, y, z], angle: <angle>, [through: [x, y, z]], [copy: true], [body: <Feature>])
combine <Name> (tools: [<Feature>, ...], [op: add|cut|intersect], [body: <Feature>])
shell, mirror, pattern_linear, pattern_circular   # whole-body operations (see below)
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

## Common mistakes

- A header other than `gusset 1`, a second `units`, a duplicate feature or id.
- Sketch contents on the same line as the opening `{`; the closing `}` must be alone.
- `angle` with points instead of lines, or with a bare number where degrees are meant.
- `body:` names the feature that created the body (`Extrude1`); `body_name:` names the
  body a new extrude or revolve creates.
