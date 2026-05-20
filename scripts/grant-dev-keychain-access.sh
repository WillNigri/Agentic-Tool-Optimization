#!/usr/bin/env bash
# grant-dev-keychain-access.sh — one-shot fix for the macOS keychain
# "Permitir Sempre" doesn't stick problem.
#
# Why this script exists
# ---------------------
# The dev `ato` CLI built from source (`cargo build --release`) is
# adhoc-signed — every fresh build produces a slightly different
# cdhash, so macOS keychain's per-caller ACL ("Always Allow this
# specific binary") never matches twice in a row. Result: a fresh
# keychain prompt on every dispatch.
#
# The fix this script ships: widen the keychain entry's
# partition list to accept any callers in the user's login session
# (apple-tool, apple, unsigned). After running once, the prod ATO
# desktop AND the dev CLI AND every future cargo rebuild get
# no-prompt access to the master_key entry — same blast radius as
# the existing $ATO_MASTER_KEY_B64 env-var escape hatch (any local
# binary can already read it through env), but no shell-rc edits.
#
# 2026-05-19 — recommended by the claude reviewer in war_room_id
# 2E6DFF25-A671-4708-9205-2DC7FA4898AA. The Apple-Developer-cert
# resign approach (codex's pick) is the "right" fix, but requires
# every dev to maintain a signing cert; this widens ACL trust
# instead and ships in one macOS command.
#
# Trust trade-off
# ---------------
# This DOES make the master_key entry accessible to any process
# running as your user, not just signed ATO binaries. Equivalent
# in trust scope to:
#   - the ATO_MASTER_KEY_B64 env-var escape hatch
#   - any local binary you run that could shell out to `security
#     find-generic-password` after macOS has unlocked the keychain
# It does NOT expose the key to other users on the machine, the
# network, or unprivileged processes.
#
# Run once. Re-run if the master_key entry is recreated.

set -euo pipefail

KEYCHAIN_SERVICE="ato-desktop"
MASTER_KEY_ACCOUNT="master_key_v1"

if [[ "${1:-}" == "--dry-run" ]]; then
  cat <<EOF
Would run:
  security set-generic-password-partition-list \\
    -S "apple-tool:,apple:,unsigned:" \\
    -s "$KEYCHAIN_SERVICE" \\
    -a "$MASTER_KEY_ACCOUNT" \\
    -k "\$(security default-keychain | tr -d '\" ')"

Effect: widens ACL trust on keychain entry
  service=$KEYCHAIN_SERVICE account=$MASTER_KEY_ACCOUNT
to accept any caller in your login session — no more prompts on
\`ato dispatch\` from dev builds.
EOF
  exit 0
fi

cat <<EOF
This script widens the macOS keychain ACL on
  service=$KEYCHAIN_SERVICE
  account=$MASTER_KEY_ACCOUNT

After running, the prod ATO desktop, the dev \`ato\` CLI from
\`cargo build --release\`, and any future rebuilds will all access
the master_key without a per-binary "Permitir" prompt.

Trade-off: any process running as your user can read the master
key via the keychain. Same scope as the ATO_MASTER_KEY_B64 env
var. NOT cross-user, NOT network-reachable.

Press Enter to proceed, Ctrl-C to abort.
EOF
read -r _

KEYCHAIN_PATH="$(security default-keychain | tr -d '" ')"
echo "Using default keychain: $KEYCHAIN_PATH"
echo

# `security ... -k <password>` expects the macOS LOGIN password as
# the value. Omitting `-k` makes macOS prompt interactively (GUI
# prompt OR terminal `password:` line) — that's what we want. The
# previous version of this script accidentally passed the keychain
# PATH to `-k`, which macOS treated as a wrong password and rejected
# silently. The keychain target is the trailing positional arg.
echo "macOS will now prompt for your login keychain password (the same"
echo "password you use to unlock the Mac). Type it carefully — silent"
echo "failure if wrong."
echo
security set-generic-password-partition-list \
  -S "apple-tool:,apple:,unsigned:" \
  -s "$KEYCHAIN_SERVICE" \
  -a "$MASTER_KEY_ACCOUNT" \
  "$KEYCHAIN_PATH"

echo
echo "✓ Partition list updated."
echo "  Test with: ato dispatch claude 'say hi' (should not prompt)."
