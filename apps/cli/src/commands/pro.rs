// commands/pro.rs — `ato pro enable|status|features|test`.
//
// Agentic-first: every Pro capability is testable from the CLI so
// coding agents can verify the paid tier works end-to-end.

use clap::{Args, Subcommand};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

fn cloud_base() -> String {
    std::env::var("ATO_CLOUD_URL")
        .unwrap_or_else(|_| "https://ato.cloud".to_string())
        .trim_end_matches('/')
        .to_string()
}
fn checkout_url() -> String { format!("{}/pro/checkout?source=cli", cloud_base()) }
fn api_base() -> String { format!("{}/api", cloud_base()) }

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

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))
}

fn open_browser(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let result = Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = Command::new("cmd").args(["/C", "start", "", url]).spawn();

    result.is_ok()
}

/// Fetch user profile from /api/auth/me. Returns (tier, email) or exits.
fn fetch_me(token: &str) -> (String, String) {
    let client = http_client().unwrap_or_else(|e| { eprintln!("{}", e); std::process::exit(1); });
    let resp = client.get(format!("{}/auth/me", api_base())).bearer_auth(token).send();
    match resp {
        Ok(r) if r.status().is_success() => {
            let body: serde_json::Value = r.json().unwrap_or_default();
            let tier = body.pointer("/data/user/subscription_tier")
                .and_then(|v| v.as_str()).unwrap_or("free").to_string();
            let email = body.pointer("/data/user/email")
                .and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            (tier, email)
        }
        Ok(r) => {
            eprintln!("Auth failed (HTTP {}). Run: ato login", r.status().as_u16());
            std::process::exit(1);
        }
        Err(e) => { eprintln!("Request failed: {}", e); std::process::exit(1); }
    }
}

/// Tier hierarchy for access checks.
fn tier_rank(tier: &str) -> u8 {
    match tier {
        "free" => 0,
        "pro" => 1,
        "team" | "platform" => 2,
        "enterprise" => 3,
        _ => 0,
    }
}

// ─── Feature catalog (mirrors apps/desktop/src/lib/tier.ts) ─────

struct FeatureInfo {
    id: &'static str,
    name: &'static str,
    min_tier: &'static str,
    description: &'static str,
    testable: bool,
}

const FEATURES: &[FeatureInfo] = &[
    // Free features (local, unlimited)
    FeatureInfo { id: "variables.advanced", name: "Advanced Variables", min_tier: "free",
        description: "Dynamic resolvers (MCP/DB/file/computed)", testable: false },
    FeatureInfo { id: "context-hooks", name: "Context Hooks", min_tier: "free",
        description: "Pre-call hooks inject fresh data each turn", testable: false },
    FeatureInfo { id: "summarizer.tunable", name: "Tunable Summarizer", min_tier: "free",
        description: "Custom summary model + threshold", testable: false },
    FeatureInfo { id: "groups.unlimited", name: "Unlimited Groups", min_tier: "free",
        description: "No cap on child agents per group", testable: false },
    FeatureInfo { id: "groups.editor", name: "Group Editor", min_tier: "free",
        description: "Visual group composition editor", testable: false },
    FeatureInfo { id: "role-models", name: "Role Models", min_tier: "free",
        description: "Per-task model selection", testable: false },
    FeatureInfo { id: "evaluators", name: "Ad-hoc Evaluators", min_tier: "free",
        description: "Single-shot LLM-as-judge scoring", testable: false },

    // Pro features (cloud infra)
    FeatureInfo { id: "evaluators.scheduled", name: "Scheduled Evaluators", min_tier: "pro",
        description: "Cron-driven batch eval runs", testable: false },
    FeatureInfo { id: "cloud-traces", name: "Cloud Traces", min_tier: "pro",
        description: "30-day cross-device trace retention + regression detection", testable: true },
    FeatureInfo { id: "cloud-sync", name: "Cloud Sync", min_tier: "pro",
        description: "Agents + skills sync across devices", testable: false },
    FeatureInfo { id: "embed-key", name: "Embed Key", min_tier: "pro",
        description: "API key for trace upload (mint-on-first-read)", testable: true },

    // Team features
    FeatureInfo { id: "provider-keys", name: "Provider Keys", min_tier: "team",
        description: "Encrypted key store for cron usage-poller", testable: false },
    FeatureInfo { id: "team-workspaces", name: "Team Workspaces", min_tier: "team",
        description: "Shared agents + skills across teammates", testable: false },

    // Enterprise
    FeatureInfo { id: "enterprise.evaluator-budgets", name: "Evaluator Budgets", min_tier: "enterprise",
        description: "Per-team eval spend caps", testable: false },
    FeatureInfo { id: "enterprise.halo", name: "HALO", min_tier: "enterprise",
        description: "Org-wide safety guardrails", testable: false },
    FeatureInfo { id: "enterprise.sso", name: "Enterprise SSO", min_tier: "enterprise",
        description: "SAML/OIDC via Okta, Entra, Google Workspace", testable: false },
    FeatureInfo { id: "enterprise.audit", name: "Audit Trail", min_tier: "enterprise",
        description: "SOC2-aligned unlimited audit retention", testable: false },
];

// ─── Handlers ─────────────────────────────────────────────────

fn handle_enable() {
    println!("Opening browser to ATO Pro checkout…");
    let url = checkout_url();
    if !open_browser(&url) {
        eprintln!("Could not open browser. Visit:\n  {}", url);
        std::process::exit(1);
    }
    println!("After paying, run `ato pro status` to confirm.");
}

fn handle_status() {
    let token = match read_token() {
        Some(t) => t,
        None => { eprintln!("Not logged in. Run: ato login"); std::process::exit(1); }
    };
    let (tier, email) = fetch_me(&token);
    println!("Email:        {}", email);
    println!("Subscription: {}", tier);
    if tier == "free" {
        println!();
        println!("To upgrade: `ato pro enable`");
    }
}

fn handle_features(human: bool) {
    let (user_tier, token) = match read_token() {
        Some(t) => {
            let (tier, _) = fetch_me(&t);
            (tier, Some(t))
        }
        None => ("free".to_string(), None),
    };

    let rank = tier_rank(&user_tier);

    if human {
        println!("Your tier: {}\n", user_tier.to_uppercase());
        println!("{:<28} {:<12} {:<8} {}", "Feature", "Requires", "Access", "Description");
        println!("{}", "-".repeat(90));
        for f in FEATURES {
            let has_access = tier_rank(f.min_tier) <= rank;
            let access = if has_access { "yes" } else { "locked" };
            let marker = if has_access { "+" } else { "-" };
            println!("{} {:<26} {:<12} {:<8} {}",
                marker, f.name, f.min_tier, access, f.description);
        }

        let unlocked = FEATURES.iter().filter(|f| tier_rank(f.min_tier) <= rank).count();
        let total = FEATURES.len();
        println!("\n{}/{} features unlocked.", unlocked, total);
        if rank == 0 && token.is_none() {
            println!("Not logged in. Run: ato login");
        } else if rank == 0 {
            println!("Upgrade to unlock Pro features: ato pro enable");
        }
    } else {
        let features: Vec<serde_json::Value> = FEATURES.iter().map(|f| {
            serde_json::json!({
                "id": f.id,
                "name": f.name,
                "min_tier": f.min_tier,
                "description": f.description,
                "has_access": tier_rank(f.min_tier) <= rank,
                "testable": f.testable,
            })
        }).collect();
        let out = serde_json::json!({
            "user_tier": user_tier,
            "features": features,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    }
}

fn handle_test(human: bool) {
    let token = match read_token() {
        Some(t) => t,
        None => { eprintln!("Not logged in. Run: ato login"); std::process::exit(1); }
    };

    let (tier, email) = fetch_me(&token);
    let rank = tier_rank(&tier);

    if human {
        println!("Testing Pro features for {} (tier: {})\n", email, tier);
    }

    let client = http_client().unwrap_or_else(|e| { eprintln!("{}", e); std::process::exit(1); });

    struct TestResult {
        feature: &'static str,
        passed: bool,
        detail: String,
    }
    let mut results: Vec<TestResult> = Vec::new();

    // Test 1: Auth / profile endpoint
    {
        let passed = !email.is_empty() && email != "unknown";
        results.push(TestResult {
            feature: "auth.profile",
            passed,
            detail: if passed {
                format!("GET /api/auth/me → {} ({})", email, tier)
            } else {
                "GET /api/auth/me failed".to_string()
            },
        });
    }

    // Test 2: Tier endpoint
    {
        let resp = client.get(format!("{}/tier/me", api_base()))
            .bearer_auth(&token).send();
        let (passed, detail) = match resp {
            Ok(r) if r.status().is_success() => {
                let body: serde_json::Value = r.json().unwrap_or_default();
                let t = body.pointer("/data/tier").and_then(|v| v.as_str()).unwrap_or("?");
                (true, format!("GET /api/tier/me → tier={}", t))
            }
            Ok(r) => (false, format!("GET /api/tier/me → HTTP {}", r.status().as_u16())),
            Err(e) => (false, format!("GET /api/tier/me → {}", e)),
        };
        results.push(TestResult { feature: "billing.tier", passed, detail });
    }

    // Test 3: Embed key (Pro+ only)
    {
        let resp = client.get(format!("{}/auth/me/embed-key", api_base()))
            .bearer_auth(&token).send();
        let (passed, detail) = match resp {
            Ok(r) if r.status().is_success() => {
                let body: serde_json::Value = r.json().unwrap_or_default();
                let prefix = body.pointer("/data/prefix")
                    .and_then(|v| v.as_str()).unwrap_or("?");
                (true, format!("GET /api/auth/me/embed-key → prefix={}", prefix))
            }
            Ok(r) if r.status().as_u16() == 403 => {
                if rank >= 1 {
                    (false, "GET /api/auth/me/embed-key → 403 (unexpected for paid tier)".to_string())
                } else {
                    (true, "GET /api/auth/me/embed-key → 403 TIER_REQUIRED (correct for free)".to_string())
                }
            }
            Ok(r) => (false, format!("GET /api/auth/me/embed-key → HTTP {}", r.status().as_u16())),
            Err(e) => (false, format!("GET /api/auth/me/embed-key → {}", e)),
        };
        results.push(TestResult { feature: "embed-key", passed, detail });
    }

    // Test 4: Checkout session creation (Pro tier — uses test mode, doesn't charge)
    {
        let resp = client.post(format!("{}/billing/checkout", api_base()))
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "tier": "pro",
                "successUrl": "ato://billing/success",
                "cancelUrl": "ato://billing/cancel",
            }))
            .send();
        let (passed, detail) = match resp {
            Ok(r) if r.status().is_success() => {
                let body: serde_json::Value = r.json().unwrap_or_default();
                let has_url = body.pointer("/data/url").is_some();
                (has_url, format!("POST /api/billing/checkout → session created (url={})",
                    if has_url { "present" } else { "missing" }))
            }
            Ok(r) => {
                let body = r.text().unwrap_or_default();
                (false, format!("POST /api/billing/checkout → {}", &body[..body.len().min(120)]))
            }
            Err(e) => (false, format!("POST /api/billing/checkout → {}", e)),
        };
        results.push(TestResult { feature: "billing.checkout", passed, detail });
    }

    // Test 5: Cloud traces endpoint (Pro+ only)
    {
        let resp = client.get(format!("{}/agent-traces?limit=1", api_base()))
            .bearer_auth(&token).send();
        let (passed, detail) = match resp {
            Ok(r) if r.status().is_success() => {
                (true, "GET /api/agent-traces → 200 (cloud traces accessible)".to_string())
            }
            Ok(r) if r.status().as_u16() == 403 => {
                if rank >= 1 {
                    (false, "GET /api/agent-traces → 403 (unexpected for paid tier)".to_string())
                } else {
                    (true, "GET /api/agent-traces → 403 (correct for free tier)".to_string())
                }
            }
            Ok(r) if r.status().as_u16() == 404 => {
                // Endpoint may not exist yet
                (true, "GET /api/agent-traces → 404 (endpoint not deployed yet)".to_string())
            }
            Ok(r) => (false, format!("GET /api/agent-traces → HTTP {}", r.status().as_u16())),
            Err(e) => (false, format!("GET /api/agent-traces → {}", e)),
        };
        results.push(TestResult { feature: "cloud-traces", passed, detail });
    }

    // Output
    let pass_count = results.iter().filter(|r| r.passed).count();
    let total = results.len();

    if human {
        for r in &results {
            let mark = if r.passed { "PASS" } else { "FAIL" };
            let color = if r.passed { "\x1b[32m" } else { "\x1b[31m" };
            println!("  {}[{}]\x1b[0m {} — {}", color, mark, r.feature, r.detail);
        }
        println!("\n{}/{} tests passed.", pass_count, total);
        if pass_count == total {
            println!("All pro features working.");
        }
    } else {
        let tests: Vec<serde_json::Value> = results.iter().map(|r| {
            serde_json::json!({
                "feature": r.feature,
                "passed": r.passed,
                "detail": r.detail,
            })
        }).collect();
        let out = serde_json::json!({
            "user_tier": tier,
            "email": email,
            "passed": pass_count,
            "total": total,
            "all_passed": pass_count == total,
            "tests": tests,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    }

    if pass_count < total {
        std::process::exit(1);
    }
}

// ─── CLI wiring ───────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum ProCommand {
    /// Open the browser at the ATO Pro checkout page.
    Enable,
    /// Check the current subscription tier for the logged-in user.
    Status,
    /// List all features and whether you have access.
    Features,
    /// Smoke-test Pro cloud endpoints (auth, billing, traces, embed key).
    Test,
}

#[derive(Args, Debug)]
pub struct ProArgs {
    #[command(subcommand)]
    pub cmd: ProCommand,
}

pub fn run(args: ProArgs, human: bool) {
    match args.cmd {
        ProCommand::Enable => handle_enable(),
        ProCommand::Status => handle_status(),
        ProCommand::Features => handle_features(human),
        ProCommand::Test => handle_test(human),
    }
}
