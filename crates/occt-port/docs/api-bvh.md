# `bvh`: Rust API contract for OCCT 8.0.1

The module is `occt_port::bvh` and translates the public API in `cpp/BVH/`. Numeric aliases are generic over `f32`/`f64` and dimensions supported by OCCT (`1..=4` where the source supports them). Rust indices are `usize` where they index vectors and `i32` where OCCT exposes an `int`. Methods are snake_case, C++ overloads with different argument lists have distinct names, and C++ out parameters return tuples/records. The public methods of each listed type are in scope; the list below is the translation inventory, while `tests/bvh.rs` is the executable numerical contract for the deterministic core.

## Reuse and conventions

Use the existing `gp`, `mathlin`, and `bnd` modules as dependencies where applicable; do not change their APIs. Public structs/enums should derive the standard `Debug`, `Clone`, `Copy`/`PartialEq` traits where their fields permit. No unsafe code or panics for invalid Rust indices/shapes. The test records contain exact `f64` bit patterns and decimal integers/bools. The core BVH arithmetic uses only arithmetic and comparisons, and the listed families are compared bit-for-bit (including signed zero and infinities); booleans and integers are exact.

## Core numeric API

* `Box<T, const N: usize>` (`BVH_Box<T,N>`): `new(min,max)`, `from_point(point)`, `clear`, `is_valid`, `add(point)`, `combine(other)`, `corner_min`, `corner_max`, `size`, `center`, `center_axis(axis)`, `area`, `is_out_box(other)`, `is_out_bounds(min,max)`, `is_out_point(point)`, `contains_box(other) -> (inside, overlaps)`, and `contains_bounds(min,max) -> (inside, overlaps)`. The `N=3` specialization has `transform` and `transformed` with `gp`/`NCollection_Mat4`-compatible 4x4 matrices. Empty boxes use the OCCT max/lowest sentinels; invalid box queries follow OCCT's return values.
* `Ray<T,N>` (`BVH_Ray<T,N>`): `new(origin,direction)`, `origin`, `direction`, `inverse_direction`; zero direction components map to positive infinity. Public fields in C++ (`Origin`, `Direct`, `InvDirect`) are observable through these accessors.
* `Tools<T,N>` (`BVH_Tools<T,N>`): `box_box_square_distance(box1,box2)` and `box_box_square_distance_bounds(min1,max1,min2,max2)`; `point_box_square_distance(point,box)` and `point_box_square_distance_bounds(point,min,max)`; `point_box_projection(point,box)` and `point_box_projection_bounds(point,min,max)`; `point_triangle_projection(point,a,b,c) -> Projection` (point, state, first/last feature indices); `point_triangle_square_distance(point,a,b,c)`. `ProjectionState` has `Vertex=0`, `Edge=1`, `Inner=2`.
* `Tree<T,N,Arity>` (`BVH_Tree`): node count/depth, min/max bounds, begin/end primitive, primitive count, level, outer/inner state, child access and mutation, clear/reserve, leaf/inner node insertion overloads for bounds, boxes, or primitive ranges, binary/quad conversion, estimated SAH, and node/min/max buffers. Rust offers checked node access returning `Result`.
* `BuildQueue` (`BVH_BuildQueue`): `new`, `size`, `enqueue`, `fetch(was_busy) -> (work_item, was_busy)`, `has_busy_threads`.
* `Object`, `ObjectSet`, `PrimitiveSet`, `BoxSet`, `IndexedBoxSet`, `Triangulation`, and `Geometry`: object dirty-state/properties, element/box/center/size/swap methods, set sizing/add/clear/build methods, geometry builder/BVH/update methods. Generic data elements are owned by value in Rust.

## Builder and traversal API

* `Builder<T,N>` / `BuilderBase`: `build`, `max_tree_depth`, `leaf_node_size`, `is_parallel`, `set_parallel`, and depth update behavior.
* `QueueBuilder<T,N>`: constructors and `num_threads`, `set_num_threads`; concrete `BinnedBuilder<T,N,Bins>`, `SpatialMedianBuilder<T,N>`, `SweepPlaneBuilder<T,N>`, and `LinearBuilder<T,N>` constructors preserve OCCT defaults and build behavior.
* `Sorter<T,N>` trait (`perform`, `perform_range`, parallel accessors), implemented by `QuickSorter<T,N>` (`new(axis)`) and `RadixSorter<T,N>` (`new(box)`, `perform`, `perform_range`, `encoded_links`). Sort mutations preserve the C++ index ordering rules.
* `Traverse<T,N,Set,Metric>` and `PairTraverse<T,N,Set,Metric>` are object-safe callback traits. User implementations supply `accept` / `accept_pair`; optional overrides mirror `accept_metric`, `reject_node`, `is_metric_better`, `reject_metric`, and `stop`. `select` returns the accepted item count. `Distance` is the single-object traversal helper and exposes `set_object`, `compute_distance`, `is_done`, and `distance`.
* `PairDistance` exposes the pair traversal callback and distance result. `Properties` and `Transform<T,N>` expose transform matrix set/get, inverse, and box application. `DistanceField<T,N>` exposes construction, build, parallel configuration, dimensions, voxel size, extents, packed data, and checked voxel access.
* `BVH::` public helpers: `VectorType<T,N>`, `MatrixType<T,N>`, `ArrayType<T,N>`, `VecComp<T,N>::Get`, `CenterAxis<T,N>::Center`, `SurfaceCalculator<T,N>::Area`, `BoxMinMax<T,N>::CwiseMin/CwiseMax`, `Array<T,N>::Value/ChangeValue/Append/Size/Clear/Reserve`, `IntFloor`, `ToBndBox` overloads (scalar/1D, 2D, 3D, 4D), `MatrixOp<T,4>::Inverse/Multiply`, and `UnitVector<T,2/3/4>::DX/DY/DZ`. `BVH_Constants_MaxTreeDepth`, leaf sizes, bin counts and `THE_NODE_MIN_SIZE` are exported as module constants. `BVH_Constants_*` are exported as module constants.

## Concrete tested signatures

The test exercises `Aabb3::new([f64;3],[f64;3])`, `is_valid`, `area`, `size`, `center`, `center_axis`, `is_out_box`, `is_out_point`, `contains_box`, `add`, `combine`, `clear`, `corner_min`, `corner_max`, and the public `tools` functions `box_box_square_distance`, `point_box_square_distance`, `point_box_projection`, `point_triangle_projection`, and `point_triangle_square_distance`; `Ray3::new` and `inverse_direction` are also checked. The dimension-3 aliases are `Aabb3 = Box<f64,3>`, `Ray3 = Ray<f64,3>`, `Tools3 = Tools<f64,3>`.

## Scope exclusions

`BVH::ToBndBox` is included only as a documented helper and not directly invoked by this oracle because its returned Bnd representation depends on `Bnd_Box`'s tolerance behavior. The oracle pre-fills stack and heap with six distinct poison patterns and keeps only output that is reproducible across runs; all selected operations have fully initialized inputs. No line throws or is marked `unchecked`: the tested methods have no exception-checked preconditions, and no invalid memory indices are invoked.

All other public API items in `cpp/BVH/` are in scope, including `BuildThread`/`BuildTool` and `DistanceField`; these higher-level callback-driven APIs are documented above but are not invoked by this focused numeric oracle.
