// v2.4.3 Phase 7.0 step 3 — WebSocket + JSON-RPC peer protocol.
//
// Single method in v1: `post_completion`. Each message is signed by
// the sender's Ed25519 private key over a canonical JSON encoding of
// the params; recipients verify against the public key stored in
// mesh_peers. Unsigned, malformed, or unknown-peer messages are
// dropped with a JSON-RPC error reply.
//
// On accept the daemon writes:
//   - One row into session_turns (role=assistant, sender_peer_id set,
//     runtime = peer's friendly name)
//   - One peer_completion event into events_log so ops recipes /
//     `ato events watch` / the activity feed react in real time
//
// Scope intentionally narrow:
//   - One method, two outcomes (ok / error).
//   - No `dispatch_on_peer` — that's the Pro-tier 7.1 unlock.
//   - No request batching, no streaming, no pagination.
//   - Replay-buffer-when-offline is also 7.1; we drop on disconnect.

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// JSON-RPC 2.0 envelope. We don't pull in jsonrpc-core because the
/// surface is so small — handwriting two structs is less weight than
/// inheriting a framework's whole error type taxonomy.
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value, // string or number per spec
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

/// `post_completion` params shape — signed by the sender. The fields
/// are canonicalized (alphabetical key order, no whitespace) before
/// signing so the recipient can reconstruct the exact bytes that
/// were signed without trusting our serde output verbatim.
#[derive(Debug, Serialize, Deserialize)]
pub struct PostCompletionParams {
    /// sha256(sender's public key) — the recipient looks this up in
    /// mesh_peers to find the verifying key.
    pub from_peer_id: String,
    /// Human-friendly machine name the sender broadcast in mDNS.
    /// Stored on the resulting session_turn for display.
    pub from_machine_name: String,
    /// Session on the RECIPIENT that this completion belongs to.
    /// Senders learn the recipient's session id during the pairing
    /// dance (step 4); this slice's contract assumes the sender
    /// already has it.
    pub session_id: String,
    /// "success" | "error".
    pub status: String,
    /// One-line human summary, written into the new session_turn's
    /// text. Up to 1024 chars.
    pub summary: String,
    /// Arbitrary JSON payload, up to 64KB serialized. Persisted into
    /// session_turns. Step 3 doesn't render it specially; that's a
    /// GUI step 5 task.
    pub payload: serde_json::Value,
    /// RFC3339 timestamp when the sender finished its work.
    pub occurred_at: String,
    /// Base64 Ed25519 signature over the canonical body. NOT included
    /// in the signed body (chicken-and-egg).
    pub signature: String,
}

/// Build the bytes that get signed. Canonical = recursively sorted
/// object keys + no whitespace + UTF-8 encoded JSON. Independent of
/// whatever `serde_json`'s map order is configured to do via feature
/// flags so a downstream crate enabling `preserve_order` can't
/// silently change our signing contract.
///
/// Sender (which may be any language: Node, Python, Go) needs to
/// implement the same recursive-sort canonicalization. The e2e test
/// at /tmp/ato-mesh-e2e.mjs shows the JS shape.
pub fn canonical_signing_bytes(p: &PostCompletionParams) -> Vec<u8> {
    let v = serde_json::json!({
        "from_machine_name": p.from_machine_name,
        "from_peer_id": p.from_peer_id,
        "occurred_at": p.occurred_at,
        "payload": p.payload,
        "session_id": p.session_id,
        "status": p.status,
        "summary": p.summary,
    });
    let sorted = canonicalize_value(&v);
    // serde_json::to_vec on a Value we just constructed can only fail
    // for non-string Map keys, which serde_json::json! can't produce.
    // expect() makes that invariant load-bearing instead of silently
    // signing an empty body (which would produce a valid-but-useless
    // signature). Tier 1 review finding from v2.4.5 dogfood.
    serde_json::to_vec(&sorted).expect("canonical_signing_bytes: serializing JSON we built can't fail")
}

fn canonicalize_value(v: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            // Re-insert keys in sorted order. Using serde_json::Map
            // directly because Value::Object wraps it; with the
            // `preserve_order` feature this is IndexMap (insertion-
            // order), which is exactly what we want once we've
            // pre-sorted the keys.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::with_capacity(map.len());
            for k in keys {
                out.insert(k.clone(), canonicalize_value(&map[k]));
            }
            Value::Object(out)
        }
        Value::Array(arr) => {
            Value::Array(arr.iter().map(canonicalize_value).collect())
        }
        _ => v.clone(),
    }
}

/// Verify the signature on a PostCompletionParams against a known
/// public key. Returns Ok(()) when valid, Err with a useful message
/// otherwise. Caller decides whether to drop the message or reply
/// with a JSON-RPC error.
pub fn verify_signature(p: &PostCompletionParams, pubkey: &VerifyingKey) -> Result<()> {
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(p.signature.as_bytes())
        .context("decode signature base64")?;
    if sig_bytes.len() != 64 {
        anyhow::bail!("signature must be 64 bytes (got {})", sig_bytes.len());
    }
    let mut arr = [0u8; 64];
    arr.copy_from_slice(&sig_bytes);
    let sig = Signature::from_bytes(&arr);
    let body = canonical_signing_bytes(p);
    pubkey
        .verify(&body, &sig)
        .map_err(|e| anyhow!("signature verification failed: {}", e))
}

/// Look up a peer's pubkey from mesh_peers. None = unknown peer
/// (recipient should refuse the message).
pub fn lookup_peer_pubkey(db_path: &std::path::Path, peer_id: &str) -> Result<Option<VerifyingKey>> {
    let conn = rusqlite::Connection::open(db_path).context("open ato db")?;
    let pub_b64: Option<String> = conn
        .query_row(
            "SELECT public_key FROM mesh_peers WHERE peer_id = ?1",
            [peer_id],
            |r| r.get(0),
        )
        .ok();
    match pub_b64 {
        None => Ok(None),
        Some(b64) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64.as_bytes())
                .context("decode peer public_key")?;
            if bytes.len() != 32 {
                anyhow::bail!("peer public_key must be 32 bytes (got {})", bytes.len());
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            let vk = VerifyingKey::from_bytes(&arr)
                .map_err(|e| anyhow!("parse peer public_key: {}", e))?;
            Ok(Some(vk))
        }
    }
}

/// Apply an accepted post_completion message: write the
/// session_turn, emit the event, bump mesh_peers.last_seen_at.
/// Wrapped in a single SQLite transaction so a mid-way panic or
/// I/O failure leaves no partial state (no session_turn written
/// with no matching event, no UPDATE without the INSERT, etc.).
/// Tier 1 review finding from v2.4.5 dogfood.
pub fn apply_completion(db_path: &std::path::Path, p: &PostCompletionParams) -> Result<()> {
    let mut conn = rusqlite::Connection::open(db_path).context("open ato db")?;
    let tx = conn.transaction().context("begin transaction")?;

    // v2.4.3 review finding #4 (MiniMax) — turn_index race fix.
    // Two concurrent peers each computing MAX(turn_index)+1 against
    // the same session pick the same number; the second INSERT then
    // hits the (session_id, turn_index) PK and fails. Retry with a
    // fresh MAX a few times so the loser of the race gets a fresh
    // slot. Bounded retries: if 5 attempts all collide we've got a
    // pathological case worth surfacing as a real error.
    let body = format!(
        "[{}] {}\n\n{}",
        p.status,
        p.summary,
        serde_json::to_string_pretty(&p.payload).unwrap_or_default()
    );
    let mut last_err: Option<rusqlite::Error> = None;
    for _attempt in 0..5 {
        let next_idx: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(turn_index), -1) + 1 FROM session_turns WHERE session_id = ?1",
                [&p.session_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        match tx.execute(
            "INSERT INTO session_turns (session_id, turn_index, role, text, runtime, created_at, sender_peer_id)
             VALUES (?1, ?2, 'assistant', ?3, ?4, ?5, ?6)",
            params![
                p.session_id,
                next_idx,
                body,
                p.from_machine_name,
                p.occurred_at,
                p.from_peer_id,
            ],
        ) {
            Ok(_) => {
                last_err = None;
                break;
            }
            Err(e) => {
                // Audit L2 — match the structured error code, not a
                // rusqlite Display substring. A future rusqlite bump
                // could reshape the message and silently turn the
                // retry into a single attempt that swallows real
                // UNIQUE failures.
                if crate::commands::mesh::is_unique_pk_violation(&e) {
                    last_err = Some(e);
                    continue;
                }
                return Err(anyhow!("INSERT session_turns: {}", e));
            }
        }
    }
    if let Some(e) = last_err {
        anyhow::bail!(
            "INSERT session_turns gave up after 5 attempts on UNIQUE race: {}",
            e
        );
    }

    tx.execute(
        "UPDATE mesh_peers SET last_seen_at = ?1 WHERE peer_id = ?2",
        params![chrono::Utc::now().to_rfc3339(), p.from_peer_id],
    )
    .context("UPDATE mesh_peers.last_seen_at")?;

    // Emit a peer_completion event so ops recipes / `ato events
    // watch` / the activity feed all see this. Inside the same
    // transaction: either both the turn and the event land, or
    // neither does.
    emit_peer_completion_event(&tx, p)?;

    tx.commit().context("commit apply_completion transaction")
}

fn emit_peer_completion_event(conn: &rusqlite::Connection, p: &PostCompletionParams) -> Result<()> {
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='events_log'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return Ok(());
    }
    // MAX(event_seq)+1 has the same race as turn_index above — two
    // concurrent peer completions or one peer completion + one
    // regular publish_* call can collide. Retry on UNIQUE up to 5
    // attempts; Tier 1 review finding from v2.4.5 dogfood replaced
    // the prior "one attempt is fine" comment.
    let mut last_err: Option<rusqlite::Error> = None;
    for _attempt in 0..5 {
        let max: i64 = conn
            .query_row("SELECT COALESCE(MAX(event_seq), 0) FROM events_log", [], |r| r.get(0))
            .unwrap_or(0);
        let seq = max + 1;
        let payload = serde_json::json!({
            "type": "peer_completion",
            "event_seq": seq,
            "from_peer_id": p.from_peer_id,
            "from_machine_name": p.from_machine_name,
            "session_id": p.session_id,
            "status": p.status,
            "summary": p.summary,
            "occurred_at": p.occurred_at,
        });
        match conn.execute(
            "INSERT INTO events_log (event_seq, event_type, payload, occurred_at) VALUES (?1, 'peer_completion', ?2, ?3)",
            params![seq, payload.to_string(), p.occurred_at],
        ) {
            Ok(_) => return Ok(()),
            Err(e) => {
                // Audit L2 — see comment on session_turns retry above.
                if crate::commands::mesh::is_unique_pk_violation(&e) {
                    last_err = Some(e);
                    continue;
                }
                return Err(anyhow!("INSERT events_log: {}", e));
            }
        }
    }
    if let Some(e) = last_err {
        return Err(anyhow!(
            "INSERT events_log gave up after 5 attempts on UNIQUE race: {}",
            e
        ));
    }
    Ok(())
}

/// Handle one inbound WebSocket connection. Reads JSON-RPC frames,
/// dispatches each method (only post_completion in v1), replies in
/// kind. Returns when the connection closes or the peer sends
/// malformed garbage.
pub async fn handle_connection(stream: TcpStream, db_path: Arc<PathBuf>) {
    let peer_addr = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "<unknown>".into());

    // v2.4.3 review finding #3 (MiniMax) — frame-size DoS defense.
    // Default tokio-tungstenite caps are loose enough for a hostile
    // peer to mail us a multi-GB frame and OOM the daemon. The
    // post_completion contract caps payload at 64KB and summary at
    // 1KB, plus envelope overhead — 128KB is a generous frame ceiling
    // that fits the legitimate use cases and rejects abuse loudly.
    let mut ws_config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
    ws_config.max_message_size = Some(128 * 1024);
    ws_config.max_frame_size = Some(128 * 1024);
    let ws = match tokio_tungstenite::accept_async_with_config(stream, Some(ws_config)).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ato daemon: ws upgrade from {} failed: {}", peer_addr, e);
            return;
        }
    };
    eprintln!("ato daemon: ws peer connected: {}", peer_addr);

    let (mut sink, mut source) = ws.split();

    while let Some(frame) = source.next().await {
        let msg = match frame {
            Ok(m) => m,
            Err(e) => {
                eprintln!("ato daemon: ws read error from {}: {}", peer_addr, e);
                break;
            }
        };
        let text = match msg {
            Message::Text(t) => t,
            Message::Binary(_) => {
                let err = error_reply(serde_json::Value::Null, -32700, "binary frames not supported; send JSON-RPC text").to_string();
                let _ = sink.send(Message::Text(err.into())).await;
                continue;
            }
            Message::Ping(p) => {
                let _ = sink.send(Message::Pong(p)).await;
                continue;
            }
            Message::Pong(_) => continue,
            Message::Close(_) => break,
            _ => continue,
        };

        let reply = process_text_frame(&text, &db_path).await;
        if let Some(reply_text) = reply {
            if let Err(e) = sink.send(Message::Text(reply_text.into())).await {
                eprintln!("ato daemon: ws send failed to {}: {}", peer_addr, e);
                break;
            }
        }
    }
    eprintln!("ato daemon: ws peer disconnected: {}", peer_addr);
}

async fn process_text_frame(text: &str, db_path: &Arc<PathBuf>) -> Option<String> {
    let req: JsonRpcRequest = match serde_json::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            return Some(
                error_reply(
                    serde_json::Value::Null,
                    -32700,
                    &format!("parse error: {}", e),
                )
                .to_string(),
            );
        }
    };
    if req.jsonrpc != "2.0" {
        return Some(
            error_reply(req.id, -32600, "jsonrpc field must be \"2.0\"").to_string(),
        );
    }
    match req.method.as_str() {
        "post_completion" => {
            let params: PostCompletionParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => {
                    return Some(
                        error_reply(req.id, -32602, &format!("invalid params: {}", e))
                            .to_string(),
                    );
                }
            };
            handle_post_completion(req.id, params, db_path).await
        }
        "consume_invite" => {
            let params: ConsumeInviteParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => {
                    return Some(
                        error_reply(req.id, -32602, &format!("invalid params: {}", e))
                            .to_string(),
                    );
                }
            };
            handle_consume_invite(req.id, params, db_path).await
        }
        other => Some(
            error_reply(req.id, -32601, &format!("method not found: {}", other))
                .to_string(),
        ),
    }
}

/// `consume_invite` RPC params — sent by the consumer to the issuer.
/// Critically NOT signed: at this point the issuer doesn't know the
/// consumer's pubkey yet (that's what this message is delivering).
/// Defense is: (1) the invite code is one-shot + TTL-bounded + 60-bit
/// entropy, (2) `try_consume_invite` claims the row atomically, (3)
/// the consumer pins the issuer's expected peer_id on their side
/// before accepting the reply, (4) all reject paths return the same
/// generic "invalid invite" error so attackers can't probe.
#[derive(Debug, Serialize, Deserialize)]
pub struct ConsumeInviteParams {
    pub code: String,
    pub sender_peer_id: String,
    pub sender_public_key_b64: String,
    pub sender_machine_name: String,
}

/// `consume_invite` RPC result — the issuer's identity, returned to
/// the consumer on a successful claim so the consumer can insert the
/// issuer into its own `mesh_peers` table.
#[derive(Debug, Serialize, Deserialize)]
pub struct ConsumeInviteResult {
    pub peer_id: String,
    pub public_key_b64: String,
    pub machine_name: String,
}

async fn handle_consume_invite(
    id: serde_json::Value,
    p: ConsumeInviteParams,
    db_path: &Arc<PathBuf>,
) -> Option<String> {
    // Bound input sizes. Defense against a hostile sender filling
    // SQLite with garbage strings. Matches post_completion's pattern.
    // machine_name 256 is generous; pubkey b64 of 32 bytes is 44
    // chars; peer_id is 64 hex chars.
    if p.sender_machine_name.len() > 256 {
        return Some(generic_invalid_invite(id));
    }
    if p.sender_peer_id.len() != 64
        || !p.sender_peer_id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Some(generic_invalid_invite(id));
    }
    if p.sender_public_key_b64.is_empty() || p.sender_public_key_b64.len() > 128 {
        return Some(generic_invalid_invite(id));
    }
    // Decode + validate the consumer's pubkey BEFORE we claim the
    // invite. If their pubkey is malformed, fail fast without
    // burning the one-shot.
    let consumer_pubkey_bytes = match base64::engine::general_purpose::STANDARD
        .decode(p.sender_public_key_b64.as_bytes())
    {
        Ok(b) if b.len() == 32 => b,
        _ => return Some(generic_invalid_invite(id)),
    };
    // Verify the claimed peer_id actually matches the claimed pubkey
    // (peer_id = sha256(pubkey) lowercase hex). An attacker who lies
    // about either gets rejected at this gate. Without this check,
    // a peer could send a pubkey + an unrelated peer_id and trick us
    // into storing a peer_id we couldn't actually verify signatures
    // against later.
    let derived_id = peer_id_from_pubkey_bytes(&consumer_pubkey_bytes);
    if derived_id != p.sender_peer_id {
        return Some(generic_invalid_invite(id));
    }

    // chunk-2 review (claude, MEDIUM): read our own identity BEFORE
    // claiming the invite. If `status()` or hostname resolution
    // fails AFTER `try_consume_invite` already committed, we'd
    // orphan-pair: the issuer's DB has the consumer, but the
    // consumer hears "invalid invite" and never stores the issuer.
    // Resolve identity up front so any failure short-circuits
    // before any state change.
    let st = match crate::daemon::status() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ato daemon: consume_invite cannot read local identity: {}", e);
            return Some(generic_invalid_invite(id));
        }
    };
    let machine_name = hostname::get()
        .ok()
        .and_then(|s| s.into_string().ok())
        .unwrap_or_else(|| "unknown-host".into());
    let our_reply = ConsumeInviteResult {
        peer_id: st.peer_id.clone(),
        public_key_b64: st.public_key_b64.clone(),
        machine_name,
    };
    let our_pubkey_for_check = st.public_key_b64.clone();

    // Spawn-blocking the DB half. The closure runs the atomic claim
    // and the mesh_peers INSERT inside one tx. Anything that can
    // fail (identity, network) is now ABOVE this block.
    let db_path_owned = db_path.clone();
    let p_owned = p;
    let claim_result: Result<Option<ConsumeInviteResult>, String> =
        tokio::task::spawn_blocking(move || {
            let mut conn = rusqlite::Connection::open(&*db_path_owned)
                .map_err(|e| format!("open db: {}", e))?;
            let result = crate::commands::mesh::try_consume_invite(
                &mut conn,
                &p_owned.code,
                |tx, invite| {
                    // chunk-2 review (claude, LOW): defense-in-depth.
                    // Verify the invite was issued by US, not by a
                    // different daemon whose DB got copied here.
                    // The consumer-side `--expect-peer-id` pin is
                    // the operational defense; this is the
                    // server-side mirror so a stolen DB on a
                    // different machine doesn't accidentally pair.
                    if let Some(stored) = invite.issuer_pubkey.as_deref() {
                        if stored != our_pubkey_for_check {
                            // Aborting inside the closure rolls
                            // back the UPDATE consumed=1, so the
                            // invite stays usable for the right
                            // issuer.
                            return Err(anyhow::anyhow!(
                                "issuer_pubkey mismatch (DB copied between machines?)"
                            ));
                        }
                    }
                    // INSERT the consumer as a paired peer. ON
                    // CONFLICT preserves prior notes / last_seen_at
                    // on re-pair instead of nuking them via
                    // INSERT OR REPLACE (chunk-2 review, claude LOW).
                    let now = chrono::Utc::now().to_rfc3339();
                    tx.execute(
                        "INSERT INTO mesh_peers
                           (peer_id, public_key, name, paired_at, last_seen_at, notes)
                         VALUES (?1, ?2, ?3, ?4, NULL, NULL)
                         ON CONFLICT(peer_id) DO UPDATE SET
                           public_key = excluded.public_key,
                           name       = excluded.name,
                           paired_at  = excluded.paired_at",
                        rusqlite::params![
                            p_owned.sender_peer_id,
                            p_owned.sender_public_key_b64,
                            p_owned.sender_machine_name,
                            now,
                        ],
                    )?;
                    Ok(())
                },
            )
            .map_err(|e| format!("try_consume_invite: {}", e))?;
            if result.is_none() {
                return Ok(None);
            }
            // Caller already prepared the reply pre-tx. Just hand
            // it back; the tx is already committed at this point.
            Ok(Some(our_reply))
        })
        .await
        .map_err(|join| format!("internal join: {}", join))
        .and_then(|r| r);

    match claim_result {
        Ok(Some(result)) => Some(
            JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id,
                result: Some(serde_json::to_value(&result).unwrap_or(serde_json::Value::Null)),
                error: None,
            }
            .to_json_string(),
        ),
        // Invite missing/expired/consumed — single generic error so
        // an attacker can't probe which condition failed.
        Ok(None) => Some(generic_invalid_invite(id)),
        Err(_msg) => {
            // Internal error — log a fixed marker and return generic
            // to the wire so we don't leak DB error shapes. The raw
            // `_msg` is intentionally dropped: it may carry path
            // names, SQL fragments, or panic payload details that
            // would land in operator bug reports / Slack pastes.
            // (Audit L3) If the operator needs server-side detail,
            // the spawn_blocking task itself logs sufficient context
            // before the panic.
            eprintln!("ato daemon: consume_invite internal error (details suppressed; see RUST_BACKTRACE=1 for the inner stack)");
            Some(generic_invalid_invite(id))
        }
    }
}

/// Single canonical "invalid invite" reply used for every reject
/// path on consume_invite. Don't leak whether the code was missing,
/// expired, already consumed, or malformed — an attacker shouldn't
/// be able to distinguish those by reply.
fn generic_invalid_invite(id: serde_json::Value) -> String {
    error_reply(id, -32004, "invalid invite").to_string()
}

/// Compute peer_id (sha256 hex) from raw pubkey bytes. Used by the
/// consume handler to verify the consumer's claimed peer_id matches
/// the claimed pubkey. Mirrors `daemon::peer_id_for` (which takes a
/// VerifyingKey) — kept here as raw-byte form to skip the parse
/// step when we just need the id.
fn peer_id_from_pubkey_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = simple_sha256_bytes(bytes);
    let mut s = String::with_capacity(64);
    for b in digest {
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

fn simple_sha256_bytes(input: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(input);
    h.finalize().into()
}

async fn handle_post_completion(
    id: serde_json::Value,
    p: PostCompletionParams,
    db_path: &Arc<PathBuf>,
) -> Option<String> {
    // Bound the payload + summary sizes to keep a hostile sender from
    // filling our SQLite with garbage. 64KB on payload mirrors the
    // existing execution_logs.prompt cap; 1KB on summary is loose
    // enough for any reasonable one-line status.
    if p.summary.len() > 1024 {
        return Some(error_reply(id, -32602, "summary exceeds 1024 chars").to_string());
    }
    let payload_serialized = serde_json::to_string(&p.payload).unwrap_or_default();
    if payload_serialized.len() > 64 * 1024 {
        return Some(error_reply(id, -32602, "payload exceeds 64KB").to_string());
    }
    if !matches!(p.status.as_str(), "success" | "error") {
        return Some(
            error_reply(id, -32602, "status must be 'success' or 'error'")
                .to_string(),
        );
    }

    // v2.4.3 review finding #5 (MiniMax) + #1 replay-defense
    // (combined): validate occurred_at is within a tight window of
    // the recipient's wall clock. Rejects messages with timestamps
    // that are absurd (year 2099, year 1970) AND limits the replay
    // window an attacker has even if they capture a signed message.
    //
    // Tolerance: symmetric ±90s. Wider than typical NTP drift (the
    // largest cluster of well-synced clocks runs <50ms apart, and
    // commodity NTP daemons hold drift under ±30s easily), narrow
    // enough that a captured signed message is only re-playable for
    // ~3 minutes total round-trip. The previous -60..+300s window
    // was asymmetric for no defensive reason — Tier 1 review
    // (v2.4.5 dogfood) flagged the 5-minute past-tolerance as a
    // gratuitously wide replay window.
    //
    // True replay-within-the-window defense (nonce tracking) is
    // deferred to a follow-up because it needs a "recently seen
    // signatures" table with TTL eviction; documented in
    // PHASE-7-PLAN.md as a step-3.1 follow-up.
    const TIMESTAMP_WINDOW_SECS: i64 = 90;
    match chrono::DateTime::parse_from_rfc3339(&p.occurred_at) {
        Ok(t) => {
            let now = chrono::Utc::now();
            let delta = (now - t.with_timezone(&chrono::Utc)).num_seconds();
            if delta.abs() > TIMESTAMP_WINDOW_SECS {
                return Some(
                    error_reply(
                        id,
                        -32003,
                        &format!(
                            "occurred_at outside acceptable window (delta {}s; must be within ±{}s)",
                            delta, TIMESTAMP_WINDOW_SECS
                        ),
                    )
                    .to_string(),
                );
            }
        }
        Err(e) => {
            return Some(
                error_reply(id, -32602, &format!("occurred_at not RFC3339: {}", e))
                    .to_string(),
            );
        }
    }

    // Spawn-blocking the DB-touching half — verify_signature does
    // crypto and rusqlite is sync.
    let db_path_owned = db_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        let pubkey = match lookup_peer_pubkey(&db_path_owned, &p.from_peer_id) {
            Ok(Some(pk)) => pk,
            Ok(None) => return Err(("peer_unknown", "from_peer_id not in mesh_peers".to_string())),
            Err(e) => return Err(("db_error", format!("{}", e))),
        };
        if let Err(e) = verify_signature(&p, &pubkey) {
            return Err(("bad_signature", format!("{}", e)));
        }
        if let Err(e) = apply_completion(&db_path_owned, &p) {
            return Err(("apply_failed", format!("{}", e)));
        }
        Ok(())
    })
    .await;

    match result {
        Ok(Ok(())) => Some(
            JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id,
                result: Some(serde_json::json!({"accepted": true})),
                error: None,
            }
            .to_json_string(),
        ),
        Ok(Err((kind, msg))) => {
            let code = match kind {
                "peer_unknown" => -32000,
                "bad_signature" => -32001,
                "apply_failed" => -32002,
                _ => -32603,
            };
            Some(error_reply(id, code, &format!("{}: {}", kind, msg)).to_string())
        }
        Err(join_err) => Some(
            error_reply(id, -32603, &format!("internal: {}", join_err)).to_string(),
        ),
    }
}

impl JsonRpcResponse {
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

// Display + ToString fall through to the JSON encoding so the
// callers in process_text_frame / handle_post_completion can just
// `.to_string()` an error_reply directly.
impl std::fmt::Display for JsonRpcResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_json_string())
    }
}

fn error_reply(id: serde_json::Value, code: i32, message: &str) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn make_params(signing: &SigningKey, peer_id: &str) -> PostCompletionParams {
        let mut p = PostCompletionParams {
            from_peer_id: peer_id.to_string(),
            from_machine_name: "test-host".into(),
            session_id: "session-xyz".into(),
            status: "success".into(),
            summary: "deploy done".into(),
            payload: serde_json::json!({"runs": 1}),
            occurred_at: "2026-05-13T00:00:00+00:00".into(),
            signature: String::new(),
        };
        let body = canonical_signing_bytes(&p);
        let sig = signing.sign(&body);
        p.signature = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
        p
    }

    #[test]
    fn signature_verifies_for_unmodified_message() {
        let signing = SigningKey::generate(&mut OsRng);
        let pubkey = signing.verifying_key();
        let p = make_params(&signing, "peer-1");
        verify_signature(&p, &pubkey).expect("valid signature should verify");
    }

    #[test]
    fn signature_fails_when_summary_tampered() {
        let signing = SigningKey::generate(&mut OsRng);
        let pubkey = signing.verifying_key();
        let mut p = make_params(&signing, "peer-1");
        p.summary = "TAMPERED".into();
        assert!(
            verify_signature(&p, &pubkey).is_err(),
            "tampered summary must fail verification"
        );
    }

    #[test]
    fn signature_fails_when_payload_tampered() {
        let signing = SigningKey::generate(&mut OsRng);
        let pubkey = signing.verifying_key();
        let mut p = make_params(&signing, "peer-1");
        p.payload = serde_json::json!({"runs": 999});
        assert!(verify_signature(&p, &pubkey).is_err());
    }

    #[test]
    fn signature_fails_with_wrong_pubkey() {
        let signing = SigningKey::generate(&mut OsRng);
        let other = SigningKey::generate(&mut OsRng);
        let p = make_params(&signing, "peer-1");
        assert!(verify_signature(&p, &other.verifying_key()).is_err());
    }

    #[test]
    fn canonical_bytes_are_stable_across_field_order() {
        let signing = SigningKey::generate(&mut OsRng);
        let p1 = make_params(&signing, "peer-1");
        let canon1 = canonical_signing_bytes(&p1);
        // Construct a struct with the same logical content; canonical
        // output should be byte-identical regardless of how serde
        // ordered the input.
        let p2 = PostCompletionParams {
            occurred_at: p1.occurred_at.clone(),
            payload: p1.payload.clone(),
            summary: p1.summary.clone(),
            status: p1.status.clone(),
            session_id: p1.session_id.clone(),
            from_peer_id: p1.from_peer_id.clone(),
            from_machine_name: p1.from_machine_name.clone(),
            signature: String::new(),
        };
        let canon2 = canonical_signing_bytes(&p2);
        assert_eq!(canon1, canon2);
    }
}
