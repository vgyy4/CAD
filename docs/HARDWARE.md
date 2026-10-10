# Hardware support

What each CPU and GPU generation gets from Gusset, and why. This is both user
documentation and the internal record of dispatch and quality-tier decisions
(ADR 0011, `docs/adr/0011-hardware-tiers-and-dispatch.md`).

**How to read this document**

- Sections marked *generated* are produced from the registry in
  `crates/gusset-hw/src/registry.rs` (and `budgets.rs`) by
  `cargo run -p gusset-hw --bin gen-hardware-doc`. Do not edit them by hand: the test
  `hardware_doc_is_up_to_date` fails in CI when they drift from the code.
- Everything else is hand-written. **Every hardware fact carries a source** (reference
  links like [ark-6700] resolve to the [Sources](#sources) list at the end).
- **"unverified"** means we have not confirmed the fact from a primary source. Do not
  rely on it, and do not copy it elsewhere without verifying it first.
- Verification method (research pass of 2026-10-10): this environment cannot open
  vendor pages directly, so "verified" means a web-search excerpt *of the cited official
  page* stated the fact. Re-check against the live page when you edit a row.

## Contents

1. [What the app does with each CPU feature](#what-the-app-does-with-each-cpu-feature) (generated)
2. [CPU matrix](#cpu-matrix)
3. [GPU quality tiers](#gpu-quality-tiers) (generated)
4. [GPU matrix](#gpu-matrix)
5. [Resource budgets](#resource-budgets) (generated)
6. [Known issues](#known-issues)
7. [System requirements](#system-requirements)
8. [Sources](#sources)

## What the app does with each CPU feature

Build baselines (ADR 0011): **x86-64-v2** on x64 (SSE4.2, POPCNT, SSSE3, CMPXCHG16B,
LAHF/SAHF; unverified against the psABI text in this session [psabi]) and
**ARMv8.0-A with NEON** on ARM64 (the default for Rust's `aarch64` targets). Everything
above the baseline is used only through runtime dispatch.

### Detected features (generated)

<!-- BEGIN GENERATED: cpu-features -->
| Feature | Arch | Detector name | Guaranteed by build baseline | Required by dispatch levels |
|---|---|---|---|---|
| SSE4.2 | x86_64 | `sse4.2` | yes | `sse42`, `avx2fma`, `avx512` |
| POPCNT | x86_64 | `popcnt` | yes | `sse42`, `avx2fma`, `avx512` |
| AVX | x86_64 | `avx` | no | `avx2fma`, `avx512` |
| AVX2 | x86_64 | `avx2` | no | `avx2fma`, `avx512` |
| FMA3 | x86_64 | `fma` | no | `avx2fma`, `avx512` |
| AVX-512F | x86_64 | `avx512f` | no | `avx512` |
| AVX-512BW | x86_64 | `avx512bw` | no | `avx512` |
| AVX-512VL | x86_64 | `avx512vl` | no | `avx512` |
| AVX-VNNI | x86_64 | `avxvnni` | no | detected and reported only |
| NEON (Advanced SIMD) | aarch64 | `neon` | yes | `neon`, `neon-dotprod`, `sve` |
| DotProd | aarch64 | `dotprod` | no | `neon-dotprod` |
| I8MM | aarch64 | `i8mm` | no | detected and reported only |
| SVE | aarch64 | `sve` | no | `sve` |
| SVE2 | aarch64 | `sve2` | no | detected and reported only |
<!-- END GENERATED -->

### Dispatch levels (generated)

The most capable supported level is chosen at startup. **Determinism:** only `scalar`
may be used by model-defining code (kernel operations, constraint solving, feature
evaluation, naming, saved files). All other levels are display-only. "Measured speedup"
is filled in from the criterion benchmarks; a level that never measurably helps is
removed.

<!-- BEGIN GENERATED: dispatch-paths -->
| Level | Arch | Required features | Code paths | Determinism | Measured speedup | Notes |
|---|---|---|---|---|---|---|
| `scalar` | any | none | kernels::transform_points_f32 (viewport mesh transform)<br>kernels::bbox_f32 (viewport bounds, picking pre-pass) | model-defining | reference (1.00x) | Reference path. No explicit SIMD and no FMA contraction; the only path model-defining code may use. |
| `sse42` | x86_64 | `sse4.2`, `popcnt` | kernels::transform_points_f32 (viewport mesh transform)<br>kernels::bbox_f32 (viewport bounds, picking pre-pass) | display-only | not yet measured | x86-64-v2 baseline. Same arithmetic as scalar (no FMA), autovectorized with SSE4.2. |
| `avx2fma` | x86_64 | `sse4.2`, `popcnt`, `avx`, `avx2`, `fma` | kernels::transform_points_f32 (viewport mesh transform)<br>kernels::bbox_f32 (viewport bounds, picking pre-pass) | display-only | not yet measured | x86-64-v3 class. Uses fused multiply-add, so results may differ from scalar in the last bits. |
| `avx512` | x86_64 | `sse4.2`, `popcnt`, `avx`, `avx2`, `fma`, `avx512f`, `avx512bw`, `avx512vl` | kernels::transform_points_f32 (viewport mesh transform)<br>kernels::bbox_f32 (viewport bounds, picking pre-pass) | display-only | not yet measured | Optional bonus path; never expected (absent on consumer Intel 12th-14th gen and Core Ultra). |
| `neon` | aarch64 | `neon` | kernels::transform_points_f32 (viewport mesh transform)<br>kernels::bbox_f32 (viewport bounds, picking pre-pass) | display-only | not yet measured | ARMv8.0-A baseline (Raspberry Pi 4 and up). Uses fused multiply-add. |
| `neon-dotprod` | aarch64 | `neon`, `dotprod` | kernels::transform_points_f32 (viewport mesh transform)<br>kernels::bbox_f32 (viewport bounds, picking pre-pass) | display-only | not yet measured | Float kernels reuse the NEON code; dotprod/i8mm are reserved for future int8 kernels. |
| `sve` | aarch64 | `neon`, `sve` | kernels::transform_points_f32 (viewport mesh transform)<br>kernels::bbox_f32 (viewport bounds, picking pre-pass) | display-only | not yet measured | Mostly servers (Graviton3/4, Neoverse V/N2). LLVM autovectorizes to SVE; SVE intrinsics are nightly-only. |

Force a level with the CLI flag `--force-dispatch <level>` (any build) or, in debug/test builds only, the environment variable `GUSSET_FORCE_DISPATCH`. Level names: `scalar`, `sse42`, `avx2fma`, `avx512`, `neon`, `neon-dotprod`, `sve`. A forced level the CPU cannot run is an error, never a silent fallback.
<!-- END GENERATED -->

## CPU matrix

Legend: **yes**/**no** = verified against the cited source; *unverified* = not
confirmed from a primary source; "—" = not applicable. The "Gusset level" column is the
dispatch level the CPU would get (derived from the features in the row and the
registry above).

### Intel (x86-64)

| Generation (example SKU) | SSE4.2 | AVX | AVX2 | FMA3 | AVX-512 (F/BW/VL) | AVX-VNNI | Cores / threads (example) | Hybrid P/E | Gusset level |
|---|---|---|---|---|---|---|---|---|---|
| 2nd gen Core, Sandy Bridge (i7-2600K) | *unverified* (yes per ARK copies) | *unverified* (yes per ARK copies) | *unverified* (no) | *unverified* (no) | *unverified* (no) | *unverified* (no) | 4 / 8 [ark-2600k] | no (*unverified*) | `sse42` |
| 3rd gen Core, Ivy Bridge | *unverified* | *unverified* | no ("lacks AVX2" in ARK, per a forum quote [dell-avx2]; *unverified*) | *unverified* | *unverified* | *unverified* | *unverified* | *unverified* | `sse42` (*unverified*) |
| 4th gen Core, Haswell (i7-4770) | yes [ark-4770] | yes (implied by AVX2) | yes [ark-4770] | yes (Intel's AVX2 description includes FMA [intel-adl-avx2]; not listed separately on ARK) | no (not listed [ark-4770]) | *unverified* (no) | 4 / 8 [ark-4770] | no (*unverified*) | `avx2fma` |
| 5th gen Core, Broadwell | *unverified* | *unverified* | *unverified* | *unverified* | *unverified* | *unverified* | *unverified* | *unverified* | *unverified* |
| 6th–10th gen Core, Skylake family (i7-6700) | yes [ark-6700] | yes (implied by AVX2) | yes [ark-6700] | yes (as above) | no (not listed [ark-6700]) | *unverified* (no) | 4 / 8 [ark-6700] | no (*unverified*) | `avx2fma` |
| 10th/11th gen mobile, Ice Lake / Tiger Lake (i7-1165G7) | yes [ark-1165g7] | yes (implied) | yes [ark-1165g7] | yes (as above) | **yes** [ark-1165g7] | *unverified* | 4 / 8 [ark-1165g7] | no (*unverified*) | `avx512` |
| 11th gen desktop, Rocket Lake | *unverified* | *unverified* | *unverified* | *unverified* | *unverified* (yes) | *unverified* | *unverified* | *unverified* | *unverified* |
| 12th gen Core, Alder Lake (i7-12700K) | yes [ark-12700k] | yes (implied) | yes [ark-12700k] | yes (as above) | **no** — not supported on the hybrid design [intel-sdm-chg][intel-orm]; Intel stated it would fuse it off going forward [toms-adl-avx512] | yes, P- and E-cores [intel-orm] | 12 / 20 [ark-12700k] | **yes**: 8P + 4E [ark-12700k] | `avx2fma` |
| 13th/14th gen Core, Raptor Lake (i7-13700) | yes [ark-13700] | yes (implied) | yes [ark-13700] | yes (as above) | **no** (not listed [ark-13700]) | *unverified* (yes) | 16 / 24 [ark-13700] | **yes**: 8P + 8E [ark-13700] | `avx2fma` |
| Core Ultra Series 1, Meteor Lake (Ultra 7 155H) | *unverified* | *unverified* | *unverified* (yes) | *unverified* | *unverified* (no) | *unverified* | 16 / 22 [ark-155h] | **yes**: 6P + 8E + 2 LP-E [ark-155h] | *unverified* (`avx2fma`) |
| Core Ultra Series 2, Lunar Lake (Ultra 7 256V) | *unverified* | *unverified* | *unverified* (yes) | *unverified* | *unverified* (no) | *unverified* | 8 / 8, no Hyper-Threading [ark-256v] | **yes**: 4P + 4 LP-E [ark-256v] | *unverified* (`avx2fma`) |
| Core Ultra Series 2, Arrow Lake (Ultra 9 285K) | *unverified* | *unverified* | *unverified* (yes) | *unverified* | *unverified* (no) | *unverified* | 24 / 24, no Hyper-Threading [ark-285k] | **yes**: 8P + 16E [ark-285k] | *unverified* (`avx2fma`) |
| Core Ultra Series 3, Panther Lake (e.g. Ultra 7 355 [ark-355]) | *unverified* | *unverified* | *unverified* | *unverified* | *unverified* | *unverified* | *unverified* | *unverified* | *unverified* |

**AVX-512 on consumer Intel 12th–14th gen: not supported.** Intel's SDM change document
states that the Alder Lake performance hybrid architecture does not support AVX-512
[intel-sdm-chg], and the optimization manual says the E-cores lack it while both core
types support AVX2 and AVX-VNNI [intel-orm]. Some early Alder Lake boards could enable it
with E-cores disabled; Intel closed that with microcode and said it would fuse it off
[toms-adl-avx512]. The i7-13700's ARK entry lists SSE4.1, SSE4.2 and AVX2 only
[ark-13700]. Gusset never expects AVX-512; it is an optional bonus path.

### AMD (x86-64)

| Generation (example SKU) | SSE4.2 | AVX | AVX2 | FMA3 | AVX-512 | Cores / threads (example) | Hybrid | Gusset level |
|---|---|---|---|---|---|---|---|---|
| FX, Piledriver (FX-8350) | *unverified* | yes [amd-fx-sheet] | *unverified* (no) | yes ("FMA" listed; FMA3 vs FMA4 not distinguished [amd-fx-sheet]) | *unverified* (no) | 8 cores [amd-fx-sheet] | no (*unverified*) | `sse42` (*unverified*) |
| Zen / Zen+ (Ryzen 7 1700; Ryzen 5 3400G APU) | yes [amd-3400g] | yes [amd-1700][amd-3400g] | *unverified* — AMD's product pages list AVX and FMA3 but not AVX2 [amd-1700][amd-3400g] | yes [amd-1700][amd-3400g] | no (not listed) | 3400G: 4 / 8 [amd-3400g] | no (*unverified*) | `avx2fma` if AVX2 is detected at runtime, else `sse42` |
| Zen 2 / Zen 3 | *unverified* | *unverified* | *unverified* (yes) | *unverified* | *unverified* (no) | *unverified* | *unverified* | *unverified* |
| Zen 4 (Ryzen 7 7700X) | yes [amd-7700x] | yes [amd-7700x] | yes [amd-7700x] | yes [amd-7700x] | **yes** [amd-7700x]; executed over a 256-bit datapath (stated for EPYC 9004 [amd-epyc-avx512]) | 8 / 16 [amd-7700x] | no (*unverified*) | `avx512` |
| Zen 5 (Ryzen 7 9700X) | yes [amd-9700x] | yes [amd-9700x] | yes [amd-9700x] | yes [amd-9700x] | **yes** [amd-9700x]; full 512-bit datapath stated for EPYC 9005 [amd-epyc-avx512], desktop *unverified* | 8 / 16 [amd-9700x] | no (*unverified*) | `avx512` |

Runtime detection decides in every case; a table entry never overrides what the CPU
reports.

### ARM64

Feature columns for ARM cores come from LLVM's processor definitions [llvm-aarch64]
(the compiler's model of each core) unless another source is given. That file is a
secondary source; vendor documentation should replace it where available.

| CPU (core) | NEON | DotProd | I8MM | SVE | SVE2 | Cores (example) | Hybrid | Gusset level |
|---|---|---|---|---|---|---|---|---|
| Raspberry Pi 4 — BCM2711, Cortex-A72 | yes | no [llvm-aarch64] | no [llvm-aarch64] | no [llvm-aarch64] | no | 4 [rpi4-spec] | no | `neon` |
| Raspberry Pi 5 — BCM2712, Cortex-A76 | yes | yes [llvm-aarch64] | no [llvm-aarch64] | no [llvm-aarch64] | no | 4 [rpi5-brief] | no | `neon-dotprod` |
| Apple M1 (LLVM `apple-m1` = `apple-a14`) | yes | yes [llvm-aarch64] | no [llvm-aarch64] | no [llvm-aarch64] | no | *unverified* | yes (P + E, *unverified* counts) | `neon-dotprod` |
| Apple M2 (`apple-a15`) | yes | yes [llvm-aarch64] | yes [llvm-aarch64] | no [llvm-aarch64] | no | *unverified* | yes (*unverified* counts) | `neon-dotprod` |
| Apple M3 (`apple-a16`) | yes | yes [llvm-aarch64] | yes [llvm-aarch64] | no [llvm-aarch64] | no | *unverified* | yes (*unverified* counts) | `neon-dotprod` |
| Apple M4 | yes | yes [llvm-aarch64] | yes [llvm-aarch64] | **no** — SME/SME2 only, no non-streaming SVE [llvm-aarch64] | no | *unverified* | yes (*unverified* counts) | `neon-dotprod` |
| Qualcomm Snapdragon X Elite (Oryon) | yes | yes [llvm-aarch64] | yes [llvm-aarch64] | **no** [llvm-aarch64][hwcooling-oryon] | no [hwcooling-oryon] | 12 [qc-x-elite] | no (*unverified*) | `neon-dotprod` |
| AWS Graviton2 (Neoverse N1) | yes | yes [llvm-aarch64] | no [llvm-aarch64] | no [llvm-aarch64] | no | *unverified* | no | `neon-dotprod` |
| AWS Graviton3 (Neoverse V1) | yes | yes [llvm-aarch64] | yes [llvm-aarch64] | **yes** [aws-graviton][llvm-aarch64] | no [llvm-aarch64] | *unverified* | no | `sve` |
| AWS Graviton4 (Neoverse V2) | yes | yes [llvm-aarch64] | yes [llvm-aarch64] | **yes** [llvm-aarch64] | **yes** [aws-graviton][llvm-aarch64] | *unverified* | no | `sve` |
| Ampere Altra / Altra Max (Neoverse N1) | yes | yes [llvm-aarch64] | no [llvm-aarch64] | no [llvm-aarch64] | no | 32–128 [ampere-altra] | no; single-threaded cores [ampere-altra] | `neon-dotprod` |
| AmpereOne (LLVM `ampere1`) | yes | yes [llvm-aarch64] | yes [llvm-aarch64] | no [llvm-aarch64] | no | *unverified* | no (*unverified*) | `neon-dotprod` |

SVE is a server feature in practice: none of the laptop/desktop/SBC chips above have it.

## GPU quality tiers

The tier is chosen automatically from the adapter class and can be overridden by the
user. Rows below are generated from `GPU_TIERS`.

<!-- BEGIN GENERATED: gpu-tiers -->
| Tier | Chosen automatically for | MSAA | SSAO | Shadows | LOD detail factor | LOD factor while moving | Render scale while moving | Notes |
|---|---|---|---|---|---|---|---|---|
| `low` | software, integrated, virtual, unknown | 1x | no | no | 1.00 | 0.50 | 0.75 | Reference-machine tier (Intel HD 520-class iGPU, software renderers). Downlevel wgpu limits only. |
| `medium` | manual only | 4x | no | no | 1.50 | 0.50 | 1.00 | Manual choice for strong iGPUs until benchmarks justify auto-selection. |
| `high` | discrete | 8x | yes | yes | 2.00 | 0.75 | 1.00 | Dedicated GPU. Falls back to 4x MSAA if the adapter does not support 8x for the target format. |
<!-- END GENERATED -->

## GPU matrix

wgpu backend support levels [wgpu-readme]: Vulkan and DX12 are first-class on Windows,
Vulkan on Linux, Metal on macOS; OpenGL is "downlevel / best effort" (GL 3.3+ on
Windows, GL ES 3.0+ on Linux). Gusset requests only wgpu's downlevel limits and queries
optional features at runtime; the per-GPU optional-feature column is *unverified*
until we record `wgpu` adapter reports from real machines (planned: a CI artifact plus
contributed reports).

| GPU family (example) | Backends Gusset can use | API levels | Default tier | Minimum / recommended driver |
|---|---|---|---|---|
| Intel HD 4600 (Haswell) | DX12, GL; **no Vulkan on Windows** [intel-hsw-vulkan][intel-vulkan-min] | DirectX 11.2/12, OpenGL 4.3 [ark-4770] | low | Windows 15.40/15.45 branch, end of support 2024-06-15 [intel-1545] |
| Intel HD 520/530 (Skylake, Gen9) | Vulkan, DX12, GL (Vulkan from 6th gen [intel-vulkan-min]) | DirectX 12, OpenGL 4.5 [ark-6700]; Vulkan version *unverified* | low | 6th gen: driver unsupported since 2022-12-30, use ≥ 31.0.101.2130 [intel-sa-01253]; 7th–10th: legacy quarterly releases, 31.0.101.2145 current at research time [intel-legacy][intel-7-10-dl] |
| Intel Iris Xe (Tiger Lake) | *unverified* | *unverified* | low | *unverified* |
| Intel Arc (A-series, Arc 140V) | *unverified* | *unverified* | high (discrete) / low (integrated) | *unverified* |
| AMD GCN 1–3 (R9 200/300, Fury) | *unverified* | *unverified* | high (discrete) | legacy: final drivers, no further releases [amd-gpu-630] |
| AMD Vega iGPU (Ryzen 5 3400G, Vega 11) | *unverified* (Vulkan, DX12, GL expected) | DirectX 12, OpenGL 4.6 [amd-3400g] | low | Polaris/Vega receive a separate Adrenalin package [amd-rn-25-5-1] |
| AMD RDNA 1–4 | *unverified* | *unverified* | high (discrete) / low (integrated) | *unverified* |
| NVIDIA Kepler | *unverified* | *unverified* | high | *unverified* |
| NVIDIA Maxwell / Pascal / Volta | *unverified* | *unverified* | high | driver branch 580 is the last; quarterly security updates through October 2028 [nv-580-blog][nv-geforce-eol] |
| NVIDIA Turing and newer | *unverified* | *unverified* | high | *unverified* |
| Apple M1 / M2 / M3–M4 GPUs | Metal | Metal GPU families Apple7 / Apple8 / Apple9 [metal-tables][mtl-gpufamily] | low (integrated) | ships with macOS |
| Qualcomm Adreno X1-85 (Snapdragon X Elite) | DX12 [qc-x-elite]; Vulkan *unverified* | DirectX 12 [qc-x-elite] | low | *unverified* (Windows-on-ARM driver quality to be assessed) |
| Raspberry Pi 4 — VideoCore VI (Mesa V3D/V3DV) | Vulkan, GL ES | OpenGL ES 3.1, Vulkan 1.0 [rpi4-spec]; Vulkan 1.2 conformance announced [rpi-vk12] | low | Mesa from current Raspberry Pi OS (*unverified* minimum) |
| Raspberry Pi 5 — VideoCore VII | Vulkan, GL ES | OpenGL ES 3.1; Vulkan 1.2 per product brief [rpi5-brief], Vulkan 1.3 per product page [rpi5-page] (conflicting) | low | *unverified* |
| WARP (Windows software) | DX12 | D3D12 feature levels 12_0 and 12_1 since Windows 10 Fall Creators Update [ms-warp] | low | ships with Windows |
| llvmpipe (Mesa software GL) | GL | Mesa implements GL 4.6; reported version depends on the driver [mesa-23-1-4]; llvmpipe's exact version *unverified* | low | *unverified* |
| lavapipe (Mesa software Vulkan) | Vulkan | Vulkan 1.3 since Mesa 22.1.0 [mesa-22-1-0] | low | Mesa ≥ 22.1 for Vulkan 1.3 [mesa-22-1-0] |

## Resource budgets

Thread pools, cache size and default tessellation quality scale with the detected
machine. Table generated from `gusset_hw::budgets` for example machines:

<!-- BEGIN GENERATED: budgets -->
| Example machine | Interactive threads | Background threads | Cache budget | Default tessellation | Speculative precompute |
|---|---|---|---|---|---|
| Raspberry Pi 4-class: 4 cores, 4 GB | 3 | 1 | 256 MiB | coarse | no |
| Reference machine: 4 cores, 8 GB | 3 | 1 | 512 MiB | standard | no |
| 4C/8T desktop (e.g. Core i7-6700), 8 GB | 7 | 3 | 512 MiB | standard | no |
| Core i7-13700 (8P+8E, 24 threads), 32 GB | 23 | 8 | 2048 MiB | extra-fine | yes |
| Hybrid laptop: 4P + 4E cores, 16 GB | 7 | 4 | 1024 MiB | fine | yes |
| Workstation: 16 cores / 32 threads, 64 GB | 31 | 15 | 4096 MiB | extra-fine | yes |

Rules (from `gusset_hw::budgets`): interactive = max(1, logical - 1); background = efficiency logical CPUs on hybrid CPUs, else interactive / 2, clamped to [1, interactive]; cache = RAM / 16 clamped to [128 MiB, 4 GiB]; if RAM is unknown, 4 GiB is assumed (cache 256 MiB).
<!-- END GENERATED -->

Background threads run at low priority / efficiency QoS: nice 10/19 on Linux, QoS
`UTILITY`/`BACKGROUND` on macOS, EcoQoS plus lowered priority on Windows (see
`gusset_hw::priority`). Detection limits: physical cores and P/E topology are read from
sysfs on Linux and `sysctl` on macOS; on Windows they are reported as unknown for now,
and installed RAM comes from `GlobalMemoryStatusEx`.

## Known issues

| GPU / CPU | Driver / OS | Issue | Gusset behavior / workaround | Source |
|---|---|---|---|---|
| Intel Haswell / Broadwell iGPU | Windows | No official Vulkan driver | Use DX12, then GL | [intel-hsw-vulkan][intel-vulkan-min] |
| Intel 4th/5th gen iGPU | Windows driver 15.40/15.45 | Out of support since 2024-06-15, no security fixes | Works if the driver works; diagnostics flag the branch (planned) | [intel-1545] |
| Intel 6th gen iGPU | Windows | Graphics driver unsupported since 2022-12-30 | Recommend ≥ 31.0.101.2130 | [intel-sa-01253] |
| AMD GCN 1–3 | Windows | Legacy; no further driver releases | Works with final driver; no workarounds yet | [amd-gpu-630] |
| NVIDIA Maxwell / Pascal / Volta | all | Security-only updates after October 2025 | Works; no workarounds yet | [nv-geforce-eol] |
| Raspberry Pi 5 | Raspberry Pi OS | Official sources disagree on the Vulkan version (1.2 vs 1.3) | Read the version from the adapter at runtime; never assume | [rpi5-brief][rpi5-page] |
| Intel 12th–14th gen | any | AVX-512 absent (can appear on some early Alder Lake boards with E-cores disabled) | Runtime detection only; AVX-512 is an optional path | [intel-sdm-chg][toms-adl-avx512] |

## System requirements

These are draft targets for end users; final values are set with the release ADR and
the measured budgets in `docs/PERFORMANCE.md`.

| | Minimum | Recommended (the reference machine) | Best experience |
|---|---|---|---|
| CPU | 64-bit x86 with x86-64-v2 (SSE4.2/POPCNT; Intel Core 2nd gen and later, AMD FX and later — *unverified* for every SKU), or 64-bit ARM (ARMv8-A, e.g. Raspberry Pi 4) | 4 cores, e.g. Intel Core 6th–8th gen or AMD Ryzen with Vega graphics | 8+ cores with AVX2 (any recent Intel/AMD), or Apple M-series / Snapdragon X |
| RAM | 4 GB | 8 GB (shared with the integrated GPU) | 16–32 GB |
| GPU | None required: falls back to a software renderer (WARP, llvmpipe/lavapipe), slower but usable | Integrated GPU, Intel HD 520/530-class or AMD Vega iGPU, with a current driver | Dedicated GPU (enables the `high` tier: SSAO, shadows, 8x MSAA) |
| Graphics API | Vulkan, DX12, Metal, or OpenGL 3.3 / GL ES 3.0 [wgpu-readme] | Vulkan or DX12 | Vulkan, DX12 or Metal |
| OS | Windows 10/11 x64 or ARM64; Linux x64/ARM64 (Wayland or X11); macOS on Apple silicon (versions set by the release ADR) | same | same |

## Sources

Fetched or searched on 2026-10-10. Vendor pages are primary; LLVM, Tom's Hardware,
hwcooling.net and forum posts are secondary.

[psabi]: https://gitlab.com/x86-psABIs/x86-64-ABI "x86-64 psABI (micro-architecture levels) — not re-fetched in this session"
[ark-2600k]: https://www.intel.com/content/www/us/en/products/sku/52214/intel-core-i72600k-processor-8m-cache-up-to-3-80-ghz/specifications.html
[dell-avx2]: https://www.dell.com/community/XPS-Desktops/XPS-One-2710-i7-3770S-Support-for-AVX2/td-p/7568304
[ark-4770]: https://www.intel.com/content/www/us/en/products/sku/75122/intel-core-i74770-processor-8m-cache-up-to-3-90-ghz/specifications.html
[ark-6700]: https://www.intel.com/content/www/us/en/products/sku/88196/intel-core-i76700-processor-8m-cache-up-to-4-00-ghz/specifications.html
[ark-1165g7]: https://www.intel.com/content/www/us/en/products/sku/208921/intel-iris-xe-graphics/specifications.html
[ark-12700k]: https://www.intel.com/content/www/us/en/products/sku/134594/intel-core-i712700k-processor-25m-cache-up-to-5-00-ghz/specifications.html
[ark-13700]: https://www.intel.com/content/www/us/en/products/sku/230490/intel-core-i713700-processor-30m-cache-up-to-5-20-ghz/specifications.html
[ark-155h]: https://www.intel.com/content/www/us/en/products/sku/236847/intel-core-ultra-7-processor-155h-24m-cache-up-to-4-80-ghz/specifications.html
[ark-256v]: https://www.intel.com/content/www/us/en/products/sku/240954/intel-core-ultra-7-processor-256v-12m-cache-up-to-4-80-ghz/specifications.html
[ark-285k]: https://www.intel.com/content/www/us/en/products/sku/241060/intel-core-ultra-9-processor-285k-36m-cache-up-to-5-70-ghz/specifications.html
[ark-355]: https://www.intel.com/content/www/us/en/products/sku/245722/intel-core-ultra-7-processor-355-12m-cache-up-to-4-70-ghz/specifications.html
[intel-adl-avx2]: https://edc.intel.com/content/www/us/en/design/ipla/software-development-platforms/client/platforms/alder-lake-desktop/12th-generation-intel-core-processors-datasheet-volume-1-of-2/009/intel-advanced-vector-extensions-2-intel-avx2/
[intel-sdm-chg]: https://cdrdv2-public.intel.com/782168/252046-sdm-change-document.pdf
[intel-orm]: https://cdrdv2-public.intel.com/821612/248966-Optimization-Reference-Manual-V1-050.pdf
[toms-adl-avx512]: https://tomshardware.com/news/intel-reportedly-kills-avx-512-alder-lake-cpus
[amd-fx-sheet]: https://subscriptions.amd.com/newsletters/channelnews/images/archive/%7B8c2ad3d7-3617-4eca-8465-b6c75b4cd054%7D_AMD_(New)_FX_Processors_Cheat_Sheet.pdf
[amd-1700]: https://www.amd.com/en/support/downloads/drivers.html/processors/ryzen/ryzen-1000-series/amd-ryzen-7-1700.html
[amd-3400g]: https://www.amd.com/en/support/downloads/drivers.html/processors/ryzen/ryzen-3000-series/amd-ryzen-5-3400g.html
[amd-7700x]: https://www.amd.com/en/products/processors/desktops/ryzen/7000-series/amd-ryzen-7-7700x.html
[amd-9700x]: https://www.amd.com/en/products/processors/desktops/ryzen/9000-series/amd-ryzen-7-9700x.html
[amd-epyc-avx512]: https://www.amd.com/en/blogs/2025/leadership-hpc-performance-with-5th-generation-amd.html
[llvm-aarch64]: https://github.com/llvm/llvm-project/blob/main/llvm/lib/Target/AArch64/AArch64Processors.td "fetched 2026-10-10 (main branch)"
[rpi4-spec]: https://www.raspberrypi.com/products/raspberry-pi-4-model-b/specifications/
[rpi5-brief]: https://pip-assets.raspberrypi.com/categories/892-raspberry-pi-5/documents/RP-008348-DS/raspberry-pi-5-product-brief
[rpi5-page]: https://www.raspberrypi.com/products/raspberry-pi-5/
[rpi-vk12]: https://www.raspberrypi.com/news/vulkan-update-version-1-2-conformance-for-raspberry-pi-4/
[qc-x-elite]: https://www.qualcomm.com/products/mobile/snapdragon/laptops-and-tablets/snapdragon-x-elite
[hwcooling-oryon]: https://www.hwcooling.net/en/oryon-arm-core-in-snapdragon-x-cpus-architecture-analysis/
[aws-graviton]: https://github.com/aws/aws-graviton-getting-started
[ampere-altra]: https://amperecomputing.com/briefs/ampere-altra-family-product-brief
[wgpu-readme]: https://github.com/gfx-rs/wgpu
[intel-hsw-vulkan]: https://community.intel.com/t5/Graphics/Vulkan-support-for-HD4600-on-windows/td-p/398463
[intel-vulkan-min]: https://www.intel.com/content/www/us/en/develop/articles/how-to-install-vulkan-apis-for-ue4.html
[intel-1545]: https://www.intel.com/content/www/us/en/download/18799/intel-graphics-driver-for-windows-15-45.html
[intel-sa-01253]: https://www.intel.com/content/www/us/en/security-center/advisory/intel-sa-01253.html
[intel-legacy]: https://www.intel.com/content/www/us/en/support/articles/000091662/graphics.html
[intel-7-10-dl]: https://www.intel.com/content/www/us/en/download/776137/intel-7th-10th-gen-processor-graphics-windows.html
[amd-gpu-630]: https://www.amd.com/en/resources/support-articles/faqs/GPU-630.html
[amd-rn-25-5-1]: https://www.amd.com/en/resources/support-articles/release-notes/RN-RAD-WIN-25-5-1.html
[nv-580-blog]: https://developer.nvidia.com/blog/navigating-gpu-architecture-support-a-guide-for-nvidia-cuda-developers/
[nv-geforce-eol]: https://www.nvidia.com/en-us/geforce/news/mafia-the-old-country-geforce-game-ready-driver/
[metal-tables]: https://developer.apple.com/metal/Metal-Feature-Set-Tables.pdf
[mtl-gpufamily]: https://developer.apple.com/documentation/metal/mtlgpufamily
[ms-warp]: https://learn.microsoft.com/en-us/windows/win32/direct3darticles/directx-warp
[mesa-23-1-4]: https://docs.mesa3d.org/relnotes/23.1.4.html
[mesa-22-1-0]: https://docs.mesa3d.org/relnotes/22.1.0.html

Reference list in readable form:

- psABI: [psabi] · Intel ARK: [ark-2600k], [ark-4770], [ark-6700], [ark-1165g7], [ark-12700k], [ark-13700], [ark-155h], [ark-256v], [ark-285k], [ark-355]
- Intel documents: [intel-adl-avx2], [intel-sdm-chg], [intel-orm], [intel-vulkan-min], [intel-1545], [intel-sa-01253], [intel-legacy], [intel-7-10-dl]; community: [intel-hsw-vulkan], [dell-avx2]; press: [toms-adl-avx512]
- AMD: [amd-fx-sheet], [amd-1700], [amd-3400g], [amd-7700x], [amd-9700x], [amd-epyc-avx512], [amd-gpu-630], [amd-rn-25-5-1]
- Arm-based: [llvm-aarch64], [rpi4-spec], [rpi5-brief], [rpi5-page], [rpi-vk12], [qc-x-elite], [hwcooling-oryon], [aws-graviton], [ampere-altra]
- GPU / APIs: [wgpu-readme], [nv-580-blog], [nv-geforce-eol], [metal-tables], [mtl-gpufamily], [ms-warp], [mesa-23-1-4], [mesa-22-1-0]
