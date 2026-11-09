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
  here). The error says so; nothing is skipped silently. *Update (ADR 0035 amendment, 2026-11-09):*
  163 is the helical-surface and 184 the helix analytic form, read from SolidWorks samples; all 15
  Solid Edge parts now import and the OCCT test no longer exempts any.
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

## Amendment (2026-11-09): Inventor feature history, decoding in progress

The part's history lives in the `PmDCSegment` streams (records framed by size and class, a
class table in the matching meta stream): feature records with a type GUID (extrude, revolve,
hole, fillet, chamfer, rectangular pattern, thicken, thread), their history order, sketches
with a sketch-to-model matrix (cm), and feature parameters. Decoded from the sample bytes
alone (`docs/formats/inventor-feature-tree.md`; prototype in `scripts/re/inventor/`): 16 of
the 35 sample parts' trees rebuild exactly to the stored result body (volume within 9e-8,
box within 8.1e-5 of the diagonal). Not decoded yet: edge-naming records (fillet and chamfer
edges are taken from the result body's blend faces), pattern directions, thicken, ellipse
sketches. The Rust import follows in amendment 2 below.

## Amendment (2026-11-09, 2): Inventor feature history imports as editable features

`.ipt` parts now come in as a feature tree when it rebuilds to the part's own result body, as
SolidWorks (ADR 0043) and FreeCAD (ADR 0033) histories do; otherwise the exact body (tier 2) with the
reason as a warning. The Python prototype (`scripts/re/inventor/`) is ported to Rust, clean room, from
the sample bytes and `docs/formats/inventor-feature-tree.md` only.

**Code.** `gusset_ole::inventor_features::feature_tree(bytes, file_name, limits)` returns the
`gusset-feature-tree` v2 JSON or an error reason. Modules (private except the entry point):
`inventor_dc` (the `PmDCSegment`: meta and `B` streams, class table, framed records, references,
feature list and history order, parameters, placements), `acis` (a topology-level ShapeManager
reader: loops of a body as sampled polylines; blend faces of the result body and the edge each
replaced), `inventor_profile` (which regions of a sketch a feature's loops select: a 500-cell grid
flood fill). `inventor_history` in `gusset-commands/src/import.rs` is wired like `solidworks_history`
(`TIER_INVENTOR_HISTORY`, the `commit_import` override) and ends in the shared `rebuild_check`
(volume within 2e-5, box within 1e-4 of the diagonal, nothing skipped).

**Features read.** Sketches (mask-compressed 4x4 placement in cm, points, lines, arcs, circles,
construction flags), extrude (new/join/cut; distance, symmetric, through all, reversed), revolve (full
or angle), hole (simple/counterbore, blind/through, drill point angle, several centres), fillet (one
radius) and chamfer (equal distances). Fillet and chamfer edges are not stored as geometry in the
file: the result body's faces tagged `NMx_blend_tag` with the feature's id are found and the original
edge is rebuilt from the faces beside them (plane/plane lines, plane+coaxial cylinder circles for
torus and cone blends) and passed as an edge midpoint, which is what the kernel can pick by.
Corner patches (spheres) are skipped.

**Left out, and said so.** A cosmetic thread (no geometry) and a feature suppressed in the source
(flag bit 2 of its header; absent from the result body) are not emitted; their names are listed in
the import's warnings ("left out, no effect on the body") and in the tree's `source.omitted`. Any
other kind (thicken, rectangular pattern, ...) or a record that does not read is emitted as
`unsupported` with the reason, so the rebuild check refuses the tree and the reason reaches the
warning ("the design history came in as an exact body: the fillet feature F cannot be read: ...").

**Hole choice (deterministic).** Gusset refuses some holes the part has (the kernel's hole feature
needs a planar start face and a clean cylinder). The reader attaches to each hole, once a body exists,
`_fallback`: the same hole as plain cuts (a through hole as a circle sketch cut through all, a blind
one as one revolve cut per centre with the counterbore and drill tip in the profile). The importer
rebuilds with all holes as hole features; when a hole is the first skipped feature it is swapped for
its fallback and the rebuild repeats (one swap per round, in history order); if the failure is not a
skipped hole (volume or box differ) it tries all holes swapped once. The outcome depends only on the
file and the kernel, never on timing. The warning says how many holes are plain cuts.

**Bounds.** Streams and inflated sizes by `Limits`; at most 1 000 000 records, 4096 list items, 2000
features, 20 000 curves per sketch, 4 million ShapeManager records and 40 million tokens; every
reference, offset and chain is checked (no unwrap, expect or indexing; no recursion); the profile grid
is fixed. The `ole_read` fuzz target runs the reader on raw bytes, behind the compound-file signature
and as records cut from the input inside a well-formed segment; unit tests mutate every byte of a
synthetic part.

**Coverage (35 sample parts, OCCT test `crates/gusset-cli/tests/ipt_history.rs`,
`GUSSET_IPT_SAMPLES`, floor `GUSSET_IPT_MIN_HISTORY`).** 16 of 35 come in as history: AL2O3_tube,
ALUMINUM_sleeve, FIBERGLASS_rod, GLASSFIBRE_isolation_inner/outer, GLASS_tube, PIA_flange_inlet/outlet,
TITAN_flange_inlet/outlet, TITAN_nut_big/small, TITAN_pressure_ring_big/small, Base Plate, side 2 (the
prototype's 16, the same parts). The Rust trees of all 35 parts equal the prototype's to 1e-6, apart
from the wording of the labels of unsupported features and the cosmetic or suppressed features Rust
leaves out. The other 19 stay exact bodies, with these reasons: 4 O-rings (an ellipse
sketch curve the tree cannot say, so the revolve's profile is open), 3 thickens and 1 rectangular
pattern (not in the tree format), 9 fillets or chamfers whose edge cannot be rebuilt or picked (2
cylinder blends without two planar supports, 1 torus corner, 1 chamfer with no tagged blend face, 5
whose rebuilt midpoint is not an edge midpoint of the model at that step), 1 fillet the kernel fails
on (`bottom cover`), 1 extrude whose boolean the kernel rejects (`LED Arm`). Time: about 3 s per part in a debug build
(the rebuild is the cost).

**Not done.** Edge-naming records (`375c6982` ids), pattern directions, thicken, ellipse sketch curves,
chamfers with two distances or an angle (read as equal distances; the rebuild check refuses them if
wrong), multi-body parts, assemblies.

## Amendment (2026-11-09, 3): Inventor round 2 - edge picks, cylinder blends, rectangular patterns

The prototype's second round (`docs/formats/inventor-feature-tree.md`, "Round 2") is ported to
`gusset_ole`. **20 of 47 sample parts** now come in as history (was 16): the 16 above plus L-Bracket,
side, Conecting plate 1 and back accses panel (OCCT test `ipt_history.rs`, floor
`GUSSET_IPT_MIN_HISTORY=20`).

- **Picks.** A fillet or chamfer edge is `{midpoint, path: [m - 0.1 mm t, m + 0.1 mm t]}` with `m` on the
  true pre-fillet edge (not the edge's middle after later cuts), so the importer's path matching finds
  the model edge whatever its extent at that step. A whole circle (fewer than two distinct vertex
  angles) is picked at its seam midpoint (centre - R * ref), decided from the result body alone: the
  prototype flips to the seam only after the importer skipped the feature; here the rule is fixed
  and no rebuild feedback is used. An arc that was a whole circle before a later cut split it is
  still picked at the arc's middle (the prototype's flip rescues those in parts that do not match
  anyway).
- **Blend faces.** `circ_mid` normalises angles to [0, 2pi) and merges angles under 1e-6 (the +-pi
  bug); planar faces that carry a fillet's tag are dropped; `NMx_edge_blend_tag` faces (circle
  chamfers) count; cylinder blends take any two supports (planes parallel to the axis, cylinders
  parallel to it) and intersect them in the cross-section; torus, cone and other blends first try
  the circle between two coaxial neighbours (planes, cylinders, cones, tori as profile curves).
- **Holes.** The plain-cut fallback's revolve axis is the profile's closing line (`{"line": id}`);
  the importer's sketch origin is the foot of the world origin, so an origin+direction axis was refused.
- **Rectangular pattern** (record `20673244`). Patterned features, direction `a244457b` (run against
  -V), count `bb1dd5df`, spacing parameter, a second direction chained as a pattern of the originals
  and the first pattern, and the midplane flag (`90874d28`, the last one) as a shift of the original
  hole by -(n-1)/2 * spacing (holes only; any other midplane pattern is `unsupported`).
- **Not done.** Thicken (not proven by an exact rebuild), ellipse sketch curves (no ellipse in the tree
  format: 4 O-rings), asymmetric chamfers of the M8 screw, full-round fillets, rolling-ball and vertex
  blends, the knurl pattern of the thumbscrews.

## Amendment (2026-11-09): Solid Edge feature history, decoding in progress

A part's history is in `PSMcluster0` (objects framed by `QQ`, a type index into `PSMtypetable`
and a rising serial; references by serial), with its profiles as wire bodies in the
`SKETCH.P_B` Parasolid partition. Decoded from the sample bytes alone
(`docs/formats/solidedge-feature-tree.md`; prototype in `scripts/re/solidedge/`): the feature
chain (protrusion, cutout, revolved cutout, hole, thread, pattern), depths and profiles. Of
the 15 sample parts only 2 are ordered-feature parts; the other 13 are synchronous parts,
which keep no extrude or revolve history (their body is all there is). 1 part rebuilds
exactly. Missing: reference-plane records, hole placement, patterns, revolve angles, and more
ordered-feature samples. `.par` stays tier 2.

## Amendment (2026-11-09, 4): Inventor distance-angle chamfers

The Inventor importer reads the chamfer type (slot 4 record, `u16 2, u16 type, u32 0`; 0 equal,
1 distance and angle) and, for type 1 on a single edge, emits `angle` (slot 10) and `face` (a point
on the support face the first distance runs along: the end of the first line of the slot-8
reference geometry, whose length must equal the distance). Other types, a first line of another
length and unequal chamfers on several edges stay refused. The M8 screw (2 chamfers, 30 deg) now
rebuilds within the volume check: 21 of 47 sample parts come in as history (was 20). Open: the
two-distances type value and which record names the face are inferred from that one part.

## Amendment (2026-11-09, 5): Inventor ellipses; what still blocks history

Sketch ellipses (`4507d460`, the O-ring grooves) are read as tree `ellipse` curves (centre point,
unit major direction, semi-axes; see `docs/formats/inventor-feature-tree.md` D). The four O-rings
now rebuild exactly: 25 of 47 sample parts come in as history (was 21). The 22 left as bodies:
4 are standard-part exports with no feature records, 3 thicken parts and 1 more need a
full-round fillet (a blend whose two supports are parallel planes at twice its radius: the
pre-fillet edge no longer exists, and a Gusset fillet with exactly half the gap fails in OCCT,
so the tree needs a "full round" feature), 6 reach a rolling-ball or vertex blend (spline
surface) that cannot be turned into a straight pre-fillet edge, 3 differ by about 1 % in volume
(the thumbscrew knurl: ~32 notches that are not in the extrusion's stored profile), 3 name an
edge the analysis cannot place, 1 uses extrude extents not decoded, 2 are kernel fillet
failures. No new tree feature was needed except full-round.

## Amendment (2026-11-09, 6): Inventor full-round fillets

A fillet feature whose blend face is a cylinder tangent to two parallel planes on opposite sides of
its axis (apart by twice the radius) is a full round (`Acis::full_round`). The reader emits
`{"type": "fillet", "full_round": {"point", "normal"}}` (ADR 0042 amendment): `normal` is the
outward direction of the middle face, `point` its crest (axis plus radius times the normal, at the
middle of the half cylinder along the axis). The side is found from the supports' vertices (the
body lies behind the crest). One feature with several middle faces becomes one tree fillet each
(`Name`, `Name_2`, ...). A full round mixed with ordinary edges of the same feature stays refused.
Result: the full-round fillets of `Long Bottom Nut Traps` rebuild in Gusset; the part still comes
in as a body (see `docs/formats/inventor-feature-tree.md` I). Coverage stays 25 of 47.

