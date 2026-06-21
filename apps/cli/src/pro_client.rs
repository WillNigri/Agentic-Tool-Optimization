// Thin client that delegates Pro-tier methodology features to the
// `ato-pro` binary (distributed only to Pro subscribers; not in the
// OSS release artifacts).
//
// Why this module exists:
//   v2.11 PR-12.8 moved the methodology diagnose + --apply
//   implementations out of this repo into the private ato-cloud Rust
//   workspace. The OSS CLI keeps the user-facing `ato evaluations
//   methodology diagnose` subcommand for muscle-memory and discovery,
//   but the implementation now subprocess-delegates to `ato-pro`.
//
// Discovery order for the Pro binary:
//   1. $ATO_PRO_PATH        (explicit override)
//   2. ~/.ato/bin/ato-pro   (default install location used by
//                            `ato pro install`)
//   3. `which ato-pro`      (in case the user dropped it on $PATH
//                            themselves)
//
// If none resolve, the customer sees a single sentence pointing them
// at the dashboard. We deliberately do not 503 silently — the gate is
// explicit so it's obvious whose job it is to fix the missing piece.

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::process::Command;

const ATO_PRO_DOWNLOAD_URL: &str = "https://agentictool.ai/account#download-pro";

pub fn locate_pro_binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ATO_PRO_PATH") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    let mut default = crate::db::home_dir();
    default.push(".ato");
    default.push("bin");
    default.push("ato-pro");
    if default.exists() {
        return Some(default);
    }
    which::which("ato-pro").ok()
}

pub fn require_pro_binary() -> Result<PathBuf> {
    locate_pro_binary().ok_or_else(|| {
        anyhow!(
            "This command is part of ATO Pro and requires the `ato-pro` binary, which is not installed.\n\
             \n\
             Install:    ato pro install     (downloads + verifies the binary; requires Pro subscription)\n\
             Subscribe:  {}\n\
             \n\
             What you get: codified diagnose proposals against your methodology runs, safe `--apply` with\n\
             lineage tracking, and scheduled re-runs. The OSS CLI keeps the surface; the heavy lifting\n\
             lives in the Pro binary so we can ship it without leaking the implementation.",
            ATO_PRO_DOWNLOAD_URL
        )
    })
}

/// Forward a methodology subcommand verbatim to `ato-pro`. The CLI
/// flags here are the same shape that `ato-pro --help` accepts — we
/// don't reparse, we just hand the user's args through and let the
/// Pro binary do the work.
pub fn delegate(subcommand: &str, args: &[String], db_path: &std::path::Path, human: bool, quiet: bool) -> Result<()> {
    let bin = require_pro_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.arg(subcommand);
    for a in args {
        cmd.arg(a);
    }
    cmd.arg("--db").arg(db_path);
    if human {
        cmd.arg("--human");
    }
    if quiet {
        cmd.arg("--quiet");
    }
    let status = cmd
        .status()
        .map_err(|e| anyhow!("spawn ato-pro at {}: {}", bin.display(), e))?;
    if !status.success() {
        anyhow::bail!(
            "ato-pro {} failed with exit code {:?}",
            subcommand,
            status.code()
        );
    }
    Ok(())
}

pub fn delegate_capture(
    subcommand: &str,
    args: &[String],
    db_path: &std::path::Path,
) -> Result<String> {
    let bin = require_pro_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.arg(subcommand);
    for a in args {
        cmd.arg(a);
    }
    cmd.arg("--db").arg(db_path);
    let output = cmd
        .output()
        .map_err(|e| anyhow!("spawn ato-pro at {}: {}", bin.display(), e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(
            "ato-pro {} failed with exit code {:?}: {}",
            subcommand,
            output.status.code(),
            stderr
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
