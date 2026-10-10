# Gusset threat model

Living document (spec: "Security"). Update it whenever an entry point is added.
Last full revision: 2026-10-10 (Phase 1).

## 1. Assets
| Asset | Why it matters |
|---|---|
| A1 User project files and their history | Users' work; loss or corruption is the worst outcome |
| A2 Integrity of the user's machine / account | We must never become a path to code execution or privilege escalation |
| A3 Secrets (Onshape API keys, update-signing private key in CI) | Abuse of accounts; malicious updates |
| A4 Update channel integrity | A compromised update is remote code execution on every install |
| A5 MCP session token | Lets a holder drive the app with the user's rights |
| A6 Privacy (file paths, usernames, model contents) | No telemetry; logs and crash reports must not leak |

## 2. Attackers
| ID | Attacker | Capabilities |
|---|---|---|
| T1 | Author of a malicious project or import file | Controls every byte of `.gus`, STEP, IGES, STL, 3MF, OBJ, `.f3d`, `.sldprt`, FCStd and `.scad` files the user opens |
| T2 | Another local user on the same machine | Can create files in shared locations (`/tmp`, `C:\ProgramData`, shared folders) and connect to localhost ports |
| T3 | A local process of the same user (malware) | Out of scope for full protection; we avoid making it easier (token file permissions, no DLL search hijack) |
| T4 | Network attacker / malicious website | DNS rebinding against localhost MCP; MITM on update downloads |
| T5 | Compromised or fooled AI agent | Speaks our MCP protocol with a valid token, but may be prompt-injected by model content |
| T6 | Supply-chain attacker | Typosquatted crates, compromised dependency release, malicious GitHub Action, tampered OCCT source |
| T7 | Attacker with a stolen/old update package | Replays an old vulnerable version or an expired manifest |

## 3. Trust boundaries and entry points
```
             ┌──────────────── user account boundary ───────────────┐
 files (T1) ─┼─▶ [import worker, sandboxed] ──IPC──▶ ┌────────────┐ │
             │                                        │  Gusset    │ │
 .gus (T1) ──┼─▶ [container parser + doc parser] ───▶ │  app       │ │
             │                                        │ (no admin) │ │
 agent (T5) ─┼─▶ [MCP: stdio bridge | 127.0.0.1 HTTP  │            │ │
 other user ─┼─X  + bearer token + Origin/Host check] │            │ │
 (T2)        │                                        └─────┬──────┘ │
 network (T4)┼─▶ [updater: TLS + Ed25519 manifest + TUF rules] ◀────┘ │
             └───────────────────────────────────────────────────────┘
 CI (T6): pinned actions, cargo-deny/audit, hash-verified OCCT, release env secrets
```

## 4. STRIDE per boundary

### B1: Opening project files (`.gus`, `.gus.txt`)
| | Threat | Mitigation |
|---|---|---|
| S | File claims to be from another app version | Version and schema checks; migrations only go forward |
| T | Tampered or torn file | Per-record CRC32C + BLAKE3; damaged files are refused, with the recovery flow (ADR 0005/0007) |
| R | — | — |
| I | File makes the app read other files (paths in imports) | Projects carry blobs *inside* the file; no external paths are dereferenced on open |
| D | Decompression bombs, huge counts, deep nesting | Limits on size, records, decompressed sizes, nesting and statement counts; parsers fuzzed |
| E | Code execution through the file | The document language is declarative and pure (ADR 0004): no I/O, no process, no network, no clock; parsing never executes anything |

### B2: Importing foreign files
| | Threat | Mitigation |
|---|---|---|
| T/D/E | Memory-corruption bugs in C++ parsers (OCCT STEP/IGES); pathological geometry | **Out-of-process import worker** (`gusset-worker`) with OS sandboxing (Linux: seccomp + Landlock, no network; Windows: AppContainer/low integrity, job object limits; macOS: sandbox profile), CPU/memory/time limits. The worker returns kernel-native B-rep bytes, which are validated in the app. A worker crash is a reported error |
| D | Mesh/zip bombs in 3MF and Rust parsers | Limits, ratio checks; fuzzing in CI |
| I | Imported metadata (names, notes) used for prompt injection | Treated as untrusted text everywhere (B4) |
| E | Blender `.blend` scripts | Blender is only ever run with `-b -Y --factory-startup` (auto-run scripts disabled), from an absolute path the user confirmed, with an argument array and no shell |

### B3: Local IPC: MCP server and stdio bridge
| | Threat | Mitigation |
|---|---|---|
| S | Another local user or process connects to the localhost port | Random 256-bit per-session bearer token in a file readable only by the user (0600 / user-only ACL), compared in constant time. On Windows, a named pipe with a user-only DACL is preferred where the client supports it |
| S | DNS rebinding from a browser | Bind 127.0.0.1/::1 only; validate `Host` (localhost names and our port only) and `Origin` (absent, or an explicit allow-list, otherwise 403). rmcp's origin enforcement is switched on explicitly, because its default allow-list is empty and does nothing |
| T | Path escapes through MCP file tools | Paths are canonicalized; `..`, symlink and junction escapes are rejected; access is confined to the project folder and re-checked after open (`openat`-style on Unix, handle path check on Windows) |
| R | Agent denies doing something | Audit log of every agent action, visible in the app and stored in the project event log |
| I | Token leakage | The token is never logged, never in URLs, and rotated per session; the kill switch revokes it immediately |
| D | Flooding | Per-session rate limits, size limits on requests, timeouts |
| E | An agent changes security settings, deletes data permanently, runs commands | No such tools exist. Settings for the updater, MCP, sandbox and permissions are not exposed. Destructive actions need confirm flags and human approval (elicitation or in-app prompt), and agents work on their own branch by default |

### B4: Prompt injection via model content (T5)
Model text (feature labels, notes, parameter comments, imported names, file names) is
untrusted. MCP output returns it only in labeled `user_text` fields, with control and
bidi/zero-width characters stripped, length truncated, and heuristically flagged
(`suspicious: true` + reason). Server instructions and the `security_guidelines`
resource tell agents this. **Server-side enforcement does not depend on the agent
behaving.**

### B5: Updater
| | Threat | Mitigation |
|---|---|---|
| S/T | Fake or modified update | Ed25519 signatures with keys compiled into the app (multiple keys for rotation, plus a signed revocation list) on the manifest and every package, plus SHA-256; TLS (rustls + platform verifier) as defense in depth |
| T | Package swapped between check and install | Downloads go to a private per-user directory under an unpredictable name, the file is opened exclusively, and it is re-verified from the same handle right before install |
| R | — | Local, size-capped update log |
| I | Telemetry | None; only the manifest is fetched |
| D | Freeze/rollback attacks | Manifests expire; strictly-newer version check; TUF-style rules |
| E | Elevation / writable install dirs | Per-user install by default; nothing runs as admin or SYSTEM; no services or tasks; machine-wide installs only to admin-only locations, verified in CI |

### B6: Code loading
DLLs are loaded only from the install directory by absolute path
(`SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_APPLICATION_DIR)`
early in `main`). Linux uses `$ORIGIN`-relative RPATH into the install directory only,
and release builds never honor `LD_LIBRARY_PATH`-like plugin variables of our own.
macOS uses the hardened runtime plus library validation. No plugin system exists yet;
when one is added it gets a WASM sandbox with capabilities.

### B7: Supply chain (T6)
`Cargo.lock` is committed. `cargo-deny` (licenses, advisories, bans, sources) and
`cargo-audit` run in CI; Dependabot is enabled. Every GitHub Action is pinned to a full
SHA; default `permissions: contents: read`. No `pull_request_target`. Release secrets
live only in a `release` environment restricted to tags on main. OCCT source is pinned
by commit and verified by tarball SHA-256. New dependencies are verified by hand (name,
repository, maintainers, downloads, license, advisories).

### B8: Local files, temp files, secrets
Data, cache, recovery, lock and update directories are per-user and owner-only. Temp
files are created with create-new semantics and random names in our own directories or
next to the target. Secrets live only in the OS credential store. Logs and opt-in crash
reports scrub usernames, home paths and secrets.

## 5. Out of scope
- Malware running as the same user with full privileges (it can read the token file).
  We only avoid making it worse.
- Physical attacks.

## 6. Review log
Security reviews per milestone are recorded in `docs/SECURITY_REVIEWS.md`.
