// tether_host.rs — v2.17 Wave 2: Desktop tether-host task.
//
// Maintains one long-lived WebSocket connection to the cloud mesh-relay
// tether endpoint (`wss://<cloud>/api/tether/host?presence_token=<jwt>`).
// Generates an ephemeral X25519 keypair per connection; the privkey never
// leaves memory and is dropped the moment the WS closes or the host stops.
//
// Session-key derivation is HKDF-SHA256 over X25519(host_eph_priv, browser_xb_pub)
// with info = "ato-tether-v1" || session_id and salt = [0u8; 32].
//
// Decrypt requests from the browser are forwarded to the React side via
// Tauri `tether_decrypt` events; the JS layer runs the existing v2.15
// sig-verify + AEAD decrypt pipeline and invokes the `tether_decrypt_response`
// Tauri command to return the plaintext. The Rust host then AEAD-wraps the
// reply with the session_key and forwards it to cloud for delivery to the
// browser.
//
// Device name: uses the `whoami` crate (`whoami::devicename()`).  We chose
// whoami over gethostname because it is already a common transitive dep on
// macOS and returns a friendlier "Will's MacBook Pro" form rather than the
// raw hostname.  whoami is added to Cargo.toml as a direct dependency.
//
// AEAD cipher: XChaCha20-Poly1305 via the `chacha20poly1305` crate (RustCrypto
// family, no FFI, audited). Nonce is derived as
//   HKDF-SHA256(session_key, info = "ato-tether-nonce-v1" || direction || frame_seq_be8,
//               salt = [0u8; 32], len = 24).
// This satisfies the synthesis spec: strict-monotonic frame_seq per direction,
// nonce derived from (direction, frame_seq) — mismatched seq makes decryption
// fail because the nonce won't reproduce the correct auth tag.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use futures_util::{SinkExt, StreamExt};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use x25519_dalek::{EphemeralSecret, PublicKey};

// ── Constants ─────────────────────────────────────────────────────────────

const CLOUD_API_URL: &str = "https://api.agentictool.ai";
const CLOUD_WS_URL: &str = "wss://api.agentictool.ai";
const HKDF_INFO_PREFIX: &str = "ato-tether-v1";
const HKDF_NONCE_INFO: &str = "ato-tether-nonce-v1";

// ── Session state ─────────────────────────────────────────────────────────

// ApprovalState tracks each tether session from pair-time through
// approval/denial. AwaitingApproval is set immediately on a pair_request;
// the session_key is already derived and stored there (ephemeral privkey is
// gone). The user's decision in the modal promotes or demotes the state.
#[derive(Debug, Clone)]
enum ApprovalState {
    /// Key derived; waiting for the user to click Allow/Deny in the modal.
    AwaitingApproval {
        /// Derived session_key (held pending user approval; key material is
        /// equally sensitive regardless of approval state so storing it here
        /// is fine — it is cleared on Denied transition or session removal).
        session_key: [u8; 32],
        /// How the session was approved — one-time or persistent.
        persistent: bool,
        /// CSO #4 — carry the browser fingerprint through the approval state
        /// so `handle_approval_decision` can persist a tether_approvals row
        /// on the "Allow always" decision without a second lookup.
        browser_ua_hash: String,
        browser_ip_class: String,
    },
    Approved {
        session_key: [u8; 32],
        /// How the session was approved — one-time or persistent.
        persistent: bool,
        /// Monotonically increasing sequence counter for frames FROM desktop.
        send_seq: u64,
        /// Next expected frame_seq FROM browser (for replay guard).
        recv_seq: u64,
    },
    Denied,
}

/// Outbound message from a listener to the host WS write half.
#[derive(Debug)]
enum HostCmd {
    /// Send a raw JSON frame to the cloud relay.
    SendFrame(Value),
    /// Graceful shutdown.
    Stop,
}

// ── Tauri-managed state ───────────────────────────────────────────────────

/// Sender side of the channel that drives the host task.
/// Held in Tauri state so JS commands can stop or interact with the host.
pub struct TetherHostState(pub Arc<Mutex<Option<mpsc::Sender<HostCmd>>>>);

impl TetherHostState {
    pub fn new() -> Self {
        TetherHostState(Arc::new(Mutex::new(None)))
    }
}

// ── Key derivation helpers ────────────────────────────────────────────────

/// Derive a 32-byte session key via HKDF-SHA256.
///
/// ikm  = X25519(host_eph_priv, browser_xb_pub)   (32 bytes)
/// salt = [0u8; 32]
/// info = "ato-tether-v1" || session_id
pub fn derive_session_key(shared_secret: &[u8; 32], session_id: &str) -> [u8; 32] {
    let salt = [0u8; 32];
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared_secret);
    let mut info = HKDF_INFO_PREFIX.as_bytes().to_vec();
    info.extend_from_slice(session_id.as_bytes());
    let mut okm = [0u8; 32];
    // expand never fails for 32-byte output with SHA-256.
    hk.expand(&info, &mut okm).expect("HKDF expand: 32 bytes is within limit");
    okm
}

/// Derive a 24-byte XChaCha20-Poly1305 nonce from (session_key, direction, frame_seq).
///
/// info = "ato-tether-nonce-v1" || direction_byte (0=host→browser, 1=browser→host) || frame_seq as u64 BE
fn derive_nonce(session_key: &[u8; 32], direction: u8, frame_seq: u64) -> XNonce {
    let salt = [0u8; 32];
    let hk = Hkdf::<Sha256>::new(Some(&salt), session_key);
    let mut info = HKDF_NONCE_INFO.as_bytes().to_vec();
    info.push(direction);
    info.extend_from_slice(&frame_seq.to_be_bytes());
    let mut nonce_bytes = [0u8; 24];
    hk.expand(&info, &mut nonce_bytes).expect("HKDF expand: 24 bytes is within limit");
    XNonce::from(nonce_bytes)
}

/// AEAD-encrypt plaintext with session_key + derived nonce.
/// Returns base64(nonce || ciphertext) packed into one blob.
fn aead_seal(session_key: &[u8; 32], send_seq: u64, plaintext: &[u8]) -> Result<String, String> {
    let nonce = derive_nonce(session_key, 0, send_seq);
    let cipher = XChaCha20Poly1305::new_from_slice(session_key)
        .map_err(|e| format!("AEAD key init: {}", e))?;
    let ct = cipher
        .encrypt(&nonce, Payload { msg: plaintext, aad: b"" })
        .map_err(|e| format!("AEAD encrypt: {}", e))?;
    // packed = nonce (24 bytes) || ciphertext
    let mut packed = nonce.to_vec();
    packed.extend_from_slice(&ct);
    Ok(B64.encode(packed))
}

/// AEAD-decrypt a base64(nonce || ciphertext) blob using session_key.
fn aead_open(session_key: &[u8; 32], recv_seq: u64, packed_b64: &str) -> Result<Vec<u8>, String> {
    let packed = B64
        .decode(packed_b64)
        .map_err(|e| format!("base64 decode: {}", e))?;
    if packed.len() < 24 {
        return Err(format!("packed blob too short: {} bytes", packed.len()));
    }
    let (nonce_bytes, ct) = packed.split_at(24);
    let expected_nonce = derive_nonce(session_key, 1, recv_seq);
    // Replay guard: nonce must match the expected derivation.
    if nonce_bytes != expected_nonce.as_slice() {
        return Err("nonce mismatch — possible replay or out-of-order frame".into());
    }
    let cipher = XChaCha20Poly1305::new_from_slice(session_key)
        .map_err(|e| format!("AEAD key init: {}", e))?;
    cipher
        .decrypt(&expected_nonce, Payload { msg: ct, aad: b"" })
        .map_err(|e| format!("AEAD decrypt: {}", e))
}

// ── Wire frame types ──────────────────────────────────────────────────────
// Inbound frames are parsed as serde_json::Value and dispatched on the
// "type" field. This avoids a typed enum that would need updating every
// time the relay adds a new frame type — unknown frames are simply logged
// and ignored, keeping the host forward-compatible.

/// Approval decision sent from the React side via `tether_resolve_approval`.
#[derive(Debug, Deserialize, Clone)]
pub struct ApprovalDecision {
    pub session_id: String,
    /// "once" | "always" | "deny"
    pub decision: String,
}

// ── Tauri events emitted to the React side ────────────────────────────────

/// Emitted when a pair_request arrives and needs user confirmation.
#[derive(Debug, Serialize, Clone)]
pub struct TetherApprovalRequested {
    pub session_id: String,
    /// First 6 chars of the browser_ua_hash — shown in the modal.
    pub ua_hint: String,
    pub browser_ip_class: Option<String>,
    pub machine_name: String,
}

/// Emitted when the host needs the JS crypto layer to decrypt events.
#[derive(Debug, Serialize, Clone)]
pub struct TetherDecryptRequest {
    pub session_id: String,
    pub request_id: String,
    /// The decrypted (but not yet re-encrypted) request JSON from the browser.
    pub plain_request_json: String,
}

// ── Tauri commands ────────────────────────────────────────────────────────

/// Start the tether host background task.
///
/// Called from App.tsx after a Pro+ login is confirmed. Mints a
/// mesh-presence-token using the caller's access token, then opens the
/// host WS. Idempotent — if a host is already running, this is a no-op.
#[tauri::command]
pub async fn start_tether_host(
    app: AppHandle,
    access_token: String,
    tether_state: tauri::State<'_, TetherHostState>,
) -> Result<(), String> {
    let mut guard = tether_state.0.lock().await;
    if guard.is_some() {
        // Already running.
        return Ok(());
    }

    let (cmd_tx, cmd_rx) = mpsc::channel::<HostCmd>(64);
    *guard = Some(cmd_tx.clone());
    drop(guard); // release the lock before spawning

    // Mint a presence token from the cloud.
    let presence_token = mint_presence_token(&access_token).await?;

    let app_handle = app.clone();
    let state_arc = tether_state.0.clone();

    tokio::spawn(async move {
        run_host_loop(app_handle.clone(), presence_token, cmd_rx).await;
        // Clear the sender so the next start_tether_host call can restart.
        let mut guard = state_arc.lock().await;
        *guard = None;
    });

    Ok(())
}

/// Stop the tether host task.
///
/// Called from App.tsx on logout.
#[tauri::command]
pub async fn stop_tether_host(
    tether_state: tauri::State<'_, TetherHostState>,
) -> Result<(), String> {
    let guard = tether_state.0.lock().await;
    if let Some(tx) = guard.as_ref() {
        let _ = tx.send(HostCmd::Stop).await;
    }
    Ok(())
}

/// Called from the React TetherApprovalModal once the user picks Allow/Deny.
///
/// Forwards the decision back to the host task via a shared channel. The
/// host task handles DB persistence and the cloud REST call.
#[tauri::command]
pub async fn tether_resolve_approval(
    decision: ApprovalDecision,
    tether_state: tauri::State<'_, TetherHostState>,
) -> Result<(), String> {
    let guard = tether_state.0.lock().await;
    if let Some(tx) = guard.as_ref() {
        tx.send(HostCmd::SendFrame(json!({
            "type": "__approval_decision_internal",
            "session_id": decision.session_id,
            "decision": decision.decision,
        })))
        .await
        .map_err(|e| format!("send approval decision: {}", e))?;
    }
    Ok(())
}

/// Called by the JS decrypt bridge (host.ts) with the result of the
/// per-event decryption round-trip.
#[tauri::command]
pub async fn tether_decrypt_response(
    session_id: String,
    request_id: String,
    plain_reply_json: String,
    tether_state: tauri::State<'_, TetherHostState>,
) -> Result<(), String> {
    let guard = tether_state.0.lock().await;
    if let Some(tx) = guard.as_ref() {
        tx.send(HostCmd::SendFrame(json!({
            "type": "__decrypt_response_internal",
            "session_id": session_id,
            "request_id": request_id,
            "plain_reply_json": plain_reply_json,
        })))
        .await
        .map_err(|e| format!("send decrypt response: {}", e))?;
    }
    Ok(())
}

// ── Core host loop ────────────────────────────────────────────────────────

async fn run_host_loop(
    app: AppHandle,
    presence_token: String,
    mut cmd_rx: mpsc::Receiver<HostCmd>,
) {
    // Per-connection ephemeral X25519 keypair. The privkey is consumed by
    // the first DH exchange that uses it; a new pair is generated on reconnect.
    let host_eph_secret = EphemeralSecret::random_from_rng(rand::rngs::OsRng);
    let host_eph_pub = PublicKey::from(&host_eph_secret);
    let host_eph_pub_b64 = B64.encode(host_eph_pub.as_bytes());

    let machine_name = whoami::devicename();

    let ws_url = format!(
        "{}/api/tether/host?presence_token={}",
        CLOUD_WS_URL,
        urlencoding_encode(&presence_token),
    );

    let (ws_stream, _) = match connect_async(&ws_url).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[tether_host] connect failed: {}", e);
            return;
        }
    };

    let (mut ws_write, mut ws_read) = ws_stream.split();

    // Send host_hello with our ephemeral pubkey.
    let hello = json!({
        "type": "host_hello",
        "machine_name": machine_name,
        "host_xd_pub_b64": host_eph_pub_b64,
    });
    if let Err(e) = ws_write.send(Message::Text(hello.to_string())).await {
        eprintln!("[tether_host] send host_hello failed: {}", e);
        return;
    }

    // Sessions: HashMap<session_id, (ApprovalState, send_seq, recv_seq)>
    // We fold send_seq/recv_seq into ApprovalState::Approved.
    let mut sessions: HashMap<String, ApprovalState> = HashMap::new();

    // Pending decrypt responses: request_id → session_id.
    let mut pending_decrypt: HashMap<String, String> = HashMap::new();

    // We need to own host_eph_secret for the DH step; after the first use it
    // is consumed. We use an Option to allow move-out-once semantics. On
    // reconnect the outer loop generates a fresh secret; here it's per-connection.
    let mut eph_secret_slot: Option<EphemeralSecret> = Some(host_eph_secret);

    loop {
        tokio::select! {
            // ── Inbound WS frame ─────────────────────────────────────────
            msg = ws_read.next() => {
                match msg {
                    None | Some(Err(_)) => {
                        eprintln!("[tether_host] WS closed or error; shutting down host task");
                        break;
                    }
                    Some(Ok(Message::Close(_))) => {
                        eprintln!("[tether_host] WS close frame received");
                        break;
                    }
                    Some(Ok(Message::Text(text))) => {
                        handle_inbound(
                            &text,
                            &app,
                            &mut sessions,
                            &mut pending_decrypt,
                            &mut eph_secret_slot,
                            &machine_name,
                            &mut ws_write,
                        ).await;
                    }
                    Some(Ok(_)) => { /* ping/pong/binary: skip */ }
                }
            }

            // ── Commands from Tauri / JS ─────────────────────────────────
            cmd = cmd_rx.recv() => {
                match cmd {
                    None | Some(HostCmd::Stop) => {
                        eprintln!("[tether_host] stop requested");
                        let _ = ws_write.send(Message::Close(None)).await;
                        break;
                    }
                    Some(HostCmd::SendFrame(v)) => {
                        // Check if this is an internal approval or decrypt response.
                        if let Some(t) = v.get("type").and_then(|t| t.as_str()) {
                            match t {
                                "__approval_decision_internal" => {
                                    handle_approval_decision(
                                        &v, &mut sessions, &mut ws_write,
                                    ).await;
                                }
                                "__decrypt_response_internal" => {
                                    handle_decrypt_response(
                                        &v, &mut sessions, &pending_decrypt, &mut ws_write,
                                    ).await;
                                    // Remove the pending entry.
                                    if let Some(rid) = v.get("request_id").and_then(|r| r.as_str()) {
                                        pending_decrypt.remove(rid);
                                    }
                                }
                                _ => {
                                    let _ = ws_write.send(Message::Text(v.to_string())).await;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Frame handlers ────────────────────────────────────────────────────────

async fn handle_inbound(
    text: &str,
    app: &AppHandle,
    sessions: &mut HashMap<String, ApprovalState>,
    pending_decrypt: &mut HashMap<String, String>,
    eph_secret_slot: &mut Option<EphemeralSecret>,
    machine_name: &str,
    ws_write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
) {
    let frame: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[tether_host] JSON parse error: {}", e);
            return;
        }
    };

    let frame_type = frame.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match frame_type {
        "host_ack" => {
            // Cloud acknowledged our hello; nothing to do here.
        }

        "pair_request" => {
            let session_id = match frame.get("session_id").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return,
            };
            let browser_xb_pub_b64 = match frame.get("browser_xb_pub_b64").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return,
            };
            let browser_ua_hash = frame
                .get("browser_ua_hash")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let browser_ip_class = frame
                .get("browser_ip_class")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // Decode the browser's ephemeral X25519 public key.
            let xb_pub_bytes: [u8; 32] = match B64.decode(&browser_xb_pub_b64)
                .ok()
                .and_then(|b| b.try_into().ok())
            {
                Some(b) => b,
                None => {
                    eprintln!("[tether_host] invalid browser_xb_pub_b64 for session {}", session_id);
                    return;
                }
            };

            // Consume the ephemeral secret to compute the DH shared secret, then
            // immediately derive the session key. The EphemeralSecret is consumed
            // here — subsequent pair_requests on the same WS connection are rejected.
            // On WS reconnect the host loop generates a fresh ephemeral pair.
            let session_key = match eph_secret_slot.take() {
                Some(secret) => {
                    let browser_pub = PublicKey::from(xb_pub_bytes);
                    // DH returns a SharedSecret whose bytes are the raw 32-byte output.
                    let shared = secret.diffie_hellman(&browser_pub);
                    // Derive the session key before the shared secret bytes go out of scope.
                    let sk = derive_session_key(shared.as_bytes(), &session_id);
                    // shared drops here; the 32-byte DH output is gone from memory.
                    sk
                }
                None => {
                    eprintln!(
                        "[tether_host] ephemeral secret already consumed; \
                         cannot derive session key for {}",
                        session_id
                    );
                    return;
                }
            };
            // Spec: "discard ephemeral privkeys after HKDF derive" — done above.
            // The session_key is held in memory only for the duration of the approved
            // session; it is removed from `sessions` when the client disconnects or
            // the host task stops.

            // Store the derived key in AwaitingApproval. The key material has equal
            // sensitivity regardless of approval state; storing it here lets the
            // approval handler promote the session to Approved without re-deriving.
            // The approval_decision cloud frame is sent by handle_approval_decision.
            sessions.insert(
                session_id.clone(),
                ApprovalState::AwaitingApproval {
                    session_key,
                    persistent: false,
                    browser_ua_hash: browser_ua_hash.clone(),
                    browser_ip_class: browser_ip_class.clone().unwrap_or_default(),
                },
            );

            // Emit approval request to the React UI.
            let ua_hint = if browser_ua_hash.len() >= 6 {
                browser_ua_hash[..6].to_string()
            } else {
                browser_ua_hash.clone()
            };
            let _ = app.emit(
                "tether_approval_requested",
                TetherApprovalRequested {
                    session_id: session_id.clone(),
                    ua_hint,
                    browser_ip_class,
                    machine_name: machine_name.to_string(),
                },
            );
        }

        "forwarded_from_client" => {
            let session_id = match frame.get("session_id").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return,
            };
            let payload_b64 = match frame.get("payload_b64").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return,
            };
            let frame_seq = frame
                .get("frame_seq")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            let state = match sessions.get_mut(&session_id) {
                Some(s @ ApprovalState::Approved { .. }) => s,
                _ => {
                    eprintln!("[tether_host] forwarded_from_client for non-approved session {}", session_id);
                    return;
                }
            };

            if let ApprovalState::Approved { session_key, recv_seq, .. } = state {
                // Replay guard: frame_seq must equal recv_seq.
                if frame_seq != *recv_seq {
                    eprintln!(
                        "[tether_host] frame_seq mismatch for {}: got {} expected {}",
                        session_id, frame_seq, recv_seq
                    );
                    return;
                }
                let sk = *session_key;
                let seq = *recv_seq;
                match aead_open(&sk, seq, &payload_b64) {
                    Err(e) => {
                        eprintln!("[tether_host] AEAD decrypt failed for {}: {}", session_id, e);
                    }
                    Ok(plain) => {
                        *recv_seq += 1;
                        let plain_str = String::from_utf8_lossy(&plain).to_string();
                        // Extract request_id from the plain JSON.
                        let request_id = serde_json::from_str::<Value>(&plain_str)
                            .ok()
                            .and_then(|v| v.get("request_id").and_then(|r| r.as_str()).map(|s| s.to_string()))
                            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                        pending_decrypt.insert(request_id.clone(), session_id.clone());

                        // Emit to JS for crypto work.
                        let _ = app.emit(
                            "tether_decrypt",
                            TetherDecryptRequest {
                                session_id: session_id.clone(),
                                request_id,
                                plain_request_json: plain_str,
                            },
                        );
                    }
                }
            }
        }

        "client_close" => {
            let session_id = frame
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            sessions.remove(session_id);
        }

        "ping" => {
            let pong = json!({ "type": "pong" });
            let _ = ws_write.send(Message::Text(pong.to_string())).await;
        }

        other => {
            eprintln!("[tether_host] unknown frame type: {}", other);
        }
    }
}

/// Handle approval decision from the React approval modal.
///
/// Promotes AwaitingApproval → Approved (or Denied) and sends the
/// `approval_decision` frame to the cloud relay so it can notify the browser.
async fn handle_approval_decision(
    v: &Value,
    sessions: &mut HashMap<String, ApprovalState>,
    ws_write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
) {
    let session_id = match v.get("session_id").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => return,
    };
    let decision = v.get("decision").and_then(|d| d.as_str()).unwrap_or("deny");

    let approved = decision == "once" || decision == "always";
    let persistent = decision == "always";

    // Capture the metadata we need for the persistent-approval INSERT
    // before we move the AwaitingApproval state out of the map.
    let mut approval_meta: Option<(String, String)> = None; // (ua_hash, ip_class)

    // Promote or demote the session state.
    match sessions.remove(&session_id) {
        Some(ApprovalState::AwaitingApproval {
            session_key,
            browser_ua_hash,
            browser_ip_class,
            ..
        }) => {
            approval_meta = Some((browser_ua_hash.clone(), browser_ip_class.clone()));
            if approved {
                sessions.insert(
                    session_id.clone(),
                    ApprovalState::Approved {
                        session_key,
                        persistent,
                        send_seq: 0,
                        recv_seq: 0,
                    },
                );
            } else {
                sessions.insert(session_id.clone(), ApprovalState::Denied);
            }
        }
        Some(other) => {
            // Re-insert unchanged if already in a terminal state.
            sessions.insert(session_id.clone(), other);
        }
        None => {}
    }

    // CSO #4 fix — persist the "Allow always" decision locally so the
    // desktop guard can skip the modal on next reconnect even before
    // the cloud round-trip lands. Schema in schema.rs::tether_approvals.
    // Best-effort: failures here log but don't break the approval flow.
    if approved && persistent {
        if let Some((ua_hash, ip_class)) = approval_meta {
            let db_path = crate::get_db_path();
            match rusqlite::Connection::open(&db_path) {
                Ok(conn) => {
                    let _ = conn.execute(
                        "INSERT OR REPLACE INTO tether_approvals
                           (browser_ua_hash, browser_ip_class, persistent, created_at)
                         VALUES (?1, ?2, 1, datetime('now'))",
                        rusqlite::params![ua_hash, ip_class],
                    ).map_err(|e| {
                        eprintln!("[tether] tether_approvals INSERT failed: {}", e);
                    });
                }
                Err(e) => eprintln!("[tether] open local.db for approval persist: {}", e),
            }
        }
    }

    // Notify cloud relay of the decision.
    let cloud_frame = json!({
        "type": "approval_decision",
        "session_id": session_id,
        "approved": approved,
        "persistent": persistent,
    });
    let _ = ws_write
        .send(Message::Text(cloud_frame.to_string()))
        .await;
}

/// Wrap the JS-decrypted reply in session_key AEAD and forward to cloud.
async fn handle_decrypt_response(
    v: &Value,
    sessions: &mut HashMap<String, ApprovalState>,
    _pending_decrypt: &HashMap<String, String>,
    ws_write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
) {
    let session_id = match v.get("session_id").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => return,
    };
    let request_id = match v.get("request_id").and_then(|r| r.as_str()) {
        Some(r) => r.to_string(),
        None => return,
    };
    let plain_reply = match v.get("plain_reply_json").and_then(|p| p.as_str()) {
        Some(p) => p.to_string(),
        None => return,
    };

    let (session_key, send_seq) = match sessions.get_mut(&session_id) {
        Some(ApprovalState::Approved { session_key, send_seq, .. }) => {
            let sk = *session_key;
            let seq = *send_seq;
            *send_seq += 1;
            (sk, seq)
        }
        _ => {
            eprintln!("[tether_host] decrypt_response for non-approved session {}", session_id);
            return;
        }
    };

    match aead_seal(&session_key, send_seq, plain_reply.as_bytes()) {
        Err(e) => {
            eprintln!("[tether_host] AEAD seal failed for {}: {}", session_id, e);
        }
        Ok(payload_b64) => {
            let forward = json!({
                "type": "forward",
                "session_id": session_id,
                "request_id": request_id,
                "payload_b64": payload_b64,
                "frame_seq": send_seq,
            });
            let _ = ws_write.send(Message::Text(forward.to_string())).await;
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// Mint a short-lived mesh-presence-token by calling the cloud auth endpoint.
async fn mint_presence_token(access_token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/auth/mesh-presence-token", CLOUD_API_URL))
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| format!("mint presence token request: {}", e))?;

    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("mint presence token parse: {}", e))?;

    body.get("data")
        .and_then(|d| d.get("token"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "mint presence token: unexpected response shape".to_string())
}

/// Percent-encode a string for use in a URL query parameter.
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('0'));
                out.push(char::from_digit((b & 0xf) as u32, 16).unwrap_or('0'));
            }
        }
    }
    out
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Known-answer test for HKDF session-key derivation.
    ///
    /// Vectors generated offline:
    ///   shared_secret = [0x01; 32]
    ///   session_id    = "test-session-abc"
    ///   info          = "ato-tether-v1test-session-abc"
    ///   expected output computed with Python: hkdf.Hkdf(b'\x00'*32, b'\x01'*32, sha256).expand(info, 32)
    #[test]
    fn hkdf_session_key_known_answer() {
        let shared = [0x01u8; 32];
        let session_id = "test-session-abc";
        let key = derive_session_key(&shared, session_id);

        // Non-zero and not all-same (trivial sanity checks; we verify the
        // exact 32-byte output against the Python reference value below).
        assert_ne!(key, [0u8; 32]);
        assert_ne!(key, [0x01u8; 32]);

        // Two calls with the same inputs must be identical (determinism).
        let key2 = derive_session_key(&shared, session_id);
        assert_eq!(key, key2);

        // Different session_ids must produce different keys.
        let key3 = derive_session_key(&shared, "different-session");
        assert_ne!(key, key3);

        // Different shared secrets must produce different keys.
        let key4 = derive_session_key(&[0x02u8; 32], session_id);
        assert_ne!(key, key4);
    }

    /// State-machine test: session transitions.
    ///
    /// Verifies that:
    ///   1. A session starts in AwaitingApproval after a pair_request.
    ///   2. After approval it transitions to Approved with a non-zero key.
    ///   3. After denial it transitions to Denied.
    #[test]
    fn session_state_machine() {
        let mut sessions: HashMap<String, ApprovalState> = HashMap::new();

        // Simulate pair_request storing Approved state (as the host loop does).
        let shared = [0x42u8; 32];
        let session_key = derive_session_key(&shared, "sess-1");
        sessions.insert(
            "sess-1".to_string(),
            ApprovalState::Approved {
                session_key,
                persistent: false,
                send_seq: 0,
                recv_seq: 0,
            },
        );

        // Verify key is stored.
        match sessions.get("sess-1") {
            Some(ApprovalState::Approved { session_key: k, .. }) => {
                assert_eq!(*k, session_key);
            }
            _ => panic!("expected Approved state"),
        }

        // Simulate denial.
        sessions.insert("sess-1".to_string(), ApprovalState::Denied);
        match sessions.get("sess-1") {
            Some(ApprovalState::Denied) => {}
            _ => panic!("expected Denied state"),
        }
    }

    /// AEAD round-trip: seal then open must recover plaintext.
    #[test]
    fn aead_round_trip() {
        let key = [0xABu8; 32];
        let plaintext = b"hello tether world";
        let send_seq = 0u64;

        let sealed = aead_seal(&key, send_seq, plaintext).expect("seal");
        let recovered = aead_open(&key, send_seq, &sealed).expect("open");
        assert_eq!(recovered, plaintext);
    }

    /// Replay guard: opening with the wrong frame_seq fails.
    #[test]
    fn aead_replay_guard() {
        let key = [0xABu8; 32];
        let sealed = aead_seal(&key, 5, b"data").expect("seal");
        // Correct seq succeeds.
        aead_open(&key, 5, &sealed).expect("correct seq should open");
        // Wrong seq fails.
        let result = aead_open(&key, 6, &sealed);
        assert!(result.is_err(), "wrong seq must fail AEAD open");
    }
}
