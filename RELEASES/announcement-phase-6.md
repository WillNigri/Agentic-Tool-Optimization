# Phase 6 announcement drafts

Two drafts for two channels. The earlier @iamknownasfesal reply has already been posted.

**Demo GIFs** in this directory:

- [`demo.gif`](demo.gif) — single-runtime streaming dispatch with chunks rendering live. ~15s, 217K. Best for the X thread's first post (shows ATO's "alive" feel without needing the viewer to understand the bridge yet).
- [`demo-bridge.gif`](demo-bridge.gif) — cross-runtime bridge: a seeded claude turn @-mentions minimax, `ato bridge` fires, MiniMax replies with `[CONSENSUS]`, loop exits. ~15s, 84K. The headline visual for the HN post.

Render either with `vhs RELEASES/demo.tape` / `vhs RELEASES/demo-bridge.tape`. The bridge demo needs `bash RELEASES/demo-bridge-seed.sh` run once first to pre-create the session.

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

[attach demo-bridge.gif]

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

## Draft 2 — HN follow-up post

*Title*: `Show HN: ATO – cross-runtime AI agent conversations + a CI gate for agent quality`

*Inline the [`demo-bridge.gif`](demo-bridge.gif) right after the third paragraph.*

Hey HN,

I posted ATO 54 days ago as a GUI for seeing what your LLM agents actually wrote to disk. Since then we've shipped Phase 6: AI agents on different runtimes can hold a single conversation through ATO, and you can lock each runtime's quality as a CI gate.

The problem: when you run Claude Code + Codex + Gemini side-by-side, you can't easily get them to talk to each other. You copy responses between terminals. The agents don't see each other's work. And there's no way to know if a recent config change quietly dropped your agent's success rate by 17pp until something breaks.

What Phase 6 adds:

- *Cross-runtime sessions*: one `ato sessions new`, multiple runtimes contributing turns. History replay for runtimes that don't share state on their end (MiniMax, Grok, DeepSeek, Qwen, OpenRouter) → native `--resume` where they do (Claude Code).
- *@-mention bridge*: when Claude's reply contains `@minimax please weigh in`, `ato dispatch ... --tag-bridge` parses the tag, dispatches into MiniMax, threads the reply back into the same session. Loops until `[CONSENSUS]` / `<consensus/>`, a round cap, or no further mention. Heuristic backstop on 3 consecutive turns from one runtime escalates to the activity feed for human review.
- *Eval-score ratchet*: `ato ratchet lock --target runtime:claude --days 30` computes the rolling success rate and persists it as a floor. `ato ratchet check` exits non-zero in CI when the recent 7-day window drops below floor − tolerance. Inspired by @garrytan's "AI agent complexity ratchet" post.
- *SSH-backed remote runtimes*: someone on X asked how to get their laptop Claude to talk to their server Claude. `ato runtimes add-remote --host you@server --runtime claude` registers a remote slug; `ato dispatch claude-server "..."` routes over SSH. Same audit / sessions / bridge surface. One-way; the reverse direction (server → laptop) is roadmap when there's user pull.
- *Runtime health check*: detects revoked Developer ID certs (CSSMERR_TP_CERT_REVOKED) on macOS before they crash you with a generic malware dialog. JS-shim aware — descends into `node_modules/<pkg>-darwin-<arch>/` to verify the bundled Mach-O on npm-installed runtimes like codex.
- *SSE streaming for API providers*: closes the 7–15s buffered wait on MiniMax / Grok / DeepSeek / Qwen / OpenRouter. CLI flag `--stream`; GUI Sessions transcript renders chunks live.
- *MCP server*: 8 → 52 tools. Sessions / bridge / ratchet / posts / health all callable from MCP-only harnesses. Includes a stdio smoke test (`npm run qa:stdio`).

The repo also ships a Claude Code skill at `.claude/skills/ato-review/SKILL.md` that instructs Claude to dispatch every non-trivial diff to a reviewer runtime via `ato dispatch` before committing — apply findings or defer with a recorded justification in the commit body. The skill itself was reviewed this way before shipping; the review caught a Python f-string quoting bug in the session-reuse path. Two layers of dogfood.

What's out of scope: ATO is still a dev-workflow ops layer, not a production agent SDK — Langfuse / Helicone cover the prod sidecar story. The `[CONSENSUS]` termination depends on the model phrasing the cue the way the bridge prompt asks; tested across several internal sessions but isn't a deterministic regression test until a mock-LLM fixture lands.

All data is local: `~/.ato/local.db` (SQLite). MIT, installers for macOS (Apple Silicon + Intel), Windows, Linux.

https://github.com/WillNigri/Agentic-Tool-Optimization

Happy to go deep on any part — the bridge implementation, the ratchet design choices, or why the bi-directional daemon mesh is out of v1.
