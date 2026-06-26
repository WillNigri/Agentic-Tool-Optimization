// commands/cost_recommend.rs — `ato optimize`
//
// Pro feature: scans the user's agents and models, compares
// head-to-head performance from war-room data AND pricing table,
// and recommends model/runtime switches that save money at equal quality.
//
// Two layers of recommendations:
//   1. Pricing-based — purely from what models the user runs + pricing table.
//      No replays needed. "You use claude-opus. claude-haiku is 93% cheaper."
//   2. Evidence-based — from head-to-head war-room/replay data.
//      Includes quality metrics (response length, success rate, speed).
//
// Model-level tiers:
//   Tier 1 (same provider, cheaper model): safest — same API, lower cost
//   Tier 2 (cross-provider, same class):   moderate risk, bigger savings
//   Tier 3 (cross-provider, diff class):   highest savings, test first

use clap::{Args, Subcommand};
use rusqlite::Connection;
use crate::runtime;

fn db_path(db_override: &Option<String>) -> std::path::PathBuf {
    match db_override {
        Some(p) => std::path::PathBuf::from(p),
        None => crate::db::default_db_path(),
    }
}

fn open_db(db_override: &Option<String>) -> Connection {
    crate::db::open_readonly(&db_path(db_override)).unwrap_or_else(|e| {
        eprintln!("Cannot open database: {}", e);
        std::process::exit(1);
    })
}

fn open_db_rw(db_override: &Option<String>) -> Connection {
    crate::db::open_readwrite(&db_path(db_override)).unwrap_or_else(|e| {
        eprintln!("Cannot open database for writing: {}", e);
        std::process::exit(1);
    })
}

// ── Pricing-based model recommendations (no replay data needed) ──────

struct ModelUsage {
    agent_slug: String,
    model: String,
    runtime: String,
    runs: i64,
    avg_cost: f64,
    total_cost: f64,
    avg_duration_s: f64,
    success_rate: f64,
}

struct ModelRecommendation {
    agent_slug: String,
    current_model: String,
    current_runs: i64,
    current_avg_cost: f64,
    current_total_cost: f64,
    alt_model: String,
    savings_pct: f64,
    tier: u8, // 1=same provider, 2=cross-provider
}

fn handle_recommend(db_override: &Option<String>, human: bool) {
    let conn = open_db(db_override);

    // Layer 1: Pricing-based recommendations from execution history
    let mut stmt = conn.prepare("
        SELECT
            COALESCE(agent_slug, '(no agent)') as agent,
            model,
            runtime,
            COUNT(*) as runs,
            AVG(cost_usd_estimated) as avg_cost,
            SUM(cost_usd_estimated) as total_cost,
            AVG(duration_ms) / 1000.0 as avg_seconds,
            AVG(CASE WHEN status='success' THEN 1.0 ELSE 0.0 END) as success_rate
        FROM execution_logs
        WHERE model IS NOT NULL AND model != ''
          AND cost_usd_estimated > 0
          AND dispatch_kind = 'active'
        GROUP BY agent_slug, model, runtime
        HAVING COUNT(*) >= 3
        ORDER BY total_cost DESC
    ").unwrap_or_else(|e| { eprintln!("Query error: {}", e); std::process::exit(1); });

    let usages: Vec<ModelUsage> = stmt.query_map([], |row| {
        Ok(ModelUsage {
            agent_slug: row.get(0)?,
            model: row.get(1)?,
            runtime: row.get(2)?,
            runs: row.get(3)?,
            avg_cost: row.get(4)?,
            total_cost: row.get(5)?,
            avg_duration_s: row.get(6)?,
            success_rate: row.get(7)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    if usages.is_empty() {
        if human {
            println!("No dispatch data found. Run some dispatches first:");
            println!("  ato dispatch claude \"your prompt\"");
        }
        return;
    }

    // Generate pricing-based recommendations
    let mut recs: Vec<ModelRecommendation> = Vec::new();
    for usage in &usages {
        let current_out_price = match runtime::pricing_for_model(&usage.model) {
            Some((_, out)) => out,
            None => continue,
        };

        // Tier 1: same provider, cheaper model
        for &alt in &runtime::cheaper_same_provider(&usage.model) {
            let alt_out = runtime::pricing_for_model(alt).unwrap().1;
            let savings = (1.0 - alt_out / current_out_price) * 100.0;
            recs.push(ModelRecommendation {
                agent_slug: usage.agent_slug.clone(),
                current_model: usage.model.clone(),
                current_runs: usage.runs,
                current_avg_cost: usage.avg_cost,
                current_total_cost: usage.total_cost,
                alt_model: alt.to_string(),
                savings_pct: savings,
                tier: 1,
            });
        }

        // Tier 2: cross-provider, cheaper (top 3)
        for &alt in runtime::cheaper_cross_provider(&usage.model).iter().take(3) {
            let alt_out = runtime::pricing_for_model(alt).unwrap().1;
            let savings = (1.0 - alt_out / current_out_price) * 100.0;
            recs.push(ModelRecommendation {
                agent_slug: usage.agent_slug.clone(),
                current_model: usage.model.clone(),
                current_runs: usage.runs,
                current_avg_cost: usage.avg_cost,
                current_total_cost: usage.total_cost,
                alt_model: alt.to_string(),
                savings_pct: savings,
                tier: 2,
            });
        }
    }

    recs.sort_by(|a, b| b.savings_pct.partial_cmp(&a.savings_pct).unwrap());

    // Layer 2: Evidence-based (head-to-head war-room data)
    let evidence = load_evidence(&conn);

    if human {
        print_human_recommend(&usages, &recs, &evidence);
    } else {
        print_json_recommend(&usages, &recs, &evidence);
    }
}

// ── Evidence-based: head-to-head comparisons ─────────────────────────

struct EvidenceComparison {
    primary_model: String,
    alt_model: String,
    rounds: i64,
    primary_cost_avg: f64,
    alt_cost_avg: f64,
    savings_pct: f64,
    primary_success_rate: f64,
    alt_success_rate: f64,
    alt_response_ratio: f64,
    primary_avg_seconds: f64,
    alt_avg_seconds: f64,
}

fn load_evidence(conn: &Connection) -> Vec<EvidenceComparison> {
    let mut stmt = match conn.prepare("
        WITH multi_model_rounds AS (
            SELECT war_room_id, war_room_round
            FROM execution_logs
            WHERE war_room_id IS NOT NULL AND war_room_round IS NOT NULL
              AND model IS NOT NULL AND model != ''
            GROUP BY war_room_id, war_room_round
            HAVING COUNT(DISTINCT model) >= 2
        )
        SELECT
            e1.model as primary_model,
            e2.model as alt_model,
            COUNT(*) as rounds,
            AVG(e1.cost_usd_estimated) as primary_cost,
            AVG(e2.cost_usd_estimated) as alt_cost,
            AVG(CASE WHEN e1.status='success' THEN 1.0 ELSE 0.0 END) as primary_success,
            AVG(CASE WHEN e2.status='success' THEN 1.0 ELSE 0.0 END) as alt_success,
            AVG(CAST(LENGTH(e2.response) AS REAL) / MAX(LENGTH(e1.response), 1)) as response_ratio,
            AVG(e1.duration_ms) / 1000.0 as primary_seconds,
            AVG(e2.duration_ms) / 1000.0 as alt_seconds
        FROM execution_logs e1
        JOIN execution_logs e2
            ON e1.war_room_id = e2.war_room_id
            AND e1.war_room_round = e2.war_room_round
            AND e1.model != e2.model
        JOIN multi_model_rounds m
            ON e1.war_room_id = m.war_room_id
            AND e1.war_room_round = m.war_room_round
        WHERE e1.status = 'success' AND e2.status = 'success'
            AND e1.cost_usd_estimated > 0 AND e2.cost_usd_estimated > 0
        GROUP BY e1.model, e2.model
        HAVING COUNT(*) >= 3
        ORDER BY (AVG(e1.cost_usd_estimated) - AVG(e2.cost_usd_estimated)) * COUNT(*) DESC
    ") {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    stmt.query_map([], |row| {
        let primary_cost: f64 = row.get(3)?;
        let alt_cost: f64 = row.get(4)?;
        let savings = if primary_cost > 0.0 { (1.0 - alt_cost / primary_cost) * 100.0 } else { 0.0 };
        Ok(EvidenceComparison {
            primary_model: row.get(0)?,
            alt_model: row.get(1)?,
            rounds: row.get(2)?,
            primary_cost_avg: primary_cost,
            alt_cost_avg: alt_cost,
            savings_pct: savings,
            primary_success_rate: row.get(5)?,
            alt_success_rate: row.get(6)?,
            alt_response_ratio: row.get(7)?,
            primary_avg_seconds: row.get(8)?,
            alt_avg_seconds: row.get(9)?,
        })
    }).unwrap().filter_map(|r| r.ok())
    .filter(|e| e.alt_cost_avg < e.primary_cost_avg)
    .collect()
}

fn print_human_recommend(
    usages: &[ModelUsage],
    recs: &[ModelRecommendation],
    evidence: &[EvidenceComparison],
) {
    println!("═══════════════════════════════════════════════════════════════");
    println!("  ATO Pro — Model Optimization (based on YOUR data)");
    println!("═══════════════════════════════════════════════════════════════\n");

    println!("  Current model usage:");
    for u in usages.iter().take(8) {
        let provider = runtime::provider_for_model(&u.model).unwrap_or("?");
        println!("    {} ({}) — {} runs, ${:.4}/run, ${:.2} total, {:.0}% success",
            u.model, provider, u.runs, u.avg_cost, u.total_cost, u.success_rate * 100.0);
    }
    println!();

    let tier1: Vec<&ModelRecommendation> = recs.iter().filter(|r| r.tier == 1).collect();
    if !tier1.is_empty() {
        println!("  ┌─ Same provider (safest) ─────────────────────────────────┐");
        for r in tier1.iter().take(5) {
            let provider = runtime::provider_for_model(&r.alt_model).unwrap_or("?");
            let alt_cost_est = r.current_avg_cost * (1.0 - r.savings_pct / 100.0);
            let evidence_tag = evidence.iter()
                .find(|e| e.primary_model == r.current_model && e.alt_model == r.alt_model)
                .map(|e| format!("  ✓ {} rounds tested, {:.0}% success", e.rounds, e.alt_success_rate * 100.0))
                .unwrap_or_else(|| "  ⚠ untested — run replays first".to_string());

            println!("    {} → {}  ({})", r.current_model, r.alt_model, provider);
            println!("      save {:.0}%  ${:.4} → ~${:.4}/run  (${:.2} on past {} runs)",
                r.savings_pct, r.current_avg_cost, alt_cost_est,
                r.current_total_cost * r.savings_pct / 100.0, r.current_runs);
            println!("      {}", evidence_tag);
            println!();
        }
        println!("  └─────────────────────────────────────────────────────────┘\n");
    }

    let tier2: Vec<&ModelRecommendation> = recs.iter().filter(|r| r.tier == 2).collect();
    if !tier2.is_empty() {
        println!("  ┌─ Cross-provider (bigger savings, test first) ───────────┐");
        for r in tier2.iter().take(5) {
            let provider = runtime::provider_for_model(&r.alt_model).unwrap_or("?");
            let alt_cost_est = r.current_avg_cost * (1.0 - r.savings_pct / 100.0);
            let evidence_tag = evidence.iter()
                .find(|e| e.primary_model == r.current_model && e.alt_model == r.alt_model)
                .map(|e| format!("  ✓ {} rounds tested, {:.0}% success", e.rounds, e.alt_success_rate * 100.0))
                .unwrap_or_else(|| "  ⚠ untested — run replays first".to_string());

            println!("    {} → {}  ({})", r.current_model, r.alt_model, provider);
            println!("      save {:.0}%  ${:.4} → ~${:.4}/run", r.savings_pct, r.current_avg_cost, alt_cost_est);
            println!("      {}", evidence_tag);
            println!();
        }
        println!("  └─────────────────────────────────────────────────────────┘\n");
    }

    if !evidence.is_empty() {
        println!("  ┌─ Tested head-to-head (highest confidence) ──────────────┐");
        for e in evidence.iter().take(5) {
            let quality = if e.alt_response_ratio > 1.2 {
                format!("{:.0}% more text", (e.alt_response_ratio - 1.0) * 100.0)
            } else if e.alt_response_ratio < 0.8 {
                format!("{:.0}% less text", (1.0 - e.alt_response_ratio) * 100.0)
            } else {
                "comparable output".to_string()
            };
            let speed = if e.alt_avg_seconds < e.primary_avg_seconds * 0.8 {
                format!("{:.0}% faster", (1.0 - e.alt_avg_seconds / e.primary_avg_seconds) * 100.0)
            } else if e.alt_avg_seconds > e.primary_avg_seconds * 1.2 {
                format!("{:.0}% slower", (e.alt_avg_seconds / e.primary_avg_seconds - 1.0) * 100.0)
            } else {
                "similar speed".to_string()
            };
            let confidence = if e.rounds >= 50 { "HIGH" }
                else if e.rounds >= 20 { "MEDIUM" }
                else { "LOW" };

            println!("    {} → {}", e.primary_model, e.alt_model);
            println!("      save {:.0}%  ${:.4} → ${:.4}  {} rounds ({})",
                e.savings_pct, e.primary_cost_avg, e.alt_cost_avg, e.rounds, confidence);
            println!("      quality: {}, {}  success: {:.0}% → {:.0}%",
                quality, speed, e.primary_success_rate * 100.0, e.alt_success_rate * 100.0);
            println!();
        }
        println!("  └─────────────────────────────────────────────────────────┘\n");
    }

    let total_spend: f64 = usages.iter().map(|u| u.total_cost).sum();
    let max_savings_pct = recs.first().map(|r| r.savings_pct).unwrap_or(0.0);
    println!("  Total tracked spend: ${:.2}", total_spend);
    if max_savings_pct > 0.0 {
        println!("  Top opportunity: {:.0}% savings → ~${:.2}/month if applied",
            max_savings_pct, total_spend * max_savings_pct / 100.0 / 4.0);
    }
    println!();
    println!("  Next steps:");
    println!("    ato optimize autotest --agent <slug>   # see replay commands");
    println!("    ato optimize run --human               # execute replays");
    println!("    ato optimize recommend --human          # re-check after replays");
    println!();
    println!("═══════════════════════════════════════════════════════════════");
}

fn print_json_recommend(
    usages: &[ModelUsage],
    recs: &[ModelRecommendation],
    evidence: &[EvidenceComparison],
) {
    let usage_json: Vec<serde_json::Value> = usages.iter().map(|u| {
        serde_json::json!({
            "agent": u.agent_slug, "model": u.model, "runtime": u.runtime,
            "provider": runtime::provider_for_model(&u.model),
            "runs": u.runs, "avg_cost": u.avg_cost, "total_cost": u.total_cost,
            "avg_duration_s": u.avg_duration_s, "success_rate": u.success_rate,
        })
    }).collect();

    let rec_json: Vec<serde_json::Value> = recs.iter().map(|r| {
        serde_json::json!({
            "agent": r.agent_slug, "from_model": r.current_model, "to_model": r.alt_model,
            "tier": r.tier, "savings_pct": (r.savings_pct * 10.0).round() / 10.0,
            "current_avg_cost": r.current_avg_cost, "current_total_cost": r.current_total_cost,
            "current_runs": r.current_runs, "provider": runtime::provider_for_model(&r.alt_model),
        })
    }).collect();

    let evidence_json: Vec<serde_json::Value> = evidence.iter().map(|e| {
        serde_json::json!({
            "from_model": e.primary_model, "to_model": e.alt_model,
            "rounds": e.rounds, "savings_pct": (e.savings_pct * 10.0).round() / 10.0,
            "cost_from": e.primary_cost_avg, "cost_to": e.alt_cost_avg,
            "success_from": e.primary_success_rate, "success_to": e.alt_success_rate,
            "response_ratio": e.alt_response_ratio,
            "speed_from_s": e.primary_avg_seconds, "speed_to_s": e.alt_avg_seconds,
        })
    }).collect();

    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "current_usage": usage_json,
        "recommendations": rec_json,
        "evidence": evidence_json,
    })).unwrap());
}

// ── autotest: show model replay commands ─────────────────────────────

fn handle_autotest(agent: String, db_override: &Option<String>, human: bool) {
    let conn = open_db(db_override);

    let mut stmt = conn.prepare("
        SELECT id, runtime, model, prompt, cost_usd_estimated, duration_ms
        FROM execution_logs
        WHERE agent_slug = ?1 AND status = 'success' AND prompt IS NOT NULL
          AND model IS NOT NULL AND model != ''
        ORDER BY created_at DESC
        LIMIT 5
    ").unwrap_or_else(|e| { eprintln!("Query error: {}", e); std::process::exit(1); });

    let rows: Vec<(String, String, String, String, Option<f64>, Option<i64>)> = stmt
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

    let current_model = &rows[0].2;
    let current_runtime = &rows[0].1;
    let same_provider_alts = runtime::cheaper_same_provider(current_model);
    let cross_provider_alts = runtime::cheaper_cross_provider(current_model);

    if human {
        println!("Agent: {}  (current: {} on {})", agent, current_model, current_runtime);
        println!("Found {} recent prompts\n", rows.len());

        if !same_provider_alts.is_empty() {
            let provider = runtime::provider_for_model(current_model).unwrap_or("?");
            println!("  Same provider ({}) — safest:", provider);
            for alt in &same_provider_alts {
                let (_, out) = runtime::pricing_for_model(alt).unwrap();
                let (_, cur_out) = runtime::pricing_for_model(current_model).unwrap();
                let savings = (1.0 - out / cur_out) * 100.0;
                println!("    {} (save {:.0}%):", alt, savings);
                println!("      ato dispatch {} \"{}\" --model {}",
                    current_runtime, truncate_prompt(&rows[0].3), alt);
            }
            println!();
        }

        if !cross_provider_alts.is_empty() {
            println!("  Cross-provider — bigger savings:");
            for alt in cross_provider_alts.iter().take(3) {
                let provider = runtime::provider_for_model(alt).unwrap_or("?");
                let rt = runtime_slug_for_model(alt);
                let (_, out) = runtime::pricing_for_model(alt).unwrap();
                let (_, cur_out) = runtime::pricing_for_model(current_model).unwrap();
                let savings = (1.0 - out / cur_out) * 100.0;
                println!("    {} ({}, save {:.0}%):", alt, provider, savings);
                println!("      ato dispatch {} \"{}\" --model {}", rt, truncate_prompt(&rows[0].3), alt);
            }
            println!();
        }

        if same_provider_alts.is_empty() && cross_provider_alts.is_empty() {
            println!("  {} is already the cheapest option!", current_model);
        }
        println!("  After replays, run: ato optimize recommend --human");
    } else {
        let mut commands: Vec<serde_json::Value> = Vec::new();
        for (id, rt, _, prompt, _, _) in &rows {
            for alt in &same_provider_alts {
                commands.push(serde_json::json!({
                    "source_id": id, "target_model": alt, "target_runtime": rt, "tier": 1,
                    "command": format!("ato dispatch {} \"{}\" --model {}", rt, escape_prompt(prompt), alt),
                }));
            }
            for alt in cross_provider_alts.iter().take(3) {
                let rt_slug = runtime_slug_for_model(alt);
                commands.push(serde_json::json!({
                    "source_id": id, "target_model": alt, "target_runtime": rt_slug, "tier": 2,
                    "command": format!("ato dispatch {} \"{}\" --model {}", rt_slug, escape_prompt(prompt), alt),
                }));
            }
        }
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "agent": agent, "current_model": current_model, "current_runtime": current_runtime,
            "prompts": rows.len(), "replay_commands": commands,
        })).unwrap());
    }
}

// ── schedule ─────────────────────────────────────────────────────────

fn handle_schedule(agent: Option<String>, intensity: String, frequency: String, token_budget: Option<u32>, db_override: &Option<String>, human: bool) {
    let conn = open_db_rw(db_override);

    let (prompts_per_run, max_models, budget_tokens, tier_name) = match intensity.as_str() {
        "light" => (3u32, 2u32, token_budget.unwrap_or(2000), "Light"),
        "deep" => (10, 99, token_budget.unwrap_or(15000), "Deep"),
        _ => (5, 4, token_budget.unwrap_or(5000), "Normal (recommended)"),
    };

    let agents: Vec<String> = if let Some(ref a) = agent {
        vec![a.clone()]
    } else {
        conn.prepare("SELECT DISTINCT agent_slug FROM execution_logs WHERE agent_slug IS NOT NULL AND agent_slug != '' ORDER BY agent_slug")
            .unwrap().query_map([], |row| row.get(0)).unwrap().filter_map(|r| r.ok()).collect()
    };

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
            rusqlite::params![a, intensity, frequency, prompts_per_run, max_models, budget_tokens],
        ).unwrap_or_else(|e| { eprintln!("Insert error: {}", e); std::process::exit(1); });
    }

    if human {
        println!("Optimization schedule created:\n");
        println!("  Intensity:    {} ({} prompts × {} model alternatives per cycle)", tier_name, prompts_per_run, max_models);
        println!("  Frequency:    {}", frequency);
        println!("  Token budget: {} tokens/cycle (~${:.2})", budget_tokens, budget_tokens as f64 * 0.000025);
        println!("  Agents:       {}", agents.join(", "));
        println!();
        println!("  To run immediately: ato optimize run --human");
    } else {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "agents": agents, "intensity": intensity, "frequency": frequency,
            "prompts_per_run": prompts_per_run, "max_models": max_models,
            "token_budget": budget_tokens,
        })).unwrap());
    }
}

// ── run ──────────────────────────────────────────────────────────────

fn handle_run(_agent: Option<String>, db_override: &Option<String>, human: bool) {
    let conn = open_db(db_override);

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

    if human { println!("Running model optimization tests...\n"); }

    let mut all_replays: Vec<serde_json::Value> = Vec::new();

    for (agent_slug, prompts_n, max_alts, _budget) in &schedules {
        let mut pstmt = conn.prepare(
            "SELECT id, runtime, model, prompt FROM execution_logs
             WHERE agent_slug = ?1 AND status = 'success'
               AND prompt IS NOT NULL AND LENGTH(prompt) > 20
               AND model IS NOT NULL AND model != ''
             ORDER BY created_at DESC LIMIT ?2"
        ).unwrap();

        let prompts: Vec<(String, String, String, String)> = pstmt
            .query_map(rusqlite::params![agent_slug, prompts_n], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            }).unwrap().filter_map(|r| r.ok()).collect();

        if prompts.is_empty() {
            if human { println!("  {} — no prompts with content, skipping", agent_slug); }
            continue;
        }

        let current_model = &prompts[0].2;
        let current_runtime = &prompts[0].1;

        let mut alts: Vec<(&str, &str, u8)> = Vec::new();
        for &m in &runtime::cheaper_same_provider(current_model) {
            alts.push((m, current_runtime, 1));
        }
        for &m in runtime::cheaper_cross_provider(current_model).iter().take(2) {
            alts.push((m, runtime_slug_for_model(m), 2));
        }
        alts.truncate(*max_alts as usize);

        if human {
            println!("  Agent: {}  (current: {} on {})", agent_slug, current_model, current_runtime);
            println!("  Testing {} prompts against {} alternatives:", prompts.len(), alts.len());
            for (alt_model, alt_rt, tier) in &alts {
                let tier_label = if *tier == 1 { "same-provider" } else { "cross-provider" };
                println!("    {} via {} ({})", alt_model, alt_rt, tier_label);
            }
            println!("  Replay commands:");
        }

        for (id, orig_rt, _, prompt) in &prompts {
            for (alt_model, alt_rt, tier) in &alts {
                let cmd = format!("ato dispatch {} \"{}\" --model {}", alt_rt, escape_prompt(prompt), alt_model);
                if human { println!("    {}", cmd); }
                all_replays.push(serde_json::json!({
                    "agent": agent_slug, "source_id": id, "source_runtime": orig_rt,
                    "target_model": alt_model, "target_runtime": alt_rt, "tier": tier,
                    "command": cmd,
                }));
            }
        }
        if human { println!(); }
    }

    if human {
        println!("  Total replays: {}", all_replays.len());
        println!("  To execute: ato optimize run | jq -r '.replays[].command' | sh");
        println!("  After replays: ato optimize recommend --human");
    } else {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "replays": all_replays, "total": all_replays.len(),
        })).unwrap());
    }
}

// ── judge: call compare-judge on two execution log entries ───────────

fn cloud_base() -> String {
    std::env::var("ATO_CLOUD_URL")
        .unwrap_or_else(|_| "https://api.agentictool.ai".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn read_token() -> Option<String> {
    crate::commands::auth::ensure_fresh_token();
    let path = crate::db::home_dir().join(".ato").join("auth.json");
    let contents = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    json.get("token")?.as_str().map(String::from)
}

fn require_token() -> String {
    read_token().unwrap_or_else(|| {
        eprintln!("Not logged in. Run: ato login");
        std::process::exit(1);
    })
}

fn handle_judge(id_a: String, id_b: String, db_override: &Option<String>, human: bool) {
    let conn = open_db(db_override);
    let token = require_token();

    let load = |id: &str| -> Option<(String, String, String, Option<f64>, Option<i64>)> {
        conn.query_row(
            "SELECT runtime, COALESCE(model,''), COALESCE(prompt,''), COALESCE(response,''),
                    cost_usd_estimated, duration_ms
             FROM execution_logs WHERE id = ?1",
            [id],
            |row| {
                let rt: String = row.get(0)?;
                let model: String = row.get(1)?;
                let prompt: String = row.get(2)?;
                let resp: String = row.get(3)?;
                let cost: Option<f64> = row.get(4)?;
                let dur: Option<i64> = row.get(5)?;
                Ok((format!("{}/{}", rt, model), prompt, resp, cost, dur))
            },
        ).ok()
    };

    let (label_a, prompt_a, response_a, cost_a, dur_a) = load(&id_a).unwrap_or_else(|| {
        eprintln!("Execution log {} not found", id_a);
        std::process::exit(1);
    });
    let (label_b, _prompt_b, response_b, cost_b, dur_b) = load(&id_b).unwrap_or_else(|| {
        eprintln!("Execution log {} not found", id_b);
        std::process::exit(1);
    });

    if human {
        println!("Comparing:\n  A: {} (id: {})\n  B: {} (id: {})\n", label_a, id_a, label_b, id_b);
        println!("Calling compare-judge...");
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();

    let body = serde_json::json!({
        "prompt": prompt_a,
        "responseA": { "runtime": label_a, "text": response_a, "costUsd": cost_a, "durationMs": dur_a },
        "responseB": { "runtime": label_b, "text": response_b, "costUsd": cost_b, "durationMs": dur_b },
    });

    let resp = client
        .post(format!("{}/api/compare-judge", cloud_base()))
        .bearer_auth(&token)
        .json(&body)
        .send();

    match resp {
        Ok(r) if r.status().is_success() => {
            let envelope: serde_json::Value = r.json().unwrap_or_default();
            let result = envelope.get("data").unwrap_or(&envelope);
            if human {
                let winner = result.get("winner").and_then(|v| v.as_str()).unwrap_or("?");
                let reason = result.get("reason").and_then(|v| v.as_str()).unwrap_or("?");
                let score_a = result.pointer("/scoreA/overall").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let score_b = result.pointer("/scoreB/overall").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let savings_pct = result.pointer("/costAnalysis/savingsPct")
                    .and_then(|v| v.as_f64()).unwrap_or(0.0);

                println!();
                println!("  Result: {}", match winner {
                    "equivalent" => "EQUIVALENT quality",
                    "a_better" => "A is better",
                    "b_better" => "B is better",
                    _ => winner,
                });
                println!("  Scores:  A={:.2}  B={:.2}", score_a, score_b);
                println!("  Reason:  {}", reason);
                if savings_pct > 0.0 {
                    println!("  Savings: {:.0}% if you switch to B", savings_pct);
                }
                println!();
                println!("  Judge model: {}", result.get("judgeModel").and_then(|v| v.as_str()).unwrap_or("?"));
            } else {
                println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
            }
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().unwrap_or_default();
            eprintln!("Compare-judge failed ({}): {}", status, body);
            if status.as_u16() == 403 { eprintln!("This is a Pro feature. Run: ato pro"); }
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Network error: {}", e);
            std::process::exit(1);
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn truncate_prompt(prompt: &str) -> String {
    let clean = prompt.replace('\n', " ").replace('\r', "");
    if clean.len() > 60 { format!("{}...", &clean[..57]) } else { clean }
}

fn escape_prompt(prompt: &str) -> String {
    prompt.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

fn runtime_slug_for_model(model: &str) -> &'static str {
    match runtime::provider_for_model(model) {
        Some("anthropic") => "claude",
        Some("openai") => "codex",
        Some("google") => "gemini",
        Some("minimax") => "minimax",
        Some("deepseek") => "deepseek",
        Some("qwen") => "qwen",
        Some("grok") => "grok",
        Some("zai") => "zai",
        _ => "claude",
    }
}

// ── CLI interface ────────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum CostRecommendCommand {
    /// Analyze model usage and recommend cheaper alternatives.
    Recommend,
    /// Show replay commands to test an agent against alternative models.
    Autotest {
        #[arg(long)]
        agent: String,
    },
    /// Set up recurring optimization tests.
    Schedule {
        #[arg(long)]
        agent: Option<String>,
        #[arg(long, default_value = "normal")]
        intensity: String,
        #[arg(long, default_value = "weekly")]
        frequency: String,
        #[arg(long)]
        token_budget: Option<u32>,
    },
    /// Run optimization tests now (generates replay commands).
    Run {
        #[arg(long)]
        agent: Option<String>,
    },
    /// Compare two dispatches via the cloud compare-judge (Pro).
    Judge {
        /// ID of the first execution log entry
        id_a: String,
        /// ID of the second execution log entry
        id_b: String,
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
        CostRecommendCommand::Judge { id_a, id_b } => handle_judge(id_a, id_b, db_override, human),
    }
}
