// v2.16 PR-B — initiator attribution detection.
//
// Resolves "who/what started this dispatch" from the environment so the
// schema columns added in PR-A (initiator_kind / client_surface /
// initiator_id, see migrations) can be populated at the edge. Detection
// is env-first: an explicit ATO_INITIATOR_KIND / ATO_CLIENT_SURFACE /
// ATO_INITIATOR_ID always wins, otherwise we infer the kind from the
// runtime's own marker variables (CLAUDECODE, OPENAI_AGENT_RUNTIME,
// GEMINI_CLI_*), and finally fall back to a plain human/cli pair.

use std::env;

/// Detect who/what initiated this invocation.
///
/// Env override (`ATO_INITIATOR_KIND`) wins; otherwise infer from
/// runtime markers, defaulting to `human`.
pub fn detect_initiator_kind() -> String {
    if let Ok(kind) = env::var("ATO_INITIATOR_KIND") {
        if !kind.is_empty() {
            return kind;
        }
    }

    if env::var("CLAUDECODE").as_deref() == Ok("1") {
        return "agent:claude".to_string();
    }
    if env::var("OPENAI_AGENT_RUNTIME").as_deref() == Ok("codex") {
        return "agent:codex".to_string();
    }
    if env::var("GEMINI_CLI_VERSION").is_ok() || env::var("GEMINI_CLI").is_ok() {
        return "agent:gemini".to_string();
    }

    match env::var("ATO_INITIATED_BY").as_deref() {
        Ok("tick") => return "coordinator".to_string(),
        Ok("scheduler") => return "scheduler".to_string(),
        _ => {}
    }

    "human".to_string()
}

/// Detect the client surface the invocation came through.
///
/// Env override (`ATO_CLIENT_SURFACE`) wins; otherwise default to `cli`.
/// Other surfaces (`desktop`, `mcp_stdio`, `tick`) are set by their
/// callers explicitly via the env var.
pub fn detect_client_surface() -> String {
    if let Ok(surface) = env::var("ATO_CLIENT_SURFACE") {
        if !surface.is_empty() {
            return surface;
        }
    }
    "cli".to_string()
}

/// Detect a stable initiator id, if one was provided.
///
/// Read from `ATO_INITIATOR_ID` only — there is nothing to infer.
pub fn detect_initiator_id() -> Option<String> {
    match env::var("ATO_INITIATOR_ID") {
        Ok(id) if !id.is_empty() => Some(id),
        _ => None,
    }
}

/// Detect the signed-in cloud member id (Model A attribution).
///
/// Reads `~/.ato/auth.json` and decodes the `userId` claim out of the JWT
/// access token (the auth service signs `{ userId, email }` — see ato-cloud
/// packages/shared/src/auth.ts). Returns `None` for pure-local use (not
/// signed in), which is correct: such turns carry no member and are not
/// eligible to append into a shared team workspace.
///
/// Decode-only, NOT verify: this is attribution metadata, not an authz
/// decision. The cloud re-verifies the token's signature on every API call;
/// a forged local member_id only mislabels the user's own local rows.
pub fn detect_member_id() -> Option<String> {
    use base64::Engine;
    let path = crate::db::home_dir().join(".ato").join("auth.json");
    let contents = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    let token = json.get("token")?.as_str()?;
    // JWT = header.payload.signature; payload is base64url(JSON claims).
    let payload_b64 = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    claims
        .get("userId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// The resolved attribution fields for a dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    pub kind: String,
    pub surface: String,
    pub id: Option<String>,
    /// Model A — signed-in cloud member id (users.id), or None if local-only.
    pub member: Option<String>,
}

impl Attribution {
    /// Resolve all fields from the environment + local auth state.
    /// `machine_id` is resolved separately at the insert site (it needs a
    /// DB connection — see db::machine_id).
    pub fn detect() -> Self {
        Attribution {
            kind: detect_initiator_kind(),
            surface: detect_client_surface(),
            id: detect_initiator_id(),
            member: detect_member_id(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Tests mutate process-global env, so serialize them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Clear every variable detection reads, so each test starts clean.
    fn clear_env() {
        for key in [
            "ATO_INITIATOR_KIND",
            "ATO_CLIENT_SURFACE",
            "ATO_INITIATOR_ID",
            "CLAUDECODE",
            "OPENAI_AGENT_RUNTIME",
            "GEMINI_CLI_VERSION",
            "GEMINI_CLI",
            "ATO_INITIATED_BY",
        ] {
            env::remove_var(key);
        }
    }

    #[test]
    fn defaults_to_human_cli() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        let a = Attribution::detect();
        assert_eq!(a.kind, "human");
        assert_eq!(a.surface, "cli");
        assert_eq!(a.id, None);
    }

    #[test]
    fn claudecode_marker_infers_claude() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        env::set_var("CLAUDECODE", "1");
        assert_eq!(detect_initiator_kind(), "agent:claude");
        clear_env();
    }

    #[test]
    fn explicit_kind_override_wins() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        // Even with a runtime marker present, the explicit override wins.
        env::set_var("CLAUDECODE", "1");
        env::set_var("ATO_INITIATOR_KIND", "agent:custom");
        assert_eq!(detect_initiator_kind(), "agent:custom");
        clear_env();
    }

    #[test]
    fn tick_maps_to_coordinator() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        env::set_var("ATO_INITIATED_BY", "tick");
        assert_eq!(detect_initiator_kind(), "coordinator");
        clear_env();
    }

    #[test]
    fn scheduler_maps_to_scheduler() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        env::set_var("ATO_INITIATED_BY", "scheduler");
        assert_eq!(detect_initiator_kind(), "scheduler");
        clear_env();
    }

    #[test]
    fn surface_and_id_overrides_apply() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        env::set_var("ATO_CLIENT_SURFACE", "desktop");
        env::set_var("ATO_INITIATOR_ID", "user-42");
        let a = Attribution::detect();
        assert_eq!(a.surface, "desktop");
        assert_eq!(a.id, Some("user-42".to_string()));
        clear_env();
    }

    // Build a minimal JWT (header.payload.sig) with the given claims JSON.
    // Only the payload segment matters to detect_member_id (decode-only).
    fn fake_jwt(claims_json: &str) -> String {
        use base64::Engine;
        let b64 = |s: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
        format!("{}.{}.{}", b64(b"{}"), b64(claims_json.as_bytes()), "sig")
    }

    #[test]
    fn member_id_decoded_from_jwt_user_id_claim() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".ato")).unwrap();
        let token = fake_jwt(r#"{"userId":"user-abc-123","email":"t@e.com"}"#);
        std::fs::write(
            tmp.path().join(".ato/auth.json"),
            serde_json::json!({ "token": token, "email": "t@e.com" }).to_string(),
        )
        .unwrap();
        let prev = env::var("HOME").ok();
        env::set_var("HOME", tmp.path());
        assert_eq!(detect_member_id(), Some("user-abc-123".to_string()));
        match prev {
            Some(h) => env::set_var("HOME", h),
            None => env::remove_var("HOME"),
        }
    }

    #[test]
    fn member_id_none_when_not_signed_in() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap(); // no .ato/auth.json
        let prev = env::var("HOME").ok();
        env::set_var("HOME", tmp.path());
        assert_eq!(detect_member_id(), None);
        match prev {
            Some(h) => env::set_var("HOME", h),
            None => env::remove_var("HOME"),
        }
    }
}
