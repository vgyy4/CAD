# SolidCore, second look (2026-11-07)

Status: research input for ADR 0023 (amendment of 2026-11-07). Not a decision record.
Checked on 2026-11-07 against SolidCore at `f4f70127afee` (main, 2026-11-07 11:06 UTC),
cloned read-only into a quarantine directory outside the repository. Numbers marked
"measured" come from the probes described in §3; everything else is read from SolidCore's
repository, its issue tracker (GitHub search API) or crates.io.

## 1. Summary

- **It now works end to end.** On 2026-10-13 SolidCore was a third ported with nothing usable
  end to end. Today every one of its 261 SMLib units is ported, and an operation layer
  (`solidcore-ops`) and a mesher (`solidcore-tess`) give primitives, extrude/revolve/sweep/loft,
  booleans, fillets and chamfers (including variable and G2 rounds, setbacks), shell, offset,
  draft, thicken, stitch, mass properties, validity checks and tessellation. STEP import and
  export exist as separate crates.
- **Correct on every case we tried.** All 23 probe cases (box, holes, slot, revolves,
  booleans with coplanar faces, fillets incl. the fully rounded block, chamfer, hole-rim
  round) give the analytic volume to 1e-11 or better and pass SolidCore's own validity check
  (measured). OCCT gives the same volumes.
- **Slower than OCCT on every case but one: up to 57x per operation** (median about 9x; box
  minus box is level), and 36x to 46x on many-tool booleans (measured, §3). There is no operation where SolidCore is faster, so
  ADR 0023's per-operation routing has nothing to route for speed today.
- **The adapter exists, opt-in.** `kernels/solidcore` implements the `Kernel`
  trait for extrude, revolve, boolean (one and many tools), fillet, chamfer, shell,
  transform, topology, mass properties, bbox and tessellation; 12 conformance tests (a subset
  of the OCCT goldens) pass. It is behind the crate feature `solidcore`, because SolidCore is
  ~740k lines (an 18 min release build on 4 cores) and needs a 200 MB git checkout.
- **Recommendation:** route nothing to SolidCore yet; keep the adapter as a conformance and
  benchmark target, re-measure monthly, and prepare two uses where it can win without being
  faster: a fallback when OCCT refuses an operation, and the kernel of a future wasm build.

## 2. State of SolidCore

### Project
- Moved from `bherbruck/solidcore` to `tangency-labs/solidcore` (the old URL redirects).
  Apache-2.0, with a NOTICE naming NVIDIA's USD BRep (SMLib/SM_API v1.0.1) as the upstream
  work. 74 commit authors since 2026-10-13, 13,849 commits (2,442 merges on main) in those
  25 days, most PRs written by AI agents under maintainer review (AGENTS.md).
- `solidcore` 1.0.1 (the kernel crate alone) is on crates.io since 2026-10-30. `solidcore-ops`,
  `solidcore-tess` and `solidcore-convert` are `publish = false`; `solidcore-record` is
  unpublished. API is declared experimental (0.0.x / 0.1.0).
- Size: ~740k non-empty lines in `crates/solidcore/src` (was ~303k), 3,639 kernel tests,
  374 tests in ops and tess, plus the goldens below.
- Layering (docs/layers.md, owner decisions of 2026-11-02): kernel (`solidcore`), record
  (plain data B-rep, serde), convert, ops (the modelling API applications call) and tess
  (the one mesher, SolidCraft's). Applications are meant to call only ops and tess. A
  refactor that moves SolidCraft's geometry into these layers is under way.

### What works end to end (ops level)
| Area | State (2026-11-07) |
|---|---|
| Primitives | box, cylinder, sphere, cone, torus (`make_*`), all measured valid |
| Profiles | planar regions with holes (lines, arcs, Bézier, conics) extruded (`prism::extrude_planar_regions`), drafted extrusion, revolve, sweep along paths (exact for lines/arcs, curve sweep otherwise), screw sweep (threads), loft and loft-to-point |
| Booleans | union/subtract/intersect, `union_all`, 2D region booleans, imprint |
| Blends | round (rolling ball), curvature-continuous (G2) round, chord round, chamfer (equal, distance-angle), variable rounds, setbacks, open corners; tangent-chain selection |
| Offset family | shell, offset faces, offset sheet, thicken, draft |
| Other | stitch, split, untrim, extend, body transform, copy |
| Queries | mass properties (volume, area, centroid, inertia; integrator estimate, not a bound), bounds, point classification, distances, normals, validity check (SMLib's `SmBrep::Test` levels) |
| Tessellation | per-face (u, v) planner, CPU or GPU evaluation of the plan, edge polylines, display levels of detail |
| STEP | `step-import` (AP203/AP214 B-rep subset, clean-room reader), `step-export`, `step-brep`; AP242 editions are an open issue (#2937) |
| Native format | `.smb` (SMLib binary/ASCII) read and write |
| wasm | builds for `wasm32-unknown-unknown`; CI compares native and wasm digests of every golden case |
| Multithreading | none in the kernel; ops runs one process-global kernel under a mutex, so calls are serialised (measured: 4 threads take as long as 1); tess uses rayon |
| Determinism | no clocks, address hashing or hash-map order; same bytes for repeated runs (measured, `.smb` identical). The kernel calls the platform's `sin`/`cos`/`atan2` (408 call sites), so bit-identical results across platforms are not guaranteed; goldens compare at 1e-9 relative |
| Faces history | `solidcore-record` defines exact face/edge history types and ops has a provenance hook (`provenance.rs`), but `attach`/`read` are crate-private and no op returns history |

### Configuration that matters
`UpstreamFixes`: 311 opt-in fixes of SMLib defects, all **off** by default (pinned C++
behaviour). SolidCraft, the ops tests, BRepBench and this adapter run with all of them on;
the docs warn that a few "are known not to combine cleanly or to be incomplete".

### How it is verified (the difftest is gone)
- Until 2026-11-06 a difftest harness built the pinned C++ SMLib and compared every ported
  unit's outputs with Rust to 1e-9 (snapshot of 2026-10-23: 554,873 cases, 0 failing, 4,186
  deferred, 7 known mismatches).
- On 2026-11-06 (PR #2816, "Kernel tests become goldens: no C++") it was replaced by
  **goldens**: each unit's cases (upstream tests re-expressed as data, upstream TestFiles,
  seeded generators) checked against outputs recorded from the Rust itself, 284 units,
  168 MB of expected JSONL. The C++ is now a reading reference only. The reason is that
  SolidCore now departs from SMLib on purpose (exact surface extensions, a new generic
  rolling-ball blend). A golden proves a result unchanged, not correct; correctness is
  argued with closed forms, certificates and validity checks. So "matched to the C++ to 1e-9"
  no longer describes the project.
- Correctness reports: an SSI corpus, SolidCraft's operation cases, a STEP corpus (NIST),
  and BRepBench (241 neutral cases with closed-form answers). Its own tracking issue
  (#3034, run of 2026-11-06 at `8d74f1f`) reports 208 of 231 run cases passing, level with
  cad-kernel, ahead of cadrs (204), OCCT (201) and next.BREP (195). That is the project's
  own report of a third-party suite; we have not reproduced it.

### Robustness: open issues
217 open issues (2026-11-07); 107 mention fillets, 61 booleans, 9 panics. Representative:
- wrong results reported as success: #2927 (a subtraction on an imported revolved part
  returns empty, unchanged or larger bodies with `Ok`), #3029 (two balls overlapping by 1e-6
  intersect to nothing), #3030 (a draft that consumes a face returns a wrong open body),
  #3087, #2956 (success with gaps beyond tolerance);
- refusals at small gaps: #3012 (cut between filleted boxes 1e-4 apart), #3011 (a dome arc
  1e-8 off its circle);
- state left behind: #3082 (a failed fillet can leave the body changed and invalid);
- regressions between commits: #2905, #2906, #2907 (fillets that built at one commit fail at
  the next; ten refusals now take over 90 s), #3048;
- performance outliers: #3078 (a variable round takes about a minute), #2964 (a G2 round runs
  20 min then fails), #2866 (600 s on a mixed rational surface), #2756 (cancel tokens
  ignored).

## 3. Measurements

### Method
- SolidCore: a throwaway probe calling `solidcore-ops` / `solidcore-tess` directly, release
  build (Rust 1.98, `opt-level=3`), every `UpstreamFixes` flag on. Volumes from
  `Body::volume(1e-9)`, validity from `Body::check(Geometry)` and `is_manifold_solid`.
- OCCT: the same cases through Gusset's `OcctKernel` (OCCT 8, the pinned build), release.
  OCCT times include our adapter's validity check and history; SolidCore times include ops'
  result contract but not the validity check (run separately).
- One process per backend, each case once, single-threaded; a shared 4-core machine with
  other builds running (load 5-7 for the per-operation rows, 4-13 for the hole grid), so
  treat ratios, not absolute times, as the result. Tessellation parameters differ (SolidCore:
  chord only; OCCT: chord plus 0.35 rad), so mesh rows compare work done, not equal output.

### Per operation (measured)
| Case | SolidCore ms | OCCT ms | SC / OCCT | Volume rel. error SC / OCCT | F/E/V SC | F/E/V OCCT |
|---|---:|---:|---:|---|---|---|
| extrude box 40x20x10 | 9.7 | 1.4 | 7x | 0 / 2e-16 | 6/12/8 | 6/12/8 |
| `make_box` 40x20x10 (primitive) | 0.8 | - | - | 0 | 6/12/8 | - |
| extrude 40x40 plate, r5 hole, symmetric | 39.8 | 0.7 | 57x | 2e-13 / 2e-16 | 8/18/12 | 7/15/10 |
| extrude slot (lines + arcs) | 32.2 | 0.6 | 51x | 4e-14 / 0 | 6/12/8 | 6/12/8 |
| revolve rectangle, full turn | 26.4 | 0.9 | 28x | 6e-13 / 0 | 3/3/2 | 3/3/2 |
| revolve rectangle, half turn | 16.7 | 1.2 | 14x | 3e-13 / 1e-16 | 5/9/6 | 5/9/6 |
| revolve washer (off axis) | 60.7 | 1.2 | 49x | 1e-12 / 1e-16 | 4/6/4 | 4/6/4 |
| box - box (coplanar top and bottom) | 9.9 | 10.1 | 1x | 1e-16 / 4e-16 | 10/24/16 | 10/24/16 |
| box - through cylinder r5 | 66.6 | 6.5 | 10x | 1e-14 / 3e-16 | 7/15/10 | 7/15/10 |
| box - flush cylinder (coplanar caps) | 14.2 | 4.5 | 3x | 1e-14 / 3e-16 | 7/15/10 | 7/15/10 |
| box ∪ cylinder on an end face | 52.3 | 5.9 | 9x | 2e-13 / 0 | 10/21/14 | 10/21/14 |
| sphere r10 ∩ box (half sphere) | 44.4 | 5.8 | 8x | 7e-12 / 4e-16 | 2/3/3 | 2/3/2 |
| fillet 1 edge r2 | 21.6 | 10.7 | 2x | 3e-13 / 0 | 7/15/10 | 7/15/10 |
| chamfer 1 edge 1 | 9.1 | 3.3 | 3x | 0 / 0 | 7/15/10 | 7/15/10 |
| fillet all 12 edges r1 (rounded block) | 217 | 39 | 6x | 1e-12 / 1e-16 | 26/48/24 | 26/56/24 |
| fillet all 12 edges r2 | 217 | 25 | 9x | 2e-12 / 1e-16 | 26/48/24 | 26/56/24 |
| fillet all 12 edges r4 | 219 | 27 | 8x | 3e-12 / 1e-16 | 26/48/24 | 26/56/24 |
| fillet hole rim r1 | 73.1 | 2.0 | 37x | 1e-12 / 0 | 8/17/11 | 8/17/11 |

The cylinder in the SolidCore boolean rows is `make_cylinder` (one side face); a cylinder
extruded from a circle profile has two half faces in SolidCore (the 8/18/12 plate), so face
and edge counts differ from OCCT's for the same feature.

### Many tools: n x n grid of 3 mm holes in a 5 mm plate (measured)
| Holes | SolidCore: tools united, one cut | OCCT: `boolean_many` | ratio | SolidCore: one cut per hole | OCCT: one cut per hole |
|---:|---:|---:|---:|---:|---:|
| 25 | 1.6 s | 0.045 s | 36x | 3.9 s | 0.22 s |
| 100 | 7.6 s | 0.19 s | 40x | 51 s | 2.8 s |
| 400 | 48 s | 1.05 s | 46x | - | - |

Volumes agree to 5e-12 in every row; every SolidCore result passes its validity check.

### Tessellation (measured)
| Body, chord | SolidCore tris, ms | OCCT tris, ms |
|---|---|---|
| sphere r10, 0.1 | 960, 16 | 972, 3.9 |
| sphere r10, 0.01 | 16,128, 152 | 10,108, 71 |
| rounded block r2, 0.1 | 1,708, 7.1 | 1,060, 5.3 |
| rounded block r2, 0.01 | 6,220, 29 | 2,612, 9.9 |

SolidCore's mesher grids planar faces too (124 triangles for a plain box at our default
tolerance, OCCT 12) and refused no face in any case. Mesh volumes converge to the exact
ones (sphere at chord 0.01: 4184.5 against 4188.8).

### Other observations (measured)
- **Oversized rounds.** A round of 21 or 50 on a 10 mm edge between faces 40 and 20 wide:
  OCCT refuses; SolidCore returns a valid solid in which the round has run over the 20 mm
  face (6 or 5 faces). Up to r = 20 both give the closed form. A router must treat this as a
  policy difference (Gusset reports such a fillet as too large).
- **Determinism.** Two runs of the same box-minus-cylinder and rounded block give
  byte-identical `.smb` files.
- **Threads.** 8 box-minus-cylinder builds: 283 ms serially, 291 ms on 4 threads (ops holds
  one kernel lock). Parallelism would need one SolidCore per worker process (ADR 0022).
- **Build.** Release build of the kernel crate: 18 min on 4 shared cores; debug test build of
  the adapter (dependencies at opt-level 2): 11 min. The git checkout is ~200 MB.

## 4. The adapter (`kernels/solidcore`)
- Depends on `solidcore`, `solidcore-ops`, `solidcore-record` and `solidcore-tess`, all pinned
  to `f4f70127afee1640c43d9f61e035baddd6dc271b` in `[workspace.dependencies]` (git, because
  three of them are not on crates.io); `deny.toml` allows that one git source. Apache-2.0 is
  on our allow-list. The dependency is optional (feature `solidcore`), so default builds,
  `THIRD_PARTY_LICENSES.md` and the shipped programs are unchanged; shipping it later means
  adding SolidCore's NOTICE (NVIDIA attribution) to the generated licence file.
- Operations: extrude, revolve, boolean, boolean_many (cut: tools united then one cut, 2x
  faster than one cut per tool), fillet, chamfer, shell, transform, topology, mass
  properties, bbox, tessellate. Everything else returns the trait's `Unsupported` error
  (sweep, loft, draft, offset, thicken, stitch, STEP, native blobs, compound).
- Face origins are recovered geometrically, because ops returns no history: a result face
  whose interior point lies on an input face of the same surface kind gets `Input`;
  unmatched blend faces get `FromEdge` of the nearest blended edge (corner patches too, never
  `FromVertex`); unmatched shell faces get `Offset` of the nearest input face; extrude and
  revolve faces are matched to the profile segment they were swept from (`Lateral`) or to the
  cap planes (`Cap`). This works for the golden cases but is a heuristic: coincident faces of
  several inputs get all of them, and a face that lies on an input face it does not come from
  (rare) is misattributed.
- Per-face area, centroid and normal come from a fine mesh (chord 1/5000 of the body):
  exact for planes, within the chord for curved faces. Edge lengths are exact for lines and
  circles, from the mesh polyline otherwise.
- Tests (`src/tests.rs`, run with `(cd kernels/solidcore && cargo test --features
  solidcore`): extrude box/hole/slot goldens with lateral tags and caps, revolve with caps and
  seams, boolean cut with history, box minus cylinder and union, boolean_many with per-tool
  history, fillet/chamfer with `FromEdge` history, the rounded block's closed form with 20
  blend faces, tessellation, shell and transforms, determinism, unsupported operations.
  Differences from the OCCT goldens are written into the tests (split circle faces, gridded
  planar meshes, oversized round accepted).

## 5. What could be routed now
| Operation | Route to SolidCore? | Why |
|---|---|---|
| any, for speed | no | 1x-57x slower (median 9x) on the measured cases |
| booleans with many tools | no | 36x-46x slower |
| fillet/chamfer/shell as fallback when OCCT refuses | not yet; worth building | SolidCore's blend is new code (rolling-ball generic blend, overflow handling) and its BRepBench score is level with or above OCCT's; but it needs B-rep exchange OCCT → SolidCore (STEP or a native schema, not there yet) and a naming story for the result |
| everything, in a wasm/web build | candidate | pure safe Rust, builds for wasm32, no C++ or LGPL; OCCT has no wasm path in Gusset |

## 6. Risks
- **Robustness.** Correct on our simple cases, but the tracker shows wrong results returned
  as success, regressions between commits and minute-long operations. Any routed result
  needs our own validity check and, for fallbacks, a volume sanity check.
- **Naming and face origins.** No history from ops; our geometric recovery is a heuristic.
  Topology also differs from OCCT for the same feature (a circle profile gives two faces,
  a rounded block has 48 edges not 56), so names made on one backend do not carry to the
  other: a model must stay on one backend per body, or naming must key on geometry, not
  indices. Exposing the provenance hook upstream would fix the first part (an outward
  action: owner approval, ADR 0023).
- **Tolerance model.** SMLib's per-entity tolerances with SolidCore's certificates (each
  surface carries its definition and a certified approximation); 311 fix flags whose default
  is the old behaviour; mass properties are estimates at a requested accuracy, not bounds.
- **Determinism.** Same bytes in one process and platform; cross-platform equality is not a
  goal there (platform libm), while Gusset requires it for model-defining math.
- **Churn and build cost.** Hundreds of merges a day, unpublished crates, an ongoing layer
  refactor: the pin must be moved deliberately and the adapter re-run each time. 740k lines
  and a 200 MB git dependency are why the adapter is opt-in.
- **Process-global state.** One kernel and one fix set per process; no parallelism inside a
  process.
- **Licence.** Apache-2.0 with a NOTICE that must ship with binaries; compatible with Gusset's
  MIT OR Apache-2.0. No copyleft. SolidCore's code is not copied into our repository.

## 7. Recommendation
1. Route no operation to SolidCore today.
2. Keep `gusset-kernel-solidcore` opt-in, pinned, and in the monthly tracking: move the pin,
   re-run its tests and the two probes, update the tables here.
3. Next steps that would let it earn a route: (a) a neutral B-rep exchange OCCT → SolidCore
   so a fallback-on-refusal route for fillet, chamfer and shell can be measured on the
   dogfooding models where OCCT refuses; (b) ask upstream (owner approval) to expose the
   provenance hook or return face history from ops; (c) when Gusset gets a web build,
   evaluate SolidCore as its only kernel.
4. Revisit speed when SolidCore publishes performance work; today its tracker treats speed as
   secondary to correctness (BRepBench, exactness).
