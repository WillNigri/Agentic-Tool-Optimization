# ATO — Your agents are on the wrong model

> **Same prompt. Sixteen runtimes. One cost table with quality scores. ATO tells you which LLM actually won — and what to switch.**

```
$ ato demo-compare

Comparing "Explain merge sort in Python"...
Runtimes: google, claude, codex

| Runtime |  Duration | Tokens |    Cost | Quality |
|---------|-----------|--------|---------|---------|
| google  |     1.8s  |    450 | $0.0011 |  ✓ pass |
| codex   |     2.1s  |    410 | $0.0035 |  ✓ pass |
| claude  |     3.2s  |    380 | $0.0240 |  ✓ pass |

→ All passed. google solved it 22× cheaper than claude.
```

### Start in 30 seconds

```bash
# macOS
brew install willnigri/ato/ato && ato demo-compare

# Linux
curl -fsSL https://agentictool.ai/install.sh | sh && ato demo-compare

# Windows — download the desktop app:
```

**[Download →](https://github.com/WillNigri/Agentic-Tool-Optimization/releases/latest)** (macOS · Windows · Linux · MIT Licensed)

`demo-compare` works immediately — picks your configured runtimes, or falls back to Ollama (runs open-source models locally). No API keys required for the first run.

### Then point it at your real work

```bash
ato dispatch claude "review this PR for security issues"
ato dispatch google "review this PR for security issues"
ato compare <run-a> <run-b>
```

Every dispatch builds the dataset. Run 20–50 across your most common tasks — that's when `optimize recommend` starts producing real recommendations.

---

## After ~20 real dispatches

Once you've dispatched real work across two or more runtimes, ATO recommends switches — usually within a day of active use:

```bash
ato optimize recommend --human
```

```
Cost Recommendations (based on YOUR data)

  1. Switch CLAUDE → GOOGLE
     Savings: 85% per round ($0.0245 → $0.0037)
     Evidence: 81 head-to-head rounds (confidence: HIGH)
     Quality: scored by LLM judge — comparable
```

Computed from rounds where both runtimes answered the same prompt and a judge scored both. Not vibes. Receipts.

---

## Free vs Pro

Everything local is free forever. Pro automates what you'd do manually.

| | Free | Pro ($29/mo) |
|---|:---:|:---:|
| Dispatch + compare across runtimes | yes | yes |
| Cost optimization recommendations | you run it | **runs while you sleep** |
| Quality checks (pass/fail) | ✓ heuristic | **+ LLM judge (scored on our key, not yours)** |
| Cloud traces + regression alerts | -- | 30-day cross-device |
| Scheduled evaluators | -- | hourly / daily / weekly |

**Free**: you drive. **Pro**: ATO replays your prompts overnight, scores quality, and tells you what to switch.

**[Start free →](https://github.com/WillNigri/Agentic-Tool-Optimization/releases/latest)** · **[ATO Pro →](https://agentictool.ai/pro)**

---

## Supported runtimes

**CLI:** Claude Code · Codex · Gemini CLI · OpenClaw · Hermes · Ollama
**API:** OpenAI · Google AI · Anthropic · Mistral · Groq · xAI · DeepSeek · Qwen · MiniMax · OpenRouter

ATO rides your existing CLI logins the way VS Code rides your GitHub login — or use stored API keys.

---

## Docs

**[CLI Reference](AGENTS.md)** · **[Architecture](docs/architecture.md)** · **[Contributing](CONTRIBUTING.md)** · **[Roadmap](ROADMAP.md)**

---

**[Website](https://agentictool.ai)** · **[ATO Pro](https://agentictool.ai/pro)** · MIT Licensed
