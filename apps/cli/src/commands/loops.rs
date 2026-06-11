// v2.14 — `ato loop` CLI surface (Loop Composer reframe of Automations).
//
// CRUD parity with the Tauri-side commands in
// `apps/desktop/src-tauri/src/commands/loops.rs` (Task #9). Both halves
// write to the same `loops` / `loop_runs` / `loop_run_steps` /
// `loop_schedules` tables in ~/.ato/local.db so a loop authored from
// the desktop UI is visible to `ato loop list` and vice versa.
//
// Subcommands shipped here (CRUD only):
//
//   ato loop create --name "X" [--description "..."]
//                   [--file graph.json] [--trigger-kind manual|schedule|webhook]
//   ato loop list
//   ato loop show <slug-or-id>
//   ato loop edit <slug-or-id> [--name "..."] [--description "..."]
//                              [--file graph.json] [--enabled true|false]
//   ato loop delete <slug-or-id>
//   ato loop runs list <slug-or-id> [--limit N]
//   ato loop runs show <run-id>
//
// Out of scope here (separate tasks):
//   - `ato loop run` — execution engine that walks the graph + spawns
//     dispatch / methodology / diagnose / review (Task #14).
//   - `ato loop schedule create|list|delete` — cron + launchd
//     integration (Task #16).
//
// Output defaults to JSON (machine + MCP friendly). `--human` swaps to
// terminal formatting — same convention as the rest of the CLI surface.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

use crate::db;
use crate::output::{emit_human, emit_json, Opts};

#[derive(Args, Debug)]
pub struct LoopArgs {
    #[command(subcommand)]
    pub sub: LoopSub,
}

#[derive(Subcommand, Debug)]
pub enum LoopSub {
    /// Create a new loop. Graph (nodes + edges) is read from --file as JSON.
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        /// Optional override for the auto-derived slug.
        #[arg(long)]
        slug: Option<String>,
        /// JSON file containing the loop graph: { nodes: [...], edges: [...] }.
        /// Defaults to an empty graph if omitted (useful for shells that want
        /// to PATCH later).
        #[arg(long)]
        file: Option<PathBuf>,
        /// "manual" | "schedule" | "webhook". Default: "manual".
        #[arg(long, default_value = "manual")]
        trigger_kind: String,
    },
    /// List all loops, newest first.
    List,
    /// Print one loop in full (slug or id).
    Show {
        slug_or_id: String,
    },
    /// Patch fields on an existing loop.
    Edit {
        slug_or_id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        enabled: Option<bool>,
    },
    /// Delete a loop (cascades to runs / steps / schedules).
    Delete {
        slug_or_id: String,
    },
    /// Execute a loop end-to-end. v2.14.0 MVP: dispatch + methodology_run
    /// kinds are wired to the real CLI primitives; other LLM-aware kinds
    /// (diagnose / apply / review / war_room / score / input) write a
    /// step row with status="skipped" and a "not implemented yet" note.
    /// Parallel + retry + variable substitution defer to v2.14.1.
    Run {
        slug_or_id: String,
        /// Free-form K=V variables that the executor exposes to steps
        /// (no template substitution wired yet in v2.14.0 — recorded
        /// on the loop_runs row for the audit trail only).
        #[arg(long = "var", value_name = "K=V")]
        vars: Vec<String>,
    },
    /// Inspect prior runs of a loop.
    Runs {
        #[command(subcommand)]
        sub: RunsSub,
    },
    /// Resume a paused dispatch (v2.15.4). Claims the row transactionally
    /// (paused → resuming), re-verifies that reset_at has passed and the
    /// runtime is no longer flagged exhausted in `subscription_resets`,
    /// then re-fires the dispatch. If the runtime is still exhausted at
    /// wake time, the row is re-paused with a fresh reset_at and the
    /// pause_count is incremented; if pause_count exceeds max_pause_count
    /// the row is abandoned and the owning loop_run is failed with a
    /// decision brief (runtime, history, recommended next action).
    Resume {
        paused_dispatch_id: String,
    },
    /// Scan for paused dispatches whose reset_at has passed and resume
    /// each one. Designed to be called from a launchd / cron tick or
    /// from `ato` startup. Walks oldest-first.
    ResumeDue,
    /// Recurring schedules for a loop. v2.14.0 ships the SQLite-backed
    /// surface (create / list / delete) so the contract is honest; the
    /// `launchd` plist registration that fires the loop at the cron
    /// expression's cadence is a v2.14.1 follow-up. Until then, wire a
    /// crontab line by hand:
    ///   <cron-expr>  /opt/homebrew/bin/ato loop run <slug>
    /// The scheduler tick will read from `loop_schedules` once the
    /// launchd integration lands.
    Schedule {
        #[command(subcommand)]
        sub: ScheduleSub,
    },
}

#[derive(Subcommand, Debug)]
pub enum ScheduleSub {
    /// Attach a recurring cron expression to a loop.
    Create {
        slug_or_id: String,
        #[arg(long)]
        cron: String,
    },
    /// List all schedules for one loop.
    List {
        slug_or_id: String,
    },
    /// Delete one schedule by its id.
    Delete {
        schedule_id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum RunsSub {
    /// List recent runs of one loop.
    List {
        slug_or_id: String,
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
    /// Show one run with its step-by-step execution log.
    Show {
        run_id: String,
    },
}

// ── Data shapes (CLI mirrors of the Tauri-side types) ────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LoopRow {
    id: String,
    slug: String,
    name: String,
    description: Option<String>,
    enabled: bool,
    graph: serde_json::Value,
    variables: Option<serde_json::Value>,
    trigger_kind: String,
    trigger_config: Option<serde_json::Value>,
    source: String,
    source_ref: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LoopRunRow {
    id: String,
    loop_id: String,
    status: String,
    started_at: String,
    finished_at: Option<String>,
    error: Option<String>,
    triggered_by: Option<String>,
    variables: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LoopRunStepRow {
    id: String,
    loop_run_id: String,
    node_id: String,
    node_type: String,
    status: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    input: Option<serde_json::Value>,
    output: Option<serde_json::Value>,
    error: Option<String>,
    execution_log_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct LoopRunWithSteps {
    #[serde(flatten)]
    run: LoopRunRow,
    steps: Vec<LoopRunStepRow>,
}

// ── SQL helpers (pure-ish — take &Connection so they're testable) ────────

const LOOP_SELECT: &str = "SELECT id, slug, name, description, enabled, graph, variables, trigger_kind, trigger_config, source, source_ref, created_at, updated_at FROM loops";

/// Pick `id` or `slug` based on UUID-shape detection so callers can pass
/// either to `show / edit / delete / runs list`. The naive
/// `WHERE id = ?1 OR slug = ?1` would be nondeterministic if a malicious
/// or accidental slug shaped like a UUID shadowed another loop's id.
/// War-room 72D76B07 (codex seat) caught this on the v2.14 foundation pass.
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
        out.push_str("loop");
    }
    out.chars().take(64).collect()
}

fn unique_slug(conn: &rusqlite::Connection, base: &str) -> Result<String> {
    let mut candidate = base.to_string();
    let mut suffix = 2;
    loop {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM loops WHERE slug = ?1",
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

fn parse_json_field(field: &str, raw: Option<String>) -> Result<Option<serde_json::Value>> {
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => serde_json::from_str::<serde_json::Value>(&s)
            .map(Some)
            .with_context(|| format!("invalid {} JSON", field)),
    }
}

fn row_to_loop(row: &rusqlite::Row<'_>) -> rusqlite::Result<(
    String,
    String,
    String,
    Option<String>,
    i32,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    String,
)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
    ))
}

fn assemble_loop(raw: (
    String,
    String,
    String,
    Option<String>,
    i32,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    String,
)) -> Result<LoopRow> {
    let (id, slug, name, description, enabled, graph_raw, variables_raw, trigger_kind, trigger_config_raw, source, source_ref, created_at, updated_at) = raw;
    let graph: serde_json::Value = serde_json::from_str(&graph_raw)
        .with_context(|| format!("invalid graph JSON on loop {}", id))?;
    let variables = parse_json_field("variables", variables_raw)?;
    let trigger_config = parse_json_field("trigger_config", trigger_config_raw)?;
    Ok(LoopRow {
        id,
        slug,
        name,
        description,
        enabled: enabled != 0,
        graph,
        variables,
        trigger_kind,
        trigger_config,
        source,
        source_ref,
        created_at,
        updated_at,
    })
}

fn load_loop(conn: &rusqlite::Connection, slug_or_id: &str) -> Result<LoopRow> {
    let sql = format!("{} WHERE {} = ?1", LOOP_SELECT, id_or_slug_column(slug_or_id));
    let raw = conn
        .query_row(&sql, params![slug_or_id], row_to_loop)
        .with_context(|| format!("loop not found: {}", slug_or_id))?;
    assemble_loop(raw)
}

// ── Run dispatch ─────────────────────────────────────────────────────────

pub fn run(args: LoopArgs, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    match args.sub {
        LoopSub::Create { name, description, slug, file, trigger_kind } => {
            run_create(name, description, slug, file, trigger_kind, db_path, opts)
        }
        LoopSub::List => run_list(db_path, opts),
        LoopSub::Show { slug_or_id } => run_show(slug_or_id, db_path, opts),
        LoopSub::Edit { slug_or_id, name, description, file, enabled } => {
            run_edit(slug_or_id, name, description, file, enabled, db_path, opts)
        }
        LoopSub::Delete { slug_or_id } => run_delete(slug_or_id, db_path, opts),
        LoopSub::Run { slug_or_id, vars } => run_execute(slug_or_id, vars, db_path, opts),
        LoopSub::Resume { paused_dispatch_id } => run_resume(&paused_dispatch_id, db_path, opts),
        LoopSub::ResumeDue => run_resume_due(db_path, opts),
        LoopSub::Schedule { sub } => match sub {
            ScheduleSub::Create { slug_or_id, cron } => run_schedule_create(slug_or_id, cron, db_path, opts),
            ScheduleSub::List { slug_or_id } => run_schedule_list(slug_or_id, db_path, opts),
            ScheduleSub::Delete { schedule_id } => run_schedule_delete(schedule_id, db_path, opts),
        },
        LoopSub::Runs { sub } => match sub {
            RunsSub::List { slug_or_id, limit } => run_runs_list(slug_or_id, limit, db_path, opts),
            RunsSub::Show { run_id } => run_runs_show(run_id, db_path, opts),
        },
    }
}

fn run_create(
    name: String,
    description: Option<String>,
    slug_override: Option<String>,
    file: Option<PathBuf>,
    trigger_kind: String,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    if name.trim().is_empty() {
        anyhow::bail!("name is empty");
    }
    if !matches!(trigger_kind.as_str(), "manual" | "schedule" | "webhook") {
        anyhow::bail!("invalid --trigger-kind: {} (expected manual|schedule|webhook)", trigger_kind);
    }
    let graph: serde_json::Value = match file {
        Some(p) => {
            let raw = fs::read_to_string(&p)
                .with_context(|| format!("read graph file {}", p.display()))?;
            serde_json::from_str(&raw).with_context(|| format!("parse graph JSON in {}", p.display()))?
        }
        None => serde_json::json!({"nodes": [], "edges": []}),
    };

    let conn = db::open_readwrite(db_path)?;
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let name_trimmed: String = name.trim().chars().take(200).collect();
    let base_slug = slug_override
        .as_deref()
        .map(slugify)
        .unwrap_or_else(|| slugify(&name_trimmed));
    let slug = unique_slug(&conn, &base_slug)?;
    let graph_str = serde_json::to_string(&graph).context("serialize graph")?;

    conn.execute(
        "INSERT INTO loops (
            id, slug, name, description, enabled, graph, variables,
            trigger_kind, trigger_config, source, source_ref,
            created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, 1, ?5, NULL, ?6, NULL, 'manual', NULL, ?7, ?7)",
        params![id, slug, name_trimmed, description, graph_str, trigger_kind, now],
    )
    .context("insert loop")?;

    let row = load_loop(&conn, &id)?;
    if opts.human {
        emit_human(&format!("Created loop '{}' (slug: {})", row.name, row.slug));
    } else {
        emit_json(&row)?;
    }
    Ok(())
}

fn run_list(db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let sql = format!("{} ORDER BY updated_at DESC", LOOP_SELECT);
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = Vec::new();
    let iter = stmt.query_map([], row_to_loop)?;
    for r in iter {
        rows.push(assemble_loop(r?)?);
    }
    if opts.human {
        if rows.is_empty() {
            emit_human("No loops yet — `ato loop create --name \"...\"` to make one.");
        } else {
            emit_human(&format!("Loops ({}):", rows.len()));
            for r in &rows {
                let status = if r.enabled { "enabled" } else { "disabled" };
                emit_human(&format!(
                    "  • {} ({}) — trigger={} status={} updated={}",
                    r.name, r.slug, r.trigger_kind, status, r.updated_at
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
    let row = load_loop(&conn, &slug_or_id)?;
    if opts.human {
        emit_human(&format!("Loop '{}' (slug: {})", row.name, row.slug));
        if let Some(d) = &row.description {
            emit_human(&format!("  description: {}", d));
        }
        emit_human(&format!("  trigger_kind: {}", row.trigger_kind));
        emit_human(&format!("  enabled: {}", row.enabled));
        emit_human(&format!("  source: {}", row.source));
        emit_human(&format!("  created_at: {}", row.created_at));
        emit_human(&format!("  updated_at: {}", row.updated_at));
        let node_count = row
            .graph
            .get("nodes")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let edge_count = row
            .graph
            .get("edges")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        emit_human(&format!("  graph: {} nodes, {} edges", node_count, edge_count));
    } else {
        emit_json(&row)?;
    }
    Ok(())
}

fn run_edit(
    slug_or_id: String,
    name: Option<String>,
    description: Option<String>,
    file: Option<PathBuf>,
    enabled: Option<bool>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let now = chrono::Utc::now().to_rfc3339();

    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(name) = name {
        let trimmed: String = name.trim().chars().take(200).collect();
        if trimmed.is_empty() {
            anyhow::bail!("--name is empty");
        }
        sets.push(format!("name = ?{}", sets.len() + 1));
        binds.push(Box::new(trimmed));
    }
    if let Some(description) = description {
        sets.push(format!("description = ?{}", sets.len() + 1));
        binds.push(Box::new(description));
    }
    if let Some(p) = file {
        let raw = fs::read_to_string(&p)
            .with_context(|| format!("read graph file {}", p.display()))?;
        let graph: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("parse graph JSON in {}", p.display()))?;
        let graph_str = serde_json::to_string(&graph)?;
        sets.push(format!("graph = ?{}", sets.len() + 1));
        binds.push(Box::new(graph_str));
    }
    if let Some(enabled) = enabled {
        sets.push(format!("enabled = ?{}", sets.len() + 1));
        binds.push(Box::new(if enabled { 1i32 } else { 0i32 }));
    }

    if sets.is_empty() {
        anyhow::bail!("no fields to update — supply --name / --description / --file / --enabled");
    }

    sets.push(format!("updated_at = ?{}", sets.len() + 1));
    binds.push(Box::new(now));
    let id_pos = sets.len() + 1;
    binds.push(Box::new(slug_or_id.clone()));

    let sql = format!(
        "UPDATE loops SET {} WHERE {} = ?{}",
        sets.join(", "),
        id_or_slug_column(&slug_or_id),
        id_pos
    );
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| &**b).collect();
    let affected = conn.execute(&sql, rusqlite::params_from_iter(bind_refs.iter()))?;
    if affected == 0 {
        anyhow::bail!("loop not found: {}", slug_or_id);
    }

    let row = load_loop(&conn, &slug_or_id)?;
    if opts.human {
        emit_human(&format!("Updated loop '{}' (slug: {})", row.name, row.slug));
    } else {
        emit_json(&row)?;
    }
    Ok(())
}

fn run_delete(slug_or_id: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let sql = format!("DELETE FROM loops WHERE {} = ?1", id_or_slug_column(&slug_or_id));
    let affected = conn
        .execute(&sql, params![slug_or_id])
        .context("delete loop")?;
    if affected == 0 {
        anyhow::bail!("loop not found: {}", slug_or_id);
    }
    if opts.human {
        emit_human(&format!("Deleted loop {}", slug_or_id));
    } else {
        emit_json(&serde_json::json!({ "deleted": slug_or_id }))?;
    }
    Ok(())
}

// ── Executor (v2.14.0 MVP) ──────────────────────────────────────────────
//
// First-cut Loop executor: walks `graph.nodes` in declaration order and
// dispatches each step to its per-kind handler. `dispatch` + `methodology_run`
// are wired to the real ATO primitives; the other LLM-aware kinds write a
// `status='skipped'` step row with a 'not yet implemented' note so the
// audit trail makes the gap obvious.
//
// Deferred to v2.14.1 (separate tasks):
//   - topological order via `graph.edges` (today: declaration order)
//   - variable substitution {{vars.x}} / {{steps.N.output.field}}
//   - parallel + retry + decision branching control flow
//   - cancellation via Ctrl-C while a step is in flight
//   - per-step --watch streaming

#[derive(Debug, Deserialize, Clone)]
struct LoopGraphNode {
    id: String,
    #[serde(rename = "type")]
    node_type: String,
    #[serde(default)]
    config: Option<serde_json::Value>,
}

/// An edge between two LoopSteps. The legacy FlowEdge shape uses
/// `from` / `to`; the v2.14 LoopEdge shape uses `source` / `target`.
/// We accept either so workflows migrated from v2.13 don't need a
/// separate edge-mapping pass.
#[derive(Debug, Deserialize, Clone)]
struct LoopGraphEdge {
    #[serde(default, alias = "from")]
    source: Option<String>,
    #[serde(default, alias = "to")]
    target: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoopGraph {
    #[serde(default)]
    nodes: Vec<LoopGraphNode>,
    #[serde(default)]
    edges: Vec<LoopGraphEdge>,
}

/// Kahn's algorithm — return nodes in topological order. Falls back to
/// declaration order if the graph has cycles or unknown node refs (with
/// a stderr warning) so a malformed loop still gets to run instead of
/// hard-failing. War-room (codex + gemini, war_room_id 0EDD3A31) caught
/// the original "declaration-order execution ignores edges" foundation
/// bug — this is the fix.
fn topological_order(graph: &LoopGraph) -> Vec<LoopGraphNode> {
    use std::collections::{HashMap, HashSet, VecDeque};

    if graph.edges.is_empty() {
        // Nothing to sort by — declaration order is the user's intent.
        return graph.nodes.clone();
    }

    let node_index: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();

    let mut in_degree = vec![0usize; graph.nodes.len()];
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); graph.nodes.len()];
    let mut unknown_refs: HashSet<&str> = HashSet::new();

    for edge in &graph.edges {
        let (src, tgt) = match (edge.source.as_deref(), edge.target.as_deref()) {
            (Some(s), Some(t)) => (s, t),
            _ => continue,
        };
        let (si, ti) = match (node_index.get(src), node_index.get(tgt)) {
            (Some(&s), Some(&t)) => (s, t),
            _ => {
                if node_index.get(src).is_none() {
                    unknown_refs.insert(src);
                }
                if node_index.get(tgt).is_none() {
                    unknown_refs.insert(tgt);
                }
                continue;
            }
        };
        adjacency[si].push(ti);
        in_degree[ti] += 1;
    }

    if !unknown_refs.is_empty() {
        eprintln!(
            "[loop-executor] warning: graph references unknown node id(s) {:?} — edges to them ignored",
            unknown_refs
        );
    }

    let mut queue: VecDeque<usize> = in_degree
        .iter()
        .enumerate()
        .filter_map(|(i, &d)| if d == 0 { Some(i) } else { None })
        .collect();
    let mut ordered: Vec<LoopGraphNode> = Vec::with_capacity(graph.nodes.len());

    while let Some(i) = queue.pop_front() {
        ordered.push(graph.nodes[i].clone());
        for &neighbor in &adjacency[i] {
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                queue.push_back(neighbor);
            }
        }
    }

    if ordered.len() != graph.nodes.len() {
        eprintln!(
            "[loop-executor] warning: graph contains a cycle ({} of {} nodes ordered); falling back to declaration order",
            ordered.len(),
            graph.nodes.len()
        );
        return graph.nodes.clone();
    }

    ordered
}

fn graph_params(node: &LoopGraphNode) -> serde_json::Map<String, serde_json::Value> {
    node.config
        .as_ref()
        .and_then(|c| c.get("params"))
        .and_then(|p| p.as_object())
        .cloned()
        .unwrap_or_default()
}

fn param_str<'a>(
    params: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    params.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
}

fn parse_vars(raw: &[String]) -> Result<serde_json::Value> {
    let mut map = serde_json::Map::new();
    for kv in raw {
        let (k, v) = kv
            .split_once('=')
            .with_context(|| format!("--var {} must be K=V", kv))?;
        map.insert(k.to_string(), serde_json::Value::String(v.to_string()));
    }
    Ok(serde_json::Value::Object(map))
}

fn run_execute(
    slug_or_id: String,
    raw_vars: Vec<String>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let variables = parse_vars(&raw_vars)?;

    // Load + parse the loop's graph.
    let loop_row = {
        let conn = db::open_readonly(db_path)?;
        load_loop(&conn, &slug_or_id)?
    };
    let graph: LoopGraph = serde_json::from_value(loop_row.graph.clone())
        .context("loop graph JSON did not match { nodes: [...] }")?;

    // Insert loop_runs row.
    let conn = db::open_readwrite(db_path)?;
    let run_id = Uuid::new_v4().to_string();
    let started_at = chrono::Utc::now().to_rfc3339();
    // `triggered_by` is a free-text audit hint, not auth — best-effort
    // username discovery via $USER / $USERNAME; falls back to "unknown".
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".into());
    let triggered_by = format!("manual:{}", username);
    let vars_str = serde_json::to_string(&variables).unwrap_or_else(|_| "{}".into());
    conn.execute(
        "INSERT INTO loop_runs (id, loop_id, status, started_at, triggered_by, variables)
         VALUES (?1, ?2, 'running', ?3, ?4, ?5)",
        params![run_id, loop_row.id, started_at, triggered_by, vars_str],
    )
    .context("insert loop_run")?;

    let mut last_error: Option<String> = None;
    let mut steps_succeeded: usize = 0;
    let mut steps_executed: usize = 0;
    // v2.15.4 — populated when a step returns StepError::Paused; signals
    // the post-loop status writer to set loop_runs.status='paused' (not
    // success/error) and to emit a resume-hint summary.
    let mut paused_signal: Option<(String, String, String)> = None;

    // Walk nodes in topological order so edge dependencies are honored.
    // Falls back to declaration order if the graph is edgeless or cyclic
    // (with a stderr warning) — see `topological_order` above.
    let order = topological_order(&graph);
    for node in &order {
        steps_executed += 1;
        let step_id = Uuid::new_v4().to_string();
        let step_started = chrono::Utc::now().to_rfc3339();
        let input_json = serde_json::to_string(&node.config.clone().unwrap_or_default()).ok();

        conn.execute(
            "INSERT INTO loop_run_steps (
                id, loop_run_id, node_id, node_type, status,
                started_at, input
             ) VALUES (?1, ?2, ?3, ?4, 'running', ?5, ?6)",
            params![step_id, run_id, node.id, node.node_type, step_started, input_json],
        )
        .context("insert loop_run_step")?;

        if opts.human {
            emit_human(&format!("→ step {} ({}) running …", node.id, node.node_type));
        }

        let result = execute_step(node, &run_id, &step_id, db_path, opts);
        let step_finished = chrono::Utc::now().to_rfc3339();

        match result {
            Ok(output_value) => {
                steps_succeeded += 1;
                let output_str = serde_json::to_string(&output_value).ok();
                // Capture the execution_log_id when the step's output
                // carries one (e.g. dispatch via execution_logs). This
                // is the foreign key future variable substitution will
                // use: {{steps.<node_id>.output}} resolves through here.
                let exec_log_id = output_value
                    .get("execution_log_id")
                    .and_then(|v| v.as_i64());
                conn.execute(
                    "UPDATE loop_run_steps
                        SET status = ?1, finished_at = ?2, output = ?3,
                            execution_log_id = ?4
                      WHERE id = ?5",
                    params![
                        "success",
                        step_finished,
                        output_str,
                        exec_log_id,
                        step_id,
                    ],
                )?;
                if opts.human {
                    emit_human(&format!(
                        "  ✓ step {} ({}) success",
                        node.id, node.node_type
                    ));
                }
            }
            Err(StepError::Skipped(msg)) => {
                conn.execute(
                    "UPDATE loop_run_steps
                        SET status = ?1, finished_at = ?2, error = ?3
                      WHERE id = ?4",
                    params!["skipped", step_finished, msg, step_id],
                )?;
                if opts.human {
                    emit_human(&format!(
                        "  ⊘ step {} ({}) skipped — {}",
                        node.id, node.node_type, msg
                    ));
                }
            }
            Err(StepError::Failed(msg)) => {
                conn.execute(
                    "UPDATE loop_run_steps
                        SET status = ?1, finished_at = ?2, error = ?3
                      WHERE id = ?4",
                    params!["error", step_finished, msg.clone(), step_id],
                )?;
                if opts.human {
                    emit_human(&format!(
                        "  ✗ step {} ({}) error — {}",
                        node.id, node.node_type, msg
                    ));
                }
                last_error = Some(format!("step {} ({}): {}", node.id, node.node_type, msg));
                // Fail-fast for v2.14.0 MVP. Per-edge error policy (continue
                // on error, retry, branch to catch) lands in v2.14.1.
                break;
            }
            Err(StepError::Paused {
                paused_dispatch_id,
                runtime: paused_runtime,
                reset_at,
            }) => {
                // v2.15.4 — subscription exhausted, policy=pause-and-wake.
                // Mark the step + run as paused (not error) and mirror the
                // paused_dispatch id onto loop_runs so the resumer can find
                // this run via either side. Exit the loop cleanly — the
                // resumer (CLI or startup scanner) re-fires this step at
                // reset_at via `ato loop resume <paused-dispatch-id>`.
                conn.execute(
                    "UPDATE loop_run_steps
                        SET status = ?1, finished_at = ?2, error = ?3
                      WHERE id = ?4",
                    params![
                        "paused",
                        step_finished,
                        format!(
                            "paused on {}; reset_at={}; resume via `ato loop resume {}`",
                            paused_runtime, reset_at, paused_dispatch_id
                        ),
                        step_id
                    ],
                )?;
                conn.execute(
                    "UPDATE loop_runs
                        SET paused_until = ?1, paused_dispatch_id = ?2
                      WHERE id = ?3",
                    params![reset_at, paused_dispatch_id, run_id],
                )?;
                if opts.human {
                    emit_human(&format!(
                        "  ⏸ step {} ({}) paused — {} exhausted until {} (paused_dispatch={})",
                        node.id, node.node_type, paused_runtime, reset_at, paused_dispatch_id
                    ));
                }
                paused_signal = Some((paused_dispatch_id, paused_runtime, reset_at));
                break;
            }
        }
    }

    let finished_at = chrono::Utc::now().to_rfc3339();
    // Smart status semantics — war-room (codex+gemini, war_room_id
    // 0EDD3A31) caught the prior "success on all-skipped" bug.
    // - error if any step hard-failed (fail-fast)
    // - success if at least one step actually executed and succeeded
    // - skipped if NO step succeeded but no step failed (e.g. all
    //   LLM-aware kinds still stubbed, or the loop is empty)
    let (status, error_col): (&str, Option<String>) = match (&paused_signal, &last_error, steps_succeeded) {
        (Some((pid, runtime, reset_at)), _, _) => (
            "paused",
            Some(format!(
                "paused on {} until {}; resume via `ato loop resume {}`",
                runtime, reset_at, pid
            )),
        ),
        (None, Some(msg), _) => ("error", Some(msg.clone())),
        (None, None, 0) => ("skipped", None),
        (None, None, _) => ("success", None),
    };
    conn.execute(
        "UPDATE loop_runs
            SET status = ?1, finished_at = ?2, error = ?3
          WHERE id = ?4",
        params![status, finished_at, error_col, run_id],
    )?;

    let summary = serde_json::json!({
        "run_id": run_id,
        "loop_id": loop_row.id,
        "loop_slug": loop_row.slug,
        "status": status,
        "started_at": started_at,
        "finished_at": finished_at,
        "error": error_col,
        "steps_executed": steps_executed,
        "steps_succeeded": steps_succeeded,
        "steps_planned": graph.nodes.len(),
        "paused_dispatch_id": paused_signal.as_ref().map(|(id, _, _)| id.clone()),
        "paused_runtime": paused_signal.as_ref().map(|(_, r, _)| r.clone()),
        "paused_until": paused_signal.as_ref().map(|(_, _, ts)| ts.clone()),
    });

    if opts.human {
        emit_human(&format!(
            "Loop '{}' ({}) finished — status={} run_id={}",
            loop_row.slug, loop_row.name, status, run_id
        ));
    } else {
        emit_json(&summary)?;
    }
    Ok(())
}

/// One-step result. `Skipped` means the kind isn't implemented yet but
/// the run continues; `Failed` is a real error and the run breaks here.
/// `Paused` (v2.15.4) means the step hit subscription exhaustion and was
/// persisted to `paused_dispatches` for later resume — the loop run
/// updates its `paused_until` mirror and returns; the resumer (CLI or
/// startup scanner) picks it up at reset_at.
enum StepError {
    Skipped(String),
    Failed(String),
    Paused {
        paused_dispatch_id: String,
        runtime: String,
        reset_at: String,
    },
}

impl From<anyhow::Error> for StepError {
    fn from(err: anyhow::Error) -> Self {
        StepError::Failed(format!("{:#}", err))
    }
}

fn execute_step(
    node: &LoopGraphNode,
    loop_run_id: &str,
    node_id: &str,
    db_path: &PathBuf,
    opts: &Opts,
) -> std::result::Result<serde_json::Value, StepError> {
    let params = graph_params(node);
    match node.node_type.as_str() {
        "dispatch" => handle_dispatch(&params, loop_run_id, node_id, db_path, opts),
        "methodology_run" => handle_methodology_run(&params, db_path, opts),
        "diagnose" | "apply" | "review" | "war_room" | "score" | "input" | "output" => {
            Err(StepError::Skipped(format!(
                "kind '{}' is not wired yet in v2.14.0 — Task #14 follow-up",
                node.node_type
            )))
        }
        // Legacy IFTTT-style kinds (migrated workflows). Same skip-with-note
        // until the legacy adapters land.
        other => Err(StepError::Skipped(format!(
            "kind '{}' is a legacy/non-LLM node — executor wiring deferred",
            other
        ))),
    }
}

fn handle_dispatch(
    params: &serde_json::Map<String, serde_json::Value>,
    loop_run_id: &str,
    node_id: &str,
    db_path: &PathBuf,
    opts: &Opts,
) -> std::result::Result<serde_json::Value, StepError> {
    let runtime = param_str(params, "runtime")
        .ok_or_else(|| StepError::Failed("dispatch: 'runtime' is required".into()))?;
    let prompt = param_str(params, "prompt")
        .ok_or_else(|| StepError::Failed("dispatch: 'prompt' is required".into()))?;
    let model = param_str(params, "model").map(String::from);
    let agent_slug = param_str(params, "agent_slug").map(String::from);

    // v2.15.4 (war_room E063A89E) — pause-and-wake pre-flight gate.
    // Before invoking dispatch::run, check if the runtime is currently
    // rate-limited AND the user's policy is `pause-and-wake`. If so,
    // persist a paused_dispatches row and bail with StepError::Paused
    // so the loop runner can record paused_until + pick this up at
    // reset_at.
    //
    // Standalone dispatches (no loop_run_id context) keep the v2.15.2
    // "degrade to stop-and-notify" behavior in dispatch.rs.
    if let Ok(Some(resets_at)) = crate::quota::lookup_future(db_path, runtime) {
        let policy = if let Ok(c) = rusqlite::Connection::open(db_path) {
            crate::quota::read_exhaustion_policy(&c)
                .unwrap_or(crate::quota::ExhaustionPolicy::AskOrDefault)
        } else {
            crate::quota::ExhaustionPolicy::AskOrDefault
        };
        if matches!(policy, crate::quota::ExhaustionPolicy::PauseAndWake) {
            if let Ok(c) = rusqlite::Connection::open(db_path) {
                let inserted = crate::paused_dispatches::insert_new(
                    &c,
                    crate::paused_dispatches::InsertPausedDispatch {
                        runtime,
                        reset_at: &resets_at,
                        loop_run_id: Some(loop_run_id),
                        step_id: Some(node_id),
                        prompt,
                        model: model.as_deref(),
                        agent_slug: agent_slug.as_deref(),
                        workspace_root: None,
                    },
                );
                if let Ok(paused_id) = inserted {
                    // Emit dispatch_exhausted with policy=pause-and-wake
                    // and the new paused_dispatch id so observability
                    // ties the event to the persisted row.
                    crate::events_publisher::publish_dispatch_exhausted(
                        &c,
                        runtime,
                        &resets_at,
                        "pause-and-wake",
                        Some(&paused_id),
                        None,
                        &chrono::Utc::now().to_rfc3339(),
                    );
                    return Err(StepError::Paused {
                        paused_dispatch_id: paused_id,
                        runtime: runtime.to_string(),
                        reset_at: resets_at,
                    });
                }
            }
        }
    }

    // Capture the latest execution_logs.id BEFORE the dispatch so we
    // can identify the row this dispatch creates by reading "highest
    // id > before". dispatch::run() writes to execution_logs as part
    // of its normal path; this is the only way to round-trip the
    // response without modifying the existing dispatch signature.
    // War-room (codex+gemini, war_room_id 0EDD3A31) caught the prior
    // "handle_dispatch returns only {runtime, ok:true}" gap — this
    // fix gives downstream steps real upstream output to reference.
    let before_max_id: i64 = {
        let conn = crate::db::open_readonly(db_path).map_err(StepError::from)?;
        conn.query_row(
            "SELECT COALESCE(MAX(id), 0) FROM execution_logs",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };

    crate::commands::dispatch::run(
        runtime,
        prompt,
        model,
        agent_slug,
        None,  // session_id — loops don't share a session today
        None,  // war_room_id — out of scope for v2.14.0
        None,  // war_room_round
        false, // stream
        false, // stream_jsonl
        false, // with_tools — loop steps run non-interactively
        db_path,
        opts,
    )?;

    // Read back the freshly-written row (the one with id > before_max_id
    // and matching runtime). This is the executor's view of the
    // dispatch's outcome — response text + cost + model + execution_log_id —
    // and is what {{steps.<id>.output.field}} resolves through in v2.14.1.
    let conn = crate::db::open_readonly(db_path).map_err(StepError::from)?;
    let row: Option<(i64, Option<String>, Option<String>, Option<String>, Option<f64>)> = conn
        .query_row(
            "SELECT id, response_text, model, status, cost_usd
               FROM execution_logs
              WHERE id > ?1 AND runtime = ?2
              ORDER BY id DESC
              LIMIT 1",
            params![before_max_id, runtime],
            |r| Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get::<_, Option<f64>>(4)?,
            )),
        )
        .ok();

    Ok(match row {
        Some((log_id, response, used_model, status, cost)) => serde_json::json!({
            "runtime": runtime,
            "execution_log_id": log_id,
            "response": response,
            "model": used_model,
            "status": status,
            "cost_usd": cost,
        }),
        None => serde_json::json!({
            "runtime": runtime,
            "execution_log_id": null,
            "warning": "dispatch ran but no execution_logs row found after-the-fact",
        }),
    })
}

fn handle_methodology_run(
    params: &serde_json::Map<String, serde_json::Value>,
    db_path: &PathBuf,
    opts: &Opts,
) -> std::result::Result<serde_json::Value, StepError> {
    let slug = param_str(params, "slug")
        .ok_or_else(|| StepError::Failed("methodology_run: 'slug' is required".into()))?;
    // Models / reps overrides defer to v2.14.1; first-cut just runs the
    // methodology as-defined.
    let run_opts = crate::methodology::runner::RunOptions::default();
    let summary = crate::methodology::runner::run_by_slug(slug, db_path, &run_opts)
        .map_err(StepError::from)?;
    let _ = opts; // quiet unused-arg warning when emit paths run via run-wide opts
    Ok(serde_json::json!({
        "methodology_slug": slug,
        "run_id": summary.run_id,
        "status": summary.status,
    }))
}

// ── Resume (v2.15.4 — pause-and-wake) ─────────────────────────────────
//
// Pause-and-wake is OSS primitive — the local lifecycle of a paused
// dispatch row lives here. The PRO upgrades (cross-machine sync, hosted
// scheduler that fires even when this laptop is asleep, analytics on
// pause patterns, team-shared paused work) ship in ato-cloud per
// docs/tiers.md.

/// Re-fire one paused dispatch. Steinberger-borrow: when we abandon,
/// emit a decision brief — runtime, what was tried, pause history,
/// recommended next action — into loop_runs.error AND stderr, not
/// just status='error'. The maintainer-orchestrator skill's "never
/// ask with only a URL or status label" rule applied to our exhaustion
/// path.
fn run_resume(paused_dispatch_id: &str, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;

    // Transactional claim: paused → resuming. Any other status (already
    // resuming/resumed/abandoned) bails — prevents double-resume races
    // between the manual CLI and the startup scanner.
    let row = crate::paused_dispatches::claim_for_resume(&conn, paused_dispatch_id)?;

    // Pre-flight (codex's amendment to v2.15.4): re-verify the runtime
    // is no longer flagged exhausted before firing. If subscription_resets
    // still holds a future reset_at, re-pause with that fresh value;
    // pause_count bumps and may flip to abandoned per max_pause_count.
    if let Ok(Some(still_resets_at)) = crate::quota::lookup_future(db_path, &row.runtime) {
        let outcome = crate::paused_dispatches::re_pause_or_abandon(
            &conn,
            paused_dispatch_id,
            &still_resets_at,
            "wake-time pre-flight: subscription_resets still holds a future reset_at",
        )?;
        let now = chrono::Utc::now().to_rfc3339();
        crate::events_publisher::publish_dispatch_resumed(
            &conn,
            paused_dispatch_id,
            &row.runtime,
            outcome,
            row.pause_count + 1,
            Some(&still_resets_at),
            &now,
        );

        if outcome == "abandoned" {
            // Steinberger-borrow: emit a decision brief, not just a
            // status. Updates loop_runs.error with a richer narrative
            // so the user sees what was tried + what to do next.
            let brief = build_abandon_brief(&row, &still_resets_at);
            let _ = conn.execute(
                "UPDATE loop_runs SET error = ?1 WHERE paused_dispatch_id = ?2 OR id = ?3",
                rusqlite::params![brief.clone(), paused_dispatch_id, row.loop_run_id.clone().unwrap_or_default()],
            );
            // Reset the loop_runs row mirror — re_pause_or_abandon already
            // cleared paused_dispatch_id; we just enriched error.
            eprintln!("\n{}\n", brief);
            if opts.human {
                emit_human(&format!(
                    "✗ paused dispatch {} ABANDONED on {} — see decision brief above",
                    paused_dispatch_id, row.runtime
                ));
            } else {
                emit_json(&serde_json::json!({
                    "paused_dispatch_id": paused_dispatch_id,
                    "outcome": "abandoned",
                    "runtime": row.runtime,
                    "pause_count": row.pause_count + 1,
                    "decision_brief": brief,
                }))?;
            }
            return Ok(());
        }

        // outcome == "re_paused"
        if opts.human {
            emit_human(&format!(
                "⏸ paused dispatch {} still exhausted on {} — re-paused until {} (pause_count={})",
                paused_dispatch_id, row.runtime, still_resets_at, row.pause_count + 1
            ));
        } else {
            emit_json(&serde_json::json!({
                "paused_dispatch_id": paused_dispatch_id,
                "outcome": "re_paused",
                "runtime": row.runtime,
                "reset_at": still_resets_at,
                "pause_count": row.pause_count + 1,
            }))?;
        }
        return Ok(());
    }

    // Runtime is clear — re-fire the original dispatch. Standalone
    // (loop_run_id=None) and loop-bound paths share the same dispatch
    // entrypoint; on success we mark_resumed + clear the loop_runs mirror.
    let fire_result = crate::commands::dispatch::run(
        &row.runtime,
        &row.prompt,
        row.model.clone(),
        row.agent_slug.clone(),
        None,  // session_id — paused loop steps don't share a session
        None,  // war_room_id
        None,  // war_room_round
        false, // stream
        false, // stream_jsonl
        false, // with_tools — non-interactive
        db_path,
        opts,
    );

    let now = chrono::Utc::now().to_rfc3339();
    match fire_result {
        Ok(_) => {
            crate::paused_dispatches::mark_resumed(
                &conn,
                paused_dispatch_id,
                "wake-time dispatch succeeded",
            )?;
            crate::events_publisher::publish_dispatch_resumed(
                &conn,
                paused_dispatch_id,
                &row.runtime,
                "resumed",
                row.pause_count,
                Some(&row.reset_at),
                &now,
            );
            if opts.human {
                emit_human(&format!(
                    "✓ paused dispatch {} resumed on {}",
                    paused_dispatch_id, row.runtime
                ));
            } else {
                emit_json(&serde_json::json!({
                    "paused_dispatch_id": paused_dispatch_id,
                    "outcome": "resumed",
                    "runtime": row.runtime,
                }))?;
            }
            Ok(())
        }
        Err(err) => {
            // Dispatch failed at wake — could be transient (network,
            // 5xx) or could be a fresh exhaustion not yet reflected in
            // subscription_resets. We leave the row in 'resuming' so a
            // future scan or manual re-resume picks it up; the caller
            // sees the error so they can decide. (Don't mark_resumed —
            // that would lie about success.)
            anyhow::bail!(
                "wake-time dispatch failed for paused {} ({}): {:#}",
                paused_dispatch_id,
                row.runtime,
                err
            )
        }
    }
}

/// Scan for paused dispatches whose reset_at has passed; resume each.
/// Designed to be safe to call on every CLI startup AND from a launchd
/// tick — list_due reads only rows in status='paused' so already-claimed
/// rows are skipped. Failures of one row don't block the rest.
fn run_resume_due(db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let due_ids = crate::paused_dispatches::list_due(&conn)?;
    drop(conn);

    if due_ids.is_empty() {
        if opts.human {
            emit_human("No paused dispatches are due to resume.");
        } else {
            emit_json(&serde_json::json!({
                "scanned": 0,
                "resumed": 0,
                "outcomes": [],
            }))?;
        }
        return Ok(());
    }

    let mut outcomes: Vec<serde_json::Value> = Vec::new();
    for id in &due_ids {
        match run_resume(id, db_path, opts) {
            Ok(()) => {
                outcomes.push(serde_json::json!({
                    "paused_dispatch_id": id,
                    "ok": true,
                }));
            }
            Err(err) => {
                outcomes.push(serde_json::json!({
                    "paused_dispatch_id": id,
                    "ok": false,
                    "error": format!("{:#}", err),
                }));
                if opts.human {
                    emit_human(&format!(
                        "  ✗ resume failed for {} — {}",
                        id, err
                    ));
                }
            }
        }
    }

    if opts.human {
        emit_human(&format!(
            "Scanned {} due paused dispatches; see per-row outcomes above.",
            due_ids.len()
        ));
    } else {
        emit_json(&serde_json::json!({
            "scanned": due_ids.len(),
            "resumed": outcomes.iter().filter(|o| o["ok"].as_bool() == Some(true)).count(),
            "outcomes": outcomes,
        }))?;
    }
    Ok(())
}

/// Steinberger-borrow: when a pause-and-wake row is abandoned (pause_count
/// would exceed max_pause_count), emit a decision brief — not just a
/// status. The brief names what was tried, the pause history, and the
/// recommended next action. Maps maintainer-orchestrator's "never escalate
/// with only a URL or status label" rule onto our exhaustion path.
fn build_abandon_brief(row: &crate::paused_dispatches::PausedDispatch, last_reset_at: &str) -> String {
    let agent_line = match &row.agent_slug {
        Some(slug) => format!("agent={}, ", slug),
        None => String::new(),
    };
    let model_line = match &row.model {
        Some(m) => format!("model={}, ", m),
        None => String::new(),
    };
    let loop_line = match &row.loop_run_id {
        Some(lr) => format!("loop_run_id={}, ", lr),
        None => "standalone, ".to_string(),
    };
    format!(
        "── DECISION BRIEF: pause-and-wake ABANDONED ──\n\
         paused_dispatch_id: {pid}\n\
         runtime:            {rt}\n\
         {agent}{model}{loop_}pause_count: {pc} / max {pcmax}\n\
         first_paused_at:    {paused_at}\n\
         last_reset_at:      {last_reset}\n\
         \n\
         WHAT WAS TRIED:\n\
         · Loop step hit subscription exhaustion on {rt}\n\
         · Policy=pause-and-wake persisted the dispatch and re-attempted at each reset\n\
         · {pc} consecutive wakes still found {rt} flagged exhausted\n\
         · Reached max_pause_count={pcmax} — abandoning to surface the decision\n\
         \n\
         RECOMMENDED NEXT ACTIONS (pick one):\n\
         · Switch policy to fallback-chain so peer runtimes pick this work up automatically\n\
           (Settings → Resilience → Fallback chain; or `ato config set exhaustion-policy fallback-chain`)\n\
         · Manually re-run with `ato dispatch <other-runtime> '...'` — your fallback order is shown in Settings → Resilience\n\
         · Inspect the audit trail: `sqlite3 ~/.ato/local.db \"SELECT audit_json FROM paused_dispatches WHERE id='{pid}'\"`\n\
         · If the runtime is permanently broken (not just exhausted), disable it in Settings → Runtimes\n",
        pid = row.id,
        rt = row.runtime,
        agent = agent_line,
        model = model_line,
        loop_ = loop_line,
        pc = row.pause_count + 1,
        pcmax = row.max_pause_count,
        paused_at = row.paused_at,
        last_reset = last_reset_at,
    )
}

// ── Schedules (v2.14.0 — SQLite-backed surface; launchd integration TBD) ──
//
// The CLI writes/reads `loop_schedules` rows directly so the contract is
// honest from day 1: a user can `ato loop schedule create weekly --cron
// "0 9 * * 1"` and the row lands. The launchd plist generation that
// actually FIRES the loop at the cron expression's cadence is a v2.14.1
// follow-up — until then, a user wiring real recurrence does it via
// their own crontab:
//
//   0 9 * * 1  /opt/homebrew/bin/ato loop run weekly
//
// The scheduler tick in #16's launchd half will read from this table so
// the row written today is already in the right shape.

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LoopScheduleRow {
    id: String,
    loop_id: String,
    cron_expr: String,
    enabled: bool,
    last_fired_at: Option<String>,
    next_fire_at: Option<String>,
    created_at: String,
}

fn run_schedule_create(
    slug_or_id: String,
    cron: String,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    if cron.trim().is_empty() {
        anyhow::bail!("--cron must be a non-empty expression");
    }
    let conn = db::open_readwrite(db_path)?;
    // Resolve the loop FIRST so we get a clean "loop not found" error
    // instead of an FK violation from the INSERT below.
    let loop_row = load_loop(&conn, &slug_or_id)?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO loop_schedules (
            id, loop_id, cron_expr, enabled, last_fired_at, next_fire_at, created_at
         ) VALUES (?1, ?2, ?3, 1, NULL, NULL, ?4)",
        params![id, loop_row.id, cron, now],
    )
    .context("insert loop_schedule")?;

    let row = LoopScheduleRow {
        id: id.clone(),
        loop_id: loop_row.id.clone(),
        cron_expr: cron.clone(),
        enabled: true,
        last_fired_at: None,
        next_fire_at: None,
        created_at: now,
    };
    if opts.human {
        emit_human(&format!(
            "Scheduled loop '{}' on '{}' (schedule_id: {})",
            loop_row.slug, cron, id
        ));
        emit_human(
            "NOTE: launchd auto-fire ships in v2.14.1. Until then, wire crontab manually:",
        );
        emit_human(&format!(
            "  {}  $(which ato) loop run {}",
            cron, loop_row.slug
        ));
    } else {
        emit_json(&row)?;
    }
    Ok(())
}

fn run_schedule_list(slug_or_id: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT s.id, s.loop_id, s.cron_expr, s.enabled, s.last_fired_at, s.next_fire_at, s.created_at
           FROM loop_schedules s
           JOIN loops l ON s.loop_id = l.id
          WHERE l.{} = ?1
          ORDER BY s.created_at DESC",
        id_or_slug_column(&slug_or_id),
    ))?;
    let mut rows = Vec::new();
    let iter = stmt.query_map(params![slug_or_id], |row| {
        Ok(LoopScheduleRow {
            id: row.get(0)?,
            loop_id: row.get(1)?,
            cron_expr: row.get(2)?,
            enabled: row.get::<_, i32>(3)? != 0,
            last_fired_at: row.get(4)?,
            next_fire_at: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;
    for r in iter {
        rows.push(r?);
    }

    if opts.human {
        if rows.is_empty() {
            emit_human(&format!("No schedules for loop {}.", slug_or_id));
        } else {
            emit_human(&format!("Schedules for {} ({}):", slug_or_id, rows.len()));
            for r in &rows {
                emit_human(&format!(
                    "  • {}  cron='{}'  enabled={}  last_fired={}  next_fire={}",
                    r.id,
                    r.cron_expr,
                    r.enabled,
                    r.last_fired_at.clone().unwrap_or_else(|| "(never)".into()),
                    r.next_fire_at.clone().unwrap_or_else(|| "(pending)".into()),
                ));
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

fn run_schedule_delete(schedule_id: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let affected = conn
        .execute(
            "DELETE FROM loop_schedules WHERE id = ?1",
            params![schedule_id],
        )
        .context("delete loop_schedule")?;
    if affected == 0 {
        anyhow::bail!("schedule not found: {}", schedule_id);
    }
    if opts.human {
        emit_human(&format!("Deleted schedule {}", schedule_id));
    } else {
        emit_json(&serde_json::json!({ "deleted": schedule_id }))?;
    }
    Ok(())
}

fn run_runs_list(slug_or_id: String, limit: i64, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let cap = limit.clamp(1, 500);
    let sql = format!(
        "SELECT lr.id, lr.loop_id, lr.status, lr.started_at, lr.finished_at,
                lr.error, lr.triggered_by, lr.variables
           FROM loop_runs lr
           JOIN loops l ON lr.loop_id = l.id
          WHERE l.{} = ?1
          ORDER BY lr.started_at DESC
          LIMIT ?2",
        id_or_slug_column(&slug_or_id),
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = Vec::new();
    let iter = stmt.query_map(params![slug_or_id, cap], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;
    for r in iter {
        let (id, loop_id, status, started_at, finished_at, error, triggered_by, raw_vars) = r?;
        let variables = parse_json_field("variables", raw_vars)?;
        rows.push(LoopRunRow {
            id,
            loop_id,
            status,
            started_at,
            finished_at,
            error,
            triggered_by,
            variables,
        });
    }
    if opts.human {
        if rows.is_empty() {
            emit_human(&format!("No runs yet for loop {}", slug_or_id));
        } else {
            emit_human(&format!("Runs of {} ({}):", slug_or_id, rows.len()));
            for r in &rows {
                let dur = match (&r.finished_at, r.started_at.as_str()) {
                    (Some(end), start) => format!(" ({} → {})", start, end),
                    (None, start) => format!(" (started {})", start),
                };
                emit_human(&format!("  • {} — status={}{}", r.id, r.status, dur));
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

fn run_runs_show(run_id: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let run_raw = conn
        .query_row(
            "SELECT id, loop_id, status, started_at, finished_at, error,
                    triggered_by, variables
               FROM loop_runs
              WHERE id = ?1",
            params![run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .with_context(|| format!("run not found: {}", run_id))?;
    let (id, loop_id, status, started_at, finished_at, error, triggered_by, raw_vars) = run_raw;
    let variables = parse_json_field("variables", raw_vars)?;
    let run = LoopRunRow {
        id,
        loop_id,
        status,
        started_at,
        finished_at,
        error,
        triggered_by,
        variables,
    };

    let mut stmt = conn.prepare(
        "SELECT id, loop_run_id, node_id, node_type, status,
                started_at, finished_at, input, output, error,
                execution_log_id
           FROM loop_run_steps
          WHERE loop_run_id = ?1
          ORDER BY started_at ASC NULLS LAST, id ASC",
    )?;
    let mut steps = Vec::new();
    let iter = stmt.query_map(params![run_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<i64>>(10)?,
        ))
    })?;
    for r in iter {
        let (
            id,
            loop_run_id,
            node_id,
            node_type,
            status,
            started_at,
            finished_at,
            input_raw,
            output_raw,
            error,
            execution_log_id,
        ) = r?;
        let input = parse_json_field("input", input_raw)?;
        let output = parse_json_field("output", output_raw)?;
        steps.push(LoopRunStepRow {
            id,
            loop_run_id,
            node_id,
            node_type,
            status,
            started_at,
            finished_at,
            input,
            output,
            error,
            execution_log_id,
        });
    }

    let bundle = LoopRunWithSteps { run, steps };
    if opts.human {
        emit_human(&format!(
            "Run {} of loop {} — status={}",
            bundle.run.id, bundle.run.loop_id, bundle.run.status
        ));
        emit_human(&format!("  started: {}", bundle.run.started_at));
        if let Some(f) = &bundle.run.finished_at {
            emit_human(&format!("  finished: {}", f));
        }
        if let Some(e) = &bundle.run.error {
            emit_human(&format!("  error: {}", e));
        }
        emit_human(&format!("  steps ({}):", bundle.steps.len()));
        for s in &bundle.steps {
            emit_human(&format!(
                "    • [{}] {} ({}) — {}",
                s.status, s.node_id, s.node_type,
                s.error.clone().unwrap_or_else(|| "ok".into())
            ));
        }
    } else {
        emit_json(&bundle)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Mirrors the production schema (only the parts the loops CLI touches)
    /// so we can exercise the SQL helpers against an in-memory DB without
    /// pulling in the desktop's schema crate.
    fn make_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute(
            "CREATE TABLE loops (
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
            )",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn slugify_handles_punctuation_and_unicode_separators() {
        assert_eq!(slugify("Weekly Security Review"), "weekly-security-review");
        assert_eq!(slugify("  spaces  around  "), "spaces-around");
        assert_eq!(slugify("!!!only-punctuation!!!"), "only-punctuation");
        assert_eq!(slugify(""), "loop");
        // Truncates to 64 chars.
        let long = "a".repeat(200);
        assert_eq!(slugify(&long).len(), 64);
    }

    #[test]
    fn unique_slug_appends_numeric_suffix_on_collision() {
        let conn = make_db();
        let now = "2026-06-10T00:00:00Z";
        conn.execute(
            "INSERT INTO loops (id, slug, name, graph, created_at, updated_at)
             VALUES ('l-1', 'weekly-review', 'L1', '{}', ?1, ?1)",
            params![now],
        )
        .unwrap();
        assert_eq!(unique_slug(&conn, "fresh").unwrap(), "fresh");
        assert_eq!(unique_slug(&conn, "weekly-review").unwrap(), "weekly-review-2");
        // Second collision after we insert -2 explicitly.
        conn.execute(
            "INSERT INTO loops (id, slug, name, graph, created_at, updated_at)
             VALUES ('l-2', 'weekly-review-2', 'L2', '{}', ?1, ?1)",
            params![now],
        )
        .unwrap();
        assert_eq!(unique_slug(&conn, "weekly-review").unwrap(), "weekly-review-3");
    }

    #[test]
    fn parse_json_field_round_trip_and_empty_handling() {
        assert!(parse_json_field("x", None).unwrap().is_none());
        assert!(parse_json_field("x", Some("".into())).unwrap().is_none());
        let v = parse_json_field("x", Some(r#"{"a":1}"#.into())).unwrap().unwrap();
        assert_eq!(v["a"], 1);
        // Bad JSON surfaces a contextual error rather than silently returning None.
        let err = parse_json_field("graph", Some("not json".into())).unwrap_err();
        assert!(format!("{}", err).contains("invalid graph JSON"));
    }

    #[test]
    fn id_or_slug_column_routes_by_uuid_shape() {
        // Real UUIDs go to the id column.
        assert_eq!(id_or_slug_column("b3d6dbe2-1111-4222-9333-444444444444"), "id");
        assert_eq!(id_or_slug_column(&Uuid::new_v4().to_string()), "id");
        // Anything that doesn't parse as a UUID is treated as a slug.
        assert_eq!(id_or_slug_column("weekly-security-review"), "slug");
        assert_eq!(id_or_slug_column("loop"), "slug");
        assert_eq!(id_or_slug_column("not-a-uuid-but-has-hyphens"), "slug");
        // Adjacent edge: a slug that LOOKS UUID-y but isn't (missing dashes
        // / wrong group sizes / non-hex chars) still resolves to slug.
        assert_eq!(id_or_slug_column("b3d6dbe2zzzzzzz"), "slug");
    }

    #[test]
    fn resolver_does_not_let_a_uuid_shaped_slug_shadow_anothers_id() {
        // The war-room (72D76B07) catch: without this fix, a malicious or
        // accidental slug like "b3d6dbe2-…" shadows another loop's id and
        // `load_loop` returns the wrong row. After the fix the resolver
        // dispatches on UUID-shape first, so loading by id returns the
        // id-row even when a slug-row exists with that exact string in
        // the slug column.
        let conn = make_db();
        let now = "2026-06-10T00:00:00Z";
        let real_id = "b3d6dbe2-1111-4222-9333-444444444444";
        // Loop A — the legitimate one with that UUID as id.
        conn.execute(
            "INSERT INTO loops (id, slug, name, graph, created_at, updated_at)
             VALUES (?1, 'real-loop', 'Real', '{}', ?2, ?2)",
            params![real_id, now],
        )
        .unwrap();
        // Loop B — a second loop whose SLUG equals A's id. Without the
        // resolver fix, `WHERE id = ?1 OR slug = ?1` would match BOTH rows
        // and `query_row` would non-deterministically return one or the
        // other.
        conn.execute(
            "INSERT INTO loops (id, slug, name, graph, created_at, updated_at)
             VALUES ('00000000-0000-4000-8000-000000000099', ?1, 'Shadow', '{}', ?2, ?2)",
            params![real_id, now],
        )
        .unwrap();

        // Loading by the UUID resolves to Loop A.
        let a = load_loop(&conn, real_id).expect("load by id");
        assert_eq!(a.id, real_id, "uuid input must resolve via id column");
        assert_eq!(a.name, "Real");

        // Loading by Loop B's slug ('00000000-0000-4000-8000-000000000099')
        // would also be a UUID and would resolve via id. To prove the
        // slug path also works, load Loop A by its actual slug.
        let a_via_slug = load_loop(&conn, "real-loop").expect("load by slug");
        assert_eq!(a_via_slug.id, real_id);
    }

    // v2.15.4 — pause-and-wake decision brief tests.
    //
    // Steinberger's maintainer-orchestrator rule: "never ask with only a URL
    // or status label." When pause-and-wake hits max_pause_count and
    // abandons, the user needs the brief — runtime, history, recommended
    // actions — not just `status='error'`.

    fn mk_paused_dispatch(
        id: &str,
        runtime: &str,
        pause_count: i64,
        max: i64,
        model: Option<&str>,
        agent: Option<&str>,
        loop_run_id: Option<&str>,
    ) -> crate::paused_dispatches::PausedDispatch {
        crate::paused_dispatches::PausedDispatch {
            id: id.into(),
            runtime: runtime.into(),
            reset_at: "2026-06-11T12:00:00+00:00".into(),
            loop_run_id: loop_run_id.map(String::from),
            step_id: None,
            prompt: "dispatch payload that does not appear in brief".into(),
            model: model.map(String::from),
            agent_slug: agent.map(String::from),
            workspace_root: None,
            pause_count,
            max_pause_count: max,
            status: "resuming".into(),
            paused_at: "2026-06-11T08:00:00+00:00".into(),
            resumed_at: None,
            abandoned_at: None,
            audit_json: None,
            created_at: "2026-06-11T08:00:00+00:00".into(),
        }
    }

    #[test]
    fn abandon_brief_includes_all_critical_fields_and_actions() {
        let row = mk_paused_dispatch(
            "pd-abc-123",
            "codex",
            3,
            3,
            Some("gpt-5"),
            Some("eng-manager"),
            Some("lr-42"),
        );
        let brief = build_abandon_brief(&row, "2026-06-11T15:00:00+00:00");

        // Names the row + runtime + pause count vs max so the user can
        // grep audit_json.
        assert!(brief.contains("pd-abc-123"), "brief must name paused_dispatch_id");
        assert!(brief.contains("codex"), "brief must name runtime");
        assert!(brief.contains("4 / max 3"), "brief must show pause_count+1 vs max");
        assert!(brief.contains("agent=eng-manager"), "brief must surface agent");
        assert!(brief.contains("model=gpt-5"), "brief must surface model");
        assert!(brief.contains("loop_run_id=lr-42"), "brief must surface loop linkage");
        assert!(brief.contains("2026-06-11T08:00:00"), "brief must name first paused_at");
        assert!(brief.contains("2026-06-11T15:00:00"), "brief must name last reset_at");

        // The decision-brief structure: what was tried + next actions.
        // Without these the brief is just a status label — which is
        // exactly what Steinberger's rule prohibits.
        assert!(brief.contains("WHAT WAS TRIED"), "brief must enumerate what was tried");
        assert!(brief.contains("RECOMMENDED NEXT ACTIONS"), "brief must offer next steps");
        assert!(brief.contains("fallback-chain"), "brief must offer the policy switch path");
        assert!(brief.contains("ato dispatch"), "brief must offer manual retry path");
        assert!(brief.contains("audit_json"), "brief must point at the audit trail");
    }

    #[test]
    fn abandon_brief_handles_standalone_dispatch_without_loop_or_agent_or_model() {
        let row = mk_paused_dispatch("pd-standalone", "claude", 2, 2, None, None, None);
        let brief = build_abandon_brief(&row, "2026-06-11T15:00:00+00:00");

        // Standalone dispatches still get a brief.
        assert!(brief.contains("pd-standalone"));
        assert!(brief.contains("claude"));
        assert!(brief.contains("standalone"), "brief must mark non-loop dispatches");
        // No agent/model lines should leak when those fields are None.
        assert!(!brief.contains("agent="), "brief must omit agent line when agent_slug is None");
        assert!(!brief.contains("model="), "brief must omit model line when model is None");
        // Recommended actions still present — the prescription doesn't
        // depend on the dispatch shape.
        assert!(brief.contains("RECOMMENDED NEXT ACTIONS"));
    }
}
