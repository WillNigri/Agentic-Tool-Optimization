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

    // 2026-05-17 — SQL views from `packages/ato-db-views`. Mirror of
    // what the desktop applies on startup. Each `CREATE VIEW IF NOT
    // EXISTS` is a no-op after the first run, so applying on every
    // open is cheap and means CLI-only users never see a missing view.
    for stmt in ato_db_views::ALL_VIEWS {
        let _ = conn.execute(stmt, []);
    }
    Ok(conn)
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
