#!/usr/bin/env bash
# One-time hook installer. Re-run after fresh clone.
#
# Points git at .githooks/ (versioned, lives in the repo) instead of
# .git/hooks/ (per-clone, never tracked). This way the gate ships with
# the codebase and every contributor / agent runs the same checks.

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

chmod +x .githooks/pre-commit .githooks/pre-push
git config core.hooksPath .githooks

cat <<'EOF'
✓ hooks installed
  pre-commit  → cargo check + vitest
  pre-push    → above + vite build + cargo build CLI + 19 ato CLI smoke commands

  bypass:   git commit/push --no-verify   (don't — see CLAUDE.md)
  uninstall: git config --unset core.hooksPath
EOF
