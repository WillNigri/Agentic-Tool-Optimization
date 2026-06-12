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
use std::path::PathBuf;
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
        #[arg(long)]
        runtime: String,
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
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MissionEventRow {
    id: String,
    mission_id: String,
    kind: String,
    payload: Option<serde_json::Value>,
    occurred_at: String,
}

const MISSION_SELECT: &str = "SELECT id, slug, name, goal, success_criteria, escalation_policy,
            workspace_strategy, base_sha, cleanup_policy, merge_strategy,
            category, state, max_loops, token_budget_usd, result_metadata,
            narrative_md_path, created_at, updated_at FROM missions";

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
        } => run_create(
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
            },
            db_path,
            opts,
        ),
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
        MissionSub::Dispatch {
            slug_or_id,
            runtime,
            prompt,
            prompt_file,
            model,
            agent,
            with_tools,
            require_tools,
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
            narrative_md_path, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'open', ?12, ?13, NULL, ?14, ?15, ?15)",
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
    runtime: String,
    prompt: Option<String>,
    prompt_file: Option<PathBuf>,
    model: Option<String>,
    agent: Option<String>,
    with_tools: bool,
    require_tools: Vec<String>,
}

fn run_dispatch_under_mission(
    input: DispatchInput,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    if input.runtime.trim().is_empty() {
        anyhow::bail!("missions dispatch: --runtime is required");
    }
    let prompt_text = read_prompt_arg(input.prompt, input.prompt_file)?;
    if prompt_text.trim().is_empty() {
        anyhow::bail!("missions dispatch: prompt is empty (use --prompt or --prompt-file)");
    }

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

    // Workspace-strategy gate. per_agent_worktree needs PR-3.
    if mission.workspace_strategy == "per_agent_worktree" {
        anyhow::bail!(
            "missions dispatch: mission '{}' has workspace_strategy='per_agent_worktree' but worktree \
             creation/cleanup is queued for PR-3. Either change the mission to single_cwd (`ato missions \
             set-state ...` after editing the row), or wait for PR-3.",
            mission.slug
        );
    }

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

    // Capture wake_started_at so we can find the freshly-written execution_logs
    // row that this dispatch creates. Same pattern v2.15.5 uses in paused-
    // dispatch resume (execution_logs.id is TEXT UUID, not auto-increment).
    let wake_started_at = chrono::Utc::now().to_rfc3339();
    drop(conn);

    if opts.human {
        emit_human(&format!(
            "→ firing {} under mission '{}' (prior dispatches: {}, prior cost: ${:.4})",
            input.runtime, mission.slug, prior_dispatch_count, prior_cost_usd
        ));
    }

    // Resolve with_tools: --require-tools implies --with-tools.
    let effective_with_tools = input.with_tools || !input.require_tools.is_empty();

    // Set ATO_REQUIRE_TOOLS env for the dispatch — dispatch.rs reads
    // --require-tools off Opts; we pass via the simpler env-var path so
    // we don't have to re-engineer the Opts struct for this caller.
    // Standard dispatch::run respects this env var (see api_dispatch and
    // the grounding policy compiler).
    if !input.require_tools.is_empty() {
        std::env::set_var("ATO_REQUIRE_TOOLS", input.require_tools.join(","));
    }

    // Fire the dispatch through the shared entrypoint. Errors here are
    // dispatch failures (network, no key, etc.) — they propagate up and
    // the mission stays in_progress so the operator can retry.
    crate::commands::dispatch::run(
        &input.runtime,
        &prompt_text,
        input.model.clone(),
        input.agent.clone(),
        None,  // session_id — Mission dispatches aren't anchored to a session today
        None,  // war_room_id
        None,  // war_room_round
        false, // stream
        false, // stream_jsonl
        effective_with_tools,
        db_path,
        opts,
    )?;

    if !input.require_tools.is_empty() {
        std::env::remove_var("ATO_REQUIRE_TOOLS");
    }

    // Read back the freshly-written execution_logs row.
    let conn = db::open_readwrite(db_path)?;
    let outcome: Option<(String, String, Option<String>, Option<f64>, Option<i64>)> = conn
        .query_row(
            "SELECT id, status, error_message, cost_usd_estimated, tool_calls_count
               FROM execution_logs
              WHERE runtime = ?1 AND created_at >= ?2
              ORDER BY created_at DESC
              LIMIT 1",
            params![input.runtime, wake_started_at],
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
            "runtime": input.runtime,
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
            "runtime": input.runtime,
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
                    input.runtime,
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
/// Mission's events. Uses json_extract on payload.execution_log_id so we
/// don't need a join column — keeps schema slim (codex's B-lite shape).
fn sum_cost_for_mission(conn: &Connection, mission_id: &str) -> Result<f64> {
    let total: Option<f64> = conn
        .query_row(
            "SELECT COALESCE(SUM(el.cost_usd_estimated), 0.0)
               FROM mission_events me
               JOIN execution_logs el
                 ON json_extract(me.payload, '$.execution_log_id') = el.id
              WHERE me.mission_id = ?1
                AND me.kind = 'dispatched'",
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

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
                updated_at          TEXT NOT NULL
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

        let n = count_dispatches_for_mission(&conn, "m-cnt").unwrap();
        assert_eq!(n, 3, "should count dispatched + loop_run_completed only");
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
}
