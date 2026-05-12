# Phase 6 announcement drafts

Three drafts for three channels. Pick the one that fits the audience.

---

## Draft 1 — X thread (3 posts)

**1/**
ATO just shipped Phase 6: AI agents on different runtimes can now hold a single conversation.

Claude and MiniMax taking turns by @-mention inside one ATO session, full audit trail in your local SQLite:

```
ato sessions new --runtime claude
ato dispatch claude "Review this. @minimax weigh in." \
    --session <id> --tag-bridge
```

The bridge loops until `[CONSENSUS]` / `<consensus/>` lands on its own line, or the round cap hits.

**2/**
Pair it with the eval-score ratchet: lock a quality floor, fail CI when recent runs drop below it.

```
ato ratchet lock --target runtime:claude --days 30
ato ratchet check                  # exit 1 on quality drift
```

Inspired by @garrytan's [AI Agent Complexity Ratchet](https://garrytan.com/) — the framing "make 90% test coverage free" applied one layer up.

**3/**
Also shipped: SSH-backed remote runtimes (answer to @iamknownasfesal's question about laptop ↔ server agents), SSE streaming for API providers, runtime-binary health check that catches revoked Developer ID certs, and `ato-review` — a Claude Code skill that makes "use ATO to review your diff" the default.

MIT. github.com/WillNigri/Agentic-Tool-Optimization

---

## Draft 2 — Reply to @iamknownasfesal

Hey — circling back on this. ATO v2.3.32+ ships an answer:

```
ato runtimes add-remote --name claude-server \
    --host you@your-server --runtime claude
ato dispatch claude-server "<prompt>"
```

The dispatch routes over SSH, response lands in your local execution_logs / Live tab next to your laptop Claude. Same audit surface, no more copy-paste between machines. Sessions and the cross-runtime bridge work transparently with the remote slug.

One-way today (laptop initiates). Full bi-directional mesh (server agent initiates back to laptop) is on the roadmap as Phase 7 — bigger lift because of the pairing / NAT-traversal / wire-protocol story. The one-way version handles the most common case; reverse direction is a deliberate next-scoping decision when there's user pull.

Repo: github.com/WillNigri/Agentic-Tool-Optimization

---

## Draft 3 — HN / blog post

### Phase 6: AI agents that talk to each other (and a quality ratchet for when they don't)

ATO is a local-first developer-workflow ops platform for multi-runtime AI agents. The Phase 6 cluster, shipped over the last two weeks, closes a real loop: agents on different runtimes can hold a single conversation, hand work to each other by @-mention, and the quality of each runtime's contribution gets locked as a CI gate.

If you've used Claude Code + Codex + Gemini and wanted to ask "which of these would be better at this task," ATO is one answer. The pitch isn't "pick the right model" — it's "let them figure it out among themselves, with you in the loop."

#### What's new

**Cross-runtime conversations** (Slice A → B). One sticky session, multiple runtimes contributing turns. When Claude writes `@minimax please weigh in` at the end of its response, `--tag-bridge` parses the mention, history-replays the session into MiniMax, captures MiniMax's reply as the next turn, and so on until someone emits `[CONSENSUS]` on its own line (or `<consensus/>` inline). A structural backstop fires if the same runtime produces 3 consecutive turns — that posts an `approval_request` to the activity feed instead of looping forever. Multi-mention round-robin prefers runtimes that haven't yet replied so "@claude @gemini both please review" gets both opinions before re-bridging.

**Eval-score ratchet for CI**. Garry Tan's [*AI Agent Complexity Ratchet*](https://garrytan.com/) post (May 2026) argues that AI coding agents make 90% test coverage free — they don't experience effort writing the fourteenth edge-case test. We applied that framing one layer up: agent runs → status / eval scored → lock the floor → next run can't regress. `ato ratchet check` exits non-zero in CI when the recent window's success rate drops below floor minus tolerance. Drop it in pre-deploy; a regression that drops your agent's success rate by >5pp fails the pipeline the same way a unit test would. Cloud `eval_score` can layer onto the same table when local evaluator hooks land — the schema's `metric` discriminator is already there.

**SSH-backed remote runtimes** answers a question [@iamknownasfesal](https://x.com/iamknownasfesal) asked on X last week: "how can I make my Claude agent on my computer talk to the one on my server? atm just copying responses into each other lol." `ato runtimes add-remote --host you@server --runtime claude` registers a remote endpoint with a local slug; `ato dispatch claude-server "..."` then routes over SSH with `BatchMode=yes ConnectTimeout=15`. The remote ATO doesn't need to run; it's just executing the binary on the laptop's behalf. Sessions and bridge work transparently with the remote slug. Full bi-directional daemon mesh (server-initiated calls back to laptop) is roadmap as Phase 7+ — pairing / NAT-traversal / wire-protocol is a multi-week story we'll scope when there's user pull for the reverse direction.

**Runtime health + the JS-shim sidecar walk** caught a real production class of bug: when Apple revoked OpenAI's Developer ID cert, codex stopped working, and macOS's response was a generic malware dialog with no actionable signal. v2.3.34 added `ato runtimes health` running `codesign --verify` + reading `com.apple.quarantine` xattrs. v2.3.36 added the GUI banner with a one-click "Run fix" button that executes the canned fix command through an IPC allowlist (no `sh -c` of untrusted input). v2.3.44 added the JS-shim sidecar walk because npm-installed runtimes are `#!/usr/bin/env node` scripts — the codesign check on the shim itself doesn't catch a revoked cert on the bundled Mach-O. The walker descends into `node_modules/<vendor>/<pkg>-darwin-<arch>/` (including the nested `vendor/<triple>/<runtime>/<runtime>` layout codex uses) and verifies the actual binary.

**SSE streaming for API providers** closes the 7–15s "wait silently for MiniMax to finish" UX gap. Plumbed through to the GUI Sessions transcript so chunks render live as the model generates them. CLI gains `--stream` (raw to stdout) and `--stream-jsonl` (line-delimited JSON events for desktop / wrapper consumption).

**MCP server** grew from 8 to 52 tools covering Observation / Operations / Authoring / Sessions / Posts / Runtimes / Events / Ratchet. Each new tool wraps the canonical CLI implementation, so the same algorithm runs on both surfaces and a schema migration only needs testing in one place. v2.3.51 ships `npm run qa:stdio` — a smoke test driving the server over real JSON-RPC stdio.

#### The meta-flex: `ato-review`

`.claude/skills/ato-review/SKILL.md` ships in the repo. When a Claude Code session opens this codebase (or any repo with the skill installed), the skill instructs Claude to dispatch every non-trivial diff to a reviewer runtime via `ato dispatch <reviewer> --session review/<branch>` before committing. Findings get applied or deferred with a one-line justification, and the audit trail lands in the commit body. Tan's complexity ratchet applied to the review step rather than test coverage.

The skill itself was MiniMax-reviewed before shipping. That review caught a Python f-string quoting bug in the session-reuse path that would have caused a SyntaxError on every commit — the review primitive caught a real bug in the review primitive.

#### What's not in scope

ATO is positioned as the developer workflow / ops layer, not a production agent SDK. Langfuse, Helicone, and the existing observability vendors cover the production sidecar story. ATO sits next to them.

The Phase 7 daemon mesh — bi-directional discovery + wire-protocol calls between ATO daemons on different machines — is intentionally blue-sky until a real user asks for the reverse direction. The SSH adapter handles the most common laptop → server case; the mesh is a multi-week security / pairing / NAT story that shouldn't autopilot.

Slice B's `[CONSENSUS]` termination depends on the model phrasing agreement the way the bridge cue asks. Tested across several internal cross-runtime sessions on this build; the mock-LLM regression fixture that would make this a real guarantee is targeted for v2.4 and tracked in QA.md §5.

#### Try it

```
npm install -g @ato/desktop
ato setup-path

# Cross-runtime conversation in one command
SID=$(ato sessions new --runtime claude --title demo | jq -r .id)
ato dispatch claude "Review X. @minimax weigh in." --session $SID --tag-bridge

# Quality ratchet for CI
ato ratchet lock --target runtime:claude --days 30
ato ratchet check  # add to your pre-deploy step

# Remote runtimes over SSH
ato runtimes add-remote --name claude-server --host you@server --runtime claude
```

Full release notes: [RELEASES/v2.3.x-phase-6.md](RELEASES/v2.3.x-phase-6.md). MIT, local-first, no cloud sign-in required for any of the above.

[github.com/WillNigri/Agentic-Tool-Optimization](https://github.com/WillNigri/Agentic-Tool-Optimization)
