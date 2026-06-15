#!/usr/bin/env bash
# Manually symlink build deps from the source repo into a mission
# worktree. Normally `ato missions worktree-create` does this
# automatically (see symlink_worktree_deps in apps/cli/src/commands/
# missions.rs); use this script when:
#   • the Rust-side symlink call failed (cross-device link, read-only
#     fs), and the pre-commit hook fell back to the reduced gate;
#   • you're working in a worktree that pre-dates 2026-06-14 (before
#     the auto-symlink fix landed).
#
# Usage: ./scripts/symlink-worktree-deps.sh [SOURCE_REPO]
#   SOURCE_REPO defaults to the parent repo discoverable via
#   $ATO_SOURCE_REPO or by walking up out of ~/.ato/missions/<slug>/.

set -euo pipefail

WT="$(pwd)"

if [ "$#" -ge 1 ]; then
    SRC="$1"
elif [ -n "${ATO_SOURCE_REPO:-}" ]; then
    SRC="$ATO_SOURCE_REPO"
elif [[ "$WT" == */.ato/missions/* ]]; then
    # ~/.ato/missions/<slug>/{worktrees|integration}/* → infer from the
    # mission's recorded source repo via `ato missions get`. If that
    # tool isn't available, fall back to the user's most-recent ato
    # checkout (best guess).
    if command -v ato >/dev/null 2>&1; then
        SLUG="$(echo "$WT" | sed -nE 's|.*/.ato/missions/([^/]+)/.*|\1|p')"
        SRC="$(ato missions get "$SLUG" --human 2>/dev/null | awk -F': ' '/source_repo/{print $2; exit}')"
    fi
fi

if [ -z "${SRC:-}" ] || [ ! -d "$SRC" ]; then
    echo "error: source repo not provided and could not be inferred." >&2
    echo "Usage: $0 <PATH_TO_SOURCE_REPO>" >&2
    exit 1
fi

link() {
    local rel="$1"
    if [ ! -e "$SRC/$rel" ]; then
        echo "skip: $SRC/$rel doesn't exist"
        return
    fi
    if [ -e "$WT/$rel" ] || [ -L "$WT/$rel" ]; then
        echo "skip: $WT/$rel already present"
        return
    fi
    mkdir -p "$(dirname "$WT/$rel")"
    ln -s "$SRC/$rel" "$WT/$rel"
    echo "linked $rel"
}

link "node_modules"
link "apps/desktop/node_modules"
link "apps/desktop/src-tauri/binaries/ato-aarch64-apple-darwin"

echo "done."
