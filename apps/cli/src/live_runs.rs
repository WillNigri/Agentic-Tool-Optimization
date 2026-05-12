// v2.3.25 Phase 6.x — CLI dispatches visible in the desktop's Live tab.
//
// The desktop maintains an in-memory active_runs registry plus a
// SQLite mirror (live_runs) so the CLI process can READ what's
// running. This module flips that relationship: the CLI now also
// WRITES to live_runs at dispatch start + DELETEs at end, so the
// desktop's list_active_runs command can return CLI runs alongside
// GUI ones.
//
// What's visible-but-unkillable: the desktop has no kill closure for
// CLI runs (kill closures aren't serializable; they live in the
// dispatcher process). The Live tab will show them with the standard
// "kill not supported" treatment. Phase 6.x v2 will add PID-tracking
// so the desktop can SIGKILL across processes.

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

pub fn insert(
    db_path: &Path,
    run_id: &str,
    runtime: &str,
    agent_slug: Option<&str>,
    workspace: Option<&str>,
    source: &str,
) -> Result<()> {
    let conn = Connection::open(db_path)?;
    // Schema-missing means desktop never ran on this machine. Failing
    // silent is fine — dispatch still proceeds, just not visible in
    // the Live tab.
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='live_runs'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return Ok(());
    }
    let started_at = chrono::Utc::now().to_rfc3339();
    let pid: Option<i64> = Some(std::process::id() as i64);
    conn.execute(
        "INSERT OR REPLACE INTO live_runs (run_id, agent_slug, runtime, workspace, source, started_at, status, child_pid)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running', ?7)",
        rusqlite::params![
            run_id,
            agent_slug,
            runtime,
            workspace,
            source,
            started_at,
            pid,
        ],
    )?;
    Ok(())
}

pub fn delete(db_path: &Path, run_id: &str) -> Result<()> {
    let conn = Connection::open(db_path)?;
    conn.execute("DELETE FROM live_runs WHERE run_id = ?1", [run_id])?;
    Ok(())
}

/// RAII guard. Drop() calls delete() so the live_runs row is cleaned
/// up on every exit path — including the `?` early-return after a
/// spawn failure (MiniMax round-1 found that the previous code only
/// deleted on the success path of cmd.output()). Stack-allocated;
/// keep one in scope for the entire dispatch lifetime.
pub struct LiveRunGuard {
    db_path: std::path::PathBuf,
    run_id: String,
}

impl LiveRunGuard {
    pub fn new(db_path: &Path, run_id: String) -> Self {
        Self {
            db_path: db_path.to_path_buf(),
            run_id,
        }
    }
}

impl Drop for LiveRunGuard {
    fn drop(&mut self) {
        let _ = delete(&self.db_path, &self.run_id);
    }
}
