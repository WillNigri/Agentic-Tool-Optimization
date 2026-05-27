// `ato observe start/stop/status` — universal multi-LLM passive
// observer surface, mirroring the desktop's auto-started watcher
// (Hard Rule #6: CLI + UI parity).
//
// `start` blocks in the foreground. We write the running PID to
// ~/.ato/observe.pid for `status` / `stop` to find. SIGTERM (or
// Ctrl-C) shuts the process down; dropping the ObserverHandle stops
// the OS watchers cleanly.
//
// Why not daemonize ourselves? Daemonization on macOS is a footgun
// (launchd / launchctl is the recommended path; double-fork breaks
// keychain ACL inheritance per [[dev-build-keychain]]). Users that
// need background operation should run the foreground command from
// systemd / launchd / nohup. The CLI surface stays simple +
// predictable.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use ato_passive_observer::{ensure_schema, start_observer, SourceKind};
use serde::{Deserialize, Serialize};

use crate::output::{emit_human, emit_json, Opts};

#[derive(Debug, Serialize)]
struct StartReport {
    pid: u32,
    runtimes: Vec<&'static str>,
    db_path: String,
}

#[derive(Debug, Serialize)]
struct StatusReport {
    running: bool,
    pid: Option<u32>,
    pid_file: String,
    detail: &'static str,
}

#[derive(Debug, Serialize)]
struct StopReport {
    pid: Option<u32>,
    signaled: bool,
    cleaned_pidfile: bool,
}

fn pid_file_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("home directory unknown"))?;
    let dir = home.join(".ato");
    fs::create_dir_all(&dir).context("create ~/.ato")?;
    Ok(dir.join("observe.pid"))
}

fn parse_runtime_filter(tokens: &[String]) -> Result<Vec<SourceKind>> {
    let mut out = Vec::new();
    for tok in tokens {
        // Allow comma-separated values inside a single --runtime flag
        // so `--runtime claude,codex` Just Works alongside repeated
        // `--runtime` invocations.
        for part in tok.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let kind = SourceKind::from_cli_token(part)
                .ok_or_else(|| anyhow!("unknown runtime '{}' (expected: claude, codex, gemini)", part))?;
            if !out.contains(&kind) {
                out.push(kind);
            }
        }
    }
    Ok(out)
}

/// Pidfile carries identity so `stop` can verify the PID still
/// belongs to *our* binary before sending SIGTERM (per review
/// MEDIUM-6). Without this, a stale pidfile + PID reuse means
/// `ato observe stop` could SIGTERM an unrelated shell or build
/// process whose PID got recycled.
#[derive(Debug, Serialize, Deserialize)]
struct PidRecord {
    pid: u32,
    exe_path: String,
    started_at_unix: u64,
    /// The actual runtime set the watcher is tracking. Empty means
    /// "all known runtimes (claude, codex, gemini)". `ato observe
    /// status` reads this and reports the truth, not the current
    /// install-set on disk — those drift when the user installs a
    /// new CLI after the daemon started (per coordinator MEDIUM-2).
    #[serde(default)]
    runtimes: Vec<String>,
}

fn current_pid_record(runtimes: &[&'static str]) -> PidRecord {
    PidRecord {
        pid: std::process::id(),
        exe_path: std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        started_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        runtimes: runtimes.iter().map(|s| s.to_string()).collect(),
    }
}

fn write_pid(path: &Path, record: &PidRecord) -> Result<()> {
    let mut f = fs::File::create(path)
        .with_context(|| format!("write pidfile {}", path.display()))?;
    let line = serde_json::to_string(record).context("serialize pid record")?;
    writeln!(f, "{}", line).context("write pid")?;
    Ok(())
}

fn read_pid_record(path: &Path) -> Option<PidRecord> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    // Tolerate legacy bare-pid pidfiles from earlier dev builds —
    // they exist on machines that ran the pre-MEDIUM-6 version and
    // crashed without unlinking. A bare integer maps to a record
    // with empty exe_path / started_at=0; stop() will fail the
    // identity match and refuse to SIGTERM, which is the safe default.
    if let Ok(rec) = serde_json::from_str::<PidRecord>(trimmed) {
        return Some(rec);
    }
    if let Ok(pid) = trimmed.parse::<u32>() {
        return Some(PidRecord {
            pid,
            exe_path: String::new(),
            started_at_unix: 0,
            runtimes: Vec::new(),
        });
    }
    None
}

fn read_pid(path: &Path) -> Option<u32> {
    read_pid_record(path).map(|r| r.pid)
}

#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    // signal 0 = existence probe, no actual signal delivered.
    unsafe { libc_kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn pid_is_alive(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

pub fn start(db_path: &Path, runtimes: &[String], opts: &Opts) -> Result<()> {
    let filter = parse_runtime_filter(runtimes)?;
    let pid_path = pid_file_path()?;

    // Refuse to start a second observer ONLY if the recorded PID is
    // alive AND that process is our own binary (per review MEDIUM-6).
    // PID reuse could otherwise leave us stuck behind an unrelated
    // shell that recycled the watcher's old PID.
    if let Some(existing) = read_pid_record(&pid_path) {
        if pid_is_alive(existing.pid) {
            let our_exe = std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !existing.exe_path.is_empty() && existing.exe_path == our_exe {
                return Err(anyhow!(
                    "an observer is already running (pid {}). \
                     Stop it first with `ato observe stop`.",
                    existing.pid
                ));
            }
            // Identity mismatch: the PID belongs to another process.
            // Treat the pidfile as stale and replace it below.
        }
        let _ = fs::remove_file(&pid_path);
    }

    // Bootstrap the tables the observer writes BEFORE starting the
    // watcher. On a headless box the desktop's init_database hasn't
    // run, so execution_logs / watcher_state / live_runs don't exist
    // and every INSERT OR IGNORE would silently swallow a
    // "no such table" error (per review HIGH-2). Idempotent against
    // the desktop's own initializer — CREATE TABLE IF NOT EXISTS
    // means a desktop box's existing schema is left untouched.
    ensure_schema(db_path)
        .map_err(|e| anyhow!("ensure_schema failed: {}", e))?;

    let handle = start_observer(db_path.to_path_buf(), &filter)
        .map_err(|e| anyhow!("start_observer failed: {}", e))?;
    if handle.is_none() {
        if opts.human {
            emit_human(
                "No supported CLI session directories found yet \
                 (~/.claude/projects, ~/.codex/sessions, ~/.gemini). \
                 Install at least one and re-run.",
            );
        } else {
            emit_json(&StartReport {
                pid: std::process::id(),
                runtimes: filter.iter().map(|k| k.runtime()).collect(),
                db_path: db_path.display().to_string(),
            })?;
        }
        return Ok(());
    }
    let _handle = handle; // keep alive for the lifetime of the call

    let runtimes_label: Vec<&'static str> = if filter.is_empty() {
        vec!["claude", "codex", "gemini"]
    } else {
        filter.iter().map(|k| k.runtime()).collect()
    };

    let pid_record = current_pid_record(&runtimes_label);
    let pid = pid_record.pid;
    write_pid(&pid_path, &pid_record)?;

    if opts.human {
        emit_human(&format!(
            "ato observe started (pid {}). Watching {} on {}. Ctrl-C to stop.",
            pid,
            runtimes_label.join(", "),
            db_path.display()
        ));
    } else {
        emit_json(&StartReport {
            pid,
            runtimes: runtimes_label.clone(),
            db_path: db_path.display().to_string(),
        })?;
    }

    // Install a SIGINT/SIGTERM handler. Any signal flips the flag;
    // the main loop polls + exits cleanly so the ObserverHandle drop
    // tears down the watchers.
    let shutdown = Arc::new(AtomicBool::new(false));
    install_signal_handlers(&shutdown);

    while !shutdown.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(500));
    }

    // Clean up our pidfile only if it still points at us (avoid
    // racing with a concurrent `start` that took over).
    if read_pid(&pid_path) == Some(pid) {
        let _ = fs::remove_file(&pid_path);
    }
    if opts.human {
        emit_human("ato observe stopped.");
    }
    Ok(())
}

static SIGNAL_RECEIVED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn signal_handler_stub(_sig: i32) {
    // Async-signal-safe: only touches an AtomicBool. The main loop
    // polls this every 500ms and exits gracefully.
    SIGNAL_RECEIVED.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
fn install_signal_handlers(shutdown: &Arc<AtomicBool>) {
    // Coordinator review LOW-4: sigaction() over the deprecated
    // signal() syscall. sigaction gives predictable behaviour
    // across re-entry + signal-during-handler edge cases; the
    // (long-deprecated) signal() syscall has portable-but-vague
    // semantics that historically reset to SIG_DFL on first delivery
    // on some systems.
    use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal};
    let action = SigAction::new(
        SigHandler::Handler(signal_handler_stub),
        SaFlags::SA_RESTART,
        SigSet::empty(),
    );
    unsafe {
        let _ = sigaction(Signal::SIGINT, &action);
        let _ = sigaction(Signal::SIGTERM, &action);
    }

    // Bridge thread: copies the static flag into the per-call
    // shutdown handle, and also treats pidfile deletion as a stop
    // signal (so `ato observe stop` works even if the SIGTERM
    // delivery is racy).
    let flag = shutdown.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(200));
        if SIGNAL_RECEIVED.load(Ordering::SeqCst) {
            flag.store(true, Ordering::SeqCst);
            return;
        }
        if let Ok(p) = pid_file_path() {
            if !p.exists() {
                flag.store(true, Ordering::SeqCst);
                return;
            }
        }
    });
}

#[cfg(not(unix))]
fn install_signal_handlers(_shutdown: &Arc<AtomicBool>) {
    // Windows: skip explicit handlers. Ctrl-C terminates the process;
    // watcher cleanup happens on stack unwind through Drop.
}

pub fn stop(opts: &Opts) -> Result<()> {
    let pid_path = pid_file_path()?;
    let record = read_pid_record(&pid_path);
    let pid = record.as_ref().map(|r| r.pid);

    // Identity check (per review MEDIUM-6): only SIGTERM the PID if
    // the recorded exe_path matches our own. PIDs get reused; a stale
    // pidfile + a recycled PID owned by a shell or build process
    // would otherwise kill an unrelated user process. When the
    // pidfile is the legacy bare-int format the exe_path is "" and
    // the match fails — we still clean up the file but skip the
    // kill, which is the safe default.
    let mut signaled = false;
    let mut skipped_due_to_identity_mismatch = false;
    if let (Some(rec), Some(p)) = (&record, pid) {
        if pid_is_alive(p) {
            let our_exe = std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !rec.exe_path.is_empty() && rec.exe_path == our_exe {
                #[cfg(unix)]
                unsafe {
                    if libc_kill(p as i32, 15) == 0 {
                        signaled = true;
                    }
                }
            } else {
                skipped_due_to_identity_mismatch = true;
            }
        }
    }
    let cleaned = pid_path.exists() && fs::remove_file(&pid_path).is_ok();

    let report = StopReport { pid, signaled, cleaned_pidfile: cleaned };
    if opts.human {
        match pid {
            Some(p) if signaled => emit_human(&format!(
                "Sent SIGTERM to pid {} and cleared {}.",
                p,
                pid_path.display()
            )),
            Some(p) if skipped_due_to_identity_mismatch => emit_human(&format!(
                "Pid {} is alive but doesn't match our binary — refusing to SIGTERM \
                 (PID reuse / stale legacy pidfile). Cleared {}.",
                p,
                pid_path.display()
            )),
            Some(p) => emit_human(&format!(
                "Pid {} was not running; cleared stale pidfile {}.",
                p,
                pid_path.display()
            )),
            None => emit_human("No observer pidfile found — nothing to stop."),
        }
    } else {
        emit_json(&report)?;
    }
    Ok(())
}

pub fn status(opts: &Opts) -> Result<()> {
    let pid_path = pid_file_path()?;
    let pid = read_pid(&pid_path);
    let running = match pid {
        Some(p) => pid_is_alive(p),
        None => false,
    };
    let detail = match (pid, running) {
        (None, _) => "no pidfile",
        (Some(_), false) => "stale pidfile (process not running)",
        (Some(_), true) => "running",
    };
    let report = StatusReport {
        running,
        pid,
        pid_file: pid_path.display().to_string(),
        detail,
    };
    if opts.human {
        match (pid, running) {
            (Some(p), true) => emit_human(&format!("ato observe is running (pid {}).", p)),
            (Some(p), false) => emit_human(&format!(
                "Stale pidfile at {} references pid {}, which is not running. \
                 Run `ato observe stop` to clean up.",
                pid_path.display(),
                p
            )),
            (None, _) => emit_human("ato observe is not running."),
        }
    } else {
        emit_json(&report)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_runtime_filter_accepts_known_tokens() {
        let v = parse_runtime_filter(&[
            "claude".into(),
            "codex,gemini".into(),
            "  GEMINI  ".into(),
        ])
        .unwrap();
        assert_eq!(v.len(), 3);
        assert!(v.contains(&SourceKind::ClaudeCode));
        assert!(v.contains(&SourceKind::Codex));
        assert!(v.contains(&SourceKind::Gemini));
    }

    #[test]
    fn parse_runtime_filter_rejects_unknown() {
        let err = parse_runtime_filter(&["aider".into()]).unwrap_err();
        assert!(err.to_string().contains("unknown runtime"));
    }

    #[test]
    fn parse_runtime_filter_empty_yields_empty() {
        let v = parse_runtime_filter(&[]).unwrap();
        assert!(v.is_empty());
    }
}
