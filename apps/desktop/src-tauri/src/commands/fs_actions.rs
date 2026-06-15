// commands/fs_actions.rs — OS-level path actions for the receipts cluster.
//
// Three small commands the frontend's ClickablePath pill leans on:
//   - `reveal_path`  — show a path in the OS file manager (selected)
//   - `open_path`    — open a path with its default handler
//   - `path_exists`  — does the path still resolve on disk?
//
// These shell out to the platform's native opener (`open` on macOS,
// `xdg-open` on Linux, `explorer` on Windows). Codex R4 hardened the
// surface in two ways:
//   1. `open_path` on Windows used `cmd /C start "" <path>`, so the
//      input was parsed by cmd.exe — a crafted path containing shell
//      metacharacters could become command injection. Now uses
//      `explorer.exe <path>` directly (no shell interpretation).
//   2. macOS `open` and Linux `xdg-open` are generic URI launchers
//      (they happily fire `http://`, `mailto:`, custom URL schemes).
//      We now validate that the input resolves to a real filesystem
//      path before spawning — scheme-like strings are rejected.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Codex R4 — reject URI-like inputs before spawning a native opener.
/// macOS `open` and Linux `xdg-open` will happily fire `http://...`,
/// `mailto:...`, and custom URL schemes; this surface is meant to
/// reveal/open local files only, so anything that doesn't canonicalize
/// to a real filesystem entry is refused with a clear error.
fn validate_local_path(raw: &str) -> Result<PathBuf, String> {
    // Trim — Tauri can pass paths with stray whitespace from JS callers.
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Empty path".to_string());
    }
    // Cheap scheme-prefix sniff that catches the common abuses without
    // false-positiving on Windows drive letters (`C:\...`). A scheme is
    // <2+ alpha chars>:<non-backslash>, drive letter is single letter
    // followed by `:\` or `:/`.
    if let Some(colon) = trimmed.find(':') {
        let scheme = &trimmed[..colon];
        let rest = &trimmed[colon + 1..];
        let is_drive_letter = scheme.len() == 1
            && scheme.chars().all(|c| c.is_ascii_alphabetic())
            && (rest.starts_with('\\') || rest.starts_with('/'));
        let looks_like_scheme = scheme.len() >= 2
            && scheme.chars().all(|c| c.is_ascii_alphabetic() || c == '+' || c == '-' || c == '.');
        if looks_like_scheme && !is_drive_letter {
            return Err(format!("Refusing to open non-filesystem URI: {scheme}:..."));
        }
    }
    let p = Path::new(trimmed);
    // The path must currently resolve. Symlinks are allowed because
    // canonicalize follows them; broken symlinks fail here, same as a
    // nonexistent path.
    let canonical = p
        .canonicalize()
        .map_err(|e| format!("Path does not resolve: {e}"))?;
    Ok(canonical)
}

/// Reveal a path in the OS file manager, selecting the entry itself.
///
/// macOS: `open -R <path>` (Finder, item selected).
/// Linux: `xdg-open <parent dir>` (no portable "select" verb, so we
///        open the containing directory).
/// Windows: `explorer /select,<path>`.
#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    let canonical = validate_local_path(&path)?;
    let canonical_str = canonical.to_string_lossy().into_owned();

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-R", &canonical_str])
            .spawn()
            .map_err(|e| format!("Failed to reveal path: {e}"))?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        // xdg-open has no "select this file" mode, so reveal the parent dir.
        let target = canonical
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or(canonical_str);
        Command::new("xdg-open")
            .arg(&target)
            .spawn()
            .map_err(|e| format!("Failed to reveal path: {e}"))?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        // `/select,<path>` must be a single argument to explorer.
        Command::new("explorer")
            .arg(format!("/select,{canonical_str}"))
            .spawn()
            .map_err(|e| format!("Failed to reveal path: {e}"))?;
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = canonical_str;
        Err("reveal_path is not supported on this platform".to_string())
    }
}

/// Open a path with the OS default handler (folder in file manager,
/// file in its associated app).
///
/// macOS: `open <path>`. Linux: `xdg-open <path>`. Windows: `explorer
/// <path>` (no `cmd /C start` — codex R4 flagged that route as a shell-
/// injection vector since paths went through cmd.exe).
#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    let canonical = validate_local_path(&path)?;
    let canonical_str = canonical.to_string_lossy().into_owned();

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&canonical_str)
            .spawn()
            .map_err(|e| format!("Failed to open path: {e}"))?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(&canonical_str)
            .spawn()
            .map_err(|e| format!("Failed to open path: {e}"))?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(&canonical_str)
            .spawn()
            .map_err(|e| format!("Failed to open path: {e}"))?;
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = canonical_str;
        Err("open_path is not supported on this platform".to_string())
    }
}

/// Does this path currently resolve on disk? Used by the existence pill.
///
/// Uses `fs::metadata` (follows symlinks) rather than `Path::exists` so a
/// permission error surfaces as `false` consistently with `exists()`'s
/// own behavior, while keeping the intent explicit.
#[tauri::command]
pub fn path_exists(path: String) -> bool {
    std::fs::metadata(Path::new(&path)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_http_scheme() {
        assert!(validate_local_path("http://evil.com/x").is_err());
        assert!(validate_local_path("https://evil.com/x").is_err());
        assert!(validate_local_path("mailto:a@b.com").is_err());
        assert!(validate_local_path("file:///etc/passwd").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(validate_local_path("").is_err());
        assert!(validate_local_path("   ").is_err());
    }

    #[test]
    fn accepts_existing_file() {
        // /etc/hosts exists on macOS + Linux test machines; on Windows
        // this test is skipped because the path doesn't exist there.
        #[cfg(unix)]
        {
            let result = validate_local_path("/etc/hosts");
            assert!(result.is_ok(), "expected /etc/hosts to validate: {:?}", result);
        }
    }

    #[test]
    fn allows_windows_drive_letter() {
        // We can't actually canonicalize `C:\...` on a Unix host, but
        // the scheme-prefix sniff must NOT reject it before the
        // canonicalize step. Use a unit-level test that re-implements
        // only the scheme check to keep the test cross-platform.
        let input = r"C:\Users\foo\bar";
        let colon = input.find(':').unwrap();
        let scheme = &input[..colon];
        let rest = &input[colon + 1..];
        let is_drive_letter = scheme.len() == 1
            && scheme.chars().all(|c| c.is_ascii_alphabetic())
            && (rest.starts_with('\\') || rest.starts_with('/'));
        assert!(is_drive_letter, "C:\\... must be classified as a drive letter, not a scheme");
    }
}
