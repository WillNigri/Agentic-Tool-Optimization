// v2.3.52 — Settings → Runtimes → Remote panel backend.
//
// Phase 6.x-J shipped CLI-only remote-runtime management
// (`ato runtimes add-remote / list-remote / remove-remote`). After
// the fesal exchange on 2026-05-13 it became clear that asking users
// to drop into the terminal to wire SSH keys is friction we don't
// need — every other runtime config in ATO is GUI-first.
//
// Same shell-out pattern as sessions_view: the CLI is the canonical
// implementation, the desktop calls it. Avoids re-implementing the
// remote_runtimes table writes here and keeps a single audit path.

use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRuntimeRow {
    pub slug: String,
    pub host: String,
    pub port: i64,
    pub ssh_user: Option<String>,
    pub key_path: Option<String>,
    pub runtime: String,
    pub binary_path: String,
    pub extra_args: Option<String>,
    pub created_at: String,
}

fn resolve_ato_binary() -> Result<String, String> {
    if let Some(p) = crate::commands::which_cli("ato") {
        return Ok(p);
    }
    // See sessions_view::resolve_ato_binary for the rationale on the
    // bare-name fallback. Same pattern; keep it consistent.
    Ok("ato".to_string())
}

#[tauri::command]
pub fn list_remote_runtimes() -> Result<Vec<RemoteRuntimeRow>, String> {
    let bin = resolve_ato_binary()?;
    let out = Command::new(&bin)
        .args(["runtimes", "list-remote"])
        .output()
        .map_err(|e| format!("spawn ato: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(format!("ato runtimes list-remote failed: {}", stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<RemoteRuntimeRow>>(stdout.trim())
        .map_err(|e| format!("parse ato output: {} (raw: {})", e, stdout))
}

#[tauri::command]
pub fn add_remote_runtime(
    name: String,
    host: String,
    runtime: String,
    port: Option<u16>,
    user: Option<String>,
    key_path: Option<String>,
    binary_path: Option<String>,
    extra_args: Option<String>,
) -> Result<(), String> {
    // IPC-boundary validation. The frontend already constrains these
    // fields but a custom Tauri caller could bypass; the CLI surfaces
    // late errors, so catch obvious shape problems here first.
    if name.trim().is_empty() {
        return Err("name is required".into());
    }
    if host.trim().is_empty() {
        return Err("host is required".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err("name must be ASCII alphanumeric / dashes / underscores / dots".into());
    }
    // v2.3.52 — reject names that collide with built-in runtime slugs.
    // If a user named a remote `claude`, every `ato dispatch claude`
    // would silently route to the remote (remote_runtimes is checked
    // before the CLI / api-provider fall-through), breaking local
    // claude. Reserved names match the CLI runtime list + the
    // api-providers registry.
    const RESERVED: &[&str] = &[
        "claude", "codex", "gemini", "hermes", "openclaw",
        "minimax", "grok", "deepseek", "qwen", "openrouter",
    ];
    if RESERVED.contains(&name.trim()) {
        return Err(format!(
            "name '{}' collides with a built-in runtime — pick something like '{}-server' or '{}-prod' instead.",
            name.trim(), name.trim(), name.trim()
        ));
    }
    if let Some(p) = port {
        if !(1..=65535).contains(&p) {
            return Err(format!("port must be 1..=65535 (got {})", p));
        }
    }

    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args([
        "runtimes",
        "add-remote",
        "--name",
        &name,
        "--host",
        &host,
        "--runtime",
        &runtime,
    ]);
    if let Some(p) = port {
        cmd.args(["--port", &p.to_string()]);
    }
    if let Some(u) = &user {
        if !u.trim().is_empty() {
            cmd.args(["--user", u.trim()]);
        }
    }
    if let Some(k) = &key_path {
        if !k.trim().is_empty() {
            cmd.args(["--key-path", k.trim()]);
        }
    }
    if let Some(b) = &binary_path {
        if !b.trim().is_empty() {
            cmd.args(["--binary-path", b.trim()]);
        }
    }
    if let Some(e) = &extra_args {
        if !e.trim().is_empty() {
            cmd.args(["--extra-args", e.trim()]);
        }
    }

    let out = cmd
        .output()
        .map_err(|e| format!("spawn ato: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(format!("ato runtimes add-remote failed: {}", stderr.trim()));
    }
    Ok(())
}

#[tauri::command]
pub fn remove_remote_runtime(name: String) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("name is required".into());
    }
    let bin = resolve_ato_binary()?;
    let out = Command::new(&bin)
        .args(["runtimes", "remove-remote", "--name", &name])
        .output()
        .map_err(|e| format!("spawn ato: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(format!("ato runtimes remove-remote failed: {}", stderr.trim()));
    }
    Ok(())
}

/// Discover candidate SSH private keys under ~/.ssh so the modal can
/// suggest them rather than make the user type a path. Filters to
/// regular files with no .pub extension; permission checks intentionally
/// skipped — if a key isn't readable the spawn error from ssh is the
/// authoritative signal.
#[tauri::command]
pub fn list_ssh_key_candidates() -> Result<Vec<String>, String> {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return Ok(Vec::new()),
    };
    let ssh_dir = std::path::PathBuf::from(&home).join(".ssh");
    let entries = match std::fs::read_dir(&ssh_dir) {
        Ok(e) => e,
        Err(_) => return Ok(Vec::new()),
    };
    let mut keys = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Skip common non-key files in ~/.ssh.
        if name.ends_with(".pub")
            || name == "known_hosts"
            || name == "config"
            || name == "authorized_keys"
            || name.ends_with(".old")
            || name.starts_with('.')
        {
            continue;
        }
        keys.push(path.display().to_string());
    }
    keys.sort();
    Ok(keys)
}
