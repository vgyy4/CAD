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

Thicken (same amendment): the reader now emits `thicken` as a join extrude of the base block's side
outline (a rectangle sketch on the cap plane, `distance = -thickness`); the thickness is the parameter
under the face reference, the cap the thicken-tagged planar face lying exactly that far outside the
first extrusion's bounding box. It is only trusted through the whole-part rebuild check. The three
thicken parts now get past `Thicken1` and their full rounds (12, 12 and 8 tree fillets) are emitted,
but each still stops at a later feature (docs/formats/inventor-feature-tree.md I), so they stay bodies.


## Amendment (2026-11-10, 7): Inventor chamfer patches, slot-end rounds, spline surfaces

Chamfer feature faces that replaced no edge (corner patches between chamfers, vertex blend spline
surfaces, planar leftovers far from their pre-chamfer line) are skipped; a chamfer between a plane and
a parallel cylinder takes its edge as their intersection line. `Acis::kind` reads spline-surface
records and their `-ref` pointers. A concave full round (slot end) is emitted as a sketch plus a join of
the two corner slivers, because the kernel's full round does tabs only. Coverage is unchanged (25 of
47); the thicken parts now stop at a fillet whose supports no longer touch (`bottom plate` Fillet11)
and at a chamfer next to a spline-surface neighbour (`Top motor plate` Chamfer6); see
`docs/formats/inventor-feature-tree.md` J. The rebuild check (volume 2e-5) is unchanged.

Addendum (same day): a cylinder blend between a plane and a convex cylinder that do not meet (a consumed middle face, `bottom plate`
Fillet11) is emitted as a join of the blend's arc and a circumscribed polyline lying in the existing body (`Acis::bridged_round`,
`bridged_rounds`); the rebuild check is the only authority. `Top motor plate` Chamfer6 builds. The three thicken parts now stop at an
84-edge blend on the bottom tab ends whose picks do not match the edges split by the slot-end joins (notes J.2); coverage stays 25 of 47.

## Amendment (2026-11-13, 8): Inventor round ipt1 - a second corpus and the newer file versions

A second corpus of 1639 `.ipt` from 57 openly licensed sources (GitHub repositories, Zenodo records; kept with the
samples, never committed) was measured part by part with the corpus test. Most of it is newer than the first corpus,
and the blockers were layout changes of the newer versions, not missing features (notes K to Q of
`docs/formats/inventor-feature-tree.md`):

- **Zstandard** (K): newer files pack the B and meta streams as Zstandard (`04 02` in the B header). Unpacked under the
  same stream/ratio limits, decoder window at most 2^27. `gusset-ole` now depends on `zstd` (already a workspace crate).
- **Header word at 14** (L): version 2803 and later insert a zero `u32` at offset 14 of the common record header; such
  records are read with it removed, per class, when the file's first part feature shows it.
- **Solid body records** (M): part-feature records named through `656dd01e` (or by a `0ca9112d` name) are bodies, not
  features, and are skipped instead of making the history "unknown".
- **Blocks between records** (N): a separator byte 1 after a record's size is followed by a block; the walk resynchronises
  on the next record that frames (bounded), else it refuses as before.
- **ATTRIB_CUSTOM naming attributes** (O): `INV_NMX_BLEND_TAG` / `INV_NMX_EDGE_BLEND_TAG` read as `NMx_blend_tag` /
  `NMx_edge_blend_tag`.
- **Fillets with several edge sets** (P): one tree fillet per radius, each edge by its blend face's radius.

The rebuild check is unchanged. Results (parts as history; OCCT corpus test, one part per process):

| corpus | parts | before | after | import failures before | after |
|---|---|---|---|---|---|
| first (Zenodo, `GUSSET_IPT_MIN_HISTORY=25`) | 47 | 25 | 25 | 0 | 0 |
| second (57 sources) | 1639 | 263 | 367 | 219 | 46 |

No part of either corpus went from history to body. No run crashed or timed out (the two screw-rotor parts take 7-10
minutes in a debug build, in the body import). The 46 remaining import failures are 44 parts whose result body has no faces (the sketch-,
work-geometry- and surface-only fixtures among them) and two whose ShapeManager integer width is not read. In the second corpus, 412 parts have no
features (imported bodies) and 201 are derived parts (`8e29cbee`); the largest blockers left among the rest are extrude
extents 4/7/12 (to next / to a face / from a face, 79 parts), volume differences (68), picks the kernel does not find
(43), feature `90bada05` (33), sheet metal (30), shell (30) and chamfer types other than 0 and 1 (30).

## Amendment (2026-11-14, 9): Inventor round ipt2 - extents to a face, unequal chamfers, shells, sweeps

Round 2 on both corpora (notes R to Z of `docs/formats/inventor-feature-tree.md`). What the reader now does:

- **Extents 4 and 7** (to the next face, to a face): the end face is named through naming records that
  are not decoded, so the reach is measured on the faces the feature made in the result body (its id in
  the sweep-generated and generated tags) and emitted as a blind distance.
- **Chamfer type 2** (two distances), with the reference face from slot 8's first line; equal distances
  are an equal chamfer on any number of edges.
- **Hole variant 1** (countersink: diameter slot 3, angle slot 5); through holes with a counterbore or
  countersink keep it in the plain-cut fallback.
- **Records of an older save** carry their own version word after `0xffffffff`; their bodies start after it.
- **Shell**: thickness, side, and the opened faces found as the rims beside the shell's inner faces in the
  result body, their number checked against the feature's face list.
- **Sweep** along a 2D path sketch: the path curves are those lying on the path's wire body in the
  history blob.
- **Parts without a solid** (a result with no face) fail with a reason that says so (`INVENTOR_NO_SOLID`)
  rather than as a damaged ShapeManager file; `gusset-sat` reads the older `ACIS BinaryFile` form without
  an integer-width character.

Importer fix (gusset-commands, all tree imports): sketch points lying on a circle keep the circle's 8
decimals, so the region's arcs end on one circle ("could not build an arc of the profile", 44 Inventor
parts).

Every new reading is a candidate only: the rebuild check (volume 2e-5, box) is unchanged and decides.
Results (parts as history; OCCT corpus test, one part per process):

| corpus | parts | history before | after | import failures before | after | without a solid |
|---|---|---|---|---|---|---|
| first (Zenodo, `GUSSET_IPT_MIN_HISTORY=25`) | 47 | 25 | 25 | 0 | 0 | 0 |
| second (57 sources) | 1639 | 367 | 449 | 46 | 0 | 46 |

No part of either corpus went from history to body. One screw-rotor part (a body before and after, 14
minutes in a debug build when run alone) ran into the harness's 15-minute limit in the parallel run. The first corpus
gains nothing this round: its 22 bodies stop at blends whose edge is not recoverable, thicken-part
blends, the thumbscrew knurl and two kernel fillet failures (notes H to J). The next target in the second
corpus is the 111 volume differences (a per-feature comparison with the result body), then the 80 picks
with no model edge.

## Amendment (2026-11-14, 10): Inventor round ipt3 - copies, extrudes up to a face, pulley flanges

Round 3 on both corpora (notes AA to AH of `docs/formats/inventor-feature-tree.md`), led by a per-feature
comparison of the rebuilt history with the part's own result body. What the reader now does:

- **Circular patterns and mirrors** (AA): their records have classes of their own (`06977131`,
  `fad9a9b5`) and were left out of the feature walk without a word. They are read: copied features,
  count, angle and work axis; the mirror's work plane; whole-body copies. A through-all extrude with slot
  7 set goes both ways.
- **Extents 4 and 7 as extrudes up to a face** (AB, the review request of round 2): the face the extrude
  ran to is the one its faces meet off its sketch plane in the result body (plane, cylinder or cone),
  given as a face pick. The measured reach stays as the fallback: the importer rebuilds with the face
  first and, failing that, with the blind reach (the round 2 tree), and says so.
- **Rectangular patterns** (AC): two directions in one record (and the 3104 unit directions) are two
  patterns; an edge direction runs from the end the record names; midplane patterns of features are a
  pattern each way.
- **Pulley flanges** (AE): Design Accelerator flanges (`9a78add3`, parameters only) are revolves of the
  section their faces of revolution in the result body show.
- **Holes** (AF): slot 17 reverses the direction; holes without sketch-point centres take them from
  their faces in the result body.
- **Revolve axes** (AG) off the sketch's own axes become construction lines of the sketch.

Shared code: `gusset-commands` (Inventor import only: the fallback from extrudes up to a face to their
blind reach, and patterns of holes swapped for cuts copy the cuts); `gusset-occt-sys` (all importers and
the modeling command `to:`): an extrude up to a cone cuts at the cone extended past its face, and the
coverage check lets a strip under 1e-6 mm along the end's edges pass. No change in `gusset-sat`.

Every new reading is a candidate only: the rebuild check (volume 2e-5, box) is unchanged and decides.
Results (parts as history; OCCT corpus test, one part per process):

| corpus | parts | history before | after | import failures before | after | without a solid |
|---|---|---|---|---|---|---|
| first (Zenodo, `GUSSET_IPT_MIN_HISTORY=25`) | 47 | 25 | 28 | 0 | 0 | 0 |
| second (57 sources) | 1639 | 449 | 562 | 0 | 0 | 46 |

No part of either corpus went from history to body (the round-2 results are the reference). In the
second corpus the volume differences fell from 111 to 60 and the picks with no model edge from 75 to 33;
the 3104 rectangular patterns (12) and the revolve axes off the sketch origin (11) are all read. Each
blocker left is worth at most a handful of parts (note AH); the largest are an iPart family whose
stored parameters disagree with its body (7) and parts that start from an imported body (31, not a
rebuild).

## Amendment (2026-11-14, 11): Solid Edge feature history imports as editable features (round se1)

`gusset_ole::solidedge_features` reads an ordered `.par` part's history from `PSMcluster0` into the same
`gusset-feature-tree` v2 form the other native readers give, and `import.file` hands it to the shared rebuild check
(`solid_edge_history` in `gusset-commands/src/import.rs`, classified perfect / imperfect / none as for Inventor; tier
"the Solid Edge feature history as editable features"). Clean room, from the sample bytes and Gusset's own import of
each part's body (layout notes F10 of `docs/formats/solidedge-feature-tree.md`):

- objects: the longest rising run of `QQ kind serial` candidates; functions: type signature, class, slots (single or
  list references); the history: the longest chain of previous-feature links (slot 1);
- extruded protrusions and cutouts: profiles (slot 2), extent (13 finite, 14 through all, 16 through next swept as
  through all, 72 up to a key point at the reach the key point stores), side (1 against the profile normal, 2 along, 3
  symmetric, 6 both ways), depth (value object or dimension relation), a second extent (slots 12-14) as `distance2`;
- revolved protrusions and cutouts: axis (an absolute line object), angle (radians; 17 = full turn);
- profiles: plane (absolute frame in the plane object), 2D lines, circles, arcs and polynomial B-splines of the
  `Profile2d`; members kept only in the sketches' Parasolid partition `SKETCH.P_B` are found through their
  `SolidEdge/ProfileEntityInfo` attribute and brought into the profile plane (gusset-ole now depends on gusset-xt);
- cosmetic threads and the base-body feature of older parts are listed as left out (the rebuild check tells whether
  the base body held geometry).

Everything else (rounds, chamfers, holes, patterns, sweeps, lofts, thin walls, rational splines, pattern-copied
profiles) is an `unsupported` feature: such a tree is refused without running the rebuild (measuring a large part's
own body can take most of an hour in a debug build), its reason first among the import's warnings. Synchronous parts
keep no ordered history and stay body imports. The rebuild check is unchanged; no value is taken from the result body.

**Coverage (184 ordered parts: 182 of `samples/se-more`, 11 GitHub sources, plus schraube and halter of Zenodo
8028435; OCCT corpus test `crates/gusset-cli/tests/par_history.rs`, `GUSSET_PAR_SAMPLES`, floor
`GUSSET_PAR_MIN_HISTORY`).** 28 come in as history (was 0): schraube, the 15 Shass27 truss members, the servo tower,
the battery pack and 10 more; 154 come in as their exact body; 2 fail in the body import itself, as before this round
(the Parasolid worker is killed on one jam-cam part and crashes on another). One part (UFO_base) spends over an hour
measuring its exact body in a debug build (body import, not the history). First reasons of the 154: a feature kind not
read (96: rounds 29, multi-section sweeps 20, chamfers 13, thin walls 5, lofts 7, part copies 4, smart patterns 4,
others), a rational spline or ellipse in a profile (27), a pattern-copied profile (19), a profile curve stored nowhere
(4), a revolve extent not read (1), a cut whose body did not come in (2) and 5 rebuilds that differ from the part
(feature groups, later direct edits, a stored body of negative volume). Next: round and chamfer edges (topological references `SEGetSmoothEdgeSeq`, not
geometry), rational splines (weights after the knots), pattern copies (curves listed, plane to be found), sweeps.

## Amendment (2026-11-14, 12): Solid Edge round se2 - rounds, chamfers, copied profiles, open cuts, sweeps, lofts

`gusset_ole::solidedge_features` reads more of a part's history (layout notes F11 of
`docs/formats/solidedge-feature-tree.md`; clean room, from the sample bytes; every rule checked by exact rebuilds):

- **Rounds and equal chamfers.** Their edge set (the `JMTransmigrateAssocFunction` the feature refers to) keeps,
  after the edges' topological names, the curve each edge lay on when the feature was made: lines with their
  range, circles and arcs, ellipses, B-splines. Each becomes an edge pick: a line, circle or arc by its middle and
  0.1 mm of it either way (the importer's path matching finds the model edge holding that piece, a tangent chain
  being rounded whole by the kernel); an ellipse or spline by its middle and the whole edge as a polyline within
  2 um. The radius and setback come from the file; variable rounds and two-setback chamfers stay `unsupported`.
  The names themselves are not resolved: the stored geometry identifies the edges on the rebuilt model.
- **Profiles copied by a smart pattern** are the seed's 2D curves on the plane the copy stores (laid out as a plane
  object); **by a mirror**, the seed profile reflected in the mirror plane, its curves turned over.
- **Cutouts with an open profile** (one chain): closed on the side slot 5 names (1 left of the chain as drawn, 2
  right) by running the ends on to a circle 10 m round them and the arc on that side.
- **Up to a key point, symmetric** (side 3): the stored reach each way. **Revolve axes** off the importer's sketch
  axes: a construction line of the sketch.
- **Sweeps** of one cross section along one path (the section may be a model circle edge) and **lofts** through
  their sections in order (the ends' conditions not read: the rebuild check decides).
- **Profile members**: splines drawn through points (kind 2: same poles and knots), empty slots marked
  `0xfffffffd`, and relations or dimensions the profile lists with its curves (skipped).

Nothing derived from the result body: every value comes from the file; the rebuild check is unchanged (an imperfect
tree still falls back to the exact body). No shared code changed (`gusset-commands`, `gusset-xt` untouched).

**Coverage (184 ordered parts, OCCT `par_history`).** Before (round se1): 28 as history, 154 as bodies, 2 failed.
After: **56 as history** (the 28 and 28 more), 126 as bodies, the same 2 failed. First reasons of the 126: a feature
kind not read 43 (thin walls 15, smart patterns 4, part copies 4, helices 4, others), a round or chamfer the kernel
does not build 28 (mostly a radius as wide as the face it rolls over, which Solid Edge allows), a rebuild that
differs 15 (two of them stored bodies of negative volume), an ellipse profile curve 7, a round edge not found 5, 28
others.

**Body-import failures (unchanged, not in the reader).** Both jam-cam spindles keep a sheet body (Parasolid body type
3, beside an empty solid) whose two gear faces have 176 (Driver_Spindle) and 96 (Reel_Drive_Spindle) B-spline edges.
The worker reads the STEP that `gusset-xt` writes for it in 1 s; OCCT's shape healing then runs
`ShapeFix_Wire::FixSelfIntersection` on those wires: on Driver_Spindle the pairwise pcurve intersections run past the
import's 900 s limit (the worker is killed), on Reel_Drive_Spindle the healing corrupts the heap (`free()` of a
`Geom2d_BSplineCurve` in `BRep_CurveOnSurface`'s destructor under `ShapeProcess::Perform`; the worker aborts or
segfaults, in most runs, not all). Neither is a decoding error of ours; a fix belongs to the shared STEP reading
(healing settings for STEP that Gusset wrote itself, or an OCCT patch) and is left to the owner. **UFO_base**: its
STEP reads in under a second; the hour goes to measuring the exact body (`gs_mass` -> `measured_props`, OCCT's
Gauss-Kronrod to a relative 1e-9) over 8 B-spline faces that `gusset-xt` fits for Parasolid rolling-ball blends
(`BLENDED_EDGE`: 20 x 9 grid -> 58 x 25 poles, 152 C0 Bezier patches each, every patch integrated on its own);
the Reflector_Mounting_Bracket spends many minutes in the same integration. Both the fit and the integration are
shared code with exactness trade-offs documented in the shim: not changed here.

**Next.** Thin walls (face sets decoded up to the open faces' slot), ellipse profile members (71 bytes), protrusions
through all or to the next face (their reach), holes, part copies; rounds whose radius equals the face they consume.
