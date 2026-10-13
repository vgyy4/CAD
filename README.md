# CAD (working title)

A Rust-native, open-source parametric 3D CAD application with a built-in MCP server,
so AI agents can do anything a human user can.

Status: early development (Phase 0 — research). See `docs/PROGRESS.md`.

License: MIT OR Apache-2.0.

## Using Gusset with AI agents (MCP)

Gusset has a built-in [MCP](https://modelcontextprotocol.io) server (ADR 0012). Agents
get the same commands as the GUI, work on their own branch (`agent/<name>`), and must
get your approval for high-impact actions (set main, delete branches, editing main,
overwriting exports). Text stored in the model is returned as untrusted `user_text`.

**stdio (recommended).** `gusset mcp [file]` attaches to the running app when it has
that project open (or the most recently started app when no file is given); otherwise
it opens the file headless (a new file is created; the agent's work is saved on exit):

```sh
claude mcp add --transport stdio gusset -- gusset mcp
claude mcp add --transport stdio gusset-part -- gusset mcp /path/to/part.gus
```

Options: `--read-only` rejects every mutating tool (also when relaying to the app);
`--no-attach` always runs headless, even when the app is open. Headless, `render_view`
renders with the GPU (any adapter, including software Vulkan/GL) or a built-in CPU
rasterizer, so it always returns a PNG. Diagnostics go to stderr; stdout carries only
the protocol.

**HTTP, directly to the running app.** The app listens on `127.0.0.1` only, on a
random port, and requires a bearer token. Both are shown in the app's "Connect an
agent" dialog and stored in an owner-only connection file:

```sh
claude mcp add --transport http gusset http://127.0.0.1:<port>/mcp \
  --header "Authorization: Bearer <token>"
```

The app can revoke the token at once (kill switch) and switch agents to read-only mode.
Agents never get shell, network or settings access, and file commands stay inside the
project folder. Read `gusset://project/security-guidelines` for the rules agents follow.

## Heavy or impossible operations

The geometry kernel runs in its own sandboxed process (`gusset-kernel-worker`, ADR 0022)
with a memory budget of 3/4 of your RAM. When an operation is too heavy for the machine,
or the kernel crashes on it, only that feature fails, with the reason (for example "the
geometry kernel crashed while building Fillet1 (segmentation fault)" or "ran out of
memory"); the app stays open and the rest of the model is rebuilt. A feature that crashed
is not tried again until you change it or something before it.

While a rebuild runs, the status bar names the feature being built and offers **Stop**,
which ends even an operation that would never finish. Agents get the same protection:
an MCP request that runs longer than 5 minutes is stopped and undone, with a
`deadline_exceeded` error. `gusset self-test` reports whether the kernel runs in its
worker process; `GUSSET_KERNEL_IN_PROCESS=1` runs it inside the app instead (for
debugging; then a kernel crash closes the app).
