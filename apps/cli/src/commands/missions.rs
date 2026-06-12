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
}
