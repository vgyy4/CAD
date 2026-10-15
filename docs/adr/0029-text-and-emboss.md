# ADR 0029: Text, embossed and engraved

Status: accepted (2026-10-15)

## Context
Text/emboss is in the v1 tier (docs/RESEARCH.md): part numbers, labels and logos on
printed and machined parts. Fusion places sketch text and extrudes it (or uses Emboss to
wrap it onto curved faces); SolidWorks has sketch text plus Extruded Boss/Cut; Onshape has
sketch text plus Extrude; OpenSCAD has `text()` + `linear_extrude`. All of them are "text
outlines as a profile, then extrude, add or cut". Wrapping onto curved faces is a later step
everywhere it exists.

Gusset sketches are solved with constraints and cut into regions by an arrangement; glyph
outlines (hundreds of Bézier segments) would make every text a large, unconstrained part of
that machinery for no benefit: text is placed, not constrained.

## Decision
1. A feature, not a sketch entity:
   `text <Name> (text: "<string>", plane: <plane>, at: [<x>, <y>], height: <length>,
   distance: <length>, [font: sans|mono], [align: left|center|right], [angle: <angle>],
   [op: new|add|cut|intersect], [body: <Feature>], [body_name: <Body>])`, and the command
   `feature.text`. `plane` and `at` work like a sketch's plane and coordinates
   (XY/XZ/YZ, a planar face, a datum plane); `at` is the start of the baseline (its centre
   or end for `align`); `angle` turns the text about `at`. `distance` and `op` mean what
   they mean for `extrude` (a cut into a face needs a negative distance).
2. `height` is the height of capital letters (the font's cap height), the number people
   measure on a part.
3. Fonts: the two fonts the app already ships through egui, so no new binary or licence:
   `sans` = Ubuntu Light (Ubuntu Font Licence), `mono` = Hack (MIT / Bitstream Vera).
   Outlines come from `skrifa` (Google fontations, MIT/Apache-2.0, already in the tree via
   egui). Coordinates are font units scaled to mm; quadratic segments are raised to cubic
   exactly; no hinting, no kerning, one line. Missing characters are an error naming them.
4. New crate `gusset-text`: string + font + height → regions of lines and cubic Béziers
   (outer loops counter-clockwise, holes clockwise, nesting found by containment). The
   evaluator turns them into a kernel profile on the plane's frame, extrudes it and adds,
   cuts or intersects it like an extrude (`Evaluator::combine`).
5. Names: the text feature is its own "sketch". A glyph contour is `g<char>_<contour + 1>`
   (char counted from 0 in the string, spaces included), its Bézier segment `k` adds `_g<k>`:
   lateral faces `Text1/side(Text1/g0_1_g3)`; caps `Text1/cap(end)` for the first
   contour's region and `Text1/cap(end, g2_1)` for the others, like sketch regions.
6. Limits: at most 200 characters and 50 000 outline segments per feature; control
   characters are refused.

## Consequences
- Text needs no sketch; editing the string, size or position is an ordinary parameter edit.
- Not yet: other fonts (a font file would be a blob like an import, vetted the same way),
  several lines, kerning, wrapping onto curved faces (emboss on a cylinder).
