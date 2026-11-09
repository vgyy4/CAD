# ADR 0046: Solid Edge (.par/.psm) and Inventor (.ipt) import

Status: accepted (2026-11-09). PROMPT.md "Phase 3+": tier-2 native imports. Solid Edge writes
its geometry as Parasolid and Inventor as ShapeManager (ACIS family); both readers exist
(ADR 0035, 0039), so the missing piece is the container, as for SolidWorks (ADR 0043).

## Context
`docs/INTEROP.md` listed both as "not started: no public samples". Samples were found on Zenodo
(CC BY 4.0): record 8028435 (Solid Edge, 15 parts and 3 assemblies), 13941029, 11061412 and
4309294 (Inventor, 35 parts and 2 assemblies; 8 of the parts have STEP files next to them)
and 8028536 (the same L bracket as `.ipt`, `.sldprt` and `.f3d`). Owner rule: clean room. The
layouts below come from the sample bytes only, with `python3 -I` scripts that never execute
anything from a sample; no other reader, format write-up or SDK was read. The OLE2 layout is
Microsoft's published [MS-CFB] and the reader of ADR 0043 is reused.

## Decision
A new crate `gusset-ole` (`solidedge::extract_xt`, `inventor::extract_sab`) returns the B-rep
data inside the container; `import.file` hands it to `gusset_xt::to_step` / `gusset_sat::to_step`
and the sandboxed STEP importer (`Kind::SolidEdge`, `Kind::Inventor` in
`gusset-commands/src/import.rs`; extensions `.par`, `.psm`, `.ipt`). Tier notes
`TIER_SOLID_EDGE` and `TIER_INVENTOR`. The OLE2 reader (`gusset_sldprt::cfb`) is shared, not
copied: it is now `pub` in `gusset-sldprt` (a later cleanup may move it into its own crate).

### Solid Edge layout facts
* OLE2 compound file. A storage `PARASOLID` holds `STREAM0.X_B`, a complete Parasolid
  *transmit*: the usual text header (`**ABCDEF...`, `**PART1;` ... `APPL=SolidEdge;`
  `FORMAT=binary;` `GUISE=transmit;` `KEY=STREAM0;` `**PART2;` `SCH=SCH_3101255_31100;`
  `**END_OF_HEADER***`), then neutral binary (`PS\0\0\0` ...). It is exactly an `.x_b` file;
  `gusset-xt` reads it unchanged. Lengths are metres.
* Some parts have `STREAM0.P_B` (`GUISE=transmit_partition`, the same model as a partition)
  and `STREAM0.D_B` (deltas) instead of the `.X_B`. The `.P_B` reads like the `.X_B`; the deltas are
  ignored. `SKETCH.P_B` is the 2D sketches' partition and is ignored. The reader takes the lowest
  `STREAM<n>.X_B`, else the lowest `.P_B`.
* The schema key has no embedded base (`SCH_<modeller>_31100`), so `gusset-xt` uses its V12 table;
  that was enough for 13 of 15 parts.
* Assemblies (`.asm`) have an empty `PARASOLID` storage: no geometry of their own.

### Inventor layout facts
* OLE2 compound file; the document is the storage `RSeStorage` with pairs of streams `M<id>`
  (a small "RSe Meta Stream Version" stream) and `B<id>` (data), plus `UFRxDoc` and property
  sets. A `B` stream is 16 constant bytes (`9e c2 2b a4 d4 11 e8 01 60 00 2d b3 ee 29 fb b0`),
  two bytes (`04 01`) and **one zlib stream** (`78 5e`/`78 9c`/`78 da`) to the end of the stream.
  All 8-9 `B` streams of all 37 files inflate with no bytes left over.
* Two of a part's inflated streams embed a binary ShapeManager file, found by its magic
  `ASM BinaryFile4` (width 4; the samples say `ASM 224.2.2.65535`; Fusion's `.smb` uses width 8).
  Four bytes before the magic is a count (`2a 00 00 00`, varies with the file) and before that a
  record type `u16` plus `00 00`. The type tells the blobs apart: **`1d 03 00 00` (0x031d) is the
  part's result body; `57 03 00 00` (0x0357) is a second ShapeManager file with the feature
  history's intermediate bodies** (sketch sheets, the plate before the holes were cut, the
  removed hole cylinders; in the L bracket it holds 5 bodies of which 2 have zero volume). The
  0x0357 blobs must not be imported (they would add the intermediate bodies). In all 37 files
  there is exactly one blob of each type (37 + 37 blobs).
* The blob has no length prefix. It runs to its own `End-of-ASM-data` record. Some files end it with
  `End-of-ASM-History-Section` and `End-of-ASM-data` written as bare type names with **no record
  terminator** and then other binary data; `gusset-sat` now stops at the first completed
  `End-of-...` type name (before, the 7 parts that do this - the O-rings, the fibre rod and
  the glass-fibre insulators - failed with "unknown binary tag 0x05").
* The header's unit words are 10 as in Fusion; `gusset-sat` already converts them (mm in the output).
* Assemblies (`.iam`) have a result blob without faces (no bodies of their own).

### Limits (untrusted input)
At most `max_streams` (8192) `B` streams tried; each inflates under
`min(max_stream_bytes (256 MiB), deflated size x max_ratio (2048))`, all of them under
`max_total_bytes` (512 MiB); sector chains are walked with a step bound (the CFB reader of ADR 0043).
A stream that is not zlib after all is skipped. The magic scan is linear. Only the first result blob is
read (a warning counts the others). Tests: synthetic OLE2 containers with truncation and mutation
(`crates/gusset-ole/src/tests.rs`), noise, size/ratio/total limits, a commands-level test
(`inventor_and_solid_edge_parts_go_through_their_readers`) and a fuzz target `ole_read`.

## Consequences
Results (OCCT, `crates/gusset-cli/tests/ole_samples.rs`, env `GUSSET_OLE_SAMPLES` = the folder
with the sample folders):

* **Inventor, 35 of 35 parts import** as one solid each. The 8 parts with STEP files next to them:
  `side`, `side 2`, `back accses panel`, `bottom cover`, `bottom plate`, `bottom plate mod`
  agree with the STEP to a relative volume error below 3e-15 and 0.0000 mm bounding box
  difference. `top cover 2` differs by 4.3e-3 in volume (both STEP files of that name, `.STEP` and
  `.stp`, agree with each other to 3e-6, so the `.ipt` is another revision; same box) and `Top motor plate` by 2.4e-5
  (1.5 mm3, the area differs by 0.8 mm2, the box is identical). The L bracket differs from the SolidWorks
  version of the same record by 3.2e-5 (6.3 mm3 of 196766, the same box): two separately drawn models.
  Those three are reported, not asserted.
* **Solid Edge, 13 of 15 parts import** as one solid each (volumes 7 mm3 screw to 106 cm3 plate, boxes
  equal to the dimensions in the names where they have them; there are no STEP twins). The other two
  (`M1.2x3 screw`, `Plastic-handle-Loaded-Felfer-holder`) stop at **Parasolid node types 163 and 184
  of schema 31100**, which `gusset-xt` does not know (163 follows a B-surface's `SURFACE_DATA`; the
  files carry no embedded schema to read their layout from, and it was not derived from the bytes
  here). The error says so; nothing is skipped silently. The OCCT test lists them as expected failures.
* Time: about 0.5 s per part in a debug build.

### Not read
Feature trees and sketches, parameters, materials and colours, PMI, drawings. **Assemblies**
(`.asm`, `.iam`) are not imported. The evidence: the component file names are readable (Solid Edge:
UTF-16 paths in the `Attachments` stream, one per referenced file, with `name.par:1` instance
names; Inventor: UTF-16 paths in `UFRxDoc`), and Solid Edge's `PSMcluster0` contains 3x3 rotation
+ translation blocks of doubles (metres) that look like placements (5 to 9 per file), but they cannot
be tied to the instances from the bytes, and no assembly has a STEP twin of the same revision
to check a guess against. For Inventor no placement blocks (doubles or floats) were found at all
in the reactor assembly's streams. A guess would put parts in wrong places silently, so
none was made. `.psm` (sheet metal) is mapped to the Solid Edge reader without a sample.

## Alternatives considered
* A separate crate for the OLE2 reader: cleaner, more churn; deferred.
* Importing every ShapeManager blob of an Inventor file: adds feature-history bodies (see above).
* Skipping the 0x031d tag and judging blobs by content (solid with faces): fragile for the history blobs
  of one-feature parts, whose intermediate body equals the result.
