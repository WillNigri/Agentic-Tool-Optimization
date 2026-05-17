// ato-db-views — SQLite VIEW definitions over the audit-trail tables.
//
// Common joins (session_turns ↔ execution_logs, per-session cost
// summary, per-(agent,runtime) rollup) used to be re-implemented at
// every call site — both in Rust query builders AND in ad-hoc
// `sqlite3` invocations. These views centralize the joins so:
//
//   - UI/Tauri commands SELECT FROM v_* and get the join for free.
//   - Power users running `sqlite3 ~/.ato/local.db` get the same view.
//   - Future schema changes (e.g., new columns on execution_logs) only
//     need to update the view definition here.
//
// All views use `CREATE VIEW IF NOT EXISTS` so the apply step is
// idempotent on every desktop startup + CLI open_readwrite call.
//
// Maintenance: when a view's underlying table gains/loses a column,
// drop the affected view via the desktop's startup migrations BEFORE
// the CREATE IF NOT EXISTS runs (otherwise the old definition sticks).
// Add a one-line `DROP VIEW IF EXISTS v_foo;` to the migrations list
// in the same commit that changes the schema.

/// Every view in canonical apply order. Order matters only if a view
/// references another view; none do today, so order is alphabetical
/// for readability.
pub const ALL_VIEWS: &[&str] = &[
    V_SESSION_AUDIT,
    V_RECENT_DISPATCHES,
    V_SESSION_COST_SUMMARY,
    V_COST_BY_AGENT_RUNTIME,
    V_ORPHANED_SESSION_TURNS,
    V_ORPHANED_EXECUTION_LOGS,
];

/// Per-turn view joining `session_turns` to its matching `execution_logs`
/// row by `(session_id, runtime, ±5s window)`.
///
/// **Cardinality guarantee:** the correlated subquery uses `LIMIT 1` so
/// the JOIN produces at most one execution_log per turn. No Cartesian
/// explosion if two dispatches happen to share the (session_id, runtime,
/// ±5s) bucket (codex-reviewer 2026-05-17 originally flagged this as the
/// real risk; the LIMIT 1 alone closes it).
///
/// **Why no inner ORDER BY:** SQLite doesn't resolve correlated outer-
/// scope column references inside a subquery's ORDER BY clause (parse
/// error "no such column: st.created_at"). The earlier draft had
/// `ORDER BY ABS(Δt) ASC LIMIT 1` to pick the CLOSEST match; that
/// failed parse. In practice the dispatch path writes one execution_logs
/// row per dispatch within ~100ms of the matching session_turns rows,
/// so the (session_id, runtime, ±5s) WHERE clause already constrains to
/// one row in normal operation. If two ever exist in the bucket, LIMIT 1
/// picks deterministically by sqlite_rowid (insertion order). Good enough.
///
/// The correct long-term fix is a real `session_turns.execution_log_id`
/// FK populated by the dispatch path. Tracked in roadmap; adding that
/// column reduces this view's JOIN to `el.id = st.execution_log_id`.
pub const V_SESSION_AUDIT: &str = "
CREATE VIEW IF NOT EXISTS v_session_audit AS
SELECT
  st.session_id,
  st.turn_index,
  st.role,
  st.runtime,
  st.agent_slug,
  st.created_at,
  substr(st.text, 1, 200) AS text_preview,
  el.id                AS execution_log_id,
  el.duration_ms,
  el.cost_usd_estimated,
  el.tokens_in,
  el.tokens_out,
  el.model,
  el.auth_mode,
  el.status            AS dispatch_status
FROM session_turns st
LEFT JOIN execution_logs el ON el.id = (
  SELECT el2.id
    FROM execution_logs el2
   WHERE el2.session_id = st.session_id
     AND el2.runtime    = st.runtime
     AND ABS(strftime('%s', el2.created_at) - strftime('%s', st.created_at)) < 5
   LIMIT 1
);
";

/// Slice of `execution_logs` with prompt/response previews pre-truncated.
///
/// **No baked-in ORDER BY (codex-reviewer 2026-05-17):** views shouldn't
/// pre-order — callers usually want their own ORDER BY + LIMIT and a
/// pre-ordered view forces an unnecessary sort if the caller wants a
/// different order. Add `ORDER BY created_at DESC` at the call site.
pub const V_RECENT_DISPATCHES: &str = "
CREATE VIEW IF NOT EXISTS v_recent_dispatches AS
SELECT
  id,
  runtime,
  agent_slug,
  model,
  status,
  auth_mode,
  duration_ms,
  tokens_in,
  tokens_out,
  cost_usd_estimated,
  session_id,
  created_at,
  substr(prompt, 1, 200)   AS prompt_preview,
  substr(response, 1, 200) AS response_preview
FROM execution_logs;
";

/// Per-session aggregate covering everything the SessionsList card +
/// Receipts panel need. Joins on `session_id`, GROUPs by it. Sessions
/// with no execution_logs rows still appear (the LEFT JOIN keeps them).
pub const V_SESSION_COST_SUMMARY: &str = "
CREATE VIEW IF NOT EXISTS v_session_cost_summary AS
SELECT
  s.id                                              AS session_id,
  s.title,
  s.runtime                                         AS anchor_runtime,
  s.agent_slug                                      AS anchor_agent,
  s.status,
  s.created_at,
  s.last_used_at,
  COUNT(el.id)                                      AS dispatch_count,
  SUM(CASE WHEN el.status = 'success' THEN 1 ELSE 0 END) AS success_count,
  SUM(CASE WHEN el.status = 'error'   THEN 1 ELSE 0 END) AS error_count,
  COUNT(DISTINCT el.runtime)                        AS distinct_runtimes,
  COUNT(DISTINCT el.agent_slug)                     AS distinct_agents,
  SUM(COALESCE(el.tokens_in, 0))                    AS total_tokens_in,
  SUM(COALESCE(el.tokens_out, 0))                   AS total_tokens_out,
  SUM(COALESCE(el.duration_ms, 0))                  AS total_duration_ms,
  SUM(COALESCE(el.cost_usd_estimated, 0))           AS total_cost_usd,
  MIN(el.created_at)                                AS first_dispatch_at,
  MAX(el.created_at)                                AS last_dispatch_at
FROM sessions s
LEFT JOIN execution_logs el ON el.session_id = s.id
GROUP BY s.id;
";

/// Per-(agent, runtime, auth_mode) rollup across ALL execution_logs
/// (no time filter — caller filters by created_at).
///
/// **Sentinel-collision fix (codex-reviewer 2026-05-17):** previous draft
/// used `COALESCE(agent_slug, '__generalist__')` which would collide if
/// any real agent slug happened to equal that literal. Now keeps
/// `agent_slug` as NULL for generalist turns and adds an explicit
/// `is_generalist` boolean. SQLite groups NULLs together correctly in
/// GROUP BY, so the rollup is still deterministic.
pub const V_COST_BY_AGENT_RUNTIME: &str = "
CREATE VIEW IF NOT EXISTS v_cost_by_agent_runtime AS
SELECT
  agent_slug,
  CASE WHEN agent_slug IS NULL THEN 1 ELSE 0 END    AS is_generalist,
  runtime,
  auth_mode,
  COUNT(*)                                          AS dispatch_count,
  SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) AS success_count,
  SUM(COALESCE(tokens_in, 0))                       AS tokens_in,
  SUM(COALESCE(tokens_out, 0))                      AS tokens_out,
  SUM(COALESCE(duration_ms, 0))                     AS duration_ms,
  SUM(COALESCE(cost_usd_estimated, 0))              AS cost_usd,
  MAX(created_at)                                   AS last_at
FROM execution_logs
GROUP BY agent_slug, runtime, auth_mode;
";

/// Audit/debug view: session_turns that have no matching execution_logs
/// row. Surfaces orphans from older flows (chat thread writes that
/// bypassed the dispatch path) or from rows lost to mid-flight crashes.
///
/// Added 2026-05-17 per codex-reviewer's request for unmatched-row views.
pub const V_ORPHANED_SESSION_TURNS: &str = "
CREATE VIEW IF NOT EXISTS v_orphaned_session_turns AS
SELECT
  st.session_id,
  st.turn_index,
  st.role,
  st.runtime,
  st.agent_slug,
  st.created_at,
  substr(st.text, 1, 200) AS text_preview
FROM session_turns st
WHERE NOT EXISTS (
  SELECT 1 FROM execution_logs el
   WHERE el.session_id = st.session_id
     AND el.runtime    = st.runtime
     AND ABS(strftime('%s', el.created_at) - strftime('%s', st.created_at)) < 5
);
";

/// Audit/debug view: execution_logs with a non-NULL session_id that
/// have no matching session_turns row. Should be empty in a healthy
/// system; non-empty means a dispatch wrote an execution log but the
/// session_turns INSERT failed silently.
///
/// Added 2026-05-17 per codex-reviewer's request for unmatched-row views.
pub const V_ORPHANED_EXECUTION_LOGS: &str = "
CREATE VIEW IF NOT EXISTS v_orphaned_execution_logs AS
SELECT
  el.id,
  el.session_id,
  el.runtime,
  el.agent_slug,
  el.status,
  el.created_at,
  substr(el.prompt, 1, 200) AS prompt_preview
FROM execution_logs el
WHERE el.session_id IS NOT NULL
  AND NOT EXISTS (
    SELECT 1 FROM session_turns st
     WHERE st.session_id = el.session_id
       AND st.runtime    = el.runtime
       AND ABS(strftime('%s', el.created_at) - strftime('%s', st.created_at)) < 5
  );
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_views_have_create_view_if_not_exists() {
        for stmt in ALL_VIEWS {
            assert!(
                stmt.contains("CREATE VIEW IF NOT EXISTS"),
                "every view MUST use IF NOT EXISTS so apply is idempotent: {stmt:.80}"
            );
        }
    }

    #[test]
    fn all_views_named_v_prefix() {
        // Convention: every view name starts with `v_` so callers can
        // tell views from tables at a glance.
        let names = [
            "v_session_audit",
            "v_recent_dispatches",
            "v_session_cost_summary",
            "v_cost_by_agent_runtime",
            "v_orphaned_session_turns",
            "v_orphaned_execution_logs",
        ];
        assert_eq!(ALL_VIEWS.len(), names.len(), "ALL_VIEWS and the names list drifted");
        for (stmt, name) in ALL_VIEWS.iter().zip(names.iter()) {
            assert!(stmt.contains(name), "view body must declare name {name}: missing in body");
        }
    }

    /// Codex-reviewer 2026-05-17 — guards against drifting back to the
    /// bad shape: a top-level (outermost) ORDER BY baked into the view.
    /// Inner ORDER BY inside a correlated subquery (e.g., "ORDER BY ...
    /// LIMIT 1" for nearest-match selection) IS legitimate — though we
    /// don't currently use it, since SQLite can't resolve correlated
    /// outer-scope columns inside a subquery's ORDER BY anyway.
    ///
    /// The check: the view body, after trimming trailing whitespace +
    /// semicolons, should not END with an ORDER BY clause. That's what
    /// "baked at the top level" means in practice — the consumer's
    /// SELECT FROM v_foo inherits that ORDER BY unless they explicitly
    /// override.
    #[test]
    fn no_top_level_order_by_in_views() {
        for stmt in ALL_VIEWS {
            let trimmed = stmt.trim_end_matches(|c: char| c.is_whitespace() || c == ';');
            // Walk the last 80 chars (enough to span "ORDER BY <col> ASC")
            // skipping anything inside the deepest parentheses level.
            // Cheaper than a full SQL parser; catches the pattern that
            // matters.
            let tail: String = trimmed.chars().rev().take(80).collect::<String>().chars().rev().collect();
            // Find the deepest closing paren in the tail; everything
            // after it is at the outer scope (the bad place for ORDER BY).
            let after_last_paren = match tail.rfind(')') {
                Some(idx) => &tail[idx + 1..],
                None => &tail[..],
            };
            assert!(
                !after_last_paren.to_uppercase().contains("ORDER BY"),
                "view has top-level ORDER BY (callers should sort, not the view): {stmt:.120}"
            );
        }
    }

    #[test]
    fn v_session_audit_uses_correlated_subquery_not_naive_join() {
        // The naive `LEFT JOIN ... AND ABS(Δt) < 5` shape allows
        // Cartesian explosion when two execution_logs match one turn.
        // The fix is a correlated subquery via `el.id = (SELECT … LIMIT 1)`.
        assert!(
            V_SESSION_AUDIT.contains("LIMIT 1"),
            "v_session_audit must pick exactly one execution_log per turn via LIMIT 1"
        );
    }

    #[test]
    fn no_sentinel_collision_in_rollup() {
        // Earlier draft used `COALESCE(agent_slug, '__generalist__')` —
        // string literal that could collide with a real agent slug.
        // The fix is an explicit `is_generalist` flag with NULL slugs.
        assert!(
            !V_COST_BY_AGENT_RUNTIME.contains("__generalist__"),
            "v_cost_by_agent_runtime must not use sentinel string for NULL agent_slug"
        );
        assert!(
            V_COST_BY_AGENT_RUNTIME.contains("is_generalist"),
            "v_cost_by_agent_runtime must expose is_generalist boolean"
        );
    }
}
