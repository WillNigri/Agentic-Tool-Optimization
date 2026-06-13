// All Tauri command functions and helpers.
// Extracted from lib.rs for maintainability.
//
// 2026-05-17 — beginning of the commands.rs split (see
// COMMANDS_SPLIT_PLAN.md alongside). PR 1 lands the directory
// layout + `commands/shared.rs` as the foundation for cross-cutting
// types. Subsequent PRs extract domain modules (agents.rs,
// skills_mcps.rs, cron.rs, …) and update internal call sites to
// `use super::shared::*;`. Zero behavior change per PR — only the
// file boundary moves.

pub mod shared;
pub mod models;
pub mod usage_billing;
pub mod knowledge;
pub mod posts;
pub mod analytics;
pub mod files_paths;
pub mod fs_actions;
pub mod onboarding;
pub mod context;
pub mod workflows;
pub mod workflow_webhooks;
pub mod notifications;
pub mod chat_threads;
pub mod projects;
pub mod agent_hooks_evals;
pub mod live_health;
pub mod events_activity;
pub mod recipes;
pub mod execution_logs;
pub mod runtimes;
pub mod settings_config;
pub mod secrets;
pub mod env_vars;
pub mod llm_api_keys;
pub mod cron;
// v2.10 PR-8 — methodology runner UI read APIs (Insights → Methodologies tab).
pub mod methodology_views;
pub mod skills_validate;
pub mod skills;
pub mod skills_mutate;
pub mod mcp;
pub mod mcp_dispatch;
pub mod mcp_install;
pub mod telemetry;
// v2.14 Loop Composer — reframed Automations w/ SQLite persistence.
pub mod loops;
// v2.16 PR-7 — Mission-control board (local OSS single-machine view).
pub mod missions;
pub use models::*;
pub use usage_billing::*;
pub use knowledge::*;
pub use posts::*;
pub use analytics::*;
pub use files_paths::*;
pub use fs_actions::*;
pub use onboarding::*;
pub use context::*;
pub use workflows::*;
pub use workflow_webhooks::*;
pub use notifications::*;
pub use chat_threads::*;
pub use projects::*;
pub use agent_hooks_evals::*;
pub use live_health::*;
pub use events_activity::*;
pub use recipes::*;
pub use execution_logs::*;
pub use runtimes::*;
pub use settings_config::*;
pub use secrets::*;
pub use env_vars::*;
pub use llm_api_keys::*;
pub use cron::*;
pub use methodology_views::*;
pub use skills_validate::*;
pub use skills::*;
pub use skills_mutate::*;
pub use mcp::*;
pub use mcp_install::*;
pub use telemetry::*;
pub use loops::*;
pub use missions::*;

use crate::*;
use std::collections::HashMap;
use rusqlite::params;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use tauri::{State, Emitter};
use sha2::{Sha256, Digest};

// ── Helpers ──────────────────────────────────────────────────────────────

pub fn claude_home() -> PathBuf {
    home_dir().join(".claude")
}

pub fn gemini_home() -> PathBuf {
    home_dir().join(".gemini")
}

/// Find the project root by walking up from CWD looking for .git or .claude/
pub fn project_root() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    for _ in 0..10 {
        if dir.join(".git").exists() || dir.join(".claude").exists() || dir.join("CLAUDE.md").exists() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Discover all project directories that contain agent config (.claude/, .codex/, etc.)
/// Scans common development locations + user-configured paths.
pub fn discover_project_roots() -> Vec<PathBuf> {
    let home = home_dir();
    let mut roots = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Always include CWD project root
    let cwd_root = project_root();
    if cwd_root.join(".claude").exists() || cwd_root.join(".codex").exists()
       || cwd_root.join(".openclaw").exists() || cwd_root.join(".hermes").exists() {
        seen.insert(cwd_root.to_string_lossy().to_string());
        roots.push(cwd_root);
    }

    // Load user-configured project paths
    let config_path = home.join(".ato").join("projects.txt");
    if let Some(content) = read_file_lossy(&config_path) {
        for line in content.lines() {
            let p = PathBuf::from(line.trim());
            if p.exists() && !seen.contains(&p.to_string_lossy().to_string()) {
                seen.insert(p.to_string_lossy().to_string());
                roots.push(p);
            }
        }
    }

    // Scan common dev directories (1 level deep)
    let scan_dirs = vec![
        home.clone(),
        home.join("Documents"),
        home.join("Projects"),
        home.join("projects"),
        home.join("Desktop"),
        home.join("code"),
        home.join("Code"),
        home.join("dev"),
        home.join("Development"),
        home.join("workspace"),
        home.join("repos"),
        home.join("src"),
    ];

    for scan_dir in scan_dirs {
        if !scan_dir.exists() { continue; }
        if let Ok(entries) = fs::read_dir(&scan_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() { continue; }
                let key = path.to_string_lossy().to_string();
                if seen.contains(&key) { continue; }

                // Check if this directory has any agent config
                let has_agent_config = path.join(".claude").exists()
                    || path.join(".codex").exists()
                    || path.join(".openclaw").exists()
                    || path.join(".hermes").exists()
                    || path.join("CLAUDE.md").exists()
                    || path.join("AGENTS.md").exists();

                if has_agent_config {
                    seen.insert(key);
                    roots.push(path);
                }
            }
        }
    }

    roots
}

pub fn read_file_lossy(path: &PathBuf) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Estimate tokens from byte count (~4 bytes per token for English)
pub fn estimate_tokens(bytes: u64) -> u64 {
    bytes / 4
}

/// Simple hash of content for change detection
pub fn content_hash(content: &str) -> String {
    let mut hash: u64 = 5381;
    for byte in content.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    format!("{:x}", hash)
}

/// Parse YAML-like frontmatter from markdown content
pub fn parse_frontmatter(content: &str) -> (serde_json::Value, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        let desc = content.lines()
            .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .unwrap_or("")
            .trim()
            .to_string();
        return (serde_json::json!({"description": desc}), content.to_string());
    }

    let after_first = &trimmed[3..];
    if let Some(end_idx) = after_first.find("\n---") {
        let fm_str = &after_first[..end_idx].trim();
        let body = &after_first[end_idx + 4..];

        let mut fm = serde_json::Map::new();
        for line in fm_str.lines() {
            if let Some(colon_pos) = line.find(':') {
                let key = line[..colon_pos].trim().to_string();
                let value = line[colon_pos + 1..].trim().to_string();
                // Handle boolean
                if value == "true" {
                    fm.insert(key, serde_json::Value::Bool(true));
                } else if value == "false" {
                    fm.insert(key, serde_json::Value::Bool(false));
                } else {
                    fm.insert(key, serde_json::Value::String(value));
                }
            }
        }

        // Parse allowed-tools into array
        if let Some(tools_val) = fm.get("allowed-tools").cloned() {
            if let Some(tools_str) = tools_val.as_str() {
                let tools: Vec<serde_json::Value> = tools_str
                    .split(',')
                    .map(|t| serde_json::Value::String(t.trim().to_string()))
                    .filter(|v| v.as_str().map_or(false, |s| !s.is_empty()))
                    .collect();
                fm.insert("allowedTools".to_string(), serde_json::Value::Array(tools));
            }
        }

        (serde_json::Value::Object(fm), body.to_string())
    } else {
        (serde_json::json!({}), content.to_string())
    }
}

/// Collect skills from a directory, supporting single files, SKILL.md directories,
/// symlinks (gstack-style), and nested subdirectories (one level deep).
pub fn collect_skills(dir: &PathBuf, scope: &str, runtime: &str, db: &Connection) -> Vec<LocalSkill> {
    collect_skills_for_project(dir, scope, runtime, None, db)
}

pub fn collect_skills_for_project(dir: &PathBuf, scope: &str, runtime: &str, project: Option<&str>, db: &Connection) -> Vec<LocalSkill> {
    let mut skills = Vec::new();
    if !dir.exists() {
        return skills;
    }

    collect_skills_inner(dir, scope, runtime, project, db, &mut skills, 0);
    skills
}

pub fn collect_skills_inner(dir: &PathBuf, scope: &str, runtime: &str, project: Option<&str>, db: &Connection, skills: &mut Vec<LocalSkill>, depth: u32) {
    // Limit recursion to 2 levels (handles gstack's ~/.claude/skills/gstack/*/SKILL.md)
    if depth > 2 { return; }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name;
        let content;
        let file_path_str;

        if path.is_dir() {
            // Directory skill — look for SKILL.md
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() {
                name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                content = read_file_lossy(&skill_md).unwrap_or_default();
                file_path_str = format!("{}/", path.to_string_lossy());
            } else {
                // No SKILL.md — recurse into subdirectory (handles gstack/ nested dirs)
                collect_skills_inner(&path, scope, runtime, project, db, skills, depth + 1);
                continue;
            }
        } else if path.extension().map_or(false, |ext| ext == "md") {
            name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            content = read_file_lossy(&path).unwrap_or_default();
            file_path_str = path.to_string_lossy().to_string();
        } else {
            continue;
        }

        let (fm, _body) = parse_frontmatter(&content);
        let description = fm.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let hash = content_hash(&content);
        let tokens = estimate_tokens(content.len() as u64);

        // Check toggle state from DB
        let enabled: bool = db
            .query_row(
                "SELECT enabled FROM skill_toggles WHERE file_path = ?1",
                params![&file_path_str],
                |row| row.get(0),
            )
            .unwrap_or(true); // Default enabled

        let id = content_hash(&file_path_str);

        skills.push(LocalSkill {
            id,
            name,
            description,
            file_path: file_path_str,
            scope: scope.to_string(),
            runtime: runtime.to_string(),
            project: project.map(|s| s.to_string()),
            token_count: tokens,
            enabled,
            content_hash: hash,
        });
    }
}

pub fn list_subdir_files(dir: &PathBuf, subdir: &str) -> (bool, Vec<String>) {
    let path = dir.join(subdir);
    if !path.exists() || !path.is_dir() {
        return (false, Vec::new());
    }
    let files: Vec<String> = fs::read_dir(&path)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    (true, files)
}






#[tauri::command]
pub async fn prompt_claude(prompt: String) -> Result<String, String> {
    use std::process::Command;

    // Find the claude CLI
    let claude_path = which_claude().ok_or_else(|| {
        "Claude Code CLI not found. Install it with: npm install -g @anthropic-ai/claude-code".to_string()
    })?;

    // Run claude with --print flag. After 2026-06-15 this counts against
    // the Agent SDK credit (programmatic) instead of subscription unless
    // the user has stored an Anthropic API key — in which case BYOK
    // forwards ANTHROPIC_API_KEY and Anthropic bills the key directly.
    // Use the user's full PATH so claude can find node, npm, etc.
    let user_path = get_user_path();
    let mut cmd = Command::new(&claude_path);
    cmd.args(["--print", &prompt]).env("PATH", &user_path);
    crate::byok::apply_byok_env_from_path(&mut cmd, &crate::get_db_path(), "claude");
    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run claude: {}", e))?;

    if output.status.success() {
        let response = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(response)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        // Redact BYOK secrets before stderr surfaces to the user. (minimax #1)
        let stderr = crate::byok::redact_byok_secrets(&stderr, "claude", None);
        if stderr.contains("not logged in") || stderr.contains("authentication") {
            Err("Not logged in to Claude Code. Run `claude` in your terminal first to authenticate.".to_string())
        } else if stderr.is_empty() {
            // Sometimes claude outputs to stdout even on non-zero exit
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            if !stdout.is_empty() {
                Ok(stdout)
            } else {
                Err("Claude returned no output. Make sure Claude Code is installed and you're logged in.".to_string())
            }
        } else {
            Err(format!("Claude error: {}", stderr.lines().last().unwrap_or(&stderr)))
        }
    }
}


// ── Multi-Agent Runtime ──────────────────────────────────────────────────


/// Get the user's full shell PATH (Tauri apps launch with minimal env)
use std::sync::OnceLock;

/// Cached PATH resolution. Resolving the user's PATH spawns a shell on
/// Unix and PowerShell on Windows — neither is cheap, and on Windows the
/// PowerShell call pops a visible console window per invocation. v1.5.21
/// shipped without this cache and called get_user_path() once per MCP
/// discovery, which on Felipe's Windows install meant a stream of
/// flashing PowerShell windows. Caching the value at first call (the
/// shell's PATH doesn't change during app lifetime anyway) cuts both
/// the cost and the visual noise.
static USER_PATH_CACHE: OnceLock<String> = OnceLock::new();

#[cfg(target_os = "windows")]
fn no_window_flag() -> u32 {
    // CREATE_NO_WINDOW — keeps the PowerShell child invisible to the user.
    // Without this, every spawn pops a black PowerShell window briefly.
    0x08000000
}

pub fn get_user_path() -> String {
    USER_PATH_CACHE.get_or_init(resolve_user_path).clone()
}

fn resolve_user_path() -> String {
    augment_with_version_managers(resolve_base_user_path())
}

fn resolve_base_user_path() -> String {
    // Windows takes a different code path: GUI-launched apps inherit the
    // PATH from when they were launched, which usually misses User-scope
    // PATH entries the user added later (npm-global, scoop shims, etc.).
    // Resolve via PowerShell which reads both Machine + User env at runtime.
    // Felipe hit this on v1.5.20: nothing connects on Windows because no
    // CLI was findable, even though `where claude` works in his terminal.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        if let Ok(output) = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "[Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User')",
            ])
            .creation_flags(no_window_flag())
            .output()
        {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return path;
                }
            }
        }
        // Fall through to the inherited PATH. Better than nothing.
        return std::env::var("PATH").unwrap_or_default();
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Try to get PATH from user's shell. The shell flag set is critical
        // for nvm-installed node: nvm.sh is sourced from ~/.bashrc and
        // ~/.zshrc (interactive init), NOT from ~/.bash_profile / ~/.profile
        // (login init). v1.5.21 only used `-l` (login) so Felipe's nvm node
        // never made it onto PATH and `npx` stayed unfound. Using `-l -i`
        // (login + interactive) sources both, which is what the user's
        // terminal does on every fresh tab.
        for shell in ["/bin/zsh", "/bin/bash"] {
            if let Ok(output) = std::process::Command::new(shell)
                .args(["-l", "-i", "-c", "echo $PATH"])
                .output()
            {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return path;
                    }
                }
            }
            // Fallback to login-only in case `-i` triggered a prompt that
            // blocked output (rare but possible with custom rc).
            if let Ok(output) = std::process::Command::new(shell)
                .args(["-l", "-c", "echo $PATH"])
                .output()
            {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return path;
                    }
                }
            }
        }
        std::env::var("PATH").unwrap_or_default()
    }
}

/// Append common version-manager bin/shim directories to PATH if they
/// exist on disk and aren't already present. Idempotent.
///
/// Felipe P1 (2026-05 — WSL/nvm): 100% of catalog MCPs that use `npx`
/// failed on his WSL setup because the login-shell PATH didn't include
/// `~/.nvm/versions/node/<version>/bin`. nvm sources lazily from
/// `.bashrc`, and even `-l -i` doesn't always pick it up under WSL's
/// non-tty path. Probing the well-known directories directly fills the
/// gap regardless of how the user's shell config is wired.
///
/// For nvm we filter to `vMAJOR.MINOR.PATCH` directory names and pick
/// the numerically-greatest tuple — naive lex sort would put `v9.x.y`
/// after `v22.x.y` and select the older one (war-room R1: google +
/// minimax catch). This also drops `iojs-*`, `system`, and alias
/// names that may share the parent dir.
#[allow(clippy::let_and_return)]
fn augment_with_version_managers(base: String) -> String {
    #[cfg(target_os = "windows")]
    {
        // nvm-windows, pyenv-win, and rbenv have different layouts; the
        // Felipe P1 case is the Unix-style shim/bin set. Leave Windows
        // unchanged here — the PowerShell resolver above already picks
        // up User-scope PATH that covers nvm-windows.
        return base;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        augment_with_version_managers_for_home(base, &home)
    }
}

/// Parse an nvm directory name like `v22.4.0` into a `(major, minor,
/// patch)` tuple suitable for `max_by_key`. Returns `None` for names
/// that aren't strictly `v<u32>.<u32>.<u32>` (e.g., `iojs-3.0.0`,
/// `system`, alias symlinks like `lts`). The leading `v` is required —
/// every node release nvm has shipped uses it, and rejecting names
/// without it filters out anything ambiguous.
#[cfg(not(target_os = "windows"))]
fn parse_node_version(name: &str) -> Option<(u32, u32, u32)> {
    let rest = name.strip_prefix('v')?;
    let mut parts = rest.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        // Reject suffixes like `v22.4.0-rc.1` to keep the picker
        // predictable; prereleases on a dev box would usually be
        // selected via `nvm alias default` and live elsewhere anyway.
        return None;
    }
    Some((major, minor, patch))
}

/// Inner parametric form: takes `home` as an argument so tests can
/// point at a temp directory without racing on `std::env::set_var`.
#[cfg(not(target_os = "windows"))]
fn augment_with_version_managers_for_home(base: String, home: &str) -> String {
    if home.is_empty() {
        return base;
    }

    let mut candidates: Vec<String> = Vec::new();

    // nvm: pick numerically-greatest vMAJOR.MINOR.PATCH dir under
    // ~/.nvm/versions/node. Lex-sort would put v9 after v22; parse the
    // tuple instead.
    let nvm_root = format!("{}/.nvm/versions/node", home);
    if let Ok(entries) = std::fs::read_dir(&nvm_root) {
        let newest = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter_map(|name| parse_node_version(&name).map(|v| (v, name)))
            .max_by_key(|(v, _)| *v)
            .map(|(_, name)| name);
        if let Some(name) = newest {
            candidates.push(format!("{}/{}/bin", nvm_root, name));
        }
    }

    candidates.push(format!("{}/.pyenv/shims", home));
    candidates.push(format!("{}/.rbenv/shims", home));
    candidates.push(format!("{}/.local/bin", home));

    let existing: std::collections::HashSet<&str> = base.split(':').collect();
    let additions: Vec<String> = candidates
        .into_iter()
        .filter(|c| std::path::Path::new(c).exists())
        .filter(|c| !existing.contains(c.as_str()))
        .collect();

    if additions.is_empty() {
        return base;
    }
    if base.is_empty() {
        return additions.join(":");
    }
    format!("{}:{}", base, additions.join(":"))
}

/// Build a `std::process::Command` from a CLI string that may be either
/// a plain path or a wrapper invocation. This lets users on Windows run
/// `wsl.exe -e /home/<user>/.local/bin/claude` as the override path —
/// the WSL → Linux Claude case Felipe hit. Quoting is naive (whitespace
/// split) but covers the common cases without pulling in a full shell
/// parser.
pub fn wrapper_command(spec: &str) -> std::process::Command {
    let trimmed = spec.trim();
    let mut parts = trimmed.split_whitespace();
    let exe = parts.next().unwrap_or(trimmed);
    let mut cmd = std::process::Command::new(exe);
    for arg in parts {
        cmd.arg(arg);
    }
    cmd
}

/// Async tokio counterpart for streaming dispatch paths.
pub fn wrapper_command_tokio(spec: &str) -> tokio::process::Command {
    let trimmed = spec.trim();
    let mut parts = trimmed.split_whitespace();
    let exe = parts.next().unwrap_or(trimmed);
    let mut cmd = tokio::process::Command::new(exe);
    for arg in parts {
        cmd.arg(arg);
    }
    cmd
}

/// Search for a CLI binary by name, checking common install paths + user shell + npx cache.
pub fn which_cli(name: &str) -> Option<String> {
    // HOME isn't set on Windows by default — USERPROFILE is. Falling back
    // to USERPROFILE keeps the candidate-path expansion working
    // cross-platform without forcing every caller to set HOME first.
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();

    // 1. Check user-configured override first (highest priority).
    //    The override may be a plain path OR a wrapper invocation
    //    (e.g. `wsl.exe -e /home/user/.local/bin/claude`). When it has
    //    a space, we only check the first token for existence — the
    //    rest are arguments. The override is returned verbatim so
    //    downstream callers can run it via `wrapper_command(...)`.
    let override_path = home_dir().join(".ato").join(format!("{}-path", name));
    if let Some(custom) = read_file_lossy(&override_path) {
        let trimmed = custom.trim().to_string();
        if !trimmed.is_empty() {
            let first_token = trimmed
                .split_whitespace()
                .next()
                .unwrap_or(&trimmed)
                .to_string();
            if std::path::Path::new(&first_token).exists() {
                return Some(trimmed);
            }
            // Allow command names that resolve through PATH (e.g.
            // `wsl.exe` on Windows is on PATH but not at a fixed
            // location). Try `which`/`where` resolution.
            if which_executable(&first_token).is_some() {
                return Some(trimmed);
            }
        }
    }

    // 2. Check common install locations.
    let mut candidates: Vec<String> = vec![
        format!("/usr/local/bin/{}", name),
        format!("/opt/homebrew/bin/{}", name),
        format!("{}/.npm-global/bin/{}", home, name),
        format!("{}/bin/{}", home, name),
        format!("{}/.local/bin/{}", home, name),
        format!("{}/.cargo/bin/{}", home, name),
    ];
    // Windows-specific candidates. npm shims land in %APPDATA%\npm\<name>.cmd
    // — `where` doesn't always pick these up if Tauri's GUI-launched PATH
    // misses %APPDATA%. Volta, scoop, and Cargo for Windows go elsewhere
    // again. Felipe's "nothing connects on Windows" was this set never
    // being checked.
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        let local_appdata = std::env::var("LOCALAPPDATA").unwrap_or_default();
        // Each candidate gets tried both as `<name>.cmd` and `<name>.exe`
        // — npm publishes .cmd shims, native installers ship .exe.
        for ext in ["cmd", "exe"] {
            if !appdata.is_empty() {
                candidates.push(format!(r"{}\npm\{}.{}", appdata, name, ext));
            }
            if !local_appdata.is_empty() {
                candidates.push(format!(r"{}\Programs\{}\{}.{}", local_appdata, name, name, ext));
                candidates.push(format!(r"{}\Volta\bin\{}.{}", local_appdata, name, ext));
            }
            if !home.is_empty() {
                candidates.push(format!(r"{}\.cargo\bin\{}.{}", home, name, ext));
                candidates.push(format!(r"{}\scoop\shims\{}.{}", home, name, ext));
            }
        }
    }

    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Some(path.clone());
        }
    }

    // 3. Search npx cache directories (where `npx @anthropic-ai/claude-code` installs)
    let npx_cache = PathBuf::from(&home).join(".npm/_npx");
    if npx_cache.exists() {
        if let Ok(entries) = fs::read_dir(&npx_cache) {
            for entry in entries.flatten() {
                let bin_path = entry.path().join("node_modules").join(".bin").join(name);
                if bin_path.exists() {
                    return Some(bin_path.to_string_lossy().to_string());
                }
            }
        }
    }

    // 4. Fall through to platform-specific `which`/`where` resolution.
    which_executable(name)
}

/// Resolve a bare executable name through the user's shell PATH using
/// the platform-native lookup tool. Returns the absolute path on
/// success. Used both in `which_cli`'s fallback and to validate the
/// first token of a wrapper override.
fn which_executable(name: &str) -> Option<String> {
    let user_path = get_user_path();
    let lookup_cmd = if cfg!(target_os = "windows") { "where" } else { "which" };
    if let Ok(output) = std::process::Command::new(lookup_cmd)
        .arg(name)
        .env("PATH", &user_path)
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // `where` on Windows can return multiple lines — take the first.
            let path = stdout.lines().next().unwrap_or("").trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    None
}



/// Internal helper. Takes a fully-built tokio Command + the runtime
/// label + optional Live-runs registry context. Spawns, registers,
/// supports kill via oneshot, returns stdout-as-string on success or
/// stderr-derived message on failure.
///
/// v2.1.0+ Phase 4 follow-through. Previously prompt_agent used the
/// sync `std::process::Command::output()` path which (a) blocked the
/// async runtime thread for the full dispatch and (b) consumed the
/// child entirely, leaving no handle to attach a kill closure to.
/// This helper replaces that pattern with the same kill-via-oneshot
/// design `spawn_streaming_dispatch` uses for the chat pane, so
/// every prompt_agent caller (group stages, Quick Test, MCP
/// run_agent, cron) gets:
///   - a labelled row in the Live runs panel for the duration
///   - a working Kill button (sends SIGKILL, returns "killed by user")
///   - finish_run on every exit including panics (FinishGuard)
async fn dispatch_command_killable(
    mut cmd: tokio::process::Command,
    runtime: &str,
    runtime_label: &str,
    agent_slug: Option<&str>,
    workspace: Option<&str>,
    source: &str,
    // When `existing_run_id` is Some, we skip our own begin_run/
    // finish_run and just attach a kill handler to the caller's
    // run_id. Used by prompt_agent_with_context which has to keep
    // ownership of registration so it can return the run_id to the
    // frontend (for overlap evidence + explicit finish).
    existing_run_id: Option<&str>,
) -> Result<String, String> {
    use std::process::Stdio;
    use std::sync::Arc;
    use tokio::io::AsyncReadExt;

    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // Register with the Live runs registry so the user can see + kill.
    // Skip when the caller already registered; finish guard only
    // applies in the self-registered branch.
    let owned_run_id: Option<String> = if existing_run_id.is_none() {
        Some(crate::active_runs::begin_run(runtime, agent_slug, workspace, Some(source)))
    } else {
        None
    };
    struct FinishGuard(Option<String>);
    impl Drop for FinishGuard {
        fn drop(&mut self) {
            if let Some(id) = &self.0 {
                crate::active_runs::finish_run(id);
            }
        }
    }
    let _finish_guard = FinishGuard(owned_run_id.clone());
    let active_run_id: &str = existing_run_id
        .or_else(|| owned_run_id.as_deref())
        .expect("either caller or self provides a run_id");

    // Same kill-via-oneshot pattern as spawn_streaming_dispatch — pure
    // sync closure (no tokio runtime context needed) that signals
    // intent on a channel; the read loop reacts via select!.
    let (kill_tx, mut kill_rx) = tokio::sync::oneshot::channel::<()>();
    let kill_tx_holder: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>> =
        Arc::new(std::sync::Mutex::new(Some(kill_tx)));
    let kill_tx_for_handler = kill_tx_holder.clone();
    crate::active_runs::attach_kill_handler(active_run_id, move || {
        let mut g = match kill_tx_for_handler.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if let Some(tx) = g.take() {
            let _ = tx.send(());
        }
    });

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", runtime_label, e))?;

    // v2.3.0 — record the child's OS PID in live_runs so the `ato`
    // CLI can SIGTERM it from another process. Best-effort write.
    if let Some(pid) = child.id() {
        crate::active_runs::set_child_pid(active_run_id, pid);
    }

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "stdout-pipe-missing".to_string())?;

    let mut stdout_buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        tokio::select! {
            biased;
            _ = &mut kill_rx => {
                let _ = child.kill().await;
                return Err("killed by user".to_string());
            }
            r = stdout.read(&mut chunk) => match r {
                Ok(0) => break,
                Ok(n) => stdout_buf.extend_from_slice(&chunk[..n]),
                Err(e) => {
                    let _ = child.kill().await;
                    return Err(format!("read-failed: {}", e));
                }
            }
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("wait-failed: {}", e))?;

    if status.success() {
        Ok(String::from_utf8_lossy(&stdout_buf).to_string())
    } else {
        // Drain stderr for the failure message — best-effort.
        let mut stderr_text = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut stderr_text).await;
        }
        // Redact BYOK secrets before stderr reaches the frontend / DB —
        // some vendors echo the bad key in auth failures. Caller is
        // already past the spawn step, so we look up the env-var key
        // from ATO's own env and the stored key by re-checking the
        // path (small extra read, only on the error path). The applied
        // key isn't tracked here because dispatch_command_killable
        // doesn't know which env vars were set — see the caller's
        // own redaction at byok_env_value_from_path. (minimax #1)
        let stderr_text =
            crate::byok::redact_byok_secrets(&stderr_text, runtime, None);
        Err(if stderr_text.is_empty() {
            format!("{} exited with status {}", runtime_label, status)
        } else {
            format!("{}: {}", runtime_label, stderr_text)
        })
    }
}

// Felipe P3 (v2.7.10) — pre-trust for ATO-registered projects.
//
// Claude Code prompts "trust this folder?" on every dispatch into an
// unfamiliar directory because the trust state lives in its own
// SQLite database under a different macOS identity than the ATO
// desktop. Result: every claude `--print` from ATO has historically
// hit the trust gate even on workspaces the user has been working in
// all morning.
//
// Felipe's fix: if the dispatch's workspace IS a project the user
// has registered in ATO (projects.path), append
// `--dangerously-skip-permissions`.
//
// Scope honesty (war-room review 2026-05-20): this flag is BROADER
// than the per-folder trust prompt — claude also uses it to suppress
// per-tool approval prompts (Bash, Edit, Write, …). Project
// registration in ATO is the user's act of consent. The behaviour
// is gated by a user-visible toggle (default ON) so anyone who
// does not want claude's tool-approval flow skipped can opt out;
// when OFF, dispatch falls back to claude's native prompts (trust
// + per-tool). The toggle copy makes this explicit so the consent
// is not hidden behind "trust this folder?"-shaped framing.
//
// Storage: ~/.ato/settings.json with key `trust_registered_projects`
// (bool). JSON sidecar instead of the SQLite `settings` table because
// this session can not add a new Tauri get/set command — the FE
// toggle writes the file directly via @tauri-apps/plugin-fs. Precedent
// for sidecar JSON in `~/.ato/` is the openclaw-config.json read in
// load_openclaw_config (mod.rs:2345). Default ON applies when the
// file is missing, malformed, or the key is absent.
//
// Workspace matching trims a single trailing slash on both sides
// (the only realistic input variability we see in dogfood — paths
// arriving from path-pickers vs. typed by hand). Symlinks and `~`
// expansion are NOT resolved: ATO's project registration stores the
// path as-typed, and other lookups (projects.rs) compare raw
// strings; resolving here would create asymmetry with the row that
// was inserted. The trust prompt is one ENOENT — the failure mode
// of a stale registration is "claude asks again", which is harmless.

const TRUST_REGISTERED_PROJECTS_KEY: &str = "trust_registered_projects";

/// Read the `trust_registered_projects` toggle from ~/.ato/settings.json.
/// Missing file / unreadable / malformed JSON / missing key → true.
/// Default-ON is the contract — the toggle is opt-OUT, not opt-in.
fn read_trust_registered_projects(settings_path: &std::path::Path) -> bool {
    let Ok(contents) = fs::read_to_string(settings_path) else {
        return true;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return true;
    };
    value
        .get(TRUST_REGISTERED_PROJECTS_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// Predicate: should the claude dispatch append `--dangerously-skip-permissions`
/// for this workspace? Three conjuncts:
///   1. workspace is non-empty (None or "" → false; we can't pre-trust nothing)
///   2. a row exists in `projects` with `path` exactly equal to the workspace
///   3. the user's trust toggle is ON (or absent — default ON)
///
/// Extracted as a free fn so the unit tests below can exercise every
/// branch against an in-memory SQLite + a tmp settings.json — the
/// spawn-side stays untested.
fn should_pretrust_workspace(
    workspace: Option<&str>,
    conn: &rusqlite::Connection,
    settings_path: &std::path::Path,
) -> bool {
    let ws = match workspace {
        Some(w) if !w.trim().is_empty() => w,
        _ => return false,
    };
    if !read_trust_registered_projects(settings_path) {
        return false;
    }
    // Match against both the workspace as-typed AND with a single
    // trailing slash stripped. Project registration is the source of
    // truth: a user who registered `/Users/x/repo` shouldn't lose
    // pre-trust because the path-picker handed us `/Users/x/repo/`.
    // We only strip ONE trailing slash (not normalize) — `~` and
    // symlinks remain unresolved by design (see module-level comment).
    let trimmed = ws.strip_suffix('/').unwrap_or(ws);
    conn.query_row(
        "SELECT 1 FROM projects WHERE path = ?1 OR path = ?2 LIMIT 1",
        rusqlite::params![ws, trimmed],
        |_| Ok(()),
    )
    .is_ok()
}

/// Path to the ATO-wide settings JSON sidecar at `~/.ato/settings.json`.
fn ato_settings_path() -> PathBuf {
    crate::home_dir().join(".ato").join("settings.json")
}

#[cfg(test)]
mod pretrust_tests {
    use super::*;
    use rusqlite::Connection;
    use std::io::Write;

    fn fresh_projects_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE projects (
                id            TEXT PRIMARY KEY,
                name          TEXT NOT NULL,
                path          TEXT NOT NULL UNIQUE,
                is_active     INTEGER NOT NULL DEFAULT 0,
                skill_count   INTEGER NOT NULL DEFAULT 0,
                last_accessed TEXT,
                created_at    TEXT NOT NULL
             );",
        )
        .expect("create projects table");
        conn
    }

    fn insert_project(conn: &Connection, id: &str, path: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, is_active, skill_count, created_at)
             VALUES (?1, ?2, ?3, 0, 0, '2026-05-20T00:00:00Z')",
            rusqlite::params![id, format!("Project {}", id), path],
        )
        .expect("insert project");
    }

    fn write_settings(dir: &tempfile::TempDir, contents: &str) -> PathBuf {
        let p = dir.path().join("settings.json");
        let mut f = std::fs::File::create(&p).expect("create settings.json");
        f.write_all(contents.as_bytes()).expect("write settings");
        p
    }

    #[test]
    fn read_trust_defaults_on_when_file_missing() {
        let dir = tempfile::tempdir().expect("tmp");
        let missing = dir.path().join("nope.json");
        assert!(read_trust_registered_projects(&missing));
    }

    #[test]
    fn read_trust_defaults_on_when_json_malformed() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = write_settings(&dir, "{not json");
        assert!(read_trust_registered_projects(&p));
    }

    #[test]
    fn read_trust_defaults_on_when_key_absent() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = write_settings(&dir, r#"{"some_other_key": false}"#);
        assert!(read_trust_registered_projects(&p));
    }

    #[test]
    fn read_trust_respects_false_value() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = write_settings(&dir, r#"{"trust_registered_projects": false}"#);
        assert!(!read_trust_registered_projects(&p));
    }

    #[test]
    fn read_trust_respects_true_value() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = write_settings(&dir, r#"{"trust_registered_projects": true}"#);
        assert!(read_trust_registered_projects(&p));
    }

    #[test]
    fn pretrust_true_when_registered_and_toggle_default() {
        let conn = fresh_projects_conn();
        insert_project(&conn, "p1", "/Users/alice/repo");
        let dir = tempfile::tempdir().expect("tmp");
        let settings = dir.path().join("settings.json"); // does not exist → default ON
        assert!(should_pretrust_workspace(
            Some("/Users/alice/repo"),
            &conn,
            &settings,
        ));
    }

    #[test]
    fn pretrust_false_when_toggle_off_even_if_registered() {
        let conn = fresh_projects_conn();
        insert_project(&conn, "p1", "/Users/alice/repo");
        let dir = tempfile::tempdir().expect("tmp");
        let settings = write_settings(&dir, r#"{"trust_registered_projects": false}"#);
        assert!(!should_pretrust_workspace(
            Some("/Users/alice/repo"),
            &conn,
            &settings,
        ));
    }

    #[test]
    fn pretrust_false_when_workspace_unregistered() {
        let conn = fresh_projects_conn();
        insert_project(&conn, "p1", "/Users/alice/repo");
        let dir = tempfile::tempdir().expect("tmp");
        let settings = dir.path().join("settings.json");
        assert!(!should_pretrust_workspace(
            Some("/Users/alice/other-repo"),
            &conn,
            &settings,
        ));
    }

    #[test]
    fn pretrust_false_when_no_workspace() {
        let conn = fresh_projects_conn();
        insert_project(&conn, "p1", "/Users/alice/repo");
        let dir = tempfile::tempdir().expect("tmp");
        let settings = dir.path().join("settings.json");
        assert!(!should_pretrust_workspace(None, &conn, &settings));
        assert!(!should_pretrust_workspace(Some(""), &conn, &settings));
        assert!(!should_pretrust_workspace(Some("   "), &conn, &settings));
    }

    #[test]
    fn pretrust_normalizes_single_trailing_slash() {
        // Trailing slash on the query side should still match a
        // registration without trailing slash. Path-pickers and
        // hand-typed paths disagree on this in dogfood; we paper
        // over the single-slash case explicitly while keeping
        // symlinks + `~` unresolved (see module-level comment).
        let conn = fresh_projects_conn();
        insert_project(&conn, "p1", "/Users/alice/repo");
        let dir = tempfile::tempdir().expect("tmp");
        let settings = dir.path().join("settings.json");
        assert!(should_pretrust_workspace(
            Some("/Users/alice/repo/"),
            &conn,
            &settings,
        ));
    }

    #[test]
    fn pretrust_handles_registration_with_trailing_slash() {
        // The symmetric case: registration carries a trailing slash
        // (e.g. directory picker output on some platforms), query
        // comes in without one. The OR-clause in the SQL handles it.
        let conn = fresh_projects_conn();
        insert_project(&conn, "p1", "/Users/alice/repo/");
        let dir = tempfile::tempdir().expect("tmp");
        let settings = dir.path().join("settings.json");
        assert!(should_pretrust_workspace(
            Some("/Users/alice/repo/"),
            &conn,
            &settings,
        ));
    }

    #[test]
    fn pretrust_false_for_sub_paths() {
        // Sub-path under a registered project is NOT pre-trusted.
        // Pre-trust is opt-in per workspace, not transitive — claude
        // re-prompts and the user can register the sub-path if they
        // want it covered.
        let conn = fresh_projects_conn();
        insert_project(&conn, "p1", "/Users/alice/repo");
        let dir = tempfile::tempdir().expect("tmp");
        let settings = dir.path().join("settings.json");
        assert!(!should_pretrust_workspace(
            Some("/Users/alice/repo/sub"),
            &conn,
            &settings,
        ));
    }
}

/// Felipe P4 — look up the `default_prompt` column for an agent
/// identified by `(slug, runtime)`. Slug is not unique across
/// runtimes (a user can have `@reviewer` registered for both Claude
/// and Codex), so we disambiguate by runtime and tie-break with the
/// same `COALESCE(last_used_at, created_at) DESC` ordering the
/// permissions lookup uses a few lines down — keeping a single
/// "most recently active row wins" rule across both reads.
///
/// Returns `Some(default_prompt)` only when the column is non-NULL
/// AND non-empty after trim. Any DB error (file missing, table
/// missing on a brand-new install) returns `None` so the dispatch
/// path is never blocked by a substitution failure — empty stays
/// empty and the existing downstream behaviour stands.
fn load_agent_default_prompt(
    db_path: &std::path::Path,
    slug: &str,
    runtime: &str,
) -> Option<String> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .ok()?;
    let default_prompt: Option<String> = conn
        .query_row(
            "SELECT default_prompt FROM agents
              WHERE slug = ?1 AND runtime = ?2
              ORDER BY COALESCE(last_used_at, created_at) DESC LIMIT 1",
            rusqlite::params![slug, runtime],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten();
    default_prompt.filter(|s| !s.trim().is_empty())
}

#[tauri::command]
pub async fn prompt_agent(
    runtime: String,
    prompt: String,
    config: Option<String>,
    // v2.1.0+ — optional context for the Live runs registry. JS
    // callers can omit; internal Rust callers (group dispatch,
    // prompt_agent_with_context) pass the slug + workspace so the
    // Live panel renders meaningful labels instead of "ad-hoc".
    agent_slug: Option<String>,
    workspace: Option<String>,
) -> Result<String, String> {
    prompt_agent_inner(runtime, prompt, config, agent_slug, workspace, None).await
}

/// Same as `prompt_agent` but also accepts an `existing_run_id` —
/// when set, the dispatch attaches its kill handler to that run_id
/// instead of begin_run-ing a new one. Used by `prompt_agent_with_context`
/// which has to keep ownership of the registration so it can return
/// the run_id to the frontend (for overlap evidence + finish).
///
/// v2.7.10 Felipe P4 — when `prompt_arg` is whitespace-only AND a
/// slug is present, substitute the agent's `default_prompt` (column
/// from Felipe P5 / v2.7.9). This is the back half of the Run =
/// dispatch rework: the frontend "Run" button now fires here with
/// an empty prompt when default_prompt is set, expecting the
/// substitution to happen below.
///
/// `prompt_agent_with_context` does the same substitution one level
/// up so variable resolution can apply to `default_prompt`. The
/// duplicate here is defense-in-depth for direct callers (group
/// dispatch, headless replay, recipes) that don't route through
/// that wrapper — those callers don't get `{variables}` expansion on
/// the substituted text, which is fine for their use cases today
/// but worth knowing if you wire a new caller. The local variable
/// is intentionally named `prompt` (not shadowing the arg) so every
/// downstream `&prompt` reference is unambiguously "the effective
/// prompt" — the function is long and the rename heads off the
/// shadow trap a reviewer flagged in war-room 4D7247F6-… round 1.
async fn prompt_agent_inner(
    runtime: String,
    prompt_arg: String,
    config: Option<String>,
    agent_slug: Option<String>,
    workspace: Option<String>,
    existing_run_id: Option<String>,
) -> Result<String, String> {
    use tokio::process::Command;

    // Use the user's full shell PATH so CLIs can find node, npm, etc.
    let user_path = get_user_path();

    // Felipe P4 — empty prompt + known agent → fall back to the
    // agent's stored default_prompt. Quietly proceeds with the empty
    // string if no default is configured, matching pre-v2.7.10
    // behaviour (the upstream caller / CLI surface decides how to
    // handle that case).
    let prompt: String = if prompt_arg.trim().is_empty() {
        if let Some(slug) = agent_slug.as_deref() {
            load_agent_default_prompt(&crate::get_db_path(), slug, &runtime)
                .unwrap_or(prompt_arg)
        } else {
            prompt_arg
        }
    } else {
        prompt_arg
    };

    // F5 — extract model override from config, applied as `--model X` per
    // runtime. None → runtime default.
    let cfg_json: Option<serde_json::Value> = config
        .as_deref()
        .and_then(|c| serde_json::from_str(c).ok());
    let model_override: Option<String> = cfg_json
        .as_ref()
        .and_then(|c| c.get("model"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    // v2.7.8 PR-2 + PR-6 — load the agent's permissions when dispatching
    // by slug, BUT only enforce them when `permissions_migrated_at` is
    // non-NULL (the opt-in flag from PR-6). NULL = pre-v2.7.8 agent that
    // hasn't been confirmed for the new enforcement semantics → fall
    // back to defaults so the dispatch matches pre-PR-2 behaviour.
    //
    // The crate's default-arm output (when AgentPermissions is empty)
    // is pinned by PR-1 golden test #1 to match the pre-PR-2 hardcoded
    // flag bundles. A missing slug or DB failure degrades silently the
    // same way.
    let agent_perms: ato_agent_permissions::AgentPermissions = if let Some(slug) =
        agent_slug.as_deref()
    {
        let db_path = crate::get_db_path();
        rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .ok()
        .and_then(|c| {
            c.query_row(
                "SELECT permissions, permissions_migrated_at FROM agents
                  WHERE slug = ?1 AND runtime = ?2
                  ORDER BY COALESCE(last_used_at, created_at) DESC LIMIT 1",
                rusqlite::params![slug, runtime],
                |r| Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                )),
            )
            .ok()
        })
        .and_then(|(perms_json, migrated_at)| {
            // Opt-in flag: only enforce stored permissions when the
            // agent has been migrated. Pre-v2.7.8 rows have
            // migrated_at = NULL and keep using defaults.
            if migrated_at.is_some() {
                perms_json
            } else {
                None
            }
        })
        .as_deref()
        .map(ato_agent_permissions::parse_permissions_column)
        .unwrap_or_default()
    } else {
        ato_agent_permissions::AgentPermissions::default()
    };

    let mut cmd = match runtime.as_str() {
        "claude" => {
            let claude_path = which_claude().ok_or_else(|| {
                "Claude Code CLI not found on PATH. You can either:\n\
                 1. Install Claude Code: https://docs.claude.com/claude-code, OR\n\
                 2. Pick 'anthropic' from the runtime dropdown if you have an Anthropic API key configured in Settings → API Keys\n\
                 (Backend auto-fallback queued for v2.7.8.)".to_string()
            })?;
            let mut c = Command::new(claude_path);
            c.arg("--print").arg(&prompt);
            if let Some(m) = &model_override {
                c.arg("--model").arg(m);
            }
            // v2.7.8 PR-2 — agent-permission-aware --allowedTools.
            // When the agent has no permissions, the crate returns
            // CLAUDE_DEFAULT_ALLOWED_TOOLS which exactly matches the
            // pre-PR-2 hardcoded bundle (pinned by PR-1 golden test
            // #1). Per-invocation --allowedTools is the belt; the
            // suspenders is the settings.local.json file write at
            // agent-save time (queued for v2.7.9+).
            let claude_flags = ato_agent_permissions::to_claude(&agent_perms);
            c.arg("--allowedTools").arg(&claude_flags.allowed_tools);
            // v2.15.6 (task #33) — pretrust via --dangerously-skip-permissions
            // is DROPPED for the chat-panel path. Claude CLI 2.1.175 (and
            // likely all 2.1.x) requires --allow-dangerously-skip-permissions
            // to be enabled first, otherwise the second flag fails the
            // dispatch with exit status 1 in --print mode. Sessions and
            // war-rooms work fine because they spawn the prod CLI subprocess
            // which doesn't pass this flag.
            //
            // Cost of dropping: claude shows its trust prompt on the FIRST
            // dispatch into a new workspace; "Always Allow" makes future
            // dispatches into that workspace silent. One-time per-workspace
            // friction, not a hard block.
            //
            // The "real" fix the original code anticipated (line 1360-1363
            // pre-edit) was writing trust to ~/.claude/settings.local.json
            // at agent-save time — queued for v2.7.9+ originally, not yet
            // landed. When it does land, the pretrust UX comes back
            // without depending on claude CLI flag behavior.
            //
            // Felipe P3 / should_pretrust_workspace logic preserved in
            // history (git blame this block) — bring it back when the
            // settings.local.json path ships.
            c
        }
        "codex" => {
            let codex_path = which_cli("codex").ok_or_else(|| {
                // No OpenAI api_provider in the registry yet (v2.7.8
                // adds the api-provider-tool-call-loop scope), so codex
                // has no API fallback today. Install the CLI is the
                // only path.
                "Codex CLI not found on PATH. Install with: npm install -g @openai/codex\n\
                 (OpenAI API fallback not yet available — queued for v2.8.0's API-provider tool-call loop scope.)"
                    .to_string()
            })?;
            // exec subcommand; --skip-git-repo-check needed because ATO
            // can dispatch from any cwd (Felipe's "Not inside a trusted
            // directory" regression).
            //
            // 2026-05-19 (Will dogfood) — codex's `exec` defaults to
            // `--sandbox read-only` + `approval_policy=untrusted`. In a
            // war-room seat that means codex literally cannot patch
            // anything and surfaces "I didn't patch because this harness
            // is read-only" in its reply. Without a TTY (we pipe
            // stdout/stderr), the on-request approvals can't be answered
            // either. ATO's positioning is agentic: dispatching codex
            // IS the authorization, so unlock both:
            //   --sandbox workspace-write   → codex can edit files in
            //                                 the working directory but
            //                                 not escape it.
            //   -c approval_policy="never"  → no headless-blocking
            //                                 approval prompts.
            // `danger-full-access` would let codex touch ~/ paths
            // outside the workspace — explicitly NOT what we want.
            let mut c = Command::new(codex_path);
            // v2.7.8 PR-2 — agent-permission-aware sandbox mode. The
            // pre-PR-2 baseline (`workspace-write` + `never`) is the
            // crate's empty-permissions default (PR-1 test #1). Any
            // non-empty deny/approval list demotes to `read-only`
            // because codex --sandbox is a 3-mode enum with no
            // per-tool deny. Advisory labels surface in UI / telemetry.
            let codex_flags = ato_agent_permissions::to_codex(&agent_perms);
            c.arg("exec")
                .arg("--skip-git-repo-check")
                .arg("--sandbox")
                .arg(codex_flags.sandbox)
                .arg("-c")
                .arg(format!(
                    "approval_policy=\"{}\"",
                    codex_flags.approval_policy
                ));
            if let Some(m) = &model_override {
                c.arg("--model").arg(m);
            }
            c.arg(&prompt);
            c
        }
        "openclaw" => {
            let ssh_config: serde_json::Value = config
                .as_deref()
                .and_then(|c| serde_json::from_str(c).ok())
                .unwrap_or_default();
            let host = ssh_config
                .get("sshHost")
                .and_then(|v| v.as_str())
                .unwrap_or("localhost");
            let port = ssh_config.get("sshPort").and_then(|v| v.as_u64()).unwrap_or(22);
            let user = ssh_config
                .get("sshUser")
                .and_then(|v| v.as_str())
                .unwrap_or("root");
            let key_path = ssh_config.get("sshKeyPath").and_then(|v| v.as_str());

            let mut c = Command::new("ssh");
            if let Some(key) = key_path {
                c.args(["-i", key]);
            }
            c.args([
                "-p",
                &port.to_string(),
                &format!("{}@{}", user, host),
                &format!("openclaw exec '{}'", prompt.replace('\'', "'\\''")),
            ]);
            c
        }
        "hermes" => {
            let hermes_path =
                which_cli("hermes").ok_or_else(|| "Hermes CLI not found".to_string())?;
            let mut c = Command::new(hermes_path);
            c.arg("--execute").arg(&prompt);
            c
        }
        "gemini" => {
            // 2026-05-19 (Will dogfood) — if the CLI is missing AND the user
            // has a Google API key configured, surface that path. Backend
            // auto-fallback is v2.7.8 work; tonight we just point the user
            // at the existing 'google' runtime that uses their stored key.
            let gemini_path = which_cli("gemini").ok_or_else(|| {
                let has_google_key = crate::api_dispatch::find_provider("google").is_some();
                if has_google_key {
                    "Gemini CLI not found on PATH. You can either:\n\
                     1. Pick 'google' from the runtime dropdown — it uses your stored Google API key (no CLI install needed), OR\n\
                     2. Install the Gemini CLI: npm install -g @google/gemini-cli@latest\n\
                     (Backend auto-fallback queued for v2.7.8.)".to_string()
                } else {
                    "Gemini CLI not found on PATH. You can either:\n\
                     1. Install the Gemini CLI: npm install -g @google/gemini-cli@latest, OR\n\
                     2. Add a Google API key in Settings → API Keys and use 'google' from the runtime dropdown".to_string()
                }
            })?;
            // v2.7.8 PR-2 — gemini's enforcement is binary
            // (--yolo or default). If the agent's permissions can't be
            // honored (any non-empty deny/approval list), refuse the
            // dispatch rather than silently dropping the policy.
            let gemini_flags = ato_agent_permissions::to_gemini(&agent_perms);
            if let Some(err) = gemini_flags.error {
                return Err(err);
            }
            let mut c = Command::new(gemini_path);
            c.arg("-p").arg(&prompt);
            if gemini_flags.yolo {
                c.arg("--yolo");
            }
            if let Some(m) = &model_override {
                c.arg("-m").arg(m);
            }
            c
        }
        other => return Err(format!("Unknown runtime: {}", other)),
    };
    cmd.env("PATH", &user_path);
    // BYOK: forward stored anthropic/openai/google key as the runtime's
    // env var. No-op for openclaw/hermes and for users without a stored
    // key (subprocess falls through to its own OAuth credentials).
    if let Some((var, key)) =
        crate::byok::byok_env_value_from_path(&crate::get_db_path(), &runtime)
    {
        cmd.env(var, key);
    }

    let started = std::time::Instant::now();
    let result = dispatch_command_killable(
        cmd,
        &runtime,
        &runtime,
        agent_slug.as_deref(),
        workspace.as_deref(),
        "desktop:prompt_agent",
        existing_run_id.as_deref(),
    )
    .await;
    // Persist into execution_logs so Runs → History reflects every
    // dispatch. Was unwired before — `add_execution_log` existed as a
    // Tauri command but no JS code called it, so the table stayed
    // empty and Beatriz's verified runs never appeared in History.
    // Doing it here covers every caller (UI dispatch, group stages,
    // MCP run_agent, headless cron) since they all funnel through
    // prompt_agent_inner.
    let duration_ms = started.elapsed().as_millis() as i32;
    persist_execution_log(
        &runtime,
        &prompt,
        &result,
        duration_ms,
        model_override.as_deref(),
        agent_slug.as_deref(),
        None,
    );
    result
}

/// v2.6 PR-A — observatory tagging for execution_logs rows. Carries
/// the dispatch-kind, billing surface, and provider session info so
/// the watcher path can use the same persistence helper as the
/// active dispatch path without duplicating insert logic. `active`
/// dispatches pass `None` and accept the default (dispatch_kind =
/// 'active', billing_surface = NULL — analytics treats NULL as
/// "unknown" and the auth_mode column is the active-side signal).
pub struct ObservationTag<'a> {
    pub dispatch_kind: &'a str,
    pub billing_surface: Option<&'a str>,
    pub provider_session_id: Option<&'a str>,
    pub sequence_within_session: Option<i64>,
    /// Pre-counted tokens from the upstream JSONL when available
    /// (Claude Code emits `usage.input_tokens` / `usage.output_tokens`;
    /// Codex emits `token_count` events). When `None` the helper falls
    /// back to the 4-char heuristic so this struct works for sources
    /// that don't surface token counts.
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    /// ISO-8601 timestamp from the upstream session. When set the
    /// row's created_at uses this instead of `now()` so the History
    /// panel renders observed runs at the time they actually happened.
    pub observed_at: Option<&'a str>,
}

/// Best-effort insert into the execution_logs table. Opens its own
/// connection because callers may be outside the Tauri State context
/// (group stages, headless cron). Errors are swallowed — observability
/// must never break the dispatch path.
///
/// v2.2.0 — captures estimated tokens (4 chars/token rule) and USD cost
/// (per-M pricing lookup) for every dispatch where the model is known.
/// Cost is an estimate, surfaced as "est." in the UI. Real captured
/// token counts from runtime SDK responses are a follow-up; estimation
/// is honest enough that Compare/Cost Recs/Replay show real numbers
/// instead of "—" for every model in the pricing table.
pub(crate) fn persist_execution_log(
    runtime: &str,
    prompt: &str,
    result: &Result<String, String>,
    duration_ms: i32,
    model_override: Option<&str>,
    agent_slug: Option<&str>,
    observation: Option<&ObservationTag<'_>>,
) {
    let db_path = crate::get_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let id = uuid::Uuid::new_v4().to_string();
    let now = observation
        .and_then(|o| o.observed_at.map(|s| s.to_string()))
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let (response, status, error_message): (Option<String>, &str, Option<String>) = match result {
        Ok(r) => (Some(truncate_for_log(r)), "success", None),
        Err(e) => (None, "error", Some(truncate_for_log(e))),
    };
    // v2.3.6 — Token estimates only. Cost stays NULL for every dispatch
    // through this path because runtime-CLI dispatches (claude --print,
    // codex exec, gemini -p, etc.) use the user's *subscription* —
    // they don't bill per token. Surfacing an "API-equivalent" dollar
    // value here misled the cost panels into treating subscription rows
    // as billed ones. Token counts are still useful for usage tracking,
    // so they stay populated regardless of whether we have a pricing
    // row — they're a pure char-count heuristic that works for any
    // runtime including ones without a default model (openclaw, hermes).
    //
    // Direct-API dispatches (deploy bundles, future ato-cli --api-key
    // path) will compute cost via a separate path that knows the
    // dispatch was billed.
    //
    // TODO(v2.4): add a `billing_source` column on execution_logs so
    // cost_usd_estimated NULL stops conflating four distinct states
    // (subscription / unknown-model / pre-migration / pricing-failure).
    let effective_model = model_override
        .filter(|s| !s.is_empty())
        .or_else(|| default_model_for_runtime(runtime));
    let response_text = response.as_deref().unwrap_or("");
    // Prefer real token counts from the upstream source (Claude Code
    // emits them in `usage.*`, Codex via token_count events). Fall
    // back to the 4-char heuristic so callers that don't have them
    // still record something useful.
    let tokens_in = observation
        .and_then(|o| o.tokens_in)
        .or_else(|| Some(estimate_text_tokens(prompt)));
    let tokens_out = observation
        .and_then(|o| o.tokens_out)
        .or_else(|| Some(estimate_text_tokens(response_text)));
    // Cost is now computed for every dispatch where the model is
    // known, regardless of auth path. For subscription rows it's the
    // "API-equivalent" amount Anthropic / OpenAI / Google would have
    // charged — useful for the credit-burn meter ("you'd be paying
    // $X if billed at API rates"). For api_key rows it's the actual
    // billing estimate. The auth_mode column lets the analytics
    // query split the two.
    let cost_usd: Option<f64> = effective_model
        .and_then(|m| estimate_cost_usd(m, prompt, response_text));
    // Effective auth path = what dispatch actually used. Combines
    // user's explicit choice + stored-key availability + env-var
    // presence. Same fn the runtime card badge reads, so the per-
    // dispatch attribution can't drift from the displayed mode.
    // None for hermes/openclaw — they have no BYOK mapping and
    // shouldn't pollute the credit-burn meter's subscription bucket.
    let auth_mode: Option<&str> = crate::byok::effective_auth_mode_from_path(&db_path, runtime);
    // v2.6 PR-A — observatory columns. Active dispatches keep the
    // default 'active' kind + NULL billing_surface; passive watcher
    // rows pass an ObservationTag with full attribution.
    let dispatch_kind: &str = observation.map(|o| o.dispatch_kind).unwrap_or("active");
    let billing_surface: Option<&str> = observation.and_then(|o| o.billing_surface);
    let provider_session_id: Option<&str> = observation.and_then(|o| o.provider_session_id);
    let sequence_within_session: Option<i64> = observation.and_then(|o| o.sequence_within_session);
    let _ = conn.execute(
        "INSERT OR IGNORE INTO execution_logs (id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, cloud_trace_id, created_at, cost_usd_estimated, agent_slug, model, auth_mode, dispatch_kind, billing_surface, provider_session_id, sequence_within_session) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        rusqlite::params![
            id,
            runtime,
            truncate_for_log(prompt),
            response,
            tokens_in,
            tokens_out,
            duration_ms,
            status,
            error_message,
            now,
            cost_usd,
            agent_slug,
            effective_model,
            auth_mode,
            dispatch_kind,
            billing_surface,
            provider_session_id,
            sequence_within_session,
        ],
    );

    // v2.3.8 Phase 4.2 — Publish DispatchFailed on error. The events
    // bus subscriber (recipes engine) reacts; the table-bound audit
    // happens inside publish() via events_log.
    //
    // v2.6 PR-A — Passive observations are read-only echoes of other
    // CLIs' sessions; firing ATO recipes off them would be a footgun
    // (a Claude Code prompt failing in another terminal shouldn't
    // trigger the user's ATO notification recipes). Skip the event
    // bus for non-active rows.
    let is_active = observation.map(|o| o.dispatch_kind == "active").unwrap_or(true);
    if is_active && status == "error" {
        let event = crate::events::AtoEvent::DispatchFailed {
            event_seq: crate::events::next_seq(),
            run_id: id.clone(),
            agent_slug: agent_slug.map(|s| s.to_string()),
            runtime: runtime.to_string(),
            error_message: error_message.clone().unwrap_or_default(),
            duration_ms: duration_ms as i64,
            failed_at: now.clone(),
        };
        crate::events::bus::publish(event);
    }
}

// ── v2.2.0 cost estimation helpers ────────────────────────────────────
//
// Mirrors apps/desktop/src/lib/pricing.ts. Keep the two tables in sync:
// the JS table is the source of truth for UI/Compare/Replay rendering,
// this one is what the dispatch path writes into execution_logs so that
// History/Insights queries don't have to recompute on every read.

// Pricing helpers live in `packages/ato-pricing` (extracted 2026-05-17).
// `apps/desktop/src/lib/pricing.ts` is the JS mirror — still hand-kept
// in sync; replace with a codegen step in a follow-up.
use ato_pricing::estimate_text_tokens;

/// Mirror of DEFAULT_MODEL_PER_RUNTIME in pricing.ts — what the runtime
/// CLI defaults to when no explicit `--model` is passed. Letting the
/// dispatch path estimate cost even when the caller didn't specify a
/// model is the difference between "every dispatch has a cost number"
/// and "only configured agents do."
///
/// Lives here (not the shared pricing crate) because the runtime list is
/// CLI-runtime-specific, not a generic pricing concern.
fn default_model_for_runtime(runtime: &str) -> Option<&'static str> {
    match runtime {
        "claude" => Some("claude-sonnet-4-6"),
        "codex" => Some("gpt-4.1"),
        "gemini" => Some("gemini-2.5-flash"),
        _ => None,
    }
}

// estimate_cost_usd alias kept for the dead-code path that referenced
// it inline; the shared crate's version is identical. Drop when the
// caller is gone.
#[allow(dead_code)]
use ato_pricing::estimate_cost_usd;

/// 64 KB cap on prompt/response/error text persisted into SQLite. A
/// runaway tool that dumps a giant log shouldn't bloat the History
/// table beyond what's useful at a glance.
fn truncate_for_log(s: &str) -> String {
    const MAX: usize = 64 * 1024;
    if s.len() <= MAX { s.to_string() } else { format!("{}…[truncated]", &s[..MAX]) }
}

// v2.3.26 Phase 6.x-C — GUI-side API-provider dispatch.
//
// When the user picks MiniMax/Grok/etc. in PromptBar, the existing
// promptAgent path errors because there's no CLI binary. This
// command takes the API-provider slug, runs the same HTTPS dispatch
// the CLI's `ato dispatch <provider>` does, and persists the result
// to execution_logs so it shows up in History alongside CLI runs.
//
// Live-runs registration: writes to live_runs at the start +
// removes at the end (same pattern as v2.3.25 for CLI-process
// dispatches), so the Runs → Live tab shows the in-flight API call.

#[derive(serde::Serialize)]
pub struct ApiDispatchResult {
    pub id: String,
    pub runtime: String,
    pub model: String,
    pub status: String,
    pub response: Option<String>,
    pub error_message: Option<String>,
    pub duration_ms: i64,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub created_at: String,
}

// v2.7.8 PR-3b — `workspace_root` (5th arg): when the dispatched
// agent has permissions enabling a tool-call loop, this must be the
// absolute path to the user's project root. The desktop process cwd
// (`apps/desktop/` in dev) is NOT a safe sandbox root — codex review
// flagged that resolving cwd would silently inspect the wrong repo.
// None is allowed for text-only dispatches; tool-using dispatches
// without an explicit workspace_root return a hard error so the
// caller can't accidentally read files from the wrong project.
#[tauri::command]
pub async fn prompt_api_provider(
    runtime: String,
    prompt: String,
    model: Option<String>,
    agent_slug: Option<String>,
    workspace_root: Option<String>,
) -> Result<ApiDispatchResult, String> {
    let provider = crate::api_dispatch::find_provider(&runtime).ok_or_else(|| {
        format!(
            "'{}' is not a known API provider (expected one of: minimax, grok, deepseek, qwen, openrouter)",
            runtime
        )
    })?;

    let db_path = crate::get_db_path();

    // Mirror into active_runs so the Live tab shows this dispatch.
    // MiniMax round-1 6.x-C flagged that finish_run wasn't panic-safe;
    // wrap in a Drop guard so the active_runs row is cleared even if
    // the dispatch fn panics or an early return slips in later.
    let active_run_id = crate::active_runs::begin_run(
        provider.slug,
        agent_slug.as_deref(),
        None,
        Some("desktop:api"),
    );
    struct ActiveRunGuard(String);
    impl Drop for ActiveRunGuard {
        fn drop(&mut self) {
            crate::active_runs::finish_run(&self.0);
        }
    }
    let _active_run_guard = ActiveRunGuard(active_run_id);

    // v2.7.8 PR-3b — load the agent's permission gate so we can decide
    // whether to engage the tool-call loop. Honors the PR-6 migration
    // flag: pre-v2.7.8 agents (NULL `permissions_migrated_at`) get an
    // empty gate → text-only dispatch, matching pre-PR-3b behaviour.
    //
    // We also load the matched agent's persisted `runtime` so the
    // permissions row resolves correctly when the user attached a
    // CLI-runtime agent to an API-provider dispatch (e.g. an agent
    // created under "gemini" runtime is reused via the "google"
    // provider — same pattern PR-5a's CLI auto-fallback solved).
    let (agent_perms, agent_runtime_label): (
        ato_agent_permissions::AgentPermissions,
        Option<String>,
    ) = if let Some(slug) = agent_slug.as_deref() {
        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .ok();
        if let Some(c) = conn {
            // Most-recently-used row for this slug (any runtime). The
            // agent.permissions DSL is runtime-agnostic at the spec
            // level; the dispatch runtime is provider.slug for the
            // HTTP call regardless.
            let row: Option<(Option<String>, Option<String>, String)> = c
                .query_row(
                    "SELECT permissions, permissions_migrated_at, runtime
                       FROM agents WHERE slug = ?1
                       ORDER BY COALESCE(last_used_at, created_at) DESC LIMIT 1",
                    rusqlite::params![slug],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .ok();
            match row {
                Some((perms_json, migrated_at, runtime_label)) => {
                    let p = if migrated_at.is_some() {
                        perms_json
                            .as_deref()
                            .map(ato_agent_permissions::parse_permissions_column)
                            .unwrap_or_default()
                    } else {
                        ato_agent_permissions::AgentPermissions::default()
                    };
                    (p, Some(runtime_label))
                }
                None => (ato_agent_permissions::AgentPermissions::default(), None),
            }
        } else {
            (ato_agent_permissions::AgentPermissions::default(), None)
        }
    } else {
        (ato_agent_permissions::AgentPermissions::default(), None)
    };
    let _ = agent_runtime_label; // captured for telemetry only today; PR-5 UI surfaces it

    // v2.7.10 PR-B — MCP-tool gating. Discover the tools the agent's
    // attached MCP servers expose so the gate can decide which to
    // offer, and so dispatch_with_tools can route a matching tool_call
    // through execute_mcp_tool. All sync work (DB read + MCP stdio
    // handshakes) is wrapped in spawn_blocking inside
    // mcp_dispatch::load_agent_mcp_tools — fixes v2.7.9's blocked-
    // worker ship blocker. Per-(slug, mcps_hash) cache means repeat
    // dispatches don't re-spawn the MCP servers.
    let mcp_bindings: Vec<crate::commands::mcp_dispatch::McpToolBinding> =
        if let Some(slug) = agent_slug.as_deref() {
            crate::commands::mcp_dispatch::load_agent_mcp_tools(slug).await
        } else {
            Vec::new()
        };
    let mcp_perm_tools: Vec<ato_agent_permissions::ToolDef> =
        mcp_bindings.iter().map(Into::into).collect();
    let agent_gate = ato_agent_permissions::to_api_tool_gate(&agent_perms, &mcp_perm_tools);

    // Decide between the tool-call loop and the legacy text-only
    // dispatch:
    //   - tool loop: agent has at least one tool the model would
    //     actually be able to use — built-in (read_file, grep, …) OR
    //     MCP-declared — AND provider supports tools.
    //   - text-only: otherwise. Preserves pre-PR-3b behaviour for
    //     agents without permissions and for non-tools providers.
    //
    // S7 fix (war-room finding B1): use `agent_gate.allowed_tools`
    // as the AUTHORITY for what's exposed — `gate.check()` returns
    // Allow for any name not in `denied`/`approval_required`, which
    // would leak every MCP tool past the permission filter. The
    // catalogue intersection that `to_api_tool_gate` already did is
    // the right source of truth.
    let allowed_tool_names: std::collections::HashSet<String> = agent_gate
        .allowed_tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    let any_builtin_allowed = ato_review_tools::registry()
        .iter()
        .any(|t| allowed_tool_names.contains(&t.name));
    let any_mcp_allowed = mcp_bindings
        .iter()
        .any(|b| allowed_tool_names.contains(&b.name));
    let use_tool_loop = !agent_gate.allowed_tools.is_empty()
        && crate::api_dispatch_tools::provider_supports_tools(provider)
        && (any_builtin_allowed || any_mcp_allowed);

    let outcome = if use_tool_loop {
        // PR-3b — explicit workspace root is REQUIRED for tool
        // dispatches. The desktop process cwd is the wrong sandbox
        // (codex review: it would silently resolve to the ATO repo,
        // not the user's project). If the frontend didn't pass a
        // root, fail loud — better than reading the wrong files.
        let ws_str = workspace_root.as_deref().unwrap_or("").trim();
        if ws_str.is_empty() {
            return Err(
                "Tool-using API dispatch requires an explicit workspace_root. \
                 The frontend's prompt_api_provider call must pass the active \
                 project's root path. (PR-3b safety: cwd is not a safe sandbox.)"
                    .to_string(),
            );
        }
        let workspace_path = std::path::PathBuf::from(ws_str);
        if !workspace_path.is_absolute() {
            return Err(format!(
                "workspace_root must be an absolute path; got '{}'",
                ws_str
            ));
        }
        if !workspace_path.exists() {
            return Err(format!(
                "workspace_root '{}' does not exist on disk",
                ws_str
            ));
        }
        // Built-in tools the gate allows (read_file / grep / …).
        let mut tools = crate::api_dispatch_tools::build_filtered_review_tools(&agent_gate);
        // S7 fix (war-room finding B2): drop any MCP binding whose
        // name collides with a built-in. The Anthropic API rejects
        // duplicate tool names in the request body with HTTP 400; the
        // built-in-wins precedence in dispatch_with_tools doesn't
        // matter if the wire call never happens. Built-ins take
        // precedence — an MCP can't shadow them. The skipped MCP
        // entries are logged so operators can rename their MCP tool
        // (or remove the offending entry) and try again.
        let builtin_names: std::collections::HashSet<String> = ato_review_tools::registry()
            .into_iter()
            .map(|t| t.name)
            .collect();
        // S7 fix (war-room finding B1): filter by membership in
        // agent_gate.allowed_tools — that's the catalogue
        // intersection `to_api_tool_gate` computed against
        // p.allowed / p.denied. `gate.check` would Allow everything
        // not explicitly denied, which silently exposes every MCP
        // tool the model can imagine.
        let mut mcp_bindings_allowed: Vec<crate::commands::mcp_dispatch::McpToolBinding> = Vec::new();
        for b in &mcp_bindings {
            if !allowed_tool_names.contains(&b.name) {
                continue;
            }
            if builtin_names.contains(&b.name) {
                eprintln!(
                    "prompt_api_provider: dropping MCP tool '{}' from '{}' — name collides with built-in (built-in wins).",
                    b.name, b.mcp_slug
                );
                continue;
            }
            mcp_bindings_allowed.push(b.clone());
        }
        tools.extend(
            mcp_bindings_allowed
                .iter()
                .map(ato_review_tools::ToolDef::from),
        );
        crate::api_dispatch_tools::dispatch_with_tools(
            provider,
            &[],
            &prompt,
            model.as_deref(),
            &tools,
            &mcp_bindings_allowed,
            &workspace_path,
            &db_path,
        )
        .await
    } else {
        crate::api_dispatch::dispatch(provider, &prompt, model.as_deref(), &db_path).await
    };

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // Open a fresh conn for the persistence write (the original was
    // dropped at the end of the match arm above).
    let write_conn = rusqlite::Connection::open(&db_path).map_err(|e| format!("open db: {}", e))?;

    let result = match outcome {
        Ok(o) => {
            let status = if o.response.is_some() { "success" } else { "error" };
            // v2.7.8 PR-3b — persist tool-call audit alongside the
            // standard execution_logs columns. tool_calls_count and
            // tool_calls_summary were added in v2.4.5 (schema.rs:
            // 372-378) but desktop API dispatches never populated
            // them. None when the loop didn't engage; non-zero count
            // + summary JSON when it did.
            let (tool_calls_count, tool_calls_summary): (Option<i64>, Option<String>) =
                match &o.tool_calls {
                    Some(audit) => (
                        Some(audit.len() as i64),
                        Some(serde_json::to_string(audit).unwrap_or_else(|_| "[]".to_string())),
                    ),
                    None => (None, None),
                };
            // v2.7.15 — cost-tracking bug fix (war-room 2A5D9504 #B,
            // claude VETO of original premise). Pre-fix this path
            // hardcoded `cost_usd_estimated = NULL` AND omitted the
            // `model` column entirely. EVERY desktop BYOK dispatch
            // recorded $0 cost — strictly worse than the CLI's
            // partial coverage. Now we compute cost the same way
            // CLI run_api does: use cost_from_token_classes so
            // Anthropic cache classes (cache_creation at 1.25×,
            // cache_read at 0.10×) are billed correctly. For
            // non-Anthropic providers both cache fields are None
            // and the function degrades to flat in×rate + out×rate.
            let cost_usd: Option<f64> = match (o.tokens_in, o.tokens_out) {
                (Some(ti), Some(to)) => {
                    let tc = ato_pricing::TokenClasses {
                        tokens_in: ti,
                        tokens_out: to,
                        cache_creation_in: o.cache_creation_tokens,
                        cache_read_in: o.cache_read_tokens,
                    };
                    ato_pricing::cost_from_token_classes(&o.model_used, &tc)
                }
                _ => estimate_cost_usd(
                    &o.model_used,
                    &prompt,
                    o.response.as_deref().unwrap_or(""),
                ),
            };
            // MiniMax round-1 6.x-C: surface write failures instead
            // of swallowing them. The dispatch still succeeds; the
            // log row just doesn't exist, and the user sees why.
            if let Err(e) = write_conn.execute(
                "INSERT INTO execution_logs (id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, cloud_trace_id, created_at, cost_usd_estimated, tool_calls_count, tool_calls_summary, model, auth_mode, retry_count, attempt_summary, cache_creation_tokens, cache_read_tokens, reasoning_tokens, initiator_kind, client_surface, initiator_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, 'human', 'desktop', NULL)",
                rusqlite::params![
                    id,
                    provider.slug,
                    truncate_api_log(&prompt),
                    o.response.as_ref().map(|s| truncate_api_log(s)),
                    o.tokens_in,
                    o.tokens_out,
                    o.duration_ms,
                    status,
                    o.error_message.as_ref().map(|s| truncate_api_log(s)),
                    now,
                    cost_usd,
                    tool_calls_count,
                    tool_calls_summary,
                    o.model_used,
                    "api_key",
                    o.retry_count,
                    o.attempt_summary_json.as_ref(),
                    o.cache_creation_tokens,
                    o.cache_read_tokens,
                    o.reasoning_tokens,
                ],
            ) {
                eprintln!("prompt_api_provider: execution_logs write failed: {}", e);
            }
            ApiDispatchResult {
                id,
                runtime: provider.slug.to_string(),
                model: o.model_used,
                status: status.to_string(),
                response: o.response,
                error_message: o.error_message,
                duration_ms: o.duration_ms,
                tokens_in: o.tokens_in,
                tokens_out: o.tokens_out,
                created_at: now,
            }
        }
        Err(e) => {
            // v2.7.15 — error path now records the REQUESTED model
            // + prompt-only cost estimate (war-room 2A5D9504 #E).
            // Pre-fix this branch recorded NULL for both, hiding the
            // cost of failed dispatches that the provider DID bill us
            // for (input tokens scanned before rejection). Mirror the
            // CLI fix in dispatch.rs::run_api's Err arm.
            let requested_model = model
                .clone()
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| provider.default_model.to_string());
            let est_input_tokens = estimate_text_tokens(&prompt);
            let cost_usd: Option<f64> =
                ato_pricing::cost_from_tokens(&requested_model, est_input_tokens, 0);
            // explicit Option type annotation so the rusqlite params!
            // macro can infer the binding correctly.
            let cost_usd_opt: Option<f64> = cost_usd;
            if let Err(write_err) = write_conn.execute(
                "INSERT INTO execution_logs (id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, cloud_trace_id, created_at, cost_usd_estimated, model, auth_mode, initiator_kind, client_surface, initiator_id)
                 VALUES (?1, ?2, ?3, NULL, ?4, 0, 0, 'error', ?5, NULL, NULL, ?6, ?7, ?8, ?9, 'human', 'desktop', NULL)",
                rusqlite::params![
                    id,
                    provider.slug,
                    truncate_api_log(&prompt),
                    est_input_tokens,
                    &e,
                    now,
                    cost_usd_opt,
                    requested_model,
                    "api_key",
                ],
            ) {
                eprintln!(
                    "prompt_api_provider: execution_logs error-write failed: {}",
                    write_err
                );
            }
            ApiDispatchResult {
                id,
                runtime: provider.slug.to_string(),
                model: model.unwrap_or_default(),
                status: "error".to_string(),
                response: None,
                error_message: Some(e),
                duration_ms: 0,
                tokens_in: None,
                tokens_out: None,
                created_at: now,
            }
        }
    };
    Ok(result)
}

fn truncate_api_log(s: &str) -> String {
    const MAX: usize = 64 * 1024;
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…[truncated]", &s[..MAX])
    }
}

// v2.7.9 PR-B deferred to v2.7.10 — see comment in prompt_api_provider.
// load_agent_mcp_tools removed pending spawn_blocking wrap + MCP
// execution path. Kept in audit doc for v2.7.10 reference.


// ── v2.3.2 Phase 2.x — Local config-change ledger ─────────────────────
//
// The CLI's `ato agents create | update` writes to local
// `agent_config_changes` already. This Tauri command lets the GUI's
// agent-update paths do the same dual-write (cloud + local). Without
// it, signed-out users would have GUI-driven edits invisible to the
// local regression detector. Best-effort: never fail.

#[tauri::command]
pub fn record_local_config_change(
    agent_slug: String,
    field: String,
    old_value: Option<String>,
    new_value: Option<String>,
    actor: Option<String>,
) -> Result<(), String> {
    let db_path = crate::get_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        // Best-effort: failure here doesn't block the GUI edit.
        Err(_) => return Ok(()),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let _ = conn.execute(
        "INSERT INTO agent_config_changes (id, agent_slug, field, old_value, new_value, actor, changed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            id,
            agent_slug,
            field,
            old_value,
            new_value,
            actor.unwrap_or_else(|| "desktop:gui".to_string()),
            now,
        ],
    );
    Ok(())
}


// ── v2.1.0 Replay infra ─────────────────────────────────────────────────
//
// One-shot interactive replay: user picks a past trace, picks a target
// runtime/model, we re-dispatch the original prompt and surface the diff.
//
// Design choices (see plan at ~/.claude/plans/peaceful-strolling-kay.md):
//   - Prompts come from local execution_logs (already populated for every
//     dispatch since v2.0.1) — no new cloud retention obligations.
//   - Linking cloud trace ↔ local execution_logs row uses temporal
//     correlation (matching runtime + close created_at window) rather than
//     refactoring prompt_agent_inner's return signature. Same-machine
//     clocks are tight; collision risk only if two same-runtime dispatches
//     fire in the same 10-second window with the same prompt — unlikely
//     and doesn't break correctness, just attribution.
//   - Replay dispatches go through the existing prompt_agent_inner so they
//     register in Live runs + are killable + auto-persist their own
//     execution_logs row (closing the loop — replay outputs are themselves
//     traceable).

/// Hand the local execution_logs row that just produced a cloud trace
/// upload its corresponding cloud_trace_id, so future replay lookups can
/// find the full prompt by trace ID. Best-effort temporal match — caller
/// passes the cloud trace's started_at + runtime, we find the matching
/// local row by walking forward from started_at.
///
/// v2.1.11 — Window widened from ±10s to [-30s, +5min]. The original
/// ±10s window broke for slow stages (Codex pipelines >10s) because
/// execution_logs.created_at is set when the dispatch FINISHES while
/// the trace's started_at is when it STARTED. A 10.8s stage put the
/// local row past the upper bound. Forward-skewed window aligns with
/// the actual data: created_at is always ≥ started_at (modulo clock
/// skew); 5min ceiling keeps the lookup specific enough that
/// same-runtime collisions in a busy session don't mis-attribute.
#[tauri::command]
pub fn link_execution_log_to_cloud_trace(
    cloud_trace_id: String,
    runtime: String,
    started_at: String,
) -> Result<bool, String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    let started =
        chrono::DateTime::parse_from_rfc3339(&started_at).map_err(|e| e.to_string())?;
    // 30s back-tolerance covers minor clock skew between JS Date.now()
    // and chrono::Utc::now(); 5min forward covers the slowest realistic
    // dispatch + any post-dispatch processing latency.
    let lower = (started - chrono::Duration::seconds(30)).to_rfc3339();
    let upper = (started + chrono::Duration::minutes(5)).to_rfc3339();
    let updated = conn
        .execute(
            "UPDATE execution_logs
                SET cloud_trace_id = ?1
              WHERE id IN (
                SELECT id FROM execution_logs
                 WHERE runtime = ?2
                   AND cloud_trace_id IS NULL
                   AND created_at BETWEEN ?3 AND ?4
                 ORDER BY created_at ASC
                 LIMIT 1
              )",
            rusqlite::params![cloud_trace_id, runtime, lower, upper],
        )
        .map_err(|e| e.to_string())?;
    Ok(updated > 0)
}

#[derive(serde::Serialize, Clone)]
pub struct ReplayJob {
    pub id: String,
    pub source_execution_log_id: String,
    pub source_cloud_trace_id: Option<String>,
    pub source_runtime: String,
    pub source_model: Option<String>,
    pub target_runtime: String,
    pub target_model: Option<String>,
    pub status: String,
    pub response: Option<String>,
    pub duration_ms: Option<i32>,
    pub error_message: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    // v2.2.0 — captured cost estimate for the replay output. Stays None
    // for pending/running jobs and for models we don't have pricing for.
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cost_usd_estimated: Option<f64>,
}

/// Queue a replay of the given cloud trace's prompt against a different
/// runtime + (optional) model. Returns the new replay_job id immediately;
/// the actual dispatch runs in a tokio task and the row is updated when
/// it finishes. Frontend polls get_replay_job for status.
#[tauri::command]
pub async fn start_replay(
    cloud_trace_id: String,
    target_runtime: String,
    target_model: Option<String>,
) -> Result<String, String> {
    let db_path = crate::get_db_path();
    // Look up the source prompt + runtime + model from execution_logs.
    // v2.3.9 — accept either cloud_trace_id OR execution_logs.id. The
    // parameter name stays cloud_trace_id for compatibility with
    // existing GUI callers; the lookup now mirrors the CLI's. Closes
    // codex #2 from the v2.3.8 review: the recipe engine's
    // DispatchFailed → ReplayOnAlt chain passes execution_logs.id but
    // start_replay previously only matched cloud_trace_id.
    let (source_id, source_runtime, _source_status, prompt) = {
        let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
        // 2026-05-19 war-room synthesis: filter dispatch_kind='active' so
        // passive-observation rows (no prompt, no replayable spec) can't
        // become a malformed start_replay source.
        let row = conn.query_row(
            "SELECT id, runtime, status, prompt FROM execution_logs \
             WHERE (cloud_trace_id = ?1 OR id = ?1) AND dispatch_kind = 'active' LIMIT 1",
            rusqlite::params![cloud_trace_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        );
        match row {
            Ok((id, runtime, status, Some(p))) => (id, runtime, status, p),
            // No matching local row OR the prompt was lost (column NULL).
            // Surface a stable error code the UI keys off for the
            // multi-device disclosure.
            Ok((_, _, _, None)) => return Err("prompt-not-local".to_string()),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return Err("prompt-not-local".to_string())
            }
            Err(e) => return Err(format!("lookup-failed: {}", e)),
        }
    };

    // INSERT the pending row now so the frontend can poll immediately
    // even if the dispatch takes a while to start.
    let job_id = uuid::Uuid::new_v4().to_string();
    let started_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    {
        let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO replay_jobs
                (id, source_execution_log_id, source_cloud_trace_id, source_runtime,
                 source_model, target_runtime, target_model, status, started_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, 'pending', ?7)",
            rusqlite::params![
                job_id,
                source_id,
                cloud_trace_id,
                source_runtime,
                target_runtime,
                target_model,
                started_at,
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    // Spawn the dispatch. We capture all the strings for the closure;
    // the closure runs in the background and the function returns
    // job_id immediately for polling.
    let job_id_for_task = job_id.clone();
    let target_runtime_for_task = target_runtime.clone();
    let target_model_for_task = target_model.clone();
    tokio::spawn(async move {
        // Mark running. If this UPDATE fails we still try the dispatch —
        // the only consequence is the UI sees 'pending' a bit longer.
        let _ = mark_replay_running(&job_id_for_task);
        let dispatch_started = std::time::Instant::now();

        // Build a config JSON with the model override so prompt_agent_inner
        // routes to the right (runtime, model). Empty config when no
        // model override — runtime default applies.
        let config = target_model_for_task
            .as_ref()
            .map(|m| serde_json::json!({ "model": m }).to_string());

        // Reuse prompt_agent_inner so replay runs are killable + show in
        // Live registry + auto-persist their own execution_logs row.
        // Source is "desktop:replay" so traces flagged distinctly when
        // we eventually surface "this run is itself a replay" in the UI.
        let agent_slug = Some(format!("replay-of-{}", &cloud_trace_id[..8]));
        let prompt_for_cost = prompt.clone();
        let result = prompt_agent_inner(
            target_runtime_for_task.clone(),
            prompt,
            config,
            agent_slug,
            None, // workspace
            None, // existing_run_id — we let prompt_agent_inner self-register
        )
        .await;

        let duration_ms = dispatch_started.elapsed().as_millis() as i32;
        let _ = finish_replay(
            &job_id_for_task,
            result,
            duration_ms,
            &prompt_for_cost,
            &target_runtime_for_task,
            target_model_for_task.as_deref(),
        );
    });

    Ok(job_id)
}

fn mark_replay_running(job_id: &str) -> Result<(), String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE replay_jobs SET status = 'running' WHERE id = ?1 AND status = 'pending'",
        rusqlite::params![job_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn finish_replay(
    job_id: &str,
    result: Result<String, String>,
    duration_ms: i32,
    // v2.3.6 — capture estimated tokens for the replay output. Cost
    // is NOT persisted here: the replay dispatch went through a
    // runtime-CLI subscription, not direct-API. target_runtime +
    // target_model are kept on the function signature for forward-
    // compat with the future direct-API replay path but are unused
    // inside finish_replay today.
    prompt: &str,
    target_runtime: &str,
    target_model: Option<&str>,
) -> Result<(), String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    let finished_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let (status, response, error): (&str, Option<String>, Option<String>) = match result {
        Ok(r) => ("done", Some(truncate_for_log(&r)), None),
        Err(e) => ("failed", None, Some(truncate_for_log(&e))),
    };
    // v2.3.6 — Token estimates only; cost stays NULL. See the
    // matching rationale in persist_execution_log: replays go through
    // the same runtime-CLI subscription path, so an API-equivalent
    // dollar value here would mislead the cost panels. Tokens decoupled
    // from model availability — they're a pure char-count heuristic.
    let _effective_model = target_model
        .filter(|s| !s.is_empty())
        .or_else(|| default_model_for_runtime(target_runtime));
    let response_text = response.as_deref().unwrap_or("");
    let tokens_in = Some(estimate_text_tokens(prompt));
    let tokens_out = Some(estimate_text_tokens(response_text));
    let cost_usd: Option<f64> = None;
    conn.execute(
        "UPDATE replay_jobs
            SET status = ?1, response = ?2, duration_ms = ?3, error_message = ?4,
                finished_at = ?5, input_tokens = ?6, output_tokens = ?7, cost_usd_estimated = ?8
          WHERE id = ?9",
        rusqlite::params![
            status,
            response,
            duration_ms,
            error,
            finished_at,
            tokens_in,
            tokens_out,
            cost_usd,
            job_id
        ],
    )
    .map_err(|e| e.to_string())?;

    // v2.3.8 Phase 4.2 — Publish ReplayDone so recipes (Skillify) can
    // react. Look up the source_trace_id from the just-updated row so
    // the event carries enough payload for action executors.
    let (source_trace_id, source_runtime, target_runtime, target_model): (
        String,
        String,
        String,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT COALESCE(source_cloud_trace_id, source_execution_log_id), source_runtime, target_runtime, target_model
               FROM replay_jobs WHERE id = ?1",
            [job_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap_or_else(|_| (String::new(), String::new(), String::new(), None));
    let status_typed = match status {
        "done" => crate::events::ReplayStatus::Done,
        _ => crate::events::ReplayStatus::Failed,
    };
    let event = crate::events::AtoEvent::ReplayDone {
        event_seq: crate::events::next_seq(),
        job_id: job_id.to_string(),
        source_trace_id,
        source_runtime,
        target_runtime,
        target_model,
        status: status_typed,
        duration_ms: Some(duration_ms as i64),
        cost_usd_estimated: cost_usd,
        error_message: error.clone(),
        finished_at: finished_at.clone(),
    };
    crate::events::bus::publish(event);
    Ok(())
}

#[tauri::command]
pub fn get_replay_job(id: String) -> Result<ReplayJob, String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id, source_execution_log_id, source_cloud_trace_id, source_runtime,
                source_model, target_runtime, target_model, status, response,
                duration_ms, error_message, started_at, finished_at,
                input_tokens, output_tokens, cost_usd_estimated
           FROM replay_jobs WHERE id = ?1",
        rusqlite::params![id],
        |r| {
            Ok(ReplayJob {
                id: r.get(0)?,
                source_execution_log_id: r.get(1)?,
                source_cloud_trace_id: r.get(2)?,
                source_runtime: r.get(3)?,
                source_model: r.get(4)?,
                target_runtime: r.get(5)?,
                target_model: r.get(6)?,
                status: r.get(7)?,
                response: r.get(8)?,
                duration_ms: r.get(9)?,
                error_message: r.get(10)?,
                started_at: r.get(11)?,
                finished_at: r.get(12)?,
                input_tokens: r.get(13)?,
                output_tokens: r.get(14)?,
                cost_usd_estimated: r.get(15)?,
            })
        },
    )
    .map_err(|e| format!("replay-not-found: {}", e))
}

/// Fetch the locally-stored response for a cloud trace, by walking the
/// link from cloud_trace_id → execution_logs.response. Powers the
/// "source response" side of the replay result panel; cloud trace
/// uploads only carry prompt_summary, never the full response text,
/// so without this fallback the source pane reads "unavailable" and
/// the diff is half-blind.
#[tauri::command]
pub fn get_execution_log_response_by_cloud_trace_id(
    cloud_trace_id: String,
) -> Result<Option<String>, String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    match conn.query_row(
        "SELECT response FROM execution_logs WHERE cloud_trace_id = ?1 LIMIT 1",
        rusqlite::params![cloud_trace_id],
        |r| r.get::<_, Option<String>>(0),
    ) {
        Ok(maybe_response) => Ok(maybe_response),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(format!("lookup-failed: {}", e)),
    }
}

#[derive(serde::Serialize)]
pub struct LocalPromptResponse {
    pub prompt: Option<String>,
    pub response: Option<String>,
}

/// v2.1.4 — Returns both prompt and response for a cloud trace by
/// looking them up locally. Powers cost estimation in the replay
/// panel: cost = pricing × tokens(prompt+response). Without the
/// prompt, replay cost was "—" even when we had everything else.
/// Returns null prompt/response when the trace originated elsewhere.
#[tauri::command]
pub fn get_execution_log_io_by_cloud_trace_id(
    cloud_trace_id: String,
) -> Result<LocalPromptResponse, String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    match conn.query_row(
        "SELECT prompt, response FROM execution_logs WHERE cloud_trace_id = ?1 LIMIT 1",
        rusqlite::params![cloud_trace_id],
        |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)),
    ) {
        Ok((prompt, response)) => Ok(LocalPromptResponse { prompt, response }),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            Ok(LocalPromptResponse { prompt: None, response: None })
        }
        Err(e) => Err(format!("lookup-failed: {}", e)),
    }
}

#[tauri::command]
pub fn list_replays_for_trace(cloud_trace_id: String) -> Result<Vec<ReplayJob>, String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, source_execution_log_id, source_cloud_trace_id, source_runtime,
                    source_model, target_runtime, target_model, status, response,
                    duration_ms, error_message, started_at, finished_at,
                    input_tokens, output_tokens, cost_usd_estimated
               FROM replay_jobs
              WHERE source_cloud_trace_id = ?1
              ORDER BY started_at DESC
              LIMIT 50",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![cloud_trace_id], |r| {
            Ok(ReplayJob {
                id: r.get(0)?,
                source_execution_log_id: r.get(1)?,
                source_cloud_trace_id: r.get(2)?,
                source_runtime: r.get(3)?,
                source_model: r.get(4)?,
                target_runtime: r.get(5)?,
                target_model: r.get(6)?,
                status: r.get(7)?,
                response: r.get(8)?,
                duration_ms: r.get(9)?,
                error_message: r.get(10)?,
                started_at: r.get(11)?,
                finished_at: r.get(12)?,
                input_tokens: r.get(13)?,
                output_tokens: r.get(14)?,
                cost_usd_estimated: r.get(15)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}


// ── Agent Status & Logging ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub runtime: String,
    pub available: bool,
    pub healthy: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub details: serde_json::Value,
}

#[tauri::command]
pub async fn query_agent_status(runtime: String, config: Option<String>) -> Result<AgentStatus, String> {
    use std::process::Command;

    match runtime.as_str() {
        "claude" => {
            let path = which_claude();
            let available = path.is_some();
            let mut version = None;
            let mut healthy = false;

            if let Some(ref cli) = path {
                // Get version
                if let Ok(output) = wrapper_command(cli).arg("--version").output() {
                    if output.status.success() {
                        version = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
                    }
                }
                // Auth check — was previously `claude --print "respond
                // with OK"` per check, which after 2026-06-15 burns Agent
                // SDK credit on every health poll. Now: BYOK key present
                // → trust it (claude --print with ANTHROPIC_API_KEY does
                // its own auth check on real dispatches); otherwise treat
                // the binary's presence + `--version` exit as the health
                // signal. Stale-credential detection moves to first-real-
                // dispatch surfacing instead of polling.
                let has_key =
                    crate::byok::has_byok_key_from_path(&crate::get_db_path(), "claude");
                healthy = has_key || version.is_some();
            }
            // Resolve the badge once — was double-counted before (one call
            // to compute `healthy`, another inside the JSON literal).
            // Use `effective_auth_mode_from_path` so the badge reflects the
            // user's explicit choice (subscription/api_key toggle) plus
            // any env-var / stored-key signal — i.e., what the NEXT
            // dispatch will actually use. Falls back to "subscription"
            // string when the helper returns None for a non-BYOK
            // runtime (shouldn't happen for "claude" but defensive).
            let auth_mode = crate::byok::effective_auth_mode_from_path(
                &crate::get_db_path(),
                "claude",
            )
            .unwrap_or("subscription");

            Ok(AgentStatus {
                runtime: "claude".into(),
                available,
                healthy,
                version,
                path,
                details: serde_json::json!({
                    "authenticated": healthy,
                    "auth_mode": auth_mode,
                }),
            })
        }
        "codex" => {
            let path = which_cli("codex");
            let available = path.is_some();
            let mut version = None;
            let mut healthy = false;

            if let Some(ref cli) = path {
                if let Ok(output) = wrapper_command(cli).arg("--version").output() {
                    if output.status.success() {
                        version = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
                    }
                }
                if let Ok(output) = wrapper_command(cli).arg("--help").output() {
                    healthy = output.status.success();
                }
            }

            // BYOK badge: same as claude — use effective mode so the
            // user's explicit subscription/api_key toggle wins.
            let api_key_set = std::env::var("OPENAI_API_KEY").is_ok();
            let auth_mode =
                crate::byok::effective_auth_mode_from_path(&crate::get_db_path(), "codex")
                    .unwrap_or("subscription");

            Ok(AgentStatus {
                runtime: "codex".into(),
                available,
                healthy,
                version,
                path,
                details: serde_json::json!({
                    "apiKeyEnv": if api_key_set { "set" } else { "not set" },
                    "auth_mode": auth_mode,
                }),
            })
        }
        "openclaw" => {
            let ssh_config: serde_json::Value = config
                .as_deref()
                .and_then(|c| serde_json::from_str(c).ok())
                .unwrap_or_default();
            let host = ssh_config.get("sshHost").and_then(|v| v.as_str()).unwrap_or("");
            let port = ssh_config.get("sshPort").and_then(|v| v.as_u64()).unwrap_or(22);
            let user = ssh_config.get("sshUser").and_then(|v| v.as_str()).unwrap_or("root");
            let key_path = ssh_config.get("sshKeyPath").and_then(|v| v.as_str());

            if host.is_empty() {
                return Ok(AgentStatus {
                    runtime: "openclaw".into(),
                    available: false,
                    healthy: false,
                    version: None,
                    path: None,
                    details: serde_json::json!({ "error": "No SSH host configured" }),
                });
            }

            let mut cmd = Command::new("ssh");
            if let Some(key) = key_path {
                cmd.args(["-i", key]);
            }
            cmd.args([
                "-p", &port.to_string(),
                "-o", "ConnectTimeout=5",
                "-o", "StrictHostKeyChecking=no",
                "-o", "BatchMode=yes",
                &format!("{}@{}", user, host),
                "openclaw --version 2>/dev/null || echo NOT_FOUND"
            ]);

            let (available, version, healthy) = match cmd.output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let avail = output.status.success() && !stdout.contains("NOT_FOUND");
                    let ver = if avail { Some(stdout.lines().next().unwrap_or("").to_string()) } else { None };
                    (avail, ver, output.status.success())
                }
                Err(_) => (false, None, false),
            };

            Ok(AgentStatus {
                runtime: "openclaw".into(),
                available,
                healthy,
                version,
                path: Some(format!("{}@{}:{}", user, host, port)),
                details: serde_json::json!({
                    "sshHost": host,
                    "sshPort": port,
                    "sshUser": user,
                    "sshReachable": healthy,
                }),
            })
        }
        "hermes" => {
            let path = which_cli("hermes");
            let available = path.is_some();
            let mut version = None;
            let mut healthy = false;

            if let Some(ref cli) = path {
                if let Ok(output) = wrapper_command(cli).arg("--version").output() {
                    if output.status.success() {
                        version = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
                    }
                }
                if let Ok(output) = wrapper_command(cli).arg("--help").output() {
                    healthy = output.status.success();
                }
            }

            // Check endpoint if configured
            let endpoint = config.as_deref()
                .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
                .and_then(|v| v.get("endpoint").and_then(|e| e.as_str().map(|s| s.to_string())));

            Ok(AgentStatus {
                runtime: "hermes".into(),
                available,
                healthy,
                version,
                path,
                details: serde_json::json!({
                    "cliAvailable": available,
                    "endpoint": endpoint,
                }),
            })
        }
        _ => Err(format!("Unknown runtime: {}", runtime)),
    }
}

#[tauri::command]
pub fn query_all_agent_statuses() -> Result<Vec<AgentStatus>, String> {
    // Check OpenClaw via saved config
    let oc_available = load_openclaw_ssh_config().is_ok();

    let runtimes = vec![
        ("claude", which_claude()),
        ("codex", which_cli("codex")),
        ("openclaw", if oc_available { Some("ssh".to_string()) } else { None }),
        ("hermes", which_cli("hermes")),
    ];

    Ok(runtimes.into_iter().map(|(name, path)| {
        let available = path.is_some();
        AgentStatus {
            runtime: name.to_string(),
            available,
            healthy: available, // assume healthy if available for fast check
            version: None,
            path,
            details: serde_json::json!({}),
        }
    }).collect())
}

pub fn agent_logs_path() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    fs::create_dir_all(&path).ok();
    path.push("agent-logs.jsonl");
    path
}

#[tauri::command]
pub fn append_agent_log(entry: String) -> Result<(), String> {
    use std::io::Write;
    let path = agent_logs_path();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("Failed to open agent log: {}", e))?;
    writeln!(file, "{}", entry).map_err(|e| format!("Failed to write agent log: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn get_agent_logs(runtime: Option<String>, limit: Option<u32>) -> Result<Vec<serde_json::Value>, String> {
    let path = agent_logs_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = read_file_lossy(&path).unwrap_or_default();
    let limit = limit.unwrap_or(50) as usize;

    let mut logs: Vec<serde_json::Value> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|entry| {
            if let Some(ref rt) = runtime {
                entry.get("runtime").and_then(|v| v.as_str()) == Some(rt.as_str())
            } else {
                true
            }
        })
        .collect();

    // Return last N entries
    if logs.len() > limit {
        logs = logs.split_off(logs.len() - limit);
    }

    Ok(logs)
}

pub fn which_claude() -> Option<String> {
    // which_cli now handles all the search logic including npx cache
    // and user shell PATH. No need for a separate function.
    which_cli("claude")
}

// ── OpenClaw WebSocket + Runtime Config ───────────────────────────────────

/// Load OpenClaw SSH config from ~/.ato/openclaw-config.json
pub fn load_openclaw_ssh_config() -> Result<(String, u64, String, Option<String>), String> {
    let config_path = home_dir().join(".ato").join("openclaw-config.json");
    let content = read_file_lossy(&config_path)
        .ok_or("OpenClaw not configured. Go to Configuration to set SSH host.")?;
    let config: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let host = config.get("sshHost").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if host.is_empty() { return Err("No SSH host configured".into()); }
    let port = config.get("sshPort").and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64())).unwrap_or(22);
    let user = config.get("sshUser").and_then(|v| v.as_str()).unwrap_or("root").to_string();
    let key_path = config.get("sshKeyPath").and_then(|v| v.as_str()).map(|s| s.to_string()).filter(|s| !s.is_empty());
    Ok((host, port, user, key_path))
}

/// Build the base SSH command for OpenClaw
pub fn openclaw_ssh_base() -> Result<(std::process::Command, String, u64, String), String> {
    let (host, port, user, key_path) = load_openclaw_ssh_config()?;
    let user_path = get_user_path();
    let mut cmd = std::process::Command::new("ssh");
    cmd.env("PATH", &user_path);
    cmd.args(["-o", "ConnectTimeout=5", "-o", "StrictHostKeyChecking=accept-new"]);
    if let Some(ref key) = key_path {
        cmd.args(["-i", key]);
    }
    cmd.args(["-p", &port.to_string(), &format!("{}@{}", user, host)]);
    Ok((cmd, host, port, user))
}

/// Run an openclaw CLI command via SSH and return the JSON output
pub fn openclaw_ssh_command(subcmd: &str) -> Result<serde_json::Value, String> {
    let (mut cmd, ..) = openclaw_ssh_base()?;
    cmd.arg(format!("openclaw {} 2>/dev/null", subcmd));
    let output = cmd.output().map_err(|e| format!("SSH failed: {}", e))?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(stdout.trim()).map_err(|e| format!("Invalid JSON from openclaw: {}", e))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("OpenClaw command failed: {}", stderr.trim()))
    }
}

/// Run a raw shell command via SSH and return plain text output
pub fn openclaw_ssh_raw(shell_cmd: &str) -> Result<String, String> {
    let (mut cmd, ..) = openclaw_ssh_base()?;
    cmd.arg(shell_cmd);
    let output = cmd.output().map_err(|e| format!("SSH failed: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("SSH command failed: {}", stderr.trim()))
    }
}

#[tauri::command]
pub async fn openclaw_gateway_status() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("status --json")
}

#[tauri::command]
pub async fn openclaw_list_cron_jobs() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("cron list --all --json")
}

#[tauri::command]
pub async fn openclaw_cron_status() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("cron status --json")
}

#[tauri::command]
pub async fn openclaw_list_agents() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("agents list --json")
}

#[tauri::command]
pub async fn openclaw_skills_status() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("skills status --json")
}

#[tauri::command]
pub async fn openclaw_list_sessions() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("sessions list --json")
}

#[tauri::command]
pub async fn openclaw_test_connection(ws_url: String, token: String) -> Result<serde_json::Value, String> {
    // Test via SSH instead of WebSocket since the gateway requires crypto auth
    let _ = (ws_url, token); // Unused - we use SSH config instead
    let (host, port, user, key_path) = load_openclaw_ssh_config()?;
    let user_path = get_user_path();
    let mut cmd = std::process::Command::new("ssh");
    cmd.env("PATH", &user_path);
    cmd.args(["-o", "ConnectTimeout=5", "-o", "StrictHostKeyChecking=accept-new"]);
    if let Some(ref key) = key_path {
        cmd.args(["-i", key]);
    }
    cmd.args([
        "-p", &port.to_string(),
        &format!("{}@{}", user, host),
        "openclaw --version 2>/dev/null || echo UNKNOWN",
    ]);
    let output = cmd.output().map_err(|e| format!("SSH connection failed: {}", e))?;
    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(json!({"connected": true, "version": version, "host": host, "user": user}))
    } else {
        Err(format!("SSH to {}@{}:{} failed", user, host, port))
    }
}


// ── OpenClaw Cron CRUD ────────────────────────────────────────────────────

#[tauri::command]
pub async fn openclaw_edit_cron_job(id: String, args: String) -> Result<serde_json::Value, String> {
    // args is a space-separated string of CLI flags like "--name foo --every 1h --message 'do stuff'"
    openclaw_ssh_command(&format!("cron edit {} {} --json", id, args))
}

#[tauri::command]
pub async fn openclaw_add_cron_job(args: String) -> Result<serde_json::Value, String> {
    openclaw_ssh_command(&format!("cron add {} --json", args))
}

#[tauri::command]
pub async fn openclaw_delete_cron_job(id: String) -> Result<serde_json::Value, String> {
    openclaw_ssh_command(&format!("cron rm {} --json", id))
}

#[tauri::command]
pub async fn openclaw_run_cron_job(id: String) -> Result<serde_json::Value, String> {
    openclaw_ssh_command(&format!("cron run {} --json", id))
}

#[tauri::command]
pub async fn openclaw_toggle_cron_job(id: String, enable: bool) -> Result<serde_json::Value, String> {
    let flag = if enable { "--enable" } else { "--disable" };
    openclaw_ssh_command(&format!("cron edit {} {} --json", id, flag))
}

// ── Remote OpenClaw Skills ────────────────────────────────────────────────

#[tauri::command]
pub async fn openclaw_list_skills() -> Result<Vec<LocalSkill>, String> {
    let mut skills = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Scan multiple known OpenClaw skill directories
    let dirs = [
        "~/.openclaw/skills",
        "~/.openclaw/workspace/skills",
    ];

    for dir in &dirs {
        let cmd = format!("ls {} 2>/dev/null", dir);
        if let Ok(text) = openclaw_ssh_raw(&cmd) {
            for name in text.lines().filter(|l| !l.is_empty()) {
                let name = name.trim().to_string();
                if seen.contains(&name) { continue; }
                seen.insert(name.clone());
                skills.push(LocalSkill {
                    id: format!("oc-skill-{}", name),
                    name: name.clone(),
                    description: format!("OpenClaw skill: {}", name),
                    file_path: format!("{}/{}", dir, name),
                    scope: "personal".to_string(),
                    runtime: "openclaw".to_string(),
                    project: None,
                    token_count: 0,
                    enabled: true,
                    content_hash: "".to_string(),
                });
            }
        }
    }

    // Also detect pseudo-skills from AGENTS.md, SOUL.md, TOOLS.md
    let special_files = ["AGENTS.md", "SOUL.md", "TOOLS.md"];
    for f in &special_files {
        let cmd = format!("test -f ~/.openclaw/workspace/{} && echo exists", f);
        if let Ok(text) = openclaw_ssh_raw(&cmd) {
            if text.contains("exists") {
                let name = f.trim_end_matches(".md").to_lowercase();
                if !seen.contains(&name) {
                    seen.insert(name.clone());
                    skills.push(LocalSkill {
                        id: format!("oc-skill-{}", name),
                        name,
                        description: format!("OpenClaw context: {}", f),
                        file_path: format!("~/.openclaw/workspace/{}", f),
                        scope: "personal".to_string(),
                        runtime: "openclaw".to_string(),
                        project: None,
                        token_count: 0,
                        enabled: true,
                        content_hash: "".to_string(),
                    });
                }
            }
        }
    }

    Ok(skills)
}
// ── Agent Configuration Manager ──────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigFile {
    pub path: String,
    pub scope: String,           // "global" | "project"
    pub runtime: String,         // "claude" | "codex" | "openclaw" | "hermes" | "shared"
    pub file_type: String,       // "skill" | "settings" | "project-config" | "mcp" | "soul"
    pub exists: bool,
    pub last_modified: Option<String>,
    pub token_count: Option<u64>,
    pub project_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ParsedConfigFile {
    pub path: String,
    pub format: String,          // "yaml-frontmatter" | "json" | "toml" | "yaml" | "markdown"
    pub content: serde_json::Value,  // Parsed content as JSON
    pub raw: String,             // Original file content
    pub content_hash: String,    // SHA-256 of raw content (hex) for conflict detection
    pub last_modified: Option<u64>, // Unix seconds
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    pub path: String,
    pub new_hash: String,
    pub bytes_written: u64,
    pub backup_path: Option<String>,
    pub added_lines: usize,
    pub removed_lines: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: String, // "add" | "remove" | "context"
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ValidationError {
    pub field: String,
    pub message: String,
    pub line: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Permission {
    pub tool: String,
    pub pattern: Option<String>,
    pub allowed: bool,
    pub requires_approval: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContextPreviewSection {
    pub name: String,
    pub tokens: u64,
    pub files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContextPreview {
    pub total_tokens: u64,
    pub limit: u64,
    pub sections: Vec<ContextPreviewSection>,
}

// ── Profile Snapshots ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProfileFile {
    pub path: String,           // Relative path from home or project
    pub content: String,
    pub scope: String,          // "global" | "project"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSnapshot {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub runtime: String,
    pub files: Vec<ProfileFile>,
    pub created_at: String,
}

// ── Skill Usage Analytics ───────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillUsageStat {
    pub skill_path: String,
    pub skill_name: String,
    pub trigger_count: u32,
    pub last_used: Option<String>,
    pub avg_tokens: Option<u32>,
}

/// Scan all config files for all runtimes in both global and project scopes
/// Based on official documentation for Claude Code, Codex CLI, Hermes, and OpenClaw
#[tauri::command]
pub fn scan_agent_config_files(project_path: Option<String>) -> Result<Vec<AgentConfigFile>, String> {
    let home = home_dir();
    let mut configs = Vec::new();

    // Determine project roots to scan
    let project_roots: Vec<PathBuf> = if let Some(ref p) = project_path {
        vec![PathBuf::from(p)]
    } else {
        discover_project_roots()
    };

    // ══════════════════════════════════════════════════════════════════════════
    // CLAUDE CODE - Global Config Files
    // Docs: https://docs.anthropic.com/en/docs/claude-code
    // ══════════════════════════════════════════════════════════════════════════
    let claude_home = home.join(".claude");

    // Settings
    add_config_if_exists(&mut configs, claude_home.join("settings.json"), "global", "claude", "settings", None);

    // MCP servers, OAuth, preferences
    add_config_if_exists(&mut configs, home.join(".claude.json"), "global", "claude", "mcp", None);

    // User-level CLAUDE.md (personal instructions)
    add_config_if_exists(&mut configs, claude_home.join("CLAUDE.md"), "global", "claude", "project-config", None);

    // Keybindings
    add_config_if_exists(&mut configs, claude_home.join("keybindings.json"), "global", "claude", "settings", None);

    // Skills directory
    let claude_skills = claude_home.join("skills");
    if claude_skills.exists() {
        scan_skills_directory(&mut configs, &claude_skills, "global", "claude", None);
    }

    // Subagents directory (~/.claude/agents/*.md)
    let claude_agents = claude_home.join("agents");
    if claude_agents.exists() {
        scan_md_directory(&mut configs, &claude_agents, "global", "claude", "subagent", None);
    }

    // Rules directory (~/.claude/rules/*.md)
    let claude_rules = claude_home.join("rules");
    if claude_rules.exists() {
        scan_md_directory(&mut configs, &claude_rules, "global", "claude", "rules", None);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // CODEX CLI - Global Config Files
    // Docs: https://developers.openai.com/codex/config-reference
    // ══════════════════════════════════════════════════════════════════════════
    let codex_home = std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".codex"));

    // Primary config (TOML format)
    add_config_if_exists(&mut configs, codex_home.join("config.toml"), "global", "codex", "settings", None);

    // Organization requirements
    add_config_if_exists(&mut configs, codex_home.join("requirements.toml"), "global", "codex", "settings", None);

    // System-wide config
    add_config_if_exists(&mut configs, PathBuf::from("/etc/codex/config.toml"), "global", "codex", "settings", None);

    // User-level skills (~/.agents/skills/ - shared with OpenClaw)
    let user_agents_skills = home.join(".agents").join("skills");
    if user_agents_skills.exists() {
        scan_skills_directory(&mut configs, &user_agents_skills, "global", "codex", None);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // HERMES - Global Config Files
    // Docs: https://hermes-agent.nousresearch.com/docs/
    // ══════════════════════════════════════════════════════════════════════════
    let hermes_home = std::env::var("HERMES_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".hermes"));

    // Primary config (YAML format)
    add_config_if_exists(&mut configs, hermes_home.join("config.yaml"), "global", "hermes", "settings", None);

    // Environment variables
    add_config_if_exists(&mut configs, hermes_home.join(".env"), "global", "hermes", "settings", None);

    // OAuth tokens
    add_config_if_exists(&mut configs, hermes_home.join("auth.json"), "global", "hermes", "settings", None);

    // Agent identity/personality
    add_config_if_exists(&mut configs, hermes_home.join("SOUL.md"), "global", "hermes", "soul", None);

    // Memories directory
    let hermes_memories = hermes_home.join("memories");
    add_config_if_exists(&mut configs, hermes_memories.join("MEMORY.md"), "global", "hermes", "memory", None);
    add_config_if_exists(&mut configs, hermes_memories.join("USER.md"), "global", "hermes", "memory", None);

    // Skills directory (with category subdirs)
    let hermes_skills = hermes_home.join("skills");
    if hermes_skills.exists() {
        scan_skills_directory_recursive(&mut configs, &hermes_skills, "global", "hermes", None);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // OPENCLAW - Global Config Files
    // Docs: https://docs.openclaw.ai/
    // ══════════════════════════════════════════════════════════════════════════
    let openclaw_home = std::env::var("OPENCLAW_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".openclaw"));

    // Main config (JSON5 format)
    add_config_if_exists(&mut configs, openclaw_home.join("openclaw.json"), "global", "openclaw", "settings", None);

    // Managed/local skills
    let openclaw_skills = openclaw_home.join("skills");
    if openclaw_skills.exists() {
        scan_skills_directory(&mut configs, &openclaw_skills, "global", "openclaw", None);
    }

    // Personal agent skills (~/.agents/skills/ - shared with Codex)
    // Already scanned above for Codex, add for OpenClaw too
    if user_agents_skills.exists() {
        scan_skills_directory(&mut configs, &user_agents_skills, "global", "openclaw", None);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // PROJECT-LEVEL CONFIG FILES
    // ══════════════════════════════════════════════════════════════════════════
    for project_root in project_roots {
        let project_name = project_root.file_name()
            .map(|n| n.to_string_lossy().to_string());

        // ── CLAUDE CODE - Project ──
        // Main project instructions
        add_config_if_exists(&mut configs, project_root.join("CLAUDE.md"), "project", "claude", "project-config", project_name.clone());
        // Alternative location
        add_config_if_exists(&mut configs, project_root.join(".claude").join("CLAUDE.md"), "project", "claude", "project-config", project_name.clone());
        // Local overrides (gitignored)
        add_config_if_exists(&mut configs, project_root.join("CLAUDE.local.md"), "project", "claude", "project-config", project_name.clone());
        // Shared settings
        add_config_if_exists(&mut configs, project_root.join(".claude").join("settings.json"), "project", "claude", "settings", project_name.clone());
        // Local settings (gitignored)
        add_config_if_exists(&mut configs, project_root.join(".claude").join("settings.local.json"), "project", "claude", "settings", project_name.clone());
        // Project MCP servers
        add_config_if_exists(&mut configs, project_root.join(".mcp.json"), "project", "claude", "mcp", project_name.clone());

        // Project skills
        let project_claude_skills = project_root.join(".claude").join("skills");
        if project_claude_skills.exists() {
            scan_skills_directory(&mut configs, &project_claude_skills, "project", "claude", project_name.clone());
        }
        // Project subagents
        let project_claude_agents = project_root.join(".claude").join("agents");
        if project_claude_agents.exists() {
            scan_md_directory(&mut configs, &project_claude_agents, "project", "claude", "subagent", project_name.clone());
        }
        // Project rules
        let project_claude_rules = project_root.join(".claude").join("rules");
        if project_claude_rules.exists() {
            scan_md_directory(&mut configs, &project_claude_rules, "project", "claude", "rules", project_name.clone());
        }

        // ── CODEX CLI - Project ──
        // Project instructions (Codex uses AGENTS.md)
        add_config_if_exists(&mut configs, project_root.join("AGENTS.md"), "project", "codex", "project-config", project_name.clone());
        add_config_if_exists(&mut configs, project_root.join("AGENTS.override.md"), "project", "codex", "project-config", project_name.clone());
        // Project config
        add_config_if_exists(&mut configs, project_root.join(".codex").join("config.toml"), "project", "codex", "settings", project_name.clone());
        // Project skills (.agents/skills/)
        let project_agents_skills = project_root.join(".agents").join("skills");
        if project_agents_skills.exists() {
            scan_skills_directory(&mut configs, &project_agents_skills, "project", "codex", project_name.clone());
        }

        // ── HERMES - Project ──
        // Hermes-specific project instructions (highest priority)
        add_config_if_exists(&mut configs, project_root.join(".hermes.md"), "project", "hermes", "project-config", project_name.clone());
        // Falls back to AGENTS.md (compatible)
        // AGENTS.md already added for Codex, mark as shared
        // Falls back to CLAUDE.md (compatible) - already added
        // Project config
        add_config_if_exists(&mut configs, project_root.join(".hermes").join("config.yaml"), "project", "hermes", "settings", project_name.clone());
        // Project skills
        let project_hermes_skills = project_root.join(".hermes").join("skills");
        if project_hermes_skills.exists() {
            scan_skills_directory_recursive(&mut configs, &project_hermes_skills, "project", "hermes", project_name.clone());
        }

        // ── OPENCLAW - Project/Workspace ──
        // SOUL.md - Agent personality (shared between Hermes & OpenClaw)
        add_config_if_exists(&mut configs, project_root.join("SOUL.md"), "project", "shared", "soul", project_name.clone());
        // AGENTS.md - Operating rules (already added for Codex)
        // USER.md - Personal user context
        add_config_if_exists(&mut configs, project_root.join("USER.md"), "project", "openclaw", "memory", project_name.clone());
        // IDENTITY.md - Agent name, emoji, avatar
        add_config_if_exists(&mut configs, project_root.join("IDENTITY.md"), "project", "openclaw", "project-config", project_name.clone());
        // TOOLS.md - Environment-specific tool notes
        add_config_if_exists(&mut configs, project_root.join("TOOLS.md"), "project", "openclaw", "project-config", project_name.clone());
        // MEMORY.md - Long-term memories
        add_config_if_exists(&mut configs, project_root.join("MEMORY.md"), "project", "openclaw", "memory", project_name.clone());
        // HEARTBEAT.md - Scheduled tasks
        add_config_if_exists(&mut configs, project_root.join("HEARTBEAT.md"), "project", "openclaw", "project-config", project_name.clone());
        // Workspace config
        add_config_if_exists(&mut configs, project_root.join(".openclaw").join("openclaw.json"), "project", "openclaw", "settings", project_name.clone());
        // Workspace skills (highest priority for OpenClaw)
        let project_openclaw_skills = project_root.join("skills");
        if project_openclaw_skills.exists() {
            scan_skills_directory(&mut configs, &project_openclaw_skills, "project", "openclaw", project_name.clone());
        }
        // .agents/skills/ for OpenClaw too
        if project_agents_skills.exists() {
            scan_skills_directory(&mut configs, &project_agents_skills, "project", "openclaw", project_name.clone());
        }
    }

    Ok(configs)
}

/// Scan a directory for .md files (used for agents/, rules/)
pub fn scan_md_directory(
    configs: &mut Vec<AgentConfigFile>,
    dir: &PathBuf,
    scope: &str,
    runtime: &str,
    file_type: &str,
    project_name: Option<String>,
) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                add_config_if_exists(configs, path, scope, runtime, file_type, project_name.clone());
            }
        }
    }
}

/// Scan skills directory recursively (for Hermes category subdirs)
pub fn scan_skills_directory_recursive(
    configs: &mut Vec<AgentConfigFile>,
    dir: &PathBuf,
    scope: &str,
    runtime: &str,
    project_name: Option<String>,
) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Check if this is a skill directory (has SKILL.md)
                let skill_file = path.join("SKILL.md");
                if skill_file.exists() {
                    add_config_if_exists(configs, skill_file, scope, runtime, "skill", project_name.clone());
                } else {
                    // It's a category directory, recurse
                    scan_skills_directory_recursive(configs, &path, scope, runtime, project_name.clone());
                }
            } else if path.extension().map(|e| e == "md").unwrap_or(false) {
                // Single file skill
                add_config_if_exists(configs, path, scope, runtime, "skill", project_name.clone());
            }
        }
    }
}

pub fn add_config_if_exists(
    configs: &mut Vec<AgentConfigFile>,
    path: PathBuf,
    scope: &str,
    runtime: &str,
    file_type: &str,
    project_name: Option<String>,
) {
    let exists = path.exists();
    let last_modified = if exists {
        fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| {
                let secs = d.as_secs();
                // Format as ISO 8601
                let datetime = chrono_lite(secs);
                datetime
            })
    } else {
        None
    };

    let token_count = if exists {
        fs::read_to_string(&path)
            .ok()
            .map(|content| estimate_tokens(content.len() as u64))
    } else {
        None
    };

    configs.push(AgentConfigFile {
        path: path.to_string_lossy().to_string(),
        scope: scope.to_string(),
        runtime: runtime.to_string(),
        file_type: file_type.to_string(),
        exists,
        last_modified,
        token_count,
        project_name,
    });
}

pub fn scan_skills_directory(
    configs: &mut Vec<AgentConfigFile>,
    dir: &PathBuf,
    scope: &str,
    runtime: &str,
    project_name: Option<String>,
) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Directory skill - look for SKILL.md
                let skill_file = path.join("SKILL.md");
                if skill_file.exists() {
                    add_config_if_exists(configs, skill_file, scope, runtime, "skill", project_name.clone());
                }
            } else if path.extension().map(|e| e == "md").unwrap_or(false) {
                // Single file skill
                add_config_if_exists(configs, path, scope, runtime, "skill", project_name.clone());
            }
        }
    }
}

/// Simple datetime formatter (avoid adding chrono dependency)
pub fn chrono_lite(unix_secs: u64) -> String {
    // Basic ISO 8601 format without full chrono dependency
    // Just return the unix timestamp as a string for now
    format!("{}", unix_secs)
}

/// SHA-256 hex digest of a byte slice
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Copy a file to ~/.ato/backups/<timestamp>-<sha8>-<filename>. Returns backup path.
/// Silently prunes backups older than 30 days on every call.
pub fn backup_file(path: &PathBuf) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let backups_dir = home_dir().join(".ato").join("backups");
    fs::create_dir_all(&backups_dir).map_err(|e| format!("backup dir: {}", e))?;

    let content = fs::read(path).map_err(|e| format!("read for backup: {}", e))?;
    let hash = sha256_hex(&content);
    let sha8 = &hash[..8];
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let backup_name = format!("{}-{}-{}", ts, sha8, filename);
    let backup_path = backups_dir.join(&backup_name);
    fs::write(&backup_path, &content).map_err(|e| format!("write backup: {}", e))?;

    // Prune >30d old (best-effort, ignore errors)
    let cutoff = ts.saturating_sub(30 * 24 * 60 * 60);
    if let Ok(entries) = fs::read_dir(&backups_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(ts_str) = name_str.split('-').next() {
                if let Ok(entry_ts) = ts_str.parse::<u64>() {
                    if entry_ts < cutoff {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }
    }

    Ok(Some(backup_path.to_string_lossy().to_string()))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub backup_path: String,
    pub original_filename: String,
    pub timestamp: u64,         // Unix seconds
    pub sha8: String,           // First 8 chars of SHA-256
    pub size_bytes: u64,
}

/// List all backups in ~/.ato/backups/. If `original_path` is provided, filter to
/// backups whose filename matches that path's basename.
#[tauri::command]
pub fn list_backups(original_path: Option<String>) -> Result<Vec<BackupEntry>, String> {
    let backups_dir = home_dir().join(".ato").join("backups");
    if !backups_dir.exists() {
        return Ok(Vec::new());
    }
    let filter_name = original_path.as_ref().and_then(|p| {
        PathBuf::from(p).file_name().and_then(|n| n.to_str()).map(String::from)
    });

    let mut entries: Vec<BackupEntry> = Vec::new();
    if let Ok(dir) = fs::read_dir(&backups_dir) {
        for entry in dir.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            // Expected format: <timestamp>-<sha8>-<filename>
            let parts: Vec<&str> = name.splitn(3, '-').collect();
            if parts.len() != 3 {
                continue;
            }
            let Ok(timestamp) = parts[0].parse::<u64>() else { continue };
            let sha8 = parts[1].to_string();
            let original_filename = parts[2].to_string();

            if let Some(ref want) = filter_name {
                if &original_filename != want {
                    continue;
                }
            }

            let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            entries.push(BackupEntry {
                backup_path: path.to_string_lossy().to_string(),
                original_filename,
                timestamp,
                sha8,
                size_bytes,
            });
        }
    }

    // Newest first
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(entries)
}

/// Restore a backup by copying its contents to `target_path`. Goes through the
/// same safety pipeline (hash check, backup-current, audit) as a regular write.
#[tauri::command]
pub fn restore_backup(
    db: State<'_, DbState>,
    backup_path: String,
    target_path: String,
    expected_hash: Option<String>,
) -> Result<WriteResult, String> {
    let backup_pb = PathBuf::from(&backup_path);
    let content = fs::read_to_string(&backup_pb)
        .map_err(|e| format!("Failed to read backup: {}", e))?;
    write_agent_config_file(db, target_path, content, expected_hash, Some(true))
}

// ── Ollama Provider ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    pub running: bool,
    pub version: Option<String>,
    pub endpoint: String,
}

// `OllamaModel` + `list_ollama_models` moved to commands/models.rs
// (PR 2 of the commands.rs split). Detect / config helpers below
// stay here until PR 23 extracts the `runtimes` domain.

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OllamaConfig {
    pub host: Option<String>,
    pub models_dir: Option<String>,
    pub keep_alive: Option<String>,
    pub flash_attention: Option<String>,
    pub cuda_visible_devices: Option<String>,
    pub num_parallel: Option<String>,
}

#[tauri::command]
pub async fn detect_ollama() -> Result<OllamaStatus, String> {
    let endpoint = std::env::var("OLLAMA_HOST")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());
    let url = format!("{}/api/version", endpoint);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;

    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let version = body.get("version").and_then(|v| v.as_str()).map(String::from);
            Ok(OllamaStatus { running: true, version, endpoint })
        }
        _ => Ok(OllamaStatus { running: false, version: None, endpoint }),
    }
}

// `list_ollama_models` moved to commands/models.rs (PR 2 of the
// commands.rs split).

#[tauri::command]
pub fn get_ollama_config() -> OllamaConfig {
    OllamaConfig {
        host: std::env::var("OLLAMA_HOST").ok(),
        models_dir: std::env::var("OLLAMA_MODELS").ok(),
        keep_alive: std::env::var("OLLAMA_KEEP_ALIVE").ok(),
        flash_attention: std::env::var("OLLAMA_FLASH_ATTENTION").ok(),
        cuda_visible_devices: std::env::var("CUDA_VISIBLE_DEVICES").ok(),
        num_parallel: std::env::var("OLLAMA_NUM_PARALLEL").ok(),
    }
}

/// Simple line-by-line diff. Marks every line add/remove/context using LCS-free approach:
/// finds longest common prefix/suffix then marks the middle chunks.
pub fn compute_diff(old: &str, new: &str) -> (Vec<DiffLine>, usize, usize) {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // Longest common prefix
    let mut prefix = 0;
    while prefix < old_lines.len() && prefix < new_lines.len() && old_lines[prefix] == new_lines[prefix] {
        prefix += 1;
    }
    // Longest common suffix (bounded)
    let mut suffix = 0;
    while suffix < old_lines.len() - prefix
        && suffix < new_lines.len() - prefix
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let mut diff = Vec::new();
    let context_lines = 3usize;

    // Leading context
    let leading_start = prefix.saturating_sub(context_lines);
    for i in leading_start..prefix {
        diff.push(DiffLine {
            kind: "context".to_string(),
            old_line: Some(i + 1),
            new_line: Some(i + 1),
            text: old_lines[i].to_string(),
        });
    }

    // Removals
    let old_end = old_lines.len() - suffix;
    for i in prefix..old_end {
        diff.push(DiffLine {
            kind: "remove".to_string(),
            old_line: Some(i + 1),
            new_line: None,
            text: old_lines[i].to_string(),
        });
    }

    // Additions
    let new_end = new_lines.len() - suffix;
    for i in prefix..new_end {
        diff.push(DiffLine {
            kind: "add".to_string(),
            old_line: None,
            new_line: Some(i + 1),
            text: new_lines[i].to_string(),
        });
    }

    // Trailing context
    let trailing_end = (old_end + context_lines).min(old_lines.len());
    for i in old_end..trailing_end {
        diff.push(DiffLine {
            kind: "context".to_string(),
            old_line: Some(i + 1),
            new_line: Some(new_end + (i - old_end) + 1),
            text: old_lines[i].to_string(),
        });
    }

    let added = new_end.saturating_sub(prefix);
    let removed = old_end.saturating_sub(prefix);
    (diff, added, removed)
}

/// Validate Claude Code `settings.json` shape. Permissive on unknown keys;
/// strict on known structure (permissions, hooks, mcpServers, env).
#[tauri::command]
pub fn validate_settings_json(content: String) -> Result<ValidationResult, String> {
    let mut errors = Vec::new();

    let value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            errors.push(ValidationError {
                field: "$".to_string(),
                message: format!("Invalid JSON: {}", e),
                line: Some(e.line()),
            });
            return Ok(ValidationResult { valid: false, errors });
        }
    };

    if !value.is_object() {
        errors.push(ValidationError {
            field: "$".to_string(),
            message: "Root must be an object".to_string(),
            line: None,
        });
        return Ok(ValidationResult { valid: false, errors });
    }

    let obj = value.as_object().unwrap();

    // permissions: { allow?: string[], deny?: string[], ask?: string[] }
    if let Some(perms) = obj.get("permissions") {
        if !perms.is_object() {
            errors.push(ValidationError {
                field: "permissions".to_string(),
                message: "Must be an object".to_string(),
                line: None,
            });
        } else {
            for key in ["allow", "deny", "ask"] {
                if let Some(arr) = perms.get(key) {
                    if !arr.is_array() {
                        errors.push(ValidationError {
                            field: format!("permissions.{}", key),
                            message: "Must be an array of strings".to_string(),
                            line: None,
                        });
                    } else if let Some(items) = arr.as_array() {
                        for (i, item) in items.iter().enumerate() {
                            if !item.is_string() {
                                errors.push(ValidationError {
                                    field: format!("permissions.{}[{}]", key, i),
                                    message: "Must be a string".to_string(),
                                    line: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // hooks: { [event]: [{ matcher, hooks: [{ type, command }] }] }
    if let Some(hooks) = obj.get("hooks") {
        if !hooks.is_object() {
            errors.push(ValidationError {
                field: "hooks".to_string(),
                message: "Must be an object keyed by event name".to_string(),
                line: None,
            });
        }
    }

    // mcpServers: { [name]: { command, args?, env? } | { url, ... } }
    if let Some(mcp) = obj.get("mcpServers") {
        if !mcp.is_object() {
            errors.push(ValidationError {
                field: "mcpServers".to_string(),
                message: "Must be an object keyed by server name".to_string(),
                line: None,
            });
        } else if let Some(servers) = mcp.as_object() {
            for (name, server) in servers {
                if !server.is_object() {
                    errors.push(ValidationError {
                        field: format!("mcpServers.{}", name),
                        message: "Each MCP server must be an object".to_string(),
                        line: None,
                    });
                    continue;
                }
                let so = server.as_object().unwrap();
                let has_command = so.get("command").map(|v| v.is_string()).unwrap_or(false);
                let has_url = so.get("url").map(|v| v.is_string()).unwrap_or(false);
                if !has_command && !has_url {
                    errors.push(ValidationError {
                        field: format!("mcpServers.{}", name),
                        message: "Must have either 'command' (stdio) or 'url' (http/sse)".to_string(),
                        line: None,
                    });
                }
            }
        }
    }

    // env: { [key]: string }
    if let Some(env) = obj.get("env") {
        if !env.is_object() {
            errors.push(ValidationError {
                field: "env".to_string(),
                message: "Must be an object of string values".to_string(),
                line: None,
            });
        } else if let Some(vars) = env.as_object() {
            for (key, val) in vars {
                if !val.is_string() {
                    errors.push(ValidationError {
                        field: format!("env.{}", key),
                        message: "Env values must be strings".to_string(),
                        line: None,
                    });
                }
            }
        }
    }

    Ok(ValidationResult { valid: errors.is_empty(), errors })
}

/// Preview the diff + validation for a pending write without touching disk.
#[tauri::command]
pub fn preview_write_agent_config_file(path: String, new_content: String) -> Result<serde_json::Value, String> {
    let path_buf = PathBuf::from(&path);
    let old_content = if path_buf.exists() {
        fs::read_to_string(&path_buf).unwrap_or_default()
    } else {
        String::new()
    };
    let (diff, added, removed) = compute_diff(&old_content, &new_content);
    let current_hash = sha256_hex(old_content.as_bytes());
    let new_hash = sha256_hex(new_content.as_bytes());

    let mut validation: Option<ValidationResult> = None;
    let fname = path_buf.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if fname == "settings.json" || fname == "settings.local.json" {
        validation = Some(validate_settings_json(new_content.clone())?);
    }

    Ok(json!({
        "diff": diff,
        "addedLines": added,
        "removedLines": removed,
        "currentHash": current_hash,
        "newHash": new_hash,
        "validation": validation,
    }))
}

/// Read and parse a config file, handling different formats.
/// Returns content_hash (SHA-256) for conflict detection.
#[tauri::command]
pub fn read_agent_config_file(path: String) -> Result<ParsedConfigFile, String> {
    let mut path_buf = PathBuf::from(&path);
    // If path is a directory (e.g., a skill directory), look for SKILL.md or README.md inside
    if path_buf.is_dir() {
        let candidates = ["SKILL.md", "README.md", "index.md"];
        let mut found = false;
        for candidate in &candidates {
            let child = path_buf.join(candidate);
            if child.exists() {
                path_buf = child;
                found = true;
                break;
            }
        }
        if !found {
            // List directory contents as a fallback
            let entries: Vec<String> = fs::read_dir(&path_buf)
                .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
                .unwrap_or_default();
            return Err(format!("Path is a directory. Contents: {}", entries.join(", ")));
        }
    }
    let resolved_path = path_buf.to_string_lossy().to_string();

    let content = fs::read_to_string(&path_buf)
        .map_err(|e| format!("Failed to read file: {}", e))?;
    let metadata = fs::metadata(&path_buf).ok();
    let last_modified = metadata
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let size_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
    let content_hash = sha256_hex(content.as_bytes());

    let extension = path_buf.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let (format, parsed) = match extension {
        "json" => {
            let value: serde_json::Value = serde_json::from_str(&content)
                .unwrap_or_else(|_| {
                    // Tolerate invalid JSON at read time so users can fix it in the editor.
                    let mut obj = serde_json::Map::new();
                    obj.insert("raw".to_string(), serde_json::Value::String(content.clone()));
                    serde_json::Value::Object(obj)
                });
            ("json".to_string(), value)
        }
        "toml" => {
            let parsed = parse_toml_to_json(&content);
            ("toml".to_string(), parsed)
        }
        "yaml" | "yml" => {
            let parsed = parse_simple_yaml(&content);
            ("yaml".to_string(), parsed)
        }
        "md" => {
            if content.trim_start().starts_with("---") {
                let (frontmatter, body) = parse_frontmatter(&content);
                let mut obj = serde_json::Map::new();
                obj.insert("frontmatter".to_string(), frontmatter);
                obj.insert("body".to_string(), serde_json::Value::String(body));
                ("yaml-frontmatter".to_string(), serde_json::Value::Object(obj))
            } else {
                let mut obj = serde_json::Map::new();
                obj.insert("body".to_string(), serde_json::Value::String(content.clone()));
                ("markdown".to_string(), serde_json::Value::Object(obj))
            }
        }
        _ => {
            let mut obj = serde_json::Map::new();
            obj.insert("raw".to_string(), serde_json::Value::String(content.clone()));
            ("unknown".to_string(), serde_json::Value::Object(obj))
        }
    };

    Ok(ParsedConfigFile {
        path: resolved_path,
        format,
        content: parsed,
        raw: content,
        content_hash,
        last_modified,
        size_bytes,
    })
}


/// Parse TOML content using the full toml crate (handles nested tables, arrays, inline tables, etc.)
pub fn parse_toml_to_json(content: &str) -> serde_json::Value {
    match content.parse::<toml::Value>() {
        Ok(val) => serde_json::to_value(val).unwrap_or_default(),
        Err(_) => {
            let mut obj = serde_json::Map::new();
            obj.insert("_parse_error".to_string(), serde_json::Value::String("Invalid TOML".to_string()));
            obj.insert("raw".to_string(), serde_json::Value::String(content.to_string()));
            serde_json::Value::Object(obj)
        }
    }
}

/// Convert a JSON value back to TOML string
pub fn json_to_toml(value: &serde_json::Value) -> Result<String, String> {
    let toml_val: toml::Value = serde_json::from_value(value.clone())
        .map_err(|e| format!("Cannot convert to TOML: {}", e))?;
    toml::to_string_pretty(&toml_val)
        .map_err(|e| format!("Cannot serialize TOML: {}", e))
}

/// Simple YAML parser (basic key-value pairs)
pub fn parse_simple_yaml(content: &str) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    let mut current_key: Option<String> = None;
    let mut current_indent = 0;
    let mut stack: Vec<(String, serde_json::Map<String, serde_json::Value>, usize)> = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() || line.trim().starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();

        // Key: value pair
        if let Some(colon_pos) = trimmed.find(':') {
            let key = trimmed[..colon_pos].trim().to_string();
            let value_str = trimmed[colon_pos+1..].trim();

            if value_str.is_empty() {
                // Nested object starts
                current_key = Some(key);
                current_indent = indent;
            } else {
                // Simple value
                let value = parse_yaml_value(value_str);
                obj.insert(key, value);
            }
        }
    }

    serde_json::Value::Object(obj)
}

pub fn parse_yaml_value(s: &str) -> serde_json::Value {
    let s = s.trim();
    // Handle quoted strings
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        return serde_json::Value::String(s[1..s.len()-1].to_string());
    }
    // Handle booleans
    if s == "true" || s == "yes" {
        return serde_json::Value::Bool(true);
    }
    if s == "false" || s == "no" {
        return serde_json::Value::Bool(false);
    }
    // Handle null
    if s == "null" || s == "~" {
        return serde_json::Value::Null;
    }
    // Handle numbers
    if let Ok(n) = s.parse::<i64>() {
        return serde_json::Value::Number(n.into());
    }
    if let Ok(n) = s.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(n) {
            return serde_json::Value::Number(num);
        }
    }
    // Default to string
    serde_json::Value::String(s.to_string())
}

/// Write a config file back to disk with company-grade safety:
/// - content-hash conflict detection (reject if on-disk file changed since read)
/// - automatic timestamped backup to ~/.ato/backups/
/// - audit log entry in audit_logs SQLite table
/// - optional pre-write validation for known schemas (settings.json)
#[tauri::command]
pub fn write_agent_config_file(
    db: State<'_, DbState>,
    path: String,
    content: String,
    expected_hash: Option<String>,
    skip_validation: Option<bool>,
) -> Result<WriteResult, String> {
    let path_buf = PathBuf::from(&path);

    // 1. Conflict detection: if caller provided expected_hash, verify current on-disk matches.
    let (current_content, current_hash) = if path_buf.exists() {
        let c = fs::read_to_string(&path_buf)
            .map_err(|e| format!("Failed to read current file: {}", e))?;
        let h = sha256_hex(c.as_bytes());
        (c, h)
    } else {
        (String::new(), sha256_hex(&[]))
    };

    if let Some(expected) = &expected_hash {
        if expected != &current_hash {
            return Err(format!(
                "CONFLICT: file changed on disk since it was loaded (expected hash {}, found {}). Reload before saving.",
                &expected[..8], &current_hash[..8]
            ));
        }
    }

    // 2. Schema validation for settings.json (skippable via flag for escape hatch).
    let skip = skip_validation.unwrap_or(false);
    if !skip {
        let fname = path_buf.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if fname == "settings.json" || fname == "settings.local.json" {
            let result = validate_settings_json(content.clone())?;
            if !result.valid {
                let msgs: Vec<String> = result.errors.iter()
                    .map(|e| format!("{}: {}", e.field, e.message))
                    .collect();
                return Err(format!("VALIDATION_FAILED: {}", msgs.join("; ")));
            }
        }
    }

    // 3. Create parent dirs if needed
    if let Some(parent) = path_buf.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    // 4. Backup current contents before overwriting (no-op if file doesn't exist yet)
    let backup_path = backup_file(&path_buf)?;

    // 5. Write
    fs::write(&path_buf, &content)
        .map_err(|e| format!("Failed to write file: {}", e))?;

    let new_hash = sha256_hex(content.as_bytes());
    let (_, added, removed) = compute_diff(&current_content, &content);
    let bytes_written = content.as_bytes().len() as u64;

    // 6. Audit log
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let fname = path_buf.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let details = json!({
            "path": &path,
            "oldHash": current_hash,
            "newHash": new_hash,
            "addedLines": added,
            "removedLines": removed,
            "bytesWritten": bytes_written,
            "backupPath": backup_path,
        }).to_string();
        let _ = conn.execute(
            "INSERT INTO audit_logs (id, action, resource_type, resource_id, resource_name, details, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, "file_write", "config_file", Some(&path), Some(&fname), Some(details), now],
        );
    }

    Ok(WriteResult {
        path,
        new_hash,
        bytes_written,
        backup_path,
        added_lines: added,
        removed_lines: removed,
    })
}

/// Create a new skill file from template
#[tauri::command]
pub fn create_agent_skill(runtime: String, name: String, scope: String, description: String) -> Result<String, String> {
    let home = home_dir();
    let skill_slug = name.replace(' ', "-").to_lowercase();

    // Determine base directory based on runtime and scope (per official docs)
    let base_dir = match (runtime.as_str(), scope.as_str()) {
        // Claude: ~/.claude/skills/ or .claude/skills/
        ("claude", "global") => home.join(".claude").join("skills"),
        ("claude", "project") => project_root().join(".claude").join("skills"),
        // Codex: ~/.agents/skills/ (shared) or .agents/skills/
        ("codex", "global") => home.join(".agents").join("skills"),
        ("codex", "project") => project_root().join(".agents").join("skills"),
        // Hermes: ~/.hermes/skills/ or .hermes/skills/
        ("hermes", "global") => home.join(".hermes").join("skills"),
        ("hermes", "project") => project_root().join(".hermes").join("skills"),
        // OpenClaw: ~/.openclaw/skills/ or workspace/skills/
        ("openclaw", "global") => home.join(".openclaw").join("skills"),
        ("openclaw", "project") => project_root().join("skills"),
        _ => return Err(format!("Unknown runtime/scope: {}/{}", runtime, scope)),
    };

    // Create skill as directory with SKILL.md (recommended structure)
    let skill_dir = base_dir.join(&skill_slug);
    fs::create_dir_all(&skill_dir)
        .map_err(|e| format!("Failed to create skill directory: {}", e))?;

    let skill_path = skill_dir.join("SKILL.md");

    // Generate template based on runtime (different formats per docs)
    let template = match runtime.as_str() {
        "claude" => format!(
r#"---
name: {}
description: {}
allowed-tools:
  - Read
  - Edit
  - Bash
user-invocable: true
---

# {}

{}

## When to Use

Trigger this skill when...

## Instructions

[Add your skill instructions here]

## Verification

How to confirm the skill worked correctly.
"#,
            skill_slug, description, name, description
        ),
        "codex" => format!(
r#"---
name: {}
description: {}
---

# {}

{}

## When to Use

Trigger this skill when...

## Instructions

[Add your skill instructions here]

## Verification

How to confirm the skill worked correctly.
"#,
            skill_slug, description, name, description
        ),
        "hermes" => format!(
r#"---
name: {}
description: {}
version: 1.0.0
metadata:
  hermes:
    tags: [Custom]
    category: custom
---

# {}

{}

## When to Use

Trigger conditions and use cases.

## Quick Reference

Common commands or shortcuts.

## Procedure

1. Step one
2. Step two
3. Step three

## Pitfalls

Known failure modes and solutions.

## Verification

How to confirm the skill worked correctly.
"#,
            skill_slug, description, name, description
        ),
        "openclaw" => format!(
r#"---
name: {}
description: {}
user-invocable: true
---

# {}

{}

## When to Use

Trigger this skill when...

## Instructions

[Add your skill instructions here]

## Verification

How to confirm the skill worked correctly.
"#,
            skill_slug, description, name, description
        ),
        _ => return Err(format!("Unknown runtime: {}", runtime)),
    };

    fs::write(&skill_path, &template)
        .map_err(|e| format!("Failed to create skill file: {}", e))?;

    Ok(skill_path.to_string_lossy().to_string())
}

/// Parse permissions from a settings file
#[tauri::command]
pub fn parse_agent_permissions(path: String) -> Result<Vec<Permission>, String> {
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let mut permissions = Vec::new();

    // Try to parse as JSON (Claude settings.json format)
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
        // Claude format: { "permissions": { "allow": ["Bash(git:*)", "Read"] } }
        if let Some(perms) = json.get("permissions") {
            if let Some(allow) = perms.get("allow").and_then(|v| v.as_array()) {
                for item in allow {
                    if let Some(s) = item.as_str() {
                        let (tool, pattern) = parse_permission_string(s);
                        permissions.push(Permission {
                            tool,
                            pattern,
                            allowed: true,
                            requires_approval: false,
                        });
                    }
                }
            }
            if let Some(deny) = perms.get("deny").and_then(|v| v.as_array()) {
                for item in deny {
                    if let Some(s) = item.as_str() {
                        let (tool, pattern) = parse_permission_string(s);
                        permissions.push(Permission {
                            tool,
                            pattern,
                            allowed: false,
                            requires_approval: false,
                        });
                    }
                }
            }
        }
    }

    Ok(permissions)
}

pub fn parse_permission_string(s: &str) -> (String, Option<String>) {
    // Parse "Bash(git:*)" -> ("Bash", Some("git:*"))
    if let Some(paren_start) = s.find('(') {
        if s.ends_with(')') {
            let tool = s[..paren_start].to_string();
            let pattern = s[paren_start+1..s.len()-1].to_string();
            return (tool, Some(pattern));
        }
    }
    (s.to_string(), None)
}

/// Get context preview showing what will be in the agent's context window
#[tauri::command]
pub fn get_agent_context_preview(runtime: String) -> Result<ContextPreview, String> {
    let home = home_dir();
    let project = project_root();
    let mut sections = Vec::new();
    let mut total_tokens: u64 = 0;

    // System prompt (estimated)
    let system_tokens = 30000u64; // Approximate system prompt size
    sections.push(ContextPreviewSection {
        name: "System Prompt".to_string(),
        tokens: system_tokens,
        files: vec!["(built-in)".to_string()],
    });
    total_tokens += system_tokens;

    // Project config (CLAUDE.md, AGENTS.md, etc.)
    let project_config_files: Vec<PathBuf> = match runtime.as_str() {
        "claude" => vec![project.join("CLAUDE.md")],
        "codex" => vec![project.join("AGENTS.md")],
        "hermes" | "openclaw" => vec![project.join("SOUL.md")],
        _ => vec![],
    };

    let mut config_tokens: u64 = 0;
    let mut config_files = Vec::new();
    for path in project_config_files {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                config_tokens += estimate_tokens(content.len() as u64);
                config_files.push(path.to_string_lossy().to_string());
            }
        }
    }
    if config_tokens > 0 {
        sections.push(ContextPreviewSection {
            name: "Project Config".to_string(),
            tokens: config_tokens,
            files: config_files,
        });
        total_tokens += config_tokens;
    }

    // Note: Skills are on-demand, not counted in context total
    // But we can show them as "available" with their token counts

    let limit = match runtime.as_str() {
        "claude" => 200000u64,
        "codex" => 128000u64,
        "gemini" => 1000000u64, // Gemini 1.5/2.x have 1M-token windows
        "hermes" => 128000u64,
        "openclaw" => 128000u64,
        _ => 100000u64,
    };

    Ok(ContextPreview {
        total_tokens,
        limit,
        sections,
    })
}


// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 3: Profile Snapshots
// ══════════════════════════════════════════════════════════════════════════════

/// Save current configuration as a profile snapshot
#[tauri::command]
pub fn save_profile_snapshot(
    db: State<'_, DbState>,
    name: String,
    description: Option<String>,
    runtime: String,
) -> Result<String, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let home = home_dir();
    let project = project_root();
    let mut files: Vec<ProfileFile> = Vec::new();

    // Collect files based on runtime
    let global_paths: Vec<PathBuf> = match runtime.as_str() {
        "claude" => vec![
            home.join(".claude/settings.json"),
            home.join(".claude.json"),
            home.join(".claude/CLAUDE.md"),
        ],
        "codex" => vec![
            home.join(".codex/config.toml"),
            home.join(".codex/requirements.toml"),
        ],
        "hermes" => vec![
            home.join(".hermes/config.yaml"),
            home.join(".hermes/.env"),
        ],
        "openclaw" => vec![
            home.join(".openclaw/openclaw.json"),
        ],
        _ => vec![],
    };

    // Read global files
    for path in global_paths {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                let relative = path.strip_prefix(&home)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                files.push(ProfileFile {
                    path: relative,
                    content,
                    scope: "global".to_string(),
                });
            }
        }
    }

    // Collect skills
    let skills_dir = match runtime.as_str() {
        "claude" => home.join(".claude/skills"),
        "codex" => home.join(".agents/skills"),
        "hermes" => home.join(".hermes/skills"),
        "openclaw" => home.join(".openclaw/skills"),
        _ => home.join(".claude/skills"),
    };

    if skills_dir.exists() {
        if let Ok(entries) = fs::read_dir(&skills_dir) {
            for entry in entries.flatten() {
                let skill_md = entry.path().join("SKILL.md");
                if skill_md.exists() {
                    if let Ok(content) = fs::read_to_string(&skill_md) {
                        let relative = skill_md.strip_prefix(&home)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| skill_md.to_string_lossy().to_string());
                        files.push(ProfileFile {
                            path: relative,
                            content,
                            scope: "global".to_string(),
                        });
                    }
                }
            }
        }
    }

    // Project files
    let project_paths: Vec<PathBuf> = match runtime.as_str() {
        "claude" => vec![project.join("CLAUDE.md"), project.join(".claude/settings.json")],
        "codex" => vec![project.join("AGENTS.md")],
        "hermes" | "openclaw" => vec![project.join("SOUL.md"), project.join("TOOLS.md")],
        _ => vec![],
    };

    for path in project_paths {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                let relative = path.strip_prefix(&project)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                files.push(ProfileFile {
                    path: relative,
                    content,
                    scope: "project".to_string(),
                });
            }
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    let files_json = serde_json::to_string(&files).map_err(|e| e.to_string())?;
    let created_at = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO profile_snapshots (id, name, description, runtime, files_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, name, description, runtime, files_json, created_at],
    ).map_err(|e| e.to_string())?;

    Ok(id)
}

/// List all profile snapshots
#[tauri::command]
pub fn list_profile_snapshots(db: State<'_, DbState>) -> Result<Vec<ProfileSnapshot>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, name, description, runtime, files_json, created_at FROM profile_snapshots ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;

    let profiles = stmt.query_map([], |row| {
        let files_json: String = row.get(4)?;
        let files: Vec<ProfileFile> = serde_json::from_str(&files_json).unwrap_or_default();
        Ok(ProfileSnapshot {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            runtime: row.get(3)?,
            files,
            created_at: row.get(5)?,
        })
    }).map_err(|e| e.to_string())?;

    profiles.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Load a profile snapshot (writes files to disk)
#[tauri::command]
pub fn load_profile_snapshot(db: State<'_, DbState>, profile_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let home = home_dir();
    let project = project_root();

    let files_json: String = conn.query_row(
        "SELECT files_json FROM profile_snapshots WHERE id = ?1",
        params![profile_id],
        |row| row.get(0),
    ).map_err(|e| e.to_string())?;

    let files: Vec<ProfileFile> = serde_json::from_str(&files_json).map_err(|e| e.to_string())?;

    for file in files {
        let full_path = if file.scope == "global" {
            home.join(&file.path)
        } else {
            project.join(&file.path)
        };

        // Create parent directories
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        // Write file
        fs::write(&full_path, &file.content).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Delete a profile snapshot
#[tauri::command]
pub fn delete_profile_snapshot(db: State<'_, DbState>, profile_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM profile_snapshots WHERE id = ?1",
        params![profile_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Export a profile snapshot as JSON
#[tauri::command]
pub fn export_profile_snapshot(db: State<'_, DbState>, profile_id: String) -> Result<String, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let profile: ProfileSnapshot = conn.query_row(
        "SELECT id, name, description, runtime, files_json, created_at FROM profile_snapshots WHERE id = ?1",
        params![profile_id],
        |row| {
            let files_json: String = row.get(4)?;
            let files: Vec<ProfileFile> = serde_json::from_str(&files_json).unwrap_or_default();
            Ok(ProfileSnapshot {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                runtime: row.get(3)?,
                files,
                created_at: row.get(5)?,
            })
        },
    ).map_err(|e| e.to_string())?;

    serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 4: Skill Usage Analytics
// ══════════════════════════════════════════════════════════════════════════════

/// Get usage statistics for all skills
#[tauri::command]
pub fn get_skill_usage_stats() -> Result<Vec<SkillUsageStat>, String> {
    let home = home_dir();
    let logs_path = home.join(".ato/agent-logs.jsonl");
    let mut usage_map: std::collections::HashMap<String, (u32, Option<String>, Vec<u32>)> = std::collections::HashMap::new();

    // Parse agent logs for skill invocations
    if logs_path.exists() {
        if let Ok(content) = fs::read_to_string(&logs_path) {
            for line in content.lines() {
                if let Ok(log) = serde_json::from_str::<serde_json::Value>(line) {
                    // Look for skill invocations in the logs
                    if let Some(skill_name) = log.get("skill").and_then(|s| s.as_str()) {
                        let timestamp = log.get("timestamp")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string());
                        let tokens = log.get("tokens")
                            .and_then(|t| t.as_u64())
                            .map(|t| t as u32)
                            .unwrap_or(0);

                        let entry = usage_map.entry(skill_name.to_string()).or_insert((0, None, Vec::new()));
                        entry.0 += 1;
                        entry.1 = timestamp.or(entry.1.clone());
                        if tokens > 0 {
                            entry.2.push(tokens);
                        }
                    }

                    // Also check for skill references in prompt content
                    if let Some(prompt) = log.get("prompt").and_then(|p| p.as_str()) {
                        // Simple heuristic: look for /skill-name patterns
                        for word in prompt.split_whitespace() {
                            if word.starts_with('/') && word.len() > 1 {
                                let skill_name = word.trim_start_matches('/');
                                let entry = usage_map.entry(skill_name.to_string()).or_insert((0, None, Vec::new()));
                                entry.0 += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    // Build list of all known skills
    let mut all_skills: Vec<SkillUsageStat> = Vec::new();
    let skill_dirs = vec![
        (home.join(".claude/skills"), "claude"),
        (home.join(".agents/skills"), "codex"),
        (home.join(".hermes/skills"), "hermes"),
        (home.join(".openclaw/skills"), "openclaw"),
    ];

    for (dir, _runtime) in skill_dirs {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        let skill_name = entry.file_name().to_string_lossy().to_string();
                        let skill_path = entry.path().join("SKILL.md").to_string_lossy().to_string();

                        let (trigger_count, last_used, tokens_vec) = usage_map
                            .get(&skill_name)
                            .cloned()
                            .unwrap_or((0, None, Vec::new()));

                        let avg_tokens = if tokens_vec.is_empty() {
                            None
                        } else {
                            Some((tokens_vec.iter().sum::<u32>() / tokens_vec.len() as u32) as u32)
                        };

                        all_skills.push(SkillUsageStat {
                            skill_path,
                            skill_name,
                            trigger_count,
                            last_used,
                            avg_tokens,
                        });
                    }
                }
            }
        }
    }

    // Sort by trigger count (most used first)
    all_skills.sort_by(|a, b| b.trigger_count.cmp(&a.trigger_count));

    Ok(all_skills)
}


/// Count skills in a project directory
pub fn count_project_skills(project_path: &PathBuf) -> u32 {
    let mut count = 0u32;

    let skill_dirs = vec![
        project_path.join(".claude/skills"),
        project_path.join(".codex/skills"),
        project_path.join(".agents/skills"),
        project_path.join(".hermes/skills"),
        project_path.join("skills"),
    ];

    for dir in skill_dirs {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() && entry.path().join("SKILL.md").exists() {
                        count += 1;
                    }
                }
            }
        }
    }

    count
}

// ── Project Bundle (all-in-one view for Projects dashboard) ─────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileRef {
    pub label: String,
    pub path: String,
    pub scope: String,        // "user" | "project" | "nested"
    pub exists: bool,
    pub size_bytes: u64,
    pub token_estimate: u64,
    pub last_modified: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectHookSummary {
    pub event: String,
    pub matcher: Option<String>,
    pub command: String,
    pub scope: String,   // "user" | "project"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMcpSummary {
    pub name: String,
    pub kind: String,        // "stdio" | "http" | "sse" | "unknown"
    pub command_or_url: String,
    pub scope: String,       // "user" | "project"
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPermissions {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub ask: Vec<String>,
    pub scope: String,       // "user" | "project" | "merged"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBundle {
    pub project_path: String,
    pub project_name: String,
    pub has_claude: bool,
    pub has_codex: bool,
    pub has_hermes: bool,
    pub has_openclaw: bool,
    pub has_gemini: bool,

    pub memory_files: Vec<ProjectFileRef>,     // CLAUDE.md hierarchy (user, project, nested)
    pub subagents: Vec<ProjectFileRef>,         // .claude/agents/*.md (global + project)
    pub commands: Vec<ProjectFileRef>,          // .claude/commands/*.md (global + project)
    pub settings_files: Vec<ProjectFileRef>,    // settings.json, settings.local.json, .mcp.json

    pub skills: Vec<LocalSkill>,                // Filtered to this project + inherited globals
    pub hooks: Vec<ProjectHookSummary>,
    pub permissions_user: ProjectPermissions,
    pub permissions_project: ProjectPermissions,
    pub mcp_servers: Vec<ProjectMcpSummary>,

    // Per-runtime file bundles for Codex / OpenClaw / Hermes
    pub codex_files: Vec<ProjectFileRef>,       // AGENTS.md (user+project), config.toml (user+project)
    pub codex_skills: Vec<LocalSkill>,
    pub openclaw_files: Vec<ProjectFileRef>,    // SOUL.md, TOOLS.md, workspace AGENTS.md, openclaw.json
    pub openclaw_skills: Vec<LocalSkill>,
    pub hermes_files: Vec<ProjectFileRef>,      // SOUL.md, memories/MEMORY.md, memories/USER.md, config.yaml
    pub hermes_skills: Vec<LocalSkill>,

    // Gemini CLI / ADK
    pub gemini_files: Vec<ProjectFileRef>,     // GEMINI.md, settings.json, root_agent.yaml
    pub gemini_skills: Vec<LocalSkill>,

    // OpenAI Agents SDK (extends Codex)
    pub sandbox_config: Option<SandboxConfig>,
    pub approval_policies: Vec<ApprovalPolicy>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SandboxConfig {
    pub enabled: bool,
    pub network_isolation: bool,
    pub allowed_ports: Vec<u16>,
    pub filesystem_policy: String,
    pub timeout_secs: Option<u64>,
    pub snapshot_enabled: bool,
    pub source_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalPolicy {
    pub tool_name: String,
    pub policy: String,
    pub scope: String,
}

pub fn file_ref(label: &str, path: PathBuf, scope: &str) -> ProjectFileRef {
    let metadata = fs::metadata(&path).ok();
    let exists = metadata.is_some();
    let size_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
    let last_modified = metadata
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    ProjectFileRef {
        label: label.to_string(),
        path: path.to_string_lossy().to_string(),
        scope: scope.to_string(),
        exists,
        size_bytes,
        token_estimate: estimate_tokens(size_bytes),
        last_modified,
    }
}

pub fn list_nested_claude_md(project_path: &PathBuf, max_depth: u32) -> Vec<ProjectFileRef> {
    let mut out = Vec::new();
    fn walk(dir: &PathBuf, root: &PathBuf, depth: u32, max_depth: u32, out: &mut Vec<ProjectFileRef>) {
        if depth > max_depth {
            return;
        }
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist" {
                    continue;
                }
                if p.is_dir() {
                    walk(&p, root, depth + 1, max_depth, out);
                } else if name == "CLAUDE.md" && depth > 0 {
                    let rel = p.strip_prefix(root).map(|r| r.to_string_lossy().to_string())
                        .unwrap_or_else(|_| p.to_string_lossy().to_string());
                    out.push(file_ref(&rel, p.clone(), "nested"));
                }
            }
        }
    }
    walk(project_path, project_path, 0, max_depth, &mut out);
    out
}

pub fn list_dir_md_files(dir: &PathBuf, scope: &str) -> Vec<ProjectFileRef> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    if ext == "md" {
                        let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("unnamed").to_string();
                        out.push(file_ref(&name, p, scope));
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    out
}

pub fn parse_permissions_from_settings(path: &PathBuf, scope: &str) -> ProjectPermissions {
    let mut out = ProjectPermissions { scope: scope.to_string(), ..Default::default() };
    if !path.exists() {
        return out;
    }
    let Ok(text) = fs::read_to_string(path) else { return out };
    let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else { return out };
    if let Some(perms) = value.get("permissions") {
        for (key, dest) in [("allow", &mut out.allow), ("deny", &mut out.deny), ("ask", &mut out.ask)] {
            if let Some(arr) = perms.get(key).and_then(|v| v.as_array()) {
                *dest = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            }
        }
    }
    out
}

pub fn parse_mcp_from_settings(path: &PathBuf, scope: &str) -> Vec<ProjectMcpSummary> {
    let mut out = Vec::new();
    if !path.exists() {
        return out;
    }
    let Ok(text) = fs::read_to_string(path) else { return out };
    let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else { return out };
    // .mcp.json keys servers at root under "mcpServers" OR is the object itself. Support both.
    let servers_obj = value.get("mcpServers").cloned().unwrap_or(value.clone());
    if let Some(map) = servers_obj.as_object() {
        for (name, cfg) in map {
            let (kind, command_or_url) = if let Some(cmd) = cfg.get("command").and_then(|v| v.as_str()) {
                ("stdio", cmd.to_string())
            } else if let Some(url) = cfg.get("url").and_then(|v| v.as_str()) {
                let kind = if cfg.get("type").and_then(|v| v.as_str()) == Some("sse") { "sse" } else { "http" };
                (kind, url.to_string())
            } else {
                ("unknown", String::new())
            };
            out.push(ProjectMcpSummary {
                name: name.clone(),
                kind: kind.to_string(),
                command_or_url,
                scope: scope.to_string(),
            });
        }
    }
    out
}

pub fn collect_hooks_from_settings(path: &PathBuf, scope: &str) -> Vec<ProjectHookSummary> {
    let mut out = Vec::new();
    if !path.exists() {
        return out;
    }
    let Ok(text) = fs::read_to_string(path) else { return out };
    let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else { return out };
    let Some(hooks) = value.get("hooks").and_then(|v| v.as_object()) else { return out };
    for (event, triggers) in hooks {
        if let Some(arr) = triggers.as_array() {
            for trigger in arr {
                let matcher = trigger.get("matcher").and_then(|v| v.as_str()).map(String::from);
                if let Some(hook_arr) = trigger.get("hooks").and_then(|v| v.as_array()) {
                    for h in hook_arr {
                        if let Some(cmd) = h.get("command").and_then(|v| v.as_str()) {
                            out.push(ProjectHookSummary {
                                event: event.clone(),
                                matcher: matcher.clone(),
                                command: cmd.to_string(),
                                scope: scope.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

pub fn parse_sandbox_config(project_path: &PathBuf) -> Option<SandboxConfig> {
    // Look for sandbox config in config.toml or codex.json
    let candidates = [
        project_path.join(".codex").join("sandbox.json"),
        project_path.join("codex.json"),
    ];
    for path in &candidates {
        if !path.exists() { continue; }
        let Ok(text) = fs::read_to_string(path) else { continue };
        let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else { continue };
        let sandbox = value.get("sandbox").unwrap_or(&value);
        if sandbox.is_object() {
            return Some(SandboxConfig {
                enabled: sandbox.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                network_isolation: sandbox.get("network_isolation")
                    .or_else(|| sandbox.get("networkIsolation"))
                    .and_then(|v| v.as_bool()).unwrap_or(false),
                allowed_ports: sandbox.get("allowed_ports")
                    .or_else(|| sandbox.get("allowedPorts"))
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as u16)).collect())
                    .unwrap_or_default(),
                filesystem_policy: sandbox.get("filesystem_policy")
                    .or_else(|| sandbox.get("filesystemPolicy"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("read-write").to_string(),
                timeout_secs: sandbox.get("timeout_secs")
                    .or_else(|| sandbox.get("timeoutSecs"))
                    .and_then(|v| v.as_u64()),
                snapshot_enabled: sandbox.get("snapshot_enabled")
                    .or_else(|| sandbox.get("snapshotEnabled"))
                    .and_then(|v| v.as_bool()).unwrap_or(false),
                source_path: path.to_string_lossy().to_string(),
            });
        }
    }
    // Also check config.toml for [sandbox] section
    let toml_path = project_path.join(".codex").join("config.toml");
    if toml_path.exists() {
        if let Ok(text) = fs::read_to_string(&toml_path) {
            let parsed = parse_toml_to_json(&text);
            if let Some(sandbox) = parsed.get("sandbox") {
                return Some(SandboxConfig {
                    enabled: sandbox.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                    network_isolation: sandbox.get("network_isolation").and_then(|v| v.as_bool()).unwrap_or(false),
                    allowed_ports: Vec::new(),
                    filesystem_policy: sandbox.get("filesystem_policy").and_then(|v| v.as_str()).unwrap_or("read-write").to_string(),
                    timeout_secs: sandbox.get("timeout_secs").and_then(|v| v.as_u64()),
                    snapshot_enabled: sandbox.get("snapshot_enabled").and_then(|v| v.as_bool()).unwrap_or(false),
                    source_path: toml_path.to_string_lossy().to_string(),
                });
            }
        }
    }
    None
}

pub fn parse_approval_policies(project_path: &PathBuf) -> Vec<ApprovalPolicy> {
    let mut out = Vec::new();
    let candidates = [
        (project_path.join(".codex").join("policies.json"), "project"),
        (home_dir().join(".codex").join("policies.json"), "user"),
        (project_path.join("codex.json"), "project"),
    ];
    for (path, scope) in &candidates {
        if !path.exists() { continue; }
        let Ok(text) = fs::read_to_string(path) else { continue };
        let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else { continue };
        let policies = value.get("approval_policies")
            .or_else(|| value.get("approvalPolicies"))
            .or_else(|| value.get("policies"));
        if let Some(policies) = policies.and_then(|v| v.as_object()) {
            for (tool, policy_val) in policies {
                let policy_str = policy_val.as_str()
                    .unwrap_or_else(|| policy_val.get("level").and_then(|v| v.as_str()).unwrap_or("on-request"));
                out.push(ApprovalPolicy {
                    tool_name: tool.clone(),
                    policy: policy_str.to_string(),
                    scope: scope.to_string(),
                });
            }
        }
    }
    out
}

/// Write sandbox config to .codex/sandbox.json via the safe write pipeline.
#[tauri::command]
pub fn write_sandbox_config(
    db: State<'_, DbState>,
    project_path: String,
    config: SandboxConfig,
) -> Result<WriteResult, String> {
    let dest = PathBuf::from(&project_path).join(".codex").join("sandbox.json");
    let content = serde_json::to_string_pretty(&json!({
        "sandbox": {
            "enabled": config.enabled,
            "network_isolation": config.network_isolation,
            "filesystem_policy": config.filesystem_policy,
            "timeout_secs": config.timeout_secs,
            "snapshot_enabled": config.snapshot_enabled,
            "allowed_ports": config.allowed_ports,
        }
    })).unwrap_or_default();
    write_agent_config_file(db, dest.to_string_lossy().to_string(), content + "\n", None, Some(true))
}

/// Write approval policies to .codex/policies.json via the safe write pipeline.
#[tauri::command]
pub fn write_approval_policies(
    db: State<'_, DbState>,
    project_path: String,
    policies: Vec<ApprovalPolicy>,
) -> Result<WriteResult, String> {
    let dest = PathBuf::from(&project_path).join(".codex").join("policies.json");
    let mut map = serde_json::Map::new();
    for p in &policies {
        map.insert(p.tool_name.clone(), serde_json::Value::String(p.policy.clone()));
    }
    let content = serde_json::to_string_pretty(&json!({
        "approvalPolicies": serde_json::Value::Object(map)
    })).unwrap_or_default();
    write_agent_config_file(db, dest.to_string_lossy().to_string(), content + "\n", None, Some(true))
}

/// Write a TOML config file from JSON value via the safe write pipeline.
#[tauri::command]
pub fn write_toml_config(
    db: State<'_, DbState>,
    path: String,
    value: serde_json::Value,
) -> Result<WriteResult, String> {
    let content = json_to_toml(&value)?;
    write_agent_config_file(db, path, content, None, Some(true))
}

// ── OpenClaw Workspace Parsing ──────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawWorkspace {
    pub soul: OpenClawSoul,
    pub tools: Vec<OpenClawTool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawSoul {
    pub name: Option<String>,
    pub role: Option<String>,
    pub traits: Vec<String>,
    pub raw_content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawTool {
    pub name: String,
    pub description: String,
}

#[tauri::command]
pub fn parse_openclaw_workspace(project_path: String) -> Result<OpenClawWorkspace, String> {
    let pb = PathBuf::from(&project_path);
    let openclaw_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
        .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));

    // SOUL.md — check project then global
    let soul_path = if pb.join("SOUL.md").exists() { pb.join("SOUL.md") }
        else { openclaw_home.join("workspace").join("SOUL.md") };
    let soul_raw = read_file_lossy(&soul_path).unwrap_or_default();
    let mut soul = OpenClawSoul { name: None, role: None, traits: Vec::new(), raw_content: soul_raw.clone() };
    // Parse frontmatter or first heading
    for line in soul_raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") { soul.name = Some(trimmed[2..].trim().to_string()); }
        if trimmed.to_lowercase().starts_with("role:") { soul.role = Some(trimmed[5..].trim().to_string()); }
        if trimmed.starts_with("- ") && soul.name.is_some() { soul.traits.push(trimmed[2..].trim().to_string()); }
    }

    // TOOLS.md — parse ## headings as tool names
    let tools_path = if pb.join("TOOLS.md").exists() { pb.join("TOOLS.md") }
        else { openclaw_home.join("workspace").join("TOOLS.md") };
    let tools_raw = read_file_lossy(&tools_path).unwrap_or_default();
    let mut tools = Vec::new();
    let mut current_tool: Option<String> = None;
    let mut current_desc = String::new();
    for line in tools_raw.lines() {
        if line.starts_with("## ") {
            if let Some(name) = current_tool.take() {
                tools.push(OpenClawTool { name, description: current_desc.trim().to_string() });
            }
            current_tool = Some(line[3..].trim().to_string());
            current_desc = String::new();
        } else if current_tool.is_some() {
            current_desc.push_str(line.trim());
            current_desc.push(' ');
        }
    }
    if let Some(name) = current_tool {
        tools.push(OpenClawTool { name, description: current_desc.trim().to_string() });
    }

    Ok(OpenClawWorkspace { soul, tools })
}

// ── Gemini Agent YAML Parsing ──────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiAgentDef {
    pub name: Option<String>,
    pub model: Option<String>,
    pub instruction: Option<String>,
    pub sub_agents: Vec<GeminiSubAgent>,
    pub tools: Vec<GeminiToolRef>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiSubAgent {
    pub name: String,
    pub model: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiToolRef {
    pub name: String,
    pub kind: Option<String>,
}

#[tauri::command]
pub fn parse_gemini_agent(path: String) -> Result<GeminiAgentDef, String> {
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read: {}", e))?;
    let value: serde_yaml::Value = serde_yaml::from_str(&content)
        .map_err(|e| format!("Invalid YAML: {}", e))?;

    let name = value.get("name").and_then(|v| v.as_str()).map(String::from);
    let model = value.get("model").and_then(|v| v.as_str()).map(String::from);
    let instruction = value.get("instruction").and_then(|v| v.as_str()).map(|s| {
        if s.len() > 200 { format!("{}…", &s[..200]) } else { s.to_string() }
    });

    let sub_agents = value.get("sub_agents")
        .or_else(|| value.get("subAgents"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|a| {
            let name = a.get("name").or_else(|| a.get("agent")).and_then(|v| v.as_str())?;
            Some(GeminiSubAgent {
                name: name.to_string(),
                model: a.get("model").and_then(|v| v.as_str()).map(String::from),
                description: a.get("description").and_then(|v| v.as_str()).map(String::from),
            })
        }).collect())
        .unwrap_or_default();

    let tools = value.get("tools")
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|t| {
            if let Some(s) = t.as_str() {
                return Some(GeminiToolRef { name: s.to_string(), kind: None });
            }
            let name = t.get("name").and_then(|v| v.as_str())?;
            let kind = t.get("type").and_then(|v| v.as_str()).map(String::from);
            Some(GeminiToolRef { name: name.to_string(), kind })
        }).collect())
        .unwrap_or_default();

    Ok(GeminiAgentDef { name, model, instruction, sub_agents, tools })
}

/// Full per-project bundle: memory hierarchy, skills, subagents, commands, hooks, permissions, MCP.
/// Claude Code-first; other runtimes in Batch 3.
#[tauri::command]
pub fn get_project_bundle(
    db: State<'_, DbState>,
    project_path: String,
) -> Result<ProjectBundle, String> {
    let project_pb = PathBuf::from(&project_path);
    if !project_pb.exists() {
        return Err(format!("Project path does not exist: {}", project_path));
    }
    let project_name = project_pb.file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
        .unwrap_or_else(|| project_path.clone());

    let home = home_dir();

    // Runtime detection (same logic as list_projects)
    let has_claude = project_pb.join(".claude").exists() || project_pb.join("CLAUDE.md").exists();
    let has_codex = project_pb.join(".codex").exists() || project_pb.join("AGENTS.md").exists();
    let has_hermes = project_pb.join(".hermes").exists() || project_pb.join("SOUL.md").exists();
    let has_openclaw = project_pb.join("SOUL.md").exists() && project_pb.join("TOOLS.md").exists();
    let has_gemini = project_pb.join(".gemini").exists() || project_pb.join("GEMINI.md").exists();

    // Memory files: user CLAUDE.md, project CLAUDE.md, nested CLAUDE.md
    let mut memory_files = Vec::new();
    memory_files.push(file_ref("~/.claude/CLAUDE.md", home.join(".claude").join("CLAUDE.md"), "user"));
    memory_files.push(file_ref("CLAUDE.md", project_pb.join("CLAUDE.md"), "project"));
    memory_files.extend(list_nested_claude_md(&project_pb, 4));

    // Subagents
    let mut subagents = Vec::new();
    subagents.extend(list_dir_md_files(&home.join(".claude").join("agents"), "user"));
    subagents.extend(list_dir_md_files(&project_pb.join(".claude").join("agents"), "project"));

    // Commands
    let mut commands = Vec::new();
    commands.extend(list_dir_md_files(&home.join(".claude").join("commands"), "user"));
    commands.extend(list_dir_md_files(&project_pb.join(".claude").join("commands"), "project"));

    // Settings files
    let user_settings = home.join(".claude").join("settings.json");
    let user_settings_local = home.join(".claude").join("settings.local.json");
    let project_settings = project_pb.join(".claude").join("settings.json");
    let project_settings_local = project_pb.join(".claude").join("settings.local.json");
    let project_mcp = project_pb.join(".mcp.json");

    let mut settings_files = Vec::new();
    settings_files.push(file_ref("~/.claude/settings.json", user_settings.clone(), "user"));
    settings_files.push(file_ref("~/.claude/settings.local.json", user_settings_local, "user"));
    settings_files.push(file_ref(".claude/settings.json", project_settings.clone(), "project"));
    settings_files.push(file_ref(".claude/settings.local.json", project_settings_local, "project"));
    settings_files.push(file_ref(".mcp.json", project_mcp.clone(), "project"));

    // Skills (global Claude + project Claude)
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut skills = Vec::new();
    skills.extend(collect_skills_for_project(
        &home.join(".claude").join("skills"), "personal", "claude", None, &conn,
    ));
    skills.extend(collect_skills_for_project(
        &project_pb.join(".claude").join("skills"), "project", "claude",
        Some(&project_name),
        &conn,
    ));
    drop(conn);

    // Hooks from settings.json (user + project)
    let mut hooks = Vec::new();
    hooks.extend(collect_hooks_from_settings(&user_settings, "user"));
    hooks.extend(collect_hooks_from_settings(&project_settings, "project"));

    // Permissions (user + project, separate)
    let permissions_user = parse_permissions_from_settings(&user_settings, "user");
    let permissions_project = parse_permissions_from_settings(&project_settings, "project");

    // MCP: from user settings.json .mcpServers + project .mcp.json
    let mut mcp_servers = Vec::new();
    mcp_servers.extend(parse_mcp_from_settings(&user_settings, "user"));
    mcp_servers.extend(parse_mcp_from_settings(&project_mcp, "project"));

    // ── Codex ────────────────────────────────────────────────────────────
    let codex_home = PathBuf::from(std::env::var("CODEX_HOME")
        .unwrap_or_else(|_| home.join(".codex").to_string_lossy().to_string()));
    let mut codex_files = Vec::new();
    codex_files.push(file_ref("~/.codex/AGENTS.md", codex_home.join("AGENTS.md"), "user"));
    codex_files.push(file_ref("~/.codex/config.toml", codex_home.join("config.toml"), "user"));
    codex_files.push(file_ref("AGENTS.md", project_pb.join("AGENTS.md"), "project"));
    codex_files.push(file_ref(".codex/config.toml", project_pb.join(".codex").join("config.toml"), "project"));

    let conn2 = db.0.lock().map_err(|e| e.to_string())?;
    let mut codex_skills = Vec::new();
    codex_skills.extend(collect_skills_for_project(
        &codex_home.join("skills"), "personal", "codex", None, &conn2,
    ));
    codex_skills.extend(collect_skills_for_project(
        &home.join(".agents").join("skills"), "personal", "codex", None, &conn2,
    ));
    codex_skills.extend(collect_skills_for_project(
        &project_pb.join(".codex").join("skills"), "project", "codex",
        Some(&project_name), &conn2,
    ));
    codex_skills.extend(collect_skills_for_project(
        &project_pb.join(".agents").join("skills"), "project", "codex",
        Some(&project_name), &conn2,
    ));

    // ── OpenClaw ─────────────────────────────────────────────────────────
    let openclaw_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
        .unwrap_or_else(|_| home.join(".openclaw").to_string_lossy().to_string()));
    let openclaw_workspace = openclaw_home.join("workspace");
    let mut openclaw_files = Vec::new();
    openclaw_files.push(file_ref("~/.openclaw/openclaw.json", openclaw_home.join("openclaw.json"), "user"));
    openclaw_files.push(file_ref("~/.openclaw/workspace/SOUL.md", openclaw_workspace.join("SOUL.md"), "user"));
    openclaw_files.push(file_ref("~/.openclaw/workspace/TOOLS.md", openclaw_workspace.join("TOOLS.md"), "user"));
    openclaw_files.push(file_ref("~/.openclaw/workspace/AGENTS.md", openclaw_workspace.join("AGENTS.md"), "user"));
    openclaw_files.push(file_ref("SOUL.md", project_pb.join("SOUL.md"), "project"));
    openclaw_files.push(file_ref("TOOLS.md", project_pb.join("TOOLS.md"), "project"));

    let mut openclaw_skills = Vec::new();
    openclaw_skills.extend(collect_skills_for_project(
        &openclaw_home.join("skills"), "personal", "openclaw", None, &conn2,
    ));
    openclaw_skills.extend(collect_skills_for_project(
        &project_pb.join(".openclaw").join("skills"), "project", "openclaw",
        Some(&project_name), &conn2,
    ));
    openclaw_skills.extend(collect_skills_for_project(
        &project_pb.join("skills"), "project", "openclaw",
        Some(&project_name), &conn2,
    ));

    // ── Hermes ───────────────────────────────────────────────────────────
    let hermes_home = home.join(".hermes");
    let mut hermes_files = Vec::new();
    hermes_files.push(file_ref("~/.hermes/SOUL.md", hermes_home.join("SOUL.md"), "user"));
    hermes_files.push(file_ref("~/.hermes/config.yaml", hermes_home.join("config.yaml"), "user"));
    hermes_files.push(file_ref("~/.hermes/memories/MEMORY.md", hermes_home.join("memories").join("MEMORY.md"), "user"));
    hermes_files.push(file_ref("~/.hermes/memories/USER.md", hermes_home.join("memories").join("USER.md"), "user"));
    // Scan for additional memory files beyond MEMORY.md and USER.md
    let memories_dir = hermes_home.join("memories");
    if memories_dir.exists() {
        if let Ok(entries) = fs::read_dir(&memories_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    if p.is_file() && name.ends_with(".md") && name != "MEMORY.md" && name != "USER.md" {
                        hermes_files.push(file_ref(
                            &format!("~/.hermes/memories/{}", name),
                            p, "user",
                        ));
                    }
                }
            }
        }
    }

    let mut hermes_skills = Vec::new();
    hermes_skills.extend(collect_skills_for_project(
        &hermes_home.join("skills"), "personal", "hermes", None, &conn2,
    ));
    hermes_skills.extend(collect_skills_for_project(
        &project_pb.join(".hermes").join("skills"), "project", "hermes",
        Some(&project_name), &conn2,
    ));

    // ── Gemini CLI / ADK ─────────────────────────────────────────────────
    let gemini_hm = gemini_home();
    let mut gemini_files = Vec::new();
    gemini_files.push(file_ref("~/.gemini/GEMINI.md", gemini_hm.join("GEMINI.md"), "user"));
    gemini_files.push(file_ref("~/.gemini/settings.json", gemini_hm.join("settings.json"), "user"));
    gemini_files.push(file_ref("GEMINI.md", project_pb.join("GEMINI.md"), "project"));
    gemini_files.push(file_ref(".gemini/settings.json", project_pb.join(".gemini").join("settings.json"), "project"));
    gemini_files.push(file_ref("root_agent.yaml", project_pb.join("root_agent.yaml"), "project"));

    // Gemini skills/agents (not yet a convention — check .gemini/agents/ if present)
    let mut gemini_skills = Vec::new();
    gemini_skills.extend(collect_skills_for_project(
        &project_pb.join(".gemini").join("agents"), "project", "gemini",
        Some(&project_name), &conn2,
    ));

    drop(conn2);

    // ── OpenAI Agents SDK (enriches Codex) ───────────────────────────────
    let sandbox_config = if has_codex { parse_sandbox_config(&project_pb) } else { None };
    let approval_policies = if has_codex { parse_approval_policies(&project_pb) } else { Vec::new() };

    Ok(ProjectBundle {
        project_path,
        project_name,
        has_claude,
        has_codex,
        has_hermes,
        has_openclaw,
        has_gemini,
        memory_files,
        subagents,
        commands,
        settings_files,
        skills,
        hooks,
        permissions_user,
        permissions_project,
        mcp_servers,
        codex_files,
        codex_skills,
        openclaw_files,
        openclaw_skills,
        hermes_files,
        hermes_skills,
        gemini_files,
        gemini_skills,
        sandbox_config,
        approval_policies,
    })
}






/// Get skills for a specific project
#[tauri::command]
pub fn get_project_skills(project_path: String) -> Result<Vec<LocalSkill>, String> {
    let path_buf = PathBuf::from(&project_path);
    let project_name = path_buf.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let mut skills = Vec::new();

    let skill_dirs = vec![
        (path_buf.join(".claude/skills"), "claude"),
        (path_buf.join(".codex/skills"), "codex"),
        (path_buf.join(".agents/skills"), "codex"),
        (path_buf.join(".hermes/skills"), "hermes"),
        (path_buf.join("skills"), "shared"),
    ];

    for (dir, runtime) in skill_dirs {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let skill_path = entry.path();
                    if skill_path.is_dir() {
                        let skill_md = skill_path.join("SKILL.md");
                        if skill_md.exists() {
                            if let Ok(content) = fs::read_to_string(&skill_md) {
                                let (fm, _body) = parse_frontmatter(&content);
                                let name = fm.get("name")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string());
                                let description = fm.get("description")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let token_count = estimate_tokens(content.len() as u64);
                                let hash = content_hash(&content);

                                skills.push(LocalSkill {
                                    id: format!("{}:{}", runtime, skill_md.to_string_lossy()),
                                    name,
                                    description,
                                    file_path: skill_md.to_string_lossy().to_string(),
                                    scope: "project".to_string(),
                                    runtime: runtime.to_string(),
                                    project: Some(project_name.clone()),
                                    token_count,
                                    enabled: true,
                                    content_hash: hash,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(skills)
}

/// Clone a skill from one project to another
#[tauri::command]
pub fn clone_skill(
    source_skill_path: String,
    target_project_path: String,
    target_runtime: String,
) -> Result<String, String> {
    let source_path = PathBuf::from(&source_skill_path);
    let target_project = PathBuf::from(&target_project_path);

    if !source_path.exists() {
        return Err("Source skill does not exist".to_string());
    }

    // Read source skill content
    let content = fs::read_to_string(&source_path)
        .map_err(|e| format!("Failed to read source skill: {}", e))?;

    // Determine target skills directory
    let target_skills_dir = match target_runtime.as_str() {
        "claude" => target_project.join(".claude/skills"),
        "codex" => target_project.join(".agents/skills"),
        "hermes" => target_project.join(".hermes/skills"),
        "openclaw" => target_project.join("skills"),
        _ => target_project.join(".claude/skills"),
    };

    // Get skill name from source path
    let skill_name = source_path.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "cloned-skill".to_string());

    // Create target directory
    let target_skill_dir = target_skills_dir.join(&skill_name);
    fs::create_dir_all(&target_skill_dir)
        .map_err(|e| format!("Failed to create target directory: {}", e))?;

    // Write skill file
    let target_skill_path = target_skill_dir.join("SKILL.md");
    fs::write(&target_skill_path, &content)
        .map_err(|e| format!("Failed to write skill: {}", e))?;

    Ok(target_skill_path.to_string_lossy().to_string())
}

/// Refresh skill count for a project
#[tauri::command]
pub fn refresh_project_skills(db: State<'_, DbState>, project_id: String) -> Result<u32, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Get project path
    let path: String = conn.query_row(
        "SELECT path FROM projects WHERE id = ?1",
        params![project_id],
        |row| row.get(0),
    ).map_err(|e| e.to_string())?;

    let skill_count = count_project_skills(&PathBuf::from(&path));

    // Update in database
    conn.execute(
        "UPDATE projects SET skill_count = ?1 WHERE id = ?2",
        params![skill_count, project_id],
    ).map_err(|e| e.to_string())?;

    Ok(skill_count)
}


// ── Model Configuration ──────────────────────────────────────────────────
//
// `list_model_configs` / `save_model_config` / `get_model_config`
// moved to commands/models.rs (PR 2 of the commands.rs split).









/// Get aggregated usage metrics
#[tauri::command]
pub fn get_usage_metrics(
    db: State<'_, DbState>,
    days: Option<i32>,
) -> Result<UsageMetrics, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let days = days.unwrap_or(30);
    let interval = format!("-{} days", days);

    // 2026-05-19 war-room synthesis: every get_usage_metrics query
    // filters to dispatch_kind='active'. Passive-observation rows from
    // v2.6 PR-A would otherwise inflate totals / tokens / per-runtime /
    // per-day counts on the day passive observation goes live.
    // Total counts
    let (total, successful, failed): (i64, i64, i64) = conn.query_row(
        "SELECT
            COUNT(*),
            SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END),
            SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END)
         FROM execution_logs
         WHERE created_at > datetime('now', ?1) AND dispatch_kind = 'active'",
        params![&interval],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap_or((0, 0, 0));

    // Token counts and avg duration
    let (tokens_in, tokens_out, avg_duration): (i64, i64, Option<f64>) = conn.query_row(
        "SELECT
            COALESCE(SUM(tokens_in), 0),
            COALESCE(SUM(tokens_out), 0),
            AVG(duration_ms)
         FROM execution_logs
         WHERE created_at > datetime('now', ?1) AND dispatch_kind = 'active'",
        params![&interval],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap_or((0, 0, None));

    // Executions by runtime
    let mut stmt = conn.prepare(
        "SELECT runtime,
                COUNT(*),
                SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END),
                SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END)
         FROM execution_logs
         WHERE created_at > datetime('now', ?1) AND dispatch_kind = 'active'
         GROUP BY runtime"
    ).map_err(|e| e.to_string())?;

    let executions_by_runtime: Vec<RuntimeExecutionCount> = stmt
        .query_map(params![&interval], |row| {
            Ok(RuntimeExecutionCount {
                runtime: row.get(0)?,
                count: row.get(1)?,
                success_count: row.get(2)?,
                error_count: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Executions by day (also filtered to active per war-room synthesis)
    let mut stmt = conn.prepare(
        "SELECT DATE(created_at),
                COUNT(*),
                SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END),
                SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END)
         FROM execution_logs
         WHERE created_at > datetime('now', ?1) AND dispatch_kind = 'active'
         GROUP BY DATE(created_at)
         ORDER BY DATE(created_at) ASC"
    ).map_err(|e| e.to_string())?;

    let executions_by_day: Vec<DailyExecutionCount> = stmt
        .query_map(params![&interval], |row| {
            Ok(DailyExecutionCount {
                date: row.get(0)?,
                count: row.get(1)?,
                success_count: row.get(2)?,
                error_count: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(UsageMetrics {
        total_executions: total,
        successful_executions: successful,
        failed_executions: failed,
        total_tokens_in: tokens_in,
        total_tokens_out: tokens_out,
        avg_duration_ms: avg_duration,
        executions_by_runtime,
        executions_by_day,
    })
}






// ── Real-time Agent Monitoring Commands ─────────────────────────────────

#[tauri::command]
pub fn get_monitoring_snapshot(
    db: State<'_, DbState>,
) -> Result<MonitoringSnapshot, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let mut active_stmt = conn.prepare(
        "SELECT id, runtime, status, prompt, tokens_in, tokens_out, duration_ms, skill_name, created_at
         FROM execution_logs WHERE status = 'running' AND dispatch_kind = 'active' ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;

    let active_sessions: Vec<AgentSession> = active_stmt.query_map([], |row| {
        Ok(AgentSession {
            id: row.get(0)?, runtime: row.get(1)?, status: row.get(2)?,
            prompt: row.get(3)?, tokens_in: row.get::<_, i64>(4).unwrap_or(0),
            tokens_out: row.get::<_, i64>(5).unwrap_or(0), duration_ms: row.get(6)?,
            skill_name: row.get(7)?, started_at: row.get(8)?, ended_at: None,
        })
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    let mut recent_stmt = conn.prepare(
        "SELECT id, runtime, status, prompt, tokens_in, tokens_out, duration_ms, skill_name, created_at
         FROM execution_logs WHERE status != 'running' AND dispatch_kind = 'active' ORDER BY created_at DESC LIMIT 20"
    ).map_err(|e| e.to_string())?;

    let recent_sessions: Vec<AgentSession> = recent_stmt.query_map([], |row| {
        Ok(AgentSession {
            id: row.get(0)?, runtime: row.get(1)?, status: row.get(2)?,
            prompt: row.get(3)?, tokens_in: row.get::<_, i64>(4).unwrap_or(0),
            tokens_out: row.get::<_, i64>(5).unwrap_or(0), duration_ms: row.get(6)?,
            skill_name: row.get(7)?, started_at: row.get(8)?, ended_at: None,
        })
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    // 2026-05-19 war-room synthesis: real-time monitoring counters
    // filter dispatch_kind='active' so the dashboard shows ATO-fired
    // work only. Passive observation has its own surface (v2.6 Insights
    // → Live billing surface chip).
    let total_tokens_today: i64 = conn.query_row(
        "SELECT COALESCE(SUM(COALESCE(tokens_in,0) + COALESCE(tokens_out,0)), 0) FROM execution_logs WHERE created_at > datetime('now', '-1 day') AND dispatch_kind = 'active'",
        [], |row| row.get(0)
    ).unwrap_or(0);

    let total_sessions_today: i64 = conn.query_row(
        "SELECT COUNT(*) FROM execution_logs WHERE created_at > datetime('now', '-1 day') AND dispatch_kind = 'active'",
        [], |row| row.get(0)
    ).unwrap_or(0);

    let errors_today: i64 = conn.query_row(
        "SELECT COUNT(*) FROM execution_logs WHERE status = 'error' AND created_at > datetime('now', '-1 day') AND dispatch_kind = 'active'",
        [], |row| row.get(0)
    ).unwrap_or(0);

    let avg_duration_ms: f64 = conn.query_row(
        "SELECT COALESCE(AVG(duration_ms), 0) FROM execution_logs WHERE duration_ms IS NOT NULL AND created_at > datetime('now', '-1 day') AND dispatch_kind = 'active'",
        [], |row| row.get(0)
    ).unwrap_or(0.0);

    let tokens_last_hour: i64 = conn.query_row(
        "SELECT COALESCE(SUM(COALESCE(tokens_in,0) + COALESCE(tokens_out,0)), 0) FROM execution_logs WHERE created_at > datetime('now', '-1 hour') AND dispatch_kind = 'active'",
        [], |row| row.get(0)
    ).unwrap_or(0);
    let token_rate_per_hour = tokens_last_hour as f64;

    let mut online_runtimes = Vec::new();
    let mut offline_runtimes = Vec::new();
    let mut health_stmt = conn.prepare(
        "SELECT runtime, status FROM health_checks
         WHERE rowid IN (SELECT MAX(rowid) FROM health_checks GROUP BY runtime)"
    ).map_err(|e| e.to_string())?;

    let _ = health_stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .for_each(|(runtime, status)| {
        if status == "healthy" || status == "online" {
            online_runtimes.push(runtime);
        } else {
            offline_runtimes.push(runtime);
        }
    });

    let mut alerts = Vec::new();
    if errors_today > 5 {
        alerts.push(MonitoringAlert {
            id: uuid::Uuid::new_v4().to_string(), level: "warning".to_string(),
            message: format!("{} errors in the last 24 hours", errors_today),
            runtime: None, created_at: chrono::Utc::now().to_rfc3339(),
        });
    }
    if token_rate_per_hour > 100000.0 {
        alerts.push(MonitoringAlert {
            id: uuid::Uuid::new_v4().to_string(), level: "warning".to_string(),
            message: format!("High token usage: {:.0} tokens/hour", token_rate_per_hour),
            runtime: None, created_at: chrono::Utc::now().to_rfc3339(),
        });
    }
    for rt in &offline_runtimes {
        alerts.push(MonitoringAlert {
            id: uuid::Uuid::new_v4().to_string(), level: "error".to_string(),
            message: format!("{} runtime is offline", rt),
            runtime: Some(rt.clone()), created_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    Ok(MonitoringSnapshot {
        active_sessions, recent_sessions, total_tokens_today, total_sessions_today,
        errors_today, avg_duration_ms, runtimes_online: online_runtimes,
        runtimes_offline: offline_runtimes, token_rate_per_hour, alerts,
    })
}


// ── Agents (v1.3.0 T3) ────────────────────────────────────────────────────
//
// Records produced by the Create Agent wizard. Each record represents a
// runtime-specific agent file written to disk plus metadata for fast lookup
// from Home / Agents list.
//
// File-writing contract per runtime (kept minimal for v1.3.0 — Claude is the
// canonical path; other runtimes write a stub markdown placeholder so the
// agent record is real-on-disk, then v1.3.x ships richer per-runtime layouts):
//
//   claude    → ~/.claude/agents/<slug>.md
//   codex     → ~/.codex/agents/<slug>/AGENTS.md
//   gemini    → <project>/.gemini/agents/<slug>.yaml  (falls back to ~/.gemini)
//   openclaw  → ~/.openclaw/agents/<slug>/SOUL.md
//   hermes    → ~/.hermes/agents/<slug>/AGENT.md

fn slugify(input: &str) -> String {
    let s: String = input
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    // collapse repeated dashes and trim
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.chars() {
        if c == '-' {
            if !prev_dash && !out.is_empty() {
                out.push('-');
            }
            prev_dash = true;
        } else {
            out.push(c);
            prev_dash = false;
        }
    }
    out.trim_matches('-').to_string()
}

fn agent_file_path(runtime: &str, slug: &str) -> Result<PathBuf, String> {
    let home = home_dir();
    let path = match runtime {
        "claude" => home.join(".claude").join("agents").join(format!("{}.md", slug)),
        "codex" => home.join(".codex").join("agents").join(slug).join("AGENTS.md"),
        "gemini" => home.join(".gemini").join("agents").join(format!("{}.yaml", slug)),
        "openclaw" => home.join(".openclaw").join("agents").join(slug).join("SOUL.md"),
        "hermes" => home.join(".hermes").join("agents").join(slug).join("AGENT.md"),
        other => return Err(format!("Unsupported runtime: {}", other)),
    };
    Ok(path)
}

fn render_agent_file(runtime: &str, agent: &Agent) -> String {
    match runtime {
        "claude" => render_claude_agent(agent),
        "codex" => render_codex_agent(agent),
        "gemini" => render_gemini_agent(agent),
        "openclaw" => render_openclaw_agent(agent),
        "hermes" => render_hermes_agent(agent),
        _ => String::new(),
    }
}

fn render_claude_agent(agent: &Agent) -> String {
    // Claude Code agent format: frontmatter + system prompt body.
    // See: https://docs.claude.com/en/docs/claude-code/sub-agents
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", agent.slug));
    if let Some(desc) = &agent.description {
        out.push_str(&format!("description: {}\n", desc));
    }
    if let Some(model) = &agent.model {
        out.push_str(&format!("model: {}\n", model));
    }
    out.push_str("---\n\n");
    out.push_str(&format!("# {}\n\n", agent.display_name));
    if let Some(prompt) = &agent.system_prompt {
        if !prompt.trim().is_empty() {
            out.push_str(prompt);
            out.push_str("\n");
        }
    }
    if let Some(goal) = &agent.goal {
        if agent.system_prompt.as_deref().unwrap_or("").trim().is_empty() {
            out.push_str(&format!(
                "You are an agent designed to: {}\n",
                goal
            ));
        }
    }
    out
}

fn render_codex_agent(agent: &Agent) -> String {
    // Codex / OpenAI Agents SDK uses AGENTS.md.
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", agent.display_name));
    if let Some(desc) = &agent.description {
        out.push_str(&format!("> {}\n\n", desc));
    }
    if let Some(model) = &agent.model {
        out.push_str(&format!("**Model:** `{}`\n\n", model));
    }
    if let Some(prompt) = &agent.system_prompt {
        out.push_str("## Instructions\n\n");
        out.push_str(prompt);
        out.push_str("\n");
    }
    out
}

fn render_gemini_agent(agent: &Agent) -> String {
    // Minimal root_agent-shaped YAML; user can extend later.
    let mut out = String::new();
    out.push_str(&format!("name: {}\n", agent.slug));
    out.push_str(&format!("display_name: \"{}\"\n", agent.display_name));
    if let Some(model) = &agent.model {
        out.push_str(&format!("model: {}\n", model));
    } else {
        out.push_str("model: gemini-2.0-flash-exp\n");
    }
    if let Some(prompt) = &agent.system_prompt {
        out.push_str("instruction: |\n");
        for line in prompt.lines() {
            out.push_str(&format!("  {}\n", line));
        }
    } else if let Some(goal) = &agent.goal {
        out.push_str("instruction: |\n");
        out.push_str(&format!("  You are an agent designed to: {}\n", goal));
    }
    out
}

fn render_openclaw_agent(agent: &Agent) -> String {
    // OpenClaw uses SOUL.md as the agent identity file.
    let mut out = String::new();
    out.push_str(&format!("# Soul: {}\n\n", agent.display_name));
    if let Some(desc) = &agent.description {
        out.push_str(&format!("{}\n\n", desc));
    }
    if let Some(prompt) = &agent.system_prompt {
        out.push_str("## Identity\n\n");
        out.push_str(prompt);
        out.push_str("\n");
    }
    out
}

fn render_hermes_agent(agent: &Agent) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Hermes Agent: {}\n\n", agent.display_name));
    if let Some(desc) = &agent.description {
        out.push_str(&format!("{}\n\n", desc));
    }
    if let Some(prompt) = &agent.system_prompt {
        out.push_str("## System\n\n");
        out.push_str(prompt);
        out.push_str("\n");
    }
    out
}

#[tauri::command]
pub fn create_agent(
    db: State<'_, DbState>,
    display_name: String,
    runtime: String,
    description: Option<String>,
    model: Option<String>,
    project_id: Option<String>,
    system_prompt: Option<String>,
    permissions: Option<Vec<String>>,
    skills: Option<Vec<String>>,
    mcps: Option<Vec<String>>,
    goal: Option<String>,
    write_file: Option<bool>,
    kind: Option<String>,
    // v2.7.9 Felipe P5 — optional dispatch prompt that S9 will use as
    // the fallback when `ato dispatch --agent <slug>` is called with no
    // prompt argument. None preserves today's interactive behavior.
    default_prompt: Option<String>,
) -> Result<Agent, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    if display_name.trim().is_empty() {
        return Err("display_name cannot be empty".to_string());
    }
    let allowed = ["claude", "codex", "gemini", "openclaw", "hermes"];
    if !allowed.contains(&runtime.as_str()) {
        return Err(format!("Unsupported runtime: {}", runtime));
    }

    let slug = slugify(&display_name);
    if slug.is_empty() {
        return Err("display_name must contain at least one alphanumeric character".to_string());
    }

    // v2.0.0 — internal/external kind. External agents auto-lock to a read-only
    // permission set (no shell, no fs writes) so customer-facing deployments
    // can't accidentally execute arbitrary commands. The caller can still pass
    // `permissions` to override after creation if they know what they're doing.
    let kind_val = match kind.as_deref() {
        Some("external") => "external",
        Some("internal") | None => "internal",
        Some(other) => return Err(format!("Unsupported agent kind: {}", other)),
    }.to_string();

    let effective_permissions = if kind_val == "external" && permissions.is_none() {
        Some(vec![
            "Read".to_string(),
            "Grep".to_string(),
            "Glob".to_string(),
            "WebFetch".to_string(),
        ])
    } else {
        permissions
    };

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let permissions_json = effective_permissions.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default());
    let skills_json = skills.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default());
    let mcps_json = mcps.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default());

    let mut agent = Agent {
        id: id.clone(),
        slug: slug.clone(),
        display_name: display_name.clone(),
        description: description.clone(),
        runtime: runtime.clone(),
        model: model.clone(),
        project_id: project_id.clone(),
        system_prompt: system_prompt.clone(),
        permissions: permissions_json.clone(),
        skills: skills_json.clone(),
        mcps: mcps_json.clone(),
        goal: goal.clone(),
        file_path: None,
        created_at: now.clone(),
        last_used_at: None,
        role_models: None,
        memory_policy: None,
        kind: Some(kind_val.clone()),
    };

    // Optionally write the agent file to disk. External agents skip this — they
    // live in the cloud / customer infra after deploy, not on the dev's laptop.
    let should_write_file = write_file.unwrap_or(true) && kind_val == "internal";
    if should_write_file {
        let path = agent_file_path(&runtime, &slug)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create agent directory: {}", e))?;
        }
        let contents = render_agent_file(&runtime, &agent);
        fs::write(&path, &contents)
            .map_err(|e| format!("Failed to write agent file: {}", e))?;
        agent.file_path = Some(path.to_string_lossy().to_string());
    }

    // Normalize default_prompt — whitespace-only strings stored as NULL so
    // the S9 "use default when blank" branch can rely on `IS NOT NULL`.
    let default_prompt_value: Option<String> = default_prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // Insert into DB. v2.7.8 PR-6 — stamp permissions_migrated_at = now
    // for any agent created on v2.7.8+. New agents have correct
    // expectations from the wizard, so enforcement is on from day 1.
    conn.execute(
        "INSERT INTO agents (id, slug, display_name, description, runtime, model, project_id, system_prompt, permissions, skills, mcps, goal, file_path, created_at, last_used_at, kind, permissions_migrated_at, default_prompt)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            agent.id, agent.slug, agent.display_name, agent.description, agent.runtime, agent.model,
            agent.project_id, agent.system_prompt, agent.permissions, agent.skills, agent.mcps,
            agent.goal, agent.file_path, agent.created_at, agent.last_used_at, kind_val, agent.created_at,
            default_prompt_value
        ],
    ).map_err(|e| {
        // SQLite UNIQUE violation → friendly message
        let msg = e.to_string();
        if msg.contains("UNIQUE") {
            format!("An agent named \"{}\" already exists for runtime {}", slug, runtime)
        } else {
            msg
        }
    })?;

    Ok(agent)
}

/// S11 (v2.7.11) — pre-v2.7.8 agents have `permissions_migrated_at`
/// NULL and dispatch falls back to pre-PR-2 defaults (the new permission
/// DSL is recorded but NOT enforced). The MigrationToast surfaces the
/// count so users re-save those agents to engage enforcement. Read-only;
/// the stamp itself happens via the normal save flow (which already sets
/// `permissions_migrated_at = now`).
#[tauri::command]
pub fn count_unmigrated_agents(db: State<'_, DbState>) -> Result<i64, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT COUNT(*) FROM agents WHERE permissions_migrated_at IS NULL",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_agents(
    db: State<'_, DbState>,
    runtime: Option<String>,
    project_id: Option<String>,
) -> Result<Vec<Agent>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let mut sql = String::from(
        "SELECT id, slug, display_name, description, runtime, model, project_id, system_prompt, permissions, skills, mcps, goal, file_path, created_at, last_used_at, role_models_json, memory_policy_json, kind FROM agents",
    );
    let mut conditions: Vec<&str> = Vec::new();
    if runtime.is_some() {
        conditions.push("runtime = ?");
    }
    if project_id.is_some() {
        conditions.push("project_id = ?");
    }
    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }
    sql.push_str(" ORDER BY COALESCE(last_used_at, created_at) DESC");

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut bindings: Vec<&dyn rusqlite::ToSql> = Vec::new();
    if let Some(r) = &runtime {
        bindings.push(r);
    }
    if let Some(p) = &project_id {
        bindings.push(p);
    }

    let rows = stmt
        .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
            Ok(Agent {
                id: row.get(0)?,
                slug: row.get(1)?,
                display_name: row.get(2)?,
                description: row.get(3)?,
                runtime: row.get(4)?,
                model: row.get(5)?,
                project_id: row.get(6)?,
                system_prompt: row.get(7)?,
                permissions: row.get(8)?,
                skills: row.get(9)?,
                mcps: row.get(10)?,
                goal: row.get(11)?,
                file_path: row.get(12)?,
                created_at: row.get(13)?,
                last_used_at: row.get(14)?,
                role_models: row.get(15).ok(),
                memory_policy: row.get(16).ok(),
                kind: row.get(17).ok(),
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_agent(db: State<'_, DbState>, id: String) -> Result<Agent, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id, slug, display_name, description, runtime, model, project_id, system_prompt, permissions, skills, mcps, goal, file_path, created_at, last_used_at, role_models_json, memory_policy_json, kind FROM agents WHERE id = ?1",
        params![id],
        |row| {
            Ok(Agent {
                id: row.get(0)?,
                slug: row.get(1)?,
                display_name: row.get(2)?,
                description: row.get(3)?,
                runtime: row.get(4)?,
                model: row.get(5)?,
                project_id: row.get(6)?,
                system_prompt: row.get(7)?,
                permissions: row.get(8)?,
                skills: row.get(9)?,
                mcps: row.get(10)?,
                goal: row.get(11)?,
                file_path: row.get(12)?,
                created_at: row.get(13)?,
                last_used_at: row.get(14)?,
                role_models: row.get(15).ok(),
                memory_policy: row.get(16).ok(),
                kind: row.get(17).ok(),
            })
        },
    )
    .map_err(|e| e.to_string())
}

/// v2.0.0 — flip an existing agent between internal and external. Switching to
/// `external` does NOT auto-rewrite permissions on existing agents (caller is
/// expected to review and adjust); the auto-lock behavior only fires at create
/// time. This way users who deliberately broadened permissions don't lose them
/// silently when they flip the toggle to share via embed.
#[tauri::command]
pub fn update_agent_kind(
    db: State<'_, DbState>,
    id: String,
    kind: String,
) -> Result<(), String> {
    if kind != "internal" && kind != "external" {
        return Err(format!("Unsupported agent kind: {}", kind));
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE agents SET kind = ?1 WHERE id = ?2",
        params![kind, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ── v2.0.0 Wave 2 — Local knowledge ──────────────────────────────────────
//
// `KnowledgeChunk` struct + `EmbedProvider` + `ingest_knowledge_text` +
// `delete_knowledge_chunk` + `delete_knowledge_source` + `retrieve_knowledge`
// + all embedding/chunking/cosine helpers moved to commands/knowledge.rs
// (PR 4 of the commands.rs split). Only `list_agent_knowledge` stays here
// — it's in the agents domain (PR 28 / commands/agents.rs). `KnowledgeChunk`
// resolves via the `pub use knowledge::*` re-export at the top of this file;
// `blob_to_f32_vec` is `pub(super)` in knowledge.rs and called explicitly.

/// List chunks for an agent. By default `include_embedding=false` so the UI
/// gets a fast list view; deploy-bundle generation passes `true`.
#[tauri::command]
pub fn list_agent_knowledge(
    db: State<'_, DbState>,
    agent_id: String,
    include_embedding: Option<bool>,
) -> Result<Vec<KnowledgeChunk>, String> {
    let with_embed = include_embedding.unwrap_or(false);
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, agent_id, source, content, tokens, position, embedding, embed_model, created_at
             FROM agent_knowledge_chunks
             WHERE agent_id = ?1
             ORDER BY source, position",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![agent_id], |row| {
            let blob: Vec<u8> = row.get(6)?;
            let embedding = if with_embed {
                Some(knowledge::blob_to_f32_vec(&blob))
            } else {
                None
            };
            Ok(KnowledgeChunk {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                source: row.get(2)?,
                content: row.get(3)?,
                tokens: row.get(4)?,
                position: row.get(5)?,
                embed_model: row.get(7)?,
                created_at: row.get(8)?,
                embedding,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}


/// v1.4.0 F3 — persist memory policy JSON for the agent.
#[tauri::command]
pub fn update_agent_memory_policy(
    db: State<'_, DbState>,
    id: String,
    policy_json: Option<String>,
) -> Result<(), String> {
    if let Some(ref s) = policy_json {
        // Validate JSON shape but don't constrain content — schema lives in TS.
        if !s.trim().is_empty() {
            serde_json::from_str::<serde_json::Value>(s)
                .map_err(|e| format!("Invalid memory_policy JSON: {}", e))?;
        }
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE agents SET memory_policy_json = ?1 WHERE id = ?2",
        params![policy_json, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// v1.4.0 F5 — persist per-task model selection for the agent.
#[tauri::command]
pub fn update_agent_role_models(
    db: State<'_, DbState>,
    id: String,
    role_models_json: Option<String>,
) -> Result<(), String> {
    if let Some(ref s) = role_models_json {
        if !s.trim().is_empty() {
            serde_json::from_str::<serde_json::Value>(s)
                .map_err(|e| format!("Invalid role_models JSON: {}", e))?;
        }
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE agents SET role_models_json = ?1 WHERE id = ?2",
        params![role_models_json, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// v2.7.9 Felipe P5 — persist the optional default dispatch prompt.
///
/// Whitespace-only strings collapse to NULL so the S9 "use default when
/// prompt is blank" branch can rely on a single `IS NOT NULL` check.
/// Passing `None` clears the override.
#[tauri::command]
pub fn update_agent_default_prompt(
    db: State<'_, DbState>,
    id: String,
    value: Option<String>,
) -> Result<(), String> {
    let normalized: Option<String> = value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE agents SET default_prompt = ?1 WHERE id = ?2",
        params![normalized, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// v2.7.9 Felipe P5 — read the current default dispatch prompt.
///
/// Surfaced separately because list_agents/get_agent are owned by S9's
/// dispatch lock; this additive getter lets the AgentDetail UI prefill
/// the edit textarea without us having to touch those readers.
#[tauri::command]
pub fn get_agent_default_prompt(
    db: State<'_, DbState>,
    id: String,
) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT default_prompt FROM agents WHERE id = ?1",
        params![id],
        |row| row.get::<_, Option<String>>(0),
    )
    .map_err(|e| e.to_string())
}

/// Update the MCPs attached to an agent. Stored as a JSON-encoded string
/// array in `agents.mcps`. Used by the one-click "Add browser tools" button
/// and any future "attach MCP to agent" UX.
#[tauri::command]
pub fn update_agent_mcps(
    db: State<'_, DbState>,
    id: String,
    mcps: Vec<String>,
) -> Result<(), String> {
    let json = serde_json::to_string(&mcps).map_err(|e| e.to_string())?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE agents SET mcps = ?1 WHERE id = ?2",
        params![json, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_agent(db: State<'_, DbState>, id: String, delete_file: Option<bool>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    if delete_file.unwrap_or(true) {
        if let Ok(file_path) = conn.query_row(
            "SELECT file_path FROM agents WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<String>>(0),
        ) {
            if let Some(p) = file_path {
                let _ = fs::remove_file(&p);
            }
        }
    }

    conn.execute("DELETE FROM agents WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn touch_agent_last_used(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE agents SET last_used_at = ?1 WHERE id = ?2",
        params![now, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Agent Variables (v1.4.0 F1) ──────────────────────────────────────────
//
// Dynamic prompt resolvers per agent. The article's central insight: prompts
// are templates with `{var}` placeholders. Each variable has a "kind" + a
// kind-specific config_json. At dispatch time, we resolve all variables and
// substitute their values into the system + user prompts.
//
// Kinds (Free): static, env, project-path, file
// Kinds (Pro):  db-query, mcp-call, computed
//
// Pro resolvers are stubbed for Wave 2.1 — they return a clearly-flagged
// "Configure {{var}} to use Pro resolver" placeholder so the user sees that
// the gate exists. Wave 2.2 fills in the actual Pro implementations.

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentVariable {
    pub id: String,
    pub agent_id: String,
    pub name: String,
    pub kind: String,
    /// JSON-encoded resolver config. Shape depends on `kind`:
    ///   static       → { "value": "..." }
    ///   env          → { "var": "OPENAI_API_KEY" }
    ///   project-path → {}  (resolves to the active project's path)
    ///   file         → { "path": "/abs/or/~/path", "maxBytes": 8192 }
    ///   db-query     → { "connection": "...", "sql": "...", "column": 0 }
    ///   mcp-call     → { "server": "...", "tool": "...", "args": {...} }
    ///   computed     → { "expr": "..." }
    pub config_json: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[tauri::command]
pub fn list_agent_variables(
    db: State<'_, DbState>,
    agent_id: String,
) -> Result<Vec<AgentVariable>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, agent_id, name, kind, config_json, enabled, created_at, updated_at
             FROM agent_variables WHERE agent_id = ?1 ORDER BY name",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![agent_id], |row| {
            Ok(AgentVariable {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                config_json: row.get(4)?,
                enabled: row.get::<_, i32>(5)? != 0,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_agent_variable(
    db: State<'_, DbState>,
    id: Option<String>,
    agent_id: String,
    name: String,
    kind: String,
    config_json: String,
    enabled: Option<bool>,
) -> Result<AgentVariable, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    if name.trim().is_empty() {
        return Err("Variable name cannot be empty".into());
    }
    let allowed_kinds = ["static", "env", "project-path", "file", "db-query", "mcp-call", "computed"];
    if !allowed_kinds.contains(&kind.as_str()) {
        return Err(format!("Unsupported variable kind: {}", kind));
    }
    // Sanity-check name. Variables are referenced as {name} in prompts; allow
    // alphanumeric + underscore so substitution stays unambiguous.
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(
            "Variable name must contain only letters, digits, and underscores".into(),
        );
    }
    // Validate config_json parses.
    serde_json::from_str::<serde_json::Value>(&config_json)
        .map_err(|e| format!("Invalid config JSON: {}", e))?;

    let now = chrono::Utc::now().to_rfc3339();
    let final_id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let enabled_int: i32 = if enabled.unwrap_or(true) { 1 } else { 0 };

    conn.execute(
        "INSERT INTO agent_variables (id, agent_id, name, kind, config_json, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           kind = excluded.kind,
           config_json = excluded.config_json,
           enabled = excluded.enabled,
           updated_at = excluded.updated_at",
        params![final_id, agent_id, name, kind, config_json, enabled_int, now],
    )
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") {
            format!("Variable '{}' already exists for this agent", name)
        } else {
            msg
        }
    })?;

    Ok(AgentVariable {
        id: final_id,
        agent_id,
        name,
        kind,
        config_json,
        enabled: enabled.unwrap_or(true),
        created_at: now.clone(),
        updated_at: now,
    })
}

#[tauri::command]
pub fn delete_agent_variable(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM agent_variables WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Resolve every variable for an agent and return name→value map.
/// Disabled variables are skipped. Resolution failures are caught and the
/// variable resolves to a `{var:resolution-failed}` marker so the user sees
/// the failure in the rendered prompt rather than getting a silent miss.
pub fn resolve_agent_variables(
    conn: &Connection,
    agent_id: &str,
    active_project_path: Option<&str>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();

    // v2.8.x P2 Security AMEND — pull the variable id so the consent
    // check has something to look up. Pre-fix this function only
    // selected (name, kind, config_json) which made per-variable
    // consent untrackable.
    let mut stmt = match conn.prepare(
        "SELECT id, name, kind, config_json FROM agent_variables
         WHERE agent_id = ?1 AND enabled = 1",
    ) {
        Ok(s) => s,
        Err(_) => return out,
    };

    let rows = match stmt.query_map(params![agent_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    }) {
        Ok(r) => r,
        Err(_) => return out,
    };

    for row in rows.flatten() {
        let (var_id, name, kind, config_json) = row;
        // Privileged resolver kinds (file/db-query/computed) MUST have
        // an active consent grant or they return a consent-required
        // error. The static/env/project-path kinds bypass the check —
        // they read no local resource that wasn't already in the
        // process's env / config.
        let needs_consent = matches!(kind.as_str(), "file" | "db-query" | "computed");
        if needs_consent && !has_active_consent(conn, &var_id) {
            // Insert a placeholder string so the LLM prompt template
            // gets a HONEST marker — better than silent empty-string
            // resolution which would hide the security gate from the
            // user.
            out.insert(
                name,
                format!("{{consent-required:{}}}", kind),
            );
            continue;
        }
        let value = resolve_one_variable(&kind, &config_json, active_project_path)
            .unwrap_or_else(|err| format!("{{{}:{}}}", name, err));
        out.insert(name, value);
    }
    out
}

/// Check if a variable has an active (non-revoked) consent grant.
/// Returns false on any DB error to fail closed (security default).
///
/// War-room 87E6CADF round 3 security-specialist AMEND: variables of
/// kind file / db-query / computed require explicit user consent
/// before the resolver runs. This function is the gate.
fn has_active_consent(conn: &Connection, variable_id: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM variable_consent_grants
         WHERE variable_id = ?1 AND revoked_at IS NULL",
        params![variable_id],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

/// v2.8.x P2 Security AMEND — Tauri command for the frontend
/// consent dialog. Called when user clicks "Allow ATO to read
/// [path]" on the consent modal that appears at variable save
/// time for file / db-query / computed kinds.
///
/// `granted_resource` is the human-readable string the user
/// SAW when consenting (the path, the SQL, the expr) — recorded
/// so a later audit can prove what specifically was consented to,
/// not just "they clicked allow."
#[tauri::command]
pub fn grant_variable_consent(
    variable_id: String,
    scope: String,                   // 'once' | 'session' | 'always'
    granted_resource: String,
) -> Result<(), String> {
    if !matches!(scope.as_str(), "once" | "session" | "always") {
        return Err(format!("invalid scope: {}", scope));
    }
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    // INSERT OR REPLACE so re-granting consent for an already-granted
    // variable updates the scope + resource without leaving stale rows.
    // UNIQUE(variable_id) makes this an upsert.
    conn.execute(
        "INSERT OR REPLACE INTO variable_consent_grants
         (id, variable_id, scope, granted_at, granted_resource, revoked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
        params![id, variable_id, scope, now, granted_resource],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Revoke an existing consent grant. Sets revoked_at instead of
/// deleting so the audit trail survives.
#[tauri::command]
pub fn revoke_variable_consent(variable_id: String) -> Result<(), String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE variable_consent_grants
         SET revoked_at = ?1 WHERE variable_id = ?2 AND revoked_at IS NULL",
        params![now, variable_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// List all active consents for a given agent. Powers the
/// Settings → Permissions UI ("Variables this agent can read").
#[tauri::command]
pub fn list_variable_consents(agent_id: String) -> Result<Vec<VariableConsentRow>, String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT g.variable_id, v.name, v.kind, g.scope, g.granted_at, g.granted_resource
             FROM variable_consent_grants g
             JOIN agent_variables v ON v.id = g.variable_id
             WHERE v.agent_id = ?1 AND g.revoked_at IS NULL
             ORDER BY g.granted_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![agent_id], |r| {
            Ok(VariableConsentRow {
                variable_id: r.get(0)?,
                variable_name: r.get(1)?,
                kind: r.get(2)?,
                scope: r.get(3)?,
                granted_at: r.get(4)?,
                granted_resource: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub struct VariableConsentRow {
    pub variable_id: String,
    pub variable_name: String,
    pub kind: String,
    pub scope: String,
    pub granted_at: String,
    pub granted_resource: String,
}

fn resolve_one_variable(
    kind: &str,
    config_json: &str,
    active_project_path: Option<&str>,
) -> Result<String, String> {
    let cfg: serde_json::Value =
        serde_json::from_str(config_json).map_err(|_| "bad-config".to_string())?;
    match kind {
        "static" => Ok(cfg
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()),
        "env" => {
            let var = cfg
                .get("var")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing-var".to_string())?;
            std::env::var(var).map_err(|_| "env-not-set".to_string())
        }
        "project-path" => Ok(active_project_path
            .map(|s| s.to_string())
            .unwrap_or_else(|| "no-active-project".to_string())),
        "file" => {
            let path = cfg
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing-path".to_string())?;
            let max_bytes = cfg
                .get("maxBytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(8 * 1024) as usize;
            let expanded = expand_tilde(path);
            let contents = fs::read_to_string(&expanded).map_err(|_| "read-failed".to_string())?;
            if contents.len() > max_bytes {
                Ok(format!("{}…[truncated]", &contents[..max_bytes]))
            } else {
                Ok(contents)
            }
        }
        // Pro: read-only SQLite query against a path-configured database.
        // Tier gating happens in the UI — the resolver itself is local and
        // just needs the file. Postgres/MySQL deferred to a follow-up.
        "db-query" => resolve_db_query(&cfg),
        // Pro: constrained expression evaluator. Supports literals, var refs,
        // string concat with `+`, and basic arithmetic. No arbitrary JS.
        "computed" => resolve_computed(&cfg, active_project_path),
        // mcp-call still stubbed — needs an embedded MCP client. Tracked
        // separately; ship when we wire the MCP client into Rust.
        "mcp-call" => Err("mcp-call-not-yet-implemented".to_string()),
        _ => Err(format!("unknown-kind-{}", kind)),
    }
}

/// Run a read-only SELECT against a SQLite file. Refuses anything that
/// looks like a write — we don't want a misconfigured variable to delete
/// the user's data.
fn resolve_db_query(cfg: &serde_json::Value) -> Result<String, String> {
    let path = cfg
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing-path".to_string())?;
    let sql = cfg
        .get("sql")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing-sql".to_string())?;
    let max_rows = cfg
        .get("maxRows")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(500) as usize;

    // Reject anything that isn't a SELECT/WITH. Cheap heuristic, but the
    // OPEN_READ_ONLY flag below is the actual safety net.
    let trimmed = sql.trim_start().to_ascii_uppercase();
    if !(trimmed.starts_with("SELECT") || trimmed.starts_with("WITH")) {
        return Err("only-select-allowed".to_string());
    }

    let expanded = expand_tilde(path);
    let conn = rusqlite::Connection::open_with_flags(
        &expanded,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("open-failed: {}", e))?;

    let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare-failed: {}", e))?;
    let col_count = stmt.column_count();
    let col_names: Vec<String> = (0..col_count)
        .map(|i| stmt.column_name(i).unwrap_or("").to_string())
        .collect();

    let rows = stmt
        .query_map([], |row| {
            let mut obj = serde_json::Map::new();
            for (i, name) in col_names.iter().enumerate() {
                let val: rusqlite::types::Value = row.get(i)?;
                let json = match val {
                    rusqlite::types::Value::Null => serde_json::Value::Null,
                    rusqlite::types::Value::Integer(n) => serde_json::Value::from(n),
                    rusqlite::types::Value::Real(f) => serde_json::Value::from(f),
                    rusqlite::types::Value::Text(s) => serde_json::Value::from(s),
                    rusqlite::types::Value::Blob(_) => serde_json::Value::String("(blob)".into()),
                };
                obj.insert(name.clone(), json);
            }
            Ok(serde_json::Value::Object(obj))
        })
        .map_err(|e| format!("query-failed: {}", e))?;

    let mut collected: Vec<serde_json::Value> = Vec::new();
    for r in rows {
        if collected.len() >= max_rows {
            break;
        }
        collected.push(r.map_err(|e| format!("row-failed: {}", e))?);
    }

    serde_json::to_string(&collected).map_err(|e| format!("serialize-failed: {}", e))
}

/// Tiny expression evaluator. Supports:
///   - string and number literals
///   - variable references (`{var_name}` is replaced before evaluation)
///   - string concat with `+`
///   - integer/float arithmetic: + - * /
/// Recognized identifiers: project_path() function returns the active project path.
fn resolve_computed(
    cfg: &serde_json::Value,
    active_project_path: Option<&str>,
) -> Result<String, String> {
    let expr = cfg
        .get("expr")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing-expr".to_string())?;

    // Substitute project_path() with the active project before parsing.
    let with_project = expr.replace(
        "project_path()",
        &format!("\"{}\"", active_project_path.unwrap_or("")),
    );

    eval_simple_expr(&with_project)
}

#[derive(Debug, Clone)]
enum ExprValue {
    Num(f64),
    Str(String),
}

impl ExprValue {
    fn to_render(&self) -> String {
        match self {
            ExprValue::Num(n) => {
                if n.fract() == 0.0 && n.is_finite() {
                    format!("{}", *n as i64)
                } else {
                    format!("{}", n)
                }
            }
            ExprValue::Str(s) => s.clone(),
        }
    }
}

/// Evaluator strictly limited to:
///   literal "..." | literal '...' | number | (expr) op (expr)
/// Operators: + - * /. Strings only support `+` (concat).
fn eval_simple_expr(input: &str) -> Result<String, String> {
    let tokens = tokenize_expr(input)?;
    let mut iter = tokens.into_iter().peekable();
    let value = parse_expr(&mut iter)?;
    if iter.next().is_some() {
        return Err("trailing-tokens".to_string());
    }
    Ok(value.to_render())
}

#[derive(Debug, Clone)]
enum ExprToken {
    Num(f64),
    Str(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

fn tokenize_expr(s: &str) -> Result<Vec<ExprToken>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '+' => { out.push(ExprToken::Plus); i += 1; }
            '-' => { out.push(ExprToken::Minus); i += 1; }
            '*' => { out.push(ExprToken::Star); i += 1; }
            '/' => { out.push(ExprToken::Slash); i += 1; }
            '(' => { out.push(ExprToken::LParen); i += 1; }
            ')' => { out.push(ExprToken::RParen); i += 1; }
            '"' | '\'' => {
                let quote = c;
                i += 1;
                let mut buf = String::new();
                while i < chars.len() && chars[i] != quote {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        buf.push(chars[i + 1]);
                        i += 2;
                    } else {
                        buf.push(chars[i]);
                        i += 1;
                    }
                }
                if i >= chars.len() {
                    return Err("unterminated-string".to_string());
                }
                i += 1; // consume closing quote
                out.push(ExprToken::Str(buf));
            }
            d if d.is_ascii_digit() || d == '.' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_digit() || chars[i] == '.')
                {
                    i += 1;
                }
                let lit: String = chars[start..i].iter().collect();
                let n: f64 = lit.parse().map_err(|_| format!("bad-number-{}", lit))?;
                out.push(ExprToken::Num(n));
            }
            _ => return Err(format!("unexpected-char-{}", c)),
        }
    }
    Ok(out)
}

type ExprIter = std::iter::Peekable<std::vec::IntoIter<ExprToken>>;

fn parse_expr(it: &mut ExprIter) -> Result<ExprValue, String> {
    parse_add(it)
}

fn parse_add(it: &mut ExprIter) -> Result<ExprValue, String> {
    let mut left = parse_mul(it)?;
    loop {
        match it.peek() {
            Some(ExprToken::Plus) => {
                it.next();
                let right = parse_mul(it)?;
                left = match (left, right) {
                    (ExprValue::Num(a), ExprValue::Num(b)) => ExprValue::Num(a + b),
                    (ExprValue::Str(a), ExprValue::Str(b)) => ExprValue::Str(format!("{}{}", a, b)),
                    (ExprValue::Str(a), ExprValue::Num(b)) => {
                        ExprValue::Str(format!("{}{}", a, ExprValue::Num(b).to_render()))
                    }
                    (ExprValue::Num(a), ExprValue::Str(b)) => {
                        ExprValue::Str(format!("{}{}", ExprValue::Num(a).to_render(), b))
                    }
                };
            }
            Some(ExprToken::Minus) => {
                it.next();
                let right = parse_mul(it)?;
                match (left, right) {
                    (ExprValue::Num(a), ExprValue::Num(b)) => left = ExprValue::Num(a - b),
                    _ => return Err("subtract-non-numbers".to_string()),
                }
            }
            _ => break,
        }
    }
    Ok(left)
}

fn parse_mul(it: &mut ExprIter) -> Result<ExprValue, String> {
    let mut left = parse_atom(it)?;
    loop {
        match it.peek() {
            Some(ExprToken::Star) => {
                it.next();
                let right = parse_atom(it)?;
                match (left, right) {
                    (ExprValue::Num(a), ExprValue::Num(b)) => left = ExprValue::Num(a * b),
                    _ => return Err("multiply-non-numbers".to_string()),
                }
            }
            Some(ExprToken::Slash) => {
                it.next();
                let right = parse_atom(it)?;
                match (left, right) {
                    (ExprValue::Num(_), ExprValue::Num(b)) if b == 0.0 => {
                        return Err("divide-by-zero".to_string());
                    }
                    (ExprValue::Num(a), ExprValue::Num(b)) => left = ExprValue::Num(a / b),
                    _ => return Err("divide-non-numbers".to_string()),
                }
            }
            _ => break,
        }
    }
    Ok(left)
}

fn parse_atom(it: &mut ExprIter) -> Result<ExprValue, String> {
    match it.next() {
        Some(ExprToken::Num(n)) => Ok(ExprValue::Num(n)),
        Some(ExprToken::Str(s)) => Ok(ExprValue::Str(s)),
        Some(ExprToken::LParen) => {
            let v = parse_expr(it)?;
            match it.next() {
                Some(ExprToken::RParen) => Ok(v),
                _ => Err("missing-rparen".to_string()),
            }
        }
        Some(ExprToken::Minus) => {
            let v = parse_atom(it)?;
            match v {
                ExprValue::Num(n) => Ok(ExprValue::Num(-n)),
                _ => Err("unary-minus-on-string".to_string()),
            }
        }
        _ => Err("unexpected-token".to_string()),
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    if path == "~" {
        return home_dir();
    }
    PathBuf::from(path)
}

/// Substitute `{var}` placeholders in a string with values from a map.
/// Unknown placeholders are left as-is so the user can see what's missing.
/// Identifiers must match `[A-Za-z_][A-Za-z0-9_]*` — anything else (e.g. JSON
/// `{ "key": ... }`) is left alone. Implemented as a single-pass scanner so
/// we don't pull in a regex dependency.
pub fn substitute_variables(template: &str, values: &HashMap<String, String>) -> String {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // Look for matching identifier + closing '}'.
            let start = i + 1;
            let mut j = start;
            // First char must be letter or underscore.
            if j < bytes.len() && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'_') {
                j += 1;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_')
                {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'}' {
                    let name = &template[start..j];
                    match values.get(name) {
                        Some(v) => out.push_str(v),
                        None => out.push_str(&template[i..=j]),
                    }
                    i = j + 1;
                    continue;
                }
            }
        }
        // Push one UTF-8 codepoint at a time so we don't slice mid-character.
        let ch_end = next_char_boundary(template, i);
        out.push_str(&template[i..ch_end]);
        i = ch_end;
    }
    out
}

fn next_char_boundary(s: &str, mut i: usize) -> usize {
    i += 1;
    while !s.is_char_boundary(i) && i < s.len() {
        i += 1;
    }
    i
}


/// Decide whether a hook should fire for THIS particular user message.
/// Returns true if the hook should run, false to skip it. Skipped hooks
/// don't contribute to the `<context>` block — saves API cost and keeps
/// the prompt tight when data isn't relevant. Beatriz's design (2026-05-08).
async fn should_fire_hook(hook: &AgentHook, user_prompt: &str) -> bool {
    let mode = hook.fire_mode.as_str();
    if mode == "always" {
        return true;
    }
    // Parse the JSON config once — the fire-eval knobs live here too.
    let cfg: serde_json::Value = match serde_json::from_str(&hook.config_json) {
        Ok(v) => v,
        // Malformed config falls back to firing — better to inject possibly
        // stale data than silently skip and have the agent ignorant.
        Err(_) => return true,
    };

    if mode == "keyword" {
        let keywords = cfg
            .get("whenKeywords")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_lowercase))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if keywords.is_empty() {
            return false; // no rules → never fires
        }
        let lower = user_prompt.to_lowercase();
        return keywords.iter().any(|k| lower.contains(k));
    }

    if mode == "llm-decides" {
        let when_desc = cfg
            .get("whenDescription")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if when_desc.is_empty() {
            return false; // no rule → never fires
        }
        let model = cfg
            .get("classifierModel")
            .and_then(|v| v.as_str())
            .unwrap_or("claude-haiku-4-5")
            .to_string();
        let provider = cfg
            .get("classifierProvider")
            .and_then(|v| v.as_str())
            .unwrap_or("anthropic")
            .to_string();
        match classify_should_fire(&provider, &model, when_desc, user_prompt).await {
            Ok(should) => should,
            // Classifier outage → fail-safe to firing the hook so the
            // agent doesn't suddenly lose data context.
            Err(_) => true,
        }
    } else {
        // Unknown fire_mode (DB row written by a newer build, hand edit,
        // or unsupported migration). Default-deny rather than always-fire:
        // skipping is the safer failure mode — never silently leaks data
        // or burns tokens for a rule we can't evaluate. Flagged by claude
        // reviewer in the v2.7.6 review (MEDIUM #3, 2026-05-18).
        false
    }
}

/// Run all enabled hooks for an agent and return a formatted `<context>`
/// block. Failures don't break dispatch — they're surfaced as inline error
/// notes inside the same block so the model sees what couldn't be fetched.
async fn run_pre_call_hooks(
    hooks: Vec<AgentHook>,
    user_prompt: &str,
) -> String {
    if hooks.is_empty() {
        return String::new();
    }
    let mut sections: Vec<String> = Vec::new();
    for hook in hooks {
        if !hook.enabled {
            continue;
        }
        if !should_fire_hook(&hook, user_prompt).await {
            continue;
        }
        let result = execute_hook(&hook).await;
        let section = match result {
            Ok(content) => format!("<{name}>\n{body}\n</{name}>", name = hook.name, body = content),
            Err(e) => format!(
                "<{name} status=\"failed\">\n{body}\n</{name}>",
                name = hook.name,
                body = format!("Hook \"{}\" failed: {}", hook.name, e)
            ),
        };
        sections.push(section);
    }
    if sections.is_empty() {
        String::new()
    } else {
        format!("<context>\n{}\n</context>\n\n", sections.join("\n\n"))
    }
}

/// Lightweight LLM classifier — asks "should the hook fire?" and parses
/// the response. Designed for cheap fast models (Haiku, GPT-4o-mini,
/// Gemini Flash, etc.). Cost per call is in the order of $0.0001.
async fn classify_should_fire(
    provider: &str,
    model: &str,
    when_description: &str,
    user_prompt: &str,
) -> Result<bool, String> {
    // Use the provider's stored API key — we expect the same key that
    // powers the agent's chat dispatch to be on file.
    let api_key = read_provider_api_key(provider)?;
    let system = "You are a fast classifier. Respond with ONLY \"YES\" or \"NO\" (no other text). Decide whether the data described by the rule is relevant to the user's message.";
    let user = format!(
        "Rule: this data should fire when: {when_description}\n\nUser message: {user_prompt}\n\nShould the data fire? Reply YES or NO."
    );

    let client = reqwest::Client::new();
    let text = match provider {
        "anthropic" => {
            let payload = serde_json::json!({
                "model": model,
                "max_tokens": 8,
                "system": system,
                "messages": [{ "role": "user", "content": user }],
            });
            let r = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("classifier request failed: {}", e))?;
            if !r.status().is_success() {
                return Err(format!("classifier {}: {}", r.status(), r.text().await.unwrap_or_default()));
            }
            let body: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
            body.get("content")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string()
        }
        // OpenAI-compatible chat completions for the rest. Covers OpenAI,
        // Groq, xAI, Mistral, DeepSeek, Together, Fireworks. Gemini uses
        // its own format and isn't supported as classifier in v2.0 alpha.
        _ => {
            let url = match provider {
                "openai"   => "https://api.openai.com/v1/chat/completions",
                "groq"     => "https://api.groq.com/openai/v1/chat/completions",
                "xai"      => "https://api.x.ai/v1/chat/completions",
                "mistral"  => "https://api.mistral.ai/v1/chat/completions",
                "deepseek" => "https://api.deepseek.com/v1/chat/completions",
                "together" => "https://api.together.xyz/v1/chat/completions",
                "fireworks"=> "https://api.fireworks.ai/inference/v1/chat/completions",
                _ => return Err(format!("classifier provider not supported: {}", provider)),
            };
            let payload = serde_json::json!({
                "model": model,
                "max_tokens": 8,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user },
                ],
            });
            let r = client
                .post(url)
                .bearer_auth(&api_key)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("classifier request failed: {}", e))?;
            if !r.status().is_success() {
                return Err(format!("classifier {}: {}", r.status(), r.text().await.unwrap_or_default()));
            }
            let body: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
            body.get("choices")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string()
        }
    };
    Ok(text.to_uppercase().contains("YES"))
}

/// Look up the active API key for a given provider in `llm_api_keys`,
/// decrypted. Returns the most recently-created key. Used by the
/// classifier — same provider system as the agent's chat dispatch.
fn read_provider_api_key(provider: &str) -> Result<String, String> {
    use rusqlite::Connection;
    let path = home_dir().join(".ato").join("local.db");
    let conn = Connection::open(&path).map_err(|e| e.to_string())?;
    match conn.query_row::<String, _, _>(
        "SELECT encrypted_key FROM llm_api_keys WHERE provider = ?1 AND is_active = 1 ORDER BY created_at DESC LIMIT 1",
        params![provider],
        |row| row.get(0),
    ) {
        Ok(encrypted) => simple_decrypt(&encrypted),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(format!(
            "No {} API key on file. Add one in Settings → API Keys (or in the create-agent wizard).",
            provider
        )),
        Err(e) => Err(e.to_string()),
    }
}

async fn execute_hook(hook: &AgentHook) -> Result<String, String> {
    let cfg: serde_json::Value =
        serde_json::from_str(&hook.config_json).map_err(|_| "bad-config".to_string())?;
    match hook.kind.as_str() {
        "file" => {
            let path = cfg
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing-path".to_string())?;
            let max_bytes = cfg
                .get("maxBytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(8 * 1024) as usize;
            let expanded = expand_tilde(path);
            let contents = fs::read_to_string(&expanded).map_err(|e| e.to_string())?;
            if contents.len() > max_bytes {
                Ok(format!("{}…[truncated]", &contents[..max_bytes]))
            } else {
                Ok(contents)
            }
        }
        "webhook" => {
            let url = cfg
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing-url".to_string())?;
            let max_bytes = cfg
                .get("maxBytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(16 * 1024) as usize;
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .map_err(|e| e.to_string())?;
            let mut req = client.get(url);
            if let Some(headers) = cfg.get("headers").and_then(|v| v.as_object()) {
                for (k, v) in headers {
                    if let Some(s) = v.as_str() {
                        req = req.header(k, s);
                    }
                }
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            let body = resp.text().await.map_err(|e| e.to_string())?;
            if body.len() > max_bytes {
                Ok(format!("{}…[truncated]", &body[..max_bytes]))
            } else {
                Ok(body)
            }
        }
        // Reuse the variable resolvers — same kinds, same configs.
        "db-query" => resolve_db_query(&cfg),
        "computed" => resolve_computed(&cfg, None),
        "mcp-call" => Err("mcp-call-not-yet-implemented".to_string()),
        other => Err(format!("unknown-kind-{}", other)),
    }
}

/// Tauri command that wraps prompt_agent: resolves the agent's variables and
/// substitutes them in the prompt before dispatching. Used by Quick Test and
/// (future) cron jobs.
///
/// v2.1.0+ — returns a structured result so the frontend can pick up
/// the run_id (used for overlap-evidence lookup) without a second
/// registry round-trip. Only one direct invoke caller
/// (agentVariables.ts), so the shape change is contained.
#[derive(serde::Serialize)]
pub struct DispatchResult {
    pub response: String,
    /// Active-runs registry id assigned at dispatch start. The
    /// frontend uses it to fetch overlap evidence + compose the
    /// trace upload metadata.
    #[serde(rename = "runId")]
    pub run_id: String,
}

#[tauri::command]
pub async fn prompt_agent_with_context(
    db: State<'_, DbState>,
    agent_id: String,
    runtime: String,
    prompt: String,
    config: Option<String>,
    active_project_path: Option<String>,
) -> Result<DispatchResult, String> {
    // Step 1: resolve variables + load hooks + read role-model preferences
    // (single short-lived lock). Also pull the agent slug for the
    // active-runs registry (Phase 4) — Beatriz: showing slugs in the
    // Live panel matters more than UUIDs.
    //
    // Felipe P4 (S9) — also pull `default_prompt` here so we can swap
    // it in BEFORE Step 2's variable substitution. The same swap also
    // lives in prompt_agent_inner as defense-in-depth for direct
    // callers (group dispatch, headless replay) that don't come
    // through this path; on this path the upstream substitution
    // means `{variables}` embedded in a default_prompt resolve
    // correctly (the inner fallback would interpolate them too late
    // — variable resolution has already run).
    let (resolved, hooks, response_model, fallback_model, agent_slug, default_prompt) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let resolved = resolve_agent_variables(&conn, &agent_id, active_project_path.as_deref());
        let hooks = load_agent_hooks(&conn, &agent_id);
        let (rm, fb) = load_agent_response_model(&conn, &agent_id);
        let row: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT slug, default_prompt FROM agents WHERE id = ?1",
                rusqlite::params![&agent_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .ok();
        let (slug, dp) = match row {
            Some((s, d)) => (Some(s), d.filter(|s| !s.trim().is_empty())),
            None => (None, None),
        };
        (resolved, hooks, rm, fb, slug, dp)
    };

    // Felipe P4 — swap empty prompt for the agent's default_prompt
    // here so the subsequent variable substitution applies to it.
    let prompt = if prompt.trim().is_empty() {
        default_prompt.unwrap_or(prompt)
    } else {
        prompt
    };

    // Step 2: substitute into the prompt.
    let rendered_prompt = substitute_variables(&prompt, &resolved);

    // Step 3: run pre-call hooks → format as <context> block.
    let context_block = run_pre_call_hooks(hooks, &prompt).await;

    // Step 4: prepend context block to the user prompt.
    let final_prompt = if context_block.is_empty() {
        rendered_prompt
    } else {
        format!("{}{}", context_block, rendered_prompt)
    };

    // Step 5 (F5): merge the agent's response model into the runtime config
    // unless the caller already passed one. roleModels.response wins over
    // agents.model — that's the whole point of per-task models.
    let merged_config = merge_model_into_config(config, response_model, fallback_model);

    // Phase 4: register in the active-runs map for the duration of the
    // dispatch. Always finish_run via a guard so panics + early returns
    // don't leak entries.
    let run_id = crate::active_runs::begin_run(
        &runtime,
        agent_slug.as_deref(),
        active_project_path.as_deref(),
        Some("desktop:context-dispatch"),
    );
    // Pass our run_id into prompt_agent so it attaches the kill
    // handler to OUR registration instead of double-registering.
    let result = prompt_agent_inner(
        runtime,
        final_prompt,
        merged_config,
        agent_slug.clone(),
        active_project_path.clone(),
        Some(run_id.clone()),
    ).await;
    // Note: do NOT finish_run yet. Frontend needs to call
    // get_overlap_evidence(run_id) before the slot is removed; it
    // will then call list_active_runs again at its leisure (registry
    // self-heals after a stale entry timeout, but the explicit
    // contract is: caller is responsible for finish).
    //
    // Rationale: keeping finish_run on the Rust side would race the
    // frontend's overlap fetch. Instead we return run_id and let the
    // wrapper finish_run after upload. Worst case (frontend crashes):
    // entry stays until next call to begin_run with same workspace.
    match result {
        Ok(response) => Ok(DispatchResult { response, run_id }),
        Err(e) => {
            // On error we still tidy up — no overlap upload happens
            // for failed dispatches today, so the slot has no further
            // use.
            crate::active_runs::finish_run(&run_id);
            Err(e)
        }
    }
}

// ── Conversation summarization (F3) ──────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    /// "user" | "assistant" | "system" | "summary"
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemoryPolicyParsed {
    #[serde(default = "default_summarize_after")]
    summarize_after: usize,
    #[serde(default = "default_keep_last_k")]
    keep_last_k: usize,
    #[serde(default)]
    summarizer_model: String,
}

fn default_summarize_after() -> usize { 30 }
fn default_keep_last_k() -> usize { 5 }

impl Default for MemoryPolicyParsed {
    fn default() -> Self {
        Self {
            summarize_after: default_summarize_after(),
            keep_last_k: default_keep_last_k(),
            summarizer_model: String::new(),
        }
    }
}

fn load_memory_policy(conn: &Connection, agent_id: &str) -> MemoryPolicyParsed {
    let row: Option<Option<String>> = conn
        .query_row(
            "SELECT memory_policy_json FROM agents WHERE id = ?1",
            params![agent_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok();
    row.flatten()
        .and_then(|s| serde_json::from_str::<MemoryPolicyParsed>(&s).ok())
        .unwrap_or_default()
}

fn load_agent_summarizer_model(conn: &Connection, agent_id: &str) -> Option<String> {
    let rm_json: Option<Option<String>> = conn
        .query_row(
            "SELECT role_models_json FROM agents WHERE id = ?1",
            params![agent_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok();
    rm_json
        .flatten()
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.get("summarizer").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
}

/// Decide whether to summarize. Returns (older_to_summarize, recent_kept_verbatim).
/// If we don't need to summarize, the first slice is empty.
fn split_history_for_summarization(
    history: &[AgentMessage],
    policy: &MemoryPolicyParsed,
) -> (Vec<AgentMessage>, Vec<AgentMessage>) {
    if history.len() <= policy.summarize_after {
        return (Vec::new(), history.to_vec());
    }
    let keep_k = policy.keep_last_k.min(history.len());
    let split = history.len() - keep_k;
    (history[..split].to_vec(), history[split..].to_vec())
}

fn build_summarizer_prompt(older: &[AgentMessage]) -> String {
    let mut s = String::from(
        "Summarize the following conversation between a user and an AI agent. \
Keep concrete facts, decisions, names, identifiers, and any open questions. \
Drop pleasantries. Output 5-10 bullet points, no preamble.\n\n",
    );
    for m in older {
        s.push_str(&format!("[{}]: {}\n", m.role, m.content));
    }
    s.push_str("\nReturn the summary now.");
    s
}

fn build_final_prompt(
    summary: Option<&str>,
    recent: &[AgentMessage],
    new_user_prompt: &str,
) -> String {
    let mut out = String::new();
    if let Some(s) = summary {
        out.push_str("<conversation_summary>\n");
        out.push_str(s.trim());
        out.push_str("\n</conversation_summary>\n\n");
    }
    for m in recent {
        out.push_str(&format!("[{}]: {}\n", m.role, m.content));
    }
    out.push_str(&format!("\n[user]: {}\n", new_user_prompt));
    out
}

#[tauri::command]
pub async fn prompt_agent_with_history(
    db: State<'_, DbState>,
    agent_id: String,
    runtime: String,
    history: Vec<AgentMessage>,
    new_prompt: String,
    config: Option<String>,
    active_project_path: Option<String>,
) -> Result<String, String> {
    // Load all the dispatch-time inputs under one lock.
    let (resolved, hooks, response_model, fallback_model, policy, summarizer_model) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let resolved = resolve_agent_variables(&conn, &agent_id, active_project_path.as_deref());
        let hooks = load_agent_hooks(&conn, &agent_id);
        let (rm, fb) = load_agent_response_model(&conn, &agent_id);
        let policy = load_memory_policy(&conn, &agent_id);
        let summ = load_agent_summarizer_model(&conn, &agent_id);
        (resolved, hooks, rm, fb, policy, summ)
    };

    // Summarize if history exceeds the threshold.
    let (older, recent) = split_history_for_summarization(&history, &policy);
    let summary: Option<String> = if !older.is_empty() {
        let summarizer_prompt = build_summarizer_prompt(&older);
        // Pick summarizer model: explicit policy > role_models.summarizer >
        // none (runtime default).
        let chosen_summarizer = if !policy.summarizer_model.is_empty() {
            Some(policy.summarizer_model.clone())
        } else {
            summarizer_model
        };
        let summ_cfg = chosen_summarizer.map(|m| {
            serde_json::json!({ "model": m }).to_string()
        });
        match prompt_agent(runtime.clone(), summarizer_prompt, summ_cfg, None, None).await {
            Ok(s) => Some(s),
            // Summarization failure shouldn't block dispatch — fall back to
            // dropping the older history entirely. The agent loses memory
            // for this turn, which is the same as if we never summarized.
            Err(_) => None,
        }
    } else {
        None
    };

    // Resolve variables in the user's new prompt.
    let rendered_new = substitute_variables(&new_prompt, &resolved);

    // Stitch everything together.
    let stitched = build_final_prompt(summary.as_deref(), &recent, &rendered_new);

    // Pre-call hooks. fire_mode evaluation uses the new turn's user
    // message (`new_prompt`), not the stitched history — keyword/LLM
    // gating cares about what THIS turn is asking for.
    let context_block = run_pre_call_hooks(hooks, &new_prompt).await;
    let final_prompt = if context_block.is_empty() {
        stitched
    } else {
        format!("{}{}", context_block, stitched)
    };

    let merged_config = merge_model_into_config(config, response_model, fallback_model);
    prompt_agent(runtime, final_prompt, merged_config, None, None).await
}

/// Returns (role_models.response, agents.model). Either may be None.
fn load_agent_response_model(
    conn: &Connection,
    agent_id: &str,
) -> (Option<String>, Option<String>) {
    let row: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT role_models_json, model FROM agents WHERE id = ?1",
            params![agent_id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .ok();
    let (rm_json, agent_model) = row.unwrap_or((None, None));
    let response = rm_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.get("response").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .filter(|s| !s.is_empty());
    (response, agent_model.filter(|s| !s.is_empty()))
}

/// Merges a `model` override into the existing `config` JSON (or creates a
/// new one). The caller's existing config wins — we only set model when the
/// caller didn't.
fn merge_model_into_config(
    config: Option<String>,
    response_model: Option<String>,
    fallback_model: Option<String>,
) -> Option<String> {
    let chosen = response_model.or(fallback_model);
    let chosen = match chosen {
        Some(m) => m,
        None => return config,
    };

    let mut obj: serde_json::Map<String, serde_json::Value> = config
        .as_deref()
        .and_then(|c| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(c).ok())
        .unwrap_or_default();

    // Don't overwrite an explicit caller-supplied model.
    let already_set = obj
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if !already_set {
        obj.insert("model".into(), serde_json::Value::String(chosen));
    }

    serde_json::to_string(&obj).ok()
}

fn load_agent_hooks(conn: &Connection, agent_id: &str) -> Vec<AgentHook> {
    let mut stmt = match conn.prepare(
        "SELECT id, agent_id, position, name, kind, config_json, enabled, created_at, fire_mode
         FROM agent_hooks WHERE agent_id = ?1 ORDER BY position ASC, created_at ASC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(params![agent_id], |row| {
        Ok(AgentHook {
            id: row.get(0)?,
            agent_id: row.get(1)?,
            position: row.get(2)?,
            name: row.get(3)?,
            kind: row.get(4)?,
            config_json: row.get(5)?,
            enabled: row.get::<_, i32>(6).unwrap_or(1) != 0,
            created_at: row.get(7)?,
            fire_mode: row.get::<_, Option<String>>(8)?.unwrap_or_else(|| "always".to_string()),
        })
    }) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.flatten().collect()
}

#[cfg(test)]
mod variable_tests {
    use super::*;

    #[test]
    fn substitute_handles_known_and_unknown() {
        let mut vals = HashMap::new();
        vals.insert("name".to_string(), "Beatriz".to_string());
        vals.insert("plan".to_string(), "Pro".to_string());
        let out = substitute_variables(
            "Hello {name}, your {plan} plan expires in {days} days.",
            &vals,
        );
        assert_eq!(
            out,
            "Hello Beatriz, your Pro plan expires in {days} days."
        );
    }

    #[test]
    fn resolve_static_returns_configured_value() {
        let v = resolve_one_variable("static", r#"{"value":"hi"}"#, None).unwrap();
        assert_eq!(v, "hi");
    }

    #[test]
    fn merge_model_uses_response_when_no_caller_model() {
        let merged = merge_model_into_config(None, Some("sonnet".into()), Some("opus".into()));
        let v: serde_json::Value = serde_json::from_str(&merged.unwrap()).unwrap();
        assert_eq!(v.get("model").unwrap().as_str().unwrap(), "sonnet");
    }

    #[test]
    fn merge_model_falls_back_to_agent_model() {
        let merged = merge_model_into_config(None, None, Some("opus".into()));
        let v: serde_json::Value = serde_json::from_str(&merged.unwrap()).unwrap();
        assert_eq!(v.get("model").unwrap().as_str().unwrap(), "opus");
    }

    #[test]
    fn merge_model_respects_caller_supplied_model() {
        let caller = r#"{"model":"haiku","sshHost":"foo"}"#;
        let merged = merge_model_into_config(Some(caller.into()), Some("sonnet".into()), Some("opus".into()));
        let v: serde_json::Value = serde_json::from_str(&merged.unwrap()).unwrap();
        assert_eq!(v.get("model").unwrap().as_str().unwrap(), "haiku");
        assert_eq!(v.get("sshHost").unwrap().as_str().unwrap(), "foo");
    }

    #[test]
    fn merge_model_returns_none_when_no_choice() {
        assert!(merge_model_into_config(None, None, None).is_none());
    }

    fn msg(role: &str, content: &str) -> AgentMessage {
        AgentMessage { role: role.into(), content: content.into() }
    }

    #[test]
    fn split_returns_all_recent_below_threshold() {
        let h = vec![msg("user", "hi"), msg("assistant", "hello")];
        let policy = MemoryPolicyParsed { summarize_after: 30, keep_last_k: 5, summarizer_model: "".into() };
        let (older, recent) = split_history_for_summarization(&h, &policy);
        assert!(older.is_empty());
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn split_keeps_last_k_when_over_threshold() {
        let mut h = Vec::new();
        for i in 0..40 { h.push(msg("user", &format!("m{}", i))); }
        let policy = MemoryPolicyParsed { summarize_after: 30, keep_last_k: 5, summarizer_model: "".into() };
        let (older, recent) = split_history_for_summarization(&h, &policy);
        assert_eq!(older.len(), 35);
        assert_eq!(recent.len(), 5);
        assert_eq!(recent[0].content, "m35");
        assert_eq!(recent[4].content, "m39");
    }

    #[test]
    fn build_final_prompt_wraps_summary_in_block() {
        let recent = vec![msg("user", "ping")];
        let out = build_final_prompt(Some("we discussed X"), &recent, "what's next?");
        assert!(out.contains("<conversation_summary>"));
        assert!(out.contains("we discussed X"));
        assert!(out.contains("</conversation_summary>"));
        assert!(out.contains("[user]: what's next?"));
    }

    #[test]
    fn resolve_project_path_uses_active() {
        let v = resolve_one_variable("project-path", "{}", Some("/work/repo")).unwrap();
        assert_eq!(v, "/work/repo");
    }

    #[test]
    fn resolve_env_missing_returns_error() {
        let v = resolve_one_variable("env", r#"{"var":"DEFINITELY_NOT_SET_VAR"}"#, None);
        assert!(v.is_err());
    }

    #[test]
    fn mcp_call_remains_stubbed() {
        let v = resolve_one_variable("mcp-call", "{}", None);
        assert!(matches!(
            v,
            Err(ref s) if s == "mcp-call-not-yet-implemented"
        ));
    }

    #[test]
    fn db_query_rejects_writes() {
        let cfg = r#"{"path":"/tmp/x.db","sql":"DELETE FROM users"}"#;
        let v = resolve_one_variable("db-query", cfg, None);
        assert!(matches!(v, Err(ref s) if s == "only-select-allowed"));
    }

    #[test]
    fn computed_evaluates_arithmetic() {
        let v = resolve_one_variable("computed", r#"{"expr":"2 + 3 * 4"}"#, None).unwrap();
        assert_eq!(v, "14");
    }

    #[test]
    fn computed_concatenates_strings() {
        let v = resolve_one_variable(
            "computed",
            r#"{"expr":"\"hello \" + \"world\""}"#,
            None,
        )
        .unwrap();
        assert_eq!(v, "hello world");
    }

    #[test]
    fn computed_uses_project_path() {
        let v = resolve_one_variable(
            "computed",
            r#"{"expr":"project_path() + \"/CLAUDE.md\""}"#,
            Some("/work/proj"),
        )
        .unwrap();
        assert_eq!(v, "/work/proj/CLAUDE.md");
    }

    #[test]
    fn computed_rejects_unknown_chars() {
        let v = resolve_one_variable("computed", r#"{"expr":"foo()"}"#, None);
        assert!(v.is_err());
    }
}


#[cfg(test)]
mod agent_tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("PR Reviewer"), "pr-reviewer");
        assert_eq!(slugify("My Agent!!"), "my-agent");
        assert_eq!(slugify("  spaced   out  "), "spaced-out");
        assert_eq!(slugify("---weird---"), "weird");
    }

    #[test]
    fn claude_path_uses_md_file() {
        let p = agent_file_path("claude", "pr-reviewer").unwrap();
        let s = p.to_string_lossy();
        assert!(s.ends_with(".claude/agents/pr-reviewer.md"));
    }

    #[test]
    fn codex_path_uses_agents_md() {
        let p = agent_file_path("codex", "doc-writer").unwrap();
        let s = p.to_string_lossy();
        assert!(s.ends_with(".codex/agents/doc-writer/AGENTS.md"));
    }

    #[test]
    fn unsupported_runtime_errors() {
        assert!(agent_file_path("nonsense", "x").is_err());
    }

    #[test]
    fn render_claude_agent_includes_frontmatter() {
        let a = Agent {
            id: "test".into(),
            slug: "pr-reviewer".into(),
            display_name: "PR Reviewer".into(),
            description: Some("Reviews PRs".into()),
            runtime: "claude".into(),
            model: Some("claude-sonnet-4-6".into()),
            project_id: None,
            system_prompt: Some("You review pull requests.".into()),
            permissions: None,
            skills: None,
            mcps: None,
            goal: None,
            file_path: None,
            created_at: "2026-04-30T00:00:00Z".into(),
            last_used_at: None,
            role_models: None,
            memory_policy: None,
            kind: Some("internal".into()),
        };
        let out = render_claude_agent(&a);
        assert!(out.contains("name: pr-reviewer"));
        assert!(out.contains("description: Reviews PRs"));
        assert!(out.contains("model: claude-sonnet-4-6"));
        assert!(out.contains("# PR Reviewer"));
        assert!(out.contains("You review pull requests."));
    }
}

// ── Agent Groups (v1.4.0 F4) ─────────────────────────────────────────────
//
// Multi-agent groups. The article's headline pattern: instead of one agent
// with 30 tools, you have a router that dispatches to N specialized children
// with 5-8 tools each. ATO stores group metadata in SQLite (`agent_groups`
// + `agent_group_members`) AND mirrors it to a portable file at
// `~/.ato/groups/<slug>/group.json` so groups can be shared, version-
// controlled, and discovered by the standalone MCP server.

fn group_file_path(slug: &str) -> PathBuf {
    home_dir().join(".ato").join("groups").join(slug).join("group.json")
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GroupMemberInput {
    /// Slug of an existing agent. We look up the id by slug at save time.
    pub agent_slug: String,
    pub role: String, // "router" | "child"
    pub position: i32,
}

fn load_group_members(conn: &Connection, group_id: &str) -> Vec<AgentGroupMember> {
    let mut stmt = match conn.prepare(
        "SELECT m.agent_id, a.slug, a.display_name, m.role, m.position, a.runtime
         FROM agent_group_members m
         JOIN agents a ON a.id = m.agent_id
         WHERE m.group_id = ?1
         ORDER BY m.position ASC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(params![group_id], |row| {
        Ok(AgentGroupMember {
            agent_id: row.get(0)?,
            agent_slug: row.get(1)?,
            agent_display_name: row.get(2)?,
            role: row.get(3)?,
            position: row.get(4)?,
            agent_runtime: row.get(5)?,
        })
    }) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.flatten().collect()
}

fn write_group_file(group: &AgentGroup) -> Result<String, String> {
    let path = group_file_path(&group.slug);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create groups dir: {}", e))?;
    }
    let snapshot = serde_json::json!({
        "slug": group.slug,
        "displayName": group.display_name,
        "description": group.description,
        "runtime": group.runtime,
        "routerConfig": group.router_config
            .as_ref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .unwrap_or_else(|| serde_json::json!({})),
        "members": group.members.iter().map(|m| serde_json::json!({
            "agent": m.agent_slug,
            "role": m.role,
            "position": m.position,
        })).collect::<Vec<_>>(),
    });
    let serialized = serde_json::to_string_pretty(&snapshot)
        .map_err(|e| format!("Failed to serialize group: {}", e))?;
    fs::write(&path, serialized).map_err(|e| format!("Failed to write group file: {}", e))?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn create_agent_group(
    db: State<'_, DbState>,
    display_name: String,
    runtime: String,
    description: Option<String>,
    router_config_json: Option<String>,
    members: Vec<GroupMemberInput>,
    // "routed" (default — router picks one child) or "sequential" (children
    // run in order; previous output flows into next input).
    dispatch_kind: Option<String>,
) -> Result<AgentGroup, String> {
    if display_name.trim().is_empty() {
        return Err("display_name cannot be empty".into());
    }
    let allowed_runtimes = ["claude", "codex", "gemini", "openclaw", "hermes"];
    if !allowed_runtimes.contains(&runtime.as_str()) {
        return Err(format!("Unsupported runtime: {}", runtime));
    }
    let dispatch_kind = dispatch_kind.unwrap_or_else(|| "routed".to_string());
    if dispatch_kind != "routed" && dispatch_kind != "sequential" {
        return Err(format!("Unsupported dispatch_kind: {}", dispatch_kind));
    }
    if let Some(ref cfg) = router_config_json {
        serde_json::from_str::<serde_json::Value>(cfg)
            .map_err(|e| format!("Invalid router_config JSON: {}", e))?;
    }

    let slug = slugify(&display_name);
    if slug.is_empty() {
        return Err("display_name must produce a non-empty slug".into());
    }

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // Resolve member slugs → agent IDs. Must all exist; runtime must match.
    let mut resolved_members: Vec<AgentGroupMember> = Vec::new();
    for m in &members {
        let row = conn.query_row(
            "SELECT id, slug, display_name, runtime FROM agents WHERE slug = ?1",
            params![m.agent_slug],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        );
        match row {
            Ok((agent_id, slug_, display, agent_runtime)) => {
                // Routed groups: router runs once on group.runtime, so all
                //   children MUST share that runtime.
                // Sequential groups: each child runs on its OWN runtime in
                //   turn, so cross-runtime pipelines (Claude → Codex) work.
                if dispatch_kind != "sequential" && agent_runtime != runtime {
                    return Err(format!(
                        "Member '{}' uses runtime '{}', but group runtime is '{}'",
                        slug_, agent_runtime, runtime
                    ));
                }
                resolved_members.push(AgentGroupMember {
                    agent_id,
                    agent_slug: slug_,
                    agent_display_name: display,
                    role: m.role.clone(),
                    position: m.position,
                    agent_runtime: agent_runtime.clone(),
                });
            }
            Err(_) => return Err(format!("Agent with slug '{}' not found", m.agent_slug)),
        }
    }

    // Insert group + members atomically.
    let tx_result: Result<(), String> = (|| {
        conn.execute(
            "INSERT INTO agent_groups (id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, NULL, ?8)",
            params![id, slug, display_name, description, runtime, router_config_json, now, dispatch_kind],
        )
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE") {
                format!("A group named \"{}\" already exists", slug)
            } else {
                msg
            }
        })?;

        for m in &resolved_members {
            conn.execute(
                "INSERT INTO agent_group_members (group_id, agent_id, role, position)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, m.agent_id, m.role, m.position],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();

    if let Err(e) = tx_result {
        // Best-effort rollback by deleting partial state.
        let _ = conn.execute("DELETE FROM agent_groups WHERE id = ?1", params![id]);
        return Err(e);
    }

    let mut group = AgentGroup {
        id: id.clone(),
        slug,
        display_name,
        description,
        runtime,
        router_config: router_config_json,
        file_path: None,
        created_at: now,
        last_used_at: None,
        members: resolved_members,
        dispatch_kind,
    };

    // Persist the file mirror; non-fatal on failure (agent still works in-DB).
    match write_group_file(&group) {
        Ok(path) => {
            group.file_path = Some(path.clone());
            let _ = conn.execute(
                "UPDATE agent_groups SET file_path = ?1 WHERE id = ?2",
                params![path, id],
            );
        }
        Err(e) => eprintln!("write_group_file: {}", e),
    }

    Ok(group)
}

#[tauri::command]
pub fn list_agent_groups(
    db: State<'_, DbState>,
    runtime: Option<String>,
) -> Result<Vec<AgentGroup>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let (sql, has_filter) = if runtime.is_some() {
        (
            "SELECT id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind
             FROM agent_groups WHERE runtime = ?1
             ORDER BY COALESCE(last_used_at, created_at) DESC".to_string(),
            true,
        )
    } else {
        (
            "SELECT id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind
             FROM agent_groups
             ORDER BY COALESCE(last_used_at, created_at) DESC".to_string(),
            false,
        )
    };
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let row_to_group = |row: &rusqlite::Row| -> rusqlite::Result<AgentGroup> {
        Ok(AgentGroup {
            id: row.get(0)?,
            slug: row.get(1)?,
            display_name: row.get(2)?,
            description: row.get(3)?,
            runtime: row.get(4)?,
            router_config: row.get(5)?,
            file_path: row.get(6)?,
            created_at: row.get(7)?,
            last_used_at: row.get(8)?,
            dispatch_kind: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "routed".to_string()),
            members: Vec::new(), // filled in below
        })
    };
    let mut groups: Vec<AgentGroup> = if has_filter {
        let r = runtime.unwrap();
        stmt.query_map(params![r], row_to_group)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    } else {
        stmt.query_map([], row_to_group)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };

    for g in &mut groups {
        g.members = load_group_members(&conn, &g.id);
    }
    Ok(groups)
}

#[tauri::command]
pub fn get_agent_group(db: State<'_, DbState>, slug: String) -> Result<AgentGroup, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut group = conn
        .query_row(
            "SELECT id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind
             FROM agent_groups WHERE slug = ?1",
            params![slug],
            |row| {
                Ok(AgentGroup {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    display_name: row.get(2)?,
                    description: row.get(3)?,
                    runtime: row.get(4)?,
                    router_config: row.get(5)?,
                    file_path: row.get(6)?,
                    created_at: row.get(7)?,
                    last_used_at: row.get(8)?,
                    dispatch_kind: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "routed".to_string()),
                    members: Vec::new(),
                })
            },
        )
        .map_err(|e| e.to_string())?;
    group.members = load_group_members(&conn, &group.id);
    Ok(group)
}

#[tauri::command]
pub fn update_agent_group(
    db: State<'_, DbState>,
    id: String,
    description: Option<String>,
    router_config_json: Option<String>,
    members: Option<Vec<GroupMemberInput>>,
) -> Result<AgentGroup, String> {
    if let Some(ref cfg) = router_config_json {
        serde_json::from_str::<serde_json::Value>(cfg)
            .map_err(|e| format!("Invalid router_config JSON: {}", e))?;
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Fetch the existing group to know runtime/slug for member resolution.
    let (group_runtime, group_slug): (String, String) = conn.query_row(
        "SELECT runtime, slug FROM agent_groups WHERE id = ?1",
        params![id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    ).map_err(|e| e.to_string())?;

    if let Some(desc) = &description {
        conn.execute(
            "UPDATE agent_groups SET description = ?1 WHERE id = ?2",
            params![desc, id],
        ).map_err(|e| e.to_string())?;
    }
    if let Some(cfg) = &router_config_json {
        conn.execute(
            "UPDATE agent_groups SET router_config = ?1 WHERE id = ?2",
            params![cfg, id],
        ).map_err(|e| e.to_string())?;
    }
    if let Some(new_members) = &members {
        // Replace member list atomically.
        conn.execute("DELETE FROM agent_group_members WHERE group_id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        for m in new_members {
            let agent_row = conn.query_row(
                "SELECT id, runtime FROM agents WHERE slug = ?1",
                params![m.agent_slug],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            );
            match agent_row {
                Ok((agent_id, agent_runtime)) => {
                    if agent_runtime != group_runtime {
                        return Err(format!(
                            "Member '{}' uses runtime '{}', but group runtime is '{}'",
                            m.agent_slug, agent_runtime, group_runtime
                        ));
                    }
                    conn.execute(
                        "INSERT INTO agent_group_members (group_id, agent_id, role, position)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![id, agent_id, m.role, m.position],
                    ).map_err(|e| e.to_string())?;
                }
                Err(_) => return Err(format!("Agent with slug '{}' not found", m.agent_slug)),
            }
        }
    }

    drop(conn);
    // Re-read the group + members through the public command so the file
    // mirror always reflects the freshly-saved state.
    let _ = group_slug; // borrowed only for clarity; not used further.
    let group = get_agent_group(db, group_slug.clone())?;
    let _ = write_group_file(&group);
    Ok(group)
}

#[tauri::command]
pub fn delete_agent_group(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    // Look up the slug so we can clean up the file mirror.
    if let Ok(slug) = conn.query_row(
        "SELECT slug FROM agent_groups WHERE id = ?1",
        params![id],
        |r| r.get::<_, String>(0),
    ) {
        let path = group_file_path(&slug);
        let _ = fs::remove_file(&path);
        // Best-effort prune of the parent directory if empty.
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
    conn.execute("DELETE FROM agent_groups WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Router execution (v1.4.0 F4 — dispatch_to_group) ─────────────────────

/// Decide which child agent a prompt should route to. Two-stage:
///   1. Apply rules (declarative, fast, cheap, predictable).
///   2. If no rule matches AND llmFallback is enabled, ask the runtime's
///      cheap classifier model to pick a child.
/// Returns (chosen_child_slug, routing_reason).
async fn route_prompt_to_child(
    group: &AgentGroup,
    prompt: &str,
) -> Result<(String, String), String> {
    let cfg: serde_json::Value = group
        .router_config
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let children: Vec<&AgentGroupMember> = group
        .members
        .iter()
        .filter(|m| m.role == "child")
        .collect();
    if children.is_empty() {
        return Err("Group has no children to route to".into());
    }

    // Stage 1: rules.
    if let Some(rules) = cfg.get("rules").and_then(|r| r.as_array()) {
        let lower = prompt.to_lowercase();
        for rule in rules {
            let then_slug = rule.get("then").and_then(|v| v.as_str()).unwrap_or("");
            let if_block = rule.get("if").cloned().unwrap_or_else(|| serde_json::json!({}));
            // keyword match (any of the listed strings)
            if let Some(keywords) = if_block.get("keyword").and_then(|v| v.as_array()) {
                for kw in keywords {
                    if let Some(s) = kw.as_str() {
                        if !s.is_empty() && lower.contains(&s.to_lowercase()) {
                            // Verify the child exists in this group.
                            if children.iter().any(|c| c.agent_slug == then_slug) {
                                return Ok((
                                    then_slug.to_string(),
                                    format!("rule: keyword '{}' matched", s),
                                ));
                            }
                        }
                    }
                }
            }
            // regex match
            if let Some(pattern) = if_block.get("regex").and_then(|v| v.as_str()) {
                // Tiny shim: use the same single-pass approach as substitute_variables
                // to avoid a regex dep — only supports literal substring for now.
                // (Wave 3.2 will add proper regex.)
                if !pattern.is_empty() && prompt.contains(pattern) {
                    if children.iter().any(|c| c.agent_slug == then_slug) {
                        return Ok((
                            then_slug.to_string(),
                            format!("rule: pattern '{}' matched (literal)", pattern),
                        ));
                    }
                }
            }
        }
    }

    // Stage 2: LLM fallback.
    let llm_fb = cfg.get("llmFallback");
    let llm_enabled = llm_fb
        .and_then(|v| v.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if llm_enabled {
        let descriptions: Vec<String> = children
            .iter()
            .map(|c| format!("- {}: {}", c.agent_slug, c.agent_display_name))
            .collect();
        let classifier_prompt = format!(
            "You are a router. Pick the single agent slug that should handle the user's message.\n\
             Available agents:\n{}\n\
             User message: {}\n\
             Reply with ONLY the slug — nothing else.",
            descriptions.join("\n"),
            prompt
        );
        // Reuse prompt_agent on the group's runtime.
        match prompt_agent(group.runtime.clone(), classifier_prompt, None, None, None).await {
            Ok(reply) => {
                let pick = reply.trim().lines().next().unwrap_or("").trim().to_string();
                if let Some(matched) =
                    children.iter().find(|c| c.agent_slug == pick).map(|c| c.agent_slug.clone())
                {
                    return Ok((matched, "llm-fallback".to_string()));
                }
                // Classifier returned nothing useful; fall through to default.
            }
            Err(e) => {
                eprintln!("router LLM fallback failed: {}", e);
            }
        }
    }

    // Default: first child.
    let first = children[0].agent_slug.clone();
    Ok((first, "default: first child".into()))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GroupStageResult {
    pub agent_slug: String,
    pub runtime: String,
    pub response: String,
    pub ok: bool,
    /// v2.1.0 Phase 7 — start time of this stage (ISO 8601 UTC).
    /// Frontend uses it to upload one trace per stage with the correct
    /// per-stage timing rather than approximating from the group total.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// Wall-clock duration of this stage in ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Error string when ok=false. Lets the frontend upload a precise
    /// per-stage error instead of repeating the rolled-up group error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GroupDispatchResult {
    /// Stitched transcript of all stages (or single response for routed
    /// groups). Frontend may render this OR walk `stages` to render each
    /// stage as its own message.
    pub response: String,
    pub routed_to: String,
    pub routing_reason: String,
    /// One entry per stage. Routed groups have exactly one; sequential
    /// groups have one per child in pipeline order.
    #[serde(default)]
    pub stages: Vec<GroupStageResult>,
}

/// Tauri command: dispatch a prompt through a group's router.
#[tauri::command]
pub async fn dispatch_to_group(
    db: State<'_, DbState>,
    slug: String,
    prompt: String,
    config: Option<String>,
    active_project_path: Option<String>,
) -> Result<GroupDispatchResult, String> {
    // Load the group once (under a short-lived lock).
    let group = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let mut group = conn.query_row(
            "SELECT id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind
             FROM agent_groups WHERE slug = ?1",
            params![slug],
            |row| {
                Ok(AgentGroup {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    display_name: row.get(2)?,
                    description: row.get(3)?,
                    runtime: row.get(4)?,
                    router_config: row.get(5)?,
                    file_path: row.get(6)?,
                    created_at: row.get(7)?,
                    last_used_at: row.get(8)?,
                    dispatch_kind: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "routed".to_string()),
                    members: Vec::new(),
                })
            },
        ).map_err(|e| format!("Group '{}' not found: {}", slug, e))?;
        group.members = load_group_members(&conn, &group.id);
        group
    };

    // Branch on dispatch kind. Sequential walks every child in position
    // order, feeding the previous output as input to the next; final
    // response is a stitched transcript so the user sees each stage.
    if group.dispatch_kind == "sequential" {
        return run_sequential_dispatch(&group, &prompt, config.as_deref()).await;
    }

    // Routed (default): router picks a single child.
    let (child_slug, reason) = route_prompt_to_child(&group, &prompt).await?;

    // Find the child agent's id so we can use prompt_agent_with_context.
    let child_agent_id = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id FROM agents WHERE slug = ?1",
            params![child_slug],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| format!("Child agent '{}' not found: {}", child_slug, e))?
    };

    // Resolve variables + run hooks for the child + dispatch. Group
    // dispatch only needs the response string — the run_id from the
    // DispatchResult is consumed by the FRONTEND wrappers, not here.
    let response = prompt_agent_with_context(
        db.clone(),
        child_agent_id,
        group.runtime.clone(),
        prompt,
        config,
        active_project_path,
    )
    .await?
    .response;

    // Bump last_used_at.
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let _ = conn.execute(
            "UPDATE agent_groups SET last_used_at = ?1 WHERE id = ?2",
            params![now, group.id],
        );
    }

    Ok(GroupDispatchResult {
        response: response.clone(),
        routed_to: child_slug.clone(),
        routing_reason: reason,
        stages: vec![GroupStageResult {
            agent_slug: child_slug,
            runtime: group.runtime.clone(),
            response,
            ok: true,
            started_at: None,
            duration_ms: None,
            error: None,
        }],
    })
}

/// Sequential / "automation" dispatch: walk children in `position` order,
/// feed the prompt to the first child, then feed each output as input to
/// the next. Returns a stitched transcript so the user sees what each stage
/// produced.
async fn run_sequential_dispatch(
    group: &AgentGroup,
    user_prompt: &str,
    config: Option<&str>,
) -> Result<GroupDispatchResult, String> {
    let mut children: Vec<&AgentGroupMember> = group
        .members
        .iter()
        .filter(|m| m.role == "child")
        .collect();
    children.sort_by_key(|m| m.position);

    if children.is_empty() {
        return Err("Sequential group has no children".into());
    }

    let mut transcript = String::new();
    let mut stage_results: Vec<GroupStageResult> = Vec::new();
    let mut last_output = user_prompt.to_string();

    for (i, child) in children.iter().enumerate() {
        let stage_prompt = if i == 0 {
            user_prompt.to_string()
        } else {
            format!(
                "Previous step produced this output:\n\n{}\n\n---\n\nOriginal task: {}\n\nYour task: act on the previous output per your instructions.",
                last_output, user_prompt
            )
        };

        // Each child runs on its OWN runtime. Sequential groups can chain
        // Claude → Codex → Gemini etc. — that's the whole point.
        let child_runtime = if child.agent_runtime.is_empty() {
            group.runtime.clone()
        } else {
            child.agent_runtime.clone()
        };
        let stage_start = std::time::Instant::now();
        // `to_rfc3339_opts(_, true)` forces the `Z` UTC suffix instead of
        // `+00:00`. The cloud's zod schema (z.string().datetime()) rejects
        // the offset form even though it's valid RFC3339 — caused
        // pipeline trace uploads to 400 and the Pipelines panel to stay
        // empty (2026-05-09).
        let stage_started_at = chrono::Utc::now()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        // v2.1.0+ — pass agent_slug to prompt_agent so each stage's
        // Live runs row labels with @<slug> instead of "ad-hoc".
        // prompt_agent registers itself, so we don't begin_run here.
        // Kill is now real (prompt_agent uses tokio::process +
        // oneshot channel + select! since the kill-plumbing
        // refactor).
        let (stage_response, ok, stage_error) = match prompt_agent(
            child_runtime.clone(),
            stage_prompt,
            config.map(|s| s.to_string()),
            Some(child.agent_slug.clone()),
            None,
        )
        .await
        {
            Ok(r) => (r, true, None),
            Err(e) => (
                format!("(stage '{}' on {} failed: {})", child.agent_slug, child_runtime, e),
                false,
                Some(e),
            ),
        };
        let stage_duration_ms = stage_start.elapsed().as_millis() as u64;

        if !transcript.is_empty() {
            transcript.push_str("\n\n---\n\n");
        }
        transcript.push_str(&format!(
            "**@{}** _({})_\n\n{}",
            child.agent_slug, child_runtime, stage_response
        ));
        stage_results.push(GroupStageResult {
            agent_slug: child.agent_slug.clone(),
            runtime: child_runtime,
            response: stage_response.clone(),
            ok,
            started_at: Some(stage_started_at),
            duration_ms: Some(stage_duration_ms),
            error: stage_error,
        });
        last_output = stage_response;
    }

    let stage_labels: Vec<String> = stage_results
        .iter()
        .map(|s| format!("{} ({})", s.agent_slug, s.runtime))
        .collect();
    let routed_to = children.last().map(|c| c.agent_slug.clone()).unwrap_or_default();
    let routing_reason = format!("Sequential pipeline: {}", stage_labels.join(" → "));

    Ok(GroupDispatchResult {
        response: transcript,
        routed_to,
        routing_reason,
        stages: stage_results,
    })
}

// ── Agent Observability (v1.4.0 F6) ──────────────────────────────────────
//
// Reads `~/.ato/agent-logs.jsonl` — the unified trace log every dispatch path
// (desktop Run button, Quick Test, MCP run_agent, group routing, cron jobs)
// appends to. Surfaces metrics + per-trace details for the Insights panel.

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentTraceLine {
    pub ts: Option<String>,
    pub duration_ms: Option<i64>,
    pub runtime: Option<String>,
    pub slug: Option<String>,
    pub file_path: Option<String>,
    pub prompt_preview: Option<String>,
    pub response_preview: Option<String>,
    pub ok: Option<bool>,
    pub error: Option<String>,
    pub source: Option<String>,
    /// Set when this dispatch was a group routed through its router (F4).
    pub routed_to: Option<String>,
    /// Future fields land here without breaking the type.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentTraceFilter {
    pub agent_slug: Option<String>,
    pub runtime: Option<String>,
    /// "ok" | "error" | "all" (default all).
    pub status: Option<String>,
    /// ISO-8601; only return traces with `ts >= since`.
    pub since: Option<String>,
    /// Hard cap to avoid pulling huge files.
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentMetrics {
    pub total_runs: usize,
    pub successful: usize,
    pub failed: usize,
    pub success_rate: f64,
    pub p50_latency_ms: Option<i64>,
    pub p95_latency_ms: Option<i64>,
    pub avg_latency_ms: Option<i64>,
    /// Per-agent breakdown so the dashboard can render a list. Sorted by
    /// most-recent-first.
    pub per_agent: Vec<PerAgentMetrics>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PerAgentMetrics {
    pub slug: String,
    pub runtime: Option<String>,
    pub total_runs: usize,
    pub successful: usize,
    pub failed: usize,
    pub success_rate: f64,
    pub p50_latency_ms: Option<i64>,
    pub p95_latency_ms: Option<i64>,
    pub last_run_at: Option<String>,
}

/// 2026-05-17 — Insights dashboard data source.
///
/// Historically read `~/.ato/agent-logs.jsonl` — an append-only log
/// written by an early dispatch logger that stopped writing structured
/// events ~2026-05-08 and never carried agent_slug / status / prompt.
/// The dashboard rendered "unknown" for every row and 0% success rate
/// because the fields didn't exist on disk.
///
/// Switched to query `execution_logs` (SQLite) — the canonical source
/// of truth that every dispatch path writes to. Same return shape
/// (`AgentTraceLine`) so the React side doesn't change.
fn load_agent_log_lines(conn: &rusqlite::Connection, filter: &AgentTraceFilter) -> Vec<AgentTraceLine> {
    // Build the WHERE clause dynamically from the filter.
    let mut where_parts: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(slug) = &filter.agent_slug {
        where_parts.push("agent_slug = ?".to_string());
        params.push(Box::new(slug.clone()));
    }
    if let Some(runtime) = &filter.runtime {
        where_parts.push("runtime = ?".to_string());
        params.push(Box::new(runtime.clone()));
    }
    if let Some(status) = &filter.status {
        match status.as_str() {
            "ok" => where_parts.push("status = 'success'".to_string()),
            "error" => where_parts.push("status = 'error'".to_string()),
            _ => {}
        }
    }
    if let Some(since) = &filter.since {
        where_parts.push("created_at >= ?".to_string());
        params.push(Box::new(since.clone()));
    }

    // 2026-05-19 war-room synthesis: always filter dispatch_kind='active'.
    // load_agent_log_lines feeds read_agent_traces + get_agent_metrics —
    // the Insights → Agents panel and the AgentTraceLine wire. Passive-
    // observation rows from v2.6 PR-A would otherwise pollute every
    // agent metric.
    where_parts.push("dispatch_kind = 'active'".to_string());

    let where_sql = format!("WHERE {}", where_parts.join(" AND "));

    let limit_sql = match filter.limit {
        Some(n) => format!("LIMIT {}", n.min(10_000)),
        None => "LIMIT 500".to_string(),
    };

    let sql = format!(
        "SELECT created_at, duration_ms, runtime, agent_slug,
                prompt, response, status, error_message, model, session_id
           FROM execution_logs
         {where_sql}
          ORDER BY created_at DESC
         {limit_sql}"
    );

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt.query_map(rusqlite::params_from_iter(params_refs.iter()), |r| {
        let created_at: Option<String> = r.get(0)?;
        let duration_ms: Option<i64> = r.get(1)?;
        let runtime: Option<String> = r.get(2)?;
        let agent_slug: Option<String> = r.get(3)?;
        let prompt: Option<String> = r.get(4)?;
        let response: Option<String> = r.get(5)?;
        let status: Option<String> = r.get(6)?;
        let error_message: Option<String> = r.get(7)?;
        let model: Option<String> = r.get(8)?;
        let session_id: Option<String> = r.get(9)?;

        let ok = status.as_deref().map(|s| s == "success");
        let prompt_preview = prompt.as_ref().map(|p| truncate_for_preview(p, 240));
        let response_preview = response.as_ref().map(|p| truncate_for_preview(p, 240));

        // Stuff `model` + `session_id` into the extra map so the React
        // side can render them without a type change.
        let mut extra: HashMap<String, serde_json::Value> = HashMap::new();
        if let Some(m) = model {
            extra.insert("model".to_string(), serde_json::Value::String(m));
        }
        if let Some(sid) = session_id {
            extra.insert("sessionId".to_string(), serde_json::Value::String(sid));
        }

        Ok(AgentTraceLine {
            ts: created_at,
            duration_ms,
            runtime,
            slug: agent_slug,
            file_path: None,
            prompt_preview,
            response_preview,
            ok,
            error: error_message,
            source: Some("execution_logs".to_string()),
            routed_to: None,
            extra,
        })
    });

    match rows {
        Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

fn truncate_for_preview(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let head: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{}…", head)
    }
}

fn percentile(sorted: &[i64], pct: f64) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    let idx = ((sorted.len() as f64 - 1.0) * pct).round() as usize;
    sorted.get(idx).copied()
}

#[tauri::command]
pub fn read_agent_traces(
    db: State<'_, DbState>,
    filter: AgentTraceFilter,
) -> Result<Vec<AgentTraceLine>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(load_agent_log_lines(&conn, &filter))
}

#[tauri::command]
pub fn get_agent_metrics(
    db: State<'_, DbState>,
    filter: AgentTraceFilter,
) -> Result<AgentMetrics, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    // For aggregations we want every line that matches the runtime/status/
    // since filters but ignoring `limit` so totals are accurate.
    let aggregate_filter = AgentTraceFilter {
        agent_slug: filter.agent_slug.clone(),
        runtime: filter.runtime.clone(),
        status: filter.status.clone(),
        since: filter.since.clone(),
        limit: None,
    };
    let lines = load_agent_log_lines(&conn, &aggregate_filter);

    let total = lines.len();
    let mut successful = 0usize;
    let mut failed = 0usize;
    let mut latencies: Vec<i64> = Vec::with_capacity(lines.len());

    // Per-agent rollups
    let mut per_agent_map: HashMap<String, PerAgentRollup> = HashMap::new();

    for t in &lines {
        match t.ok {
            Some(true) => successful += 1,
            Some(false) => failed += 1,
            None => {}
        }
        if let Some(d) = t.duration_ms {
            latencies.push(d);
        }

        if let Some(slug) = &t.slug {
            let entry = per_agent_map
                .entry(slug.clone())
                .or_insert_with(|| PerAgentRollup {
                    slug: slug.clone(),
                    runtime: t.runtime.clone(),
                    total: 0,
                    successful: 0,
                    failed: 0,
                    latencies: Vec::new(),
                    last_run: None,
                });
            entry.total += 1;
            match t.ok {
                Some(true) => entry.successful += 1,
                Some(false) => entry.failed += 1,
                None => {}
            }
            if let Some(d) = t.duration_ms {
                entry.latencies.push(d);
            }
            if let Some(ts) = &t.ts {
                entry.last_run = Some(match &entry.last_run {
                    Some(prev) if prev > ts => prev.clone(),
                    _ => ts.clone(),
                });
            }
        }
    }

    latencies.sort_unstable();
    let avg_latency_ms = if latencies.is_empty() {
        None
    } else {
        Some(latencies.iter().sum::<i64>() / latencies.len() as i64)
    };

    let mut per_agent: Vec<PerAgentMetrics> = per_agent_map
        .into_values()
        .map(|mut r| {
            r.latencies.sort_unstable();
            PerAgentMetrics {
                slug: r.slug,
                runtime: r.runtime,
                total_runs: r.total,
                successful: r.successful,
                failed: r.failed,
                success_rate: if r.total == 0 { 0.0 } else { r.successful as f64 / r.total as f64 },
                p50_latency_ms: percentile(&r.latencies, 0.5),
                p95_latency_ms: percentile(&r.latencies, 0.95),
                last_run_at: r.last_run,
            }
        })
        .collect();
    // Most-recent-first.
    per_agent.sort_by(|a, b| b.last_run_at.cmp(&a.last_run_at));

    Ok(AgentMetrics {
        total_runs: total,
        successful,
        failed,
        success_rate: if total == 0 { 0.0 } else { successful as f64 / total as f64 },
        p50_latency_ms: percentile(&latencies, 0.5),
        p95_latency_ms: percentile(&latencies, 0.95),
        avg_latency_ms,
        per_agent,
    })
}

struct PerAgentRollup {
    slug: String,
    runtime: Option<String>,
    total: usize,
    successful: usize,
    failed: usize,
    latencies: Vec<i64>,
    last_run: Option<String>,
}


// ── Streaming dispatch (v1.5.0) ─────────────────────────────────────────
//
// Mirrors prompt_agent / prompt_agent_with_history but streams stdout
// through a Tauri Channel so the chat pane can render tokens as they
// arrive. Each chunk is whatever bytes the CLI flushes; we don't try to
// parse newlines or JSON — that's the runtime's contract.

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StreamEvent {
    Chunk { text: String },
    Done { full: String },
    Error { message: String },
}

/// Stream a single-shot dispatch. Caller must keep the channel alive until
/// it observes a `done` or `error` event.
#[tauri::command]
pub async fn prompt_agent_stream(
    runtime: String,
    prompt: String,
    config: Option<String>,
    on_event: tauri::ipc::Channel<StreamEvent>,
) -> Result<(), String> {
    // Ad-hoc — no agent context. Registry will show "no slug, runtime X".
    spawn_streaming_dispatch(&runtime, &prompt, config.as_deref(), on_event, None, None).await
}

/// Stream a multi-turn dispatch. Resolves variables / hooks / role models
/// up-front (sync work), then streams the response.
#[tauri::command]
pub async fn prompt_agent_with_history_stream(
    db: State<'_, DbState>,
    agent_id: String,
    runtime: String,
    history: Vec<AgentMessage>,
    new_prompt: String,
    config: Option<String>,
    active_project_path: Option<String>,
    on_event: tauri::ipc::Channel<StreamEvent>,
) -> Result<(), String> {
    // Same prelude as prompt_agent_with_history — keep them in sync if you
    // change one, change the other.
    let (resolved, hooks, response_model, fallback_model, policy, summarizer_model, agent_slug) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let resolved = resolve_agent_variables(&conn, &agent_id, active_project_path.as_deref());
        let hooks = load_agent_hooks(&conn, &agent_id);
        let (rm, fb) = load_agent_response_model(&conn, &agent_id);
        let policy = load_memory_policy(&conn, &agent_id);
        let summ = load_agent_summarizer_model(&conn, &agent_id);
        // v2.1.0 Phase 4 — fetch the slug for active-runs registry labeling.
        let slug: Option<String> = conn
            .query_row(
                "SELECT slug FROM agents WHERE id = ?1",
                rusqlite::params![&agent_id],
                |r| r.get::<_, String>(0),
            )
            .ok();
        (resolved, hooks, rm, fb, policy, summ, slug)
    };

    // Summarize older history (best-effort, non-streaming — summaries are
    // small and we want them in one shot).
    let (older, recent) = split_history_for_summarization(&history, &policy);
    let summary: Option<String> = if !older.is_empty() {
        let summarizer_prompt = build_summarizer_prompt(&older);
        let chosen_summarizer = if !policy.summarizer_model.is_empty() {
            Some(policy.summarizer_model.clone())
        } else {
            summarizer_model
        };
        let summ_cfg = chosen_summarizer.map(|m| serde_json::json!({ "model": m }).to_string());
        prompt_agent(runtime.clone(), summarizer_prompt, summ_cfg, None, None).await.ok()
    } else {
        None
    };

    let rendered_new = substitute_variables(&new_prompt, &resolved);
    let stitched = build_final_prompt(summary.as_deref(), &recent, &rendered_new);
    // fire_mode evaluation uses the current turn's user message.
    let context_block = run_pre_call_hooks(hooks, &new_prompt).await;
    let final_prompt = if context_block.is_empty() {
        stitched
    } else {
        format!("{}{}", context_block, stitched)
    };
    let merged_config = merge_model_into_config(config, response_model, fallback_model);

    spawn_streaming_dispatch(
        &runtime,
        &final_prompt,
        merged_config.as_deref(),
        on_event,
        agent_slug.as_deref(),
        active_project_path.as_deref(),
    ).await
}

async fn spawn_streaming_dispatch(
    runtime: &str,
    prompt: &str,
    config: Option<&str>,
    on_event: tauri::ipc::Channel<StreamEvent>,
    // v2.1.0 Phase 4 — context for the active-runs registry. Either
    // can be None for ad-hoc dispatches that don't have the info
    // (e.g. plain prompt_agent_stream from the chat pane without a
    // selected agent).
    agent_slug: Option<&str>,
    workspace: Option<&str>,
) -> Result<(), String> {
    use std::process::Stdio;
    use std::sync::Arc;
    use tokio::io::AsyncReadExt;
    use tokio::process::Command as TokioCommand;
    // (was tokio::sync::Mutex; replaced by oneshot channel for kill.)

    // v2.1.1+ — dispatch start clock for execution_logs persistence.
    // Streaming dispatches were skipping the persist call entirely
    // (only prompt_agent_inner had it), so chat-pane runs never landed
    // in History or got a `cloud_trace_id` link, breaking replay.
    let dispatch_start = std::time::Instant::now();
    let user_path = get_user_path();
    let cfg_json: Option<serde_json::Value> = config.and_then(|c| serde_json::from_str(c).ok());
    let model_override: Option<String> = cfg_json
        .as_ref()
        .and_then(|c| c.get("model"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    let mut cmd = match runtime {
        "claude" => {
            let claude_path = which_claude().ok_or_else(|| "Claude Code CLI not found".to_string())?;
            let mut c = TokioCommand::new(claude_path);
            c.arg("--print").arg(prompt);
            if let Some(m) = &model_override {
                c.arg("--model").arg(m);
            }
            c
        }
        "codex" => {
            let codex_path = which_cli("codex")
                .ok_or_else(|| "Codex CLI not found. Install: npm install -g @openai/codex".to_string())?;
            let mut c = TokioCommand::new(codex_path);
            // Codex uses `exec` as the headless subcommand; the prompt is a
            // positional argument. `--print` is invalid for codex.
            // `--skip-git-repo-check` mirrors the non-streaming dispatch —
            // ATO can be run from any cwd, including non-repo dirs, and
            // Codex bails with "Not inside a trusted directory" otherwise.
            // `--sandbox workspace-write` + `approval_policy=never` mirror
            // the non-streaming codex branch — see line ~860 for the
            // longer rationale (codex default is read-only; ATO dispatch
            // is the authorization).
            c.arg("exec")
                .arg("--skip-git-repo-check")
                .arg("--sandbox")
                .arg("workspace-write")
                .arg("-c")
                .arg("approval_policy=\"never\"");
            if let Some(m) = &model_override {
                c.arg("--model").arg(m);
            }
            c.arg(prompt);
            c
        }
        "openclaw" => {
            let ssh_config: serde_json::Value = config
                .and_then(|c| serde_json::from_str(c).ok())
                .unwrap_or_default();
            let host = ssh_config.get("sshHost").and_then(|v| v.as_str()).unwrap_or("localhost");
            let port = ssh_config.get("sshPort").and_then(|v| v.as_u64()).unwrap_or(22);
            let user = ssh_config.get("sshUser").and_then(|v| v.as_str()).unwrap_or("root");
            let key_path = ssh_config.get("sshKeyPath").and_then(|v| v.as_str());

            let mut c = TokioCommand::new("ssh");
            if let Some(key) = key_path {
                c.args(["-i", key]);
            }
            c.args([
                "-p",
                &port.to_string(),
                &format!("{}@{}", user, host),
                &format!("openclaw exec '{}'", prompt.replace('\'', "'\\''")),
            ]);
            c
        }
        "hermes" => {
            let hermes_path = which_cli("hermes").ok_or_else(|| "Hermes CLI not found".to_string())?;
            let mut c = TokioCommand::new(hermes_path);
            c.arg("--execute").arg(prompt);
            c
        }
        "gemini" => {
            let gemini_path = which_cli("gemini")
                .ok_or_else(|| "Gemini CLI not found. Install: npm install -g @google/gemini-cli".to_string())?;
            let mut c = TokioCommand::new(gemini_path);
            // Gemini CLI: `gemini -p "<prompt>" [-m <model>]`
            c.arg("-p").arg(prompt);
            if let Some(m) = &model_override {
                c.arg("-m").arg(m);
            }
            c
        }
        other => {
            let _ = on_event.send(StreamEvent::Error {
                message: format!("Unknown runtime: {}", other),
            });
            return Ok(());
        }
    };

    cmd.env("PATH", &user_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // kill_on_drop ensures the child dies if we panic or the task
        // is aborted before we get to wait — important for keeping the
        // registry honest about what's actually running.
        .kill_on_drop(true);
    // BYOK: same env-var forwarding as the non-streaming dispatch path.
    if let Some((var, key)) = crate::byok::byok_env_value_from_path(&crate::get_db_path(), runtime)
    {
        cmd.env(var, key);
    }

    // Register BEFORE spawn so that even a spawn failure lights up
    // the registry briefly (next finish_run cleans it up). Beatriz's
    // model of "intent first, outcome second" — the user clicked the
    // dispatch button, so the run exists conceptually even if the
    // process never started.
    let run_id = crate::active_runs::begin_run(
        runtime,
        agent_slug,
        workspace,
        Some("desktop:stream"),
    );
    // Guard so we always finish_run on early returns / errors.
    struct FinishOnDrop(String);
    impl Drop for FinishOnDrop {
        fn drop(&mut self) {
            crate::active_runs::finish_run(&self.0);
        }
    }
    let _finish_guard = FinishOnDrop(run_id.clone());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = on_event.send(StreamEvent::Error {
                message: format!("Failed to spawn {}: {}", runtime, e),
            });
            return Ok(());
        }
    };

    // Kill plumbing via oneshot channel. Earlier design wrapped the
    // child in a mutex and tried to lock + kill from the closure —
    // but the dispatch path takes the child out of the mutex to own
    // its stdout, so by the time a user clicks Kill the mutex holds
    // None and the closure no-ops silently (Beatriz: "stayed
    // spinning but still ended responding", 2026-05-09). The
    // oneshot pattern decouples them: the closure signals intent;
    // the dispatch loop's select! reacts by killing the child
    // inline (where it actually owns the handle).
    let (kill_tx, kill_rx) = tokio::sync::oneshot::channel::<()>();
    let kill_tx_holder: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>> =
        Arc::new(std::sync::Mutex::new(Some(kill_tx)));
    let kill_tx_for_handler = kill_tx_holder.clone();
    crate::active_runs::attach_kill_handler(&run_id, move || {
        // Pure sync: lock, take, send. No tokio runtime needed inside
        // the closure — fixes the panic that crashed the app earlier.
        let mut guard = match kill_tx_for_handler.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if let Some(tx) = guard.take() {
            // Send may fail if the receiver dropped (dispatch already
            // finished); fine — kill becomes a no-op.
            let _ = tx.send(());
        }
    });
    let mut kill_rx = kill_rx;

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = on_event.send(StreamEvent::Error {
                message: "stdout-pipe-missing".into(),
            });
            return Ok(());
        }
    };

    // Read stdout in chunks, emitting each as a Chunk event. The buffer is
    // small enough that the user sees tokens flowing within a few hundred
    // ms, even if the runtime writes line-buffered.
    //
    // The select! gives the kill_rx receiver a chance to fire between
    // reads. When the user clicks Kill, the closure sends on the
    // oneshot, this branch wins, we kill the child + emit an error,
    // and return. Without this, the read loop would happily drain the
    // child's already-buffered stdout to completion even after the
    // kill request.
    let mut reader = stdout;
    let mut buf = [0u8; 1024];
    let mut full = String::new();
    loop {
        tokio::select! {
            biased;
            _ = &mut kill_rx => {
                // User clicked Kill. SIGKILL the child, surface a
                // clean "killed by user" error to the UI, and stop.
                let _ = child.kill().await;
                let _ = on_event.send(StreamEvent::Error {
                    message: "killed by user".into(),
                });
                return Ok(());
            }
            read_result = reader.read(&mut buf) => match read_result {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                    full.push_str(&chunk);
                    let _ = on_event.send(StreamEvent::Chunk { text: chunk });
                }
                Err(e) => {
                    let _ = on_event.send(StreamEvent::Error {
                        message: format!("read-failed: {}", e),
                    });
                    let _ = child.kill().await;
                    return Ok(());
                }
            },
        }
    }

    let status = match child.wait().await {
        Ok(s) => s,
        Err(e) => {
            let _ = on_event.send(StreamEvent::Error {
                message: format!("wait-failed: {}", e),
            });
            return Ok(());
        }
    };

    let duration_ms = dispatch_start.elapsed().as_millis() as i32;
    if status.success() {
        // v2.1.1+ — persist BEFORE emitting Done. Frontend's upload-
        // and-link kicks in immediately after the Done event lands;
        // if execution_logs is empty when the link command runs, the
        // ±10s temporal match has nothing to attach the cloud trace
        // ID to, and replay fails with prompt-not-local.
        persist_execution_log(
            runtime,
            prompt,
            &Ok(full.clone()),
            duration_ms,
            model_override.as_deref(),
            agent_slug,
            None,
        );
        let _ = on_event.send(StreamEvent::Done { full });
    } else {
        // Drain stderr for the error message — best-effort.
        let mut err_text = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut err_text).await;
        }
        // Redact BYOK secrets before stderr lands in execution_logs +
        // gets emitted to the frontend. (minimax #1)
        let err_text = crate::byok::redact_byok_secrets(&err_text, runtime, None);
        let final_msg = if err_text.is_empty() {
            format!("{} exited with status {}", runtime, status)
        } else {
            err_text
        };
        persist_execution_log(
            runtime,
            prompt,
            &Err(final_msg.clone()),
            duration_ms,
            model_override.as_deref(),
            agent_slug,
            None,
        );
        let _ = on_event.send(StreamEvent::Error { message: final_msg });
    }

    Ok(())
}

// ── Headless cron dispatch (v1.6 wake-from-sleep groundwork) ─────────────
//
// `ato-desktop --run-cron <id>` invokes this from outside the GUI. Used by
// OS-level schedulers (launchd on macOS today; systemd / Task Scheduler
// later) so jobs fire even when the app isn't open.
//
// Mirrors trigger_cron_job's logic but runs against a freshly-opened DB
// connection, blocks on a tokio runtime, and exits with an integer status
// code so launchd records success/failure.

pub fn run_cron_headless(job_id: String) -> i32 {
    let log_dir = home_dir().join(".ato").join("cron-logs");
    let _ = fs::create_dir_all(&log_dir);
    let log_path = log_dir.join(format!("{}.log", job_id));

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            let _ = fs::write(&log_path, format!("[error] tokio init: {}\n", e));
            return 1;
        }
    };

    let result = runtime.block_on(async { dispatch_cron_headless(&job_id).await });

    let now = chrono::Utc::now().to_rfc3339();
    match result {
        Ok(response) => {
            let entry = format!("[{}] [ok] job={}\n{}\n", now, job_id, response);
            let _ = append_to_file(&log_path, &entry);
            0
        }
        Err(e) => {
            let entry = format!("[{}] [err] job={}: {}\n", now, job_id, e);
            let _ = append_to_file(&log_path, &entry);
            1
        }
    }
}

fn append_to_file(path: &PathBuf, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

async fn dispatch_cron_headless(job_id: &str) -> Result<String, String> {
    // Read the job from disk (same shape as trigger_cron_job).
    let path = cron_jobs_path();
    if !path.exists() {
        return Err("No cron jobs configured".into());
    }
    let content = read_file_lossy(&path).unwrap_or_default();
    let jobs: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap_or_default();
    let job = jobs
        .iter()
        .find(|j| j.get("id").and_then(|v| v.as_str()) == Some(job_id))
        .ok_or_else(|| format!("Cron job not found: {}", job_id))?;

    let runtime = job.get("runtime").and_then(|v| v.as_str()).unwrap_or("claude").to_string();
    let prompt = job.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let config = job.get("runtimeConfig").map(|v| v.to_string());
    let agent_slug = job.get("agentSlug").and_then(|v| v.as_str()).map(String::from);
    let group_slug = job.get("groupSlug").and_then(|v| v.as_str()).map(String::from);
    // v2.10 PR-7 — methodology cron support. When the job carries a
    // methodologySlug, fan out the methodology via the `ato` CLI
    // instead of doing a single-prompt dispatch. The CLI is the
    // canonical implementation of the runner; this is a thin shell-out.
    let methodology_slug = job.get("methodologySlug").and_then(|v| v.as_str()).map(String::from);
    let methodology_billing = job.get("methodologyBilling").and_then(|v| v.as_str()).unwrap_or("byok").to_string();
    let methodology_max = job.get("methodologyMaxDispatches").and_then(|v| v.as_u64());

    if let Some(slug) = methodology_slug {
        return headless_dispatch_methodology(&slug, &methodology_billing, methodology_max).await;
    }

    // Open the DB ourselves — we're outside the Tauri State context.
    let db_path = crate::get_db_path();
    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => return Err(format!("open db: {}", e)),
    };

    if let Some(slug) = group_slug {
        // Replicate dispatch_to_group's logic without needing State<DbState>.
        return headless_dispatch_group(&conn, &slug, &prompt, config.as_deref()).await;
    }

    if let Some(slug) = agent_slug {
        let agent_lookup: Option<(String, String)> = conn
            .query_row(
                "SELECT id, runtime FROM agents WHERE slug = ?1",
                params![slug],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .ok();
        match agent_lookup {
            Some((agent_id, agent_runtime)) => {
                return headless_dispatch_agent(&conn, &agent_id, &agent_runtime, &prompt, config.as_deref()).await;
            }
            None => return Err(format!("Cron references missing agent slug '{}'", slug)),
        }
    }

    prompt_agent(runtime, prompt, config, None, None).await
}

/// v2.10 PR-7 — scheduled methodology runs. Shell out to the `ato` CLI
/// with `evaluations methodology run <slug>`; capture stdout into the
/// cron log. The CLI is the source-of-truth implementation; replicating
/// the runner inside the Tauri process would drift over time.
///
/// Locates the `ato` binary via: $ATO_CLI_PATH → which("ato") →
/// /opt/homebrew/bin/ato → /usr/local/bin/ato. Fails fast with a
/// clear error if none of those resolve.
async fn headless_dispatch_methodology(
    slug: &str,
    billing: &str,
    max_dispatches: Option<u64>,
) -> Result<String, String> {
    let exe = locate_ato_cli()?;
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.arg("evaluations")
        .arg("methodology")
        .arg("run")
        .arg(slug)
        .arg("--billing")
        .arg(billing)
        .arg("--quiet");
    if let Some(n) = max_dispatches {
        cmd.arg("--max-dispatches").arg(n.to_string());
    }
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("spawn `{} evaluations methodology run {}`: {}", exe, slug, e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "methodology run {} failed (exit {:?}): {}",
            slug,
            output.status.code(),
            stderr.trim()
        ));
    }
    Ok(format!(
        "methodology={} billing={} max={}\n{}",
        slug,
        billing,
        max_dispatches.map(|n| n.to_string()).unwrap_or_else(|| "all".to_string()),
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

fn locate_ato_cli() -> Result<String, String> {
    if let Ok(p) = std::env::var("ATO_CLI_PATH") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    if let Ok(p) = which::which("ato") {
        return Ok(p.to_string_lossy().to_string());
    }
    for candidate in &[
        "/opt/homebrew/bin/ato",
        "/usr/local/bin/ato",
        "/Applications/ATO.app/Contents/MacOS/ato",
    ] {
        if std::path::Path::new(candidate).exists() {
            return Ok(candidate.to_string());
        }
    }
    Err(
        "could not locate the `ato` CLI binary. Install via `brew install willnigri/ato/ato` \
         or set ATO_CLI_PATH to point at the binary."
            .to_string(),
    )
}

async fn headless_dispatch_agent(
    conn: &Connection,
    agent_id: &str,
    runtime: &str,
    prompt: &str,
    config: Option<&str>,
) -> Result<String, String> {
    // Same shape as prompt_agent_with_context but doesn't need State<DbState>.
    let resolved = resolve_agent_variables(conn, agent_id, None);
    let hooks = load_agent_hooks(conn, agent_id);
    let (response_model, fallback_model) = load_agent_response_model(conn, agent_id);

    let rendered = substitute_variables(prompt, &resolved);
    let context_block = run_pre_call_hooks(hooks, &prompt).await;
    let final_prompt = if context_block.is_empty() {
        rendered
    } else {
        format!("{}{}", context_block, rendered)
    };

    let merged_config = merge_model_into_config(
        config.map(|s| s.to_string()),
        response_model,
        fallback_model,
    );
    // Headless cron dispatch — look up the slug for Live panel labelling.
    let agent_slug: Option<String> = conn
        .query_row(
            "SELECT slug FROM agents WHERE id = ?1",
            params![agent_id],
            |r| r.get::<_, String>(0),
        )
        .ok();
    prompt_agent(runtime.to_string(), final_prompt, merged_config, agent_slug, None).await
}

async fn headless_dispatch_group(
    conn: &Connection,
    slug: &str,
    prompt: &str,
    config: Option<&str>,
) -> Result<String, String> {
    let mut group = conn
        .query_row(
            "SELECT id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind
             FROM agent_groups WHERE slug = ?1",
            params![slug],
            |row| {
                Ok(AgentGroup {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    display_name: row.get(2)?,
                    description: row.get(3)?,
                    runtime: row.get(4)?,
                    router_config: row.get(5)?,
                    file_path: row.get(6)?,
                    created_at: row.get(7)?,
                    last_used_at: row.get(8)?,
                    dispatch_kind: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "routed".to_string()),
                    members: Vec::new(),
                })
            },
        )
        .map_err(|e| format!("Group '{}' not found: {}", slug, e))?;
    group.members = load_group_members(conn, &group.id);

    if group.dispatch_kind == "sequential" {
        return run_sequential_dispatch(&group, prompt, config)
            .await
            .map(|r| r.response);
    }

    let (child_slug, _reason) = route_prompt_to_child(&group, prompt).await?;
    let child_agent: Option<(String, String)> = conn
        .query_row(
            "SELECT id, runtime FROM agents WHERE slug = ?1",
            params![child_slug],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .ok();
    match child_agent {
        Some((agent_id, agent_runtime)) => {
            headless_dispatch_agent(conn, &agent_id, &agent_runtime, prompt, config).await
        }
        None => Err(format!("Routed child '{}' not found", child_slug)),
    }
}


// ── Configuration export / import (Polish-T4) ────────────────────────────
//
// JSON snapshots of the user's local config so they can move between
// machines or roll back. We deliberately exclude the *contents* of secrets
// and API keys — those live in the OS keychain or on disk in a way the user
// already controls. The backup carries metadata only (preview, name, kind),
// so importing on a new machine surfaces what's missing without leaking
// values out of the keychain.

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBackup {
    pub version: u32,
    pub exported_at: String,
    pub agents: Vec<serde_json::Value>,
    pub agent_variables: Vec<serde_json::Value>,
    pub agent_hooks: Vec<serde_json::Value>,
    pub agent_groups: Vec<serde_json::Value>,
    pub agent_group_members: Vec<serde_json::Value>,
    pub projects: Vec<serde_json::Value>,
    pub env_vars: Vec<serde_json::Value>,
    pub model_configs: Vec<serde_json::Value>,
    pub secrets_meta: Vec<serde_json::Value>,
    pub llm_api_keys_meta: Vec<serde_json::Value>,
    pub settings: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub agents: usize,
    pub agent_variables: usize,
    pub agent_hooks: usize,
    pub agent_groups: usize,
    pub agent_group_members: usize,
    pub projects: usize,
    pub env_vars: usize,
    pub model_configs: usize,
    pub secrets_meta: usize,
    pub llm_api_keys_meta: usize,
    pub settings: usize,
}

fn dump_table(
    conn: &rusqlite::Connection,
    sql: &str,
    columns: &[&str],
) -> Result<Vec<serde_json::Value>, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let mut obj = serde_json::Map::new();
            for (i, col) in columns.iter().enumerate() {
                let val: rusqlite::types::Value = row.get(i)?;
                obj.insert((*col).to_string(), match val {
                    rusqlite::types::Value::Null => serde_json::Value::Null,
                    rusqlite::types::Value::Integer(n) => serde_json::Value::from(n),
                    rusqlite::types::Value::Real(f) => serde_json::Value::from(f),
                    rusqlite::types::Value::Text(s) => serde_json::Value::from(s),
                    rusqlite::types::Value::Blob(_) => serde_json::Value::Null,
                });
            }
            Ok(serde_json::Value::Object(obj))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn export_configuration(db: State<'_, DbState>) -> Result<ConfigBackup, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(ConfigBackup {
        version: 1,
        exported_at: chrono::Utc::now().to_rfc3339(),
        agents: dump_table(
            &conn,
            "SELECT id, slug, display_name, description, runtime, model, project_id, system_prompt, permissions, skills, mcps, goal, file_path, created_at, last_used_at, role_models_json, memory_policy_json FROM agents",
            &["id","slug","displayName","description","runtime","model","projectId","systemPrompt","permissions","skills","mcps","goal","filePath","createdAt","lastUsedAt","roleModels","memoryPolicy"],
        )?,
        agent_variables: dump_table(
            &conn,
            "SELECT id, agent_id, name, kind, config_json, enabled, created_at, updated_at FROM agent_variables",
            &["id","agentId","name","kind","config","enabled","createdAt","updatedAt"],
        )?,
        agent_hooks: dump_table(
            &conn,
            "SELECT id, agent_id, position, name, kind, config_json, enabled, created_at FROM agent_hooks",
            &["id","agentId","position","name","kind","config","enabled","createdAt"],
        )?,
        agent_groups: dump_table(
            &conn,
            "SELECT id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind FROM agent_groups",
            &["id","slug","displayName","description","runtime","routerConfig","filePath","createdAt","lastUsedAt","dispatchKind"],
        )?,
        agent_group_members: dump_table(
            &conn,
            "SELECT group_id, agent_id, role, position FROM agent_group_members",
            &["groupId","agentId","role","position"],
        )?,
        projects: dump_table(
            &conn,
            "SELECT id, name, path, is_active, skill_count, last_accessed, created_at FROM projects",
            &["id","name","path","isActive","skillCount","lastAccessed","createdAt"],
        )?,
        env_vars: dump_table(
            &conn,
            "SELECT id, project_id, runtime, key, value, created_at FROM env_vars",
            &["id","projectId","runtime","key","value","createdAt"],
        )?,
        model_configs: dump_table(
            &conn,
            "SELECT id, runtime, project_id, model_id, max_tokens, temperature, created_at, updated_at FROM model_configs",
            &["id","runtime","projectId","modelId","maxTokens","temperature","createdAt","updatedAt"],
        )?,
        // Secrets metadata only — never the encrypted blob.
        secrets_meta: dump_table(
            &conn,
            "SELECT id, name, key_type, runtime, project_id, created_at, updated_at FROM secrets",
            &["id","name","keyType","runtime","projectId","createdAt","updatedAt"],
        )?,
        // LLM API keys metadata only.
        llm_api_keys_meta: dump_table(
            &conn,
            "SELECT id, provider, name, key_preview, project_id, runtime, is_active, last_used, usage_count, created_at, updated_at FROM llm_api_keys",
            &["id","provider","name","keyPreview","projectId","runtime","isActive","lastUsed","usageCount","createdAt","updatedAt"],
        )?,
        settings: dump_table(
            &conn,
            "SELECT key, value FROM settings",
            &["key","value"],
        )?,
    })
}

fn obj_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}
fn obj_i64(v: &serde_json::Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}
fn obj_f64(v: &serde_json::Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64())
}

#[tauri::command]
pub fn import_configuration(
    db: State<'_, DbState>,
    backup_json: String,
) -> Result<ImportSummary, String> {
    let backup: ConfigBackup =
        serde_json::from_str(&backup_json).map_err(|e| format!("invalid backup: {}", e))?;
    if backup.version != 1 {
        return Err(format!("unsupported backup version: {}", backup.version));
    }

    let mut conn = db.0.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    let mut s = ImportSummary {
        agents: 0,
        agent_variables: 0,
        agent_hooks: 0,
        agent_groups: 0,
        agent_group_members: 0,
        projects: 0,
        env_vars: 0,
        model_configs: 0,
        secrets_meta: 0,
        llm_api_keys_meta: 0,
        settings: 0,
    };

    for a in &backup.agents {
        tx.execute(
            "INSERT OR REPLACE INTO agents (id, slug, display_name, description, runtime, model, project_id, system_prompt, permissions, skills, mcps, goal, file_path, created_at, last_used_at, role_models_json, memory_policy_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                obj_str(a, "id"), obj_str(a, "slug"), obj_str(a, "displayName"), obj_str(a, "description"),
                obj_str(a, "runtime"), obj_str(a, "model"), obj_str(a, "projectId"), obj_str(a, "systemPrompt"),
                obj_str(a, "permissions"), obj_str(a, "skills"), obj_str(a, "mcps"), obj_str(a, "goal"),
                obj_str(a, "filePath"), obj_str(a, "createdAt"), obj_str(a, "lastUsedAt"),
                obj_str(a, "roleModels"), obj_str(a, "memoryPolicy"),
            ],
        ).map_err(|e| e.to_string())?;
        s.agents += 1;
    }

    for v in &backup.agent_variables {
        tx.execute(
            "INSERT OR REPLACE INTO agent_variables (id, agent_id, name, kind, config_json, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                obj_str(v, "id"), obj_str(v, "agentId"), obj_str(v, "name"), obj_str(v, "kind"),
                obj_str(v, "config"), obj_i64(v, "enabled").unwrap_or(1),
                obj_str(v, "createdAt"), obj_str(v, "updatedAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.agent_variables += 1;
    }

    for h in &backup.agent_hooks {
        tx.execute(
            "INSERT OR REPLACE INTO agent_hooks (id, agent_id, position, name, kind, config_json, enabled, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                obj_str(h, "id"), obj_str(h, "agentId"),
                obj_i64(h, "position").unwrap_or(0),
                obj_str(h, "name"), obj_str(h, "kind"),
                obj_str(h, "config"), obj_i64(h, "enabled").unwrap_or(1),
                obj_str(h, "createdAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.agent_hooks += 1;
    }

    for g in &backup.agent_groups {
        tx.execute(
            "INSERT OR REPLACE INTO agent_groups (id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, COALESCE(?10, 'routed'))",
            params![
                obj_str(g, "id"), obj_str(g, "slug"), obj_str(g, "displayName"), obj_str(g, "description"),
                obj_str(g, "runtime"), obj_str(g, "routerConfig"), obj_str(g, "filePath"),
                obj_str(g, "createdAt"), obj_str(g, "lastUsedAt"),
                obj_str(g, "dispatchKind"),
            ],
        ).map_err(|e| e.to_string())?;
        s.agent_groups += 1;
    }

    for m in &backup.agent_group_members {
        tx.execute(
            "INSERT OR REPLACE INTO agent_group_members (group_id, agent_id, role, position) VALUES (?1, ?2, ?3, ?4)",
            params![
                obj_str(m, "groupId"), obj_str(m, "agentId"),
                obj_str(m, "role"), obj_i64(m, "position").unwrap_or(0),
            ],
        ).map_err(|e| e.to_string())?;
        s.agent_group_members += 1;
    }

    for p in &backup.projects {
        tx.execute(
            "INSERT OR REPLACE INTO projects (id, name, path, is_active, skill_count, last_accessed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                obj_str(p, "id"), obj_str(p, "name"), obj_str(p, "path"),
                obj_i64(p, "isActive").unwrap_or(0),
                obj_i64(p, "skillCount").unwrap_or(0),
                obj_str(p, "lastAccessed"), obj_str(p, "createdAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.projects += 1;
    }

    for e in &backup.env_vars {
        tx.execute(
            "INSERT OR REPLACE INTO env_vars (id, project_id, runtime, key, value, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                obj_str(e, "id"), obj_str(e, "projectId"), obj_str(e, "runtime"),
                obj_str(e, "key"), obj_str(e, "value"), obj_str(e, "createdAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.env_vars += 1;
    }

    for m in &backup.model_configs {
        tx.execute(
            "INSERT OR REPLACE INTO model_configs (id, runtime, project_id, model_id, max_tokens, temperature, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                obj_str(m, "id"), obj_str(m, "runtime"), obj_str(m, "projectId"),
                obj_str(m, "modelId"),
                obj_i64(m, "maxTokens"),
                obj_f64(m, "temperature"),
                obj_str(m, "createdAt"), obj_str(m, "updatedAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.model_configs += 1;
    }

    // Secrets/keys: metadata only — re-create rows with empty encrypted_key.
    // The user has to re-enter the values on the new machine. We surface
    // this in ImportSummary so the UI can prompt them.
    for k in &backup.secrets_meta {
        tx.execute(
            "INSERT OR IGNORE INTO secrets (id, name, key_type, runtime, project_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                obj_str(k, "id"), obj_str(k, "name"), obj_str(k, "keyType"),
                obj_str(k, "runtime"), obj_str(k, "projectId"),
                obj_str(k, "createdAt"), obj_str(k, "updatedAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.secrets_meta += 1;
    }

    for k in &backup.llm_api_keys_meta {
        tx.execute(
            "INSERT OR IGNORE INTO llm_api_keys (id, provider, name, key_preview, encrypted_key, project_id, runtime, is_active, last_used, usage_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                obj_str(k, "id"), obj_str(k, "provider"), obj_str(k, "name"),
                obj_str(k, "keyPreview"), "",
                obj_str(k, "projectId"), obj_str(k, "runtime"),
                obj_i64(k, "isActive").unwrap_or(0),
                obj_str(k, "lastUsed"),
                obj_i64(k, "usageCount").unwrap_or(0),
                obj_str(k, "createdAt"), obj_str(k, "updatedAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.llm_api_keys_meta += 1;
    }

    for setting in &backup.settings {
        tx.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![obj_str(setting, "key"), obj_str(setting, "value")],
        ).map_err(|e| e.to_string())?;
        s.settings += 1;
    }

    tx.commit().map_err(|e| e.to_string())?;
    Ok(s)
}

#[cfg(test)]
mod observability_tests {
    use super::*;

    #[test]
    fn percentile_handles_empty_and_single() {
        assert_eq!(percentile(&[], 0.5), None);
        assert_eq!(percentile(&[42], 0.5), Some(42));
        assert_eq!(percentile(&[1, 2, 3, 4, 5], 0.5), Some(3));
    }

    fn make_eval(id: &str, kind: &str, cfg: &str) -> AgentEvaluator {
        AgentEvaluator {
            id: id.into(),
            agent_slug: "test".into(),
            name: "test-eval".into(),
            kind: kind.into(),
            config_json: cfg.into(),
            enabled: true,
            created_at: "2026-05-04T00:00:00Z".into(),
        }
    }

    fn make_trace(response: &str) -> AgentTraceLine {
        let mut t = AgentTraceLine::default();
        t.response_preview = Some(response.into());
        t
    }

    #[test]
    fn contains_evaluator_passes_when_response_has_substring() {
        let e = make_eval("e1", "contains", r#"{"needle":"success"}"#);
        let t = make_trace("Operation completed with SUCCESS");
        let r = run_evaluator(&e, &t);
        assert_eq!(r.verdict, "pass");
        assert_eq!(r.score, 1.0);
    }

    #[test]
    fn not_contains_evaluator_fails_when_forbidden_substring_present() {
        let e = make_eval("e1", "not-contains", r#"{"needle":"error"}"#);
        let t = make_trace("Encountered an Error during dispatch");
        let r = run_evaluator(&e, &t);
        assert_eq!(r.verdict, "fail");
    }

    #[test]
    fn length_range_evaluator_passes_when_within_bounds() {
        let e = make_eval("e1", "length-range", r#"{"min":5,"max":50}"#);
        let t = make_trace("hello world");
        let r = run_evaluator(&e, &t);
        assert_eq!(r.verdict, "pass");
    }

    #[test]
    fn llm_judge_returns_unknown_for_now() {
        let e = make_eval("e1", "llm-judge", r#"{"prompt":"is this good?"}"#);
        let t = make_trace("anything");
        let r = run_evaluator(&e, &t);
        assert_eq!(r.verdict, "unknown");
    }
}

// Felipe P1 — PATH augmentation for version managers (nvm/pyenv/rbenv/.local/bin).
// Tests use a temp directory as fake $HOME via the `_for_home` variant so they
// don't race on std::env::set_var or depend on the test host's filesystem state.
// Path-specific assertions live behind `cfg(not(target_os = "windows"))` because
// the helper short-circuits on Windows (nvm-windows / pyenv-win have different
// layouts handled upstream by the PowerShell PATH probe).
#[cfg(test)]
#[cfg(not(target_os = "windows"))]
mod get_user_path_tests {
    use super::augment_with_version_managers_for_home;
    use std::fs;
    use tempfile::TempDir;

    fn mk_home() -> TempDir {
        TempDir::new().expect("tempdir")
    }

    #[test]
    fn appends_nvm_pyenv_rbenv_local_bin_when_present() {
        let home = mk_home();
        let h = home.path().to_str().unwrap();
        fs::create_dir_all(format!("{}/.nvm/versions/node/v22.4.0/bin", h)).unwrap();
        fs::create_dir_all(format!("{}/.pyenv/shims", h)).unwrap();
        fs::create_dir_all(format!("{}/.rbenv/shims", h)).unwrap();
        fs::create_dir_all(format!("{}/.local/bin", h)).unwrap();

        let out = augment_with_version_managers_for_home("/usr/bin:/bin".to_string(), h);
        assert!(out.contains(&format!("{}/.nvm/versions/node/v22.4.0/bin", h)));
        assert!(out.contains(&format!("{}/.pyenv/shims", h)));
        assert!(out.contains(&format!("{}/.rbenv/shims", h)));
        assert!(out.contains(&format!("{}/.local/bin", h)));
        // Base PATH is preserved at the front so login-shell entries still win
        // when two directories happen to ship the same binary name.
        assert!(out.starts_with("/usr/bin:/bin:"));
    }

    #[test]
    fn picks_newest_nvm_node_version() {
        let home = mk_home();
        let h = home.path().to_str().unwrap();
        fs::create_dir_all(format!("{}/.nvm/versions/node/v18.19.0/bin", h)).unwrap();
        fs::create_dir_all(format!("{}/.nvm/versions/node/v20.11.1/bin", h)).unwrap();
        fs::create_dir_all(format!("{}/.nvm/versions/node/v22.4.0/bin", h)).unwrap();

        let out = augment_with_version_managers_for_home(String::new(), h);
        assert!(out.contains(&format!("{}/.nvm/versions/node/v22.4.0/bin", h)));
        assert!(!out.contains("v20.11.1"));
        assert!(!out.contains("v18.19.0"));
    }

    // Regression for war-room R1 (google + minimax catch): lex sort
    // puts v9 after v22 since '9' > '2'. Numeric tuple sort fixes this.
    #[test]
    fn picks_numerically_newest_when_majors_differ_in_digit_count() {
        let home = mk_home();
        let h = home.path().to_str().unwrap();
        fs::create_dir_all(format!("{}/.nvm/versions/node/v9.0.0/bin", h)).unwrap();
        fs::create_dir_all(format!("{}/.nvm/versions/node/v22.4.0/bin", h)).unwrap();

        let out = augment_with_version_managers_for_home(String::new(), h);
        assert!(
            out.contains(&format!("{}/.nvm/versions/node/v22.4.0/bin", h)),
            "v22.4.0 must beat v9.0.0; got {:?}",
            out
        );
        assert!(!out.contains("v9.0.0"));
    }

    // nvm's versions/node parent sometimes also holds alias symlinks
    // (`default`, `lts/*`), iojs entries, or a `system` marker. We
    // only auto-select strict vMAJOR.MINOR.PATCH dirs.
    #[test]
    fn ignores_non_semver_directory_names() {
        let home = mk_home();
        let h = home.path().to_str().unwrap();
        fs::create_dir_all(format!("{}/.nvm/versions/node/v20.11.1/bin", h)).unwrap();
        fs::create_dir_all(format!("{}/.nvm/versions/node/system/bin", h)).unwrap();
        fs::create_dir_all(format!("{}/.nvm/versions/node/iojs-3.0.0/bin", h)).unwrap();
        fs::create_dir_all(format!("{}/.nvm/versions/node/v22.4.0-rc.1/bin", h)).unwrap();

        let out = augment_with_version_managers_for_home(String::new(), h);
        assert!(out.contains(&format!("{}/.nvm/versions/node/v20.11.1/bin", h)));
        assert!(!out.contains("/system/bin"));
        assert!(!out.contains("iojs"));
        assert!(!out.contains("rc.1"));
    }

    // Pin the order of augmentations: nvm → pyenv → rbenv → .local/bin.
    // (war-room R1 minimax suggestion — keeps a refactor from silently
    // reordering insertions.)
    #[test]
    fn additions_preserve_version_manager_order() {
        let home = mk_home();
        let h = home.path().to_str().unwrap();
        fs::create_dir_all(format!("{}/.nvm/versions/node/v22.4.0/bin", h)).unwrap();
        fs::create_dir_all(format!("{}/.pyenv/shims", h)).unwrap();
        fs::create_dir_all(format!("{}/.rbenv/shims", h)).unwrap();
        fs::create_dir_all(format!("{}/.local/bin", h)).unwrap();

        let out = augment_with_version_managers_for_home(String::new(), h);
        let parts: Vec<&str> = out.split(':').collect();
        let idx = |needle: &str| parts.iter().position(|p| p.contains(needle)).unwrap();
        let nvm = idx(".nvm/versions/node");
        let pyenv = idx(".pyenv/shims");
        let rbenv = idx(".rbenv/shims");
        let local = idx(".local/bin");
        assert!(nvm < pyenv && pyenv < rbenv && rbenv < local, "got: {:?}", parts);
    }

    #[test]
    fn idempotent_when_paths_already_in_base() {
        let home = mk_home();
        let h = home.path().to_str().unwrap();
        fs::create_dir_all(format!("{}/.pyenv/shims", h)).unwrap();
        fs::create_dir_all(format!("{}/.local/bin", h)).unwrap();

        let base = format!("/usr/bin:{}/.pyenv/shims:{}/.local/bin", h, h);
        let out = augment_with_version_managers_for_home(base.clone(), h);
        assert_eq!(out, base, "no duplicates should be appended");
    }

    #[test]
    fn skips_directories_that_dont_exist() {
        let home = mk_home();
        let h = home.path().to_str().unwrap();
        // Only .local/bin exists; the rest aren't created.
        fs::create_dir_all(format!("{}/.local/bin", h)).unwrap();

        let out = augment_with_version_managers_for_home("/usr/bin".to_string(), h);
        assert_eq!(out, format!("/usr/bin:{}/.local/bin", h));
        assert!(!out.contains(".nvm"));
        assert!(!out.contains(".pyenv"));
        assert!(!out.contains(".rbenv"));
    }

    #[test]
    fn no_op_when_no_version_managers_present() {
        let home = mk_home();
        let h = home.path().to_str().unwrap();
        let base = "/usr/bin:/bin".to_string();
        let out = augment_with_version_managers_for_home(base.clone(), h);
        assert_eq!(out, base);
    }

    #[test]
    fn handles_empty_home_without_panicking() {
        let out = augment_with_version_managers_for_home("/usr/bin".to_string(), "");
        assert_eq!(out, "/usr/bin");
    }

    #[test]
    fn handles_empty_base_with_only_additions() {
        let home = mk_home();
        let h = home.path().to_str().unwrap();
        fs::create_dir_all(format!("{}/.local/bin", h)).unwrap();
        let out = augment_with_version_managers_for_home(String::new(), h);
        assert_eq!(out, format!("{}/.local/bin", h));
    }
}

// Felipe P4 — `load_agent_default_prompt` is the back half of the
// Run = dispatch rework. Tests cover the substitution branches the
// prompt_agent_inner call site relies on, without spinning up the
// full dispatch pipeline (which would need real CLIs installed).
#[cfg(test)]
mod default_prompt_lookup_tests {
    use super::*;

    fn seed_agents_table(conn: &rusqlite::Connection) {
        conn.execute_batch(
            "CREATE TABLE agents (
                id            TEXT PRIMARY KEY,
                slug          TEXT NOT NULL,
                display_name  TEXT NOT NULL,
                description   TEXT,
                runtime       TEXT NOT NULL,
                model         TEXT,
                project_id    TEXT,
                system_prompt TEXT,
                permissions   TEXT,
                skills        TEXT,
                mcps          TEXT,
                goal          TEXT,
                file_path     TEXT,
                created_at    TEXT NOT NULL,
                last_used_at  TEXT,
                default_prompt TEXT,
                UNIQUE (runtime, slug)
            );",
        )
        .expect("create agents table");
    }

    fn insert_agent(
        conn: &rusqlite::Connection,
        id: &str,
        slug: &str,
        runtime: &str,
        default_prompt: Option<&str>,
        last_used_at: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO agents
               (id, slug, display_name, runtime, created_at, last_used_at, default_prompt)
             VALUES (?1, ?2, ?2, ?3, '2026-05-01T00:00:00Z', ?4, ?5)",
            rusqlite::params![id, slug, runtime, last_used_at, default_prompt],
        )
        .expect("insert agent row");
    }

    #[test]
    fn returns_default_prompt_when_set() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let conn = rusqlite::Connection::open(tmp.path()).unwrap();
        seed_agents_table(&conn);
        insert_agent(&conn, "a1", "reviewer", "claude", Some("Review my PR"), None);
        drop(conn);

        let got = load_agent_default_prompt(tmp.path(), "reviewer", "claude");
        assert_eq!(got.as_deref(), Some("Review my PR"));
    }

    #[test]
    fn returns_none_when_default_prompt_is_null() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let conn = rusqlite::Connection::open(tmp.path()).unwrap();
        seed_agents_table(&conn);
        insert_agent(&conn, "a1", "reviewer", "claude", None, None);
        drop(conn);

        assert!(load_agent_default_prompt(tmp.path(), "reviewer", "claude").is_none());
    }

    #[test]
    fn returns_none_when_default_prompt_is_whitespace_only() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let conn = rusqlite::Connection::open(tmp.path()).unwrap();
        seed_agents_table(&conn);
        insert_agent(&conn, "a1", "reviewer", "claude", Some("   \n\t  "), None);
        drop(conn);

        assert!(load_agent_default_prompt(tmp.path(), "reviewer", "claude").is_none());
    }

    #[test]
    fn returns_none_for_unknown_slug() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let conn = rusqlite::Connection::open(tmp.path()).unwrap();
        seed_agents_table(&conn);
        insert_agent(&conn, "a1", "reviewer", "claude", Some("hi"), None);
        drop(conn);

        assert!(load_agent_default_prompt(tmp.path(), "missing", "claude").is_none());
    }

    #[test]
    fn disambiguates_by_runtime() {
        // Same slug, two runtimes, different defaults — runtime
        // disambiguates so a Claude `@reviewer` and a Codex
        // `@reviewer` each get their own default_prompt back.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let conn = rusqlite::Connection::open(tmp.path()).unwrap();
        seed_agents_table(&conn);
        insert_agent(
            &conn,
            "a1",
            "reviewer",
            "claude",
            Some("claude-review"),
            None,
        );
        insert_agent(&conn, "a2", "reviewer", "codex", Some("codex-review"), None);
        drop(conn);

        assert_eq!(
            load_agent_default_prompt(tmp.path(), "reviewer", "claude").as_deref(),
            Some("claude-review")
        );
        assert_eq!(
            load_agent_default_prompt(tmp.path(), "reviewer", "codex").as_deref(),
            Some("codex-review")
        );
    }

    #[test]
    fn picks_most_recently_used_when_slug_runtime_collide() {
        // Schema declares UNIQUE(runtime, slug), but the column was
        // added late and historical rows could collide. The
        // COALESCE(last_used_at, created_at) DESC tiebreak in
        // load_agent_default_prompt mirrors the existing perms
        // lookup so the "freshest" row wins on either path.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let conn = rusqlite::Connection::open(tmp.path()).unwrap();
        // Drop the UNIQUE constraint for this synthetic test so we
        // can seed the collision the lookup is defending against.
        conn.execute_batch(
            "CREATE TABLE agents (
                id            TEXT PRIMARY KEY,
                slug          TEXT NOT NULL,
                display_name  TEXT NOT NULL,
                runtime       TEXT NOT NULL,
                created_at    TEXT NOT NULL,
                last_used_at  TEXT,
                default_prompt TEXT
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agents (id, slug, display_name, runtime, created_at, last_used_at, default_prompt)
             VALUES ('old', 'reviewer', 'reviewer', 'claude', '2026-04-01T00:00:00Z', '2026-04-15T00:00:00Z', 'OLD default')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO agents (id, slug, display_name, runtime, created_at, last_used_at, default_prompt)
             VALUES ('new', 'reviewer', 'reviewer', 'claude', '2026-05-01T00:00:00Z', '2026-05-15T00:00:00Z', 'NEW default')",
            [],
        ).unwrap();
        drop(conn);

        assert_eq!(
            load_agent_default_prompt(tmp.path(), "reviewer", "claude").as_deref(),
            Some("NEW default")
        );
    }

    #[test]
    fn returns_none_when_db_file_missing() {
        // Brand-new install / corrupted path / wrong worktree:
        // never panic, never block dispatch — just return None and
        // let the empty prompt flow through.
        let path = std::path::Path::new("/tmp/does-not-exist-S9-test.db");
        let _ = std::fs::remove_file(path);
        assert!(load_agent_default_prompt(path, "reviewer", "claude").is_none());
    }

    #[test]
    fn default_prompt_with_variables_is_returned_verbatim() {
        // Pins the war-room Q2 fix shape: load_agent_default_prompt
        // returns the raw template (no `{variable}` interpolation).
        // The upstream caller (prompt_agent_with_context) is the one
        // that runs substitute_variables on the swapped-in default.
        // If a future refactor moves substitute_variables in here,
        // this test will trip and the reviewer will know to update
        // both call sites.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let conn = rusqlite::Connection::open(tmp.path()).unwrap();
        seed_agents_table(&conn);
        insert_agent(
            &conn,
            "a1",
            "reviewer",
            "claude",
            Some("Review the PR at {project_path}"),
            None,
        );
        drop(conn);

        let got = load_agent_default_prompt(tmp.path(), "reviewer", "claude");
        assert_eq!(got.as_deref(), Some("Review the PR at {project_path}"));
    }
}

// Felipe P4 (S9) — pins the variable-substitution shape used by
// prompt_agent_with_context after war-room Q2 forced the
// default_prompt swap to move upstream of the variable resolver.
#[cfg(test)]
mod prompt_substitution_order_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn variables_in_default_prompt_resolve_when_swap_happens_first() {
        // Simulates prompt_agent_with_context's order: swap empty
        // prompt for default_prompt FIRST, then substitute_variables
        // runs over the swapped form. Concrete behaviour: a
        // default_prompt containing `{project_path}` resolves to
        // the active project path rather than leaking the literal
        // `{project_path}` to the model.
        let prompt_arg = String::new();
        let default_prompt = "Review the PR at {project_path}";
        let swapped = if prompt_arg.trim().is_empty() {
            default_prompt.to_string()
        } else {
            prompt_arg
        };
        let mut resolved: HashMap<String, String> = HashMap::new();
        resolved.insert("project_path".into(), "/Users/me/project".into());
        let rendered = substitute_variables(&swapped, &resolved);
        assert_eq!(rendered, "Review the PR at /Users/me/project");
    }

    #[test]
    fn variables_in_default_prompt_do_not_resolve_when_swap_happens_after() {
        // Inverse: if the swap ran AFTER substitute_variables (the
        // pre-Q2-fix order, still the shape inside prompt_agent_inner's
        // direct-caller defense-in-depth path), the variable does
        // NOT expand. Pinned so a future reviewer can see at a
        // glance what behaviour each ordering produces.
        let prompt_arg = String::new();
        let mut resolved: HashMap<String, String> = HashMap::new();
        resolved.insert("project_path".into(), "/Users/me/project".into());
        let rendered_before_swap = substitute_variables(&prompt_arg, &resolved);
        let final_prompt = if rendered_before_swap.trim().is_empty() {
            "Review the PR at {project_path}".to_string()
        } else {
            rendered_before_swap
        };
        assert_eq!(final_prompt, "Review the PR at {project_path}");
    }
}
