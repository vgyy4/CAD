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
  * `4507d460` ellipse (O-rings): centre, axis-end points, radii; see D. Emitted as tree `ellipse`.
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
* Chamfer: slot 2 distance (cm), 3 second distance, 4 `7daa0032` type, 10 angle, 8 `d83ef271`
  reference geometry. The type record ends `u16 2, u16 type, u32 0`: type 0 = equal distances,
  type 1 = distance and angle (confirmed on the M8 screw, `Fase2`/`Fase3`: slot 2 = 0.0677 cm
  measured on the planar face, slot 10 = 30 deg, the other leg on the cylinder is
  0.0677 tan 30 = 0.0391 cm in the result body; slot 3 is then a stale default). Slot 8 lists two
  `797737b1` records (kind u32 at offset 10: 2 line, 5 curve), one per support face of the first
  edge, in the part's cm: f64 triples from offset 22, first the start point on the edge, then the end
  point `distance` away inside that support face. In the M8 the first record runs along the face of the
  first distance (length = slot 2); equal chamfers show the same two lines of equal length. The
  importer emits `angle` (deg) and `face: {point: end of record 1}` for type 1 on a single edge; an
  unseen type (two distances is probably another value), a first line whose length is not the
  distance, or several edges (the pick would apply to all) stay refused. Only one non-equal
  chamfer exists in the corpus (the two M8 features), so which record is "first" rests on it.

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

# Round 2 (2026-11-09): edge tags, patterns, ellipses, thicken

# LAYOUT.md : additions to docs/formats/inventor-feature-tree.md (round 2)

Scratch setup notes
* The repo's scripts/re/inventor/ lacks `ole.py stlbox.py sab.py blobs.py sk.py` (imports). Re-created in proto/:
  ole.py/stlbox.py copied from scripts/re/solidworks, sab.py from scripts/re/fusion (tokens() now stops at the
  first unknown tag = past `End-of-ASM-data`), blobs.py (`part_blobs`: B*.bin blobs `ASM BinaryFile` with the u32 record type 8 bytes before: 0x031d result, 0x0357 history), sk.py (`backrefs`).
* 47 .ipt in the 4 records; truth for 45 (the 9434K580 spring and one other time out at 150 s in Gusset itself).

## A. Edge naming of fillet/chamfer (CONFIRMED on L-Bracket, Connecting plate 2, Heatpad Tray)
* Feature slot 0 `1b48e9da` -> list of edge sets `aad64116` -> `90874d51` (edge collection) -> `375c6982` (one per edge).
* `375c6982` is back-referenced by `90874d53` (record "edge name"): body = `u32 flags(0x0501..)`, ref Solid body record,
  `u32 2, u32 0, u32 2`, list(tag 0x30000008) of the source (e.g. `3d 00 00 00` = non-ref index+1 of a sketch line),
  empty list, `00`, ref `375c6982`, ref `cce92042` (the TAG TABLE, see below), **`u32 tag` as the LAST 4 bytes**.
* `cce92042` (one per part) = table of `u32 tag, ref` pairs: tag -> entity record (`2e04a208`, `0645c2a5`, `9bb4281c` ...).
* That last-u32 `tag` equals, on every checked fillet/chamfer, the 8th int (ints[7]) of the `NMx_blend_tag` attribute on the
  result body's blend face(s) of that feature (feature id = ints[3]). A split edge gives several blend faces with the same tag.
  => The edge named by the feature and the blend face(s) it produced are linked 1:1 by the tag. (script edgetags.py)

## B. Why midpoints failed, and fixes (CONFIRMED by exact rebuilds: side, Conecting plate 1)
1. The midpoint taken from the RESULT body is the middle of the edge as it is LAST (after later cuts trim/split it). The
   importer (tree_import.rs `edges`) first needs a model edge midpoint within 0.01 mm, then falls back to `path`
   (`containing_edges`: the model edge that holds midpoint + first + last path point). Fix: emit
   `{"midpoint": m, "path": [m-eps*t, m+eps*t]}` with m ON the pre-fillet edge (line: along the support-plane
   intersection; circle: on the circle, +-eps/R rotated) and eps = 0.1 mm: the importer then picks the model edge that
   contains the point whatever its extent at that time. (blend.py local_path / EDGE_INFO)
2. Planar faces carrying a FILLET's `NMx_blend_tag` are leftovers (an edge split later); drop them (a fillet face is never planar).
3. circ_mid bug: atan2 gives -pi and +pi for the same angle; after round() they differ by 1e-9, which made the "largest gap"
   a bogus 2pi-1e-9 and the arc midpoint the vertex. Normalise to [0,2pi) and merge angles < 1e-6 apart.
4. Hole fallback (revolve cut): the importer's sketch origin is the foot of the WORLD origin on the plane, so a revolve axis
   given as origin+direction at the hole centre is refused ("axis off the sketch's origin"). Use the profile's closing
   line (apex -> start, on the hole axis) as `"axis": {"line": "<id>"}`. => `side` rebuilds exactly.
5. The CLI `gusset build` prints only ok/FAILED per feature, not why a feature was skipped. The reason is in the
   `import.file` result `skipped_user_text` (MCP: `gusset mcp m.gus --no-attach`, tool `run` {name:"import.file",args:{path}}),
   see proto/mcpcall.py + skips.py.

## C. Rectangular pattern (L-Bracket exact rebuild, 3 patterns)
Pattern feature record `20673244` (class 'PatternFeature'; name type GUID `767ac2da`). Its refs, in record order:
* the patterned features (`90874d91` part features and/or earlier `20673244`), in pattern order; a second direction is a
  SECOND pattern feature whose list is [originals..., first pattern] (CONFIRMED: Pattern3 = [Hole1, Pattern2]).
* `a244457b` DIRECTION (125 bytes): the LAST 81 bytes are `3 x (3 x f64 cm)` then `u8 1, ref, ref`:
  `A (3), V (3), B (3)`. Edge reference: A start, V = B - A, B end (L-Bracket P1/P3: A=(-4.5,17,0), V=(9,0,0), B=(4.5,17,0) =
  the end-face edge, LED_Arm: V=(-1.75,0,0)). Axis/other reference: A = B = a point, V = direction x length
  (L-Bracket P2: A=B=(5.5,1,0), V=(0,15,0)).
* `bb1dd5df` COUNT: the one whose last u32 is the count (>1) (the other `RDxVarN` has 0); both u32 at the end are the count.
* `90874d26` Param = spacing (cm; L-Bracket d18=3 -> 30 mm, d29=0.7 -> 7 mm).
* `90874d28` flags (4 at the end of the record order). LAST one = 1 on P2 only. P2's rebuilt hole centres are 151,158,165
  around the original Hole1 at 158 => flag = MIDPLANE (symmetric about the original). [GUESS from one case]
* SIGN (GUESS, 2 observations, same edge): the pattern runs along -V (L-Bracket P1: cut slots start at x=+55 mm and go -X;
  P3: holes 30,0,-30). Under midplane the sign is irrelevant (P2). Verified by volume 196765.929 = truth and the hole centres.
* Gusset has no midplane pattern: emitted by moving the original hole by -(n-1)/2*spacing along the direction (positions of the
  hole feature), only for hole features (others -> unsupported).
* Volume cannot distinguish the spacing for holes in solid material: verified with hole centres from the STL (groups.py).

## D. Ellipse sketch curve `4507d460` (4 O-rings; CONFIRMED against truth volume and bbox, axis-aligned only)
Record tail (offsets from the record END, all ellipses seen): `f64 s, f64 a, f64 b, u8 1` at len-25, len-17, len-9
(s = value at len-25). Header: `u32 flags(0)` after the common header; refs (high-bit u32) include the sketch (offset 14),
the points: for the three listed points `[axis end 1, axis end 2, centre]` order varies, the CENTRE is the ref'd
`ce52df35` SkPoint with flags 0 (the others have flags 0x10000 = "hole centre" bit; in Oring_65x3 only the centre is ref'd).
* `a` = semi-major, `b` = semi-minor (cm), `s` = y component of the major axis direction (0.0/-0.0 = along sketch x, 1.0 = along sketch y).
  Data (cm): Oring_10x5 centre (0.785,0) s=1 a=0.324 b=0.215; 30x3_5 centre (1.675,0) s=0 a=0.175 b=0.158505;
  54x3 centre (2.91,0) s=1 a=0.199 b=0.12; 65x3 centre (3.4,0) s=0 a=0.15 b=0.119039.
* Checks: V = 2*pi*Rc*pi*a*b with Rc = centre x (mm) gives the truth volume (1079.401 for 10x5); bbox: radius = Rc + semi-x,
  z = +-semi-y (the revolve axis is the sketch line x=0 along sketch y; 10x5 z +-3.24 = a (s=1), 65x3 z +-1.19 = b (s=0)).
* So: semi_x = a if s==0 else b; semi_y = b if s==0 else a. A general rotation would need cos too: s alone is a GUESS
  (sin of the angle); no rotated sample exists.
* Revolve: `Umdrehung1` extent 3 (full), axis = the construction line `ce52df3a` #34 on the sketch y axis (points (0,-0.45),(0,0.45)).
* Implemented (2026-11-09): tree `{"type":"ellipse","center":<centre point>,"major":[cos,sin],"a","b"}`. Reader (`inventor_features.rs`
  `ellipse()`): `s` = sin of the major-axis angle; the referred SkPoints are centre + major-axis end + minor-axis end (65x3 refers only
  the centre). The centre is the referred point from which every other referred point lies 0, `a` or `b` away; the x sign of the
  direction is taken from the major-axis end point when present, else positive (a point at distance `a` whose y offset = s*a). All 4
  O-rings now import as history (volumes match the stored shapes to 1e-7; warning only about 'rot_spl_sur' approximations).

## E. Cylinder blends generalised (CONFIRMED: back_accses_panel exact)
A cylindrical blend face (cone-surface with zero half-angle... `cyl` in blend.py) replaced a STRAIGHT edge parallel to its axis.
Cross-section the problem perpendicular to the blend axis u: the blend circle (radius r, centre C) is tangent to the two
support faces: planes parallel to u (|dist(C,plane)| = r) or cylinders parallel to u (|centre distance| = R+r or |R-r|).
The pre-fillet edge = the support curves' intersection nearest to C (line/line, line/circle, circle/circle), a line along u;
its range comes from the blend face's vertices (clamped to end planes as before). Replaces the plane-plane-only rule
(failure texts "cylinder blend with 0/1 planar supports").

## F. Thicken (`7281f4ec`; bottom plate, bottom plate mod, top motor plate)
* Slots: 0 `eff2257a` (face reference) -> `bf8b8868` -> { `24bcb2f1` -> `509fb5cc` (face name), **Param = thickness (cm)** (bottom plate "d24" = 2.0 =
  20 mm) }; 1 operation `729abe28` (3 = join); 6 Param d25 = 4.4 (cm; not the thickness, meaning unknown); 12 Param
  `RDxVarN` = 10.0 (unknown). The face name `509fb5cc` is back-referenced by `03aa812c` (refs a SkPoint through `f3fc69c6`,
  the tag table `cce92042` and tag keys): a face named through a sketch point of Sketch1; NOT decoded to a face.
* Result body: planar faces tagged `NMx_Brep_Feature_tag` (4 side planes) with ints[3] = the Thicken id; the cap is the one
  lying exactly `thickness` outside the base block (bottom plate: x=+73 vs base +-53; top motor plate: x=-73).
* EQUIVALENCE (CONFIRMED on the bounding box of all 3 parts: box err 2.7e-18/9.4e-18; volume within 0.2..1 % but not exact because
  other fillets of these parts are still unresolved): thicken of the base block's side face by d == extrude (join) of that side's
  outline (the base block's y/z extent), `distance = -d` from a sketch plane at the cap with outward normal. NOT `shell`.
  (The slab is bounded by the base block's extent, so a later fillet on the base's side edges shows up on the slab.)

## G. Other findings (round 2)
* Chamfers of the M8 screw (`Fase2/3`) carry `NMx_edge_blend_tag` (ints = [-1,42,1,featid,-1,tag]) instead of `NMx_blend_tag`
  (the same tag as the named edge, see A). blend.py `tagged()` now accepts both. They are circle chamfers (cone faces) that
  rebuild, but the volume stays 0.6 % off: the cone half-angle is 60 deg from the axis while both distances are equal
  (d8 = d40 = 0.06766 cm), i.e. the legs are NOT equal; scanning the distance gives the truth volume at d ~ 0.5175 mm
  (0.6766 mm * 0.765). Gusset's chamfer takes one distance, so asymmetric chamfers need two distances / distance+angle. OPEN.
* Full-round fillets ("supports do not meet": a cylinder blend whose two planar supports are parallel, radius = half the
  gap; bottom plate Fillet10/11, Top motor plate Fillet12, Long_Bottom_Nut_Traps Fillet2) have no edge before the fillet. Not expressible. OPEN.
* Blend faces of type `spline-surface-rb_blend_spl_sur-...` (rolling-ball blend between cone/plane supports) and
  `srf_srf_v_bl_spl_sur` / `VBL_SURF` (vertex blends): not analysed (edge is a conic, and the importer only matches Line/Circle edges
  by `path`, so a conic edge would need the exact Gusset midpoint). OPEN: LED_Arm/Elbow/Washer, Heatpad, Fixture Pull Bracket,
  Leg_Shelf, Thumbscrew 20mm M5.
* Thumbscrews: all features build, volume 1 % off: `Extrusion4` (last feature) is one tiny rectangle at the rim whose
  sketch is a (sketch-level) pattern of ~30 knurl notches; the profile list `22947391` of the feature has only the first loop
  (history bodies 129..142 are circles/helpers). The truth has the knurl, so the pattern instances are not in the loops. OPEN.
* top_cover_2: all features build; remaining 0.19 % = `Chamfer3`, whose 4 tagged faces are sliver vertex-blend pieces. OPEN.
* The history blob (`57 03 00 00`) holds only sketch/profile bodies and removed volumes (5 bodies in L-Bracket), NOT
  intermediate solids: pre-fillet bodies cannot be taken from it. The result blob (`1d 03`) is an ACIS history stream
  (`Begin-of-ASM-History-Data`, 23 `delta_state` records, records after `End-of-ASM-History-Section` are the deleted
  pre-feature entities, e.g. the pre-fillet edge 772 with its vertices (5.5,0,0) and straight curve) but their ref numbering is
  not the record index; not decoded. The tag route (A) + support-face geometry (B/E) was sufficient instead.
* Importer-side limits met: `path` picks only match Line/Circle edges; full circles (1 vertex) never match by `path`
  -> use the seam midpoint (build.py flips a pick to `_full` = centre - R*ref when the importer reports it missing).
