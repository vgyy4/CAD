# ADR 0008: Security architecture

- Status: accepted
- Date: 2026-10-10
- Threat model: `docs/THREAT_MODEL.md`

## Decisions
1. **No privilege, ever.** Gusset never requests admin/root and ships no services,
   scheduled tasks, setuid binaries, drivers or privileged helpers. It installs
   per-user by default. Machine-wide installs only target admin-writable locations,
   and CI checks the resulting ACLs/permissions.
2. **Memory safety.** `#![forbid(unsafe_code)]` is the workspace default. Only these
   crates may contain `unsafe`, each with `// SAFETY:` comments on every block, and
   each has Miri runs where feasible:
   - `gusset-occt-sys` (C++ FFI shim, and the safe wrappers in its `safe` module that
     `gusset-kernel-occt` and the worker use; those two crates forbid `unsafe`)
   - `gusset-store` (Windows `MoveFileExW` write-through only)
   - `gusset-hw` (CPU feature/brand queries, thread-priority FFI, pointing stdout at
     stderr for the `gusset mcp` protocol stream, and the Windows DLL search-path
     hardening that the app, CLI and worker call at startup)
   - `gusset-sandbox` (OS sandbox setup for the import worker, process-group kill)

   The reviewer checks this list against `grep -r "allow(unsafe_code)"`, and
   `crates/gusset-sandbox/tests/unsafe_allowlist.rs` enforces it: every other crate must
   inherit the workspace lints (`unsafe_code = "forbid"`), and this list must match the
   test's.
3. **Code-view language sandbox:** the language is declarative and pure (ADR 0004), and
   its parser and evaluator enforce size, depth and count limits. A future scripting
   feature gets its own WASM sandbox with explicit capabilities and a per-project
   permission prompt.
4. **Out-of-process importers:**
   - The `gusset-worker` binary (same install directory) parses STEP/IGES and every
     proprietary format.
   - It is started with an argument array (no shell), gets the input bytes over stdin,
     and returns results over stdout using a length-prefixed, versioned, size-limited
     message format.
   - Sandbox, applied before it reads any input:
     - Linux: Landlock (no filesystem access), seccomp-bpf allow-list, no network
       namespace access (the `unshare` user namespace if available), and
       `RLIMIT_AS`/`RLIMIT_CPU`.
     - Windows: a job object (memory/CPU/kill-on-close) plus a restricted
       token/low integrity, and AppContainer when feasible.
     - macOS: `sandbox_init` with a no-filesystem/no-network profile.
   - Where a platform mechanism is unavailable, the worker still runs out of process
     with resource limits, and the gap is reported in diagnostics.
5. **MCP security** is described in ADR 0012: localhost only, bearer token, Origin/Host
   validation, project-folder path confinement, confirm flags, human approval for
   high-impact actions, audit log, rate limits, read-only mode and a kill switch.
6. **Code loading hardening** at the top of `main`, before any other code runs:
   - Windows: `SetDefaultDllDirectories`, and OCCT DLLs are loaded by absolute path
     (delay-load or explicit `LoadLibraryExW` with
     `LOAD_LIBRARY_SEARCH_APPLICATION_DIR`).
   - Linux: RPATH `$ORIGIN/../lib/gusset` only.
   - macOS: `@executable_path/../Frameworks` plus the hardened runtime.
7. **External programs** (only Blender for now) are launched only from an absolute path
   the user confirmed in Settings, with an argument array and fixed safe flags.
8. **Network:** only the updater and user-initiated integrations (Onshape) use it, via
   rustls with `rustls-platform-verifier`, with no insecure fallbacks. Nothing touches
   the network on file open.
9. **Secrets** live only in the OS credential store (the `keyring` ecosystem, verified
   before adoption). They never appear in settings, project files, logs or MCP output.
10. **Logging** uses a scrubber that replaces the home directory with `~`, removes the
    username, and redacts anything token-shaped. There is no telemetry. Crash reports
    are opt-in, with a preview of exactly what would be sent.
11. **CI security gates:** cargo-deny, cargo-audit, clippy with security lints
    (`unwrap_used` etc. are already enabled), CodeQL for Actions workflows and C++ (the
    shim), OpenSSF Scorecard, fuzzing of every parser (cargo-fuzz smoke runs on PRs,
    longer runs nightly), Miri on the unsafe crates, and installer permission checks.

## Consequences
- The import worker costs a process spawn per import (tens of ms), which is acceptable
  for an infrequent operation.
- Some sandbox features are unavailable on old kernels or OS versions. They degrade
  visibly (diagnostics), never silently.
