// v2.3.32 Phase 6.x-J — SSH-backed remote runtime adapter.
//
// Triggered by @iamknownasfesal asking on X: "how can i make my
// claude agent that is on my computer vs that is on my server talk
// with each other? atm just copying responses into each other lol".
//
// Shape:
//   - User registers a remote with `ato runtimes add-remote ...`,
//     giving it a local slug (e.g. `claude-server`).
//   - `ato dispatch claude-server "..."` looks up the row, builds
//     `ssh -i <key> -p <port> user@host '<binary> <args> <prompt>'`,
//     and captures stdout/stderr/exit like a local dispatch.
//   - Persistence path is identical to local dispatch — same
//     execution_logs row shape, same live_runs registration. The GUI
//     and the activity feed don't need to care it ran remotely.
//
// Out of scope (Phase 7+):
//   - Reverse direction (server-initiated calls back to the laptop).
//     That needs both sides running an authenticated daemon.
//   - Streaming. We capture full stdout after the binary exits; for
//     long-running remotes the user sees a wait. Phase 6.x-F (API
//     streaming) is the precursor; SSH streaming would follow.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RemoteRuntime {
    pub slug: String,
    pub host: String,
    pub port: i64,
    pub ssh_user: Option<String>,
    pub key_path: Option<String>,
    /// The *base* runtime this remote runs (claude / codex / gemini /
    /// hermes / openclaw). Drives prompt-argument shape so we don't
    /// hardcode `--print` for codex.
    pub runtime: String,
    /// Absolute path or PATH-resolvable name of the binary on the
    /// remote. Defaults match the local resolution table.
    pub binary_path: String,
    /// Optional extra args appended verbatim, e.g. "--no-update-check".
    pub extra_args: Option<String>,
    pub created_at: String,
}

/// Look up a remote by its user-given slug. Returns None when no row
/// matches — callers should treat that as "fall through to local
/// dispatch / api-provider dispatch".
pub fn lookup(conn: &Connection, slug: &str) -> Result<Option<RemoteRuntime>> {
    let row = conn
        .query_row(
            "SELECT slug, host, port, ssh_user, key_path, runtime, binary_path, extra_args, created_at
               FROM remote_runtimes WHERE slug = ?1",
            [slug],
            |r| {
                Ok(RemoteRuntime {
                    slug: r.get(0)?,
                    host: r.get(1)?,
                    port: r.get(2)?,
                    ssh_user: r.get(3)?,
                    key_path: r.get(4)?,
                    runtime: r.get(5)?,
                    binary_path: r.get(6)?,
                    extra_args: r.get(7)?,
                    created_at: r.get(8)?,
                })
            },
        )
        .ok();
    Ok(row)
}

pub fn list(conn: &Connection) -> Result<Vec<RemoteRuntime>> {
    let mut stmt = conn.prepare(
        "SELECT slug, host, port, ssh_user, key_path, runtime, binary_path, extra_args, created_at
           FROM remote_runtimes
          ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(RemoteRuntime {
                slug: r.get(0)?,
                host: r.get(1)?,
                port: r.get(2)?,
                ssh_user: r.get(3)?,
                key_path: r.get(4)?,
                runtime: r.get(5)?,
                binary_path: r.get(6)?,
                extra_args: r.get(7)?,
                created_at: r.get(8)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn insert(
    conn: &Connection,
    slug: &str,
    host: &str,
    port: i64,
    ssh_user: Option<&str>,
    key_path: Option<&str>,
    runtime: &str,
    binary_path: &str,
    extra_args: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO remote_runtimes (slug, host, port, ssh_user, key_path, runtime, binary_path, extra_args, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![slug, host, port, ssh_user, key_path, runtime, binary_path, extra_args, now],
    )
    .with_context(|| format!("INSERT remote_runtimes slug={}", slug))?;
    Ok(())
}

pub fn delete(conn: &Connection, slug: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM remote_runtimes WHERE slug = ?1", [slug])?)
}

/// Build the args-vector for the *remote* binary invocation. Mirrors
/// the local dispatch.rs runtime-specific argument shaping so users
/// don't have to think about it. Returns the per-runtime fragment;
/// callers wrap it in `<binary_path> <fragment>` and pass that as
/// the SSH remote command.
fn remote_command_string(remote: &RemoteRuntime, prompt: &str, model: Option<&str>) -> String {
    // Shell-quote the prompt for safety. Single-quote, escape any
    // embedded single quotes by ending+escape+re-opening: `it's` →
    // `'it'\''s'`. This is the same pattern the desktop's openclaw
    // SSH path uses and is the safe-by-default approach for arbitrary
    // user input traveling over `ssh user@host '<cmd>'`.
    let quoted = format!("'{}'", prompt.replace('\'', "'\\''"));
    let model_quoted = model.map(|m| format!("'{}'", m.replace('\'', "'\\''")));

    let runtime_args = match remote.runtime.as_str() {
        "claude" => match &model_quoted {
            Some(m) => format!("--print --model {} {}", m, quoted),
            None => format!("--print {}", quoted),
        },
        "codex" => match &model_quoted {
            Some(m) => format!("exec --skip-git-repo-check --model {} {}", m, quoted),
            None => format!("exec --skip-git-repo-check {}", quoted),
        },
        "gemini" => match &model_quoted {
            Some(m) => format!("-p {} -m {}", quoted, m),
            None => format!("-p {}", quoted),
        },
        "hermes" => format!("--execute {}", quoted),
        "openclaw" => format!("exec {}", quoted),
        // Fall back to "binary prompt" — safe for runtimes that take
        // the prompt as the trailing positional arg. Won't be right
        // for everything but a reasonable default until we add the
        // shape to this table.
        _ => quoted,
    };

    // v2.4.8 audit M4 — shell-quote binary_path and extra_args.
    // Pre-2.4.8 these were concatenated verbatim, so a malicious
    // row in remote_runtimes (imported config, marketplace,
    // support paste) could inject shell metachars onto the remote
    // host. The prompt + model are already quoted above; this
    // closes the symmetry gap.
    //
    // extra_args supports multiple whitespace-separated args
    // (e.g. "--no-update-check --verbose"); we split + quote each.
    let bin_q = shell_quote(&remote.binary_path);
    let extra = remote
        .extra_args
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            let parts: Vec<String> = s.split_whitespace().map(shell_quote).collect();
            if parts.is_empty() {
                String::new()
            } else {
                format!(" {}", parts.join(" "))
            }
        })
        .unwrap_or_default();

    format!("{}{} {}", bin_q, extra, runtime_args)
}

/// Single-quote escape: `it's` → `'it'\''s'`. Same shape used for
/// the prompt + model values above; this helper centralizes it.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Execute a dispatch against a remote runtime over SSH. Returns the
/// raw `Output` plus, if BYOK was applied, the key that was inlined
/// into the remote command — caller passes it to redact_byok_secrets
/// when persisting stderr so an auth-failure echo can't leak it into
/// execution_logs.error_message.
pub fn exec(
    remote: &RemoteRuntime,
    prompt: &str,
    model: Option<&str>,
) -> Result<(std::process::Output, Option<String>)> {
    let remote_cmd = remote_command_string(remote, prompt, model);

    // BYOK over SSH: rather than rely on the remote sshd's AcceptEnv
    // config (which most users won't configure), inline the env var
    // as a command prefix. SSH transports it over the encrypted
    // channel; the remote shell parses `KEY=val cmd args` and the
    // subprocess inherits the env var for that one invocation.
    //
    // Tradeoffs vs SendEnv:
    //   + works on any standard sshd without server-side config
    //   - the key briefly appears in the remote ps listing while the
    //     subprocess is starting (until the shell consumes the
    //     prefix). Brief, but real — comparable to SendEnv's
    //     exposure surface, less than appending it to a config file
    //     on the remote host
    //   - HISTFILE-style remote shell history could log it; users
    //     dispatching to a host with that audit on should prefer
    //     SendEnv + AcceptEnv (not auto-configured here)
    let db_path = crate::db::default_db_path();
    let (final_cmd, applied_key): (String, Option<String>) =
        match crate::byok::byok_env_value(&db_path, &remote.runtime) {
            Some((env_var, key)) => {
                // Defensive single-quote escape on the key. API keys
                // shouldn't contain quotes, but a corrupted entry
                // shouldn't be able to break shell parsing.
                let key_quoted = format!("'{}'", key.replace('\'', "'\\''"));
                (
                    format!("{}={} {}", env_var, key_quoted, remote_cmd),
                    Some(key),
                )
            }
            None => (remote_cmd, None),
        };

    let target = match &remote.ssh_user {
        Some(u) => format!("{}@{}", u, remote.host),
        None => remote.host.clone(),
    };

    let mut cmd = Command::new("ssh");
    if let Some(k) = &remote.key_path {
        cmd.args(["-i", k]);
    }
    // BatchMode=yes: never prompt for a password or passphrase.
    // Forces a clean failure when the key isn't loaded, instead of
    // hanging an ATO dispatch waiting on tty input the CLI can't
    // provide.
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=15",
        "-p",
        &remote.port.to_string(),
        &target,
        &final_cmd,
    ]);

    let output = cmd
        .output()
        .with_context(|| format!("Failed to spawn ssh for remote runtime '{}'", remote.slug))?;
    Ok((output, applied_key))
}

/// Open a connection at the standard db path. Used by callers that
/// want a one-liner lookup without managing their own conn.
pub fn lookup_in_db(db_path: &PathBuf, slug: &str) -> Result<Option<RemoteRuntime>> {
    let conn = crate::db::open_readonly(db_path)?;
    lookup(&conn, slug)
}
