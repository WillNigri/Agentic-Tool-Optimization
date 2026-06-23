# ATO Roadmap

## Mission

**ATO is your local war room for humans and LLMs: decide together, call tools, and verify every outcome.** Drive it from a GUI, a CLI, or your coding agent over MCP — same data, same operations, same audit trail.

See [`README.md`](./README.md) for the full pitch, [`AGENTS.md`](./AGENTS.md) for the surface a coding agent reads, and [`docs/tiers.md`](./docs/tiers.md) for the open-core tiering principle (what's Free, what's Pro, and why).

> **Note on scope of this file.** This is the *public* roadmap for the open-source product. It covers the Free, local-first building blocks that live in this repo. Detailed product planning, pricing, and the Pro/Team/Enterprise (cloud) roadmap live in the private `ato-cloud` repository and are intentionally not duplicated here.

---

## Open-core boundary

ATO is open-core. The principle that decides where a feature lives:

- **Free (this repo, MIT)** = the **building blocks** you can wire together yourself. Primitives, schemas, math, viewing surfaces, config formats, the CLI, the desktop app, the MCP server. *"Capability without the harness."*
- **Pro / Team / Enterprise (the private `ato-cloud` repo)** = the **codified, hosted automations** on top of those building blocks — orchestrators, hosted scheduling, cross-machine sync, team state, and org governance. *"Same capability, but we run it for you."*

Three tests before adding a feature to this repo:

1. **The fork test.** If someone forked this repo and removed every tier-gate call, would they get the feature? If yes, it belongs here (Free). If that would hand them a hosted/Pro automation, it belongs in `ato-cloud`.
2. **The by-hand test.** Can a user reproduce it with a shell script around `ato dispatch` + their own tooling? If so, that recipe *is* the free path; the one-button version is Pro.
3. **The split test.** If a feature is part-primitive, part-automation, split it: the primitive stays free here, the codified automation goes to `ato-cloud`. Document both in [`docs/tiers.md`](./docs/tiers.md).

When in doubt, a Pro/Team/Enterprise feature does **not** belong in this repo.

---

## Recent highlights (shipped)

A high-level view of what's landed. See `CHANGELOG.md` for the full release history.

- **Multi-runtime dispatch** — one interface across Claude Code, Codex, Gemini CLI, OpenClaw, Hermes, plus API providers (Anthropic, OpenAI, Google, and more). Every dispatch is logged with an auditable receipt.
- **Grounded mode + tool-use loop** — require an agent to actually call `read_file`/`grep`/etc. before its answer counts; receipts record which tools each model used.
- **War rooms & sessions** — run the same question across multiple runtimes, multi-round, with a shared transcript and a close/summarize lifecycle.
- **Methodology runner** — a reusable test recipe (prompts × models × reps) scored with a rubric, with per-cell statistics.
- **Loops** — a visual composer (drag-and-drop) and a CLI (`ato loop`) to chain steps into a repeatable workflow.
- **Missions** — coordinate multiple coding agents across worktrees toward a goal, with a per-mission narrative and budget/loop guards.
- **Inputs & output bundles** — reusable markdown context bundles (`ato inputs`) any agent/loop can reference, and packaged run results (`ato bundles`).
- **Passive observation** — locally observe native CLI sessions (Claude Code / Codex / Gemini) and tag every dispatch with its billing surface.
- **Local-first security** — API keys encrypted at rest under an OS-keychain-backed master key, with `ato master-key heal-orphans` for recovery.
- **Cross-surface parity** — GUI, CLI, and MCP server (for coding agents) operate on the same local SQLite data.

---

## OSS roadmap (Free building blocks)

Near-term work in this repo. (Cloud/Pro/Team items are tracked privately.)

### Loop runner completeness (in progress)

Finish wiring the loop execution engine so the visual composer's nodes all run:

- ✅ **Variable substitution** — `{{vars.x}}` and `{{steps.<node>.output.<field>}}` resolve against run inputs + prior step outputs.
- ✅ **`input` / `output` node kinds** — reference an inputs bundle; emit an output bundle.
- ✅ **`score` node kind** — run a rubric (regex / structural / composite) over a prior step.
- ✅ **`review` / `war_room` node kinds** — multi-seat panel / review-with-consensus as a step.
- ✅ **`diagnose` / `apply` node kinds** — propose/apply an agent-definition change (Pro: delegates to `ato-pro`).
- ✅ **Per-step retry** — `config.retry = { max_attempts, backoff_ms }`, with retry only on real failures.
- 🟡 **Control flow** — parallel independent steps + decision branching (deferred: most loops are sequential and the `war_room` node already covers fan-out; revisit on demand).

### Other Free primitives

- **Git linkage** — every dispatch stamps the current commit SHA (shipped); surfacing it in more views.
- **Cost accuracy** — keep the local cost math correct across every provider's token classes.
- **Observation tiers** — broaden passive observation of local terminal LLM usage.

---

## Contributing

This repo is the place to contribute Free, local-first features and fixes. If a proposed feature is a hosted or multi-user automation, it likely belongs in the private cloud repo — open an issue and we'll help place it. See [`CONTRIBUTING.md`](./CONTRIBUTING.md) and [`docs/tiers.md`](./docs/tiers.md).
