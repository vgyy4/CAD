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
