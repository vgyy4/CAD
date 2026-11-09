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
