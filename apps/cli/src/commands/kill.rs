// `ato kill <run-id>` — terminate a running dispatch.
//
// The desktop tracks every in-flight dispatch in live_runs (SQLite mirror
// of its in-memory registry). When the dispatch path spawns the runtime
// subprocess, it records the child's OS PID in live_runs.child_pid.
// The CLI reads that PID and sends SIGTERM directly. The desktop's
// dispatch handler's tokio::select! catches the child exit, runs cleanup
// (finish_run removes the row), and returns "killed by user".
//
// This works across the CLI/desktop process boundary because OS signals
// don't care which process sends them — only that you own the PID's
// session (true for any process running as the same user).
//
// For runs the CLI itself started: those are sync, so they're not
// queryable from a separate CLI invocation while they're running. The
// human would Ctrl-C the original shell. That's a Phase 1.x limitation
// we accept — making CLI dispatches async + registered in live_runs is
// a small follow-up.

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::process::Command;

#[derive(Debug, Serialize)]
pub struct KillResult {
    pub run_id: String,
    pub child_pid: Option<i64>,
    pub signaled: bool,
    pub note: String,
}

pub fn run(conn: &Connection, run_id: &str, opts: &Opts) -> Result<()> {
    // live_runs is created by the desktop on first launch after v2.3.0.
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='live_runs'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if table_exists == 0 {
        return Err(anyhow!(
            "live_runs table not found. The desktop GUI populates this table; either the desktop hasn't run since v2.3.0 or it isn't running now."
        ));
    }

    let row: Option<(String, Option<i64>)> = conn
        .query_row(
            "SELECT run_id, child_pid FROM live_runs WHERE run_id = ?1",
            [run_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    let (run_id, pid_opt) = match row {
        Some(t) => t,
        None => {
            return Err(anyhow!(
                "No live run with id '{}'. Use `ato runs live` to see active runs.",
                run_id
            ))
        }
    };

    let result = match pid_opt {
        Some(pid) => {
            let signaled = send_term_signal(pid as u32).is_ok();
            KillResult {
                run_id: run_id.clone(),
                child_pid: Some(pid),
                signaled,
                note: if signaled {
                    "Sent SIGTERM to child process. The desktop's dispatch handler will finalize the run.".to_string()
                } else {
                    "Failed to send signal — the child process may have already exited.".to_string()
                },
            }
        }
        None => KillResult {
            run_id: run_id.clone(),
            child_pid: None,
            signaled: false,
            note: "This run has no recorded child_pid (older registration or the dispatch path doesn't expose one). Kill from the desktop GUI instead.".to_string(),
        },
    };

    if opts.human {
        emit_human(&format!(
            "Kill {} (pid={}): {} — {}",
            result.run_id,
            result.child_pid.map(|p| p.to_string()).unwrap_or_else(|| "?".into()),
            if result.signaled { "signaled" } else { "not signaled" },
            result.note
        ));
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

#[cfg(unix)]
fn send_term_signal(pid: u32) -> Result<()> {
    // Use the system `kill` binary rather than libc bindings. Smaller
    // dependency surface; portable across Linux + macOS without
    // platform-specific crates.
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .context("failed to invoke kill(1)")?;
    if !status.success() {
        return Err(anyhow!("kill -TERM {} exited with status {}", pid, status));
    }
    Ok(())
}

#[cfg(windows)]
fn send_term_signal(pid: u32) -> Result<()> {
    let status = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .status()
        .context("failed to invoke taskkill")?;
    if !status.success() {
        return Err(anyhow!("taskkill /F /PID {} exited with status {}", pid, status));
    }
    Ok(())
}
