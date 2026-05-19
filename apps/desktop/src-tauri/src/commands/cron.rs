// commands/cron.rs — Local cron jobs + cross-platform OS scheduler
// (launchd / systemd / schtasks).
//
// PR 26 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope (10 commands + helpers + tests):
//
//   Local cron CRUD (5):
//     - list_cron_jobs       — ATO jobs + Claude Code native + Claude Desktop
//     - save_cron_job        — upsert against ~/.ato/cron-jobs.json
//     - delete_cron_job
//     - get_cron_history     — filter ~/.ato/cron-history.json by jobId
//     - trigger_cron_job     — manual fire (calls dispatch_to_group /
//                              prompt_agent_with_context via super::*)
//
//   OS scheduler façade (5):
//     - cron_os_scheduler_supported
//     - cron_os_scheduler_kind
//     - register_cron_os_scheduler        — launchd plist / systemd timer /
//                                           schtasks XML, per-platform
//     - unregister_cron_os_scheduler
//     - is_cron_os_scheduler_registered
//
//   Plus path helpers (cron_jobs_path, cron_history_path), the
//   CalInterval struct, all per-OS builders (launchd plist, systemd
//   unit pair, schtasks XML), and the cron_launchd_tests test module.
//
// Deferred to a later PR:
//   - OpenClaw cron commands (openclaw_list/cron_status/add/edit/delete/
//     run/toggle_cron_job) — they call super::openclaw_ssh_command but
//     travel with the rest of the openclaw surface to keep that domain
//     contiguous.
//   - run_cron_headless + dispatch_cron_headless / headless_dispatch_*
//     — depend on agent_hooks, group dispatch, variable resolution
//     (resolve_agent_variables, load_agent_hooks, route_prompt_to_child,
//     load_group_members, etc.). They travel with PR 28 (agents.rs)
//     where those helpers live.

use rusqlite::params;
use std::fs;
use std::path::PathBuf;
use tauri::State;

use crate::{home_dir, DbState};

// ── Cron Job Persistence ─────────────────────────────────────────────────

pub fn cron_jobs_path() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    fs::create_dir_all(&path).ok();
    path.push("cron-jobs.json");
    path
}

pub fn cron_history_path() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    fs::create_dir_all(&path).ok();
    path.push("cron-history.json");
    path
}

#[tauri::command]
pub fn list_cron_jobs() -> Result<Vec<serde_json::Value>, String> {
    let mut all_jobs: Vec<serde_json::Value> = Vec::new();

    // 1. ATO-created cron jobs
    let path = cron_jobs_path();
    if path.exists() {
        if let Some(content) = super::read_file_lossy(&path) {
            if let Ok(jobs) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                all_jobs.extend(jobs);
            }
        }
    }

    // 2. Claude Code native scheduled tasks (from ~/.claude/claudecron/tasks.db)
    let claude_cron_db = super::claude_home().join("claudecron").join("tasks.db");
    if claude_cron_db.exists() {
        if let Ok(conn) = rusqlite::Connection::open_with_flags(
            &claude_cron_db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) {
            // Try to read tasks from Claude's schema
            let query_result = conn.prepare(
                "SELECT id, name, schedule, prompt, enabled, created_at, last_run_at FROM tasks",
            );
            if let Ok(mut stmt) = query_result {
                let tasks = stmt.query_map([], |row| {
                    let id: String = row.get(0)?;
                    let name: String = row.get(1)?;
                    let schedule: String = row.get(2)?;
                    let prompt: String = row.get(3)?;
                    let enabled: bool = row.get(4)?;
                    let created_at: String = row.get(5)?;
                    let last_run_at: Option<String> = row.get(6)?;

                    Ok(serde_json::json!({
                        "id": format!("claude-native-{}", id),
                        "name": name,
                        "description": format!("Claude Code scheduled task"),
                        "schedule": schedule,
                        "runtime": "claude",
                        "prompt": prompt,
                        "enabled": enabled,
                        "status": if enabled { "healthy" } else { "paused" },
                        "source": "claude-code",
                        "createdAt": created_at,
                        "updatedAt": created_at,
                        "lastRunAt": last_run_at,
                    }))
                });

                if let Ok(rows) = tasks {
                    for task in rows.flatten() {
                        all_jobs.push(task);
                    }
                }
            }
        }
    }

    // 3. Claude Desktop Cowork scheduled tasks
    // macOS: ~/Library/Application Support/Claude/
    let claude_desktop_dir = home_dir()
        .join("Library")
        .join("Application Support")
        .join("Claude");
    if claude_desktop_dir.exists() {
        // Look for any task/schedule databases
        for db_name in ["tasks.db", "scheduled_tasks.db", "cowork.db"] {
            let db_path = claude_desktop_dir.join(db_name);
            if db_path.exists() {
                if let Ok(conn) = rusqlite::Connection::open_with_flags(
                    &db_path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                ) {
                    // Try common table names
                    for table in ["tasks", "scheduled_tasks", "dispatches"] {
                        let query = format!("SELECT * FROM {} LIMIT 50", table);
                        if let Ok(stmt) = conn.prepare(&query) {
                            let col_names: Vec<String> = (0..stmt.column_count())
                                .map(|i| stmt.column_name(i).unwrap_or("unknown").to_string())
                                .collect();
                            drop(stmt);

                            if let Ok(mut stmt2) = conn.prepare(&query) {
                                if let Ok(rows) = stmt2.query_map([], |row| {
                                    let mut obj = serde_json::Map::new();
                                    for (i, col_name) in col_names.iter().enumerate() {
                                        let val: String =
                                            row.get::<_, String>(i).unwrap_or_default();
                                        obj.insert(
                                            col_name.clone(),
                                            serde_json::Value::String(val),
                                        );
                                    }
                                    obj.insert(
                                        "source".to_string(),
                                        serde_json::Value::String("claude-desktop".to_string()),
                                    );
                                    obj.insert(
                                        "runtime".to_string(),
                                        serde_json::Value::String("claude".to_string()),
                                    );
                                    Ok(serde_json::Value::Object(obj))
                                }) {
                                    for task in rows.flatten() {
                                        all_jobs.push(task);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(all_jobs)
}

#[tauri::command]
pub fn save_cron_job(job: String) -> Result<(), String> {
    let parsed: serde_json::Value =
        serde_json::from_str(&job).map_err(|e| format!("Invalid cron job JSON: {}", e))?;
    let id = parsed
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Cron job must have an id".to_string())?;

    let path = cron_jobs_path();
    let mut jobs: Vec<serde_json::Value> = if path.exists() {
        let content = super::read_file_lossy(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Update or insert
    if let Some(idx) = jobs
        .iter()
        .position(|j| j.get("id").and_then(|v| v.as_str()) == Some(id))
    {
        jobs[idx] = parsed;
    } else {
        jobs.push(parsed);
    }

    let serialized = serde_json::to_string_pretty(&jobs)
        .map_err(|e| format!("Failed to serialize cron jobs: {}", e))?;
    fs::write(&path, serialized).map_err(|e| format!("Failed to write cron jobs: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn delete_cron_job(id: String) -> Result<(), String> {
    let path = cron_jobs_path();
    if !path.exists() {
        return Ok(());
    }

    let content = super::read_file_lossy(&path).unwrap_or_default();
    let mut jobs: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap_or_default();
    jobs.retain(|j| j.get("id").and_then(|v| v.as_str()) != Some(&id));

    let serialized = serde_json::to_string_pretty(&jobs)
        .map_err(|e| format!("Failed to serialize cron jobs: {}", e))?;
    fs::write(&path, serialized).map_err(|e| format!("Failed to write cron jobs: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn get_cron_history(job_id: String) -> Result<Vec<serde_json::Value>, String> {
    let path = cron_history_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = super::read_file_lossy(&path).unwrap_or_default();
    let all: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap_or_default();
    Ok(all
        .into_iter()
        .filter(|e| e.get("jobId").and_then(|v| v.as_str()) == Some(&job_id))
        .collect())
}

#[tauri::command]
pub async fn trigger_cron_job(db: State<'_, DbState>, id: String) -> Result<String, String> {
    // Read the job from disk.
    let path = cron_jobs_path();
    if !path.exists() {
        return Err("No cron jobs configured".to_string());
    }
    let content = super::read_file_lossy(&path).unwrap_or_default();
    let jobs: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap_or_default();
    let job = jobs
        .iter()
        .find(|j| j.get("id").and_then(|v| v.as_str()) == Some(&id))
        .ok_or_else(|| format!("Cron job not found: {}", id))?;

    let runtime = job
        .get("runtime")
        .and_then(|v| v.as_str())
        .unwrap_or("claude")
        .to_string();
    let prompt = job
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let config = job.get("runtimeConfig").map(|v| v.to_string());
    let agent_slug = job
        .get("agentSlug")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let group_slug = job
        .get("groupSlug")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Preferred dispatch order: group → agent → raw runtime+prompt.
    if let Some(slug) = group_slug {
        // Sequential pipelines & routed groups both go through dispatch_to_group;
        // it returns a stitched transcript suitable as a single string result.
        let result = super::dispatch_to_group(db, slug, prompt, config, None).await?;
        return Ok(result.response);
    }

    if let Some(slug) = agent_slug {
        // Look up the agent by slug → run via prompt_agent_with_context so
        // variables / hooks / role-models / memory policy all fire.
        let agent_id_runtime: Option<(String, String)> = {
            let conn = db.0.lock().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT id, runtime FROM agents WHERE slug = ?1",
                params![slug],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .ok()
        };
        match agent_id_runtime {
            Some((agent_id, agent_runtime)) => {
                // v2.1.0+ — prompt_agent_with_context now returns
                // DispatchResult{response, run_id}. Internal Rust callers
                // (cron, group dispatch) only need the response. The
                // run_id is consumed by frontend wrappers; here we just
                // unwrap and discard it.
                return super::prompt_agent_with_context(
                    db,
                    agent_id,
                    agent_runtime,
                    prompt,
                    config,
                    None,
                )
                .await
                .map(|r| r.response);
            }
            None => {
                return Err(format!(
                    "Cron job references agent '{}' which doesn't exist anymore",
                    slug
                ));
            }
        }
    }

    // Fallback: raw dispatch (legacy / advanced). No agent context;
    // registers anonymously in the Live runs panel.
    super::prompt_agent(runtime, prompt, config, None, None).await
}

// ── Cron → launchd (macOS) ───────────────────────────────────────────────
//
// Translate the user's cron expression into one or more
// StartCalendarInterval entries that launchd understands. launchd doesn't
// support cron's full grammar (no ranges/steps/lists) — we expand to a
// cross-product of concrete entries. Common cases (fixed time daily,
// weekday-only, hourly, every-N-minutes) work; exotic expressions return
// an error and the user gets the in-app scheduler instead.

#[derive(Debug, Clone, Default)]
struct CalInterval {
    minute: Option<u32>,
    hour: Option<u32>,
    day: Option<u32>,
    month: Option<u32>,
    weekday: Option<u32>,
}

fn parse_cron_field(field: &str, min: u32, max_excl: u32) -> Result<Vec<Option<u32>>, String> {
    if field == "*" {
        return Ok(vec![None]);
    }
    let mut out: Vec<u32> = Vec::new();
    for chunk in field.split(',') {
        if let Some(stripped) = chunk.strip_prefix("*/") {
            // Step: */N
            let step: u32 = stripped
                .parse()
                .map_err(|_| format!("bad step: {}", chunk))?;
            if step == 0 {
                return Err("step cannot be 0".into());
            }
            let mut v = min;
            while v < max_excl {
                out.push(v);
                v += step;
            }
        } else if let Some((lo, hi)) = chunk.split_once('-') {
            let lo: u32 = lo
                .parse()
                .map_err(|_| format!("bad range start: {}", chunk))?;
            let hi: u32 = hi
                .parse()
                .map_err(|_| format!("bad range end: {}", chunk))?;
            if lo > hi || hi >= max_excl || lo < min {
                return Err(format!("range out of bounds: {}", chunk));
            }
            for v in lo..=hi {
                out.push(v);
            }
        } else {
            let v: u32 = chunk.parse().map_err(|_| format!("bad field: {}", chunk))?;
            if v < min || v >= max_excl {
                return Err(format!("value out of bounds: {}", chunk));
            }
            out.push(v);
        }
    }
    Ok(out.into_iter().map(Some).collect())
}

fn cron_to_launchd_intervals(cron: &str) -> Result<Vec<CalInterval>, String> {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() != 5 {
        return Err("cron must have 5 fields (minute hour day month weekday)".into());
    }
    let minutes = parse_cron_field(parts[0], 0, 60)?;
    let hours = parse_cron_field(parts[1], 0, 24)?;
    let days = parse_cron_field(parts[2], 1, 32)?;
    let months = parse_cron_field(parts[3], 1, 13)?;
    // launchd weekday: 0 (Sunday) - 6 (Saturday). Cron same.
    let weekdays = parse_cron_field(parts[4], 0, 7)?;

    let mut out = Vec::new();
    for &m in &minutes {
        for &h in &hours {
            for &d in &days {
                for &mon in &months {
                    for &w in &weekdays {
                        out.push(CalInterval {
                            minute: m,
                            hour: h,
                            day: d,
                            month: mon,
                            weekday: w,
                        });
                    }
                }
            }
        }
    }
    if out.len() > 100 {
        return Err(format!(
            "cron expression expands to {} launchd entries (max 100)",
            out.len()
        ));
    }
    Ok(out)
}

fn interval_to_plist_dict(iv: &CalInterval) -> String {
    let mut out = String::from("    <dict>\n");
    if let Some(v) = iv.minute {
        out.push_str(&format!(
            "      <key>Minute</key><integer>{}</integer>\n",
            v
        ));
    }
    if let Some(v) = iv.hour {
        out.push_str(&format!(
            "      <key>Hour</key><integer>{}</integer>\n",
            v
        ));
    }
    if let Some(v) = iv.day {
        out.push_str(&format!(
            "      <key>Day</key><integer>{}</integer>\n",
            v
        ));
    }
    if let Some(v) = iv.month {
        out.push_str(&format!(
            "      <key>Month</key><integer>{}</integer>\n",
            v
        ));
    }
    if let Some(v) = iv.weekday {
        out.push_str(&format!(
            "      <key>Weekday</key><integer>{}</integer>\n",
            v
        ));
    }
    out.push_str("    </dict>\n");
    out
}

fn build_launchd_plist(
    job_id: &str,
    ato_binary: &str,
    cron: &str,
    log_dir: &str,
) -> Result<String, String> {
    let intervals = cron_to_launchd_intervals(cron)?;
    let label = format!("ai.agentictool.cron-{}", job_id);

    let interval_xml = if intervals.len() == 1 {
        interval_to_plist_dict(&intervals[0])
    } else {
        let mut s = String::from("    <array>\n");
        for iv in &intervals {
            // Indent one extra level inside the array.
            for line in interval_to_plist_dict(iv).lines() {
                s.push_str("    ");
                s.push_str(line);
                s.push('\n');
            }
        }
        s.push_str("    </array>\n");
        s
    };

    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{binary}</string>
    <string>--run-cron</string>
    <string>{job_id}</string>
  </array>
  <key>StartCalendarInterval</key>
{intervals}  <key>RunAtLoad</key>
  <false/>
  <key>StandardOutPath</key>
  <string>{log_dir}/{job_id}.out.log</string>
  <key>StandardErrorPath</key>
  <string>{log_dir}/{job_id}.err.log</string>
</dict>
</plist>
"#,
        label = label,
        binary = ato_binary,
        job_id = job_id,
        intervals = interval_xml,
        log_dir = log_dir,
    ))
}

fn launchd_plist_path(job_id: &str) -> PathBuf {
    home_dir()
        .join("Library")
        .join("LaunchAgents")
        .join(format!("ai.agentictool.cron-{}.plist", job_id))
}

fn current_ato_binary_path() -> Result<String, String> {
    // The path of the running binary. When the OS scheduler later invokes
    // the unit, it'll exec this same binary with --run-cron <id>.
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {}", e))?;
    Ok(exe.to_string_lossy().to_string())
}

// ── Linux: systemd --user timers ─────────────────────────────────────────
//
// Each cron job becomes a (.service, .timer) pair under
// `~/.config/systemd/user/`. The timer's OnCalendar field is derived from
// the cron expression — systemd's calendar grammar is a superset of cron
// (supports `*`, ranges with `..`, lists, and steps), so the mapping is
// mostly direct. Wake-from-sleep (`WakeSystem=true`) requires polkit + a
// configured RTC and isn't always honored — we set it as best-effort and
// rely on systemd to fire on next-boot via `Persistent=true` for any
// firings that were missed during sleep.

fn cron_to_systemd_oncalendar(cron: &str) -> Result<String, String> {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() != 5 {
        return Err("cron must have 5 fields (minute hour day month weekday)".into());
    }
    // Validate each field by reusing the launchd parser — same grammar.
    parse_cron_field(parts[0], 0, 60)?;
    parse_cron_field(parts[1], 0, 24)?;
    parse_cron_field(parts[2], 1, 32)?;
    parse_cron_field(parts[3], 1, 13)?;
    parse_cron_field(parts[4], 0, 7)?;

    let translate_step = |field: &str| field.replace("*/", "*/");
    let minute = translate_step(parts[0]);
    let hour = translate_step(parts[1]);
    let day = if parts[2] == "*" {
        "*".into()
    } else {
        parts[2].replace('-', "..")
    };
    let month = if parts[3] == "*" {
        "*".into()
    } else {
        parts[3].replace('-', "..")
    };

    // systemd weekdays are names: Mon..Fri, Sat,Sun. Translate the cron
    // numeric weekday (0=Sun, 6=Sat) to systemd names.
    let weekday_part = if parts[4] == "*" {
        String::new()
    } else {
        let names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let translate_one = |s: &str| -> Result<String, String> {
            let n: usize = s.parse().map_err(|_| format!("bad weekday: {}", s))?;
            if n >= 7 {
                return Err(format!("bad weekday: {}", s));
            }
            Ok(names[n].to_string())
        };
        let translated: Result<Vec<String>, String> = parts[4]
            .split(',')
            .map(|piece| {
                if let Some((lo, hi)) = piece.split_once('-') {
                    Ok(format!("{}..{}", translate_one(lo)?, translate_one(hi)?))
                } else {
                    translate_one(piece)
                }
            })
            .collect();
        let joined = translated?.join(",");
        format!("{} ", joined)
    };

    // Format: [WEEKDAY ]*-MM-DD HH:MM:SS
    Ok(format!(
        "{wd}*-{mo}-{d} {h}:{m}:00",
        wd = weekday_part,
        mo = month,
        d = day,
        h = hour,
        m = minute,
    ))
}

fn build_systemd_service(job_id: &str, ato_binary: &str) -> String {
    format!(
        r#"[Unit]
Description=ATO scheduled agent dispatch — {job_id}

[Service]
Type=oneshot
ExecStart={binary} --run-cron {job_id}
"#,
        job_id = job_id,
        binary = ato_binary,
    )
}

fn build_systemd_timer(job_id: &str, oncalendar: &str) -> String {
    format!(
        r#"[Unit]
Description=ATO scheduled agent timer — {job_id}

[Timer]
OnCalendar={oncalendar}
Persistent=true
WakeSystem=true

[Install]
WantedBy=timers.target
"#,
        job_id = job_id,
        oncalendar = oncalendar,
    )
}

#[allow(dead_code)] // only used on Linux; kept compiled elsewhere for parity.
fn systemd_user_dir() -> PathBuf {
    home_dir().join(".config").join("systemd").join("user")
}

#[allow(dead_code)]
fn systemd_unit_paths(job_id: &str) -> (PathBuf, PathBuf) {
    let dir = systemd_user_dir();
    (
        dir.join(format!("ato-cron-{}.service", job_id)),
        dir.join(format!("ato-cron-{}.timer", job_id)),
    )
}

#[cfg(target_os = "linux")]
fn register_systemd(job_id: &str, cron: &str) -> Result<String, String> {
    let binary = current_ato_binary_path()?;
    let oncalendar = cron_to_systemd_oncalendar(cron)?;
    let dir = systemd_user_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir systemd dir: {}", e))?;

    let (service_path, timer_path) = systemd_unit_paths(job_id);
    fs::write(&service_path, build_systemd_service(job_id, &binary))
        .map_err(|e| format!("write service: {}", e))?;
    fs::write(&timer_path, build_systemd_timer(job_id, &oncalendar))
        .map_err(|e| format!("write timer: {}", e))?;

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    let timer_unit = format!("ato-cron-{}.timer", job_id);
    let enable = std::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", &timer_unit])
        .output()
        .map_err(|e| format!("systemctl enable: {}", e))?;
    if !enable.status.success() {
        return Err(format!(
            "systemctl --user enable --now {} failed: {}",
            timer_unit,
            String::from_utf8_lossy(&enable.stderr)
        ));
    }
    Ok(timer_path.to_string_lossy().to_string())
}

#[cfg(target_os = "linux")]
fn unregister_systemd(job_id: &str) {
    let timer_unit = format!("ato-cron-{}.timer", job_id);
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "--now", &timer_unit])
        .output();
    let (service_path, timer_path) = systemd_unit_paths(job_id);
    let _ = fs::remove_file(&service_path);
    let _ = fs::remove_file(&timer_path);
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
}

// ── Windows: schtasks via Task Scheduler XML ─────────────────────────────
//
// We generate a Task Scheduler XML file that captures the cron schedule
// (using calendar/time triggers) and `WakeToRun=true` so the laptop wakes
// to fire the job. `schtasks /Create /XML <file> /TN <name> /F` registers
// it; /Delete removes it.

fn cron_to_schtasks_xml_trigger(cron: &str) -> Result<String, String> {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() != 5 {
        return Err("cron must have 5 fields (minute hour day month weekday)".into());
    }
    let minutes = parse_cron_field(parts[0], 0, 60)?;
    let hours = parse_cron_field(parts[1], 0, 24)?;
    let days = parse_cron_field(parts[2], 1, 32)?;
    let months = parse_cron_field(parts[3], 1, 13)?;
    let weekdays = parse_cron_field(parts[4], 0, 7)?;

    // Pick a representative start time. Task Scheduler triggers have one
    // start time + a repetition pattern, so for cron expressions like
    // `*/15 * * * *` we use StartBoundary at midnight + Repetition every 15min.
    let first_minute = minutes.first().and_then(|m| *m).unwrap_or(0);
    let first_hour = hours.first().and_then(|h| *h).unwrap_or(0);
    let start_boundary = format!("2024-01-01T{:02}:{:02}:00", first_hour, first_minute);

    // Decide trigger type based on what's specified.
    let weekday_specified = parts[4] != "*";
    let day_specified = parts[2] != "*";
    let monthly = day_specified && !weekday_specified;
    let weekly = weekday_specified;
    let multi_minute = minutes.len() > 1;
    let multi_hour = hours.len() > 1;

    if multi_minute || multi_hour {
        // Use a Time trigger with a Repetition. Repetition interval: smallest
        // step we can detect.
        let interval = if multi_minute {
            // assume even step
            if minutes.len() >= 2 {
                let m0 = minutes[0].unwrap_or(0);
                let m1 = minutes[1].unwrap_or(0);
                format!("PT{}M", m1.saturating_sub(m0).max(1))
            } else {
                "PT15M".to_string()
            }
        } else {
            "PT1H".to_string()
        };
        return Ok(format!(
            r#"    <TimeTrigger>
      <Repetition>
        <Interval>{interval}</Interval>
        <Duration>P1D</Duration>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
      <StartBoundary>{start}</StartBoundary>
      <Enabled>true</Enabled>
    </TimeTrigger>
"#,
            interval = interval,
            start = start_boundary,
        ));
    }

    if weekly {
        let names = [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ];
        let mut day_xml = String::new();
        for w in weekdays.iter().filter_map(|w| *w) {
            if let Some(name) = names.get(w as usize) {
                day_xml.push_str(&format!("        <{0} />\n", name));
            }
        }
        return Ok(format!(
            r#"    <CalendarTrigger>
      <StartBoundary>{start}</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByWeek>
        <DaysOfWeek>
{days}        </DaysOfWeek>
        <WeeksInterval>1</WeeksInterval>
      </ScheduleByWeek>
    </CalendarTrigger>
"#,
            start = start_boundary,
            days = day_xml,
        ));
    }

    if monthly {
        let mut day_xml = String::new();
        for d in days.iter().filter_map(|d| *d) {
            day_xml.push_str(&format!("        <Day>{}</Day>\n", d));
        }
        let mut month_xml = String::new();
        let month_names = [
            "", "January", "February", "March", "April", "May", "June", "July", "August",
            "September", "October", "November", "December",
        ];
        for m in months.iter().filter_map(|m| *m) {
            if let Some(name) = month_names.get(m as usize) {
                month_xml.push_str(&format!("          <{0} />\n", name));
            }
        }
        let months_block = if month_xml.is_empty() {
            String::new()
        } else {
            format!("        <Months>\n{}        </Months>\n", month_xml)
        };
        return Ok(format!(
            r#"    <CalendarTrigger>
      <StartBoundary>{start}</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByMonth>
        <DaysOfMonth>
{days}        </DaysOfMonth>
{months_block}      </ScheduleByMonth>
    </CalendarTrigger>
"#,
            start = start_boundary,
            days = day_xml,
            months_block = months_block,
        ));
    }

    // Default: daily at the specified time.
    Ok(format!(
        r#"    <CalendarTrigger>
      <StartBoundary>{start}</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByDay>
        <DaysInterval>1</DaysInterval>
      </ScheduleByDay>
    </CalendarTrigger>
"#,
        start = start_boundary,
    ))
}

fn build_schtasks_xml(job_id: &str, ato_binary: &str, cron: &str) -> Result<String, String> {
    let trigger = cron_to_schtasks_xml_trigger(cron)?;
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>ATO scheduled agent dispatch — {job_id}</Description>
  </RegistrationInfo>
  <Triggers>
{trigger}  </Triggers>
  <Settings>
    <WakeToRun>true</WakeToRun>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <Enabled>true</Enabled>
  </Settings>
  <Actions>
    <Exec>
      <Command>{binary}</Command>
      <Arguments>--run-cron {job_id}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        job_id = job_id,
        binary = ato_binary,
        trigger = trigger,
    ))
}

#[cfg(target_os = "windows")]
fn register_schtasks(job_id: &str, cron: &str) -> Result<String, String> {
    let binary = current_ato_binary_path()?;
    let xml = build_schtasks_xml(job_id, &binary, cron)?;

    // Write XML to a temp file. schtasks expects UTF-16 LE with BOM —
    // construct it explicitly so the encoding declaration in the XML
    // header isn't a lie.
    let temp_dir = std::env::temp_dir();
    let xml_path = temp_dir.join(format!("ato-cron-{}.xml", job_id));
    let mut bytes = vec![0xFF, 0xFE]; // UTF-16 LE BOM
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    fs::write(&xml_path, &bytes).map_err(|e| format!("write xml: {}", e))?;

    let task_name = format!("ATO\\Cron\\{}", job_id);
    let create = std::process::Command::new("schtasks")
        .args(["/Create", "/F", "/XML", &xml_path.to_string_lossy(), "/TN", &task_name])
        .output()
        .map_err(|e| format!("schtasks /Create: {}", e))?;
    let _ = fs::remove_file(&xml_path);
    if !create.status.success() {
        return Err(format!(
            "schtasks /Create failed: {}",
            String::from_utf8_lossy(&create.stderr)
        ));
    }
    Ok(task_name)
}

#[cfg(target_os = "windows")]
fn unregister_schtasks(job_id: &str) {
    let task_name = format!("ATO\\Cron\\{}", job_id);
    let _ = std::process::Command::new("schtasks")
        .args(["/Delete", "/F", "/TN", &task_name])
        .output();
}

#[cfg(target_os = "windows")]
fn is_schtasks_registered(job_id: &str) -> bool {
    let task_name = format!("ATO\\Cron\\{}", job_id);
    std::process::Command::new("schtasks")
        .args(["/Query", "/TN", &task_name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Public Tauri commands — OS-agnostic façade ───────────────────────────
//
// Renamed from the original `*_cron_launchd` to be honest about what they
// do across platforms. The old launchd-specific helpers are wrapped here.

#[tauri::command]
pub fn cron_os_scheduler_supported() -> bool {
    cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    ))
}

#[tauri::command]
pub fn cron_os_scheduler_kind() -> &'static str {
    if cfg!(target_os = "macos") {
        "launchd"
    } else if cfg!(target_os = "linux") {
        "systemd-user"
    } else if cfg!(target_os = "windows") {
        "schtasks"
    } else {
        "unsupported"
    }
}

#[tauri::command]
pub fn register_cron_os_scheduler(job_id: String, cron: String) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let binary = current_ato_binary_path()?;
        let log_dir = home_dir().join(".ato").join("cron-logs");
        fs::create_dir_all(&log_dir).map_err(|e| format!("mkdir cron-logs: {}", e))?;
        let plist = build_launchd_plist(&job_id, &binary, &cron, &log_dir.to_string_lossy())?;
        let path = launchd_plist_path(&job_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir LaunchAgents: {}", e))?;
        }
        fs::write(&path, &plist).map_err(|e| format!("write plist: {}", e))?;
        let _ = std::process::Command::new("launchctl")
            .args(["unload", &path.to_string_lossy()])
            .output();
        let load = std::process::Command::new("launchctl")
            .args(["load", &path.to_string_lossy()])
            .output()
            .map_err(|e| format!("launchctl load: {}", e))?;
        if !load.status.success() {
            return Err(format!(
                "launchctl load failed: {}",
                String::from_utf8_lossy(&load.stderr)
            ));
        }
        return Ok(path.to_string_lossy().to_string());
    }
    #[cfg(target_os = "linux")]
    {
        return register_systemd(&job_id, &cron);
    }
    #[cfg(target_os = "windows")]
    {
        return register_schtasks(&job_id, &cron);
    }
    #[allow(unreachable_code)]
    Err(format!(
        "OS-level cron not implemented on this platform (job {})",
        job_id
    ))
}

#[tauri::command]
pub fn unregister_cron_os_scheduler(job_id: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let path = launchd_plist_path(&job_id);
        if path.exists() {
            let _ = std::process::Command::new("launchctl")
                .args(["unload", &path.to_string_lossy()])
                .output();
            let _ = fs::remove_file(&path);
        }
    }
    #[cfg(target_os = "linux")]
    {
        unregister_systemd(&job_id);
    }
    #[cfg(target_os = "windows")]
    {
        unregister_schtasks(&job_id);
    }
    Ok(())
}

#[tauri::command]
pub fn is_cron_os_scheduler_registered(job_id: String) -> bool {
    #[cfg(target_os = "macos")]
    {
        return launchd_plist_path(&job_id).exists();
    }
    #[cfg(target_os = "linux")]
    {
        let (_, timer_path) = systemd_unit_paths(&job_id);
        return timer_path.exists();
    }
    #[cfg(target_os = "windows")]
    {
        return is_schtasks_registered(&job_id);
    }
    #[allow(unreachable_code)]
    false
}

#[cfg(test)]
mod cron_launchd_tests {
    use super::*;

    #[test]
    fn parses_simple_daily_schedule() {
        let intervals = cron_to_launchd_intervals("0 7 * * *").unwrap();
        assert_eq!(intervals.len(), 1);
        assert_eq!(intervals[0].minute, Some(0));
        assert_eq!(intervals[0].hour, Some(7));
        assert_eq!(intervals[0].day, None);
        assert_eq!(intervals[0].weekday, None);
    }

    #[test]
    fn expands_weekday_range() {
        let intervals = cron_to_launchd_intervals("0 9 * * 1-5").unwrap();
        assert_eq!(intervals.len(), 5);
        assert!(intervals
            .iter()
            .all(|i| i.minute == Some(0) && i.hour == Some(9)));
        let weekdays: Vec<u32> = intervals.iter().filter_map(|i| i.weekday).collect();
        assert_eq!(weekdays, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn expands_step_minutes() {
        let intervals = cron_to_launchd_intervals("*/15 * * * *").unwrap();
        assert_eq!(intervals.len(), 4);
        let minutes: Vec<u32> = intervals.iter().filter_map(|i| i.minute).collect();
        assert_eq!(minutes, vec![0, 15, 30, 45]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(cron_to_launchd_intervals("not a cron").is_err());
        assert!(cron_to_launchd_intervals("60 * * * *").is_err());
    }

    #[test]
    fn plist_xml_contains_label_and_binary() {
        let plist = build_launchd_plist(
            "abc-123",
            "/Applications/ATO.app/Contents/MacOS/ato-desktop",
            "0 7 * * *",
            "/tmp",
        )
        .unwrap();
        assert!(plist.contains("ai.agentictool.cron-abc-123"));
        assert!(plist.contains("/Applications/ATO.app/Contents/MacOS/ato-desktop"));
        assert!(plist.contains("--run-cron"));
        assert!(plist.contains("<integer>7</integer>"));
    }

    #[test]
    fn systemd_oncalendar_daily() {
        let cal = cron_to_systemd_oncalendar("0 7 * * *").unwrap();
        assert_eq!(cal, "*-*-* 7:0:00");
    }

    #[test]
    fn systemd_oncalendar_weekday_range() {
        let cal = cron_to_systemd_oncalendar("0 9 * * 1-5").unwrap();
        assert_eq!(cal, "Mon..Fri *-*-* 9:0:00");
    }

    #[test]
    fn systemd_oncalendar_step_minute() {
        // systemd OnCalendar accepts */15 syntax verbatim — we just pass it through.
        let cal = cron_to_systemd_oncalendar("*/15 * * * *").unwrap();
        assert!(cal.starts_with("*-*-* *:*/15:00"));
    }

    #[test]
    fn systemd_unit_files_have_required_sections() {
        let svc = build_systemd_service("abc-123", "/usr/local/bin/ato-desktop");
        assert!(svc.contains("[Unit]"));
        assert!(svc.contains("[Service]"));
        assert!(svc.contains("--run-cron abc-123"));

        let timer = build_systemd_timer("abc-123", "*-*-* 09:00:00");
        assert!(timer.contains("[Timer]"));
        assert!(timer.contains("OnCalendar=*-*-* 09:00:00"));
        assert!(timer.contains("Persistent=true"));
        assert!(timer.contains("WakeSystem=true"));
    }

    #[test]
    fn schtasks_xml_weekly_includes_days() {
        let xml = build_schtasks_xml("abc-123", "C:\\ato\\ato-desktop.exe", "0 9 * * 1-5").unwrap();
        assert!(xml.contains("WakeToRun>true"));
        assert!(xml.contains("--run-cron abc-123"));
        assert!(xml.contains("<Monday />"));
        assert!(xml.contains("<Friday />"));
        assert!(xml.contains("CalendarTrigger"));
    }

    #[test]
    fn schtasks_xml_daily() {
        let xml = build_schtasks_xml("xyz", "C:\\ato\\ato-desktop.exe", "0 7 * * *").unwrap();
        assert!(xml.contains("ScheduleByDay"));
        assert!(xml.contains("StartBoundary>2024-01-01T07:00:00"));
    }

    #[test]
    fn schtasks_xml_step_uses_repetition() {
        let xml = build_schtasks_xml("xyz", "C:\\ato\\ato-desktop.exe", "*/15 * * * *").unwrap();
        assert!(xml.contains("<Repetition>"));
        assert!(xml.contains("<Interval>PT15M</Interval>"));
    }
}
