# SolidWorks part feature tree (clean-room notes)

Derived from sample bytes only (2026-11-09; 33 public OLE2-era parts of the Slip-Pad hardware project, 3 NIST parts in the newer chunk format). Prototype decoder: `scripts/re/solidworks/` (`python3 -I swfeat.py OUTDIR PART.SLDPRT...`). Status: ADR 0043 amendment.

# SolidWorks part feature history: layout notes (clean room)

Everything here comes from the sample bytes only: 33 Slip-Pad OLE2 parts, 3 NIST parts in
the newer chunk format, and the Parasolid body that Gusset imports, used as ground truth.
"Confirmed" means the rule held on every sample it was tested on. "Guess" means it fits the
bytes but has not been checked against an independent value.

## 1. Containers
* OLE2 parts: the streams are `Config-0-ResolvedFeatures`, `Config-0`, `Definition`, ...
  A `*__ZLB` stream is a 16-byte GUID `231dd571...ef99`, then two u32 sizes, then a zlib
  stream that starts at offset 24. Confirmed.
* `Config-0-Partition` and `-GhostPartition` hold a u32, the GUID, sizes, and then zlib at
  offset 28. Inside is a Parasolid binary transmit file (`PS\0\0\0?: TRANSMIT FILE`). The
  sketch geometry is not stored there: GhostPartition holds no sketch doubles.
* Newer format (NIST): the same object stream sits in `Contents/Config-0-ResolvedFeatures`
  inside the chunk container (see sldasm_rot.py).

## 2. Object-stream framing (MFC CArchive style). Confirmed.
* Every object starts with a u16 tag:
  * `FFFF`: a new class. Then u16 schema (always 1 so far), u16 name length, and the ASCII
    name. The class takes the next map index, and the object it introduces takes the one
    after that.
  * `8000|i`: a new object of the already declared class with index `i`.
  * `0000`: a null pointer.
  * Any other value: a back-reference to the object with that map index.
* Class indices and object indices share one counter.
  * Config-0 evidence: moPart_c=1, moHeader_c=3, su_CStringArray=5. Then `05 80` is a new
    su_CStringArray object (index 7), suObList=8, moLogs_c=0xA, and later `0a 80` and
    `0c 80` follow.
* **ResolvedFeatures continues the map where Config-0 ends.** The first node in
  ResolvedFeatures uses tag `45 80` (shaft_3x22mm), which is class 0x45 = moNodeName_c as
  declared in Config-0 at 0x892.
  * This held for all 33 parts: the moNodeName_c index from a Config-0 scan matches the tag
    used in ResolvedFeatures.
  * The walker finds the base index (the number of map entries in Config-0) by
    self-calibration. Shaft: base=170, and class 0xD1 = moRefPlane_c is checked by
    `d1 80` before "Top Plane".
* CString: u8 length (`FF` -> u16, `FFFF` -> u32). The prefix `FF FEFF` means UTF-16LE,
  with the length repeated after it. Example: `ff fe ff 08 "Comments"`.
* The stream starts with a u16 count (0x11 for the shaft), then the top-level objects. In
  the newer format a u32 comes first.
* Tree node (common head, confirmed on folders, ref planes and sketches; extrusions use a
  longer head):
  `[moNodeName_c obj: CString name, u32 0] u32 flags (0x40000000 / 0xC0000000 / 0x40000140)
   u32 feature-id, head (59 bytes for folders/planes/sketches; contains u16-counted
   back-reference lists for features, e.g. extrusion head = 10x00, 01 00, <backref>, ...),
   CString comment, 46 bytes starting 00 FFFF 0000 01 FF*8 0000803F-like (f32 -1),
   u32 stamp, u32 stamp, u16 2`, then the class-specific tail.
  * Newer files have `...00 03 FF*8...` at the start of the 46-byte block.
  * Parameters (moLengthParameter_c, ...) use the same nodename. After it comes
    `u32 0, u32 0x40000000, u32 0xFFFFFFFF, u32 0, u16 0, f64 value(m or rad)`.
* Class tails read exactly so far (walker.py/classes.py):
  * Folder: `u16 flag, u16 count + objects`.
  * moFavoriteFolder_c/moHistoryFolder_c: one extra u16.
  * moHistoryFeatItemData_c: 18 bytes plus a component object.
  * moUnitComponent_c: 12 bytes.
  * moComp*: unitcomp, 31 bytes, u32 feature-id, u32 stamp, then class-specific fields.
  * moRefPlane_c: 116 bytes, 3 f64 label position, u32, 6 bytes, plane-data object.
  * moDefaultRefPlnData_c: 3 f64 origin, 3 f64 normal, u8 hasRot, [9 f64 R], 3 f64 t,
    f64 scale=1, u8, 4 f64 extent, then 49 bytes.
* **Status of the typed walker:** it reads the 13 folder and reference-plane nodes exactly
  on 32 of 33 parts (`rf.py`). It stops inside the first sgSketch, where the relations and
  dimensions are not decoded yet. loadcell_spacer_v1 fails earlier, in the folders.
  * About 100 of the 121 classes still have no reader, so no part is walked end to end yet.
  * The feature list and its order come from certain anchors instead (`nodes2.py`). A
    feature node is a moNodeName_c tag followed by the 46-byte block. Its class is the
    declaration right before it, or a class-ref tag. A class-ref tag is mapped to a declared
    class by signature, which is a guess. It gave the expected classes on all 33 parts after
    one fix, where an extrude/revolve ambiguity is settled by the end-spec class.

## 3. Sketches (moProfileFeature_c -> sgSketch). Confirmed by bounding boxes.
* The sgSketch body is `u32 nPoints` then the items, followed by more lists (lines, an
  unknown kind, arcs, ...). Each list is a u32 count plus items.
* Item: `u16 x0, u32 a, u32 id, entity`.
* Entity header: `FF FF 1F 00 01 FF*8 00 00 80 BF`. Byte 4 is 03 in newer files.
* After the header: u32, u16 x4, f32 -1, `obj ext`, u16 flags, u8, f64 0.0, f64 1.0. Newer
  files add 8 more bytes here.
  * `obj ext` is null, or for projected geometry (for example the origin) an sgExtEnt_c
    chain: moSketchExtRef_w, then moCompSketchEntHandle_c, then a CString such as
    "Point1@Origin".
  * Flag bit 0x08 means construction.
* Point payload: `u16 type (0x1E point, 0x1F origin), f64 x, f64 y` in metres, in sketch
  coordinates. After that: u16, u16 n, two handle objects (`u16 ent, i32 -1, u32 0`, which
  link the point to its segments), u16, `FE FF FF FF`, 34 bytes, u16.
* Line payload: `u16 start, u16 end` (0-based indices into the point list), `u16 1`
  (0 = centerline), u16 0, f64 -1.
* Arc/circle payload: same start as a line, then `i32 dir, u16 centre, 4 x i32 -2`.
  * dir -1 means clockwise from start to end. This was checked on idler_differential_arm:
    the arc around (10,6) passes through y=7.5, and the true box top is 7.499.
  * start == end means a full circle.
* Plane: the sketch span contains a **moCompRefPlane_c**: comp base (u32 = plane feature
  id: 2 Front, 3 Top, 4 Right), then `u16 0, u32 k, u8 hasRot, [9 f64 R], 3 f64 t, f64 1.0`.
  * A sketch on a face holds a plane-data block instead (origin, normal, hasRot, R, t).
  * The columns of R are the sketch x, y and normal in model space, and
    `v_sketch = R^T v_model + t`, so the origin is `-R t`. Base_loadCell Sketch2 has
    t=(0,0,-0.003), which puts its plane at Y=3, the top face.
  * Default planes: Front = XY; Top has x=+X and y=-Z; Right has x=-Z and y=+Y.
  * Bounding boxes from the extracted sketches match the Parasolid body within 0.01 mm on
    PCB, loadcell_spacer, drive_belt, both shafts, both PET tapes and both idler arms.
* Dimensions: in the shaft, the "D1" moLengthParameter_c = 0.003 is the circle diameter,
  and the radius from the geometry is 1.5 mm. A first D1 = 0.00633 is also present and its
  meaning is unknown.

## 4. Extrude / revolve (guesses noted)
* moExtrusion_c is the first boss. moICE_c is used for later bosses and cuts. moCut_c is
  another cut class.
* moEndSpec_c: `u16 0, u32 X, 10x00, u32 endType, u32 flag, obj dimension`.
  * endType 0 = blind and 6 = mid-plane are confirmed: shaft_3x55mm is -27.5..27.5 and the
    PET tapes are z = ±2.
  * endType 2 = through all is a guess: no dimension object follows it.
  * endTypes 1, 4 and 9 are unknown (up-to-next/surface/offset?).
  * `flag` is probably "reverse direction" (guess).
* The depth is the value of the first length parameter in the feature's span. Revolve
  angles appear as parameters of 6.283185 rad.
* The profile sketch is taken to be the sketch just before the feature in stream order
  (guess). The extrusion head also holds a back-reference to it.
* Boss or cut for moICE_c: **no flag found yet**. The prototype uses the class (moCut_c)
  and otherwise the feature name ("Cut-...").
* Fillet/Chamfer: only the radius (first parameter) is read. The edges are not decoded.

## 5. Round 2 additions

Everything below was checked by rebuilding the emitted tree with the gusset binary
(`rebuild.py`, which runs `gusset build ... --import x.gtree.json` and exports an STL) and
comparing against the Parasolid body: volume within 1e-4 relative and box within 0.01 mm.

### 5.1 Sketch lists. Confirmed.
* The 8 bytes in the entity header are not always FF*8. Centerline records have
  `ff ff ff ff 04 00 ff ff`, so headers are matched as
  `FF FF 1F 00 <1|3> <8 bytes> 00 00 80 BF`. Missing these records skewed the earlier list
  measurements.
* Lists: `u32 count`, then each item is `u16 x0 (0 or 2), u32 a, u32 id, entity`.
* List order: points, lines, two lists that were empty in every sample, arcs. Then 32
  bytes, then a u16 that is the count of the dimension objects that follow (sgPntPntDist /
  sgCircleDim ...). Sketches whose last item is a line show 48 zero bytes there instead of
  the expected 46. These 2 bytes are not explained, so the typed walker still stops at the
  end of the lists.
* Line payload `u16 start, u16 end, u16 kind`: kind 1 is a normal line and kind 0 a
  centerline (confirmed: the bearing revolve axis). Flag bit 0x08 means construction.
* **Item key `a`:** the u32 before the record id. It is the key that extrusion side faces and
  `moSketchChain_c` use.
  * moSketchChain_c body: `u16 n, n x u32 a`, which lists the profile loop in order.
  * Base_loadCell Sketch1 lists 9,4,3,2,1,8, which matches the geometry: right-mid,
    right-lower, bottom, left, top, right-upper.

### 5.2 End conditions (moEndSpec_c)
* Body: `u16 0, u32 X, 10 x 00, u32 type, u32 flag, obj dimension`.
* Each extrude has 2 or 3 of these objects. The first one is direction 1.
* The EndSpec class index is learned as the most common class-ref tag that has this
  layout and X=1, with indices >= 0x3000 excluded (those are doubles).
  * The first round picked the wrong class in about 20 features. For example,
    baseJoint/Boss-Extrude6 is really type 6 (mid-plane).

| type | meaning | evidence |
|---|---|---|
| 0 | blind, D1 = depth | many parts |
| 6 | mid-plane, D1 = total depth | shaft_3x55mm, PET tapes, baseJoint |
| 9 (flag 1) | through all, both directions | loadCell_v1: symmetric ±500 rebuilds to the exact volume (11401.140 mm³); a one-sided cut does not |
| 1 (flag 1) | through all, both directions | idler_single_v2: one-sided removes exactly half (65.08 mm³) |
| 1 / 2 (flag 0) | through all one way (guess) | emitted as -500 mm for cuts; Base_loadCell, Top_modular, Top_beltSupportRim come within 1.5% (their remaining features are unresolved) |
| 4, 5 | unknown (up to vertex/surface?) | left unresolved |

* `flag` = 1 means "both directions", not "reverse". I found no reverse flag:
  * The sign search (`search.py`) never needed a flip that a field explains.
  * `X` is 1 on most first features and on through-all cuts. Its meaning is unknown.

### 5.3 Boss vs cut and direction
* moICE_c and moCut_c carry no boss/cut flag that I could find. The operation still comes
  from the class (moCut_c) or the name ("Cut-...").
* **Cuts go against the sketch normal.** Confirmed: Base_loadCell, Top_modular,
  loadCell_v1 and spring_holder only rebuild with this rule. Bosses go along the normal.

### 5.4 Fillet / chamfer edges (moEdgeRef_c)
* Edge record header: `u32 1, u32 0, u32 nRefs(4), u8 0, u8 kind(2|3), u16 0, u32 id,
  2 x f64 sentinel 7dc39425ad49b254, u16 0`.
* Then nRefs face references: `<class tag> u16 0 <tag> u16, u32 feature-id, u32 stamp,
  u32 index`. Cap references are followed by one more u32 (0 or 1).
* **The first two references are the edge's two faces; the last two are the faces that
  bound its end vertices.**
* Side face of an extrusion: index = the sketch item key `a`. Cap face: index 0 = start,
  1 = end.
* Midpoint rule:
  * side/side edge: the shared sketch point, at mid-depth;
  * side/cap edge: the midpoint of the segment, at the cap's height.
* The fillet's own selection is the first run of edge records in its span.
* Results:
  * Base_loadCell Fillet1 resolves 4 of 7 edges; with them the part comes within 1.46e-4.
  * loadcell_centerPlatform now matches.
  * The idler arms resolve 16 of 30. The unresolved edges use side keys (15-19) that no
    item of the profile sketch carries; mirrored or derived entities are suspected.
* Records written by other classes (e.g. some Chamfer_c) do not match this header yet.
  Some fillets show "0 of 0" found.

### 5.5 Still open
* Typed walker past the sketch lists (dimension objects): not done. So back-referenced
  sketch planes and the stream-based profile link are still not available. The profile
  sketch is still taken to be the one just before the feature.
* Hole wizard, mirror, patterns: emitted as `{"type":"unsupported"}`.
* idler_single_v1: the Parasolid truth volume is negative (-163.557), so the truth itself is
  suspect.

## 6. Round 3: hole wizard

### 6.1 Feature layout. Confirmed on all 11 hole-wizard features in the samples.
* The moHoleWzd_c node is short (about 0x75 bytes).
* It is followed by exactly two moProfileFeature_c sketches:
  * a **placement sketch** on the face;
  * a **section sketch**, which often has no entities. An empty sketch has no entity
    headers at all, which is why it used to fail as "point count ... > records 0".
* After the two sketches come the hole's dimension parameters, with these names:
  * `Hole Dia.`, `Thru Hole Dia.`, `Tap Drill Dia.`
  * `Hole Depth`, `Thru Hole Depth`, `Tap Drill Depth`
  * `C'Sink Dia.` / `Near C'Sink Dia.`
  * `C'Sink Angle` / `Near C'Sink Angle` (radians; 1.5708 = 90°)
  * `Head Clearance` (0.15 mm)
  * a parameter named `4` with value 25.4 mm (meaning unknown)
* **Hole centres** are the stand-alone points of the placement sketch: point records whose
  u16 after x,y (field `A`) is 1.
  * Line endpoints have A=0, and records with id -1 are dimension helpers.
  * Checked: motorClip (0, ±8.5) and spring_base_tab (±7, 2.5) lie on the true holes;
    the rebuild volumes move by the expected amount.
* Face: the placement sketch plane; the normal is the sketch normal, pointing out of the face.

### 6.2 Head clearance. Confirmed by volume, not emitted.
* motorClip still misses by 4.57 mm³ = 2 x π·2.2²·0.15. That is the head-clearance cylinder:
  the countersink is preceded by a cylinder of the countersink diameter, 0.15 mm deep.
* gtree `hole` cannot express it. A pocket cut plus a hole on the pocket floor did not
  work: the importer silently drops the hole.
* The prototype writes `head_clearance` (mm) as an extra field.

## 7. Round 4

### 7.1 Head clearance emission
* Each countersink hole that has a `Head Clearance` parameter is now emitted as, per
  position:
  1. a circle sketch of the countersink diameter on the face;
  2. a `-clearance` cut;
  3. a hole from the pocket floor (`position - clearance * normal`).
* The round-3 "vanishing hole" was a single hole whose two positions lie on two different
  pocket floors. Gusset drops such a hole silently: no skip/fail line, no volume change.
  * Repro: `rebuild/motorClip_clearance_vanish.gtree.json`, run with
    `bin/gusset build truth/empty.gus.txt --import <it>`.
  * The same tree with one position imports the hole.
  * The prototype therefore emits one hole per position.
* Result: spring_base_tab goes from 735.72 to 732.52 against a truth of 723.785. That is
  4.37 mm³ per hole still missing after the clearance, so the clearance does not explain
  the remaining error.
* motorClip has no `Head Clearance` parameter, but its gap (4.57 mm³ = 2 * pi * 2.2^2 * 0.15)
  looks like one. Applying a 0.15 clearance anyway only takes it from 7.0e-3 to 2.4e-3, so it
  is not applied.

### 7.2 End type 1, flag 0
* The EndSpec bodies of all type-1 cuts are byte-identical. This was checked on
  Top_modular/Cut-Extrude2, spring_base_tab/Cut-Extrude1, Top_beltSupportRim/Cut-Extrude1
  and 5, and spring_base/Cut-Extrude2.
* Yet spring_base_tab needs the inward through-all, while Top_modular only matches if
  Cut-Extrude2 removes nothing.
* So nothing in the EndSpec distinguishes them. The difference must come from elsewhere:
  the sketch plane, profile-region selection, or another field of the feature.

## 8. Round 5

### 8.1 Edge records, generalised
* The header can carry 0-2 extra zero bytes before the kind byte; it is matched as
  `01 00*7 n 00{4,6} kind 00 00 id sentinel*2 00 00`.
* Face references carry 0, 1 or 2 extra u32 after the 20-byte core:
  * plain side (moFromSktEntSurfIdRep_c): 0
  * plain cap (moEndFaceSurfIdRep_c): 1
  * ICE side and cap (moFromSktEnt3IntSurfIdRep_c / moEndFace3IntSurfIdRep_c): 2,
    i.e. `-1, 0` for sides, `n, 0` for caps
* Cap classes are learned per file. A class counts as a cap when either:
  * it is followed by exactly one extra word on a reference that is not the last in its
    record; or
  * it bounds a side/side edge with indices {0, 1}.
* Face references can name faces of different features, e.g. a cut wall against the boss's
  top cap.
  * Side/cap edge: the midpoint of the side segment, clipped between the crossings with
    the bounding side faces when they belong to another feature, and moved onto the cap
    plane along the side's extrusion direction.
  * Side/side edge between two features: the 2D crossing of the two segments seen along
    the normal. Its height is halfway between the bounding caps, or between the caps of the
    non-through feature.
* Confirmed: Base_loadCell Fillet2 rounds the four vertical edges of the Cut-Extrude2
  pocket (ICE side keys 26-31, bounded by Boss-Extrude1's caps). With it, Base_loadCell
  rebuilds exactly.
* Confirmed by rebuild: references to faces the profile sketch no longer has are stale and
  can be dropped. Base_loadCell Fillet1 lists 7 edges; only the 4 corners exist, and the
  part is exact with those 4.

### 8.2 Gusset observations
* idler_differential_arm_short Fillet2 has two edges, the concave corners at (+-8.5, 3.5).
  * Each edge alone rounds (volume 245.179 -> 244.834).
  * Both edges together report `ok` and are not skipped, but the volume stays 245.179.
  * Repro: `rebuild/arm_fillet2_noop.gtree.json`.
* `mcp.py` imports a tree through `gusset mcp` (`run import.file`) and prints
  `skipped_user_text`. It needs `mcpw/p.gus` (made with `gusset pack`).
