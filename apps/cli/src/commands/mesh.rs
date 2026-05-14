// `ato mesh ...` — Phase 7.0 step 4: invite-code pairing + peer
// management.
//
// The pairing flow, plain English:
//   1. Alice runs `ato mesh invite create` on her laptop. Her daemon
//      generates a high-entropy code (`ATO-XXXX-XXXX-XXXX`), persists
//      it to `mesh_invites` (TTL default 15 min, one-shot) ALONG
//      WITH her own daemon's public key, and prints the code +
//      host:port for the consumer to dial.
//   2. Alice sends the code to Bob out-of-band (Slack, in person).
//   3. Bob runs `ato mesh invite consume <code> --host alice.local
//      --port 7755`. His daemon opens a WS client to Alice's daemon,
//      sends `consume_invite{ code, sender_peer_id, sender_pubkey,
//      sender_machine_name }`. Alice's daemon performs an atomic
//      `try_consume_invite` (UPDATE consumed=1 + INSERT mesh_peers
//      inside a single transaction so concurrent redeemers cannot
//      both win), and replies with her own peer_id + pubkey + name.
//      Bob's daemon then inserts Alice into its own mesh_peers.
//   4. After this, both daemons can verify each other's signed
//      `post_completion` messages and the mesh is paired.
//
// Security model — explicitly NOT trying to defend against:
//   - A peer on the same LAN who can sniff mDNS broadcasts (the
//     pubkey is broadcast publicly; that's fine, pubkeys are public
//     by design).
//   - Offline brute-force after a captured network capture (the
//     code's entropy + 15-min TTL bound the attacker's window
//     in-real-time).
//
// What we DO defend against:
//   - Code guessing: 60 bits of entropy (12-char Crockford base32),
//     one-shot, TTL-bounded.
//   - Code reuse / race: atomic UPDATE-then-INSERT inside a single
//     transaction in `try_consume_invite`, so two concurrent
//     consume_invite RPCs for the same code cannot both succeed.
//   - Format spoofing: validate_code_format() rejects anything not
//     matching the expected shape before we ever touch the DB. The
//     parameterized query is the real defense; this is a second
//     layer that also helps with operator typos.
//   - Wrong-daemon redirect: the consumer pins `--expect-peer-id`
//     when running `mesh invite consume`. If the daemon they dial
//     replies with a different peer_id (DNS poison, MitM, typo),
//     the pairing aborts BEFORE writing local mesh_peers.
//   - Database transplant: the invite row stores `issuer_pubkey` at
//     creation time. The daemon-side consume handler verifies that
//     value still matches the daemon's current pubkey before
//     committing. Prevents a copied DB from accidentally accepting
//     invites that were originally issued by a different machine.
//   - Information leakage on bad codes: the "invalid code" reply
//     doesn't say WHY (expired vs. unknown vs. already-consumed)
//     so an attacker can't probe to learn which prefixes are valid.

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use rand::RngCore;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

use crate::daemon::DEFAULT_DAEMON_PORT;
use crate::output::{emit_human, emit_json, Opts};

// Crockford base32 — no I, L, O, U. 32 chars → 5 bits per char.
// 12 chars across three groups = 60 bits of entropy. With a 15-min
// TTL and one-shot consumption, the brute-force window is
// vanishingly small.
const CROCKFORD_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CODE_PREFIX: &str = "ATO-";
const CODE_GROUP_LEN: usize = 4;
const CODE_GROUPS: usize = 3;
pub const DEFAULT_TTL_MINUTES: i64 = 15;
// Beyond 24h, the operator should rotate. Enforced server-side as a
// hard cap on `--expires`.
const MAX_TTL_MINUTES: i64 = 60 * 24;

/// Generate a fresh, formatted invite code.
///
/// Returns `ATO-XXXX-XXXX-XXXX` where each X is a Crockford base32
/// character drawn from the OS CSPRNG. 60 bits of entropy total.
pub fn generate_invite_code() -> String {
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
            let byte = bytes[g * CODE_GROUP_LEN + i];
            // Mask to 5 bits — the high 3 bits of each random byte
            // are unused but discarded uniformly so there's no bias
            // toward any character class.
            let idx = (byte & 0x1f) as usize;
            s.push(CROCKFORD_ALPHABET[idx] as char);
        }
    }
    s
}

/// Strict format check: `ATO-XXXX-XXXX-XXXX` where each X is in the
/// Crockford alphabet. Defense layer beyond parameterized queries —
/// also catches operator typos before we touch the DB.
pub fn validate_code_format(code: &str) -> bool {
    let expected_len = CODE_PREFIX.len() + CODE_GROUP_LEN * CODE_GROUPS + (CODE_GROUPS - 1);
    if code.len() != expected_len {
        return false;
    }
    if !code.starts_with(CODE_PREFIX) {
        return false;
    }
    let body = &code[CODE_PREFIX.len()..];
    let mut group_chars = 0usize;
    let mut groups_seen = 0usize;
    for c in body.bytes() {
        if c == b'-' {
            if group_chars != CODE_GROUP_LEN {
                return false;
            }
            groups_seen += 1;
            group_chars = 0;
            continue;
        }
        if !CROCKFORD_ALPHABET.contains(&c) {
            return false;
        }
        group_chars += 1;
        if group_chars > CODE_GROUP_LEN {
            return false;
        }
    }
    // Last group + accounting check.
    group_chars == CODE_GROUP_LEN && groups_seen == CODE_GROUPS - 1
}

/// A peer_id is the sha256 hex of the peer's Ed25519 pubkey — 64
/// lowercase hex chars. Validate the shape before passing to
/// destructive ops like `mesh peers remove`.
pub fn validate_peer_id_format(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[derive(Debug, Serialize)]
pub struct InviteRow {
    pub code: String,
    pub issued_at: String,
    pub expires_at: String,
    pub consumed: bool,
    pub issuer_pubkey: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PeerRow {
    pub peer_id: String,
    pub name: String,
    pub paired_at: String,
    pub last_seen_at: Option<String>,
    pub notes: Option<String>,
}

/// Open the SQLite DB read-write WITHOUT SQLITE_OPEN_CREATE. A typo
/// like `--db ~/.ato/locla.db` should fail fast with a clear "file
/// not found" rather than silently creating an empty DB and then
/// breaking on the first query.
///
/// Also runs the CLI-side schema bootstrap so a fresh `ato` binary
/// running against a DB that hasn't been opened by the desktop since
/// the last migration still picks up new mesh columns. The desktop
/// owns the authoritative migrations in `lib.rs`; this is purely a
/// defensive mirror for headless / CLI-first invocations.
fn open_db_strict(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
    )
    .with_context(|| format!("open {} (read-write, no create)", db_path.display()))?;
    bootstrap_mesh_columns(&conn);
    Ok(conn)
}

/// Defensive ALTERs that the desktop also runs. SQLite has no
/// `ALTER TABLE … ADD COLUMN IF NOT EXISTS`; we swallow the
/// "duplicate column" error with `let _ =` exactly like the desktop
/// migration block does. Runs on every CLI invocation — costs ~10µs
/// when the columns already exist.
fn bootstrap_mesh_columns(conn: &Connection) {
    let _ = conn.execute(
        "ALTER TABLE mesh_invites ADD COLUMN issuer_pubkey TEXT",
        [],
    );
}

/// True if `e` is a UNIQUE PRIMARY KEY violation. Used by the
/// invite_create retry loop. Matches on the structured error code
/// rather than the error message string — survives rusqlite version
/// bumps that reshape `Display` and avoids accidental matches on a
/// future UNIQUE on some other column.
// pub for daemon/protocol.rs retry loops — same definition, one place. (audit L2)
pub fn is_unique_pk_violation(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(ff, _)
            if ff.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

/// CLI: `ato mesh invite create [--expires <minutes>]`.
///
/// Creates a single one-shot invite, persisted alongside the issuing
/// daemon's pubkey so the consume side can verify it came from the
/// expected machine. Prints the code + expiry + dial command for
/// the operator to relay out-of-band.
pub fn invite_create(db_path: &Path, expires_minutes: i64, opts: &Opts) -> Result<()> {
    if !(1..=MAX_TTL_MINUTES).contains(&expires_minutes) {
        anyhow::bail!(
            "expires must be between 1 and {} minutes (got {})",
            MAX_TTL_MINUTES,
            expires_minutes
        );
    }
    // Bind the invite to the issuer's identity. Loading the daemon
    // keys here doesn't require the daemon to be running — the
    // pubkey is on disk at ~/.ato/daemon/keys/public.bin. If the
    // keys haven't been generated yet (first invocation), `status`
    // generates them.
    let issuer = crate::daemon::status()
        .context("read daemon identity (run `ato daemon start` once if this is a first-time setup)")?;
    let conn = open_db_strict(db_path)?;
    let now = chrono::Utc::now();
    let expires_at = now + chrono::Duration::minutes(expires_minutes);
    // Loop in case of an astronomically unlikely PK collision. 60 bits
    // of entropy makes this essentially impossible; the bound is
    // defense-in-depth, not a real expectation.
    for _attempt in 0..5 {
        let code = generate_invite_code();
        let res = conn.execute(
            "INSERT INTO mesh_invites (code, issued_at, expires_at, consumed, issuer_pubkey)
             VALUES (?1, ?2, ?3, 0, ?4)",
            params![
                code,
                now.to_rfc3339(),
                expires_at.to_rfc3339(),
                issuer.public_key_b64
            ],
        );
        match res {
            Ok(_) => {
                let row = InviteRow {
                    code: code.clone(),
                    issued_at: now.to_rfc3339(),
                    expires_at: expires_at.to_rfc3339(),
                    consumed: false,
                    issuer_pubkey: Some(issuer.public_key_b64.clone()),
                };
                if opts.human {
                    emit_human(&format!(
                        "Invite code: {}\n  Expires:    {} ({} minutes from now)\n\nShare the code with the peer out-of-band (Slack, in person, etc.).\nThey then run:\n  ato mesh invite consume {} --host <your-host> --port {}",
                        code,
                        expires_at.to_rfc3339(),
                        expires_minutes,
                        code,
                        DEFAULT_DAEMON_PORT,
                    ));
                } else {
                    emit_json(&row)?;
                }
                return Ok(());
            }
            Err(ref e) if is_unique_pk_violation(e) => continue,
            Err(e) => return Err(anyhow!("INSERT mesh_invites: {}", e)),
        }
    }
    anyhow::bail!("could not generate a unique invite code after 5 attempts (DB or RNG issue)")
}

/// CLI: `ato mesh invite list`. Active (unconsumed, unexpired) by
/// default; `--all` includes consumed/expired.
pub fn invite_list(db_path: &Path, include_all: bool, opts: &Opts) -> Result<()> {
    let conn = open_db_strict(db_path)?;
    let now = chrono::Utc::now().to_rfc3339();
    let sql = if include_all {
        "SELECT code, issued_at, expires_at, consumed, issuer_pubkey
         FROM mesh_invites ORDER BY issued_at DESC"
    } else {
        "SELECT code, issued_at, expires_at, consumed, issuer_pubkey
         FROM mesh_invites
         WHERE consumed = 0 AND expires_at > ?1
         ORDER BY issued_at DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let map_row = |r: &rusqlite::Row| -> rusqlite::Result<InviteRow> {
        Ok(InviteRow {
            code: r.get(0)?,
            issued_at: r.get(1)?,
            expires_at: r.get(2)?,
            consumed: r.get::<_, i64>(3)? != 0,
            issuer_pubkey: r.get(4).ok(),
        })
    };
    let rows: Vec<InviteRow> = if include_all {
        stmt.query_map([], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map([&now], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    if opts.human {
        if rows.is_empty() {
            emit_human(if include_all {
                "No invites have ever been issued."
            } else {
                "No active invites. Create one with `ato mesh invite create`."
            });
        } else {
            emit_human(&format!("{} invite(s):", rows.len()));
            for r in &rows {
                emit_human(&format!(
                    "  {}  issued={}  expires={}  consumed={}",
                    r.code, r.issued_at, r.expires_at, r.consumed
                ));
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

/// CLI: `ato mesh peers list`. Read-only view of `mesh_peers`.
pub fn peers_list(db_path: &Path, opts: &Opts) -> Result<()> {
    let conn = open_db_strict(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT peer_id, name, paired_at, last_seen_at, notes
         FROM mesh_peers ORDER BY paired_at DESC",
    )?;
    let rows: Vec<PeerRow> = stmt
        .query_map([], |r| {
            Ok(PeerRow {
                peer_id: r.get(0)?,
                name: r.get(1)?,
                paired_at: r.get(2)?,
                last_seen_at: r.get(3).ok(),
                notes: r.get(4).ok(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if opts.human {
        if rows.is_empty() {
            emit_human(
                "No paired peers yet. Create an invite with `ato mesh invite create`, send the code to the peer, and have them run `ato mesh invite consume`.",
            );
        } else {
            emit_human(&format!("{} paired peer(s):", rows.len()));
            for p in &rows {
                emit_human(&format!(
                    "  {:20}  peer_id={:.16}…  paired={}  last_seen={}",
                    p.name,
                    p.peer_id,
                    p.paired_at,
                    p.last_seen_at.as_deref().unwrap_or("never"),
                ));
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

/// CLI: `ato mesh peers remove <peer_id>`. Best-effort delete by PK
/// after validating the input format so a typo doesn't silently
/// match zero rows behind a vague "no peer" message.
pub fn peers_remove(db_path: &Path, peer_id: &str, opts: &Opts) -> Result<()> {
    if !validate_peer_id_format(peer_id) {
        anyhow::bail!(
            "peer_id must be a 64-character lowercase hex string (sha256 of the peer's pubkey); got {} chars",
            peer_id.len()
        );
    }
    let conn = open_db_strict(db_path)?;
    let affected = conn.execute(
        "DELETE FROM mesh_peers WHERE peer_id = ?1",
        params![peer_id],
    )?;
    if opts.human {
        if affected == 0 {
            emit_human(&format!("No paired peer with peer_id {}.", peer_id));
        } else {
            emit_human(&format!("Removed peer {}.", peer_id));
        }
    } else {
        #[derive(Serialize)]
        struct Out<'a> {
            peer_id: &'a str,
            removed: usize,
        }
        emit_json(&Out { peer_id, removed: affected })?;
    }
    Ok(())
}

/// Atomically claim an invite code and run the caller's pairing
/// step inside the same transaction. Used by the `consume_invite`
/// JSON-RPC handler in `daemon/protocol.rs`.
///
/// The closure receives the SELECTed `InviteRow` (including the
/// stored `issuer_pubkey`) so the caller can verify the invite was
/// issued by the current daemon (defense against a copied DB from a
/// different machine accepting an invite that wasn't really its
/// own). The closure then INSERTs the consumer into mesh_peers
/// using fields from the RPC payload. The whole "code claimed +
/// peer paired" transition is atomic.
///
/// Returns `Ok(Some(T))` when the code was claimed and the closure
/// succeeded; `Ok(None)` when the code is missing/expired/consumed
/// (single error path — no leakage about which condition failed);
/// `Err` only for actual DB failures or when the closure itself
/// rejects (and the tx rolls back, returning the invite to the
/// unconsumed pool).
pub fn try_consume_invite<F, T>(
    conn: &mut Connection,
    code: &str,
    on_claimed: F,
) -> Result<Option<T>>
where
    F: FnOnce(&Transaction<'_>, &InviteRow) -> Result<T>,
{
    if !validate_code_format(code) {
        return Ok(None);
    }
    let tx = conn.transaction().context("begin try_consume_invite tx")?;
    let now = chrono::Utc::now().to_rfc3339();
    // Atomic claim. Returns affected rows; 0 means "no eligible
    // invite" (either nonexistent, expired, or already consumed).
    let n = tx
        .execute(
            "UPDATE mesh_invites SET consumed = 1
             WHERE code = ?1 AND consumed = 0 AND expires_at > ?2",
            params![code, now],
        )
        .context("UPDATE mesh_invites (claim)")?;
    if n == 0 {
        // Nothing to commit. Let the tx drop = rollback.
        return Ok(None);
    }
    // Read back the row we just claimed (including the issuer_pubkey
    // the caller needs to verify the consumer).
    let row: InviteRow = tx
        .query_row(
            "SELECT code, issued_at, expires_at, consumed, issuer_pubkey
             FROM mesh_invites WHERE code = ?1",
            params![code],
            |r| {
                Ok(InviteRow {
                    code: r.get(0)?,
                    issued_at: r.get(1)?,
                    expires_at: r.get(2)?,
                    consumed: r.get::<_, i64>(3)? != 0,
                    issuer_pubkey: r.get(4).ok(),
                })
            },
        )
        .context("SELECT just-claimed invite")?;
    // Caller does its pairing INSERT inside this same tx. If it
    // returns Err, tx drops = rollback = invite returns to the pool.
    let out = on_claimed(&tx, &row)?;
    tx.commit().context("commit try_consume_invite tx")?;
    Ok(Some(out))
}

/// CLI: `ato mesh invite consume <code> --host H --port P --expect-peer-id X`.
///
/// The consumer side of the pairing. Opens a WebSocket client to
/// the issuer's daemon, sends a `consume_invite` JSON-RPC request
/// carrying our own identity, validates the issuer's reply against
/// the pinned peer_id, and stores the issuer into our local
/// `mesh_peers` only after the pin check passes.
///
/// Bounded by a 10s overall wall clock (connect + write + read).
/// The issuer's daemon does the actual atomic claim inside
/// `try_consume_invite`; this side is purely the network client +
/// "store after verify" half.
pub fn invite_consume(
    db_path: &Path,
    code: &str,
    host: &str,
    port: u16,
    expect_peer_id: &str,
    note: Option<&str>,
    opts: &Opts,
) -> Result<()> {
    // Fast-fail input validation BEFORE we dial anything.
    if !validate_code_format(code) {
        anyhow::bail!(
            "code must match ATO-XXXX-XXXX-XXXX with Crockford base32 chars (no I/L/O/U)"
        );
    }
    if !validate_peer_id_format(expect_peer_id) {
        anyhow::bail!(
            "--expect-peer-id must be a 64-character lowercase hex string (the issuer prints theirs on `mesh invite create`)"
        );
    }
    if host.is_empty() {
        anyhow::bail!("--host is required");
    }

    // Read our own identity so we can announce ourselves to the
    // issuer. status() loads (or generates on first use) the
    // daemon keys at ~/.ato/daemon/keys/.
    let me = crate::daemon::status()
        .context("read local daemon identity (run `ato daemon start` once if first-time setup)")?;
    let machine_name = hostname::get()
        .ok()
        .and_then(|s| s.into_string().ok())
        .unwrap_or_else(|| "unknown-host".into());

    // Build the request locally — minimal, no Tokio touches yet.
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "consume-1",
        "method": "consume_invite",
        "params": {
            "code": code,
            "sender_peer_id": me.peer_id,
            "sender_public_key_b64": me.public_key_b64,
            "sender_machine_name": machine_name,
        }
    });

    // Spin up a tokio runtime for the duration of the dial. Single
    // thread is enough — one outbound WS, sub-second total. We avoid
    // making the entire CLI async just for this one network call.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    let url = format!("ws://{}:{}", host, port);
    let reply: ConsumeInviteWireReply = rt
        .block_on(async {
            // 10s overall budget: connect + send + receive.
            tokio::time::timeout(
                Duration::from_secs(10),
                ws_consume_invite_call(&url, &req),
            )
            .await
            .map_err(|_| anyhow!("timed out talking to {} (10s)", url))?
        })
        .with_context(|| format!("consume_invite roundtrip to {}", url))?;

    // Server-side errors come back as a JSON-RPC error object. The
    // issuer's daemon collapses every reject reason into a single
    // -32004 "invalid invite" so we can't probe what's wrong.
    if let Some(err) = reply.error {
        anyhow::bail!(
            "issuer rejected consume_invite: {} (code {}). Common causes: code was already consumed, code expired, code typed wrong.",
            err.message,
            err.code
        );
    }
    let result = reply.result.ok_or_else(|| {
        anyhow!("issuer returned no result and no error — protocol violation")
    })?;

    // PIN CHECK: the issuer's claimed peer_id MUST match what the
    // user expected via --expect-peer-id. This is the defense
    // against dialing the wrong daemon (DNS poison, MitM, typo).
    if result.peer_id != expect_peer_id {
        anyhow::bail!(
            "issuer peer_id mismatch — expected {} but got {}. ABORTING pairing. If you trust the issuer's actual peer_id, re-run with the correct --expect-peer-id.",
            expect_peer_id,
            result.peer_id
        );
    }
    // Derived peer_id must match the returned pubkey too (peer_id =
    // sha256(pubkey)). Catches a daemon that lies about either.
    let returned_pubkey = base64::engine::general_purpose::STANDARD
        .decode(result.public_key_b64.as_bytes())
        .context("decode issuer pubkey b64")?;
    if returned_pubkey.len() != 32 {
        anyhow::bail!(
            "issuer pubkey wrong length: {} bytes (expected 32 for Ed25519)",
            returned_pubkey.len()
        );
    }
    let derived = peer_id_hex_from_bytes(&returned_pubkey);
    if derived != result.peer_id {
        anyhow::bail!(
            "issuer peer_id does not match the sha256 of their returned pubkey. ABORTING pairing."
        );
    }

    // chunk-2 review (claude, LOW): bound the issuer's reply
    // before we commit it to local storage. Trust boundary
    // asymmetry — we opted in to this peer, but a hostile or
    // buggy issuer shouldn't be able to write a multi-MB string
    // into our mesh_peers.name column. Matches the daemon-side
    // 256 limit for symmetry.
    if result.machine_name.len() > 256 {
        anyhow::bail!(
            "issuer machine_name is {} bytes (max 256) — refusing to store",
            result.machine_name.len()
        );
    }
    // All checks passed — store the issuer into local mesh_peers.
    // chunk-2 review (claude, LOW): use ON CONFLICT DO UPDATE so a
    // re-pair preserves prior `notes` / `last_seen_at` rather than
    // nuking them via INSERT OR REPLACE. The caller-supplied --note
    // only overrides when explicitly passed.
    let conn = open_db_strict(db_path)?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO mesh_peers
           (peer_id, public_key, name, paired_at, last_seen_at, notes)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5)
         ON CONFLICT(peer_id) DO UPDATE SET
           public_key = excluded.public_key,
           name       = excluded.name,
           paired_at  = excluded.paired_at,
           notes      = COALESCE(excluded.notes, mesh_peers.notes)",
        params![
            result.peer_id,
            result.public_key_b64,
            result.machine_name,
            now,
            note,
        ],
    )?;

    if opts.human {
        emit_human(&format!(
            "Paired with {}\n  peer_id:    {}\n  pubkey:     {}\n  via:        ws://{}:{}\n",
            result.machine_name, result.peer_id, result.public_key_b64, host, port
        ));
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ConsumeInviteWireReply {
    #[serde(default)]
    result: Option<ConsumeInviteReplyResult>,
    #[serde(default)]
    error: Option<JsonRpcErrorObject>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ConsumeInviteReplyResult {
    peer_id: String,
    public_key_b64: String,
    machine_name: String,
}

#[derive(Debug, Deserialize)]
struct JsonRpcErrorObject {
    code: i32,
    message: String,
}

/// Internal: open the WS connection, send the request, read one
/// reply text frame, decode it. Bounded by the caller's timeout.
async fn ws_consume_invite_call(
    url: &str,
    req: &serde_json::Value,
) -> Result<ConsumeInviteWireReply> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let (mut ws, _resp) = tokio_tungstenite::connect_async(url)
        .await
        .with_context(|| format!("connect_async {}", url))?;
    let body = serde_json::to_string(req).context("serialize consume_invite request")?;
    ws.send(Message::Text(body.into()))
        .await
        .context("send consume_invite frame")?;
    // Read until we get a text frame (skipping pings / close).
    let mut reply: Option<ConsumeInviteWireReply> = None;
    while let Some(frame) = ws.next().await {
        let m = frame.context("read ws frame")?;
        match m {
            Message::Text(t) => {
                let parsed: ConsumeInviteWireReply =
                    serde_json::from_str(&t).context("decode consume_invite reply")?;
                reply = Some(parsed);
                break;
            }
            Message::Close(_) => anyhow::bail!("peer closed before reply"),
            _ => continue, // pings, binary — ignore
        }
    }
    // Send a proper Close frame so the issuer's daemon log doesn't
    // see a "Connection reset without closing handshake" every
    // single pairing. Best-effort: if the close itself errors,
    // the reply we already captured is what matters.
    let _ = ws.send(Message::Close(None)).await;
    reply.ok_or_else(|| anyhow!("peer disconnected without sending a reply"))
}

fn peer_id_hex_from_bytes(pubkey: &[u8]) -> String {
    use std::fmt::Write as _;
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(pubkey);
    let digest: [u8; 32] = h.finalize().into();
    let mut s = String::with_capacity(64);
    for b in digest {
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

/// Read an invite without claiming it. Used only for display /
/// debugging surfaces (`ato mesh invite list` covers the normal
/// case). Not safe for the pairing path — see `try_consume_invite`.
#[allow(dead_code)] // Reserved for future debug commands.
pub fn peek_invite(conn: &Connection, code: &str) -> Result<Option<InviteRow>> {
    if !validate_code_format(code) {
        return Ok(None);
    }
    let row: Option<InviteRow> = conn
        .query_row(
            "SELECT code, issued_at, expires_at, consumed, issuer_pubkey
             FROM mesh_invites WHERE code = ?1",
            params![code],
            |r| {
                Ok(InviteRow {
                    code: r.get(0)?,
                    issued_at: r.get(1)?,
                    expires_at: r.get(2)?,
                    consumed: r.get::<_, i64>(3)? != 0,
                    issuer_pubkey: r.get(4).ok(),
                })
            },
        )
        .optional()?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_code_has_correct_shape() {
        for _ in 0..50 {
            let c = generate_invite_code();
            assert!(validate_code_format(&c), "rejected own code: {}", c);
            assert!(c.starts_with("ATO-"));
            assert_eq!(c.len(), 4 + 4 + 1 + 4 + 1 + 4);
        }
    }

    #[test]
    fn code_format_rejects_lowercase() {
        // Crockford is uppercase only.
        assert!(!validate_code_format("ATO-abcd-efgh-ijkl"));
    }

    #[test]
    fn code_format_rejects_ambiguous_chars() {
        // I, L, O, U are NOT in the Crockford alphabet.
        assert!(!validate_code_format("ATO-IIII-IIII-IIII"));
        assert!(!validate_code_format("ATO-LLLL-LLLL-LLLL"));
        assert!(!validate_code_format("ATO-OOOO-OOOO-OOOO"));
        assert!(!validate_code_format("ATO-UUUU-UUUU-UUUU"));
    }

    #[test]
    fn code_format_rejects_wrong_prefix() {
        // Must be 18 bytes (correct length) so the prefix branch
        // actually fires — earlier version of this test used a
        // 14-byte string that was rejected by the length check.
        // Caught by claude review on chunk 1.
        assert!(!validate_code_format("XTO-ABCD-1234-WXYZ"));
        assert!(!validate_code_format("ATO0ABCD-1234-WXYZ"));
        assert!(!validate_code_format("    -ABCD-1234-WXYZ"));
    }

    #[test]
    fn code_format_rejects_wrong_length() {
        assert!(!validate_code_format("ATO-ABC-1234-5678"));   // group too short
        assert!(!validate_code_format("ATO-ABCDE-1234-5678")); // group too long
        assert!(!validate_code_format("ATO-ABCD-1234"));        // missing group
        assert!(!validate_code_format("ATO-ABCD-1234-5678-9"));// trailing
    }

    #[test]
    fn code_format_accepts_valid_known_good() {
        // Hand-crafted with only chars that ARE in the Crockford
        // alphabet (no I, L, O, U).
        assert!(validate_code_format("ATO-ABCD-1234-WXYZ"));
        assert!(validate_code_format("ATO-0000-0000-0000"));
        assert!(validate_code_format("ATO-ZZZZ-ZZZZ-ZZZZ"));
    }

    #[test]
    fn peer_id_format_accepts_valid() {
        let v = "a".repeat(64);
        assert!(validate_peer_id_format(&v));
        let v = "0123456789abcdef".repeat(4);
        assert!(validate_peer_id_format(&v));
    }

    #[test]
    fn peer_id_format_rejects_uppercase() {
        let v = "A".repeat(64);
        assert!(!validate_peer_id_format(&v));
    }

    #[test]
    fn peer_id_format_rejects_wrong_length() {
        assert!(!validate_peer_id_format(&"a".repeat(63)));
        assert!(!validate_peer_id_format(&"a".repeat(65)));
        assert!(!validate_peer_id_format(""));
    }

    #[test]
    fn peer_id_format_rejects_non_hex() {
        let mut v = "a".repeat(63);
        v.push('z');
        assert!(!validate_peer_id_format(&v));
    }
}
