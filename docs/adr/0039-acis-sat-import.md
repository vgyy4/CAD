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
own ACIS transform. (Nested components and the placement of `.f3z` designs relative to each
other, not read at first, are in the amendment of 2026-11-08.)

Binary B-spline surfaces (2026-11-05): a survey of all 42 bodies of the Fusion samples found
one part lost. Its binary `spline-surface` wrote the four closure/singularity words as
enumeration values (`0 0 0 0`) before the knot counts, and the reader took the first `0`
for a count. The reader now tries that layout first when four small integers follow the
degrees, then falls back to counts right after them. Binary files also name exact B-spline
surfaces `exact_spl_sur` (text: `exactsur`): read as exact, not as an approximation.

## Amendment (2026-11-08): nested components, `.f3z` references, newer streams

Layout facts read from the Adafruit samples and checked body by body against the authors' own
STEP exports (`crates/gusset-cli/tests/fusion_samples.rs` writes the STEP of every imported
body, `scripts/f3d_compare.py` puts the author's solids at their places in the assembly tree
and matches them by the box of their vertices). Nothing here comes from other projects'
readers.

Two layouts of `Design1/BulkStream.dat` occur (2025 files call it
`FusionDesignSegmentType1/BulkStream.dat`, Zstandard inside the ZIP, same record layout; its
sibling `FusionACTSegmentType1` is a table of property names, not read). Ids are UUIDs written
as UTF-16LE text (some end in `$`) and, in files since about 2023, also as class tags in front
of records (`24 00 00 00` and the 36 ASCII characters). Matrices are the same in both: 16
little-endian doubles, row-major, centimetres.

- Older layout, per component a block. Two ids and the name, the body (`BREP.<id>.smb`), then
  either the component's listed instances (for each: matrix, the 4-byte length of the text, the
  UTF-16 name `<component>:<instance>`; the gap is exactly 4 bytes in all 12 designs checked) or
  `CreateComponent` and 1 to 2.5 KB later one matrix. Listed matrices are relative to the
  component they were created in (an electronics board, `PCB Component`); the matrix after
  `CreateComponent` is that component's placement in design coordinates (`Board`, inside
  `PCB Component`, repeats its matrix, and so does every component placed identically inside
  it). So no parent link is needed: world = last such matrix before the list x listed matrix.
  That matrix is trusted only in a design that also lists instances; elsewhere it is whatever
  move the design history ended with (in `QTPy`'s root it is the reference's placement, in
  `Top Cover Slotted` a move that was undone).
- Newer layout, occurrence records. Class `FF415D89-A1DB-44AC-B824-E9A772B86ED4`: UTF-16 ids
  `parent$ own$ design$ component` (without `parent$` for an occurrence in the root), and a
  record number after a 3-digit text. The same occurrence appears in the history parts of the
  stream, with and without parent; the parent from any record and the first matrix found are
  used. Class `F9CB8717-EB86-47F2-AAA7-39E142C57A09` starts a component (its id, then its
  name); a body belongs to the last component record before it, and every occurrence of that
  component gets a copy (Crickit: six standoffs, twelve screws), the world placement being the
  chain of parents composed, outermost first. An occurrence's matrix is in class
  `2E3AC990-483D-4A55-8662-C064B976A957` (`01` and the owner's record number as u64 just before
  the class tag, matrix 43 bytes after it; the owner's occurrence record has number + 1) or in
  class `E03784ED-5E19-4E14-B9F2-3B07017018CD` after or around the occurrence's own id. No matrix
  means the origin.
- References (xrefs) of an `.f3z`. `Manifest.json` names the root design,
  `DesignDescription.json` lists the designs (`designObjects[].relativePath`) and, on the
  design that references others, `references[].relationships[]`: an object `id` and
  `metadata.neutronRole`, a UUID. In the root stream the role appears in class `E03784ED-...`
  records, with the placement matrix after the role, before it, or before an owner id and the
  role after the matrix (three variants); a reference used several times lists the other
  instances' matrices right after the first (at most 160 bytes apart; the MacroPad's twelve
  switches). The stream keeps earlier states: the record with the most matrices wins, ties go
  to the last. A record naming a role with no matrix is a reference at the origin. A referenced
  design is placed by its reference's matrix, composed with its own component placements. When
  the root moved a component inside the referenced design (the board of an electronics
  design), a record of class `CE2913AA-CFE0-4F04-9102-24424ED3BCFA` repeats the reference's
  matrix and stores the component's placement in the root's coordinates 8 to 24 bytes after it
  (Trinkey, QTPy); the design is then placed so that its board lands there.
- Not read: patterns (instances 2 to 4 of a rectangular pattern, the pins of a header, exist
  only as pattern parameters and world matrices after the pattern record); designs whose
  components keep only stale matrices (the Pico: `BOOTSEL` and `PicoW` sit 1.6 to 2.4 mm off
  because the matrix found is an earlier state); a moved board reference that names its role
  without a matrix (the Buck case); the 2025 layout's reference matrices (HDMI: the matrices
  found are earlier states); joints.

Bounds: 200000 records per class, 4096 instances per reference, 32 levels of nesting, 8 levels
of designs referencing designs, 4096 bodies and 256 B-rep parts per import, 4 MiB of JSON.
`read_design` (and `placements`, which calls it) is in the `sat_read` fuzz target and has a
noise test.

Measured on the samples (solids of the author's STEP with one of our bodies within 0.05 mm
by vertex box, before to after): LoRa Radio Bonnet 0 to 82 of 83; CyberDeck 11 to 20 of 62;
NeoKey Breakout 11 to 12 of 15; MacroPad Assembly 0 to 167 of 185; Keyplate 1 to 1 of 1; Pixel
Trinkey 0 to 63 of 65; QT Py 0 to 138 of 142; USB-C HDMI 0 of 54; BQ25185 Buck 0 of 90; Pico
0 of 216; Crickit HAT 20 to 48 of 85 (35 of the author's solids have no body of that size in our
import at all; of the other 50, 48 are placed).
