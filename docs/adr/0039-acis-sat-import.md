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
- Not read: pattern instances, should a design store any as parameters only (none found: the
  HDMI cable's pins turned out to be ordinary bodies, see "USB-C HDMI cable" below); designs whose
  components keep only stale matrices; a moved board reference that names its role
  without a matrix (the Buck case); the 2025 layout's reference matrices (HDMI: the matrices
  found are earlier states); joints.

Bounds: 200000 records per class, 4096 instances per reference, 32 levels of nesting, 8 levels
of designs referencing designs, 4096 bodies and 256 B-rep parts per import, 4 MiB of JSON.
`read_design` (and `placements`, which calls it) is in the `sat_read` fuzz target and has a
noise test.

Measured on the samples (solids of the author's STEP with one of our bodies within 0.05 mm
by vertex box, before to after): LoRa Radio Bonnet 0 to 82 of 83; CyberDeck 11 to 20 of 62;
NeoKey Breakout 11 to 12 of 15; MacroPad Assembly 0 to 167 of 185; Keyplate 1 to 1 of 1; Pixel
Trinkey 0 to 63 of 65; QT Py 0 to 138 of 142; USB-C HDMI 0 of 54; BQ25185 Buck 0 to 65 of 90; Pico
0 of 216; Crickit HAT 20 to 48 of 85 (35 of the author's solids have no body of that size in our
import at all; of the other 50, 48 are placed).

Pico case, 2026-11-08 (first reading): the referenced `RaspberryPi Pico W v2` design (stream
version 310) holds none of the record classes above. It was thought to be placed by joints.
Re-investigated 2026-11-08 (joints): **not joints.** The root design stores a placement per
component of the referenced design, and the components are listed in the referenced design.

- Root stream (`Pi Pico Case SLA`): the reference's class `E03784ED` record (role `5c6b9519-...`,
  matrix (0, -25, 7) mm, a half turn about Z) is followed, 1465 bytes later, by a run of class
  `CE2913AA` records: three of 407 bytes with two matrices each, the second 14 bytes after the
  first (128 + 14), then the last pair repeated 76 times with 60-byte empty records between. The
  first matrix of a pair is a joint origin (not needed). The second is the component's world
  placement in the root: (3.551, -15.595, 4.593) for `BOOTSEL v3`, (0, -25.5, 3.0) for
  `RaspBerryPi Pico_CircuitBoard_logo v15`, (10.501, 25.505, 4.593) for `PicoW v1`, each exactly
  the author's STEP placement. The distinct second matrices in order of first appearance are the
  components', in the order the referenced design lists them (`fusion::component_parts`).
  (The few `DcJointOriginFeatureMetaType` records of the root hold joint origins for the case
  halves; no joint geometry was needed.)
- Referenced stream: no component or occurrence records. Component names carry a version
  (`BOOTSEL v3`, `PicoW v1`; the design's own name, `RaspberryPi Pico W v2 v2`, comes first).
  A body blob right after a name is that component's (BOOTSEL: 7 bodies; the logo: 101); any
  other blob belongs to the last component, whose record is written after the bodies it took
  over (`PicoW v1`: 74 blobs, 106 bodies). Bodies carry no owner id (checked: no id repeats).
  This grouping rests on one sample (`fusion::component_groups`) and is used only when the
  referenced design has exactly as many groups as the root stored placements.
- A body of a component takes the stored placement as its whole placement (the body blob is
  in the component's own coordinates). 0 to 213 of 216 solids placed; the three left are not
  explained.

Buck case, 2026-11-08: a reference named without a matrix (at the origin) whose board the root
moved has no repeat of the reference's matrix: the first `CE2913AA` record after it holds the
board's placement as its first matrix. That record is now read (the first one within 3000 bytes
whose matrix is not the identity): 0 to 65 of 90 solids placed; the other samples unchanged
(LoRa 82, CyberDeck 20, NeoKey 12, MacroPad 167, Keyplate 1, Trinkey 63, QT Py 138, Crickit 48).

USB-C HDMI cable, 2026-11-08: 50 of the author's 54 solids are pin copies made by Fusion
patterns (8 distinct sizes in all). Investigated and corrected: **the pin copies are stored as
bodies.** The referenced design's blob (`BREP.1ae4a36e-....smb`) holds 52 bodies (52 `body`,
`lump`, `shell` records: 24 + 20 + 4 pins and the housing parts); no pattern record, parameter or
extra matrix exists for them in either design stream (the strings `Pattern`, `Rectangular`,
`Circular`, `Quantity` appear in none of the three `BulkStream.dat`). They were missing for two other reasons:

1. The blob did not convert (`pcurve: v knot count`): a `pcurve` record carries a second subtype
   block, `{ exact_spl_sur 22800 0 nurbs <du> <dv> both 2 0 0 0 <nu> <nv> ... }`, a rational
   surface with `both` and four enumeration values (not the three words of the older layout)
   before the knot counts, and `{ ref n }` of a `spline-surface` points at it. `bs3_surface` now
   tries the counts after the four values when the plain reading fails. This also made more
   faces readable in the other samples (below).
2. The design was placed by an earlier state. The 2025 stream layout (`FusionDesignSegmentType1`)
   has the reference's matrix in a class `E03784ED` record, then the same matrix repeated in
   records with no class tag (1000 and 640 bytes later) and, 14 bytes after the matrix of the last
   repeat (128 + 14 after its start), the placement the root gave the design (a 180 degree turn
   about Z and (0, 7.7, 4.27) mm instead of (5.5, 15.46, 19.0) mm). The same shape holds for the
   second reference (a quarter turn about X: (0, 10.35, 1.78) mm, then (0.219, 9.65, 0.892) mm).
   `fusion::nested_loose` reads it when no class `CE2913AA` repeat is found, and `f3d` uses a
   reference's moved placement as the design's own when the design has no context component.
   Both references land within 0.001 mm of the author's solids.

Result: 0 to 53 of 54 (the one solid left is 24.65 mm from any body of ours; not investigated). The reader fix also lifted: BQ25185 Buck
65 to 71 of 90, Crickit HAT 48 to 54 of 85, MacroPad 167 to 168 of 185, Pixel Trinkey 63 to 64
of 65, QT Py 138 to 139 of 142; LoRa 82, CyberDeck 20, NeoKey 12, Keyplate 1 and Pico 0 are
unchanged. (The remaining solid: the author's 'Panel Body' is 25.8 x 39.5 x 31 mm, ours
25.8 x 4 x 31 mm: the stored blob is an earlier state of the body, with sheets beside it that
later features thickened; a geometry difference, not a placement one.) True pattern instances (parameters only) were not found in any sample; if a design
turns up that stores them that way, nothing reads them.

Fusion colours (2026-11-08, implemented): the findings below are read by `gusset-sat` and
`gusset-commands` and the colours reach the app through the STEP face-colour path (ADR 0045
amendment). Three links:

1. Body blob to material id: `Sat::materials` collects every `ATTRIB_CUSTOM-attrib` whose
   name is `NEUTRON_Material_attrib_def` (record `$-1 $link $owner name 1 <uuid>`), by owner
   record number. The attribute chains are not followed: Fusion's links are not in an order
   that reaches all of them, the owner field is reliable. A face's own material wins, else its
   body's.
2. Material id to asset id: `fusion::material_assets` pairs each lowercase UUID in the
   design stream's UTF-16 text with the first uppercase `<UUID>$` within 300 bytes after it.
3. Asset id to colour: each `ProteinAssets.BlobParts/ProteinAsset.*.protein` is a ZIP whose
   `AssetData/InstanceProperties.bin` has a record per asset, starting with its id in ASCII
   (not followed by `_` or a letter or digit: `<id>_Post2015` is another record). Four
   little-endian doubles r, g, b, a (sRGB in [0, 1], a exactly 1.0) sit 203 bytes after the
   id's start; if they are not valid there, the first valid quadruple before the next record is
   taken (`fusion::instance_colours`); with none the asset has no colour.

`gusset_sat::to_step_coloured` takes the material-to-`0xRRGGBB` map and writes, per distinct
colour, `COLOUR_RGB` ... `PRESENTATION_STYLE_ASSIGNMENT`, a `STYLED_ITEM('color', ...)` per
coloured face and one `MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION`, as
`gusset-xt` does; the OCCT reader's majority rule gives the body its colour. The importer
(`import.rs`, `f3d`) reads the `.protein` entries of every design (also those inside an
`.f3z`), the material pairs of the same `[Active]` streams as the placements, and builds the
map once for the file. Bounds: 4096 packages, 32 MiB per `InstanceProperties.bin` (all within
the file's decompression budget), 200,000 asset colours and 200,000 material colours, 100,000
ids per stream or package; a damaged package only loses its colours. The readers are linear
and never panic on noise (unit tests).

Checked on Adafruit's LoRa bonnet: all faces that carry a material get a colour (gold, black
header, ...).

History blobs (2026-11-08, second pass). Some body blobs have lost their material attributes
although their faces were painted: the LoRa board (`BREP.ac1167de...smb`, 1526 faces) has none,
but the history blob Fusion writes just before it in the design stream
(`BREP.dca3d57e...smbh`, same ACIS layout, parsed by the same reader) has 151 faces with
`e852ce83-...`, the material whose asset is the author's STEP colour `FBD4A9` on exactly 151
faces of the board. The faces keep their `generic_tag_attrib_def` tag (record `... 1 3 <id> 0 1
<feature> 0`) across the two blobs: all 151 tags of the history name exactly one face of the
body blob. So `fusion::history_pairs` pairs each `.smb` name in the stream with the nearest
`.smbh` name before it (within 16 KiB, no other body name between), `gusset_sat::History::read`
collects tag to material id from the history blob (a tag with two materials is dropped), and
`to_step_coloured_history` gives faces without a material of their own the one of their tag, a
face's own material winning. The history is used only when at least half of its tags name
exactly one face of the body, so a wrong pair does nothing. `.smbh` entries are read like
`.smb` ones (same budgets). Effect: 1 more of the 37 STEP files of the LoRa bonnet coloured
(14 to 15; the 151 are the board's small hole bodies, not its main plate), about one body per
design in the other samples (the board or the bodies painted before later features).

Body references (2026-11-08, third pass, implemented). The imported electronics bodies (RFM69,
OLED, the 6 mm switch: 41, 4 and 8 sub-bodies) have no material in their blob or its history; their
colours are in the `BASEFEAT_PM_BODY_REF` / `PassiveRefMetaType` records of the design stream
(60 in the LoRa bonnet). Record layout: `14 00 00 00 BASEFEAT_PM_BODY_REF`, `12 00 00 00
PassiveRefMetaType`, `01` and the record number (u32), the class id `D3937028-C20C-...` (36 ASCII
characters), `00 00 02 00 00 00` and the body's 1-based index (u32), later the colour property id
`702b9cd2-537c-429e-8cc4-22beeeb98c37`, six zero bytes and the colour as three little-endian `f32`
(sRGB, 0 to 1) and `1.0`. A feature writes its records consecutively with the indices 1, 2, ..., n;
its record is a feature record that points to each by `01` + record number a little before it names
`BREP.<h>.smbh` and then `BREP.<b>.smb`. The mapping, found by comparing face counts:
- A group of n references belongs to the history blob `h` its feature names (all n pointers must
  lead to the same `.smbh` name within 4 KiB, else the group is dropped) and so to the body blob
  `b` paired with it (`history_pairs`).
- Index k colours the k-th body of `b` that has a transform, counting from the second: the first
  such body is a 6-face helper, and `b` has two more bodies of one face without a transform, which
  the history blob lacks. The history blob has n + 1 bodies: LoRa's RFM69 blob has 43 bodies in the
  `.smb` (41 with a transform) and 41 in the `.smbh`, 40 references; the OLED 6, 4 and 3; the
  switch 10, 8 and 7. The face counts agree with the colours (RFM69: the 70-face board `48A768`,
  16 bodies of 9 faces `F3CB7C`, then 6-face bodies `4D4D4D`/`E8AD23`, one 18-face `F5F5F6`; the
  switch: 16 faces `F5F5F6`, 3 faces `191919`, ... 22 faces `191919`). The pairing is used only when
  n equals the number of those bodies exactly.
- The colour is the body's default: a face's own material or its history tag's wins
  (`gusset_sat::History::with_body_colours`, `Design::body_colours`).
- A record that stands alone (the next record has index 1 again; one per feature of 2-body blobs
  that already have materials) is not read: its colour parses as black in every case and names no
  body, so a group needs at least two records. A feature's first record (the helper body) is not
  coloured; the STEP has no colour for it either.
Effect on the LoRa bonnet (render `fusion_colours3.png`): 50 more sub-bodies coloured (RFM69 green
`48A768` with gold pins, OLED black with a blue `6CC4DA` glass, the switch white and black);
placements unchanged (LoRa 82, Crickit 48). Other designs have `BASEFEAT_PM_BODY_REF` records in a
layout without the class id and index (no colour read, nothing changes).

Not covered, and why (the author's STEP of the LoRa bonnet is the ground truth):
- The main plate of the board (10 faces, `786E46` in the STEP) is painted with a material
  named `Board` after the component's name (a UUID record with the name and the colour,
  `a1bfa42f-...`, in the design stream); no blob or record found carries that UUID, only the
  name matches. Not read: a name match is a guess.
- Pin headers, fiducials and solder jumpers (the 17,635-byte blobs): the STEP is of a later
  design version that replaced them by an other part, so there is no ground truth here.


Threads and two reader fixes, 2026-11-08. Helical surfaces occur in one sample only, an
Autodesk SMT file (`a.smt`, ASM 224, a threaded cap and two parts): two faces on a
`spline-surface { helix_spl_line ... }` with no stored approximation (`nullbs nullbs`), which
used to be left out (a body with open shells). Record, after the name: `[version] F u0 F u1
F t0 F t1 F t0 F t1 centre major minor pitch taper direction null_surface null_surface nullbs
nullbs ...`, then the surface's own parameter box. The point at `(u, t)` is the helix of
`helix_int_cur` (`centre + major cos t + minor sin t + pitch t / 2pi`) plus `u` times the
direction (0 0 -1): a land of the thread, 0.198 wide, along the axis. Checked: the corner
vertices of both faces (radii 11.882 and 10.165, pitch 3) lie on it at `u = 0` and the face
t-ranges. `step::helix_surface` writes it as a degree 1 by cubic B-spline over the helix's range
(16 segments per turn, as the helix curves; chord error under 0.001 mm), bounded by the helix
turn cap; a taper, a missing range or a non-finite number is refused. The part now converts
with all its faces (58 to 60 faces, no open shells); the orientation is healed by the importer
(flipping the surface gave identical bodies). No helical sweep appears in any Fusion sample
(searched all 255 blobs), so Fusion threads are untested.

Procedural surfaces: the stored approximation is the first `nubs`/`nurbs` that reads as a
surface (up to 8 tried): a sweep's or blend's path curve (`nubs <degree> <closure> <count>`,
no second count) can come before it and made four Fusion blobs fail (`v knot count`).
With the 2025-layout rational fix above: MacroPad Assembly 168 to 173 of 185, BQ25185 Buck 71 to
86 of 90; all other samples unchanged (LoRa 82, CyberDeck 20, NeoKey 12, Keyplate 1, Trinkey
64, QT Py 139, HDMI 53, Pico 213, Crickit 54). The three Pico solids still missed (a 42 x 56 x
10 mm case part we have as 26 x 56 x 10, a 26 x 56 x 4.2 one as 1.6 thick, a 8 x 6 x 3.5 one as
6.9 x 4.2 x 2) differ in shape, like the HDMI panel: the stored blob is an earlier state of the
body; they are not placement errors.

## Amendment (2026-11-09): the Fusion timeline, decoding in progress

A Fusion design's parametric timeline lives in the design segment's `MetaStream.dat` /
`BulkStream.dat` beside the ACIS bodies. Its framing, sketches (placement, points, lines,
arcs, circles; cm) and extrusions (operation, extent, signed distance) are decoded from the
sample bytes alone (`docs/formats/fusion-design-stream.md`; prototype in `scripts/re/fusion/`):
all 37 sample design streams frame, and 176 of 229 new-body extrusions reproduce a stored
body's box within 0.01 mm. Since then (three decoding rounds): extrusion profiles (all 387 profile records parse),
join/cut targets, timeline classification, component membership, face and edge naming
(fillet/chamfer edges where the faces are sketch-swept; 73 straight edges positively
checked against the bodies), through-all and to-object sides. Still missing: holes, mirror
planes and most pattern directions, revolve axes, edges between earlier blends, and
sweeps/shells/splits; no single-component sample design is complete yet. Until then `.f3d` stays tier 2. The
import will follow FreeCAD's rule (ADR 0033 amendment 2026-11-09): features only when they
rebuild to the stored bodies.

Round 4 (2026-11-09): holes (diameter, depth, position and face plane), pattern directions
(stored as world geometry, two-direction and symmetric extents) and mirror planes decode.
The L-Bracket (Zenodo 8028536) rebuilds exactly from its decoded timeline (13 features, the
stored volume and box), and so does the MacroPad RP2040 keyplate's component. A Rust port
behind the rebuild check comes next; revolve axes and counterbore/countersink holes wait
for samples that contain them.

## Amendment (2026-11-09, later): the Fusion timeline as features (`gusset_sat::fusion_tree`)

`.f3d` designs whose timeline the reader can say in full come in as editable features, not as
the stored bodies (tier 1: "the Fusion design timeline as editable features"). The reader is the
Rust port of the clean-room prototype (`scripts/re/fusion/`, layout notes in
`docs/formats/fusion-design-stream.md`): `gusset_sat::fusion_tree::feature_tree` frames the design
stream, reads sketches, profile regions, extrudes (new/join/cut, symmetric, through-all, to-object),
fillets and chamfers by edge midpoint, holes, linear patterns (chained, symmetric split) and
mirrors, and uses the stored ACIS blobs for edge naming (face tags) and through-all reach. It
returns the `gusset-feature-tree` JSON, or an `Err` naming the first feature it cannot say
(revolves, draft, shell, sweep, base features, patterns of bodies, two-sided extrudes, tapers,
extrudes whose profile or extent is not decoded). Reads are bounded (records, strings, depths,
stream size) and never panic; the fuzz target `fusion_design` runs it on split input.
`feature_tree_local` gives the features in each component's own coordinates (what the prototype
writes); `feature_tree` moves every component's features by its occurrence placement (sketch
planes, edge midpoints, hole and mirror geometry, pattern directions), and refuses a component
whose bodies stand in different places.

The import (`fusion_history` in `import.rs`) keeps the tree only when it rebuilds to what the file
stores, like FreeCAD, SolidWorks and Inventor histories:

* `rebuild_check` first: the features' volume within 2e-5 and box equal the sum of the stored
  solids (MacroPad RP2040 keyplate: the Board and keyplate extrudes give both bodies exactly; the
  PCB component's surfaces are not solids and are left out as before).
* A file may instead store the body of every feature step (the L-Bracket's blob holds the prism,
  the blended prism, the slotted body and the slot and hole cut tools; the finished body with its
  nine holes is not stored, the author's STL has it). Then `fusion_states_check` rebuilds each
  prefix of the timeline and requires every stored solid to be a state of the model (its volume
  after some feature) or a cut tool: some feature's volume change must equal the tool's volume
  times the number of copies the tree makes (`fusion_instances`: a cut or join extrude one, a hole
  its positions, a linear pattern its count less one times the copies of the features it repeats,
  a mirror the copies of its inputs; never derived from the volume). Every feature after the last
  stored state (L-Bracket: the hole and its three patterns, 9 copies of the 237.583 mm3 tool)
  must change the volume by exactly a stored tool times its copies, so a wrong pattern count is
  refused (unit tests in `import.rs`); a feature after the last state whose copies cannot be
  derived (a blend, a pattern of one, a new body) refuses the history with that reason. At least
  one state must be stored and the finished model must lie in the stored solids' box.

Result on the sample set (19 files): `8028536__L-Bracket.f3d` (13 features: fillets by two radius
sets, chamfer, cut through the leg, chained linear patterns, hole) and `5128 MacroPad
RP2040-Keyplate.f3d` come in as history. The Rust trees equal the prototype's for both
(`crates/gusset-cli/tests/f3d_history.rs`, `GUSSET_F3D_PROTO`). The other 17 stay exact bodies,
with the reason: no timeline or `.f3z` archives (6; the Assembly's keyplate component would need
the design inside the archive read separately), base features/PCB bodies (5), a feature of a class
not decoded (2), a pattern of bodies (1), a profile whose curves do not close or whose pieces are not
reproduced (2), a component used with several placements (the bottom plate, whose four mounting-hole
copies the tree has once). The sample test is gated on `GUSSET_F3D_SAMPLES` and takes
`GUSSET_F3D_MIN_HISTORY` as its floor.

## Amendment (2026-11-09, round 4): profile gaps, timeline groups, circular and body patterns

* Profile regions: a loop whose record omits a curve is closed by the one sketch curve that joins its open ends
  (Crickit CPX Extrude1), and a closed curve cut where it starts counts that crossing once (bq25185 USB Charger Extrude2).
  The prototype (`regions.py`) has the same two fixes.
* Timeline group `A917FE80` (the "A#FE#" of the corpus summary) is a non-geometric entry: it lists the features that follow
  it; they convert as before. CyberDeck HAT moves on to its next blocker (Sweep).
* `pattern_circular` (axis from the `F2A7590D` point and vector, `countU`, a full `TotalAngle`) and patterns of one body
  (`target`, `features: []`) are produced; both refuse a pattern whose file makes fewer bodies than its count (suppressed
  instances; the one circular sample, the Crickit Assembly's C-Pattern1, is such a case, so it stays unsupported).
  Patterns of components (MacroPad RP2040 R-Pattern1) have no tree form. The Python prototype does not do the pattern
  part; the Rust trees are authoritative for it.
* Base Feature: analysed only (`docs/formats/fusion-design-stream.md` section 13): PM/DM body pairs, DM bodies are the last
  N bodies of the post-state `.smbh`, 5 of 7 designs have them off the timeline, and the tree would need a stored-bodies
  feature (blob file, first body, count, component placement). Not converted: that needs a tree-format and
  `gusset-commands` change, which this round did not make.
* Corpus result (18 designs): 1 as history before and after; the two profile failures and the group blocker are gone, each
  design now stops at its next feature (Base Feature for the BQ25185 and NeoKey/Trinkey designs, Chamfer3 with 4 unresolved
  edges for Crickit CPX, Sweep for CyberDeck HAT, Component Insert for the `.f3z` inner designs, a component pattern for
  MacroPad RP2040).

## Amendment (2026-11-09, round 5): sweeps, component occurrences, `.f3z` roots

* Sweep `FCBB1707` is read (operation, profile loop, path curves by `crv_primary_id`; format notes section 14). A profile
  that is off the path's start plane, or one loop of a sketch with several, becomes a copy of its sketch with only
  that loop, moved along its normal onto the plane through the path's nearer end; Sweep1 of the CyberDeck HAT then
  rebuilds to the stored pin (4.735 mm3, same box). Twist and taper are not converted.
* A component stored once and placed several times (the MacroPad bottom-plate's mounting holes) no longer refuses the
  design: its features are written once per occurrence. That design now reaches the importer's step check and fails there
  (the check compares stored solids with the model's total volume and counts a cut extrude as one copy); the fix is in
  `gusset-commands` and is written up in the format notes.
* `.f3z`: no design of the six converts yet. The mapping to use (root design from `Manifest.json`) and the reason that
  still blocks them (Component Insert and Base Feature bodies live in other designs of the archive: a stored-bodies
  feature is needed) are in the format notes. `gusset_sat::fusion_tree::feature_tree_unchecked` (doc-hidden) returns the
  tree with the unread features left in, for diagnosing a design.
* Corpus: 1 of 18 as history before and after; no design was lost. Blockers now: CyberDeck HAT -> Fillet1 (swept
  faces) then Base Features; MacroPad bottom-plate -> the step check; Crickit CPX / Assembly -> Chamfer3 and a
  long chain (Draft, Shell, OffsetFaces, Move, RemoveBody, Base Feature, unrecognised extrude layouts).
