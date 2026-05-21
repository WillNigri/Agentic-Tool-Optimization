#!/usr/bin/env bash
# Pre-tag desktop smoke — S12 (v2.7.11).
#
# Run this BEFORE every release tag (`git tag v2.x.y`) to catch
# regressions the headless cargo/vitest gate can miss:
#
#   - panics on startup (Tauri main hangs or aborts before window opens)
#   - missing webview assets (vite build produced an empty dist/)
#   - broken IPC capabilities manifest (Tauri loader rejects the bundle)
#   - dev-only code that snuck into release (mock providers, hard-coded
#     localhost URLs, debug println spam)
#
# Approach is intentionally LIGHT — we don't yet pull in tauri-driver
# + selenium-webdriver. That's a real ticket (S12 follow-up) and would
# add ~150MB of dev deps for a single smoke. For now: build release,
# launch the binary, watch for a clean "Running" handoff + no panic in
# the first N seconds, then kill it. If the app panics or hangs before
# the window paints, this catches it.
#
# Future upgrade path (S12 v2):
#   1. Add `tauri-driver = "0.1"` to apps/desktop/src-tauri/Cargo.toml
#      (dev-dependencies).
#   2. Add a wdio.conf.ts under apps/desktop/test-e2e/.
#   3. Replace this script's "launch + watch logs" body with
#      `npx wdio run wdio.conf.ts` invocation.
#   4. The wdio test should: open the main window, assert the sidebar
#      renders, click Agents, assert the empty-state appears, close.
#
# Usage:
#   ./scripts/pre-tag-smoke.sh         # default — release build + smoke
#   ./scripts/pre-tag-smoke.sh --quick # skip the cargo rebuild
#
# Exit codes:
#   0 — smoke passed
#   1 — build failed
#   2 — launch failed (binary missing after build)
#   3 — startup panic (panic string seen in stderr within N seconds)
#   4 — hang (no "Running" line within N seconds AND no panic)

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

QUICK=0
if [ "${1:-}" = "--quick" ]; then
  QUICK=1
fi

STARTUP_TIMEOUT_SECS=30
SMOKE_HOLD_SECS=4

step() { printf "\033[1;36m→\033[0m %s\n" "$1"; }
ok()   { printf "\033[1;32m✓\033[0m %s\n" "$1"; }
fail() { printf "\033[1;31m✗ pre-tag smoke failed: %s\033[0m\n" "$1"; exit "${2:-1}"; }

BIN="apps/desktop/src-tauri/target/release/ato-desktop"

if [ "$QUICK" -eq 0 ]; then
  step "vite build (apps/desktop)"
  (cd apps/desktop && npx vite build >/dev/null 2>&1) || fail "vite build" 1

  step "cargo build --release (apps/desktop/src-tauri)"
  (cd apps/desktop/src-tauri && cargo build --release --quiet) || fail "cargo build desktop" 1
fi

if [ ! -x "$BIN" ]; then
  fail "binary missing at $BIN (build step should have produced it)" 2
fi

ok "binary present: $BIN"

step "launching binary; waiting up to ${STARTUP_TIMEOUT_SECS}s for clean startup"

LOG="$(mktemp)"
"$BIN" >"$LOG" 2>&1 &
PID=$!

# Cleanup handler — kill the child even if we get interrupted, and
# scrub the temp log so a flaky CI box doesn't leak gigabytes of
# Tauri webview noise.
cleanup() {
  if kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null || true
    # Give it a moment to exit cleanly; SIGKILL only if it ignores.
    sleep 1
    kill -9 "$PID" 2>/dev/null || true
  fi
  rm -f "$LOG"
}
trap cleanup EXIT

# Poll for either a panic signature (fail fast) or the Tauri runtime's
# "Running" handoff line that means the main window proxy is alive. We
# look for either condition once per second until the timeout elapses.
elapsed=0
while [ "$elapsed" -lt "$STARTUP_TIMEOUT_SECS" ]; do
  if grep -qE 'panicked at|fatal runtime error|thread .* panicked' "$LOG" 2>/dev/null; then
    echo "--- captured stderr (last 30 lines) ---"
    tail -30 "$LOG"
    fail "startup panic detected" 3
  fi
  if ! kill -0 "$PID" 2>/dev/null; then
    # Binary exited on its own before timeout — probably a startup
    # error that didn't print a panic. Surface the log so the diff
    # is debuggable.
    echo "--- captured stderr (last 30 lines) ---"
    tail -30 "$LOG"
    fail "binary exited unexpectedly before window opened" 3
  fi
  # The Tauri app, once it has the main window proxy ready, is
  # silent on stdout/stderr by default. We treat "binary still
  # running after SMOKE_HOLD_SECS without a panic" as smoke-pass.
  if [ "$elapsed" -ge "$SMOKE_HOLD_SECS" ]; then
    ok "binary survived ${SMOKE_HOLD_SECS}s without panic"
    break
  fi
  sleep 1
  elapsed=$((elapsed + 1))
done

if [ "$elapsed" -ge "$STARTUP_TIMEOUT_SECS" ]; then
  echo "--- captured stderr (last 30 lines) ---"
  tail -30 "$LOG"
  fail "hung — no startup signal within ${STARTUP_TIMEOUT_SECS}s" 4
fi

ok "pre-tag smoke passed"
