// Self-contained persistence for passive observations. Mirrors the
// columns the desktop's `persist_execution_log` writes for active
// dispatches, minus the event-bus publish — passive rows are
// read-only echoes of other CLIs' work and must never trigger ATO
// recipes (a Claude Code failure in another terminal shouldn't fire
// the user's ATO notification rules).
//
// Why duplicate the INSERT shape rather than calling the desktop's
// helper? The desktop helper lives behind Tauri State and pulls in
// the event bus, BYOK lookups, and pricing. We need the observer to
// work from any process (CLI, future systemd unit, headless CI) with
// nothing in scope but a SQLite path. The cost is one SQL statement
// duplicated — paid in exchange for the watcher being usable outside
// the desktop process.

use std::path::Path;

use ato_pricing::{estimate_cost_usd, estimate_text_tokens};
use rusqlite::Connection;

use crate::sources::SourceKind;

/// Mirrors apps/desktop's default_model_for_runtime — keeps the cost
/// table populated for passive runs even when the upstream CLI didn't
/// expose the model name on the response line. Lives here (not in
/// ato-pricing) because the runtime list is CLI-runtime-specific, not
/// a pure pricing concern.
fn default_model_for_runtime(runtime: &str) -> Option<&'static str> {
    match runtime {
        "claude" => Some("claude-sonnet-4-6"),
        "codex" => Some("gpt-4.1"),
        "gemini" => Some("gemini-2.5-flash"),
        _ => None,
    }
}

const MAX_LOG_BYTES: usize = 64 * 1024;

/// Truncate to MAX_LOG_BYTES without panicking on multi-byte UTF-8
/// codepoints (per review HIGH-3). `&s[..MAX]` would split a curly
/// quote / emoji / non-Latin glyph mid-byte and panic the watcher
/// worker thread — fatal in the CLI case where no Tauri restart
/// surface exists. Walk backwards from the cap until we hit a char
/// boundary, then slice.
fn truncate_for_log(s: &str) -> String {
    if s.len() <= MAX_LOG_BYTES {
        return s.to_string();
    }
    let mut cut = MAX_LOG_BYTES;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…[truncated]", &s[..cut])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string_is_pass_through() {
        assert_eq!(truncate_for_log("hello"), "hello");
    }

    #[test]
    fn truncate_at_ascii_boundary() {
        let s = "a".repeat(MAX_LOG_BYTES + 100);
        let out = truncate_for_log(&s);
        assert!(out.starts_with(&"a".repeat(MAX_LOG_BYTES)));
        assert!(out.ends_with("…[truncated]"));
    }

    #[test]
    fn truncate_at_multibyte_boundary_no_panic() {
        // Build a payload where the byte at MAX_LOG_BYTES falls
        // INSIDE a 4-byte emoji (U+1F600). The naive `&s[..MAX]`
        // would panic; ours walks back to the prior boundary.
        let prefix = "a".repeat(MAX_LOG_BYTES - 2);
        let mut s = String::with_capacity(MAX_LOG_BYTES + 8);
        s.push_str(&prefix);
        s.push('\u{1F600}'); // 4 bytes
        s.push_str(&"a".repeat(8));
        assert!(s.len() > MAX_LOG_BYTES);
        let out = truncate_for_log(&s);
        // No panic. Truncated; ends with the marker. Prefix preserved.
        assert!(out.ends_with("…[truncated]"));
        assert!(out.starts_with(&prefix));
        // Output must itself be valid UTF-8 (we built it from a &str
        // slice — but assert the round-trip explicitly).
        assert_eq!(out, String::from_utf8(out.clone().into_bytes()).unwrap());
    }

    #[test]
    fn truncate_at_nbsp_boundary() {
        // U+00A0 (non-breaking space) is 2 bytes — the more common
        // mid-byte panic in real LLM output that includes typographic
        // whitespace.
        let prefix = "x".repeat(MAX_LOG_BYTES - 1);
        let mut s = prefix.clone();
        s.push('\u{00A0}');
        s.push_str("trailing");
        let out = truncate_for_log(&s);
        assert!(out.ends_with("…[truncated]"));
        assert!(out.starts_with(&prefix));
    }
}

/// Insert one observed (prompt, response) pair into execution_logs.
/// `INSERT OR IGNORE` on the partial UNIQUE index
/// `idx_execution_logs_session_seq(provider_session_id,
/// sequence_within_session)` makes this idempotent across re-scans.
#[allow(clippy::too_many_arguments)]
pub fn emit_row(
    db_path: &Path,
    kind: SourceKind,
    session_id: &str,
    sequence: i64,
    prompt: &str,
    response: &str,
    tokens_in: Option<i64>,
    tokens_out: Option<i64>,
    model: Option<&str>,
    started_at: Option<&str>,
) {
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let id = uuid::Uuid::new_v4().to_string();
    let now = started_at
        .map(|s| s.to_string())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let runtime = kind.runtime();
    let effective_model = model
        .filter(|s| !s.is_empty())
        .or_else(|| default_model_for_runtime(runtime));

    // Prefer real token counts from the upstream JSONL; fall back to
    // the 4-char heuristic so callers without `usage` blocks still
    // populate something useful.
    let tokens_in = tokens_in.or_else(|| Some(estimate_text_tokens(prompt)));
    let tokens_out = tokens_out.or_else(|| Some(estimate_text_tokens(response)));

    let cost_usd: Option<f64> = effective_model
        .and_then(|m| estimate_cost_usd(m, prompt, response));

    let billing_surface = kind.default_billing_surface();
    let dispatch_kind = "passive_observation";

    let _ = conn.execute(
        "INSERT OR IGNORE INTO execution_logs ( \
            id, runtime, prompt, response, tokens_in, tokens_out, \
            duration_ms, status, error_message, skill_name, \
            cloud_trace_id, created_at, cost_usd_estimated, agent_slug, \
            model, auth_mode, dispatch_kind, billing_surface, \
            provider_session_id, sequence_within_session \
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 'success', NULL, NULL, \
                   NULL, ?7, ?8, NULL, ?9, NULL, ?10, ?11, ?12, ?13)",
        rusqlite::params![
            id,
            runtime,
            truncate_for_log(prompt),
            truncate_for_log(response),
            tokens_in,
            tokens_out,
            now,
            cost_usd,
            effective_model,
            dispatch_kind,
            billing_surface,
            session_id,
            sequence,
        ],
    );

    // Clear the transient in-progress row now that the pair is closed.
    clear_passive_live_row(&conn, kind, session_id);
}

/// Insert a transient `live_runs` row when we see a user message in a
/// session we haven't already marked in-progress. The companion
/// `clear_passive_live_row` removes it once the assistant response
/// lands (or on next desktop boot via the existing `DELETE FROM
/// live_runs`).
///
/// run_id key shape `passive:<source-id>:<session-uuid>` keeps the row
/// unique across watcher restarts within the same desktop session
/// (re-emitting on file event is idempotent due to INSERT OR IGNORE)
/// and visually identifiable in the CLI debug surface.
pub fn mark_passive_in_progress(
    db_path: &Path,
    kind: SourceKind,
    session_id: &str,
    started_at: Option<&str>,
    cwd: Option<&str>,
) {
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let run_id = format!("passive:{}:{}", kind.id(), session_id);
    let started = started_at
        .map(|s| s.to_string())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let _ = conn.execute(
        "INSERT OR IGNORE INTO live_runs \
            (run_id, agent_slug, runtime, workspace, source, started_at, \
             status, child_pid, dispatch_kind, billing_surface) \
         VALUES (?1, NULL, ?2, ?3, ?4, ?5, 'running', NULL, \
                 'passive_observation', ?6)",
        rusqlite::params![
            run_id,
            kind.runtime(),
            cwd,
            format!("observed:{}", kind.id()),
            started,
            kind.default_billing_surface(),
        ],
    );
}

fn clear_passive_live_row(conn: &Connection, kind: SourceKind, session_id: &str) {
    let run_id = format!("passive:{}:{}", kind.id(), session_id);
    let _ = conn.execute("DELETE FROM live_runs WHERE run_id = ?1", [&run_id]);
}

/// Retroactively attach token counts to the most-recent passive
/// observation for a given session (per review MEDIUM-5). Codex
/// emits its `token_count` event AFTER the assistant message it
/// applies to lands, so the row goes in with NULL tokens; this
/// helper closes the loop when the count arrives.
///
/// Only updates the latest row whose tokens_* are still NULL — a
/// retransmit of the same token_count is therefore idempotent and a
/// hypothetical future Codex ordering change (token_count first,
/// message second) wouldn't double-write.
pub fn update_tokens_for_latest(
    db_path: &std::path::Path,
    kind: SourceKind,
    session_id: &str,
    tokens_in: Option<i64>,
    tokens_out: Option<i64>,
) {
    if tokens_in.is_none() && tokens_out.is_none() {
        return;
    }
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let _ = conn.execute(
        "UPDATE execution_logs \
            SET tokens_in  = COALESCE(tokens_in,  ?1), \
                tokens_out = COALESCE(tokens_out, ?2) \
          WHERE id = ( \
            SELECT id FROM execution_logs \
             WHERE dispatch_kind = 'passive_observation' \
               AND runtime = ?3 \
               AND provider_session_id = ?4 \
             ORDER BY sequence_within_session DESC \
             LIMIT 1)",
        rusqlite::params![tokens_in, tokens_out, kind.runtime(), session_id],
    );
}
