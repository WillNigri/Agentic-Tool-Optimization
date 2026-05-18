// commands/notifications.rs — Slack / Discord / Telegram / Email
// notification channels.
//
// PR 14 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope (6 commands):
//   - save_notification_channel       — upsert a channel (slack/discord/telegram/email)
//   - list_notification_channels      — list configured channels
//   - delete_notification_channel     — delete one channel
//   - toggle_notification_channel     — flip enabled flag
//   - send_notification               — dispatch to all subscribed channels
//   - test_notification_channel       — fire a one-off test message
//
// Plus the data shapes (NotificationChannel / SendNotificationRequest /
// NotificationResult) and the four provider-specific async send helpers
// (slack/discord/telegram/email). Email goes via lettre (SMTP); the
// three webhook providers go via reqwest POST.

use lettre::{
    message::{header::ContentType, Mailbox},
    transport::smtp::authentication::Credentials,
    Message, SmtpTransport, Transport,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::DbState;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NotificationChannel {
    pub id: String,
    pub provider: String,  // slack, discord, telegram, email
    pub name: String,
    pub config: serde_json::Value,
    pub events: Vec<String>,
    pub enabled: bool,
    pub created_at: String,
    pub last_sent_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SendNotificationRequest {
    pub event_type: String,
    pub title: String,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NotificationResult {
    pub channel_id: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Save a notification channel configuration
#[tauri::command]
pub fn save_notification_channel(
    state: State<DbState>,
    channel: NotificationChannel,
) -> Result<NotificationChannel, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;

    // Ensure table exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS notification_channels (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            name TEXT NOT NULL,
            config TEXT NOT NULL,
            events TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            last_sent_at TEXT
        )",
        [],
    ).map_err(|e| e.to_string())?;

    let config_json = serde_json::to_string(&channel.config).map_err(|e| e.to_string())?;
    let events_json = serde_json::to_string(&channel.events).map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT OR REPLACE INTO notification_channels (id, provider, name, config, events, enabled, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            &channel.id,
            &channel.provider,
            &channel.name,
            &config_json,
            &events_json,
            if channel.enabled { 1 } else { 0 },
            &channel.created_at,
        ],
    ).map_err(|e| e.to_string())?;

    Ok(channel)
}

/// List all notification channels
#[tauri::command]
pub fn list_notification_channels(state: State<DbState>) -> Result<Vec<NotificationChannel>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;

    // Ensure table exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS notification_channels (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            name TEXT NOT NULL,
            config TEXT NOT NULL,
            events TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            last_sent_at TEXT
        )",
        [],
    ).map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, provider, name, config, events, enabled, created_at, last_sent_at
         FROM notification_channels
         ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;

    let channels = stmt
        .query_map([], |row| {
            let config_str: String = row.get(3)?;
            let events_str: String = row.get(4)?;
            Ok(NotificationChannel {
                id: row.get(0)?,
                provider: row.get(1)?,
                name: row.get(2)?,
                config: serde_json::from_str(&config_str).unwrap_or(serde_json::json!({})),
                events: serde_json::from_str(&events_str).unwrap_or(vec![]),
                enabled: row.get::<_, i32>(5)? == 1,
                created_at: row.get(6)?,
                last_sent_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(channels)
}

/// Delete a notification channel
#[tauri::command]
pub fn delete_notification_channel(state: State<DbState>, channel_id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM notification_channels WHERE id = ?1",
        params![&channel_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Toggle notification channel enabled state
#[tauri::command]
pub fn toggle_notification_channel(state: State<DbState>, channel_id: String, enabled: bool) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE notification_channels SET enabled = ?1 WHERE id = ?2",
        params![if enabled { 1 } else { 0 }, &channel_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Send a notification to all enabled channels that match the event type
#[tauri::command]
pub async fn send_notification(
    state: State<'_, DbState>,
    request: SendNotificationRequest,
) -> Result<Vec<NotificationResult>, String> {
    let channels = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;

        let mut stmt = conn.prepare(
            "SELECT id, provider, name, config, events, enabled, created_at, last_sent_at
             FROM notification_channels
             WHERE enabled = 1"
        ).map_err(|e| e.to_string())?;

        let channels: Vec<NotificationChannel> = stmt
            .query_map([], |row| {
                let config_str: String = row.get(3)?;
                let events_str: String = row.get(4)?;
                Ok(NotificationChannel {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    name: row.get(2)?,
                    config: serde_json::from_str(&config_str).unwrap_or(serde_json::json!({})),
                    events: serde_json::from_str(&events_str).unwrap_or(vec![]),
                    enabled: true,
                    created_at: row.get(6)?,
                    last_sent_at: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        channels
    };

    let mut results = Vec::new();

    for channel in channels {
        // Check if channel is subscribed to this event type
        if !channel.events.contains(&request.event_type) {
            continue;
        }

        let result = match channel.provider.as_str() {
            "slack" => send_slack_notification(&channel, &request).await,
            "discord" => send_discord_notification(&channel, &request).await,
            "telegram" => send_telegram_notification(&channel, &request).await,
            "email" => send_email_notification(&channel, &request).await,
            _ => Err(format!("Unknown provider: {}", channel.provider)),
        };

        let notification_result = NotificationResult {
            channel_id: channel.id.clone(),
            success: result.is_ok(),
            error: result.err(),
        };

        // Update last_sent_at if successful
        if notification_result.success {
            let conn = state.0.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE notification_channels SET last_sent_at = datetime('now') WHERE id = ?1",
                params![&channel.id],
            ).ok();
        }

        results.push(notification_result);
    }

    Ok(results)
}

/// Send Slack webhook notification
pub async fn send_slack_notification(
    channel: &NotificationChannel,
    request: &SendNotificationRequest,
) -> Result<(), String> {
    let webhook_url = channel.config.get("webhookUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing webhookUrl in Slack config".to_string())?;

    let payload = serde_json::json!({
        "text": format!("*{}*\n{}", request.title, request.message),
        "blocks": [
            {
                "type": "header",
                "text": { "type": "plain_text", "text": &request.title }
            },
            {
                "type": "section",
                "text": { "type": "mrkdwn", "text": &request.message }
            }
        ]
    });

    let client = reqwest::Client::new();
    let response = client
        .post(webhook_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send Slack notification: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Slack API error {}: {}", status, body));
    }

    Ok(())
}

/// Send Discord webhook notification
pub async fn send_discord_notification(
    channel: &NotificationChannel,
    request: &SendNotificationRequest,
) -> Result<(), String> {
    let webhook_url = channel.config.get("webhookUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing webhookUrl in Discord config".to_string())?;

    let payload = serde_json::json!({
        "embeds": [{
            "title": &request.title,
            "description": &request.message,
            "color": 5814783  // ATO accent color
        }]
    });

    let client = reqwest::Client::new();
    let response = client
        .post(webhook_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send Discord notification: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Discord API error {}: {}", status, body));
    }

    Ok(())
}

/// Send Telegram bot notification
pub async fn send_telegram_notification(
    channel: &NotificationChannel,
    request: &SendNotificationRequest,
) -> Result<(), String> {
    let bot_token = channel.config.get("botToken")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing botToken in Telegram config".to_string())?;

    let chat_id = channel.config.get("chatId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing chatId in Telegram config".to_string())?;

    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
    let text = format!("*{}*\n\n{}", request.title, request.message);

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .form(&[
            ("chat_id", chat_id),
            ("text", &text),
            ("parse_mode", "Markdown"),
        ])
        .send()
        .await
        .map_err(|e| format!("Failed to send Telegram notification: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Telegram API error {}: {}", status, body));
    }

    Ok(())
}

/// Send email notification via SMTP
pub async fn send_email_notification(
    channel: &NotificationChannel,
    request: &SendNotificationRequest,
) -> Result<(), String> {
    // Extract SMTP configuration
    let host = channel.config.get("host")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing SMTP host in Email config".to_string())?;

    let port = channel.config.get("port")
        .map(|v| {
            // Handle both number and string values
            v.as_u64().unwrap_or_else(|| {
                v.as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(587)
            })
        })
        .unwrap_or(587) as u16;

    let username = channel.config.get("authUser")
        .or_else(|| channel.config.get("username"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing SMTP username in Email config".to_string())?;

    let password = channel.config.get("authPass")
        .or_else(|| channel.config.get("password"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing SMTP password in Email config".to_string())?;

    let from_email = channel.config.get("from")
        .and_then(|v| v.as_str())
        .unwrap_or(username);

    let from_name = channel.config.get("fromName")
        .and_then(|v| v.as_str())
        .unwrap_or("ATO Notifications");

    let to_email = channel.config.get("to")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing 'to' address in Email config".to_string())?;

    let use_tls = channel.config.get("useTls")
        .map(|v| {
            // Handle both boolean and string values
            v.as_bool().unwrap_or_else(|| {
                v.as_str().map(|s| s == "true").unwrap_or(true)
            })
        })
        .unwrap_or(true);

    // Build HTML email body
    let html_body = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0a0a0f; color: #e5e5e5; padding: 20px; }}
        .container {{ max-width: 600px; margin: 0 auto; background: #111116; border-radius: 8px; padding: 24px; border: 1px solid #222; }}
        .header {{ color: #00FFB2; font-size: 24px; font-weight: 600; margin-bottom: 16px; }}
        .event-badge {{ display: inline-block; background: #00FFB2; color: #0a0a0f; padding: 4px 12px; border-radius: 4px; font-size: 12px; font-weight: 600; margin-bottom: 16px; }}
        .content {{ color: #b3b3b3; line-height: 1.6; }}
        .footer {{ margin-top: 24px; padding-top: 16px; border-top: 1px solid #222; color: #666; font-size: 12px; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="event-badge">{}</div>
        <div class="header">{}</div>
        <div class="content">{}</div>
        <div class="footer">Sent by ATO (Agentic Tool Optimization)</div>
    </div>
</body>
</html>"#,
        request.event_type.to_uppercase(),
        request.title,
        request.message.replace("\n", "<br>")
    );

    // Parse email addresses
    let from_mailbox: Mailbox = format!("{} <{}>", from_name, from_email)
        .parse()
        .map_err(|e| format!("Invalid 'from' email address: {}", e))?;

    let to_mailbox: Mailbox = to_email
        .parse()
        .map_err(|e| format!("Invalid 'to' email address: {}", e))?;

    // Build the email message
    let email = Message::builder()
        .from(from_mailbox)
        .to(to_mailbox)
        .subject(format!("[ATO] {}", request.title))
        .header(ContentType::TEXT_HTML)
        .body(html_body)
        .map_err(|e| format!("Failed to build email: {}", e))?;

    // Build SMTP transport with credentials
    let creds = Credentials::new(username.to_string(), password.to_string());

    let mailer = if use_tls {
        SmtpTransport::starttls_relay(host)
            .map_err(|e| format!("Failed to create SMTP transport: {}", e))?
            .port(port)
            .credentials(creds)
            .build()
    } else {
        SmtpTransport::builder_dangerous(host)
            .port(port)
            .credentials(creds)
            .build()
    };

    // Send the email
    mailer.send(&email)
        .map_err(|e| format!("Failed to send email: {}", e))?;

    Ok(())
}

/// Test a notification channel configuration
#[tauri::command]
pub async fn test_notification_channel(channel: NotificationChannel) -> Result<NotificationResult, String> {
    let test_request = SendNotificationRequest {
        event_type: "test".to_string(),
        title: "Test Notification".to_string(),
        message: format!("This is a test notification from ATO to verify your {} configuration.", channel.provider),
        data: None,
    };

    let result = match channel.provider.as_str() {
        "slack" => send_slack_notification(&channel, &test_request).await,
        "discord" => send_discord_notification(&channel, &test_request).await,
        "telegram" => send_telegram_notification(&channel, &test_request).await,
        "email" => send_email_notification(&channel, &test_request).await,
        _ => Err(format!("Unknown provider: {}", channel.provider)),
    };

    Ok(NotificationResult {
        channel_id: channel.id,
        success: result.is_ok(),
        error: result.err(),
    })
}
