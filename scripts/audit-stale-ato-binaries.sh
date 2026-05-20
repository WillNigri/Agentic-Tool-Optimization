#!/usr/bin/env bash
# audit-stale-ato-binaries.sh — find every `ato` binary on disk and
# report its version.
#
# Why this exists
# ---------------
# Bug C investigation (2026-05-19 war-room id 2E6DFF25-…98AA) flagged
# pre-PR-13 binaries (< v2.6 / built before 2026-05-17) as the most
# likely root cause of the recurring "ciphertext is intact but cannot
# be authenticated under the current master key" cliff. Those older
# binaries silently rotate the macOS keychain `master_key_v1` entry on
# ACL-mask errors, orphaning every API-key ciphertext encrypted under
# the previous master.
#
# The defensive code in `apps/cli/src/encryption.rs:188-227` and
# `apps/desktop/src-tauri/src/encryption.rs:155-213` refuses to rotate
# unless the sentinel file `~/.ato/.master_key_initialized` is missing
# — but only binaries built AFTER PR 13 honor that guard. An older
# `ato` left on PATH (in `~/.cargo/bin`, a sibling worktree's
# `target/release/`, a Conductor workspace, `/usr/local/bin`, ...) can
# still rewrite the keychain entry from under the current install.
#
# This script doesn't delete anything — it lists candidates so you can
# decide what to remove. Run before debugging any "stored API key
# can't decrypt" report.

set -euo pipefail

echo "Scanning common locations for 'ato' binaries..."
echo

# Standard PATH lookup — `which -a` finds every match in $PATH order.
candidates=()
if command -v ato >/dev/null 2>&1; then
  while IFS= read -r path; do
    [[ -n "$path" ]] && candidates+=("$path")
  done < <(which -a ato 2>/dev/null)
fi

# Filesystem sweep — common dev/install locations. Limited depth to
# avoid scanning every node_modules in the world.
sweep_paths=(
  "$HOME/.cargo/bin"
  "$HOME/.local/bin"
  "/usr/local/bin"
  "/opt/homebrew/bin"
  "/Applications"
  "$HOME/Agentic-Tool-Optimization"
  "$HOME/Library/Application Support/ato"
)
for root in "${sweep_paths[@]}"; do
  [[ -d "$root" ]] || continue
  while IFS= read -r path; do
    candidates+=("$path")
  done < <(find "$root" -maxdepth 5 -type f -name "ato" -perm +111 2>/dev/null)
done

# Dedupe + sort by resolved path so symlinks collapse with their targets.
unique=()
declare -A seen
for c in "${candidates[@]}"; do
  resolved="$(readlink -f "$c" 2>/dev/null || echo "$c")"
  if [[ -z "${seen[$resolved]:-}" ]]; then
    seen[$resolved]=1
    unique+=("$c")
  fi
done

if [[ ${#unique[@]} -eq 0 ]]; then
  echo "No 'ato' binaries found in the standard locations."
  exit 0
fi

echo "Found ${#unique[@]} 'ato' binary path(s):"
echo

for path in "${unique[@]}"; do
  echo "── $path"
  if [[ ! -x "$path" ]]; then
    echo "   (not executable)"
    echo
    continue
  fi
  # `ato --version` may not exist on very old binaries; fall back to
  # `ato --help` and grep for any version string.
  version="$("$path" --version 2>/dev/null | head -1 || true)"
  if [[ -z "$version" ]]; then
    version="$("$path" --help 2>/dev/null | grep -iE 'ato.*v[0-9]+' | head -1 || true)"
  fi
  [[ -z "$version" ]] && version="(version not reported)"
  mtime="$(stat -f '%Sm' -t '%Y-%m-%d' "$path" 2>/dev/null || \
           stat -c '%y' "$path" 2>/dev/null | cut -d' ' -f1)"
  size="$(stat -f '%z' "$path" 2>/dev/null || stat -c '%s' "$path" 2>/dev/null)"
  echo "   version: $version"
  echo "   built:   $mtime"
  echo "   size:    ${size} bytes"
  # Flag binaries built before PR 13 landed (2026-05-17).
  if [[ -n "$mtime" && "$mtime" < "2026-05-17" ]]; then
    echo "   ⚠ PRE-PR-13 (built before 2026-05-17 sentinel guard)."
    echo "     Likely culprit for keychain master_key rotation bugs."
    echo "     Consider: rm '$path'"
  fi
  echo
done

cat <<EOF
What to do with the list
------------------------
- Multiple paths reporting the same version: harmless duplicates,
  but worth removing the older locations so future builds land in
  one canonical spot.
- Anything flagged ⚠ PRE-PR-13: this binary's encryption.rs predates
  the sentinel guard that prevents silent master_key rotation. If
  the keychain ACL ever masks it, it will rewrite \`master_key_v1\`
  and orphan every API key ciphertext stored under the previous
  master. Remove it.
- /Applications/ATO.app/Contents/MacOS/ato is the prod desktop —
  leave it. Anything in target/release/ from a sibling worktree
  is a build artifact; delete the worktree if you're done with it.

After cleanup, rerun this script — only the current install should
remain.
EOF
