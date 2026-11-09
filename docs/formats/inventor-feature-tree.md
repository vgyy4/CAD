# Inventor part feature history (clean-room notes)

Derived from sample bytes only (2026-11-09; 35 public `.ipt` parts and 2 `.iam` from Zenodo). Prototype decoder: `scripts/re/inventor/`. Status: ADR 0046 amendment.

# Inventor part (.ipt) feature history: layout notes (clean room)

Derived only from the bytes of the 35 `.ipt` / 2 `.iam` samples in
`quarantine/zenodo-inventor-2026-11-09/` (Zenodo 4309294, 11061412, 13941029, 8028536), with
Gusset's own import of each part as ground truth. No reader, write-up or SDK was consulted.
"Confirmed" = held on every sample it was checked on; "guess" = fits the bytes, not checked
against an independent value. Lengths in the file are **cm** (confirmed: `L-Bracket` d0=17,
d1=4.5, d3=11 against its 170 x 45 x 110 mm body; every exact rebuild below uses x10).
Angles are radians.

Prototype: `python3 -I extract.py DUMP part.ipt...` (inflate streams), then
`python3 -I build.py OUT truth/truth.json DUMP/<part>...` (decode + Gusset oracle) and
`python3 -I rebuild.py truth/truth.json OUT/*.gtree.json` (volume/box check).

## 1. Container and segments (confirmed, 37/37 files)

* OLE2 storage `RSeStorage`: pairs `M<id>` / `B<id>` (ADR 0046). `B`: 16 constant bytes,
  `04 01`, one zlib stream.
* `M<id>` (meta): `u32 n`, ASCII `"RSe Meta Stream Version 8"` (n bytes), 18 bytes, `u32 L`,
  **UTF-16 segment name** (L chars), GUID, dates, then **its own zlib stream** (found by
  the first offset that inflates to EOF). Segment names seen: `PmDCSegment` (feature tree,
  sketches, parameters, history ShapeManager blob), `PmBrowserSegment` (browser tree and
  display names), `PmBRepSegment` (result body, tag 0x031d), `PmGraphicsSegment`,
  `PmAppSegment`, `PmResultSegment`, `DesignViewSegment`, `NBNotebookSegment`,
  `FBAttributeSegment`; assemblies use `Am*`.
* Inflated meta: `u32`, `u16`, `u32`, `u32`, **`u32 nrec`**, then `nrec` x `u32 size|0x80000000`
  (the size of each record in the B stream). Later in the meta: a table of 10-byte entries
  (not needed), then `00..00 04 00 00 00` and the **class table**: `u32 ncls`, then `ncls` x 28
  bytes = `GUID(16, little-endian fields) u16 version u16 flag(0..2) u32 0 u32 n`. The class
  table is found by that `04000000` marker plus the entry checks (rse.py `classes`).
* Inflated `B` stream = records: `u32 class0`, then per record `content(size)`, `u32 size`
  (equal to the meta's size: a check), `u8 0`, `u32 classnext` (`0xffffffff` after the last).
  **class = value - 0x100, an index into the meta's class table.** Up to 64 bytes of tail follow.
  Example `L-Bracket` PmDCSegment: `00 01 00 00` (record 0 is class 0), content at 4..0x94,
  `90 00 00 00` (=0x90, the meta's first size) `00` `01 01 00 00` (record 1 is class 1), ...
  This walk is exact on every record of `PmDCSegment`, `PmBrowserSegment`, `PmBRepSegment`,
  `DesignView`, `Notebook`, `FBAttribute` in all files (47,095 records). It does NOT hold for
  `PmGraphicsSegment`, `PmAppSegment`, `PmResultSegment` (other layout, not needed).
* Class GUIDs are stable across files and versions (same GUID = same structure in the
  0x0836..0x0a92 files): roles below are named by GUID prefix.

## 2. Records and references (PmDCSegment)

* Reference = `u32 (index+1) | 0x80000000` to a record of the same segment; 0 = null.
  (1498 of 2543 refs in `L-Bracket` are answered by a back-ref; the parent/child structure
  is consistent everywhere it was used.)
* Common header: `u32 0`, `u16`, `u32 ref -> Name record or 0` (offset 6), `u8 0, u8 flags1,
  u8 flags2, u8 0` (offset 10), `ref owner` (offset 14, the document record), **`u32 id`**
  (offset 18, the object's naming id), then optional `u32 0xffffffff`/`0` and `u32 version`
  (0x0836, 0x0899, 0x089b, 0x0960, 0x0a2c, 0x0a92; absent in the oldest file) depending on
  class and version. Because of that, class bodies are best parsed relative to the record
  END or by searching the `0x30000002` list tag (done for params, placements, dirs).
* Lists: `u32 0x30000002, u32 n, u32 cap, u32 0x10 (0 in old files), n x u32 ref`; an empty
  list is `0x30000002, 0`.
* **Suppressed feature: flags1 (byte 10) bit 0x04** (byte 11 0x66 instead of 0x62). Confirmed:
  the suppressed holes of `side 2` (Hole3/4) and `top cover 2` (Hole3) are absent from the
  result body; after skipping them `side 2` rebuilds exactly. 19 features in 7 parts.
* Strings: `u32 nchars` + UTF-16LE.

### Feature list and order (confirmed)
* `2b48a42b-11d2-6458-6000-74b79b49ebb0` **Name**: `ref target` at offset 14, a list, the
  UTF-16 name, then for features a **16-byte type GUID**:
  `0ca91131` extrude, `0054015f` revolve, `1f757d1a` hole, `f1f715dc` fillet,
  `f900713f` chamfer, `767ac2da` rectangular pattern, `7281f4ec` thicken, `28aa1906` thread
  (cosmetic). Confirmed by the default names (Extrusion/Umdrehung/Bohrung/Fillet/Fase/...)
  and renamed ones (`Körper`=extrude, `Fase1`=revolve in the M8 screw).
* Feature records: `90874d91` (part feature) and `20673244` (pattern feature). Their body
  starts with `u32 subtype`, then **the slot list** (`0x30000002` list of fixed length per
  type: extrude 31, revolve 20, hole 26/28, fillet 17, chamfer 13).
* **History order** = the order in which the features' ids (offset 18) appear in the single
  `452121b6` record. It differs from record order in 3 parts (LED Arm, LED Elbow, Leg Shelf,
  where the record order is not creation order either); the 452121b6 order is the natural one.
* The feature id is also written into the result body: ShapeManager attributes
  `NMx_*_tag` carry it as their 4th int (blend faces: `NMx_blend_tag`, holes `NMx_Hole_Brep_tag`,
  sweeps `NMx_Sweep_Generated_Brep_tag`). Confirmed on `L-Bracket` (Fillet1=84, Fillet2=110,
  Chamfer1=142) and used for all fillets/chamfers.

### Parameters (confirmed)
`90874d26` Param: `... u32 n, UTF-16 name ("d17"), u32 0, u32, u32, f64 value, f64 value,
u16 0, u16 0xffff` (value at len-20). Units cm / rad.

### Sketches (confirmed)
* `90874d11` Sketch (named by a Name record). Its refs include `90874d18` **placement** and
  `ce52df40` (5 f64; the last three = sketch normal).
* **Placement `90874d18` = sketch-to-model 4x4 matrix, compressed**: `u32 mask`; bit k
  (k<16) = entry k is +-1, bit 16+k = entry k is 0, both = -1; every other entry follows as
  f64 in row-major order. Columns: x axis, y axis, normal, translation (cm). E.g. `0x7deb8214`
  (no doubles) = YZ plane (x->Y, y->Z); `0x73de8421 + [3.7]` = XY plane at z=3.7 cm;
  `0x751784a1 + [5.5, ~0, ~0, ~0]` = origin (5.5,1,0), x=-X, y=+Y, normal -Z (bottom face of
  the L bracket, origin at a face corner). Checked against every exact rebuild.
* Attachment: `3d64ccf0` ties the placement to a work plane (`ce52df42`: 9 trailing f64 =
  origin, x axis, y axis; YZ/XZ/XY first); `90874d23` ties it to a face (refs to an origin
  work point `ce52df3e`, a direction `ce52df40`, the parent sketch placement, a face-naming
  record `509fb5cc`). The placement matrix already holds the result, so these are not needed.
* Entities (body: `u32 flags`, `ref sketch`, ...): flags 0x40 = projected/reference,
  0x80000 = construction, 0x10000 = hole centre.
  * `ce52df35` point: f64 x, f64 y (cm), then lists of attached curves.
  * `ce52df3a` line: list [start, end, (points on the line...)].
  * `ce52df3b` circle/arc: list [start, end] (both null = full circle), ..., `ref centre`,
    `f64 radius`, `u8` (1 for full circles). Arcs run counter-clockwise start->end (confirmed
    by the slot of `L-Bracket` and the flanges, which rebuild exactly).
  * `4507d460` ellipse (O-rings): centre, two axis points, radii. Not expressible in the tree.
  * `8ef06c89` work axis: f64 start(3), f64 vector(3) after a list.
  * Constraints (`90874d94`, `90874d96`, `e1108c00`, ...) and dimensions were not decoded.

### Profiles (confirmed)
Feature slot 1 (extrude/revolve) = `22947391` list of loop records `a477243b`; each refers
(through `cc0f7521`: `..., ref HistoryBlob, u32 body tag, u32`) to a **body of the history
ShapeManager blob** (record `f645595c` in PmDCSegment, tag 0x0357): a wire body per loop, or a
sheet body per region. `f9884c43` links the loops list to its sketch. The prototype maps the
loop geometry back into the sketch plane and picks one point per Gusset region (grid flood
fill over the sketch curves, kept when inside an odd number of profile loops).

### Extrude slots (confirmed on the exact rebuilds; 80 extrusions)
0 `729abe28` operation: last u16 1 new, 2 cut, 3 join (4 intersect: guess, unseen).
1 loops. 2 `ce52df40` direction (= the sketch normal). 3 `90874d28` flag, last byte 1 =
**reversed** (opposite the sketch normal). 4 distance param. 5 taper param (always 0 here).
6 `92637d29` extent: last u16 1 distance, 2 symmetric (total distance, split), 5 through all.
7 flag (second direction?; set together with symmetric).

### Revolve slots (confirmed on TITAN flanges; 7 revolves)
0 operation (as extrude), 1 loops, 2 axis = `8ef06c89` work axis (start, vector), 3 extent
`92637d29` (3 = full 360), 4 angle param (pi/2 default when full), 5 `c7a06ac2` (direction?).

### Hole slots (confirmed: 9 parts with holes rebuild exactly)
0 `43cd7c11` type u32 (3 simple/drilled; bit 0x20000 = counterbore: guess, not yet
confirmed by an exact part), 1 diameter, 2 depth (to the drill shoulder), 3 cbore diameter,
4 cbore depth, 5 csink angle, 6 drill point angle (0 = flat), 7 list of centre sketch points,
8 placement of the centres' sketch, 9 extent (1 blind, 5 through all), 16 direction (outward
face normal), 20 csink diameter (guess). Tapped holes carry the minor diameter (M3: 2.459).

### Fillet / chamfer (partly confirmed)
* Fillet slot 0 `1b48e9da` list of edge sets `aad64116` (each: edge collection `90874d51`,
  radius param, ...). Edges are referenced through the naming system (`375c6982` ids), not
  decoded. Instead the prototype takes the result body's faces tagged `NMx_blend_tag` with the
  feature id, rebuilds the original edge from the two support faces (plane-plane lines;
  plane+coaxial cylinder circles for torus/cone blends) and its end faces, and passes the edge
  midpoint. Full circles use Gusset's seam convention (midpoint = centre - R*ref, ref = world X
  projected, or world Y when the normal is along X; found by experiment with the binary).
* Chamfer: slot 2 distance (cm), 3 second distance, 4 `7daa0032` type, 10 angle.
  Only equal-distance chamfers are emitted.

## 3. Coverage (Gusset debug binary as oracle, `rebuild.py`)

**16 of 35 parts rebuild exactly** (volume rel. error <= 9e-8, box <= 8.1e-5 of the diagonal):
AL2O3_tube, ALUMINUM_sleeve, FIBERGLASS_rod, GLASSFIBRE_isolation_inner/outer, GLASS_tube,
PIA_flange_inlet/outlet, TITAN_flange_inlet/outlet, TITAN_nut_big/small,
TITAN_pressure_ring_big/small, Base Plate, side 2.

Not exact, with the cause:
* 4 O-rings: ellipse sketch curve (no ellipse in the tree format).
* L-Bracket: 3 rectangular patterns (direction record `a244457b` not decoded; count and
  spacing are params, e.g. d17=3, d18=3 cm).
* Top motor plate, bottom plate, bottom plate mod: thicken (unsupported) plus unresolved fillets.
* Others: fillets/chamfers whose support faces are not plane/plane or plane/cylinder
  (spline or blend neighbours), or whose edge midpoint from the result body is not the
  pre-fillet midpoint (Gusset then refuses the pick); one revolve-cut hole fallback each in
  `side` and `bottom plate(s)` refused; `M8` first extrude refused (cause not found);
  `LED Arm` Extrusion2 refused; `bottom cover` and `top cover 2` keep a large volume
  difference that the unresolved fillets do not explain (open).

## 4. Open questions
* The PmAppSegment/PmGraphics/PmResult record layout (framing differs).
* Edge naming (`375c6982`, `509fb5cc`, `NMx_*` int lists) - only the feature-id field is used.
* Hole counterbore/countersink flags (0x20000 guessed), chamfer types, revolve slot 5,
  extrude slot 7, pattern direction, thicken, the second extent direction.
* Multi-body parts and op code 4 (none in the samples).
