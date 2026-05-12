# VHS tape: cross-runtime bridge demo.
#
# Reads the session id seeded by demo-bridge-seed.sh and runs the
# bridge — visually shows claude → minimax handoff with [CONSENSUS]
# termination.
#
# Setup (run once before recording):
#   bash RELEASES/demo-bridge-seed.sh
# Render:
#   vhs RELEASES/demo-bridge.tape
# Output:
#   RELEASES/demo-bridge.gif

Output RELEASES/demo-bridge.gif

Set FontSize 18
Set Width 1280
Set Height 720
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 30ms
Set Shell "bash"

Hide
Type "cd /Users/beatriznigri/Agentic-Tool-Optimization && export PATH=$PWD/apps/cli/target/debug:$PATH"
Enter
Type "SID=$(cat /tmp/ato-hn-demo-session.id)"
Enter
Sleep 200ms
Type "clear"
Enter
Sleep 500ms
Show

# Run the bridge — it reads claude's last assistant turn (which
# already contains @minimax), parses the mention, dispatches into
# the session, terminates on [CONSENSUS].
Type "ato bridge --session $SID --max-rounds 1 --human"
Sleep 300ms
Enter

# Bridge fires ~6-10s end-to-end with MiniMax. The "✓ [CONSENSUS]
# reached" line at the end is the moment.
Sleep 13s
Sleep 2500ms
