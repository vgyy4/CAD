# ADR 0002: Geometry kernel — OCCT 8.0.1 through our own C shim, behind a `Kernel` trait

- Status: accepted
- Date: 2026-10-10
- Research: `docs/research/kernels.md`

## Requirements
Booleans, fillets, chamfers, shell, sweep/loft, robust STEP (AP203/214/242) and IGES
I/O, and operation history (for persistent naming, ADR 0003). It must build and pass
tests natively on Windows x64/ARM64 and Linux x64/ARM64 (hard criterion), and macOS ARM
best effort. It must be license-compatible (ADR 0001) and swappable.

## Options (status checked 2026-10-10)
| Option | License | Capability | Platforms | Verdict |
|---|---|---|---|---|
| truck | Apache-2.0 | booleans; fillet single-edge on master only; no chamfer/shell/IGES/healing; no crates.io release since 2024-09 | pure Rust | not sufficient |
| monstertruck | Apache-2.0 | promising fillet/chamfer rewrite, STEP; no shell; ~7 months old, one maintainer | pure Rust | watch; candidate second backend |
| Fornjot | 0BSD | project shut down (archived 2026-06) | — | no |
| brepkit | AGPL | — | — | banned |
| opencascade-rs | **LGPL Rust crates** (statically linked) + OCCT 7.8 | good | Linux-only CI | license-incompatible as a dependency |
| cadrum (chijin) | MIT, OCCT 8.0.1 **static**, downloads a prebuilt from the author's releases | good | no Windows ARM64 | static LGPL + supply-chain risk → no |
| occt-rs/occt-rs | AGPL-3.0 | — | — | banned |
| **OCCT 8.0.1 + our own C shim** | LGPL-2.1 + OCC exception (dynamic) | full: booleans, fillet, chamfer, shell, sweep/loft, healing, STEP/IGES/XDE, history | upstream CI includes Windows ARM64 MSVC; Linux ARM64 via distro builds | **chosen** |

## Decision
1. **`gusset-kernel`** defines a kernel-agnostic `Kernel` trait and value types (planes,
   2D profiles, shape handles, topology listings with geometric keys, operation
   history, meshes, mass properties, structured `KernelError`s). Nothing above this
   crate sees OCCT types.
2. **`gusset-occt-sys`** contains our own small C++ shim (`extern "C"` functions only,
   MIT/Apache, compiled by the `cc` crate). It catches *every* C++ exception
   (`Standard_Failure` and `...`) at the boundary and converts it to an error code
   plus message. Shapes cross the boundary as opaque handles with explicit free
   functions. This is an FFI crate: `unsafe` is allowed only here, and every block
   carries a `// SAFETY:` comment.
3. **`gusset-kernel-occt`** implements the `Kernel` trait on the shim, in safe Rust.
4. **OCCT is built from the upstream tag `V8_0_1`** (commit
   `b8f597c677811d1f9f4d8a97f5ae2825c0353a42`), verified by commit hash (and a source
   tarball SHA-256 in CI), as **shared libraries**, with the Draw module off and the
   other modules restricted to what we use. CI builds it once per target and caches it
   keyed by the hash and build-flags file (`deps/occt/`). Release bundles ship the
   unmodified `.so`/`.dll`/`.dylib` next to the app with `NOTICE-OCCT.md`, the LGPL
   text and the build recipe, so users can relink.
5. **Determinism:** OCCT's parallel modes (`SetRunParallel`) are off for model-defining
   operations. Tessellation for display may run in parallel.
6. **Robustness:**
   - Every kernel result is validated (`BRepCheck_Analyzer`) before it is accepted, so
     an invalid solid becomes a feature error, not a silent bad model.
   - Long operations run on worker threads with cancellation (OCCT `Message_ProgressIndicator`).
   - Imports (STEP/IGES) run out of process (ADR 0008), because a crashing parser must
     not take the app down.
7. **A `MockKernel`** (pure Rust, boxes and simple prisms with exact history) lives in
   `gusset-kernel` for unit tests of naming, evaluation and the document layer. The whole
   workspace except the OCCT crates builds and tests without a C++ toolchain.
8. **Second backend later:** once monstertruck (or a successor) covers fillet/chamfer/shell
   robustly, a pure-Rust `Kernel` implementation can be added behind a cargo feature and
   run against the same golden test suite.

## Consequences
- A C++ toolchain is needed for full builds (MSVC on Windows, gcc/clang on Linux,
  clang on macOS). The cold OCCT build is long, so CI caching is mandatory.
- Binary size grows by about 30–50 MB (OCCT modeling + data exchange). This is recorded
  in `docs/PERFORMANCE.md` as part of the binary-size budget.
- LGPL obligations are handled by the release workflow (notice, source link, recipe).
