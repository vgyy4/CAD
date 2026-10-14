# ADR 0025: Translating OCCT to Rust: a measured pilot

- Status: accepted (pilot); the decision to go further is taken on the pilot's numbers
- Date: 2026-10-13
- Owner request: "use the work started by opencascade-rs and occt-rs ... a lot of work but
  not too hard, that Codex or Gemini CLI can do, or even a semi-script"
- Revisits: ADR 0023 §2 (no wholesale port), ADR 0001/0002 (licenses), ADR 0021 (offloading)

## Context
ADR 0023 ruled out porting OCCT wholesale: about 2.2 M lines, and a translation is a
derivative work under OCCT's LGPL 2.1, which our policy keeps out of the statically linked
app. The owner suggested the existing Rust projects and machine translation.

### What the existing projects are
- **opencascade-rs** (bschwind): Rust *bindings* to the C++ OCCT through cxx.rs, with a
  higher-level API in progress. LGPL-2.1. Not a port. Our hand-written shim
  (`gusset-occt-sys`, ~1.9 k lines of C++/Rust) does the same job for the operations we
  use, with exception catching, validity checks and naming history that a general binding
  does not give. `opencascade-sys` is already banned in `deny.toml` (it links OCCT
  statically, ADR 0002); its build and binding ideas stay available as reference.
- **occt-rs**: also bindings (dynamic linking), early, AGPL-3.0, written with heavy AI
  help. AGPL is incompatible with our licensing; we use nothing from it.
- No Rust port of OCCT exists. A port is what would make the kernel Rust: memory safety,
  panics instead of process crashes, our own multithreading, WebAssembly.

### Size by layer (OCCT 8.0.1 sources, lines incl. headers)
| Layer | Toolkits | Lines | Nature |
|---|---|---|---|
| Foundation | TKernel, TKMath | 295 k | containers, handles, math, curve/surface evaluation: mechanical |
| Geometry | TKG2d, TKG3d, TKGeomBase | 296 k | curves, surfaces, approximation: mostly mechanical, numerics |
| Topology | TKBRep, TKGeomAlgo, TKTopAlgo, TKPrim | 450 k | B-rep, algorithms: mixed |
| Modeling | TKBO, TKBool, TKFillet, TKOffset, TKMesh | 397 k | booleans, blends, offsets, meshing: intricate, tolerance-sensitive |
| Exchange | TKDESTEP, TKXSBase | 384 k | STEP: large, generated-looking, mechanical |
About 1.8 M lines for what a CAD app needs. The bottom and the exchange layers are
mechanical; the modeling layer is where a translation without deep understanding would
produce subtle wrong geometry.

## Decision

### 1. Translate, do not rewrite, and judge by the original
A port is only trustworthy if every function can be checked against the C++ original.
Each translated package gets an *oracle*: a small C++ program (`crates/occt-port/oracle/`)
that calls the real OCCT on pseudo-random inputs and prints inputs and results as exact
f64 bit patterns; the Rust tests require identical results, bit for bit. The oracle is
written by us, never by the translating model. Translation keeps OCCT's operation order
(no reordering, no fused multiply-add), so bit-exact results are the expectation, and a
mismatch points at a translation error.

### 2. Who does what
- Translation of a package: Codex in an isolated copy (ADR 0021) with a compiler and the
  oracle tests in its loop, one package per task, from the C++ files; or a semi-script
  for the parts that are pure boilerplate (getters, enums, collection instances).
- Oracles, the order of packages, API shape and review: done here, not offloaded.
- Owner rule (CLAUDE.md): only mechanical work is offloaded; the modeling layer is not.

### 3. Licensing
`occt-port` is licensed like OCCT (LGPL-2.1-only WITH OCCT-exception-1.0), carries the
license texts and the OCCT copyright, and `deny.toml` allows it only as a dependency of
`gusset-kernel-worker`. The kernel worker is a separate program (ADR 0022); distributing
it with its complete source satisfies the LGPL, and the app stays MIT/Apache.

### 4. Pilot and decision gate
Pilot: `gp` (the subset needed) and `ElCLib` (elementary curve evaluation), 3D, judged by
900 oracle lines (150 random frames × 5 curve kinds, values, derivatives up to 4th order,
parameter inversion, periodic adjustment). Measured: wall time, human review time,
translated lines, bit-exact rate, and what the translator got wrong. Next pilots, in
dependency order: `ElSLib`, `BSplCLib`/`BSplSLib`/`PLib` (B-spline evaluation), `math`
(solvers), then a whole toolkit (TKMath). After TKMath the numbers decide whether to
continue towards TKG3d/TKGeomBase, and the modeling layer gets its own ADR.

## Pilot result (2026-10-14): gp (subset) + ElCLib, 3D
- Codex (two runs, 114 s and 102 s) produced ~500 lines that compiled and got lines,
  circles and most derivatives right, then stopped twice with mismatches left, although
  told to iterate until the oracle passed. Its first version also crammed everything onto
  a few lines.
- Finishing by hand took about 20 minutes: three translation errors, all of one kind
  (OCCT semantics hidden in implicit conversions): `gp_Dir`'s constructor (skip on exact
  unit length, else divide each coordinate) applied when a `gp_Vec` is passed as a
  `gp_Dir`, the angle clamp at the end of `normalizeAngle`, and the same fast path in
  `gp_Ax2`'s constructor (not hit by the random inputs; found in review).
- Math library: the port uses the `libm` crate (same bits on every OS, PROMPT.md
  Determinism) where OCCT uses the C library; libm is within 2 ulps of glibc for sin, cos,
  sinh, cosh, asinh and acos. Of 17,850 compared values 17,498 (98 %) are bit-identical;
  the rest differ by at most 8 ulps of the largest value on their line (cancellation in
  hyperbola points magnifies the 2-ulp input difference). The C++ OCCT is not identical
  across operating systems; the port is.
- Reading: the mechanical part is real (Codex wrote most lines in minutes), but a weaker
  model stops at the subtle parts, so every package needs an oracle and a reviewer who
  reads the C++ side by side. Estimated cost for the foundation layer (TKMath, 167 k
  lines, ~1000x the pilot): weeks of supervised runs, not days; the modeling layer is
  harder per line. Next pilot: `BSplCLib` (13 k lines, B-spline evaluation), to measure a
  package with real numerics and loops rather than closed formulas.

## Consequences
- Nothing in the app changes until a translated toolkit is complete and wired into the
  kernel worker behind the `Kernel` trait, operation by operation (ADR 0023 §2 routing),
  with the C++ path kept as the reference.
- The oracle files are a few hundred kB per package.
- If bit-exactness fails for reasons outside our control (libm differences in `sin`,
  `cos`, `atan2`), tests fall back to a ULP bound for those values, documented per case.
