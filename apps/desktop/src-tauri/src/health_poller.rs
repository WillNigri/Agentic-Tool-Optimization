use rusqlite::Connection;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Background health poller that checks runtime status periodically
pub struct HealthPoller {
    running: Arc<Mutex<bool>>,
    interval_secs: u64,
}

impl HealthPoller {
    pub fn new() -> Self {
        Self {
            running: Arc::new(Mutex::new(false)),
            interval_secs: 30,
        }
    }

    /// Start the background health polling
    pub fn start(&self, app_handle: AppHandle, db_path: String) {
        let running = self.running.clone();
        let interval = self.interval_secs;

        // Set running flag
        {
            let mut r = running.lock().unwrap();
            if *r {
                return; // Already running
            }
            *r = true;
        }

        thread::spawn(move || {
            while *running.lock().unwrap() {
                // Run health checks for all runtimes
                let results = check_all_runtimes();

                // Store results in database and emit to frontend
                if let Ok(conn) = Connection::open(&db_path) {
                    for result in &results {
                        store_health_check(&conn, result);
                    }
                }

                // Emit health update event to frontend
                app_handle.emit("health-update", &results).ok();

                // Sleep for the interval
                thread::sleep(Duration::from_secs(interval));
            }
        });
    }

    /// Stop the background health polling
    pub fn stop(&self) {
        let mut running = self.running.lock().unwrap();
        *running = false;
    }

    pub fn is_running(&self) -> bool {
        *self.running.lock().unwrap()
    }

    pub fn set_interval(&mut self, secs: u64) {
        self.interval_secs = secs;
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckResult {
    pub runtime: String,
    pub status: String,
    pub latency_ms: Option<i64>,
    pub error_message: Option<String>,
    pub checked_at: String,
}

/// Check health of all configured runtimes
fn check_all_runtimes() -> Vec<HealthCheckResult> {
    let runtimes = vec!["claude", "codex", "hermes", "openclaw"];
    let mut results = Vec::new();

    for runtime in runtimes {
        let result = check_runtime_health(runtime);
        results.push(result);
    }

    results
}

/// Check health of a specific runtime
fn check_runtime_health(runtime: &str) -> HealthCheckResult {
    let now = chrono::Utc::now().to_rfc3339();
    let start = Instant::now();

    let (status, error_message) = match runtime {
        "claude" => check_claude_health(),
        "codex" => check_codex_health(),
        "hermes" => check_hermes_health(),
        "openclaw" => check_openclaw_health(),
        _ => ("unknown".to_string(), Some("Unknown runtime".to_string())),
    };

    let latency_ms = if status == "healthy" || status == "degraded" {
        Some(start.elapsed().as_millis() as i64)
    } else {
        None
    };

    HealthCheckResult {
        runtime: runtime.to_string(),
        status,
        latency_ms,
        error_message,
        checked_at: now,
    }
}

/// Check Claude Code CLI health
fn check_claude_health() -> (String, Option<String>) {
    // Try to run 'claude --version' to check if it's available
    match Command::new("claude").arg("--version").output() {
        Ok(output) => {
            if output.status.success() {
                ("healthy".to_string(), None)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                ("degraded".to_string(), Some(stderr))
            }
        }
        Err(e) => {
            // Try npx
            match Command::new("npx")
                .args(["@anthropic/claude-code", "--version"])
                .output()
            {
                Ok(output) if output.status.success() => ("healthy".to_string(), None),
                _ => ("down".to_string(), Some(format!("Claude not found: {}", e))),
            }
        }
    }
}

/// Check Codex CLI health
fn check_codex_health() -> (String, Option<String>) {
    // Try to run 'codex --version' or 'npx codex --version'
    match Command::new("codex").arg("--version").output() {
        Ok(output) => {
            if output.status.success() {
                ("healthy".to_string(), None)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                ("degraded".to_string(), Some(stderr))
            }
        }
        Err(e) => {
            // Try npx
            match Command::new("npx").args(["codex", "--version"]).output() {
                Ok(output) if output.status.success() => ("healthy".to_string(), None),
                _ => ("down".to_string(), Some(format!("Codex not found: {}", e))),
            }
        }
    }
}

/// Check Hermes health (local server)
fn check_hermes_health() -> (String, Option<String>) {
    // Check if hermes is running on default port (8080)
    match std::net::TcpStream::connect_timeout(
        &"127.0.0.1:8080".parse().unwrap(),
        Duration::from_secs(2),
    ) {
        Ok(_) => ("healthy".to_string(), None),
        Err(_) => {
            // Try alternative command
            match Command::new("hermes").arg("--version").output() {
                Ok(output) if output.status.success() => {
                    ("degraded".to_string(), Some("Hermes installed but server not running".to_string()))
                }
                _ => ("down".to_string(), Some("Hermes not available".to_string())),
            }
        }
    }
}

/// Check OpenClaw health (SSH connectivity)
fn check_openclaw_health() -> (String, Option<String>) {
    // Check if OpenClaw gateway is configured
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let config_path = home.join(".openclaw").join("config.json");

    if !config_path.exists() {
        return ("down".to_string(), Some("OpenClaw not configured".to_string()));
    }

    // Read config and check gateway URL
    match std::fs::read_to_string(&config_path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(config) => {
                if config.get("gateway").is_some() {
                    // Would need actual WebSocket connection test here
                    // For now, just check config exists
                    ("healthy".to_string(), None)
                } else {
                    ("degraded".to_string(), Some("Gateway not configured".to_string()))
                }
            }
            Err(e) => ("degraded".to_string(), Some(format!("Invalid config: {}", e))),
        },
        Err(e) => ("down".to_string(), Some(format!("Cannot read config: {}", e))),
    }
}

/// Store a health check result in the database
fn store_health_check(conn: &Connection, result: &HealthCheckResult) {
    let id = uuid::Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO health_checks (id, runtime, status, latency_ms, error_message, checked_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            id,
            result.runtime,
            result.status,
            result.latency_ms,
            result.error_message,
            result.checked_at
        ],
    ).ok();

    // Clean up old records (keep last 7 days)
    conn.execute(
        "DELETE FROM health_checks WHERE checked_at < datetime('now', '-7 days')",
        [],
    ).ok();
}

/// State wrapper for HealthPoller
pub struct HealthPollerState(pub Arc<Mutex<HealthPoller>>);

impl HealthPollerState {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(HealthPoller::new())))
    }
}
