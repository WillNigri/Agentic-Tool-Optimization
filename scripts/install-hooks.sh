#!/usr/bin/env bash
# One-time hook installer. Re-run after fresh clone.
#
# Points git at .githooks/ (versioned, lives in the repo) instead of
# .git/hooks/ (per-clone, never tracked). This way the gate ships with
# the codebase and every contributor / agent runs the same checks.

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

# Detect an existing hooks path the contributor may have set up for
# themselves — e.g. husky, lefthook, a custom team standard. Don't
# silently clobber it. (codex review v2.7.6 INFO #5 / LOW #5.)
EXISTING="$(git config --get core.hooksPath || true)"
if [ -n "$EXISTING" ] && [ "$EXISTING" != ".githooks" ]; then
  cat >&2 <<EOF
✗ refusing to install: git already has core.hooksPath = '$EXISTING'

  This installer would overwrite that with '.githooks'. Either:
    • unset first:  git config --unset core.hooksPath
    • or chain manually so both hook sources fire (e.g. husky's
      installer respects core.hooksPath; configure as needed).

EOF
  exit 1
fi

chmod +x .githooks/pre-commit .githooks/pre-push
git config core.hooksPath .githooks

cat <<'EOF'
✓ hooks installed
  pre-commit  → cargo check (desktop + CLI) + vitest
  pre-push    → above + vite build + cargo build CLI + ato CLI smoke

  bypass:    git commit/push --no-verify   (don't — see CLAUDE.md)
  uninstall: git config --unset core.hooksPath
EOF
