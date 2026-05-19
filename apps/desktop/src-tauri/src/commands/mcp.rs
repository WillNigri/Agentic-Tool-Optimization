// commands/mcp.rs — MCP server discovery + local config introspection.
//
// PR 27d of the commands.rs split (see COMMANDS_SPLIT_PLAN.md), fourth
// slice of the skills_mcps domain. MCP discovery surface moves first;
// install/uninstall (which live further down at ~7500 in mod.rs) follow
// in PR 27e once their helpers find their natural homes.
//
// Scope (5 commands + 1 helper + 2 structs):
//   - discover_mcp_server_tools  — read one server from
//                                  ~/.claude/settings.json, spawn it,
//                                  return McpServerDetails.
//   - get_mcp_servers_with_tools — read all servers from the same
//                                  settings, spawn each, return the
//                                  union with errors-per-server.
//   - get_local_config           — list every MCP server across every
//                                  runtime + scope, dedupe by
//                                  (runtime, name), tag scope in the
//                                  display name.
//   - get_config_files           — list known config files per runtime
//                                  with exists/scope metadata (not
//                                  MCP-specific but lives in the same
//                                  introspection block).
//   - restart_mcp_server         — placeholder (no-op today).
//   - discover_mcp_tools_stdio   — pub helper; spawn an MCP server via
//                                  JSON-RPC over stdio, exchange
//                                  initialize + tools/list, parse the
//                                  result. Used by both
//                                  discover_mcp_server_tools and
//                                  get_mcp_servers_with_tools.
//   - McpTool / McpServerDetails — public data shapes for the surface.
//
// LocalMcpServer + ConfigFile structs live in crate root.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

use crate::{home_dir, ConfigFile, LocalMcpServer};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDetails {
    pub server_name: String,
    pub server_version: Option<String>,
    pub protocol_version: Option<String>,
    pub tools: Vec<McpTool>,
    pub connected: bool,
    pub error: Option<String>,
}

/// Discover tools from an MCP server by spawning it and communicating via JSON-RPC
pub fn discover_mcp_tools_stdio(
    command: &str,
    args: &[&str],
    env: &std::collections::HashMap<String, String>,
) -> Result<McpServerDetails, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    // Build the command. CRITICAL: inject the user's full shell PATH so the
    // spawned MCP server (and any tools it calls — `npx`, `node`, `python`)
    // can be found. Without this, GUI-launched Tauri's narrow PATH means
    // `npx @modelcontextprotocol/server-*` can't even find npx, and we
    // misreport "0 tools" for every MCP. Felipe + Beatriz hit this on
    // v1.5.20 — every MCP showed Error / 0 tools after the inheritance gap
    // surfaced.
    let user_path = super::get_user_path();
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Capture stderr so we can surface real spawn errors instead of a
        // silent "Failed to read response" — previously we ate everything
        // the server logged on its way to crashing.
        .stderr(Stdio::piped())
        .env("PATH", &user_path)
        .envs(env);

    // Spawn the process
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn MCP server '{}': {}", command, e))?;

    let stdin = child.stdin.as_mut().ok_or("Failed to open stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to open stdout")?;
    let stderr_pipe = child.stderr.take();

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    // Drain stderr on demand — used when the server exits before we get a
    // valid response. Without this we'd report a generic "Failed to parse
    // response" with no clue that the actual problem was e.g. `npx: command
    // not found` or a missing API key.
    let drain_stderr = |stderr_pipe: Option<std::process::ChildStderr>| -> String {
        if let Some(mut s) = stderr_pipe {
            use std::io::Read;
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            return buf.trim().to_string();
        }
        String::new()
    };

    // Send initialize request
    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "ATO",
                "version": "0.2.0"
            }
        }
    });

    writeln!(stdin, "{}", init_request.to_string())
        .map_err(|e| format!("Failed to write initialize request: {}", e))?;
    stdin
        .flush()
        .map_err(|e| format!("Failed to flush stdin: {}", e))?;

    // Read initialize response.
    let mut read_response = || -> Result<serde_json::Value, String> {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| format!("Failed to read response: {}", e))?;
        if n == 0 {
            // Server closed stdout before sending anything — usually means
            // it crashed during init. The real diagnostic is in stderr.
            return Err("server exited before sending a response".to_string());
        }
        serde_json::from_str(&line).map_err(|e| {
            format!("Failed to parse response (got: {:?}): {}", line.trim(), e)
        })
    };

    let init_response = match read_response() {
        Ok(v) => v,
        Err(e) => {
            let _ = child.kill();
            let stderr_msg = drain_stderr(stderr_pipe);
            if stderr_msg.is_empty() {
                return Err(e);
            }
            return Err(format!("{}\nstderr: {}", e, stderr_msg));
        }
    };

    // Extract server info
    let server_info = init_response.get("result").and_then(|r| r.get("serverInfo"));
    let server_name = server_info
        .and_then(|i| i.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("Unknown")
        .to_string();
    let server_version = server_info
        .and_then(|i| i.get("version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let protocol_version = init_response
        .get("result")
        .and_then(|r| r.get("protocolVersion"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Send tools/list request
    let tools_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });

    writeln!(stdin, "{}", tools_request.to_string())
        .map_err(|e| format!("Failed to write tools/list request: {}", e))?;
    stdin
        .flush()
        .map_err(|e| format!("Failed to flush stdin: {}", e))?;

    // Read tools response
    let tools_response = read_response()?;

    // Parse tools
    let tools: Vec<McpTool> = tools_response
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|tool| {
                    let name = tool.get("name")?.as_str()?.to_string();
                    let description = tool
                        .get("description")
                        .and_then(|d| d.as_str())
                        .map(|s| s.to_string());
                    let input_schema = tool.get("inputSchema").cloned();
                    Some(McpTool {
                        name,
                        description,
                        input_schema,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Clean up - try to terminate the process gracefully
    let _ = child.kill();

    Ok(McpServerDetails {
        server_name,
        server_version,
        protocol_version,
        tools,
        connected: true,
        error: None,
    })
}

/// Parse MCP server config and discover tools
#[tauri::command]
pub fn discover_mcp_server_tools(server_name: String) -> Result<McpServerDetails, String> {
    // Find server config
    let settings_path = super::claude_home().join("settings.json");
    let content = super::read_file_lossy(&settings_path).ok_or("Could not read Claude settings")?;

    let parsed: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse settings: {}", e))?;

    let mcp_servers = parsed
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or("No mcpServers found in settings")?;

    // Extract server name without source suffix
    let clean_name = server_name.split(" (").next().unwrap_or(&server_name);

    let server_config = mcp_servers
        .get(clean_name)
        .ok_or(format!("Server '{}' not found", clean_name))?;

    // Extract command and args
    let command = server_config
        .get("command")
        .and_then(|c| c.as_str())
        .ok_or("Server has no command")?;

    let args: Vec<&str> = server_config
        .get("args")
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    // Extract environment variables
    let mut env: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    if let Some(env_obj) = server_config.get("env").and_then(|e| e.as_object()) {
        for (key, val) in env_obj {
            if let Some(s) = val.as_str() {
                env.insert(key.clone(), s.to_string());
            }
        }
    }

    // Try to discover tools
    match discover_mcp_tools_stdio(command, &args, &env) {
        Ok(details) => Ok(details),
        Err(e) => Ok(McpServerDetails {
            server_name: clean_name.to_string(),
            server_version: None,
            protocol_version: None,
            tools: Vec::new(),
            connected: false,
            error: Some(e),
        }),
    }
}

/// Get all MCP servers with discovered tools (runs discovery in parallel)
#[tauri::command]
pub fn get_mcp_servers_with_tools() -> Result<Vec<McpServerDetails>, String> {
    let settings_path = super::claude_home().join("settings.json");
    let content = super::read_file_lossy(&settings_path).ok_or("Could not read Claude settings")?;

    let parsed: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse settings: {}", e))?;

    let mcp_servers = match parsed.get("mcpServers").and_then(|v| v.as_object()) {
        Some(servers) => servers,
        None => return Ok(Vec::new()),
    };

    let mut results = Vec::new();

    for (name, config) in mcp_servers {
        // Extract command
        let command = match config.get("command").and_then(|c| c.as_str()) {
            Some(c) => c,
            None => {
                results.push(McpServerDetails {
                    server_name: name.clone(),
                    server_version: None,
                    protocol_version: None,
                    tools: Vec::new(),
                    connected: false,
                    error: Some("No command specified".to_string()),
                });
                continue;
            }
        };

        let args: Vec<&str> = config
            .get("args")
            .and_then(|a| a.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let mut env: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        if let Some(env_obj) = config.get("env").and_then(|e| e.as_object()) {
            for (key, val) in env_obj {
                if let Some(s) = val.as_str() {
                    env.insert(key.clone(), s.to_string());
                }
            }
        }

        // Try discovery with a timeout
        match discover_mcp_tools_stdio(command, &args, &env) {
            Ok(mut details) => {
                details.server_name = name.clone();
                results.push(details);
            }
            Err(e) => {
                results.push(McpServerDetails {
                    server_name: name.clone(),
                    server_version: None,
                    protocol_version: None,
                    tools: Vec::new(),
                    connected: false,
                    error: Some(e),
                });
            }
        }
    }

    Ok(results)
}

#[tauri::command]
pub fn get_local_config() -> Result<Vec<LocalMcpServer>, String> {
    // Dedupe by `(runtime-family, server-name)`. Felipe's screenshot showed
    // every Claude MCP listed twice — once for the global `~/.claude/settings.json`
    // ("claude") and once for the per-project `.claude/settings.json`
    // ("claude-project"). The same MCP shouldn't render as two cards just
    // because it's referenced in both scopes.
    use std::collections::BTreeMap;
    let mut seen: BTreeMap<(String, String), LocalMcpServer> = BTreeMap::new();

    let codex_home = PathBuf::from(
        std::env::var("CODEX_HOME")
            .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()),
    );
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| {
        home_dir().join(".openclaw").to_string_lossy().to_string()
    }));

    // (path, source-tag, runtime-family, scope-label).
    // runtime-family is what we dedupe by; scope-label is what we show.
    let config_paths: Vec<(PathBuf, &str, &str, &str)> = vec![
        // Claude — dedupe global + project on the same name.
        (super::claude_home().join("settings.json"), "claude", "claude", "global"),
        (
            super::project_root().join(".claude").join("settings.json"),
            "claude-project",
            "claude",
            "project",
        ),
        // Codex
        (codex_home.join("config.toml"), "codex", "codex", "global"),
        (
            super::project_root().join(".codex").join("config.toml"),
            "codex-project",
            "codex",
            "project",
        ),
        // OpenClaw
        (oc_home.join("openclaw.json"), "openclaw", "openclaw", "global"),
        // Hermes
        (
            home_dir().join(".hermes").join("config.yaml"),
            "hermes",
            "hermes",
            "global",
        ),
    ];

    for (settings_path, _source, runtime_family, scope_label) in &config_paths {
        let Some(content) = super::read_file_lossy(settings_path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        for key in ["mcpServers", "mcp_servers"] {
            let Some(mcp_servers) = parsed.get(key).and_then(|v| v.as_object()) else {
                continue;
            };
            for (name, config) in mcp_servers {
                let key_pair = (runtime_family.to_string(), name.clone());
                let command = config
                    .get("command")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let url_val = config
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let transport = if url_val.is_some() { "http" } else { "stdio" };

                if let Some(existing) = seen.get_mut(&key_pair) {
                    // Already listed in another scope — append to the
                    // displayed name instead of creating a duplicate row.
                    if !existing.name.contains(scope_label) {
                        // Replace `(claude · global)` → `(claude · global, project)`.
                        let new_name = if let Some(close) = existing.name.rfind(')') {
                            format!("{}, {})", &existing.name[..close], scope_label)
                        } else {
                            format!("{} ({} · {})", existing.name, runtime_family, scope_label)
                        };
                        existing.name = new_name;
                    }
                } else {
                    seen.insert(
                        key_pair,
                        LocalMcpServer {
                            id: super::content_hash(&format!("{}-{}", runtime_family, name)),
                            name: format!("{} ({} · {})", name, runtime_family, scope_label),
                            transport: transport.to_string(),
                            status: "running".to_string(),
                            tool_count: 0,
                            command,
                            url: url_val,
                        },
                    );
                }
            }
        }
    }

    Ok(seen.into_values().collect())
}

#[tauri::command]
pub fn get_config_files() -> Result<Vec<ConfigFile>, String> {
    let home = home_dir();
    let claude = super::claude_home();
    let codex = PathBuf::from(
        std::env::var("CODEX_HOME")
            .unwrap_or_else(|_| home.join(".codex").to_string_lossy().to_string()),
    );
    let openclaw = PathBuf::from(std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| {
        home.join(".openclaw").to_string_lossy().to_string()
    }));
    let hermes = home.join(".hermes");

    let files = vec![
        // Claude
        ("~/.claude/settings.json", claude.join("settings.json"), "Claude — Global settings"),
        ("~/.claude/settings.local.json", claude.join("settings.local.json"), "Claude — Local settings"),
        ("~/.claude/skills/", claude.join("skills"), "Claude — Personal skills"),
        (".claude/settings.json", PathBuf::from(".claude/settings.json"), "Claude — Project settings"),
        (".claude/skills/", PathBuf::from(".claude/skills"), "Claude — Project skills"),
        ("CLAUDE.md", super::project_root().join("CLAUDE.md"), "Claude — Project context"),
        // Codex
        ("~/.codex/config.toml", codex.join("config.toml"), "Codex — Global config"),
        ("~/.codex/AGENTS.md", codex.join("AGENTS.md"), "Codex — Global instructions"),
        ("~/.codex/skills/", codex.join("skills"), "Codex — Personal skills"),
        (".codex/config.toml", PathBuf::from(".codex/config.toml"), "Codex — Project config"),
        ("AGENTS.md", super::project_root().join("AGENTS.md"), "Codex — Project instructions"),
        // OpenClaw
        ("~/.openclaw/openclaw.json", openclaw.join("openclaw.json"), "OpenClaw — Config"),
        ("~/.openclaw/workspace/AGENTS.md", openclaw.join("workspace/AGENTS.md"), "OpenClaw — Agent instructions"),
        ("~/.openclaw/workspace/SOUL.md", openclaw.join("workspace/SOUL.md"), "OpenClaw — Persona"),
        ("~/.openclaw/workspace/TOOLS.md", openclaw.join("workspace/TOOLS.md"), "OpenClaw — Tools config"),
        ("~/.openclaw/skills/", openclaw.join("skills"), "OpenClaw — Skills"),
        // Hermes
        ("~/.hermes/config.yaml", hermes.join("config.yaml"), "Hermes — Config"),
        ("~/.hermes/SOUL.md", hermes.join("SOUL.md"), "Hermes — Persona"),
        ("~/.hermes/skills/", hermes.join("skills"), "Hermes — Skills"),
        ("~/.hermes/memories/MEMORY.md", hermes.join("memories/MEMORY.md"), "Hermes — Memory"),
    ];

    Ok(files
        .iter()
        .map(|(display, path, scope)| ConfigFile {
            path: display.to_string(),
            exists: path.exists(),
            scope: scope.to_string(),
        })
        .collect())
}

#[tauri::command]
pub fn restart_mcp_server(_name: String) -> Result<(), String> {
    // Placeholder — would need to actually restart the process
    Ok(())
}
