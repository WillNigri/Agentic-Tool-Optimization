# VHS tape for the Phase 6 announcement GIF.
#
# Records a real cross-runtime ATO session: open a session, dispatch
# to MiniMax with --stream so chunks land live, watch the response
# materialize chunk-by-chunk.
#
# Render with:   vhs RELEASES/demo.tape
# Output:        RELEASES/demo.gif

Output RELEASES/demo.gif

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
Sleep 200ms
Type "clear"
Enter
Sleep 500ms
Show

# ──── Scene 1: open a sticky session ────
Type "ato sessions new --runtime claude --title demo --human"
Sleep 300ms
Enter
Sleep 1500ms

# ──── Scene 2: dispatch with --stream so chunks render live ────
Type "ato dispatch minimax 'In two sentences: why does locking an agent success-rate floor as a CI gate matter for multi-runtime AI ops?' --stream --human"
Sleep 300ms
Enter

# The dispatch will stream its response for ~5-9 seconds. The chunks
# appear in real time as the model generates them.
Sleep 11s

# Let the final footer linger so the viewer can read the summary line.
Sleep 2500ms
