# ADR 0039: ACIS SAT import

Status: accepted (2026-11-04). PROMPT.md "Phase 3+": tier-2 native imports. ACIS is the
kernel of Fusion 360 (its `.f3d` archives hold ACIS bodies), Inventor's and SolidEdge's
exchange paths and many older CAD tools; `.sat` is its text save format.

## Context
After the Parasolid XT reader (ADR 0035), ACIS is the other kernel format a large share of
commercial models exist in. OCCT has no ACIS reader. The NIST engineering design models
repository holds about 48 500 `.sat` files (Drexel, CMU, PTC and others, ACIS 1 to 7), and for
the CMU spectrometer files also Spatial's own STEP translation of each (`.sat.stp`).

## Decision
A new crate, `gusset-sat`, reads SAT text clean-room, from those sample files only (no
vendor documentation or third-party reader code is used or committed), and writes STEP AP214
that the sandboxed worker imports, exactly as for XT:

* **Records**: three header lines, then `type fields #` records; version 7 adds an integer
  and a pointer after each record's attribute (the transform record has only the integer) and
  writes strings as `@len text`; versions 4 to 6 share one layout. Versions before 4 are
  refused.
* **Topology**: body, lump, shell (and subshells), face, loop, coedge, edge, vertex, point;
  body transforms are applied (a 3x3 matrix by rows on row vectors, translation, scale), as the
  samples show (their local geometry is centred and only the transformed bodies fit together).
* **Geometry**: planes, cylinders, cones (radius `|major| + h sin/cos` along the axis, derived
  from the cones' circular edges), lines, circles and ellipses. An edge without a curve (a
  cone's apex) becomes a vertex loop. A shell with free edges is written as an open shell
  (sheet) with a warning. Spline surfaces and curves (`spline-surface`, `intcurve-curve`),
  spheres, tori and elliptical cones are refused with their type named.
* **Limits**: records, tokens, chain lengths, faces and subshell depth are capped; the fuzz
  target `sat_read` covers the reader.
* `import.file` takes `.sat`.

### Amendment (same day): version 1 and B-spline curves
Version 1 files (1.05, 1.06; the NIST `ACIS` folder) read too: one header line, edges
without parameters (`start end coedge curve sense`), senses written `0`/`1`; 1.05 cones have
no interval. `intcurve-curve` records (`exactcur`, `surfintcur`) carry their B-spline as
`nubs|nurbs degree open|closed n (knot multiplicity)... points`, with end multiplicity
`degree`; they become STEP B-spline curves (end multiplicities raised by one), the record's
sense applied to the edge. Periodic B-splines are refused. An OCCT test closes a box with a
B-spline edge both ways round; the NIST version 1 parts (Boeing part, demo parts) and two
files with intersection curves import as solids.

### Amendment (same day): versions 2 and 3, spheres, tori, spline surfaces
Versions 2 and 3 have the three header lines and version 1's edges. Lines in front of the
first header line (a mailer's) are skipped. Tolerant entities (`tedge-edge`, `tcoedge-coedge`,
`tvertex-vertex`) are read as their base type. Spheres (`centre radius uref pole`, a negative
radius turns the normal inward) and tori (`centre axis major minor uref`) map to STEP's.
`spline-surface` data blocks are numbered in file order (each `{` not followed by `ref`), and
`{ ref n }` reuses the n-th; `exactsur` holds `nubs|nurbs du dv` words, two knot counts, both
knot vectors and the control points u major (rows along v; a part with 14 such faces closes
into one solid only in this order). Procedural kinds (`sweepsur` and the like) are read as
the B-spline approximation they store after their own data, with a warning. A wider sample of
24 more NIST files: every one with geometry imports but one with periodic B-spline curves.

## Consequences
* The OCCT tests import a generated box at its exact volume (both layouts) and, with
  `GUSSET_SAT_SAMPLES`, five CMU files at the volume of Spatial's own STEP translations.
  A survey of 29 fetched samples: every version 5 to 7 file without spline geometry imports
  as solids; one sheet body comes through as a sheet.
* Next: spline surfaces and intersection curves (the `exact`/`approx` B-spline data the
  records carry), spheres and tori once samples show their layout, then `.f3d` (a zip whose
  bodies are binary ACIS, SAB/SMB, a byte form of the same records).
