// `ato demo-compare` — zero-config first-run demo of the compare-runtimes wedge.
//
// Designed per the 2026-05-16 Devex seat spec in the gstack war-room
// (Round 3 of PMF-DELIVERABLES-2026-05-16.md). Runs the same prompt
// through two runtimes, prints a unified cost-comparison table, ends
// with a "next step" line pointing at `ato dispatch`. Must always
// produce output — even with zero API keys configured — so the demo
// never errors out on first launch.
//
// Runtime resolution falls back in three tiers:
//   1. User's configured API providers (any row in llm_api_keys with
//      is_active=1 that maps to ato_api_providers::registry()).
//   2. Local Ollama on localhost:11434 (HEAD probe).
//   3. Two simulated stub responses ("offline demo" mode) — keeps
//      first-launch from ever feeling broken.
//
// Why subprocess-of-self for the actual dispatches: dogfoods the full
// `ato dispatch` stack (BYOK + encryption + execution_logs + cost
// rollup) without duplicating it here. Each dispatch lands as a real
// row in execution_logs, so the user sees the receipts in History
// immediately after the demo completes.

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// Per-dispatch ceiling. A first-launch demo must never hang waiting on
// a slow provider; 45s is generous for fast providers (typically 2-8s)
// but firmly caps the latent failure modes (slow upstream, network drop,
// DNS misery). On timeout we kill the child and surface the partial.
const DISPATCH_TIMEOUT: Duration = Duration::from_secs(45);

// Tier 1a ordering — fast, reliable providers preferred when multiple
// keys are configured. Anything not in this list still runs in Tier 1b
// (insertion order from the registry). Order is empirical not dogmatic;
// flip it as provider latencies move.
const FAST_PROVIDERS: &[&str] = &["google", "anthropic", "openai", "openrouter", "grok"];

const DEMO_PROMPT: &str = "Explain merge sort in Python in under 4 sentences. Include time complexity.";

#[derive(Debug, Serialize)]
pub struct DemoResult {
    pub prompt: String,
    pub runtimes_used: Vec<String>,
    pub source: &'static str, // "configured" | "ollama" | "stub"
    pub rows: Vec<DemoRow>,
}

#[derive(Debug, Serialize)]
pub struct DemoRow {
    pub runtime: String,
    pub model: Option<String>,
    pub response_preview: String,
    pub status: String,
    pub duration_ms: Option<i64>,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub cost_usd_estimated: Option<f64>,
    pub run_id: Option<String>,
}

/// Public entrypoint wired from main.rs.
pub fn run(db_path: &PathBuf, prompt: Option<String>, runtimes_override: Option<String>, opts: &Opts) -> Result<()> {
    let prompt = prompt.unwrap_or_else(|| DEMO_PROMPT.to_string());

    let (runtimes, source) = resolve_runtimes(db_path, runtimes_override)?;

    if opts.human {
        emit_human(&format!("Comparing {:?}...", &prompt));
        emit_human(&format!("Runtimes: {} (source: {})", runtimes.join(", "), source));
        emit_human("");
    }

    let mut rows = Vec::with_capacity(runtimes.len());
    for rt in &runtimes {
        // Stub runtimes are recognized by source OR by name prefix so
        // `--runtimes stub-claude,stub-codex` works for offline testing.
        let row = if source == "stub" || rt.starts_with("stub") {
            stub_row(rt)
        } else {
            dispatch_real(rt, &prompt, opts.human).unwrap_or_else(|e| DemoRow {
                runtime: rt.clone(),
                model: None,
                response_preview: format!("(dispatch failed: {})", truncate(&e.to_string(), 100)),
                status: "error".into(),
                duration_ms: None,
                tokens_in: None,
                tokens_out: None,
                cost_usd_estimated: None,
                run_id: None,
            })
        };
        if opts.human {
            print_row_preview(&row);
        }
        rows.push(row);
    }

    if opts.human {
        emit_human("");
        print_comparison_table(&rows);
        emit_human("");
        emit_human(&format!(
            "Next step: `ato dispatch {} \"<your prompt here>\"` to run a real workflow.",
            rows.first().map(|r| r.runtime.as_str()).unwrap_or("claude")
        ));
        if source == "stub" {
            emit_human("");
            emit_human("Note: this run used stubbed responses (no API keys configured, no Ollama).");
            emit_human("Add a key in Settings → API Keys to see real model output and cost numbers.");
        }
    } else {
        emit_json(&DemoResult {
            prompt,
            runtimes_used: runtimes.clone(),
            source,
            rows,
        })?;
    }

    Ok(())
}

/// Pick exactly two runtimes from the three-tier fallback ladder.
fn resolve_runtimes(
    db_path: &PathBuf,
    override_list: Option<String>,
) -> Result<(Vec<String>, &'static str)> {
    // Explicit override always wins.
    if let Some(s) = override_list {
        let rts: Vec<String> = s
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        if !rts.is_empty() {
            return Ok((rts, "override"));
        }
    }

    // Tier 1: configured API providers (active llm_api_keys rows).
    let configured = configured_api_runtimes(db_path)?;
    if configured.len() >= 2 {
        return Ok((configured.into_iter().take(2).collect(), "configured"));
    }

    // Tier 2: one configured + Ollama, OR two-Ollama (Ollama can serve
    // multiple models, but the demo wants two visibly distinct rows,
    // so we treat Ollama as one slot).
    let ollama_up = ollama_reachable();
    if configured.len() == 1 && ollama_up {
        let mut v = configured;
        v.push("ollama".into());
        return Ok((v, "ollama"));
    }
    if ollama_up {
        // No keys at all but Ollama is up — pair Ollama with stub so the
        // user still sees a comparison table (the contrast is the point).
        return Ok((vec!["ollama".into(), "stub".into()], "ollama"));
    }

    // Tier 3: two stubs.
    Ok((vec!["stub-claude".into(), "stub-codex".into()], "stub"))
}

fn configured_api_runtimes(db_path: &PathBuf) -> Result<Vec<String>> {
    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open {} read-only", db_path.display()))?;

    let mut configured = Vec::new();
    for p in ato_api_providers::registry() {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM llm_api_keys WHERE LOWER(provider) = LOWER(?1) AND is_active = 1",
                [p.slug],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if n > 0 {
            configured.push(p.slug.to_string());
        }
    }
    // Bring FAST_PROVIDERS to the front, keep stable relative order
    // among the rest. The demo wants fast-and-reliable first; slower
    // providers stay reachable via --runtimes override.
    configured.sort_by_key(|name| {
        FAST_PROVIDERS
            .iter()
            .position(|f| f.eq_ignore_ascii_case(name))
            .unwrap_or(usize::MAX / 2)
    });
    Ok(configured)
}

fn ollama_reachable() -> bool {
    // Cheap GET against /api/version — present on every Ollama install.
    // Tight timeout so first-launch demo never stalls when Ollama isn't there.
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(300))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    matches!(client.get("http://localhost:11434/api/version").send(), Ok(r) if r.status().is_success())
}

/// Invoke `ato dispatch <runtime> <prompt>` as a subprocess of self
/// and parse the JSON outcome. Keeps the full dispatch contract intact
/// (BYOK auth, encrypted keys, execution_logs persistence, cost rollup).
///
/// Hard-capped at DISPATCH_TIMEOUT so a slow upstream never strands
/// the first-launch demo at "(running)…" forever.
fn dispatch_real(runtime: &str, prompt: &str, human_status: bool) -> Result<DemoRow> {
    if human_status {
        emit_human(&format!("> {} (running, max {}s)…", runtime, DISPATCH_TIMEOUT.as_secs()));
    }

    let self_exe = std::env::current_exe().context("locate own binary for subprocess dispatch")?;

    let mut child = Command::new(&self_exe)
        .arg("dispatch")
        .arg(runtime)
        .arg(prompt)
        .arg("--quiet")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn `ato dispatch {}`", runtime))?;

    // Manual timeout — std::process::Child has no built-in wait_timeout.
    // Poll try_wait() with a short sleep; kill on overrun. Simple and
    // adds no deps.
    let start = Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(s) => break s,
            None => {
                if start.elapsed() > DISPATCH_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!("timed out after {}s", DISPATCH_TIMEOUT.as_secs());
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        }
    };

    let mut stdout_buf = String::new();
    if let Some(mut s) = child.stdout.take() {
        s.read_to_string(&mut stdout_buf).ok();
    }
    let mut stderr_buf = String::new();
    if let Some(mut s) = child.stderr.take() {
        s.read_to_string(&mut stderr_buf).ok();
    }

    if !status.success() {
        anyhow::bail!("exit {}: {}", status, truncate(stderr_buf.trim(), 120));
    }

    let parsed: DispatchJson = serde_json::from_str(&stdout_buf).with_context(|| {
        format!("parse dispatch JSON for {}: {}", runtime, truncate(&stdout_buf, 200))
    })?;

    Ok(DemoRow {
        runtime: runtime.to_string(),
        model: parsed.model,
        response_preview: truncate(&parsed.response.unwrap_or_default(), 240),
        status: parsed.status,
        duration_ms: parsed.duration_ms,
        tokens_in: parsed.tokens_in,
        tokens_out: parsed.tokens_out,
        cost_usd_estimated: parsed.cost_usd_estimated,
        run_id: Some(parsed.id),
    })
}

fn stub_row(label: &str) -> DemoRow {
    let preview = match label {
        s if s.contains("codex") => "Recursive divide-and-conquer: split list until single elements, then merge sorted pairs. Stable and predictable. Time complexity O(n log n); space O(n).",
        _ => "Splits the list recursively, merging sorted sub-lists on the way back up. Elegant and efficient. Time complexity O(n log n); auxiliary space O(n).",
    };
    DemoRow {
        runtime: label.to_string(),
        model: Some("stub".into()),
        response_preview: preview.to_string(),
        status: "success".into(),
        duration_ms: Some(0),
        tokens_in: Some(48),
        tokens_out: Some(28),
        cost_usd_estimated: Some(0.0),
        run_id: None,
    }
}

fn print_row_preview(row: &DemoRow) {
    let badge = if row.status == "success" { "✓" } else { "✗" };
    emit_human(&format!("> {} {}", row.runtime, badge));
    for line in row.response_preview.lines().take(3) {
        emit_human(&format!("  {}", line));
    }
    emit_human("");
}

fn print_comparison_table(rows: &[DemoRow]) {
    let sep = "-".repeat(64);
    emit_human(&sep);
    emit_human(&format!(
        "| {:<14} | {:>10} | {:>10} | {:>10} | {:>8} |",
        "Runtime", "Duration", "Tokens in", "Tokens out", "Cost"
    ));
    emit_human(&sep);
    for r in rows {
        let dur = r
            .duration_ms
            .map(|d| format!("{:.1}s", d as f64 / 1000.0))
            .unwrap_or_else(|| "—".into());
        let ti = r
            .tokens_in
            .map(|t| t.to_string())
            .unwrap_or_else(|| "—".into());
        let to = r
            .tokens_out
            .map(|t| t.to_string())
            .unwrap_or_else(|| "—".into());
        let cost = r
            .cost_usd_estimated
            .map(|c| format!("${:.4}", c))
            .unwrap_or_else(|| "—".into());
        emit_human(&format!(
            "| {:<14} | {:>10} | {:>10} | {:>10} | {:>8} |",
            truncate(&r.runtime, 14),
            dur,
            ti,
            to,
            cost
        ));
    }
    emit_human(&sep);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", head)
    }
}

/// The subset of the dispatch JSON we care about. Captures just the
/// fields shown in the demo table; ignores everything else dispatch.rs
/// emits so this doesn't break when the dispatch shape grows.
#[derive(Debug, Deserialize)]
struct DispatchJson {
    id: String,
    status: String,
    #[serde(default)]
    response: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    duration_ms: Option<i64>,
    #[serde(default)]
    tokens_in: Option<i64>,
    #[serde(default)]
    tokens_out: Option<i64>,
    #[serde(default)]
    cost_usd_estimated: Option<f64>,
}
