# Onshape feature-tree exporter

**UNVERIFIED — not yet run against Onshape.** Written from memory of the public REST API
(the docs site was not reachable from the authoring session), so the items under "Things
to check" need confirming on the first run. Targets Onshape REST API **v6**. Output
format: `docs/adr/0042-feature-tree-interchange.md`.

`gusset_export.py` is a read-only dumper (Python 3, standard library only): GETs for
features, sketches, parts, mass properties and bounding boxes, plus one POST to the
FeatureScript evaluator to turn fillet/chamfer edge ids (and revolve axes that are not
sketch lines) into world coordinates. It never modifies the document.

## Setup
1. Onshape Developer Portal (dev-portal.onshape.com) -> API keys -> create a key pair.
2. `export ONSHAPE_ACCESS_KEY=...` and `export ONSHAPE_SECRET_KEY=...` (never commit them;
   the script never prints them and error messages carry only HTTP status and path).
3. `python gusset_export.py "https://cad.onshape.com/documents/<did>/w/<wid>/e/<eid>" -o out.gtree.json`

Mapping: `newSketch` (lines, circles, arcs; plane from the sketch transform; no
constraints yet), `extrude` (blind/symmetric; NEW/ADD/REMOVE/INTERSECT -> new/join/cut/
intersect), `revolve` (full or one-direction), `fillet`, `chamfer` (equal distance),
`assignVariable` -> `parameters`, `hole` (simple, counterbore, countersink, located by sketch
points; the sketch plane is the face), `linearPattern` / `circularPattern` (of features),
`mirror` (of features, planar face) and `shell` (faces and plane/axis/direction resolved with the
FeatureScript evaluator, like fillet edges). Anything else becomes `unsupported` with the feature
type; a feature that fails to convert also becomes `unsupported` with a note. Lengths in
mm and angles in degrees (literal expressions with units are trusted, else the SI
`value` is converted). Suppressed features are left out.

## Manual test (about ten minutes, a person with an Onshape account)
1. Create a public document with one Part Studio. Add a variable `#width = 40 mm`
   (Variable feature).
2. Sketch 1 on the Top plane: rectangle from (0,0), 40 x 20 mm (dimension the width
   with `#width`). Extrude 1: NEW, blind, 10 mm.
3. Fillet 1: one vertical edge (the corner at x=40, y=20), radius 2 mm.
4. Sketch 2 on the Front plane: a construction line from (0,0) up 20 mm, and a
   10 x 5 mm rectangle with its left side 5 mm right of that line. Revolve 1: ADD, full
   360 degrees, axis = the construction line.
5. Run the command above with the document URL.
6. Expected: `sample/expected.gtree.json` — same structure; ids (`p1`, `c1`), feature
   names and document name may differ. Check the units (40, 20, 10, 2), the sketch
   curves, the fillet midpoint (40, 20, 5), the revolve axis, the `width` parameter, and
   that the body volume and box are present. Report differences as issues.

## Things to check on the first run
- Shape of the `sketches` response: `transform` (assumed 4x4 row-major, metres, as
  `{"matrix": [...]}` or a bare list) and `geomEntities[].startPoint/endPoint/geometry`
  being world coordinates in metres.
- Arc direction (`geometry.clockwise` assumed relative to the sketch normal).
- Whether sketch-entity query ids in the revolve `axis` parameter equal `entityId`.
- Whether `BTMParameterQuantity.value` is SI; the script prefers the literal expression.
- The FeatureScript evaluator request body (`queries` -> `deterministicIds`) and result nesting.
- Per-part bounding boxes are not offered; `bbox_mm` is written only for single-part studios.

## Tests
`python3 -m unittest` in this directory: request signing (structure, checked against an
independent HMAC computation; the docs' worked example was not available) and conversion
of hand-written responses to `sample/expected.gtree.json`. No network.
