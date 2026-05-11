// ato — the local-first CLI for ATO.
//
// Talks to the same SQLite database (~/.ato/local.db) the desktop GUI
// reads/writes. Designed to be driven by humans AND coding agents:
// every meaningful operation outputs JSON to stdout by default
// (parseable), with a --human flag that switches to a readable
// terminal-friendly view.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;
mod db;
mod output;
mod runtime;

#[derive(Parser, Debug)]
#[command(
    name = "ato",
    version,
    about = "Local-first CLI for ATO — the developer-workflow ops platform for multi-runtime AI agents",
    long_about = "ATO CLI. Read AGENTS.md in the repo root for the full command surface, MCP equivalents, and recipes.\n\nAll commands output JSON to stdout by default. Pass --human for readable formatting.",
)]
struct Cli {
    /// Output format: JSON by default (machine-readable), --human for terminal-friendly
    #[arg(long, global = true)]
    human: bool,

    /// Suppress non-essential output (errors still print to stderr)
    #[arg(long, global = true)]
    quiet: bool,

    /// Override the SQLite DB path (default: ~/.ato/local.db)
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Inspect recent dispatches (executions of an agent / runtime)
    Dispatches {
        #[command(subcommand)]
        sub: DispatchesSub,
    },
    /// Active and historical runs
    Runs {
        #[command(subcommand)]
        sub: RunsSub,
    },
    /// Configuration change history (model swaps, prompt edits)
    #[command(name = "config-changes")]
    ConfigChanges {
        #[command(subcommand)]
        sub: ConfigChangesSub,
    },
    /// File attribution for a specific run (which files the agent touched)
    #[command(name = "files-touched")]
    FilesTouched {
        /// Run ID (the execution_logs row id, or the cloud trace ID)
        id: String,
    },
    /// Replay history and replay-jobs lookup
    Replays {
        #[command(subcommand)]
        sub: ReplaysSub,
    },
    /// Dispatch a prompt to a runtime
    Dispatch {
        /// Runtime: claude, codex, gemini, openclaw, hermes
        runtime: String,
        /// The prompt text
        prompt: String,
        /// Override the model (per-runtime: --model claude-sonnet-4-6, etc.)
        #[arg(long)]
        model: Option<String>,
        /// Optional agent slug — for labeling only in this Phase 1 cut
        #[arg(long)]
        agent: Option<String>,
    },
    /// Replay an existing dispatch against a different runtime/model
    Replay {
        #[command(subcommand)]
        sub: ReplaySub,
    },
    /// Compare two runs side-by-side (by id or cloud trace ID)
    Compare {
        /// First run ID
        a: String,
        /// Second run ID
        b: String,
    },
    /// Author skills (Phase 1 ships only "draft from replay")
    Skills {
        #[command(subcommand)]
        sub: SkillsSub,
    },
    /// Terminate a running dispatch
    Kill {
        /// Run ID to kill (must be currently in live_runs)
        run_id: String,
    },
    /// Manage agents (create / update minimal records — full authoring lives in the GUI)
    Agents {
        #[command(subcommand)]
        sub: AgentsSub,
    },
}

#[derive(Subcommand, Debug)]
enum DispatchesSub {
    /// Recent dispatches across all runtimes (default: last 20)
    Recent {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        runtime: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum RunsSub {
    /// Currently active runs (in-flight dispatches)
    Live,
    /// Get a single run by ID
    Get {
        /// Run / execution_logs ID
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigChangesSub {
    /// List config changes for an agent
    List {
        /// Agent slug (required — config changes are per-agent)
        #[arg(long)]
        agent: String,
        /// How far back to look (e.g. 7d, 24h, 30d). Default: 7d.
        #[arg(long, default_value = "7d")]
        since: String,
    },
}

#[derive(Subcommand, Debug)]
enum ReplaysSub {
    /// List replays for a given cloud trace ID
    #[command(name = "for-trace")]
    ForTrace {
        /// Cloud trace ID
        trace_id: String,
    },
}

#[derive(Subcommand, Debug)]
enum ReplaySub {
    /// Start a replay (synchronous — waits for the dispatch to finish)
    Start {
        /// Source trace ID (cloud_trace_id) or execution_logs ID
        source_id: String,
        /// Target runtime to replay against
        #[arg(long)]
        runtime: String,
        /// Override the target model
        #[arg(long)]
        model: Option<String>,
    },
    /// Get a replay job by ID (use --wait to poll until terminal)
    Get {
        /// Replay job ID
        job_id: String,
        /// Block until the replay reaches done/failed/cancelled
        #[arg(long)]
        wait: bool,
    },
}

#[derive(Subcommand, Debug)]
enum SkillsSub {
    /// Draft a SKILL.md from a successful replay
    Draft {
        /// Replay job ID to derive the skill from
        #[arg(long = "from-replay")]
        from_replay: String,
        /// Output path; defaults to ~/.<target-runtime>/skills/<slug>/SKILL.md
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum AgentsSub {
    /// Create a new agent record
    Create {
        /// Unique slug (per-runtime)
        #[arg(long)]
        slug: String,
        /// Runtime: claude, codex, gemini, openclaw, hermes, ollama
        #[arg(long)]
        runtime: String,
        /// Display name (defaults to slug)
        #[arg(long = "display-name")]
        display_name: Option<String>,
        /// Description / one-line summary of what the agent does
        #[arg(long)]
        description: Option<String>,
        /// Model override (e.g. claude-sonnet-4-6)
        #[arg(long)]
        model: Option<String>,
        /// System prompt
        #[arg(long = "system-prompt")]
        system_prompt: Option<String>,
        /// Optional project ID to scope the agent to
        #[arg(long = "project-id")]
        project_id: Option<String>,
    },
    /// Update an existing agent's editable fields
    Update {
        /// Slug of the agent to update
        slug: String,
        /// Disambiguate when the same slug exists on multiple runtimes
        #[arg(long)]
        runtime: Option<String>,
        /// New model
        #[arg(long)]
        model: Option<String>,
        /// New system prompt
        #[arg(long = "system-prompt")]
        system_prompt: Option<String>,
        /// New display name
        #[arg(long = "display-name")]
        display_name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let db_path = cli.db.clone().unwrap_or_else(db::default_db_path);

    // Open read-only for read-paths; write commands re-open with
    // write privileges internally.
    let ro_conn = || -> Result<rusqlite::Connection> {
        db::open_readonly(&db_path)
            .with_context(|| format!("Could not open ATO database at {}", db_path.display()))
    };

    let opts = output::Opts {
        human: cli.human,
        quiet: cli.quiet,
    };

    match cli.command {
        Commands::Dispatches { sub } => match sub {
            DispatchesSub::Recent {
                limit,
                runtime,
                status,
            } => commands::dispatches::recent(&ro_conn()?, limit, runtime, status, &opts),
        },
        Commands::Runs { sub } => match sub {
            RunsSub::Live => commands::runs::live(&ro_conn()?, &opts),
            RunsSub::Get { id } => commands::runs::get(&ro_conn()?, &id, &opts),
        },
        Commands::ConfigChanges { sub } => match sub {
            ConfigChangesSub::List { agent, since } => {
                commands::config_changes::list(&ro_conn()?, &agent, &since, &opts)
            }
        },
        Commands::FilesTouched { id } => commands::files_touched::run(&ro_conn()?, &id, &opts),
        Commands::Replays { sub } => match sub {
            ReplaysSub::ForTrace { trace_id } => {
                commands::replays::for_trace(&ro_conn()?, &trace_id, &opts)
            }
        },
        Commands::Dispatch {
            runtime,
            prompt,
            model,
            agent,
        } => commands::dispatch::run(&runtime, &prompt, model, agent, &db_path, &opts),
        Commands::Replay { sub } => match sub {
            ReplaySub::Start {
                source_id,
                runtime,
                model,
            } => commands::replay::start(&source_id, &runtime, model, &db_path, &opts),
            ReplaySub::Get { job_id, wait } => {
                commands::replay::get(&job_id, wait, &db_path, &opts)
            }
        },
        Commands::Compare { a, b } => commands::compare::run(&ro_conn()?, &a, &b, &opts),
        Commands::Skills { sub } => match sub {
            SkillsSub::Draft { from_replay, out } => {
                let conn = db::open_readwrite(&db_path)?;
                commands::skills::draft_from_replay(&conn, &from_replay, out, &opts)
            }
        },
        Commands::Kill { run_id } => commands::kill::run(&ro_conn()?, &run_id, &opts),
        Commands::Agents { sub } => {
            let conn = db::open_readwrite(&db_path)?;
            match sub {
                AgentsSub::Create {
                    slug,
                    runtime,
                    display_name,
                    description,
                    model,
                    system_prompt,
                    project_id,
                } => commands::agents::create(
                    &conn,
                    &slug,
                    &runtime,
                    display_name,
                    description,
                    model,
                    system_prompt,
                    project_id,
                    &opts,
                ),
                AgentsSub::Update {
                    slug,
                    runtime,
                    model,
                    system_prompt,
                    display_name,
                    description,
                } => commands::agents::update(
                    &conn,
                    &slug,
                    runtime,
                    model,
                    system_prompt,
                    display_name,
                    description,
                    &opts,
                ),
            }
        }
    }
}
