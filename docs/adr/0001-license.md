# ADR 0001: License and dependency license policy

- Status: accepted
- Date: 2026-10-10

## Decision
- All Gusset code is dual-licensed **MIT OR Apache-2.0** (`LICENSE-MIT`, `LICENSE-APACHE`).
- Dependency policy, enforced by `cargo-deny` (`deny.toml`) in CI:
  - **Allowed:** MIT, Apache-2.0 (with or without the LLVM exception), BSD-2/3-Clause,
    ISC, Zlib, Unicode-3.0, Unicode-DFS-2016, CC0-1.0, BSL-1.0, MPL-2.0 (file-level
    copyleft; we never modify MPL files in-tree), and public-domain (SQLite-style).
  - **LGPL** (e.g. OCCT, LGPL-2.1 with the Open CASCADE exception) only if **dynamically
    linked**: we ship the unmodified shared libraries, publish the exact source and
    build recipe (and any patches), and include the attribution notice. Statically
    linking an LGPL C++ library is not allowed. LGPL Rust crates are not allowed, because
    Rust links statically.
  - **GPL/AGPL/SSPL**: never linked into Gusset. A GPL tool may only exist as a separately
    distributed program that talks to Gusset over IPC/CLI, documented in its own ADR.
  - Code is never copied from GPL projects or from repositories with no license.
- **Fonts** bundled as data (not code) may use permissive font licences: OFL-1.1, and
  the Ubuntu Font Licence 1.0 for egui's default fonts (`epaint_default_fonts`). The
  UFL allows redistribution bundled with software and puts no conditions on our code;
  modified fonts must be renamed, and we ship them unmodified. It is allowed in
  `deny.toml` as an exception for that crate only, not globally. (Recorded 2026-10-10.)
- `THIRD_PARTY_LICENSES.md` is generated from the dependency tree by CI, with a
  "check that the file is up to date" job.

## Consequences
- OCCT bindings must link dynamically (see the kernel ADR). OCCT bindings that are
  themselves AGPL (if any) cannot be used; we write our own thin binding instead.
