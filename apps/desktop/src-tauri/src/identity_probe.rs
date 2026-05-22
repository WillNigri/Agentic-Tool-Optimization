// Per-OS identity probe + ledger writer. PR-2 of master_key_v2.
//
// Architecture war-roomed (war_room_id 9B1F252F, 2026-05-21) and
// locked CLEAR-TO-CODE by claude + google + minimax. The full set of
// locked decisions lives in memory `project_master_key_v2_design.md`
// (post-war-room block at the top). Summary of the bits that matter
// here:
//
//   * Probe shape: sha256(team_id || bundle_id) truncated to 16 bytes
//     (32 hex chars). Enough entropy for an opaque deterministic ID;
//     plaintext team-id stays out of the database.
//   * macOS extraction: subprocess `codesign -d --verbose=2` + regex
//     parse on stderr. The FFI route via `security-framework` crate
//     was the war-room's first pick but adds version-pin churn risk
//     for marginal latency gain (5-15ms vs 30-50ms — both well below
//     the 100ms startup budget). Future PR can swap once the FFI
//     surface stabilizes; tests guard the contract either way.
//   * Linux: `$APPIMAGE` env-var sentinel only. Real AppImage GPG
//     signature parsing is its own future PR — claude X (rejected
//     mixing in `/etc/os-release` since distro upgrades would false-
//     trigger).
//   * Windows: coarse `sha256(exe_path || os_version_major)`. Real
//     Authenticode parsing is a later PR; this gives us SOMETHING
//     stable to compare so PR-3 isn't blind on Windows.
//   * When to run: sync at startup, after `migrate_legacy_api_keys`.
//     5-15ms on macOS, microseconds on Linux/Windows. Async would
//     create a race window where PR-3 reads NULL on first launch
//     after an upgrade — claude D.
//   * Idempotency: UPDATE master_key_ledger WHERE identity_probe IS
//     NULL. PR-2 NEVER overwrites a populated probe — the "probe
//     changed" signal IS what PR-3 needs to detect. Silent overwrite
//     here would make PR-3 blind forever.
//   * Env-bypass (`ATO_MASTER_KEY_B64` set): skip probe write
//     entirely. A dev-build's probe is unsigned-macos / unsigned-
//     linux; writing that onto the v1 row — which represents the
//     PROD-signed keychain key — would corrupt PR-3's comparison.

use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::Mutex;

/// Top-level: compute the per-OS probe for the currently-running
/// binary. Always returns SOMETHING — sentinels in unsigned /
/// unsignable contexts so the column never holds NULL once
/// populate_active_row runs successfully. Stable across launches of
/// the same binary identity; changes on legitimate transitions
/// (signing-cert team change, new install).
pub fn compute_probe() -> String {
    let raw = compute_raw_components();
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    let bytes = hasher.finalize();
    bytes[..16].iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(target_os = "macos")]
fn compute_raw_components() -> String {
    macos::compute_components().unwrap_or_else(|| "unsigned-macos".to_string())
}

#[cfg(target_os = "linux")]
fn compute_raw_components() -> String {
    linux::compute_components()
}

#[cfg(target_os = "windows")]
fn compute_raw_components() -> String {
    windows::compute_components()
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn compute_raw_components() -> String {
    "unsupported-platform".to_string()
}

#[cfg(target_os = "macos")]
mod macos {
    /// Returns `Some("team_id|bundle_id")` when `codesign -d --verbose=2`
    /// can read either field; `None` when neither is available (ad-hoc
    /// signed, completely unsigned, or codesign binary missing).
    /// Output goes to stderr — that's standard for codesign verbose
    /// inspection. Parsing is line-oriented + tolerant of unknown
    /// extra lines so future codesign output additions don't break us.
    pub fn compute_components() -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        let output = std::process::Command::new("codesign")
            .arg("-d")
            .arg("--verbose=2")
            .arg(&exe)
            .output()
            .ok()?;
        // codesign writes the verbose dump to stderr; stdout stays
        // empty on a successful -d. Don't gate on status.success()
        // because codesign exits non-zero on unsigned binaries but
        // still emits SOMETHING useful (or nothing — we handle both).
        parse_codesign_stderr(&String::from_utf8_lossy(&output.stderr))
    }

    /// Pure parser extracted so the "not set" branch, leading/trailing
    /// whitespace, quoted-value edge case, and missing-both-fields case
    /// are unit-testable against synthetic fixtures (without spawning
    /// codesign). Review war-room 9B1F252F round 2 — claude's highest-
    /// value AMEND.
    pub(super) fn parse_codesign_stderr(stderr: &str) -> Option<String> {
        let mut team_id = String::new();
        let mut bundle_id = String::new();
        for line in stderr.lines() {
            // Lines we care about look like:
            //   TeamIdentifier=ABCD1234
            //   Identifier=io.nigri.ato
            // The `Identifier=` form is the bundle-ID-equivalent for
            // signed binaries. Belt-and-suspenders: trim whitespace
            // AND strip surrounding quotes in case a future codesign
            // release wraps the value (none do today; google + minimax
            // round-2 reviewers flagged it as a theoretical fragility).
            if let Some(rest) = line.strip_prefix("TeamIdentifier=") {
                team_id = strip_quotes(rest.trim()).to_string();
            } else if let Some(rest) = line.strip_prefix("Identifier=") {
                bundle_id = strip_quotes(rest.trim()).to_string();
            }
        }
        // Reject the explicit "not set" sentinel codesign uses for
        // ad-hoc signed binaries.
        if team_id == "not set" {
            team_id.clear();
        }
        if team_id.is_empty() && bundle_id.is_empty() {
            None
        } else {
            Some(format!("{}|{}", team_id, bundle_id))
        }
    }

    fn strip_quotes(s: &str) -> &str {
        let s = s.strip_prefix('"').unwrap_or(s);
        s.strip_suffix('"').unwrap_or(s)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    /// `$APPIMAGE` is set by the AppImage runtime to the absolute path
    /// of the .AppImage file when the app is launched from one. We
    /// hash the basename (stable across mount points, identifies the
    /// specific release file). When not running from an AppImage —
    /// `.deb` install, dev build, snap, etc. — return the sentinel.
    ///
    /// Deliberately NOT mixing in `/etc/os-release` IDs (would false-
    /// trigger on distro upgrades) or GPG signature bytes (PR-2 stays
    /// additive-only; signature parsing is a future PR per
    /// war-room C decision).
    pub fn compute_components() -> String {
        match std::env::var("APPIMAGE") {
            Ok(path) if !path.trim().is_empty() => {
                let basename = std::path::Path::new(&path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone());
                format!("appimage|{}", basename)
            }
            _ => "unsigned-linux".to_string(),
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    /// Coarse stand-in for real Authenticode parsing (a future PR).
    /// `exe_path` is stable per install. We append the `$OS` env var
    /// value (set by Windows itself, typically `"Windows_NT"`) as a
    /// crude OS-family marker — real OS-version reads via the
    /// `windows` crate's `Win32_System_SystemInformation` API are
    /// deferred along with the Authenticode parse work. PR-3 gets
    /// SOMETHING stable to compare instead of always reading NULL on
    /// win; the value just won't catch a Windows 10 → 11 upgrade
    /// until the future PR lands.
    pub fn compute_components() -> String {
        let exe_path = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let os_major = std::env::var("OS")
            .ok()
            .unwrap_or_else(|| "windows".to_string());
        format!("windows|{}|{}", exe_path, os_major)
    }
}

/// Write the freshly-computed probe to the active master_key_ledger
/// row, but only when the probe column is still NULL. PR-2's contract
/// is "fill the NULL; never overwrite." If the row's probe is already
/// set, this UPDATE matches zero rows and returns `Ok(0)`.
///
/// Returns `Ok(rows_affected)` so callers can confirm a write
/// happened (1) or was deliberately a no-op (0). Errors bubble for
/// schema-missing / connection-broken cases — the caller in
/// `lib::run()` swallows them with `let _ =` so a probe write failure
/// never blocks app startup.
///
/// **Env-bypass guard.** When `ATO_MASTER_KEY_B64` is set (typically
/// dev builds running outside the keychain), skip the write entirely.
/// A dev-build probe would persist a dev identity onto the v1 row
/// that represents the production-keychain key — corrupting PR-3's
/// future comparison. Test #6 in this module pins the guard.
// Kept after the PR-3 refactor that moved the prod call-site to
// `run_full_probe_cycle`. Unit tests still exercise the bypass
// guard + the compute_probe path through this entry point; future
// direct callers (e.g. a PR-4 manual-rekey command) can also use it
// when they want populate-only semantics without the check pass.
#[allow(dead_code)]
pub fn populate_active_row(conn: &Connection) -> rusqlite::Result<usize> {
    if env_bypass_active() {
        return Ok(0);
    }
    let probe = compute_probe();
    populate_active_row_with(conn, &probe)
}

/// PR-3 orchestrator — the single entry point lib.rs::run calls.
/// Computes the probe ONCE (claude's "compute-once invariant"), feeds
/// the same value into both `populate_active_row_with_probe` and
/// `check_for_mismatch`, returns the resulting `ProbeStatus` so the
/// caller can stash it for Tauri-command serving + event emit.
/// Env-bypass short-circuits at the top — saves the codesign-spawn
/// cost on dev launches AND keeps the v1 ledger row pristine.
pub fn run_full_probe_cycle(conn: &Connection) -> ProbeStatus {
    if env_bypass_active() {
        return ProbeStatus::NotPopulated;
    }
    let probe = compute_probe();
    // Populate result deliberately ignored (claude code-review
    // FC2FAB88 r2 #8): probe writes are observational per PR-2's
    // contract. If the write fails, `check_for_mismatch` reads back
    // the row as-is — either still NULL (→ NotPopulated) or stale
    // (→ Matched/Mismatched against whatever PR-1 backfilled). Boot
    // path never crashes; user-visible status stays meaningful.
    let _ = populate_active_row_with_probe(conn, &probe);
    check_for_mismatch(conn, &probe)
}

/// Public variant of `populate_active_row` that accepts a pre-computed
/// probe — lets `run_full_probe_cycle` compute once and pass the same
/// value to both populate + check. The env-bypass guard runs here
/// too as defense-in-depth (any future caller bypassing the
/// orchestrator still doesn't pollute the prod-keychain row).
pub fn populate_active_row_with_probe(
    conn: &Connection,
    probe: &str,
) -> rusqlite::Result<usize> {
    if env_bypass_active() {
        return Ok(0);
    }
    populate_active_row_with(conn, probe)
}

/// PR-3 entry point: detect drift between the stored ledger probe and
/// the freshly-computed probe. Three terminal states (per arch
/// war-room FC2FAB88 lock):
///
///   * `NotPopulated` — PR-2 hasn't populated the row yet (or env-
///     bypass is active so we deliberately didn't). UI = no banner.
///   * `Matched` — happy path; stored == computed. Most launches.
///   * `Mismatched { … }` — the signal PR-4 will act on. On the
///     first detection in this session we write an audit_logs row;
///     subsequent detections of the same (resource_id,
///     computed_probe) tuple are deduped — `audit_logged: false`
///     tells the caller "we noticed but stayed silent."
///   * `Unknown { reason }` — schema not initialized yet, DB error,
///     or anything else that prevented us from making a decision.
///     Caller should NOT trigger any UI surface — just log.
///
/// Pure: takes `conn` + the already-computed `probe`. lib.rs::run
/// computes the probe ONCE and threads it through `populate_active_row_with`
/// + this function so we don't pay codesign-spawn cost twice (claude
/// arch round 1 — "probe-compute-once invariant"). Env-bypass skip
/// matches `populate_active_row` so the two stay symmetric — if
/// PR-2 wrote nothing, PR-3 reads nothing.
pub fn check_for_mismatch(conn: &Connection, computed_probe: &str) -> ProbeStatus {
    if env_bypass_active() {
        // Match PR-2's env-bypass semantics: dev builds never
        // participate in mismatch detection. eprintln gives dev
        // visibility without polluting the production audit trail.
        eprintln!(
            "[security] identity-probe mismatch check skipped: ATO_MASTER_KEY_B64 bypass active"
        );
        return ProbeStatus::NotPopulated;
    }
    let stored: Option<Option<String>> = conn
        .query_row(
            "SELECT identity_probe FROM master_key_ledger WHERE version = ?1",
            ["v1"],
            |r| r.get::<_, Option<String>>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            // Schema not initialized yet (e.g., init_database hasn't
            // run on this connection) shows up as
            // `SqliteFailure(_, Some("no such table: master_key_ledger"))`.
            // Bubble that as Unknown rather than a panic.
            other => Err(other),
        })
        .unwrap_or_else(|e| {
            eprintln!(
                "[security] identity-probe check: SELECT failed: {} \
                 (treating as Unknown — schema may not be initialized)",
                e
            );
            // Sentinel for the schema-missing branch.
            Some(Some(String::from("__SCHEMA_ERROR__")))
        });

    match stored {
        // No row in the ledger (PR-1 backfill hasn't run on this DB).
        None => ProbeStatus::Unknown {
            reason: "no v1 row in master_key_ledger (PR-1 backfill not yet applied)".to_string(),
        },
        // Row exists, probe column is NULL (PR-2 hasn't populated yet
        // OR env-bypass kept it NULL — but we already returned above
        // if env-bypass is active).
        Some(None) => ProbeStatus::NotPopulated,
        // Schema-missing sentinel.
        Some(Some(s)) if s == "__SCHEMA_ERROR__" => ProbeStatus::Unknown {
            reason: "master_key_ledger schema unavailable".to_string(),
        },
        // Probe populated → compare.
        Some(Some(stored_value)) => {
            if stored_value == computed_probe {
                ProbeStatus::Matched
            } else {
                let detected_at = chrono::Utc::now().to_rfc3339();
                let audit_logged =
                    write_mismatch_audit(conn, &stored_value, computed_probe, &detected_at)
                        .unwrap_or_else(|e| {
                            eprintln!(
                                "[security] identity-probe mismatch audit write failed: {} \
                                 (continuing — probe status still surfaced)",
                                e
                            );
                            false
                        });
                ProbeStatus::Mismatched {
                    stored_probe: stored_value,
                    computed_probe: computed_probe.to_string(),
                    detected_at,
                    audit_logged,
                }
            }
        }
    }
}

/// Write the mismatch event to `audit_logs` unless we've ALREADY
/// written an entry for this exact `(action, resource_id,
/// computed_probe)` tuple. Returns `Ok(true)` on a fresh write,
/// `Ok(false)` on a deduped skip (a prior session already audited
/// this exact drift). Errors bubble so the caller can degrade to
/// "surface status without audit" instead of crashing the boot path.
///
/// Dedup query is O(1) on the small audit_logs table thanks to the
/// existing `idx_audit_logs_action` + `idx_audit_logs_resource`
/// indexes. The `details LIKE` predicate substring-matches on the
/// JSON column — adequate for an opaque-hash exact-match without
/// pulling in a JSON1 dependency.
fn write_mismatch_audit(
    conn: &Connection,
    stored_probe: &str,
    computed_probe: &str,
    detected_at: &str,
) -> rusqlite::Result<bool> {
    const ACTION: &str = "identity_probe_mismatch_detected";
    const RESOURCE_TYPE: &str = "master_key_ledger";
    const RESOURCE_ID: &str = "v1";

    // Hex-invariant guard (claude code-review FC2FAB88 r2 #1): the
    // LIKE dedup query embeds `computed_probe` literally without an
    // ESCAPE clause. `compute_probe` produces strictly `[0-9a-f]{32}`
    // — no SQL wildcards (`%`, `_`) reachable. If a future PR ever
    // re-encodes probes as base64 (`/`, `+`, `=`) or any non-hex
    // scheme, this assert fails loudly in tests and forces us to add
    // a proper escape function before dedup silently breaks.
    debug_assert!(
        computed_probe.chars().all(|c| c.is_ascii_hexdigit()),
        "dedup LIKE relies on probe being hex; if encoding changes add an ESCAPE clause"
    );

    // Dedup: have we written this exact mismatch before?
    let needle = format!("\"computed_probe\":\"{}\"", computed_probe);
    let already: i64 = conn.query_row(
        "SELECT COUNT(1) FROM audit_logs
          WHERE action = ?1 AND resource_id = ?2 AND details LIKE ?3",
        rusqlite::params![ACTION, RESOURCE_ID, format!("%{}%", needle)],
        |r| r.get(0),
    )?;
    if already > 0 {
        return Ok(false);
    }

    let details = serde_json::json!({
        "stored_probe": stored_probe,
        "computed_probe": computed_probe,
        "detected_at": detected_at,
    })
    .to_string();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO audit_logs
            (id, action, resource_type, resource_id, resource_name,
             details, created_at)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6)",
        rusqlite::params![id, ACTION, RESOURCE_TYPE, RESOURCE_ID, details, detected_at],
    )?;
    Ok(true)
}

/// Tauri-managed state holding the ProbeStatus computed once at
/// startup by `run_full_probe_cycle`. `get_identity_probe_status`
/// reads this rather than recomputing — the codesign-spawn cost is
/// paid once per launch, not once per poll. PR-5's UI can poll
/// cheaply or subscribe to the `identity-probe-status` event
/// emitted in lib.rs::run's `.setup()` closure.
///
/// Mutex (not OnceLock) on purpose: a future PR may add a "recheck"
/// command (e.g. after the user re-keys via PR-5) that needs to
/// update the cached status. The lock is uncontended in practice —
/// only the startup write + Tauri-command reads touch it.
pub struct IdentityProbeState(pub Mutex<ProbeStatus>);

impl IdentityProbeState {
    pub fn new(initial: ProbeStatus) -> Self {
        Self(Mutex::new(initial))
    }
}

/// Tauri command — return the cached ProbeStatus from
/// IdentityProbeState. PR-5 calls this on app mount + on Settings
/// tab open. If the state lock is poisoned (a panic somewhere
/// else), surface as Unknown rather than crashing the IPC.
#[tauri::command]
pub fn get_identity_probe_status(
    state: tauri::State<'_, IdentityProbeState>,
) -> ProbeStatus {
    state
        .0
        .lock()
        .map(|s| s.clone())
        .unwrap_or_else(|_| ProbeStatus::Unknown {
            reason: "identity-probe state lock poisoned".to_string(),
        })
}

/// Frontend-facing identity-probe state. Serialized as
/// `{"status":"...", "...":""}` via serde's default enum
/// representation — the variant name is the discriminator. PR-5's
/// React banner branches on the status field.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProbeStatus {
    /// PR-2 hasn't populated the probe yet, or env-bypass is active.
    /// UI = no banner; this is the most common state on a fresh
    /// install before the first PR-2-bearing launch.
    NotPopulated,
    /// Stored probe agrees with the freshly-computed one. The vast
    /// majority of launches land here.
    Matched,
    /// Drift detected. PR-5 surfaces a banner; PR-4 owns the rekey
    /// flow. `audit_logged` is `true` when THIS session wrote a new
    /// audit_logs row; `false` when a prior session already audited
    /// this exact `computed_probe` (dedup skip).
    Mismatched {
        stored_probe: String,
        computed_probe: String,
        detected_at: String,
        audit_logged: bool,
    },
    /// Schema not initialized, DB error, or another reason we
    /// couldn't make a decision. Caller logs; UI surfaces nothing.
    ///
    /// **`reason` is ops-facing only.** PR-5's UI MUST NOT render
    /// `reason` verbatim — surface Unknown as "status check
    /// unavailable" or hide the banner entirely. Raw SQL/IO error
    /// text never reaches `reason` (it goes through `eprintln!` in
    /// `check_for_mismatch`); the strings here are hand-curated
    /// diagnostic hints ("no v1 row in master_key_ledger…",
    /// "master_key_ledger schema unavailable", "lock poisoned").
    /// Any future contributor adding a new Unknown-producing branch
    /// must keep that contract — no leaking of `rusqlite::Error`
    /// Display text to the IPC surface. Claude code-review FC2FAB88
    /// r2 #6.
    Unknown {
        reason: String,
    },
}

/// Internal variant accepting an explicit probe value — exposed for
/// the unit tests so they can pin the SQL semantics without going
/// through `compute_probe()`'s per-OS path. Also useful if a future
/// PR moves probe computation to a background task and just passes
/// the result in.
///
/// **Do not call from startup paths.** Production callers MUST go
/// through `populate_active_row` so the `ATO_MASTER_KEY_B64`
/// env-bypass guard fires; otherwise a dev-build probe persists
/// onto the v1 row that represents the production-keychain key, and
/// PR-3's future comparison reads it as a legitimate identity
/// transition. Review war-room 9B1F252F round 2 — claude's note.
fn populate_active_row_with(conn: &Connection, probe: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE master_key_ledger
            SET identity_probe = ?1
          WHERE version = ?2
            AND identity_probe IS NULL",
        rusqlite::params![probe, "v1"],
    )
}

fn env_bypass_active() -> bool {
    std::env::var("ATO_MASTER_KEY_B64")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    // Each test that touches ATO_MASTER_KEY_B64 must serialize against
    // the others — env vars are process-global and parallel `cargo
    // test` would race. The unit tests here that don't touch the env
    // var still hold the lock so the env-bypass test can't read a
    // partially-mutated state. Cheap insurance.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn fresh_db_with_ledger_row() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE master_key_ledger (
                version           TEXT PRIMARY KEY,
                keychain_account  TEXT NOT NULL,
                ciphertext_format TEXT NOT NULL,
                identity_probe    TEXT,
                source            TEXT NOT NULL DEFAULT 'keychain',
                created_at        TEXT NOT NULL,
                retired_at        TEXT,
                notes             TEXT
             );
             INSERT INTO master_key_ledger
                 (version, keychain_account, ciphertext_format,
                  identity_probe, source, created_at, retired_at, notes)
             VALUES
                 ('v1', 'master_key_v1', 'aes-gcm-v1', NULL,
                  'keychain', '2026-05-21T00:00:00Z', NULL,
                  'test fixture');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn populate_writes_probe_when_column_is_null() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ATO_MASTER_KEY_B64");
        let conn = fresh_db_with_ledger_row();
        let n = populate_active_row(&conn).unwrap();
        assert_eq!(n, 1, "expected 1 row updated on first populate");
        let probe: Option<String> = conn
            .query_row(
                "SELECT identity_probe FROM master_key_ledger WHERE version='v1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(probe.is_some(), "probe column should be populated after write");
        assert!(
            !probe.unwrap().is_empty(),
            "probe value should be non-empty hex string"
        );
    }

    #[test]
    fn populate_is_noop_when_probe_already_set() {
        // PR-2 contract: never overwrite a populated probe. Even if the
        // freshly-computed probe differs (which is the mismatch signal
        // PR-3 wants), PR-2 must leave it alone so PR-3 can detect
        // the drift.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ATO_MASTER_KEY_B64");
        let conn = fresh_db_with_ledger_row();
        // Seed an existing probe value DIFFERENT from what compute_probe
        // would produce.
        conn.execute(
            "UPDATE master_key_ledger SET identity_probe = ?1 WHERE version = 'v1'",
            ["sentinel-already-populated"],
        )
        .unwrap();
        let n = populate_active_row(&conn).unwrap();
        assert_eq!(n, 0, "expected 0 rows updated when probe already set");
        let probe: String = conn
            .query_row(
                "SELECT identity_probe FROM master_key_ledger WHERE version='v1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            probe, "sentinel-already-populated",
            "PR-2 must NEVER overwrite a populated probe"
        );
    }

    #[test]
    fn populate_is_noop_when_no_v1_row_exists() {
        // Defensive against init-order reversal — if for any reason
        // schema::init_database hasn't run before this is called (or
        // its INSERT OR IGNORE was skipped), we should return Ok(0)
        // rather than panicking or creating a phantom row.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ATO_MASTER_KEY_B64");
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE master_key_ledger (
                version           TEXT PRIMARY KEY,
                keychain_account  TEXT NOT NULL,
                ciphertext_format TEXT NOT NULL,
                identity_probe    TEXT,
                source            TEXT NOT NULL DEFAULT 'keychain',
                created_at        TEXT NOT NULL,
                retired_at        TEXT,
                notes             TEXT
             );",
        )
        .unwrap();
        let n = populate_active_row(&conn).unwrap();
        assert_eq!(n, 0, "no v1 row → UPDATE matches 0 rows");
    }

    #[test]
    fn populate_preserves_other_columns() {
        // The UPDATE must touch ONLY the identity_probe column. Anything
        // else changing (notes, source, ciphertext_format, etc.) would
        // be a subtle bug that PR-3 would silently inherit.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ATO_MASTER_KEY_B64");
        let conn = fresh_db_with_ledger_row();
        let before: (String, String, String, String, String) = conn
            .query_row(
                "SELECT keychain_account, ciphertext_format, source,
                        created_at, notes
                   FROM master_key_ledger WHERE version='v1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        populate_active_row(&conn).unwrap();
        let after: (String, String, String, String, String) = conn
            .query_row(
                "SELECT keychain_account, ciphertext_format, source,
                        created_at, notes
                   FROM master_key_ledger WHERE version='v1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(before, after, "populate touched a column it shouldn't have");
    }

    #[test]
    fn populate_skips_when_env_bypass_active() {
        // Pins the env-bypass guard. Setting ATO_MASTER_KEY_B64 should
        // bail BEFORE any compute_probe / SQL write. The v1 row's probe
        // must stay NULL.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("ATO_MASTER_KEY_B64", "dev-bypass-value");
        let conn = fresh_db_with_ledger_row();
        let n = populate_active_row(&conn).unwrap();
        std::env::remove_var("ATO_MASTER_KEY_B64");
        assert_eq!(n, 0, "env-bypass must skip the write");
        let probe: Option<String> = conn
            .query_row(
                "SELECT identity_probe FROM master_key_ledger WHERE version='v1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            probe, None,
            "env-bypass must leave identity_probe NULL"
        );
    }

    #[test]
    fn compute_probe_truncates_to_low_bytes_not_high() {
        // Pin truncation direction (`bytes[..16]` not `bytes[16..]`).
        // A future cleanup that swaps the slice would otherwise pass
        // every other test silently — probes would still be 32 hex
        // chars and still deterministic — but every prior install's
        // probe would become a different value, false-triggering PR-3
        // rekey for every user on upgrade. Review war-room 9B1F252F
        // round 2 — claude micro-fix.
        use sha2::{Digest, Sha256};
        let raw = super::compute_raw_components();
        let full = Sha256::digest(raw.as_bytes());
        let want: String = full[..16].iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            compute_probe(),
            want,
            "compute_probe must hash + truncate to the LOW 16 bytes"
        );
    }

    #[test]
    fn compute_probe_returns_stable_nonempty_hex() {
        // Two calls in the same process should produce the same probe —
        // determinism is the entire contract. Also pin the format
        // (32 hex chars) so a future "let's truncate to 8 chars to
        // save bytes" change has to also update this test.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let p1 = compute_probe();
        let p2 = compute_probe();
        assert_eq!(p1, p2, "compute_probe must be deterministic");
        assert!(!p1.is_empty(), "probe must be non-empty");
        assert_eq!(
            p1.len(),
            32,
            "probe must be sha256 truncated to 16 bytes = 32 hex chars"
        );
        assert!(
            p1.chars().all(|c| c.is_ascii_hexdigit()),
            "probe must be hex-only"
        );
    }

    // Per-OS parser fixture tests. cfg-gated to the platform whose
    // parser they exercise so the test binary on the other platforms
    // doesn't try to compile against unavailable code paths.

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_parser_extracts_team_and_bundle_from_real_codesign_output() {
        // A captured codesign -d --verbose=2 dump (synthetic but
        // shape-accurate per Apple's published format). The parser
        // must pull both fields out and ignore the surrounding noise.
        let stderr = "Executable=/Applications/ATO.app/Contents/MacOS/ato-desktop\n\
                      Identifier=io.nigri.ato\n\
                      Format=app bundle with Mach-O thin (arm64)\n\
                      CodeDirectory v=20500 size=42 flags=0x10000(runtime)\n\
                      Signature size=8939\n\
                      Authority=Developer ID Application: Will Nigri (TEAMID1234)\n\
                      TeamIdentifier=TEAMID1234\n\
                      Timestamp=Jun 21, 2026 at 1:23:45 PM\n";
        let got = super::macos::parse_codesign_stderr(stderr);
        assert_eq!(got.as_deref(), Some("TEAMID1234|io.nigri.ato"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_parser_returns_none_when_neither_field_present() {
        // Truly unsigned binary — codesign emits an error to stderr
        // with no Identifier / TeamIdentifier lines. compute_probe
        // then falls back to the "unsigned-macos" sentinel.
        let stderr = "Executable=/tmp/local-dev-binary\n\
                      ./local-dev-binary: code object is not signed at all\n";
        assert_eq!(super::macos::parse_codesign_stderr(stderr), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_parser_treats_not_set_team_as_empty() {
        // Ad-hoc signed binaries (cargo build --release without an
        // Apple signing identity) report `TeamIdentifier=not set`.
        // The bundle id is still present; the parser keeps it and
        // omits the team-id rather than producing `not set|<bundle>`.
        let stderr = "Identifier=io.nigri.ato.dev\n\
                      TeamIdentifier=not set\n";
        let got = super::macos::parse_codesign_stderr(stderr);
        assert_eq!(got.as_deref(), Some("|io.nigri.ato.dev"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_parser_strips_surrounding_quotes_if_present() {
        // Belt-and-suspenders: today codesign doesn't quote these
        // values, but the round-2 reviewers (google + minimax) named
        // it as a theoretical fragility worth defending against. If
        // Apple ever wraps with quotes in a future codesign release,
        // the probe value changes (PR-3 signal) but doesn't fold the
        // quote characters into the hashed input.
        let stderr = "TeamIdentifier=\"TEAMID1234\"\n\
                      Identifier=\"io.nigri.ato\"\n";
        let got = super::macos::parse_codesign_stderr(stderr);
        assert_eq!(got.as_deref(), Some("TEAMID1234|io.nigri.ato"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_basename_handles_path_with_no_slashes() {
        // $APPIMAGE is normally an absolute path, but a user shelling
        // `APPIMAGE=foo.AppImage ./ato-desktop` for testing would set
        // it to a slash-less value. Path::file_name still returns
        // Some("foo.AppImage") in that case; the probe just becomes
        // `appimage|foo.AppImage`. Pin so a future refactor that
        // calls dirname()/parent() doesn't accidentally regress to
        // an empty basename. Review war-room 9B1F252F round 2 —
        // minimax flagged the edge case.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("APPIMAGE", "ato-desktop-2.7.14.AppImage");
        let got = super::linux::compute_components();
        std::env::remove_var("APPIMAGE");
        assert_eq!(got, "appimage|ato-desktop-2.7.14.AppImage");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_empty_appimage_falls_back_to_sentinel() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("APPIMAGE", "");
        let got = super::linux::compute_components();
        std::env::remove_var("APPIMAGE");
        assert_eq!(got, "unsigned-linux");
    }

    // ─── PR-3 mismatch-detection tests ───────────────────────────────
    //
    // Architecture war-roomed at FC2FAB88. Tests pin the 4-state
    // ProbeStatus contract + the audit-write dedup semantics +
    // env-bypass parity with PR-2 + schema-missing graceful Unknown.

    fn fresh_db_with_ledger_and_audit() -> Connection {
        // Mirror the relevant slice of schema.rs::init_database — the
        // master_key_ledger row (PR-1 backfill) + the audit_logs table
        // shape — without pulling in the rest of the 1000-line schema.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE master_key_ledger (
                version           TEXT PRIMARY KEY,
                keychain_account  TEXT NOT NULL,
                ciphertext_format TEXT NOT NULL,
                identity_probe    TEXT,
                source            TEXT NOT NULL DEFAULT 'keychain',
                created_at        TEXT NOT NULL,
                retired_at        TEXT,
                notes             TEXT
             );
             INSERT INTO master_key_ledger
                 (version, keychain_account, ciphertext_format,
                  identity_probe, source, created_at, retired_at, notes)
             VALUES
                 ('v1', 'master_key_v1', 'aes-gcm-v1', NULL,
                  'keychain', '2026-05-22T00:00:00Z', NULL, 'test');
             CREATE TABLE audit_logs (
                 id            TEXT PRIMARY KEY,
                 action        TEXT NOT NULL,
                 resource_type TEXT NOT NULL,
                 resource_id   TEXT,
                 resource_name TEXT,
                 details       TEXT,
                 created_at    TEXT NOT NULL
             );
             CREATE INDEX idx_audit_logs_action ON audit_logs(action);
             CREATE INDEX idx_audit_logs_resource ON audit_logs(resource_type, resource_id);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn check_returns_not_populated_when_probe_is_null() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ATO_MASTER_KEY_B64");
        let conn = fresh_db_with_ledger_and_audit();
        let status = check_for_mismatch(&conn, "any-computed-probe");
        assert_eq!(status, ProbeStatus::NotPopulated);
    }

    #[test]
    fn check_returns_matched_when_stored_equals_computed() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ATO_MASTER_KEY_B64");
        let conn = fresh_db_with_ledger_and_audit();
        conn.execute(
            "UPDATE master_key_ledger SET identity_probe = ?1 WHERE version='v1'",
            ["abc123"],
        )
        .unwrap();
        let status = check_for_mismatch(&conn, "abc123");
        assert_eq!(status, ProbeStatus::Matched);
        // No audit row written on match.
        let audit_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(audit_count, 0, "matched path must not write audit");
    }

    #[test]
    fn check_returns_mismatched_with_audit_logged_true_on_first_detect() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ATO_MASTER_KEY_B64");
        let conn = fresh_db_with_ledger_and_audit();
        conn.execute(
            "UPDATE master_key_ledger SET identity_probe = ?1 WHERE version='v1'",
            ["stored-abc"],
        )
        .unwrap();
        let status = check_for_mismatch(&conn, "computed-xyz");
        match status {
            ProbeStatus::Mismatched {
                stored_probe,
                computed_probe,
                detected_at,
                audit_logged,
            } => {
                assert_eq!(stored_probe, "stored-abc");
                assert_eq!(computed_probe, "computed-xyz");
                assert!(!detected_at.is_empty(), "detected_at must be set");
                assert!(audit_logged, "first detect must write audit row");
            }
            other => panic!("expected Mismatched, got {:?}", other),
        }
        // Audit row written with the right shape.
        let (action, resource_type, resource_id, details): (
            String,
            String,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT action, resource_type, resource_id, details
                   FROM audit_logs LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(action, "identity_probe_mismatch_detected");
        assert_eq!(resource_type, "master_key_ledger");
        assert_eq!(resource_id, "v1");
        // JSON shape contract — PR-5's frontend reads these keys.
        assert!(details.contains("\"stored_probe\":\"stored-abc\""));
        assert!(details.contains("\"computed_probe\":\"computed-xyz\""));
        assert!(details.contains("\"detected_at\""));
    }

    #[test]
    fn check_dedupes_audit_on_second_detect_with_same_probes() {
        // PR-3 contract: the SAME (action, resource_id, computed_probe)
        // tuple writes ONE audit row total. A relaunch loop with the
        // same persistent mismatch must not flood audit_logs.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ATO_MASTER_KEY_B64");
        let conn = fresh_db_with_ledger_and_audit();
        conn.execute(
            "UPDATE master_key_ledger SET identity_probe = ?1 WHERE version='v1'",
            ["stored-abc"],
        )
        .unwrap();
        let first = check_for_mismatch(&conn, "computed-xyz");
        let second = check_for_mismatch(&conn, "computed-xyz");
        match (&first, &second) {
            (
                ProbeStatus::Mismatched { audit_logged: true, .. },
                ProbeStatus::Mismatched { audit_logged: false, .. },
            ) => {}
            _ => panic!(
                "expected (Mismatched audit_logged=true, Mismatched audit_logged=false), got ({:?}, {:?})",
                first, second
            ),
        }
        let audit_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(audit_count, 1, "dedup must keep audit row count at 1");
    }

    #[test]
    fn check_writes_distinct_audit_when_computed_probe_changes() {
        // Belt-and-suspenders on the dedup logic: a DIFFERENT
        // computed_probe (e.g. user upgraded macOS major version
        // → identity changed AGAIN) is a NEW event, gets its own
        // audit row. Otherwise PR-3 would mask multi-cliff users.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ATO_MASTER_KEY_B64");
        let conn = fresh_db_with_ledger_and_audit();
        conn.execute(
            "UPDATE master_key_ledger SET identity_probe = ?1 WHERE version='v1'",
            ["stored-abc"],
        )
        .unwrap();
        let _ = check_for_mismatch(&conn, "first-drift");
        let _ = check_for_mismatch(&conn, "second-drift");
        let audit_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(audit_count, 2, "distinct computed_probe → distinct audit rows");
    }

    #[test]
    fn check_skips_on_env_bypass_active() {
        // Even with a clear mismatch in the ledger, env-bypass short-
        // circuits to NotPopulated AND writes no audit row. Mirrors
        // PR-2's populate guard — if PR-2 didn't write, PR-3 must not
        // surface a comparison.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("ATO_MASTER_KEY_B64", "dev-bypass-value");
        let conn = fresh_db_with_ledger_and_audit();
        conn.execute(
            "UPDATE master_key_ledger SET identity_probe = ?1 WHERE version='v1'",
            ["stored-abc"],
        )
        .unwrap();
        let status = check_for_mismatch(&conn, "computed-xyz");
        std::env::remove_var("ATO_MASTER_KEY_B64");
        assert_eq!(status, ProbeStatus::NotPopulated);
        let audit_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(audit_count, 0, "env-bypass must skip audit entirely");
    }

    #[test]
    fn check_returns_unknown_on_missing_ledger_table() {
        // Defensive: caller hands us a connection that hasn't run
        // schema::init_database yet. We must surface Unknown, not
        // panic / not bubble the SqliteFailure error.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ATO_MASTER_KEY_B64");
        let conn = Connection::open_in_memory().unwrap();
        let status = check_for_mismatch(&conn, "any");
        match status {
            ProbeStatus::Unknown { reason } => {
                assert!(!reason.is_empty(), "Unknown must carry a reason");
            }
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn run_full_probe_cycle_env_bypass_returns_not_populated() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("ATO_MASTER_KEY_B64", "x");
        let conn = fresh_db_with_ledger_and_audit();
        let status = run_full_probe_cycle(&conn);
        std::env::remove_var("ATO_MASTER_KEY_B64");
        assert_eq!(status, ProbeStatus::NotPopulated);
        // Verify the probe column wasn't populated either.
        let probe: Option<String> = conn
            .query_row(
                "SELECT identity_probe FROM master_key_ledger WHERE version='v1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(probe, None);
    }

    #[test]
    fn run_full_probe_cycle_happy_path_populates_then_matches() {
        // End-to-end: fresh ledger (probe=NULL) → run_full_probe_cycle
        // populates the row AND reads it back AND compares → Matched
        // (the same probe wrote then read). Pins the "compute once,
        // use twice" invariant claude flagged.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ATO_MASTER_KEY_B64");
        let conn = fresh_db_with_ledger_and_audit();
        let status = run_full_probe_cycle(&conn);
        assert_eq!(status, ProbeStatus::Matched);
        let probe: Option<String> = conn
            .query_row(
                "SELECT identity_probe FROM master_key_ledger WHERE version='v1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(probe.is_some(), "populate must have written the probe");
    }

    #[test]
    fn probe_status_serde_round_trip_for_pr5_contract() {
        // PR-5's React banner deserializes this exact shape. If a
        // future change accidentally renames a variant or field, the
        // frontend silently breaks — this test fails first.
        let s = ProbeStatus::Mismatched {
            stored_probe: "abc".to_string(),
            computed_probe: "def".to_string(),
            detected_at: "2026-05-22T12:00:00Z".to_string(),
            audit_logged: true,
        };
        let json = serde_json::to_string(&s).unwrap();
        // Variant tag uses snake_case from rename_all.
        assert!(json.contains("\"status\":\"mismatched\""));
        assert!(json.contains("\"stored_probe\":\"abc\""));
        assert!(json.contains("\"computed_probe\":\"def\""));
        assert!(json.contains("\"detected_at\":\"2026-05-22T12:00:00Z\""));
        assert!(json.contains("\"audit_logged\":true"));

        // The NotPopulated / Matched variants serialize as a bare
        // status field (no payload).
        let np = serde_json::to_string(&ProbeStatus::NotPopulated).unwrap();
        assert_eq!(np, "{\"status\":\"not_populated\"}");
        let m = serde_json::to_string(&ProbeStatus::Matched).unwrap();
        assert_eq!(m, "{\"status\":\"matched\"}");
    }
}
