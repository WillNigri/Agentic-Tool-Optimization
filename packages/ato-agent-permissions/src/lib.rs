// ato-agent-permissions — translate ATO's agent.permissions DSL into
// per-runtime enforcement.
//
// The CreateAgentWizard produces a structured Permissions object with
// allowed / requireApproval / denied lists of semantic labels. The
// desktop persists those as a JSON-encoded tagged-string array
// (allow:X / approve:Y / deny:Z) onto the `agents.permissions` column.
//
// Every dispatch path needs the same logical translation: "given the
// agent's permissions, what runtime-native flags or tool-gate config
// should this dispatch carry?" This crate is that translator.
//
// Pure functions only — no I/O, no DB reads, no env access. Inputs:
// AgentPermissions. Outputs: per-runtime flag/gate structs. Caller
// is responsible for loading permissions and spawning processes.
//
// War-room provenance: docs/audits/agent-permissions-plumb-through-
// 2026-05-20.md (codex+claude review, 2026-05-20).

use serde::{Deserialize, Serialize};
use serde_json::json;

/// The DSL the wizard produces and the `agents.permissions` column
/// persists. `summary` is intentionally not carried here — it lives
/// only on the wizard's structured object and is discarded at flatten
/// time today; the audit's Finding 1 documents that lossy round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AgentPermissions {
    pub allowed: Vec<String>,
    pub require_approval: Vec<String>,
    pub denied: Vec<String>,
}

impl AgentPermissions {
    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty() && self.require_approval.is_empty() && self.denied.is_empty()
    }
}

/// Parse the tagged-string array format from GuidedPath.tsx:174-179
/// (`["allow:Read", "approve:send_emails", "deny:Bash(rm:*)"]`)
/// back into structured AgentPermissions.
///
/// Unknown / untagged entries are ignored — earlier wizard versions
/// emitted bare capability strings without prefixes; treating those as
/// "no policy" is safer than guessing the bucket.
///
/// Round-trip note: `summary` is not recoverable from this format.
pub fn parse_permissions_column(json: &str) -> AgentPermissions {
    let arr: Vec<String> = match serde_json::from_str::<Vec<String>>(json) {
        Ok(v) => v,
        Err(_) => return AgentPermissions::default(),
    };
    let mut out = AgentPermissions::default();
    for entry in arr {
        if let Some(rest) = entry.strip_prefix("allow:") {
            out.allowed.push(rest.to_string());
        } else if let Some(rest) = entry.strip_prefix("approve:") {
            out.require_approval.push(rest.to_string());
        } else if let Some(rest) = entry.strip_prefix("deny:") {
            out.denied.push(rest.to_string());
        }
    }
    out
}

/// Serialize back to the tagged-string format. Inverse of
/// parse_permissions_column. `summary` is not part of this round-trip
/// (see Finding 1 in the audit doc).
pub fn serialize_permissions_column(p: &AgentPermissions) -> String {
    let mut arr: Vec<String> = Vec::new();
    for a in &p.allowed {
        arr.push(format!("allow:{}", a));
    }
    for a in &p.require_approval {
        arr.push(format!("approve:{}", a));
    }
    for a in &p.denied {
        arr.push(format!("deny:{}", a));
    }
    serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string())
}

// ─── Claude Code ────────────────────────────────────────────────────

/// Claude Code's native enforcement surface: `--allowedTools` CLI
/// flag plus `~/.claude/settings.local.json` permissions object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClaudeFlags {
    /// Space-separated tool patterns for the `--allowedTools` CLI flag.
    pub allowed_tools: String,
    /// JSON for `~/.claude/settings.local.json`'s `permissions`
    /// object (`{ allow: [...], deny: [...], ask: [...] }`).
    pub settings_local: serde_json::Value,
}

/// The current hardcoded sibling-runtime allowlist at
/// commands/mod.rs:854 + dispatch.rs:541. Backward-compat default when
/// the agent has no permissions — preserves today's behaviour for
/// pre-v2.7.8 agents and for slug-less dispatches.
pub const CLAUDE_DEFAULT_ALLOWED_TOOLS: &str =
    "Bash(ato:*) Bash(gemini:*) Bash(codex:*) Bash(openclaw:*) Bash(hermes:*) Bash(minimax:*)";

pub fn to_claude(p: &AgentPermissions) -> ClaudeFlags {
    if p.is_empty() {
        return ClaudeFlags {
            allowed_tools: CLAUDE_DEFAULT_ALLOWED_TOOLS.to_string(),
            settings_local: json!({ "allow": [], "deny": [], "ask": [] }),
        };
    }
    ClaudeFlags {
        allowed_tools: p.allowed.join(" "),
        settings_local: json!({
            "allow": p.allowed,
            "deny": p.denied,
            "ask": p.require_approval,
        }),
    }
}

// ─── Codex ──────────────────────────────────────────────────────────

/// Codex `exec` enforcement surface: `--sandbox` mode (3-valued enum)
/// plus `-c approval_policy=...`. Anything finer than the 3 modes is
/// not enforceable; we record it as advisory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CodexFlags {
    /// One of: "read-only", "workspace-write", "danger-full-access".
    pub sandbox: &'static str,
    /// One of: "never", "on-request", "untrusted".
    pub approval_policy: &'static str,
    /// Labels we couldn't enforce structurally. UI surfaces these as
    /// "policy is advisory on codex." Empty when the spec maps cleanly.
    pub advisory_only: Vec<String>,
}

/// Today's hardcoded codex flags at commands/mod.rs:889-894 + CLI
/// dispatch.rs:578-581. Backward-compat default.
pub const CODEX_DEFAULT_SANDBOX: &str = "workspace-write";
pub const CODEX_DEFAULT_APPROVAL: &str = "never";

pub fn to_codex(p: &AgentPermissions) -> CodexFlags {
    if p.is_empty() {
        return CodexFlags {
            sandbox: CODEX_DEFAULT_SANDBOX,
            approval_policy: CODEX_DEFAULT_APPROVAL,
            advisory_only: Vec::new(),
        };
    }

    // Any non-empty denied or requireApproval forces demotion to
    // read-only — codex can't enforce per-tool denials, so we have to
    // remove the broader capability to be safe. The denied labels
    // become advisory so the UI can surface "blocked: rm" if codex
    // attempts it (which it can't, because read-only blocks all writes).
    let demote = !p.denied.is_empty() || !p.require_approval.is_empty();
    let mut advisory_only: Vec<String> = Vec::new();
    advisory_only.extend(p.denied.iter().cloned());
    advisory_only.extend(p.require_approval.iter().cloned());

    CodexFlags {
        sandbox: if demote { "read-only" } else { "workspace-write" },
        approval_policy: "never",
        advisory_only,
    }
}

// ─── Gemini CLI ─────────────────────────────────────────────────────

/// Gemini CLI's enforcement is binary: `--yolo` for full access, or
/// default (on-request approvals that hang headlessly).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeminiFlags {
    /// True iff the agent permissions reduce to "everything allowed."
    pub yolo: bool,
    /// When the agent has a non-empty denied / requireApproval list,
    /// gemini can't enforce it. The dispatch path should refuse with
    /// this error message rather than silently dropping the policy.
    pub error: Option<String>,
}

pub fn to_gemini(p: &AgentPermissions) -> GeminiFlags {
    if p.is_empty() {
        // Backward-compat: today's gemini dispatch passes no agentic
        // flags at all. Reflect that with yolo=false, no error — the
        // dispatch path's existing CLI-not-found / headless-hang
        // failure modes remain. Future PR may default this to yolo=true
        // if we decide ATO dispatching IS the authorization for gemini
        // too.
        return GeminiFlags { yolo: false, error: None };
    }
    if !p.denied.is_empty() || !p.require_approval.is_empty() {
        return GeminiFlags {
            yolo: false,
            error: Some(
                "Gemini CLI does not support fine-grained permissions. \
                 Switch this agent's runtime to `google` API provider, \
                 or broaden permissions to remove the deny/approve list."
                    .to_string(),
            ),
        };
    }
    GeminiFlags { yolo: true, error: None }
}

// ─── OpenClaw / Hermes (pass-through) ───────────────────────────────

/// OpenClaw and Hermes enforce their own permissions surface
/// (SOUL.md / TOOLS.md). ATO records the policy as metadata so the
/// runtime's loader can read it back, but does not gate the spawn.
pub fn to_openclaw(p: &AgentPermissions) -> serde_json::Value {
    json!({
        "passthrough": true,
        "allowed": p.allowed,
        "require_approval": p.require_approval,
        "denied": p.denied,
        "notice": "OpenClaw enforces these via TOOLS.md; ATO does not gate spawn.",
    })
}

pub fn to_hermes(p: &AgentPermissions) -> serde_json::Value {
    json!({
        "passthrough": true,
        "allowed": p.allowed,
        "require_approval": p.require_approval,
        "denied": p.denied,
        "notice": "Hermes enforces these via SOUL.md; ATO does not gate spawn.",
    })
}

// ─── API providers (tool-call gate) ─────────────────────────────────

/// One tool definition emitted into the API-provider request body.
/// Per-provider HTTP shapes (OpenAI vs Anthropic vs Gemini) are built
/// from this normalized struct in api_dispatch's dispatch_with_tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's parameters.
    pub parameters: serde_json::Value,
}

/// The gate the interceptor consults on every `tool_call` parsed from
/// a provider response. Decides allow / pause-for-approval / deny.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolGate {
    /// Tools the agent's permissions explicitly allow. Emitted into
    /// the request body's `tools` field so the model only sees these.
    pub allowed_tools: Vec<ToolDef>,
    /// Tool names matching this list trigger a pending_approval row.
    pub approval_required: Vec<String>,
    /// Tool names matching this list are refused before execution;
    /// the loop appends a `tool` role error and re-dispatches.
    pub denied: Vec<String>,
}

/// The built-in tool catalogue for API-provider dispatches. Read-class
/// only for the MVP (PR-3). PR-5 adds write_file + shell with approval
/// UI; PR-3a layers MCP-declared tools on top via a second arg.
pub fn builtin_read_tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "read_file".to_string(),
            description: "Read a file from the workspace. Returns the file contents as text. Cannot read files outside the workspace root.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path relative to the workspace root." },
                    "offset": { "type": "integer", "description": "Optional 0-indexed start line." },
                    "limit": { "type": "integer", "description": "Optional max line count." }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "grep".to_string(),
            description: "Search the workspace for a regex pattern. Returns matching file paths and line numbers.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern (ripgrep syntax)." },
                    "path": { "type": "string", "description": "Optional path scope, defaults to workspace root." }
                },
                "required": ["pattern"]
            }),
        },
        ToolDef {
            name: "list_directory".to_string(),
            description: "List the contents of a directory in the workspace.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path relative to workspace root." }
                },
                "required": ["path"]
            }),
        },
    ]
}

/// True iff a wizard label refers to a read-class capability. The
/// label space is freeform LLM output; we match a known set of
/// case-insensitive synonyms. Anything not matched here is treated
/// as a domain verb (e.g. "send_emails") that only matches by exact
/// MCP tool name in a later layer.
fn label_is_read_class(label: &str) -> bool {
    matches!(
        label.to_ascii_lowercase().as_str(),
        "read" | "read_file" | "grep" | "glob" | "list_directory" | "ls" | "web_fetch" | "webfetch"
    )
}

/// Match a wizard label against a built-in tool name, case- and
/// synonym-aware. "Read" matches "read_file"; "Bash" matches "shell".
fn label_matches_tool(label: &str, tool_name: &str) -> bool {
    let l = label.to_ascii_lowercase();
    let t = tool_name.to_ascii_lowercase();
    if l == t { return true; }
    match (l.as_str(), t.as_str()) {
        ("read", "read_file") | ("read_file", "read") => true,
        ("write", "write_file") | ("write_file", "write") => true,
        ("glob", "list_directory") | ("list_directory", "glob") | ("ls", "list_directory") => true,
        ("bash", "shell") | ("shell", "bash") => true,
        ("webfetch", "web_fetch") | ("web_fetch", "webfetch") | ("fetch", "web_fetch") => true,
        _ => false,
    }
}

/// Build the tool gate from an agent's permissions. The `mcp_tools`
/// arg lets PR-3a layer MCP-declared tools on top of the built-in
/// catalogue; pass `&[]` for the PR-3 MVP that ships built-ins only.
pub fn to_api_tool_gate(p: &AgentPermissions, mcp_tools: &[ToolDef]) -> ToolGate {
    // Start with built-ins + MCP tools.
    let mut catalogue: Vec<ToolDef> = builtin_read_tools();
    catalogue.extend(mcp_tools.iter().cloned());

    // If permissions are empty, the gate is empty — the dispatch path
    // falls through to "no tools field in the request body," matching
    // today's text-only behaviour. Backward-compat invariant.
    if p.is_empty() {
        return ToolGate {
            allowed_tools: Vec::new(),
            approval_required: Vec::new(),
            denied: Vec::new(),
        };
    }

    // A tool ends up in allowed_tools iff:
    //   - some label in `allowed` matches its name, AND
    //   - no label in `denied` matches its name.
    // requireApproval labels DO NOT remove the tool from the catalogue
    // — the model still sees the tool definition; the interceptor
    // pauses on the call. Without that, the model can't even attempt
    // a tool it might be approved to use.
    let allowed_tools: Vec<ToolDef> = catalogue
        .into_iter()
        .filter(|t| {
            let in_allow = p.allowed.iter().any(|l| label_matches_tool(l, &t.name));
            let in_deny = p.denied.iter().any(|l| label_matches_tool(l, &t.name));
            in_allow && !in_deny
        })
        .collect();

    ToolGate {
        allowed_tools,
        approval_required: p.require_approval.clone(),
        denied: p.denied.clone(),
    }
}

/// Convenience: check a tool call against the gate. Returns the gate
/// decision the interceptor should act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Allow,
    RequireApproval,
    Deny,
}

impl ToolGate {
    pub fn check(&self, tool_name: &str) -> GateDecision {
        if self.denied.iter().any(|l| label_matches_tool(l, tool_name)) {
            return GateDecision::Deny;
        }
        if self.approval_required.iter().any(|l| label_matches_tool(l, tool_name)) {
            return GateDecision::RequireApproval;
        }
        GateDecision::Allow
    }
}

// Silence dead-code warnings for the read-class helper while PR-3
// hasn't wired the consumer yet. Remove when api_dispatch reads it.
#[allow(dead_code)]
fn _label_is_read_class_used(s: &str) -> bool {
    label_is_read_class(s)
}

// ─── Golden tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Test 1 — Empty permissions → backward-compat defaults.
    // Pins today's hardcoded output at commands/mod.rs:854 (claude),
    // :889 (codex), and :955 (gemini, no agentic flags).
    #[test]
    fn test_1_empty_permissions_backward_compat() {
        let p = AgentPermissions::default();

        let claude = to_claude(&p);
        assert_eq!(claude.allowed_tools, CLAUDE_DEFAULT_ALLOWED_TOOLS);

        let codex = to_codex(&p);
        assert_eq!(codex.sandbox, "workspace-write");
        assert_eq!(codex.approval_policy, "never");
        assert!(codex.advisory_only.is_empty());

        let gemini = to_gemini(&p);
        assert!(!gemini.yolo);
        assert!(gemini.error.is_none());

        let gate = to_api_tool_gate(&p, &[]);
        assert!(gate.allowed_tools.is_empty());
        assert!(gate.denied.is_empty());
        assert!(gate.approval_required.is_empty());
    }

    // Test 2 — denied: ["Bash(rm:*)"] across runtimes.
    // - Claude: omits from --allowedTools, adds to settings_local.deny.
    // - Codex: demotes sandbox to read-only, records advisory.
    // - Gemini: yolo=false, error explains the structural limit.
    #[test]
    fn test_2_denied_bash_rm_cross_runtime() {
        let p = AgentPermissions {
            allowed: vec!["Read".into(), "Grep".into()],
            require_approval: vec![],
            denied: vec!["Bash(rm:*)".into()],
        };

        let claude = to_claude(&p);
        assert!(!claude.allowed_tools.contains("Bash(rm:*)"));
        assert_eq!(claude.allowed_tools, "Read Grep");
        let deny = claude.settings_local.get("deny").unwrap().as_array().unwrap();
        assert_eq!(deny.len(), 1);
        assert_eq!(deny[0].as_str().unwrap(), "Bash(rm:*)");

        let codex = to_codex(&p);
        assert_eq!(codex.sandbox, "read-only");
        assert!(codex.advisory_only.contains(&"Bash(rm:*)".to_string()));

        let gemini = to_gemini(&p);
        assert!(!gemini.yolo);
        assert!(gemini.error.is_some());
    }

    // Test 3 — gemini with mixed allow + deny → error.
    #[test]
    fn test_3_gemini_narrow_spec_errors() {
        let p = AgentPermissions {
            allowed: vec!["Read".into(), "Grep".into()],
            require_approval: vec![],
            denied: vec!["Write".into()],
        };
        let gemini = to_gemini(&p);
        assert!(!gemini.yolo);
        assert!(gemini.error.as_deref().unwrap().contains("Gemini CLI does not support"));
    }

    // Test 4 — gemini with broad allow + no constraints → yolo.
    #[test]
    fn test_4_gemini_full_allow_yolo() {
        let p = AgentPermissions {
            allowed: vec!["Read".into(), "Grep".into(), "Write".into(), "Bash".into()],
            require_approval: vec![],
            denied: vec![],
        };
        let gemini = to_gemini(&p);
        assert!(gemini.yolo);
        assert!(gemini.error.is_none());
    }

    // Test 5 — External-kind auto-lock (mirrors commands/mod.rs:5407-5413).
    // Read-class only on claude's --allowedTools; tool gate emits only
    // read-class built-ins.
    #[test]
    fn test_5_external_kind_auto_lock() {
        let p = AgentPermissions {
            allowed: vec!["Read".into(), "Grep".into(), "Glob".into(), "WebFetch".into()],
            require_approval: vec![],
            denied: vec![],
        };

        let claude = to_claude(&p);
        assert_eq!(claude.allowed_tools, "Read Grep Glob WebFetch");

        let gate = to_api_tool_gate(&p, &[]);
        let names: Vec<&str> = gate.allowed_tools.iter().map(|t| t.name.as_str()).collect();
        // Read → read_file, Grep → grep, Glob → list_directory.
        // WebFetch has no built-in in the MVP catalogue (PR-5 adds it).
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"grep"));
        assert!(names.contains(&"list_directory"));
        assert!(!names.contains(&"write_file"));
        assert!(!names.contains(&"shell"));
    }

    // Test 6 — requireApproval semantic label on codex + claude + gate.
    #[test]
    fn test_6_require_approval_semantic_label() {
        let p = AgentPermissions {
            allowed: vec!["Read".into()],
            require_approval: vec!["send_emails".into()],
            denied: vec![],
        };

        let codex = to_codex(&p);
        assert_eq!(codex.sandbox, "read-only");
        assert!(codex.advisory_only.contains(&"send_emails".to_string()));

        let claude = to_claude(&p);
        let ask = claude.settings_local.get("ask").unwrap().as_array().unwrap();
        assert_eq!(ask[0].as_str().unwrap(), "send_emails");
        assert!(!claude.allowed_tools.contains("send_emails"));

        let gate = to_api_tool_gate(&p, &[]);
        assert!(gate.approval_required.contains(&"send_emails".to_string()));
        // Gate.check directly returns RequireApproval for that name.
        assert_eq!(gate.check("send_emails"), GateDecision::RequireApproval);
    }

    // Test 7 — Round-trip parse/serialize lossy by design (no summary).
    #[test]
    fn test_7_round_trip_parse_serialize() {
        let p = AgentPermissions {
            allowed: vec!["Read".into(), "Grep".into()],
            require_approval: vec!["send_emails".into()],
            denied: vec!["Bash(rm:*)".into(), "transfer_funds".into()],
        };
        let serialized = serialize_permissions_column(&p);
        let parsed = parse_permissions_column(&serialized);
        assert_eq!(parsed, p);

        // Real-world fixture from GuidedPath.tsx output shape.
        let raw = r#"["allow:Read","approve:send_emails","deny:Bash(rm:*)"]"#;
        let parsed = parse_permissions_column(raw);
        assert_eq!(parsed.allowed, vec!["Read".to_string()]);
        assert_eq!(parsed.require_approval, vec!["send_emails".to_string()]);
        assert_eq!(parsed.denied, vec!["Bash(rm:*)".to_string()]);
    }

    // Test 8 — Unknown label on codex (semantic verb, no native pattern)
    // → translation succeeds, label ends up in advisory_only so the
    // dispatch path's telemetry can quantify exposure per §6 Q2.
    #[test]
    fn test_8_unknown_label_codex_advisory() {
        let p = AgentPermissions {
            allowed: vec!["Read".into()],
            require_approval: vec![],
            denied: vec!["transfer_funds".into()],
        };
        let codex = to_codex(&p);
        assert_eq!(codex.sandbox, "read-only");
        assert!(codex.advisory_only.contains(&"transfer_funds".to_string()));

        let gate = to_api_tool_gate(&p, &[]);
        assert!(gate.denied.contains(&"transfer_funds".to_string()));
        assert_eq!(gate.check("transfer_funds"), GateDecision::Deny);
    }

    // Bonus: pass-through runtimes carry the metadata.
    #[test]
    fn test_passthrough_openclaw_hermes() {
        let p = AgentPermissions {
            allowed: vec!["read_files".into()],
            require_approval: vec![],
            denied: vec!["shell".into()],
        };
        let oc = to_openclaw(&p);
        assert_eq!(oc.get("passthrough").unwrap().as_bool(), Some(true));
        let hm = to_hermes(&p);
        assert_eq!(hm.get("passthrough").unwrap().as_bool(), Some(true));
    }

    // Bonus: MCP-tool layering (PR-3a preview — proves the second-arg
    // shape works; full wiring lands in PR-3a).
    #[test]
    fn test_mcp_tool_layering() {
        let p = AgentPermissions {
            allowed: vec!["Read".into(), "gmail.send".into()],
            require_approval: vec![],
            denied: vec![],
        };
        let mcp_tools = vec![ToolDef {
            name: "gmail.send".to_string(),
            description: "Send an email via Gmail.".to_string(),
            parameters: json!({}),
        }];
        let gate = to_api_tool_gate(&p, &mcp_tools);
        let names: Vec<&str> = gate.allowed_tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"gmail.send"));
    }
}
