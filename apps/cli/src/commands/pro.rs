// commands/pro.rs — `ato pro enable` / `ato pro status`.
//
// Phase A chunk 6 (war-room 87E6CADF round 3, DevEx AMEND): the
// smooth OSS → Pro upgrade flow. Pre-fix, a user who wanted to
// pay had to manually navigate to ato.cloud and find the upgrade
// button. Post-fix: one terminal command opens the browser at the
// Stripe checkout URL.
//
// Subcommands:
//   - enable   open the browser at the Pro checkout
//   - status   call /api/auth/me + print the current subscription
//
// Original draft by minimax (parallel-engineering workflow, Will
// authorized 2026-05-22). Integrated + polished by claude:
//   - Uses crate::db::home_dir() to match the rest of the CLI
//     (was using `dirs::home_dir()` which adds a dep we don't want)
//   - reqwest is already a transitive dep
//   - Doc comments match the project's house style
//   - `--source=cli` query param so cloud-side conversion analytics
//     can attribute upgrades to the CLI funnel vs the desktop GUI

use clap::{Args, Subcommand};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

const CHECKOUT_URL: &str = "https://ato.cloud/pro/checkout?source=cli";
const AUTH_ME_URL: &str = "https://ato.cloud/api/auth/me";

fn auth_file_path() -> PathBuf {
    crate::db::home_dir().join(".ato").join("auth.json")
}

/// Read bearer token from ~/.ato/auth.json. Returns None when the
/// file doesn't exist (user hasn't run `ato login` yet) or is
/// malformed (we don't crash — caller decides what to do).
fn read_token() -> Option<String> {
    let mut file = fs::File::open(auth_file_path()).ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    json.get("token")?.as_str().map(String::from)
}

/// Open URL in the system browser. Per-OS dispatch; returns true on
/// successful spawn (NOT on browser-actually-opened — we never know
/// that on any OS reliably).
fn open_browser(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let result = Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = Command::new("cmd").args(["/C", "start", "", url]).spawn();

    result.is_ok()
}

fn handle_enable() {
    println!("Opening browser to ATO Pro checkout…");

    if !open_browser(CHECKOUT_URL) {
        eprintln!(
            "Could not open browser automatically. Please visit:\n  {}",
            CHECKOUT_URL
        );
        std::process::exit(1);
    }

    println!("After paying, run `ato pro status` to confirm your subscription is active.");
}

fn handle_status() {
    let token = match read_token() {
        Some(t) => t,
        None => {
            eprintln!("No auth token found at ~/.ato/auth.json. Run `ato login` first.");
            std::process::exit(1);
        }
    };

    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("HTTP client init failed: {}", e);
            std::process::exit(1);
        }
    };

    let resp = client.get(AUTH_ME_URL).bearer_auth(&token).send();
    match resp {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                let body = response.text().unwrap_or_default();
                if let Ok(err_json) = serde_json::from_str::<serde_json::Value>(&body) {
                    if let Some(msg) = err_json.get("error").and_then(|v| v.as_str()) {
                        eprintln!("Error ({}): {}", status, msg);
                    } else {
                        eprintln!("HTTP {}", status);
                    }
                } else {
                    eprintln!("HTTP {}", status);
                }
                std::process::exit(1);
            }

            match response.json::<serde_json::Value>() {
                Ok(body) => {
                    let sub = body
                        .get("subscription_tier")
                        .or_else(|| body.get("subscription"))
                        .or_else(|| body.get("tier"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let email = body
                        .get("email")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(no email on record)");
                    println!("Email:        {}", email);
                    println!("Subscription: {}", sub);
                    if sub == "free" {
                        println!();
                        println!("To upgrade: `ato pro enable`");
                    }
                }
                Err(_) => {
                    eprintln!("Could not parse response from {}", AUTH_ME_URL);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Request failed: {}", e);
            std::process::exit(1);
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum ProCommand {
    /// Open the browser at the ATO Pro checkout page.
    Enable,
    /// Check the current subscription tier for the logged-in user.
    Status,
}

#[derive(Args, Debug)]
pub struct ProArgs {
    #[command(subcommand)]
    pub cmd: ProCommand,
}

pub fn run(args: ProArgs) {
    match args.cmd {
        ProCommand::Enable => handle_enable(),
        ProCommand::Status => handle_status(),
    }
}
