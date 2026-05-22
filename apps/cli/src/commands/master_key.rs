// CLI mirror of the master_key_v2 lifecycle. PR-6.
//
// Today's only subcommand: `ato master-key export` — print the
// current OS-keychain master key to stdout, base64-encoded. Used
// to populate PR-5's "paste the old key" textarea on a different
// machine / install where the user can't drop to `security
// find-generic-password` directly (e.g. headless servers, Linux,
// Windows).
//
// Safety: `--confirm-i-understand-this-prints-the-key` flag
// required. The key in shell history is a real leakage risk; the
// flag exists so an `ato master-key` typo never accidentally
// exposes it. Output goes to stdout (not stderr) so it can be
// piped to `pbcopy` / `xclip` cleanly without the warning.
//
// PR-5's "rekey" UX consumes this output. Future PR-7 could add
// `ato master-key rekey --from-stdin` for headless rekey without
// the desktop UI; held until a real dogfood demand surfaces.

use crate::encryption;
use crate::output::Opts;
use anyhow::{anyhow, Result};
use rusqlite::Connection;

/// Print the master key (base64). Confirms the flag was set; refuses
/// otherwise with a hint about the safety rationale. Emits to stdout
/// only — the warning preamble goes to stderr so a pipe to
/// `pbcopy` / `xclip` captures ONLY the key bytes.
pub fn export(confirm: bool, _opts: &Opts) -> Result<()> {
    if !confirm {
        return Err(anyhow!(
            "refusing to print master key without `--confirm-i-understand-this-prints-the-key`.\n\
             \n\
             The base64 key prints to stdout and lands in shell history. Only run this if\n\
             you immediately pipe it to a secure paste destination (PR-5 rekey textarea,\n\
             pbcopy, etc.) and DON'T leave it in your scrollback.\n\
             \n\
             Re-run with: ato master-key export --confirm-i-understand-this-prints-the-key"
        ));
    }
    // Use the PR-6 wrapper rather than poking at private master_key().
    // Same keychain + cache + ATO_MASTER_KEY_B64 bypass paths apply.
    let b64 = encryption::export_master_key_b64()?;
    // Warning to stderr so stdout stays clean for piping.
    eprintln!(
        "[master-key export] Printing master_key_v1 (32 bytes, base64). \
         This key decrypts every stored llm_api_keys row. Paste it into \
         PR-5's rekey textarea + then clear your shell history."
    );
    // Key to stdout.
    println!("{}", b64);
    Ok(())
}

/// Reserved for a future `ato master-key rekey --from-stdin` that
/// mirrors PR-4's desktop rekey transaction for headless contexts.
/// Held — PR-5's UI covers every dogfood case today; ship CLI rekey
/// when a real headless-server dogfood asks for it.
#[allow(dead_code)]
pub fn rekey_from_stdin(_conn: &Connection, _opts: &Opts) -> Result<()> {
    Err(anyhow!(
        "ato master-key rekey is not yet implemented. Use the desktop \
         app's rekey banner (PR-5) — or open an issue with your headless \
         dogfood scenario."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Opts;

    #[test]
    fn export_refuses_without_confirm_flag() {
        let opts = Opts { human: false, quiet: false };
        let err = export(false, &opts).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--confirm-i-understand-this-prints-the-key"),
            "error must name the safety flag for the user: {}",
            msg
        );
        assert!(
            msg.contains("shell history"),
            "error must mention the leakage concern: {}",
            msg
        );
    }

    #[test]
    fn rekey_stub_returns_not_implemented() {
        let conn = Connection::open_in_memory().unwrap();
        let opts = Opts { human: false, quiet: false };
        let err = rekey_from_stdin(&conn, &opts).unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }
}
