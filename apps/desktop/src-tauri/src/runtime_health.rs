// v2.3.36 Phase 6.x-I.2 — Runtime-binary health check for the desktop.
//
// Mirrors the logic in apps/cli/src/commands/runtimes.rs. Same shape,
// same status values, same fix commands. Lives in the desktop crate
// instead of being shelled-out so the banner doesn't depend on the
// `ato` binary being on the user's PATH (chicken-and-egg if `ato`
// itself is the runtime that got cert-revoked).
//
// Triggered by Will hitting CSSMERR_TP_CERT_REVOKED on codex on
// 2026-05-11. CLI side shipped v2.3.34; this is the GUI banner half.

use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

const RUNTIMES: &[&str] = &["claude", "codex", "gemini", "hermes", "openclaw"];

#[derive(Debug, Serialize, Clone)]
pub struct RuntimeHealthRow {
    pub runtime: &'static str,
    pub binary_path: Option<String>,
    /// `ok` / `missing` / `revoked` / `quarantined` / `unsigned` / `unknown`.
    pub status: String,
    pub detail: Option<String>,
    /// Canned shell one-liner the GUI's "Run fix" button executes.
    pub fix_command: Option<String>,
}

#[tauri::command]
pub fn runtime_health_check() -> Vec<RuntimeHealthRow> {
    RUNTIMES.iter().map(|r| check_one(r)).collect()
}

/// Tauri command that executes a fix_command from a previous health
/// check. We don't accept arbitrary shell strings — the input must
/// match one of the known fix shapes. Anything else is rejected so a
/// compromised IPC channel can't run arbitrary commands.
#[tauri::command]
pub fn runtime_health_run_fix(fix_command: String) -> Result<String, String> {
    // Allowlist: the only shapes we ever emit are
    //   1. `npm install -g <pkg>@latest`
    //   2. `xattr -d com.apple.quarantine <path>`
    // We re-parse and re-execute via Command::new with split args,
    // rather than spawning `sh -c <untrusted>`, so even if a future
    // bug emits a weird fix_command the worst case is an exec
    // failure, not an arbitrary-command-injection.
    if let Some(rest) = fix_command.strip_prefix("npm install -g ") {
        let pkg = rest.trim();
        if !is_safe_npm_pkg(pkg) {
            return Err(format!("Refusing to run npm install with suspicious package: {}", pkg));
        }
        run_capture(Command::new("npm").args(["install", "-g", pkg]))
    } else if let Some(rest) = fix_command.strip_prefix("xattr -d com.apple.quarantine ") {
        let path = rest.trim().trim_matches('\'');
        if path.is_empty() || path.contains(';') || path.contains('&') {
            return Err(format!("Refusing to run xattr with suspicious path: {}", path));
        }
        run_capture(Command::new("xattr").args(["-d", "com.apple.quarantine", path]))
    } else {
        Err(format!(
            "Refusing to execute unrecognized fix shape: {}",
            fix_command
        ))
    }
}

fn is_safe_npm_pkg(pkg: &str) -> bool {
    // Permits e.g. `@openai/codex@latest`, `@anthropic-ai/claude-code@latest`,
    // `@google/gemini-cli@latest`. Rejects shell metachars or whitespace.
    !pkg.is_empty()
        && pkg.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '-' | '_' | '.' )
        })
}

fn run_capture(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if out.status.success() {
        Ok(format!("{}\n{}", stdout, stderr).trim().to_string())
    } else {
        Err(format!(
            "exit {}: {}",
            out.status,
            stderr.trim().to_string()
        ))
    }
}

fn check_one(runtime: &'static str) -> RuntimeHealthRow {
    // Reuse the desktop's existing PATH-resolver — it already handles
    // user overrides, login-shell PATH augmentation, and the common
    // install locations (npm global, npx, etc.). Avoids depending on
    // the `which` crate just for one call.
    let binary: PathBuf = match crate::commands::which_cli(runtime) {
        Some(p) => PathBuf::from(p),
        None => {
            return RuntimeHealthRow {
                runtime,
                binary_path: None,
                status: "missing".into(),
                detail: Some(format!("'{}' not found on PATH", runtime)),
                fix_command: install_command_for(runtime).map(String::from),
            };
        }
    };

    if !cfg!(target_os = "macos") {
        return RuntimeHealthRow {
            runtime,
            binary_path: Some(binary.display().to_string()),
            status: "ok".into(),
            detail: None,
            fix_command: None,
        };
    }

    if let Some(qval) = read_quarantine_xattr(&binary) {
        return RuntimeHealthRow {
            runtime,
            binary_path: Some(binary.display().to_string()),
            status: "quarantined".into(),
            detail: Some(format!("com.apple.quarantine = {}", qval)),
            fix_command: Some(format!(
                "xattr -d com.apple.quarantine '{}'",
                binary.display()
            )),
        };
    }

    let codesign_out = Command::new("codesign")
        .args(["--verify", "--verbose=2", &binary.display().to_string()])
        .output();
    match codesign_out {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if stderr.contains("CSSMERR_TP_CERT_REVOKED") {
                RuntimeHealthRow {
                    runtime,
                    binary_path: Some(binary.display().to_string()),
                    status: "revoked".into(),
                    detail: Some(extract_codesign_reason(&stderr)),
                    fix_command: install_command_for(runtime).map(String::from),
                }
            } else if stderr.contains("not signed at all") {
                RuntimeHealthRow {
                    runtime,
                    binary_path: Some(binary.display().to_string()),
                    status: "unsigned".into(),
                    detail: Some("code object is not signed at all".into()),
                    fix_command: None,
                }
            } else if out.status.success() {
                RuntimeHealthRow {
                    runtime,
                    binary_path: Some(binary.display().to_string()),
                    status: "ok".into(),
                    detail: None,
                    fix_command: None,
                }
            } else {
                RuntimeHealthRow {
                    runtime,
                    binary_path: Some(binary.display().to_string()),
                    status: "unknown".into(),
                    detail: Some(extract_codesign_reason(&stderr)),
                    fix_command: None,
                }
            }
        }
        Err(e) => RuntimeHealthRow {
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

fn extract_codesign_reason(stderr: &str) -> String {
    for line in stderr.lines() {
        let l = line.trim();
        if l.contains("CSSMERR_") || l.contains("invalid") || l.starts_with("Sealed Resources") {
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

#[cfg(test)]
mod tests {
    use super::*;

    // The allowlist parser is the load-bearing security boundary for
    // the "Run fix" button. Anything that gets past these tests would
    // let a compromised IPC channel run arbitrary shell. Test both
    // accepted shapes and a spread of rejected ones.

    #[test]
    fn accepts_canonical_npm_install() {
        // We can't run npm in a unit test, but we can verify the
        // parser routes to the npm branch instead of erroring out.
        // The actual exec will fail in the test (npm-i to a fake pkg
        // would either succeed or fail at the network layer); we only
        // care that the rejection path doesn't fire.
        let safe = is_safe_npm_pkg("@openai/codex@latest");
        let safe2 = is_safe_npm_pkg("@anthropic-ai/claude-code@latest");
        let safe3 = is_safe_npm_pkg("@google/gemini-cli@latest");
        assert!(safe);
        assert!(safe2);
        assert!(safe3);
    }

    #[test]
    fn rejects_shell_metacharacters_in_pkg() {
        assert!(!is_safe_npm_pkg("foo; rm -rf /"));
        assert!(!is_safe_npm_pkg("foo && whoami"));
        assert!(!is_safe_npm_pkg("$(curl evil.sh)"));
        assert!(!is_safe_npm_pkg("foo|cat"));
        assert!(!is_safe_npm_pkg("foo bar"));   // space
        assert!(!is_safe_npm_pkg("foo`id`"));
        assert!(!is_safe_npm_pkg(""));
    }

    #[test]
    fn unrecognized_shapes_are_rejected() {
        // Anything that isn't one of the two allowlisted prefixes
        // must short-circuit before exec.
        let cases = [
            "echo pwned",
            "/bin/sh -c whoami",
            "rm -rf ~",
            "npm install -gX foo",          // mangled prefix
            "xattr -d com.apple.quarantine; rm -rf /",  // injection attempt
            "",
        ];
        for s in cases {
            let result = runtime_health_run_fix(s.to_string());
            assert!(
                matches!(&result, Err(msg) if msg.contains("Refusing")),
                "expected refusal for input {:?}, got {:?}",
                s,
                result
            );
        }
    }

    #[test]
    fn xattr_rejects_path_with_semicolons() {
        let r = runtime_health_run_fix(
            "xattr -d com.apple.quarantine '/tmp/foo; rm -rf /'".into(),
        );
        assert!(
            matches!(&r, Err(msg) if msg.contains("suspicious path") || msg.contains("Refusing")),
            "got {:?}",
            r
        );
    }

    #[test]
    fn xattr_accepts_clean_path() {
        // We can't actually have a quarantined file to remove here,
        // so the exec will fail. The parser must NOT reject it
        // before exec — that's the contract being tested.
        let r = runtime_health_run_fix(
            "xattr -d com.apple.quarantine /tmp/this-file-does-not-exist".into(),
        );
        // Either Ok (xattr happened to succeed) or Err with a real
        // exec error (not a "Refusing" allowlist rejection). Allowlist
        // rejections are the bug; real exec errors are fine.
        if let Err(msg) = &r {
            assert!(
                !msg.contains("Refusing"),
                "allowlist incorrectly rejected clean path: {}",
                msg
            );
        }
    }
}

fn install_command_for(runtime: &str) -> Option<&'static str> {
    match runtime {
        "claude" => Some("npm install -g @anthropic-ai/claude-code@latest"),
        "codex" => Some("npm install -g @openai/codex@latest"),
        "gemini" => Some("npm install -g @google/gemini-cli@latest"),
        _ => None,
    }
}
