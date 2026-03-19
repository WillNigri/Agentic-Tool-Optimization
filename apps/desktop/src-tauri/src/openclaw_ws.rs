use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use std::time::Duration;
use std::sync::atomic::{AtomicU64, Ordering};

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Execute an RPC call to the OpenClaw gateway.
/// Protocol: connect via WebSocket, send a "connect" handshake with token,
/// wait for connect response, then send a "request" message and read the response.
pub async fn rpc_call(ws_url: &str, token: &str, method: &str, params: Value) -> Result<Value, String> {
    let url = normalize_url(ws_url);

    // Connect with 5s timeout (plain WebSocket, no auth headers needed)
    let ws_stream = tokio::time::timeout(
        Duration::from_secs(5),
        connect_async(&url),
    )
    .await
    .map_err(|_| "Connection timed out after 5 seconds".to_string())?
    .map_err(|e| format!("WebSocket connection failed: {}", e))?;

    let (mut write, mut read) = ws_stream.0.split();

    // Step 1: Send connect handshake with token
    let connect_msg = json!({
        "type": "connect",
        "token": token,
        "scopes": ["operator.admin"]
    });
    write
        .send(Message::Text(connect_msg.to_string()))
        .await
        .map_err(|e| format!("Failed to send connect: {}", e))?;

    // Step 2: Wait for connect response (with timeout)
    let connect_resp = wait_for_message(&mut read, 5).await?;
    let connect_data: Value = serde_json::from_str(&connect_resp)
        .map_err(|e| format!("Invalid connect response: {}", e))?;

    // Check if connect was successful
    if let Some(err) = connect_data.get("error") {
        let msg = err.get("message").and_then(|v| v.as_str())
            .or_else(|| err.as_str())
            .unwrap_or("Auth failed");
        return Err(format!("Gateway auth failed: {}", msg));
    }

    // Step 3: Send the RPC request
    let req_id = REQUEST_ID.fetch_add(1, Ordering::SeqCst);
    let request_msg = json!({
        "type": "request",
        "id": req_id,
        "method": method,
        "params": params
    });
    write
        .send(Message::Text(request_msg.to_string()))
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    // Step 4: Read responses until we get our reply (skip events/heartbeats)
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err("Response timed out after 15 seconds".to_string());
        }

        let msg_text = match tokio::time::timeout(remaining, read.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => text,
            Ok(Some(Ok(Message::Close(_)))) => return Err("Gateway closed connection".to_string()),
            Ok(Some(Ok(_))) => continue, // Skip binary/ping/pong
            Ok(Some(Err(e))) => return Err(format!("WebSocket error: {}", e)),
            Ok(None) => return Err("Connection closed".to_string()),
            Err(_) => return Err("Response timed out".to_string()),
        };

        let parsed: Value = match serde_json::from_str(&msg_text) {
            Ok(v) => v,
            Err(_) => continue, // Skip unparseable messages
        };

        // Check if this is a response to our request
        let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let msg_id = parsed.get("id").and_then(|v| v.as_u64());

        if msg_type == "response" && msg_id == Some(req_id) {
            // Close gracefully
            let _ = write.send(Message::Close(None)).await;

            // Check for error
            if let Some(error) = parsed.get("error") {
                let err_msg = error.get("message").and_then(|v| v.as_str())
                    .unwrap_or("Unknown error");
                return Err(format!("RPC error: {}", err_msg));
            }

            return Ok(parsed.get("result").cloned().unwrap_or(parsed));
        }

        // Also handle "connected" type as success for the connect step
        if msg_type == "connected" || msg_type == "connect" {
            continue; // We already got connect response, keep waiting for our RPC response
        }
    }
}

/// Test if the gateway is reachable and authenticated
pub async fn test_connection(ws_url: &str, token: &str) -> Result<Value, String> {
    rpc_call(ws_url, token, "status", json!({})).await
}

/// Read one text message from the WebSocket with timeout
async fn wait_for_message(
    read: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    timeout_secs: u64,
) -> Result<String, String> {
    let deadline = Duration::from_secs(timeout_secs);
    loop {
        match tokio::time::timeout(deadline, read.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => return Ok(text),
            Ok(Some(Ok(Message::Close(_)))) => return Err("Connection closed by server".to_string()),
            Ok(Some(Ok(_))) => continue, // Skip binary/ping/pong
            Ok(Some(Err(e))) => return Err(format!("WebSocket error: {}", e)),
            Ok(None) => return Err("Connection closed".to_string()),
            Err(_) => return Err(format!("No response within {}s", timeout_secs)),
        }
    }
}

/// Normalize URL: convert http(s):// to ws(s)://
fn normalize_url(ws_url: &str) -> String {
    if ws_url.starts_with("http://") {
        ws_url.replacen("http://", "ws://", 1)
    } else if ws_url.starts_with("https://") {
        ws_url.replacen("https://", "wss://", 1)
    } else if !ws_url.starts_with("ws://") && !ws_url.starts_with("wss://") {
        format!("ws://{}", ws_url)
    } else {
        ws_url.to_string()
    }
}
