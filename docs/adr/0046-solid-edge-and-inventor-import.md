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

## Amendment (2026-11-19, 25): Inventor round ipt9 - face planes, tapers, extents from a face, members on faces

Measured first: the first failure of each of the 1008 parts not as history, then, for the ones whose
features build, a per-feature comparison with the stored body (scratch, not committed). Most failures are
not rebuildable from the file (422 parts have no features, 64 are derived from parts not in their folder,
43 start from an imported body); the rest spread thinly. Decoded this round (notes AS of
`docs/formats/inventor-feature-tree.md`):

| rule | parts gained |
|---|---|
| iPart member sketches on faces move across their plane with the earlier points in it; hole centres and hole end planes follow the row | 19 (AN816 members) |
| extrude taper (slot 5, radians, positive widens); a tapered join to the next face runs 10% past its corners into the material behind the end face | 5 (ISO 7045 screws) |
| extent 4 with no face left in the result: the tree's "to next" | 4 |
| extent 12: the distance from the face plane slot 17 keeps (the sketch moved onto it) | 3 |
| extent 7 with no face left in the result: the face plane slot 18 keeps (`151280f0`), parallel to the sketch | 2 |
| revolve slot 5 = 2: half the angle each way (the sketch turned back by half) | 2 |
| sketch arcs of radius 0 left out; arcs whose ends meet written as circles | 1 |
| sheet-metal face side from slot 2 (0, 1, 2 = both ways) instead of the result | 0 (the 15 parts now stop at folds) |
| lofts through the sketches of their section loops (ruled for two) | 0 (`lf_basic` 7.4e-5 off) |

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4).

| corpus | parts | history before | after | without a solid |
|---|---|---|---|---|
| first (Zenodo) | 47 | 28 | 28 | 0 |
| second (57 sources) | 1639 | 650 | 686 | 46 |

No part went from history to body (one body part timed out under load: the MoS2 rig's `SRM`, 607 s alone
as before). Only Inventor reader code changed (`gusset-ole` Inventor modules): the Solid Edge and SolidWorks
corpora were not rerun.

What blocks the rest, measured: blends 111 (no model edge at a pick 37); volume differences 50 (ISO 7046
recesses that differ before their taper, LEGO stud text, a solve that flips a groove fillet to the other
side of its chord in AN815-28D, AN816-28D and -32D, revolves about 1e-4 off); Design Accelerator gears and
"Solid1::" derives of a part with the part's own name 32; sheet-metal folds and bends 16; AN818 (points
projected from face intersections, an offset work plane the row moves) and MS20819 members 13; surfaces and
direct edits 15; shells whose open faces count differently 9; twisted coils 4; lofts 3.

## Amendment (2026-11-19, 26): Solid Edge round xt1 - stored bodies that convert

Of the 91 parts not rebuilt as history after se7, 40 had stored bodies that gusset-xt did not
convert to a closed solid, so no rebuild of them could be checked. ADR 0035's amendment of the
same day converts them: rolling-ball and cliff-edge blends from their true spine, SP-curves
across blends, lemon tori of negative major radius, contact curves of blends refined onto
the ball's contacts (measured causes and rules there).

**Conversion (the 40 stored bodies, OCCT's STEP read and `BRepCheck`).** Before: none a valid
closed solid (open shells, OCCT volumes from -2.1e8 to 3.7e8 mm3; the rebuild check saw NaN or
an open body). After: **36 valid closed solids**, with no warning but bodies the reader leaves
aside (010 and 014's mesh body, 011 and 016's empty body, 031 and 109's wire body). Left
invalid: 011 (faces on its lemon dome keep self-intersecting parameter wires after OCCT's
healing), 050 (a closed contact curve on a closed offset B-surface keeps the chart's fit, 2 mm
off, because the refined ring makes OCCT's healing throw), 137 (a B-surface face bounded only
by a ring on the surface's own edge, area 0), 166 (OCCT's healing throws on an offset surface,
`Geom_RectangularTrimmedSurface::U parameters out of range`, and drops the parameter curves).

**Coverage (180 ordered parts, OCCT `par_history`, one part per process).** Before (se7): 89 as
history. After: **89 as history**, none lost, every other part with the same first reason
except 067, whose rebuild is now compared: the features give 5638513.24 mm3, the stored body
5638334.80 (3.2e-5, the check is 2e-5; its four cliff-edge rounds are not what OCCT's fillet
builds). The same conversion measured on the other formats: SolidWorks 294 of 1352 as
history (was 293, none lost; `Lock Collar`'s stored body measured 662.7199 mm3 against its
features' 662.6970 and now agrees within the check), Inventor 678 of 1686 as before (no
Parasolid there; every part with the same outcome).

**What blocks the 40 now (first reasons, measured).** Round features OCCT does not build: 27
(fillet failed 19, radius too large for the edge 7, an edge not found 1); 19 of them are parts
whose stored body has cliff-edge blends, where Solid Edge's ball rolls along an edge that
OCCT's fillet has no mode for (a cliff-edge round in the kernel is the next step for these).
Other rebuild failures 5 (023 sweep path, 091 and 175 booleans, 109 chamfer, 115 revolve),
feature kinds not read 3 (011, 014, 066), profiles 2 (010, 031), lofts and sweeps 2 (050's
loft of cross sections with end tangents, 137's sweep of cross sections), 067 above.

## Amendment (2026-11-20, 27): Inventor round ipt10 - two-direction extrudes, chamfer picks, row steps, sketch region tolerances

Measured first as in round ipt9 (each body part's first failure, then each prefix of the rebuilt history
against the stored body; scratch, not committed). Decoded or fixed this round (notes AT of
`docs/formats/inventor-feature-tree.md`):

| rule | parts gained |
|---|---|
| extrudes in two directions (slots 27 to 29: second distance, taper, extent); a mirrored copy flips both sides | 4 (gun handle and magazine, two revisions each) |
| an iPart row's sketch solved in 8 (then 32) steps from the saved values when the direct solve swings an arc to the other side of its chord | 3 (AN815-28D, AN816-28D, AN816-32D) |
| a blend on edges an earlier cut broke up is retried on the whole edge before that cut | 4 (CNS 9671 M1.4 screws) |
| chamfers on both halves of one circle are one edge; a chamfer on one edge is picked where its reference lines start | 5 (ISO 4766, ISO 7436, the Content Center screw, the M10x30 coupler screw) |
| a blend face without a vertex is left to the feature's other faces | 1 (servo motor cover) |
| sketch regions: segments that part by less than a tenth of the tolerance over their length overlap instead of crossing somewhere along the stretch; circles whose centres are their radii's sum or difference apart within the tolerance touch at one point; curves leaving a point within 1e-6 rad of one another are ordered by curvature | 6 (Tapadera, Locking Plate, the planetary gearbox port, Flange motor side, Winkel 90_CU, the arm's bracket cover) |

The three sketch fixes are in `gusset-sketch` (the region arrangement every importer and the sketcher
use): Inventor's stored points sit 1e-9 to 1e-12 off the exact tangency or line that a solve leaves, and
each slip lost a region. The rebuild check is unchanged (every feature builds, volume within 2e-5, box
within 1e-4).

| corpus | parts | history before | after | without a solid |
|---|---|---|---|---|
| first (Zenodo) | 47 | 28 | 28 | 0 |
| second (57 sources) | 1639 | 686 | 709 | 46 |

No part went from history to body (the MoS2 rig's `SRM` times out under load, a body part either way).
The changes reach `gusset-commands` and `gusset-sketch`, which SolidWorks and Solid Edge share: both
corpora were rerun and lost nothing. SolidWorks 315 of 1352 (314 on this round's base; `GT2_Bobbin_v2` gained
by the sketch fixes); Solid Edge 90 of 180 measured ordered parts (89 before; `008.par` gained).

What blocks the rest, measured on the 902 bodies: no features 422; derived from a part not in the folder or
in a cycle 66; imported base bodies 48; features the reader does not export (Design Accelerator gears,
"Solid1::" self-derives, sheet-metal folds, delete face, face drafts, stitches) 95; blends about 99 (no
model edge at a pick 39: a fillet rolling over an edge, earlier features that differ from the file's, a
stale extrusion parameter in AN816-7D); volume differences 46 (`lf_basic` and its kin: Inventor's ruled
loft is not OCCT's, 7e-5; ISO 7046 recesses; LEGO text); sketch regions 17 (curves 0.04 mm short of their
neighbours, sketch text); AN818 (a work plane offset `d16` the row moves, circles projected from the
revolve) and MS20819 members (the larger rows' constraints conflict, also when reached in steps) 13;
shells whose open faces count differently 11 (most also hold a face draft); lofts 11; coils 6.

## Amendment (2026-11-20, 28): Solid Edge round se8 - cliff-edge rounds in the kernel

Of the 37 parts whose first unbuilt feature was a round after xt1, the commonest geometry was
a family of eight (081-090): a tab 2.54 x 0.22 mm standing on a swept tube (a torus face),
its foot rounded by 0.381 mm, wider than the tab is thick. Solid Edge's stored bodies hold
three cliff-edge blends there (`blend_type` `E`, the ball rolling on the torus and on the
tab's top or side edge) that meet one another along their intersection curves, and a second
round (R blends between those blends and the tab's back) on top. OCCT's fillet has no mode
for a ball that touches an edge; the kernel now builds these corners itself (ADR 0004
amendment 2026-11-20: rows of the ball's arc at stations along the far edge, lofted; corners
whose far edges meet run on past that vertex until their blends cross).

Measured per round failure (the OCCT shim's corner on each first failing round's input, kept
as dumps in scratch): cliff corners built for 041, 055, 081-090, 110; refused where the
narrow face's far side is several edges or ends at the round's own vertices (008, 044, 100,
177, 178), where no face is narrower than the ball (OCCT failures of plain rounds: 016, 043,
045, 057, 063, 064, 071, 080, 101, 107, 133-136, 166, 167), and where the far edge's ends do
not line up with the round's (170).

**Coverage (180 ordered parts, OCCT `par_history`, one part per process).** Before (xt1):
89 as history. After: **96 as history**, none lost: 081, 082, 085, 086, 087 and 090 (their
rounds now build, volume within 1.0e-6 of the stored bodies) and 094 (its concave round of a
low wall, which `cut_corners` had mitred into a plain corner, is the cliff corner now).
Changed first reasons, still not history: 083 and 088 now stop at their second round (r 0.381
on 0.381 mm edges of the tab's back, OCCT fails), 041 at a later round (an edge not found),
055 and 110 at rounds whose cliff corners leave faces with no origin. (174 hit the import time
limit at a sweep once while two other rounds' imports shared the machine; alone it measures
as before, 3 minutes, its features' volume differing from the stored body's.)

**Other formats (the kernel change reaches every importer).** SolidWorks 333 of 1352 as
history, as before, none lost (two parts rebuild one more feature before their first failing
fillet); Inventor 714 of 1686, as before, none lost (one part's first reason changed the same
way; one body-only part hit the 900 s limit at an unrelated coil while other rounds ran).

**What is left among round failures (measured).** Narrow faces whose far side is not one edge
(5), plain rounds OCCT does not build where no face is narrow (20 rounds, mostly corners of
three or more rounded edges and B-spline faces), cliff corners leaving faces without origins
(2), a round whose contact leaves its face only near an end (067's pipe tee, 3.2e-5 off), both
faces narrower than the ball (101, 016).

## Amendment (2026-11-20, 29): Inventor round ipt11 - sheet-metal folds, face names, face drafts, sweep wire paths

Measured first as in rounds ipt9 and ipt10 (each body part's first failure on this round's base,
then each prefix of the rebuilt history against the stored body; scratch, not committed). Decoded or
fixed this round (notes AU of `docs/formats/inventor-feature-tree.md`), all in `gusset-ole`'s
Inventor reader:

| rule | parts gained |
|---|---|
| sheet-metal folds (`4c37ffe8`) as the tree's `bend`: angle, radius, bend line, side and direction against the face's outward normal, zone place, K-factor 0.44; folds sketched on an earlier fold's flange | 10 (omicronrobot's frames, fronts, sides, brackets) |
| a sweep path given as a wire body (`fc203f47` naming a `cc0f7521` history body): the sketch whose curves the wire runs along | 1 (FIRST1939 bumper cover) |
| face names (`509fb5cc` -> `03aa812c` -> naming table `cce92042` -> `0645c2a5`: feature id, profile curve, cap or side); face drafts (`f4fa0900`) of planar faces and extruded circles, the neutral plane where each face's plane before the draft meets its plane in the result | 2 (`dr_fixed_plane`, TipHolder) |
| loops matched to their sketch's lines, circles and arcs exactly (sampled chords lie 0.09 mm inside an arc of 160 mm radius) | 0 (two lofts reach later features) |
| shells whose rims do not count their open faces place them from the faces' names | 0 (five parts reach later features) |
| one-edge chamfer reference lines more than 1 cm off the edge read are not used | 0 (the bay adapters' Chamfer3 builds; they stop at Mirror2's copy of it) |

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4). Only
`gusset-ole`'s Inventor modules changed: no code SolidWorks or Solid Edge share, so their corpora were
not rerun.

| corpus | parts | history before | after | without a solid |
|---|---|---|---|---|
| first (Zenodo) | 47 | 28 | 28 | 0 |
| second (57 sources) | 1639 | 709 | 722 | 46 |

No part went from history to body (full run of the 1686 at the face-draft commit, then the parts
whose trees the last two commits change). Two parts time out under load (a body part either way).

What blocks the rest, measured on the 890 bodies of the second corpus: no features 411; derived from a
part not in the folder or in a cycle 40 or more; imported base bodies 43 (and 5 via their base part);
volume differences 47; features the reader does not export about 80 (Design Accelerator gears,
"Solid1::" self-derives, delete face 8, sheet-metal bends between plates `4cf758f2` with corners and
hems, ribs, stitches); blends about 110 (cylinder blends with one usable support 9, no model edge at
a pick, chamfer and fillet failures); folds whose bend zone holds a step of the outline (Inventor's
bend relief) 3; sketch regions and loft sections about 20; face names whose curve number does not land
on a line in larger sketches (the numbering counts entities the reader does not read: EY4180 A-00,
A-07).

## Amendment (2026-11-20, 30): Solid Edge round se9 - closed cliff corners, concave arcs, unnamed slivers

Measured first (all 180 ordered parts, one per process, on this round's base: 97 as history, as
after se8) and then, for each part whose first failure is a round, that round's input body against
the OCCT shim's fallbacks (dumps and tools in scratch, not committed). Fixed this round, all in the
OCCT shim (ADR 0004 amendment of this date):

| cause (measured) | parts | outcome |
|---|---|---|
| a closed cliff corner (a ring whose side is lower than the radius) lofted once leaves its two end sections in the body as a slit of faces with no origin | 110 (cut), 055 (filled) | both history (055 within 2.1e-6 of the stored body) |
| a sliver OCCT's fillet splits off a trimmed face is named by its history neither as modified nor generated | 059 | its first round builds; it stops at its second (r 1.27 on 2.54 mm edges) |
| concave arcs round a cylinder had no corner (the foot of a boss with rounded corners, filled to the boss's full height) | 071 | the round builds; the features give 2592362.32 mm3, the stored body 2592281.62 (3.1e-5, the check is 2e-5) |
| (the closed-loft change) | 057 | its 127 mm round builds; it stops at a later cut that removes all material |

**Coverage (180 ordered parts, OCCT `par_history`, one part per process).** Before: 97 as history.
After: **99 as history** (055, 110), none lost. The kernel change reaches every importer, so the
other corpora were rerun in full: SolidWorks 370 of 1352 as history, as before, none lost (three
parts rebuild further before their first failure); Inventor 750 of 1686, as before, none lost and
no other outcome changed (two parts that crashed while another run filled the disk measure as before
alone).

**What is left among round failures (measured on each first failing round's input).**
- 133-136 (four parts, one geometry: a box on a capsule, eight edges rounded): OCCT's fillet fails
  at one vertex where the round must run on over the sphere and the cylinder past the box's
  side, while the mirror-image vertex builds; mirroring, rotating, healing, moving seams,
  splitting edges and other fillet parameters do not change that, and the corners there (three
  rounded edges at the box's front corners) need OCCT's vertex blends. 136 first stops at its
  round's edge: its profile's right lobe, an ellipse in the sketches' partition, is read as an arc.
- 083, 088: three rounded edges meeting at the tab's back corners on B-spline faces; OCCT's
  result has self-intersecting wires.
- 100: a cliff corner whose far edge's ends do not line up with the round's (the strip's ends
  are cut square to the axis, the edges are 60 degree arcs); 170 the same.
- 044: a D-shaped narrow face (far edge ends at the round's own vertices); 044 also has failing
  chamfers.
- 177, 178 (one geometry), 101, 016, 063, 064, 166, 167, 043, 080, 107: plain rounds OCCT does not
  build with no face narrower than the ball, or both faces narrower.
- 061: a later round's edge is split differently in our model (no edge at its midpoint);
  041 likewise at a later round.
Other blockers are as after se8: part copies (004, 005, 009, 013) and simplified base features
(014, 142) keep the copied or simplified body only in the result, so they are not rebuilt; 020's
Protrusion_4 is not in the stored body at all (the stored body is the state after Protrusion_3: a
feature in error, flag not decoded); 140's and 054/056/060's stored bodies do not measure (negative
or zero volume); 163's spline revolve and 035's two-skin revolved cut differ in volume.

## Amendment (2026-11-23, 31): Inventor round ipt12 - second spline class, mirror order, split blend supports

Measured first (all 1686 parts on this round's base, as after ipt11: 750 as history), then the parts
each change reaches (scratch, not committed). The 415 parts "with no features" were counted by record
role: 409 hold only an imported body, 6 are derived from an assembly; no tree layout is missed there
(note AV of `docs/formats/inventor-feature-tree.md`). Changed this round, all in `gusset-ole`'s
Inventor reader:

| rule | parts gained |
|---|---|
| a cylinder blend's support surface split into several faces counts once; of more than two supports, the pairs that meet must meet at one edge | 2 (iPod Nano, Holder_Tip) |
| sketch B-splines of class `f9372fd4` (the layout of `317b7346`); a spline's ends are the listed points at its end poles (the list also names points on the curve); closed splines; arrays below their capacity | 0 (27 parts: the how-to outlines, coins and banners now read further and stop at later sketches, trimmed splines or kernel unions) |
| mirror and pattern copies in the order the features were made, not the pick order | 0 (the EY4180 bay adapters still stop at Mirror2: the copied chamfer's edge is 1.5 mm off the mirrored pick) |

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4). Only
`gusset-ole`'s Inventor modules changed, so the SolidWorks and Solid Edge corpora were not rerun.

| corpus | parts | history before | after | without a solid |
|---|---|---|---|---|
| first (Zenodo) | 47 | 28 | 28 | 0 |
| second (57 sources) | 1639 | 722 | 724 | 46 |

No part went from history to body: a full run of the 1686 at the mirror-order commit, then the 55
parts with spline records after each spline commit. SRM (the MoS2 rig's rotor) takes 807 s alone and
times out at 900 s under load; it is a body either way.

Measured and left: the `rev1 corners` mirrors (3) were saved against an earlier base (their stored body
is 101.6 x 1066.8 x 101.6 mm, the base in the folder 76.2 x 1016 x 76.2); the `lf_basic` and `dv_*`
lofts (the exact ruled loft from a square to a circle is 5645.40 mm3, Inventor's 5644.98) and
`sw_twist` (a twisted square prism is exactly 3000 mm3, Inventor's 2996.89) are approximations in the
stored body; `fl_two_sets` needs both radius sets blended in one operation (built one after the
other, the second set's tangent chain runs over the first set's arc and down the corner edge);
face-name curve numbers in larger sketches do not follow record order (EY4180 A-07: record position 43
is named 47) and no class of record the sketch owns accounts for the gap; the sheet-metal fixtures
stop at plates on plate faces, bends between plates, corners and hems.

## Amendment (2026-11-23, 32): Solid Edge round se10 - features in error, misnamed curves, profiles' own copies, spun offsets

Measured first (all 180 ordered parts, one per process, on this round's base: 99 as history, as
after se9), then each non-history part's tree read alone (reader probe, dumps and tools in scratch,
not committed). Fixed this round:

| cause (measured) | parts | outcome |
|---|---|---|
| a feature Solid Edge could not build is kept in the file but not in the stored body: its function object's first flag byte (after the `QQ` head, kind and serial) has 0x08 set (on 11 of the some 2,200 features read: 020's and 142's last, 135's three cutouts, 136's two protrusions, 019's last round, two of 010's, one of 144's; most others have 0x02) | 020 | left out (`omitted`, "a feature in error"); 020 rebuilds to its stored body |
| some type tables name the kind of profile curves as a dimension (`JPMIDimensions`, 015's circles, 031's B-splines) or a point on a curve (012), so the members were skipped as annotations and the profile had no curves | 015, 031, 012 | a member so named that is laid out as a curve (not 46 bytes like a key point, `u8 0` at +8, readable) is read; 015 rebuilds (1e-7), 031 and 012 stop later |
| a 2D profile lists its members twice: the curves it was drawn from, then (after `u8 1`, a class id and the same count) its own copies; the first list can name a sketch curve of an older shape (136's right lobe: an arc in the sketch, an ellipse in the body) or one stored nowhere (010, 176) | 136, 010, 176 | the own copy is read where it is a curve of the part (only failing parts' trees change); 136 then stops at the box-on-capsule round alone, like 133-135; 010 stops at a round, 176 at a union OCCT leaves invalid |
| stored bodies: an offset of a spun surface (163's 2 mm thin wall) came out on the wrong side, because OCCT reads a SURFACE_OF_REVOLUTION with u and v swapped and keeps the offset's distance; the stored mug measured 844 mm3 against the features' 17289 | 163 | gusset-xt writes such distances negated (ADR 0035 amendment of this date); the stored body measures 17289.05 mm3 |
| the box check: OCCT's `AddOptimal` box of 163's stored mug was 0.045 mm short of its 29.654 mm radius on one side (a B-spline profile spun with a boss on its seam) | 163 | `gs_bbox` adds the peaks of each face of revolution's circles that lie on the face (ADR 0004 amendment of this date); 163 rebuilds, 4.0e-6 in volume |

**Coverage (180 ordered parts, OCCT `par_history`, one part per process).** Before: 99 as
history. After: **102 as history** (015, 020, 163), none lost. 052 timed out in the stored body's
STEP import once under a machine load near 18 (its tree is unchanged); measured alone it fails
as before ("a profile of model edges other than one circle"). The kernel and gusset-xt changes
reach every importer, so the other corpora were rerun in full on this round's base and head:
SolidWorks 370 of 1352 as history before and after, no part's outcome or reason changed;
Inventor 750 of 1686 before and after, none lost and no history outcome changed (two parts
that timed out under load in the base run came in as bodies, as before). Fusion sets were not
run here.

**Left (measured).**
- 054, 056, 060 (054 and 060 one geometry): the stored body loses its three thin-wall faces in
  OCCT's STEP reader ("Type OffsetSurface is out of scope"): their bases are closed rational
  B-surfaces (two cubic half circles round, a triple knot at u = 0.5, weights 1, 1/3, 1/3, 1),
  only C0 in their parameter though tangent-continuous, and OCCT's reader refuses an offset of a
  C0 surface; the body is an open shell of volume 0. A C1 basis cannot be had by knot removal
  (the weights break homogeneous C1); writing the offset as a fitted B-surface would.
- 140: the stored body (tori, -23242 mm3, 63 mm deeper than the features) is a different shape
  in conversion, not a sign slip.
- Part copies (004, 005, 009, 013): each copies a sibling part of its folder (P8, P7, P27, P9 of
  the same project; the path is stored), and every one of those siblings fails itself (018, 017,
  010, 019), so reading the copy would gain nothing yet.
- 133-136: OCCT's fillet fails on one edge alone (135: the top-side edge whose back end meets the
  hemisphere 0.5 mm above its seam with the capsule's cylinder; its mirror edge builds).
- 019: every chamfer edge of the bottom and top tangent chains gives OCCT an invalid solid alone
  (OCCT carries the chamfer along the whole tangent chain, which ends at a 0.09 mm high sliver).
- 069, 070: lofted cutouts to a point (a 30-byte point member), and their copies.
- 012, 017: SmartBoss features (not read); 014, 142: simplified base features; 051, 058, 162:
  round kind 17 (its size slot is a 455-byte object, not a value).
- 041: Round_2's edge lies 224 mm from any of ours: Round_1 (762 mm on a 304.8 mm high body,
  a cliff round) builds, but not as Solid Edge's.
- Other rounds as after se9: 083, 088 (three-edge corners on B-spline faces), 100, 170 (far
  edges not lined up with the round's ends), 044, and the plain rounds OCCT does not build
  (177, 178, 101, 016, 063, 064, 166, 167, 043, 080, 107, 061).

## Amendment (2026-11-23, 33): Inventor round ipt13 - blend picks past their edge, step-wide chamfers, copy order

Measured first: all 1686 parts on this round's base (752 as history, as after ipt12), then the 106
bodies whose first failure is a fillet or chamfer, each with its failing pick, the four model edges
nearest it and the blend face's neighbours (scratch, not committed). By cause:

| first failure | parts | what the pick lands near |
|---|---|---|
| no model edge at a pick | 39 | 6 within 0.5 mm, 16 more within 5 mm, 17 farther: an earlier feature differs (text embosses read as their whole sketch, a midplane pattern one spacing off, fillets merged with a later join's face), a fillet rolling over a neighbouring blend (the S-shaped pairs of 11061412's brackets, Zawias), a concave chamfer read as a convex one (LT-PAM stopper) |
| spline blend surfaces (`ref`, `rb_blend_spl_sur`, `cyl_spl_sur`) | 23 | blends between a plane and a cylinder or cone off its axis, or along a spline edge: the edge is not a line or circle the reader can rebuild (CRS-METEO base radiators, connector mains, vanes and rotors: 10) |
| cylinder blends with 0, 1 or 3 usable supports | 11 | a support used up by the blend or by a neighbouring blend of the same feature |
| OCCT refuses the chamfer or fillet | 14 | 8 chamfers (2 as wide as their step, below), 6 fillets |
| other: corner circles 3, support pairs 2, no edge read 5, the bay adapters (after the mirror fix, a text engraving) 4, the LEGO bricks (a text boss read as a disc) 2, a fillet with several radii, a face fillet, a full round beside other edges | 19 | |

Changed this round:

| rule | parts gained |
|---|---|
| a mirrored blend copy reflects its picks' path points with their midpoints (shared tree import) | 0 (the EY4180 bay adapters' Mirror2 builds; they then stop at a text engraving read as its whole sketch, 271 mm3 off) |
| of a two-direction pattern's midplane direction with an even count, the extra occurrence lies along the stored direction (behind a turned one, ahead otherwise) | 0 (the bay adapters' nut traps, count 8, now land; the parts stop as above) |
| a straight pick on no model edge takes the model's straight edge on its line nearest its midpoint, as the last resort (shared tree import) | 1 (CRS-METEO box-mount 0015; its two later versions stop at a fillet OCCT refuses) |
| `VBL_SURF` blend faces are corner patches | 0 (fl_setback and the connector mains stop at their spline blends) |
| chamfers as wide as their step: cut as the triangle they removed, revolved | 2 (ISO 7435 M2 set screw, GOST 12639 rivet) |
| a feature a mirror or pattern copies, made after it but placed before it in the order record, moves to just before the copy (full place order is wrong in some files) | 1 (EY4180 Differential Signal Generator A-02) |

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4).

| corpus | parts | history before | after | without a solid |
|---|---|---|---|---|
| first (Zenodo) | 47 | 28 | 28 | 0 |
| second (57 sources) | 1639 | 724 | 728 | 46 |

Checked: the trees of all 1686 parts were read on the base and the head; every part whose tree
changed, that mirrors a blend, whose base failure was a missing model edge, or that is read through a
base part (204) was rebuilt on the head: no part went from history to body. The two tree-import
changes reach SolidWorks, Solid Edge and Fusion imports (only where a blend pick would otherwise find
no edge, or where a mirror copies a blend whose picks carry a path); the SolidWorks and Solid Edge
corpora were rerun on the base and the head (below).

| corpus | parts | base | head |
|---|---|---|---|
| SolidWorks (sw-more, Slip-Pad, ARIG revisions, Zenodo) | 1352 | 373 | 373 (no part's line changed; V-Slot Door Handle crashes the test process on both) |
| Solid Edge (ordered parts, one per process) | 180 | 102 (round se10's head) | 102 (no part's row changed; 064 crashes on both) |

Fusion imports share the tree import: a straight Fusion pick with a path that names no edge now
takes the edge on its line before the import gives up; Fusion picks without a path are unchanged.
Fusion was not rerun (the session's job).

What blocks the rest of the 106 (measured, not changed): the bay adapters' engravings and the LEGO
bricks' stud logos are sketch text, read as their whole sketch or as the disc under it; spline blend
surfaces (23) need the edge between a plane and a cylinder, cone or spline off its axis, which no
line or circle the reader rebuilds; cylinder blends that rolled onto a neighbouring blend of the same
feature (the S-shaped pairs of 11061412's Fixture Pull Bracket, Zawias) give the edge between the
plane and that blend, not the one the model has; a concave chamfer whose step face was used up reads
as convex (LT-PAM stopper: both corners fit the faces); OCCT refuses 6 chamfers and 6 fillets on edges
the picks find (the box mounts' Fillet6, EndCap's outline chain, LED Washer's slot edges); AN816's
chamfer lands 0.381 mm off because the iPart member's revolve is that much shorter. `fl_two_sets`
was not taken up: one part needs a fillet with a radius per edge in the kernel.

## Amendment (2026-11-23, 34): Solid Edge round se11 - C0 tube offsets, apple-shaped domes, round kind 17, sections inside their paths

Measured first (all 180 ordered parts, one per process, on this round's base: 102 as history, as
after se10), then each candidate's tree and stored body alone (probe and tools in scratch, not
committed). Changed this round:

| cause (measured) | parts | outcome |
|---|---|---|
| OCCT's STEP reader drops an offset of a B-surface that is only C0 in its parameters (the thin walls of 054, 056, 060: closed rational B-surfaces of two cubic half circles) | 054, 056, 060 | gusset-xt writes the offset of such a tube as the tube scaled row by row (ADR 0035 amendment of this date); the stored bodies come in closed and their rebuilds are compared, but do not match: OCCT's quadratures disagree on the stored bodies (056: 443753 mm3 by default, 461968 adaptive, a 0.02 mm mesh 466521; the features 465295.55); 054 is not valid by BRepCheck |
| a lemon-shaped torus (positive major radius 0.006 mm below its 55.4 mm minor one) bounding 140's revolved dome kept its inner lobe in OCCT: the stored body measured -23243 mm3 | 140 | the outer lobe is written as its revolved arc where all the faces' points lie on it (ADR 0035 amendment); the stored body measures 17958.1591 mm3 against the features' 17958.1590; 140 rebuilds |
| rounds of kind 17 (051, 058, 162) keep their radius in the same 455-byte size object as kind 1 (25.4 mm in 051) | 051, 058, 162 | read as constant radii; all three stop later, at rounds or unions OCCT does not build (051's 25.4 mm kind-17 round itself, 058's 1.27 mm round on a 2.54 mm edge, 162's Protrusion_2 union) |
| a sweep's cross section at a joint inside its path (023: the path's two arcs meet at the section) | 023 | swept both ways from the joint, the curves after it a feature of its own (`path_curves`); 023 then stops at its thin wall (OCCT's shell) |

**Coverage (180 ordered parts, OCCT `par_history`, one part per process).** Before: 102 as
history. After: **103 as history** (140), none lost (the sweep commit reaches 023 alone, rerun
on the final head). The gusset-xt changes reach every Parasolid-based importer, so the other
corpora were rerun in full on this round's base and head: SolidWorks 373 of 1352 as history
before and after, no part's outcome line changed; Inventor (whose import does not convert XT:
its stored bodies are ACIS) 752 of 1686 before and after, no outcome changed. Fusion sets were
not run here.

**Left (measured).**
- 054, 056, 060: closed now, but their stored bodies do not measure consistently (above); the
  swept outer walls' areas differ from the exact sweep's (056: 70037.8 against 70055.7 mm2).
- 115: RevolvedCutout_2 (a profile with one edge on its axis, 0.05 mm off the sketch's y axis)
  fails in OCCT's revolve; moved onto the sketch axis it builds, but Round_1 and Round_2 fail
  after it.
- 162's stored body is a solid with a void whose outer shell loses a thin-wall face (an offset
  of a spun C0 B-curve; the tube rule does not apply), and its Protrusion_2 union is invalid.
- 168, 052: profiles of model edges that are 3D B-splines (not a circle).
- SmartBoss (012, 017): a boss with a hole and stiffening ribs, its 11 parameters in a
  `JMPropsToSmartBossDataUpdateFunc` (diameters, depths, draft angles in radians, a rib count
  of 4); not read. Ribs (018: 6, 011: 4) and delete faces likewise.
- Others as after se10 (lofts to a point 069/070, part copies, simplified base features, the
  kernel rounds listed in amendment 32).

## Amendment (2026-11-23, 35): Inventor round ipt14 - crossing profile loops, small regions, sketch text, joins through all, pattern order

Measured first: all 1686 parts on this round's base (756 as history, as after ipt13). Of the 882
bodies, 53 build every feature and stop at the volume check; each was rebuilt feature by feature
against the stored body (scratch, not committed). Most of those that could be told apart had a
profile pick that left a region out or took one too many, so this round worked on the picks.

| cause (measured) | rule (format notes AX) | parts gained |
|---|---|---|
| a slot drawn as a rectangle and two end circles, each picked whole: three bodies whose loops cross, and the parity count dropped the lens regions where they overlap | a region inside two bodies whose loops cross (each runs well inside and well outside the other) is kept; touching is not crossing (the AN818 nut's inscribed circle; a keychain's ring 0.06 mm outside a spline outline's samples) | ArmBar x2, Winding Assembly Base Ring |
| regions under a few cells of the 500-cell region grid: half discs of 0.4 mm circles on a PCB edge, slivers a 200 mm line cuts from a 20 mm V-slot profile, a 1 mm wedge a slanted line cuts off a 210 mm strip | small circles and arcs, loops under a quarter of the sketch and the squares round curve ends are looked at again on finer grids (not for the fallback of a loop listed twice) | servo PCBs x2, fuzzy-architecture bracket and V-slot |
| a join through all ran 10 m each way | up to the next face; with slot 7 set, both ways as two extrudes | Spool to Winder Transfer Gear |
| sketch text is not read as curves, so the profile took the sketch's other curves | when a loop of the feature runs wholly off the sketch's curves, the feature's loops (straight and circular edges) are its sketch | LightSaber Connector |
| a feature pattern cut every copy first and added copies after: a fillet on a tab the pattern also copies landed in the air (gusset-eval, shared) | when a feature that takes material away follows one that adds it, the copies are made feature by feature; other patterns as before | Lead Screw Coupler Connector Male |
| (the profile pick rules together; not split by rule) | | Connection disc of gear, Proto-Board, servo horn, Testrig Bauteile 20~3, Adapter-8-5-Unterteil, EndCap (its Fillet2 failed on the base) |

Tried and not kept: a hole to a work plane on the far side of its normal run the other way (the
CRS-METEO i-rings' Hole5 builds then, and the parts stop at a chamfer on rounded faces; the unit test
keeps such a plane refused, and with no rebuild to confirm it the commit was reverted). A cut OCCT
leaves undone without an error (the Wago connector's end recess: its sketch plane touches the body's
rounded side along a line; plain OCCT 8 cuts, commons and the reverse cut all take nothing up to a
fuzzy value of 1e-5, and 1e-4 cuts it) was redone in the kernel with a growing fuzzy value when the
tool's centroid stayed inside; the part then measures 2.0e-5 off from its arcs and gains nothing, so
the kernel change was not kept (scratch patch; it would cost two volume integrals on every cut).

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4).

| corpus | parts | history before | after | without a solid |
|---|---|---|---|---|
| first (Zenodo) | 47 | 28 | 28 | 0 |
| second (57 sources) | 1639 | 728 | 744 | 46 |

Checked: the trees of all 1686 parts were read on the base and the head; the full corpus was rebuilt
on the head, and every part whose tree changed or whose feature patterns take material away after
adding it (105) again on the final head: 16 parts went from body to history, none from history to
body. Gameboy and Zapdos Pokemon Card (bodies on the base, 264 s and 467 s) ran past the runner's
900 s limit on the head while the machine ran three other corpora (load 10 to 17 on 4 cores); rerun
with a longer limit, Zapdos stops at the importer's own time limit ("importing took too long",
313 s), and Gameboy comes in as bodies as on the base (after 1625 s; its reason is now an unequal
chamfer whose edges' reference faces are not read). Reading Zapdos's tree takes 132 s of CPU with
the finer grids against 116 s without.

**Other formats.** Only the feature pattern order is shared (gusset-eval); the profile, text and join
rules are in the Inventor reader. A pattern takes the new path only when a feature that takes
material away follows one that adds it; otherwise it makes the same kernel calls as before.

| corpus | parts | base | head |
|---|---|---|---|
| SolidWorks (sw-more, Slip-Pad, ARIG revisions, Zenodo; full run on base and head) | 1352 | 384 | 384 (two lines changed, both bodies before and after: the sw-more base-square spacers' volume gaps shrink from 1545 to 55 mm3) |
| Solid Edge (ordered parts, one per process; full run on the head, with the new path marked) | 180 | 103 (round se11's head) | 103 (no row changed; only 145's FastPattern_2 takes the new path, and 145 has the same row on the base; 064 crashes on both) |

Fusion imports reach the pattern change where a Fusion feature pattern lists a cut, fillet or chamfer
after a join; Fusion was not rerun (the session's job).

**Left (measured).** 42 bodies build every feature and stop at the volume check (53 on the base):
- Sketch text with spline glyphs (`intcurve` edges) is not read: Main Bearing Cover's engraving,
  the keychain's lettering. The EY4180 bay adapters' arrows are read (straight edges), but their
  engraves cut into the air: their slot-2 direction points away from the floor; turned round they
  take 5.3 of the 6.85 mm3 each needs (A-01 is 13.7 mm3 off, was 271). The LEGO bricks' stud logos
  read as straight-edged letters come to within 0.15 mm3 (2x2), the O's inner loop a wire body of
  its own like every loop here, so a hole and a second picked region cannot be told apart.
- OCCT cuts that take nothing without an error where the tool's start face touches a cylinder of
  the body along a line (Wago Inline; fuzzy 1e-4 cuts it, above).
- Near misses whose cause was not found: TapaE/TapaS (9e-5 and 8e-5), the jarpoole screw (1.1e-4),
  Spool Drive Spindle (1.4e-4: 0.21 mm3 left round its cross slot).
- A fillet of two sets made together (`fl_two_sets`, 98 mm3: Gusset makes the second set after the
  first and rolls it round the corner); the CRS-METEO bearing clamps (22 mm3: the bore cut after
  their coil changes the volume neither way round).
- As after ipt13: spline blend surfaces (23, each a different neighbourhood: supports used up by
  neighbouring blends of the same feature, vertex blends, loft supports), no model edge at a pick,
  derived parts whose base is not in the folder (40), imported base bodies (43), features the
  plugin does not export (79: Design Accelerator gears, delete face, sheet-metal bends, ribs).

## Amendment (2026-11-23, 36): Solid Edge round se12 - smart bosses, cutouts outside a closed profile

Measured first (all 180 ordered parts, one per process, on this round's base: 103 as history,
as after se11), then the parts se11 left with features not read (reader dumps, the stored
bodies' faces near each boss, scratch tools, not committed). Changed this round:

| cause (measured) | parts | outcome |
|---|---|---|
| smart bosses (`JMSmartBossAssocFunction`, Solid Edge's mounting boss) were not read. Slot 2 is a profile of sketch points (each a 78-byte point: u, v at +8, copied at +58), slot 4 a `JMPropsToSmartBossDataUpdateFunc` whose slots 1-11 are value objects (`f64` at +8; byte +18 0x9d on, 0x91 off): 1 the boss diameter, 2 and 3 the hole's diameter and depth (off in 012: no hole), 4 the rib width, 5 how far the ribs' outer faces start outside the boss (0 in 012, 0.5 mm in 017; 017's recess at the hole's mouth is that much wider than the hole), 6 how far the ribs start from the boss's end (5 mm, 1 mm; 017's recess is as deep), 7 the boss draft (0), 8 the ribs' slope off the axis (8 and 15 degrees), 9 the rib count (4), 10 and 11 off in both parts. The boss runs from its plane to the next face, on the side of the plane where the edges the feature stores on its axis lie (the plane's normal points either way: 012 and 017's first two against the boss, 017's last two with it). Ribs: the first along the plane's x, centred on the axis. | 012, 017 (5 bosses) | read: each boss an extrude up to the next face, its hole and recess cuts, each rib a body up to the next face from its foot (`to: next, op: new`, ADR 0061 amendment of this date) trimmed to its slope and joined; checked against the stored bodies' faces (boss cylinders, rib planes and slopes, the 0.053 mm2 triangles at the ribs' feet). Both parts stop earlier, at their thin walls (below) |
| a cutout's side (slot 5) also holds for a closed profile: run from its first curve as drawn, 017's side outline is clockwise with side 1 (left): the material outside it is cut, not the inside | 017, 061 | written as the profile inside a box round it, the region between picked (`profiles: [{point}]`); 061's Round_1 then finds its edges (it stops at Round_2), 017 at its thin wall. No other part's tree changes |

**Coverage (180 ordered parts, OCCT `par_history`, one part per process).** Before: 103 as
history. After: **103 as history**, none gained, none lost. Rows that changed: 012 (now read
whole; its first feature, a revolved dome whose closing line is solved 3.3e-7 mm off the axis,
crashes OCCT's check of the revolve in `Extrema_ExtCC::Points` - the in-process test records a
crash, the app's kernel worker survives it), 017 (stops at Thinwall_1), 061 (stops at Round_2).
Moving the revolve axis onto such a line (tried, reverted) avoids 012's crash but changes 160's
pentagons, each revolved about one of its own edges, whose union then crashes OCCT instead. The
evaluator change (`to: next, op: new`) reaches every importer only where a tree asks for it, so
the other corpora were rerun in full on this round's base and head: SolidWorks 384 of 1352 as history before and after, no part's outcome line changed;
Inventor 756 of 1686 before and after, no history outcome changed (two parts that timed out
under a machine load near 18 in the base run came in as bodies on the head). Fusion sets were not run here.

**Left (measured).**
- 012: past the crash, Thinwall_1 (a 2 mm inside shell of the apple-shaped dome, open at its
  base) fails in OCCT's offset (no reason given).
- 017: Cutout_3 (an open U profile, three lines drawn one way and the last the other, both free
  ends "drawn to") is closed on the wrong side: the stored body has the notch inside the U
  (point tests at x = 74 mm); taking the chain's direction from the most curves drawn its way
  fixes 017's U but breaks 007, 010, 150 and 175, so it is not the rule. Thin wall and chamfers
  follow.
- Ribs (`JMRibAssocFunction`; 018: 6, 011: 4): slot 2 the profile (an open line), 3 = 12, 4 = 9,
  5 the side (1 or 3), 6 the direction (1 or 2), 7 the thickness (a dimension relation), 8 = 14
  (to the next face). The smart boss's rib (a body up to the next face, trimmed, joined) is the
  likely form; 018 first stops at a draft whose cylinder face is not bounded by whole circles,
  011 at DeleteFaces and DeleteRegions.
- 067, 071: rebuilt within 4.2e-5 and 3.1e-5 of their stored volumes; 071's difference is one
  sliver (80 mm3) between OCCT's round and Solid Edge's at the top edge x = -121.92 mm (both
  B-spline faces, 0.1 mm apart on average).
- Others as after se11 (thin walls 111, 145, 023; kernel rounds; 054, 056, 060; lofts to a point;
  part copies; simplified base features).

## Amendment (2026-11-24, 37): Inventor round ipt15 - B-spline glyphs and profile loops, tangent spline joints

Measured first: all 1686 parts on this round's base (772 as history, as after ipt14). Of the 42
bodies that build every feature and stop at the volume check, the sketch text family was taken first:
its glyph outlines (wire bodies of the history blob) and the loops of picked regions had their curved
edges read as chords.

| cause (measured) | rule (format notes AY) | parts gained |
|---|---|---|
| a glyph's curved edges are `exact_int_cur` B-splines (TrueType quadratics); their parameters are doubles the reader did not take for a curve, so each edge was its chord (the LEGO logo's O a quadrilateral, the studs 0.15 mm3 thin) | the reader keeps every `exact_int_cur` subtype's B-spline by subtype number (`{ ref n }` resolved); a text sketch writes a whole B-spline edge as a sketch `bspline` | LEGO 2x2, 2x3, 2x6 |
| two glyph splines meeting tangent cross again next to their joint; the region arrangement snapped such hits only for arcs, so the hit cut a piece of no length off one spline, depending on where the sketch lay (gusset-sketch, shared) | a spline's radius is bounded by its size for that allowance | LEGO 2x4, 2x4 V2 |
| the loops of picked region bodies took B-spline edges as chords, so a pick's parity next to a spline outline was wrong | profile loops sample B-spline edges along their curve | Sports Team Keychain, MIT Intramurals Banner |

Tried and not kept: the EY4180 bay adapters' engraves turned round (a debug switch, not committed):
they then cut 7.3 mm3 each but on the wrong face (the stored body's arrows lie on the opposite face;
A-01 3.1 mm3 off), so the direction is not the whole cause.

The rebuild check is unchanged (every feature builds, volume within 2e-5, box within 1e-4).

| corpus | parts | history before | after | without a solid |
|---|---|---|---|---|
| first (Zenodo) | 47 | 28 | 28 | 0 |
| second (57 sources) | 1639 | 744 | 751 | 46 |

Checked: the full corpus on the base and on the head: 7 parts went from body to history, none the other
way; 6 bodies changed their first failure (four stop later than before: the robot base covers, Homer
Coin and Natural Trumpet, whose text sketches were refused before; Playmobil 3's loft section now
finds no sketch for its loop instead of no regions; 175 T pulley ran past the import time limit under
load and gives its base reason when run alone).

**Other formats.** The B-spline reading is in the Inventor reader (`acis.rs` is Inventor's only). The
arrangement change is shared (gusset-sketch): it only snaps a hit between two curves that share a
joint, where one of them is a spline, within `tol + sqrt(2 tol d)` of that joint (`d` the spline's box
diagonal).

| corpus | parts | base | head |
|---|---|---|---|
| SolidWorks (sw-more, Slip-Pad, ARIG revisions, Zenodo; full run on base and head) | 1352 | 389 | 389 (no line changed) |
| Solid Edge (ordered parts, one per process; full run on base and head) | 180 | 103 | 103 (no row changed; 174 ran past the import time limit under load on the head and gives its base row alone) |

Fusion imports reach the arrangement change wherever a Fusion sketch has splines that share an end;
Fusion was not rerun (the session's job).

**Left (measured).** 36 bodies build every feature and stop at the volume check (42 on the base): the
EY4180 bay adapters (engraves, above), Main Bearing Cover (Hole1 leaves 70 mm3 too little between
z 8.1 and 9 mm, before its text), the Wago connector's silent cut, TapaE/TapaS, the jarpoole screw,
Spool Drive Spindle, `fl_two_sets`, the CRS-METEO bearing clamps, the ISO 7046 cross recesses, as after
ipt14; spline blend surfaces, picks with no model edge, derived parts with a missing base, imported base
bodies and the features the plugin does not export are unchanged.

## Amendment (2026-11-24, 38): round rib1 - ribs measured in Solid Edge and Inventor

Gusset gained a rib (ADR 0062: the document's `rib`, the kernel's sweep up to the next face with the
pieces past the part dropped, tree type `rib`); the SolidWorks reader writes it (ADR 0043 amendment,
round rib1). Neither reader here writes it yet:

- **Solid Edge** (`JMRibAssocFunction`, amendment 36's slots: 2 the open profile, 5 the side, 6 the
  direction, 7 the thickness relation, 8 = 14 up to the next face): the two parts with ribs stop
  before them (018 at a draft whose cylinder face is not bounded by whole circles, 011 at
  DeleteFaces and DeleteRegions), so a reading could not be checked against a stored body; left
  unwired. The smart boss's ribs stay as they are (a body up to the next face, trimmed, joined).
- **Inventor**: three parts stop at a rib first (Corliss's `rb_parallel`, `rb_thick4`,
  `rb_normal_sym_draft`: `Rib1`, feature class `d968d1e0`, not read); no other part reports one first.
  Their records were not decoded this round (the third also has a draft on the rib's sides, which
  Gusset's rib does not take).

Both corpora were rerun in full on this round's base and head (one part per process, the round's
own binaries): Inventor 744 of 1639 and 28 of 47 as history before and after, every part's outcome
class unchanged; Solid Edge 103 of 180 before and after, one row differing outside history (052
timed out under load on the base and gave its usual reason on the head).

## Amendment (2026-11-24, 39): Inventor round ipt16 - ribs, the side of engraves

- **Ribs** (`d968d1e0`, format notes AZ): a rib parallel to its sketch, up to the next face, is written
  as the tree's `rib` (ADR 0062): the sketch named by the `d61732c1` record beside the profile list, its
  non-construction lines, slot 3 the thickness, `toward` against the outward normal of the result face
  the rib made through its first line, `side` from the thickness arrow slot 9 keeps. A rib normal to its
  sketch or of a finite extent (slot 6 active) and drafted rib sides (slot 11) are refused with that
  reason. `rb_parallel` and `rb_thick4` rebuild; `rb_normal_sym_draft` stays out (normal, drafted 2
  degrees). No other part of the corpus has the class.
- **Engrave and emboss sides** (notes AJ, AZ): slot 2 of an emboss is its sketch's normal, not a
  direction into the material; on a top face it points out, and those engraves cut into the air. The side
  is now where the stored result lies wholly along the normal (its points and the reach of its circles
  and ellipses; an emboss's own height allowed); where the result lies on both sides of the sketch plane
  the normal is kept as before. Storage-Rack A-01, BICYCLE SPANNER and LOCK GUIDE rebuild; Main Bearing
  Cover's lettering now cuts the part and it comes to 0.07 mm3 (1.1e-4) of its stored body (amendment
  37's "Hole1 leaves 70 mm3 too little" was this engrave); HOOK gets one feature further; the EY4180 bay
  adapters (sketch on the floor's inner face, walls on both sides) keep the normal and stay 13.7 mm3 off.

| corpus | parts | base | head |
|---|---|---|---|
| Inventor new (ipt-more etc.; full base run, head rerun on the 23 parts with ribs or embosses, the only features the change reaches) | 1639 | 751 | 756 (+5, none lost) |
| Inventor old (Zenodo) | 47 | 28 | 28 |

The change is in the Inventor reader only (`inventor_features.rs`, two Inventor helpers in `acis.rs`); no
code shared with SolidWorks, Solid Edge or Fusion changed, so those corpora were not rerun.

**Left (measured).** 32 bodies stop at the volume check (35 on the base), Main
Bearing Cover among them at 0.07 mm3: the EY4180 bay adapters (above), the three `rev1 corners` mirrors of
tegu-enclosure (3.28e6 against 8.26e6 mm3: the base part in the folder is not the one they were derived
from), BEARING (164 mm3 too much after Sweep1, which takes every region of its sketch, a ball's circle among them; not measured further),
terminal_mount_front (Extrusion4's cut leaves 68 mm3 and takes 13 mm3 too much at the mount's end), the
gun body (19 mm3 short after Hole2), EjectionSystem (Fillet1 and the cuts before it, 13 mm3), as well as
the near misses of amendment 37. Features not exported (delete face 8 parts first, sheet-metal bends,
Design Accelerator gears), sheet-metal plates whose loop no sketch holds (8), derived parts with a missing
base and imported base bodies are unchanged.

## Amendment (2026-11-24, 40): round sw20 - mirrors of mirrors and holes past their face, measured

Round sw20 changed the tree import (ADR 0042 amendment: a mirror of features that lists another mirror copies its
seeds and their copies; seeds in the tree's order; holes past their face or on none cut as revolves). Both corpora
were rerun in full against their own baselines (the base's test binaries, same workers). Solid Edge: 103 of 180,
every row unchanged. Inventor: 772 of 1686 history, none gained or lost; four parts get further (EY4180's A-01 and
A-03, FPV_Chasis and Heatpad Tray now copy their mirrors of mirrors and stop at a chamfer, a fillet or a pattern of
mirror copies), and one part that timed out under load in the base run now gives its reason.

## Amendment (2026-11-24, 41): Inventor round ipt17 - browser places, emboss faces, long round arcs

- **Browser order** (format notes BA): features are built in the order of their places in the order
  record (note AW), no longer in id order with only copied features moved. The EY4180 bay adapters show
  it: their nut traps sit after the floor's vents and fill by place, and the stored body keeps the traps
  whole; an emboss dragged above the base extrude built nothing in Inventor (no face of it in the result)
  and is now left out ("placed before the first solid"). A fillet or chamfer made after a shell but
  placed before it stays after the shell (Gusset's shell does not offset a blend face whose offset vanishes
  in its corner). 110 parts change order; only the bay adapters' outcome changes.
- **Emboss sides from their own faces**: the result tags an emboss's glyph floor or top
  (`NMx_EmbossBottomFaceTag`) and walls (`NMx_EmbossRimFaceTag`); where they lie off the sketch plane is the
  side (amendment 39's whole-body rule is the fallback). The bay adapters' arrow cuts into their floor.
- **Round arcs of more than half a circle**: a blend face whose vertices are its arc's two ends takes the
  side from its own arcs' samples (as half circles did), not from the largest gap between the vertices,
  which is the other arc past 180 degrees. Drag Lock rebuilds; HOOK's fillet is now found and crashes the
  kernel's fillet (OCCT, a null face in `ChFi3d_Builder::PerformIntersectionAtEnd`): the corpus test runs
  the kernel in its own process and stops on it, the app's kernel worker fails the one operation.

| corpus | parts | base | head |
|---|---|---|---|
| Inventor new (ipt-more etc.; full base and head runs, the arc change rerun on the 13 parts whose trees it changes) | 1639 | 756 | 761 (+5, none lost) |
| Inventor old (Zenodo) | 47 | 28 | 28 |

Gained: EY4180 5.25-Bay-Adapter A-01, A-01_MIR, A-02 and A-03 (A-01's tree, the others derived from it),
Drag Lock. None lost (HOOK, a body before, now crashes the in-process test kernel: see above). The changes
are in the Inventor reader only (`inventor_dc.rs`, `inventor_features.rs`, `acis.rs`); no code shared with
SolidWorks, Solid Edge or Fusion changed, so those corpora were not rerun.

**Measured, not kept.** A sweep that takes the regions its profile list names (a shared tree change:
`profiles` on `sweep`, the extrude's region picks) gains nothing: BEARING's race, swept alone, then fails
to join the balls that touch its groove ("removed all material"). A kernel retry of cuts that leave none of
their tool's faces on the result (OCCT misses a tool whose end plane touches a round end along a line; a
fuzzy value of 1e-4 mm finds it) makes the Wago clamp's slot but leaves the part 2.004e-5 off its stored
volume. Neither is committed.

**Left (measured).** 29 bodies stop at the volume check: the CRS-METEO bearing clamps (the cut of the
coil's top comes back unchanged from the kernel in memory, though it cuts after a STEP round trip), the
Wago clamp (above), the ISO 7046 recesses (a tapered join "to next" written blind runs out through the
head's underside), the gun body (19.4 mm3 short after its holes), the ruled lofts of the Oblikovati
fixtures (0.42 mm3 over: Gusset pairs the square's corners with the circle's points on the diagonals,
the largest volume any such pairing gives; Inventor's pairing is not found), and the others of amendment 39. Self-named derives ("Solid1::" a file of the part's
own name, 32 with the Design Accelerator gears) hold no features of their own. The sheet-metal parts whose
second face no sketch holds also bend, hem and corner (not read). Delete face (8 first failures) stays
unread: no tree type takes it, and four of the eight have further failures behind it.
