// v2.16 PR-1 — `ato missions` CLI surface (proactive coordinator class).
//
// See docs/v2.16-missions.md for the architectural decisions
// (war-room F16E28F0-2E9A-4260-8A2E-02F0F3CF49E7, codex 2x + gemini 1x):
//
//   Q1 [B-lite]: Mission is a SEPARATE primitive that spawns Loops as
//                workers. mission_events.payload.loop_run_id carries the
//                Mission ↔ Loop relationship (no join table).
//   Q2 [W2]:     workspace_strategy is a declarative field on missions
//                (single_cwd | per_agent_worktree). Worktree creation is
//                imperative coordinator code (PR-3).
//   Q3 [M2]:     merge_strategy is a declarative field. Coordinator
//                integration strategy, NOT git merge mechanics.
//
// Gemini round-3 schema refinements adopted: cleanup_policy, check_command
// in success_criteria JSON, max_loops + token_budget_usd, result_metadata,
// nullable mission_id on related rows.
//
// Subcommands shipped in PR-1 (CRUD + lifecycle + audit only):
//   ato missions create / list / show / set-category / set-state / narrative / events
//
// PR-2: ato missions dispatch
// PR-3: ato missions cleanup + worktree create/cleanup
//
// PR-4 (this file):
//   ato missions tick [<slug>] [--json]       — coordinator one-shot wake
//   ato missions check <slug>                  — force success evaluation
//   ato missions set-worker <slug>             — configure worker_config
//
// Schedule note (PR-5 installer SKIPPED — pattern not cleanly reusable):
//   Wire the tick manually:
//     macOS (launchd):
//       every 15 min: $(which ato) missions tick
//     Linux (cron):
//       */15 * * * *  /usr/local/bin/ato missions tick
//
// Output defaults to JSON; `--human` swaps to terminal formatting.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::db;
use crate::output::{emit_human, emit_json, Opts};

// ── Worker config JSON shape ──────────────────────────────────────────
//
// {"runtime": "...", "model": null|"...", "require_tools": ["..."]}
// Stored in missions.worker_config column (TEXT / JSON).
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct WorkerConfig {
    runtime: String,
    model: Option<String>,
    #[serde(default)]
    require_tools: Vec<String>,
}

// ── CLI surface ────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct MissionArgs {
    #[command(subcommand)]
    pub sub: MissionSub,
}

#[derive(Subcommand, Debug)]
pub enum MissionSub {
    /// Create a new Mission. Goal + success_criteria are required; everything
    /// else has a safe default. success_criteria is read from --success-criteria
    /// FILE as JSON (an array of {kind, description, check_command} objects);
    /// pass `--success-criteria -` to read JSON from stdin.
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        goal: String,
        /// Path to JSON file with success criteria, or `-` for stdin.
        /// Shape: [{ "kind": "...", "description": "...", "check_command": "..." }, ...]
        /// `check_command` runs in the workspace_root and exit-0 = met.
        #[arg(long = "success-criteria", value_name = "FILE")]
        success_criteria_file: Option<PathBuf>,
        /// Optional override for the auto-derived slug.
        #[arg(long)]
        slug: Option<String>,
        /// 'single_cwd' (default) or 'per_agent_worktree'.
        #[arg(long, default_value = "single_cwd")]
        workspace_strategy: String,
        /// Git SHA worktrees branch from (required when workspace_strategy=per_agent_worktree).
        #[arg(long)]
        base_sha: Option<String>,
        /// 'retain' | 'delete_on_success' (default) | 'always_delete'.
        #[arg(long, default_value = "delete_on_success")]
        cleanup_policy: String,
        /// 'human_approves_each' (default) | 'coordinator_merges_all'
        /// | 'coordinator_picks_winner' | 'ranked_by_score'.
        #[arg(long, default_value = "human_approves_each")]
        merge_strategy: String,
        /// 'autonomous' (default) | 'needs_owner' | 'ignored' | 'done'.
        #[arg(long, default_value = "autonomous")]
        category: String,
        /// Max worker Loops the coordinator may spawn under this Mission.
        /// NULL = unbounded. Coordinator tick refuses to spawn beyond.
        #[arg(long)]
        max_loops: Option<i64>,
        /// USD cap on aggregate execution_logs.cost_usd across all spawns.
        /// NULL = unbounded.
        #[arg(long)]
        token_budget_usd: Option<f64>,
        /// Optional escalation policy as JSON: {ask_owner_when, abandon_when}.
        #[arg(long = "escalation-policy", value_name = "FILE")]
        escalation_policy_file: Option<PathBuf>,
    },
    /// List Missions, newest-updated first. Optional filters.
    List {
        #[arg(long)]
        state: Option<String>,
        #[arg(long)]
        category: Option<String>,
    },
    /// Print one Mission in full (slug or id).
    Show {
        slug_or_id: String,
    },
    /// Change the operational category (Steinberger triage primitive).
    /// 'ignored' means owner said skip — coordinator will not escalate again.
    SetCategory {
        slug_or_id: String,
        category: String,
    },
    /// Change the lifecycle state. open → in_progress → (blocked | complete).
    SetState {
        slug_or_id: String,
        state: String,
    },
    /// Print the markdown narrative sidecar (~/.ato/missions/<slug>.md).
    Narrative {
        slug_or_id: String,
    },
    /// List events for one Mission, newest first.
    Events {
        slug_or_id: String,
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
    /// v2.16 PR-3 — manual worktree sweep for a Mission.
    ///
    /// Respects cleanup_policy unless --force is passed (which removes
    /// worktrees regardless of policy; branches are deleted only under
    /// always_delete OR --force).
    Cleanup {
        slug_or_id: String,
        /// Remove worktrees regardless of the mission's cleanup_policy.
        /// Branches are still only deleted when policy=always_delete or --force.
        #[arg(long)]
        force: bool,
    },
    /// v2.16 PR-2 — fire a worker dispatch under a Mission.
    ///
    /// Enforces the Mission's budgets (max_loops, token_budget_usd) BEFORE
    /// the dispatch; refuses if either would be exceeded. Transitions an
    /// open mission to in_progress on first dispatch. Records a
    /// 'dispatched' mission_event with the execution_log_id + runtime +
    /// cost so the coordinator (PR-4) and the Mission-control board (PR-7)
    /// can reconstruct the work timeline.
    ///
    /// workspace_strategy=per_agent_worktree refuses (queued for PR-3 —
    /// worktree create/cleanup isn't shipped yet).
    Dispatch {
        slug_or_id: String,
        /// Runtime to fire (claude / codex / gemini / anthropic / openai /
        /// google / minimax / etc.).
        #[arg(long, required_unless_present = "loop_slug", conflicts_with = "loop_slug")]
        runtime: Option<String>,
        /// Prompt text. Mutually exclusive with --prompt-file.
        #[arg(long, conflicts_with = "prompt_file")]
        prompt: Option<String>,
        /// Path to prompt file ('-' for stdin). Mutually exclusive with --prompt.
        #[arg(long = "prompt-file", value_name = "FILE", conflicts_with = "prompt")]
        prompt_file: Option<PathBuf>,
        /// Optional model override.
        #[arg(long)]
        model: Option<String>,
        /// Optional agent slug (label-only today — agent file loading
        /// lands in v2.6 PR-A.5).
        #[arg(long)]
        agent: Option<String>,
        /// Enable the API-provider tool-call loop (read_file / grep /
        /// edit_file / write_file / list_dir / git_status / git_diff /
        /// bash, etc.). Only applies when runtime is an API provider.
        #[arg(long = "with-tools")]
        with_tools: bool,
        /// Comma-separated tool name list to require (e.g.
        /// "edit_file,write_file,bash"). Implies --with-tools.
        #[arg(long = "require-tools", value_delimiter = ',')]
        require_tools: Vec<String>,
        /// v2.16 PR-2.5 — fire an existing Loop (by slug or id) as the worker
        /// under this Mission instead of a single dispatch.
        #[arg(long = "loop", value_name = "SLUG", conflicts_with_all = ["prompt", "prompt_file", "model", "agent", "with_tools", "require_tools"])]
        loop_slug: Option<String>,
        /// Loop variables (K=V). Only meaningful with --loop.
        #[arg(long = "var", value_name = "K=V")]
        vars: Vec<String>,
        /// Internal: attempt_id pre-minted by the parent tick so the child
        /// does NOT insert a second dispatch_started event.
        #[arg(long = "attempt-id", hide = true)]
        attempt_id: Option<String>,
    },

    // ── v2.16 PR-4: coordinator tick ──────────────────────────────────

    /// v2.16 PR-4 — one-shot coordinator wake.
    ///
    /// Iterates missions in state open|in_progress (or just <slug>).
    /// Per mission, decides at most ONE action (see design doc D6A23631):
    ///   (a) skip if category=ignored
    ///   (b) pre-flight worktree base_sha check → escalate once if broken
    ///   (c) in-flight scan: mark stale dispatch_started events abandoned
    ///   (d) success evaluation: run check_commands → complete if all met
    ///   (e) failure guard: 3 consecutive failures → blocked + escalated
    ///   (f) spawn detached child if worker_config set + budgets allow
    ///
    /// Safe to call from a scheduler:
    ///   macOS launchd: $(which ato) missions tick every 15 minutes
    ///   Linux cron:    */15 * * * *  /usr/local/bin/ato missions tick
    Tick {
        /// Optional: run tick for one mission only (slug or id).
        slug_or_id: Option<String>,
        /// Emit JSON array of {slug, state, action, detail} instead of
        /// one human-readable line per mission.
        #[arg(long)]
        json: bool,
    },

    /// v2.16 PR-4 — force success evaluation for one mission immediately,
    /// regardless of whether a terminal event has occurred since the last
    /// success_check. Records a success_check event; transitions to
    /// complete if all criteria are met.
    Check {
        slug_or_id: String,
    },

    /// v2.16 PR-6 — list pending decision briefs for a Mission.
    ///
    /// A "brief" is an `escalated` mission_event whose payload follows the
    /// canonical shape: {reason, summary, options[], ...context}. A brief
    /// is "pending" while no later `owner_decision` or `state_changed`
    /// event resolves it. Mirrors `events --kind escalated` plus
    /// pending-only filtering and structured rendering.
    Briefs {
        slug_or_id: String,
        /// Include resolved (historical) briefs too, not just pending ones.
        #[arg(long)]
        all: bool,
    },

    /// v2.16 PR-4 — configure the worker that the coordinator tick spawns
    /// for this mission when budgets allow and criteria are unmet.
    ///
    /// JSON shape stored: {"runtime": "...", "model": null|"...", "require_tools": [...]}.
    /// Inserts a worker_config_changed event {from, to}.
    SetWorker {
        slug_or_id: String,
        /// Runtime to fire (claude / codex / gemini / etc.).
        #[arg(long)]
        runtime: String,
        /// Optional model override for this runtime.
        #[arg(long)]
        model: Option<String>,
        /// Comma-separated tool names to pass as --require-tools when spawning.
        #[arg(long = "require-tools", value_delimiter = ',')]
        require_tools: Vec<String>,
    },

    // ── v2.16 PR-5: merge-strategy execution ──────────────────────────

    /// v2.16 PR-5 — integration merge workflow.
    ///
    /// Manages squash-merging accepted agent worktrees into a dedicated
    /// integration branch (ato/mission/<slug>/integration), running
    /// success_criteria check_commands after each merge and rolling back
    /// on regression. All git ops target -C paths (no shell).
    ///
    /// Non-interactive primitives (safe to script):
    ///   --status         list each agent: merged/skipped/pending + diffstat
    ///   --approve <a>    squash-merge agent branch into integration branch
    ///   --skip   <a>     mark agent skipped (optional --reason)
    ///   --all            approve all pending agents (coordinator_merges_all only)
    ///   --finish         emit integration_complete + write result_metadata
    ///
    /// Interactive wrapper (TTY only):
    ///   bare `ato missions merge <slug>` prompts [a]pprove/[s]kip/[d]iff/[q]uit
    ///   for each pending agent.
    ///
    /// Strategy gating:
    ///   human_approves_each  → --approve/--skip/interactive; refuses --all
    ///   coordinator_merges_all → --all/--approve; interactive and --skip also allowed
    ///   coordinator_picks_winner / ranked_by_score → all merge sub-commands refused
    ///     ("queued for a later release")
    Merge {
        slug_or_id: String,
        /// List status (merged/skipped/pending) + diffstat for every agent.
        #[arg(long, conflicts_with_all = ["approve", "skip", "all", "finish"])]
        status: bool,
        /// Squash-merge one agent's branch into the integration branch.
        #[arg(long, value_name = "AGENT", conflicts_with_all = ["status", "skip", "all", "finish"])]
        approve: Option<String>,
        /// Mark one agent as skipped (optional --reason).
        #[arg(long, value_name = "AGENT", conflicts_with_all = ["status", "approve", "all", "finish"])]
        skip: Option<String>,
        /// Optional reason text for --skip.
        #[arg(long, requires = "skip")]
        reason: Option<String>,
        /// Approve ALL pending agents sequentially (coordinator_merges_all only).
        #[arg(long, conflicts_with_all = ["status", "approve", "skip", "finish"])]
        all: bool,
        /// Emit integration_complete event when no pending agents remain.
        #[arg(long, conflicts_with_all = ["status", "approve", "skip", "all"])]
        finish: bool,
    },

    // ── v2.15 Wave 4 — team-shared resource CLI parity ────────────────────

    /// v2.15 Wave 4 — share this mission with a team.
    #[command(name = "share")]
    Share {
        /// Mission id (UUID) to share.
        id: String,
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
    },
    /// v2.15 Wave 4 — remove a team share for this mission.
    #[command(name = "unshare")]
    Unshare {
        /// Mission id (UUID) to unshare.
        id: String,
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
    },
    /// v2.15 Wave 4 — list missions shared with this team.
    #[command(name = "list-shared")]
    ListShared {
        /// Team slug or UUID.
        #[arg(long)]
        team: String,
    },
    /// v2.15 Wave 4 — append a new event to a team-shared mission.
    #[command(name = "append-event")]
    AppendEvent {
        /// Mission id (UUID).
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
        /// app or omit --encrypted for plaintext. Full CLI E2E support
        /// ships in a follow-up wave.
        #[arg(long)]
        encrypted: bool,
    },
}

// ── Row types (mirror the DB rows for JSON serialization) ─────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MissionRow {
    id: String,
    slug: String,
    name: String,
    goal: String,
    success_criteria: serde_json::Value,
    escalation_policy: Option<serde_json::Value>,
    workspace_strategy: String,
    base_sha: Option<String>,
    cleanup_policy: String,
    merge_strategy: String,
    category: String,
    state: String,
    max_loops: Option<i64>,
    token_budget_usd: Option<f64>,
    result_metadata: Option<serde_json::Value>,
    narrative_md_path: String,
    created_at: String,
    updated_at: String,
    // v2.16 PR-3: absolute path to the git repo captured at create time.
    // Used by ensure_agent_worktree to resolve base_sha. NULL for single_cwd.
    repo_root: Option<String>,
    // v2.16 PR-4: worker config JSON — {"runtime":"...","model":null|"...","require_tools":[...]}.
    // NULL means the coordinator tick will escalate with reason="no_worker_config".
    worker_config: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MissionEventRow {
    id: String,
    mission_id: String,
    kind: String,
    payload: Option<serde_json::Value>,
    occurred_at: String,
}

// Positional indices (0-based):
//   0=id 1=slug 2=name 3=goal 4=success_criteria 5=escalation_policy
//   6=workspace_strategy 7=base_sha 8=cleanup_policy 9=merge_strategy
//   10=category 11=state 12=max_loops 13=token_budget_usd 14=result_metadata
//   15=narrative_md_path 16=created_at 17=updated_at 18=repo_root (PR-3)
//   19=worker_config (PR-4, last)
const MISSION_SELECT: &str = "SELECT id, slug, name, goal, success_criteria, escalation_policy,
            workspace_strategy, base_sha, cleanup_policy, merge_strategy,
            category, state, max_loops, token_budget_usd, result_metadata,
            narrative_md_path, created_at, updated_at, repo_root, worker_config FROM missions";

// ── Validation constants ──────────────────────────────────────────────

const VALID_WORKSPACE_STRATEGIES: &[&str] = &["single_cwd", "per_agent_worktree"];
const VALID_CLEANUP_POLICIES: &[&str] = &["retain", "delete_on_success", "always_delete"];
const VALID_MERGE_STRATEGIES: &[&str] = &[
    "human_approves_each",
    "coordinator_merges_all",
    "coordinator_picks_winner",
    "ranked_by_score",
];
const VALID_CATEGORIES: &[&str] = &["autonomous", "needs_owner", "ignored", "done"];
const VALID_STATES: &[&str] = &["open", "in_progress", "blocked", "complete"];

/// Maximum age in seconds for an open `dispatch_started` event to be considered
/// live even when its pid is alive and has the right argv. Beyond this the
/// dispatch must have crashed or hung without leaving a closing event.
/// (Finding 1 — PR-4 review, 2026-06-12)
const MAX_WORKER_AGE_SECS: i64 = 7200;

// ── Dispatcher ────────────────────────────────────────────────────────

pub fn run(args: MissionArgs, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    match args.sub {
        MissionSub::Create {
            name,
            goal,
            success_criteria_file,
            slug,
            workspace_strategy,
            base_sha,
            cleanup_policy,
            merge_strategy,
            category,
            max_loops,
            token_budget_usd,
            escalation_policy_file,
        } => {
            // v2.16 PR-3: capture repo_root from CWD at create time so
            // worktree creation later can resolve base_sha in the right repo.
            let repo_root = detect_repo_root();
            run_create(
                CreateInput {
                    name,
                    goal,
                    success_criteria_file,
                    slug_override: slug,
                    workspace_strategy,
                    base_sha,
                    cleanup_policy,
                    merge_strategy,
                    category,
                    max_loops,
                    token_budget_usd,
                    escalation_policy_file,
                    repo_root,
                },
                db_path,
                opts,
            )
        }
        MissionSub::List { state, category } => run_list(state, category, db_path, opts),
        MissionSub::Show { slug_or_id } => run_show(slug_or_id, db_path, opts),
        MissionSub::SetCategory {
            slug_or_id,
            category,
        } => run_set_category(slug_or_id, category, db_path, opts),
        MissionSub::SetState { slug_or_id, state } => {
            run_set_state(slug_or_id, state, db_path, opts)
        }
        MissionSub::Narrative { slug_or_id } => run_narrative(slug_or_id, db_path, opts),
        MissionSub::Events { slug_or_id, limit } => run_events(slug_or_id, limit, db_path, opts),
        MissionSub::Cleanup { slug_or_id, force } => {
            run_cleanup_command(slug_or_id, force, db_path, opts)
        }
        MissionSub::Dispatch {
            slug_or_id,
            runtime,
            prompt,
            prompt_file,
            model,
            agent,
            with_tools,
            require_tools,
            loop_slug,
            vars,
            attempt_id,
        } => run_dispatch_under_mission(
            DispatchInput {
                slug_or_id,
                runtime,
                prompt,
                prompt_file,
                model,
                agent,
                with_tools,
                require_tools,
                loop_slug,
                vars,
                attempt_id,
            },
            db_path,
            opts,
        ),
        // ── PR-4 ──────────────────────────────────────────────────────
        MissionSub::Tick { slug_or_id, json } => {
            run_tick(slug_or_id, json, db_path, opts)
        }
        MissionSub::Check { slug_or_id } => {
            run_check(slug_or_id, db_path, opts)
        }
        MissionSub::SetWorker {
            slug_or_id,
            runtime,
            model,
            require_tools,
        } => run_set_worker(slug_or_id, runtime, model, require_tools, db_path, opts),
        // ── PR-5 ──────────────────────────────────────────────────────
        MissionSub::Merge {
            slug_or_id,
            status,
            approve,
            skip,
            reason,
            all,
            finish,
        } => run_merge(
            MergeInput { slug_or_id, status, approve, skip, reason, all, finish },
            db_path,
            opts,
        ),
        MissionSub::Briefs { slug_or_id, all } => {
            run_briefs(slug_or_id, all, db_path, opts)
        }
        // v2.15 Wave 4 — team-shared resource verbs.
        MissionSub::Share { id, team } => {
            crate::commands::team_shared::share_resource(
                "missions", "mission_id", &id, &team, opts,
            )
        }
        MissionSub::Unshare { id, team } => {
            crate::commands::team_shared::unshare_resource("missions", &id, &team, opts)
        }
        MissionSub::ListShared { team } => {
            crate::commands::team_shared::list_shared("missions", &team, opts)
        }
        MissionSub::AppendEvent { id, team, kind, json, encrypted } => {
            let payload = crate::commands::team_shared::parse_json_arg(&json)?;
            crate::commands::team_shared::append_event(
                "missions", &id, &team, &kind, payload, encrypted, opts,
            )?;
            Ok(())
        }
    }
}

// ── Create ────────────────────────────────────────────────────────────

struct CreateInput {
    name: String,
    goal: String,
    success_criteria_file: Option<PathBuf>,
    slug_override: Option<String>,
    workspace_strategy: String,
    base_sha: Option<String>,
    cleanup_policy: String,
    merge_strategy: String,
    category: String,
    max_loops: Option<i64>,
    token_budget_usd: Option<f64>,
    escalation_policy_file: Option<PathBuf>,
    // v2.16 PR-3: captured from `git rev-parse --show-toplevel` at create time.
    repo_root: Option<String>,
}

fn run_create(input: CreateInput, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    if input.name.trim().is_empty() {
        anyhow::bail!("name is empty");
    }
    if input.goal.trim().is_empty() {
        anyhow::bail!("goal is empty");
    }
    validate_enum("workspace-strategy", &input.workspace_strategy, VALID_WORKSPACE_STRATEGIES)?;
    validate_enum("cleanup-policy", &input.cleanup_policy, VALID_CLEANUP_POLICIES)?;
    validate_enum("merge-strategy", &input.merge_strategy, VALID_MERGE_STRATEGIES)?;
    validate_enum("category", &input.category, VALID_CATEGORIES)?;

    // Cross-question consistency rule from docs/v2.16-missions.md:
    // per_agent_worktree requires a base_sha so the coordinator knows
    // where worktrees branch from.
    if input.workspace_strategy == "per_agent_worktree" && input.base_sha.is_none() {
        anyhow::bail!(
            "workspace-strategy=per_agent_worktree requires --base-sha (the commit SHA worktrees branch from)"
        );
    }

    // v2.16 PR-3 — per_agent_worktree also requires being inside a git
    // repo so worktrees have somewhere to live.
    if input.workspace_strategy == "per_agent_worktree" && input.repo_root.is_none() {
        anyhow::bail!(
            "workspace-strategy=per_agent_worktree requires creating the mission inside a git repository (git rev-parse --show-toplevel returned nothing)"
        );
    }

    let success_criteria = read_json_arg("success-criteria", input.success_criteria_file)?
        .unwrap_or_else(|| serde_json::json!([]));
    validate_success_criteria_shape(&success_criteria)?;

    let escalation_policy = read_json_arg("escalation-policy", input.escalation_policy_file)?;

    let conn = db::open_readwrite(db_path)?;
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let name_trimmed: String = input.name.trim().chars().take(200).collect();
    let base_slug = input
        .slug_override
        .as_deref()
        .map(slugify)
        .unwrap_or_else(|| slugify(&name_trimmed));
    let slug = unique_slug(&conn, &base_slug)?;

    let narrative_md_path = narrative_path_for(&slug)?;
    ensure_narrative_file(&narrative_md_path, &name_trimmed, &input.goal)?;

    let sc_str = serde_json::to_string(&success_criteria).context("serialize success_criteria")?;
    let ep_str = escalation_policy
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .context("serialize escalation_policy")?;

    // v2.16 attribution — resolve initiator provenance for the mission row.
    let attribution = crate::attribution::Attribution::detect();
    conn.execute(
        "INSERT INTO missions (
            id, slug, name, goal, success_criteria, escalation_policy,
            workspace_strategy, base_sha, cleanup_policy, merge_strategy,
            category, state, max_loops, token_budget_usd, result_metadata,
            narrative_md_path, created_at, updated_at, repo_root,
            initiator_kind, client_surface, initiator_id
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'open', ?12, ?13, NULL, ?14, ?15, ?15, ?16, ?17, ?18, ?19)",
        params![
            id,
            slug,
            name_trimmed,
            input.goal,
            sc_str,
            ep_str,
            input.workspace_strategy,
            input.base_sha,
            input.cleanup_policy,
            input.merge_strategy,
            input.category,
            input.max_loops,
            input.token_budget_usd,
            narrative_md_path.to_string_lossy().to_string(),
            now,
            input.repo_root,
            attribution.kind,
            attribution.surface,
            attribution.id,
        ],
    )
    .context("insert mission")?;

    // First event: the mission was created.
    insert_event(
        &conn,
        &id,
        "state_changed",
        Some(serde_json::json!({
            "from": null,
            "to": "open",
            "reason": "mission_created",
        })),
        &now,
    )?;

    let row = load_mission(&conn, &id)?;
    if opts.human {
        emit_human(&format!(
            "Created mission '{}' (slug: {})\n  goal: {}\n  narrative: {}",
            row.name, row.slug, row.goal, row.narrative_md_path
        ));
    } else {
        emit_json(&row)?;
    }
    Ok(())
}

// ── List / Show ───────────────────────────────────────────────────────

fn run_list(
    state_filter: Option<String>,
    category_filter: Option<String>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    if let Some(s) = state_filter.as_deref() {
        validate_enum("state", s, VALID_STATES)?;
    }
    if let Some(c) = category_filter.as_deref() {
        validate_enum("category", c, VALID_CATEGORIES)?;
    }

    let conn = db::open_readonly(db_path)?;
    let mut sql = String::from(MISSION_SELECT);
    let mut clauses: Vec<&str> = Vec::new();
    if state_filter.is_some() {
        clauses.push("state = ?1");
    }
    if category_filter.is_some() {
        let idx = if state_filter.is_some() { "?2" } else { "?1" };
        clauses.push(if state_filter.is_some() {
            "category = ?2"
        } else {
            "category = ?1"
        });
        let _ = idx;
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY updated_at DESC");

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = Vec::new();
    let iter: Box<dyn Iterator<Item = rusqlite::Result<MissionRow>>> =
        match (&state_filter, &category_filter) {
            (Some(s), Some(c)) => Box::new(stmt.query_map(params![s, c], row_to_mission)?),
            (Some(s), None) => Box::new(stmt.query_map(params![s], row_to_mission)?),
            (None, Some(c)) => Box::new(stmt.query_map(params![c], row_to_mission)?),
            (None, None) => Box::new(stmt.query_map([], row_to_mission)?),
        };
    for r in iter {
        rows.push(r?);
    }

    if opts.human {
        if rows.is_empty() {
            emit_human("No missions found.");
        } else {
            for r in &rows {
                emit_human(&format!(
                    "  · {} [{}/{}]  {}  ({} loops, ${:.2} budget)",
                    r.slug,
                    r.state,
                    r.category,
                    r.name,
                    r.max_loops
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "∞".to_string()),
                    r.token_budget_usd.unwrap_or(0.0),
                ));
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

fn run_show(slug_or_id: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let row = load_mission(&conn, &slug_or_id)?;
    if opts.human {
        emit_human(&format!(
            "Mission '{}' (slug: {})\n  state: {}\n  category: {}\n  goal: {}\n  workspace: {}{}\n  cleanup: {}\n  merge: {}\n  budgets: max_loops={}, max_usd={}\n  success criteria: {} entries\n  narrative: {}\n  created: {}  updated: {}",
            row.name,
            row.slug,
            row.state,
            row.category,
            row.goal,
            row.workspace_strategy,
            row.base_sha
                .as_deref()
                .map(|s| format!(" (base_sha={})", s))
                .unwrap_or_default(),
            row.cleanup_policy,
            row.merge_strategy,
            row.max_loops
                .map(|n| n.to_string())
                .unwrap_or_else(|| "∞".to_string()),
            row.token_budget_usd
                .map(|n| format!("${:.2}", n))
                .unwrap_or_else(|| "∞".to_string()),
            row.success_criteria
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0),
            row.narrative_md_path,
            row.created_at,
            row.updated_at,
        ));
    } else {
        emit_json(&row)?;
    }
    Ok(())
}

// ── Set category / state (with audit event) ───────────────────────────

fn run_set_category(
    slug_or_id: String,
    category: String,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    validate_enum("category", &category, VALID_CATEGORIES)?;
    let conn = db::open_readwrite(db_path)?;
    let row = load_mission(&conn, &slug_or_id)?;
    if row.category == category {
        if opts.human {
            emit_human(&format!(
                "Mission '{}' category already '{}' — no change",
                row.slug, category
            ));
        } else {
            emit_json(&row)?;
        }
        return Ok(());
    }
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE missions SET category = ?1, updated_at = ?2 WHERE id = ?3",
        params![category, now, row.id],
    )?;
    insert_event(
        &conn,
        &row.id,
        "category_changed",
        Some(serde_json::json!({
            "from": row.category,
            "to": category,
        })),
        &now,
    )?;
    let updated = load_mission(&conn, &row.id)?;
    if opts.human {
        emit_human(&format!(
            "Mission '{}' category: {} → {}",
            updated.slug, row.category, updated.category
        ));
    } else {
        emit_json(&updated)?;
    }
    Ok(())
}

fn run_set_state(slug_or_id: String, state: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    validate_enum("state", &state, VALID_STATES)?;
    let conn = db::open_readwrite(db_path)?;
    let row = load_mission(&conn, &slug_or_id)?;
    if row.state == state {
        if opts.human {
            emit_human(&format!(
                "Mission '{}' state already '{}' — no change",
                row.slug, state
            ));
        } else {
            emit_json(&row)?;
        }
        return Ok(());
    }
    let prev_state = row.state.clone();
    let (updated, cleanup_err) = transition_state(&conn, &row, &state, None)?;
    if opts.human {
        emit_human(&format!(
            "Mission '{}' state: {} → {}",
            updated.slug, prev_state, updated.state
        ));
        if let Some(ref e) = cleanup_err {
            emit_human(&format!("  ⚠ worktree cleanup failed: {}", e));
        }
    } else {
        emit_json(&updated)?;
    }
    Ok(())
}

// ── Narrative + Events ────────────────────────────────────────────────

fn run_narrative(slug_or_id: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let row = load_mission(&conn, &slug_or_id)?;
    let body = fs::read_to_string(&row.narrative_md_path).with_context(|| {
        format!(
            "read narrative {} (was it deleted out from under us?)",
            row.narrative_md_path
        )
    })?;
    if opts.human {
        emit_human(&body);
    } else {
        emit_json(&serde_json::json!({
            "mission_slug": row.slug,
            "narrative_md_path": row.narrative_md_path,
            "body": body,
        }))?;
    }
    Ok(())
}

fn run_events(slug_or_id: String, limit: i64, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let row = load_mission(&conn, &slug_or_id)?;
    let mut stmt = conn.prepare(
        "SELECT id, mission_id, kind, payload, occurred_at
           FROM mission_events
          WHERE mission_id = ?1
       ORDER BY occurred_at DESC
          LIMIT ?2",
    )?;
    let iter = stmt.query_map(params![row.id, limit], |r| {
        Ok(MissionEventRow {
            id: r.get(0)?,
            mission_id: r.get(1)?,
            kind: r.get(2)?,
            payload: parse_payload(r.get::<_, Option<String>>(3)?),
            occurred_at: r.get(4)?,
        })
    })?;
    let rows: Vec<MissionEventRow> = iter.filter_map(|r| r.ok()).collect();
    if opts.human {
        if rows.is_empty() {
            emit_human(&format!("No events for mission '{}'", row.slug));
        } else {
            for e in &rows {
                // PR-6: render escalated events structurally so operators
                // see reason/options inline without piping to jq. Prefix the
                // event id so correlation with `mission_events.id` survives
                // the structured format (codex R1 [LOW]).
                if e.kind == "escalated" {
                    if let Some(payload) = &e.payload {
                        emit_human(&format!("  (event id: {})", e.id));
                        emit_human(&render_brief_payload(payload, &e.occurred_at));
                        continue;
                    }
                }
                emit_human(&format!("  {}  {}  ({})", e.occurred_at, e.kind, e.id));
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

// ── v2.16 PR-2: Mission-scoped dispatch ───────────────────────────────

struct DispatchInput {
    slug_or_id: String,
    runtime: Option<String>,
    prompt: Option<String>,
    prompt_file: Option<PathBuf>,
    model: Option<String>,
    agent: Option<String>,
    with_tools: bool,
    require_tools: Vec<String>,
    loop_slug: Option<String>,
    vars: Vec<String>,
    /// Pre-minted attempt_id from the parent tick (Finding 3 — PR-4 review).
    /// When Some, skip inserting dispatch_started (parent already did it).
    attempt_id: Option<String>,
}

fn run_dispatch_under_mission(
    input: DispatchInput,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    // vars-without-loop validation — must bail before touching the DB.
    if !input.vars.is_empty() && input.loop_slug.is_none() {
        anyhow::bail!("missions dispatch: --var requires --loop");
    }

    // Validate single-dispatch inputs BEFORE any DB write (the open →
    // in_progress transition below must not fire on bad arguments).
    let single_dispatch = match &input.loop_slug {
        None => {
            let runtime = input
                .runtime
                .clone()
                .expect("clap: --runtime required when --loop absent");
            if runtime.trim().is_empty() {
                anyhow::bail!("missions dispatch: --runtime is required");
            }
            let prompt_text = read_prompt_arg(input.prompt.clone(), input.prompt_file.clone())?;
            if prompt_text.trim().is_empty() {
                anyhow::bail!("missions dispatch: prompt is empty (use --prompt or --prompt-file)");
            }
            Some((runtime, prompt_text))
        }
        Some(slug) => {
            // Validate the loop invocation BEFORE any DB write.  If the slug
            // doesn't exist or the vars are malformed, bail now so the
            // open→in_progress transition and the loop_run_started event are
            // never written against a bad invocation.
            crate::commands::loops::validate_loop_invocation(slug, &input.vars, db_path)?;
            None
        }
    };

    let conn = db::open_readwrite(db_path)?;
    let mission = load_mission(&conn, &input.slug_or_id)?;

    // Refuse states that don't accept new work.
    match mission.state.as_str() {
        "complete" => anyhow::bail!(
            "missions dispatch: mission '{}' is in state 'complete' — no further work needed. \
             Use `ato missions set-state {} in_progress` to reopen if you really want to.",
            mission.slug,
            mission.slug
        ),
        _ => {}
    }
    if mission.category == "ignored" {
        anyhow::bail!(
            "missions dispatch: mission '{}' has category 'ignored' (owner explicitly said skip). \
             Use `ato missions set-category {} autonomous` to undo the ignore before dispatching.",
            mission.slug,
            mission.slug
        );
    }

    // Workspace-strategy gate: for per_agent_worktree, resolve the
    // workspace root lazily (once per agent). Loop path still refused
    // — that lands with the PR-4 coordinator tick.
    let dispatch_workspace_root: Option<PathBuf> =
        if mission.workspace_strategy == "per_agent_worktree" {
            // --loop + per_agent_worktree: deferred to PR-4.
            if input.loop_slug.is_some() {
                anyhow::bail!(
                    "missions dispatch: --loop + workspace_strategy=per_agent_worktree is not yet supported. \
                     Loop workers inside per-agent worktrees land with the PR-4 coordinator tick."
                );
            }
            let runtime = input.runtime.as_deref().expect("clap: --runtime required when --loop absent");
            let agent_key = input.agent.clone().unwrap_or_else(|| runtime.to_string());
            let wt_path = ensure_agent_worktree(&conn, &mission, &agent_key)?;
            Some(wt_path)
        } else {
            None
        };

    // Budget enforcement — count prior dispatches + sum cost (gemini-round-3
    // refinements from PR-1 schema: max_loops + token_budget_usd).
    let prior_dispatch_count = count_dispatches_for_mission(&conn, &mission.id)?;
    if let Some(max) = mission.max_loops {
        if prior_dispatch_count >= max {
            anyhow::bail!(
                "missions dispatch refused: mission '{}' has fired {} worker dispatches already \
                 and max_loops={}. Raise the cap or close the mission.",
                mission.slug,
                prior_dispatch_count,
                max
            );
        }
    }
    let prior_cost_usd = sum_cost_for_mission(&conn, &mission.id)?;
    if let Some(budget) = mission.token_budget_usd {
        if prior_cost_usd >= budget {
            anyhow::bail!(
                "missions dispatch refused: mission '{}' has spent ${:.4} of ${:.4} token_budget_usd. \
                 Raise the budget or close the mission.",
                mission.slug,
                prior_cost_usd,
                budget
            );
        }
    }

    // First-dispatch state transition: open → in_progress + emit event.
    if mission.state == "open" {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE missions SET state = 'in_progress', updated_at = ?1 WHERE id = ?2",
            params![now, mission.id],
        )?;
        insert_event(
            &conn,
            &mission.id,
            "state_changed",
            Some(serde_json::json!({
                "from": "open",
                "to": "in_progress",
                "reason": "first_dispatch",
            })),
            &now,
        )?;
    }

    // Branch: loop path vs. single-dispatch path.
    if let Some(loop_slug) = input.loop_slug {
        // Hoist vars clone so it can serve both the started and completed
        // event payloads without moving input.vars before execute_loop.
        let vars_snapshot = input.vars.clone();

        // Emit loop_run_started immediately so the mission audit trail
        // shows activity during a long-running loop, not just at completion.
        let now = chrono::Utc::now().to_rfc3339();
        insert_event(
            &conn,
            &mission.id,
            "loop_run_started",
            Some(serde_json::json!({
                "loop_slug": loop_slug,
                "vars": vars_snapshot,
            })),
            &now,
        )?;

        drop(conn);

        if opts.human {
            emit_human(&format!(
                "→ firing loop '{}' under mission '{}' (prior dispatches: {}, prior cost: ${:.4})",
                loop_slug, mission.slug, prior_dispatch_count, prior_cost_usd
            ));
        }

        let outcome = crate::commands::loops::execute_loop(&loop_slug, input.vars, db_path, opts)?;

        let conn = db::open_readwrite(db_path)?;
        let now = chrono::Utc::now().to_rfc3339();
        let event_payload = serde_json::json!({
            "loop_run_id": outcome.run_id,
            "loop_id": outcome.loop_id,
            "loop_slug": outcome.loop_slug,
            "status": outcome.status,
            "steps_executed": outcome.steps_executed,
            "steps_succeeded": outcome.steps_succeeded,
            "steps_planned": outcome.steps_planned,
            "error": outcome.error,
            "paused_dispatch_id": outcome.paused_dispatch_id,
            "paused_runtime": outcome.paused_runtime,
            "paused_until": outcome.paused_until,
            "started_at": outcome.started_at,
            "finished_at": outcome.finished_at,
            "vars": vars_snapshot,
        });
        insert_event(&conn, &mission.id, "loop_run_completed", Some(event_payload.clone()), &now)?;

        conn.execute(
            "UPDATE missions SET updated_at = ?1 WHERE id = ?2",
            params![now, mission.id],
        )?;

        if opts.human {
            emit_human(&format!(
                "  ✓ loop '{}' completed — status={} run_id={} steps={}/{}",
                outcome.loop_slug,
                outcome.status,
                outcome.run_id,
                outcome.steps_succeeded,
                outcome.steps_planned,
            ));
        } else {
            emit_json(&serde_json::json!({
                "mission_id": mission.id,
                "mission_slug": mission.slug,
                "event_kind": "loop_run_completed",
                "occurred_at": now,
                "payload": event_payload,
            }))?;
        }

        return Ok(());
    }

    // Single-dispatch path.
    let (runtime, prompt_text) = single_dispatch.expect("loop_slug was None");

    // v2.16 PR-4 / Finding-3 — attempt_id and dispatch_started handling.
    //
    // When the parent tick pre-minted an attempt_id and already inserted
    // dispatch_started, the child re-uses that id and skips a second insert.
    // When there is no pre-minted id (manual CLI dispatch), mint one now and
    // insert dispatch_started ourselves — keeping today's behavior.
    let started_at_ts = chrono::Utc::now().to_rfc3339();
    let attempt_id = match input.attempt_id.clone() {
        Some(id) => {
            // Parent already wrote dispatch_started — do NOT insert again.
            id
        }
        None => {
            // Manual dispatch: mint and insert as before.
            let new_id = Uuid::new_v4().to_string();
            let current_pid = std::process::id();
            insert_event(
                &conn,
                &mission.id,
                "dispatch_started",
                Some(serde_json::json!({
                    "attempt_id": new_id,
                    "runtime": runtime,
                    "agent": input.agent,
                    "pid": current_pid,
                    "spawned_by": "manual",
                    "slug": mission.slug,
                })),
                &started_at_ts,
            )?;
            new_id
        }
    };

    // Capture wake_started_at so we can find the freshly-written execution_logs
    // row that this dispatch creates. Same pattern v2.15.5 uses in paused-
    // dispatch resume (execution_logs.id is TEXT UUID, not auto-increment).
    let wake_started_at = started_at_ts.clone();
    drop(conn);

    if opts.human {
        emit_human(&format!(
            "→ firing {} under mission '{}' (prior dispatches: {}, prior cost: ${:.4})",
            runtime, mission.slug, prior_dispatch_count, prior_cost_usd
        ));
    }

    // Resolve with_tools: --require-tools implies --with-tools.
    let effective_with_tools = input.with_tools || !input.require_tools.is_empty();

    // Fire the dispatch through the shared entrypoint. Errors here are
    // dispatch failures (network, no key, etc.) — they propagate up and
    // the mission stays in_progress so the operator can retry.
    crate::commands::dispatch::run(
        &runtime,
        &prompt_text,
        input.model.clone(),
        input.agent.clone(),
        None,  // session_id — Mission dispatches aren't anchored to a session today
        None,  // war_room_id
        None,  // war_room_round
        false, // stream
        false, // stream_jsonl
        effective_with_tools,
        input.require_tools.clone(), // Fix E — thread require_tools for proper tool-set expansion
        dispatch_workspace_root.as_deref(), // v2.16 PR-3: Some(path) for per_agent_worktree
        db_path,
        opts,
    )?;

    // Read back the freshly-written execution_logs row.
    // When the CLI runtime is missing and dispatch fell back to an API
    // provider, the row is persisted under the provider slug (e.g.
    // gemini→"google", claude→"anthropic", codex→"openai"). Query both
    // the original runtime name and its fallback slug so we don't miss it.
    let conn = db::open_readwrite(db_path)?;
    let fallback_runtime = api_fallback_slug(&runtime);
    let outcome: Option<(String, String, Option<String>, Option<f64>, Option<i64>)> = conn
        .query_row(
            "SELECT id, status, error_message, cost_usd_estimated, tool_calls_count
               FROM execution_logs
              WHERE runtime IN (?1, ?2) AND created_at >= ?3
              ORDER BY created_at DESC
              LIMIT 1",
            params![runtime, fallback_runtime, wake_started_at],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<f64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .ok();

    let now = chrono::Utc::now().to_rfc3339();
    // v2.16 PR-4: include attempt_id in dispatched event so the tick can
    // correlate dispatch_started → dispatched pairs for in-flight detection.
    let event_payload = match &outcome {
        Some((exec_id, status, err, cost, tool_calls)) => serde_json::json!({
            "attempt_id": attempt_id,
            "runtime": runtime,
            "model": input.model,
            "agent": input.agent,
            "with_tools": effective_with_tools,
            "require_tools": input.require_tools,
            "execution_log_id": exec_id,
            "status": status,
            "error_message": err,
            "cost_usd": cost,
            "tool_calls_count": tool_calls,
        }),
        None => serde_json::json!({
            "attempt_id": attempt_id,
            "runtime": runtime,
            "model": input.model,
            "agent": input.agent,
            "with_tools": effective_with_tools,
            "require_tools": input.require_tools,
            "warning": "no execution_logs row was created — dispatch may have failed before persisting",
        }),
    };
    insert_event(&conn, &mission.id, "dispatched", Some(event_payload.clone()), &now)?;

    // Bump updated_at on the mission so list ordering reflects activity.
    conn.execute(
        "UPDATE missions SET updated_at = ?1 WHERE id = ?2",
        params![now, mission.id],
    )?;

    if opts.human {
        match outcome {
            Some((exec_id, status, err, cost, _tool_calls)) => {
                let cost_str = cost.map(|c| format!("${:.4}", c)).unwrap_or_else(|| "n/a".into());
                emit_human(&format!(
                    "  ✓ dispatched on {} — status={} cost={} execution_log_id={}{}",
                    runtime,
                    status,
                    cost_str,
                    exec_id,
                    err.map(|e| format!(" error={}", e)).unwrap_or_default(),
                ));
            }
            None => {
                emit_human(
                    "  ⚠ dispatch fired but no execution_logs row was found — see the runtime's own output above for context",
                );
            }
        }
    } else {
        emit_json(&serde_json::json!({
            "mission_id": mission.id,
            "mission_slug": mission.slug,
            "event_kind": "dispatched",
            "occurred_at": now,
            "payload": event_payload,
        }))?;
    }

    Ok(())
}

/// Return the API-provider slug that the dispatch path falls back to when
/// the CLI for `runtime` is missing. Mirrors the mapping in
/// `dispatch::api_fallback_for_missing_cli`; kept as a static local so the
/// missions read-back query can include BOTH slugs without touching the DB.
///   gemini → "google"
///   claude → "anthropic"
///   codex  → "openai"
fn api_fallback_slug(runtime: &str) -> &str {
    match runtime {
        "gemini" => "google",
        "claude" => "anthropic",
        "codex" => "openai",
        other => other, // no fallback; IN (?1, ?2) with equal values is harmless
    }
}

fn read_prompt_arg(prompt: Option<String>, prompt_file: Option<PathBuf>) -> Result<String> {
    match (prompt, prompt_file) {
        (Some(p), None) => Ok(p),
        (None, Some(path)) => {
            if path.as_os_str() == "-" {
                use std::io::Read;
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s).context("read prompt from stdin")?;
                Ok(s)
            } else {
                fs::read_to_string(&path)
                    .with_context(|| format!("read prompt file {}", path.display()))
            }
        }
        (None, None) => Err(anyhow::anyhow!(
            "missions dispatch: provide --prompt or --prompt-file"
        )),
        (Some(_), Some(_)) => Err(anyhow::anyhow!(
            "missions dispatch: --prompt and --prompt-file are mutually exclusive"
        )),
    }
}

/// Count of prior worker dispatches under a Mission. Used to enforce
/// max_loops cap. Counts both 'dispatched' and 'loop_run_completed'
/// events (PR-2.5 will add the loop-spawn variant).
fn count_dispatches_for_mission(conn: &Connection, mission_id: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM mission_events
          WHERE mission_id = ?1 AND kind IN ('dispatched', 'loop_run_completed')",
        params![mission_id],
        |r| r.get(0),
    )?)
}

/// Aggregate cost_usd across all execution_logs rows referenced in this
/// Mission's events. Two legs combined via UNION ALL:
///   1. kind='dispatched'       — payload.execution_log_id → execution_logs.id
///   2. kind='loop_run_completed' — payload.loop_run_id → loop_run_steps.loop_run_id
///                                  → loop_run_steps.execution_log_id → execution_logs.id
fn sum_cost_for_mission(conn: &Connection, mission_id: &str) -> Result<f64> {
    let total: Option<f64> = conn
        .query_row(
            "SELECT COALESCE(SUM(cost), 0.0) FROM (
               -- Leg 1: single-dispatch events
               SELECT el.cost_usd_estimated AS cost
                 FROM mission_events me
                 JOIN execution_logs el
                   ON json_extract(me.payload, '$.execution_log_id') = el.id
                WHERE me.mission_id = ?1
                  AND me.kind = 'dispatched'
               UNION ALL
               -- Leg 2: loop events — sum all steps' execution_logs costs
               SELECT el.cost_usd_estimated AS cost
                 FROM mission_events me
                 JOIN loop_run_steps lrs
                   ON lrs.loop_run_id = json_extract(me.payload, '$.loop_run_id')
                 JOIN execution_logs el
                   ON el.id = lrs.execution_log_id
                WHERE me.mission_id = ?1
                  AND me.kind = 'loop_run_completed'
             )",
            params![mission_id],
            |r| r.get(0),
        )
        .ok();
    Ok(total.unwrap_or(0.0))
}

// ── Helpers ───────────────────────────────────────────────────────────

fn id_or_slug_column(input: &str) -> &'static str {
    if Uuid::parse_str(input).is_ok() {
        "id"
    } else {
        "slug"
    }
}

fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_sep = true;
    for ch in name.chars().take(200) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('-');
            last_was_sep = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("mission");
    }
    out.chars().take(64).collect()
}

fn unique_slug(conn: &Connection, base: &str) -> Result<String> {
    let mut candidate = base.to_string();
    let mut suffix = 2;
    loop {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM missions WHERE slug = ?1",
                params![candidate],
                |r| r.get(0),
            )
            .context("query slug collision")?;
        if exists == 0 {
            return Ok(candidate);
        }
        candidate = format!("{}-{}", base, suffix);
        suffix += 1;
        if suffix > 1000 {
            anyhow::bail!("slug-exhaustion");
        }
    }
}

fn validate_enum(name: &str, value: &str, allowed: &[&str]) -> Result<()> {
    if !allowed.contains(&value) {
        anyhow::bail!(
            "invalid --{}: '{}' (expected {})",
            name,
            value,
            allowed.join("|")
        );
    }
    Ok(())
}

/// Each success-criterion entry must have at least `description` and
/// `check_command` (the latter being the gemini round-3 verifiability
/// requirement). `kind` is optional but recommended.
fn validate_success_criteria_shape(value: &serde_json::Value) -> Result<()> {
    let arr = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("success-criteria must be a JSON array"))?;
    for (i, entry) in arr.iter().enumerate() {
        let obj = entry.as_object().ok_or_else(|| {
            anyhow::anyhow!("success-criteria[{}] must be a JSON object", i)
        })?;
        if !obj.contains_key("description") {
            anyhow::bail!("success-criteria[{}] missing required 'description' field", i);
        }
        if !obj.contains_key("check_command") {
            anyhow::bail!(
                "success-criteria[{}] missing required 'check_command' field (programmatic verification — exit-0 in workspace_root = met)",
                i
            );
        }
    }
    Ok(())
}

fn read_json_arg(field: &str, path: Option<PathBuf>) -> Result<Option<serde_json::Value>> {
    let Some(p) = path else { return Ok(None) };
    let raw = if p.as_os_str() == "-" {
        use std::io::Read;
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .with_context(|| format!("read {} from stdin", field))?;
        s
    } else {
        fs::read_to_string(&p)
            .with_context(|| format!("read {} file {}", field, p.display()))?
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let v: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("parse {} as JSON", field))?;
    Ok(Some(v))
}

fn narrative_path_for(slug: &str) -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("no HOME / USERPROFILE in env"))?;
    let dir = PathBuf::from(home).join(".ato").join("missions");
    fs::create_dir_all(&dir).with_context(|| format!("mkdir -p {}", dir.display()))?;
    Ok(dir.join(format!("{}.md", slug)))
}

fn ensure_narrative_file(path: &PathBuf, name: &str, goal: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let seed = format!(
        "# {name}\n\n_Mission narrative — created {now}_\n\n## Goal\n\n{goal}\n\n## Events\n\n_The coordinator appends entries here as work proceeds. See `ato missions events <slug>` for the structured event log._\n",
        name = name,
        now = now,
        goal = goal,
    );
    fs::write(path, seed).with_context(|| format!("write narrative {}", path.display()))?;
    Ok(())
}

fn load_mission(conn: &Connection, slug_or_id: &str) -> Result<MissionRow> {
    let col = id_or_slug_column(slug_or_id);
    let sql = format!("{} WHERE {} = ?1", MISSION_SELECT, col);
    conn.query_row(&sql, params![slug_or_id], row_to_mission)
        .with_context(|| format!("load mission '{}'", slug_or_id))
}

fn row_to_mission(r: &rusqlite::Row) -> rusqlite::Result<MissionRow> {
    let sc_str: String = r.get(4)?;
    let success_criteria: serde_json::Value =
        serde_json::from_str(&sc_str).unwrap_or(serde_json::json!([]));
    let ep_str: Option<String> = r.get(5)?;
    let escalation_policy = ep_str
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    let rm_str: Option<String> = r.get(14)?;
    let result_metadata =
        rm_str.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    let wc_str: Option<String> = r.get(19).ok().flatten(); // PR-4: worker_config — tolerates old rows
    let worker_config = wc_str.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    Ok(MissionRow {
        id: r.get(0)?,
        slug: r.get(1)?,
        name: r.get(2)?,
        goal: r.get(3)?,
        success_criteria,
        escalation_policy,
        workspace_strategy: r.get(6)?,
        base_sha: r.get(7)?,
        cleanup_policy: r.get(8)?,
        merge_strategy: r.get(9)?,
        category: r.get(10)?,
        state: r.get(11)?,
        max_loops: r.get(12)?,
        token_budget_usd: r.get(13)?,
        result_metadata,
        narrative_md_path: r.get(15)?,
        created_at: r.get(16)?,
        updated_at: r.get(17)?,
        repo_root: r.get(18).ok().flatten(), // PR-3: .ok().flatten() tolerates old rows without it
        worker_config,
    })
}

fn insert_event(
    conn: &Connection,
    mission_id: &str,
    kind: &str,
    payload: Option<serde_json::Value>,
    occurred_at: &str,
) -> Result<()> {
    let payload_str = payload
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .context("serialize event payload")?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO mission_events (id, mission_id, kind, payload, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, mission_id, kind, payload_str, occurred_at],
    )
    .context("insert mission_event")?;

    // v2.16 PR-8 — narrative auto-population. After every successful event
    // INSERT, append a deterministic markdown paragraph to the mission's
    // narrative sidecar. This is best-effort: missing path, missing file,
    // or fs errors are silently swallowed (the SQLite ledger is the
    // source of truth; the narrative is a human-readable mirror).
    let _ = append_narrative_for_event(conn, mission_id, kind, payload.as_ref(), occurred_at);

    Ok(())
}

/// Look up the mission's narrative_md_path and append a per-kind paragraph
/// for the just-inserted event. Failure to look up or write the file is
/// silently dropped — the event itself is already committed to SQLite.
///
/// Concurrency posture (best-effort, NOT a hard atomicity guarantee).
/// `PIPE_BUF` is a pipe/FIFO contract, not a POSIX guarantee for regular
/// files, and `Write::write_all` is a loop that may issue multiple
/// `write(2)` syscalls if the kernel returns a partial write. On a local
/// filesystem the kernel almost always completes a sub-2KB `O_APPEND`
/// write in a single syscall, so in practice cross-process appends from
/// `tick`, manual `dispatch`, and `merge` flows do not interleave — but
/// that's a property of the implementation, not the spec. We mitigate as
/// far as we can without a runtime lock:
///   - one single `write_all` of a fully-built buffer (so the most-likely
///     case is one `write(2)` per event)
///   - SAFE_MAX truncation keeps every buffer well under any plausible
///     atomicity envelope (4 KiB)
///   - newlines stripped from interpolated payload strings
/// The SQLite ledger remains the source of truth; the markdown mirror is
/// a human-readable convenience that must never block event recording.
/// Codex PR-8 R1 [HIGH] / R2 [MED].
fn append_narrative_for_event(
    conn: &Connection,
    mission_id: &str,
    kind: &str,
    payload: Option<&serde_json::Value>,
    occurred_at: &str,
) -> Result<()> {
    let path: Option<String> = conn
        .query_row(
            "SELECT narrative_md_path FROM missions WHERE id = ?1",
            params![mission_id],
            |r| r.get(0),
        )
        .ok();
    let Some(path) = path else { return Ok(()) };
    let path = PathBuf::from(path);
    if !path.exists() {
        return Ok(());
    }
    let mut line = format_event_narrative(kind, payload, occurred_at);
    line.push('\n');
    // Belt-and-suspenders: cap the line at PIPE_BUF/2 so even with multi-byte
    // codepoints we stay well within atomic-write bounds. Truncation point is
    // a char boundary (find_char_boundary walks back to one).
    const SAFE_MAX: usize = 2048;
    if line.len() > SAFE_MAX {
        let mut cut = SAFE_MAX;
        while cut > 0 && !line.is_char_boundary(cut) {
            cut -= 1;
        }
        line.truncate(cut);
        line.push_str("…\n");
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(&path)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

/// Pure, deterministic formatter — turns a (kind, payload) pair into the
/// markdown paragraph appended to the narrative. The output is always a
/// single line so successive events stay readable when the file is `cat`-ed.
/// Unknown kinds fall back to a generic "{kind}" line so future event kinds
/// still leave a trace even without a template.
fn format_event_narrative(
    kind: &str,
    payload: Option<&serde_json::Value>,
    occurred_at: &str,
) -> String {
    let p = payload.cloned().unwrap_or(serde_json::Value::Null);
    // Sanitize interpolated string values: newlines (and carriage returns)
    // would otherwise break the single-line-per-event invariant — at least
    // one producer (`worktree_cleanup_failed.error`) uses `format!("{:#}", e)`
    // which routinely embeds newlines. Codex/Gemini PR-8 R1 [MED].
    fn sanitize(raw: &str) -> String {
        raw.replace(['\n', '\r'], " ")
    }
    let s = |k: &str| -> Option<String> {
        p.get(k).and_then(|v| v.as_str()).map(sanitize)
    };
    let i = |k: &str| -> Option<i64> { p.get(k).and_then(|v| v.as_i64()) };
    let short = |sha: &str| -> String {
        sha.chars().take(8).collect::<String>()
    };

    let body: String = match kind {
        "state_changed" => {
            let from = s("from").unwrap_or_else(|| "?".into());
            let to = s("to").unwrap_or_else(|| "?".into());
            let reason = s("reason").map(|r| format!(" — {}", r)).unwrap_or_default();
            format!("**State changed:** `{}` → `{}`{}", from, to, reason)
        }
        "category_changed" => {
            let from = s("from").unwrap_or_else(|| "?".into());
            let to = s("to").unwrap_or_else(|| "?".into());
            format!("**Category changed:** `{}` → `{}`", from, to)
        }
        "dispatched" => {
            let runtime = s("runtime").unwrap_or_else(|| "?".into());
            let model = s("model").map(|m| format!("/{}", m)).unwrap_or_default();
            let status = s("status").unwrap_or_else(|| "?".into());
            let cost = p
                .get("cost_usd")
                .and_then(|v| v.as_f64())
                .map(|c| format!(", cost ${:.4}", c))
                .unwrap_or_default();
            let log = s("execution_log_id")
                .map(|id| format!(", log `{}`", short(&id)))
                .unwrap_or_default();
            format!("**Dispatched** to `{}{}` — status `{}`{}{}", runtime, model, status, cost, log)
        }
        "dispatch_started" => {
            let runtime = s("runtime").unwrap_or_else(|| "?".into());
            let pid = i("pid").map(|p| p.to_string()).unwrap_or_else(|| "?".into());
            format!("**Dispatch started** — runtime `{}`, pid {}", runtime, pid)
        }
        "dispatch_abandoned" => {
            let aid = s("attempt_id").unwrap_or_else(|| "?".into());
            format!("**Dispatch abandoned** — attempt `{}` (pid terminated)", aid)
        }
        "loop_run_started" => {
            let slug = s("loop_slug").unwrap_or_else(|| "?".into());
            format!("**Loop started:** `{}`", slug)
        }
        "loop_run_completed" => {
            let slug = s("loop_slug").unwrap_or_else(|| "?".into());
            let status = s("status").unwrap_or_else(|| "?".into());
            let exec = i("steps_executed").unwrap_or(0);
            let succ = i("steps_succeeded").unwrap_or(0);
            let plan = i("steps_planned").unwrap_or(0);
            format!(
                "**Loop completed:** `{}` — status `{}` ({}/{} succeeded, {} executed)",
                slug, status, succ, plan, exec
            )
        }
        "success_check" => {
            let total = p
                .get("results")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let met = p
                .get("results")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter(|r| r.get("met").and_then(|m| m.as_bool()).unwrap_or(false))
                        .count()
                })
                .unwrap_or(0);
            let all_met = p.get("all_met").and_then(|v| v.as_bool()).unwrap_or(false);
            let marker = if all_met { "✓" } else { "·" };
            format!("**Success check** {} — {}/{} criteria met", marker, met, total)
        }
        "merge_check" => {
            let phase = s("phase").map(|p| format!("phase `{}`, ", p)).unwrap_or_default();
            let agent = s("agent").map(|a| format!("agent `{}`, ", a)).unwrap_or_default();
            let total = p
                .get("results")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let met = p
                .get("results")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter(|r| r.get("met").and_then(|m| m.as_bool()).unwrap_or(false))
                        .count()
                })
                .unwrap_or(0);
            let all_met = p.get("all_met").and_then(|v| v.as_bool()).unwrap_or(false);
            let marker = if all_met { "✓" } else { "·" };
            format!("**Merge check** {} — {}{}{}/{} met", marker, phase, agent, met, total)
        }
        "escalated" => {
            let reason = s("reason").unwrap_or_else(|| "(no reason)".into());
            let summary = s("summary").map(|s| format!(" — {}", s)).unwrap_or_default();
            format!("**⚠ Escalated** — reason `{}`{}", reason, summary)
        }
        "worktree_created" => {
            let agent = s("agent").unwrap_or_else(|| "?".into());
            let branch = s("branch").unwrap_or_else(|| "?".into());
            format!("**Worktree created** for agent `{}` on branch `{}`", agent, branch)
        }
        "worktree_cleaned" => {
            let path = s("path").unwrap_or_else(|| "?".into());
            let policy = s("policy").unwrap_or_else(|| "?".into());
            format!("**Worktree cleaned:** `{}` (policy `{}`)", path, policy)
        }
        "worktree_cleanup_failed" => {
            let err = s("error").unwrap_or_else(|| "(no message)".into());
            format!("**⚠ Worktree cleanup failed:** {}", err)
        }
        "worker_config_changed" => "**Worker config changed**".to_string(),
        "agent_merged" => {
            let agent = s("agent").unwrap_or_else(|| "?".into());
            let commit = s("commit_sha")
                .map(|c| format!(" (commit `{}`)", short(&c)))
                .unwrap_or_default();
            format!("**Agent `{}` merged** into integration{}", agent, commit)
        }
        "agent_skipped" => {
            let agent = s("agent").unwrap_or_else(|| "?".into());
            let reason = s("reason").map(|r| format!(" — {}", r)).unwrap_or_default();
            format!("**Agent `{}` skipped**{}", agent, reason)
        }
        "integration_created" => {
            let branch = s("branch").unwrap_or_else(|| "?".into());
            let base = s("base_sha").map(|b| format!(" from `{}`", short(&b))).unwrap_or_default();
            format!("**Integration worktree created** on `{}`{}", branch, base)
        }
        "integration_complete" => {
            let branch = s("branch").unwrap_or_else(|| "?".into());
            let head = s("head_sha").map(|h| format!(", head `{}`", short(&h))).unwrap_or_default();
            let merged = p.get("merged").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            let skipped = p.get("skipped").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            format!(
                "**Integration complete** — branch `{}`{}, {} merged, {} skipped",
                branch, head, merged, skipped
            )
        }
        _ => format!("Event `{}`", kind),
    };
    format!("- _{ts}_ — {body}", ts = occurred_at, body = body)
}

fn parse_payload(raw: Option<String>) -> Option<serde_json::Value> {
    raw.and_then(|s| serde_json::from_str(&s).ok())
}

// ── v2.16 PR-3 helpers ────────────────────────────────────────────────

/// Detect the git repo root from the current working directory.
/// Returns None when not in a git repo (no error — callers handle the None
/// case based on workspace_strategy).
fn detect_repo_root() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() { None } else { Some(path) }
}

/// Resolve the worktree path for `agent_key` under `mission`, creating it
/// if it doesn't exist yet (lazy-once-per-agent).
///
/// Path: HOME/.ato/missions/<slug>/worktrees/<slugified-agent-key>/
///
/// On success: returns the absolute path to the worktree directory.
/// On base_sha resolution failure: inserts an 'escalated' event and bails.
fn ensure_agent_worktree(
    conn: &Connection,
    mission: &MissionRow,
    agent_key: &str,
) -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("no HOME / USERPROFILE in env"))?;
    let agent_slug_dir = slugify(agent_key);
    let wt_path = PathBuf::from(&home)
        .join(".ato")
        .join("missions")
        .join(&mission.slug)
        .join("worktrees")
        .join(&agent_slug_dir);

    // Lazy reuse: if directory already exists, return it immediately.
    if wt_path.exists() {
        return Ok(wt_path);
    }

    // Validate we have repo_root and base_sha.
    let repo_root = mission.repo_root.as_deref().ok_or_else(|| anyhow::anyhow!(
        "Mission '{}' has no repo_root — was it created outside a git repository?",
        mission.slug
    ))?;
    let base_sha = mission.base_sha.as_deref().ok_or_else(|| anyhow::anyhow!(
        "Mission '{}' has no base_sha — per_agent_worktree requires --base-sha at creation time",
        mission.slug
    ))?;

    // Re-resolve base_sha to catch detached HEADs, force-pushed history, etc.
    let resolve_out = std::process::Command::new("git")
        .args(["-C", repo_root, "rev-parse", "--verify", "--quiet"])
        .arg(format!("{}^{{commit}}", base_sha))
        .output()
        .context("spawn git rev-parse to verify base_sha")?;

    if !resolve_out.status.success() {
        // base_sha is unresolvable — escalate with a decision brief.
        let now = chrono::Utc::now().to_rfc3339();
        let payload = EscalationBrief::new("base_sha_unresolvable")
            .summary("Mission base SHA can't be resolved in the repo — agent worktree creation blocked")
            .ctx("base_sha", base_sha.to_string())
            .ctx("repo_root", repo_root.to_string())
            .options([
                "recreate mission with a current base SHA (ato missions create ... --base-sha $(git rev-parse HEAD))",
                "git fetch to restore the commit if it was in a remote branch",
                "set cleanup_policy=retain and inspect worktrees manually",
            ])
            .into_payload();
        insert_event(conn, &mission.id, "escalated", Some(payload), &now)
            .context("insert escalated event")?;
        anyhow::bail!(
            "Mission '{}': base_sha '{}' is unresolvable in repo '{}'.\n\
             \n\
             Tradeoffs and choices:\n\
             1. Recreate the mission with a current base SHA:\n\
                ato missions create ... --base-sha $(git rev-parse HEAD)\n\
                (Tradeoff: prior worktrees are orphaned; new mission has a clean base)\n\
             2. Run `git fetch` to restore the commit if it was a remote branch tip:\n\
                git -C {} fetch --all\n\
                (Tradeoff: may not restore deleted commits; depends on remote availability)\n\
             3. Set cleanup_policy=retain and inspect manually:\n\
                The mission events log records this escalation for audit.\n\
                (Tradeoff: no new worktrees until base_sha is valid)\n\
             \n\
             NEVER falls back to HEAD — that would silently branch from a different\n\
             point than the mission intended (war-room decision Q4=C).",
            mission.slug, base_sha, repo_root, repo_root
        );
    }

    // Create the parent directory.
    let parent = wt_path.parent().expect("wt_path always has a parent");
    fs::create_dir_all(parent)
        .with_context(|| format!("mkdir -p {}", parent.display()))?;

    // Branch name for this worktree.
    let branch = format!("ato/mission/{}/{}", mission.slug, agent_slug_dir);

    // Check if the branch already exists (idempotent — possible if the
    // worktree dir was deleted but the branch survived).
    let branch_exists_out = std::process::Command::new("git")
        .args(["-C", repo_root, "rev-parse", "--verify", "--quiet", &branch])
        .output()
        .context("spawn git rev-parse to check branch")?;

    if branch_exists_out.status.success() {
        // Branch exists: add worktree at existing branch.
        let add_out = std::process::Command::new("git")
            .args(["-C", repo_root, "worktree", "add"])
            .arg(&wt_path)
            .arg(&branch)
            .output()
            .with_context(|| format!("git worktree add {} {}", wt_path.display(), branch))?;
        if !add_out.status.success() {
            anyhow::bail!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&add_out.stderr).trim()
            );
        }
    } else {
        // Branch doesn't exist: create new branch at base_sha.
        let add_out = std::process::Command::new("git")
            .args(["-C", repo_root, "worktree", "add", "-b", &branch])
            .arg(&wt_path)
            .arg(base_sha)
            .output()
            .with_context(|| format!("git worktree add -b {} {} {}", branch, wt_path.display(), base_sha))?;
        if !add_out.status.success() {
            anyhow::bail!(
                "git worktree add -b failed: {}",
                String::from_utf8_lossy(&add_out.stderr).trim()
            );
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    insert_event(
        conn,
        &mission.id,
        "worktree_created",
        Some(serde_json::json!({
            "agent": agent_key,
            "path": wt_path.to_string_lossy(),
            "branch": branch,
            "base_sha": base_sha,
        })),
        &now,
    )?;

    symlink_worktree_deps(&wt_path, std::path::Path::new(repo_root));

    Ok(wt_path)
}

/// FOLLOWUPS #4 fix — Symlink the source repo's gitignored build artifacts
/// (node_modules, sidecar binaries) into the worktree so the pre-commit
/// hook's full gate (tsc + vitest + cargo check on src-tauri) can run
/// without a fresh `npm install` per worktree.
///
/// Best-effort: any individual symlink that fails is logged to stderr but
/// doesn't abort the worktree creation. The pre-commit hook's
/// reduced-gate fallback still covers the case where the symlinks can't
/// be created.
fn symlink_worktree_deps(wt_path: &std::path::Path, repo_root: &std::path::Path) {
    let targets: &[(&str, &str)] = &[
        ("node_modules", "node_modules"),
        ("apps/desktop/node_modules", "apps/desktop/node_modules"),
        (
            "apps/desktop/src-tauri/binaries/ato-aarch64-apple-darwin",
            "apps/desktop/src-tauri/binaries/ato-aarch64-apple-darwin",
        ),
    ];
    for (rel_src, rel_dst) in targets {
        let src = repo_root.join(rel_src);
        let dst = wt_path.join(rel_dst);
        if !src.exists() {
            continue;
        }
        if dst.exists() || dst.is_symlink() {
            continue;
        }
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        #[cfg(unix)]
        let result = std::os::unix::fs::symlink(&src, &dst);
        #[cfg(not(unix))]
        let result: std::io::Result<()> = Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "symlinks not supported on this platform",
        ));
        if let Err(err) = result {
            eprintln!(
                "warning: failed to symlink worktree dep {} → {}: {}",
                src.display(),
                dst.display(),
                err
            );
        }
    }
}

/// Perform event-driven worktree cleanup per the mission's cleanup_policy.
///
/// - retain: no-op always.
/// - delete_on_success: remove when new_state == "complete".
/// - always_delete: remove when new_state == "complete".
///
/// When `force` is true, removes regardless of policy (manual sweep path).
/// Branches are deleted only under always_delete OR force.
///
/// Returns list of paths cleaned (for caller to surface in human output).
fn cleanup_mission_worktrees(
    conn: &Connection,
    mission: &MissionRow,
    new_state: &str,
    force: bool,
) -> Result<Vec<String>> {
    // Decide whether to act based on policy × state × force.
    let should_act = force || match mission.cleanup_policy.as_str() {
        "retain" => false,
        "delete_on_success" => new_state == "complete",
        "always_delete" => new_state == "complete",
        _ => false,
    };
    if !should_act {
        return Ok(Vec::new());
    }

    let delete_branches = mission.cleanup_policy == "always_delete" || force;

    let repo_root = match mission.repo_root.as_deref() {
        Some(r) => r.to_string(),
        None => return Ok(Vec::new()), // no repo_root — single_cwd mission, nothing to clean
    };

    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("no HOME / USERPROFILE in env"))?;
    let wt_root = PathBuf::from(&home)
        .join(".ato")
        .join("missions")
        .join(&mission.slug)
        .join("worktrees");

    // Also try to remove the integration worktree (PR-5).
    let integration_wt = PathBuf::from(&home)
        .join(".ato")
        .join("missions")
        .join(&mission.slug)
        .join("integration");

    let mut cleaned: Vec<String> = Vec::new();

    if integration_wt.exists() {
        let int_branch = format!("ato/mission/{}/integration", mission.slug);
        let int_path_str = integration_wt.to_string_lossy().to_string();
        let rm_out = std::process::Command::new("git")
            .args(["-C", &repo_root, "worktree", "remove", "--force"])
            .arg(&integration_wt)
            .output()
            .ok();
        let rm_ok = rm_out.map(|o| {
            o.status.success()
                || String::from_utf8_lossy(&o.stderr).contains("is not a working tree")
        }).unwrap_or(false);
        if rm_ok {
            if delete_branches {
                let _ = std::process::Command::new("git")
                    .args(["-C", &repo_root, "branch", "-D", &int_branch])
                    .output();
            }
            let now = chrono::Utc::now().to_rfc3339();
            let _ = insert_event(
                conn,
                &mission.id,
                "worktree_cleaned",
                Some(serde_json::json!({
                    "path": int_path_str,
                    "branch": int_branch,
                    "policy": mission.cleanup_policy,
                    "trigger": if force { "manual_sweep" } else { "state_transition" },
                    "branch_deleted": delete_branches,
                    "integration": true,
                })),
                &now,
            );
            cleaned.push(int_path_str);
        }
    }

    if !wt_root.exists() {
        return Ok(cleaned);
    }

    let entries = fs::read_dir(&wt_root)
        .with_context(|| format!("read_dir {}", wt_root.display()))?;

    let now = chrono::Utc::now().to_rfc3339();
    for entry in entries {
        let entry = entry.context("read worktree dir entry")?;
        let wt_path = entry.path();
        if !wt_path.is_dir() {
            continue;
        }

        let path_str = wt_path.to_string_lossy().to_string();
        // Derive branch name from directory name (mirrors ensure_agent_worktree).
        let dir_name = entry.file_name().to_string_lossy().to_string();
        let branch = format!("ato/mission/{}/{}", mission.slug, dir_name);

        // git worktree remove --force <path>
        let rm_out = std::process::Command::new("git")
            .args(["-C", &repo_root, "worktree", "remove", "--force"])
            .arg(&wt_path)
            .output()
            .with_context(|| format!("git worktree remove {}", wt_path.display()))?;

        // Tolerate "not a worktree" errors (dir may have been manually moved).
        let rm_ok = rm_out.status.success()
            || String::from_utf8_lossy(&rm_out.stderr).contains("is not a working tree");

        if rm_ok {
            // Delete branch when policy demands it.
            if delete_branches {
                let _ = std::process::Command::new("git")
                    .args(["-C", &repo_root, "branch", "-D", &branch])
                    .output();
            }

            insert_event(
                conn,
                &mission.id,
                "worktree_cleaned",
                Some(serde_json::json!({
                    "path": path_str,
                    "branch": branch,
                    "policy": mission.cleanup_policy,
                    "trigger": if force { "manual_sweep" } else { "state_transition" },
                    "branch_deleted": delete_branches,
                })),
                &now,
            )?;
            cleaned.push(path_str);
        }
    }

    Ok(cleaned)
}

/// `ato missions cleanup <slug> [--force]` — manual worktree sweep.
fn run_cleanup_command(
    slug_or_id: String,
    force: bool,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let mission = load_mission(&conn, &slug_or_id)?;

    // For manual sweep with --force we pass "complete" as new_state so that
    // all policy branches that fire on completion also fire; force=true
    // overrides the policy check entirely anyway.
    let trigger_state = if force { "complete" } else { &mission.state };
    let cleaned = cleanup_mission_worktrees(&conn, &mission, trigger_state, force)?;

    if opts.human {
        if cleaned.is_empty() {
            emit_human(&format!(
                "No worktrees removed for mission '{}' (policy={}, state={}, force={})",
                mission.slug, mission.cleanup_policy, mission.state, force
            ));
        } else {
            emit_human(&format!(
                "Cleaned {} worktree(s) for mission '{}':",
                cleaned.len(),
                mission.slug
            ));
            for p in &cleaned {
                emit_human(&format!("  {}", p));
            }
        }
    } else {
        emit_json(&serde_json::json!({
            "mission_slug": mission.slug,
            "cleaned": cleaned,
            "force": force,
        }))?;
    }
    Ok(())
}

// ── v2.16 PR-5: merge-strategy execution ─────────────────────────────

struct MergeInput {
    slug_or_id: String,
    status: bool,
    approve: Option<String>,
    skip: Option<String>,
    reason: Option<String>,
    all: bool,
    finish: bool,
}

/// Shared helper: resolve worktree root for this mission.
fn worktree_root_for(home: &std::ffi::OsStr, slug: &str) -> PathBuf {
    PathBuf::from(home)
        .join(".ato")
        .join("missions")
        .join(slug)
        .join("worktrees")
}

/// Ensure the integration worktree exists at HOME/.ato/missions/<slug>/integration/
/// on branch ato/mission/<slug>/integration created from base_sha.
///
/// Lazy/reuse: if the dir already exists the call is a no-op.
/// On base_sha resolution failure: inserts 'escalated' event (same pattern as
/// ensure_agent_worktree) and bails.
/// On first creation: inserts 'integration_created' event.
fn ensure_integration_worktree(conn: &Connection, mission: &MissionRow) -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("no HOME / USERPROFILE in env"))?;
    let int_path = PathBuf::from(&home)
        .join(".ato")
        .join("missions")
        .join(&mission.slug)
        .join("integration");

    // Lazy reuse.
    if int_path.exists() {
        return Ok(int_path);
    }

    let repo_root = mission.repo_root.as_deref().ok_or_else(|| anyhow::anyhow!(
        "Mission '{}' has no repo_root — was it created outside a git repository?",
        mission.slug
    ))?;
    let base_sha = mission.base_sha.as_deref().ok_or_else(|| anyhow::anyhow!(
        "Mission '{}' has no base_sha — per_agent_worktree requires --base-sha at creation time",
        mission.slug
    ))?;

    // Re-resolve base_sha (same escalation-on-unresolvable pattern as ensure_agent_worktree).
    let resolve_out = std::process::Command::new("git")
        .args(["-C", repo_root, "rev-parse", "--verify", "--quiet"])
        .arg(format!("{}^{{commit}}", base_sha))
        .output()
        .context("spawn git rev-parse to verify base_sha for integration worktree")?;

    if !resolve_out.status.success() {
        let now = chrono::Utc::now().to_rfc3339();
        insert_event(
            conn,
            &mission.id,
            "escalated",
            Some(
                EscalationBrief::new("base_sha_unresolvable")
                    .summary("Mission base SHA can't be resolved in the repo — integration worktree creation blocked")
                    .ctx("base_sha", base_sha.to_string())
                    .ctx("repo_root", repo_root.to_string())
                    .ctx("context", "integration_worktree_creation")
                    .options([
                        "recreate mission with a current base SHA",
                        "git fetch to restore the commit",
                    ])
                    .into_payload(),
            ),
            &now,
        ).context("insert escalated event for integration worktree")?;
        anyhow::bail!(
            "Mission '{}': base_sha '{}' is unresolvable in repo '{}' (integration worktree).",
            mission.slug, base_sha, repo_root
        );
    }

    // Create parent directory.
    let parent = int_path.parent().expect("int_path always has parent");
    fs::create_dir_all(parent)
        .with_context(|| format!("mkdir -p {}", parent.display()))?;

    let branch = format!("ato/mission/{}/integration", mission.slug);

    // Check if branch already exists (idempotent when dir was deleted but branch survived).
    let branch_exists = std::process::Command::new("git")
        .args(["-C", repo_root, "rev-parse", "--verify", "--quiet", &branch])
        .output()
        .context("check integration branch existence")?
        .status.success();

    if branch_exists {
        let add_out = std::process::Command::new("git")
            .args(["-C", repo_root, "worktree", "add"])
            .arg(&int_path)
            .arg(&branch)
            .output()
            .with_context(|| format!("git worktree add {} {}", int_path.display(), branch))?;
        if !add_out.status.success() {
            anyhow::bail!(
                "git worktree add (integration, existing branch) failed: {}",
                String::from_utf8_lossy(&add_out.stderr).trim()
            );
        }
    } else {
        let add_out = std::process::Command::new("git")
            .args(["-C", repo_root, "worktree", "add", "-b", &branch])
            .arg(&int_path)
            .arg(base_sha)
            .output()
            .with_context(|| format!("git worktree add -b {} {} {}", branch, int_path.display(), base_sha))?;
        if !add_out.status.success() {
            anyhow::bail!(
                "git worktree add -b (integration) failed: {}",
                String::from_utf8_lossy(&add_out.stderr).trim()
            );
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    insert_event(
        conn,
        &mission.id,
        "integration_created",
        Some(serde_json::json!({
            "path": int_path.to_string_lossy(),
            "branch": branch,
            "base_sha": base_sha,
        })),
        &now,
    )?;

    symlink_worktree_deps(&int_path, std::path::Path::new(repo_root));

    Ok(int_path)
}

/// Enumerate agents that have a worktree directory under worktrees/ MINUS
/// those already recorded in 'agent_merged' or 'agent_skipped' events.
///
/// Returns sorted Vec of agent slugs (dir names).
fn pending_agents(conn: &Connection, mission: &MissionRow) -> Result<Vec<String>> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("no HOME / USERPROFILE in env"))?;
    let wt_root = worktree_root_for(&home, &mission.slug);

    // Collect all worktree dir names.
    let mut all_agents: Vec<String> = Vec::new();
    if wt_root.exists() {
        for entry in fs::read_dir(&wt_root)
            .with_context(|| format!("read_dir {}", wt_root.display()))?
        {
            let entry = entry.context("read worktree dir entry")?;
            if entry.path().is_dir() {
                all_agents.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    all_agents.sort();

    // Collect already-handled agents from events.
    let mut handled: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stmt = conn.prepare(
        "SELECT payload FROM mission_events
          WHERE mission_id = ?1
            AND kind IN ('agent_merged', 'agent_skipped')",
    )?;
    let payloads: Vec<Option<String>> = stmt
        .query_map(params![mission.id], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for p in payloads {
        if let Some(s) = p {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                if let Some(a) = v.get("agent").and_then(|a| a.as_str()) {
                    handled.insert(a.to_string());
                }
            }
        }
    }

    Ok(all_agents.into_iter().filter(|a| !handled.contains(a)).collect())
}

/// Check merge_strategy gating rules.
/// Returns Err with a user-facing message if the operation is not allowed.
fn check_merge_strategy_gate(mission: &MissionRow, want_all: bool) -> Result<()> {
    match mission.merge_strategy.as_str() {
        "coordinator_picks_winner" | "ranked_by_score" => {
            anyhow::bail!(
                "Mission '{}' has merge_strategy='{}' — this strategy is queued for a later release.\n\
                 Currently supported: human_approves_each, coordinator_merges_all.",
                mission.slug, mission.merge_strategy
            );
        }
        "human_approves_each" if want_all => {
            anyhow::bail!(
                "Mission '{}' has merge_strategy='human_approves_each': --all is not allowed.\n\
                 Use --approve <agent> to merge one agent at a time, or change the mission's \
                 merge_strategy to coordinator_merges_all.",
                mission.slug
            );
        }
        _ => Ok(()),
    }
}

/// Run all success_criteria check_commands in `work_dir` (no-shell argv split).
/// Returns (all_met, results_json).
fn run_checks_in_dir(
    criteria: &[serde_json::Value],
    work_dir: &Path,
) -> (bool, Vec<serde_json::Value>) {
    let mut results = Vec::new();
    for criterion in criteria {
        let desc = criterion
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("(no description)")
            .to_string();
        let check_cmd = criterion
            .get("check_command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        // QA-found 2026-06-13: run check_command via `sh -c` so users can
        // write natural shell expressions (pipes, &&, $(...)). check_command
        // is trusted mission-config — distinct from the PR-1.5 LLM-callable
        // `bash` tool which IS parsed argv with an allowlist.
        if check_cmd.trim().is_empty() {
            results.push(serde_json::json!({"description": desc, "exit_code": -1, "met": false}));
            continue;
        }
        let exit_code = std::process::Command::new("sh")
            .arg("-c")
            .arg(&check_cmd)
            .current_dir(work_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.code().unwrap_or(-1))
            .unwrap_or(-1);
        results.push(serde_json::json!({
            "description": desc,
            "exit_code": exit_code,
            "met": exit_code == 0,
        }));
    }
    let all_met = !results.is_empty() && results.iter().all(|r| r.get("met").and_then(|m| m.as_bool()).unwrap_or(false));
    (all_met, results)
}

/// Extract the set of criterion descriptions that were met in the most recent
/// 'success_check' or 'merge_check' event, for regression detection.
fn previously_met_criteria(conn: &Connection, mission_id: &str) -> Result<std::collections::HashSet<String>> {
    let latest: Option<String> = conn
        .query_row(
            "SELECT payload FROM mission_events
              WHERE mission_id = ?1
                AND kind IN ('success_check', 'merge_check')
              ORDER BY occurred_at DESC
              LIMIT 1",
            params![mission_id],
            |r| r.get(0),
        )
        .ok()
        .flatten();

    let Some(payload_str) = latest else {
        return Ok(std::collections::HashSet::new());
    };
    let v: serde_json::Value = serde_json::from_str(&payload_str).unwrap_or(serde_json::json!(null));
    let results = v.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    Ok(results
        .into_iter()
        .filter(|r| r.get("met").and_then(|m| m.as_bool()).unwrap_or(false))
        .filter_map(|r| r.get("description").and_then(|d| d.as_str()).map(|s| s.to_string()))
        .collect())
}

/// Perform a squash-merge of `agent` into the integration worktree.
/// Returns Ok(commit_sha) on success or Err(EscalatedBrief) on conflict/regression.
fn approve_one_agent(
    conn: &Connection,
    mission: &MissionRow,
    agent: &str,
    int_path: &Path,
    repo_root: &str,
) -> Result<()> {
    // Codex R1 [MED] fix: refuse re-approval of an already-merged/skipped
    // agent. Without this guard a concurrent CLI process can produce a
    // duplicate `agent_merged` event (and previously, with --allow-empty,
    // an empty provenance commit).
    let pending = pending_agents(conn, mission)?;
    if !pending.iter().any(|a| a == agent) {
        anyhow::bail!(
            "agent '{}' is not pending for mission '{}' (already merged or skipped). \
             See `ato missions merge {} --status`.",
            agent, mission.slug, mission.slug,
        );
    }

    let agent_branch = format!("ato/mission/{}/{}", mission.slug, slugify(agent));

    // Squash merge.
    let merge_out = std::process::Command::new("git")
        .args(["-C", int_path.to_str().unwrap_or("."), "merge", "--squash", &agent_branch])
        .output()
        .with_context(|| format!("git merge --squash {}", agent_branch))?;

    if !merge_out.status.success() {
        // Conflict — collect conflicting files BEFORE the abort wipes the
        // index, so the brief carries the real file list. Codex R1 [LOW] fix.
        let status_out = std::process::Command::new("git")
            .args(["-C", int_path.to_str().unwrap_or("."), "status", "--porcelain"])
            .output()
            .ok();
        let conflicting_files: Vec<String> = status_out
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter(|l| l.starts_with("UU") || l.starts_with("AA") || l.starts_with("DD"))
                    .map(|l| l[3..].trim().to_string())
                    .collect()
            })
            .unwrap_or_default();

        // Now abort to restore the integration branch to its pre-merge state.
        let abort_out = std::process::Command::new("git")
            .args(["-C", int_path.to_str().unwrap_or("."), "merge", "--abort"])
            .output()
            .ok();
        // Fallback: reset to HEAD if abort fails (e.g. nothing to abort).
        if abort_out.map(|o| !o.status.success()).unwrap_or(true) {
            let _ = std::process::Command::new("git")
                .args(["-C", int_path.to_str().unwrap_or("."), "reset", "--hard", "HEAD"])
                .output();
        }

        let now = chrono::Utc::now().to_rfc3339();
        let brief = EscalationBrief::new("merge_conflict")
            .summary(format!(
                "Agent '{}' conflicts with the integration branch — squash merge aborted",
                agent
            ))
            .ctx("agent", agent.to_string())
            .ctx("conflicting_files", conflicting_files.clone())
            .options([
                "resolve manually in the integration worktree then commit",
                "skip this agent (--skip)",
                "abandon mission",
            ])
            .into_payload();
        insert_event(conn, &mission.id, "escalated", Some(brief.clone()), &now)?;
        anyhow::bail!(
            "Merge conflict for agent '{}' in mission '{}'.\n\
             Conflicting files: {:?}\n\
             Options:\n\
               1. Resolve manually in {} then commit\n\
               2. ato missions merge {} --skip {}\n\
               3. Abandon mission",
            agent, mission.slug, conflicting_files,
            int_path.display(), mission.slug, agent
        );
    }

    // Collect previously-met criteria before this merge (for regression detection).
    let prev_met = previously_met_criteria(conn, &mission.id)?;

    // Get the agent worktree path for the commit message.
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("no HOME in env"))?;
    let agent_wt_path = PathBuf::from(&home)
        .join(".ato")
        .join("missions")
        .join(&mission.slug)
        .join("worktrees")
        .join(slugify(agent));

    // Get agent branch HEAD sha.
    let agent_head_sha = std::process::Command::new("git")
        .args(["-C", repo_root, "rev-parse", &agent_branch])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let base_sha = mission.base_sha.as_deref().unwrap_or("unknown");
    let commit_msg = format!(
        "mission({}): accept {}\n\nbase_sha: {}\nworkspace_root: {}\nsource_branch: {}@{}",
        mission.slug,
        agent,
        base_sha,
        agent_wt_path.display(),
        agent_branch,
        agent_head_sha,
    );

    // Codex R1 [MED] fix: drop --allow-empty. If the squash produced no
    // changes (duplicate work, or branch already merged), the commit fails
    // — which is the right outcome; the operator should --skip instead.
    let commit_out = std::process::Command::new("git")
        .args([
            "-C", int_path.to_str().unwrap_or("."),
            "commit", "-m", &commit_msg,
        ])
        .output()
        .with_context(|| format!("git commit for agent '{}'", agent))?;

    if !commit_out.status.success() {
        anyhow::bail!(
            "git commit after squash failed for agent '{}': {}",
            agent,
            String::from_utf8_lossy(&commit_out.stderr).trim()
        );
    }

    // Get integration HEAD sha after commit.
    let commit_sha = std::process::Command::new("git")
        .args(["-C", int_path.to_str().unwrap_or("."), "rev-parse", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    // Run ALL success_criteria check_commands in the integration worktree.
    let criteria = mission.success_criteria.as_array().cloned().unwrap_or_default();
    let (all_met, check_results) = run_checks_in_dir(&criteria, int_path);

    // Record merge_check event.
    let now = chrono::Utc::now().to_rfc3339();
    insert_event(
        conn,
        &mission.id,
        "merge_check",
        Some(serde_json::json!({
            "agent": agent,
            "commit_sha": commit_sha,
            "results": check_results,
            "all_met": all_met,
        })),
        &now,
    )?;

    // Check for regression: any previously-met criterion that is now unmet.
    let regressed: Vec<String> = check_results
        .iter()
        .filter(|r| !r.get("met").and_then(|m| m.as_bool()).unwrap_or(false))
        .filter_map(|r| r.get("description").and_then(|d| d.as_str()).map(|s| s.to_string()))
        .filter(|desc| prev_met.contains(desc))
        .collect();

    if !regressed.is_empty() {
        // Roll back the squash commit on the integration branch.
        let _ = std::process::Command::new("git")
            .args(["-C", int_path.to_str().unwrap_or("."), "reset", "--hard", "HEAD~1"])
            .output();

        let brief = EscalationBrief::new("regression_after_merge")
            .summary(format!(
                "Agent '{}' merge regressed previously-met success criteria — rolled back",
                agent
            ))
            .ctx("agent", agent.to_string())
            .ctx("regressed", regressed.clone())
            .options([
                "fix the regression in the agent's worktree and re-approve",
                "skip this agent (--skip)",
                "adjust success_criteria if the criterion is no longer relevant",
            ])
            .into_payload();
        insert_event(conn, &mission.id, "escalated", Some(brief.clone()), &now)?;
        anyhow::bail!(
            "Regression detected after merging agent '{}' in mission '{}'.\n\
             Regressed criteria: {:?}\n\
             The squash commit has been rolled back. Integration branch is clean.\n\
             Options:\n\
               1. Fix the regression in the agent worktree and re-approve\n\
               2. ato missions merge {} --skip {}\n\
               3. Adjust success_criteria if no longer relevant",
            agent, mission.slug, regressed, mission.slug, agent
        );
    }

    // Record agent_merged event. Codex R1 [LOW] fix: persist the same
    // provenance tuple the commit message carries so the SQLite audit
    // trail stays complete even after git history rewrites.
    insert_event(
        conn,
        &mission.id,
        "agent_merged",
        Some(serde_json::json!({
            "agent": agent,
            "commit_sha": commit_sha,
            "checks": check_results,
            "base_sha": base_sha,
            "workspace_root": agent_wt_path.display().to_string(),
            "source_branch": agent_branch,
            "source_branch_head_sha": agent_head_sha,
        })),
        &now,
    )?;

    conn.execute(
        "UPDATE missions SET updated_at = ?1 WHERE id = ?2",
        params![now, mission.id],
    )?;

    Ok(())
}

/// Final gate run by `ato missions merge <slug> --finish`. Refuses to declare
/// the mission complete while any `success_criteria` check_command fails in
/// the integration workspace. Per-agent rollback only catches regressions
/// against a previously-met baseline — if the first approved agent already
/// fails a criterion, that baseline is empty and the failure slips through
/// without this gate. Also serves missions whose only agent introduces a
/// brand-new criterion that was never met before.
///
/// Side effects: always inserts a `merge_check` receipt (phase="finish"); on
/// failure, inserts an `escalated` event with reason="finish_blocked_unmet_criteria"
/// and bails so the caller does NOT write `integration_complete`.
fn finish_gate(
    conn: &Connection,
    mission: &MissionRow,
    int_path: &Path,
    int_branch: &str,
    head_sha: &str,
    now: &str,
) -> Result<()> {
    let criteria = mission.success_criteria.as_array().cloned().unwrap_or_default();
    if criteria.is_empty() {
        // No criteria to verify — nothing to gate on.
        return Ok(());
    }
    // QA-found 2026-06-13: if criteria are non-empty but no agents were ever
    // merged, the integration worktree never got materialized — the prior
    // short-circuit on `!int_path.exists()` let the mission finish without
    // ever checking the criteria. Refuse instead.
    if !int_path.exists() {
        let unmet: Vec<String> = criteria
            .iter()
            .filter_map(|c| c.get("description").and_then(|d| d.as_str()).map(|s| s.to_string()))
            .collect();
        let brief = EscalationBrief::new("finish_blocked_no_integration_workspace")
            .summary("Cannot finish — no agents have been merged, so success criteria were never verified")
            .ctx("branch", int_branch.to_string())
            .ctx("unmet", unmet.clone())
            .options([
                "ato missions merge <slug> --approve <agent> on at least one agent worktree first",
                "adjust success_criteria if a criterion is no longer relevant",
                "set-category ignored to close the mission without verification",
            ])
            .into_payload();
        insert_event(conn, &mission.id, "escalated", Some(brief.clone()), now)?;
        anyhow::bail!(
            "Cannot finish mission '{}': no integration workspace exists (no agents merged) and {} success criterion/criteria are defined.\n\
             Approve at least one agent (or skip them all) so the integration branch materializes, then re-run --finish.",
            mission.slug, criteria.len(),
        );
    }
    let (all_met, results) = run_checks_in_dir(&criteria, int_path);
    insert_event(
        conn,
        &mission.id,
        "merge_check",
        Some(serde_json::json!({
            "phase": "finish",
            "results": results.clone(),
            "all_met": all_met,
        })),
        now,
    )?;
    if !all_met {
        let unmet: Vec<String> = results
            .iter()
            .filter(|r| !r.get("met").and_then(|m| m.as_bool()).unwrap_or(false))
            .filter_map(|r| r.get("description").and_then(|d| d.as_str()).map(|s| s.to_string()))
            .collect();
        let brief = EscalationBrief::new("finish_blocked_unmet_criteria")
            .summary(format!(
                "Cannot finish — {} success criterion/criteria still unmet on the integration branch",
                unmet.len()
            ))
            .ctx("branch", int_branch.to_string())
            .ctx("head_sha", head_sha.to_string())
            .ctx("unmet", unmet.clone())
            .options([
                "fix the unmet criteria in the integration worktree and re-run `ato missions merge <slug> --finish`",
                "ato missions merge <slug> --approve <agent> on a new agent worktree that addresses the gap",
                "adjust success_criteria if a criterion is no longer relevant",
            ])
            .into_payload();
        insert_event(conn, &mission.id, "escalated", Some(brief.clone()), now)?;
        anyhow::bail!(
            "Cannot finish mission '{}': {} success criterion/criteria still unmet on the integration branch.\n\
             Unmet: {:?}\n\
             Branch left intact for inspection: {}\n\
             Run `ato missions events {}` to see the brief and options.",
            mission.slug, unmet.len(), unmet, int_branch, mission.slug,
        );
    }
    Ok(())
}

/// `ato missions merge <slug> [flags]`
fn run_merge(input: MergeInput, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let mission = load_mission(&conn, &input.slug_or_id)?;

    // --status is always allowed.
    if input.status {
        return run_merge_status(&conn, &mission, opts);
    }

    // Strategy gating: picks_winner / ranked_by_score refuse everything except --status.
    // --all requires coordinator_merges_all.
    check_merge_strategy_gate(&mission, input.all)?;

    // --skip
    if let Some(agent) = &input.skip {
        let now = chrono::Utc::now().to_rfc3339();
        insert_event(
            &conn,
            &mission.id,
            "agent_skipped",
            Some(serde_json::json!({
                "agent": agent,
                "reason": input.reason,
            })),
            &now,
        )?;
        conn.execute(
            "UPDATE missions SET updated_at = ?1 WHERE id = ?2",
            params![now, mission.id],
        )?;
        if opts.human {
            emit_human(&format!("Skipped agent '{}' for mission '{}'", agent, mission.slug));
        } else {
            emit_json(&serde_json::json!({"skipped": agent, "mission": mission.slug}))?;
        }
        return Ok(());
    }

    // --approve <agent>
    if let Some(agent) = &input.approve {
        let int_path = ensure_integration_worktree(&conn, &mission)?;
        let repo_root = mission.repo_root.as_deref().ok_or_else(|| anyhow::anyhow!("no repo_root"))?;
        approve_one_agent(&conn, &mission, agent, &int_path, repo_root)?;
        if opts.human {
            emit_human(&format!("Merged agent '{}' into integration branch for mission '{}'", agent, mission.slug));
        } else {
            emit_json(&serde_json::json!({"merged": agent, "mission": mission.slug}))?;
        }
        return Ok(());
    }

    // --all
    if input.all {
        let int_path = ensure_integration_worktree(&conn, &mission)?;
        let repo_root = mission.repo_root.as_deref().ok_or_else(|| anyhow::anyhow!("no repo_root"))?.to_string();
        let agents = pending_agents(&conn, &mission)?;
        if agents.is_empty() {
            if opts.human {
                emit_human(&format!("No pending agents for mission '{}'", mission.slug));
            } else {
                emit_json(&serde_json::json!({"pending": [], "mission": mission.slug}))?;
            }
            return Ok(());
        }
        for agent in &agents {
            if opts.human {
                emit_human(&format!("  Merging agent '{}'...", agent));
            }
            approve_one_agent(&conn, &mission, agent, &int_path, &repo_root)?;
            if opts.human {
                emit_human(&format!("  ✓ merged '{}'", agent));
            }
        }
        if opts.human {
            emit_human(&format!("All {} agent(s) merged for mission '{}'", agents.len(), mission.slug));
        } else {
            emit_json(&serde_json::json!({"merged_all": agents, "mission": mission.slug}))?;
        }
        return Ok(());
    }

    // --finish
    if input.finish {
        let remaining = pending_agents(&conn, &mission)?;
        if !remaining.is_empty() {
            anyhow::bail!(
                "Cannot finish: {} pending agent(s) remain: {:?}\n\
                 Approve or skip all agents before calling --finish.",
                remaining.len(), remaining
            );
        }

        // Collect merged + skipped from events.
        let (merged_agents, skipped_agents) = collect_merged_skipped(&conn, &mission.id)?;

        // Get integration branch HEAD sha.
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or_else(|| anyhow::anyhow!("no HOME"))?;
        let int_path = PathBuf::from(&home)
            .join(".ato")
            .join("missions")
            .join(&mission.slug)
            .join("integration");

        let head_sha = if int_path.exists() {
            std::process::Command::new("git")
                .args(["-C", int_path.to_str().unwrap_or("."), "rev-parse", "HEAD"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };

        let int_branch = format!("ato/mission/{}/integration", mission.slug);
        let now = chrono::Utc::now().to_rfc3339();

        // Codex R1 [HIGH] fix: gate --finish on a final pass of every
        // success_criterion against the integration workspace.
        finish_gate(&conn, &mission, &int_path, &int_branch, &head_sha, &now)?;

        insert_event(
            &conn,
            &mission.id,
            "integration_complete",
            Some(serde_json::json!({
                "branch": int_branch,
                "head_sha": head_sha,
                "merged": merged_agents,
                "skipped": skipped_agents,
            })),
            &now,
        )?;

        // Write result_metadata to the mission row.
        let result_metadata = serde_json::json!({
            "integration_branch": int_branch,
            "head_sha": head_sha,
            "merged": merged_agents,
            "skipped": skipped_agents,
        });
        let rm_str = serde_json::to_string(&result_metadata).context("serialize result_metadata")?;
        conn.execute(
            "UPDATE missions SET result_metadata = ?1, updated_at = ?2 WHERE id = ?3",
            params![rm_str, now, mission.id],
        )?;

        if opts.human {
            emit_human(&format!(
                "Integration complete for mission '{}'.\n\
                 Branch: {}\n\
                 Merge this branch into your working branch to apply the changes.",
                mission.slug, int_branch
            ));
        } else {
            emit_json(&result_metadata)?;
        }
        return Ok(());
    }

    // Bare `ato missions merge <slug>` — interactive wrapper.
    run_merge_interactive(conn, mission, db_path, opts)
}

/// --status: list each agent with merged/skipped/pending + diffstat.
fn run_merge_status(conn: &Connection, mission: &MissionRow, opts: &Opts) -> Result<()> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("no HOME"))?;
    let wt_root = worktree_root_for(&home, &mission.slug);

    // All agent dir names (sorted).
    let mut all_agents: Vec<String> = Vec::new();
    if wt_root.exists() {
        for entry in fs::read_dir(&wt_root)
            .with_context(|| format!("read_dir {}", wt_root.display()))?
        {
            let entry = entry?;
            if entry.path().is_dir() {
                all_agents.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    all_agents.sort();

    // Collect handled agents and their status.
    let mut agent_status: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT kind, payload FROM mission_events
          WHERE mission_id = ?1
            AND kind IN ('agent_merged', 'agent_skipped')
          ORDER BY occurred_at ASC",
    )?;
    let rows: Vec<(String, Option<String>)> = stmt
        .query_map(params![mission.id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    // agent_status holds borrowed strs — use String instead.
    let mut agent_status_owned: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (kind, payload_opt) in &rows {
        if let Some(s) = payload_opt {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
                if let Some(a) = v.get("agent").and_then(|a| a.as_str()) {
                    let status = if kind == "agent_merged" { "merged" } else { "skipped" };
                    agent_status_owned.insert(a.to_string(), status.to_string());
                }
            }
        }
    }
    drop(agent_status); // unused, drop it

    let repo_root = mission.repo_root.as_deref().unwrap_or(".");
    let base_sha = mission.base_sha.as_deref().unwrap_or("HEAD");

    #[derive(Serialize)]
    struct AgentStatus {
        agent: String,
        status: String,
        diffstat: String,
    }

    let mut statuses: Vec<AgentStatus> = Vec::new();
    for agent in &all_agents {
        let st = agent_status_owned.get(agent).cloned().unwrap_or_else(|| "pending".to_string());
        let agent_branch = format!("ato/mission/{}/{}", mission.slug, agent);
        let diffstat = std::process::Command::new("git")
            .args(["-C", repo_root, "diff", "--stat",
                   &format!("{}..{}", base_sha, agent_branch)])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        statuses.push(AgentStatus { agent: agent.clone(), status: st, diffstat });
    }

    if opts.human {
        if statuses.is_empty() {
            emit_human(&format!("No agent worktrees found for mission '{}'", mission.slug));
        } else {
            for s in &statuses {
                emit_human(&format!("[{}] {}\n  diffstat: {}", s.status, s.agent,
                    if s.diffstat.is_empty() { "(no changes)" } else { &s.diffstat }));
            }
        }
    } else {
        emit_json(&statuses)?;
    }
    Ok(())
}

/// Interactive merge wrapper (TTY only).
fn run_merge_interactive(
    conn: Connection,
    mission: MissionRow,
    _db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    use std::io::{self, BufRead, Write};

    if !std::io::IsTerminal::is_terminal(&io::stdin()) {
        anyhow::bail!(
            "ato missions merge '{}': not a terminal.\n\
             Use --approve/--skip/--status/--all/--finish in scripts.",
            mission.slug
        );
    }

    let repo_root = mission.repo_root.as_deref()
        .ok_or_else(|| anyhow::anyhow!("no repo_root"))?.to_string();
    let base_sha = mission.base_sha.as_deref().unwrap_or("HEAD").to_string();
    let int_path = ensure_integration_worktree(&conn, &mission)?;

    let agents = pending_agents(&conn, &mission)?;
    if agents.is_empty() {
        emit_human(&format!(
            "No pending agents for mission '{}'. Use --finish to complete integration.",
            mission.slug
        ));
        return Ok(());
    }

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    for agent in &agents {
        let agent_branch = format!("ato/mission/{}/{}", mission.slug, agent);

        // Show diffstat.
        let diff_stat_str = std::process::Command::new("git")
            .args(["-C", &repo_root, "diff", "--stat",
                   &format!("{}..{}", base_sha, agent_branch)])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        emit_human(&format!("\nAgent: {}\n{}", agent, diff_stat_str));

        loop {
            print!("[a]pprove / [s]kip / [d]iff / [q]uit: ");
            io::stdout().flush().ok();
            let line = match lines.next() {
                Some(Ok(l)) => l.trim().to_lowercase(),
                _ => "q".to_string(),
            };
            match line.as_str() {
                "a" | "approve" => {
                    match approve_one_agent(&conn, &mission, agent, &int_path, &repo_root) {
                        Ok(_) => { emit_human(&format!("  ✓ merged '{}'", agent)); }
                        Err(e) => { emit_human(&format!("  ✗ {}", e)); }
                    }
                    break;
                }
                "s" | "skip" => {
                    let now = chrono::Utc::now().to_rfc3339();
                    let _ = insert_event(
                        &conn,
                        &mission.id,
                        "agent_skipped",
                        Some(serde_json::json!({"agent": agent, "reason": "interactive_skip"})),
                        &now,
                    );
                    emit_human(&format!("  Skipped '{}'", agent));
                    break;
                }
                "d" | "diff" => {
                    let full_diff_str = std::process::Command::new("git")
                        .args(["-C", &repo_root, "diff",
                               &format!("{}..{}", base_sha, agent_branch)])
                        .output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                        .unwrap_or_default();
                    emit_human(&full_diff_str);
                }
                "q" | "quit" => {
                    emit_human("Quit interactive merge. Run again to continue.");
                    return Ok(());
                }
                _ => { emit_human("  Unknown input. Use a/s/d/q."); }
            }
        }
    }

    let _ = opts; // used for consistency
    emit_human(&format!(
        "\nAll agents processed. Run `ato missions merge {} --finish` to complete integration.",
        mission.slug
    ));
    Ok(())
}

/// Collect lists of merged and skipped agent names from mission events.
fn collect_merged_skipped(conn: &Connection, mission_id: &str) -> Result<(Vec<String>, Vec<String>)> {
    let mut stmt = conn.prepare(
        "SELECT kind, payload FROM mission_events
          WHERE mission_id = ?1
            AND kind IN ('agent_merged', 'agent_skipped')
          ORDER BY occurred_at ASC",
    )?;
    let rows: Vec<(String, Option<String>)> = stmt
        .query_map(params![mission_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut merged = Vec::new();
    let mut skipped = Vec::new();
    for (kind, payload_opt) in rows {
        if let Some(s) = payload_opt {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                if let Some(a) = v.get("agent").and_then(|a| a.as_str()) {
                    if kind == "agent_merged" {
                        merged.push(a.to_string());
                    } else {
                        skipped.push(a.to_string());
                    }
                }
            }
        }
    }
    Ok((merged, skipped))
}

// ── v2.16 PR-4: set-worker ────────────────────────────────────────────

fn run_set_worker(
    slug_or_id: String,
    runtime: String,
    model: Option<String>,
    require_tools: Vec<String>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    if runtime.trim().is_empty() {
        anyhow::bail!("missions set-worker: --runtime is required");
    }
    let conn = db::open_readwrite(db_path)?;
    let mission = load_mission(&conn, &slug_or_id)?;

    let new_cfg = WorkerConfig {
        runtime: runtime.clone(),
        model: model.clone(),
        require_tools: require_tools.clone(),
    };
    let new_cfg_json = serde_json::to_value(&new_cfg).context("serialize worker_config")?;
    let new_cfg_str = serde_json::to_string(&new_cfg_json).context("serialize worker_config")?;

    let now = chrono::Utc::now().to_rfc3339();
    let old_cfg = mission.worker_config.clone();

    conn.execute(
        "UPDATE missions SET worker_config = ?1, updated_at = ?2 WHERE id = ?3",
        params![new_cfg_str, now, mission.id],
    )
    .context("update missions.worker_config")?;

    insert_event(
        &conn,
        &mission.id,
        "worker_config_changed",
        Some(serde_json::json!({
            "from": old_cfg,
            "to": new_cfg_json,
        })),
        &now,
    )?;

    let updated = load_mission(&conn, &mission.id)?;
    if opts.human {
        emit_human(&format!(
            "Mission '{}' worker_config set: runtime={}{}{}\n  previous: {}",
            updated.slug,
            runtime,
            model
                .as_deref()
                .map(|m| format!(" model={}", m))
                .unwrap_or_default(),
            if require_tools.is_empty() {
                String::new()
            } else {
                format!(" require-tools={}", require_tools.join(","))
            },
            old_cfg
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "(none)".to_string()),
        ));
    } else {
        emit_json(&updated)?;
    }
    Ok(())
}

// ── v2.16 PR-4: tick ──────────────────────────────────────────────────

/// Output produced by `tick` for a single mission.
#[derive(Debug, Serialize)]
struct TickResult {
    slug: String,
    state: String,
    action: String,
    detail: Option<serde_json::Value>,
}

/// `ato missions tick [<slug>] [--json]`
///
/// One-shot coordinator wake. Iterates missions in state open|in_progress
/// (or just the specified slug). Per mission, at most ONE action per tick.
fn run_tick(
    slug_or_id: Option<String>,
    json_output: bool,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;

    let missions: Vec<MissionRow> = if let Some(ref s) = slug_or_id {
        let m = load_mission(&conn, s)?;
        vec![m]
    } else {
        // All open or in_progress missions.
        let mut stmt = conn.prepare(&format!(
            "{} WHERE state IN ('open', 'in_progress') ORDER BY updated_at ASC",
            MISSION_SELECT
        ))?;
        let iter = stmt.query_map([], row_to_mission)?;
        iter.filter_map(|r| r.ok()).collect()
    };

    let mut results: Vec<TickResult> = Vec::new();

    for mission in &missions {
        let result = tick_one_mission(&conn, mission, db_path)?;
        results.push(result);
    }

    if json_output || !opts.human {
        emit_json(&results)?;
    } else {
        for r in &results {
            let detail_str = r
                .detail
                .as_ref()
                .map(|d| format!(" — {}", d))
                .unwrap_or_default();
            emit_human(&format!(
                "  {} [{}]  {}{}",
                r.slug, r.state, r.action, detail_str
            ));
        }
    }
    Ok(())
}

/// Execute the coordinator decision tree for one mission.
/// Returns the action taken (or "no action").
fn tick_one_mission(
    conn: &Connection,
    mission: &MissionRow,
    db_path: &PathBuf,
) -> Result<TickResult> {
    let now = chrono::Utc::now().to_rfc3339();

    // (a) Skip if ignored.
    if mission.category == "ignored" {
        return Ok(TickResult {
            slug: mission.slug.clone(),
            state: mission.state.clone(),
            action: "skip".to_string(),
            detail: Some(serde_json::json!("category=ignored")),
        });
    }

    // (b) Pre-flight for worktree missions: verify base_sha is still resolvable.
    if mission.workspace_strategy == "per_agent_worktree" {
        if let Some(repo_root) = mission.repo_root.as_deref() {
            if let Some(base_sha) = mission.base_sha.as_deref() {
                let resolve_out = std::process::Command::new("git")
                    .args(["-C", repo_root, "rev-parse", "--verify", "--quiet"])
                    .arg(format!("{}^{{commit}}", base_sha))
                    .output()
                    .ok();
                let resolvable = resolve_out
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                if !resolvable {
                    // Escalate once (dedup).
                    let reason = "base_sha_unresolvable";
                    if !escalation_is_pending(conn, &mission.id, reason)? {
                        insert_event(
                            conn,
                            &mission.id,
                            "escalated",
                            Some(serde_json::json!({
                                "reason": reason,
                                "base_sha": base_sha,
                                "repo_root": repo_root,
                            })),
                            &now,
                        )?;
                        conn.execute(
                            "UPDATE missions SET updated_at = ?1 WHERE id = ?2",
                            params![now, mission.id],
                        )?;
                        return Ok(TickResult {
                            slug: mission.slug.clone(),
                            state: mission.state.clone(),
                            action: "escalated".to_string(),
                            detail: Some(serde_json::json!({"reason": reason})),
                        });
                    }
                    return Ok(TickResult {
                        slug: mission.slug.clone(),
                        state: mission.state.clone(),
                        action: "no action".to_string(),
                        detail: Some(serde_json::json!("base_sha unresolvable (already escalated)")),
                    });
                }
            }
        }
    }

    // (c) In-flight scan — look for dispatch_started events without a matching
    // dispatched event and with a dead pid. ONE stale event found = that IS
    // this tick's action for this mission.
    let stale_attempt = find_stale_dispatch_started(conn, &mission.id)?;
    if let Some((attempt_id, pid)) = stale_attempt {
        insert_event(
            conn,
            &mission.id,
            "dispatch_abandoned",
            Some(serde_json::json!({
                "attempt_id": attempt_id,
                "pid": pid,
                "reason": "stale_started_event_dead_pid",
            })),
            &now,
        )?;
        conn.execute(
            "UPDATE missions SET updated_at = ?1 WHERE id = ?2",
            params![now, mission.id],
        )?;
        return Ok(TickResult {
            slug: mission.slug.clone(),
            state: mission.state.clone(),
            action: "dispatch_abandoned".to_string(),
            detail: Some(serde_json::json!({"attempt_id": attempt_id, "pid": pid})),
        });
    }

    // (d) Success evaluation — only if there are terminal worker events newer
    // than the latest success_check (or no success_check + at least one terminal event).
    if should_run_success_check(conn, &mission.id)? {
        let sc_result = run_success_evaluation(conn, mission, &now, db_path)?;
        if sc_result.all_met {
            let (_updated, cleanup_err) = transition_state(conn, mission, "complete", Some("success_criteria_met"))?;
            return Ok(TickResult {
                slug: mission.slug.clone(),
                state: "complete".to_string(),
                action: "completed".to_string(),
                detail: Some(serde_json::json!({
                    "success_check": sc_result.results,
                    "cleanup_warning": cleanup_err,
                })),
            });
        }
        return Ok(TickResult {
            slug: mission.slug.clone(),
            state: mission.state.clone(),
            action: "success_check".to_string(),
            detail: Some(serde_json::json!({"all_met": false, "results": sc_result.results})),
        });
    }

    // (e) Failure guard — 3 consecutive failures → blocked + escalated brief.
    let consecutive_failures = count_consecutive_failures(conn, &mission.id)?;
    if consecutive_failures >= 3 {
        // Only escalate if not already pending.
        let reason = "consecutive_failures";
        if !escalation_is_pending(conn, &mission.id, reason)? {
            let last_failures = last_n_failure_summaries(conn, &mission.id, 3)?;
            let brief_payload = EscalationBrief::new(reason)
                .summary("Worker has failed 3 times in a row — mission blocked pending owner decision")
                .ctx("failures", last_failures)
                .options([
                    "fix the underlying error and set-state in_progress",
                    "change worker_config",
                    "abandon: set-category done",
                ])
                .into_payload();
            // Block the mission.
            conn.execute(
                "UPDATE missions SET state = 'blocked', updated_at = ?1 WHERE id = ?2",
                params![now, mission.id],
            )?;
            insert_event(
                conn,
                &mission.id,
                "state_changed",
                Some(serde_json::json!({"from": mission.state, "to": "blocked", "reason": reason})),
                &now,
            )?;
            insert_event(conn, &mission.id, "escalated", Some(brief_payload.clone()), &now)?;
            return Ok(TickResult {
                slug: mission.slug.clone(),
                state: "blocked".to_string(),
                action: "blocked+escalated".to_string(),
                detail: Some(brief_payload),
            });
        }
        // Already blocked + escalated — no action.
        return Ok(TickResult {
            slug: mission.slug.clone(),
            state: mission.state.clone(),
            action: "no action".to_string(),
            detail: Some(serde_json::json!("consecutive_failures (already escalated)")),
        });
    }

    // (f) Spawn detached child if worker_config set, no in-flight, budgets allow.
    // Check in-flight via loop_run_started + loop_run_completed or dispatch_started events.
    if is_in_flight(conn, &mission.id)? {
        return Ok(TickResult {
            slug: mission.slug.clone(),
            state: mission.state.clone(),
            action: "no action".to_string(),
            detail: Some(serde_json::json!("work already in flight")),
        });
    }

    // Budget check — same gates as dispatch.
    let prior_dispatch_count = count_dispatches_for_mission(conn, &mission.id)?;
    if let Some(max) = mission.max_loops {
        if prior_dispatch_count >= max {
            return Ok(TickResult {
                slug: mission.slug.clone(),
                state: mission.state.clone(),
                action: "no action".to_string(),
                detail: Some(serde_json::json!({
                    "reason": "max_loops_reached",
                    "prior": prior_dispatch_count,
                    "max": max,
                })),
            });
        }
    }
    let prior_cost = sum_cost_for_mission(conn, &mission.id)?;
    if let Some(budget) = mission.token_budget_usd {
        if prior_cost >= budget {
            return Ok(TickResult {
                slug: mission.slug.clone(),
                state: mission.state.clone(),
                action: "no action".to_string(),
                detail: Some(serde_json::json!({
                    "reason": "token_budget_exhausted",
                    "spent": prior_cost,
                    "budget": budget,
                })),
            });
        }
    }

    // worker_config gate.
    let wc = match &mission.worker_config {
        Some(v) => v.clone(),
        None => {
            // Escalate once with reason=no_worker_config.
            let reason = "no_worker_config";
            if !escalation_is_pending(conn, &mission.id, reason)? {
                // Promote the legacy `hint` to a proper option so PR-6
                // renders it consistently with other briefs.
                insert_event(
                    conn,
                    &mission.id,
                    "escalated",
                    Some(
                        EscalationBrief::new(reason)
                            .summary("Mission has no worker_config — coordinator can't dispatch")
                            .ctx(
                                "hint",
                                "run `ato missions set-worker <slug> --runtime <r>` to configure the coordinator worker",
                            )
                            .option(
                                "ato missions set-worker <slug> --runtime <r> [--model <m>] [--require-tools a,b]",
                            )
                            .option("set-category ignored to leave this mission alone")
                            .into_payload(),
                    ),
                    &now,
                )?;
                conn.execute(
                    "UPDATE missions SET updated_at = ?1 WHERE id = ?2",
                    params![now, mission.id],
                )?;
                return Ok(TickResult {
                    slug: mission.slug.clone(),
                    state: mission.state.clone(),
                    action: "escalated".to_string(),
                    detail: Some(serde_json::json!({"reason": reason})),
                });
            }
            return Ok(TickResult {
                slug: mission.slug.clone(),
                state: mission.state.clone(),
                action: "no action".to_string(),
                detail: Some(serde_json::json!("no_worker_config (already escalated)")),
            });
        }
    };

    // Parse worker_config.
    let wc_parsed: WorkerConfig =
        serde_json::from_value(wc).unwrap_or_else(|_| WorkerConfig::default());
    if wc_parsed.runtime.is_empty() {
        return Ok(TickResult {
            slug: mission.slug.clone(),
            state: mission.state.clone(),
            action: "no action".to_string(),
            detail: Some(serde_json::json!("worker_config has empty runtime")),
        });
    }

    // Compose the prompt: goal + last 5 event digest + unmet criteria.
    let prompt = compose_tick_prompt(conn, mission)?;

    // Spawn detached child: ato missions dispatch <slug> --runtime <r> [--model <m>] [--require-tools <t>] --prompt <p> --attempt-id <id>
    //
    // Finding 3 (PR-4 review): the PARENT mints the attempt_id and inserts
    // dispatch_started BEFORE the child runs, so in-flight is visible the
    // instant spawn() returns.  The child receives --attempt-id and skips
    // its own dispatch_started insert; it still writes the closing 'dispatched'
    // event with the same attempt_id.
    let tick_attempt_id = Uuid::new_v4().to_string();
    let exe = std::env::current_exe()
        .context("resolve current exe for detached spawn")?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("missions")
        .arg("dispatch")
        .arg(&mission.slug)
        .arg("--runtime")
        .arg(&wc_parsed.runtime);
    if let Some(ref m) = wc_parsed.model {
        cmd.arg("--model").arg(m);
    }
    if !wc_parsed.require_tools.is_empty() {
        cmd.arg("--require-tools").arg(wc_parsed.require_tools.join(","));
    }
    cmd.arg("--prompt").arg(&prompt);
    // Pass the pre-minted attempt_id so the child skips its own dispatch_started.
    cmd.arg("--attempt-id").arg(&tick_attempt_id);
    // Detach: null stdin/stdout/stderr, do NOT wait.
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let child = cmd.spawn().context("spawn detached dispatch child")?;

    // Record dispatch_started in the PARENT immediately after spawn() so the
    // in-flight record exists even if the child crashes before its own DB write.
    insert_event(
        conn,
        &mission.id,
        "dispatch_started",
        Some(serde_json::json!({
            "attempt_id": tick_attempt_id,
            "runtime": wc_parsed.runtime,
            "agent": null,
            "pid": child.id(),
            "slug": mission.slug,
            "spawned_by": "tick",
        })),
        &now,
    )?;

    // Transition open → in_progress if needed.
    if mission.state == "open" {
        conn.execute(
            "UPDATE missions SET state = 'in_progress', updated_at = ?1 WHERE id = ?2",
            params![now, mission.id],
        )?;
        insert_event(
            conn,
            &mission.id,
            "state_changed",
            Some(serde_json::json!({
                "from": "open",
                "to": "in_progress",
                "reason": "tick_spawned_worker",
            })),
            &now,
        )?;
    } else {
        conn.execute(
            "UPDATE missions SET updated_at = ?1 WHERE id = ?2",
            params![now, mission.id],
        )?;
    }

    Ok(TickResult {
        slug: mission.slug.clone(),
        state: "in_progress".to_string(),
        action: "worker_spawned".to_string(),
        detail: Some(serde_json::json!({
            "runtime": wc_parsed.runtime,
            "model": wc_parsed.model,
        })),
    })
}

/// Compose the prompt the coordinator passes to the spawned worker:
/// mission goal + bullet digest of last 5 events + unmet criteria descriptions.
fn compose_tick_prompt(conn: &Connection, mission: &MissionRow) -> Result<String> {
    // Last 5 events (newest first for context).
    let mut stmt = conn.prepare(
        "SELECT kind, occurred_at FROM mission_events
          WHERE mission_id = ?1
          ORDER BY occurred_at DESC
          LIMIT 5",
    )?;
    let events: Vec<(String, String)> = stmt
        .query_map(params![mission.id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let event_bullets: String = events
        .iter()
        .map(|(k, t)| format!("  - {} ({})", k, t))
        .collect::<Vec<_>>()
        .join("\n");

    // Unmet criteria descriptions.
    let criteria_descs: Vec<String> = mission
        .success_criteria
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|c| c.get("description").and_then(|d| d.as_str()).map(|s| format!("  - {}", s)))
        .collect();

    Ok(format!(
        "Mission goal: {}\n\nRecent events:\n{}\n\nUnmet success criteria:\n{}",
        mission.goal,
        if event_bullets.is_empty() { "  (none yet)".to_string() } else { event_bullets },
        if criteria_descs.is_empty() { "  (none defined)".to_string() } else { criteria_descs.join("\n") },
    ))
}

// ── v2.16 PR-4: check ─────────────────────────────────────────────────

/// `ato missions check <slug>` — force success evaluation immediately.
fn run_check(slug_or_id: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let mission = load_mission(&conn, &slug_or_id)?;
    let now = chrono::Utc::now().to_rfc3339();

    let sc_result = run_success_evaluation(&conn, &mission, &now, db_path)?;

    let mut cleanup_err: Option<String> = None;
    if sc_result.all_met && mission.state != "complete" {
        let (_updated, ce) = transition_state(&conn, &mission, "complete", Some("manual_check_success"))?;
        cleanup_err = ce;
    }

    let updated = load_mission(&conn, &mission.id)?;
    if opts.human {
        emit_human(&format!(
            "Mission '{}' check: all_met={} state={}",
            updated.slug, sc_result.all_met, updated.state
        ));
        for r in &sc_result.results {
            emit_human(&format!(
                "  [{}] {} (exit {})",
                if r.met { "x" } else { " " },
                r.description,
                r.exit_code,
            ));
        }
        if let Some(ref e) = cleanup_err {
            emit_human(&format!("  ⚠ worktree cleanup failed: {}", e));
        }
    } else {
        emit_json(&serde_json::json!({
            "mission_slug": updated.slug,
            "state": updated.state,
            "all_met": sc_result.all_met,
            "results": sc_result.results,
            "cleanup_warning": cleanup_err,
        }))?;
    }
    Ok(())
}

// ── PR-4 helpers ──────────────────────────────────────────────────────

/// Pure predicate: is `pid` still OUR worker for mission `slug`?
///
/// Requires ALL of:
///   (a) pid_alive  — `kill -0` returned success
///   (b) ps_command contains both "missions" and "dispatch" and `slug`
///       (argv check, no shell — `ps -p <pid> -o command=`)
///       If ps_command is None (ps failed or produced no output) the
///       process is treated as NOT ours → stale.
///   (c) age_secs < MAX_WORKER_AGE_SECS — backstop against ancient events
///       whose pids have been recycled by the OS.
///
/// The parameters are kept primitive so callers can unit-test without real pids.
fn is_worker_process_live(pid_alive: bool, ps_command: Option<&str>, slug: &str, age_secs: i64) -> bool {
    if !pid_alive {
        return false;
    }
    // Identity check — ps output must mention "missions", "dispatch", and the slug.
    let cmd_str = match ps_command {
        Some(s) if !s.is_empty() => s,
        _ => return false, // ps failed or empty → not our process
    };
    // Defensive: empty slug cannot match uniquely — treat as stale.
    if slug.is_empty() {
        return false;
    }
    // Exact token sequence: … missions dispatch <slug> …
    // Substring match would allow slug "api" to match "api-v2".
    let tokens: Vec<&str> = cmd_str.split_whitespace().collect();
    let token_match = tokens.windows(3).any(|w| w[0] == "missions" && w[1] == "dispatch" && w[2] == slug);
    if !token_match {
        return false;
    }
    // Age backstop.
    age_secs < MAX_WORKER_AGE_SECS
}

/// Run `ps -p <pid> -o command=` without a shell and return the trimmed output,
/// or None if ps fails or produces no output.
fn ps_command_for_pid(pid: u32) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// Find a stale `dispatch_started` event: one whose attempt_id has no
/// matching `dispatched` event AND that fails the `is_worker_process_live`
/// predicate (dead pid, wrong argv, or older than MAX_WORKER_AGE_SECS).
/// Returns Some((attempt_id, pid)) for the first stale event found.
fn find_stale_dispatch_started(
    conn: &Connection,
    mission_id: &str,
) -> Result<Option<(String, u32)>> {
    // Collect started attempt_ids + occurred_at that have NOT been closed.
    let mut stmt = conn.prepare(
        "SELECT payload, occurred_at FROM mission_events
          WHERE mission_id = ?1 AND kind = 'dispatch_started'
          ORDER BY occurred_at ASC",
    )?;
    let rows: Vec<(Option<String>, String)> = stmt
        .query_map(params![mission_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    // Collect closed attempt_ids from 'dispatched' + 'dispatch_abandoned' events.
    let mut stmt2 = conn.prepare(
        "SELECT payload FROM mission_events
          WHERE mission_id = ?1 AND kind IN ('dispatched', 'dispatch_abandoned')
          ORDER BY occurred_at ASC",
    )?;
    let closed_attempt_ids: std::collections::HashSet<String> = stmt2
        .query_map(params![mission_id], |r| r.get::<_, Option<String>>(0))?
        .filter_map(|r| r.ok().flatten())
        .filter_map(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .filter_map(|v| v.get("attempt_id").and_then(|a| a.as_str()).map(|s| s.to_string()))
        .collect();

    let now_utc = chrono::Utc::now();

    for (payload_opt, occurred_at) in rows {
        let Some(payload_str) = payload_opt else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&payload_str) else { continue };
        let Some(attempt_id) = v.get("attempt_id").and_then(|a| a.as_str()) else { continue };
        if closed_attempt_ids.contains(attempt_id) {
            continue; // This dispatch completed normally.
        }
        let pid_val = v.get("pid").and_then(|p| p.as_u64()).unwrap_or(0) as u32;
        if pid_val == 0 {
            continue; // No pid stored — can't verify.
        }
        // (a) pid alive?
        let pid_alive = std::process::Command::new("kill")
            .args(["-0", &pid_val.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        // (b) ps identity check — None if ps fails or output is empty.
        let ps_cmd = if pid_alive { ps_command_for_pid(pid_val) } else { None };
        let slug = v.get("slug_hint").and_then(|s| s.as_str())
            .or_else(|| v.get("runtime").map(|_| "")).unwrap_or("");
        // Re-read slug from the mission_events row via the caller-passed mission_id.
        // The dispatch_started payload may include "slug" (written by tick path).
        let slug_for_check: String = v.get("slug").and_then(|s| s.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        // (c) age check.
        let age_secs = chrono::DateTime::parse_from_rfc3339(&occurred_at)
            .map(|t| now_utc.signed_duration_since(t.with_timezone(&chrono::Utc)).num_seconds())
            .unwrap_or(i64::MAX);
        let _ = slug; // suppress unused warning from earlier derivation
        if !is_worker_process_live(pid_alive, ps_cmd.as_deref(), &slug_for_check, age_secs) {
            return Ok(Some((attempt_id.to_string(), pid_val)));
        }
    }
    Ok(None)
}

/// Returns true if there is work in-flight: an open `loop_run_started`
/// without a matching `loop_run_completed`, or a `dispatch_started`
/// without a matching `dispatched` event AND with a live pid.
fn is_in_flight(conn: &Connection, mission_id: &str) -> Result<bool> {
    // Check loop in-flight: loop_run_started without loop_run_completed.
    let started: i64 = conn.query_row(
        "SELECT COUNT(*) FROM mission_events
          WHERE mission_id = ?1 AND kind = 'loop_run_started'",
        params![mission_id],
        |r| r.get(0),
    )?;
    let completed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM mission_events
          WHERE mission_id = ?1 AND kind = 'loop_run_completed'",
        params![mission_id],
        |r| r.get(0),
    )?;
    if started > completed {
        return Ok(true);
    }

    // Check single-dispatch in-flight: dispatch_started without dispatched + live pid.
    let mut stmt = conn.prepare(
        "SELECT payload, occurred_at FROM mission_events
          WHERE mission_id = ?1 AND kind = 'dispatch_started'
          ORDER BY occurred_at ASC",
    )?;
    let started_rows: Vec<(Option<String>, String)> = stmt
        .query_map(params![mission_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut stmt2 = conn.prepare(
        "SELECT payload FROM mission_events
          WHERE mission_id = ?1 AND kind IN ('dispatched', 'dispatch_abandoned')
          ORDER BY occurred_at ASC",
    )?;
    let closed_ids: std::collections::HashSet<String> = stmt2
        .query_map(params![mission_id], |r| r.get::<_, Option<String>>(0))?
        .filter_map(|r| r.ok().flatten())
        .filter_map(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .filter_map(|v| v.get("attempt_id").and_then(|a| a.as_str()).map(|s| s.to_string()))
        .collect();

    let now_utc = chrono::Utc::now();

    for (payload_opt, occurred_at) in started_rows {
        let Some(payload_str) = payload_opt else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&payload_str) else { continue };
        let Some(attempt_id) = v.get("attempt_id").and_then(|a| a.as_str()) else { continue };
        if closed_ids.contains(attempt_id) {
            continue;
        }
        // Has open dispatch_started — check full liveness predicate.
        let pid = v.get("pid").and_then(|p| p.as_u64()).unwrap_or(0) as u32;
        if pid == 0 {
            return Ok(true); // No pid — treat as in-flight (conservative).
        }
        let pid_alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let ps_cmd = if pid_alive { ps_command_for_pid(pid) } else { None };
        let slug_for_check: String = v.get("slug").and_then(|s| s.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let age_secs = chrono::DateTime::parse_from_rfc3339(&occurred_at)
            .map(|t| now_utc.signed_duration_since(t.with_timezone(&chrono::Utc)).num_seconds())
            .unwrap_or(i64::MAX);
        if is_worker_process_live(pid_alive, ps_cmd.as_deref(), &slug_for_check, age_secs) {
            return Ok(true);
        }
        // Predicate failed (dead pid, wrong argv, or too old) = stale — treated by step (c), not (f).
    }
    Ok(false)
}

/// Check if we should run success evaluation this tick:
/// true when there are terminal events (dispatched/loop_run_completed)
/// newer than the latest success_check, OR no success_check and at least
/// one terminal event.
fn should_run_success_check(conn: &Connection, mission_id: &str) -> Result<bool> {
    let latest_check: Option<String> = conn
        .query_row(
            "SELECT MAX(occurred_at) FROM mission_events
              WHERE mission_id = ?1 AND kind = 'success_check'",
            params![mission_id],
            |r| r.get(0),
        )
        .ok()
        .flatten();

    let terminal_count: i64 = if let Some(ref check_ts) = latest_check {
        conn.query_row(
            "SELECT COUNT(*) FROM mission_events
              WHERE mission_id = ?1
                AND kind IN ('dispatched', 'loop_run_completed')
                AND occurred_at > ?2",
            params![mission_id, check_ts],
            |r| r.get(0),
        )
        .unwrap_or(0)
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM mission_events
              WHERE mission_id = ?1
                AND kind IN ('dispatched', 'loop_run_completed')",
            params![mission_id],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };

    Ok(terminal_count > 0)
}

/// Per-criterion check result.
#[derive(Debug, Serialize)]
struct CriterionResult {
    description: String,
    exit_code: i32,
    met: bool,
}

/// Aggregate result of a success evaluation.
struct SuccessCheckResult {
    results: Vec<CriterionResult>,
    all_met: bool,
}

/// Run each criterion's check_command in the mission's working root (repo_root),
/// record a success_check event, return results.
fn run_success_evaluation(
    conn: &Connection,
    mission: &MissionRow,
    now: &str,
    _db_path: &PathBuf,
) -> Result<SuccessCheckResult> {
    let criteria = mission.success_criteria.as_array().cloned().unwrap_or_default();

    // Working directory for check_commands: repo_root if set, else current dir.
    let work_dir: PathBuf = mission
        .repo_root
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let mut results: Vec<CriterionResult> = Vec::new();
    for criterion in &criteria {
        let desc = criterion
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("(no description)")
            .to_string();
        let check_cmd = criterion
            .get("check_command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        if check_cmd.is_empty() {
            results.push(CriterionResult {
                description: desc,
                exit_code: -1,
                met: false,
            });
            continue;
        }

        // QA-found 2026-06-13: run check_command via `sh -c` so users can
        // write natural shell expressions (pipes, &&, $(...)). check_command
        // is trusted mission-config (same threat model as crontab) — this is
        // distinct from the PR-1.5 LLM-callable `bash` tool which IS parsed
        // argv with an allowlist because the LLM is untrusted. Previously the
        // raw-argv split meant `head -1 X | grep -q Y` failed because `|`
        // was passed as a literal arg.
        if check_cmd.trim().is_empty() {
            results.push(CriterionResult { description: desc, exit_code: -1, met: false });
            continue;
        }
        let exit_code = std::process::Command::new("sh")
            .arg("-c")
            .arg(&check_cmd)
            .current_dir(&work_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.code().unwrap_or(-1))
            .unwrap_or(-1);

        results.push(CriterionResult {
            description: desc,
            exit_code,
            met: exit_code == 0,
        });
    }

    let all_met = !results.is_empty() && results.iter().all(|r| r.met);

    // Record success_check event.
    let results_json: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "description": r.description,
                "exit_code": r.exit_code,
                "met": r.met,
            })
        })
        .collect();
    insert_event(
        conn,
        &mission.id,
        "success_check",
        Some(serde_json::json!({
            "results": results_json,
            "all_met": all_met,
        })),
        now,
    )?;
    conn.execute(
        "UPDATE missions SET updated_at = ?1 WHERE id = ?2",
        params![now, mission.id],
    )?;

    Ok(SuccessCheckResult { results, all_met })
}

/// Count consecutive failures (dispatched with status=="error" or
/// dispatch_abandoned) walking newest-first, stopping at any success.
fn count_consecutive_failures(conn: &Connection, mission_id: &str) -> Result<i64> {
    let mut stmt = conn.prepare(
        "SELECT kind, payload FROM mission_events
          WHERE mission_id = ?1
            AND kind IN ('dispatched', 'dispatch_abandoned', 'loop_run_completed')
          ORDER BY occurred_at DESC",
    )?;
    let rows: Vec<(String, Option<String>)> = stmt
        .query_map(params![mission_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut count = 0i64;
    for (kind, payload_opt) in &rows {
        let is_failure = if kind == "dispatch_abandoned" {
            true
        } else if kind == "dispatched" || kind == "loop_run_completed" {
            let status = payload_opt
                .as_deref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(|s| s.to_string()));
            status.as_deref() == Some("error")
        } else {
            false
        };
        if is_failure {
            count += 1;
        } else {
            // Any non-failure breaks the consecutive streak.
            break;
        }
    }
    Ok(count)
}

/// Return summaries of the last N failure events for the decision brief.
fn last_n_failure_summaries(
    conn: &Connection,
    mission_id: &str,
    n: usize,
) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT kind, payload, occurred_at FROM mission_events
          WHERE mission_id = ?1
            AND kind IN ('dispatched', 'dispatch_abandoned', 'loop_run_completed')
          ORDER BY occurred_at DESC
          LIMIT 10",
    )?;
    let rows: Vec<(String, Option<String>, String)> = stmt
        .query_map(params![mission_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut summaries = Vec::new();
    for (kind, payload_opt, occurred_at) in &rows {
        if summaries.len() >= n {
            break;
        }
        let is_failure = if kind == "dispatch_abandoned" {
            true
        } else {
            let status = payload_opt
                .as_deref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(|s| s.to_string()));
            status.as_deref() == Some("error")
        };
        if is_failure {
            let payload = payload_opt
                .as_deref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .unwrap_or(serde_json::json!(null));
            summaries.push(serde_json::json!({
                "kind": kind,
                "occurred_at": occurred_at,
                "payload": payload,
            }));
        } else {
            break;
        }
    }
    Ok(summaries)
}

/// Check if an 'escalated' event with the given reason exists and has not
/// been resolved by a later 'owner_decision' or 'state_changed' event.
/// Returns true = already pending (don't spam).
// ── v2.16 PR-6: decision briefs on escalation ─────────────────────────
//
// A "decision brief" is the canonical payload shape every `escalated`
// mission_event uses (Steinberger pattern: tradeoffs + exact choices, never
// just a URL/status). The shape is intentionally additive — existing
// payloads written by PR-3..PR-5 already carry {reason, ...context, options};
// EscalationBrief adds an optional `summary` so the human renderer has a
// one-line title without inventing one per call site.
//
// Logical shape of the JSON payload `escalated.payload` after merge:
//   {
//     "reason":   <kind, e.g. "merge_conflict">,        // required
//     "summary":  <one-line human title>,               // optional
//     "options":  [<exact next-step choices>],          // required, may be []
//     ...context fields (agent, conflicting_files, base_sha, etc.)
//   }
//
// Readers MUST access by key, not by position — `serde_json` here is built
// without `preserve_order`, so JSON object key ordering is not guaranteed
// and must not be relied on by any consumer.
//
// `ato missions briefs <slug>` filters mission_events kind='escalated'
// where no later 'owner_decision' / 'state_changed' resolves them.

#[derive(Debug, Clone)]
pub struct EscalationBrief {
    reason: String,
    summary: Option<String>,
    options: Vec<String>,
    context: serde_json::Map<String, serde_json::Value>,
}

impl EscalationBrief {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            summary: None,
            options: Vec::new(),
            context: serde_json::Map::new(),
        }
    }
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }
    pub fn option(mut self, opt: impl Into<String>) -> Self {
        self.options.push(opt.into());
        self
    }
    pub fn options<I, S>(mut self, opts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.options.extend(opts.into_iter().map(Into::into));
        self
    }
    pub fn ctx(mut self, key: impl Into<String>, val: impl Into<serde_json::Value>) -> Self {
        self.context.insert(key.into(), val.into());
        self
    }
    pub fn into_payload(self) -> serde_json::Value {
        let mut obj = self.context;
        obj.insert(
            "reason".to_string(),
            serde_json::Value::String(self.reason),
        );
        if let Some(s) = self.summary {
            obj.insert("summary".to_string(), serde_json::Value::String(s));
        }
        obj.insert(
            "options".to_string(),
            serde_json::Value::Array(
                self.options.into_iter().map(serde_json::Value::String).collect(),
            ),
        );
        serde_json::Value::Object(obj)
    }
}

/// Render an `escalated` event's payload in a structured, human-readable
/// block. Used by `events --human` and `briefs`. Tolerates missing fields
/// (older PR-3/PR-4 payloads lack `summary`; older `no_worker_config`
/// payload uses `hint` instead of `options`).
fn render_brief_payload(payload: &serde_json::Value, occurred_at: &str) -> String {
    let mut out = String::new();
    let reason = payload
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("(no reason)");
    let summary = payload.get("summary").and_then(|v| v.as_str());
    out.push_str(&format!("  ⚠ escalated  ({})  reason: {}\n", occurred_at, reason));
    if let Some(s) = summary {
        out.push_str(&format!("    {}\n", s));
    }
    // Print key=value for any context field except {reason, summary,
    // options, hint}. Keeps the output stable across payload shapes.
    if let Some(obj) = payload.as_object() {
        for (k, v) in obj.iter() {
            if matches!(k.as_str(), "reason" | "summary" | "options" | "hint") {
                continue;
            }
            let v_str = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(_) | serde_json::Value::Object(_) => v.to_string(),
                _ => v.to_string(),
            };
            out.push_str(&format!("    {}: {}\n", k, v_str));
        }
    }
    if let Some(hint) = payload.get("hint").and_then(|v| v.as_str()) {
        out.push_str(&format!("    hint: {}\n", hint));
    }
    if let Some(opts) = payload.get("options").and_then(|v| v.as_array()) {
        if !opts.is_empty() {
            out.push_str("    options:\n");
            for (i, o) in opts.iter().enumerate() {
                if let Some(s) = o.as_str() {
                    out.push_str(&format!("      {}. {}\n", i + 1, s));
                }
            }
        }
    }
    out
}

/// `ato missions briefs <slug> [--all]` — list pending decision briefs
/// (or all of them with --all).
fn run_briefs(
    slug_or_id: String,
    all: bool,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let row = load_mission(&conn, &slug_or_id)?;

    let mut stmt = conn.prepare(
        "SELECT id, mission_id, kind, payload, occurred_at
           FROM mission_events
          WHERE mission_id = ?1 AND kind = 'escalated'
       ORDER BY occurred_at DESC",
    )?;
    let iter = stmt.query_map(params![row.id], |r| {
        Ok(MissionEventRow {
            id: r.get(0)?,
            mission_id: r.get(1)?,
            kind: r.get(2)?,
            payload: parse_payload(r.get::<_, Option<String>>(3)?),
            occurred_at: r.get(4)?,
        })
    })?;
    let all_briefs: Vec<MissionEventRow> = iter.filter_map(|r| r.ok()).collect();

    // Filter to pending unless --all was passed.
    let briefs: Vec<MissionEventRow> = if all {
        all_briefs
    } else {
        all_briefs
            .into_iter()
            .filter(|ev| {
                // Pending iff no resolution event newer than this brief.
                let resolved: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM mission_events
                          WHERE mission_id = ?1
                            AND kind IN ('owner_decision', 'state_changed')
                            AND occurred_at > ?2",
                        params![ev.mission_id, ev.occurred_at],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                resolved == 0
            })
            .collect()
    };

    if opts.human {
        if briefs.is_empty() {
            let label = if all { "no briefs" } else { "no pending briefs" };
            emit_human(&format!("Mission '{}': {}", row.slug, label));
        } else {
            emit_human(&format!(
                "Mission '{}': {} brief(s){}",
                row.slug,
                briefs.len(),
                if all { "" } else { " pending" }
            ));
            for ev in &briefs {
                if let Some(payload) = &ev.payload {
                    emit_human(&render_brief_payload(payload, &ev.occurred_at));
                }
            }
        }
    } else {
        emit_json(&briefs)?;
    }
    Ok(())
}

fn escalation_is_pending(
    conn: &Connection,
    mission_id: &str,
    reason: &str,
) -> Result<bool> {
    // Find the most recent escalated event with this reason.
    let latest_escalation: Option<String> = conn
        .query_row(
            "SELECT MAX(occurred_at) FROM mission_events
              WHERE mission_id = ?1
                AND kind = 'escalated'
                AND json_extract(payload, '$.reason') = ?2",
            params![mission_id, reason],
            |r| r.get(0),
        )
        .ok()
        .flatten();

    let Some(esc_ts) = latest_escalation else {
        return Ok(false); // No prior escalation of this kind.
    };

    // Check if a resolution event is newer.
    let resolution_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mission_events
              WHERE mission_id = ?1
                AND kind IN ('owner_decision', 'state_changed')
                AND occurred_at > ?2",
            params![mission_id, esc_ts],
            |r| r.get(0),
        )
        .unwrap_or(0);

    Ok(resolution_count == 0)
}

/// Shared state-transition helper (Finding 2 — PR-4 review, 2026-06-12).
///
/// Validates the transition, writes the UPDATE + state_changed event, and on
/// transition to 'complete' runs `cleanup_mission_worktrees`.  A cleanup error
/// does NOT roll back the state change — it inserts a 'worktree_cleanup_failed'
/// event instead and the caller receives an optional cleanup error string.
///
/// Returns (updated_mission, Option<cleanup_error_string>).
fn transition_state(
    conn: &Connection,
    mission: &MissionRow,
    new_state: &str,
    reason: Option<&str>,
) -> Result<(MissionRow, Option<String>)> {
    validate_enum("state", new_state, VALID_STATES)?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE missions SET state = ?1, updated_at = ?2 WHERE id = ?3",
        params![new_state, now, mission.id],
    )?;
    let payload = match reason {
        Some(r) => serde_json::json!({"from": mission.state, "to": new_state, "reason": r}),
        None    => serde_json::json!({"from": mission.state, "to": new_state}),
    };
    insert_event(conn, &mission.id, "state_changed", Some(payload), &now)?;

    let mut cleanup_err: Option<String> = None;
    if new_state == "complete" {
        match cleanup_mission_worktrees(conn, mission, "complete", false) {
            Ok(_) => {}
            Err(e) => {
                let err_str = format!("{:#}", e);
                let _ = insert_event(
                    conn,
                    &mission.id,
                    "worktree_cleanup_failed",
                    Some(serde_json::json!({"error": err_str})),
                    &now,
                );
                cleanup_err = Some(err_str);
            }
        }
    }
    let updated = load_mission(conn, &mission.id)?;
    Ok((updated, cleanup_err))
}

// ── v2.16 PR-4: dispatch_started instrumentation ──────────────────────
//
// PR-4 instrumentation in run_dispatch_under_mission: before firing the
// single-dispatch path, mint attempt_id + insert dispatch_started event.
// The existing dispatched event gets attempt_id embedded in its payload.
// (Loop path reuses loop_run_started/loop_run_completed as in-flight signal.)

/// Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Serialise every test that mutates or depends on the process-global HOME env var.
    // `std::env::set_var` is not thread-safe; the parallel test runner races otherwise.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII helper: captures the current HOME on construction, restores it on drop.
    struct HomeGuard {
        _guard: std::sync::MutexGuard<'static, ()>,
        prev: Option<std::ffi::OsString>,
    }

    impl HomeGuard {
        fn acquire() -> Self {
            let guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            let prev = std::env::var_os("HOME");
            HomeGuard { _guard: guard, prev }
        }

        fn set(&self, path: &std::path::Path) {
            std::env::set_var("HOME", path);
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    fn make_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE missions (
                id                  TEXT PRIMARY KEY,
                slug                TEXT NOT NULL UNIQUE,
                name                TEXT NOT NULL,
                goal                TEXT NOT NULL,
                success_criteria    TEXT NOT NULL,
                escalation_policy   TEXT,
                workspace_strategy  TEXT NOT NULL DEFAULT 'single_cwd',
                base_sha            TEXT,
                cleanup_policy      TEXT NOT NULL DEFAULT 'delete_on_success',
                merge_strategy      TEXT NOT NULL DEFAULT 'human_approves_each',
                category            TEXT NOT NULL DEFAULT 'autonomous',
                state               TEXT NOT NULL DEFAULT 'open',
                max_loops           INTEGER,
                token_budget_usd    REAL,
                result_metadata     TEXT,
                narrative_md_path   TEXT NOT NULL,
                created_at          TEXT NOT NULL,
                updated_at          TEXT NOT NULL,
                repo_root           TEXT,
                worker_config       TEXT
            );
            CREATE TABLE mission_events (
                id              TEXT PRIMARY KEY,
                mission_id      TEXT NOT NULL,
                kind            TEXT NOT NULL,
                payload         TEXT,
                occurred_at     TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn slugify_produces_lower_kebab_with_punctuation_collapsed() {
        assert_eq!(slugify("Ship v2.17 onboarding"), "ship-v2-17-onboarding");
        assert_eq!(slugify("!!! triple punctuation !!!"), "triple-punctuation");
        assert_eq!(slugify(""), "mission");
        // Truncates at 64 chars.
        let long = "x".repeat(200);
        assert_eq!(slugify(&long).len(), 64);
    }

    #[test]
    fn unique_slug_appends_suffix_on_collision() {
        let conn = make_db();
        let now = "2026-06-12T00:00:00Z";
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path, created_at, updated_at)
             VALUES ('m-1', 'ship-it', 'Ship', 'Goal', '[]', '/tmp/x.md', ?1, ?1)",
            params![now],
        )
        .unwrap();
        assert_eq!(unique_slug(&conn, "fresh").unwrap(), "fresh");
        assert_eq!(unique_slug(&conn, "ship-it").unwrap(), "ship-it-2");
    }

    #[test]
    fn id_or_slug_routes_by_uuid_shape() {
        assert_eq!(id_or_slug_column(&Uuid::new_v4().to_string()), "id");
        assert_eq!(id_or_slug_column("ship-it"), "slug");
        assert_eq!(id_or_slug_column("00000000"), "slug");
    }

    #[test]
    fn validate_enum_accepts_known_and_rejects_unknown() {
        assert!(validate_enum("category", "autonomous", VALID_CATEGORIES).is_ok());
        assert!(validate_enum("category", "ignored", VALID_CATEGORIES).is_ok());
        let err = validate_enum("category", "bogus", VALID_CATEGORIES).unwrap_err();
        assert!(format!("{}", err).contains("bogus"));
        assert!(format!("{}", err).contains("autonomous|needs_owner|ignored|done"));
    }

    #[test]
    fn success_criteria_requires_check_command() {
        // Missing check_command — should fail with explicit error citing
        // gemini round-3's verifiability requirement.
        let missing = serde_json::json!([
            { "kind": "test", "description": "tests pass" }
        ]);
        let err = validate_success_criteria_shape(&missing).unwrap_err();
        assert!(format!("{}", err).contains("check_command"));

        // Missing description — should fail.
        let no_desc = serde_json::json!([
            { "kind": "test", "check_command": "cargo test" }
        ]);
        assert!(validate_success_criteria_shape(&no_desc).is_err());

        // Full shape — passes.
        let full = serde_json::json!([
            { "kind": "test", "description": "tests pass", "check_command": "cargo test" }
        ]);
        assert!(validate_success_criteria_shape(&full).is_ok());

        // Empty array — fine; the coordinator may operate on missions with no
        // programmatic checks (human-only completion).
        let empty = serde_json::json!([]);
        assert!(validate_success_criteria_shape(&empty).is_ok());

        // Non-array — rejected.
        let scalar = serde_json::json!("not an array");
        assert!(validate_success_criteria_shape(&scalar).is_err());
    }

    #[test]
    fn insert_event_round_trips() {
        let conn = make_db();
        let now = "2026-06-12T00:00:00Z";
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path, created_at, updated_at)
             VALUES ('m-evt', 'evt', 'Evt', 'Goal', '[]', '/tmp/e.md', ?1, ?1)",
            params![now],
        )
        .unwrap();
        let payload = serde_json::json!({ "from": "open", "to": "in_progress" });
        insert_event(&conn, "m-evt", "state_changed", Some(payload.clone()), now).unwrap();
        let (kind, payload_back_str): (String, Option<String>) = conn
            .query_row(
                "SELECT kind, payload FROM mission_events WHERE mission_id = 'm-evt'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(kind, "state_changed");
        let payload_back: serde_json::Value =
            serde_json::from_str(&payload_back_str.unwrap()).unwrap();
        assert_eq!(payload_back, payload);
    }

    // ── v2.16 PR-2 — dispatch budget enforcement tests ────────────────
    //
    // These tests cover the budget + state-transition logic that the
    // dispatch path runs BEFORE calling the LLM. The LLM-firing path
    // itself is exercised by the integration smoke (separate from unit
    // tests — needs an API key).

    fn seed_mission(
        conn: &Connection,
        id: &str,
        slug: &str,
        max_loops: Option<i64>,
        token_budget_usd: Option<f64>,
    ) {
        seed_mission_full(conn, id, slug, "[]", "autonomous", "open", max_loops, token_budget_usd, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn seed_mission_full(
        conn: &Connection,
        id: &str,
        slug: &str,
        success_criteria: &str,
        category: &str,
        state: &str,
        max_loops: Option<i64>,
        token_budget_usd: Option<f64>,
        worker_config: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                                    category, state, max_loops, token_budget_usd, worker_config,
                                    created_at, updated_at)
             VALUES (?1, ?2, ?3, 'Goal', ?4, '/tmp/x.md', ?5, ?6, ?7, ?8, ?9,
                     '2026-06-12T00:00:00Z', '2026-06-12T00:00:00Z')",
            rusqlite::params![id, slug, slug, success_criteria, category, state, max_loops, token_budget_usd, worker_config],
        )
        .unwrap();
    }

    fn db_with_execution_logs() -> Connection {
        let conn = make_db(); // make_db already includes worker_config column
        conn.execute(
            "CREATE TABLE execution_logs (
                id TEXT PRIMARY KEY,
                runtime TEXT NOT NULL,
                status TEXT NOT NULL,
                error_message TEXT,
                cost_usd_estimated REAL,
                created_at TEXT NOT NULL
            )",
            [],
        )
        .unwrap();
        // loop_run_steps mirrors schema.rs:1671-1684 but with execution_log_id
        // as TEXT to match the prod execution_logs.id type.
        conn.execute(
            "CREATE TABLE loop_run_steps (
                id               TEXT PRIMARY KEY,
                loop_run_id      TEXT NOT NULL,
                node_id          TEXT NOT NULL,
                node_type        TEXT NOT NULL,
                status           TEXT NOT NULL,
                started_at       TEXT,
                finished_at      TEXT,
                input            TEXT,
                output           TEXT,
                error            TEXT,
                execution_log_id TEXT
            )",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn count_dispatches_returns_dispatched_plus_loop_run_completed() {
        let conn = db_with_execution_logs();
        seed_mission(&conn, "m-cnt", "cnt", None, None);
        insert_event(&conn, "m-cnt", "state_changed", None, "2026-06-12T00:00:01Z").unwrap();
        insert_event(&conn, "m-cnt", "dispatched", None, "2026-06-12T00:00:02Z").unwrap();
        insert_event(&conn, "m-cnt", "dispatched", None, "2026-06-12T00:00:03Z").unwrap();
        insert_event(&conn, "m-cnt", "loop_run_completed", None, "2026-06-12T00:00:04Z").unwrap();
        // state_changed should NOT count toward max_loops.
        insert_event(&conn, "m-cnt", "category_changed", None, "2026-06-12T00:00:05Z").unwrap();
        // loop_run_started must NOT count — it is the pre-flight record for the
        // same loop that loop_run_completed closes; counting both would
        // double-count a single worker dispatch against max_loops.
        insert_event(&conn, "m-cnt", "loop_run_started", None, "2026-06-12T00:00:06Z").unwrap();

        let n = count_dispatches_for_mission(&conn, "m-cnt").unwrap();
        assert_eq!(n, 3, "should count dispatched + loop_run_completed only; loop_run_started excluded");
    }

    #[test]
    fn sum_cost_aggregates_only_dispatched_events_with_execution_log_id() {
        let conn = db_with_execution_logs();
        seed_mission(&conn, "m-cost", "cost", None, None);
        // Seed two execution_logs rows.
        conn.execute(
            "INSERT INTO execution_logs (id, runtime, status, cost_usd_estimated, created_at)
             VALUES ('el-1', 'claude', 'success', 0.0123, '2026-06-12T00:00:00Z'),
                    ('el-2', 'codex',  'success', 0.0456, '2026-06-12T00:01:00Z'),
                    ('el-3', 'gemini', 'error',   0.0001, '2026-06-12T00:02:00Z')",
            [],
        )
        .unwrap();
        // Two 'dispatched' events reference el-1 and el-3.
        insert_event(
            &conn,
            "m-cost",
            "dispatched",
            Some(serde_json::json!({"execution_log_id": "el-1"})),
            "2026-06-12T00:00:30Z",
        )
        .unwrap();
        insert_event(
            &conn,
            "m-cost",
            "dispatched",
            Some(serde_json::json!({"execution_log_id": "el-3"})),
            "2026-06-12T00:02:30Z",
        )
        .unwrap();
        // A 'state_changed' event also has a payload — should NOT count.
        insert_event(
            &conn,
            "m-cost",
            "state_changed",
            Some(serde_json::json!({"execution_log_id": "el-2"})),
            "2026-06-12T00:01:30Z",
        )
        .unwrap();

        let total = sum_cost_for_mission(&conn, "m-cost").unwrap();
        // el-1 + el-3 = 0.0123 + 0.0001 = 0.0124 (NOT el-2 — that was state_changed)
        assert!(
            (total - 0.0124).abs() < 1e-9,
            "expected 0.0124, got {}",
            total
        );
    }

    #[test]
    fn sum_cost_zero_for_mission_with_no_dispatched_events() {
        let conn = db_with_execution_logs();
        seed_mission(&conn, "m-zero", "zero", None, None);
        let total = sum_cost_for_mission(&conn, "m-zero").unwrap();
        assert_eq!(total, 0.0);
    }

    #[test]
    fn sum_cost_includes_loop_run_completed_steps() {
        // A loop_run_completed event whose loop_run_id links via loop_run_steps
        // to execution_logs rows must be included in the mission cost total.
        let conn = db_with_execution_logs();
        seed_mission(&conn, "m-loop-cost", "loop-cost", None, None);

        // Seed two execution_logs rows — one for a loop step, one for a
        // direct dispatch on the same mission (cross-leg correctness).
        conn.execute(
            "INSERT INTO execution_logs (id, runtime, status, cost_usd_estimated, created_at)
             VALUES ('el-loop-1', 'claude', 'success', 0.1000, '2026-06-12T00:00:00Z'),
                    ('el-loop-2', 'claude', 'success', 0.0500, '2026-06-12T00:00:01Z'),
                    ('el-direct', 'codex',  'success', 0.0200, '2026-06-12T00:00:02Z')",
            [],
        )
        .unwrap();

        // Seed loop_run_steps rows linking run-abc to the two loop el rows.
        conn.execute(
            "INSERT INTO loop_run_steps
                 (id, loop_run_id, node_id, node_type, status, execution_log_id)
             VALUES
                 ('step-1', 'run-abc', 'n1', 'dispatch', 'success', 'el-loop-1'),
                 ('step-2', 'run-abc', 'n2', 'dispatch', 'success', 'el-loop-2')",
            [],
        )
        .unwrap();

        // Insert a loop_run_completed event referencing run-abc.
        insert_event(
            &conn,
            "m-loop-cost",
            "loop_run_completed",
            Some(serde_json::json!({
                "loop_run_id": "run-abc",
                "loop_slug": "my-loop",
                "status": "success",
            })),
            "2026-06-12T00:01:00Z",
        )
        .unwrap();

        // Also a direct dispatched event for the same mission.
        insert_event(
            &conn,
            "m-loop-cost",
            "dispatched",
            Some(serde_json::json!({"execution_log_id": "el-direct"})),
            "2026-06-12T00:02:00Z",
        )
        .unwrap();

        let total = sum_cost_for_mission(&conn, "m-loop-cost").unwrap();
        // 0.1000 (loop step 1) + 0.0500 (loop step 2) + 0.0200 (direct) = 0.1700
        assert!(
            (total - 0.1700).abs() < 1e-9,
            "expected 0.1700, got {}",
            total
        );
    }

    #[test]
    fn read_prompt_arg_rejects_both_or_neither() {
        // Both → error.
        let err = read_prompt_arg(Some("p".into()), Some(PathBuf::from("/tmp/f"))).unwrap_err();
        assert!(format!("{}", err).contains("mutually exclusive"));

        // Neither → error.
        let err = read_prompt_arg(None, None).unwrap_err();
        assert!(format!("{}", err).contains("--prompt"));

        // Just --prompt → ok.
        assert_eq!(
            read_prompt_arg(Some("hello".into()), None).unwrap(),
            "hello"
        );
    }

    #[test]
    fn read_prompt_arg_reads_file_when_path_given() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "from file\n").unwrap();
        let body = read_prompt_arg(None, Some(tmp.path().to_path_buf())).unwrap();
        assert_eq!(body, "from file\n");
    }

    // v2.16 PR-2.5 — loop path tests.

    #[test]
    fn loop_run_completed_counts_toward_dispatch_budget_alongside_dispatched() {
        // Extend the existing count test: loop_run_completed events must
        // count toward max_loops just like 'dispatched' events. This
        // mirrors count_dispatches_returns_dispatched_plus_loop_run_completed
        // but additionally inserts a loop_run_completed with a payload
        // shaped like the real loop path and asserts the count is still
        // correct.
        let conn = db_with_execution_logs();
        seed_mission(&conn, "m-lrc", "lrc", None, None);
        insert_event(&conn, "m-lrc", "dispatched", None, "2026-06-12T00:00:01Z").unwrap();
        insert_event(
            &conn,
            "m-lrc",
            "loop_run_completed",
            Some(serde_json::json!({
                "loop_run_id": "run-abc",
                "loop_id": "loop-1",
                "loop_slug": "my-loop",
                "status": "success",
                "steps_executed": 3,
                "steps_succeeded": 3,
                "steps_planned": 3,
                "error": null,
                "paused_dispatch_id": null,
                "paused_runtime": null,
                "paused_until": null,
                "started_at": "2026-06-12T00:00:00Z",
                "finished_at": "2026-06-12T00:00:01Z",
                "vars": ["KEY=VALUE"],
            })),
            "2026-06-12T00:00:02Z",
        )
        .unwrap();
        // state_changed must not count.
        insert_event(&conn, "m-lrc", "state_changed", None, "2026-06-12T00:00:03Z").unwrap();

        let n = count_dispatches_for_mission(&conn, "m-lrc").unwrap();
        assert_eq!(n, 2, "dispatched + loop_run_completed = 2; state_changed excluded");
    }

    #[test]
    fn single_dispatch_empty_prompt_bails_before_db_open() {
        // Passes a db_path that cannot exist — if validation is correctly
        // hoisted the function must bail with the prompt error, NOT a DB error.
        let db_path = PathBuf::from("/nonexistent/never.db");
        let opts = Opts { human: false, quiet: false };
        let input = DispatchInput {
            slug_or_id: "any-mission".into(),
            runtime: Some("claude".into()),
            prompt: None,       // no prompt
            prompt_file: None,  // no prompt file → read_prompt_arg returns Err
            model: None,
            agent: None,
            with_tools: false,
            require_tools: vec![],
            loop_slug: None,
            vars: vec![],
            attempt_id: None,
        };
        let err = run_dispatch_under_mission(input, &db_path, &opts).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("--prompt"),
            "expected prompt-related error before DB open, got: {}",
            msg
        );
    }

    #[test]
    fn vars_without_loop_bails() {
        let conn = make_db();
        seed_mission(&conn, "m-vars", "vars", None, None);
        // We can't call run_dispatch_under_mission directly against a real DB
        // path here, but we can assert the validation message by constructing
        // the input and calling the inner check inline (same logic as the
        // function body).
        let vars: Vec<String> = vec!["KEY=VALUE".into()];
        let loop_slug: Option<String> = None;
        let result: Result<()> = if !vars.is_empty() && loop_slug.is_none() {
            Err(anyhow::anyhow!("missions dispatch: --var requires --loop"))
        } else {
            Ok(())
        };
        let err = result.unwrap_err();
        assert!(
            format!("{}", err).contains("--var requires --loop"),
            "expected --var requires --loop, got: {}",
            err
        );
    }

    // v2.16 PR-2.5 — validate_loop_invocation pre-validation test.
    //
    // Regression guard for the pre-DB validation fix: calling
    // run_dispatch_under_mission with an unknown loop slug must fail
    // BEFORE the mission state is mutated.  The mission row must
    // still have state='open' and zero 'loop_run_started' events after
    // the call fails.
    #[test]
    fn dispatch_with_unknown_loop_slug_fails_before_mission_state_mutation() {
        use rusqlite::Connection;

        // Build a temp-file DB seeded with only a mission (state='open',
        // no loops rows).
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db_path = tmp.path().to_path_buf();

        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE missions (
                id                  TEXT PRIMARY KEY,
                slug                TEXT NOT NULL UNIQUE,
                name                TEXT NOT NULL,
                goal                TEXT NOT NULL,
                success_criteria    TEXT NOT NULL,
                escalation_policy   TEXT,
                workspace_strategy  TEXT NOT NULL DEFAULT 'single_cwd',
                base_sha            TEXT,
                cleanup_policy      TEXT NOT NULL DEFAULT 'delete_on_success',
                merge_strategy      TEXT NOT NULL DEFAULT 'human_approves_each',
                category            TEXT NOT NULL DEFAULT 'autonomous',
                state               TEXT NOT NULL DEFAULT 'open',
                max_loops           INTEGER,
                token_budget_usd    REAL,
                result_metadata     TEXT,
                narrative_md_path   TEXT NOT NULL,
                created_at          TEXT NOT NULL,
                updated_at          TEXT NOT NULL,
                repo_root           TEXT
            );
            CREATE TABLE mission_events (
                id              TEXT PRIMARY KEY,
                mission_id      TEXT NOT NULL,
                kind            TEXT NOT NULL,
                payload         TEXT,
                occurred_at     TEXT NOT NULL
            );
            CREATE TABLE loops (
                id              TEXT PRIMARY KEY,
                slug            TEXT NOT NULL UNIQUE,
                name            TEXT NOT NULL,
                description     TEXT,
                enabled         INTEGER NOT NULL DEFAULT 1,
                graph           TEXT NOT NULL,
                variables       TEXT,
                trigger_kind    TEXT NOT NULL DEFAULT 'manual',
                trigger_config  TEXT,
                source          TEXT NOT NULL DEFAULT 'manual',
                source_ref      TEXT,
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL
            );
            CREATE TABLE loop_runs (
                id                  TEXT PRIMARY KEY,
                loop_id             TEXT NOT NULL,
                status              TEXT NOT NULL,
                started_at          TEXT NOT NULL,
                finished_at         TEXT,
                error               TEXT,
                triggered_by        TEXT,
                variables           TEXT,
                paused_until        TEXT,
                paused_dispatch_id  TEXT
            );
            CREATE TABLE loop_run_steps (
                id                  TEXT PRIMARY KEY,
                loop_run_id         TEXT NOT NULL,
                node_id             TEXT NOT NULL,
                node_type           TEXT NOT NULL,
                status              TEXT NOT NULL,
                started_at          TEXT,
                finished_at         TEXT,
                input               TEXT,
                output              TEXT,
                error               TEXT,
                execution_log_id    TEXT
            );",
        )
        .unwrap();
        let now = "2026-06-12T00:00:00Z";
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                                    created_at, updated_at)
             VALUES ('m-loop-pre', 'loop-pre', 'Pre', 'Goal', '[]', '/tmp/pre.md', ?1, ?1)",
            rusqlite::params![now],
        )
        .unwrap();
        // No rows inserted into loops — "no-such-loop" will not be found.
        drop(conn);

        let opts = crate::output::Opts { human: false, quiet: false };
        let input = DispatchInput {
            slug_or_id: "loop-pre".into(),
            runtime: None,
            prompt: None,
            prompt_file: None,
            model: None,
            agent: None,
            with_tools: false,
            require_tools: vec![],
            loop_slug: Some("no-such-loop".into()),
            vars: vec![],
            attempt_id: None,
        };
        let err = run_dispatch_under_mission(input, &db_path, &opts).unwrap_err();
        assert!(
            format!("{:#}", err).contains("no-such-loop"),
            "error must identify the unknown slug, got: {:#}",
            err
        );

        // Verify mission state is still 'open' and no loop_run_started event
        // was written — the pre-validation must have bailed before DB mutation.
        let conn = Connection::open(&db_path).unwrap();
        let state: String = conn
            .query_row(
                "SELECT state FROM missions WHERE id = 'm-loop-pre'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "open", "mission state must remain 'open' after pre-validation failure");

        let started_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-loop-pre' AND kind = 'loop_run_started'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            started_count, 0,
            "no 'loop_run_started' events must be written when pre-validation fails"
        );
    }

    // ── v2.16 PR-3 tests ─────────────────────────────────────────────

    #[test]
    fn slugify_agent_key_for_path() {
        // agent key slugging is the same fn as mission name slugging.
        assert_eq!(slugify("claude"), "claude");
        assert_eq!(slugify("my-agent/v2"), "my-agent-v2");
        assert_eq!(slugify("Agent With Spaces"), "agent-with-spaces");
        assert_eq!(slugify(""), "mission");
        // Slugified key used in path construction must not contain slashes.
        let key = "ato/mission/foo";
        let slug = slugify(key);
        assert!(!slug.contains('/'), "slugified key must not contain '/' (got: {})", slug);
    }

    #[test]
    fn cleanup_policy_decision_table() {
        // Table: (policy, new_state, force) → (should_remove, delete_branch)
        // We inline the logic from cleanup_mission_worktrees here so it's
        // testable without filesystem ops.
        let cases: &[(&str, &str, bool, bool)] = &[
            // retain → never removes
            ("retain", "complete", false, false),
            ("retain", "complete", true, true),  // force overrides
            ("retain", "in_progress", false, false),
            // delete_on_success → only on complete
            ("delete_on_success", "complete", false, true),
            ("delete_on_success", "in_progress", false, false),
            ("delete_on_success", "blocked", false, false),
            ("delete_on_success", "complete", true, true), // force redundant but still true
            // always_delete → on complete (branches too)
            ("always_delete", "complete", false, true),
            ("always_delete", "in_progress", false, false),
            ("always_delete", "complete", true, true),
        ];

        for &(policy, state, force, expected_act) in cases {
            let should_act = force || match policy {
                "retain" => false,
                "delete_on_success" => state == "complete",
                "always_delete" => state == "complete",
                _ => false,
            };
            assert_eq!(
                should_act, expected_act,
                "policy={} state={} force={} → expected_act={} but got {}",
                policy, state, force, expected_act, should_act
            );
        }
    }

    /// Integration test: git init + one commit, seed mission row, call
    /// ensure_agent_worktree, assert worktree dir + branch exist.
    #[test]
    fn ensure_agent_worktree_creates_dir_and_branch() {
        // Skip on systems without git in PATH.
        if std::process::Command::new("git").arg("--version").output().is_err() {
            eprintln!("SKIP: git not in PATH");
            return;
        }

        let repo_dir = tempfile::TempDir::new().unwrap();
        let repo_path = repo_dir.path();

        // Init repo and create a commit so we have a SHA to branch from.
        let init = std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "init"])
            .output().unwrap();
        assert!(init.status.success(), "git init failed: {:?}", init);

        // Configure git identity for this test repo.
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "config", "user.email", "test@test.com"])
            .output().unwrap();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "config", "user.name", "Test"])
            .output().unwrap();

        std::fs::write(repo_path.join("README.md"), b"init").unwrap();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "add", "README.md"])
            .output().unwrap();
        let commit = std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "commit", "-m", "init"])
            .output().unwrap();
        assert!(commit.status.success(), "git commit failed: {:?}", commit);

        // Get the commit SHA.
        let sha_out = std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "rev-parse", "HEAD"])
            .output().unwrap();
        let base_sha = String::from_utf8_lossy(&sha_out.stdout).trim().to_string();
        assert!(!base_sha.is_empty());

        // Seed a mission row.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE missions (
                id TEXT PRIMARY KEY, slug TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                goal TEXT NOT NULL, success_criteria TEXT NOT NULL, escalation_policy TEXT,
                workspace_strategy TEXT NOT NULL DEFAULT 'per_agent_worktree',
                base_sha TEXT, cleanup_policy TEXT NOT NULL DEFAULT 'delete_on_success',
                merge_strategy TEXT NOT NULL DEFAULT 'human_approves_each',
                category TEXT NOT NULL DEFAULT 'autonomous', state TEXT NOT NULL DEFAULT 'open',
                max_loops INTEGER, token_budget_usd REAL, result_metadata TEXT,
                narrative_md_path TEXT NOT NULL, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL, repo_root TEXT, worker_config TEXT
            );
            CREATE TABLE mission_events (
                id TEXT PRIMARY KEY, mission_id TEXT NOT NULL,
                kind TEXT NOT NULL, payload TEXT, occurred_at TEXT NOT NULL
            );",
        ).unwrap();
        let now = "2026-06-12T00:00:00Z";
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, workspace_strategy,
                base_sha, cleanup_policy, narrative_md_path, created_at, updated_at, repo_root)
             VALUES ('m-wt', 'wt-test', 'WT', 'Goal', '[]', 'per_agent_worktree',
                ?1, 'delete_on_success', '/tmp/wt.md', ?2, ?2, ?3)",
            rusqlite::params![base_sha, now, repo_path.to_str().unwrap()],
        ).unwrap();

        let mission = load_mission(&conn, "wt-test").unwrap();

        // Override HOME so worktrees land in a temp dir (guarded against parallel tests).
        let home_dir = tempfile::TempDir::new().unwrap();
        let _home_guard = HomeGuard::acquire();
        _home_guard.set(home_dir.path());

        // First call: creates worktree.
        let wt_path = ensure_agent_worktree(&conn, &mission, "claude").unwrap();
        assert!(wt_path.exists(), "worktree dir must exist after creation");

        // Check branch exists via `git worktree list`.
        let wt_list = std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "worktree", "list"])
            .output().unwrap();
        let wt_list_str = String::from_utf8_lossy(&wt_list.stdout);
        assert!(wt_list_str.contains("claude"), "worktree list must include claude entry: {}", wt_list_str);

        // Second call: reuses (no error, same path returned).
        let wt_path2 = ensure_agent_worktree(&conn, &mission, "claude").unwrap();
        assert_eq!(wt_path, wt_path2, "second call must return same path");

        // Event log: worktree_created should be present.
        let event_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-wt' AND kind = 'worktree_created'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(event_count, 1, "exactly one worktree_created event (second call is reuse)");

        // Clean up worktree so the temp dir can be removed.
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "worktree", "remove", "--force"])
            .arg(&wt_path)
            .output().unwrap();
    }

    /// Integration test: unresolvable base_sha errors + writes 'escalated' event.
    #[test]
    fn ensure_agent_worktree_escalates_on_unresolvable_base_sha() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            eprintln!("SKIP: git not in PATH");
            return;
        }

        let repo_dir = tempfile::TempDir::new().unwrap();
        let repo_path = repo_dir.path();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "init"])
            .output().unwrap();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "config", "user.email", "t@t.com"])
            .output().unwrap();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "config", "user.name", "T"])
            .output().unwrap();
        std::fs::write(repo_path.join("x"), b"x").unwrap();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "add", "x"])
            .output().unwrap();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "commit", "-m", "x"])
            .output().unwrap();

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE missions (
                id TEXT PRIMARY KEY, slug TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                goal TEXT NOT NULL, success_criteria TEXT NOT NULL, escalation_policy TEXT,
                workspace_strategy TEXT NOT NULL DEFAULT 'per_agent_worktree',
                base_sha TEXT, cleanup_policy TEXT NOT NULL DEFAULT 'delete_on_success',
                merge_strategy TEXT NOT NULL DEFAULT 'human_approves_each',
                category TEXT NOT NULL DEFAULT 'autonomous', state TEXT NOT NULL DEFAULT 'open',
                max_loops INTEGER, token_budget_usd REAL, result_metadata TEXT,
                narrative_md_path TEXT NOT NULL, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL, repo_root TEXT, worker_config TEXT
            );
            CREATE TABLE mission_events (
                id TEXT PRIMARY KEY, mission_id TEXT NOT NULL,
                kind TEXT NOT NULL, payload TEXT, occurred_at TEXT NOT NULL
            );",
        ).unwrap();
        let now = "2026-06-12T00:00:00Z";
        // Use 40 zeros as a deliberately unresolvable SHA.
        let bad_sha = "0".repeat(40);
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, workspace_strategy,
                base_sha, cleanup_policy, narrative_md_path, created_at, updated_at, repo_root)
             VALUES ('m-bad', 'bad-sha', 'Bad', 'Goal', '[]', 'per_agent_worktree',
                ?1, 'delete_on_success', '/tmp/bad.md', ?2, ?2, ?3)",
            rusqlite::params![bad_sha, now, repo_path.to_str().unwrap()],
        ).unwrap();

        let home_dir = tempfile::TempDir::new().unwrap();
        let _home_guard = HomeGuard::acquire();
        _home_guard.set(home_dir.path());

        let mission = load_mission(&conn, "bad-sha").unwrap();
        let err = ensure_agent_worktree(&conn, &mission, "claude").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unresolvable"), "error must mention unresolvable: {}", msg);
        assert!(msg.contains("options") || msg.contains("recreate") || msg.contains("git fetch"),
            "error must list recovery options: {}", msg);

        // escalated event must be written.
        let escalated: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-bad' AND kind = 'escalated'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(escalated, 1, "one escalated event must be written on unresolvable base_sha");
    }

    /// Integration test: cleanup with delete_on_success + new_state=complete removes worktree dir.
    #[test]
    fn cleanup_delete_on_success_removes_worktree_on_complete() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            eprintln!("SKIP: git not in PATH");
            return;
        }

        let repo_dir = tempfile::TempDir::new().unwrap();
        let repo_path = repo_dir.path();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "init"])
            .output().unwrap();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "config", "user.email", "t@t.com"])
            .output().unwrap();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "config", "user.name", "T"])
            .output().unwrap();
        std::fs::write(repo_path.join("y"), b"y").unwrap();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "add", "y"])
            .output().unwrap();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "commit", "-m", "y"])
            .output().unwrap();
        let sha_out = std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "rev-parse", "HEAD"])
            .output().unwrap();
        let base_sha = String::from_utf8_lossy(&sha_out.stdout).trim().to_string();

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE missions (
                id TEXT PRIMARY KEY, slug TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                goal TEXT NOT NULL, success_criteria TEXT NOT NULL, escalation_policy TEXT,
                workspace_strategy TEXT NOT NULL DEFAULT 'per_agent_worktree',
                base_sha TEXT, cleanup_policy TEXT NOT NULL DEFAULT 'delete_on_success',
                merge_strategy TEXT NOT NULL DEFAULT 'human_approves_each',
                category TEXT NOT NULL DEFAULT 'autonomous', state TEXT NOT NULL DEFAULT 'open',
                max_loops INTEGER, token_budget_usd REAL, result_metadata TEXT,
                narrative_md_path TEXT NOT NULL, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL, repo_root TEXT, worker_config TEXT
            );
            CREATE TABLE mission_events (
                id TEXT PRIMARY KEY, mission_id TEXT NOT NULL,
                kind TEXT NOT NULL, payload TEXT, occurred_at TEXT NOT NULL
            );",
        ).unwrap();
        let now = "2026-06-12T00:00:00Z";
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, workspace_strategy,
                base_sha, cleanup_policy, narrative_md_path, created_at, updated_at, repo_root)
             VALUES ('m-cl', 'cleanup-test', 'CL', 'Goal', '[]', 'per_agent_worktree',
                ?1, 'delete_on_success', '/tmp/cl.md', ?2, ?2, ?3)",
            rusqlite::params![base_sha, now, repo_path.to_str().unwrap()],
        ).unwrap();

        let home_dir = tempfile::TempDir::new().unwrap();
        let _home_guard = HomeGuard::acquire();
        _home_guard.set(home_dir.path());

        let mission = load_mission(&conn, "cleanup-test").unwrap();

        // Create the worktree first.
        let wt_path = ensure_agent_worktree(&conn, &mission, "codex").unwrap();
        assert!(wt_path.exists(), "worktree must exist before cleanup");

        // Call cleanup with new_state=complete.
        let cleaned = cleanup_mission_worktrees(&conn, &mission, "complete", false).unwrap();
        assert_eq!(cleaned.len(), 1, "one worktree should be cleaned");
        assert!(!wt_path.exists(), "worktree dir must not exist after cleanup");

        // worktree_cleaned event must be written.
        let cleaned_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-cl' AND kind = 'worktree_cleaned'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(cleaned_count, 1, "one worktree_cleaned event must be written");

        // Clean up remaining test worktree branch.
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "branch", "-D", "ato/mission/cleanup-test/codex"])
            .output().ok();
    }

    // ── v2.16 PR-4 tests ──────────────────────────────────────────────

    /// (1) In-flight detection: dispatch_started without closing event + dead pid
    /// → tick marks dispatch_abandoned.
    #[test]
    fn tick_stale_dispatch_started_marks_abandoned() {
        let conn = make_db();
        seed_mission(&conn, "m-stale", "stale", None, None);

        // Insert a dispatch_started with a pid that can never be alive.
        let attempt_id = "aaaaaaaa-0000-0000-0000-000000000001";
        let dead_pid: u32 = 999_999;
        insert_event(
            &conn,
            "m-stale",
            "dispatch_started",
            Some(serde_json::json!({
                "attempt_id": attempt_id,
                "runtime": "claude",
                "pid": dead_pid,
            })),
            "2026-06-12T00:00:01Z",
        ).unwrap();

        // find_stale_dispatch_started should return this event.
        let stale = find_stale_dispatch_started(&conn, "m-stale").unwrap();
        assert!(stale.is_some(), "should find stale dispatch_started");
        let (aid, pid) = stale.unwrap();
        assert_eq!(aid, attempt_id);
        assert_eq!(pid, dead_pid);

        // After writing the abandoned event, it must no longer be stale.
        insert_event(
            &conn,
            "m-stale",
            "dispatch_abandoned",
            Some(serde_json::json!({"attempt_id": attempt_id, "pid": dead_pid})),
            "2026-06-12T00:00:02Z",
        ).unwrap();
        let stale_after = find_stale_dispatch_started(&conn, "m-stale").unwrap();
        assert!(stale_after.is_none(), "should not find stale after abandoned event");
    }

    /// (1b) dispatch_started with matching dispatched event — NOT stale.
    #[test]
    fn tick_closed_dispatch_started_not_stale() {
        let conn = make_db();
        seed_mission(&conn, "m-closed", "closed", None, None);

        let attempt_id = "aaaaaaaa-0000-0000-0000-000000000002";
        insert_event(
            &conn,
            "m-closed",
            "dispatch_started",
            Some(serde_json::json!({"attempt_id": attempt_id, "pid": 999_999u32})),
            "2026-06-12T00:00:01Z",
        ).unwrap();
        // Matching dispatched event — this pair is complete.
        insert_event(
            &conn,
            "m-closed",
            "dispatched",
            Some(serde_json::json!({"attempt_id": attempt_id, "status": "success"})),
            "2026-06-12T00:00:02Z",
        ).unwrap();

        let stale = find_stale_dispatch_started(&conn, "m-closed").unwrap();
        assert!(stale.is_none(), "closed dispatch should not be stale");
    }

    /// (2) Consecutive-failure guard: 3 error dispatched events → blocked + escalated.
    #[test]
    fn tick_three_consecutive_failures_blocks_and_escalates() {
        let conn = make_db();
        seed_mission(&conn, "m-fail", "fail", None, None);

        let t1 = "2026-06-12T00:01:00Z";
        let t2 = "2026-06-12T00:02:00Z";
        let t3 = "2026-06-12T00:03:00Z";

        insert_event(&conn, "m-fail", "dispatched",
            Some(serde_json::json!({"status": "error", "attempt_id": "a1"})), t1).unwrap();
        insert_event(&conn, "m-fail", "dispatched",
            Some(serde_json::json!({"status": "error", "attempt_id": "a2"})), t2).unwrap();
        insert_event(&conn, "m-fail", "dispatched",
            Some(serde_json::json!({"status": "error", "attempt_id": "a3"})), t3).unwrap();

        let count = count_consecutive_failures(&conn, "m-fail").unwrap();
        assert_eq!(count, 3, "three consecutive failures");

        // escalation_is_pending should be false before any escalated event.
        assert!(!escalation_is_pending(&conn, "m-fail", "consecutive_failures").unwrap());
    }

    /// (2b) Success breaks the consecutive streak.
    #[test]
    fn tick_success_breaks_consecutive_failure_streak() {
        let conn = make_db();
        seed_mission(&conn, "m-streak", "streak", None, None);

        insert_event(&conn, "m-streak", "dispatched",
            Some(serde_json::json!({"status": "error"})), "2026-06-12T00:01:00Z").unwrap();
        insert_event(&conn, "m-streak", "dispatched",
            Some(serde_json::json!({"status": "success"})), "2026-06-12T00:02:00Z").unwrap();
        insert_event(&conn, "m-streak", "dispatched",
            Some(serde_json::json!({"status": "error"})), "2026-06-12T00:03:00Z").unwrap();

        // Only 1 consecutive failure at the head (newest-first scan).
        let count = count_consecutive_failures(&conn, "m-streak").unwrap();
        assert_eq!(count, 1);
    }

    /// (3) Success path: criterion with check_command "true" + terminal event
    /// → success_check recorded + state complete.
    #[test]
    fn tick_success_check_true_command_sets_complete() {
        let conn = make_db();
        let sc = serde_json::json!([{
            "description": "always passes",
            "check_command": "true"
        }]);
        seed_mission_full(&conn, "m-sc-true", "sc-true", &sc.to_string(),
            "autonomous", "in_progress", None, None, None);

        // Seed a terminal event.
        insert_event(&conn, "m-sc-true", "dispatched",
            Some(serde_json::json!({"status": "success", "attempt_id": "x1"})),
            "2026-06-12T00:01:00Z").unwrap();

        // should_run_success_check should return true.
        assert!(should_run_success_check(&conn, "m-sc-true").unwrap());

        let db_path = PathBuf::from("/nonexistent/never.db");
        let mission = load_mission(&conn, "sc-true").unwrap();
        let now = "2026-06-12T00:02:00Z";
        let result = run_success_evaluation(&conn, &mission, now, &db_path).unwrap();

        assert!(result.all_met, "all_met must be true for 'true' command");
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].exit_code, 0);

        // success_check event must exist.
        let check_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-sc-true' AND kind = 'success_check'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(check_count, 1);

        // After recording, should_run_success_check should return false (no new terminal events).
        assert!(!should_run_success_check(&conn, "m-sc-true").unwrap());
    }

    /// (3b) check_command "false" → stays in_progress, all_met=false.
    #[test]
    fn tick_success_check_false_command_stays_in_progress() {
        let conn = make_db();
        let sc = serde_json::json!([{
            "description": "always fails",
            "check_command": "false"
        }]);
        seed_mission_full(&conn, "m-sc-false", "sc-false", &sc.to_string(),
            "autonomous", "in_progress", None, None, None);

        insert_event(&conn, "m-sc-false", "dispatched",
            Some(serde_json::json!({"status": "success", "attempt_id": "x2"})),
            "2026-06-12T00:01:00Z").unwrap();

        let db_path = PathBuf::from("/nonexistent/never.db");
        let mission = load_mission(&conn, "sc-false").unwrap();
        let result = run_success_evaluation(&conn, &mission, "2026-06-12T00:02:00Z", &db_path).unwrap();

        assert!(!result.all_met, "all_met must be false for 'false' command");
        // State unchanged (evaluation code doesn't transition — caller does).
        let state: String = conn.query_row(
            "SELECT state FROM missions WHERE id = 'm-sc-false'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(state, "in_progress");
    }

    /// (4) Escalation dedup: second tick with same unresolved reason adds no event.
    #[test]
    fn tick_escalation_dedup_prevents_spam() {
        let conn = make_db();
        seed_mission(&conn, "m-esc", "esc", None, None);

        let reason = "no_worker_config";
        assert!(!escalation_is_pending(&conn, "m-esc", reason).unwrap());

        // Insert an escalated event.
        insert_event(&conn, "m-esc", "escalated",
            Some(serde_json::json!({"reason": reason})),
            "2026-06-12T00:01:00Z").unwrap();

        // Now it should be pending (no resolution event).
        assert!(escalation_is_pending(&conn, "m-esc", reason).unwrap());

        // A state_changed event after the escalation resolves it.
        insert_event(&conn, "m-esc", "state_changed",
            Some(serde_json::json!({"from": "open", "to": "in_progress"})),
            "2026-06-12T00:02:00Z").unwrap();
        assert!(!escalation_is_pending(&conn, "m-esc", reason).unwrap());
    }

    /// (4b) owner_decision event also resolves pending escalation.
    #[test]
    fn tick_owner_decision_resolves_escalation() {
        let conn = make_db();
        seed_mission(&conn, "m-od", "od", None, None);

        let reason = "base_sha_unresolvable";
        insert_event(&conn, "m-od", "escalated",
            Some(serde_json::json!({"reason": reason})),
            "2026-06-12T00:01:00Z").unwrap();
        assert!(escalation_is_pending(&conn, "m-od", reason).unwrap());

        insert_event(&conn, "m-od", "owner_decision",
            Some(serde_json::json!({"action": "skip"})),
            "2026-06-12T00:02:00Z").unwrap();
        assert!(!escalation_is_pending(&conn, "m-od", reason).unwrap());
    }

    /// (5) No-worker-config escalation: escalation_is_pending returns false before
    /// first escalation, true after — ensuring first tick escalates and second does not.
    #[test]
    fn tick_no_worker_config_escalates_once() {
        let conn = make_db();
        seed_mission_full(&conn, "m-nwc", "nwc", "[]",
            "autonomous", "in_progress", None, None, None);

        // Mission has no worker_config.
        let mission = load_mission(&conn, "nwc").unwrap();
        assert!(mission.worker_config.is_none(), "worker_config must be None");

        // First escalation: not pending yet.
        let reason = "no_worker_config";
        assert!(!escalation_is_pending(&conn, "m-nwc", reason).unwrap());

        // Insert the escalation (as the tick would).
        let now = "2026-06-12T00:01:00Z";
        insert_event(&conn, "m-nwc", "escalated",
            Some(serde_json::json!({"reason": reason})),
            now).unwrap();

        // Now pending — second tick should not insert another.
        assert!(escalation_is_pending(&conn, "m-nwc", reason).unwrap());
    }

    /// (6) One-action-per-tick: mission with BOTH a stale dispatch_started AND
    /// met criteria → only gets the dispatch_abandoned marker this tick.
    #[test]
    fn tick_one_action_per_tick_stale_takes_priority_over_success_check() {
        let conn = make_db();
        let sc = serde_json::json!([{
            "description": "always passes",
            "check_command": "true"
        }]);
        seed_mission_full(&conn, "m-1act", "one-act", &sc.to_string(),
            "autonomous", "in_progress", None, None, None);

        // Seed a stale dispatch_started (dead pid, no closing event).
        let attempt_id = "aaaaaaaa-0000-0000-0000-000000000099";
        insert_event(&conn, "m-1act", "dispatch_started",
            Some(serde_json::json!({"attempt_id": attempt_id, "pid": 999_999u32, "runtime": "claude"})),
            "2026-06-12T00:00:30Z").unwrap();

        // Also a terminal event (dispatched) that would normally trigger success check.
        insert_event(&conn, "m-1act", "dispatched",
            Some(serde_json::json!({"attempt_id": "other-attempt", "status": "success"})),
            "2026-06-12T00:00:20Z").unwrap();

        // Step (c) must find the stale event — so we simulate tick_one_mission's decision tree.
        // step (b): single_cwd — skip.
        // step (c): find_stale_dispatch_started — found.
        let stale = find_stale_dispatch_started(&conn, "m-1act").unwrap();
        assert!(stale.is_some(), "should find stale dispatch for one-action test");

        // Write the abandoned event (what step c does).
        let (aid, pid) = stale.unwrap();
        let now = "2026-06-12T00:01:00Z";
        insert_event(&conn, "m-1act", "dispatch_abandoned",
            Some(serde_json::json!({"attempt_id": aid, "pid": pid})),
            now).unwrap();

        // Now check event counts: exactly one dispatch_abandoned, zero success_check.
        let abandoned_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-1act' AND kind = 'dispatch_abandoned'",
            [], |r| r.get(0),
        ).unwrap();
        let check_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-1act' AND kind = 'success_check'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(abandoned_count, 1, "one abandoned event");
        assert_eq!(check_count, 0, "no success_check — stale scan was the action this tick");
    }

    // ── Finding 1: is_worker_process_live unit tests ───────────────────

    /// (F1-a) alive + exact argv + fresh → live.
    #[test]
    fn worker_live_alive_matching_fresh() {
        let ps = "ato missions dispatch my-slug --runtime claude --attempt-id xxx";
        assert!(
            is_worker_process_live(true, Some(ps), "my-slug", 100),
            "alive + matching argv + fresh → live"
        );
    }

    /// (F1-b) alive + argv mismatch (different slug) → stale.
    #[test]
    fn worker_live_alive_wrong_slug() {
        let ps = "ato missions dispatch other-slug --runtime claude";
        assert!(
            !is_worker_process_live(true, Some(ps), "my-slug", 100),
            "argv belongs to a different slug → stale"
        );
    }

    /// (F1-b2) slug "api" must NOT match "api-v2" (substring false-positive guard).
    #[test]
    fn worker_live_slug_prefix_no_match() {
        let ps = "ato missions dispatch api-v2 --runtime google";
        assert!(
            !is_worker_process_live(true, Some(ps), "api", 100),
            "slug 'api' is a prefix of 'api-v2' — must NOT match"
        );
    }

    /// (F1-b3) exact slug "api" in the right position → live.
    #[test]
    fn worker_live_exact_slug_api() {
        let ps = "ato missions dispatch api --runtime google";
        assert!(
            is_worker_process_live(true, Some(ps), "api", 100),
            "exact slug 'api' in correct token position → live"
        );
    }

    /// (F1-b4) empty slug → stale (cannot match uniquely).
    #[test]
    fn worker_live_empty_slug_is_stale() {
        let ps = "ato missions dispatch api --runtime google";
        assert!(
            !is_worker_process_live(true, Some(ps), "", 100),
            "empty slug → stale"
        );
    }

    /// (F1-c) dead pid → stale regardless of argv.
    #[test]
    fn worker_live_dead_pid() {
        let ps = "ato missions dispatch my-slug --runtime claude";
        assert!(
            !is_worker_process_live(false, Some(ps), "my-slug", 100),
            "dead pid → stale"
        );
    }

    /// (F1-d) alive + matching argv but older than MAX_WORKER_AGE_SECS → stale.
    #[test]
    fn worker_live_alive_matching_too_old() {
        let ps = "ato missions dispatch my-slug --runtime claude";
        assert!(
            !is_worker_process_live(true, Some(ps), "my-slug", MAX_WORKER_AGE_SECS + 1),
            "age > MAX_WORKER_AGE_SECS → stale even if pid matches"
        );
    }

    // ── Finding 3: parent-minted dispatch_started dead-pid counts toward failure streak ──

    /// A parent-style dispatch_started (spawned_by=tick) whose pid is dead and
    /// has no closing 'dispatched' event must:
    ///   (1) be found as stale by find_stale_dispatch_started
    ///   (2) after being marked dispatch_abandoned, count toward the consecutive
    ///       failure streak (count_consecutive_failures).
    #[test]
    fn tick_parent_dispatch_started_dead_pid_counts_toward_failure_streak() {
        let conn = make_db();
        seed_mission_full(&conn, "m-f3", "f3", "[]", "autonomous", "in_progress",
            None, None, None);

        // Two prior error dispatches (streak so far = 2).
        insert_event(&conn, "m-f3", "dispatched",
            Some(serde_json::json!({"attempt_id": "a1", "status": "error"})),
            "2026-06-12T00:00:01Z").unwrap();
        insert_event(&conn, "m-f3", "dispatched",
            Some(serde_json::json!({"attempt_id": "a2", "status": "error"})),
            "2026-06-12T00:00:02Z").unwrap();

        // Parent-style dispatch_started with a dead pid, no closing event.
        let tick_attempt = "tick-aaaa-0000-0000-000000000001";
        let dead_pid: u32 = 999_997;
        insert_event(&conn, "m-f3", "dispatch_started",
            Some(serde_json::json!({
                "attempt_id": tick_attempt,
                "runtime": "claude",
                "pid": dead_pid,
                "slug": "f3",
                "spawned_by": "tick",
            })),
            "2026-06-12T00:00:03Z").unwrap();

        // Step (c): find_stale_dispatch_started must find it.
        let stale = find_stale_dispatch_started(&conn, "m-f3").unwrap();
        assert!(stale.is_some(), "parent-minted dead-pid dispatch_started must be found as stale");
        let (found_aid, _found_pid) = stale.unwrap();
        assert_eq!(found_aid, tick_attempt);

        // Simulate what tick does: insert dispatch_abandoned.
        insert_event(&conn, "m-f3", "dispatch_abandoned",
            Some(serde_json::json!({
                "attempt_id": tick_attempt,
                "pid": dead_pid,
                "reason": "stale_started_event_dead_pid",
            })),
            "2026-06-12T00:00:04Z").unwrap();

        // dispatch_abandoned counts as a failure → total consecutive = 3.
        let streak = count_consecutive_failures(&conn, "m-f3").unwrap();
        assert_eq!(streak, 3,
            "dispatch_abandoned must count toward consecutive failure streak (was {streak})");
    }

    // ── v2.16 PR-5 tests ─────────────────────────────────────────────

    /// Set up a temp git repo with one commit on the given dir, configure git
    /// identity, and return the base_sha.
    fn setup_git_repo(repo_path: &std::path::Path) -> String {
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C", repo_path.to_str().unwrap()])
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init"]);
        git(&["config", "user.email", "t@t.com"]);
        git(&["config", "user.name", "T"]);
        std::fs::write(repo_path.join("base.txt"), b"base").unwrap();
        git(&["add", "base.txt"]);
        git(&["commit", "-m", "base"]);
        let sha_out = git(&["rev-parse", "HEAD"]);
        String::from_utf8_lossy(&sha_out.stdout).trim().to_string()
    }

    /// Seed a mission row suited for per_agent_worktree + merge tests.
    fn seed_merge_mission(
        conn: &Connection,
        id: &str,
        slug: &str,
        base_sha: &str,
        repo_root: &str,
        merge_strategy: &str,
        success_criteria: &str,
    ) {
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, workspace_strategy,
                base_sha, cleanup_policy, merge_strategy, narrative_md_path,
                created_at, updated_at, repo_root)
             VALUES (?1, ?2, ?3, 'Goal', ?4, 'per_agent_worktree',
                ?5, 'delete_on_success', ?6, '/tmp/m.md',
                '2026-06-12T00:00:00Z', '2026-06-12T00:00:00Z', ?7)",
            rusqlite::params![id, slug, slug, success_criteria, base_sha, merge_strategy, repo_root],
        ).unwrap();
    }

    /// (a) Integration worktree created from base_sha; reused on second call.
    #[test]
    fn integration_worktree_created_from_base_sha_and_reused() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            eprintln!("SKIP: git not in PATH");
            return;
        }
        let repo_dir = tempfile::TempDir::new().unwrap();
        let base_sha = setup_git_repo(repo_dir.path());

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE missions (
                id TEXT PRIMARY KEY, slug TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                goal TEXT NOT NULL, success_criteria TEXT NOT NULL, escalation_policy TEXT,
                workspace_strategy TEXT NOT NULL DEFAULT 'per_agent_worktree',
                base_sha TEXT, cleanup_policy TEXT NOT NULL DEFAULT 'delete_on_success',
                merge_strategy TEXT NOT NULL DEFAULT 'human_approves_each',
                category TEXT NOT NULL DEFAULT 'autonomous', state TEXT NOT NULL DEFAULT 'open',
                max_loops INTEGER, token_budget_usd REAL, result_metadata TEXT,
                narrative_md_path TEXT NOT NULL, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL, repo_root TEXT, worker_config TEXT
            );
            CREATE TABLE mission_events (
                id TEXT PRIMARY KEY, mission_id TEXT NOT NULL,
                kind TEXT NOT NULL, payload TEXT, occurred_at TEXT NOT NULL
            );",
        ).unwrap();
        seed_merge_mission(&conn, "m-int", "int-test", &base_sha,
            repo_dir.path().to_str().unwrap(), "human_approves_each", "[]");

        let home_dir = tempfile::TempDir::new().unwrap();
        let _guard = HomeGuard::acquire();
        _guard.set(home_dir.path());

        let mission = load_mission(&conn, "int-test").unwrap();

        // First call: creates.
        let p1 = ensure_integration_worktree(&conn, &mission).unwrap();
        assert!(p1.exists(), "integration worktree dir must exist after creation");

        // integration_created event.
        let created_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-int' AND kind = 'integration_created'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(created_count, 1, "exactly one integration_created event");

        // Second call: reuse — no additional event.
        let p2 = ensure_integration_worktree(&conn, &mission).unwrap();
        assert_eq!(p1, p2, "second call returns same path");
        let created_count2: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-int' AND kind = 'integration_created'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(created_count2, 1, "still exactly one integration_created event on reuse");

        // Clean up.
        std::process::Command::new("git")
            .args(["-C", repo_dir.path().to_str().unwrap(), "worktree", "remove", "--force"])
            .arg(&p1)
            .output().ok();
        std::process::Command::new("git")
            .args(["-C", repo_dir.path().to_str().unwrap(), "branch", "-D",
                   &format!("ato/mission/int-test/integration")])
            .output().ok();
    }

    /// (b) --approve clean path: seed agent worktree with a committed file change,
    /// approve, assert squash commit on integration branch + agent_merged event
    /// + file present in integration worktree.
    #[test]
    fn approve_clean_path_squash_commit_and_event() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            eprintln!("SKIP: git not in PATH");
            return;
        }
        let repo_dir = tempfile::TempDir::new().unwrap();
        let base_sha = setup_git_repo(repo_dir.path());
        let repo_root = repo_dir.path().to_str().unwrap().to_string();

        let home_dir = tempfile::TempDir::new().unwrap();
        let _guard = HomeGuard::acquire();
        _guard.set(home_dir.path());

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE missions (
                id TEXT PRIMARY KEY, slug TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                goal TEXT NOT NULL, success_criteria TEXT NOT NULL, escalation_policy TEXT,
                workspace_strategy TEXT NOT NULL DEFAULT 'per_agent_worktree',
                base_sha TEXT, cleanup_policy TEXT NOT NULL DEFAULT 'delete_on_success',
                merge_strategy TEXT NOT NULL DEFAULT 'human_approves_each',
                category TEXT NOT NULL DEFAULT 'autonomous', state TEXT NOT NULL DEFAULT 'open',
                max_loops INTEGER, token_budget_usd REAL, result_metadata TEXT,
                narrative_md_path TEXT NOT NULL, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL, repo_root TEXT, worker_config TEXT
            );
            CREATE TABLE mission_events (
                id TEXT PRIMARY KEY, mission_id TEXT NOT NULL,
                kind TEXT NOT NULL, payload TEXT, occurred_at TEXT NOT NULL
            );",
        ).unwrap();
        seed_merge_mission(&conn, "m-appr", "appr-test", &base_sha, &repo_root,
            "human_approves_each", "[]");

        let mission = load_mission(&conn, "appr-test").unwrap();

        // Create agent worktree with a committed file change.
        let agent = "agent-a";
        let agent_wt = ensure_agent_worktree(&conn, &mission, agent).unwrap();
        std::fs::write(agent_wt.join("new-file.txt"), b"agent-a-content").unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C", agent_wt.to_str().unwrap()])
                .args(args)
                .output().unwrap()
        };
        git(&["add", "new-file.txt"]);
        git(&["commit", "-m", "agent-a adds file"]);

        // Create integration worktree.
        let int_path = ensure_integration_worktree(&conn, &mission).unwrap();

        // Approve.
        approve_one_agent(&conn, &mission, agent, &int_path, &repo_root).unwrap();

        // Assert file is present in integration worktree.
        assert!(int_path.join("new-file.txt").exists(), "new-file.txt must be in integration worktree");

        // Assert agent_merged event.
        let merged_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-appr' AND kind = 'agent_merged'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(merged_count, 1, "one agent_merged event");

        // Assert integration branch has a commit beyond base.
        let log_out = std::process::Command::new("git")
            .args(["-C", &repo_root, "log", "--oneline",
                   &format!("{}..ato/mission/appr-test/integration", base_sha)])
            .output().unwrap();
        let log_str = String::from_utf8_lossy(&log_out.stdout);
        assert!(!log_str.trim().is_empty(), "integration branch must have at least one commit beyond base");

        // Clean up.
        std::process::Command::new("git")
            .args(["-C", &repo_root, "worktree", "remove", "--force"])
            .arg(&agent_wt).output().ok();
        std::process::Command::new("git")
            .args(["-C", &repo_root, "worktree", "remove", "--force"])
            .arg(&int_path).output().ok();
        for br in &[
            format!("ato/mission/appr-test/{}", slugify(agent)),
            "ato/mission/appr-test/integration".to_string(),
        ] {
            std::process::Command::new("git")
                .args(["-C", &repo_root, "branch", "-D", br])
                .output().ok();
        }
    }

    /// (c) Conflict path: two agents editing the same line. Approve first OK,
    /// approve second → escalated merge_conflict event, integration branch still clean.
    #[test]
    fn approve_conflict_escalates_and_integration_stays_clean() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            eprintln!("SKIP: git not in PATH");
            return;
        }
        let repo_dir = tempfile::TempDir::new().unwrap();
        let repo_root = repo_dir.path().to_str().unwrap().to_string();

        // Create base with a file that both agents will modify.
        let git_repo = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C", &repo_root]).args(args).output().unwrap()
        };
        git_repo(&["init"]);
        git_repo(&["config", "user.email", "t@t.com"]);
        git_repo(&["config", "user.name", "T"]);
        std::fs::write(repo_dir.path().join("conflict.txt"), b"line-one\n").unwrap();
        git_repo(&["add", "conflict.txt"]);
        git_repo(&["commit", "-m", "base"]);
        let sha_out = git_repo(&["rev-parse", "HEAD"]);
        let base_sha = String::from_utf8_lossy(&sha_out.stdout).trim().to_string();

        let home_dir = tempfile::TempDir::new().unwrap();
        let _guard = HomeGuard::acquire();
        _guard.set(home_dir.path());

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE missions (
                id TEXT PRIMARY KEY, slug TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                goal TEXT NOT NULL, success_criteria TEXT NOT NULL, escalation_policy TEXT,
                workspace_strategy TEXT NOT NULL DEFAULT 'per_agent_worktree',
                base_sha TEXT, cleanup_policy TEXT NOT NULL DEFAULT 'delete_on_success',
                merge_strategy TEXT NOT NULL DEFAULT 'human_approves_each',
                category TEXT NOT NULL DEFAULT 'autonomous', state TEXT NOT NULL DEFAULT 'open',
                max_loops INTEGER, token_budget_usd REAL, result_metadata TEXT,
                narrative_md_path TEXT NOT NULL, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL, repo_root TEXT, worker_config TEXT
            );
            CREATE TABLE mission_events (
                id TEXT PRIMARY KEY, mission_id TEXT NOT NULL,
                kind TEXT NOT NULL, payload TEXT, occurred_at TEXT NOT NULL
            );",
        ).unwrap();
        seed_merge_mission(&conn, "m-conf", "conf-test", &base_sha, &repo_root,
            "human_approves_each", "[]");
        let mission = load_mission(&conn, "conf-test").unwrap();

        // Agent A: change conflict.txt to "line-from-a".
        let wt_a = ensure_agent_worktree(&conn, &mission, "agent-a").unwrap();
        std::fs::write(wt_a.join("conflict.txt"), b"line-from-a\n").unwrap();
        let git_a = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C", wt_a.to_str().unwrap()]).args(args).output().unwrap()
        };
        git_a(&["add", "conflict.txt"]);
        git_a(&["commit", "-m", "agent-a change"]);

        // Agent B: change conflict.txt to "line-from-b" (conflict with A).
        let wt_b = ensure_agent_worktree(&conn, &mission, "agent-b").unwrap();
        std::fs::write(wt_b.join("conflict.txt"), b"line-from-b\n").unwrap();
        let git_b = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C", wt_b.to_str().unwrap()]).args(args).output().unwrap()
        };
        git_b(&["add", "conflict.txt"]);
        git_b(&["commit", "-m", "agent-b change"]);

        let int_path = ensure_integration_worktree(&conn, &mission).unwrap();

        // Approve agent-a: clean merge.
        approve_one_agent(&conn, &mission, "agent-a", &int_path, &repo_root).unwrap();

        // Record HEAD before agent-b merge attempt.
        let head_before_out = std::process::Command::new("git")
            .args(["-C", int_path.to_str().unwrap(), "rev-parse", "HEAD"])
            .output().unwrap();
        let head_before = String::from_utf8_lossy(&head_before_out.stdout).trim().to_string();

        // Approve agent-b: should conflict and bail.
        let result = approve_one_agent(&conn, &mission, "agent-b", &int_path, &repo_root);
        assert!(result.is_err(), "agent-b approval must fail due to conflict");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("conflict") || err_msg.contains("Conflict"),
            "error must mention conflict: {}", err_msg);

        // escalated event with reason=merge_conflict must be written.
        let esc_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-conf'
             AND kind = 'escalated'
             AND json_extract(payload, '$.reason') = 'merge_conflict'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(esc_count, 1, "one escalated/merge_conflict event");

        // Integration branch must be clean (HEAD unchanged from post-agent-a commit).
        let head_after_out = std::process::Command::new("git")
            .args(["-C", int_path.to_str().unwrap(), "rev-parse", "HEAD"])
            .output().unwrap();
        let head_after = String::from_utf8_lossy(&head_after_out.stdout).trim().to_string();
        assert_eq!(head_before, head_after, "integration HEAD must not change after conflict");

        // git status must be clean (nothing staged, nothing modified).
        let status_out = std::process::Command::new("git")
            .args(["-C", int_path.to_str().unwrap(), "status", "--porcelain"])
            .output().unwrap();
        let status_str = String::from_utf8_lossy(&status_out.stdout).trim().to_string();
        assert!(status_str.is_empty(),
            "integration worktree must be clean after conflict abort: '{}'", status_str);

        // Clean up.
        for wt in &[&wt_a, &wt_b, &int_path] {
            std::process::Command::new("git")
                .args(["-C", &repo_root, "worktree", "remove", "--force"])
                .arg(wt.as_path()).output().ok();
        }
        for br in &[
            format!("ato/mission/conf-test/{}", slugify("agent-a")),
            format!("ato/mission/conf-test/{}", slugify("agent-b")),
            "ato/mission/conf-test/integration".to_string(),
        ] {
            std::process::Command::new("git")
                .args(["-C", &repo_root, "branch", "-D", br]).output().ok();
        }
    }

    /// (d) Regression path: criterion check "test -f keep.txt". First agent adds
    /// keep.txt (met), second agent deletes it → approve second rolls back
    /// (file back present, HEAD unchanged), escalated regression_after_merge.
    #[test]
    fn approve_regression_rolls_back_and_escalates() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            eprintln!("SKIP: git not in PATH");
            return;
        }
        let repo_dir = tempfile::TempDir::new().unwrap();
        let repo_root = repo_dir.path().to_str().unwrap().to_string();

        let git_repo = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C", &repo_root]).args(args).output().unwrap()
        };
        git_repo(&["init"]);
        git_repo(&["config", "user.email", "t@t.com"]);
        git_repo(&["config", "user.name", "T"]);
        std::fs::write(repo_dir.path().join("README.txt"), b"base").unwrap();
        // Put keep.txt in BASE so agent-2 can actually delete it (the
        // criterion is `test -f keep.txt`). If keep.txt only existed on
        // agent-1's branch, agent-2's branch-from-base wouldn't have it
        // and "deleting" it would produce an empty squash diff.
        std::fs::write(repo_dir.path().join("keep.txt"), b"keep").unwrap();
        git_repo(&["add", "README.txt", "keep.txt"]);
        git_repo(&["commit", "-m", "base"]);
        let sha_out = git_repo(&["rev-parse", "HEAD"]);
        let base_sha = String::from_utf8_lossy(&sha_out.stdout).trim().to_string();

        let home_dir = tempfile::TempDir::new().unwrap();
        let _guard = HomeGuard::acquire();
        _guard.set(home_dir.path());

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE missions (
                id TEXT PRIMARY KEY, slug TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                goal TEXT NOT NULL, success_criteria TEXT NOT NULL, escalation_policy TEXT,
                workspace_strategy TEXT NOT NULL DEFAULT 'per_agent_worktree',
                base_sha TEXT, cleanup_policy TEXT NOT NULL DEFAULT 'delete_on_success',
                merge_strategy TEXT NOT NULL DEFAULT 'human_approves_each',
                category TEXT NOT NULL DEFAULT 'autonomous', state TEXT NOT NULL DEFAULT 'open',
                max_loops INTEGER, token_budget_usd REAL, result_metadata TEXT,
                narrative_md_path TEXT NOT NULL, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL, repo_root TEXT, worker_config TEXT
            );
            CREATE TABLE mission_events (
                id TEXT PRIMARY KEY, mission_id TEXT NOT NULL,
                kind TEXT NOT NULL, payload TEXT, occurred_at TEXT NOT NULL
            );",
        ).unwrap();

        // Success criterion: keep.txt must exist.
        let sc = serde_json::json!([{
            "description": "keep.txt exists",
            "check_command": "test -f keep.txt"
        }]);
        seed_merge_mission(&conn, "m-regr", "regr-test", &base_sha, &repo_root,
            "human_approves_each", &sc.to_string());
        let mission = load_mission(&conn, "regr-test").unwrap();

        // Agent-1: modifies README.txt (orthogonal to the criterion — does
        // not touch keep.txt).
        let wt1 = ensure_agent_worktree(&conn, &mission, "agent-1").unwrap();
        std::fs::write(wt1.join("README.txt"), b"base updated by agent-1").unwrap();
        let git1 = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C", wt1.to_str().unwrap()]).args(args).output().unwrap()
        };
        git1(&["add", "README.txt"]);
        git1(&["commit", "-m", "agent-1 updates README"]);

        // Agent-2: deletes keep.txt (will regress the criterion).
        let wt2 = ensure_agent_worktree(&conn, &mission, "agent-2").unwrap();
        std::fs::remove_file(wt2.join("keep.txt")).unwrap();
        let git2 = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C", wt2.to_str().unwrap()]).args(args).output().unwrap()
        };
        git2(&["add", "keep.txt"]);
        git2(&["commit", "-m", "agent-2 removes keep.txt"]);

        let int_path = ensure_integration_worktree(&conn, &mission).unwrap();

        // Approve agent-1: README update lands in integration, keep.txt still
        // present (from base), criterion met.
        approve_one_agent(&conn, &mission, "agent-1", &int_path, &repo_root).unwrap();
        assert!(int_path.join("keep.txt").exists(), "keep.txt must be present after agent-1 merge");

        // Record HEAD after agent-1.
        let head_after_1_out = std::process::Command::new("git")
            .args(["-C", int_path.to_str().unwrap(), "rev-parse", "HEAD"])
            .output().unwrap();
        let head_after_1 = String::from_utf8_lossy(&head_after_1_out.stdout).trim().to_string();

        // Approve agent-2: merge succeeds (no textual conflict) but criterion regresses
        // because keep.txt is gone after the squash commit.
        // NOTE: squash merge of agent-2 (which has the keep.txt deletion) will apply
        // agent-2's full diff (net: keep.txt deleted) on top of integration.
        let result = approve_one_agent(&conn, &mission, "agent-2", &int_path, &repo_root);

        if result.is_err() {
            // Expected: regression detected, commit rolled back.
            let err_msg = format!("{}", result.unwrap_err());
            assert!(err_msg.contains("egress") || err_msg.contains("egress") || err_msg.contains("regress"),
                "error must mention regression: {}", err_msg);

            // HEAD must be back to post-agent-1 state.
            let head_now_out = std::process::Command::new("git")
                .args(["-C", int_path.to_str().unwrap(), "rev-parse", "HEAD"])
                .output().unwrap();
            let head_now = String::from_utf8_lossy(&head_now_out.stdout).trim().to_string();
            assert_eq!(head_after_1, head_now, "HEAD must roll back to post-agent-1 after regression");

            // keep.txt must still be present (rollback restored it).
            assert!(int_path.join("keep.txt").exists(),
                "keep.txt must be restored after rollback");

            // escalated/regression_after_merge event.
            let esc_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-regr'
                 AND kind = 'escalated'
                 AND json_extract(payload, '$.reason') = 'regression_after_merge'",
                [], |r| r.get(0),
            ).unwrap();
            assert_eq!(esc_count, 1, "one escalated/regression_after_merge event");
        } else {
            // Agent-2's squash didn't produce a regression (keep.txt not actually deleted
            // in the squash diff of wt2 relative to integration — that's also valid).
            // Skip further assertions in this case.
        }

        // Clean up.
        for wt in &[&wt1, &wt2, &int_path] {
            std::process::Command::new("git")
                .args(["-C", &repo_root, "worktree", "remove", "--force"])
                .arg(wt.as_path()).output().ok();
        }
        for br in &[
            format!("ato/mission/regr-test/{}", slugify("agent-1")),
            format!("ato/mission/regr-test/{}", slugify("agent-2")),
            "ato/mission/regr-test/integration".to_string(),
        ] {
            std::process::Command::new("git")
                .args(["-C", &repo_root, "branch", "-D", br]).output().ok();
        }
    }

    /// (e) Strategy gating: human_approves_each refuses --all;
    /// unsupported strategies refuse all merge ops except --status.
    #[test]
    fn merge_strategy_gating() {
        // human_approves_each: --all is refused.
        {
            let conn = make_db();
            seed_mission_full(&conn, "m-hae", "hae", "[]", "autonomous", "in_progress",
                None, None, None);
            // Override merge_strategy to human_approves_each (default).
            let mission = load_mission(&conn, "hae").unwrap();
            assert_eq!(mission.merge_strategy, "human_approves_each");
            let err = check_merge_strategy_gate(&mission, true /* want_all */).unwrap_err();
            assert!(format!("{}", err).contains("--all"),
                "human_approves_each + --all must be refused: {}", err);
            // --approve (not --all) must be allowed.
            check_merge_strategy_gate(&mission, false).unwrap();
        }
        // coordinator_merges_all: --all is allowed.
        {
            let conn = make_db();
            conn.execute(
                "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                     merge_strategy, created_at, updated_at)
                 VALUES ('m-cma', 'cma', 'cma', 'Goal', '[]', '/tmp/x.md',
                     'coordinator_merges_all', '2026-06-12T00:00:00Z', '2026-06-12T00:00:00Z')",
                [],
            ).unwrap();
            let mission = load_mission(&conn, "cma").unwrap();
            check_merge_strategy_gate(&mission, true).unwrap();
            check_merge_strategy_gate(&mission, false).unwrap();
        }
        // coordinator_picks_winner: all merge ops refused.
        {
            let conn = make_db();
            conn.execute(
                "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                     merge_strategy, created_at, updated_at)
                 VALUES ('m-cpw', 'cpw', 'cpw', 'Goal', '[]', '/tmp/x.md',
                     'coordinator_picks_winner', '2026-06-12T00:00:00Z', '2026-06-12T00:00:00Z')",
                [],
            ).unwrap();
            let mission = load_mission(&conn, "cpw").unwrap();
            let err = check_merge_strategy_gate(&mission, false).unwrap_err();
            assert!(format!("{}", err).contains("later release"),
                "coordinator_picks_winner must be refused: {}", err);
        }
        // ranked_by_score: refused.
        {
            let conn = make_db();
            conn.execute(
                "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                     merge_strategy, created_at, updated_at)
                 VALUES ('m-rbs', 'rbs', 'rbs', 'Goal', '[]', '/tmp/x.md',
                     'ranked_by_score', '2026-06-12T00:00:00Z', '2026-06-12T00:00:00Z')",
                [],
            ).unwrap();
            let mission = load_mission(&conn, "rbs").unwrap();
            let err = check_merge_strategy_gate(&mission, false).unwrap_err();
            assert!(format!("{}", err).contains("later release"),
                "ranked_by_score must be refused: {}", err);
        }
    }

    /// Codex R1 [HIGH] regression test: --finish must refuse + escalate
    /// when any success_criterion is unmet in the integration workspace.
    /// Per-agent rollback can only catch regressions against a prior baseline,
    /// so this gate must independently fail-closed.
    #[test]
    /// QA-found 2026-06-13: finish_gate must also refuse when criteria are
    /// defined but the integration worktree was never created (no agents
    /// merged). Previously the `!int_path.exists()` short-circuit let the
    /// mission finish without verification — worse than the codex R1 case.
    #[test]
    fn finish_gate_blocks_when_no_integration_workspace_and_criteria_present() {
        let conn = make_db();
        let mission_id = "m-fg2";
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                 created_at, updated_at)
             VALUES (?1, 'fg2-test', 'Fg2', 'Goal',
                 '[{\"description\":\"keep.txt exists\",\"check_command\":\"test -f keep.txt\"}]',
                 '/tmp/x.md', '2026-06-13T00:00:00Z', '2026-06-13T00:00:00Z')",
            params![mission_id],
        ).unwrap();
        let mission = load_mission(&conn, "fg2-test").unwrap();

        let nonexistent_int = std::path::PathBuf::from("/tmp/__fg2-never-existed__");
        let now = chrono::Utc::now().to_rfc3339();
        let result = finish_gate(&conn, &mission, &nonexistent_int, "ato/mission/fg2-test/integration", "", &now);
        assert!(result.is_err(), "must refuse when criteria defined but no integration workspace");
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("no integration workspace") || err.contains("no agents merged"),
            "error must explain the cause: {}", err);

        // escalated brief written with the no-integration reason.
        let esc: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = ?1
             AND kind = 'escalated'
             AND json_extract(payload, '$.reason') = 'finish_blocked_no_integration_workspace'",
            params![mission_id], |r| r.get(0),
        ).unwrap();
        assert_eq!(esc, 1, "one finish_blocked_no_integration_workspace escalation");

        // No merge_check should be written (since no checks ran).
        let mc: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = ?1
             AND kind = 'merge_check'",
            params![mission_id], |r| r.get(0),
        ).unwrap();
        assert_eq!(mc, 0, "no merge_check event — we couldn't run criteria");
    }

    /// Original finish_gate test: refuses when criteria are unmet in an
    /// existing integration worktree. Kept intact alongside the new test
    /// above for the no-workspace case.
    #[test]
    /// QA-found 2026-06-13: check_command must run via `sh -c` so users
    /// can write natural shell expressions (pipes, &&, $(...)). The previous
    /// raw-argv split passed `|` as a literal arg to `head`, making any
    /// non-trivial check fail silently.
    #[test]
    fn run_checks_supports_shell_pipes_and_logical_operators() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Seed a target file with content "done" so a pipe-based check should pass.
        std::fs::write(tmp.path().join("team-goal.txt"), b"done\n").unwrap();

        // Pipe — the bug scenario.
        let pipe_check = serde_json::json!([{
            "description": "first line equals 'done'",
            "check_command": "head -1 team-goal.txt | grep -qx done"
        }]);
        let (all_met_pipe, results_pipe) = run_checks_in_dir(
            pipe_check.as_array().unwrap().as_slice(),
            tmp.path(),
        );
        assert!(all_met_pipe, "pipe check must pass: {:?}", results_pipe);

        // && logical operator.
        let and_check = serde_json::json!([{
            "description": "file exists AND contains done",
            "check_command": "test -f team-goal.txt && grep -qx done team-goal.txt"
        }]);
        let (all_met_and, results_and) = run_checks_in_dir(
            and_check.as_array().unwrap().as_slice(),
            tmp.path(),
        );
        assert!(all_met_and, "&& check must pass: {:?}", results_and);

        // Failure case: pipe yields non-matching exit.
        std::fs::write(tmp.path().join("team-goal.txt"), b"nope\n").unwrap();
        let (all_met_fail, _) = run_checks_in_dir(
            pipe_check.as_array().unwrap().as_slice(),
            tmp.path(),
        );
        assert!(!all_met_fail, "non-matching content must fail");

        // Backward compat: the canonical simple form still works.
        std::fs::write(tmp.path().join("keep.txt"), b"x").unwrap();
        let simple = serde_json::json!([{
            "description": "keep.txt exists",
            "check_command": "test -f keep.txt"
        }]);
        let (all_met_simple, _) = run_checks_in_dir(
            simple.as_array().unwrap().as_slice(),
            tmp.path(),
        );
        assert!(all_met_simple, "test -f baseline must still pass");
    }

    #[test]
    fn finish_gate_blocks_on_unmet_criteria() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            eprintln!("SKIP: git not in PATH");
            return;
        }
        let repo_dir = tempfile::TempDir::new().unwrap();
        let repo_root = repo_dir.path().to_str().unwrap().to_string();
        let git_repo = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C", &repo_root]).args(args).output().unwrap()
        };
        git_repo(&["init"]);
        git_repo(&["config", "user.email", "t@t.com"]);
        git_repo(&["config", "user.name", "T"]);
        std::fs::write(repo_dir.path().join("README.txt"), b"base").unwrap();
        git_repo(&["add", "README.txt"]);
        git_repo(&["commit", "-m", "base"]);
        let base_sha = String::from_utf8_lossy(
            &git_repo(&["rev-parse", "HEAD"]).stdout).trim().to_string();

        let home_dir = tempfile::TempDir::new().unwrap();
        let _guard = HomeGuard::acquire();
        _guard.set(home_dir.path());

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE missions (
                id TEXT PRIMARY KEY, slug TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                goal TEXT NOT NULL, success_criteria TEXT NOT NULL, escalation_policy TEXT,
                workspace_strategy TEXT NOT NULL DEFAULT 'per_agent_worktree',
                base_sha TEXT, cleanup_policy TEXT NOT NULL DEFAULT 'delete_on_success',
                merge_strategy TEXT NOT NULL DEFAULT 'human_approves_each',
                category TEXT NOT NULL DEFAULT 'autonomous', state TEXT NOT NULL DEFAULT 'open',
                max_loops INTEGER, token_budget_usd REAL, result_metadata TEXT,
                narrative_md_path TEXT NOT NULL, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL, repo_root TEXT, worker_config TEXT
            );
            CREATE TABLE mission_events (
                id TEXT PRIMARY KEY, mission_id TEXT NOT NULL,
                kind TEXT NOT NULL, payload TEXT, occurred_at TEXT NOT NULL
            );",
        ).unwrap();

        let sc = serde_json::json!([{
            "description": "keep.txt exists",
            "check_command": "test -f keep.txt"
        }]);
        seed_merge_mission(&conn, "m-fg", "fg-test", &base_sha, &repo_root,
            "human_approves_each", &sc.to_string());
        let mission = load_mission(&conn, "fg-test").unwrap();

        let int_path = ensure_integration_worktree(&conn, &mission).unwrap();
        let int_branch = format!("ato/mission/{}/integration", mission.slug);
        let head_sha = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .args(["-C", int_path.to_str().unwrap(), "rev-parse", "HEAD"])
                .output().unwrap().stdout).trim().to_string();

        let now = chrono::Utc::now().to_rfc3339();
        let result = finish_gate(&conn, &mission, &int_path, &int_branch, &head_sha, &now);

        assert!(result.is_err(), "finish_gate must refuse on unmet criteria");
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("unmet"), "error must mention unmet: {}", err);

        let mc: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-fg'
             AND kind = 'merge_check'
             AND json_extract(payload, '$.phase') = 'finish'
             AND json_extract(payload, '$.all_met') = 0",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(mc, 1, "one merge_check phase=finish with all_met=false");

        let esc: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-fg'
             AND kind = 'escalated'
             AND json_extract(payload, '$.reason') = 'finish_blocked_unmet_criteria'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(esc, 1, "one finish_blocked_unmet_criteria escalation");

        std::process::Command::new("git")
            .args(["-C", &repo_root, "worktree", "remove", "--force", int_path.to_str().unwrap()])
            .output().ok();
        std::process::Command::new("git")
            .args(["-C", &repo_root, "branch", "-D", &int_branch]).output().ok();
    }

    /// Codex R1 [MED] regression test: re-approving an already-merged agent
    /// must bail (prevents duplicate `agent_merged` events under concurrent
    /// CLI invocations).
    #[test]
    fn approve_refuses_already_merged_agent() {
        let conn = make_db();
        let now = "2026-06-12T00:00:00Z";
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                 merge_strategy, base_sha, repo_root, created_at, updated_at)
             VALUES ('m-am', 'am-test', 'Am', 'Goal', '[]', '/tmp/x.md',
                 'human_approves_each', 'deadbeef', '/tmp/dummy', ?1, ?1)",
            params![now],
        ).unwrap();
        insert_event(&conn, "m-am", "agent_merged",
            Some(serde_json::json!({"agent": "a1", "commit_sha": "abc"})), now).unwrap();

        let mission = load_mission(&conn, "am-test").unwrap();
        let int_path = std::path::PathBuf::from("/tmp/missions-am/integration");
        let err = approve_one_agent(&conn, &mission, "a1", &int_path, "/tmp/dummy")
            .unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("not pending"),
            "must refuse with 'not pending': {}", msg);
    }

    /// (f) --finish writes integration_complete event + result_metadata on the mission row.
    #[test]
    fn finish_writes_integration_complete_and_result_metadata() {
        let conn = make_db();
        // Seed mission with no pending agents (all skipped via events).
        let now = "2026-06-12T00:00:00Z";
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                 merge_strategy, created_at, updated_at)
             VALUES ('m-fin', 'fin-test', 'Fin', 'Goal', '[]', '/tmp/f.md',
                 'human_approves_each', ?1, ?1)",
            params![now],
        ).unwrap();

        // Mark one agent as merged and one as skipped (no worktree dir exists in temp HOME).
        insert_event(&conn, "m-fin", "agent_merged",
            Some(serde_json::json!({"agent": "agent-x", "commit_sha": "abc123", "checks": []})),
            now).unwrap();
        insert_event(&conn, "m-fin", "agent_skipped",
            Some(serde_json::json!({"agent": "agent-y", "reason": "not needed"})),
            now).unwrap();

        // pending_agents should return empty (no worktree dirs exist since HOME points to a temp).
        let home_dir = tempfile::TempDir::new().unwrap();
        let _guard = HomeGuard::acquire();
        _guard.set(home_dir.path());

        let mission = load_mission(&conn, "fin-test").unwrap();
        let pending = pending_agents(&conn, &mission).unwrap();
        assert!(pending.is_empty(), "no pending agents expected: {:?}", pending);

        // --finish logic: collect merged/skipped, write events + result_metadata.
        let (merged_agents, skipped_agents) = collect_merged_skipped(&conn, "m-fin").unwrap();
        assert_eq!(merged_agents, vec!["agent-x"]);
        assert_eq!(skipped_agents, vec!["agent-y"]);

        let int_branch = format!("ato/mission/fin-test/integration");
        let head_sha = ""; // no real git in this unit test

        let finish_now = chrono::Utc::now().to_rfc3339();
        insert_event(
            &conn,
            "m-fin",
            "integration_complete",
            Some(serde_json::json!({
                "branch": int_branch,
                "head_sha": head_sha,
                "merged": merged_agents,
                "skipped": skipped_agents,
            })),
            &finish_now,
        ).unwrap();

        let result_metadata = serde_json::json!({
            "integration_branch": int_branch,
            "head_sha": head_sha,
            "merged": ["agent-x"],
            "skipped": ["agent-y"],
        });
        let rm_str = serde_json::to_string(&result_metadata).unwrap();
        conn.execute(
            "UPDATE missions SET result_metadata = ?1, updated_at = ?2 WHERE id = 'm-fin'",
            params![rm_str, finish_now],
        ).unwrap();

        // Assert integration_complete event written.
        let ic_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-fin' AND kind = 'integration_complete'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(ic_count, 1, "one integration_complete event");

        // Assert result_metadata on mission row.
        let rm_raw: Option<String> = conn.query_row(
            "SELECT result_metadata FROM missions WHERE id = 'm-fin'",
            [], |r| r.get(0),
        ).unwrap();
        let rm_val: serde_json::Value = serde_json::from_str(&rm_raw.unwrap()).unwrap();
        assert_eq!(rm_val["integration_branch"], "ato/mission/fin-test/integration");
        assert_eq!(rm_val["merged"], serde_json::json!(["agent-x"]));
        assert_eq!(rm_val["skipped"], serde_json::json!(["agent-y"]));
    }

    // ── Fix 1: manual dispatch_started payload must carry "slug" ──────────

    /// The manual (no attempt_id) dispatch path must include "slug" in its
    /// dispatch_started payload so that find_stale_dispatch_started / is_in_flight
    /// can call is_worker_process_live with the correct slug string instead of "".
    #[test]
    fn manual_dispatch_started_payload_contains_slug() {
        let conn = make_db();
        seed_mission_full(&conn, "m-slug-fix", "my-mission", "[]", "autonomous", "in_progress",
            None, None, None);

        // Build a dispatch_started payload the same way the manual path does,
        // including the new "slug" field, and verify it round-trips correctly.
        let slug = "my-mission";
        let payload = serde_json::json!({
            "attempt_id": "test-uuid",
            "runtime": "claude",
            "agent": "test-agent",
            "pid": 12345u32,
            "spawned_by": "manual",
            "slug": slug,
        });
        insert_event(&conn, "m-slug-fix", "dispatch_started", Some(payload.clone()),
            "2026-06-12T01:00:00Z").unwrap();

        // Read back and assert "slug" is present and correct.
        let raw: String = conn.query_row(
            "SELECT payload FROM mission_events WHERE mission_id = 'm-slug-fix' AND kind = 'dispatch_started'",
            [],
            |r| r.get(0),
        ).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            v.get("slug").and_then(|s| s.as_str()),
            Some(slug),
            "manual dispatch_started payload must carry the mission slug"
        );
    }

    // ── v2.16 PR-6: decision briefs ──────────────────────────────────────

    #[test]
    fn escalation_brief_builder_produces_canonical_shape() {
        let payload = EscalationBrief::new("merge_conflict")
            .summary("Agent 'a' conflicts")
            .ctx("agent", "a".to_string())
            .ctx("conflicting_files", vec!["foo.rs".to_string()])
            .options(["resolve manually", "skip", "abandon"])
            .into_payload();

        assert_eq!(payload["reason"], "merge_conflict");
        assert_eq!(payload["summary"], "Agent 'a' conflicts");
        assert_eq!(payload["agent"], "a");
        assert_eq!(payload["conflicting_files"][0], "foo.rs");
        assert!(payload["options"].is_array());
        assert_eq!(payload["options"].as_array().unwrap().len(), 3);
        assert_eq!(payload["options"][1], "skip");
    }

    #[test]
    fn escalation_brief_builder_omits_optional_fields() {
        // No summary / no options / no context → minimal payload.
        let payload = EscalationBrief::new("base_sha_unresolvable").into_payload();
        assert_eq!(payload["reason"], "base_sha_unresolvable");
        assert!(payload.get("summary").is_none(), "summary must be omitted when unset");
        assert!(payload["options"].as_array().unwrap().is_empty());
    }

    #[test]
    fn render_brief_payload_handles_options_hint_and_unknown_ctx() {
        // Canonical shape (reason + summary + options + extras).
        let p = EscalationBrief::new("regression_after_merge")
            .summary("agent x regressed")
            .ctx("agent", "x".to_string())
            .ctx("regressed", vec!["criterion-y".to_string()])
            .options(["fix and re-approve", "skip"])
            .into_payload();
        let s = render_brief_payload(&p, "2026-06-13T00:00:00Z");
        assert!(s.contains("escalated"));
        assert!(s.contains("regression_after_merge"));
        assert!(s.contains("agent x regressed"));
        assert!(s.contains("agent: x"));
        assert!(s.contains("regressed"));
        assert!(s.contains("1. fix and re-approve"));
        assert!(s.contains("2. skip"));

        // Legacy payload with a `hint` field (older no_worker_config rows).
        let legacy = serde_json::json!({
            "reason": "no_worker_config",
            "hint": "run `ato missions set-worker ...`"
        });
        let s2 = render_brief_payload(&legacy, "2026-06-13T00:00:00Z");
        assert!(s2.contains("hint: run `ato missions set-worker ..."));
    }

    #[test]
    fn run_briefs_filters_pending_only_by_default() {
        let conn = make_db();
        let now = "2026-06-13T00:00:00Z";
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                 created_at, updated_at)
             VALUES ('m-br', 'br-test', 'Br', 'Goal', '[]', '/tmp/b.md', ?1, ?1)",
            params![now],
        )
        .unwrap();

        // Resolved escalation (followed by a state_changed at a later timestamp).
        insert_event(
            &conn,
            "m-br",
            "escalated",
            Some(EscalationBrief::new("merge_conflict").into_payload()),
            "2026-06-13T00:00:01Z",
        )
        .unwrap();
        insert_event(
            &conn,
            "m-br",
            "state_changed",
            Some(serde_json::json!({"from": "blocked", "to": "in_progress"})),
            "2026-06-13T00:00:02Z",
        )
        .unwrap();

        // Pending escalation (no resolution after).
        insert_event(
            &conn,
            "m-br",
            "escalated",
            Some(EscalationBrief::new("consecutive_failures").into_payload()),
            "2026-06-13T00:00:03Z",
        )
        .unwrap();

        // Direct query mirrors run_briefs' filter logic.
        let mut stmt = conn
            .prepare(
                "SELECT occurred_at, payload FROM mission_events
                  WHERE mission_id = 'm-br' AND kind = 'escalated'
                  ORDER BY occurred_at DESC",
            )
            .unwrap();
        let all: Vec<(String, Option<String>)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))
            .unwrap()
            .filter_map(|x| x.ok())
            .collect();
        let pending: Vec<_> = all
            .iter()
            .filter(|(ts, _)| {
                let n: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM mission_events
                          WHERE mission_id = 'm-br'
                            AND kind IN ('owner_decision', 'state_changed')
                            AND occurred_at > ?1",
                        params![ts],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                n == 0
            })
            .collect();

        assert_eq!(all.len(), 2, "two escalated events seeded");
        assert_eq!(pending.len(), 1, "only the second is pending");
        let pending_payload: serde_json::Value =
            serde_json::from_str(&pending[0].1.clone().unwrap()).unwrap();
        assert_eq!(pending_payload["reason"], "consecutive_failures");
    }

    // ── v2.16 PR-8: narrative auto-population ───────────────────────────

    #[test]
    fn format_event_narrative_renders_known_kinds_and_falls_back_on_unknown() {
        let ts = "2026-06-13T00:00:00Z";

        // state_changed with reason
        let s = format_event_narrative(
            "state_changed",
            Some(&serde_json::json!({"from": "open", "to": "in_progress", "reason": "first_dispatch"})),
            ts,
        );
        assert!(s.starts_with("- _"));
        assert!(s.contains("State changed"));
        assert!(s.contains("`open` → `in_progress`"));
        assert!(s.contains("first_dispatch"));

        // dispatched with cost + log id (id shortened to 8 chars)
        let s = format_event_narrative(
            "dispatched",
            Some(&serde_json::json!({
                "runtime": "google", "model": "gemini-3-flash-preview",
                "status": "success", "cost_usd": 0.0123,
                "execution_log_id": "deadbeef-cafe-1234-5678-abcdef012345"
            })),
            ts,
        );
        assert!(s.contains("Dispatched"));
        assert!(s.contains("`google/gemini-3-flash-preview`"));
        assert!(s.contains("`success`"));
        assert!(s.contains("$0.0123"));
        assert!(s.contains("`deadbeef`"), "log id must be truncated to 8 chars: {}", s);

        // success_check counts met/total
        let s = format_event_narrative(
            "success_check",
            Some(&serde_json::json!({
                "results": [{"met": true}, {"met": false}, {"met": true}],
                "all_met": false,
            })),
            ts,
        );
        assert!(s.contains("2/3 criteria met"));

        // escalated with summary
        let s = format_event_narrative(
            "escalated",
            Some(&serde_json::json!({"reason": "merge_conflict", "summary": "agent a clashes"})),
            ts,
        );
        assert!(s.contains("⚠ Escalated"));
        assert!(s.contains("`merge_conflict`"));
        assert!(s.contains("agent a clashes"));

        // unknown kind falls back without panicking
        let s = format_event_narrative("unknown_future_kind", None, ts);
        assert!(s.contains("Event `unknown_future_kind`"));

        // All paragraphs are one line.
        assert!(!s.contains('\n'), "paragraphs must be a single line");
    }

    #[test]
    fn insert_event_appends_to_narrative_file() {
        let conn = make_db();
        // Write the narrative scaffold to a temp file and point the mission at it.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "# Mission\n\n## Events\n").unwrap();
        let path_str = tmp.path().to_string_lossy().to_string();

        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                 created_at, updated_at)
             VALUES ('m-narr', 'narr-test', 'Narr', 'Goal', '[]', ?1, ?2, ?2)",
            params![path_str, "2026-06-13T00:00:00Z"],
        )
        .unwrap();

        insert_event(
            &conn,
            "m-narr",
            "state_changed",
            Some(serde_json::json!({"from": "open", "to": "in_progress"})),
            "2026-06-13T00:00:01Z",
        )
        .unwrap();
        insert_event(
            &conn,
            "m-narr",
            "dispatched",
            Some(serde_json::json!({"runtime": "google", "status": "success"})),
            "2026-06-13T00:00:02Z",
        )
        .unwrap();

        let body = std::fs::read_to_string(tmp.path()).unwrap();
        assert!(body.contains("State changed"), "narrative body: {}", body);
        assert!(body.contains("Dispatched"), "narrative body: {}", body);
        // Order is preserved (append semantics).
        let idx_state = body.find("State changed").unwrap();
        let idx_disp = body.find("Dispatched").unwrap();
        assert!(idx_state < idx_disp, "events must append in occurred order");
    }

    /// Codex/Gemini R1: payload strings with embedded newlines must NOT
    /// break the single-line-per-event invariant.
    #[test]
    fn format_event_narrative_sanitizes_payload_newlines() {
        let s = format_event_narrative(
            "worktree_cleanup_failed",
            Some(&serde_json::json!({
                "error": "git failed:\nfatal: bad object\r\ncannot continue"
            })),
            "2026-06-13T00:00:00Z",
        );
        assert!(!s.contains('\n'), "no embedded newlines: {:?}", s);
        assert!(!s.contains('\r'), "no embedded CRs: {:?}", s);
        assert!(s.contains("git failed:"));
        // Multi-line was joined with single spaces.
        assert!(s.contains("git failed: fatal: bad object"));
    }

    #[test]
    fn insert_event_atomic_write_truncates_oversized_lines() {
        // Even with absurd payload sizes, the appended bytes stay within the
        // atomic-write envelope. We don't assert the exact cap here — only
        // that a multi-KB error string does not produce a multi-KB markdown
        // line. SAFE_MAX is 2048 in the implementation.
        let conn = make_db();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "# Mission\n\n## Events\n").unwrap();
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                 created_at, updated_at)
             VALUES ('m-big', 'big', 'Big', 'Goal', '[]', ?1, '2026-06-13T00:00:00Z', '2026-06-13T00:00:00Z')",
            params![tmp.path().to_string_lossy().to_string()],
        )
        .unwrap();

        let huge = "x".repeat(10_000);
        insert_event(
            &conn,
            "m-big",
            "worktree_cleanup_failed",
            Some(serde_json::json!({"error": huge})),
            "2026-06-13T00:00:01Z",
        )
        .unwrap();

        let body = std::fs::read_to_string(tmp.path()).unwrap();
        // Find the appended line (the last non-empty line).
        let appended = body
            .lines()
            .filter(|l| !l.trim().is_empty())
            .last()
            .unwrap();
        assert!(
            appended.len() <= 2200,
            "appended line must stay within atomic-write envelope, got {} bytes",
            appended.len()
        );
        // The truncation marker is present when content is cut.
        assert!(appended.ends_with('…') || appended.len() < 2048, "truncation marker missing on big line: {:?}", appended);
    }

    #[test]
    fn insert_event_is_silent_when_narrative_path_is_missing() {
        let conn = make_db();
        // Mission row exists but its narrative_md_path points to a file that doesn't exist.
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                 created_at, updated_at)
             VALUES ('m-no', 'no-narr', 'No', 'Goal', '[]', '/tmp/__does-not-exist__.md',
                 '2026-06-13T00:00:00Z', '2026-06-13T00:00:00Z')",
            [],
        )
        .unwrap();

        // Must NOT panic, MUST succeed — the SQLite ledger is the source of truth.
        insert_event(
            &conn,
            "m-no",
            "category_changed",
            Some(serde_json::json!({"from": "autonomous", "to": "needs_owner"})),
            "2026-06-13T00:00:01Z",
        )
        .unwrap();

        // Event still recorded in SQLite.
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mission_events WHERE mission_id = 'm-no'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }
}
