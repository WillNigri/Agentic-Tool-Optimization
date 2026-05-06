// v1.3.0 — Embedded terminal (T5).
//
// Spawns interactive PTY sessions backed by `portable-pty` and streams output
// to the frontend via Tauri events `pty://data/<ptyId>` (utf-8 chunks). The
// frontend (xterm.js) writes to stdin via `pty_write` and resizes via
// `pty_resize`. Sessions live in a process-wide HashMap keyed by ptyId; killing
// or dropping the entry frees the OS resources.

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

const DATA_EVENT_PREFIX: &str = "pty://data/";
const EXIT_EVENT_PREFIX: &str = "pty://exit/";

struct PtyHandle {
    /// Writer side of the PTY master. Used by `pty_write`.
    writer: Box<dyn Write + Send>,
    /// Master PTY for resize calls. `Send + Sync` per portable-pty.
    master: Box<dyn portable_pty::MasterPty + Send>,
    /// Owns the spawned child so the PTY isn't garbage-collected.
    /// Boxed `dyn Child + Send + Sync` to allow `kill()` later.
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

#[derive(Default)]
pub struct PtyState(pub Arc<Mutex<HashMap<String, PtyHandle>>>);

impl PtyState {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PtyDataEvent {
    data: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PtyExitEvent {
    pty_id: String,
    code: Option<i32>,
}

/// Spawn an interactive shell PTY. Returns the new ptyId.
///
/// `cwd` — initial working directory; falls back to the user's home if invalid.
/// `shell` — explicit shell path; falls back to `$SHELL` (Unix) / `pwsh.exe` (Windows).
#[tauri::command]
pub async fn pty_spawn(
    app: AppHandle,
    state: State<'_, PtyState>,
    cwd: Option<String>,
    shell: Option<String>,
    rows: Option<u16>,
    cols: Option<u16>,
) -> Result<String, String> {
    let pty_id = uuid::Uuid::new_v4().to_string();
    let pty_system = native_pty_system();

    let pair = pty_system
        .openpty(PtySize {
            rows: rows.unwrap_or(30),
            cols: cols.unwrap_or(100),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty failed: {}", e))?;

    let shell_path = resolve_shell(shell);
    let mut cmd = CommandBuilder::new(&shell_path);

    // Login-shell-ish args so the user's PATH and aliases are picked up.
    if cfg!(unix) && (shell_path.ends_with("zsh") || shell_path.ends_with("bash")) {
        cmd.args(["-l"]);
    }

    let resolved_cwd = match cwd.as_deref() {
        Some(c) if std::path::Path::new(c).is_dir() => c.to_string(),
        _ => dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string()),
    };
    cmd.cwd(&resolved_cwd);

    // Inherit the env vars Tauri picked up at startup. Tauri 2 already spawns
    // a login shell to pick up PATH on macOS; we just need TERM and HOME.
    cmd.env("TERM", "xterm-256color");
    if let Some(h) = dirs::home_dir() {
        cmd.env("HOME", h.to_string_lossy().to_string());
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn failed: {}", e))?;

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take_writer failed: {}", e))?;

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("try_clone_reader failed: {}", e))?;

    {
        let mut map = state.0.lock().map_err(|e| e.to_string())?;
        map.insert(
            pty_id.clone(),
            PtyHandle {
                writer,
                master: pair.master,
                child,
            },
        );
    }

    // Reader thread: stream output as Tauri events. Stops on EOF / read error,
    // then emits the exit event so the frontend can dispose its xterm instance.
    let app_for_thread = app.clone();
    let id_for_thread = pty_id.clone();
    let state_for_thread = state.0.clone();

    std::thread::spawn(move || {
        let event_name = format!("{}{}", DATA_EVENT_PREFIX, id_for_thread);
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buf[..n]).to_string();
                    if let Err(e) = app_for_thread.emit(&event_name, PtyDataEvent { data }) {
                        eprintln!("pty emit failed: {}", e);
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        // Best-effort exit code lookup, then drop the handle.
        let code = {
            let mut map = state_for_thread.lock().ok();
            map.as_mut()
                .and_then(|m| m.get_mut(&id_for_thread))
                .and_then(|h| h.child.try_wait().ok().flatten())
                .map(|s| s.exit_code() as i32)
        };
        let _ = app_for_thread.emit(
            &format!("{}{}", EXIT_EVENT_PREFIX, id_for_thread),
            PtyExitEvent {
                pty_id: id_for_thread.clone(),
                code,
            },
        );
        if let Ok(mut m) = state_for_thread.lock() {
            m.remove(&id_for_thread);
        }
    });

    Ok(pty_id)
}

#[tauri::command]
pub fn pty_write(state: State<'_, PtyState>, pty_id: String, data: String) -> Result<(), String> {
    let mut map = state.0.lock().map_err(|e| e.to_string())?;
    let handle = map
        .get_mut(&pty_id)
        .ok_or_else(|| format!("Unknown ptyId: {}", pty_id))?;
    handle
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| format!("write failed: {}", e))?;
    handle.writer.flush().ok();
    Ok(())
}

#[tauri::command]
pub fn pty_resize(
    state: State<'_, PtyState>,
    pty_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    let map = state.0.lock().map_err(|e| e.to_string())?;
    let handle = map
        .get(&pty_id)
        .ok_or_else(|| format!("Unknown ptyId: {}", pty_id))?;
    handle
        .master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("resize failed: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn pty_kill(state: State<'_, PtyState>, pty_id: String) -> Result<(), String> {
    let mut map = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(mut handle) = map.remove(&pty_id) {
        let _ = handle.child.kill();
        let _ = handle.writer.flush();
    }
    Ok(())
}

#[tauri::command]
pub fn pty_list(state: State<'_, PtyState>) -> Result<Vec<String>, String> {
    let map = state.0.lock().map_err(|e| e.to_string())?;
    Ok(map.keys().cloned().collect())
}

fn resolve_shell(explicit: Option<String>) -> String {
    if let Some(s) = explicit.filter(|s| !s.is_empty()) {
        return s;
    }
    if cfg!(target_os = "windows") {
        std::env::var("COMSPEC").unwrap_or_else(|_| "pwsh.exe".to_string())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_shell_uses_explicit() {
        assert_eq!(resolve_shell(Some("/bin/bash".into())), "/bin/bash");
    }

    #[test]
    fn resolve_shell_ignores_empty_explicit() {
        // Empty string should fall through to env / default.
        let resolved = resolve_shell(Some(String::new()));
        assert!(!resolved.is_empty());
    }
}
