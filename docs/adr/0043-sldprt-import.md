# ADR 0043: SolidWorks .sldprt import

Status: accepted (2026-11-08). PROMPT.md "Phase 3+": tier-2 native imports. A SolidWorks part
carries its exact geometry as a Parasolid partition; the Parasolid reader exists (ADR 0035), so
the missing piece is the container.

## Context
`docs/research/interop.md` 2.2 expected "a container reader that extracts the embedded XT
partition". There is no public specification of the container. Owner rule: clean room. The
layout below was derived from the sample bytes only (blussyya's controlled SolidWorks 2011 and
2022 test models with the STEP SolidWorks exported for each, NIST's SolidWorks 2018 parts and
nine third-party parts from modeller 17.1 to 36.1) with `python3 -I` scripts that never execute
anything from a sample. No other SolidWorks reader, format write-up or SDK was read. The
OLE2 compound file layout is Microsoft's published [MS-CFB].

## Decision
A new crate `gusset-sldprt` returns the partition as an XT transmit file;
`import.file` hands it to `gusset_xt::to_step` and the sandboxed STEP importer, exactly like
`.x_t` (`Kind::Sldprt` in `gusset-commands/src/import.rs`). Tier note: `TIER_XT`.

### Layout facts learnt
* **Two containers**, told apart by the first bytes.
  * *OLE2 compound file* (`d0 cf 11 e0 a1 b1 1a e1`; seen with modeller 17.1, 22.1 (SolidWorks
    2011) and 25.1). The stream `Config-<n>-Partition` holds the partition.
    Streams under 4096 bytes live in the mini stream as the specification says.
  * *Chunk container* (everything from modeller 28.1 on in our samples, up to 36.1; SolidWorks 2022 is 33.1): 4
    varying bytes, `00 00 00 04`, then a sequence of stream records. A record is the marker
    `14 00 06 00 08 00` plus 4 bytes that are constant within one file but differ between
    files, a CRC-32 of the inflated data, the deflated size, the inflated size, the name
    length (all u32 little endian), the name with the two nibbles of every byte swapped
    (`37 77 85 ...` is `swXmlContents/...`) and the data as a **raw deflate** stream. The
    stream is `Contents/Config-<n>-Partition`.
  * The chunk file also holds look-alike records: small ones near the start whose size fields
    follow no rule (`A = 4B`, `C = 2B`) and a directory at the end with plain names. Only
    records whose name matches, which inflate to exactly the stated size and whose CRC-32
    matches are accepted; of several valid ones the last wins.
* **Partition stream** (inflated chunk data, or the OLE2 stream): a list of blocks, each `u32 L`,
  a 16-byte tag (constant `231dd571da8148a2a85898b21b89ef99`), `u32` inflated size, `u32`
  zlib size, the zlib stream, and padding up to `L` (`L = 24 + zlib size + 8`). The first block
  inflates to a Parasolid *neutral binary partition* (`PS\0\0\0?: TRANSMIT FILE (partition)
  created by modeller version ...`, key `SCH_<modeller>_<schema>_13006`, an embedded schema
  on the V13 base), the second to the partition's undo deltas (`(deltas)`), which are ignored.
  `Config-<n>-GhostPartition` is the same for the hidden configuration state; ignored.
* **Several configurations** (`Config-0`, `Config-1`, ...): the lowest-numbered one with a
  partition is imported, the others produce a warning.
* **Parasolid side** (`gusset-xt`): the edit scripts of all SolidWorks files copy 23 BODY fields,
  one more than schema 12006 has; the byte stream shows it is a byte after `body_type`
  (`nom_geom_state`). `schema::embedded_base()` adds this to the V12 table that stands in for the
  V13 base. With it the partitions of all 23 samples decode (22 hold a solid), which also settles the
  "embedded deltas unverified" item of ADR 0035. Lengths are metres, as in every XT file (`gusset-xt` writes millimetres).

### Limits (untrusted input)
At most 8192 streams/records (marker candidates 16x that), 256 MiB per inflated stream, 512 MiB
in total, inflated size at most 2048x the deflated size, sector chains walked with a step bound
(loops are errors), the stream list and DIFAT bounded by the file size; `miniz_oxide`'s limit
variants cap every inflate. A mutation/truncation test and a fuzz target (`sldprt_read`) check
that noise never panics.

### Not read
Feature tree and history (`Contents/Config-<n>-ResolvedFeatures`, `swXmlContents/Features`,
`Definition`), sketches, dimensions and equations, PMI, materials and custom properties,
colours and display lists, the preview image. Drawings
(`.slddrw`) are not read; assemblies are read as a component list (amendment below). Parts whose final partition is empty keep their bodies in
`Config-<n>-FeatureBodies/LocalBodies` (one imported-geometry part in our samples,
`Ecran_128x32oled`): see the amendment below.
Files from before modeller 17 (SolidWorks 2008 and older) were not available to test.

## Consequences
* Results (OCCT, `crates/gusset-cli/tests/sldprt_samples.rs`, env `GUSSET_SLDPRT_SAMPLES`):
  the 12 blussyya pairs (SolidWorks 2011 OLE2 and 2022 chunk; cube, cube with hole, shell,
  torus, loft with splines, cylinder with fillet) agree with SolidWorks' own STEP to a
  relative volume error below 1e-15 and 0.0000 mm bounding box difference. The other 10 parts import
  as one solid each, `Ecran_128x32oled` as 114 bodies (amendment below). NIST's `ftc_06` differs from NIST's
  AP242 file by 0.3 % in volume and 2.54 mm in the box (two revisions of the model, not a reader
  difference), so the NIST pair is informational.
* A rolling-ball blend and a self-intersecting torus in one part (`psu`) are skipped with the
  XT reader's warnings, as for `.x_t`.
* The samples are CC BY-SA / MIT data and are not in the repository; tests build synthetic
  containers (OLE2 writer and chunk writer in `crates/gusset-sldprt/src/tests.rs`).

## Amendment (2026-11-08): bodies of an imported feature

A part whose geometry comes from an imported feature (`Importée1` in `Ecran_128x32oled`)
writes an empty final partition (268 bytes, no body) and keeps the bodies in
`Config-<n>-FeatureBodies/LocalBodies`. That stream is a `u32` body count, then per body a
short name record (`ff fe ff`, a length byte, the UTF-16 name, three `u32`s) and one block of
the partition's kind: a length, a 16-byte tag shared by every block of the stream, the inflated
and deflated sizes and a zlib stream holding a binary Parasolid transmit with one body. The
reader finds the first block by its shape (the length covers the header and the data, which
starts `78`), then each next one by the shared tag after the name record; every block is
inflated under the same budget as the partition, at most 4096 bodies. The import uses these
bodies only when the final partition has none. `Ecran_128x32oled`: 114 distinct bodies (47
pins share their size), 1545.995 mm3 in a 34.5 x 20.6 x 3.3 mm box, with no STEP of its own
to compare against.

Only one `LocalBodies` stream is read (a part with several imported features keeps the others; the
chunk container takes the lowest name, OLE2 the first directory entry; security review T3).

## Amendment (2026-11-08): assemblies (`.sldasm`)

Sample: NIST's single-die nanocalorimeter assembly (SolidWorks 2018, public domain) with its
three parts. Derived from the bytes only, with `python3 -I` scripts, as above.

### Layout facts learnt
* An assembly is the same chunk container (`69 ce 88 ca 00 00 00 04`). Next to the part streams'
  siblings it holds plain XML streams (`swXmlContents/COMPINSTANCETREE`, `docProps/*.xml`,
  `[Content_Types].xml`) and binary ones (`Contents/Config-0`, `Contents/Config-0-MatesList`,
  `Contents/DisplayLists`, ...). There is no `Config-<n>-Partition`: the geometry lives in the part files.
* `swXmlContents/COMPINSTANCETREE` holds everything needed to place parts: `swFile` elements
  (`id`, `swDocType` = `PART`/`ASSEMBLY`, `swPath` = the full path of the file *where the author
  saved it*, with backslashes), `swModel` elements (`id`, `swName`, `swConfigurationName`, `swFileRef`
  = a `swFile` id, `swBoundingBox` = six numbers, min then max, in the part's own space) and, inside
  the root `swModel` (the one no other model uses), one `swReference` per component instance:
  `swName` + `swReferenceNumber` (instance `Name-1`), `swModelRef`, `swSuppressed`, `swHidden`,
  `swIsVirtualComponent`, and `swTransform`, sixteen numbers.
* `swTransform` is a row-vector 4x4: `1 0 0 0 / 0 1 0 0 / 0 0 1 0 / tx ty tz 1`, translation in the last
  row, `p' = p M`. **Lengths are metres** (bounding boxes of 0.014 for a 14 mm die).
* A component made inside the assembly (`swIsVirtualComponent="YES"`, name `Part1^<assembly>`) keeps
  its part in the stream `VirtualComp/Part1` (see the second amendment); the stored path is a temp folder.
* The file names in `swPath` are the author's; our sample files were downloaded with spaces turned
  into `_`, so the importer matches a name exactly, then ignoring case, then treating space and
  `_` alike.

### Evidence for the placements
The assembly's own bounding box (`-0.13551042 0.03572588 -0.02168140 / -0.12153442 0.04106688
-0.02118140`) equals the die layer's part box plus its translation `(-0.06819776, -0.01923062,
-0.02168140)` on all six values. The imported bodies (OCCT, `crates/gusset-cli/tests/sldasm_sample.rs`,
env `GUSSET_SLDASM_SAMPLES`) land at: die z -21.6814 .. -21.1814 mm (0.5 mm thick, 13.976 x 5.341 mm),
silicon nitride layer z -21.6814 .. -21.6813 (rotated by its matrix, same footprint as the die) and
top metal layer z -21.6815 .. -21.6814 inside the die outline: the layers stack on the die's underside
as the assembly intended, so metres, the row-vector layout and the translation are confirmed.

### Decision
`gusset_sldprt::read_assembly` returns the components (name, file name, configuration, kind, 4x4
column-vector placement, suppressed, hidden, part box). `import.file` on `.sldasm` reads each referenced
`.sldprt` from the assembly's own folder only (a stored name is cut to its file name, refused if it has
a separator, drive or dot name, matched against that folder's listing, opened without following links),
imports it as a part and moves it through `gusset_xt::to_step_placed` by the instance placement
(composed through sub-assemblies, which are read from their `.sldasm` files next to it, four levels deep).
A part used n times gives n bodies named after the component (`Name-2`; `Name-2 1`... for a multi-body part).
Missing or unreadable parts, suppressed and hidden components, mirrored or scaled
placements are warnings, not errors; an assembly with no importable component is an error.

### Limits
* The rotation reading is settled (2026-11-09) on 54 public assemblies of a robotics team's
  SolidWorks projects (ARIG, 2021-2024, most with asymmetric rotations): placing each component's
  stored bounding box with the row-vector matrix as stored reproduces the root model's stored
  box exactly in 41 of them; the transposed matrix matches only in the 9 whose rotations are all
  symmetric. The others hold sub-assemblies or components whose stored boxes are stale, and miss
  either way.
* Sub-assembly nesting inside one file is untested (the sample has none); it is handled by reading the
  referenced `.sldasm` file, whose own component list is complete. Structure inside the file is not used.
* Mates, configurations other than the one a component names, exploded views and in-assembly features are
  not read. A component's configuration is not selected (the part's lowest
  configuration with a partition is imported). Compound-file (OLE2) assemblies, from before about 2015,
  are refused: no sample.
* Bounds: 20000 components, 2000 placed instances, 4 sub-assembly levels, 2 GiB of part files, 1 GiB of
  shapes; the XML scanner expands no entities beyond the predefined and numeric ones. `sldprt_read`
  fuzzes `read_assembly` too.

## Amendment 2 (2026-11-08): virtual components, rotation convention

**Virtual components.** `VirtualComp/Part1` and `VirtualComp/Part2` of the NIST assembly are not opaque:
each is a complete chunk container (the same 8-byte start, records with a CRC-32, 36 streams such as
`Contents/Config-0-Partition`, `swXmlContents/Features`, `docProps/*`), i.e. a part file embedded in the
assembly. `gusset_sldprt::extract_xt` reads them as they are (a 292-byte partition: schema, no body, because
both parts are empty: their `Features` list no feature). The stream of a component is `VirtualComp/` plus the
text of its `swName` before `^` (2 of 2; `Component::embedded`); `embedded_part(bytes, limits, stream)` returns
the bytes and `import.file` imports them like a part file, placed by the instance, with the assembly's name
for the body. The sample cannot show a body from a virtual component (both are empty, so each gives the
warning "not imported: no solid"); the path to a body is the ordinary `.sldprt` one, covered by the
`.sldprt` tests plus unit tests of the stream lookup with missing, over-limit, garbage and cut streams.
Limit: the name-to-stream rule has two data points; a missing stream is a warning.

**Rotation convention: not determinable from this sample, left as is.** Both rotated instances have
symmetric rotation matrices (silicon nitride: `[[-1,0,0],[0,0,-1],[0,-1,0]]`, top metal: `diag(-1,1,-1)`),
so `R` and `R^T` give identical placed boxes; the assembly box, the part boxes and the stack all agree under
either reading. The tiny residues (`-5.55e-17` at row 0 col 1, `-1.97e-31` at row 0 col 2) are asymmetric, but
the same 12 numbers appear verbatim, in the same order, in `Contents/Config-0`, `Config-0-MatesList` and
`DisplayLists` (and the translation in `Config-0-LWDATA` as a box), so those streams carry the same
representation and no independent orientation to compare. The row-vector reading rests on the translation
being the last row. An assembly with an asymmetric rotation (any 90-degree turn about one axis) settles it.

## Amendment 3 (2026-11-09): the feature tree, decoding in progress

`Config-0-ResolvedFeatures` is an MFC-archive object stream continuing `Config-0`'s object
index (class declarations `FFFF schema len name`, new objects `8000|class`, back-references).
Decoded from the sample bytes alone (`docs/formats/solidworks-feature-tree.md`; prototype in
`scripts/re/solidworks/`): the feature list and order, the default planes, sketch points,
lines and arcs (metres, sketch frame from the plane's rotation and translation), extrusion
depth and blind/mid-plane ends, revolve angles. On 6 of 33 parts the prototype's tree gives
the true volume within 0.1 %. Still missing before features can be imported: sketch
relations and dimensions (the typed walker stops there, so later back-referenced planes are
unresolved), the boss/cut flag, other end conditions, fillet edges, hole-wizard and mirror
records, and the newer (chunk-container) layouts. `.sldprt` stays tier 2 until a tree
rebuilds to the stored Parasolid body, which will be checked as for FreeCAD (ADR 0033
amendment 2026-11-09).

## Amendment 4 (2026-11-09): feature history import

**Decision.** `gusset_sldprt::features::feature_tree` turns a part's
`Config-0-ResolvedFeatures` (and `Config-0`, for the class index of the reference-plane
component) into a Gusset feature tree (`.gtree.json` version 2, ADR 0042): sketches (points,
lines, arcs, circles, centerlines; plane from the default-plane component or a plane-data
block), extrudes (blind, mid-plane, through all), revolves (axis = the first centerline) and
fillets/chamfers picked by edge midpoint. It is a faithful port of the prototype in
`scripts/re/solidworks/` (layout notes: `docs/formats/solidworks-feature-tree.md`), guesses
included: features are found by anchors rather than walked object by object, the profile
sketch is the one before the feature, boss or cut comes from the class or a "Cut-" name, cuts
go against the sketch normal, and the end-condition class is learnt per part. On the 33
Slip-Pad parts the port's trees equal the prototype's bit for bit.

Any feature the reader cannot say refuses the whole history with a reason naming it: an
unknown class (`{"type":"unsupported"}` in the prototype), a sketch whose plane or lists
were not found, an extrude with an unknown end condition, a revolve without an axis line, a
fillet or chamfer with an unresolved edge, or a part with no extrude or revolve.

**Check.** As for FreeCAD (ADR 0033 amendment), shared code in `import.rs`
(`rebuild_check`): the tree is built in a scratch session against the exact bodies read from
the Parasolid partition; every feature must build, the volume must agree within 2e-5 and the
bounding box within 1e-4 of its diagonal. Then the part comes in as features (tier 1, "the
SolidWorks feature history as editable features"); otherwise as exact bodies, as before,
with the warning "the design history came in as exact bodies: {why}".

**Limits.** Feature and `Config-0` streams of at most 64 MiB; at most 2048 feature nodes,
65536 node names and 65536 matches of any pattern in one feature's span; names of at most
256 UTF-16 units; every scan is linear or bounded by the node cap, nothing recurses, reads
past the data are errors (or a skipped sketch, where the prototype caught them). The
`sldprt_read` fuzz target calls it on the same inputs as the container reader.

**Coverage** of the first port (superseded by amendment 5) on the 35 Slip-Pad parts (`GUSSET_SLDPRT_SAMPLES`,
`crates/gusset-cli/tests/sldprt_history.rs`, floor `GUSSET_SLDPRT_MIN_HISTORY`): 12 come in
as history (PCB_v1, PET_tape_v1, PET_tape_top_v1, bearing_3x6x2, drive_belt_71mm_v1,
loadcell_spacer_v1, shaft_3x22mm, shaft_3x55mm, spring_holder_v1, loadCell_v1,
idler_single_v2, loadcell_centerPlatform_v1, the last with a fillet), 22 as exact bodies, and
one (setScrew_M2x2_5) fails in the Parasolid reader (node type 184), as before. The reasons:
hole wizard features (7), unresolved fillet edges (3) and chamfer edges (3), imported bodies
(2), mirror patterns (2), a mirrored part (1), no feature stream (1), a chamfer whose
midpoints miss the rebuilt edges (arm_idler_single_v4), a volume mismatch
(idler_differential_baseJoint_v1: 96.6 vs 160.0 mm3), and a part whose stored body has a
negative volume (idler_single_v1). The three NIST parts in the chunk container were not in
this run.

## Amendment 5 (2026-11-09): hole wizard, mirror, face-sketch frames, edge records

**Decision.** `features.rs` follows the prototype through its round 3-7 additions
(`docs/formats/solidworks-feature-tree.md` sections 6-9). New records read:

- **Hole wizard** (`moHoleWzd_c`): the two sketches that follow the node are the placement
  sketch and the (often empty) section sketch. The hole centres are the placement sketch's
  stand-alone points (point field `A` = 1, id not -1, not projected), taken onto its plane;
  the face is that plane (normal out of the face). Diameter, depth and countersink come from
  the node's named parameters (`Hole Dia.`, `Thru Hole Depth`, `Near C'Sink Angle`, ...). A
  countersink with a `Head Clearance` is written as a 0.15 mm circle pocket plus one hole on
  the pocket floor per position (a hole whose positions lie on different faces is dropped by
  the importer). A hole without centres, diameter or face refuses the history.
- **Mirror** (`moMirrorPattern_c`): the plane is the frame stored in the mirror's own body
  (`<4 f64 extents> 01 <R> <t> 1.0`); the whole body is mirrored and merged. The face list
  in the same body is only a fallback (cap of a known feature).
- **Sketches on model faces**: the first plane-data block is the face's, with axes that may
  be turned; the sketch's own frame block (`01 00 00 00 <12 x 00> 01 <R> <t> 1.0`, same
  normal) that follows replaces it.
- **Edge records**: 0-2 extra zero bytes in the header, 0-2 extra words per face reference,
  face lists of n-1 references; cap classes are learned per file; an edge may lie between
  faces of two features (a cut's wall against a boss's cap); references to faces the
  profile sketch no longer has are stale and dropped (`stale_edges`). Edge crossings sample
  arcs at 2000 points; the work of all crossings of one part is capped at 1e8 point
  pairs (more refuses the history).
- **Segments** with flag bit 0x01 are not profile, like construction (bit 0x08); an empty
  sketch (no entity headers) is an empty sketch, not a failure.
- Parameter and end-condition patterns match any length byte, including a newline (the
  prototype's DOTALL): `Hole Depth` is ten characters long.

The trees of all 33 parts with a feature stream equal the prototype's field for field
(only the diagnostic `edges_selected` of the Rust reader is extra). Tests: synthetic streams
for the hole wizard, head clearance, mirror, face-sketch frame and a two-feature fillet edge
(`crates/gusset-sldprt/src/tests.rs`), and the damage test now mutates hole, mirror and
fillet streams.

**Coverage** on the 35 Slip-Pad parts: **17 come in as history** (was 12): Base_loadCell_v1,
PCB_v1, PET_tape_v1, PET_tape_top_v1, Top_modular_aperture_v2, arm_idler_v2, bearing_3x6x2,
drive_belt_71mm_v1, finger_aperture_15mm_v4, idler_single_v2, loadCell_v1,
loadcell_centerPlatform_v1, loadcell_spacer_v1, shaft_3x22mm, shaft_3x55mm,
spring_base_tab_v1, spring_holder_v1; 17 as exact bodies; one (setScrew_M2x2_5) fails in the
Parasolid reader as before. The prototype rebuilds 18: the eighteenth, **motorClip_v1**,
rebuilds to 656.082 against 656.055 mm3 (4.1e-5; the prototype accepts 1e-4) and is refused
by the 2e-5 check shared with FreeCAD. A cut depth of 0.901 instead of the stored 0.9 mm gives
656.056, so the residual is one part in 1e-3 of something not identified, not a missing
record; the tolerance is the owner's decision. The reasons for the rest: end type 4 (2),
imported bodies (2), chamfer edges not resolved (3), cosmetic thread (1), mirrored stock (1),
fillet edges not resolved (3: arm_idler_single_v4 3 of 4, the two idler arms 16 of 30), volume
mismatch (baseJoint 1.4e-4, idler_single_v1 whose stored body is suspect, motorClip), a
countersink on a clearance pocket floor that the kernel refuses (spring_base_v1), and no
feature stream (1).


## Amendment 6 (2026-11-09): newer chunk-container parts

The clean-room prototype (`scripts/re/solidworks/`, diff in `newer-format.diff`, layout notes
"Newer chunk-container parts" in `docs/formats/solidworks-feature-tree.md`) learnt the feature
streams of the 2015+ chunk-container parts of the public ARIG revision set. `gusset_sldprt::features`
takes over each gain; the Rust trees of all 99 parts of the Slip-Pad, ARIG revision and Zenodo sets equal
the prototype's field for field (only the diagnostic `edges_selected` of the Rust reader is extra).

- **Streams.** `Config-<N>-ResolvedFeatures` and `Config-<N>` for any N, with or without the `Contents/`
  prefix, in both containers; the lowest N that has both streams is read (the 8 Servo revisions use N = 1).
  The newer stream starts with a `u32 B` before the `u16` object count; nothing in the reader looks at the
  header (anchors only), so no change was needed for it.
- **Unit-component tag.** A chunk-container part learns the class tag of `moUnitComponent_c` from the
  bytes (the most frequent `XX 80 02 00 <10 x 00>`), not from the `Config-0` numbering. Reference planes read
  their id at tag + 45 (older) or + 69 (newer component base, 55 bytes instead of 31) and the plane of the
  first sketch, written before the node, is searched 400 bytes backwards.
- **Parameters** may hold an empty string (`ff fe ff 00`) before their final `00 00`.
- **Boss or cut** comes from the node flags word, low 20 bits `0x201ca` = cut (`0x00140` = boss), besides the
  class and the old "Cut-" name rule; it holds for OLE2 parts too (the 17 history parts are unchanged).
  A line of kind 0 is a centerline or construction only when its flags have `0x8008`.
- **Mirror.** `moMirrorSolid_c` reads like the mirror pattern; the mirror face is the first reference of the last
  face list of the node (the `moSingleFaceRef_w` object): the cap plane, or for a side face the plane through
  its sketch line along the extrusion normal. Edge lists may hold n - 1 references.
- **Distance-angle chamfer** (second parameter an angle in (0.1, pi/2) rad, not 45 degrees) is a `chamfer` with
  `angle` (degrees) and a `face` pick (ADR 0042 amendment) for the face D1 runs along: the face shared by the most
  selected side/side edges (ties: lower key; a guess from one sample, plaque_superieure). Edges are grouped by that
  face, one chamfer per group (`Name`, `Name_2`, ...); edges that are not such a corner get a last chamfer with no
  pick. This replaces the triangular through-cuts per corner. The bytes have no equal/two-distance flag (all 13
  chamfer nodes carry D1 and D2 only, D2 an angle), so no `distance2` is written.
- **Sketch lines are normalized** (merge coincident points, split lines at points on them, drop walls shared by
  two different segments and zero-length lines): two loops sharing part of an edge made the region finder ignore
  the cut silently. Capped at 5e7 point-line tests per sketch (a larger sketch is left as read).
- **Hole wizard** in other UI languages: the parameters not called `D<n>`, in order diameter, depth, tip angle.
- More geometry-free folder classes are skipped (selection sets, notes area, feature, surface/solid body,
  reference axis/plane and profile folders).

**Coverage.** Slip-Pad: 17 of 35 as history (unchanged). ARIG revisions (`arig-revisions`, 50 part files):
**10 as history** (was 0): `plaque_base_roulante` 5 of 5 and `plaque_superieure` 5 of 6. The sixth
(`plaque_superieure` 1618664833) is refused by the rebuild check: its Congé1 (r = 3 mm, three concave
corners) is a silent no-op in the kernel when the three edges are given together (the prototype notes the same:
1.5e-4 low). 33 files are `moMirrorStock_c` parts (mirror of an external part, no sketch to read) and come in as
exact bodies; the 6 `Angle_support` revisions fail in the Parasolid reader (damaged XT, node type -15684) and
need loft cut, move face, thin boss, fillet-edge and through-all-up-to-next records.

### Amendment: history round 2 (2026-11-09)
- Cap references with two extra words (index 0/1, first extra not -1) are caps in edge records (`Rep.ext2`); lifts
  Top_beltSupportRim to history. A revolve axis is chosen so the whole profile lies on one side; cosmetic threads and the
  configuration table are skipped. Slip-Pad 18 of 35, ARIG 10 of 50 (28 of 85).
- Tally of the rest and the unsolved ones (extrusion direction of MicroGearMotor Boss-Extrude1 has no known flag; revolve and
  fillet-face edge references; mirror-stock bodies need an "imported body" tree feature) are in
  `docs/formats/solidworks-feature-tree.md` section L8. The test labels revision files as `<part>/<stamp>`.

## Amendment (2026-11-09): stored stock bodies and rounded corners

A `moMirrorStock_c` part (a mirror of an external part) is a `stored_body` feature whose body is the part's
`StockBodies` partition, mirrored across the axis plane found by comparing bounding boxes, and the rebuild check
stays as it was. A blind cut with a second dimension D2 starts D2 behind its sketch plane. A fillet of two straight
profile sides is remembered, so that later edge references to its face locate the cap arc and a shortened side.
A part whose history would be the stored body alone, or whose file keeps only the finished
(mirrored) body, comes in as bodies: replaying features on a finished body passes the rebuild check
trivially. Coverage 33 of 85 (slip-pad 19 of 35, arig-revisions 14 of 50).
See `docs/formats/solidworks-feature-tree.md` L9.

## Amendment (2026-11-09): extrusion direction and start offsets

The unit vector after the `0005 0002 0002 0002` marker of an extrude node is the extrusion direction (a z direction is
written as the y component, negated); a distance that runs against it is reversed, for axis-aligned sketches that are
neither symmetric nor through-all. A blind cut with a second length (`D2`, or `D<n+1>` after the `n` dimensions of its own
sketch) starts that far from its sketch plane, ahead of it along the cut for end spec X 1 and behind it for X 0. The
circle edges of a full revolve are named by edge records (key of the profile segment on either side) and resolved as
closed paths. The rebuild check is unchanged. Coverage 34 of 85 (slip-pad 20 of 35, arig-revisions 14 of 50);
MicroGearMotor_v1 is the new one. GT2_Bobbin_v2 now fails in the kernel (a 1 mm chamfer along a 1 mm wall), the
end types 4/5 and the Angle_support features are decoded as far as `docs/formats/solidworks-feature-tree.md` L10 says.

## Amendment (2026-11-09): up-to-surface and offset-from-surface ends

The extrude node's end types 4 (up to surface) and 5 (offset from surface) name their face in the node's last
`moSingleFaceRef_w`: its first reference is an edge-record reference (feature id, face number) to a cap or a side of an earlier
extrude, resolved with the faces the reader already models. The reader writes `"end_condition": "to_face"` (ADR 0042 amendment)
with the face's point and outward normal, `offset` and `reverse` when the face is ahead of the sweep and covers the profile swept
onto it; otherwise the plane's position as a blind distance (SolidWorks extends a planar surface; a face behind the sketch, as
spring_base_v1 Boss-Extrude2's, only gives a distance). Type 5 puts the end plane `D1` into the face's material. The reader's
own end-type label is now `sw_end_kind` (it had shadowed the tree's `end_condition`, which made every non-blind extrude fail
to import: the importer learned that key in the up-to-face commit; coverage had dropped to 13 for that one commit). Coverage
34 of 85, unchanged: FrameBracket (both files) now gets past its four type-4 features and the type-5 cut and stops at
Fillet1 (edges between faces of features with different normals) and its hole wizards; spring_base_v1 stops at its
countersinks, which are not a fit-check equality problem: the pocket floor (r 3.15 at x = -22) is clipped by the plate's
edge at x = -25, so the mouth overhangs the plate by 0.15 mm and the check is right (left as it is). See
`docs/formats/solidworks-feature-tree.md` L10.

**Amendment 8 (edges between merged coplanar faces; findings).** An edge record whose two faces are not neighbours in one
profile (a later feature's face coplanar with an earlier one merges with it, and the record keeps naming the earlier feature's
face) is resolved as the line where the two face planes meet, between the planes of the two vertex faces
(`plane_edge`, straight sides and caps only). FrameBracket Fillet1 now resolves 2 of 2 edges and builds; the part still comes
in as bodies (a cut read against the truth, hole wizards on faces that do not hold them, a hole width not read). Coverage 34 of
85, unchanged. Findings that close other leads: idler_differential_arm(_short) fail against an incomplete truth (the Parasolid
conversion drops the lug fillets' spindle-torus faces, so the history cannot match it); the 1 x 1 mm chamfer OCCT refuses
(GT2_Bobbin_v2, Top_mainplate) has an exact construction (a cut by the revolved triangle) but the truth is 7e-5 away from a plain
1 x 1 chamfer, over the 2e-5 check. See `docs/formats/solidworks-feature-tree.md` L11.

## Amendment (2026-11-13): more SolidWorks versions, imported bodies (round sw1)

Measured first on a new corpus, **sw-more** (1253 parts from 30 public sources, whole corpus), beside the old three. The
reader stopped most sw-more parts before any rebuild: of 1253 feature trees only 162 read whole. Decoded from the sample
bytes (`docs/formats/solidworks-feature-tree.md` L12):

- **Sketch layouts.** Version 3 entity headers have the 8 zero bytes before the payload only in some files (the bytes
  decide); older modellers write the header's schema word as 0x07 instead of 0x1f.
- **Plane components** of newer versions put the plane-feature id 12 bytes (or 12 bytes and a word) further on; the id
  is looked for at each offset from 43 to 112. A first sketch may sit on a `moFixedRefPlnData_c` block.
- **Geometry-free nodes** (markups, comments, attributes, alignment groups, sensors, centre of mass, cut-list, detail and
  graphics folders, reference points, axes and coordinate systems) are skipped; an end condition with a 1 in byte 12 is
  read.
- **Imported bodies** (`moBaseBody_c`) are `stored_body` features; the importer takes the body of the same name from
  `LocalBodies`. A history is refused when nothing editable follows the imported bodies (540 sw-more parts) or when they
  already are the finished part, since replaying cuts or joins on the finished part would pass the check trivially. A
  `LocalBodies` body over 1 MiB continues in further zlib chunks, which the body import had cut off (sw-more parts whose
  bodies could not be read at all: 26 -> 14).
- **A 90-degree distance-angle chamfer** on a revolve's circle edge is a two-distance chamfer (D1 on the planar or
  cylindrical face, D1 / cos(corner angle) on the cone), from adpator's bytes and its own body.

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4 of the diagonal).

**Coverage** (history / parts; the history test with `GUSSET_SLDPRT_SAMPLES`):

| corpus | before | after |
|---|---|---|
| slip-pad | 23 / 35 | 23 / 35 |
| arig-revisions | 15 / 50 | 15 / 50 |
| zenodo (SolidWorks parts) | 0 / 14 | 1 / 14 |
| sw-more | 47 / 1253 | 156 / 1253 |
| all | 85 / 1352 | 195 / 1352 |

No part lost its history. Two sw-more parts kill the importing process and are skipped by the corpus driver: RJ45-LED
(SIGSEGV in OCCT mass properties, in-process) and Link_2 (the hole fit check of `gusset-eval` meshes a 290 mm imported
body at 0.01 mm and grows past 8 GB); both are kernel or evaluator issues, reported, not changed here. The largest
remaining reasons in sw-more: STEP imports with no feature (540), volume mismatches (77; the bearings among them are
mirrors of cut features read as whole-body mirrors), sketch planes (43), fillet edges (40), shells (22), patterns (33).

## Amendment (2026-11-13): volume mismatches, sketch planes, end conditions (round sw2)

Each sw-more volume mismatch was classified against the part's own Parasolid body: a scratch harness rebuilds the
history one feature at a time and measures what lies outside the body and what is missing from it after each
feature, so the feature that goes wrong shows. Decoded from the sample bytes (`docs/formats/solidworks-feature-tree.md`
L13):

- **Unit-component tags above 255** (`24 81 02 00`): the reference-plane components of files with many classes; a tag
  votes only where a reference-plane frame follows it (stray runs had outvoted the real tag).
- **End-condition byte 12** reverses the extrusion: a boss against its sketch normal, a cut along it. It agrees with
  the direction vector wherever one is written, and decides where none is.
- **Elliptical sketch arcs** (four axis end points where an arc writes four -2 words) are `ellipse_arc` curves; they
  had been read as straight lines.
- **Line and point references** (`moLineRef_w`, `moEndPointRef_w`) carry their geometry in model coordinates: a revolve
  about a reference axis turns about that line; an extrude up to a vertex (end type 3) ends on the plane through it.
  A centerline that is a projected model edge is not a revolve axis.
- **Names nested in a node's header** (a reused sketch's `Sketch3<3>`, an attribute) no longer cut the node's span
  short (extrudes lost their end conditions, zsCAD sketches their entities).
- **Sketches kept in `Config-<n>`** (driven by equations or configurations) are read from its `sgSketch` objects, in
  order, by the sketch nodes that hold none.
- Construction arcs off their circle and circles through their own centre are left out (Gusset refused them, or its
  solver moved shared profile points: idler_differential_baseJoint_v1's 1.4e-4 residual was this).
- Fillet edge records that start with the fillet's own face are read; imported features that share a name take the
  numbered `LocalBodies` records in order (`import.rs`).

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4 of the diagonal), and so are
the stored-body rules (a history of stored bodies alone, or on a finished body, stays refused).

**Coverage** (history / parts; the history test with `GUSSET_SLDPRT_SAMPLES`, whole corpora):

| corpus | round sw1 | round sw2 |
|---|---|---|
| slip-pad | 23 / 35 | 24 / 35 |
| arig-revisions | 15 / 50 | 15 / 50 |
| zenodo (SolidWorks parts) | 1 / 14 | 1 / 14 |
| sw-more | 156 / 1253 | **195** / 1253 |
| all | 195 / 1352 | **235** / 1352 |

No part lost its history. RJ45-LED and Link_2 are skipped by the corpus driver as before (in-process crashes, fixed on
another branch). Gained: idler_differential_baseJoint_v1 (slip-pad) and 39 sw-more parts, listed in L13. The largest
remaining sw-more reasons: STEP imports with no feature (540), volume mismatches (83; parts that now read whole land here),
fillet edges (39), shells (22), patterns (37), edge midpoints not on the model (18). Not decoded: the feature lists of
mirrors and patterns (the bearings' mirror of cuts is still read as a whole-body mirror), mirror planes on user reference
planes, dimensions kept in the configuration stream.

## Amendment (2026-11-13): patterns and mirrors of features (round sw3)

The feature stream is one object archive: a reference to an earlier object is its archive index. Patterns and mirrors
of features name their seed features that way (a seed list `T n <indices> T 00 00 01 00 00 00 00 00 01 00 00 00 00 T
m`), and the index of a feature node comes from back references to it: its own dimension objects, the owner word of
references to its own faces, or, for a node that declares its class, the class index plus one. Decoded from the sample
bytes (`docs/formats/solidworks-feature-tree.md` L14):

- **Linear and circular feature patterns** (`moLPattern_c`, `moCirPattern_c`): seeds as above, counts `D1`/`D2` (integer
  parameters), spacings `D3`/`D4`, step and total angles `D2`/`D3`, the direction or axis from the pattern's line
  reference; a two-way linear pattern is two Gusset patterns. Patterns of bodies and second circular directions are not
  read (refused with a reason).
- **Mirrors of features** (`moMirrorPattern_c`) mirror their listed features; only `moMirrorSolid_c` mirrors the body
  (before, both mirrored the body, which refilled mirrored cuts).
- **Edges on a mirror's copy** (a 16-byte reference to the mirror wrapping the seed face's reference) are solved reflected
  back through the mirror plane; selections of long edge records run on while the records follow each other.

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4 of the diagonal), and so are
the stored-body rules.

**Coverage** (history / parts; whole corpora):

| corpus | round sw2 | round sw3 |
|---|---|---|
| slip-pad | 24 / 35 | 24 / 35 |
| arig-revisions | 15 / 50 | 15 / 50 |
| zenodo (SolidWorks parts) | 1 / 14 | 1 / 14 |
| sw-more | 195 / 1253 | **199** / 1253 |
| all | 235 / 1352 | **239** / 1352 |

No part lost its history. arig-revisions has no single cause: 29 of its 35 refusals are mirror parts whose file keeps only
the mirrored body of another part (a stored body, not a rebuild), the other 6 are one part's revisions that stop at a loft
cut followed by thin extrudes. Not decoded: faces of pattern copies in later fillets (18 parts), mirror planes on user
planes, the bearings' eight-edge chamfer (now 0.2 to 0.5 % light), lofts, shells.

## Amendment (2026-11-13): measured blockers, equations, shells (round sw4)

Round sw4 measured the refusals first and took only blockers with a count (`docs/formats/solidworks-feature-tree.md`
L15). Decoded from the sample bytes:

- **Sketch entities past alignment groups**: a sketch whose span holds no entity reads on over the `moAlignGroup_c`
  nodes written after it; entity payloads skip 8 leading zero bytes whatever the header version.
- **Depths an equation drives**: a blind or mid-plane extrusion without `D1` takes it from the configuration stream,
  where the equation `"D1@<feature>"= ...` is followed by the dimension's own parameter object.
- **Mirror planes named by id**: a mirror without a frame block names its plane by the plane feature's id; the Front,
  Top and Right planes are read so (plane features of the part are not: they keep a definition, not a frame).
- **Edges**: closed edges `[A, B, B, B]` (a circle's wall or a revolve's cylinder or cone cut by a plane across it), and
  edges along an extrusion that end on other features' planes.
- **Shells** (`moShell_c`) of one removed face: the face is the first reference of the node's last record, the wall
  `D1`; caps of profiles of arcs only are picked inside their largest arc.

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4 of the diagonal), and so are
the stored-body rules.

**Coverage** (history / parts; whole corpora):

| corpus | round sw3 | round sw4 |
|---|---|---|
| slip-pad | 24 / 35 | 24 / 35 |
| arig-revisions | 15 / 50 | 15 / 50 |
| zenodo (SolidWorks parts) | 1 / 14 | 1 / 14 |
| sw-more | 199 / 1253 | **223** / 1253 |
| all | 239 / 1352 | **263** / 1352 |

No part lost its history. The per-feature harness over round sw3's 85 volume mismatches no longer loses its verdict when
one boolean against the stored body fails (2 parts left without one); the larger discrepancy grows most at a cut (26), the
first boss (24), a later boss (12), material no feature adds (7), stored bodies (6): no shared cause. The remaining
blockers each cover one to three parts: fillets on pattern copies, up-to-surface ends on faces not placed, mirror planes on
plane features of the part, mirrors of mirror features, shells of several faces, variable fillets, lofts.

## Amendment (2026-11-16): fillet edges and extrude end conditions (round sw5)

Round sw5 measured the 85 parts that stopped first at fillet or chamfer edges (51) or at an extrude end condition (34),
and the 13 whose fillet edge midpoints lie on no edge of the rebuilt model, and grouped them by root cause from the sample
bytes (`docs/formats/solidworks-feature-tree.md` L16). Decoded:

- **The cut flag of older files**: a node's flags `0x1ca` (without bit 17) marks a cut; localized cuts had been joins.
- **Up to a planar face the model no longer covers**: the reader writes `plane_distance` (the blind distance to a
  parallel face's plane) beside `to_face`; the tree import (ADR 0042) uses it when the face does not cover the sweep as
  rebuilt, which is what SolidWorks does with a planar face.
- **End planes** of caps without a point inside them, of the flat walls of cuts through all, and of reference planes
  (`moPlaneRef_w`: four extents, origin, unit normal after the `c7 cf ff ff` marker and a version word); point
  references with version words 6000 and 9000; the last point reference of the node is the vertex.
- **End specs** after a `moDirectionSpec_c` (a direction by reference), byte 8 as a flipped cut (through all: the common
  part with the sweep), the end-spec class learned from X = 0 references when no X = 1 one exists.
- **Edges** where two profile curves meet without crossing (a line ending on an arc); of two meeting points, the one the
  record's order puts the first face on the left of the run between the vertex faces.
- **Up to a cylinder** an arc sweeps: `to_face` on it; the kernel and the rebuild check judge.

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4 of the diagonal), and so are
the stored-body rules.

**Coverage** (history / parts; whole corpora):

| corpus | round sw4 | round sw5 |
|---|---|---|
| slip-pad | 24 / 35 | 24 / 35 |
| arig-revisions | 15 / 50 | 15 / 50 |
| zenodo (SolidWorks parts) | 1 / 14 | 1 / 14 |
| sw-more | 223 / 1253 | **230** / 1253 |
| all | 263 / 1352 | **270** / 1352 |

No part lost its history. Of the 34 end-condition parts 4 rebuild, 18 stop later, 12 remain (faces the reader does not
model, a through-all boss after an imported body, single-part layouts). The fillet edges have no shared cause within reach:
about 22 parts name faces of earlier fillets or chamfers that are not profile corners, 6 faces of imported bodies or
pattern copies, 3 are face-to-face fillets, the rest single geometric cases; the midpoints off the model are per-part
sketch differences.

## Amendment (2026-11-16): lofts, sweeps, aligned sections (round sw6)

Round sw6 measured the feature kinds the reader refused before any rebuild (lofts 23, sweeps 8, split bodies 16, splits
5, helices 8, threads 4, ribs 6, drafts 4, domes 4, variable fillets 18, three annotation folders 12): with each kind
skipped in a scratch build, how many parts would reach the rebuild check and how many stop at a later blocker
(`docs/formats/solidworks-feature-tree.md` L17). Decoded from the sample bytes:

- **Sections by node id.** Lofts (`moBlend_c`, `moBlendCut_c`) and sweeps (`moSweep_c`, `moSweepCut_c`) name their
  sketches in profile components: the sketch node's id after 8 zero bytes (or four and `50 46 00 00`), followed by a time
  stamp. A loft lists its sections in order; a sweep its profile, then its path.
- **Connectors.** A loft writes a point on each section; a section that is one circle is split into two half arcs there,
  and the loft is written `aligned`: the sections twist as SolidWorks joins them.
- **Circular profiles.** A sweep that names only its path sketch and writes `<MOD-DIAM>` sweeps a circle of the dimension
  before that text, drawn square to the path at an open end.
- **No-op nodes**: configuration tables, weldment cut lists and their folders, cosmetic weld beads, reference point folders.
- **A line drawn twice in one sketch loop** keeps one copy (pieces two lines cover were all dropped as shared walls).

Gusset gained one thing for this: `loft (..., aligned: true)` (document language, `LoftSpec`, the shim's `gs_loft`, the
feature tree's loft): the sections' outlines correspond edge for edge from their starts and keep their twist, where
OCCT's ThruSections would realign closed sections to untwist them. Only the SolidWorks reader writes it; Fusion and
FreeCAD trees are unchanged.

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4 of the diagonal), and so are
the stored-body rules.

**Coverage** (history / parts; whole corpora):

| corpus | round sw5 | round sw6 |
|---|---|---|
| slip-pad | 24 / 35 | 24 / 35 |
| arig-revisions | 15 / 50 | 15 / 50 |
| zenodo (SolidWorks parts) | 1 / 14 | 1 / 14 |
| sw-more | 230 / 1253 | **235** / 1253 |
| all | 270 / 1352 | **275** / 1352 |

No part lost its history. Beyond reach this round, with evidence in L17: split bodies (the part keeps no body of the part
it was split from: no `StockBodies` stream, only the final body), threads and helical sweep cuts (helix decoded: `D3`
height, `D4` pitch, `D5` turns, `D7` start angle; the hand is not found in the bytes, and the rebuild check cannot tell a
left-handed groove from a right-handed one), variable fillets (no tree support, and none of the 18 parts would reach the
check: 13 stop at an up-to-surface end, 3 at open sketches, 2 at fillet edges), domes, ribs and drafts (one part each
would reach the check). One loft (output_shaft_stage_4) rebuilds untwisted although its connectors are 2.7 degrees apart:
the connector rule matches four lofts and misses this one.

## Amendment (2026-11-17): suppressed features, contour choosers, sketches by child index (round sw7)

Round sw7 took the 98 parts whose features all rebuilt but whose volume differed from the part's body and grouped them by
the feature where the rebuilt solid first leaves the body or misses it (`docs/formats/solidworks-feature-tree.md` L18).
Decoded from the sample bytes:

- **Suppressed features.** The `u32` 47 bytes before a feature node's own block is 1 when the feature is suppressed in
  the configuration read (the one whose body is compared); such features are left out. A node that nests others owns
  the block after theirs.
- **Cut flags with bit 2** (`0x2_01ce`, the zsCAD parts) mark cuts too; a cut in a part with several imported bodies
  cuts all of them.
- **Contour choosers.** A feature that extrudes only some regions of its sketch lists them, each with its sketch
  segments and its bounding box in model space. The reader writes such profiles as picks by box; the tree import
  takes the region with that box.
- **An extrude's sketch** is its node's first child, by archive index; an index below one already seen names an
  earlier, unused sketch.

Gusset gained one thing for this: a feature tree's profile pick may name a region by its bounding box
(`{"bbox": [x, y, z, diagonal]}`, mm, model space) instead of a point; the closest region within 2 % of the diagonal and
three times closer than any other is taken, and a box that singles out no region keeps the whole sketch. Only the
SolidWorks reader writes it; Fusion and FreeCAD trees (point picks) are unchanged.

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4 of the diagonal), and so are
the stored-body rules.

**Coverage** (history / parts; whole corpora):

| corpus | round sw6 | round sw7 |
|---|---|---|
| slip-pad | 24 / 35 | 24 / 35 |
| arig-revisions | 15 / 50 | 15 / 50 |
| zenodo (SolidWorks parts) | 1 / 14 | 1 / 14 |
| sw-more | 235 / 1253 | **246** / 1253 |
| all | 275 / 1352 | **286** / 1352 |

No part lost its history. 86 parts still rebuild to a different volume. Beyond reach this round, with evidence in L18:
contour choosers whose boxes and segments describe another state of the sketch than the one the bytes hold (the
V-Slot family, the Connecting Plates and a dozen others keep their whole sketch), zsCAD parts whose stored bodies
already lie outside the part, and single-part causes (a chamfer record, hole wizards, patterns of bodies).

## Amendment (2026-11-17): partitions past 1 MiB, bodies kept in an import feature (round sw8)

Round sw8 took the 14 parts whose embedded Parasolid failed to read (`docs/formats/solidworks-feature-tree.md` L19).
Decoded from the sample bytes:

- **Continuation chunks.** A partition block whose data inflates to more than 1 MiB holds a 1 MiB zlib stream and then
  further chunks inside the block's length (`u32` inflated size, `u32` deflated size, zlib data), ended by two zero
  sizes: the layout already read for `LocalBodies` blocks. The partition reader stopped after the first 1 MiB, so six
  parts "ended early" just under byte 1048576 and the three gears of one gearbox (chunks of 4 kB after the first MiB)
  read a node count past the cut. Both readers now share one continuation reader under the same limits (every chunk
  counts against the stream and file budgets and the inflate ratio).
- **Bodies of an imported body feature** (`moMBimport_c`) in a part without a `LocalBodies` stream lie in the feature's
  own record in the lowest configuration's `ResolvedFeatures`, as a block of the partition's kind (the tag every block
  carries, `23 1d d5 71 ...`); the final partition then holds no body. `named_local_bodies` falls back to those
  blocks; a partition or deltas transmit found there is not taken as a body. Three parts of the corpora have such a
  feature; the fallback applies to the two without `LocalBodies`.

The rebuild check and the stored-body rules are unchanged. History stays at 286 of 1352 (no part gained or lost it);
11 of the 14 parts now import as bodies, each stopping its history at a feature the reader does not build (surface
extrude/revolve, emboss, helix, split line, the imported body alone, an unresolved fillet edge). The other 3 are
correct refusals or out of scope: ignition-device-screw has no feature beyond the default planes,
cable_chain_sketch is a sketch only, and multispec_camera_mount_tilt is one graphics mesh body
(`moMeshBodyFeature_c`, data in `MeshData/Config-0-Mesh-27`, a format not decoded), not a B-rep.

## Amendment (2026-11-17): fillet edges named by their faces (round sw9)

Round sw9 took the 104 parts that stopped at a fillet (`docs/formats/solidworks-feature-tree.md` L20). Grouped
by why an edge failed, the largest cause was the reader's model of the faces: an edge record names the edge's two
faces and its two end faces by feature and face number, and the reader turned that into a midpoint only for faces
it can draw (extrude sides and caps, rounded profile corners). Faces of earlier fillets, edges whose
midpoint the reader's model puts off Gusset's model, and edges that are whole circles were left unresolved.

- **Edges by face names.** The reader now writes each record's faces as the features it read made them (an extrude
  or revolve side by the sketch curves of its key, an extrude cap with its plane, a straight side with its plane,
  the face a fillet or chamfer made from its k-th selected edge) and the record's selection number. The tree
  import (ADR 0042 amendment "edges named by their faces"; additive and optional, other sources unchanged) takes
  the midpoint first and, where it names no edge or there is none, the edge between faces whose Gusset names hold
  the two (`side(sketch/entity)`, `cap(...)`, `blend(<the edge that selection number resolved to>)`), then flat
  faces on the given planes (Gusset names a merged coplanar face after either feature), the end faces sorting
  several candidates; pieces Gusset cut from one closed edge count as one. The selection-number rule for fillet
  faces is the one round 3 found (L9).
- **Superseded selections** (records of known features whose edge the reader could not place, dropped since round
  3) are written by their faces and marked optional: left out where the model has no such edge, as before.
- **Variable fillets** (`VarFillet_c`): the radii are the node's `D0`, `D01`, ... parameters, one per vertex.
  Gusset has no variable-radius fillet (neither the kernel trait nor the document language), so only a variable
  fillet whose radii are all one value is read, as a constant fillet: OeMga's bottom, middle and top (eight or
  three radii of 1 mm; they now stop at later sketches without closed regions and, middle, at two edges of the
  variable fillet the reader cannot name). The r3 keycaps (0.4 / 2 mm alternating) and 126p-visor-mount (1 mm and 0) stay refused.

The rebuild check is unchanged. History 286 -> 290 of 1352, none lost: IMU Arm Mount 3D, FR8ZZ Hex Bearing (fillet
edges by face names), Motor, BK12_nut (chamfer edges whose midpoints were off the model). Of the 104, 27 now stop
where OCCT cannot build the fillet SolidWorks built (radius large against an edge, fillets meeting on short edges;
two crash the in-process kernel of the test harness), 26 still have edges the reader cannot name (faces of
pattern copies, imported bodies, lofts and chamfers written as angle chamfers, features the node list does not
hold, sketch entities that SolidWorks mirrored or derived and the reader does not read), 15 are truly variable
fillets, 14 have an edge whose faces the rebuild places elsewhere (an earlier feature differs), 5 rebuild every
feature and differ in volume.

## Amendment (2026-11-17): helices and helical sweeps (round sw10)

Round sw10 took the features SolidWorks parts stop at that Gusset's document can already build (L21 in
`docs/formats/solidworks-feature-tree.md`): helices (11 parts), threads (4), drafts (4), ribs (6), domes (4).

- **Helix** (`moHelix_c`). The node's record (`06 00 00 00`, six zero bytes, then the start angle, the clockwise and
  reverse-direction flags as `u32`, the height, pitch and revolutions; the height is the pitch times the
  revolutions in every helix of the corpus) and its base sketch (its child, as an extrude's: one circle) give the
  helix. A helix is a curve: it adds nothing to the part, so a helix no feature uses no longer stops the history.
- **Helical sweep.** A sweep whose path is a helix names the helix node as it names its sketches. The helix climbs
  from its base plane along the plane's normal when reversed, against it when not; clockwise is seen with the normal
  towards the viewer; the profile runs the helix from the end it lies at (within a turn). Evidence: the Shaft
  Couplers' slot profile lies on their reversed clockwise helix just past its top end (6 to 15.2 mm, 135 to 99
  degrees), and the gears' helices named RH and LH differ only in the clockwise flag. Written as the tree's
  `helix`; a helix that climbs no world axis in its positive direction runs along a construction line the reader
  adds to the profile sketch on the axis, which the tree import now takes (`helix.line`, ADR 0042 tree format;
  additive, other sources unchanged).
- **3D sketches** (`mo3DProfileFeature_c`) are curves too and are skipped like sketches (a feature that needs one
  finds no path and is refused).

Not done, and why: threads (`moSweepThread_c`) keep their data outside the node and cut a library profile that
Gusset's `thread` would have to match exactly; drafts need their faces, neutral plane and pull direction decoded
from edge records (the two candidates draft faces next to fillets); ribs and domes have no Gusset operation
(a rib extends an open profile to the part, a dome is a surface fit), so they stay refused.

The rebuild check is unchanged. History 290 -> 292 of 1352, none lost: Nut Block 8mm Acme and 5mm Low Profile
Socket Head Screw (helices no feature uses). The helical sweeps are read but no part gains by them yet: the two
Shaft Couplers rebuild their slot, then OCCT's boolean of the later counterbore (a 4 mm cylinder, floor at z = 10)
with the 5.1-turn B-spline helicoids of the slot returns a valid solid that cut nothing (volume unchanged, area
up; the same slot built from a profile 3 mm higher cuts correctly): a kernel robustness problem, not a reading
one. The other helix parts stop elsewhere (a composite-curve spring, a fillet edge, the bottle cap's thread-end
sweeps, the gears' tooth-cut patterns, a `moBoss_c`).

## Amendment (2026-11-18): drafts, fillet edges on pattern copies (round sw11)

Round sw11 (L22 in `docs/formats/solidworks-feature-tree.md`) took the four features round sw10 measured.

- **Draft.** `moDraft_c` is written as the tree's `draft`: the angle `D1`, the neutral plane named by its
  `moPlaneRef_w` (an extrude face, else the stored plane) with the pull along that face's outward normal, and one
  point per drafted face from the list the span ends with. A draft whose neutral plane cannot be placed is refused.
- **Edges on pattern copies.** Edge records name a face of a pattern's copy through the pattern, the seed face and
  the copy's number, as mirrors do. The reader places such an edge by moving the seed edge's problem back to the
  seed and its midpoint forward, and names its face as the pattern's instance; the tree import's face selector takes
  `pattern` and `copy` for that (additive; other sources unchanged). Before, the edge selection ended silently at
  the first such record.

Not done, and why: threads still lack their helix data (only the profile sketch, the node's child, was found); ribs
and domes have no Gusset operation and stay refused.

The rebuild check is unchanged. History 292 -> 293 of 1352, none lost: am-1305 500 Hex Spacer. The other draft
parts stop at features this round does not read (a pattern of bodies, a 3D-sketch loft, an imported surface).

## Amendment (2026-11-19): split lines, own face records, second directions, projected points (round sw12)

Round sw12 (L23 in `docs/formats/solidworks-feature-tree.md`) compared the 96 parts that rebuilt to a different
volume face by face with their stored bodies, and took the causes that recurred:

- **Split lines** (`moPLine_c`) divide faces and leave the solid as it is: skipped like sketches. A feature that
  picks one of their faces or edges is refused as before.
- **A fillet's own face records.** Newer files write one record per face a fillet or chamfer made before the edges it
  picks; the selection now starts at the first record that names none of the feature's own faces.
- **Pieces of a revolved circle** cut by other features' faces: the piece from the first vertex face to the second
  with the first face on its left (the round sw5 rule).
- **Drill angle**: a hole wizard's `Drill Angle` is the point angle (180 degrees: a flat bottom).
- **Second direction**: an extrude with `D1` and `D2` runs `D2` to the other side of its sketch (tree `distance2`);
  a cut's start offset is a later `D<n>` (round sw2's reading of `D2` as one gives the same solid on its sample).
- **End type 2 on a boss** is up to the next face (tree `to_next`, ADR 0061 amendment), through all as the fallback.
- **Projected sketch points without an object chain** are read from their own payload (they took the next
  entity's coordinates).

Measured and not done: the variable-fillet radii's ends are decoded (each `moVertDim_c` lists the faces at its
vertex) but no variable-fillet part can pass the check: the 14 r3 keycaps' stored bodies do not convert to STEP
(gusset-xt: a torus with a negative major radius, variable-fillet `BLENDED_EDGE` surfaces), so their reference is a
sheet of volume 0. The OpenBuilds bearings' chamfers list eight edges where the body has four (no distinguishing
byte found); a D-cut shaft fails in OCCT (a cut whose arc lies on the cylinder leaves a sliver face).

The rebuild check is unchanged. History 293 -> 314 of 1352, none lost.

## Amendment (2026-11-19): converted segments, revolve axes, drafted extrudes (round sw13)

Round sw13 (L24 in `docs/formats/solidworks-feature-tree.md`) took the causes that recurred among the refusals that
are not STEP-style imports:

- **Converted sketch segments** without an object chain are read from their own payload, as round sw12's projected
  points (they took the next segment's place, leaving the profile open).
- **Revolve axes.** A full revolve's box centre lies on its axis; where the line the reader chose misses it, the
  line through it with the profile on one side is taken (a profile edge or a lone line off the sketch's axes).
  Centerlines go before construction lines.
- **Open revolve profiles** whose loose ends lie on the axis are closed along the axis, as SolidWorks does.
- **Drafted extrudes** become the extrude and a `draft` of its side faces about the sketch plane, pulled the way the
  feature runs into the part; the extrude keeps its own faces (`refine` off) where its sides would continue an
  earlier extrude's. The draft angle is the extrude's placed `D3`.

Measured and not done: linear pattern directions run against the picked edge in eight of the ten the rebuild could
decide, with no byte found that tells the two cases apart; contour choosers whose boxes exceed their curves; open
cut profiles that end on the body's outline.

The rebuild check is unchanged. History 314 -> 332 of 1352, none lost (converted segments 4, revolve axes and open revolve profiles 9, drafted
extrudes 5; Expansion Block by the same fixes).

## Amendment (2026-11-20): blend edges, up-to-next cuts, start faces, contour choosers, pattern senses (round sw14)

Round sw14 (L25 in `docs/formats/solidworks-feature-tree.md`) measured the refusals that are not STEP-style imports
(volume mismatches 80, fillets and chamfers about 70) and took:

- **Edge records 22 bytes apart** stay one selection (newer files write an `f64` and six zero bytes more between
  them).
- **Up to next on cuts**: end type 2 is up to next for cuts as for bosses; through all stays the fallback.
- **Extrudes that start from a face**: a blind extrude whose node ends on a kind 2 single face reference starts on
  that face's plane where it is parallel to the sketch.
- **Contour choosers** naming every listed contour are used (the sketch's list need not hold every region); a
  chooser older than its sketch's dimensions picks regions by a point inside what its segments enclose.
- **Linear pattern senses**: the file writes the picked edge's own direction and no byte says which way the copies
  run; a pattern of cuts is marked `either_way` and the tree import keeps the sense whose copies remove more.
- **The stored body decides blend edges** (`Tree::kept_edges`, set by the SolidWorks importer only): a blend's
  edge that is still an edge of the part's stored body was not blended there and is left out, unless all would be.
- **Blends chain by chain**: a fillet or chamfer the kernel refuses whole is tried as one blend per chain of edges
  that meet at a vertex (shared tree import; the Inventor and Solid Edge corpora lose nothing).

The rebuild check is unchanged. History 333 -> 358 of 1352, none lost (bearings 4 by kept edges; up-to-next cuts,
start faces, choosers, pattern senses and chain blends the rest).

## Amendment (2026-11-20): pattern seeds, Config-0 sketches, re-blended edges, face lofts, flipped-side cuts (round sw15)

Round sw15 (L26 in `docs/formats/solidworks-feature-tree.md`) measured its own baseline (359 of 1352) and took the
reader-side causes that a byte rule explains:

- **Pattern and mirror seeds** whose archive index no back reference gives (hole wizard features) are the one
  feature read between the nodes whose indices enclose it: indices grow along the stream.
- **Config-0's classes and sketch owners.** A class tag below the count `Config-0` ends on names the class counted
  there, taken for the sketch class only (Toolbox parts declare it there); in chunk files only where that count also
  places `moUnitComponent_c` as the stream's own tags do. The sketch objects `Config-0` keeps go to the sketches the
  equation texts before them name, not by order.
- **Blend edges a later blend names again** were not blended there and are left out of the earlier blend.
- **Line references with version word 13000**, and a revolve axis that is a line of another sketch in the sketch
  plane, added to the sketch as a centerline.
- **Lofts from a cap** of an earlier extrusion (one sketch and one face reference).
- **Blind flipped-side cuts** as a cut of the region between the profile (and its holes) and a far frame, on a copy
  of the sketch.
- **Face sketches drawn facing into the face**: the sketch frame with the face's normal reversed, where none keeps it.

The reader changed only (gusset-sldprt); the tree import and the rebuild check are unchanged. History 359 -> 370 of
1352 against the round's own baseline, none lost (pattern seeds 3, Config-0 sketches 3, flipped-side cuts 2,
re-blended edges 1, face frames 1; Dual V-Wheel Steel, lost by the baseline run to a worker pipe that did not
close, imports again).

## Amendment (2026-11-23): shell face records, arc digits, contour points, mirrored ends, pattern senses (round sw16)

Round sw16 (L27 in `docs/formats/solidworks-feature-tree.md`) measured its own baseline (370 of 1352, row for row
round sw15's) and took:

- **Shell face records**: every reference record of kind 3 with an id of its own names a face the shell removes
  (the face first, its neighbours after it); the last record's first face stays the fallback.
- **Arc points keep their digits** where six decimals put the points on a drawn arc's circle more than 2e-7 mm
  apart in radius (the kernel builds an arc on its ends' mean radius and refuses ends 1e-7 mm off it); every other
  point keeps six, as planes and depths do, and full circles keep six.
- **Contour picks carry a point and a box**: the tree import takes the point where the box fits no single region
  (shared tree import; only the SolidWorks reader writes both).
- **Mirrored extrudes up to a face** toggle `reverse` and reflect their face pick, since the reflected sketch's
  normal is turned over (shared tree import, `reflect_feature`).
- **Linear patterns that add material** are marked `sense_by_box`: the import keeps the sense whose model
  reaches less past the box of the stored result's edges (`Tree::kept_edges`), as written where that does not
  tell.
- **Second directions**: a two-direction extrude's start cap lies at `distance2` for the edges and faces the
  reader places on it; a symmetric extrude's side points sit at mid-height.

The rebuild check is unchanged. History 370 -> 373 of 1352 against the round's own baseline, none lost
(Part3 by contour points, the two stepper motor shafts by arc digits); the Inventor (722 of 1639, 28 of 47) and
Solid Edge (99 of 180) corpora rerun with the shared tree import changes lose nothing. The kernel is unchanged.

## Amendment (2026-11-23): child indices, faces across a cylinder, shell walls, tooth patterns, 3D-sketch holes (round sw17)

Round sw17 (L28 in `docs/formats/solidworks-feature-tree.md`) measured its own baseline (373 of 1352, row for row
round sw16's) and took:

- **Sketches by child index**: where several unused sketches lie in the gap a child index falls in, the sketch
  whose entity references carry the index is taken.
- **Faces across a cylinder** (shared tree import, `named.rs`): the loose pass of an edge named by its faces also
  takes the faces that continue a named cylindrical face across edges about the same axis.
- **Shell walls**: references through a shell (and in-place class declarations in edge records) are read; a face
  pick may carry `shell`, which the import resolves to the shell's `inner(<face>)` (shared tree import).
- **Patterns**: suppressed seeds are left out; a circular pattern without `D1` takes the count kept in its own
  data; class tags one below a declaring node's archive index name that class outright.
- **Extrude depths** without `D1` from the same record; `moBoss_c` nodes are extrudes.
- **Mirrors**: a mirror of every feature so far mirrors the body; a mirror on a plane feature takes a sketch's
  plane on it.
- **Holes placed by a 3D sketch** take the section sketch's origin and axis line.

The rebuild check is unchanged. History 373 -> 384 of 1352, none lost. Only the SolidWorks reader writes face picks
on edges, so the `named.rs` changes cannot reach Inventor, Solid Edge or Fusion imports; the Solid Edge corpus was
rerun against the round's base binary. The kernel is unchanged.
