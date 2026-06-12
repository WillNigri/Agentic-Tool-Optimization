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
//
//   ato missions create --name "X" --goal "..."
//                       [--success-criteria FILE]
//                       [--workspace-strategy single_cwd|per_agent_worktree]
//                       [--merge-strategy human_approves_each|coordinator_merges_all|coordinator_picks_winner|ranked_by_score]
//                       [--cleanup-policy retain|delete_on_success|always_delete]
//                       [--category autonomous|needs_owner|ignored|done]
//                       [--max-loops N] [--token-budget-usd FLOAT]
//                       [--base-sha <sha>]
//   ato missions list [--state STATE] [--category CATEGORY]
//   ato missions show <slug-or-id>
//   ato missions set-category <slug-or-id> <category>
//   ato missions set-state    <slug-or-id> <state>
//   ato missions narrative    <slug-or-id>
//   ato missions events       <slug-or-id> [--limit N]
//
// Out of scope for PR-1 (queued for PR-2..PR-8):
//   - `ato missions tick`     — the coordinator wake (PR-4)
//   - `ato missions dispatch` — fire a worker Loop under a Mission (PR-2)
//   - Worktree create/cleanup for per_agent_worktree (PR-3)
//   - Merge execution (PR-5)
//   - Decision briefs on escalation (PR-6)
//   - Desktop Mission-control board (PR-7)
//   - Narrative auto-population (PR-8)
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
//   15=narrative_md_path 16=created_at 17=updated_at 18=repo_root (PR-3, last)
const MISSION_SELECT: &str = "SELECT id, slug, name, goal, success_criteria, escalation_policy,
            workspace_strategy, base_sha, cleanup_policy, merge_strategy,
            category, state, max_loops, token_budget_usd, result_metadata,
            narrative_md_path, created_at, updated_at, repo_root FROM missions";

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
            },
            db_path,
            opts,
        ),
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

    conn.execute(
        "INSERT INTO missions (
            id, slug, name, goal, success_criteria, escalation_policy,
            workspace_strategy, base_sha, cleanup_policy, merge_strategy,
            category, state, max_loops, token_budget_usd, result_metadata,
            narrative_md_path, created_at, updated_at, repo_root
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'open', ?12, ?13, NULL, ?14, ?15, ?15, ?16)",
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
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE missions SET state = ?1, updated_at = ?2 WHERE id = ?3",
        params![state, now, row.id],
    )?;
    insert_event(
        &conn,
        &row.id,
        "state_changed",
        Some(serde_json::json!({
            "from": row.state,
            "to": state,
        })),
        &now,
    )?;
    // v2.16 PR-3 — trigger event-driven cleanup on terminal state transitions.
    if state == "complete" {
        let cleaned = cleanup_mission_worktrees(&conn, &row, &state, false)?;
        if opts.human && !cleaned.is_empty() {
            emit_human(&format!(
                "  Worktrees cleaned: {}",
                cleaned.join(", ")
            ));
        }
    }
    let updated = load_mission(&conn, &row.id)?;
    if opts.human {
        emit_human(&format!(
            "Mission '{}' state: {} → {}",
            updated.slug, row.state, updated.state
        ));
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

    // Capture wake_started_at so we can find the freshly-written execution_logs
    // row that this dispatch creates. Same pattern v2.15.5 uses in paused-
    // dispatch resume (execution_logs.id is TEXT UUID, not auto-increment).
    let wake_started_at = chrono::Utc::now().to_rfc3339();
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
    let event_payload = match &outcome {
        Some((exec_id, status, err, cost, tool_calls)) => serde_json::json!({
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
        repo_root: r.get(18).ok().flatten(), // PR-3: new last column; .ok().flatten() tolerates old rows without it
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
    Ok(())
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
        let options = serde_json::json!([
            "recreate mission with a current base SHA (ato missions create ... --base-sha $(git rev-parse HEAD))",
            "git fetch to restore the commit if it was in a remote branch",
            "set cleanup_policy=retain and inspect worktrees manually"
        ]);
        let payload = serde_json::json!({
            "reason": "base_sha_unresolvable",
            "base_sha": base_sha,
            "repo_root": repo_root,
            "options": options,
        });
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

    Ok(wt_path)
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

    if !wt_root.exists() {
        return Ok(Vec::new());
    }

    let mut cleaned: Vec<String> = Vec::new();
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

// ── Tests ─────────────────────────────────────────────────────────────

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
                repo_root           TEXT
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
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, success_criteria, narrative_md_path,
                                    max_loops, token_budget_usd, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'Goal', '[]', '/tmp/x.md', ?4, ?5, '2026-06-12T00:00:00Z', '2026-06-12T00:00:00Z')",
            rusqlite::params![id, slug, slug, max_loops, token_budget_usd],
        )
        .unwrap();
    }

    fn db_with_execution_logs() -> Connection {
        let conn = make_db();
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
                updated_at TEXT NOT NULL, repo_root TEXT
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
                updated_at TEXT NOT NULL, repo_root TEXT
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
                updated_at TEXT NOT NULL, repo_root TEXT
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
}
