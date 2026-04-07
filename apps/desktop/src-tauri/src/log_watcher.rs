use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Manages file watching for real-time log updates
pub struct LogWatcher {
    watcher: Option<RecommendedWatcher>,
    watching: bool,
}

impl LogWatcher {
    pub fn new() -> Self {
        Self {
            watcher: None,
            watching: false,
        }
    }

    /// Start watching the log file for changes
    pub fn start(&mut self, app_handle: AppHandle) -> Result<(), String> {
        if self.watching {
            return Ok(());
        }

        let log_path = get_log_file_path();

        // Ensure parent directory exists
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        // Create the log file if it doesn't exist
        if !log_path.exists() {
            std::fs::write(&log_path, "").ok();
        }

        let (tx, rx) = channel::<Result<Event, notify::Error>>();

        let mut watcher = RecommendedWatcher::new(
            move |res| {
                tx.send(res).ok();
            },
            Config::default().with_poll_interval(Duration::from_secs(1)),
        )
        .map_err(|e| format!("Failed to create watcher: {}", e))?;

        watcher
            .watch(&log_path, RecursiveMode::NonRecursive)
            .map_err(|e| format!("Failed to watch log file: {}", e))?;

        // Spawn a thread to handle file events
        let app = app_handle.clone();
        std::thread::spawn(move || {
            handle_log_events(rx, app);
        });

        self.watcher = Some(watcher);
        self.watching = true;

        Ok(())
    }

    /// Stop watching the log file
    pub fn stop(&mut self) {
        self.watcher = None;
        self.watching = false;
    }

    pub fn is_watching(&self) -> bool {
        self.watching
    }
}

/// Get the path to the agent logs file
fn get_log_file_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".ato").join("agent-logs.jsonl")
}

/// Handle file events from the watcher
fn handle_log_events(rx: Receiver<Result<Event, notify::Error>>, app: AppHandle) {
    use notify::EventKind;

    let mut last_size: u64 = 0;
    let log_path = get_log_file_path();

    // Get initial file size
    if let Ok(metadata) = std::fs::metadata(&log_path) {
        last_size = metadata.len();
    }

    for res in rx {
        match res {
            Ok(event) => {
                // Only process modify events
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    // Read new lines from the file
                    if let Ok(metadata) = std::fs::metadata(&log_path) {
                        let current_size = metadata.len();

                        if current_size > last_size {
                            // Read only the new content
                            if let Ok(content) = std::fs::read_to_string(&log_path) {
                                // Get the new lines
                                let new_content = if last_size > 0 {
                                    content.split_at(last_size as usize).1
                                } else {
                                    &content
                                };

                                // Parse and emit each new log entry
                                for line in new_content.lines() {
                                    if !line.trim().is_empty() {
                                        if let Ok(log_entry) = serde_json::from_str::<serde_json::Value>(line) {
                                            app.emit("log-entry", log_entry).ok();
                                        }
                                    }
                                }
                            }

                            last_size = current_size;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Log watcher error: {:?}", e);
            }
        }
    }
}

/// State wrapper for LogWatcher
pub struct LogWatcherState(pub Arc<Mutex<LogWatcher>>);

impl LogWatcherState {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(LogWatcher::new())))
    }
}
