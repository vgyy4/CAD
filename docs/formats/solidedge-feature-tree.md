# Solid Edge part feature history (clean-room notes)

Derived from sample bytes only (2026-11-09; Zenodo record 8028435: 15 `.par`, of which 2 are ordered-feature parts with a history and 13 synchronous parts without one). Prototype: `scripts/re/solidedge/`. Status: ADR 0046 amendment.

# Solid Edge .par feature-tree layout (clean room, from Zenodo 8028435)
(append-only log; newest findings at the bottom)

## F0: OLE streams of a .par (all 15 parts)
Root streams: SummaryInformation, DocumentSummaryInformation, PSMclustertable, PSMroots, PSMuserroots,
PSMtypetable, PSMsegmenttable, PSMcluster0 (big: 70-240 KB), GeometricVersionHistory, Versions,
"0x00000000"/"0x00002000"/"0x00004000" (misc), PMIDesignDimStream, SKETCH.P_B, STREAM0.X_B (or P_B+D_B),
Cache0, Library0, PartsLiteData, JSitesList, JBlocks ... Candidates for the feature tree: PSMcluster0 + PSMtypetable + PSMroots.

## F1: where the model lives (15 .par)
* `PSMtypetable`: starts `ePyT`(u32 ver=1)...; records: GUID(16) + u32 type index + u16 1 + u16 0xffff + u32 k(1|5|14..)
  + str(u16 len incl NUL, bytes, NUL) class name + str same for DLL name (PARTDEX.DLL, GRAPH2D.DEX, PROFDEX.DLL...).
  Class names: CLSID_JProfile/JProfile2d/JProfileSet, CLSID_JLine2d/JArc2d/JCircle2d/JRectangle2d, constraints
  (JHoriz2d, JVert2d, JPointOn2d, JTangent2d, JPerpendicular2d, JOffsetCns2d, JAlignHz2d), CLSID_JHole2d, CLSID_JMHolePropData,
  CLSID_JDim, CLSID_JKeyPoint2d, CLSID_JFaceSet... (my parser: scripts psm.py, regex on the `01 00 ff ff NN 00 00 00` marker).
* `PSMcluster0`: the object store (70-240 KB). starts with magic `44 f5 90 6c`, u32 first-record length, then records.
  Contains UTF-16 property names (PreferenceSet, Windows, SiteObjects...), and per feature "FeatureNNNN" names, "Profile2d",
  "JMProtrusionFunction", "JMRevolvedCutoutFunction", "ExtrudedProtrusion_1_FiniteDepth", "Hole_1_Diameter" ... (UTF-16LE).
* `SKETCH.P_B`: Parasolid partition (sketch wires); `STREAM0.X_B` or `.P_B+.D_B`: body.
* IMPORTANT: 13 of 15 parts are SYNCHRONOUS ("Direct Edit 1" in PartsLiteData, no ordered features, only Sketch_N profiles + Hole_N_* dims).
  Only `schraube.par` (JMProtrusionFunction + JMRevolvedCutoutFunction, "Extrusionsauspr...") and `halter fuer probnhalter.par`
  (ExtrudedProtrusion_1, ExtrudedCutout_1/2: German names Extrusionsausschnitt_N_Feste_Tiefe) are ordered-feature parts.

## F2: PSMcluster0 framing (partial)
* offset 0: magic `44 f5 90 6c`, u32 (file-dependent, 0xcd..0x18e), then object records. Object records are introduced by `51 51`
  (ASCII "QQ") + u16 payload length + u32 object id (seen: `51 51 3d 00 4d 01 00 00`: len 0x3d, id 0x14d; next `51 51 0c 00 4e 01 00 00`).
  Strings are UTF-16LE preceded by a byte = nchars*8 (bit length) in the name dictionaries.
* Feature/associative-function records: the tail of the cluster (about 55%..100%) holds, per feature, a type-signature string such as
  `%>r%<r%<r%n` (ASCII, `%<r` input ref, `%>r` output ref, `%#<r` ...) immediately followed by the function class name:
  JMProtrusionFunction, JMCutoutFunction, JMRevolvedCutoutFunction, JMHoleFunction, JMUserDefinedPatternFunction,
  JMCreateReferencePlane, JMRefPlaneConnect, JProfileRecomputeEx, JProfGetPDFunc, JMProfileChainBindFunc, PrfWBndFunc,
  ProfileVertexBindFunc, JMCreateLinearAutoDimensionForClosedProfile, JMDimToHoleDataUpdateFunc, JMHoleDataToHole2dUpdateFunc,
  JMTransmigrateAssocFunction (sync-modeling bookkeeping), SEDirectEditFeatureAssocFunc (sync "Direct Edit"),
  plus date `dd.mm.yyyy` / `hh:mm:ss` UTF-16 timestamp strings and user names (r.lueck, e.woods) per record.
* Variable (dimension) names in a name table ~ offset 20 KB: "ExtrudedProtrusion_1_FiniteDepth", "ExtrudedCutout_1_FiniteDepth",
  "Hole_1_Diameter", "Hole_2_FiniteDepth", "Linear_1..5" (halter); values not yet located.

## F3: object framing in PSMcluster0 (CONFIRMED on schraube: 306 objects, serials 1..1243 monotone)
* An object starts with `51 51` ("QQ"), u16 `kind`, u32 `serial` (little endian). `kind` = type index into PSMtypetable (record GUID, u32 idx, ... name),
  e.g. kind 64 = CLSID_JProfile2d, 67 = CLSID_JKeyPoint2d, 36 = Double Value Object. Serials strictly increase (gaps allowed) -> use them
  to reject accidental `51 51` inside payloads. No length field: an object runs to the next accepted `QQ`. (psm.py: objects()).
* Cross references are 24-byte items: u32 0x10, u32 0x80000000, u32 target serial, GUID(16) of the target class (seen in feature function records).

## F4: SKETCH.P_B = Parasolid partition of the sketches (CONFIRMED decodes on schraube)
* Standard XT neutral binary with embedded schema (SCH_3101255_31100_13006, "PS\0\0" + u16 len + modeller + i32 len + key + u16 ntypes + i32 userfields,
  then per node type first seen an edit script). Decoded with xt.py (port of gusset-xt/decode.rs; same table + V13 body override).
* schraube SKETCH.P_B: 3 BODY (wire bodies), LINE x3, CIRCLE x2, POINT x3, EDGE x5, VERTEX x3 + ATTRIBUTEs named SolidEdge/ProfileEntityInfo, SolidEdge/ProfileMemberIDs.
* SKETCH.P_B geometry is in ABSOLUTE PART COORDINATES, METRES (schraube: CIRCLE r=0.0035 centre (0,0,0) normal (0,0,-1); CIRCLE r=0.002 centre (0,0,-0.004) normal +z;
  3 LINEs in the x=0 plane forming a triangle (0,0,-0.01286)-(0,0.0013,-0.016)-(0,0,-0.016)). One wire BODY (body_type 6 = wire? 2 = ...) per profile;
  nodes: BODY -> REGION -> SHELL -> EDGE (curve = LINE/CIRCLE) -> FIN -> VERTEX -> POINT. No separate arcs/planes: sketch plane has to be recovered
  from circle normals / coplanarity (or from the feature function record in PSMcluster0 - TBD).
* Attributes on each profile BODY: `MONIKER/BODY_ID_ATTRIB` (REAL_VALUES [n], CHAR 'SE 220.0.0.104'; n = 480/407/336, an object serial),
  `SolidEdge/ProfileMemberIDs` on vertices/edges (INT_VALUES [id]; id = 481/408/337 = serial of the profile object in PSMcluster0?), `SolidEdge/ProfileEntityInfo`, MNKR_ATTR_*.

## F5: ordered-feature parts, semantics (schraube.par, CONFIRMED against the Parasolid body)
* Truth (OCCT import of STREAM0.P_B): screw, head cylinder r3.5 z0..-4, shank r2 z-4..-16, conical hole r1.3 at the tip z=-16, apex z=-12.86; V=299.18.
* Feature order = ascending object serial of the function objects: JMProtrusionFunction(397) JMProtrusionFunction(469) JMRevolvedCutoutFunction(1240)
  (plus cosmetic thread JMThreadFunction 475). Each function object is preceded in the cluster by: JMTransmigrateAssocFunction, JMCreateReferencePlane (obj kind 6),
  JProfileRecomputeEx (AttrSetValues), the Profile2d object, JMCreateLinearAutoDimensionForClosedProfile (a "Double Value Object") and a
  466-byte "Assoc subsystem Standard Relation" object = the dimension: f64 at +202, +291, +402 (little endian, metres) = the depth.
  schraube: relation 395 = 0.004 (protrusion 1: circle r3.5, 4 mm), relation 467 = 0.012 (protrusion 2: circle r2 at z=-4, 12 mm). V = pi*3.5^2*4 + pi*2^2*12 - cone = 299.2 OK.
* Profile2d object (kind of CLSID_JProfile2d): QQ kind serial, u32 n, n x u32 member serials (circle/line/keypoint elements), then GUID ... ; the
  SKETCH.P_B edge attribute SolidEdge/ProfileEntityInfo INT [1,0,<member serial>,0,<profile attrset serial>] links a SKETCH.P_B edge to its Profile2d member.
  MONIKER/BODY_ID_ATTRIB REAL = serial of the profile's AttrSetDef (profile attrset serial - 1).

## F6: feature chain + links (CONFIRMED on schraube, halter)
* A feature function object = object containing ASCII `JM<Name>Function\0` (Protrusion, Cutout, RevolvedCutout, RevolvedProtrusion?, Hole, Thread, UserDefinedPattern).
  Inside it the 24-byte reference items `10 00 00 00 | 00 00 00 80 | u32 serial | GUID16` appear in a fixed slot order. refs list (in file order):
  [0] = PREVIOUS feature function in the tree (0 for the first) -> follow to get the tree order (holes in a pattern included; serial order is NOT tree order:
  halter chain 1538 > 2468 > 2580 > 2694 > 2399 > 1768 > 1756 .. 1750 > 1913),
  [4] = the dimension relation object (466-byte "Standard Relation": f64 at +402 = depth in metres), [9] = AttrSetValues serial of the feature's profile
  (= `SolidEdge/ProfileMemberIDs` INT value in SKETCH.P_B; schraube 337/408/481, halter 1016/1553/2191).
* Function object contains "[u32 x6 header][...]" after bytes `5a bb 04 00`: the 5th u32 is 0 (schraube P1, halter 1538) or 1 (schraube P2, halter cut/protrusion 2399, cuts, revolve);
  hypothesis: 1 = direction reversed vs. the profile plane normal (fits all 7 samples, not proven).
* After those header words: 6 f64 = body bounding box AFTER the feature (metres) (schraube: after P1 [-3.5,-3.5,-4]..[3.5,3.5,0]; after P2 zmin -16).

## F7: RESULT schraube.par rebuilds EXACTLY (vol 299.18 = truth, box 0.000)
Tree built by sefeat.py: Sketch(circle r3.5 @z0, normal -z) -> extrude 4 (new) ; Sketch(circle r2 @z-4, normal +z) -> extrude -12 (join) ; Sketch(3 lines, plane x=0) -> revolve 360 about Z (cut).
All values (4, 12 mm: relation objects +402; profile geometry: SKETCH.P_B) are read from the file; the revolve axis (Z) is a heuristic (profile line lying on a world axis),
angle 360 is a default (no angle field found); extrude directions come from bbox-after in the function object (protrusions) / flag5 (see F6).

## F8: Feature lists (features.txt, order = prev-pointer chain, refs[0]) and variables
* Variable/dimension name table (inside PSMcluster0, ~offset 20 KB, UTF-16LE): entry = u32 serial, u32 nchars, name, u32 2, u32 1, u32 target-feature serial, u32 2, u32 3, u8 1.
  Value: object `serial`; if it is the 466-byte relation -> f64 at +402; if it is a small (<=160 B) object -> f64 at +8. Metres. Names: ExtrudedProtrusion_1_FiniteDepth,
  ExtrudedCutout_1_FiniteDepth, Hole_N_Diameter / _FiniteDepth / _ThreadDepth / _CounterBoreDiameter / _CounterBoreDepth (localised: Extrusionsausprägung_N_*, Bohrung_N_*).
* Ordered parts (2/15): schraube = Protrusion(4) , Protrusion(12), Thread, RevolvedCutout ; halter = Protrusion(20), Cutout(5), Hole, Hole, Protrusion(7.535),
  Pattern + 8 Hole (1768..1750), Cutout(1).
* Synchronous parts (13/15): no Protrusion/Cutout function objects at all (history folded into the Parasolid body); only Hole (+ UserDefinedPattern) and Thread
  function objects remain, together with the original sketches in SKETCH.P_B (not always: 4 parts have none) -> the solid cannot be rebuilt from the features.
  Hole data present: diameter, FiniteDepth (50.8 = 2 in default for through), ThreadDepth, CounterBoreDiameter/Depth. Hole positions not decoded.
* Two parts (M1.2x3_screw, Plastic-handle-Loaded-Felfer-holder) fail even the Parasolid import in Gusset (ground truth unavailable).
* Assemblies (.asm): not examined.

## F9: COVERAGE (harness rebuild.py, vol within 1e-4 rel / box 0.01 mm)
* EXACT: schraube (rel 0, box 0.000).
* PARTIAL: halter_fuer_probnhalter: Protrusion(20) + Cutout(1) rebuild, vol 118371.75 vs 106021.475 (11.6 %), box exact 0.000. Cutout_2468's profile (10 lines: outer rectangle
  50.6x50.1 with a 3 mm strip and a dangling 12 mm stub, overlapping collinear edges) is rejected by Gusset ("not imported"); which loop is cut is not in the profile data decoded.
  Truth shows a pocket whose floor is at z 13.4-14 and wider (x +-26.3) than the cutout sketch (+-25.3) -> other features (fillets/holes/Protrusion_2399) contribute; not decoded.
* 12 sync parts: not reconstructible. Profile (sketch) geometry is decoded for those that have SKETCH.P_B.
* Open: direction flag meaning (flag5), revolve axis and angle, profile region selection, hole placement, Protrusion 2399 (profile slot [9] points to a hole serial), thread feature.

## F10: round se1 (2026-11-14), the second corpus and the Rust reader
Corpus: 184 ordered `.par` (182 from 11 openly licensed GitHub sources in `samples/se-more`, plus schraube and
halter). Schema keys seen: SCH_*_28002/28101/30000/30100/31100/32001/34001/36001/37102. Reader:
`crates/gusset-ole/src/solidedge_features.rs`; corpus test `crates/gusset-cli/tests/par_history.rs`.

* **Objects (corrects F3).** Serials rise through the store but may jump far (31333 after 8143 in one part): the objects
  are the longest strictly rising run of `QQ kind serial` candidates (kind < 4096). A `QQ` inside a payload breaks the rise
  and drops out. Object kinds name typetable classes for most objects, but NOT for the 2D curves (a line's kind maps to an
  unrelated class name, e.g. "Name Manager object"); constraint, key-point and dimension kinds do map right, so profile
  members are filtered by class name and the rest is recognised by layout.
* **Typetable record:** `class id(16), u32 index, 01 00 ff ff, u32 instance count, u16 n, name(n incl NUL), u16 m, dll`.
* **Function:** first type signature in the object: `u32 n, sig (n-1 chars of %<>#rn), NUL, u32 m, class (m-1 chars),
  NUL, u32 slot count`, slots: `0x10, u32 flags (0x80000000 in, 0x40000000 out), u32 serial, class id(16)` or
  `0x80000010, u32 count[, u32 flags, count x (u32 serial, class id(16))]`. Same slot layout for protrusion and cutout
  in every version seen (21 slots), revolves 20.
* **History:** feature = function whose class is `JM*Function` (minus Transmigrate/DummyCompute/KeyPoint bookkeeping);
  slot 1 = previous feature; the history is the longest chain.
* **Extrude slots:** 2 = list of profiles (`JProfileRecomputeEx` attribute sets), 3 = extent kind, 4 = side,
  6 = depth, 12/13/14 = the second extent (kind, side, depth), all value objects except 6 and 14 which may also be
  dimension relations. Value objects (<= 64 bytes): f64 at +8. Relations (450-466 bytes): `u32 1, u32 k` at +0x88/+0x90,
  `k` again later (+0xbe/+0xc6), then the f64. Extent kinds: 13 finite, 14 through all, 16 through next, 72 to a key point
  (slot 6 is then a `JMLinearKeyPointFunction`), 17 revolve full turn. Sides: 1 against the profile plane's normal,
  2 along it, 3 symmetric (depth = total), 6 both ways (through). CONFIRMED by the stored tool box of 124 protrusions and
  by exact rebuilds.
* **Revolve slots:** 6 = axis (`JMCreateRAThrough2dLine`, head `u32, u8 3, point, direction, t0, t1`, metres, absolute),
  7 = angle (radians), 3 = 13 finite / 17 full, 4 = side (1 negative turn).
* **Plane** (`JMCreateReferencePlane` and the base planes): head `u32, u8 14, origin(3), normal(3), x direction(3)` (metres,
  absolute; the `JTransfValue` it refers to is relative and identity in the samples). The profile's 2D frame is x = x
  direction, y = normal x x direction.
* **Profile:** the attribute set names its `Profile2d` (UTF-16 "Profile2d" after `u32 serial, u8 72`); `JProfile2d`:
  `u32 n, n member serials`. Members (u8 0, u8 flags 0/8 at +8): line 46 bytes `x1 y1 x2 y2`; circle 55 bytes
  `cx cy r 0.0 1.0 u8`; arc 55 bytes `cx cy r u8 ccw a0 a1` (ccw 0: clockwise from a0 to a1); polynomial B-spline
  `u32 1, u32 0x80000001, u32 n, u32 0, n poles, u8 order, u32 k, k mults, k knots, 20 bytes`; `u32 2` there is a rational
  spline (weights not decoded); 71 bytes ellipse arc (not decoded). A member serial 0xffffffff is an empty slot.
* **Members missing from the store** (sketch-based profiles, newer files): `SKETCH.P_B` edges carry
  `SolidEdge/ProfileEntityInfo` integers `[1, 0, member serial, 0, profile]`; LINE / CIRCLE curves there, the head fin's
  vertex ends the edge (as in a body), the curve sense gives the turn. CONFIRMED (the 15 truss members of Shass27 rebuild
  only so).
* **DummyCompute profiles** (slot 2 of 546 features): copies made by smart patterns (220 of a sample) and mirror copies;
  they list their curves at +16 (`u32 n` at +12) and 3D copies of them, but no plane: not read.
* **Second extents** are common in the older files (servo tower: 16.002 one way, 6.858 the other).
* Coverage of round se1 (OCCT test par_history, 184 ordered parts): 28 as history, 154 as exact bodies, 2 failing in the body import (worker killed/crashed, as before). ADR 0046 amendment 11.

## F11: round se2 (2026-11-14): blends, copied profiles, open cuts, sweeps, lofts
Same corpus (184 ordered parts). All from the sample bytes; each rule checked by exact rebuilds in `par_history`.

* **Edge sets** (`JMTransmigrateAssocFunction`, the edges a round or chamfer works on; the names are
  `SEGetRoundableEdgeSeq` entries, faces `JMGetSmoothFaceSeq`). After the names: `ff`, a class id ending in
  `cd 11 8b a4 08 00 36 24 fc 01`, `u32 1, u32 n, u32 0, u8 2`, then `n` curves, each a class id ending in
  `3a ce 11 9a 5e 08 00 36 9b 41 02`, a type byte and its data (metres, absolute), then a few words (`u32 0`; newer
  files two; after a spline a count and a zero): type 3 line (point, unit direction, t0, t1); 15 circle or ellipse
  (centre, unit normal, major axis whose length is the radius, minor/major ratio); 16 an arc of one (the same, then
  the angles from and to, counter-clockwise about the normal from the major axis, wrapping past 2 pi); 27 B-spline
  (`u32 n`, `n` poles xyz, `u32` weight count (0 in every sample), `u32 k`, `k` knots, `f64 -1`, `u8 0`, `u8 order`,
  3 bytes). The curves are the edges as they were when the feature was made: the washer's round lists the two
  circles r 9.525 at z 0 and 2.54 (CONFIRMED by exact rebuilds of 14 parts with rounds and chamfers).
* **Round** slots: 2 edge set, 3 kind (1 constant radius in 339 of 345; 2 and 17 not read), 4 radius (value object
  or relation), 7 five option values (66 69 71 152 154 nearly always). **Chamfer**: 2 kind (41 equal setbacks;
  42 with a reference face set in slot 3 and a second value in slot 6, not read), 4 edge set, 5 setback.
* **Smart pattern copies** (`JMDummyComputeFunction`, slot 1 the `JMSmartPatternFunction`, whose slot 4 is the seed
  feature): `u32` at +8, `u32 n` at +12, `n` member serials (the seed profile's 2D curves), the curves again (a type
  byte and their data in the seed's frame), then the copy's plane laid out as a plane object (origin, normal, x
  direction; 9 `f64`) just before `00 00 00 00 00 01 01 00 00 00` and the class id `40117dba-4476-1b10-ac07-
  08003601b14e`. CONFIRMED: a 16-fold circular pattern in 22.5 degree steps (normal kept, x turned), a pattern
  along a 3D curve.
* **Mirror copies** (slot 1 a `JMMirrorCopyFunction`: slot 2 the mirror plane, slot 3 the seed feature) store no
  plane: the seed profile's plane reflected, its curves turned over (y to -y) so the sketch keeps the reflected
  normal; for an open profile the side flips.
* **Open profiles of cutouts**: one chain with two free ends; slot 5 is the side cut away, 1 left of the chain as
  drawn, 2 right (lines from their first point; an arc member with `u8 0` at +34 is drawn clockwise). The reader
  runs the ends on along their directions to a circle 10 m round them and closes on that side.
* **Up to a key point, side 3**: symmetric, the stored reach each way (the pattern copies of such a cut store twice
  the reach as their symmetric depth).
* **Multi-section sweeps**: slot 2 the path's profiles, slot 3 the cross sections' (a sketch profile, or an edge
  set of one whole circle: an edge of the model), 16 another profile (not read). One section along one path is a
  sweep (frenet). **Lofts**: slot 2 the cross sections in order; slots 11 and 14 the ends' conditions (20, 113),
  13 and 16 their weights: not read, the rebuild check decides.
* **Spline members of kind 2** (`u32 2` at +10, flags 1 or 0x80000001 at +14) are drawn through points: the poles
  and knots are the curve; after the knots `u32 0, u8 1|4, u8 1, u32 m`, `m` point parameters and their indices.
* **Profile members** with byte 8 not 0 (relations and dimensions the `Profile2d` lists with its curves) are not
  curves.
* **Thin walls** (`JMThinwallAssocFunctionNew`, 13 parts): slot 2 a value (4), 3 the thickness (relation), 4 (one
  or two faces) and 5 (one face) face sets (`JMGetSmoothFaceSeq`, the same list layout; every face in the corpus a
  plane, type 14: origin, normal, x direction), each plane followed by `u32` 1 or 2, `u32 m`, `u8 2, u8 0` and its
  `m` boundary curves as type byte and data like an edge set's (no class ids). Not read yet: which slot holds the
  open faces and what slot 2 says are open; the importer would pick each face by points of its boundary.
* Not read: ellipse members (71 bytes), thin walls, holes, part copies, helices, protrusions through all or to the
  next face (their reach is the part's), variable rounds, chamfers with two setbacks.
* Coverage of round se2 (OCCT test `par_history`, the same 184 ordered parts): **56 as history** (was 28; none lost),
  126 as exact bodies, 2 failing in the body import as before (see ADR 0046 amendment 12 for why). First reasons of
  the 126: a feature kind not read 43 (thin walls 15, smart patterns 4, part copies 4, helices 4, others), a round or
  chamfer the kernel does not build 28 (mostly a radius as large as the face it rolls over: Solid Edge lets the
  round take the face, OCCT fails), a rebuild that differs 15, a profile curve not read 7 (ellipses), a round edge
  not found 5, others 28.

