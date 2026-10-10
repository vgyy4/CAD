# Research: MCP, SIMD dispatch, project-file storage, supporting crates, CI runners

- **Date:** 2026-10-10
- **Scope:** Phase 0 research for the Rust-native parametric CAD app (license MIT OR Apache-2.0; no GPL/AGPL linking; LGPL only if dynamically linked).
- **Method:** Everything below was checked on 2026-10-10. Crate data comes from the crates.io API
  (`https://crates.io/api/v1/crates/<name>`), crate tarballs from `static.crates.io` (only read, never built or run),
  and source and docs from `raw.githubusercontent.com`. Advisories come from a shallow clone of `rustsec/advisory-db`
  (last commit 2026-10-09).
- **Blocked sources:** The network policy blocked several primary sites: `modelcontextprotocol.io`, `sqlite.org`, `docs.rs`,
  `docs.github.com`, `github.blog` and `linebender.org`. In those cases I used the same content from the projects' own GitHub
  repositories. Where I fell back to a secondary source, the text says so.

---

## 0. Executive summary (recommendations)

| Area | Recommendation |
|---|---|
| MCP spec target | Implement **2026-07-28** (released 2026-07-28, stateless core). Also stay **dual-era**: keep serving legacy `initialize` clients (2025-11-25, 2025-06-18). A modern-only server breaks legacy clients. Do **not** build on Roots, Sampling or Logging, which are deprecated in 2026-07-28. Use tool parameters and resources, stderr and OpenTelemetry instead. |
| MCP SDK | **`rmcp` 3.5.1** (2026-10-05, Apache-2.0, MSRV 1.88). Features: `server`, `macros`, `schemars`, `transport-io`, `transport-streamable-http-server`, plus `elicitation` if we use it. It implements 2026-07-28 and stays compatible with 2025-11-25 and earlier. The Rust SDK is not "Tier 1"; its 2026-07-28 support is described as beta. |
| MCP transports | **stdio first.** Claude Code launches the app headless as a child process, and the spec says stdio servers take credentials from the environment, not OAuth. **Optional Streamable HTTP** for attaching to the running GUI. Rules for that mode: bind 127.0.0.1 only; keep rmcp's default loopback `allowed_hosts`; call `enforce_origin_validation()`, because the default is *no Origin check*; require a random per-install bearer token; consider a Unix socket or named pipe instead. |
| SIMD | Stable Rust. Baseline `x86-64-v2` (`-C target-cpu=x86-64-v2`) plus runtime dispatch to AVX2+FMA. aarch64 baseline is NEON; dispatch to `dotprod` (intrinsics stable since 1.98). SVE only through LLVM autovectorization behind `#[target_feature(enable="sve")]`, because SVE intrinsics are nightly-only. Use **`fearless_simd` 1.1** (Apache-2.0 OR MIT) for explicit kernels and **`multiversion` 0.9** (MIT OR Apache-2.0) for autovectorized loops. Fall back to raw `std::arch` with safe `#[target_feature]` fns (stable since 1.86) for hot spots. Avoid `std::simd`, which is still nightly. |
| Project file | **SQLite via `rusqlite` 0.40.2 (`bundled`, SQLite 3.53.4, public domain).** Use **rollback journal `DELETE` mode + `synchronous=FULL`** (or `EXTRA`) on the user-visible file. **Never WAL on the user-visible file**, because of its persistent `-wal`/`-shm` sidecars. Save by writing a fresh temp DB in the same directory (`VACUUM INTO` or the backup API), fsync it, then atomically rename it over the target. Use `application_id` plus `user_version` and forward-only migrations. Payload blobs are zstd-compressed with BLAKE3 content hashes. |
| Fallback container | `redb` 4.3.0 is a credible single-file, sidecar-free option, but it is a key-value store whose on-disk format changed in 2025 (v2 to v3) and whose major versions have broken APIs. That makes it weaker for long-term interchange. Not recommended as the primary format. |
| Compression / hashing | `zstd` 0.14.1 (BSD-3-Clause, bundles C libzstd 1.5.7). `ruzstd` 0.9.0 (MIT) only as a pure-Rust decoder: its encoder implements only the "Fastest" level. `blake3` 1.8.7. `crc32c` 0.6.8 still works but has had no release since 2024-06, so prefer `crc-fast` 1.10 or `crc32fast` 1.5.2 (CRC-32, not C) if a CRC is needed. |
| Updates | Sign releases with **minisign** and verify with `minisign-verify` 0.3.0 (MIT). Distribute through GitHub Releases. Consider `velopack` 1.2.161 (MIT) for Windows/macOS/Linux installers and deltas. Evaluate `tough` 0.24 (TUF) later. `self_update` 1.3.0 (MIT) supports zipsign signatures. |
| Packaging | `cargo-dist` 0.32.0 is maintained by the community (governance changed in 2025). `cargo-packager` 0.11.8 has had no release since 2025-11. WiX **v7** (2026-04-06) is under the OSMF EULA: free under US$10k/yr revenue, otherwise a fee. MSIX: Microsoft `winapp` CLI (public preview, v0.3.2 2026-06-11). |
| CI | Public repo: standard runners are free and unlimited. Labels: `ubuntu-24.04`, `ubuntu-24.04-arm`, `windows-2025`, `windows-11-arm`, `macos-15` / `macos-26` (arm64), `macos-15-intel`. **Do not use `macos-14`**: deprecated, brownouts in Oct 2026, unsupported from 2026-11-02. Private repos: 2,000 min/month on Free. |

---

## 1. Model Context Protocol (MCP)

### 1.1 Latest revision: **2026-07-28**

- The official changelog (`docs/specification/2026-07-28/changelog.mdx` in
  `github.com/modelcontextprotocol/modelcontextprotocol`, main branch, read 2026-10-10) lists changes since the
  previous revision, **2025-11-25**.
  Source: https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/docs/specification/2026-07-28/changelog.mdx
- The official blog post "The 2026-07-28 Specification" (https://blog.modelcontextprotocol.io/posts/2026-07-28/) is dated
  2026-07-28 and announces general availability. It reports that the Tier 1 SDKs (TypeScript, Python, Go, C#) support it.
  Per that post, the **Rust SDK supports it in beta**.
- A release candidate was published on 2026-05-21, according to secondary sources (WorkOS:
  https://workos.com/blog/mcp-2026-spec-agent-authentication, and https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/).

**Major changes since 2025-11-25** (from the changelog):

1. **Sessions removed.** Protocol-level sessions and the `Mcp-Session-Id` header are gone from Streamable HTTP. A server
   that needs state across calls mints explicit handles and passes them as tool arguments (SEP-2567).
2. **Handshake removed.** There is no more `initialize` / `notifications/initialized`. Every request carries
   `io.modelcontextprotocol/protocolVersion` and `.../clientCapabilities` in `_meta`. A version mismatch returns
   `UnsupportedProtocolVersionError` (SEP-2575).
3. **New `server/discover` RPC.** Servers MUST implement it. It advertises versions, capabilities, identity and
   `instructions`. Clients may use it as a stdio backward-compatibility probe.
4. **`subscriptions/listen` replaces the HTTP GET stream and `resources/subscribe`/`unsubscribe`.**
5. **Methods removed:** `ping`, `logging/setLevel` and `notifications/roots/list_changed`. The log level is now per request
   (`io.modelcontextprotocol/logLevel` in `_meta`).
6. **Tasks moved to an extension** (`io.modelcontextprotocol/tasks`), with polling via `tasks/get` and a new
   `tasks/update` (SEP-2663).
7. **Multi Round-Trip Requests (MRTR) replace server-initiated requests.** This covers `roots/list`,
   `sampling/createMessage` and `elicitation/create`. The server returns `InputRequiredResult`
   (`resultType: "input_required"`, with `inputRequests`). The client then retries the original request with
   `inputResponses` (SEP-2322).
8. **`resultType` is required on all results.** A result without the field is treated as `"complete"`.
9. **SSE resumability (`Last-Event-ID`) removed.** If a stream breaks, the client re-issues the request.

**Minor changes:**
- `extensions` capability field.
- OpenTelemetry `traceparent` / `tracestate` / `baggage` in `_meta`.
- Deterministic `tools/list` order (SHOULD).
- Required `Mcp-Method` / `Mcp-Name` HTTP headers, plus `x-mcp-header` (SEP-2243).
- `ttlMs` / `cacheScope` on list and read results (SEP-2549).
- The "resource not found" code changes from -32002 to -32602.
- RFC 9207 `iss` validation.
- `application_type` in DCR.
- Issuer-bound client credentials.
- `inputSchema` / `outputSchema` accept any JSON Schema 2020-12 keyword, and `structuredContent` can be any JSON value (SEP-2106).
- URL-mode `notifications/elicitation/complete` and `elicitationId` removed.
- New error-code allocation: `UnsupportedProtocolVersion` = -32022, `MissingRequiredClientCapability` = -32021,
  `HeaderMismatch` = -32020.

### 1.2 Deprecations (verified precisely)

The user's belief is **correct**. The deprecated-features registry
(https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/docs/specification/2026-07-28/deprecated.mdx)
lists:

| Feature | Deprecated in | Migration | Earliest removal |
|---|---|---|---|
| **Roots** | 2026-07-28 (SEP-2577) | Pass directories/files via tool parameters, resource URIs or server config | first revision on/after 2027-07-28 |
| **Sampling** | 2026-07-28 (SEP-2577) | Integrate directly with LLM provider APIs | first revision on/after 2027-07-28 |
| **Logging** | 2026-07-28 (SEP-2577) | stderr for stdio; OpenTelemetry | first revision on/after 2027-07-28 |
| Dynamic Client Registration | 2026-07-28 (PR #2858) | Client ID Metadata Documents | first revision on/after 2027-07-28 |
| `includeContext: "thisServer"/"allServers"` | 2025-11-25 | omit or `"none"` | follows Sampling |
| HTTP+SSE transport | 2025-03-26 | Streamable HTTP | three months after SEP-2596 reaches Final |

"Deprecated" means the feature still works during a window of at least 12 months, but new implementations SHOULD NOT
adopt it (SEP-2596 lifecycle policy).

**Implication for us:** the CAD server must not depend on roots, sampling or logging.

- **Project paths:** take them as explicit tool parameters or resource URIs.
- **Logging:** log to stderr in stdio mode, and use `tracing` with optional OTel.

### 1.3 Compatibility between protocol eras

`basic/versioning.mdx` (2026-07-28) defines three kinds of implementation:

- **modern** (2026-07-28+, per-request `_meta`)
- **legacy** (2025-11-25 and earlier, `initialize`)
- **dual-era**

The compatibility matrix there says:

- A **legacy client talking to a modern-only server fails.** It has no fall-forward mechanism.
- A **dual-era server works with both.**

On stdio, a dual-era server picks its behavior from the first message:

- `server/discover`, or any request carrying modern `_meta`, selects modern semantics.
- `initialize` selects legacy semantics.

**We must be dual-era.** rmcp 3.x already is (see 1.9).

Source: https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/docs/specification/2026-07-28/basic/versioning.mdx

### 1.4 Server instructions

- In 2026-07-28, `instructions` is an optional field of the **`server/discover` result**: "natural-language guidance for
  LLMs on how to use this server effectively". It sits next to `serverInfo`, `capabilities` and the cache hints
  (`server/discover.mdx`).
- On legacy connections it is the `instructions` field of `InitializeResult`.
- **Claude Code** loads server instructions at session start even when tool search defers tool definitions. The docs
  (https://code.claude.com/docs/en/mcp, read 2026-10-10) recommend explaining:
  - which category of tasks the tools handle,
  - when Claude should search for them,
  - the key capabilities.
- **Length limit:** the docs say Claude Code truncates each tool description and each server's instructions at 2,048
  characters by default, configurable with `CLAUDE_CODE_MAX_MCP_DESCRIPTION_LENGTH` (v2.1.280+). The **Claude Code
  changelog for v2.1.296 (2026-10-09)** says this default changed to **4,096**
  (https://code.claude.com/docs/en/changelog). The docs page has not caught up. **Design for at most 2,048 characters**,
  with the critical text first.

### 1.5 Elicitation

- Spec text: `client/elicitation.mdx`, 2026-07-28. There are two modes:
  - **form**: in-band, using a flat JSON-schema object.
  - **url**: out-of-band, for sensitive interactions. Servers **MUST NOT** request secrets or credentials in form mode,
    and **MUST** use URL mode for them.
- An empty `elicitation: {}` capability means form mode only.
- In 2026-07-28, elicitation requests are delivered as **`inputRequests` inside an `InputRequiredResult`** (MRTR). They
  are no longer a server-initiated `elicitation/create` request.
- The `rmcp` feature `elicitation` exists, and the README documents MRTR.
- **Claude Code** supports both modes. On 2026-07-28 connections it declares `elicitation: {form: {}, url: {}}`. Form
  mode shows a dialog. URL mode asks before opening the browser, and the URL is capped at about 8,000 characters.
  An `Elicitation` hook can auto-respond (https://code.claude.com/docs/en/mcp).
- **Use for CAD:** confirm destructive operations, or ask for missing parameters, such as which of two ambiguous edges
  to fillet. Never ask for credentials through it.

### 1.6 Structured tool output

- Tools may declare an `outputSchema`. The result's `structuredContent` can be **any JSON value** in 2026-07-28; it was
  an object in 2025-06-18 and 2025-11-25.
- "For backwards compatibility, a tool that returns structured content SHOULD also return the serialized JSON in a
  TextContent block" (`server/tools.mdx`).
- Tool annotations (`readOnlyHint`, `destructiveHint`, ...) are untrusted hints for clients.
- **Claude Code specifics** (from the MCP docs page):
  - Input schemas must be valid JSON Schema 2020-12.
  - Top-level property names must be 1–64 characters of `[A-Za-z0-9_.-]`.
  - Root-level `anyOf` / `oneOf` / `allOf` gets flattened.
  - `_meta["anthropic/maxResultSizeChars"]` (up to 500,000) raises the result-size threshold.
  - `_meta["anthropic/alwaysLoad"]` exempts a tool from deferral.
  - `_meta["anthropic/requiresUserInteraction"]: true` forces a human approval prompt on every call. This is useful for
    our "delete file" or "overwrite project" tools.
  - `MAX_MCP_OUTPUT_TOKENS` defaults to 25,000 tokens.
  - Image results (PNG, JPEG, GIF, WebP) are shown inline and saved to disk. This is useful for viewport screenshots.

### 1.7 Streamable HTTP security requirements (2026-07-28)

Source: `basic/transports/streamable-http.mdx`, "Security Warning":

1. Servers **MUST validate the `Origin` header** on all incoming connections to prevent DNS rebinding. If the header is
   present and invalid, the server MUST respond **HTTP 403**.
2. When running locally, servers **SHOULD bind only to localhost (127.0.0.1)**, not 0.0.0.0.
3. Servers **SHOULD implement proper authentication** for all connections.

Other requirements:

- POST requests MUST carry `Accept: application/json, text/event-stream`, plus the standard `Mcp-Method` / `Mcp-Name`
  and `MCP-Protocol-Version` headers.
- Servers MUST NOT send independent JSON-RPC requests on response streams.

**Real-world precedent:**

- **RUSTSEC-2026-0189** (CVE-2026-42559, 2026-04-29): **rmcp < 1.4.0** did not validate `Host`, which allowed DNS
  rebinding against local rmcp HTTP servers. The fix added default loopback-only host checks.
- **RUSTSEC-2026-0140** (dynoxide-rs) shows that a **Host check alone is insufficient**. A `fetch(..., {mode:'no-cors'})`
  from a hostile page sends a matching Host while the Origin goes unchecked. That is a cross-origin CSRF.
- Source: `rustsec/advisory-db` `crates/rmcp/RUSTSEC-2026-0189.md` and `crates/dynoxide-rs/RUSTSEC-2026-0140.md`.

**rmcp 3.5.1 defaults** (verified in the published crate source, `src/transport/streamable_http_server/tower.rs`):

- `allowed_hosts` defaults to `["localhost", "127.0.0.1", "::1"]`. Good.
- `allowed_origins` **defaults to empty, which DISABLES Origin validation "for backward compatibility"**. Requests
  without an `Origin` header always pass.
- `StreamableHttpServerConfig::enforce_origin_validation()` turns validation on with an empty allowlist, so every
  request that *carries* an Origin is rejected. Non-browser MCP clients such as Claude Code send no Origin, so they
  still work, while every browser-originated request is refused.
- **We must call `enforce_origin_validation()`**, and add a bearer token on top.

### 1.8 Authorization guidance for local servers

- `basic/authorization/index.mdx` (2026-07-28) says:
  - Authorization is OPTIONAL.
  - HTTP-based transports SHOULD follow the OAuth 2.1-based spec.
  - **"Implementations using an STDIO transport SHOULD NOT follow this specification, and instead retrieve credentials
    from the environment."**
- Security best practices (`docs/docs/2026-07-28/tutorials/security/security_best_practices.mdx`, "Local MCP Server
  Compromise") says MCP servers intended to run locally SHOULD prevent unauthorized use by malicious processes:
  - "Use the `stdio` transport to limit access to just the MCP client".
  - If using HTTP, "Require an authorization token" or "Use unix domain sockets or other IPC mechanisms with restricted
    access".
- **Our design:**
  - **stdio mode:** no auth. The parent process is the client.
  - **HTTP mode, attaching to the GUI:**
    - Generate a 256-bit random token per install or per launch, using the OS CSPRNG.
    - Store it in the OS keychain (`keyring`), or in a user-only (0600) file.
    - Require `Authorization: Bearer <token>` and compare it in constant time.
    - Bind 127.0.0.1 and enforce Origin as in 1.7.
    - The user wires it up with `claude mcp add --transport http cad http://127.0.0.1:<port>/mcp --header "Authorization: Bearer ${CAD_MCP_TOKEN}"`,
      or with `headersHelper` (see 1.10).
  - Full OAuth is not warranted for loopback.

### 1.9 `rmcp` crate

- **Latest version:** **3.5.1**, published **2026-10-05**.
- **License:** Apache-2.0. **MSRV:** 1.88.
- **Repository:** https://github.com/modelcontextprotocol/rust-sdk (the official SDK).
- **Downloads:** 34.2 M total, 18.7 M recent (crates.io API).
- **Release history:** 3.5.0 (2026-09-28), 3.4.1 (09-23), 3.4.0 (09-15), 3.3.0 (09-10), 3.2.0 (08-31). Very active.
- **Changelog:** https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/CHANGELOG.md. Recent
  fixes include:
  - Origin allowlist port matching (3.5.0),
  - rejecting duplicate SEP-2243 headers,
  - default cache hints for 2026-07-28 peers (3.5.1).
- **Protocol versions** (from `src/model.rs` on main):
  - `KNOWN_VERSIONS` = 2024-11-05, 2025-03-26, 2025-06-18, 2025-11-25, 2026-07-28.
  - `LATEST` = `V_2026_07_28`.
  - `LATEST_WITH_INITIALIZE` = `V_2025_11_25`, added in 3.5.0.
  - The README states it "implements the stable MCP 2026-07-28 specification while remaining fully compatible with the
    2025-11-25 release and earlier".
  - On HTTP it serves 2026-07-28 statelessly automatically. `with_legacy_session_mode(false)` makes legacy clients
    stateless too. `with_stateless_protocol_metadata_required(true)` makes the server modern-only; **don't use it**.
- **Cargo features** (rmcp 3.5.1 `[features]`):

| Feature | Purpose |
|---|---|
| `server` (default) | Server role. Pulls in `transport-async-rw`, `schemars`, `uuid`. |
| `macros` (default) | `#[tool]`, `#[tool_router]`, etc. via `rmcp-macros`. |
| `schemars` | JSON Schema 2020-12 generation. |
| `transport-io` | stdio. |
| `transport-streamable-http-server` | Tower `StreamableHttpService`; mount it in axum. Includes `server-side-http`. |
| `transport-child-process` | Client side. Useful for our integration tests. |
| `client`, `transport-streamable-http-client(-reqwest)` | For test harnesses. |
| `elicitation`, `request-state` | Elicitation helpers. MRTR `requestState` HMAC sealing. |
| `auth`, ... | OAuth client side. Not needed. |

- **Advisories:** RUSTSEC-2026-0189 (fixed in 1.4.0). Nothing open against 3.x.

### 1.10 Claude Code: protocol revision and configuration

- **Protocol revision**, from the Claude Code changelog (https://code.claude.com/docs/en/changelog, newest entry
  2.1.296, 2026-10-09):
  - **2.1.292:** "Changed local (stdio) MCP server connections to negotiate protocol version **2026-07-28 by default** on
    every install ...; `MCP_PROTOCOL_NEGOTIATION=legacy` opts out."
  - 2.1.292 also: stdio servers that ignore the newer protocol probe are remembered for 7 days after one slow connect,
    and connected the older way. **So our server must answer `server/discover` promptly**, either with a result or with
    an error.
  - **2.1.295:** claude.ai connectors negotiate 2026-07-28 by default.
  - MCP docs: on the v2 client runtime (`MCP_SDK_GENERATION=v2`), HTTP, stdio and claude.ai connector servers are asked
    whether they support 2026-07-28. Other servers connect over the earlier `initialize` handshake.
  - **Conclusion:** current Claude Code requests 2026-07-28 and falls back to the legacy handshake. A dual-era rmcp
    server works either way.
- **Configuration** (https://code.claude.com/docs/en/mcp):

  ```bash
  # stdio (Claude Code spawns the app headless)
  claude mcp add --transport stdio cad -- /path/to/cad-app --mcp-stdio
  claude mcp add --env CAD_PROJECT_DIR=/work/proj --transport stdio cad -- cad-app --mcp-stdio
  # Streamable HTTP (attach to running GUI)
  claude mcp add --transport http cad http://127.0.0.1:47811/mcp \
    --header "Authorization: Bearer <token>"
  # scope: -s local (default) | project (writes .mcp.json) | user
  ```

  `.mcp.json` supports `${VAR}` and `${VAR:-default}` in `command`, `args`, `env`, `url` and `headers`. Example:

  ```json
  {"mcpServers":{"cad":{"type":"http","url":"http://127.0.0.1:47811/mcp",
    "headers":{"Authorization":"Bearer ${CAD_MCP_TOKEN}"}}}}
  ```

  Other options:
  - `headersHelper`: a command that prints a JSON header map. It runs on each connection, has a 10 s timeout, and runs
    for project scope only after folder trust.
  - `type` also accepts `streamable-http` as an alias for `http`.
  - Timeouts: `MCP_TIMEOUT` (startup), `MCP_TOOL_TIMEOUT`, and a per-server `"timeout"`.
  - The `--` separator: everything after it goes to the server untouched. Put another option between `--env` and the
    server name.

---

## 2. CPU-feature runtime dispatch and SIMD on stable Rust (Oct 2026)

**Facts** (from Rust `RELEASES.md` at https://raw.githubusercontent.com/rust-lang/rust/master/RELEASES.md, newest
version 1.99.0 of 2026-10-01, unless noted):

| Item | Status |
|---|---|
| `is_x86_feature_detected!` | Stable for years (1.27). |
| `is_aarch64_feature_detected!` | Stable since **1.60.0** (2022-04-07). Feature strings `dotprod`, `i8mm`, `sve` and `sve2` are stable (std_detect source, `#[stable(feature="simd_aarch64", since="1.60.0")]`). |
| `#[target_feature]` with aarch64 features | Stable since **1.61.0**. |
| **target_feature 1.1** (safe fns may carry `#[target_feature]`) | **Stabilized in 1.86.0** (2025-04-03). See https://blog.rust-lang.org/2025/04/03/Rust-1.86.0/. Such fns are safe to call only from contexts with the same features. Elsewhere the call needs `unsafe` after runtime detection. They can't be passed as `Fn*`. |
| Safe `std::arch` intrinsics | 1.87.0: "Most `std::arch` intrinsics which don't take pointer arguments can now be called from safe code if the caller has the appropriate target features already enabled". |
| AVX-512 | Target features and intrinsics stable since **1.89.0** (2025-08-07). `avx512fp16` intrinsics stable since 1.94.0. |
| NEON dot-product intrinsics (`vdotq_s32`, ...) | Stable on **aarch64 since 1.98.0** (2026-08-20) per stdarch source (`stable(feature="stdarch_neon_dotprod", since="1.98.0")`). Still unstable on 32-bit arm. |
| SVE intrinsics | **Nightly only**: feature `stdarch_aarch64_sve`, tracking issue rust-lang/rust#145052. The 2026 project goal covers nightly experimentation. Sources: https://rust-lang.github.io/rust-project-goals/2025h1/arm-sve-sme.html, https://blog.rust-lang.org/2026/05/18/project-goals-2026-04/, and the open initial-intrinsics PR https://github.com/rust-lang/stdarch/pull/2071. |
| `std::simd` (portable_simd) | **Still nightly-only** in Oct 2026, per the "State of SIMD in Rust 2026" survey (https://shnatsel.github.io/state-of-simd-rust-2026/, excerpts via search) and secondary coverage (https://news.lavx.hu/article/rust-simd-reaches-a-practical-turning-point-in-2026). |

**Crates** (crates.io API, 2026-10-10):

| Crate | Version (date) | License | Notes |
|---|---|---|---|
| `fearless_simd` | **1.1.0** (2026-10-06); 1.0.0 on 2026-09-21 | Apache-2.0 OR MIT | MSRV 1.89. Linebender. Zero deps. `Level::new()` plus `dispatch!` and the optional `#[simd]` attribute (`fearless_simd_macros` 0.1.0). Levels: x86 SSE2 / **v2 (SSE4.2)** / **v3 (AVX2)** / AVX-512 (Ice Lake+); aarch64 **NEON**; WASM SIMD128 at compile time. Safe raw NEON and x86 intrinsics through level tokens. No SVE; trig is missing. Source: https://raw.githubusercontent.com/linebender/fearless_simd/main/fearless_simd/README.md |
| `multiversion` | **0.9.0** (2026-09-06) | MIT OR Apache-2.0 | MSRV 1.86. `#[multiversion(targets("x86_64+avx2+fma", "x86_64+sse4.2", "aarch64+neon+dotprod", "aarch64+sve"))]` clones a function per target and dispatches at runtime. It is the only crate where you list exact extension sets, which makes it best for autovectorized loops. |
| `pulp` | 0.22.3 (2026-06-20) | MIT | Powers `faer`. Built-in multiversioning, native-width only, more boilerplate. |
| `wide` | 1.7.1 (2026-09-14) | Zlib OR Apache-2.0 OR MIT | Mature fixed-width types. **No runtime dispatch**: it uses compile-time features only. |
| `std::simd` | nightly | — | Not usable on stable. |

**Recommendation for stable Rust:**

1. **Baselines.**
   - x86-64: ship with `-C target-cpu=x86-64-v2` (SSE4.2 + POPCNT + SSSE3). This is set in `.cargo/config.toml`
     `[target.x86_64-*] rustflags`, and documented as the minimum CPU.
   - aarch64: the default target already includes NEON.
2. **Hot numeric kernels** (mesh tessellation, BVH traversal, point-cloud ops, distance queries) are written once against
   **`fearless_simd`**'s generic `Simd` trait and dispatched with `Level::new()`, cached once at startup. They then
   execute v2, v3 (AVX2+FMA), AVX-512 or NEON automatically.
3. **Plain loops that LLVM autovectorizes well** (array transforms, reductions) use **`multiversion`** with explicit
   targets: `x86_64+avx2+fma`, `aarch64+dotprod`, `aarch64+sve`. This is also the **only stable path to SVE code
   generation**: `#[target_feature(enable = "sve")]` is stable, and LLVM can autovectorize to SVE, even though SVE
   intrinsics are nightly.
4. **For a few intrinsic-level kernels** (e.g. `vdotq_s32` on aarch64, available since 1.98): use safe
   `#[target_feature(enable = "...")] fn` (target_feature 1.1). Make the single `unsafe` call site after an
   `is_*_feature_detected!` check, wrapped in a small dispatch module with a `// SAFETY:` comment.
5. **Geometry kernel robustness:**
   - Never let SIMD paths change results in exact predicates. FMA changes rounding, so robust orientation and
     incircle predicates must have identical results across levels, or use FMA-free code.
   - Test every kernel at every level in CI. fearless_simd lets you force a level, and `--cfg disable_dispatch_avx2`
     etc. exist.
   - Cover AVX2 on x86 runners and NEON on `ubuntu-24.04-arm` / `windows-11-arm` / `macos-15`.

---

## 3. Project file container

### 3.1 SQLite (`rusqlite`)

- **`rusqlite` 0.40.2** (2026-08-08), MIT, 122 M downloads. It uses **`libsqlite3-sys` 0.38.2**. With the `bundled`
  feature it compiles the **SQLite 3.53.4** amalgamation (libsqlite3-sys `sqlite3/sqlite3.h` on master; the
  `upgrade.sh` script pulls `sqlite-amalgamation-3530400`).
- **SQLite license:** public domain ("SQLite Is Public Domain", sqlite.org/copyright.html). Compatible with MIT OR
  Apache-2.0, though `cargo-about` needs a manual clarification entry.
- **Long-term support:** SQLite's developers state support until **2050**, and the file format is "well documented and
  stable" (sqlite.org/lts.html). It is a **US Library of Congress recommended storage format** for datasets
  (sqlite.org/locrsf.html).
- **Advisories:** RUSTSEC-2020-0014, RUSTSEC-2021-0128 and RUSTSEC-2022-0090 are all long patched. Nothing open against
  0.40 / 0.38.

> **Sourcing note:** sqlite.org is blocked by our network policy. I read the identical documentation from the Debian
> package mirror `pexip/os-sqlite3` (`www/*.html`, a copy of SQLite ~3.46 docs):
> https://raw.githubusercontent.com/pexip/os-sqlite3/HEAD/www/howtocorrupt.html (and `wal.html`, `pragma.html`,
> `lang_vacuum.html`, `copyright.html`, `lts.html`, `locrsf.html`). Re-check against sqlite.org when it is reachable.

**Journal modes** (`PRAGMA journal_mode`):

| Mode | Behavior | Sidecars |
|---|---|---|
| **DELETE** (default) | Rollback journal `<db>-journal`, **deleted at the end of each transaction**. "The delete operation is the action that causes the transaction to commit." | `-journal` exists only during a write transaction, or after a crash as a "hot journal". |
| TRUNCATE / PERSIST | Journal kept (zero-length or zero-header). | Leaves a persistent `-journal` file. Bad for us. |
| MEMORY / OFF | Journal in RAM or none. | An app crash mid-write can corrupt the database (howtocorrupt §7). Never use. |
| **WAL** | Write-ahead log. | `-wal` and `-shm` exist while any connection is open. Usually deleted when the last connection closes, **but retained if the process exits uncleanly** or with `SQLITE_FCNTL_PERSIST_WAL`. The WAL file "is part of the persistent state of the database". WAL also does not work over network filesystems, needs write access for `-shm` even when reading, and can't change `page_size`. SQLite's own docs: the "quasi-persistent -wal file and -shm shared memory file ... can make SQLite less appealing for use as an application file-format". |

**`synchronous` levels** (`PRAGMA synchronous`):

- **EXTRA (3):** like FULL, plus the directory containing a rollback journal is synced after the journal is unlinked
  (DELETE mode). That gives extra durability if power is lost right after commit.
- **FULL (2):** syncs so that "an operating system crash or power failure will not corrupt the database". The usual
  setting outside WAL. howtocorrupt says SQLite "should always be run with its default synchronous setting of FULL" for
  maximum robustness.
- **NORMAL (1):** WAL is always consistent with it but may lose the last transactions on power loss. In DELETE mode
  there is a "very small (though non-zero) chance" of corruption on older filesystems.
- **OFF (0):** corruption is possible on OS crash or power loss.

**`PRAGMA fullfsync` and `checkpoint_fullfsync`:**

- These use `F_FULLFSYNC` on macOS. They are off by default and macOS-only. Turn **`fullfsync=ON`** on for the project
  file on macOS, because plain `fsync` on macOS does not flush the drive cache.
- Rust's own `File::sync_all` / `sync_data` already use `fcntl(F_FULLFSYNC)` on Apple platforms (rust
  `library/std/src/sys/fs/unix.rs`, `os_fsync`), which matters for our own temp-file writes.

**"How To Corrupt An SQLite Database File"** (section list and summary):

1. **File overwrite by a rogue thread or process.** Examples:
   - 1.1 Continuing to use a closed fd that got reused for the DB.
   - 1.2 Backup or restore while a transaction is active. Use the backup API, or copy only when quiescent, *with* the
     journal or WAL.
   - 1.3 **Deleting a hot journal** (`-journal`/`-wal`) after a crash, which breaks recovery.
   - 1.4 **Mispairing DB and journal files**: swapping or moving journals, copying a DB without its journal, or
     overwriting a DB without deleting its hot journal.
2. **File-locking problems:**
   - 2.1 Broken or missing locks (notably network filesystems).
   - 2.2 POSIX advisory locks are cancelled when *any* fd on the file is closed by another thread.
   - 2.3 Mixed locking protocols.
   - 2.4 Unlinking or renaming a DB while it is in use.
   - 2.5 Multiple hard or soft links to one file.
   - 2.6 Carrying a connection across `fork()`.
3. **Failure to sync:**
   - 3.1 Drives that lie about sync, USB sticks especially. WAL is more forgiving: only a checkpoint is vulnerable.
   - 3.2 Disabling sync via PRAGMA.
4. **Disk and flash failures:** non-powersafe flash controllers, fake-capacity USB sticks.
5. **Memory corruption** by other code in the process. Relevant because we link C code.
6. **OS problems:** old Linux threads, QNX mmap, filesystem corruption.
7. **Configuration errors:** `synchronous=OFF`; changing `schema_version` with other connections open;
   `journal_mode=OFF/MEMORY` plus an app crash; `writable_schema` misuse.
8. **Historical SQLite bugs**, all fixed: a shrinkage false positive, rollback/WAL switching, an I/O error during lock,
   free-list leak, a 3.6/3.7 mix, a Windows recovery race, nested-transaction journal boundary.

**Consequences for a user-visible document file:**

- Users copy, sync (Dropbox, OneDrive), email and rename `.cad` files freely. Every one of 1.2, 1.3, 1.4, 2.4 and 2.5
  becomes likely if the user-visible file is a *live* database with sidecars.
- So the user-visible file should be **quiescent and self-contained** whenever the app is not in the middle of saving.

### 3.2 `redb`

- **`redb` 4.3.0** (2026-09-15), MIT OR Apache-2.0, MSRV 1.90, 13 M downloads. Author cberner.
- **Releases:** 4.0.0 (2026-04-02), 4.1, 4.2, 4.3; a 2.6.4 backport on 2026-10-02.
- **README:** "Status: Stable and maintained. The file format is stable, and a reasonable effort will be made to provide
  an upgrade path if there are any future changes to it." Source: https://raw.githubusercontent.com/cberner/redb/master/README.md
- **Crash safety** (`docs/design.md`):
  - Copy-on-write B+trees, Merkle-checksummed with XXH3-128.
  - Default "1PC+C" commit uses a **single fsync**. A partially committed transaction is detected by checksum and rolled
    back.
  - Optional non-durable commits.
  - Quick-repair or full repair on open after a crash.
- **Sidecars:** none. It is a single file. Locking uses OS file locks on the database file itself (std
  `File::try_lock` / range locks, from `src/tree_store/page_store/file_backend/*` in 4.3.0). `Database::compact()` exists.
- **History caveats:**
  - **File format v2 was dropped in 3.0.0** (2025-08-09). Migration was via `Database::upgrade()` in 2.6.
  - 4.0.0 fixed a **data-loss bug** in `get_mut` / `insert_reserve` guards (3.1.3 / 4.0.0, 2026-04-02).
  - The 5.0.0 changelog in progress (unreleased) mentions migrating tables by copying them, but "the file format is
    unchanged".
  - So the format moves on a roughly 1-year cadence, and readers need the matching redb major version.
- **Fit:**
  - Good: crash safety, no sidecars, pure Rust, fast.
  - Weaker: it is a KV store only, with no SQL, `integrity_check` or ad-hoc inspection. There is no 25-year format
    commitment, and a third party can't open the file without our code. Its documented upgrade path relies on running
    old redb code.

### 3.3 Custom chunked format

A custom format would look like this:

- A header with magic, version and a table-of-contents offset.
- Append-only chunks, each with type, length, zstd payload and BLAKE3 hash.
- A footer TOC.
- Writes always produce a new file plus an atomic rename.

What this costs:

- **Pros:** full control, minimal size, trivially sidecar-free, simple to fuzz.
- **Cons:** we own all the crash-safety, migration and tooling work. Incremental saves of large projects mean rewriting
  the whole file or building our own free-space and journalling layer, which is exactly what SQLite and redb already
  give us.

### 3.4 Recommendation: SQLite container, written by "build-then-rename"

**Chosen design:** the user-visible `.cad` file is a SQLite database (rollback-journal format), produced atomically.

1. **Working copy.**
   - On open, copy the user's file into the app's per-project working directory (`dirs` data/cache dir) with the
     SQLite **backup API**.
   - Edit there with WAL + `synchronous=NORMAL` for fast autosave and crash recovery. Sidecars there are invisible to
     the user.
   - Hold an exclusive app lock (std `File::try_lock`, stable since 1.89) on a lock file in the working dir.
   - Write a separate "document open" marker for multi-instance detection.
2. **Save.**
   - `VACUUM INTO '<target-dir>/.<name>.cad.tmp-<rand>'`. This produces a compact, defragmented, single-file DB in
     `journal_mode=DELETE`.
   - Then `fsync` that file. **`VACUUM INTO` does not call fsync itself**, per the SQLite VACUUM docs.
   - Then atomic `rename` over the target. On Windows, std `fs::rename` replaces atomically on NTFS.
   - Then fsync the directory on Unix.
   - Optionally keep the previous file as `.bak` or send it to the OS trash with the `trash` crate.
   - Result: **no sidecar is ever left next to the user's file**. A crash mid-save leaves the old file intact plus a
     stray `.tmp-*`, which we clean up on next start (only names matching our pattern).
3. **Opening a user file directly** (read-only import or "repair") uses `?mode=ro&immutable=1` URIs only on read-only
   media. Normally it goes through the backup API into the working copy, so the user's file is never opened read-write
   in place, and we never leave `-journal` / `-wal` next to it.
4. **Pragmas on the shipped file:**
   - `PRAGMA application_id = <our 32-bit magic>`, so `file(1)` and our sniffer can recognise it.
   - `PRAGMA user_version = <schema version>`.
   - `page_size = 4096` (or 8192), `journal_mode = DELETE`, `auto_vacuum = NONE`. VACUUM INTO compacts anyway.
   - Run `PRAGMA integrity_check` (or `quick_check`) on open as a background verification step.
5. **Schema:**
   - Normalised tables for the feature tree, parameters, sketches and metadata.
   - Large payloads (B-rep, tessellation caches, thumbnails) are **zstd-compressed BLOBs keyed by BLAKE3 hash**
     (content-addressed, deduplicated).
   - Derived caches live in separate tables that can be dropped without data loss, which keeps files small ("save
     without caches" option).
   - Text columns stay human-inspectable (JSON) where size allows, which helps long-term readability.
6. **Migrations:**
   - Forward-only, numbered SQL plus Rust migrations, keyed by `user_version`, using `rusqlite_migration` 2.6.0
     (Apache-2.0) or a ~100-line in-house runner.
   - Always migrate the *working copy*, never the user's original.
   - Refuse to open files whose version is newer than we understand, with a read-only fallback.
   - Keep golden test fixtures of every released schema version forever.

**How this meets the criteria:**

| Criterion | How it is met |
|---|---|
| Crash safety | Atomic rename. Working copy in WAL. FULL or EXTRA sync where it matters. |
| Low storage | VACUUM INTO compaction, zstd blobs, droppable caches. |
| Fast open/save | Backup API and VACUUM INTO run at disk speed. Incremental autosave in WAL. |
| Clean deletion | One file, no sidecars at rest. |
| Portability | SQLite runs everywhere, and the file opens with any SQLite tool. |
| Format stability | SQLite's 2050 commitment plus our `user_version` migrations. |

---

## 4. Supporting crates

All rows come from the crates.io API on 2026-10-10, using the max stable version. Advisories come from the RustSec
advisory-db clone. "Recent" means downloads in the last 90 days.

| Crate | Version (date) | License | Downloads total / recent | Repo | Notes |
|---|---|---|---|---|---|
| `zstd` | 0.14.1 (2026-10-09) | BSD-3-Clause | 418 M / 92 M | gyscos/zstd-rs | `zstd-sys` 2.1.1+zstd.1.5.7 bundles C libzstd. Upstream is dual BSD/GPLv2; we take BSD. Needs a C compiler. |
| `ruzstd` | 0.9.0 (2026-07-26) | MIT | 71 M / 18 M | KillingSpark/zstd-rs | Pure Rust. Full decoder, about 1.4–3.5x slower than C. **Encoder: only `Fastest` (~zstd level 1) and `Uncompressed` are implemented**; Default/Better/Best are marked UNIMPLEMENTED in 0.9.0 `src/encoding/mod.rs`. RUSTSEC-2024-0400 is fixed in ≥0.7.3. |
| `blake3` | 1.8.7 (2026-08-20) | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | 198 M / 50 M | BLAKE3-team/BLAKE3 | Has its own runtime SIMD dispatch. |
| `crc32c` | 0.6.8 (2024-06-09) | Apache-2.0/MIT | 88 M / 18 M | zowens/crc32c | Works, but no release in more than 2 years. Alternatives: `crc-fast` 1.10.0 (2025-12-31, MIT OR Apache-2.0; CRC-32C and others, SIMD) or `crc32fast` 1.5.2 (2026-09-12; CRC-32/IEEE). |
| `xxhash-rust` | 0.8.19 (2026-09-28) | **BSL-1.0** | 115 M / 35 M | DoumanAsh/xxhash-rust | BSL-1.0 is permissive but must be added to the cargo-deny allow list. |
| `trash` | 5.2.9 (2026-09-13) | MIT | 3.4 M / 1.1 M | ArturKovacs/trash | Moves files to the OS Recycle Bin, Trash or freedesktop trash. |
| `fs4` | 1.1.0 (2026-04-28) | MIT OR Apache-2.0 | 72 M / 19 M | al8n/fs4 | File locks and allocation. **Std `File::lock`/`try_lock`/`lock_shared` have been stable since Rust 1.89**, so we likely need neither fs4 nor fd-lock. |
| `fd-lock` | 4.0.4 (2025-03-10) | MIT OR Apache-2.0 | 67 M / 15 M | yoshuawuyts/fd-lock | As above. |
| `tempfile` | 3.27.0 (2026-03-11) | MIT OR Apache-2.0 | 876 M / 205 M | Stebalien/tempfile | `NamedTempFile::new_in(dir)` plus `persist()` gives atomic replace. |
| `directories` | 6.0.0 (2025-01-12) | MIT OR Apache-2.0 | 74 M / 14 M | soc/directories-rs | The old RUSTSEC-2020-0054 "unmaintained" notice is **withdrawn**. |
| `dirs` | 7.0.0 (2026-09-05) | MIT OR Apache-2.0 | 357 M / 89 M | **codeberg.org/dirs/dirs-rs** | Repo moved to Codeberg. RUSTSEC-2020-0053 is withdrawn. Prefer `directories` (`ProjectDirs`) for app-specific paths. |
| `atomicwrites` | 0.4.4 (2024-09-19) | MIT | 20 M / 3 M | untitaker/rust-atomicwrites | Slow cadence. `tempfile::persist` plus an explicit fsync covers our needs. |
| `minisign-verify` | 0.3.0 (2026-09-25) | MIT | 16 M / 7 M | jedisct1/rust-minisign-verify | Zero-dep verifier for minisign signatures. **Recommended** for update-manifest verification. |
| `ed25519-dalek` | 3.0.0 (2026-07-06) | BSD-3-Clause | 233 M / 66 M | dalek-cryptography | RUSTSEC-2022-0093 is fixed in ≥2. Use it only if we need raw Ed25519. |
| `tough` | 0.24.0 (2026-07-10) | MIT OR Apache-2.0 | 2.6 M / 0.7 M | awslabs/tough | TUF client. Strongest update security (rollback and freeze protection), but heavier. RUSTSEC-2020-0024 is fixed. |
| `self_update` | 1.3.0 (2026-09-02) | MIT | 12 M / 2.4 M | jaemk/self_update | GitHub Releases backend. `signatures` feature (zipsign) and `checksums` (SHA-256/512). macOS bundle-aware `verify_binary` hook. |
| `velopack` | 1.2.161 (2026-09-29) | MIT | 0.12 M / 0.04 M | velopack/velopack | Real crate: "Installer and automatic update framework for cross-platform desktop applications". Delta updates. The packaging CLI `vpk` is a separate tool; **verify its distribution channel before CI use**. |
| `cargo-packager` | 0.11.8 (2025-11-27) | Apache-2.0 OR MIT | 0.2 M | crabnebula-dev/cargo-packager | Updater crate `cargo-packager-updater` 0.2.3 (2025-07-21), with signing keys from `cargo packager signer generate` (https://docs.crabnebula.dev/cloud/auto-updates/packager/). Quiet for about 10 months. |
| `keyring` | 4.2.0 (2026-08-29) | MIT OR Apache-2.0 | 30 M / 14 M | open-source-cooperative/keyring-rs | **v4 restructure:** the API moved to `keyring-core` 1.0.0 (2026-04-21), with stores in separate crates. The `keyring` crate's default `v1` feature keeps the old API. For new code, use `keyring-core` plus the platform store crates. |
| `rustls-platform-verifier` | 0.7.1 (2026-09-24) | MIT OR Apache-2.0 | 171 M / 85 M | rustls | Uses the OS trust store. Recommended with rustls. |
| `ureq` | 3.4.2 (2026-09-13) | MIT OR Apache-2.0 | 224 M / 68 M | algesten/ureq | Blocking and small. Good for the update check. |
| `reqwest` | 0.13.5 (2026-09-08) | MIT OR Apache-2.0 | 786 M / 212 M | seanmonstar/reqwest | Async. Already pulled by `rmcp` only with client features. |
| `accesskit` | 0.25.1 (2026-09-25) | MIT OR Apache-2.0 | 28 M / 7 M | AccessKit/accesskit | UI accessibility tree. Used by egui, Xilem, etc. |
| `image` | 0.25.10 (2026-03-10) | MIT OR Apache-2.0 | 209 M / 59 M | image-rs/image | Old advisories only (2019/2020). Disable unneeded codecs. |
| `png` | 0.18.1 (2026-02-14) | MIT OR Apache-2.0 | 254 M / 84 M | image-rs/image-png | Thumbnails and screenshots. |
| `tiny-skia` | 0.12.0 (2026-02-02) | BSD-3-Clause | 54 M / 18 M | linebender/tiny-skia | CPU 2D rasteriser for drawings and thumbnails. |
| `stl_io` | 0.11.0 (2026-03-15) | MIT | 5 M / 1 M | hmeyer/stl_io | STL read and write. |
| `threemf` | 0.8.0 (2026-03-05) | 0BSD | 45 k / 6 k | hannobraun/3mf-rs | Low adoption. Expect to contribute or extend it. |
| `tobj` | 4.0.5 (2026-08-02) | MIT | 2.9 M / 0.7 M | Twinklebear/tobj | OBJ loader. |
| `gltf` | 1.4.1 (2024-05-10) | MIT OR Apache-2.0 | 10 M / 2.3 M | gltf-rs/gltf | No release in about 2.4 years, but still widely downloaded. Acceptable for export (or write glTF JSON ourselves with `serde_json`). |
| `rhai` | 1.26.1 (2026-09-10) | MIT OR Apache-2.0 | 13 M / 4.4 M | rhaiscript/rhai | See below. |
| `boa_engine` | 0.22.0 (2026-08-28) | Unlicense OR MIT | 5 M / 1.6 M | boa-dev/boa | Pure-Rust JS engine. Pre-1.0. RUSTSEC-2024-0444 is fixed in ≥0.19. |

**Sandboxed scripting / "code CAD" language.** The rhai README
(https://raw.githubusercontent.com/rhaiscript/rhai/main/README.md) claims:

- "Don't panic" guarantee.
- Sandboxed: an immutable engine "cannot mutate the containing environment unless explicitly permitted".
- Limits on call-stack depth, string, array and map sizes, and **max operations** (runaway scripts). It also offers
  progress callbacks for cancellation, and can disable keywords and looping.
- **Rhai has no built-in file or network I/O.** All capabilities come from functions we register, which makes it
  capability-based.
- Passes Miri.

Alternatives:

| Crate | Version (date) | License | Notes |
|---|---|---|---|
| `wasmtime` | 49.0.2 (2026-10-02) | Apache-2.0 WITH LLVM-exception | Strongest isolation: WASM, fuel/epoch limits. Many advisories; 49.0.2 fixes RUSTSEC-2026-0315/0316/0325/0326/0327. **Must track patch releases closely.** |
| `wasmi` | 2.0.0 (2026-09-01) | MIT/Apache-2.0 | Interpreter, fuel metering, smaller attack surface. |
| `mlua` | 0.12.2 | MIT | Lua/Luau. Luau has a sandbox mode. Binds C. |
| `rquickjs` | 0.14.0 | MIT | QuickJS, C. |
| `rune` | 0.14.2 | MIT OR Apache-2.0 | Quiet. |
| `koto` | 0.16.1 | MIT | Small user base. |
| `starlark` | 0.14.2 | Apache-2.0 | Deterministic, hermetic, Python-like. Attractive for reproducible parametric models. |

**Recommendation:** Rhai for user macros and parametric "code CAD" (pure Rust, op limits, no ambient I/O). Keep the
door open to WASM (wasmi or wasmtime) for third-party plugins needing stronger isolation. Starlark is the runner-up if
we need deterministic, hermetic evaluation semantics.

**Tooling:**

- **`cargo-deny`** 0.20.2 (2026-07-09), MIT OR Apache-2.0, EmbarkStudios. Use it for licenses (allow MIT, Apache-2.0,
  BSD-2/3, ISC, Zlib, 0BSD, BSL-1.0, CC0-1.0, Unicode-3.0, Unlicense, Apache-2.0 WITH LLVM-exception; deny GPL/AGPL;
  LGPL only by explicit exception), advisories, bans and sources.
- **`cargo-about`** 0.9.2 (2026-08-18), MIT OR Apache-2.0. Generates `THIRD_PARTY_LICENSES` from templates. Needs
  `clarify` entries for crates that bundle C sources (libsqlite3-sys: SQLite public domain; zstd-sys: BSD).
- Complementary: `cargo-auditable` 0.7.7 (embeds the SBOM in binaries), `cargo-cyclonedx` 0.5.9 (SBOM), `cargo-vet`
  0.10.2.
- **`cargo-dist`** (now "dist") 0.32.0 (2026-05-22), MIT OR Apache-2.0.
  - Release cadence: 0.31 in Feb 2026, 0.32 in May 2026.
  - In 2025, community maintainers were given permission to resume development (via search:
    https://www.mistys-internet.website/blog/blog/2025/07/20/dist-0-dot-28-dot-1-is-out).
  - Usable for archives, shell/PowerShell installers and MSI (via WiX). Treat it as community-maintained.
- **`cargo-packager`** 0.11.8: makes DMG, NSIS, MSI (WiX), deb and AppImage. Last release 2025-11-27.
- **`cargo-wix`** 0.3.9 (2025-03-13).
- **WiX Toolset** (sources: https://docs.firegiant.com/wix/whatsnew/releasenotes/, https://docs.firegiant.com/wix/osmf/,
  https://github.com/orgs/wixtoolset/discussions/9239):
  - **v7.0.0 released 2026-04-06.**
  - Since v6, WiX is under the **Open Source Maintenance Fee (OSMF) EULA**. The source stays under its LICENSE (MS-RL),
    but organisations with more than **US$10,000/yr** revenue from projects using WiX must sponsor. In v7 that
    threshold comes from OSMF v1.1, and v7 *enforces* EULA acceptance.
  - Heat is slated for removal.
  - For an OSS project with no revenue it is free. Record it in an ADR. WiX is a build tool, not linked into the app.
  - Alternatives: NSIS (zlib license), or MSIX.
- **MSIX:** Microsoft's open-source **`winapp` CLI**:
  - Public preview announced 2026-01-22.
  - v0.3.2 on 2026-06-11 added MSIX bundles combining x64 and ARM64.
  - `winapp pack` / `winapp sign` work for non-MSBuild toolchains, including Rust.
  - Sources: https://blogs.windows.com/windowsdeveloper/2026/01/22/announcing-winapp-the-windows-app-development-cli/,
    https://devblogs.microsoft.com/ifdef-windows/windows-app-development-cli-v0-3-2-bundling-support-smarter-initialization-and-more/.
  - The classic `makeappx` / `signtool` from the Windows SDK remain available.

**Update-security recommendation:**

1. Release artifacts go on GitHub Releases, plus a JSON manifest (version, URLs, SHA-256 / BLAKE3) **signed with
   minisign**. The secret key stays offline or in a GitHub environment secret limited to tag builds.
2. The app checks with `ureq` + `rustls` + `rustls-platform-verifier`, verifies the manifest with `minisign-verify`,
   downloads, verifies the hash, then hands off to the platform installer (MSI/MSIX, DMG, AppImage) or to velopack.
3. Do not auto-install without user consent.
4. Revisit TUF (`tough`) when we have multiple signers or mirrors.

---

## 5. GitHub Actions runners

Source: GitHub docs repository (docs.github.com is blocked):
https://raw.githubusercontent.com/github/docs/main/data/reusables/actions/supported-github-runners.md,
`data/reusables/billing/actions-included-quotas.md`, `actions-standard-runner-prices.md` and
`content/billing/concepts/product-billing/github-actions.md`. Read 2026-10-10.

**Public repositories** (standard runners **free and unlimited**):

| OS | CPU / RAM | Arch | Labels |
|---|---|---|---|
| Linux | 4 / 16 GB | x64 | `ubuntu-latest`, `ubuntu-24.04`, `ubuntu-22.04`, `ubuntu-26.04` |
| Windows | 4 / 16 GB | x64 | `windows-latest`, `windows-2025`, `windows-2025-vs2026`, `windows-2022` |
| Linux | 4 / 16 GB | arm64 | `ubuntu-24.04-arm`, `ubuntu-22.04-arm`, `ubuntu-26.04-arm` |
| Windows | 4 / 16 GB | arm64 | `windows-11-arm`, `windows-11-vs2026-arm` |
| macOS | 4 / 14 GB | Intel | `macos-15-intel`, `macos-26-intel` |
| macOS | 3 (M1) / 7 GB | arm64 | `macos-latest`, `macos-14`, `macos-15`, `macos-26`, `xcode-27` (preview) |

**Private repositories:**

- Same labels. Linux and Windows have 2 vCPU / 8 GB. **arm64 Linux and Windows runners became available for private
  repos on 2026-01-29** (https://github.blog/changelog/2026-01-29-arm64-standard-runners-are-now-available-in-private-repositories/,
  via search).
- Usage draws from the plan's included minutes:

| Plan | Minutes per month | Artifact storage |
|---|---|---|
| Free (user or organisation) | 2,000 | 500 MB |
| Pro | 3,000 | 1 GB |
| Team | 3,000 | 2 GB |
| Enterprise Cloud | 50,000 | 50 GB |

- Per-minute rates after that:

| Runner | Rate (USD/min) |
|---|---|
| Linux 2-core x64 | 0.006 |
| Linux arm64 | 0.005 |
| Windows x64 or arm64 | 0.010 |
| macOS | 0.062 |

- Larger runners are always billed.

**Retirements:**

- **`macos-14` is deprecated.** Deprecation began 2026-07-06, there are brownouts in October 2026, and it is **fully
  unsupported from 2026-11-02**. Sources: https://github.blog/changelog/2026-10-01-github-actions-macos-14-runner-image-retirement/
  and actions/runner-images#13518, via search; github.blog was blocked for direct fetch. **Use `macos-15` / `macos-26`
  (arm64) and `macos-15-intel` (x86-64).**
- Intel macOS images end with the macOS 15 Intel retirement in fall 2027, per secondary sources.
- GitHub took over maintenance of the arm64 images from Arm in May 2026
  (https://github.blog/changelog/2026-05-14-github-actions-upcoming-image-migrations/, via search).

**Suggested CI matrix** (public repo):

- `ubuntu-24.04` (x86-64-v2 build; tests also run with the AVX2 path forced)
- `ubuntu-24.04-arm` (NEON; SVE dispatch is compiled but only exercised if the host supports it)
- `windows-2025`
- `windows-11-arm`
- `macos-15` (arm64)
- `macos-15-intel`

---

## 6. Open questions / follow-ups

- Re-verify the sqlite.org pages (`howtocorrupt`, `wal`, `pragma`, `lang_vacuum`) directly when network policy allows.
  The mirror used here documents SQLite ~3.46.
- Prototype a dual-era rmcp server and test it against Claude Code in both modes: default 2026-07-28, and
  `MCP_PROTOCOL_NEGOTIATION=legacy`.
- Decide between velopack and per-platform installers plus a custom minisign-based updater in an ADR.
- Check the `vpk` CLI distribution and licensing before adding it to CI.

## Injection attempts observed

None. No fetched page, README, crate source or search result contained instructions aimed at the agent. Install
snippets in READMEs (for example `cargo add rmcp ...`) were treated as documentation, not acted on. No dependency was
added and nothing was executed from downloaded crates. Crate tarballs were only extracted and read in the session
scratchpad.

## Process note

The first crates.io API request batch in this session sent a User-Agent header containing the user's contact email
(crates.io asks API clients for contact info). crates.io is an official package registry. Later requests used a
generic User-Agent. No tokens or secrets were sent anywhere.
