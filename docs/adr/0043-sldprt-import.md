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
colours and display lists, the preview image. Assemblies (`.sldasm`) and drawings
(`.slddrw`) are not read. Parts whose final partition is empty keep their bodies in
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
