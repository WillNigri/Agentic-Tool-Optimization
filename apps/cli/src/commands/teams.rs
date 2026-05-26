// v2.13 — `ato teams` OSS thin-client. Shared agents + methodologies are
// a Team-tier feature; all persistence + tier gating lives in ato-cloud
// (services/teams + services/api-gateway/src/middleware/teamTier.ts).
// This file is the additive HTTP surface the OSS desktop + agents call.
//
// Open-core posture (memory: pro-features-never-in-oss): the multi-user
// state IS the Pro/Team value. We expose `teams agents share | list` and
// `teams methodologies share` here only as thin-client GET/POST/DELETE
// shims — no business logic, no fallback path. Free customers without
// the cloud surface get a clean 402 PRO_REQUIRED with upgrade_url.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

use crate::output::{emit_human, emit_json, Opts};

fn api_base() -> String {
    match std::env::var("ATO_CLOUD_URL") {
        Ok(url) => format!("{}/api", url.trim_end_matches('/')),
        Err(_) => "https://api.agentictool.ai/api".to_string(),
    }
}

fn auth_file_path() -> PathBuf {
    crate::db::home_dir().join(".ato").join("auth.json")
}

fn read_token() -> Result<String> {
    let contents = fs::read_to_string(auth_file_path())
        .context("Not signed in. Run `ato login` first.")?;
    let json: Value = serde_json::from_str(&contents)
        .context("Failed to parse ~/.ato/auth.json")?;
    json.get("token")
        .and_then(|t| t.as_str())
        .map(String::from)
        .context("Auth token missing — run `ato login` again")
}

fn http_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("HTTP client build failed")
}

#[derive(Args, Debug)]
pub struct TeamsArgs {
    #[command(subcommand)]
    pub sub: TeamsSub,
}

#[derive(Subcommand, Debug)]
pub enum TeamsSub {
    /// Shared agents in a team workspace (Team tier).
    Agents {
        #[command(subcommand)]
        sub: AgentsSub,
    },
    /// Shared methodology configs in a team workspace (Team tier).
    Methodologies {
        #[command(subcommand)]
        sub: MethodologiesSub,
    },
}

#[derive(Subcommand, Debug)]
pub enum AgentsSub {
    /// Share an agent you own into a team.
    Share {
        /// The cloud-side agents.id (UUID) to share.
        #[arg(long)]
        agent_id: String,
        /// Team UUID.
        #[arg(long)]
        team: String,
    },
    /// List agents shared into a team.
    List {
        /// Team UUID.
        #[arg(long)]
        team: String,
    },
    /// Unshare an agent from a team.
    Unshare {
        #[arg(long)]
        agent_id: String,
        #[arg(long)]
        team: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum MethodologiesSub {
    /// Share a methodology config snapshot into a team. Reads the OSS-local
    /// methodology row and POSTs the snapshot.
    Share {
        /// Methodology slug (local OSS identifier).
        slug: String,
        /// Team UUID.
        #[arg(long)]
        team: String,
    },
    /// List methodologies shared into a team.
    List {
        /// Team UUID.
        #[arg(long)]
        team: String,
    },
    /// Unshare a methodology by id.
    Unshare {
        #[arg(long)]
        methodology_id: String,
        #[arg(long)]
        team: String,
    },
}

#[derive(Serialize)]
struct ShareAgentBody<'a> {
    agent_id: &'a str,
}

#[derive(Serialize)]
struct ShareMethodologyBody<'a> {
    methodology_id: &'a str,
    slug: &'a str,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    config: Value,
}

struct LocalMethodologyRow {
    id: String,
    slug: String,
    description: Option<String>,
    archetype: String,
    variant_matrix_json: String,
    rubric_json: String,
}

pub fn run(args: TeamsArgs, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    match args.sub {
        TeamsSub::Agents { sub } => run_agents(sub, opts),
        TeamsSub::Methodologies { sub } => run_methodologies(sub, db_path, opts),
    }
}

fn run_agents(sub: AgentsSub, opts: &Opts) -> Result<()> {
    let token = read_token()?;
    let client = http_client()?;

    match sub {
        AgentsSub::Share { agent_id, team } => {
            let url = format!("{}/teams/{}/agents/share", api_base(), team);
            let resp = client
                .post(&url)
                .bearer_auth(&token)
                .json(&ShareAgentBody { agent_id: &agent_id })
                .send()
                .context("Failed to call /api/teams/.../agents/share")?;
            handle_response(resp, opts, "Shared agent into team")
        }
        AgentsSub::List { team } => {
            let url = format!("{}/teams/{}/agents", api_base(), team);
            let resp = client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .context("Failed to call /api/teams/.../agents")?;
            handle_list_response(resp, opts, "agent")
        }
        AgentsSub::Unshare { agent_id, team } => {
            let url = format!("{}/teams/{}/agents/{}/share", api_base(), team, agent_id);
            let resp = client
                .delete(&url)
                .bearer_auth(&token)
                .send()
                .context("Failed to call DELETE /api/teams/.../agents/.../share")?;
            handle_response(resp, opts, "Unshared agent from team")
        }
    }
}

fn run_methodologies(sub: MethodologiesSub, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let token = read_token()?;
    let client = http_client()?;

    match sub {
        MethodologiesSub::Share { slug, team } => {
            let row = load_local_methodology(&slug, db_path)?;
            let variant_matrix: Value = serde_json::from_str(&row.variant_matrix_json)
                .with_context(|| format!("Bad variant_matrix JSON for slug={}", slug))?;
            let rubric: Value = serde_json::from_str(&row.rubric_json)
                .with_context(|| format!("Bad rubric JSON for slug={}", slug))?;
            let config = serde_json::json!({
                "archetype": row.archetype,
                "variant_matrix": variant_matrix,
                "rubric": rubric,
            });
            let url = format!("{}/teams/{}/methodologies/share", api_base(), team);
            let body = ShareMethodologyBody {
                methodology_id: &row.id,
                slug: &row.slug,
                name: &row.slug,
                description: row.description.as_deref(),
                config,
            };
            let resp = client
                .post(&url)
                .bearer_auth(&token)
                .json(&body)
                .send()
                .context("Failed to call /api/teams/.../methodologies/share")?;
            handle_response(resp, opts, "Shared methodology into team")
        }
        MethodologiesSub::List { team } => {
            let url = format!("{}/teams/{}/methodologies", api_base(), team);
            let resp = client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .context("Failed to call /api/teams/.../methodologies")?;
            handle_list_response(resp, opts, "methodology")
        }
        MethodologiesSub::Unshare { methodology_id, team } => {
            let url = format!(
                "{}/teams/{}/methodologies/{}/share",
                api_base(),
                team,
                methodology_id
            );
            let resp = client
                .delete(&url)
                .bearer_auth(&token)
                .send()
                .context("Failed to call DELETE /api/teams/.../methodologies/.../share")?;
            handle_response(resp, opts, "Unshared methodology from team")
        }
    }
}

fn load_local_methodology(slug: &str, db_path: &PathBuf) -> Result<LocalMethodologyRow> {
    let conn = crate::db::open_readonly(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, slug, description, archetype, variant_matrix, rubric
           FROM methodologies
          WHERE slug = ?1
          LIMIT 1",
    )?;
    let mut rows = stmt.query_map([slug], |row| {
        Ok(LocalMethodologyRow {
            id: row.get(0)?,
            slug: row.get(1)?,
            description: row.get::<_, Option<String>>(2)?,
            archetype: row.get(3)?,
            variant_matrix_json: row.get(4)?,
            rubric_json: row.get(5)?,
        })
    })?;
    let row = rows
        .next()
        .with_context(|| format!("No local methodology with slug `{}` found", slug))??;
    Ok(row)
}

/// Mirror the desktop client's success guard (apps/desktop/src/lib/cloud-api.ts:184) —
/// a 200 OK that carries `{"success": false, "error": {...}}` is still a failure.
/// Endpoints that don't envelope (no `success` field) fall through to HTTP status.
fn is_success(status: reqwest::StatusCode, body: &Value) -> bool {
    if !status.is_success() {
        return false;
    }
    !matches!(body.get("success").and_then(|v| v.as_bool()), Some(false))
}

fn handle_response(resp: reqwest::blocking::Response, opts: &Opts, human_msg: &str) -> Result<()> {
    let status = resp.status();
    let body: Value = resp.json().unwrap_or(serde_json::json!({}));

    if !is_success(status, &body) {
        return handle_response_error(status, &body, opts);
    }

    if opts.human {
        emit_human(human_msg);
    } else {
        // Unwrap the cloud envelope so callers `jq '.team_id'` not `jq '.data.team_id'`,
        // matching the convention in production_signals.rs (which emits unwrapped rows).
        let payload = body.get("data").cloned().unwrap_or(body);
        emit_json(&payload)?;
    }
    Ok(())
}

fn handle_list_response(resp: reqwest::blocking::Response, opts: &Opts, kind: &str) -> Result<()> {
    let status = resp.status();
    let body: Value = resp.json().unwrap_or(serde_json::json!({}));

    if !is_success(status, &body) {
        return handle_response_error(status, &body, opts);
    }

    // Unwrap envelope before either emit path. `data` is expected to be an array
    // for list endpoints; fall back to the full body if the response is bare.
    let payload = body.get("data").cloned().unwrap_or(body.clone());

    if !opts.human {
        emit_json(&payload)?;
        return Ok(());
    }

    let rows = payload.as_array();
    match rows {
        Some(arr) if !arr.is_empty() => {
            emit_human(&format!("Shared {}s ({}):", kind, arr.len()));
            for row in arr {
                let slug = row.get("slug").and_then(|v| v.as_str()).unwrap_or("?");
                let name = row
                    .get("display_name")
                    .or_else(|| row.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(slug);
                let by = row
                    .get("shared_by_email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let at = row.get("shared_at").and_then(|v| v.as_str()).unwrap_or("?");
                emit_human(&format!("  • {} ({}) — by {} at {}", name, slug, by, at));
            }
        }
        _ => emit_human(&format!("No shared {}s in this team yet.", kind)),
    }
    Ok(())
}

fn handle_response_error(
    status: reqwest::StatusCode,
    body: &Value,
    opts: &Opts,
) -> Result<()> {
    if opts.human {
        let code = body
            .pointer("/error/code")
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN");
        let msg = body
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("Request failed");
        emit_human(&format!("[{}] {} ({})", status, msg, code));
        if let Some(url) = body.pointer("/error/upgrade_url").and_then(|v| v.as_str()) {
            emit_human(&format!("Upgrade: {}", url));
        }
    } else {
        emit_json(body)?;
    }
    std::process::exit(1);
}
