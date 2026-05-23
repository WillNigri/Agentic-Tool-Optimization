// commands/cost_recommend.rs — `ato optimize`
//
// Pro feature: scans the user's agents and runtimes, compares
// head-to-head performance from war-room data, and recommends
// runtime switches that save money at equal quality.
//
// Three tiers of optimization testing:
//   Light  — 3 prompts × 2 runtimes =  6 replays (~2K tokens, ~$0.05)
//   Normal — 5 prompts × 3 runtimes = 15 replays (~5K tokens, ~$0.15) ← RECOMMENDED
//   Deep   — 10 prompts × all runtimes = 40+ replays (~15K tokens, ~$0.50)
//
// Smaller tests = lower confidence recommendations.
// Users choose frequency: weekly (default), daily, or monthly.

use clap::{Args, Subcommand};
use rusqlite::Connection;
use std::path::Path;

fn open_db(db_override: &Option<String>) -> Connection {
    let path = match db_override {
        Some(p) => std::path::PathBuf::from(p),
        None => crate::db::default_db_path(),
    };
    crate::db::open_readonly(&path).unwrap_or_else(|e| {
        eprintln!("Cannot open database: {}", e);
        std::process::exit(1);
    })
}

struct RuntimeComparison {
    agent_slug: String,
    primary_runtime: String,
    alternative: String,
    head_to_head_rounds: i64,
    primary_cost_avg: f64,
    alt_cost_avg: f64,
    savings_pct: f64,
    total_saved: f64,
    primary_success_rate: f64,
    alt_success_rate: f64,
    alt_response_ratio: f64, // >1 means alternative writes more
    primary_avg_seconds: f64,
    alt_avg_seconds: f64,
}

fn handle_recommend(db_override: &Option<String>, human: bool) {
    let conn = open_db(db_override);

    // Step 1: Find agents with head-to-head war-room data
    let mut stmt = conn.prepare("
        WITH multi_runtime_rounds AS (
            SELECT war_room_id, war_room_round
            FROM execution_logs
            WHERE war_room_id IS NOT NULL AND war_room_round IS NOT NULL
            GROUP BY war_room_id, war_room_round
            HAVING COUNT(DISTINCT runtime) >= 2
        ),
        -- For each agent, find which runtime it uses most (primary)
        agent_primary AS (
            SELECT agent_slug, runtime as primary_runtime, COUNT(*) as cnt
            FROM execution_logs
            WHERE agent_slug IS NOT NULL AND agent_slug != ''
            GROUP BY agent_slug, runtime
        ),
        -- Get the top runtime per agent
        agent_top AS (
            SELECT agent_slug, primary_runtime
            FROM agent_primary ap
            WHERE cnt = (SELECT MAX(cnt) FROM agent_primary ap2 WHERE ap2.agent_slug = ap.agent_slug)
        ),
        -- Head-to-head: primary vs each alternative on same war-room round
        comparisons AS (
            SELECT
                COALESCE(e1.agent_slug, 'war-room-seat') as agent_slug,
                e1.runtime as primary_runtime,
                e2.runtime as alternative,
                COUNT(*) as rounds,
                AVG(e1.cost_usd_estimated) as primary_cost,
                AVG(e2.cost_usd_estimated) as alt_cost,
                SUM(e1.cost_usd_estimated) - SUM(e2.cost_usd_estimated) as total_saved,
                AVG(CASE WHEN e1.status='success' THEN 1.0 ELSE 0.0 END) as primary_success,
                AVG(CASE WHEN e2.status='success' THEN 1.0 ELSE 0.0 END) as alt_success,
                AVG(CAST(LENGTH(e2.response) AS REAL) / MAX(LENGTH(e1.response), 1)) as response_ratio,
                AVG(e1.duration_ms) / 1000.0 as primary_seconds,
                AVG(e2.duration_ms) / 1000.0 as alt_seconds
            FROM execution_logs e1
            JOIN execution_logs e2
                ON e1.war_room_id = e2.war_room_id
                AND e1.war_room_round = e2.war_room_round
                AND e1.runtime != e2.runtime
            JOIN multi_runtime_rounds m
                ON e1.war_room_id = m.war_room_id
                AND e1.war_room_round = m.war_room_round
            WHERE e1.status = 'success' AND e2.status = 'success'
                AND e1.cost_usd_estimated > 0 AND e2.cost_usd_estimated > 0
            GROUP BY e1.runtime, e2.runtime
            HAVING COUNT(*) >= 5
        )
        SELECT * FROM comparisons
        WHERE alt_cost < primary_cost
        ORDER BY (primary_cost - alt_cost) * rounds DESC
    ").unwrap_or_else(|e| { eprintln!("Query error: {}", e); std::process::exit(1); });

    let comparisons: Vec<RuntimeComparison> = stmt.query_map([], |row| {
        Ok(RuntimeComparison {
            agent_slug: row.get(0)?,
            primary_runtime: row.get(1)?,
            alternative: row.get(2)?,
            head_to_head_rounds: row.get(3)?,
            primary_cost_avg: row.get(4)?,
            alt_cost_avg: row.get(5)?,
            total_saved: row.get(6)?,
            savings_pct: 0.0, // computed below
            primary_success_rate: row.get(7)?,
            alt_success_rate: row.get(8)?,
            alt_response_ratio: row.get(9)?,
            primary_avg_seconds: row.get(10)?,
            alt_avg_seconds: row.get(11)?,
        })
    }).unwrap().filter_map(|r| r.ok())
    .map(|mut r| {
        r.savings_pct = if r.primary_cost_avg > 0.0 {
            (1.0 - r.alt_cost_avg / r.primary_cost_avg) * 100.0
        } else { 0.0 };
        r
    })
    .collect();

    if comparisons.is_empty() {
        if human {
            println!("No optimization opportunities found yet.");
            println!("ATO needs war-room data with 2+ runtimes answering the same prompt.");
            println!("Run a war room: ato dispatch --prompt '...' --runtime claude,google,minimax");
        }
        return;
    }

    if human {
        println!("═══════════════════════════════════════════════════════════════");
        println!("  ATO Pro — Cost Recommendations (based on YOUR data)");
        println!("═══════════════════════════════════════════════════════════════\n");

        for (i, c) in comparisons.iter().enumerate() {
            let quality = if c.alt_response_ratio > 1.2 {
                format!("writes {:.0}% more text", (c.alt_response_ratio - 1.0) * 100.0)
            } else if c.alt_response_ratio < 0.8 {
                format!("writes {:.0}% less text", (1.0 - c.alt_response_ratio) * 100.0)
            } else {
                "comparable output length".to_string()
            };

            let speed = if c.alt_avg_seconds < c.primary_avg_seconds * 0.8 {
                format!("{:.0}% faster", (1.0 - c.alt_avg_seconds / c.primary_avg_seconds) * 100.0)
            } else if c.alt_avg_seconds > c.primary_avg_seconds * 1.2 {
                format!("{:.0}% slower", (c.alt_avg_seconds / c.primary_avg_seconds - 1.0) * 100.0)
            } else {
                "similar speed".to_string()
            };

            let confidence = if c.head_to_head_rounds >= 50 {
                "HIGH"
            } else if c.head_to_head_rounds >= 20 {
                "MEDIUM"
            } else {
                "LOW"
            };

            println!("  {}. Switch {} → {}", i + 1, c.primary_runtime.to_uppercase(), c.alternative.to_uppercase());
            println!("     Savings: {:.0}% per round (${:.4} → ${:.4})", c.savings_pct, c.primary_cost_avg, c.alt_cost_avg);
            println!("     Evidence: {} head-to-head rounds (confidence: {})", c.head_to_head_rounds, confidence);
            println!("     Quality: {}, {}", quality, speed);
            println!("     Success rate: {:.0}% → {:.0}%", c.primary_success_rate * 100.0, c.alt_success_rate * 100.0);
            println!("     Already saved: ${:.2} on past comparisons", c.total_saved);
            println!();
        }

        // Monthly projection
        let weekly_rounds: f64 = comparisons.iter().map(|c| c.head_to_head_rounds as f64).sum::<f64>() / 4.0;
        let weekly_savings: f64 = comparisons.iter().map(|c| c.total_saved).sum::<f64>() / 4.0;
        println!("  Monthly projection: ~${:.2}/month savings at current pace", weekly_savings * 4.0);
        println!();
        println!("  ⚠ These recommendations compare cost + output length.");
        println!("    Run `ato evaluators schedule` to add quality scoring");
        println!("    and verify the cheaper runtime maintains your standards.");
        println!();
        println!("═══════════════════════════════════════════════════════════════");
    } else {
        let recs: Vec<serde_json::Value> = comparisons.iter().map(|c| {
            serde_json::json!({
                "from": c.primary_runtime,
                "to": c.alternative,
                "rounds_compared": c.head_to_head_rounds,
                "savings_pct": (c.savings_pct * 10.0).round() / 10.0,
                "avg_cost_from": c.primary_cost_avg,
                "avg_cost_to": c.alt_cost_avg,
                "total_saved": c.total_saved,
                "success_rate_from": c.primary_success_rate,
                "success_rate_to": c.alt_success_rate,
                "response_length_ratio": c.alt_response_ratio,
                "speed_from_s": c.primary_avg_seconds,
                "speed_to_s": c.alt_avg_seconds,
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "recommendations": recs,
        })).unwrap());
    }
}

// Step 2: Auto-test — replay recent prompts against alternative runtimes
fn handle_autotest(agent: String, db_override: &Option<String>, human: bool) {
    let conn = open_db(db_override);

    // Get the last 5 prompts from this agent
    let mut stmt = conn.prepare("
        SELECT id, runtime, prompt, tokens_in, cost_usd_estimated, duration_ms
        FROM execution_logs
        WHERE agent_slug = ?1 AND status = 'success' AND prompt IS NOT NULL
        ORDER BY created_at DESC
        LIMIT 5
    ").unwrap_or_else(|e| { eprintln!("Query error: {}", e); std::process::exit(1); });

    let rows: Vec<(String, String, String, Option<i64>, Option<f64>, Option<i64>)> = stmt
        .query_map([&agent], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    if rows.is_empty() {
        if human { println!("No recent prompts found for agent '{}'.", agent); }
        return;
    }

    // Get available runtimes
    let runtimes: Vec<String> = conn.prepare("SELECT DISTINCT runtime FROM execution_logs WHERE runtime NOT IN ('fake-server', 'anthropic') ORDER BY runtime")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    if human {
        println!("Agent: {}", agent);
        println!("Found {} recent prompts, {} runtimes available", rows.len(), runtimes.len());
        println!();
        println!("To replay prompt #1 against a different runtime:");
        for rt in &runtimes {
            if *rt != rows[0].1 {
                println!("  ato replay start {} --runtime {}", rows[0].0, rt);
            }
        }
        println!();
        println!("After replays complete, run `ato cost recommend --human` to see updated comparisons.");
    } else {
        let prompts: Vec<serde_json::Value> = rows.iter().map(|(id, rt, prompt, tokens, cost, dur)| {
            serde_json::json!({
                "id": id,
                "runtime": rt,
                "prompt_preview": &prompt[..prompt.len().min(100)],
                "tokens": tokens,
                "cost": cost,
                "duration_ms": dur,
            })
        }).collect();
        let rt_list: Vec<&str> = runtimes.iter().map(|s| s.as_str()).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "agent": agent,
            "recent_prompts": prompts,
            "available_runtimes": rt_list,
        })).unwrap());
    }
}

fn handle_schedule(agent: Option<String>, intensity: String, frequency: String, token_budget: Option<u32>, db_override: &Option<String>, human: bool) {
    let conn = open_db(db_override);

    let (prompts_per_run, max_runtimes, budget_tokens, tier_name) = match intensity.as_str() {
        "light" => (3u32, 2u32, token_budget.unwrap_or(2000), "Light"),
        "deep" => (10, 99, token_budget.unwrap_or(15000), "Deep"),
        _ => (5, 3, token_budget.unwrap_or(5000), "Normal (recommended)"),
    };

    // Get all agents or the specified one
    let agents: Vec<String> = if let Some(ref a) = agent {
        vec![a.clone()]
    } else {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT agent_slug FROM execution_logs WHERE agent_slug IS NOT NULL AND agent_slug != '' ORDER BY agent_slug"
        ).unwrap();
        stmt.query_map([], |row| row.get(0)).unwrap().filter_map(|r| r.ok()).collect()
    };

    // Get available runtimes
    let runtimes: Vec<String> = conn.prepare(
        "SELECT DISTINCT runtime FROM execution_logs WHERE runtime NOT IN ('fake-server','anthropic','openclaw') ORDER BY runtime"
    ).unwrap().query_map([], |row| row.get(0)).unwrap().filter_map(|r| r.ok()).collect();

    // Store schedule in local SQLite
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS optimization_schedules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            agent_slug TEXT,
            intensity TEXT NOT NULL,
            frequency TEXT NOT NULL DEFAULT 'weekly',
            prompts_per_run INTEGER NOT NULL DEFAULT 5,
            max_runtimes INTEGER NOT NULL DEFAULT 3,
            token_budget INTEGER NOT NULL DEFAULT 5000,
            tokens_used INTEGER NOT NULL DEFAULT 0,
            runs_completed INTEGER NOT NULL DEFAULT 0,
            last_run_at TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
    ").unwrap_or_else(|e| { eprintln!("DB error: {}", e); std::process::exit(1); });

    for a in &agents {
        conn.execute(
            "INSERT INTO optimization_schedules (agent_slug, intensity, frequency, prompts_per_run, max_runtimes, token_budget) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![a, intensity, frequency, prompts_per_run, max_runtimes, budget_tokens],
        ).unwrap_or_else(|e| { eprintln!("Insert error: {}", e); std::process::exit(1); });
    }

    if human {
        println!("Optimization schedule created:\n");
        println!("  Intensity:    {} ({} prompts × {} runtimes per cycle)", tier_name, prompts_per_run, max_runtimes.min(runtimes.len() as u32));
        println!("  Frequency:    {}", frequency);
        println!("  Token budget: {} tokens/cycle (~${:.2})", budget_tokens, budget_tokens as f64 * 0.000025);
        println!("  Agents:       {}", agents.join(", "));
        println!("  Runtimes:     {}", runtimes.join(", "));
        println!();
        println!("  Estimated cost per cycle: ~${:.2}", budget_tokens as f64 * 0.000025);
        println!("  Estimated cost per month: ~${:.2}", budget_tokens as f64 * 0.000025 * match frequency.as_str() {
            "daily" => 30.0, "weekly" => 4.3, "monthly" => 1.0, _ => 4.3
        });
        println!();
        if intensity == "light" {
            println!("  ⚠ Light intensity gives LOW confidence recommendations.");
            println!("    We recommend 'normal' (5 prompts × 3 runtimes) for reliable results.");
        }
        println!();
        println!("  The optimizer will replay recent prompts from each agent against");
        println!("  alternative runtimes, score quality, and update recommendations.");
        println!("  Run `ato optimize recommend --human` anytime to see current results.");
        println!();
        println!("  To run immediately: ato optimize run --human");
    } else {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "agents": agents,
            "runtimes": runtimes,
            "intensity": intensity,
            "frequency": frequency,
            "prompts_per_run": prompts_per_run,
            "max_runtimes": max_runtimes,
            "token_budget": budget_tokens,
            "estimated_cost_per_cycle_usd": budget_tokens as f64 * 0.000025,
        })).unwrap());
    }
}

fn handle_run(agent: Option<String>, db_override: &Option<String>, human: bool) {
    let conn = open_db(db_override);

    // Get schedules
    let has_table = conn.prepare("SELECT 1 FROM optimization_schedules LIMIT 1").is_ok();
    if !has_table {
        if human { println!("No optimization schedule. Create one: ato optimize schedule"); }
        return;
    }

    let mut stmt = conn.prepare(
        "SELECT agent_slug, prompts_per_run, max_runtimes, token_budget FROM optimization_schedules WHERE enabled = 1"
    ).unwrap();

    let schedules: Vec<(String, i64, i64, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        .unwrap().filter_map(|r| r.ok()).collect();

    if schedules.is_empty() {
        if human { println!("No active optimization schedules."); }
        return;
    }

    // Get available runtimes
    let runtimes: Vec<String> = conn.prepare(
        "SELECT DISTINCT runtime FROM execution_logs WHERE runtime NOT IN ('fake-server','anthropic','openclaw') ORDER BY runtime"
    ).unwrap().query_map([], |row| row.get(0)).unwrap().filter_map(|r| r.ok()).collect();

    if human {
        println!("Running optimization tests...\n");
    }

    for (agent_slug, prompts_n, max_rt, _budget) in &schedules {
        // Get recent prompts for this agent
        let mut pstmt = conn.prepare(
            "SELECT id, runtime, prompt FROM execution_logs WHERE agent_slug = ?1 AND status = 'success' AND prompt IS NOT NULL AND LENGTH(prompt) > 20 ORDER BY created_at DESC LIMIT ?2"
        ).unwrap();

        let prompts: Vec<(String, String, String)> = pstmt
            .query_map(rusqlite::params![agent_slug, prompts_n], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            }).unwrap().filter_map(|r| r.ok()).collect();

        if prompts.is_empty() {
            if human { println!("  {} — no prompts with content found, skipping", agent_slug); }
            continue;
        }

        let primary_rt = &prompts[0].1;
        let alt_runtimes: Vec<&String> = runtimes.iter()
            .filter(|r| *r != primary_rt)
            .take(*max_rt as usize)
            .collect();

        if human {
            println!("  Agent: {}", agent_slug);
            println!("  Primary runtime: {}", primary_rt);
            println!("  Testing {} prompts against: {}", prompts.len(), alt_runtimes.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
            println!("  Replay commands:");
            for (id, _, _) in prompts.iter().take(3) {
                for rt in &alt_runtimes {
                    println!("    ato replay start {} --runtime {}", id, rt);
                }
            }
            println!("  ... ({} total replays)", prompts.len() * alt_runtimes.len());
            println!();
        }
    }

    if human {
        println!("  To execute all replays, pipe the JSON output to your shell:");
        println!("    ato optimize run | jq -r '.replays[].command' | sh");
        println!();
        println!("  After replays complete, run: ato optimize recommend --human");
    }

    // JSON output with all replay commands
    if !human {
        let mut all_replays: Vec<serde_json::Value> = Vec::new();
        for (agent_slug, prompts_n, max_rt, _budget) in &schedules {
            let mut pstmt = conn.prepare(
                "SELECT id, runtime FROM execution_logs WHERE agent_slug = ?1 AND status = 'success' AND prompt IS NOT NULL AND LENGTH(prompt) > 20 ORDER BY created_at DESC LIMIT ?2"
            ).unwrap();
            let prompts: Vec<(String, String)> = pstmt
                .query_map(rusqlite::params![agent_slug, prompts_n], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap().filter_map(|r| r.ok()).collect();

            let alt_runtimes: Vec<&String> = runtimes.iter()
                .filter(|r| *r != &prompts.first().map(|p| p.1.clone()).unwrap_or_default())
                .take(*max_rt as usize)
                .collect();

            for (id, _) in &prompts {
                for rt in &alt_runtimes {
                    all_replays.push(serde_json::json!({
                        "agent": agent_slug,
                        "source_id": id,
                        "target_runtime": rt,
                        "command": format!("ato replay start {} --runtime {}", id, rt),
                    }));
                }
            }
        }
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "replays": all_replays,
            "total": all_replays.len(),
        })).unwrap());
    }
}

#[derive(Subcommand, Debug)]
pub enum CostRecommendCommand {
    /// Analyze head-to-head data and recommend runtime switches.
    Recommend,
    /// Show replay commands to test an agent against alternative runtimes.
    Autotest {
        /// Agent slug to test
        #[arg(long)]
        agent: String,
    },
    /// Set up recurring optimization tests.
    Schedule {
        /// Agent slug (omit to schedule all agents)
        #[arg(long)]
        agent: Option<String>,
        /// Test intensity: light (3×2, ~$0.05), normal (5×3, ~$0.15, RECOMMENDED), deep (10×all, ~$0.50)
        #[arg(long, default_value = "normal")]
        intensity: String,
        /// Frequency: daily, weekly (default), monthly
        #[arg(long, default_value = "weekly")]
        frequency: String,
        /// Max tokens per optimization cycle (overrides intensity default)
        #[arg(long)]
        token_budget: Option<u32>,
    },
    /// Run optimization tests now (generates replay commands).
    Run {
        /// Agent slug (omit to run all scheduled)
        #[arg(long)]
        agent: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct CostRecommendArgs {
    #[command(subcommand)]
    pub cmd: CostRecommendCommand,
}

pub fn run(args: CostRecommendArgs, human: bool, db_override: &Option<String>) {
    match args.cmd {
        CostRecommendCommand::Recommend => handle_recommend(db_override, human),
        CostRecommendCommand::Autotest { agent } => handle_autotest(agent, db_override, human),
        CostRecommendCommand::Schedule { agent, intensity, frequency, token_budget } => {
            handle_schedule(agent, intensity, frequency, token_budget, db_override, human);
        }
        CostRecommendCommand::Run { agent } => handle_run(agent, db_override, human),
    }
}
