/**
 * Telemetry Module
 *
 * Provides opt-in analytics tracking for understanding usage patterns.
 * All data is anonymized and no personal information is collected.
 */

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

/// Telemetry event for tracking user interactions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryEvent {
    pub event_type: String,
    pub properties: HashMap<String, serde_json::Value>,
    pub timestamp: String,
    pub session_id: String,
    pub device_id: String,
}

/// Telemetry settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySettings {
    pub enabled: bool,
    pub device_id: String,
    pub endpoint: Option<String>,
}

/// Telemetry state for managing tracking
pub struct TelemetryState {
    pub settings: Mutex<TelemetrySettings>,
    pub session_id: String,
    pub client: Client,
    pub events_queue: Mutex<Vec<TelemetryEvent>>,
}

impl TelemetryState {
    pub fn new() -> Self {
        // Generate or load device ID (anonymous identifier)
        let device_id = get_or_create_device_id();

        Self {
            settings: Mutex::new(TelemetrySettings {
                enabled: false, // Opt-in by default
                device_id: device_id.clone(),
                endpoint: None,
            }),
            session_id: Uuid::new_v4().to_string(),
            client: Client::new(),
            events_queue: Mutex::new(Vec::new()),
        }
    }
}

/// Get or create a persistent anonymous device ID
fn get_or_create_device_id() -> String {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ato");

    let device_id_path = config_dir.join("device_id");

    if let Ok(id) = std::fs::read_to_string(&device_id_path) {
        return id.trim().to_string();
    }

    // Create new device ID
    let new_id = Uuid::new_v4().to_string();

    // Try to persist it
    if let Err(_) = std::fs::create_dir_all(&config_dir) {
        return new_id;
    }

    let _ = std::fs::write(&device_id_path, &new_id);
    new_id
}

/// Standard event types for consistency
pub mod events {
    pub const APP_LAUNCHED: &str = "app_launched";
    pub const APP_CLOSED: &str = "app_closed";
    pub const SIGNUP_STARTED: &str = "signup_started";
    pub const SIGNUP_COMPLETED: &str = "signup_completed";
    pub const LOGIN_COMPLETED: &str = "login_completed";
    pub const SKILL_CREATED: &str = "skill_created";
    pub const SKILL_INSTALLED: &str = "skill_installed";
    pub const SKILL_EXECUTED: &str = "skill_executed";
    pub const AUTOMATION_CREATED: &str = "automation_created";
    pub const AUTOMATION_EXECUTED: &str = "automation_executed";
    pub const RUNTIME_CONNECTED: &str = "runtime_connected";
    pub const NOTIFICATION_SENT: &str = "notification_sent";
    pub const SETTINGS_CHANGED: &str = "settings_changed";
    pub const FEATURE_USED: &str = "feature_used";
    pub const ERROR_OCCURRED: &str = "error_occurred";
}
