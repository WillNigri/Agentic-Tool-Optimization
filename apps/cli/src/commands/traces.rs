// commands/traces.rs — `ato traces backfill`.
//
// Reads local traces from ~/.ato/agent-logs.jsonl and uploads them
// to the cloud. Gives day-1 analytics for new Pro users and fills
// gaps from logged-out periods.

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
    let mut file = fs::File::open(auth_file_path()).ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    json.get("token")?.as_str().map(String::from)
}

fn logs_path() -> PathBuf {
    crate::db::home_dir().join(".ato").join("agent-logs.jsonl")
}

fn handle_backfill(days: u32, human: bool) {
    let token = match read_token() {
        Some(t) => t,
        None => { eprintln!("Not logged in. Run: ato login"); std::process::exit(1); }
    };

    // Read from SQLite execution_logs (the real data with tokens/cost)
    let db_path = crate::db::default_db_path();
    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cannot open {}: {}", db_path.display(), e);
            std::process::exit(1);
        }
    };

    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    let cutoff_str = cutoff.to_rfc3339();

    let mut stmt = conn.prepare(
        "SELECT id, runtime, tokens_in, tokens_out, duration_ms, status, error_message,
                cost_usd_estimated, agent_slug, model, created_at, prompt, war_room_id
         FROM execution_logs
         WHERE created_at >= ?1
         ORDER BY created_at ASC"
    ).unwrap_or_else(|e| { eprintln!("Query error: {}", e); std::process::exit(1); });

    let mut traces: Vec<serde_json::Value> = Vec::new();

    let rows = stmt.query_map([&cutoff_str], |row| {
        Ok((
            row.get::<_, String>(0)?,  // id
            row.get::<_, String>(1)?,  // runtime
            row.get::<_, Option<i64>>(2)?, // tokens_in
            row.get::<_, Option<i64>>(3)?, // tokens_out
            row.get::<_, Option<i64>>(4)?, // duration_ms
            row.get::<_, String>(5)?,  // status
            row.get::<_, Option<String>>(6)?, // error_message
            row.get::<_, Option<f64>>(7)?,  // cost_usd_estimated
            row.get::<_, Option<String>>(8)?, // agent_slug
            row.get::<_, Option<String>>(9)?, // model
            row.get::<_, String>(10)?,  // created_at
            row.get::<_, Option<String>>(11)?, // prompt (for summary)
            row.get::<_, Option<String>>(12)?, // war_room_id
        ))
    });

    if let Ok(rows) = rows {
        for row in rows.flatten() {
            let (id, runtime, tokens_in, tokens_out, duration_ms, status, error,
                 cost, agent_slug, model, created_at, prompt, war_room_id) = row;

            let ok = status == "success" || status == "ok";
            let slug = agent_slug
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown-agent".to_string());
            let summary = prompt.and_then(|p| {
                let trimmed: String = p.chars().take(200).collect();
                if trimmed.is_empty() { None } else { Some(trimmed) }
            });

            let mut metadata = serde_json::json!({
                "backfilledAt": chrono::Utc::now().to_rfc3339(),
                "localId": id,
            });
            if let Some(ref model) = model {
                metadata["model"] = serde_json::Value::String(model.clone());
            }
            if let Some(ref wrid) = war_room_id {
                metadata["warRoomId"] = serde_json::Value::String(wrid.clone());
            }

            // Build trace — only include optional numeric fields when present
            // (cloud zod schema rejects null but accepts missing)
            let mut trace = serde_json::json!({
                "agentSlug": slug,
                "runtime": runtime,
                "startedAt": created_at,
                "durationMs": duration_ms.unwrap_or(0),
                "ok": ok,
                "source": "cli-backfill",
                "metadata": metadata,
            });
            if let Some(ti) = tokens_in { trace["promptTokens"] = serde_json::json!(ti); }
            if let Some(to_) = tokens_out { trace["responseTokens"] = serde_json::json!(to_); }
            if let Some(c) = cost { trace["costUsd"] = serde_json::json!(c); }
            if let Some(ref e) = error { trace["error"] = serde_json::json!(e); }
            if let Some(ref s) = summary { trace["promptSummary"] = serde_json::json!(s); }

            traces.push(trace);
        }
    }

    if traces.is_empty() {
        if human { println!("No traces found in last {} days.", days); }
        return;
    }

    if human {
        println!("Found {} local traces from last {} days. Uploading to cloud...", traces.len(), days);
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|e| { eprintln!("HTTP error: {}", e); std::process::exit(1); });

    let mut uploaded = 0;
    let mut failed = 0;

    // Upload in batches of 50
    for chunk in traces.chunks(50) {
        let resp = client
            .post(format!("{}/api/agent-traces", cloud_base()))
            .bearer_auth(&token)
            .json(&serde_json::json!({ "traces": chunk }))
            .send();

        match resp {
            Ok(r) if r.status().is_success() => {
                let body: serde_json::Value = r.json().unwrap_or_default();
                let n = body.pointer("/data/inserted").and_then(|v| v.as_i64()).unwrap_or(chunk.len() as i64);
                uploaded += n as usize;
            }
            _ => { failed += chunk.len(); }
        }
    }

    if human {
        println!("Backfill complete: {} uploaded, {} failed.", uploaded, failed);
    } else {
        println!("{}", serde_json::json!({
            "total_local": traces.len(),
            "uploaded": uploaded,
            "failed": failed,
            "days": days,
        }));
    }
}

#[derive(Subcommand, Debug)]
pub enum TracesCommand {
    /// Upload local traces to cloud (fills gaps from logged-out periods).
    Backfill {
        /// Number of days to look back
        #[arg(long, default_value = "30")]
        days: u32,
    },
}

#[derive(Args, Debug)]
pub struct TracesArgs {
    #[command(subcommand)]
    pub cmd: TracesCommand,
}

pub fn run(args: TracesArgs, human: bool) {
    match args.cmd {
        TracesCommand::Backfill { days } => handle_backfill(days, human),
    }
}
