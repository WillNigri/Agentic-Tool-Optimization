// commands/loops.rs — v2.14 Loop Composer persistence layer.
//
// Per docs/eager-yawning-crane.md plan section C. SQLite-backed CRUD
// for the `loops`, `loop_runs`, `loop_run_steps`, and `loop_schedules`
// tables (schema in src/schema.rs). This module is CRUD-only — read,
// write, list, delete. No execution; no scheduling; no cloud sync.
//
// Out of scope here (separate tasks):
//   - executor that walks the loop graph + spawns dispatch / methodology
//     / diagnose / review (Task #14)
//   - localStorage → SQLite migration of the v2.13 `workflows` data
//     (Task #15, in commands/workflows.rs alongside the legacy
//     file-based list_workflows / save_workflow surface)
//   - scheduling tick (Task #16)
//   - cloud sync of loops across devices (v2.14.1, ato-cloud only)

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::DbState;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Loop {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    /// Canonical loop graph as JSON: { nodes: LoopStep[], edges: LoopEdge[] }.
    pub graph: serde_json::Value,
    pub variables: Option<serde_json::Value>,
    /// "manual" | "cron" | "event"
    pub trigger_kind: String,
    pub trigger_config: Option<serde_json::Value>,
    /// "manual" | "migrated-from-automations" | "skill" | "group"
    pub source: String,
    pub source_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoopCreateInput {
    pub name: String,
    pub description: Option<String>,
    /// Optional — if omitted, derived from name.
    pub slug: Option<String>,
    /// Codex R4: optional override for the default-enabled behavior.
    /// None means "use the default" (enabled=true); Some(false) lets
    /// the caller persist a newly-created disabled workflow correctly.
    pub enabled: Option<bool>,
    pub graph: serde_json::Value,
    pub variables: Option<serde_json::Value>,
    pub trigger_kind: Option<String>,
    pub trigger_config: Option<serde_json::Value>,
    pub source: Option<String>,
    pub source_ref: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoopUpdateInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub graph: Option<serde_json::Value>,
    pub variables: Option<serde_json::Value>,
    pub trigger_kind: Option<String>,
    pub trigger_config: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoopRun {
    pub id: String,
    pub loop_id: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
    pub triggered_by: Option<String>,
    pub variables: Option<serde_json::Value>,
    /// Attribution PR (2026-06-13) — initiator provenance so the Loop
    /// Composer run-history list can render an InitiatorBadge per run.
    /// NULL on runs recorded before the attribution backfill.
    pub initiator_kind: Option<String>,
    pub client_surface: Option<String>,
    pub initiator_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoopRunStep {
    pub id: String,
    pub loop_run_id: String,
    pub node_id: String,
    pub node_type: String,
    pub status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub input: Option<serde_json::Value>,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub execution_log_id: Option<String>,
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Derive a URL-safe slug from a free-text name. Lowercase, alphanumeric +
/// hyphens; collapses runs of separators; trims to 64 chars. The DB has a
/// UNIQUE constraint on `slug` so callers must handle collisions — see
/// `unique_slug` below.
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

/// Return a slug that does not collide with any existing row. Appends
/// `-2`, `-3`, … on collision. Caller already holds the DB lock.
fn unique_slug(conn: &rusqlite::Connection, base: &str) -> Result<String, String> {
    let mut candidate = base.to_string();
    let mut suffix = 2;
    loop {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM loops WHERE slug = ?1",
                params![candidate],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists == 0 {
            return Ok(candidate);
        }
        candidate = format!("{}-{}", base, suffix);
        suffix += 1;
        if suffix > 1000 {
            return Err("slug-exhaustion".into());
        }
    }
}

fn parse_json(field: &str, raw: Option<String>) -> Result<Option<serde_json::Value>, String> {
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => serde_json::from_str::<serde_json::Value>(&s)
            .map(Some)
            .map_err(|e| format!("invalid {} json: {}", field, e)),
    }
}

fn row_to_loop(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String, String, Option<String>, i32, String, Option<String>, String, Option<String>, String, Option<String>, String, String)> {
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

fn assemble_loop(
    raw: (String, String, String, Option<String>, i32, String, Option<String>, String, Option<String>, String, Option<String>, String, String),
) -> Result<Loop, String> {
    let (id, slug, name, description, enabled, graph_raw, variables_raw, trigger_kind, trigger_config_raw, source, source_ref, created_at, updated_at) = raw;
    let graph = serde_json::from_str(&graph_raw)
        .map_err(|e| format!("invalid graph json on loop {}: {}", id, e))?;
    let variables = parse_json("variables", variables_raw)?;
    let trigger_config = parse_json("trigger_config", trigger_config_raw)?;
    Ok(Loop {
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

const LOOP_SELECT: &str = "SELECT id, slug, name, description, enabled, graph, variables, trigger_kind, trigger_config, source, source_ref, created_at, updated_at FROM loops";

/// Loop identifier resolution. Callers pass either a UUID `id` or a kebab-case
/// `slug` — we detect which by attempting a UUID parse. Without this, the
/// naive `WHERE id = ?1 OR slug = ?1` is nondeterministic: a malicious slug
/// of UUID shape (e.g. `b3d6dbe2-…`) shadows another loop's id and writes
/// hit the wrong row. War-room 72D76B07 (codex seat) caught this on the
/// v2.14 foundation pass; fix landed before LLM-aware kinds (#12) layered
/// more code on top of the same broken WHERE clauses.
fn id_or_slug_column(input: &str) -> &'static str {
    if uuid::Uuid::parse_str(input).is_ok() {
        "id"
    } else {
        "slug"
    }
}

// ── Loop CRUD ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_loops(db: State<'_, DbState>) -> Result<Vec<Loop>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let sql = format!("{} ORDER BY updated_at DESC", LOOP_SELECT);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_loop).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(assemble_loop(r.map_err(|e| e.to_string())?)?);
    }
    Ok(out)
}

#[tauri::command]
pub fn get_loop(db: State<'_, DbState>, id: String) -> Result<Loop, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let sql = format!("{} WHERE {} = ?1", LOOP_SELECT, id_or_slug_column(&id));
    let raw = conn
        .query_row(&sql, params![id], row_to_loop)
        .map_err(|e| format!("loop not found: {}", e))?;
    assemble_loop(raw)
}

#[tauri::command]
pub fn create_loop(db: State<'_, DbState>, input: LoopCreateInput) -> Result<Loop, String> {
    if input.name.trim().is_empty() {
        return Err("name-empty".into());
    }
    let trigger_kind = input
        .trigger_kind
        .unwrap_or_else(|| "manual".to_string());
    if !matches!(
        trigger_kind.as_str(),
        "manual" | "cron" | "event" | "schedule" | "webhook"
    ) {
        return Err(format!("invalid trigger_kind: {}", trigger_kind));
    }

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let name = input.name.trim().chars().take(200).collect::<String>();
    let base_slug = input
        .slug
        .as_deref()
        .map(slugify)
        .unwrap_or_else(|| slugify(&name));
    let slug = unique_slug(&conn, &base_slug)?;
    let graph_str = serde_json::to_string(&input.graph)
        .map_err(|e| format!("graph json: {}", e))?;
    let variables_str = match &input.variables {
        Some(v) => Some(serde_json::to_string(v).map_err(|e| format!("variables json: {}", e))?),
        None => None,
    };
    let trigger_config_str = match &input.trigger_config {
        Some(v) => Some(serde_json::to_string(v).map_err(|e| format!("trigger_config json: {}", e))?),
        None => None,
    };
    let source = input.source.unwrap_or_else(|| "manual".to_string());
    // Codex R4: honor an explicit `enabled=false` on create. Default
    // remains true so existing callers see no change.
    let enabled = input.enabled.unwrap_or(true);
    let enabled_int: i32 = if enabled { 1 } else { 0 };

    conn.execute(
        "INSERT INTO loops (
            id, slug, name, description, enabled, graph, variables,
            trigger_kind, trigger_config, source, source_ref,
            created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
        params![
            id,
            slug,
            name,
            input.description,
            enabled_int,
            graph_str,
            variables_str,
            trigger_kind,
            trigger_config_str,
            source,
            input.source_ref,
            now,
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(Loop {
        id,
        slug,
        name,
        description: input.description,
        enabled,
        graph: input.graph,
        variables: input.variables,
        trigger_kind,
        trigger_config: input.trigger_config,
        source,
        source_ref: input.source_ref,
        created_at: now.clone(),
        updated_at: now,
    })
}

#[tauri::command]
pub fn update_loop(
    db: State<'_, DbState>,
    id: String,
    input: LoopUpdateInput,
) -> Result<Loop, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();

    // Build the UPDATE dynamically so unset fields stay NULL-free. Each
    // optional field is a column we toggle into the SET list with a
    // numbered placeholder.
    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(name) = input.name {
        let trimmed: String = name.trim().chars().take(200).collect();
        if trimmed.is_empty() {
            return Err("name-empty".into());
        }
        sets.push(format!("name = ?{}", sets.len() + 1));
        binds.push(Box::new(trimmed));
    }
    if let Some(description) = input.description {
        sets.push(format!("description = ?{}", sets.len() + 1));
        binds.push(Box::new(description));
    }
    if let Some(enabled) = input.enabled {
        sets.push(format!("enabled = ?{}", sets.len() + 1));
        binds.push(Box::new(if enabled { 1i32 } else { 0i32 }));
    }
    if let Some(graph) = input.graph {
        let s = serde_json::to_string(&graph).map_err(|e| format!("graph json: {}", e))?;
        sets.push(format!("graph = ?{}", sets.len() + 1));
        binds.push(Box::new(s));
    }
    if let Some(variables) = input.variables {
        let s = serde_json::to_string(&variables)
            .map_err(|e| format!("variables json: {}", e))?;
        sets.push(format!("variables = ?{}", sets.len() + 1));
        binds.push(Box::new(s));
    }
    if let Some(trigger_kind) = input.trigger_kind {
        if !matches!(
            trigger_kind.as_str(),
            "manual" | "cron" | "event" | "schedule" | "webhook"
        ) {
            return Err(format!("invalid trigger_kind: {}", trigger_kind));
        }
        sets.push(format!("trigger_kind = ?{}", sets.len() + 1));
        binds.push(Box::new(trigger_kind));
    }
    if let Some(trigger_config) = input.trigger_config {
        let s = serde_json::to_string(&trigger_config)
            .map_err(|e| format!("trigger_config json: {}", e))?;
        sets.push(format!("trigger_config = ?{}", sets.len() + 1));
        binds.push(Box::new(s));
    }

    if sets.is_empty() {
        // No-op update — just return current state.
        drop(conn);
        return get_loop(db, id);
    }

    sets.push(format!("updated_at = ?{}", sets.len() + 1));
    binds.push(Box::new(now));
    let id_pos = sets.len() + 1;
    binds.push(Box::new(id.clone()));

    let sql = format!(
        "UPDATE loops SET {} WHERE {} = ?{}",
        sets.join(", "),
        id_or_slug_column(&id),
        id_pos
    );
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| &**b).collect();
    let affected = conn
        .execute(&sql, rusqlite::params_from_iter(bind_refs.iter()))
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err("loop not found".into());
    }

    drop(conn);
    get_loop(db, id)
}

#[tauri::command]
pub fn delete_loop(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    // ON DELETE CASCADE wipes loop_runs / loop_run_steps / loop_schedules.
    let sql = format!("DELETE FROM loops WHERE {} = ?1", id_or_slug_column(&id));
    let affected = conn
        .execute(&sql, params![id])
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err("loop not found".into());
    }
    Ok(())
}

// ── Loop runs (read-only here; writes happen in executor — Task #14) ────

#[tauri::command]
pub fn list_loop_runs(
    db: State<'_, DbState>,
    loop_id: String,
    limit: Option<i64>,
) -> Result<Vec<LoopRun>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let cap = limit.unwrap_or(50).clamp(1, 500);
    let sql = format!(
        "SELECT lr.id, lr.loop_id, lr.status, lr.started_at, lr.finished_at,
                lr.error, lr.triggered_by, lr.variables,
                lr.initiator_kind, lr.client_surface, lr.initiator_id
           FROM loop_runs lr
           JOIN loops l ON lr.loop_id = l.id
          WHERE l.{} = ?1
          ORDER BY lr.started_at DESC
          LIMIT ?2",
        id_or_slug_column(&loop_id),
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![loop_id, cap], |row| {
            let raw_vars: Option<String> = row.get(7)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                raw_vars,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        let (
            id,
            loop_id,
            status,
            started_at,
            finished_at,
            error,
            triggered_by,
            raw_vars,
            initiator_kind,
            client_surface,
            initiator_id,
        ) = r.map_err(|e| e.to_string())?;
        let variables = parse_json("variables", raw_vars)?;
        out.push(LoopRun {
            id,
            loop_id,
            status,
            started_at,
            finished_at,
            error,
            triggered_by,
            variables,
            initiator_kind,
            client_surface,
            initiator_id,
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn get_loop_run_steps(
    db: State<'_, DbState>,
    run_id: String,
) -> Result<Vec<LoopRunStep>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, loop_run_id, node_id, node_type, status,
                    started_at, finished_at, input, output, error,
                    execution_log_id
               FROM loop_run_steps
              WHERE loop_run_id = ?1
              ORDER BY started_at ASC NULLS LAST, id ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![run_id], |row| {
            let input_raw: Option<String> = row.get(7)?;
            let output_raw: Option<String> = row.get(8)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                input_raw,
                output_raw,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
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
        ) = r.map_err(|e| e.to_string())?;
        let input = parse_json("input", input_raw)?;
        let output = parse_json("output", output_raw)?;
        out.push(LoopRunStep {
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
    Ok(out)
}

// ── v2.14 step 3: run_loop_by_slug ──────────────────────────────────────
//
// Tauri command that the LoopComposer's Run button calls. Shells out to
// the prod ato CLI binary (`ato loop run <slug>`) — the CLI is the
// execution engine (v2.14 MVP wired in apps/cli/src/commands/loops.rs).
// The CLI writes the loop_runs + loop_run_steps rows; this command just
// returns the loop_run id so the desktop can poll get_loop_run_steps
// for status updates.

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopRunStarted {
    pub run_id: String,
    pub status: String,
}

#[tauri::command]
pub async fn run_loop_by_slug(slug_or_id: String) -> Result<LoopRunStarted, String> {
    // Resolve the ato binary — prefer the prod app's sibling path so
    // we hit the same keychain ACL identity the GUI uses for everything
    // else (the dev `cargo run` binary has a different signature and
    // gets refused by the master_key keychain item).
    let ato_path = resolve_ato_cli_path()?;

    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&ato_path)
            .args(["loop", "run", &slug_or_id])
            .env("ATO_CLIENT_SURFACE", "desktop")
            .env("ATO_INITIATOR_KIND", "human")
            .output()
    })
    .await
    .map_err(|e| format!("spawn join error: {e}"))?
    .map_err(|e| format!("Failed to spawn ato CLI: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!(
            "ato loop run exited with status {} — {}",
            output.status,
            stderr.trim()
        ));
    }

    // The CLI emits JSON on success: {"run_id": "...", "status": "...", ...}.
    let stdout = String::from_utf8_lossy(&output.stdout);
    #[derive(serde::Deserialize)]
    struct CliRunResult {
        run_id: String,
        status: String,
    }
    let parsed: CliRunResult = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("CLI returned non-JSON output: {e} — stdout was: {stdout}"))?;

    Ok(LoopRunStarted {
        run_id: parsed.run_id,
        status: parsed.status,
    })
}

/// Best-effort resolve the prod ato CLI binary. The desktop is always
/// installed alongside the CLI at /Applications/ATO.app/Contents/MacOS/ato
/// on macOS; on other platforms we fall back to the PATH-resolved `ato`.
fn resolve_ato_cli_path() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let prod = "/Applications/ATO.app/Contents/MacOS/ato";
        if std::path::Path::new(prod).exists() {
            return Ok(prod.to_string());
        }
    }
    // Fallback: PATH lookup — let the OS spawn handle resolution.
    Ok("ato".to_string())
}

// Tests for slug derivation + update partial-write semantics live in
// the CLI side (apps/cli/src/commands/loop.rs, Task #10) where the
// helpers can be tested against a bare Connection without the Tauri
// State<'_, DbState> boilerplate.
