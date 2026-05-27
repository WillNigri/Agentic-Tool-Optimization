// v2.13 Phase 6.x — Runtime quota visibility.
//
// Reads each runtime's local quota state file (if any) and returns a
// structured probe. Pure filesystem read — no network, no shell-out.
//
// State of the world (2026-05-26): none of Claude Code / Codex / Gemini
// CLI write a stable usage.json today. We probe a list of candidate
// paths and parse whatever shape we find; when nothing exists the
// probe returns `found = false` with the paths we tried, so the UI
// can render "quota unknown" honestly instead of pretending zero.
//
// When the runtimes ship a stable usage file in the future, add the
// path + JSON shape to `candidate_paths_for` + `parse_known_shape`
// and the probe starts returning data — no other plumbing needed.

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// One probe result per runtime. Always returned — when the file
/// doesn't exist or can't be parsed, `found = false` and the data
/// fields are None; the UI shows "quota unknown" + the path we tried.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeQuotaProbe {
    pub runtime: String,
    /// Path we read (or attempted to read). None means the runtime has
    /// no candidate paths registered yet.
    pub source_path: Option<String>,
    /// True iff we found a parseable quota file at one of the candidate
    /// paths. False covers "no file" and "file present but unparseable."
    pub found: bool,
    pub messages_used: Option<u64>,
    pub messages_limit: Option<u64>,
    /// RFC3339 timestamp when the period resets. None when unknown,
    /// the file doesn't carry a reset, or the value didn't parse as
    /// RFC3339 — we'd rather drop a malformed string than let the UI
    /// render `<time dateTime="next Tuesday">`.
    pub period_reset_at: Option<String>,
    /// Free-form note for the UI — explains what we tried when
    /// `found = false`. Stays None on success.
    pub note: Option<String>,
}

/// Runtimes we know how to probe. Intentionally a subset of
/// `commands::runtimes::RUNTIMES` — hermes and openclaw have no
/// local quota state file (the OpenClaw SSH-relay runtime sits on
/// top of remote shells, and Hermes ships no usage sidecar today).
pub const KNOWN_RUNTIMES: &[&str] = &["claude", "codex", "gemini"];

/// Probe every known runtime. Order matches `KNOWN_RUNTIMES`.
pub fn probe_all() -> Vec<RuntimeQuotaProbe> {
    KNOWN_RUNTIMES.iter().map(|r| probe(r)).collect()
}

/// Probe a single runtime. Reads each candidate path until one yields
/// a parseable shape. The first hit wins.
pub fn probe(runtime: &str) -> RuntimeQuotaProbe {
    let home = crate::db::home_dir();
    let candidates = candidate_paths_for(runtime, &home);
    if candidates.is_empty() {
        return RuntimeQuotaProbe {
            runtime: runtime.to_string(),
            source_path: None,
            found: false,
            messages_used: None,
            messages_limit: None,
            period_reset_at: None,
            note: Some(format!("no candidate path registered for runtime '{}'", runtime)),
        };
    }
    let mut last_attempted: Option<PathBuf> = None;
    for path in candidates {
        last_attempted = Some(path.clone());
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue, // missing file is the common case; keep probing
        };
        let value: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => continue, // unparseable JSON — try next candidate
        };
        if let Some(parsed) = parse_known_shape(&value) {
            return RuntimeQuotaProbe {
                runtime: runtime.to_string(),
                source_path: Some(path.display().to_string()),
                found: true,
                messages_used: parsed.used,
                messages_limit: parsed.limit,
                period_reset_at: parsed.reset_at,
                note: None,
            };
        }
    }
    RuntimeQuotaProbe {
        runtime: runtime.to_string(),
        source_path: last_attempted.map(|p| p.display().to_string()),
        found: false,
        messages_used: None,
        messages_limit: None,
        period_reset_at: None,
        note: Some("no parseable quota file at any candidate path".into()),
    }
}

fn candidate_paths_for(runtime: &str, home: &Path) -> Vec<PathBuf> {
    // Order matters — first hit wins. We list the most specific path
    // first so future runtime updates that ship a dedicated quota file
    // override any general "usage" sidecar.
    match runtime {
        "claude" => vec![
            home.join(".claude").join("usage.json"),
            home.join(".claude").join("usage").join("current.json"),
            home.join(".claude").join("quota.json"),
        ],
        "codex" => vec![
            home.join(".codex").join("usage.json"),
            home.join(".codex").join("quota.json"),
        ],
        "gemini" => vec![
            home.join(".gemini").join("usage.json"),
            home.join(".gemini").join("quota.json"),
        ],
        _ => Vec::new(),
    }
}

struct Parsed {
    used: Option<u64>,
    limit: Option<u64>,
    reset_at: Option<String>,
}

/// Accept any of a few common field-name conventions. None of the
/// runtimes have committed to a shape, so we try the obvious aliases
/// and pick whichever exists. Falls through to None when the JSON
/// carries neither used nor limit (we won't try to interpret a file
/// that doesn't look like a quota at all).
fn parse_known_shape(v: &Value) -> Option<Parsed> {
    let used = pick_u64(v, &["messages_used", "used", "requests_used", "count"]);
    let limit = pick_u64(v, &["messages_limit", "limit", "requests_limit", "max"]);
    let raw_reset = pick_string(
        v,
        &["period_reset", "period_reset_at", "reset_at", "resets_at", "reset_time"],
    );
    // Validate the reset string parses as RFC3339 before accepting it.
    // Drop unparseable strings to None — better than leaking epoch-
    // seconds or a localized date into the frontend's <time> element.
    let reset_at = raw_reset
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|_| s));
    if used.is_none() && limit.is_none() && reset_at.is_none() {
        return None;
    }
    Some(Parsed { used, limit, reset_at })
}

fn pick_u64(v: &Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        if let Some(n) = v.get(*key).and_then(|x| x.as_u64()) {
            return Some(n);
        }
        // Tolerate "42" as a string — some runtimes serialize counters
        // as strings to avoid JS precision loss. Parse back to u64.
        if let Some(s) = v.get(*key).and_then(|x| x.as_str()) {
            if let Ok(n) = s.parse::<u64>() {
                return Some(n);
            }
        }
    }
    None
}

fn pick_string(v: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(s) = v.get(*key).and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_runtime_returns_no_candidates() {
        let probe = probe("hermes");
        assert!(!probe.found);
        assert!(probe.source_path.is_none());
        assert!(probe.note.as_deref().unwrap().contains("no candidate path"));
    }

    // No `probe()` integration test against the live $HOME — it would
    // flake on any machine that happens to have a real
    // ~/.claude/usage.json (the v2.6 passive observer dogfood path can
    // create one). The parse_known_shape branches below cover the
    // logic that matters without touching $HOME.

    #[test]
    fn parses_messages_used_limit_shape() {
        let v: Value = serde_json::from_str(
            r#"{"messages_used": 42, "messages_limit": 100, "period_reset": "2026-06-01T00:00:00Z"}"#,
        )
        .unwrap();
        let parsed = parse_known_shape(&v).unwrap();
        assert_eq!(parsed.used, Some(42));
        assert_eq!(parsed.limit, Some(100));
        assert_eq!(parsed.reset_at.as_deref(), Some("2026-06-01T00:00:00Z"));
    }

    #[test]
    fn parses_used_limit_alias_shape() {
        let v: Value =
            serde_json::from_str(r#"{"used": 12, "limit": 50, "reset_at": "2026-06-01T00:00:00Z"}"#)
                .unwrap();
        let parsed = parse_known_shape(&v).unwrap();
        assert_eq!(parsed.used, Some(12));
        assert_eq!(parsed.limit, Some(50));
    }

    #[test]
    fn tolerates_string_encoded_numbers() {
        let v: Value =
            serde_json::from_str(r#"{"messages_used": "42", "messages_limit": "100"}"#).unwrap();
        let parsed = parse_known_shape(&v).unwrap();
        assert_eq!(parsed.used, Some(42));
        assert_eq!(parsed.limit, Some(100));
    }

    #[test]
    fn rejects_non_quota_json() {
        let v: Value = serde_json::from_str(r#"{"foo": "bar"}"#).unwrap();
        assert!(parse_known_shape(&v).is_none());
    }

    #[test]
    fn drops_non_rfc3339_reset_to_none() {
        // A runtime that ships epoch-seconds or a localized string in
        // `period_reset` should NOT leak through to the frontend's
        // <time> tag. Accept the row (used + limit are still useful)
        // but drop the unparseable reset.
        let v: Value = serde_json::from_str(
            r#"{"messages_used": 5, "messages_limit": 100, "period_reset": "next Tuesday"}"#,
        )
        .unwrap();
        let parsed = parse_known_shape(&v).unwrap();
        assert_eq!(parsed.used, Some(5));
        assert_eq!(parsed.limit, Some(100));
        assert!(parsed.reset_at.is_none());
    }

    #[test]
    fn keeps_rfc3339_reset() {
        let v: Value = serde_json::from_str(
            r#"{"messages_used": 5, "period_reset": "2026-06-01T00:00:00Z"}"#,
        )
        .unwrap();
        let parsed = parse_known_shape(&v).unwrap();
        assert_eq!(parsed.reset_at.as_deref(), Some("2026-06-01T00:00:00Z"));
    }
}
