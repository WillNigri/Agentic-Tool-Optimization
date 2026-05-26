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
use ato_passive_observer::{start_observer, SourceKind};
use serde::Serialize;

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

fn write_pid(path: &Path, pid: u32) -> Result<()> {
    let mut f = fs::File::create(path)
        .with_context(|| format!("write pidfile {}", path.display()))?;
    writeln!(f, "{}", pid).context("write pid")?;
    Ok(())
}

fn read_pid(path: &Path) -> Option<u32> {
    let raw = fs::read_to_string(path).ok()?;
    raw.trim().parse::<u32>().ok()
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

    // Refuse to start a second observer on the same machine.
    if let Some(existing) = read_pid(&pid_path) {
        if pid_is_alive(existing) {
            return Err(anyhow!(
                "an observer is already running (pid {}). \
                 Stop it first with `ato observe stop`.",
                existing
            ));
        }
        // Stale pidfile from a previous crash — clean up.
        let _ = fs::remove_file(&pid_path);
    }

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

    let pid = std::process::id();
    write_pid(&pid_path, pid)?;

    let runtimes_label: Vec<&'static str> = if filter.is_empty() {
        vec!["claude", "codex", "gemini"]
    } else {
        filter.iter().map(|k| k.runtime()).collect()
    };

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
    extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    unsafe {
        // SIGINT = 2, SIGTERM = 15.
        signal(2, signal_handler_stub as *const () as usize);
        signal(15, signal_handler_stub as *const () as usize);
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
    let pid = read_pid(&pid_path);

    let mut signaled = false;
    if let Some(p) = pid {
        if pid_is_alive(p) {
            #[cfg(unix)]
            unsafe {
                // SIGTERM = 15
                if libc_kill(p as i32, 15) == 0 {
                    signaled = true;
                }
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
