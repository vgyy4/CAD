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
24 more NIST files: every one with geometry imports. Periodic B-splines are written like open
ones (end multiplicity `degree`, first point repeated last) and read alike.

### Amendment (same day): binary ACIS and Fusion 360 archives
SAB, the binary form (`ASM BinaryFile8` in Fusion 360's `.smb` parts; read from Adafruit's
public CAD parts), is a stream of tagged values that maps one to one onto the version 7
records: integers, doubles, strings, pointers, two-valued words (0x0b the first, `forward`
or an infinite interval end; 0x0a the second), three-double positions and vectors, `{`/`}`,
type names as `0x0e` parts closed by a `0x0d` identifier, and 0x11 ending each record; the
header's doubles give the unit (Fusion: 10, centimetres). Newer versions write an integer
between a vertex's edge and point. `.sab` reads like `.sat`; `.f3d` is a ZIP whose
`Breps.BlobParts/*.smb` entries are read with the hardened archive reader (one decompression
budget, at most 64 parts; `.smbh`, the history, is skipped). Fusion's component placements are
in its own design stream and are not read: bodies come in where their B-rep stores them (a
warning says so). Tests: the generated box's binary form converts to the same STEP as its
text, and `gusset build --import box.f3d` gets its exact volume; the Adafruit parts import
with all their bodies, half of whose vertices match Adafruit's own STEP exports exactly (the
rest are moved by component placements).

### Amendment (same day): newer text, helices, partial faces, `.f3z`
Text files of the newest versions (`.smt`, 22400; subtype names followed by a version number,
`int_int_cur 22310 full nubs ...`) read as version 7. Helices (`helix_int_cur [version] F t0
F t1 centre major minor pitch taper`, a point at `t` being `centre + major cos t + minor sin t +
pitch t/2π`, checked against their edges' vertices) become C1 cubic B-splines through points
at most a sixteenth of a turn apart, with the exact tangents. A face on geometry not read yet
(helical sweeps `helix_spl_line`, tapered helices, elliptical cones) is left out with a
warning instead of failing the file: its shell comes in as a sheet. `.f3z` (a ZIP of `.f3d`
designs) reads the parts of each design within the same budget.

## Consequences
* The OCCT tests import a generated box at its exact volume (both layouts) and, with
  `GUSSET_SAT_SAMPLES`, five CMU files at the volume of Spatial's own STEP translations.
  A survey of 29 fetched samples: every version 5 to 7 file without spline geometry imports
  as solids; one sheet body comes through as a sheet.
* Next: spline surfaces and intersection curves (the `exact`/`approx` B-spline data the
  records carry), spheres and tori once samples show their layout, then `.f3d` (a zip whose
  bodies are binary ACIS, SAB/SMB, a byte form of the same records).
* Security review (2026-11-04, every finding reproduced): the converter runs in the app
  before the sandbox, so its costs are bounded there. Shells, faces, loops and coedges are
  walked once per body (a lattice of subshells was exponential; a loop or coedge reached twice
  is refused, which stopped a 1 MB file growing into 1 GB of STEP); lumps are written once over
  all bodies; a surface or curve is read once even when it fails; a B-spline surface shared
  through `{ ref n }` is written once; records are borrowed, never copied; helices have at most
  500 turns; the STEP text stops at 256 MiB (refused as too large); header lines are read to
  4 KiB; at most 10M values and 100 warnings; `.f3d` entries inflate to at most 256 MiB each;
  directions of non-finite length are refused.

## Amendment (2026-11-04): Fusion component placements

Read clean-room from sample designs (Adafruit's published Fusion files, kept outside the
repository): `Design1/BulkStream.dat` is uncompressed, with names as UTF-16LE strings. A
component lists its bodies as `BREP.<id>.smb`; its occurrence follows as a rigid 4×4 matrix
(16 little-endian doubles, row-major, last row 0 0 0 1, centimetres), the occurrence name
`<component>:<instance>` and the string `CreateComponent`. `gusset_sat::fusion::placements`
pairs each body with the first occurrence after it unless another body comes first (the
root component's bodies have none and stay put). It accepts only orthonormal matrices with
determinant +1 and translations under 100 km, reads at most 10000 occurrences, and searches
at most 600 bytes back from each name; it is in the `sat_read` fuzz target. The import
names each placed body after its occurrence and composes the placement after the body's
own ACIS transform. Not read yet: nested components (their matrices are relative to the
parent) and the placement of `.f3z` designs relative to each other (warned).

Binary B-spline surfaces (2026-11-05): a survey of all 42 bodies of the Fusion samples found
one part lost. Its binary `spline-surface` wrote the four closure/singularity words as
enumeration values (`0 0 0 0`) before the knot counts, and the reader took the first `0`
for a count. The reader now tries that layout first when four small integers follow the
degrees, then falls back to counts right after them. Binary files also name exact B-spline
surfaces `exact_spl_sur` (text: `exactsur`): read as exact, not as an approximation.

