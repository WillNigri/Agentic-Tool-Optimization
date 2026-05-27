// v2.13 Phase 6.x polish — Settings → Runtimes → Quota panel backend.
//
// Same shell-out pattern as remote_runtimes_view: the CLI is the
// canonical implementation, the desktop calls it. The CLI's
// `runtime_quota::probe_all` reads ~/.claude/usage.json and friends
// directly from disk — read-only, no network, no SQLite write.

use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeQuotaProbeRow {
    pub runtime: String,
    pub source_path: Option<String>,
    pub found: bool,
    pub messages_used: Option<u64>,
    pub messages_limit: Option<u64>,
    pub period_reset_at: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StatusEnvelope {
    // Option<Vec> + serde(default) tolerates the field being null or
    // omitted. An older `ato` that doesn't recognize --with-quota
    // would exit non-zero (clap rejects unknown flags); that case is
    // caught earlier at the `out.status.success()` check, never here.
    // This branch covers a CLI that ran but emitted malformed JSON
    // (banner-on-stdout, partial write, etc.).
    #[serde(default)]
    runtime_quota_probes: Option<Vec<RawProbe>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct RawProbe {
    runtime: String,
    source_path: Option<String>,
    found: bool,
    messages_used: Option<u64>,
    messages_limit: Option<u64>,
    period_reset_at: Option<String>,
    note: Option<String>,
}

fn resolve_ato_binary() -> Result<String, String> {
    if let Some(p) = crate::commands::which_cli("ato") {
        return Ok(p);
    }
    Ok("ato".to_string())
}

#[tauri::command]
pub fn list_runtime_quota_probes() -> Result<Vec<RuntimeQuotaProbeRow>, String> {
    let bin = resolve_ato_binary()?;
    let out = Command::new(&bin)
        .args(["runtimes", "status", "--with-quota"])
        .output()
        .map_err(|e| format!("spawn ato: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(format!(
            "ato runtimes status --with-quota failed: {}",
            stderr.trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }
    let env: StatusEnvelope = serde_json::from_str(stdout.trim()).map_err(|e| {
        format!(
            "parse ato output: {} (raw: {})",
            e,
            truncate_for_toast(stdout.trim(), 500)
        )
    })?;
    Ok(env
        .runtime_quota_probes
        .unwrap_or_default()
        .into_iter()
        .map(|p| RuntimeQuotaProbeRow {
            runtime: p.runtime,
            source_path: p.source_path,
            found: p.found,
            messages_used: p.messages_used,
            messages_limit: p.messages_limit,
            period_reset_at: p.period_reset_at,
            note: p.note,
        })
        .collect())
}

/// Cap raw CLI stdout for inclusion in an error message that lands in
/// the panel's toast. The bound is in *characters* (≤ ~2 KB in
/// pathological UTF-8) — small enough for a toast, large enough to
/// diagnose the parse error. char_indices keeps the cut on a UTF-8
/// boundary so unicode payloads never panic the formatter.
fn truncate_for_toast(raw: &str, max_chars: usize) -> String {
    let split = raw
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(raw.len());
    if split < raw.len() {
        format!("{}…[truncated, {} total bytes]", &raw[..split], raw.len())
    } else {
        raw.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_under_cap_is_lossless() {
        let s = "hello world";
        assert_eq!(truncate_for_toast(s, 500), s);
    }

    #[test]
    fn truncate_over_cap_appends_size_marker() {
        let s = "a".repeat(1000);
        let out = truncate_for_toast(&s, 500);
        assert!(out.starts_with(&"a".repeat(500)));
        assert!(out.contains("[truncated, 1000 total bytes]"));
    }

    #[test]
    fn truncate_never_panics_on_multibyte_chars() {
        // 1000 × "é" (2 bytes each in UTF-8) — byte-indexed slicing
        // would land mid-char and panic. char_indices keeps us safe.
        let s = "é".repeat(1000);
        let out = truncate_for_toast(&s, 500);
        // Round-trips as valid UTF-8 by virtue of being a String at all.
        assert!(out.contains("é"));
        assert!(out.contains("[truncated, 2000 total bytes]"));
    }
}
