// `ato bench run` — the open-box benchmark in one command.
//
// Your keys → your models → a VERIFIABLE grader → a REPRODUCIBLE receipt. Given
// a pinned LiveCodeBench JSONL file (which you fetch-and-pin yourself — we never
// vendor the corpus) and a list of models you have keys for, this dispatches
// each problem to each model at temperature 0, runs the model's code in a
// sandbox against the test cases, and prints a transparent scorecard: pass-rate
// ± Wilson CI, cost, latency, tokens, plus the dataset / harness / exec-env
// hashes that make a re-run verifiable.
//
// OSS/FREE: running public benchmarks (or your own) with your own keys. No tier
// gate. Curated premium suites + continuous auto-routing stay Pro (ato-cloud).
//
// Orchestration reuses the proven runner pattern: shell out to `ato dispatch`
// (so we inherit key resolution, retries, accounting, and the keychain ACL) and
// parse its JSON receipt. Temperature is passed to the child via the
// ATO_DISPATCH_TEMPERATURE env var.

use anyhow::{anyhow, bail, Context, Result};
use ato_bench::{
    all_cutoffs, cutoff_for_model, grade_problem, import_lcb_jsonl, is_parseable_cutoff,
    select_sandbox, CutoffOrigin, DatasetSnapshot, ExecEnv, ExecLimits, GraderConfig,
    HarnessConfig, LcbImportOptions, ModelCutoffInfo, RunContext, Sampling, SandboxOptions,
    Scorecard, Z_95,
};
use clap::Args;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::Command;

const PROMPT_WRAPPER: &str = "You are solving a competitive programming problem. Write a COMPLETE \
Python 3 program that reads ALL input from standard input and writes the answer to standard output. \
Do not define a function to be called externally — read from stdin and print to stdout. Respond with \
ONLY the program inside a single ```python code block.\n\nProblem:\n{{problem}}";

#[derive(Args, Debug)]
pub struct BenchArgs {
    #[command(subcommand)]
    pub cmd: BenchCmd,
}

#[derive(clap::Subcommand, Debug)]
pub enum BenchCmd {
    /// Run a benchmark over a pinned dataset across the given models.
    Run(RunArgs),
    /// List the vendor-stated training-cutoff registry (with sources) used to
    /// classify contamination.
    Cutoffs,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Path to a pinned LiveCodeBench JSONL file. Fetch-and-pin it yourself,
    /// e.g. `curl -sL https://huggingface.co/datasets/livecodebench/\
    /// code_generation_lite/resolve/<revision>/test.jsonl -o test.jsonl`.
    #[arg(long)]
    dataset_file: PathBuf,

    /// If set and `--dataset-file` does not exist, download this URL to it first.
    #[arg(long)]
    fetch_url: Option<String>,

    /// Comma-separated model ids you have keys for (e.g.
    /// "gemini-3-flash-preview,claude-sonnet-4-6").
    #[arg(long)]
    models: String,

    /// Upstream LCB version tag, recorded on the scorecard (e.g. release_v6).
    #[arg(long)]
    lcb_version: Option<String>,

    /// Upstream HF commit revision, recorded on the scorecard for pinning.
    #[arg(long)]
    lcb_revision: Option<String>,

    /// Cap the number of problems (for a quick run). Default: all imported.
    #[arg(long)]
    limit: Option<usize>,

    /// Sampling temperature (0 = most deterministic; the reproducibility default).
    #[arg(long, default_value_t = 0.0)]
    temperature: f64,

    /// Per-test wall-clock limit in milliseconds.
    #[arg(long, default_value_t = 10_000)]
    wall_ms: u64,

    /// Write the full scorecard JSON (the receipt) to this path.
    #[arg(long)]
    out: Option<PathBuf>,

    /// Allow running model code WITHOUT network isolation if no sandbox is
    /// available (unsafe; only for code you trust).
    #[arg(long, default_value_t = false)]
    allow_unsandboxed: bool,

    /// Override or supply a model's training cutoff for contamination
    /// classification, as MODEL=DATE (date: YYYY, YYYY-MM, or YYYY-MM-DD).
    /// Repeatable. Beats the built-in vendor-stated registry (`ato bench
    /// cutoffs` lists it).
    #[arg(long = "model-cutoff", value_name = "MODEL=DATE")]
    model_cutoffs: Vec<String>,
}

/// The JSON receipt `ato dispatch` prints to stdout.
#[derive(Debug, Deserialize)]
struct DispatchReceipt {
    #[serde(default)]
    response: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    cost_usd_estimated: Option<f64>,
    #[serde(default)]
    tokens_in: Option<i64>,
    #[serde(default)]
    tokens_out: Option<i64>,
    #[serde(default)]
    duration_ms: Option<i64>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
}

/// Per-task model-call accounting (separate from the grader's TaskReceipt).
#[derive(Default)]
struct DispatchTotals {
    cost_usd: f64,
    tokens_in: i64,
    tokens_out: i64,
    dispatch_ms: i64,
    dispatch_errors: usize,
}

pub fn run(args: BenchArgs, db_path: &str) -> Result<()> {
    match args.cmd {
        BenchCmd::Run(a) => run_bench(a, db_path),
        BenchCmd::Cutoffs => list_cutoffs(),
    }
}

/// `ato bench cutoffs` — print the registry so the contamination gate is
/// auditable without reading source. Data, not judgment: every row is a
/// vendor-stated date with its source URL.
fn list_cutoffs() -> Result<()> {
    let entries = all_cutoffs();
    println!("Vendor-stated training cutoffs used for contamination classification.");
    println!("Models not listed classify as 'unknown' (pass --model-cutoff to supply one).\n");
    for e in entries {
        println!(
            "{:<28} {:<10} {:<13} verified {}  {}",
            e.model,
            e.cutoff,
            format!("({})", e.kind.as_str()),
            e.verified,
            e.source
        );
    }
    println!("\n{} models in registry.", entries.len());
    Ok(())
}

/// Parse repeated `--model-cutoff MODEL=DATE` flags. Duplicate models or
/// unparseable dates are hard errors — a silent fallback to Unknown would
/// defeat the point of supplying a cutoff.
fn parse_cutoff_overrides(
    pairs: &[String],
) -> Result<std::collections::HashMap<String, String>> {
    let mut map = std::collections::HashMap::new();
    for pair in pairs {
        let (model, date) = pair
            .split_once('=')
            .ok_or_else(|| anyhow!("--model-cutoff '{pair}' is not MODEL=DATE"))?;
        let (model, date) = (model.trim(), date.trim());
        if model.is_empty() {
            bail!("--model-cutoff '{pair}' has an empty model id");
        }
        if !is_parseable_cutoff(date) {
            bail!(
                "--model-cutoff '{pair}': date '{date}' is not YYYY, YYYY-MM, or YYYY-MM-DD"
            );
        }
        if map.insert(model.to_string(), date.to_string()).is_some() {
            bail!("--model-cutoff given twice for model '{model}'");
        }
    }
    Ok(map)
}

/// Resolve the cutoff for one model: an explicit user override beats the
/// built-in registry; neither → None (classifies Unknown, honestly).
fn resolve_cutoff(
    model: &str,
    overrides: &std::collections::HashMap<String, String>,
) -> Option<ModelCutoffInfo> {
    if let Some(date) = overrides.get(model) {
        return Some(ModelCutoffInfo {
            cutoff: date.clone(),
            kind: String::new(),
            source: String::new(),
            origin: CutoffOrigin::User,
        });
    }
    cutoff_for_model(model).map(|e| ModelCutoffInfo {
        cutoff: e.cutoff.to_string(),
        kind: e.kind.as_str().to_string(),
        source: e.source.to_string(),
        origin: CutoffOrigin::Registry,
    })
}

fn run_bench(args: RunArgs, db_path: &str) -> Result<()> {
    // 0. Validate cutoff overrides up front — a typo'd date must fail before
    // any network or model spend.
    let cutoff_overrides = parse_cutoff_overrides(&args.model_cutoffs)?;

    // 1. Fetch-and-pin (optional) then load the dataset bytes.
    if !args.dataset_file.exists() {
        if let Some(url) = &args.fetch_url {
            eprintln!("Fetching {url} → {}", args.dataset_file.display());
            let bytes = http_get(url)?;
            std::fs::write(&args.dataset_file, &bytes)
                .with_context(|| format!("write {}", args.dataset_file.display()))?;
        } else {
            bail!(
                "dataset file {} not found (pass --fetch-url to download it)",
                args.dataset_file.display()
            );
        }
    }
    let bytes = std::fs::read(&args.dataset_file)
        .with_context(|| format!("read {}", args.dataset_file.display()))?;
    let content_sha = format!("sha256:{}", hex(&Sha256::digest(&bytes)));

    // 2. Import (faithful; technical stdin-only filter with transparent counts).
    let imported = import_lcb_jsonl(&bytes, &LcbImportOptions::default());
    eprintln!(
        "Imported {} problems ({} functional excluded, {} decode errors) from {} records",
        imported.problems.len(),
        imported.excluded_functional,
        imported.decode_errors.len(),
        imported.total_records
    );
    let mut problems = imported.problems;
    if let Some(n) = args.limit {
        problems.truncate(n);
    }
    if problems.is_empty() {
        bail!(
            "no runnable problems imported from {}",
            args.dataset_file.display()
        );
    }

    // 3. Sandbox (Docker preferred; macOS seatbelt fallback; unconfined opt-in).
    let sandbox = select_sandbox(&SandboxOptions {
        allow_unconfined: args.allow_unsandboxed,
        ..Default::default()
    })
    .map_err(|e| anyhow!("{e}"))?;
    let sandbox_backend = sandbox.report().backend.clone();
    eprintln!(
        "Sandbox: {} (network_isolated={})",
        sandbox_backend,
        sandbox.report().network_isolated
    );

    let exe = crate::cli_path::resolve_ato_binary().context("locate ato binary")?;
    let run_date = chrono::Utc::now().to_rfc3339();
    let models: Vec<String> = args
        .models
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if models.is_empty() {
        bail!("no models given (--models)");
    }
    // Every override must name a model in this run. A typo'd key would
    // otherwise be silently ignored while the registry (or nothing) decides
    // contamination — the operator would believe their stricter cutoff was in
    // force when it wasn't.
    for key in cutoff_overrides.keys() {
        if !models.iter().any(|m| m == key) {
            bail!(
                "--model-cutoff given for '{key}', which is not in --models ({})",
                models.join(",")
            );
        }
    }

    let harness = HarnessConfig {
        system_prompt: None,
        prompt_wrapper: PROMPT_WRAPPER.to_string(),
        stop_tokens: Vec::new(),
        sampling: Sampling {
            temperature: args.temperature,
            top_p: None,
            max_tokens: None,
            seed: None,
            attempts: 1,
        },
        grader: GraderConfig {
            limits: ExecLimits {
                wall_ms: args.wall_ms,
                ..ExecLimits::default()
            },
            ..GraderConfig::default()
        },
    };
    let env = ExecEnv {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        sandbox_backend,
        runtime_version: python_version(),
    };

    let mut scorecards = Vec::new();
    for model in &models {
        let provider = ato_pricing::provider_for_model(model)
            .ok_or_else(|| {
                anyhow!("unknown provider for model '{model}' — is it in the pricing registry?")
            })?
            .to_string();
        let cutoff_info = resolve_cutoff(model, &cutoff_overrides);
        eprintln!(
            "\n=== Benchmarking {model} ({provider}) over {} problems at temperature {} ===",
            problems.len(),
            args.temperature
        );
        match &cutoff_info {
            Some(c) => eprintln!(
                "Training cutoff: {} ({})",
                c.cutoff,
                match c.origin {
                    CutoffOrigin::User => "user-supplied".to_string(),
                    CutoffOrigin::Registry => format!("vendor-stated {}", c.kind),
                }
            ),
            None => eprintln!(
                "Training cutoff: unknown — not in the registry (`ato bench cutoffs`); \
                 contamination will classify 'unknown'. Pass --model-cutoff {model}=<date> to supply one."
            ),
        }

        let mut receipts = Vec::with_capacity(problems.len());
        let mut totals = DispatchTotals::default();
        for (i, problem) in problems.iter().enumerate() {
            let prompt = PROMPT_WRAPPER.replace("{{problem}}", &problem.prompt);
            let wr_id = uuid::Uuid::new_v4().to_string();
            let (response, revision) = match dispatch_one(
                &exe,
                &provider,
                model,
                &prompt,
                db_path,
                args.temperature,
                &wr_id,
            ) {
                Ok(rec) => {
                    totals.cost_usd += rec.cost_usd_estimated.unwrap_or(0.0);
                    totals.tokens_in += rec.tokens_in.unwrap_or(0);
                    totals.tokens_out += rec.tokens_out.unwrap_or(0);
                    totals.dispatch_ms += rec.duration_ms.unwrap_or(0);
                    if rec.status.as_deref() != Some("success") {
                        totals.dispatch_errors += 1;
                    }
                    (rec.response.unwrap_or_default(), rec.model)
                }
                Err(e) => {
                    totals.dispatch_errors += 1;
                    eprintln!("  [{}/{}] dispatch error: {e}", i + 1, problems.len());
                    (String::new(), None)
                }
            };
            let ctx = RunContext {
                model: model.clone(),
                provider: provider.clone(),
                model_revision: revision,
                model_cutoff: cutoff_info.as_ref().map(|c| c.cutoff.clone()),
                sampling: harness.sampling.clone(),
            };
            let receipt =
                grade_problem(problem, &response, &ctx, &harness.grader, sandbox.as_ref());
            eprintln!(
                "  [{}/{}] {} → {}",
                i + 1,
                problems.len(),
                problem.id,
                if receipt.pass { "PASS" } else { "fail" }
            );
            receipts.push(receipt);
        }

        let dataset = DatasetSnapshot {
            source: "livecodebench".to_string(),
            version_tag: args.lcb_version.clone(),
            revision: args
                .lcb_revision
                .clone()
                .or_else(|| Some(content_sha.clone())),
            task_ids: problems.iter().map(|p| p.id.clone()).collect(),
            run_date: run_date.clone(),
        };
        let sc = Scorecard {
            model: model.clone(),
            provider: provider.clone(),
            dataset,
            harness: harness.clone(),
            env: env.clone(),
            model_cutoff: cutoff_info.clone(),
            receipts,
        };
        print_scorecard(&sc, &totals);
        scorecards.push(sc);
    }

    if let Some(out) = &args.out {
        let json = serde_json::to_string_pretty(&scorecards)?;
        std::fs::write(out, json).with_context(|| format!("write {}", out.display()))?;
        eprintln!("\nReceipt written to {}", out.display());
    }
    Ok(())
}

fn dispatch_one(
    exe: &std::path::Path,
    runtime: &str,
    model: &str,
    prompt: &str,
    db_path: &str,
    temperature: f64,
    wr_id: &str,
) -> Result<DispatchReceipt> {
    let out = Command::new(exe)
        .arg("dispatch")
        .arg(runtime)
        .arg(prompt)
        .arg("--model")
        .arg(model)
        .arg("--war-room-id")
        .arg(wr_id)
        .arg("--db")
        .arg(db_path)
        .env("ATO_DISPATCH_TEMPERATURE", format!("{temperature}"))
        .output()
        .with_context(|| format!("spawn `ato dispatch {runtime}`"))?;
    if !out.status.success() {
        bail!(
            "dispatch exited {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let start = stdout
        .find('{')
        .ok_or_else(|| anyhow!("no JSON receipt in dispatch output"))?;
    // Parse the first JSON value and ignore any trailing text.
    let mut de = serde_json::Deserializer::from_str(&stdout[start..]);
    let rec = DispatchReceipt::deserialize(&mut de).context("parse dispatch receipt")?;
    if let Some(err) = &rec.error_message {
        if rec.response.as_deref().unwrap_or("").is_empty() {
            bail!("dispatch reported error: {err}");
        }
    }
    Ok(rec)
}

fn print_scorecard(sc: &Scorecard, totals: &DispatchTotals) {
    let clean = sc.clean_pass_rate(Z_95);
    let all = sc.pass_rate(Z_95);
    let cs = sc.contamination_summary();
    let n = sc.receipts.len();
    println!("\n──────── OPEN-BOX SCORECARD ────────");
    println!("model:     {} ({})", sc.model, sc.provider);
    match &sc.model_cutoff {
        Some(c) => match c.origin {
            CutoffOrigin::User => println!("cutoff:    {} (user-supplied)", c.cutoff),
            CutoffOrigin::Registry => println!(
                "cutoff:    {} (vendor-stated {})  source: {}",
                c.cutoff, c.kind, c.source
            ),
        },
        None => println!(
            "cutoff:    unknown — not in registry; contamination classifies 'unknown' \
             (`ato bench cutoffs`, or --model-cutoff to supply)"
        ),
    }
    println!("problems:  {n}");
    if clean.n == 0 {
        // Don't render a bare "0.0%" a skimmer could misread as a 0% score —
        // there are simply no contamination-clean SCORABLE tasks. (Clean tasks
        // can exist yet be unscorable — malformed problems are dataset defects
        // excluded from every denominator — so say "scorable", precisely.)
        println!(
            "pass-rate (contamination-clean, headline): n/a  (no contamination-clean scorable tasks — see all-scorable below)"
        );
    } else {
        println!(
            "pass-rate (contamination-clean, headline): {:.1}%  95% CI [{:.1}%, {:.1}%]  ({}/{} tasks)",
            clean.point * 100.0,
            clean.low * 100.0,
            clean.high * 100.0,
            clean.passes,
            clean.n
        );
    }
    println!(
        "pass-rate (all scorable):                  {:.1}%  95% CI [{:.1}%, {:.1}%]  ({}/{} tasks)",
        all.point * 100.0,
        all.low * 100.0,
        all.high * 100.0,
        all.passes,
        all.n
    );
    println!(
        "contamination: clean={} predates={} unknown={}{}",
        cs.clean,
        cs.predates,
        cs.unknown,
        if cs.has_overlap() {
            "  ⚠ some tasks predate the model's cutoff — headline uses clean-only"
        } else {
            ""
        }
    );
    if cs.clean == 0 {
        if sc.model_cutoff.is_none() {
            println!("  note: no cutoff known for this model → all tasks 'unknown'; headline is n/a.");
        } else {
            println!("  note: zero tasks post-date the cutoff → nothing is contamination-clean; headline is n/a.");
        }
    } else if clean.n == 0 {
        // Post-cutoff tasks exist but every one is unscorable (malformed /
        // no-oracle problems) — a dataset defect, not a model outcome. Say so
        // instead of letting "n/a" read as a contamination problem.
        println!(
            "  note: {} contamination-clean task(s) exist but none are scorable (malformed problems — dataset defect); headline is n/a.",
            cs.clean
        );
    }
    println!(
        "cost: ${:.4}   latency: {} ms total ({} ms/task avg)   tokens: {} in / {} out   dispatch_errors: {}",
        totals.cost_usd,
        totals.dispatch_ms,
        if n > 0 { totals.dispatch_ms / n as i64 } else { 0 },
        totals.tokens_in,
        totals.tokens_out,
        totals.dispatch_errors
    );
    println!("dataset_hash: {}", sc.dataset_hash());
    println!("harness_hash: {}", sc.harness_hash());
    println!("env_hash:     {}", sc.env_hash());
    println!("re-run reproduces iff these hashes match and the CI overlaps.");
    println!("────────────────────────────────────");
}

fn http_get(url: &str) -> Result<Vec<u8>> {
    let resp = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?
        .get(url)
        .send()
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        bail!("GET {url} → HTTP {}", resp.status());
    }
    Ok(resp.bytes()?.to_vec())
}

fn python_version() -> String {
    Command::new("python3")
        .arg("--version")
        .output()
        .ok()
        .map(|o| {
            let s = if o.stdout.is_empty() {
                o.stderr
            } else {
                o.stdout
            };
            String::from_utf8_lossy(&s).trim().to_string()
        })
        .unwrap_or_else(|| "python3 (version unknown)".to_string())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
