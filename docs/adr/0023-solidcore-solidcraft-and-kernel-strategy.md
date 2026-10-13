# ADR 0023: SolidCore, SolidCraft and the kernel strategy

- Status: accepted
- Date: 2026-10-13
- Owner request (2026-10-13): integrate SolidCore and SolidCraft for performance ("there
  are places where OCCT is better and places where SolidCore is better"), port OCCT to pure
  Rust with multithreading, and use much of SolidCraft's Fusion-like UI layout, changed a
  little or a lot.

## What they are (checked 2026-10-13, repositories cloned read-only into quarantine)
- **SolidCraft** (github.com/bherbruck/solidcraft, MIT OR Apache-2.0, ~1.7k stars, 886
  commits, ~254k lines of Rust): a parametric CAD app with the same shape as Gusset (egui
  0.36 + wgpu, MCP server, headless CLI, web build). Kernel: the `truck` crates (Apache-2.0,
  RICOS), with a vendored, patched `truck-shapeops`. Features beyond ours: assemblies with
  joints, sheet metal, threads, draft, split, configurations, a polished Fusion-style UI
  (ribbon with grouped menus, browser tree, view cube, navigation bar, command line,
  timeline with playback). It calls itself "early but usable for simple parts". Its own
  docs show truck's limits: booleans that need numerical retries (operand nudging,
  `boolean_guard.rs`), "fillet between two curved faces: not supported yet", shells thicker
  than a wall refused. All UI artwork is original and code-drawn (no Autodesk assets).
- **SolidCore** (github.com/bherbruck/solidcore, Apache-2.0, ~303k lines): a safe-Rust port
  of NVIDIA's SMLib (USD BRep, Apache-2.0), deterministic, no `unsafe`, no panics, wasm,
  verified against the C++ to 1e-9 by a difftest harness. About a third is ported (math,
  NURBS, curves); its README says "nothing is usable end to end yet (no primitives, booleans
  or fillets)". It is meant to replace truck in SolidCraft. No performance or threading
  claims exist yet.

## Decisions

### 1. No wholesale port of OCCT to Rust
- Size: the OCCT parts Gusset uses are ~1.6 M lines of C++ (foundation, modeling data and
  algorithms) plus ~0.6 M for STEP/IGES, carrying 30 years of numerical robustness work.
- Licence: a translation of OCCT is a derivative work and stays LGPL-2.1; it could not be
  MIT/Apache like Gusset, and statically linked Rust breaks the relinking condition our spec
  puts on LGPL code (PROMPT.md, Licensing).
- Speed does not come from the language: the slow paths we measured were algorithms
  (quadratic loops, redundant checks, single-threaded passes). Those are fixed where they
  are, with small OCCT patches (ADR 0002 amendment), parallel passes and process-level
  parallelism, at a fraction of the cost.

### 2. A multi-kernel architecture instead
The `Kernel` trait already isolates the geometry engine, and the kernel worker (ADR 0022)
moves it out of process. Next steps:
- **Per-operation routing:** a `RoutingKernel` sends each operation to the backend that
  does it best and converts shapes between backends through a neutral B-rep exchange
  (OCCT BRep / STEP today, a native schema later). Decisions per operation are made from
  measurements, not belief, with a shared conformance suite (our OCCT goldens and
  naming goldens) that every backend must pass for the operations it serves.
- **SolidCore adapter** (`gusset-kernel-solidcore`) as soon as SolidCore has primitives,
  booleans and tessellation: run the conformance suite and the perf reports against it, and
  route to it the operations where it is correct and faster. Until then, track its progress
  monthly in docs/PROGRESS.md. Contributing upstream is welcome but is an outward action
  the owner approves case by case.
- **truck** is not adopted as a backend: SolidCraft's own robustness docs show it is behind
  OCCT on booleans and fillets, which are the operations that matter most.
- **Native Rust fast paths** where we own the math exactly (topology summaries, mass
  properties of analytic faces, tessellation of planes and cylinders, sketch profiles) can
  move out of the C++ side when profiling shows they dominate.

### 3. Multithreading where it pays
- Inside OCCT: parallel validity checks and per-face/edge measures (done), patches to
  quadratic algorithms (two so far).
- Across features: the evaluator builds independent bodies and subtrees in parallel on
  several kernel workers (ADR 0022 makes each worker a process, so OCCT's global state is
  not shared). Results are merged in document order, so naming stays deterministic.
- STEP import of many bodies in parallel batches (SolidCraft measured 60-80 % less time).

### 4. UI: adopt SolidCraft's Fusion-style layout, in our design system
Gusset's GUI moves to the layout the owner likes in SolidCraft (DESIGN.md §2 updated):
title bar with file/undo, workspace tabs, a ribbon of icon tools in named dropdown groups
(Create, Modify, Construct, Inspect, Insert, Select), a browser tree (Document settings,
Origin, Bodies, Sketches, Parameters, Branches), a view cube and a navigation bar in the
viewport, a command line, and a timeline with playback controls and feature icons. Where
SolidCraft's code-drawn widgets (icons, view cube, navigation bar, timeline glyphs) fit,
they are adapted with attribution (MIT OR Apache-2.0: the copyright notice stays in the file
header and in THIRD_PARTY_LICENSES / ATTRIBUTION), and restyled with our tokens (DESIGN.md
§3) so Gusset keeps its own look. Our differences stay: the code view, the iteration graph
and branches, agent visibility, and expressions everywhere.

## Consequences
- OCCT remains the production kernel for v0.3; performance work continues inside it.
- A second backend becomes possible without touching the evaluator, MCP or GUI.
- Third-party UI code carries attribution; `scripts/check-secrets.sh` and cargo-deny are
  unaffected (no new dependency for adapted code).
