# ADR 0045: Body appearances

Status: accepted (2026-11-08)

## Context
Path-traced stills (ADR 0009 amendment) drew every body in the default grey plastic, so a
render could not show what a part is made of. Rendering is in the later tier; its next
step is materials. Bodies are named by the feature that makes them and carry no appearance
anywhere in the document.

## Decision
* A document statement, not a feature: `appearance <Body> (color: "#rrggbb", finish: …)`.
  It changes no geometry, so it stays out of the feature timeline and the evaluator, like
  `configuration`. The colour is quoted because `#` starts a comment.
* Four finishes, chosen for what parts are made of, not for a material library:
  `plastic` (diffuse under a 4 % clear coat; the old look), `metal` (reflects in its own
  colour, Schlick Fresnel, roughness 0.12), `glass` (index 1.5, Fresnel reflection or
  refraction, tinted once per pass; sun shadows pass through it tinted, not bent),
  `rubber` (diffuse only). The viewport draws the colour and shades the finish cheaply
  (metal: tinted sharp highlight and a sky/ground reflection; glass: lightened with a strong
  highlight and drawn see-through after the opaque model, alpha 0.4, without writing depth;
  rubber: no highlight); the path tracer does it properly.
* Specular paths (camera, metal, glass, coat) see the sun's disc; diffuse paths take the
  sun by next-event estimation only, so nothing is counted twice. Each path adds at most
  12 to its pixel, which removes fireflies from the small disc.
* History: appearances live in the parameter object with the parameters and
  configurations; compare and three-way merge treat them per body, like configurations.
  `feature.rename` carries them along; a missing body leaves its appearance unused.
* One command, `body.appearance {body, color?, finish?, clear?}` (GUI menu "Appearance…"
  on a feature, CLI, MCP). The MCP `render_view`, `gusset build -e x.png` and the app's
  viewport and rendered-image export all use them.

## Consequences
* Old readers refuse documents with appearances (unknown statement); documents without
  them are unchanged byte for byte.
* No textures. Per-face colours came later for STEP (amendment below). Parasolid (`.x_t`/`.x_b`) and SolidWorks imports
  keep each body's name (STEP's `\X2\` escapes decoded) as its label and its colour
  (`SDL/TYSA_COLOUR`) as a plastic appearance: the colour most of its faces have, else the
  body's own. Counting faces rather than area can pick the colour of many small faces (the
  OLED board reads lavender, its holes' colour, over its blue top). STEP/3MF colours could
  map onto appearances later. 3MF export writes the colours as one `basematerials` group
  (`displaycolor`), which slicers show, and reads them back (`pid`/`pindex` of an object) as
  appearances of the imported reference meshes; glTF/GLB writes one material per colour and
  reads a mesh's first material colour back.
  Reference meshes take appearances like bodies.
* STEP (amendment, same day): the worker reads STEP through XDE (`STEPCAFControl_Reader`)
  and keeps the same root shapes, except that an assembly root (a compound of several
  solids whose every face belongs to a solid, at most 4096) comes in as its solids. Each
  body takes its product or component name and its colour (its own, its label's, else the
  colour most of its faces have); names lose control, bidi and zero-width characters and are
  cut to 200. The colour travels from the worker on the body name after U+001F, so the
  importer interface did not change. Adafruit's LoRa bonnet: 1 grey body before, 83 named
  and coloured bodies now; reading took 2.5 s either way.
* Face colours (amendment, 2026-11-08): an appearance may list faces in other colours than
  the body's, grouped by colour: `faces: {"#d02020": [Import1/face(3), ...], ...}` (at most
  10 000 faces). The STEP reader lists, per body, the faces whose XDE colour differs from the
  body's by their index in `TopExp::MapShapes` order (the kernel's face numbering, which the
  BRep round trip to the app keeps); they travel on the body name after a second U+001F
  (`name\u{1f}#rrggbb\u{1f}3=d02020,4=...`, `StepShape::encoded_name`) and are stored by
  the import's persistent face names, so faces a later feature keeps keep their colour and
  faces it replaces lose it. The viewport, `render_view` and the path tracer draw them
  (`Scene::set_face_color`; a coloured body skips the coarse level-of-detail copy), STEP
  export writes them back as face colours, `body.appearance` keeps them when it changes the
  body's colour or finish, `clear` drops them, and renaming a feature renames them.
  `body.appearance` with `faces` colours chosen faces of any body, or with `clear` drops
  their colours; the body keeps its own colour.
  3MF export writes them on the faces' triangles (`p1` over the object's `pindex`, only when
  the body has a colour). glTF/GLB export writes a primitive per colour on the body's vertices, and its
  reader takes the colour of the primitive with the most triangles. Parasolid and SolidWorks face colours (`SDL/TYSA_COLOUR` on faces)
  are written by gusset-xt as AP214 styled items, so they reach the app the same way; the
  STEP's own body colour then wins over the converter's look (its face colours are relative to
  it). Adafruit's OLED part now shows its blue board, white silkscreen and dark screen instead
  of one lavender body. 3MF triangle colours are read too: an imported mesh's triangles are grouped by colour,
  one face per colour, kept in the stored copy (3MF instead of STL then) and drawn.
  A merge takes colour and finish apart from face colours (one side's finish and the other's
  face colours combine); deleting a feature drops the colours of the faces it made
  (docs/reviews/2026-11-08-face-colours-review.md).
* Render workspace (amendment, 2026-11-08): a RENDER tab (spec: Design / Render / Manufacture
  workspaces) holds Appearance and Render image. `export.image` (path, width, height, samples,
  view; at most 4096 px a side and 1024 samples) path-traces the model with its appearances to a
  PNG in the project folder for agents and the CLI; in the app the tool renders the current view.
  The commands crate does not depend on the renderer: hosts register one
  (`set_photo_renderer`; gusset-mcp's scene builder, the same scene as `render_view`).
  The Render tab's Photo preview path-traces the viewport in place at half resolution, in
  passes of 1, 4, 16 and 64 samples while the view stays still; a camera, model or size change
  starts over, and nothing runs once the last pass is in.

