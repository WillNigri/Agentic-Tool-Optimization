// `ato setup-path` — make the CLI reachable from any shell.
//
// Without `ato` on PATH, no coding agent can shell out to it. Every
// command we shipped — dispatch, replay, kill, observation — returns
// "command not found." This command is the bridge between "binary
// ships inside the bundle" and "agent can use it."
//
// Behavior:
//   - Detects the current binary's path (works for both Tauri-bundle
//     sidecar path and `cargo install`-style installs)
//   - Checks whether `ato` already resolves on PATH
//   - If it points at the same binary → no-op (already set up)
//   - If it points elsewhere → refuse to overwrite (the user has a
//     different `ato` and we shouldn't surprise them)
//   - Otherwise tries to symlink to /usr/local/bin/ato; falls back
//     to ~/.local/bin/ato when /usr/local/bin isn't writable
//
// Windows: not yet implemented in this subcommand. Tauri's NSIS bundle
// has an "Add to PATH" option that handles the Windows install-time
// path. Linux .deb's postinst handles the deb path. macOS users run
// this command (or click the button in the desktop's first-launch
// dialog that runs it).

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct SetupPathResult {
    /// One of: "already-on-path", "installed-to-path", "not-on-path-and-not-installed",
    /// "path-points-elsewhere", "windows-unsupported"
    pub status: String,
    /// The binary the CLI was run from (canonicalized).
    pub binary_path: PathBuf,
    /// Where `ato` resolves on PATH right now (if anywhere).
    pub current_path_resolution: Option<PathBuf>,
    /// If a symlink was created, where it lives.
    pub installed_to: Option<PathBuf>,
    /// Whether the install location is itself already on PATH. False
    /// means the user needs to also add the directory to PATH.
    pub install_dir_on_path: Option<bool>,
    /// One-line human-readable summary of what happened.
    pub note: String,
}

pub fn run(check: bool, dir_override: Option<PathBuf>, force: bool, opts: &Opts) -> Result<()> {
    let current_exe = std::env::current_exe()
        .context("Could not resolve the current binary's path")?;
    let current_exe = fs::canonicalize(&current_exe).unwrap_or(current_exe);

    let existing = which::which("ato").ok();
    let existing_canon = existing
        .as_ref()
        .and_then(|p| fs::canonicalize(p).ok());

    // 1. Already-on-path same binary.
    if existing_canon.as_ref() == Some(&current_exe) {
        return emit(
            SetupPathResult {
                status: "already-on-path".to_string(),
                binary_path: current_exe.clone(),
                current_path_resolution: existing.clone(),
                installed_to: None,
                install_dir_on_path: Some(true),
                note: format!(
                    "ato is already resolvable at {}",
                    existing.as_ref().unwrap().display()
                ),
            },
            opts,
        );
    }

    // 2. A different binary is on PATH. Don't clobber without --force.
    if let Some(other) = existing.clone() {
        if !force {
            return emit(
                SetupPathResult {
                    status: "path-points-elsewhere".to_string(),
                    binary_path: current_exe,
                    current_path_resolution: Some(other.clone()),
                    installed_to: None,
                    install_dir_on_path: None,
                    note: format!(
                        "A different `ato` binary is already on PATH at {}. Refusing to overwrite. Pass --force to replace it.",
                        other.display()
                    ),
                },
                opts,
            );
        }
    }

    // 3. Just checking — don't make any changes.
    if check {
        return emit(
            SetupPathResult {
                status: "not-on-path-and-not-installed".to_string(),
                binary_path: current_exe.clone(),
                current_path_resolution: existing.clone(),
                installed_to: None,
                install_dir_on_path: None,
                note: format!(
                    "ato is not on PATH. Current binary: {}. Run `ato setup-path` (no --check) to install.",
                    current_exe.display()
                ),
            },
            opts,
        );
    }

    // 4. Windows — out of scope for this subcommand. The Tauri NSIS
    //    installer's addToPath handles Windows install-time PATH.
    #[cfg(windows)]
    {
        return emit(
            SetupPathResult {
                status: "windows-unsupported".to_string(),
                binary_path: current_exe,
                current_path_resolution: existing,
                installed_to: None,
                install_dir_on_path: None,
                note: "Windows PATH setup is handled by the installer. Reinstall the ATO desktop app via the NSIS installer with the 'Add to PATH' option enabled.".to_string(),
            },
            opts,
        );
    }

    // 5. Unix install. Try /usr/local/bin first; fall back to ~/.local/bin.
    #[cfg(unix)]
    {
        let candidates = build_candidates(dir_override);
        let mut attempts: Vec<String> = Vec::new();
        for dir in &candidates {
            match try_symlink_to(&current_exe, dir) {
                Ok(symlink_path) => {
                    let install_dir_on_path = path_contains(dir);
                    let note = if install_dir_on_path {
                        format!(
                            "Symlinked at {}. Open a new shell (or run `hash -r`) to pick it up.",
                            symlink_path.display()
                        )
                    } else {
                        format!(
                            "Symlinked at {}, but {} is not on PATH. Add this to your ~/.zshrc or ~/.bashrc: export PATH=\"{}:$PATH\"",
                            symlink_path.display(),
                            dir.display(),
                            dir.display()
                        )
                    };
                    return emit(
                        SetupPathResult {
                            status: "installed-to-path".to_string(),
                            binary_path: current_exe.clone(),
                            current_path_resolution: existing.clone(),
                            installed_to: Some(symlink_path),
                            install_dir_on_path: Some(install_dir_on_path),
                            note,
                        },
                        opts,
                    );
                }
                Err(e) => attempts.push(format!("  {}: {}", dir.display(), e)),
            }
        }
        return Err(anyhow!(
            "Failed to install — no candidate directory was writable. Tried:\n{}\n\nTry passing --dir <path> with a directory you own.",
            attempts.join("\n")
        ));
    }

    #[allow(unreachable_code)]
    Ok(())
}

fn emit(result: SetupPathResult, opts: &Opts) -> Result<()> {
    if opts.human {
        emit_human(&format!("[{}] {}", result.status, result.note));
        if let Some(p) = &result.installed_to {
            emit_human(&format!("  symlink: {}", p.display()));
        }
        emit_human(&format!("  source:  {}", result.binary_path.display()));
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

#[cfg(unix)]
fn build_candidates(override_dir: Option<PathBuf>) -> Vec<PathBuf> {
    if let Some(d) = override_dir {
        return vec![d];
    }
    let mut v: Vec<PathBuf> = Vec::new();
    v.push(PathBuf::from("/usr/local/bin"));
    if let Ok(home) = std::env::var("HOME") {
        v.push(PathBuf::from(format!("{}/.local/bin", home)));
    }
    v
}

#[cfg(unix)]
fn try_symlink_to(source: &Path, dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let dest = dir.join("ato");
    // If something already lives at the destination, we got here only
    // because the caller passed --force (the early branch wouldn't have
    // let us through otherwise when a different `ato` was on PATH).
    // Remove it before relinking.
    if dest.exists() || dest.symlink_metadata().is_ok() {
        fs::remove_file(&dest)?;
    }
    std::os::unix::fs::symlink(source, &dest)?;
    Ok(dest)
}

#[cfg(unix)]
fn path_contains(dir: &Path) -> bool {
    let path_env = std::env::var("PATH").unwrap_or_default();
    let canon_dir = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    path_env.split(':').any(|p| {
        let p = Path::new(p);
        let canon = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        canon == canon_dir
    })
}
