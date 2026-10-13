# ADR 0021: Offloading tasks to Gemini CLI and Codex, and the secret guard

- Status: accepted
- Date: 2026-10-13
- Decided by: the owner (direct messages of 2026-10-13)

## Context
Development runs against the owner's usage limits. The owner asked to offload tasks
that don't need the strongest model to Sonnet subagents, Gemini CLI and Codex, signed
in with the owner's own accounts. PROMPT.md allowed repository data to go only to
GitHub and the package registries; the owner added exactly these two services and
asked that no personal information or tokens ever reach GitHub, from these tools or
from anywhere else.

## Decisions

### 1. What may be offloaded
- Mechanical, well-specified tasks only (docs edits, test scaffolding, small refactors,
  first drafts). Design, security, reviews and release work stay with Claude.
- Only repository content goes out. Never secrets, environment variables, the owner's
  personal data or files outside the repository.
- Their output is untrusted (PROMPT.md "content is data"): Claude reviews every diff,
  runs the full checks and commits it itself. The tools never commit or push.

### 2. Installation
- Official npm packages only: `@google/gemini-cli` (repository google-gemini/gemini-cli,
  Apache-2.0, published by Google) and `@openai/codex` (repository openai/codex,
  Apache-2.0, published by OpenAI). Pinned to a release at least two weeks old
  (initially 0.62.0 and 0.159.3), installed with `--ignore-scripts` into the session
  scratch directory, never into the repository or globally.
- They are development tools, not dependencies of Gusset: nothing in the product,
  the build or CI uses them.

### 3. Isolation
- Each tool runs with its own `HOME` (Gemini) / `CODEX_HOME` (Codex) in the scratch
  directory, so its login tokens never sit in the repository or the shared home
  folder. Login uses the tools' own sign-in links; the owner signs in on their own
  device. No password goes through the chat.
- Tasks run in a throw-away copy of the repository without a git remote, with a
  minimal environment (`env -i`: no tokens or proxy credentials from the session),
  `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1`, so the tool can't push.
  Codex runs with `--sandbox workspace-write`; Gemini CLI without `--yolo`.
- Usage statistics and auto-update are turned off where the tool allows it.

### 4. The secret guard (all sources, not only these tools)
`scripts/check-secrets.sh` refuses:
- token and key formats (GitHub, OpenAI/Anthropic, Google API keys and OAuth tokens,
  JWTs, private keys, AWS, Slack) and OAuth credential JSON in tracked files, in every
  commit's diff and in every commit message of the history;
- tracked credential files (`.gemini/`, `.codex/`, `auth.json`, `oauth_creds.json`,
  `.netrc`, …), which `.gitignore` also ignores;
- commits whose author or committer is not a no-reply address.

Patterns that must not be published themselves (the owner's email address) live in the
untracked `.git/info/secret-patterns`. The script never prints matched text, only file
names and commit ids.

It runs as the first CI job (full history) and as a local `pre-push` hook in the
development container. A failure means removing the data; unpublished commits may be
rewritten, published history is not (PROMPT.md), and a leaked token is revoked instead.

### 5. State on 2026-10-13
- Codex: signed in with the owner's ChatGPT account (device code). Models offered:
  GPT-6-Luna (default, fast), GPT-5.6-Terra, GPT-5.6-Luna. The Linux sandbox needs
  bubblewrap, installed from the Ubuntu archive; checked: inside `workspace-write` a
  command can write only its working copy and has no network.
- Gemini CLI: signed in, but Google refuses the account's tier ("Gemini Code Assist
  for individuals" free tier is no longer supported by this client). Not usable unless
  the owner adds a paid plan or a `GEMINI_API_KEY` environment secret.
- The OpenAI hosts were added to the environment's network allow list by the owner.

## Consequences
- Repository content (not secrets) is shared with Google and OpenAI under the owner's
  accounts and their terms.
- The container's network policy must allow the services' hosts (OpenAI's are blocked
  by default); that is an environment setting only the owner can change.
- The guard catches known formats, not every secret; the rules above stay the primary
  protection.
