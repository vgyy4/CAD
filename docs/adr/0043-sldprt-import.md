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
