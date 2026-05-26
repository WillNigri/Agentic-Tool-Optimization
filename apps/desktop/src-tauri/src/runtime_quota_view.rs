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
    runtime_quota_probes: Vec<RawProbe>,
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
    let env: StatusEnvelope = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("parse ato output: {} (raw: {})", e, stdout))?;
    Ok(env
        .runtime_quota_probes
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
