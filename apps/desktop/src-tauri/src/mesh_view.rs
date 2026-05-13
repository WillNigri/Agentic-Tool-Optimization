// Phase 7.0 step 5 — Tauri commands behind Settings → Mesh.
//
// Read paths (discovered / peers / invites) are direct SQLite reads
// using the existing DbState. Write paths use a mix:
//   - Create invite: direct SQL + small inline code generation. The
//     CLI's authoritative generator lives in apps/cli/src/commands/
//     mesh.rs but is small enough to mirror here; both write through
//     the same `mesh_invites` PK so cross-process collision is moot.
//   - Consume invite: shells out to `ato mesh invite consume …`
//     so the daemon-to-daemon WebSocket client + pin check live in
//     one place (the CLI binary) and the GUI doesn't have to carry
//     a parallel async/tokio dispatch path. The bundled sidecar at
//     `Contents/Resources/binaries/ato-<target>` is the production
//     path; `find_ato_binary()` falls back to PATH for dev.
//   - Remove peer: direct DELETE.
//
// All read commands pass through `bootstrap_mesh_columns` first so
// a fresh-install DB picks up the v2.4.4-phase7 `mesh_invites
// .issuer_pubkey` column even if the user opened the GUI before
// running `ato mesh` from the CLI.

use rand::RngCore;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

// Overall deadline for `ato mesh invite consume` shell-out. The CLI
// itself wraps `ws_consume_invite_call` in a 10s timeout but the
// subprocess can still wait on SQLite busy_timeout + tokio runtime
// boot before reaching that call. 30s is comfortably above the
// observed worst case in dev (~3-4s) without leaving a Tauri command
// hanging forever if the remote daemon is unreachable. (claude #1)
const CONSUME_INVITE_TIMEOUT: Duration = Duration::from_secs(30);

use crate::DbState;

// ── Crockford base32 invite codes (mirror of CLI's mesh.rs). ──────
const CROCKFORD_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CODE_PREFIX: &str = "ATO-";
const CODE_GROUP_LEN: usize = 4;
const CODE_GROUPS: usize = 3;
const DEFAULT_TTL_MINUTES: i64 = 15;
const MAX_TTL_MINUTES: i64 = 60 * 24;

fn generate_invite_code() -> String {
    let mut rng = rand::rngs::OsRng;
    let total_chars = CODE_GROUP_LEN * CODE_GROUPS;
    let mut bytes = vec![0u8; total_chars];
    rng.fill_bytes(&mut bytes);
    let mut s = String::with_capacity(CODE_PREFIX.len() + total_chars + CODE_GROUPS - 1);
    s.push_str(CODE_PREFIX);
    for g in 0..CODE_GROUPS {
        if g > 0 {
            s.push('-');
        }
        for i in 0..CODE_GROUP_LEN {
            let idx = (bytes[g * CODE_GROUP_LEN + i] & 0x1f) as usize;
            s.push(CROCKFORD_ALPHABET[idx] as char);
        }
    }
    s
}

fn validate_peer_id_format(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

// Defensive ALTER mirrored from CLI's mesh.rs bootstrap_mesh_columns.
fn bootstrap_mesh_columns(conn: &Connection) {
    let _ = conn.execute(
        "ALTER TABLE mesh_invites ADD COLUMN issuer_pubkey TEXT",
        [],
    );
}

// ── Wire types ─────────────────────────────────────────────────────
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshDiscoveredPeer {
    pub peer_id: String,
    pub name: String,
    pub addr: String,
    pub version: Option<String>,
    pub last_seen_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshPeerRow {
    pub peer_id: String,
    pub name: String,
    pub paired_at: String,
    pub last_seen_at: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshInviteRow {
    pub code: String,
    pub issued_at: String,
    pub expires_at: String,
    pub consumed: bool,
    pub issuer_pubkey: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshConsumeResult {
    pub peer_id: String,
    pub public_key_b64: String,
    pub machine_name: String,
}

// ── Read commands ──────────────────────────────────────────────────
#[tauri::command]
pub fn mesh_list_discovered(db: State<'_, DbState>) -> Result<Vec<MeshDiscoveredPeer>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    bootstrap_mesh_columns(&conn);
    let mut stmt = conn
        .prepare(
            "SELECT peer_id, name, addr, version, last_seen_at
               FROM mesh_discovered
              ORDER BY last_seen_at DESC",
        )
        .map_err(|e| format!("prepare mesh_discovered: {}", e))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(MeshDiscoveredPeer {
                peer_id: r.get(0)?,
                name: r.get(1)?,
                addr: r.get(2)?,
                version: r.get(3).ok(),
                last_seen_at: r.get(4)?,
            })
        })
        .map_err(|e| format!("query mesh_discovered: {}", e))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("collect mesh_discovered: {}", e))
}

#[tauri::command]
pub fn mesh_list_peers(db: State<'_, DbState>) -> Result<Vec<MeshPeerRow>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    bootstrap_mesh_columns(&conn);
    let mut stmt = conn
        .prepare(
            "SELECT peer_id, name, paired_at, last_seen_at, notes
               FROM mesh_peers
              ORDER BY paired_at DESC",
        )
        .map_err(|e| format!("prepare mesh_peers: {}", e))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(MeshPeerRow {
                peer_id: r.get(0)?,
                name: r.get(1)?,
                paired_at: r.get(2)?,
                last_seen_at: r.get(3).ok(),
                notes: r.get(4).ok(),
            })
        })
        .map_err(|e| format!("query mesh_peers: {}", e))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("collect mesh_peers: {}", e))
}

#[tauri::command]
pub fn mesh_list_invites(
    db: State<'_, DbState>,
    include_all: Option<bool>,
) -> Result<Vec<MeshInviteRow>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    bootstrap_mesh_columns(&conn);
    let now = chrono::Utc::now().to_rfc3339();
    let include_all = include_all.unwrap_or(false);
    let sql = if include_all {
        "SELECT code, issued_at, expires_at, consumed, issuer_pubkey
           FROM mesh_invites ORDER BY issued_at DESC"
    } else {
        "SELECT code, issued_at, expires_at, consumed, issuer_pubkey
           FROM mesh_invites
          WHERE consumed = 0 AND expires_at > ?1
          ORDER BY issued_at DESC"
    };
    let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare invites: {}", e))?;
    let map_row = |r: &rusqlite::Row| -> rusqlite::Result<MeshInviteRow> {
        Ok(MeshInviteRow {
            code: r.get(0)?,
            issued_at: r.get(1)?,
            expires_at: r.get(2)?,
            consumed: r.get::<_, i64>(3)? != 0,
            issuer_pubkey: r.get(4).ok(),
        })
    };
    let rows: Vec<MeshInviteRow> = if include_all {
        stmt.query_map([], map_row)
            .map_err(|e| format!("query invites: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect invites: {}", e))?
    } else {
        stmt.query_map([&now], map_row)
            .map_err(|e| format!("query invites: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect invites: {}", e))?
    };
    Ok(rows)
}

// ── Write commands ─────────────────────────────────────────────────
#[tauri::command]
pub fn mesh_create_invite(
    db: State<'_, DbState>,
    expires_minutes: Option<i64>,
) -> Result<MeshInviteRow, String> {
    let expires = expires_minutes.unwrap_or(DEFAULT_TTL_MINUTES);
    if !(1..=MAX_TTL_MINUTES).contains(&expires) {
        return Err(format!(
            "expires must be between 1 and {} minutes (got {})",
            MAX_TTL_MINUTES, expires
        ));
    }
    // Read the local daemon's pubkey so the row is bound to the
    // issuer (same defense as the CLI's invite_create). The daemon
    // module owns key-creation policy (0600 perms, atomic write); if
    // the keypair doesn't exist yet we fail fast and tell the user
    // to run `ato daemon start` rather than generate it from here.
    let issuer_pubkey = read_daemon_pubkey_b64()
        .map_err(|e| format!("read daemon identity: {}", e))?;

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    bootstrap_mesh_columns(&conn);
    let now = chrono::Utc::now();
    let expires_at = now + chrono::Duration::minutes(expires);
    for _attempt in 0..5 {
        let code = generate_invite_code();
        let res = conn.execute(
            "INSERT INTO mesh_invites (code, issued_at, expires_at, consumed, issuer_pubkey)
             VALUES (?1, ?2, ?3, 0, ?4)",
            params![code, now.to_rfc3339(), expires_at.to_rfc3339(), issuer_pubkey],
        );
        match res {
            Ok(_) => {
                return Ok(MeshInviteRow {
                    code,
                    issued_at: now.to_rfc3339(),
                    expires_at: expires_at.to_rfc3339(),
                    consumed: false,
                    issuer_pubkey: Some(issuer_pubkey),
                });
            }
            Err(rusqlite::Error::SqliteFailure(ff, _))
                if ff.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                continue;
            }
            Err(e) => return Err(format!("INSERT mesh_invites: {}", e)),
        }
    }
    Err("could not generate a unique invite code after 5 attempts".into())
}

#[tauri::command]
pub fn mesh_remove_peer(db: State<'_, DbState>, peer_id: String) -> Result<bool, String> {
    if !validate_peer_id_format(&peer_id) {
        return Err(format!(
            "peer_id must be a 64-character lowercase hex string (got {} chars)",
            peer_id.len()
        ));
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let affected = conn
        .execute(
            "DELETE FROM mesh_peers WHERE peer_id = ?1",
            params![peer_id],
        )
        .map_err(|e| format!("DELETE mesh_peers: {}", e))?;
    Ok(affected > 0)
}

#[tauri::command]
pub async fn mesh_consume_invite(
    code: String,
    host: String,
    port: u16,
    expect_peer_id: String,
    note: Option<String>,
) -> Result<MeshConsumeResult, String> {
    // Shell out to the CLI's `ato mesh invite consume` — same WS
    // client, same pin check, same atomic INSERT into mesh_peers.
    // The CLI prints the result as JSON when --human is omitted.
    //
    // Wrapped in a wall-clock timeout so a hung subprocess (remote
    // daemon down, network black hole) doesn't leave the Pair modal
    // spinner stuck. The CLI's own 10s WS timeout doesn't cover
    // startup + SQLite busy_timeout, hence the outer 30s. (claude #1)
    let ato = find_ato_binary()?;
    let mut cmd = Command::new(&ato);
    cmd.kill_on_drop(true);
    cmd.args(&[
        "mesh",
        "invite",
        "consume",
        &code,
        "--host",
        &host,
        "--port",
        &port.to_string(),
        "--expect-peer-id",
        &expect_peer_id,
    ]);
    if let Some(n) = note.as_deref() {
        cmd.args(&["--note", n]);
    }
    let out = match timeout(CONSUME_INVITE_TIMEOUT, cmd.output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            return Err(format!(
                "spawn `{} mesh invite consume`: {}",
                ato.display(),
                e
            ));
        }
        Err(_) => {
            return Err(format!(
                "ato mesh invite consume timed out after {}s (remote {}:{} unreachable?)",
                CONSUME_INVITE_TIMEOUT.as_secs(),
                host,
                port,
            ));
        }
    };
    if !out.status.success() {
        return Err(format!(
            "ato mesh invite consume failed (exit {}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // CLI prints the JSON of ConsumeInviteReplyResult { peer_id, public_key_b64, machine_name }.
    let parsed: MeshConsumeResult = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("decode CLI JSON: {} (raw: {})", e, stdout.trim()))?;
    Ok(parsed)
}

// ── Helpers ────────────────────────────────────────────────────────

/// Locate the `ato` CLI binary. Production path: bundled as a Tauri
/// sidecar at `Contents/Resources/binaries/ato-<target-triple>`.
/// Dev path: fall back to `apps/cli/target/release/ato` relative to
/// the project root, then `which ato` (debug builds only — a PATH
/// hijack of `ato` in release builds would let an attacker run
/// arbitrary code under the user's shell when they Pair, so the
/// release binary refuses to use $PATH at all). (security-specialist #1)
fn find_ato_binary() -> Result<PathBuf, String> {
    // 1. Bundled sidecar (production).
    let target_triple = current_target_triple();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent
                .join("../Resources/binaries")
                .join(format!("ato-{}", target_triple));
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }
    // 2. Dev path relative to the repo root.
    if let Ok(cwd) = std::env::current_dir() {
        let mut p = cwd.clone();
        while p.parent().is_some() {
            let candidate = p.join("apps/cli/target/release/ato");
            if candidate.exists() {
                return Ok(candidate);
            }
            p = p.parent().unwrap().to_path_buf();
        }
    }
    // 3. PATH fallback — debug builds only.
    #[cfg(debug_assertions)]
    {
        if let Ok(p) = which::which("ato") {
            return Ok(p);
        }
    }
    Err("ato binary not found. Install via `brew install --cask ato` or run from a dev tree with apps/cli/target/release/ato built.".into())
}

fn current_target_triple() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(target_os = "linux") {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc.exe"
    } else {
        "ato"
    }
}

/// Read the daemon's public key (base64 of the Ed25519 verifying
/// key) from `~/.ato/daemon/keys/public.bin`. If the daemon has
/// never been started, returns an error — the user is expected to
/// run `ato daemon start` once before pairing. We don't generate
/// the key here because the daemon module owns key-creation policy
/// (0600 perms, atomic write); duplicating that in the GUI would
/// risk drift.
fn read_daemon_pubkey_b64() -> Result<String, String> {
    use base64::Engine as _;
    let home = std::env::var("HOME").map_err(|e| format!("HOME: {}", e))?;
    let path = PathBuf::from(home).join(".ato/daemon/keys/public.bin");
    if !path.exists() {
        return Err(format!(
            "{} not found. Run `ato daemon start` once to generate the daemon keypair before creating invites.",
            path.display()
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    if bytes.len() != 32 {
        return Err(format!(
            "{} is {} bytes (expected 32 for Ed25519)",
            path.display(),
            bytes.len()
        ));
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}
