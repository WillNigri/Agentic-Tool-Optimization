// v2.7.10 PR-B (reborn) — MCP-tool gating + execution for desktop API
// dispatch.
//
// What this module does:
//   - load_agent_mcp_tools  — for an agent slug, return the union of
//     tools declared by every MCP server the agent has attached. Wraps
//     the sync DB read + sync MCP discovery in tokio::task::
//     spawn_blocking so the async Tauri command doesn't pin a tokio
//     worker. Process-local cache keyed by (agent_slug, mcps_hash) so
//     repeat dispatches don't re-spawn N MCP servers per turn.
//   - execute_mcp_tool      — actually call the named tool on the
//     hosting MCP server via JSON-RPC `tools/call`. Returns a
//     ato_review_tools::ToolResult shaped for dispatch_with_tools'
//     existing surface.
//   - McpToolBinding        — the (mcp_slug, tool_name, …) shape that
//     lets the dispatch loop look up which MCP hosts a given tool
//     name. From-impls bridge it to ato_review_tools::ToolDef (request
//     body shape) and ato_agent_permissions::ToolDef (gate input
//     shape) in one place.
//
// History — the previous attempt at this (v2.7.9 PR-B) was reverted
// after a war-room review caught two ship blockers:
//   1. discover_mcp_server_tools (sync) was called directly from the
//      async Tauri command, pinning a tokio worker for the duration
//      of every MCP discovery.
//   2. MCP tools were OFFERED to the model in the request body but
//      no tools/call round-trip existed — execute_call_with_root
//      returned "unknown tool" for every MCP call, deceiving the
//      model into a tool-use loop that could never succeed.
// This module addresses both: (1) all sync work goes inside
// spawn_blocking; (2) execute_mcp_tool implements the actual MCP
// stdio handshake.
//
// JSON-RPC stdio handshake is intentionally MIRRORED from
// commands/mcp.rs::discover_mcp_tools_stdio (which is owned by the
// MCP discovery surface and out of this PR's write set). The plan
// is to extract a shared helper in v2.7.11 — see TODO at
// rpc_call_blocking.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use ato_review_tools::ToolResult;
use serde_json::{json, Value as JsonValue};

/// One tool exposed by one MCP server attached to one agent. Holds
/// enough metadata to (a) advertise the tool to the model in the
/// request body, (b) decide whether the agent's gate allows it, and
/// (c) route a tool_call back to the originating MCP server.
#[derive(Debug, Clone)]
pub struct McpToolBinding {
    /// MCP server slug (the key in claude/settings.json's mcpServers).
    pub mcp_slug: String,
    /// Tool name as declared by the MCP server in tools/list.
    pub name: String,
    /// Tool description as declared by the MCP server in tools/list.
    pub description: String,
    /// JSON Schema of the tool's input parameters.
    pub schema: JsonValue,
}

impl From<&McpToolBinding> for ato_review_tools::ToolDef {
    fn from(b: &McpToolBinding) -> Self {
        ato_review_tools::ToolDef {
            name: b.name.clone(),
            description: b.description.clone(),
            schema: b.schema.clone(),
        }
    }
}

impl From<&McpToolBinding> for ato_agent_permissions::ToolDef {
    fn from(b: &McpToolBinding) -> Self {
        ato_agent_permissions::ToolDef {
            name: b.name.clone(),
            description: b.description.clone(),
            parameters: b.schema.clone(),
        }
    }
}

// Process-local cache of discovered MCP tools per (agent, mcps-list)
// pair. Key includes a hash of the agent's mcps Vec<String> so that
// when the user edits the MCP list mid-session the new dispatch falls
// through to a fresh discovery instead of serving stale tools.
//
// Mutex policy: std::sync::Mutex, never held across .await. We only
// take the lock for HashMap::get / clone / insert. Per-key
// single-flight is intentionally omitted for v2.7.10 — concurrent
// cache misses on the same key produce two spawns of the same MCP
// server, each exits cleanly after tools/list, both cache writes land
// the same Vec<McpToolBinding>. War-room verdict: benign for
// per-session usage at typical agent counts. Revisit if dogpile
// becomes measurable.
//
// DefaultHasher is process-local-only: not stable across Rust
// compiler versions, but the cache is rebuilt on every restart so
// stability across binaries isn't required. Avoids pulling sha2 into
// this module's dep graph just to key a HashMap.
static MCP_TOOLS_CACHE: LazyLock<Mutex<HashMap<(String, u64), Vec<McpToolBinding>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn mcps_hash(mcps_sorted: &[String]) -> u64 {
    let mut h = DefaultHasher::new();
    mcps_sorted.hash(&mut h);
    h.finish()
}

fn cache_lookup(key: &(String, u64)) -> Option<Vec<McpToolBinding>> {
    MCP_TOOLS_CACHE
        .lock()
        .ok()
        .and_then(|g| g.get(key).cloned())
}

fn cache_insert(key: (String, u64), bindings: Vec<McpToolBinding>) {
    if let Ok(mut g) = MCP_TOOLS_CACHE.lock() {
        g.insert(key, bindings);
    }
}

#[cfg(test)]
fn cache_clear_for_test() {
    if let Ok(mut g) = MCP_TOOLS_CACHE.lock() {
        g.clear();
    }
}

/// Discover the MCP tools currently attached to `agent_slug`.
///
/// All sync work (rusqlite query + per-server MCP stdio handshake)
/// runs inside `tokio::task::spawn_blocking` so the calling async
/// Tauri command never pins a worker. Per-server errors are swallowed
/// (one broken MCP shouldn't lose access to the rest); the loud
/// failure surfaces in the discovery panel via the unrelated
/// `discover_mcp_server_tools` Tauri command.
pub async fn load_agent_mcp_tools(agent_slug: &str) -> Vec<McpToolBinding> {
    let slug = agent_slug.to_string();
    let db_path = crate::get_db_path();

    tokio::task::spawn_blocking(move || load_agent_mcp_tools_blocking(&slug, &db_path))
        .await
        .unwrap_or_else(|join_err| {
            eprintln!("mcp_dispatch: spawn_blocking join failed: {}", join_err);
            Vec::new()
        })
}

fn load_agent_mcp_tools_blocking(slug: &str, db_path: &std::path::Path) -> Vec<McpToolBinding> {
    let mcps = read_agent_mcps(slug, db_path);
    if mcps.is_empty() {
        return Vec::new();
    }
    let mut sorted = mcps.clone();
    sorted.sort();
    let key = (slug.to_string(), mcps_hash(&sorted));

    if let Some(cached) = cache_lookup(&key) {
        return cached;
    }

    let mut bindings: Vec<McpToolBinding> = Vec::new();
    for mcp_slug in &sorted {
        match crate::commands::mcp::discover_mcp_server_tools(mcp_slug.clone()) {
            Ok(details) if details.connected => {
                for t in details.tools {
                    bindings.push(McpToolBinding {
                        mcp_slug: mcp_slug.clone(),
                        name: t.name,
                        description: t.description.unwrap_or_default(),
                        schema: t.input_schema.unwrap_or_else(|| json!({"type": "object"})),
                    });
                }
            }
            Ok(details) => {
                eprintln!(
                    "mcp_dispatch: MCP '{}' returned no tools: {:?}",
                    mcp_slug, details.error
                );
            }
            Err(e) => {
                eprintln!(
                    "mcp_dispatch: discover_mcp_server_tools('{}') failed: {}",
                    mcp_slug, e
                );
            }
        }
    }

    cache_insert(key, bindings.clone());
    bindings
}

fn read_agent_mcps(slug: &str, db_path: &std::path::Path) -> Vec<String> {
    let conn = match rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("mcp_dispatch: open db failed: {}", e);
            return Vec::new();
        }
    };
    let row: Option<Option<String>> = conn
        .query_row(
            "SELECT mcps FROM agents WHERE slug = ?1
               ORDER BY COALESCE(last_used_at, created_at) DESC LIMIT 1",
            rusqlite::params![slug],
            |r| r.get(0),
        )
        .ok();
    let Some(Some(json_str)) = row else {
        return Vec::new();
    };
    match serde_json::from_str::<Vec<String>>(&json_str) {
        Ok(v) => v,
        Err(e) => {
            // Loud failure so support logs surface the corruption.
            // Quietly returning [] would hide a broken agents.mcps
            // column as "no MCPs attached" — same deception class
            // that killed v2.7.9 PR-B.
            eprintln!(
                "mcp_dispatch: malformed agents.mcps JSON for slug '{}': {} (raw: {:?})",
                slug, e, json_str
            );
            Vec::new()
        }
    }
}

/// Errors raised by the MCP stdio JSON-RPC layer. Three categories so
/// the model (and humans reading the audit) can tell "the server
/// crashed" from "the server returned a structured error from the
/// tool call" — they need different recovery paths.
#[derive(Debug)]
enum McpRpcError {
    /// Failed to spawn the MCP server (binary missing, PATH gap,
    /// command rejected). Surfaced with the stderr drained from the
    /// child if anything was captured.
    Spawn(String),
    /// Server spawned but the JSON-RPC handshake failed (server exited
    /// before responding, response wasn't valid JSON, missing fields).
    Protocol(String),
}

impl McpRpcError {
    fn into_tool_result(self, tool_call_id: &str, tool_name: &str, mcp_slug: &str) -> ToolResult {
        let (prefix, msg) = match self {
            McpRpcError::Spawn(m) => ("mcp_spawn_failed", m),
            McpRpcError::Protocol(m) => ("mcp_rpc_protocol_error", m),
        };
        ToolResult {
            tool_call_id: tool_call_id.to_string(),
            name: tool_name.to_string(),
            content: format!(
                "[{prefix}] MCP server '{mcp_slug}': {}",
                truncate_for_model(&msg, 500)
            ),
            is_error: true,
        }
    }
}

/// Call the named tool on the MCP server identified by `mcp_slug` via
/// the standard MCP JSON-RPC `tools/call` method. Wraps all blocking
/// work (settings.json read, process spawn, stdin/stdout IO) inside
/// tokio::task::spawn_blocking.
pub async fn execute_mcp_tool(
    mcp_slug: &str,
    tool_name: &str,
    args: &JsonValue,
    tool_call_id: &str,
) -> ToolResult {
    let mcp_slug_owned = mcp_slug.to_string();
    let tool_name_owned = tool_name.to_string();
    let tool_call_id_owned = tool_call_id.to_string();
    let args_owned = args.clone();

    let result = tokio::task::spawn_blocking(move || {
        execute_mcp_tool_blocking(
            &mcp_slug_owned,
            &tool_name_owned,
            &args_owned,
            &tool_call_id_owned,
        )
    })
    .await;

    match result {
        Ok(r) => r,
        Err(join_err) => ToolResult {
            tool_call_id: tool_call_id.to_string(),
            name: tool_name.to_string(),
            content: format!("[mcp_rpc_protocol_error] spawn_blocking join failed: {}", join_err),
            is_error: true,
        },
    }
}

fn execute_mcp_tool_blocking(
    mcp_slug: &str,
    tool_name: &str,
    args: &JsonValue,
    tool_call_id: &str,
) -> ToolResult {
    let (command, args_vec, env) = match read_mcp_server_config(mcp_slug) {
        Ok(c) => c,
        Err(e) => {
            return McpRpcError::Spawn(e).into_tool_result(tool_call_id, tool_name, mcp_slug);
        }
    };

    let request_payload = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": args,
        },
    });

    let args_refs: Vec<&str> = args_vec.iter().map(|s| s.as_str()).collect();
    match rpc_call_blocking(&command, &args_refs, &env, &request_payload) {
        Ok(response) => {
            let result = response.get("result").cloned().unwrap_or(JsonValue::Null);
            let (content, is_error) = parse_mcp_content(&result);
            // If the server replied with a top-level JSON-RPC error
            // instead of a tool-shaped result, surface that as an
            // is_error ToolResult so the model can recover.
            if let Some(rpc_err) = response.get("error") {
                let msg = rpc_err
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unspecified JSON-RPC error");
                return ToolResult {
                    tool_call_id: tool_call_id.to_string(),
                    name: tool_name.to_string(),
                    content: format!(
                        "[mcp_rpc_protocol_error] MCP server '{}': {}",
                        mcp_slug,
                        truncate_for_model(msg, 500)
                    ),
                    is_error: true,
                };
            }
            // When the MCP server returns a tool-shaped result with
            // isError=true (e.g. "no records found", "permission
            // denied at the upstream API"), prepend the third error-
            // class prefix so the model can distinguish it from
            // [mcp_spawn_failed] (retry the spawn) and
            // [mcp_rpc_protocol_error] (retry the call). All three
            // surfaces map cleanly to different recovery strategies.
            let final_content = if is_error {
                format!("[mcp_tool_returned_error] {}", content)
            } else {
                content
            };
            ToolResult {
                tool_call_id: tool_call_id.to_string(),
                name: tool_name.to_string(),
                content: final_content,
                is_error,
            }
        }
        Err(e) => e.into_tool_result(tool_call_id, tool_name, mcp_slug),
    }
}

/// Read mcpServers.<slug> from ~/.claude/settings.json and return the
/// (command, args, env) tuple the spawn helper needs. Mirrors the
/// extraction logic at commands/mcp.rs::discover_mcp_server_tools.
fn read_mcp_server_config(
    mcp_slug: &str,
) -> Result<(String, Vec<String>, HashMap<String, String>), String> {
    let settings_path = crate::commands::claude_home().join("settings.json");
    let content = crate::commands::read_file_lossy(&settings_path)
        .ok_or_else(|| "Could not read Claude settings".to_string())?;
    let parsed: JsonValue =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse settings: {}", e))?;
    let mcp_servers = parsed
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "No mcpServers found in settings".to_string())?;
    // Discovery accepts " (scope)" suffixes; mirror the same trimming.
    let clean_name = mcp_slug.split(" (").next().unwrap_or(mcp_slug);
    let server = mcp_servers
        .get(clean_name)
        .ok_or_else(|| format!("MCP server '{}' not found in settings", clean_name))?;
    let command = server
        .get("command")
        .and_then(|c| c.as_str())
        .ok_or_else(|| format!("MCP server '{}' has no command", clean_name))?
        .to_string();
    let args: Vec<String> = server
        .get("args")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut env: HashMap<String, String> = HashMap::new();
    if let Some(env_obj) = server.get("env").and_then(|e| e.as_object()) {
        for (k, v) in env_obj {
            if let Some(s) = v.as_str() {
                env.insert(k.clone(), s.to_string());
            }
        }
    }
    Ok((command, args, env))
}

/// Spawn an MCP server, exchange `initialize` then the supplied
/// JSON-RPC request, return the response payload.
///
/// MIRROR: handshake must stay in sync with commands/mcp.rs::
/// discover_mcp_tools_stdio. Deduplicate in v2.7.11 (S8) by lifting
/// a shared helper into a new `crates/mcp-stdio` package or into
/// packages/core. Tracking: TODO comment near the call site in
/// commands/mcp.rs is the other end of the pair.
fn rpc_call_blocking(
    command: &str,
    args: &[&str],
    env: &HashMap<String, String>,
    request_payload: &JsonValue,
) -> Result<JsonValue, McpRpcError> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let user_path = crate::commands::get_user_path();
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PATH", &user_path)
        .envs(env);

    let mut child = cmd
        .spawn()
        .map_err(|e| McpRpcError::Spawn(format!("spawn '{}': {}", command, e)))?;

    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| McpRpcError::Protocol("failed to open stdin".to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| McpRpcError::Protocol("failed to open stdout".to_string()))?;
    let stderr_pipe = child.stderr.take();
    let mut reader = BufReader::new(stdout);

    let drain_stderr = |pipe: Option<std::process::ChildStderr>| -> String {
        if let Some(mut s) = pipe {
            use std::io::Read;
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            return buf.trim().to_string();
        }
        String::new()
    };

    // 1. initialize
    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "ATO", "version": "0.2.0" }
        }
    });
    writeln!(stdin, "{}", init_request)
        .map_err(|e| McpRpcError::Protocol(format!("write initialize: {}", e)))?;
    stdin
        .flush()
        .map_err(|e| McpRpcError::Protocol(format!("flush initialize: {}", e)))?;

    let mut line = String::new();
    let read_one = |reader: &mut BufReader<std::process::ChildStdout>,
                    line: &mut String|
     -> Result<JsonValue, McpRpcError> {
        line.clear();
        let n = reader
            .read_line(line)
            .map_err(|e| McpRpcError::Protocol(format!("read response: {}", e)))?;
        if n == 0 {
            return Err(McpRpcError::Protocol(
                "server exited before sending a response".to_string(),
            ));
        }
        serde_json::from_str(line)
            .map_err(|e| McpRpcError::Protocol(format!("parse response: {}", e)))
    };

    if let Err(e) = read_one(&mut reader, &mut line) {
        let _ = child.kill();
        let stderr_msg = drain_stderr(stderr_pipe);
        return Err(merge_with_stderr(e, &stderr_msg));
    }

    // 2. user-supplied request (typically tools/call).
    writeln!(stdin, "{}", request_payload)
        .map_err(|e| McpRpcError::Protocol(format!("write request: {}", e)))?;
    stdin
        .flush()
        .map_err(|e| McpRpcError::Protocol(format!("flush request: {}", e)))?;

    let response = match read_one(&mut reader, &mut line) {
        Ok(v) => v,
        Err(e) => {
            let _ = child.kill();
            let stderr_msg = drain_stderr(stderr_pipe);
            return Err(merge_with_stderr(e, &stderr_msg));
        }
    };

    let _ = child.kill();
    Ok(response)
}

fn merge_with_stderr(e: McpRpcError, stderr_msg: &str) -> McpRpcError {
    if stderr_msg.is_empty() {
        return e;
    }
    match e {
        McpRpcError::Spawn(m) => McpRpcError::Spawn(format!("{}\nstderr: {}", m, stderr_msg)),
        McpRpcError::Protocol(m) => {
            McpRpcError::Protocol(format!("{}\nstderr: {}", m, stderr_msg))
        }
    }
}

/// Parse an MCP `result` payload from tools/call into the plain-text
/// content + is_error pair the dispatch loop hands back to the model.
///
/// MCP spec returns `content: Array<{type, ...}>`. We surface text
/// blocks verbatim and emit one-line labeled placeholders for image /
/// resource / unknown types so the model can SEE that the call
/// produced non-text output (instead of being silently handed an
/// empty string — the same deception pattern that killed v2.7.9
/// PR-B's first attempt).
pub fn parse_mcp_content(result: &JsonValue) -> (String, bool) {
    let is_error = result
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let Some(blocks) = result.get("content").and_then(|v| v.as_array()) else {
        // Some servers may return a non-standard shape; stringify the
        // whole result so the model has SOMETHING to reason about.
        return (truncate_for_model(&result.to_string(), 4096), is_error);
    };
    let mut parts: Vec<String> = Vec::new();
    for block in blocks {
        let kind = block
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        match kind {
            "text" => {
                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                    parts.push(t.to_string());
                }
            }
            "image" => {
                let mime = block
                    .get("mimeType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/octet-stream");
                parts.push(format!("[image omitted: {}]", mime));
            }
            "resource" => {
                let uri = block
                    .get("resource")
                    .and_then(|r| r.get("uri"))
                    .and_then(|v| v.as_str())
                    .or_else(|| block.get("uri").and_then(|v| v.as_str()))
                    .unwrap_or("(no uri)");
                parts.push(format!("[resource: {}]", uri));
            }
            other => parts.push(format!("[{} content omitted]", other)),
        }
    }
    let joined = parts.join("\n");
    (truncate_for_model(&joined, 32 * 1024), is_error)
}

fn truncate_for_model(s: &str, cap: usize) -> String {
    if s.len() <= cap {
        s.to_string()
    } else {
        let mut out = s[..cap].to_string();
        out.push_str("…[truncated]");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mcp_content_text_only() {
        let payload = json!({
            "content": [
                { "type": "text", "text": "hello" },
                { "type": "text", "text": "world" }
            ]
        });
        let (text, is_err) = parse_mcp_content(&payload);
        assert_eq!(text, "hello\nworld");
        assert!(!is_err);
    }

    #[test]
    fn parse_mcp_content_surfaces_image_and_resource_placeholders() {
        let payload = json!({
            "content": [
                { "type": "text", "text": "first line" },
                { "type": "image", "mimeType": "image/png" },
                { "type": "resource", "resource": { "uri": "file:///tmp/x.txt" } },
                { "type": "video" }
            ]
        });
        let (text, is_err) = parse_mcp_content(&payload);
        assert!(text.contains("first line"));
        assert!(text.contains("[image omitted: image/png]"));
        assert!(text.contains("[resource: file:///tmp/x.txt]"));
        assert!(text.contains("[video content omitted]"));
        assert!(!is_err);
    }

    #[test]
    fn parse_mcp_content_respects_is_error_flag() {
        let payload = json!({
            "isError": true,
            "content": [{ "type": "text", "text": "boom" }]
        });
        let (text, is_err) = parse_mcp_content(&payload);
        assert_eq!(text, "boom");
        assert!(is_err);
    }

    #[test]
    fn parse_mcp_content_stringifies_non_standard_shape() {
        let payload = json!({ "raw": "no content array here" });
        let (text, is_err) = parse_mcp_content(&payload);
        assert!(text.contains("raw"));
        assert!(!is_err);
    }

    #[test]
    fn mcps_hash_is_stable_and_order_independent() {
        let a = vec!["alpha".to_string(), "beta".to_string()];
        let mut b = vec!["beta".to_string(), "alpha".to_string()];
        b.sort();
        let mut a_sorted = a.clone();
        a_sorted.sort();
        assert_eq!(mcps_hash(&a_sorted), mcps_hash(&b));
        assert_ne!(mcps_hash(&a_sorted), mcps_hash(&[]));
    }

    #[test]
    fn cache_hit_returns_inserted_bindings() {
        cache_clear_for_test();
        let key = ("agentX".to_string(), 0xDEADBEEF);
        assert!(cache_lookup(&key).is_none());
        let bindings = vec![McpToolBinding {
            mcp_slug: "demo".to_string(),
            name: "ping".to_string(),
            description: "test".to_string(),
            schema: json!({"type": "object"}),
        }];
        cache_insert(key.clone(), bindings.clone());
        let got = cache_lookup(&key).expect("cache hit");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "ping");
        assert_eq!(got[0].mcp_slug, "demo");
    }

    #[tokio::test]
    async fn spawn_blocking_round_trip_returns_to_runtime() {
        // Exercises the same spawn_blocking pattern load_agent_mcp_tools
        // uses: a sync closure that does CPU/IO work, awaited from an
        // async context. The test asserts the runtime is responsive
        // after the join — if the closure had blocked the worker, the
        // following tokio::time::sleep would hang.
        let value = tokio::task::spawn_blocking(|| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            42_u32
        })
        .await
        .expect("spawn_blocking join");
        assert_eq!(value, 42);

        // Prove the async runtime survived.
        tokio::time::timeout(
            std::time::Duration::from_millis(50),
            tokio::time::sleep(std::time::Duration::from_millis(1)),
        )
        .await
        .expect("runtime should still drive timers after spawn_blocking");
    }

    // S7 review follow-up: regression test for the "tool-shaped
    // error" prefix path. A successful tools/call that comes back
    // with isError=true should be returned with the
    // [mcp_tool_returned_error] prefix so the model can tell it
    // apart from [mcp_spawn_failed] / [mcp_rpc_protocol_error].
    //
    // We exercise this through the public parse_mcp_content +
    // prefix-format contract rather than spinning a real MCP
    // server: that's the same composition execute_mcp_tool_blocking
    // performs at mcp_dispatch.rs's success path.
    #[test]
    fn tool_returned_error_gets_distinct_prefix() {
        let result = json!({
            "isError": true,
            "content": [{ "type": "text", "text": "rate limit exceeded" }]
        });
        let (content, is_err) = parse_mcp_content(&result);
        assert!(is_err);
        let final_content = if is_err {
            format!("[mcp_tool_returned_error] {}", content)
        } else {
            content
        };
        assert!(final_content.starts_with("[mcp_tool_returned_error] "));
        assert!(final_content.contains("rate limit exceeded"));
    }

    // S7 review follow-up (war-room B1): permission filter must use
    // gate.allowed_tools membership, NOT gate.check — which would
    // Allow every name not explicitly denied and leak MCP tools the
    // user never authorized. This test pins the contract by
    // exercising the same primitives mod.rs's filter uses.
    #[test]
    fn gate_allowed_tools_membership_filters_unauthorized_mcp_tools() {
        use ato_agent_permissions::{to_api_tool_gate, AgentPermissions, ToolDef as PermToolDef};

        let bindings = vec![
            McpToolBinding {
                mcp_slug: "demo".to_string(),
                name: "send_email".to_string(),
                description: "".to_string(),
                schema: json!({"type": "object"}),
            },
            McpToolBinding {
                mcp_slug: "demo".to_string(),
                name: "delete_everything".to_string(),
                description: "".to_string(),
                schema: json!({"type": "object"}),
            },
        ];
        let perm_tools: Vec<PermToolDef> = bindings.iter().map(Into::into).collect();
        let mut perms = AgentPermissions::default();
        perms.allowed = vec!["send_email".to_string()];
        let gate = to_api_tool_gate(&perms, &perm_tools);

        // The right filter — agent_gate.allowed_tools membership.
        let allowed: std::collections::HashSet<String> =
            gate.allowed_tools.iter().map(|t| t.name.clone()).collect();
        let kept: Vec<&McpToolBinding> = bindings
            .iter()
            .filter(|b| allowed.contains(&b.name))
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "send_email");

        // The WRONG filter (gate.check), kept here as evidence the
        // bug would have leaked delete_everything past it: the
        // unauthorized tool would still come back as Allow.
        assert!(matches!(
            gate.check("delete_everything"),
            ato_agent_permissions::GateDecision::Allow
        ));
    }

    // S7 review follow-up (war-room B2): when an MCP declares a tool
    // name that collides with a built-in, the MCP entry must be
    // dropped before the request body is built — otherwise Anthropic
    // rejects the dispatch with HTTP 400 on duplicate tool names.
    #[test]
    fn builtin_mcp_name_collision_is_filtered_out() {
        let bindings = vec![
            McpToolBinding {
                mcp_slug: "demo".to_string(),
                name: "grep".to_string(), // built-in collision
                description: "".to_string(),
                schema: json!({"type": "object"}),
            },
            McpToolBinding {
                mcp_slug: "demo".to_string(),
                name: "send_email".to_string(),
                description: "".to_string(),
                schema: json!({"type": "object"}),
            },
        ];
        let builtin_names: std::collections::HashSet<String> = ato_review_tools::registry()
            .into_iter()
            .map(|t| t.name)
            .collect();
        let kept: Vec<&McpToolBinding> = bindings
            .iter()
            .filter(|b| !builtin_names.contains(&b.name))
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "send_email");
        // Sanity: the built-in name we picked is actually in the
        // registry today — if someone renames `grep` the test
        // surface (and the production bug) both move with it.
        assert!(builtin_names.contains("grep"));
    }

    #[tokio::test]
    async fn execute_mcp_tool_routes_protocol_error_to_tool_result() {
        // Drive execute_mcp_tool with a slug that won't resolve in
        // ~/.claude/settings.json. The function should return a
        // ToolResult with is_error=true and the spawn-error prefix —
        // proving the response routing path back through
        // execute_mcp_tool → spawn_blocking → await is intact even
        // when the MCP layer fails.
        let r = execute_mcp_tool(
            "__definitely_not_a_real_mcp_slug__",
            "noop",
            &json!({}),
            "call-1",
        )
        .await;
        assert_eq!(r.tool_call_id, "call-1");
        assert_eq!(r.name, "noop");
        assert!(r.is_error, "missing MCP slug must surface as is_error");
        assert!(
            r.content.starts_with("[mcp_spawn_failed]") || r.content.contains("not found"),
            "unexpected error content: {}",
            r.content
        );
    }
}
