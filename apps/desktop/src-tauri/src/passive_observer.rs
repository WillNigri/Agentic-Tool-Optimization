// Tauri-state wrapper around the shared `ato-passive-observer` crate.
//
// The watcher logic — source discovery, notify integration, line
// parsing for Claude Code / Codex / Gemini, persistence into
// execution_logs — lives in `packages/ato-passive-observer/`. This
// file exists only to plug that crate into Tauri's State<> lifecycle:
//
//   * `PassiveObserverState` is the per-app handle Tauri holds.
//   * `start()` is idempotent so the `setup` hook can re-fire on hot
//     reload without leaking watchers.
//   * Dropping the handle (e.g. on app exit) stops the OS-level FS
//     watchers and lets the worker thread exit cleanly.
//
// v2.6 PR-A originally landed the watcher inline here. v2.13 lifted
// it to the shared crate so `ato observe` (CLI) can drive the same
// parsers without a Tauri runtime — Hard Rule #6 (CLI + UI parity).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ato_passive_observer::{start_observer, ObserverHandle, SourceKind};

pub struct PassiveObserver {
    started: bool,
    _handle: Option<ObserverHandle>,
}

impl PassiveObserver {
    pub fn new() -> Self {
        Self { started: false, _handle: None }
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    /// Idempotent start across all supported runtimes (Claude Code,
    /// Codex, Gemini). A second call is a no-op so the `setup` hook
    /// can re-fire without leaking watchers.
    pub fn start(&mut self, db_path: PathBuf) -> Result<(), String> {
        if self.started {
            return Ok(());
        }
        // Empty filter = all known runtimes.
        let empty: [SourceKind; 0] = [];
        match start_observer(db_path, &empty) {
            Ok(Some(handle)) => {
                self._handle = Some(handle);
                self.started = true;
                Ok(())
            }
            Ok(None) => {
                // No supported CLI directories present yet — fine.
                // The user may install Claude Code / Codex / Gemini
                // later; today's watcher set stays empty until the
                // next desktop launch. Mark started so we don't retry
                // every setup-hook fire on a pathological machine.
                self.started = true;
                Ok(())
            }
            Err(e) => {
                // Set started before bailing so a re-fired setup hook
                // doesn't re-log every time on HOME=unset machines.
                self.started = true;
                Err(e)
            }
        }
    }
}

pub struct PassiveObserverState(pub Arc<Mutex<PassiveObserver>>);

impl PassiveObserverState {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(PassiveObserver::new())))
    }
}
