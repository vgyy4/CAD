# `toploc` and `poly`: Rust API contract

This contract covers OCCT 8.0.1's location classes of `TopLoc` (`cpp/TopLoc/`): `TopLoc_Location`, `TopLoc_Datum3D`,
`TopLoc_ItemLocation`, `TopLoc_SListOfItemLocation`, and the mesh data structures of `Poly` (`cpp/Poly/`):
`Poly_Triangulation`, `Poly_Triangle`, `Poly_Polygon3D`, `Poly_Polygon2D`, `Poly_PolygonOnTriangulation`, `Poly_Connect`,
`Poly::` (Catenate, Write/Dump/Read, ComputeNormals, PointOnTriangle, Intersect, IntersectTriLine, PolygonProperties),
`Poly_MergeNodesTool`, `Poly_MakeLoops` (+ `Poly_MakeLoops3D`, `Poly_MakeLoops2D`, `Poly_MakeLoops::HeapOfInteger`) and
`Poly_CoherentTriangulation` with `Poly_CoherentNode`, `Poly_CoherentLink`, `Poly_CoherentTriangle`. They are the modules
`occt_port::toploc` and `occt_port::poly` (add `pub mod toploc;` and `pub mod poly;` to `src/lib.rs`; `src/` may use private
submodules). `TColStd_PackedMapOfInteger`, which `Poly_MakeLoops` needs for the order in which it starts contours, is
`poly::PackedMapOfInteger`. Tests: `tests/poly.rs` with the golden file `tests/poly.txt` printed by `oracle/poly.cpp`
(real OCCT 8.0.1).

Nothing that exists in `src/` changes. The modules reuse `occt_port::gp` (`Trsf`, `TrsfForm`, `Pnt`, `Pnt2d`, `XYZ`, `XY`,
`Dir`, `Dir2d`, `Ax1`, `gp::resolution()`), `occt_port::bnd::BndBox` (the cached min/max box: `add_box`, `transformed`) and
`occt_port::mathlin::MathError`, and must not copy them.

**Sources.** `cpp/TopLoc/`, `cpp/Poly/`, `cpp/TColStd/` and, from the OCCT installation (`$OCCT/include/opencascade`, sources
in `deps/occt-src`), the templates the code goes through: `NCollection_Array1` (note `Assign`), `NCollection_AliasedArray`
(`Resize`), `NCollection_Vec3<float>`, `NCollection_PackedMap`, `NCollection_Primes` (prime list below) and
`NCollection_IndexedMap` (insertion-ordered, 1-based, `Substitute` in place). Not covered: `DumpJson` / `InitFromJson`,
`Poly_TriangulationParameters`, deferred loading (`LoadDeferredData`...), `Map*Array`, `Poly_ArrayOfNodes` as a type of its own,
`Poly_CoherentTriangulation::Iterator*` (use the slot accessors), `Poly_CoherentTriPtr`, `HashCode()` values (only
"equal locations hash equal and the identity hashes to 0" is tested), and `NCollection_BaseAllocator` arguments.

## Conventions

* Scalars: reals `f64`, OCCT `int` is `i32`, `float` is `f32` (a `float` is printed in the golden file as the `f64` it widens to).
  Operations are done in the order of the C++ source with no fused multiply-add and no reassociation: `f32` arithmetic stays `f32`
  (`ComputeNormals`: component sums in `f32`, the modulus is `sqrtf` of `x*x+y*y+z*z`, and `NCollection_Vec3<float>::operator/(f32)`
  multiplies by `1.0f / m`; `Normalized()` divides each component by the modulus and leaves a null vector alone).
* Families compared bit for bit (`EXACT` in `tests/poly.rs`): everything but `merge_tool`, `merge_static`, `merge_element_errors`
  and `make_loops`, which depend on `cos` (`SetMergeAngle`: `(float)cos(angle)`) and `atan2` (angle choice in `MakeLoops`). Use
  `libm::cos`, `libm::atan2`; those four families allow 8 ulps of the largest absolute value on the line, but the oracle prints only
  cases whose outputs do not change at all when every C-library result is moved by an ulp in 24 patterns.
* Errors: `Result<_, occt_port::mathlin::MathError>` (any variant; the test only checks `Err`). **`throws`** lines are exceptions
  OCCT really raises (explicit `throw`, or checks compiled into headers the oracle instantiates itself: `Array1` /
  `AliasedArray` range checks on `Node(i)`, `Triangle(i)`, `Poly_Connect::Triangle/Triangles/Nodes`, `gp_Dir` of a null vector,
  `SetDoublePrecision` on allocated nodes, `Resize(<=0)`, null `Parameter`, `SetParameters` with other bounds, `AddElement(n)` with
  `n != 3,4`, `Poly_Triangulation(n <= 0, ..)`, `ProgramError`s inside `Poly_CoherentTriangulation`). **`unchecked`** lines are
  calls whose checks are compiled out of `libTKMath` (OCCT would read or write outside an array, dereference a null pointer or
  return uninitialised memory); the oracle does not execute them and **the Rust API must return `Err`**: `FirstDatum` /
  `FirstPower` of the identity; `Value()` of an empty `SListOfItemLocation`; any triangle with a node index outside `1..=NbNodes`
  given to `ComputeNormals`, `Poly::Intersect`, `Poly_Connect`, `Poly_MergeNodesTool`, `Poly_CoherentTriangulation(tri)`
  (`mesh_bad_index`); a triangle count `< 0`; `Poly_Connect::Initialize(N)` with `N` outside `1..=NbNodes` or on a default object;
  `Poly_Polygon3D(Nodes, Params)` with `Params` of another length or lower bound; `Parameters()` of a polygon without parameters;
  `PolygonProperties` of an empty array; `ReplaceLink` giving two map entries the same key (`make_loops`); `Poly_CoherentLink`
  nothing else. In every case no panic: the lints forbid indexing, so a state C++ would read out of range is an `Err`.
* Uninitialised memory: arrays created by `Resize`, `AddUVNodes`, `AddNormals` or `Poly_Triangulation(n, m, ..)` are unspecified
  in C++ (the oracle always writes them before reading); the Rust API may zero them. Lines whose C++ result depends on such
  memory are not printed.
* Text values (`Dump`, `ShallowDump`, `Write`) are stored with spaces as `_` and newlines as `\n` in one token. A
  `double` is written as `std::ostream` does: `%.{p}g` with the stream precision `p` (6 by default, `0` acts as 1; `-0`,
  exponent `e+10`/`e-05`), a `std::setw(n)` right-aligns in `n` characters. Pointers in `ShallowDump` are written `<ptr>`.

## `toploc`

`Datum3D` (`TopLoc_Datum3D`): `new()` (identity), `from_trsf(&Trsf)`, `transformation()`, `trsf()`, `form() -> TrsfForm`,
`shallow_dump() -> String` (`" TopLoc_Datum3D <ptr>\n"` then three rows `"  ( %10g,%10g,%10g,%10g)\n"` of `Value(i, j)`, then
`"\n"`; `%10g` is `setw(10)` of a default-precision double). Datums are shared as `Rc<Datum3D>`; **identity is `Rc::ptr_eq`**
(two datums with equal transformations are different datums).

`ItemLocation` (`TopLoc_ItemLocation`): `new(datum: Rc<Datum3D>, power: i32)` stores `datum`, `power` and the transformation
`datum.trsf.powered(power)`; `datum()`, `power()`, `transformation()` (the cumulative transformation of the list node, see
below).

`SListOfItemLocation`: a persistent singly linked list (`Rc` nodes; `Clone` shares). `new()`, `from_item_and_tail(item, &tail)`:
the node's transformation is the item's, `pre_multiply`d by the tail head's transformation when the tail is not empty;
`construct(item)` = `*self = from_item_and_tail(item, self)`; `is_empty()`, `more()` (= `!is_empty`), `clear()`,
`value() -> Result<&ItemLocation>` (empty: `Err`, unchecked), `tail()` (the tail of the empty list is the empty list), `to_tail()` /
`next()` (replace by the tail; C++ reads freed memory when the head node has no other owner, the Rust version simply takes the
tail).

`Location` (`TopLoc_Location`, a list of items; `Clone` shares nodes): `new()`, `from_trsf(&Trsf)` (a fresh datum, power 1),
`from_datum(Rc<Datum3D>)`, `is_identity()`, `identity()` / `clear()` (reset), `first_datum()`, `first_power()` (identity: `Err`),
`next_location()` (the tail as a `Location`; identity for identity), `transformation()` (identity transformation for the identity,
else the head node's), `to_trsf()`, `hash_code()`, `is_equal` / `is_different` / `PartialEq`, `shallow_dump()`
(`"TopLoc_Location : "`, `"Identity\n"` or for every item `"\n       Exponent : <p>\n"` + the datum dump, then `"\n"`), and
* `inverted()`: identity gives itself; else the items in reverse order with powers negated (same datums);
* `multiplied(other)`: `self` if `other` is identity, `other` if `self` is; else `r = self * other.next_location()` (recursively),
  `p = other.first_power()`; if `r` is not identity and `other.first_datum()` is the same datum as `r.first_datum()` then
  `p += r.first_power()` and `r` loses its head; if `p != 0` prepend `(other.first_datum(), p)`;
* `divided(o) = self * o.inverted()`; `predivided(o)`: `o` identity -> self; self identity -> `o.inverted()`; `self == o` -> identity;
  else `o.inverted() * self`;
* `powered(n)`: identity -> itself; `n == 1` -> itself; `n == 0` -> identity; one item -> `(datum, power * n)`; `n > 0` ->
  `self * powered(n - 1)`; `n < 0` -> `inverted().powered(-n)`;
* equality: same datums (pointer) and powers pairwise, same length; the golden file also checks equal locations hash equally and
  `hash_code() == 0` for the identity.
Transformation values are exact (`gp::Trsf::powered`, `pre_multiply`, `inverted` of the existing `gp`).

## `poly`: triangulations

`Triangle` (`Poly_Triangle`): `new()` = (0,0,0), `from_nodes`, `set`, `set_index(index 1..=3, node)`, `get()`, `value(1..=3)`;
bad index `Err`. `Eq`.

`Triangulation` (`Poly_Triangulation`, a value type; `Clone` = `Copy()` = the copy constructor: deflection, purpose, arrays,
cached box):
* `new()` (deflection 0, purpose 0, empty, double precision). `with_sizes(nb_nodes, nb_triangles, has_uv, has_normals)`:
  `nb_nodes <= 0` `Err` (throws), `nb_triangles < 0` `Err` (unchecked); double-precision nodes, UV nodes if asked, normals if asked.
  `from_arrays(nodes, triangles, lower)` / `from_arrays_uv(nodes, uv, triangles, lower)`: double precision nodes (`nodes`
  non-empty, `triangles` non-empty). **Quirk of OCCT 8:** `NCollection_Array1::operator=` takes over the lower bound of its
  source, so `triangle(i)` is addressed `lower..lower + nb_triangles - 1` (`triangle_lower()`); everything else is 1-based.
* `deflection` / `set_deflection`, `mesh_purpose` / `set_mesh_purpose` (`u32`), `clear()` (empties nodes, triangles, UV, normals;
  node and UV arrays keep their precision), `has_geometry()` (nodes and triangles non-empty), `nb_nodes`, `nb_triangles`,
  `has_uv_nodes`, `has_normals`, `is_double_precision`, `set_double_precision(b)` (`Err` when nodes are allocated; applies to UV too).
* `node(i)`, `set_node`, `uv_node`, `set_uv_node` (1-based; out of range `Err`); with single precision storage values are
  rounded to `f32` on `set_*` and widened on read. `triangle(i)`, `set_triangle` (see `triangle_lower`). `normal(i) -> [f32; 3]`,
  `normal_dir(i) -> Dir` (`Dir::new` of the widened components; zero `Err`), `set_normal(i, [f32; 3])`, `set_normal_dir(i, &Dir)`
  (`as f32` of each component). Range errors `Err` (throws).
* `resize_nodes(n, copy_old)`: `n <= 0` `Err`; resizes nodes, and UV / normals if present (same rules as `AliasedArray::Resize`:
  no change when the length is equal, else `min(old, n)` elements kept when `copy_old`); `resize_triangles(n, copy_old)` (`n < 0`
  `Err`, `n == 0` allowed); `add_uv_nodes()` (resize to the node count when absent or of another length), `remove_uv_nodes()`,
  `add_normals()` (same), `remove_normals()`, `compute_normals()`: `add_normals()`, zero all, add every triangle's
  `(n1 - n0) x (n2 - n0)` (in `f64`, then `as f32`) to its three nodes, then each normal becomes `n / m` (multiply by `1/m`,
  `m = sqrtf(x*x+y*y+z*z)`) or `(0,0,1)` when `m == 0`.
* `has_cached_min_max()`, `cached_min_max() -> BndBox` (an empty box when none), `set_cached_min_max(&BndBox)` (a void box
  removes the cache), `update_cached_min_max()` (`min_max` of an identity transformation, accurate, then set),
  `min_max(&mut box, &Trsf, accurate) -> bool`: if a cache exists and (`!has_geometry()` or `!accurate` or the form of the
  transformation is Identity, Translation, PntMirror or Scale) the box is `cache.transformed(trsf)`, else the box of the nodes
  (`Transformed` when the form is not Identity); a void box returns `false`, else it is added to `box` (`add_box`) and `true`.
* free functions: `catenate(&[Option<&Triangulation>]) -> Option<Triangulation>` (null entries skipped; no nodes -> `None`; the result
  has double precision nodes, no UV or normals; triangles are shifted); `compute_normals(&mut t)`; `point_on_triangle(p1, p2, p3, p)
  -> (distance², uv)` as `Poly::PointOnTriangle` (`gp::resolution()` thresholds; `std::min/max(a, b)` as `if b < a`/`if a < b`);
  `intersect_tri_line(start, dir, v0, v1, v2) -> (i32, Option<f64>)` (param only when the result is non-zero; confusion `1e-15`);
  `intersect(&t, &Ax1, closest) -> Option<(Triangle, f64)>`; `polygon_properties(&[Pnt2d]) -> (area, perimeter)` (fewer than 2
  points: `(0, 0)`).

`Polygon3D`: `with_size(n, has_params)` (negative `Err`), `from_nodes(&[Pnt])`, `from_nodes_parameters(nodes, nodes_lower, params,
params_lower)` (`Err` unless equal lengths and equal lower bounds), `deflection` / `set_deflection`, `nb_nodes`, `nodes()`,
`change_nodes()`, `has_parameters`, `parameters() -> Result<&[f64]>` (none: `Err`), `change_parameters()`. Node and parameter
arrays are always 1-based (explicit copy loops). `Polygon2D`: same without parameters (`with_size`, `from_nodes`).
`PolygonOnTriangulation`: `with_size(n, has_params)`, `from_nodes(&[i32], lower)`, `from_nodes_parameters(nodes, nodes_lower,
params, params_lower)` (lengths may differ; **both arrays keep the lower bounds of their sources** as above), `nb_nodes`,
`nodes_lower` / `nodes_upper`, `node(i)`, `set_node(i, n)`, `has_parameters`, `parameters_bounds() -> Option<(lo, up)>`,
`parameter(i)`, `set_parameter(i, v)` (none: `Err`), `set_parameters(Option<(lower, values)>)` (bounds other than the nodes':
`Err`; `None` removes), `deflection`. `Clone` = `Copy()`; for parameters of another length than the nodes `Copy()` is unspecified
(the golden file then checks only the original).

Text: `write_triangulation(&t, compact, precision)`, `dump_triangulation` (= non-compact), `write_polygon3d`, `dump_polygon3d`,
`write_polygon2d`, `dump_polygon2d` exactly as `Poly::Write` (see `cpp/Poly/Poly.cxx`; compact `"Poly_Triangulation\n<nn> <nt> <uv>\n<defl>\n"`
and rows `"x y z\n"`...). `read_triangulation(&str)`, `read_polygon3d`, `read_polygon2d` reproduce `std::istream >>` on the text: a
first token other than the header gives `Ok(None)`; numbers are parsed like `strtod`/`strtol`; whitespace of any kind separates
tokens; text after the data is ignored; `nb_nodes <= 0` for a triangulation `Err` (throws). Inputs that read
uninitialised values are not printed.

## `poly`: connectivity, merging, loops

`Connect<'_>`-free API (the triangulation is shared as `Rc<Triangulation>`): `new()`, `from_triangulation(&Rc<Triangulation>)`,
`load(&Rc<..>)`, `triangulation() -> Option<&Rc<..>>`, `triangle(node)` (a triangle using the node, the last one written, 0 if
none), `triangles(t) -> (t1, t2, t3)` (neighbours over the edges `n1n2`, `n2n3`, `n3n1`, 0 if none), `nodes(t) -> (n1, n2, n3)` (the
node opposite each edge), `initialize(n)`, `more()`, `next()`, `value()` exactly as `Poly_Connect.cxx`; out-of-range `t` / `node`
`Err`. Non-manifold edges keep the last two triangles written; the around-node walk visits each triangle once.

`MergeNodesTool`: `new(smooth_angle, merge_tolerance, nb_facets)`, `merge_nodes(Option<&Triangulation>, &Trsf, reverse, angle,
tol, force) -> Result<Option<Triangulation>>` (static), getters and setters of the C++ header (`merge_tolerance` and `merge_angle`
return the `f32` the value was rounded to, widened), `add_triangulation(Option<&Triangulation>, &Trsf, reverse)`, `add_triangle`,
`add_quad`, `add_element(&[XYZ], n)` / `push_last_element(n)` (`n` other than 3, 4 `Err`), `change_element_node(i)`,
`element_node_index(i)`, `nb_nodes`, `nb_elements`, `nb_degenerative_elems`, `nb_merged_elems`, `result()` (`Err` when no node was
pushed: `Resize(0)` throws). All coordinates are compared in `f32`; the hash map only finds the first match: the golden file keeps
**only cases whose results are the same for 9 different bucket counts**, so any structure that finds a matching node (cell of
`(f32)pos * (f32)(1/tol)` truncated to `i64`, the node's own cell and its 26 neighbours when the tolerance is positive; exact `f32`
equality otherwise) works. `isEqual` is `|dx|,|dy|,|dz| <= (f32)tol` (positive tolerance) and `dot(n1, n2) >= cos` or, with
merge-opposite, `<= -cos`; the normal is `Normalized()` of the `f32` cross product, used only when the angle cosine is above 0.01.

`Link { node1, node2, flags }` (public fields): `new(n1, n2)` (flags 1), `reverse()` (xor 4), `is_reversed()`, `nullify()`,
`is_null()`; equality ignores order of the nodes. Flag values: none 0, forward 1, reverse 2, both 3, reversed bit 4.
Helpers are the traits `MakeLoopsHelper::adjacent_links(node) -> &[Link]`, `MakeLoops3DHelper` (`first_tangent`, `last_tangent`
-> `Option<Dir>`, `normal(node) -> Option<Dir>`) and `MakeLoops2DHelper` (`Option<Dir2d>`). `MakeLoops3D::new(&dyn ..)` and
`MakeLoops2D::new(left_way, &dyn ..)` (the same methods): `add_link` (ignored when `node1 == node2`; otherwise insertion into an
insertion-ordered map keyed by the unordered node pair, flags or-ed), `replace_link(l, new)` (adds `l` when absent, then substitutes
in place; ignored when `new` has equal nodes), `set_link_orientation(l, o) -> i32` (old `flags & 3` or 0), `find_link(l)` (a null link
when absent), `perform() -> i32` (bit 1 loops done, 2 hanging links, 4 failure), `nb_loops`, `loop_links(i)`, `nb_hanging`,
`hanging_links()`; algorithm exactly as `Poly_MakeLoops.cxx` including the left-way choice (`3D`: `CrossCrossed` projection and
`AngleWithRef`; `2D`: `Angle`, negated for the right way; `< 1e-4 - pi` is `pi`; strict `<` keeps the first) and the passes
over `HeapOfInteger`.

`PackedMapOfInteger::new(nb_buckets)` (< 1 is 1) = `TColStd_PackedMapOfInteger`: `add`, `remove`, `contains`, `extent`,
`is_empty`, `clear`, `keys()` = the **iteration order**: a node holds the keys of one block of 32 (`index = (key as u32) >> 5`, bit
`key & 31`), buckets are `index % nb_buckets + 1`, a new node is put at the head of its chain, the node is removed when empty,
`Add` first resizes when the map is empty or has more nodes than buckets to `NextPrimeForMap(nodes)` = the first of `101, 1009,
2003, 5003, 10007, 20011, 37003, 57037, 65003, 100019, 209953, 472393, 995329, ...` that is `>= n + 1` (kept when not larger than the
current count and the map is not empty); resizing walks the old buckets `0..=n`, chain by chain, and pushes each node on the head
of its new bucket. The iteration walks buckets ascending, each chain from the head, each block from the lowest key bit.
`HeapOfInteger::new(nb)` (a packed map and a lazy iterator): `add`, `remove` (advances the iterator first when it points at that
key), `contains`, `top()` (first key of the iteration; empty `Err`), `is_empty()` (initialises the iterator, so `&mut self`),
`clear`.

## `poly`: coherent triangulation

Slots instead of pointers: nodes `0..=max_node()`, triangles `0..=max_triangle()` (empty slots stay, appended in order, never
reused), links `0..nb_link_slots()` (cleared by `ComputeLinks`). `CoherentNode` (`new()`, `from_xyz`, `x/y/z`, `set_uv` /
`get_u/get_v` (default `Precision::Infinite() = 2e100`), `set_normal` (stored as `f32`) / `has_normal` (`|n|² > 1e-7`) /
`get_normal`, `set_index` / `get_index` (default -1), `is_free_node()` (no triangle), `triangles()` the ring of incident
triangle slots in ring order: a new triangle is prepended to the ring, the head is the most recently added), `CoherentLink`
(`new`, `from_nodes`, `from_triangle(&tri, side)`, `node(i & 1)`, `opposite_node(i & 1)`, `get_attribute` / `set_attribute`,
`is_empty`, `nullify`), `CoherentTriangle` (`new`, `from_nodes`, `node`, `is_empty`, `n_connections`, `connected_node`,
`connected_triangle`, `link` slots as above). `CoherentTriangulation`: `new()`, `from_triangulation(Option<&Triangulation>)`,
`get_triangulation()`, `remove_degenerated(tol, want_list)`, `get_free_nodes()`, `max_node`, `max_triangle`, `set_deflection` /
`deflection`, `set_node(&XYZ, i)` (`i < 0` append; `i <= len` replace/append, `i > len` `Err`: unspecified), `node`,
`change_node`, `n_nodes`, `triangle`, `n_triangles`, `n_links`, `link(slot)`, `remove_triangle(slot) -> bool`, `remove_link(slot)`,
`add_triangle(a, b, c) -> Option<slot>` (`None` for a negative node), `replace_nodes`, `add_link(tri, conn)`, `find_triangle(link)`,
`compute_links()`, `clear_links()`, `clone_coherent()` (`None` unless it has nodes and triangles), `dump()`
(`"Node <i>"` + `"  X =%9.4f; Y =%9.4f; Z =%9.4f\n"` and `"      %5d %5d %5d\n"` per ring triangle; free nodes skipped). The algorithms are those of
`Poly_CoherentTriangulation.cxx`, `Poly_CoherentTriangle.cxx` (connections), `Poly_CoherentNode.cxx`; a `Standard_ProgramError`
becomes `Err`. The golden programs call `compute_links` only without link slots (stale link pointers are undefined in C++).

## Record grammar (golden file)

Each line is `<family> <inputs> | <outputs>`. Reals are 16 hex digits, ints decimal, `T` + text for `txt`.
* `trsf`: kind then fields: `2 pnt f` scale, `3 xyz` translation, `4 pnt dir` Ax1 mirror, `5 pnt`, `6 pnt dir` plane mirror,
  `7 x y z w` quaternion rotation, `8 x y z w xyz f` (rotation + translation part + scale), `9` identity. `bx`: as `bnd`.
* `lp`: `nd` then `nd` trsf bodies (datum pool; the registry of datums starts as the pool). `lprog`: `n` then per op `code [arg]`:
  0 identity, 1 `D k` (location of pool datum k), 2 `T k` (location from the transformation of datum k: a **new** datum appended to
  the registry), 3 multiply, 4 divide, 5 predivide, 6 `POW n`, 7 invert, 8 duplicate the top; stack machine, result = top.
  Datum numbers in outputs are registry indices. `items`: `n` then `n` pairs (datum, power).
* `tri`: `kind` (-1 null handle, no more tokens); else `nn nt uv nrm lo defl purpose`, `nn` nodes `xyz`, `nn` `xy` if `uv`,
  `nt` triples, `nn` triples of floats if `nrm`. Build kinds: 0 `Poly_Triangulation(nn, nt, uv, nrm)`; 1 default constructor,
  `SetDoublePrecision(false)`, `ResizeNodes(nn, false)`, `ResizeTriangles(nt, false)`, `AddUVNodes`, `AddNormals`; 2 array
  constructor with arrays whose lower bound is `lo` (then `AddNormals`); then `SetNode/SetUVNode/SetNormal` for 1..nn, `SetTriangle`
  for `lo..` (kind 2) or `1..`, `Deflection(defl)`, `SetMeshPurpose(purpose)`.
* `tops`: `n` ops: 0 `AddUVNodes`, 1 `RemoveUVNodes`, 2 `AddNormals`, 3 `RemoveNormals`, 4 `ResizeNodes n copy`, 5
  `ResizeTriangles n copy`, 6 `SetDoublePrecision b`, 7 `Clear`, 8 `ComputeNormals`, 9 `Deflection f`, 10 `SetMeshPurpose p`, 11 `SetNode i xyz`,
  12 `SetUVNode i xy`, 13 `SetTriangle i a b c`, 14 `SetNormal i f f f`, 15 `T = Copy()`.
* `trilist`: `k` then `k` tri bodies. `pg3`: `kind nn hasp lo np plo defl`, nodes, `np` parameters (kinds: 0 `(nn, hasp)` then set values, 1
  nodes array with lower `lo`, 2 nodes and parameters arrays). `pg2`: `kind nn lo defl` nodes. `pot`: `kind nn hasp nlo np plo defl`, `nn`
  ints, `np` reals. `pts2d`: `n` then `n` xy.
* `mcfg`: `angle tol unit nbf flags` (flags: 1 merge-opposite, 2 drop-degenerative, 4 merge-elems). `mops`: `n` ops: 0 + 3 xyz (AddTriangle),
  1 + 4 xyz (AddQuad), 2 + tri + trsf + reverse (AddTriangulation).
* `loops`: `dim left nn`, `nn` positions (`xyz` for 3, `xy` for 2), `nh` helper links `n1 n2 mask` (mask bit 1: no first tangent, bit 2: no
  last tangent; the helper's adjacency of node k is the helper links incident to k in table order; a tangent of a link is the direction from
  its effective start (node2 if reversed) to its end, none for a zero length or a masked link), for `dim == 3` `nn` normals `has dir`,
  `n` ops: 0 `AddLink n1 n2 flags`, 1 `ReplaceLink n1 n2 f m1 m2 f2`, 2 `SetLinkOrientation n1 n2 o` (prints the old orientation), 3
  `FindLink n1 n2` (prints the link), 4 `Perform` (prints the code, the loops, the hanging links; every link as `n1 n2 flags`).
* `mapops`: `nb n` then `n` pairs `(code, key)`: packed map 0 add, 1 remove, 2 contains, 3 snapshot, 4 clear (`extent is_empty count keys...`); heap 0
  add, 1 remove, 2 contains, 3 top, 4 is_empty, 5 clear.
* `cops`: `n` ops (indices are taken modulo the current counts): 0 `x y z mode` (SetNode; mode 1 reuses the first free node, 2 uses
  index = length), 1 `a b c` AddTriangle, 2 `a` RemoveTriangle, 3 `a b c d` ReplaceNodes (negative node -> -1), 4 ComputeLinks (-1 if
  link slots exist), 5 ClearLinks, 6 `tol want` RemoveDegenerated, 7 `a conn` AddLink, 8 `a` RemoveLink (the a-th non-empty link), 9
  `a u v`, 10 `a x y z`, 11 `a idx` node setters, 12 `f` SetDeflection. The final state dump is `put_coh` of the oracle.

## Families

Every family has at least 28 distinct lines (30 for `toploc`), all with inputs. `unchecked` lines: `loc_first` (identity),
`slist_build`, `mesh_bad_index` (all), `tri_ctor_invalid`, `pg3_state`, `pg3_params_none` (all), `poly_polygon_properties`,
`connect_checks`, `make_loops`. The input text of the oracle (`oracle/poly.cpp`) documents each family.
