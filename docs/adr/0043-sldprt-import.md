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
