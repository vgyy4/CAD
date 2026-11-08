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
  `rubber` (diffuse only). The viewport draws the colour; the finish matters in the path
  tracer only.
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
  map onto appearances later.
