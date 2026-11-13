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

## 30. Round 19 F: package generators, bodies moved between components (2026-11-11)
Reader: `timeline` in `fusion_tree/mod.rs`, `carry_moved_bodies` in `fusion_tree/bodyops.rs`, `design_json` in
`fusion_tree/json.rs`.

### ElectronicsPackageGenerator `827B3935`: a custom feature over ordinary features
* The record holds the strings `adsk.fusion.custompackage3d`, `Autodesk Inc.`, an id, `ElectronicsPackageGenerator`,
  a zero UUID (no user name) and the package kind (5905: `surface_mount_header_female`). Its **first reference
  list** (`u32 n` at byte 21, then n references) names the features it made, in order: 5905's 15 are WorkPlane,
  Sketch, Extrude, WorkPlane, Sketch, Extrude, R-Pattern, WorkPlane, Sketch, Sweep, Chamfer, Mirror `D1728651`,
  Move `64E40C8E`, R-Pattern, R-Pattern. The list follows again behind the record's own id.
* These features are in no timeline list (`2F4C1849`) and no compute list: only the generator is. The reader now
  puts the members in its place, in list order, and skips the generator itself. They are ordinary parametric
  features with their own parameters and the bodies they make; what the generator's own parameters (pin count,
  pitch) were is not read, so editing them in Gusset edits the features, not the package.
* 5967 holds six generators (one per header), each behind a component activation `54F9ACE8`.

### Body->Comp `56225BC7` and CutPasteBodies `BA3059E4`: a body moved into another component
* Both have the same layout: a body selection `2CA5A1CD` at byte 29, then a Base Feature `D7015B81` reference and
  a RemoveBody `4A782808` reference (both off-timeline features). The record's name strings are `Body->Comp` and
  `Body->Comp1 - Housing` (5358): the base name holds `->`, which the name reader now accepts for this one word.
* The Base Feature holds the body in its new component (a stored body) and the RemoveBody takes it out of the old
  one. Nothing about the shape changes: the reader carries the body on. The body links whose maker is the Base
  Feature are given to the maker of the removed body, and the move and both halves are skipped, so later features
  in the new component act on the history body (5358: `Extrude - Pad Clearance` in component Housing cuts
  `Extrude - Housing`). A move whose halves are not one Base Feature making one body and one RemoveBody of one body
  is refused.
* 5358 and 5360 now stop at a later feature (below).

### The header package (5905, 5967): what its features needed
* **Two-sided extrude with a taper per side.** `PinSocket` has `AlongDistance` 0.322 mm with `TaperAngle` 45 deg and
  `AgainstDistance` 2.558 mm with `Side2TaperAngle` 0: a lead-in widening to the top face, then a straight hole. The
  stored cut tool is 0.7493 mm3 = 0.46^2 x 2.558 (straight) + the frustum 0.322/3 (0.46^2 + 1.104^2 + 0.46 x 1.104).
  The reader made it one extrude of 2.88 mm from a moved sketch tapered over its whole length; now the extrude keeps
  both reaches and both tapers (`distance2`, `taper2_deg`; ADR 0042 amendment) and the importer builds and drafts
  each side from the sketch plane and cuts both in one combine.
* **Pattern of a tapered cut.** The importer wrote a tapered cut as a new body, a draft and a combine, and named the
  feature by the target body: a feature pattern of it then repeated the body. The feature is now the combine.
* **A symmetric direction of two.** `PinPattern2` (20 x 2) has the V direction marked symmetric (the third double after
  the `ff ff ff ff` of its `EA0E146E`) with a count of 2 and `vSpaceDistance` -2.54 mm (`-param_e`). The header body
  spans y -3.81..1.27 mm and Move1 moves the mirrored pin row by -2.54 mm, so the two instances are the original and
  one copy at the signed spacing. Only a count of 2 is read that way; other even counts stay refused. The 39
  `AD719C1A` records (one per copy) and the `EA0E146E`'s 39 index pairs number the copies; not decoded.
* **A body mirror's copy as a body.** MirrorPin1 mirrors the Pin body into a new body (no merge); Move1 selects that
  body and R-Pattern3 patterns the mirror feature. A move or remove may now target a one-body mirror of a body, and a
  pattern of such a mirror is a pattern of its body (`merge: false`, one new body per copy as the file has).
* **Chamfer of two distances.** Holder `0D383DB2` (also the holder of newer equal chamfers) with `Distance 1` and
  `Distance 2` (5905: `param_b / 4` = 0.115 mm and `param_b / 1.5` = 0.3067 mm; 5358: 0.3/0.5 and 0.2/0.5 mm). The
  chamfer record names its edges as `01`, `u32 n`, then n `u64`s (0, 0x200, 0x300, ...: edge numbers, not
  references); the holder's bytes are the same in every sample, so no flag says which face `Distance 1` runs along.
  The stored bodies say it: of an edge's two planar faces A and B, A is the reference when a stored point of A lies
  `Distance 1` (times the sine of the faces' angle) from plane B and a stored point of B lies `Distance 2` from plane
  A, and not the other way round; all edges must agree on one face, which becomes the chamfer's `face` (a stored-face
  pick, moved with the component's placement). 5905's pin tip: the end face takes 0.115 mm, the sides 0.3067 mm
  (the other way round, 2 x 0.3067 mm would not fit the 0.46 mm pin).
* 5905 then stops at `MirrorComponent1` (`71BBEDD5`): it names an occurrence (`CE2913AA`) of the header component,
  a mirror plane (a `5A1BF548` selection), a CreateComponent and a `DerivedInstance` (`66E375EA`, an occurrence
  `2D6E13A1` of a new `CE2913AA`): a mirrored copy of the whole component. Not converted.
* 5967 then builds every header (the stored pin states 0.6504 and 0.6774 mm3 and the socket tool 0.7493 mm3 are
  explained) and stops at the board: `Extrude2` (the board's PCB cut) changes the volume by 835.9874 mm3 while the
  537 stored tools no other feature explains hold 687.7067 mm3. The 537 tools are exactly the 537 circles of the
  board's Sketch2 (sum of pi r^2 x 1.57 mm = 687.7067 mm3; no two overlap); where the other 148.2807 mm3 come from is
  not found.

### Derived Base Feature `B1E5B971` (3502)
* The bodies of another design derived into this one (3502: the display's glass, polarizers and flex of a separate
  design, seven features, each behind a component activation). The record is the creator of `D26351F0` body links
  whose bodies name a `.smbh` state list, exactly as a Base Feature's: the reader takes them the same way, as stored
  bodies (the source design is not in the file, so there is nothing to rebuild them from). Two of the seven make no
  body and are skipped.
* 3502 then stops at `Bend1` (`1CF7B9E3`, a bend of the flex body in Component7, each bend right after a sketch of its own): not decoded.

### The magnetic connector (5358, 5360): faces as profiles and extents
* **Sweep path records of 45 bytes.** The path's `E2CEFD18` records hold `u64 sketch data id`, `u64 crv_primary_id`
  as their last 16 bytes: at 0x21 in the 49-byte records, at 0x1d in the 45-byte ones (tag of three decimal places)
  of 5358, as the revolve axis reads them. The sweep read 0x21 always.
* **A face as a sweep profile.** `Sweep - Pin Right Angle` has no `4BD53E5A` profile but a `5662F619` face selection:
  an end cap (generator `1` or `2`, one operation) of an extrude of the pin. Such a face is that extrude's profile on the
  cap's plane: the tree gets a copy of the extrude's sketch with the curves of the extrude's regions, moved along
  its normal to the plane through the path's free end nearest the cap (as a sketch profile off the path is swept
  from the path's start plane). A face another feature renamed, a side face or more than one face is refused.
* **From a face to a face.** `Extrude - Pad Clearance` and `Extrude - Pad` start at a face (`ProfileOffset` 0 with a
  bounded-face recipe) and end at one (`Side1Offset` 0, a second recipe): z 4.0 mm (Housing's top, `2 [302, 311]`)
  and z 3.6 mm (`2 [334]` / `2 [365]`). The first recipe's selection stands before the `ProfileOffset` holder in the
  record, the second's before `Side1Offset`'s; with both offsets 0 and no taper the solid is the slab between the
  two planes whichever is the start, so the first is read as the start and the second as the end.
* **An empty fillet.** 5358's off-timeline `Fillet1` in Housing (`7063`) references only the design root: no set, no
  value, no edge. It changes no body and is skipped.
* **Edges of two bodies on one another.** `Fillet - Magnets` rounds a magnet's top circle, which lies on the rim of
  the magnet's pocket in the housing: the edge's points match an edge of both bodies. A fillet or chamfer now names
  the body its edge selections' body links lead to (`on_body`, ADR 0042 amendment), which decides there.
* 5358 and 5360 then stop at `Fillet - Pin`, which does not build: the sweep runs along `Sketch3` of the root
  component (plane origin x = -1.27 mm on `WorkPlane1`) taken at its own coordinates in the Pins component (the rule
  of 4863, `in_own_component`), while the stored blend edge at the sweep's end lies at x = -3.81 mm, the first pin:
  2.54 mm (one pitch) apart. Which frame the path is in at the sweep (before the joints `Assemble2`/`Assemble3` move
  the occurrences) is not established.

### Inserted designs with sweeps (4862)
* The sweep path fix above makes the inserted header of 4862 (adafruit set) readable: its tree used to fail on the
  sweep path, so the insert came in as the bodies its `.smb` blobs store. A sweep and a combine hold no point or
  direction of their own (profile and path are sketches, a combine names bodies), so an insert placed off the origin
  may now move them. 4862 then fails the step check, where it came in as history before: the features give
  9168.1416 mm3, the stored shapes 9454.3366 mm3, and the board's `Extrude2` (84 holes, whose stored tools hold
  92.0987 mm3) changes the volume by 260.8520 mm3. 5967 shows the same with its rebuilt headers (148.2807 mm3 more
  than the 537 holes); both boards' holes are where the header pins stand. Where the extra volume comes from is not
  found; the cut itself acts on the board body alone.

## 31. Round 20 G: end bodies by solid, symmetric measures, body-pattern copies, origin axes (2026-11-11)

### End bodies compared solid by solid (1463, 3422)
* A new-body extrude of regions that do not touch is one body of several solids here and one body per solid in the
  file: 1463 Extrude2 (16 pads of 42.5 mm3, one body of 680 mm3 here), 3422 Extrude2 (six keys). The step check
  already took each solid as a state; the end-body confirmation compared whole bodies, so the 16 end bodies of
  42.5 mm3 met no finished body. The finished bodies are now listed as the file stores them (each body that is
  several solids apart as its solids, the same split the states use). Both designs then confirm their last
  features (1463 Chamfer1, 3422 Shell1 and its cuts) by the end bodies, volume for volume, and come in as history.

### Symmetric extrudes: whole length or each side (355)
* After the extrude's direction, a `u32` says how a symmetric (extent 3) distance is measured: **1 the whole
  length, 2 each side's**. It is followed by zeros (11 in the records with plain references, none in those with
  typed ones; with the `u32`'s three high bytes, 14 and 3) and the feature's selection list (`01 00 00 00`, a
  reference). Read in every symmetric extrude of the corpus (18 records, versions 266 to 447). Each side's:
  6299 Extrude2 (2) (11 mm; its stored tool is 22 mm long, its profile 6.4855 mm2 x 22 = 142.6806 mm3 stored),
  324 Extrude1 and Extrude11 (16.5 and 6 mm; bodies 33 and 12 mm long), 355 Extrude4 (9.3 mm; its tool spans
  x -9.3..9.3). Whole: 4632 Extrude1 (12.4 mm; body 35 x 38 x 12.4 mm = 16492 mm3) and the L-Bracket (110 mm).
  The 6114/6124/6126 joins carry 2 as well (not otherwise checked: those designs stop earlier).
* The tree's symmetric distance is the whole length: each side's is doubled (`symmetric_measure` in the JSON).
  A symmetric extrude whose measure is not found is refused. 355's Extrude4 now cuts its whole tool through the
  body (387.1469 mm3, the end body) and 355 comes in as history.

### Copies of a body pattern were not counted (4863)
* A body pattern with `merge: false` gives each copy a body whose feature is the copy's name (`P_copy1` ..); the
  tree import's "bodies the import made" filter matched feature names only, so the copies were left out of the
  imported volume. The step check measures each step by that volume: from R-Pattern1 on, a single-step build
  lacked 19 x 4.7301 + 19 x 4.1566 = 168.85 mm3 while a run of new bodies built as one counted them, so 4863's
  steps jumped by that amount and its board cut (Extrude2 (3), 86 drill tools, 92.7152 mm3) read as 261.5620 mm3.
  The copies are now counted. 4863 comes in as history.

### Blends of edges on several bodies (1145)
* 1145 Fillet1 rounds an edge of the base (Extrude1, r 8.2 at z 0) and one of the cap (Revolve2, r 6.5 at z 18.2)
  in one feature. The tree import now writes one blend per body, same size (`Fillet1`, `Fillet1_2`).

### Revolve about an origin axis (3350)
* The axis selection (`5A1BF548`) names a `90055C05` record whose body is the `u64` id of the axis record; for an
  origin axis that is a `19031004` record named `X`, `Y` or `Z` holding a point and a unit direction (cm, the
  component's frame) at its body, then two display doubles (0.175, 2.25). The `50AEE8B4` sibling under the
  selection repeats the display segment (point (0, 0, 0.175), vector (0, 0, 2.075)). 3350's three revolves turn
  about Z; they now read as revolves about the origin and the Z direction.
* The same `90055C05` reference names sketch planes (`8D424E29` `XY`/`XZ`/`YZ`) and features elsewhere: only the
  `19031004` axis records are taken as axes.

### The end bodies now refuse a history that differs from them
* Where the file stores end bodies, they confirmed the features after the last stored state but refused nothing:
  a feature between two stored states whose result no stored solid shows was never compared. With 1145's modeled
  thread left out (a scratch experiment), every state and tool was explained and the design came in as history
  although its end bodies are 2435.8962 and 436.9949 mm3 where the model has 2742.2136 and 476.7911. End bodies
  that are not the finished bodies now refuse the history. No design that came in as history before has such
  end bodies (checked on the whole corpus run).

### Not reached
* 1145 and 3350 stop at their modeled threads (section 29). With the threads left out 1145 is refused by its end
  bodies (above: the base differs by 306.32 mm3 and the cap by 39.80 mm3, so not only the base's thread is
  missing) and 3350 stops at Fillet2 (two edges
  not resolved, then Thread2, an internal M16x1 of the whole face). Modelling Fusion's thread form and its ends to
  the check's 2e-5 is not attempted: how the thread runs out at its offset and at the face's end is not known.

### Result
* adafruit-more: 120 of 150 as history (355, 1463, 3422 and 4863 gained; none lost; 116 before). adafruit: 13 of 18
  (its copy of 4863 gained). 1145 and 3350 still stop at their modeled threads.


## 32. Round 20 H: the board cuts under inserted headers (2026-11-11)
Reader: `apply_with` in `gusset-commands/src/tree_import.rs`; check: `fusion_states_check` in `import.rs`.

* **Not the cut, the measure.** The excess of section 30 was no geometry: the board's `Extrude2` of 4862 removes
  92.0987 mm3 and that of 5967 687.7067 mm3, exactly their stored tools. The step check takes the model's volume
  after a feature built on its own from the tree import's `volume_mm3.imported`, which summed only the bodies named
  after an imported feature, and the volume after a run of body-making features (sketches, new-body extrudes and
  sweeps) from the sum of all bodies. A pattern with `merge: false` (the header's pin rows `R-Pattern1`/`R-Pattern2`)
  makes bodies named `<pattern>_copy<i>`: they were in the run sums and missing from the single-feature volumes, so
  the first single feature after a run (the board cut) seemed to remove them. 4862: 19 copies of 4.7301 mm3 and 19 of
  4.1517 mm3 = 168.75 mm3, the whole excess.
* The imported volume now counts the copies of the import's own patterns (`gusset_doc::pattern_copy_of`; the same
  fix round 20 G made for 4863, 977d0fe, which this round takes). 4862 and 5967 come in as history:
  4862 with 64 states and 88 cut tools, 5967 with 114 states and 543 tools.


## 33. Round 20 I: mirrored components, thin extrudes, the sheet-metal Bend (2026-11-11)
Reader: `fusion_tree/mirrorcomp.rs`, `fusion_tree/thin.rs`; importer: `fusion_stored_bodies` in
`gusset-commands/src/import.rs`.

### MirrorComponent `71BBEDD5` with its DerivedInstance `66E375EA` (4343, 5905)
* The record names the occurrence it mirrors (`CE2913AA`, whose first `E03784ED` is the source component; the record
  names that component too), the mirror plane (a `5A1BF548` selection -> `CC54ECAD`, a construction plane of the
  component that holds both occurrences: 5905's root WorkPlane1, y = 0; 4343's plane x = 0 of `PCB Component`), the
  new component's `CreateComponent` (-> `5D15E992` -> the new `E03784ED`) and the DerivedInstance. The DerivedInstance
  names a `2D6E13A1` holder of the new occurrence (`CE2913AA`) and stores the occurrence's 4 x 4 matrix (cm) again,
  as the occurrence record does (a rigid matrix at byte 48 of the DerivedInstance, 213 of the occurrence). Both
  occurrences sit in the same `904F885E` list (their parent's).
* The new occurrence's matrix is proper (5905: diag(1, -1, -1), move (-24.12, 8.9, 1.54) mm; the header's own is the
  identity, move (-24.12, -8.9, 1.54) mm), so the reflection is in the new component's own bodies: in its coordinates
  they are the source's moved by `T_new^-1 . M . T_source` (the parent's own placement cancels out). For 5905 that is
  the reflection in z = 0 of the component's frame, and the stored blob of the new component (`10b22090`: the
  header body z -3.8..-0.92 mm, pins z -1.15..0) is the header's (body z 0.92..3.8, pins 0..1.15) reflected in it.
* The tree writes one body mirror (`merge: false`) per body the source component has at that point, in the new
  component, with the plane of that map; `json::place` computes it again where the importer puts the two components'
  bodies (a map that is not a reflection in a plane is refused). The DerivedInstance is skipped with its mirror.
* 5905's new blob holds 60 pins where the header builds 40 (at every second pitch one pin is stored twice); the step
  check takes each duplicate as the same state.
* **4343** now comes in as history: the mirror copies `FPC-22FLZ-SM2-TB v1`'s two Base Feature bodies. Two importer
  rules needed care, neither a looser check:
  * A mirror into a new body leaves its target as it is, so it no longer counts as "a feature that changes a stored
    body that is already a finished body" (the copy is what the step check holds against the stored solids).
  * `Base Feature1 (24)` of `4UCONN_18510` is one ACIS lump of five shells (the outside listed last): the outside
    (183.819 mm3 alone), cavities of 0.365 (a sphere), 0.464 and 0.113 mm3, and a 0.009 mm3 lens (a sphere cap) that
    OCCT's STEP reader does not take for a cavity of the outside and makes a solid of its own: the body comes in as
    two solids (182.876 and 0.009 mm3; the file's `.smb` gives the same two). A stored body that comes in as several
    solids and that no other feature names is now that many stored bodies of the tree (`<name> piece <i>`).
  The step check: 270 stored solids are states of the rebuilt model, 155 are cut tools, 2 are dropped inputs.
* **5905** builds the mirror (the features' volume rises by the mirrored header, 739.27 mm3) and then stops where 5967
  does: the board's `Extrude2` changes the volume by 283.2097 mm3, its 180 stored tools hold 258.4962 mm3 (section 30,
  not this round's). A wrong plane gives the same message, so 5905 does not confirm the mirror on its own; 4343 does.

### Thin extrude (5981, 6299, 6126)
* An extrude with a `Side1WallThickness` holder (`D91D429C`) has no `4BD53E5A` profile: it selects sketch curves
  (`5A1BF548` -> `E2CEFD18`, sketch and primary id in the last 16 bytes, as a sweep path) and each selection also names
  the curve's own geometry in component coordinates (`50AEE8B4` a line: point and vector, cm; `B0B3FA59` an arc: centre,
  normal, reference direction, radius, sweep). Just before the reference to the thickness holder: `u8 1` (thin) and
  `u32` the wall's location (5981 Extrude3: 2, 0.6 mm; 6299 Extrude7/10: 2; 6299 Extrude14 and 6126 Extrude5: 1).
  Normal extrudes have zeros there.
* Location 2: the wall lies to the left of the chain, run the way the first selected curve runs (a line start to end),
  about the sketch normal. 5981's stored edges show it: the wall's faces meet at the chain's own points (the arc's
  ends) and 0.6 mm outside the arc (radius 1.4 -> 2.0 mm). Location 1 is read as the other side by symmetry but not
  confirmed (refused, "not confirmed by a rebuild yet").
* The tree writes the wall's outline as a sketch of its own (`<name> wall`: the chain offset by 0 and by the wall,
  closed by two caps; tangent joins, corners only between lines, mitred) and the extrude extrudes its region. No new
  operation of the document language.
* The wall's faces are named by generator: `10 + k` the two sides along the k-th selected curve, `10 + n` the cap at
  the chain's start, `10 + n + 1` at its end (5981 and 6299, three curves each). The extrude is two operations (5981:
  363 and 364); the side on the chain carries both, the other side only the first. The caps and straight sides are now
  modelled, so blends of their edges resolve before the faces are cut away (5981's Fillet3 rounds the slot's four end
  corners, r 0.3 = half the wall: the stored states have no face 13 or 14 left).
* 5981 then stops at `Chamfer2` (0.4 mm): its edge joins Extrude4's face `-1` (a cap of the post below the slot) and
  the wall's arc side merged with the post's cylinder (`("11", [363, 364]), ("3", [367])`); neither the face `-1` nor
  a wall side along an arc is modelled, and the stored states no longer hold the edge. After it `Chamfer6` names faces
  `-3`, `-6`, `-8` of the wall (not decoded) and `Combine1` (value 1) follows; Combine1 is therefore neither confirmed
  nor refuted by a rebuild.
* 6126's `Extrude5 (2)` is a thin extrude of three separate lines (two collinear, one crossing): not one chain, refused.
  6299 stops at `Chamfer1 (2)` first, as before.

### Bend `1CF7B9E3` (3502): read, not converted
* A sheet-metal bend of the flex body (a Derived Base Feature body) along a sketch line: the record names a face
  selection (`2CA5A1CD` -> `5662F619`, the face the line is on), the line (`5A1BF548` -> `E2CEFD18` -> `50AEE8B4`), a
  `69EE2FA7` with a second face (the side that stays), an `88277bc9` holding `BendAngle` (-90 deg) and a selection, the
  `BendRadius` holder (0.45 mm), and an `E75C86D6` that names the sheet-metal rule (`B42ED662`, `Thickness` 0.05 mm).
  Bend1 and Bend2 each follow a sketch of their own.
* Gusset has no bend (cutting a body along a line and turning one side about an axis through a cylindrical zone that
  keeps the neutral length); adding one is a kernel operation of its own with its golden test, beyond this round.
  3502 stays at Bend1.

### Result
* adafruit-more: 117 of 150 as history (4343 gained, none lost); adafruit: 11 (none lost). The thin extrude reading
  touches only extrudes with a wall thickness (5981, 6299, 6126, checked one by one after it).

## 34. Round 21 J: body patterns of new bodies, multi-solid mirrors, suppressed copies, corner blends (2026-11-11)
Readers: `fusion_tree/json.rs` (patterns, mirrors, shells, extrudes); blends: `blend_by_corners` in the shim.

### A pattern of a new-body feature is a body pattern (2190)
* 2190 R-Pattern1 repeats Extrude4, an extrude that made a body of its own, four times and makes three bodies. The
  tree's feature pattern repeats what a feature added to or cut from a body, so it refused ("Extrude4 made a new
  body"). A pattern of one extrude, revolve or sweep with operation new that made one body, written just after it
  (no feature between has changed its body) and making one body per copy, is now a pattern of that body
  (`merge: false`), as for a mirrored body (5905). 2190 comes in as history.

### A mirror of a body of several solids (3101)
* 3101 Sweep1 makes two bodies (the two legs) and Mirror1 selects both and makes two. The tree has Sweep1 as one body
  of two solids, so the mirror "of 1 body making 2" was refused. Where the selected bodies of each feature are all the
  bodies it made (a new-body extrude, sweep or revolve) and the mirror makes one copy per selected body, the tree
  mirrors that feature's body once (`creates_bodies` = its solids).
* Its Fillet2 (radius 0.2 on the top arcs of a 0.2 mm high ring cut by four slots) takes the ring's whole outer
  wall; OCCT's blend fails. The corner fallback now takes arcs as well as whole circles: the corner section is revolved
  past the arc's ends and trimmed by the plane that ends the arc at each end (a plane face that crosses the arc, with
  the material on the arc's side, within 60 degrees of the plane through the axis); the removed volume must be the
  trimmed corner's. With a radius of 0.1999 OCCT's own blend builds and the states agree, which showed the result
  wanted before the fallback was written. 3101 comes in as history.

### Suppressed copies of a pattern (512)
* The instance list `7493A99D` of a pattern ends with `u32 n` and n `u32` instance numbers (1 is the original), the
  instances left out: 512 C-Pattern1 (4 instances about Z) ends `02 00 00 00 03 00 00 00 04 00 00 00` (copies 3 and 4
  left out; it makes one body), the Crickit's 12-instance pattern `02 .. 04 .. 0A` (9 bodies, section 13). All other
  patterns of the two sets end with `n` = 0 and make count - 1 bodies. A full-circle body pattern with copies left out
  is written as one pattern of two per kept copy, turned by (k - 1) / count of a turn (512: one copy at 90 degrees).
* 512 Shell1 removed the cap at the sketch origin's projection (0, 0, 1.6), a point of the board's top: the board
  was shelled. A shell's cap is now picked by the stored cap's points, as a draft's (section 28).
* 512 Fillet1 rounds the top of a window through a 0.4 mm wall to a half circle (two concave edges, radius = half the
  width) and Fillet3 rounds the ends of a 0.6 mm groove that arms cross (16 concave edges between the arms' planes and
  the groove's cylinders, radius 0.3). OCCT fails on both. The corner fallback now fills the corners of concave edges
  (straight edges between perpendicular planes, and straight edges between a plane and a cylinder about an axis along
  the edge: the round touching the line and the circle on the open side, in section), the added volume checked
  against the corners'. 512 comes in as history.

### Chamfers whose corners meet (258)
* 258 Chamfer1 chamfers the four top edges of a 1 mm step by 1 mm: OCCT fails and the corner fallback refused,
  because the corners of two edges meeting at a vertex overlap there and their volumes were summed. For chamfers the
  removed volume is now checked against the corners' union, and corners of edges that do not meet must not overlap
  (a blend wider than its face stays refused, as the existing test of radius 4.5 on an 8 mm end shows).
* 258 then stops at Draft1: the face drafted is the pin's start cap, which lies on the neutral plane (the chamfer face
  the pin was sketched on), so the reader's pick is ambiguous and how Fusion tilts a face in its own neutral plane is
  not known. Not converted.

### Joins that make bodies, and an extrude of a face (610)
* 610 Extrude5 joins to Extrude1 and makes two bodies (parts of its tool that touch no body); Extrude6 then joins to
  the bodies of Extrude1 and Extrude5 and makes one, Extrude7 and Extrude8 act on all of them. The tree's join keeps
  such parts in the target's body as solids of their own: a later list that names the target's body and bodies such
  joins made now names that one body (lists without the target's own body are left as they are).
* 610 Extrude8's profile is a face: a bounded-face recipe naming Extrude1's end cap `2` bounded by Extrude2's side
  `4`. Extrude1 extrudes two regions of Sketch1 by 1 mm and Extrude2 joins one of them by 2 mm, so the cap left open
  is the other region. An extrude whose profile is an extrude's cap (one operation), with later joins from the same
  sketch reaching past it whose sides bound the face, is now the regions those joins leave open, started at the cap
  (moved sketch, as for offset starts). 610 Extrude8 reads; it then stops at Fillet3 (one edge between Extrude1/2's
  merged side `5`+`3` and Extrude2's cap is not resolved).

### Not reached
* 4431: both Base Features have their DM bodies in a later state of the component (JST-PH3's five bodies, one of them
  joined to another by Combine1, so the `.smbh` list has four; the switch's body is followed by two later extrudes'
  and is cut later). Each DM body and its PM body name each other and the PM list names a `.smb`, but that blob holds
  more bodies than the PM list (6 for 5 and 12 for 1, the component's other bodies among them), so the place of a PM
  body in it is not known without pairing by geometry (as `fusion_dropped_inputs` does). Not converted.

### Result
* adafruit-more: 124 of 150 as history (2190, 3101 and 512 gained; none lost; 121 before). 610 now stops at its
  Fillet3, 258 at Draft1, 4431 at its Base Features. adafruit: 13 of 18 (unchanged).

## 35. Round 21 K: outlines that touch themselves, circles drawn twice, cuts of several bodies (2026-11-11)

### A notch whose corner meets the outline (2348)
* 2348's `Board` sketch: the board's rounded outline (65 x 56.5 mm, r 3 corners), a slot, and a notch whose last
  line runs from (0, 35.23) on the board's left edge to (0.1, 18.23), just inside it. The notch is a hole that
  touches the outline in one point, so the board's region is one loop passing that point twice (four zero-length
  lines on x = 0 lie on the edge as well and drop out). Swept, the point becomes a vertical edge four side faces
  share. The extrude's fast closed-shell test wanted two faces per edge and refused it; OCCT's analyzer finds the
  prism valid. The stored board is 5570.2218 mm3 = (3664.774 - 83.72 - 33.14) mm2 x 1.57 mm, this solid.
* The shim now gives a shell whose edges are all used an even number of times to the full analyzer (odd counts are
  still refused). 2348 comes in as history (147 states, 245 cut tools).

### A circle drawn twice cuts its copy half way round (858, revises section 25)
* 858's `Extrude5` (the double-flat shaft): two copies of a circle r 2.5 mm (seam at the sketch's +x), a 5 x 3 mm
  rectangle whose short sides are tangent to the circle at its seam and opposite it, and whose long sides cross it
  at +-36.87 and +-143.13 deg. The loop lists both copies as pieces 1, 3 and 4 of 5 and the two long sides' middle
  pieces. Five pieces with the region's arcs 1 (the right arc, round the seam), 3 and 4 (the left arc in two)
  mean: the four crossings cut, the seam does not, the tangent points do not, and one more cut lies at 180 deg.
* 324's flat (a chord and the circle drawn twice, the cap piece 2 of 3) needs one cut besides the chord's ends,
  anywhere from 127 to 360 deg: the seam fits, and so does 180 deg. One rule fits both: **a closed copy cuts
  the curve at the point half way round from the copy's seam**. An open curve lying on a circle (an arc) still
  cuts it at its ends.

### Regions that touch themselves, united (858)
* 858's `Extrude1` takes eight regions of a sketch where a circle r 4.5 mm is tangent to the tab's top line: the
  region above the line, outside that circle, passes the tangent point twice. Uniting the regions computed each
  member's area by chaining its edges again, which fails at such a point, so the regions stayed apart and the
  sweep was refused. The area now comes from the member's loops as they are.

### An extrude cut through several bodies (858)
* `Extrude6` cuts the case (`Extrude1`) and the shaft (`Extrude4`): its body links name both. The tree's extrude
  now takes a list of bodies for a cut, and the importer writes one cut per body (`Extrude6`, `Extrude6_2`) with
  the same profile and extent; both are one step of the state check, as the blends of section 31 are.
* 858 then stops at `Fillet3`: its edge (z -19 mm, on the r 14 mm circle from (7.3, -11.93) to (13.54, -3.5))
  is a piece of the side of `Extrude2` that Fusion keeps apart where the profile's curve was cut (at the tab's
  line); Gusset's join unifies the coaxial pieces of the circle into one face, so no edge of the model is that
  piece. Without unifying (`refine: false` on every extrude, an experiment) `Fillet3` builds but `Fillet2`'s edge
  (x -7.3, from z 0 to -17) is then split at z -0.8 where `Extrude1` and `Extrude2` meet, an edge Fusion does unify.
  So Fusion merges a tool's faces with the body's across the boolean but keeps the tool's own faces apart where its
  profile curves were cut. Gusset has no such mode: not attempted. After it come `Fillet4` (0.75 mm on an edge of
  1 mm), `Extrude13` (an arc of its profile does not build) and `OffsetFaces2`.

### A face of an extrude by a numbering the stored faces confirm (6299)
* `Chamfer1 (2)` names the edge between side `8` and cap `2` of `Extrude1 (2)` and nothing else. The extrude's
  profile is an outline of 16 pieces and a round hole; the generator numbering (section 13) was not used when a
  profile has holes. Generators 3 to 6 of this extrude name stored faces whose pieces are known, and all four are
  the pieces the numbering gives: the numbering is now used for a profile with holes where at least two stored
  faces confirm it and none contradicts it.
* An edge named by a side and a cap of one extrude and by no bounding face is the whole side at the cap: its middle
  is the side's chord middle moved onto the cap (tried after the model and the stored bodies). A model edge that is
  only part of it does not match that middle, so the import stops there rather than guess.
* 6299 then stops at `Extrude17`, whose profile is a face: side `5` of `Extrude` op 326 as a later feature (op 425)
  left it, a plane parallel to the extrude's direction. Not an extrude's cap: the face's outline is not a sketch,
  and no extrude of a body face exists in Gusset. Not attempted.

### The magnetic connector's pin (5358, 5360)
* Section 30's frame question has its answer in the stored blend: the sweep body ends at x = -3.81 mm and the model's
  sweep does too (Fusion sweeps the profile, off the path by -2.54 mm along the path plane's normal, with that
  offset kept, as OCCT's pipe does). `Fillet - Pin` failed for another reason: the profile (the cap of the pin's
  extrude, a disc r 0.45 mm and the ring out to 0.5 mm) was copied with both circles, swept as two regions, and the
  end kept the circle between them; the 0.1 mm blend of the rim ran into it. Curves all of whose pieces bound two of
  the extrude's regions lie inside the cap face and are now left out of the copy. `Fillet - Pin` builds.
* 5358 then stops at `Chamfer2` (two distances, on two sides of `Extrude3` next to the edge `Chamfer1` blended):
  OCCT refuses the two-distance chamfer of both edges at once (they share the cap; separate passes are tried only
  for independent edges) and the fallback that cuts the corners away leaves a face with no origin (`face 20 has no
  usable origin`), likely a cutter end face where the corner meets `Chamfer1`'s slope (not examined further).
* 5360 (three pins) builds every feature and reaches the step check, which refuses it: a stored solid is neither a
  state of the model nor a tool the check explains (not examined this round).

### Result
* adafruit-more: 122 of 150 as history (2348 gained; none lost; 121 before). adafruit: 13 of 18 (unchanged).
* First blockers now: 858 `Fillet3` (tool faces Fusion keeps apart), 6299 `Extrude17` (a body face as profile),
  5358 `Chamfer2`, 5360 the step check.

## 36. Round 22 L: modeled threads, from the stored bodies (2026-11-11)

### What Fusion's modeled thread is
Read from the end bodies of 1145's and 3350's history blobs (`.smbh`; the `.smb` holds the state before the thread):
* The thread first moves the **whole** selected cylindrical face to the thread's diameter: an external thread to
  half its major diameter (1145: the rod of radius 8.2 mm becomes 7.942 mm over all 13 mm, not only the threaded
  length; 3350: 9 mm becomes 7.942 mm), an internal one to half its minor diameter (3350's Thread2: the 8 mm hole
  becomes 7.51775 mm). Blends made later are made on the moved face (1145's Fillet1: a torus of 7.342 + 0.6 mm).
* Then it cuts the groove of the basic profile. Faces: two flanks (`helix_spl_line` ruled surfaces), the root
  (a cylinder strip between two helices, one ACIS face winding nine turns), the crest (the moved face, now also
  bounded by helices). Profile in an axial plane, P the pitch, h the radial depth (major - minor) / 2, w = h tan of
  half the flank angle: the crest flat (material at the major diameter of an external thread, at the minor one of
  an internal one) is P/8 wide; the root flat takes the rest (1145: 0.2151 mm = P - P/8 - 2w). The flat at the
  major diameter is centred P/16 before the face's start (along the face's axis, which runs the way its extrude
  went) modulo P, the flat at the minor diameter 7P/16 after it, for the external and the internal threads alike;
  at angle 0 (the sketch's x) the groove of 1145 spans [0, 7P/8] at the major diameter. All three ends are at whole
  pitches from the face's start, so the phase could also be anchored at the thread's start; volumes and boxes do
  not depend on it.
* The thread ends **square**: the groove stops on planes across the axis, at the offset (1145: a planar face at
  z = 4 mm, its loop two flank-plane curves, an arc at the major and one at the minor radius) and at the face's
  end (the end face carries the groove's section). No run-out, no chamfer, no taper.
* Where it lies: `ThreadOffset` and `ThreadLength` from one end of the face, clipped to the face (3350's Thread1:
  12 mm from 5 mm on a 14 mm face is 9 mm). The `u32` after the side byte says which end: 2 in 1145 (the offset
  from the extrude's start, the sketch plane), 1 in 3350's Thread1 (from the extrude's end cap); only these two
  samples have an offset, so the reading is consistent with both but not proven. Full-length threads (3350's
  Thread2, 324) have 2. All four samples are right-handed.
* `helix_spl_line` (fixed in `step.rs`): after `centre major minor pitch taper axis null_surface null_surface
  nullbs nullbs` come three numbers (rise, twist, climb): the point at (u, t) is the helix with its radius vectors
  scaled by 1 + u rise and moved u climb along the unit axis (1145: rise 1.17499 = 0.5715 mm / 7.3705 mm per cm of u,
  climb 0.5 = tan 30 deg x rise x radius; every edge of the flank lies on it). The reader had taken the axis for
  the ruling, so every flank came in as a band on the root cylinder.

### Measuring it
* 1145's threaded body, analytic: rod 2576.0367 - fillet 3.7901 - groove 135.877 (9 mm x the groove's section
  A 2 pi r_c / P = 15.0975 mm3/mm) = 2436.3700 mm3. Stored, converted: 2435.03 at first. Two errors:
  - the helices were fitted with 16 Hermite pieces a turn from each curve's own start, so an edge lay 0.5 um off
    its flank and the flank's volume was 2.6e-5 off: pieces now break where the helix crosses fixed half-planes
    about its axis (64 a turn), so curves on one helix share their pieces;
  - OCCT's adaptive volume quadrature (the shim's choice for shapes with freeform faces) integrated the root,
    a cylinder face winding nine turns, as one period: 189.57 against 220.25 for that face. A shape with a cylinder
    or cone face that one of its edges goes 1.5 turns round now gets the default quadrature, which agrees with the
    mesh and Gauss-Kronrod (VolumePropertiesGK) to 2e-6; Gauss-Kronrod itself took over ten minutes on 5753's
    stored screws, and a face's pcurve box alone (whole cylinders pass one turn a little) sent 2219 there too.
    Converted now: 2436.3698 mm3.
* The rebuild with Gusset's exact helical sweeps: 2436.3740 mm3 (1.7e-6 from the stored body).

### The tree
* `thread.rs` turns a modeled thread on the side of a whole extruded circle (axis along a world axis through the
  origin) into tree features: an annulus extrude (cut for an external thread, join for an internal one) over the
  face's length, then the groove: one-turn helical sweeps of the profile (run 0.03 P / tan(half angle) past the
  surface it cuts, so no edge lies on it), the first a pitch below the thread's start. A sweep that reaches past an
  end of the thread inside the face is made as a body, cut off by a disc of that length (`combine` intersect) and
  then cut from the body; one that reaches past the face's end is cut directly (it runs out through the end face,
  as Fusion's does).
* OCCT and long grooves: one sweep of eleven turns cut 1145's rod not at all (no error, the body unchanged) when the
  groove passed through the top face; so did some phases of the same groove (the result depended on where the
  rod's seam was); two-turn sweeps failed once at the top face ("invalid solid"). One turn a sweep cut exactly in
  every case tried.
* Fillets and chamfers after the thread pick their edges by points taken from the extrude: an edge whose points all
  lay on the face before it moved is moved with it (`moved_edges`).
* Not converted: threads on other faces (3350's Thread2 is on a revolve's face: "its face is not the side of an
  extruded circle"), on an axis that is not a world axis through the origin (324: holes off the Y axis), a face
  narrower (external) or a hole wider (internal) than the thread, or an unknown offset end code.

### Result
* adafruit-more: 130 of 150 as history, adafruit 13 of 18: the same designs as round 22 M's tally (none gained,
  none lost). The threads themselves now rebuild:
  - 1145: Thread1 builds (2436.3740 mm3 against the stored 2436.3698) and Fillet1 rounds its moved edge; the design
    is refused by its cap: Revolve2 (a revolved degree-5 spline, filleted) gives 476.79 mm3 where the end body is
    436.99 (39.80 less, the same gap as with the thread left out in round 20; not a thread matter).
  - 3350: Thread1 builds; the design stops at Thread2, an internal thread on a revolve's face (the face reader
    knows only extruded circles; the form above holds for it: the 8 mm hole is filled to 7.51775 mm and the groove
    cut to 8.0761 mm).
  - 324: Thread2 is refused before its geometry ("it is not on one face": its selection is not one face recipe,
    the thread is patterned by C-Pattern1 on holes off the origin); Split3, Combine1 and unresolved fillets follow.

## 37. Round 22 M: the magnetic connectors' last steps (2026-11-11)

### 5360: the step check (three faults, each shown by the stored shapes)
* **A sweep's tool.** The 3.8705 mm3 stored solid (x -3.04..-2.04, y -3.85..0.5, z -1.9..0) is the tool of
  `Sweep - Pin Right Angle`, which joins the pin: the sweep adds 3.7920 mm3 and its tool holds 0.0785 mm3 more, the
  pin's bottom disc (`Extrude - Pin Bottom`, r 0.5 x 0.1 mm) that it overlaps. The check knew no copy count for a
  sweep, so no rule could give it the tool. A sweep that joins or cuts now makes one tool (one profile), as a
  one-profile extrude does; the placed-tool rule then finds 0.0785 mm3 of it in the pin before and all of it after.
* **A blend edge on two bodies at once.** The off-timeline `Fillet1` (component Magnet, body link
  `Extrude - Magnets`) rounds the left magnet's rims, which lie on the rims of its pocket in the housing. Each pick's
  nearest edge was the housing's (the first body searched), so all picks landed on one body and the body link was
  not consulted: the housing lost 0.0096 mm3 instead of the magnet (25.5176 -> 25.5081 mm3, as the end blobs store
  it). A pick whose nearest edge lies equally near in two bodies now lets the body link decide, as picks on two
  bodies already did.
* **The end blob a body move leaves.** The design moves the housing and the magnets out of the root
  (`Body->Comp1 - Housing`, `Body->Comp2 - Magnets`, `CutPasteBodies1 - Magnet 2`). Its four `.smbh` blobs hold: the
  pins (3 x 7.1176), the housing (172.4527), the two magnets (2 x 25.5081), and a fourth blob of 174.9762, 25.5176
  and 25.5081 mm3: exactly the model after `Fillet - Magnets` (226.0019 mm3 in all, body for body), the root's
  bodies as they were when they left it. Where the tree moved bodies between components (a skipped `Body->Comp` or
  `CutPasteBodies`), an end blob whose bodies are one for one the bodies (or solids) of the model after an earlier
  step, adding up to that step's whole volume, is now left out of the end bodies; the other blobs must still be the
  finished bodies, volume for volume, and their box still bounds the model.
* 5360 comes in as history: 9 stored solids are states, 12 cut tools, and the end bodies are the finished bodies.

### 5358: a chamfer next to a chamfer's slope
* `Extrude3` is a tab under the housing (x -1.25..1.25, y 1.9..2.5, z -0.5..0). `Chamfer1` takes 0.3 mm of its cap
  and the whole 0.5 mm of its front (y 1.9); `Chamfer2` takes 0.2 mm of the cap and the whole 0.5 mm height of both
  sides (x +-1.25), whose cap edges now run from the slope (y 2.2) to the back. OCCT refuses both edges at once and
  each alone (the chamfer reaches the side's far edge); the corner fallback cut each corner only along its edge,
  which left a triangle across the edge's end at y 2.2 (the body goes on above the slope inside the corner), a face
  with no naming origin.
* A blend ended by a slanting third face is trimmed by it: the corner runs on past the edge's end until it leaves
  the body (here a tetrahedron of 0.2 x 0.5 / 2 x 0.3 / 3 = 0.005 mm3 per side). The fallback now does so where its
  first cut leaves a face across an edge's end, and keeps the result only if the longer cut still removes the whole
  corner along the edge and leaves no face without an origin (so it ran out of the body before its far end). The
  stored end body of the housing is 311.94552 mm3, the model's to 1e-5 mm3; cutting only along the edges gives
  311.95552 (0.01 mm3 more, outside the check's tolerance of 0.0062 mm3). Golden test:
  `a_chamfer_next_to_a_chamfers_slope_runs_on_to_it`.
* 5358 comes in as history (9 states, 14 cut tools, end bodies = finished bodies). The end blobs agree without the
  move-snapshot rule.
* The corner fallback now sits beside round 21 J's concave and arc corners: the extension applies to convex
  straight corners only, and the overlap test of chamfers whose edges do not meet still uses the corners along
  their edges.

### 610: Fillet3 and the revolve that intersects
* `Fillet3` (r 0.75 mm) names the edge between `Extrude2`'s end cap `2 [303]` and the side `5 [301]` it shares with
  `Extrude1` (`("5", [301]), ("3", [303])`), with no faces at its ends. `Extrude1`'s profile is a disc and a ring
  (circles r 3 and r 7), a profile with holes whose generator numbering no stored face confirms, so side `5` is
  not modelled; the stored bodies have it as one cylinder r 7 mm about z. A closed edge between a plane and a
  cylinder known only from the stored bodies, about an axis across the plane, is now that whole circle, given as
  points all round it (chords within 0.005 mm), which the import matches against the model's edge.
* Then the step check refused a stored solid of 493.2727 mm3: the tool of `Revolve1`, an intersect of
  `Extrude4` (117.5563 -> 113.1542 mm3). The rules knew tools of cuts and joins only. A feature with no tool count
  that removes material now owns the one unexplained tool that holds, of the bodies it changes, exactly what they
  are after it, both before and after it (113.1542 mm3 here): what an intersect keeps is body and tool.
* 610 comes in as history (8 states, 4 tools, end bodies = finished bodies).

### 5981: negative generators (not decoded)
* `Chamfer2`'s edge joins `-1 [367]` (`Extrude4`, the post r 1.4 mm under the slot, z 6.8..10) and the post's
  side merged with the wall's arc side, ended by the post's bottom cap `2 [367]`. No stored face carries `-1` (the
  post's tool has `1`, `2`, `3`). The stored state after the chamfer has the bottom cap at r 1.0 and the merged
  cylinder down to z 7.2 at the seam, which reads as the whole bottom rim chamfered by 0.4 mm, but then the edge
  would be between `2` and the cylinder, not `-1`. Over adafruit-more, negative generators name a blend edge's own
  face only in 5981's `Chamfer2` and `Chamfer6` (`-3`, `-6`, `-8` of the thin extrude); in the other blend recipes
  that hold them (thin-wall extrudes) they only bound an edge's ends, and those edges resolve without them. Too
  few cases to decode them from the bytes: 5981 stays at `Chamfer2`.

### Result
* adafruit-more: 130 of 150 as history (5358, 5360 and 610 gained; none lost; 127 before). adafruit: 13 of 18
  (unchanged, none lost).
* First blocker of the four: 5981 `Chamfer2` (negative generator `-1`, not decoded); 5358, 5360 and 610 pass.

## 38. Round 23 N: Split Body, the shell of a whole body, mirror planes on faces (2026-11-12)
Reader: `split_json`, `mirror_face_plane` and the whole-body branch of `shell_json` in `fusion_tree/json.rs`;
importer: `split` and `shell` in `gusset-commands/src/tree_import/v2.rs`; document: `split` (ADR 0004 amendment);
kernel: `gs_shell` with no opened face.

### Split Body `65CA3206` (3651)
* 3651 Split1 (component `FPC-50pin v1`): the record holds `u32 1` + a `2CA5A1CD` selection of the tool (here
  a `5A1BF548` -> `CC54ECAD` construction plane, WorkPlane2: origin (1.525, 0.11, 0.09) cm, x (1, 0, 0),
  y (0, 0, -1), so the plane y = 1.1 mm), `u32 1` + the one `D26351F0` body link it makes (its creator is the
  split; a `D3937028` holder follows it), `u32 1` + the selection of the body it splits (`9716F783` -> the link
  of `Extrude1 (3)`, the connector's base, x 0.6..29.9, y 0..5.75 mm after its cuts), then the four input refs
  (tool selection, plane, body selection, body recipe). Section 29's order (body first) was from another
  record version; the reader goes by the classes, not the order.
* One new link and a plane that crosses the body (y 0..5.75 across y = 1.1): the selected body keeps its own
  link and the split makes one more body. The model now has both: 150.801 mm3 (y 1.1..5.75) and 58.014 mm3
  (y 0..1.1), 208.815 mm3 in all, the body's volume before the split.
* Gusset had no operation for it. The document language now has `split <Name> (plane: <Plane>, [body: <B>])`
  (ADR 0004 amendment): the kernel cuts and intersects the body with a block on the plane; the part on the
  side the plane's normal points to becomes the body `<Name>`, the rest stays the body. The tree's `split`
  names the body, the plane (placed like a mirror plane) and `creates_bodies: 1`; later features that name
  the split's body reach the new body.
* Not confirmed by the stored bodies: no stored solid of 3651 holds either piece or the whole after the
  connector's cuts (its blob holds Extrude1 (3) at 274.5 mm3 and the cut tools), so which piece keeps the old
  link is not known. Nothing in the design names either piece afterwards. A split that makes more than one
  new link (324: two), or whose tool is a face, is refused.

### A mirror plane that is a face (the 2x12 header part: 3651, 3900, 4200, 4242, 4400)
* With the split read, 3651 still stopped at the box check (the features reached z = -4.0 mm, the stored
  solids -3.7). The header part's `Mirror1` has no construction plane: its plane holder (`C444E6AD`) selects a
  face (`2CA5A1CD` -> `5662F619` -> face recipe `6 [484]`, Extrude1's side x = 0). The reader took the first
  `CC54ECAD` anywhere below the record, the component's XY plane, and mirrored in z = 0. The stored state of
  the pin pair (48.6155 mm3, x -2.54..2.54, y 0..2.54, z 0..4) shows the plane x = 0; the volume is the same
  either way, so only the box saw it. The face's plane (`Ctx::face`) is now the mirror plane where the holder
  selects one planar face; the header (583.3856 mm3) then lies at x -2.54..2.54, y -15.24..15.24, z 0..4, as
  stored.
* **3651** comes in as history: 123 stored solids are states of the rebuilt model and 51 are cut tools.

### Shell of a whole body (4632)
* `Shell1` selects a body (`9716F783`) and no face. The shim's thick solid with no removed face gives OCCT's
  offset solid only; `gs_shell` now builds the hollow solid: the body's own skin with the offset skin as a
  closed void (thickness < 0: the offset skin outside, the body's skin the void), each skin turned to face out
  or in by the sign of its own volume. The body's faces keep their names, the offset faces are
  `<shell>/inner(<face>)`. Golden tests: `shell_with_no_opened_face_is_a_hollow_solid` (30 x 20 x 10 box, wall
  2 mm: 3504 mm3; outward: 34 x 24 x 14 - 6000) and `shell_of_a_whole_body_is_hollow` (through the document).
* The reader writes the shell with `body` and `faces: []`. 4632 then reads past Shell1 and Split1 (a split of
  `Extrude1` by the origin plane XZ) but stops reading at `Extrude11`, so neither is yet confirmed by a
  rebuild of 4632's stored states: an extrude whose profile is a sketch curve
  (`2CA5A1CD` -> `5A1BF548`, no `4BD53E5A`; parameters `AlongDistance` 5 mm, `TaperAngle`, `ProfileOffset`,
  extent to a face `5662F619`), i.e. a surface extrude of an open curve; then `Thicken1` (`BCE0AD0E`, a body
  selection of Extrude11's surface and one thickness holder) and `C-Pattern1`, 12 copies of both about the fan
  axis. Each is a feature the tree does not write yet (surface extrude to a face, thicken, a pattern of a
  surface feature); 4632 is several features away.

### ACIS binary tag `0x17` (3502)
* 3502's derived bodies (Derived Base Feature, the display's glass, polarizers and flex) carry a
  `FusionDeriveSrcEntId` attribute whose value is tag `17` followed by 8 bytes (`17 38 00 .. 00`, then the
  record terminator `11`): a 64-bit integer. The SAB reader refused the blobs ("unknown binary tag 0x17"); it
  now reads the value as an integer (test `a_64_bit_integer_tag_reads_as_an_integer`).
* With the bends left out (a local experiment only), the step check then reaches the flex: one stored solid of
  5.077285 mm3 (x 0..9.47, y 0..0.05, z -14.63..0 mm, a 0.05 mm sheet in the XZ plane) is not a state of the
  model; the derived flex body itself is 5.0808 mm3. The bends (`BendRadius` 0.45 mm, `BendAngle` -90 deg,
  `Thickness` 0.05 mm) change the volume by -0.0035 mm3, which with a neutral layer at K x t gives
  w (0.5 - K) = -0.45 mm for the bent width w: the K factor and where the bend zone lies against the line are
  not read from the record yet. A bend is a kernel operation of its own; 3502 stays at Bend1.

### Result
* adafruit-more: 131 of 150 as history (3651 gained, none lost; 130 before). adafruit: 13 of 18 (unchanged, none
  lost).
* First blockers: 3651 passes; 3502 `Bend1` (a sheet-metal bend: not converted); 4632 `Extrude11` (a surface
  extrude of a sketch curve to a face, then `Thicken1` and its circular pattern).

## 39. Round 23 O: a draft of a slanted face, Base Feature bodies from the PM blob (2026-11-12)

### A draft of a face at a slant to the pull (258)
* 258's `Draft1` and `Draft2` each draft one pin's start cap (plane y = 29 mm, outward normal -y) about the 45 degree
  chamfer face beside it (neutral plane y + z = 34 mm, outward normal (0, 1, 1)/sqrt 2): `Draft1` by +45 degrees,
  `Draft2` by -45. The cap does not lie in the neutral plane; it meets it in the line y = 29, z = 5 (the pin's lowest
  line). The stored cap of each pin, after the draft, is the ellipse where the pin's cylinder meets the chamfer face
  (points such as (-2, 28, 6) and (-2.5, 28.5, 5.5)): the pins run on to the chamfer face.
* **Flipped pull.** The older draft layout (925-byte `F4163A07`, 2014-2016 files) has three bytes after the header,
  then `00 00 00 01` and the first reference. The first byte is 1 in 258's `Draft1` only and 0 in its `Draft2` and
  in the drafts of 324 and 355 (whose 10 and -10 degree drafts rebuild with the pull as the neutral face's outward
  normal). Read as "the pull is reversed", the two drafts of 258 give the same result on mirror-image pins. Newer
  layouts are left as they were (the byte is not where they keep it).
* **Rule.** Fusion turns each drafted face about its line on the neutral plane, its outward normal n towards the pull p
  by the angle a: n' = n cos a + m sin a, m the part of p across n (unit). For a side wall (n square to p) this is the
  draft Gusset already makes. For a plane face at a slant it is the side-wall draft about the plane through that line
  with normal m. With p = (0, 1, 1)/sqrt 2 reversed and a = +45 (`Draft1`), or not reversed and a = -45 (`Draft2`),
  n' = (0, -1, -1)/sqrt 2: the cap turns into the chamfer face's plane, as stored. The reader writes such a draft
  (one plane face) with that plane; the face is picked by the stored points that lie on its plane before the draft
  (the stored face's points come from every state that has the face, and the drafted ones lie on the chamfer face,
  which they would pick as well).
* 258 comes in as history (4 stored states and 1 cut tool explained).

### Base Feature bodies whose list is a later state (4431)
* `Base Feature1` (a JST connector, 5 DM bodies) and `Base Feature1 (2)` (the switch, 1 DM body) name a `.smbh` state
  list that is a later state of their components. The connector's list (after `Combine1`) lists 4 bodies, without
  the DM body that `Combine1` takes as its tool (that body still names the list as its state). The switch's list
  holds it with the bodies of two later new-body extrudes, and its `.smbh` body is already cut by `Extrude1 (2)`.
* The PM blobs (`.smb`) keep the component's bodies of earlier states as well: the connector's 5 PM bodies among 6
  solids (the first a copy of the second, a 0.5 x 2 x 3.3 mm block), the switch's 1 among 11 solids and 17 sheets
  (the other solids are the cap's extrudes and the cut tool). In both the PM list's bodies are, in list order, the
  last solid bodies of the blob (as DM bodies are the last of a history blob): connector blob bodies 1..5 (3.3, 2.6379,
  2.6379, 101.8808, 2.6379 mm3), switch blob body 21 (607.5388 mm3). Reader: where a DM body is not in its list, or
  not among its last bodies, the Base Feature's stored bodies are its PM bodies (DM k and PM k name each other) from
  the PM blob, at those places.
* A Combine whose tool or target is one body of a Base Feature of several is now that stored body (the k-th body
  link of the feature is its k-th stored body).
* With these the whole tree reads and every feature rebuilds to the stored states, the end of the switch's chain
  included: 521.2856 (switch after the cut), 47.4707 and 632.6862 mm3 are the bodies of its `.smbh`, and the
  connector's housing becomes 105.1808 = 101.8808 + 3.3 mm3 after `Combine1`, as its `.smbh` stores it.
* **Still refused (not converted):**
  1. The import refuses a stored body that a later feature changes when it equals a finished body of the file: the
     finished bodies are the `.smb` solids, and the connector's PM blob holds the housing as it was before
     `Combine1`. Experiment only (the rule bypassed): the step check then goes on.
  2. Step check, experiment only: `Extrude1 (2)`'s cut tool (127.1088 mm3, r 3.4 x 3.5 mm) holds 86.2532 mm3 of the
     switch before the cut (exactly the cut) and none after, but the tools of the later joins of the cap (4.48,
     12.768 and 27.436 mm3) also hold parts of the switch's top before the cut and none after; the placed-tool rule
     gives a feature all such tools at once, so the cut is not explained. Taking the tools feature by feature until
     no feature takes another explains it (each feature's tools still have to be its volume change exactly).
  3. Then `Fillet1` (the last feature, 633.3340 -> 632.6862 mm3) follows the last stored `.smb` state and has no copy
     count; the end bodies (`.smbh`) are not consulted in a tree with stored bodies. They would confirm it, except
     that the connector's `.smbh` also holds the 3.3 mm3 block that the tree's `Combine1` consumes. Whether
     `Combine1` keeps its tool is not decoded: its 349-byte record is byte for byte the layout of the consuming
     Combines of 3651, 3900, 4200, 4242 and 4400 (only references and a counter differ), and the state list omits
     the block while the block's record still names that list.
  4431 stays as bodies; its first blocker is now (1).

### Not attempted
* 858 (`Fillet3`, a refine that unifies across the boolean only, and three more blockers after it) and 6299
  (`Extrude17` from a body face, then two lofts, two more extrudes of faces and a thin extrude unconfirmed) are further
  from passing than 258 and 4431 and were left for later rounds.

### Result
* adafruit-more: 131 of 150 as history (258 gained; none lost; 130 before). adafruit: 13 of 18 (unchanged, none
  lost).
* First blockers of the four: 258 passes; 4431 the finished-body rule on its PM-blob stored body (above, 1-3);
  858 `Fillet3` and 6299 `Extrude17` as in section 35.

## 40. Round 23 P: stored blends on procedural supports, extrude sides, a threaded nut (2026-11-12)

### 1145: the stored cap was read wrong, not built wrong
* Revolve2's cap (a degree-5 spline revolved, its rim filleted 0.6 mm) rebuilt to 476.7911 mm3 against a stored
  436.9949. Pappus over the sketch's spline and the 2D fillet arc gives 476.7903: the rebuild was right. A mesh of
  the converted stored cap gave 475.6 while every OCCT quadrature (default, adaptive, Gauss-Kronrod) gave 436.99:
  the converted shell had a gap.
* Its fillet is an `rb_blend_spl_sur` whose second support is the revolved spline (`blend_support_surface spline
  T { rot_spl_sur ... }`), and the support's block holds its own B-spline approximation before the blend's. The
  reader took the first `nubs|nurbs` that reads as a surface, the support's: the "fillet" face was the dome over
  u 0.922..1, ending at r 6.5, z 18.2, and the cylinder below it stopped at z 17.73 (88.48 mm3/mm x 0.47 mm =
  41.5 mm3 of shell missing). A procedural surface's approximation is now one in its own block only (nested
  `{ ... }` skipped; a `{ ref n }` definition ends at its closing brace). Stored cap: 476.7417 mm3.
* That approximation is coarse: cubic, three spans across, 3.4 um inside the true rolling-ball surface (0.0494 mm3,
  1.04e-4 of the body; the step check allows 2e-5). A constant-radius rolling-ball blend whose ball centre (the
  `intcurve { off_int_cur ... }` of its own block, here a circle r 5.9 at z 17.7307 about the cap's axis) runs round
  a circle is a piece of a torus; where the defining curve lies on one circle within 1e-5 of its radius, the
  approximation's corners at one distance from it and its other points within 2% of that, the face is written as
  that torus (seam at the curve's start, facing as the approximation). Stored cap: 476.7899 mm3 (2.5e-6 off the
  rebuild, 1.7e-6 off Pappus). 1145 comes in as history.
* The same nesting held 3722's coils: their stored solids now convert to 743.6554 and 758.1703 mm3 (were -13877 and
  501.18), the Pappus values of round 19's reading (below).

### 3350: an extrude's side is its distance's, not its vector's
* `extrude_raw`'s unit vector was taken to flip the extrude where it ran against the sketch normal. Over both sets
  only 3350 has such vectors (four extrudes, each with components like 1.457e-15: a computed vector), and its stored
  bodies show every one going the way its signed `AlongDistance` says along the sketch normal: Extrude1 (-14 mm on
  XY) is the rod at z -14..0, Extrude3 (3 mm on a sketch at z -14 facing down) a disc at z -17..-14, Extrude6 (3 mm
  on a sketch at z -17 facing down) bodies at z -20..-17. The vector no longer decides the side (one in the sketch
  plane still gives the assembly's direction, 4800). With the rod the right way up, Fillet4 builds.

### 3350: Thread2 is on a hole of a profile with holes
* The thread's face recipe is `("10", [431])`: generator 10 of Extrude8 (a hexagon with an 8 mm hole, z -5..-2), not a
  revolve's face. The extrude's generator numbering is used for profiles with holes only where stored faces confirm
  it, and the stored faces of the hole have one vertex each (a whole cylinder), which `truth` cannot place. The
  thread now takes such a face from the stored faces of that name when they lie on one cylinder about the extrude's
  direction through the centre of exactly one whole circle of the profile with that radius.
* The internal thread's move was the wrong way round: Fusion fills an 8 mm hole in to the minor radius 7.51775 mm (a
  join of the ring 7.51775..8), and a hole already narrower than the minor diameter is now refused (no sample).

### 3350: Fillet1, a convex round beside a concave one
* Fillet1 (0.5 mm) rounds the groove that Revolve2 cuts at the rod's foot: its concave top edge (r 7, z -10.5), the
  rod's rim above it (r 7.942, z -10.5) and the groove's foot (r 7 against the 45 degree chamfer). The step between
  the first two is 0.942 mm, less than both rounds need. Fusion's stored rim torus is centred at z -10.0017, not
  -10.0: its ball touches the rod and the concave round (centre distance 1.000 to (7.5, -11)), its v range is
  1.51276 rad (stored `F 0 F 1.51276`). Filleting the concave edge first and the convex ones after gives exactly
  that: 2573.04750 mm3 by OCCT and by Pappus over the section. OCCT at once returns an invalid solid that ShapeFix
  turns into a valid one of 2575.58 mm3 (more than the unrounded 2573.49). The shim now tries concave-first passes
  before the repair, for fillets of both kinds of edge (golden test
  `a_convex_fillet_beside_a_concave_one_rolls_on_it`).
* The threaded rod is itself invalid to BRepCheck (helix edges of the groove's B-spline fits a little off their faces,
  wires at the thread's start); a blend whose every fault lies in a vertex, edge, wire or face the body had unchanged
  is now kept (faults the blend made, or any on a shell or solid, still refuse it).

### 3350: OffsetFaces1 (not built)
* Its two faces (`Extrude4/side c6, c7`, the pocket walls r 4.9 and 6.0 at z 1.1..1.5) lie on the faces of Extrude5,
  the insert cut by the same profile, so the pick tied. The offset now carries the body its selection links to, as
  blends do, and a tie goes to that body: Extrude1, which the end blob confirms (pocket r 0.49 -> 0.48 and
  0.60 -> 0.61 cm, the insert unchanged).
* OCCT's `BRepOffset_MakeOffset` then fails on the whole rod: offsetting the threaded body throws
  "Geom_BSplineSurface: Weights values too small" (it builds offset surfaces of every face, the thread's flanks
  included). An offset of a local piece (the body split round the faces, offset and fused back) is the likely way;
  not attempted. 3350 stops here.

### 3722: the coil, read but not built
* Section enums of `CoilPrimitive` (round 19's reading confirmed by the now correct stored solids): the triangle is
  equilateral, 2 mm (`SectionSize`) across radially and 4/sqrt 3 mm along the axis, its side on the coil's diameter.
  Coil 1 (d 67, position 3, type 3): side at r 33.5, apex out to 35.5, centroid r 34.1667; coil 2 (d 71, position
  1, type 4): side at r 35.5, apex in to 33.5, centroid 34.8333. A helical sweep of an axial section has volume
  A x angle x r_centroid: 2.3094 x 3 pi x 34.1667 = 743.6554 and x 34.8333 = 758.1703 mm3, the stored values to
  1e-7. So position 3 = outside, 1 = inside; type 3 = triangle pointing out, 4 = pointing in (each seen once, so
  position and type are not separated).
* Not converted: the coil's axis and start (the `DA1231DE` entity and `7B6C3C2D` frame) and its hand are not decoded,
  and the design goes on with Move1 of the coil, Combine1, Chamfer1 on the coil's end edges (unresolved) and Shell3,
  which OCCT fails on (section 25). 3722 stays at `CoilPrimitive1`.

### The adafruit-next threads
* 916 (the same M16x1 nut as 3350): its hole's stored faces lie on two cylinders under one name (the state after
  the thread keeps the name on the minor diameter), so the single-surface pick gave none; the hole is now the
  profile's one whole circle about that axis with the radius of one of them. Thread2 reads; the design stops at
  Fillet1: the same groove rounds as 3350's, but the thread starts at z -16, 0.5 mm above the rim edge, and the rim
  round reaches the cylinder at z -16.0017 (Fusion's torus), 1.7 um below the groove's square end. OCCT fails on it in
  every order.
* 3221 and 3429 now read their threads and stop elsewhere (3221: CopyPasteBodies1 with one Base Feature and no
  RemoveBodies; 3429: Fillet2, four edges unresolved).
* 3692 (an internal 1/4 UNS thread) reads and builds; its first body is refused by the end blob: 431.5074 mm3 against
  the stored 441.2291 (the other five bodies agree). Not examined.

### Result
* adafruit-more: 133 of 150 as history (1145 gained; none lost; 132 before). adafruit: 13 of 18 (unchanged).
  adafruit-next: 110 of 150 (unchanged, none lost).
* First blockers: 1145 passes; 3350 OffsetFaces1 (OCCT's offset throws on the threaded rod); 3722 CoilPrimitive1
  (axis, start and hand not decoded; Shell3 and a chamfer of the coil's ends follow); 916 Fillet1.

## 41. Round 24 Q: blend edges of the adafruit-next set (2026-11-12)
The adafruit-next set (150 designs the reader was never tuned on) stopped at fillets and chamfers in nine designs.
Readers: `fusion_tree/naming.rs` (edge midpoints), `feature.rs` (blend sets); the end-blob rule in `import.rs`.

### Fillet faces on a box's convex edges (67)
* 67 Fillet1 rounds the four long edges of a box, each between a side and a cap of `Extrude1`; Fillet2 then rounds
  the box's two end outlines, eight straight edges and eight arcs between an end face and Fillet1's faces
  (`5 [301]` and `4 [303]`, ended by `6 [301]` and `1 [301]`). The fillet face model (a rolling ball between two
  planes) only took two sides, so the arcs were not resolved. A side and a cap of one extrude meet at a convex edge
  of the prism (the material inside both), so a cap now counts as a fillet's side where the edge's two faces are of
  one extrude. 67 comes in as history.

### Edges of a mirror's copy (770)
* 770's `Mirror1` copies two join extrudes (tabs); `Fillet2` rounds the end edges of the original tabs and of the
  copies. The copies' faces are named with three operations: the face's own extrude, the mirror's, and one
  `AD719C1A` record per copy (`9 [308, 313, 314]`). The tabs' end faces (8 mm wide, rounded by 4 mm) vanish under
  the fillet and no stored state holds the copies before it, so their edges were not resolved. An edge every face
  of which carries `[own, mirror, copy]` is now the same edge of the original (the operations cut to `[own]`),
  reflected in the mirror plane.
* `Fillet3` (unnamed, r 2 mm, 18 edges) names the bottom rim of the box `6 [301]` / `1 [301] + 1 [316]` twice,
  once ended by the side `21 [316]` (x 15 mm) of a foot `Extrude3` joined at the bottom and Fillet1's corner face
  `4 [303]`, once by the foot's other side `6 [316]` (x 47 mm) and the other corner face. The only stored edge
  between those two faces is a piece of a later state (ended by `4 [316]` and `23 [316]`), which both recipes took:
  one edge at x 31 mm instead of two at x 9 and 56 mm. A straight edge between two planes ended by a fillet face
  that rounds the corner of one of its planes now ends where the fillet leaves that plane, `r` from the other
  side's plane (the modelled answer comes before the stored edges).
* 770 then stops at that fillet's import: Gusset's model has no edge at (9, 33, 0) mm (the nearest is 5 mm away);
  the bottom of the box is split by `Split1` and `Extrude3` joins feet to `Extrude1`, and the model's lower body
  carries the rim from x 15 mm on only. Not examined further.

### A closed edge where a cone meets a cylinder (6118)
* 6118 Fillet2 names the edge between an extrude's round side `3 [306]` (r 11.75 mm about y) and `5 [309]` of a
  revolve, with no faces at its ends. The stored bodies hold `5 [309]` as a 45 degree cone about the same axis.
  A closed edge between a cylinder and a coaxial cone, both stored, is now the circle where the cone reaches the
  cylinder's radius (radius at height h: `r + h sin / cos`, section 28), given as points round it.
* 6118 then stops at the step check: a stored solid of 2477.0412 mm3 is neither a state nor a tool (the features
  give 6330.8 mm3 of the 23848.4 stored; the design inserts components that are not merged).

### Pockets whose corners a fillet rounded (6252)
* 6252 Chamfer3 chamfers the rim of a notch in a wall (`4 [306]`); the notch's corners were rounded by a fillet
  (`[325]`, r 1 mm) whose sides are a chamfer face and a cut's side, so its faces are not modelled; the stored
  bodies have them as cylinders about an axis across the wall. Two kinds of edges are now resolved by the stored
  cylinders: a line where two planes meet, ended by a plane or by a stored cylinder about an axis across the line
  that the line touches (it ends at the touching point), and an arc where a plane meets a stored cylinder about an
  axis across it, ended by two planes the cylinder touches (the shorter arc between the touching points). An end
  may also be a modelled fillet face (a fillet of `[324]` rounds the notch's other corners).
* Where the two touching points are opposite (a half circle, the round end of a thin wall: 5998's `Chamfer7`), the
  arc bulges to the side of the cylinder's stored points.

### Generator -1 (6252, 5981)
* 6252 `Chamfer1 (2)` names an edge by `-1 [351]`, the side `3 [351]` of a round boss and its end cap `2 [351]`.
  No stored face carries `-1`. The stored state after the chamfer holds the chamfer face `1 [353]` from cap `2`
  (z 7.4 mm, r 1.1 mm) to the side (z 7.8 mm): the rim of cap `2` is chamfered by 0.4 mm. adafruit-more 5981's
  `Chamfer2` (section 37) has the same recipe shape (`-1`, the post's side, its cap `2`), and its stored state too
  shows the rim of cap `2` chamfered. An edge named by `-1`, a side and cap `2` of one extrude is now the edge
  between the side and cap `2`. (Two samples; what `-1` encodes is not decoded.) In 5981 the side is merged with the
  wall's arc side (`11 [363, 364]` + `3 [367]`): the side's entry of the same extrude is taken.
* 6252 then stops at `Extrude6` (layout not recognised); 5981 reads past `Chamfer2` and stops at `Chamfer6`, whose
  edges are bounded by the thin extrude's `-3`, `-6` and `-8` (not decoded).

### A face picked whole (5998, 5360)
* 5998 `Chamfer2` names no edge: its selection hangs under the value holder `0D383DB2` (the holder of two-distance
  chamfers, section 30) and holds one **bounded-face** recipe: the bottom cap `1 [305]` of the box and the eight faces
  round it (four sides, two faces of `Fillet1`, two of `Chamfer1`). Fusion blends every edge of a face picked whole.
  The faces round it are not listed in loop order (`2 [308], 5, 6, 2 [307], 4, 1 [307], 3, 1 [308]`), so each edge
  where the face meets a plane `B` round it runs along their common line between the nearest ends on either side of
  a point of `B` (a modelled side's middle, else the mean of `B`'s stored points) that the other faces make (planes
  crossing the line, fillets leaving the face or `B`); a fillet face round it meets it along the fillet's arc. The
  reader now also looks for a blend's selection under `0D383DB2` where none hangs under the feature itself.
* 5360 (the Pins variant) `Fillet - Contacts` has a set (`4A557CE2`) whose selection names no feature back (every
  set read so far named its feature), so it was not taken as the fillet's own; it is taken now where its selections
  name no other blend. Its one recipe is a bounded face with one face round it: the end cap `2 [338]` of a round pin
  and its side `5 [338]`, the closed rim between them.
* 5998 then stops at `Chamfer7` (five edges of a thin wall's round ends and fillet faces `[386]` whose stored
  cylinders lie at the other end of the wall: the recipe pairs are not understood).

### An end blob that is both an earlier state and a finished body (5360)
* With the fillet read, the Pins variant of 5360 stopped at the end bodies: the stored end bodies lacked the housing.
  Its housing (174.9762 mm3) is not changed after the step where it was the model's only body, so its component's
  blob matched that state and was left out as a move snapshot (section 37). A blob that is also, one for one, bodies
  of the finished model now stays among the end bodies; the root's snapshot (taken before `Fillet - Magnets`
  rounded a magnet, 25.5176 against 25.5081 mm3) still goes. This keeps more blobs to check against the finished
  bodies, never fewer. 5360's Pins variant comes in as history.

### Not converted
* 2278 `Fillet1` (16 vertical edges of two pockets, r 3 mm) and 3777 `Fillet4` (two arcs of a disc's rim, r 2.5 mm):
  OCCT's blend and the corner fallback fail. In 2278 a convex and a concave corner 6 mm apart (x 115.5 and 121.5 mm
  at z = 20 mm) are both rounded by 3 mm, which uses up the side face between them; not examined further.
* 2315 `Fillet3` (r 1 mm on the tops of posts): Fusion's picks are quarter arcs about centres 4 mm from the model's
  half circles (the model's posts are rounded on the other side), likely the regions of `Extrude12`; not examined.

### Result
* adafruit-next: 112 of 150 as history (67 and the Pins variant of 5360 gained; none lost; 110 before).
  adafruit-more: 132 of 150 (unchanged, none lost). adafruit: 13 of 18 (unchanged, none lost).
* First blockers now: 770 the unnamed `Fillet3`'s import (the model's lower body has no rim at x 9 mm); 6118 the step
  check (a 2477.0412 mm3 stored solid); 6252 `Extrude6` (layout); 5998 `Chamfer7`; 2278 and 3777 OCCT's fillet;
  2315 `Fillet3` (picks on quarter arcs the model does not have).

## 42. Round 24 R: the new set's extrude layouts, profile loops, revolve axes and a mirrored sweep (2026-11-12)

The adafruit-next set (150 designs the reader was never tuned on, 110 as history in its first run) left reader gaps
in extrude layouts, profile regions, revolve axes and sweeps. Decoded from the samples' bytes only.

### Extrude enum block: a second flag of 0
* The block is `u32 op, u32 extent, u32 kind, u8, u8 01`, then a `u32` start kind (0 profile plane, 1 offset,
  2 from an object; section 7). In three records with a `ProfileOffset` (2530's `Extrude2` and `Extrude3`, 4632's
  record 5067 in adafruit-more) the second flag byte is `00`, the start kind 1 or 2. Over the three adafruit sets:
  3889 records `00 01 | 0`, 34 `01 01 | 0`, 42 `00 01 | 1`, 133 `00 01 | 2`, 6 `01 01 | 2`, and these 3 `00 00 | 1..2`.
  The reader takes a `00` second flag when a start kind 1 or 2 follows. (Both designs stop later: 2530's extrudes
  select open sketch curves, which make surfaces that a Thicken uses; 4632's selects a face.)

### Symmetric extrudes: the measure further on, and none for through-all
* 817's `Extrude2` and `Extrude3` (class version 388, typed references, distances `( 4.2 / 2 ) * 1 mm` and
  `( 4.7 / 2 ) * 1 mm`) hold the measure (`02`: each side's) 236 bytes after the direction, past five typed
  references; the reader looked 200 bytes far and now looks 320. The chamfer `Chamfer1` of `Extrude3` sits at
  z = +-2.35 mm, so each side's 2.35 mm is right.
* 1264's `Extrude2` and `Extrude3` are symmetric and through all (kind 0, no `AlongDistance`): the record has no
  measure (24 zero bytes, then the selection list), and none is needed. The reach is taken from the stored bodies as
  for one-sided through-all cuts, and the whole length is twice the farther reach.

### Profile loops
* **Construction-flagged curves in a loop.** Of the 534 lines of the 396-byte layout whose construction byte (section
  18) is 1, eleven are named by a profile loop of their sketch (2895's `Extrude4`, 3010's board, 817, 4128). Read as
  construction, the loop named an unknown curve or did not close. A flagged curve that a loop of its sketch names
  is now a profile curve (the loops are read only for sketches that have a flagged curve).
* **Slits.** A hole loop of one whole open curve (3010: the line between two mounting holes, inside the outline)
  bounds no area and is left out. A hole loop of closed curves joined by open pieces that end on them (the two
  r = 2.9 mm circles and the middle piece of that line) is those circles as holes.
* **A curve listed in all its pieces.** 4300's board outline lists a line and an arc both as pieces 1/2 and 2/2:
  they overrun each other by 1.9 um where they meet (each one's end lies on the other), so Fusion cuts each in two
  and walks both pieces. The pieces could not be cut out; a curve whose pieces cannot be cut out but whose every
  piece the loop lists now stands whole, once. (Only then: 2348's board lists all six pieces of its left edge, which
  other lines lying on it cut, and the whole edge would overlap them.)
* **Ends that Fusion joins.** In the same outline an arc's end and the next line's start are 2.8e-6 mm apart (two
  sketch points), and the overrunning line and arc above leave a 1.9 um overlap; the model's sketch merges ends only
  within 1e-6 mm, so the board had no region. The tree's sketch now maps points less than 1e-6 cm apart onto one
  point, and maps a line's end that lies on an arc's circle within 1e-3 cm of the arc's end (the arc's end lying on
  the line) onto that end.

### Chamfers of a left and a right distance
* 2895's `Chamfer2` names `leftDistance` 2 mm and `rightDistance` 3 mm (older records; newer ones `Distance 1` and
  `Distance 2`, section 30); it was read as a 2 mm chamfer. It now takes the two-distance path. Its reference face is
  decided from the stored faces as in section 30, but `Fillet2` later rounds the 2 mm line across the top, so only the
  3 mm line down the side remains: the stored faces show one of the two lines of one assignment and neither of the
  other, which now decides it.

### Revolve axes
* **A construction line.** 1313's `Revolve1` names line `prim 200` of its sketch, a construction line (dropped from the
  sketch's curves). The axis is now given by its place (origin and direction) when the line is construction.
* **A cylindrical face.** 1054's `Revolve1` selects (`2CA5A1CD` -> `5662F619` -> `7ACC2A03`) a face recipe, generator
  `3` of `Extrude1`, a cylinder; no sketch curve and no origin axis. The axis is the stored cylinder's axis (a whole
  turn, so its sense does not matter).

### A mirror of a sweep
* 3923's `Mirror1` mirrors `Sweep1`, a sweep that made a new body. Gusset builds mirrored features again reflected,
  which sweeps are not; a mirror of one new-body sweep is now a mirrored copy of its body (no merge). 1119 mirrors a
  sweep together with a cut that cuts it and a body of its own (`Mirror1`: `Extrude3`, `Sweep1`; `Mirror2`: the
  sweep, `Mirror1` and the cut), which this does not cover.

### Profile pieces: a curve that ends on another
* 471's ring segments: two lines run from an inner circle (r 10.65 mm) out to an outer one, and each circle's
  piece between them is a loop piece. The lines end on the true circles; the inner circle's inscribed polyline
  lies inside its circle, so a line from outside never meets it and the circle was not cut there. As a last
  resort the open curves of the loop that neither cross nor touch a curve now cut it where they end on it (within
  its chords' sag). 471 then stops at `Chamfer1`, a chamfer of a left and a right distance on an edge whose face
  is not planar.

### Where the reader now reads further but the design stops
* 817: `Fillet3` picks the edge at (3.4, 15.9, 0) mm, where the stored fillet runs along the whole side of
  `Extrude2`'s tab (z -2.1..2.1); the rebuilt model has two edges there, split at z = 0 by `Extrude1`'s bottom
  face (a convex and a concave one on one line). Edge resolution, not the extrudes.
* 1054: `OffsetFaces1` (-0.1 mm on `Extrude5`'s cylinder, r 1.5 mm) does not build here ("offset produced no
  result"); the revolve about the cylinder's axis builds.
* 1313: `Extrude1`'s region is bounded by four B-spline pieces that cut one another near the corners (each is
  piece 2/3); the model's sketch does not form that region.

### Not reached
* 4222's `Extrude1 (2)`: a loop names curves `148` and `142` (pieces 2/3), which no record of the file holds; the
  sketch was changed after the profile was computed.
* 1833's `Extrude2`: the loop lists the rectangle's bottom twice (curve `556` whole and `543` piece 2/3), its two
  sides in lower pieces (1/2, 2/2) and no top; nothing in the sketch cuts the sides. The profile names a curve the
  sketch no longer has.
* 6002's `Sweep1` runs along three body edges (a `5662F619` edge selection, not sketch curves); the design has a dozen
  more blockers (thin extrudes of wall location 1, unresolved fillet edges, a `MoveFace`).
* 2530: its extrudes of open curves make surfaces for a `Thicken`.
* 1264: `Extrude8` is a thin extrude of wall location 1 (section 33, not confirmed) whose curves are not one open
  chain; Base Feature copies follow.

### Result
* adafruit-next: 114 of 150 as history (110 before; 2895, 3010, 3923 and 4300 gained; none lost). adafruit-more:
  132 of 150 (unchanged, none lost). adafruit: 13 of 18 (unchanged, none lost).
* First blockers of the round's designs now: 817 `Fillet3` (edge pick), 1054 `OffsetFaces1` (does not build),
  1313 `Extrude1` (B-spline region), 1119 `Mirror1` (a sweep mirrored with a cut), 1264 `Extrude8` (thin), 1833 and
  4222 (profiles that name curves the sketch no longer has), 2530 (open-curve extrudes), 6002 `Sweep1` (body-edge
  path).

## 43. Round 24 S: box primitives that join or cut, body moves of several bodies, body copies (2026-11-12)
Reader: `fusion_tree/pcb.rs` (`box_primitive`), `fusion_tree/bodyops.rs` (`carry_moved_bodies`), `fusion_tree/json.rs`.
Seven adafruit-next designs stopped at a body operation. Shared causes: two of them (2354, 3000) at the
BoxPrimitive record, three (2831, 3093, 3221) at the body-move records of section 30.

### BoxPrimitive `F193B033`: the operation and the placement's pad
* **Operation.** The record body (after the header) is `01`, a `u32` operation and `01` before the first reference,
  with the extrude's numbers: 1 join, 2 cut, 4 new body. The 60 boxes of the other designs (PCB generators of nine
  designs) are all 4. 2354 has 17 boxes: 6 new bodies, 5 joins, 6 cuts; 3000's one box is a join. A join or cut names
  a body selection (`2CA5A1CD` -> `9716F783` -> the target's `D26351F0` link) and a face selection (`5662F619`, the
  face it was placed on); most name no body link of their own (2354's 1665 and 10314 do).
* **The pad before the matrix.** The 4 x 4 stands behind the fifth `D91D429C` holder reference: two bytes behind a
  plain reference (`01 u64 00`), three behind a typed one (`01 u64` + the class GUID as text, with no zero byte of its
  own: the first reference to a class in a typed stream). Each reference in that list is followed by one more zero
  byte, then one byte (0) before the matrix. 3000's box was read two bytes in and refused as "not a rigid frame".
* **Height.** `DistanceZ` is signed (2354's cuts into the board: -0.05 cm, -0.06 cm; a join -0.6 cm). A new box keeps
  its rule (the side on which its stored body lies; all 60 have a positive height and lie on the +z side). A join or cut has no stored body of its own: it is extruded `DistanceZ` along the frame's z (sign
  included) and acts on the body its selection names (`op_target`). Not yet confirmed by a rebuild: neither design
  reaches the step check (below).
* **2354's new boxes and their state list.** BoxPrimitive1's body names a state list of one body whose `.smbh` holds
  three solids, all the same (16 vertices, z 0.125..0.5 cm), while the box is 16.5 x 10 x 0.5 cm from z = 0: the
  stored body is a later state of the box (cut by later features), so it cannot confirm the box's side. Left refused.
  With the box taken by its sign (experiment only), 2354 then stops at `Extrude1 (2)`, whose profile is a face.

### Body moves of several bodies (2831)
* `CutPasteBodies1` (twice in 2831) moves two bodies at once: one Base Feature making two bodies (two `D26351F0` links)
  and two RemoveBodies of one body each. Each made body pairs with the removed body whose body record (`D3937028`)
  has the same name (`Base0`, `Sensor0`; `Collector0`, `Emitter0`); a name that is missing or not unique on either
  side is refused, as is a Base Feature whose body count is not the RemoveBodies'. One body keeps the old rule.
* 2831 now comes in as history.
* 3093's `CutPasteBodies` (`27061`) moves twelve bodies whose body records have no name: refused. They are copies of a
  body pattern (R-Pattern2), whose copies the tree cannot tell apart either.

### Body copies (3221)
* `CopyPasteBodies` has the record class of CutPasteBodies (`BA3059E4`) with a Base Feature and no RemoveBody: the
  body stays and the Base Feature holds a copy. The Base Feature is read as any Base Feature (its body a stored body
  of the tree) and comes into the tree right after the copy record (Base Features are off the timeline and were read
  after every feature: `Move1`, which moves the copy, found no maker).
* 3221 now stops at `Pipe1` (Pipe is not supported; `Loft1` follows).

### Not finished
* **3000**: the box now reads; `Mirror1` mirrors two faces: the box's face `6` (operation 318) and face `2` of
  operations [318, -319] (the box's side, drafted by `Draft1`). A mirror of faces takes one operation per face and
  extrudes only (section 38): a box with a draft is not converted. With the mirror left out (experiment only) the
  design stops at `Extrude10`, whose profile regions are not decoded (curve piece 2/3 not reproduced).
* **3093**: `Combine1`'s target (the selection list of one) is a copy of `R-Pattern1` (link `26330`), its tools the
  pattern's source body (Extrude5) and the other two copies. Which copy a link is, the record does not say (section
  31); later features (Sketch12, R-Pattern2, the Body->Comp) name the combined body by that copy's link. With the
  combine and the twelve-body move let through (experiment only) it stops at `R-Pattern2` (a pattern of that body).
* **3802**: `Extrude4` (join, 1.92 mm, one region of Sketch1) names the bodies of Extrude1 and Extrude2, and later
  features (Extrude5, Split1, Extrude14) still name both apart: the join did not merge them, so the rule of section
  31 (one body after the join) does not apply. Which of the two the tool joins is not decided by the reader; with
  either (experiment only) the design stops at `Fillet2` (four edges unresolved) before any step check.
* **5907**: the bodies reader (`fusion.rs`) reads no occurrence record in this stream (its occurrences are
  `CE2913AA`; the reader looks for `FF415D89`), so the mirrored component's source (`Material-077`, a Base Feature
  of ten pins) is placed by a block matrix that follows its blob: a quarter turn about x at (26.035, 10.16, 0.785) mm
  (the board's centre, half its thickness). The occurrence itself stores (1.9, 1.3, 1.57) mm, unturned, and the new
  occurrence diag(1, -1, -1) at (1.9, 19.02, 1.57) mm: the reflection in the plane y = 10.16 mm, the mirror's plane;
  the new blob is the source's reflected in its own z = 0. The new component's blob is placed nowhere. The map from
  the placed source to the new component is then a reflection composed with that quarter turn, not a reflection.
  Fixing it means reading these occurrences in `fusion.rs`, which places every body of the file: not done here.

### Result
* adafruit-next: 111 of 150 as history (2831 gained; none lost; 110 before). adafruit-more: 133 of 150 (unchanged,
  none lost). adafruit: 13 of 18 (unchanged, none lost).
* First blockers: 2831 passes; 2354 BoxPrimitive1 (its stored body is a later state); 3000 Mirror1 (a mirror of a
  box's faces, one drafted); 3093 Combine1 (target a pattern copy); 3221 Pipe1; 3802 Extrude4 (a join naming two
  bodies that stay apart); 5907 the mirrored component (occurrences the bodies reader does not read).

## 44. Round 25 T: blends that use up a face, a rim round cut by flats, offsets of cylinders (2026-11-12)
Kernel: `crates/gusset-occt-sys/cpp/gusset_occt.cpp` (`mixed_corners`, `ring_turns`, `offset_cylinders`, `gs_boolean`);
import: `tree_import/v2.rs` (`find_face_by_points`). Four designs stopped in the geometry kernel, not the reader.

### A convex and a concave round across one face (2278 Fillet1)
* 16 vertical edges of two pockets, r 3 mm. The pocket has a 6 mm notch: its convex corner (x 115.5) and its concave
  corner (x 121.5) on the face z = 20 mm are both rounded by 3 mm, which leaves nothing of that face. OCCT's blend fails
  (it cannot end a blend on a face it uses up), and the corner fallback (section 34) took only corners of one kind.
* The fallback now cuts the convex corners away, then fills the concave ones: two booleans, each checked against its
  corners' exact volume, the face history merged across both (golden test
  `a_convex_and_a_concave_round_that_use_up_their_face`). 2278 now comes in as history.

### The two arcs of a rim between flats (3777 Fillet4)
* A disc of radius 9.95 mm with flats at y = 8.1 and y = -6.9 mm: its foot is two arcs, rounded by 2.5 mm. Fusion's
  stored body (end blob, its coordinates three times the model's) has one torus face, R 7.45 r 2.5, the round of the
  whole rim: under the flats it goes on round the axis, and the flats end on the torus (their foot edges are the
  plane's curves on it, z down to -50.514 mm, not the disc's foot at -50.6).
* OCCT's blend fails at the flats. Each arc's own corner, revolved just past the plane at its ends (section 40), left
  out the material under the flats that the whole rim's round takes (47.30 of 52.92 mm3 found). The arcs of one circle
  rounded alike now have their corners revolved on to the middle of the gap on each side, where they meet; each is
  trimmed by the planes at its ends as before. Together they are the whole rim's corner cut off by the flats. The
  rebuild matches the stored body to the boolean (no volume either way); its round is two torus faces, Fusion's one.
* One whole-ring corner (revolved once, trimmed by the flats) was tried first: its cut depends on the boolean's
  handling of the round's tangent faces, and gave the right solid, a second solid left in the cut tool, or an invalid
  one, depending only on where the revolve's seam fell (and between two builds of the same code). Not kept.
* The corner volumes are now measured as `gs_mass` measures them (adaptive integration on tori): the default
  quadrature was 2e-4 to 6e-4 mm3 off here, more than the check's 1e-7 of the volume.
* 3777 then stopped at an intersection (`Courbes non jointives`) of two flatted bars on one cylinder, one of them
  bored at its ends and the other along its length; OCCT throws with the first as the object and gives the right
  common (559.8247 mm3, a stored solid) the other way round. A failed intersection is now retried with its inputs
  swapped. 3777 comes in as history.

### Offsets of cylindrical faces (3350 and 1054 OffsetFaces1)
* BRepOffset fails on both: on 3350's threaded body it builds offset surfaces of every face and throws on the thread
  flanks ("Geom_BSplineSurface: Weights values too small"), after some 45 s; on 1054 it returns no shape.
* 3350's two faces are the walls of an annular pocket (r 4.9 and 6.0 mm, z 1.1 .. 1.3), each with a 0.2 mm round
  at its top rim. Fusion's end body has the walls at 4.8 and 6.1 and the rounds moved with them: tori R 4.6 and 6.3
  r 0.2, still tangent to the top plane z = 1.5, which stays (the areas of all four faces match the rebuild to 1e-4
  mm2). BRepOffset, where it does build such a case, carries the offset on along the tangent rounds to the top plane
  and lowers the whole top (a test body lost 23.3 mm3 where Fusion's rule takes 2.74).
* `offset_cylinders`: a cylinder whose edges end it on planes across its axis (or on tangent rounds about the axis
  that end on such planes) or run along the axis beside planes along it, is offset as a boolean: the section between
  the face (with its rounds) and its offset place (the rounds moved by the same amount, as a round of the moved edge
  is) is turned about the axis and cut away (an offset into the material) or added. The material taken (the empty
  space filled) must be bounded on the old surfaces by exactly the face and its rounds (their areas), and the volume
  change must be that material's. Faces of the result left beside a plane that grows take that plane's origin (on
  3350 the boolean's unification is refused: the threaded body is itself invalid to BRepCheck). Used first where a
  face carries a round, else after BRepOffset fails (golden tests `a_hole_beside_slots_is_widened`,
  `a_pocket_wall_offset_carries_its_round`). 3350's offset now builds, without the 45 s BRepOffset attempt.
* 1054: the offset's face lies on two bodies: the hole of `Extrude2` and the side of `Extrude5`'s plug (r 1.5, 1 mm).
  The plug was extruded from three regions of the circle and keeps its side in four faces, each holding a part of the
  pick's 65 points; the hole's one face holds them all and took the offset, though the record names the plug's body.
  Where faces of the body the record names hold every point between them, the offset now takes those faces (test
  `an_offset_on_a_body_whose_face_is_in_pieces_offsets_the_pieces`). The plug is then 1.4 mm, as the end blob has it.

### Where the designs stop now
* 3350: every feature builds; the step check finds a stored solid of 135.93 mm3 that is neither a state nor a cut
  tool: a ring r 8.6 .. 11.5 mm with a `rot_spl_sur` face, read as its B-spline approximation (the stored blob's
  135.988 mm3), likely a revolve's cut tool. Not the offset's.
* 1054: `Fillet1` (r 0.6 on `Extrude5`) names a whole circle of radius 1.5 at the plug's foot; after the offset the
  foot is 1.4, so the pick lands on the hole's mouth in `Extrude2` (two arcs between the slots), where the blend fails.
  `Chamfer1` (on `Extrude2`) likewise names a whole circle r 1.5 at z = 0, which the slots of `Extrude4` cut. The
  stored edges look recorded before the offset and the slots were in place (features put earlier in the timeline):
  a reader question, not examined.
* 4632 (`Extrude11`, a surface extruded up to a face, then Thicken1 and a pattern of both): not attempted.

### Result
* adafruit-next: 119 of 150 as history (2278 and 3777 gained; none lost; 117 before). adafruit-more: 133 of 150
  (unchanged, none lost). adafruit: 13 of 18 (unchanged, none lost).
* First blockers of the round's designs now: 2278 and 3777 pass; 3350 the step check (a 135.93 mm3 stored solid,
  likely a revolve's cut tool with a `rot_spl_sur` face); 1054 `Fillet1` (edges recorded at the plug's radius
  before the offset); 4632 `Extrude11` (the reader: "profile is not a sketch profile").

## 45. Round 25 U: occurrences of streams without class tags, a mirror of drafted faces, stale profile loops (2026-11-12)
Readers: `fusion_tree/joint.rs` (`paths`), `fusion_tree/mod.rs` (`occurrence_placements`), `fusion_tree/pcb.rs`
(`box_primitive`), `fusion_tree/sketch.rs` and `geom.rs` (`cycle_of_curves`), `fusion_tree/bodyops.rs` (`draft_json`),
`fusion_tree/json.rs` (mirror of faces); importer: `tree_import.rs` (`loop_points`), `tree_import/v2.rs` (draft mirrored).

### Occurrences the bodies reader does not find (5907)
* The bodies reader (`fusion.rs`) finds occurrences by the class tags `24 00 00 00` + GUID in the BulkStream. Some
  streams carry no class tags at all (5907: none of FF415D89, CE2913AA, F9CB8717, E03784ED appears in the
  BulkStream; the classes are in the MetaStream's table only). The reader then falls back to the older layout's
  blocks: listed instances (`name:designator`, right) and, where a design lists instances, a component's
  `CreateComponent` matrix. In 5907 that matrix put the mirrored component's source (a 1x20 header, a Base Feature)
  a quarter turn about x at the board's centre, off the board.
* In the framed stream the occurrence is `CE2913AA` (its first `E03784ED` reference is its component; its `904F885E`
  list names the component that holds it), the first rigid 4 x 4 of its record is its placement in that component,
  and every occurrence path has a `549FBB80` record (section 24) with its placement after each timeline step. 5907:
  the header's occurrence and path record both store (1.9, 1.3, 1.57) mm, unturned (the header's blob spans
  x -1.27..49.53, z -3..8.5 mm: it stands on the board top, z 1.57, pins through it); the mirrored component's
  diag(1, -1, -1) at (1.9, 19.02, 1.57) mm. The source to new map is then the reflection in y = 10.16 mm, the
  mirror's plane, and the new blob the source's reflected in its own z = 0, as section 43 found.
* For a stream in which the bodies reader finds no occurrence record, every body is now placed by the last stored
  placement of its component's path records (joint.rs's placement rule: a blob already placed the same way, or
  unplaced at the origin, or with more placements than the paths give (a board's listed parts), is left alone).
  Streams with tagged occurrence records are not touched. Over the three sets (301 streams) this changes the
  placements of 22 streams (20 designs; 17 histories before), all of them in the final runs below. 5907 comes in
  as history.

### 3000: the box primitive's frame, the draft's pull, the mirror of drafted faces
* **Box frame.** The corner frame a `BoxPrimitive` stores is in the frame of the origin snapshot (`7B6C3C2D`) the
  record names first (its first rigid 4 x 4). 3000's join box stores (4, -18.85, 0) mm, x (0, 1, 0), y (-1, 0, 0);
  its snapshot is a half turn about z, which gives (-4, 18.85) mm, x along -y, y along +x: the box x -4..-3,
  y 16.35..18.85 mm, where the stored body and Draft1's face have it. 2354's boxes have such snapshots too (half
  turns about z with z offsets 0.5..0.62 cm, quarter turns); its first box is refused before (section 43), so it is
  not confirmed there. The 60 PCB boxes name no snapshot.
* **Draft pull.** Draft1 (older layout, flip byte 0, `-20.0 deg` and `0.0 deg`) tapers the box's top (outward +z)
  about the holder's wall `5 [306]` (y = 18.8336 mm, outward -y). By section 39's rule (the face's normal turned
  towards the pull, the neutral face's outward normal, by the angle) the top rises towards -y; the stored drafted
  face `2 [318, -319]` falls towards -y (z 3 at the wall, 2.096 at y 16.35: normal (0, -0.342, 0.940)), which only
  the reversed pull gives. Nothing in the record that differs from 355's drafts (diffed byte for byte against 355,
  258 and 324) explains it. The reader now compares each drafted plane face the stored bodies hold after the draft
  (its name carrying the draft's operation, negated) with both turns: where some agree only with the reversed pull
  and none with the outward one, the draft is flipped. 355's drafts agree with the outward pull and stay as they
  were. Volumes: no draft 158.6233 mm3 for the holder, drafted the old way 159.7459 (+1.1226), the right way
  157.5007.
* **Mirror of faces.** `Mirror1` selects four faces of the box, one of them `2 [318, -319]`: a face a later feature
  changed names that feature's operation negated (section 28). A mirror of faces now takes `[op, -k]` where `k` is a
  draft's operation, and mirrors the draft with its maker when the selected faces are all of the draft's faces (no
  blends). The importer mirrors a draft by reflecting its faces' points and surfaces and its neutral plane (a
  reflection keeps the turn towards the reflected pull). The holder then rebuilds to 163.8291 mm3, the stored value
  (157.5007 + the mirrored drafted box 6.3283).
* **Profile picks near arcs.** Extrude1's region (the board: a circle of 25.4 mm less a notch, so one arc of 343
  degrees) was tested against 16 chords, 0.44 mm inside the arc; a pick 0.28 mm from the rim (between the rim and
  a 1.6 mm hole 0.5 mm from it) fell outside every region. Arcs now take one chord per 3.75 degrees (16 to 96).

### Profile loops recorded against an earlier sketch (3000's Sketch7)
* Sketch7: an outline 7 x 3.55 mm, a 3 x 1 mm rectangle on its bottom edge and a line up the middle. Extrude10's and
  Extrude11's loops list the outline's top edge as pieces `1/3, 2/3, 3/3` and `3/4`, with the small rectangle's
  curves: they were recorded when the rectangle sat on the top edge (cutting it in three, four with the middle
  line). Extrude12, made later, lists the bottom edge `2/3` beside the rectangle, as the sketch is now; the stored
  bodies (Extrude10 over the whole outline z 0..1, Extrude11 on the right half of the small rectangle z 1..2,
  Extrude12's cut z 0.8..1 over the small rectangle) follow the sketch as it is now. The records keep the piece
  numbers of their own time; the profile record (`0D57BD2F`) holds keys and piece numbers only, no geometry.
* Where an outer loop neither reproduces its pieces nor closes, its curves as they are now, cut wherever any curve
  of the sketch crosses or touches them, are taken when they bound exactly one face (pieces with a loose end are
  dropped until none is left; what remains must be one simple cycle). Extrude11: the right half of the small
  rectangle. A region with children whose children needed this, or failed, is its outline's faces, one pick in each
  (Extrude10: the four faces of the outline). Only loops that failed before take this path.
* 3000 comes in as history (17 stored solids are states of the rebuilt model, 18 cut tools).

### 3722 (not attempted)
* The coil's axis, start and hand are still not decoded, and after `CoilPrimitive1` come Move1, Combine1, Chamfer1
  (an unresolved edge), the Cap component's coil, move, combine, a to-object cut, a chamfer and Shell3, which OCCT
  fails on (section 25). Too far for this round; the box finding above (a primitive's frame is in its origin
  snapshot's frame) is the first thing to try on the coil's `7B6C3C2D`.

### Result
* adafruit-next: 119 of 150 as history (3000 and 5907 gained; none lost; 117 before). adafruit-more: 133 of 150
  (unchanged, none lost; every history among the streams whose placements changed stays one). adafruit: 13 of 18
  (unchanged, none lost).
* First blockers: 3000 and 5907 pass; 3722 `CoilPrimitive1` (not attempted).

## 46. Round 25 V: extrudes from construction planes, the side of a split, coils, thin walls round arcs (2026-11-12)
Readers: `fusion_tree/feature.rs` (`extrude_start`), `fusion_tree/pcb.rs` (`coil_primitive`), `fusion_tree/combine.rs`,
`fusion_tree/naming.rs` (`line_on_cylinder`), `fusion_tree/thin.rs`; importer: `tree_import/v2.rs` (`facing_plane`).

### 817: an extrude that starts from a construction plane
* `Extrude2` and `Extrude3` (symmetric, 4.2 and 4.7 mm) have the start kind 2 ("from an object") after the enum
  block's flags, `ProfileOffset` 0, no face recipe, and a selection `2CA5A1CD` -> `5A1BF548` -> `CC54ECAD`: a plane
  through (13.9, 7.95, 5.15) mm with x and y the world's (a work plane through the middle of `Extrude1`'s
  27.8 x 15.9 x 10.3 mm box). The stored tab (`Extrude2`) spans z 3.05..7.25 mm and `Extrude3`'s bodies 2.8..7.5 mm:
  both are centred on that plane. The reader started them on the sketch plane (z -2.1..2.1), so the tab's side was
  cut in two by the box's bottom and `Fillet3`'s edge midpoint (3.4, 15.9, 0) mm was the reader's own wrong place.
* An extrude of start kind 2 with no face recipe and no to-object end now starts on that plane (its distance along
  the sketch normal, plus the offset); a plane not parallel to the sketch, or more than one, is refused. 817 comes
  in as history.

### 770: the side a split keeps
* `Split1`'s plane is z = 5 mm with the normal (0, 0, -1); the split's new body is the side the normal points to: the
  lower one (`Extrude3` joins feet to `Split1`'s body, and the stored lower body, z 0..5 mm, carries them). The
  importer wrote the plane as `plane (base: XY, offset: 5 mm)`, whose normal is +z, so the document's new body was
  the upper half and the feet were joined to it; the lower body kept no rim at x 3..15 mm (`Fillet3`'s pick
  (9, 33, 0) mm). A split's plane is now written facing the tree's normal: a base plane turned half a turn about its
  own x (`X` on XY and XZ, `Y` on YZ), then offset along that; a tilted plane facing the other way is refused. 770
  comes in as history.

### 3722: the coil
* **Frame and hand.** The coil's origin snapshot (`7B6C3C2D`) in the root has no matrix (where the Cap's holds a
  typed reference to `F670456D` and a 4 x 4, it holds `cd f0 00 01` and zeros); the Cap's is diag(-1, -1, 1), a
  half turn about z. The stored helices (`helix_spl_line`, `helix_int_cur`: centre, major, minor, pitch vector, then
  `t` 0..3 pi) are: root, major (3.55, 0, 0) cm, minor (0, 3.55, 0), pitch 0.4333 along +z; Cap, major (-3.55, 0, 0),
  minor (0, -3.55, 0). Both are right-handed (minor = z x major); the root's starts on +x, the Cap's on -x. So the
  coil climbs its snapshot's z from the snapshot's origin and starts on its x. The hand is not read: both coils
  have `u8 1, u8 0` after the `Height` holder, and no sample is left-handed. Coil 1 also selects the XY plane at
  the origin (`5A1BF548` -> `CC54ECAD`), coil 2 a face of the Cap (`5662F619`); both coils stand at their
  component's origin, and neither that selection nor the `DA1231DE` entity is used.
* **Section.** Position and type as section 40 read them: (3, 3) the triangle's side on the diameter, its apex out;
  (1, 4) its apex in; other pairs are refused. Pitch = `Height` / `Revolutions` (a coil with a `Pitch` parameter, a
  taper or an operation other than a new body is refused).
* **The tree.** A sketch in the axial plane at the start (x along the frame's x, y along its z) holding the triangle,
  and a `sweep` along a helix of that pitch and height. The document builds coil 1 to 743.658 mm3 (stored
  743.6554). Where the history blob holds the coil's body, its vertex box must have the size of the coil's ends; the
  state lists of 3722's coils do not name their own bodies ("a body is not in its own state list"), so there the
  importer's state checks alone confirm the coil.
* `Move1` then reads (the coil is a one-body sweep); `Combine1`'s target is `Revolve2`: a revolve that makes one
  body now counts as a combine's target or tool. `Chamfer1`'s second edge joins the coil's end face `2 [312, 314]`
  and the revolve's cylinder `5 [307]` (r 33.5 mm, stored only), ended by the coil's helical faces. No stored state
  holds the joined body before the chamfer, and the stored end face is what the 1.99 mm chamfer left of a 2 mm
  triangle (points at r 35.49..35.5 mm). A plane along a stored cylinder's axis meets it in two lines; the edge is
  the one on the side of the plane face's stored points, given as a 0.2 mm piece about their middle projected onto
  it, which the model's edge must hold (`line_on_cylinder`).
* **Rebuild (not reached).** Three features fail in OCCT:
  - `Shell3` (2 mm inside, the bottom opened), which comes before the coil: `Revolve2`'s dome is an arc of r 54 mm
    about (0.658, 61.865) mm, so the surface is a spindle torus meeting the axis at z 115.861 mm, 0.7 deg off
    square. A standalone experiment (the revolve, with and without `Fillet2`) fails in all four `MakeThickSolidByJoin`
    modes (arc or intersection joins, with or without the thorough intersection); the same body with the arc centred
    on the axis (a sphere) shells; built on an exact `Geom_ToroidalSurface` it throws in
    `Geom_OffsetSurface::EvalD0`; as NURBS it fails. Fusion's stored inner face is the torus of minor radius 52 mm,
    meeting the axis at z 113.861 mm. An offset of a cone point on the axis is what OCCT cannot do here.
  - `Chamfer1` and `Chamfer1 (2)` (1.99 mm): OCCT chamfers either edge of the coil alone (740.59 mm3), but on the
    joined body it fails even at 1 mm with "ChFi3d_Builder: only 2 faces": the coil's side on r 33.5 mm is a B-spline
    face of the sweep that the join does not unify with the revolve's cylinder, so the chamfer ends at a vertex
    between tangent faces. Sweeping a line parallel to the axis into an exact cylinder face (a helix is a straight
    line in the cylinder's parameter plane) would let the join merge them; not done (the helical sweep is shared with
    the modeled threads).
  3722's first blocker is now `Shell3`.

### 5998: the side of a thin wall round an arc
* `Extrude6 (2)` is a thin cut (0.4 mm, location 2) along a U: a line up the right leg, an arc of r 1.4 mm about
  (-5, 4.6) mm, a line down the left leg. Its stored faces: the sides on the chain `10 [384, 385]` and
  `12 [384, 385]` on the sketch's lines, the others `10 [384]` and `12 [384]` 0.4 mm outside, the round end
  `11 [384]` r 1.8 mm. The wall lies outside the U; the reader had put it inside (left of the chain), so `Chamfer7`'s
  edges on the outer sides and the legs' round ends (`Fillet5`'s r 0.2 mm faces `[386]`) were not where the stored
  cylinders are.
* The selections' own geometry (`50AEE8B4` lines: point and vector; `B0B3FA59` the arc: centre, normal, reference
  direction, radius, sweep) runs the chain the reader runs: up the right leg, counter-clockwise about +z, down the
  left leg. The wall is to its right. 5981's `Extrude3` (its stored edges) and 6299's `Extrude7 (2)` (its stored
  vertices at x -2.8 and -6.8 mm about legs at -3.4 and -6.2 mm) also lie outside their arcs, and their chains run
  the arc clockwise (to the left). The extrude's direction does not decide it (5981 goes against its sketch's normal,
  6299 and 5998 along it). Location 2 is now the side outside the chain's arcs; a chain of lines keeps the left (no
  sample tells), one whose arcs turn both ways is refused.
* `Chamfer7` now resolves; 5998 stops at `Loft1` (not read): `B6678572` with two `2F3200BA` sections, each a
  `2CA5A1CD` -> `4BD53E5A` profile (`Sketch6`'s circle r 1.4 mm about (-3.54, 5) at z 7.8 mm, and `Sketch5 (2)`'s
  circle about (-5, 4.6) at z 10 mm), a `9716F783` body selection and a `B0387387`. `Extrude7 (2)` after it extrudes
  a face with a -45 deg taper. Not attempted.

### Result
* adafruit-next: 121 of 150 as history (817 and 770 gained; none lost; 119 before). adafruit-more: 133 of 150
  (unchanged, none lost). adafruit: 13 of 18 (unchanged, none lost).
* First blockers: 817 and 770 pass; 3722 `Shell3` (OCCT cannot offset the spindle torus's point on the axis), then
  `Chamfer1` and `Chamfer1 (2)` (the coil's side is not one face with the revolve's cylinder); 5998 `Loft1` (not read).

## 47. Round 26 W: the stored nut read exactly, coils on exact cylinders, a shell of revolution (2026-11-12)
Reader: `gusset-sat` `step.rs`, `step/exact.rs`; kernel: `gusset_occt.cpp` (`measured_props`, `exact_helix_cylinders`);
importer: `import.rs` (`fusion_instances`).

### 3350: the stored tool was exact, its count was not known
* The stored solid of 135.9311 mm3 is one of two rings (z -5.5..-4.5 and -2.5..-1.5 mm, r 8.6..11.5 mm) that
  `Revolve3` cuts from the hexagon nut's corners with `Sketch7`'s two degree-5 spline profiles. Its `rot_spl_sur`
  face's own approximation is exact (the profile's poles times a rational quadratic circle), and Pappus over the
  profile gives 135.93110: the reading was right. No rule could place the tools because a revolve had no copy
  count: a cut or join revolve now counts one tool per profile loop, as an extrude does (fewer where a loop changes
  no material). The two rings then explain `Revolve3`; the check went on to the end bodies.
* The stored nut then measured 347.7759 mm3 where the rebuild gave 347.8530 and the analytic nut (Pappus for the
  corners, 12.3634 mm3 cut; the thread's ring 70.5297 joined and its groove, section 0.30856 mm2 at centroid radius
  7.74263 mm over 3 mm, 45.0336 cut) is 347.85246. Two faults in reading and measuring it:
  - **Intersection curves.** The hexagon's sides meet the revolved splines in `int_int_cur` edges, cubic fits within
    0.68 um that lie a mean of 0.09 um off the revolved surface. OCCT trims each face at the fit's projection onto
    its own surface, so the faces of an edge left a gap: the nut's part outside r 8.3 mm measured 276.2377 against
    276.2692 for the same solid made by booleans. Where both surfaces of an `int_int_cur` are planes or revolved
    exact B-splines (`rot_spl_sur` of an `exact_int_cur`), the curve is now rebuilt on them: Newton steps from the
    fit's own points onto the line where the two tangent planes meet (exact nearest points: the plane's projection,
    the revolved profile's nearest point in the meridian plane), Hermite pieces, sixteen to each span of the fit, on
    the fit's parameters, the tangent along the normals' cross product. A point that moves more than 1e-3 of the
    curve's size keeps the fit (a misread surface). That part now measures 276.2692.
  - **Quadrature.** A shape with a wound face (the thread's crest, a cylinder strip of several turns: section 36)
    was integrated by OCCT's default quadrature alone, which put the nut's trimmed spline faces 0.049 mm3 off. The
    wound faces and the others now go into two compounds (each face taken about the same origin) and are
    integrated each their own way: the default quadrature for the wound faces, the adaptive one for the rest, as a
    shape without a wound face already was. The stored nut: 347.8520 mm3 (1.3e-6 off the analytic value), the parts
    inside and outside r 8.3 mm adding up to it.
* 3350 comes in as history: 10 stored solids are states and 5 are cut tools, the finished bodies the end blob's.

### 3722: the coil on an exact cylinder
* A profile line parallel to a helix's axis sweeps a strip of a cylinder (a helix is a straight line on the
  cylinder's parameter plane). MakePipeShell gives it as a B-spline; the face is now put on the exact cylinder (its
  edges kept, their pcurves projected anew and shifted into one strip of the parameter plane) and split into half
  turns: OCCT's union of a rod with a coil whose cylinder face wound one and a half turns returned the rod alone. The
  change is kept where the swept solid stays valid and its volume stays within 1e-5 (the slanted sides are fitted,
  2e-6 measured). Thread grooves have such sides (the crest and root flats); 1145 and 3350 still rebuild to their
  stored bodies.
* Golden test `a_helical_sweep_of_an_axial_line_is_a_cylinder_face` (3722's dimensions): with the coil's side a
  B-spline both chamfers of the joined body failed ("ChFi3d_Builder: only 2 faces" on the start's edge,
  "TopOpeBRepDS_DataStructure::Point" on the end's foot); now both build, 1.99 mm, alone and together (the rod's
  seam turned half a radian from the coil's start, below).
* **New blocker: the seam.** In 3722 the coil starts at +x, on the seam of `Revolve2`'s cylinder (the revolve's
  profile lies in the XZ plane at +x). The start's chamfer, whose edge ends on that seam, still fails ("chamfer
  failed": the chamfer builder is not done); with the rod turned half a radian it builds. Splitting the closed faces
  (`ShapeUpgrade_ShapeDivideClosed`, into two or three pieces) gave invalid blends or wrong volumes and was dropped.
  Moving a revolve's seam (or a blend fallback that rebuilds the closed face on its surface turned about the axis
  and lets `ShapeFix_Face` add the seam elsewhere) is the next step for `Chamfer1`; `Chamfer1 (2)` (the Cap's coil,
  started at -x on the Cap's own base) was not examined further.

### 3722: Shell3 in the half section (tried, not kept)
* The shell of a solid of revolution is the solid less its cavity: the section through the axis offset inward by the
  thickness (both halves, so the pole is a corner of it: Fusion's inner torus of minor radius 52 mm meets the axis at
  z 113.861 mm), extended through the opened faces, revolved. The cavity of `Revolve2` with `Fillet2` is, by
  Pappus, 2 pi times the integral of r times its height under the circle of radius 52 mm about (0.658, 61.865) mm,
  for r up to 31.5 mm (the 2 mm round offset by 2 mm is a sharp corner: the fillet's centre).
* An experiment in the shim (OCCT's thick solid first; the fallback for a solid every face of which is a plane
  across one axis or a surface of revolution about it, a spline round included where its normals lie in planes
  through the axis) failed in OCCT's 2D offset: `BRepOffsetAPI_MakeOffset` on the section (its dome arcs are
  B-splines from the boolean section, its round an arc of radius equal to the thickness) returned an open wire of
  one edge. A working version needs the section's curves as lines and arcs (circles fitted to the section's
  splines, or the meridians taken from the faces' own surfaces) and convex arcs no larger than the thickness
  replaced by the corner of their neighbours before the offset. Not committed; 3722's first blocker stays `Shell3`.

### Result
* adafruit-more: 134 of 150 as history (3350 gained; none lost; 133 before). adafruit: 13 of 18 (unchanged, none
  lost). adafruit-next: 123 of 150 (unchanged, none lost).
* First blockers: 3350 passes; 3722 `Shell3` (OCCT's thick solid; the half-section fallback needs its own 2D
  offset), then `Chamfer1` (its start edge ends on `Revolve2`'s seam).

## 48. Round 26 X: Loft, Pipe, an extrude from a loft's end, surface extrudes and Thicken (2026-11-12)
Readers: `fusion_tree/loft.rs` (`loft_json`, `pipe_json`, `profile_of_a_loft_cap`), `fusion_tree/json.rs`
(`sweep_path`, shared with sweeps). The document already has `loft` and `sweep`; no document feature was added.

### Loft (`B6678572`)
* The corpus has four: 5998 `Loft1`, 6299 `Loft1` and `Loft2`, 3221 `Loft1`. The body's first byte is the operation
  (as a sweep's; 1, join, in all four), then four flag bytes (`01 01 01 01`, 3221 `01 01 01 00`; not read), then
  `ff ff ff ff` and zeros. The sections are a list (`u32 n`, references padded with two zero bytes) of `2F3200BA`
  records; each names one profile selection `2CA5A1CD`, whose child is a `4BD53E5A` sketch profile (5998, 6299) or a
  `5662F619` face (3221: the faces `2 [3]` and `2 [4]`, end caps). The loft's own `2CA5A1CD` -> `9716F783` names the
  bodies it joins (5998: the case's `Extrude1 (2)`).
* A section of a sketch profile is read as the profile's one region (as an extrude's regions are found); the tree
  loft takes copies of the sketches that hold just that region's curves (`Loft1 section k`). An arc drawn over a
  circle of the region is left out of the copy: 5998's `Sketch5 (2)` holds the U of the thin cut `Extrude6 (2)`, whose
  end arc lies on the section's circle (r 1.4 mm about (-5, 4.6) mm), and the region's pieces run on both.
* 5998: section 1 is `Sketch6`'s circle r 1.4 mm about (-3.54, 5) mm at z 7.8 mm, section 2 the circle about (-5, 4.6)
  mm at z 10 mm: an oblique cylinder (the stored loft face is the elliptical cone the bodies reader skips). A section
  of a point or a face, rails or a centre line (a `5A1BF548` reference) are refused.
* The tree loft is checked as a sweep: one operation of the four and, for a join, one body of one feature.

### An extrude from a loft's end face
* 5998 `Extrude7 (2)` and 6299 `Extrude8`: 0.4 mm, taper -45 deg, join; their profile is a face (`5662F619`) named
  `1 [op]`, the op being the loft's. The face's plane, from the stored bodies, is the plane of one of the loft's
  sections: that section's copy and region point are the extrude's profile. 5998's is section 1 (z 7.8 mm): the
  stored case has a 45 deg cone of r 1.4 mm with its apex side at (-3.54, 5, 7.8) mm. 6299's is its section 2.

### Pipe (`73615646`)
* Four in the corpus: 64 `Pipe1`..`Pipe3`, 3221 `Pipe1`, all in records with typed references. The body: `u8 0`, then
  the operation as a `u32` (2 cut in 64, 4 new in 3221, as an extrude's), `01 01` (not read), then parameter holders
  `AlongDistance` (1: the whole path), `AgainstDistance` (0), `SectionSize` and `SectionThickness`, a `9414C87F`
  (generic), the path selection `2CA5A1CD` -> `5A1BF548` -> `E2CEFD18` sketch curves (read as a sweep's path).
* After the path selection's reference and two zero bytes, a `u32`: 0 in 3221, whose stored pipe is a cylinder of
  r 1.5 mm (half the 3 mm `SectionSize`) about the path with no inner face (so `SectionThickness` 0.75 mm is not a
  wall: not hollow); 1 in 64, whose stored cut tools are square prisms 2 x 2 and 1.5 x 1.5 mm (the `SectionSize`)
  centred on the path lines, sides along and across the path sketch's normal. The square's `u32` is followed by a
  `f64` 0 (taken for its angle) where the circle's is not. 0 is read as a circle, 1 as a square; others refused.
* The tree: a sketch across the path's free start (`<pipe> section`, y along the path sketch's normal) with the
  circle or square of the section size, and a `sweep` along the path curves (orientation fixed for one straight
  line, Frenet otherwise). Only a whole path (along 1, against 0), of lines and arcs in one open chain, is read.
* 64 comes in as history: three square cuts along the breadboard's top edges (the file stores the box and the
  three tools; 45347.5 mm3 of box, 722.5 mm3 of tools).

### Surface extrudes and Thicken (not done)
* 4632 `Extrude11`: the extrude record's enum block is op 4 (new), extent 1, kind 2, then start kind 2 (from an
  object: the face selection `2CA5A1CD` -> `5662F619`); its profile selection is `2CA5A1CD` -> `5A1BF548` (sketch
  curves: the open B-spline `c5017` of `Sketch` at 4917), not a `4BD53E5A` profile; `AlongDistance` 5 mm,
  `ProfileOffset` 0. `Thicken1` (`BCE0AD0E`): `Thickness` 0.5 mm, two body selections: `Extrude11`'s sheet and
  `Extrude10`'s body (5066). `C-Pattern1` then copies both 12 times. The document has `surface_extrude` and
  `thicken` (ADR 0036), but the tree importer reads neither, the thicken's side flag and whether it joins
  `Extrude10` are not established, and a pattern of a surface and a thicken has not been built; not attempted.
* 2530: `Extrude2` and `Extrude3` (0.9 mm each way) extrude open curves of `Sketch3` as surfaces (the same kind 2 and
  start kind), `Thicken1`, then a `Fillet1` whose edges are on the thickened body, two `RemoveBody` of the sheets,
  `Mirror1` (it mirrors 1 body and makes 2) and `Move1` (4 bodies): out of reach with this round.

### Result
* 64 comes in as history. 5998 now reads past `Loft1` and `Extrude7 (2)` to `Extrude11 (2)`, a cut of a sketch
  text (`92F9A5F4` profiles of the `Labels` sketch, as section 20's Cases): out of reach (the glyphs are not in the
  file). 3221 reads `Pipe1` and stops at `Extrude7` (a cut of 0.5 mm from a face of `Pipe1`), then `Loft1` (two
  face sections). 6299 still stops at `Extrude17` (a face profile), with `Loft2` ("loop does not close") and
  `Fillet4 (2)` (one edge unresolved) after it.
* adafruit-next: 124 of 150 as history (64 gained; none lost against round 25 T and V); adafruit-more: 133 of 150
  (unchanged, none lost); adafruit: 13 of 18 (unchanged, none lost).

## 49. Round 27 Y: face profiles, surface extrudes and Thicken, sweeps along edges (2026-11-12)
Readers: `fusion_tree/surface.rs` (`surface_curves`, `Edit::thicken`), `fusion_tree/json.rs` (`profile_of_a_face`,
`face_pick`, `sweep_path_edges`), `fusion_tree/loft.rs` (face sections). Document: `profile: <face>` (a planar body
face as the profile of an extrude, sweep or loft; grammar.md), lateral faces `<feature>/along(<edge>)`. Kernel: a
face's boundary as a profile (`face_profile`, from the topology: lines, circles, arcs); the shim's thicken turns
an inside-out solid the right way out. Tree format: ADR 0042 amendment of 2026-11-12.

### Surface extrudes
* An extrude record (`DD405BC2`) whose profile selection `2CA5A1CD` names sketch curves (`5A1BF548` ->
  `E2CEFD18`, read as a sweep path's: the last 16 bytes are `u64 sketch`, `u64 crv_primary_id`) instead of a
  `4BD53E5A` profile sweeps those curves into a sheet. Lines, arcs and splines are looked up (4632's
  `Extrude11`: the open degree-5 B-spline `c5017` of `Sketch5`). Only a new body (op 4) is read so: 5998's
  `Extrude11 (2)`, a cut whose selection names the curves of a sketch text, is not a surface.
* The enum block does not tell a surface from a solid: kind (the `u32` after the extent) is 2 for 4632's
  `Extrude11` and for its solid `Extrude10`, 2 or 1 or 0 for other solids. The start (from an object: 4632's
  starts on the face 0.2 mm off `Sketch5`) is read as a solid extrude's, by a moved copy of the sketch.
* The tree writes `curves` on the extrude; the importer writes `surface_extrude`.

### Thicken (`BCE0AD0E`)
* Two records in the corpus: 4632 `Thicken1` and 2530 `Thicken1`. Body: `u32 1`, `u8 0`, the operation as a
  `u32` (as an extrude's: 1 join in 4632, 4 new in 2530), then `u32 n` and `n` selections `2CA5A1CD` -> `9716F783`
  (the sheets: 4632 `Extrude11`'s, 2530 `Extrude2`'s and `Extrude3`'s), two flag bytes (`01 01` in both), the
  parameter holder (`Thickness`), then a second list: for 4632's join the one selection of the body joined to
  (`Extrude10`'s hub), for 2530's new bodies its two body links `D26351F0` (the bodies made). The flags are not
  decoded; the tree reads the thicken as symmetric (Fusion's thicken input has `isSymmetric` and
  `isChainSelection`), which no rebuild has confirmed yet (below).
* The importer writes a join as `thicken` of each sheet in place and a `combine` into the target; a feature
  pattern of the sheet and the thicken (4632's `C-Pattern1`: both, 12 times about the hub's axis) repeats the
  combine. A new body thickens a `move` copy of its one sheet (the sheet stays, as Fusion keeps it: 2530 removes
  its sheets later with `RemoveBody`); several sheets into new bodies are refused.
* The shim's thicken (`BRepOffsetAPI_MakeThickSolid::MakeThickSolidBySimple`) returned 4632's blade inside out
  (volume -13.169 mm3 for a 26.338 mm2 sheet 0.5 mm thick, either side): it did not unite with the hub. Now
  reversed when its volume is negative. An L-shaped sheet of two plane faces still thickens wrong by the simple
  algorithm (487.8 mm3 where 580 is right): the corner is not joined; not needed here.

### Face profiles
* An extrude whose profile selection names one face (`2CA5A1CD` -> `5662F619` -> a face recipe) and that no
  earlier reading turns into sketch regions (an exposed cap, a loft's end) is written with `profile_face`, a pick
  of the face (a cap by its stored face's points, another plane by a point of it): blind, one-sided and straight.
  Its distance counts along the face's outward normal (3221 `Extrude7`: -0.5 mm, a cut into the tilted end of
  `Pipe1`; 6299 `Extrude17`: -1 mm into `Extrude1`).
* A loft section that names a face (3221 `Loft1`: the end cap of `Extrude1` at z 8.8 mm and the cut end of
  `Pipe1`) is written as `{"face": <pick>}`. `Loft1` joins both bodies: written as a join of the one and a
  `combine` of the other, as extrudes and sweeps of several bodies already are.

### Sweeps along body edges
* 6002 `Sweep1`: its path selection (`2CA5A1CD`, named twice in the record) holds three `5662F619`, each an edge
  recipe (`7ACC2A03`, `edge_recipe_data`), resolved to the edges' middles as a fillet's are. The tree writes
  `path_edges`; the importer finds the model's edges and writes `path: [edge(...), ...]`.

### Result
* 4632 now reads and builds `Extrude11`, `Thicken1` and `C-Pattern1` (the hub with 12 blades) and stops at
  `Fillet6` and `Fillet7` on `Split1`: their edges (the corners of an L-shaped wall, middles at y 2.6 mm, and its
  top rim) are not on the rebuilt `Split1`, whose nearest edges are 0.5 and 0.75 mm away (not looked into: the
  split and fillet readers). It would not pass the state checks either way: the file's only fan body is the plain
  hub (565.487 mm3 = pi 6^2 5, stored), no stored solid has a blade (the 98 sheets left out are the sheets), so in
  the source the blades never joined the hub (the Thicken most likely failed in Fusion; the record has no flag we
  know of that says so). The rebuild adds them, and the checks refuse it, rightly.
* 2530: `Extrude2`, `Extrude3` read as surface extrudes and `Thicken1` as a thicken of two sheets into new bodies
  (refused: the file stores two thickened bodies, 0.892 mm3 each, and which sheet makes which is not decoded);
  then `Fillet1` (four edges on the thickened bodies not resolved), `Mirror1` (one body making two) and `Move1`
  (four bodies) are still in the way.
* 3221 now reads in full and builds (`Extrude7` and `Loft1` included) and is refused by the state check "a
  feature changes a stored body that already is a finished body of the file": `Move1` and `Move2` move the
  Base Feature copies (section 48), which the file stores as they end. Not loosened.
* 6002 reads `Sweep1` and stops at a thin extrude (`Extrude10` and others) whose wall side (location 1) no rebuild has
  confirmed. 6299 reads `Extrude17` and stops at `Fillet4 (2)` (one edge not resolved); its `Extrude18` (one face,
  -0.6 mm) is not read yet. 5998's `Extrude11 (2)` is a cut of a sketch text, not a face.
## 50. Round 27: Fusion's Thread as a document thread, its side byte, blends beside threads and offsets (2026-11-12)
Document: `thread` (`gusset-doc`, `gusset-eval/src/thread.rs`, ADR 0034 amendment); reader: `fusion_tree/thread.rs`;
importer: `tree_import/v2.rs` (`thread`, `offset_face`), `tree_import.rs` (`moved_picks`).

### The thread feature
* Gusset had helical sweeps and cosmetic hole threads, but no thread on an existing face. `thread <Name> (face,
  pitch, [designation], [modeled], [major], [minor], [angle], [length], [offset], [along], [left])` is that feature:
  cosmetic by default; modeled, it builds Fusion's form of section 36 (the whole face moved to the major diameter of a
  shaft or the minor one of a hole, the basic groove cut one turn a sweep, square where the thread ends inside the
  face, run out through a free face end, stopped at a face end something looks back from: a shoulder, a hole's floor).
  Internal or external is the face's own side. It is composed from revolve, helical sweep and boolean calls; no new
  shim entry. The groove must take out half its share of the material at least, else the feature fails.
* A helical sweep whose helix did not start level with its section (the importer's old trees put every turn's helix
  at the world origin) swept the section the wrong way along an axis pointing down: an internal thread whose hole's
  axis ran -Z lost 1.67 mm3 where the plate's face crossed two turns' shared caps. Each turn's helix now starts at
  its section's middle height; golden tests (`modeling_thread.rs`) give the analytic volume on a shaft (also from
  the other end, left-handed, and built up where the thread is wider than the shaft), in a through hole of 3 and
  4.5 mm and in a blind hole (stopped at its floor), to 2e-5.
* The tree's `thread` feature replaces the sketches, rings, sweeps and combines the reader wrote: it picks the face
  at its middle and gives the diameters, pitch, angle, length and offset (`along` runs from the end the offset is
  measured from: the extrude's start for end code 2, its end for 1), modeled where a stored body holds its helices.
  A cosmetic thread that cannot be placed is left out with a note, as before. Later blends' edges on the moved face
  follow it about any axis.

### The side byte (3692)
* The byte after the flank angle is 1 in all six external records of the three sets (6g, 2A) and 0 in the four
  internal ones (6H); the byte after the form name, which had been read as the side, is 1 in every full-length
  thread (3692, 916 Thread2, 324, 3350 Thread2, 3221's 6H) and 0 in every one with a length. 3692's 1/4-56 UNS 2A
  is external, full length, on a 6 mm shaft: Fusion builds the shaft up to the 6.279 mm major diameter (the face
  moves either way), then cuts the groove. It had been built as an internal thread (431.5074 mm3 against the stored
  441.2291). 3692 comes in as history.

### Blend edges recorded before an offset (1054)
* `Fillet1` rounds the foot of `Extrude5`'s plug, which `OffsetFaces1` took from r 1.5 to 1.4 mm; its edge is
  recorded at 1.5 mm (as a thread's later blends are, section 36), where the hole of `Extrude2` around the plug has
  its mouth, and the round failed there. An offset of cylindrical faces now records each cylinder it moved (the
  radius one of the face's circular edges has after it); a later blend on that body whose edge lies wholly on the old
  cylinder, where the body has no edge any more, is picked on the new one. 1054 comes in as history.

### 916 Fillet1 (not solved; root cause)
* Reproduced on a document (a 16 mm rod 20 mm long, the M16x1 thread from 4 mm, the foot groove of `Revolve2`, the
  three 0.5 mm rounds): the fillet fails for an offset of 4 mm and of anything below it down to 3.99, and builds for
  4.0002 and above; with offset 4 it builds for radius 0.4999 and fails for 0.5 and 0.5000005. So OCCT fails where
  the round's plane-to-cylinder tangent line (z = -16.5 + r) lies on the groove's square end (z = -16), although the
  finished round, rolling on the concave one beside it (concave-first passes, section 40), meets the cylinder
  1.7 um lower (-16.0017, Fusion's torus). Vertex tolerances there are 5e-6 mm, but holding every tolerance of a copy
  to 1e-7 does not help (tried, not kept), and a radius 1e-6 smaller "builds" a solid 113 mm3 too large. Not a
  tolerance matter: ChFi3d's first guess of the contact runs along the crest face's boundary. Next: a blend of
  circular edges about one axis made in the meridian section (rounds rolling on each other) and revolved.

### 3350 and 3000 on `wall`
* On `wall` 3350 stops at its `Fillet1` ("fillet produced an invalid solid") after the half-turn split of
  helical bands; here its thread is the `thread` feature (one ring, then all the groove's turns cut in one boolean)
  and 3350 comes in as history again.
* 3000 (no thread at all) failed its step check on `wall`: the boolean's half-turn split took any cylinder face with
  a seam and an edge that is neither a line nor a circle for a helical band, so cylinders cut by other cylinders or by
  slanting planes were split and the booleans changed. A band's edge must now be a helix on it: its curve on the
  face's parameter plane straight (within 1e-4 of its length at seven points), turning at least a quarter round and
  climbing. 3000 comes in as history again; the gear, thread and sweep golden tests and the OCCT crate's tests pass.

### Result
* adafruit-next: 3692 and 1054 come in as history; 916 still stops at `Fillet1`; 3221 (Extrude, a face profile) and
  3429 (Fillet2, unresolved) as before. adafruit-more: 1145, 3350 history; 324 at Thread2 ("it is not on one face":
  a patterned thread on holes off the origin). Designs with offsets of faces (3027, 3464 history; 858, 6299, 5998,
  6002, 3093, 6126, the Crickit assembly at their earlier blockers) unchanged; 3000 history again (above).

## 51. Round 27 B: sketch text in a font Gusset has, the sheet-metal Bend, new box primitives (2026-11-12)
Readers: `fusion_tree/text.rs` (`read_text`, `extrude_texts`, `text_extrude`), `fusion_tree/bend.rs` (`bend_json`),
`fusion_tree/pcb.rs` (`box_primitive`), `fusion_tree/json.rs` (Base Feature bodies a Bend changes). Generator:
`tree_import.rs` (`text_feature`, `bend_feature`). Document: `bend` (ADR 0037 amendment, `gusset-eval/src/bend.rs`).

### Sketch text `E0618268` (5998, 6114, 6124, 6126, 6299)
* Five designs hold sketch text (19 records). The record: an `f64` 1 and an `f64` 0 and an `f32` 1, the font's name
  (`u32` length + UTF-16: "Arial", "Arial Rounded MT Bold", "Arial Narrow"), `u8 0`, the height (`f64`, cm; also the
  `D91D429C` holder that follows, whose parameter is the same value), `u8 0`, a `u32` code, three zero bytes, the
  string (`u32` length + UTF-16), a `u32` code, `u8 0`, the weight (400 or 700), five zero bytes, a 4x4 (row-major,
  cm), then `00 00 00 00 01`, `f32` 1 twice and the sketch's reference (`44A64366`) last. The 4x4 is a turn in the
  sketch plane and a shift: its first column is the baseline's direction, its translation the text box's corner
  (6299's "RESET" turned 75 deg, 6114's "USB" and "SOLAR" turned 90 deg). The two codes are 3 and 2 (5998's "+" and
  "-"), 3 and 3 (its "C" and "D"), 1 and 1 everywhere else: alignment or box kind, not read.
* An extrude of text names one `92F9A5F4` profile per text (5998's `Extrude11 (2)`: four); the profile's `90055C05`
  child ends in the text record's id as a plain `u64` (not a reference: `c5 22` = 8901, the "+").
* The glyph outlines come from the font, which the file does not hold. A cut of text stores its tools, one solid per
  text: 5998's four glyph prisms are bodies 0..3 of `BREP.274f068a...smb`, each exactly the cut's 0.4 mm deep from
  the sketch plane (x 14.1..14.5 mm on the plane x = 14.5 mm). Their boxes lie up and right of the text corners: the
  glyphs start 0.02..0.44 mm right of the corner and 0.31..0.90 mm above it ("+" at 3 mm: y -4.761..-2.780, z
  1.432..3.415 mm against the corner (-4.778, 0.947) mm). The same blob holds the glyphs' side sheets (a few nm wide).
* **Reading.** A text extrude becomes, per text, a `text` feature in a font Gusset has (`mono` for a fixed-pitch
  family, `sans` otherwise; no serif font is bundled). Where every text has its stored tools (solids exactly the
  extrude's reach from the sketch plane, at least 10 um wide, half or more of whose box lies in the box the text
  takes from its corner, 0.8 heights per character by -0.5..1.5 heights, and a quarter or less in any other text's),
  the tree adds them as `stored_body` features and a `combine` named after the extrude cuts (or joins) them: the
  texts stay as suppressed `text` features, centred on their glyphs, so they can be edited and switched on. Without
  stored tools the texts do the extrude in the substitute font and the rebuild check judges them (it is not
  loosened: a substitute font changes the volume, so such a design comes in as bodies). The import warns once per
  text: "the font 'Arial' is not in the file; the text "+" uses Gusset's sans font instead; it is kept suppressed,
  and the glyphs the file stores make the feature".
* A Gusset base plane whose normal is the sketch's reversed (`flipped`) would mirror the text: the text gets a
  datum plane with the sketch's own normal instead (the tilted placement).
* 5998 now reads past `Extrude11 (2)` and in full, then stops in the rebuild at `Draft1` (a face pick that two pieces
  of `Extrude5`'s end cap hold equally). 6124's "GND", "4.5V" and "3V" (Top's `Extrude4 (2)`, a cut of 0.28 mm
  capitals) match nine stored solids, one per character (3, 4 and 2), by the rule above; their placements carry
  3e-14 out of the sketch plane, accepted below 1e-9. 6124 then stops at `Fillet4` (four edges unresolved), with
  `Combine1` (its operation value 1 not confirmed) after it; 6299 at
  `Extrude17` (a face profile), 6126 at its thin extrude, 6114 has no feature history (an `.f3z` whose root design
  has none).

### Bend `1CF7B9E3` (3502)
* Read: `BendAngle` (-90 deg in both bends) and `BendRadius` (0.45 mm) are the record's own holders; the rule
  (`E75C86D6` -> `B42ED662`, "FPC cable (Convert) (Derived)") holds `Thickness` (0.05 mm) and, as the double right
  after its 36-character id text, the K-factor 0.44 (with "Aluminum", "Thickness * 4.0", ".45mm" around it). The line
  is `88277bc9` -> `2CA5A1CD` -> `5A1BF548` -> `E2CEFD18` (a sweep path of one line: `Sketch1`'s `c4724` at
  y -39.4, of two parallel lines 0.4 mm apart; `Sketch2`'s `c4940`). The `88277bc9`'s body is `01 00` and a `u32` 2 in
  both. The `69EE2FA7` holds two doubles, a zero byte and a point (cm, the body's component frame) of the face that
  stays: Bend1 (2.6807, 0.05, -4.6194) mm, on the flat flex's top face short of the line; Bend2 (4.6836, -2.5560,
  -4.6586) mm, on the outer face of the straight Bend1 stood up.
* **The geometry the file stores.** Component7's DM body in `BREP.1c3b3597...smbh` is the bent flex (5.080812 mm3, x
  0..9.47, y -3.3436..0.05, z -4.6586..2.4364 mm); its PM body in `BREP.f284f2bf...smb` (body 1 of 6) is the flat one
  (5.077285 mm3 = the outline's 101.5457 mm2 times 0.05 mm, z -14.63..0). Bent minus flat, 0.003527 mm3, is
  `theta t^2 (1/2 - k) sum w` with k = 0.44 and both bends across the full widths (9.47 + 5.5 mm). The bent box fits
  straights of 4.1586, 2.3936 and 6.595 mm with two zones of pi/2 (0.45 + 0.022) = 0.74142 mm each, together the flat
  14.63 mm: the first zone runs from z -4.1586 to -4.9 mm, where `c4724` lies (y 39.4 mm is 4.9 mm from the flex's
  edge at 34.5). So the zone ends at the line on the side that stays; the `u32` 2 is read as that and other codes are
  refused. Bend2 agrees: its line (z 2.72 mm on the stood-up straight) lies 3.135 mm (2.3936 + 0.7414) past the first
  zone's end, which puts that end at z -0.415 mm, where `Sketch2`'s projected outline has an edge. `Sketch1` holds the flat flex's whole outline (14.63
  mm long in one plane): the bends were drawn on the flat body, so the Base Feature's body a Bend changes comes in
  as its PM body.
* **Gusset's `bend`** (new): the document had no bend. `bend B1 (body: ..., line: S.l3, angle: ..., bend_radius: ...,
  k_factor: ..., [side: left|right], [position: center|start|end], [flip: true])`, evaluated with the kernel's
  booleans, revolve and transforms (no new kernel operation), OCCT test in `modeling_sheet.rs`. The tree's bend keeps
  the side holding the point; a negative angle turns against the source sketch's normal; both are mapped through a
  reversed Gusset sketch.
* **Not finished.** 3502 now reads in full; the import stops at its guard "a feature changes a stored body that
  already is a finished body of the file": the PM body is one of the file's `.smb` bodies, so the guard (which keeps a
  feature from passing because its stored body already is the end result) fires. It was not changed. With it set aside
  in a local experiment, `Bend1` finds the flex 1.525 mm above `Sketch1`'s plane and 5.94 mm along the line from where
  the sketch's outline has it (the tree places Component7 elsewhere than the sketch was drawn against; the bodies
  import places that body at a third place, mirrored in x): the placement of Component7's occurrence against the root
  sketches is the next step, then whether a dropped-input PM body may be changed (an owner's decision on the guard).

### Box primitives (2354)
* The corpus holds `BoxPrimitive` (78 records in 13 designs) and `CoilPrimitive` (3722) only; no cylinder, sphere or
  torus primitive occurs, so none is read. A new box whose history blob holds a later state of it (2354's
  BoxPrimitive1, 1 (2), 3, 8, 1 (4): "the history blob holds 3 solid bodies, its state list 1", "a body is not in its
  own state list") is placed by its signed height, as a join or cut is; where the stored body is the box itself it
  still decides the side. 2354 then stops at `Extrude1 (2)`, whose profile is a face (another round's work).

### 3093 and 3802 (investigated, unchanged)
* As section 43 found: 3093's `Combine1` targets one copy of `R-Pattern1` (link 26330) and the record does not say
  which copy a link is; with it let through the design stops at `R-Pattern2`, a pattern of the combined body. 3802's
  `Extrude4` join names the bodies of `Extrude1` and `Extrude2`, which later features (Extrude5, Split1, Extrude14)
  still name apart: the join merged neither into the other, and which one the tool joins is not in the record; with
  either it stops at `Fillet2` (four edges unresolved). Neither is in reach without a rule the files do not show.

### Result
* adafruit-next: 123 of 150 as history, adafruit-more: 133 of 150, adafruit: 13 of 18; the same designs as the base
  run of this branch's start (none gained, none lost). The rounding fix of 6124's text placements came after the run
  and was checked on 6124 alone.
* First blockers: 2354 `Extrude1 (2)` (a face profile; was BoxPrimitive1); 5998 `Draft1` in the rebuild (a face pick
  two pieces of one cap hold equally; was the text extrude); 6124 `Fillet4` (was the text extrude); 3502 the
  importer's finished-body guard (was "Bend is not supported"), then Component7's placement against the root sketches.

## 52. Round 27 G: geometry failures: posts rounded across, a dome shelled, a circle's path ends, legs mirrored (2026-11-12)
Reader: `fusion_tree/naming.rs` (`ExtGeom::away_from_profile`, `face_of_fillet`), `fusion_tree/feature.rs`
(`ExtGeom::bounds`); importer: `tree_import.rs` (`containing_edges`), `tree_import/v2.rs` (`new_body_copies`);
kernel: `gusset_occt.cpp` (`revolved_cavity`, `shell_by_cavity`).

### 2315: rounds of posts that use up their ends
* `Extrude12` joins four posts 8 x 4 mm (one extrude of eight regions: each post a rectangle less a circle, and the
  circle); `Fillet2` rounds their 16 vertical edges by 2 mm, which uses up the 4 mm ends (each end becomes a half
  cylinder); `Fillet3` rounds the posts' top edges by 1 mm. Its picks were 1.5 mm and 3 mm off the model's edges.
  Three faults:
  - **Which way a side faces.** A planar side's outward normal was taken away from the middle of all the profile's
    points (x 33 mm, between the posts): the ends of the posts right of the middle faced inward, and the rounds of
    `Fillet2` on them were put outside the posts (centres 4 mm from the model's). The normal now comes from the
    profile's own area where it can: probes 0.1 um either side of the face, tested even-odd against the curve
    pieces that bound the extruded area (those of one region only; a piece two regions share lies inside it), where
    one probe is in and one out. The middle of the points stays the fallback (open profiles, thin walls).
  - **Fillet face numbers.** `Fillet3`'s recipes name `Fillet2`'s faces `1` to `8` (the stored state after it has
    8 cylinders r 2 mm, one per end), not 16: the two rounds of a used-up end lie on one cylinder and are one face.
    Face `g` of a fillet is now the `g`-th distinct round in recipe order (two recipes whose rounds have the same
    radius and axis share a number); where a recipe's round cannot be worked out, the `g`-th recipe as before. The
    stored faces confirm all 8 (`4 [329]` is the left end of the third post, centre (37.62, -52.83) mm, and so on).
  - **Quarter arcs of a half circle.** Fusion keeps the two rounds of an end apart, so its top edge there is two
    quarter arcs; Gusset's is one half circle. A Fusion pick without a path is now also held by a model edge its
    midpoint lies on, away from the edge's ends (a line, or a circle's arc, within 0.01 mm); the blend's state check
    rejects a wrong guess.
* 2315 comes in as history.

### 3722: Shell3, the shell of a spindle dome
* `Revolve2` is a cylinder r 33.5 mm, 19 mm tall, under a dome: the arc of r 54 mm about (0.658, 61.865) mm, which
  meets the axis at z 115.861 mm (a spindle torus: its section dips to the axis in a notch of 1.4 deg). `Fillet2`
  rounds the corner by 2 mm (concave: the dome bulges past the cylinder there). `Shell3` opens the base, 2 mm
  inside. OCCT's thick solid throws on the point of the axis. New fallback in the shim, for a single solid opened
  at faces across an axis, inward walls only, when OCCT fails or throws:
  - the axis is that of the solid's faces of revolution (all on one line, every plane across it); the solid is one
    of revolution when Pappus on the half sections at two angles (0 and 2 rad) gives its volume within 1e-5;
  - the section through the axis (both halves: a point of the axis is a corner of it), with a rectangle 2 x the
    wall deep on each piece of an opened face's trace beyond it (OCCT's revolve leaves a line across the axis a
    surface of revolution: an opened face counts as a plane when its points lie at one height along the axis,
    within 1e-6 mm), unified, its boolean B-spline edges made lines and arcs (`GeomConvert_CurveToAnaCurve`, 1e-6);
  - offset inward in its plane (`BRepOffsetAPI_MakeOffset`, intersection joins; arc joins only where that fails and
    the one concave corner is on the axis, where a round over microns at radius 0 changes nothing), the half on the
    radial side kept and revolved: the cavity. A concave corner off the axis is refused (OCCT's 3D offset extends
    faces there, the 2D one would round them);
  - the shell is the solid less the cavity; the solid's faces keep their identity (the base becomes the ring left
    of it), each cavity face is an offset of the solid's face that lies the wall's thickness from it along its
    normal (the nearer of the two sides).
* Golden test `a_shell_of_a_spindle_dome_is_its_section_offset_and_revolved` (3722's dimensions): 72499.74 mm3 by
  Pappus (wall between the outer radius and the inner one: 31.5 mm, the round offset to r 4 mm about its centre,
  the circle r 52 mm to the axis at z 113.861 mm, as section 47 found Fusion's), three offset faces. The kernel
  measures the shell 72500.31 mm3 and the dome 725066.58 against 725058.50 by Pappus: the adaptive integration
  (`measured_props`, 1e-7) of the revolved arc face that reaches the axis is 1.1e-5 off, OCCT's default one is
  right (2e-8). Within the importer's 2e-5; not changed here.
* 3722 now builds `Shell3`; it stops at `Chamfer1` (the coil starts on `Revolve2`'s seam, section 47), then
  `Chamfer1 (2)`. A blend fallback that moves the seam of a closed face (rebuild the face on its surface turned
  about the axis, split the closed edges at the new seam, replace them in the neighbouring faces) was judged too
  large for this round.

### 858: the path ends of a cut circle
* `Fillet3` picks the bottom edge of `Extrude2`'s r 14 mm side between the rectangle's side x = 7.3 mm and the
  tab's line y = -3.5 mm. Gusset's bottom edge there is the whole circle (one vertex, at (-7.3, -11.946) mm), and the
  containing-edge rule (an edge holding the pick's midpoint and path ends) should take it, but the reader's path
  ends are (7.3, -11.927) and (13.538, -3.5) mm: 0.016 mm inside the circle (r 13.9836 mm), where its other
  points lie on it. Where no edge holds the ends, the path points next to them stand in for them. `Fillet3` and
  `Fillet4` now build.
* 858 then stops at `Extrude13` ("could not build an arc of the profile"): its sketch draws two r 0.5 mm arcs twice
  (`c4208` is `c4015`, `c4216` is `c4009`: same centre, start and end), as section 35's circles drawn twice; then
  `OffsetFaces2`. Not attempted.

### 1119: a mirror of legs and their notches
* `Sweep1` makes a leg (a new body), `Extrude3` cuts a notch 0.15 mm deep in it (targets `Extrude1` and `Sweep1`),
  `Mirror1` mirrors `Extrude3` and `Sweep1` across z, `Mirror2` mirrors `Sweep1`, `Mirror1` and `Extrude3` across x.
  A mirror of features is now a mirrored copy of each body where every listed feature is a sweep that made a new
  body, an imported mirror of such features, or a cut whose targets include one of those bodies (the copy carries
  the cut). Both mirrors build (four legs).
* 1119 then fails the step check: each stored leg is 0.2702 mm3 (two notch tools of 0.0041 mm3 each: 0.2784 mm3
  before them), Gusset's sweep 0.3153 mm3. The profile (1 x 0.45 mm in the plane x = 6 mm, at the path's end) lies
  in a plane that holds the path's first tangent: the sweep of a profile that is not across its path, along a
  chain of three lines with corners, which Fusion and OCCT's Frenet sweep orient differently. Not resolved.

### Not attempted
* 5981 `Chamfer6` (negative generators `-3`, `-6`, `-8` of a thin extrude: section 37, still too few cases to decode)
  and 6118 (`Fillet1 (2)`: how many copies it makes is not in the tree).

### Result
* adafruit-next: 124 of 150 as history (2315 gained; none lost against the base, where 3000 already fails its step
  check on `Extrude7`, as in round 26 W's run; round 26 X's own run had it). adafruit-more: 133 of 150 (unchanged,
  none lost). adafruit: 13 of 18 (unchanged, none lost).
* First blockers: 2315 passes; 3722 `Chamfer1` (the seam); 858 `Extrude13` (arcs drawn twice); 1119 the step
  check (the leg's sweep).

## 53. Round 28: blends OCCT fails beside a thread's end and on a seam (2026-11-13)
Kernel: `gusset_occt.cpp` (`revolved_blend`, `seam_moved_blend`, `merge_pieces`); both are the last fallbacks of
`blend_edges`, tried only where OCCT's blend at once, in passes, concave first and by corners has failed.

### 916 Fillet1: rounds of circles about one axis made on the revolved half section
* Root cause (section 50): the convex round of the r 7.942 edge rolls on the concave one beside it and meets the
  rod at z -16.0017, 1.7 um below the thread groove's square end (z -16); ChFi3d's first guess of the contact runs
  along the groove's boundary and the fillet fails, at once and in passes. Reproduced on the dumped shape with a
  standalone program (fails at once, and in the passes 0 1 2 and 0 2 1 at the third round).
* The blend of whole circles about one axis is made on a solid of revolution with the same circles: the shape's
  section in a half plane through the axis, in a window 3.1 blend sizes round the circles (`BRepAlgoAPI_Common` of
  the solid and the planar window, which must be one face), revolved. OCCT blends that (at once, or concave first).
  What the blend took away (the solid less the blended one) is cut from the shape and what it added joined to it.
  Kept only where the change stays inside the window, the shape's volume changes by exactly those two volumes
  (1e-7: so the shape is that solid of revolution wherever the blend touched it, the thread being 1.7 um away) and
  the result is valid; else the half plane is tried at five more angles. The blend faces are the blended solid's
  faces made from the circles, through both booleans.
* Golden test `rounds_at_a_threads_square_end_are_made_in_the_half_section` (`modeling_thread.rs`, 916's foot): the
  rounds and the thread do not meet, so the threaded, rounded rod's volume is the threaded rod's plus what the
  rounds change on the plain rod (OCCT rounds that directly), to 1e-7.
* 916 comes in as history.

### 3722 Chamfer1 and Chamfer1 (2): the seam moved away from the coils' ends
* Root cause: the coil's start edge (`Chamfer1`, the globe's base) ends on the parameter origin of the base's
  cylinder r 33.5 mm: the revolve's profile and the coil both start at +x. The coil, a strip one and a half turns
  long, cuts that cylinder into two faces (below and above the strip) which meet on the old seam line between the
  strip's turns; the start's vertex lies on that line, and ChFi3d does not end a blend there. `Chamfer1 (2)` (the
  cap's internal coil): its end edge at +x is 2.6e-5 mm (0.7 urad) beside the cap's seam and fails the same way
  (its start at -x builds alone). With the rod turned half a radian both build (section 47's golden test).
* The blend is made on a copy of the shape whose seams near the edges are moved: the faces of revolution at an
  edge's end that wrap round their axis and whose parameter origin passes within twice the blend size of the end
  are cut along the line at the angle farthest from the edges' ends and from the faces' corners
  (`BRepFeat_SplitShape` with the section of each face by a half plane through the axis: it splits the edges the
  line crosses in the neighbouring faces too), and the pieces of faces on one surface (same points at the same
  parameters) are made again into faces on the surface turned about its axis to that angle (`merge_pieces`): the
  edges two pieces share off the line are dropped (the old seam), the ones on it become the seam (their two curves
  on the plane by orientation: the material lies left of an edge), the others keep their curves shifted into one
  period; one face per region of the parameter plane (a strip that winds a whole turn round the face cuts it in two
  at the new seam as it did at the old). The copy must be valid and of the same volume (1e-5: the adaptive
  integration of 3722's coil body moves by some 1e-6 when its faces are cut anew); the blend's edges must be single
  edges of it. Faces of the result take the origins of the copy's faces they came from; a face ShapeFix's repair of
  the blend rebuilt beyond its context takes those of the face of the moved shape it lies on.
* Tried, not kept: `ShapeUpgrade_UnifySameDomain` with every edge but the old seam kept merged nothing; a
  `BRepAlgoAPI_Splitter` with the whole half plane split every face it crossed and the solid itself (volume changed).
* Golden test `a_chamfer_that_ends_on_a_seam_is_made_with_the_seam_moved` (`gusset-kernel-occt`, section 47's coil):
  each chamfer and both together equal the rod-turned-half-a-radian ones to 1e-7, every face has an origin and each
  edge's chamfer face is named from it. OCCT's own spread is that size: the start's chamfer with the rod turned by
  -1.5 to 0.5 rad ranges over 0.0043 mm3 (6e-8 of the body).

### Also
* Two tests of `modeling_tree_import.rs` were left open by a merge (the file did not compile with OCCT); closed.

### Result
* 916 comes in as history (adafruit-next +1). 3722 builds every feature now and stops at the step check: "a stored
  solid of 1462.4114 mm3 is neither a state of the model nor the cut tool of a feature". That solid is `Extrude2`'s
  tool (the ring r 31.5 to 35 mm, 2 mm: 1462.41 mm3, 816.8 of it in the wall and the coil's end); the body's measured
  change is 645.64 mm3. The coil body's adaptive volume (`measured_props`) is 73243.80 mm3 before the chamfers
  where the default quadrature gives 73243.40, and moves by 0.1 to 0.2 mm3 between equal shapes: more than the step
  check's 2e-5 of the change. Next for 3722: the integration of the coil's B-spline flanks.
* The 55 designs of the three sets that came in as history with fillets or chamfers (21 adafruit-next, 33
  adafruit-more, 1 adafruit) all still do; the new fallbacks run only where every earlier way has failed.

## 54. Round 29: the kernel's volume measure, face by face by Gauss-Kronrod (2026-11-13)
Kernel: `gusset_occt.cpp` (`measured_props`, `seam_moved_blend`).

### 3722: the coil body's volume moved between equal shapes
* Root cause: not the coil's B-spline flanks but the dome. Measured face by face (standalone, on the dumped body
  before `Chamfer1`), the flanks differ between OCCT's quadratures by at most 2e-4 mm3; the two faces of revolution
  of the shelled dome (the arcs of r 54 and 52 mm revolved, reaching the axis) carry 596765 and -533427 mm3 about
  the reference point, and OCCT's adaptive quadrature (`VolumeProperties` with a relative 1e-7 asked for) put them
  4.83 and -4.32 mm3 off (8e-6). Their sum, 0.4 mm3, is the 73243.80 against 73243.40 mm3 of section 53, and it
  changes with the reference point (the mean of the shape's vertices, so with every new vertex): the 0.1 to 0.2 mm3
  between equal shapes. The dome alone (`Revolve2` with its arc): Pappus 725020.81653 mm3, default quadrature
  725020.81653, adaptive 725028.20, Gauss-Kronrod 725020.81653.
* The default quadrature is no cure either: on 916's thread flanks it is 5e-5 mm3 off per face (1.6e-7 of the
  body), and on the rim of section 44 (`fillet_of_a_rim_between_two_flats...`) the flats that the torus trims
  (their edges B-splines) are 4e-4 mm3 off: integrated face by face about one point, the default quadrature on the
  planes and Gauss-Kronrod on the torus gave the two equal rims 1294.28479 and 1294.28517 mm3; Gauss-Kronrod on
  every face 1294.2846405 both.
* Also wrong before: a shape with a wound face (section 36) was integrated as two compounds, each about its own
  vertex mean; the volume of a set of faces that does not close depends on that point.

### The measure now
* A shape of planes, cylinders, cones and spheres alone: OCCT's default quadrature, unchanged.
* Otherwise face by face about one point (the mean of the shape's vertices): the default quadrature first, then
  every face that does not wind round its surface again by Gauss-Kronrod (`BRepGProp_VinertGK`, over the spans of
  the surface, a relative 1e-9). Wound faces keep the default quadrature (section 36).
* Gauss-Kronrod asks a relative error of every integral. Two cases never meet it and subdivide a thousand times
  (inside each piece the inner integral again): a face whose volume about the point is nothing but rounding (a
  plane through it: a helical sweep's end caps hold its axis; the generated M10 nut took 354 s instead of 1 s), and
  the moments for the centre of a face round the point (an extruded circle's side: 3 s instead of 1 ms). A face
  the default quadrature puts below 1e-9 of the sum of all faces' volumes keeps that value, and Gauss-Kronrod
  integrates the volume only; the centre is the default quadrature's.
* Result: the coil body 73243.40114 mm3 (default 73243.40154); the cap body before `Chamfer1 (2)`, its copy with
  the seam moved and that copy cut, three equal shapes, 7346.0875757 mm3 all (adaptive 7346.087810, 812, 808;
  default 7346.087482, 406, 409); the two rims 1294.2846405 both. `seam_moved_blend` now checks the moved copy to
  1e-7 (was 1e-5).
* Golden test `a_revolved_dome_measures_its_exact_volume_however_it_is_turned` (`gusset-kernel-occt`): 3722's
  dome section (a surface of revolution) revolved, its Pappus volume in closed form to 1e-9, turned by three angles
  and moved, the same.

### Time
* Kernel tests (`gusset-kernel-occt`, 69): 3.5 to 4.4 s. `modeling_gear`, `modeling_thread`, `modeling_sweep`, `modeling_tree_import` per test as before
  (gear 274 s against 292 s in all, thread 66 s against 66 s; sweep and tree import within a second per test).
* Corpus (31 designs: 3722, the helical and spline ones 3350, 1145, 5753, 2219, 6299, 5358, 3651, 4863, 916, 3221,
  5998, 6002, 3692, 3000, 3429, 1119 and a sample of the designs with blends), each run with the old and the new
  kernel in turn: 628 s against 624 s in all, no design slower by more than 7 s (5753: 40.5 to 47.5 s; 916 102 to
  92 s, 3722 9 s both).

### Result
* The 31 designs come in as before (24 as history, the others with the same reasons); none lost.
* 3722 passes the check that stopped it in section 53 (`Extrude2`'s tool of 1462.4114 mm3 is now explained: the
  coil body's change measures true) and stops at the next stored solid: "a stored solid of 46358.0566 mm3 is
  neither a state of the model nor the cut tool of a feature". The rebuilt globe base runs 725030.05 (`Revolve2`),
  725058.52 (`Fillet2`), 72499.74 (`Shell3`), 73243.40 (with the coil), 73888.99 (end) mm3; the file stores no
  solid of any of these, and its end bodies hold 86440.36 mm3 against the rebuild's 81241.19. The cap's stored
  states (6451.26, 10551.04, 11309.21 mm3) are the rebuild's, so if its end (7352.20) is too, the base differs
  from Fusion's by 5199.17 mm3 at the end. Next for 3722:
  which of the base's features (most likely `Shell3`'s cavity) Fusion makes otherwise, from the stored solid of
  46358.0566 mm3.

## 55. Round 29 D: next blockers: a draft's cap by its neighbours, blends on mirrored copies, a logo, coincident points (2026-11-13)
Reader: `fusion_tree/bodyops.rs` (`draft_json`), `fusion_tree/naming.rs` (`stored_face_beside`, `mirrored_face`,
`fillet_crossings`, `on_stored_round`), `fusion_tree/geom.rs` (`piece_at`, `bridged_chains`), `fusion_tree/sketch.rs`
(`Ellipse`), `fusion_tree/combine.rs`; importer: `tree_import.rs` (`shared_points`), `tree_import/v2.rs`
(`combine`, `remove_body`); kernel: `gusset_occt.cpp` (`offset_plane_by_prism`).

### 5998 Draft1: which of two caps of one name
* `Extrude5` joins two rails (one sketch, two rectangles 20 x 0.9 mm at y = +-8.25): both end caps carry the name
  `2 [Extrude5]`, and `Draft1` and `Draft2` each name "the" cap. The face is a bounded-face recipe (`7ACC2A03` with
  `bounded_face_recipe_data`) whose face list is the face itself, then the faces round it: `Draft1`'s cap
  `2 [328] | 3 [328] | 6 [328] | 4 [328] | 5 [305, 309]`, `Draft2`'s `2 [328] | 7 [328] | 10 [328] | 9 [328] | ...`.
  The drafts' own side faces (x = +-10, y = 8.25 and y = -8.25) and neutral planes (y = 8.7, y = -8.7) agree.
* Rule: of the stored faces carrying the cap's name, keep those that share an edge point (1e-7 cm) with a stored face
  of every name round it. Nothing is guessed: where that does not narrow the faces down, the pick is as before.
* 5998 then builds `Draft1` and `Draft2` and stops at `Chamfer3` (OCCT: "chamfer failed"): the 0.6 mm chamfer of the
  rim of `Extrude6`'s slot, cut 0.6 mm deep into the 1.6 mm floor (z 1.0 to 1.6), uses up the slot's wall. ChFi3d does
  not end a chamfer on the face's far edge; Fusion does. Not attempted (a kernel fallback: the cut of the slot's outline
  tapered at 45 degrees from the floor to the rim).

### 6124 and 6299: blend edges on mirrored copies and across a round
* 6124 `Fillet4` rounds the bottom rim of the case: the box (`Extrude2`, corners rounded by `Fillet1`) joined with a tab
  (`Extrude9`) and its mirror copy (`Mirror2`), their joins rounded by `Fillet3`. Four edges were unresolved:
  - **Mirrored faces.** A face named `(g, [op, mirror, copy])` (the mirror `D1728651`, its copy `AD719C1A`) was read
    as the face `(g, [op])`, not reflected: `6 [374, 376, 377]` gave the tab's own side at y > 0. It is now the
    original's face reflected in the mirror's plane (planes and fillet rounds; a face pick likewise); `Fillet3`'s
    rounds of the copy follow.
  - **A line that runs into a round.** The tab's side meets the box at `Fillet1`'s corner round, which rounds two
    other planes. The edge ends where its line enters that round: the crossing (in the quarter between the round's
    tangent lines) nearest the edge's other ends.
* 6299 `Fillet4 (2)`: the side `11` of the thin cut `Extrude7 (2)` is known only from the stored bodies, and the plane
  faces that carry it among other entries disagree. A face is now also read as the plane every stored face carrying
  exactly its name lies on. 6299 then stops at `Loft2` ("loop does not close": a loop of a circle's two halves and a
  whole line across it; not looked into).

### 4632 Fillet7: the round of an edge of a cut
* `Fillet6` rounds three vertical corners of a notch `Extrude7` cut into the box's wall; `Fillet7` runs along the top
  rim, over those rounds. A fillet face was always read as the round of a convex edge, each side's normal into the
  material as the extrude's profile gives it, which is the wrong side for a cut's face (it faces into the cut). The
  rim's arcs came out 0.117 mm off the model's. Where the round as read lies on no stored round of the fillet (a
  cylinder of its radius, 1e-7 cm, about its axis, 1e-6 cm), the one way of turning the two sides that does is taken
  (the stored round `1 [480]` about x = 1.2, z = 17.3 mm: the cut's side turned).
* 4632 now builds every feature and is refused by the step check ("a stored solid of 24.9678 mm3 is neither a state of
  the model nor the cut tool of a feature"), as section 49 expected: the file's fan is the plain hub, the blades never
  joined it in Fusion; the rebuild joins them. Not loosened.

### 6124's logo: a cut at a spline's start, a hole on a bridge, ellipses, Combine 1
* `Extrude16` (the Adafruit logo, `Sketch4 (3)`: five closed petal splines and a pentagon of lines) was "outer loops 3".
  - A line crosses the petal `c5942` 3e-8 cm from its first point (both rounded): the cut came out as a cut just after
    the start, numbering the closed curve's pieces one off (pieces 3 and 4 of 6 were the wrong two). A cut within
    1e-6 of a closed curve's start is now at its start.
  - The loop then walks the petals' outline, a piece of the line x = 0 from the outline to a corner of the pentagon
    (once, not there and back) and the pentagon. Such a loop now falls into the outline's ring and the hole's: the
    one piece without which the others close in rings, each hole inside the outline.
* `Extrude17` cuts five ellipses (`FF23079A`) 10 mm into the logo. The record: after the curve ids, 13 doubles (cm):
  the centre, the major axis's unit direction, the minor axis's (in the sketch plane), the two semi-axes, the parameter
  range (0 to 2 pi in all five), then the centre point's reference. Whole ellipses are read (tree `ellipse`; the
  regions take the curve from the major axis's end round); arcs of ellipses are not decoded.
* `Combine1`'s operation is 1, in all five such records a logo body taken out of a case: the API's numbering (join 0,
  cut 1), now read as a cut. Fusion keeps the tool and removes it with `RemoveBody-Body51`; the document's combine has
  used it up, so a later removal of a combine's tool is a no-op.
* 6124 (both copies, adafruit and adafruit-more) now reads and builds in full and is refused by the step check:
  "Extrude17 changes the volume by 3428.0243 mm3, not 5 copies of a stored cut tool" (five ellipses of 68.56 mm2,
  10 mm deep, after the last stored state; the file stores no tool of that cut). Not loosened.

### 858: points on one spot, a shaft's end pushed out
* `Extrude13`'s sketch has two points in one place (the arc `c4225` starts on `p4220`, the line `c4003` on `p3998`,
  both at (-7.3, 11.946129) mm); the file has no constraints between them. The arc's ends disagree on its radius by
  4e-7 mm (7 digits), the document's solver moved `p4220` by 1.7e-7 mm to make them agree and left `p3998`, and the
  profile's arc ended off its circle: OCCT's `BRepBuilderAPI_MakeEdge` failed ("could not build an arc of the
  profile"). In a sketch without constraints (the Fusion trees), a curve end exactly where an earlier point lies is now
  written as that point, unless a curve would use one point twice; poles and construction points stay apart.
* `OffsetFaces2` (+0.4 mm) pushes the shaft's lower end (`Extrude4`, a rod r 2.5 mm from z 1.6 to 4, joined by
  `Extrude5` to the same rod with two flats up to z 10, its circle drawn twice so its right arc is in two at the seam)
  out. The kernel's move of the face refuses (a neighbour would change shape) and BRepOffset gives no shape. New last
  fallback of `gs_offset_faces` (`offset_plane_by_prism`): one planar face whose neighbours all stand along its
  outward normal (planes across it, cylinders along it) is joined with, or cut by, the prism it sweeps; the volume
  must change by its area times the distance (1e-7); the prism's far cap stands for the face, a piece of its side the
  unification leaves apart from the neighbour it continues takes that neighbour's origin. Golden test
  `a_shafts_end_between_flats_is_pushed_out_by_its_prism` (fails without the fallback).
* 858 comes in as history (13 stored solids states of the model, 7 its cut tools).

### Not attempted
* 2354 `Revolve1` and `Revolve2`: the profile is a face (a bounded-face recipe `1 [318]`, a box primitive's face) and
  the axis an edge (`5 [310] | 1 [310]`); the document's revolve takes a sketch profile and a sketch line or base axis
  only.

### Result
* Gained: 858 (adafruit-more) comes in as history. 6124 (two copies) and 4632 now build every feature and are refused
  by the state checks (above); 5998 stops at `Chamfer3` (OCCT), 6299 at `Loft2` (reader), 2354 at `Revolve1`.
* No loss on a sample of 35 designs that came in as history (every 6th of adafruit-more, every 10th of
  adafruit-next) and on the designs with offsets of faces (3027, 3464, 3350, 1054, 3692).

## 56. Round 30 A: a face revolved about an edge, which way a part turn goes, a loop curve that meets nothing (2026-11-13)
Readers: `fusion_tree/json.rs` (`revolve_of_a_face`, `revolve_angle`), `fusion_tree/naming.rs` (`revolve_turn`),
`fusion_tree/geom.rs` (`touching`), `fusion_tree/sketch.rs`; importer: `tree_import.rs` (`face_revolve`, `edge_line`).
Document: `revolve` takes `profile: <face>` and `axis: edge(...)`, a negative `angle` turns the other way
(grammar.md). Tree format: a revolve's `profile_face`, its `axis.edge` (an edge pick, as a sweep's `path_edges`) and,
for a part turn, `axis.direction`.

### 2354 Revolve1, Revolve2: a box's face about an edge
* The revolve record (`E3849A15`) names no `4BD53E5A` profile: its selections (`2CA5A1CD` -> `5662F619`) are a
  bounded-face recipe (`1 [318]`, a face of `BoxPrimitive3`) and an edge recipe (`5 [310] | 1 [310]`). The face is
  read as an extrude's face profile (`face_pick`), the edge as a sweep's path edges (`sweep_path_edges`, its middle).
* Both are part turns (`AlongAngle` 4.18879 = 240 degrees, and 180). Which way a revolve turns is not in the record;
  the stored faces tell. A revolve's faces carry its operation alone: the start cap `1 [op]`, the end cap `2 [op]`,
  the swept faces (`11`, `12`, `13`). The swept faces are surfaces of revolution about the axis: a cylinder or cone
  about it, a plane across it, which gives the axis line (the edge's middle must lie on it, 1e-6 cm). From the start
  cap's side of the axis (the mean of its points' offsets from the axis; else the end cap's turned back), a
  right-handed turn by the angle about the right direction reaches every point of the faces (their vertices and
  points along their edges), and about the other direction does not. `Revolve1`: start cap at x = 0 below the axis
  (y, through z 5 mm), end cap at (0.866, 0, 0.5) from it: +y, written `angle: 240 deg` about the edge.
* The importer finds the edge by its middle and writes `axis: edge(...)`; the document's edge axis points towards +X,
  else +Y, else +Z, so a direction against that is written as a negative angle. Golden test
  `a_face_revolved_about_an_edge_either_way`.
* `Revolve2` turns a face that its axis halves (the cap `2 [374]` reaches 0.0388 cm to both sides of it): a half turn
  either way makes the same whole cylinder, the cap is on both sides, and the turn is not read ("a part revolve whose
  stored faces do not tell which way it turns"). The document's revolve of a face across its axis would not build
  either (OCCT's revolve of a profile crossing the axis is self-intersecting); a full turn of one half is the shape.

### 6299 Loft2: a loop that lists a curve meeting none of the others
* `Sketch3 (2)` holds a slot (rectangle with a half-circle end `c5958`, r 1.4 mm about (1.4, 7)) and the circle
  `c6812` (r 1.25 mm, same centre), cut in two halves by the slot's side through the centre. `Loft2`'s first section's
  loop lists both halves (1/2, 2/2) and the whole end arc, whose ends lie 0.15 mm outside the circle: it meets neither
  half. The stored loft's start cap `1 [389]` is one whole circle (a single vertex at r 1.25 mm): the section is the
  circle.
* Rule: where the loop's curves do not chain (nor as a bridged hole), a curve neither of whose ends comes within a
  thousandth of the loop's size of another's end is left out, if the others then close. Placed after the bridged-hole
  readings: placed before them it took 2831's loop of an outline and a hole on a bridge apart (lost in a first run).
* 6299 then reads `Loft2` (two circles) and stops at `Chamfer2` (14 of 16 edges not resolved).

### Result
* No design gained history: 2354 now reads `Revolve1` and stops at `BoxPrimitive6` (a box joined to three bodies, two
  of which `Extrude5` then cuts as separate bodies), before `Revolve2`; 6299 reads `Loft2` and stops at `Chamfer2`.
  Neither revolve nor the loft is rebuilt against the stored bodies yet (the readers stop first).
* No loss on the sample of 40 designs (every 6th of adafruit-more, every 10th of adafruit-next): 35 come in as history,
  as in round 29; the other five are 1400 and 2226 (no history), 3221 and 3502 (state check), 6299.

## 57. Round 30 B: a spindle torus's point on the axis, a chamfer that uses up its sides, rounds a hair off square (2026-11-13)
Converter: `gusset-sat` `step.rs` (`Writer::apex_seam`, `write_seam`), `fixtures.rs` (`spindle_dome_sat`); kernel:
`gusset_occt.cpp` (`chamfer_by_taper`, `corner_of`).

### 3722: the stored solid of 46358.0566 mm3 is `Revolve2`'s dome, read wrong
* Every stored body converted to STEP and read on its own (`STEPControl_Reader`): the solid of 46358.0566 mm3 is the
  cylinder r 33.5 mm, 19 mm tall, and one face on the spindle torus of section 52 (major 0.6583 mm, minor 54 mm about
  z 61.865 mm) bounded by the cylinder's top circle and a vertex loop at the dome's top (0, 0, 115.861) mm: `Revolve2`
  as the file stores it. OCCT's STEP reader drops a vertex loop on a toroidal surface (`StepToTopoDS_TranslateFace`
  goes on to the next bound), and ShapeFix then closes the face at the other point where the tube crosses the axis
  (z 7.869 mm): the bowl under the dome, and 46358.06 is the cylinder less that bowl. The file's end body of the base
  came in the same way (1909.57 mm3).
* So not `Shell3`. With the face read right the stored end body of the base and the rebuild's have the same dome
  faces (outer r 54 mm 33218.588 mm2, inner r 52 mm 30823.295 mm2), rounds (r 2 mm 381.499, r 4 mm 723.646) and inner
  wall (r 31.5 mm 3567.30, in three pieces in the rebuild).
* Fix: a torus face whose minor radius exceeds its major one, with one loop of edges and one loop that is a point on
  the axis, is written as OCCT writes such faces: a seam from the edge loop's start to the point and back in the edge
  loop, on the arc of the tube's circle through that start that does not pass the other crossing (counterclockwise
  from x towards the axis unless the other crossing lies on the way); OCCT's reader adds the degenerate edge at the
  point. The decision is made from the records before anything is written, so every other face is written as before:
  every `.smb`/`.smbh` blob of the three sets converts byte for byte as before except 3722's two (the dome and the
  base's end body).
* Golden test `a_sat_dome_on_a_spindle_torus_imports_with_its_volume` (`gusset-kernel-occt`, 3722's section from
  `fixtures::spindle_dome_sat`): 725030.0311 mm3, Pappus in closed form, to 1e-9 (the bowl without the fix); unit test
  `a_spindle_torus_face_ends_at_the_axis_by_a_seam` (`gusset-sat`).
* 3722 now: every stored solid is a state of the model or a cut tool (`Revolve2` stored 725030.0527, rebuilt
  725030.0492); the check stops at "how many copies Chamfer1 (2) makes cannot be derived from the tree, and it follows
  the last stored state". The end bodies that could confirm the last features are not read: the base's history blob
  (`c11e060a....smbh`) holds an empty body (no faces), and `fusion_end_bodies` gives up on any body but a sheet. Read,
  they would not agree yet: the blobs also hold the combines' tool bodies (the coil, 743.66 mm3, and the cap's
  `Extrude1` state, 10551.04 mm3), and the base's end is 73895.0589 mm3 (Gauss-Kronrod 73895.0597) against the
  rebuild's 73888.9897.
* The 6.07 mm3 is `Chamfer1`. The stored end's coil flanks are 755.55 mm2 each (rebuild 750.99 and 748.69), its two
  ends planes of 3.3475 mm2 (normals (+-0.697, -0.718, 0); at the start z 2.803 to 5.115 mm, below the coil's 2.845:
  the flanks run on past the old end), the base's cylinder 2634.48 mm2 round the coil (rebuild 2641.33). Fusion
  chamfers the concave edge between each coil end and the base's cylinder (a wedge added at both ends). The rebuild
  does so at the end (`line_on_cylinder`) but at the start chamfers the convex edge between the end face and the
  upper flank, (33.5, 0, 5.155) to (35.5, 0, 4) mm (r28's dump): the reader's `stored_edge` finds, in the stored
  bodies, an edge between faces named `5 [307]` and `1 [312, 314]` with its middle at (34.5, 0, 4.577) mm. The recipe
  `5 [307] | 1 [312, 314] | 4 [312, 314] | 3 [312, 314]` names the edge between `Revolve2`'s face 5 and the coil's
  start cap, ended by the flanks 4 and 3: the base of the start's triangle, middle (33.5, 0, 4.0) mm. Next for 3722
  (reader: which stored edge carries those names).

### 5998 Chamfer3: a chamfer that uses up a slot's sides
* The dumped shape: `Extrude6`'s slot is 3 mm wide (sides x = 8 and 11 mm from y = -4 to 4, round ends r 1.5 mm),
  pocketed from the floor's top z 1.6 down to 1.0 mm; `Chamfer3` takes its four rim edges by 0.6 mm. ChFi3d fails
  ("chamfer failed") at once and in every earlier fallback; the chamfer's faces would end on the slot's bottom edges.
* New last fallback of `blend_edges` (`chamfer_by_taper`): a chamfer of one size along one whole loop of a planar
  face's edges, all convex, lines or arcs, whose other faces stand square to the face (planes across it, cylinders
  along its normal). The tool is the ruled loft (`BRepOffsetAPI_ThruSections`) from the loop moved the chamfer's size
  down the sides to its offset by twice the size (`BRepOffsetAPI_MakeOffset`, intersection joins: a hole's loop grows,
  the outer loop shrinks) the size above the face; the shape less the tool. The volume removed must be exactly the
  ring between the loop and its offset by t over the depth, d/6 (A0 + 4 A(d/2) + A(d)) - A0 d (exact: the offset's
  area is quadratic in t), to 1e-7 of the body; every face of the result must have an origin; each loft side the
  boolean leaves is its edge's chamfer face (planes, cones on the arcs). 5998: 4.802654704 mm3 removed, as expected.
* Golden test `a_chamfer_that_uses_up_a_pockets_sides_is_its_rim_tapered` (`gusset-kernel-occt`, 5998's slot in a
  plate): P d^2/2 + pi d^3/3 to 1e-9, one plane or cone per rim edge; it fails without the fallback.

### 5998 Fillet5: rounds of a slot's end a hair off square
* Then `Fillet5` failed ("fillet produced an invalid solid"): r 0.2 mm on both inner corners at the end of two slots
  0.4 mm wide, turned 20 degrees (a full round). OCCT at once gives an invalid solid (713.96 mm3 for 848.56), in passes
  the second round of an end fails; `blend_by_corners` refused: the end and the sides are 9.96e-8 rad off square
  (Fusion's angles to 7 digits), and `corner_of` wanted 1e-9. Now 1e-6 rad; the corners made square are off by some
  1e-7 of a blend size, far below its volume check (1e-7 of the body). Golden test
  `rounds_of_a_slots_end_a_hair_off_square_fill_its_corners` (fails at 1e-9).
* 5998 then stops at `OffsetFaces2` (component Top): "no planar or cylindrical face of the model holds the point
  [-10.756497, 6.917761, 9.0]" (a face pick of the reader; not looked into).

### Result
* 3722 passes the stored-solid check (next: `Chamfer1`'s start edge, above); 5998 builds `Chamfer3` and `Fillet5` and
  stops at `OffsetFaces2`. No design gained history yet.
* Converter: every `.smb`/`.smbh` blob of the three sets converts to the same STEP as before but 3722's two (25 files
  Python's zipfile cannot open were not compared).
* No loss: round 29's 31 designs (helical, spline and blend designs) and 52 more that came in as history (every 5th of
  the three sets' history designs, and 1867) come in as before; 1867 still stops at `Fillet1` ("too large for edge").
* Tests: `gusset-kernel-occt` (73), `gusset-sat`, `modeling_gear`, `modeling_thread`, `modeling_tree_import`,
  `modeling_sim_shell`, `modeling_chamfer`, `modeling_full_round` pass.

## 58. Round 31 A: chamfer edges: a state after the blend, a line's ends, the rim at the far cap (2026-11-13)
Reader: `fusion_tree/acis.rs` (`NamedEdge::body_ops`), `fusion_tree/naming.rs` (`stored_edge`, `line_on_cylinder`,
`across_the_extrude`), `fusion_tree/feature.rs` (`blend_sets`); importer: `import.rs` (`fusion_end_bodies`).

### 3722 Chamfer1: the start of the coil
* The edge section 57 found, between `5 [307]` and `1 [312, 314]` with its middle at (34.5, 0, 4.577) mm, is in the
  base's history blob (`c11e060a....smbh`): the end body, after `Chamfer1` (its faces `1 [315]`, `2 [315]` are there).
  Fusion's chamfer of the start leaves a sliver of the coil's start cap `1 [312, 314]` (x 35.49 to 35.5 mm about
  z 4.0) whose slanted edge to (33.5, 0, 5.155) meets the cylinder's face. No other stored body holds a face `5 [307]`
  beside the coil's caps.
* Rule: each stored edge carries the operations named on its body's faces; while a blend's edges are read, an edge of a
  body that holds a face of that blend's operations is not taken (a state after the blend). The recipe then falls
  through to `line_on_cylinder`.
* `line_on_cylinder` took the middle of the plane face's stored points along the line: the sliver's points give
  z 4.162 mm, 0.162 mm off the model's edge (a B-spline edge, the cap of the fitted coil, so the line test of
  `containing_edges` does not hold it). The edge is now the piece of the line a stored face of the plane's name ends on
  (two of its vertices on the line, the stored points' middle between them), or of the name it had as a combine's tool
  before the combine (`2A94257F`) trimmed it: the coil's own cap `1 [312]` (`71d8f5f5....smb`, the coil alone), a
  triangle with its base on the line from z 2.845 to 5.155 mm. The piece is given whole (`path`); the end's edge
  (-33.5, 0, 10.5) mm comes out the same, now with its whole piece.
* `Chamfer1` builds: the base's end volume was 73888.99 mm3, now 73895.1078 (the stored end body 73895.0597, 6.5e-7).

### 3722's end bodies
* The base's history blob holds a body without faces beside its end bodies; `fusion_end_bodies` gave up on all of
  them for it. Such a body is now passed over, as a sheet body is.
* The check then reads the end bodies and refuses on them: "the finished bodies (73895.1078, 7352.1969 mm3) are not the
  end bodies the file stores (73895.0597, 743.6554, 10551.0389, 7352.2430 mm3)". The base and the cap agree (6.5e-7 and
  6.3e-6, within 2e-5). The other two are the two Combines' tools as they were combined: the coil `CoilPrimitive1`
  after `Move1` (743.66) and the cap's `Extrude1` after `Extrude2 (2)` (10551.04). Fusion kept them (Keep Tools); the
  document's `combine` uses its tools up. Where the record says so is not found: both Combines (690, 1980) have no
  RemoveBody reference (`u32 n` = 0) and list the tool's own body link, as 3900's header part's Combines do, whose
  tools section 22 reads as used up; 690 has one more `u32 1` before the selections than 1980, both keep their tools.
  The body links (`D26351F0`), the selections, `B7F34D7B`/`5FBE538E`/`6CCF41D5` and the component's body list
  (`C3423E04`, `A2EDAFD4`) do not tell 3722's tools from 3900's either. Not guessed; next for 3722 (which flag is Keep
  Tools; with it, the importer keeps a copy of each tool, as `thicken` keeps its sheet). "How many copies Chamfer1 (2)
  makes" is no longer the reason: the end bodies are read and confirm or refuse the features after the last state.

### 6299 Chamfer2: the top rim of slots cut through a plate
* `Extrude10 (2)` (op 377, and 377, 378) cuts two slots through the plate `Extrude8` (op 353, z 13 to 14.6 mm);
  `Chamfer2` (op 393) takes the top rims: edges between the top cap `2 [353]` and the slots' sides `10`, `11`, `12`
  (the slot's arc end). The stored bodies hold the top rims only chamfered (faces `9` to `16 [393]` from z 14.2 to
  14.6) and the bottom rims at z 13 under the same side names with the same faces at their ends.
* Rule (`across_the_extrude`): an edge between a cap `1`/`2` of an extrude (one operation) and a face whose stored
  surfaces all run straight across it (planes along the extrude's direction, cylinders about it) is the stored edge
  between that face and the other cap, ends alike (any state; the blend does not take that edge), moved along the
  direction onto this cap (both caps' stored planes parallel). A side that does not reach this cap leaves no model edge
  there and the import stops.
* `Chamfer2` 14 unresolved edges to 6: the 8 resolved include the rims at the vertical rounds `1 [392]`, `2 [392]`
  (cylinders r 0.3 mm). The 6 left are the top rims of `Loft2`'s faces `3`, `6`, `8 [387]` (each also named with 388):
  no stored body holds a face of those names. 6252's `Chamfer2 (2)` likewise 8 to 6 (6252 stops earlier, at an extrude).

### Other chamfer blockers
* 5981 `Chamfer6` (component Top Cover, op 410 its faces): 8 recipes of the top rim of `Extrude` op 363 name its sides
  `-3`, `-6`, `-8 [363]` (and `[363, 364]`); the stored bodies name that extrude's sides `10`, `11`, `12 [363]` and hold
  no face of a negative generator. Which side a negative generator is, is not read; not attempted.
* Crickit CPX Assembly (adafruit): its chamfer reads in full now (earlier rounds); it stops at `Combine1` ("the
  Combine's target is not the one body of a feature of the tree").
* 471 (adafruit-next) `Chamfer`: a chamfer of two distances whose face is not planar; another reader, not attempted.

### Result
* No design gained history. 3722 builds every feature and is refused by the end bodies (the kept tools, above); 6299
  reads two thirds of `Chamfer2`.
* No loss: the feature trees of all designs of the three sets (27 + 156 + 155) dumped before and after: only 3722,
  6299 and 6252 change, in the blends above. The 40-design sample (every 6th history design of adafruit-more, every
  10th of adafruit-next, every 4th of adafruit) all come in as history, as before.
* Tests: `gusset-sat` (134, new `a_stored_edge_knows_the_operations_its_body_has_been_through`), `gusset-commands`,
  `modeling_tree_import`, `modeling_chamfer` pass; clippy and fmt clean.

## 59. Round 31 B: a clip's offset, a join that leaves a body apart, a thin wall about its curves, a bend's sketch where it was drawn (2026-11-13)
Kernel: `gusset_occt.cpp` (`gs_offset_faces`, `offset_planes_by_sweeps`, `swept_by_planes`). Readers:
`fusion_tree/naming.rs` (`truth`, `half_turn_across`), `fusion_tree/json.rs` (`joins_of_several_bodies`,
`sketches_where_they_were_drawn_against`), `fusion_tree/thin.rs` (`chains`, `with_other_walls`),
`fusion_tree/joint.rs` (`positions`).

### 5998 OffsetFaces2: a side the first offset moved, and a body BRepOffset lost
* The point [-10.756497, 6.917761, 9.0] is the middle of `c6026`, a side of `Extrude8 (2)` (Top's lid clips: one sketch,
  two profiles mirrored in y, 2 mm deep from z 10 down). The face was in the model; the body it was looked for in had
  two faces. `OffsetFaces1` (-0.15 mm, 14 faces of the clips and of three V-grooves along the lid's sides) went to
  BRepOffset, which gave back a valid shell of two faces (`Loft1`'s oblique side and its end cap): every other face
  of the body had no image. The same happens with a plate and a clip reaching past its edge, no loft at all.
* Kernel: a BRepOffset result in which an input face has no image is refused, and a new fallback moves planar faces
  whose edges are lines between planes one after the other: each face's swept solid, the ruled loft from its outline
  to the outline where its moved plane meets its neighbours' planes (between two edges of one line, square to the
  line), is cut from the body (inward) or joined to it (outward). A face moved later sees the faces moved before it,
  so the two faces of a groove meet on their moved planes. Each step must change the volume by exactly its swept
  solid's (1e-7 of the body): the solid lies wholly inside (cut) or outside (joined). The moved face is the solid's
  far cap; a piece of its side the unification leaves takes the origin of the neighbour whose plane it is on. 5998's
  14 steps agree to 3e-8 mm3 each. Golden test `a_clips_slanted_side_moves_between_its_neighbours_planes` (fails
  without the fallback: BRepOffset gives the plate without its clip).
* Reader: `OffsetFaces1`'s side `10 [394]` was read as `c6032`, a side `OffsetFaces2` moves back. The stored bodies
  hold face `10` twice: over the clip's whole straight side (x = -12.9 mm, y 3.774..7.037, `c5903`'s piece and
  `c6017`) and over the piece the join leaves exposed (y 6.2..7.037, `c6017`). Both counted as candidates, so the
  generator map decided, wrongly. Candidates over the same pieces of a line now count once, the one over fewer
  pieces kept: `10` is `c6017` (and `3`, its mirror, `c5909`, now by its own middle). By the mirror symmetry of the
  two offsets (1: `3`, `8` / `10`, `11`; 2: `4`, `5`, `6` / `13`, `14`, `9`) the profile's sides are numbered round it
  from `c5909` (`c6032` for the mirrored one).
* 5998 then builds both offsets and stops at `OffsetFaces1 (2)` (Bottom, -0.4 mm on the walls of the text's glyph
  pockets, moved alone): "the moved faces would cut through the rest of the part" (not looked into).

### 2354 BoxPrimitive6: a join that leaves one of its bodies apart
* `BoxPrimitive6` (x -2.5..0.917, y 33..60, z 5.5..7 mm) joins to `BoxPrimitive1 (2)` (the flat ribbon, z 5..5.5),
  `BoxPrimitive3` (a strip x 0..0.5, z 4..5) and `Revolve1` (the bend about x = 0, z = 5). `Extrude5` then cuts
  "`BoxPrimitive3` and `Revolve1`" from the box's top down 3.5 mm, and every later feature names `Revolve1`, among
  them `Fillet2`, whose rounds at x = 60 mm are the flat ribbon's far end: the flat part is in `Revolve1`'s body. The
  box touches the flat part (its face at z = 5.5) and the bent part, not the strip.
* Rule (with the round-26 one for a join of several bodies): where later features name several of the join's
  bodies, the tool joined the one every later feature naming any of them names (`Revolve1`); the others they name
  stayed bodies of their own (`BoxPrimitive3`, still cut by `Extrude5` as a body of its own); those no later feature
  names became part of it (`BoxPrimitive1 (2)`, a `combine`). Where no body is named by every such feature the
  join stays refused (3802's `Extrude4`, whose later features name `Extrude1` and `Extrude2` each alone, as
  before). A first reading (every body under one name, later references renamed) was replaced by this one before
  the run below: it would have merged the strip, and 3802's bodies.
* 2354 then stops at `Revolve2` (180 degrees about an edge through the middle of a box's face: the stored end cap
  `2 [374]` reaches 0.388 mm on both sides of the axis, the join used up the start cap, and the turn's only other
  faces are a cylinder of that radius and its two end planes). A half turn of a face the axis crosses is now read
  either way where both sides of the face reach the same radii (the same solid), else by the side of the stored
  points beyond the shorter side's reach (the start cap, or the end cap where the start is gone). Unit test
  `a_half_turn_of_a_face_the_axis_crosses_turns_where_its_sides_differ`. 2354 then stops at `Fillet1` (four edges
  unresolved, radius 6.65 mm on `Revolve1`; not looked into).

### 6126 Extrude5 (2): wall location 1 is the middle
* The thin cut (0.6 mm deep from z 15.6, wall 1 mm, location 1) follows three separate lines of `Sketch4 (2)`: a "-"
  (x = 4, y 1.5..5.3 mm) and a "+" (x = 4, y -5.3..-1.5 and y = -3.4, x 2.1..5.9). Its stored faces (op 364 of the
  body's names) are the slots' sides at x 3.5 and 4.5, x 3.5 and 4.5, y -3.9 and -2.9 mm: 0.5 mm either side of each
  line. Location 1 is a wall about the middle (location 2 one side, sections 33 and 37).
* The faces are named per selected curve k: `12 + 7k` both sides (the left one also carries the second operation, as
  in section 33), `+3` the cap on the sketch plane, `+5` the floor, `+4` and `+6` the ends at the line's start and
  end. Not yet used: a wall along several chains keeps no cap or side names.
* Reading: a wall about the middle may follow several chains (curves split where their ends meet): one wall sketch
  per chain and one extrude per chain (copies of the feature, `part 2`, `part 3`). 6126 then reads `Extrude5 (2)` and
  stops at `Fillet2 (2)` (12 edges of the slots' rims unresolved: they name the walls' faces, after `OffsetFaces1`).

### 3502 Bend1: the root sketch where Component7 stood when it was bent
* The timeline: `Position2`, `Sketch1`, `Bend1`, `Sketch2`, `Bend2`, `Position3`, ... A Position (`8EE00B00`) captures
  occurrence positions; each occurrence path record (`549FBB80`) lists, per Position (and component activation), the
  path's world placement then (the steps `joint.rs` already reads; cm). Component7's path (the LCD assembly's
  occurrence, then Component7's) stands at Position2 at x' = 24.703 - x, y' = 34.5 - z, z' = -0.865 - y (mm): its flat
  flex's top face (y = 0.05) lies on `Sketch1`'s plane z = -0.915 and its outline on the sketch's (x 15.233..24.703,
  y 34.5..49.13). Position3 turned the assembly over: at the end it stands at x' = x - 24.703, z' = y + 0.61, where
  the tree placed the bends' sketches against it (1.525 mm off the plane, mirrored in x: section 51).
* Rule: a feature of a placed component that uses a sketch of an unplaced one (the root) gets a copy of the sketch
  moved by the component's final placement after the inverse of its placement at the last Position before the
  feature (`<sketch> where <feature> used it`). Applied to bends; Bend1's copy lies on z = 0.66 mm, the flex's top
  face where it ends, and its line at the flex's z = -4.9 mm (section 51's zone end). Unit test
  `a_root_sketch_a_component_feature_used_moves_with_the_component_after_it`.
* 3502 still stops at the importer's guard on a Base Feature's input body (left as it is: the owner's history choice
  covers trying anyway); the bends are not rebuilt by the corpus runner.

### Result
* No design gained history. First blockers now: 5998 `OffsetFaces1 (2)` (was `OffsetFaces2`); 2354 `Fillet1` (was
  `BoxPrimitive6`); 6126 `Fillet2 (2)` (was its thin extrude); 3502 the importer's guard on a Base Feature's input
  body, as before (its bends' sketches now placed where they were drawn).
* No loss: the 40 designs of round 30's sample (every 6th of adafruit-more, every 10th of adafruit-next) give the
  same 35 as history; of the touched designs 3027, 3350, 3464, 610, 858, 1054 and 3692 (offsets of faces) come in
  as history as before, 3722, 5981, 6299 and 3802 stop where they did.
* Tests: `gusset-kernel-occt` (74, the new golden test), `gusset-sat` (136); clippy (`gusset-sat`,
  `gusset-occt-sys`, `gusset-kernel-occt`) and fmt clean.
## 60. Round 32 K: Combine "Keep Tools": no Combine of the samples keeps its tools (2026-11-13)
Reader: `fusion_tree/combine.rs` (`used_up`), `parse.rs` (`Sat::tags`), `lib.rs` (`body_tags`); importer:
`import.rs` (`fusion_end_bodies`, `fusion_used_up`).

### The question
Section 58 read 3722's two extra stored bodies (the coil after `Move1`, 743.66 mm3; the cap's `Extrude1`, 10551.04)
as Combine tools Fusion kept, and 3900's header part as a design whose tools are used up, with the same record layout.
Every Combine `2A94257F` of the three sets (35 records in 18 distinct design streams; the copies in other sets are
the same streams) was compared field by field, then with the records around it.

### The record: no field tells
* Body, now read in full: `u32 operation`, `u32 n` + n `4A782808` (RemoveBody) references, `u8 a`, `u8 0`, `u32 1`
  + one `2E3AC990` reference or a null (`00`), `u64` (a counter: 0x13a, 0x156, 0x227 ... rising through a file),
  `u32 ntools` + the tool selections, `u32 0` (two in the layouts with a `2E3AC990`), `u32 m` + the tools' body links,
  `u32 1` + the target selection, a null UUID string, the (selection, `9716F783`) pairs, `u32`, `Combine`, the name's
  index, zeros, `B7F34D7B`, a double, `5FBE538E`, `6CCF41D5`, then the `261`/`258`/`265` section.
* The "one more `u32 1`" of 3722's 690 against 1980 (section 58) is the record header's attribute flag with an empty
  attribute list (`01` + `u32 0`, 4 bytes longer); 3093's Combine1 and the Crickit Assembly's have it too.
* `a` is 1 only in 324 NEMA-17 (whose Combine stops behind a Thread); 0 in the other 34. The padding of references
  (1 or 2 zero bytes) follows the file's version, not the Combine. Apart from the ids, the counter and the tool counts,
  3722's 690 and 1980 are byte for byte 3900's 8368, 8543, 4200's, 4242's, 4400's, 4300's, 4128's, 3651's Combines.

### What the file does say: the component's body list
* A body record `D3937028` names its component's body list `CD57BC48`: `u32` (the next body index), `u32 n` + n
  body records (the component's current bodies), a `E55D0457`, the blob's body map `74CA4562`, a `372234A0`, the same
  n bodies again and the component `E03784ED`. The map is `u32 n` + n pairs (`u64` tag, `u64` body record id), then
  the blob's file name `BREP.<id>.smbh`. The tag is the integer an ACIS version 7 record writes after its attribute
  pointer: 3722's cap blob holds two bodies tagged 0x6de (the cap's `Extrude1`, record 1507) and 0x6dd (the cap, 1687),
  as its map pairs them.
* A body record's index (`u64` after `u32 2`) counts the component's bodies: 3722's base Revolve2 2, coil 3.
* Every one of the 103 tools of the 35 Combines (3722 included) is missing from its component's body list after the
  Combine, while its blob's map still names it and the blob still holds its last shape. The targets stay listed
  (18), but where a later Combine takes the target as its tool (the header parts' Combine3, 17).

| design (component) | Combines | tools | tools in the body list | tools in the blob |
|---|---|---|---|---|
| 3722 Snowglobe (base, cap) | 2 | 2 | 0 | 2 (blob ends 3 and 2: tool, target, a body without faces) |
| 3900, 4200, 4242, 4400 (adafruit-more), 4128, 4300 (adafruit-next): header part | 3 each | 7 each | 0 | 7 (blob 8 of 9: Extrude1, Mirror1, 5 copies, Mirror2, one without faces) |
| 3651 FeatherWing header part | 3 | 7 | 0 | 7 |
| 4862, 4863 CyberDeck | 1 | 19 | 0 | 19 (blob 20 of 20) |
| 3093 Crickit CPX, Crickit CPX Assembly | 2 | 4 | 0 | 4 (blob 36) |
| 4431 STEMMA Buttons | 1 | 1 | 0 | 1 (list 4 of map 5) |
| 6114 Solar Charger Case | 1 | 1 | 0 | 1 (list 1 of map 2) |
| 6124, 6126 BQ25185, 5981 TRRS (tool removed by RemoveBody) | 1 | 1 | 0 | 1 (the tool's own blob) |
| 324 NEMA-17 | 1 | 1 | 0 | 1 (the list holds a `3A8E00DC`, no body record) |

* So 3722's tools are used up as 3900's are; the stored coil and `Extrude1` are the history blob's record of the tools'
  last shape (3900's header blob holds solids of 291.69, 24.31 and five of 48.62 mm3 beside the finished 583.39: the
  part's tools as they were combined). Nothing in the samples keeps a tool: no Keep Tools to read, and no `keep_tools`
  option is added to the document's `combine`.
* The body list is not the visible bodies: of the body records whose byte after the index and stamp reads (`01 00`
  or `01 01`), 222 are listed with 0 (the tools' value), and none with 1 is missing from a list. Bodies a list leaves out otherwise: RemoveBody
  (2530's thickened sheets), a body joined into another (1313, 3221), bodies moved to another component (5358, 5360,
  2831: the root's list is empty, its blob's bodies are the moved ones, which `fusion_end_blobs_kept` handles).

### The importer: used-up tools are no end bodies
* The tree's `combine` lists the tools it used up that a history blob still holds: `used_up: [{blob, tag}]` (a tool
  whose body record its component still lists gets none). `fusion_end_bodies` passes over a blob body whose tag
  (`gusset_sat::body_tags`, in `body_count` order) is listed for that blob; an entry not read, or a blob whose tags do
  not read, keeps all of its bodies. Every other stored body must still be a finished body, volume for volume.
* 3722 comes in as history: "the finished bodies are the end bodies of the file's history blob, volume for volume"
  (73895.1078 against 73895.0597, 7352.1969 against 7352.2430 mm3).

### Result
* 3722 Snowglobe gains history (its tree's two Combines: `used_up` tags 2406 = 0x966 in the base's blob, 1758 = 0x6de
  in the cap's).
* No loss: every design with a Combine (the 22 files of the three sets that hold one) imports as before but 3722
  (3900, 3651, 4200, 4242, 4400, 4128, 4300, 4862, 4863 history, word for word; 3093, the Crickit Assembly, 6124,
  6126, 324, 4431, 5981, 6114 the same refusals). The tree changes only by the `used_up` key of combines; the parser
  keeps the fields it read before (the tag is set apart, not read as a field).
* Not done: Keep Tools itself. A Combine that keeps its tools would leave the tool's body record in its component's
  body list; the importer would then find the tool among the end bodies, and the document's `combine` would need to
  keep a copy (as `thicken` keeps its sheet). No sample has one, so neither the flag nor the copy is written.
* Tests: `gusset-sat` (136, new `a_bodys_tag_is_the_integer_after_its_attribute_pointer`,
  `a_combines_tool_left_out_of_its_components_body_list_is_used_up`), `gusset-commands` (277, new
  `the_tools_a_combine_used_up_are_read_from_the_tree`) pass; clippy and fmt clean.

## 61. Round 33: a thin wall's faces per line and per curve, faces through joins, face profiles on touching bodies (2026-11-13)
Readers: `fusion_tree/thin.rs` (`Caps::role`, `LineWall`, `Rims`), `fusion_tree/naming.rs` (`face_of_extrude`,
`stored_plane`, `modelled_edge`, `rim_of_a_thin_cap`), `fusion_tree/json.rs` (`offset_faces_json`,
`profile_face_body`); importer: `tree_import/v2.rs` (`FacePick.body`, `find_pick`); kernel: `gusset_occt.cpp`
(`gs_move_faces`); OCCT patch `gusset-0004-draft-vertex-parallel-extrema.patch` (ADR 0002).

### 6126 Fillet2 (2): the slots' ends, as the offset left them
* Section 59's numbering of a wall about the middle of separate lines is now modelled (`LineWall`): per selected
  line k of n, from b = 3 + 3n (12 for 6126's three lines, 6 for 6299's `Extrude14`, one line): `b + 7k` both
  sides, `+3` the cap on the sketch plane, `+5` the far cap, `+4` / `+6` the ends at the line's start / end. The
  first operation names the same ends `+1` and `+2`: `Fillet2 (2)`'s edges run between `13 [363]` and a side and
  end on `16 [364]`'s plane; `OffsetFaces1` selects `23 [364]` where `Fillet2 (2)` names `20 [363]`.
* Which side carries the second operation, and which of `+1`/`+2` is the start, depends on the way the wall
  goes: against the sketch normal (6126, -0.6 mm) the left side about the normal and the line's run, and `+1` the
  start; along it (6299's `Extrude14`, 2 mm: the stored `6 [406, 407]` at x = -14.8 mm of a line x = -16.1, y -8
  -> 8, and `Fillet7` pairing `8 [406]` with `10 [407]` at y = -8) the right one and `+2`.
* `OffsetFaces1` (-0.25 mm) moved four of the ends before the fillet, and the fillet still names them by the
  extrude alone (no `-368` in its recipes; the stored sides beside them carry it). An offset of faces now records
  the faces it moved and how far; a wall's face under either of its names is moved that far out of the part (into
  the slot for a cut: 6126's "+" grows from y -1.5 to -1.25 mm, as the stored sides show: they end 0.5 mm, the
  round's radius, before it). Unit test `a_wall_about_separate_lines_names_its_faces_per_line`.
* 6126 then reads every feature and the rebuild runs to `RemoveInstance1` (`D3DC9613`, the root's removal of one
  occurrence `2D6E13A1`), which is not read: the next blocker.

### 2354 Fillet1 and Fillet2: faces through a join, an end two states give two places
* `Fillet1`'s four unresolved edges name a box's end `6 [376, 377, 380, -381]` (a copy `R-Pattern2` made, trimmed
  by `Extrude6`'s join); the stored states hold `6 [376, 377, 380]` (before the join) and none with `-381`. A
  negative entry is an operation that changed the face's bounds; where it is one of the trimming kinds (extrude,
  sweep, revolve, hole, blend, combine, loft) a stored plane is now found through it, on either name.
* `Fillet2`'s two last edges name tab ends `6 [310, 326, 359]` and `6 [310]` whose names later states also give
  to other faces (a face across the tab at y = 54.5 mm, a face at x = 54.5): the stored planes disagree. An edge's
  face that the model does not give and the stored planes do not agree on is now the plane of the stored faces of
  that name that meet a stored face of every name at the edge's ends (`stored_face_beside` with the recipe's end
  faces; the edge's other face, the top the blend rounds, no longer meets it in those states).
* The rebuild then refused `Extrude1 (2)` ("invalid type: null, expected a string"): a face profile was written
  with `"sketch": null`, which the document's reader does not take. It now carries no sketch. `Revolve1`'s profile
  face (the end of the strip `BoxPrimitive3` at x = 0) lies on the display box's side too, and both hold all of
  the pick's points: a face profile now names the feature whose body holds it (`profile_face.body`, the feature of
  the face's first operation), and the import takes the face on that body where faces of several bodies tie (as
  an offset's `on_body` does). Test `a_face_profile_on_two_touching_bodies_is_the_one_on_its_body`.
* 2354 then stops in the rebuild at `Extrude3`: "body 'BoxPrimitive5' does not exist". `BoxPrimitive5` (record
  1665) is a cut (operation 2) that names a body link of its own (section 43), and later features use that body
  (`R-Pattern1` patterns it, `Extrude4` joins the copies to the ribbon); what such a cut makes is not decoded.

### 6299 Chamfer2: the rims of a slot cut through a plate keep the wall's cap names
* The six edges name faces `3`, `6`, `8 [387]` (`Extrude10 (2)`, a thin cut of 0.6 mm, location 2, along a line,
  an arc and a line, from the plate's bottom z = 13 mm up through its top z = 14.6) with the plate's top
  `2 [353]`: the slot's top rims. No stored body holds those names. Each name occurs in two edges told apart by the
  side beside it (`10 [387]` off the chain, `10 [387, 388]` on it): it is the wall's cap over one curve, which the
  cut through the plate leaves only as the rims' name.
* The first operation of a thin extrude along one chain names its caps per curve: `-m` on the sketch plane, `m`
  at the far end, m counting on along the chain (1 for its start, 2 per line, 3 per arc): 3, 6, 8 for 6299's line,
  arc and line, the same for 5981's hairpin (`Chamfer6`: `-3`, `-6`, `-8` at its sketch plane), and ±3, ±6, ±9
  for 6126's three single-line chains (the vertical edges of `Fillet2 (2)` end on `-3` and `3`). An edge between a
  cap and a plane through it is the rim of the side the recipe names beside it, on that plane: a line's middle, an
  arc's points (`Rims`). Unit test `a_walls_caps_are_named_per_curve_along_its_chain`.
* 6299 then stopped at `Fillet7` (Bottom, r 1.3 mm: the ends of `Extrude14`'s 2.6 mm slot), which the side and
  end naming above reads; it now reads every feature and the rebuild stops at `Extrude2` (Bottom, a join from a
  sketch moved 1.6 mm, lines and circles of 1.1 and 2.5 mm): "could not build an arc of the profile" (not looked
  into). 5981's `Chamfer6` resolves six of its eight edges (not confirmed by a rebuild); the two left are rims of
  `Fillet3`'s rounds, whose stored faces lie elsewhere (x 10.4 mm, not at the slot): not taken.

### 5998 OffsetFaces1 (2): the part failed the self check before the move
* The offset moves the two ends of the minus glyph's pocket 0.4 mm into the part (y 3.46 -> 3.06 and 4.591 ->
  4.991 mm, as the stored states show) with `move_faces`, which then refused: "the moved faces would cut through the
  rest of the part". The part fails `BRepAlgoAPI_Check`'s self-interference test before the move already (the
  glyph solids the file stores, cut into the case). A move on such a part now passes where the result has no more
  faults than the part had and every faulty shape of the result is one the move made of a faulty shape of the
  part; a part that passed must still pass.
* 5998 then rebuilds every feature and stops at the step check: "a stored solid of 1.8263 mm3 is neither a state
  of the model nor the cut tool of a feature" (`Extrude2` of `Component Insert1`, the inserted board).

### A draft that crashed
* With `Chamfer6`'s last two edges taken from those stored rounds (an experiment, not kept: its chamfer then
  failed on an edge of length 0), 5981's rebuild drafted a part the failed chamfer left different, and
  `Draft_Modification` dereferenced the points of an extremum between a curve parallel to the new face and that
  face, which has none: a segmentation fault of the importer. OCCT patch `gusset-0004` refuses the vertex instead
  (`Draft_VertexRecomputation`), as when the extremum finds nothing; `gusset-kernel-occt`'s goldens are unchanged.

### Result
* No design gained history; all four blockers of the round are past. First blockers now: 6126 `RemoveInstance1`
  (was `Fillet2 (2)`); 2354 the rebuild of `Extrude3` on `BoxPrimitive5`'s body (was `Fillet1`); 5998 the step
  check at the inserted board's `Extrude2` (was `OffsetFaces1 (2)`); 6299 the rebuild of `Extrude2` (was
  `Chamfer2`, then `Fillet7`). 6124 also passes its `Fillet4` and stops at the step check of an extrude.
* No loss: round 30's sample of 40 history designs gives the same 35 as history and the same 6 refusals but 2354's
  and 6299's (later blockers); of the touched designs 3027, 3350, 3464, 610, 858, 1054, 3692, 3722 and 3900 come
  in as history as before, 3221, 3502, 3802 and 5981 stop where they did.
* Tests: `gusset-sat` (141; new `a_wall_about_separate_lines_names_its_faces_per_line`,
  `a_walls_caps_are_named_per_curve_along_its_chain`), `gusset-commands` (277), `gusset-kernel-occt` (74),
  `gusset-cli` `modeling_tree_import` (80, new `a_face_profile_on_two_touching_bodies_is_the_one_on_its_body`),
  `modeling_draft`, `modeling_move`, `modeling_offset` pass; clippy (`gusset-sat`, `gusset-commands`,
  `gusset-occt-sys`, `gusset-kernel-occt`, `gusset-cli`) and fmt clean.
