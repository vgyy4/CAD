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

## Amendment (2026-11-14, 13): Solid Edge round se3 - thin walls, tapers, ellipses, revolve sides

`gusset_ole::solidedge_features` reads more of a part's history (layout notes F12 of
`docs/formats/solidedge-feature-tree.md`; clean room, from the sample bytes; every rule checked by exact rebuilds):

- **Thin walls** become shells (wall inside the body, side value 4; the one part with 5 stays `unsupported`): the
  thickness, and the faces they open from the face set in slot 4. A face set lists plane faces with their
  boundary loops as they were when the feature was made; each becomes a face pick of points inside it on its plane.
- **Tapered extrudes** (slot 17: `173`, angle, 4 widening or 5 narrowing) carry `taper_deg`; the slot was ignored.
- **Revolve sides** name the way the profile moves (2 along its normal, 1 against), as for extrudes; the turn's sense
  follows from where the profile lies about the axis. Round se1's fixed reading (side 1 a negative turn) held only
  where a positive turn carries the profile along its normal.
- **Open revolve profiles** close against the axis (both ends on it), or for cutouts with one end on it round a box on
  the profile's side of the axis, on the side slot 5 names.
- **Ellipse members** (71 bytes) and `SKETCH.P_B` ellipses become ellipses and elliptical arcs.
- **Protrusions through all or to the next face** along a world axis reach where the box the feature stores (the part
  after it, part of the feature's own record) ends; cuts keep the long reach.
- Synchronous parts whose only functions are patterns report no ordered history (as those with only holes).

The through-all reach is the only value not given as a feature parameter: it is the extent Solid Edge stored with
the feature itself, not taken from the result body. The rebuild check is unchanged (an imperfect tree still falls
back to the exact body). No shared code changed (`gusset-commands`, `gusset-occt-sys`, `gusset-xt` untouched).

**Coverage (184 ordered parts, OCCT `par_history`).** Before (round se2): 56 as history, 126 as bodies, 2 failed.
After: **62 as history** (none lost; new: 033 and 034 strap cuts (revolve side), 037, 114 and 161 (thin
walls, with an open revolved cut in 114 and a tapered base in 161), 072 (protrusion through all)),
120 as bodies, the same 2 failed in the body import.

**What blocks the rest, by cause (first reasons of the bodies).**

- *Rounds and chamfers the kernel does not build* (about 30). Measured by rebuilding each part with only the failing
  blend's radius reduced by 2 %: only 5 then build (026 and 105, a concave round as tall as the face it consumes;
  071, 094, 167). Of these only 105 would then match its part (volume within 1e-4 at the reduced radius); 026's
  stored body is half the features' volume and the other three fail at later blends. The rest fail at any radius: concave rounds on the intersection
  curves of a pipe sweep and a block (the eight R-4D pipe parts, the largest single group), a round that crosses a
  window in its face (008), and single cases. A face-consuming fallback in the shared fillet chain would gain at
  most that one part, so the shim is not changed (no Fusion corpus check needed).
- *Feature kinds not read* (24): part copies 4, helix protrusions 3 (+1 helical curve; a helix's pitch and turns
  are in slots 8 and 9 but its curve is stored as a spline, not decoded), body patterns, mirror copies, fast
  mirrors, holes and simplified base features 2 each, others 1. Each is worth at most 2 to 4 parts, and most of
  those parts have further unread features.
- *Rebuilds that differ* (15): per feature, against the box each feature stores: cuts to the next face swept through
  all (027, 028), through-all protrusions that stop at the first face inside a hollow part (093, 095), loft end
  conditions not read (039, 068), a round OCCT builds with material outside the part (067), stored bodies of
  negative volume (068, 140), and differences no stored box shows (035, 042, 153, ...).
- Thin walls: 13 of the 15 parts' thin walls are read (one has side 5, one comes before its part's first body
  because its revolves are not read); 3 parts gained. The others stop elsewhere: rounds after the wall (041, 050,
  080), the tea cup's handle sweep whose path starts 2.2 mm off its section plane (023), an invalid OCCT shell of a
  dome (111), three pipes whose stored bodies keep only part of their sweeps (054, 056, 060), a stored body of
  844 mm3 for 17289 mm3 of features (163), and features not read before or after the wall (011, 012, 014, 017,
  145, 162).

Each remaining cause is worth fewer than 4 parts except the kernel rounds on sweep intersections (8, all one
source) and the unread feature kinds taken together.

## Amendment (2026-11-14, 14): the spindle and UFO body imports

The body-import failures of amendment 12 had two causes, neither in OCCT's healing as such.

**Spindles: a sense error in `gusset-xt`.** Parasolid writes many edges on a `TRIMMED_CURVE` (sense `+`)
whose basis curve has sense `-`; the trim runs from `parm_1` to `parm_2` the way the basis does (all 25,274
trimmed curves of the corpus: a `+` basis runs up, a `-` basis down). STEP gets the basis curve, and the
edge took the trim's sense alone, so every tooth's tip and root arc of the jam-cam gears became its
complement (6.2 rad instead of 0.06). Their faces were tangles of overlapping arcs, and
`ShapeFix_Wire::FixSelfIntersection` intersected every pair (Driver_Spindle: past the 900 s limit) or
corrupted the heap while repairing them (Reel_Drive_Spindle: `malloc` aborts when `ShapeFix_Shape`'s
`ShapeBuild_ReShape` is freed, 3 runs in 5; the path follows address order: no fault under valgrind, gdb
or an AddressSanitizer build of TKShHealing in 21 runs, so no OCCT patch was made). The senses now
compose (`model::curve_sense`). The bodies were never an empty solid: each file holds the gear solid
(141 and 95 faces) and a one-face sheet (a disc) beside it. Both now read in under a second as a valid
solid (1251.7 and 577.3 mm3) plus the sheet. 92 of 1728 corpus transmits have such edges; read as the
worker reads them, 16 invalid shapes become valid and backside.SLDPRT (killed before) reads.
SPS_Injector, whose sphere face `gusset-xt` writes with edges up to 1.4 mm off it (a separate defect,
next), now stalls in the same healing step instead of reading as an invalid solid of negative volume.

Healing settings for STEP that Gusset writes were measured and left as they are: without
`FixSelfIntersectionMode`, 25 of 1102 corpus shapes that read valid become invalid (none the other way) (the fix moves vertices
and tolerances onto the fitted intersection curves), so the step is needed. A time budget on the transfer
(a progress indicator whose `UserBreak` fires after a deadline) does stop it, but any budget below the
worker's limit would change imports that now finish near it; the worker's limit stays the bound.

**UFO_base and Reflector_Mounting_Bracket: an OCCT bug** (ADR 0002, patch `gusset-0006`). The fitted
rolling-ball blends are u-periodic B-splines; OCCT's healing gives some of their faces pcurves one period
on, or a seam off the first knot, and `BRepGProp_Face::GetUKnots` then found no knot in the face's range,
so `measured_props`' Gauss-Kronrod ran over all 152 C0 patches at once: about an hour, and 1e-5 off. With
the patch the knots repeat over the periods: UFO_base measures in 3 s, the bracket in 1.2 s (the four
parts import in 28 s together). A face's value agrees with an independent integration over the exact
spans to 5e-9.

**Open.** `query.body` measures a sheet with the closed-shell formula (the spindles' discs read -760 and
-226 mm3); SPS_Injector's sphere edges.

## Amendment (2026-11-18, 15): Inventor round ipt4 - coils, embosses, combines, splits, sheet-metal faces

Round 4 took the features the reader did not export (notes AI to AN of
`docs/formats/inventor-feature-tree.md`, with a table of what each unexported GUID is). The largest,
`8e29cbee` (201 parts), is a derived part or iPart member: the file holds only the body of another file,
so there is nothing to rebuild. What the reader now does:

- **Coils** (`b41914da`, AI): a helical sweep of the profile (ADR 0034) with the kind, hand, axis
  direction, pitch, height and turns read; flat ends, tapers, spirals and sections across the axis are
  refused. The coil parts of both corpora stop at other features, so none gains yet.
- **Embosses and engraves from the face** (`285e398c`, AJ): an extrude of the profile by the depth,
  against or along the stored direction.
- **Combines** (`c0236733`, AK): the bodies are those the features made (their `0e6870ae` body lists); a
  feature that makes a combine's tool keeps its own body.
- **Splits** (`8371a8ba`, AL): a trim is a split at its work plane and the removal of one side; split faces
  are left out.
- **Sheet-metal faces** (`55585fc6`, AM) of unfolded plates: the sketch is found from the loop's history
  body, the face is an extrude by the thickness on the side the result shows.

Only `gusset-ole` changed (Inventor reader); no shared code. Every new reading is a candidate only: the
rebuild check (volume 2e-5, box) is unchanged and decides. Results (parts as history; OCCT corpus test,
one part per process):

| corpus | parts | history before | after | without a solid |
|---|---|---|---|---|
| first (Zenodo) | 47 | 28 | 28 | 0 |
| second (57 sources) | 1639 | 562 | 574 | 46 |

No part of either corpus went from history to body. The 12 gained: 3 combines, 3 embosses, 2 splits, 4
sheet-metal faces. Lofts were tried (the sections found as for the sheet-metal faces) and left out:
Inventor pairs a square and a circle differently from a ruled loft (7e-5 in volume).

## Amendment (2026-11-18, 16): Inventor round ipt5 - derived parts and iPart members from their base part

The largest refused group of round 4 (203 parts, `8e29cbee`) are derived parts and iPart members: the file
holds only the body of another part. Their base parts are mostly in the same folder (30 of the 31 names in
the second corpus). Decision: such a part is rebuilt as its base part's feature history, followed by the
derive's placement (mirror, move, scale) and the features the part adds after it; the unchanged rebuild
check against the part's own stored body decides, as for any import. Layout notes: AO of
`docs/formats/inventor-feature-tree.md`.

The base is read from another file, so the lookup is narrow (the same rules as SolidWorks assemblies
reading their parts, `gusset-commands/src/import.rs`):

- only by the plain file name the part stores (no separators, drive prefixes, `.`/`..`, control
  characters; `.ipt` only), and only if listing the part's own folder shows exactly that name;
- opened without following links (`open_regular`), as a regular file, under the same size cap and cancel
  flag as the import itself, and read with the same limits;
- at most `MAX_DERIVE_DEPTH` levels of bases; a part that names itself, or a chain that returns to a part
  already read, is a cycle and refused;
- a missing or unreadable base is a reason naming it; the part then comes in as its body, as before. A part
  that derives nothing reads exactly as before, and no other format is affected.

An iPart member stores its row as its own parameters under the factory's names; the factory is read with
the member's values. The factory stores its sketches solved for its saved row only, and the reader does not
decode sketch dimensions, so a member whose row changes a sketch dimension does not rebuild (the rebuild
check reports the difference); members that change only feature values (extrude distances etc.) do.

| corpus | parts | history before | after | without a solid |
|---|---|---|---|---|
| first (Zenodo) | 47 | 28 | 28 | 0 |
| second (57 sources) | 1639 | 574 | 585 | 46 |

No part went from history to body. The 11 gained: 5 AN/MS fitting members, 2 aluminium channels, 2 sample
parts, a mirrored terminal and one derived architecture part. Most of the remaining members need their
factory's sketches solved again for their row; 22 derives name a same-named file elsewhere, and 5 Design
Accelerator parts name a component without a file.

## Amendment (2026-11-18, 17): Inventor round ipt6 - iPart members solved for their row

Amendment 16 left iPart members whose row changes a sketch dimension of the factory as bodies (71 parts):
the factory stores its sketches solved for its saved row only. The reader now decodes the sketch's
constraints and dimensions (their classes, entities and parameters, note AP of
`docs/formats/inventor-feature-tree.md`) and, for such a member, solves the factory's sketch again with
`gusset-sketch` (`gusset_ole::inventor_sketch`; `gusset-ole` now depends on `gusset-sketch`, which
depends only on `libm`):

- The saved geometry is the starting shape and tells how each dimension measures (side, which angle,
  sign); the member's values replace the saved ones. Reference (driven) dimensions drive nothing.
  Projected points stay where they are unless the row moves what they show.
- The solve must keep Inventor's shape: a result that runs a line, or a dimensioned point pair, the other
  way is refused and the next statement of the sketch is tried (lines along the axes held first, then
  free; angles of lines that end tangent on an arc by the arc's radius line; such a line shrunk to
  nothing). Every attempt is a fixed, ordered list, so the outcome is the same every time.
- Later sketches that project model edges follow only when each projected point is an earlier sketch's
  point swept by an extrude or turned by a revolve; a sketch on a face moves with its origin when the
  origin is such a point. Otherwise the feature is refused with the reason.
- The profile picks are made on the saved sketch (the history body's loops show it) and found again on
  the solved one by the curve pieces around each region.
- A member placed exactly as its factory gives its own stored body to the fillet and chamfer readers (it
  carries the factory's feature ids).

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4): it decides.
Members that change no sketch dimension, and every other part, read as before.

| corpus | parts | history before | after | without a solid |
|---|---|---|---|---|
| second (57 sources) | 1639 | 585 | 622 | 46 |

No part went from history to body. The 37 gained: 35 AN/MS fitting members (AN924 11, AN806 10, AN871 7,
MS20819 7) and the two iPart members of `iPartSample` (MetaReader samples). Left among the 71: the AN816
family (its factory does not rebuild itself), AN818 (an offset work plane the row moves, and a sketch that
projects intersections of faces), 4 MS20819 members whose shrunk line leaves the solve stuck and one
AN806 member 0.07 % off in volume.

## Amendment (2026-11-18, 18): Inventor round ipt7 - owned names, body moves and scales, blend picks

The 74 target parts of this round stopped at an unknown `FeatureN` (34) or at a fillet or chamfer pick
that no model edge matched (40). Measured first (notes AQ of `docs/formats/inventor-feature-tree.md`):

- Every unknown `FeatureN` is a feature whose name word points at a record of another class
  (`903f453f`, `48cf71ca`, `b6482af8`, `475e7861`); its name is the typed name record whose owner word is
  the feature. The first such feature of each part: 20 surface features (extrusion, revolution, sweep
  and offset surfaces; 11 of them the Design Accelerator's "Pitch Diameter"), 9 direct-edit scales, 1
  direct-edit move, 1 direct-edit size, `co_mesh`'s `Surface2` (`e34d8f79`) and, in the gun box, a hole
  (its pattern then refused a cosmetic thread).
- The pick failures have many causes, each in one to four parts: a torus blend trimmed by a later cut
  across the axis (the screws' slots) read the cut's floor as a support; an arc that crosses a revolved
  face's seam is two edges in Gusset's model and one in Inventor's; supports used up by the blend or by a
  later blend (a 3 mm wall rounded 2 mm at both ends, a chamfer as wide as its step); a feature the
  order record put first by a stray byte match; and, in about a third, an earlier feature that leaves the
  model wrong (an iPart row, sketch geometry) so the blend has nothing to find.

Decisions:

- A feature named through its owned name record is read as its type says. Surface features make no
  solid and are left out with the reason; a later feature that needs the surface refuses on its own.
- Move Bodies (`2b839fe9`) and the direct edits Move (`8fa6a6de`) and Scale (`5bcb6fff`) of whole bodies
  become `move_body` / `scale_body` (existing Gusset operations). The scale's matrix keeps the base point
  in the shift's place (the `de_scale` sample: the scaled block's top face centre). A scale that also
  turns (the chess set) and a move of faces are refused with the reason.
- Blend edges: a trimmed torus blend's supports are the neighbour curves it touches, once each (a sphere
  on the axis counts); the importer takes the two model edges that meet at a pick's midpoint and run
  either way along its path (a seam split); a mirror that copies a hole and the pattern of it makes the
  pattern again of the copies; a rectangular pattern skips a cosmetic thread that was left out.
- The order record is read by its entries (`0x80000003`, id); the loose reading stays for a record
  whose entries are not all found so.

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4).

| corpus | parts | history before | after | without a solid |
|---|---|---|---|---|
| first (Zenodo) | 47 | 28 | 28 | 0 |
| second (57 sources) | 1639 | 622 | 635 | 46 |

No part went from history to body. Of the 74 targets 6 came in (`de_scale`, two screws, the gun box and
its old version, `CoverStyrofoam`); 7 more parts outside them (5 by the order record). Left: the Design
Accelerator gear tooth features `73d72559`/`36628967` (9 parts; Inventor's tooth profile with its
corrections and root fillets would have to be generated exactly, which Gusset's gear operation does not);
6 chess parts (a scale that turns, of bodies imported without features); surfaces consumed by thicken,
trim, stitch, replace face and delete face (no Gusset operation reads them as features); 32 pick failures
of the kinds above, 4 fillets that the kernel cannot build (the M1.4 screws: a lemon-shaped torus), and 2
parts with a scale whose features do not give the part's volume (`pedal`, `Arm_3`: the box matches, the
volume is 3 to 6 % off; the cause is not found).

## Amendment (2026-11-18, 19): Inventor round ipt8 - imported base bodies, one-way patterns, splines, holes to a plane

The 95 target parts of this round failed with a volume difference (70) or with "a cut or join before any
body" (25). Measured feature by feature against the stored body (notes AR of
`docs/formats/inventor-feature-tree.md`), by cause:

| cause | parts | outcome |
|---|---|---|
| the part starts from an imported body (a solid body record before every feature) | 33 (22 early cuts, 11 read as volume differences) | not rebuildable: the file keeps that shape only inside the result; the reader says so |
| Make Components part (no derive feature, base not named) | 3 | not read |
| profile picks: sketch splines not read, a small hole under the grid, a loop listed twice | 5 | 4 come in |
| holes ending at a plane or face (extent 7) | 4 (+1 iPart member of one) | 3 and the member come in |
| one-way rectangular pattern: a stale internal count, fit spacing | 3 | patterns right; the parts still differ elsewhere |
| a new solid kept apart; a body mirror that removes the original | 2 | both come in |
| extrude taper (slot 5) | 4 (ISO 7046 screws) | neither sign gives the stored volume: not read |
| sketch text on studs; a scale; a join through all; up-to-face on a face too small | 4, 2, 1, 2 | left (no Gusset operation for text profiles or "through all" joins; up-to-face needs a covering face) |
| within 1e-4 to 2e-4 of the volume (revolves, small cuts) | 7 | cause not found |
| single parts (a region pick, a fillet set, a twisted sweep, a hole across a face edge, derived bases) | the rest | left |

Decisions:

- A part whose first body is a solid body record no feature made is refused as a whole with that reason
  (no history part of either corpus has one). Nothing is faked from the result.
- Sketch B-splines (`317b7346`, clamped, degree/knots/weights/poles) become tree `bspline` curves; their
  inner poles are construction points. A sketch with one is not solved again for an iPart row.
- Profile picks count all loops of the feature (one wire per loop); only when that leaves no region do
  the loop bodies count one by one. A loop with no pick is looked at again on a grid over its own box.
- One-way rectangular patterns take the count after the body list and read the first flag after the
  direction as fit spacing; the stored direction is the run direction.
- An operation-1 extrude or revolve whose body list names a new body stays a body; cuts and joins change
  the bodies their list names (else the first body). The second flag of a body mirror removes the
  original (`merge: false` and `remove_body`).
- Holes with extent 7 end at slot 13's work plane or, without one, at the depth their faces reach in the
  result. Extrude slot 2 against the sketch normal turns the sweep. Sketch lines shorter than 1e-6 mm are
  left out of the tree.

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4).

| corpus | parts | history before | after | without a solid |
|---|---|---|---|---|
| first (Zenodo) | 47 | 28 | 28 | 0 |
| second (57 sources) | 1639 | 635 | 648 | 46 |

No part went from history to body. Of the 95 targets 11 came in (`Tilting head`, `Upper cover of robot
head`, `dock_base`, `leica_under_board`, `ex_newsolid`, the Testrig mirror half, `EPR_tube`, `Nieudane`,
`AN816` and its member `AN816-8D`, the older gun body); 2 more outside them (an ISO 4162 screw and
`Sampledurchführung`).

## Amendment (2026-11-18, 20): Solid Edge round se4 - chamfer angles, key point turns, mirrors, helices, holes, variable rounds

`gusset_ole::solidedge_features` reads more of a part's history (layout notes F13 of
`docs/formats/solidedge-feature-tree.md`; clean room, from the sample bytes):

- **Setback-and-angle chamfers** (kind 42): the setback along the reference face (a face set), the
  angle from it; with one reference face per edge, one chamfer per face. Face sets now also hold
  **cylinder faces** (type 23), picked halfway up a band between two circles about the axis.
- **Revolves up to a key point**: the angle the `JMAngularKeyPointFunction` stores.
- **Mirror copies of features** (`JMMirrorCopyFunction`, `JMFastMirrorFunction`): a Gusset mirror of the
  feature copied, built again reflected.
- **Helix protrusions and cutouts**: a sweep along a helix (ADR 0034 path) of the cross section about the
  stored axis, its length and turns; the stored boxes of a screw flight agree.
- **Holes**: a cut of the hole circles (each hole profile's 2D member stores centre, diameter and circle),
  through all or to the depth; a blind hole's drill point (its angle) is a revolved cone cut below each
  circle. Counterbores and countersinks are not decoded: the rebuild check tells.
- **Value objects shared by several features** (a depth for a pattern of holes, a point angle with a unit)
  were taken for dimension relations and gave nothing; read now.
- **Variable rounds** (kind 2) become variable-radius fillets (ADR 0061 `vertex_radii`): vertex sets with
  a radius each. No part of the corpus reaches one in a rebuild (the one part with them fails earlier at
  a boolean), so this reading is unconfirmed by a rebuild.
- **Sketch splines** that a profile keeps only in `SKETCH.P_B` (`B_CURVE` edges). Shared code: `gusset-xt`
  gains `b_curve` (a `B_CURVE`'s degree, poles, weights, flat knots); nothing else outside `gusset-ole`
  changed.

Round kind 17 (three parts) is still not read: in none of them does the rebuild reach the round, so the
difference from kind 1 is not confirmed. The rebuild check is unchanged (volume 2e-5, box 1e-4).

**Coverage (180 ordered parts, OCCT `par_history`, one part per process).** Before (dd5b4a8): 63 as
history. After: **71 as history**, none lost; new: 076 (key point revolve), 102 and 106 (angle chamfers),
155 and 156 (mirror copies), 157 and 183 (holes; 183 is `halter fuer probnhalter`, whose 8 patterned
holes have drill points), 172 (sketch splines). 109 bodies.

**What blocks the rest (first reasons of the 109 bodies, measured).**

- *Rounds the kernel does not build* (about 40): fillet failed 23, radius too large for the edge 10 (OCCT
  fails, the message is Gusset's guess), round edge not found 5, chamfers 3. Probed by rebuilding up to
  the failing round with each edge alone and with the radius scaled: e.g. 107's round on a revolved
  B-spline face fails alone and at half the radius; 100's on the sides of a 12 degree revolved cut fails
  on every edge alone; 170's r60 round of a screw flight's end (Solid Edge's blend runs below the flight)
  fails. These are OCCT fillet limits, not decoding errors.
- *Feature kinds not read* (16): part copies 4 (the copied part is another file), body patterns 2 (a
  plane or profile and stored faces; layout not decoded), fast patterns 2, simplified base features 2,
  wraps 2, smart boss, delete faces, helical curve, projected curve 1 each.
- *Rebuilds that differ* (10) as in amendment 13 (cuts to the next face, which Gusset cannot express:
  there is no "up to the next face" end; loft end conditions; through-all protrusions in hollow parts),
  and 5 whose stored body measures 0: `gusset-xt` cannot convert some `BLENDED_EDGE` faces, the shell is
  left open, and since daee02a an open sheet adds no volume (was a meaningless number before).
- Round kind 17: 3; through-all reach along a slanted normal: 2; open revolve profiles with no end on the
  axis: 2; single cases.

**A kernel crash.** 040 now reaches its rebuild and OCCT segfaults inside `BRepFilletAPI_MakeFillet::Build`
(`BRepBlend_SurfRstConstRad::Values` -> `Geom2dAdaptor_Curve::EvalD1`) on its r381 round. In the app the
kernel runs in its worker and the crash is reported; the in-process corpus test dies, which is why it now
runs one part per process. Not patched (an OCCT fillet bug; owner's call whether to chase it).

## Amendment (2026-11-19, 21): round op2 - rounds the OCCT fillet does not build

The rounds of amendment 20's "what blocks the rest" are kernel work, not decoding: the OCCT kernel
now builds more of them from their corners (ADR 0004 amendment 2026-11-19: cliff edges where a face
is narrower than the round, mitred corners of two rounds, corners that end on a slanting or curved
face, a round whose contact runs along another edge, a straight edge between a plane and a cylinder
along it). Every fallback is checked exactly against its corners' volume; the rebuild check is
unchanged (volume 2e-5, box 1e-4).

**Coverage (180 ordered parts, OCCT `par_history`, one part per process).** Before (9c4efcc): 71 as
history. After: **76 as history**, none lost; new: 026 and 105 (a concave round at a step across a
ring, trimmed by the ring's cylinders; the result matches the stored body to 1e-9 when measured
by Gauss-Kronrod, 4.9e-6 by the default quadrature), 040 (the r381 round now made from its corners,
no OCCT call: the crash is gone), 053 and 065 (cliff-edge and mitred rounds).

**What blocks the rounds left (first reasons, measured).** fillet failed 21 (was 23), radius too large
for the edge 7 (was 10), chamfers 3, round edge not found 6 (was 5: 094 now builds its r12.7 cliff
round, whose centre equals the stored body's blend cylinder, and stops at Round_4, whose edge is
6.35 mm from any edge of the rebuilt model). Measured causes of the others: a tab 0.13-0.22 mm high
on a bent tube rounded by 0.381 (8 parts: 081-090; the ball would roll on the torus against the
tab's top edge, a blend OCCT does not build, each edge alone fails); a lug on a dome whose round ends
on the sphere next to the sphere's edge with the cylinder (133-135; one edge fails alone, its mirror image
builds); B-spline and revolved supports (045, 063, 100, 107); rounds wider than a lobe of the
outline (177, 178); a window under a round's contact line (008); a hole rim on a face 0.15 mm wider
than the hole (016); hole rims rounded wider than the plate (110, its corners not all material).

## Amendment (2026-11-19, 22): Solid Edge round se5 - fast and body patterns, curves, ends to the next face

`gusset_ole::solidedge_features` reads more feature kinds (layout notes F14 of
`docs/formats/solidedge-feature-tree.md`; clean room, from the sample bytes):

- **Fast patterns** (`JMFastPatternFunction`): the pattern's own sketch holds a
  `JPattern2dFunction` (the circle, count and angle between copies); read as a Gusset circular
  feature pattern about the sketch normal through the circle's centre. A group entry stands for
  a feature (a fast mirror) patterned with the others.
- **Body patterns** (`JMBodyPatternFunction`): laid out as a fast pattern, they copy the whole
  body; one on a reference plane mirrors the body in it.
- **Curves and face splits** (projected, wrapped and helical curves, split faces) are left out
  with a note: they add or remove no material, and the rebuild check still decides.
- **A second extent up to a key point** (as the first's).
- **Extent 16 (to the next face)** is written as Gusset's new end up to the next face (ADR 0061
  amendment: `to: next`, kernel `extrude_to_next`), the old through-all reach given to fall back
  on; both ways as two features.

Shared code: the kernel, document, evaluator, commands and tree import gain the end up to the
next face (ADR 0061 amendment); nothing else outside `gusset-ole` changed. The rebuild check is
unchanged (volume 2e-5, box 1e-4). Inventor (678 of 1686) and SolidWorks (293 of 1352) come in
as before, every part with the same outcome.

**Coverage (180 ordered parts, OCCT `par_history`, one part per process).** Before (b901843): 76
as history. After: **80 as history**, none lost; new: 138 (two fast patterns, 4 and 5 copies),
151 (a body mirrored in a reference plane), 073 (a projected curve and a split face left out),
029 (three wrapped sketches left out). 100 bodies.

**The end up to the next face gains no part.** Every part with a cut to the next face that came
in as history still does (026, 033, 053, 073, 076, 077, 105); none of those that differ changes
outcome: 027 and 028 differ at their first such cut because the cut's region also takes tabs
Solid Edge keeps (a profile or side question, not the end); in 042 (a tube, holes from planes
tangent to it) Solid Edge's result lies between cutting the near wall only (2.1e5 mm3 too much
material) and cutting through all (9.7e4 mm3 too little); 093 and 095 are through-all
protrusions (extent 14, with a second extent) in hollow parts, which the reader does not write
as ends up to the next face. Measured, so the rule stays as decided in ADR 0061's amendment.

**What blocks the rest (first reasons of the 100 bodies, measured).**

- *Rounds and shells the kernel does not build* (34): fillet failed 21, radius too large for the
  edge 6, round edge not found 4, a face with no usable origin 1; thin walls (shell failed) 2
  (111, and 145 now that its patterns are read).
- *Rebuilds that differ* (17): 12 by volume (above, and 035, 039, 068 loft ends, 153, 163's
  first revolve, 020's Protrusion_4 that makes a body the part does not keep, 174's sweep after
  its 16-fold body pattern), 5 whose stored body does not measure (open `BLENDED_EDGE` shells).
- *Feature kinds not read* (8, was 16): part copies 4 (the copied part is another file),
  simplified base features 2, a smart boss 1, delete faces 1.
- Synchronous parts 5; round kind 17: 3 (read as kind 1, 051 and 058 reach their rounds, which do
  not build: still unconfirmed, not read); profile reading singletons (members kept elsewhere,
  open revolve profiles, through-all reach along a slanted normal, ...) about 20.

## Amendment (2026-11-19, 23): Solid Edge round se6 - ends through all, mirror seeds, drafts

`gusset_ole::solidedge_features` reads more of a part's history (layout notes F15 of
`docs/formats/solidedge-feature-tree.md`; clean room, from the sample bytes):

- **Ends through all** (extent 14) are written as ends up to the next face (ADR 0061 amendment),
  as extent 16 already was, the through-all reach kept to fall back on. Measured on the tank
  parts: a protrusion through all stops at the first face it meets (093, 095: the stored bodies
  hold a boss only between the profile plane and the dome) and a cut through all both ways
  takes only the wall it starts in; both rebuild to 1e-9 so, and 027's cuts match too. A finite
  second extent becomes a blind feature of its own; along a slanted normal (no reach in the
  stored box) the reach through all is the fall-back. Every part whose ends through all came in
  before still does (040, 072, 104).
- **Mirror copies of several features**: the copied profile's seed is the mirrored feature whose
  profile lists the copy's first member (the reader took the first listed): 152, 028 and 092
  rebuild (028's "tabs Solid Edge keeps" of amendment 22 were mirrored tabs on the wrong plane).
- **Drafts** (`JMAddDraftAssocFuntionEx`, so spelt): a Gusset draft of the stored faces about the
  neutral plane; the history chain no longer breaks at them (018, 066, 080 lost the features
  before). 080's way 4 matches its stored boxes flipped; way 5 is the other sense, unconfirmed.
- **Open profiles**: a revolved cutout's chain with both ends off the axis runs on from both
  ends (091); a protrusion's open chain is closed between its ends, which lie on the body (055).
- **Whole ellipse members** (a circle's 55-byte layout with another axis or ratio: 166, 173),
  **thin walls outside** the body (side 5: 166), **helix pitch** (slot 7; the hand is slot 6:
  109) and **pattern copies** whose frame end carries a flag byte (109's flats).

No shared code changed (the gusset-xt converter was tried for negative-major-radius tori, which
091's and 011's stored bodies lose: the faces then convert but the lemon trims wrongly, 2.6e11
mm3; reverted). The rebuild check is unchanged (volume 2e-5, box 1e-4).

**Coverage (180 ordered parts, OCCT `par_history`, one part per process).** Before (3f31fe2): 80
as history. After: **86 as history**, none lost; new: 027, 093, 095 (ends through all), 028,
092, 152 (mirror seeds). 94 bodies. 055, 080, 091, 109, 166 and 173 now read every feature and
stop later (at rounds, a chamfer or a boolean the kernel does not build).

**What blocks the rest (first reasons of the 94 bodies, measured).**

- *Rounds and chamfers the kernel does not build* (37): fillet failed 24, radius too large for
  the edge 9, chamfers 4 (now 109's too); round edge not found 4 (050 and 061 after features whose stored boxes
  agree; 136's profile takes a member whose store object is a line of another sketch while
  `SKETCH.P_B` keeps the ellipse it means, keyed by member and sketch; 173 after a loft along a
  helical curve), a face with no usable origin 1; thin walls (shell failed) 2.
- *Rebuilds that differ* (14): 9 by volume - lofts with end conditions (038, 039, 068: every
  feature before agrees with its stored box), a cut to the next face in a tube (042, as
  amendment 22), 035's inner skin, 020's last protrusion (its stored box is not a box of the
  part), 153, 163 and 174 (B-spline profiles whose stored boxes are their poles' and say
  nothing); 5 whose stored body does not measure (open or inside-out shells after conversion).
- *Feature kinds not read* (9): part copies 4, simplified base features 2, a smart boss, delete
  faces, a web network.
- Synchronous parts 5; round kind 17: 3; booleans the kernel refuses 4 (091's hoops, 160's
  five 135-degree revolves of one pentagon, 171, 175); sweeps the kernel refuses 2 (023's
  profile sits at a joint of its path: two sweeps from the joint would do; 169); single
  profile cases about 13 (members kept only in another sketch or not at all: 010, 012, 015,
  031; sweeps along model edges or of two sections: 052, 137, 168; lofted cutouts of curves
  not read: 069, 070; a draft face set with a cylinder band not bounded by circles: 018; an
  open revolved protrusion: 144; a revolve that crosses its axis in the kernel: 115; a cut
  before any body: 176).

## Amendment (2026-11-19, 24): Solid Edge round se7 - loft end conditions, circles paired by length

`gusset_ole::solidedge_features` reads more of a part's lofts (layout notes F16 of
`docs/formats/solidedge-feature-tree.md`; clean room, from the sample bytes and the stored
bodies' loft faces):

- **Loft end conditions**: slots 11 and 14 are the start's and the end's codes, 113 free and 20
  square to the section with the tangent magnitude in slot 13 or 16. Written as Gusset's new
  loft end tangents (ADR 0017 amendment: `start_tangent` / `end_tangent`, a multiple of the
  distance between the two profiles' centres; text, command, tree import, kernel and worker
  request, golden test). Solid Edge's stored faces are cubic across the sections with the inner
  rows a third of the tangent from the sections and a free end without bend, which Gusset now
  builds; their length scale is 0.2-0.3 % longer than the centre distance (a Parasolid rule
  not recovered), so the rebuilds agree to 7e-6 (038, 039) of the part's volume, inside the
  unchanged check. Codes other than 20 and 113 (none in the corpus) are refused.
- **A circle lofted to an outline** of lines and arcs is split into arcs in proportion to the
  outline's edges, from the outline's start round its sense, and the loft keeps the sections'
  starts (`aligned`): 068's stored faces split its circle 80/100/80/100 degrees against a
  rectangle; OCCT's own pairing (at the corners' polar angles) was 110 mm3 off, now 2.2e-6.
- **A cut before any body** is left out with a note (176's first cutout removes nothing); 176
  then stops at a profile member kept in no sketch.

Shared code: the kernel (`LoftSpec`, the OCCT shim's loft), the worker protocol (version 13),
the document (`LoftDef`, text, validation, grammar), the evaluator, the loft command and the
tree import. The rebuild check is unchanged (volume 2e-5, box 1e-4). Inventor (678 of 1686) and
SolidWorks (293 of 1352) come in as before, every part with the same outcome.

**Coverage (180 ordered parts, OCCT `par_history`, one part per process).** Before (21aacdc): 86
as history. After: **89 as history**, none lost; new: 038, 039 (loft end tangents), 068 (an end
tangent and a circle paired by length with a rectangle).

**Tried and measured, not kept.**

- *160's booleans* are an OCCT defect, reproduced outside Gusset: two 135-degree revolves of one
  regular pentagon, each about one of its edges (adjacent edges), share the pentagon as a start
  face and a cone apex at the common vertex; OCCT's general fuse returns an empty shape (the
  builder of solids cannot orient the split faces; four `BOPAlgo_AlertUnableToOrientTheShape`),
  common gives nothing and each cut gives its argument back, at every fuzzy value from 1e-7 to
  1e-2; revolves about edges two apart (144 degrees) hang the boolean. Converting the solids to
  B-splines gives a union, but one whose volume moves by 2e-5 with which argument is converted.
  Not fixable in Gusset's use of OCCT without an OCCT patch.
- *111's thin wall*: OCCT's thick solid is invalid; the revolved-cavity fall-back (ADR 0004)
  refuses its section (concave corners off the axis from the revolved groove). 145's body is
  polyhedral with dozens of small features: the offset is invalid too.
- *059*: its first round on a tab rounded on both sides leaves a sliver face without history
  (an origin from the face it lies on names it); the next round, two fillets of half the tab's
  width, is a full round of the tab's top, but the base rounds already fill the sides, so a full
  round leaves a step Solid Edge does not have. Both reverted.
- *061*: Round_2's edges (the bosses' corners at x = +-101.6) are not edges of the rebuilt body
  while every stored box agrees; the boss's side face continues past them, so an earlier
  feature (a cut to the next face or the bosses' extent) differs where the boxes cannot tell.

**What blocks the rest (first reasons of the 91 parts not as history, measured).** Stored bodies
that do not measure (BLENDED_EDGE faces gusset-xt cannot convert, tori of negative radius, SP
curves): 40, among them most of the round failures (081-090, 133-135, 177, 178, ...), 050 and
136. Of the 51 that measure: no feature history 5, feature kinds not read 6, round kind 17: 3;
fillet or chamfer failures 10 (008, 044, 055, 094, 110; chamfers 007, 019, 112; radius too
large 167, 170); rebuilds that differ 10 (020, 035, 042, 153, 163, 174 by volume; 054, 056, 060,
140 whose features give no volume); booleans and sweeps the kernel refuses 3 (160 above; 171, a
smooth loft through three spline sections whose stored faces are a C2 cubic spline across the
sections, knots 0, 0.5 and 1 (an interpolating spline with uniform parameters, free ends), which
bulges about 4 mm further than OCCT's smooth loft, and whose next join is invalid: a Gusset loft
built that way generalises this round's end tangents and is the next step there; 169); round edges not found 2 (061, 173) and 059; thin walls 2 (111,
145); profiles 9 (012, 015, 018, 052, 069 and 070 - thirty lofted cutouts each, a feature group
owner listed among a profile's members -, 144, 168, 176).
