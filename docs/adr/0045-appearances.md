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
* No textures and no per-face colours. Parasolid (`.x_t`/`.x_b`) and SolidWorks imports
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
