# Fusion design stream (clean-room notes)

Derived from sample bytes only (2026-11-09; 37 design streams of 23 public Adafruit and other designs). Prototype decoder: `scripts/re/fusion/` (Python, `python3 -I f3dfeat.py OUTDIR FILES...`). Status and the plan: ADR 0039 amendment.

# Fusion 360 design stream: layout notes (clean-room, from sample bytes only)

Corpus: 23 distinct `.f3d`/`.f3z` sample files (not in the repository) (80 paths, the rest duplicates/symlinks),
47 designs, 37 distinct design-segment streams (hash-deduplicated). Every statement below is
marked **confirmed** (holds on all streams tested, with the check named) or **guess**.

All integers little-endian. `str` = u32 length + ASCII. `wstr` = u32 char count + UTF-16LE.
`ref` = `01` + u64 object id + `00` (10 bytes; confirmed by the `StrongRefMetaType` attribute
values, which are exactly these 10 bytes). Some streams (older class versions: 1092.f3d, LoRa
2d4a745a, Crickit) write **typed refs**: `01` + u64 id + `str` target class GUID (49 bytes), e.g.
`01 a8 00 00 00 00 00 00 00 24 00 00 00 "8DA771B7-52ED-…"`.

## 1. Container
* `.f3d` = ZIP; `.f3z` = ZIP of `.f3d` ZIPs. Compression methods 0, 8 and **93 (zstd)**
  (13 entries in the corpus; Python's zipfile cannot read them, `f3dzip.py` decodes via libzstd).
* A design segment = a pair `<seg>/MetaStream.dat` + `<seg>/BulkStream.dat`, under
  `FusionAssetName[Active]/` (also `Animation/`, ignored). Segment dirs seen: `Design1`
  (2021-2024 files), `FusionDesignSegmentType1` (2025 file), `FusionACTSegmentType1`, `ACT1`,
  `FusionBrowserSegmentType1`. All share the framing below.

## 2. MetaStream = class table + object index (confirmed: 37/37 streams parse to the last 8-1034 bytes)
```
str  segment name ("Design" / "FusionDesignSegmentType")
u32  0
wstr uuid (all zeros in the samples)
header, two variants:
   2021-24: str segment name again, u32 0, u32 1, u32 0
   2025   : u32 1234, u32 8, u32 ?, 4 bytes, str "FusionDesignSegmentType", str "Fusion", u32 1, u32 0
   (the parser just scans for the first `u32 n, 24 00 00 00, <GUID>`)
u32  nclasses
nclasses x { str class GUID, str parent GUID ("" for roots), u32 class version,
             str module ("Fusion","MSketch","Geometry","CommonData","Body","Component","Scene",
             "EntityTracking",...), u32 n, n x u64 object id }
u32  n, n x u64        -- ids not listed under a class (stream roots: ids 0, 1, 5, ...)
u32  n, n x {u64 id, u64 offset into BulkStream}     -- the object index
u32  n, n x {u64 id, u64 offset}                     -- a second, short table (meaning unknown)
(trailing 8..1034 bytes not decoded)
```
Example (`458a6c97…f3d`): class table at 0x78 (`5d 00 00 00` = 93 classes); index count
`a4 00 00 00` at 0x27b1, first pairs `(0,0) (1,0x1e9) (2,0x417) (3,0x42d)`.

## 3. BulkStream = concatenated object records
Record i spans `[index[i], next higher offset)`. **Confirmed on 105,644 records**: every record
starts with `str` of 3 ASCII digits (the *class tag*) and `u64 id` equal to its index id, and
**tag = 256 + position of the record's class in the MetaStream class table**.
```
03 00 00 00 "363"  07 01 00 00 00 00 00 00      tag 363, id 0x107 (=263)  (L-Bracket sketch point)
00 00 00 00  00  01  01 00 00 00                u32 0, u8 0, u8 has-attributes, u32 n attributes
                                                (older class versions: u32 0, u8 has-attributes, u32 n)
  str "pt_tag"  str "IntrinsicMetaTypeuint64"  u64 0x65             attribute (key, type, value)
<body>
```
Attribute value sizes: `…uint64` 8, `…IString` wstr, `PassiveRefMetaType`/`StrongRefMetaType`
10 (a ref), `EA46307F-…` (a colour) 16. Keys seen: `pt_tag`, `crv_primary_id`,
`crv_secondary_id`, `crv_tag_id_idx`, `PCBCurve/PCBLayer/PCBLoop`, `EntityGenesis`,
`BASEFEAT_PM_BODY_REF`, `DcFeatureOperationIdFlag`, `SpecifiedColor`, `EdgeStart/EndVertexProgenitor`.

**Sections**: a record of a derived class carries further sections, each `str tag` + the same
`u64 id` + `u32 0`, holding the data of another class in its chain (e.g. sketch record = own
section tag→`44A64366` + section tag→`37AD519C`; a construction line `16DEFC4D` record holds a
`DCA267ED` section with the line data). Decoders therefore look up "the section of class X"
(confirmed for points/lines/arcs: all decoded entities are internally consistent, see §5).

Bodies stay field-positional: no field tags, field sets change with the class version
(e.g. Extrude `DD405BC2` v5/v7/v8 shift the enum block by 1-2 bytes; Line v-old has 9 doubles,
newer 12). Decoders anchor on stable byte patterns instead of fixed offsets.

## 4. Timeline and feature order
* `2F4C1849-1A5A-4F6C-A086-8DD445CBF94B` = **timeline**: body `… ref(owner C3423E04) 00, u32 n,
  n x (ref)` in timeline order. One per design root (`C3423E04` records exist per component; only
  the root's list is non-empty). L-Bracket: `0a 00 00 00` then refs 0xeb,0xf5,0x134,0x168,0x1ab,
  0x1ca,0x25d,0x28c,0x392,0x3cf = [component-activation, Sketch1, Extrude1, Fillet1, Chamfer1,
  Sketch2, Extrude2, R-Pattern1, Hole1, R-Pattern2] (confirmed by names).
* `B65B5682` = compute-order list of all graph nodes (features + parameters); `B13113CC` =
  dependency pairs. Designs whose history is not captured (PCB-generated components) keep their
  features **only** here; the prototype appends them after the timeline, flagged `in_timeline:false`.
* Feature name: a `wstr` UUID (zeros in newer files), within ~1 KB a `wstr` base name
  ("Sketch","Extrude","Fillet",…), `u32` index (numbering restarts per component), 4 bytes,
  `wstr` user name (length 0 if none; e.g. "Board"). Name = user name or base+index.
* Feature class GUIDs (with base names, counts over the corpus): Extrude `DD405BC2`,
  Sketch `8DA771B7`, Fillet `A07D5F17`, Chamfer `F757D611`, Revolve `E3849A15`, Draft `F4163A07`,
  Mirror `D1728651`, R-Pattern `AE12AD3F`/`E8816367`, C-Pattern `11F1A5CE`, Hole `1C037A07`,
  Shell `E8063467`, Sweep `FCBB1707`, Loft `B6678572`, Combine `2A94257F`, OffsetFaces `D087EFE5`,
  Move `64E40C8E`, WorkPlane `D869265F`, CreateComponent `78B9DEFC`, Base Feature `D7015B81`,
  Component Insert `428B5C6E`, As-built `AA8B3D40`, JointOrigin `02E8AA49`, PCB `B0EEF272`,
  Canvas `0EE4A83E` … (full list in `f3dfeat.py`).

## 5. Sketches (confirmed)
* Sketch feature `8DA771B7` refs the sketch data record `44A64366` (module MSketch).
* `44A64366` section `37AD519C`: first `ref` = placement record `F47A46FB`; then ref lists
  (`u32 n` + n refs, element stride 11) of all entities: points, lines, arcs, dimensions
  (`8C780195` linear, `664AD49F`/`6EEAFDE5` diameter/angular, `8EDAC24D` radial), constraints
  (`60403D47`, in the own section), splines `D82E012F`, text `E0618268`, …
* Placement `F47A46FB`: either 16 doubles at body+7 (4x4 row-major; **columns = sketch x axis,
  y axis, normal, origin; cm**), or a 34/74-byte short form `… 01 00 ref(component)` with no
  matrix = the component's XY plane (identity). The identity reading is verified indirectly: 163
  new-body extrudes whose sketches use it match an ACIS body box (below).
  L-Bracket Sketch2 matrix rows `[1 0 0 0][0 0 -1 -1][0 1 0 0][0 0 0 1]` = x→X, y→Z, origin
  Y=-1 cm; the .smb holds the sketch region as a sheet body at exactly Y=-1, X±5.5, Z 1.5..17.
* Point `C2CEDAE7` section body: `ref` (point's curve list `362B7EC3`), 9 bytes (10 in one
  older typed-ref version), **x y z f64 cm** (sketch space; z = 0). 9,679 of 9,955 listed points
  decode; the rest fail the sanity filter used to pick the 9/10-byte gap.
* Line `DCA267ED` section body: origin(3), vector = end−start (3), unit direction (3) [, 3 more
  in newer versions], then refs: **end point, start point**. Check: start = origin and
  end = origin+vector for **1616/1616** lines.
* Arc/circle `F0130424`: center(3), normal(3), x-dir(3), radius, start angle, end angle (rad),
  then refs **center, end, start** (start = point at start angle: 184/184 in the first pass;
  center point = center and both end points at the radius: **1958/1958** arcs+circles with
  decoded points, 0 inconsistent). Full circle when end−start = 2π (717). Normal is ±Z in sketch space
  (126 arcs have −Z: the prototype swaps start/end to emit counter-clockwise arcs; CCW sweep
  verified for all 184 arcs).
* cm confirmed: L-Bracket points `(-4.5,0) (0,17) (-1,17) (-1,1)` cm vs dimensions "45 mm",
  "170 mm", "10 mm"; parameter values (`7A6A3D31`) are stored in cm / rad next to their mm/deg
  expression text.

## 6. Parameters
`D91D429C` (parameter holder, refs the owning feature) → `7A6A3D31`: wstr expression ("110 mm",
"caseWidth - 12 mm"), 5 bytes, wstr role name ("AlongDistance", "AgainstDistance",
"TaperAngle", "Radius", "Distance", "AlongAngle", "ProfileOffset", "Side1WallThickness", …),
wstr unit, wstr param name ("d3"), **f64 value in cm/rad** (confirmed: 110 mm ↔ 11.0,
0.0 deg ↔ 0, 360 deg ↔ 6.2832).

## 7. Extrude `DD405BC2`
* Profile: refs a `4BD53E5A` record whose UTF-16 decimal text is **the sketch record's object id**
  (226/226 such texts resolve to a `44A64366`). Which profiles of the sketch are used is NOT decoded
  (the record also holds a profile UUID). Face-based or PCB-data profiles use other classes.
* Enum block (position varies by class version; anchor regex in `extrude_raw`):
  `u32 op, u32 extent, u32 kind, [00|01] 01`, then a unit vector (3 doubles) = extrude
  direction, which equalled the sketch normal for every extrude with a known plane (dot = +1).
  * op: **4 = new body** (verified, see §9), **1 = join** (42/55 prisms inside a body box),
    **2 = cut** (L-Bracket slot through the leg; distances like "-6.8 mm"; weaker evidence),
    3 = intersect is a guess (never seen).
  * extent: 1 one side, 2 two sides (has AgainstDistance), 3 symmetric (one AlongDistance =
    **total** length: L-Bracket 110 mm ↔ body X −55..55).
  * kind: 2 = distance, 0/1 = to-object / all (no AlongDistance param) — guess.
* Distance: AlongDistance value ×10 mm, signed (negative = against the normal; verified on QTPy
  Extrude1 −1.6 mm ↔ body Z −1.6..0).

## 8. Fillet / chamfer / revolve
* Fillet radius = param "Radius" (56/56 fillets; multi-radius sets not handled).
  Chamfer = "Distance" (37/44). Revolve angle = "AlongAngle"; profile via `4BD53E5A` like extrude.
* Edges and the revolve axis are topological references (`9716F783` → `7ACC2A03`
  EntityTracking "body_recipe_data" records): **not decoded**, so no edge midpoints / axis.

## 9. Evidence summary (validate.py, consistency.py)
Ground truth = per-body vertex boxes from the ACIS `.smb` blobs (own SAB tokenizer, BinaryFile4 and
BinaryFile8). For each `new` extrude: box of the sketch geometry swept by the distance, compared
with every body box of the same design (component coordinates).
* new-body extrudes: **176/229 match a body box within 0.01 mm** (all 3 axes; 177 in-plane).
  Misses: case designs where later features (shell, cut, fillet, mirror) change the body, or a
  profile subset is used (1054.f3d extrudes 2 different profiles of one sketch), or the sketch
  holds construction geometry outside the profile.
* all extrudes (new+join+cut): 248/329 prisms lie inside some body box.
* Corpus coverage: 37/37 segment streams frame; 105,644 records with tag = 256+class index;
  29 designs written, 23 with features: 291 sketches, 395 extrudes, 56 fillets, 44 chamfers,
  2 revolves, 622 unsupported feature records (CreateComponent 213, Base Feature 102, Draft 33, …).

## 10. Round 2: profiles, bodies, components, timeline classes

### Profile regions (prof.py, regions.py)
* Extrude/revolve → `4BD53E5A` (profile; names the sketch by its object id) ← `0D57BD2F`
  (profile geometry; first ref = owner `4BD53E5A`). Layout after the owner ref and one byte:
  `u32 nregions`, per region `u32 nloops`, per loop `u32 n`, n × 40-byte entries
  `{u32 kind (2/3), u64 crv_primary_id, u64 crv_secondary_id, u32 reversed?, u32 a, u32 b, 8 bytes}`,
  `u8 flag` (1 = outer, 0 = hole), then per region `u32 0`. Parses 338/387 records exactly up to the
  next section tag (confirmed); 49 records (Crickit, 1WireRGBW, 1054…) use a nested variant
  (non-zero region trailer followed by more loops) that is NOT decoded.
* `(crv_primary_id, crv_secondary_id)` = the attribute pair every sketch curve record carries:
  2409/2412 loop entries resolve to exactly one sketch curve (confirmed).
* `(a, b)` = the loop uses piece a of b of that curve, the curve being cut at its crossings with
  the other profile-bounding curves (confirmed by reproducing the split on the samples; construction
  lines do not cut, and they are not flagged reliably, so the splitter first uses only curves that
  bound some profile of the same sketch, then neighbours in the loop, then all curves).
* Output: `profiles:[{"point":[x,y]}]` (sketch coords, mm), one point per region, found by a grid
  search for the point farthest from all edges inside the outer loop and outside the holes.
  `_profile_bbox_mm` (outer-loop box) is emitted for checking.
* Box check with the decoded regions instead of all sketch points: new-body extrudes matching a
  body box within 0.01 mm went from 176/229 to **206/236**.

### Bodies and targets
* `D26351F0` = body link: refs (body record `D3937028`, creating feature, component timeline
  `C3423E04`) — 676/677 links in the corpus have exactly this shape. Creators seen: Base Feature
  352, Extrude 239, R-Pattern 71, C-Pattern 9, Split 3, Sweep 2.
* A join/cut reaches the body links of the bodies it modifies through its own child records
  (`9716F783`, `2CA5A1CD`, …). `target` = name of the link's creating feature: 139 of 140
  join/cut extrudes get exactly one target (structurally derived; geometrically checked only on
  the L-Bracket slot cut → Extrude1).

### Components
* Component record `E03784ED`: last non-UUID UTF-16 string = component name; refs its timeline
  `C3423E04`; bodies through `CD57BC48` → `40DCC3F4` (`BREP.<id>.smb`).
* Feature → component: feature's last ref `3A6D1E62` ← `C3423E04` ← `E03784ED` (1049/1055
  features resolved). Each feature carries `component`; `components:[{name, bodies:[BREP…]}]`
  lets the importer place a component's features with the same transform it already computes for
  that component's bodies (placements themselves are not re-decoded here; gusset_sat::fusion has them).
  `single_component` = all features in one component and at most one component with bodies.

### Timeline classes
* Dropped (moved to `skipped`): CreateComponent `78B9DEFC`, component activation `54F9ACE8`,
  JointOrigin `02E8AA49`, Assemble `8DFDD543`, As-built `AA8B3D40`, Position `8EE00B00`,
  Canvas `0EE4A83E`, Decal `1339A425`, WorkPlane `D869265F`, PCB link `B0EEF272`. Basis: none of
  them is ever the creator in a body link, none references a body record. (Position and
  WorkPlane are assumed harmless for body geometry — guess.)
* Base Feature `D7015B81`: every one creates bodies (refs `D3937028` + body links) → kept as
  unsupported with `creates_bodies` (its bodies are plain ACIS, importable as such).
* Patterns `AE12AD3F`/`E8816367` (rectangular), `11F1A5CE` (circular), Mirror `D1728651`:
  inputs (5A1BF548 → 6CCF41D5 list of features), counts (`A085449A` integer parameters `countU`,
  `CountV`), spacing (`uSpaceDistance`, `vSpaceDistance`), `TotalAngle` decoded; direction / axis /
  mirror plane are topological references → still unsupported.

### Edges (fillet/chamfer/pattern direction/revolve axis): not decoded
`4B31DA1B` → `7ACC2A03` "edge_recipe_data": an edge is named by the faces around it, each face as
(operation id, generator index), e.g. `(0x37, op 0x12f)`. `5D89B935` maps operation ids to features
(0x12f and 0x130 → Extrude1 of the L-Bracket), `ABA79BE7` lists per operation the generator ids
(0x33…0x38 for the 6 profile curves). Resolving that to geometry needs the generator → sketch curve
and face-kind (side/start/end cap) mapping, which I did not establish.

## 11. Round 3: face/edge naming, nested profiles, extents

### Naming records (naming.py)
* `5D89B935` operation table → `{op id: feature}`. Two layouts: `u32 n, n × (u64 op, u64 feature)`
  (older) and `u32 n, n × (u64 feature, u32 k, k × u64 op)` (newer). A feature can own several
  operations (a symmetric extrude has two, one per half).
* `ABA79BE7` (one per body): runs of `u32 n, n × str generator, u64 op` = generators an operation
  left on the body.
* `7ACC2A03` recipe (all 3,668 in the corpus parse): `u32 1, u32 3, u32 nfaces`, per face
  `u32 k` entries `{str generator (decimal text, may be negative), u32 x, u32 n, n × i32 op ids,
  u32 0}` — creator op first, then ops that modified the face — followed by the kind string
  (`edge_recipe_data`, `face_recipe_data`, `bounded_face_recipe_data`, `vertex_recipe_data`,
  `body_recipe_data`) and topology indices. **Edge recipe: faces 0 and 1 meet at the edge,
  faces 2.. bound its ends** (confirmed on the L-Bracket against the ACIS body).
* Selected entities of a feature: feature → `2CA5A1CD` → `5662F619` → `7ACC2A03`; fillet radius
  sets: `4A557CE2` (one per set, own Radius parameter) → `2CA5A1CD` → … .

### Extrude face generators (extnaming.py) — ground truth from the ACIS bodies
* The `.smb` faces carry `generic_tag_attrib_def` attributes whose text is the generator, and
  sketch-region edges carry `sketch_attrib_def "crv_primary crv_secondary …"`. A tagged face whose
  vertices lie on exactly one profile curve's swept surface identifies that generator: 902 of
  1,138 side generators resolved this way (genmap.py).
* **"1" = start cap (at t0, the sketch plane or −d/2 when symmetric), "2" = end cap (t1)**
  (confirmed: L-Bracket caps at X = ∓55 mm; 1Wire chamfers on cap 2 lie at t1).
* Sides "3", "4", …: one per profile curve, loops in profile-record order; each outer loop walked
  **counter-clockwise about the sketch normal, starting with the curve that, in that walk, starts
  at the start point of the loop's highest-record-id curve**. Against the ACIS tags: 830 of 897
  single-outer-loop generators (92.5 %; 96 % before the nested-profile extrudes were added), 45 of
  142 for loops with holes (rule unknown there). The emitter uses the ACIS tag when the face
  survives and the rule only for hole-free profiles; otherwise the face stays unresolved.

### Fillet / chamfer edges (edges.py)
* Face geometry: caps = planes ⟂ normal at t0/t1; sides = plane through a sketch line and the
  normal, or cylinder for an arc. Edge = plane ∩ plane, cut by the planar bounding faces →
  midpoint; cap ∩ arc side → mid point of the arc piece at the cap height.
* Corpus: 591 selected edges; 227 resolved (220 straight, 7 arcs); 364 unresolved, mostly because
  a face was made by a fillet/chamfer (no model), its extrude's profile failed, or bounds are not
  planes. Evidence on the 220 straight ones: in no case does the stored body still hold the
  computed edge; 73 are positively confirmed (both set-back lines at the radius/distance exist and
  the edge is gone, or — fillets — a cylinder of the radius lies parallel at r/sin(θ/2));
  the remaining 147 cannot be checked (later features changed those parts; 104 are in Crickit).
  L-Bracket: Fillet 5 mm at sketch (−10, 10), 10 mm at (0, 0), Chamfer 5 mm at X = ±55, Z = 170
  — all match the ACIS body.
* Emitted per radius set: `{"type":"fillet","radius","edges":[{"midpoint":[mm]}],
  "edges_unresolved":n}`; 22/60 fillet sets and 15/44 chamfer sets are complete.

### Nested profiles (prof.py) — all 387 profile records now parse
`Profile := u32 nloops, loops, u32 m, m × (u32 nloops, loops)`: when the selected area spans
several sketch faces the first loops are its boundary (may be empty) and the m entries the faces.
Points are emitted per face; side generators follow the boundary loops. Box check: 219/237
new-body extrudes match (was 206/236).

### Extents
* To-object (`Side1Offset`): the first face of the extrude's `bounded_face_recipe_data` is the
  target; when it is a plane parallel to the sketch, distance = its offset along the normal +
  Side1Offset (8 extrudes).
* Through-all (kind 0, no distance; 1 in the corpus): the stored record does not say the side (its
  direction vector equals the sketch normal although the L-Bracket cut goes the other way). The
  emitter measures it: copies of the profile's straight edges in the stored body at offsets along
  the normal; one-sided offsets give side and reach (L-Bracket Extrude2: −10 mm, i.e. through the
  10 mm leg).

### Patterns, mirror, revolve axis
* Linear pattern direction: `F2A7590D` → `5662F619` → edge recipe → edge direction; the sign is
  chosen where the stored body holds the first copy of the input extrude's profile edges
  (L-Bracket R-Pattern1: +X, 30 mm, ×3 — matches the slots at X = −30, 0, 30 mm). Patterns of
  bodies/components (no feature inputs) and two-direction patterns stay unsupported.
* Mirror planes and most pattern directions reference origin planes/axes (no recipe) and the
  revolve axis is a `face_recipe` on a cylindrical face: not decoded.

## 12.0 housekeeping
* The repo copy of the prototype lacked `edgeprobe.py` (imported by edges.py, genmap.py, f3dfeat.py). Rewritten
  this round (vector helpers, `ext_geom`, `edge_recipes_of`, `input_recipes`, `body_edge_sets`, `colinear_exists`).
  Reproduces round-3 L-Bracket output: Sketch1/2, Extrude1/2 (cut, through -10), Fillet 5/10, Chamfer 5, R-Pattern1 (+X 30x3).
* Ground truth note: the L-Bracket .smb/.smbh hold many body states; `gusset --import file.f3d` shows Import1..5.

## 12.1 Holes (only ONE hole exists in the whole corpus: L-Bracket Hole1, class 1C037A07; all statements are 1-sample)
* Feature `1C037A07` params (via D91D429C -> 7A6A3D31, cm/rad): `HoleDepth` "10.00 mm", `HoleDiameter` "5.5 mm", `TipAngle` "118.0 deg".
  No counterbore/countersink/through sample exists (searched all UTF-16 strings of all 37 streams for Bore/Sink/Through: none).
  Role names for those variants are therefore UNKNOWN (guess: CBoreDiameter/CBoreDepth/CSinkDiameter/CSinkAngle).
* Position: feature -> `69EE2FA7` -> `5A1BF548` -> `994C518A` = [u32 0, 2 bytes] then **x y z f64 (cm, component/world)** at body+0
  (L-Bracket: -3.0 -1.0 15.8 = X -30, Y -10, Z 158 mm; it is point 898 `pt_tag 140` of Sketch2, which lies on Sketch2's plane).
  The rest of 994C518A is a 3x3 identity-like block + `-1 -1` sentinels: not decoded.
* Face frame: feature -> `F2A7590D`-like `5A1BF548` -> `CC54ECAD` (96 bytes): three unit vectors (9 doubles at body+1):
  **outward face normal (0,-1,0)**, u (1,0,0), v (0,0,1); n = u x v. The face is the Y=-10 mm face of the leg (the hole enters towards +Y).
* Depth 10 mm = leg thickness: the stored body shows a 5.5 mm cylinder from Y=-10 to Y=0 (cylinders at X -30/0/30, Z 151/158/165 mm).

## 12.2 L-Bracket end-to-end (confirmed)
* Tree = Sketch1, Extrude1, Fillet1 (+set 2), Chamfer1, Sketch2, Extrude2 (cut), R-Pattern1, Hole1, R-Pattern2 (3 chained
  pattern_linear: U+ , U-, V). Rebuilds to ONE body, volume 197515.929 mm3, box [-55,-45,0]..[55,0,170], 13/13 features ok, 0 skipped.
* Ground truth: gusset's .smbh importer yields Import2 = 199654.175 mm3 (state before the holes; also reproduced by my tree without
  Hole1/R-Pattern2) and Import1 = one hole tool, 237.583 mm3. 199654.175 - 9 * 237.583 = 197516.0 (mine 197515.929, 4e-7 rel).
  The STL has all nine circles of r 2.75 mm centred X in {-30,0,30}, Z in {151,158,165} on both Y=-10 and Y=0 faces (53 vertices each).

## 12.3 Pattern directions (F2A7590D) — the direction is stored as geometry, no recipe needed
* `F2A7590D` record (225 bytes in the 26 pattern-direction ones checked; the corpus also holds 245/465-byte variants used by other features, not examined): 15-byte preamble, `u32 flags` at 21 (**8** = as stored, **16** = direction
  reversed by the user; 1 sample of 16: L-Bracket R-Pattern2 V), then **point (3 f64) and vector (3 f64), cm, world** at 25, then
  `u32 kind` at 89 = 7 origin-axis ref (`90055C05`), 8 body edge (`5662F619`->recipe), 10 sketch curve (`E2CEFD18`); then refs.
  Direction = normalize(vector), sign included. Checked against the stored bodies: R-Pattern1 vector (10,0,0) -> +X (slots at -30/0/30);
  R-Pattern2 V (flag 16) vector (1,0,0) -> +X (holes at X -30/0/30). R-Pattern2 U vector (0,0,-15) -> -Z (symmetric, sign not testable).
  When not reversed the vector is the picked edge/line vector (not normalised: 10, 15); when reversed it is a unit vector.
* A sketch-curve direction is `E2CEFD18` (45 bytes): `u64 sketch data id` (body+0x1d... e.g. 454 = Sketch2) and `u64 crv_primary_id` of the
  curve (0x76 = 118 -> line 516 of Sketch2, 0x7d = 125 -> line 526). The vector in F2A7590D equals that sketch line's start->end in world.
  The `50AEE8B4` sibling repeats point + vector (and a frame). So "pattern direction = sketch line" is confirmed by id, and the geometry is already in F2A7590D.
* Two-direction (rectangular) pattern: two F2A7590D refs in the pattern record, order U then V; U <-> `countU` + `uSpaceDistance`,
  V <-> `CountV` + `vSpaceDistance` (R-Pattern2: U Z 3 x 7 mm, V X 3 x 30 mm; confirmed by the nine bores in the stored body).
* **Symmetric extent**: the pattern's `EA0E146E` record holds `ff ff ff ff` followed by 3 doubles = per-direction flags
  (U, V, ?): R-Pattern1 (0,0,0), R-Pattern2 (1,0,0). R-Pattern2 holes sit at Z 151/158/165 around the original (158) => U symmetric,
  V (X -30,0,30 from the original at -30) one-sided. 2 samples only; (the third value is unknown).
  Gusset's `pattern_linear` has no symmetric flag, so a symmetric odd-count direction is emitted as two patterns (+s and -s,
  count (n+1)/2 each, both on the originals); the V pattern takes originals + both U results. Even symmetric counts are not converted.
* Pattern tree for the L-Bracket: R-Pattern2 (U +7 x2) , R-Pattern2 U2 (spacing -7 x2), R-Pattern2 V (+X 30 x3, features Hole1 + both).
* Patterns of bodies/components (no feature inputs; every CyberDeck/MacroPad/Crickit R-Pattern, C-Pattern) are not converted; their direction
  vector is emitted as `direction` only. Crickit R-Pattern2 (countU 3, CountV 4) has ONE direction record: V direction unknown -> unsupported.
  Origin-axis directions (kind 7) decode to a point/vector like the others (e.g. (0.175,0,0)+(2.075,0,0) cm), but no stored body proves them.

## 12.4 Mirror planes (D1728651)
* Plane = `5A1BF548` -> `CC54ECAD` (96 bytes): **origin (3), x axis (3), y axis (3) as f64 cm**, normal = x cross y. Same record the hole face uses.
  8 mirrors in the corpus (BQ25185 3V/5V cases): normals (0,1,0) x6 and (1,0,0) x2, origin (0,0,0) = XZ / YZ origin planes. Input features come
  from the usual `6CCF41D5` lists (Extrude + Draft). Schema accepted by gusset (`mirror` with plane origin/normal); NOT geometry-verified
  (the case designs also hold Draft/Shell/Combine, which gusset cannot rebuild).
* Hole face frame uses the same record: origin (0,-1,0) cm, x (1,0,0), y (0,0,1) -> normal (0,-1,0), the Y=-10 mm face. (Revises 12.1: the
  three vectors are origin/x/y, not n/u/v.)

## 12.5 Revolve axis — BLOCKED
* The current corpus has no Revolve (`E3849A15`) record at all (the 2 revolves of rounds 1-3 were in 1054/1092/1WireRGBW, not available now).
  Nothing derived. Hypothesis to test when a sample returns: the axis is picked like a pattern direction (`5A1BF548` -> `E2CEFD18` sketch
  curve / `90055C05` origin axis / edge) and the world geometry sits in an `F2A7590D`-style point + vector record under the revolve; a
  `face_recipe` on a cylinder (rounds 1-3 note) would otherwise need cylinder axis recovery from the ACIS faces.

## 12.6 Whole components that rebuild
* MacroPad RP2040-Keyplate (and the identical `3a5f270b` component inside the Assembly .f3z): Board extrude + keyplate extrude, 2 sketches:
  tree rebuilds 2 bodies, volumes 3558.606 and 473.721 mm3 = stored bodies exactly, boxes equal. Fully supported.
* MacroPad RP2040-bottom-plate (`42dd9734`): 5 sketches + 5 extrudes (Board with a 4-hole cut). Tree gives Board 9706.218 = stored
  pre-cut Board (9750.608) minus the 4 stored cut tools (4 x 11.098); plates 621.612 and 605.735 exact. The stored file lists 4 copies of the
  mounting-hole body (component instances), the tree has one. Counts as supported with that caveat.
* Everything else in the Adafruit set is blocked by `Base Feature` (PCB bodies), Component Insert, Draft, Shell, Combine, Sweep, Move,
  RemoveBody, Scale or by extrude profiles/extents the prototype does not resolve (importable.py lists the first blockers per design).
* Fix this round: `ext_geom` returns t0/t1 = None when the extent is not a distance (the rewritten edgeprobe crashed Crickit otherwise).

## 13. Round 4: profile gaps, groups, circular and body patterns, Base Feature (2026-11-09)
All from the 18-design Adafruit set; "confirmed" names the check.

### Profile loops that omit a curve; closed curves cut at their start
* Crickit CPX Extrude1: the outer loop of an octagon with eight corner arcs lists 7 arcs and 8 lines (15 entries; the
  record parses exactly, the entries are in creation order, not walking order). The missing arc (`crv_primary_id` 9464,
  bottom right) exists in the sketch, as does a second arc of the same geometry (9749, drawn later). Both corner arcs at
  the other end of that edge are duplicated the same way, and the record names
  one of the pair at the bottom left and none at the bottom right. Rule used: when the loop's polylines leave exactly the open ends
  the chain cannot join, the sketch curve (not already in the loop) whose two ends sit on two open ends completes the
  loop (`geom::close_gaps`; the first in sketch order wins on ties, the geometry is identical). Reason for the omission: not
  established (hypothesis: the record is written before the sketch's last edit and one arc was re-created).
  Evidence: Crickit CPX Extrude1 now reads (it was the first blocker). Unverified by a stored body of that extrude; the
  rebuild check decides per design. **Still failing in the same design (other features, not needed by the corpus result
  because Draft/Shell/Combine/Move/RemoveBody follow)**: (a) a 5-entry loop whose top edge is line 27650 (x -0.38..0.38,
  y 4.064) plus "piece 2 of 3" of another line 19290 lying on the same y over x 0..-0.38: the piece duplicates part of
  27650 and leaves a spur the chain cannot close (dropping a polyline wholly covered by another one of the loop would close
  it; not done: the rule is a guess); (b) piece 2 of 3 of a vertical line 0.7 cm long that three stubs (T-junctions at 0.29,
  0.5, 0.71) cut into 4 pieces: which stub Fusion does not count is not established.
* A closed curve (circle) cut where it starts: the crossing is found at parameter ~0 and ~1 (`-2.4e-14`, `1.000000000000024`);
  after `rem_euclid(1)` and rounding to 9 places the two became `1.0` and `0.0` and counted as two cuts. A value of 1.0 now
  maps to 0.0. bq25185 USB Charger Extrude2 (a slot whose round end is a circle cut by the slot's arcs): "piece 1/2" now
  reproduces. Confirmed: that design now reads up to its first Base Feature.

### Timeline group `A917FE80` (the "A#FE#" blocker)
* Parent class `CB844AB6`; one record per user group in the timeline, body = list of refs (typed or plain) of the grouped
  features in timeline order, then a trailing ref to the component timeline marker `3A6D1E62`. CyberDeck HAT: the group
  (id 5561) lists 24 features; the timeline holds the group first, then exactly those 24 in the same order
  (confirmed on this one group; the Crickit Assembly has the same record). It names nothing new and creates no body (never
  a creator in a body link), so it is a non-geometric timeline entry, like CreateComponent. The features inside it are
  ordinary and are converted as before. No name string in the record (the importer shows `?`); "A#FE#" in the corpus
  summary is the class prefix with digits masked.

### Circular pattern `11F1A5CE` (one sample: Crickit CPX Assembly, "C-Pattern1")
* Parameters as for the rectangular patterns: `countU` = 12 (`A085449A` -> `7A6A3D31`), `TotalAngle` = 360 deg
  (`D91D429C` -> `7A6A3D31`, 6.2832 rad). Axis: `F2A7590D` (same record as the linear direction, the sample is a typed-ref
  stream so it sits at the same body offsets, flag 8): **point (0,0,0.175) cm and vector (0,0,2.075) cm** at body+25 / +49,
  `5A1BF548` -> `90055C05` = an origin axis, here Z. The vector's length (2.075) is the axis' display length, only the
  direction counts; the point is on the axis (Z through x=y=0).
* Input = a body, not a feature: `8DB90DAE` -> `9716F783` -> `D26351F0` (body link, creator Extrude2 (2)). A pattern's
  `6CCF41D5` feature list is empty then (this is also why every "pattern of bodies" stayed unsupported).
* **Suppressed instances**: the record lists 12 instances (`7493A99D`, 12 refs) but creates only 9 body links
  (`D26351F0` with the pattern as creator): the file has 10 bodies of 12 (CPX NeoPixels: 10). A tree pattern cannot skip
  instances, so a body pattern is converted only when made + 1 == countU x CountV; this one is not (left unsupported).
  So no circular sample converts today; the code path (`pattern_circular` with `axis`, `angle` 360, `count`, `target` or
  `features`) is exercised by the unit tests of the chain only. Partial angles are not converted (how Fusion spaces the
  copies for angle < 360 is not shown by a sample).
* Body patterns (linear): the input bodies are the `D26351F0` links directly under the `9716F783` selection records
  (reached through `EA0E146E` / `2CA5A1CD` / `8DB90DAE`). Links under the direction edge (`F2A7590D` -> `5662F619`) are
  the body the edge lies on, not an input (Crickit R-Pattern1: Extrude1's body is the direction edge's, Extrude5's body
  is the input; made = 3 = countU - 1). The tree form is `pattern_linear` with `features: []` and `target` = the creating
  feature; two directions are not chained (the second would have to copy the first's bodies).
* 5100 MacroPad `R-Pattern1` is a pattern of components: four `2D6E13A1` occurrence refs, no body selection. A pattern of
  components has no tree form (it places copies of a component), so MacroPad RP2040 stays at that feature (and at its
  Base Features after it).

### Base Feature `D7015B81` (150 records in the set, 7 designs; investigation only, not converted)
* Layout (record of 448 bytes with 3 bodies): `D3937028` refs: the PM bodies, the DM bodies, then the PM list again;
  `493A89BA` x2; one `D26351F0` body link per DM body (link = [DM body, base feature, component timeline]); `B7F34D7B`,
  `5FBE538E`, `6CCF41D5`, last the `3A6D1E62` component marker. Counts over all 150: #PM == #DM == #links; PM[k] and DM[k]
  reference each other (same order); 147 of 150 have the first block "PM bodies then DM bodies" (3 other orders); PM bodies
  carry the attribute `BASEFEAT_PM_BODY_REF` and **never** have a body link (they are not visible bodies), DM bodies carry
  `BASEFEAT_DM_BODY_REF` and are the base feature's output (the body link's creator is the base feature).
* Where the geometry is: each body record refs a body-state list `CD57BC48`. The PM bodies' list is the state before the
  base feature; it names its file through `40DCC3F4` (`BREP.<uuid>.smb`). The DM bodies' list is the state after; it names
  `74CA4562` (`BREP.<uuid>.smbh`, 142 of 142 with a PM file). That `.smbh` holds **every body of the component after the
  base feature, in list order**: NeoKey FeatherWing KAILH_SOCKET: list [Extrude body, DM1, DM2, DM3] and the `.smbh` has
  4 bodies with 8/106/24/24 vertices and boxes equal to the PM file's last three bodies; the PM `.smb` has 6 (the plate,
  two sheet bodies that the base feature consumed, three PM solids). 23 of 23 base features checked (4979, 6092, 5020): list
  length == bodies in the `.smbh` and the DM bodies are the last N of the list (the earlier ones are the component's
  bodies made by extrudes before, e.g. the "Board"-style plate). So a DM body k of N is body `len-N+k` of the `.smbh`
  (list order == file order, verified by vertex counts/boxes on the one sample opened in detail).
* Position in the timeline: 0 of the base features of 5 designs are in the timeline (they exist only in the compute-order
  list `B65B5682`: importer flag `in_timeline:false`, appended last); 2 of 6 in the Crickit CPX Assembly are timeline
  entries (followed by `RemoveBody-BodyN` features that delete their bodies). PCB-generated components (NeoKey, BQ25185,
  MacroPad, CyberDeck parts) therefore have the plate/extrude features in the timeline and the package solids as base
  feature bodies off it.
* Later features that reference the bodies: found by walking body links from every feature: only `RemoveBody` (Crickit
  Assembly), joint origins and a body pattern (MacroPad Assembly 5128, "hardware" standoffs) and one extrude of NeoKey
  Trinkey (Board Extrude2, through a `2DF7DA30` body-state snapshot listing the component's bodies, not as its operand;
  its `target` stays "Board"). No join/cut extrude of the set takes a base feature body as its target.
* What a tree-side "stored body" feature needs (not implemented; the tree format and gusset-commands are not touched):
  1. a feature, e.g. `{"type":"stored_bodies","name":"Base Feature1","component":..., "file":"BREP.<uuid>.smbh",
     "first":len-N, "count":N}`, whose bodies come from that ACIS blob's bodies `first..first+N` (the importer already
     reads these blobs: `gusset_sat::fusion` places them per component occurrence), in the **component's own
     coordinates** (the blob is stored there; `json::place` already moves tree features with the occurrence, a stored-body
     feature must take the same placement);
  2. bodies named per DM body (`<feature>`, `<feature> 2` ...) so later `target`s, patterns and RemoveBody can name them
     (a body link's creator is the base feature, so today's `target` logic already yields the base feature's name);
  3. no timeline position needed for the 5 off-timeline designs (append, no feature depends on them); the two timeline
     base features of the Crickit Assembly need the order;
  4. for the rebuild check the stored model must contain these bodies: with them as stored solids the final bodies of the
     PCB components equal the tree's (without them the body count differs, which is why the designs fall back to bodies);
  5. 8 of 150 base features have no PM `.smb` and (5020 "0805-NO") create 0 bodies: they must be accepted as empty.
  The remaining blockers of the six PCB designs after that are unknown (the first one hit is the base feature itself:
  4979, 4980, 5020, 6091 (after the profile fix), 6092, 6106).

### Corpus after this round (Rust trees, first unsupported feature per design)
3093 Crickit CPX: Chamfer3 (4 edges unresolved; further down Draft, Shell, Combine, Move, OffsetFaces, RemoveBody, Base Feature);
6091/6092/6106/4979/4980/5020 and the BQ25185 cases: Base Feature; 4863 CyberDeck HAT and Crickit CPX Assembly: Sweep1 /
Chamfer3 (past the group); MacroPad RP2040: R-Pattern1 (pattern of components); `.f3z` inner designs: Component Insert1
(`428B5C6E`, not decoded) or no timeline.

## 14. Round 5: Sweep, component instances, `.f3z` roots, bottom-plate (2026-11-09)

### Sweep `FCBB1707` (CyberDeck HAT: Sweep1, Sweep2; confirmed against the stored pin)
* Record layout (541 bytes in both samples, identical bytes except ids): after the header (body at the offset
  `parse_header` gives) the first u32 is the **operation** (4 = new body, same enum as the extrude: 1 join, 2 cut,
  3 intersect, 4 new; both samples are 4). Then `01 00 00 01 01 01` (flags, constant in the samples; the orientation
  is not among the things proved: the tree says `frenet`, which rebuilt the stored pin exactly). The parameter holders
  `D91D429C` (refs, each with a `7A6A3D31`): `AlongDistance`, `AgainstDistance`, `AlongRailDistance`,
  `AgainstRailDistance` (all 1, unitless), `TwistAngle`, `TaperAngle` (0 deg). Twist and taper are not converted (never non-zero in a sample).
* Profile = a `4BD53E5A` ref (as for extrudes; its wide string names the sketch data id) with its `0D57BD2F` region
  record: the profile is **one loop** of a sketch that may hold several (Sketch2 holds two squares and more).
* Path = one `5A1BF548` ref per path curve, each pointing to an `E2CEFD18` (45 bytes, the same record as a sketch-curve
  pattern direction): **u64 sketch data id at +0x21, u64 `crv_primary_id` at +0x29**. The `crv_primary_id` is the
  curve's attribute (not its object id); the tree's `cNNNN` ids are object ids, so the primary id is looked up among the
  sketch's curves (`sketch::curve_by_primary`). Sweep1: sketch 3466 (a sketch of the component `header`, the sweep
  belongs to component `Pins`), curves 127 and 129 = two lines, 2.56 mm down then 9 mm at 30 degrees.
* Which end of the path starts: Fusion does not require the profile on the path. The pin's profile sketch lies in z = 0,
  the path starts in z = 0.5 and 1.27 mm to the side (x = 0 vs the profile's x = 1.27): the stored pin starts at
  z = 0.5 (the profile moved along its normal to the plane through the path start, x and y kept, the offset to the path
  kept all the way) and ends exactly at the path's end (vertices (-3.507, -9.694) and (-2.953, -10.014) around the end
  point (-3.23, -9.854)). Gusset's sweep needs the path to start on the profile's plane, so the reader writes a copy of
  the profile sketch (only the loop's curves) moved along its normal onto the plane through the nearer end of the path.
  Checked: Sweep1 rebuilt in Gusset = 4.735 mm3, box (0.95..1.59, -3.51..1.59, -10.01..0.50) mm = the stored
  pin body of the `Pins` blob (16-vertex state, before the fillet).
* Not finished: CyberDeck HAT then stops at Fillet1 (1 + 1 edges unresolved: the faces are swept faces, `naming::face`
  knows extrude faces only) and, beyond, at six Base Features. So no design is gained by the sweep alone.

### A component with several occurrences (MacroPad bottom-plate: four `MOUNTINGHOLE_3.0_PLATEDTHIN:U$1..4`)
* One blob (`BREP.<id>.smb`), four occurrence placements. The features of such a component are written once per
  placement (names of the copies " [2]", " [3]", ...; the references between a component's features follow the
  renaming; each copy right behind the original in the timeline), then every copy is moved by its occurrence.
  Different blobs of one component placed in different ways stay refused.
* The design then fails the importer's step check, not the reader: stored solids are Board 9750.608 mm3 (the board
  **before** its Extrude4 cut), four holes' cut tools of 11.098 mm3 each, four mounting holes of 1.936, the front plate
  621.612 and the back plate 605.735; the tree gives 10941.31 mm3, the stored 11030.09 (= the tree + the uncut board's
  44.39 + the four tools 44.39). `fusion_states_check` (gusset-commands, `import.rs`) compares each stored solid to the
  **total** volume of the model after a feature, which only works for a design with one body at a time; here the new
  bodies of the other components (mounting holes, plates) are states of single bodies, and the four tools of one cut are four
  solids of one feature (`fusion_instances` counts a cut extrude as 1 copy). Needed there (not done, other agent's crate):
  per-body states (the volume of the body the feature changed) and a copy count of the profile's loops for a cut extrude.

### `.f3z` archives (6 files: the "no feature history" cases)
* `Manifest.json` names the root design (`{"root": "<uuid>.f3d"}`); `DesignDescription.json` lists the others as XREFs.
  Root designs: Crickit HAT (39 timeline entries, 1 Component Insert, 24 Base Feature records), CyberDeck Bonnet (14, 1, 6),
  NeoKey Breakout (12, 1, 4; also a second, empty stream of the same design), MacroPad Kit assembly (4 entries, 4 inserts, 1 base),
  the two cases 6124/6126 (4 timeline entries holding 61 listed features, 1 insert, no Base Feature). The referenced designs
  carry the PCB (Base Features) or the other parts.
* `fusion_history` gives `feature_tree` the entries of **all** inner designs, so `pick_stream` finds several streams. Mapping to
  write (not done, import.rs belongs to another agent): take the root design's entries only (those whose path starts
  with the `Manifest.json` root) and, if the root has no timeline, the single inner design that has one. Every root
  above has a timeline, so the root rule alone covers the 6 files; the placements of the referenced designs are what
  `import.rs` already derives from the root (`Use`/`Xref`).
* `Component Insert` (`428B5C6E`, 434 bytes in the cases; only looked at, not decoded): after the header there are 12 doubles
  that read as a rotation and translation (not proved), two refs to the same `2D6E13A1` occurrence record, a nil design
  UUID string, the wide string "Component Insert" and the usual `B7F34D7B` / `5FBE538E` / `6CCF41D5` / `3A6D1E62` tail. The
  referenced design is named by the occurrence, not by the record.
  The root's features after it (shell, fillets, extrudes: 61 entries in 6124) act on the root's own case bodies and on the
  inserted PCB; the inserted bodies exist only in the referenced design's blobs, so the tree needs the same stored-bodies
  feature as the Base Features to rebuild the stored model. No `.f3z` can come in as history before that.

### Crickit CPX chain after Chamfer3 (documented, not worked)
* Chamfer3 (4 edges) is followed by a Combine, a pattern of components (R-Pattern2), a Shell, eight Drafts, three
  OffsetFaces, two Moves, two Base Features, thirteen RemoveBody and further blends with unresolved edges (Chamfer4 2,
  Fillet2 1, Chamfer5 2) and **nine extrudes the layout search does not recognise** (Extrude13, 15, 18, 26, 28, 32, 33,
  37, 39). Resolving Chamfer3 alone gains nothing; the Assembly shows the same chain.
