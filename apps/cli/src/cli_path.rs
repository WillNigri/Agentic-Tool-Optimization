// v2.11 PR-12.6 — Shared `ato` binary discovery.
//
// `methodology::runner::dispatch_cell` shells out to spawn `ato
// dispatch …`. Until this PR it hard-coded `std::env::current_exe()`,
// which meant a developer running ATO from a fresh `cargo build` could
// never reach API providers — the dev binary's keychain ACL on macOS
// is different from the prod-app-bundle's, so decryption of stored
// provider keys silently failed.
//
// Will's correction (2026-05-25): the keys aren't broken; the dev
// binary's identity is. Pointing the spawn at the prod binary fixes
// every API provider in one move.
//
// This module mirrors the resolution chain `commands/cron.rs` /
// `apps/desktop/src-tauri/src/commands/mod.rs` already use for cron
// jobs that dispatch headlessly. Same env-var name, same fallbacks,
// same precedence so a customer who sets ATO_CLI_PATH once gets the
// same behavior across cron, methodology runner, and diagnose.
//
// Resolution chain (first hit wins):
//   1. $ATO_CLI_PATH override.
//   2. `which("ato")` (PATH lookup).
//   3. /opt/homebrew/bin/ato       (Apple Silicon Homebrew default).
//   4. /usr/local/bin/ato          (Intel Homebrew / manual installs).
//   5. /Applications/ATO.app/Contents/MacOS/ato (macOS app bundle).
//   6. std::env::current_exe()     (the calling binary itself).
//
// (6) is the safety net so the runner can still operate in CI / sandboxed
// environments where none of (1)-(5) resolve — the existing dev-build
// limitation re-asserts, but we never crash.

use anyhow::Result;
use std::path::PathBuf;

pub fn resolve_ato_binary() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("ATO_CLI_PATH") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    if let Ok(p) = which::which("ato") {
        return Ok(p);
    }
    for candidate in &[
        "/opt/homebrew/bin/ato",
        "/usr/local/bin/ato",
        "/Applications/ATO.app/Contents/MacOS/ato",
    ] {
        if std::path::Path::new(candidate).exists() {
            return Ok(PathBuf::from(candidate));
        }
    }
    // Last resort — the binary that called this function. Will fall
    // back to whatever `current_exe()` decrypts (dev binary in `cargo
    // build` contexts, prod binary in installed contexts).
    std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("could not resolve any `ato` binary: current_exe failed ({})", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ato_cli_path_env_var_takes_precedence() {
        std::env::set_var("ATO_CLI_PATH", "/tmp/explicit-override");
        let p = resolve_ato_binary().unwrap();
        assert_eq!(p, PathBuf::from("/tmp/explicit-override"));
        std::env::remove_var("ATO_CLI_PATH");
    }

    #[test]
    fn empty_ato_cli_path_falls_through_to_next_candidate() {
        std::env::set_var("ATO_CLI_PATH", "");
        // Should NOT return "" — should fall through to `which`/
        // homebrew/app-bundle/current_exe. We can't assert the exact
        // value here because it depends on the test runner's machine,
        // but we can assert it's not the empty string.
        let p = resolve_ato_binary().unwrap();
        assert_ne!(p, PathBuf::from(""));
        std::env::remove_var("ATO_CLI_PATH");
    }

    #[test]
    fn resolve_returns_something_resolvable_on_default_machine() {
        // Belt-and-suspenders: without any env override, the chain
        // must produce SOMETHING (even if it's just `current_exe()`).
        std::env::remove_var("ATO_CLI_PATH");
        let p = resolve_ato_binary().unwrap();
        assert!(!p.as_os_str().is_empty());
    }
}
