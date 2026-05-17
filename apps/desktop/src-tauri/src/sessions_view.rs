// v2.3.42 — Sessions view for the GUI.
//
// Phase 6 sessions (Slice A + A.2 + B) are CLI-first today: they live
// in the `sessions` + `session_turns` tables and the CLI exposes
// `ato sessions ...` and `ato bridge`. The desktop GUI never had a
// first-class surface for them — they only appeared incidentally
// under Execution Logs after v2.3.41's grouping.
//
// This module adds two Tauri commands:
//   - list_sessions_full   — overview rows for the Sessions tab list
//   - get_session_transcript — every turn for one session, ordered
//
// Both are read-only. Continuing a session from the GUI uses the
// existing prompt_agent flow with an extra session_id parameter
// (wired separately so this module stays narrowly scoped to the
// view layer).

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

use crate::DbState;

/// v2.6 Slice C — tracks the PIDs of in-flight `ato sessions close`
/// subprocesses so the frontend's Cancel button can interrupt them.
/// Keyed by session_id because a user can only close one session at
/// a time per session (a second close on the same session is refused
/// by the CLI's idempotency guard anyway). The Child is dropped after
/// wait_with_output returns; the PID entry is removed in the same
/// scope. SIGTERM lets the subprocess unwind cleanly — reqwest
/// drops the in-flight HTTP request, the UPDATE never runs, and the
/// session stays 'open'.
pub struct CloseInflight(pub Mutex<HashMap<String, u32>>);

impl CloseInflight {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionListRow {
    pub id: String,
    pub runtime: String,
    pub agent_slug: Option<String>,
    pub title: Option<String>,
    pub created_at: String,
    pub last_used_at: String,
    pub turn_count: i64,
    /// Distinct runtimes that appear in this session's turns. For a
    /// claude-only session this is `["claude"]`. For a Slice B
    /// cross-runtime conversation it's e.g. `["claude", "minimax"]`.
    /// Drives the runtime badges in the list UI.
    pub runtimes_used: Vec<String>,
    /// 2026-05-16 — distinct agent slugs that appear on the assistant
    /// turns of this session. Empty when every turn was a generalist
    /// dispatch (no `--agent` flag). For a war-room session it's e.g.
    /// `["positioning", "devex", "ceo", "designer", "office-hours"]`.
    /// Drives the persona-badge cluster on the SessionsList card.
    pub agents_used: Vec<String>,
    /// 2026-05-16 — session-total cost in USD, summed across every
    /// successful execution_logs row tied to this session_id. Renders
    /// as a small "$0.0644" pill on the card next to the turn count so
    /// users can scan cost per session without opening it. NULL when
    /// no execution_logs rows reference the session (older sessions
    /// pre-session_id-on-execution-logs).
    pub total_cost_usd: Option<f64>,
    /// Last (assistant) turn's text, truncated. Gives the user a
    /// "what was this conversation about" preview without expanding.
    pub last_assistant_preview: Option<String>,
    // v2.6 Slice C — lifecycle + coordinator-generated metadata.
    // `status` is "open" or "closed". `auto_title` is preferred over
    // the user-supplied `title` in the list when present (it's the
    // coordinator's distilled label). `summary`, `tags`, and
    // `project_id` are populated on close and refreshed on each
    // subsequent close after a reopen.
    pub status: String,
    pub closed_at: Option<String>,
    pub auto_title: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub project_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionTurn {
    pub turn_index: i64,
    pub role: String,
    pub text: String,
    pub runtime: String,
    pub created_at: String,
    /// 2026-05-16 — agent slug captured when the dispatching turn was
    /// fired with `--agent <slug>`. NULL means a generalist dispatch
    /// (raw model priors, no persona overlay). Drives the persona role
    /// label in the chat-bubble UI.
    pub agent_slug: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionTranscript {
    pub id: String,
    pub runtime: String,
    pub agent_slug: Option<String>,
    pub title: Option<String>,
    pub turns: Vec<SessionTurn>,
    // v2.6 Slice C — coordinator metadata, same fields as the list row.
    pub status: String,
    pub closed_at: Option<String>,
    pub auto_title: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub project_id: Option<String>,
}

#[tauri::command]
pub fn list_sessions_full(
    db: State<'_, DbState>,
    limit: Option<i64>,
) -> Result<Vec<SessionListRow>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_sessions_inner(&conn, limit.unwrap_or(50)).map_err(|e| e.to_string())
}

fn list_sessions_inner(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<SessionListRow>> {
    // SELECT the v2.6 lifecycle columns alongside the originals.
    // COALESCE wraps status because the v2.6 migration sets a default
    // of 'open' but pre-migration rows on a partially-upgraded install
    // could still surface NULL on read (defensive — the ALTER carries
    // the default forward, but the cost of being safe is zero).
    let mut stmt = conn.prepare(
        "SELECT s.id, s.runtime, s.agent_slug, s.title, s.created_at, s.last_used_at, s.turn_count,
                COALESCE(s.status, 'open'), s.closed_at, s.auto_title, s.summary, s.tags_json, s.project_id
           FROM sessions s
          ORDER BY s.last_used_at DESC
          LIMIT ?1",
    )?;
    let rows: Vec<SessionListRow> = stmt
        .query_map([limit], |r| {
            let tags_json: Option<String> = r.get(11)?;
            let tags: Vec<String> = tags_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            Ok(SessionListRow {
                id: r.get(0)?,
                runtime: r.get(1)?,
                agent_slug: r.get(2)?,
                title: r.get(3)?,
                created_at: r.get(4)?,
                last_used_at: r.get(5)?,
                turn_count: r.get(6)?,
                runtimes_used: Vec::new(),
                agents_used: Vec::new(),
                total_cost_usd: None,
                last_assistant_preview: None,
                status: r.get(7)?,
                closed_at: r.get(8)?,
                auto_title: r.get(9)?,
                summary: r.get(10)?,
                tags,
                project_id: r.get(12)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Enrich each row with computed fields. Two cheap follow-up queries
    // per session — fine for the default limit of 50, and the indexes
    // on session_turns(session_id, turn_index ASC) make them O(log N).
    let mut enriched = Vec::with_capacity(rows.len());
    for mut row in rows {
        // Distinct runtimes in this session's turns. For Slice B
        // cross-runtime sessions this is what makes the multi-badge UI
        // render correctly.
        let mut rt_stmt = conn.prepare_cached(
            "SELECT DISTINCT runtime FROM session_turns WHERE session_id = ?1 ORDER BY turn_index ASC",
        )?;
        let runtimes: Vec<String> = rt_stmt
            .query_map([&row.id], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        // Fall back to the session's anchor runtime when session_turns
        // is empty (e.g. a freshly opened session before its first
        // dispatch lands a turn).
        row.runtimes_used = if runtimes.is_empty() {
            vec![row.runtime.clone()]
        } else {
            runtimes
        };

        // 2026-05-16 — distinct agent slugs on assistant turns. Order
        // by first appearance (MIN(turn_index)) so the badge cluster
        // matches the order seats spoke in. Generalist turns (NULL
        // agent_slug) are excluded — they show up via the runtime
        // badges alone.
        let mut ag_stmt = conn.prepare_cached(
            "SELECT agent_slug FROM session_turns
              WHERE session_id = ?1 AND role = 'assistant' AND agent_slug IS NOT NULL
              GROUP BY agent_slug
              ORDER BY MIN(turn_index) ASC",
        )?;
        let agents: Vec<String> = ag_stmt
            .query_map([&row.id], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        row.agents_used = agents;

        // 2026-05-16 — session-total cost from execution_logs. NULL out
        // (rather than 0.0) when there are no rows so the UI knows the
        // session pre-dates session-id-on-execution-logs and can hide
        // the pill instead of rendering a misleading "$0.00".
        let mut cost_stmt = conn.prepare_cached(
            "SELECT SUM(COALESCE(cost_usd_estimated, 0)), COUNT(*)
               FROM execution_logs
              WHERE session_id = ?1",
        )?;
        let (sum_cost, n): (Option<f64>, i64) = cost_stmt
            .query_row([&row.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap_or((None, 0));
        row.total_cost_usd = if n > 0 { sum_cost.or(Some(0.0)) } else { None };

        // Last assistant turn → preview. Order by turn_index DESC so we
        // get the chronologically last assistant message, not whichever
        // arrived first.
        let mut last_stmt = conn.prepare_cached(
            "SELECT text FROM session_turns
              WHERE session_id = ?1 AND role = 'assistant'
              ORDER BY turn_index DESC
              LIMIT 1",
        )?;
        let preview: Option<String> = last_stmt
            .query_row([&row.id], |r| r.get::<_, String>(0))
            .ok();
        row.last_assistant_preview = preview.map(|s| {
            // Trim to 160 chars max so list rows stay one-line on most
            // viewports. The full text is available in the transcript.
            if s.chars().count() > 160 {
                let truncated: String = s.chars().take(160).collect();
                format!("{}…", truncated)
            } else {
                s
            }
        });

        enriched.push(row);
    }
    Ok(enriched)
}

// v2.6 Slice C — content search across turn text. The metadata search
// in the React component handles title/summary/tag/runtime matching
// client-side; this Tauri command extends it to "find sessions that
// contain these words anywhere in the conversation." Returns the set
// of session ids whose turns contain ALL the query tokens (each
// token can match any turn — they don't have to be in the same turn,
// since multi-turn conversations split topics across messages).
//
// Implementation is plain LIKE rather than FTS5 because (a) the
// turn-text table is bounded by a single user's local sessions —
// not a corpus — and (b) keeping it LIKE means no migration cost
// and no FTS5 index drift to worry about. If a user reports it
// being slow we can swap in FTS5 transparently.
#[tauri::command]
pub fn search_session_turns(
    db: State<'_, DbState>,
    query: String,
) -> Result<Vec<String>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    // Tokenize on whitespace and require every token to appear in
    // SOME turn of the session. Cap tokens to 8 to bound the query
    // size and reject empty strings post-trim.
    let tokens: Vec<String> = trimmed
        .split_whitespace()
        .take(8)
        .map(|s| s.to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // For each token, find the set of sessions whose turns include it.
    // Intersect across tokens to get sessions containing all of them.
    let mut result_set: Option<std::collections::HashSet<String>> = None;
    for token in &tokens {
        let like_pattern = format!("%{}%", token);
        let mut stmt = conn
            .prepare_cached(
                "SELECT DISTINCT session_id FROM session_turns WHERE LOWER(text) LIKE ?1",
            )
            .map_err(|e| e.to_string())?;
        let ids: std::collections::HashSet<String> = stmt
            .query_map([&like_pattern], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        result_set = Some(match result_set {
            Some(existing) => existing.intersection(&ids).cloned().collect(),
            None => ids,
        });
        // Short-circuit once the intersection is empty.
        if result_set.as_ref().map(|s| s.is_empty()).unwrap_or(false) {
            return Ok(Vec::new());
        }
    }
    Ok(result_set.map(|s| s.into_iter().collect()).unwrap_or_default())
}

// 2026-05-16 — cost receipts panel.
//
// The Loom shot-list's most compelling moment is the cost-comparison
// table that shows "the cheapest model caught the bug." That data lives
// in execution_logs.cost_usd_estimated + tokens_in/out + duration_ms,
// joined to the session by session_id. This command exposes the per-
// (runtime, agent_slug) breakdown for a single session so the chat
// detail can render a receipts panel at the bottom.
//
// Rows include both successful AND error turns (errors still cost
// tokens at the provider) — `successful_turns` lets the UI distinguish.
// Generalist turns surface as agent_slug = None; the UI renders these
// as "<generalist>" so the row reads cleanly.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionCostRow {
    pub runtime: String,
    pub agent_slug: Option<String>,
    pub total_turns: i64,
    pub successful_turns: i64,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub total_duration_ms: Option<i64>,
    /// 2026-05-16 — `cost_null_turns` counts rows where the dispatch
    /// computed a NULL cost (model missing from pricing table). The UI
    /// surfaces these as "$? (model not in pricing table)" so a stale
    /// pricing table doesn't masquerade as a free dispatch.
    pub cost_null_turns: i64,
    pub total_cost_usd: f64,
    /// "subscription" / "api_key" / "local" — read from
    /// `execution_logs.auth_mode` when populated (authoritative; per-
    /// row truth from the dispatch). Falls back to a static lookup on
    /// the runtime name for pre-auth-mode rows.
    pub billing_mode: String,
}

/// Fallback for older rows where `execution_logs.auth_mode` is NULL.
/// Delegates to the shared `ato_pricing::billing_mode` so the CLI and
/// desktop classify runtimes identically.
fn billing_mode_fallback(runtime: &str) -> &'static str {
    ato_pricing::billing_mode(runtime).as_str()
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionCostBreakdown {
    pub session_id: String,
    pub total_cost_usd: f64,
    pub total_turns: i64,
    pub total_tokens_in: i64,
    pub total_tokens_out: i64,
    pub total_duration_ms: i64,
    pub rows: Vec<SessionCostRow>,
}

#[tauri::command]
pub fn get_session_cost_breakdown(
    db: State<'_, DbState>,
    session_id: String,
) -> Result<SessionCostBreakdown, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT runtime,
                    agent_slug,
                    auth_mode,
                    COUNT(*) AS total_turns,
                    SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) AS successful_turns,
                    SUM(COALESCE(tokens_in, 0))  AS tokens_in,
                    SUM(COALESCE(tokens_out, 0)) AS tokens_out,
                    SUM(COALESCE(duration_ms, 0)) AS total_duration_ms,
                    SUM(CASE WHEN cost_usd_estimated IS NULL AND status = 'success' THEN 1 ELSE 0 END) AS cost_null_turns,
                    SUM(COALESCE(cost_usd_estimated, 0)) AS total_cost_usd
               FROM execution_logs
              WHERE session_id = ?1
              GROUP BY runtime, agent_slug, auth_mode
              ORDER BY total_cost_usd DESC, runtime ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows: Vec<SessionCostRow> = stmt
        .query_map([&session_id], |r| {
            let runtime: String = r.get(0)?;
            let auth_mode: Option<String> = r.get(2)?;
            let billing_mode = auth_mode
                .clone()
                .unwrap_or_else(|| billing_mode_fallback(&runtime).to_string());
            Ok(SessionCostRow {
                runtime,
                agent_slug: r.get(1)?,
                total_turns: r.get(3)?,
                successful_turns: r.get(4)?,
                tokens_in: r.get::<_, Option<i64>>(5)?,
                tokens_out: r.get::<_, Option<i64>>(6)?,
                total_duration_ms: r.get::<_, Option<i64>>(7)?,
                cost_null_turns: r.get(8)?,
                total_cost_usd: r.get(9)?,
                billing_mode,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let total_cost_usd: f64 = rows.iter().map(|r| r.total_cost_usd).sum();
    let total_turns: i64 = rows.iter().map(|r| r.total_turns).sum();
    let total_tokens_in: i64 = rows.iter().map(|r| r.tokens_in.unwrap_or(0)).sum();
    let total_tokens_out: i64 = rows.iter().map(|r| r.tokens_out.unwrap_or(0)).sum();
    let total_duration_ms: i64 = rows.iter().map(|r| r.total_duration_ms.unwrap_or(0)).sum();

    Ok(SessionCostBreakdown {
        session_id,
        total_cost_usd,
        total_turns,
        total_tokens_in,
        total_tokens_out,
        total_duration_ms,
        rows,
    })
}

#[tauri::command]
pub fn get_session_transcript(
    db: State<'_, DbState>,
    session_id: String,
) -> Result<SessionTranscript, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    type Header = (
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let (
        runtime,
        agent_slug,
        title,
        status,
        closed_at,
        auto_title,
        summary,
        tags_json,
        project_id,
    ): Header = conn
        .query_row(
            "SELECT runtime, agent_slug, title,
                    COALESCE(status, 'open'), closed_at, auto_title, summary, tags_json, project_id
               FROM sessions WHERE id = ?1",
            [&session_id],
            |r| Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            )),
        )
        .map_err(|e| format!("session not found: {}", e))?;
    let tags: Vec<String> = tags_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    let mut stmt = conn
        .prepare(
            "SELECT turn_index, role, text, runtime, created_at, agent_slug
               FROM session_turns
              WHERE session_id = ?1
              ORDER BY turn_index ASC",
        )
        .map_err(|e| e.to_string())?;

    let turns: Vec<SessionTurn> = stmt
        .query_map([&session_id], |r| {
            Ok(SessionTurn {
                turn_index: r.get(0)?,
                role: r.get(1)?,
                text: r.get(2)?,
                runtime: r.get(3)?,
                created_at: r.get(4)?,
                agent_slug: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(SessionTranscript {
        id: session_id,
        runtime,
        agent_slug,
        title,
        turns,
        status,
        closed_at,
        auto_title,
        summary,
        tags,
        project_id,
    })
}

// ───────────────────────────────────────────────────────────────────────
// v2.3.43 — Tauri commands for the New / Continue / Bridge buttons.
//
// Each shells out to the `ato` CLI binary, which is the canonical
// implementation of sessions / dispatch / bridge. The desktop's own
// prompt_agent path doesn't yet support --session natively (a deeper
// change); going through the CLI keeps these slices independent and
// the behavior provably identical to what an agent invoking
// `ato dispatch ... --session` would do.

#[derive(Debug, Deserialize)]
struct CliSessionNewOutput {
    id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchIntoSessionResult {
    pub run_id: String,
    pub status: String,
    pub response: Option<String>,
    pub error_message: Option<String>,
    pub duration_ms: Option<i64>,
}

fn resolve_ato_binary() -> Result<String, String> {
    // Prefer the bundled installation paths, then fall through to the
    // same PATH resolution other Tauri commands use. Falls back to bare
    // "ato" so the user's shell can locate it if installed elsewhere.
    if let Some(p) = crate::commands::which_cli("ato") {
        return Ok(p);
    }
    // Last resort: bare command name; Command::new will surface a clean
    // exec error if PATH doesn't include it.
    Ok("ato".to_string())
}

#[tauri::command]
pub fn create_session(
    runtime: String,
    title: Option<String>,
    agent_slug: Option<String>,
) -> Result<String, String> {
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args(["sessions", "new", "--runtime", &runtime]);
    if let Some(t) = &title {
        cmd.args(["--title", t]);
    }
    if let Some(slug) = &agent_slug {
        cmd.args(["--as", slug]);
    }
    let out = cmd.output().map_err(|e| format!("spawn ato: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(format!("ato sessions new failed: {}", stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: CliSessionNewOutput = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("parse ato output: {} (raw: {})", e, stdout))?;
    Ok(parsed.id)
}

#[tauri::command]
pub fn dispatch_into_session(
    runtime: String,
    prompt: String,
    session_id: String,
    model: Option<String>,
) -> Result<DispatchIntoSessionResult, String> {
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args(["dispatch", &runtime, &prompt, "--session", &session_id]);
    if let Some(m) = &model {
        cmd.args(["--model", m]);
    }
    let out = cmd.output().map_err(|e| format!("spawn ato: {}", e))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    // The CLI exits non-zero only on a pre-flight error (quota / unknown
    // runtime). Real per-dispatch errors come back as a JSON payload
    // with status="error", so we still need to parse stdout when present.
    let raw = if stdout.trim().is_empty() {
        // No stdout — fall back to surfacing stderr to the user.
        return Err(format!(
            "ato dispatch produced no JSON output: {}",
            stderr.trim()
        ));
    } else {
        stdout
    };
    let v: serde_json::Value =
        serde_json::from_str(raw.trim()).map_err(|e| format!("parse ato output: {}", e))?;
    Ok(DispatchIntoSessionResult {
        run_id: v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        status: v
            .get("status")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string(),
        response: v.get("response").and_then(|x| x.as_str()).map(String::from),
        error_message: v
            .get("error_message")
            .and_then(|x| x.as_str())
            .map(String::from),
        duration_ms: v.get("duration_ms").and_then(|x| x.as_i64()),
    })
}

#[tauri::command]
pub fn bridge_session(
    session_id: String,
    max_rounds: Option<u32>,
) -> Result<String, String> {
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args(["bridge", "--session", &session_id, "--human"]);
    if let Some(n) = max_rounds {
        cmd.args(["--max-rounds", &n.to_string()]);
    }
    let out = cmd.output().map_err(|e| format!("spawn ato: {}", e))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        return Err(format!(
            "ato bridge failed (status {}): {}",
            out.status,
            stderr.trim()
        ));
    }
    // The bridge writes its progress as human-readable lines to stdout;
    // return the whole transcript so the UI can show it in a "bridge
    // result" panel.
    Ok(stdout.trim().to_string())
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ChunkEventPayload {
    session_id: String,
    text: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DoneEventPayload {
    session_id: String,
    result: serde_json::Value,
}

/// v2.3.48 — streaming dispatch into a session. Spawns the CLI with
/// `--stream-jsonl`, reads each line of stdout as a JSON event, and
/// emits Tauri events for the frontend to render:
///   - `session-stream-chunk` { sessionId, text } per chunk
///   - `session-stream-done`  { sessionId, result } at the end
/// Returns the final DispatchResult once the stream completes so the
/// caller can await it like a regular Tauri command. Errors propagate
/// as Tauri-command errors with stderr context.
#[tauri::command]
pub fn dispatch_into_session_streaming(
    app: AppHandle,
    runtime: String,
    prompt: String,
    session_id: String,
    model: Option<String>,
) -> Result<serde_json::Value, String> {
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args([
        "dispatch",
        &runtime,
        &prompt,
        "--session",
        &session_id,
        "--stream-jsonl",
    ]);
    if let Some(m) = &model {
        cmd.args(["--model", m]);
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn ato dispatch --stream-jsonl: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "missing stdout pipe".to_string())?;
    let reader = BufReader::new(stdout);

    let mut final_result: Option<serde_json::Value> = None;
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                return Err(format!("read CLI stdout: {}", e));
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue, // skip non-JSON lines defensively
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("chunk") => {
                let text = v
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let _ = app.emit(
                    "session-stream-chunk",
                    ChunkEventPayload {
                        session_id: session_id.clone(),
                        text,
                    },
                );
            }
            Some("done") => {
                let result = v.get("result").cloned().unwrap_or(serde_json::Value::Null);
                let _ = app.emit(
                    "session-stream-done",
                    DoneEventPayload {
                        session_id: session_id.clone(),
                        result: result.clone(),
                    },
                );
                final_result = Some(result);
            }
            _ => {}
        }
    }

    // Reap the child to surface any non-zero exit + stderr.
    let exit_status = child
        .wait()
        .map_err(|e| format!("wait CLI exit: {}", e))?;
    if !exit_status.success() {
        let mut stderr_buf = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            use std::io::Read;
            let _ = stderr.read_to_string(&mut stderr_buf);
        }
        return Err(format!(
            "ato dispatch exited with {}: {}",
            exit_status,
            stderr_buf.trim()
        ));
    }

    final_result.ok_or_else(|| "stream finished without a `done` event".to_string())
}

// ───────────────────────────────────────────────────────────────────────
// v2.6 Slice C — close / reopen lifecycle.
//
// Both commands shell out to the CLI so the canonical logic stays in
// one place (and `ato sessions close <id>` works identically from a
// terminal). The desktop frontend awaits the close call — it's
// expected to block for a few seconds while the coordinator LLM
// produces a title + summary + tags. The frontend renders a
// "Coordinator is summarizing…" modal during the wait.

/// Reject values that would be interpreted by clap as a flag rather
/// than a positional/flag value. Without this, an attacker-controlled
/// session_id like `--model evil` would be parsed as two flags and
/// could redirect the summarizer to an arbitrary model. We pair this
/// with `--` after the subcommand name as a defense in depth so even
/// values that contain odd characters can't break the arg parser.
fn validate_session_id(v: &str) -> Result<(), String> {
    if v.is_empty() {
        return Err("session_id is empty".to_string());
    }
    // Session IDs are UUIDs in this codebase (create_inner uses
    // Uuid::new_v4). Accept anything matching 8-4-4-4-12 hex; reject
    // anything else with a clear error rather than letting it through.
    let bytes = v.as_bytes();
    if bytes.len() != 36 {
        return Err(format!("session_id is not a UUID: {}", v));
    }
    for (i, b) in bytes.iter().enumerate() {
        let expect_dash = matches!(i, 8 | 13 | 18 | 23);
        if expect_dash {
            if *b != b'-' {
                return Err(format!("session_id is not a UUID: {}", v));
            }
        } else if !b.is_ascii_hexdigit() {
            return Err(format!("session_id is not a UUID: {}", v));
        }
    }
    Ok(())
}

/// Agent slugs are user-provided but constrained to a kebab/snake set
/// across the rest of this codebase. Reject anything else before we
/// pass it as a flag value.
fn validate_agent_slug(v: &str) -> Result<(), String> {
    if v.is_empty() || v.len() > 64 {
        return Err(format!("agent slug length out of range: {}", v.len()));
    }
    for c in v.chars() {
        if !(c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(format!("agent slug contains invalid characters: {}", v));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn close_session(
    inflight: State<'_, CloseInflight>,
    session_id: String,
    agent_slug: Option<String>,
    model: Option<String>,
) -> Result<serde_json::Value, String> {
    validate_session_id(&session_id)?;
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args(["sessions", "close"]);
    // Flag-bearing options go BEFORE `--` so clap parses them as flags.
    // After `--`, clap treats the next tokens as positional values
    // regardless of leading dashes — defense-in-depth against a stray
    // `--foo` session_id getting parsed as a flag if validate_session_id
    // is ever weakened.
    if let Some(slug) = agent_slug.as_deref() {
        if !slug.is_empty() {
            validate_agent_slug(slug)?;
            cmd.args(["--as", slug]);
        }
    }
    if let Some(m) = model.as_deref() {
        if !m.is_empty() {
            cmd.args(["--model", m]);
        }
    }
    cmd.arg("--");
    cmd.arg(&session_id);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Spawn → register PID → wait. The PID registration enables the
    // Cancel button in the UI to send SIGTERM via cancel_close_session.
    // We MUST remove the PID from the map in both the success and
    // error paths so a subsequent close isn't blocked by a stale entry.
    let child = cmd.spawn().map_err(|e| format!("spawn ato: {}", e))?;
    let pid = child.id();
    {
        let mut map = inflight.0.lock().map_err(|e| e.to_string())?;
        map.insert(session_id.clone(), pid);
    }
    let result = child
        .wait_with_output()
        .map_err(|e| format!("wait ato: {}", e));
    // Always deregister, even on wait_with_output error.
    {
        let mut map = inflight.0.lock().map_err(|e| e.to_string())?;
        map.remove(&session_id);
    }
    let out = result?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        // SIGTERM produces a non-zero exit. Surface a distinguishable
        // error so the frontend can recognize the cancel case and not
        // confuse it with a real failure.
        if let Some(code) = out.status.code() {
            if code != 0 && stderr.trim().is_empty() && stdout.trim().is_empty() {
                return Err("__cancelled__".to_string());
            }
        }
        return Err(format!(
            "ato sessions close failed (status {}): {}",
            out.status,
            stderr.trim()
        ));
    }
    // Refuse to embed raw stdout in the error — it can contain
    // truncated LLM output from a failed close, which may include
    // transcript content (potentially pasted secrets).
    serde_json::from_str(stdout.trim())
        .map_err(|_| "ato sessions close returned unparseable JSON".to_string())
}

/// Send SIGTERM to an in-flight `ato sessions close` subprocess so
/// the user's Cancel click in the blocking modal aborts the LLM call.
/// No-op when no close is in flight for this session (e.g., the user
/// double-clicked Cancel and the first click already worked).
#[tauri::command]
pub fn cancel_close_session(
    inflight: State<'_, CloseInflight>,
    session_id: String,
) -> Result<bool, String> {
    validate_session_id(&session_id)?;
    let pid = {
        let map = inflight.0.lock().map_err(|e| e.to_string())?;
        map.get(&session_id).copied()
    };
    let Some(pid) = pid else {
        return Ok(false);
    };
    // Shell to `kill -TERM <pid>` instead of pulling in libc/nix as a
    // new dep. macOS + Linux both ship `kill` in /bin; the desktop
    // app targets these platforms (Windows support is roadmap-only).
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .map_err(|e| format!("spawn kill: {}", e))?;
    Ok(status.success())
}

#[tauri::command]
pub fn reopen_session(session_id: String) -> Result<serde_json::Value, String> {
    validate_session_id(&session_id)?;
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args(["sessions", "reopen", "--", &session_id]);
    let out = cmd.output().map_err(|e| format!("spawn ato: {}", e))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        return Err(format!(
            "ato sessions reopen failed (status {}): {}",
            out.status,
            stderr.trim()
        ));
    }
    serde_json::from_str(stdout.trim())
        .map_err(|_| "ato sessions reopen returned unparseable JSON".to_string())
}
