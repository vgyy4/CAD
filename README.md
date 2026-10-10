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
