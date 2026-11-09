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
