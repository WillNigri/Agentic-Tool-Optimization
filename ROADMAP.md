# ATO Roadmap

## Released

### v0.3.0 — Multi-LLM Platform
- Multi-runtime support: Claude Code, Codex, OpenClaw, Hermes
- Two-way communication with all runtimes (send prompts + get status)
- Skills Manager with per-runtime tabs and recursive scanning
- Skills Marketplace (browse, install, publish, share, auto-improve)
- AI-powered skill creation with in-app approval dialog
- Automation Builder with auto-detection from skill content
- Cron Monitor with Google Calendar view and click-to-inspect
- Per-runtime Context Visualizer (skills shown as on-demand)
- Setup Wizard for first-time runtime configuration
- Subagents Manager with runtime selection
- MCP server with 8 tools including runtime status
- GitHub Actions CI for macOS, Windows, Linux
- i18n: English, Portuguese, Spanish

### v0.4.0 — Monitoring & Analytics
- Real-time log viewer with file watcher
- Background health polling for all runtimes
- Usage analytics dashboard with execution metrics
- Latency/uptime charts per runtime
- Cost tracking per runtime with burn rate visualization

### v0.5.0 — Cloud Sync & Collaboration
- Cloud backend (ato-cloud) with PostgreSQL
- GitHub OAuth login
- Team workspaces with shared skill libraries
- Team member management (invite, roles, permissions)
- Team skills sharing and collaboration
- Activity logs for audit trail
- Skill sync across devices

### v0.6.0 — Deeper Runtime Integration
- Live context tracking from runtime session logs (reads Claude session JSONL)
- Real MCP tool discovery (JSON-RPC protocol to running MCP servers)
- Config editor with write support (FileViewer with save functionality)
- Hooks read/write from actual settings files (HooksManager + Tauri commands)

### v0.7.0 — Marketplace Backend
- Marketplace service with PostgreSQL schema
- Skill submissions with versioning (semver)
- Search, filter, and discovery endpoints
- Ratings and reviews with helpfulness voting
- Skill packs (collections) with import/export as JSON
- Update notifications for installed skills

### v0.8.0 — Advanced Automation
- Webhook triggers (inbound) with path/method/secret configuration
- Parallel node execution with group tracking
- Error handling nodes (try-catch, retry with exponential backoff)
- Variables and data passing between nodes (set, get, transform, jq expressions)
- Workflow templates (4 built-in: Webhook to Slack, Parallel Deploy, Error Handling, Data Transform)
- New node types: parallel, try-catch, retry, variable, template
- Enhanced execution state with runId, trigger payload, parallel groups, retry tracking

### v0.5.5 — Notifications & Integrations
- Notifications service with provider abstraction (Tauri backend)
- Slack webhook integration (Block Kit formatting)
- Discord webhook integration (embed support)
- Telegram bot integration (Markdown formatting)
- Email notifications (SMTP - placeholder, requires lettre crate)
- Notification preferences per event type (8 event types)
- Desktop UI for managing notification channels (existing component, now connected to backend)
- SQLite persistence for channel configurations
- Test notification functionality

---

### v1.0.0 — Production Ready (Released April 2026)
- SDK (`@ato-sdk/js`), web dashboard, cost tracking
- LLM API key management, audit logging, agent monitor
- SSO, rate limiting, Homebrew tap

### v1.1.0 — Projects Dashboard + Multi-Runtime (Released April 2026)
- Projects Dashboard with 7 Claude sections + multi-runtime switcher
- 6 runtimes: Claude Code, Codex/OpenAI Agents SDK, Gemini CLI/ADK, OpenClaw, Hermes
- Ollama provider: auto-detect, model picker, copy endpoint
- CodeMirror 6 editor with conflict detection, auto-backup, audit logging
- Sandbox config + approval policies (editable with write-back)
- File watcher, token chart, backup/restore, i18n (EN/PT/ES)
- 46 tests (35 Rust + 11 frontend), CI/CD, code splitting

### v1.2.0 — Agent Command Center (In Progress)
- Visual workspace canvas: drag nodes, zoom in/out, pan
- Live execution visualization: agent activity pulses nodes, animated edge dots
- Skill palette: drag-to-install from marketplace with suggestions
- Command palette (⌘K): search nodes, skills, actions
- Multi-select batch operations on skill nodes
- Grid + Canvas dual view mode
- Strategy game-inspired UX: semantic zoom, animated transitions

---

### v1.3.0 — The GUI Pivot (Released May 2026)
**Goal: become the place where you create an agent, not just manage one.** Repositioning ATO from "multi-runtime control panel" to "the GUI for creating, managing, and observing AI agents" — for non-technical users, power users, and teams alike.

- **IA collapse: 24 sidebar entries → 6 sections** (`Home`, `Agents`, `Skills & MCPs`, `Runs`, `Insights`, `Settings`)
- **New `Home` page** with "Create Agent" CTA, recent agents, recent runs, alerts
- **Create Agent — Guided (chat path)**: multi-turn LLM-driven wizard with required questions (domain, tone/style, filesystem scope, permissions, optional skills); draft persistence; runtime/model/skill/MCP suggestion cards; conversation runs on the user-picked runtime
- **Create Agent — Quick (form path)**: one-page form with all fields visible (project picker, skills/MCPs multi-select, draft auto-save)
- Agent record in SQLite + file-writing for `~/.claude/agents/`, `~/.codex/agents/`, `~/.gemini/agents/`, `~/.openclaw/agents/`, `~/.hermes/agents/`
- **MCP install UI** in `McpDashboard`: registry browser + custom install + path-scoping picker for filesystem MCP (folders the agent can access, with a native folder picker)
- **Embedded terminal**: full xterm.js + portable-pty shell — Chat / Shell modes
- **Subscriptions OR API keys** — first-class auth dual: VS-Code-style detection of `claude` / `codex` / `gemini` CLI logins, OR stored API keys, user's choice per runtime
- **Run loop** (the F5 of agents): per-runtime invocation matrix; "Run" button on every agent card opens an interactive shell scoped to that agent (real persistent session, full memory); "Quick test" dialog for stateless single-shot
- **Cross-runtime dispatch via MCP**: ATO's MCP server now exposes `list_agents` + `run_agent` tools, so any runtime configured with `ato` MCP can natively invoke any ATO-managed agent regardless of which runtime owns it. Resolves the "Codex/Gemini @-mention isn't native" caveat at the protocol layer. Cross-runtime calls auto-log to `~/.ato/agent-logs.jsonl`.
- **Runtime parity matrix** on Customize Overview — honest table of what each runtime supports for create/install/run, with notes about the limits we can't paper over
- Merge Configuration + Runtime Settings → `Settings → Runtimes`
- Fold Cloud Sync / Teams / Skill Sync / Notifications under `Settings → Cloud`
- Demote Workspace canvas to a sub-view of `Settings → Projects`
- Command palette (⌘K) — promoted from v1.2.0
- i18n strings for all new copy (EN/PT/ES)

### v1.4.0 — Production-Grade Agent Authoring (Released May 2026)
**Goal: turn ATO from "agents-as-static-files" into "agents-as-context-engineered-systems."** Driven by industry consensus on what makes production agents survive vs. demos: dynamic context, specialization, observability. This is also where Free / Pro / Team / Enterprise tier gating becomes visible across the product.

**The seven context-engineering primitives:**
- **F1. Dynamic prompts with variables** — `{var}` syntax with resolvers (static / env / project-path on Free; file / db-query / mcp-call / computed on Pro). Variables tab on every agent.
- **F2. Pre-call context hooks** *(Pro)* — ordered list of resolvers that fire before each turn and inject results into a `<context>` block in the user message. CRM / DB / file / webhook / MCP-call / computed.
- **F3. Conversation summarizers** — per-agent Memory tab. Summarize-when-N + keep-last-K + summarizer-model. Free has fixed defaults; Pro is tunable.
- **F4. Multi-agent groups (router + children)** — first-class object: a router + N specialized child agents (Free up to 3 children; Pro unlimited). Visual graph editor reusing the AutomationFlow canvas patterns. Routers support rules + LLM-classifier + hybrid mode. MCP `run_agent` transparently dispatches group slugs through the router.
- **F5. Per-task model selection** *(Pro)* — agents gain `roleModels: { router, summarizer, response, evaluator }` so cheap-fast models handle routing/classification while advanced models handle the response.
- **F6. Tracing + observability dashboard** — Insights → Agent observability: success rate, latency, token cost, last 100 runs (Free); 30/90/∞-day cloud retention (Pro/Team/Enterprise). Trace explorer shows the full sequence (variables → hooks → router → child → response → tool calls).
- **F7. Evaluators** *(Pro)* — manual + scheduled batch only (never live). Heuristic (substring/regex/length/tool-was-called) + LLM-as-judge. Quality scores show in the dashboard.
- **F8. Tool description quality** — "Improve description for this agent" button uses the agent's runtime to rewrite MCP descriptions in context of the agent's actual goal.

**Plus the original v1.4 polish items, all shipping in the same release:**
- Agent templates / blueprints (5 starters: PR reviewer, doc writer, codebase explainer, data analyst, devops helper)
- Skill version history + rollback (DB schema bump)
- Global search across agents / skills / projects / secrets / audit (powers ⌘K)
- Configuration export/backup (.zip of all configs + restore flow)
- Runtime comparison surface (lift the buried `RuntimeComparisonModal` to `Settings → Runtimes → Compare`)
- Bulk skill operations (multi-select enable/disable)

**Tier gating UX:** Pro features are visible to Free users with a small crown lock badge + "Upgrade to Pro" tooltip — Linear / Notion / Figma pattern. Discovery sells; hidden features can't drive upgrades.

**Cloud-side pairing (in `ato-cloud`):**
- Migration `008_v1_4_0_observability.sql` — `agent_traces`, `agent_evaluations`, `agent_groups` (synced).
- New route `POST /agent-traces` (`requireTier('pro')`) — receives traces, persists, computes aggregates.
- Tier checks expand on existing `requireTier` middleware.

**Detailed ticket-by-ticket build plan**: see `docs/V1.4.0-IMPLEMENTATION.md`.

### v1.5.0 — Daily Workspace (Released May 2026)
**Goal: turn ATO from "control panel for agents" into "the place where you do agentic work."** The pivot from configuration GUI to daily workspace.

- **Persistent chat threads** — SQLite-backed `chat_threads` + `chat_messages` tables; conversations survive restart, listed in a dropdown with msg count + last activity, scoped optionally to active project, rename via double-click, delete via hover trash
- **Multi-runtime mid-thread** — switch Claude → Codex → Gemini in the same conversation. Full thread history travels to whichever runtime answers next, regardless of agent selection
- **Streaming responses** — `prompt_agent_stream` / `prompt_agent_with_history_stream` Rust commands stream stdout via `tokio::process::Command` + `tauri::ipc::Channel<StreamEvent>`. Tokens appear live with a blinking cyan caret
- **Syntax-highlighted markdown** — `react-markdown` + `remark-gfm` + `rehype-highlight`; assistant messages render as proper markdown (headings, GFM tables, fenced code blocks with hover-revealed Copy button), user/error/attachment stay raw
- **File attachments** — paperclip pick or drag-drop a text file (≤32KB, binary refused); contents wrap in `<attachment>` block and join history
- **Per-thread sticky agent** — picking an agent persists it to the thread's `agent_id`; switching threads restores the agent
- **Runtime mid-thread for no-agent path** — frontend stitches thread history into a single framed prompt so cross-runtime swaps without an agent still carry context

### v1.5.5 — Production-Ready Agents (Discoverability) (Released May 2026)
The dynamic-prompt features that landed in v1.4.0 (variables, hooks, summarizers, evaluators, per-task models) are powerful but **invisible to most users** — Felipe spent weeks building agents and didn't realize they exist. v1.5.5 closes the gap between "we have it" and "users know we have it":
- **Production-grade agent template** — a 6th template (`production-grade`) wired up with 4 example variables (env / project-path / computed for `{user_name}`, `{project_name}`, `{project_root}`, `{today}`), one pre-call context hook reading `CHANGELOG.md`, and a memory policy. The wizard honors `dynamicScaffold` so creating from this template lands the variables, hooks, and policy in the DB — not just the system prompt.
- **First-run welcome tour** — `WelcomeTour` 3-slide modal gated on `localStorage["ato.welcome-tour.shown"]`. Plants the "agents adapt at fire time" mental model, ends by sending the user straight to the Production template via `openCreateAgent("templates", "production-grade")`.
- **Empty-state CTAs** on Variables / Context / Memory / Models tabs that point at the Production template — Memory and Models use a header-line hint since those tabs are configured-by-default and never go truly empty.
- **Settings → API Keys** — Grok added in v1.5.4; wizard hint lists all 15 providers.

### v1.6.0 — Intelligence Layer (Planned)
- **Automations tab repurpose — group pipelines as flow nodes** ([detailed plan](docs/V1.6.0-AUTOMATIONS-REPURPOSE.md))
  - Today the Runs → Automations tab visualizes skill-derived flow charts (parsed from `## Step N` / `## Phase N` headers in SKILL.md files). Useful but narrow — and v1.5 groups now own the word "automation."
  - v1.6 turns it into the canonical visualization for **everything that runs without a human in the loop**: routed groups, sequential pipelines, scheduled cron jobs, hooks, and skill flows — all on the same canvas. Each node is a real agent / runtime / tool with live status (idle / running / errored).
  - Sequential group "Claude → Codex" becomes a left-to-right flow with arrows showing data flow; routed group becomes a fan-out from the router node; cron jobs anchor at the left edge with a clock icon.
  - Click a node → Insights opens that agent's trace explorer for the last N runs.
- Real-time collaborative workspace (WebSocket via ato-cloud)
- Team cursors (Figma-style)
- Cross-runtime policy enforcement templates
- Hosted terminal sessions for Team tier (cloud)
- Proactive suggestions ("Your project is missing X")
- Cost optimization alerts from SDK traces
- Agent performance benchmarking across runtimes
- **HALO integration** — feed traces from `~/.ato/agent-logs.jsonl` into Context Labs' HALO RLM engine (MIT, on PyPI), surface harness-improvement reports as one-click inline diffs

### v2.0.0 — External Agents / Hosted Deployment (Released May 2026)
The strategic v2 release: ATO becomes the place where companies build customer-facing chatbots, deploy them to their own infrastructure (any LLM provider), and track their behavior — without us competing with hosting providers. ([detailed plan](docs/V2.0.0-EXTERNAL-AGENTS.md))

Shipped across alpha.1–alpha.5:
- **"Internal vs External" toggle on agent create** — external agents get a Deploy tab + Knowledge tab + Raw tab, surface the relevant chat-LLM provider keys (all 9 providers), and skip Skills/MCPs/Project that don't apply.
- **Knowledge ingestion** — drag-drop text + ingest, multi-provider embeddings auto-detected across OpenAI / Voyage / Gemini / Cohere / Ollama. Stored locally in SQLite (`agent_knowledge_chunks` table) with cosine similarity retrieval. Inlined into deploy bundles so the deployed agent stays self-contained.
- **Deploy targets** — generate a deployable bundle for any of the 9 chat-LLM providers (Anthropic, OpenAI, Gemini, Groq, Mistral, DeepSeek, xAI, Together, Fireworks). Templates: Cloudflare Worker, Vercel Edge Function, Docker, standalone Node script.
- **Embed widget** — vanilla-JS chat-bubble (~250 LOC, IIFE, no deps) emitted with every deploy bundle. `data-*` attribute config, localStorage history, customer-brandable.
- **Trace sink integrations** — one-click forward from each bundle to Langfuse + generic webhook (OTLP shape) in addition to the ATO Insights pipeline. We don't compete with request-level tools; we own agent-level + multi-runtime + embed-side.
- **Insights → External tab** — per-agent metric cards (run count / success rate / p50/p95 latency / cost over 7/30/90d window), drill-down trace explorer. Reads cloud `/api/agent-traces*` (Pro tier).
- **Apple Developer signing + notarization** — production CI signs and notarizes every macOS DMG so customers don't see Gatekeeper warnings.

Deferred to a v2.0.x patch:
- **Bundle → cloud trace forwarding auth** — bundles POST `Bearer ATO_TRACE_KEY`, cloud expects JWT. External-bundle traces silently 401 today; internal-agent traces flow correctly.
- **External API + DB connections as scoped tools** — pushed to v2.1+ alongside the eval workbench.

### v2.1.0 — Multi-Runtime Differentiated Observability (Released May 2026)
The unique-value layer on top of v2.0's deployment surface — what existing observability tools cannot do because they don't have our multi-runtime + agent-design context.

Phase 1 — Trace pipeline (shipped):
- **Embed-key auth path** — `users.embed_key` column + `POST /api/agent-traces/embed` Bearer-auth route. Bundles deployed to Cloudflare/Vercel/Docker/Node now successfully forward traces back to ato-cloud (the Wave 5 write-side gap is closed).
- **Gateway routes** — `/api/agent-traces` and `/api/agent-traces/embed` properly proxied (the v1.4.0 skills route was orphaned for 6 months).
- **Embed key UI** — DeployTab surfaces the per-account ATO_TRACE_KEY with reveal/copy/rotate.

Phase 2 — Configuration impact ledger (shipped):
- **`agent_config_changes` table** — versioned audit of every model swap, prompt edit, hook add. Recorded automatically by `createAgent` + `updateAgentMcps` + `updateAgentMemoryPolicy` + `updateAgentRoleModels` + `updateAgentKind`.
- **AgentDetail History tab** — per-agent timeline of changes with field, old→new diff, actor, timestamp.
- **Dashboard overlay** — External Insights drill-down merges traces + change markers chronologically so regressions tie back to specific edits.

Phase 3 — File attribution per dispatch (shipped):
- **Mtime-snapshot diff** — Rust `file_attribution` module captures pre/post project-state snapshots around every dispatch; diffs them into the list of files the agent touched. Works for every runtime since it's filesystem-level.
- **`agent_traces.files_touched` JSONB column** — populated by every `prompt_agent_with_context` and streaming dispatch; surfaced in External Insights drill-down with collapsible per-trace file list.
- **The "detective work" answer** — multi-agent runs (sequential pipelines, routed groups) now show which agent touched which files without manual git-blame.

Phase 4 — Live runs registry (shipped):
- **In-memory active-runs map** (Rust `active_runs.rs`) — every `prompt_agent_with_context` dispatch registers `(run_id, agent_slug, runtime, workspace, started_at, status)` for the duration of the run.
- **Insights → Live sub-tab** — polled every 2s; shows what's running with workspace/runtime/elapsed time + a one-click Kill button per row. Default tab so the live state is the first thing users see.
- **The "missing ops layer"** answer (Twitter feedback): no more reading every terminal buffer to find the stuck dispatch.

Phase 5 — Partially shipped (continues):
- **Pipeline trace visualizer** *(shipped v2.0.1)* — multi-stage dispatches grouped by `parent_run_id` get their own Insights → Pipelines sub-tab with handoff arrows + per-stage timing/files. Honest scope: kind-agnostic, mirrors the External tab's strict-by-design filter philosophy.
- **Eval workbench (compare traces)** *(shipped v2.0.1)* — Insights → Compare sub-tab. Lists agents with ≥2 cloud traces; click → existing `TraceCompareModal` opens with diff view (duration/cost/files/ok-status). Replaces the v1 pattern of routing the comparison surface through the External-only drill-down.
- **Cross-runtime regression detection** *(v1 shipped, deep version shipped v2.0.2)* — `/agent-traces/regressions` joins config changes × traces × evaluations. RegressionsPanel cards show eval-score delta column + "View N failing examples →" drill-down opening a modal of post-change failing traces with prompts/errors. AgentDetail surfaces a regression banner above the tab nav when its agent has an active regression. Severity widens to fire on ≥15pp eval drop even when ok-rate is unchanged. Cross-runtime A/B (replay) still deferred — needs full-prompt persistence + re-dispatch infra (separate release).
- **Cost optimization recommendations** *(prescriptive layer shipped v2.0.3)* — `/agent-traces/cost-recommendations` surfaces same-agent swaps when historical multi-runtime data exists and the alt is ≥30% cheaper at ok-rate within 10pp + eval-score within 5pp. Rendered as a section above Insights → Usage benchmarks. Render-nothing when no recs in window so the panel stays clean. Shadow-evaluation across never-tried runtimes (the "would Codex be cheaper?" case) still requires replay infra (next release).
- **Embed-side analytics** — page where the chat lives, time-to-first-message, drop-off rate per message turn, escalation keyword clusters.

### v2.0.1 — Honest Surfaces + Plumbing Fixes (Released May 2026)
Patch on `2.0.0`. All fixes for things broken-as-shipped, plus two Phase 5 bullets pulled forward because they're also discoverability gaps.

- **Insights → Pipelines sub-tab** — multi-stage dispatches grouped by `parent_run_id`; click any row → existing `PipelineModal` opens directly. Internal pipe-writer/reviewer chains and external-bundle pipelines both land here regardless of agent kind.
- **Insights → Compare sub-tab** — `CompareTracesPanel` lists agents with ≥2 cloud traces, opens `TraceCompareModal`. Removed the previous dishonest pattern of dressing internal agents as `kind=external` to satisfy the External-tab filter; demos are now scope-honest.
- **External tab kept strict** to `kind=external` only. Empty state points users at the Pipelines tab if they're chasing internal multi-agent runs.
- **Pipeline trace upload** — desktop now emits RFC3339 with `Z` suffix (`to_rfc3339_opts(_, true)`). Cloud schema also accepts `{offset: true}`. Was 400-ing every pipeline stage with `VALIDATION_ERROR / "Invalid datetime"`.
- **Auth-store mirror** — `useAuthStore.setAuth` now writes the localStorage slot that `lib/cloud-api.ts` reads (`storeTokens()`); `logout` calls `clearTokens`; `refreshAccessToken` mirrors rotated tokens; `isCloudUser` added to `partialize`; `onRehydrateStorage` re-hydrates the localStorage mirror on app boot. Fixes the "Not authenticated" error on Deploy tab embed key + every other `cloud-api.ts` caller despite a visible Pro badge.
- **Runs → History persistence** — `prompt_agent_inner` now inserts an `execution_logs` row after every dispatch (UI, group stages, MCP `run_agent`, headless cron). The table was permanently empty before because `add_execution_log` had no JS callers.
- **CI fix** — extracted `queryClient` from `main.tsx` to `lib/queryClient.ts` so non-React callers (the demo store) don't drag in `ReactDOM.createRoot(...)` at module load. Vitest's jsdom env was crashing on every CI run since v2.0.0-alpha.x.
- **Diagnostic on Pipelines empty state** + console-logged trace upload failures, so the next "panel says empty" mystery is a 30-second diagnosis instead of a 30-minute one.

### v3.0.0+ — Multi-Tenant + Compliance (Planned, exploratory)
- **Team workspaces** — shared agents, shared knowledge, shared trace history with per-member ACLs.
- **PII / safety scanning** — auto-flag conversations with sensitive data; redact-on-export.
- **Compliance bundles** — SOC2-ready audit log, retention controls, export-on-request, BYOK encryption.
- **Marketplace for agent templates** — community-submitted agent recipes; revenue share if the OSS marketplace earns from sponsored placements.
- **Agent versioning + rollback** — `git`-style history per agent, A/B routing, canary deploys.

### v4.0.0+ — Federated Agent Network (Speculative)
- **Agent-to-agent discovery protocol** — agents on different ATO installations can call each other via a registered handle (`acme/triage` → `acme/legal-review`). MCP-based, optional.
- **Cross-tenant audit / abuse defense** — when external agents call each other, who pays / who's responsible / how is provenance preserved.
- **Agent reputation system** — an agent's track record (success rate, eval scores, conversations served) becomes a portable signal across deployments.

### v5.0.0+ — Open Standards / Spin-out Layer (Speculative)
- **ATO becomes the reference implementation** for an open agent-deployment standard, similar to how `kubectl` is the reference for Kubernetes' API. Anyone can build a competing GUI / hosting provider that speaks the same agent spec.
- **Plugin SDK** — third parties (Cursor, Windsurf, Aider, etc.) implement the protocol so the same agent runs unchanged across runtimes.

### v1.7.0–1.8.0 — Polish (Planned, fits between v1.6 and v2.0)
- Cron-driven evaluator scheduling
- `mcp-call` variable / hook resolver (embedded MCP client)
- Trace-retention enforcement on cloud
- Search across persistent threads
- Mobile companion (read-only)
- Wizard runtime + agent runtime decoupling — pick MiniMax / Qwen / Grok / etc. as the *agent's* runtime while the wizard conversation stays on a CLI ([note in tier.ts policy](apps/desktop/src/lib/tier.ts))

---

## Future Runtime Support

As new AI coding agents emerge:
- Cursor
- Windsurf / Codeium
- Aider
- Continue.dev
- Custom agents via plugin API
