// ato-passive-observer — shared tail-watcher for the v2.6+ universal
// multi-LLM observability tier.
//
// Why a shared crate? Before v2.13 the watcher lived only in the
// desktop's `passive_observer.rs` (Tauri-coupled, auto-started on app
// boot). That meant headless users — CLI-only dev boxes, CI machines,
// servers running `ato observe` as a systemd service — got no
// observability at all. v2.13 lifts the parser + watcher + persistence
// into this crate so both surfaces consume the same logic:
//   - `apps/desktop/src-tauri/src/passive_observer.rs` (thin Tauri
//     state wrapper)
//   - `apps/cli/src/commands/observe.rs` (`ato observe start/stop/status`)
//
// Hard rule #6 (CLI + UI parity) and rule #3 (no duplication / ≤600
// LOC per module) drove the extraction.
//
// What lives here:
//   1. Source discovery — ~/.claude/projects, ~/.codex/sessions,
//      ~/.gemini/sessions / ~/.gemini/tmp/<session-id>/logs.json
//   2. The notify v6 watcher loop with 250ms event coalescing.
//   3. Per-runtime line parsers (Claude Code, Codex, Gemini).
//   4. Self-contained persistence into execution_logs +
//      watcher_state (no Tauri / no event-bus dependency).
//
// What does NOT live here:
//   - Tier-gating. Passive observation is OSS / Free per
//     [[pro-features-never-in-oss]] — local SQLite only. Cloud
//     aggregation is Pro (services/observability-ingest in
//     ato-cloud).
//   - Pricing math. We delegate to `ato-pricing` for token estimation
//     + USD cost lookup so the pricing table stays single-sourced.
//   - HTTP. The cloud uploader is a separate concern; this crate
//     only writes local SQLite rows.

pub mod parser_claude;
pub mod parser_codex;
pub mod parser_gemini;
pub mod persist;
pub mod schema;
pub mod sources;
pub mod worker;

pub use schema::ensure_schema;

use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub use sources::{Source, SourceKind};
pub use worker::ScanRequest;

/// Handle returned by `start_observer`. Drop it to stop the watcher
/// — `_watchers` going out of scope cancels the OS-level FS event
/// subscriptions; the worker thread exits when the mpsc sender is
/// dropped and the channel closes.
pub struct ObserverHandle {
    _watchers: Vec<RecommendedWatcher>,
    /// Kept alive so the worker thread doesn't exit prematurely; the
    /// caller-side handle lets `stop()` explicitly close it.
    _tx: Sender<ScanRequest>,
}

/// Start a watcher across every supported runtime directory the user
/// has on disk. Returns Ok(None) when no supported CLI session
/// directories exist yet — the caller can treat that as "idle, retry
/// later" rather than an error.
///
/// `runtime_filter` optionally restricts which CLIs are watched. An
/// empty vec means "all known runtimes".
pub fn start_observer(
    db_path: PathBuf,
    runtime_filter: &[SourceKind],
) -> Result<Option<ObserverHandle>, String> {
    let home = dirs::home_dir().ok_or_else(|| "home directory unknown".to_string())?;
    let mut sources = sources::discover_sources(&home);
    if !runtime_filter.is_empty() {
        sources.retain(|s| runtime_filter.contains(&s.kind));
    }
    if sources.is_empty() {
        return Ok(None);
    }

    let (tx, rx) = channel::<ScanRequest>();

    let mut watchers: Vec<RecommendedWatcher> = Vec::new();
    for src in &sources {
        let tx_clone = tx.clone();
        let src_clone = src.clone();
        let mut w = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(ev) = res {
                    on_fs_event(&src_clone, ev, &tx_clone);
                }
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        )
        .map_err(|e| format!("failed to create watcher for {:?}: {}", src.root, e))?;
        if src.root.exists() {
            w.watch(&src.root, RecursiveMode::Recursive)
                .map_err(|e| format!("failed to watch {:?}: {}", src.root, e))?;
        }
        watchers.push(w);
    }

    // Initial sweep: process every existing jsonl from its stored
    // offset so the user's pre-launch history catches up immediately.
    let initial_paths: Vec<(SourceKind, PathBuf)> = sources
        .iter()
        .flat_map(|s| sources::enumerate_existing(s).into_iter().map(move |p| (s.kind, p)))
        .collect();
    for (kind, path) in initial_paths {
        let _ = tx.send(ScanRequest { kind, path });
    }

    // mpsc::Receiver has exactly one consumer; move it into the
    // worker thread directly. (Earlier draft wrapped it in
    // Arc<Mutex<>> — removed per review LOW-9; the wrapping added a
    // runtime lock that could never be observed by anyone else.)
    let db = db_path;
    std::thread::Builder::new()
        .name("ato-passive-observer".to_string())
        .spawn(move || worker::worker_loop(db, &rx))
        .map_err(|e| format!("failed to spawn watcher worker: {}", e))?;

    Ok(Some(ObserverHandle { _watchers: watchers, _tx: tx }))
}

fn on_fs_event(src: &Source, ev: Event, tx: &Sender<ScanRequest>) {
    if !matches!(ev.kind, EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)) {
        return;
    }
    for path in ev.paths {
        if !sources::is_session_file(src.kind, &path) {
            continue;
        }
        let _ = tx.send(ScanRequest { kind: src.kind, path });
    }
}
