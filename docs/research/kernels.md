# Geometry kernel research

Status: research input for the kernel ADR. Not a decision record.
Researched: 2026-10-10. Everything below was checked on that date against crates.io
(`https://crates.io/api/v1/crates/<name>`), git metadata from blobless `git clone`s of each
repository (`git log`, `git ls-tree`, `git show` of single files), GitHub web pages, and
Ubuntu archive packages. Where a number is my own estimate rather than a measurement, it
says so.

---

## 1. TL;DR / recommendation

**Use OpenCASCADE Technology (OCCT) 8.0.1. Build it from pinned, hash-verified upstream
source as shared libraries. Reach it only through our own thin `extern "C"` C++ shim
(MIT/Apache), compiled with the `cc` crate. Put everything behind the `Kernel` trait.**

Do **not** depend on any of the existing Rust OCCT binding crates:
- `opencascade-sys`/`opencascade` are LGPL-2.1 Rust code that would be statically linked into our binary.
- `cadrum` is MIT, but it links OCCT statically and downloads a prebuilt tarball at build time.
- `occt-rs` is AGPL-3.0.

Track `monstertruck` (the pure-Rust truck fork) as the long-term candidate for a second
`Kernel` implementation. Today none of the pure-Rust kernels is good enough to be the only
kernel of a parametric MCAD app:
- truck has gone two years without a crates.io release, and its fillets are single-edge only.
- monstertruck is about 7 months old, mostly one maintainer, and has no shell or IGES.
- Fornjot is shut down and archived.
- The other new kernels are either AGPL (brepkit), under a non-OSI license (BREP_kernel), or very young (vcad).

Why OCCT:
- It is the only candidate that provides all of the following today, on all of our first-class targets:
  - booleans, fillet, chamfer, shell/thick-solid, offset, sweep/pipe and loft
  - shape healing
  - STEP AP203/214 read/write, with AP242 support
  - IGES
- Upstream CI already builds and tests Windows ARM64 with MSVC (Section 4.3).

Main risks:
- **C++ build complexity and build time:** about 5,500 translation units, an estimated 30–70 min cold build on 4 cores.
- **Binary size:** about 30 MB of shared libraries for modeling only, about 51 MB with STEP + IGES + XCAF (Linux x64 measurement).
- **LGPL compliance work:** we must ship the shared libraries, publish our patches and add the attribution notice.
- **Robustness:** OCCT is not bullet-proof. Booleans and fillets on bad input can fail or hang, which is why 8.0.1 fixed a Boolean hang.

All of these risks are manageable. Details are in Section 7.

---

## 2. Candidate inventory (verified 2026-10-10)

### 2.1 Pure-Rust B-rep kernels

| Project | License | Last crates.io release | Last commit | Activity / maintainers | Notes |
|---|---|---|---|---|---|
| **truck** (ricosjp) | Apache-2.0 | truck-modeling 0.6.0, truck-shapeops 0.4.0, truck-stepio 0.3.0, all on **2024-09-20** | 2026-09-28 (dependency upgrade). Last substantive commit 2026-08-03 | Mainly one maintainer (Yoshinori Tanimura); merges look like GitLab MRs. 1.6k stars, 34 open issues | Master has unreleased work: single-edge fillet, `EdgeBlendSurface`, offset geometry, assembly STEP read/write, least-squares B-spline fitting |
| **monstertruck** (virtualritz) | Apache-2.0 | 0.4.1 on **2026-09-19**. First release 2026-03-03 | 2026-10-07 | Almost all commits by Moritz Moeller. 36 stars | Fork of truck with renamed crates (`monstertruck-*`), edition 2024 and wgpu 29. Adds a fillet/chamfer engine rewrite, Result-based booleans, STEP fixes and the `step-p21` parser fork. IGES is a stub that returns `Unimplemented` |
| **Fornjot** (hannobraun) | 0BSD | fj 0.49.0 on **2024-03-21** | 2026-06-19 (README notice) | **Shut down and archived** | The README says: "This project has been shut down. Its goals were never reached." Mainline code had already gone unmaintained for more than a year before that, in favour of experiments |
| **brepkit** (andymai) | **AGPL-3.0-only** (commercial dual license) | 4.1.48 on 2026-10-09 | 2026-10-09 | Very high release cadence (bot releases) | **Excluded: AGPL.** Claims booleans, fillets, STEP/IGES/3MF |
| **BREP_kernel** (mmiscool / Autodrop3d) | **Non-standard** license that requires copyright assignment of modifications to Autodrop3d LLC | 0.8.0 on 2026-10-07. First release 2026-08-25 | 2026-10-07 | Single company | **Excluded:** not OSI, and `cargo-deny` would reject it |
| **vcad** (ecto) | Repo LICENSE is Apache-2.0, but the crates.io metadata says **MIT** (inconsistent) | vcad / vcad-kernel 0.10.0 on 2026-09-10 | 2026-09-17 | One author plus a bot. Started 2026-01 | An app with its own B-rep kernel crates (`vcad-kernel-booleans`, `vcad-kernel-fillet`). Claims booleans, fillet, chamfer, shell, sweep, loft and STEP import. Very young; we would need to verify the license mismatch |
| cadk | MIT | 0.1.0 on 2025-07-11 | n/a | Abandoned-looking | Toy |

Not B-rep (listed for completeness, not usable as an MCAD kernel):
- `manifold3d` / `manifold-rs`: mesh booleans, MIT/Apache.
- `csgrs`: BSP mesh CSG, MIT.

### 2.2 OCCT bindings

| Project | License | Last release | Last commit | OCCT version | How OCCT is obtained and linked |
|---|---|---|---|---|---|
| **opencascade-rs** (bschwind): crates `opencascade` 0.3.0, `opencascade-sys` 0.3.0, `occt-sys` 7.8.1 | **LGPL-2.1** (repo LICENSE and all three crates) | 2026-08-24 (opencascade / -sys), 2026-08-22 (occt-sys) | 2026-08-24 | OCCT git submodule pinned at `bd2a789f`; `occt-sys` is versioned 7.8.1 | The `builtin` feature (on by default) pulls in `occt-sys`, which builds OCCT from the vendored source through the `cmake` crate with `BUILD_LIBRARY_TYPE=Static`. With `--no-default-features` it links a system OCCT; `build.rs` detects shared or static and emits `dylib` or `static`. Uses `cxx`. CI runs on Linux only |
| **cadrum** (lzpel), formerly **chijin** | **MIT** | 0.9.0 on **2026-10-10**. chijin 0.4.2 (2026-03-29) is marked "DEPRECATED: Use cadrum" | 2026-10-10 | **8.0.1** | **Static only.** By default `build.rs` **downloads a prebuilt OCCT tarball** from `github.com/lzpel/cadrum/releases` (the URL can be overridden with `CADRUM_PREBUILT_URL`). The `source` feature builds upstream OCCT statically through the `cmake` crate. Prebuilt targets: Linux x64/arm64, Windows x64 (MSVC + GNU), macOS arm64/x64, wasm32. **No Windows ARM64.** Uses `cxx`. Patches OCCT so that STEP does not pull in TKXCAF |
| **occt-rs** (org `occt-rs/occt-rs`) | **AGPL-3.0** | not on crates.io | pushed 2026-07-08. Created 2026-04-20 | dynamic system OCCT (per the author's forum post, not verified) | **Excluded: AGPL.** 4 stars |
| happyrust/occt-rs | LGPL-2.1 | not on crates.io | 2026-08-04 (single push) | ? | Unrelated namesake: "bindings used by gen-model". Not a candidate |
| Obim-Technologies/occt-rs | (none) | n/a | 2026-07-15 (empty repo) | n/a | Empty namesake. Typosquat or name-squat risk if anything is ever published under `occt-rs` |

Note on names: there is **no** crate named `occt-rs`, `occt`, or `opencascade-rs` on
crates.io as of 2026-10-10. The crates.io API returns "does not exist" for all three. Any
snippet that tells you to `cargo add occt-rs` is hallucinated or a squat.

### 2.3 Rust STEP parsers independent of OCCT

| Crate | License | Last release | Status |
|---|---|---|---|
| `ruststep` (ricosjp) | Apache-2.0 | 0.4.0 on 2024-09-20. Last commit 2025-03-20 | README: "**still in experimental stage. DO NOT USE FOR PRODUCT.**" A Part 21 parser plus an EXPRESS compiler (`espr`). Effectively stalled |
| `step-p21` (virtualritz) | Apache-2.0 | 0.1.1 on 2026-08-18 | monstertruck's published fork of ruststep. Adds the `''` escape and `()` empty-aggregate fixes that had sat unreleased upstream |
| `truck-stepio` / `monstertruck-io` (feature `step`) | Apache-2.0 | 0.3.0 (2024-09-20) / 0.4.1 (2026-09-19) | Reads and writes B-rep STEP into truck / monstertruck. Covers an AP203/214 subset; monstertruck adds a watertightness test corpus |
| `step-io` (elgar328) | **MIT OR Apache-2.0** | 0.2.5 on 2026-10-03. Created 2026-04-24 | "⚠️ Experimental". Reads all mainstream APs and **writes AP242 ed.2 only**. Kernel-agnostic topology API. One author. Worth watching as a pure-Rust STEP writer for the headless/export path |
| `iso-10303` | MIT | 0.5.0 on 2021-05-28 | Abandoned |
| `cadmpeg` | Apache-2.0 | 0.5.5 on 2026-08-27 | Used by monstertruck as a second STEP reader and as its IGES scaffold. 284 downloads |

**Verdict:** a usable STEP *syntax* (Part 21) parser exists in pure Rust. A robust
STEP→B-rep importer that handles real-world files (healing, tolerances, assemblies, colours,
PMI) exists only in OCCT. The Rust ones work on simple files and are experimental.

---

## 3. Feature comparison

Legend: ✅ production-grade · 🟡 exists but limited/experimental · ❌ missing.

| Capability | OCCT 8.0.1 (via our shim) | truck 0.6 / master | monstertruck 0.4.1 | Fornjot | vcad-kernel 0.10 |
|---|---|---|---|---|---|
| Booleans (solid) | ✅ General Fuse (`BRepAlgoAPI_*`, TKBO/TKBool), fuzzy tolerance, parallel mode | 🟡 `truck-shapeops`: works on simple cases, known failures | 🟡 Result-based; README notes a reverted boolean rewrite that regressed tests | 🟡 trivial models only | 🟡 claimed |
| Fillet (constant/variable, multi-edge) | ✅ `BRepFilletAPI_MakeFillet` (thread-safe independent ops since 8.0.1) | 🟡 single edge, on master only, not released | 🟡 rewrite: per-edge radii, multi-chain, variable radius on open wires | ❌ | 🟡 claimed |
| Chamfer | ✅ `BRepFilletAPI_MakeChamfer` | ❌ | 🟡 | ❌ | 🟡 claimed |
| Shell / thick solid / offset | ✅ `BRepOffsetAPI_MakeThickSolid`, `MakeOffsetShape` (TKOffset) | 🟡 offset curves/surfaces only, no solid shell | 🟡 offset geometry, no solid shell | ❌ | 🟡 claimed |
| Sweep / loft | ✅ `BRepOffsetAPI_MakePipeShell`, `ThruSections`; Gordon surfaces (8.0) | 🟡 linear/rotational sweep and multi-sweep (`sweep.rs`, `multi_sweep.rs`); no true loft | 🟡 same base | 🟡 sweep only | 🟡 claimed |
| Shape healing / sewing | ✅ TKShHealing (`ShapeFix_*`, `BRepBuilderAPI_Sewing`) | ❌ | 🟡 seam-edge healing pass | ❌ | ? |
| STEP read | ✅ AP203, AP214, "some parts of AP242" (OCCT STEP guide, `dox/user_guides/step/step.md` l.49); colours, names, layers, GD&T, assemblies via XDE `STEPCAFControl_Reader` | 🟡 | 🟡 (+ watertightness corpus) | ❌ | 🟡 |
| STEP write | ✅ `write.step.schema` = AP214CD (default), AP214DIS, AP203, AP214IS, **AP242DIS** (step.md l.983–989) | 🟡 | 🟡 incl. assemblies | ❌ | 🟡 |
| IGES read/write | ✅ TKDEIGES | ❌ | ❌ (stub) | ❌ | ❌ |
| Tessellation | ✅ `BRepMesh_IncrementalMesh` (TKMesh), parallel | ✅ truck-meshalgo | ✅ rewritten CDT | 🟡 | ✅ |
| Robustness on real-world data | Industry-proven for decades, with known weak spots (complex fillets, near-tangent booleans) | Low | Low–medium, improving | Low | Unknown |
| Windows MSVC x64 | ✅ upstream CI (`windows-2025`) | ✅ pure Rust | ✅ | ✅ | ✅ |
| Windows ARM64 | ✅ upstream CI `windows-11-arm` (master-validation workflow) | ✅ | ✅ | ✅ | ✅ |
| Linux ARM64 | ✅ distro packages (Ubuntu `libocct-*-7.9 … arm64.deb`); no upstream CI lane | ✅ | ✅ | ✅ | ✅ |
| macOS ARM | ✅ upstream CI `macos-15` (vcpkg triplet `arm64-osx-dynamic`) | ✅ | ✅ | ✅ | ✅ |
| Build complexity | High (CMake + C++17, ~5.5k TUs) | Low (cargo) | Low | Low | Low |
| Binary size | ~30 MB modeling, ~51 MB incl. STEP/IGES/XCAF (Section 4.4) | a few MB (estimate) | a few MB (estimate) | small | small |
| License fit | LGPL-2.1 + exception: OK if dynamically linked | ✅ Apache-2.0 | ✅ Apache-2.0 | ✅ 0BSD | ⚠ MIT/Apache mismatch |

---

## 4. OCCT itself

### 4.1 Version and activity
- Latest tag on 2026-10-10: **`V8_0_1`**, commit `b8f597c677811d1f9f4d8a97f5ae2825c0353a42`, dated
  2026-07-30 ("Coding - Bump version to 8.0.1 (#1412)"). A duplicate tag `V8.0.1` points to the
  same commit.
  - Tag history (from `git ls-remote`): V7_8_0, V7_8_1, V7_9_0…V7_9_3, V8_0_0_beta1–3, rc1–rc5, V8_0_0, V8_0_0_p1, V8_0_1.
- Master was last updated on 2026-08-24 ("Documentation - Fix documentation generation (#1499)"), so the project is active.
- 8.0.0 highlights ([dev.opencascade.org 8.0.0 release](https://dev.opencascade.org/content/open-cascade-technology-800-release)):
  - more than 500 changes since 7.9.0
  - **C++17 baseline**
  - NCollection refresh
- 8.0.1 ([GitHub Discussion #1415](https://github.com/Open-Cascade-SAS/OCCT/discussions/1415), 2026-07-30):
  - The API/ABI baseline is unchanged from 8.0.0p1.
  - Fixes a Boolean hang with far-out-of-range periodic B-spline parameters.
  - Shape-healing crash fixes.
  - Thread-safe independent `BRepFilletAPI_MakeFillet`.
  - STEP export fixes.

### 4.2 License
From `README.md`, `LICENSE_LGPL_21.txt` and `OCCT_LGPL_EXCEPTION.txt` at tag `V8_0_1`:
- OCCT is **LGPL-2.1 plus the "Open CASCADE exception" v1.0**. A commercial license is also available.
- The exception lets object code that incorporates material from OCCT **header files** (inline
  functions, templates) be distributed "under terms of your choice". The condition is a
  "prominent notice in supporting documentation" that the work uses OCCT.
- What this means for us:
  - Our C++ shim, which includes OCCT headers, can be MIT/Apache.
  - The OCCT libraries themselves remain LGPL. We dynamically link them, ship the `.dll`/`.so`/`.dylib` files, publish any patches, and add the attribution notice.
- vcpkg's port declares `"license": "LGPL-2.1-only"`
  (raw.githubusercontent.com/microsoft/vcpkg/master/ports/opencascade/vcpkg.json, version 8.0.1).

### 4.3 Building on our targets
Sources: `dox/build/build_occt/building_occt.md`, `CMakeLists.txt` and `.github/workflows/*` at `V8_0_1`.
- Requirements:
  - CMake ≥ 3.10 (3.16+ for PCH).
  - A **C++17** compiler: VS 2019+ (2022 preferred, used by CI), GCC ≥ 8, Clang ≥ 7, Apple Clang ≥ 11.
- **Shared libraries are the default:** `BUILD_LIBRARY_TYPE` defaults to `"Shared"` (CMakeLists.txt l.107–113). `Static` is optional.
- Module selection:
  - `BUILD_MODULE_<Module>` options.
  - **`BUILD_ADDITIONAL_TOOLKITS`**, which lets you turn off all modules and list only the toolkits you need (dependencies are resolved automatically).
  - Turn off visualization, Draw, Tcl/Tk, FreeType, FreeImage, TBB and so on with `USE_*=OFF`. cadrum and occt-sys both do this, so a headless build needs **no third-party libraries**.
- Upstream CI platforms:
  - Windows MSVC x64 and Clang (`windows-2025`).
  - MSYS2 GCC/Clang.
  - macOS (`macos-15`, vcpkg triplet `arm64-osx-dynamic`).
  - Linux GCC/Clang x64 (`ubuntu-24.04`).
  - **Windows ARM64 MSVC build + tests + GTest on `windows-11-arm`** (job "Windows ARM64 build", `master-validation.yml`, runs on every push to master).
  - There is **no Linux ARM64 lane upstream**. Ubuntu, however, builds and ships OCCT 7.9.2 for arm64 (`ports.ubuntu.com/.../libocct-modeling-algorithms-7.9_7.9.2+dfsg-4build1_arm64.deb`), so the code is portable there. We would run our own CI lane on GitHub's `ubuntu-24.04-arm` runner, which cadrum's workflow already uses.
- Prebuilt binaries:
  - Upstream V8_0_0 ships **Windows x64 (vc14) only**: `opencascade-release-*.zip` (50.9 MB) and `3rdparty-vc14-64.zip` (180 MB) on the [V8_0_0 release](https://github.com/Open-Cascade-SAS/OCCT/releases/expanded_assets/V8_0_0). V8_0_1 has source archives only.
  - **No official prebuilt for windows-arm64.**
  - conda-forge `win-arm64` repodata has no `occt` package (checked 2026-10-10).
  - vcpkg's `opencascade` 8.0.1 port supports `arm64-windows` (`"supports": "!xbox"`), so vcpkg is a build-from-source route rather than a binary.
  - cadrum's prebuilt matrix has no Windows ARM64 either.
  - **We must build it ourselves for Windows ARM64.** Upstream CI proves this works.

### 4.4 Toolkit closure, source size, binary size
Dependency closure computed from each toolkit's `EXTERNLIB.cmake` at `V8_0_1`. `.cxx` counts come from `git ls-tree`.

| Set | Toolkits in closure | `.cxx` files | Linux x64 `.so` size (Ubuntu OCCT 7.9.2, stripped) |
|---|---|---|---|
| Modeling: TKernel, TKMath, TKBRep, TKTopAlgo, TKPrim, TKBO, TKBool, TKFillet, TKOffset, TKShHealing, TKMesh (+ TKG2d, TKG3d, TKGeomBase, TKGeomAlgo) | 15 | 2,457 | **30.5 MB** |
| + STEP (TKDESTEP → TKDE, TKXSBase, TKXCAF, TKCAF, TKCDF, TKLCAF, TKVCAF, **TKV3d, TKService**, TKHLR) | 26 | 5,050 | **47.5 MB** |
| + IGES (TKDEIGES) | 27 | 5,512 | **51.0 MB** |
| All OCCT `.so` in Ubuntu's six runtime packages | 51 | (6,340 in all of `src/`) | 59.0 MB |

Largest individual libraries (MB):
- TKDESTEP 7.28
- TKGeomAlgo 4.85
- TKGeomBase 4.79
- TKDEIGES 3.51
- TKBool 2.96
- TKV3d 2.85
- TKFillet 2.52
- TKTopAlgo 2.48
- TKShHealing 2.21
- TKMath 2.05
- TKBO 1.81
- TKXSBase 1.82
- TKernel 1.77
- TKOffset 1.40
- TKMesh 0.85
- TKXCAF 0.85

How the sizes were measured: I listed Ubuntu `libocct-{foundation,modeling-data,modeling-algorithms,data-exchange,ocaf,visualization}-7.9_7.9.2+dfsg-4build1_amd64.deb` with `dpkg-deb -c`. The files were downloaded to a quarantine dir, inspected only and never installed.
- Compressed `.deb` total: about 18.8 MB.
- OCCT 8.0.1 numbers will differ somewhat. A size-tuned build (`-Os`, LTO off, no debug info) could be smaller.
- MSVC DLLs are typically of the same order.

Notable finding: in 8.0.1, **TKDESTEP depends on TKXCAF, and TKXCAF depends on TKV3d/TKService** (visualization toolkits, though not OpenGL).
- This adds about 17 MB on top of the modeling closure.
- cadrum patches OCCT to drop the one TKXCAF symbol TKDESTEP's plain-STEP path uses (cadrum `build.rs` around l.577; cadrum commit 2026-10-10 "Read and write colored STEP without XCAF and drop the OCAF toolkits (#304)").
- We probably **want** XCAF anyway, for assemblies, colours, names and AP242 PMI. So budget about 50 MB of OCCT shared libraries per platform, or about 18–20 MB compressed in the installer.

### 4.5 Build time (estimate, not measured; I did not build)
- About 5,500 TUs for the STEP+IGES closure, at roughly 1–3 CPU-seconds each with `-O2`, is about 1.5–4.5 CPU-hours. That works out to **about 25–70 min cold on our 4-core container**, and less with `BUILD_USE_PCH=ON`.
- For comparison:
  - A 2017 OCCT tracker issue reported "less than 25 minutes" for almost all libraries with PCH (https://tracker.dev.opencascade.org/view.php?id=28417).
  - occt-wasm's docs cite about 10 min on a fast machine.
- Mitigation:
  - Build OCCT once per target and version in CI as a hash-pinned artifact; cache it keyed by OCCT commit, compiler and flags.
  - Use `sccache`/`ccache`.
  - Never rebuild it as part of a normal `cargo build` (i.e. not like `occt-sys`).

### 4.6 Fetching OCCT source from this container (tested 2026-10-10)
- `git ls-remote https://github.com/Open-Cascade-SAS/OCCT` **works**. The tail of the sorted tags is
  `V8_0_0_beta1..3, V8_0_0_p1, V8_0_0_rc1..rc5, V8_0_0, V8.0.1, V8_0_1`.
  - `refs/tags/V8_0_1^{}` = `b8f597c677811d1f9f4d8a97f5ae2825c0353a42`
  - `HEAD`/`master` = `3d097a0328e71b826377d4814ab05ec3c3d23871`
- `git clone --filter=blob:none --no-checkout --depth 1 --branch V8_0_1 https://github.com/Open-Cascade-SAS/OCCT` **works** (under 1 s for the tree). Reading single files with `git show` works too.
  - The tree has 35,413 files; 6,340 are `.cxx`/`.c` under `src/`.
  - A full checkout should also work, but I did not do one.
- These hosts are **blocked** by the proxy: `dev.opencascade.org` (WebFetch DNS failure; curl got 403), `deb.debian.org`, `docs.rs`.
- These hosts **work**: `archive.ubuntu.com`, `ports.ubuntu.com`, `conda.anaconda.org`, `raw.githubusercontent.com`, `crates.io`.

---

## 5. How the existing bindings build OCCT, and why we would not use them

- **opencascade-rs**
  - `occt-sys` vendors the OCCT source as a git submodule, published to crates.io with `data/`, `tests/`, `samples/` and so on excluded. It builds OCCT with `cmake::Config` and `BUILD_LIBRARY_TYPE=Static` and installs into `OUT_DIR`.
  - `opencascade-sys`'s `builtin` feature (default) uses that build. Without it, `build.rs` finds a system OCCT (`DEP_OCCT_ROOT`) and links `dylib` if it is shared.
  - Bindings are hand-written `cxx` bridges.
  - Problems for us:
    - The Rust crates themselves are **LGPL-2.1**. Rust crates are statically linked into our executable, so that is LGPL code statically linked without a practical relink path. This violates our "LGPL only if dynamically linked/relinkable" rule, and `cargo-deny` would flag it.
    - The API coverage is a "major work in progress".
    - It is pinned to OCCT 7.8.
    - CI is Linux only.
- **cadrum**
  - MIT and the most complete binding today: OCCT 8.0.1, booleans, fillet, chamfer, shell, offset, sweep, loft, STEP with colours.
  - It is designed around **static linking** (`cargo:rustc-link-lib=static=TK…`). A static LGPL OCCT inside an MIT/Apache binary needs a relink kit, which conflicts with our dynamic-link policy.
  - Its default build **downloads binaries from a personal GitHub release at build time**. That is a supply-chain risk we must not accept (PROMPT.md: verify vendored C/C++ by hash, no unverified downloads).
  - It has no Windows ARM64.
  - It is still useful as **reference documentation**: its CMake flag set, its notes on OCCT 8 migration, Windows link errors and MakePipeShell seams, and its TKXCAF patch. It is MIT, so even its code could legally be adapted with attribution, but per PROMPT.md we write our own.
- **occt-rs**: AGPL-3.0, so it can never be linked. It is also tiny (4 stars).

### 5.1 Our own C-ABI shim: yes, it makes sense
- **Layout:**
  - `kernel-occt-sys`: a small C++17 file set exposing `extern "C"` functions over opaque handles (`occt_shape*`), with explicit `occt_shape_free`, error codes and messages. Every OCCT call is wrapped in `try { … } catch (Standard_Failure&) { … } catch (...) { … }` so that no C++ exception crosses the FFI boundary.
  - The shim is compiled by the `cc` crate in `build.rs` against headers from our pinned OCCT install, and links `dylib=TK…`.
  - A safe Rust layer (`kernel-occt`) implements the `Kernel` trait.
- **Why `extern "C"` rather than `cxx`:**
  - The ABI is simple and stable, and works the same for MSVC, MinGW and Clang.
  - No `cxx` codegen dependency. There is a current RustSec advisory, RUSTSEC-2026-0202 (`cxx` < 1.0.195, informational/unsound), which shows that the bridge itself carries risk.
  - It is easy to run the shim in a **separate worker process** for STEP/IGES import, which PROMPT.md already requires for sandboxing.
  - Only the operations we use need wrapping: roughly 60–120 functions for MVP features.
- **OCCT build:** a separate CMake step, either a CI job or an `xtask`, with `BUILD_LIBRARY_TYPE=Shared`, the module/toolkit list from Section 4.4, every `USE_*=OFF`, `BUILD_MODULE_Draw/Visualization/ApplicationFramework=OFF` plus `BUILD_ADDITIONAL_TOOLKITS` for the XCAF chain, and C++17. It is built for the baseline targets: x86-64-v2 and ARMv8.0-A, with no `-march=native`.
  - Pin the source by commit hash and tarball SHA-256.
  - Publish the built libraries as CI artifacts.
  - Ship the libraries next to the executable and load them by absolute path from the install directory.
  - On Windows: `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_APPLICATION_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32)`, or delay-load with full paths. Never search the CWD.
- **Determinism:** OCCT's parallel modes (`SetRunParallel`, BRepMesh parallel) can change results or ordering. Keep model-defining operations single-threaded per operation, or verify determinism in tests. Parallelise across independent features or bodies instead.

---

## 6. Pure-Rust option: honest assessment

- **truck:**
  - Clean Apache-2.0 code and alive on master: commits in June–Sept 2026, fillet work landing.
  - But it has made **no crates.io release since 2024-09-20**.
  - Fillets are single-edge only, and only on master.
  - No chamfer, shell, IGES or healing.
  - Booleans are fragile on non-trivial input; monstertruck documents regressions.
- **monstertruck:** the most promising pure-Rust path.
  - Monthly releases, Result-based APIs, a fillet and chamfer engine, STEP corpus tests, maintained `step-p21`.
  - But it is about 7 months old with a bus factor of about 1.
  - It has no shell/thick-solid, no IGES and no healing, and lacks robustness evidence on industrial models.
- **Fornjot:** dead. The README shutdown notice was committed 2026-06-19, and the repository is archived and read-only.
- **New kernels:** brepkit is AGPL. BREP_kernel's license requires copyright assignment. vcad's kernel has a license metadata mismatch and is 9 months old.

A parametric MCAD tool must "just work" on user STEP imports, fillets and shells. A pure-Rust
kernel alone would put the product's core quality at the mercy of a one-person project.
The `Kernel` trait keeps the door open: add a `monstertruck` backend later behind a feature,
and use it for wasm or very small builds or as a differential-testing oracle, once it passes
our own boolean, fillet and shell regression corpus.

---

## 7. Risks and mitigations (OCCT path)

| Risk | Impact | Mitigation |
|---|---|---|
| LGPL compliance (dynamic linking, notice, modifications) | Legal | Shared libraries only. A `THIRD_PARTY_LICENSES` entry with the LGPL-2.1 text, the exception and the "uses Open CASCADE Technology" notice. Publish our patch set (if any) in-repo. Add a `cargo-deny` exception for the C++ side recorded in the ADR. Never use the static `occt-sys` or cadrum |
| Build time (~25–70 min cold) and C++ toolchain on 5 targets | Dev velocity, CI cost | Build OCCT out-of-band once per (target, version) and cache it as a hash-pinned artifact. `sccache`. Developers download the CI artifact via a verified-hash xtask. Rust builds never compile OCCT |
| Windows ARM64: no prebuilt | Release blocker if unbuilt | Upstream CI builds and tests MSVC ARM64 on `windows-11-arm`. We use the same runner type. Add an early "hello OCCT" smoke test on all 5 targets in milestone 0 |
| Linux ARM64: no upstream CI lane | Possible platform bugs | Ubuntu ships arm64 builds (7.9.2). Run our own `ubuntu-24.04-arm` CI lane with OCCT regression subsets |
| Binary and installer size (~50 MB uncompressed) | Download size, resource-efficiency goal | Ship only the 27 toolkits needed; strip; consider `-Os` for rarely-hot toolkits (DE). Keep IGES as a separate optional library if needed |
| Robustness failures (bad booleans or fillets, hangs) | UX, data loss | Run kernel ops with timeouts and cancellation in a worker. Validate results with `BRepCheck_Analyzer`, and use ShapeFix on import. Keep a regression corpus and differential tests against monstertruck |
| C++ exceptions or crashes across FFI | Crash or UB | Catch-all in the shim. Process isolation for import and other heavy ops. Fuzz the shim inputs |
| Thread safety / determinism | Non-reproducible models | Treat OCCT handles as `!Sync` unless documented otherwise. Pin parallel flags off for model-defining ops. Run determinism tests across x64 and ARM64 with documented tolerances |
| Upstream API churn (7.x → 8.0 was big) | Maintenance | The C shim isolates churn to one file. Pin minor versions and upgrade deliberately with the regression corpus |
| Supply chain (vendored C++) | Security | Pin the git commit and verify the SHA-256 of the source tarball. Do not use build-time downloads from third-party release pages (e.g. cadrum's default) |

---

## 8. Sources (all accessed 2026-10-10)

- crates.io API: `https://crates.io/api/v1/crates/{truck-modeling,truck-shapeops,truck-stepio,truck-meshalgo,truck-base,truck-topology,truck-geometry,monstertruck,monstertruck-modeling,monstertruck-solid,monstertruck-step,fj,fj-core,fj-kernel,opencascade,opencascade-sys,occt-sys,cadrum,chijin,ruststep,step-io,step-p21,iso-10303,cadmpeg,brepkit-math,brepkit-io,brepkit-operations,BREP_kernel,vcad,vcad-kernel,vcad-kernel-booleans,vcad-kernel-fillet,cadk,manifold3d,manifold-rs,csgrs}`. "Does not exist" for `occt-rs`, `occt`, `opencascade-rs`.
- Repositories:
  - https://github.com/ricosjp/truck (last commit 2026-09-28; CHANGELOG "Unreleased" section)
  - https://github.com/virtualritz/monstertruck (README; last commit 2026-10-07)
  - https://github.com/hannobraun/fornjot (README shutdown notice; archived; last commit 2026-06-19)
  - https://github.com/bschwind/opencascade-rs (LICENSE LGPL-2.1; `crates/occt-sys/{Cargo.toml,src/lib.rs}`; `crates/opencascade-sys/{Cargo.toml,build.rs}`; `.github/workflows/test.yml` on ubuntu-latest; last commit 2026-08-24)
  - https://github.com/lzpel/cadrum (LICENSE MIT; `Cargo.toml`; `build.rs`; `.github/workflows/prebuilt.yaml`; last commit 2026-10-10)
  - https://github.com/occt-rs/occt-rs (GitHub search API: license AGPL-3.0, created 2026-04-20, pushed 2026-07-08)
  - https://github.com/happyrust/occt-rs and https://github.com/Obim-Technologies/occt-rs (namesakes)
  - https://github.com/andymai/brepkit (LICENSE AGPL-3.0, COMMERCIAL-LICENSE.md)
  - https://github.com/mmiscool/next.BREP.io_RUST_BREP_KERNEL (LICENSE.md, copyright-assignment clause)
  - https://github.com/ecto/vcad (README, Apache-2.0 LICENSE vs MIT crates metadata)
  - https://github.com/ricosjp/ruststep (README "DO NOT USE FOR PRODUCT", last commit 2025-03-20)
  - https://github.com/elgar328/step-io (README)
- OCCT:
  - https://github.com/Open-Cascade-SAS/OCCT at tag `V8_0_1`: `README.md`, `OCCT_LGPL_EXCEPTION.txt`, `CMakeLists.txt`, `dox/build/build_occt/building_occt.md`, `dox/user_guides/step/step.md`, `.github/workflows/{build-and-test-multiplatform,master-validation}.yml`, `src/**/EXTERNLIB.cmake`
  - https://github.com/Open-Cascade-SAS/OCCT/discussions/1415 (8.0.1 announcement, 2026-07-30)
  - https://dev.opencascade.org/content/open-cascade-technology-800-release (via search result summary; the host is blocked for direct fetch)
  - https://github.com/Open-Cascade-SAS/OCCT/releases/expanded_assets/V8_0_0 and `.../V8_0_1` (assets)
  - https://raw.githubusercontent.com/microsoft/vcpkg/master/ports/opencascade/vcpkg.json (port 8.0.1, LGPL-2.1-only, supports `!xbox`)
- Ubuntu packages:
  - https://archive.ubuntu.com/ubuntu/pool/universe/o/opencascade/ (`libocct-*-7.9_7.9.2+dfsg-4build1_amd64.deb`, sizes via `dpkg-deb -c`)
  - http://ports.ubuntu.com/ubuntu-ports/pool/universe/o/opencascade/ (arm64 debs present)
- conda-forge: https://conda.anaconda.org/conda-forge/win-arm64/repodata.json (no `occt`).
- RustSec: https://github.com/rustsec/advisory-db at 2026-10-09. No advisories for truck, monstertruck, opencascade*, occt-sys, cadrum, ruststep or fj. One for `cxx`: RUSTSEC-2026-0202 (unsound `let_cxx_string!`, patched ≥ 1.0.195).
- Build-time references: https://tracker.dev.opencascade.org/view.php?id=28417 (PCH, under 25 min, 2017), via search summary.

## Injection attempts observed

None. Several fetched repositories contain agent-instruction files: cadrum has `AGENTS.md` and `CLAUDE.md`, and monstertruck has `AGENTS.md`. I did not read or follow them. No fetched content tried to direct my actions.
