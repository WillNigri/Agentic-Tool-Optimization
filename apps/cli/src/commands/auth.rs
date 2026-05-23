// commands/auth.rs — `ato login` / `ato signup` / `ato logout` / `ato whoami`.
//
// Agentic-first: every auth operation works headlessly so coding
// agents (Claude Code, Codex, Gemini CLI) can authenticate on
// behalf of the user without opening a browser.
//
// Token storage: ~/.ato/auth.json
//   { "token": "<access>", "refreshToken": "<refresh>", "email": "…" }
//
// The same file is read by `ato pro status` and the desktop app's
// useAuthStore (via localStorage mirror).

use clap::{Args, Subcommand};
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

const API_BASE: &str = "https://ato.cloud/api/auth";

fn auth_file_path() -> PathBuf {
    crate::db::home_dir().join(".ato").join("auth.json")
}

fn save_auth(token: &str, refresh_token: &str, email: &str) -> Result<(), String> {
    let dir = crate::db::home_dir().join(".ato");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create ~/.ato: {}", e))?;
    let json = serde_json::json!({
        "token": token,
        "refreshToken": refresh_token,
        "email": email,
    });
    let path = dir.join("auth.json");
    fs::write(&path, serde_json::to_string_pretty(&json).unwrap())
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    // Restrict permissions on Unix (tokens are secrets)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn read_token() -> Option<String> {
    let mut file = fs::File::open(auth_file_path()).ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    json.get("token")?.as_str().map(String::from)
}

fn read_refresh_token() -> Option<String> {
    let mut file = fs::File::open(auth_file_path()).ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    json.get("refreshToken")?.as_str().map(String::from)
}

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))
}

/// Prompt for a value (visible input). Used for email/name.
fn prompt(label: &str) -> String {
    eprint!("{}: ", label);
    io::stderr().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).unwrap_or_default();
    buf.trim().to_string()
}

/// Prompt for a password. Uses `stty -echo` on Unix to hide input.
/// Falls back to visible input if stty fails (piped stdin, agents).
fn prompt_password(label: &str) -> String {
    eprint!("{}: ", label);
    io::stderr().flush().ok();
    #[cfg(unix)]
    {
        let stty_off = std::process::Command::new("stty")
            .arg("-echo")
            .status();
        let mut buf = String::new();
        io::stdin().read_line(&mut buf).unwrap_or_default();
        if stty_off.is_ok() {
            let _ = std::process::Command::new("stty").arg("echo").status();
            eprintln!();
        }
        return buf.trim().to_string();
    }
    #[cfg(not(unix))]
    {
        let mut buf = String::new();
        io::stdin().read_line(&mut buf).unwrap_or_default();
        buf.trim().to_string()
    }
}

fn handle_login(email: Option<String>, password: Option<String>) {
    let email = email.unwrap_or_else(|| prompt("Email"));
    let password = password.unwrap_or_else(|| prompt_password("Password"));

    if email.is_empty() || password.is_empty() {
        eprintln!("Email and password are required.");
        std::process::exit(1);
    }

    let client = match http_client() {
        Ok(c) => c,
        Err(e) => { eprintln!("{}", e); std::process::exit(1); }
    };

    let resp = client
        .post(format!("{}/login", API_BASE))
        .json(&serde_json::json!({ "email": email, "password": password }))
        .send();

    match resp {
        Ok(response) => {
            let status = response.status();
            let body: serde_json::Value = match response.json() {
                Ok(b) => b,
                Err(_) => { eprintln!("Invalid response from server"); std::process::exit(1); }
            };

            if !status.is_success() {
                let code = body.pointer("/error/code").and_then(|v| v.as_str()).unwrap_or("");
                let msg = body.pointer("/error/message").and_then(|v| v.as_str())
                    .unwrap_or("Login failed");

                if code == "EMAIL_NOT_VERIFIED" {
                    eprintln!("Email not verified. Check your inbox for the verification link.");
                    eprintln!("To resend: ato auth resend-verify --email {}", email);
                } else {
                    eprintln!("Error ({}): {}", status.as_u16(), msg);
                }
                std::process::exit(1);
            }

            let token = body.pointer("/data/tokens/accessToken")
                .and_then(|v| v.as_str()).unwrap_or("");
            let refresh = body.pointer("/data/tokens/refreshToken")
                .and_then(|v| v.as_str()).unwrap_or("");
            let user_email = body.pointer("/data/user/email")
                .and_then(|v| v.as_str()).unwrap_or(&email);
            let tier = body.pointer("/data/user/subscription_tier")
                .and_then(|v| v.as_str()).unwrap_or("free");

            if let Err(e) = save_auth(token, refresh, user_email) {
                eprintln!("{}", e);
                std::process::exit(1);
            }

            println!("Logged in as {} (tier: {})", user_email, tier);
        }
        Err(e) => {
            eprintln!("Request failed: {}", e);
            std::process::exit(1);
        }
    }
}

fn handle_signup(email: Option<String>, password: Option<String>, name: Option<String>) {
    let email = email.unwrap_or_else(|| prompt("Email"));
    let password = password.unwrap_or_else(|| prompt_password("Password (min 8 chars, upper+lower+number)"));
    let name = name.unwrap_or_else(|| prompt("Name"));

    if email.is_empty() || password.is_empty() {
        eprintln!("Email and password are required.");
        std::process::exit(1);
    }

    let client = match http_client() {
        Ok(c) => c,
        Err(e) => { eprintln!("{}", e); std::process::exit(1); }
    };

    let mut body = serde_json::json!({ "email": email, "password": password });
    if !name.is_empty() {
        body["name"] = serde_json::Value::String(name);
    }

    let resp = client
        .post(format!("{}/register", API_BASE))
        .json(&body)
        .send();

    match resp {
        Ok(response) => {
            let status = response.status();
            let body: serde_json::Value = match response.json() {
                Ok(b) => b,
                Err(_) => { eprintln!("Invalid response from server"); std::process::exit(1); }
            };

            if !status.is_success() {
                let msg = body.pointer("/error/message").and_then(|v| v.as_str())
                    .unwrap_or("Signup failed");
                eprintln!("Error ({}): {}", status.as_u16(), msg);
                std::process::exit(1);
            }

            let token = body.pointer("/data/tokens/accessToken")
                .and_then(|v| v.as_str()).unwrap_or("");
            let refresh = body.pointer("/data/tokens/refreshToken")
                .and_then(|v| v.as_str()).unwrap_or("");
            let user_email = body.pointer("/data/user/email")
                .and_then(|v| v.as_str()).unwrap_or(&email);

            if let Err(e) = save_auth(token, refresh, user_email) {
                eprintln!("{}", e);
                std::process::exit(1);
            }

            println!("Account created for {}", user_email);
            println!("Check your inbox for the verification email.");
            println!("After verifying, run: ato login");
        }
        Err(e) => {
            eprintln!("Request failed: {}", e);
            std::process::exit(1);
        }
    }
}

fn handle_logout() {
    let token = read_token();
    let refresh = read_refresh_token();

    // Try server-side logout (best-effort)
    if let Some(ref t) = token {
        if let Ok(client) = http_client() {
            let mut body = serde_json::json!({});
            if let Some(ref r) = refresh {
                body["refreshToken"] = serde_json::Value::String(r.clone());
            }
            let _ = client
                .post(format!("{}/logout", API_BASE))
                .bearer_auth(t)
                .json(&body)
                .send();
        }
    }

    // Delete local auth file
    let path = auth_file_path();
    if path.exists() {
        if let Err(e) = fs::remove_file(&path) {
            eprintln!("Warning: could not remove {}: {}", path.display(), e);
        }
    }

    println!("Logged out.");
}

fn handle_whoami() {
    let token = match read_token() {
        Some(t) => t,
        None => {
            eprintln!("Not logged in. Run: ato login");
            std::process::exit(1);
        }
    };

    let client = match http_client() {
        Ok(c) => c,
        Err(e) => { eprintln!("{}", e); std::process::exit(1); }
    };

    let resp = client
        .get(format!("{}/me", API_BASE))
        .bearer_auth(&token)
        .send();

    match resp {
        Ok(response) => {
            let status = response.status();
            let body: serde_json::Value = match response.json() {
                Ok(b) => b,
                Err(_) => { eprintln!("Invalid response"); std::process::exit(1); }
            };

            if !status.is_success() {
                let msg = body.pointer("/error/message").and_then(|v| v.as_str())
                    .unwrap_or("Auth failed — token may be expired. Run: ato login");
                eprintln!("{}", msg);
                std::process::exit(1);
            }

            let email = body.pointer("/data/user/email")
                .and_then(|v| v.as_str()).unwrap_or("unknown");
            let tier = body.pointer("/data/user/subscription_tier")
                .and_then(|v| v.as_str()).unwrap_or("free");
            let name = body.pointer("/data/user/name")
                .and_then(|v| v.as_str()).unwrap_or("");
            let verified = body.pointer("/data/user/email_verified")
                .and_then(|v| v.as_bool()).unwrap_or(false);

            println!("Email:        {}", email);
            if !name.is_empty() { println!("Name:         {}", name); }
            println!("Verified:     {}", if verified { "yes" } else { "no" });
            println!("Subscription: {}", tier);
        }
        Err(e) => {
            eprintln!("Request failed: {}", e);
            std::process::exit(1);
        }
    }
}

fn handle_resend_verify(email: Option<String>) {
    let email = email.unwrap_or_else(|| {
        // Try reading from auth.json
        if let Ok(mut f) = fs::File::open(auth_file_path()) {
            let mut s = String::new();
            if f.read_to_string(&mut s).is_ok() {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&s) {
                    if let Some(e) = j.get("email").and_then(|v| v.as_str()) {
                        return e.to_string();
                    }
                }
            }
        }
        prompt("Email")
    });

    if email.is_empty() {
        eprintln!("Email is required.");
        std::process::exit(1);
    }

    let client = match http_client() {
        Ok(c) => c,
        Err(e) => { eprintln!("{}", e); std::process::exit(1); }
    };

    let resp = client
        .post(format!("{}/resend-verification", API_BASE))
        .json(&serde_json::json!({ "email": email }))
        .send();

    match resp {
        Ok(response) => {
            if response.status().is_success() {
                println!("Verification email sent to {} (if an unverified account exists).", email);
            } else {
                eprintln!("Request failed (HTTP {})", response.status().as_u16());
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Request failed: {}", e);
            std::process::exit(1);
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    /// Sign in to ATO Cloud. Saves token to ~/.ato/auth.json.
    Login {
        /// Email (prompted if omitted)
        #[arg(long)]
        email: Option<String>,
        /// Password (prompted securely if omitted)
        #[arg(long)]
        password: Option<String>,
    },
    /// Create a new ATO Cloud account.
    Signup {
        /// Email (prompted if omitted)
        #[arg(long)]
        email: Option<String>,
        /// Password (prompted securely if omitted)
        #[arg(long)]
        password: Option<String>,
        /// Display name (prompted if omitted)
        #[arg(long)]
        name: Option<String>,
    },
    /// Sign out and delete local auth tokens.
    Logout,
    /// Show the currently logged-in user and subscription tier.
    Whoami,
    /// Resend the email verification link.
    #[command(name = "resend-verify")]
    ResendVerify {
        /// Email (uses saved auth email if omitted)
        #[arg(long)]
        email: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub cmd: AuthCommand,
}

pub fn run(args: AuthArgs) {
    match args.cmd {
        AuthCommand::Login { email, password } => handle_login(email, password),
        AuthCommand::Signup { email, password, name } => handle_signup(email, password, name),
        AuthCommand::Logout => handle_logout(),
        AuthCommand::Whoami => handle_whoami(),
        AuthCommand::ResendVerify { email } => handle_resend_verify(email),
    }
}
