// commands/evaluators.rs — `ato evaluators schedule|list|results|delete`.
//
// Pro feature: scheduled evaluators run against cloud traces on a
// cron (hourly/daily/weekly). The cloud service polls and executes;
// this CLI creates/manages schedules and reads results.

use clap::{Args, Subcommand};
use std::fs;
use std::io::Read;
use std::path::PathBuf;

fn cloud_base() -> String {
    std::env::var("ATO_CLOUD_URL")
        .unwrap_or_else(|_| "https://api.agentictool.ai".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn auth_file_path() -> PathBuf {
    crate::db::home_dir().join(".ato").join("auth.json")
}

fn read_token() -> Option<String> {
    crate::commands::auth::ensure_fresh_token();
    let mut file = fs::File::open(auth_file_path()).ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    json.get("token")?.as_str().map(String::from)
}

fn require_token() -> String {
    read_token().unwrap_or_else(|| {
        eprintln!("Not logged in. Run: ato login");
        std::process::exit(1);
    })
}

fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_else(|e| { eprintln!("HTTP client error: {}", e); std::process::exit(1); })
}

fn handle_schedule(agent: String, schedule: String, last_n: u32, evaluators_json: Option<String>, human: bool) {
    let token = require_token();
    let client = http_client();

    // Parse evaluators from JSON string or use a default contains check
    let evaluators: serde_json::Value = match evaluators_json {
        Some(json) => serde_json::from_str(&json).unwrap_or_else(|e| {
            eprintln!("Invalid evaluators JSON: {}", e);
            std::process::exit(1);
        }),
        None => {
            // Default: check that responses have reasonable length
            serde_json::json!([
                { "name": "response-not-empty", "kind": "length-range", "config": { "min": 10, "max": 100000 } }
            ])
        }
    };

    let resp = client
        .post(format!("{}/api/eval-schedules", cloud_base()))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "agentSlug": agent,
            "schedule": schedule,
            "lastN": last_n,
            "evaluators": evaluators,
        }))
        .send();

    match resp {
        Ok(r) if r.status().is_success() => {
            let body: serde_json::Value = r.json().unwrap_or_default();
            if human {
                let id = body.pointer("/data/id").and_then(|v| v.as_str()).unwrap_or("?");
                let next = body.pointer("/data/next_run_at").and_then(|v| v.as_str()).unwrap_or("?");
                println!("Schedule created: {}", id);
                println!("Agent:    {}", agent);
                println!("Schedule: {}", schedule);
                println!("Last N:   {}", last_n);
                println!("Next run: {}", next);
            } else {
                println!("{}", serde_json::to_string_pretty(&body).unwrap());
            }
        }
        Ok(r) => {
            let body = r.text().unwrap_or_default();
            eprintln!("Failed: {}", &body[..body.len().min(200)]);
            std::process::exit(1);
        }
        Err(e) => { eprintln!("Request failed: {}", e); std::process::exit(1); }
    }
}

fn handle_list(human: bool) {
    let token = require_token();
    let client = http_client();

    let resp = client
        .get(format!("{}/api/eval-schedules", cloud_base()))
        .bearer_auth(&token)
        .send();

    match resp {
        Ok(r) if r.status().is_success() => {
            let body: serde_json::Value = r.json().unwrap_or_default();
            if human {
                let schedules = body.pointer("/data").and_then(|v| v.as_array());
                match schedules {
                    Some(arr) if arr.is_empty() => println!("No scheduled evaluators. Create one: ato evaluators schedule --agent <slug>"),
                    Some(arr) => {
                        println!("{:<38} {:<16} {:<10} {:<8} {:<6} {}", "ID", "Agent", "Schedule", "Enabled", "Runs", "Last Run");
                        println!("{}", "-".repeat(100));
                        for s in arr {
                            let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                            let agent = s.get("agent_slug").and_then(|v| v.as_str()).unwrap_or("?");
                            let sched = s.get("schedule").and_then(|v| v.as_str()).unwrap_or("?");
                            let enabled = if s.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false) { "yes" } else { "no" };
                            let runs = s.get("total_runs").and_then(|v| v.as_i64()).unwrap_or(0);
                            let last = s.get("last_result_at").and_then(|v| v.as_str()).unwrap_or("never");
                            println!("{:<38} {:<16} {:<10} {:<8} {:<6} {}", id, agent, sched, enabled, runs, last);
                        }
                    }
                    None => println!("No schedules found."),
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&body).unwrap());
            }
        }
        Ok(r) => { eprintln!("HTTP {}", r.status().as_u16()); std::process::exit(1); }
        Err(e) => { eprintln!("Request failed: {}", e); std::process::exit(1); }
    }
}

fn handle_results(schedule_id: String, human: bool) {
    let token = require_token();
    let client = http_client();

    let resp = client
        .get(format!("{}/api/eval-schedules/{}/results", cloud_base(), schedule_id))
        .bearer_auth(&token)
        .send();

    match resp {
        Ok(r) if r.status().is_success() => {
            let body: serde_json::Value = r.json().unwrap_or_default();
            if human {
                let runs = body.pointer("/data").and_then(|v| v.as_array());
                match runs {
                    Some(arr) if arr.is_empty() => println!("No results yet. The scheduler runs on the configured interval."),
                    Some(arr) => {
                        for run in arr {
                            let at = run.get("created_at").and_then(|v| v.as_str()).unwrap_or("?");
                            let traces = run.get("traces_evaluated").and_then(|v| v.as_i64()).unwrap_or(0);
                            let pass = run.get("pass_count").and_then(|v| v.as_i64()).unwrap_or(0);
                            let fail = run.get("fail_count").and_then(|v| v.as_i64()).unwrap_or(0);
                            let partial = run.get("partial_count").and_then(|v| v.as_i64()).unwrap_or(0);
                            let ms = run.get("duration_ms").and_then(|v| v.as_i64()).unwrap_or(0);
                            let err = run.get("error").and_then(|v| v.as_str());

                            let status = if err.is_some() { "\x1b[31mERROR\x1b[0m" } else if fail > 0 { "\x1b[33mWARN\x1b[0m" } else { "\x1b[32mOK\x1b[0m" };
                            println!("[{}] {} — {} traces, {}P/{}F/{}U ({}ms)",
                                status, at, traces, pass, fail, partial, ms);
                            if let Some(e) = err {
                                println!("  Error: {}", e);
                            }
                        }
                    }
                    None => println!("No results."),
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&body).unwrap());
            }
        }
        Ok(r) => { eprintln!("HTTP {}", r.status().as_u16()); std::process::exit(1); }
        Err(e) => { eprintln!("Request failed: {}", e); std::process::exit(1); }
    }
}

fn handle_delete(schedule_id: String) {
    let token = require_token();
    let client = http_client();

    let resp = client
        .delete(format!("{}/api/eval-schedules/{}", cloud_base(), schedule_id))
        .bearer_auth(&token)
        .send();

    match resp {
        Ok(r) if r.status().is_success() => println!("Schedule deleted."),
        Ok(r) => { eprintln!("HTTP {}", r.status().as_u16()); std::process::exit(1); }
        Err(e) => { eprintln!("Request failed: {}", e); std::process::exit(1); }
    }
}

#[derive(Subcommand, Debug)]
pub enum EvaluatorsCommand {
    /// Create a scheduled evaluator for an agent (runs on cloud traces).
    Schedule {
        /// Agent slug to evaluate
        #[arg(long)]
        agent: String,
        /// Schedule: hourly, every6h, daily, weekly
        #[arg(long, default_value = "daily")]
        schedule: String,
        /// Number of recent traces to evaluate per run
        #[arg(long, default_value = "10")]
        last_n: u32,
        /// Evaluators as JSON array (optional, defaults to response-not-empty check)
        #[arg(long)]
        evaluators: Option<String>,
    },
    /// List all scheduled evaluators.
    List,
    /// Show results from a scheduled evaluator.
    Results {
        /// Schedule ID
        id: String,
    },
    /// Delete a scheduled evaluator.
    Delete {
        /// Schedule ID
        id: String,
    },
}

#[derive(Args, Debug)]
pub struct EvaluatorsArgs {
    #[command(subcommand)]
    pub cmd: EvaluatorsCommand,
}

pub fn run(args: EvaluatorsArgs, human: bool) {
    match args.cmd {
        EvaluatorsCommand::Schedule { agent, schedule, last_n, evaluators } => {
            handle_schedule(agent, schedule, last_n, evaluators, human);
        }
        EvaluatorsCommand::List => handle_list(human),
        EvaluatorsCommand::Results { id } => handle_results(id, human),
        EvaluatorsCommand::Delete { id } => handle_delete(id),
    }
}
