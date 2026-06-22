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
    /// Create a new team workspace. Team tier required.
    Create {
        /// Team display name.
        #[arg(long)]
        name: String,
        /// Optional description.
        #[arg(long)]
        description: Option<String>,
    },
    /// List all teams the current user belongs to.
    List,
    /// Delete a team (admin/owner only).
    Delete {
        #[arg(long)]
        team: String,
    },
    /// Invite a teammate by email.
    Invite {
        /// Team UUID.
        #[arg(long)]
        team: String,
        /// Email to invite.
        #[arg(long)]
        email: String,
        /// Role for the invitee. Defaults to member.
        #[arg(long, default_value = "member")]
        role: String,
    },
    /// Accept a team invitation. Use the token from `ato teams invite`
    /// (printed on invite) or the invitation email link.
    Accept {
        /// Invitation token.
        #[arg(long)]
        token: String,
    },
    /// List members of a team.
    Members {
        #[arg(long)]
        team: String,
    },
    /// Remove a member from a team (admin/owner only).
    RemoveMember {
        #[arg(long)]
        team: String,
        /// User UUID of the member to remove.
        #[arg(long)]
        user_id: String,
    },
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
        /// Human-readable name teammates see in the shared list.
        /// Defaults to the slug if omitted.
        #[arg(long)]
        name: Option<String>,
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
        TeamsSub::Create { name, description } => run_create(name, description, opts),
        TeamsSub::List => run_list(opts),
        TeamsSub::Delete { team } => run_delete(team, opts),
        TeamsSub::Invite { team, email, role } => run_invite(team, email, role, opts),
        TeamsSub::Accept { token } => run_accept(token, opts),
        TeamsSub::Members { team } => run_members(team, opts),
        TeamsSub::RemoveMember { team, user_id } => run_remove_member(team, user_id, opts),
        TeamsSub::Agents { sub } => run_agents(sub, opts),
        TeamsSub::Methodologies { sub } => run_methodologies(sub, db_path, opts),
    }
}

#[derive(Serialize)]
struct CreateTeamBody<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
}

#[derive(Serialize)]
struct InviteBody<'a> {
    email: &'a str,
    role: &'a str,
}

#[derive(Serialize)]
struct AcceptBody<'a> {
    token: &'a str,
}

fn run_create(name: String, description: Option<String>, opts: &Opts) -> Result<()> {
    let token = read_token()?;
    let client = http_client()?;
    let url = format!("{}/teams", api_base());
    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .json(&CreateTeamBody { name: &name, description: description.as_deref() })
        .send()
        .context("Failed to call POST /api/teams")?;
    handle_response(resp, opts, &format!("Created team '{}'", name))
}

fn run_list(opts: &Opts) -> Result<()> {
    let token = read_token()?;
    let client = http_client()?;
    let url = format!("{}/teams", api_base());
    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .context("Failed to call GET /api/teams")?;
    handle_list_response(resp, opts, "team")
}

fn run_delete(team: String, opts: &Opts) -> Result<()> {
    let token = read_token()?;
    let client = http_client()?;
    // Cloud /teams/:id routes are keyed by UUID; accept a slug too.
    let team = crate::commands::team_shared::resolve_team_id(&team, &token)?;
    let url = format!("{}/teams/{}", api_base(), team);
    let resp = client
        .delete(&url)
        .bearer_auth(&token)
        .send()
        .context("Failed to call DELETE /api/teams/...")?;
    handle_response(resp, opts, "Deleted team")
}

fn run_invite(team: String, email: String, role: String, opts: &Opts) -> Result<()> {
    let token = read_token()?;
    let client = http_client()?;
    let team = crate::commands::team_shared::resolve_team_id(&team, &token)?;
    let url = format!("{}/teams/{}/members", api_base(), team);
    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .json(&InviteBody { email: &email, role: &role })
        .send()
        .context("Failed to call POST /api/teams/.../members")?;
    handle_invite_response(resp, opts, &email, &role)
}

/// Like handle_response, but surfaces the invitation token so a CLI-only
/// invitee can accept with `ato teams accept --token <token>` (acceptance was
/// previously web/email-only — there was no CLI accept path). JSON mode already
/// emits the token via the unwrapped `data`; this adds it to the human view.
fn handle_invite_response(
    resp: reqwest::blocking::Response,
    opts: &Opts,
    email: &str,
    role: &str,
) -> Result<()> {
    let status = resp.status();
    let body: Value = resp.json().unwrap_or(serde_json::json!({}));
    if !is_success(status, &body) {
        return handle_response_error(status, &body, opts);
    }
    if opts.human {
        let tok = body
            .get("data")
            .and_then(|d| d.get("token"))
            .and_then(|v| v.as_str());
        match tok {
            Some(t) => emit_human(&format!(
                "Invited {} as {}.\n  invite token: {}\n  They accept with: ato teams accept --token {}",
                email, role, t, t
            )),
            None => emit_human(&format!("Invited {} as {}", email, role)),
        }
    } else {
        let payload = body.get("data").cloned().unwrap_or(body);
        emit_json(&payload)?;
    }
    Ok(())
}

fn run_accept(invite_token: String, opts: &Opts) -> Result<()> {
    let token = read_token()?;
    let client = http_client()?;
    let url = format!("{}/teams/invitations/accept", api_base());
    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .json(&AcceptBody { token: &invite_token })
        .send()
        .context("Failed to call POST /api/teams/invitations/accept")?;
    handle_response(resp, opts, "Joined team — invitation accepted")
}

fn run_members(team: String, opts: &Opts) -> Result<()> {
    let token = read_token()?;
    let client = http_client()?;
    let team = crate::commands::team_shared::resolve_team_id(&team, &token)?;
    let url = format!("{}/teams/{}/members", api_base(), team);
    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .context("Failed to call GET /api/teams/.../members")?;
    handle_members_response(resp, opts)
}

/// Render a team's members. Distinct from `handle_list_response` (which is
/// for SHARED RESOURCES: slug / display_name / shared_by_email). A member row
/// is `{ role, joined_at, invited_by, user: { email, name } }` — using the
/// shared-resource renderer printed every field as `?` ("? (?) — by ? at ?").
/// JSON output is unchanged (the nested `user.email`/`user.name` is already
/// present, so agents reading `--json` get identities directly).
fn handle_members_response(resp: reqwest::blocking::Response, opts: &Opts) -> Result<()> {
    let status = resp.status();
    let body: Value = resp.json().unwrap_or(serde_json::json!({}));
    if !is_success(status, &body) {
        return handle_response_error(status, &body, opts);
    }
    let payload = body.get("data").cloned().unwrap_or_else(|| body.clone());
    let arr = match payload.as_array() {
        Some(a) => a,
        None => {
            let preview: String = payload.to_string().chars().take(256).collect();
            eprintln!("[malformed-response] expected an array under `data`; body: {}", preview);
            std::process::exit(1);
        }
    };
    if !opts.human {
        emit_json(&payload)?;
        return Ok(());
    }
    if arr.is_empty() {
        emit_human("No members in this team.");
        return Ok(());
    }
    emit_human(&format!("Members ({}):", arr.len()));
    for row in arr {
        // email/name live under the nested `user` object; fall back to a
        // flattened shape just in case the endpoint changes.
        let email = row
            .pointer("/user/email")
            .or_else(|| row.get("email"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let name = row
            .pointer("/user/name")
            .or_else(|| row.get("name"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(email);
        let role = row.get("role").and_then(|v| v.as_str()).unwrap_or("member");
        let joined = row.get("joined_at").and_then(|v| v.as_str()).unwrap_or("?");
        emit_human(&format!("  • {} <{}> — {} (joined {})", name, email, role, joined));
    }
    Ok(())
}

fn run_remove_member(team: String, user_id: String, opts: &Opts) -> Result<()> {
    let token = read_token()?;
    let client = http_client()?;
    let team = crate::commands::team_shared::resolve_team_id(&team, &token)?;
    let url = format!("{}/teams/{}/members/{}", api_base(), team, user_id);
    let resp = client
        .delete(&url)
        .bearer_auth(&token)
        .send()
        .context("Failed to call DELETE /api/teams/.../members/...")?;
    handle_response(resp, opts, "Removed member from team")
}

fn run_agents(sub: AgentsSub, opts: &Opts) -> Result<()> {
    let token = read_token()?;
    let client = http_client()?;

    match sub {
        AgentsSub::Share { agent_id, team } => {
            let team = crate::commands::team_shared::resolve_team_id(&team, &token)?;
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
            let team = crate::commands::team_shared::resolve_team_id(&team, &token)?;
            let url = format!("{}/teams/{}/agents", api_base(), team);
            let resp = client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .context("Failed to call /api/teams/.../agents")?;
            handle_list_response(resp, opts, "agent")
        }
        AgentsSub::Unshare { agent_id, team } => {
            let team = crate::commands::team_shared::resolve_team_id(&team, &token)?;
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
        MethodologiesSub::Share { slug, team, name } => {
            let team = crate::commands::team_shared::resolve_team_id(&team, &token)?;
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
            // Teammates browsing the shared list see `display_name = name`; without
            // --name they'd see the machine slug. Default to slug only as a fallback.
            let display_name = name.as_deref().unwrap_or(&row.slug);
            let body = ShareMethodologyBody {
                methodology_id: &row.id,
                slug: &row.slug,
                name: display_name,
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
            let team = crate::commands::team_shared::resolve_team_id(&team, &token)?;
            let url = format!("{}/teams/{}/methodologies", api_base(), team);
            let resp = client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .context("Failed to call /api/teams/.../methodologies")?;
            handle_list_response(resp, opts, "methodology")
        }
        MethodologiesSub::Unshare { methodology_id, team } => {
            let team = crate::commands::team_shared::resolve_team_id(&team, &token)?;
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

/// Permissive variant of the desktop client's success guard
/// (apps/desktop/src/lib/cloud-api.ts:184). The desktop treats a 200 OK with
/// no `success` field as failure; this Rust version treats it as success, on
/// purpose — bare (non-enveloped) endpoints exist and should still pass
/// through. Only an explicit `{"success": false, ...}` routes to the error
/// path. If/when the cloud regression-tests every team-tier endpoint to
/// always envelope, tighten this to `Some(true)` only and re-mirror.
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

    // Unwrap envelope before either emit path. `data` is expected to be an
    // array for list endpoints; fall back to the full body if the response is
    // bare. `unwrap_or_else` makes the fallback lazy so the clone only runs
    // when `data` is missing.
    let payload = body.get("data").cloned().unwrap_or_else(|| body.clone());

    // Validate shape FIRST so both --json and --human paths fail the same way
    // on a malformed response. Without this, an `ato … --json | jq` pipeline
    // would silently receive garbage. Dump the body to stderr (not stdout) so
    // it doesn't corrupt the JSON consumer's input; truncate to keep logs sane
    // and avoid leaking large response payloads.
    let arr = match payload.as_array() {
        Some(a) => a,
        None => {
            let preview: String = payload.to_string().chars().take(256).collect();
            eprintln!(
                "[malformed-response] expected an array under `data`; body: {}",
                preview
            );
            std::process::exit(1);
        }
    };

    if !opts.human {
        emit_json(&payload)?;
        return Ok(());
    }

    // Pluralize correctly: "methodology" → "methodologies", not "methodologys".
    let plural = if kind.ends_with('y') {
        format!("{}ies", &kind[..kind.len() - 1])
    } else {
        format!("{}s", kind)
    };

    if arr.is_empty() {
        emit_human(&format!("No shared {} in this team yet.", plural));
        return Ok(());
    }

    emit_human(&format!("Shared {} ({}):", plural, arr.len()));
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
