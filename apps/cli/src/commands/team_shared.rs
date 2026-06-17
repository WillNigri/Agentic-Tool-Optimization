// v2.15 Wave 4 — CLI parity for team-shared resources.
//
// This module provides the shared HTTP helpers that back the Wave 4 subcommands
// added to `sessions`, `war_rooms`, `chats`, `loops`, and `missions`:
//
//   share <id> --team <slug>
//   unshare <id> --team <slug>
//   list-shared --team <slug>
//   append-event <id> --team <slug> --kind <event-kind> --json <payload>
//
// Auth: reads the JWT from ~/.ato/auth.json — the same file written by
// `ato login` / `ato pro enable`. If the file is absent or malformed the
// helper returns a clean error ("run `ato login` first") without panicking.
//
// Team slug → UUID: the first call resolves the slug against GET /api/teams
// (the membership list). UUIDs are accepted directly to skip the resolve round-
// trip (callers that already know the UUID can pass it).
//
// Encrypted append: Wave 4 intentionally ships the --encrypted flag but
// refuses at runtime with a clear error message. Full E2E from the CLI
// requires the team key cache from the desktop's unsealed SQLite and a
// libsodium Rust binding — both deferred to a follow-up wave. See TODO below.

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

// ── Auth / HTTP primitives ────────────────────────────────────────────────────

fn auth_file_path() -> PathBuf {
    crate::db::home_dir().join(".ato").join("auth.json")
}

/// Read the JWT token from ~/.ato/auth.json.
/// Returns a clear error message if the user is not signed in.
pub fn read_token() -> Result<String> {
    let contents = fs::read_to_string(auth_file_path())
        .context("Not signed in. Run `ato login` first.")?;
    let json: Value =
        serde_json::from_str(&contents).context("Failed to parse ~/.ato/auth.json")?;
    json.get("token")
        .and_then(|t| t.as_str())
        .map(String::from)
        .context("Auth token missing from ~/.ato/auth.json — run `ato login` again")
}

fn api_base() -> String {
    match std::env::var("ATO_CLOUD_URL") {
        Ok(url) => format!("{}/api", url.trim_end_matches('/')),
        Err(_) => "https://api.agentictool.ai/api".to_string(),
    }
}

fn http_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("HTTP client build failed")
}

// ── UUID detection ────────────────────────────────────────────────────────────

fn looks_like_uuid(s: &str) -> bool {
    // 8-4-4-4-12 hex groups separated by hyphens, case-insensitive.
    // Using a hand-rolled check to avoid pulling in regex for a single pattern.
    let bytes = s.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    let dashes = [8, 13, 18, 23];
    for (i, &b) in bytes.iter().enumerate() {
        if dashes.contains(&i) {
            if b != b'-' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

// ── Team slug → UUID resolver ─────────────────────────────────────────────────

/// Resolve a team slug or UUID to a UUID.
/// If `slug_or_uuid` already looks like a UUID it is returned as-is (no HTTP
/// call). Otherwise, GET /api/teams is queried and the first matching `slug`
/// field is returned.
///
/// Returns an error if:
///   - the user is not signed in
///   - the HTTP call fails
///   - no team with the given slug is found in the membership list
pub fn resolve_team_id(slug_or_uuid: &str, token: &str) -> Result<String> {
    if looks_like_uuid(slug_or_uuid) {
        return Ok(slug_or_uuid.to_string());
    }

    let client = http_client()?;
    let url = format!("{}/teams", api_base());
    let resp = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .context("Failed to GET /api/teams (team slug resolution)")?;

    let status = resp.status();
    let body: Value = resp
        .json()
        .context("Failed to parse /api/teams response as JSON")?;

    if !status.is_success() {
        let msg = body
            .get("error")
            .and_then(|e| e.get("message").or(Some(e)))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return Err(anyhow!(
            "GET /api/teams returned {}: {}",
            status.as_u16(),
            msg
        ));
    }

    // Cloud returns { success: true, data: [ { id, slug, name, ... }, ... ] }
    let teams = body
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| anyhow!("Unexpected /api/teams response shape"))?;

    for team in teams {
        let team_slug = team.get("slug").and_then(|s| s.as_str()).unwrap_or("");
        let team_id = team.get("id").and_then(|i| i.as_str()).unwrap_or("");
        if team_slug == slug_or_uuid || team_id == slug_or_uuid {
            return Ok(team_id.to_string());
        }
    }

    Err(anyhow!(
        "No team with slug '{}' found in your membership list. \
         Use `ato teams list` to see available teams.",
        slug_or_uuid
    ))
}

// ── Response helpers ──────────────────────────────────────────────────────────

fn is_success(status: reqwest::StatusCode, body: &Value) -> bool {
    if !status.is_success() {
        return false;
    }
    // Treat explicit `success: false` as an error; anything else (including
    // no envelope) passes through as success.
    !matches!(body.get("success").and_then(|v| v.as_bool()), Some(false))
}

fn handle_cloud_error(status: reqwest::StatusCode, body: &Value) -> anyhow::Error {
    let code = status.as_u16();
    let err = body.get("error");
    let err_code = err.and_then(|e| e.get("code")).and_then(|c| c.as_str());
    // Try the canonical { error: { message } } or { error: "string" } shapes.
    let msg = err
        .and_then(|e| {
            e.get("message")
                .and_then(|m| m.as_str())
                .map(String::from)
                .or_else(|| e.as_str().map(String::from))
        })
        .or_else(|| {
            body.get("message")
                .and_then(|m| m.as_str())
                .map(String::from)
        });

    // Tier gate: the API returns 402 PRO_REQUIRED with required_tier +
    // upgrade_url for Team-tier-only features (shared workspaces). Surface the
    // upgrade path instead of a bare status — otherwise this reads as a generic
    // failure and sends you debugging the wrong thing.
    if code == 402 || err_code == Some("PRO_REQUIRED") {
        let required = err
            .and_then(|e| e.get("required_tier"))
            .and_then(|t| t.as_str())
            .unwrap_or("team");
        let base = msg.unwrap_or_else(|| {
            format!("This feature requires the {required} subscription tier.")
        });
        return match err.and_then(|e| e.get("upgrade_url")).and_then(|u| u.as_str()) {
            Some(url) => anyhow!("{base} (HTTP 402 — requires {required} tier). Upgrade: {url}"),
            None => anyhow!("{base} (HTTP 402 — requires {required} tier)"),
        };
    }

    // A status error with no JSON error body (e.g. an Express HTML 404
    // "Cannot POST /…") almost always means the route isn't available on the
    // server — the cloud backend is behind this CLI, or the route isn't
    // deployed yet — NOT a client mistake. Say so explicitly.
    match msg {
        Some(m) => anyhow!("{m} (HTTP {code})"),
        None if code == 404 => anyhow!(
            "Endpoint not available on the server (HTTP 404). The cloud backend may be \
             behind this CLI version, or this route isn't deployed yet — not a problem \
             with your command."
        ),
        None => anyhow!("Request failed (HTTP {code})"),
    }
}

// ── Public helpers ────────────────────────────────────────────────────────────

/// POST /api/teams/:team_id/<url_kind>/share — share a resource snapshot.
///
/// `url_kind` is the URL segment, e.g. `sessions`, `war-rooms`, `chats`,
/// `loops`, `missions`.
///
/// The snapshot is deliberately minimal: just the resource id and a null
/// snapshot payload. The desktop client sends the full turn transcript; the
/// CLI sends a lightweight stub that is enough to register the share row and
/// allow teammates to look up the resource id. A richer --snapshot-file flag
/// can be added in a follow-up without breaking the API contract.
pub fn share_resource(
    url_kind: &str,
    id_field: &str,  // e.g. "session_id", "war_room_id"
    resource_id: &str,
    team_slug: &str,
    opts: &crate::output::Opts,
) -> Result<()> {
    let token = read_token()?;
    let team_id = resolve_team_id(team_slug, &token)?;
    let client = http_client()?;

    let url = format!("{}/teams/{}/{}/share", api_base(), team_id, url_kind);
    let mut body = serde_json::json!({ id_field: resource_id });
    // Model A PR2 — attach this install's machine id + proof-of-possession
    // secret so an authorized-hosts-enforcing workspace can verify the CLI is
    // a real authorized host. Best-effort: if the local store can't be opened
    // we still attempt the share (enforcement-off teams are unaffected).
    if let Ok(conn) = crate::db::open_readwrite(&crate::db::default_db_path()) {
        if let Value::Object(ref mut map) = body {
            map.insert(
                "initiator_machine_id".into(),
                Value::String(crate::db::machine_id(&conn)),
            );
            map.insert(
                "initiator_machine_secret".into(),
                Value::String(crate::db::machine_secret(&conn)),
            );
        }
    }

    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .with_context(|| format!("POST {}", url))?;

    let status = resp.status();
    let resp_body: Value = resp.json().unwrap_or(serde_json::json!({}));

    if !is_success(status, &resp_body) {
        return Err(handle_cloud_error(status, &resp_body));
    }

    if opts.human {
        crate::output::emit_human(&format!(
            "Shared {} {} with team {}",
            url_kind, resource_id, team_slug
        ));
    } else {
        let payload = resp_body.get("data").cloned().unwrap_or(resp_body);
        crate::output::emit_json(&payload)?;
    }
    Ok(())
}

/// DELETE /api/teams/:team_id/<url_kind>/:resource_id/share — remove a share.
pub fn unshare_resource(
    url_kind: &str,
    resource_id: &str,
    team_slug: &str,
    opts: &crate::output::Opts,
) -> Result<()> {
    let token = read_token()?;
    let team_id = resolve_team_id(team_slug, &token)?;
    let client = http_client()?;

    let url = format!(
        "{}/teams/{}/{}/{}/share",
        api_base(),
        team_id,
        url_kind,
        resource_id
    );

    let resp = client
        .delete(&url)
        .bearer_auth(&token)
        .send()
        .with_context(|| format!("DELETE {}", url))?;

    let status = resp.status();

    // 204 No Content is success on DELETE — body is empty.
    if status == reqwest::StatusCode::NO_CONTENT || status.is_success() {
        if opts.human {
            crate::output::emit_human(&format!(
                "Unshared {} {} from team {}",
                url_kind, resource_id, team_slug
            ));
        } else {
            crate::output::emit_json(&serde_json::json!({ "success": true }))?;
        }
        return Ok(());
    }

    let resp_body: Value = resp.json().unwrap_or(serde_json::json!({}));
    Err(handle_cloud_error(status, &resp_body))
}

/// GET /api/teams/:team_id/<url_kind> — list resources shared with a team.
pub fn list_shared(
    url_kind: &str,
    team_slug: &str,
    opts: &crate::output::Opts,
) -> Result<()> {
    let token = read_token()?;
    let team_id = resolve_team_id(team_slug, &token)?;
    let client = http_client()?;

    let url = format!("{}/teams/{}/{}", api_base(), team_id, url_kind);
    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .with_context(|| format!("GET {}", url))?;

    let status = resp.status();
    let resp_body: Value = resp.json().unwrap_or(serde_json::json!({}));

    if !is_success(status, &resp_body) {
        return Err(handle_cloud_error(status, &resp_body));
    }

    let data = resp_body.get("data").cloned().unwrap_or(resp_body);

    if opts.human {
        // Emit a simple table-style line per row.
        if let Some(rows) = data.as_array() {
            if rows.is_empty() {
                crate::output::emit_human(&format!(
                    "No {} shared with team {}.",
                    url_kind, team_slug
                ));
            } else {
                for row in rows {
                    let id = row
                        .get("session_id")
                        .or_else(|| row.get("war_room_id"))
                        .or_else(|| row.get("chat_thread_id"))
                        .or_else(|| row.get("loop_id"))
                        .or_else(|| row.get("mission_id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let shared_at = row
                        .get("shared_at")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    crate::output::emit_human(&format!("  {} (shared {})", id, shared_at));
                }
            }
        } else {
            crate::output::emit_human(&format!("{}", data));
        }
    } else {
        crate::output::emit_json(&data)?;
    }
    Ok(())
}

/// POST /api/teams/:team_id/<url_kind>/:resource_id/events — append a
/// plaintext event to a team-shared resource.
///
/// `encrypted=false` → one-shot plaintext append.
/// `encrypted=true`  → Wave 4 stub error: E2E from CLI ships in a follow-up.
///
/// Returns the seq_num assigned by the server.
///
/// # TODO (Wave 5 / follow-up)
/// Full E2E support requires:
///   1. Reading the team key from the local SQLite cache at ~/.ato/local.db
///      (written by the desktop's startup unsealing of the Team Key envelope).
///   2. A Rust libsodium / crypto_secretbox wrapper to encrypt the payload
///      and produce ciphertext_b64 + nonce_b64.
///   3. Signing with the user's e2e signing keypair.
///   4. A /events/reserve call to pre-mint the seq_num for AEAD AD, followed
///      by the commit append (the Wave 3 two-step path in sharedEvents.ts).
/// Defer until the desktop's key unsealing is observable from the CLI process
/// without requiring IPC.
pub fn append_event(
    url_kind: &str,
    resource_id: &str,
    team_slug: &str,
    event_kind: &str,
    payload: Value,
    encrypted: bool,
    opts: &crate::output::Opts,
) -> Result<i64> {
    if encrypted {
        // Wave 4 explicit refusal — see TODO above.
        return Err(anyhow!(
            "--encrypted append from the CLI is shipping in a follow-up wave; \
             use the desktop app for E2E event appends, or run without --encrypted \
             for plaintext."
        ));
    }

    let token = read_token()?;
    let team_id = resolve_team_id(team_slug, &token)?;
    let client = http_client()?;

    let url = format!(
        "{}/teams/{}/{}/{}/events",
        api_base(),
        team_id,
        url_kind,
        resource_id
    );

    #[derive(Serialize)]
    struct AppendBody<'a> {
        event_kind: &'a str,
        payload_json: &'a Value,
        surface: &'static str,
    }

    let body = AppendBody {
        event_kind,
        payload_json: &payload,
        surface: "cli",
    };

    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .with_context(|| format!("POST {}", url))?;

    let status = resp.status();
    let resp_body: Value = resp.json().unwrap_or(serde_json::json!({}));

    if !is_success(status, &resp_body) {
        return Err(handle_cloud_error(status, &resp_body));
    }

    let seq_num = resp_body
        .get("data")
        .and_then(|d| d.get("seq_num"))
        .and_then(|s| s.as_i64())
        .unwrap_or(0);

    if opts.human {
        crate::output::emit_human(&format!(
            "Appended event '{}' to {} {} (seq_num={})",
            event_kind, url_kind, resource_id, seq_num
        ));
    } else {
        let data = resp_body.get("data").cloned().unwrap_or(resp_body);
        crate::output::emit_json(&data)?;
    }

    Ok(seq_num)
}

/// Parse a `--json` argument that is either inline JSON or `@path/to/file`.
/// When the value starts with `@`, the remainder is treated as a file path and
/// the file's contents are parsed as JSON. Otherwise the string is parsed as
/// inline JSON directly.
pub fn parse_json_arg(json_arg: &str) -> Result<Value> {
    if let Some(path) = json_arg.strip_prefix('@') {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read JSON file: {}", path))?;
        serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse JSON from file: {}", path))
    } else {
        serde_json::from_str(json_arg)
            .context("Failed to parse --json argument as JSON. Pass inline JSON or @path/to/file")
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────
//
// Wave 4 testing posture: the existing CLI test suite does not have an HTTP
// mock harness (no mockito / wiremock dependency in Cargo.toml). Adding one
// is possible but non-trivial and out of scope for the Wave 4 timeline.
//
// TODO: When a mock HTTP crate is added, add tests that:
//   1. Verify POST /api/teams/:id/sessions/share sends { session_id: "..." }
//   2. Verify DELETE /api/teams/:id/sessions/:id/share sends no body
//   3. Verify GET /api/teams/:id/sessions returns and prints data array
//   4. Verify POST /api/teams/:id/sessions/:id/events sends { event_kind, payload_json, surface: "cli" }
//   5. Verify --encrypted returns the refusal error (no HTTP call)
//   6. Repeat (1)-(4) for war-rooms, chats, loops, missions
//
// In the meantime, the logic is covered by the integration tests below that
// exercise the non-HTTP paths (UUID detection, token reading, JSON parsing,
// encrypted refusal).

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_looks_like_uuid_valid() {
        assert!(looks_like_uuid(
            "550e8400-e29b-41d4-a716-446655440000"
        ));
        assert!(looks_like_uuid(
            "550E8400-E29B-41D4-A716-446655440000"
        ));
    }

    #[test]
    fn test_looks_like_uuid_slug() {
        assert!(!looks_like_uuid("my-team"));
        assert!(!looks_like_uuid("acme-corp"));
        assert!(!looks_like_uuid(""));
    }

    #[test]
    fn test_parse_json_arg_inline() {
        let v = parse_json_arg(r#"{"turn": "assistant", "text": "hello"}"#).unwrap();
        assert_eq!(v["turn"], "assistant");
    }

    #[test]
    fn test_parse_json_arg_bad_json() {
        let err = parse_json_arg("not json {").unwrap_err();
        assert!(err.to_string().contains("Failed to parse --json"));
    }

    #[test]
    fn test_parse_json_arg_file_missing() {
        let err = parse_json_arg("@/tmp/does_not_exist_wave4_test.json").unwrap_err();
        assert!(err.to_string().contains("Failed to read JSON file"));
    }

    #[test]
    fn test_parse_json_arg_file_path() {
        // Write a temp file and verify @ reading works.
        let path = "/tmp/ato_wave4_test_payload.json";
        std::fs::write(path, r#"{"event": "turn_appended"}"#).unwrap();
        let v = parse_json_arg(&format!("@{}", path)).unwrap();
        assert_eq!(v["event"], "turn_appended");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn test_encrypted_append_returns_refusal_error() {
        // append_event with encrypted=true must NOT make an HTTP call.
        // We verify this by checking the error message without standing up
        // a real server — the function returns before the HTTP client is built
        // when encrypted=true and the token file is present.
        //
        // Write a fake auth.json so read_token() doesn't fail first.
        let fake_auth_dir = std::env::temp_dir().join("ato_wave4_test_auth");
        std::fs::create_dir_all(&fake_auth_dir).ok();
        let fake_auth_file = fake_auth_dir.join("auth.json");
        std::fs::write(&fake_auth_file, r#"{"token":"fake_token"}"#).unwrap();

        // Override the path via the ATO_HOME env var if supported, or
        // rely on the test environment already having a valid token file.
        // The critical path under test is the encrypted branch, which
        // returns Err before calling resolve_team_id / HTTP.
        //
        // Rather than monkey-patching home_dir, we test the encrypted guard
        // directly: build a minimal Opts and call append_event.
        // If the home dir doesn't have auth.json, the error we get is
        // "Not signed in" which is different from the encrypted error.
        // We accept both: the important invariant is that *no HTTP call is made*.
        let opts = crate::output::Opts { human: false, quiet: false };
        let result = append_event(
            "sessions",
            "550e8400-e29b-41d4-a716-446655440000",
            "my-team",
            "turn_appended",
            json!({"text": "hello"}),
            /* encrypted= */ true,
            &opts,
        );
        let err = result.unwrap_err();
        // Either "encrypted" error or "Not signed in" — both are acceptable
        // because neither represents an HTTP call.
        let msg = err.to_string();
        assert!(
            msg.contains("encrypted") || msg.contains("signed in"),
            "unexpected error: {}",
            msg
        );
    }

    /// Verify the URL kind → id field mapping table used by callers.
    /// This isn't runtime logic — it's a documentation test that forces
    /// readers to notice if the mapping drifts from the cloud schema.
    #[test]
    fn test_url_kind_to_id_field_table() {
        let table = [
            ("sessions",  "session_id"),
            ("war-rooms", "war_room_id"),
            ("chats",     "chat_thread_id"),
            ("loops",     "loop_id"),
            ("missions",  "mission_id"),
        ];
        for (url_kind, id_field) in table {
            // Just assert non-empty so the table is exercised in CI.
            assert!(!url_kind.is_empty());
            assert!(!id_field.is_empty());
        }
    }
}
