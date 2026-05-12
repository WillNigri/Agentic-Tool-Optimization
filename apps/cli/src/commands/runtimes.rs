// v2.3.33 Phase 6.x-I — Runtime-binary health check.
//
// Why this exists: when ATO spawns a runtime CLI whose Developer ID
// cert has been revoked (or which is quarantined / unsigned), macOS
// either pops a generic malware dialog and silently kills the parent
// process (cert revocation), or refuses to exec (quarantine). The
// user sees ATO crash or a confusing error with no actionable path.
//
// This module runs `codesign --verify --verbose=2 <path>` + reads
// `com.apple.quarantine` xattr for every detected runtime, classifies
// the result, and returns a per-runtime health row with a concrete
// fix command when applicable. Agents read the JSON output; the
// desktop reads the same shape via a future Tauri command to drive
// an in-app banner.
//
// On non-macOS hosts the check is a no-op (status = "ok") since
// Gatekeeper / xattr / codesign only apply to Darwin.

use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

use crate::output::{emit_human, emit_json, Opts};

/// Runtimes we know how to spawn locally (api providers are excluded
/// — they're network-only and don't have a binary to verify).
const RUNTIMES: &[&str] = &["claude", "codex", "gemini", "hermes", "openclaw"];

#[derive(Debug, Serialize)]
pub struct HealthRow {
    pub runtime: &'static str,
    pub binary_path: Option<String>,
    /// One of: `ok`, `missing`, `revoked`, `quarantined`, `unsigned`,
    /// `unknown`. `ok` means the binary is on PATH and passes
    /// codesign / xattr checks. `unknown` covers the unlikely case
    /// where codesign itself fails to run.
    pub status: String,
    /// Human-readable detail (the relevant snippet from codesign /
    /// the xattr value). Useful for agents that want to render an
    /// explanation without re-running the check.
    pub detail: Option<String>,
    /// Shell one-liner the user can run to fix the issue. None when
    /// the runtime is healthy or when there's no canned fix (e.g.
    /// unsigned third-party CLI — the right action depends).
    pub fix_command: Option<String>,
}

pub fn run_health_check(opts: &Opts) -> Result<()> {
    let rows: Vec<HealthRow> = RUNTIMES.iter().map(|r| check_one(r)).collect();
    if opts.human {
        emit_human(&format!("Runtime health: {} checked", rows.len()));
        for r in &rows {
            let tag = match r.status.as_str() {
                "ok" => "✓ ok",
                "missing" => "—  not installed",
                "revoked" => "✗  cert REVOKED",
                "quarantined" => "✗  quarantined",
                "unsigned" => "?  unsigned",
                _ => "?  unknown",
            };
            emit_human(&format!(
                "  {:10} {}{}",
                r.runtime,
                tag,
                r.binary_path
                    .as_deref()
                    .map(|p| format!("  ({})", p))
                    .unwrap_or_default(),
            ));
            if let Some(d) = &r.detail {
                emit_human(&format!("             reason: {}", d));
            }
            if let Some(fix) = &r.fix_command {
                emit_human(&format!("             fix:    {}", fix));
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

fn check_one(runtime: &'static str) -> HealthRow {
    let binary = match which::which(runtime) {
        Ok(p) => p,
        Err(_) => {
            return HealthRow {
                runtime,
                binary_path: None,
                status: "missing".into(),
                detail: Some(format!("'{}' not found on PATH", runtime)),
                fix_command: install_command_for(runtime).map(String::from),
            };
        }
    };

    // Non-macOS: nothing useful to check; if which() found it, call
    // it OK. Linux/Windows users don't hit Gatekeeper issues.
    if !cfg!(target_os = "macos") {
        return HealthRow {
            runtime,
            binary_path: Some(binary.display().to_string()),
            status: "ok".into(),
            detail: None,
            fix_command: None,
        };
    }

    // Quarantine xattr is the fast-fail check — if the binary was
    // downloaded by a browser / curl with the flag set, Gatekeeper
    // will block exec on first run.
    if let Some(qval) = read_quarantine_xattr(&binary) {
        return HealthRow {
            runtime,
            binary_path: Some(binary.display().to_string()),
            status: "quarantined".into(),
            detail: Some(format!("com.apple.quarantine = {}", qval)),
            fix_command: Some(format!(
                "xattr -d com.apple.quarantine {}",
                shell_quote(&binary.display().to_string())
            )),
        };
    }

    // codesign --verify catches the headline case: revoked Developer
    // ID certs (CSSMERR_TP_CERT_REVOKED). Triggered on Will's machine
    // 2026-05-11 when Apple revoked OpenAI's signing cert.
    let codesign_out = Command::new("codesign")
        .args(["--verify", "--verbose=2", &binary.display().to_string()])
        .output();
    match codesign_out {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if stderr.contains("CSSMERR_TP_CERT_REVOKED") {
                HealthRow {
                    runtime,
                    binary_path: Some(binary.display().to_string()),
                    status: "revoked".into(),
                    detail: Some(extract_codesign_reason(&stderr)),
                    fix_command: install_command_for(runtime).map(String::from),
                }
            } else if stderr.contains("not signed at all") {
                HealthRow {
                    runtime,
                    binary_path: Some(binary.display().to_string()),
                    status: "unsigned".into(),
                    detail: Some("code object is not signed at all".into()),
                    // No automatic fix — unsigned third-party CLIs can
                    // still run; we just flag it so the user knows.
                    fix_command: None,
                }
            } else if out.status.success() {
                HealthRow {
                    runtime,
                    binary_path: Some(binary.display().to_string()),
                    status: "ok".into(),
                    detail: None,
                    fix_command: None,
                }
            } else {
                HealthRow {
                    runtime,
                    binary_path: Some(binary.display().to_string()),
                    status: "unknown".into(),
                    detail: Some(extract_codesign_reason(&stderr)),
                    fix_command: None,
                }
            }
        }
        Err(e) => HealthRow {
            runtime,
            binary_path: Some(binary.display().to_string()),
            status: "unknown".into(),
            detail: Some(format!("codesign invocation failed: {}", e)),
            fix_command: None,
        },
    }
}

fn read_quarantine_xattr(path: &PathBuf) -> Option<String> {
    let out = Command::new("xattr")
        .args(["-p", "com.apple.quarantine", &path.display().to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Pull the most informative line out of codesign's stderr. Falls
/// back to the whole blob trimmed to ~200 chars if no specific marker
/// is present.
fn extract_codesign_reason(stderr: &str) -> String {
    for line in stderr.lines() {
        let l = line.trim();
        if l.contains("CSSMERR_") || l.starts_with("Sealed Resources") || l.contains("invalid") {
            return l.to_string();
        }
    }
    let trimmed = stderr.trim();
    if trimmed.len() > 200 {
        format!("{}…", &trimmed[..200])
    } else {
        trimmed.to_string()
    }
}

/// Canned install/reinstall command per runtime. Used as the fix
/// suggestion for both `missing` and `revoked`, since both are
/// resolved by reinstalling from npm (which pulls a freshly-signed
/// binary).
fn install_command_for(runtime: &str) -> Option<&'static str> {
    match runtime {
        "claude" => Some("npm install -g @anthropic-ai/claude-code@latest"),
        "codex" => Some("npm install -g @openai/codex@latest"),
        "gemini" => Some("npm install -g @google/gemini-cli@latest"),
        // Hermes / OpenClaw aren't on npm; users install via their
        // own docs. Leaving fix_command None nudges them to the
        // upstream README rather than running a wrong-shape command.
        _ => None,
    }
}

fn shell_quote(s: &str) -> String {
    if s.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '/' | '.' | '_' | '-' | '~')
    }) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_commands_for_known_runtimes() {
        assert!(install_command_for("claude").unwrap().contains("claude-code"));
        assert!(install_command_for("codex").unwrap().contains("codex"));
        assert!(install_command_for("gemini").unwrap().contains("gemini-cli"));
        assert!(install_command_for("hermes").is_none());
        assert!(install_command_for("openclaw").is_none());
    }

    #[test]
    fn extracts_revoked_cert_line() {
        let stderr = "/usr/local/bin/codex: code is signed but…\n\
                      /usr/local/bin/codex: CSSMERR_TP_CERT_REVOKED";
        assert!(extract_codesign_reason(stderr).contains("CSSMERR_TP_CERT_REVOKED"));
    }

    #[test]
    fn shell_quote_passes_simple_paths_through() {
        assert_eq!(shell_quote("/usr/local/bin/codex"), "/usr/local/bin/codex");
    }

    #[test]
    fn shell_quote_wraps_spaces() {
        let q = shell_quote("/path with space/bin");
        assert_eq!(q, "'/path with space/bin'");
    }
}
