#!/usr/bin/env bash
# Refuses tokens, keys and personal data in the repository (ADR 0021).
#
# Checks the tracked tree, every commit's diff and message in the history, and the
# author and committer addresses. Extra patterns that must not be published themselves
# (e.g. the owner's email address) go one extended regex per line into the untracked
# file .git/info/secret-patterns; this script and CI never print what they matched.
#
# usage: scripts/check-secrets.sh            (tree + full history)
#        scripts/check-secrets.sh --tree     (tracked tree only, fast)
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

patterns=(
  'gh[opsur]_[A-Za-z0-9]{36}'                                       # GitHub tokens
  'github_pat_[A-Za-z0-9_]{60,}'                                    # GitHub fine-grained tokens
  '(^|[^A-Za-z0-9])sk-(proj-|ant-|svcacct-)?[A-Za-z0-9_-]{32,}'     # OpenAI / Anthropic keys
  'AIza[0-9A-Za-z_-]{35}'                                           # Google API keys
  'ya29\.[0-9A-Za-z_-]{20,}'                                        # Google OAuth access tokens
  '1//0[0-9A-Za-z_-]{30,}'                                          # Google OAuth refresh tokens
  'eyJ[A-Za-z0-9_-]{20,}\.eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}' # JWTs (ChatGPT/Codex tokens)
  '-----BEGIN ([A-Z]+ )?PRIVATE KEY-----'                           # private keys
  'AKIA[0-9A-Z]{16}'                                                # AWS keys
  'xox[abpr]-[A-Za-z0-9-]{10,}'                                     # Slack tokens
  '"(access|refresh|id)_token"[[:space:]]*:[[:space:]]*"[^"]{8,}'   # OAuth credential files
)
local_file="$(git rev-parse --git-path info/secret-patterns)"
if [[ -f "$local_file" ]]; then
  while IFS= read -r line; do
    [[ -n "$line" && "$line" != \#* ]] && patterns+=("$line")
  done <"$local_file"
fi
args=()
for p in "${patterns[@]}"; do args+=(-e "$p"); done

# Credential files of the CLIs and services we use must never be tracked.
cred_files='(^|/)(\.gemini|\.codex)/|(^|/)(oauth_creds|google_accounts|credentials)\.json$|(^|/)auth\.json$|(^|/)\.netrc$|(^|/)\.git-credentials$'

fail=0
if hits=$(git ls-files | grep -E "$cred_files"); then
  echo "error: credential files are tracked:" >&2
  printf '  %s\n' $hits >&2
  fail=1
fi
if hits=$(git grep -IlE "${args[@]}" -- . 2>/dev/null); then
  echo "error: possible secrets or personal data in tracked files:" >&2
  printf '  %s\n' $hits >&2
  fail=1
fi

if [[ "${1:-}" != "--tree" ]]; then
  if git log --all -p --format='%H%n%B' | grep -qIE "${args[@]}"; then
    # Name the commits, not the matches.
    for c in $(git rev-list --all); do
      if git show --format='%B' "$c" | grep -qIE "${args[@]}"; then
        echo "error: possible secret or personal data in commit $c" >&2
      fi
    done
    fail=1
  fi
  # Commits are authored as no-reply identities, never personal addresses.
  allowed='^(noreply@anthropic\.com|noreply@github\.com|[0-9]+\+[A-Za-z0-9-]+@users\.noreply\.github\.com|[A-Za-z0-9-]+\[bot\]@users\.noreply\.github\.com)$'
  while read -r commit author committer; do
    if ! [[ "$author" =~ $allowed && "$committer" =~ $allowed ]]; then
      echo "error: commit $commit has a personal author or committer address" >&2
      fail=1
    fi
  done < <(git log --all --format='%H %ae %ce')
fi

if [[ $fail -ne 0 ]]; then
  echo "Remove the data (and rewrite only unpublished commits); see ADR 0021." >&2
  exit 1
fi
echo "check-secrets: clean"
