# ATO Roadmap

## Mission

**ATO is your local war room for humans and LLMs: decide together, call tools, and verify every outcome.** Drive it from a GUI, a CLI, or your coding agent over MCP — same data, same operations, same audit trail.

See [`README.md`](./README.md) for the full pitch and [`AGENTS.md`](./AGENTS.md) for the surface a coding agent reads.

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
- SDK (`@ato-sdk/js`) — narrow-scoped trace forwarder for ATO-authored agents deployed outside the desktop app (Cloudflare Worker / Vercel / Docker / Node bundles). **Not** a general-purpose LLM observability SDK; that's Langfuse / Helicone / LangSmith territory and we stay out of that lane (see `STRATEGY.md` in `ato-cloud`).
- Web dashboard, cost tracking
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

### v1.6.0 — Intelligence Layer (Automations canvas shipped May 2026)
- **Automations tab repurpose — group pipelines as flow nodes** *(shipped — multi-source aggregator + click-through to Insights)* ([detailed plan](docs/V1.6.0-AUTOMATIONS-REPURPOSE.md))
  - Runs → Automations now visualizes **everything that runs without a human in the loop**: routed groups, sequential pipelines, scheduled cron jobs, agent hooks, and skill flows — all on the same canvas. `automationsAggregator.ts` pulls from each source; `groupsToWorkflows`, `cronsToWorkflows`, `hooksToWorkflows` plus the original `skill-to-workflow` converter normalize them into a common shape.
  - Sequential groups render left-to-right with stage pills; routed groups fan out from the router; cron jobs anchor at the left edge with a clock icon; hooks attach as input nodes.
  - Live status decorated from `getAgentMetrics` so each node carries idle / running / succeeded / errored + last-run timestamp.
  - WorkflowToolbar dropdown filters by source ("Skills · Schedules · Pipelines · Routed Groups · Hooks · Manual") + by runtime.
  - **Click "View runs"** on any node → soft-handoff via localStorage to Insights → Agents, which expands that agent's row on mount.
  - Empty-state copy enumerates the four entry points (group / cron / hook / skill) instead of pointing only at Edit mode.
- Real-time collaborative workspace (WebSocket via ato-cloud) *(planned)*
- Team cursors (Figma-style) *(planned)*
- Cross-runtime policy enforcement templates *(planned)*
- Hosted terminal sessions for Team tier (cloud) *(planned)*
- Proactive suggestions ("Your project is missing X") *(planned)*
- Cost optimization alerts from SDK traces *(planned)*
- Agent performance benchmarking across runtimes *(planned)*
- **HALO integration** — feed traces from `~/.ato/agent-logs.jsonl` into Context Labs' HALO RLM engine (MIT, on PyPI), surface harness-improvement reports as one-click inline diffs *(planned)*

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
- **Cross-runtime regression detection** *(v1 shipped, deep version shipped v2.0.2)* — `/agent-traces/regressions` joins config changes × traces × evaluations. RegressionsPanel cards show eval-score delta column + "View N failing examples →" drill-down opening a modal of post-change failing traces with prompts/errors. AgentDetail surfaces a regression banner above the tab nav when its agent has an active regression. Severity widens to fire on ≥15pp eval drop even when ok-rate is unchanged.
- **Cost optimization recommendations** *(prescriptive layer shipped v2.0.3)* — `/agent-traces/cost-recommendations` surfaces same-agent swaps when historical multi-runtime data exists and the alt is ≥30% cheaper at ok-rate within 10pp + eval-score within 5pp. Rendered as a section above Insights → Usage benchmarks. Render-nothing when no recs in window so the panel stays clean.
- **Replay infrastructure (interactive)** *(shipped v2.1.0)* — TraceCompareModal gains a Replay button → picker for target runtime/model → re-dispatches the original prompt via `prompt_agent_inner` (so it's killable + appears in Live runs). Result panel polls `replay_jobs` table, renders source vs replay side-by-side with duration delta. Prompts come from local `execution_logs` (linked to cloud trace IDs by ±10s temporal correlation post-upload) so no new cloud retention obligations. Pre-dispatch disclosure surfaces data-residency intent on every replay. Multi-device replay deferred (the local-only constraint surfaces a clean "prompt not local" message when relevant).
- **Replay scheduling (cloud-side batch)** *(deferred to v2.1.x patch)* — server-side replay queue, encrypted credential vault, cost guardrails, batch-replay-N-failing-examples-from-this-regression UX. Plan locked in but each piece requires an explicit data-residency review pass before shipping prompts to cloud-side compute.
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

### v2.5.0 — Phase 7 cloud-relay (Released 2026-05-14)

The paid Pro/Team counterpart to the free LAN-only mesh (Phase 7.0, OSS). Same protocol; adds a cloud relay so two daemons behind different NATs can talk over the internet.

- **`services/mesh-relay/` (new service, port 3007)** in `ato-cloud` — WebSocket router with Ed25519 signatures preserved end-to-end between peers. Cloud is a dumb pipe — it cannot forge or read dispatches; it only proves "this connection belongs to a paying user."
- **`mesh_daemons` + `mesh_tokens` tables** (migration 017) — `mesh_tokens` SHA-256 hashed at rest. `peer_id` CHECK constraint enforces `^[0-9a-f]{64}$`.
- **REST endpoints** under `/api/mesh/daemons` (Pro-tier gated, JWT auth): register / list / revoke. Max 10 active daemons per user.
- **Gateway WS upgrade** on `/api/mesh/relay` forwards to the relay service; daemons authenticate with a long-lived `mst_*` bearer token.
- **Rate limit** 50 deliver-frames / 10s per source daemon; 64 KB payload cap; 90s idle timeout; self-loop refused.
- Threat model + design notes in `docs/PHASE-7-CLOUD-RELAY-DESIGN.md`; multi-LLM review transcript in `docs/reviews/phase7-cloud-relay-2026-05-14.md`.

Deferred to a later Phase 7 patch: offline queue, multi-instance relay (single-Railway today; horizontal scaling needs Redis or pg-LISTEN), and the OSS GUI for daemon registration.

### v2.5.1 — Insights health-panel cleanup + live_runs zombie reaper (Released 2026-05-14)

Four bugs Will surfaced in the Insights panel; all four about the panel reporting wrong things about runtimes.

- **`live_runs` zombie reaper** — `reap_dead_live_runs` in `active_runs.rs` probes each row's `child_pid` via POSIX `kill -0` on every `list_active_runs` call. Rows whose PID is dead get reaped after a 30s grace window. Fixes the "ad-hoc CLAUDE row stuck for 1h+ after a SIGKILLed `ato review`" symptom — SIGKILL bypasses `LiveRunGuard::drop`, so until v2.5.1 those rows sat forever.
- **Runtime detection (Claude / Codex / Gemini)** — `health_poller`'s checks now route through `which_cli` instead of bare `Command::new("claude")`. `which_cli` already honored the user's login-+-interactive shell PATH, so NVM-managed installs (`~/.nvm/versions/node/*/bin/`) now resolve and the cards flip green.
- **"Not installed" ≠ "Down"** — Hermes (never installed) was rendering red "Down." Error messages now use "not installed on this machine" wording, which `HealthDashboard.effectiveStatus()` already maps to the neutral grey "Not configured" pill.
- **Monitored-runtimes preference** — new `runtime_preferences` SQLite table (`runtime`, `monitored`, `updated_at`). New Tauri commands `list_runtime_preferences` + `set_runtime_monitored`. First-launch seed via `which_cli` so a fresh install only monitors detected runtimes. Health poller + `get_health_status` both filter on the toggle, so un-monitored runtimes never show up. New Settings → Runtimes → Monitoring sub-tab with per-runtime toggles.

Multi-LLM review transcript + audit decisions in `docs/reviews/v2.5.1-health-panel-2026-05-14.md`.

### Path to 85+ on all five elegance fronts (war-roomed 2026-05-19)

Honest audit after v2.7.6 dogfood pass: the "85%+ across all 5 fronts" framing in earlier release notes was aspirational, not measured. Real scores: TS gate ~95, DB schema ~85, Backend org ~70 (`commands/mod.rs` still 9,133 lines + 3 other Rust files over 1,500), Frontend org ~65 (`PromptBar/index.tsx` 1,501 lines), Surface ~55 (7 UX bugs caught in one dogfood session — chevron-hidden launcher, FirstChatWizard not globally mounted, SessionsList pending-flag subscription wrong, NewSessionModal hidden behind detail view, line-through pills, subtab routing bug, 0-msg ghost rows). Weighted average ~70%.

War-room id `1DF02DA9-125E-4A98-B78D-083BA605A80B` (claude + codex; gemini skipped — keychain rotation cliff locked the API key) ordered the work to get every front honestly above 85.

**v2.7.7 — frontend seam + write-path discipline**
- Bundle: extract `PromptBar/InputRow.tsx` + collapse 4 picker booleans (`showRuntimePicker` / `showAgentPicker` / `showThreadPicker` / `showRoomTypePicker`) into `openPicker: "runtime"|"agent"|"thread"|"roomType"|null` discriminated union. Closes latent backdrop-stacking bug (multiple `fixed inset-0 z-30` overlays open simultaneously catch the wrong close click). Frontend 65 → 80.
- Shared `useQuery({queryKey:["enabled-runtimes"]})` between `PromptBar` + `FirstChatWizard`. Kills duplicated `queryAllAgentStatuses` + `listLlmApiKeys` subscriptions. Frontend 80 → 83.
- Split `sessions_view.rs` (1,635 lines) before lazy row creation lands on top of it. Backend 70 → 75.

**v2.7.8 — surface fix + the backend elephant**
- Lazy row creation at write points: don't write `chat_threads` on focus, don't write `sessions` pre-first-turn, don't write war-room row pre-dispatch. Replaces v2.7.6 list-side filter band-aid. Surface 55 → 70.
- **Mandatory pre-tag dogfood pass.** Tauri-webdriver script encoding the 7-step golden path (cold launch → FirstChatWizard from Home → FirstChatWizard from PromptBar → session-without-turn → war-room-and-return → toggle runtime readiness with wizard+PromptBar both open → assert no ghost rows, no hidden modals, no dead affordances). Wired into pre-push hook for `v*.*.*` tag commits only. Both reviewers picked this over snapshot diffs / component error boundaries / vitest expansion. Surface 70 → 85.
- `commands/mod.rs` PR 28 — extract `agents.rs` (~50 commands; "the elephant"). Drops `mod.rs` to ~5,000 lines. Backend 75 → 81.
- **Gemini CLI agentic-flag pass-through** (need-to-have, blocked on gemini CLI install). Mirror the codex `--sandbox workspace-write` + `approval_policy=never` unlock to the gemini branches in `apps/desktop/src-tauri/src/commands/mod.rs` and `apps/cli/src/commands/dispatch.rs`. Gemini CLI defaults to on-request approval — same headless-hang failure mode codex had before commits `72aff8b` + `a440f96`. 5-minute change once the binary is on PATH; documented here so it doesn't drop.
- **CLI runtime → API provider auto-fallback** (need-to-have, Will dogfood 2026-05-19). When a user dispatches `gemini` (or `claude`) and the CLI binary isn't installed, but a matching API key IS configured (`google` for gemini, `anthropic` for claude), the backend should silently route through `crate::api_dispatch::dispatch()` instead of erroring with "CLI not found." The mapping is already in `apps/cli/src/byok.rs:34-46` (`claude → ("ANTHROPIC_API_KEY", "anthropic")`, `gemini → ("GEMINI_API_KEY", "google")`). Today's interim fix is a better error message that points the user at the existing `google`/`anthropic` picker option, but the auto-fallback removes the dead-end entirely. Scope: refactor `prompt_agent_inner` in `mod.rs` to check CLI availability first, route to api_dispatch if missing+key-present, mirror the same execution_logs / streaming bookkeeping (~80 LOC of cross-module surgery). Codex needs OpenAI added to `packages/ato-api-providers` before its fallback can work (queued under v2.8.0 API-provider tool-call loop).
- **Agent-permission plumb-through audit + wiring** (need-to-have; the most credibility-load-bearing item on this list). CreateAgentWizard surfaces `spec.permissions.{allowed, requireApproval, denied, summary}` to the user — promises the agent will be allowed certain actions, require approval for others, be denied the rest. **Currently those promises aren't fully wired through to the runtime-dispatch surface.** Today's codex sandbox unlock (`workspace-write` + `approval_policy=never` in commits `72aff8b` + `a440f96`) is uniform across every codex dispatch — it doesn't read the agent's `permissions` spec and translate `denied:["Bash(rm:*)"]` into a per-call sandbox restriction, nor map `requireApproval:["Bash(git push:*)"]` into Claude Code's `--allowedTools` minus that pattern. Same gap likely for every other permission concept the UI exposes. **Plan needed**, not a single PR:
  1. **Audit pass:** read every permission-shaped field in `apps/desktop/src/components/CreateAgentWizard/{GuidedPath,QuickPath}.tsx` + the persisted `spec.permissions` shape in `~/.claude/agents/<slug>.md`, `~/.codex/agents/<slug>/`, `~/.gemini/agents/<slug>.yaml`, etc. (per CLAUDE.md's file-writing contract). Write up "what the wizard says vs. what dispatch actually does today" — file: `docs/audits/agent-permissions-plumb-through-2026-05-19.md`.
  2. **Translation layer:** for each runtime, design the mapping from ATO's permission DSL (allowed / requireApproval / denied) to that runtime's native gate:
     - Claude Code: `--allowedTools` allowlist + `~/.claude/settings.local.json` `permissions.allow` / `deny`
     - Codex: `--sandbox <mode>` (`read-only` | `workspace-write` | `danger-full-access`) + `-c approval_policy=<mode>` + `-c sandbox_permissions=[...]`
     - Gemini: `--yolo` toggle + future per-tool flags
     - OpenClaw / Hermes: they enforce their own; pass-through metadata only
     - API providers: gates the tool-call loop (see v2.8.0 item below) — `denied` patterns refuse the model's tool_call before execution
  3. **Dispatch path:** every spawn site in `apps/desktop/src-tauri/src/commands/mod.rs` and `apps/cli/src/commands/dispatch.rs` reads the agent's persisted permissions, computes the runtime-specific flag bundle, and passes it. Today only 4 codex paths got the uniform unlock; the agent-aware version must replace those + cover claude / gemini / openclaw / hermes too.
  4. **UI feedback:** when the wizard shows `denied: ["Bash(rm:*)"]`, the run-detail view should show "rm command blocked by agent policy" if the model attempts it. Closes the loop so users see the promise being enforced.

  Without this, ATO is selling permissions it can't enforce — "the dispatch IS the authorization" only works for users who never look at the agent-creation UI's promises.

**v2.8.0 — backend file surgery + keychain durability + tool-call loop**
- Split `lib.rs` (2,370 lines) — schema init / Tauri command registration / structs. Backend 81 → 85.
- Split `recipes_engine.rs` (2,245 lines) — engine / triggers / actions. Backend 85 → 88.
- Versioned master-key + identity-change detection. Today's keychain rotation cliff (memory `feedback_dev_build_keychain.md`) silently orphans every stored API key when the macOS keychain ACL identity changes. Real users hit this on signing-cert rollover, macOS version upgrades, keychain resets. Ship `master_key_v2` with re-encryption transaction on rotation instead of orphaning ciphertext; prompt-to-re-enter on identity change instead of silent rotate. Surface 85 → 87.
- `anchor_runtime` column on `chat_threads` → ships the WhatsApp-row LLM-icon column the v2.7.6 truncation war-room shipped without. Surface 87 → 88.
- **API-provider tool-call loop** (need-to-have for the multi-LLM arbitration pitch to be honest). Today the 7 API providers in `packages/ato-api-providers/` (minimax, grok, deepseek, qwen, openrouter, anthropic, google) dispatch as `POST /v1/chat/completions` — pure text-in, text-out. They have **zero tool access**, can't read files, can't grep the codebase, can't make a decision about your code. When you war-room with codex + claude + minimax, the API-provider seats reason about whatever's in the prompt context; only the CLI agents (codex / claude / openclaw / hermes) actually investigate. That's a credibility hole: ATO sells "compare every LLM on your task" but half the seats are reasoning blind. Fix: implement OpenAI/Anthropic function-call loop in `apps/cli/src/api_dispatch.rs` — parse `tool_calls` in the response, execute the requested tool locally (file read, grep, write, shell — same workspace-write boundary as codex), append `tool` role messages with the results, re-dispatch until the model emits a final answer with no more tool calls. Per-provider testing matrix because each provider's function-call shape is slightly different (OpenAI vs Anthropic vs Gemini all diverge). Effort estimate: ~200-400 LOC + 7 provider acceptance tests. Surface ?? (lifts the "honest agentic multi-LLM" claim from aspirational to real).

**Projected scores after v2.8.0 lands:** TS 96, DB schema 87, Backend org 88, Frontend org 83, Surface 87. Weighted average ~89%. Frontend may need a second pass (`PromptBar/_helpers.ts` audit + SessionsList second cut) to clear 85.

**Dropped from milestone gating** (do as housekeeping, not release-blockers): `cron.rs` unused-fn warnings (`cron_to_schtasks_xml_trigger`, `build_schtasks_xml`) → gate by `#[cfg(target_os="windows")]` opportunistically; untracked artifacts (`yc-session.md`, `codeelegancesession.txt`, two unused `Cargo.lock` files in `packages/ato-{posts,recipes}/`).

**Process change:** release notes claim per-front percentages only when there's a linked measurement (file LOC delta, bug count, test pass count). Stops the "85%+ across all 5" language from leaking into release notes without numbers backing it.

War-room transcripts: `docs/reviews/elegance-roadmap-war-room-2026-05-19.md` (forthcoming write-up of both seats' answers).

### v2.7.6 — Elegance day part II: TS cliff cleared + 5 fronts at 85%+ (Released 2026-05-19)

Continuation of the 2026-05-18 elegance arc. Single goal: clear the 151-error TypeScript debt cliff (hidden behind a `noEmit:true + composite:true` tsconfig misconfig) and push all 5 elegance fronts to 85%+ in one day.

**TS gate (151 → 0):**
- Dropped `noEmit:true` from `apps/desktop/tsconfig.node.json` (incompatible with `composite:true`, was suppressing every real error).
- Added `apps/desktop/src/vite-env.d.ts` with `/// <reference types="vite/client" />` — wiped 18 `import.meta.env` errors at the root.
- Added `"types": ["vitest/globals", "vite/client"]` to `apps/desktop/tsconfig.json`.
- Collapsed the stale `AgentRuntime` literal-union in `lib/agents.ts` to a re-export of `RuntimeId` from the single runtime registry.
- 7 root-cause attacks cleared 95+ errors before per-file tail. Final: `tsc --noEmit` rc=0.

**Backend foundation — commands.rs split (PRs 22-27e shipped today):**
- PR 22 `execution_logs.rs` (2 cmds) — core CRUD with v2.3.41 columns.
- PR 26 `cron.rs` (10 cmds + 13 launchd tests) — OS scheduler glue.
- PR 27a `skills_validate.rs` (2 cmds) — skills validation surface.
- PR 27b `skills.rs` (3 cmds) — skills read surface.
- PR 27c `skills_mutate.rs` (6 cmds + version snapshots) — create/delete/update/restore.
- PR 27d `mcp.rs` (5 cmds) — MCP discovery + config.
- PR 27e `mcp_install.rs` (2 cmds + 5 tests) — MCP install/uninstall.
- `commands/mod.rs`: **17,270 → ~12,400 lines** (-28% from baseline, -19% today). 1 PR remains (`agents.rs` — the elephant, ~50 cmds).

**Frontend foundation — card-variant splits:**
- `PromptBar.tsx` (1722 → 1498 lines, -13%) — extracted `_helpers.ts` (RUNTIME_META, simulateMock, messagesToAgentHistory) + `ChatRow.tsx`. Renamed to `PromptBar/index.tsx`.
- `SessionsList.tsx` (1379 → 763 lines, -45%) — extracted 4 card variants (`ChatCard`, `WarRoomCard`, `SingleRunCard`, `SessionCard`) into `SessionsList/SessionCards/`. `SessionListRow` interface lifted to `_helpers.ts` for shared access.

**DB schema — active_dispatches view + dispatch_kind filtering (war-roomed):**
- 2026-05-19 war-room (`claude` + `codex`) on whether to split `execution_logs` into separate `active_dispatches` / `passive_observations` tables. Divergent verdict (claude no-split, codex split). CTO synthesis: ship Option 1 NOW, defer split until passive observation rows exceed ~10× active.
- Shipped: `CREATE VIEW IF NOT EXISTS active_dispatches AS SELECT * FROM execution_logs WHERE dispatch_kind = 'active'`.
- Added `dispatch_kind = 'active'` filter to 11 read paths (analytics, execution_logs list, sessions_view single-run synthesis, local_insights). `compute_billing_surface_summary` intentionally left unfiltered as the cross-kind reader.
- `packages/ato-db-views/src/lib.rs` — `v_recent_dispatches` + `v_cost_by_agent_runtime` now filter `dispatch_kind = 'active'`.
- Full transcripts in `docs/reviews/execution-logs-war-room-2026-05-19.md` and `docs/reviews/path-b-stage-2-war-room-2026-05-19.md`.

**Test gate green:**
- 170 Rust tests pass (51 CLI + 103 desktop + 5 db-views + 5 api-providers + 4 pricing + 2 posts). `ato-api-providers` test fixed — registry grew to 7 providers (added `anthropic`) but exact-list invariant assert hadn't been updated.
- 20/20 vitest tests pass.
- 19/20 `ato` CLI commands return well-formed JSON (`config-changes list` needs `--agent`, expected).

**Bottom-pane multi-launcher bugfixes (caught while dogfooding):**
- `FirstChatWizard` was mounted only in `Home.tsx`. Clicking "War room" from the bottom-pane chevron while on any other section flipped `firstChatOpen=true` in the Zustand store with no listener — modal never appeared. Moved the mount to `Dashboard.tsx` (alongside `CreateAgentWizard`) so it's available from every section.
- `SessionsList` consumed the `pendingOpenNewSession` flag via a `useEffect` whose deps were the stable Zustand consume *functions*. Effect ran once at mount; clicking "Multi-turn session" from the chevron while already on Sessions tab silently set the flag with no observer. Switched deps to the pending *values* (`pendingOpenSessionId`, `pendingOpenSessionKind`, `pendingOpenNewSession`) so the effect re-runs when the flags flip.

**War-room modal UX (multi-LLM reviewed):**
- 2026-05-19 war-room `F009D1D3-…E1C9` (claude + codex; gemini CLI not installed, codex substituted) unanimous on three fixes — applied same session:
  1. **Toggleable pills** in `FirstChatWizard` so users can deselect any of the detected runtimes. Tracks `excluded: Set<string>` (sticky across runtime health flaps) instead of `selected`; `selected` derives from `enabled \ excluded` via `useMemo`. Pills use filled-vs-outlined metaphor (no line-through).
  2. **Soft "+ add another"**: instead of dumping the user in Settings, an inline explainer panel surfaces two paths ("Add API key" → `Settings → API Keys`; "Set up CLI subscription" → `Settings → Runtimes`). Subtab routing always owns the `setSubTab` write (codex caught: it was inside the else branch, getting skipped when the parent passed `onOpenSettings`).
  3. **LlmApiKeys subscription banner**: one-line explainer + "Open Runtimes" button so users who arrive from the "+ add another" CLI-subscription flow find the right surface.

**Deferred to v2.7.7:**
- Bottom-pane inline room-type picker (segmented control in the input row, replacing the chevron-hidden launcher). War-room synthesis: ship segmented control + drop the wizard modal entirely; one-time coachmark on first war-room selection. Bigger UX slice — needs its own scoped PR.
- `NewSessionModal` participant picker (currently only the coordinator). Reviewer consensus: use the same toggleable-pill widget as the war-room modal for "Invite other LLMs." Auto-bridge `@<runtime>` mentions in the continue-message input.

### v2.7.5 — Consolidation arc + elegance day (Released 2026-05-18)

The day was a single-themed push: drive every layer of the codebase to **85-90% elegance** across surface, frontend organization, backend organization, runtime type system, and database schema. Driven by Will's observation that surface polish had outpaced the foundation, and that the foundation needed to catch up before more features land.

**User-visible arc — one inbox, one launcher:**
- **First-Chat Wizard** (PR-C). Home CTA "Start a war room" → single-screen modal with silent runtime detection + prompt + Send. Replaces the previous CreateAgentWizard launch on that CTA.
- **Path A — chat threads UNION into Sessions feed.** `list_sessions_full` now reads chat threads alongside sessions / single-runs / war-rooms. One inbox. New 🗨 chat card + read-only `ChatThreadDetailView`. No schema migration.
- **Path B — bottom-pane multi-launcher.** PromptBar's "+ New conversation" becomes a 3-option dropdown: Quick chat (stays in pane) / Multi-turn session (Sessions tab + NewSessionModal) / War room (FirstChatWizard).
- **Copy normalization (×2 rounds).** "war-room" → "war room" hyphenation across en/pt/es i18n + components. Sessions tab description rewrite. PromptBar input placeholder now dynamic: `"Ask {{runtime}} anything…"` instead of hardcoded "Claude."

**Frontend foundation:**
- **Single runtime registry** (`apps/desktop/src/lib/runtimes.ts`). Replaces 10× in-component `RUNTIME_COLORS` duplicates + a stale 4-entry `AgentRuntime` type + a stale PromptBar `RUNTIME_OPTIONS` picker that silently dropped 6 of 10 runtimes. New `RUNTIME_REGISTRY` is the canonical map; helpers (`runtimeBadge`/`runtimeHex`/`runtimeLabel`/`runtimeIcon`) provide safe fallback for legacy values. Adding a runtime is now one entry.
- **SessionsList.tsx split.** 2493 → 1379 lines (-44%) by extracting `SessionTranscriptView.tsx` (884 lines) and `NewSessionModal.tsx` (168 lines). Shared types/helpers (`SessionTranscript`, `runtimeDisplay`, `inferCoordinatorTarget`, `NEW_SESSION_RUNTIMES`) consolidated into `_helpers.ts`.

**Backend foundation — commands.rs split (12 of 24 PRs shipped):**
- PR 2 `models.rs` (4 cmds), PR 3 `usage_billing.rs` (4), PR 4 `knowledge.rs` (4 + full RAG pipeline), PR 5 `posts.rs` (5), PR 6 `analytics.rs` (4), PR 7 `files_paths.rs` (3), PR 8 `onboarding.rs` (1+structs), PR 11 `context.rs` (5), PR 12 `workflows.rs` (5), PR 13 `workflow_webhooks.rs` (7), PR 14 `notifications.rs` (6), PR 15 `chat_threads.rs` (8).
- `apps/desktop/src-tauri/src/commands/mod.rs`: **17,270 → 14,012 lines** (-19%). 12 PRs remain.
- PR 9 (`security_policies`) + PR 10 (`external_deploy`) deferred — both depend on cross-cutting helpers (`file_ref` 30 callsites, `collect_skills_for_project` 18 callsites, `parse_*` parsers) that need to migrate to their natural domains first. Revisit after the larger domain extractions land.

**Held for war-room consultation (do NOT touch unilaterally):**
- **Path B Stage 2** — `chat_threads` → `sessions` storage unification (schema + backfill + PromptBar refactor).
- **`execution_logs` audit** — that table gained 5 columns this month with 3 more coming in v2.6 PR-A; worth war-rooming whether it splits before more columns land.

Build matrix green throughout — `cargo check rc=0`, `vite build rc=0`, `vitest 20/20` after every commit. Full progress + status in `docs/CONTINUATION_PLAN.md` § "Elegance day — 2026-05-18".

### v2.6 — Universal multi-LLM observation tier (Planned, next milestone)

Passive observation of native CLI sessions (Claude Code, Codex CLI, etc.) plus billing-surface tagging on every dispatch — under the war-room mission, this is the layer that lets you see what every LLM ran on this machine, not just what ATO dispatched. Plan locked 2026-05-14; full doc at `/Users/beatriznigri/.claude/plans/peaceful-strolling-kay.md`.

Three tiers of observation, plus an honest Tier 4 callout:

- **Tier 1 (PR-A, next ship)** — local watcher for terminal LLM CLIs. `execution_logs` gains `dispatch_kind` (`active` vs `passive_observation`), `billing_surface` (`claude_code_subscription` / `anthropic_api` / etc.), and `provider_session_id` (dedup key). New `passive_observer.rs` Rust module mirrors `log_watcher.rs`; parses Claude Code's `~/.claude/projects/<slug>/<uuid>.jsonl` and Codex CLI's `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. Insights → Live gets a billing-surface chip + Source filter; Insights → Usage gets a group-by-billing-surface toggle + a "Last 7 days at a glance" header card.
- **Tier 2 (PR-C, opt-in, Pro+, Team-admin gated)** — local mitmproxy-style traffic capture for power users / orgs. Bodies stored encrypted at rest under the existing AES-256-GCM-under-keychain scheme. Default off. Team-admin policy flag can require it on company PCs. Phase-2 cloud sync uses customer-managed keys.
- **Tier 3 (PR-B)** — cloud-side polling of provider usage APIs (OpenAI `/v1/usage`, Anthropic org reports, Gemini billing, MiniMax, OpenRouter, DeepSeek, Groq, Together) using user-stored keys. New `provider_keys` table in ato-cloud, encrypted at rest. New `services/usage-poller/` on port 3008 (introduces `node-cron` to ato-cloud). Daily 03:00 UTC poll; results in a new `provider_usage` table aggregated by `(user, provider, period)`. New analytics endpoints `/api/analytics/provider-usage[/timeline]`. Desktop merges local-watched + cloud-polled data into the same Usage tab, deduped by `(provider, period)`.
- **Tier 4 — out of scope** (phone apps, claude.ai web on consumer plans). Surfaced honestly in the Usage tab as a "blind spot" line. The candor is the differentiator vs competitors that silently undercount.

PR-A and PR-B each ship with the standard multi-LLM `ato review --consensus` round pre-merge. PR-C ships with an extended threat-model round because TLS interception is its own risk class.

### v2.3.0 — Agent-driveable platform (Active, next 60 days)

**Goal:** ATO becomes operable end-to-end by the developer's coding agent. Same data, same operations, same audit trail, accessible from three actor surfaces: GUI (humans), CLI (`ato <command>` shelling out, primary agent surface), MCP (stdio, in-harness agents). Pairs with the platform amendment in `ato-cloud/docs/STRATEGY.md` (2026-05-11).

Load-bearing pieces:

- **`ato` CLI binary** — separate from `ato-desktop`; pure Rust; talks to the local SQLite DB directly. Subcommand structure: `dispatches`, `runs`, `regressions`, `cost`, `replay`, `compare`, `skills`, `agents`, `recipes`, `events`. JSON output by default, `--human` flag for readable formatting. Documented in `AGENTS.md` at the repo root.
- **Expanded MCP server** — *target hit, v2.3.35: 50 tools across Observation / Operations / Authoring / Sessions / Posts / Runtimes / Events.* Stdio transport stays. Each new tool shells out to the `ato` CLI rather than re-implementing SQLite queries in TS.
- **`AGENTS.md` doc** *(shipped 2026-05-11)* — canonical agent-facing manual covering CLI commands, MCP tools, file paths, event subscriptions, common recipes, safety notes. The doc a coding agent reads to learn ATO.
- **Local-mode for regressions and cost recs** — *shipped v2.3.2.* `compute_regressions_local` + `compute_cost_recommendations_local` Tauri commands run over local `execution_logs` + `agent_config_changes`; `ato regressions list` + `ato cost recommendations` mirror on the CLI. The Insights panels prefer cloud when the user is signed-in Pro, fall back to local otherwise (and on cloud errors).
- **Ops recipes (programmable trigger→action workflows)** — extends the Automations canvas with event-trigger node types (`on regression`, `on dispatch_failed`, `on cost_threshold`, `on replay_done`, `on schedule`) and ops-action node types (`draft skill`, `replay on alt runtime`, `kill run`, `post to webhook`, `notify human in activity feed`). Skillify ships as one example recipe template, not a hardcoded feature.
- **Activity feed** — chronological view in the GUI where humans and agents both post. Where shared insights between human and agent surface.
- **Event subscription protocol** — `ato events watch --type <event>` streams JSON events one per line so agents can stay long-lived and react to what happens.

### v3.0.0+ / v4.0.0+ / v5.0.0+ — Blue-sky

Items that previously sat in this section (federated agent network, kubectl-for-agents, compliance bundles, marketplace for agent templates, etc.) have been moved to [`BLUE-SKY.md`](BLUE-SKY.md). They don't currently fit the mission stated at the top of this file. They live in the blue-sky doc so engineering decisions don't drift into them by accident.

### v1.7.0–1.8.0 — Polish (Planned, fits between v1.6 and v2.0)
- Cron-driven evaluator scheduling
- `mcp-call` variable / hook resolver (embedded MCP client)
- Trace-retention enforcement on cloud
- Search across persistent threads
- Mobile companion (read-only)
- Wizard runtime + agent runtime decoupling — pick MiniMax / Qwen / Grok / etc. as the *agent's* runtime while the wizard conversation stays on a CLI ([note in tier.ts policy](apps/desktop/src/lib/tier.ts))

---

## Phase 6.x — CLI dispatches visible in Live Runs (Planned)

Today, the **Live** tab in Runs only shows GUI-driven dispatches.
The reason: `active_runs` is an in-memory map inside the desktop
process; CLI runs (`ato dispatch ...`) execute in a separate
process and can't write to that map.

After Phase 4.3 we have the `events_log` cross-process channel.
The fix: CLI publishes `dispatch_started` / `dispatch_finished`
events on every dispatch; a new desktop watcher mirrors them into
`active_runs::begin_run` / `finish_run`. Killing remains tricky
across processes (active_runs holds the actual process handle) —
v1 makes CLI runs visible-but-unkillable; v2 adds PID-tracking
for cross-process kill.

Triggered by Will noticing during the v2.3.21 MiniMax benchmark
that ATO's own review dispatches via `ato dispatch minimax` were
invisible to the Live tab while they were running.

## Phase 6.x-I — Runtime-binary health check (CLI shipped v2.3.34)

When ATO tries to spawn a runtime CLI whose Developer ID cert has
been revoked (or which is unsigned / quarantined), macOS pops a
generic malware dialog and silently kills the parent app. The user
sees ATO crash and a confusing "codex contains malware" message,
with no actionable path back.

### Surface

At startup, after `detect_agent_runtimes` finds a CLI, run
`spctl -a -vv <path>` and parse the result. For each rejected
runtime, surface an in-app banner pinned to the top of Home /
Settings → Runtimes with:
- Specific reason (`CSSMERR_TP_CERT_REVOKED`, `no usable
  signature`, `quarantine`)
- The exact fix command (`npm install -g @openai/codex@latest`,
  `xattr -d com.apple.quarantine ...`)
- A "Run fix" button that executes it via the sidecar shell

Triggered today (2026-05-12) when Will's codex install hit a
cert-revoked block mid-session. ~50 LOC; would have replaced
30 minutes of diagnostic back-and-forth with a one-click resolution.

**Shipped (v2.3.34, CLI piece):**
- `ato runtimes health` runs `codesign --verify --verbose=2` and
  reads `com.apple.quarantine` xattr for each detected runtime. Per
  row: `runtime, binary_path, status, detail, fix_command`. Status
  values: `ok / missing / revoked / quarantined / unsigned / unknown`.
- Canned fix commands for `revoked`/`missing`: `npm install -g
  <pkg>@latest` per known runtime; for `quarantined`: `xattr -d
  com.apple.quarantine <path>`.
- 4 unit tests on the parser + install-map.

**Shipped (Phase 6.x-I.2, v2.3.36):**
- Desktop banner pinned to Home above the "Connect a runtime" prompt.
  Renders only when at least one row has status `revoked` /
  `quarantined` / `unsigned` / `unknown`. Auto-refetches every 5min.
- One-click "Run fix" button — `runtime_health_run_fix` Tauri command
  re-parses the fix string against an allowlist (only `npm install -g
  <pkg>@latest` and `xattr -d com.apple.quarantine <path>` shapes
  pass) and executes via Command::new with split args. No `sh -c` of
  untrusted strings.

**Still open:**
- Walk through JS-shim CLIs (like the npm `codex`) to verify their
  bundled Mach-O sidecars, not just the shim itself. The shim is
  unsigned but benign; the underlying binary is what gets revoked.

## Phase 6.x — Runtime quota visibility (Planned, small)

Rate-limit info is only visible when you try a dispatch and it fails.
ATO already sees these errors (they flow through `ato dispatch`'s
stderr) but doesn't persist or surface them proactively. Caught
during the v2.3.19 commit when codex hit its limit mid-review.

### Surface

- New table `runtime_quotas (runtime TEXT, resets_at TEXT, source
  TEXT, captured_at TEXT)`.
- Dispatch error path: regex `try again at <timestamp>` patterns
  and upsert into runtime_quotas.
- `ato runtimes status` (extension to existing): per runtime,
  show "ok" or "rate-limited until <ts>".
- `ato dispatch <runtime>` pre-flight: if runtime_quotas has a
  future resets_at, return the saved reset time without burning
  another dispatch attempt.

~50 LOC total. Worth shipping standalone or alongside Phase 6
sessions.

## Phase 6.x-F — API provider streaming (Shipped v2.3.47)

Non-CLI providers (MiniMax, Grok, DeepSeek, Qwen, OpenRouter) were
buffering 7–15s of output before showing anything. Streaming closes
that UX gap by emitting SSE chunks to stdout as they arrive.

### Surface

- `ato dispatch <provider> "<prompt>" --stream` — sets `stream: true`
  on the request, parses the SSE stream chunk-by-chunk, writes each
  `choices[0].delta.content` to stdout (with flush) the moment it
  lands. Tokens-in / tokens-out captured from the final `usage`
  chunk and persisted into `execution_logs` exactly like a buffered
  dispatch — no separate audit code path.
- Bridge / MCP / replay paths use the buffered path; streaming is
  opt-in via the user-facing flag only.
- Provider compatibility: standard OpenAI-shape works natively
  (Grok / DeepSeek / Qwen / OpenRouter). MiniMax also supports
  `stream=true`; we check each chunk's `base_resp.status_code` and
  surface in-stream failures (model-not-supported, etc.) the same
  way the buffered path does.

### What's NOT in v1

- CLI runtimes (claude / codex / gemini / hermes / openclaw) ignore
  `--stream`. They already stream to their own pipe; capturing that
  for ATO's live tail is a deeper change (per-runtime stdout parser).
- Tauri event emission for the desktop UI to render chunks live —
  the chat pane currently waits for the full response. Adding
  streaming to the GUI is a separate slice that wires the same
  callback into Tauri events.

## Phase 6.x-K — Eval-score ratchet (Shipped v2.3.39)

Inspired by Garry Tan's *AI Agent Complexity Ratchet* (May 2026):
the idea that AI coding agents make 90% test coverage free, and the
ratchet of test + doc + eval threshold means quality only goes up.
ATO's eval-score ratchet brings the same primitive to agent ops:
lock a quality floor per target, and `ato ratchet check` fails CI
whenever recent activity dips below it.

### Surface

- `ato ratchet lock --target <agent:slug | runtime:name | global>
   [--days 30] [--threshold 0.05] [--notes "..."]` — computes the
   target's success rate over the last `days` and persists it as a
   floor. Fails fast when there's no data to baseline against.
- `ato ratchet check [--target ...] [--window-days 7]` — for each
  lock, computes the recent window's success rate, compares to
  `floor - threshold`. Exit 1 when any target breaches; exit 0 when
  all pass. Drop into CI as a deploy gate.
- `ato ratchet status [--target ...]` — same shape as `check` but
  always exits 0 (informational, for humans).
- `ato ratchet list` / `ato ratchet unlock --target ...`.
- MCP: `ratchet_check` + `ratchet_list` tools for MCP-only harnesses.

### Metric for v1

`success_rate` from `execution_logs.status`. Coarse but universally
available locally — no cloud sign-in, no separate evaluator needed.
The schema's `metric` discriminator column means adding `eval_score`
(when cloud evals land locally, or when users opt into a local LLM-
judge) is additive: same table, same query path, new code path
behind the metric branch.

### Why this fits ATO's wedge

Tan's framework is general SWE wisdom; the *AI-agent-specific* part
is the closed loop "agent runs → evaluator scores → result locks
the floor for the next agent run." That loop lives at the workflow-
ops layer, which is ATO's exact wedge. Tests-as-coverage and TTY
harnesses don't fit; eval-score ratcheting does.

## Phase 6.x-J — SSH-backed remote runtime adapter (Planned, small)

Triggered by @iamknownasfesal on X (2026-05-11): *"how can i make my
claude agent that is on my computer vs that is on my server talk with
each other? atm just copying responses into each other lol"*

ATO already has the SSH primitive (OpenClaw runtime uses key-based
auth over SSH). Generalize it so any registered runtime can target a
remote host and answers route back through the same dispatch path
that powers Live Runs / activity feed / sessions.

### Surface

- `ato runtimes add-remote --name <label> --host user@server
  --runtime claude --binary-path /usr/local/bin/claude` — registers
  a remote endpoint with a local slug (e.g. `claude-server`).
- `ato dispatch claude-server "..."` — routes to the remote via
  SSH, captures stdout/stderr/exit status, persists to
  execution_logs as if it ran locally.
- Sessions (Phase 6) work transparently: a session bound to
  `claude-server` keeps `--resume` on the remote machine; the
  history mirror still lives in the local SQLite so cross-runtime
  history replay works between local and remote runtimes.
- Failure modes: SSH connect timeout, auth failure, remote binary
  missing → all surface as dispatch error rows, not crashes.

### What it does NOT do

This is the *fast* shape — one-way invocation from the laptop to
the server. The remote ATO daemon doesn't need to exist yet. The
server is a dumb runtime executor; ATO on the laptop owns sessions,
logs, and the bridging logic. If the user wants the server-side
agent to initiate work back at the laptop, that's the bi-directional
mesh in Phase 7+.

~150 LOC: lift `services/ssh-openclaw.rs`'s exec path into a generic
`remote_runtime::exec(host, key_path, cmd, args)`; add a
`remote_runtimes` table (slug PK, host, key_path, runtime, binary_path);
extend the runtime registry to look up remote slugs and route
through the SSH executor instead of `Command::new`.

## Phase 7 — Bi-directional ATO daemon mesh (7.0 + 7.1 shipped; 7.2+ planned)

**Status**: 7.0 (LAN) + 7.1 (cloud relay) shipped 2026-05-14. Full plan in [`PHASE-7-PLAN.md`](PHASE-7-PLAN.md).

Packaging decision (locked):

- **Phase 7.0 — free, LAN-only** *(shipped)*: mDNS discovery + invite-code pairing on the same network. Server-side ATO daemon can post completion notifications to the laptop's daemon over WebSocket + JSON-RPC with Ed25519-signed messages. Narrow `post_completion(session_id, status, payload)` surface that closes the @iamknownasfesal "server finish → agent pc" gap.
- **Phase 7.1 — Pro / Team tier on ato-cloud** *(shipped 2026-05-14, ato-cloud v2.5.0)*: cloud relay WebSocket router on `wss://api.agentictool.ai/api/mesh/relay`. Daemons authenticate with long-lived `mst_*` mesh-tokens; cloud is a dumb pipe (Ed25519 signatures preserved end-to-end between peers). Pro-tier gated. Max 10 daemons per user. See ato-cloud's `services/mesh-relay/` + `docs/PHASE-7-CLOUD-RELAY-DESIGN.md`.
- **Phase 7.2+ — full bi-directional dispatch + per-peer ACLs** *(planned)*: today's relay only forwards `post_completion`. The expansion lets a paired peer ask the other to run any allowed runtime, with per-peer scopes (e.g. server can call `claude` but not read `secrets`). Multi-machine session topologies. Needs OSS GUI for daemon registration + an extended threat-model review round.

The packaging matters: free users get a real working LAN mesh, not a teaser. The Pro upgrade is "stop fighting your firewall + unlock the full mesh." Aligns with the existing free-desktop / paid-cloud ladder.



The full version of Phase 6.x-J's remote runtime story: every machine
runs an `ato daemon` that registers itself with a peer ATO daemon
(via mDNS on a LAN, or an authenticated cloud-relay handshake across
the open internet). Once two daemons know about each other,
dispatches route as wire-protocol calls in either direction —
laptop → server *or* server → laptop — and the activity feed /
sessions sync between them.

Concretely:
- `ato daemon` (background service) listens on a Unix socket + an
  authenticated WebSocket; persists its identity (keypair) in
  `~/.ato/daemon/`.
- Peer discovery: mDNS (LAN), invite codes (manual pairing), or a
  cloud-relay channel (Pro tier) for NAT-traversal.
- Wire protocol: dispatch / kill / list-runs / sessions / activity
  posts — same surface as the local Tauri commands today.
- ACL: per-peer scopes (e.g. server can call `claude` but not read
  `secrets`).

This is Phase 7+ territory because the security / pairing / NAT
story alone is multi-week, and the existing one-way SSH adapter
already covers ~80% of the practical use case (most people want to
*invoke* a beefy remote machine, not have the remote initiate back).
Worth scoping the moment a real user asks for the reverse direction.

## Phase 6 — Cross-runtime agent conversations (Slice A + B shipped v2.3.33)

The activity feed (Phase 5) is async broadcast — anyone posts, anyone
reads. Phase 6 adds the synchronous conversation primitive: two LLMs
talking to each other *through* ATO until they reach consensus or one
side calls a final decision.

Today every `ato dispatch` opens a fresh chat. The continuity lives
in the dispatcher's context (e.g. Claude Code packing prior rounds
into each new prompt). That works for one-shot review but not for:
- Multi-turn delegation ("kick off a 5-step task, come back later,
  resume where you left off")
- Iterative refinement between two agents (codex asks Claude a
  clarifying question mid-review; Claude answers; codex revises)
- Long-running negotiations where consensus matters more than speed

### Surface

- `ato sessions new --runtime claude --as <slug>` — open a sticky
  conversation, returns a session id
- `ato dispatch claude "..." --session <id>` — append to the
  conversation, returns the response
- `ato sessions list` / `ato sessions get <id>` / `ato sessions
  archive <id>`
- ATO maintains `session_id → runtime-native-session-id` mapping
  per runtime (claude `--resume`, codex `--continue` flag, Gemini's
  similar). Each runtime stays in its own thread; ATO is the
  registry, not a multiplexer.

### Cross-runtime conversations

The harder, more interesting half: codex and Claude talking *to
each other* through ATO.
- Codex receives a review request, replies with "approving but
  want @claude's read on X"
- ATO parses the @-mention, dispatches X to Claude on a new (or
  existing) session
- Claude's response goes back into codex's session as a turn
- Loop until one side outputs `[CONSENSUS]` or the human
  intervenes

The mechanism is plumbing: tag detection, session bridging, turn
budgeting (cap at N round-trips), termination keywords. The
*judgment* — when is the discussion converging vs spinning —
is the harder design question and probably starts as
"human-in-the-loop after 3 rounds."

**Shipped (v2.3.33):**
- `ato dispatch <runtime> "..." --session <id> --tag-bridge` —
  after the primary response, scan for `@<token>` mentions, resolve
  through remote_runtimes → api_providers → CLI runtimes, and
  dispatch the next round into the same session. Loops until
  `[CONSENSUS]` on a line by itself, no mention found, or
  `--max-rounds` (default 3).
- `ato bridge --session <id> --max-rounds N` for manual re-triggers.
- Self-reference guard, code-fence stripping in the mention parser.
- Session-runtime constraint relaxed: a session can host turns
  from multiple runtimes; the original anchor stays in
  `sessions.runtime` for native --resume.
- CLI runtimes (claude, codex, gemini, hermes, openclaw) get a
  transcript prefix when continuing a non-anchored session, so
  cross-runtime history is visible without native session resume.

**Still open:**
- Smarter consensus detection (currently exact line-match). Could
  let the model emit a structured tag like `<consensus/>`.
- "Spinning" detector — when N rounds pass without progress, escalate
  to a human via activity feed rather than just hitting the round cap.
- Multi-mention round-robin (today: first resolvable mention wins).

### Why it lives after Phase 5

The activity feed gives us the storage shape (posts with
author_kind / kind / payload). Phase 6 sessions could be modeled
as `kind=session_turn` posts grouped by `payload.session_id`,
making the feed and sessions the same substrate viewed two ways.
Or sessions get their own table — TBD when scoping.

---

## Future Runtime Support

As new AI coding agents emerge:
- Cursor
- Windsurf / Codeium
- Aider
- Continue.dev
- Custom agents via plugin API
