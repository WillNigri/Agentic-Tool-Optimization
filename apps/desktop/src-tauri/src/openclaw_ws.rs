use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use std::time::Duration;

/// Execute a single JSON-RPC 2.0 call over WebSocket.
/// Opens connection, sends request, reads one response, closes.
pub async fn rpc_call(ws_url: &str, token: &str, method: &str, params: Value) -> Result<Value, String> {
    // Normalize URL: convert http(s):// to ws(s)://
    let url = if ws_url.starts_with("http://") {
        ws_url.replacen("http://", "ws://", 1)
    } else if ws_url.starts_with("https://") {
        ws_url.replacen("https://", "wss://", 1)
    } else {
        ws_url.to_string()
    };

    // Build HTTP request with auth header for the WebSocket upgrade
    let request = tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Host", extract_host(&url).unwrap_or_else(|| "localhost".to_string()))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", tokio_tungstenite::tungstenite::handshake::client::generate_key())
        .body(())
        .map_err(|e| format!("Failed to build WebSocket request: {}", e))?;

    // Connect with 5s timeout
    let ws_stream = tokio::time::timeout(
        Duration::from_secs(5),
        connect_async(request),
    )
    .await
    .map_err(|_| "Connection timed out after 5 seconds".to_string())?
    .map_err(|e| format!("WebSocket connection failed: {}", e))?;

    let (mut write, mut read) = ws_stream.0.split();

    // Build JSON-RPC 2.0 request
    let rpc_request = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });

    // Send the request
    write
        .send(Message::Text(rpc_request.to_string()))
        .await
        .map_err(|e| format!("Failed to send WebSocket message: {}", e))?;

    // Read one response with 10s timeout
    let response = tokio::time::timeout(Duration::from_secs(10), read.next())
        .await
        .map_err(|_| "Response timed out after 10 seconds".to_string())?
        .ok_or_else(|| "WebSocket stream closed without response".to_string())?
        .map_err(|e| format!("Failed to read WebSocket response: {}", e))?;

    // Close the connection gracefully
    let _ = write.send(Message::Close(None)).await;

    // Parse the response
    match response {
        Message::Text(text) => {
            let parsed: Value = serde_json::from_str(&text)
                .map_err(|e| format!("Invalid JSON response: {}", e))?;

            // Check for JSON-RPC error
            if let Some(error) = parsed.get("error") {
                let msg = error.get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown RPC error");
                let code = error.get("code")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(-1);
                return Err(format!("RPC error {}: {}", code, msg));
            }

            // Extract result
            Ok(parsed.get("result").cloned().unwrap_or(json!(null)))
        }
        Message::Close(_) => Err("Server closed connection before responding".to_string()),
        _ => Err("Unexpected WebSocket message type".to_string()),
    }
}

/// Test if the gateway is reachable
pub async fn test_connection(ws_url: &str, token: &str) -> Result<Value, String> {
    rpc_call(ws_url, token, "status", json!({})).await
}

/// Extract host from a WebSocket URL for the Host header
fn extract_host(url: &str) -> Option<String> {
    // Strip scheme
    let without_scheme = url
        .strip_prefix("ws://")
        .or_else(|| url.strip_prefix("wss://"))
        .unwrap_or(url);

    // Take everything before the first /
    let host_port = without_scheme.split('/').next()?;
    Some(host_port.to_string())
}
