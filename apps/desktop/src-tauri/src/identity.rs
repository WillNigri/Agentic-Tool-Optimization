// v2.8.x P2 — Identity passthrough for MCPs.
//
// MCPs need to know which USER initiated a tool call so the MCP
// author can enforce row/column-level ACLs at the data layer.
// ATO is the identity provider; the MCP/data lake is the source
// of truth for what each user can read.
//
// Two transport channels (MCP supports stdio + HTTP):
//   - **stdio MCPs**: identity flows via spawn environment variables
//     (ATO_USER_ID, ATO_WORKSPACE_ID, etc) and via JSON-RPC
//     `params._meta` block (MCP spec reserves `_meta` for client
//     metadata)
//   - **HTTP MCPs** (streamable-http transport): identity flows via
//     real HTTP request headers (X-ATO-User-Id, etc) — future PR
//
// Resolution order for each field:
//   1. Explicit override from the dispatch call (Phase 2: workspace
//      + room come from cloud session — out of scope for OSS Phase 1)
//   2. Environment variable (the OSS Phase 1 escape hatch — user
//      sets ATO_USER_ID in their shell before `ato dispatch`)
//   3. Fallback (system $USER for user_id; "" for workspace/room)
//
// War-room note: this is intentionally minimal for the OSS Phase 1
// ship. The cloud-side Team tier replaces env-var resolution with
// authenticated workspace membership; same wire format, different
// source of truth.

use serde::Serialize;
use std::collections::HashMap;

/// Identity context carried through a single dispatch lifecycle.
/// All fields are optional — MCP authors get whatever ATO knows
/// at the time of the call and decide their own degradation
/// strategy (allow vs deny on missing identity).
#[derive(Debug, Clone, Serialize, Default)]
pub struct AtoIdentity {
    /// User performing the dispatch. Email, login, or stable UUID.
    /// Defaults to `$USER` from the environment if unset.
    pub user_id: Option<String>,
    /// Workspace the dispatch belongs to. Populated by cloud-sync;
    /// `None` for local-only / OSS Phase 1 installs.
    pub workspace_id: Option<String>,
    /// Room / war-room id the dispatch is scoped to. Populated when
    /// the caller is operating inside a room context.
    pub room_id: Option<String>,
    /// Session id linking related dispatches.
    pub session_id: Option<String>,
    /// Agent slug if this dispatch is going through a named agent
    /// (vs ad-hoc).
    pub agent_slug: Option<String>,
}

impl AtoIdentity {
    /// Resolve identity from process environment. This is the OSS
    /// Phase 1 entry point — cloud-side resolution lives in
    /// `ato-cloud` and supersedes this when authenticated.
    pub fn from_env() -> Self {
        Self {
            user_id: std::env::var("ATO_USER_ID")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| std::env::var("USER").ok().filter(|s| !s.is_empty())),
            workspace_id: std::env::var("ATO_WORKSPACE_ID").ok().filter(|s| !s.is_empty()),
            room_id: std::env::var("ATO_ROOM_ID").ok().filter(|s| !s.is_empty()),
            session_id: std::env::var("ATO_SESSION_ID").ok().filter(|s| !s.is_empty()),
            agent_slug: std::env::var("ATO_AGENT_SLUG").ok().filter(|s| !s.is_empty()),
        }
    }

    /// Render as environment variables for spawning a child MCP
    /// process. Empty fields are skipped so the child sees only
    /// what we actually know — MCP authors can `match env::var(…)`
    /// and degrade cleanly.
    pub fn to_env(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        if let Some(v) = &self.user_id {
            m.insert("ATO_USER_ID".to_string(), v.clone());
        }
        if let Some(v) = &self.workspace_id {
            m.insert("ATO_WORKSPACE_ID".to_string(), v.clone());
        }
        if let Some(v) = &self.room_id {
            m.insert("ATO_ROOM_ID".to_string(), v.clone());
        }
        if let Some(v) = &self.session_id {
            m.insert("ATO_SESSION_ID".to_string(), v.clone());
        }
        if let Some(v) = &self.agent_slug {
            m.insert("ATO_AGENT_SLUG".to_string(), v.clone());
        }
        m
    }

    /// Render as JSON for the MCP `params._meta` field. Per the MCP
    /// spec (`_meta` reserved for client-supplied metadata), the
    /// MCP server reads this off any tools/call request without
    /// needing custom protocol extensions.
    pub fn to_meta_json(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        if let Some(v) = &self.user_id {
            obj.insert("ato.user_id".to_string(), serde_json::Value::String(v.clone()));
        }
        if let Some(v) = &self.workspace_id {
            obj.insert("ato.workspace_id".to_string(), serde_json::Value::String(v.clone()));
        }
        if let Some(v) = &self.room_id {
            obj.insert("ato.room_id".to_string(), serde_json::Value::String(v.clone()));
        }
        if let Some(v) = &self.session_id {
            obj.insert("ato.session_id".to_string(), serde_json::Value::String(v.clone()));
        }
        if let Some(v) = &self.agent_slug {
            obj.insert("ato.agent_slug".to_string(), serde_json::Value::String(v.clone()));
        }
        serde_json::Value::Object(obj)
    }

    /// True when this identity has at least a user_id — useful for
    /// audit logging "did we send identity or fall back to anonymous".
    pub fn has_user(&self) -> bool {
        self.user_id.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_reads_user_id_override() {
        // SAFETY: tests run single-threaded by default for env-mutation
        // tests; we restore the prior state after.
        let prev = std::env::var("ATO_USER_ID").ok();
        std::env::set_var("ATO_USER_ID", "alice@acme.com");
        let id = AtoIdentity::from_env();
        assert_eq!(id.user_id.as_deref(), Some("alice@acme.com"));
        if let Some(p) = prev {
            std::env::set_var("ATO_USER_ID", p);
        } else {
            std::env::remove_var("ATO_USER_ID");
        }
    }

    #[test]
    fn from_env_falls_back_to_system_user() {
        let prev = std::env::var("ATO_USER_ID").ok();
        std::env::remove_var("ATO_USER_ID");
        let id = AtoIdentity::from_env();
        // $USER is set by every Unix shell + CI; if not, the test
        // environment is too weird to make assumptions about.
        if std::env::var("USER").is_ok() {
            assert!(id.user_id.is_some(), "should fall back to $USER");
        }
        if let Some(p) = prev {
            std::env::set_var("ATO_USER_ID", p);
        }
    }

    #[test]
    fn to_env_skips_empty_fields() {
        let id = AtoIdentity {
            user_id: Some("alice".into()),
            workspace_id: None,
            room_id: None,
            session_id: None,
            agent_slug: None,
        };
        let env = id.to_env();
        assert_eq!(env.get("ATO_USER_ID").map(String::as_str), Some("alice"));
        assert!(!env.contains_key("ATO_WORKSPACE_ID"));
        assert!(!env.contains_key("ATO_ROOM_ID"));
    }

    #[test]
    fn to_meta_json_uses_dotted_keys() {
        let id = AtoIdentity {
            user_id: Some("alice@acme.com".into()),
            workspace_id: Some("ws_1".into()),
            ..Default::default()
        };
        let m = id.to_meta_json();
        assert_eq!(m["ato.user_id"], "alice@acme.com");
        assert_eq!(m["ato.workspace_id"], "ws_1");
        assert!(m.get("ato.room_id").is_none(), "absent fields omitted, not nulled");
    }

    #[test]
    fn has_user_true_only_with_nonempty_id() {
        assert!(!AtoIdentity::default().has_user());
        let with = AtoIdentity {
            user_id: Some("alice".into()),
            ..Default::default()
        };
        assert!(with.has_user());
        let empty = AtoIdentity {
            user_id: Some("".into()),
            ..Default::default()
        };
        assert!(!empty.has_user(), "empty string is not a user");
    }
}
