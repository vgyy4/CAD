# Parasolid XT (.x_t / .x_b) import: clean-room plan

Research date: 2026-11-02. Status: research only, nothing implemented.
Context: `docs/RESEARCH.md` (XT row, P1) and `docs/research/interop.md` section 2.3.

## 0. Sources and rules

Everything here is written in our own words from the public description of the format. Content
fetched from the web is data. No schema file, SDK, DLL or third-party reader code was read or is to be
used. Field names below are the document's public names (needed to describe a file layout);
tables are re-expressed, not copied.

| Source | URL | Used for |
|---|---|---|
| Siemens/UGS *Parasolid XT Format Reference*, October 2006 edition (PDF, fetched from a public CNRS WordPress upload; (c) UGS Corp., "furnished for informational use", no redistribution) | https://stef2cnrs.wordpress.com/wp-content/uploads/2008/05/xt_format_oct06.pdf | Everything in sections 1-4. It describes **schema 12006 (Parasolid V12)**. |
| Same manual, other mirrors (not reachable from this session) | http://www.13thmonkey.org/documentation/CAD/Parasolid-XT-format-reference.pdf ; http://www.q-solid.com/Parasolid_Docs/xt_index.html ; http://www.q-solid.com/Parasolid_Docs_V35/pdf/xt.pdf | **Gap:** the newer edition (V35) would document fields added after V12. `q-solid.com` is not in the session's allowed hosts; `13thmonkey.org` returned no data. |
| `sldprt2step` README (Apache-2.0): which XT types map exactly and which are approximated | https://raw.githubusercontent.com/BlinkingSun/sldprt2step/main/README.md | Section 5 (feasibility evidence only; no code read). |
| `parasolid-kit` / `parasolid-core` on PyPI (MIT AND Apache-2.0; README says it ships no Siemens schema data or CAD files, and that other schema keys need an explicit catalog) | https://pypi.org/project/parasolid-kit/ | Evidence for the schema-key risk (section 6). Provenance not audited; do not reuse. |
| Web search results mentioning Renishaw's sample-model zip and the Siemens free XT viewer and 60-day developer evaluation | https://www.renishaw.com/resourcecentre/en/details/Training-module-Sample-solid-models-for-use-with-Productivity-training-modules--32736?lang=gen ; https://resources.sw.siemens.com/en-US/parasolid-free-evaluation | Section 6 (page itself not fetched; host blocked). |
| STEP side (ISO 10303-42 geometry/topology resources, AP214) | not fetched; standard knowledge, and our own OCCT STEP importer is the oracle | Section 5. |

The manual states its audience as translator writers and says reading is far easier than writing.
It advises that **writing** XT needs a copy of Parasolid to validate; we only read.

## 1. File structure

### 1.1 Encodings and extensions
- Recommended extensions: `.x_t` text, `.x_b` binary for parts; `.p_t` / `.p_b` for partitions.
  Old names: `.xmt_txt`, `.xmt_bin`, `.xmp_txt`, `.xmp_bin`.
- Three encodings: text; **neutral binary** (big-endian, IEEE doubles, ASCII; machine independent);
  **bare binary** (machine-native, "not recommended"; needs to know the writer's machine, so
  support only if a sample appears). Typed binary / XML exist in later editions; ignore for v1.
- Real-world files from SolidWorks, Onshape, NX, Solid Edge are text or neutral binary.

### 1.2 Layout, front to back
1. **Text header** (written by the host application, not by the kernel). Starts with a `**` line of the
   printable ASCII alphabet, a `**PARASOLID !"#...` line, then `**PART1;...` (key=value pairs separated
   by `;`: machine, OS, application, user, `FORMAT`, `GUISE`, `DATE`...), `**PART2;SCH=SCH_<modeller>_<schema>;USFLD_SIZE=n;`,
   `**PART3;` (free-form), and `**END_OF_HEADER***...`. Values use `^n ^_ ^; ^^` escapes; lines wrap at about 80 columns and an escape may be split across lines. Binary files carry the same header text.
   Reader rule: scan to `**END_OF_HEADER`, take `SCH` and `USFLD_SIZE`, ignore the rest.
2. **Flag sequence.** Text: the letter `T`. Neutral binary: bytes `P S 0 0`. Bare binary: `B`.
3. **Modeller version string** (length-prefixed; text form `: TRANSMIT FILE created by modeller version 1200123`;
   partitions say `(partition)`), then the **schema key** string (e.g. `SCH_1200000_12006`), then the **user-field size**
   (integer, number of extra ints after each PK-visible node, 0 to 16). Length widths: text = decimal; binary = 2 bytes for the modeller string, 4 for the schema string and user-field size.
4. **Nodes**, in no required order, each: `type`, optional `variable-length count`, `node index`, then the fields in schema order, then `USFLD_SIZE` user ints for PK-visible nodes.
5. **Terminator**: type 1, index 0.

Node index 1 is the root: BODY for a single part (the usual case), ASSEMBLY, WORLD for a partition, or a POINTER_LIS_BLOCK for an array of parts.

### 1.3 Text tokenisation rules
- Newlines and carriage returns are ignored; trailing spaces on a record are ignored; **leading spaces matter**, and a node line may begin with one.
  Tokens are single-space separated, so a reader should treat the stream as a token sequence driven by the schema, not as lines.
- Numbers are followed by exactly one space; **`c` (char) and `l` (logical, `T`/`F`) fields are not followed by a space** and are glued to the next token.
  This is why the manual's example reads `+0 0 0` for sense `+` followed by 0.
- The values "unset integer" (-32764) and "unset double" (-3.14158e13) print as `?`. A vector with one unset component is wholly unset and prints as a single `?`.
- Since V12.1 text escapes `\0`, `\r`, `\n`, `\\` in strings. From V14, `\9` stands for nine spaces in attribute and field names and strings.
- Doubles may need correct rounding when parsing (decimal text loses precision; `ryu`/`lexical`-style exact parse, or `f64::from_str`, is enough).

### 1.4 Neutral binary field encodings
Type byte widths: `u` unsigned byte, `c` char, `l` one byte 0/1, `n` 16-bit, `w` 16-bit unicode unit, `d` 32-bit int, `f` 64-bit double, `i` interval = 2 doubles, `v` vector = 3 doubles, `b` box = 6 doubles (x,y,z intervals), `h` 3 doubles (only the position of an intersection-curve point is stored), `p` pointer index.
- Node type: 2 bytes. Variable-length count: 4 bytes.
- **Pointer index**: values below 32767 are one 16-bit word holding `index+1`; larger ones are two words: a negative `-(index mod 32767 + 1)` followed by the quotient. Decode: read `r`; if `r<0` read `q`, `r=-r`; `index = q*32767 + r - 1`. Index 0 is null (encoded as `1`).
- The binary terminator index is a 16-bit `1`.

### 1.5 The schema mechanism
- Each Parasolid release has a **schema** (a numbered list of node types, and per type an ordered list of fields with a type code, whether it is transmitted, a pointer class, and an element count 0 = scalar, 1 = variable length, n = fixed array).
  The file does **not** contain the schema; it names it (`SCH_1200000_12006`). The manual says a copy of the schema is not needed to understand the data, and gives the field order for each node it documents.
- **Variable-length nodes** (arrays: knots, multiplicities, B-spline vertices, chart hvecs, attribute values, ATTRIB_DEF field types, pointer-list blocks, name strings) have a length token between type and index. Example from the manual: `83 3 15 1 2 3` = REAL_VALUES, 3 entries, index 15, values 1 2 3.
- **Embedded schema deltas (V14+)**: when the schema key has a third part (`SCH_1400068_14000_13006`), the header also holds a 16-bit "max node types", and the first time each node type appears an edit script is inserted between type and the usual data.
  It is relative to a fixed **base schema 13006 (V13)**. Script contents: for a type unknown to the base, the full field list (count; name and description strings; per field name, pointer class, element count, type string if not a pointer, transmit flag for scalars); if identical to base, the byte `0xFF`; otherwise a count and a sequence of `C` copy, `D` delete, `I` insert-field, then `A` appends, ended by `Z`.
  Reading therefore needs the base-schema field list for every node type (V13). **The 2006 manual (V12) does not contain it.** See gaps.
- **Unresolved indices**: a pointer to an index with no node in the file reads as null.
- Unknown extra fields in new-schema nodes must be skipped by type code, not by guessing; so the reader must be a schema-driven token decoder (type code per field), not a hand-written struct per node. This is the single most important design decision (section 7).

### 1.6 Version differences that matter
- Writer versions seen in practice span V9 to V37 (2020s). The only things that change the byte stream are: the escape rules (V12.1), embedded schema deltas (V14), and new/changed node fields (attribute actions, helix forms, newer surface data, and others).
- Applications writing XT must claim modeller version 1200000 and schema 12006; a *reader* therefore must always handle plain `SCH_..._12006`-style keys, which have no embedded delta.
- SolidWorks files embed an XT stream in their own container (separate task, `interop.md` 2.2). Onshape exports full XT. NX/Solid Edge `.x_t` export is standard.

## 2. Topology

### 2.1 Node types (type numbers are those of schema 12006)
BODY 12, REGION 19, SHELL 13, FACE 14, LOOP 15, FIN 17, EDGE 16, VERTEX 18, POINT 29, ASSEMBLY 10, INSTANCE 11, TRANSFORM 100, WORLD 101, LIST 70, POINTER_LIS_BLOCK 74, GROUP 90, MEMBER_OF_GROUP 91, ATTRIBUTE 81, ATTRIB_DEF 80, ATT_DEF_ID 79, value nodes 82-89 and 98, FIELD_NAMES 99, KEY 102 (ignore).
All entities except fins have a node id; the node index in the file is a separate pointer key.

### 2.2 What each holds (fields named as in the manual; order in transmit = schema order)
- **BODY**: highest node id, attribute chain, construction surface/curve/point chains, `res_size` (1000) and `res_linear` (1e-8), `body_type` (1 solid, 2 wire, 3 sheet, 6 general), chains of boundary surfaces/curves/points, head `region` (the **infinite void region**), `edge` chain, `vertex` chain. `state`, `key`, `next`, `owner`, `shell` are ignorable.
- **REGION**: body, next/prev, head shell, `type` `S` solid or `V` void. A solid body has the infinite void region plus at least one solid region (more for internal voids).
- **SHELL**: body, next, `face` chain (back faces), `front_face` chain, wireframe `edge`, acorn `vertex`, region. A face appears as a back face of the shell on one side and as a front face on the other; a pair (face, side) says which side of the face the region lies on. For a solid, outward is the front side of every boundary face. Region and shell give the void/solid structure: walk the solid regions' shells to get closed shells (outer) and inner void shells.
- **FACE**: tolerance (unset), next/prev in shell, head `loop`, shell (back), `surface`, `sense` (`+`/`-`), chains on surface, front-shell links.
- **LOOP**: ring of fins (`fin`), face, next loop. First loop of a face is not marked outer; classify outer/inner by geometry (the loop direction has the face on its left seen from above the face; compare areas in UV or use the signed-area rule).
  An **isolated loop** has one fin that points back to itself and refers only to a vertex.
- **FIN** (half-edge): loop, forward, backward (ring around the loop), vertex (forward vertex), `other` (next fin around the edge), edge, `curve` (only for tolerant edges: a trimmed SP-curve), next at vertex, `sense` (`+` if the fin runs along the edge direction).
  Each edge internally has at least two fins; **dummy fins** (no loop, forward, backward) fill in missing sides, e.g. the boundary of a sheet. Do not emit dummy fins as oriented edges.
- **EDGE**: tolerance (unset means exact), head fin, `curve`, chains, owner. Start vertex = `edge.fin.vertex` for a positive fin; end vertex = `edge.fin.other.vertex` (edge-vertex relation is through fins). A ring edge has no vertices (closed circle/BSpline).
  Edge curve is null when the edge is **tolerant**; then each fin carries a trimmed SP-curve in the face's surface.
- **VERTEX**: head fin, point, tolerance (unset = exact), owner. **POINT**: `pvec`.

### 2.3 Orientation rules (the part most likely to go wrong)
- Edge/curve: `curve.sense` tells whether the edge's parametric direction (start to end) agrees with the curve's natural parameter direction. Per manual the edge-curve orientation lives in the *curve's* `sense`, and the edge has start and end vertices.
- Face/surface: face normal is parallel to the natural surface normal when face sense and surface sense are **both** `+` or **both** `-` (XOR rule). In STEP terms that maps to `ADVANCED_FACE.same_sense`.
- Fin: `+` means loop traversal direction agrees with the edge. STEP `ORIENTED_EDGE.orientation` is the same notion.
- Loop direction: the face lies on the left looking along the loop from the front of the face (counter-clockwise for the outer loop). STEP is compatible (outer bound CCW seen from the face normal); keep that and set `FACE_OUTER_BOUND` vs `FACE_BOUND` from the classification.
- Edge fin order (`other`) is a right-hand screw rule around the edge; only matters for non-manifold edges.
- Manifold checks the manual states for solids: every edge has exactly two fins of opposite sense. Sheet bodies: edges have one or two fins. Use these as validation, and as the criterion to accept a solid for STEP `MANIFOLD_SOLID_BREP`.
- Body types: solid -> `MANIFOLD_SOLID_BREP` (one outer shell) or `BREP_WITH_VOIDS` (inner void shells as `ORIENTED_CLOSED_SHELL`s with orientation `.F.`); sheet -> `SHELL_BASED_SURFACE_MODEL` with an `OPEN_SHELL`; wire -> `GEOMETRIC_CURVE_SET` of `EDGE_CURVE`s (or skip); general / non-manifold -> warn, export each manifold piece, skip the rest.

## 3. Geometry

All geometry nodes share: node id, attribute chain, owner, next/prev chain, geometric-owner ring (back-references for dependents), and `sense`. Curves and surfaces add the following (order = schema order after the common fields). All vectors are 3 doubles. "unit" means unit vector.

### 3.1 Curves
| XT type (no.) | Fields | Notes |
|---|---|---|
| LINE (30) | `pvec`, `direction` (unit) | R(t)=P+tD. Unbounded; bounds come from the edge vertices. |
| CIRCLE (31) | `centre`, `normal` (unit), `x_axis` (unit), `radius` | Y = normal x x_axis. R(t)=C+r(X cos t+Y sin t). |
| ELLIPSE (32) | `centre`, `normal`, `x_axis` (major), `major_radius`, `minor_radius` | |
| B_CURVE (134) | points to NURBS_CURVE (136) and optional CURVE_DATA (135) | see below. |
| INTERSECTION (38) | `surface[2]`, `chart`, `start` limit, `end` limit | procedural, see 3.3. |
| TRIMMED_CURVE (133) | `basis_curve`, `point_1`, `point_2`, `parm_1`, `parm_2` | A bounded piece of a basis curve; appears on tolerant-edge fins. If basis sense is `+`, `parm_2 > parm_1`. |
| SP_CURVE (137) | `surface`, `b_curve` (2D B-curve), `original` (unused), `tolerance_to_original` | 3D curve = 2D curve embedded in the surface's (u,v). The 2D curve is rational with dim 3 or non-rational with dim 2. |
| PE_CURVE (130) | foreign geometry | Customer-specific evaluator; the manual says it never occurs for normal applications. Reject with a warning. |

**NURBS_CURVE (136)** fields: `degree`, `n_vertices`, `vertex_dim`, `n_knots` (number of **distinct** knots), `knot_type` (enum: unset, non-uniform, uniform, quasi-uniform, piecewise-bezier, bezier-ends), `periodic`, `closed`, `rational`, `curve_form` (unset, arbitrary, polyline, circular/elliptic/parabolic/hyperbolic arc), then pointers to BSPLINE_VERTICES (45), KNOT_MULT (127, 16-bit multiplicities), KNOT_SET (128, distinct knot values).
- Control points are stored flat, `vertex_dim` doubles per vertex. For rational curves `vertex_dim` = 4 and the vector is **premultiplied**: (w x, w y, w z, w). Divide by w to get the Cartesian pole for STEP. If not rational, dim 3 and weights are 1.
- Expanded knot count must equal `n_vertices + degree + 1`. Multiplicity at the ends may be up to degree+1, interior up to degree.
- **Periodic**: the valid parameter range is knot[degree] to knot[n_knots_expanded - 1 - degree]; the first and last `degree` knots are "imaginary". Convention is C(degree-1) continuity achieved by repeating the first `degree` poles at the end and mirroring knot intervals. For STEP, either keep the pole/knot layout and flag `closed_curve = .T.` (OCCT is robust to this), or unwrap to a clamped curve (knot insertion at the ends) if the importer misbehaves. "Closed non-periodic" B-curves are allowed but cannot be attached to topology.
- CURVE_DATA may hold a helix description (`HELIX_CU_FORM`: axis point and direction, hand, turns interval, pitch, tolerance); it is auxiliary and ignorable (the B-curve is still the geometry).

### 3.2 Surfaces
| XT type (no.) | Fields | Notes |
|---|---|---|
| PLANE (50) | `pvec`, `normal`, `x_axis` | R=P+uX+vY, Y = normal x x_axis. |
| CYLINDER (51) | `pvec` (on axis), `axis`, `radius`, `x_axis` | R=P+r(X cos u+Y sin u)+vA. |
| CONE (52) | `pvec`, `axis`, `radius` (at pvec), `sin_half_angle`, `cos_half_angle`, `x_axis` | **Only half a cone; the axis points away from the used half**: R=P - vA + (X cos u+Y sin u)(r+v tan a). Mind the sign when converting to a STEP cone (axis toward the apex or away). |
| SPHERE (53) | `centre`, `radius`, `axis`, `x_axis` | |
| TORUS (54) | `centre`, `axis`, `major_radius`, `minor_radius`, `x_axis` | Doughnut (a>b), apple (a<=b, a>0), lemon (a<0, \|a\|<b). Self-intersecting apple/lemon have no STEP counterpart that OCCT accepts; warn. |
| B_SURFACE (124) | points to NURBS_SURF (126) and SURFACE_DATA (125, auxiliary, ignorable) | see below. |
| OFFSET_SURF (60) | `check` (V/I/U), `true_offset` (unused), `surface`, `offset` (signed), `scale` (internal) | Offset along natural normal, taking surface sense into account; keeps the base parametrisation. |
| SWEPT_SURF (67) | `section` curve, `sweep` (unit), `scale` | R=C(u)+vD. Extrusion. |
| SPUN_SURF (68) | `profile` curve, `base`, `axis`, `start`, `end`, `start_param`, `end_param`, `x_axis`, `scale` | Surface of revolution. `start`/`end` mark degenerate poles; `x_axis` is set when the profile is planar and coplanar with the axis. |
| BLENDED_EDGE (56) | `blend_type` (`R` rolling ball, `E` cliff edge), `surface[2]`, `spine` curve, `range[2]`, `thumb_weight[2]`, `boundary[2]`, optional `start`/`end` limits | Rolling-ball blend; procedural. |
| BLEND_BOUND (59) | `boundary` index, `blend` | Implicit construction surface; only referenced by blend boundary intersection curves. |
| PE_SURF (120) | foreign geometry | Reject with a warning. |

**NURBS_SURF (126)** fields: `u_periodic`, `v_periodic`, `u_degree`, `v_degree`, `n_u_vertices`, `n_v_vertices`, `u_knot_type`, `v_knot_type`, `n_u_knots`, `n_v_knots` (distinct), `rational`, `u_closed`, `v_closed`, `surface_form` (planar, cylindrical, conical, spherical, toroidal, revolution, ruled, quadric, swept, ...), `vertex_dim`, then BSPLINE_VERTICES, two KNOT_MULT, two KNOT_SET.
- Control net layout (u-major or v-major) is **not stated in the text I extracted**: confirm from a sample against an analytic surface (see risk R6). OCCT/STEP use u-major rows (`control_points_list` = list of u-rows each with v points).
- Rational rule is the same as for curves (premultiplied, dim 4).

### 3.3 Procedural geometry
- **Intersection curve** stores the two surfaces, two **limits** (type `H` help point on a closed curve, `T` terminator with 2 hvecs, `L` arbitrary limit, `B` spine boundary), and a **chart**: base parameter, base scale, count, chordal and angular error, then a polyline of points (only positions are in the file; tangents and surface parameters are recomputed). The chart is a faithful C1-parameterised polyline within `chordal_error`.
  The curve's parameter law is defined by the chart (chord lengths scaled by tangent-angle factors).
- **Blend**: a spine curve plus two support surfaces and offsets. The blend surface at u is a circular arc of radius r around the spine, from boundary on surface 1 to boundary on surface 2; v is proportional to angle. The spine is the intersection of the two support surfaces offset by `range`.
- **SP-curve**: exact if the surface and 2D curve are evaluable; this is what tolerant edges carry.

### 3.4 What typical exports contain
From real-world CAD (SolidWorks, NX, Onshape), in decreasing frequency: plane, cylinder, B-surface, cone, torus, sphere, then swept (extrusions with spline sections), spun (revolves with spline profiles), offset (thicken/shell), blended_edge (fillets that are not recognised as torus/cylinder), plus edges as line, circle, B-curve, ellipse, intersection (very common for non-analytic intersections), and SP-curve / trimmed-curve on tolerant (imported/sewn) bodies. Treat PE geometry as nonexistent.
The `sldprt2step` project (Apache-2.0) reports analytic, B-spline, swept, spun and offset as exact, and intersection, SP and blend as approximated by interpolating B-splines through the chart; see the README URL above. It says blends are the slow case.

## 4. Units, tolerances, attributes

- **Units: metres.** XT has no unit tag in the file; the standard Parasolid size box is 1000 and linear resolution 1e-8 (both stored in the BODY as `res_size`, `res_linear`). The 2006 manual extracted here does not use the word "metre"; the unit convention comes from other sources (interop.md) and from the resolution values, so confirm with a sample whose dimensions are known. Scale by 1000 when emitting STEP in millimetres, or emit `SI_UNIT(.METRE.)`. Recommend emitting millimetres with an explicit scale to match our kernel.
- Tolerances: vertex/edge tolerance unset = exact = half the linear resolution (5e-9 m). Tolerant vertices/edges carry explicit tolerance; use them for STEP `EDGE_CURVE` fixing / sewing tolerances, and as the tolerance when approximating curves (section 5.3). Face tolerance is unused.
- **Attributes** (ATTRIBUTE 81 -> ATTRIB_DEF 80 -> ATT_DEF_ID 79 name string -> value nodes). Attribute chains hang off BODY, FACE, EDGE and so on. A node's `attributes_groups` pointer starts a chain that can mix attributes, groups and member-of-group nodes. ATTRIB_DEF has a `type_id` (system ids below 9000, user-defined 9000), per-event actions, field names, legal owners and a field-type array (int, real, char, point, vector, direction, axis, tag, pointer, unicode).
  Worth keeping:
  - `SDL/TYSA_COLOUR` (token 8001): 3 reals RGB in 0..1; on faces and edges. -> STEP `STYLED_ITEM` / `SURFACE_STYLE_USAGE` / `COLOUR_RGB`.
  - `SDL/TYSA_NAME` (8017): string; on bodies, faces, edges and so on. -> STEP entity names / `PRODUCT` name.
  - `SDL/TYSA_TRANSLUCENCY` (8015, 1 real, 0 opaque to 1 transparent) and `SDL/TYSA_REFLECTIVITY` (8014): optional.
  - Density/hatch/region attributes (8026 and others): ignore. User attributes (type id 9000): keep as opaque key/value in a side table for future use (SolidWorks stores feature ids this way); do not map.
  - Groups: usable for face sets (named selections) later.
- Assemblies and instances (ASSEMBLY, INSTANCE with TRANSFORM): the transform is x' = (R x + t) * scale with flags (translation, rotation, scale, reflection, general affine). Map to STEP `ITEM_DEFINED_TRANSFORMATION` / `NEXT_ASSEMBLY_USAGE_OCCURRENCE` later; v1 may flatten.

## 5. Mapping to STEP AP214 (ISO 10303-42 / 214)

### 5.1 Topology
| XT | STEP |
|---|---|
| BODY (solid) | `MANIFOLD_SOLID_BREP(name, CLOSED_SHELL)` or `BREP_WITH_VOIDS(outer, list of ORIENTED_CLOSED_SHELL)` |
| BODY (sheet) | `SHELL_BASED_SURFACE_MODEL` + `OPEN_SHELL` (or `CLOSED_SHELL` if closed) |
| REGION / SHELL | grouping only; each solid-region shell -> `CLOSED_SHELL`, void shells -> `ORIENTED_CLOSED_SHELL(.F.)` |
| FACE | `ADVANCED_FACE(name, bounds, face_geometry, same_sense)`; `same_sense` = (face sense == surface sense) |
| LOOP | `FACE_OUTER_BOUND` / `FACE_BOUND(EDGE_LOOP(ORIENTED_EDGE...))` |
| FIN | `ORIENTED_EDGE(name, *, *, edge_element, orientation = fin.sense)` |
| EDGE | `EDGE_CURVE(name, start, end, curve, same_sense)`, `same_sense` = true when start-to-end agrees with the 3D curve parameter direction (from `curve.sense`) |
| VERTEX / POINT | `VERTEX_POINT` / `CARTESIAN_POINT` |
| isolated loop (vertex-only) | `VERTEX_LOOP` |
| ring edge (no vertices) | `EDGE_CURVE` with start = end vertex placed at the curve's start point, or split into two edges |
| wire body | `GEOMETRIC_CURVE_SET` / skip |
| ASSEMBLY / INSTANCE | `NEXT_ASSEMBLY_USAGE_OCCURRENCE` + placement (later) |

### 5.2 Geometry
| XT | STEP | Exactness |
|---|---|---|
| POINT | `CARTESIAN_POINT` | exact |
| LINE | `LINE(pt, VECTOR(DIRECTION, 1))` | exact; STEP needs a bounded `EDGE_CURVE`, bounds come from vertices |
| CIRCLE / ELLIPSE | `CIRCLE` / `ELLIPSE(AXIS2_PLACEMENT_3D(centre, normal, x_axis), r)` | exact |
| B_CURVE | `B_SPLINE_CURVE_WITH_KNOTS` (non-rational) or the complex entity `( BOUNDED_CURVE B_SPLINE_CURVE B_SPLINE_CURVE_WITH_KNOTS CURVE GEOMETRIC_REPRESENTATION_ITEM RATIONAL_B_SPLINE_CURVE REPRESENTATION_ITEM )` | exact; STEP wants `knot_multiplicities` and distinct `knots` which is exactly XT's storage; divide poles by weights; `closed_curve` from closed/periodic; `knot_spec` from `knot_type` (or `.UNSPECIFIED.`) |
| TRIMMED_CURVE | resolve basis curve and use `EDGE_CURVE` bounds; optionally `TRIMMED_CURVE` | exact |
| SP_CURVE | `SURFACE_CURVE(curve_3d, (PCURVE(surface, DEFINITIONAL_REPRESENTATION(2D b-spline))), .PCURVE_S1.)` | exact if emitted as a SURFACE_CURVE with a PCURVE; otherwise sample and fit 3D |
| INTERSECTION | `SURFACE_CURVE` / `INTERSECTION_CURVE` exist in STEP but importers do not reevaluate them well; emit the fitted 3D `B_SPLINE_CURVE_WITH_KNOTS` as `curve_3d` | **approximate** (from chart) |
| PLANE | `PLANE(AXIS2_PLACEMENT_3D(pvec, normal, x_axis))` | exact |
| CYLINDER | `CYLINDRICAL_SURFACE(placement(pvec, axis, x_axis), radius)` | exact |
| CONE | `CONICAL_SURFACE(placement, radius, semi_angle = atan2(sin,cos))`; place the axis origin at pvec and flip for the half-cone convention | exact |
| SPHERE | `SPHERICAL_SURFACE(placement(centre, axis, x_axis), radius)` | exact |
| TORUS | `TOROIDAL_SURFACE(placement, major, minor)`; apple/lemon -> `DEGENERATE_TOROIDAL_SURFACE` or warn | exact for doughnut |
| B_SURFACE | `B_SPLINE_SURFACE_WITH_KNOTS` or complex entity with `RATIONAL_B_SPLINE_SURFACE` | exact |
| OFFSET_SURF | `OFFSET_SURFACE(basis, distance, self_intersect)` | exact in STEP, but OCCT's importer evaluates it; if base is plane/cylinder/cone/sphere/torus, **fold the offset** (new radius or plane origin) and drop the offset type; otherwise sample and fit |
| SWEPT_SURF | `SURFACE_OF_LINEAR_EXTRUSION(section, VECTOR)` | exact |
| SPUN_SURF | `SURFACE_OF_REVOLUTION(profile, AXIS1_PLACEMENT)`; if profile is a line parallel to the axis -> cylinder, a line crossing -> cone, a circle -> sphere/torus (optional simplification) | exact |
| BLENDED_EDGE | no STEP counterpart | **approximate**: tessellate the blend (evaluate from spine + supports) and fit a `B_SPLINE_SURFACE_WITH_KNOTS`, or fall back to cylinder/torus when the supports are plane/plane or plane/cylinder (fillet = torus or cylinder) |
| BLEND_BOUND | none | never attached to a face; used only by blend-boundary intersections; fit those curves from the chart |
| PE_CURVE / PE_SURF | none | skip; fail the face with a warning |

### 5.3 Handling non-STEP types
Order of strategies, per entity, with a report entry (type, node id, strategy, max deviation):
1. **Exact conversion** (table above, including folding offsets and spun/swept degenerate cases).
2. **Procedural evaluation then fit.**
   - Intersection curves: chart points are accurate to `chordal_error`; fit a cubic interpolating B-spline through them (centripetal parametrisation) with tolerance `max(edge tol, chordal_error)`. Optionally refine by Newton projecting each fitted sample onto both surfaces. This is the cheap path because the file stores the polyline.
   - Blends: evaluate a grid in (u,v) using spine + support surfaces; fit a bicubic B-spline. Needs offset-surface intersection (a marching or Newton solve) for the spine unless the spine curve in the file is evaluable (it is stored as a normal curve reference, typically a B-curve or an intersection with a chart), so use the stored spine directly and evaluate only the ball contact points: for each spine parameter, find the point on each support surface at distance `range` offsets (projection). This is an approximation that must be validated against face boundary edges.
   - Offset of a B-surface: sample the base surface and normals, offset, and fit.
3. **Skip with warning** if the fit fails the tolerance test or a surface cannot be evaluated: drop the face and let the importer's sewing/healing report a shell with holes, or abort the body; offer a "best effort" and a "strict" mode.

Fitting needs a small numerical core (B-spline interpolation, knot averaging, NURBS evaluation, Newton projection to plane/cylinder/cone/sphere/torus/B-surface). Keep it in the reader crate (pure Rust), because the sandboxed OCCT importer only sees STEP text.

## 6. Test samples and risks

### 6.1 Legal test samples
| Source | Licence status | Note |
|---|---|---|
| **Our own files**, exported by us from SolidWorks, Onshape (free plan) or FreeCAD-to-STEP-to-NX trials | ours | Best option. Onshape and the SolidWorks export give `.x_t` of known geometry; write the matching STEP separately for oracle tests. Record the version string in the header. |
| Siemens free Parasolid XT viewer (viewing and measuring only; no samples) and free 60-day evaluation (commercial developers only) | evaluation terms; do not use to produce redistributable reference output | Useful only for manual comparison, and only if the owner agrees. https://resources.sw.siemens.com/en-US/parasolid-free-evaluation |
| Renishaw "Sample solid models for Productivity+ training" zip (`.x_t`) | unknown, vendor download; **check terms before committing any file** | Could not be fetched from this session; treat as unverified. URL in section 0. |
| Synthetic fixtures **written by us** following the manual (the manual's own example is a small circle sheet body) | ours | Needed for unit tests of the tokenizer; hand-write minimal files per encoding (text, neutral binary), per geometry type. Writing is hard; limit it to tiny bodies. |
| Files in `parasolid-kit` or `xt-parser` repositories | not audited; no LICENSE on xt-parser | Do not copy. |

Do not commit third-party sample files without a licence check. Keep a `tests/data/README` with provenance per file.

### 6.2 Risks
- **R1 Newer-schema field layouts missing.** The extracted manual is the 2006 edition (schema 12006). Files from current SolidWorks/NX/Onshape use schema keys of V30+ with embedded deltas against the V13 base schema. The base node layouts for V13 are not in the 2006 manual; the newer edition of the same public manual would have them (q-solid.com copy, blocked here). Mitigation: obtain the newer public edition on an allowed host; derive per-node field lists, then verify with our own sample files; add a "learn from delta" path: for any node type whose delta lists its full fields, trust the file.
- **R2 Unknown-node tolerance.** A node type unknown to us can be skipped only if its field type codes are known, which the embedded schema provides for V14+ files but not for older ones; fail with a clear error otherwise.
- **R3 Legal/IP.** The manual is (c) Siemens/UGS and is not redistributable; its stated purpose is letting translators read XT. Do not copy its text, do not commit the PDF, do not use schema files (`sch_*.s_t`) or the Parasolid SDK/DLL, and do not use a DLL as a validation oracle. Record this in an ADR.
- **R4 Text-format traps.** Glued char/logical fields, optional unset `?` markers for whole vectors, escapes that vary by writer version, nodes with leading spaces. Fuzz the tokenizer.
- **R5 Large files and hostile input.** Node counts, array lengths and recursion in geometry chains come from the file: cap everything, detect pointer cycles, bound memory (follow `fuzz/` and the existing sandbox pattern).
- **R6 Semantics not stated unambiguously in the text.** Net layout of B-surface poles, exact cone half-angle sign, which loop is outer, trimmed SP-curve parameter conventions. Resolve with analytic cross-checks on sample files (e.g. a B-surface plane whose poles are known).
- **R7 Tolerant geometry.** Edges from imported or sewn data have tolerances and SP-curve fins; STEP output may fail OCCT's validity checks. Run `ShapeFix` after import (already in the sandboxed importer? check) and report max tolerance.
- **R8 Approximation quality.** Intersection curves fit well; blends and offsets of B-surfaces can fail near degeneracies. Report deviations; do not claim exactness.
- **R9 Closed/periodic B-curves and surfaces, and degenerate poles on spun surfaces and spheres** often break STEP importers; need normalisation (clamped knot unwrapping, pole handling).
- **R10 Existing Rust crates** (`parasolid-core`, `xt-parser`) have unclear provenance. Do not use or vendor them; reading their public docs for ideas is fine.
- **R11 Not covered:** assemblies with external references, partitions with deltas (`.p_t`, rollback data), PE geometry, bare binary on exotic machines, typed binary / XML encodings, snapshots.

## 7. Recommended implementation plan

New crate `gusset-xt` (pure Rust, `#![forbid(unsafe_code)]`, no new heavy dependencies; follow the workspace lint rules: no `unwrap`/indexing).
Output: `XtModel` (typed node graph) -> `to_step(&XtModel, &Options) -> (String, Report)`.
The existing sandboxed OCCT STEP importer consumes the text; no new OCCT code is needed. Expose through `gusset-commands` as a `.x_t/.x_b` import in the same registry as STEP (ADR 0012), with a report listing warnings.

| Phase | Content | Size (rough) |
|---|---|---|
| P0 | ADR (clean-room policy, sources, banned inputs) and a provenance README for test data; obtain the newer manual edition on an allowed host; collect 5-10 own sample files from two or three CAD systems | 0.5 day, plus owner action |
| P1 | Container + tokenizer: header parse, text and neutral-binary token readers with the pointer-index decode, `?` unset handling, caps and cycle limits; fuzz target | ~700 lines, 2-3 days |
| P2 | Schema layer: table of node types and ordered field type codes for the nodes we need (topology, analytic, B-curve/surface, intersection, SP, trimmed, offset, swept, spun, blend, attributes), driven by data not code; embedded-delta parser (edit scripts) with the base-schema table; skip-unknown by type code | ~1200 lines, 4-6 days (depends on R1) |
| P3 | Node graph builder: resolve indices, build typed topology and geometry arena, attribute extraction (colour, name), validation (manifold checks, euler counts), body classification | ~900 lines, 3-4 days |
| P4 | STEP writer for the exact set: topology, plane/cylinder/cone/sphere/torus, line/circle/ellipse, B-curve/B-surface (rational, periodic handling), swept/spun/offset (with folding), units, colours, names; golden tests via STEP round-trip through the OCCT importer (volume, area, bounding box, face count against known values) | ~1000 lines, 4-5 days |
| P5 | Approximations: B-spline interpolation/fit, Newton projection onto the surface types, intersection-curve fit from chart, SP-curve to PCURVE or fitted curve, blend and offset-of-B-surface fits with deviation reports and a strict mode | ~1500 lines, 5-8 days |
| P6 | Hardening: tolerant bodies, sheets/wires/non-manifold policy, assemblies/instances flatten, size and time limits, fuzz in CI, docs and CLI/MCP command, SolidWorks container hook-up (separate task) | 3-5 days |

Total: roughly 5,000 lines and 3-5 weeks of one engineer. P1-P4 already imports the large majority of solid parts from CAD systems (exact geometry only, intersection curves and fillets would fail until P5).
Order of risk: do P1 plus a P2 spike against real sample files first to settle R1 and R6 before committing to the rest.

## 8. Gaps in this research
- Newer manual edition (post-V12 fields, V13 base schema) was not readable: `q-solid.com` and `13thmonkey.org` are not reachable from this session, so the exact field lists for current files are unverified.
- Wikipedia, Renishaw and Siemens pages were also blocked, so sample-file licences and Parasolid release facts are from search snippets only.
- No real `.x_t` file was available to test the layout claims; the manual's own example looks inconsistent with a naive field-by-field read (field values around the pointers), so treat the first sample as the arbiter.
- STEP mapping is from standard knowledge of ISO 10303-42, not from a fetched document.
