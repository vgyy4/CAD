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

### 8.3 Round 6 notes
* Mirror (FrameBracket Mirror1).
  * Its post-base body holds two u16-counted back-reference lists, `01 00 6c 01` and
    `01 00 6e 01`. Both point to object indices below the ResolvedFeatures base, i.e. objects
    in Config-0 (guess: the bodies).
  * It also holds a face list: header `01 00*7 08 00*11 sentinel*2 00 00`, then 7 references.
    The first is a cap (cap 1 of 'main flange'); the rest are its bounding side faces. Face
    lists hold n-1 references.
  * Mirroring the whole body about that cap plane (z=5) blows the box up to z 40. Mirroring
    about x=0 adds 3300 mm3 toward the true volume.
  * FrameBracket's own tree already has the wrong z extent (box error 20 mm without the
    mirror), so the plane cannot be confirmed yet.
* The truth itself is inconsistent on several parts.
  * Gusset's Parasolid volume and the volume of the STL it exports for the same body
    disagree beyond tessellation error:
    * motorClip 651.5 vs 663.7
    * spring_base 7829 vs 7960
    * finger_aperture 2712 vs 1279
    * Top_mainplate 2701 vs 2667
    * arm_idler_v2 2141 vs 2112
    * the idler arms 204.9 vs 194.0
  * Against the truth STL, arm_idler_v2 rebuilds within 5e-4 (2113.2 vs 2112.1).

## 9. Round 7 (after the Gusset cone fix)

The truth was regenerated with the fixed binaries. 18 parts now rebuild exactly; the 5 gained
this round are arm_idler_v2, motorClip, finger_aperture, spring_base_tab and Top_modular.
Nothing changed in the trees for these: their reference volumes were wrong before.

* **Face-sketch frame (confirmed by box).**
  * A sketch on a model face stores the face's plane data first; its axes may be turned
    180 deg about the normal.
  * The sketch's own frame follows as `01 00 00 00 <12 x 00> 01 <R 3x3> <t> 1.0`, with the
    same normal column.
  * FrameBracket Sketch11 put Boss-Extrude3 at z -20..0 with the face block. With the
    following block it sits at 0..20, matching the true box.
* **Mirror plane (confirmed by box).**
  * The moMirrorPattern_c body holds the mirror plane's frame:
    `<4 f64 display extents (+-88.99, +-55)> 01 <R> <t> 1.0`.
  * In FrameBracket Mirror1 that frame is the Right plane, x = 0, and it brings the
    FrameBracket box to the true box exactly.
  * The face list in the same body (cap 1 of 'main flange' plus its bounding faces) is not
    the plane. Its role is unknown.
  * The prototype emits the whole body mirrored with `merge: true`.
* **Segment flag bit 0x01: not part of the profile.**
  * baseJoint Sketch11 holds an arc with flags 5 whose centre does not even fit its end
    points (radii 35.0 vs 35.4).
  * Treating flags & 0x01 like construction brings baseJoint from 40% to 1.4e-4
    (159.979 vs 159.956). That part of the evidence is confirmed by volume.
  * The meaning of the bit is guessed; PCB's id -1 helper record carries it too.
  * Top_mainplate also improves, from 21% to 2.5%.
* Top_modular Cut-Extrude2 uses all regions of Sketch6. Its earlier mismatch came from the
  wrong reference volume.

# Newer chunk-container parts (2015+), round 1 (2026-11-09)

# LAYOUT.md — newer chunk-container .sldprt feature tree (clean room, from ARIG samples)
(append-only; newest at bottom)

## L1. Where the tree lives
* Chunk records (see crates/gusset-sldprt/src/chunks.rs / py/chunks.py) hold the OLE2-era streams under the
  prefix `Contents/`: `Contents/Config-<N>-ResolvedFeatures` and `Contents/Config-<N>` where N is 0 in 41 of 49
  samples and **1** in the 8 Servo_HS-485-HB revisions (so "Config-0" must not be hard-coded; the prototype's
  `Config-0-ResolvedFeatures` lookup misses them: "no ResolvedFeatures stream"). Match `Config-(\d+)-ResolvedFeatures`.
* Streams are already raw object streams: **no `__ZLB` GUID wrapper**, the container inflates them (raw deflate).
* Other streams: `Contents/Config-N-Partition` (Parasolid), `-GhostPartition` (17 of 49), `-StockBodies`
  (24 of 49, exactly the "<name>_Symétrique" mirror parts, see L2), `-ModelHeader`, `-LWDATA`, `Contents/Definition`,
  `swXmlContents/Features` (868-byte XML shell without features, useless).
## L2. Sample census (arig-revisions, 49 files)
* 33 of the 49 files (all `elfa_2021__Symétrie_*`: Access_HS-645, Angle_support, Igus_Bague_d06, Servo_HS-485-HB,
  carter_screen_side) are **mirror parts**: the tree is the 13 standard folders + 3 default planes + Origin +
  one `moMirrorStock_c` named "<part>_Symétrique" + empty folders. No sketch/extrude exists, the body derives from
  an external part. Not rebuildable from the feature list (unsupported, one feature each).
* Real feature trees: `elfa_2021_plaque_base_roulante` (5 revs), `elfa_2021_plaque_superieure` (6),
  `elfa_2021_Angle_support` (6; its Parasolid is damaged in Gusset: "XT file is damaged ... node type -15684", no truth).

## L3. Framing of Config-N-ResolvedFeatures in the chunk container (all 49 samples)
* Byte 0: **u32 B** (105..153 in the samples), byte 4: **u16 count of top-level objects** (0x11 in the 33 mirror parts, 0x19..0x44
  in the others). In OLE2 files the stream starts with the u16 count only; the u32 is new. First object (an `FF FF 01 00 12 00
  moCommentsFolder_c` class declaration) follows at byte 6 in every sample.
* B is the MFC-style map index at which this stream's first *class declaration* (moCommentsFolder_c) is numbered
  (checked on Igus_Bague_d06: with B=105, NotesAreaFtrFolder=131 and RefPlane=148 equal the tags `83 80` / `94 80` that later
  nodes use). I.e. class tags are `0x8000 | idx` and the numbering continues at B, as in OLE2 files where B was
  derived from Config-0. Counting the idx exactly needs every object tag, including the classes that were registered
  *before* this stream (nodename, unit component, ...): their tags are NOT the Config-0 declaration indices.
  moNodeName_c is `04 80` in plaque_*, `06 80` in the 33 mirror parts (Config-0 numbering says 3); moUnitComponent_c is
  `2b 80` (43) in plaque_*/Angle_support, found as the most common `XX 80 02 00 <10 x 00>` (py/uc.py `uctag`). Learn both
  from the bytes, never from Config-0 indices.
* Nodename tag: the 2 bytes after the first class declaration (`moCommentsFolder_c 04 80 ff fe ff <CString>`), the
  prototype's regex still works unchanged.
* Parameter objects (moLengthParameter_c etc.): `nodename "D1", u32 0, u32 0x40000000, u32 -1, u32 0,` then **`ff fe ff 00`
  (an empty CString, new in this format)**, `u16 0, f64 value`. Without the inserted CString the prototype's
  PARAM_TAIL regex never matches (=> "extrude distance None"). Fix: `(?:\xff\xfe\xff\x00)?` before the `\x00\x00`.
  Confirmed on 1000+ parameters in plaque_*/Angle_support; e.g. plaque_base_roulante Boss.-Extru.1 D1 = 0x3f70624dd2f1a9fc = 4 mm.
* EndSpec (moEndSpec_c) is unchanged: after the 11-byte name `00 00 | X u32 | 10 x 00 | type u32 | flag u32`
  (Boss.-Extru.1: X=1, type 0 blind, flag 0), then the dimension object (class tag then moFeatureDimHandle_c /
  ParallelPlaneDistanceDim_c ... that holds the "D1" parameter).
* Comp base of moCompRefPlane_c is 24 bytes longer than OLE2: the unit component (`2b 80 02 00 <10 x 00>`, 14 bytes) is followed by
  **55** bytes (zeros, 16 x FF, 8 zeros) instead of 31, then `u32 plane-feature-id (2 Front, 3 Top, 4 Right), u32 stamp,
  u16 0, u32 k, u8 hasRot, [9 f64 R], 3 f64 t, f64 1.0` as before (plaque_base_roulante Esquisse1: id 2, k 3, hasRot 0).
* **Boss versus cut flag (new, also true for OLE2 samples):** the feature-node flags word: low 20 bits `0x00140` = boss/add,
  `0x201ca` = cut/remove (top bits 0x40000000 / 0xC0000000 are just selection). Held on 100% of slip-pad moICE_c/moCut_c/moExtrusion_c
  nodes (e.g. 'Bear…' moICE_c 0xc00201ca) and on the French-named ARIG ICE nodes ('Enlèv. mat.-Extru.n' 0x?00201ca,
  'Boss.-Extru.n' 0x?0000140). It replaces the English "Cut-" name heuristic (fails in other UI languages). moRevolution_c:
  boss 0x40000000, cut 0x?00201ca; moRevCut_c has 0x40000000 (class decides there).

## L4. First exact rebuilds (plaque_base_roulante, all 5 revisions: volume rel 0.0, box 0.000)
Changes to the OLE2 prototype that were needed (each verified by the rebuild):
1. Stream lookup `Contents/Config-(\d+)-ResolvedFeatures` (see L1).
2. PARAM_TAIL with optional `ff fe ff 00` (L3) -> extrude depth (D1) found; blind (type 0) and mid-plane (6) as before.
3. Unit component tag learned from bytes (`XX 80 02 00 <10 x 00>`, most frequent) instead of Config-0 index; the sketch plane
   (moCompRefPlane_c) is found at unit-tag + 69 (u32 plane id) instead of +45; the FIRST sketch's plane object is serialized
   *before* the moProfileFeature_c class declaration (right after Origin's tail, `moCompRefPlane_c` decl within 400 bytes
   before the node), so extend the search window backwards when the span has none.
4. Boss/cut from the flags word (L3).
5. Folder classes moSelectionSetFolder_c, moNotesAreaFtrFolder_c, moFtrFolder_c, moFeat*BodyFolder_c, moRef*FtrFolder_c,
   moProfileFtrFolder_c carry no geometry (skip, they are not failures).
6. **moMirrorSolid_c** (feature "Symétrie1") = mirror of the whole solid about a FACE: the body holds an edge-record-like face
   list (header `01 00*7 n 00 00 00 00 kind 00 00 id sentinel*2 00 00`, here n=6, kind 2) that holds **n-1 = 5 references**
   (the old regex demanded n). First reference = the mirrored plane: tag/fid/idx = (side class, fid 0x23 = Boss.-Extru.1,
   idx 5 = sketch item key `a` of the line at x=0). Plane = through that sketch line, normal = line direction x extrusion
   normal. Emitted as {type: mirror, plane, merge: true} -> exact (plaque_base_roulante 5/5, incl. Congé1 12 edges and
   Congé2 2 edges resolved by the unchanged edges.py).
7. **Mirror face = first reference of the moSingleFaceRef_w list (the LAST face list in the moMirrorSolid_c span), not of the
   first list.** Each Symétrie node holds two face lists: list A (before `moSingleFaceRef_w`, typically kind 0/2) and list B
   (the moSingleFaceRef_w object). Verified on plaque_superieure Symétrie1: A = [S14, C1, S'(0x2f,3), C0, S17], B = [S17, C0, S8, C1, S14]
   where Sn = side face with sketch item key a=n of Boss.-Extru.1 (fid 0x2c); the truth mirror plane is the sketch line
   a=17 (z=0) = B[0] (box error 0.000 with B[0], 180 mm with A[0]). In plaque_base_roulante A[0]=B[0]=S5 (coincidence).
   Rule: plane through the sketch line of B[0] (or the cap plane if B[0] is a cap class), normal = line dir x extrusion normal.
   Result: plaque_superieure box exact on all 6 revisions; volume residual 8.5e-3 left = chamfer (see below).
8. **Distance-angle chamfer** (plaque_superieure Chanfrein1: params D1=0.03 (length), D2=0.8726646 rad = 50 deg): D2 is an angle when
   0.1 < D2 <= pi/2 (a length param is in metres, so that range cannot be a chamfer distance). Truth corner legs: 30 mm on the
   face a=4 and 30*tan(50 deg)=35.75 mm on the side face (STL vertices (60,-135) and (90,-99.25)). Edge refs of the Chamfer
   node = side/side edges [14,4] and [4,6] (+2 stale refs on construction lines); D1 lies on the face common to both edges
   (guess, one sample). Since Gusset has unequal chamfers (ADR 0042 amendment) the importer writes a real `chamfer` with
   `angle` in degrees (not 45; 45 deg is the equal chamfer) and a `face` pick naming the face D1 runs along: the face shared by
   the most selected side/side edges (ties: lower key), one chamfer per reference face (`Name`, `Name_2`, ...), the pick a
   point 0.3 of the face's line (at most 5 mm) from the first edge's midpoint. Edges that are not a corner of two lines of one
   sketch go in a last chamfer without a pick (default face). plaque_superieure still 5 of 6 revisions exact.
   Bytes: every chamfer in the corpus (13 nodes, 2 parameters D1/D2) has D2 an angle (45 deg x7, 50, 70, 90 deg); there is no
   type flag, equal-distance and 45-degree distance-angle chamfers are identical, and no two-distance sample exists (no
   `distance2` is written; a length D2 <= 0.1 is still ignored).

## L5. Status after round 1 (see py/ for the patched prototype; `python3 -I py/batch.py <name-substr>` rebuilds and compares)
* **Exact (vol rel < 2e-5, box < 0.01 mm): 10 of 11 parts that have a truth** = plaque_base_roulante 5/5, plaque_superieure 5/6.
  The 6th (1618664833) is 1.5e-4 low: Congé1 (r=3, 3 concave pocket corners) is a silent no-op in Gusset when the 3 edges are
  given together (edges 1 and 3 alone each add 2.897 mm3, edge 2 (the corner next to a 3 mm wall) adds nothing) - Gusset side.
* Old OLE2 set unchanged: 17 of 33 (no regressions from the patches).
* Sketch line `kind` u16 (the one after start/end, "1 normal / 0 centerline" in OLE2 notes) is NOT a centerline flag by itself in
  this format: plaque_superieure Esquisse6 has two profile lines with kind 0 and flags 4 (normal) that the truth cuts. Treat as
  centerline/construction only when flags & 0x8008 (bit 0x08 construction, 0x8000 = true centerline: 0x80da on the axis line).
* Sketch normalisation needed for Gusset: two closed loops that share part of an edge (pocket drawn next to a pocket) make the
  Gusset import silently ignore the cut (repro exp/dup.json: volume 60000 instead of 57356; no skip/fail line). Union outline works
  (exp/union.json 57212.25 as expected). py/swfeat.py normalize_lines(): merge coincident points, split lines at points on them,
  drop walls shared by two different segments, drop zero-length lines.

## L6. Byte-diff evidence from the revision pairs (plaque_superieure 1615503535 -> 1615847865: thickness 2 -> 1.5 mm)
* The 77 length/angle parameters of the stream differ in exactly ONE: `D1` at 0x700d, 0.002 -> 0.0015 (that is Boss.-Extru.1's
  EndSpec dimension; truth box y 2.0 -> 1.5 and volume 80769.8 -> 60577.35 agree). So D1 of the dimension object that follows
  the moEndSpec_c is the extrusion depth, in metres.
* plaque_base_roulante 1615039322 -> 1615365433 (90534 -> 96964): same thickness, a pocket removed; tree identical except one
  sketch dimension.
* Feature order/ids: node `u32 feature-id` increases monotonically (0x19 Esquisse1, 0x23 Boss.-Extru.1, 0x24 Esquisse2 ...);
  Mirror/fillet/chamfer face references use these ids (fid) + the sketch-item key `a` (side) or 0/1 (caps).

## L7. Not solved (blocked), with the evidence
* 33 of 49 revision files are `moMirrorStock_c` parts (mirror of an external part; geometry only in Config-N-StockBodies /
  Partition, both `__ZLB`-wrapped Parasolid). No sketch/extrude to decode; the tree is 13 standard folders + 3 planes + Origin +
  1 stock node. Reported as unsupported (1 feature). A rebuild would need the stock Parasolid + a mirror plane, not expressible in gtree.
* Angle_support (46-63 features): decoded = 23-31 sketches, 12-17 extrudes (cut/boss from flags), 5 hole wizards (French param
  names, see L5), fillets partly. NOT decoded: moBlendCut_c ("Enlèvement de matière-Lissage1", loft cut), moMoveFace_c
  ("Déplacer la face2"), moBossThin_c ("Extru.-Mince1"), distance-angle Chamfer on it, fillet edges ("Congé2/3" unresolved),
  boss with end type 2 (not "through all": emitted as +500 mm, wrong), cut end type 5 (= -15 mm taken from first param, unverified).
  Volume 3.4e-3 off at best (rev 1618859437), box not comparable (truth only via mirrored S_ part: sizes).
* End types seen on the new samples: 0 blind, 1/2 through-all cut (one direction for flag 0), 9+flag 1 both directions
  (as OLE2), 5 unknown. For a BOSS end type 2 is NOT through-all (unknown meaning: up-to-next?).
* moBaseBody_c / moRefSurface_c (77 + 22 nodes in the 14 zenodo parts) are imported (STEP) bodies: no parametric tree.

## L8. Round 2 (reasons tally, what was fixed, what was learned)
Corpus (slip-pad 35 + arig-revisions 50): 27 -> 28 as history (slip-pad 17 -> 18, arig 10 unchanged).
Remaining reasons, by parts: 34 `moMirrorStock_c` (33 arig `*_Symétrie_*` revisions + `arm_idler_single_v3_mirrored`; body
is only in StockBodies/Partition, gtree has no "imported body" feature: report); 6 `Angle_support` revisions + `setScrew`
fail in the Parasolid reader (gusset-xt, not this crate); fillet/chamfer edges unresolved 6 (see below); end type
4/5 on extrudes 2 (FrameBracket x2, also cosmetic threads, a fillet and end type 5, so not close); volume mismatch 3
(motorClip 4e-5, baseJoint 1.4e-4, idler_single 12 %); `moBaseBody_c` 2 (STEP bodies); `moHelix_c` 1 (sweep along a helix);
Gusset-side 2 (plaque_superieure 1618664833 fillet; spring_base CSK hole 6.3 mm).
* **Cap faces with two extra words.** A cap reference in an edge record usually has one extra u32 after the 20-byte core; the
  caps of a *cut* (and of a join whose side faces are of the `3Int` kind) have two, `u32 1/0, u32 0`, whereas side references
  have `ffffffff, 0`. Such a reference with index 0/1 is a cap (`Rep.ext2`); the last reference of a face list (n-1 refs) must
  not count (record 0x151fb of Top_beltSupportRim). Fixes Top_beltSupportRim (8 chamfer edges, rebuilds exact) and lets
  Top_mainplate Chamfer2 resolve (the part then stops at Chamfer1, a 3 mm 45-degree chamfer on a 3 mm wall, which OCCT refuses).
* **Cosmetic threads and the configuration table** (`moCosmeticThread_c`, `moConfigTableFolder_c`) add no geometry: skipped.
* **Revolve axis.** With several centerlines the first can cut the profile (MicroGearMotor Sketch3: two vertical centerlines,
  the profile lies along the sketch x axis). The axis must have the whole profile on one side: the first such centerline or
  construction line, else a profile line on the sketch x/y axis. MicroGearMotor's revolve builds now.
* **Extrusion direction (not solved).** MicroGearMotor Boss-Extrude1 (Front plane, 9 mm) only rebuilds when reversed (Sketch2 sits
  on the plate's far face at z = -9, the revolve and the tapped hole agree: volume within 1e-3 after reversing). No reverse flag
  was found: the EndSpec (type 0, flag 0, X 1) is identical to every non-reversed boss; the 260 bytes before D1 and the bytes after
  it contain no field that separates it from 119 other boss/cut nodes except a 3 x f64 unit vector 1 byte after the
  `0005 0002 0002 0002` marker (98 of 138 nodes), whose relation to the sketch normal is not consistent (dot product 0 in 30,
  +1 in 40, -1 in 28 of 98). Parts with a single feature pass either way (volume check), so the corpus has no second witness.
* **Revolve faces in edge records** (GT2_Bobbin Chamfer1): all four references are one revolve (`fid` 59, tags 0x82b3) with idx = the
  sketch key of the profile segment; the edge between keys 10 and 11 is the circle through their shared vertex. `Swept` only
  knows extrudes, so these are unresolved; the importer accepts a `path` of points for a full circle.
* **Fillets on fillet faces** (idler_differential_arm, 14 of 30 edges): references with another feature's id (a Fillet2 face,
  tag 672, fid 68); the other 16 use keys 15-19 / 7-12 that no parsed sketch item carries. Not resolved.
* idler_single_v1: Cut-Extrude1 (blind 10, EndSpec X = 0, the only blind cut with X 0) removes 22 mm3 more in the truth than in
  our rebuild; reversing its direction changes nothing, so the cut is probably dropped silently by the importer. Unresolved.

## L9. Round 3 (stored stock bodies, start offsets, rounded corners)
Corpus (slip-pad 35 + arig-revisions 50): 28 -> 33 as history (slip-pad 18 -> 19, arig-revisions 10 -> 14). A first count of 63
included mirror parts whose history was the stored body alone (20) or the stored finished body (9); those come in as bodies (below).
* **Mirrored stock parts** (`moMirrorStock_c`, 33 arig `*_Symétrie_*` revisions + `arm_idler_single_v3_mirrored`): the reader emits a
  `stored_body` feature (tree name = the node name, `body` 0) and the importer fills `Tree::stored_bodies` from
  `Config-N-StockBodies` (same block framing as the partition: `u32 len, 16-byte tag, u32 inflated, u32 deflated, zlib`; the
  first block is a `TRANSMIT FILE (partition)`; 24 of 49 revision files have the stream). The node carries no mirror plane, so
  `solidworks_history` takes the axis plane that maps the stock's box onto the final body's box (bounding boxes only: any
  plane parallel to XY/XZ/YZ), adds `mirror (merge: true)` plus a half-space cut that removes the stock (a `merge: false` mirror
  would keep the stock as a second body), and the unchanged rebuild check decides. Stock equal to the part (no mirror: Access,
  Igus_Bague) needs none. When the image overlaps the stock (Angle_support) the stored body is the stock mirrored by the kernel.
  Files with no `StockBodies` stream (Servo_HS-485-HB, carter_screen_side rev 1) keep only the finished body. Storing it and
  replaying the later features would pass the rebuild check trivially (a repeated cut or join changes nothing), so these
  parts come in as bodies. Likewise, where no mirror feature is written (the stock already is the part, or the image is
  pre-mirrored in the kernel) and nothing editable follows the stored body, the part comes in as bodies.
* **Start offset `D2`** (idler_single_v1 Cut-Extrude1, the only extrude with a second dimension, EndSpec X 0): the truth's bore
  spans x -1 .. 2.9 from the Right plane (normal +x) with D1 = 10 mm blind and D2 = 1 mm. The cut starts D2 behind its sketch
  plane and runs along the normal; the sketch plane is shifted accordingly. One witness, so the direction of the offset
  relative to the cut direction is a guess that fits the position (the part is symmetric, the volume cannot tell).
* **Fillet faces in edge records** (idler_differential_arm / _short): a fillet's faces are named by the fillet's feature id and
  the 1-based number of the edge in its selection (the selection order); a later record's `[face A, face B, vertex face C, D]`
  may name them. A fillet of two straight profile sides is kept as a rounded corner (tangent points, arc middle); an edge
  between a cap and that face is the arc (middle = corner + (r / sin(h) - r) along the bisector, h = half the corner angle),
  and a side that meets the corner is shortened to the tangent point at that end, whatever neighbours the record lists (the
  record of side 3 still names side 13 as its neighbour after the fillet). Records whose faces are keys missing from the parsed
  sketch (keys 15-19) are the stale selections. With this all 30 edges resolve; the parts then give 27 mm3 too much
  (336.10 against 309.08): the parsed sketch is short of segments (keys 15-19 are real faces, three of their cap edges are
  filleted), not solved.
* **motorClip 4e-5**: the truth's plane face at y = 10.1 is 25.822 mm2, the exact value (cone against plane) is 25.831; the
  exact body's cone/plane intersection is a fitted curve, so the shortfall is in the exact-body reading (gusset-xt), not in the
  features. **baseJoint 1.4e-4**: four planar faces differ by 0.002 .. 0.02 mm2, not located.
* Not attempted for lack of time: GT2_Bobbin (revolve faces), arm_idler_single_v4 (chamfer face; also two extrudes without end
  spec), MicroGearMotor reverse flag, FrameBracket end types 4/5, spring_base (end type 5 and the countersink).

## L10. Round 4 (extrusion direction, start offsets)
Corpus (slip-pad 35 + arig-revisions 50): 33 -> 34 as history (slip-pad 19 -> 20).
* **Extrusion direction (solved, MicroGearMotor).** The unit vector one byte after the `0005 0002 0002 0002` marker of an extrude
  node IS the extrusion direction, in a frame where a z direction is written as the y component negated (+z -> (0,-1,0),
  -z -> (0,1,0)) and x/y directions as they are. The earlier "dot product 0/+1/-1" scatter was this z twist. Checked on 80
  extrudes of the slip-pad parts with a vector (blind and cut, axis-aligned sketches): all agree with `sign(distance) * sketch
  normal` except MicroGearMotor Boss-Extrude1 (the known reversed one) and four nodes that carry no end spec or end type 5
  (FrameBracket Cut-Extrude3 / Boss-Extrude10, Top_mainplate Cut-Extrude3, arm_idler_single_v4 Boss-Extrude5), whose sign
  our reading of D1 gets wrong; the reader now negates a distance that runs against the written direction (only for
  axis-aligned sketches, not for symmetric or through-all). Tilted sketch planes (arm_idler_single_v4 Boss-Extrude4,
  Cut-Extrude7) write a vector that is neither the normal nor an axis: not used.
* **Start offset (D19).** The dimensions of a feature are numbered through its sketch: a cut with a 17-dimension sketch
  writes its depth as `D1` and its start offset as `D19`; with no sketch dimensions it is `D2` (idler_single_v1). MicroGearMotor
  Cut-Extrude2 (D1 4.25, D19 0.8) and Cut-Extrude3 (D1 2.5, D19 0.8) void z -5.05 .. -0.8 and -8.35 .. -5.85 from sketch planes
  at z 0 and -5.05: the cut starts D19 *ahead* of the plane along its direction (end spec X 1). idler_single_v1 has X 0 and
  starts behind. Both fit "X 1: ahead, X 0: behind". The truth was compared with the rebuild by ray casting the two STLs
  along z (layer volumes), which located the 0.8 mm shifts although the total volume was within 1e-3.
  loadcell_centerPlatform_v1 Cut-Extrude3 has a D19 of 4 mm too and still rebuilds (its cut lies in empty space either way).
* **Revolve circles (GT2_Bobbin_v2, reader done, part still blocked).** In an edge record of a full revolve the faces are
  the profile segments by sketch key (`a`, as for extrudes); the edge between two segments that share a vertex is the circle
  through it about the axis line. The reader emits it as `{midpoint (opposite the profile plane), path (200-point closed
  circle)}`. Of the five selected edges, three name construction segments (stale selections); the other two (keys 3/8 and
  10/11, the flange rims at x 7 and x 11, r 8) are the whole chamfer: the truth minus the no-chamfer rebuild is 48.05 mm3 =
  2 x 24.1 (a 1 x 1 mm triangle ring at r 8 - 1/3), and its z-ray layers sit exactly at x 6..7 and 11..12. OCCT refuses the
  chamfer because the 1 mm distance equals the 1 mm wall it runs along (0.9 works; 0.999999 works but gives 1723.829 against
  1723.947 mm3, 7e-5 off, so the truth is not exactly two 1 x 1 chamfers either). Top_mainplate Chamfer1 is the same case.
