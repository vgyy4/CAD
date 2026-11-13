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
* **End types are the SolidWorks API's `swEndConditions_e`** (consistent with every value seen: 0 blind, 1 through all, 2 up to
  next, 3 up to vertex, 4 up to surface, 5 offset from surface, 6 mid plane, 9 through all both). So a BOSS of type 2 is "up to
  next" (L7), not through-all.
* **Up to surface / offset from surface (types 4 and 5).** The face is the first reference of the node's last
  `moSingleFaceRef_w` object, a normal edge-record reference (`<class> 00 00 | <class> | <u32 feature id> | <u32 stamp> | <u32
  face>`): the face of an earlier extrude, a cap (face 0 = at the sketch plane, 1 = the far end) or a side (the sketch key of its
  line). The stamp is the creation time of that feature (a `u32` of seconds), the id its feature id (so `spring_base_v1` 32 =
  Boss-Extrude1). The object is found by the pair of 8-byte words `7d c3 94 25 ad 49 b2 54` twice in its header, the last such
  pair of the node; the `moEndFace3IntSurfIdRep_c` before it names the extrude's *own* end face (feature id = the node's id, face
  1) and the faces it was made from, so a later feature can refer to it; it is not the up-to face.
  Type 5 ends at that face's plane moved `D1` *into* the material of its feature (against its outward normal): spring_base_v1
  Boss-Extrude2 (face 0 of Boss-Extrude1 at y = 0, `D1` 10, sketch at y = 3) ends at y = 10, a distance of 7 *along the sketch
  normal*; FrameBracket Cut-Extrude3 (face 1 of Boss-Extrude3 at y = -3, `D1` 1, sketch at y = 0) stops 1 mm above the far
  face (the other sign would cut the whole boss: volume 426 mm3 off). FrameBracket's four type-4 features (Boss-Extrude4/5/6,
  Cut-Extrude6) end at the planes of the next sketches (y 7, x 42.5, z 20, x 40.4).
  The reader emits `end_condition: "to_face"` with the face's point and outward normal when the face (the cap or side wall of
  the earlier extrude, as that feature drew it) is ahead of the sweep and covers the whole profile swept onto it; when it
  does not (SolidWorks extends a planar surface, the original feature's face may be smaller than the profile, as the
  half-width wall at y = 7 of FrameBracket's main flange is), or the face is behind the sketch (spring_base Boss-Extrude2),
  it writes the plane's distance as a blind extrusion. Only FrameBracket Cut-Extrude3 comes out as `to_face` in the corpus.
  Neither FrameBracket nor spring_base becomes a history for other reasons: FrameBracket's Fillet1 edges join faces of
  features with different normals (0 of 2 resolve) and its hole wizards fail; spring_base's four countersinks fail the
  importer's fit check for a reason that is not equality: the pocket floor (r 3.15, hole at x = -22) is clipped by the plate's
  edge at x = -25 (3.0 mm from the hole), so the countersink mouth (r 3.15) overhangs the plate by 0.15 mm. The check is right
  to refuse it; it was left alone.
* **setScrew**: the revolve sketch and the others parse with no curves at all (a sketch the parser does not read), and the
  part also has a helix and a swept thread cut; not attempted further.
* **Angle_support (6 revisions)**: still `moBlendCut_c` (loft cut), two `moBossThin_c`, `moMoveFace_c`, three end type 4/5
  cuts, a chamfer with unresolved edges (4), fillets of 18 and 89 edges that do not resolve, and boss end type 2 (up to
  next) emitted as +500 mm. Seven independent gaps, each larger than one afternoon; not started.
* **arm_idler_single_v4**: the extrudes without an end spec (Boss-Extrude4/5) sit on tilted sketch planes whose written
  direction vector (1,0,0) is not the plane normal (0.81, 0, -0.59), so the vector rule says nothing there; Fillet1 resolves
  3 of 4 edges and Chamfer1 needs a face pick. Not solved.
* **idler_differential_arm(_short)**: the volume excess is localised: ray casting the truth against the rebuild shows the
  upper half of both end lugs (centres (+-16.5, 6), r 1.5, above y = 5.9) is missing in the truth for all z, leaving a stub
  at x 16.1 .. 16.9, y 6.1 .. 6.4. The truth's bounding box still reaches y = 7.4995 there. The profile keys 15-19 that no
  parsed segment carries are probably what shapes the lug; not decoded. Fillet1 is stored before Fillet2 in the stream
  (ids 66 and 68) and refers to Fillet2's faces, so stream order is the rebuild order.
* **Method note**: comparing the truth (the part's own Parasolid body, exported by `gusset build part.SLDPRT --export x.stl`)
  with the rebuilt STL by ray casting along an axis and printing layer volumes (and 2D masks) finds an error that the
  total volume hides (MicroGearMotor: 1e-3 of the volume, three layers 0.8 mm off). The scratch scripts are not kept.

## L11. Round 5 (edges between merged coplanar faces; what blocks FrameBracket, idler_differential_arm, the chamfers)
Corpus (slip-pad 35 + arig-revisions 50): 34 -> 34 as history. One reader fix; the rest are findings.
* **Edges between planar faces of different features (solved, FrameBracket Fillet1).** A later feature's face that is coplanar
  with an earlier one merges with it, and the edge record keeps naming the *earlier* feature's face. FrameBracket Fillet1's two
  edges are `[Boss-Extrude3 side 16, Boss-Extrude3 side 14, Boss-Extrude5 side 10, Boss-Extrude5 side 8]`: sides 16 (z = 20) and
  14 (x = 42.5) of the plate are not neighbours in its profile (an R10 corner arc lies between them) and meet only on the
  box of Boss-Extrude5, whose top and end faces are coplanar with them. The edge is the line where the two face planes meet,
  between the planes of the two vertex faces (references 3 and 4): (42.5, 23, 20) and (-42.5, 8, 20). Ray casting the truth
  against the rebuild without the fillet found it first (the truth is 0.8 mm lower at x = 40.25, z = 19.17: a convex r5 round).
  `plane_edge` computes it when the sketch-based reading finds nothing (sides and caps of extrudes, straight lines only).
  Both edges resolve and the fillet builds; no other part changes (34 stay, same reasons).
* **FrameBracket still stops, now for three independent reasons.** (1) Cut-Extrude3 (end type 5 "offset from surface") as read
  removes y -2..0 over the plate (|x| < 22.5, z 5..20), 850 mm3; the truth has that plate intact (z-ray at (0, -1) gives
  z 10..20 in the truth, nothing in the rebuild) and the rebuild WITHOUT the cut matches the truth to the holes (26262 against
  26239 mm3 with the CSK and the first tapped hole missing). A later cut cannot restore it, so the cut is a no-op in the
  truth or reads a different plane/direction; the node's flags word (0xc00201ca) is that of every cut and no suppression
  flag was found. The "volume 426 mm3 off for the other sign" of L10 was measured on a different partial history. Not solved.
  (2) The CSK wizard builds a pocket on the plate's far face at (0, 3), where the truth has no plate (the plate is hollow
  z < 10 for |x| < 22.5, made by a later through cut), and the first tapped hole puts 3 positions on faces that do not
  hold them. (3) The truth's holes at x = 35.5 are 3.46 mm wide against 3.0 mm in the rebuild (Cut-Extrude6..9 or
  Mount Holes): a sketch dimension that is not read. FrameBracket_v2_loadCell_v1 adds Cut-Extrude17 with no end spec.
* **Merged coplanar faces for up-to-surface ends (item not done).** Would only change which of the five FrameBracket
  extrudes are `to_face` instead of blind; FrameBracket does not come in, so nothing is gained.
* **idler_differential_arm / _short: the truth is incomplete, not the profile.** The Parasolid reader warns `torus N is
  self-intersecting (apple or lemon)` / `face N was left out` for the lug fillets (Fillet1/2, r 1 on the r 1.5 lugs: major
  radius 0.5 < minor radius 1, a spindle torus). The truth body is missing those faces (its mesh volume 194.0 against the
  window's 204.9; the bounding box still reaches y = 7.4995 because it comes from the Parasolid box), which is why the
  "upper half of both lugs" is missing in the truth. Keys 15-19 are stale selections: the parsed sketch has all 13 segments
  the file writes (keys 0, 1, 3-6, 13, 14, 20-24); no entity carries 15-19 or 2, 7-12. Nothing to decode in the profile; the
  fix belongs to the exact-body conversion (gusset-xt: spindle tori), outside this crate. Fillet1 refers to Fillet2's faces
  (tag 672, fid 68) and Fillet2 comes first in the stream, as before.
* **arm_idler_single_v4.** Fillet1's fourth edge is `[Boss-Extrude1 side 5, face 1 of feature 100 (Chamfer1), Cut-Extrude1
  cap 1, Boss-Extrude1 cap 1]`: the edge between side 5 and the chamfer's bevel face. It needs the bevel as a face with
  a tangent line on side 5 (for the distance-angle Chamfer1, 3 mm and 70 degrees, the other offset is 3 tan 70 = 8.24 mm on
  the face the chamfer does not run along; which face is the reference is the one-sample guess of `chamfer_angle`).
  Boss-Extrude4/5 (no end spec, tilted planes) follow. Three coupled guesses; not started.
* **GT2_Bobbin_v2 / Top_mainplate (item 5).** The chamfer is a 1 x 1 mm bevel along a 1 mm wide flange face. An exact
  construction exists (cut the revolved triangle (x 7, r 8), (x 6, r 8), (x 7, r 7); the cutter's face is coplanar with
  the flange face and its far vertex lies on the bore edge), but it would give 2 x 24.086 = 48.17 mm3 where the truth
  removes 48.05 (centroid radius 0.25 % lower), 7e-5 of the volume against the check's 2e-5. So the truth is not a plain
  1 x 1 chamfer and a correct construction would still fail the check; no kernel change made.

## L12. Round 6 (sw1: the sw-more corpus; layouts of more versions, imported bodies)
Corpora: slip-pad 35, arig-revisions 50, zenodo 14 SolidWorks parts, and **sw-more** (1253 parts from 30 public sources,
326 OLE2 and the rest chunk container; whole corpus measured, not a sample). Baseline: slip-pad 23, arig 15, zenodo 0,
sw-more 47 as history. Measured first with a reader-only tally (the feature tree and its own check, no rebuild) over
sw-more: 162 of 1253 trees read whole; the rest stopped at layouts the reader did not know. Every fix below was decoded
from those bytes and checked by the unchanged rebuild check.
* **Version 3 entity headers without the gap.** The payload of a sketch entity whose header has version byte 3 starts
  8 zero bytes after the `0.0 1.0` pair in the ARIG parts, but right after it in adpator (zenodo) and in many sw-more parts
  (`1e 00 <x> <y>` for a point directly after `f0 3f`). Those sketches parsed as empty (adpator: four sketches, no curve;
  the rebuild then said "no closed regions" or "no axis line"). A payload never starts with 8 zero bytes (a point starts
  with its type 0x1e/0x1f, a segment with two different point indices), so the bytes decide.
* **Schema word 0x07.** Older modellers (setScrew, Spring_0_5len) write the entity header as `FF FF 07 00 <1|3> FF x 8 00 00
  80 BF` instead of `FF FF 1F 00 ...`; newer parts keep `07` for library sketches (hole-wizard sections, the origin
  sketch). Both words are read. setScrew's sketches now parse (Sketch22: 10 points); the part still stops at its helix.
* **Component base further on.** The plane-feature id of a reference-plane component (`u32 id, u32 stamp, 00 00, u32 k,
  hasRot, [R], t, 1.0`) sits 55 bytes after the unit-component tag in the ARIG parts, 67 in the DroneDock parts (12 more
  zero bytes after the run of 16 FF bytes) and 69 in the Ahm3d0x parts (a word `50 46 00 00` after those zeros). The id is
  looked for at every offset from 43 to 112, the first that holds a whole frame (orthonormal rotation, scale 1.0). Plane
  failures in the reader tally: 238 -> 45 (43 with the next item).
* **A first sketch on a fixed reference plane** (zsCAD `4ad`): the node is preceded by `moFixedRefPlnData_c`, an ordinary
  plane-data block (origin, normal, hasRot, t, 1.0), not a `moCompRefPlane_c`; it is looked for in the same 400 bytes.
* **Geometry-free nodes** seen in sw-more: `moInkMarkupFolder_c` (Markups, in 215 parts), `moConfigCommentsFolder_c`,
  `moAttribute_c` (FaceIDTag/Assoc), `moAlignGroup_c`, `moSensor_c`, `moCenterOfMass_c`, `moCutListFolder_c`,
  `moDetailFolder_c`, `moMeshBodyFeatureFolder_c`, `moFeatGraphicsBodyFolder_c`, and reference points, axes and coordinate
  systems (`moRefPoint_c`, `moRefAxis_c`, `moCoordSys_c`) are skipped like the reference planes; a feature that relies on
  one fails the rebuild check instead.
* **End condition byte 12.** An end spec may carry `01` at byte 12 of its 18-byte head (`<tag> 00 00 01 00 00 00 00 00 00
  00 01 00 00 00 00 00`, Lock Collar Cut-Extrude2, V-Slot Rail Boss-Extrude3); it is accepted and not read (the written
  direction vector says the direction). Unknown end conditions in the reader tally: 37 -> 21.
* **Imported bodies (`moBaseBody_c`).** The body is kept as imported in `LocalBodies` (OLE2) or
  `Config-<n>-FeatureBodies/LocalBodies` (chunk container): `u32 count`, then per body a record `FF FE FF <n> <name UTF-16>`,
  a flag word, `00`, `u32 A`, `00` (newer parts: two more words before the flag) and a block `u32 B, 16-byte tag, u32
  inflated, u32 deflated, zlib`. The record name is the node's name (`Imported1`, `0780-001-1-solid1`; surfaces carry
  `_0`). A body over 1 MiB goes on in further chunks of the same block (`u32 inflated, u32 deflated, zlib`, ended by two
  zero words; GT2_Spur's gear: 1 MiB + 582 kB), which the body import had cut after the first chunk. The reader emits a
  `stored_body`, the importer hands over the named body; the history is refused when nothing editable follows ("the
  history is the imported bodies only": 533 sw-more parts are STEP imports with no feature) or when the imported bodies
  already are the finished part. GT2_Spur and GT2_Bobbin_Idler get past the import and stop at a boss of end type 1
  ("through all" for a boss: up to the far side of the imported gear, whose faces the reader does not model).
* **A 90-degree distance-angle chamfer** (adpator 倒角1: D1 0.3 mm, angle exactly pi/2). Ray casting the part's body against
  the rebuild showed the top face ending at r 22.7 (0.3 mm short of the rim) and the bevel a cylinder: square on the face D1
  runs along. A 0.3 mm equal chamfer removes 6.0 mm3, the truth 16.4 (the rebuild was 10.1 mm3 heavy). On the circle edge of a
  full revolve between two profile lines this is a two-distance chamfer: D1 on the reference face and D1 / cos(corner angle)
  on the other (here 0.826 mm, corner 68.7 degrees); the reference face is the plane or cylinder of the two, the other a
  cone (one sample). adpator comes in as history.

**Findings, not fixed.**
* `RJ45-LED.sldprt` (AeroSeif raspberry-pi) kills the process: SIGSEGV in OCCT's `math_KronrodSingleIntegration::Perform`
  under `BRepGProp_VinertGK` (mass properties of the imported body, `gs_mass`), called in-process from
  `commit_prepared -> body_metrics`. A 256 MiB stack does not help. The corpus driver skips it; the import of an untrusted
  file should not be able to do that (OCCT or kernel-worker issue, outside this crate).
* **Mirror of cut features** (Mini Ball Bearing, the 608ZZ/688Z/625 bearings, about 1.04-1.06 too heavy): the mirror is
  read as "the whole body mirrored and united", which fills the grooves the mirrored cuts should make. The mirror's list of
  features was not decoded; the node holds face references `(<class> 00 00 89 80 <u32 fid> <u32 stamp> <u32 face>)` to
  Boss-Extrude1 (30), Cut-Extrude2 (55) and Cut-Extrude1 (41) and a `u32 3` list of three object indices (0x67, 0x66, 0x65).
* **Nodes inside a feature's span** (V-Slot Rail_PIVOT): `Sketch8<3>` is written inside Boss-Extrude3's data, before its
  end spec, so the end spec falls into the sketch's span and the extrude reads none.
* idler_differential_baseJoint_v1 (1.4e-4): ray casting finds no difference above the tessellation (all differing cells
  are chords near the r 2.275 cylinder's silhouette); not located.

**Coverage after the round** (rebuild check unchanged): slip-pad 23 of 35, arig-revisions 15 of 50 (both unchanged, no
losses), zenodo 1 of 14 (adpator; was 0), sw-more **156 of 1253** (was 47; no part lost). sw-more parts whose bodies could not
be read at all: 26 -> 14 (the LocalBodies chunks). Remaining sw-more reasons, by parts: imported bodies only 540, volume
mismatch 77, sketch plane not found 43, fillet edges not resolved 40, `moShell_c` 22, extrude without end condition 21, no
feature stream 21, edge midpoint not on the model 20, circular/linear patterns 18 + 15, split body 16, loft 16, closed
regions 15, up-to-surface ends 13, reference surfaces 12, mirror plane not resolved 11; 2 parts kill the test process
(RJ45-LED above; Link_2 below).
* `Link_2.SLDPRT` (KUKA iiwa) now reaches its hole wizard on the imported body, and the hole's fit check tessellates the
  290 mm body with an absolute 0.01 mm deflection (`gusset-eval` `hole.rs`): `BRepMesh_Delaun` grows past 8 GB in a minute
  (in-process). Before this round the part stopped at its Markups folder. The finished body alone meshes fine for an STL
  export; the fit check's deflection is the problem (gusset-eval, not changed here).

## L13. Round 7 (sw2: volume mismatches, sketch planes, end conditions)
Measured against round sw1's final logs (slip-pad 23, arig-revisions 15, zenodo 1, sw-more 156). Each volume mismatch was
classified with a scratch harness that rebuilds the history one feature at a time and measures, after each feature,
the rebuilt solid outside the part's own Parasolid body and the body missing from it (two booleans per feature); the
feature where either jumps is the one read wrong. Reader-only tallies (the feature tree and its own check, no rebuild)
over all 1352 parts: 937 trees read whole before the round, 968 after. Decoded from the sample bytes:
* **Unit-component tags above 255.** Files with more than 255 classes before `moUnitComponent_c` write its tag with
  the high byte too (`24 81 02 00`, class 0x124: Spacer_long_base_square, Case_Electronics, the SG90 servos, the washers).
  The tag learner only took `XX 80`. Accepting any high byte let stray `XX YY 02 00` runs of zeros outvote the real tag
  (V-Slot Spacer Block: three `ad 94` against two `73 80`), so only a run followed by a whole reference-plane frame
  (the component base the plane reader already checks, 43..112 bytes on) votes. Sketch-plane failures in the reader
  tally: 43 -> 6 (with the nested names below).
* **End condition byte 12 is "reverse direction".** The `u32` at byte 12 of the end-condition head turns a boss against
  its sketch normal and a cut along it. On every extrude that also writes a direction vector (the `0005 0002 0002 0002`
  marker, L10) the two agree (history parts: 9 bosses with the byte set run against the normal, 1 cut along it; none
  disagrees); parts that write no vector need the byte: 200mm Topmost Platform's eight cuts ran away from the plate
  (each left the volume unchanged), cap, back_shoulder_attach, the Cablechain brackets.
* **Elliptical sketch arcs.** A sketch segment that writes four `u16` point indices where an arc writes four `-2` words
  (`p + 22`) is an ellipse (arc): the points are the axis ends (major +, minor +, major -, minor -), `dir` 1 runs
  counter-clockwise from start to end as for arcs, and the centre is halfway between the major ends (Y-Plate's notch
  and the butterfly valve's outline are circle-shaped ones). They had been read as straight lines from start to end. The
  tree writes `ellipse_arc` (ADR 0042) about a centre point of its own (`pe<k>`).
* **Revolve axes by reference.** A revolve about a reference axis (`moRefAxis_c`) or an edge carries a line reference
  (`moLineRef_w`): `c7 cf ff ff c7 cf ff ff 00 00 00 00 70 17 00 00`, 8 zero bytes (16 in newer parts), then `f64
  length`, the start point and the unit direction in model coordinates (m). The V-wheels revolve about Axis1 (the z
  axis); the reader had taken a construction line at the rim. The axis is the sketch line on that line, else the line
  as origin and direction; full-revolve circle edges now keep the axis as two sketch points.
* **A projected centerline is not an axis.** A sketch may carry a model edge projected into it with the centerline flags
  (M5_bolt's and M6_bolt's Sketch3: the bolt's bottom silhouette, flags `0x80d1` and a reference object in the entity's
  reference slot); taken as the first centerline, the cut-revolve turned about it and cut nothing. A revolve's first
  centerline is now the first one without a reference object.
* **Up to a vertex (end type 3).** The node's point reference (`moEndPointRef_w`) writes the vertex in model coordinates
  after `c7 cf ff ff c7 cf ff ff 00 00 00 00 f8 2a 00 00` and 8 zero bytes; the extrude ends on the plane through it
  parallel to the sketch (V-Slot Rail_PIVOT Boss-Extrude2, Raspberry Pi Case_BOTTOM). A vertex reached through a
  reference chain (`moReferenceChain_c`, GPIO_cover) is not read.
* **Names nested in a node's header.** A feature that reuses a sketch writes `Sketch3<3>` inside its own header, and
  zsCAD sketches an attribute name (`Assoc1`); both find the same 46-byte block as the node around them, so they cut its
  span short: the extrude lost its end condition (V-Slot Rail_PIVOT, Connecting Plate_BOTTOM_M3, PiCam Case_TOP, the
  Turkish crankshaft parts), the sketch its entities and plane (all twelve zsCAD parts now read). Extrudes without an
  end condition in the reader tally: 21 -> 7. Nested reused-sketch
  and attribute names are dropped; a hole wizard's nested sketches stay (dropping every nested name broke them).
* **Sketches kept in the configuration stream.** Sketches driven by equations or configurations keep their entities
  in `Config-<n>` (the stream read for the class index), and the sketch node in the feature stream holds none
  (NEMA23_mount, Support-Rod, tramPlate, Pan-Slot Screw M4, the bolts). `Config-<n>` holds one `sgSketch` object per such
  sketch: the first after the class declaration, the others under its class tag (`tag, u32 point count, u16, u32 key,
  u32 id`, then the first entity header); the empty sketch nodes take them in order when there are as many of each.
  Their dimensions (fillet radii, extrude depths) can live there too and are not read (NEMA23_mount's Fillet1:
  "no radius found").
* **Degenerate arcs and circles.** A construction arc whose ends are not both on its circle (the centre is one of its
  ends: Hobby Motor 3v) and a circle through its own centre (mid_elbow's Sketch9, about a projected point) are not
  entities as read; Gusset refused the sketch, or (idler_differential_baseJoint_v1, whose residual 1.4e-4 L11 could not
  locate) its solver moved the shared profile points to put the arc's ends on one circle. Both are left out.
* **Fillet records that start with their own face.** `[own face, vertex face, face, vertex face, face]` (Power Supply
  Unit, Ultrasonic Sensor, robotCasterWheel, sensor pannel) is read as `[face, face, vertex face, vertex face]`.
* **Imported bodies of one name.** Imported features that share a name keep their bodies as `name`, `name_1`, ...
  (Hobby Motor 3v's seven); each took the first. Now each body is taken once, the exact name first.
* Fillets and chamfers with unresolved edges list them in `sw_unresolved` (feature name / face number, `c` for caps,
  `stale:` for references to known features): the remaining fillet failures are mostly faces of earlier fillets that
  are not a round of two straight sides (14 parts), sketches whose dimensions live in the configuration stream, mirror
  instance faces (`Mirror2/33553`: a reference that nests the seed face) and pattern instances (`?`).

**Findings, not fixed.**
* **Mirrors of features** (the four bearings, Universal L Bracket): the truth has the grooves on both sides, i.e. the
  cuts are mirrored, not the body. The mirror node lists face references to one face of each of Boss-Extrude1,
  Cut-Extrude2 and Cut-Extrude1 (an edge-record face list) and a `u32 n` list of object indices that every mirror has,
  body mirrors included (ARIG's `Symétrie1`: `03 67 66 65`); neither is a feature list as read, and mirroring all three
  features (the boss's copy refills the grooves) does not give the truth either. The bearings' Chamfer1 also names its
  edges on mirror instances: `<tag> 00 00 89 80 <fid of Mirror1> <stamp>` followed by the seed face's own reference
  (Cut-Extrude2 face 3), which the edge reader takes as one reference with face number 0x808c.
* **Shells** (22 parts): 13 are one family (`r3*`, Rozakiin) whose shell is followed by a variable fillet; decoding the
  shell alone gains nothing there.
* **Patterns**: 14 trees would read whole with patterns decoded. The pattern node holds an edge record for the axis
  (Boss-Extrude1 face 2 / Cut-Extrude1 face 1 in pulley_small's CirPattern1) and the same `01 00 xx 00` object
  references as the mirrors; the patterned features were not identified.
* **Mirror planes on reference planes** (13 parts, mostly the Turkish engine set): the mirror names a user plane
  (`Plane1`, `Düzlem1`), whose geometry the reader does not keep.

**Coverage after the round** (rebuild check unchanged; whole corpora, the history test through the corpus driver):
slip-pad 24 of 35 (was 23: idler_differential_baseJoint_v1, the construction arc above), arig-revisions 15 of 50 and
zenodo 1 of 14 (unchanged), sw-more **195 of 1253** (was 156; 39 gained, none lost): cap, back_shoulder_attach, wheel
dirctor, TX2_Case_Top_Plate, 126p-engine-susp-bushing, 200mm Ring Bearing, 200mm Topmost Platform, Bottom Motor Spacer,
IMU Mount, IMU, Quick Release Rod, Ultrasonic mount, Parça1, the two plain washers, the two Threadedrod mounts, Y-Brace,
Y-Plate, V-Slot 40x40, the four V-wheels, three Cablechain brackets, TempBoxBracket, both Support-Rods, valve, V-Slot
Connector_3mm_LIMITSWITCH, 5bd, 5dd, M5_bolt, M6_bolt, tramPlate, waste_board, O2C Bottom Layer (the two bolts come from the
projected-centerline fix, measured on the three parts whose trees it changes after the whole-corpus run). All corpora:
195 -> 235 of 1352. Remaining sw-more reasons, by parts: imported bodies only 540, volume mismatch 83 (parts that now read whole land
here first), fillet edges 39, `moShell_c` 22, no feature stream 21, circular/linear patterns 19 + 18, edge midpoint not on
the model 18, up-to-surface ends 16, split body 16, loft 16, mirror plane not resolved 13, reference surfaces 12, closed
regions 12, sketch planes 6. RJ45-LED and Link_2 are skipped by the corpus driver as in round sw1 (in-process crashes).

## L14. Round 8 (sw3: patterns, mirrors of features, edges on a mirror's copy)
Measured against round sw2's final logs (slip-pad 24, arig-revisions 15, zenodo 1, sw-more 195). The 83 sw-more
volume mismatches were run through the per-feature harness again (scratch; what lies outside the part's own body and what
is missing from it after each feature). Where the first divergence shows: at the first boss 26 parts (the profile or its
plane already differs), at a cut 12, at stored or imported bodies 10 (zsCAD's `Body___<n>` parts, Hobby Motor), and 28
where the harness's own booleans against the stored body fail (no verdict); no single cause covers more than a handful.
The one that did repeat was the mirror: every `moMirrorPattern_c` had been rebuilt as a mirror of the whole body. Decoded
from the sample bytes:
* **The feature stream is one object archive, and patterns name their seeds by archive index.** References to objects
  written earlier are their index in the archive (`u16`, or `ff 7f` and a `u32`), so a pattern's seed features are a list
  of indices: after the pattern's header, `T n <n indices> T 00 00 01 00 00 00 00 00 01 00 00 00 00 T m`, where `T` is
  the list's class tag (`08 80`, `58 80`, `a3 81`: it differs per file) and `m` counts the bodies a pattern of bodies
  repeats (the Ahm3d0x gears and wheel: no seed, one body; not read). Spacer_long_base_square's LPattern1:
  `08 80 02 00 84 04 33 06 08 80 00 00 ...` = Boss-Extrude3 and Cut-Extrude1.
* **The index of a feature node** is not written with it; three back references give it: the feature's own dimension
  objects (`P 01 00 01 00 00 00 P` inside its span: Cut-Extrude1 = 0x633 above), the owner word of face references to
  the feature's own faces (`<class tag> P <class tag> u16 <u32 feature id> <stamp>`, zero in references made by other
  features: Fillet2's faces carry 0x137c, Case_Electronics), and, for a node that declares its class, the class index
  plus one (the index the class's later nodes write as their tag: SG90's `Furo4` declares `moHoleWzd_c`, `Furo1` is
  `tag 709`, the linear pattern names 710). An index two nodes claim is dropped. In the 37 parts that stopped at a pattern, every seed list
  resolves but those of Center_board_bottom and _top (a single tapped hole nothing points back to) and two SG90 patterns.
* **Pattern parameters.** Linear: `D1`, `D2` instance counts (`moIntegerParameter_c`: a `u32` where the dimension
  writes its `f64`), `D3`, `D4` spacings (m) along the first and second line reference. Circular: `D1` count, `D2` the
  step angle (when written) and `D3` the total (2 pi for equal spacing), `D4` the count of a second direction (Case_Electronics: 2,
  not read). Lines: after `c7 cf ff ff 00 00 00 00` (the marker word once to three times, then a `u32` that varies:
  0x1770, 0x2710, 0x32c8, 0x36b0 ...) and 4 to 20 zero bytes, `f64 length`, the start point and the unit direction in
  model coordinates (m), as the revolve axes of L13. A two-way linear pattern becomes two Gusset patterns, the second
  repeating the seeds and the first's copies.
* **Mirrors of features.** All 54 `moMirrorPattern_c` nodes in the corpora carry the same seed list (all with seeds);
  the 39 `moMirrorSolid_c` none. The tree now mirrors the listed features (pulley_small's Mirror1 mirrors Cut-Extrude1;
  the bearings' Mirror1 their Cut-Extrude2, the recess, which the body mirror had filled: 1.04 too heavy -> 1.002).
* **Edges on a mirror's copy.** A face of the copy is written as a 16-byte reference to the mirror (`<tag> 00 00 <class>
  u16 <u32 mirror id> <stamp>`, no face number) followed by the seed face's own reference with its extra words (the
  bearings' Chamfer1: Mirror1, then Cut-Extrude2 face 3). The edge is solved reflected back: the copy's faces are the seed
  faces, every other face that of its extrusion reflected in the mirror plane (origin and axes reflected, depth negated),
  and the midpoint found is reflected again. Stepper-Bracket's Fillet2 resolves.
* **Selections of long records.** Edge records of four references with their extra words run past 0xa0 bytes (the
  bearings' eight chamfer edges are 132 to 204 bytes apart); a record also continues the selection when it starts right
  after the previous one ends (its class tag and up to 14 bytes between).

**Findings, not fixed.**
* **Pattern instance faces** (fillets on a pattern's copies: Spacer_*_base_square, Front_panel, NEMA23_stepper, the
  42-tooth pulley, 4in Hub; 18 parts list such references, 23 more list mirror-copy faces): `<16-byte reference to the pattern> <seed face reference>`,
  then the instance number (`u32`, 1 = the first copy) and sometimes a second, context reference; the layout of that tail
  varies and was not settled.
* **Mirror planes on user planes.** The mirror keeps a copy of a plane's display block (`c7 cf ff ff` x3, the four
  display extents), but the extents of the copy match the plane's only sometimes (piston's Düzlem1: then the plane-data
  block of the plane node, `moFaceRefPlnData_c` origin and normal, gives the plane; Parça1's Aynalama7: they differ), and
  the plane may be written after the mirror. Not committed: the one part it resolves stays a volume mismatch.
* **The bearings** now differ by 0.2 to 0.5 % the other way: their Chamfer1 lists eight edges (a `u16 8` before the first
  record), the bore and outer rims and the recess rims on both sides; the Parasolid body's chamfer volume matches
  bore x2 + outer + one recess rim, not all eight. Not resolved.
* **Fillets that add material.** The stepper motor shafts' Fillet1 (R 1 on the end circle a D-flat cut has broken) builds
  in Gusset and adds 6.1 mm3 instead of removing about 2.9 (OCCT/Gusset fillet, not the reader).
* **Hecatron sketches with unsolved circles.** 5 Hole T Joining Plate's Sketch2 (and the Universal L Bracket - Triple,
  Grip_tip outlines) write a circle whose start, end and centre are one point at an arbitrary place and its size only as a
  dimension; the reader leaves it out ("no closed regions").
* **arig-revisions** (15 of 50 since round 1): 29 of the 35 refusals are the `_Symétrique` mirror parts (a mirrored copy of
  another file's body: the history would be the stored body only, which is not a rebuild), the other 6 are the
  Angle_support revisions, which stop at a loft cut (`moBlendCut_c`) followed by thin extrudes (`moBossThin_c`) and fillets
  on them; no single cause.

**Coverage after the round** (rebuild check unchanged; whole corpora, the history test through the corpus driver at
dc3b58a; the later class-declaration rule changes only the three SG90 trees, none of which reads whole): slip-pad 24 of
35, arig-revisions 15 of 50, zenodo 1 of 14 (unchanged), sw-more **199 of 1253** (was 195; none lost):
combined_mount_camera_lid (its circular pattern), Example1 and Universal L Bracket - Double (mirrors of features; both
were volume mismatches), Stepper-Bracket (an edge on a mirror's copy). All corpora: 235 -> 239 of 1352. Patterns no
longer stop any part as unsupported (37 before): those parts now stop at their next blocker (fillets on pattern copies,
up-to-vertex ends, combine, split, draft, delete-face, sketches with unsolved circles) or, 11 parts, at a pattern not
read (4 patterns of bodies, 5 seeds not found, one second circular direction, one without a seed list). Remaining sw-more
reasons, by parts: imported bodies only 540, volume mismatch 85, fillet edges 42, `moShell_c` 22, no feature stream 21,
edge midpoint not on the model 18, up-to-surface ends 16, split body 16, loft 16, sketches without closed regions 14,
mirror plane not resolved 12. RJ45-LED and Link_2 are skipped by the corpus driver as before.

## L15. Round 9 (sw4: measured blockers; equations, shells, closed edges, standard mirror planes)
Measured first against round sw3's final logs (slip-pad 24, arig-revisions 15, zenodo 1, sw-more 199). The refusal
tally put no single cause above a dozen parts that a decode could reach: imported bodies only 549, volume mismatches 85,
features that do not rebuild 74 (fillet edge midpoints not on the model 19, sketches without closed regions 15, the rest
one or two parts each), fillet and chamfer edges not all read 52, `moShell_c` 22 (14 of them the r3 keycap family, which
then stops at a variable fillet). The blockers taken, in order of parts per effort, decoded from the sample bytes:
* **Alignment groups before a sketch's entities.** A sketch node can be followed by its `moAlignGroup_c` nodes (Arm Tab
  Large PolyCarb First's Sketch2: thirteen of them) and only then by its entities; the span of the sketch held none, so it
  read empty ("no closed regions"). When a sketch's own span holds no entity header, it now runs on over the alignment
  groups after it, for the entities and the plane (9 LakerRobotics Arm Tab parts).
* **The 8 zero bytes before an entity payload** are written by some version 1 headers too (the Hecatron OpenBuilds
  Aluminum Spacer: every point read as type 0); they are skipped whatever the header version (a payload never starts
  with 8 zero bytes).
* **Depths an equation drives.** A feature whose `D1` an equation drives keeps no `D1` in its node. The configuration
  stream writes the equation as a CString (`"D1@Boss-Extrude1"= "rod_length_xaxis"`), then a `moRelDimension_c`,
  `moDimRefWrapper_c`, `moFeatureDimHandle_c` and the dimension's own parameter object named `D1`, laid out as in the
  feature stream, a few hundred bytes on. A blind or mid-plane extrusion without a depth takes it from there
  (ThreadedRod 369.52 mm, ThreadedRod-ZAxis, LinearRod, debug_block). HGR20's configurations drive different lengths
  (125 and 350 mm) and the part's body matches neither: it stays a volume mismatch.
* **Mirror planes named by id.** A mirror without its own frame block names its plane: the plane feature's `u32` id 38
  bytes before the display block it keeps (`<tag> 00 00 <id> <stamp> 00 00 03 00 ... c7 cf ff ff` x3). For the Front, Top
  and Right planes (ids 2, 3, 4) that is the plane (pulley_large, Handle Bar, 2_2kw_spindle, Link 6, the HalitKosemen
  Turkish parts); arm's Mirror1 had taken a tilted face from the fallback face list and is the Front plane. A plane
  feature of the part keeps its definition, not a frame (base v2's Plane2 is a `moConstraintCoincLinePerpPlaneRefplaneData_c`:
  through a line, square to a plane; the four doubles in it are display extents): not read, the face-list fallback stays.
* **Closed edges.** An edge record `[A, B, B, B]` (the last three references one face) names the closed loop where face A
  meets face B. Where A is the wall of a full circle of an extrusion, or the cylinder or cone a revolve sweeps from a
  profile line, and B a plane across it (a flat face of an extrusion, or the disc a revolve sweeps from a line square to
  its axis), the loop is the circle cut by the plane; it is picked opposite the circle's start, as revolve circles are
  (ballscrew_nut_block's and y_tram_new's Chamfer1: hole rims on a side wall).
* **Edges along an extrusion between other features' planes.** An edge between two sides of one extrusion ran between
  that extrusion's caps unless both vertex faces were caps of one feature; when both vertex faces are planes across the
  edge (a cap, or a flat wall of another feature) it now runs between them (T-Nut-M4/M5-DropIn's Fillet2: from the boss's
  top down to the wall of a cut made from the side).
* **Shells.** `moShell_c` writes its wall as `D1` and, last in its span, a record whose first reference is the face it
  removes (the records before it name the shell's own faces). It becomes a tree shell of that face, picked as end faces
  are; caps of profiles of arcs only (RobotCap's disc) are now picked just inside the middle of their largest arc. RobotCap
  (two copies) and Motor Holder rebuild; Linear Actuator Bracket and Rover Top build the shell's face but the kernel's
  shell fails; station_lower's face (a side of a profile arc) is not placed. Several removed faces were not seen.

**Per-feature harness over the 85 sw-more volume mismatches of round sw3** (scratch): the harness's own booleans against
the stored body failed for 28 parts in round sw3; where the cut fails it now takes the other cut or the common part
(`vol = common + outside`, `truth = common + missing`), which leaves 2 without a verdict (piston, 8020-228104: all three
booleans fail on the Parasolid body). The feature that grows the larger of the final outside or missing volumes most:
a cut 26, the first boss 24, a later boss 12, material never added by any feature 7 (6mm Motor Holder, Part8, the 4bd
zsCAD part ...), stored or imported bodies 6 (zsCAD `Body___<n>`, Hobby Motor, StationUpper, 10-32 Nylock Nut), the
bearings' chamfer 4, a pattern or mirror 2 (Link 3's CirPattern1, 47065T177's Mirror1), a hole 2 (Link_1v2, Link_4). No
cause covers more than a handful of parts that one decode could fix.

**Findings, not fixed.** Up-to-surface ends whose face is not placed (18 parts: caps of all-arc profiles now are, but the
rebuilt V-Slot Wheels and gt2_pulley 2 then differ in volume, so the change to their ends was not kept), fillets on pattern
copies (3 parts where it is the first blocker), a mirror of a mirror feature (Link 6: Gusset's tree import mirrors
extrudes, holes, fillets and drafts, not mirror copies), mirror planes on plane features of the part, revolve faces swept
from profile circles (EX +2.4's flange edge), saddle-shaped closed edges (Krank mili).

**Coverage after the round** (rebuild check unchanged; whole corpora, the history test through the corpus driver at
bc4c71f): slip-pad 24 of 35, arig-revisions 15 of 50, zenodo 1 of 14 (unchanged), sw-more **223 of 1253** (was 199; none
lost): the nine Arm Tab Large parts, ThreadedRod, ThreadedRod-ZAxis, LinearRod, debug_block, T-Nut-M4/M5-DropIn,
Aluminum Spacer, Motor Holder, RobotCap (two), arm, Handle Bar, 2_2kw_spindle, ballscrew_nut_block, y_tram_new. All
corpora: 239 -> 263 of 1352. Remaining sw-more reasons, by parts: imported bodies only 540, volume mismatch 91 (arm left
it; HGR20, pulley_large, tekerlek and others entered it once they read whole), features that do not rebuild 69
(edge midpoints not on the model 19), fillet and chamfer edges 50, no feature stream 21, variable fillets 16 (the r3
keycaps, past their shells), lofts 12, up-to-surface ends 11, sketches without closed regions 4, mirror planes not
resolved 4. RJ45-LED and Link_2 are skipped by the corpus driver as before. What is left divides into blockers of one to
three parts each.
