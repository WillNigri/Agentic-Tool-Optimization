// commands/context.rs — context-window introspection.
//
// PR 11 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope:
//   - `get_context_estimate`     — generic Claude-flavored estimate
//                                  (system + skills + CLAUDE.md + MCP +
//                                  conversation), used when the runtime
//                                  argument isn't known.
//   - `get_context_for_runtime`  — per-runtime breakdown for
//                                  claude/codex/openclaw/hermes; routes
//                                  unknown runtimes back to the generic
//                                  estimate.
//   - `list_context_files`       — every memory/settings/config file
//                                  the desktop knows about, per runtime.
//   - `read_context_file`        — read one file's contents.
//   - `write_context_file`       — overwrite one file's contents.
//
// Helpers moved with the commands (only callers are context commands):
//   - `dir_skill_bytes` (sum of .md files in a directory)
//   - `file_tokens` (single-file token estimate)
//
// Cross-cutting helpers (claude_home, project_root, home_dir,
// estimate_tokens, read_file_lossy, which_claude, which_cli,
// load_openclaw_ssh_config, openclaw_ssh_command, discover_project_roots)
// stay in commands/mod.rs and are reached via `super::`. They have
// many other callers that will migrate to their natural domains in
// later PRs of the split — premature promotion to `shared.rs` would
// require knowing every callsite, which we don't yet.

use std::fs;
use std::path::PathBuf;

use crate::{ContextBreakdown, ContextCategory};

use super::{
    claude_home, home_dir, project_root, estimate_tokens,
    read_file_lossy, which_claude, which_cli,
    load_openclaw_ssh_config, openclaw_ssh_command,
    discover_project_roots,
};

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContextFile {
    pub runtime: String,
    pub name: String,
    pub file_path: String,
    pub token_count: u64,
    pub exists: bool,
}

#[tauri::command]
pub fn get_context_estimate() -> Result<ContextBreakdown, String> {
    let mut categories = Vec::new();
    let mut total: u64 = 0;

    // System prompts (estimated)
    let system_tokens: u64 = 28000;
    categories.push(ContextCategory { name: "System Prompts".into(), tokens: system_tokens, color: "#FF4466".into() });
    total += system_tokens;

    // Skills
    let personal_dir = claude_home().join("skills");
    let project_dir = project_root().join(".claude").join("skills");
    let mut skill_bytes: u64 = 0;
    for dir in [&personal_dir, &project_dir] {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    skill_bytes += fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                } else if p.is_dir() {
                    let sm = p.join("SKILL.md");
                    skill_bytes += fs::metadata(&sm).map(|m| m.len()).unwrap_or(0);
                }
            }
        }
    }
    let skill_tokens = estimate_tokens(skill_bytes);
    categories.push(ContextCategory { name: format!("Skills"), tokens: skill_tokens, color: "#00FFB2".into() });
    total += skill_tokens;

    // CLAUDE.md
    let claude_md = project_root().join("CLAUDE.md");
    let claude_md_tokens = fs::metadata(&claude_md)
        .map(|m| estimate_tokens(m.len()))
        .unwrap_or(0);
    categories.push(ContextCategory { name: "CLAUDE.md".into(), tokens: claude_md_tokens, color: "#FFB800".into() });
    total += claude_md_tokens;

    // MCP schemas (estimated based on config)
    let settings_path = claude_home().join("settings.json");
    let mcp_tokens: u64 = if settings_path.exists() {
        if let Some(content) = read_file_lossy(&settings_path) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                let server_count = parsed.get("mcpServers")
                    .and_then(|v| v.as_object())
                    .map(|m| m.len())
                    .unwrap_or(0);
                (server_count as u64) * 2500 // ~2500 tokens per MCP server schema
            } else { 0 }
        } else { 0 }
    } else { 0 };
    categories.push(ContextCategory { name: "MCP Schemas".into(), tokens: mcp_tokens, color: "#3b82f6".into() });
    total += mcp_tokens;

    // Conversation (estimated from recent session)
    let conv_tokens: u64 = 15000; // rough estimate
    categories.push(ContextCategory { name: "Conversation".into(), tokens: conv_tokens, color: "#a78bfa".into() });
    total += conv_tokens;

    Ok(ContextBreakdown {
        total_tokens: total,
        limit: 200000,
        categories,
    })
}

/// Estimate byte size of all .md files in a directory (recursively one level)
pub fn dir_skill_bytes(dir: &PathBuf) -> u64 {
    let mut bytes: u64 = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().map_or(false, |e| e == "md") {
                bytes += fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            } else if p.is_dir() {
                let sm = p.join("SKILL.md");
                bytes += fs::metadata(&sm).map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    bytes
}

pub fn file_tokens(path: &PathBuf) -> u64 {
    fs::metadata(path).map(|m| estimate_tokens(m.len())).unwrap_or(0)
}

#[tauri::command]
pub fn get_context_for_runtime(runtime: String) -> Result<ContextBreakdown, String> {
    // Check if runtime is installed — return limit=0 if not (frontend shows "not connected")
    let is_available = match runtime.as_str() {
        "claude" => which_claude().is_some(),
        "codex" => which_cli("codex").is_some(),
        "openclaw" => {
            let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
                .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));
            oc_home.exists() || load_openclaw_ssh_config().is_ok()
        }
        "hermes" => {
            which_cli("hermes").is_some() || home_dir().join(".hermes").exists()
        }
        _ => false,
    };

    if !is_available {
        return Ok(ContextBreakdown {
            total_tokens: 0,
            limit: 0, // Frontend uses limit=0 to detect "not connected"
            categories: Vec::new(),
        });
    }

    let mut categories = Vec::new();
    let mut total: u64 = 0;

    match runtime.as_str() {
        "claude" => {
            // Always loaded
            let sys: u64 = 28000;
            categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
            total += sys;
            // CLAUDE.md — always loaded
            let cm = file_tokens(&project_root().join("CLAUDE.md"));
            categories.push(ContextCategory { name: "CLAUDE.md".into(), tokens: cm, color: "#FFB800".into() });
            total += cm;
            // MCP schemas — always loaded
            let settings_path = claude_home().join("settings.json");
            let mcp: u64 = read_file_lossy(&settings_path)
                .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                .and_then(|v| v.get("mcpServers").and_then(|s| s.as_object()).map(|m| m.len() as u64 * 2500))
                .unwrap_or(0);
            categories.push(ContextCategory { name: "MCP Schemas".into(), tokens: mcp, color: "#3b82f6".into() });
            total += mcp;
            // Conversation — estimated active
            categories.push(ContextCategory { name: "Conversation".into(), tokens: 15000, color: "#a78bfa".into() });
            total += 15000;
            // Skills — on-demand, NOT counted in total (loaded only when triggered)
            let skill_bytes = dir_skill_bytes(&claude_home().join("skills"))
                + dir_skill_bytes(&project_root().join(".claude").join("skills"));
            let st = estimate_tokens(skill_bytes);
            categories.push(ContextCategory { name: "Skills (on-demand)".into(), tokens: st, color: "#00FFB233".into() });
            // NOT added to total — skills are loaded individually when invoked

            Ok(ContextBreakdown { total_tokens: total, limit: 200000, categories })
        }
        "codex" => {
            let codex_home = PathBuf::from(std::env::var("CODEX_HOME")
                .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()));
            // Always loaded
            let sys: u64 = 20000;
            categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
            total += sys;
            let agents_md = file_tokens(&project_root().join("AGENTS.md"))
                + file_tokens(&codex_home.join("AGENTS.md"));
            categories.push(ContextCategory { name: "AGENTS.md".into(), tokens: agents_md, color: "#FFB800".into() });
            total += agents_md;
            let cfg = file_tokens(&codex_home.join("config.toml"));
            categories.push(ContextCategory { name: "config.toml".into(), tokens: cfg, color: "#3b82f6".into() });
            total += cfg;
            categories.push(ContextCategory { name: "Conversation".into(), tokens: 12000, color: "#a78bfa".into() });
            total += 12000;
            // Skills — on-demand
            let skill_bytes = dir_skill_bytes(&codex_home.join("skills"))
                + dir_skill_bytes(&project_root().join(".agents").join("skills"))
                + dir_skill_bytes(&project_root().join(".codex").join("skills"));
            let st = estimate_tokens(skill_bytes);
            categories.push(ContextCategory { name: "Skills (on-demand)".into(), tokens: st, color: "#00FFB233".into() });

            Ok(ContextBreakdown { total_tokens: total, limit: 192000, categories })
        }
        "openclaw" => {
            let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
                .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));
            let ws = oc_home.join("workspace");
            let has_local = ws.exists();

            if has_local {
                // Local OpenClaw install
                let sys: u64 = 15000;
                categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
                total += sys;
                let agents = file_tokens(&ws.join("AGENTS.md"));
                categories.push(ContextCategory { name: "AGENTS.md".into(), tokens: agents, color: "#FFB800".into() });
                total += agents;
                let soul = file_tokens(&ws.join("SOUL.md"));
                categories.push(ContextCategory { name: "SOUL.md".into(), tokens: soul, color: "#f97316".into() });
                total += soul;
                let tools = file_tokens(&ws.join("TOOLS.md"));
                categories.push(ContextCategory { name: "TOOLS.md".into(), tokens: tools, color: "#06b6d4".into() });
                total += tools;
                let mem_dir = ws.join("memory");
                let mem_bytes = dir_skill_bytes(&mem_dir);
                let mem = estimate_tokens(mem_bytes);
                categories.push(ContextCategory { name: "Memory".into(), tokens: mem, color: "#a78bfa".into() });
                total += mem;
                let skill_bytes = dir_skill_bytes(&oc_home.join("skills"))
                    + dir_skill_bytes(&ws.join("skills"));
                let st = estimate_tokens(skill_bytes);
                categories.push(ContextCategory { name: "Skills (on-demand)".into(), tokens: st, color: "#00FFB233".into() });
            } else {
                // Remote OpenClaw via SSH — fetch file sizes
                let sys: u64 = 15000;
                categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
                total += sys;

                if let Ok(result) = openclaw_ssh_command(
                    "exec 'echo $(wc -c < ~/.openclaw/workspace/SOUL.md 2>/dev/null || echo 0) $(wc -c < ~/.openclaw/workspace/AGENTS.md 2>/dev/null || echo 0) $(wc -c < ~/.openclaw/workspace/TOOLS.md 2>/dev/null || echo 0) $(wc -c < ~/.openclaw/workspace/MEMORY.md 2>/dev/null || echo 0) $(find ~/.openclaw/workspace/skills -name SKILL.md -exec wc -c {} + 2>/dev/null | tail -1 | awk \"{print \\$1}\" || echo 0) $(find ~/.openclaw/workspace/memory -type f -exec wc -c {} + 2>/dev/null | tail -1 | awk \"{print \\$1}\" || echo 0)'"
                ) {
                    // Parse the response — it should be a string with space-separated byte counts
                    let text = result.as_str().unwrap_or("").trim().to_string();
                    let parts: Vec<u64> = text.split_whitespace()
                        .filter_map(|s| s.parse().ok())
                        .collect();

                    let soul_bytes = parts.first().copied().unwrap_or(0);
                    let agents_bytes = parts.get(1).copied().unwrap_or(0);
                    let tools_bytes = parts.get(2).copied().unwrap_or(0);
                    let memory_bytes = parts.get(3).copied().unwrap_or(0);
                    let skills_bytes = parts.get(4).copied().unwrap_or(0);
                    let mem_dir_bytes = parts.get(5).copied().unwrap_or(0);

                    let soul = estimate_tokens(soul_bytes);
                    categories.push(ContextCategory { name: "SOUL.md".into(), tokens: soul, color: "#f97316".into() });
                    total += soul;
                    let agents = estimate_tokens(agents_bytes);
                    categories.push(ContextCategory { name: "AGENTS.md".into(), tokens: agents, color: "#FFB800".into() });
                    total += agents;
                    let tools = estimate_tokens(tools_bytes);
                    categories.push(ContextCategory { name: "TOOLS.md".into(), tokens: tools, color: "#06b6d4".into() });
                    total += tools;
                    let mem = estimate_tokens(memory_bytes + mem_dir_bytes);
                    categories.push(ContextCategory { name: "Memory".into(), tokens: mem, color: "#a78bfa".into() });
                    total += mem;
                    let st = estimate_tokens(skills_bytes);
                    categories.push(ContextCategory { name: "Skills (on-demand)".into(), tokens: st, color: "#00FFB233".into() });
                } else {
                    // SSH failed — show estimate
                    categories.push(ContextCategory { name: "SOUL.md (estimated)".into(), tokens: 2000, color: "#f97316".into() });
                    total += 2000;
                    categories.push(ContextCategory { name: "AGENTS.md (estimated)".into(), tokens: 500, color: "#FFB800".into() });
                    total += 500;
                }
            }

            Ok(ContextBreakdown { total_tokens: total, limit: 200000, categories })
        }
        "hermes" => {
            let hermes_home = home_dir().join(".hermes");
            // Always loaded
            let sys: u64 = 12000;
            categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
            total += sys;
            // SOUL.md
            let soul = file_tokens(&hermes_home.join("SOUL.md"));
            categories.push(ContextCategory { name: "SOUL.md".into(), tokens: soul, color: "#f97316".into() });
            total += soul;
            // Memory — always loaded
            let mem_bytes = file_tokens(&hermes_home.join("memories").join("MEMORY.md"))
                + file_tokens(&hermes_home.join("memories").join("USER.md"));
            categories.push(ContextCategory { name: "Memory".into(), tokens: mem_bytes, color: "#a78bfa".into() });
            total += mem_bytes;
            // Config
            let cfg = file_tokens(&hermes_home.join("config.yaml"));
            categories.push(ContextCategory { name: "config.yaml".into(), tokens: cfg, color: "#3b82f6".into() });
            total += cfg;
            // Skills — on-demand
            let skills_dir = hermes_home.join("skills");
            let mut skill_bytes = dir_skill_bytes(&skills_dir);
            if skills_dir.exists() {
                if let Ok(entries) = fs::read_dir(&skills_dir) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            skill_bytes += dir_skill_bytes(&entry.path());
                        }
                    }
                }
            }
            let st = estimate_tokens(skill_bytes);
            categories.push(ContextCategory { name: "Skills (on-demand)".into(), tokens: st, color: "#00FFB233".into() });

            Ok(ContextBreakdown { total_tokens: total, limit: 128000, categories })
        }
        _ => get_context_estimate(),
    }
}

#[tauri::command]
pub fn list_context_files() -> Result<Vec<ContextFile>, String> {
    let mut files = Vec::new();

    // Claude context files
    let claude = claude_home();
    for name in ["CLAUDE.md", "settings.json", "settings.local.json"] {
        let p = claude.join(name);
        let exists = p.exists();
        let tokens = if exists { estimate_tokens(fs::metadata(&p).map(|m| m.len()).unwrap_or(0)) } else { 0 };
        files.push(ContextFile {
            runtime: "claude".into(), name: name.into(),
            file_path: p.to_string_lossy().into(), token_count: tokens, exists,
        });
    }
    // Project CLAUDE.md
    for proj in discover_project_roots() {
        let p = proj.join("CLAUDE.md");
        if p.exists() {
            let tokens = estimate_tokens(fs::metadata(&p).map(|m| m.len()).unwrap_or(0));
            let label = format!("CLAUDE.md ({})", proj.file_name().unwrap_or_default().to_string_lossy());
            files.push(ContextFile {
                runtime: "claude".into(), name: label,
                file_path: p.to_string_lossy().into(), token_count: tokens, exists: true,
            });
        }
    }

    // OpenClaw context files
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));
    let oc_workspace = oc_home.join("workspace");
    for name in ["SOUL.md", "AGENTS.md", "TOOLS.md", "config.yaml"] {
        let p = oc_workspace.join(name);
        let exists = p.exists();
        let tokens = if exists { estimate_tokens(fs::metadata(&p).map(|m| m.len()).unwrap_or(0)) } else { 0 };
        files.push(ContextFile {
            runtime: "openclaw".into(), name: name.into(),
            file_path: p.to_string_lossy().into(), token_count: tokens, exists,
        });
    }
    // Also check root .openclaw
    for name in ["SOUL.md", "config.yaml"] {
        let p = oc_home.join(name);
        if p.exists() && !files.iter().any(|f| f.file_path == p.to_string_lossy().to_string()) {
            let tokens = estimate_tokens(fs::metadata(&p).map(|m| m.len()).unwrap_or(0));
            files.push(ContextFile {
                runtime: "openclaw".into(), name: name.into(),
                file_path: p.to_string_lossy().into(), token_count: tokens, exists: true,
            });
        }
    }

    // Hermes context files
    let hermes_home = home_dir().join(".hermes");
    for name in ["SOUL.md", "config.yaml", "memories/MEMORY.md", "memories/USER.md"] {
        let p = hermes_home.join(name);
        let exists = p.exists();
        let tokens = if exists { estimate_tokens(fs::metadata(&p).map(|m| m.len()).unwrap_or(0)) } else { 0 };
        files.push(ContextFile {
            runtime: "hermes".into(), name: name.into(),
            file_path: p.to_string_lossy().into(), token_count: tokens, exists,
        });
    }

    // Filter to only existing files
    Ok(files.into_iter().filter(|f| f.exists).collect())
}

#[tauri::command]
pub fn read_context_file(file_path: String) -> Result<String, String> {
    read_file_lossy(&PathBuf::from(&file_path)).ok_or_else(|| format!("Cannot read: {}", file_path))
}

#[tauri::command]
pub fn write_context_file(file_path: String, content: String) -> Result<(), String> {
    fs::write(&file_path, &content).map_err(|e| format!("Failed to write: {}", e))
}
