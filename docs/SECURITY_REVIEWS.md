# Security reviews

Findings from the milestone security reviews (PROMPT.md "Security reviews"), the MCP
red-team pass (docs/DOGFOODING.md) and anything else security-relevant, with their fix
status. Scratch harnesses referenced below lived outside the repository.

## Fix tracker

| Source | Finding | Status |
|---|---|---|
| Dogfood S1 / Code review C3 | Agent can rename main / make its branch look like main | **Fixed**: agents rename only their own branches without approval; "main"-like names refused for agents; branch names unique ignoring case; `agent/<name>` reuse requires the agent created it and it is not main |
| Dogfood S2 | Approval prompts show opaque ids | **Fixed**: prompts include server-resolved "Affects: b1 = "Main", created by the user, THE CURRENT MAIN BRANCH" |
| Dogfood S3 / Review #8 | Read-only mode writes files | **Fixed**: `writes_files` command flag; exports blocked for read-only agents and by the read-only bridge |
| Dogfood S4 | Bulk threshold ignores parameters | **Fixed**: parameter changes count |
| Dogfood S5 | Audit log in memory only | **Fixed**: agent changes, file writes and refusals are persisted as `AgentAction` events in the project history and reloaded on open |
| Review #1 / Code review L5 | Forged approval via client elicitation | **Fixed**: when the app hosts the session, approvals use only the in-app prompt (client elicitation answers and `input_required` retries are ignored); headless, elicitation is the only channel, stated in tool descriptions, server instructions and the ADR 0012 trust model |
| Review #2 | Windows DLL search path | Open — next |
| Review #3 | In-process BinTools parsing of project blobs | Open — next |
| Review #4–6 | see below | Open — next |
| Review #7 | Kill switch does not keep the bridge out | **Fixed**: revoke pauses agents (no valid token; connection file rewritten `paused` without a token); a restarted bridge refuses to start; `McpServer::allow_agents` issues a new token and file generation |
| Review #9 / Dogfood S7 / Code review H3, L1, L2 | Export names; GUI exports fail; vacuous test; export temp files | **Fixed**: agent exports confined, extension allow-list matching the format, no hidden components, device names, trailing dots/spaces or stream syntax, no absolute paths in errors; user exports accept any path; writes use `durable_write`; `rejects_escapes` asserts every case |
| Review #10, #15 / Dogfood S6 / Code review L6 | Rate limits bypassable, reads unlimited, sessions unbounded, identities reused after restart | **Fixed**: per-identity (client name without connection counter) and global budgets for tool calls, `render_view` (plus 2 concurrent), `get_text` and resources with `retry_after_s`; HTTP bucket shared by all connections; ≤ 32 legacy sessions, closed after 10 idle minutes; agent names in the project history are never reused |
| Review #11, #12 / Code review M6 | FIFO import hangs; worker can force 4 GiB; import blocks the model thread | **Fixed**: files opened `O_NOFOLLOW|O_NONBLOCK`, regular files only, capped read; import parses off the model thread and is cancellable (worker killed); worker output capped at 16× input + 64 MiB (≤ 1 GiB) |
| Review #13 | Connection-file owner; stale/squatted ports | **Fixed**: file and directory must be owned by the current user and closed to others; token-bound BLAKE3 identity challenge before anything is sent, and a proof on every response |
| Review #14 | x32 ABI | Open — next |
| Review #16 / Dogfood S8, S9 | Identifiers outside `user_text`; branch names | **Fixed**: identifier fields that are not strict identifiers are wrapped as flagged `user_text`; branch names kept out of messages; agent branch names are short display names (no `/`, `..`, leading dots, main-like names) |
| Review #17–20 | see below | Open — next |
| Dogfood F-B1 / Code review H5 | Import not wired; no mesh import | **Fixed**: the sandboxed worker is the STEP/IGES importer in `gusset mcp`, `gusset build --import` and the app; STL/3MF/OBJ import as reference mesh bodies (PROGRESS deviation); imported B-rep faces are named again (import never rebuilt before) |

## Review 1: MVP milestone (2026-10-11)

- Reviewer: dedicated security-review subagent (PROMPT.md "Security reviews"), working on the threat model (`docs/THREAT_MODEL.md`) and ADRs 0008, 0012, 0013, 0014 and 0015.
- Build reviewed: `claude/new-session-nedlj5` at `d2d0b5b`, including `692a265` (Windows `<lock>.info` sidecar).
- Method: code review plus black-box attacks on the real code. The attacks ran through the `gusset` CLI (`target/debug/gusset`, no OCCT, so the mock kernel was used) and through scratch harnesses outside the repository. The harnesses link the real crates by path:
  - `history_dos`: builds adversarial `.gus` files.
  - `http_server`: the real `McpServer::start_http` on a mock-kernel session.
  - `sandbox_probe` and `fake_worker`: the real `gusset_sandbox::lockdown`.
  - `worker_hang`: the real `gusset_worker::import_out_of_process`.
  - `mcp_stdio.py`, `mcp_fifo.py` and `rate.py`: MCP clients.
- Platform: Linux x86_64, kernel 6.18. Windows and macOS findings come from code review only and are marked **unverified**.
- Repository files modified: none.

### Summary

| # | Severity | Finding | Status |
|---|---|---|---|
| 1 | High | Human approval can be forged by the MCP client: any token holder approves its own `set_main`, branch deletion and overwrites | verified |
| 2 | High | No Windows DLL search-path hardening anywhere; wgpu loads `dxcompiler.dll` / `libEGL.dll` by bare name | unverified (code) |
| 3 | High | `.gus` files carry B-rep blobs that are parsed by OCCT `BinTools::Read` **inside the app process**, bypassing the import sandbox | unverified exploitability (code path verified) |
| 4 | Medium | Quadratic history-graph check: a 195 KB `.gus` file blocks opening for about 20 min of CPU | verified |
| 5 | Medium | History limits are checked on load, not on write: projects become unsavable (>~150k nodes) or unopenable (>100k branches) | verified |
| 6 | Medium | Sandbox gaps: fork/clone, user namespaces, io_uring and `kill()` are allowed; a grandchild defeats the worker timeout and hangs the app | verified |
| 7 | Medium | Kill switch writes the new token to the same connection file, so the bridge regains access on its next start | verified (code) |
| 8 | Medium | Read-only agent mode still lets agents write files (`export_step`/`export_mesh` are flagged READ) | verified |
| 9 | Low | Export paths accept any name or extension inside the project folder (`.git/hooks/pre-commit`, `*.desktop`, dotfiles) | verified |
| 10 | Low | Rate limits are per TCP connection and per agent identity; reconnecting bypasses both; MCP sessions accumulate without bound | verified |
| 11 | Low | `import_file` on a FIFO blocks the model thread forever | verified |
| 12 | Low | Compromised worker can make the app buffer up to 4 GiB of output | code |
| 13 | Low | Bridge trusts connection files by mode only (no owner check) and connects to stale ports (port squatting) | code |
| 14 | Low | seccomp deny-list does not handle the x32 ABI | unverified |
| 15 | Low | `render_view` and `get_text` bypass the per-agent command rate limit | code |
| 16 | Low | Attacker-controlled identifiers (feature/parameter/sketch names) are returned outside `user_text` | code |
| 17 | Info | Windows lock sidecar (`692a265`) is written with a plain `fs::write` (follows links), is never cleaned by housekeeping, and has a check-then-open race | code |
| 18 | Info | The GUI's suggested `claude mcp add --header "Authorization: Bearer <token>"` stores the live token in the agent's config and shell history | code |
| 19 | Info | Spec CI gates missing: CodeQL, OpenSSF Scorecard, installer ACL check | code |
| 20 | Info | Updater download/apply is not implemented; "file swap between check and install" defences exist only on paper; freeze window is up to 120 days | code |

Counts: **3 high, 5 medium, 8 low, 4 info**. Critical: none.
No prompt-injection attempts were found in repository files, fixtures or tool output during this review.

---

### 1. High: forged human approval through MCP elicitation
- **Where:** `crates/gusset-mcp/src/server.rs:127` (`supports_form_elicitation`), `:324-335` (2026-07-28 `input_required` retry), `:379-386` (legacy `elicitation/create`).
- **Problem:** the server decides whether to ask "the human" by trusting the capabilities the client declares. It then accepts whatever answer the client sends back. The client is the agent's own MCP stack, so it can claim elicitation support and answer `accept` itself.
  - Any token holder can do this: an agent with a shell can read `<runtime>/gusset/mcp/*.json` and talk HTTP directly. So can a compromised or buggy MCP client.
  - This covers set-main, branch deletion, file overwrite, edits to main and bulk changes.
  - The in-app prompt (`ApprovalProvider`) is only used when the client does not claim elicitation.
- **Reproduction:**
  ```
  gusset pack examples/bracket.gus.txt -o mcpproj/p.gus
  python3 -I mcp_stdio.py forge mcpproj/p.gus
  ```
  The script initialises with `"capabilities":{"elicitation":{"form":{}}}`, creates branch `x` and calls `branch_set_main {"branch":"x"}`. When the server sends `elicitation/create` ("…wants to run "Set as main"… Approve?"), the script itself replies `{"action":"accept","content":{"approve":true}}`.
- **Observed:** `branch_set_main {"main": "b3"}`. `branch_list` then shows the agent's branch `x` as `"main": true`. The same works over HTTP. The modern flow (`requestState` plus `inputResponses`) has the same trust model.
- **Impact:** "Human approval for high-impact actions" is not enforced server-side. A prompt-injected agent can move main and delete branches without the user ever seeing a prompt, which breaks spec "Never rely only on the agent obeying".
- **Fix:**
  - When the GUI is attached, always require the in-app approval for `affects_main`/destructive commands, alone or in addition to elicitation.
  - Treat an elicitation answer as approval only in headless stdio mode, and document that it trusts the client.
  - Show the approval in the GUI audit log with "approved via client" vs "approved in app".

### 2. High: no DLL search-path hardening on Windows (DLL planting)
- **Where:**
  - `crates/gusset-app/src/main.rs:33`, `crates/gusset-cli/src/main.rs:35`, `crates/gusset-worker/src/main.rs:9`: no `SetDefaultDllDirectories` call. `grep -r SetDefaultDllDirectories` finds nothing, although ADR 0008 §2/§6 and threat model B6 say it is done "at the top of main".
  - wgpu-hal 30.0.1 `src/dx12/instance.rs:75` does `libloading::Library::new("dxcompiler.dll")` with `Dx12Compiler::Auto` and no static DXC.
  - `src/gles/egl.rs:736` loads `libEGL.dll` by bare name.
  - egui-wgpu enables `PRIMARY | GL`, and `gusset-render/src/headless.rs:213` falls back to GL.
- **Problem:** the spec-mandated restriction of the DLL search path (system32 and application directory only) is missing, and some dependencies load optional DLLs by bare name. The app therefore relies on the default Windows search order for those loads.
- **Reproduction:** code review only (no Windows host). The call sites are listed above.
- **Impact:** violates PROMPT.md "Code loading" and ADR 0008 §6. Rated high because it concerns code loading in the main app.
- **Fix:**
  - Call `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_APPLICATION_DIR)` as the first statement of `main` in all three binaries. This is already planned under gusset-app's unsafe allowance, and the CLI and worker need it too.
  - Link with `/DEPENDENTLOADFLAG:0x800`.
  - Configure wgpu explicitly: a fixed `Dx12Compiler` (FXC or static DXC), explicit backends, and no `from_env`.
  - Add a Windows CI test that records loaded modules (or Process Monitor-style checks) and asserts that every one comes from system32 or the install directory.

### 3. High: OCCT B-rep parsing of project-file blobs runs in-process (sandbox bypass)
- **Where:**
  - `crates/gusset-eval/src/lib.rs:299` (`FeatureKind::Import` calls `k.read_native(&bytes)`).
  - `crates/gusset-kernel-occt/src/lib.rs:313`.
  - `crates/gusset-occt-sys/cpp/gusset_occt.cpp:889-900` (`BinTools::Read` on the blob).
- **Problem:** STEP/IGES parsing goes to the sandboxed worker, but the worker's output is stored as a blob inside the `.gus` file. Every open or rebuild of a project then parses that blob with OCCT's C++ binary reader in the GUI, CLI or MCP process.
  - A malicious `.gus` (T1) can carry any bytes in an `import` feature's blob. The container's BLAKE3 hash only proves that the file is self-consistent.
  - So the riskiest C++ parser reachable from a file open runs **unsandboxed**, contrary to threat model B1/B2 ("pathological geometry … out-of-process").
- **Reproduction:** reachability verified by code. Exploitability is **unverified**: this host has no OCCT-linked build, and `BinTools::Read` has not been fuzzed (no fuzz target covers it).
- **Impact:** at minimum, a crash or hang on file open. A memory-corruption bug in `BinTools` would mean code execution as the user from opening a project.
- **Fix:**
  - Parse stored B-rep blobs in the worker too: a `read_native` request that returns re-serialised, validated data, or a shape check plus a cached mesh.
  - Alternatively, store a format we parse in Rust.
  - At minimum, add a `brep_read` fuzz target with ASan (fuzz-miri.yml) and size/entity limits before `BinTools::Read`.

### 4. Medium: quadratic cycle check in the history graph (open-file DoS)
- **Where:** `crates/gusset-history/src/persist.rs:221-236`. Only the start node is memoised in `depth_ok`, not every node on the walked path. A parent chain written in descending-id order therefore costs O(N²).
- **Reproduction:**
  ```
  history_dos chain <N> [out.gus]
  ```
  This writes a valid container whose `nodes` record is `n0` (root) and `n_k` with parent `n_{k+1}` (`n_N` with parent `n0`), all pointing at one snapshot.
  - Release build: N=16 000 takes 10.0 s, and N=32 000 takes 45.8 s.
  - `inputs/chain170k.gus` is **195 KB** (the nodes record is 16.1 MB, just under the 16 MiB record limit). Extrapolated: about 20 min of CPU in a release build.
  - `inputs/chain20k.gus` (33 KB): `timeout 120 gusset text chain20k.gus` was killed after 120 s (debug build).
- **Impact:** opening a small hostile project, or one previewed by housekeeping or by `gusset mcp`, freezes the process. The fuzzers do not find this (their inputs are small).
- **Fix:** mark every node on a walked path as verified (or do a single topological pass with colours), giving O(N) in total. Add a regression test with a reversed chain.

### 5. Medium: history limits enforced only on load (agent can make a project unsavable or unopenable)
- **Where:** load-time limits are in `crates/gusset-history/src/persist.rs:180,249` (`MAX_NODES`, `MAX_BRANCHES`). The container record limit is `crates/gusset-store/src/container/mod.rs:174` (16 MiB for NODE records). `History::create_branch`/`commit` (`history.rs:429,477`) check neither, and `History::gc` (`history.rs:825`) has no caller outside the crate.
- **Reproduction:**
  - `history_dos branches 100001 inputs/branches100001.gus` prints `encode ok: 153762 bytes`, then `DECODE FAILED: the project file is damaged: too many branches`.
  - `history_dos nodes 200000` prints `ENCODE FAILED: … a Node record of 26777899 bytes exceeds the 16777216-byte limit`. Every edit is a node, so after about 150k edits **no save and no recovery snapshot can be written**.
- **Impact:**
  - Long-lived projects eventually stop saving.
  - An agent at its default 600 commands/min (and more with several identities, see #10) can reach either limit in hours.
  - Branch case: the saved file opens only through salvage, which also fails `from_records` and drops all history. That conflicts with "agents can't permanently delete anything".
- **Fix:**
  - Enforce the same limits when writing: refuse `create_branch` and warn near the node limit.
  - Run GC (ADR 0006) on save.
  - Split or raise the NODE record limit (a large kind, or several records).
  - Add a test that `decode(encode(h))` succeeds whenever `encode` succeeds.

### 6. Medium: import-worker sandbox gaps
- **Where:** `crates/gusset-sandbox/src/lib.rs:84-89` (Landlock ABI V5: no signal or abstract-UNIX-socket scoping), `:113-144` (seccomp deny-list), and `crates/gusset-worker/src/lib.rs:229-250` (timeout and pipe handling).
- **Reproduction:** `sandbox_probe <victim-pid>` applies the real `lockdown()`, which reports everything applied: rlimits, `landlock (no filesystem, no TCP)` and seccomp. It then probes:

  | Probe | Result |
  |---|---|
  | `fork()` (glibc uses `clone`) | succeeds |
  | `clone(CLONE_NEWUSER)` | succeeds (`unshare` is denied, `clone` flags are not) |
  | `io_uring_setup` | returns fd 3 (io_uring operations are not seen by seccomp) |
  | `kill(victim, SIGTERM)` | 0; the unrelated victim process of the same user died (`returncode -15`) |
  | `socket()` | -1, as intended |

  - `worker_hang fake_worker`: the fake worker applies the same lockdown, forks a grandchild that holds stdout for 40 s, and exits 0. `import_out_of_process(…, timeout = 2 s)` returned only after **40.0 s** (`Protocol("the import worker sent no response")`).
  - The grandchild is not killed: `child.kill()` only kills the direct child, and there is no process group or job on Linux/macOS. The reader thread `join` has no timeout.
- **Impact:** a parser exploit in the worker can kill or stop the user's other processes (including the app), keep running after the timeout, hang the importing thread indefinitely, and reach a much larger kernel attack surface (user namespaces, io_uring). Not yet reachable: `import.file` has no importer wired in the app, but this is the milestone design.
- **Fix:**
  - seccomp: deny `clone`/`clone3` unless `CLONE_THREAD` is set (argument filter), and deny `io_uring_*`, `kill`/`tkill`/`tgkill` to other pids (or use Landlock ABI V6 `LANDLOCK_SCOPE_SIGNAL` plus abstract-UNIX scoping where available).
  - Set `PR_SET_NO_NEW_PRIVS` explicitly and `RLIMIT_NPROC`.
  - Spawn the worker in its own process group and `killpg` on timeout or exit.
  - Read stdout with a deadline instead of `read_to_end` on a thread joined without a timeout.
  - Prefer an allow-list once OCCT's syscall set is measured.

### 7. Medium: kill switch does not keep the bridge out
- **Where:** `crates/gusset-mcp/src/http.rs:449-466` (`revoke` writes the new token to the connection file, line 460) and `crates/gusset-mcp/src/bridge.rs:141-169` (the bridge reads the token from that file at start).
- **Problem:** after "Revoke token", any `gusset mcp` bridge that restarts (MCP clients restart servers automatically, and an agent with a shell can restart one) reads the fresh token and reconnects. The UI says "Token revoked: all agents were disconnected", which suggests a lasting effect.
- **Impact:** the per-session kill switch only interrupts sessions; it does not revoke access for agents using the bridge.
- **Fix:**
  - On revoke, pause the endpoint until the user re-enables agents (no new token is written), or delete the connection file.
  - Require an explicit "allow agents again" action.
  - Label the button accurately.

### 8. Medium: read-only agent mode still writes files
- **Where:** `crates/gusset-commands/src/commands.rs:1648,1723` (`export.step`/`export.mesh` use `FLAGS: READ`), `crates/gusset-commands/src/registry.rs:154` and `crates/gusset-mcp/src/bridge.rs:312` (both only check `flags.mutating`).
- **Reproduction:**
  ```
  python3 -I mcp_stdio.py readonly mcpproj/p.gus --read-only
  ```
  - `feature_delete` gives `read_only` (as expected).
  - `export_mesh {"path":"written-in-read-only.stl"}` gives `{"written": …, "bytes": 1084}`, and the file exists.
- **Impact:** "Read-only agents: Agents can look but cannot change anything" (UI text) is false. Agents can create files in the project folder.
- **Fix:** add a `writes_files` flag, and refuse such commands in read-only mode and in `read_only_refusal`.

### 9. Low: unrestricted export file names inside the project folder
- **Where:** `crates/gusset-commands/src/paths.rs:8-38`.
- **Problem:** confinement works. These were all refused: `../escape.stl`, `/tmp/abs.stl`, `link/x.stl` (symlinked directory) and `linkfile.stl` (symlinked file). However, any name and extension is accepted.
- **Reproduction:** `python3 -I mcp_stdio.py paths mcpproj/p.gus` wrote `.git/hooks/pre-commit` and `evil.desktop` (mode 0644, STL content).
- **Impact:** limited, because the content is mesh/STEP data and the files are not executable. But agents can create or replace (with approval) dotfiles and VCS metadata in project folders, which are often git repositories or the home directory.
- **Fix:**
  - Require the extension to match the format.
  - Reject hidden path components (`.git`, `.*`) and Windows device names and ADS.
  - Re-check after opening (`O_NOFOLLOW`/`openat`, as the threat model B3 says).

### 10. Low: rate limits are bypassable and MCP sessions are unbounded
- **Where:**
  - `crates/gusset-mcp/src/http.rs:276-283`: the token bucket is per TCP connection.
  - `crates/gusset-commands/src/session.rs:279`: per-actor limit.
  - `crates/gusset-mcp/src/server.rs:270-285`: identity per MCP session id, or per self-chosen client name.
  - `http.rs:187`: `LocalSessionManager::default()`.
- **Reproduction:** `rate.py` and inline scripts against `http_server`:
  - 300 requests on one keep-alive connection: two 429s.
  - 300 requests on new connections: **300×200 in 3.0 s**, over the 50/s limit.
  - 5 000 `initialize` calls on new connections in 2.9 s grew the server RSS from 45 MB to **307 MB** (about 52 KB per session, never reclaimed).
- **Impact:** only a token holder can do this (T5). Rate limits do not limit, and the app's memory can be exhausted. Each new identity also gets its own command rate budget and its own `agent/<name>` branch.
- **Fix:**
  - Rate-limit per token (global) as well as per connection.
  - Cap concurrent connections and sessions, with idle expiry.
  - Bound the `rate` and `agents` maps.

### 11. Low: `import_file` on a FIFO hangs the model thread
- **Where:** `crates/gusset-commands/src/commands.rs:1780-1784` (`metadata` then `std::fs::read`, with no file-type check, before the importer check).
- **Reproduction:**
  ```
  mkfifo mcpproj/pipe.step
  python3 -I mcp_fifo.py mcpproj/p.gus
  ```
  - `import_file {"path":"pipe.step"}` gets no answer.
  - A following `query_model` also gets no answer within 10 s.
- **Impact:** DoS of the whole session from a FIFO in the project folder (for example one extracted from a tar archive). This also applies to the GUI import once it is wired.
- **Fix:** require `is_file()`. Open with `O_NONBLOCK`/`O_NOFOLLOW`, then check the opened handle's type and size.

### 12. Low: the worker can force a 4 GiB allocation in the app
- **Where:** `crates/gusset-worker/src/lib.rs:21` (`MAX_OUTPUT = 4 GiB`) and `:236` (`read_to_end`). `decode_response` then copies every body.
- **Impact:** a compromised worker can exhaust the app's memory (crash and loss of unsaved work), which defeats "a malicious file can only crash the worker".
- **Fix:** read the frame header first. Cap the total at the size the import actually needs (for example a few times the input size, with a hard cap well below RAM), and stream the bodies.

### 13. Low: connection-file trust and stale ports
- **Where:** `crates/gusset-mcp/src/bridge.rs:141-153` checks `mode & 0o077 == 0` and that the path is not a symlink, but not that the owner is the current uid or that the directory is ours. `:188-193` treats any listener on the recorded port as the app.
- **Problem:** if the app crashed (stale file), another local user (T2) who binds that port receives the bridge's requests (the agent's tool calls, with an already-dead token). Its responses go straight to the agent, which is a prompt-injection channel.
- **Impact:** with a misconfigured `XDG_RUNTIME_DIR`, files written by other users would also be accepted.
- **Fix:**
  - Check the file and directory owner (`st_uid == geteuid()`).
  - Remove stale files whose pid is gone.
  - Authenticate the server to the bridge, for example with an HMAC challenge using the token, or by checking that the listening socket belongs to our uid via `/proc/net/tcp` / `SO_PEERCRED` on a UNIX socket instead of TCP.

### 14. Low (unverified): x32 ABI bypasses the seccomp deny-list
- **Where:** `crates/gusset-sandbox/src/lib.rs:154`. seccompiler 0.5 checks only `AUDIT_ARCH_X86_64` and does not reject syscall numbers with `__X32_SYSCALL_BIT`. With default-allow, x32 `execve` (nr 0x40000000+520) is not matched.
- **Impact:** only kernels that enable x32 at runtime are affected (most distributions disable it).
- **Fix:** add a rule that fails any `nr >= 0x40000000` on x86_64, or move to an allow-list.

### 15. Low: `render_view` and `get_text` skip the per-agent rate limit
- **Where:** `crates/gusset-mcp/src/server.rs:413-475` and `:477`. These tools do not go through `registry::execute`, so `rate_ok` never counts them.
- **Problem:** each `render_view` evaluates and tessellates on the model thread and renders up to 2048² pixels, with no cap on concurrent renders.
- **Fix:** count them in the actor's rate limit and allow one render at a time per session.

### 16. Low: identifiers returned outside `user_text`
- **Where:** `crates/gusset-commands/src/commands.rs:1492` (`"name": f.name`) and the parameter, body, sketch and face names.
- **Problem:** these come from the document (T1-controlled, up to 64 identifier characters) but are not labelled untrusted. Only the heuristic catches obvious phrases.
- **Fix:** document in the server instructions that names are untrusted too, or return them as `name_user_text`.

### 17. Info: Windows lock sidecar (`692a265`)
- **Where:** `crates/gusset-store/src/lock.rs:259`.
- **Problem:**
  - `std::fs::write(sidecar(path), …)` follows reparse points, and `holder_info` reads the sidecar through links too.
  - Housekeeping never removes stale `<id>.lock.info` files after a crash; `ids_in` and `clean_temps` ignore them.
  - `open_lock_file` checks `symlink_metadata` and then opens without `O_NOFOLLOW`, which is a TOCTOU.
- **Impact:** all of this lives in the owner-only `<data>/locks` directory, so only a same-user process (T3, out of scope) could exploit it. Stale info is harmless display data, because the OS lock decides.
- **Fix:** create the sidecar with create-new into a temp file and rename it; open with `FILE_FLAG_OPEN_REPARSE_POINT` / `O_NOFOLLOW`; delete orphaned sidecars in housekeeping.

### 18. Info: token persisted by the suggested setup command
- **Where:** `crates/gusset-app/src/agents.rs:313-321`.
- **Problem:** copying the "with token" command stores the live bearer token in the agent's config file and in shell history.
- **Fix:** prefer the stdio bridge (`claude mcp add … -- gusset mcp`) in the UI and warn when showing the token.

### 19. Info: CI gaps against the spec
- **Problem:** there is no CodeQL workflow (ADR 0008 §11 mentions Actions and C++), no OpenSSF Scorecard, and no installer ACL or permission check (spec "CI checks on every installer target"; only per-user MSI exists today, so there is no machine-wide install to check yet).
- `cargo install cargo-cyclonedx --version 0.5.9 --locked` in release builds is pinned by version, not hash. It runs in a job without secrets.

### 20. Info: updater
- **Not implemented yet:** download, extraction and apply. The "same handle re-verification", safe extraction and per-file hash checks are design only, so the "file swap between check and install" attack could not be tested. Persisting `KeyRing::revoked` is the caller's job and has no caller yet.
- **Replay:** a captured, still-valid manifest (90-day validity, up to 120 days accepted) can be replayed to hide a newer release (freeze). The client does not remember the newest manifest version it has seen.
- **Path checks:** `is_safe_relative_path` deduplicates by ASCII case only. Non-ASCII case folding and Unicode normalisation (NTFS/APFS) can make two entries the same file. Fix this before extraction is written.

---

### Checked and found solid
- **MCP HTTP checks** (live, `http_server` harness):

  | Request | Result |
  |---|---|
  | No token | 401 |
  | Wrong token | 401 |
  | Token in the query string | 401 |
  | `Host: evil.example:<port>` | 403 |
  | `Origin: http://evil.example` | 403 |
  | `Origin: null` | 403 |
  | Unauthenticated CORS preflight | 403 |
  | `/mcp/` | 404 |
  | Same-origin with a valid token | 200 |

  Constant-time compare; loopback-only bind.
- **Connection file:** directory 0700, file 0600, atomic create-new temp plus rename. The token never appears in `Debug` or in bridge stderr. The bridge refuses group- or other-readable files and symlinks.
- **Path confinement:** `..`, absolute paths, symlinked directories and symlinked target files are all refused (live).
- **Approval handle:** single use, bound to agent, tool and args, expires; `approved` as a tool argument is refused.
- **Container decoder** (`gusset-store`): size, record, root and total-decompressed budgets, zstd window limit, CRC32C plus BLAKE3, strict layout. Fuzzed in CI.
- **Mesh import** (`gusset-meshio`): limits on size, triangles, vertices, objects, ZIP entries, decompressed bytes and compression ratio; XML depth and attributes; no DOCTYPE; bounded component fan-out and depth with cycle detection.
- **Parameters:** a 60-level doubling chain and a 999-deep chain both evaluate quickly (memoised) via MCP `query_model` (0.06 s and 0.6 s).
- **Store:** private directories refuse a symlinked leaf; temp files use create-new and random names; housekeeping and cache never follow symlinks; lock files are 0600 with an inode re-check.
- **Updater verification:** minisign with prehash only (legacy refused); revocation can only remove keys; expiry, maximum validity window, strictly-newer version, target match, HTTPS URL prefix, sizes and path rules. With no built-in keys, every manifest is refused.
- **Worker spawn:** absolute path, `env_clear()`, argument array, stdin/stdout only, stderr to null. Landlock (no filesystem, no TCP) and the `socket`/`exec` denials work (live).
- **Release and CI:**
  - All actions are pinned by SHA.
  - `permissions: {}` / `contents: read` by default.
  - No `pull_request_target`.
  - Secrets appear only in the `publish` job (`environment: release`), after a check that the tag is on the default branch; `persist-credentials: false`.
  - Tools are pinned by SHA-256; Linux RPATH is checked to be `$ORIGIN`-relative.
  - The Windows MSVC runtime is deployed app-local; the MSI is per-user under `%LOCALAPPDATA%` and is checked for services, tasks and Run keys.
- **Supply chain:** cargo-deny (advisories, yanked=deny, licences, sources limited to crates.io, no wildcards), cargo-audit `--deny warnings`, Dependabot for cargo and actions.

### Scratch artefacts
Under `scratchpad/security/`: `harness/` (Rust crate: `history_dos`, `http_server`, `sandbox_probe`, `fake_worker`, `worker_hang`), `mcp_stdio.py`, `mcp_fifo.py`, `mcp_query.py`, `rate.py`, and `inputs/` (`chain20k.gus`, `chain170k.gus`, `branches100001.gus`, `param_*.gus.txt`).
