#!/usr/bin/env bash
# Run the standard multi-LLM review against the current branch and
# print a ready-to-paste PR description block.
#
# Usage:
#   scripts/llm-review.sh                       # diff vs origin/main
#   scripts/llm-review.sh --against HEAD~3      # diff vs HEAD~3
#   scripts/llm-review.sh --reviewer claude --reviewer minimax  # override
#
# The default reviewer set matches what we use on every meaningful
# commit: @security-specialist + @perf-reviewer (agent-as-reviewer
# personas backed by Gemini) + claude + minimax. Both Gemini-backed
# reviewers run in parallel by sharing the same session, so the second
# one reads the first's findings before adding its own.
#
# Requires: `ato` on PATH or apps/cli/target/release/ato built.

set -euo pipefail

REVIEWERS=("@security-specialist" "@perf-reviewer" "claude" "minimax")
AGAINST=""
OUT="$(mktemp -t ato-review-XXXXXX.md)"

# Parse flags. Anything we don't recognize gets forwarded to ato review
# verbatim so callers can pass --consensus, --lean, --skip-build etc.
ARGS=()
while (( $# )); do
  case "$1" in
    --against)        AGAINST="$2"; shift 2 ;;
    --reviewer)       ARGS+=("--reviewer" "$2"); shift 2 ;;
    --out)            OUT="$2"; shift 2 ;;
    *)                ARGS+=("$1"); shift ;;
  esac
done

# Default reviewer set kicks in only if the caller didn't override.
if ! printf '%s\n' "${ARGS[@]:-}" | grep -q -- '--reviewer'; then
  for r in "${REVIEWERS[@]}"; do
    ARGS+=("--reviewer" "$r")
  done
fi

# Default base ref: origin/main (or main when no remote). The CLI's own
# --against fallback is "merge base with origin/main" so omitting --against
# is also fine — passing explicitly here makes the command reproducible.
if [[ -z "$AGAINST" ]]; then
  if git rev-parse --verify --quiet origin/main >/dev/null; then
    AGAINST="origin/main"
  else
    AGAINST="main"
  fi
fi

# Locate the ato binary.
ATO=""
if command -v ato >/dev/null 2>&1; then
  ATO="ato"
elif [[ -x apps/cli/target/release/ato ]]; then
  ATO="apps/cli/target/release/ato"
else
  echo "error: 'ato' not on PATH and apps/cli/target/release/ato not built" >&2
  echo "       run 'cargo build -p ato --release' first or 'brew install --cask ato'" >&2
  exit 1
fi

echo "→ Running multi-LLM review (base: $AGAINST, out: $OUT)..." >&2
"$ATO" review --against "$AGAINST" --out "$OUT" --human "${ARGS[@]}" || {
  echo "error: review failed — see output above" >&2
  exit 1
}

# Emit the PR-ready block on stdout. Pipe to `pbcopy` (macOS) or `xclip`
# (Linux) and paste into the GitHub PR description.
cat <<EOF
<details>
<summary>Multi-LLM review transcript ($AGAINST → HEAD)</summary>

\`\`\`markdown
$(cat "$OUT")
\`\`\`

</details>
EOF

echo "" >&2
echo "→ Review saved to $OUT" >&2
echo "→ PR-ready block printed above. Pipe through pbcopy/xclip to copy." >&2
