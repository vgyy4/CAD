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

* Start off the profile plane (`ProfileOffset`, 68 extrudes in adafruit-more): with no face recipe among
  the extrude's recipes it is an offset plane at that distance along the normal (signed like
  `AlongDistance`); with one `BoundedFace`/`Face` recipe (and no `Side1Offset` end) the start is that face's
  plane plus the offset ("from object"; every such offset in the corpus is 0). 3464 Joy Bonnet: Extrude2-8
  start on the board's top or on the cap of an earlier extrude, and each then adds exactly its stored tool.
  The cap offsets that name faces later include the start. Written as an extrude from a copy of its
  sketch moved to the start (`<name> start`), like an extrude to two sides. A start with two face
  recipes (start and to-object end; which is which is not known) or a face that is not a plane parallel
  to the sketch is refused.
* OffsetFaces `D087EFE5`: the faces are `7ACC2A03` face recipes under its selection (planar caps and sides),
  the `distance` parameter positive away from the material. 3464's OffsetFaces3 (-1.437 mm on the two
  long sides of the header) gives the volume of the file's end body (6406.567 mm3).

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

## 15. Round 7: sketch points, `.smbh` unit, circle pieces, Component Insert (2026-11-09)
All from the 18-design Adafruit set; each statement names its check.

### NeoKey sketches had 21 of 93 circles (4979, 4980, 5020)
* The sketch lists all its entities (Sketch3 of 4979: 102 points, 93 circles, 8 lines in one reference list); the reader
  dropped most circles because their **centre point is dropped**: a point record's z is exactly 0 in some points
  and rounding noise (`-1.1e-17`) in others (the points of circles created by a transformed copy). `point()`
  required z == 0. A z below 1e-9 is on the plane now (z set to 0). Confirmed: Extrude3 reads 95 of 95 profile
  regions and 4979, 4980 come in as history (45 / 111 stored solids are states, 95 / 112 cut tools).
* NeoKey Trinkey then stopped at its 0805-NO Base Feature ("makes no body", two stored 2.4961 mm3 bodies missing): the body
  link `D26351F0` of that feature starts with a reference to an object of class `40DCC3F4` (the blob-name holder, object
  10) before the creator. `Graph::new` took the first reference that is not a body/component record as the creator.
  `40DCC3F4` is skipped (the only link of the set that has one). 5020 comes in as history.

### The `.smbh` header unit is not a length unit (6092, 6106: the "5x" bodies)
* The first header double of every `.smb` is 10 (centimetres to millimetres). An `.smbh` carries **10 x something**: 10
  (6091), 20 (the NeoKeys, Crickit HAT, MacroPad), 30 (Crickit CPX, keyplate, bottom-plate), 40 (Crickit Assembly), 50
  (CyberDeck, NeoKey Breakout, BQ25185 3V Buck / 5V / Cases). Constant inside one design. Not a scale: all 46 DM bodies of
  6092's `.smbh` blobs have exactly the raw vertex box of a pre-feature body of an `.smb` (same numbers), and the
  component's final bodies agree with the raw numbers read as centimetres (0402 pad body 0.46 x 0.5 x 0.48 mm = 0.11 mm3
  vs 13.8 mm3 at unit 50). The body records carry identity transforms (`transform` scale 1). What the 20..50 counts is
  not established (no sample property tracks it). `to_step_nth` (the `.smbh` reader) uses 10 whatever the header says.
  The SolidWorks/Inventor paths are untouched.

### Circle pieces: piece 1 holds the start (6092, 6106)
* 6092 Extrude2 (cut) has four slots (a pill outline of 2 lines and 4 quarter arcs) with a circle of the slot's width in the
  middle, tangent to both lines. The profile group lists the slot outline plus **three child regions**: the disc, the left
  part and the right part. The circle is cut at its two tangent points (top, bottom) into 2 pieces, the lines into 2, and the
  left part names "circle piece 2 of 2", the right part "piece 1 of 2". The circle starts at its rightmost point,
  so **the piece that holds the curve's start is piece 1**, the other follows in parameter order. The reader numbered the
  pieces by their first cut (piece 1 = from the first cut to the second), the opposite, and drew each side part
  with the wrong half circle; both rings close, so nothing complained; the interior point of each side part fell in the
  disc, the three picks of a slot named one region, `Sketch2.regions(...)` held 53 ids for 60 picks and the cut removed 2.684
  mm3 less than the stored tools (the slots' tool is the whole pill: 1.0091 / 1.4801 mm3).
  Rule now (`geom::piece`, closed curves): cuts in (0, 1] (a cut at the start counts as 1), piece k ends at the k-th cut; piece
  1 starts at the last cut of the turn before. For a cut exactly at the start this is the old numbering (the round 4 case,
  USB Charger Extrude2, still reproduces), so both samples agree.
* The two unit tests `a_closed_curve_cut_at_its_start_...` and `a_closed_curve_cut_off_its_start_...` pin the two cases.

### Corpus after this round
8 of 18 as history (3 before): MacroPad keyplate, bottom-plate, bq25185 USB Charger (6091), NeoKey FeatherWing (4979),
1x4 QT (4980), Trinkey (5020), BQ25185 3V Buck (6092), BQ25185 5V (6106). Blockers of the rest, unchanged: Crickit CPX and
Assembly -> Chamfer3; CyberDeck HAT -> Fillet1 (swept faces); MacroPad RP2040 -> R-Pattern1 (pattern of components);
Crickit HAT, CyberDeck Bonnet, NeoKey Breakout, BQ25185 Cases -> Component Insert.
`fusion_states_check` still rebuilds every prefix from scratch for each feature that changes a body (6091 about 5 minutes,
the corpus about 25): the per-feature volumes it needs are what `tree_import::apply` already evaluates after each feature
(`Gen::evaluate`), so a hook there (a callback with the session after feature k) would give all states in one pass; the
check itself would not change. `tree_import.rs` is outside this round's files, so it is not done.

## 16. Round 8: Component Insert merged (2026-11-09)
Done in the reader (`gusset-sat`, `fusion_tree/insert.rs`), not in `fusion_history`: the reader already has the design
stream, and the archive's other designs only have to be handed over.

* API: `feature_tree_with_inserts(files, name, resolve)`; `resolve(keys)` gets the keys of an insert (lowercase UUID texts of
  its occurrence) and returns the entries (streams and blobs) of the inserted design or `None`. `feature_tree` is the same with
  a resolver that gives none, so a lone `.f3d` still stops at its Component Insert ("the design it inserts is not merged").
  `fusion_history` reads `Manifest.json` and `DesignDescription.json` (`xref_files`), resolves a key to the archive's `.f3d`
  (exact key, else the same first 36 characters) and adds the inserted designs' blobs to the entries that
  `fusion_stored_bodies` searches.
* The insert's place in the tree is taken by the inserted design's own checked, component-placed tree (recursively; depth
  limit 4), every feature moved by the insert's placement (`json::move_feature`, the function `place` uses for components:
  sketch planes, edge midpoints, hole faces and positions, mirror and pattern axes; stored-body placements are composed with
  the one they already have) and renamed `<name> [<insert name>]` (every reference to a renamed feature follows). A moved
  insert whose tree has a kind `move_feature` does not move (sweep, ...) is refused.
* A design whose tree cannot be read (no timeline, no extrude, an unsupported feature) is inserted as `stored_body` features,
  one per solid body of its `.smb` blobs (sheets are skipped like the import skips them), `note` says why. The rebuild/state
  check then decides: the history counts only if every stored solid of the root is explained.
* Cost bound: `fusion_states_check` counted every non-sketch, non-stored feature against its 80-feature limit; new-body
  extrudes and sweeps are built in runs (one build for a run), so only the features that change a body count now. Crickit HAT
  has 85 new-body extrudes after instance replication. The checks themselves are unchanged.

### Corpus (six `.f3z`)
| archive | before | after |
|---|---|---|
| NeoKey Breakout (4978) | Component Insert | **history** (27 stored solids are states, 27 cut tools) |
| CyberDeck Bonnet (4862) | Component Insert | **history** (68 states, 84 cut tools; the header is inserted as stored bodies) |
| Crickit HAT (3957) | Component Insert | bodies: 221 stored solids (0.9988, 1.2331, 12.6267, 9.667 mm3 ...) are neither states of the model nor cut tools; the model has none of the 1.2331 mm3 bodies (55 stored). Not caused by the insert: the inserted header (one 1030.5 mm3 solid) is a state. |
| BQ25185 3V Buck Case (6124) | Component Insert | bodies: the inserted PCB design rebuilds inside; stops at the root's Shell1 |
| BQ25185 5V Buck Case (6126) | Component Insert | bodies: same, Shell1 (then Draft, Combine, Move, Scale, RemoveBody) |
| MacroPad RP2040-Assembly (5128) | no history | bodies: its inserts now resolve, so the reason is the inserted MacroPad's R-Pattern (no feature history of its own before) |

## 17. Round 9: Shell, Combine, insert targets, Crickit HAT's 221 solids (2026-11-09)
Helper: `scripts/re/fusion/tl.py` (timeline listing, `skel` record skeletons, `tree` record walks).

### Shell `E8063467` (decoded; not yet rebuilt end to end)
* Wall: parameter `innerThickness` (via the `D91D429C` holder, centimetres) = Gusset's `inside`; `outerThickness` would be
  `outside` (no sample has it). The removed faces are `7ACC2A03` recipes under `2CA5A1CD` -> `5662F619`; in 6124/6126 the
  recipe is `bounded_face_recipe_data`: its **first face name is the face itself, the others bound it** (as for the To-object
  extent). Resolved like fillet faces (`Ctx::face`): Shell1 of 6124 removes cap "2" of Extrude2 (the 14 mm case body's
  open top, point (0, 0, 14) mm) with a 1.6 mm wall. Emitted as `{"type":"shell","faces":[{"point"}],"thickness","side"}`
  without normals (cap and side normals from the names are not oriented outward). 12 further Shells live in the Crickit
  designs (not reached).
* NOT verified by a rebuild: the check never got past an earlier feature in 6124/6126. The earlier report "stops at Shell1"
  was a tree-level statement (the reader's `check`); no rebuild of the root was ever run. Rebuilding up to Shell1 shows
  Fillet1 failing ("no edge has its midpoint at [-17, -12, 7]"): see below.

### Why 6124/6126 are far from history (full blocker list, 6124)
Fillet2 (4 edges unresolved), Chamfer1 (8), Chamfer1 of Top (2), Fillet4 (16), Chamfer2 of Top (8), Extrude4 of Top (profile
is a face, not a sketch), Extrude16/17 (profile regions of the logo sketches), Draft1 x2, Scale5, Move3-5, Combine1,
RemoveBody. Tree features that exist: `shell`, `combine`, `mirror`. Missing: draft, scale/move of a body (the tree has
no body transform), remove-body. RemoveBody-Body51 is the tool removal of Combine1 (Combine refs the `4A782808` record);
it leaves no trace only if the combine consumes its tool, so it could be dropped together with a Combine that does.
* **Sketch construction lines are read as profile lines.** Sketch1 of Bottom (34 x 24 mm box) has two extra lines
  (-17,0)-(0,0)-(0,12) whose line records differ from the box lines in one byte (tail of the `DCA267ED` section: the byte
  3 after the `01` that follows the second reference is 1 instead of 0; layout of the 307-byte records only, other lengths
  not checked). They split the rectangle in the importer's profile search and the chosen profile point (-4.25, 0) lies ON
  such a line. The body is then not the box: Fillet1's edge midpoints (+-17, +-12, 7) miss by 8.46 mm. This (not Shell)
  is the first thing to fix for these two designs; a `construction` flag per line would also remove the "outer loops"
  ambiguities seen elsewhere. Not done: needs the flag position for every line/arc/circle record length.
* Combine `2A94257F`: no parameter holder. Refs: `4A782808` (the RemoveBody of the tool), `2E3AC990` (flags `00 01 00 01` +
  back reference), then selection lists. **Tools = the first `2CA5A1CD` list(s) (`u32 n` + n selections, each ->
  `9716F783` -> body link `D26351F0`), target = the last single selection** (6124: tool body of Extrude17, target
  body 1796 = the Shell1 body; the same link 1796 appears in the shell's selection). The operation (join/cut/intersect) is
  NOT found: the eight Combines of the corpus differ only in u64 ids and counts (0x219, 0x150, 0x181, 0x1f2 after the
  first section: not an enum), `2E3AC990` is identical in all four samples that have one. Not emitted as a tree
  feature for that reason (a guessed operation would be a guess).

### Component Insert targets
Implemented in `insert.rs` (`retarget`, `final_bodies`): after a merge, a later feature whose `target` names the insert
now names the bodies the inserted features leave (one name, else a list that the check refuses). No sample reaches it:
in the six `.f3z` every root cut/join has a one-feature target (6124/6126: Extrude5 "cut Extrude2"), so this is
covered only by a unit test.

### Crickit HAT (3957): the 221 solids are the hole tools of the Board's Extrude2
* The Board's blob (`BREP.cd166dcd...smb`) has 230 bodies: 222 solids (the 5551.7171 mm3 uncut board and **221 cylinders
  of height 1.57 mm** with r = 0.25 (109), 0.35 (2), 0.4 (2), 0.45 (45), 0.5 (55), 1.4 (4), 1.6 (4): the volumes
  0.3083 / 0.6042 / 0.7892 / 0.9988 / 1.2331 / 9.6673 / 12.6267 are exactly 1.57 pi r^2) and 8 sheets. Sketch2 of the Board
  has exactly these 221 circles and Extrude2 (cut, 1.57 mm, all 221 profiles read) uses them: the "1.2331 mm3 bodies the
  model lacks" are the 55 holes of r = 0.5 mm.
* The model cuts only 233.335 mm3 of 238.334 (5 of the 45 r = 0.45 tools remove nothing: they lie outside the board
  material), so `fusion_explain_steps` finds no feature whose delta is a stored tool times its copies. The loose rule
  (sum of the uncovered tools) fails on the same 4.9988 mm3 (5 x 0.9988) and is not loosened.
* The **cut board (5318.3819 mm3) is not stored anywhere** in the file (stored solids: only the uncut 5551.7171). Total:
  stored 13330.6143 - model 12858.9504 = 471.664 = 233.335 (uncut vs cut board) + 238.334 (tools). So the file stores the
  history state of the cut (board + tools) but not its result, or stores it as a body the reader drops (the 8 sheets are
  profile faces: bodies 221/222 have 221 vertices at z = 0, 224/225/228/229 are the outline faces, 226/227 two-vertex
  pieces; none is a solid with holes). Whether the final design has the holes cannot be told from the file here.
* All features of the Crickit root are `in_timeline: false` (no captured timeline), so which of them are part of the final
  design is itself uncertain. Next step if resumed: decide, from the `B65B5682` compute order and the `SpecifiedColor`
  attribute that only the cut carries, whether Extrude2 is applied; then the check could count the 5 outside tools
  (they would need a "tool that removes nothing" rule, i.e. a loosening that needs the owner's decision).

### Corpus after this round
Unchanged: 10 of 18 as history.

## 18. Round 10: construction curves, shelled faces, Case chain (2026-11-09)
### Construction flag of sketch curves
* One byte after the curve's last point reference (the 2nd of a line, the 3rd of an arc/circle), at an offset that depends on
  the record layout. Checked against the profiles (a curve of a sketch that has profiles; `(primary, secondary)` ids in the
  `0D57BD2F` loops): every flagged curve lies outside all profiles of its sketch, and no other byte of the record has that
  property. Layouts: line 303 and 307 bytes: +7 (24 + 49 flagged, 0 used); line 396: +2 (72 flagged, 0 used); arc 471: +2
  (one flagged arc, 0 used; the pair +6/+7 is a different flag that also occurs on used lines, so it is not construction).
  Layouts 413, 417, 440, 444, 447, 448, 496, 500 (lines) and the other arc layouts hold no flagged curve in the sample set
  and are read as "not construction". The reader drops construction curves from the sketch (`sketch::construction`).
* Effect: Sketch1 of 6124's Bottom component is the plain 34 x 24 mm box now; Extrude2 = 11424 mm3, which is a stored solid.
  Fillet1 (11231.7168) and Shell1 (3349.0916 mm3 = the hand-computed shell of the filleted box) rebuild. No design that is
  history changed (10 of 18).

### Shelled faces
A face name `[extrude op, shell op]` ([321, 324] in 6124) is the extrude's face moved inward by the shell's `innerThickness`
(inward = the side of the mean of the profile vertices at mid height); `[321]` stays the outer wall. With this, Fillet2's 4
edges (pocket corners cut through the 1.6 mm wall, bounded by the outer and the inner wall) resolve and Fillet2 rebuilds
(+3.5 mm3, concave corners). Later ops that are not shells leave the face where it was (as before).

### Where 6124/6126 stop now
Chamfer1 (8 edges): its faces are Fillet2's. **A fillet's generator k is the face of the k-th edge of its edge list** (Fillet2:
g1..g4 = the corners (g3,g4), (g3,g6), (g5,g6), (g4,g5) of Extrude5's pocket; confirmed on all 8 Chamfer1 edges: 4 are
arcs "fillet face x outer wall" bounded by the two pocket sides, 4 are straight "outer wall x pocket side" bounded by two
fillet faces). Not implemented: fillet faces need a cylinder face kind (arc mid point = corner + r (a + b)(1 - 1/sqrt 2), a, b the
unit normals of the two sides toward the void; straight edge mid point = mid point of the two corners). After the Chamfers
come Draft1 (no tree feature), Fillet3/4 (16 edges), the Top component's blends and Extrude4 (face profile), the logo
Extrude16/17 (profile regions), Scale5, Move3-5, Combine1, RemoveBody. So neither Case can become history with the
current tree; what a tree would need: `draft`, a body `transform` (scale + move), `remove_body`, and a combine operation.

### Combine operation: not found
Compared all eight Combine records (3093 CPX x2, CPX Assembly x2, Bonnet, CyberDeck HAT, 6124, 6126): the same fields in
the same order; they differ only in ids, tool counts and the u64 after the first block (0x219, 0x150, 0x181, 0x1f2 ... =
not an enum). The `2E3AC990` records are identical in all four that have one. No sample has a Combine whose operation is
known (the Bonnet's and the CyberDeck HAT's are in unreadable inserted designs; the rebuild of the Cases stops before
theirs), so a join/cut/intersect byte cannot be told apart by comparison. Next evidence needed: a Combine whose
result is a stored state (volume before/after).

## 19. Round 11: Move, Scale, RemoveBody, Draft, fillet faces (2026-11-10)
Reader: `fusion_tree/bodyops.rs`; tree types `move_body`, `scale_body`, `remove_body`, `draft`.

* **Selection**: a `2CA5A1CD` list -> `9716F783` -> `7ACC2A03` with `body_recipe_data` + the `D26351F0` body link; the
  body keeps the tree name of the feature that made it. A face recipe there would be a face move (not read). All Move,
  Scale and RemoveBody records of the corpus select bodies.
* **Move `64E40C8E`**: the first ref is an `84E6245F` record holding a row-major 4x4 matrix (16 doubles, translation in the
  last column, cm, last row exactly 0 0 0 1) at a record-dependent offset (found by that row). Only translations are
  emitted: no sample has a rotation, so its convention is unverified; a body created by the Move (copy) is refused.
* **Scale `D45FD6C4`**: parameter `ScaleFactor` (uniform; the only parameter in the samples), centre = the point of the
  `69EE2FA7` selection (a vertex, via `5662F619` -> `994C518A`).
* **Verified against the stored logo bodies** (vertex boxes of the `.smb`; original Extrude16 box
  [-22.365,-23.527,0]..[22.385,19.053,20] mm): 6124: Scale5 0.2 about (0.0064,4.9531,10), Move3 (-0.0064,-4.9531,18),
  Move4 (-0.0772,0.8494,0), Move5 (0,0,0.4) give [-4.551,-4.846,26.4]..[4.399,3.669,30.4], the stored box; 6126: Scale5 0.2,
  Scale6 0.8, Move3 (0,0,-16), Move4 (0,-4.7484,4.0336), Move5 (0,0,0.6) give [-3.573,-4.352,-2.966]..[3.587,2.461,0.234],
  the stored box. (Volume not checked: the logo extrudes are not rebuilt.)
* **Draft `F4163A07`**: `Angle1` (`Angle2` = 0), a list of bounded-face recipes (the faces) and a list with one plain face
  recipe (the neutral plane). Emitted with the plane face's outward normal as pull direction and `flip` false; both are
  unverified (no stored state after a Draft is reachable). The Crickit drafts name faces that are not extrude faces.
* **RemoveBody `4A782808`** (an off-timeline extra): same body selection; in the Cases it removes the Combine's tool.
* **Combine operation: still not found.** No Combine's result is a stored state: the Cases stop before it, and the Cases'
  stored solids hold no body that follows a Combine.
* **Fillet faces**: generator k of a fillet op is the face of its k-th edge recipe (set order); the face is a rolling ball of
  the set's radius between the two planar sides of that edge (centre r inside both sides, on the profile-interior side).
  An arc edge "fillet x wall" has its middle r from the centre along minus the bisector; a straight edge "wall x side"
  between two fillets has the middle of the two tangent points. Chamfer1 of 6124 (8 edges) now builds in the rebuild
  (every edge midpoint is found on the model: 3382.7765 -> 3369.5028 mm3; no stored solid holds that state). Also resolved:
  Top's Chamfer1(2) (4 edges, the boss base at z = 15.6) and Chamfer2.
* Left in 6124: Fillet4 (4 of 13 edges, faces of multi-op names [374, 376, 377]), Top's Extrude4 (profile is a face),
  the logo Extrude16/17 (spline-like sketch entities `D82E012F`, `FF23079A`), Combine1. 6126 also has OffsetFaces1 and
  RemoveInstance1.

## 20. Round 12: patterns of components, tools in voids, joints, text (2026-11-10)
Reader: `fusion_tree/occpattern.rs`; importer: `fusion_explain_steps` / `fusion_states_check` in gusset-commands `import.rs`.

### Rectangular pattern of components (MacroPad RP2040 5100: R-Pattern1 of the `M3 SMT8mm` standoff; confirmed)
* The `E8816367` record lists the original occurrence (`2D6E13A1`) first, then one occurrence per copy, each followed in
  the record by a 4x4 matrix; the list is repeated near the end with the parameter holders: `countU`, `CountV`
  (`A085449A` holders), `uSpaceDistance`, `vSpaceDistance` (`D91D429C`, cm), one `F2A7590D` direction (U) and a
  `5A1BF548` reference (the V direction, not decoded). 5100: 2 x 2, 52.07 mm along +x, -81.28 mm.
* An occurrence's first `CE2913AA` record names its component (`E03784ED`) and, for a copy, holds the copy's placement in
  the parent component (row-major 4x4, cm; 5100: (5.207, 0, 0), (0, -8.128, 0), (5.207, -8.128, 0)). The original's
  record has no matrix. The block-layout stream scanner (`fusion::read_design`) never saw these copies: the body import
  placed **one** standoff.
* Rule: copy placement = (original occurrence's placement) then (copy matrix). Read only when the copies are exactly the
  lattice of the parameters (countU x CountV occurrences, i * uSpaceDistance along the stored direction plus j steps of
  one vector of length |vSpaceDistance|, no rotation); otherwise the pattern stays unsupported and names why.
* Independent check: the `standoffs` component's Sketch1 has four r = 2.125 mm circles at (3.81, 19.05), (55.88, 19.05),
  (3.81, 100.33), (55.88, 100.33) (component coordinates), the Board (same placement) has four r = 2.125 holes there, and
  the original standoff stands on (3.81, 100.33): the three copies land on the other three. 5100 now comes in as history.
* Not covered: a pattern of an inserted design's occurrence (5128 Assembly R-Pattern1 of 12 switches: the occurrence's
  `CE2913AA` names no component of this design) and Crickit CPX Assembly's (the repeated component lists no body).

### Crickit HAT (3957): the five r = 0.45 tools that remove nothing
* The Board's Sketch1 outline has five inner loops: milled slots around (2.48, 8.27), (2.48, 15.47), (7.60, 42.67),
  (11.00, 37.87) and (13.85, 42.67). The five r = 0.45 hole circles of Sketch2 that cut nothing are exactly at those
  centres (`c1097`, `c1100`, `c1145`, `c1148`, `c1151`): plated slots drawn as a hole plus a slot. They cut nothing in
  Fusion either (its board already has the slots: the stored uncut board is the rebuilt one, 5551.7171 mm3).
* The step check now measures, for a loose cut whose tools do not add up to its volume change, the part of each stored
  tool inside the bodies the feature changes (their common volume before the feature) and accepts only when those parts
  add up exactly: the 221 tools pass (positions included). Nothing is loosened: a tool in the wrong place now fails too.
* 3957 then fails the box check: the inserted 2x20 header (`92c70d58`, 1030.5448 mm3) is placed by the Component Insert's
  matrix (y -9.81..-4.73) in the tree and not placed at all by the body import. The root's **Assemble1** (`8DFDD543`, a
  joint) moves it afterwards: two joint frames (cm: one turned, origin (4.858, 0, -0.1); one at the board hole
  (5.663, 5.123, 0.157)) and the parameters `alignAngle` 90 deg, `alignOffsetX` 0.3 mm, `alignOffsetY` 0, `alignOffsetZ`
  -1.5 mm, plus a motion record (`OffsetX/Y/Z`, `AngleZ`, all 0). How these combine is not established: the reading
  "frame on the header -> 90 deg -> offset -> frame at the hole" puts the pins 0.02 mm off the hole grid in x (one sample, no
  stored final placement), so it is not used. Joints also govern 5128 Assembly (Assemble1..8).

### Cases 6124/6126: Top's Extrude4 (2) / Extrude5 (2) cut **text**
* The "face profile" is a sketch text: Sketch3 (2) holds three `E0618268` records with the font name and the text
  ("Arial Rounded MT Bold": "GND", "4.5V", "3V") and a `5275CBA4`; the extrude's profile records (`92F9A5F4`) name the
  sketch (3834), no curves. The glyph outlines are not in the file (they come from the font), so these extrudes cannot be
  rebuilt; both Cases stay bodies whatever else is solved.

## 21. Round 13: the 150-design corpus (2026-11-10)
Worked from `docs/formats/fusion-corpus-tally.md` (150 further public Adafruit designs): the commonest first blockers, each
fixed in the reader. Counts are over that set unless said otherwise.

### Profile loops (the 15 "loop does not close" and 4 "curve piece not reproduced" designs)
* **"Piece a of b" counts the cuts by the curves of the same loop**, not by all profile curves of the sketch (section 10
  had it as the latter): line 1793 of 3464 Joy Bonnet is "2/2" in the board outline (cut by one loop neighbour) and "2/3"
  in a small rectangle (cut by that loop's two sides), and four curves cross it in all. **The ends of a line that lies on
  the line cut it too** (parallel lines do not cross). Checked on every piece entry the reader reaches: the cuts by the
  loop's own curves give exactly `b` pieces for 646 of 647 open pieces and 175 of 178 closed ones (without the lying-on
  ends 581; by all profile curves 259; by all sketch curves 569). The old orders (profile curves, all curves, loop
  neighbours) remain as fallbacks, the unchecked neighbour trim last.
* **A loop lists every sketch curve on its boundary**: where a rectangle is drawn on a side of another, the shared edge
  is listed twice (805 slide switch: both rectangles' bottom lines over the same span). A polyline lying wholly on another
  polyline of the loop is left out of the ring (`geom::drop_covered`; of two equal ones the first stays).
* **A hole joined to the outline is walked in the outline's loop**: 610 DC Jack lists, in one loop flagged outer, a
  rectangle, a line from its bottom edge up to a circle, and the circle. The ring is the rectangle, the circle a hole of
  the region (`geom::bridged`: open polylines with a free end are the curves walked both ways; each closed curve must hang
  on one or touch the outline, and lie inside it). The same reading covers a circle tangent to an edge of the outline
  (a D shape with a circle touching its chord).
* A cut of a circle sampled as 64 chords lies on a chord, up to the chord's sag off the circle (6299: 0.00027 cm), which
  the ring join's tolerance did not allow: the join now allows the sag of the curved polylines it joins.
* Newer streams (2025 files) write **64-bit hashes as `crv_primary_id`** in the profile entries (`30 d4 3c 03 da d8 9e 45`);
  the entry layout is unchanged. The reader bounded the id at 2^40 and reported "0D57BD2F layout" (7 designs).
* Left: two coincident circles in one loop (their sampled polylines cross everywhere; 2 designs), a circle with pieces
  1/2 and 2/2 against another circle (1), a line "2/3" that nothing of its loop touches (1).

### `8DBAF917` (the unnamed "DBAF" feature, 7 designs, 319 records)
Its parent in the class table is `54F9ACE8` (component activation); the record names an occurrence `2D6E13A1` and its
CreateComponent; no body record and no body link names one (0 of 319). A non-geometric timeline entry like its parent.

### Extrudes to two sides (`extent` 2, `AgainstDistance`)
The solid lies between `-AgainstDistance` and `AlongDistance` along the extrude direction; a negative `AgainstDistance`
("-3.00 mm" with Along 4 mm: 878, 1501) starts it off the sketch plane on the first side. The tree has one distance, so
the extrude is written from a copy of its sketch moved along the normal to the second end (`<name> start`), over the
length between the ends. The rebuild check decides per design (the naming code already used these two ends).

### Mirror of bodies (`D1728651` without feature inputs)
The selected bodies are reached like a body pattern's (`2CA5A1CD`/`8DB90DAE` -> `9716F783` -> body link); the mirror is the
creator of one new body link per selected body. Written as a tree `mirror` with `target` and `merge: false`, one per
body; a mirror that makes a different number of bodies stays unsupported. (10 designs have one; "2x12 short female" is a
library part with two.)

### Face names are in the stored bodies (the general way to an edge)
* A `generic_tag_attrib_def` attribute of an ACIS face holds the **whole face name**, not only the generator: after the
  attribute's second name string come `1`, the entry count, then per entry the generator text, a number, the count of
  operation ids, the ids and `0`, i.e. the entry layout of a `7ACC2A03` recipe face (keyplate: `'6' 0 1 313 0`).
  The reader used the generator text alone (and so could mix faces of different operations).
* So an edge recipe can be looked up: an edge of a stored body whose two faces carry exactly the recipe's first two
  names is that edge, whatever made the faces (sweep, revolve, pattern copy, chamfer). Its middle comes from the edge's
  curve (`straight`: point + t * direction; `ellipse`: centre, normal, major axis, ratio) at the mean of the two stored
  parameters, taken only when the parameters reproduce both end vertices (with either sense). Several stored edges of
  one pair of names must agree on the middle, or exactly one must have the recipe's further faces at its ends.
  Of the 3217 edge recipes of the set 1243 (39 %) have such a stored edge; of 1631 in the 18-design set, 404.
* A face of a recipe that the model does not explain is taken from the stored bodies when a **planar** stored face has
  the name, alone or followed only by operations that trim a face without moving it (extrude, sweep, revolve, hole,
  fillet, chamfer, combine, loft); all such faces must lie on one plane. Used for the plane/plane edges and the
  to-object extent, not for picks that need a point of the face (shell, draft).
* Used as a fallback behind the modelled faces. Unresolved blend edges in the set: 718 -> 358; designs whose first
  blocker is an unresolved edge: 12 -> 5.

### Joints: the layout of `Assemble` `8DFDD543` and why the pins sat 0.02 mm off (read, not converted)
* Record (3957 `Assemble1`, 795 bytes): reference to a `7B6C3C2D` (the first joint frame's holder), a row-major 4x4
  matrix at body+36 (cm) = **frame 1** in the moved component's own coordinates: rows `[0 -1 0 4.858] [-1 0 0 0]
  [0 0 -1 -0.1]`, so x axis (0,-1,0), y (-1,0,0), z (0,0,-1), origin (48.58, 0, -1) mm; then a reference to the
  `JointOrigin` `02E8AA49` and a second matrix = **frame 2**, identity at (56.63, 51.23, 1.57) mm (a hole of the board,
  on its top face); then the parameter holders in the order `alignAngle`, `alignOffsetZ`, `alignOffsetX`, `alignOffsetY`
  and, further on, the motion values (`OffsetX/Y/Z`, `AngleZ`). The `7B6C3C2D` repeats frame 1 and names the geometry it
  snaps to: a point selection `69EE2FA7` at (4.858, 0, -0.1) and two `F2A7590D` edges.
* One of those edges is (4.858, 0.032, -0.1) + (0, -0.064, 0) cm: a 0.64 mm edge of the last pin, and frame 1's origin is
  its **middle**, i.e. 0.32 mm from the pin's axis. The reading "frame 1 turned by `alignAngle` about z, then moved by the
  offsets, lands on frame 2" puts the pin axis at 0.32 - 0.30 = 0.02 mm from the hole centre in x and exactly on it in y:
  the 0.02 mm of round 12 is the difference between the half width of the pin and the 0.30 mm the author typed, not a
  misreading. Still one sample with no stored final placement (the occurrence's `CE2913AA` and the insert hold only the
  placement before the joint), and the tally shows no design of the 150 whose first blocker is a joint, so it is not
  converted yet.
* The resulting placement **is** stored (round 14 note): the root's transform records (`E03784ED`) name the header's
  reference as `<uuid>_urn:adsk.wipprod:fs.file:vf.<id>?version=2` (UTF-16), the matrix right after that text. There are
  three for 3957, all owned by the same occurrence id: one without a matrix, one with the insert's matrix (rotation
  diag(1, -1, -1), t = (0, -8.54, 1.3381) mm, followed by `Component Insert`) and, later in the stream, one with
  diag(1, -1, -1), t = (8.35, 51.23, -0.93) mm. That last one puts frame 1's origin (48.58, 0, -1) at (56.93, 51.23, 0.07),
  i.e. the hole frame (56.63, 51.23, 1.57) plus the typed offsets (x 0.30, z -1.5): the joint's result. The reader stopped
  after the UUID (no matrix found, so the header sat at the origin); it now skips the `_urn:` text and, as for every
  reference, takes the last record, so the body import places the header where Fusion has it. Only 3957 of the corpus has
  such keys.

### Typed references to two oddly spelled classes (the "tools are not where the board is" refusals)
* Two classes spell their GUID with the dashes out of place, in the class table and in every typed reference:
  `40DCC3F41D-E94F-B3B1-3E95-9B6ABE572C` (the blob-name holder of a body-state list) and
  `A7AEA631-985B-4DD1-8CE2-DE2C-14B54081`. The reader took a typed reference for `01`, an object id and a `str` of the
  form 8-4-4-4-12, so in a typed-reference stream (older class versions) the reference from a component's state list
  `CD57BC48` to its `40DCC3F4` was not seen, the component had no `.smb` blob, and `json::place` left its features at the
  identity while the body import (which reads the names from the UTF-16 runs) stood the bodies where the occurrence puts
  them. A typed reference is now also `01`, id, and the 36 characters of **the target's own class as the class table
  spells it**.
* Seen on 3857 Feather M4 Express: the stored board is [-25.4, -0.785, 0]..[25.4, 0.785, 22.86] mm (in the xz plane,
  centred), the tree's board was [0, 0, 0]..[50.8, 22.86, 1.57]; of the 133 hole tools (113.81 mm3) 3.99 mm3 lay in the
  tree's board. With the reference read, the tools lie in the board (111.81 mm3, two r = 0.45 tools in slots) and the
  design is history; 1833 microUSB breakout too (its box check failed the same way).

### Newer streams (2025 files): body-making features that are in no list the reader walks (found, not read)
The five designs whose first blocker was `8DBAF917` now stop at the state check: the file stores 300 solids and the
tree has the board extrude and 12 Base Features. The other bodies are made by feature classes the timeline walk does not
know, each the creator of body links: `2BB91A84` and `94B9F77E` (named `PCBExtrude`: copper, soldermask and substrate
layers; the first lists layer names `16_copper`, `16_copper_polygon`, `17_bottom_copper`, `16_smd_copper` and one body
link each) and `F193B033` (`BoxPrimitive`: five parameter holders and a 4x4 placement). Their parent chain is
`2EDC2536` > `666BFBBC` > `A2052F46`. Among the stored solids are cylinders 400 mm long (z -200..200, r 0.25 and others)
in the layer components: drill tools. Reading them needs the layer bodies as stored bodies (their geometry comes from the
board file, not from sketches) and the drill tools as their steps; not done.

### State-check refusals that remain (adafruit-more)
* A single stored solid that is the state **before** the design's last blends (2011 Battery: 16650.0 mm3 = the box, the
  features give 16443.98 with Fillet1; 931 OLED): the final body is not in the file, so the blends cannot be checked.
  Refused, as the rules ask.
* Stored solids of components the tree lacks (4313: the display's and the card socket's package bodies), which then
  count as unexplained "tools" and hide the board's cut; a stored solid with a negative volume (3211: -0.0063 mm3, a
  body read inside out); 2900: a 193.75 mm3 solid no feature makes.

### Combine `2A94257F`: the record's leading fields (read from 29 records; the operation is still not converted)
* Body: `u32 A`, `u32 n` + n references to `4A782808` (the RemoveBody of a consumed tool; n is 0 or 1), a flag block
  (with or without a `2E3AC990`), a `u64`, then `u32 ntools` + that many `2CA5A1CD` selections (**the tools**),
  `u32 m` + m direct `D26351F0` body links (the tools' own links, m = ntools; none when the tool is removed), and
  `u32 1` + one `2CA5A1CD` (**the target**); the selections are listed once more near the end.
  3651 FeatherWing's header part (also in 3900, 4200, 4242, 4400): Combine1 = tool Mirror1's body into Extrude1's,
  Combine2 = the five bodies of R-Pattern1 into Extrude1's, Combine3 = Extrude1's body into Mirror2's.
* `A` is 0 in 24 records (all of that part's, STEMMA Buttons, the Crickit CPX ones, CyberDeck HAT/Bonnet) and 1 in 5
  (NEMA-17, 5981 TRRS Trinkey Case, 6114 Solar Charger Case, and the two BQ25185 Cases, whose Combine1 takes the logo body
  as a tool and removes it). The round-10 comparison missed it (it compared records of `A` = 0 only, and the Cases').
  It reads like the operation (0 where bodies are put together, 1 where a logo is taken out of a case), but no rebuild
  has confirmed either value yet: the next step is to write Combine1 of the header part as a join and as a cut and let
  the state check tell (the part's stored bodies follow each Combine). A tool can be the several bodies of one body
  pattern, which the tree names with one feature name; that needs a name per copy first.

## 22. Round 14: Combine joins, the implicit second pattern direction, tools by place (2026-11-10)
Reader: `fusion_tree/combine.rs`, `fusion_tree/patdir.rs`, `tool_reach` in `fusion_tree/json.rs`; importer:
`fusion_explain_steps` in gusset-commands `import.rs`.

### Combine `2A94257F`: value 0 of the first field is a join (confirmed by a rebuild)
* The test section 21 asked for, on the pin-header part "2x16 short female" of 3900 Hallowing (Combine1: Mirror1's body
  into Extrude1's): written as a **join** the design rebuilds its stored states; written as a **cut** the step check
  refuses it (10543.31 mm3 against the stored 11631.15 mm3, a stored solid of 7.1025 mm3 unexplained). So `A` = 0 is a
  join. `A` = 1 (5 records, a logo body taken out of a case) has no rebuild yet and stays refused by name.
* Tools and target: the first reference list of `2CA5A1CD` selections after the fields is the tools, the next list of
  one selection the target; each selection's `9716F783` names one body link, whose creator is the tree feature that
  made the body (an extrude, a sweep, a stored body, a mirror of a body).
* Tools that are **all the copies of one body pattern of the target** (Combine2: the five bodies of R-Pattern1 into
  Extrude1's): the tree's pattern of a body already unites the copies with the original, so this Combine is written as
  nothing (listed under `skipped` with that reason). Copies of a pattern of another body, or some of the copies, stay
  refused: a copy has no name of its own in the tree.
* The tree's `combine` consumes its tools, as these records do (their tools have no body afterwards).

### Rectangular pattern with one stored direction and `CountV` > 1 (the header part's R-Pattern1: 2 x 16)
* The record has one `F2A7590D` (U: +x, `uSpaceDistance` 2.54 mm) and `vSpaceDistance` -2.54 mm with no second
  direction record: V is perpendicular to U. On which side is not in the record. It is read off the stored bodies: the
  straight profile edges of the patterned extrudes, moved to every copy's place (flattened onto the sketch plane), must
  each be a stored edge for one sense of `n x U` (n the sketch normal) and fail for the other; else the pattern stays
  unconverted. Here V = +y (copies towards -y with the negative spacing), the same turn as the 5100 component pattern
  of section 20 (U = +x, V = +y there by the stored copy matrices).

### Construction flag of the 276- and 300-byte line layouts
One byte after the second point reference (`01` = construction). The header part's holes are squares with a centre
cross of two construction lines, and its outline has two construction diagonals; read as profile curves the cross cut
the hole's region in four and the cut took a quarter (the chamfer's edges were then 0.335 mm off). 28 flagged lines of
the 300-byte layout in the set, none in a profile the reader reaches; the 276-byte layout is the same part in 3900.

### Extrude "to a body" (extent 1, third enum field 1, no distance parameter)
* 878 Extrude6: the enum block is `2, 1, 1` (cut, one side, **1**; a distance extrude has 2 there), the only parameter
  is `TaperAngle`, and after it comes a `2CA5A1CD` -> `9716F783` selection of a **body** (not a face). The stored tool
  is 51 x 20 x 4 mm = 4080 mm3: from the sketch plane up to the far side of the selected body as it then is.
* Read from the tool itself: the stored body that has every straight edge of the profile's outline (the pieces only one
  picked region has) at the sketch plane and at exactly one other offset gives the signed length; several such bodies
  must agree. No such tool: the extent stays unread. (The earlier "through all" reading measured edges of any stored
  body and refused two-sided evidence; it remains the fallback.)
* A to-object extrude whose object is a `face_recipe_data` face (1501 Extrude8: face `1` of Extrude7) is still refused:
  the design then stops at a spline outline, so nothing confirms the reading.

### Step check: tools by where they are (878 as the sample)
878 stores 13 solids: four states and the tools of three features that each made fewer tools than loops (a join of 8
loops with 4 tools of 34.0234 mm3, a join of 4 with 4 of 6.2832, the cut above with one of 4080). The older rule offers
every unexplained tool to each such feature, so two of them in one design never pass. Added behind the older rules: a
feature that removes material owns the unexplained stored tools whose place holds material before it and none after
it; one that adds material those whose place is all material after it and was not before; what the tools hold of the
body before (or lack of it) must be the feature's volume change exactly, and they are not more than its loops. A tool
that reaches out of the body it cuts (4080 mm3, of which 1029.67 in the part) is thereby checked by position too.
Measured for at most 64 unexplained tools. Tools in voids of a cut the placed tools explain are accepted as before
(none of them may hold any of the body).

### 2025 PCB files: what their timeline is (decoded, not converted)
* The design timeline `2F4C1849` holds only the board sketch and the silkscreens; everything else is in the compute
  list `B65B5682`: Extrude1 (the board), `94B9F77E` PCBExtrude1 and PCBExtrude2 in the board component, one
  `2BB91A84` PCBExtrude per copper component (four bodies each: `<n>_copper`, `<n>_copper_polygon`,
  `<n>_top_copper`/`_bottom_copper`, `<n>_smd_copper`), one `94B9F77E` per soldermask component, then per footprint a
  Base Feature or a `F193B033` BoxPrimitive.
* Every body link of these features leads, like a Base Feature's, to a `D3937028` body in a `CD57BC48` state list that
  names a `.smbh` blob: 6067 has 30 links (board blob: Extrude1's body and PCBExtrude2's `CopperHoles`, two bodies;
  copper blobs four bodies each; one per soldermask, box and package).
* **PCBExtrude1 of the board (no body link) is the drill**: after its `C46D3EEB` reference come five zero bytes,
  `u64 n` and `n` entries of five doubles: x, y (cm) and three zeros. 6067: 10 holes at (2.54, 2.54), (2.54, 15.24),
  (6.35, 2.54) ... mm. No diameter in the entry; the stored drill tools (cylinders from z = -200 to 200 mm) have it.
  The record also names the board's `369DDC0A` (layer data: 1.067 mm, 0.035 mm copper).
* **BoxPrimitive `F193B033`**: five `D91D429C` holders `DistanceX`, `DistanceY`, `DistanceZ`, `OffsetDistanceX`,
  `OffsetDistanceY` (6067: 4 x 4 x 0.1 mm, offsets 0), a `CAA90BED` plane reference and a row-major 4x4 at body+78
  (cm): identity turned, translation (-DistanceX/2, -DistanceY/2, 0) in the samples, i.e. the matrix is the box's
  corner. Which way DistanceZ runs is not read yet (the stored box decides).
* What a conversion needs: these three classes in the walk; the box as a rectangle sketch and an extrude; a PCBExtrude
  body as a stored body **before** its drill (the layer outline comes from the board file) followed by a cut of the
  hole table (circles with the stored tools' radii), so that the pre-drill solids and the drill tools in the blobs are
  states and tools of the model. Not done: the blobs' own order of states and tools has not been read.

## 23. Round 15: the PCB features of 2025 files, the step check per solid and per tool copy (2026-11-10)
Reader: `fusion_tree/pcb.rs`, `body_shapes` in `fusion_tree/acis.rs`, `Graph::new` in `fusion_tree/mod.rs`; importer:
`fusion_stored_bodies`, `fusion_explain_placed`, `fusion_states_check` in gusset-commands `import.rs`; evaluator:
`gusset-eval` `merge.rs`.

### PCB features: what is converted and from which bytes (six designs: 6067, 6250, 6494, 6501, 6522, 6538)
* The compute-list walk now takes `F193B033` (BoxPrimitive), `94B9F77E` and `2BB91A84` (both named PCBExtrude).
* **`94B9F77E`, its kind.** Behind the record's `C46D3EEB` reference: a zero byte, `u32 kind`, `u32 n`. Kind 0 is the
  drill (the board's PCBExtrude1, no body link), 1 the plated holes (`CopperHoles`, one body link), 2 and 3 the two
  soldermasks (one body link each). The same in all six files.
* **The drill's table (correction of section 22).** `n` entries of **40 bytes**: a zero `u32`, x, y (cm) and twenty
  zero bytes; the next reference follows the last entry directly (6067: n = 10, 6494: n = 20, the other four: n = 0).
  Section 22 read a `u64` count and five doubles, which runs four bytes over the record's next reference.
* **The table is the pad holes only.** 6067's drill tools have 26 cylinders: the 10 of the table (four mounting holes,
  six header pads) and 16 vias, whose coordinates (e.g. 3.5786, 8.4046 mm) occur nowhere in the design stream, only in
  the stored bodies: they come from the board file. So the holes are read off the tool; every table entry must be a
  hole of a tool, or the drill is refused.
* **The drill tools.** A component's `.smb` holds them next to its bodies: a body that is nothing but whole cylinders
  along z, one per lump (z from -200 to 200 mm). The board's `.smb` holds the board **before** the drill and its tool,
  each soldermask's `.smb` the mask **after** the drill and its own copy of the tool; the drilled board is only in the
  `.smbh`. 6250 stores a second, short tool per body (two r = 0.7 mm holes of its jack, 3.18 mm long, at another height
  in every component). Each tool becomes a sketch of circles on the plane through its middle and a symmetric cut of its
  length: of the board's extrude for the drill record, of the mask's own extrude for a mask.
* **A soldermask** is the board's outline (the sketch and picks of the extrude the layer record `369DDC0A` names)
  extruded from the component's z = 0 by the thickness its stored body has, towards the side the body lies on, then
  drilled. The thickness must be a double of the layer record (6067: 0.00804 cm twice, 6538: 0.00711188); the record's
  layout shifts between files, so the value is looked for, not read at an offset. Check: (board area - hole area) x
  thickness is the stored mask to all printed digits (6067: 446.0739 - 27.4085 mm2, x 0.0804 = 33.6607 mm3).
* **Copper and plated holes** come from the board file (tracks, pads, barrels): they are the bodies of the feature's
  state list, as for a Base Feature. A copper side is four bodies (`<n>_copper`, `_copper_polygon`,
  `_top_copper`/`_bottom_copper`, `_smd_copper`) of many lumps each; a lump becomes one stored body of the tree.
* **Which blob body is which state-list body.** The board's `.smbh` of 6067 holds four bodies for a state list of two
  (the board, `CopperHoles`): two are flat sheets (all vertices in z = 0) that no list names. The list's bodies are
  the blob's bodies that are not flat, in order; another count is refused. (A Base Feature keeps its rule of section
  13.)
* **BoxPrimitive.** Parameters `DistanceX`, `DistanceY`, `DistanceZ` (and two offsets, 0 in every sample); the 4x4
  (row-major, cm) stands two bytes behind the fifth `D91D429C` holder reference and is the frame of the box's corner
  (6067: translation (-0.2, -0.2, 0) for a 4 x 4 mm box, (-0.76962, -0.3175, 0) for a 14.935 x 5.987 one: the corner,
  not minus half the size). On which side of the frame's xy plane the box lies is taken from the body stored for it:
  the box of the eight corners must be the box of that body's vertices for +DistanceZ or for -DistanceZ, else the
  feature is refused (which also covers offsets that are not 0). It becomes a rectangle sketch and an extrude.

### A component's own timeline is the first it names
The root component of 6501 and 6522 references two `C3423E04` timelines: its own and that of the component being
edited when the file was saved (`TMF880X`). The reader gave a timeline to the first component in stream order that
names it, so that component's Base Feature was read as the root's and stood at the origin, 1.2 mm from its stored
body. Counted over the 150 designs: 12 of 2347 components name more than one timeline, and every extra one is the
first timeline of exactly one other component.

### Step check: one copy of a tool per body, solids of a body, the box
* **A tool stored once per body.** The drill tools are stored three times (board, two masks), each copy at its own
  component's height, and are far longer than the bodies are thick, so every copy holds the same material of every
  body: the placed-tool rule of section 22 gave all 78 to the board's drill (26 loops). Now, for a feature, a tool
  that is another tool over again within the box of the bodies the feature changes (the same volume, its box there
  equal to the other's or inside it: the short tool of 6250 reaches only part of the way through the board from a
  mask's height) is left for the features that cut the other bodies. Each cut must still remove exactly what its tools
  hold of its body; the same holds for the tools in voids (6250: two r = 0.3 holes where the outline has none).
* **Solids of one body.** The file's solids are compared one by one. A new-body extrude of two regions apart is two
  bodies in Fusion and one body of two solids here (2900: header strips of 250 and 193.75 mm3 from Extrude5): the
  solids of such a body are states too.
* **The box.** The model must lie in the box of the stored states and of the tools of its joins; the tools of cuts are
  left out (a drill tool reaches 200 mm past its board; before, the check compared with every stored solid).

### Two regions that share an edge (805 slide switch: Extrude4 removed 2 of 20 mm3)
805's block is an extrude of three picked regions that lie side by side. The kernel swept each region into its own
solid: one body of three solids that touch along walls, with the right volume. The slot of Extrude4 (10 x 4 x 0.5 mm
across the bottom, stored as a 20 mm3 tool) was then cut out of one of the three only (2 x 2 x 0.5). Not a reading
error: `gusset-eval` now unites the regions a feature takes when they share whole edges, before the sweep.

### Sketch splines `D82E012F`: the stored curve is a B-spline (read, not converted)
* After the record's references: `01 00`, `u32 degree`, a tolerance double (1e-9), then three arrays, each led by its
  count twice and an element size of 8: the knots (doubles), the weights (doubles) and the control points (x, y, z
  doubles, cm). 69 of the set's 79 spline records read this way with knots = control points + degree + 1 (8 designs:
  5981 has 60, 6114 and 6124 five each, 1501 four). 44 are cubic with 4 control points, 20 quadratic with 3, 5 of
  degree 5 with 10; all are clamped, planar and have unit weights; the control points are single-precision values
  (-0.640000153: an imported outline). 10 records have another layout (not read).
* Gusset's sketch spline is a centripetal Catmull-Rom curve through its points (ADR 0016): its segments are cubics
  whose tangents follow from the neighbouring points. A quadratic or cubic Bezier segment with free inner control
  points is not such a curve, and a degree-5 curve of ten control points still less: none of the 69 can be written
  exactly. Converting them needs an exact B-spline curve in the tree format and in the sketch model (degree, knots,
  control points, optional weights). Until then a profile with such an edge stays refused ("curve unknown"); it is
  not approximated.

### State-check refusals looked at and left (causes, as far as found)
(Round 17 found the causes of all of these but 5420: section 26.)
* 4313: Extrude2 (43 loops) is explained exactly by 43 stored tools (53.0222 mm3); one more stored solid of
  182.1189 mm3 belongs to no feature of the tree. 5021: the same with 22 tools (46.0391 mm3) and three solids of
  0.0280 mm3 left over. Whose bodies these are is not found.
* 3211: a stored solid of -0.0063 mm3 (the body converter reads this lead of a chip LED inside out; the same solid
  is in 6067, where the package's Base Feature makes it and the signs agree). In 3211 the component
  `CHIPLED_0805_NOOUTLINE` has three bodies (`D3937028`) in the state list of its `.smb` that **no `D26351F0` body
  link names**: no feature of the stream makes them, and the component's one Base Feature makes another body (body 1
  of 2 of its `.smbh`). The tree therefore lacks them. Bringing such bodies in as stored bodies needs the pairing of
  the list's bodies with the blob's (the `.smb` holds four solids and two sheets for the three); not read.
* 4800 MagTag now passes the tool rules and stops at the box (a jointed part stands 1.2 mm from its stored body, like
  3957 and 4200: joints). 4367 TFT Gizmo, which stored the same 22.2724 mm3 solid, comes in as history.
* 2219, 4745, 5420: a stored solid no feature explains (0.0791, 0.0101, 144.4789 mm3); not examined further.

## 24. Round 16: joints decoded and checked against the stored placements (2026-11-11)
Reader: `fusion_tree/joint.rs` (`joint_placements`, `place_by_joints`). Learned from the 42 `Assemble` records of the
two Adafruit sets (38 distinct joints; four records are repeats); all lengths in cm in the file, matrices row-major 4x4
(`p' = R p + t`), a plain reference is `01`, `u64` id, `00` (10 bytes).

### `Assemble` `8DFDD543` (the joint feature)
* Header (`str` version, `u64` id, a version-dependent run of zero/flag bytes), then the **flip** byte three bytes
  before the first reference (0 or 1; the two bytes after it are 0).
* Two **joint origins**, each: a reference to a `7B6C3C2D` (a frame snapped to geometry: it repeats the frame and names
  a point `69EE2FA7` and edges `F2A7590D`, with its own `OffsetX/Y/Z`, `AngleZ` holders) or to a `JointOrigin`
  `02E8AA49` (a named origin; UUID zero = the component's own origin); `00`; a byte 0 (a matrix follows) or 1 (none:
  the origin of the side's component); the frame = the origin in the coordinates of the side's occurrence.
* `00 00 00 00`, then four `D91D429C` parameter holders in the order `alignAngle` (rad), `alignOffsetZ`,
  `alignOffsetX`, `alignOffsetY`; further on `u32 2` and two `2E3AC990` transform records, a reference to the joint's
  **snapshot** `7F5A0426`, the UUID, the lists of the geometry and holders again, the name `Assemble` and its number.
* The `2E3AC990` pair holds the two frames in some later state of the design (5358/5360: both moved by -1.27 mm
  after the joint) and is not used.
* Four designs (6114, 6124, 6126, 6299) have a longer run before the flip byte and more bytes between an origin's
  reference and its matrix: not read (the joint is reported unverified).
* The joint **type** (rigid, revolute, ...) and motion values were not located: every joint of the sets is satisfied
  by the rule below with no motion term, so the sets may hold rigid joints only.

### Snapshot `7F5A0426`
* `01`, `u32 n` and `n` entries (occurrence `CE2913AA` reference, `00`, `u32`) for every occurrence on either side,
  a version-dependent run, then `u32 2` and the two **sides**. A side: a byte 0/1 and (for 0) a matrix = the side's
  frame **with the offsets, the angle and the flip applied**; `u32 k` and `k` occurrences (each followed by `00`) =
  the path of the side's occurrence, counted from the component named next (`E03784ED` reference, `00`): the
  component the joint lives in. `k = 0`: that component itself.
* An occurrence inside an inserted design is named by UUID texts instead of a reference (4862, 5128, 5981): not read.

### Occurrence path `549FBB80` (where every occurrence stood after each feature)
* (`27B2054B` references for some), `u32 k`, `k` occurrences each followed by `00` (the path from the root component
  down), the root component and `00`, a `u32`, `u32 m`, then `m` steps: `01 00`, a reference to a timeline feature
  (the occurrence's creation `54F9ACE8`, a Component Insert `428B5C6E`, an as-built joint `AA8B3D40`, a joint
  `8DFDD543`), `00`, a byte 0/1 and (for 0) a matrix = **the path's placement in the design after that feature**,
  all parents included. The last matrix is where the occurrence stands in the saved design; a path without any
  stands at the origin.

### The rule, and its check
* With `F` = half a turn about x when flip is set (else the identity), `A` = the turn by `alignAngle` about z with
  the translation (`alignOffsetX`, `alignOffsetY`, `alignOffsetZ`): the snapshot's first frame is
  `origin1 * F * A^-1`, its second frame is `origin2`, and the joint puts the first side so that
  `W1 * frame1 = W2 * frame2` (`W` = the side's path placement after the joint, taken from the path records;
  for a joint inside a component the path is prefixed by each path of that component). So a joint places its first
  side at `W1 = W2 * origin2 * A * F^-1 * origin1^-1`.
* Example (3957): origin 1 `[0 -1 0 4.858] [-1 0 0 0] [0 0 -1 -0.1]`, no flip, 90 deg, offsets (0.03, 0, -0.15):
  frame 1 = diag(1, -1, -1) at (4.828, 0, -0.25); frame 2 = identity at (5.663, 5.123, 0.157); the inserted header's
  path record has diag(1, -1, -1) at (0.835, 5.123, -0.093) after the joint, which carries frame 1 onto frame 2.
* All three statements hold for every joint whose records are read: 23 of the 38 distinct joints (rotations within
  1e-6, translations within 1e-5 mm). Unread: ten with UUID-named occurrences (4862, 5128 x 8, 5981) and the four
  of the other origin layout; one more (4367 `Assemble3`) fails the placement check (its side's path record does
  not list the joint, and the placement taken for it is a later one).

### What the importer does with it
* Joints stay timeline entries without geometry. When **every** joint of a design stream passes the check and every
  path record is read, the path records place the design: a body blob whose occurrence records give other
  placements than its component's paths (the placement before a joint, or none at all: 4200's two headers had no
  placement, 4863's jointed parts stood where they were inserted) gets the paths' final placements, never fewer
  than were read; a Component Insert in the root component takes its occurrence's last path placement instead of
  the insert's matrix (3957). The body import uses the same placements.
* 3957, 4200 and 4400 now come in as history (their box mismatches were the unplaced or pre-joint occurrences).
  4800 is unchanged: its one joint is verified and its bodies already stood where the path records have them, so
  its box mismatch (features y -5.215..5.585, stored -5.085..6.785; 293.8 mm3 short) is not a joint's.

## 25. Round 17 A: stale profile records, curves drawn twice, tapered extrudes (2026-11-11)

### Profile records (`0D57BD2F`) that outlive their curves
* A profile record is not always rewritten when its sketch is edited. 5360 (a connector cut down from four pins to
  three) still lists the fourth pin's circles (`crv_primary_id` 171, 175, 183) as holes of the housing and as
  regions of their own; no record of the design carries those ids (searched over every record's attributes). 2756
  lists a deleted circle (357) as a hole of a rectangle (child 0) and as the island inside it (child 1).
* Reading: a loop whose curves are all unknown is gone. A hole loop is dropped; a part whose outer loop (flag 1) is
  gone is dropped. A loop with some known and some unknown curves still stops the import ("curve unknown").

### Curves drawn twice
* A circle drawn twice (two `F0130424` records with the same centre, radius and seam): the copy touches the
  circle along its whole length, so it cuts it only at its own ends, the shared seam. 324's shaft flat: a chord
  whose ends lie on the circle, two copies of the circle, each listed as piece 2 of 3 (the cap above the chord: the
  pieces end at the chord's first end, its second end and the seam). The cut parameters now take a curved
  polyline lying wholly on the curve as its two end points.
* A loop that lists every edge twice can chain by walking each edge out and back (3027's slot: two half circles
  and two lines, each drawn twice) or round twice (its circle drawn twice, chained at the seam): the ring passes
  its points twice and holds no interior point. Such a ring is replaced by the ring of the loop's curves taken once
  each, when that ring is simple.
* 858 is not settled: a circle drawn twice with a square (half-width = radius, so its sides are tangent at the
  seam and opposite it) lists the circle as 5 pieces (1, 3 and 4 in the loop). The loop closes only when the
  tangent point opposite the seam cuts the circle and the seam does not; the seam cut that 324 needs gives the
  wrong pieces here.

### A piece that nothing cuts
* 512 lists the outline's bottom edge as piece 2 of 3 beside the four sides of an inner rectangle that closes on
  its own (its top lies on the outline's top, listed as piece 2 of 3 too); nothing in the sketch meets that bottom
  edge. A piece that cannot be cut out of its curve is now left out when the loop's other curves close without it;
  otherwise the import still stops ("curve piece a/b not reproduced").

### Tapered extrudes
* `TaperAngle` (radians, a model parameter of the extrude like `AlongDistance`): negative leans the side walls
  inward, so the solid narrows away from its sketch plane (592's plug shell: -3 deg over 12 mm; 6299's lips:
  -45 deg). The sign is confirmed by the state check: 592 passes with the solid narrowing and fails (a fillet edge
  0.445 mm off) the other way.
* Import: the straight extrude as a body of its own, a `draft` of all its side faces (`Name/side(...)`) about the
  sketch plane pulled along the sweep by the negated angle, then a `combine` with the target for join, cut and
  intersect. Symmetric and two-sided tapers are still refused.
* Edges on a tapered extrude's side faces are not modelled from the profile (the walls lean); they come from the
  stored bodies' named edges.

### Revolve (`E3849A15`) decoded (revises 12.5)
* Body: `u32 operation` first (4 = new body in both corpus revolves, the extrude enum's values), then `u32 2`
  (meaning not known; both revolves have it), `u8 0`, `u32 1`.
* Profile: a `2CA5A1CD` selection -> `4BD53E5A` profile record, read like an extrude's (`0D57BD2F` regions).
* Angle: the model parameter `AlongAngle` (radians; 360 deg in both).
* Axis: a second `2CA5A1CD` selection -> `5A1BF548` -> `E2CEFD18` sketch-curve reference, the same as a sweep
  path's curve. Its last 16 bytes are `u64 sketch data id`, `u64 crv_primary_id`: at 0x1d in the 45-byte records
  of 1145 and 3722 (tags `282`, `270`), at 0x21 in the 49-byte ones the sweep reader met. In both designs the axis
  is a line of the profile's own sketch (1145: two whole-turn revolves of one sketch about lines 114 and 134).
* Import: `revolve` with `axis: {line}` and `angle: 360`. A part turn (which way it turns is not decoded), an axis
  in another sketch or not a line, and any parameter besides `AlongAngle` are refused. Both designs' revolves
  build; neither design reaches the state check yet (1145 stops at a Thread, 3722 at a shell whose removed face
  is not a cap or planar side of an extrude), so their shape is not confirmed.

## 26. Round 17 B: stored solids the step check could not place (2026-11-11)
Eight designs whose features read but whose step check refused a stored solid or the box (all eight now come in
as history: adafruit-more 103 to 111 of 150, no design lost): the causes, and what
changed.

### Base Feature inputs that it drops (2219, 3211, 4242, 4745, 5021)
* A Base Feature's record names its PM bodies, then its DM bodies, then **again the PM bodies that become DM
  bodies**, in DM order (section 13). Where the PM block is longer than the DM block, the PM bodies named once are
  inputs the Base Feature drops: they are in the component's `.smb` (the PM bodies' state list `CD57BC48` names it
  through `40DCC3F4`) and in no later body. 5021's slide potentiometer: 15 PM bodies, 6 DM; the 9 dropped are three
  0.4 x 0.2 x 0.35 mm blocks (0.0280 mm3), three pins turned upright and a housing 0.05 mm off its final place.
  2219: 31 PM, 21 DM (10 pins of 0.0791 mm3); 4745: 41 and 21, two occurrences; 3211: 3 and 1; 4242: four Base
  Features with 4, 1, 1 and 1 dropped.
* The PM list's bodies are, in list order, a run of the PM blob's solid bodies: in 5021 blob bodies 3..17 of 20
  (body 0 is the component's extrude, the others sheets), and the kept ones (list places 9..14) have exactly the
  volumes and boxes of the `.smbh` DM bodies.
* Reader: the `stored_body` features of such a Base Feature carry `dropped_inputs` (`brep`: the PM blob, `count`:
  PM bodies, `kept`: the list place of each DM body's PM body). Importer (`fusion_dropped_inputs`): reads the PM
  blob's solid bodies with the occurrence's placement and takes the one run whose kept bodies are the DM bodies
  (volume and box); each dropped body then accounts for the one stored solid of its volume and box. No run, or
  more than one: nothing is accounted for. The note says how many stored solids were such inputs.

### One tool times the loops, by coincidence (4313)
* Extrude2 cuts 43 loops: 11 header holes (1.2331 mm3), 4 mounting holes (7.7067) and 28 vias (0.3083), 53.0222
  mm3 together. 43 x 1.2331 = 53.0233 is within the volume tolerance of that, so the rule "one stored tool times the
  copies" took the 11 header holes as the whole cut and left the mounting holes over. A cut of several loops
  stores a tool per loop: that rule now needs as many stored tools of the volume as the copies; the sum rule then
  explains the 43.

### An extrude direction given in the assembly (4800)
* The extrude record's direction is a unit vector in the sketch's frame (4524 of 4530 extrudes of the two Adafruit
  sets). In 4800's standoffs (Extrude1, Extrude2) and screen (Extrude1 (2)) it lies **in** the sketch plane
  (normal z, direction y): it is given in the assembly, where the occurrence turns the sketch normal onto it (both
  components were drawn in place). The side was taken from the sign of a dot product of about 1e-48, the wrong way:
  the standoffs stood through the board and the screen 0.1 mm off. Now such a direction is compared with the
  sketch normal as the occurrence places it (it must be parallel to it, else the design is refused); an extrude of
  that kind that starts off its sketch or goes both ways is refused.

### A join's bodies are its body selections (2441)
* Extrude11 joins standoffs (to object) to Extrude9's body. Its record reaches two body links: one through its body
  selection (`9716F783`, Extrude9's), one through the face selection of its extent (`5662F619`, a face of
  Extrude1's plate). The second was taken as a target and a `combine` merged the plate into Extrude9's body; the
  file's end bodies keep three bodies (15394.8521, 19207.7568, 3614.7904 mm3). Where a feature has body
  selections, they are now its targets; a body reached only through a face selection is not. In the two sets this
  changes 2441, 2232 (Extrude7, the same pattern) and the pad extrudes of 5358 and 5360 (both stop earlier, at
  features the reader cannot read).

## 27. Round 18 C: features that are read but did not build (2026-11-11)
Seven designs stopped at a feature the reader reads in full: a sweep, two shells, two chamfers, a fillet and a
PCB board extrude. For each, the input was compared with the stored solids (volume, box) to tell a reading error
from a kernel failure.

### A sketch of another component is taken at its own coordinates in the feature's component (4863)
* Sweep1/Sweep2 (component `Pins`) sweep along curves of Sketch1 (component `header`); Extrude1 of `Header Female`
  extrudes the same Sketch1; Extrude2 of `Header Female` cuts with the Pins' profile sketch. Round 16 places the
  occurrences where the joints left them, and `header`, `Pins` and `Header Female` end up apart (all turned 30
  degrees about x; `Pins` stands at (1.27, -2.22, -5.77) mm in the header's frame). Placed with its own component
  the path started 4.588 mm off the profile plane; Extrude1 stood 1.9 mm off the edges Chamfer2 names.
* The stored bodies settle it: the pin body of the `Pins` blob is the sweep with the path at Sketch1's own
  coordinates in the Pins frame (section 12.5), and Chamfer2's edges (Header Female frame) lie on Extrude1 built
  from Sketch1's own coordinates. Fusion took the sketch while the occurrences still coincided; the joints that moved
  them came later and do not carry the feature along. Rule: a feature of component A that uses a sketch of
  component B gets a copy of that sketch in A (`plane_note` "a sketch of another component, taken in this
  feature's"), placed with A's occurrence. Applies to extrudes, revolves and sweeps (profile and path). Not checked
  against a design where the occurrences already stood apart when the feature was made; the step check would refuse
  such a design (the bodies would not match), not pass it wrongly.
* 4863 now builds every feature; the step check stops at the board's drill cut Extrude2 (3) (261.56 mm3 removed,
  the 86 stored tools hold 92.72 mm3) and a stored 0.9988 mm3 solid it cannot place.

### Shell: the opened face is named by a point of the face (2756, 3422)
* The shell wrote `FaceG::Plane.p` (any point of the plane: for an extrude side, the start of its sketch segment
  in the sketch plane, a corner). 2756's Shell1 opens the front face of the USB connector (Extrude2, z 1.6..4.1):
  its corner (0, 4.89, 1.6) also lies on the board's top edge, and the importer shelled the board (Extrude1). It now
  writes `mid` (the chord middle at mid height, (0, 8.89, 2.85)): the connector.
* 3422's Shell1 opens one side of each of six boxes (Extrude2 makes six bodies; the Gusset body is one of six
  solids): OCCT's thick solid fails on the compound. The shim now shells solid by solid (a solid with no opened
  face stays as it is, as Fusion's shell acts on the bodies of its faces): 1890 -> 630 mm3, the cavities 6 x 7 x 5 mm
  (open on y). The step check cannot confirm it: the file stores the six boxes before the shell (315 mm3 each) and
  the Extrude3 tools (6 x 7 mm3), nothing after; Shell1 follows the last stored state. Refused as before.
* 2756's connector (chamfered, then filleted on 12 edges) is shelled by OCCT only with every pair of offset faces
  intersected (`MakeThickSolidByJoin(..., intersection = true, ..., GeomAbs_Intersection)`): the shim retries so
  when the default fails. 94.839 -> 41.232 mm3; the step check confirms the design (now history).

### Blends OCCT refuses (1463, 3632, 2232, 4863)
* A chamfer as deep as the face it runs into: 1463 Chamfer1 (the bottom rim r = 25.5 of a 1 mm ring, 1 mm), 3632
  Chamfer2 (the rim of a 0.6 mm deep round pocket, 0.6 mm). A fillet pair that uses up a face: 2232 Fillet1
  (radius 4 on both corners of 8 mm wide ends: a full round). Inputs as read are right (distances, edges, the face
  widths equal the blend), OCCT's `BRepFilletAPI` fails on all three.
* 4863 Fillet2: radius 1 on the two bend edges of a bent pin (0.64 mm long, inner and outer). OCCT fails on the pair
  and blends each edge alone.
* Shim fallbacks, in order, after the plain blend fails: (1) edges that share no vertex and no face, blended one
  after another (the history of the passes merged with `BRepTools_History`; an edge the fillet rebuilt is found by
  its ends and middle); (2) the corner of each convex edge between perpendicular faces whose section is constant
  along it (a line between planes: a prism; a whole circle between a plane across its axis and a coaxial cylinder:
  a revolution) cut away by a tool reaching past both faces, taken only when the removed volume equals the corners'
  own to 1e-7 (each corner wholly in the body, nothing else taken).
* Results: 3632 is now history. 1463 builds; its step check cannot confirm Chamfer1: the stored solids are the
  states before it (4100.2236 mm3; the chamfer ring is 79.0633 mm3), so it is refused as before. 2232's Fillet1 and
  the chamfers build.

### United regions keep a split edge whole (2232)
* Extrude3 (the HDMI connector) takes the two regions of a rectangle that the board outline crosses. `gusset-eval`
  unites them (section 23), but the rectangle's top and bottom lines stayed cut where the outline crossed them: the
  body had two walls and two edges there, and Chamfer1's edge midpoints (the middle of the whole edge) were 0.6 mm
  from either piece. The united outline now joins consecutive pieces of one line that run on straight. Arcs are
  not joined: joining a circle's pieces made 3464's next boolean (Extrude2) invalid.
* 2232 then stops at Mirror2: its record names WorkPlane2 (a construction plane, the mirror plane by its position
  x = 60.04) as its only input feature, selects no body, and links Extrude8's body. Probably a mirror of that body
  joined into it (the features give 44305.6 mm3, the stored shapes 59417.4); not read.

### A board whose cut-out touches its outline at one point (2348)
* 2348's board sketch has a notch (x 0.1..5, y 18.23..35.23) whose left side runs from (0, 35.23), on the
  outline, to (0.1, 18.23): the region is the board minus the notch, its boundary through (0, 35.23) twice, with a
  0.1 mm sliver along the left edge. Read right: built with the notch as a hole touching the outline, the board is
  5570.222 mm3 and 7593.788 mm2, the stored board exactly.
* OCCT cannot hold that solid as one valid solid: as one loop the prism's edge at the point bounds four faces, as
  outline and touching hole the face's wires touch, and every later boolean (the drill cut Extrude2) is refused as
  invalid. Left: no result-preserving form was found (two solids would change the body's solid count).

### Result
* adafruit-more: 113 of 150 as history in the corpus run (2756 and 3632 gained; 3464 was lost to the arc joins,
  which were then dropped, and comes in as history again on a single run), so 114 with the final code. adafruit: 12
  of 18, unchanged.

## 28. Round 18 D: faces and edges picked by the stored bodies (2026-11-11)
Faces a shell removes, a draft tapers or an offset moves, and blend edges, that the extrude model (caps and
sides) does not give.

### Named faces of every surface type
* Every face of a stored body (`.smb`, `.smbh`) that carries a `generic_tag_attrib_def` name (section 12) is read
  with its surface record: the type is the word before `-surface` in the record type (`plane`, `cone`, `sphere`,
  `torus`, `spline`, ...), then its positions/vectors and doubles as stored, and the face's vertices and points
  along its line and ellipse edges (`acis::named_faces`).
* `plane-surface`: point, normal (section 12). `cone-surface`: base centre, axis (unit), major axis (its length is
  the base radius), then the doubles ratio, sine and cosine of the half angle and a scale. A cylinder is a cone
  with sine 0: 3722's revolve face `5 [307]` is `centre (0, 0, 0.95), axis z, major (3.35, 0, 0)`, doubles
  `1, 0, 1, 3.35`; 3027's hole side `25 [302]` has cosine -1 (the axis points against the material side). The
  radius at height `h` along the axis is `radius + h * sin / cos`; no sample with a true cone checks that sign.
* **Faces with no edges.** Blobs of some states hold face records no coedge runs around (no loop with edges),
  under a name the body's real face also has, on another plane (3093 `.smbh`: `17 [317]` at y = 0.665 cm and a
  second one at y = -0.097 cm; 5981 and 6299 have such records too). They name no face: the stored planes and
  named faces leave them out. (3093's chamfer edges between the cap `2 [314]` and the slot sides of Extrude
  `317` resolve then.)
* **Negative operation ids.** A face an OffsetFaces or Draft changed gets the feature's op **negated** in its
  name: 6299's `5 [326, -425]` is `5 [326]` moved 1 mm by OffsetFaces (op 425), and the drafts after it name
  `[326, -425]` and `[326, -425, -430]` faces. A face an offset moves keeps its name otherwise: 3027's
  `25 [302]` is a 0.635 mm hole in the `.smbh` and a 0.838 mm hole in the `.smb`, both named `25 [302]`.

### Picks by boundary points and surface (tree format: ADR 0042 amendment of this date)
* A face recipe the model does not give (`face_with_middle` / `face_pick` fail) is picked by the stored faces
  whose name **starts with exactly** its entry: their surfaces must agree (then the pick carries the surface) or
  else the pick goes by points alone (`surface: other`, the offset case above: the model's face at the feature
  holds its own state's points). The points are the faces' vertices and edge points over all stored states
  (at most 64). An extrude's arc side is picked by its cylinder (sketch arc centre and radius, sketch normal) with
  the profile arc's points at mid height, or, for an extent that is not a distance, the stored points on that
  cylinder.
* The importer takes the face on the surface (all tessellation nodes within 0.001 mm) holding the most points;
  a tie is refused. 3027 (an offset of a hole side) now comes in as history.

### Blend edges between an extrude's slot and another extrude's wall (5981)
* A side plane of one extrude across the axis of another extrude's arc side cuts the whole arc piece, like a cap
  does, when the edge's other faces are that extrude's own sides (5981's Chamfer4: wall `5 [310]`, slot arc
  `3 [336]` between its sides `4` and `6`).
* A straight edge between two planes that runs into an arc side of the profile ends where it touches the arc's
  cylinder (5981: wall `5 [310]` and slot side `6 [336]`, ended by the cap `2 [310]` and the arc `3 [336]`).

### Draft: angle sign, pull direction, older layouts
* **Face sense.** The first logical token of an ACIS `face` record (`0x0a` true, `0x0b` false) is its sense:
  `true` is reversed, the outward normal is then the plane's normal negated. 258's pin caps `1 [309]` and
  `1 [311]` (outward -y, plane normal +y) are the only reversed faces seen; every other face read is forward,
  and Fusion orients a hole's cylinder by a cosine of -1 instead (3027).
* **Sign.** The pull is the neutral face's outward normal; a positive `Angle1` narrows the part along it, a
  negative one widens it, as Gusset's draft does. 355 checks both: Draft1 (10 deg) and Draft2 (-10 deg) share the
  neutral face `2 [307]` (y = 1 mm, outward +y). Its `.smbh` holds the drafted faces `3 [309, -311]` (normal
  (0, 0.1736, 0.9848)) and `3 [307, -313]` (the same plane, normal negated): z = 1.1736 mm - 0.1763 (y - 0.015
  mm), so Extrude3's top (outward +z) falls along +y (narrows) and Extrude2's bottom (outward -z) rises against
  it (widens on the pull side). The rebuild gives exactly those volume changes (-9.92 and -0.88 mm3) once the
  importer pulls along the tree's normal on an offset datum plane (it pulled along the XZ frame normal, -y).
* **Older files (324).** Both angles are parameters with the role `Draft`, in holder order (the second 0: one
  angle, -75 deg in 324). The faces are a list of **plain** face recipes, wrapped in a `71D91267` record: its
  bytes hold the recipe class name `7ACC2A03-...` as text twice (u32 length, text, `00 00 01`), then the usual
  face list (`u32 1, u32 3, u32 1, ...`, `3 [307]`) and `face_recipe_data`. Of the two plain-face lists the
  faces' has the lower record id, as the bounded-face list does in newer files (355: 604 faces, 611 plane;
  258: 869 and 876).
* **Caps.** A drafted extrude cap is picked by its stored points (the modelled cap point is the sketch origin's
  projection: 258's two pin caps on y = 29 mm were one pick).
* **Refused.** A drafted face that does not stand along the pull (258: the pin caps, outward -y, drafted 45 deg
  about a chamfer face with normal (0, 0.707, 0.707)): Gusset's draft is for side walls, and OCCT's draft
  crashed the process on it (`Extrema_ExtCS::Points` in `Draft_Modification::Perform`).

### Shell of a whole body (4632)
* 4632's Shell1 selects a body (`9716F783`, a body recipe), not faces: a closed shell, a body with a void. The
  kernel's thick solid with no removed faces gives the inner offset solid (a 30 x 20 x 10 box with a 2 mm wall:
  2496 mm3, the void, not 3504), so the reader still refuses it ("no removed face was found").

## 29. Round 19 E: threads, splits, coils, mirrors of faces and of components (2026-11-11)

### Mirror of faces with blends of their edges (2232)
* A construction plane among a mirror's input features is its mirror plane, not a feature it repeats: 2232's Mirror2
  names WorkPlane2 (x = 60.042164 mm) and nothing else. It selects four faces (`5662F619` face recipes, each one
  face named by one generator and one operation): `6 [346]` and `4 [346]` of Extrude12 (a 1 x 20 x 2 mm block joined
  into Extrude8's case, x 120.1..121.1) and `6 [335]` and `7 [335]` of Chamfer3, which chamfers eight edges, two
  of them the block's inner edges. The faces of a mirror may thus come from several features: every one must be a
  cut or join extrude, or a fillet or chamfer.
* A selected blend face stands for the blend's edge nearest to it, the face taken from the stored bodies by its name
  (`stored_face`): here the edges at x = 120.1, z = 1.2 and 3.2 (the nearest by a factor of two and more).
* Fusion copies the blended faces: the mirrored block is a wedge (1 x 2 mm, its two 1 mm chamfers meet), 0.38 mm of it
  standing out of the left wall. Chamfering the joined copy fails (the chamfer would run into the wall). The tree
  therefore copies the extrude as a body of its own (its sketch's frame reflected, the distance negated: the same
  reflection the tree's mirror of features makes), chamfers the reflected edges on it, and joins it to the body the
  extrude joined (`combine`). Extrude8's body: 7068.8114 -> 7071.7656 mm3, the end body of the history blob
  7071.7657 mm3 (the 2.9542 mm3 of the wedge outside the wall). 2232 now comes in as history.
* A mirror of faces whose makers are only extrudes stays a mirror of those features (one or several); with blends,
  cuts are not converted (a blended cut tool is no sample's).

### Thread `584D9526` (1145, 3350, 324): all four in the corpus are modeled
* Record body (after the header): `f64` flank angle in degrees (60), `u8` (1 on the external threads, 0 on the
  internal ones), four `wstr`: class (`6g`, `6H`), designation (`M16x1`, `M3x0.35`), size (`16.0`), form
  (`ISO Metric profile`); `u8` internal (0 external, 1 internal); `u32` (2, 1, 2, 2: not understood); `f64` major and
  minor diameter (cm); `u8` 1; `f64` pitch and pitch diameter (cm). Then the parameter holders `ThreadLength` and
  `ThreadOffset` when the thread does not run the whole face (1145: 9 mm from 4 mm; 3350 Thread1: 12 mm from 5 mm),
  a `u8` 1, `u32` count and the face selection (`2CA5A1CD`). No byte was found that tells a modeled thread from a
  cosmetic one: every one of the four is modeled.
* Modeled or cosmetic is told by the stored bodies: a modeled thread leaves helix curves (`intcurve` of subtype
  `helix_int_cur`: root, two perpendicular radius vectors, the pitch vector, then the axis) whose travel per turn is
  the thread's pitch and whose radius is half its major or minor diameter. 1145's `.smbh` holds two of radius
  0.7942 cm (major 1.5884 / 2), pitch 0.1 cm, from z = 0.4 cm over 9 turns (the 9 mm from 4 mm); 3350's holds radii
  0.7942 and 0.73705 (Thread1, external) and 0.80761 (Thread2, internal: major 1.61522 / 2); 324's `.smb` and
  `.smbh` hold radius 0.15352 cm, pitch 0.035 (Thread2, M3x0.35 internal). The faces also carry a
  `thread_face_data` attribute.
* The form, from 3350 Thread1's edges at angle 0: crest edges at z = 0.0125 and 0 cm modulo the pitch (a crest flat
  of P/8 on the major radius), root edges at 0.067 and 0.0455 (the root flat on the minor radius takes the rest),
  flanks across 0.033 cm axially for 0.05715 radially (tan 30 degrees).
* The reader now says which: a thread with no such helix in any stored body is cosmetic and left out of the tree
  (`skipped`, "a cosmetic thread (...): no helical face of it is stored, it changes no geometry"; the state check
  then confirms that the stored states have no thread); a modeled one is refused with its designation, class,
  side, diameters, pitch and extent. Building the form with the helical sweep was not tried: how Fusion ends the
  thread (the run-out at the offset and at the face's end) is not established, and each of the three designs
  stops at another feature as well (with the threads left out: 1145 Fillet1, "its edges are on several bodies";
  3350 Revolve1, "the revolve axis is not a line of its sketch"; 324 Fillet1, edges not resolved, and Split3).

### Split Body `65CA3206` (3651, 324, 4632): read, not converted
* Body: `u8 1, u8 1`, `u32 0`, `u8 0`, `u32` (0x264, 0x16b: not understood), `u32 0`, `u16 0`, then `u32 1` and the
  selection of the body to split (`2CA5A1CD` -> a `9716F783` body recipe), `u32 n` and the n body links the split
  makes (`D26351F0`: 3651 Split1 one, 324 Split3 two), `u32 1` and the selection of the splitting tool
  (`2CA5A1CD` -> `5A1BF548`, a construction plane, in 3651: WorkPlane2; `5662F619`, a face, in 324). The feature's
  input list repeats them: body selection, tool, body recipe.
* Gusset has no operation that cuts one body into several that stay bodies (a cut or an intersection keeps one
  side), so the tree cannot say it; adding one is a document-language change (an operation of its own in
  `gusset-doc`, `gusset-eval` and the tree importer), left for when a design needs it. Neither design gains from it
  now: 324 stops at its modeled thread first, and 3651 with the split left out stops at the box check (the
  features reach z = -4.0 mm, the stored solids -3.7 mm), which a split (it keeps the volume and the box) does not
  change.

### CoilPrimitive `DB2E4EDC` (3722): parameters read, the stored coil does not convert
* Parameters (holder index, role): 0 `Diameter`, 1 `SectionSize`, 2 `TaperAngle`, 3 `Revolutions`, 4 `Height`. 3722
  has two coils, one per component: d = 67 mm and 71 mm (the second an expression of a user parameter), 1.5 turns,
  6.5 mm, section 2 mm, taper 0. Body: `u8 0`, `u32 4` (the operation, new body, as an extrude's), `u32 1`, the
  `Diameter` and `Height` holders, `u8 1, u8 0`, a `DA1231DE` (an entity by its uuid: the axis or base point), a
  `7B6C3C2D` frame, `u8 1`, then `Revolutions`, `u32` (3 / 1), `SectionSize`, `u32` (3 / 4), `TaperAngle`, `u32 1`,
  and the body link. Read as section position (3 outside, 1 inside) and section type (3 and 4: the two triangles):
  both coils' solids lie between r = 33.5 and 35.5 mm (outside of d 67, inside of d 71), and z from -1.1547 to
  7.6547 mm, which is 6.5 mm of travel plus half of 4 / sqrt 3 each side: an equilateral triangle 2 mm high.
  That reading is not confirmed: the first coil's stored solid converts to a negative volume (-13877 mm3, the same
  after Move1), the second's to 501.18 mm3, while a 2 mm triangle swept 1.5 turns gives 744 to 758 mm3 (Pappus).
  The coil's faces are helical splines; their conversion, not the coil, is in doubt. Left unsupported. 3722 also
  stops before the coil: OCCT fails on Shell3, and Chamfer1's edges are not resolved.

### MirrorComponent `71BBEDD5` and DerivedInstance `66E375EA` (4343): read, not converted
* The mirror names the occurrence it mirrors (`CE2913AA` -> component `E03784ED`), the mirror plane (`5A1BF548` ->
  `CC54ECAD`), the `CreateComponent` of the new component and its `DerivedInstance`. The DerivedInstance stores a
  4 x 4 matrix (rows (1, 0, 0, 3.94), (0, 0, -1, -1.30897), (0, 1, 0, 0.494966) cm, (0, 0, 0, 1)): a proper rotation
  (determinant +1) and a move, so the reflection is in the new component's own bodies, not in its placement. With
  both left out the design stops at a stored body of its Base Feature that does not come in as one body, so the
  mirror alone gains nothing; left unsupported.

### Result
* adafruit-more: 116 of 150 as history (2232 gained, none lost). The threads of 1145, 3350 and 324 are now refused as
  modeled threads with what they are; 3651 (Split), 3722 (CoilPrimitive) and 4343 (MirrorComponent) stop where
  they did.
