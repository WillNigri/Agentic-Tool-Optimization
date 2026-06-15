// ato — the local-first CLI for ATO.
//
// Talks to the same SQLite database (~/.ato/local.db) the desktop GUI
// reads/writes. Designed to be driven by humans AND coding agents:
// every meaningful operation outputs JSON to stdout by default
// (parseable), with a --human flag that switches to a readable
// terminal-friendly view.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod api_dispatch;
mod api_dispatch_tools;
mod byok;
mod commands;
mod encryption;
// v2.9.0 PR-1 — grounded mode foundation. Compiles agent record +
// per-dispatch overrides into a checked policy; computes the receipt
// verdict that powers the "every AI follows your rules" pitch. Strict
// enforcement (mid-stream tool rejection) lands in PR-2.
mod grounding;
// v2.10.0 PR-1 — Methodology Runner foundation. Composes grounded-mode
// receipts into one methodology run with dual cost accounting (customer
// spend + our margin). Spec at docs/methodology-runner.md; empirical
// motivation in Part 5 of the v2.9 build log series.
mod methodology;
// v2.11 PR-12.05 — open-core tier gate. Free = run primitives. Pro =
// automations we package on top of the primitives. See module header
// for the resolution chain.
mod tier;
// v2.11 PR-12.6 — shared `ato` binary discovery (ATO_CLI_PATH override +
// homebrew/app-bundle fallback). Used by the methodology runner +
// diagnose dispatch paths so a dev build can delegate to the prod
// binary for keychain-bound API providers.
mod cli_path;
mod review_tools;
// v2.16 PR-B — initiator attribution detection (kind / surface / id).
// Env-first resolution of who/what started a dispatch; populates the
// PR-A schema columns at the edge. See module header.
mod attribution;
// v2.15.4 — pause-and-wake persistence + lifecycle (war_room E063A89E).
// Authoritative storage for paused_dispatches table; loop_runs has
// mirror columns for fast queries. See module header for the full design.
mod paused_dispatches;
mod daemon;
mod db;
mod events_publisher;
mod live_runs;
mod output;
mod pro_client;
mod quota;
mod remote_runtime;
mod runtime;
// v2.13 Phase 6.x polish — read-only filesystem probes for each
// runtime's local quota state file (e.g. ~/.claude/usage.json). Pure
// observability; no network. Powers `ato runtimes status --with-quota`
// and the desktop's RuntimeQuotaPanel via the same JSON output.
mod runtime_quota;

#[derive(Parser, Debug)]
#[command(
    name = "ato",
    version,
    about = "Local-first CLI for ATO — the developer-workflow ops platform for multi-runtime AI agents",
    long_about = "ATO CLI. Read AGENTS.md in the repo root for the full command surface, MCP equivalents, and recipes.\n\nAll commands output JSON to stdout by default. Pass --human for readable formatting.",
)]
struct Cli {
    /// Output format: JSON by default (machine-readable), --human for terminal-friendly
    #[arg(long, global = true)]
    human: bool,

    /// Suppress non-essential output (errors still print to stderr)
    #[arg(long, global = true)]
    quiet: bool,

    /// Override the SQLite DB path (default: ~/.ato/local.db)
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Sign in, sign up, or manage your ATO Cloud account.
    Auth(commands::auth::AuthArgs),
    /// Shortcut for `ato auth login`.
    Login {
        /// Email (prompted if omitted)
        #[arg(long)]
        email: Option<String>,
        /// Password (prompted securely if omitted)
        #[arg(long)]
        password: Option<String>,
    },
    /// Shortcut for `ato auth signup`.
    Signup {
        /// Email (prompted if omitted)
        #[arg(long)]
        email: Option<String>,
        /// Password (prompted securely if omitted)
        #[arg(long)]
        password: Option<String>,
        /// Display name (prompted if omitted)
        #[arg(long)]
        name: Option<String>,
    },
    /// Shortcut for `ato auth logout`.
    Logout,
    /// Shortcut for `ato auth whoami`.
    Whoami,
    /// Upgrade / status for ATO Pro subscription (Phase A chunk 6).
    Pro(commands::pro::ProArgs),
    /// Scheduled evaluators — run quality checks on cloud traces automatically (Pro).
    Evaluators(commands::evaluators::EvaluatorsArgs),
    /// v2.10.0 PR-2 — methodology runner (local-first, Pro extends to cloud later).
    /// Define + list + inspect methodologies (reusable test recipes); estimate
    /// cost before fan-out. The runner that actually fans out + composes the
    /// receipts lands in v2.10 PR-3. See docs/methodology-runner.md.
    Evaluations(commands::methodology::EvaluationsArgs),
    /// v2.11 PR-11 — workspaces. Local-first namespace primitive for
    /// organizing agents + methodologies + runs. Free tier ships a single
    /// "Personal" workspace; Team tier (ato-cloud) adds multi-user
    /// membership + cross-device sync over the same tables.
    Workspaces(commands::workspaces::WorkspacesArgs),
    /// v2.11 PR-12.5 — production_signals (OSS consumer side). Add /
    /// list / delete signals the diagnose pipeline consumes when the
    /// methodology is bound to an agent. The Langfuse/Helicone ingester
    /// lives in ato-cloud; this CLI accepts any structured JSON the
    /// customer can pipe in (`langfuse traces export --json | ato
    /// production-signals add --agent-slug X -f -`).
    #[command(name = "production-signals")]
    ProductionSignals(commands::production_signals::ProductionSignalsArgs),
    /// v2.13 — Team workspaces. Share agents + methodologies with
    /// teammates (Team tier; persistence + tier gating in ato-cloud).
    Teams(commands::teams::TeamsArgs),
    /// v2.14 — Loop Composer. Persisted SQLite-backed graphs of LLM
    /// operations (dispatch / methodology run / diagnose / review /
    /// war-room) that compose into recurring inference workflows.
    /// Reframed from the v2.13 Automations tab. See Loop Composer
    /// plan: ~/.claude/plans/eager-yawning-crane.md.
    Loop(commands::loops::LoopArgs),
    /// v2.16 — Missions: proactive goal-driven coordinator. Spawns Loops
    /// (workers) over time toward a stated goal with verifiable success
    /// criteria. See `docs/v2.16-missions.md` for the design.
    #[command(name = "missions")]
    Missions(commands::missions::MissionArgs),
    /// Log a Claude Code subagent (code-writer / cso / pr-reviewer /
    /// etc.) run so it appears in the same Sessions feed as `ato
    /// dispatch` runs. Bracket each Agent tool invocation:
    /// `ato subagent log create` before, `ato subagent log finish` after.
    /// Multi-agent fan-outs share a `--war-room-id` and can be summarized
    /// via `ato war-rooms close <id>`.
    #[command(name = "subagent")]
    Subagent(commands::subagent::SubagentArgs),
    /// v2.17 — Bundles: packaged inference results. A bundle = a source
    /// row (mission / methodology run / loop run / session / dispatch) +
    /// its dispatches + judge scores + artifact files + manifest.
    /// Exportable as a tarball for sharing externally.
    #[command(name = "bundles")]
    Bundles(commands::bundles::BundlesArgs),
    /// v2.17 — Inputs: stored markdown / text / json context bundles
    /// addressable by slug, so agent / loop / methodology configs can
    /// reference a named prompt scaffold instead of duplicating it.
    Inputs(commands::inputs::InputsArgs),
    /// Cost optimization — compare runtimes on YOUR data and get switch recommendations.
    #[command(name = "optimize")]
    Optimize(commands::cost_recommend::CostRecommendArgs),
    /// Cloud trace management — backfill local traces to cloud.
    Traces(commands::traces::TracesArgs),
    /// Inspect recent dispatches (executions of an agent / runtime)
    Dispatches {
        #[command(subcommand)]
        sub: DispatchesSub,
    },
    /// Active and historical runs
    Runs {
        #[command(subcommand)]
        sub: RunsSub,
    },
    /// Configuration change history (model swaps, prompt edits)
    #[command(name = "config-changes")]
    ConfigChanges {
        #[command(subcommand)]
        sub: ConfigChangesSub,
    },
    /// File attribution for a specific run (which files the agent touched)
    #[command(name = "files-touched")]
    FilesTouched {
        /// Run ID (the execution_logs row id, or the cloud trace ID)
        id: String,
    },
    /// Replay history and replay-jobs lookup
    Replays {
        #[command(subcommand)]
        sub: ReplaysSub,
    },
    /// Dispatch a prompt to a runtime
    Dispatch {
        /// Runtime: claude, codex, gemini, openclaw, hermes
        runtime: String,
        /// The prompt text
        prompt: String,
        /// Override the model (per-runtime: --model claude-sonnet-4-6, etc.)
        #[arg(long)]
        model: Option<String>,
        /// Optional agent slug. When set, the agent's persona
        /// (system_prompt from the SQLite agents table) is prepended
        /// to the dispatch as a `## Persona` block, and the slug is
        /// recorded for telemetry.
        #[arg(long)]
        agent: Option<String>,
        /// v2.3.31 Phase 6 Slice A — resume an existing sticky session.
        /// `ato sessions new` returns the id to pass here.
        #[arg(long)]
        session: Option<String>,
        /// PR 14 (Sessions UX polish, 2026-05-18) — tag this dispatch
        /// with a shared war-room id so parallel R1 dispatches across
        /// runtimes are grouped into a single "war-room" card in the
        /// Sessions feed. Any UUID-shaped string is accepted; users
        /// typically generate one with `uuidgen` and pass it to N
        /// `ato dispatch` calls. Standalone dispatches (no --session)
        /// + the same --war-room-id = one logical war-room round
        /// without colliding on session_turns' PRIMARY KEY.
        #[arg(long = "war-room-id")]
        war_room_id: Option<String>,
        /// PR 16 (2026-05-18) — multi-turn war-rooms. Round number
        /// (1-indexed) for this dispatch within the war-room.
        /// Defaults to 1 when --war-room-id is set without --war-
        /// room-round. For round > 1 the dispatch sees a synthesized
        /// transcript of all prior rounds (every seat's reply,
        /// including this seat's own) before the LLM is called —
        /// each seat answers independently within a round but every
        /// round sees the full peer history. Caller is responsible
        /// for incrementing the round counter; the CLI does NOT
        /// auto-compute MAX(round)+1 (would race under parallel
        /// dispatches).
        #[arg(long = "war-room-round")]
        war_room_round: Option<i64>,
        /// v2.3.33 Phase 6 Slice B — after the response, scan for
        /// `@<runtime>` mentions and bridge the conversation to that
        /// runtime, then loop until `[CONSENSUS]` or --max-rounds.
        /// Requires --session.
        #[arg(long, default_value_t = false)]
        tag_bridge: bool,
        /// v2.3.33 Phase 6 Slice B — max bridge round-trips before
        /// the loop bails (default 3).
        #[arg(long, default_value_t = 3)]
        max_rounds: u32,
        /// v2.3.47 Phase 6.x-F — stream the response chunk-by-chunk
        /// to stdout instead of buffering the whole reply. Currently
        /// supported for API providers (MiniMax / Grok / DeepSeek /
        /// Qwen / OpenRouter); ignored for CLI runtimes.
        #[arg(long, default_value_t = false)]
        stream: bool,
        /// v2.3.48 — emit streamed chunks as line-delimited JSON
        /// events (`{"type":"chunk","text":"..."}` per chunk, then
        /// `{"type":"done","result":{...}}` at the end). Designed
        /// for desktop GUI / wrappers parsing per-line. Implies
        /// `--stream`; ignored without an API provider runtime.
        #[arg(long, default_value_t = false)]
        stream_jsonl: bool,
        /// v2.9.0 PR-1 — grounded mode. Propose a different grounding
        /// mode for this dispatch (off | soft | strict). Tighten-only:
        /// refused if it would relax below the agent's allowed_mode_floor
        /// (refusal is recorded on the receipt; the dispatch still runs
        /// under the agent's record). When unset, the agent's record
        /// mode applies. See docs/grounding.md.
        #[arg(long = "mode-override")]
        mode_override: Option<String>,
        /// v2.9.0 PR-1 — grounded mode. Comma-separated list of tools
        /// the agent MUST call at least once before its reply is marked
        /// compliant (e.g. `--require-tools read_file,grep`).
        /// Tightens only — always accepted. Recorded as
        /// MustUseTool mandatory rules with auto-id `cli-tool-N`.
        #[arg(long = "require-tools", value_delimiter = ',')]
        require_tools: Vec<String>,
        /// v2.9.0 PR-1 — grounded mode. Comma-separated list of file
        /// path globs the agent MUST read via a read_file-style tool
        /// at least once (e.g. `--require-paths "src/auth/**,tests/auth/**"`).
        /// Tightens only — always accepted. Recorded as
        /// MustReadPathGlob mandatory rules with auto-id `cli-path-N`.
        #[arg(long = "require-paths", value_delimiter = ',')]
        require_paths: Vec<String>,
        /// v2.9.0 PR-1 — grounded mode. Comma-separated additional deny
        /// rules (e.g. `--additional-denies "Bash,Bash(rm:*)"`).
        /// Tightens only — always accepted. Format matches v2.7.8
        /// permissions strings.
        #[arg(long = "additional-denies", value_delimiter = ',')]
        additional_denies: Vec<String>,
        /// v2.9.0 PR-1 — grounded mode. Skip ONE mandatory rule for
        /// this dispatch only. Pair with --skip-reason. The rule still
        /// appears in the override audit so the receipt records the skip
        /// + the reason verbatim (no silent bypass). Counts against
        /// the agent's compliance metric.
        #[arg(long = "skip-mandatory")]
        skip_mandatory: Option<String>,
        /// v2.9.0 PR-1 — grounded mode. Required when --skip-mandatory
        /// is set. The reason is recorded verbatim on the receipt
        /// (`grounding_overrides` JSON) so the audit trail is complete.
        #[arg(long = "skip-reason")]
        skip_reason: Option<String>,
        /// v2.9.0 PR-1 — grounded mode. Preview the compiled policy
        /// without invoking the runtime — useful for "what tools would
        /// this agent be allowed to call?" before paying for a real
        /// dispatch. Override audit records `DryRun`; no runtime call,
        /// no execution_logs row written.
        #[arg(long = "dry-run", default_value_t = false)]
        grounding_dry_run: bool,
        /// PR-14 (Pro) — recursive-safe dispatch with depth + budget +
        /// cycle detection. When set, the dispatch delegates to the
        /// private `ato-pro dispatch` binary which enforces caps before
        /// each call. `depth_cap=1` = no recursion; `depth_cap=3` is
        /// the default for sub-agent workflows. 0 is rejected as
        /// ambiguous. Customers can replicate by hand: increment
        /// `ATO_DISPATCH_DEPTH` env var + check before each call.
        #[arg(long = "depth-cap")]
        depth_cap: Option<u32>,
        /// PR-14 (Pro) — total budget envelope in USD for the entire
        /// recursive chain. Default unset (no budget enforcement).
        /// When set, triggers --depth-cap forwarding to ato-pro.
        #[arg(long = "budget")]
        budget: Option<f64>,
        /// PR-14 (Pro) — per-call cost estimate charged against the
        /// budget. Default 0.05 (a sonnet-class call). Only meaningful
        /// when --budget is set.
        #[arg(long = "estimated-call-cost")]
        estimated_call_cost: Option<f64>,
        /// PR-14 (Pro) — disable cycle detection on the recursive
        /// harness. Default off (cycle detection ON).
        #[arg(long = "no-cycle-detect", default_value_t = false)]
        no_cycle_detect: bool,
    },
    /// Replay an existing dispatch against a different runtime/model
    Replay {
        #[command(subcommand)]
        sub: ReplaySub,
    },
    /// Compare two runs side-by-side (by id or cloud trace ID)
    Compare {
        /// First run ID
        a: String,
        /// Second run ID
        b: String,
    },
    /// Zero-config first-run demo: same prompt through two runtimes, comparison table at the end
    #[command(name = "demo-compare")]
    DemoCompare {
        /// Override the demo prompt (defaults to a short merge-sort explainer)
        #[arg(long)]
        prompt: Option<String>,
        /// Comma-separated runtimes to use (defaults to fallback ladder: configured API keys → Ollama → stubs)
        #[arg(long)]
        runtimes: Option<String>,
    },
    /// Author skills (Phase 1 ships only "draft from replay")
    Skills {
        #[command(subcommand)]
        sub: SkillsSub,
    },
    /// Terminate a running dispatch
    Kill {
        /// Run ID to kill (must be currently in live_runs)
        run_id: String,
    },
    /// Manage agents (create / update minimal records — full authoring lives in the GUI)
    Agents {
        #[command(subcommand)]
        sub: AgentsSub,
    },
    /// Make `ato` reachable on your shell's PATH (run once after install)
    #[command(name = "setup-path")]
    SetupPath {
        /// Only check whether ato is already on PATH; don't make changes
        #[arg(long)]
        check: bool,
        /// Override the install directory (defaults to /usr/local/bin then ~/.local/bin)
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Replace an existing `ato` on PATH that points at a different binary
        #[arg(long)]
        force: bool,
    },
    /// Regression detection (joins config changes with trace stats over local data)
    Regressions {
        #[command(subcommand)]
        sub: RegressionsSub,
    },
    /// Cost recommendations (when historical multi-runtime data justifies a swap)
    Cost {
        #[command(subcommand)]
        sub: CostSub,
    },
    /// Manage ops recipes (programmable trigger→action workflows)
    Recipes {
        #[command(subcommand)]
        sub: RecipesSub,
    },
    /// Inspect the event bus (event audit log)
    Events {
        #[command(subcommand)]
        sub: EventsSub,
    },
    /// Activity feed — shared human + agent + system post stream
    Posts {
        #[command(subcommand)]
        sub: PostsSub,
    },
    /// Runtime status / quota visibility
    Runtimes {
        #[command(subcommand)]
        sub: RuntimesSub,
    },
    /// Sticky multi-turn conversations (Phase 6 Slice A — claude only for now)
    Sessions {
        #[command(subcommand)]
        sub: SessionsSub,
    },
    /// v2.7.13 — close / reopen / inspect war rooms. A war room is the
    /// N execution_logs rows sharing a war_room_id; closing one runs
    /// the coordinator over every seat's reply and persists a
    /// title/summary/tags/category on the new `war_rooms` row.
    #[command(name = "war-rooms")]
    WarRooms {
        #[command(subcommand)]
        sub: WarRoomsSub,
    },
    /// v2.7.13 — close / reopen / inspect chat threads. Same close
    /// shape as sessions (coordinator summarizes the messages) but
    /// targets the chat_threads / chat_messages tables.
    Chats {
        #[command(subcommand)]
        sub: ChatsSub,
    },
    /// v2.7.14 master_key_v2 PR-6 — read the OS-keychain master key
    /// for export (so PR-5's desktop "paste the old key" flow is
    /// ergonomic on machines where the user can't drop to `security`
    /// CLI). Behind a confirmation flag so accidental shell-history
    /// captures stay rare.
    #[command(name = "master-key")]
    MasterKey {
        #[command(subcommand)]
        sub: MasterKeySub,
    },
    /// Cross-runtime conversation bridge (Phase 6 Slice B). Scans the
    /// latest assistant turn of a session for `@<runtime>` mentions and
    /// loops dispatches between runtimes until `[CONSENSUS]` or the
    /// round cap. Useful for manual re-triggers after a transient
    /// failure mid-loop.
    Bridge {
        /// Session id (from `ato sessions list`).
        #[arg(long)]
        session: String,
        /// Max bridge round-trips before bailing.
        #[arg(long, default_value_t = 3)]
        max_rounds: u32,
    },
    /// Phase 6.x-K — eval-score ratchet. Lock a quality floor per
    /// agent / runtime / global; `ratchet check` exits non-zero when
    /// the recent window's success rate dips below it. Designed to
    /// drop into CI / pre-deploy hooks.
    Ratchet {
        #[command(subcommand)]
        sub: RatchetSub,
    },
    /// Multi-LLM code review with rich context. Captures the diff
    /// against `--against <ref>` (default: merge base with main),
    /// the full text of every touched file, recent git log per file,
    /// and build/test output. Dispatches that bundle to N reviewers
    /// in a shared session so the second reviewer sees the first's
    /// findings via history replay (no diff re-paste). Optional
    /// `--consensus` round surfaces real disagreements that
    /// polite-agreement bias otherwise hides. Saves a markdown
    /// transcript ready to paste into a PR description.
    Review {
        /// Base ref to diff against. Defaults to the merge base
        /// with `origin/main` (or `main` if no remote), or `HEAD~1`
        /// as a last resort.
        #[arg(long)]
        against: Option<String>,
        /// Reviewer runtime slug; repeatable. Defaults to the first
        /// two configured of (minimax, google, grok, deepseek, qwen,
        /// openrouter).
        #[arg(long = "reviewer")]
        reviewers: Vec<String>,
        /// Write the transcript to this markdown file. Otherwise
        /// emits structured JSON to stdout (or prints inline in
        /// --human mode).
        #[arg(long)]
        out: Option<String>,
        /// Skip running `cargo build` even if Rust files changed.
        #[arg(long)]
        skip_build: bool,
        /// Skip running `cargo test` even if Rust files changed.
        #[arg(long)]
        skip_tests: bool,
        /// After the initial review, run a consensus round where
        /// each reviewer is asked which findings they'd withdraw
        /// and which from others they want to push back on.
        #[arg(long)]
        consensus: bool,
        /// Strip per-file content from the bundle. The reviewer
        /// gets the diff + a list of touched file paths + recent
        /// log, and is expected to call `read_file` / `grep` to
        /// examine the live code. Useful for "force the LLM to
        /// behave like a human reviewer" experiments and for
        /// extremely large diffs that overflow the prompt cap.
        #[arg(long)]
        lean: bool,
    },
    /// Phase 7.0 — bi-directional LAN mesh daemon (scaffold).
    /// Step 1 ships start / stop / status; step 2 (v2.4.1) adds mDNS
    /// discovery on `_ato._tcp.local` and the `mesh discovered`
    /// surface. WS+JSON-RPC protocol, pairing, and the GUI Mesh tab
    /// land in subsequent slices.
    Daemon {
        #[command(subcommand)]
        sub: DaemonSub,
    },
    /// Phase 7.0 mesh — list discovered peers, manage pairing (once
    /// step 4 ships).
    Mesh {
        #[command(subcommand)]
        sub: MeshSub,
    },
    /// v2.13 — universal multi-LLM passive observer. Tails
    /// ~/.claude/projects, ~/.codex/sessions, and ~/.gemini for
    /// session JSONLs and ingests each (user-prompt, assistant-
    /// response) pair into execution_logs as
    /// `dispatch_kind='passive_observation'`. The desktop app
    /// auto-starts this on boot; the CLI surfaces it for headless
    /// dev boxes, CI runners, and remote servers. See
    /// [[ato-live-billing-path]].
    Observe {
        #[command(subcommand)]
        sub: ObserveSub,
    },
}

#[derive(Subcommand, Debug)]
enum ObserveSub {
    /// Start the watcher in the foreground. Ctrl-C to stop. Writes
    /// ~/.ato/observe.pid for `ato observe status / stop`.
    Start {
        /// Restrict which runtimes to watch. Repeatable: --runtime
        /// claude --runtime codex --runtime gemini. Omit for all.
        #[arg(long = "runtime")]
        runtimes: Vec<String>,
    },
    /// Signal the running observer (read from ~/.ato/observe.pid) to
    /// stop. Best-effort: if the process is gone we clean up the
    /// stale pidfile and report success.
    Stop,
    /// Print whether an observer is running and on which sources.
    Status,
}

#[derive(Subcommand, Debug)]
enum MeshSub {
    /// List peers found on the local network via mDNS. Discovery
    /// does NOT mean trust — promoting a discovered peer into the
    /// allowlist will require the pairing handshake (step 4).
    Discovered {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

#[derive(Subcommand, Debug)]
enum DaemonSub {
    /// Run the daemon in the foreground. Spawn under launchd /
    /// systemd in deployments; ok to background with `&` for ad-hoc
    /// development.
    Start,
    /// Send SIGTERM to the running daemon (pid recorded at
    /// ~/.ato/daemon/daemon.pid).
    Stop,
    /// Report daemon state: running / not, pid, peer_id, public key.
    Status,
}

#[derive(Subcommand, Debug)]
enum RatchetSub {
    /// Lock a quality floor for a target.
    Lock {
        /// `agent:<slug>`, `runtime:<name>`, or `global`.
        #[arg(long)]
        target: String,
        /// How many days back to use for the baseline (default 30).
        #[arg(long, default_value_t = 30)]
        days: i64,
        /// How far below baseline counts as fail, in absolute terms.
        /// E.g. 0.05 = 5 percentage points. Default 0.05.
        #[arg(long, default_value_t = 0.05)]
        threshold: f64,
        /// Optional free-text note saved with the lock.
        #[arg(long)]
        notes: Option<String>,
    },
    /// List all locked ratchets.
    List,
    /// Check the recent window against the locked floors. Exits
    /// non-zero (CI-fail) when any ratchet is breached.
    Check {
        /// Optional `--target ...` to check only one ratchet.
        #[arg(long)]
        target: Option<String>,
        /// Window the check looks back over (default 7 days).
        #[arg(long, default_value_t = commands::ratchet::CHECK_WINDOW_DEFAULT)]
        window_days: i64,
        /// Also post a system message to the activity feed for every
        /// breach. Ops recipes consume the underlying ratchet_breach
        /// event already; this flag is for the human-glance use case.
        #[arg(long, default_value_t = false)]
        post_on_fail: bool,
    },
    /// Show current rates vs floors without failing the CLI on a breach.
    Status {
        #[arg(long)]
        target: Option<String>,
    },
    /// Remove a locked ratchet.
    Unlock {
        #[arg(long)]
        target: String,
    },
}

#[derive(Subcommand, Debug)]
enum SessionsSub {
    /// Create a new sticky session
    New {
        /// Runtime backing this session (claude — Slice A only)
        #[arg(long)]
        runtime: String,
        /// Optional agent slug attached to this session
        #[arg(long = "as")]
        agent_slug: Option<String>,
        /// Optional human-readable title for `ato sessions list`
        #[arg(long)]
        title: Option<String>,
        /// PR 11 — snapshot a project id at create time. The session
        /// inherits the project for filtering + display purposes. When
        /// omitted, sessions are born project-less and the close-time
        /// coordinator may still suggest one. Validated against the
        /// projects table; an unknown id is silently dropped to None
        /// rather than failing the create (UI cache may be stale).
        #[arg(long)]
        project: Option<String>,
    },
    /// List sessions newest-first
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Get a single session by id
    Get { id: String },
    /// Delete a session (does NOT clean up the underlying runtime's history)
    Delete { id: String },
    /// Close a session — the coordinator agent generates a title,
    /// summary, topic tags, category, team, and inferred project_id
    /// from the turn history, all persisted on the session row. A
    /// closed session can be reopened with `ato sessions reopen`.
    Close {
        id: String,
        /// Override the coordinator agent slug. Defaults to the
        /// session's stored agent_slug, then falls back to a generic
        /// summarizer running on the session's anchor runtime.
        #[arg(long = "as")]
        agent_slug: Option<String>,
        /// Override the summarizer model.
        #[arg(long)]
        model: Option<String>,
        /// v2.7.12 — pick which LLM runtime summarizes the session
        /// (e.g. `--coordinator anthropic`, `--coordinator google`,
        /// `--coordinator minimax`). Must be a registered API
        /// provider slug with a resolvable key. Takes precedence over
        /// --as / the session's stored agent_slug / the session's
        /// anchor runtime when picking the summarizer.
        #[arg(long)]
        coordinator: Option<String>,
        /// v2.7.12 — free-form human note persisted on the session's
        /// `human_comment` column. Surfaced in the closed-session
        /// summary card alongside the coordinator's auto-generated
        /// summary so the human's framing of the conversation lives
        /// next to the LLM's. Trimmed; empty becomes NULL.
        #[arg(long = "human-comment")]
        human_comment: Option<String>,
        /// Suppress the warning emitted to stderr when the coordinator
        /// omits `category` or `team` from its JSON response. Closing
        /// still proceeds in either case (NULL columns are allowed by
        /// the schema); this flag just acknowledges the gap so scripted
        /// closes don't trip alerting on stderr noise. Does NOT bypass
        /// the parse-time validation of an out-of-vocab category — that
        /// remains a hard error.
        #[arg(long = "force-close-without-context", default_value_t = false)]
        force_close_without_context: bool,
    },
    /// Reopen a previously-closed session. The next dispatch can
    /// continue the conversation; the next close will refresh the
    /// summary with the new turns.
    Reopen { id: String },

    // ── v2.15 Wave 4 — team-shared resource CLI parity ────────────────────

    /// v2.15 Wave 4 — share this session with a team.
    #[command(name = "share")]
    Share {
        /// Session id (UUID) to share.
        id: String,
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
    },
    /// v2.15 Wave 4 — remove a team share for this session.
    #[command(name = "unshare")]
    Unshare {
        /// Session id (UUID) to unshare.
        id: String,
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
    },
    /// v2.15 Wave 4 — list sessions shared with this team.
    #[command(name = "list-shared")]
    ListShared {
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
    },
    /// v2.15 Wave 4 — append a new event to a team-shared session.
    #[command(name = "append-event")]
    AppendEvent {
        /// Session id (UUID).
        id: String,
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
        /// Event kind, e.g. `turn_appended`, `judge_verdict`.
        #[arg(long)]
        kind: String,
        /// Payload as inline JSON, or @path/to/file.json.
        #[arg(long)]
        json: String,
        /// Use the E2E two-step encrypted flow.
        /// NOTE: Refused in Wave 4 with a clear error; use the desktop
        /// app or omit --encrypted for plaintext.
        #[arg(long)]
        encrypted: bool,
    },
}

/// v2.7.13 — war-room close lifecycle subcommands. Mirrors the shape
/// of `SessionsSub::Close/Reopen/Get` since both go through the same
/// `conversation_close::close_conversation` orchestrator.
#[derive(Subcommand, Debug)]
enum WarRoomsSub {
    /// Close a war room — coordinator summarizes every seat's reply
    /// and persists title/summary/tags/category/team to the war_rooms
    /// row. A closed war room can be reopened with `ato war-rooms
    /// reopen <id>`.
    Close {
        id: String,
        #[arg(long = "as")]
        agent_slug: Option<String>,
        #[arg(long)]
        model: Option<String>,
        /// Pick which LLM runtime summarizes the war room (e.g.
        /// `--coordinator anthropic`). Required when no API key has
        /// been configured for the default-resolution chain.
        #[arg(long)]
        coordinator: Option<String>,
        /// Free-form human note persisted on the war_rooms row,
        /// rendered alongside the coordinator's summary in the UI.
        #[arg(long = "human-comment")]
        human_comment: Option<String>,
        /// Suppress the soft warning when the coordinator omits
        /// category / team. Close still proceeds with NULLs.
        #[arg(long = "force-close-without-context", default_value_t = false)]
        force_close_without_context: bool,
    },
    /// Reopen a previously-closed war room.
    Reopen { id: String },
    /// Print the war room snapshot (status, seat count, last summary).
    Get { id: String },

    // ── v2.15 Wave 4 — team-shared resource CLI parity ────────────────────

    /// v2.15 Wave 4 — share this war room with a team.
    #[command(name = "share")]
    Share {
        /// War room id (UUID) to share.
        id: String,
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
    },
    /// v2.15 Wave 4 — remove a team share for this war room.
    #[command(name = "unshare")]
    Unshare {
        /// War room id (UUID) to unshare.
        id: String,
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
    },
    /// v2.15 Wave 4 — list war rooms shared with this team.
    #[command(name = "list-shared")]
    ListShared {
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
    },
    /// v2.15 Wave 4 — append a new event to a team-shared war room.
    #[command(name = "append-event")]
    AppendEvent {
        /// War room id (UUID).
        id: String,
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
        /// Event kind, e.g. `turn_appended`, `judge_verdict`.
        #[arg(long)]
        kind: String,
        /// Payload as inline JSON, or @path/to/file.json.
        #[arg(long)]
        json: String,
        /// Use the E2E two-step encrypted flow.
        /// NOTE: Refused in Wave 4 with a clear error; use the desktop
        /// app or omit --encrypted for plaintext.
        #[arg(long)]
        encrypted: bool,
    },
}

/// v2.7.13 — chat-thread close lifecycle subcommands. Same shape as
/// the sessions/war-rooms variants.
#[derive(Subcommand, Debug)]
enum ChatsSub {
    /// Close a chat thread — coordinator summarizes the messages and
    /// persists title/summary/tags/category/team to the chat_threads
    /// row.
    Close {
        id: String,
        #[arg(long = "as")]
        agent_slug: Option<String>,
        #[arg(long)]
        model: Option<String>,
        /// Pick which LLM runtime summarizes (e.g. `--coordinator
        /// google`). Falls through to the chat's anchored agent or
        /// the first registered API provider with a key when omitted.
        #[arg(long)]
        coordinator: Option<String>,
        /// Free-form human note persisted on the chat_threads row.
        #[arg(long = "human-comment")]
        human_comment: Option<String>,
        /// Suppress the soft warning when the coordinator omits
        /// category / team.
        #[arg(long = "force-close-without-context", default_value_t = false)]
        force_close_without_context: bool,
    },
    /// Reopen a previously-closed chat thread.
    Reopen { id: String },
    /// Print the chat thread snapshot.
    Get { id: String },

    // ── v2.15 Wave 4 — team-shared resource CLI parity ────────────────────

    /// v2.15 Wave 4 — share this chat with a team.
    #[command(name = "share")]
    Share {
        /// Chat thread id (UUID) to share.
        id: String,
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
    },
    /// v2.15 Wave 4 — remove a team share for this chat.
    #[command(name = "unshare")]
    Unshare {
        /// Chat thread id (UUID) to unshare.
        id: String,
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
    },
    /// v2.15 Wave 4 — list chats shared with this team.
    #[command(name = "list-shared")]
    ListShared {
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
    },
    /// v2.15 Wave 4 — append a new event to a team-shared chat.
    #[command(name = "append-event")]
    AppendEvent {
        /// Chat thread id (UUID).
        id: String,
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
        /// Event kind, e.g. `turn_appended`, `judge_verdict`.
        #[arg(long)]
        kind: String,
        /// Payload as inline JSON, or @path/to/file.json.
        #[arg(long)]
        json: String,
        /// Use the E2E two-step encrypted flow.
        /// NOTE: Refused in Wave 4 with a clear error; use the desktop
        /// app or omit --encrypted for plaintext.
        #[arg(long)]
        encrypted: bool,
    },
}

/// v2.7.14 master_key_v2 PR-6 — CLI mirror of the master-key
/// lifecycle. Today the only subcommand is `export`. Future PRs
/// may add `rekey --from-stdin` for headless rekey without the
/// desktop UI; held until a real headless dogfood asks for it.
#[derive(Subcommand, Debug)]
enum MasterKeySub {
    /// Print the current OS-keychain master key to stdout, base64-
    /// encoded. Used to populate PR-5's "paste the old key"
    /// textarea on a different machine / install. Requires
    /// `--confirm-i-understand-this-prints-the-key` so it never
    /// runs by accident (the key in shell history is a real
    /// leakage risk).
    Export {
        #[arg(long = "confirm-i-understand-this-prints-the-key", default_value_t = false)]
        confirm: bool,
    },
}

#[derive(Subcommand, Debug)]
enum RuntimesSub {
    /// Show known runtime quotas: which runtimes are rate-limited
    /// and until when (parsed from previous dispatch errors). With
    /// `--with-quota`, also reads each runtime's local usage file from
    /// disk and returns parsed messages-used / messages-limit / reset.
    Status {
        /// Also probe each runtime's local quota state file
        /// (~/.claude/usage.json etc.) and include the parsed
        /// messages-used / messages-limit / period-reset alongside
        /// the rate-limit rows. Read-only filesystem probe; no network.
        ///
        /// JSON SHAPE NOTE: opting into this flag switches the JSON
        /// output from the legacy bare array `[QuotaRow, …]` (v2.12
        /// shape; default) to an envelope
        /// `{ quotas: [...], runtime_quota_probes: [...] }`. Existing
        /// v2.12 consumers that don't pass the flag see no change.
        #[arg(long = "with-quota")]
        with_quota: bool,
    },
    /// Phase 6.x-I — check whether each detected runtime binary is
    /// signed / non-quarantined / non-revoked. Surfaces the specific
    /// reason and a fix command when something is broken.
    Health,
    /// v2.4.2 — Smoke-test every api-provider end-to-end with a
    /// minimal dispatch. Catches registry drift (deprecated default
    /// model, wrong URL, bad auth shape) before users hit it.
    /// Providers without a configured key are reported as `no_key`
    /// and don't fail the check. Exits non-zero if any configured
    /// provider fails the roundtrip.
    TestProviders {
        /// Run only one provider's smoke test (e.g. `--slug google`).
        #[arg(long)]
        slug: Option<String>,
    },
    /// v2.15.0 Slice C — List models the user's stored API key can
    /// actually call, fetched live from the provider's models endpoint
    /// (with a 10-minute in-process cache). Output includes `source:
    /// live | curated_fallback` so callers see provenance honestly.
    /// MiniMax has no public list endpoint, so it returns a curated
    /// fallback with `fallback_reason` set.
    Models {
        /// The provider slug (e.g. `google`, `openai`, `anthropic`).
        /// Use the same slug as `ato dispatch <slug>`.
        #[arg(long)]
        slug: String,
        /// Bypass the in-process cache for THIS call. Same UX shape as
        /// Settings → Models "Pull live" button.
        #[arg(long)]
        no_cache: bool,
    },
    /// Register a remote machine that runs a runtime CLI. Once added,
    /// `ato dispatch <slug> "..."` routes over SSH instead of spawning
    /// a local binary. Phase 6.x-J — laptop ↔ server bridging.
    AddRemote {
        /// Local slug for this remote (e.g. `claude-server`). Used as
        /// the runtime argument in `ato dispatch <slug>`.
        #[arg(long)]
        name: String,
        /// SSH host. Either bare host (with --user) or user@host.
        #[arg(long)]
        host: String,
        /// SSH port (default 22).
        #[arg(long, default_value_t = 22)]
        port: u16,
        /// SSH user. Required unless --host already contains user@.
        #[arg(long)]
        user: Option<String>,
        /// Path to the SSH private key. If omitted, ssh-agent / default
        /// keys are used (BatchMode=yes still applies).
        #[arg(long)]
        key_path: Option<String>,
        /// Base runtime running on the remote: claude / codex / gemini
        /// / hermes / openclaw. Drives argument shape.
        #[arg(long)]
        runtime: String,
        /// Path to the runtime binary on the remote, or a PATH-resolvable
        /// name (e.g. `claude` if it's in the login shell's PATH).
        #[arg(long, default_value = "")]
        binary_path: String,
        /// Optional extra args appended verbatim to every dispatch
        /// (e.g. `--no-update-check`).
        #[arg(long)]
        extra_args: Option<String>,
    },
    /// List registered remote runtimes.
    ListRemote,
    /// Remove a registered remote runtime by slug.
    RemoveRemote {
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum PostsSub {
    /// Post a message to the activity feed
    Add {
        /// Body text
        text: String,
        /// Author kind: human (default), agent, or system
        #[arg(long = "as", default_value = "human")]
        author_kind: String,
        /// Author slug (e.g. "codex-reviewer"). Omit for plain humans.
        #[arg(long = "slug")]
        author_slug: Option<String>,
        /// Post kind: message (default), event_notice, approval_request, approval_decision
        #[arg(long = "kind", default_value = "message")]
        kind: String,
        /// Optional events_log.event_seq this post relates to
        #[arg(long = "related-event-seq")]
        related_event_seq: Option<i64>,
    },
    /// List recent posts (newest first)
    List {
        /// How many to return (default 20)
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Filter by kind
        #[arg(long = "kind")]
        kind: Option<String>,
    },
    /// Get a single post by id
    Get {
        /// Post id (uuid)
        id: String,
    },
    /// Tail new posts as they land. Emits one JSONL row per post.
    Tail {
        /// Only emit posts of this kind.
        #[arg(long = "kind")]
        kind: Option<String>,
        /// Start streaming from posts created AFTER this id's
        /// timestamp. Default: skip everything that exists now and
        /// only show new posts (tail-f semantics).
        #[arg(long = "since-id")]
        since_id: Option<String>,
        /// Stop after emitting N posts (default: no cap).
        #[arg(long = "max-rows")]
        max_rows: Option<usize>,
        /// Poll interval in milliseconds (default 500, min 100, max 5000).
        #[arg(long = "poll-ms", default_value_t = 500)]
        poll_ms: u64,
    },
    /// Approve an ApprovalRequest post (writes an ApprovalDecision)
    Approve {
        /// Id of the ApprovalRequest post to approve
        request_id: String,
        /// Optional note explaining the decision
        #[arg(long = "notes")]
        notes: Option<String>,
    },
    /// Deny an ApprovalRequest post (writes an ApprovalDecision)
    Deny {
        /// Id of the ApprovalRequest post to deny
        request_id: String,
        /// Optional note explaining the decision
        #[arg(long = "notes")]
        notes: Option<String>,
    },
    /// List ApprovalRequest posts that don't have a matching
    /// ApprovalDecision yet
    Pending {
        /// How many to return (default 20)
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

#[derive(Subcommand, Debug)]
enum EventsSub {
    /// Recent events (from events_log table; populated by the desktop bus)
    Recent {
        /// Optional event type filter (regression_detected, dispatch_failed, replay_done, etc.)
        #[arg(long = "type")]
        event_type: Option<String>,
        /// How many to return (default 20)
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Tail new events as they land. Emits one JSONL row per event.
    Watch {
        /// Only emit events of this type.
        #[arg(long = "type")]
        event_type: Option<String>,
        /// Start streaming from this event_seq + 1. Default: skip
        /// everything that exists now and only show new events.
        #[arg(long = "since")]
        since_seq: Option<i64>,
        /// Stop after emitting N events (default: no cap).
        #[arg(long = "max-rows")]
        max_rows: Option<usize>,
        /// Poll interval in milliseconds (default 500, min 100, max 5000).
        #[arg(long = "poll-ms", default_value_t = 500)]
        poll_ms: u64,
    },
}

#[derive(Subcommand, Debug)]
enum RecipesSub {
    /// List installed recipes
    List,
    /// Get a single recipe by slug
    Get { slug: String },
    /// List built-in recipe templates available to install
    Templates,
    /// Install a built-in template as a working recipe
    Install {
        /// Template slug (see `ato recipes templates`)
        template_slug: String,
        /// Override the installed recipe's slug
        #[arg(long = "as")]
        rename_to: Option<String>,
    },
    /// Enable a recipe (start firing on matching events)
    Enable { slug: String },
    /// Disable a recipe (stop firing; preserves config)
    Disable { slug: String },
    /// Show audit log of recent runs for a recipe
    Runs {
        slug: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Delete a recipe (config + JSON mirror)
    Delete { slug: String },
}

#[derive(Subcommand, Debug)]
enum RegressionsSub {
    /// List regressions detected over local data
    List {
        /// Days of history to consider (default 30, max 365)
        #[arg(long, default_value_t = 30)]
        days: i64,
        /// Window on each side of a config change (hours; default 168 = 7d)
        #[arg(long = "window-hours", default_value_t = 168)]
        window_hours: i64,
        /// Min samples on each side to render a change (default 20)
        #[arg(long = "min-samples", default_value_t = 20)]
        min_samples: i64,
    },
}

#[derive(Subcommand, Debug)]
enum CostSub {
    /// Surface model-swap recommendations when local data justifies them
    Recommendations {
        /// Days of history to consider (default 30)
        #[arg(long, default_value_t = 30)]
        days: i64,
        /// Min runs per (agent, runtime) combo to be considered (default 10)
        #[arg(long = "min-runs", default_value_t = 10)]
        min_runs: i64,
    },
}

#[derive(Subcommand, Debug)]
enum DispatchesSub {
    /// Recent dispatches across all runtimes (default: last 20)
    Recent {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        runtime: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum RunsSub {
    /// Currently active runs (in-flight dispatches)
    Live,
    /// Get a single run by ID
    Get {
        /// Run / execution_logs ID
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigChangesSub {
    /// List config changes for an agent
    List {
        /// Agent slug (required — config changes are per-agent)
        #[arg(long)]
        agent: String,
        /// How far back to look (e.g. 7d, 24h, 30d). Default: 7d.
        #[arg(long, default_value = "7d")]
        since: String,
    },
}

#[derive(Subcommand, Debug)]
enum ReplaysSub {
    /// List replays for a given cloud trace ID
    #[command(name = "for-trace")]
    ForTrace {
        /// Cloud trace ID
        trace_id: String,
    },
}

#[derive(Subcommand, Debug)]
enum ReplaySub {
    /// Start a replay (synchronous — waits for the dispatch to finish)
    Start {
        /// Source trace ID (cloud_trace_id) or execution_logs ID
        source_id: String,
        /// Target runtime to replay against
        #[arg(long)]
        runtime: String,
        /// Override the target model
        #[arg(long)]
        model: Option<String>,
    },
    /// Get a replay job by ID (use --wait to poll until terminal)
    Get {
        /// Replay job ID
        job_id: String,
        /// Block until the replay reaches done/failed/cancelled
        #[arg(long)]
        wait: bool,
    },
}

#[derive(Subcommand, Debug)]
enum SkillsSub {
    /// Draft a SKILL.md from a successful replay
    Draft {
        /// Replay job ID to derive the skill from
        #[arg(long = "from-replay")]
        from_replay: String,
        /// Output path; defaults to ~/.<target-runtime>/skills/<slug>/SKILL.md
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum AgentsSub {
    /// Create a new agent record
    ///
    /// Two shapes:
    /// 1. Inline: `--slug <s> --runtime <r> --system-prompt <text>` (the historical form).
    /// 2. From file: `--from-file <path> --runtime <r>` reads a Claude-Code-style
    ///    agent file (`~/.claude/agents/<slug>.md` format: YAML frontmatter with
    ///    `name:`/`display_name:`/`description:`/`model:` + body = system prompt).
    ///    Any CLI flag overrides the matching field from the file. Slug falls
    ///    back to the filename stem when frontmatter has no `name:`.
    Create {
        /// Unique slug (per-runtime). Optional when `--from-file` is set
        /// AND the file's frontmatter has `name:` OR the filename stem is acceptable.
        #[arg(long)]
        slug: Option<String>,
        /// Runtime: claude, codex, gemini, openclaw, hermes, ollama
        #[arg(long)]
        runtime: String,
        /// Display name (defaults to slug)
        #[arg(long = "display-name")]
        display_name: Option<String>,
        /// Description / one-line summary of what the agent does
        #[arg(long)]
        description: Option<String>,
        /// Model override (e.g. claude-sonnet-4-6)
        #[arg(long)]
        model: Option<String>,
        /// System prompt. Optional when `--from-file` is set and the file
        /// has a body after its frontmatter.
        #[arg(long = "system-prompt")]
        system_prompt: Option<String>,
        /// Optional project ID to scope the agent to
        #[arg(long = "project-id")]
        project_id: Option<String>,
        /// Read agent fields from a Claude-Code-style markdown file
        /// (YAML frontmatter + body). CLI flags override file values.
        #[arg(long = "from-file")]
        from_file: Option<PathBuf>,
    },
    /// List registered agents, optionally filtered by runtime or project
    List {
        /// Filter by runtime
        #[arg(long)]
        runtime: Option<String>,
        /// Filter by project ID
        #[arg(long = "project-id")]
        project_id: Option<String>,
    },
    /// Delete an agent record
    Delete {
        /// Slug of the agent to delete
        #[arg(long)]
        slug: String,
        /// Disambiguate when the same slug exists on multiple runtimes;
        /// without this, the most-recently-used row wins
        #[arg(long)]
        runtime: Option<String>,
        /// Also remove the per-runtime config file (e.g. ~/.claude/agents/<slug>.md).
        /// Off by default — files are often checked into git or shared.
        #[arg(long = "also-remove-file")]
        also_remove_file: bool,
    },
    /// Update an existing agent's editable fields
    Update {
        /// Slug of the agent to update
        slug: String,
        /// Disambiguate when the same slug exists on multiple runtimes
        #[arg(long)]
        runtime: Option<String>,
        /// New model
        #[arg(long)]
        model: Option<String>,
        /// New system prompt
        #[arg(long = "system-prompt")]
        system_prompt: Option<String>,
        /// New display name
        #[arg(long = "display-name")]
        display_name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// Replace the skills list with this comma-separated set (skill slugs)
        #[arg(long, value_delimiter = ',')]
        skills: Option<Vec<String>>,
        /// Add a single skill to the agent's list (idempotent)
        #[arg(long = "add-skill")]
        add_skill: Option<String>,
        /// Remove a single skill from the agent's list (no-op if absent)
        #[arg(long = "remove-skill")]
        remove_skill: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let db_path = cli.db.clone().unwrap_or_else(db::default_db_path);

    // Open read-only for read-paths; write commands re-open with
    // write privileges internally.
    let ro_conn = || -> Result<rusqlite::Connection> {
        db::open_readonly(&db_path)
            .with_context(|| format!("Could not open ATO database at {}", db_path.display()))
    };

    let opts = output::Opts {
        human: cli.human,
        quiet: cli.quiet,
    };

    match cli.command {
        Commands::Auth(args) => {
            commands::auth::run(args);
            return Ok(());
        }
        Commands::Login { email, password } => {
            commands::auth::run(commands::auth::AuthArgs {
                cmd: commands::auth::AuthCommand::Login { email, password },
            });
            return Ok(());
        }
        Commands::Signup { email, password, name } => {
            commands::auth::run(commands::auth::AuthArgs {
                cmd: commands::auth::AuthCommand::Signup { email, password, name },
            });
            return Ok(());
        }
        Commands::Logout => {
            commands::auth::run(commands::auth::AuthArgs {
                cmd: commands::auth::AuthCommand::Logout,
            });
            return Ok(());
        }
        Commands::Whoami => {
            commands::auth::run(commands::auth::AuthArgs {
                cmd: commands::auth::AuthCommand::Whoami,
            });
            return Ok(());
        }
        Commands::Pro(args) => {
            commands::pro::run(args, cli.human);
            return Ok(());
        }
        Commands::Evaluators(args) => {
            commands::evaluators::run(args, cli.human);
            return Ok(());
        }
        Commands::Evaluations(args) => {
            commands::methodology::run(args, &db_path, &opts)?;
            return Ok(());
        }
        Commands::Workspaces(args) => {
            commands::workspaces::run(args, &db_path, &opts)?;
            return Ok(());
        }
        Commands::ProductionSignals(args) => {
            commands::production_signals::run(args, &db_path, &opts)?;
            return Ok(());
        }
        Commands::Teams(args) => {
            commands::teams::run(args, &db_path, &opts)?;
            return Ok(());
        }
        Commands::Loop(args) => {
            commands::loops::run(args, &db_path, &opts)?;
            return Ok(());
        }
        Commands::Missions(args) => {
            commands::missions::run(args, &db_path, &opts)?;
            return Ok(());
        }
        Commands::Subagent(args) => {
            commands::subagent::run(args, &db_path, &opts)?;
            return Ok(());
        }
        Commands::Bundles(args) => {
            commands::bundles::run(args, &db_path, &opts)?;
            return Ok(());
        }
        Commands::Inputs(args) => {
            commands::inputs::run(args, &db_path, &opts)?;
            return Ok(());
        }
        Commands::Traces(args) => {
            commands::traces::run(args, cli.human);
            return Ok(());
        }
        Commands::Optimize(args) => {
            commands::cost_recommend::run(args, cli.human, &cli.db.as_ref().map(|p| p.to_string_lossy().to_string()));
            return Ok(());
        }
        Commands::Dispatches { sub } => match sub {
            DispatchesSub::Recent {
                limit,
                runtime,
                status,
            } => commands::dispatches::recent(&ro_conn()?, limit, runtime, status, &opts),
        },
        Commands::Runs { sub } => match sub {
            RunsSub::Live => commands::runs::live(&ro_conn()?, &opts),
            RunsSub::Get { id } => commands::runs::get(&ro_conn()?, &id, &opts),
        },
        Commands::ConfigChanges { sub } => match sub {
            ConfigChangesSub::List { agent, since } => {
                commands::config_changes::list(&ro_conn()?, &agent, &since, &opts)
            }
        },
        Commands::FilesTouched { id } => commands::files_touched::run(&ro_conn()?, &id, &opts),
        Commands::Replays { sub } => match sub {
            ReplaysSub::ForTrace { trace_id } => {
                commands::replays::for_trace(&ro_conn()?, &trace_id, &opts)
            }
        },
        Commands::Dispatch {
            runtime,
            prompt,
            model,
            agent,
            session,
            war_room_id,
            war_room_round,
            tag_bridge,
            max_rounds,
            stream,
            stream_jsonl,
            mode_override,
            require_tools,
            require_paths,
            additional_denies,
            skip_mandatory,
            skip_reason,
            grounding_dry_run,
            depth_cap,
            budget,
            estimated_call_cost,
            no_cycle_detect,
        } => {
            // PR-14: when any recursive-harness flag is set, delegate
            // the entire dispatch to the Pro binary. The harness owns
            // depth + budget + cycle enforcement; OSS code never sees
            // those caps so a fork that removes the delegation gets
            // unbounded recursion (a foot-gun, not a feature) — exactly
            // the open-core boundary the codified safety harness is
            // selling.
            if depth_cap.is_some() || budget.is_some() {
                let mut args: Vec<String> = vec![
                    "--runtime".into(), runtime.clone(),
                    "--prompt".into(), prompt.clone(),
                ];
                if let Some(m) = &model {
                    args.push("--model".into());
                    args.push(m.clone());
                }
                if let Some(a) = &agent {
                    args.push("--agent".into());
                    args.push(a.clone());
                }
                if let Some(d) = depth_cap {
                    args.push("--depth-cap".into());
                    args.push(d.to_string());
                }
                if let Some(b) = budget {
                    args.push("--budget".into());
                    args.push(format!("{:.6}", b));
                }
                if let Some(e) = estimated_call_cost {
                    args.push("--estimated-call-cost".into());
                    args.push(format!("{:.6}", e));
                }
                if no_cycle_detect {
                    args.push("--no-cycle-detect".into());
                }
                return pro_client::delegate(
                    "dispatch",
                    &args,
                    &db_path,
                    opts.human,
                    opts.quiet,
                );
            }

            // stream-jsonl implies stream; a wrapper can set just
            // --stream-jsonl without needing to also pass --stream.
            let stream = stream || stream_jsonl;
            if tag_bridge && session.is_none() {
                anyhow::bail!(
                    "--tag-bridge requires --session (the bridge loop appends to that session's turn history)."
                );
            }

            // v2.9.0 PR-1 slice 2 — assemble the grounding overrides
            // from CLI flags. When nothing was passed, `overrides.has_any()`
            // is false and the dispatch path runs exactly as today; when
            // at least one flag is set, we compile a policy + record the
            // override audit + (if --dry-run) short-circuit before
            // touching the runtime.
            let parsed_mode_override = match mode_override.as_deref() {
                None | Some("") => None,
                Some(s) => {
                    let m = commands::dispatch::GroundingMode::parse(s);
                    if m.as_str() != s {
                        anyhow::bail!(
                            "--mode-override='{}' is not a known mode (expected: off | soft | strict)",
                            s
                        );
                    }
                    Some(m)
                }
            };
            let grounding_overrides = commands::dispatch::DispatchGroundingOverrides {
                mode_override: parsed_mode_override,
                additional_denies,
                require_tools,
                require_paths,
                skip_mandatory,
                skip_reason,
                dry_run: grounding_dry_run,
            };

            if grounding_overrides.has_any() {
                // Compile the policy now against conservative record
                // defaults (off / off / no rules). PR-2 will load the
                // actual agent record so the compiled policy reflects
                // the agent's denies + mandatories. The compile step is
                // what validates the override is internally consistent
                // (e.g. --skip-mandatory requires --skip-reason).
                let policy = grounding_overrides
                    .compile_with_record_defaults(
                        commands::dispatch::GroundingMode::Off,
                        commands::dispatch::GroundingMode::Off,
                        Vec::new(),
                        Vec::new(),
                    )
                    .map_err(|e| anyhow::anyhow!("grounding policy compile failed: {}", e))?;

                // v2.9.0 PR-4 — refuse strict-mode dispatches against
                // the parserless runtimes (Ollama / OpenClaw / Hermes).
                // These runtimes ARE full agents and DO have tools, but
                // their dispatch paths don't yet emit structured tool-
                // call telemetry that ATO can parse to verify rule
                // compliance. Running strict mode against them today
                // would either (a) silently succeed every dispatch as
                // `compliant` despite zero observation (false positive,
                // the opposite of the PR-1 claude regression), or
                // (b) silently fail every dispatch as `violation`
                // regardless of behavior (false negative, useless
                // signal). Both are theater.
                //
                // The synthesis decision from the design debate (see
                // plan §A2 and docs/grounding.md): refuse with options.
                // The user sees three concrete paths forward rather
                // than a black-box rejection. The experimental tool-
                // call marker parser (option 3) is documented but not
                // wired in PR-4 minimum — that's a v2.9.x follow-on.
                let parserless_runtimes = ["ollama", "openclaw", "hermes"];
                if policy.mode == commands::dispatch::GroundingMode::Strict
                    && parserless_runtimes.contains(&runtime.as_str())
                {
                    anyhow::bail!(
                        "Strict mode is not yet supported on the '{}' runtime — its dispatch \
                         path doesn't emit structured tool-call telemetry, so the verdict \
                         couldn't be honestly computed. Three options:\n\
                         \n\
                         1. Switch runtime: re-run with claude / codex / gemini (CLI runtimes \
                            with native tool-call telemetry) or any API provider (anthropic / \
                            openai / google / mistral / groq / etc — routed through the \
                            function-calling tool loop in v2.9 PR-3).\n\
                         \n\
                         2. Downgrade to soft: re-run with `--mode-override soft`. The \
                            mandatory rules are listed in the system prompt as expected \
                            behavior and the verdict will record `advisory` regardless of \
                            actual compliance — observe-only, no false positives or false \
                            negatives.\n\
                         \n\
                         3. Wait for the experimental marker parser: a follow-on slice will \
                            ship a best-effort tool-call marker parser for these runtimes \
                            (documented in docs/grounding.md §strict-on-parserless). Until \
                            then, the dispatch is refused rather than silently misreport \
                            its compliance.\n",
                        runtime
                    );
                }

                if opts.human {
                    output::emit_human(&format!(
                        "Grounding policy compiled:\n  mode: {}\n  denies: {}\n  mandatories: {}\n  override audit entries: {}",
                        policy.mode.as_str(),
                        policy.denies.len(),
                        policy.mandatories.len(),
                        policy.overrides_audit.len(),
                    ));
                }

                if grounding_overrides.dry_run {
                    // No runtime invocation. Emit the policy as the
                    // result and return early. Caller can pipe to jq
                    // to inspect the compiled override audit.
                    output::emit_json(&serde_json::json!({
                        "dry_run": true,
                        "mode": policy.mode.as_str(),
                        "denies": policy.denies,
                        "mandatories": policy.mandatories,
                        "overrides_audit": policy.overrides_audit,
                    }));
                    return Ok(());
                }
            }

            // v2.9.0 PR-2 — when grounding is on AND the runtime is
            // claude AND there's no session (sessions + stream-json
            // land in a follow-up slice), opt the claude CLI invocation
            // into --output-format stream-json so tool_use blocks are
            // surfaced for the verdict computation. The dispatch.rs
            // claude arm reads this env var at command-build time.
            // After dispatch::run returns, we parse the raw stream-json
            // response back into (response_text, tool_calls) and write
            // both onto the receipt row.
            let claude_stream_json_active = grounding_overrides.has_any()
                && runtime == "claude"
                && session.is_none();
            if claude_stream_json_active {
                std::env::set_var("ATO_CLAUDE_STREAM_JSON", "1");
            }

            // v2.9.0 PR-3 — when grounding is on AND the runtime is an
            // API provider (gemini/openai/anthropic/mistral/etc going
            // through api_dispatch.rs path), flip with_tools=true so
            // dispatch.rs's existing branch at line 1651 routes through
            // api_dispatch_tools::dispatch_with_tools(). The
            // function-calling tool loop fires, the model can call
            // read_file/grep/git_log, every call lands in
            // execution_logs.tool_calls_summary natively (no re-parsing
            // like the claude PR-2 path), and the verdict computation
            // sees real observations instead of empty.
            //
            // For claude/codex/gemini CLI runtimes, with_tools stays
            // false — the per-runtime CLI handles tools natively
            // (claude --allowedTools, codex sandbox, gemini --yolo) and
            // PR-2's stream-json parser handles the audit channel.
            // Fix B — runtime_is_api_provider must also return true for
            // CLI runtimes that have a known API fallback (gemini→google,
            // claude→anthropic, codex→openai).  When the gemini binary is
            // absent, dispatch::run routes through the google API provider;
            // if we only checked is_api_provider("gemini") (which returns
            // false) we'd compute with_tools_for_grounding=false and the
            // fallback call site would receive with_tools=false, bypassing
            // dispatch_with_tools entirely.  The static mapping mirrors
            // api_fallback_for_missing_cli() in dispatch.rs.
            let runtime_is_api_provider = crate::api_dispatch::is_api_provider(&runtime)
                || matches!(runtime.as_str(), "gemini" | "claude" | "codex");
            let with_tools_for_grounding =
                grounding_overrides.has_any() && runtime_is_api_provider;

            // Run the primary dispatch.
            // dispatch::run handles session-turn persistence so by the
            // time we return, session_turns has the assistant's reply.
            // (When grounding_overrides were passed, the post-dispatch
            // UPDATE below stamps the override audit onto the receipt
            // row dispatch::run just wrote.)
            commands::dispatch::run(
                &runtime,
                &prompt,
                model,
                agent.clone(),
                session.clone(),
                war_room_id.clone(),
                war_room_round,
                stream,
                stream_jsonl,
                with_tools_for_grounding,
                // Fix E — thread require_tools through so run_api can
                // build the correct offered set (trio UNION required).
                grounding_overrides.require_tools.clone(),
                None, // workspace_root — non-Mission dispatch uses process CWD
                &db_path,
                &opts,
            )?;

            // v2.9.0 PR-1 slice 2 — if the caller passed any grounding
            // overrides, stamp the audit JSON onto the latest
            // execution_log row this process just wrote. The compile
            // step happened earlier (above); we re-run it here because
            // the inner dispatch path doesn't see the overrides yet
            // (PR-2 will plumb them through dispatch::run). Same record
            // defaults — conservative until PR-2.
            //
            // This UPDATE is fire-and-forget: if it fails, the dispatch
            // itself already succeeded and the user already got their
            // reply. The grounding audit is observability, not correctness.
            if grounding_overrides.has_any() && !grounding_overrides.dry_run {
                if let Ok(policy) = grounding_overrides.compile_with_record_defaults(
                    commands::dispatch::GroundingMode::Off,
                    commands::dispatch::GroundingMode::Off,
                    Vec::new(),
                    Vec::new(),
                ) {
                    if let Some(overrides_json) = policy.overrides_json() {
                        let _ = commands::dispatch::stamp_grounding_overrides_on_latest(
                            &db_path,
                            agent.as_deref(),
                            session.as_deref(),
                            war_room_id.as_deref(),
                            &overrides_json,
                            &opts,
                        );
                    }

                    // v2.9.0 PR-2 — when claude was invoked with
                    // stream-json output, the row's `response` column
                    // now holds the raw NDJSON event log instead of the
                    // assistant's final text. Parse it back, extract
                    // the response_text + tool_use observations, and
                    // rewrite the row so:
                    //   - response       = the actual assistant reply
                    //   - tool_calls_summary = JSON of ToolCallAudit-shaped rows
                    //   - tool_calls_count   = observation count
                    // Then the verdict step below sees the tool calls
                    // and correctly produces compliant (not false-
                    // negative advisory+unmet, the PR-1 regression).
                    if claude_stream_json_active {
                        let _ = commands::dispatch::reparse_claude_stream_json_on_latest(
                            &db_path,
                            agent.as_deref(),
                            session.as_deref(),
                            war_room_id.as_deref(),
                            &opts,
                        );
                    }

                    // v2.9.0 PR-1 slice 3 — verdict computation. Runs
                    // AFTER the overrides stamp because both walk the
                    // same row-finding heuristic; doing the verdict
                    // second means we can detect the row reliably (it
                    // now carries grounding_overrides IS NOT NULL,
                    // which the verdict query filters on for an extra
                    // safety guard).
                    let _ = commands::dispatch::stamp_grounding_verdict_on_latest(
                        &db_path,
                        agent.as_deref(),
                        session.as_deref(),
                        war_room_id.as_deref(),
                        &policy,
                        &opts,
                    );
                }
            }

            // Clear the env var so it doesn't leak into any downstream
            // commands the shell may run.
            if claude_stream_json_active {
                std::env::remove_var("ATO_CLAUDE_STREAM_JSON");
            }
            // v2.3.33 Phase 6 Slice B — kick off the cross-runtime
            // bridge loop. Always Ok() — failures inside the loop are
            // surfaced via emit_human + execution_logs but don't
            // propagate, because the user already got a successful
            // primary response.
            if tag_bridge {
                if let Some(sid) = session {
                    commands::bridge::run_loop(&sid, max_rounds, &db_path, &opts)?;
                }
            }
            Ok(())
        }
        Commands::Replay { sub } => match sub {
            ReplaySub::Start {
                source_id,
                runtime,
                model,
            } => commands::replay::start(&source_id, &runtime, model, &db_path, &opts),
            ReplaySub::Get { job_id, wait } => {
                commands::replay::get(&job_id, wait, &db_path, &opts)
            }
        },
        Commands::Compare { a, b } => commands::compare::run(&ro_conn()?, &a, &b, &opts),
        Commands::DemoCompare { prompt, runtimes } => {
            commands::demo_compare::run(&db_path, prompt, runtimes, &opts)
        }
        Commands::Skills { sub } => match sub {
            SkillsSub::Draft { from_replay, out } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::skills::draft_from_replay(&conn, &from_replay, out, &opts)
            }
        },
        Commands::Kill { run_id } => commands::kill::run(&ro_conn()?, &run_id, &opts),
        Commands::SetupPath { check, dir, force } => commands::setup_path::run(check, dir, force, &opts),
        Commands::Regressions { sub } => match sub {
            RegressionsSub::List {
                days,
                window_hours,
                min_samples,
            } => commands::regressions::list(&ro_conn()?, days, window_hours, min_samples, &opts),
        },
        Commands::Cost { sub } => match sub {
            CostSub::Recommendations { days, min_runs } => {
                commands::cost::recommendations(&ro_conn()?, days, min_runs, &opts)
            }
        },
        Commands::Recipes { sub } => match sub {
            RecipesSub::List => commands::recipes::list(&ro_conn()?, &opts),
            RecipesSub::Get { slug } => commands::recipes::get(&ro_conn()?, &slug, &opts),
            RecipesSub::Templates => commands::recipes::templates(&opts),
            RecipesSub::Install {
                template_slug,
                rename_to,
            } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::recipes::install_template(&conn, &template_slug, rename_to, &opts)
            }
            RecipesSub::Enable { slug } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::recipes::set_enabled(&conn, &slug, true, &opts)
            }
            RecipesSub::Disable { slug } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::recipes::set_enabled(&conn, &slug, false, &opts)
            }
            RecipesSub::Runs { slug, limit } => {
                commands::recipes::runs(&ro_conn()?, &slug, limit, &opts)
            }
            RecipesSub::Delete { slug } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::recipes::delete(&conn, &slug, &opts)
            }
        },
        Commands::Events { sub } => match sub {
            EventsSub::Recent { event_type, limit } => {
                commands::events::recent(&ro_conn()?, event_type, limit, &opts)
            }
            EventsSub::Watch {
                event_type,
                since_seq,
                max_rows,
                poll_ms,
            } => commands::events::watch(
                &db_path,
                event_type,
                since_seq,
                max_rows,
                poll_ms,
                &opts,
            ),
        },
        Commands::Posts { sub } => match sub {
            PostsSub::Add {
                text,
                author_kind,
                author_slug,
                kind,
                related_event_seq,
            } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::posts::add(
                    &conn,
                    text,
                    author_kind,
                    author_slug,
                    kind,
                    related_event_seq,
                    &opts,
                )
            }
            PostsSub::List { limit, kind } => {
                commands::posts::list(&ro_conn()?, limit, kind, &opts)
            }
            PostsSub::Get { id } => commands::posts::get(&ro_conn()?, &id, &opts),
            PostsSub::Tail {
                kind,
                since_id,
                max_rows,
                poll_ms,
            } => commands::posts::tail(&db_path, kind, since_id, max_rows, poll_ms, &opts),
            PostsSub::Approve { request_id, notes } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::posts::decide(&conn, &request_id, true, notes, &opts)
            }
            PostsSub::Deny { request_id, notes } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::posts::decide(&conn, &request_id, false, notes, &opts)
            }
            PostsSub::Pending { limit } => {
                commands::posts::pending(&ro_conn()?, limit, &opts)
            }
        },
        Commands::Sessions { sub } => match sub {
            SessionsSub::New {
                runtime,
                agent_slug,
                title,
                project,
            } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::sessions::new(&conn, runtime, agent_slug, title, project, &opts)
            }
            SessionsSub::List { limit } => {
                commands::sessions::list(&ro_conn()?, limit, &opts)
            }
            SessionsSub::Get { id } => commands::sessions::get(&ro_conn()?, &id, &opts),
            SessionsSub::Delete { id } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::sessions::delete(&conn, &id, &opts)
            }
            SessionsSub::Close {
                id,
                agent_slug,
                model,
                coordinator,
                human_comment,
                force_close_without_context,
            } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::sessions::close(
                    &conn,
                    &id,
                    agent_slug,
                    model,
                    coordinator,
                    human_comment,
                    force_close_without_context,
                    &opts,
                )
            }
            SessionsSub::Reopen { id } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::sessions::reopen(&conn, &id, &opts)
            }
            // v2.15 Wave 4 — team-shared session verbs.
            SessionsSub::Share { id, team } => {
                commands::team_shared::share_resource("sessions", "session_id", &id, &team, &opts)
            }
            SessionsSub::Unshare { id, team } => {
                commands::team_shared::unshare_resource("sessions", &id, &team, &opts)
            }
            SessionsSub::ListShared { team } => {
                commands::team_shared::list_shared("sessions", &team, &opts)
            }
            SessionsSub::AppendEvent { id, team, kind, json, encrypted } => {
                let payload = commands::team_shared::parse_json_arg(&json)?;
                commands::team_shared::append_event(
                    "sessions", &id, &team, &kind, payload, encrypted, &opts,
                )?;
                Ok(())
            }
        },
        Commands::WarRooms { sub } => match sub {
            WarRoomsSub::Close {
                id,
                agent_slug,
                model,
                coordinator,
                human_comment,
                force_close_without_context,
            } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::war_rooms::close(
                    &conn,
                    &id,
                    agent_slug,
                    model,
                    coordinator,
                    human_comment,
                    force_close_without_context,
                    &opts,
                )
            }
            WarRoomsSub::Reopen { id } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::war_rooms::reopen(&conn, &id, &opts)
            }
            WarRoomsSub::Get { id } => commands::war_rooms::get(&ro_conn()?, &id, &opts),
            // v2.15 Wave 4 — team-shared war-room verbs.
            WarRoomsSub::Share { id, team } => {
                commands::team_shared::share_resource(
                    "war-rooms", "war_room_id", &id, &team, &opts,
                )
            }
            WarRoomsSub::Unshare { id, team } => {
                commands::team_shared::unshare_resource("war-rooms", &id, &team, &opts)
            }
            WarRoomsSub::ListShared { team } => {
                commands::team_shared::list_shared("war-rooms", &team, &opts)
            }
            WarRoomsSub::AppendEvent { id, team, kind, json, encrypted } => {
                let payload = commands::team_shared::parse_json_arg(&json)?;
                commands::team_shared::append_event(
                    "war-rooms", &id, &team, &kind, payload, encrypted, &opts,
                )?;
                Ok(())
            }
        },
        Commands::Chats { sub } => match sub {
            ChatsSub::Close {
                id,
                agent_slug,
                model,
                coordinator,
                human_comment,
                force_close_without_context,
            } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::chats::close(
                    &conn,
                    &id,
                    agent_slug,
                    model,
                    coordinator,
                    human_comment,
                    force_close_without_context,
                    &opts,
                )
            }
            ChatsSub::Reopen { id } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::chats::reopen(&conn, &id, &opts)
            }
            ChatsSub::Get { id } => commands::chats::get(&ro_conn()?, &id, &opts),
            // v2.15 Wave 4 — team-shared chat verbs.
            ChatsSub::Share { id, team } => {
                commands::team_shared::share_resource(
                    "chats", "chat_thread_id", &id, &team, &opts,
                )
            }
            ChatsSub::Unshare { id, team } => {
                commands::team_shared::unshare_resource("chats", &id, &team, &opts)
            }
            ChatsSub::ListShared { team } => {
                commands::team_shared::list_shared("chats", &team, &opts)
            }
            ChatsSub::AppendEvent { id, team, kind, json, encrypted } => {
                let payload = commands::team_shared::parse_json_arg(&json)?;
                commands::team_shared::append_event(
                    "chats", &id, &team, &kind, payload, encrypted, &opts,
                )?;
                Ok(())
            }
        },
        Commands::MasterKey { sub } => match sub {
            MasterKeySub::Export { confirm } => commands::master_key::export(confirm, &opts),
        },
        Commands::Bridge {
            session,
            max_rounds,
        } => commands::bridge::run_loop(&session, max_rounds, &db_path, &opts),
        Commands::Ratchet { sub } => match sub {
            RatchetSub::Lock {
                target,
                days,
                threshold,
                notes,
            } => {
                let (kind, value) = commands::ratchet::parse_target(&target)?;
                commands::ratchet::lock(
                    &db_path,
                    &kind,
                    &value,
                    days,
                    threshold,
                    notes.as_deref(),
                    &opts,
                )
            }
            RatchetSub::List => commands::ratchet::list(&db_path, &opts),
            RatchetSub::Check {
                target,
                window_days,
                post_on_fail,
            } => {
                let filter = target.as_deref().map(commands::ratchet::parse_target).transpose()?;
                let ok = commands::ratchet::check(
                    &db_path,
                    filter,
                    window_days,
                    /* emit_events */ true,
                    post_on_fail,
                    &opts,
                )?;
                // CI gate: non-zero exit on any breach so a failed
                // ratchet fails the pipeline step it ran in.
                if !ok {
                    std::process::exit(1);
                }
                Ok(())
            }
            RatchetSub::Status { target } => {
                let filter = target.as_deref().map(commands::ratchet::parse_target).transpose()?;
                commands::ratchet::status(&db_path, filter, &opts)
            }
            RatchetSub::Unlock { target } => {
                let (kind, value) = commands::ratchet::parse_target(&target)?;
                commands::ratchet::unlock(&db_path, &kind, &value, &opts)
            }
        },
        Commands::Review {
            against,
            reviewers,
            out,
            skip_build,
            skip_tests,
            consensus,
            lean,
        } => commands::review::run(
            against.as_deref(),
            reviewers,
            out.as_deref(),
            skip_build,
            skip_tests,
            consensus,
            lean,
            &db_path,
            &opts,
        ),
        Commands::Daemon { sub } => match sub {
            DaemonSub::Start => daemon::start(db_path.clone()),
            DaemonSub::Stop => daemon::stop(),
            DaemonSub::Status => {
                let s = daemon::status()?;
                if opts.human {
                    output::emit_human(&format!(
                        "running: {}\npid:     {}\npeer_id: {}\npubkey:  {}\nport:    {}\nkeys:    {}",
                        s.running,
                        s.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                        s.peer_id,
                        s.public_key_b64,
                        s.port,
                        s.keys_path
                    ));
                } else {
                    output::emit_json(&s)?;
                }
                Ok(())
            }
        },
        Commands::Observe { sub } => match sub {
            ObserveSub::Start { runtimes } => {
                commands::observe::start(&db_path, &runtimes, &opts)
            }
            ObserveSub::Stop => commands::observe::stop(&opts),
            ObserveSub::Status => commands::observe::status(&opts),
        },
        Commands::Mesh { sub } => match sub {
            MeshSub::Discovered { limit } => {
                let rows = daemon::mdns::list_discovered(&db_path)?;
                let truncated: Vec<_> = rows.into_iter().take(limit).collect();
                if opts.human {
                    if truncated.is_empty() {
                        output::emit_human(
                            "No peers discovered yet. Start the daemon (`ato daemon start`) and wait ~10s for mDNS to converge.",
                        );
                    } else {
                        output::emit_human(&format!("{} discovered peer(s):", truncated.len()));
                        for p in &truncated {
                            output::emit_human(&format!(
                                "  {:20}  peer_id={:.16}…  {}  v{}  last_seen={}",
                                p.name,
                                p.peer_id,
                                p.addr,
                                p.version.as_deref().unwrap_or("?"),
                                p.last_seen_at
                            ));
                        }
                    }
                } else {
                    output::emit_json(&truncated)?;
                }
                Ok(())
            }
        },
        Commands::Runtimes { sub } => match sub {
            RuntimesSub::Status { with_quota } => {
                let rows = quota::list_all(&db_path)?;
                // v2.13 Phase 6.x — when --with-quota is set, also surface
                // each runtime's local quota state. Probe results are
                // independent of the rate-limit rows (the latter come from
                // dispatch error parsing; the former come from the runtime's
                // own usage.json), so they ride alongside in the JSON
                // payload rather than being merged.
                let quota_probes = if with_quota {
                    Some(runtime_quota::probe_all())
                } else {
                    None
                };
                if opts.human {
                    if rows.is_empty() {
                        output::emit_human("No quota information captured yet.");
                    } else {
                        output::emit_human(&format!("{} runtime quotas:", rows.len()));
                        let now = chrono::Utc::now();
                        for r in &rows {
                            let active = chrono::DateTime::parse_from_rfc3339(&r.resets_at)
                                .map(|t| t > now)
                                .unwrap_or(false);
                            let tag = if active { "rate-limited" } else { "expired" };
                            output::emit_human(&format!(
                                "  {:12} {} until {}  (source: {})",
                                r.runtime, tag, r.resets_at, r.source
                            ));
                        }
                    }
                    if let Some(probes) = &quota_probes {
                        output::emit_human(&format!(
                            "Runtime quota probes ({} runtime{}):",
                            probes.len(),
                            if probes.len() == 1 { "" } else { "s" },
                        ));
                        for p in probes {
                            if p.found {
                                let used = p
                                    .messages_used
                                    .map(|n| n.to_string())
                                    .unwrap_or_else(|| "?".into());
                                let limit = p
                                    .messages_limit
                                    .map(|n| n.to_string())
                                    .unwrap_or_else(|| "?".into());
                                output::emit_human(&format!(
                                    "  {:10} {} / {} messages{}{}",
                                    p.runtime,
                                    used,
                                    limit,
                                    p.period_reset_at
                                        .as_deref()
                                        .map(|r| format!("  resets {}", r))
                                        .unwrap_or_default(),
                                    p.source_path
                                        .as_deref()
                                        .map(|s| format!("  ({})", s))
                                        .unwrap_or_default(),
                                ));
                            } else {
                                output::emit_human(&format!(
                                    "  {:10} quota unknown{}{}",
                                    p.runtime,
                                    p.source_path
                                        .as_deref()
                                        .map(|s| format!("  (tried {})", s))
                                        .unwrap_or_default(),
                                    p.note
                                        .as_deref()
                                        .map(|n| format!("  — {}", n))
                                        .unwrap_or_default(),
                                ));
                            }
                        }
                    }
                } else if let Some(probes) = quota_probes {
                    // --with-quota opts into the envelope shape. Without
                    // the flag we emit the legacy bare array so every
                    // v2.12-era script keeps working unchanged.
                    output::emit_json(&serde_json::json!({
                        "quotas": rows,
                        "runtime_quota_probes": probes,
                    }))?;
                } else {
                    output::emit_json(&rows)?;
                }
                Ok(())
            }
            RuntimesSub::Health => commands::runtimes::run_health_check(&opts),
            RuntimesSub::TestProviders { slug } => {
                commands::providers::run(&db_path, slug.as_deref(), &opts)
            }
            RuntimesSub::Models { slug, no_cache } => {
                commands::providers::list_models(&db_path, &slug, no_cache, &opts)
            }
            RuntimesSub::AddRemote {
                name,
                host,
                port,
                user,
                key_path,
                runtime,
                binary_path,
                extra_args,
            } => {
                // Accept either `--host user@server` or `--host server
                // --user u`. Normalize so the stored row always has
                // ssh_user separate from host (clearer for `ato
                // runtimes list-remote` output, and avoids quoting
                // surprises when building the ssh command).
                let (effective_user, effective_host) = match (user.as_deref(), host.split_once('@')) {
                    (None, Some((u, h))) => (Some(u.to_string()), h.to_string()),
                    (Some(u), Some((_existing, h))) => (Some(u.to_string()), h.to_string()),
                    (Some(u), None) => (Some(u.to_string()), host.clone()),
                    (None, None) => (None, host.clone()),
                };
                // Default binary_path to the bare runtime name — matches
                // the convention of having the binary on the remote
                // login shell's PATH. Users can override per-row.
                let effective_binary = if binary_path.trim().is_empty() {
                    runtime.clone()
                } else {
                    binary_path.clone()
                };
                let conn = db::open_readwrite(&db_path)?;
                remote_runtime::insert(
                    &conn,
                    &name,
                    &effective_host,
                    port as i64,
                    effective_user.as_deref(),
                    key_path.as_deref(),
                    &runtime,
                    &effective_binary,
                    extra_args.as_deref(),
                )?;
                if opts.human {
                    output::emit_human(&format!(
                        "Registered remote runtime '{}' → ssh {}{} (runtime: {}, binary: {})",
                        name,
                        effective_user
                            .as_deref()
                            .map(|u| format!("{}@", u))
                            .unwrap_or_default(),
                        effective_host,
                        runtime,
                        effective_binary,
                    ));
                    output::emit_human(&format!(
                        "Try: ato dispatch {} \"hello from the laptop\"",
                        name
                    ));
                } else {
                    output::emit_json(&serde_json::json!({ "slug": name, "ok": true }))?;
                }
                Ok(())
            }
            RuntimesSub::ListRemote => {
                let conn = db::open_readonly(&db_path)?;
                let rows = remote_runtime::list(&conn)?;
                if opts.human {
                    if rows.is_empty() {
                        output::emit_human(
                            "No remote runtimes registered. Add one with `ato runtimes add-remote`.",
                        );
                    } else {
                        output::emit_human(&format!("{} remote runtime(s):", rows.len()));
                        for r in &rows {
                            let target = match &r.ssh_user {
                                Some(u) => format!("{}@{}", u, r.host),
                                None => r.host.clone(),
                            };
                            output::emit_human(&format!(
                                "  {:20} ssh {} (port {}) → {} {}",
                                r.slug, target, r.port, r.runtime, r.binary_path
                            ));
                        }
                    }
                } else {
                    output::emit_json(&rows)?;
                }
                Ok(())
            }
            RuntimesSub::RemoveRemote { name } => {
                let conn = db::open_readwrite(&db_path)?;
                let n = remote_runtime::delete(&conn, &name)?;
                if opts.human {
                    if n == 0 {
                        output::emit_human(&format!("No remote runtime named '{}'.", name));
                    } else {
                        output::emit_human(&format!("Removed remote runtime '{}'.", name));
                    }
                } else {
                    output::emit_json(&serde_json::json!({ "slug": name, "deleted": n }))?;
                }
                Ok(())
            }
        },
        Commands::Agents { sub } => {
            let conn = db::open_readwrite(&db_path)?;
            match sub {
                AgentsSub::Create {
                    slug,
                    runtime,
                    display_name,
                    description,
                    model,
                    system_prompt,
                    project_id,
                    from_file,
                } => {
                    if let Some(path) = from_file {
                        commands::agents::create_from_file(
                            &conn,
                            &path,
                            &runtime,
                            slug,
                            display_name,
                            description,
                            model,
                            system_prompt,
                            project_id,
                            &opts,
                        )
                    } else {
                        // Inline form requires explicit slug + system_prompt.
                        // Surface clear errors instead of letting the DB layer
                        // throw cryptic NOT NULL constraint violations.
                        let slug = slug.ok_or_else(|| anyhow::anyhow!(
                            "`--slug` is required unless `--from-file` is set."
                        ))?;
                        if system_prompt.is_none() {
                            return Err(anyhow::anyhow!(
                                "`--system-prompt` is required unless `--from-file` is set."
                            ));
                        }
                        commands::agents::create(
                            &conn,
                            &slug,
                            &runtime,
                            display_name,
                            description,
                            model,
                            system_prompt,
                            project_id,
                            &opts,
                        )
                    }
                }
                AgentsSub::List {
                    runtime,
                    project_id,
                } => commands::agents::list(&conn, runtime, project_id, &opts),
                AgentsSub::Delete {
                    slug,
                    runtime,
                    also_remove_file,
                } => commands::agents::delete(
                    &conn,
                    &slug,
                    runtime,
                    also_remove_file,
                    &opts,
                ),
                AgentsSub::Update {
                    slug,
                    runtime,
                    model,
                    system_prompt,
                    display_name,
                    description,
                    skills,
                    add_skill,
                    remove_skill,
                } => {
                    // Translate the three CLI flags into a single
                    // mutation enum. The flags are mutually exclusive;
                    // multiple at once is a user error worth surfacing.
                    let mutation = match (skills, add_skill, remove_skill) {
                        (Some(list), None, None) => Some(commands::agents::SkillsMutation::Replace(list)),
                        (None, Some(s), None) => Some(commands::agents::SkillsMutation::Add(s)),
                        (None, None, Some(s)) => Some(commands::agents::SkillsMutation::Remove(s)),
                        (None, None, None) => None,
                        _ => return Err(anyhow::anyhow!(
                            "Pass at most one of --skills, --add-skill, --remove-skill."
                        )),
                    };
                    commands::agents::update(
                        &conn,
                        &slug,
                        runtime,
                        model,
                        system_prompt,
                        display_name,
                        description,
                        mutation,
                        &opts,
                    )
                }
            }
        }
    }
}
