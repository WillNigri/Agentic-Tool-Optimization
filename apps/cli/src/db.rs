// Database access: open a read-only handle on the same SQLite file the
// desktop GUI uses. We intentionally do NOT call init_database here —
// the CLI assumes the desktop has run at least once and created the
// schema. If the DB doesn't exist, every command fails with a clean
// error pointing the user at the install instructions.
//
// Why read-only by default: defense against accidental writes when an
// agent shells out without thinking. Subcommands that need to write
// reopen with write privileges explicitly (Phase 1 only has reads).

use anyhow::{anyhow, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

pub fn default_db_path() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    path.push("local.db");
    path
}

pub fn home_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
    } else if let Ok(profile) = std::env::var("USERPROFILE") {
        PathBuf::from(profile)
    } else {
        PathBuf::from(".")
    }
}

/// 5 second busy_timeout — when desktop and CLI overlap on the same
/// SQLite, the loser waits up to this long for the lock to clear
/// before failing with `database is locked`. Without it, concurrent
/// writes from both processes can transient-fail. Caught by
/// codex-reviewer in the v2.3.7 review.
const SQLITE_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub fn open_readonly(path: &Path) -> Result<Connection> {
    if !path.exists() {
        return Err(anyhow!(
            "ATO database not found at {}.\n\nThis usually means the ATO desktop app hasn't been installed or hasn't been run yet. Install: https://agentictool.ai or `brew install --cask ato`.",
            path.display()
        ));
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
    Ok(conn)
}

/// Open the same DB with write permissions. Used by Operations and
/// Authoring commands that need to INSERT/UPDATE rows. Same path-existence
/// check + same error message — the schema is assumed to be in place
/// (created by the desktop on first launch).
pub fn open_readwrite(path: &Path) -> Result<Connection> {
    if !path.exists() {
        return Err(anyhow!(
            "ATO database not found at {}.\n\nWrite operations require the ATO desktop app to have run at least once to create the schema. Install: https://agentictool.ai or `brew install --cask ato`.",
            path.display()
        ));
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
    // 2026-05-16 — idempotent column adds the desktop migration also
    // does. If the user is running CLI-only without ever opening the
    // desktop, these ALTER TABLEs ensure the dispatch INSERTs (which
    // write to these columns) don't fail with "no such column". Each
    // ALTER fails silently when the column already exists.
    let _ = conn.execute(
        "ALTER TABLE session_turns ADD COLUMN agent_slug TEXT",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_session_turns_agent_slug ON session_turns(agent_slug)",
        [],
    );
    // PR 14a (2026-05-18) — war_room_id on execution_logs. Same
    // CLI-only guard as the column above: a user dispatching from
    // the CLI without ever opening the desktop needs this column to
    // exist so `tag_war_room_id` in commands/dispatch.rs doesn't
    // hit "no such column" on the follow-up UPDATE.
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN war_room_id TEXT",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_war_room_id
            ON execution_logs(war_room_id, created_at DESC)
          WHERE war_room_id IS NOT NULL",
        [],
    );
    // PR 16 (2026-05-18) — war-rooms evolve to multi-turn. Mirror
    // of the desktop migration so CLI-only users (no desktop ever
    // opened) get the war_room_round column without hitting "no
    // such column" on `tag_war_room` UPDATEs. See lib.rs for the
    // full semantics; in one sentence: the prior single-round war-
    // room becomes round 1 of an N-round sequence. Within a round
    // seats fire in parallel and don't see each other; across
    // rounds every seat sees the full transcript.
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN war_room_round INTEGER",
        [],
    );
    let _ = conn.execute(
        "UPDATE execution_logs SET war_room_round = 1
          WHERE war_room_id IS NOT NULL AND war_room_round IS NULL",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_war_room_round
            ON execution_logs(war_room_id, war_room_round, created_at ASC)
          WHERE war_room_id IS NOT NULL",
        [],
    );

    // v2.15.5 — fallback-chain receipt (war_room CC9DBD0E). CLI-only
    // guard: fails silently when the column already exists (desktop
    // migration applied it first via schema.rs). Must appear before any
    // run_api INSERT that binds this column.
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN fallback_of TEXT",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_fallback_of \
            ON execution_logs(fallback_of) \
            WHERE fallback_of IS NOT NULL",
        [],
    );

    // v2.17 — git_commit_sha provenance (CLI backfill, mirrors schema.rs).
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN git_commit_sha TEXT",
        [],
    );

    // v2.15.1 — retry receipt columns (CLI-only guard, mirrors schema.rs).
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN attempt_summary TEXT",
        [],
    );

    // cost-accounting fix cluster (2026-06-12) — cache + reasoning token
    // columns. Each ALTER fails silently when the column already exists
    // (desktop schema.rs applied it first). Backfill runs below.
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN cache_creation_tokens INTEGER",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN cache_read_tokens INTEGER",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN reasoning_tokens INTEGER",
        [],
    );

    // ato_meta: tiny key/value table for migration markers.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS ato_meta (key TEXT PRIMARY KEY, value TEXT)",
        [],
    );

    // One-time NULL-cost recompute (Section 5).
    // Guard: only runs when the 'cost_recompute_v1' marker is absent.
    // For each historical row that has tokens but no cost and a known
    // model, recompute and write the cost. Makes it cheap and idempotent.
    //
    // Candidate predicate intentionally excludes rows that already carry
    // cache_creation_tokens or cache_read_tokens: those rows must be
    // priced via cost_from_token_classes with the real cache values (not
    // the cache-blind formula). They keep cost_usd_estimated=NULL until
    // the next dispatch path writes them correctly.
    //
    // Historical rows are repriced at CURRENT table rates — an estimate,
    // the same approximation the cost panel already makes for all rows.
    let already_done: bool = conn
        .query_row(
            "SELECT 1 FROM ato_meta WHERE key = 'cost_recompute_v1'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);
    if !already_done {
        // Collect candidate rows in one SELECT, then UPDATE in a loop.
        // Using a Vec avoids holding an active statement across UPDATEs.
        let candidates: Vec<(String, String, i64, i64)> = conn
            .prepare(
                "SELECT id, model, tokens_in, tokens_out
                   FROM execution_logs
                  WHERE cost_usd_estimated IS NULL
                    AND tokens_in IS NOT NULL
                    AND tokens_out IS NOT NULL
                    AND model IS NOT NULL
                    AND model != ''
                    AND cache_creation_tokens IS NULL
                    AND cache_read_tokens IS NULL",
            )
            .map(|mut stmt| {
                stmt.query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
                })
                .map(|rows| rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
                .unwrap_or_default()
            })
            .unwrap_or_default();
        for (id, model, tin, tout) in candidates {
            // cost_from_token_classes with None cache fields is
            // arithmetically identical to cost_from_tokens today and
            // future-proof if per-class rates diverge.
            let tc = ato_pricing::TokenClasses {
                tokens_in: tin,
                tokens_out: tout,
                cache_creation_in: None,
                cache_read_in: None,
            };
            if let Some(cost) = ato_pricing::cost_from_token_classes(&model, &tc) {
                let _ = conn.execute(
                    "UPDATE execution_logs SET cost_usd_estimated = ?1 WHERE id = ?2",
                    rusqlite::params![cost, id],
                );
            }
        }
        // Set the marker so subsequent opens are instant.
        let _ = conn.execute(
            "INSERT OR REPLACE INTO ato_meta (key, value) VALUES ('cost_recompute_v1', '1')",
            [],
        );
    }

    // v2.16 PR-3 — repo_root column on missions (per_agent_worktree
    // support). Same CLI-only guard pattern: fails silently when the
    // column already exists (desktop migration applied it first).
    let _ = conn.execute("ALTER TABLE missions ADD COLUMN repo_root TEXT", []);

    // v2.16 PR-4 — worker_config column on missions (coordinator tick).
    // JSON shape: {"runtime":"...","model":null|"...","require_tools":["..."]}.
    // NULL = tick will escalate with reason="no_worker_config".
    let _ = conn.execute("ALTER TABLE missions ADD COLUMN worker_config TEXT", []);
    // inputs bundle storage (OSS). CLI mirrors the desktop schema so a
    // write from `ato inputs` works even if the user has not opened the
    // desktop since upgrading.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS inputs (
            id            TEXT PRIMARY KEY,
            slug          TEXT NOT NULL UNIQUE,
            name          TEXT NOT NULL,
            content       TEXT NOT NULL,
            kind          TEXT NOT NULL DEFAULT 'markdown',
            tags          TEXT,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_inputs_updated
            ON inputs(updated_at DESC)",
        [],
    );

    // v2.17 — Output bundles. Idempotent backfill so CLI-only users get
    // the table even if the desktop schema migration hasn't run yet.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS output_bundles (
            id            TEXT PRIMARY KEY,
            slug          TEXT NOT NULL UNIQUE,
            name          TEXT NOT NULL,
            description   TEXT,
            source_kind   TEXT NOT NULL,
            source_id     TEXT NOT NULL,
            manifest      TEXT NOT NULL,
            export_path   TEXT,
            signed_url    TEXT,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL
        )",
        [],
    );

    // 2026-05-17 — SQL views from `packages/ato-db-views`. Mirror of
    // what the desktop applies on startup. Each `CREATE VIEW IF NOT
    // EXISTS` is a no-op after the first run, so applying on every
    // open is cheap and means CLI-only users never see a missing view.
    for stmt in ato_db_views::ALL_VIEWS {
        let _ = conn.execute(stmt, []);
    }
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    /// Minimal in-memory schema that exercises open_readwrite's recompute.
    fn bootstrap(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE execution_logs (
                id                    TEXT PRIMARY KEY,
                runtime               TEXT NOT NULL,
                prompt                TEXT,
                response              TEXT,
                tokens_in             INTEGER,
                tokens_out            INTEGER,
                duration_ms           INTEGER,
                status                TEXT NOT NULL,
                error_message         TEXT,
                skill_name            TEXT,
                cloud_trace_id        TEXT,
                created_at            TEXT NOT NULL,
                cost_usd_estimated    REAL,
                session_id            TEXT,
                tool_calls_count      INTEGER,
                tool_calls_summary    TEXT,
                model                 TEXT,
                auth_mode             TEXT,
                agent_slug            TEXT,
                retry_count           INTEGER NOT NULL DEFAULT 0,
                attempt_summary       TEXT,
                fallback_of           TEXT,
                war_room_id           TEXT,
                war_room_round        INTEGER,
                cache_creation_tokens INTEGER,
                cache_read_tokens     INTEGER,
                reasoning_tokens      INTEGER
            );
            CREATE TABLE ato_meta (key TEXT PRIMARY KEY, value TEXT);",
        )
        .expect("bootstrap schema");
    }

    fn insert_row(
        conn: &Connection,
        id: &str,
        model: Option<&str>,
        tokens_in: Option<i64>,
        tokens_out: Option<i64>,
        cost: Option<f64>,
        cache_creation_tokens: Option<i64>,
        cache_read_tokens: Option<i64>,
    ) {
        conn.execute(
            "INSERT INTO execution_logs
             (id, runtime, status, created_at, prompt, model, tokens_in, tokens_out,
              cost_usd_estimated, cache_creation_tokens, cache_read_tokens)
             VALUES (?1, 'anthropic', 'success', '2026-06-12T00:00:00+00:00', 'hi',
                     ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, model, tokens_in, tokens_out, cost,
                               cache_creation_tokens, cache_read_tokens],
        )
        .expect("insert");
    }

    /// Recompute prices a cache-free row and leaves a cache-bearing row alone.
    #[test]
    fn recompute_skips_cache_bearing_rows() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        bootstrap(&conn);

        // Row A: known model, no cost, no cache tokens → should be priced.
        insert_row(&conn, "row-a", Some("claude-sonnet-4-6"), Some(1000), Some(500),
                   None, None, None);

        // Row B: known model, no cost, but HAS cache_creation_tokens →
        // must NOT be touched by the cache-blind recompute formula.
        insert_row(&conn, "row-b", Some("claude-sonnet-4-6"), Some(1000), Some(500),
                   None, Some(200), None);

        // Run the recompute block manually (mirrors open_readwrite logic).
        let candidates: Vec<(String, String, i64, i64)> = conn
            .prepare(
                "SELECT id, model, tokens_in, tokens_out
                   FROM execution_logs
                  WHERE cost_usd_estimated IS NULL
                    AND tokens_in IS NOT NULL
                    AND tokens_out IS NOT NULL
                    AND model IS NOT NULL
                    AND model != ''
                    AND cache_creation_tokens IS NULL
                    AND cache_read_tokens IS NULL",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        for (id, model, tin, tout) in candidates {
            let tc = ato_pricing::TokenClasses {
                tokens_in: tin,
                tokens_out: tout,
                cache_creation_in: None,
                cache_read_in: None,
            };
            if let Some(cost) = ato_pricing::cost_from_token_classes(&model, &tc) {
                conn.execute(
                    "UPDATE execution_logs SET cost_usd_estimated = ?1 WHERE id = ?2",
                    rusqlite::params![cost, id],
                )
                .unwrap();
            }
        }

        let cost_a: Option<f64> = conn
            .query_row(
                "SELECT cost_usd_estimated FROM execution_logs WHERE id = 'row-a'",
                [], |r| r.get(0),
            )
            .unwrap();
        let cost_b: Option<f64> = conn
            .query_row(
                "SELECT cost_usd_estimated FROM execution_logs WHERE id = 'row-b'",
                [], |r| r.get(0),
            )
            .unwrap();

        assert!(cost_a.is_some(), "row-a (no cache tokens) must be priced by recompute");
        assert!(
            cost_b.is_none(),
            "row-b (has cache_creation_tokens) must NOT be touched by cache-blind recompute"
        );
    }
}

/// Parse a `--since` window string like "7d", "24h", "30m" into a SQLite
/// `datetime('now', '-...')` modifier string. Returns the modifier so
/// callers can do `datetime('now', modifier)` in their queries.
///
/// Supported suffixes: `d` days, `h` hours, `m` minutes. The number is
/// parsed as an integer (no fractional units). Returns an error for
/// anything we don't recognize so the CLI fails loudly rather than
/// silently returning a wrong window.
pub fn parse_since(since: &str) -> Result<String> {
    if since.is_empty() {
        return Err(anyhow!("--since cannot be empty"));
    }
    let (num_str, unit) = since.split_at(since.len() - 1);
    let n: i64 = num_str
        .parse()
        .map_err(|_| anyhow!("--since must be like '7d', '24h', or '30m', got '{}'", since))?;
    let unit_str = match unit {
        "d" => "days",
        "h" => "hours",
        "m" => "minutes",
        _ => {
            return Err(anyhow!(
                "--since unit must be d, h, or m; got '{}' in '{}'",
                unit,
                since
            ));
        }
    };
    Ok(format!("-{} {}", n, unit_str))
}
