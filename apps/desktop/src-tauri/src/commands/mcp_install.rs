// commands/mcp_install.rs — MCP server install / uninstall against each
// runtime's native config file.
//
// PR 27e of the commands.rs split (see COMMANDS_SPLIT_PLAN.md), fifth
// and final slice of the skills_mcps domain. The MCP discovery surface
// moved in PR 27d (mcp.rs); this PR adds the install/uninstall surface
// + its file-format adapters.
//
// Supported runtimes today:
//   - claude    → ~/.claude/settings.json `mcpServers.<name>` (JSON)
//   - gemini    → ~/.gemini/settings.json `mcpServers.<name>` (JSON)
//   - openclaw  → ~/.openclaw/openclaw.json (JSON)
//   - codex     → ~/.codex/config.toml [mcp_servers.<name>] (TOML)
//   - hermes    → ~/.hermes/config.yaml `mcp_servers.<name>` (YAML)
//
// Unsupported runtimes return a clear error so the UI can fall back to
// the "copy snippet" flow.
//
// Scope (2 commands + 4 helpers + 1 struct + tests):
//   - install_mcp_server     — write an MCP server entry into the
//                              runtime's config file (JSON / TOML / YAML).
//   - uninstall_mcp_server   — drop the named server from the config.
//   - McpInstallEntry        — public input shape from the UI.
//   - mcp_settings_path      — resolve runtime → config path.
//   - build_mcp_json_value   — entry → JSON value for JSON-shaped configs.
//   - write_with_perm_hint   — wrap fs::write so the error spells out
//                              the failing path AND points users at
//                              `chown` on permission denied.
//   - mcp_install_tests      — unit tests for path resolution + entry
//                              JSON shaping.
//
// Cross-domain helpers reached via super::* — claude_home, gemini_home.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::home_dir;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct McpInstallEntry {
    pub name: String,
    pub command: Option<String>,    // for stdio
    pub args: Option<Vec<String>>,  // for stdio
    pub env: Option<HashMap<String, String>>, // for stdio
    pub url: Option<String>,        // for sse/http
    pub transport: String,          // "stdio" | "sse" | "http"
}

fn mcp_settings_path(runtime: &str) -> Result<PathBuf, String> {
    match runtime {
        "claude" => Ok(super::claude_home().join("settings.json")),
        "gemini" => Ok(super::gemini_home().join("settings.json")),
        "codex" => {
            let codex_home = PathBuf::from(
                std::env::var("CODEX_HOME")
                    .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()),
            );
            Ok(codex_home.join("config.toml"))
        }
        "openclaw" => {
            let oc_home = PathBuf::from(
                std::env::var("OPENCLAW_HOME")
                    .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()),
            );
            Ok(oc_home.join("openclaw.json"))
        }
        "hermes" => Ok(home_dir().join(".hermes").join("config.yaml")),
        other => Err(format!(
            "Runtime '{}' does not support MCP install yet — copy the snippet manually.",
            other
        )),
    }
}

fn build_mcp_json_value(entry: &McpInstallEntry) -> serde_json::Value {
    if entry.transport == "stdio" {
        let mut obj = serde_json::Map::new();
        if let Some(cmd) = &entry.command {
            obj.insert("command".into(), serde_json::Value::String(cmd.clone()));
        }
        if let Some(args) = &entry.args {
            obj.insert(
                "args".into(),
                serde_json::Value::Array(
                    args.iter().map(|s| serde_json::Value::String(s.clone())).collect(),
                ),
            );
        }
        if let Some(env) = &entry.env {
            let mut env_obj = serde_json::Map::new();
            for (k, v) in env {
                env_obj.insert(k.clone(), serde_json::Value::String(v.clone()));
            }
            obj.insert("env".into(), serde_json::Value::Object(env_obj));
        }
        serde_json::Value::Object(obj)
    } else {
        let mut obj = serde_json::Map::new();
        if let Some(url) = &entry.url {
            obj.insert("url".into(), serde_json::Value::String(url.clone()));
        }
        serde_json::Value::Object(obj)
    }
}

#[tauri::command]
pub fn install_mcp_server(runtime: String, entry: McpInstallEntry) -> Result<String, String> {
    if entry.name.trim().is_empty() {
        return Err("MCP server name cannot be empty".to_string());
    }

    let path = mcp_settings_path(&runtime)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {}", e))?;
    }

    if runtime == "hermes" {
        // YAML path: load (or create), ensure mcp_servers map exists, insert.
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let mut doc: serde_yaml::Value = if existing.trim().is_empty() {
            serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
        } else {
            serde_yaml::from_str(&existing)
                .map_err(|e| format!("Invalid YAML in {:?}: {}", path, e))?
        };

        let map = doc
            .as_mapping_mut()
            .ok_or_else(|| format!("Config root in {:?} must be a mapping", path))?;
        let key = serde_yaml::Value::String("mcp_servers".to_string());
        let servers = map
            .entry(key)
            .or_insert(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
        let servers_map = servers
            .as_mapping_mut()
            .ok_or("`mcp_servers` exists but isn't a mapping")?;

        let mut server_map = serde_yaml::Mapping::new();
        if entry.transport == "stdio" {
            if let Some(cmd) = &entry.command {
                server_map.insert(
                    serde_yaml::Value::String("command".into()),
                    serde_yaml::Value::String(cmd.clone()),
                );
            }
            if let Some(args) = &entry.args {
                server_map.insert(
                    serde_yaml::Value::String("args".into()),
                    serde_yaml::Value::Sequence(
                        args.iter().map(|s| serde_yaml::Value::String(s.clone())).collect(),
                    ),
                );
            }
            if let Some(env) = &entry.env {
                let mut env_map = serde_yaml::Mapping::new();
                for (k, v) in env {
                    env_map.insert(
                        serde_yaml::Value::String(k.clone()),
                        serde_yaml::Value::String(v.clone()),
                    );
                }
                server_map.insert(
                    serde_yaml::Value::String("env".into()),
                    serde_yaml::Value::Mapping(env_map),
                );
            }
        } else if let Some(url) = &entry.url {
            server_map.insert(
                serde_yaml::Value::String("url".into()),
                serde_yaml::Value::String(url.clone()),
            );
        }

        servers_map.insert(
            serde_yaml::Value::String(entry.name.clone()),
            serde_yaml::Value::Mapping(server_map),
        );

        let serialized = serde_yaml::to_string(&doc)
            .map_err(|e| format!("Failed to serialize YAML: {}", e))?;
        write_with_perm_hint(&path, serialized.as_bytes())?;
    } else if runtime == "codex" {
        // TOML path: load (or create) the document, add an [mcp_servers.<name>] table.
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let mut doc: toml::Value = if existing.trim().is_empty() {
            toml::Value::Table(toml::value::Table::new())
        } else {
            toml::from_str(&existing).map_err(|e| format!("Invalid TOML in {:?}: {}", path, e))?
        };

        let table = doc.as_table_mut().ok_or("Codex config root must be a table")?;
        let servers = table
            .entry("mcp_servers".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        let servers_table = servers
            .as_table_mut()
            .ok_or("`mcp_servers` already exists but is not a table")?;

        let mut server_table = toml::value::Table::new();
        if entry.transport == "stdio" {
            if let Some(cmd) = &entry.command {
                server_table.insert("command".into(), toml::Value::String(cmd.clone()));
            }
            if let Some(args) = &entry.args {
                server_table.insert(
                    "args".into(),
                    toml::Value::Array(args.iter().map(|s| toml::Value::String(s.clone())).collect()),
                );
            }
            if let Some(env) = &entry.env {
                let mut env_table = toml::value::Table::new();
                for (k, v) in env {
                    env_table.insert(k.clone(), toml::Value::String(v.clone()));
                }
                server_table.insert("env".into(), toml::Value::Table(env_table));
            }
        } else if let Some(url) = &entry.url {
            server_table.insert("url".into(), toml::Value::String(url.clone()));
        }
        servers_table.insert(entry.name.clone(), toml::Value::Table(server_table));

        let serialized = toml::to_string_pretty(&doc)
            .map_err(|e| format!("Failed to serialize TOML: {}", e))?;
        write_with_perm_hint(&path, serialized.as_bytes())?;
    } else {
        // JSON path: load (or create) the document, ensure mcpServers exists, add entry.
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let mut doc: serde_json::Value = if existing.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&existing).map_err(|e| format!("Invalid JSON in {:?}: {}", path, e))?
        };

        if !doc.is_object() {
            return Err(format!("Config root in {:?} must be an object", path));
        }
        let obj = doc.as_object_mut().unwrap();
        if !obj.contains_key("mcpServers") {
            obj.insert("mcpServers".into(), serde_json::json!({}));
        }
        let servers = obj
            .get_mut("mcpServers")
            .and_then(|v| v.as_object_mut())
            .ok_or("`mcpServers` already exists but is not an object")?;
        servers.insert(entry.name.clone(), build_mcp_json_value(&entry));

        let serialized = serde_json::to_string_pretty(&doc)
            .map_err(|e| format!("Failed to serialize JSON: {}", e))?;
        write_with_perm_hint(&path, serialized.as_bytes())?;
    }

    Ok(path.to_string_lossy().to_string())
}

/// Counterpart to `install_mcp_server` — drops the named server from the
/// runtime's config file. Felipe's feedback: "preciso de uma opcao de
/// editar ou deletar os mcps". For edit we just delete + reinstall from
/// the frontend; both flows go through here.
#[tauri::command]
pub fn uninstall_mcp_server(runtime: String, name: String) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("MCP server name cannot be empty".to_string());
    }

    let path = mcp_settings_path(&runtime)?;
    if !path.exists() {
        return Ok(path.to_string_lossy().to_string()); // nothing to remove
    }

    if runtime == "hermes" {
        let existing = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
        let mut doc: serde_yaml::Value = serde_yaml::from_str(&existing)
            .map_err(|e| format!("Invalid YAML in {:?}: {}", path, e))?;
        if let Some(map) = doc.as_mapping_mut() {
            if let Some(serde_yaml::Value::Mapping(servers)) =
                map.get_mut(serde_yaml::Value::String("mcp_servers".into()))
            {
                servers.remove(&serde_yaml::Value::String(name.clone()));
            }
        }
        let serialized = serde_yaml::to_string(&doc)
            .map_err(|e| format!("Failed to serialize YAML: {}", e))?;
        write_with_perm_hint(&path, serialized.as_bytes())?;
    } else if runtime == "codex" {
        let existing = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
        let mut doc: toml::Value = toml::from_str(&existing)
            .map_err(|e| format!("Invalid TOML in {:?}: {}", path, e))?;
        if let Some(table) = doc.as_table_mut() {
            if let Some(servers) = table.get_mut("mcp_servers").and_then(|v| v.as_table_mut()) {
                servers.remove(&name);
            }
        }
        let serialized = toml::to_string_pretty(&doc)
            .map_err(|e| format!("Failed to serialize TOML: {}", e))?;
        write_with_perm_hint(&path, serialized.as_bytes())?;
    } else {
        // JSON path (claude, gemini, openclaw)
        let existing = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
        let mut doc: serde_json::Value = serde_json::from_str(&existing)
            .map_err(|e| format!("Invalid JSON in {:?}: {}", path, e))?;
        if let Some(obj) = doc.as_object_mut() {
            if let Some(servers) = obj.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
                servers.remove(&name);
            }
        }
        let serialized = serde_json::to_string_pretty(&doc)
            .map_err(|e| format!("Failed to serialize JSON: {}", e))?;
        write_with_perm_hint(&path, serialized.as_bytes())?;
    }

    Ok(path.to_string_lossy().to_string())
}

/// Wrap fs::write so the error spells out the failing path AND, on
/// permission denied, points users at the most likely cause (Felipe ran
/// into this on WSL). This is what made his marketplace installs fail
/// silently — the error was buried under "Failed to write" with no
/// actionable guidance.
fn write_with_perm_hint(path: &PathBuf, content: &[u8]) -> Result<(), String> {
    match fs::write(path, content) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Err(format!(
            "Permission denied writing {:?}. On WSL/Linux this usually means the file is owned by another user (e.g., root) — try `sudo chown $USER {:?}` or delete the file so ATO can recreate it.",
            path, path
        )),
        Err(e) => Err(format!("Failed to write {:?}: {}", path, e)),
    }
}

#[cfg(test)]
mod mcp_install_tests {
    use super::*;

    #[test]
    fn unsupported_runtime_errors() {
        // Now we support claude / codex / gemini / openclaw / hermes.
        assert!(mcp_settings_path("hermes").is_ok());
        assert!(mcp_settings_path("openclaw").is_ok());
        assert!(mcp_settings_path("nonsense-runtime").is_err());
    }

    #[test]
    fn build_stdio_value_has_command_args() {
        let entry = McpInstallEntry {
            name: "fs".into(),
            command: Some("npx".into()),
            args: Some(vec!["-y".into(), "@modelcontextprotocol/server-filesystem".into()]),
            env: None,
            url: None,
            transport: "stdio".into(),
        };
        let v = build_mcp_json_value(&entry);
        assert_eq!(v["command"], "npx");
        assert_eq!(v["args"][0], "-y");
    }
}
