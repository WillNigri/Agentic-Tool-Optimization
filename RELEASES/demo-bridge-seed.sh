#!/bin/bash
# Pre-seed a session for the cross-runtime bridge GIF demo.
# Exports the session id to a known file so the vhs tape can pick it up.

set -euo pipefail

cd /Users/beatriznigri/Agentic-Tool-Optimization
export PATH="$PWD/apps/cli/target/debug:$PATH"

SID=$(ato sessions new --runtime claude --title "hn-demo" \
      | python3 -c 'import sys,json; print(json.load(sys.stdin)["id"])')

sqlite3 ~/.ato/local.db <<SQL
INSERT INTO session_turns (session_id, turn_index, role, text, runtime, created_at) VALUES
  ('$SID', 0, 'user', 'Should we ship the SSH adapter as v1?', 'claude', datetime('now')),
  ('$SID', 1, 'assistant', 'Yes — the SSH adapter covers the most common laptop-to-server case in ~150 LOC. @minimax please confirm and reply [CONSENSUS] on a line by itself if you agree, or push back if not.', 'claude', datetime('now'));
SQL

# Write the id to a known location so the tape can read it.
echo "$SID" > /tmp/ato-hn-demo-session.id
echo "Seeded session: $SID"
